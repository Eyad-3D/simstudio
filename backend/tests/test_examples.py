"""Examples are served read-only from the app itself (CON-10).

They used to be copied into the projects folder once, behind a ``.seeded``
marker, so an install never received new or corrected examples; and in
development the projects folder was ``backend/projects`` itself, so saving an
example rewrote the repo's example files (which the golden tests read). Now
the engine lists and serves the examples from the app's own copy and never
writes it; the UI opens one as an unsaved copy with an id of its own.
"""
from __future__ import annotations

import json
import shutil
from pathlib import Path

import pytest
from fastapi.testclient import TestClient
from helpers import bev_axle

from app import paths, storage
from app.main import app
from app.solver import simulate

client = TestClient(app)
EXAMPLES = {"bev-car", "hybrid-car"}


@pytest.fixture
def user_dir(tmp_path, monkeypatch) -> Path:
    """The projects folder of a new install (not created yet)."""
    target = tmp_path / "userdata" / "projects"
    monkeypatch.setenv("SIMSTUDIO_PROJECTS_DIR", str(target))
    return target


def _snapshot(folder: Path) -> dict[str, tuple]:
    """Every file and folder under `folder`, with its bytes and modification
    time. A folder's time changes when an entry is added or removed in it, so
    this also catches a temp file that was created and renamed away."""
    entries = {
        p.relative_to(folder).as_posix(): (p.read_bytes() if p.is_file() else None,
                                           p.stat().st_mtime_ns)
        for p in folder.rglob("*")
    }
    return {**entries, ".": (None, folder.stat().st_mtime_ns)}


@pytest.fixture(scope="module")
def run() -> dict:
    """A finished run, as the UI stores it."""
    result = simulate(bev_axle(), "case").model_dump()
    return {"id": "run-1", "caseId": "case", "caseName": "Case", "startedAt": 1000,
            "status": result["status"], "result": result}


def _ids(listing: list[dict]) -> set[str]:
    return {p["id"] for p in listing}


def _visible_examples() -> set[str]:
    return {e["id"] for e in client.get("/api/examples").json() if not e["hidden"]}


def _save_copy(example_id: str, copy_id: str, name: str | None = None) -> dict:
    """What the UI does: open the example as a copy with an id of its own and
    save that as a new project."""
    project = client.get(f"/api/examples/{example_id}").json()
    project["id"] = copy_id
    if name:
        project["name"] = name
    res = client.put(f"/api/projects/{copy_id}", json=project, headers={"If-None-Match": "*"})
    assert res.status_code == 200, res.text
    return project


def test_a_new_install_lists_the_examples_and_writes_nothing(user_dir):
    listed = client.get("/api/examples").json()
    assert _ids(listed) == EXAMPLES
    assert all(e["name"] and e["description"] and e["hidden"] is False for e in listed)
    assert client.get("/api/projects").json() == []  # the examples are not the user's
    assert client.get("/api/projects/bev-car").status_code == 404
    assert not user_dir.exists()  # nothing copied in, nothing created


def test_an_example_opens_read_only_and_saves_as_a_new_project(user_dir):
    before = _snapshot(paths.EXAMPLES_DIR)
    res = client.get("/api/examples/bev-car")
    assert res.status_code == 200
    example = res.json()
    assert example["id"] == "bev-car" and example["name"] == "Battery Electric Car"
    assert "revision" not in example and "etag" not in res.headers  # nothing to save over

    _save_copy("bev-car", "bev-car-k3y9x2a", name="My BEV")
    saved = client.get("/api/projects/bev-car-k3y9x2a").json()
    saved["name"] = "My BEV, again"
    revision = saved.pop("revision")
    res = client.put("/api/projects/bev-car-k3y9x2a", json=saved, headers={"If-Match": f'"{revision}"'})
    assert res.status_code == 200, res.text

    assert _ids(client.get("/api/projects").json()) == {"bev-car-k3y9x2a"}
    assert (user_dir / "bev-car-k3y9x2a.json").is_file()
    assert len(client.get("/api/projects/bev-car-k3y9x2a/backups").json()) == 1
    # the example is as it shipped, listed and served unchanged
    assert _snapshot(paths.EXAMPLES_DIR) == before
    assert _visible_examples() == EXAMPLES
    assert client.get("/api/examples/bev-car").json() == example


def test_a_project_saved_under_an_examples_id_goes_to_the_users_folder(user_dir):
    """Such as a project a recovery draft kept from before this change."""
    before = _snapshot(paths.EXAMPLES_DIR)
    project = client.get("/api/examples/hybrid-car").json()
    project["name"] = "Edited"
    assert client.put("/api/projects/hybrid-car", json=project).status_code == 200

    assert json.loads((user_dir / "hybrid-car.json").read_bytes())["name"] == "Edited"
    assert _snapshot(paths.EXAMPLES_DIR) == before
    assert client.get("/api/examples/hybrid-car").json()["name"] == "P2 Hybrid Car"


@pytest.fixture
def seeded_install(user_dir, tmp_path, monkeypatch, run) -> Path:
    """A projects folder an earlier version seeded: a copy of each example and
    the .seeded marker. The user has since edited the BEV copy and run it, and
    deleted the hybrid one. The update ships a new example."""
    user_dir.mkdir(parents=True)
    for name in EXAMPLES:
        shutil.copyfile(paths.EXAMPLES_DIR / f"{name}.json", user_dir / f"{name}.json")
    (user_dir / ".seeded").write_text("SimStudio copied its example projects here on first run.\n")
    old = json.loads((user_dir / "bev-car.json").read_bytes())
    old["name"] = "My old BEV"
    (user_dir / "bev-car.json").write_text(json.dumps(old))
    (user_dir / "hybrid-car.json").unlink()
    assert client.put("/api/projects/bev-car/runs/run-1", json=run).status_code == 200

    bundle = tmp_path / "bundle"
    shutil.copytree(paths.EXAMPLES_DIR, bundle, ignore=shutil.ignore_patterns("runs"))
    new = json.loads((bundle / "bev-car.json").read_bytes())
    new.update(id="new-car", name="New Car")
    (bundle / "new-car.json").write_text(json.dumps(new))
    monkeypatch.setattr(storage, "EXAMPLES_DIR", bundle)
    return user_dir


def test_an_update_brings_the_current_examples_to_a_seeded_install(seeded_install):
    before = _snapshot(seeded_install)

    # the old copy stays the user's project, as they left it, with its runs
    projects = client.get("/api/projects").json()
    assert [(p["id"], p["name"]) for p in projects] == [("bev-car", "My old BEV")]
    assert client.get("/api/projects/bev-car").json()["name"] == "My old BEV"
    assert _ids(client.get("/api/projects/bev-car/runs").json()) == {"run-1"}
    # the example of the same id is listed and served as shipped, and so is the new one
    listed = {e["id"]: e for e in client.get("/api/examples").json()}
    assert set(listed) == EXAMPLES | {"new-car"}
    assert listed["bev-car"]["name"] == "Battery Electric Car" and not listed["bev-car"]["hidden"]
    assert client.get("/api/examples/bev-car").json()["name"] == "Battery Electric Car"
    assert not listed["new-car"]["hidden"]
    # the example the user deleted stays out of the menu, as the marker meant
    assert listed["hybrid-car"]["hidden"]

    # no user file was changed or removed: the only one added lists the hidden example
    after = _snapshot(seeded_install)
    assert set(after) - set(before) == {".hidden-examples"}
    assert {k: v for k, v in after.items() if k in before and k != "."} == {
        k: v for k, v in before.items() if k != "."
    }


def test_restoring_the_examples_of_a_seeded_install_lasts(seeded_install):
    assert "hybrid-car" not in _visible_examples()
    assert client.post("/api/examples/restore").json() == {"restored": ["hybrid-car"]}
    # the marker is still there and the copy still missing: restored for good
    assert (seeded_install / ".seeded").is_file()
    assert not (seeded_install / "hybrid-car.json").exists()
    assert _visible_examples() == EXAMPLES | {"new-car"}
    assert _visible_examples() == EXAMPLES | {"new-car"}


def test_hidden_examples_stay_hidden_until_restored(user_dir):
    assert client.post("/api/examples/bev-car/hide").json() == {"hidden": "bev-car"}
    assert client.post("/api/examples/bev-car/hide").status_code == 200  # already hidden
    assert _visible_examples() == EXAMPLES - {"bev-car"}
    assert client.get("/api/examples/bev-car").status_code == 200  # hidden, not gone
    next_launch = TestClient(app)
    assert next_launch.get("/api/examples").json() == client.get("/api/examples").json()

    assert client.post("/api/examples/restore").json() == {"restored": ["bev-car"]}
    assert _visible_examples() == EXAMPLES
    assert client.post("/api/examples/restore").json() == {"restored": []}
    assert client.get("/api/projects").json() == []  # the list of hidden ones is not a project


@pytest.mark.parametrize("example_id, status", [
    ("no-such-car", 404), ("%2E%2E", 400), ("a" * 300, 400),
])
def test_only_shipped_examples_can_be_read_or_hidden(user_dir, example_id, status):
    assert client.get(f"/api/examples/{example_id}").status_code == status
    assert client.post(f"/api/examples/{example_id}/hide").status_code == status
    assert not user_dir.exists()


def test_a_damaged_list_of_hidden_examples_hides_nothing(user_dir):
    user_dir.mkdir(parents=True)
    for content in ("{not json", '{"hidden": "bev-car"}', '["bev-car"]', '{"hidden": [1, "../x"]}'):
        (user_dir / ".hidden-examples").write_text(content)
        assert _visible_examples() == EXAMPLES


def test_development_saves_outside_the_examples(monkeypatch):
    monkeypatch.delenv("SIMSTUDIO_PROJECTS_DIR", raising=False)
    folder = paths.projects_dir().resolve()
    examples = paths.EXAMPLES_DIR.resolve()
    assert folder != examples and examples not in folder.parents and folder not in examples.parents


def test_development_writes_nothing_to_the_examples(tmp_path, monkeypatch, run):
    """The whole save, backup, run and hide cycle in development (no
    SIMSTUDIO_PROJECTS_DIR) leaves backend/projects exactly as it was."""
    monkeypatch.delenv("SIMSTUDIO_PROJECTS_DIR", raising=False)
    dev = tmp_path / "dev-projects"
    monkeypatch.setattr(paths, "DEV_PROJECTS_DIR", dev)
    before = _snapshot(paths.EXAMPLES_DIR)

    _save_copy("bev-car", "bev-car-dev1234")
    project = client.get("/api/examples/bev-car").json()
    project["name"] = "Saved under the example's id"
    for _ in range(2):  # the second save keeps a backup of the first
        assert client.put("/api/projects/bev-car", json=project).status_code == 200
        project["name"] += "!"
    assert client.put("/api/projects/bev-car/runs/run-1", json=run).status_code == 200
    assert client.post("/api/examples/hybrid-car/hide").status_code == 200
    assert client.post("/api/examples/restore").status_code == 200
    assert client.delete("/api/projects/bev-car").status_code == 200

    assert _snapshot(paths.EXAMPLES_DIR) == before
    assert (dev / "bev-car-dev1234.json").is_file() and (dev / ".backups" / "bev-car").is_dir()
    assert client.get("/api/examples/bev-car").json()["name"] == "Battery Electric Car"


def test_every_example_is_served_under_its_file_name():
    for f in paths.EXAMPLES_DIR.glob("*.json"):
        assert storage.load_example(f.stem).id == f.stem
