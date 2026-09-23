"""Gear losses act on the power that flows through each gear, whichever port
the solver walked a rigid section of the driveline from.

A P2 driveline (engine, clutch, E-Motor on a node, gearbox, final drive,
differential) cruises at a steady 60 km/h. The power at the wheels is the
power put in times every efficiency on the way: gearbox 95 %, final drive
90 % and differential 98 %. That holds when the motor drives, and when the
engine drives through the clutch while the unpowered motor drags (the net
of the two crosses the gears). Recuperating downhill, the motor gets that
share of what the wheels put in. Before, the section was walked from the
node (the clutch asked for it before the differential did), so the power
of the motor and the engine reached the wheels past the gearbox and the
final drive without their losses.
"""
import pytest
from helpers import conn, dbc, driver_wiring, el, project, series

from app.solver import simulate
from app.solver.runtime import RPM

ETA = 0.95 * 0.90 * 0.98  # gearbox × final drive × differential


def _p2(drive: str, grade_pct: float = 0.0):
    """Driven by the E-Motor (engine off, clutch open) or by the engine
    (clutch closed, motor unpowered), at 60 km/h from the start."""
    elements = [
        el("veh", "vehicle.body", "Vehicle", initial_speed_kmh=60, mass_kg=1500),
        el("drv", "driver.driver", "Driver", regen_weight_pct=100),
        el("task", "signal.driving_task", "Task", profile="0:60; 40:60"),
        el("batt", "battery.generic", "Battery", initial_soc_pct=60),
        el("hvbus", "electric.node", "HV Bus"),
        el("mot", "motor.emotor", "E-Motor"),
        el("eng", "engine.combustion", "Engine"),
        el("cl", "mech.clutch", "Clutch"),
        el("nd", "mech.node", "P2 Node"),
        el("gb", "mech.gearbox", "Gearbox", default_gear=6, efficiency_pct=95),
        el("fd", "mech.final_drive", "Final Drive", efficiency_pct=90),
        el("diff", "mech.differential", "Differential", efficiency_pct=98),
        el("whl", "propulsion.wheel", "Wheel L"),
        el("whr", "propulsion.wheel", "Wheel R"),
        el("engine_on", "signal.constant", "Engine On", value=float(drive == "engine")),
        el("grade", "signal.constant", "Grade", value=grade_pct),
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
        dbc(2, "engine_on", "sig_out", "eng", "sig_on_in"),
        dbc(3, "engine_on", "sig_out", "cl", "sig_engage_in"),
        dbc(4, "grade", "sig_out", "veh", "sig_grade_in"),
    ]
    if drive == "engine":
        databus.append(dbc(5, "drv", "sig_accel_pedal", "eng", "sig_throttle_in"))
    else:
        databus.append(dbc(5, "drv", "sig_traction_cmd", "mot", "sig_demand_in"))
    return project(elements, connections, databus, duration=40, time_step=0.5)


@pytest.mark.parametrize("drive,grade_pct", [("motor", 0.0), ("engine", 0.0), ("motor", -4.0)])
def test_gear_losses_act_on_the_power_through_the_gears(drive, grade_pct):
    result = simulate(_p2(drive, grade_pct), "case")
    assert result.status in ("success", "warning"), [m.text for m in result.messages]

    def last(el_id, port):
        return series(result, el_id, port)[-1]["value"]

    assert last("veh", "sig_speed") == pytest.approx(60, abs=0.2)
    p_wheels = sum(last(w, "sig_torque") * last(w, "sig_speed") / RPM for w in ("whl", "whr"))
    p_in = (last("mot", "sig_mech_power") + last("eng", "sig_power")) * 1000.0
    if drive == "engine":
        assert last("mot", "sig_mech_power") < -0.1  # the unpowered motor drags
    if grade_pct < 0:
        assert p_in < -2000.0  # recuperating
        assert p_in == pytest.approx(ETA * p_wheels, rel=0.005)
    else:
        assert p_in > 2000.0
        assert p_wheels == pytest.approx(ETA * p_in, rel=0.005)
