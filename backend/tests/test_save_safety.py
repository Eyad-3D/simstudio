"""Crash-proof, conflict-aware saves (PLT-04).

A save must be all-or-nothing (temp file + atomic replace, the previous file
kept as .bak), and a save based on an out-of-date copy — another window saved
first, or another program changed the file — is refused with 409 instead of
silently overwriting.
"""
import json
import os
import subprocess
import sys
import threading
from pathlib import Path

import pytest
from fastapi.testclient import TestClient

from app import storage
from app.main import app
from app.paths import EXAMPLES_DIR

client = TestClient(app)
BACKEND = Path(__file__).parent.parent


@pytest.fixture(autouse=True)
def projects(tmp_path, monkeypatch):
    monkeypatch.setenv("SIMSTUDIO_PROJECTS_DIR", str(tmp_path))
    for name in ("bev-car", "hybrid-car"):
        (tmp_path / f"{name}.json").write_bytes((EXAMPLES_DIR / f"{name}.json").read_bytes())
    return tmp_path


def _load(pid="bev-car"):
    r = client.get(f"/api/projects/{pid}")
    assert r.status_code == 200, r.text
    body = r.json()
    return body, body.pop("revision", None), r.headers.get("etag")


def _put(body, **headers):
    return client.put(f"/api/projects/{body['id']}", json=body, headers=headers)


# ---- revisions and conflicts -----------------------------------------------------

def test_get_returns_a_revision_matching_the_etag():
    _, rev, etag = _load()
    assert rev and etag == f'"{rev}"'
    _, rev2, _ = _load()
    assert rev2 == rev, "an unchanged file keeps its revision"


def test_second_window_cannot_overwrite_the_first(projects):
    a, rev_a, _ = _load()
    b, rev_b, _ = _load()
    a["name"] = "Saved by window A"
    b["name"] = "Saved by window B"
    first = _put(a, **{"If-Match": f'"{rev_a}"'})
    assert first.status_code == 200, first.text
    new_rev = first.json()["revision"]
    assert new_rev != rev_a and first.headers["etag"] == f'"{new_rev}"'

    second = _put(b, **{"If-Match": f'"{rev_b}"'})
    assert second.status_code == 409
    assert "changed on disk" in second.json()["detail"]
    on_disk = json.loads((projects / "bev-car.json").read_text(encoding="utf-8"))
    assert on_disk["name"] == "Saved by window A"


def test_saving_again_with_the_returned_revision_works():
    body, rev, _ = _load()
    for i in range(3):
        body["name"] = f"Edit {i}"
        r = _put(body, **{"If-Match": f'"{rev}"'})
        assert r.status_code == 200, r.text
        rev = r.json()["revision"]
    assert _load()[0]["name"] == "Edit 2"


def test_file_changed_by_another_program_is_a_conflict(projects):
    body, rev, _ = _load("hybrid-car")
    path = projects / "hybrid-car.json"
    disk = json.loads(path.read_text(encoding="utf-8"))
    disk["description"] = "edited in a text editor"
    path.write_text(json.dumps(disk, indent=2), encoding="utf-8")
    body["name"] = "UI edit"
    r = _put(body, **{"If-Match": f'"{rev}"'})
    assert r.status_code == 409
    assert json.loads(path.read_text(encoding="utf-8"))["description"] == "edited in a text editor"


def test_file_deleted_since_loading_is_a_conflict(projects):
    body, rev, _ = _load()
    (projects / "bev-car.json").unlink()
    r = _put(body, **{"If-Match": f'"{rev}"'})
    assert r.status_code == 409
    assert "deleted" in r.json()["detail"]
    assert not (projects / "bev-car.json").exists()


def test_new_project_must_not_replace_an_existing_file(projects):
    body, _, _ = _load()  # e.g. an exported copy imported again
    body["name"] = "Imported copy"
    r = _put(body, **{"If-None-Match": "*"})
    assert r.status_code == 409
    assert "already exists" in r.json()["detail"]
    body["id"] = "brand-new"
    r = _put(body, **{"If-None-Match": "*"})
    assert r.status_code == 200, r.text
    assert (projects / "brand-new.json").exists()


def test_revision_sent_back_in_the_body_is_checked_and_not_stored(projects):
    body, rev, _ = _load()
    body["revision"] = rev  # a client that PUTs back exactly what GET returned
    body["name"] = "Round trip"
    r = _put(body)
    assert r.status_code == 200, r.text
    assert "revision" not in json.loads((projects / "bev-car.json").read_text(encoding="utf-8"))
    body["name"] = "Stale"
    assert _put(body).status_code == 409, "the body's old revision is out of date now"


def test_save_without_precondition_still_overwrites():
    # scripts and older clients that send no revision keep last-writer-wins
    body, _, _ = _load()
    body["name"] = "Unconditional"
    assert _put(body).status_code == 200
    assert _put(body).status_code == 200
    assert _load()[0]["name"] == "Unconditional"


def test_concurrent_saves_from_one_revision_let_exactly_one_win(projects):
    body, rev, _ = _load()
    results = []

    def save(i):
        b = dict(body, name=f"Writer {i}")
        results.append((i, _put(b, **{"If-Match": f'"{rev}"'}).status_code))

    threads = [threading.Thread(target=save, args=(i,)) for i in range(8)]
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    winners = [i for i, code in results if code == 200]
    assert len(winners) == 1 and sorted(c for _, c in results) == [200] + [409] * 7
    assert _load()[0]["name"] == f"Writer {winners[0]}"


# ---- atomic write and backup ---------------------------------------------------------

def test_previous_version_is_kept_as_bak(projects):
    before = (projects / "bev-car.json").read_bytes()
    body, rev, _ = _load()
    body["name"] = "After"
    assert _put(body, **{"If-Match": f'"{rev}"'}).status_code == 200
    assert (projects / "bev-car.json.bak").read_bytes() == before
    names = {p["id"] for p in client.get("/api/projects").json()}
    assert names == {"bev-car", "hybrid-car"}, "backups and temp files are not projects"


def test_failed_replace_leaves_the_file_and_no_debris(projects, monkeypatch):
    before = (projects / "bev-car.json").read_bytes()
    project = storage.load_project("bev-car")
    project.name = "Never written"

    def boom(src, dst):
        raise OSError("simulated crash before the rename")

    monkeypatch.setattr(storage.os, "replace", boom)
    with pytest.raises(OSError):
        storage.save_project(project)
    assert (projects / "bev-car.json").read_bytes() == before
    assert not [p for p in projects.iterdir() if p.name.endswith(".tmp")]


@pytest.mark.skipif(not sys.platform.startswith("linux"), reason="uses RLIMIT_FSIZE")
def test_write_cut_off_midway_never_corrupts_the_project(projects):
    """Run a save in a process whose writes stop 20 kB above the project's
    current size (as when the disk fills up or the power fails mid-write):
    the project must stay loadable."""
    before = (projects / "bev-car.json").read_bytes()
    limit = len(before) + 20_000  # room for the current version, not the new one
    script = f"""
import resource, signal, sys
sys.path.insert(0, {str(BACKEND)!r})
from app import storage
p = storage.load_project("bev-car")
p.description = "x" * {10 * limit}  # the new version is far larger than the limit
signal.signal(signal.SIGXFSZ, signal.SIG_IGN)
resource.setrlimit(resource.RLIMIT_FSIZE, ({limit}, {limit}))
try:
    storage.save_project(p)
except OSError as e:
    print("save failed:", e.errno)
"""
    env = dict(os.environ, SIMSTUDIO_PROJECTS_DIR=str(projects))
    out = subprocess.run([sys.executable, "-c", script], env=env, capture_output=True,
                         text=True, timeout=60)
    assert "save failed" in out.stdout, out.stdout + out.stderr
    assert (projects / "bev-car.json").read_bytes() == before
    storage.load_project("bev-car")  # still valid JSON and a valid project
    assert not [p for p in projects.iterdir() if p.name.endswith(".tmp")]
