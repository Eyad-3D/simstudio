"""CON-16, CON-04, CON-11: the bundled drive cycles, their fingerprints, and
the Driving Task's and Road Profile's reference to them."""
from __future__ import annotations

import csv
import hashlib
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
REGISTER = Path(__file__).resolve().parents[2] / "docs" / "data-register.csv"
# the reuse bases docs/DATA-REGISTER.md allows for bundled cycles (CON-31)
REUSE_BASES = {"EU-2011/833", "US-17USC105", "JP-Art13", "Apache-2.0"}


@pytest.mark.parametrize("cycle_id", list(cycles.CYCLES))
def test_trace_matches_the_published_duration_and_distance(cycle_id):
    info = cycles.info(cycle_id)
    duration = info["duration_s"]
    if "published" in cycles.CYCLES[cycle_id]:
        duration, km = cycles.CYCLES[cycle_id]["published"]
        assert info["duration_s"] == duration
        assert info["distance_km"] == pytest.approx(km, rel=1e-3)  # within 0.1 %
    ends = [p[2] for p in info["phases"]]
    assert ends == [] or ends[-1] == duration
    starts = [p[1] for p in info["phases"]]
    assert starts == [] or (starts[0] == 0 and starts[1:] == ends[:-1])  # phases tile the cycle


@pytest.mark.parametrize("cycle_id", list(cycles.CYCLES))
def test_every_cycle_keeps_its_fingerprints(cycle_id):
    """CON-04: the sum of the 1 Hz speeds and the SHA-256 of the file, as
    scripts/cycles/build_cycles.py recorded them, so no file changes unseen."""
    fp = cycles.CYCLES[cycle_id]["fingerprint"]
    raw = (cycles.DIR / f"{cycle_id}.csv").read_bytes()
    assert hashlib.sha256(raw).hexdigest() == fp["sha256"]
    pts = list(cycles.trace(cycle_id))
    total = sum(interp_profile(pts, float(t), False) for t in range(int(pts[-1][0]) + 1))
    assert total == pytest.approx(fp["speed_sum_kmh"], abs=0.05)


def test_wltc_speed_sum_fingerprint():
    """The sum of the 1 Hz speeds, the fingerprint CON-04 checks cycles by,
    as the regulation's WLTC class 3b table gives it."""
    assert cycles.CYCLES["wltc-3b"]["fingerprint"]["speed_sum_kmh"] == pytest.approx(83758.6)
    assert cycles.info("wltc-3b")["distance_km"] == pytest.approx(23.27, abs=0.01)


def test_the_library_has_twenty_cycles_each_with_a_source_and_a_register_row():
    with REGISTER.open(encoding="utf-8", newline="") as f:
        register = {r["id"]: r for r in csv.DictReader(f)}
    assert len(cycles.CYCLES) >= 20
    for cycle_id, meta in cycles.CYCLES.items():
        assert meta["source"] and meta["reuse"] in REUSE_BASES, cycle_id
        row = register[meta["register"]]
        assert row["file"] == f"backend/app/cycles/{cycle_id}.csv", cycle_id
        assert row["kind"] == "drive-cycle" and row["reuse_basis"] == meta["reuse"], cycle_id


def test_cycles_with_grade_feed_a_road_profile_along_their_distance():
    assert cycles.has_grade("long-haul-100km") and not cycles.has_grade("wltc-3b")
    grade = cycles.grade_by_distance("long-haul-100km")
    km = cycles.info("long-haul-100km")["distance_km"]
    assert grade[-1][0] / 1000 == pytest.approx(km, abs=1e-3)
    assert all(b[0] > a[0] for a, b in zip(grade, grade[1:]))
    # within the whole route's -2.3 to +2.9 %
    assert -2.4 < min(g for _, g in grade) < 0 < max(g for _, g in grade) < 3.0


def test_api_lists_and_serves_cycles():
    c = TestClient(app)
    listed = c.get("/api/cycles").json()
    assert [x["id"] for x in listed] == list(cycles.CYCLES)
    assert listed[0]["name"] == "WLTC class 3b" and listed[0]["duration_s"] == 1800
    one = c.get("/api/cycles/wltc-3b").json()
    assert len(one["t"]) == len(one["v"]) == len(cycles.trace("wltc-3b"))
    assert one["grade"] is False and one["source"].startswith("Commission Regulation (EU) 2017/1151")
    assert c.get("/api/cycles/cltc").status_code == 404  # sold standard: never bundled


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
    _task(p).parameterOverrides["cycle"] = "cltc"
    errors = [c for c in validate_project(p) if c.level == "error"]
    assert [c.text for c in errors] == [
        "Driving Task 'Vehicle Task' uses the drive cycle 'cltc', which this version of "
        "LightSim does not include."]
    assert errors[0].elementIds == ["el-task"] and errors[0].fix
    r = simulate(p, "case-city")
    assert r.status == "failed" and "'cltc'" in r.messages[0].text


def test_an_unknown_cycle_in_a_case_fails_that_run():
    p = load_example("bev-car")
    case = next(c for c in p.cases if c.id == "case-wltc")
    case.parameterOverrides["el-task"]["cycle"] = "cltc"
    r = simulate(p, "case-wltc")
    assert r.status == "failed" and "'cltc'" in r.messages[0].text


def test_examples_name_their_cycles_by_id():
    for f in ("bev-car.json", "hybrid-car.json"):
        raw = json.loads((EXAMPLES / f).read_text(encoding="utf-8"))
        ovs = [ov for c in raw["cases"] for ov in (c.get("parameterOverrides") or {}).values()]
        assert not [ov for ov in ovs if "profile" in ov]
        assert {ov["cycle"] for ov in ovs if "cycle" in ov} <= set(cycles.CYCLES)


def _with_road_profile(p, cycle_id):
    from app.schemas import ElementInstance
    p.systems[0].elements.append(ElementInstance(
        id="el-road", componentDefId="signal.road_profile", label="Route",
        position={"x": 0, "y": 0}, parameterOverrides={"cycle": cycle_id}))
    return p


def test_a_road_profile_takes_its_cycles_grade_and_refuses_one_without():
    p = _with_road_profile(load_example("bev-car"), "long-haul-100km")
    params = build_model(p).params_of["el-road"]
    assert params["profile"] == cycles.grade_profile_text("long-haul-100km")
    assert params["mode"] == "distance"
    assert not [c.text for c in validate_project(p) if "profile" in c.text and c.level == "error"]
    p = _with_road_profile(load_example("bev-car"), "wltc-3b")
    errors = [c.text for c in validate_project(p) if c.level == "error"]
    assert errors == ["Road Profile 'Route' takes its grade from the drive cycle 'WLTC class 3b', "
                      "which has no grade; pick a cycle with a grade, or Custom profile."]
