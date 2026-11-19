"""Files kept with a project: FMUs, AI models, measured data (STD-02).

The metric: a project with an FMU and an ONNX file, saved on one machine,
opens and runs unchanged on another. Here "another machine" is a second,
empty projects folder the bundle is imported into.
"""
import hashlib
import io
import json
import zipfile
from pathlib import Path

import pytest
from fastapi.testclient import TestClient

from app import attachments, security, storage, trust
from app.main import app
from app.paths import EXAMPLES_DIR

client = TestClient(app)

# stand-ins with the bytes such files start with; nothing here runs them
FMU = b"PK\x03\x04" + b"modelDescription.xml" + bytes(range(256)) * 40
ONNX = b"\x08\x07\x12\x07pytorch" + bytes(reversed(range(256))) * 30
CSV = b"t,v\n0,0\n1,2.5\n"


@pytest.fixture(autouse=True)
def projects(tmp_path, monkeypatch):
    folder = tmp_path / "machine-a"
    monkeypatch.setenv("LIGHTSIM_PROJECTS_DIR", str(folder))
    monkeypatch.setenv(security.DEV_PATHS_ENV, "1")
    return folder


def _example(pid: str) -> dict:
    return {**json.loads((EXAMPLES_DIR / "bev-car.json").read_text(encoding="utf-8")), "id": pid}


def _attach(pid: str, name: str, data: bytes) -> dict:
    res = client.post(f"/api/projects/{pid}/attachments", params={"name": name}, content=data)
    assert res.status_code == 200, res.text
    return res.json()


def _ref(info: dict) -> dict:
    return {k: info[k] for k in ("path", "sha256", "bytes")}


def test_attach_list_read_and_remove(projects):
    info = _attach("car", "motor.fmu", FMU)
    assert info == {"path": "resources/motor.fmu", "name": "motor.fmu", "bytes": len(FMU),
                    "sha256": hashlib.sha256(FMU).hexdigest(), "kind": "fmu"}
    assert (projects / "resources" / "car" / "motor.fmu").read_bytes() == FMU
    _attach("car", "drive.csv", CSV)
    assert [f["name"] for f in client.get("/api/projects/car/attachments").json()] == [
        "drive.csv", "motor.fmu"]
    assert client.get("/api/projects/car/attachments/motor.fmu").content == FMU
    assert attachments.read("car", "resources/drive.csv") == CSV
    assert attachments.path_of("car", "resources/motor.fmu").is_file()
    assert client.delete("/api/projects/car/attachments/drive.csv").status_code == 200
    assert client.get("/api/projects/car/attachments/drive.csv").status_code == 404


def test_names_are_cleaned_and_never_overwrite_another_file():
    assert _attach("car", "../../etc/passwd", CSV)["name"] == "passwd"
    assert _attach("car", "C:\\models\\my motor?.fmu", FMU)["name"] == "my motor_.fmu"
    assert _attach("car", ".hidden", CSV)["name"] == "hidden"
    assert _attach("car", "con.txt", CSV)["name"] == "file-con.txt"
    # the same bytes again: the same file; other bytes: a new name
    assert _attach("car", "my motor?.fmu", FMU)["name"] == "my motor_.fmu"
    assert _attach("car", "my motor?.fmu", ONNX)["name"] == "my motor_-2.fmu"
    for bad in ("..", ".gitignore", "a/b", "a\\b", ""):
        with pytest.raises(ValueError):
            attachments.path_of("car", f"resources/{bad}")
    assert client.get("/api/projects/car/attachments/..%2Fx").status_code in (400, 404)


def test_data_checks_find_missing_changed_and_unattached_files(projects):
    project = _example("car")
    fmu = _attach("car", "motor.fmu", FMU)
    csv = _attach("car", "drive.csv", CSV)
    project["attachments"] = [_ref(fmu), _ref(csv)]
    el = project["systems"][0]["elements"][0]
    el["parameterOverrides"]["model_file"] = "resources/motor.fmu"
    ok = client.post("/api/validate", json={"project": project}).json()
    assert not [c for c in ok if "attached" in c["text"]]
    (projects / "resources" / "car" / "drive.csv").write_bytes(CSV + b"2,3\n")
    (projects / "resources" / "car" / "motor.fmu").unlink()
    el["parameterOverrides"]["data_file"] = "resources/other.csv"
    checks = [c for c in client.post("/api/validate", json={"project": project}).json()
              if "attached" in c["text"]]
    by_text = {c["text"].split("'")[1] if c["text"].startswith("The") else c["text"]: c for c in checks}
    assert by_text["resources/motor.fmu"]["level"] == "error", "a part needs it"
    assert by_text["resources/motor.fmu"]["elementIds"] == [el["id"]]
    assert by_text["resources/drive.csv"]["level"] == "warning"
    assert "changed since it was attached" in by_text["resources/drive.csv"]["text"]
    unattached = [c for c in checks if "not attached" in c["text"]]
    assert len(unattached) == 1 and unattached[0]["level"] == "error"
    # an error stops the run before it starts
    res = client.post("/api/simulate", json={"project": project, "caseId": project["cases"][0]["id"]})
    assert res.json()["status"] == "failed"


def test_a_project_with_an_fmu_and_an_onnx_file_runs_unchanged_on_another_machine(
        tmp_path, monkeypatch):
    """STD-02's metric, as a round trip through a bundle."""
    project = _example("fmu-car")
    project["attachments"] = [_ref(_attach("fmu-car", "motor.fmu", FMU)),
                              _ref(_attach("fmu-car", "policy.onnx", ONNX))]
    assert client.put("/api/projects/fmu-car", json=project).status_code == 200
    saved = client.get("/api/projects/fmu-car").json()
    saved.pop("revision")
    bundle = client.post("/api/bundle", json={"project": saved})
    assert bundle.status_code == 200 and bundle.headers["content-type"] == "application/zip"
    with zipfile.ZipFile(io.BytesIO(bundle.content)) as z:
        assert sorted(z.namelist()) == ["project.json", "resources/motor.fmu", "resources/policy.onnx"]

    # machine B: an empty projects folder
    monkeypatch.setenv("LIGHTSIM_PROJECTS_DIR", str(tmp_path / "machine-b"))
    res = client.post("/api/bundle/import", content=bundle.content)
    assert res.status_code == 200, res.text
    opened = res.json()["project"]
    assert opened["id"] == "fmu-car"
    assert opened["attachments"] == saved["attachments"]
    files = {f["name"]: f for f in client.get("/api/projects/fmu-car/attachments").json()}
    assert files["motor.fmu"]["sha256"] == hashlib.sha256(FMU).hexdigest()
    assert files["policy.onnx"]["sha256"] == hashlib.sha256(ONNX).hexdigest()
    assert files["policy.onnx"]["kind"] == "onnx"
    assert client.put("/api/projects/fmu-car", json=opened).status_code == 200
    checks = client.post("/api/validate", json={"project": opened}).json()
    assert not [c for c in checks if "attached" in c["text"]]
    case = next(c for c in opened["cases"] if c.get("kind", "cycle") == "cycle")
    case = {**case, "duration": 30.0}
    opened["cases"] = [case]
    run = client.post("/api/simulate", json={"project": opened, "caseId": case["id"]}).json()
    assert run["status"] in ("success", "warning"), run["messages"]


def test_an_imported_bundle_never_mixes_with_a_project_of_the_same_id(projects):
    assert client.put("/api/projects/car", json=_example("car")).status_code == 200
    _attach("car", "motor.fmu", ONNX)  # this project's own, different file
    project = {**_example("car"), "attachments": [
        {"path": "resources/motor.fmu", "sha256": hashlib.sha256(FMU).hexdigest(), "bytes": len(FMU)}]}
    project["systems"][0]["elements"][0]["parameterOverrides"]["model_file"] = "resources/motor.fmu"
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w") as z:
        z.writestr("project.json", json.dumps(project))
        z.writestr("resources/motor.fmu", FMU)
        z.writestr("../evil.txt", b"x")
        z.writestr("resources/../../evil.txt", b"x")
    res = client.post("/api/bundle/import", content=buf.getvalue())
    assert res.status_code == 200, res.text
    got = res.json()["project"]
    assert got["id"] != "car"
    assert attachments.read(got["id"], "resources/motor.fmu") == FMU
    assert attachments.read("car", "resources/motor.fmu") == ONNX
    assert not list(Path(projects).rglob("evil.txt"))
    assert client.post("/api/bundle/import", content=b"not a zip").status_code == 400


@pytest.mark.parametrize("bad_id", ["my project", "team/car", "../car", "x" * 200, ""])
def test_a_bundle_whose_project_id_no_file_name_can_carry_gets_a_new_one(projects, bad_id):
    project = {**_example(bad_id), "attachments": [
        {"path": "resources/data.csv", "sha256": hashlib.sha256(CSV).hexdigest(), "bytes": len(CSV)}]}
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w") as z:
        z.writestr("project.json", json.dumps(project))
        z.writestr("resources/data.csv", CSV)
    res = client.post("/api/bundle/import", content=buf.getvalue())
    assert res.status_code == 200, res.text
    got = res.json()["project"]["id"]
    assert storage.SAFE_ID.fullmatch(got) and got != bad_id
    if bad_id == "my project":
        assert got.startswith("my-project-")
    assert attachments.read(got, "resources/data.csv") == CSV


def test_save_as_takes_the_attached_files_along(tmp_path):
    project = {**_example("car"), "attachments": [_ref(_attach("car", "motor.fmu", FMU))]}
    assert client.put("/api/projects/car", json=project).status_code == 200
    target = tmp_path / "repo" / "car.lightsim"
    target.parent.mkdir()
    res = client.post("/api/files/save-as", json={"path": str(target), "projectId": "car"})
    new_id = res.json()["id"]
    assert client.put(f"/api/projects/{new_id}", json={**project, "id": new_id}).status_code == 200
    assert (tmp_path / "repo" / "car.lightsim-resources" / "motor.fmu").read_bytes() == FMU
    assert storage.location(new_id).resources == tmp_path / "repo" / "car.lightsim-resources"
    assert not (tmp_path / "repo" / "car.lightsim-resources" / ".gitignore").exists(), \
        "attached files are part of the model: git keeps them"


def test_trust_is_remembered_by_fingerprint():
    fp = hashlib.sha256(b"script + fmu hashes").hexdigest()
    assert client.get(f"/api/trust/{fp}").json() == {"trusted": False}
    assert client.post(f"/api/trust/{fp}").json() == {"trusted": True}
    assert client.get(f"/api/trust/{fp}").json() == {"trusted": True}
    assert trust.is_trusted(fp)
    assert client.get("/api/trust/not-hex").status_code == 400
