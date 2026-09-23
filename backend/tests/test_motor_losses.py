"""E-Motor loss convention: the loss map (motor + inverter) holds every loss
of the powered drive, spin losses at zero torque included, and the drag
table applies only while the inverter is off. Spin losses count once."""
import pytest
from helpers import bev_axle, conn, el, project, series

from app.library import library_by_id
from app.solver import simulate
from app.solver.domains import RunContext
from app.solver.maps import interp1, interp2, parse_table2d
from app.solver.network import build_model
from app.solver.runtime import RPM, Runtime

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
    assert drag > 1.0  # the catalog default has drag at 8,000 1/min
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
