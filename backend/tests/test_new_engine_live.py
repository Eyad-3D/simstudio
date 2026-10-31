"""Live runs on the new engine's path (step 2, item 4; review finding 19):
one stop and progress hook, the engine's RunControl, serves the app's live
run (its points streamed as they are recorded, the result's bit for bit,
at the case's pace), a study's Stop, and live edits of runtime parameters
(each an event at the run's next output time); the result says which
engine ran it, its version, its solver options and the live edits it made,
and a replay of its RunSnapshot gives the same result (its SHA-256).

Needs the compiled module ``lightsim_engine`` (as test_new_engine_flag.py:
skipped without it, failing instead with LIGHTSIM_REQUIRE_NEW_ENGINE=1)."""
from __future__ import annotations

import hashlib
import json
import threading
import time

import pytest
from test_new_engine_flag import needs_engine

from app import studies
from app.schemas import LiveEdit, RunSnapshot
from app.solver import new_engine, replay, simulate
from app.storage import load_example
from app.version import VERSION

pytestmark = needs_engine


@pytest.fixture(autouse=True)
def _new(monkeypatch):
    monkeypatch.setenv(new_engine.ENV_VAR, "new")
    monkeypatch.setenv(new_engine.STRICT_VAR, "1")


def _sha(result) -> str:
    return hashlib.sha256(json.dumps(result.model_dump(mode="json"), sort_keys=True,
                                     separators=(",", ":")).encode()).hexdigest()


def _case(project, case_id):
    return next(c for c in project.cases if c.id == case_id)


class _Live:
    """A live run's emit and control: the messages streamed, and what the
    control gives once a point at or after ``at`` s has streamed."""

    def __init__(self, at=float("inf"), then=()):
        self.stream, self.at, self.then, self.lock = [], at, list(then), threading.Lock()

    def emit(self, msg):
        with self.lock:
            self.stream.append(msg)

    def control(self):
        with self.lock:
            due = any(m["type"] == "step" and m["t"] >= self.at for m in self.stream)
            out, self.then = (self.then, []) if due else ([], self.then)
        return out

    def steps(self):
        return [m for m in self.stream if m["type"] == "step"]


def _streamed_is_the_result(steps, result):
    """Every streamed point against the final result, bit for bit: its time,
    every channel it holds, every channel the result has a value for."""
    final = {f"{c.elementId}:{c.portId}": [p["value"] for p in c.timeSeries]
             for c in result.channels}
    times = [p["t"] for p in result.channels[0].timeSeries]
    assert len(steps) == len(times)
    for k, m in enumerate(steps):
        assert m["t"] == times[k]
        assert set(m["values"]) == set(final), k
        for name, v in m["values"].items():
            assert v == final[name][k] or (v is None and final[name][k] is None), (name, k)


@pytest.mark.parametrize("example,case_id,duration", [
    ("bev-car", "case-city-live", None),
    ("hybrid-car", "case-mixed-live", 120.0),  # (its Script block in today's worker)
])
def test_a_live_run_streams_every_point_of_its_result(example, case_id, duration):
    project = load_example(example)
    case = _case(project, case_id)
    case.realtimeFactor = 1000.0  # (its pace does not change a result)
    if duration:
        case.duration = duration
    live = _Live()
    result = simulate(project, case_id, live.emit, live.control)
    assert result.messages[0].text.startswith("Run on the new engine"), result.messages[0].text
    assert result.status in ("success", "warning") and result.engine == "new"
    _streamed_is_the_result(live.steps(), result)
    assert [m["pct"] for m in live.steps()][-1] == 100.0


def test_a_paced_run_keeps_its_pace():
    """A run with a real-time factor streams its points no faster than the
    clock allows (the engine waits on the run's queue meanwhile)."""
    project = load_example("bev-car")
    case = _case(project, "case-city-live")
    case.duration, case.realtimeFactor = 20.0, 40.0  # (0.5 s of wall time)
    live = _Live()
    t0 = time.monotonic()
    result = simulate(project, "case-city-live", live.emit, live.control)
    assert time.monotonic() - t0 >= 0.45
    _streamed_is_the_result(live.steps(), result)


def test_a_stop_ends_the_run_where_it_is():
    """The app's cancel stops the run at the engine's next step: the result
    ends there, cancelled, with today's words and its figures per distance
    not valid, as today's stopped run has them."""
    live = _Live(at=400.0, then=[{"type": "cancel"}])
    result = simulate(load_example("bev-car"), "case-wltc", live.emit, live.control)
    times = [p["t"] for p in result.channels[0].timeSeries]
    assert result.status == "cancelled", [m.text for m in result.messages]
    assert 400.0 <= times[-1] < 1800.0
    said = [m.text for m in result.messages if "cancelled by user" in m.text]
    assert said == [f"Simulation cancelled by user at t = {times[-1]:g} s."]
    row = next(s for s in result.summary if s.label == "Consumption")
    assert row.notValid == f"run cancelled at t = {times[-1]:g} s"
    assert result.energy is not None and result.limits is not None


def test_a_study_point_stops_on_the_new_engine():
    """A study's Stop (the workers' cancel flag, polled through the run's
    control) stops a point that is running: it ends cancelled at t > 0."""
    stop = threading.Event()
    studies._init_worker(stop)
    try:
        out = {}
        project = load_example("bev-car").model_dump_json()
        th = threading.Thread(target=lambda: out.update(r=studies.run_point(project, "case-wltc")))
        th.start()
        time.sleep(0.3)
        stop.set()
        th.join()
    finally:
        studies._init_worker(None)
    from app.schemas import SimResult
    result = SimResult.model_validate_json(out["r"][0])
    assert result.status == "cancelled" and result.engine == "new"
    assert studies._incomplete(result).startswith("stopped at t = ")
    assert float(studies._incomplete(result).split("= ")[1].split(" s")[0]) > 0.0


def test_a_live_edit_is_made_at_the_next_output_time_and_replayed_exactly():
    """A live edit of a runtime parameter (the Vehicle's drag coefficient)
    is an event at the run's next output time; the result lists it at the
    time the engine took it, and a replay of the run's snapshot makes it
    again there: the same result, bit for bit. A structural parameter's
    edit takes effect on the next run, as today's says."""
    project = load_example("bev-car")
    edit = {"type": "set_param", "elementId": "el-vehicle", "key": "cd", "value": 0.35}
    fixed = {"type": "set_param", "elementId": "el-final-drive", "key": "ratio", "value": 9.0}
    live = _Live(at=300.0, then=[edit, fixed])
    result = simulate(project, "case-wltc", live.emit, live.control)
    (made,) = result.liveEdits
    assert (made.elementId, made.key, made.value) == ("el-vehicle", "cd", 0.35)
    assert 300.0 <= made.t < 1800.0
    told = "'Final Drive.ratio' changed — structural parameters take effect on the next run."
    assert {"type": "message", "level": "info", "text": told} in live.stream
    assert told not in [m.text for m in result.messages]  # (the run's own words: a replay's)
    table = {"type": "set_param", "elementId": "el-climate", "key": "demand_table",
             "value": {"-20": 6.0, "40": 4.0}}
    later = _Live(at=10.0, then=[table])
    told = "'Climate Control.demand_table' changed — a table takes effect on the next run on the new engine."
    tabled = simulate(project, "case-city", later.emit, later.control)
    assert tabled.liveEdits == [] and {"type": "message", "level": "info", "text": told} in later.stream
    plain = simulate(project, "case-wltc")
    assert plain.liveEdits == [] and _sha(plain) != _sha(result)
    again = replay(RunSnapshot.of(project, _case(project, "case-wltc"), result, VERSION))
    assert again.liveEdits == result.liveEdits
    assert _sha(again) == _sha(result)


def test_the_result_says_what_ran_it_and_the_snapshot_keeps_it(monkeypatch):
    import lightsim_engine

    project = load_example("bev-car")
    new = simulate(project, "case-city")
    assert (new.engine, new.engineVersion) == ("new", lightsim_engine.version())
    assert set(new.solverOptions) == {"rtol", "atol", "outputStep", "traceStep"}
    snap = RunSnapshot.of(project, _case(project, "case-city"), new, VERSION)
    assert (snap.engine, snap.engineVersion, snap.solverOptions) == (
        new.engine, new.engineVersion, new.solverOptions)
    assert _sha(replay(snap)) == _sha(new)

    monkeypatch.delenv(new_engine.ENV_VAR)
    monkeypatch.delenv(new_engine.STRICT_VAR)
    today = simulate(project, "case-city")
    assert (today.engine, today.engineVersion) == ("today", VERSION)
    assert set(today.solverOptions) == {"timeStep", "solverStep", "outputEvery"}
    monkeypatch.setenv(new_engine.ENV_VAR, "new")  # (the snapshot's engine, not the flag's)
    snap = RunSnapshot.of(project, _case(project, "case-city"), today, VERSION)
    assert _sha(replay(snap)) == _sha(today)
    edited = snap.model_copy(update={"liveEdits": [LiveEdit(t=1.0, elementId="el-vehicle",
                                                            key="cd", value=0.3)]})
    refused = replay(edited)
    assert refused.status == "failed" and "live edits" in refused.messages[0].text
