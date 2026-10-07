"""Signal wires that would give wrong results without a message (VAL-17):
units that do not match, a percentage into an input that expects 0-1, and
loops of signal blocks. Two sources on one input (UX-37) are in
test_data_checks.py.

Each seeded mistake must be flagged; the shipped examples must stay clean."""
from __future__ import annotations

import pytest
from helpers import dbc, el

from app.schemas import PortDef
from app.solver import simulate
from app.solver.network import build_model, signal_loops
from app.storage import load_example
from app.validation import validate_project


def _warnings(project, words: str) -> list[str]:
    return [c.text for c in validate_project(project) if c.level == "warning" and words in c.text]


def _port(pid: str, direction: str, unit: str = "No Unit") -> PortDef:
    return PortDef(id=pid, name=pid, direction=direction, kind="signal", unitGroup=unit)


def _with_script(proj, unit: str, source=("el-battery", "sig_soc")):
    """The example with a Script whose one input, of ``unit``, reads ``source``."""
    script = el("el-x", "signal.script", "Strategy", code="def step(t, dt, inputs, state, params):\n    return {}\n")
    script.dynamicPorts = [_port("in_1", "input", unit)]
    proj.systems[0].elements.append(script)
    proj.dataBusConnections.append(dbc(90, *source, "el-x", "in_1"))
    return proj


# ---- units ------------------------------------------------------------------------

def test_soc_in_percent_into_a_script_input_set_to_fraction_is_flagged():
    found = _warnings(_with_script(load_example("bev-car"), "Fraction"), "0-1")
    assert found == [
        "'HV Battery Pack.SOC' gives a value in % (0-100) but 'Strategy.in_1' expects 0-1: the "
        "block gets a number 100 times too large, and the run gives no message."]


@pytest.mark.parametrize("block,port,key", [
    ("control.pid", "sig_setpoint_in", "signal_unit"),
    ("control.pid", "sig_feedback_in", "signal_unit"),
    ("signal.lookup", "sig_x_in", "x_unit"),
    ("signal.lookup", "sig_y_in", "y_unit"),
])
def test_soc_into_a_pid_or_lookup_set_to_fraction_is_flagged(block, port, key):
    proj = load_example("bev-car")
    proj.systems[0].elements.append(el("el-x", block, "Block", **{key: "Fraction"}))
    proj.dataBusConnections.append(dbc(90, "el-battery", "sig_soc", "el-x", port))
    assert len(_warnings(proj, "100 times too large")) == 1


def test_a_block_whose_unit_is_not_set_is_not_judged():
    proj = load_example("bev-car")
    proj.systems[0].elements.append(el("el-x", "control.pid", "Block"))
    proj.dataBusConnections.append(dbc(90, "el-battery", "sig_soc", "el-x", "sig_setpoint_in"))
    assert _warnings(proj, "expects") == []
    assert _warnings(_with_script(load_example("bev-car"), "No Unit"), "expects") == []
    assert _warnings(_with_script(load_example("bev-car"), "Percent"), "expects") == []


def test_soc_into_a_motor_demand_is_flagged_without_any_setting():
    """The motor's demand, the brakes', the throttle and the clutch are 0-1
    commands in the catalogue itself."""
    proj = load_example("bev-car")
    proj.dataBusConnections = [d for d in proj.dataBusConnections if d.port2Id != "sig_demand_in"
                               or d.element2Id != "el-motor"]
    proj.dataBusConnections.append(dbc(90, "el-battery", "sig_soc", "el-motor", "sig_demand_in"))
    assert len(_warnings(proj, "'E-Motor.Traction Command' expects 0-1")) == 1


def test_a_fraction_into_a_percent_input_is_flagged_the_other_way():
    proj = _with_script(load_example("bev-car"), "Percent", ("el-driver", "sig_traction_cmd"))
    assert _warnings(proj, "100 times too small") != []


def test_a_road_grade_in_percent_into_a_fraction_input_is_flagged():
    proj = load_example("bev-car")
    proj.systems[0].elements.append(el("el-road", "signal.road_profile", "Road"))
    proj.dataBusConnections.append(dbc(91, "el-vehicle", "sig_distance", "el-road", "sig_distance_in"))
    proj.dataBusConnections.append(dbc(92, "el-road", "sig_grade", "el-vehicle", "sig_grade_in"))
    assert _warnings(proj, "expects") == []  # % into % is right
    _with_script(proj, "Fraction", ("el-road", "sig_grade"))
    assert len(_warnings(proj, "100 times too large")) == 1


def test_a_speed_into_a_rotational_speed_input_is_flagged():
    proj = _with_script(load_example("bev-car"), "Rotational Speed", ("el-vehicle", "sig_speed"))
    assert _warnings(proj, "expects") == [
        "'Vehicle.Vehicle Speed' is a Velocity signal but 'Strategy.in_1' expects Rotational "
        "Speed: the number is passed on as it is, without converting it."]
    check = next(c for c in validate_project(proj) if "Rotational Speed" in c.text)
    assert check.elementIds == ["el-x", "el-vehicle"]  # both ends are shown
    assert check.fix


def test_the_examples_wiring_passes_the_unit_check():
    for name in ("bev-car", "fs-electric", "hybrid-car"):
        assert _warnings(load_example(name), "expects") == []


# ---- loops --------------------------------------------------------------------------

def _loop_project():
    """Scripts A and B feed each other; C reads B (downstream of the loop)."""
    proj = load_example("bev-car")
    code = ("def step(t, dt, inputs, state, params):\n"
            "    return {'out_1': inputs.get('in_1', 0) + 1}\n")
    for i, label in (("a", "A"), ("b", "B"), ("c", "C")):
        s = el(f"el-{i}", "signal.script", label, code=code)
        s.dynamicPorts = [_port("in_1", "input"), _port("out_1", "output")]
        proj.systems[0].elements.append(s)
    proj.dataBusConnections += [dbc(90, "el-a", "out_1", "el-b", "in_1"),
                                dbc(91, "el-b", "out_1", "el-a", "in_1"),
                                dbc(92, "el-b", "out_1", "el-c", "in_1")]
    return proj


def test_a_signal_loop_is_named_with_its_order_and_its_late_value():
    proj = _loop_project()
    checks = [c for c in validate_project(proj) if "form a loop" in c.text]
    assert [c.text for c in checks] == [
        "Signal blocks 'A', 'B' form a loop. Every solver step they run in this order, and "
        "LightSim inserts a one-step delay (at most 10 ms) where 'A' reads 'B': that value is "
        "the one from the step before."]
    assert checks[0].level == "warning"
    assert checks[0].elementIds == ["el-a", "el-b"]  # the loop only, not C
    assert checks[0].fix


def test_blocks_after_a_loop_run_after_it():
    """C reads B, so it runs after the loop and gets B's value of the same step."""
    assert build_model(_loop_project()).signal_blocks[-3:] == ["el-a", "el-b", "el-c"]


def test_the_delay_is_what_the_run_does():
    """A = B of the step before + 1, B = A + 1 and C = B + 1, so after n
    solver steps A = 2n - 1, B = 2n and C = 2n + 1."""
    proj = _loop_project()
    proj.cases[0].duration = 1.0
    proj.cases[0].timeStep = 0.5
    result = simulate(proj, proj.cases[0].id)
    last = {c.elementId: c.timeSeries[-1]["value"] for c in result.channels
            if c.portId == "out_1"}
    assert last["el-b"] == last["el-a"] + 1  # B = A of this step + 1
    assert last["el-c"] == last["el-b"] + 1  # C sees B of this step
    assert last["el-a"] == pytest.approx(2 * 100 - 1)  # 100 steps of 10 ms, two adds a lap


def test_a_block_that_feeds_itself_is_a_loop():
    assert signal_loops({"a", "b"}, {"a": {"a"}, "b": {"a"}}) == [["a"]]
    assert signal_loops({"a", "b", "c", "d"},
                        {"a": {"b"}, "b": {"a"}, "c": {"d"}, "d": {"c"}}) == [["a", "b"], ["c", "d"]]
