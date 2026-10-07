"""Resizing motors and engines with scale factors (MOD-47).

Metric: a motor scaled by k_T = 1 and k_n = 1 reproduces its map exactly;
a k_T = 2 motor shows twice the torque with the documented loss scaling;
scaled maps pass the MOD-18 edge checks.
"""
from __future__ import annotations

import pytest
from helpers import series

from app.library import library_by_id
from app.schemas import SimCase
from app.solver import simulate
from app.solver.maps import interp1, interp2, parse_table1d, parse_table2d
from app.solver.network import build_model
from app.solver.runtime import RPM
from app.solver.scaling import scaled
from app.storage import load_example
from app.validation import validate_project

MOTOR = {p.key: p.default for p in library_by_id()["motor.emotor"].parameters}
ENGINE = {p.key: p.default for p in library_by_id()["engine.combustion"].parameters}


def test_100_percent_is_the_map_as_typed():
    for key in ("full_load_torque", "power_loss"):
        pts = parse_table2d(MOTOR[key])
        assert scaled("motor.emotor", key, pts, {**MOTOR}) is pts
    pts = parse_table1d(MOTOR["drag_torque"])
    assert scaled("motor.emotor", "drag_torque", pts, {**MOTOR, "torque_scale_pct": 100,
                                                        "speed_scale_pct": 100}) is pts


def test_torque_scale_doubles_torque_and_scales_losses():
    p = {**MOTOR, "torque_scale_pct": 200}
    fl0, fl = parse_table2d(MOTOR["full_load_torque"]), None
    fl = scaled("motor.emotor", "full_load_torque", fl0, p)
    loss0 = parse_table2d(MOTOR["power_loss"])
    loss = scaled("motor.emotor", "power_loss", loss0, p)
    for v, n in ((300.0, 2500.0), (350.0, 6000.0)):
        assert interp2(fl, v, n) == pytest.approx(2 * interp2(fl0, v, n))
    for n, t in ((3000.0, 150.0), (6000.0, 300.0)):
        # the loss at 2 × a torque is 2 × the original's at that torque
        assert interp2(loss, n, 2 * t) == pytest.approx(2 * interp2(loss0, n, t))
    # efficiency at the same relative torque is unchanged (losses and power both × 2)
    drag0 = parse_table1d(MOTOR["drag_torque"])
    drag = scaled("motor.emotor", "drag_torque", drag0, p)
    assert interp1(drag, 6000.0) == pytest.approx(2 * interp1(drag0, 6000.0))


def test_speed_scale_stretches_speed_at_the_same_power():
    p = {**MOTOR, "speed_scale_pct": 200, "max_speed_rpm": 12000}
    fl0 = parse_table2d(MOTOR["full_load_torque"])
    fl = scaled("motor.emotor", "full_load_torque", fl0, p)
    for v, n in ((300.0, 2000.0), (350.0, 5000.0)):
        assert interp2(fl, v, 2 * n) * 2 * n == pytest.approx(interp2(fl0, v, n) * n)
    loss0 = parse_table2d(MOTOR["power_loss"])
    loss = scaled("motor.emotor", "power_loss", loss0, p)
    assert interp2(loss, 6000.0, 50.0) == pytest.approx(interp2(loss0, 3000.0, 100.0))
    from app.solver.scaling import max_speed_rpm
    assert max_speed_rpm(p) == 24000


def test_voltage_scale_moves_the_voltage_axis_only():
    p = {**MOTOR, "voltage_scale_pct": 200}
    fl0 = parse_table2d(MOTOR["full_load_torque"])
    fl = scaled("motor.emotor", "full_load_torque", fl0, p)
    assert [v for v, _ in fl] == [2 * v for v, _ in fl0]
    assert scaled("motor.emotor", "power_loss", parse_table2d(MOTOR["power_loss"]), p) == \
        parse_table2d(MOTOR["power_loss"])


def test_engine_scale_keeps_its_fuel_use_per_kwh():
    p = {**ENGINE, "engine_scale_pct": 150}
    fuel0 = parse_table2d(ENGINE["fuel_map"])
    fuel = scaled("engine.combustion", "fuel_map", fuel0, p)
    n, t = 3000.0, 100.0
    bsfc0 = interp2(fuel0, n, t) / (t * n / RPM)
    bsfc = interp2(fuel, n, 1.5 * t) / (1.5 * t * n / RPM)
    assert bsfc == pytest.approx(bsfc0)
    fl0 = parse_table1d(ENGINE["full_load_torque"])
    assert max(t for _, t in scaled("engine.combustion", "full_load_torque", fl0, p)) == \
        pytest.approx(1.5 * max(t for _, t in fl0))


def _bev(**motor):
    p = load_example("bev-car")
    for e in p.systems[0].elements:
        if e.id == "el-motor":
            e.parameterOverrides.update(motor)
    p.cases.append(SimCase(id="perf", name="0-100", kind="performance", duration=20,
                           timeStep=0.01))
    for e in p.systems[0].elements:
        if e.id == "el-task":
            e.parameterOverrides = {"profile": "0:100; 20:100"}
    return p


def test_a_doubled_motor_runs_with_twice_the_torque_and_its_rotor_inertia():
    base = simulate(_bev(), "perf")
    big = simulate(_bev(torque_scale_pct=200), "perf")
    assert big.status == base.status == "success"
    t0 = max(x["value"] for x in series(base, "el-motor", "sig_torque"))
    t2 = max(x["value"] for x in series(big, "el-motor", "sig_torque"))
    assert t0 < t2 <= 2 * t0 + 1e-6
    s0 = {x.label: x.value for x in base.summary}
    s2 = {x.label: x.value for x in big.summary}
    assert s2["Time to 100 km/h"] < s0["Time to 100 km/h"]
    m0 = build_model(_bev(), {}, {})
    m2 = build_model(_bev(torque_scale_pct=200), {}, {})
    j0 = sum(seg.inertia for dl in m0.drivelines for seg in dl.segments)
    j2 = sum(seg.inertia for dl in m2.drivelines for seg in dl.segments)
    assert j2 > j0  # the rotor is twice as long


def test_scaled_maps_pass_the_map_edge_checks():
    for motor in ({"torque_scale_pct": 200}, {"speed_scale_pct": 150},
                  {"torque_scale_pct": 60, "speed_scale_pct": 80}):
        p = _bev(**motor)
        checks = validate_project(p)
        assert not [c.text for c in checks if c.level == "error"]
        assert not [c.text for c in checks if c.level == "warning" and "E-Motor" in c.text]
        assert any("is resized" in c.text for c in checks if c.level == "info")
        city = load_example("bev-car")
        for e in city.systems[0].elements:
            if e.id == "el-motor":
                e.parameterOverrides.update(motor)
        r = simulate(city, "case-city")
        assert r.status != "failed"
        # no table read past its data, no machine over its maximum speed
        assert not [m.text for m in r.messages if m.level != "info"
                    and ("table" in m.text or "maximum speed" in m.text)]


def test_scales_outside_the_rules_range_warn():
    checks = validate_project(_bev(torque_scale_pct=300))
    assert any("outside the 50–200 %" in c.text for c in checks if c.level == "warning")
