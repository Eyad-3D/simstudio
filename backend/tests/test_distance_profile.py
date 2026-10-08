"""A Driving Task that sets the speed against distance (ENG-34): the car
follows the profile at the distance it has driven, a case can end after a
number of laps, braking happens at the same place whatever the car weighs,
and the verdict judges the trace against distance."""
from __future__ import annotations

import pytest

from app.solver import simulate
from app.solver.verdict import distance_trace_metrics
from app.validation import validate_project
from tests.helpers import bev_axle, series

LAP = "0:30; 200:30; 300:90; 600:90; 700:40; 800:40; 900:70; 1100:70; 1200:30"


def lap_project(laps=2.0, duration=300.0, mass=None, **task):
    p = bev_axle(profile=LAP)
    t = next(e for e in p.systems[0].elements if e.id == "task")
    t.parameterOverrides.update({"mode": "distance", "repeat": True, **task})
    v = next(e for e in p.systems[0].elements if e.id == "veh")
    v.parameterOverrides["initial_speed_kmh"] = 30
    if mass:
        v.parameterOverrides["mass_kg"] = mass
    case = p.cases[0]
    case.duration, case.timeStep, case.endLaps = duration, 0.1, laps
    return p


def values(result, el_id: str, port_id: str) -> list[float]:
    return [p["value"] for p in series(result, el_id, port_id)]


def first_brake_m(result) -> float:
    """Where the Driver first brakes (this car brakes with its motor only)."""
    cmd = values(result, "drv", "sig_traction_cmd")
    dist = values(result, "veh", "sig_distance")
    return next(d for c, d in zip(cmd, dist) if c < -0.01)


def test_follows_speed_against_distance_and_ends_after_laps():
    r = simulate(lap_project(), "case")
    assert r.status == "success", [m.text for m in r.messages]
    dist = values(r, "veh", "sig_distance")
    assert dist[-1] == pytest.approx(2 * 1200, abs=1.0)  # two laps, then it stops
    assert "ended at 2400 m driven" in r.messages[0].text
    # on the 90 km/h straight of the second lap the target is 90 km/h
    target = values(r, "task", "sig_demand")
    i = next(k for k, d in enumerate(dist) if d > 1200 + 450)
    assert target[i] == pytest.approx(90.0)
    assert values(r, "veh", "sig_speed")[i] == pytest.approx(90.0, abs=2.0)


def test_braking_point_stays_put_when_the_mass_changes():
    light = first_brake_m(simulate(lap_project(laps=1), "case"))
    heavy = first_brake_m(simulate(lap_project(laps=1, mass=2600), "case"))
    assert 600 <= light <= 610  # the target starts falling at 600 m
    assert heavy == pytest.approx(light, abs=3.0)


def test_lap_count_needs_a_distance_task():
    p = lap_project(mode="time")
    r = simulate(p, "case")
    assert any("asks for 2 laps" in m.text for m in r.messages)
    assert series(r, "veh", "sig_distance")  # it still ran its duration
    checks = [c.text for c in validate_project(p)]
    assert any("ends after 2 laps" in t for t in checks)


def test_running_out_of_time_before_the_laps_end_warns():
    r = simulate(lap_project(laps=5, duration=60), "case")
    assert r.status == "warning"
    assert any("did not drive the case's 5 laps (6,000 m)" in m.text for m in r.messages)


def test_data_checks_for_distance_profiles():
    p = lap_project(profile="0:30; 500:0; 600:30")
    texts = [c.text for c in validate_project(p)]
    assert any("asks for 0 km/h at 500 m" in t for t in texts)
    p = lap_project(cycle="wltc-3b")
    errors = [c.text for c in validate_project(p) if c.level == "error"]
    assert any("a speed against time, but its Profile Axis is Distance" in t for t in errors)


def test_a_case_can_switch_the_axis():
    p = bev_axle(profile=LAP)
    p.cases[0].parameterOverrides = {"task": {"mode": "distance", "cycle": "wltc-3b"}}
    errors = [c.text for c in validate_project(p) if c.level == "error"]
    assert any("in case 'Case'" in t for t in errors)


def test_distance_trace_band_is_one_second_of_travel():
    # a 50 → 20 km/h step at 100 m, driven 3 m late: inside ±1 s of travel
    xs = [float(x) for x in range(0, 200)]
    tgt = [50.0 if x < 100 else 20.0 for x in xs]
    late = [50.0 if x < 103 else 20.0 for x in xs]
    ts = [x / (50 / 3.6) for x in xs]
    assert distance_trace_metrics(ts, xs, tgt, late).outside_wltp_s == 0.0
    # 30 m late is not
    very_late = [50.0 if x < 130 else 20.0 for x in xs]
    assert distance_trace_metrics(ts, xs, tgt, very_late).outside_wltp_s > 1.0


def test_laps_without_repeat_profile_warn():
    checks = [c.text for c in validate_project(lap_project(laps=3, repeat=False))]
    assert any("does not repeat its profile" in t for t in checks)
    assert not any("does not repeat its profile" in t for t in
                   (c.text for c in validate_project(lap_project(laps=3))))
