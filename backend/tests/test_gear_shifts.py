"""A gear shift is a rigid, instantaneous engagement: the speeds jump to the
new ratio keeping the angular (and, through tyres that grip, linear)
momentum, so kinetic energy can only be lost, and the energy books hold
what the speeds hold, the loss being the gearbox's (its 'gear shifts'
term). Before, the shift kept one part's speed and set the others from the
new ratio: with the load on a shaft an upshift made the load jump from
67.5 to 115.8 rad/s and the run ended with 1683 kJ of kinetic energy for
the 1085 kJ the motor gave, while the books said 1086 kJ were stored."""
from __future__ import annotations

import math

import pytest
from helpers import conn, dbc, el, project, series

from app.solver import simulate

RPM = 60.0 / (2.0 * math.pi)
KWH = 3.6e6
J1, J2, T = 0.05, 135.0, 200.0  # motor rotor, load, motor torque (benchmarks' mech_gear_change)


def _motor(n_max: float = 20000.0) -> list:
    flat = {"0": T, f"{n_max:g}": T}
    zero = {"0": 0.0, f"{T:g}": 0.0}
    return [el("src", "electric.voltage_source", "Supply", voltage_V=400.0),
            el("bus", "electric.node", "Bus"),
            el("mot", "motor.emotor", "E-Motor", inertia_kgm2=J1,
               full_load_torque={"100": flat, "1000": flat},
               power_loss={"0": zero, f"{n_max:g}": zero},
               drag_torque={"0": 0.0, f"{n_max:g}": 0.0}, max_speed_rpm=n_max),
            el("dem", "signal.constant", "Demand", value=1.0)]


def _gear_profile(first: float, second: float, at: float) -> str:
    return f"0:{first}; {at - 1e-7!r}:{first}; {at - 1e-7!r}:{second}"


def _rotational(i1: float, i2: float, t_shift: float = 4.0, duration: float = 8.0):
    """E-Motor → lossless two-speed Gearbox → Propeller of zero torque (J2)."""
    elements = _motor() + [
        el("gb", "mech.gearbox", "Gearbox", ratios={"1": i1, "2": i2}, efficiency_pct=100,
           inertia_in_kgm2=0, inertia_out_kgm2=0),
        el("load", "propulsion.propeller", "Load", torque_ref_Nm=0, inertia_kgm2=J2),
        el("gear", "signal.driving_task", "Gear", profile=_gear_profile(1, 2, t_shift)),
    ]
    connections = [conn(1, "src", "pos", "bus", "t1"), conn(2, "bus", "t2", "mot", "pos"),
                   conn(3, "mot", "shaft", "gb", "flange_in"),
                   conn(4, "gb", "flange_out", "load", "shaft")]
    databus = [dbc(1, "dem", "sig_out", "mot", "sig_demand_in"),
               dbc(2, "gear", "sig_demand", "gb", "sig_gear_in")]
    return project(elements, connections, databus, duration=duration, time_step=0.01)


def _speeds(result, el_id: str) -> dict[float, float]:
    return {round(p["t"], 6): p["value"] / RPM for p in series(result, el_id, "sig_speed")}


def _books(result) -> dict:
    return {f.label: f for f in result.partEnergy}


@pytest.mark.parametrize("i1, i2", [(12.0, 7.0), (7.0, 12.0)])  # up, then down
def test_a_shift_keeps_the_angular_momentum_and_books_the_energy_it_loses(i1, i2):
    r = simulate(_rotational(i1, i2), "case")
    w1, w2 = _speeds(r, "mot"), _speeds(r, "load")
    # before the shift the motor's constant torque spins both up (explicit
    # Euler is exact for it); at the shift the load's J2 ω2 + i2 J1 ω1 is kept
    before = T * i1 * 4.0 / (J2 + i1 * i1 * J1)
    assert w2[4.0] == pytest.approx(before, rel=1e-12)
    after = (J2 + i1 * i2 * J1) * before / (J2 + i2 * i2 * J1)
    assert w2[8.0] == pytest.approx(after + T * i2 * 4.0 / (J2 + i2 * i2 * J1), rel=1e-12)
    assert w1[8.0] == pytest.approx(i2 * w2[8.0], rel=1e-12)
    # kinetic energy is lost, never made, up- and downshifting, and the
    # gearbox books it as its loss
    lost = 0.5 * (J2 + i1 * i1 * J1) * before ** 2 - 0.5 * (J2 + i2 * i2 * J1) * after ** 2
    assert lost > 0
    books = _books(r)
    gb = books["Gearbox"]
    assert gb.terms["gear shifts"] * KWH == pytest.approx(lost, rel=1e-9)
    assert gb.losses * KWH == pytest.approx(lost, rel=1e-9)
    # the rotating parts' books hold what the speeds hold
    held = 0.5 * J1 * w1[8.0] ** 2 + 0.5 * J2 * w2[8.0] ** 2
    assert books["Rotating parts"].stored * KWH == pytest.approx(held, rel=1e-12)


def _geared_vehicle(locked_clutch_engine: bool = False):
    """E-Motor → Gearbox (3.9 → 2.4) → Final Drive → Differential → two
    Wheels under a Vehicle at 40 km/h, the gear stepping up at 1 s; with
    ``locked_clutch_engine`` an idling Engine behind a closed Clutch on the
    gearbox's input, as in the P2 Hybrid Car."""
    elements = _motor(12000.0) + [
        el("veh", "vehicle.body", "Vehicle", initial_speed_kmh=40.0),
        el("gb", "mech.gearbox", "Gearbox", ratios={"1": 3.9, "2": 2.4}),
        el("fd", "mech.final_drive", "Final Drive", ratio=3.4),
        el("diff", "mech.differential", "Differential"),
        el("wl", "propulsion.wheel", "Wheel L", vehicle_load_share_pct=50),
        el("wr", "propulsion.wheel", "Wheel R", vehicle_load_share_pct=50),
        el("gear", "signal.driving_task", "Gear", profile=_gear_profile(1, 2, 1.0)),
    ]
    connections = [conn(1, "src", "pos", "bus", "t1"), conn(2, "bus", "t2", "mot", "pos"),
                   conn(4, "gb", "flange_out", "fd", "flange_in"),
                   conn(5, "fd", "flange_out", "diff", "flange_in"),
                   conn(6, "diff", "flange_out_a", "wl", "shaft"),
                   conn(7, "diff", "flange_out_b", "wr", "shaft")]
    if locked_clutch_engine:
        elements += [el("eng", "engine.combustion", "Engine"), el("cl", "mech.clutch", "Clutch"),
                     el("node", "mech.node", "Node")]
        connections += [conn(8, "eng", "shaft", "cl", "flange_a"),
                        conn(9, "cl", "flange_b", "node", "f1"),
                        conn(10, "mot", "shaft", "node", "f2"),
                        conn(11, "node", "f3", "gb", "flange_in")]
    else:
        connections.append(conn(3, "mot", "shaft", "gb", "flange_in"))
    databus = [dbc(1, "dem", "sig_out", "mot", "sig_demand_in"),
               dbc(2, "gear", "sig_demand", "gb", "sig_gear_in")]
    return project(elements, connections, databus, duration=2.0, time_step=0.01)


def test_through_gripping_tyres_the_vehicle_takes_part_in_the_shift():
    """The tyres grip, so the impulse reaches the Vehicle: its speed steps
    up a little at the upshift (the motor's angular momentum goes into the
    car), the wheels keep rolling with it instead of spinning up, and the
    books close on the kinetic energy the speeds hold."""
    r = simulate(_geared_vehicle(), "case")
    v = {round(p["t"], 6): p["value"] / 3.6 for p in series(r, "veh", "sig_speed")}
    w = {round(p["t"], 6): p["value"] / RPM for p in series(r, "wl", "sig_speed")}
    mot = _speeds(r, "mot")
    radius = 0.33  # the library's wheel
    # no wheel spin: the tyres' slip (about 3.5 % under the full first-gear
    # torque) carries on through the shift; before, the wheels kept their
    # speed and the motor's momentum was lost
    slip = {t: w[t] * radius / v[t] - 1.0 for t in (0.99, 1.0, 1.01, 1.02)}
    assert all(0.0 < s < 0.05 for s in slip.values()), slip
    assert slip[1.01] == pytest.approx(slip[1.0], abs=0.01), slip
    assert v[1.01] - v[1.0] > 2.0 * (v[1.0] - v[0.99])  # the motor's momentum went into the car
    # the motor's speed meets the new ratio at once
    assert mot[1.01] / w[1.01] == pytest.approx(2.4 * 3.4, rel=1e-9)
    books = _books(r)
    assert books["Gearbox"].terms["gear shifts"] > 0
    residual = next(s.value for s in r.summary if s.label == "Energy balance residual")
    assert abs(residual) < 0.5  # % (the solver step's own interface error)


def test_an_engine_behind_a_closed_clutch_keeps_its_speed_and_the_clutch_slips():
    """A clutch's torque is limited, so it passes no impulse: at the shift
    the engine keeps its speed and the clutch slips at its torque until the
    engine meets the new input speed, then sticks again (its heat is the
    clutch's loss). Before the clutch stuck in the step its slip passed
    through zero, it overshot at the 10 ms step and rang from one side to
    the other for the rest of this run."""
    r = simulate(_geared_vehicle(locked_clutch_engine=True), "case")
    eng, mot = _speeds(r, "eng"), _speeds(r, "mot")
    slip = {round(p["t"], 6): p["value"] / RPM for p in series(r, "cl", "sig_slip_speed")}
    torque = {round(p["t"], 6): p["value"] for p in series(r, "cl", "sig_torque")}
    assert slip[0.99] == 0.0  # stuck before the shift
    assert eng[1.01] > mot[1.01] + 30.0  # the engine kept its speed, the input dropped
    assert torque[1.01] == pytest.approx(300.0)  # slipping at its whole torque
    stuck = next(t for t in sorted(slip) if t > 1.0 and slip[t] == 0.0)
    assert stuck < 1.2 and all(slip[t] == 0.0 for t in slip if t >= stuck)  # stuck, for good
    books = _books(r)
    assert books["Clutch"].losses > 0 and books["Gearbox"].terms["gear shifts"] > 0
