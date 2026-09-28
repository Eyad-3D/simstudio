"""Lap mode (MOD-42): SimCase.kind "lap" on the model's Race Track.

A quasi-steady-state lap solver finds the speed along the track from the
tyres' grip (with downforce, load transfer and load sensitivity) and the
powertrain's force, and the energy pass drives that trace through the
model's own motor, gear and battery code. These tests check it against
closed forms, the time-domain acceleration test, energy conservation and
FS Rules 2026 v1.1 (FSG) D 4.1, D 5.1.1, D 6.1 and D 7.1 for the layouts."""
import math
import time

import pytest
from fastapi.testclient import TestClient
from helpers import FS_FULL_LOAD, FS_LOSS, conn, dbc, el, fs_car, series

from app.main import app
from app.schemas import SimCase
from app.solver import lapsim, simulate
from app.solver.domains import RunContext
from app.solver.network import WheelRef, build_model
from app.solver.runtime import AIR_DENSITY, GRAVITY, RPM, Runtime, tyre_mu
from app.storage import load_example
from app.validation import validate_project

SKIDPAD_R = 9.125  # m: the lane centre between the 15.25 m and 21.25 m circles (D 4.1)
TRACK_CHANNELS = {"sig_lap_distance", "sig_lap", "sig_curvature", "sig_long_accel",
                  "sig_lat_accel", "sig_limit", "sig_x", "sig_y", "sig_elevation"}


def _rows(result):
    return {s.label: s.value for s in result.summary}


def _values(result, el_id, port):
    return [p["value"] for p in series(result, el_id, port)]


def _lap_run(proj, spacing=1.0):
    gear_of: dict = {}
    model = build_model(proj, gear_of, {})
    ctx = RunContext(proj, model, Runtime(model, None), gear_of, {})
    return lapsim.LapRun(ctx, spacing)


def _on_circles(result, lap=2):
    """Lap ``lap``'s speeds, m/s, at the points on the skidpad's circles (not
    where the line changes from one circle to the other)."""
    return [v / 3.6 for v, k, n in zip(_values(result, "veh", "sig_speed"),
                                       _values(result, "trk", "sig_curvature"),
                                       _values(result, "trk", "sig_lap"))
            if n == lap and abs(abs(k) - 1 / SKIDPAD_R) < 1e-4]


def _lap_ends(result):
    """Each lap's end time, from the Race Track's Lap channel (Store every 1)."""
    ts = [p["t"] for p in series(result, "trk", "sig_lap")]
    laps = _values(result, "trk", "sig_lap")
    return [ts[k - 1] for k in range(1, len(laps)) if laps[k] != laps[k - 1]] + [ts[-1]]


@pytest.mark.parametrize("cza", [0.0, 3.0])
def test_skidpad_lap_matches_closed_form_speed_with_downforce(cza):
    """The item's metric: on the skidpad (lane centre, R 9.125 m) with no
    drag or rolling resistance, the cornering speed is
    v = √(μ·m·g / (m/R − μ·½·ρ·CzA)); lap 2's Sector 1 (the right circle)
    takes 2πR/v within 0.5 % and every lap-2 speed on the circles is within
    0.5 % of v (measured −0.017 % and −0.029 %, the time rounded to 1 ms;
    the speeds equal v to 1e-6 %). Where
    the line straightens for a metre between the circles the speed rises
    by up to 3 %, which is not checked."""
    proj = fs_car("Skidpad", 2, vehicle={"cd": 0.0, "downforce_cza_m2": cza},
                  wheel={"rolling_resistance": 0.0})
    result = simulate(proj, "case")
    assert result.status == "success", [m.text for m in result.messages]
    mu, m = 1.5, 280.0
    v_cf = math.sqrt(mu * m * GRAVITY / (m / SKIDPAD_R - mu * 0.5 * AIR_DENSITY * cza))
    t_cf = 2 * math.pi * SKIDPAD_R / v_cf
    assert abs(_rows(result)["Sector 1 time"] - t_cf) / t_cf < 0.005
    lap2 = _on_circles(result)
    assert len(lap2) > 100
    assert max(abs(v / v_cf - 1) for v in lap2) < 0.005


def test_skidpad_with_load_sensitivity_and_lateral_transfer():
    """With a CG height, track widths and a load sensitivity of −0.2/kN,
    the lateral transfer moves m·a_y·h from the inner to the outer wheels
    (45 % on the front axle, its static share), where the grip grows less
    than the load: the speed is the root of Σ μ(Fz)·Fz = m·v²/R over the
    four wheels, within 0.5 % (measured 1e-8; 2.97 % below the speed
    without load sensitivity)."""
    h, t_f, t_r, dmu = 0.28, 1.2, 1.18, -0.2e-3
    proj = fs_car("Skidpad", 2, vehicle={"cd": 0.0, "downforce_cza_m2": 3.0, "cg_height_m": h,
                                         "track_front_m": t_f, "track_rear_m": t_r},
                  wheel={"rolling_resistance": 0.0, "mu_load_sensitivity_per_kN": dmu * 1000})
    result = simulate(proj, "case")
    assert result.status == "success"
    m, mu = 280.0, 1.5

    def grip_left(v):  # Σ μ(Fz)·Fz − m·a_y over the four wheels
        ay = v * v / SKIDPAD_R
        down = 0.5 * AIR_DENSITY * 3.0 * v * v
        total = 0.0
        for share, track, lltd in ((0.45, t_f, 0.45), (0.55, t_r, 0.55)):
            axle = share * m * GRAVITY + 0.5 * down  # aero balance 50 %
            moved = lltd * m * ay * h / track
            for fz, fz0 in ((axle / 2 + moved, share * m * GRAVITY / 2),
                            (axle / 2 - moved, share * m * GRAVITY / 2)):
                total += max(0.0, mu + dmu * (fz - fz0)) * fz
        return total - m * ay

    lo, hi = 1.0, 40.0
    for _ in range(100):
        mid = 0.5 * (lo + hi)
        lo, hi = (mid, hi) if grip_left(mid) > 0 else (lo, mid)
    lap2 = _on_circles(result)
    assert len(lap2) > 100
    assert max(abs(v / lo - 1) for v in lap2) < 0.005
    no_sensitivity = math.sqrt(mu * m * GRAVITY / (m / SKIDPAD_R - mu * 0.5 * AIR_DENSITY * 3.0))
    assert lo < no_sensitivity * 0.99


@pytest.mark.parametrize("mu,vehicle,wheel", [
    (1.2, {}, {}),
    (1.5, {}, {}),
    (2.5, {}, {}),  # the motor, not the tyres, limits
    (1.5, {"cg_height_m": 0.28}, {"mu_load_sensitivity_per_kN": -0.2}),
])
def test_75_m_straight_matches_the_time_domain_within_2_percent(mu, vehicle, wheel):
    """The item's metric: a lap case on the Acceleration 75 m layout gives
    the time the time-domain acceleration test (STU-37, 10 ms steps, slip
    tyres, the same wheel loads and tyre friction) takes over 75 m from
    rest, within 2 % (measured +0.33, +0.42, 0.00 and +0.85 %)."""
    proj = fs_car("Acceleration 75 m", mu=mu, vehicle=vehicle, wheel=wheel)
    proj.cases.append(SimCase(id="accel", name="Acceleration", kind="acceleration",
                              endDistance=75.0, duration=25.0, timeStep=0.01))
    lap, accel = simulate(proj, "case"), simulate(proj, "accel")
    assert lap.status == accel.status == "success"
    t_lap, t_td = _rows(lap)["Lap time"], _rows(accel)["Time to 75 m"]
    assert abs(t_lap - t_td) / t_td < 0.02
    assert "Speed at the finish" in _rows(lap)


def test_1_km_lap_solves_under_1_s():
    """The item's metric: the Autocross layout (979 m), one lap, every
    point stored, end to end in under 1 s (best of 3; 0.14 s measured)."""
    proj = fs_car("Autocross")
    best = math.inf
    for _ in range(3):
        t0 = time.perf_counter()
        result = simulate(proj, "case")
        best = min(best, time.perf_counter() - t0)
    assert result.status == "success"
    assert best < 1.0


def _closure(result, proj, aux_kw=0.0):
    """The battery's net energy against what the lap took, from the
    recorded channels and the model's values, % of the battery's: the
    kinetic energy ½·m_eff·v² (m_eff from the inertias), the road load
    (trapezoids of ½·ρ·CdA·v² + c_rr·m·g over the recorded distance), the
    friction brakes (their torque at the wheels' mean speed over each step),
    the gears (the motor's mechanical power × (1 − 97 % × 99 %), or its
    regenerated power × (1/(97 % × 99 %) − 1)), the motor's losses and the
    consumer."""
    ts = [p["t"] for p in series(result, "batt", "sig_power")]
    dts = [b - a for a, b in zip(ts, ts[1:])]

    def integral(el_id, port, fn=lambda x: x):  # step values, W·s: recorded at each step's end
        vals = _values(result, el_id, port)
        return sum(fn(x) * dt for x, dt in zip(vals[1:], dts))

    battery = integral("batt", "sig_power") * 1000.0
    r, i = 0.23, 4.0
    m_eff = 280 + (4 * 0.3 + 4 * 0.02 + 0.005 + 0.01) / r ** 2 + (0.02 + 0.002) * i * i / r ** 2
    v = [x / 3.6 for x in _values(result, "veh", "sig_speed")]
    s = _values(result, "veh", "sig_distance")
    kinetic = 0.5 * m_eff * (v[-1] ** 2 - v[0] ** 2)
    force = [0.5 * AIR_DENSITY * 1.2 * x * x + 0.015 * 280 * GRAVITY * min(1.0, x / 0.3) for x in v]
    road = sum(0.5 * (force[k] + force[k + 1]) * (s[k + 1] - s[k]) for k in range(len(s) - 1))
    friction = 0.0
    for w in ("fl", "fr", "rl", "rr"):
        torque = _values(result, f"b{w}", "sig_torque")
        omega = [x / RPM for x in _values(result, w, "sig_speed")]
        friction += sum(torque[k + 1] * 0.5 * (omega[k] + omega[k + 1]) * dts[k]
                        for k in range(len(dts)))
    eta = 0.97 * 0.99
    gears = integral("mot", "sig_mech_power",
                     lambda p: p * (1 - eta) if p > 0 else -p * (1 / eta - 1)) * 1000.0
    motor = integral("mot", "sig_losses") * 1000.0
    parts = kinetic + road + friction + gears + motor + aux_kw * 1000.0 * ts[-1]
    return 100.0 * (parts - battery) / battery


@pytest.mark.parametrize("battery,aux_kw,dcdc_pct", [
    ({}, 0.0, None),
    ({"internal_resistance_ohm": 1.2}, 0.0, None),  # its maximum-power point (61 kW) limits the motor
    ({"internal_resistance_ohm": 1.2}, 1.0, None),
    ({}, 1.0, 90),  # on a low-voltage bus behind a 90 % DC-DC
])
def test_lap_energy_closes_against_the_component_losses(battery, aux_kw, dcdc_pct):
    """The item's metric: over an Autocross lap the battery's net energy at
    its terminals matches the kinetic energy, road load, friction brakes,
    gear and motor losses (and a 1 kW consumer, with its DC-DC's losses)
    within 0.5 %, from the recorded channels (measured +0.002, +0.030,
    +0.033 and +0.002 %), and so does the summary's Lap energy balance
    error, also where the battery's maximum-power point holds the motor
    back."""
    proj = fs_car(battery=battery)
    if aux_kw:
        s = proj.systems[0]
        s.elements.append(el("aux", "electric.constant_drive", "Aux", power_kW=aux_kw))
        if dcdc_pct:
            s.elements += [el("dcdc", "controller.dcdc", "DC-DC", efficiency_pct=dcdc_pct,
                              output_voltage_V=24), el("lv", "electric.node", "LV Bus")]
            s.connections += [conn(97, "hvbus", "t2", "dcdc", "a_pos"),
                              conn(98, "dcdc", "b_pos", "lv", "t1"), conn(99, "lv", "t2", "aux", "pos")]
        else:
            s.connections.append(conn(99, "hvbus", "t2", "aux", "pos"))
    result = simulate(proj, "case")
    rows = _rows(result)
    assert abs(rows["Lap energy balance error"]) <= 0.5
    assert abs(_closure(result, proj, aux_kw / (dcdc_pct or 100) * 100)) <= 0.5
    if battery:
        assert rows["Time limited by battery"] > 0


def test_flying_laps_repeat():
    """A closed lap ends at a speed the next lap's first corners allow, so
    the flying laps after a standing first lap are the same (the backward
    pass brakes for the next lap's corners, not for its standing start)."""
    result = simulate(fs_car("Autocross", 3), "case")
    ends = _lap_ends(result)
    lap1, lap2, lap3 = ends[0], ends[1] - ends[0], ends[2] - ends[1]
    assert abs(lap3 - lap2) / lap2 < 1e-6
    assert lap1 > lap2
    rows = _rows(result)
    assert rows["Lap 1 time"] == pytest.approx(lap1, abs=1e-3)
    assert rows["Lap time"] == pytest.approx(lap2, abs=1e-3)
    assert rows["Total time"] == pytest.approx(ends[2], abs=1e-3)


def test_spacing_converged():
    """1 m between points is converged: the Autocross flying lap at 1 m and
    at 0.25 m spacing are within 0.1 % (measured 0.07 %)."""
    times = []
    for spacing in (1.0, 0.25):
        lap = _lap_run(fs_car("Autocross"), spacing)
        first = lap.solve(0.0)
        times.append(lap.solve(first.v[-1]).t[-1])
    assert abs(times[0] - times[1]) / times[1] < 0.001


def test_presets_close_and_follow_fs_guidance():
    """The layouts drawn for LightSim: the Skidpad is two circles of radius
    9.125 m, closed and back where it started; the Autocross is a closed lap
    of 0.9-1.5 km (D 6.1.2, about 1 km for D 7.1) with straights of 80 m or
    less (D 6.1.1), a turning diameter of at least 9 m and a slalom on
    cones 10 m apart (7.5-12 m in D 6.1.1, 9-15 m in D 7.1.1); Acceleration
    75 m is an open 75 m straight (D 5.1.1)."""
    skid = lapsim.load_track({"layout": "Skidpad"})
    assert skid.closed and skid.length == pytest.approx(4 * math.pi * SKIDPAD_R, abs=0.01)
    assert abs(skid.heading) < 1e-6
    assert math.hypot(skid.x[-1], skid.y[-1]) < 0.1
    assert skid.sector_ends[0] == pytest.approx(2 * math.pi * SKIDPAD_R, abs=0.01)

    auto = lapsim.load_track({"layout": "Autocross"})
    assert auto.closed and 900 <= auto.length <= 1500
    assert abs(abs(math.degrees(auto.heading)) - 360.0) < 0.5
    assert math.hypot(auto.x[-1], auto.y[-1]) < 0.5
    segs = lapsim.layouts()["Autocross"]["segments"]
    straights, run = [], 0.0
    for length, radius in segs + segs[:1]:  # the start straight runs over the finish line
        if radius:
            straights.append(run)
            run = 0.0
        else:
            run += length
    assert max(straights) <= 80.0
    assert min(abs(r) for _, r in segs if r) >= 4.5
    assert len(auto.sector_ends) == 3
    # the slalom: the line weaves 2 m either side of cones on x = 150 m
    slalom = [y for x, y, _ in lapsim.layouts()["Autocross"]["drawing"] if x in (148, 152)]
    assert len(slalom) == 5 and {b - a for a, b in zip(slalom, slalom[1:])} == {10}

    accel = lapsim.load_track({"layout": "Acceleration 75 m"})
    assert not accel.closed and accel.length == 75.0 and set(accel.kappa) == {0.0}


def test_limit_times_add_up():
    """The six 'Time limited by …' rows share the Total time out, and the
    Race Track's Limit channel holds only their codes 1-6."""
    result = simulate(fs_car("Autocross", 2, battery={"output_power_limit_kW": 60}, mu=2.2), "case")
    rows = _rows(result)
    limited = [rows[f"Time limited by {name}"] for name in lapsim.LIMITS]
    assert sum(limited) == pytest.approx(rows["Total time"], abs=6 * 0.0005)
    assert rows["Time limited by power cap"] > 0 and rows["Time limited by braking"] > 0
    # MOD-39's check counts the same time held at the 60 kW limit
    assert rows["Accumulator — time held at the output power limit"] == pytest.approx(
        rows["Time limited by power cap"], abs=0.02)
    assert set(_values(result, "trk", "sig_limit")) <= {1, 2, 3, 4, 5, 6}


def test_all_wheel_drive_passes_the_slower_motor_top_speed():
    """A second E-Motor on the front wheels through a 6.5 final drive stops
    driving at 86.7 km/h (its 6,500 1/min); on a 600 m straight the rear
    motor takes the car on towards its own top speed (measured 139.5 km/h,
    the time-domain acceleration test 134.9 km/h), and the energy pass,
    with the front motor's drag above its top speed, closes the balance
    (measured 0.002 %)."""
    proj = fs_car("Custom")
    s = proj.systems[0]
    next(e for e in s.elements if e.id == "trk").parameterOverrides.update(
        curvature_table={"0": 0, "600": 0}, closed=False)
    s.connections = [c for c in s.connections if c.sourceElementId not in ("bfl", "bfr")]
    s.elements += [
        el("mot2", "motor.emotor", "E-Motor F", full_load_torque=FS_FULL_LOAD, power_loss=FS_LOSS,
           drag_torque={"0": 0, "6500": 0.5}, max_speed_rpm=6500, inertia_kgm2=0.02),
        el("fd2", "mech.final_drive", "Final Drive F", ratio=6.5, efficiency_pct=97),
        el("diff2", "mech.differential", "Differential F", efficiency_pct=99),
        el("nfl", "mech.node", "Node FL"), el("nfr", "mech.node", "Node FR")]
    s.connections += [conn(200, "hvbus", "t4", "mot2", "pos"),
                      conn(201, "mot2", "shaft", "fd2", "flange_in"),
                      conn(202, "fd2", "flange_out", "diff2", "flange_in")]
    for i, (w, out) in enumerate((("fl", "flange_out_a"), ("fr", "flange_out_b"))):
        s.connections += [conn(203 + 3 * i, "diff2", out, f"n{w}", "f1"),
                          conn(204 + 3 * i, f"n{w}", "f2", f"b{w}", "flange"),
                          conn(205 + 3 * i, f"n{w}", "f3", w, "shaft")]
    proj.dataBusConnections.append(dbc(300, "drv", "sig_traction_cmd", "mot2", "sig_demand_in"))
    result = simulate(proj, "case")
    assert max(_values(result, "veh", "sig_speed")) > 130.0
    assert abs(_rows(result)["Lap energy balance error"]) <= 0.5


def test_live_edits_reach_the_next_lap():
    """A live edit of the tyres' μ (1.5 to 1.0) early in lap 1 of three:
    laps 2 and 3 corner at no more than 1.0 g, and lap 3 takes the time a
    run at μ 1.0 takes (the cornering speeds are solved again at each
    lap's start)."""
    calls = [0]

    def control():
        calls[0] += 1
        return ([{"type": "set_param", "elementId": w, "key": "mu", "value": 1.0}
                 for w in ("fl", "fr", "rl", "rr")] if calls[0] == 100 else [])

    result = simulate(fs_car("Autocross", 3), "case", control=control)
    lat = _values(result, "trk", "sig_lat_accel")
    lap = _values(result, "trk", "sig_lap")
    assert max(abs(a) for a, k in zip(lat, lap) if k > 1) <= 1.0 + 1e-3
    ends = _lap_ends(result)
    fresh = _rows(simulate(fs_car("Autocross", 3, mu=1.0), "case"))["Lap time"]
    assert ends[2] - ends[1] == pytest.approx(fresh, abs=2e-3)


def test_lap_times_not_valid_when_the_car_had_less_than_they_assume():
    """A battery that reaches its minimum SOC in lap 2, or a motor run past
    its Full-Load Torque table: the lap times, like the Energy per lap, are
    marked not valid (the laps were solved with power the car did not
    have, or from data it did not hold)."""
    result = simulate(fs_car("Autocross", 2, battery={"capacity_kWh": 0.5}), "case")
    marked = {s.label: s.notValid for s in result.summary}
    for label in ("Lap time", "Lap 1 time", "Total time", "Sector 1 time", "Average speed",
                  "Energy per lap"):
        assert marked[label] == "the battery reached its minimum SOC"
    assert marked["Lap energy balance error"] is None

    proj = fs_car()
    mot = next(e for e in proj.systems[0].elements if e.id == "mot")
    mot.parameterOverrides["full_load_torque"] = {"300": FS_FULL_LOAD["600"],
                                                  "400": FS_FULL_LOAD["600"]}
    marked = {s.label: s.notValid for s in simulate(proj, "case").summary}
    assert "past its 'Full-Load Torque' table" in marked["Lap time"]
    assert marked["Sector 2 time"] == marked["Energy per lap"] == marked["Lap time"]


def test_lap_case_refuses_unsupported_drivelines():
    """The P2 hybrid with a Race Track: a lap case is refused, naming its
    Combustion Engine and its Clutch, in the run and in Data Checks."""
    proj = load_example("hybrid-car")
    proj.systems[0].elements.append(el("trk", "track.lap", "Race Track"))
    case = proj.cases[0]
    case.kind = "lap"
    result = simulate(proj, case.id)
    assert result.status == "failed"
    texts = [m.text for m in result.messages]
    engine = next(t for t in texts if "Combustion Engine" in t)
    assert any("Clutch" in t for t in texts)
    checks = [c.text for c in validate_project(proj) if c.level == "error"]
    assert f"Case '{case.name}': {engine}" in checks


@pytest.mark.parametrize("change,expected", [
    (lambda p: p.systems[0].elements.pop(
        next(k for k, e in enumerate(p.systems[0].elements) if e.id == "trk")),
     "A lap case needs a Race Track"),
    (lambda p: [e.parameterOverrides.update(axle="Front") for e in p.systems[0].elements
                if e.componentDefId == "propulsion.wheel"],
     "A lap case needs wheels on both axles"),
    (lambda p: next(e for e in p.systems[0].elements if e.id == "trk").parameterOverrides.update(
        layout="Custom", curvature_table={"0": 0, "20": 0.6, "40": 0}),
     "its Curvature reaches 0.6 1/m"),
    (lambda p: next(e for e in p.systems[0].elements if e.id == "trk").parameterOverrides.update(
        layout="Custom", curvature_table={"5": 0, "100": 0}),
     "its Curvature table must start at 0 m"),
    (lambda p: next(e for e in p.systems[0].elements if e.id == "trk").parameterOverrides.update(
        laps=2.5),
     "Laps must be a whole number"),
    (lambda p: setattr(p.systems[0], "elements",
                       [e for e in p.systems[0].elements if e.id != "drv"]),
     "A lap case needs a Driver"),
])
def test_lap_problems_are_refused(change, expected):
    """A lap case without a Race Track, with every wheel on one axle, with
    a curvature tighter than 0.5 1/m (a 2 m radius), a Custom table that
    does not start at 0 m, a fraction of a lap or no Driver is refused in
    the run and in Data Checks."""
    proj = fs_car()
    change(proj)
    result = simulate(proj, "case")
    assert result.status == "failed"
    assert any(expected in m.text for m in result.messages), [m.text for m in result.messages]
    assert any(expected in c.text for c in validate_project(proj) if c.level == "error")


def test_a_custom_track_runs_and_its_closure_is_checked():
    """A Custom curvature table: a 40 m radius circle (closed, sectors at a
    quarter and a half) runs; the same table as a half circle marked closed
    is warned about in Data Checks."""
    r = 40.0
    circle = 2 * math.pi * r
    proj = fs_car()
    trk = next(e for e in proj.systems[0].elements if e.id == "trk")
    trk.parameterOverrides.update(layout="Custom", sector_ends=f"{circle / 4}; {circle / 2}",
                                  curvature_table={"0": 1 / r, f"{circle}": 1 / r})
    result = simulate(proj, "case")
    assert result.status == "success", [m.text for m in result.messages]
    assert {"Sector 1 time", "Sector 2 time", "Sector 3 time"} <= _rows(result).keys()
    assert not [c for c in validate_project(proj) if "Closed Circuit" in c.text]
    trk.parameterOverrides["curvature_table"] = {"0": 1 / r, f"{circle / 2}": 1 / r}
    trk.parameterOverrides["sector_ends"] = ""
    assert [c for c in validate_project(proj)
            if c.level == "warning" and "Closed Circuit" in c.text and "turns the car 180°" in c.text]


def test_old_projects_unchanged():
    """The refactors leave drive cycles exactly as they were: tyre_mu gives
    a wheel's own μ bit for bit without load sensitivity (the golden tests,
    run with LIGHTSIM_GOLDEN_EXACT=1, check the examples' results), and a
    load sensitivity changes the time-domain tyre's grip."""
    w = WheelRef(el_id="w", m=1.0, radius=0.3, load_share=0.25, mu=0.9173, c_slip=10, c_rr=0.01,
                 fz_static=3000.0)
    for fz in (0.0, 1234.5678, 3000.0, 1e5):
        assert tyre_mu(w, fz) == w.mu
        assert tyre_mu(w, fz, lateral=True) == w.mu
    w.dmu_per_n, w.mu_y = -0.2e-3, 1.1
    assert tyre_mu(w, 4000.0) == pytest.approx(0.9173 - 0.2)
    assert tyre_mu(w, 4000.0, lateral=True) == pytest.approx(1.1 - 0.2)
    assert tyre_mu(w, 1e6) == 0.0  # never below 0

    proj = fs_car("Acceleration 75 m", wheel={"mu_load_sensitivity_per_kN": -0.2,
                                              "mu_nominal_load_N": 500})
    proj.cases[0].kind, proj.cases[0].endDistance = "acceleration", 75.0
    proj.cases[0].duration, proj.cases[0].timeStep = 25.0, 0.01
    sensitive = _rows(simulate(proj, "case"))["Time to 75 m"]
    for e in proj.systems[0].elements:
        e.parameterOverrides.pop("mu_load_sensitivity_per_kN", None)
    assert _rows(simulate(proj, "case"))["Time to 75 m"] < sensitive


def test_track_channels_only_in_lap_runs():
    """A cycle case on a model with a Race Track records no Race Track
    channels; a lap case records all nine and the Driver's, the E-Motor's
    and the battery's, and the Driver's, wheels' and brakes' too."""
    proj = fs_car()
    proj.cases.append(SimCase(id="cycle", name="Cycle", duration=20, timeStep=0.1))
    cycle = simulate(proj, "cycle")
    assert cycle.status == "success"
    assert not [c for c in cycle.channels if c.elementId == "trk"]
    lap = simulate(proj, "case")
    got = {(c.elementId, c.portId) for c in lap.channels}
    assert {p for e, p in got if e == "trk"} == TRACK_CHANNELS
    assert {("drv", "sig_traction_cmd"), ("drv", "sig_brake_cmd"), ("mot", "sig_torque"),
            ("mot", "sig_speed"), ("batt", "sig_power"), ("batt", "sig_soc"),
            ("bfl", "sig_torque"), ("rl", "sig_force"), ("rl", "sig_normal_load"),
            ("diff", "sig_torque_a")} <= got
    g = {c.portId: c.unit for c in lap.channels if c.elementId == "trk"}
    assert g["sig_long_accel"] == g["sig_lat_accel"] == "g" and g["sig_curvature"] == "1/m"
    # the lateral acceleration is v²·κ, in g
    v = [x / 3.6 for x in _values(lap, "veh", "sig_speed")]
    kappa = _values(lap, "trk", "sig_curvature")
    lat = _values(lap, "trk", "sig_lat_accel")
    assert max(abs(a - vv * vv * k / GRAVITY) for a, vv, k in zip(lat, v, kappa)) < 2e-3


def test_the_app_runs_a_lap_case():
    """Through the app: Data Checks pass a lap-only model with no Target
    Speed wired, and the run gives the lap rows first."""
    proj = fs_car("Skidpad", 2)
    proj.dataBusConnections = [d for d in proj.dataBusConnections if d.port2Id != "sig_target_in"]
    client = TestClient(app)
    checks = client.post("/api/validate", json={"project": proj.model_dump()}).json()
    assert not [c for c in checks if c["level"] == "error"], checks
    result = client.post("/api/simulate", json={"project": proj.model_dump(),
                                                "caseId": "case"}).json()
    assert result["status"] == "success"
    assert result["summary"][0]["label"] == "Lap time"
    assert result["messages"][0]["text"].startswith("Case 'Case' solved in lap mode: 2 laps")
