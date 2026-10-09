"""The acceleration test (STU-37): SimCase.kind "acceleration".

The Driver holds full throttle for the whole run without a target, the run
ends at the end of the solver step in which the vehicle has driven
startLine + endDistance, and the time from the start line to that line and
the speed there are read inside the steps that crossed them, so they do not
depend on the output step. The case duration is the time limit. FS Rules
2026 v1.1 (FSG): 75 m from the start line (D 5.1.1), the car staged 0.30 m
behind it (D 5.2.3)."""
import bisect

import pytest
from fastapi.testclient import TestClient
from helpers import bev_axle, conn, el, series

from app.library import load_library
from app.main import app
from app.solver import simulate
from app.solver.maps import parse_table2d
from app.solver.network import build_model
from app.solver.runtime import AIR_DENSITY, GRAVITY, RPM
from app.storage import load_example

WHEELS = ("el-wheel-fl", "el-wheel-fr", "el-wheel-rl", "el-wheel-rr")
BRAKES = ("el-brake-fl", "el-brake-fr", "el-brake-rl", "el-brake-rr")
BAT = "HV Battery Pack"


def _bev(step=0.01, start=0.0, reference=None, duration=25.0, overrides=None):
    """The BEV example's first case as a 75 m acceleration test from rest."""
    proj = load_example("bev-car")
    case = proj.cases[0]
    case.kind, case.endDistance, case.startLine = "acceleration", 75.0, start
    case.referenceTime, case.duration, case.timeStep = reference, duration, step
    case.parameterOverrides = overrides or {}
    return proj


def _rows(result):
    return {s.label: s for s in result.summary}


def _values(result, el_id, port):
    return [p["value"] for p in series(result, el_id, port)]


def _at(ts, xs, level, ys=None):
    """Linear interpolation of ``ys`` (default: the time) where ``xs`` first reaches ``level``."""
    ys = ts if ys is None else ys
    k = next(k for k, x in enumerate(xs) if x >= level)
    return ys[k - 1] + (level - xs[k - 1]) / (xs[k] - xs[k - 1]) * (ys[k] - ys[k - 1])


def _point_mass(proj, case, d0, d1, dt=1e-4):
    """RK4 of a slip-free point mass, m_eff·dv/dt = T_full(n)·i·η/r − F_roll − F_aero,
    from rest: (time from d0 to d1 m, speed at d1 in km/h). m_eff lumps the
    wheel-side and the motor-side inertias through the reduction."""
    pp = build_model(proj, {}, case.parameterOverrides).params_of
    veh, fd, dif, mot = pp["el-vehicle"], pp["el-final-drive"], pp["el-diff"], pp["el-motor"]
    r = pp["el-wheel-fl"]["radius_m"]
    i = fd["ratio"] * dif["ratio"]
    eta = fd["efficiency_pct"] / 100 * dif["efficiency_pct"] / 100
    j_wheels = (sum(pp[w]["inertia_kgm2"] for w in WHEELS)
                + sum(pp[b]["inertia_kgm2"] for b in BRAKES)
                + dif["inertia_kgm2"] + fd["inertia_out_kgm2"])
    j_motor = mot["inertia_kgm2"] + fd["inertia_in_kgm2"]
    m = veh["mass_kg"]
    m_eff = m + (j_wheels + j_motor * i * i) / (r * r)
    cda = veh["cd"] * veh["frontal_area_m2"]
    crr = pp["el-wheel-fl"]["rolling_resistance"]  # the same on all four
    full = dict(parse_table2d(mot["full_load_torque"]))[330.0]  # rows at ≥ 330 V are the same
    rpms = [x for x, _ in full]

    def t_full(rpm):
        k = min(max(1, bisect.bisect_left(rpms, rpm)), len(full) - 1)
        (x0, y0), (x1, y1) = full[k - 1], full[k]
        return y0 + (rpm - x0) / (x1 - x0) * (y1 - y0)

    def acc(v):
        f_drive = t_full(v / r * i * RPM) * i * eta / r
        f_roll = crr * m * GRAVITY * min(1.0, v / 0.3)  # the engine's taper at rest
        return (f_drive - f_roll - 0.5 * AIR_DENSITY * cda * v * v) / m_eff

    t = x = v = 0.0
    crossed = {}
    while d1 not in crossed:
        k1 = acc(v)
        k2 = acc(v + 0.5 * dt * k1)
        k3 = acc(v + 0.5 * dt * k2)
        k4 = acc(v + dt * k3)
        v_new = v + dt / 6 * (k1 + 2 * k2 + 2 * k3 + k4)
        x_new = x + dt / 6 * (v + 2 * (v + 0.5 * dt * k1) + 2 * (v + 0.5 * dt * k2)
                              + v + dt * k3)
        for d in (d0, d1):
            if d not in crossed and x_new >= d:
                f = (d - x) / (x_new - x)
                crossed[d] = (t + f * dt, (v + f * (v_new - v)) * 3.6)
        t, x, v = t + dt, x_new, v_new
    return crossed[d1][0] - crossed[d0][0], crossed[d1][1]


def _fs_car(mu=1.5, target=False, step=0.01):
    """A Formula Student-sized rear-drive car from bev_axle: 280 kg, CdA
    1.21 m², 0.23 m tyres, 55 % of the weight on the driven wheels and two
    free wheels, each on a brake, at the front. No target wired unless
    ``target``: an acceleration test needs none."""
    proj = bev_axle(profile="0:250; 100:250")
    els = proj.systems[0].elements
    for e in els:
        ov = e.parameterOverrides
        if e.id == "veh":
            ov.update(mass_kg=280, cd=1.1, frontal_area_m2=1.1)
        elif e.id in ("whl", "whr"):
            ov.update(radius_m=0.23, inertia_kgm2=0.3, vehicle_load_share_pct=27.5, mu=mu,
                      axle="Rear")
        elif e.id == "fd":
            ov.update(ratio=3.2)
    for i, side in enumerate(("fl", "fr")):
        els += [el(f"w{side}", "propulsion.wheel", f"Wheel {side.upper()}", radius_m=0.23,
                   inertia_kgm2=0.3, vehicle_load_share_pct=22.5, mu=mu),
                el(f"b{side}", "mech.brake", f"Brake {side.upper()}", inertia_kgm2=0.05)]
        proj.systems[0].connections.append(conn(20 + i, f"b{side}", "flange", f"w{side}", "shaft"))
    if not target:
        proj.dataBusConnections = [d for d in proj.dataBusConnections
                                   if d.port2Id != "sig_target_in"]
    case = proj.cases[0]
    case.kind, case.endDistance, case.startLine = "acceleration", 75.0, 0.3
    case.duration, case.timeStep = 25.0, step
    return proj


def test_a_75_m_run_without_slip_matches_a_point_mass():
    """The item's metric: with the tyres' grip made unlimited (μ 100, slip
    stiffness 100, at a 1 ms step) the timed 75 m matches a hand-integrated
    point mass within 1 % (measured +0.10 %), and so does the speed at the
    line (−0.18 %). At 100 % SOC the terminal voltage stays at or above
    330 V, where the motor's full-load rows are the same."""
    ov = {w: {"mu": 100.0, "slip_stiffness": 100.0} for w in WHEELS}
    ov["el-battery"] = {"initial_soc_pct": 100.0}
    proj = _bev(step=0.001, start=0.3, overrides=ov)
    result = simulate(proj, proj.cases[0].id)
    assert result.status == "success", [m.text for m in result.messages]
    t_pm, v_pm = _point_mass(proj, proj.cases[0], 0.3, 75.3)
    s = _rows(result)
    assert s["Time to 75 m"].value == pytest.approx(t_pm, rel=0.01)
    assert s["Speed at 75 m"].value == pytest.approx(v_pm, rel=0.01)
    assert min(_values(result, "el-battery", "sig_voltage")) >= 330.0


def test_the_run_ends_at_the_line_and_is_timed_inside_the_last_step():
    """The run stops at the end of the solver step that reaches 75 m, and the
    time and speed are read inside that step, so the output step does not
    change them. The time equals where the distance channel of a 10 ms
    full-throttle performance run (which goes on past the line) crosses
    75 m."""
    fine = simulate(_bev(step=0.01), "case-city")
    coarse = simulate(_bev(step=0.1), "case-city")
    for label in ("Time to 75 m", "Speed at 75 m"):
        assert _rows(fine)[label].value == _rows(coarse)[label].value
    s = _rows(fine)
    # (5.527 s and 86.11 km/h before the distance was the trapezoid of each
    # step's speeds: the speed at the step's end, held over the step, ran
    # 0.12 m ahead of it by the line)
    assert s["Time to 75 m"].value == pytest.approx(5.533, abs=0.002)
    assert s["Speed at 75 m"].value == pytest.approx(86.16, abs=0.02)
    dist = _values(fine, "el-vehicle", "sig_distance")
    v_line = s["Speed at 75 m"].value / 3.6
    assert 75.0 <= dist[-1] <= 75.0 + v_line * 0.01 + 1e-4
    assert s["Simulated duration"].value < 5.6
    assert s["Time to 75 m"].limit == 25.0 and s["Time to 75 m"].passed is True
    assert fine.status == "success", [m.text for m in fine.messages]
    assert any(m.level == "info" and "solved: 554 of 2500 steps × 0.01 s, ended at 75 m "
               "driven at t = 5.54 s" in m.text
               for m in fine.messages)

    perf = load_example("bev-car")
    case = perf.cases[0]
    case.kind, case.duration, case.timeStep = "performance", 8.0, 0.01
    case.parameterOverrides = {"el-task": {"profile": "0:250; 100:250"}}
    ref = simulate(perf, case.id)
    ts = [p["t"] for p in series(ref, "el-vehicle", "sig_distance")]
    assert s["Time to 75 m"].value == pytest.approx(
        _at(ts, _values(ref, "el-vehicle", "sig_distance"), 75.0), abs=0.002)


def test_the_start_line_starts_the_timer_and_the_gap_is_signed():
    """Staged 0.3 m behind the start line (FS Rules 2026 v1.1 (FSG) D 5.2.3), the
    time runs from 0.3 m to 75.3 m driven: 5.216 s against 5.533 s from
    rest. The gap to a reference time is positive when slower."""
    result = simulate(_bev(start=0.3, reference=5.0), "case-city")
    ts = [p["t"] for p in series(result, "el-vehicle", "sig_distance")]
    dist = _values(result, "el-vehicle", "sig_distance")
    s = _rows(result)
    assert s["Time to 75 m"].value == pytest.approx(_at(ts, dist, 75.3) - _at(ts, dist, 0.3),
                                                    abs=1e-3)
    assert s["Time to 75 m"].value == pytest.approx(5.216, abs=0.002)
    assert s["Gap to reference time"].value == pytest.approx(s["Time to 75 m"].value - 5.0,
                                                             abs=1e-3)
    assert s["Gap to reference time"].value > 0


def test_full_throttle_needs_no_target():
    """With no Target Speed wired, the Driver of an acceleration test still
    holds full throttle (a cycle's Driver holds 0 km/h and the car never
    moves), and the run is not judged against a cycle."""
    result = simulate(_fs_car(), "case")
    texts = [m.text for m in result.messages]
    assert result.status == "success", texts
    assert not any("Target Speed" in t or "Cycle not followed" in t for t in texts)
    assert all(v == 1.0 for v in _values(result, "drv", "sig_accel_pedal")[1:])
    s = _rows(result)
    assert s["Time to 100 km/h"].value == pytest.approx(3.99, abs=0.05)
    assert s["Time to 75 m"].value == pytest.approx(4.300, abs=0.01)


def test_the_app_runs_an_acceleration_test_without_a_target():
    """Data Checks ask for a Target Speed only when a case reads one: the
    target-less car runs through the app while its only case is an
    acceleration test, and still does once it has a cycle case too, whose
    runs alone the missing target stops."""
    proj = _fs_car()
    client = TestClient(app)
    ran = client.post("/api/simulate", json={"project": proj.model_dump(), "caseId": "case"}).json()
    assert ran["status"] == "success", ran["messages"]
    proj.cases.append(proj.cases[0].model_copy(update={"id": "cycle", "kind": "cycle",
                                                       "endDistance": None}))
    ran = client.post("/api/simulate", json={"project": proj.model_dump(), "caseId": "case"}).json()
    assert ran["status"] == "success", ran["messages"]
    refused = client.post("/api/simulate",
                          json={"project": proj.model_dump(), "caseId": "cycle"}).json()
    assert refused["status"] == "failed"
    assert any("has no Target Speed signal" in m["text"] for m in refused["messages"])


def test_missing_the_line_within_the_time_limit_is_a_warning():
    """The case duration is the time limit: a car that has not reached the
    line when it runs out gets a warning and no time. A run stopped by the
    user did not miss the line."""
    result = simulate(_bev(duration=3.0), "case-city")
    assert result.status == "warning"
    warnings = [m.text for m in result.messages if m.level == "warning"]
    assert len(warnings) == 1
    assert warnings[0].startswith("Acceleration test: the vehicle did not reach the 75 m line "
                                  "within the case's 3 s")
    assert "Time to 75 m" not in _rows(result)

    calls = {"n": 0}

    def control():
        calls["n"] += 1
        return [{"type": "cancel"}] if calls["n"] == 3 else []

    stopped = simulate(_bev(), "case-city", control=control)
    assert stopped.status == "cancelled"
    assert not any("did not reach" in m.text for m in stopped.messages)


def test_time_at_the_grip_limit():
    """The share of the run a driven wheel spent at the tyres' grip limit:
    the FS-sized car spins its rear wheels the whole run at μ 1.5 and never
    at μ 3; the BEV example never reaches it."""
    at = "Time at the tyres' grip limit"
    assert _rows(simulate(_fs_car(mu=1.5), "case"))[at].value >= 95.0
    assert _rows(simulate(_fs_car(mu=3.0), "case"))[at].value == 0.0
    assert _rows(simulate(_bev(), "case-city"))[at].value == 0.0


def test_the_estimate_note_follows_the_load_transfer():
    """The results are marked as estimates; the note says whether the wheel
    loads shift (MOD-40's Centre of Gravity Height)."""
    flat = simulate(_fs_car(), "case")
    assert any(m.level == "info" and "estimates" in m.text and "do not shift" in m.text
               for m in flat.messages)
    proj = _fs_car()
    veh = next(e for e in proj.systems[0].elements if e.id == "veh")
    veh.parameterOverrides.update(cg_height_m=0.3, wheelbase_m=1.55)
    shifted = simulate(proj, "case")
    assert any(m.level == "info" and "load transfer is included" in m.text
               for m in shifted.messages)


def test_terminal_power_rows():
    """Each battery's peak terminal power and its mean over the run. With
    the battery's 'Formula Student Electric' preset (80 kW, 0.5 s window,
    MOD-39) its power check gives the peak, and passes."""
    result = simulate(_bev(), "case-city")
    s = _rows(result)
    assert s[f"{BAT} — peak terminal power"].value == pytest.approx(
        max(_values(result, "el-battery", "sig_power")), abs=0.01)
    run_s = s["Simulated duration"].value
    net_kwh = s[f"{BAT} — energy delivered"].value - s[f"{BAT} — energy recuperated"].value
    assert s[f"{BAT} — mean terminal power"].value == pytest.approx(net_kwh * 3600 / run_s,
                                                                    rel=0.005)
    assert any("no battery has an Output Power Limit" in m.text for m in result.messages)

    battery = next(c for c in load_library() if c.id == "battery.generic")
    preset = next(p for p in battery.presets if p.name == "Formula Student Electric")
    capped = simulate(_bev(overrides={"el-battery": dict(preset.values)}), "case-city")
    c = _rows(capped)
    assert [r for r in c if r.endswith("— peak terminal power")] == [f"{BAT} — peak terminal power"]
    assert c[f"{BAT} — peak terminal power"].value <= 80.0 * 1.001
    assert c[f"{BAT} — peak terminal power, averaged"].passed is True
    assert f"{BAT} — mean terminal power" in c
    assert capped.status == "success", [m.text for m in capped.messages]
    assert not any("no battery has an Output Power Limit" in m.text for m in capped.messages)
