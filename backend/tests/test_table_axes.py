"""Tables of any number of axes and the Map block (docs/spec/tables.md):
the stored form at depth 1 to 4 through the project file, Data Checks, the
parameter sheet's long form, and a Map block of three inputs and two
outputs run on the new engine, in full and in quick mode, against the
map's own definition."""
from __future__ import annotations

import io

import openpyxl
import pytest

from app import validation
from app.dataio import params
from app.library import library_by_id
from app.schemas import ComponentDef, Connection, ElementInstance, ParameterDef, PortDef, Project
from app.solver import new_engine, simulate
from app.solver.maps import TableError, parse_grid
from app.storage import load_example

WLTC = "case-wltc"


def grid(depth: int, n: int = 2, start: float = 0.0):
    """A full grid of `depth` axes with `n` breakpoints each; a value is
    its breakpoints' sum plus `start`."""
    def level(d: int, acc: float):
        if d == depth:
            return acc + start
        return {str(10 * (d + 1) * i): level(d + 1, acc + 10 * (d + 1) * i) for i in range(n)}
    return level(0, 0.0)


@pytest.mark.parametrize("depth", [1, 2, 3, 4])
def test_a_full_grid_reads_row_major_with_the_first_axis_outermost(depth):
    axes, values = parse_grid(grid(depth), depth)
    assert axes == [[0.0, 10.0 * (d + 1)] for d in range(depth)]
    assert len(values) == 2 ** depth
    # the last point is every axis's last breakpoint; the second differs on the last axis
    assert values[-1] == sum(10.0 * (d + 1) for d in range(depth))
    assert values[1] == 10.0 * depth


def test_the_contract_examples_read_as_written():
    three = {"0": {"20": {"-100": 3.1, "100": 2.9}, "80": {"-100": 2.6, "100": 2.4}},
             "25": {"20": {"-100": 1.9, "100": 1.8}, "80": {"-100": 1.5, "100": 1.4}}}
    axes, values = parse_grid(three, 3)
    assert axes == [[0, 25], [20, 80], [-100, 100]]
    assert values == [3.1, 2.9, 2.6, 2.4, 1.9, 1.8, 1.5, 1.4] and values[(1 * 2 + 0) * 2 + 1] == 1.8


@pytest.mark.parametrize("bad, words", [
    ({"0": {"0": {"0": 1}}, "1": {"0": {"5": 1}}}, "not a full grid"),
    ({"0": {"0": {"0": 1, "0.0": 2}}}, "is there twice"),
    ({"0": {"x": {"0": 1}}}, "is not a number"),
    ({"0": {"0": 1}}, "nested one level per axis"),
    ({"0": {"0": {"0": "a"}}}, "value at"),
])
def test_a_table_of_three_axes_that_is_not_a_full_grid_is_refused(bad, words):
    with pytest.raises(TableError, match=words):
        parse_grid(bad, 3, ["A", "B", "C"])


def _map(inputs: int, outputs: int = 2, maps=None, label="Motor Map") -> ElementInstance:
    groups = ["Rotational Speed", "Torque", "Temperature", "Voltage"]
    ports = [PortDef(id=f"in{i}", name=f"In {i}", direction="input", kind="signal",
                     unitGroup=groups[i % 4]) for i in range(inputs)]
    ports += [PortDef(id=f"out{j}", name=f"Out {j}", direction="output", kind="signal",
                      unitGroup="Power") for j in range(outputs)]
    if maps is None:
        maps = {f"out{j}": grid(inputs, start=j) for j in range(outputs)}
    return ElementInstance(id="el-map", componentDefId="signal.map", label=label,
                           position={"x": 0, "y": 0}, dynamicPorts=ports,
                           parameterOverrides={"maps": maps})


def _with(el: ElementInstance, example: str = "bev-car") -> Project:
    project = load_example(example)
    project.systems[0].elements.append(el)
    return project


def _errors(project: Project) -> list[str]:
    return [c.text for c in validation.validate_project(project)
            if c.level == "error" and "Map" in c.text]


@pytest.mark.parametrize("depth", [1, 2, 3, 4])
def test_a_map_block_at_any_depth_survives_the_project_file(depth):
    project = _with(_map(depth))
    again = Project.model_validate_json(project.model_dump_json())
    el = next(e for e in again.systems[0].elements if e.id == "el-map")
    assert el.parameterOverrides["maps"] == {f"out{j}": grid(depth, start=j) for j in range(2)}
    assert [p.id for p in el.dynamicPorts] == [p.id for p in _map(depth).dynamicPorts]
    assert _errors(again) == []


def test_a_port_s_unit_text_survives_the_project_file():
    el = _map(1)
    el.dynamicPorts[0].unitGroup, el.dynamicPorts[0].unitText = "No Unit", "bar·s"
    again = Project.model_validate_json(_with(el).model_dump_json())
    port = next(e for e in again.systems[0].elements if e.id == "el-map").dynamicPorts[0]
    assert (port.unitGroup, port.unitText) == ("No Unit", "bar·s")


def test_data_checks_name_what_is_wrong_with_a_map_block():
    assert any("has no input" in e for e in _errors(_with(_map(0, maps={"out0": 1.0, "out1": 2.0}))))
    assert any("has no output" in e for e in _errors(_with(_map(2, 0, maps={}))))
    missing = _errors(_with(_map(2, maps={"out0": grid(2), "other": grid(2)})))
    assert any("no table for its output 'out1'" in e for e in missing)
    assert any("'other', which is not one of its outputs" in e for e in missing)
    shared = grid(3)
    other = grid(3)
    other["0"]["0"] = {"0": 1.0, "45": 2.0}
    for m in (other["10"], other["0"]):
        for v in m.values():
            v.pop("30", None)
            v["45"] = 2.0
    differ = _errors(_with(_map(3, maps={"out0": shared, "out1": other})))
    assert any("share one grid" in e and "In 2" in e for e in differ), differ
    ragged = grid(3)
    ragged["10"]["20"] = {"0": 1.0}
    assert any("not a full grid" in e for e in _errors(_with(_map(3, maps={"out0": ragged, "out1": grid(3)}))))


def test_a_library_table_of_three_axes_is_checked_as_a_full_grid(monkeypatch):
    part = ComponentDef(id="test.cells", category="Battery", name="Cells", icon="table",
                        domain="signal", ports=[], parameters=[ParameterDef(
                            key="r", label="Cell Resistance", unit="mΩ", type="table",
                            axes=[{"name": "Temperature", "unit": "°C"}, {"name": "SOC", "unit": "%"},
                                  {"name": "Current", "unit": "A"}], default=grid(3))])
    lib = {**library_by_id(), part.id: part}
    monkeypatch.setattr(validation, "library_by_id", lambda: lib)
    ragged = grid(3)
    ragged["0"]["20"] = {"0": 1.0}
    project = load_example("bev-car")
    project.systems[0].elements.append(ElementInstance(
        id="el-c", componentDefId="test.cells", label="Pack", position={"x": 0, "y": 0},
        parameterOverrides={"r": ragged}))
    found = [c.text for c in validation.validate_project(project) if "Cell Resistance" in c.text]
    assert found and "not a full grid" in found[0] and "Current" in found[0]
    # (the engine reads such a table by its number of axes: nothing refuses it for them)
    assert new_engine.unsupported(project, WLTC) is None


# A Map block on the BEV: Speed (km/h, from the Vehicle), SOC (%, from the
# battery) and Torque (N·m, from the E-Motor) in; Loss (kW) and Efficiency
# (%) out, into the vehicle's Monitor. The grid's speed axis ends at 100
# km/h, below the WLTC's top speed, so a clamped (or Error) Speed is read
# past its data.
SPEEDS, SOCS, TORQUES = [0.0, 40.0, 100.0], [0.0, 50.0, 100.0], [-250.0, 0.0, 120.0, 250.0]


def _loss(v, s, t):
    return 0.5 + 0.01 * v + 0.002 * s * abs(t) / 100 + 1e-4 * v * t


def _eff(v, s, t):
    return 80 + 0.05 * v - 0.01 * s + 0.02 * t - 1e-4 * v * s


def _grid(f):
    return {str(v): {str(s): {str(t): f(v, s, t) for t in TORQUES} for s in SOCS} for v in SPEEDS}


def _trilinear(f, at, outside):
    """The map's definition, computed here: straight lines along each axis
    between its breakpoints (the eight corners of the cell, weighted), past
    an axis's data held at its edge (clamp) or along its edge segment
    (linear)."""
    axes = (SPEEDS, SOCS, TORQUES)
    cell = []
    for x, xs, rule in zip(at, axes, outside):
        i = 0 if x <= xs[0] else min(len(xs) - 2, max(k for k in range(len(xs)) if xs[k] <= x))
        w = (x - xs[i]) / (xs[i + 1] - xs[i])
        if rule == "clamp":
            w = min(max(w, 0.0), 1.0)
        cell.append((i, w))
    total = 0.0
    for corner in range(8):
        weight, point = 1.0, []
        for k, (i, w) in enumerate(cell):
            up = corner >> k & 1
            weight *= w if up else 1.0 - w
            point.append(axes[k][i + up])
        total += weight * f(*point)
    return total


def _map_bev(outside: list[str], mode: str = "full") -> Project:
    project = load_example("bev-car")
    system = project.systems[0]
    ports = [PortDef(id="speed", name="Speed", direction="input", kind="signal", unitGroup="Velocity"),
             PortDef(id="soc", name="SOC", direction="input", kind="signal", unitGroup="Percent"),
             PortDef(id="torque", name="Torque", direction="input", kind="signal", unitGroup="Torque"),
             PortDef(id="loss", name="Loss", direction="output", kind="signal", unitGroup="Power"),
             PortDef(id="eff", name="Efficiency", direction="output", kind="signal", unitGroup="Percent")]
    system.elements.append(ElementInstance(
        id="el-map", componentDefId="signal.map", label="Motor Map", position={"x": 0, "y": 0},
        dynamicPorts=ports, parameterOverrides={"maps": {"loss": _grid(_loss), "eff": _grid(_eff)}},
        tableOutside={"maps": outside}))
    monitor = next(e for e in system.elements if e.id == "el-veh-monitor")
    monitor.dynamicPorts = list(monitor.dynamicPorts or []) + [
        PortDef(id="map_loss", name="Map loss", direction="input", kind="signal", unitGroup="Power"),
        PortDef(id="map_eff", name="Map efficiency", direction="input", kind="signal", unitGroup="Percent")]
    for i, (a, ap, b, bp) in enumerate([("el-vehicle", "sig_speed", "el-map", "speed"),
                                        ("el-battery", "sig_soc", "el-map", "soc"),
                                        ("el-motor", "sig_torque", "el-map", "torque"),
                                        ("el-map", "loss", "el-veh-monitor", "map_loss"),
                                        ("el-map", "eff", "el-veh-monitor", "map_eff")]):
        system.connections.append(Connection(id=f"c-map-{i}", sourceElementId=a, sourcePortId=ap,
                                             targetElementId=b, targetPortId=bp))
    next(c for c in project.cases if c.id == WLTC).mode = mode
    return project


@pytest.mark.parametrize("mode", ["full", "quick"])
def test_a_map_block_of_three_inputs_and_two_outputs_runs_as_its_map_says(mode):
    outside = ["clamp", "clamp", "linear"]
    result = simulate(_map_bev(outside, mode), WLTC)
    assert result.status == "success", [m.text for m in result.messages]
    ch = {c.portId: c for c in result.channels if c.elementId == "el-map"}
    assert {"speed", "soc", "torque", "loss", "eff"} <= set(ch)
    assert (ch["loss"].unit, ch["eff"].unit) == ("kW", "%")
    series = {k: [p["value"] for p in c.timeSeries] for k, c in ch.items()}
    times = [p["t"] for p in ch["speed"].timeSeries]
    checked = 0
    for n in range(0, len(times), 7):
        at = [series[k][n] for k in ("speed", "soc", "torque")]
        if any(x is None for x in at):
            continue
        for port, f in (("loss", _loss), ("eff", _eff)):
            want = _trilinear(f, at, outside)
            assert series[port][n] == pytest.approx(want, rel=1e-9, abs=1e-9), (port, times[n], at)
        checked += 1
    assert checked > 100
    # the time Speed was read past 100 km/h, held there (clamp): booked, and said in the Map's words
    above = sum(t1 - t0 for t0, t1, v in zip(times, times[1:], series["speed"][1:]) if v > SPEEDS[-1])
    assert above > 50.0
    said = [m.text for m in result.messages if m.text.startswith("'Motor Map' output 'Loss': read past "
                                                                    "its data on its input 'Speed' for ")]
    assert said and "held its edge value (clamp)" in said[0], [m.text for m in result.messages]
    booked = float(said[0].split(" for ")[1].split(" s ")[0])
    assert booked == pytest.approx(above, abs=3 * (times[1] - times[0]))


def test_a_map_blocks_outside_rules_are_checked_one_per_input():
    said = [c.text for c in validation.validate_project(_map_bev(["clamp", "error", "linear"]))
            if "Motor Map" in c.text]
    assert not said, said  # (three inputs, three rules: nothing to say, and the app runs it)
    (short,) = [c for c in validation.validate_project(_map_bev(["clamp", "linear"]))
                if "Motor Map" in c.text]
    assert short.level == "error"
    assert short.text == ("'Motor Map' has 2 outside-the-data settings for its 3 inputs (one per "
                          "input) — the run ignores them.")


@pytest.mark.parametrize("mode", ["full", "quick"])
def test_a_map_input_set_to_error_stops_the_run_naming_the_map_and_the_input(mode):
    result = simulate(_map_bev(["error", "clamp", "clamp"], mode), WLTC)
    assert result.status == "failed"
    text = " ".join(m.text for m in result.messages)
    assert "loss_map" in text or "eff_map" in text, text
    assert "first (Speed) axis" in text and "outside = Error" in text, text


def _sheet(project: Project, name_part: str) -> tuple[openpyxl.Workbook, str]:
    wb = openpyxl.load_workbook(io.BytesIO(params.export_xlsx(project)))
    return wb, next(n for n in wb.sheetnames if name_part in n)


@pytest.mark.parametrize("depth", [1, 3, 4])
def test_a_map_block_goes_through_the_parameter_sheet_as_a_long_list(depth):
    project = _with(_map(depth))
    wb, name = _sheet(project, "Motor Map")
    rows = [[c.value for c in r] for r in wb[name].iter_rows()]
    assert rows[1][:depth + 2] == [*(f"In {i} [{u}]" for i, u in zip(
        range(depth), ["1/min", "N·m", "°C", "V"])), "Out 0 [kW]", "Out 1 [kW]"]
    assert len(rows) == 2 + 2 ** depth
    assert rows[3][:depth] == [0] * (depth - 1) + [10 * depth]  # the last axis changes fastest
    res = params.import_sheet(project, params.export_xlsx(project), "p.xlsx")
    assert res.errors == [] and res.changes == []
    # one value changed, and a unit of the same kind in a header, come back as one change
    rows_ws = list(wb[name].iter_rows())
    rows_ws[2][depth].value = 99.0
    rows_ws[1][0].value = "In 0 [rad/s]" if depth else rows_ws[1][0].value
    for r in rows_ws[2:]:
        r[0].value = r[0].value * 3.141592653589793 / 30
    buf = io.BytesIO()
    wb.save(buf)
    res = params.import_sheet(project, buf.getvalue(), "p.xlsx")
    assert res.errors == []
    (change,) = res.changes
    first = change.new["out0"]
    for _ in range(depth):
        first = first[next(iter(first))]
    assert first == 99.0
    assert change.new["out1"] == project.systems[0].elements[-1].parameterOverrides["maps"]["out1"]


def test_a_long_list_that_is_not_a_full_grid_is_refused_with_its_row():
    project = _with(_map(3))
    wb, name = _sheet(project, "Motor Map")
    ws = wb[name]
    ws.delete_rows(5)
    buf = io.BytesIO()
    wb.save(buf)
    res = params.import_sheet(project, buf.getvalue(), "p.xlsx")
    assert res.changes == [] and "not a full grid" in res.errors[0]["text"]
    ws.cell(row=5, column=1, value=0)
    ws.cell(row=5, column=2, value=0)
    ws.cell(row=5, column=3, value=0)
    buf = io.BytesIO()
    wb.save(buf)
    res = params.import_sheet(project, buf.getvalue(), "p.xlsx")
    assert any("also in row 3" in e["text"] for e in res.errors), res.errors


def test_the_csv_export_keeps_a_map_as_json_at_any_depth():
    project = _with(_map(4))
    res = params.import_sheet(project, params.export_csv(project), "p.csv")
    assert res.errors == [] and res.changes == []
