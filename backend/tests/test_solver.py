"""Solver tests: maps, profiles, vehicle dynamics, differential modes,
battery ECM, scripts, and the bundled BEV example."""
import math

import pytest
from helpers import bev_axle, conn, dbc, el, project, series, sig_port

from app.schemas import ElementInstance
from app.solver import (
    ScriptError,
    compile_script,
    interp1,
    interp2,
    interp_profile,
    parse_profile,
    parse_table1d,
    parse_table2d,
    simulate,
)
from app.solver.maps import TableError
from app.storage import load_project
from app.validation import validate_project

# ---- maps -------------------------------------------------------------------

def test_table1d_parse_sort_and_interp():
    pts = parse_table1d({"500": 2.0, "0": 1.0, "1000": 4.0})
    assert [p[0] for p in pts] == [0.0, 500.0, 1000.0]
    assert interp1(pts, 250) == 1.5
    assert interp1(pts, -100) == 1.0  # clamped
    assert interp1(pts, 9999) == 4.0  # clamped


def test_table1d_rejects_bad_data():
    with pytest.raises(TableError):
        parse_table1d({"a": 1})
    with pytest.raises(TableError):
        parse_table1d({})
    with pytest.raises(TableError):
        parse_table1d({"1": 1, "1.0": 2})  # duplicate key


def test_table2d_bilinear_and_clamp():
    sheets = parse_table2d({
        "100": {"0": 10.0, "10": 20.0},
        "200": {"0": 30.0, "10": 40.0},
    })
    assert interp2(sheets, 100, 5) == 15.0
    assert interp2(sheets, 150, 5) == 25.0  # midway between sheets
    assert interp2(sheets, 50, 0) == 10.0  # outer clamp
    assert interp2(sheets, 300, 10) == 40.0


def test_profile_parsing():
    pts = parse_profile("0:15; 30:110; 120:95")
    assert pts == [(0.0, 15.0), (30.0, 110.0), (120.0, 95.0)]
    assert interp_profile(pts, 15, False) == 62.5
    assert interp_profile(pts, 999, False) == 95.0


# ---- recording controls (outputEvery + smaller step) -------------------------

def _bev_30s(**case_over):
    proj = bev_axle(profile="0:0; 5:60; 30:60")
    proj.cases[0].duration = 30
    proj.cases[0].timeStep = 0.5
    for k, v in case_over.items():
        setattr(proj.cases[0], k, v)
    return proj


def test_output_every_decimates_but_preserves_physics():
    full = simulate(_bev_30s(), "case")
    dec = simulate(_bev_30s(outputEvery=5), "case")

    n_full = len(series(full, "veh", "sig_speed"))
    n_dec = len(series(dec, "veh", "sig_speed"))
    assert n_dec == pytest.approx(n_full / 5, abs=2)  # ~1/5 the points
    # every channel is sampled on the same recorded grid
    assert all(len(c.timeSeries) == n_dec for c in dec.channels)
    # the final step is always kept
    assert series(dec, "veh", "sig_speed")[-1]["t"] == pytest.approx(30.0, abs=0.5)
    # decimation changes only the sampling, not the solve
    assert series(full, "veh", "sig_speed")[-1]["value"] == pytest.approx(
        series(dec, "veh", "sig_speed")[-1]["value"], abs=0.5)


def test_step_size_below_old_floor_allowed():
    proj = bev_axle(profile="0:0; 2:40; 5:40")
    proj.cases[0].duration = 5
    proj.cases[0].timeStep = 0.005  # finer than the previous 0.01 floor
    result = simulate(proj, "case")
    assert result.status in ("success", "warning"), [m.text for m in result.messages]
    # 5 s / 0.005 ≈ 1000 recorded points
    assert len(series(result, "veh", "sig_speed")) > 500


# ---- per-case parameter overrides (Phase 4) ---------------------------------

def _bev_case(mass=None):
    proj = bev_axle(profile="0:0; 5:60; 30:60")
    proj.cases[0].duration = 30
    if mass is not None:
        proj.cases[0].parameterOverrides = {"veh": {"mass_kg": mass}}
    return proj


def test_case_override_changes_result_non_destructively():
    """A case-level override layers on top of the element's own overrides:
    a heavier vehicle (same driving task) drains more energy — and the base
    element parameters are left untouched, so the override is non-destructive."""
    base = _bev_case()
    heavy = _bev_case(mass=3200)

    base_res = simulate(base, "case")
    heavy_res = simulate(heavy, "case")
    assert base_res.status in ("success", "warning"), [m.text for m in base_res.messages]
    assert heavy_res.status in ("success", "warning"), [m.text for m in heavy_res.messages]

    base_soc = series(base_res, "batt", "sig_soc")[-1]["value"]
    heavy_soc = series(heavy_res, "batt", "sig_soc")[-1]["value"]
    assert heavy_soc < base_soc - 0.05  # measurably more drain

    # overriding is non-destructive: the base vehicle element still has no
    # parameterOverrides of its own, and an empty case map is a no-op.
    assert heavy.systems[0].elements[0].parameterOverrides == {}
    heavy.cases[0].parameterOverrides = {}
    reset_soc = series(simulate(heavy, "case"), "batt", "sig_soc")[-1]["value"]
    assert reset_soc == pytest.approx(base_soc, abs=1e-6)


def test_case_override_sweep_is_monotonic():
    """The mechanism a parameter sweep rides on: stepping one overridden value
    yields ordered results (heavier ⇒ lower final SOC)."""
    socs = [
        series(simulate(_bev_case(mass=m), "case"), "batt", "sig_soc")[-1]["value"]
        for m in (1500, 2500, 3500)
    ]
    assert socs[0] > socs[1] > socs[2]


# ---- vehicle dynamics --------------------------------------------------------

def test_speed_tracking_and_regen():
    proj = bev_axle(profile="0:0; 5:80; 20:80; 25:0; 40:0")
    proj.cases[0].duration = 40
    result = simulate(proj, "case")
    assert result.status in ("success", "warning"), [m.text for m in result.messages]

    speed = {p["t"]: p["value"] for p in series(result, "veh", "sig_speed")}
    assert 75 < speed[20.0] < 85  # tracks the 80 km/h plateau
    assert speed[40.0] < 1.0  # comes to a stop

    # decel phase recuperates: motor torque goes negative
    torque = [p["value"] for p in series(result, "mot", "sig_torque")]
    assert min(torque) < -5

    soc = [p["value"] for p in series(result, "batt", "sig_soc")]
    assert soc[-1] < soc[0]


def test_standstill_holds_at_zero():
    proj = bev_axle(profile="0:0; 5:50; 15:50; 20:0; 60:0")
    proj.cases[0].duration = 60
    result = simulate(proj, "case")
    speed = [p["value"] for p in series(result, "veh", "sig_speed")]
    tail = speed[-10:]
    assert all(v < 0.2 for v in tail), f"vehicle should hold standstill, got {tail}"


def test_motor_steady_state_matches_road_load():
    """At constant 50 km/h the motor torque must equal road load through
    the gear chain — the physics regression anchor."""
    proj = bev_axle(profile="0:0; 5:50; 60:50")
    proj.cases[0].duration = 60
    result = simulate(proj, "case")
    t_motor = series(result, "mot", "sig_torque")[-1]["value"]

    v = 50 / 3.6
    mass = 1800.0
    f_roll = 0.012 * mass * 9.81 * 0.5  # two wheels à 25 % share
    f_aero = 0.5 * 1.2 * (0.28 * 2.2) * v * v  # Cd × frontal area
    wheel_torque = (f_roll + f_aero) * 0.33
    rpm = v / 0.33 * 9.7 * 60 / (2 * math.pi)
    drag = interp1(parse_table1d({"0": 0, "3000": 1.2, "6000": 2.6, "9000": 4.2, "12000": 6.0}), rpm)
    expected = wheel_torque / (9.7 * 0.97 * 0.98) + drag
    assert abs(t_motor - expected) < 1.5, f"{t_motor} vs {expected}"


# ---- differential ------------------------------------------------------------

def _final_speed(locked: bool, mu_left: float) -> tuple[float, float]:
    result = simulate(bev_axle(locked=locked, mu_left=mu_left), "case")
    v = series(result, "veh", "sig_speed")[-1]["value"]
    slip_l = max(abs(p["value"]) for p in series(result, "whl", "sig_slip"))
    return v, slip_l


def test_diff_locked_equals_open_on_uniform_grip():
    v_open, _ = _final_speed(False, 1.0)
    v_lock, _ = _final_speed(True, 1.0)
    assert abs(v_open - v_lock) < 1.0


def test_diff_modes_differ_on_split_mu():
    v_open, slip_open = _final_speed(False, 0.1)
    v_lock, _ = _final_speed(True, 0.1)
    assert slip_open > 0.5, "open diff must spin up the icy wheel"
    assert v_lock > v_open + 3, "locked diff must out-accelerate open on split-mu"


def test_diff_open_torque_split_is_equal():
    result = simulate(bev_axle(locked=False, mu_left=0.1), "case")
    ta = series(result, "diff", "sig_torque_a")
    tb = series(result, "diff", "sig_torque_b")
    for a, b in zip(ta, tb):
        assert abs(a["value"] - b["value"]) < 1e-6


# ---- battery -----------------------------------------------------------------

def test_battery_min_soc_blocks_discharge():
    proj = bev_axle(profile="0:0; 5:100; 600:100")
    proj.cases[0].duration = 600
    proj.cases[0].timeStep = 1.0
    for e in proj.systems[0].elements:
        if e.id == "batt":
            e.parameterOverrides.update({"capacity_kWh": 0.4, "initial_soc_pct": 30})
    result = simulate(proj, "case")
    soc = [p["value"] for p in series(result, "batt", "sig_soc")]
    assert soc[-1] >= 9.9, "SOC must not drop (visibly) below the 10 % floor"
    assert any("minimum SOC" in m.text for m in result.messages)


def test_battery_rc_pair_adds_voltage_sag():
    def run(rc_r, rc_tau):
        proj = bev_axle(profile="0:0; 5:100; 30:100")
        for e in proj.systems[0].elements:
            if e.id == "batt":
                e.parameterOverrides.update(
                    {"rc_resistance_ohm": rc_r, "rc_time_constant_s": rc_tau})
        result = simulate(proj, "case")
        return min(p["value"] for p in series(result, "batt", "sig_voltage"))

    assert run(0.06, 10.0) < run(0.0, 0.0) - 0.5


# ---- scripts -----------------------------------------------------------------

def _scripted_axle(code: str):
    proj = bev_axle()
    proj.systems[0].elements.append(ElementInstance(
        id="scr", componentDefId="signal.script", label="Limiter",
        position={"x": 0, "y": 0}, parameterOverrides={"code": code},
        dynamicPorts=[sig_port("cmd_in", "input"), sig_port("cmd_out", "output")],
    ))
    proj.dataBusConnections = [
        dbc(1, "task", "sig_demand", "drv", "sig_target_in"),
        dbc(90, "veh", "sig_speed", "drv", "sig_speed_in"),
        dbc(2, "drv", "sig_traction_cmd", "scr", "cmd_in"),
        dbc(3, "scr", "cmd_out", "mot", "sig_demand_in"),
    ]
    return proj


def test_script_reroutes_traction_command():
    code = ("def step(t, dt, inputs, state, params):\n"
            "    state['n'] = state.get('n', 0) + 1\n"
            "    return {'cmd_out': clamp(inputs['cmd_in'], -1, 0.25)}\n")
    result = simulate(_scripted_axle(code), "case")
    assert result.status in ("success", "warning")
    out = [p["value"] for p in series(result, "scr", "cmd_out")]
    assert 0 < max(out) <= 0.2501


def test_script_runtime_error_fails_run():
    code = ("def step(t, dt, inputs, state, params):\n"
            "    if t > 5:\n"
            "        raise ValueError('boom')\n"
            "    return {'cmd_out': 0.1}\n")
    result = simulate(_scripted_axle(code), "case")
    assert result.status == "failed"
    assert any("boom" in m.text for m in result.messages)


def test_compile_script_requires_step():
    with pytest.raises(ScriptError):
        compile_script("x = 1", "S")


# ---- validation ---------------------------------------------------------------

def test_diff_missing_output_is_error():
    proj = bev_axle()
    proj.systems[0].connections = [c for c in proj.systems[0].connections if c.id != "c6"]
    checks = validate_project(proj)
    assert any("both outputs" in c.text for c in checks if c.level == "error")


def test_dynamic_ports_rejected_on_plain_components():
    proj = bev_axle()
    proj.systems[0].elements[0].dynamicPorts = [sig_port("x", "input")]
    checks = validate_project(proj)
    assert any("does not allow" in c.text for c in checks if c.level == "error")


def test_bad_table_is_reported():
    proj = bev_axle()
    for e in proj.systems[0].elements:
        if e.id == "mot":
            e.parameterOverrides["drag_torque"] = {"not-a-number": 1.0}
    checks = validate_project(proj)
    assert any("not numeric" in c.text for c in checks if c.level == "error")


# ---- bundled example -----------------------------------------------------------

def test_bev_demo_validates_and_runs():
    proj = load_project("bev-car")
    checks = validate_project(proj)
    assert not [c for c in checks if c.level == "error"], [c.text for c in checks]

    result = simulate(proj, "case-city")
    assert result.status == "success", [m.text for m in result.messages]

    speed = {p["t"]: p["value"] for p in series(result, "el-vehicle", "sig_speed")}
    assert 45 < speed[120.0] < 55
    assert 75 < speed[300.0] < 85
    assert speed[600.0] < 1.0

    soc = [p["value"] for p in series(result, "el-battery", "sig_soc")]
    assert 85 < soc[-1] < 90

    labels = [s.label for s in result.summary]
    assert any("Consumption" in label for label in labels)
    assert any("Distance driven" in label for label in labels)
