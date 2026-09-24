"""Numerics: the solver against exact answers, not against its own past.

Closed-form reference cases (each within 0.5 %), convergence order (the
error halves when the step halves) and a stability grid over slip stiffness
× solver step. Every parameter an expected value depends on is set here, at
the library value of 0.2.0 where the case does not need its own, so a change
of a library default cannot move an expectation."""
import math

import pytest
from helpers import bev_axle, conn, dbc, el, project, series

import app.solver.core as core
from app.solver import simulate
from app.solver.runtime import AIR_DENSITY, GRAVITY

RPM = 60.0 / (2.0 * math.pi)
# Loss-free maps over the whole range a motor runs in (0-12000 1/min,
# 0-400 N·m), so a map-edge policy that stops a run outside its data leaves
# them alone.
ZERO_LOSS = {"0": {"0": 0, "400": 0}, "12000": {"0": 0, "400": 0}}
ZERO_DRAG = {"0": 0, "12000": 0}
FLAT_TORQUE = {v: {"0": 345.6, "12000": 345.6} for v in ("250", "400")}


def _run(proj):
    result = simulate(proj, "case")
    assert result.status != "failed", [m.text for m in result.messages]
    return result


def _v(result):
    return [(p["t"], p["value"] / 3.6) for p in series(result, "veh", "sig_speed")]


def _pin(proj, *pins):
    """Set parameters per element id: {element id: {key: value}}."""
    for e in proj.systems[0].elements:
        for pin in pins:
            e.parameterOverrides.update(pin.get(e.id, {}))
    return proj


# ---- coast-down ---------------------------------------------------------------

M, CD, AREA, V0 = 1500.0, 0.30, 2.0, 100 / 3.6
K_AERO = 0.5 * AIR_DENSITY * CD * AREA  # N/(m/s)², at the engine's own air density


def _lone_body(step, duration=60.0, **veh):
    """A vehicle body without wheels: only aero (and grade) act on it."""
    veh = {"initial_speed_kmh": V0 * 3.6, "cd": CD, "frontal_area_m2": AREA, "mass_kg": M, **veh}
    return project([el("veh", "vehicle.body", "Vehicle", **veh)], [], [],
                   duration=duration, time_step=step)


def _aero_coast_errors(step):
    """(max relative speed error, relative distance error) against
    v = v0 / (1 + k v0 t), x = ln(1 + k v0 t) / k with k = ½ρCdA / m."""
    result = _run(_lone_body(step))
    k = K_AERO / M
    ev = max(abs(v - V0 / (1 + k * V0 * t)) * (1 + k * V0 * t) / V0 for t, v in _v(result))
    x_end = series(result, "veh", "sig_distance")[-1]
    x_exact = math.log(1 + k * V0 * x_end["t"]) / k
    return ev, abs(x_end["value"] - x_exact) / x_exact


def test_aero_coast_down_follows_its_closed_form():
    ev, ex = _aero_coast_errors(0.01)
    assert ev < 0.005 and ex < 0.005, (ev, ex)


R, JW, JD, CRR = 0.33, 1.2, 0.015, 0.012


def _free_axle(step, duration, k_slip=10, crr=CRR, cd=CD, brake_nm=None, j_brake=0.14, v0_kmh=100):
    """Vehicle on one undriven axle (differential + two wheels), optionally a
    brake on the differential input held at a constant command of 1. The
    wheels keep the default 25 % load share each, so the engine's scaling of
    the shares to 100 % is part of every case built here."""
    els = [el("veh", "vehicle.body", "Vehicle", initial_speed_kmh=v0_kmh, cd=cd,
              frontal_area_m2=AREA, mass_kg=M),
           el("diff", "mech.differential", "Diff", ratio=1.0, inertia_kgm2=JD, efficiency_pct=100),
           *[el(w, "propulsion.wheel", w, radius_m=R, inertia_kgm2=JW, mu=1.0,
                rolling_resistance=crr, slip_stiffness=k_slip)
             for w in ("whl", "whr")]]
    cons = [conn(1, "diff", "flange_out_a", "whl", "shaft"),
            conn(2, "diff", "flange_out_b", "whr", "shaft")]
    bus = []
    if brake_nm is not None:
        els += [el("brk", "mech.brake", "Brake", max_torque_Nm=brake_nm, inertia_kgm2=j_brake),
                el("ped", "signal.constant", "Pedal", value=1)]
        cons.append(conn(3, "brk", "flange", "diff", "flange_in"))
        bus.append(dbc(1, "ped", "sig_out", "brk", "sig_demand_in"))
    return project(els, cons, bus, duration=duration, time_step=step)


def test_coast_down_with_rolling_resistance_follows_its_closed_form():
    """F = A + C v² with A = c_rr m g, C = ½ρCdA on the whole rolling mass
    (the wheels and differential spin with it):
    v(t) = √(A/C) tan(atan(v0 √(C/A)) − √(AC) t / m_eff)."""
    a, c = CRR * M * GRAVITY, K_AERO
    m_eff = M + (2 * JW + JD) / R**2
    result = _run(_free_axle(0.01, 60.0))
    for t, v in _v(result):
        exact = math.sqrt(a / c) * math.tan(
            math.atan(V0 * math.sqrt(c / a)) - math.sqrt(a * c) * t / m_eff)
        assert v == pytest.approx(exact, rel=0.005), t


# ---- constant force, braking, grade --------------------------------------------

RATIO, J_MOT, J_FD_IN, J_FD_OUT = 9.7, 0.045, 0.01, 0.02
AXLE = {  # bev_axle's inertias, ratios and radii
    "mot": {"inertia_kgm2": J_MOT},
    "fd": {"ratio": RATIO, "inertia_in_kgm2": J_FD_IN, "inertia_out_kgm2": J_FD_OUT},
    "diff": {"ratio": 1.0, "inertia_kgm2": JD},
    **{w: {"radius_m": R, "inertia_kgm2": JW, "mu": 1.0} for w in ("whl", "whr")},
}


def test_constant_torque_launch_accelerates_uniformly():
    """A quarter of a flat 345.6 N·m full-load torque through a lossless
    chain: v = F t / m_eff."""
    proj = bev_axle()
    proj.systems[0].elements = [e for e in proj.systems[0].elements if e.id not in ("drv", "task")]
    proj.systems[0].elements.append(el("cmd", "signal.constant", "Demand", value=0.25))
    proj.dataBusConnections = [dbc(1, "cmd", "sig_out", "mot", "sig_demand_in")]
    proj.cases[0].duration, proj.cases[0].timeStep = 4.0, 0.01
    _pin(proj, AXLE, {
        "veh": {"cd": 0.0, "mass_kg": M},
        "mot": {"full_load_torque": FLAT_TORQUE, "power_loss": ZERO_LOSS, "drag_torque": ZERO_DRAG},
        "fd": {"efficiency_pct": 100},
        "diff": {"efficiency_pct": 100},
        "batt": {"internal_resistance_ohm": 1e-6, "capacity_kWh": 60},
        **{w: {"rolling_resistance": 0.0, "slip_stiffness": 10} for w in ("whl", "whr")},
    })
    m_eff = M + ((J_MOT + J_FD_IN) * RATIO**2 + J_FD_OUT + JD + 2 * JW) / R**2
    accel = 0.25 * 345.6 * RATIO / R / m_eff
    result = _run(proj)
    for t, v in _v(result):
        if t >= 1.0:  # after the tyres take up their slip
            assert v == pytest.approx(accel * t, rel=0.005), t


def test_braking_distance_is_v0_squared_over_2a():
    t_brake, j_brake = 1400.0, 0.14
    m_eff = M + (2 * JW + JD + j_brake) / R**2
    decel = t_brake / R / m_eff
    result = _run(_free_axle(0.01, 12.0, crr=0.0, cd=0.0, brake_nm=t_brake, j_brake=j_brake))
    stop = V0 / decel
    x_stop = next(p["value"] for p in series(result, "veh", "sig_distance") if p["t"] >= stop + 0.5)
    assert x_stop == pytest.approx(V0**2 / (2 * decel), rel=0.005)


@pytest.mark.xfail(strict=True, reason="until MOD-11 (exact slope force)")
def test_coasting_up_a_grade_turns_kinetic_energy_into_m_g_h():
    """A 25 % grade (tan and sin differ by 3 %): the body stops after rising
    h = v0² / 2g, i.e. after v0² / (2 g sin(atan 0.25)) along the road.
    Today the slope force is m g grade/100, 3.1 % too strong; the change
    that makes it m g sin(atan(grade/100)) removes the marker."""
    grade, v0 = 25.0, 72 / 3.6
    proj = _lone_body(0.01, duration=12.0, cd=0.0, initial_speed_kmh=72)
    proj.systems[0].elements.append(el("g", "signal.constant", "Grade", value=grade))
    proj.dataBusConnections.append(dbc(1, "g", "sig_out", "veh", "sig_grade_in"))
    x = series(_run(proj), "veh", "sig_distance")[-1]["value"]
    assert x * math.sin(math.atan(grade / 100)) == pytest.approx(v0**2 / (2 * GRAVITY), rel=0.005)


# ---- clutch --------------------------------------------------------------------

def test_clutch_engagement_loses_the_two_inertia_energy():
    """Motor (J1) spun up with the clutch open, torque off, clutch closed
    onto a free inertia (J2): momentum is kept, (J1 w1)/(J1+J2), and
    ½ J1 J2/(J1+J2) w1² is lost — in the clutch, as its torque × slip.

    The motor's and the clutch's channels hold each step's operating point
    (its start), so the torque × slip sum is one step early: the 0.5 % band
    is wider than that, and this test does not guard channel timing."""
    j1, j2 = 0.045, 0.5
    els = [el("src", "electric.voltage_source", "Supply", voltage_V=350),
           el("bus", "electric.node", "Bus"),
           el("mot", "motor.emotor", "Motor", inertia_kgm2=j1, full_load_torque=FLAT_TORQUE,
              power_loss=ZERO_LOSS, drag_torque=ZERO_DRAG),
           el("clu", "mech.clutch", "Clutch", max_torque_Nm=1.0),
           el("load", "propulsion.propeller", "Inertia", torque_ref_Nm=0, inertia_kgm2=j2),
           el("dem", "signal.driving_task", "Demand", profile="0:0.01; 1:0.01; 1.001:0"),
           el("eng", "signal.driving_task", "Engage", profile="0:0; 1.5:0; 1.501:1")]
    cons = [conn(1, "src", "pos", "bus", "t1"), conn(2, "bus", "t2", "mot", "pos"),
            conn(3, "mot", "shaft", "clu", "flange_a"), conn(4, "clu", "flange_b", "load", "shaft")]
    bus = [dbc(1, "dem", "sig_demand", "mot", "sig_demand_in"),
           dbc(2, "eng", "sig_demand", "clu", "sig_engage_in")]
    result = _run(project(els, cons, bus, duration=6.0, time_step=0.01))
    w_mot, w_load = series(result, "mot", "sig_speed"), series(result, "load", "sig_speed")
    w1 = next(p["value"] for p in w_mot if p["t"] >= 1.4) / RPM
    assert w1 > 50  # spun up; the load still at rest
    w_end = (w_mot[-1]["value"] / RPM, w_load[-1]["value"] / RPM)
    assert w_end[0] == pytest.approx(j1 * w1 / (j1 + j2), rel=1e-6)
    assert w_end[1] == pytest.approx(j1 * w1 / (j1 + j2), rel=1e-6)
    loss = 0.5 * j1 * j2 / (j1 + j2) * w1**2
    ke_lost = 0.5 * j1 * w1**2 - 0.5 * (j1 + j2) * w_end[0] ** 2
    assert ke_lost == pytest.approx(loss, rel=0.005)
    torque, slip = series(result, "clu", "sig_torque"), series(result, "clu", "sig_slip_speed")
    in_clutch = sum(tq["value"] * s["value"] / RPM for tq, s in zip(torque[1:], slip[1:])) * 0.01
    assert in_clutch == pytest.approx(loss, rel=0.005)


# ---- battery -------------------------------------------------------------------
# The constant-current discharge check (1C empties a full battery in 3600 s)
# is MOD-38's, in test_battery_charge.py: it passes only once SOC counts
# amp-hours instead of watt-hours.

def _battery_at_constant_current(amps, ocv, step, duration, **battery):
    """A battery feeding a Constant Drive whose power a Lookup sets to
    amps × terminal voltage (one solver step behind): a constant current."""
    els = [el("batt", "battery.generic", "Battery", ocv_table=ocv, **battery),
           el("bus", "electric.node", "Bus"),
           el("load", "electric.constant_drive", "Load"),
           el("lut", "signal.lookup", "I x V", mode="1D", sample_time_s=0,
              table_1d={"0": 0, "1000": amps})]  # kW at 1000 V
    cons = [conn(1, "batt", "pos", "bus", "t1"), conn(2, "bus", "t2", "load", "pos")]
    bus = [dbc(1, "batt", "sig_voltage", "lut", "sig_x_in"),
           dbc(2, "lut", "sig_out", "load", "sig_demand_in")]
    return project(els, cons, bus, duration=duration, time_step=step)


I_STEP, R1, TAU, E0 = 100.0, 0.05, 5.0, 350.0


def _rc_error(step):
    """Largest |v_RC − I R1 (1 − e^(−t/τ))| over a current step, as a
    fraction of I R1 (flat OCV, R0 ≈ 0: v_RC = OCV − terminal voltage)."""
    result = _run(_battery_at_constant_current(
        I_STEP, {"0": E0, "100": E0}, step, 20.0, internal_resistance_ohm=1e-6,
        rc_resistance_ohm=R1, rc_time_constant_s=TAU, capacity_kWh=60, initial_soc_pct=90,
        min_soc_pct=0))
    return max(abs(E0 - p["value"] - I_STEP * R1 * (1 - math.exp(-p["t"] / TAU)))
               for p in series(result, "batt", "sig_voltage")) / (I_STEP * R1)


def test_rc_branch_step_response_is_first_order():
    # the battery's voltage channel holds each step's start, one step behind
    # the current (dt/τ = 0.2 % of the error); this does not guard that timing
    assert _rc_error(0.01) < 0.005


# ---- convergence order ------------------------------------------------------------

@pytest.mark.parametrize("error_of", [lambda h: _aero_coast_errors(h)[1], _rc_error],
                         ids=["vehicle (explicit)", "battery RC (implicit)"])
def test_error_halves_when_the_step_halves(error_of):
    """First-order integrators: each halving of the solver step at least
    ~halves the error against the exact answer. A quantity stuck at a hidden
    fixed step (as the controllers were before 0.2.0) would not converge."""
    errors = [error_of(h) for h in (0.01, 0.005, 0.0025)]
    assert errors[0] / errors[1] > 1.8 and errors[1] / errors[2] > 1.8, errors


# ---- stability grid -------------------------------------------------------------
# Each grid test asserts that the set of unstable (step, slip stiffness) cells
# equals the set recorded here, so it ratchets both ways: a change that makes a
# stable cell unstable fails, and so does one that makes an unstable cell
# stable, until that cell is deleted from the set (the gain is then kept).
# When a grid test fails, its message lists the cells that moved. If the move
# is intended (ENG-09 empties both sets; ENG-14 may change the 20 ms column),
# edit the set in the same change and say why. Stable and unstable cells sit
# about 2x or more from their thresholds (launch: max slip at most 0.051 or at
# least 1.56, against 0.1; hold: at most 0.2 km/h or at least 1.8 km/h,
# against 0.5), so a small model change does not flip one by accident.

STEPS = (0.0025, 0.005, 0.01, 0.02)  # 20 ms is past today's 10 ms cap: ENG-09's range
STIFFNESS = (10, 30, 100, 300)
UNSTABLE_LAUNCH = {(0.0025, 100), (0.0025, 300), (0.005, 100), (0.005, 300), (0.01, 30),
                   (0.01, 100), (0.01, 300), (0.02, 10), (0.02, 30), (0.02, 100), (0.02, 300)}
UNSTABLE_HOLD = {(0.0025, 100), (0.0025, 300), (0.005, 100), (0.005, 300), (0.01, 30),
                 (0.01, 100), (0.01, 300), (0.02, 30), (0.02, 100), (0.02, 300)}


def _unstable_cells(monkeypatch, build, bad):
    """(step, slip stiffness) cells where bad(result) holds; steps above the
    10 ms cap are reached by raising the cap."""
    cells = set()
    for h in STEPS:
        monkeypatch.setattr(core, "MAX_SUBSTEP", h)
        for k in STIFFNESS:
            if bad(_run(build(max(h, 0.01), k))):
                cells.add((h, k))
    return cells


def _moved(cells, recorded):
    return f"newly unstable {sorted(cells - recorded)}, now stable {sorted(recorded - cells)}"


def _launch(case_step, k_slip):
    proj = bev_axle(profile="0:0; 5:100; 30:100")
    proj.cases[0].duration, proj.cases[0].timeStep = 10.0, case_step
    return _pin(proj, AXLE, {
        "veh": {"mass_kg": 1800, "cd": 0.28, "frontal_area_m2": 2.2},
        **{w: {"slip_stiffness": k_slip, "rolling_resistance": CRR} for w in ("whl", "whr")},
    })


def test_launch_slip_stays_physical(monkeypatch):
    """A launch to 100 km/h well inside the grip limit: slip stays near
    force / (load x stiffness), below 0.1, and finite (ENG-09's metric)."""
    def bad(result):
        slips = [abs(p["value"]) for w in ("whl", "whr") for p in series(result, w, "sig_slip")]
        return not all(math.isfinite(s) for s in slips) or max(slips) >= 0.1
    cells = _unstable_cells(monkeypatch, _launch, bad)
    assert cells == UNSTABLE_LAUNCH, _moved(cells, UNSTABLE_LAUNCH)


def test_braked_car_stays_at_rest(monkeypatch):
    """Braked from 100 km/h with 1400 N·m held: once stopped (9.98 s) the car
    must stay below 0.5 km/h (today the unstable cells creep at 2-47 km/h)."""
    def build(case_step, k_slip):
        return _free_axle(case_step, 20.0, k_slip=k_slip, crr=0.0, cd=0.0, brake_nm=1400.0)

    def bad(result):
        return not max(v for t, v in _v(result) if t > 11.0) * 3.6 < 0.5
    cells = _unstable_cells(monkeypatch, build, bad)
    assert cells == UNSTABLE_HOLD, _moved(cells, UNSTABLE_HOLD)
