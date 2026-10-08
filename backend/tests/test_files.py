"""Projects as .lightsim files in any folder (PLT-33), and sweeps that leave
the model file alone (PLT-34).

Only the desktop shell names file paths (app/security.py); the tests act as
the shell by setting LIGHTSIM_DEV_FILE_PATHS, or LIGHTSIM_SHELL_TOKEN and its
header.
"""
import json
from pathlib import Path

import pytest
from fastapi.testclient import TestClient

from app import files, run_store, security
from app.main import app
from app.paths import EXAMPLES_DIR

client = TestClient(app)


@pytest.fixture(autouse=True)
def projects(tmp_path, monkeypatch):
    folder = tmp_path / "app-data" / "projects"
    monkeypatch.setenv("LIGHTSIM_PROJECTS_DIR", str(folder))
    monkeypatch.setenv(security.DEV_PATHS_ENV, "1")
    monkeypatch.delenv(security.SHELL_TOKEN_ENV, raising=False)
    return folder


@pytest.fixture
def repo(tmp_path) -> Path:
    """A folder of the user's, outside the projects folder (a git repository)."""
    folder = tmp_path / "team-repo" / "models"
    folder.mkdir(parents=True)
    return folder


def _example(name: str = "bev-car") -> dict:
    return json.loads((EXAMPLES_DIR / f"{name}.json").read_text(encoding="utf-8"))


def _write(path: Path, project: dict) -> Path:
    path.write_text(json.dumps(project, indent=2), encoding="utf-8")
    return path


def _open(path: Path) -> dict:
    res = client.post("/api/files/open", json={"path": str(path)})
    assert res.status_code == 200, res.text
    return res.json()


def _save(project: dict, revision: str | None = None) -> dict:
    headers = {"If-Match": f'"{revision}"'} if revision else {}
    res = client.put(f"/api/projects/{project['id']}", json=project, headers=headers)
    assert res.status_code == 200, res.text
    return res.json()


def _run(run_id: str, case_id: str = "case-city") -> dict:
    return {"id": run_id, "caseId": case_id, "caseName": "City", "startedAt": 1_700_000_000_000,
            "status": "success",
            "result": {"caseId": case_id, "status": "success", "messages": [], "channels": [],
                       "summary": [{"label": "Range", "value": 412.0, "unit": "km"}]}}


def test_a_file_in_any_folder_opens_saves_and_keeps_its_runs_beside_it(repo, projects):
    path = _write(repo / "car.lightsim", {**_example(), "id": "team-car"})
    opened = _open(path)
    assert opened == {"id": "team-car", "path": str(path), "name": "Battery Electric Car"}
    body = client.get("/api/projects/team-car").json()
    assert body["filePath"] == str(path)
    rev = body.pop("revision")
    body.pop("filePath")
    body.pop("upgradedFrom")
    body["name"] = "Team car"
    _save(body, rev)
    saved = json.loads(path.read_text(encoding="utf-8"))
    assert saved["name"] == "Team car" and "filePath" not in saved and saved["id"] == "team-car"
    assert not (projects / "team-car.json").exists(), "nothing goes to the projects folder"
    assert not (repo / "car.lightsim.bak").exists(), "no .bak beside the user's file"
    # runs and backups sit beside the file, each folder ignored by git
    assert client.put("/api/projects/team-car/runs/run-1", json=_run("run-1")).status_code == 200
    assert (repo / "car.lightsim-runs" / "run-1.json.gz").is_file()
    assert (repo / "car.lightsim-runs" / ".gitignore").read_text().splitlines()[-1] == "*"
    assert (repo / "car.lightsim-backups" / ".gitignore").is_file()
    assert len(client.get("/api/projects/team-car/backups").json()) == 1


def test_a_reopened_file_finds_its_runs_and_is_on_recent_files(repo):
    path = _write(repo / "car.lightsim", {**_example(), "id": "team-car"})
    _open(path)
    client.put("/api/projects/team-car/runs/run-1", json=_run("run-1"))
    # an app restart: the engine reads the list of files again from disk
    recent = client.get("/api/files").json()
    assert [(f["id"], f["path"], f["exists"]) for f in recent] == [("team-car", str(path), True)]
    assert recent[0]["name"] == "Battery Electric Car" and recent[0]["elements"] > 0
    assert _open(path)["id"] == "team-car"
    assert [r["id"] for r in client.get("/api/projects/team-car/runs").json()] == ["run-1"]
    # taken off Recent files, the file itself stays
    assert client.delete("/api/files/team-car").status_code == 200
    assert client.get("/api/files").json() == [] and path.is_file()
    assert client.delete("/api/files/team-car").status_code == 404
    # opened again, it is back on the list under the same id, with its runs
    assert _open(path)["id"] == "team-car"
    assert [f["id"] for f in client.get("/api/files").json()] == ["team-car"]
    assert [r["id"] for r in client.get("/api/projects/team-car/runs").json()] == ["run-1"]


def test_taking_the_open_file_off_recent_files_keeps_saving_to_it(repo, projects):
    path = _write(repo / "car.lightsim", {**_example(), "id": "team-car"})
    _open(path)
    body = client.get("/api/projects/team-car").json()
    rev = body.pop("revision")
    for key in ("filePath", "upgradedFrom"):
        body.pop(key, None)
    assert client.delete("/api/files/team-car").status_code == 200
    # the window that has it open: no "deleted on disk", and Save writes the file
    assert client.get("/api/projects/team-car/revision").json() == {"revision": rev}
    body["name"] = "Team car"
    _save(body, rev)
    assert json.loads(path.read_text(encoding="utf-8"))["name"] == "Team car"
    assert not (projects / "team-car.json").exists()
    assert client.get("/api/files").json() == []
    # an app restart reads the hidden entry from disk too
    assert files.lookup("team-car") == path and files.recent() == []


def test_a_copy_whose_id_is_taken_gets_its_own(repo, projects):
    original = _write(repo / "car.lightsim", {**_example(), "id": "team-car"})
    copy = _write(repo / "car-copy.lightsim", {**_example(), "id": "team-car"})
    assert _open(original)["id"] == "team-car"
    other = _open(copy)["id"]
    assert other != "team-car" and other.startswith("team-car-")
    assert client.get(f"/api/projects/{other}").json()["id"] == other
    assert json.loads(copy.read_text())["id"] == "team-car", "the file changes only when saved"
    # a project in the projects folder with the same id also counts
    _save({**_example(), "id": "mine"})
    clash = _write(repo / "mine.lightsim", {**_example(), "id": "mine"})
    assert _open(clash)["id"] != "mine"


def test_only_lightsim_project_files_open(repo, tmp_path):
    bad = [
        ("relative/car.lightsim", 400),
        (str(repo / "missing.lightsim"), 404),
        (str(_write(repo / "car.json", _example())), 400),
        (str(_write(repo / "notes.lightsim", {"hello": "world"})), 400),
    ]
    for path, status in bad:
        res = client.post("/api/files/open", json={"path": path})
        assert res.status_code == status, (path, res.text)
    assert client.get("/api/files").json() == [], "nothing refused is remembered"


def test_only_the_desktop_shell_can_name_paths(repo, monkeypatch):
    path = _write(repo / "car.lightsim", _example())
    monkeypatch.delenv(security.DEV_PATHS_ENV)
    assert client.post("/api/files/open", json={"path": str(path)}).status_code == 403
    assert client.post("/api/files/save-as", json={"path": str(path), "projectId": "x"}).status_code == 403
    monkeypatch.setenv(security.SHELL_TOKEN_ENV, "shell-secret")
    monkeypatch.setenv(security.DEV_PATHS_ENV, "1")  # ignored once the shell has a secret
    assert client.post("/api/files/open", json={"path": str(path)}).status_code == 403
    assert client.post("/api/files/open", json={"path": str(path)},
                       headers={"X-LightSim-Shell": "wrong"}).status_code == 403
    assert client.post("/api/files/open", json={"path": str(path)},
                       headers={"X-LightSim-Shell": "shell-secret"}).status_code == 200


def test_save_as_moves_a_new_projects_runs_and_copies_a_saved_one(repo, projects):
    # a project never saved: its runs (kept in the projects folder) move along
    assert client.put("/api/projects/fresh/runs/run-1", json=_run("run-1")).status_code == 200
    res = client.post("/api/files/save-as", json={"path": str(repo / "fresh.lightsim"), "projectId": "fresh"})
    assert res.status_code == 200, res.text
    assert res.json()["id"] == "fresh" and res.json()["runsMoved"] is True
    _save({**_example(), "id": "fresh"})
    assert (repo / "fresh.lightsim").is_file()
    assert [r["id"] for r in client.get("/api/projects/fresh/runs").json()] == ["run-1"]
    assert not (projects / "runs" / "fresh").exists()
    # a saved project: the new file is a copy with an id of its own
    _save({**_example(), "id": "kept"})
    res = client.post("/api/files/save-as", json={"path": str(repo / "kept.lightsim"), "projectId": "kept"})
    copy_id = res.json()["id"]
    assert copy_id != "kept" and res.json()["runsMoved"] is False
    assert (projects / "kept.json").is_file()
    # a folder that does not exist, or another extension, is refused
    assert client.post("/api/files/save-as", json={"path": str(repo / "no" / "x.lightsim"),
                                                  "projectId": "a"}).status_code == 404
    assert client.post("/api/files/save-as", json={"path": str(repo / "x.json"),
                                                  "projectId": "a"}).status_code == 400


def test_a_change_on_disk_shows_in_the_revision_and_blocks_a_stale_save(repo):
    path = _write(repo / "car.lightsim", {**_example(), "id": "team-car"})
    _open(path)
    body = client.get("/api/projects/team-car").json()
    rev = body.pop("revision")
    assert client.get("/api/projects/team-car/revision").json() == {"revision": rev}
    # a git pull brings a teammate's version
    _write(path, {**_example(), "id": "team-car", "name": "Pulled"})
    pulled = client.get("/api/projects/team-car/revision").json()["revision"]
    assert pulled != rev
    body.pop("filePath")
    body.pop("upgradedFrom")
    res = client.put("/api/projects/team-car", json=body, headers={"If-Match": f'"{rev}"'})
    assert res.status_code == 409
    assert client.get("/api/projects/team-car").json()["name"] == "Pulled"
    path.unlink()
    assert client.get("/api/projects/team-car/revision").json() == {"revision": None}


def test_the_app_never_deletes_a_users_file(repo):
    path = _write(repo / "car.lightsim", {**_example(), "id": "team-car"})
    _open(path)
    assert client.delete("/api/projects/team-car").status_code == 400
    assert path.is_file()


def test_running_a_sweep_leaves_the_model_file_byte_identical(repo):
    """PLT-34: a sweep's runs and its results table go to the runs folder."""
    path = _write(repo / "car.lightsim", {**_example(), "id": "team-car"})
    _open(path)
    body = client.get("/api/projects/team-car").json()
    rev = body.pop("revision")
    body.pop("filePath")
    body.pop("upgradedFrom")
    _save(body, rev)
    before = path.read_bytes()
    points = []
    for i, mass in enumerate([1500.0, 1800.0]):
        run = {**_run(f"run-{i}"), "sweepId": "sweep-1", "sweepValue": mass}
        assert client.put(f"/api/projects/team-car/runs/run-{i}", json=run).status_code == 200
        points.append({"values": [mass], "runId": f"run-{i}", "status": "success", "kpis": {"Range": 412.0}})
    study = {"id": "sweep-1", "startedAt": 1_700_000_000_000, "caseId": "case-city",
             "factors": [{"elementId": "el-vehicle", "paramKey": "mass_kg", "values": [1500.0, 1800.0]}],
             "kpis": [{"label": "Range", "unit": "km"}], "points": points}
    assert client.put("/api/projects/team-car/studies/sweep-1", json=study).status_code == 200
    assert path.read_bytes() == before
    assert (repo / "car.lightsim-runs" / "studies" / "sweep-1.json").is_file()
    assert client.get("/api/projects/team-car/studies").json()[0]["points"][1]["values"] == [1800.0]
    # deleting the runs keeps the study; deleting the study keeps the runs
    assert client.delete("/api/projects/team-car/runs").json()["deleted"] == 2
    assert len(client.get("/api/projects/team-car/studies").json()) == 1
    assert client.delete("/api/projects/team-car/studies/sweep-1").status_code == 200
    assert path.read_bytes() == before


def test_deleting_a_project_deletes_its_studies_too(projects):
    _save({**_example(), "id": "gone"})
    run_store.save_study("gone", run_store.Study.model_validate(
        {"id": "s1", "startedAt": 1, "caseId": "c", "factors": []}))
    assert client.delete("/api/projects/gone").status_code == 200
    assert not (projects / "runs" / "gone").exists()


def test_the_list_of_files_survives_damage(projects):
    projects.mkdir(parents=True)
    (projects / ".open-files.json").write_text("{not json")
    assert client.get("/api/files").json() == []
    (projects / ".open-files.json").write_text(json.dumps(
        {"files": [{"id": "../x", "path": "/tmp/a.lightsim"}, {"id": "ok", "path": "relative.lightsim"}, 7]}))
    assert files.recent() == []
