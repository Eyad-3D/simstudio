"""Source-limit handshake: before any motor torque is applied, every bus
states what its source can deliver and absorb over the solver step, and the
motors are held to it — so no motor draws energy a battery or fuel cell
does not have, and no recuperated energy vanishes into a full battery, a
charge limit or a one-way DC-DC.

Every case records at the solver step (0.01 s), so the recorded powers are
exactly the ones each step used."""
import pytest
from helpers import bev_axle, conn, dbc, el, project, series

from app.solver import simulate

DT = 0.01


def _set(proj, **overrides):
    for e in proj.systems[0].elements:
        if e.id in overrides:
            e.parameterOverrides.update(overrides[e.id])
    return proj


def _run(proj, duration):
    proj.cases[0].duration = duration
    proj.cases[0].timeStep = DT
    result = simulate(proj, "case")
    assert result.status in ("success", "warning"), [m.text for m in result.messages]
    return result


def _values(result, el_id, port):
    return [p["value"] for p in series(result, el_id, port)]


def _summary(result):
    return {s.label: s.value for s in result.summary}


def _with_brakes(proj):
    proj.systems[0].elements += [el("brl", "mech.brake", "Brake L"), el("brr", "mech.brake", "Brake R")]
    proj.systems[0].connections += [conn(20, "whl", "shaft", "brl", "flange"),
                                    conn(21, "whr", "shaft", "brr", "flange")]
    proj.dataBusConnections += [dbc(20, "drv", "sig_brake_cmd", "brl", "sig_demand_in"),
                                dbc(21, "drv", "sig_brake_cmd", "brr", "sig_demand_in")]
    return proj


def _constant_demand_axle(source_els, source_conns, demand, **vehicle):
    """Motor on a bus with the given source, commanded by a constant (a
    stand-in for any script or hybrid controller), no driver."""
    elements = [
        el("veh", "vehicle.body", "Vehicle", **vehicle),
        el("bus", "electric.node", "Bus"),
        el("mot", "motor.emotor", "E-Motor"),
        el("fd", "mech.final_drive", "Final Drive"),
        el("diff", "mech.differential", "Differential", locked=True),
        el("whl", "propulsion.wheel", "Wheel L", vehicle_load_share_pct=50),
        el("whr", "propulsion.wheel", "Wheel R", vehicle_load_share_pct=50),
        el("cmd", "signal.constant", "Command", value=demand),
        *source_els,
    ]
    connections = [
        *source_conns,
        conn(2, "bus", "t2", "mot", "pos"),
        conn(3, "mot", "shaft", "fd", "flange_in"),
        conn(4, "fd", "flange_out", "diff", "flange_in"),
        conn(5, "diff", "flange_out_a", "whl", "shaft"),
        conn(6, "diff", "flange_out_b", "whr", "shaft"),
    ]
    return project(elements, connections, [dbc(1, "cmd", "sig_out", "mot", "sig_demand_in")])


def _battery_net_in_kwh(result, r0):
    """Energy that went into storage: terminal energy in minus I²R losses."""
    power = _values(result, "batt", "sig_power")[1:]
    current = _values(result, "batt", "sig_current")[1:]
    return -sum(p * 1000.0 + i * i * r0 for p, i in zip(power, current)) * DT / 3.6e6


def _assert_source_covers_motor(result, source, port="sig_power"):
    """Step by step, the source's power is the motor's electrical power."""
    p_src = _values(result, source, port)
    p_mot = _values(result, "mot", "sig_elec_power")
    worst = max(abs(a - b) for a, b in zip(p_src[1:], p_mot[1:]))
    assert worst < 1e-3, f"source and motor power differ by up to {worst:.4f} kW"
    e_src = sum(p_src[1:]) * DT / 3600.0
    e_mot = sum(p_mot[1:]) * DT / 3600.0
    assert e_src == pytest.approx(e_mot, rel=1e-3, abs=1e-6), (e_src, e_mot)
    assert _summary(result)["Electrical energy balance error"] < 0.1


def test_battery_driven_to_its_limits_closes_the_energy_balance():
    """A small, weak battery: the launch hits its maximum-power point, the
    cruise empties it to its minimum SOC, and the braking at the end runs
    into its charge limit. The battery must supply exactly what the motor
    draws throughout, the charge it counts must match the current that
    flowed, and the car must slow down once it is empty."""
    proj = _with_brakes(bev_axle(profile="0:0; 5:100; 60:100; 70:0; 80:0"))
    _set(proj, batt={"capacity_kWh": 0.15, "initial_soc_pct": 30, "internal_resistance_ohm": 1.0,
                     "max_charge_power_kW": 5})
    result = _run(proj, 80)
    texts = " ".join(m.text for m in result.messages)
    assert "maximum power point" in texts
    assert "reached minimum SOC" in texts

    _assert_source_covers_motor(result, "batt")
    soc = _values(result, "batt", "sig_soc")
    assert min(soc) >= 10.0 - 1e-6, "never below the minimum SOC"
    assert min(_values(result, "batt", "sig_power")) >= -5.0 - 1e-6, "never above the charge limit"

    q_ah = 0.15e3 / 345.0  # Usable Capacity at the library OCV table's SOC-weighted mean
    current = _values(result, "batt", "sig_current")
    assert (soc[-1] - soc[0]) / 100.0 * q_ah == pytest.approx(-sum(current[1:]) * DT / 3600.0, rel=1e-6)
    assert _summary(result)["E-Motor — time limited by supply"] > 10.0

    speed = {p["t"]: p["value"] for p in series(result, "veh", "sig_speed")}
    assert speed[60.0] < 0.8 * max(speed.values()), "the car slows once the battery is empty"


def test_min_soc_leaves_the_motor_nothing():
    """bench/exp_energy.py: before, the motor drew 1.64 kWh while the empty
    battery supplied 0.075 kWh and the car cruised on at 100 km/h."""
    proj = _set(bev_axle(profile="0:0; 5:100; 120:100"),
                batt={"capacity_kWh": 0.4, "initial_soc_pct": 30})
    result = _run(proj, 60)
    _assert_source_covers_motor(result, "batt")
    speed = _values(result, "veh", "sig_speed")
    after = speed[500:]  # empty from t = 3 s: it coasts down, the target is 100 km/h
    assert all(b <= a + 1e-9 for a, b in zip(after, after[1:]))
    assert after[-1] < 35.0


def test_max_power_point_limits_the_motor():
    """Before: 91 kW motor draw against 11.3 kW from a 3-ohm battery."""
    proj = _set(bev_axle(profile="0:0; 5:150; 60:150"), batt={"internal_resistance_ohm": 3.0})
    result = _run(proj, 15)
    _assert_source_covers_motor(result, "batt")
    batt = _values(result, "batt", "sig_power")
    ocv, r0 = 368.0, 3.0  # 90 % SOC on the default OCV table
    assert max(batt) <= ocv * ocv / (4 * r0) / 1000.0 + 1e-6


def test_fuel_cell_overload_limits_the_motor():
    """ml/phys_tests5.py T6b: before, the motor drew 116 kW from a fuel cell
    that supplied 67 kW."""
    proj = _constant_demand_axle([el("fc", "fuelcell.stack", "Fuel Cell", max_current_A=200)],
                                 [conn(1, "fc", "pos", "bus", "t1")], 1.0, initial_speed_kmh=60)
    result = _run(proj, 5)
    _assert_source_covers_motor(result, "fc")
    assert max(_values(result, "fc", "sig_power")) <= 200 * 336 / 1000.0 + 1e-6
    assert any("maximum power — motor torque limited" in m.text for m in result.messages)


def test_regen_command_above_the_charge_limit_is_cut():
    """A controller asks for full regeneration into a 10 kW charge limit:
    before, 111 kW left the motor and 10 kW reached the battery."""
    proj = _constant_demand_axle(
        [el("batt", "battery.generic", "Battery", max_charge_power_kW=10, initial_soc_pct=50)],
        [conn(1, "batt", "pos", "bus", "t1")], -1.0, initial_speed_kmh=100, cd=0.0)
    result = _run(proj, 5)
    _assert_source_covers_motor(result, "batt")
    assert min(_values(result, "mot", "sig_elec_power")) >= -10.0 - 1e-6


def test_full_battery_regen_goes_to_the_friction_brakes():
    """Braking with a full battery: before, 0.11 kWh of recuperation
    vanished into it (its SOC was clamped at 100 %). Now the driver brakes
    with the friction brakes and the car still stops on time."""
    proj = _with_brakes(bev_axle(profile="0:100; 5:100; 25:0; 40:0"))
    _set(proj, batt={"initial_soc_pct": 100}, veh={"initial_speed_kmh": 100})
    result = _run(proj, 30)
    soc = _values(result, "batt", "sig_soc")
    stored_kwh = (soc[-1] - soc[0]) / 100.0 * 60.0
    assert stored_kwh == pytest.approx(_battery_net_in_kwh(result, 0.08), abs=1e-5)
    assert max(soc) <= 100.0
    _assert_source_covers_motor(result, "batt")
    speed = {p["t"]: p["value"] for p in series(result, "veh", "sig_speed")}
    assert speed[25.0] < 1.0
    assert max(_values(result, "brl", "sig_torque")) > 100.0
    assert any("is full" in m.text for m in result.messages)


def test_one_way_dcdc_passes_no_regen_upstream():
    """A motor behind a one-way DC-DC (fuel cell → DC-DC → motor bus) can
    feed back only what its own bus uses; before, its regeneration
    vanished at the converter."""
    proj = _constant_demand_axle(
        [el("fc", "fuelcell.stack", "Fuel Cell"), el("fcbus", "electric.node", "FC Bus"),
         el("dc", "controller.dcdc", "DC-DC", output_voltage_V=350),
         el("aux", "electric.constant_drive", "Aux", power_kW=2.0)],
        [conn(1, "fc", "pos", "fcbus", "t1"), conn(7, "fcbus", "t2", "dc", "a_pos"),
         conn(8, "dc", "b_pos", "bus", "t1"), conn(9, "bus", "t3", "aux", "pos")],
        -0.5, initial_speed_kmh=80, cd=0.0)
    result = _run(proj, 5)
    mot = _values(result, "mot", "sig_elec_power")
    aux = _values(result, "aux", "sig_power")
    out = _values(result, "dc", "sig_power_out")
    assert min(out[1:]) >= 0.0
    for m, a, o in zip(mot[1:], aux[1:], out[1:]):
        assert o == pytest.approx(m + a, abs=1e-6)  # the converter covers the bus, no more
    assert min(mot) >= -2.0 - 1e-6  # regeneration only feeds the 2 kW consumer
    assert _summary(result)["Electrical energy balance error"] < 0.1


def test_empty_battery_sheds_its_consumers():
    """An empty battery cannot carry an auxiliary load either."""
    proj = bev_axle(profile="0:0; 60:0")
    proj.systems[0].elements.append(el("aux", "electric.constant_drive", "Aux", power_kW=3.0))
    proj.systems[0].connections.append(conn(30, "hvbus", "t2", "aux", "pos"))
    _set(proj, batt={"capacity_kWh": 0.02, "initial_soc_pct": 20})
    result = _run(proj, 20)
    aux = _values(result, "aux", "sig_power")
    assert aux[1] == pytest.approx(3.0)
    assert aux[-1] == pytest.approx(0.0, abs=1e-9)
    assert min(_values(result, "batt", "sig_soc")) >= 10.0 - 1e-6
    assert any("cut back" in m.text for m in result.messages)


# ---- regeneration the supply cannot take (MOD-02) ---------------------------

def _asked_generator_kwh(result, demand: float, volts: list[float]) -> float:
    """Generator energy a constant regen command asked for, worked out here
    from the E-Motor's catalog maps: each step, torque = command × full-load
    torque (at the step's speed and the bus voltage it started with) ×
    generator scale, electrical power = torque × speed + map loss."""
    import math

    from app.library import library_by_id
    from app.solver.maps import interp2, parse_table2d

    defaults = {p.key: p.default for p in library_by_id()["motor.emotor"].parameters}
    full = parse_table2d(defaults["full_load_torque"])
    loss = parse_table2d(defaults["power_loss"])
    q4 = defaults["q4_torque_scale_pct"] / 100.0
    rpm = _values(result, "mot", "sig_speed")
    total = 0.0
    for i in range(1, len(rpm)):
        torque = demand * interp2(full, volts[i - 1], rpm[i]) * q4
        total += min(0.0, torque * rpm[i] * math.pi / 30.0 + interp2(loss, rpm[i], abs(torque)) * 1000.0)
    return -total * DT / 3.6e6


def _kwh(result, el_id, port):
    return sum(_values(result, el_id, port)[1:]) * DT / 3600.0


REGEN_LIMITS = {
    # name: (sources, their connections, what the motor's bus took, bus voltage)
    "full battery": (
        [el("batt", "battery.generic", "Battery", initial_soc_pct=100)],
        [conn(1, "batt", "pos", "bus", "t1")],
        lambda r: -_kwh(r, "batt", "sig_power"), lambda r: _values(r, "batt", "sig_voltage")),
    "charge-power limit": (
        [el("batt", "battery.generic", "Battery", max_charge_power_kW=10, initial_soc_pct=50)],
        [conn(1, "batt", "pos", "bus", "t1")],
        lambda r: -_kwh(r, "batt", "sig_power"), lambda r: _values(r, "batt", "sig_voltage")),
    "one-way DC-DC": (
        [el("batt", "battery.generic", "Battery", initial_soc_pct=50), el("hv", "electric.node", "HV"),
         el("dc", "controller.dcdc", "DC-DC", output_voltage_V=350),
         el("aux", "electric.constant_drive", "Aux", power_kW=2.0)],
        [conn(1, "batt", "pos", "hv", "t1"), conn(10, "hv", "t2", "dc", "a_pos"),
         conn(11, "dc", "b_pos", "bus", "t1"), conn(12, "bus", "t3", "aux", "pos")],
        lambda r: _kwh(r, "aux", "sig_power") - _kwh(r, "dc", "sig_power_out"),
        lambda r: [350.0] * len(_values(r, "mot", "sig_speed"))),
    "fuel-cell-only bus": (
        [el("fc", "fuelcell.stack", "Fuel Cell"), el("h2", "fuel.h2_tank", "H2 Tank")],
        [conn(1, "fc", "pos", "bus", "t1")],
        lambda r: -_kwh(r, "fc", "sig_power"),
        lambda r: [400.0] + _values(r, "fc", "sig_voltage")[1:]),  # open-circuit voltage at t = 0
}


@pytest.mark.parametrize("case", list(REGEN_LIMITS))
def test_regeneration_the_supply_cannot_take_is_reported(case):
    """ml/phys_tests5.py T5, phys_tests6.py, mod/regen_sinks.py: a controller
    asks for regeneration its bus cannot take. The motor is held to what the
    bus takes; the rest of what the command asked for is reported as "not
    recovered" (before: only a warning, 91-100 % of it unaccounted). The
    generator energy asked for = what the bus took + not recovered, within
    0.1 %, and a full battery recuperates nothing."""
    sources, wires, taken, volts = REGEN_LIMITS[case]
    demand = -1.0 if case == "charge-power limit" else -0.5
    proj = _constant_demand_axle(sources, wires, demand, initial_speed_kmh=100, cd=0.0)
    result = _run(proj, 10)
    summary = _summary(result)
    asked = _asked_generator_kwh(result, demand, volts(result))
    lost = summary["E-Motor — regeneration not recovered"]
    assert asked > 0.1
    assert taken(result) + lost == pytest.approx(asked, rel=1e-3), (taken(result), lost, asked)
    if case == "full battery":
        assert summary["Battery — energy recuperated"] == 0.0
        assert max(_values(result, "batt", "sig_soc")) <= 100.0


def test_driver_recuperates_up_to_the_charge_limit_and_brakes_the_rest():
    """The Driver's blending checks the recuperation it asks for against what
    the battery can take this step, on the motor's own maps and through the
    drivetrain's efficiency: braking from 100 km/h into a 20 kW charge limit
    now charges at the limit (before: 15.4 kW at most, the recuperation
    weight was applied to the limit too) and never needs the handshake to
    cut it; the friction brakes take the rest and the car stops on time.
    (A locked differential: the driveline starts at the vehicle speed.)"""
    proj = _with_brakes(bev_axle(locked=True, profile="0:100; 5:100; 25:0; 40:0"))
    _set(proj, batt={"max_charge_power_kW": 20}, veh={"initial_speed_kmh": 100})
    result = _run(proj, 30)
    # while braking needs more than 20 kW (below about 35 km/h it needs less)
    charge = [-p["value"] for p in series(result, "batt", "sig_power") if 6.0 <= p["t"] <= 15.0]
    assert max(charge) <= 20.0 + 1e-6
    assert min(charge) > 0.98 * 20.0
    assert "E-Motor — regeneration not recovered" not in _summary(result)
    assert "E-Motor — time limited by supply" not in _summary(result)
    assert max(_values(result, "brl", "sig_torque")) > 50.0
    speed = {p["t"]: p["value"] for p in series(result, "veh", "sig_speed")}
    assert speed[25.0] < 1.0


def test_a_full_battery_recuperates_nothing_under_the_driver():
    """Braking from the first instant with the battery at 100 %: nothing is
    recuperated, nothing is cut and the friction brakes stop the car."""
    proj = _with_brakes(bev_axle(locked=True, profile="0:90; 18:0; 30:0"))
    _set(proj, batt={"initial_soc_pct": 100}, veh={"initial_speed_kmh": 100})
    result = _run(proj, 25)
    summary = _summary(result)
    assert summary["Battery — energy recuperated"] == 0.0
    assert max(_values(result, "batt", "sig_soc")) <= 100.0
    assert "E-Motor — regeneration not recovered" not in summary
    speed = {p["t"]: p["value"] for p in series(result, "veh", "sig_speed")}
    assert speed[19.0] < 1.0
    assert max(_values(result, "brl", "sig_torque")) > 100.0
