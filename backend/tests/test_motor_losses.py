"""E-Motor loss convention: the loss map (motor + inverter) holds every loss
of the powered drive, spin losses at zero torque included, and the drag
table applies only while the inverter is off. Spin losses count once."""
import pytest
from helpers import bev_axle, conn, el, project, series

from app.library import library_by_id
from app.solver import simulate
from app.solver.domains import RunContext
from app.solver.maps import interp1, interp2, parse_table1d, parse_table2d
from app.solver.network import build_model
from app.solver.runtime import AIR_DENSITY, RPM, Runtime
from app.storage import load_project

OMEGA_8000 = 8000 / RPM  # rad/s


def _motor():
    proj = project(
        [el("batt", "battery.generic", "Battery"), el("bus", "electric.node", "Bus"),
         el("mot", "motor.emotor", "E-Motor")],
        [conn(1, "batt", "pos", "bus", "t1"), conn(2, "bus", "t2", "mot", "pos")], [])
    model = build_model(proj)
    ctx = RunContext(proj, model, Runtime(model, None), {}, {})
    ctx.dt = 0.01
    return ctx, ctx.motors["mot"]


def test_powered_motor_has_no_drag_on_top_of_its_loss_map():
    ctx, mc = _motor()
    volts = ctx.bus_voltage[ctx.motor_bus["mot"].id]
    t_shaft = ctx.motor_torque(mc, 0.1, OMEGA_8000)
    assert t_shaft == pytest.approx(0.1 * interp2(mc.full_load, volts, 8000))
    assert mc.torque == pytest.approx(t_shaft)  # recorded torque is the shaft torque
    loss_w = interp2(mc.loss, 8000, t_shaft) * 1000.0
    assert mc.p_mech_w == pytest.approx(t_shaft * OMEGA_8000)
    assert mc.p_elec_w == pytest.approx(t_shaft * OMEGA_8000 + loss_w)
    assert mc.p_loss_w == pytest.approx(loss_w)


def test_unpowered_motor_coasts_on_its_drag_torque():
    ctx, mc = _motor()
    drag = interp1(mc.drag, 8000)
    assert drag > 0.5  # the catalog default has drag at 8,000 1/min
    t_shaft = ctx.motor_torque(mc, 0.0, OMEGA_8000)
    assert t_shaft == pytest.approx(-drag)
    assert mc.torque == pytest.approx(-drag)
    assert mc.p_elec_w == 0.0  # the inverter is off: nothing drawn
    assert mc.p_loss_w == pytest.approx(drag * OMEGA_8000)


def test_steady_cruise_counts_the_spin_loss_once():
    """Lossless gears at a steady 80 km/h: the motor's shaft power equals the
    power the wheels pass to the road, and its electrical power exceeds that
    by exactly the loss map at the operating point. Subtracting the drag
    table as well (the old convention) leaves the shaft about 1.8 kW short."""
    proj = bev_axle(profile="0:80; 60:80")
    proj.cases[0].duration = 60
    for e in proj.systems[0].elements:
        if e.id == "veh":
            e.parameterOverrides["initial_speed_kmh"] = 80
        if e.id in ("fd", "diff"):
            e.parameterOverrides["efficiency_pct"] = 100
    result = simulate(proj, "case")
    assert result.status in ("success", "warning"), [m.text for m in result.messages]

    def last(el_id, port):
        return series(result, el_id, port)[-1]["value"]

    p_wheels = sum(last(w, "sig_torque") * last(w, "sig_speed") / RPM for w in ("whl", "whr"))
    p_shaft = last("mot", "sig_mech_power") * 1000.0
    p_elec = last("mot", "sig_elec_power") * 1000.0
    rpm, torque = last("mot", "sig_speed"), last("mot", "sig_torque")
    loss_map = parse_table2d(next(
        p.default for p in library_by_id()["motor.emotor"].parameters if p.key == "power_loss"))
    assert p_shaft == pytest.approx(p_wheels, rel=0.01)
    assert p_elec - p_shaft == pytest.approx(interp2(loss_map, rpm, abs(torque)) * 1000.0, rel=1e-3)


def _default(def_id, key):
    return next(p.default for p in library_by_id()[def_id].parameters if p.key == key)


def test_default_spin_loss_is_about_one_kilowatt_near_8000_rpm():
    """The default loss map's zero-torque column is the powered motor's spin
    loss: 0.5-1.5 kW near 8,000 1/min for a motor of this size (it was
    2.6 kW). Unpowered, only the drag is left, so it never costs more than
    the powered spin loss (it was 7.5 kW against 4.6 kW at 12,000 1/min)."""
    loss = parse_table2d(_default("motor.emotor", "power_loss"))
    drag = parse_table1d(_default("motor.emotor", "drag_torque"))
    assert 0.5 <= interp2(loss, 8000, 0) <= 1.5
    for rpm in range(0, 12001, 500):
        assert interp1(drag, rpm) * rpm / RPM / 1000 <= interp2(loss, rpm, 0), rpm


def test_bundled_bev_cruises_at_100_kmh_with_80_to_90_percent_battery_to_wheel():
    """MOD-04 metric: the bundled BEV at a steady 100 km/h turns 80-90 % of
    the battery's power, net of its auxiliary load, into road load (aero
    plus rolling resistance). It was 68.1 % with the spin loss counted twice
    and 79.8 % with the old default loss map; the Cupra Born rework (CON-03)
    gives the example its own motor maps and road load, read here from the
    example itself."""
    proj = load_project("bev-car")
    for e in proj.systems[0].elements:
        if e.id == "el-task":
            e.parameterOverrides["profile"] = "0:100; 60:100"
        if e.id == "el-vehicle":
            e.parameterOverrides["initial_speed_kmh"] = 100
    case = proj.cases[0]
    case.duration = 30
    result = simulate(proj, case.id)
    assert result.status == "success", [m.text for m in result.messages]

    def last(el_id, port):
        return series(result, el_id, port)[-1]["value"]

    params = build_model(proj).params_of
    veh = params["el-vehicle"]
    v = last("el-vehicle", "sig_speed") / 3.6
    assert v * 3.6 == pytest.approx(100, abs=0.5)
    rolling_n = sum(params[w]["rolling_resistance"] * params[w]["vehicle_load_share_pct"] / 100.0
                    for w in ("el-wheel-fl", "el-wheel-fr", "el-wheel-rl", "el-wheel-rr"))
    road_w = (0.5 * AIR_DENSITY * veh["cd"] * veh["frontal_area_m2"] * v * v
              + rolling_n * veh["mass_kg"] * 9.81) * v
    net_w = (last("el-battery", "sig_power") - last("el-consumer", "sig_power")) * 1000.0
    assert 0.80 <= road_w / net_w <= 0.90
