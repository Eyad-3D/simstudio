"""The run reports (RES-22 energy, RES-38 limits, RES-39 duty): each part's
books close, the car's sources and sinks add up, the duty matches a
calculation on the stored points, and the limit lane covers the run."""
import math

import pytest
from helpers import bev_axle, series

from app.solver import simulate
from app.storage import load_example


def _close(part) -> float:
    return part.inKWh - part.outKWh - part.lostKWh - part.storedKWh


def _bev(duration=60.0, step=0.01):
    proj = bev_axle(profile="0:0; 10:60; 30:60; 45:0; 60:0")
    proj.cases[0].duration = duration
    proj.cases[0].timeStep = step
    return proj


def test_every_part_keeps_its_books_and_the_car_adds_up():
    proj = _bev()
    r = simulate(proj, proj.cases[0].id)
    assert r.status in ("success", "warning")
    e = r.energy
    assert e is not None and e.sourceKWh > 0
    for p in e.parts:
        assert abs(_close(p)) < 1e-5, p
    # the bands drawn: the sources and the sinks, with the remainder its own band
    assert abs(sum(f.kWh for f in e.sources) - sum(f.kWh for f in e.sinks) - e.remainderKWh) < 1e-5
    assert abs(e.remainderPct) < 1.0
    labels = {f.label for f in e.sinks}
    assert {"Air drag", "Rolling resistance"} <= labels
    assert any(f.group == "recovered" for f in e.sinks)  # braking charged the battery
    # the battery's books are the summary's
    bat = next(p for p in e.parts if p.kind == "battery.generic")
    delivered = next(s for s in r.summary if s.label.endswith("energy delivered"))
    assert bat.outKWh == pytest.approx(delivered.value, abs=1e-3)
    assert e.balanceErrorPct == next(
        s.value for s in r.summary if s.label == "Electrical energy balance error")


def test_the_energy_report_can_be_turned_off():
    proj = _bev(duration=10.0)
    proj.cases[0].energyReport = False
    r = simulate(proj, proj.cases[0].id)
    assert r.energy is None
    assert r.duty and r.limits  # the other reports stay


def test_duty_matches_the_full_resolution_points():
    proj = _bev()
    r = simulate(proj, proj.cases[0].id)
    motor = next(d for d in r.duty if d.kind == "motor.emotor")
    row = next(x for x in motor.rows if x.quantity == "Shaft power")
    pts = series(r, motor.elementId, "sig_mech_power")
    vals = [p["value"] for p in pts[1:]]  # each point holds its step's value
    rms = math.sqrt(sum(v * v for v in vals) / len(vals))
    assert row.rms == pytest.approx(rms, rel=5e-3)
    assert row.mean == pytest.approx(sum(vals) / len(vals), rel=5e-3, abs=1e-3)
    assert row.max == pytest.approx(max(vals), rel=5e-3)
    bat = next(d for d in r.duty if d.kind == "battery.generic")
    assert {x.quantity for x in bat.rows} >= {"Power", "Current"}


def test_the_limit_lane_covers_the_run():
    proj = _bev()
    r = simulate(proj, proj.cases[0].id)
    lim = r.limits
    assert lim is not None and len(lim.lanes) == 1
    lane = lim.lanes[0]
    assert sum(lane.seconds.values()) == pytest.approx(lim.tEnd, abs=1e-6)
    assert lim.tEnd == pytest.approx(60.0, abs=1e-6)
    assert lane.changes[0][0] == 0.0
    assert {"braking", "demand"} <= set(lane.seconds)
    assert all(lim.states[int(c)] for _, c in lane.changes)


def test_formula_student_acceleration_shows_grip_then_the_power_limit():
    proj = load_example("fs-electric")
    r = simulate(proj, "case-accel-75m")
    lane = r.limits.lanes[0]
    # the rear wheels spin at the launch, then the 80 kW limit holds the car
    assert lane.seconds.get("grip", 0) > 0.5
    assert lane.seconds.get("set_limit", 0) > 1.0
    order = [r.limits.states[int(c)] for _, c in lane.changes]
    assert order.index("grip") < order.index("set_limit")
    # a short, violent run still adds up within 1 %
    assert abs(r.energy.remainderPct) < 1.0


@pytest.mark.parametrize("project_id,case_id", [("bev-car", "case-city"), ("hybrid-car", "case-mixed")])
def test_the_examples_energy_adds_up(project_id, case_id):
    proj = load_example(project_id)
    case = next(c for c in proj.cases if c.id == case_id)
    case.duration = 200.0
    r = simulate(proj, case_id)
    e = r.energy
    assert abs(e.remainderPct) < 1.0
    for p in e.parts:
        assert abs(_close(p)) < 1e-5, p
    if project_id == "hybrid-car":
        assert any(f.label.startswith("Fuel") for f in e.sources)
        assert any(d.kind == "engine.combustion" for d in r.duty)
