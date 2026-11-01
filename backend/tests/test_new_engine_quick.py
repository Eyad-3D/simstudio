"""Quick mode (step 2's item 5): a case's mode "quick" runs on the new
engine's fast mode (app/solver/new_engine.py: quick, run_quick) whatever
LIGHTSIM_ENGINE says. A case it does not run runs in full instead with a
warning carrying its QUICK-* code, or in strict mode fails with it.

These are guards; the QUICK line of step 2's acceptance
(benchmarks/back_to_back/acceptance.toml) judges every example case
against its full run."""
from __future__ import annotations

import hashlib
import json
import os

import pytest

from app.schemas import RunSnapshot
from app.solver import core, new_engine, simulate
from app.storage import load_example

try:
    import lightsim_engine  # noqa: F401
    MISSING = None
except ImportError as e:
    MISSING = f"the compiled module lightsim_engine is not installed ({e})"
REQUIRED = os.environ.get("LIGHTSIM_REQUIRE_NEW_ENGINE", "").strip() == "1"
needs_engine = pytest.mark.skipif(MISSING is not None and not REQUIRED, reason=MISSING or "")

CITY = "case-city"


@pytest.fixture(autouse=True)
def no_flags(monkeypatch):
    monkeypatch.delenv(new_engine.ENV_VAR, raising=False)
    monkeypatch.delenv(new_engine.STRICT_VAR, raising=False)


def _quick(name: str, case_id: str, **case):
    project = load_example(name)
    c = next(c for c in project.cases if c.id == case_id)
    c.mode = "quick"
    for k, v in case.items():
        setattr(c, k, v)
    return project


def _sha(result) -> str:
    return hashlib.sha256(json.dumps(result.model_dump(mode="json"), sort_keys=True,
                                     separators=(",", ":")).encode()).hexdigest()


@needs_engine
def test_quick_mode_runs_a_cycle_on_fast_mode_without_the_flag(monkeypatch):
    """The BEV's city cycle in quick mode, the flag unset: run by the new
    engine's fast mode (what ran it says so), its rows the figures that sum
    over the run, the charge used within 1 % of the full run's."""
    q = simulate(_quick("bev-car", CITY), CITY)
    assert q.status == "success", [m.text for m in q.messages]
    assert q.engine == "new" and q.solverOptions == {"quickStep": 1.0}
    assert q.messages[0].text.startswith("Quick mode: the new engine's fast mode")
    keys = [s.key for s in q.summary]
    assert all(new_engine.QUICK_ROWS.match(k) for k in keys)
    monkeypatch.setenv(new_engine.ENV_VAR, "new")
    full = simulate(load_example("bev-car"), CITY)
    sums = [s.key for s in full.summary if new_engine.QUICK_ROWS.match(s.key)]
    assert keys == sums  # every such row of the full run, in its order, and no other
    soc0 = next(c for c in full.channels if c.portId == "sig_soc").timeSeries[0]["value"]
    f = {s.key: s.value for s in full.summary}
    used = soc0 - next(s.value for s in q.summary if s.key == "el-battery.final_soc_pct")
    assert used == pytest.approx(soc0 - f["el-battery.final_soc_pct"], rel=0.01)
    assert next(c for c in q.channels if c.portId == "sig_soc").timeSeries[0]["value"] == soc0
    assert not q.energy and not q.duty and not q.partEnergy


@needs_engine
def test_a_limit_the_car_meets_is_flagged_as_a_warning():
    """A battery that cannot give what the cycle asks (a 5 kW output limit,
    enforced): fast mode does not act on it, it flags it, and the run says
    when, as a warning."""
    project = _quick("bev-car", CITY)
    project.cases[[c.id for c in project.cases].index(CITY)].parameterOverrides["el-battery"] = {
        "output_power_limit_kW": 5.0, "power_limit_enforced": True}
    q = simulate(project, CITY)
    flags = [m.text for m in q.messages if m.level == "warning" and m.text.startswith("Quick mode:")]
    assert q.status == "warning" and flags, [m.text for m in q.messages]
    assert "could not have followed the cycle" in flags[0] and "from t = " in flags[0]


@needs_engine
@pytest.mark.parametrize("name,case_id,code", [
    ("fs-electric", "case-accel-75m", "QUICK-KIND"),
    ("hybrid-car", "case-udds", "QUICK-BALANCE"),
])
def test_a_case_quick_mode_does_not_run_runs_in_full_with_its_code(monkeypatch, name, case_id, code):
    """Not strict: the case runs in full (on the engine the flag picks)
    with a warning carrying the code; strict: it fails with the code and
    runs nothing."""
    project = _quick(name, case_id)
    result = simulate(project, case_id)
    assert result.messages[0].level == "warning"
    assert result.messages[0].text.startswith(f"Not run in quick mode [{code}]")
    assert "it ran in full instead" in result.messages[0].text
    assert result.engine != "new"  # (today's: the flag is unset)
    monkeypatch.setenv(new_engine.STRICT_VAR, "1")
    strict = simulate(project, case_id)
    assert strict.status == "failed" and not strict.channels
    assert f"[{code}]" in strict.messages[0].text


@needs_engine
def test_fast_modes_refusal_falls_back_with_quick_failed(monkeypatch):
    """The engine's own refusal of a case (its codes in the words) is
    QUICK-FAILED: the case runs in full instead, saying so."""
    import lightsim_engine as le

    def refuse(*args, **kwargs):
        raise le.ModelError("Fast mode found no cycle to prescribe.")
    monkeypatch.setattr(le, "fast", refuse)
    result = simulate(_quick("bev-car", CITY), CITY)
    assert result.messages[0].text.startswith("Not run in quick mode [QUICK-FAILED]")
    assert result.status != "failed"


@needs_engine
def test_a_quick_run_replays_to_the_same_result():
    """A quick run's snapshot replays to the same SHA-256, on fast mode."""
    project = _quick("bev-car", CITY)
    q = simulate(project, CITY)
    assert q.solverOptions == {"quickStep": 1.0}
    case = next(c for c in project.cases if c.id == CITY)
    snap = RunSnapshot.of(project, case, q)
    again = core.replay(snap)
    assert again.solverOptions == q.solverOptions and _sha(again) == _sha(q)


@needs_engine
@pytest.mark.parametrize("end", [{"endDistance": 3000.0}, {"endLaps": 2.0},
                                 {"startLine": 2000.0, "endDistance": 1000.0}])
def test_a_case_that_ends_at_a_distance_or_laps_runs_in_full_with_quick_end(end):
    """Fast mode runs the cycle's whole duration: a case that ends at a
    Distance or a lap count runs in full with QUICK-END, as today's full run
    of it (its rows and its own warnings), its status re-derived with the
    code's warning (a full run of it that succeeds is a warning)."""
    result = simulate(_quick("bev-car", CITY, **end), CITY)
    assert result.messages[0].text.startswith("Not run in quick mode [QUICK-END]")
    full = simulate(_quick("bev-car", CITY, mode="full", **end), CITY)
    assert result.status == "warning" and full.status in ("success", "warning")
    assert {s.key: s.value for s in result.summary} == {s.key: s.value for s in full.summary}
    assert [m.text for m in result.messages[1:]] == [m.text for m in full.messages]


def _said(result) -> list[str]:
    return [m.text for m in result.messages
            if m.level in ("warning", "error") and not m.text.startswith("Quick mode:")]


@needs_engine
@pytest.mark.parametrize("ambient", [{"temperature_C": 65.0}, {"temperature_C": -70.0},
                                     {"pressure_kPa": 121.59}])
def test_quick_mode_says_todays_checks_of_the_run(ambient):
    """An Ambient outside the usual air, and the Climate Control's demand
    table read past its data at that temperature: quick mode warns as
    today's full run does, in its words, marks the same of its rows not
    valid and grades the case's references as today's does."""
    project = _quick("bev-car", CITY)
    case = next(c for c in project.cases if c.id == CITY)
    case.parameterOverrides.setdefault("el-ambient", {}).update(ambient)
    q = simulate(project, CITY)
    assert q.engine == "new"
    case.mode = "full"
    full = simulate(project, CITY)
    assert _said(q) == _said(full) and _said(q) and q.status == full.status == "warning"
    assert {s.key: s.notValid for s in q.summary} == {
        s.key: s.notValid for s in full.summary if new_engine.QUICK_ROWS.match(s.key)}
    assert case.references and [r.label for r in q.references] == [r.kpi for r in case.references]
    graded = {r.label: (r.grade, r.note) for r in full.references}
    assert [(r.grade, r.note) for r in q.references] == [graded[r.label] for r in q.references]


@needs_engine
def test_quick_mode_says_the_model_builders_warnings():
    """A second Ambient: the model builder's warning, in today's words."""
    from app.schemas import ElementInstance
    project = _quick("bev-car", CITY)
    project.systems[0].elements.append(ElementInstance(
        id="amb2", componentDefId="boundary.ambient", label="Second Ambient", position={"x": 0, "y": 0}))
    q = simulate(project, CITY)
    assert q.engine == "new" and q.status == "warning"
    assert any(t.startswith("Only the first Ambient") for t in _said(q)), _said(q)


def _cut(project, case_id: str, key: str, axis: int, last: float) -> None:
    """The case's E-Motor table ``key`` cut to its points up to ``last`` on
    ``axis`` (0: the outer, 1: a 2D table's inner)."""
    case = next(c for c in project.cases if c.id == case_id)
    motor = next(e for e in project.systems[0].elements if e.id == "el-motor")
    table = motor.parameterOverrides[key]
    cut = ({k: v for k, v in table.items() if float(k) <= last} if axis == 0
           else {k: {x: y for x, y in v.items() if float(x) <= last} for k, v in table.items()})
    case.parameterOverrides.setdefault("el-motor", {})[key] = cut


@needs_engine
@pytest.mark.parametrize("axis, last", [(0, 4000.0), (1, 50.0)], ids=["speed", "torque"])
def test_a_table_axis_set_to_error_read_past_its_data_is_not_run_in_quick_mode(monkeypatch, axis, last):
    """The BEV's E-Motor Power Loss map, both of whose axes are set to
    Error, cut to 4000 1/min (its Speed axis) or to 50 N·m (its Torque
    axis), on the WLTC: today's full run stops there and fails. Fast mode
    stops there too (FAST-TABLE-ERROR): the case runs in full with
    QUICK-FAILED and fails as today's does, in its words; strict mode fails
    with the code (before: the whole cycle's totals as a warning, strict
    too)."""
    wltc = "case-wltc"
    project = _quick("bev-car", wltc)
    _cut(project, wltc, "power_loss", axis, last)
    q = simulate(project, wltc)
    first = q.messages[0].text
    assert first.startswith("Not run in quick mode [QUICK-FAILED]") and "FAST-TABLE-ERROR" in first
    next(c for c in project.cases if c.id == wltc).mode = "full"
    today = simulate(project, wltc)
    assert today.engine == "today" and today.status == "failed"
    stop = [t for t in _said(today) if "set to stop the run (Error)" in t]
    assert len(stop) == 1 and stop[0] in _said(q)
    assert q.status == "failed"
    assert {s.key: s.value for s in q.summary} == {s.key: s.value for s in today.summary}
    next(c for c in project.cases if c.id == wltc).mode = "quick"
    monkeypatch.setenv(new_engine.STRICT_VAR, "1")
    strict = simulate(project, wltc)
    assert strict.status == "failed" and not strict.channels
    assert "[QUICK-FAILED]" in strict.messages[0].text
    assert "FAST-TABLE-ERROR" in strict.messages[0].text


@needs_engine
@pytest.mark.parametrize("case_id", [CITY, "case-wltc"])
def test_a_table_today_reads_only_while_unpowered_is_not_judged_in_quick_mode(case_id):
    """The BEV's E-Motor Drag Torque (unpowered) table cut to 4000 1/min:
    today's engine reads it only while the motor is unpowered, which on
    these cycles it never is that fast, and passes the run. Quick mode
    gives today's verdict: no warning, no row not valid, the references
    graded as today's; a note with NEW-TABLE-NOT-JUDGED says the record is
    not judged and why (before: a warning, the consumptions not valid)."""
    project = _quick("bev-car", case_id)
    _cut(project, case_id, "drag_torque", 0, 4000.0)
    q = simulate(project, case_id)
    assert q.engine == "new"
    next(c for c in project.cases if c.id == case_id).mode = "full"
    today = simulate(project, case_id)
    assert today.engine == "today" and today.status == "success"
    assert q.status == "success", _said(q)
    assert not [s.key for s in q.summary if s.notValid]
    graded = {r.label: r.grade for r in today.references}
    assert [r.grade for r in q.references] == [graded[r.label] for r in q.references]
    notes = [m.text for m in q.messages if f"[{new_engine.NOT_JUDGED}]" in m.text]
    assert len(notes) == 1 and "only while the motor is unpowered" in notes[0], notes
