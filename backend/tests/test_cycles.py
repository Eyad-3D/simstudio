"""CON-16: the bundled drive cycles and the Driving Task's reference to them."""
from __future__ import annotations

import json
from pathlib import Path

import pytest
from fastapi.testclient import TestClient

from app import cycles
from app.main import app
from app.solver import simulate
from app.solver.network import build_model
from app.solver.profiles import interp_profile
from app.storage import load_example
from app.validation import validate_project

EXAMPLES = Path(__file__).resolve().parents[1] / "projects"


@pytest.mark.parametrize("cycle_id", list(cycles.CYCLES))
def test_trace_matches_the_published_duration_and_distance(cycle_id):
    info = cycles.info(cycle_id)
    duration, km = cycles.CYCLES[cycle_id]["published"]
    assert info["duration_s"] == duration
    assert info["distance_km"] == pytest.approx(km, rel=1e-3)  # within 0.1 %
    assert info["phases"] == [] or info["phases"][-1][2] == duration


def test_wltc_speed_sum_fingerprint():
    """The sum of the 1 Hz speeds, the fingerprint CON-04 checks cycles by."""
    pts = list(cycles.trace("wltc-3b"))
    assert sum(interp_profile(pts, float(t), False) for t in range(1801)) == pytest.approx(
        83758.6, abs=0.05)


def test_api_lists_and_serves_cycles():
    c = TestClient(app)
    listed = c.get("/api/cycles").json()
    assert [x["id"] for x in listed] == list(cycles.CYCLES)
    assert listed[0]["name"] == "WLTC class 3b" and listed[0]["duration_s"] == 1800
    one = c.get("/api/cycles/wltc-3b").json()
    assert len(one["t"]) == len(one["v"]) == len(cycles.trace("wltc-3b"))
    assert c.get("/api/cycles/nedc").status_code == 404


def _task(project):
    return next(e for s in project.systems for e in s.elements if e.id == "el-task")


def test_a_cycle_drives_its_trace_and_a_case_profile_still_wins():
    p = load_example("bev-car")
    _task(p).parameterOverrides["cycle"] = "udds"
    assert build_model(p).params_of["el-task"]["profile"] == cycles.profile_text("udds")
    own = build_model(p, case_overrides={"el-task": {"profile": "0:0; 10:10"}})
    assert own.params_of["el-task"]["profile"] == "0:0; 10:10"
    # a case's own cycle beats both
    case = build_model(p, case_overrides={"el-task": {"cycle": "hwfet", "profile": "0:0; 10:10"}})
    assert case.params_of["el-task"]["profile"] == cycles.profile_text("hwfet")


def test_the_typed_profile_is_not_checked_while_a_cycle_is_set():
    p = load_example("bev-car")
    _task(p).parameterOverrides.update(profile="not a profile", cycle="udds")
    assert not [c.text for c in validate_project(p) if "profile" in c.text]


def test_an_unknown_cycle_is_one_error_on_its_part_and_fails_the_run():
    p = load_example("bev-car")
    _task(p).parameterOverrides["cycle"] = "nedc"
    errors = [c for c in validate_project(p) if c.level == "error"]
    assert [c.text for c in errors] == [
        "Driving Task 'Vehicle Task' uses the drive cycle 'nedc', which this version of "
        "LightSim does not include."]
    assert errors[0].elementIds == ["el-task"] and errors[0].fix
    r = simulate(p, "case-city")
    assert r.status == "failed" and "'nedc'" in r.messages[0].text


def test_an_unknown_cycle_in_a_case_fails_that_run():
    p = load_example("bev-car")
    case = next(c for c in p.cases if c.id == "case-wltc")
    case.parameterOverrides["el-task"]["cycle"] = "nedc"
    r = simulate(p, "case-wltc")
    assert r.status == "failed" and "'nedc'" in r.messages[0].text


def test_examples_name_their_cycles_by_id():
    for f in ("bev-car.json", "hybrid-car.json"):
        raw = json.loads((EXAMPLES / f).read_text(encoding="utf-8"))
        ovs = [ov for c in raw["cases"] for ov in (c.get("parameterOverrides") or {}).values()]
        assert not [ov for ov in ovs if "profile" in ov]
        assert {ov["cycle"] for ov in ovs if "cycle" in ov} <= set(cycles.CYCLES)
