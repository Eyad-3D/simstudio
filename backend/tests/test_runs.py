"""Run history on disk: finished runs are stored per project, listed again
after a restart, and can be deleted one by one or all at once."""
from __future__ import annotations

import gzip
import json

import pytest
from fastapi.testclient import TestClient
from helpers import bev_axle

from app import run_store, storage
from app.main import app
from app.solver import simulate

client = TestClient(app)


@pytest.fixture
def projects_dir(tmp_path, monkeypatch):
    target = tmp_path / "userdata" / "projects"
    monkeypatch.setenv("SIMSTUDIO_PROJECTS_DIR", str(target))
    storage._seeded.clear()
    yield target
    storage._seeded.clear()


@pytest.fixture(scope="module")
def result() -> dict:
    return simulate(bev_axle(), "case").model_dump()


def make_run(result: dict, run_id: str, started: int, **extra) -> dict:
    return {
        "id": run_id,
        "caseId": result["caseId"],
        "caseName": "City Cycle",
        "startedAt": started,
        "status": result["status"],
        "result": result,
        **extra,
    }


def put(project_id: str, run: dict) -> dict:
    res = client.put(f"/api/projects/{project_id}/runs/{run['id']}", json=run)
    assert res.status_code == 200, res.text
    return res.json()


def test_a_stored_run_comes_back_unchanged(projects_dir, result):
    run = make_run(result, "run-a1", 1_758_000_000_000)
    assert put("bev-car", run)["stored"] == 1

    back = client.get("/api/projects/bev-car/runs/run-a1")
    assert back.status_code == 200
    assert back.json() == json.loads(json.dumps(run))  # every sample, gaps included

    # one gzip file per run, next to the project files
    path = projects_dir / "runs" / "bev-car" / "run-a1.json.gz"
    assert json.loads(gzip.decompress(path.read_bytes())) == back.json()
    assert path.stat().st_size < len(back.content) / 3


def test_the_index_lists_key_results_without_channel_data(projects_dir, result):
    put("bev-car", make_run(result, "run-old", 1000))
    put("bev-car", make_run(result, "run-sweep", 2000, sweepId="sweep-1", sweepParam="Vehicle Mass",
                            sweepValue=1500, sweepUnit="kg", incomplete="stopped at t = 12 s"))

    runs = client.get("/api/projects/bev-car/runs").json()
    assert [r["id"] for r in runs] == ["run-sweep", "run-old"]  # newest first
    newest = runs[0]
    assert newest["caseName"] == "City Cycle"
    assert newest["status"] == result["status"]
    assert newest["sweepValue"] == 1500 and newest["incomplete"] == "stopped at t = 12 s"
    assert newest["summary"] == result["summary"] and newest["summary"]
    assert newest["bytes"] > 0
    assert "result" not in newest
    assert "incomplete" not in runs[1]  # unset fields stay unset


def test_runs_belong_to_their_project(projects_dir, result):
    put("bev-car", make_run(result, "run-bev", 1000))
    put("hybrid-car", make_run(result, "run-hyb", 2000))

    assert [r["id"] for r in client.get("/api/projects/bev-car/runs").json()] == ["run-bev"]
    assert [r["id"] for r in client.get("/api/projects/hybrid-car/runs").json()] == ["run-hyb"]
    assert client.get("/api/projects/bev-car/runs/run-hyb").status_code == 404
    # a project with no runs (or not saved yet) simply has an empty history
    assert client.get("/api/projects/project-new1234/runs").json() == []


def test_runs_survive_a_restart(projects_dir, result):
    put("bev-car", make_run(result, "run-a", 1000))
    put("bev-car", make_run(result, "run-b", 2000))
    storage._seeded.clear()  # simulate the next launch

    fresh = TestClient(app)
    assert [r["id"] for r in fresh.get("/api/projects/bev-car/runs").json()] == ["run-b", "run-a"]
    assert fresh.get("/api/projects/bev-car/runs/run-a").json()["result"] == result


def test_a_lost_or_stale_index_is_rebuilt_from_the_run_files(projects_dir, result):
    put("bev-car", make_run(result, "run-a", 1000))
    put("bev-car", make_run(result, "run-b", 2000))
    folder = projects_dir / "runs" / "bev-car"

    (folder / "index.json").unlink()
    assert [r["id"] for r in client.get("/api/projects/bev-car/runs").json()] == ["run-b", "run-a"]

    (folder / "index.json").write_text("{not json", encoding="utf-8")
    (folder / "run-a.json.gz").unlink()
    (folder / "broken.json.gz").write_bytes(b"not gzip")
    runs = client.get("/api/projects/bev-car/runs").json()
    assert [r["id"] for r in runs] == ["run-b"]
    assert runs[0]["summary"] == result["summary"]


def test_delete_one_run_and_clear_all(projects_dir, result):
    for i in range(3):
        put("bev-car", make_run(result, f"run-{i}", 1000 + i))
    put("hybrid-car", make_run(result, "run-h", 5000))

    res = client.delete("/api/projects/bev-car/runs/run-1")
    assert res.json() == {"deleted": "run-1", "stored": 2}
    assert [r["id"] for r in client.get("/api/projects/bev-car/runs").json()] == ["run-2", "run-0"]
    assert client.get("/api/projects/bev-car/runs/run-1").status_code == 404
    assert client.delete("/api/projects/bev-car/runs/run-1").status_code == 404

    assert client.delete("/api/projects/bev-car/runs").json() == {"deleted": 2, "stored": 0}
    assert client.get("/api/projects/bev-car/runs").json() == []
    assert not (projects_dir / "runs" / "bev-car").exists()
    # other projects keep theirs
    assert len(client.get("/api/projects/hybrid-car/runs").json()) == 1


def test_deleting_a_project_deletes_its_runs(projects_dir, result):
    put("bev-car", make_run(result, "run-a", 1000))
    assert client.delete("/api/projects/bev-car").status_code == 200
    assert not (projects_dir / "runs" / "bev-car").exists()


def test_the_oldest_runs_make_room_when_the_disk_budget_is_reached(projects_dir, result, monkeypatch):
    size = put("bev-car", make_run(result, "run-0", 1000))["bytes"]
    monkeypatch.setattr(run_store, "BUDGET_BYTES", int(size * 2.5))
    assert put("bev-car", make_run(result, "run-1", 2000))["pruned"] == []

    res = put("bev-car", make_run(result, "run-2", 3000))
    assert res["pruned"] == ["run-0"]
    assert res["stored"] == 2
    assert res["bytes"] <= res["budget"] == int(size * 2.5)
    assert [r["id"] for r in client.get("/api/projects/bev-car/runs").json()] == ["run-2", "run-1"]
    assert not (projects_dir / "runs" / "bev-car" / "run-0.json.gz").exists()

    # a run bigger than the whole budget is still kept (the newest always is)
    monkeypatch.setattr(run_store, "BUDGET_BYTES", 10)
    res = put("bev-car", make_run(result, "run-3", 4000))
    assert res["pruned"] == ["run-1", "run-2"] and res["stored"] == 1


@pytest.mark.parametrize("project_id, run_id", [
    ("%2E%2E", "run-a"), (".hidden", "run-a"), ("bev-car", "%2E%2E"), ("bev-car", "a.b."), ("bev car", "run-a"),
])
def test_ids_that_are_not_plain_names_are_refused(projects_dir, result, project_id, run_id):
    # a literal ".." is collapsed by the client; "%2E%2E" reaches the handler as ".."
    run = make_run(result, run_id.replace("%2E", "."), 1000)
    res = client.put(f"/api/projects/{project_id}/runs/{run_id}", json=run)
    assert res.status_code == 400
    assert client.get(f"/api/projects/{project_id}/runs").status_code in (200, 400)
    assert client.delete(f"/api/projects/{project_id}/runs").status_code in (200, 400)
    assert not any(projects_dir.parent.rglob("*.gz"))
    assert (projects_dir / "bev-car.json").is_file()


def test_dot_names_never_reach_the_file_system(projects_dir):
    for bad in (".", "..", "../x", "x/..", ""):
        with pytest.raises(ValueError):
            run_store.list_runs(bad)
        with pytest.raises(ValueError):
            run_store.clear_runs(bad)
        with pytest.raises(ValueError):
            run_store.run_path("bev-car", bad)


def test_a_run_id_must_match_the_url(projects_dir, result):
    res = client.put("/api/projects/bev-car/runs/run-x", json=make_run(result, "run-y", 1000))
    assert res.status_code == 400


def test_clients_without_gzip_get_plain_json(projects_dir, result):
    put("bev-car", make_run(result, "run-a", 1000))
    res = client.get("/api/projects/bev-car/runs/run-a", headers={"Accept-Encoding": "identity"})
    assert "content-encoding" not in res.headers
    assert json.loads(res.content)["id"] == "run-a"


def test_over_long_ids_are_refused_not_a_server_error(projects_dir):
    # a 300-character id made the file system refuse the name: a 500 before
    long_id = "a" * 300
    assert client.get(f"/api/projects/{long_id}").status_code == 400
    assert client.get(f"/api/projects/{long_id}/runs").status_code == 400
    assert client.get(f"/api/projects/bev-car/runs/{long_id}").status_code == 400


def test_a_hand_edited_index_with_wrong_types_is_rebuilt(projects_dir, result):
    put("bev-car", make_run(result, "run-a", 1000))
    index = projects_dir / "runs" / "bev-car" / "index.json"
    index.write_text(json.dumps({"runs": [{"id": ["run-a"], "startedAt": "soon", "bytes": "big"}]}))
    res = client.get("/api/projects/bev-car/runs")
    assert res.status_code == 200
    assert [r["id"] for r in res.json()] == ["run-a"]


def test_runs_of_never_saved_projects_go_first_past_the_total_budget(projects_dir, result, monkeypatch):
    size = put("bev-car", make_run(result, "saved-old", 1000))["bytes"]
    put("unsaved-new", make_run(result, "orphan", 2000))  # no project file
    monkeypatch.setattr(run_store, "TOTAL_BUDGET_BYTES", int(size * 2.5))
    res = put("bev-car", make_run(result, "saved-new", 3000))
    assert res["pruned"] == [] and res["stored"] == 2
    assert client.get("/api/projects/unsaved-new/runs").json() == []

    # then the oldest of the saved projects' runs, never the one just stored
    res = put("hybrid-car", make_run(result, "hybrid", 4000))
    assert [r["id"] for r in client.get("/api/projects/bev-car/runs").json()] == ["saved-new"]
    assert [r["id"] for r in client.get("/api/projects/hybrid-car/runs").json()] == ["hybrid"]


def test_projects_with_ids_the_api_cannot_serve_are_not_listed(projects_dir):
    storage._ensure_dir()
    (projects_dir / "odd.json").write_text(json.dumps({"id": "../../x", "name": "Odd"}))
    (projects_dir / "dot.json").write_text(json.dumps({"id": ".", "name": "Dot"}))
    ids = [p["id"] for p in client.get("/api/projects").json()]
    assert "../../x" not in ids and "." not in ids
    assert "bev-car" in ids
