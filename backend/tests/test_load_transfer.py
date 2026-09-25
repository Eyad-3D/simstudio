"""Load transfer and downforce (MOD-40).

Each wheel's normal load is its share of the weight normal to the road plus
its axle's part of the longitudinal load transfer m·(a + g·sin θ)·h/L, with
a the vehicle's acceleration over the previous solver step, and of the
downforce ½·ρ·CzA·v² split by the aero balance. With a CG height and a
downforce area of 0 (the defaults) every wheel keeps its share of the
weight, as before. Recorded values are rounded to 1e-5, which sets the
tolerances of the exact checks."""
import math

import pytest
from helpers import bev_axle, coast_project, conn, dbc, el, series

from app.solver import simulate
from app.solver.runtime import GRAVITY, air_density

M, H, L, FRONT = 300.0, 0.30, 1.55, 0.45  # a Formula Student-sized car, 45 % front
R, J_FREE = 0.26, 0.3 + 0.01  # tyre radius; a free wheel's inertia with its brake's
DT = 0.01


def _values(result, el_id, port):
    return [p["value"] for p in series(result, el_id, port)]


def _speeds(result):
    return [x / 3.6 for x in _values(result, "veh", "sig_speed")]


def _tag_axles(proj):
    """coast_project's wheels 0 and 1 on the front axle, 2 and 3 on the rear."""
    for e in proj.systems[0].elements:
        if e.componentDefId == "propulsion.wheel":
            e.parameterOverrides["axle"] = "Front" if e.id in ("w0", "w1") else "Rear"
    return proj


def fs_car(driven="Rear", mu=2.0, h=H, fd=4.0, v0=36.0, duration=1.5):
    """bev_axle's driven wheels on the `driven` axle and two free wheels, each
    on a brake, on the other: 300 kg, 45 % on the front axle, no drag and no
    rolling resistance, at the 10 ms step, started at 36 km/h (out of the
    tyres' low-speed zone). The Driver asks for 150 km/h: full traction."""
    proj = bev_axle(profile="0:150; 60:150")
    other = "Front" if driven == "Rear" else "Rear"
    share = {"Front": 100 * FRONT / 2, "Rear": 100 * (1 - FRONT) / 2}
    tyre = dict(mu=mu, rolling_resistance=0.0, radius_m=R, inertia_kgm2=0.3)
    els = proj.systems[0].elements
    for e in els:
        if e.id in ("whl", "whr"):
            e.parameterOverrides.update(axle=driven, vehicle_load_share_pct=share[driven], **tyre)
        elif e.id == "veh":
            e.parameterOverrides.update(mass_kg=M, cd=0.0, cg_height_m=h, wheelbase_m=L,
                                        initial_speed_kmh=v0)
        elif e.id == "fd":
            e.parameterOverrides["ratio"] = fd
    for i in (1, 2):
        els += [el(f"wf{i}", "propulsion.wheel", f"Free {i}", axle=other,
                   vehicle_load_share_pct=share[other], **tyre),
                el(f"bf{i}", "mech.brake", f"Brake {i}", inertia_kgm2=0.01)]
        proj.systems[0].connections.append(conn(20 + i, f"bf{i}", "flange", f"wf{i}", "shaft"))
    proj.cases[0].duration, proj.cases[0].timeStep = duration, DT
    return proj


def test_axle_loads_follow_m_a_h_over_l():
    """The item's metric: the rear axle gains m·a·h/L under acceleration and
    the front axle under braking, within 0.5 % (measured 0.011 % at
    7.6 m/s² and 0.034 % at 12.5 m/s²), with a from the recorded speed; the
    axles carry the weight
    together. The transfer is one step behind: each step's loads follow the
    previous step's acceleration exactly (this is what KNOWN-LIMITS says)."""
    # (a) accelerating at a constant motor torque (the Driver unwired)
    proj = fs_car()
    proj.dataBusConnections = [c for c in proj.dataBusConnections if c.port2Id != "sig_demand_in"]
    proj.systems[0].elements.append(el("cmd", "signal.constant", "Command", value=0.5))
    proj.dataBusConnections.append(dbc(70, "cmd", "sig_out", "mot", "sig_demand_in"))
    result = simulate(proj, "case")
    v = _speeds(result)
    front, rear = _values(result, "veh", "sig_load_front"), _values(result, "veh", "sig_load_rear")
    static = (1 - FRONT) * M * GRAVITY
    accel = [0.0] + [(v[i] - v[i - 1]) / DT for i in range(1, len(v))]  # 0 before the run
    checked = 0
    for i in range(1, len(v)):
        assert front[i] + rear[i] == pytest.approx(M * GRAVITY, abs=1e-4)
        # (c) the documented lag, at every point (the first ones included,
        # where the acceleration builds up step by step)
        assert rear[i] - static == pytest.approx(M * accel[i - 1] * H / L, abs=0.05)
        if 0.1 <= i * DT <= 1.1:
            expected = M * accel[i] * H / L
            assert abs(rear[i] - static - expected) <= 0.005 * expected
            checked += 1
    assert checked == 101 and min(accel[10:111]) > 5.0

    # (b) braking with all four brakes at 20 % from 100 km/h
    proj = _tag_axles(coast_project(
        {"mass_kg": M, "cd": 0.0, "cg_height_m": H, "wheelbase_m": L},
        {"rolling_resistance": 0.0, "mu": 2.0, "radius_m": R,
         "vehicle_load_share_pct": 100 * FRONT / 2},
        v0=100.0, duration=4.0, dt=DT, inertia=0.3))
    for e in proj.systems[0].elements:
        if e.id in ("w2", "w3"):
            e.parameterOverrides["vehicle_load_share_pct"] = 100 * (1 - FRONT) / 2
    proj.systems[0].elements.append(el("bk", "signal.constant", "Brake Command", value=0.2))
    proj.dataBusConnections += [dbc(60 + i, "bk", "sig_out", f"b{i}", "sig_demand_in")
                                for i in range(4)]
    result = simulate(proj, "case")
    v = _speeds(result)
    front = _values(result, "veh", "sig_load_front")
    checked = 0
    for i in range(20, len(v)):
        if v[i] * 3.6 < 10.0:
            break
        expected = M * (v[i - 1] - v[i]) / DT * H / L
        assert abs(front[i] - FRONT * M * GRAVITY - expected) <= 0.005 * expected
        checked += 1
    assert checked > 150 and expected > M * 10.0 * H / L  # 1.8 s braking at over 10 m/s²


def test_downforce_adds_half_rho_cza_v_squared():
    """Downforce ½·ρ·CzA·v² at the Ambient's air density, split by the aero
    balance (measured 5e-10 and 3e-9 on the rounded channels), and it adds
    to the rolling resistance (0.002 %; without it the car would slow 13 %
    less)."""
    rho, cza, m = air_density(-7.0), 3.0, 1500.0
    veh = {"mass_kg": m, "cd": 0.0, "downforce_cza_m2": cza, "aero_balance_front_pct": 40}
    result = simulate(_tag_axles(coast_project(veh, {"rolling_resistance": 0.0},
                                               ambient={"temperature_C": -7}, v0=120.0,
                                               duration=2.0, dt=0.1)), "case")
    v = _speeds(result)
    total = [sum(x) for x in zip(*(_values(result, f"w{i}", "sig_normal_load") for i in range(4)))]
    front = _values(result, "veh", "sig_load_front")
    assert v[-1] == pytest.approx(120 / 3.6, rel=1e-9)  # nothing slows it down
    for vi, fz, f_front in zip(v, total, front):
        down = 0.5 * rho * cza * vi * vi
        assert fz - m * GRAVITY == pytest.approx(down, rel=1e-3)
        assert f_front - m * GRAVITY / 2 == pytest.approx(0.4 * down, rel=1e-3)

    # with rolling resistance the downforce's load rolls too (drag mode)
    c_rr, inertia, r = 0.015, 0.001, 0.33
    result = simulate(_tag_axles(coast_project(veh, {"rolling_resistance": c_rr},
                                               ambient={"temperature_C": -7}, v0=120.0,
                                               duration=0.2, dt=0.1, inertia=inertia)), "case")
    v = _speeds(result)
    m_eff = m + 8 * inertia / r ** 2
    expected = c_rr * (m * GRAVITY + 0.5 * rho * cza * v[0] ** 2) / m_eff
    assert (v[0] - v[1]) / 0.1 == pytest.approx(expected, rel=5e-3)


@pytest.mark.parametrize("driven, h", [("Rear", 0.3), ("Front", 0.3), ("Rear", 0.0)])
def test_slip_limited_launch_follows_the_closed_form(driven, h):
    """With the driven wheels spinning at μ, the driven axle's load follows
    the acceleration: a = μ·m·g·s/(m + 2·J/r² ∓ μ·m·h/L), s the driven
    axle's static share, J the free wheels' inertia; the lag settles in a few
    steps (measured 6.447-6.448 against 6.446 m/s² rear-driven, 3.606-3.607
    against 3.606 front-driven). Before, both gave the h = 0 values, 5.24
    and 4.28 m/s²."""
    mu = 1.0
    result = simulate(fs_car(driven, mu=mu, h=h, fd=12.0, duration=1.0), "case")
    v = _speeds(result)
    s, sign = (1 - FRONT, 1) if driven == "Rear" else (FRONT, -1)
    closed = mu * M * GRAVITY * s / (M + 2 * J_FREE / R ** 2 - sign * mu * M * h / L)
    for i in range(31, 61):  # 0.3-0.6 s
        assert (v[i] - v[i - 1]) / DT == pytest.approx(closed, rel=5e-3)


def test_a_lifting_axle_carries_nothing():
    """A rear-driven car with its CG higher than it can hold the front down
    (h 1.4 m on a 1.55 m wheelbase at μ 1.5): the front wheels lift, the rear
    ones carry the whole weight, the acceleration stops at μ·g, and the run
    warns. Without the load-conserving clamp it swung between 12 and
    25 m/s²."""
    mu = 1.5
    result = simulate(fs_car(mu=mu, h=1.4, fd=12.0, duration=1.0), "case")
    v = _speeds(result)
    front, rear = _values(result, "veh", "sig_load_front"), _values(result, "veh", "sig_load_rear")
    for i in range(3, len(v)):  # lifted from the third step on
        assert front[i] == 0.0
        assert rear[i] == pytest.approx(M * GRAVITY, abs=1e-4)
        if i <= 50:  # later the motor's power limits it
            assert (v[i] - v[i - 1]) / DT == pytest.approx(mu * GRAVITY, rel=5e-3)
    assert result.status == "warning"
    assert any("front wheels of Vehicle 'Vehicle' lift off the road" in m.text
               for m in result.messages), result.messages


def test_the_slope_moves_load_to_the_rear():
    """Standing on a 20 % grade the weight's pull down the slope moves
    m·g·sin θ·h/L to the rear axle; coasting up it with nothing but the
    grade slowing the car moves nothing (a + g·sin θ = 0)."""
    m, h, wheelbase = 1500.0, 0.55, 2.7
    theta = math.atan(0.2)
    static = 0.5 * m * GRAVITY * math.cos(theta)
    for v0, transfer in ((0.0, m * GRAVITY * math.sin(theta) * h / wheelbase), (60.0, 0.0)):
        result = simulate(_tag_axles(coast_project(
            {"mass_kg": m, "cd": 0.0, "cg_height_m": h, "wheelbase_m": wheelbase},
            {"rolling_resistance": 0.0}, grade=20.0, v0=v0, duration=1.0, dt=DT)), "case")
        rear = _values(result, "veh", "sig_load_rear")
        # from the first step on: point 0 is recorded before the grade is read
        for x in rear[2:]:
            assert x == pytest.approx(static + transfer, rel=1e-8 if v0 == 0 else 1e-4)


def test_geometry_off_keeps_the_static_shares():
    """With the defaults (CG height 0, no downforce) each wheel carries its
    share of the weight normal to the road, as before, also launching up a
    10 % grade. A CG height given as a case value to a model whose wheels are
    all on one axle (which Data Checks cannot see) is warned about at run
    time and shifts nothing."""
    proj = bev_axle(profile="0:0; 1:50; 3:50")
    proj.systems[0].elements.append(el("grade", "signal.constant", "Grade", value=10.0))
    proj.dataBusConnections.append(dbc(80, "grade", "sig_out", "veh", "sig_grade_in"))
    proj.cases[0].duration, proj.cases[0].timeStep = 3.0, DT
    each = 0.5 * 1800.0 * GRAVITY * math.cos(math.atan(0.1))
    for case_values in ({}, {"veh": {"cg_height_m": 0.5}}):
        proj.cases[0].parameterOverrides = case_values
        result = simulate(proj, "case")
        assert max(_speeds(result)) > 10.0
        for wheel in ("whl", "whr"):
            loads = _values(result, wheel, "sig_normal_load")[1:]  # point 0: before the grade
            assert loads == pytest.approx([each] * len(loads), abs=1e-5)
        one_axle = [m.text for m in result.messages if "all its wheels are on one axle" in m.text]
        assert len(one_axle) == (1 if case_values else 0)
