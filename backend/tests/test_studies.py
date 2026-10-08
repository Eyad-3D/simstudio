"""Studies on all cores (ENG-05): a study's points run side by side in worker
processes, each is stored as a run of the project, its summary comes back at
once, the server stays free while it runs, and a stop ends it."""
from __future__ import annotations

import time

import pytest
from fastapi.testclient import TestClient

from app import run_store, studies
from app.main import app
from app.solver import simulate
from app.storage import load_example
from tests.helpers import bev_axle


@pytest.fixture()
def client(tmp_path, monkeypatch):
    monkeypatch.setenv("LIGHTSIM_PROJECTS_DIR", str(tmp_path))
    with TestClient(app) as c:
        yield c


def short_bev(duration=60.0):
    p = load_example("bev-car")
    p.id = "study-test"
    p.cases[0].duration = duration
    return p


def mass_points(masses):
    return [{"overrides": {"el-vehicle": {"mass_kg": m}}, "values": [m], "label": f"mass {m}"}
            for m in masses]


def summary_of(event, label):
    return next(s["value"] for s in event["summary"] if s["label"] == label)


def test_points_match_one_run_each_and_are_stored(client):
    p = short_bev()
    masses = [1500, 1900, 2300]
    r = client.post("/api/studies", json={
        "project": p.model_dump(mode="json"), "caseId": "case-city",
        "points": mass_points(masses), "sweepId": "sw1", "sweepParam": "Vehicle Mass",
        "sweepUnit": "kg", "workers": 2})
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["workers"] == 2 and len(body["points"]) == 3
    stored = {e["id"]: e for e in run_store.list_runs("study-test")}
    for event, m in zip(body["points"], masses):
        q = short_bev()
        q.cases[0].parameterOverrides.setdefault("el-vehicle", {})["mass_kg"] = m
        alone = simulate(q, "case-city")
        assert event["status"] == alone.status
        # identical to a run of its own, to the last digit
        assert summary_of(event, "Consumption") == next(
            s.value for s in alone.summary if s.label == "Consumption")
        assert event["values"] == [m] and event["wallS"] > 0
        e = stored[event["runId"]]
        assert (e["sweepId"], e["sweepValue"], e["caseName"]) == ("sw1", m, f"mass {m}")
    # the full traces are on disk, with the model the point ran
    run = client.get(f"/api/projects/study-test/runs/{body['points'][0]['runId']}").json()
    assert run["result"]["channels"]
    assert run["snapshot"]["case"]["parameterOverrides"]["el-vehicle"]["mass_kg"] == 1500
    # the model as it was: the swept value is the run's own, not a change (UX-41)
    then = next(c for c in run["snapshot"]["project"]["cases"] if c["id"] == "case-city")
    assert "mass_kg" not in then["parameterOverrides"].get("el-vehicle", {})
    assert run["snapshot"]["liveEdits"] == []  # the app reads it


def test_a_model_with_scripts_runs_in_the_pool(client):
    p = load_example("hybrid-car")  # its controller is a Script block
    p.id = "study-hybrid"
    case = next(c for c in p.cases if c.id == "case-mixed")
    case.duration, case.chargeBalance = 60.0, False
    r = client.post("/api/studies", json={
        "project": p.model_dump(mode="json"), "caseId": "case-mixed", "workers": 2,
        "points": [{"overrides": {}, "values": [k]} for k in (1, 2)]})
    assert r.status_code == 200, r.text
    assert all(e["status"] in ("success", "warning") for e in r.json()["points"])


def test_the_live_channel_streams_points_and_stops(client):
    p = short_bev(duration=600.0)
    with client.websocket_connect("/api/studies/run") as ws:
        ws.send_json({"type": "start", "project": p.model_dump(mode="json"),
                      "caseId": "case-city", "points": mass_points([1500, 1600, 1700, 1800]),
                      "workers": 2})
        started = ws.receive_json()
        assert started == {"type": "started", "workers": 2, "points": 4}
        t0 = time.monotonic()
        # the server answers at once while the workers solve
        assert client.get("/api/health").status_code == 200
        assert time.monotonic() - t0 < 1.0
        time.sleep(8.0)  # the two workers start and solve their first points
        ws.send_json({"type": "cancel"})
        events = []
        while True:
            e = ws.receive_json()
            if e["type"] == "done":
                break
            events.append(e)
    assert len(events) == 4
    assert {e["status"] for e in events} <= {"cancelled", "not run"}
    assert any(e["status"] == "not run" for e in events)  # two never started
    done = [e for e in events if e["status"] == "cancelled"]
    assert done, "the points that were running end as stopped runs, not as not run"
    assert all(e["incomplete"].startswith("stopped") and e["runId"] for e in done)


def test_a_study_that_cannot_start_says_why(client):
    p = short_bev()
    r = client.post("/api/studies", json={"project": p.model_dump(mode="json"),
                                          "caseId": "nope", "points": mass_points([1500])})
    assert r.status_code == 400 and "not found" in r.json()["detail"]


def test_the_default_pool_leaves_a_core_and_fits_in_memory(monkeypatch):
    p = bev_axle()
    monkeypatch.setattr(studies.os, "cpu_count", lambda: 8)
    monkeypatch.setattr(studies, "_memory_bytes", lambda: 64 * 2**30)
    assert studies.default_workers(p, 100) == 7
    assert studies.default_workers(p, 3) == 3
    monkeypatch.setattr(studies, "_memory_bytes", lambda: 2 * 2**30)  # 2 GB
    assert studies.default_workers(p, 100) == 4  # 1 GB ÷ 250 MB
    hybrid = load_example("hybrid-car")  # Script blocks: + 512 MB a worker
    assert studies.default_workers(hybrid, 100) == 1
