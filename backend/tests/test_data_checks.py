"""Data Checks that catch models which would run 'successfully' but wrongly."""
import pytest
from fastapi.testclient import TestClient
from helpers import bev_axle, conn, dbc, el, project

from app.main import app
from app.schemas import Connection, Project
from app.storage import load_example
from app.validation import validate_project

client = TestClient(app)


def _errors(project):
    return [c.text for c in validate_project(project) if c.level == "error"]


# ---- UX-37: two signals wired into one input ----------------------------------------

def test_two_sources_on_one_input_is_an_error_naming_both_links():
    proj = load_example("bev-car")
    proj.dataBusConnections.append(dbc(99, "el-vehicle", "sig_speed", "el-driver", "sig_target_in"))
    errors = [e for e in _errors(proj) if "sources" in e]
    assert errors == [
        "'Driver.Target Speed' has 2 sources: 'Vehicle Task.Target Speed' and "
        "'Vehicle.Vehicle Speed' — "
        "an input takes one signal and the run would silently use only the last link. "
        "Remove all but one of them."
    ]


def test_the_run_is_refused_instead_of_driving_zero_km():
    proj = load_example("bev-car")
    proj.dataBusConnections.append(dbc(99, "el-vehicle", "sig_speed", "el-driver", "sig_target_in"))
    result = client.post("/api/simulate",
                         json={"project": proj.model_dump(), "caseId": "case-city"}).json()
    assert result["status"] == "failed"
    assert any("2 sources" in m["text"] for m in result["messages"])


def test_fan_in_is_found_across_canvas_wires_and_data_bus_links():
    proj = load_example("hybrid-car")
    # an older file kept a signal link as a canvas wire, input side first
    proj.systems[0].connections.append(Connection(
        id="c-sig", sourceElementId="el-hcu", sourcePortId="soc",
        targetElementId="el-vehicle", targetPortId="sig_speed"))
    errors = [e for e in _errors(proj) if "sources" in e]
    assert len(errors) == 1
    # listed in the order the solver applies them: the last one would win
    assert errors[0].startswith("'Hybrid Control Unit.soc' has 2 sources: 'Vehicle.Vehicle Speed' "
                                "and 'HV Battery.SOC'")


def test_repeated_link_and_fan_out_are_fine():
    proj = load_example("bev-car")
    # the same link twice feeds the input from one source only
    proj.dataBusConnections.append(dbc(98, "el-task", "sig_demand", "el-driver", "sig_target_in"))
    # one output feeding many inputs (brake command → four brakes) is normal
    assert not [e for e in _errors(proj) if "sources" in e]


def test_shipped_examples_have_no_fan_in():
    for name in ("bev-car", "hybrid-car"):
        assert not [e for e in _errors(load_example(name)) if "sources" in e]


# ---- VAL-01: models that cannot drive must not pass -------------------------------

def _without(name: str, *faults: str) -> Project:
    """The example with elements (and their wires/links), wires or links deleted."""
    d = load_example(name).model_dump()
    for s in d["systems"]:
        s["elements"] = [e for e in s["elements"] if e["id"] not in faults]
        s["connections"] = [c for c in s["connections"] if not set(faults) &
                            {c["id"], c["sourceElementId"], c["targetElementId"]}]
    d["dataBusConnections"] = [c for c in d["dataBusConnections"] if not set(faults) &
                               {c["id"], c["element1Id"], c["element2Id"]}]
    return Project.model_validate(d)


@pytest.mark.parametrize("name, fault, expected", [
    ("bev-car", "el-battery", "E-Motor 'E-Motor' has no power source"),
    ("bev-car", "c-1", "E-Motor 'E-Motor' has no power source"),
    ("bev-car", "el-motor", "The model has no E-Motor or Engine — nothing drives the wheels."),
    ("bev-car", "el-final-drive", "E-Motor 'E-Motor' is not mechanically connected"),
    ("bev-car", "el-diff", "E-Motor 'E-Motor' is not connected to any wheel"),
    ("bev-car", "el-wheel-fl", "'Differential': Flange Out A reaches no wheel"),
    ("bev-car", "db-2", "E-Motor 'E-Motor' has no Traction Command signal"),
    ("bev-car", "db-1", "Driver 'Driver' has no Target Speed signal"),
    ("bev-car", "el-driver", "No Driver follows the Driving Task 'Vehicle Task'"),
    ("bev-car", "el-vehicle", "Wheels present but no Vehicle element"),
    ("hybrid-car", "el-gearbox", "No E-Motor or Engine is connected to the wheels"),
    ("hybrid-car", "el-clutch", "Engine 'Engine' is not mechanically connected"),
    ("hybrid-car", "db-4", "Driver 'Driver' does not command any E-Motor or Engine"),
    ("hybrid-car", "db-6", "Engine 'Engine' has no Throttle signal — it will only idle."),
])
def test_a_model_that_cannot_drive_is_blocked(name, fault, expected):
    errors = _errors(_without(name, fault))
    assert any(e.startswith(expected) for e in errors), errors


@pytest.mark.parametrize("name, fault, expected", [
    ("bev-car", "el-brake-rl", "'Wheel RL' is not connected to anything"),
    ("bev-car", "c-3", "'Power Consumer' has nothing connected to its Positive Terminal (+)"),
    ("bev-car", "el-brake-fl", "'Wheel FL' has no brake (3 of 4 wheels have one)"),
    ("bev-car", "db-3", "Brake 'Brake FL' has no Brake Command signal"),
    ("hybrid-car", "db-3", "'Hybrid Control Unit' input 'speed' is not connected"),
    ("hybrid-car", "db-9", "Gearbox 'Gearbox' has no Gear Select signal — it stays in gear 1"),
    ("hybrid-car", "db-8", "Clutch 'Clutch' has no Engagement signal"),
    # the hybrid's battery also feeds its 12 V loads (CON-02), so without its
    # E-Motor it is the control script's speed input that reads 0
    ("hybrid-car", "el-motor", "'Hybrid Control Unit' input 'motor_rpm' is not connected"),
    ("hybrid-car", ("el-motor", "el-aux"), "'HV Battery' supplies nothing"),
])
def test_parts_that_silently_do_nothing_are_warned_about(name, fault, expected):
    checks = validate_project(_without(name, *([fault] if isinstance(fault, str) else fault)))
    assert any(c.level == "warning" and c.text.startswith(expected) for c in checks), \
        [c.text for c in checks]


def test_the_run_is_refused_instead_of_success_with_zero_km():
    # before: status 'success', 0.0 km driven, "All data checks passed"
    proj = _without("bev-car", "el-motor")
    result = client.post("/api/simulate",
                         json={"project": proj.model_dump(), "caseId": "case-city"}).json()
    assert result["status"] == "failed"
    assert any("nothing drives the wheels" in m["text"] for m in result["messages"])


def test_all_clear_says_what_was_and_was_not_checked():
    texts = [c.text for c in validate_project(load_example("bev-car"))]
    assert len(texts) == 1 and texts[0].startswith("All data checks passed: wiring, power supply")
    assert "ready to run" not in texts[0] and "cannot tell whether the results" in texts[0]


def test_a_generator_set_is_not_mistaken_for_a_broken_drive():
    # series hybrid: an engine turns a generator on a shaft with no wheels
    proj = bev_axle()
    proj.systems[0].elements += [
        el("eng", "engine.combustion", "Engine"),
        el("tank", "fuel.tank", "Tank"),
        el("gen", "motor.emotor", "Generator"),
        el("gcmd", "signal.constant", "Generator Cmd", value=-0.3),
        el("thr", "signal.constant", "Throttle", value=0.4),
    ]
    proj.systems[0].connections += [
        conn(40, "eng", "shaft", "gen", "shaft"),
        conn(41, "hvbus", "t2", "gen", "pos"),
    ]
    proj.dataBusConnections += [
        dbc(40, "gcmd", "sig_out", "gen", "sig_demand_in"),
        dbc(41, "thr", "sig_out", "eng", "sig_throttle_in"),
    ]
    assert _errors(proj) == []


def test_a_test_bench_without_a_vehicle_needs_no_wheels():
    # a motor on a dyno brake: no Vehicle, so nothing has to reach a wheel
    proj = project(
        [el("batt", "battery.generic", "Battery"), el("mot", "motor.emotor", "Motor"),
         el("dyno", "mech.brake", "Dyno"), el("cmd", "signal.constant", "Cmd", value=0.5),
         el("load", "signal.constant", "Load", value=0.2)],
        [conn(1, "batt", "pos", "mot", "pos"), conn(2, "mot", "shaft", "dyno", "flange")],
        [dbc(1, "cmd", "sig_out", "mot", "sig_demand_in"),
         dbc(2, "load", "sig_out", "dyno", "sig_demand_in")],
    )
    assert [(c.level, c.text) for c in validate_project(proj) if c.level != "info"] == []


def test_a_locked_differential_may_have_a_free_output():
    proj = bev_axle(locked=True)
    proj.systems[0].elements = [e for e in proj.systems[0].elements if e.id != "whr"]
    proj.systems[0].connections = [c for c in proj.systems[0].connections if c.id != "c6"]
    assert not [e for e in _errors(proj) if "reaches no wheel" in e]


# ---- VAL-01: implausible parameters (warnings quoting the numbers) -------------------

@pytest.mark.parametrize("element, key, value, expected", [
    ("el-vehicle", "mass_kg", 180_000, "has a mass of 180000 kg, outside the range"),
    ("el-battery", "capacity_kWh", 0.05, "has a capacity of 0.05 kWh, too small"),
    ("el-consumer", "power_kW", 250, "draws a constant 250 kW"),
    ("el-final-drive", "ratio", 97, "has a ratio of 97, far above"),
    ("el-battery", "initial_soc_pct", 4, "starts at 4 % SOC, at or below its minimum of 4 %"),
])
def test_implausible_parameters_are_warned_about(element, key, value, expected):
    proj = load_example("bev-car")
    target = next(e for e in proj.systems[0].elements if e.id == element)
    target.parameterOverrides[key] = value
    new = [c for c in validate_project(proj) if c.level != "info"]
    assert len(new) == 1 and new[0].level == "warning" and expected in new[0].text, new


@pytest.mark.parametrize("key, value, expected", [
    ("coulombic_efficiency_pct", 0, "Coulombic efficiency of 'HV Battery Pack' must be in (0, 100]"),
    ("coulombic_efficiency_pct", 101, "Coulombic efficiency of 'HV Battery Pack' must be in (0, 100]"),
    ("capacity_Ah", -1, "Charge capacity of 'HV Battery Pack' must be in (-0.001, 100000]"),
    ("capacity_Ah", 0, None),
])
def test_battery_charge_parameters_are_range_checked(key, value, expected):
    """MOD-38: 0 % would divide by zero and over 100 % would create energy;
    a Charge Capacity of 0 means 'from the Usable Capacity'."""
    proj = load_example("bev-car")
    next(e for e in proj.systems[0].elements if e.id == "el-battery").parameterOverrides[key] = value
    errors = _errors(proj)
    if expected is None:
        assert errors == []
    else:
        assert len(errors) == 1 and errors[0].startswith(expected), errors


def test_wheel_load_shares_must_add_up():
    proj = load_example("bev-car")
    for e in proj.systems[0].elements:
        if e.componentDefId == "propulsion.wheel":
            e.parameterOverrides["vehicle_load_share_pct"] = 5
    new = [c.text for c in validate_project(proj) if c.level != "info"]
    assert len(new) == 1 and new[0].startswith("Wheel load shares add up to 20 %, not 100 %")
