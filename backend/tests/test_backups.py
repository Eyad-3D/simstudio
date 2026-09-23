"""Automatic backups of every project (PLT-29).

Each save keeps the version it replaces in ``.backups/<id>/``; the newest 20
are listed (by when that version was saved) and can be read back, and none of
this touches the project file itself or the project list.
"""
import json
import os

import pytest
from fastapi.testclient import TestClient

from app import storage
from app.main import app
from app.paths import EXAMPLES_DIR

client = TestClient(app)


@pytest.fixture(autouse=True)
def projects(tmp_path, monkeypatch):
    monkeypatch.setenv("SIMSTUDIO_PROJECTS_DIR", str(tmp_path))
    for name in ("bev-car", "hybrid-car"):
        (tmp_path / f"{name}.json").write_bytes((EXAMPLES_DIR / f"{name}.json").read_bytes())
    return tmp_path


def _save_as(name: str, pid: str = "bev-car") -> None:
    """Load the project, rename it and save it back, like the UI does."""
    body = client.get(f"/api/projects/{pid}").json()
    rev = body.pop("revision")
    body["name"] = name
    res = client.put(f"/api/projects/{pid}", json=body, headers={"If-Match": f'"{rev}"'})
    assert res.status_code == 200, res.text


def _backups(pid: str = "bev-car") -> list[dict]:
    res = client.get(f"/api/projects/{pid}/backups")
    assert res.status_code == 200, res.text
    return res.json()


def test_after_25_saves_the_last_20_versions_can_be_restored(projects):
    original = json.loads((projects / "bev-car.json").read_text(encoding="utf-8"))
    for i in range(1, 26):
        _save_as(f"Version {i}")
    listed = _backups()
    assert len(listed) == storage.KEEP_BACKUPS == 20
    # newest first: the version each of the last 20 saves replaced
    assert [b["name"] for b in listed] == [f"Version {i}" for i in range(24, 4, -1)]
    assert len(list((projects / ".backups" / "bev-car").glob("*.json"))) == 20
    for b in listed:
        back = client.get(f"/api/projects/bev-car/backups/{b['id']}")
        assert back.status_code == 200, back.text
        assert back.json()["name"] == b["name"]
        assert "revision" not in back.json(), "a backup is not the file on disk"
        assert b["elements"] == sum(len(s["elements"]) for s in original["systems"])
    # reading backups never changes the project file
    assert json.loads((projects / "bev-car.json").read_text(encoding="utf-8"))["name"] == "Version 25"


def test_a_backup_holds_the_replaced_file_byte_for_byte_and_its_save_time(projects):
    path = projects / "bev-car.json"
    before = path.read_bytes()
    os.utime(path, (1_700_000_000, 1_700_000_000))
    _save_as("After")
    [backup] = _backups()
    assert backup["savedAt"] == 1_700_000_000_000
    assert backup["revision"] == storage.revision_of(before)
    assert backup["bytes"] == len(before)
    assert (projects / ".backups" / "bev-car" / f"{backup['id']}.json").read_bytes() == before


def test_a_save_that_changes_nothing_keeps_no_backup(projects):
    _save_as("Same")
    _save_as("Same")  # the file already holds exactly this
    _save_as("Same")
    assert [b["name"] for b in _backups()] == ["Battery Electric Car"]


def test_a_new_project_has_no_backups_until_it_is_saved_over(projects):
    body = client.get("/api/projects/bev-car").json()
    body.pop("revision")
    body["id"] = "brand-new"
    res = client.put("/api/projects/brand-new", json=body, headers={"If-None-Match": "*"})
    assert res.status_code == 200, res.text
    assert _backups("brand-new") == []
    assert _backups("never-saved") == []


def test_backups_are_not_projects(projects):
    for i in range(3):
        _save_as(f"Version {i}")
    names = {p["id"] for p in client.get("/api/projects").json()}
    assert names == {"bev-car", "hybrid-car"}
    assert _backups("hybrid-car") == [], "each project has its own backups"


@pytest.mark.parametrize("backup_id", ["x", ".hidden", "1700000000000-zzzzzzzzzzzzzzzz", "1700000000000-abc"])
def test_backup_ids_that_are_not_backups_are_refused(projects, backup_id):
    res = client.get(f"/api/projects/bev-car/backups/{backup_id}")
    assert res.status_code == 400, res.text


def test_unknown_backup_is_404_and_a_corrupt_one_400(projects):
    _save_as("After")
    [backup] = _backups()
    assert client.get("/api/projects/bev-car/backups/1700000000000-0123456789abcdef").status_code == 404
    (projects / ".backups" / "bev-car" / f"{backup['id']}.json").write_text("{not json")
    assert client.get(f"/api/projects/bev-car/backups/{backup['id']}").status_code == 400
    assert _backups()[0]["name"] is None, "an unreadable backup is still listed"
