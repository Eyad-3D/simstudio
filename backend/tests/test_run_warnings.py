"""A run warns that its Vehicle "will not move" only when nothing can move it,
as Data Checks decide it (validation._moves_unpowered): a coast-down, with
an Initial Speed (the model's or the case's) or a downhill slope wired to
its Grade input, moves with nothing driving it. "No Driver element" is not
said of a model whose E-Motors and Engines all have their command wired, or
that has none. Before, a coast-down run carried both warnings."""
from __future__ import annotations

import pytest
from helpers import bev_axle, coast_project, dbc, el, project

from app.solver import simulate
from app.solver.network import NO_DRIVER, NO_WHEELS


def _warnings(result) -> list[str]:
    return [m.text for m in result.messages if m.level == "warning"]


def _body(v0: float = 0.0, grade: float | None = None, case_v0: float | None = None):
    """A Vehicle body alone (no wheels), its road load as coefficients."""
    elements = [el("veh", "vehicle.body", "Vehicle", road_load_mode="Coefficients A/B/C",
                   initial_speed_kmh=v0)]
    databus = []
    if grade is not None:
        elements.append(el("g", "signal.constant", "Grade", value=grade))
        databus.append(dbc(1, "g", "sig_out", "veh", "sig_grade_in"))
    p = project(elements, [], databus, duration=5.0, time_step=0.1)
    if case_v0 is not None:
        p.cases[0].parameterOverrides = {"veh": {"initial_speed_kmh": case_v0}}
    return p


@pytest.mark.parametrize("kwargs", [{"v0": 100.0}, {"grade": -3.0}, {"case_v0": 50.0}])
def test_a_coast_down_carries_no_will_not_move_warning(kwargs):
    r = simulate(_body(**kwargs), "case")
    assert NO_WHEELS not in _warnings(r)


@pytest.mark.parametrize("kwargs", [{}, {"grade": 3.0}, {"v0": 50.0, "case_v0": 0.0}])
def test_a_vehicle_nothing_can_move_is_still_warned_about(kwargs):
    r = simulate(_body(**kwargs), "case")
    assert NO_WHEELS in _warnings(r)


def _axle_without_driver(wired: bool):
    """bev_axle without its Driver; with ``wired`` a Constant commands the motor."""
    p = bev_axle()
    root = p.systems[0]
    root.elements = [e for e in root.elements if e.id != "drv"]
    p.dataBusConnections = [d for d in p.dataBusConnections
                            if "drv" not in (d.element1Id, d.element2Id)]
    if wired:
        root.elements.append(el("cmd", "signal.constant", "Command", value=0.1))
        p.dataBusConnections.append(dbc(50, "cmd", "sig_out", "mot", "sig_demand_in"))
    p.cases[0].duration = 1.0
    return p


def test_no_driver_warning_only_where_a_command_is_unwired():
    assert NO_DRIVER in _warnings(simulate(_axle_without_driver(wired=False), "case"))
    assert NO_DRIVER not in _warnings(simulate(_axle_without_driver(wired=True), "case"))
    # a coast-down on wheels: nothing to command
    assert NO_DRIVER not in _warnings(simulate(coast_project(duration=1.0), "case"))
