"""Every part reports what goes in, comes out and is lost (MOD-10).

Metric: for every example, each part's in − out − loss − Δstored closes
(here exactly, by construction), and the books of all parts together close
within a small residual of the energy the sources gave; an all-wheel-drive
car shows different power on its front and rear final drives.
"""
from __future__ import annotations

import pytest
from helpers import bev_axle, conn, dbc, driver_wiring, el, example_result, project, series

from app.solver import simulate
from app.storage import load_example


def _flows(result) -> dict:
    return {f.label: f for f in result.partEnergy}


def _row(result, label):
    return next(s for s in result.summary if s.label == label)


@pytest.mark.parametrize("project_id,case_id", [
    ("bev-car", "case-city"), ("hybrid-car", "case-mixed"), ("fs-electric", "case-accel-75m")])
def test_each_part_closes_and_all_together_nearly(project_id, case_id):
    r = example_result(project_id, case_id)
    assert r.partEnergy
    for f in r.partEnergy:
        assert f.energyIn >= 0 and f.energyOut >= 0
        assert f.energyIn - f.energyOut - f.losses - f.stored == pytest.approx(0.0, abs=2e-6)
    # every flow out of one part is (nearly) the flow into the next: what
    # is left is the solver's step (0.01 % to 0.4 % in the examples)
    assert abs(_row(r, "Energy balance residual").value) < 0.5


def test_battery_and_motor_agree_with_the_summary_and_channels():
    r = example_result("bev-car", "case-city")
    f = _flows(r)
    s = {x.label: x.value for x in r.summary}
    bat = f["HV Battery Pack"]
    assert bat.energyOut == pytest.approx(s["HV Battery Pack — energy delivered"], abs=1e-3)
    assert bat.energyIn == pytest.approx(s["HV Battery Pack — energy recuperated"], abs=1e-3)
    assert bat.losses == pytest.approx(s["HV Battery Pack — internal losses"], abs=1e-4)
    mot = f["E-Motor"]
    assert mot.losses > 0 and mot.rmsPower > 0 and mot.peakPower >= mot.rmsPower
    # the vehicle's terms: air drag and rolling resistance are its losses
    veh = f["Vehicle"]
    assert veh.terms["air drag"] + veh.terms["rolling resistance"] == pytest.approx(
        veh.losses, abs=1e-4)
    assert veh.peakPower is not None and f["Wheel FL"].rmsPower is None


def test_final_drive_power_is_its_own_not_the_driveline_total():
    """KNOWN-LIMITS 0.2.0: a Shaft between the hybrid's engine and clutch and
    its Final Drive both showed the summed power of engine and motor."""
    p = load_example("hybrid-car")
    root = p.systems[0]
    # put a Shaft between the engine and the clutch
    for c in root.connections:
        if {c.sourceElementId, c.targetElementId} == {"el-engine", "el-clutch"}:
            if c.sourceElementId == "el-engine":
                c.sourceElementId, c.sourcePortId = "el-shaft", "flange_b"
            else:
                c.targetElementId, c.targetPortId = "el-shaft", "flange_b"
            break
    else:
        pytest.fail("no engine-clutch connection in the example")
    root.elements.append(el("el-shaft", "mech.shaft", "Shaft", efficiency_pct=99))
    root.connections.append(conn(900, "el-engine", "shaft", "el-shaft", "flange_a"))
    case = next(c for c in p.cases if c.id == "case-mixed")
    case.duration = 300
    r = simulate(p, "case-mixed")
    assert r.status in ("success", "warning")

    def at(el_id, port, t):
        return next(x["value"] for x in series(r, el_id, port) if x["t"] == t)

    for t in (100.0, 200.0, 281.0):
        engine = at("el-engine", "sig_power", t)
        motor = at("el-motor", "sig_mech_power", t)
        shaft = at("el-shaft", "sig_power", t)
        fd = at("el-fd", "sig_power", t)
        assert shaft == pytest.approx(engine, abs=1e-4)  # its input: the engine's shaft
        # 99 %: driving it loses 1 % of its input; dragged, it asks 1/0.99
        loss = 0.01 * engine if engine >= 0 else -engine * (1 / 0.99 - 1)
        assert at("el-shaft", "sig_losses", t) == pytest.approx(loss, abs=1e-4)
        if abs(motor) > 0.5:  # the final drive takes both, less the clutch and gearbox
            assert fd != pytest.approx(shaft, abs=0.1)
        assert fd <= engine * 0.99 + max(motor, 0.0) + 1e-6


def _awd(split_a_pct: float):
    """One motor, a transfer case to a front and a rear final drive, each to
    an open differential and two wheels."""
    elements = [
        el("veh", "vehicle.body", "Vehicle"),
        el("drv", "driver.driver", "Driver"),
        el("task", "signal.driving_task", "Task", profile="0:0; 10:80; 40:80"),
        el("batt", "battery.generic", "Battery"),
        el("hvbus", "electric.node", "HV Bus"),
        el("mot", "motor.emotor", "E-Motor"),
        el("tc", "mech.transfer_case", "Transfer Case", torque_split_a_pct=split_a_pct,
           efficiency_pct=98),
        el("fdf", "mech.final_drive", "Final Drive Front", efficiency_pct=97),
        el("fdr", "mech.final_drive", "Final Drive Rear", efficiency_pct=97),
        el("dif", "mech.differential", "Diff Front"),
        el("dir", "mech.differential", "Diff Rear"),
    ]
    connections = [conn(1, "batt", "pos", "hvbus", "t1"), conn(2, "hvbus", "t3", "mot", "pos"),
                   conn(3, "mot", "shaft", "tc", "flange_in"),
                   conn(4, "tc", "flange_out_a", "fdf", "flange_in"),
                   conn(5, "tc", "flange_out_b", "fdr", "flange_in"),
                   conn(6, "fdf", "flange_out", "dif", "flange_in"),
                   conn(7, "fdr", "flange_out", "dir", "flange_in")]
    i = 10
    for d, axle in (("dif", "Front"), ("dir", "Rear")):
        for side in ("a", "b"):
            w = f"w{d}{side}"
            elements.append(el(w, "propulsion.wheel", w, axle=axle))
            connections.append(conn(i, d, f"flange_out_{side}", w, "shaft"))
            i += 1
    return project(elements, connections,
                   [*driver_wiring(), dbc(2, "drv", "sig_traction_cmd", "mot", "sig_demand_in")],
                   duration=40, time_step=0.5)


def test_awd_front_and_rear_final_drives_show_their_own_power():
    r = simulate(_awd(30.0), "case")
    assert r.status in ("success", "warning"), [m.text for m in r.messages]
    front = series(r, "fdf", "sig_power")
    rear = series(r, "fdr", "sig_power")
    mot = series(r, "mot", "sig_mech_power")
    k = next(i for i, x in enumerate(mot) if x["t"] == 5.0)
    assert front[k]["value"] != pytest.approx(rear[k]["value"], rel=0.05)
    # 30 % of the torque to the front, after the transfer case's 98 %
    assert front[k]["value"] == pytest.approx(0.3 * 0.98 * mot[k]["value"], rel=1e-6)
    assert rear[k]["value"] == pytest.approx(0.7 * 0.98 * mot[k]["value"], rel=1e-6)
    f = _flows(r)
    assert f["Final Drive Front"].energyIn < f["Final Drive Rear"].energyIn
    assert abs(_row(r, "Energy balance residual").value) < 0.5


def test_a_locked_differential_is_walked_every_step_and_still_closes():
    proj = bev_axle(locked=True)
    r = simulate(proj, "case")
    for f in r.partEnergy:
        assert f.energyIn - f.energyOut - f.losses - f.stored == pytest.approx(0.0, abs=2e-6)
    assert abs(_row(r, "Energy balance residual").value) < 0.5


def test_climate_control_is_its_own_line():
    proj = bev_axle(profile="0:0; 5:50; 60:50")
    root = proj.systems[0]
    root.elements += [el("clim", "electric.climate", "Climate"),
                      el("amb", "boundary.ambient", "Ambient", temperature_C=-10)]
    root.connections.append(conn(50, "hvbus", "t4", "clim", "pos"))
    r = simulate(proj, "case")
    clim = _flows(r)["Climate"]
    assert clim.part == "electric.climate"
    assert clim.energyIn == pytest.approx(4.7 * 30 / 3600, rel=0.01)  # 4.7 kW for 30 s
    assert clim.losses == pytest.approx(clim.energyIn)


def test_lap_case_books_the_mechanics_from_the_lap():
    r = example_result("fs-electric", "case-autocross")
    f = _flows(r)
    assert "Vehicle" in f and set(f["Vehicle"].terms) >= {"road load", "friction brakes"}
    assert not any(s.label == "Energy balance residual" for s in r.summary)
