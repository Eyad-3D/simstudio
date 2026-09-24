"""Combustion engine: the full-load curve and fuel map are brake (net)
maps, the drag table applies only while the engine is not fired, zero
throttle in overrun cuts the fuel, a rev limiter cuts fuel and torque above
the full-load curve's last speed, and the run reports CO₂."""
import pytest
from helpers import conn, dbc, driver_wiring, el, project, series

from app.solver import simulate
from app.solver.domains import RunContext
from app.solver.maps import interp1, interp2
from app.solver.network import build_model
from app.solver.runtime import RPM, Runtime


def _engine(throttle: float):
    proj = project([el("eng", "engine.combustion", "Engine"),
                    el("thr", "signal.constant", "Throttle", value=throttle)],
                   [], [dbc(1, "thr", "sig_out", "eng", "sig_throttle_in")])
    model = build_model(proj)
    rt = Runtime(model, None)
    ctx = RunContext(proj, model, rt, {}, {})
    ctx.dt = 0.01
    rt.publish("thr", "sig_out", throttle)
    return ctx, ctx.engines["eng"]


def test_full_throttle_delivers_the_full_load_curve():
    """Before, the drag table was subtracted while firing: 58.9 kW peak
    against 81.7 kW on the default full-load curve."""
    ctx, ec = _engine(1.0)
    peak_kw = 0.0
    for rpm, t_full in ec.full_load.pts:
        torque = ctx.engine_torque(ec, rpm / RPM)
        assert torque == pytest.approx(t_full), rpm
        assert ec.fuel_kgh == pytest.approx(interp2(ec.fuel_map.pts, rpm, t_full)), rpm
        peak_kw = max(peak_kw, torque * rpm / RPM / 1000.0)
    assert peak_kw >= 80.0


def test_zero_throttle_cuts_fuel_above_the_reentry_speed():
    """Before, a lifted pedal still burned map(speed, 0), 1.4 kg/h at
    3,000 1/min, while the engine was dragged by the wheels."""
    ctx, ec = _engine(0.0)
    assert ec.reentry_rpm == 1100
    for rpm in (1200, 2000, 3000, 4500, 6000):
        torque = ctx.engine_torque(ec, rpm / RPM)
        assert ec.fuel_kgh == 0.0, rpm
        assert torque == pytest.approx(-interp1(ec.drag.pts, rpm)), rpm


def test_idle_governor_holds_idle_on_a_willans_line():
    """At zero throttle below the re-entry speed the governor fuels the
    engine: zero brake torque at idle on map(idle, 0), less fuel above idle
    down to none at the drag torque, extra torque below idle."""
    ctx, ec = _engine(0.0)
    idle = ec.idle_rpm
    assert ctx.engine_torque(ec, idle / RPM) == pytest.approx(0.0)
    assert ec.fuel_kgh == pytest.approx(interp2(ec.fuel_map.pts, idle, 0.0))
    # half-way to the drag torque, half the zero-torque fuel
    rpm = idle + 0.25 * idle * interp1(ec.drag.pts, idle) / 2 / interp1(ec.full_load.pts, idle)
    torque = ctx.engine_torque(ec, rpm / RPM)
    assert torque == pytest.approx(-interp1(ec.drag.pts, rpm) / 2, rel=0.02)
    assert ec.fuel_kgh == pytest.approx(interp2(ec.fuel_map.pts, rpm, 0.0) / 2, rel=0.02)
    assert ctx.engine_torque(ec, 0.9 * idle / RPM) > 0.0


def test_rev_limiter_cuts_fuel_and_torque():
    """Before, a free-revving engine at full throttle ran up to 17,700
    1/min on a curve that ends at 6,000, its torque held flat."""
    proj = project([el("eng", "engine.combustion", "Engine"),
                    el("thr", "signal.constant", "Throttle", value=1.0),
                    el("sh", "mech.shaft", "Shaft")],
                   [conn(1, "eng", "shaft", "sh", "flange_a")],
                   [dbc(1, "thr", "sig_out", "eng", "sig_throttle_in")], duration=5, time_step=0.01)
    result = simulate(proj, "case")
    rpm = [p["value"] for p in series(result, "eng", "sig_speed")]
    fuel = [p["value"] for p in series(result, "eng", "sig_fuel_rate")]
    torque = [p["value"] for p in series(result, "eng", "sig_torque")]
    assert 6000 < max(rpm) < 6100
    for n, f, tq in zip(rpm, fuel, torque):
        if n > 6000:
            assert f == 0.0 and tq < 0.0
    # reaching the limiter is info (MOD-18): the run is judged on how long it
    # stays there, and overshooting it by a step is not counted as over speed
    assert [m.level for m in result.messages if "rev limiter" in m.text] == ["info"]
    assert not [s.label for s in result.summary if "maximum speed" in s.label]


def _ice_car(profile: str, duration: float, speed: float = 100.0, **tank):
    elements = [
        el("veh", "vehicle.body", "Vehicle", initial_speed_kmh=speed),
        el("drv", "driver.driver", "Driver"),
        el("task", "signal.driving_task", "Task", profile=profile),
        el("eng", "engine.combustion", "Engine"),
        el("tank", "fuel.tank", "Tank", **tank),
        el("cl", "mech.clutch", "Clutch"),
        el("gb", "mech.gearbox", "Gearbox", default_gear=4),
        el("fd", "mech.final_drive", "Final Drive", ratio=4.1),
        el("diff", "mech.differential", "Differential"),
        el("whl", "propulsion.wheel", "Wheel L"),
        el("whr", "propulsion.wheel", "Wheel R"),
    ]
    connections = [
        conn(1, "eng", "shaft", "cl", "flange_a"), conn(2, "cl", "flange_b", "gb", "flange_in"),
        conn(3, "gb", "flange_out", "fd", "flange_in"), conn(4, "fd", "flange_out", "diff", "flange_in"),
        conn(5, "diff", "flange_out_a", "whl", "shaft"), conn(6, "diff", "flange_out_b", "whr", "shaft"),
    ]
    databus = [*driver_wiring(), dbc(2, "drv", "sig_traction_cmd", "eng", "sig_throttle_in")]
    return project(elements, connections, databus, duration=duration, time_step=0.1)


def test_engine_starts_at_the_vehicle_speed_behind_a_closed_clutch():
    """At 100 km/h from the start, an engine behind a closed clutch turns
    with the wheels from t = 0 (4th gear 1.15 x final drive 4.1); behind a
    clutch a Constant holds open it starts at rest, as it would with the
    engine off. Before, it always started at rest and the closed clutch
    tore it up to speed in the first steps."""
    closed = simulate(_ice_car("0:100; 1:100", 0.2), "case")
    wheel_rpm = series(closed, "whl", "sig_speed")[0]["value"]
    assert wheel_rpm > 800
    assert series(closed, "eng", "sig_speed")[0]["value"] == pytest.approx(1.15 * 4.1 * wheel_rpm)
    assert abs(series(closed, "cl", "sig_slip_speed")[1]["value"]) < 10.0

    proj = _ice_car("0:100; 1:100", 0.2)
    proj.systems[0].elements.append(el("open", "signal.constant", "Open", value=0))
    proj.dataBusConnections.append(dbc(3, "open", "sig_out", "cl", "sig_engage_in"))
    opened = simulate(proj, "case")
    assert series(opened, "eng", "sig_speed")[0]["value"] == 0.0
    assert series(opened, "whl", "sig_speed")[0]["value"] == pytest.approx(wheel_rpm)


def test_coasting_in_gear_burns_no_fuel_and_co2_follows_the_fuel():
    result = simulate(_ice_car("0:100; 5:100; 25:60; 40:60", 40), "case")
    assert result.status in ("success", "warning"), [m.text for m in result.messages]
    pedal = [p["value"] for p in series(result, "drv", "sig_accel_pedal")]
    rpm = [p["value"] for p in series(result, "eng", "sig_speed")]
    fuel = [p["value"] for p in series(result, "eng", "sig_fuel_rate")]
    coasting = [f for a, n, f in zip(pedal, rpm, fuel) if a == 0.0 and n > 1100]
    assert len(coasting) > 100 and max(coasting) == 0.0

    s = {x.label: x.value for x in result.summary}
    # l/100 km × kg/l × kg CO₂/kg × 10 = g/km (petrol defaults of the tank)
    assert s["CO₂ emissions"] == pytest.approx(s["Fuel consumption"] * 0.745 * 3.17 * 10.0, rel=5e-3)


def test_co2_uses_the_tanks_factor():
    petrol = {x.label: x.value for x in simulate(_ice_car("0:60; 60:60", 60, speed=60), "case").summary}
    diesel = {x.label: x.value for x in simulate(
        _ice_car("0:60; 60:60", 60, speed=60, co2_kg_per_kg=3.16, density_kg_per_l=0.835),
        "case").summary}
    assert petrol["Fuel consumption"] > 3.0  # a steady cruise, not a coast-down
    assert diesel["CO₂ emissions"] == pytest.approx(petrol["CO₂ emissions"] * 3.16 / 3.17, rel=1e-3)
    assert diesel["Fuel consumption"] == pytest.approx(
        petrol["Fuel consumption"] * 0.745 / 0.835, rel=5e-3)
