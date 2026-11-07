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

import gc
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


def test_a_live_run_s_points_are_not_kept_on_the_way():
    """The path keeps only whether a live run has shown a point, not the
    points: a long run's stream, which the app's websocket sends on and
    drops, is not held in memory until the run ends (no list holds one)."""
    held = []

    def emit(msg):  # (a few points looked at: gc.get_referrers scans every object)
        if msg["type"] == "step" and msg["t"] % 100 == 0:
            held.append(any(isinstance(r, list) for r in gc.get_referrers(msg)))
    simulate(load_example("bev-car"), "case-city", emit, lambda: [])
    assert held and not any(held)


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


def test_a_stop_and_an_edit_land_near_the_point_the_view_shows():
    """The engine runs at most LIVE_QUEUE output points ahead of the view
    (and the step it is taking), so a Stop or a live edit given once the
    view shows 200 s of the city cycle (1 s points, run as fast as it goes)
    lands within 10 s of it (with room for 256 points an edit landed 279 s
    later; today's engine makes it at its next step)."""
    project = load_example("bev-car")
    live = _Live(at=200.0, then=[{"type": "cancel"}])
    stopped = simulate(project, "case-city", live.emit, live.control)
    assert 200.0 <= stopped.channels[0].timeSeries[-1]["t"] < 210.0
    live = _Live(at=200.0, then=[{"type": "set_param", "elementId": "el-vehicle", "key": "cd",
                                  "value": 0.35}])
    (made,) = simulate(project, "case-city", live.emit, live.control).liveEdits
    assert 200.0 <= made.t < 210.0


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
    edit takes effect on the next run, as today's says, and the result
    lists it with its note, which a replay gives again."""
    project = load_example("bev-car")
    edit = {"type": "set_param", "elementId": "el-vehicle", "key": "cd", "value": 0.35}
    fixed = {"type": "set_param", "elementId": "el-final-drive", "key": "ratio", "value": 9.0}
    live = _Live(at=300.0, then=[edit, fixed])
    result = simulate(project, "case-wltc", live.emit, live.control)
    made, deferred = result.liveEdits
    assert (made.elementId, made.key, made.value, made.note) == ("el-vehicle", "cd", 0.35, None)
    assert 300.0 <= made.t < 1800.0
    told = ("'Final Drive.ratio' changed — structural parameters take effect on the next run "
            "[NEW-LIVE-NEXT-RUN].")
    assert (deferred.elementId, deferred.key, deferred.note) == ("el-final-drive", "ratio", told)
    assert {"type": "message", "level": "info", "text": told} in live.stream
    assert [m.text for m in result.messages].count(told) == 1
    table = {"type": "set_param", "elementId": "el-climate", "key": "demand_table",
             "value": {"-20": 6.0, "40": 4.0}}
    later = _Live(at=10.0, then=[table])
    told = ("'Climate Control.demand_table' changed — a table takes effect on the next run on the "
            "new engine [NEW-LIVE-NEXT-RUN].")
    tabled = simulate(project, "case-city", later.emit, later.control)
    # (a live edit holds a scalar, and the app sends no table: its note is the live log's)
    assert tabled.liveEdits == [] and {"type": "message", "level": "info", "text": told} in later.stream
    plain = simulate(project, "case-wltc")
    assert plain.liveEdits == [] and _sha(plain) != _sha(result)
    again = replay(RunSnapshot.of(project, _case(project, "case-wltc"), result, VERSION))
    assert again.liveEdits == result.liveEdits
    assert _sha(again) == _sha(result)


class _Timed(_Live):
    """A live run's emit and control that gives each (t, message) once a
    point at or after t has streamed."""

    def __init__(self, timed):
        super().__init__()
        self.timed = list(timed)

    def control(self):
        with self.lock:
            last = max((m["t"] for m in self.stream if m["type"] == "step"), default=-1.0)
            out = [m for t, m in self.timed if t <= last]
            self.timed = [(t, m) for t, m in self.timed if t > last]
        return out


def _set(el_id, key, value):
    return {"type": "set_param", "elementId": el_id, "key": key, "value": value}


def _replayed(project, case_id, result):
    return replay(RunSnapshot.of(project, _case(project, case_id), result, VERSION))


def _consumption(result):
    return next(s.value for s in result.summary if s.key == "consumption_kwh_per_100km")


def test_two_edits_of_one_parameter_are_both_made_live_and_replayed():
    """The second edit of a parameter is made on top of the first (the
    drag coefficient up at 200 s, back to its own at 400 s), live and in a
    replay of the run, which is the same bit for bit."""
    project = load_example("bev-car")
    up, back = _set("el-vehicle", "cd", 0.35), _set("el-vehicle", "cd", 0.27)  # (0.27: its own)
    one = _Timed([(200.0, up)])
    first = simulate(project, "case-city", one.emit, one.control)
    two = _Timed([(200.0, up), (400.0, back)])
    both = simulate(project, "case-city", two.emit, two.control)
    assert [(e.value, e.note) for e in both.liveEdits] == [(0.35, None), (0.27, None)]
    assert 400.0 <= both.liveEdits[1].t < 600.0
    plain = simulate(project, "case-city")
    assert _consumption(plain) < _consumption(both) < _consumption(first)
    again = _replayed(project, "case-city", both)
    assert again.liveEdits == both.liveEdits and _sha(again) == _sha(both)


def test_an_edit_the_new_engine_builds_another_model_for_is_for_the_next_run():
    """A tunable edit the new engine runs another structure for (the
    Differential's lock; today's engine makes it live) takes effect on the
    next run: the result lists it with its coded note, and its replay gives
    the same result."""
    project = load_example("bev-car")
    live = _Live(at=100.0, then=[_set("el-diff", "locked", True)])
    result = simulate(project, "case-city", live.emit, live.control)
    told = ("'Differential.locked' changed — the new engine builds another model for it: it takes "
            "effect on the next run [NEW-LIVE-NEXT-RUN].")
    assert [(e.key, e.value, e.note) for e in result.liveEdits] == [("locked", True, told)]
    assert [m.text for m in result.messages].count(told) == 1
    plain = simulate(project, "case-city")
    assert [(s.key, s.value) for s in result.summary] == [(s.key, s.value) for s in plain.summary]
    assert _sha(_replayed(project, "case-city", result)) == _sha(result)


# what the path's checks after the run read (new_engine._checked): an edit of
# each, which today's engine checks at each step with the values then
CHECKED = [
    ("bev-car", "case-city", "el-ambient", "temperature_C", 65),  # the air outside the usual
    ("bev-car", "case-city", "el-ambient", "pressure_kPa", 121.59),
    ("bev-car", "case-city", "el-motor", "max_speed_rpm", 1.0),  # a motor past its maximum speed
    ("bev-car", "case-city", "el-battery", "min_soc_pct", 95),  # a battery at its minimum SOC
    ("bev-car", "case-city", "el-battery", "max_charge_power_kW", 10),  # charging held back
    ("bev-car", "case-city", "el-motor", "torque_scale_pct", 120),  # a table's axes
    ("fs-electric", "case-accel-75m", "el-battery", "voltage_class_V", 480),  # its checks
    ("fs-electric", "case-accel-75m", "el-vehicle", "cg_height_m", 0.4),  # a lift
]
CHECKS_NOTE = ("the new engine's checks after its run read it as it was at the run's start: it "
               "takes effect on the next run [NEW-LIVE-NEXT-RUN].")


@pytest.mark.parametrize("example,case_id,el_id,key,value", CHECKED)
def test_each_value_the_checks_after_the_run_read_is_an_edit_for_the_next_run(
        example, case_id, el_id, key, value):
    import lightsim_engine

    from app import cycles

    project = load_example(example)
    built = lightsim_engine.build(project.model_dump_json(), case_id, cycles_dir=str(cycles.DIR))
    edits = new_engine._Edits(project, case_id, built)
    with pytest.raises(new_engine._Later, match="checks after its run read it"):
        edits.changes(el_id, key, value)
    assert edits.changes("el-vehicle", "cd", 0.4)  # (one they do not read is made)


@pytest.mark.parametrize("el_id,key,value", [c[2:] for c in CHECKED[:4]])
def test_an_edit_the_checks_after_the_run_read_is_not_made(el_id, key, value):
    """An edit today's engine flags at the step it sees it (the Ambient's
    air, a motor's maximum speed, a battery's minimum SOC) would go unjudged
    by the path's checks, which read the parts as at the run's start: it is
    not made, the run is the run without it, the result lists it with its
    coded note, and a replay gives the same result."""
    project = load_example("bev-car")
    live = _Live(at=200.0, then=[_set(el_id, key, value)])
    result = simulate(project, "case-city", live.emit, live.control)
    (taken,) = result.liveEdits
    assert taken.note == f"'{taken.note.split(chr(39))[1]}' changed — {CHECKS_NOTE}"
    assert taken.note in [m.text for m in result.messages]
    plain = simulate(project, "case-city")
    assert [(s.key, s.value, s.notValid) for s in result.summary] == [
        (s.key, s.value, s.notValid) for s in plain.summary]
    assert result.status == plain.status
    assert _sha(_replayed(project, "case-city", result)) == _sha(result)


def test_an_edit_too_late_for_the_run_is_listed_with_its_note():
    """An edit given once the run has shown its last point is not made:
    the result lists it with its coded note (the engine's words when the run
    had ended, else too late), the live log gives it once, and a replay
    gives the same result. One the engine was given and did not make came
    too late, which the live log gives once when the run ends (_Live.made)."""
    project = load_example("bev-car")
    _case(project, "case-city").duration = 20.0
    live = _Live(at=20.0, then=[_set("el-vehicle", "cd", 0.35)])
    result = simulate(project, "case-city", live.emit, live.control)
    (late,) = result.liveEdits
    assert late.note and late.note.startswith("'Vehicle.cd' changed") and late.note.endswith(
        "[NEW-LIVE-NEXT-RUN].") and late.note in [m.text for m in result.messages]
    assert [m for m in live.stream if m["type"] == "message"] == [
        {"type": "message", "level": "info", "text": late.note}]
    assert _sha(_replayed(project, "case-city", result)) == _sha(result)
    model = new_engine._context(project, "case-city").model
    told = "'Vehicle.cd' changed too late in the run to take effect: it takes effect on the next run " \
           "[NEW-LIVE-NEXT-RUN]."
    (e,) = new_engine._made([("el-vehicle", "cd", 0.35, 19.5, True, None)], [], model)
    assert (e.t, e.note) == (19.5, told)
    # (whether the engine takes an edit given at its last step is a race: what made() says, alone)
    run = new_engine._Live.__new__(new_engine._Live)
    stream = []
    run.model, run.emit, run.said = model, stream.append, set()
    run.taken = [("el-vehicle", "cd", 0.35, 19.5, True, None)]
    for _ in range(2):  # (said once)
        assert [e.note for e in run.made([])] == [told]
    assert stream == [{"type": "message", "level": "info", "text": told}]


def _engine(monkeypatch, engine):
    if engine == "new":
        monkeypatch.setenv(new_engine.ENV_VAR, "new")
        monkeypatch.setenv(new_engine.STRICT_VAR, "1")
    else:
        monkeypatch.delenv(new_engine.ENV_VAR, raising=False)
        monkeypatch.delenv(new_engine.STRICT_VAR, raising=False)


def test_a_made_edit_is_read_as_edited_by_the_reports_as_today_s_are(monkeypatch):
    """The reports read the parts as the run left them, as today's engine's
    RunContext has them at its run's end: the hand calculation of the road
    load with the drag coefficient as edited, within 1 % of today's with the
    same edit (and 5 % above the run's own without it)."""
    project = load_example("bev-car")
    need = {}
    for engine in ("new", "today"):
        _engine(monkeypatch, engine)
        live = _Live(at=200.0, then=[_set("el-vehicle", "cd", 0.35)])
        result = simulate(project, "case-city", live.emit, live.control)
        need[engine] = next(r.reference for r in result.references if "road load" in r.label)
    plain = simulate(project, "case-city")  # (today's)
    unedited = next(r.reference for r in plain.references if "road load" in r.label)
    assert abs(need["new"] - need["today"]) <= 0.01 * need["today"]
    assert need["new"] > 1.05 * unedited


def test_a_stopped_acceleration_test_has_no_finish(monkeypatch):
    """A Stop during an acceleration test's second run (the one the live
    view shows) ends it where it is, short of the line: no time, speed or
    gap at 75 m, and what it says judged as a stopped run, as on today's
    engine."""
    said = {}
    for engine in ("new", "today"):
        _engine(monkeypatch, engine)
        live = _Live(at=1.0, then=[{"type": "cancel"}])
        result = simulate(load_example("fs-electric"), "case-accel-75m", live.emit, live.control)
        assert result.status == "cancelled", (engine, [m.text for m in result.messages])
        said[engine] = ({s.key: (s.passed, s.notValid) for s in result.summary
                         if s.key.startswith("accel")},
                        sorted(m.level for m in result.messages if m.level != "info"))
    assert said["new"] == said["today"] == ({}, [])


def test_a_live_run_the_new_engine_fails_is_not_run_again_on_today_s(monkeypatch):
    """In the app's mode (not strict), a live run the new engine fails after
    it has streamed points, or read an edit, fails with its code, saying
    what today's engine, streaming it again from t = 0, would lose (never a
    Stop or edits it was not given). A run that showed and read nothing (a
    study's point), and the AI tools' run, whose points go to a progress
    bar only (whether it has one or not), run on today's, saying so with
    the code."""
    from app.ai.engine import Engine

    monkeypatch.delenv(new_engine.STRICT_VAR)
    project = load_example("bev-car")  # (its battery's resistance x100: the run stops at ~321 s)
    _case(project, "case-city").parameterOverrides.setdefault("el-battery", {})[
        "internal_resistance_ohm"] = 8.0
    for then, lost in (([], "after the points it has shown"),
                       ([_set("el-vehicle", "cd", 0.3)], "without the live edits it was given")):
        live = _Live(at=100.0, then=then)
        result = simulate(project, "case-city", live.emit, live.control)
        times = [m["t"] for m in live.steps()]
        assert times == sorted(set(times)) and 300.0 < times[-1] < 600.0  # (never from 0 again)
        assert result.status == "failed" and result.engine is None
        (msg,) = result.messages
        assert "[NEW-FAILED]" in msg.text and "RUN-INTEGRATOR" in msg.text, msg.text
        assert f"It is not run again on today's engine, which would start it again at t = 0 {lost}:" \
            in msg.text and "Stop" not in msg.text, msg.text
    quiet = simulate(project, "case-city", None, lambda: [])
    assert quiet.engine == "today" and "[NEW-FAILED]" in quiet.messages[0].text
    for progress in (None, lambda pct: None):
        ai = Engine.simulate(project, "case-city", max_seconds=120.0, cancel=threading.Event(),
                             progress=progress)
        assert ai.engine == "today" and ai.channels, [m.text for m in ai.messages]
        assert ai.messages[0].text.startswith("The new engine could not run this case [NEW-FAILED]")


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
    assert refused.messages[0].text.startswith("Not replayed [REPLAY-TODAY-LIVE-EDITS]: ")


@pytest.mark.parametrize("block", ["lookup", "pid"])
def test_a_live_edit_of_a_sample_time_over_the_solver_step_is_for_the_next_run(block):
    """Today's engine holds a PID or Lookup block's output between samples
    from a Sample Time edit on; the new engine runs it at every step, so
    the edit is not made live: the result lists it with NEW-LIVE-NEXT-RUN
    (the next run is NEW-SAMPLED, on today's engine). One within the step is
    taken as made."""
    from test_new_engine_evidence import _sampled
    project = _sampled(block, 0.0)
    live = _Live(at=20.0, then=[_set("blk", "sample_time_s", 5.0)])
    result = simulate(project, "case-city", live.emit, live.control)
    assert result.engine == "new" and result.status == "success"
    (edit,) = result.liveEdits
    assert (edit.key, edit.value) == ("sample_time_s", 5.0)
    assert edit.note and "[NEW-LIVE-NEXT-RUN]" in edit.note and "holds the output" in edit.note
    small = _Live(at=20.0, then=[_set("blk", "sample_time_s", 0.005)])
    (taken,) = simulate(project, "case-city", small.emit, small.control).liveEdits
    assert taken.note is None


def _branch_table(policy):
    """The sweep's fault: the BEV's city case with its E-Motor's drag table
    cut at the high end of its speed axis, outside = ``policy``; run live."""
    import sys
    from pathlib import Path
    sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
    from benchmarks.back_to_back import sweep
    raw, case_id = sweep.fault_project(f"table:bev-car/case-city-live:el-motor.drag_torque:0:hi:half:{policy}")
    next(c for c in raw["cases"] if c["id"] == case_id)["realtimeFactor"] = 0  # (no pace)
    from app.schemas import Project
    return Project.model_validate(raw), case_id


def test_a_live_run_with_an_error_axis_one_branch_reads_runs_on_today_s_engine(monkeypatch):
    """The E-Motor's drag table is read by today's engine only while the
    motor is unpowered, so with its axis set to Error today's run succeeds;
    the new engine stops at its first read past the data (24 s into the city
    case), and a live run that has shown points is not run again on today's
    engine. So a live run of it goes to today's engine up front, with
    NEW-LIVE-ERROR-AXIS (before: failed, NEW-FAILED, after 24 s). With
    Clamp it stays on the new engine, and a run with no view is not
    refused."""
    project, case_id = _branch_table("error")
    shown = _Live()
    refused = simulate(project, case_id, shown.emit, shown.control)
    assert refused.messages[0].text.startswith("Not run [NEW-LIVE-ERROR-AXIS]"), refused.messages[0].text
    assert not shown.stream
    monkeypatch.delenv(new_engine.STRICT_VAR)
    live = _Live()
    result = simulate(project, case_id, live.emit, live.control)
    assert result.engine == "today" and result.status == "success", [m.text for m in result.messages]
    assert "[NEW-LIVE-ERROR-AXIS]" in result.messages[0].text
    assert not [m for m in result.messages if m.level in ("warning", "error")]
    clamp, _ = _branch_table("clamp")
    assert simulate(clamp, case_id, _Live().emit, _Live().control).engine == "new"
    monkeypatch.setenv(new_engine.STRICT_VAR, "1")
    assert not simulate(project, case_id).messages[0].text.startswith("Not run [NEW-LIVE-ERROR-AXIS]")
