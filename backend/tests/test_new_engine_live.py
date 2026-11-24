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
from helpers import todays
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


@pytest.mark.parametrize("end_distance", [None, 300.0])
def test_a_live_performance_run_streams_every_point_of_its_result(end_distance):
    """A performance test (the BEV's step to 100 km/h), alone and ending at a
    distance (the engine runs it to its finish first, then again, and the
    view is shown the points of the second run only)."""
    project = load_example("bev-car")
    case = _case(project, "case-wltc")
    case.kind, case.duration, case.endDistance = "performance", 40.0, end_distance
    case.parameterOverrides = {"el-task": {"cycle": "", "profile": "0:100; 40:100"}}
    live = _Live()
    result = simulate(project, "case-wltc", live.emit, live.control)
    assert result.messages[0].text.startswith("Run on the new engine"), result.messages[0].text
    assert result.status == "success" and result.engine == "new", result.messages[0].text
    _streamed_is_the_result(live.steps(), result)
    assert len(live.steps()) == (41 if end_distance is None else 15)
    if end_distance is None:  # (a run that ends at its distance shows its share of the duration, as an acceleration test's does)
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
    monkeypatch.setenv(new_engine.ENV_VAR, engine)


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
        proj = load_example("fs-electric")
        result = simulate(todays(proj) if engine == "today" else proj, "case-accel-75m", live.emit,
                          live.control)
        assert result.status == "cancelled", (engine, [m.text for m in result.messages])
        said[engine] = ({s.key: (s.passed, s.notValid) for s in result.summary
                         if s.key.startswith("accel")},
                        sorted(m.level for m in result.messages if m.level != "info"))
    assert said["new"] == said["today"] == ({}, [])


def test_a_live_run_the_new_engine_stops_fails_with_its_code(monkeypatch):
    """A live run the new engine stops after it has streamed points fails
    with the engine's code: the points shown stay (never streamed again from
    t = 0), and nothing runs on today's engine; the same for a run with no
    view (a study's point) and the AI tools' run (before the switch each of
    these ran again on today's engine, NEW-LIVE-RESTART)."""
    from app.ai.engine import Engine
    from app.solver import core

    def no_today(*args, **kwargs):
        raise AssertionError("a case ran on today's engine")

    monkeypatch.delenv(new_engine.ENV_VAR, raising=False)
    monkeypatch.setattr(core, "simulate_today", no_today)
    project = load_example("bev-car")  # (its battery's resistance x100: the run stops at ~321 s)
    _case(project, "case-city").parameterOverrides.setdefault("el-battery", {})[
        "internal_resistance_ohm"] = 8.0
    for then in ([], [_set("el-vehicle", "cd", 0.3)]):
        live = _Live(at=1e9 if not then else 100.0, then=then)
        result = simulate(project, "case-city", live.emit, live.control)
        times = [m["t"] for m in live.steps()]
        assert times == sorted(set(times)) and 300.0 < times[-1] < 600.0  # (never from 0 again)
        assert "restart" not in [m["type"] for m in live.stream]
        assert result.status == "failed" and result.engine is None
        (msg,) = result.messages
        assert msg.text.startswith("Not run [NEW-FAILED]: ") and "RUN-INTEGRATOR" in msg.text, msg.text
        assert "today's engine" not in msg.text
    quiet = simulate(project, "case-city", None, lambda: [])
    assert quiet.status == "failed" and quiet.messages[0].text.startswith("Not run [NEW-FAILED]: ")
    for progress in (None, lambda pct: None):
        ai = Engine.simulate(project, "case-city", max_seconds=120.0, cancel=threading.Event(),
                             progress=progress)
        assert ai.status == "failed" and not ai.channels, [m.text for m in ai.messages]
        assert ai.messages[0].text.startswith("Not run [NEW-FAILED]: ")


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

    monkeypatch.setenv(new_engine.ENV_VAR, "today")
    today = simulate(project, "case-city")
    assert (today.engine, today.engineVersion) == ("today", VERSION)
    assert set(today.solverOptions) == {"timeStep", "solverStep", "outputEvery"}
    monkeypatch.setenv(new_engine.ENV_VAR, "new")  # (the snapshot's engine, not the variable's)
    snap = RunSnapshot.of(project, _case(project, "case-city"), today, VERSION)
    assert _sha(replay(snap)) == _sha(today)
    edited = snap.model_copy(update={"liveEdits": [LiveEdit(t=1.0, elementId="el-vehicle",
                                                            key="cd", value=0.3)]})
    refused = replay(edited)
    assert refused.status == "failed" and "live edits" in refused.messages[0].text
    assert refused.messages[0].text.startswith("Not replayed [REPLAY-TODAY-LIVE-EDITS]: ")


@pytest.mark.parametrize("block", ["lookup", "pid"])
def test_a_live_edit_of_a_sample_time_that_changes_its_samples_is_for_the_next_run(block):
    """The new engine samples a PID or a Lookup with a Sample Time at the
    times the run started with: a live edit that changes when it samples
    (0 to 5 s) is not made, the result lists it with NEW-LIVE-NEXT-RUN; one
    that does not (0 to 5 ms, within today's 10 ms step, where both engines
    run it at every step) is taken. (Before: an edit over the step was for
    the next run on today's engine, NEW-SAMPLED, and a live run with a PID
    went to today's engine before it started, NEW-LIVE-PID.)"""
    from test_new_engine_evidence import _sampled
    project = _sampled(block, 0.0)
    live = _Live(at=20.0, then=[_set("blk", "sample_time_s", 5.0)])
    result = simulate(project, "case-city", live.emit, live.control)
    assert result.engine == "new" and result.status == "success", [m.text for m in result.messages]
    (edit,) = result.liveEdits
    assert (edit.key, edit.value) == ("sample_time_s", 5.0)
    assert edit.note and "[NEW-LIVE-NEXT-RUN]" in edit.note and "Sample Time" in edit.note
    small = _Live(at=20.0, then=[_set("blk", "sample_time_s", 0.005)])
    (taken,) = simulate(project, "case-city", small.emit, small.control).liveEdits
    assert taken.note is None


def test_a_live_edit_of_a_pid_gain_reaches_its_samples():
    """A PID's gains reach the block that runs its law as inputs read at
    each sample: a live edit of its P gain changes its output from the next
    output time on (its output is P times the error, I and D 0, the
    setpoint 0)."""
    from test_new_engine_evidence import _out, _sampled
    project = _sampled("pid", 0.0)
    live = _Live(at=20.0, then=[_set("blk", "kp", 0.02)])
    result = simulate(project, "case-city", live.emit, live.control)
    (edit,) = result.liveEdits
    assert edit.note is None, edit.note
    out = _out(result)
    speed = {round(p["t"], 6): p["value"] for p in next(
        c for c in result.channels if c.elementId == "el-vehicle" and c.portId == "sig_speed").timeSeries}
    early = [t for t in out if 1.0 < t < 19.0]
    late = [t for t in out if t > edit.t + 1.0]
    assert early and all(abs(out[t] + 0.01 * speed[t]) <= 2e-3 * max(1.0, abs(out[t])) for t in early)
    assert late and all(abs(out[t] + 0.02 * speed[t]) <= 2e-3 * max(1.0, abs(out[t])) for t in late)


def _fault(fid):
    """A fault of the sweep (benchmarks/back_to_back/sweep.py), with no pace."""
    import sys
    from pathlib import Path
    sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
    from benchmarks.back_to_back import sweep
    raw, case_id = sweep.fault_project(fid)
    next(c for c in raw["cases"] if c["id"] == case_id)["realtimeFactor"] = 0  # (no pace)
    from app.schemas import Project
    return Project.model_validate(raw), case_id


def _branch_table(policy):
    """The sweep's fault: the BEV's city case with its E-Motor's drag table
    cut at the high end of its speed axis, outside = ``policy``; run live."""
    return _fault(f"table:bev-car/case-city-live:el-motor.drag_torque:0:hi:half:{policy}")


def test_a_live_run_with_an_error_axis_one_branch_reads_runs_on_the_new_engine(monkeypatch):
    """The E-Motor's drag table is read by today's engine only while the
    motor is unpowered, so with its axis set to Error today's run succeeds.
    The new engine's read of it while the motor is powered sits in the
    branch of its torque that is not taken then, and an Error axis stops
    the run only where its read's branch is taken: the live run succeeds on
    the new engine (before: it went to today's engine up front,
    NEW-LIVE-ERROR-AXIS; before that it failed after 24 s, NEW-FAILED)."""
    project, case_id = _branch_table("error")
    live = _Live()
    result = simulate(project, case_id, live.emit, live.control)
    assert result.engine == "new" and result.status == "success", [m.text for m in result.messages]
    assert live.steps()
    monkeypatch.setenv(new_engine.ENV_VAR, "today")
    assert simulate(project, case_id).status == "success"


def test_a_viewed_run_of_an_example_runs_once_and_reads_a_stop_from_its_start(monkeypatch):
    """The example BEV's E-Motor has axes set to Error on its full-load
    torque and its loss map: a run with a view, as every run the app
    starts, runs once on the new engine, and a Stop
    sent 0.1 s into it ends it within the live run's mark (STREAM S2: p95 at
    most 0.25 s, on Windows CI in tests/test_stop_latency.py). Before: the
    case first ran in full with no view and read no Stop (the Stop returned
    1.4 s later on WLTC)."""
    project = load_example("bev-car")
    _case(project, "case-wltc").realtimeFactor = 0
    real, calls = new_engine.run_new, []

    def spy(project, case_id, emit=None, control=None, *a, **k):
        calls.append(emit is not None)
        return real(project, case_id, emit, control, *a, **k)
    monkeypatch.setattr(new_engine, "run_new", spy)
    asked = []
    timer = threading.Timer(0.1, lambda: asked.append(time.perf_counter()))
    timer.start()
    result = simulate(project, "case-wltc", lambda m: None, lambda: [{"type": "cancel"}] if asked else [])
    waited = time.perf_counter() - asked[0]
    timer.cancel()
    assert result.engine == "new" and result.status == "cancelled", [m.text for m in result.messages]
    assert calls == [True]
    assert waited <= 0.25, waited


@pytest.mark.parametrize("fid", [
    "table:bev-car/case-city-live:el-motor.full_load_torque:1:lo:edge:error",
    "table:bev-car/case-city-live:el-motor.power_loss:0:hi:edge:error",
    "table:bev-car/case-city-live:el-motor.power_loss:1:hi:edge:error",
    "table:bev-car/case-city-live:el-motor.power_loss:1:lo:edge:error",
    "table:bev-car/case-city-live:el-motor.full_load_torque:0:hi:edge:error",  # (its voltage)
    "table:bev-car/case-city-live:el-motor.drag_torque:0:hi:edge:linear",  # (the loss map's torque: Error)
    "table:hybrid-car/case-mixed-live:el-engine.fuel_map:0:hi:edge:error",
    "table:hybrid-car/case-mixed-live:el-engine.fuel_map:1:hi:edge:error",
    "table:hybrid-car/case-mixed-live:el-engine.full_load_torque:0:lo:edge:error",
])
def test_a_live_run_with_an_error_axis_cut_short_ends_as_today_s(fid, monkeypatch):
    """A part's table cut short of what the part reaches, an axis on Error
    (the faults a live run of went to today's engine before it started,
    NEW-LIVE-ERROR-AXIS, until the new engine stopped only where a read's
    branch is used): a live run on the new engine ends as today's does, a
    success where today's succeeds, stopped at that table where today's
    fails."""
    project, case_id = _fault(fid)
    live = _Live()
    result = simulate(project, case_id, live.emit, live.control)
    texts = [m.text for m in result.messages]
    monkeypatch.setenv(new_engine.ENV_VAR, "today")
    today = simulate(project, case_id)
    if today.status == "failed":
        assert result.status == "failed" and "outside its data" in texts[0], texts
    else:
        assert result.engine == "new" and result.status == today.status, texts


@pytest.mark.parametrize("fid", ["overspeed:bev-car/case-city-live:down", "overspeed:bev-car/case-city:down"])
def test_a_motor_driven_past_its_maximum_speed_is_not_stopped_by_a_table_that_ends_there(fid):
    """The example BEV with its Final Drive ratio x3 on a 6 % downhill grade
    drives the motor past its 16,000 1/min: today's engine warns (over
    speed); the new engine read the full-load and loss maps there too, past
    their data (axes on Error), and stopped (NEW-FAILED, RUN-TABLE) after
    showing points. It now reads them at the speed held to the maximum
    (above it the inverter is off), so it warns the same, with a view."""
    project, case_id = _fault(fid)
    result = simulate(project, case_id, lambda m: None, lambda: [])
    assert result.engine == "new" and result.status == "warning", (result.status, [m.text for m in result.messages])
    assert any("maximum speed" in m.text for m in result.messages)


@pytest.mark.parametrize("fid", [
    # (today warns; the new engine stopped at 939 s, RUN-EVENT-STORM, until an E-Motor driven over
    # its maximum speed ran again only 0.1 % below it)
    "overspeed:bev-car/case-wltc:down",
    pytest.param("overspeed:hybrid-car/case-mixed:gear", marks=pytest.mark.xfail(strict=True, reason=(
        "NEW-FAILED: the new engine stops this over-speed run after showing points (the hybrid's "
        "mixed cycle with its gears x3: its Engine switches between fired and not fired at 7 s) where "
        "today's engine succeeds. A defect of the new path, open."))),
])
def test_a_run_with_a_view_of_a_motor_past_its_maximum_speed_gives_today_s_verdict(fid, monkeypatch):
    """The app's run of a motor driven to its maximum speed (the sweep's
    over-speed faults) should give today's verdict on the new engine, with
    a view as every app run has."""
    project, case_id = _fault(fid)
    live = _Live()
    result = simulate(project, case_id, live.emit, live.control)
    monkeypatch.setenv(new_engine.ENV_VAR, "today")
    today = simulate(project, case_id)
    assert today.status != "failed"
    assert result.engine == "new" and result.status == today.status, [m.text for m in result.messages]


def test_a_live_run_with_a_pid_at_its_limit_runs_on_the_new_engine(monkeypatch):
    """A PID with a Sample Time of 0 at its limit for a long stretch of the
    city case: the new engine runs today's discrete PID at today's 10 ms
    step, which does not slide along its limit as the continuous one did
    (it stopped 309 s in, RUN-EVENT-STORM): the live run succeeds on the new
    engine and its output at every point is today's, within the
    comparison's band (before: it went to today's engine up front,
    NEW-LIVE-PID; before that it failed after 310 points, NEW-FAILED)."""
    project, case_id = _fault("sampled:bev-car/case-city-live:pid:0")
    live = _Live()
    result = simulate(project, case_id, live.emit, live.control)
    assert result.engine == "new" and result.status == "success", [m.text for m in result.messages]
    assert live.steps()
    monkeypatch.setenv(new_engine.ENV_VAR, "today")
    today = simulate(project, case_id)

    def out(r):
        ch = next(c for c in r.channels if c.elementId == "x-spare" and c.portId == "sig_out")
        return {round(p["t"], 6): p["value"] for p in ch.timeSeries}
    new, old = out(result), out(today)
    assert min(new.values()) == -10.0 and min(old.values()) == -10.0  # (at its limit on both)
    band = 0.02 * (max(old.values()) - min(old.values()))
    assert max(abs(new[t] - old[t]) for t in old if t > 0) <= band
