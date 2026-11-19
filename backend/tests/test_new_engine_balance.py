"""A hybrid's charge-balanced cycle case on the new engine (balance.py,
ENG-33): run after run from the charge the last ended with, as today's
engine runs it, with what simulate_balanced promises (the first run
streamed, the later ones quiet; a live edit, a stop or a failure; the
summary rows), a replay that makes it again bit for bit, and a study's
point balanced as on today's engine. test_charge_balance.py holds the
figures; the module lightsim_engine is needed (needs_engine)."""
from __future__ import annotations

import hashlib
import json

import pytest
from test_new_engine_flag import needs_engine

from app import studies
from app.schemas import RunSnapshot, SimResult
from app.solver import new_engine, replay, simulate
from app.storage import load_example
from app.version import VERSION

pytestmark = needs_engine

CASE = "case-udds"
ROWS = ("el-battery.balanced_start_soc_pct", "battery_energy_change_pct_of_fuel", "charge_balance_runs")


@pytest.fixture(autouse=True)
def _new(monkeypatch):
    monkeypatch.setenv(new_engine.ENV_VAR, "new")


def _sha(result) -> str:
    return hashlib.sha256(json.dumps(result.model_dump(mode="json"), sort_keys=True,
                                     separators=(",", ":")).encode()).hexdigest()


def _hybrid(soc=None, **case):
    p = load_example("hybrid-car")
    c = next(c for c in p.cases if c.id == CASE)
    if soc is not None:
        c.parameterOverrides["el-battery"]["initial_soc_pct"] = soc
    for k, v in case.items():
        setattr(c, k, v)
    return p


def _row(result, key):
    return next((s for s in result.summary if s.key == key), None)


def _replayed(project, result):
    return replay(RunSnapshot.of(project, next(c for c in project.cases if c.id == CASE),
                                 result, VERSION))


class _Live:
    """A live run's emit and control: ``stop`` once balancing has said run 2
    starts, ``edit`` once a point at or after 100 s has streamed."""

    def __init__(self, stop=False, edit=None):
        self.stream, self.stop, self.edit = [], stop, edit

    def emit(self, msg):
        self.stream.append(msg)

    def control(self):
        if self.stop and any("Charge balancing: run 2 " in m.get("text", "") for m in self.stream):
            return [{"type": "cancel"}]
        if self.edit and any(m["type"] == "step" and m["t"] >= 100.0 for m in self.stream):
            edit, self.edit = self.edit, None
            return [edit]
        return []


def test_a_hybrids_cycle_case_runs_charge_balanced():
    """The hybrid's EPA city case with its default Charge balance runs on
    the new engine (before: refused, NEW-BALANCE), with today's rows under
    today's keys (each once) and its message second, after the engine's."""
    result = simulate(load_example("hybrid-car"), CASE)
    assert result.engine == "new" and result.status in ("success", "warning"), result.messages[:3]
    keys = [s.key for s in result.summary]
    assert len(keys) == len(set(keys)), keys
    assert all(k in keys for k in ROWS), keys
    assert _row(result, "fuel_consumption_l_per_100km").label == "Fuel consumption"
    assert result.messages[0].text == "Run on the new engine."
    assert result.messages[1].text.startswith(("Charge-balanced in ", "Charge balancing did not settle"))
    assert result.solverOptions[new_engine.BALANCE_RUNS] == _row(result, "charge_balance_runs").value


def test_the_first_run_streams_and_the_later_ones_say_which_run_they_are():
    live = _Live()
    result = simulate(_hybrid(30.0), CASE, live.emit, live.control)
    runs = int(_row(result, "charge_balance_runs").value)
    assert runs >= 2
    steps = [m for m in live.stream if m["type"] == "step"]
    assert len(steps) == len(result.channels[0].timeSeries)  # one run's points: the first's
    said = [m["text"] for m in live.stream if m["type"] == "message"
            and m["text"].startswith("Charge balancing: run ")]
    assert said == [f"Charge balancing: run {k} of up to 5, starting at the charge run {k - 1} "
                    f"ended with." for k in range(2, runs + 1)]
    # the replay balances again: the same result, bit for bit
    assert _sha(_replayed(_hybrid(30.0), result)) == _sha(result)


def test_a_streamed_run_without_a_control_is_not_taken_for_an_edited_one():
    """A run that streams its points but reads no control (a script's emit)
    made no live edit: balancing goes on to its second run."""
    stream = []
    result = simulate(_hybrid(30.0, duration=300.0), CASE, stream.append)
    assert result.engine == "new" and result.liveEdits == []
    assert not any("a parameter was changed" in m.text for m in result.messages), result.messages[1].text
    assert _row(result, "charge_balance_runs").value >= 2


def test_a_live_edit_in_the_first_run_ends_balancing_and_replays():
    edit = {"type": "set_param", "elementId": "el-vehicle", "key": "cd", "value": 0.35}
    live = _Live(edit=edit)
    result = simulate(_hybrid(30.0), CASE, live.emit, live.control)
    assert [(e.elementId, e.key, e.value) for e in result.liveEdits] == [("el-vehicle", "cd", 0.35)]
    assert _row(result, "charge_balance_runs").value == 1
    assert result.status == "warning"
    assert any("did not settle (a parameter was changed while it ran)" in m.text for m in result.messages)
    assert _sha(_replayed(_hybrid(30.0), result)) == _sha(result)


def test_a_stop_in_a_later_run_reports_the_last_finished_run_and_replays():
    live = _Live(stop=True)
    result = simulate(_hybrid(30.0), CASE, live.emit, live.control)
    assert result.engine == "new" and result.status == "warning"
    assert _row(result, "charge_balance_runs").value == 1
    assert any("did not settle (run 2 was stopped)" in m.text for m in result.messages)
    assert result.solverOptions[new_engine.BALANCE_STOPPED] == 1.0
    assert _sha(_replayed(_hybrid(30.0), result)) == _sha(result)


def test_a_failure_in_a_later_run_reports_the_last_finished_run(monkeypatch):
    run_new, calls = new_engine.run_new, []

    def failing(*a, **kw):
        calls.append(1)
        if len(calls) == 2:
            raise RuntimeError("planted")
        return run_new(*a, **kw)
    monkeypatch.setattr(new_engine, "run_new", failing)
    result = simulate(_hybrid(30.0), CASE)
    assert result.engine == "new" and result.status == "warning"
    assert _row(result, "charge_balance_runs").value == 1
    assert any("did not settle (run 2 was cut short)" in m.text for m in result.messages)
    assert any(m.level == "error" and m.text.startswith("Not run [NEW-ERROR]") and "planted" in m.text
               for m in result.messages), [m.text for m in result.messages]


@pytest.mark.parametrize("case, balanced", [
    (dict(realtimeFactor=50.0, duration=20.0), False),  # paced (live tuning)
    (dict(chargeBalance=False, duration=20.0), False),
    (dict(chargeBalance=True, kind="performance", duration=60.0), True),
])
def test_which_cases_are_balanced(case, balanced):
    result = simulate(_hybrid(**case), CASE, (lambda m: None), (lambda: []))
    assert result.engine == "new" and result.status != "failed", result.messages[:3]
    said = [m.text for m in result.messages if m.text.startswith(("Charge-balanced", "Charge balancing"))]
    assert bool(said) == balanced, said


def test_a_studys_point_is_balanced_as_on_todays_engine():
    """A study's point runs through simulate in its worker (studies.run_point):
    on the new engine it is charge-balanced as on today's."""
    project = _hybrid(30.0)
    got, _ = studies.run_point(project.model_dump_json(), CASE)
    point = SimResult.model_validate_json(got)
    assert point.engine == "new" and _row(point, "charge_balance_runs").value >= 2
