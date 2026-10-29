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


def test_a_table_read_past_its_data_is_judged(new, monkeypatch):
    """The engine's records of a table read past its data (its first time,
    for how long, how far) become today's summary rows, note and verdict:
    the BEV's E-Motor drag table cut at 6000 1/min, which the WLTC's top
    speed runs past for much longer than the run's allowance."""
    proj = load_example("bev-car")
    motor = next(e for e in proj.systems[0].elements if e.id == "el-motor")
    motor.parameterOverrides["drag_torque"] = {"0": 0.0, "3000": 0.7, "6000": 0.8}
    result = simulate(proj, "case-wltc")
    rows = {s.key: s for s in result.summary}
    share = rows["el-motor.outside_drag_torque_speed_time_pct"]
    far = rows["el-motor.outside_drag_torque_speed_furthest"]
    speed = next(c for c in result.channels if c.elementId == "el-motor" and c.portId == "sig_speed")
    top = max(v for v in speed.max if v is not None)
    assert far.unit == "1/min" and 6000.0 < far.value <= top * (1 + 1e-6)
    assert far.value == pytest.approx(top, rel=1e-3)
    assert 1.0 < share.value < 100.0 and share.unit == "%"
    assert any("past its 'Drag Torque (unpowered)' table" in t for t in _texts(result, "warning"))
    assert rows["consumption_kwh_per_100km"].notValid.startswith("E-Motor 'E-Motor' ran")
    assert result.status == "warning"


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
