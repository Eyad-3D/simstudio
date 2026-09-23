"""Expansion components: engine/fuel, gearbox, clutch, transfer case,
fuel cell + DC-DC setpoint, PID, lookup, road profile."""
from helpers import bev_axle, conn, dbc, driver_wiring, el, project, series, sig_port

from app.schemas import ElementInstance
from app.solver import simulate
from app.solver.network import build_model
from app.validation import validate_project


def test_electrical_two_terminal_roles_and_implicit_return():
    # bev_axle wires only the positive rail; negatives are left implicit
    proj = bev_axle()
    model = build_model(proj)
    hv = [b for b in model.buses if b.battery == "batt"]
    assert hv, "battery should own its bus via the positive (+) terminal"
    assert "mot" in hv[0].motors, "motor draws on the bus via its positive terminal"
    assert "batt" in model.floating_returns and "mot" in model.floating_returns
    checks = validate_project(proj)
    assert any("implicit ground return" in c.text for c in checks if c.level == "info")
    assert not [c for c in checks if c.level == "error"]


def test_explicit_return_rail_clears_floating_and_solves():
    proj = bev_axle()
    proj.systems[0].elements.append(el("grnd", "boundary.ground", "Ground"))
    proj.systems[0].connections += [
        conn(30, "batt", "neg", "grnd", "t1"),
        conn(31, "mot", "neg", "grnd", "t2"),
    ]
    model = build_model(proj)
    assert "batt" not in model.floating_returns
    assert "mot" not in model.floating_returns
    result = simulate(proj, "case")
    assert result.status in ("success", "warning"), [m.text for m in result.messages]
    assert not any("implicit ground return" in m.text for m in result.messages)


def _ice_car(profile="0:0; 5:60; 60:60", tank_kg=45.0, gear_script=None):
    """Engine → clutch → gearbox → final drive → diff → two wheels."""
    elements = [
        el("veh", "vehicle.body", "Vehicle"),
        el("drv", "driver.driver", "Driver"),
        el("task", "signal.driving_task", "Task", profile=profile),
        el("eng", "engine.combustion", "Engine"),
        el("tank", "fuel.tank", "Tank", capacity_kg=tank_kg),
        el("cl", "mech.clutch", "Clutch"),
        el("gb", "mech.gearbox", "Gearbox", default_gear=2),
        el("fd", "mech.final_drive", "Final Drive", ratio=4.1),
        el("diff", "mech.differential", "Differential"),
        el("whl", "propulsion.wheel", "Wheel L"),
        el("whr", "propulsion.wheel", "Wheel R"),
    ]
    connections = [
        conn(1, "eng", "shaft", "cl", "flange_a"),
        conn(2, "cl", "flange_b", "gb", "flange_in"),
        conn(3, "gb", "flange_out", "fd", "flange_in"),
        conn(4, "fd", "flange_out", "diff", "flange_in"),
        conn(5, "diff", "flange_out_a", "whl", "shaft"),
        conn(6, "diff", "flange_out_b", "whr", "shaft"),
    ]
    databus = [
        *driver_wiring(),
        dbc(2, "drv", "sig_accel_pedal", "eng", "sig_throttle_in"),
    ]
    proj = project(elements, connections, databus, duration=60, time_step=0.5)
    if gear_script:
        proj.systems[0].elements.append(gear_script)
        proj.dataBusConnections.append(dbc(9, "shift", "gear", "gb", "sig_gear_in"))
    return proj


def test_ice_car_drives_and_burns_fuel():
    proj = _ice_car()
    checks = validate_project(proj)
    assert not [c for c in checks if c.level == "error"], [c.text for c in checks]
    result = simulate(proj, "case")
    assert result.status in ("success", "warning"), [m.text for m in result.messages]

    speed = {p["t"]: p["value"] for p in series(result, "veh", "sig_speed")}
    assert 52 < speed[40.0] < 68, f"ICE car should hold ~60 km/h, got {speed[40.0]}"
    fuel = [p["value"] for p in series(result, "tank", "sig_mass")]
    assert fuel[-1] < fuel[0], "fuel must be consumed"
    rpm = [p["value"] for p in series(result, "eng", "sig_speed")]
    assert max(rpm) > 1500, "engine must rev under load"


def test_engine_idles_and_stalls_when_dry():
    proj = _ice_car(profile="0:0; 60:0", tank_kg=0.003)  # a few grams of fuel
    result = simulate(proj, "case")
    rpm = [p["value"] for p in series(result, "eng", "sig_speed")]
    assert max(rpm[:20]) > 500, "idle governor should spin the engine up"
    assert rpm[-1] < 100, "engine should stall once the tank is dry"
    assert any("tank empty" in m.text.lower() for m in result.messages)


def test_gear_shift_changes_engine_speed_ratio():
    shift = ElementInstance(
        id="shift", componentDefId="signal.script", label="Shifter",
        position={"x": 0, "y": 0},
        parameterOverrides={"code": (
            "def step(t, dt, inputs, state, params):\n"
            "    return {'gear': 2 if t < 30 else 4}\n")},
        dynamicPorts=[sig_port("gear", "output")],
    )
    proj = _ice_car(profile="0:0; 5:50; 60:50", gear_script=shift)
    result = simulate(proj, "case")
    eng = {p["t"]: p["value"] for p in series(result, "eng", "sig_speed")}
    whl = {p["t"]: p["value"] for p in series(result, "whl", "sig_speed")}
    gear = {p["t"]: p["value"] for p in series(result, "gb", "sig_gear")}
    assert gear[20.0] == 2 and gear[50.0] == 4
    r_before = eng[25.0] / max(whl[25.0], 1e-6)
    r_after = eng[55.0] / max(whl[55.0], 1e-6)
    # 2nd gear 2.4 → 4th gear 1.15: overall ratio drops by ~2.09×
    assert 1.7 < r_before / max(r_after, 1e-6) < 2.5, (r_before, r_after)


def test_clutch_open_disconnects_engine():
    proj = _ice_car(profile="0:0; 60:0")
    proj.systems[0].elements.append(ElementInstance(
        id="zero", componentDefId="signal.constant", label="Open",
        position={"x": 0, "y": 0}, parameterOverrides={"value": 0},
    ))
    proj.dataBusConnections.append(dbc(8, "zero", "sig_out", "cl", "sig_engage_in"))
    result = simulate(proj, "case")
    rpm = [p["value"] for p in series(result, "eng", "sig_speed")]
    assert 600 < rpm[-1] < 1100, "engine should idle free with the clutch open"
    v = [p["value"] for p in series(result, "veh", "sig_speed")]
    assert max(v) < 0.5, "no drive torque reaches the wheels"
    t_cl = [abs(p["value"]) for p in series(result, "cl", "sig_torque")]
    assert max(t_cl) < 1e-6


def _awd(locked: bool, mu_front: float):
    """Motor → transfer case → two axle diffs → four wheels."""
    elements = [
        el("veh", "vehicle.body", "Vehicle"),
        el("drv", "driver.driver", "Driver"),
        el("task", "signal.driving_task", "Task", profile="0:0; 5:100; 30:100"),
        el("batt", "battery.generic", "Battery"),
        el("hvbus", "electric.node", "HV Bus"),
        el("mot", "motor.emotor", "E-Motor"),
        el("fd", "mech.final_drive", "Final Drive"),
        el("tc", "mech.transfer_case", "Transfer Case", locked=locked),
        el("dfff", "mech.differential", "Front Diff"),
        el("dffr", "mech.differential", "Rear Diff"),
        el("wfl", "propulsion.wheel", "Wheel FL", mu=mu_front),
        el("wfr", "propulsion.wheel", "Wheel FR", mu=mu_front),
        el("wrl", "propulsion.wheel", "Wheel RL"),
        el("wrr", "propulsion.wheel", "Wheel RR"),
    ]
    connections = [
        conn(1, "batt", "pos", "hvbus", "t1"),
        conn(2, "hvbus", "t3", "mot", "pos"),
        conn(3, "mot", "shaft", "fd", "flange_in"),
        conn(4, "fd", "flange_out", "tc", "flange_in"),
        conn(5, "tc", "flange_out_a", "dfff", "flange_in"),
        conn(6, "tc", "flange_out_b", "dffr", "flange_in"),
        conn(7, "dfff", "flange_out_a", "wfl", "shaft"),
        conn(8, "dfff", "flange_out_b", "wfr", "shaft"),
        conn(9, "dffr", "flange_out_a", "wrl", "shaft"),
        conn(10, "dffr", "flange_out_b", "wrr", "shaft"),
    ]
    databus = [
        *driver_wiring(),
        dbc(2, "drv", "sig_traction_cmd", "mot", "sig_demand_in"),
    ]
    return project(elements, connections, databus)


def test_transfer_case_awd_locked_vs_open_on_icy_front_axle():
    r_open = simulate(_awd(False, 0.1), "case")
    r_lock = simulate(_awd(True, 0.1), "case")
    assert r_open.status in ("success", "warning"), [m.text for m in r_open.messages]
    # compared while accelerating: both reach the 100 km/h plateau by t = 30 s
    v_open = {p["t"]: p["value"] for p in series(r_open, "veh", "sig_speed")}[10.0]
    v_lock = {p["t"]: p["value"] for p in series(r_lock, "veh", "sig_speed")}[10.0]
    slip_front_open = max(abs(p["value"]) for p in series(r_open, "wfl", "sig_slip"))
    assert slip_front_open > 0.5, "open center: icy front axle spins up"
    assert v_lock > v_open + 3, "locked center pushes torque to the gripping rear axle"

    # uniform grip: locked == open
    v_o = series(simulate(_awd(False, 1.0), "case"), "veh", "sig_speed")[-1]["value"]
    v_l = series(simulate(_awd(True, 1.0), "case"), "veh", "sig_speed")[-1]["value"]
    assert abs(v_o - v_l) < 1.5


def test_fuel_cell_via_dcdc_setpoint_into_battery_bus():
    elements = [
        el("veh", "vehicle.body", "Vehicle"),
        el("drv", "driver.driver", "Driver"),
        el("task", "signal.driving_task", "Task", profile="0:0; 5:70; 120:70"),
        el("fc", "fuelcell.stack", "Fuel Cell"),
        el("h2", "fuel.h2_tank", "H2 Tank"),
        el("dc", "controller.dcdc", "DC-DC", power_setpoint_kW=25),
        el("fcbus", "electric.node", "FC Bus"),
        el("batt", "battery.generic", "Battery", initial_soc_pct=60),
        el("hvbus", "electric.node", "HV Bus"),
        el("mot", "motor.emotor", "E-Motor"),
        el("fd", "mech.final_drive", "Final Drive"),
        el("diff", "mech.differential", "Differential"),
        el("whl", "propulsion.wheel", "Wheel L"),
        el("whr", "propulsion.wheel", "Wheel R"),
    ]
    connections = [
        conn(1, "fc", "pos", "fcbus", "t1"),
        conn(2, "fcbus", "t2", "dc", "a_pos"),
        conn(3, "dc", "b_pos", "hvbus", "t1"),
        conn(4, "batt", "pos", "hvbus", "t2"),
        conn(5, "hvbus", "t3", "mot", "pos"),
        conn(6, "mot", "shaft", "fd", "flange_in"),
        conn(7, "fd", "flange_out", "diff", "flange_in"),
        conn(8, "diff", "flange_out_a", "whl", "shaft"),
        conn(9, "diff", "flange_out_b", "whr", "shaft"),
    ]
    databus = [
        *driver_wiring(),
        dbc(2, "drv", "sig_traction_cmd", "mot", "sig_demand_in"),
    ]
    proj = project(elements, connections, databus, duration=120, time_step=1.0)
    result = simulate(proj, "case")
    assert result.status in ("success", "warning"), [m.text for m in result.messages]

    fc_p = [p["value"] for p in series(result, "fc", "sig_power")]
    assert abs(max(fc_p) - 25.0 / 0.97) < 1.5, "FC supplies the DC-DC setpoint / efficiency"
    h2 = [p["value"] for p in series(result, "h2", "sig_mass")]
    assert h2[-1] < h2[0], "hydrogen must be consumed"
    # steady 70 km/h needs < 25 kW, so the battery must be charging there
    batt_i = {p["t"]: p["value"] for p in series(result, "batt", "sig_current")}
    assert batt_i[100.0] < -1.0, "surplus FC power charges the battery"
    speed = {p["t"]: p["value"] for p in series(result, "veh", "sig_speed")}
    assert 64 < speed[100.0] < 76


def test_pid_controller_tracks_setpoint():
    elements = [
        el("sp", "signal.constant", "Setpoint", value=50),
        el("pid", "control.pid", "PID", kp=0.2, ki=0.8, out_min=0, out_max=100),
    ]
    databus = [
        dbc(1, "sp", "sig_out", "pid", "sig_setpoint_in"),
        dbc(2, "pid", "sig_out", "pid", "sig_feedback_in"),  # unity feedback loop
    ]
    proj = project(elements, [], databus, duration=60, time_step=0.5)
    result = simulate(proj, "case")
    out = [p["value"] for p in series(result, "pid", "sig_out")]
    assert abs(out[-1] - 50.0) < 1.0, f"integral action should close the loop, got {out[-1]}"


def test_lookup_block_1d_and_2d():
    elements = [
        el("x", "signal.constant", "X", value=0.5),
        el("y", "signal.constant", "Y", value=2.0),
        el("lk1", "signal.lookup", "Doubler", table_1d={"0": 0, "1": 2}),
        el("lk2", "signal.lookup", "Product", mode="2D",
           table_2d={"0": {"0": 0, "4": 0}, "1": {"0": 0, "4": 4}}),
    ]
    databus = [
        dbc(1, "x", "sig_out", "lk1", "sig_x_in"),
        dbc(2, "x", "sig_out", "lk2", "sig_x_in"),
        dbc(3, "y", "sig_out", "lk2", "sig_y_in"),
    ]
    proj = project(elements, [], databus, duration=5, time_step=1.0)
    result = simulate(proj, "case")
    assert abs(series(result, "lk1", "sig_out")[-1]["value"] - 1.0) < 1e-6
    assert abs(series(result, "lk2", "sig_out")[-1]["value"] - 1.0) < 1e-6  # 0.5·2 bilinear


def test_road_profile_time_mode_and_vehicle_grade():
    elements = [
        el("veh", "vehicle.body", "Vehicle"),
        el("drv", "driver.driver", "Driver"),
        el("task", "signal.driving_task", "Task", profile="0:30; 60:30"),
        el("road", "signal.road_profile", "Road", mode="time",
           profile="0:0; 20:5; 40:5; 60:0"),
        el("batt", "battery.generic", "Battery"),
        el("hvbus", "electric.node", "HV Bus"),
        el("mot", "motor.emotor", "E-Motor"),
        el("fd", "mech.final_drive", "Final Drive"),
        el("diff", "mech.differential", "Differential"),
        el("whl", "propulsion.wheel", "Wheel L"),
        el("whr", "propulsion.wheel", "Wheel R"),
    ]
    connections = [
        conn(1, "batt", "pos", "hvbus", "t1"),
        conn(2, "hvbus", "t3", "mot", "pos"),
        conn(3, "mot", "shaft", "fd", "flange_in"),
        conn(4, "fd", "flange_out", "diff", "flange_in"),
        conn(5, "diff", "flange_out_a", "whl", "shaft"),
        conn(6, "diff", "flange_out_b", "whr", "shaft"),
    ]
    databus = [
        *driver_wiring(),
        dbc(2, "drv", "sig_traction_cmd", "mot", "sig_demand_in"),
        dbc(3, "road", "sig_grade", "veh", "sig_grade_in"),
    ]
    proj = project(elements, connections, databus, duration=60, time_step=0.5)
    result = simulate(proj, "case")
    grade = {p["t"]: p["value"] for p in series(result, "road", "sig_grade")}
    assert grade[30.0] == 5.0 and grade[5.0] < 2.0
    # on the 5 % climb at 30 km/h the motor works visibly harder
    torque = {p["t"]: p["value"] for p in series(result, "mot", "sig_torque")}
    assert torque[30.0] > torque[10.0] + 5


def test_hybrid_p2_hcu_charges_low_battery():
    """Engine —clutch— (node: motor) — gearbox — FD — diff — wheels,
    supervised by an HCU script: low SOC forces the engine on, and the
    motor load-shifts to charge the battery while cruising."""
    hcu_code = (
        "def step(t, dt, inputs, state, params):\n"
        "    soc = inputs.get('soc', 60.0)\n"
        "    v = inputs.get('speed', 0.0)\n"
        "    trac = inputs.get('traction', 0.0)\n"
        "    on = state.get('on', 0.0)\n"
        "    if on:\n"
        "        if soc > 62.0 and trac < 0.55:\n"
        "            on = 0.0\n"
        "    else:\n"
        "        if soc < 48.0 or trac > 0.85:\n"
        "            on = 1.0\n"
        "    state['on'] = on\n"
        "    clutch = 1.0 if (on and v > 15.0) else 0.0\n"
        "    throttle = clamp(trac + 0.25, 0.0, 1.0) if clutch > 0.5 else (0.3 if on else 0.0)\n"
        "    motor = trac if clutch < 0.5 else clamp(trac - 0.3, -1.0, 0.3)\n"
        "    gear = 1.0\n"
        "    for g, up in ((2, 20), (3, 35), (4, 55), (5, 75), (6, 95)):\n"
        "        if v > up:\n"
        "            gear = float(g)\n"
        "    return {'motor_cmd': motor, 'throttle': throttle, 'engine_on': on,\n"
        "            'clutch_cmd': clutch, 'gear': gear}\n"
    )
    elements = [
        el("veh", "vehicle.body", "Vehicle"),
        el("drv", "driver.driver", "Driver"),
        el("task", "signal.driving_task", "Task", profile="0:0; 10:60; 180:60"),
        el("batt", "battery.generic", "Battery", initial_soc_pct=40, capacity_kWh=10),
        el("hvbus", "electric.node", "HV Bus"),
        el("mot", "motor.emotor", "E-Motor"),
        el("eng", "engine.combustion", "Engine"),
        el("tank", "fuel.tank", "Tank"),
        el("cl", "mech.clutch", "Clutch"),
        el("nd", "mech.node", "P2 Node"),
        el("gb", "mech.gearbox", "Gearbox"),
        el("fd", "mech.final_drive", "Final Drive", ratio=4.1),
        el("diff", "mech.differential", "Differential"),
        el("whl", "propulsion.wheel", "Wheel L"),
        el("whr", "propulsion.wheel", "Wheel R"),
        ElementInstance(
            id="hcu", componentDefId="signal.script", label="HCU",
            position={"x": 0, "y": 0}, parameterOverrides={"code": hcu_code},
            dynamicPorts=[
                sig_port("soc", "input"), sig_port("speed", "input"),
                sig_port("traction", "input"),
                sig_port("motor_cmd", "output"), sig_port("throttle", "output"),
                sig_port("engine_on", "output"), sig_port("clutch_cmd", "output"),
                sig_port("gear", "output"),
            ],
        ),
    ]
    connections = [
        conn(1, "batt", "pos", "hvbus", "t1"),
        conn(2, "hvbus", "t3", "mot", "pos"),
        conn(3, "eng", "shaft", "cl", "flange_a"),
        conn(4, "cl", "flange_b", "nd", "f1"),
        conn(5, "mot", "shaft", "nd", "f2"),
        conn(6, "nd", "f3", "gb", "flange_in"),
        conn(7, "gb", "flange_out", "fd", "flange_in"),
        conn(8, "fd", "flange_out", "diff", "flange_in"),
        conn(9, "diff", "flange_out_a", "whl", "shaft"),
        conn(10, "diff", "flange_out_b", "whr", "shaft"),
    ]
    databus = [
        *driver_wiring(),
        dbc(2, "batt", "sig_soc", "hcu", "soc"),
        dbc(3, "veh", "sig_speed", "hcu", "speed"),
        dbc(4, "drv", "sig_traction_cmd", "hcu", "traction"),
        dbc(5, "hcu", "motor_cmd", "mot", "sig_demand_in"),
        dbc(6, "hcu", "throttle", "eng", "sig_throttle_in"),
        dbc(7, "hcu", "engine_on", "eng", "sig_on_in"),
        dbc(8, "hcu", "clutch_cmd", "cl", "sig_engage_in"),
        dbc(9, "hcu", "gear", "gb", "sig_gear_in"),
    ]
    proj = project(elements, connections, databus, duration=180, time_step=1.0)
    checks = validate_project(proj)
    assert not [c for c in checks if c.level == "error"], [c.text for c in checks]
    result = simulate(proj, "case")
    assert result.status in ("success", "warning"), [m.text for m in result.messages]

    speed = {p["t"]: p["value"] for p in series(result, "veh", "sig_speed")}
    assert 52 < speed[120.0] < 68, f"hybrid should cruise ~60 km/h, got {speed[120.0]}"
    on = [p["value"] for p in series(result, "hcu", "engine_on")]
    assert max(on) == 1.0, "low SOC must switch the engine on"
    soc = [p["value"] for p in series(result, "batt", "sig_soc")]
    assert soc[-1] > soc[60] + 0.5, "engine surplus should charge the battery while cruising"
    fuel = [p["value"] for p in series(result, "tank", "sig_mass")]
    assert fuel[-1] < fuel[0]
