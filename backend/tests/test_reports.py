"""The run reports (RES-22 energy, RES-38 limits, RES-39 duty): the energy
table is every part's own books (MOD-10), the car's sources and sinks add
up to within the books' closing residual, the duty matches a calculation on
the stored points, and the limit lane covers the run."""
import math

import pytest
from helpers import bev_axle, conn, dbc, el, example_result, project, series

from app.solver import simulate
from app.solver.energy import SOURCE_PARTS, Flow
from app.solver.reports import book_report
from app.storage import load_example


def _close(part) -> float:
    return part.inKWh - part.outKWh - part.lostKWh - part.storedKWh


def _residual_kwh(r) -> float:
    """The summary's Energy balance residual in kWh: Σ (in − out) over the
    parts, + when they took in more than they were given."""
    released = sum(-f.stored for f in r.partEnergy if f.part in SOURCE_PARTS and f.stored < 0)
    pct = next(s.value for s in r.summary if s.label == "Energy balance residual")
    return pct / 100.0 * released


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
    # the table is each part's own books: a row per gear and wheel, none
    # worked out from what is left
    books = {f.label: f for f in r.partEnergy}
    for p in e.parts:
        f = books[p.label]
        assert (p.inKWh, p.outKWh, p.lostKWh, p.storedKWh) == pytest.approx(
            (f.energyIn, f.energyOut, f.losses, f.stored), abs=2e-6), p.label
    assert {"Final Drive", "Differential", "Wheel L", "Wheel R"} <= {p.label for p in e.parts}
    assert not any(p.kind == "driveline" for p in e.parts)
    groups = {f.label: f.group for f in e.sinks}
    assert groups["Final Drive"] == "losses" and groups["Wheel L — tyre slip"] == "losses"
    # what is not accounted for is only where the books together do not
    # close: the summary's Energy balance residual, counted the other way
    assert e.remainderKWh == pytest.approx(-_residual_kwh(r), abs=2e-6)
    assert abs(e.remainderPct) < 0.5  # (0.19 %: a hard launch at a 10 ms step)


def _book(leak_kwh: float) -> list:
    kwh = 3.6e6
    bat = Flow("b", "Battery", "battery.generic")
    bat.out_j, bat.stored_j = 1.0 * kwh, -1.0 * kwh
    mot = Flow("m", "E-Motor", "motor.emotor")
    mot.in_j, mot.out_j = (1.0 - leak_kwh) * kwh, 0.9 * kwh
    fd = Flow("fd", "Final Drive", "mech.final_drive")
    fd.in_j, fd.out_j = 0.9 * kwh, 0.88 * kwh
    veh = Flow("v", "Vehicle", "vehicle.body")
    veh.in_j = 0.88 * kwh
    veh.terms = {"air drag": 0.5 * kwh, "rolling resistance": 0.38 * kwh, "climbing": 0.0,
                 "acceleration": 0.0}
    return [veh, fd, mot, bat]


def test_what_the_books_do_not_close_is_not_accounted_for():
    e = book_report(_book(0.0))
    assert [p.label for p in e.parts] == ["Battery", "E-Motor", "Final Drive", "Vehicle"]
    assert e.sourceKWh == pytest.approx(1.0)
    assert {f.label: f.kWh for f in e.sinks} == pytest.approx(
        {"E-Motor — losses": 0.1, "Final Drive": 0.02, "Air drag": 0.5, "Rolling resistance": 0.38})
    assert e.remainderKWh == pytest.approx(0.0, abs=1e-9)
    # 10 Wh that left the battery but never reached the motor
    e = book_report(_book(0.01))
    assert e.remainderKWh == pytest.approx(0.01, abs=1e-9)
    assert e.remainderPct == pytest.approx(1.0)


def test_a_lap_case_names_its_brakes_and_gears():
    # the lap's own energy pass books them, as the Vehicle's terms
    e = example_result("fs-electric", "case-autocross").energy
    groups = {f.label: f.group for f in e.sinks}
    assert groups["Friction brakes"] == "brakes"
    assert groups["Gears and spinning parts"] == "losses"
    assert groups["Air drag and rolling resistance"] == "road"
    assert abs(e.remainderPct) < 0.05


def test_a_propeller_keeps_its_books():
    els = [el("src", "electric.voltage_source", "Supply", voltage_V=350),
           el("bus", "electric.node", "Bus"),
           el("mot", "motor.emotor", "Motor"),
           el("prop", "propulsion.propeller", "Propeller", torque_ref_Nm=50, ref_speed_rpm=3000),
           el("dem", "signal.constant", "Demand", value=0.3)]
    cons = [conn(1, "src", "pos", "bus", "t1"), conn(2, "bus", "t2", "mot", "pos"),
            conn(3, "mot", "shaft", "prop", "shaft")]
    r = simulate(project(els, cons, [dbc(1, "dem", "sig_out", "mot", "sig_demand_in")],
                         duration=20.0, time_step=0.1), "case")
    assert r.status in ("success", "warning"), [m.text for m in r.messages]
    prop = next(f for f in r.partEnergy if f.label == "Propeller")
    assert prop.energyIn > 0.01 and prop.losses == pytest.approx(prop.energyIn)
    assert abs(_residual_kwh(r)) < 0.005 * prop.energyIn
    assert abs(r.energy.remainderPct) < 0.5


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


def test_a_hybrid_charging_its_battery_is_not_braking():
    # the P2 hybrid's script charges the battery with the motor while the
    # engine pulls: the band says braking only while the driver brakes
    proj = load_example("hybrid-car")
    r = simulate(proj, "case-mixed")
    braking_s = r.limits.lanes[0].seconds["braking"]
    ch = {(c.elementId, c.portId): c.timeSeries for c in r.channels}
    brake, traction = ch[("el-driver", "sig_brake_cmd")], ch[("el-driver", "sig_traction_cmd")]
    driver_s = sum(b["t"] - a["t"] for a, b, t in zip(brake, brake[1:], traction[1:])
                   if (b["value"] or 0) > 1e-6 or (t["value"] or 0) < -1e-6)
    assert braking_s == pytest.approx(driver_s, abs=10.0)
    assert braking_s < 0.4 * r.limits.tEnd


def test_formula_student_acceleration_shows_grip_then_the_power_limit():
    proj = load_example("fs-electric")
    r = simulate(proj, "case-accel-75m")
    lane = r.limits.lanes[0]
    # the rear wheels spin at the launch, then the 80 kW limit holds the car
    assert lane.seconds.get("grip", 0) > 0.5
    assert lane.seconds.get("set_limit", 0) > 1.0
    order = [r.limits.states[int(c)] for _, c in lane.changes]
    assert order.index("grip") < order.index("set_limit")
    # a short, violent run still adds up within 0.5 %
    assert abs(r.energy.remainderPct) < 0.5


@pytest.mark.parametrize("project_id,case_id", [("bev-car", "case-city"), ("hybrid-car", "case-mixed")])
def test_the_examples_energy_adds_up(project_id, case_id):
    proj = load_example(project_id)
    case = next(c for c in proj.cases if c.id == case_id)
    case.duration = 200.0
    r = simulate(proj, case_id)
    e = r.energy
    assert abs(e.remainderPct) < 0.1
    assert e.remainderKWh == pytest.approx(-_residual_kwh(r), abs=1e-5)
    for p in e.parts:
        assert abs(_close(p)) < 1e-5, p
    if project_id == "hybrid-car":
        assert any(f.label.startswith("Fuel") for f in e.sources)
        assert any(d.kind == "engine.combustion" for d in r.duty)
