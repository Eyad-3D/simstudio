"""The new engine's path returns today's whole SimResult, made by today's own
code from the run's evidence (app/solver/evidence.py, new_engine.py): the
verdict, the part energies and the energy report from the engine's books,
the duty from its integrals over every step, what held each driveline back
from its trace and events, the references and hand calculations. Before
step 2's evidence part the path gave channels, figures and the build's
notes only: every test here failed on it.

They need the compiled module ``lightsim_engine``, as test_new_engine_flag.py
says (LIGHTSIM_REQUIRE_NEW_ENGINE=1 makes a missing module fail them)."""
from __future__ import annotations

import copy
import math
import os

import pytest
from helpers import dbc, el, sig_port

from app.schemas import ElementInstance, Project
from app.solver import new_engine, simulate
from app.storage import load_example

try:
    import lightsim_engine
    MISSING = None
except ImportError as e:
    lightsim_engine = None
    MISSING = f"the compiled module lightsim_engine is not installed ({e})"

REQUIRED = os.environ.get("LIGHTSIM_REQUIRE_NEW_ENGINE", "").strip() == "1"
pytestmark = pytest.mark.skipif(MISSING is not None and not REQUIRED, reason=MISSING or "")


@pytest.fixture
def new(monkeypatch):
    monkeypatch.setenv(new_engine.ENV_VAR, "new")
    monkeypatch.setenv(new_engine.STRICT_VAR, "1")


def _today(project, case_id, monkeypatch):
    with monkeypatch.context() as m:
        m.delenv(new_engine.ENV_VAR, raising=False)
        return simulate(project, case_id)


def _without(proj, el_id):
    proj = copy.deepcopy(proj)
    system = proj.systems[0]
    system.elements = [e for e in system.elements if e.id != el_id]
    system.connections = [c for c in system.connections
                          if el_id not in (c.sourceElementId, c.targetElementId)]
    proj.dataBusConnections = [c for c in proj.dataBusConnections
                               if el_id not in (c.element1Id, c.element2Id)]
    return proj


def _texts(result, level=None):
    return [m.text for m in result.messages if level is None or m.level == level]


def test_the_new_path_returns_the_whole_result(new, monkeypatch):
    """Every report today's engine makes, on the new engine: the same parts,
    rows, lanes and references, their numbers close (today's are its own
    fixed-step approximation)."""
    proj = load_example("bev-car")
    result = simulate(proj, "case-city")
    today = _today(proj, "case-city", monkeypatch)
    assert result.messages[0].text.startswith("Run on the new engine")
    assert result.status == today.status == "success"
    # part energies: every part today books (each driveline's spinning parts
    # one row of their own, as today's), each closing its books
    parts = sorted((p.elementId or p.label, p.part) for p in result.partEnergy)
    assert parts == sorted((p.elementId or p.label, p.part) for p in today.partEnergy)
    for p in result.partEnergy:
        assert p.energyIn - p.energyOut - p.losses - p.stored == pytest.approx(0.0, abs=1e-9)
    mine = {p.elementId: p for p in result.partEnergy}
    for p in today.partEnergy:
        if p.part in ("battery.generic", "motor.emotor", "electric.constant_drive", "vehicle.body"):
            assert mine[p.elementId].energyIn == pytest.approx(p.energyIn, rel=0.01, abs=1e-4)
            assert mine[p.elementId].losses == pytest.approx(p.losses, rel=0.01, abs=1e-4)
    assert mine["el-vehicle"].terms["air drag"] == pytest.approx(
        next(p for p in today.partEnergy if p.elementId == "el-vehicle").terms["air drag"], rel=0.01)
    # the energy report
    assert result.energy is not None and today.energy is not None
    assert result.energy.sourceKWh == pytest.approx(today.energy.sourceKWh, rel=1e-3)
    assert {f.label for f in result.energy.sinks} == {f.label for f in today.energy.sinks}
    # duty: the same parts and quantities, close
    assert [(d.elementId, [r.quantity for r in d.rows]) for d in result.duty] \
        == [(d.elementId, [r.quantity for r in d.rows]) for d in today.duty]
    for d_new, d_old in zip(result.duty, today.duty):
        for a, b in zip(d_new.rows, d_old.rows):
            assert a.rms == pytest.approx(b.rms, rel=0.01), (d_new.elementId, a.quantity)
            assert a.max == pytest.approx(b.max, rel=0.01, abs=1e-3), (d_new.elementId, a.quantity)
    # what held the driveline back: the same lane, the seconds close
    assert result.limits is not None
    lane, old = result.limits.lanes[0], today.limits.lanes[0]
    assert (lane.label, lane.elementIds, set(lane.seconds)) == (old.label, old.elementIds, set(old.seconds))
    for state, secs in old.seconds.items():
        assert lane.seconds[state] == pytest.approx(secs, abs=0.5)
    # the references and the hand calculations, graded as today's
    assert [(r.label, r.grade) for r in result.references] \
        == [(r.label, r.grade) for r in today.references]


@pytest.mark.parametrize("name, case_id", [("fs-electric", "case-accel-75m"),
                                           ("hybrid-car", "case-udds")])
def test_mapped_energy_rows_sum_to_the_books(name, case_id):
    """The part energies' losses and stored energies sum to the engine's own
    totals of the run, to round-off (the ENERGY line's E1): the heat its
    parts' books lost, with what a rigid engagement took at the part its
    books name (a gear shift), and their stored energies' change; the hybrid
    shifts gears."""
    proj = load_example(name)
    case = next(c for c in proj.cases if c.id == case_id)
    if name == "hybrid-car":  # a single run of its first 200 s, which shift gears
        case.chargeBalance, case.duration = False, 200.0
    os.environ[new_engine.ENV_VAR] = "new"
    try:
        result = simulate(proj, case_id)
    finally:
        del os.environ[new_engine.ENV_VAR]
    assert result.messages[0].text.startswith("Run on the new engine")
    built = lightsim_engine.build(proj.model_dump_json(), case_id)
    if name == "hybrid-car":  # its Script block in today's worker, as the path runs it
        from app.solver.sandbox import ScriptSandbox, ScriptSpec
        el = next(e for e in proj.systems[0].elements if e.componentDefId == "signal.script")
        spec = ScriptSpec(el_id=el.id, label=el.label, code=el.parameterOverrides["code"],
                          input_keys=[p.id for p in el.dynamicPorts if p.direction == "input"],
                          params=dict(el.parameterOverrides))
        with ScriptSandbox([spec]) as box:
            run = built.simulate(native_scripts=False, scripts={el.id: lambda t, dt, ins: box.run(
                el.id, el.label, t, dt, dict(ins), spec.params)})
    else:
        run = built.simulate()
    books = run.energy
    if name == "hybrid-car":
        assert sum(p["impulse_lost_J"] for p in books["parts"]) > 0  # it shifted
    kwh = 1.0 / 3.6e6
    through = books["throughput_J"] * kwh
    lost = books["lost_J"] + sum(p["impulse_lost_J"] for p in books["parts"])
    assert sum(p.losses for p in result.partEnergy) == pytest.approx(lost * kwh, abs=1e-12 * through)
    assert sum(p.stored for p in result.partEnergy) == pytest.approx(
        books["stored_change_J"] * kwh, abs=1e-12 * through)


def _sine_project(p0=5.0, amp=3.0, period=1.0, n=40, periods=4) -> Project:
    """An ideal battery feeding a Power Consumer whose demand a Script block
    holds at P0 + A sin(2 pi t / T) between its N ticks a period: over whole
    periods its RMS is sqrt(P0^2 + A^2 / 2) exactly (the sum of sin^2 over N
    equally spaced phases is N / 2)."""
    code = ("import math\n\n"
            "def step(t, dt, inputs, state, params):\n"
            f"    return {{'p': {p0!r} + {amp!r} * math.sin(2 * math.pi * t / {period!r})}}\n")
    return Project.model_validate({
        "id": "sine", "name": "Sine", "systems": [{
            "id": "root", "name": "Sine", "parentId": None,
            "elements": [
                {"id": "b", "componentDefId": "battery.generic", "label": "Battery",
                 "position": {"x": 0, "y": 0},
                 "parameterOverrides": {"capacity_kWh": 10.0, "internal_resistance_ohm": 0.0,
                                        "rc_resistance_ohm": 0.0, "initial_soc_pct": 80.0}},
                {"id": "load", "componentDefId": "electric.constant_drive", "label": "Load",
                 "position": {"x": 0, "y": 0}, "parameterOverrides": {"power_kW": 0.0}},
                {"id": "sine", "componentDefId": "signal.script", "label": "Sine",
                 "position": {"x": 0, "y": 0},
                 "parameterOverrides": {"code": code, "sample_time_s": period / n},
                 "dynamicPorts": [{"id": "p", "name": "p", "direction": "output",
                                   "kind": "signal", "unitGroup": "No Unit"}]},
                {"id": "g", "componentDefId": "boundary.ground", "label": "Ground",
                 "position": {"x": 0, "y": 0}}],
            "connections": [
                {"id": "c1", "sourceElementId": "b", "sourcePortId": "pos",
                 "targetElementId": "load", "targetPortId": "pos"},
                {"id": "c2", "sourceElementId": "b", "sourcePortId": "neg",
                 "targetElementId": "load", "targetPortId": "neg"},
                {"id": "c3", "sourceElementId": "b", "sourcePortId": "neg",
                 "targetElementId": "g", "targetPortId": "t1"}]}],
        "dataBusConnections": [{"id": "d1", "element1Id": "sine", "port1Id": "p",
                                "element2Id": "load", "port2Id": "sig_demand_in"}],
        "cases": [{"id": "case", "name": "Sine", "duration": period * periods,
                   "timeStep": period / n}],
    })


def test_the_duty_rms_of_a_held_sine_is_exact(new, monkeypatch):
    """The battery's power duty RMS from the engine's integrals over every
    step (not from every fourth step): a held sine's closed form to 1e-9."""
    monkeypatch.setenv("LIGHTSIM_SCRIPT_TRUST", "off")
    result = simulate(_sine_project(), "case")
    assert result.status == "success", _texts(result)
    row = next(r for d in result.duty if d.elementId == "b" for r in d.rows if r.quantity == "Power")
    exact = math.sqrt(5.0 ** 2 + 3.0 ** 2 / 2)
    assert row.rms == pytest.approx(exact, rel=1e-9)
    assert row.mean == pytest.approx(5.0, rel=1e-9)


def test_the_start_guess_notes_are_notes(new):
    """The engine's notes about its start values (two names of one variable
    with different start guesses, a start preparation could not solve) are
    notes: before, each was the run's warning, and every run said
    "warning" (review finding 36)."""
    result = simulate(load_example("bev-car"), "case-city")
    notes = [m for m in result.messages if "START-ALIAS-CONFLICT" in m.text]
    assert notes and all(m.level == "info" for m in notes)
    assert result.status == "success", _texts(result, "warning")


def test_a_fault_the_notes_hid_is_judged(new):
    """The BEV with its E-Motor deleted drives nowhere: today's engine fails
    it, and so does the new path's verdict now (before: "warning", from the
    start-guess note alone)."""
    result = simulate(_without(load_example("bev-car"), "el-motor"), "case-city")
    assert result.status == "failed"
    assert any(t.startswith("The vehicle did not drive the cycle") for t in _texts(result, "error"))


def test_the_new_engine_run_warnings_reach_the_user(new):
    """The engine's own warnings on its run are the run's warnings (before:
    only the build's were shown): the Formula Student car without its
    E-Motor, whose energy books drift."""
    result = simulate(_without(load_example("fs-electric"), "el-motor"), "case-accel-75m")
    run_warnings = [t for t in _texts(result, "warning") if t.startswith("The new engine's run:")]
    assert run_warnings, _texts(result)
    assert result.status == "warning"


def test_non_finite_values_are_judged_before_they_are_gaps(new, monkeypatch):
    """A channel that is not a number fails the run (the verdict reads the
    engine's values; the result shows them as gaps): before, NaN became
    None first and the run passed."""
    monkeypatch.setenv("LIGHTSIM_SCRIPT_TRUST", "off")
    proj = load_example("bev-car")
    code = ("def step(t, dt, inputs, state, params):\n"
            "    return {'y': float('nan') if t > 5 else 0.0}\n")
    proj.systems[0].elements.append(ElementInstance(
        id="nan", componentDefId="signal.script", label="NaN maker",
        position={"x": 0, "y": 0}, parameterOverrides={"code": code, "sample_time_s": 0.5},
        dynamicPorts=[sig_port("y", "output")]))
    proj.systems[0].elements.append(el("mon", "signal.monitor", "Monitor"))
    proj.dataBusConnections.append(dbc(50, "nan", "y", "mon", "sig_in"))
    result = simulate(proj, "case-city")
    assert result.status == "failed", _texts(result)
    assert any("Non-finite values" in t and "nan:y" in t for t in _texts(result, "error"))
    y = next(c for c in result.channels if c.elementId == "nan" and c.portId == "y")
    assert y.timeSeries[-1]["value"] is None  # shown as a gap
    assert all(s.notValid == "the solution broke down" for s in result.summary
               if s.label != "Simulated duration")


def _verdict(result):
    return (result.status, sorted((s.key, s.notValid) for s in result.summary if s.notValid),
            sorted((s.key, s.passed) for s in result.summary if s.passed is not None))


def test_a_table_read_past_its_data_is_judged(new, monkeypatch):
    """The engine's records of a table read past its data (its first time,
    for how long, how far) become today's summary rows, note and verdict,
    and today's engine is the judge: the BEV's winter WLTC at -30 °C, below
    its climate demand table's -20 °C, both engines read for the whole run.
    The axis is shown in °C (before: 273.15 off, -303.15 °C, as if the
    engine read it in kelvin), and the record's words are its note, not
    also a warning of the engine's run."""
    proj = load_example("bev-car")
    case = next(c for c in proj.cases if c.id == "case-wltc-winter")
    case.parameterOverrides["el-ambient"]["temperature_C"] = -30.0
    today = _today(proj, "case-wltc-winter", monkeypatch)
    result = simulate(proj, "case-wltc-winter")
    assert _verdict(result) == _verdict(today)
    assert result.status == "warning"
    stem = "el-climate.outside_demand_table_outside_temperature"
    rows, old = {s.key: s for s in result.summary}, {s.key: s for s in today.summary}
    assert (rows[f"{stem}_furthest"].value, rows[f"{stem}_furthest"].unit) == (-30.0, "°C")
    assert rows[f"{stem}_furthest"].value == old[f"{stem}_furthest"].value
    assert rows[f"{stem}_time_pct"].value == pytest.approx(old[f"{stem}_time_pct"].value)
    note = [t for t in _texts(result, "info") if "Heating/Cooling Demand: Outside Temperature" in t]
    assert len(note) == 1 and "-30 °C is past the edge of its data (-20 °C)" in note[0], note
    assert not [t for t in _texts(result) if "demand_table" in t], "the record told twice"


@pytest.mark.xfail(strict=True, reason="the engine's table guards count a read in a branch the run "
                                       "does not take (the E-Motor's drag table while it is powered)")
def test_a_table_read_only_in_a_branch_not_taken_is_not_judged(new, monkeypatch):
    """The BEV's E-Motor drag table cut at 6000 1/min on the WLTC: today's
    engine reads it only while the motor is unpowered, below 6000 1/min, and
    passes the run; the engine's guard counts the read while it is powered
    too (548 s past it) and the verdict warns. Today's verdict is the
    judge."""
    proj = load_example("bev-car")
    motor = next(e for e in proj.systems[0].elements if e.id == "el-motor")
    motor.parameterOverrides["drag_torque"] = {"0": 0.0, "3000": 0.7, "6000": 0.8}
    assert _verdict(simulate(proj, "case-wltc")) == _verdict(_today(proj, "case-wltc", monkeypatch))


def test_a_cycle_run_past_its_end_is_not_judged_on_its_schedule(new, monkeypatch):
    """A WLTC run 100 s past its 1800 s: its Driving Task's profile is read
    past its data, a schedule today's engine holds at its end and does not
    judge as a part's data; the new path's verdict is today's (before: a
    warning and 12 rows not valid, with an empty unit), and a note with its
    code says the record is not judged."""
    proj = load_example("bev-car")
    next(c for c in proj.cases if c.id == "case-wltc").duration = 1900.0
    result = simulate(proj, "case-wltc")
    assert _verdict(result) == _verdict(_today(proj, "case-wltc", monkeypatch))
    assert result.status == "success", _texts(result, "warning")
    notes = [t for t in _texts(result, "info") if f"[{new_engine.NOT_JUDGED}]" in t]
    assert len(notes) == 1 and notes[0].startswith("'Vehicle Task' profile:"), notes
    assert not any(k.startswith("el-task.outside") for k in (s.key for s in result.summary))


@pytest.mark.parametrize("name, case_id, profile", [
    ("bev-car", "case-city", None),
    ("bev-car", "case-city", "0:0; 3:200; 30:200; 60:200"),  # cannot keep up
])
def test_the_new_verdict_does_not_depend_on_the_output_step(new, name, case_id, profile):
    """The verdict reads the run's trace from the dense output, every 0.1 s
    whatever the case's step: the same status and notValid rows at an
    output step of 0.1, 1 and 5 s (the META line's relation)."""
    verdicts = {}
    for step in (0.1, 1.0, 5.0):
        proj = load_example(name)
        case = next(c for c in proj.cases if c.id == case_id)
        case.timeStep = step
        if profile:
            task = next(e for e in proj.systems[0].elements if e.componentDefId == "signal.driving_task")
            task.parameterOverrides["profile"] = profile
            case.duration = 60.0
        result = simulate(proj, case_id)
        verdicts[step] = (result.status, sorted((s.key, s.notValid) for s in result.summary if s.notValid),
                          sorted(t for t in _texts(result, "warning") + _texts(result, "error")
                                 if t.startswith(("Cycle not followed", "The vehicle"))))
    assert verdicts[0.1][0] == verdicts[1.0][0] == verdicts[5.0][0]
    assert verdicts[0.1][1] == verdicts[1.0][1] == verdicts[5.0][1]
    if profile:
        assert verdicts[1.0][0] == "warning" and verdicts[1.0][1]


def test_an_acceleration_test_reaches_its_line_at_any_tolerance(new, monkeypatch):
    """The engine ends an acceleration test at its finish event, located to
    its root finding's precision: at rtol 1e-8 a hair short of the line,
    which the verdict read as not reached (no time, a warning) before."""
    real = lightsim_engine.build

    class Tight:
        def __init__(self, m):
            self.m = m

        def __getattr__(self, name):
            return getattr(self.m, name)

        def simulate(self, **kw):
            return self.m.simulate(**{**kw, "rtol": 1e-8})
    monkeypatch.setattr(lightsim_engine, "build", lambda *a, **k: Tight(real(*a, **k)))
    result = simulate(load_example("fs-electric"), "case-accel-75m")
    rows = {s.key: s for s in result.summary}
    assert result.status == "success", _texts(result, "warning")
    assert rows["accel_time_s"].passed is True
    assert rows["accel_time_s"].value == pytest.approx(3.7537, abs=1e-3)


def test_a_battery_check_with_no_window_is_a_plain_result(new):
    """A battery with an Output Power Limit and no averaging window: its
    check's passed flag was NumPy's bool (the peak read from the run's
    arrays), and the result could not be written as JSON (the app's save,
    the harness's record): a crash of the path where none should be."""
    proj = load_example("bev-car")
    bat = next(e for e in proj.systems[0].elements if e.id == "el-battery")
    bat.parameterOverrides.update(output_power_limit_kW=15.0, power_limit_window_s=0.0)
    result = simulate(proj, "case-city")
    row = next(s for s in result.summary if s.key == "el-battery.peak_terminal_power_averaged_kw")
    assert type(row.passed) is bool and type(row.value) is float
    import json
    assert json.loads(result.model_dump_json())["summary"]
    # held at its cap (enforced): the run says so, as today's does
    assert any(t.startswith("Battery 'HV Battery Pack' held at its Output Power Limit")
               for t in _texts(result, "info"))


def test_a_limit_change_is_at_the_run_s_own_event(new):
    """What held the driveline back changes at the engine's own events: the
    BEV with a tiny battery is held by its supply from the moment its
    battery's window closes, an event of the run."""
    proj = load_example("bev-car")
    bat = next(e for e in proj.systems[0].elements if e.id == "el-battery")
    bat.parameterOverrides["capacity_kWh"] = float(bat.parameterOverrides.get("capacity_kWh", 75)) / 100
    result = simulate(proj, "case-city")
    lane = result.limits.lanes[0]
    supply = [t for t, s in lane.changes if result.limits.states[int(s)] == "supply"]
    assert supply
    run = lightsim_engine.build(proj.model_dump_json(), "case-city").simulate()
    events = [e["t"] for e in run.events]
    assert any(abs(supply[0] - round(t, 4)) < 1e-9 for t in events), (supply, events[:20])
    assert lane.seconds["supply"] + lane.seconds["demand"] == pytest.approx(600.0, abs=1e-6)


def test_lab_rows_share_their_base_rows_validity_by_key(monkeypatch):
    """lab_base, which the new path uses on the engine's figures, gives the
    base row today's lab_rows gives each lab row (today's engine)."""
    import app.solver.core as core
    from app.solver import labfig
    from app.solver.labfig import lab_base
    real, got = labfig.lab_rows, {}

    def spy(*a, **k):
        rows, base = real(*a, **k)
        got.update(rows=rows, base=base)
        return rows, base
    monkeypatch.setattr(core, "lab_rows", spy)
    for name, case_id in (("bev-car", "case-wltc"), ("hybrid-car", "case-udds")):
        proj = load_example(name)
        if name == "hybrid-car":
            next(c for c in proj.cases if c.id == case_id).chargeBalance = False
        got.clear()
        _today(proj, case_id, monkeypatch)
        assert got["base"], name
        assert lab_base(got["rows"], electric=name == "bev-car") == got["base"], name


def _battery(proj, **over):
    next(e for e in proj.systems[0].elements if e.id == "el-battery").parameterOverrides.update(over)
    return proj


@pytest.mark.parametrize("limit_kw, window_s", [(23.0, 0.5), (23.6, 0.0)])
def test_a_checked_battery_limit_is_judged_on_every_step(new, monkeypatch, limit_kw, window_s):
    """A battery's Output Power Limit, only checked: its averaged peak is the
    engine's highest moving average of the battery's power over every point
    it visited (today's: over every step), so the verdict is today's at an
    output step of 0.1, 1 and 5 s (the META line's relation); with no
    window it is the peak itself, and the row shows the number its passed
    flag is judged on. Before, from the output grid's means: a success at
    5 s with a 0.5 s window, and at the case's own 1 s with none, its row
    23.602 kW against 23.6 kW marked passed."""
    proj = _battery(load_example("bev-car"), output_power_limit_kW=limit_kw,
                    power_limit_window_s=window_s, power_limit_enforced=False)
    key = "el-battery.peak_terminal_power_averaged_kw"
    today = next(s for s in _today(proj, "case-city", monkeypatch).summary if s.key == key)
    assert today.passed is False
    for step in (0.1, 1.0, 5.0):
        p = copy.deepcopy(proj)
        next(c for c in p.cases if c.id == "case-city").timeStep = step
        result = simulate(p, "case-city")
        row = next(s for s in result.summary if s.key == key)
        assert result.status == "warning" and row.passed is False, (step, row.value)
        assert row.value > limit_kw and row.value == pytest.approx(today.value, rel=2e-3), step
        assert any(t.startswith("Battery 'HV Battery Pack' broke its Output Power Limit")
                   for t in _texts(result, "warning")), step


def test_an_enforced_cap_held_to_the_solver_s_tolerance_is_not_broken(new):
    """An enforced Output Power Limit holds the battery at it: the engine
    holds it to its tolerance (80.0000020 kW at 80 kW, rtol 1e-6), which is
    at the cap, not over it (before: "broke its Output Power Limit", where
    today's engine passes): the Formula Student car with no internal
    resistance."""
    proj = _battery(load_example("fs-electric"), internal_resistance_ohm=0.0)
    result = simulate(proj, "case-accel-75m")
    row = next(s for s in result.summary if s.key == "el-battery.peak_terminal_power_averaged_kw")
    assert row.passed is True and row.value == pytest.approx(80.0, rel=1e-12)
    assert result.status == "success", _texts(result, "warning")


def _dragged_past_its_maximum_speed() -> Project:
    """The BEV of 20 t started at 140 km/h, its Driver asked by a Constant
    for 0 km/h, its E-Motor's Maximum Speed 8000 1/min: the car drags the
    motor past it for its first 17 s."""
    proj = load_example("bev-car")
    proj.systems[0].elements.append(el("el-src", "signal.constant", "Target", value=0.0))
    for c in proj.dataBusConnections:
        if c.element1Id == "el-task" and c.port1Id == "sig_demand":
            c.element1Id, c.port1Id = "el-src", "sig_out"
    for e in proj.systems[0].elements:
        if e.id == "el-vehicle":
            e.parameterOverrides.update(initial_speed_kmh=140.0, mass_kg=20000.0)
        elif e.id == "el-motor":
            e.parameterOverrides["max_speed_rpm"] = 8000.0
    next(c for c in proj.cases if c.id == "case-city").duration = 200.0
    return proj


def test_a_motor_driven_past_its_maximum_speed_is_judged(new, monkeypatch):
    """A motor the car drives above its Maximum Speed: its time above it and
    its highest speed, the summary's rows and the verdict's warning, as
    today's engine gives them (before: never judged, the rows missing and
    the run a success)."""
    proj = _dragged_past_its_maximum_speed()
    today = _today(proj, "case-city", monkeypatch)
    result = simulate(proj, "case-city")
    rows, old = {s.key: s for s in result.summary}, {s.key: s for s in today.summary}
    assert rows["el-motor.highest_speed_rpm"].value == pytest.approx(
        old["el-motor.highest_speed_rpm"].value, rel=1e-4)
    assert rows["el-motor.time_above_max_speed_pct"].value == pytest.approx(
        old["el-motor.time_above_max_speed_pct"].value, abs=0.1)
    said = [t for t in _texts(result, "warning") if "past its maximum speed" in t]
    assert len(said) == 1 and said[0].startswith(
        "E-Motor 'E-Motor' ran 5,628 1/min past its maximum speed for 17 s of 200 s"), said
    assert result.status == today.status == "warning"
    assert any(t.startswith("E-Motor 'E-Motor' was driven above its maximum speed (8,000 1/min)")
               for t in _texts(result, "info"))


def test_a_speed_at_its_maximum_to_the_solver_s_tolerance_is_not_above_it():
    """A machine its limiter holds at its maximum speed, to the solver's
    tolerance (1e-7 of it above), is not above it, as today's counts no
    step its own limiter holds it there; 1e-5 above it is (a trace of the
    run, no engine: no example case reaches the limit that way)."""
    from types import SimpleNamespace as NS
    for over, above in ((1e-7, 0.0), (1e-5, 3.0)):
        use = NS(edge=8000.0, outside_s=0.0, value=0.0, t=0.0)
        ctx = NS(motors={"m": NS(speed_use=use)}, engines={}, model=NS(elements={"m": NS(label="M")}))
        speed = [8000.0 * (1 + over)] * 4
        run = NS(trace={"times": [0.0, 1.0, 2.0, 3.0], "m:sig_speed": speed},
                 integrals={"m:sig_speed": {"max": max(speed), "min": 0.0}})
        said: list = []
        new_engine._over_speed(ctx, NS(speeds={"m": "m:sig_speed"}), run, said)
        assert use.outside_s == pytest.approx(above, rel=1e-6) and len(said) == (above > 0), over


def _fuel_cell_on_the_motor_bus(max_current_a: float) -> Project:
    """The tests' fuel cell car with its fuel cell straight on the motor's
    bus (no DC-DC, no battery), short of the car's peak demand."""
    from helpers import conn, fuel_cell_car
    proj = fuel_cell_car(profile="0:0; 8:70; 120:70")
    system = proj.systems[0]
    gone = {"dc", "fcbus", "batt"}
    system.elements = [e for e in system.elements if e.id not in gone]
    system.elements.append(el("gnd", "boundary.ground", "Ground"))
    system.connections = [c for c in system.connections
                          if not {c.sourceElementId, c.targetElementId} & gone]
    system.connections += [conn(20, "fc", "pos", "hvbus", "t1"), conn(21, "fc", "neg", "gnd", "t1"),
                           conn(22, "mot", "neg", "gnd", "t2")]
    next(e for e in system.elements if e.id == "fc").parameterOverrides["max_current_A"] = max_current_a
    return proj


def test_a_supply_at_its_limit_warns_as_today(new, monkeypatch):
    """Today's run-time warnings that a supply limit held the car back, each
    once and in today's words, judged by today's engine: a fuel cell short
    of the motor's demand, and a battery run empty that cuts its consumer
    back (before: the new path said nothing of either, a success where
    today's warns)."""
    fc = _fuel_cell_on_the_motor_bus(300.0)
    msg = "Fuel cell 'Fuel Cell' demand exceeds its maximum power — motor torque limited."
    today, result = _today(fc, "case", monkeypatch), simulate(fc, "case")
    assert msg in _texts(today, "warning") and _texts(result, "warning").count(msg) == 1
    assert result.status == today.status == "warning"
    assert msg not in _texts(simulate(_fuel_cell_on_the_motor_bus(400.0), "case"))  # (it keeps up)
    empty = _battery(load_example("bev-car"), capacity_kWh=0.62)
    cut = "Power consumer 'Power Consumer' cut back at t = "

    def when(r):
        said = [t for t in _texts(r, "warning") if t.startswith(cut)]
        assert len(said) == 1, _texts(r, "warning")
        return float(said[0][len(cut):].split(" ")[0])
    assert when(simulate(empty, "case-city")) == pytest.approx(
        when(_today(empty, "case-city", monkeypatch)), abs=1.0)


@pytest.mark.parametrize("layout", ["runs-dry", "empty", "braking"])
def test_a_fuel_cell_s_supply_warnings_are_today_s(new, monkeypatch, layout):
    """The fuel cell on the motor's bus, judged by today's engine: its tank
    run dry 1.6 s before the end (today's says its demand exceeds its
    maximum power; before, the new path dropped every stretch held after
    the tank was empty and said nothing, a success where today's warns),
    its tank empty from the start (today's says so, and nothing of its
    maximum power), and braking (no stretch held while recuperating is
    one held while drawing). Its cannot-take-power-back warning is left
    out: today's Driver never asks a fuel cell for recuperation, the new
    engine's does (KNOWN-LIMITS)."""
    proj = _fuel_cell_on_the_motor_bus(400.0)
    tank = next(e for e in proj.systems[0].elements if e.id == "h2").parameterOverrides
    if layout == "runs-dry":  # (empty at 118.4 s of 120 s on the new engine)
        tank.update(capacity_kg=0.018, initial_fill_pct=100.0)
    elif layout == "empty":
        tank.update(capacity_kg=1.0, initial_fill_pct=0.0)
    else:
        next(e for e in proj.systems[0].elements if e.id == "task").parameterOverrides[
            "profile"] = "0:0; 8:70; 60:70; 66:0; 120:0"

    def said(r):
        return sorted(t for t in _texts(r, "warning") if t.startswith(("Fuel cell", "Hydrogen tank"))
                      and "cannot take power back" not in t)
    today, result = _today(proj, "case", monkeypatch), simulate(proj, "case")
    assert said(result) == said(today)
    assert said(today) == {"runs-dry": ["Fuel cell 'Fuel Cell' demand exceeds its maximum power — motor "
                                        "torque limited."],
                           "empty": ["Hydrogen tank empty — fuel cell shut down."], "braking": []}[layout]
    assert result.status == today.status


def test_a_consumer_a_fuel_cell_cuts_back_is_all_it_says(new, monkeypatch):
    """The BEV with a circuit of its own, a fuel cell short of the 60 kW
    Power Consumer on it and no motor: today's engine says the consumer is
    cut back and nothing of the fuel cell's maximum power, which holds back
    no motor; so does the new path."""
    from helpers import conn
    proj = load_example("bev-car")
    system = proj.systems[0]
    system.elements += [el("fc", "fuelcell.stack", "Fuel Cell", max_current_A=100.0),
                        el("h2", "fuel.h2_tank", "H2 Tank"), el("fcbus", "electric.node", "FC Bus"),
                        el("load", "electric.constant_drive", "Load", power_kW=60.0),
                        el("gnd2", "boundary.ground", "Ground 2")]
    system.connections += [conn(90, "fc", "pos", "fcbus", "t1"), conn(91, "load", "pos", "fcbus", "t2"),
                           conn(92, "fc", "neg", "gnd2", "t1"), conn(93, "load", "neg", "gnd2", "t2")]
    next(c for c in proj.cases if c.id == "case-city").duration = 120.0
    today, result = _today(proj, "case-city", monkeypatch), simulate(proj, "case-city")
    assert _texts(result, "warning") == _texts(today, "warning") == [
        "Power consumer 'Load' cut back at t = 0 s — its source cannot supply it."]
    assert result.status == today.status == "warning"


@pytest.mark.parametrize("over", [{"capacity_kWh": 0.62}, {"max_discharge_current_A": 60.0}])
def test_a_battery_held_by_its_own_limits_warns_as_today(new, monkeypatch, over):
    """A battery that holds the motors back at its minimum SOC, or at its
    battery management's discharge current limit, is not at its
    maximum-power point: today's warnings exactly (the minimum SOC's own,
    at the run's event: 214 s, where the output grid's next point said
    215 s), today's verdict, and no maximum-power-point warning (each was
    said with its exclusion taken back)."""
    proj = _battery(load_example("bev-car"), **over)
    today, result = _today(proj, "case-city", monkeypatch), simulate(proj, "case-city")

    def said(r):  # (the cycle's verdict in words carries the run's own numbers)
        return [t for t in _texts(r, "warning") if not t.startswith("Cycle not followed")]
    assert said(result) == said(today)
    assert _verdict(result) == _verdict(today)
    assert not [t for t in _texts(result) if "maximum power point" in t]


def test_a_checked_battery_limit_with_a_script_is_judged_as_today(new, monkeypatch):
    """The battery check of a project with a Script block, whose run goes
    through today's worker (the path's second call of the engine): its
    averaged peak over every step, today's verdict (before the window was
    asked there too, its row read 0.0 kW and passed)."""
    monkeypatch.setenv("LIGHTSIM_SCRIPT_TRUST", "off")
    proj = _battery(load_example("bev-car"), output_power_limit_kW=23.0, power_limit_window_s=0.5,
                    power_limit_enforced=False)
    code = "def step(t, dt, inputs, state, params):\n    return {'y': 1.0}\n"
    proj.systems[0].elements.append(ElementInstance(
        id="s", componentDefId="signal.script", label="S", position={"x": 0, "y": 0},
        parameterOverrides={"code": code, "sample_time_s": 0.1},
        dynamicPorts=[sig_port("y", "output")]))
    proj.systems[0].elements.append(el("mon", "signal.monitor", "Monitor"))
    proj.dataBusConnections.append(dbc(50, "s", "y", "mon", "sig_in"))
    key = "el-battery.peak_terminal_power_averaged_kw"
    today, result = _today(proj, "case-city", monkeypatch), simulate(proj, "case-city")
    assert result.messages[0].text.startswith("Run on the new engine")
    row, old = (next(s for s in r.summary if s.key == key) for r in (result, today))
    assert result.status == today.status == "warning"
    assert row.passed is old.passed is False
    assert row.value == pytest.approx(old.value, rel=2e-3)


def test_a_battery_check_the_run_kept_no_record_of_is_the_path_s_fault(new, monkeypatch):
    """A checked battery whose run kept no moving average of its power (an
    engine call that did not ask for it) fails with the path's code, not a
    pass at 0 kW: before, its averaged peak stayed 0 and passed."""
    real = lightsim_engine.build

    class NoWindows:
        def __init__(self, m):
            self.m = m

        def __getattr__(self, name):
            return getattr(self.m, name)

        def simulate(self, **kw):
            return self.m.simulate(**{k: v for k, v in kw.items() if k != "windows"})
    monkeypatch.setattr(lightsim_engine, "build", lambda *a, **k: NoWindows(real(*a, **k)))
    proj = _battery(load_example("bev-car"), output_power_limit_kW=23.0, power_limit_window_s=0.5,
                    power_limit_enforced=False)
    result = simulate(proj, "case-city")
    assert result.status == "failed"
    first = result.messages[0].text
    assert first.startswith("Not run [NEW-ERROR]") and "the moving average of el-battery:sig_power" in first


def test_a_script_output_with_no_port_warns_as_today(new, monkeypatch):
    """A Script that returns a key none of its output ports has: today's
    engine drops the value and says so, once; so does the new path's runner
    of today's worker (before: a success, nothing said)."""
    monkeypatch.setenv("LIGHTSIM_SCRIPT_TRUST", "off")
    proj = load_example("bev-car")
    code = "def step(t, dt, inputs, state, params):\n    return {'y': 1.0, 'z': 2.0}\n"
    proj.systems[0].elements.append(ElementInstance(
        id="s", componentDefId="signal.script", label="S", position={"x": 0, "y": 0},
        parameterOverrides={"code": code, "sample_time_s": 0.1},
        dynamicPorts=[sig_port("y", "output")]))
    proj.systems[0].elements.append(el("mon", "signal.monitor", "Monitor"))
    proj.dataBusConnections.append(dbc(50, "s", "y", "mon", "sig_in"))
    next(c for c in proj.cases if c.id == "case-city").duration = 60.0
    msg = "Script 'S' returned 'z' which is not one of its output ports — value dropped."
    today, result = _today(proj, "case-city", monkeypatch), simulate(proj, "case-city")
    assert _texts(today, "warning").count(msg) == 1 == _texts(result, "warning").count(msg)
    assert result.status == today.status == "warning"


def test_a_battery_at_its_minimum_soc_is_flagged(new, monkeypatch):
    """A battery run empty: today's flag, which its checks' "usable energy
    left" row fails on (and the consumption rows' first reason), as today's
    engine sets it: without the flag the row passed."""
    proj = _battery(load_example("bev-car"), capacity_kWh=0.62, output_power_limit_kW=200.0)
    key = "el-battery.usable_energy_left_kwh"
    rows = {s.key: s for s in simulate(proj, "case-city").summary}
    old = {s.key: s for s in _today(proj, "case-city", monkeypatch).summary}
    assert rows[key].passed is False and old[key].passed is False


def test_an_empty_fuel_tank_is_flagged(new, monkeypatch):
    """The hybrid's fuel tank run empty on a single UDDS run: its fuel and
    CO₂ rows are not valid, "the fuel tank ran empty", as today's engine
    has them (the flag, not only the message)."""
    monkeypatch.setenv("LIGHTSIM_SCRIPT_TRUST", "off")
    proj = load_example("hybrid-car")
    case = next(c for c in proj.cases if c.id == "case-udds")
    case.chargeBalance, case.duration = False, 400.0
    next(e for e in proj.systems[0].elements if e.id == "el-tank").parameterOverrides["capacity_kg"] = 0.05
    result = simulate(proj, "case-udds")
    assert result.messages[0].text.startswith("Run on the new engine")
    assert _verdict(result) == _verdict(_today(proj, "case-udds", monkeypatch))
    rows = {s.key: s for s in result.summary}
    assert rows["fuel_consumption_l_per_100km"].notValid == "the fuel tank ran empty"


def test_a_preparation_note_that_is_not_a_warning_is_a_note(new, monkeypatch):
    """A build diagnostic whose severity is not "warning" (and which is no
    start note) is a note: it neither makes the run a warning nor hides a
    fault (the build's warnings stay warnings)."""
    real = lightsim_engine.build

    class Noted:
        def __init__(self, m):
            self.m = m

        def __getattr__(self, name):
            return getattr(self.m, name)

        @property
        def report(self):
            rep = dict(self.m.report)
            rep["warnings"] = [*rep["warnings"], {"code": "SOME-NOTE", "severity": "info",
                                                  "message": "something worth knowing"}]
            return rep
    monkeypatch.setattr(lightsim_engine, "build", lambda *a, **k: Noted(real(*a, **k)))
    result = simulate(load_example("bev-car"), "case-city")
    assert "Preparation note [SOME-NOTE]: something worth knowing" in _texts(result, "info")
    assert result.status == "success", _texts(result, "warning")
