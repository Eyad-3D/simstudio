"""Data Checks that catch models which would run 'successfully' but wrongly."""
import copy
import re

import pytest
from fastapi.testclient import TestClient
from helpers import bev_axle, conn, dbc, el, project
from test_broken_models import EXAMPLES, faults

from app.library import load_library
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
    for name in ("bev-car", "fs-electric", "hybrid-car"):
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
    ("coulombic_efficiency_pct", 0,
     "Coulombic Efficiency of 'HV Battery Pack' must be above 0 and at most 100 % — got 0."),
    ("coulombic_efficiency_pct", 101,
     "Coulombic Efficiency of 'HV Battery Pack' must be above 0 and at most 100 % — got 101."),
    ("capacity_Ah", -1,
     "Charge Capacity of 'HV Battery Pack' must be at least 0 and at most 100000 Ah — got -1."),
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


@pytest.mark.parametrize("cells, flagged", [(150, True), (140, False)])
def test_voltage_class_data_check(cells, flagged):
    """MOD-39: a pack whose open-circuit voltage at 100 % SOC is above its
    Voltage Class is flagged before the run (150 × 4.2 V = 630 V against
    600 V); 140 cells (588 V) are not."""
    proj = load_example("bev-car")
    bat = next(e for e in proj.systems[0].elements if e.id == "el-battery")
    cell = {0: 3.0, 10: 3.45, 50: 3.7, 100: 4.2}
    bat.parameterOverrides.update(voltage_class_V=600,
                                  ocv_table={str(k): round(v * cells, 3) for k, v in cell.items()})
    hits = [c for c in validate_project(proj) if "Voltage Class" in c.text]
    if flagged:
        assert len(hits) == 1 and hits[0].level == "warning", hits
        assert "630 V" in hits[0].text and "600 V" in hits[0].text
    else:
        assert hits == []


@pytest.mark.parametrize("key, value, expected", [
    ("output_power_limit_kW", -5, "Output Power Limit of 'HV Battery Pack' must be at least 0 kW — got -5."),
    ("power_limit_margin_pct", 150,
     "Power Limit Margin of 'HV Battery Pack' must be at least 0 and at most 100 % — got 150."),
    ("power_limit_window_s", -1,
     "Power Check Window of 'HV Battery Pack' must be at least 0 and at most 60 s — got -1."),
    ("voltage_class_V", -600, "Voltage Class of 'HV Battery Pack' must be at least 0 V — got -600."),
    ("output_power_limit_kW", 80, None),
])
def test_power_limit_parameters_are_range_checked(key, value, expected):
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


# ---- MOD-40: vehicle geometry --------------------------------------------------------

@pytest.mark.parametrize("values, level, expected", [
    ({"cg_height_m": 0.3, "untag": True}, "error",
     "Vehicle 'Vehicle' has a Centre of Gravity Height of 0.3 m, but all its wheels are on the "
     "Front axle, so no load can shift between axles. Set Axle to Rear on the rear wheels."),
    ({"cg_height_m": 0.3, "all_rear": True}, "error",
     "Vehicle 'Vehicle' has a Centre of Gravity Height of 0.3 m, but all its wheels are on the "
     "Rear axle, so no load can shift between axles. Set Axle to Front on the front wheels."),
    ({"cg_height_m": 30, "wheelbase_m": 1.55}, "warning",
     "Vehicle 'Vehicle' has a Centre of Gravity Height of 30 m, above its Wheelbase of 1.55 m"),
    ({"wheelbase_m": 0}, "error", "Wheelbase of 'Vehicle' must be above 0 m — got 0."),
    ({"aero_balance_front_pct": 120}, "error",
     "Front Aero Balance of 'Vehicle' must be at least 0 and at most 100 % — got 120."),
    ({"cg_height_m": -0.1}, "error",
     "Centre of Gravity Height of 'Vehicle' must be at least 0 m — got -0.1."),
    ({"cg_height_m": 0.55, "downforce_cza_m2": -0.5}, None, None),  # tagged, lift allowed
])
def test_vehicle_geometry_data_checks(values, level, expected):
    """A CG height needs wheels on both axles (the examples' are tagged by
    their labels; untagged wheels count as Front); a height above the
    wheelbase is most likely the wrong unit."""
    proj = load_example("bev-car")
    values = dict(values)
    untag, all_rear = values.pop("untag", False), values.pop("all_rear", False)
    for e in proj.systems[0].elements:
        if e.id == "el-vehicle":
            e.parameterOverrides.update(values)
        elif untag and e.componentDefId == "propulsion.wheel":
            del e.parameterOverrides["axle"]
        elif all_rear and e.componentDefId == "propulsion.wheel":
            e.parameterOverrides["axle"] = "Rear"
    new = [c for c in validate_project(proj) if c.level != "info"]
    if expected is None:
        assert new == []
    else:
        assert len(new) == 1 and new[0].level == level and new[0].text.startswith(expected), new
        assert new[0].elementId == "el-vehicle"


# ---- MOD-11: road load counted once, and the Ambient's air -------------------------

@pytest.mark.parametrize("mode, included, warned", [
    ("Coefficients A/B/C", False, True),
    ("Coefficients A/B/C", True, False),
    ("Drag and rolling resistance", False, False),
])
def test_road_load_double_count_warning(mode, included, warned):
    """Coast-down coefficients hold the axle's drag; its lossy gears would
    count it again (the BEV's Final Drive is 98 %)."""
    proj = load_example("bev-car")
    next(e for e in proj.systems[0].elements if e.id == "el-vehicle").parameterOverrides.update(
        road_load_mode=mode, abc_include_driveline_losses=included)
    new = [c.text for c in validate_project(proj) if c.level != "info"]
    if warned:
        assert len(new) == 1 and "'Final Drive' 98 %" in new[0], new
        assert "Tick 'Coefficients Include Driveline Losses'" in new[0]
    else:
        assert new == []


@pytest.mark.parametrize("mode, warned", [("Coefficients A/B/C", True),
                                           ("Drag and rolling resistance", False)])
def test_negative_road_load_coefficients_warn(mode, warned):
    """A negative A or C drives the car instead of holding it back (a
    negative B is real: EPA lists some)."""
    proj = load_example("bev-car")
    next(e for e in proj.systems[0].elements if e.id == "el-vehicle").parameterOverrides.update(
        road_load_mode=mode, road_load_a_N=-50, road_load_b_N_per_kmh=-0.5,
        road_load_c_N_per_kmh2=-0.03)
    new = [c.text for c in validate_project(proj) if c.level != "info"]
    assert new == (["Vehicle 'Vehicle' has a road-load A of -50 N and C of -0.03 N/(km/h)² — a "
                    "negative A or C pushes the car along, so it speeds up when it coasts. "
                    "Check the sign."] if warned else [])


def _with_ambients(*values):
    proj = load_example("bev-car")
    proj.systems[0].elements += [
        el(f"amb{i}", "boundary.ambient", f"Ambient {i}", temperature_C=t, pressure_kPa=p)
        for i, (t, p) in enumerate(values)]
    return [(c.level, c.text) for c in validate_project(proj) if c.level != "info"]


@pytest.mark.parametrize("values, expected", [
    ([(20, 101.325)], []),
    ([(20, 101.325), (35, 85)],
     [("warning", "Only the first Ambient ('Ambient 0') sets the air density; "
                  "the others are ignored.")]),
    ([(20, 101.325), (20, 1.013)],  # an unused Ambient's air sets no density
     [("warning", "Only the first Ambient ('Ambient 0') sets the air density; "
                  "the others are ignored.")]),
    ([(20, 1.013)], [("warning", "'Ambient 0' has a pressure of 1.013 kPa (50 to 110 kPa is "
                                 "usual; 1 bar = 100 kPa), which gives the Vehicle's drag an air "
                                 "density of 0.012 kg/m³ — check the value and its unit.")]),
    ([(293.15, 101.325)], [("warning", "'Ambient 0' has a temperature of 293.15 °C (-60 to 60 °C "
                                       "is usual), which gives the Vehicle's drag an air density "
                                       "of 0.623 kg/m³ — check the value and its unit.")]),
    ([(20, 0)], [("error", "Pressure of 'Ambient 0' must be above 0 kPa — got 0.")]),
    ([(-300, 101.325)], [("error", "Temperature of 'Ambient 0' must be above -273.15 and at most "
                                   "1000 °C — got -300.")]),
])
def test_ambient_checks(values, expected):
    assert _with_ambients(*values) == expected


# ---- UX-10 / LRN-05: the catalogue's limits are the ones Data Checks use -----------

LIMITED = [(c.id, p) for c in load_library() for p in c.parameters
           if p.type == "number" and (p.minimum, p.exclusiveMinimum, p.maximum) != (None,) * 3]


def _range_errors(cid: str, key: str, value) -> list[str]:
    proj = project([el("x", cid, "X", **{key: value})], [], [])
    return [c.text for c in validate_project(proj) if c.level == "error" and " of 'X' " in c.text]


@pytest.mark.parametrize("cid, pdef", LIMITED, ids=[f"{c}.{p.key}" for c, p in LIMITED])
def test_every_catalog_limit_is_checked(cid, pdef):
    """A value just outside a parameter's limits in components.json is one
    error that says what is allowed; the edges and the default are fine."""
    outside, edges = [], [pdef.minimum, pdef.maximum]
    if pdef.exclusiveMinimum is not None:
        outside.append(pdef.exclusiveMinimum)
    if pdef.minimum is not None:
        outside.append(pdef.minimum - 0.0001)
    if pdef.maximum is not None:
        outside.append(pdef.maximum + 1)
    name = pdef.label.split(" (")[0]
    for v in outside:
        assert _range_errors(cid, pdef.key, v) == [
            f"{name} of 'X' {pdef.range_problem(v)} — got {v:g}."]
    for v in (pdef.default, *(e for e in edges if e is not None)):
        assert _range_errors(cid, pdef.key, v) == []
    assert _range_errors(cid, pdef.key, "abc") == [f"{name} of 'X' is not a number."]


def test_a_case_override_keeps_to_the_limits_too():
    """A case cannot run on an Initial SOC of 150 %: its own values are
    checked like the part's, and the fix line says where to change them."""
    proj = load_example("bev-car")
    case = proj.cases[0]
    case.parameterOverrides["el-battery"] = {"initial_soc_pct": 150, "capacity_kWh": 70}
    errors = [(c.text, c.elementIds, c.fix) for c in validate_project(proj) if c.level == "error"]
    assert errors == [(f"Initial SOC of 'HV Battery Pack' in case '{case.name}' must be above 0 "
                       "and at most 100 % — got 150.", ["el-battery"],
                       "Change or remove the override in Cases & Parameters.")]
    result = client.post("/api/simulate",
                         json={"project": proj.model_dump(), "caseId": case.id}).json()
    assert result["status"] == "failed" and "150" in result["messages"][0]["text"]


# ---- UX-09: every problem names its parts and says what to do ---------------------

# words of a check text that already say what to do (it then needs no fix line)
ADVICE = re.compile(r"\b(wire|connect|remove|give|set them|lower|extend|tick|check the|add a|"
                    r"drag|lock it|fewer)\b", re.I)


def test_every_corpus_problem_names_its_part_and_says_what_to_do():
    """Over the 118 broken models of VAL-01, a Problems row can always show
    its part(s) on the diagram and say how to fix it."""
    unnamed, unadvised = set(), set()
    for name in EXAMPLES:
        for _, d in faults(name):
            for c in validate_project(Project.model_validate(d)):
                if c.level == "info":
                    continue
                if not c.elementIds or c.elementId != c.elementIds[0]:
                    unnamed.add(c.text)
                if not c.fix and not ADVICE.search(c.text):
                    unadvised.add(c.text)
    assert unnamed == set()
    assert unadvised == set()


WHEELS = {"el-wheel-fl", "el-wheel-fr", "el-wheel-rl", "el-wheel-rr"}


def _bev(edit) -> list:
    d = load_example("bev-car").model_dump()
    edit(d, d["systems"][0])
    return validate_project(Project.model_validate(d))


def _twin(system: dict, el_id: str) -> None:
    """A copy of `el_id` ('<id>-2'), wired like it."""
    twin = copy.deepcopy(next(e for e in system["elements"] if e["id"] == el_id))
    twin["id"], twin["label"] = f"{el_id}-2", f"{twin['label']} 2"
    system["elements"].append(twin)
    system["connections"] += [{**c, "id": f"{c['id']}-2", "sourceElementId": twin["id"]}
                              for c in system["connections"] if c["sourceElementId"] == el_id]


def _shares(system: dict, pct: float) -> None:
    for e in system["elements"]:
        if e["id"] in WHEELS:
            e["parameterOverrides"]["vehicle_load_share_pct"] = pct


def _drop_vehicle(d: dict, system: dict) -> None:
    system["elements"] = [e for e in system["elements"] if e["id"] != "el-vehicle"]
    d["dataBusConnections"] = [c for c in d["dataBusConnections"]
                               if "el-vehicle" not in (c["element1Id"], c["element2Id"])]


@pytest.mark.parametrize("edit, starts, parts", [
    (lambda d, s: _twin(s, "el-vehicle"), "Only one Vehicle element",
     {"el-vehicle", "el-vehicle-2"}),
    (lambda d, s: _twin(s, "el-battery"), "Bus has two batteries",
     {"el-battery", "el-battery-2"}),
    (lambda d, s: _shares(s, 40), "Wheel load shares add up to 160 %", WHEELS),
    (lambda d, s: _shares(s, 0), "Wheel load shares add up to 0 %", WHEELS),
    (_drop_vehicle, "Wheels present but no Vehicle", WHEELS),
    (lambda d, s: d.update(cases=[]), "Project has no simulation case", set()),
], ids=["two Vehicles", "two batteries", "shares 160 %", "shares 0 %", "no Vehicle", "no case"])
def test_global_checks_name_every_part(edit, starts, parts):
    """Checks about the model as a whole select every part involved (a
    Problems row then frames them all); only project-level ones name none."""
    found = [c for c in _bev(edit) if c.text.startswith(starts)]
    assert len(found) == 1, [c.text for c in _bev(edit)]
    check = found[0]
    assert set(check.elementIds) == parts and len(check.elementIds) == len(parts)
    assert check.elementId == (check.elementIds[0] if parts else None)
    assert check.fix or ADVICE.search(check.text)


def test_a_wire_the_diagram_cannot_show_goes_with_its_part():
    """A wire to a part or port that is gone is not drawn, so it cannot be
    clicked: the fix line names the part to delete with it."""
    def edit(d, s):
        s["connections"] += [
            {"id": "c-gone", "sourceElementId": "el-battery", "sourcePortId": "pos",
             "targetElementId": "el-nowhere", "targetPortId": "t1"},
            {"id": "c-port", "sourceElementId": "el-battery", "sourcePortId": "no_such_port",
             "targetElementId": "el-hvbus", "targetPortId": "t1"},
        ]
    fixes = {c.text.split("'")[1]: (c.elementIds, c.fix) for c in _bev(edit) if "references missing" in c.text}
    delete = "delete 'HV Battery Pack' (the wire goes with it) and add it again."
    assert fixes == {"c-gone": (["el-battery"], "The diagram cannot show this wire: " + delete),
                     "c-port": (["el-battery"], "The diagram cannot show this wire: " + delete)}
