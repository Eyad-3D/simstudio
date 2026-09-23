"""Live parameter edits honor the catalog's variability metadata:
"fixed" (structural) params defer to the next run with a message,
"tunable" ones apply silently.
"""
import pytest
from helpers import bev_axle

from app.solver import simulate


def _run_with_edit(element_id: str, key: str, value):
    proj = bev_axle(profile="0:0; 5:60; 10:60")
    proj.cases[0].duration = 10
    sent = {"done": False}

    def control():
        if not sent["done"]:
            sent["done"] = True
            return [{"type": "set_param", "elementId": element_id, "key": key, "value": value}]
        return []

    return simulate(proj, "case", control=control)


def test_fixed_parameter_edit_defers_with_message():
    result = _run_with_edit("fd", "ratio", 5.0)
    assert result.status in ("success", "warning"), [m.text for m in result.messages]
    assert any(
        "'Final Drive.ratio' changed" in m.text and "next run" in m.text
        for m in result.messages
    ), [m.text for m in result.messages]


def test_tunable_parameter_edit_applies_silently():
    result = _run_with_edit("drv", "driver_kp", 0.5)
    assert result.status in ("success", "warning"), [m.text for m in result.messages]
    assert not any("take effect on the next run" in m.text for m in result.messages)


# ---- live edits and gear shifts (ENG-04) --------------------------------------

def _geared_car(gear_expr: str, profile: str = "0:0; 60:110", initial_kmh: float = 0.0):
    """Engine → clutch → gearbox → final drive (4.1) → diff → two wheels
    carrying the whole weight, gears chosen by a script."""
    from helpers import conn, dbc, driver_wiring, el, project, sig_port

    from app.schemas import ElementInstance

    elements = [
        el("veh", "vehicle.body", "Vehicle", initial_speed_kmh=initial_kmh),
        el("drv", "driver.driver", "Driver"),
        el("task", "signal.driving_task", "Task", profile=profile),
        el("eng", "engine.combustion", "Engine"),
        el("cl", "mech.clutch", "Clutch"),
        el("gb", "mech.gearbox", "Gearbox", default_gear=2),
        el("fd", "mech.final_drive", "Final Drive", ratio=4.1),
        el("diff", "mech.differential", "Differential"),
        el("whl", "propulsion.wheel", "Wheel L", vehicle_load_share_pct=50),
        el("whr", "propulsion.wheel", "Wheel R", vehicle_load_share_pct=50),
        ElementInstance(
            id="shift", componentDefId="signal.script", label="Shifter", position={"x": 0, "y": 0},
            parameterOverrides={"code": "def step(t, dt, inputs, state, params):\n"
                                        f"    return {{'gear': {gear_expr}}}\n"},
            dynamicPorts=[sig_port("gear", "output")]),
    ]
    connections = [
        conn(1, "eng", "shaft", "cl", "flange_a"),
        conn(2, "cl", "flange_b", "gb", "flange_in"),
        conn(3, "gb", "flange_out", "fd", "flange_in"),
        conn(4, "fd", "flange_out", "diff", "flange_in"),
        conn(5, "diff", "flange_out_a", "whl", "shaft"),
        conn(6, "diff", "flange_out_b", "whr", "shaft"),
    ]
    databus = [*driver_wiring(), dbc(2, "drv", "sig_accel_pedal", "eng", "sig_throttle_in"),
               dbc(3, "shift", "gear", "gb", "sig_gear_in")]
    return project(elements, connections, databus, duration=45, time_step=0.5)


def _spy_shifts(monkeypatch) -> list[float]:
    """Times of the 'reconfigured' events (driveline rebuilds for a shift)."""
    from app.solver import domains

    times: list[float] = []
    real = domains.GearSlave.do_step

    def spy(self, t, h):
        result = real(self, t, h)
        if result.events:
            times.append(t)
        return result
    monkeypatch.setattr(domains.GearSlave, "do_step", spy)
    return times


def test_live_edit_survives_two_gear_shifts(monkeypatch):
    """bench/exp_live.py: grip cut to 0.1 during the run held the tire force
    at 0.1 × load until the first shift, which rebuilt the driveline from the
    saved project and put the grip back to 1.0. A structural edit sent with
    it must stay deferred through the shifts."""
    from helpers import series

    shifts = _spy_shifts(monkeypatch)
    proj = _geared_car("1 if t < 6 else (2 if t < 14 else 3)", profile="0:0; 10:100; 60:100")
    proj.cases[0].duration = 25
    calls = {"n": 0}

    def control():
        calls["n"] += 1
        if calls["n"] == 3:  # at t = 1 s
            return [{"type": "set_param", "elementId": w, "key": "mu", "value": 0.1}
                    for w in ("whl", "whr")] + [
                {"type": "set_param", "elementId": "fd", "key": "ratio", "value": 2.0}]
        return []

    result = simulate(proj, "case", control=control)
    assert result.status in ("success", "warning"), [m.text for m in result.messages]
    assert [round(t, 2) for t in shifts] == [6.0, 14.0], shifts  # and none at t = 0

    limit = 0.1 * 0.5 * 1800 * 9.81  # grip × load on one wheel
    force = [(p["t"], abs(p["value"])) for p in series(result, "whl", "sig_force")]
    for lo, hi in ((2, 6), (6.5, 14), (14.5, 25)):  # before, between and after the shifts
        peak = max(f for t, f in force if lo <= t <= hi)
        assert limit * 0.9 < peak <= limit + 1e-6, (lo, hi, peak, limit)

    eng = {p["t"]: p["value"] for p in series(result, "eng", "sig_speed")}
    whl = {p["t"]: p["value"] for p in series(result, "whl", "sig_speed")}
    third = 1.6 * 4.1  # the catalog's 3rd gear × the final drive, not the deferred 2.0
    assert eng[24.0] / whl[24.0] == pytest.approx(third, rel=0.05)


def test_the_first_gear_sample_is_the_starting_gear(monkeypatch):
    """A car already moving at t = 0 with a script-selected gear: the old
    solver rebuilt the driveline at t = 0 with its speeds zeroed, so the
    wheels dropped from 402 to 17 1/min and the tires braked the car at
    their grip limit. Neither a gear equal to the default nor another one
    may cost a rebuild event at t = 0."""
    from helpers import series

    shifts = _spy_shifts(monkeypatch)
    for gear in ("2", "3"):  # the gearbox's default gear, and another one
        proj = _geared_car(gear, profile="0:50; 5:50", initial_kmh=50)
        proj.cases[0].duration, proj.cases[0].timeStep = 1.0, 0.01
        result = simulate(proj, "case")
        wheel_rpm = 50 / 3.6 / 0.33 * 60 / (2 * 3.141592653589793)
        speeds = [p["value"] for p in series(result, "whl", "sig_speed")][:5]
        assert min(speeds) > 0.9 * wheel_rpm, (gear, speeds)
    assert shifts == []


def test_drivelines_shift_in_the_same_step():
    """Two e-axles with their own two-speed gearbox on one gear signal: both
    change ratio at the same solver step (before, the second one a step
    later)."""
    from helpers import conn, dbc, el, project, series, sig_port

    from app.schemas import ElementInstance

    elements = [el("veh", "vehicle.body", "Vehicle", initial_speed_kmh=40),
                el("batt", "battery.generic", "Battery"), el("bus", "electric.node", "Bus"),
                ElementInstance(
                    id="shift", componentDefId="signal.script", label="Shifter",
                    position={"x": 0, "y": 0},
                    parameterOverrides={"code": "def step(t, dt, inputs, state, params):\n"
                                                "    return {'gear': 1 if t < 0.5 else 2}\n"},
                    dynamicPorts=[sig_port("gear", "output")])]
    connections = [conn(1, "batt", "pos", "bus", "t1")]
    databus = []
    for i, ax in enumerate(("f", "r")):
        elements += [el(f"mot{ax}", "motor.emotor", f"Motor {ax}"),
                     el(f"gb{ax}", "mech.gearbox", f"Gearbox {ax}", ratios={"1": 3.0, "2": 1.5}),
                     el(f"w{ax}", "propulsion.wheel", f"Wheel {ax}", vehicle_load_share_pct=50)]
        connections += [conn(10 + i, "bus", f"t{2 + i}", f"mot{ax}", "pos"),
                        conn(20 + i, f"mot{ax}", "shaft", f"gb{ax}", "flange_in"),
                        conn(30 + i, f"gb{ax}", "flange_out", f"w{ax}", "shaft")]
        databus.append(dbc(40 + i, "shift", "gear", f"gb{ax}", "sig_gear_in"))
    proj = project(elements, connections, databus, duration=1.0, time_step=0.01)
    result = simulate(proj, "case")
    for ax in ("f", "r"):
        mot = {round(p["t"], 2): p["value"] for p in series(result, f"mot{ax}", "sig_speed")}
        whl = {round(p["t"], 2): p["value"] for p in series(result, f"w{ax}", "sig_speed")}
        assert mot[0.5] / whl[0.5] == pytest.approx(3.0, rel=0.01), ax
        assert mot[0.51] / whl[0.51] == pytest.approx(1.5, rel=0.01), ax
