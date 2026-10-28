"""The both-engines diff's own tests: guards of the harness, not judges
(the judges are today's engine, the regulations' text, the schema).

The end-to-end test runs the new engine and needs the module
lightsim_engine; with LIGHTSIM_REQUIRE_NEW_ENGINE=1 (CI's Windows engine
job) a missing module fails it instead of skipping it (review finding 18).
"""
from __future__ import annotations

import json
import math
import os
from collections import deque
from fractions import Fraction as F

import pytest

from benchmarks.back_to_back import diff, report, runs, stream, traces

try:
    import lightsim_engine  # noqa: F401
    MISSING = None
except ImportError as e:
    MISSING = f"the compiled module lightsim_engine is not installed ({e})"
REQUIRED = os.environ.get("LIGHTSIM_REQUIRE_NEW_ENGINE", "").strip() == "1"
needs_engine = pytest.mark.skipif(MISSING is not None and not REQUIRED, reason=MISSING or "")


def test_the_acceptance_file_names_every_line_and_code():
    from app.solver import new_engine
    acc = runs.ACCEPTANCE
    assert [line["id"] for line in acc["line"]] == [
        "B2B", "STRICT", "SCHEMA", "FAULTS", "META", "ANALYTIC", "ENERGY", "STREAM", "FMU", "TEXT",
        "QUICK", "SCRIPTS", "NOSKIP", "STATUS"]
    for line in acc["line"]:
        assert all(line.get(k) for k in ("what", "cases", "measure", "pass_mark", "judge")), line["id"]
    assert set(acc["codes"]) == set(new_engine.CODES)
    for item in acc["item"]:
        assert item["expect"] == "run" or item["expect"].removeprefix("refuse:") in acc["codes"]
    assert len(acc["item"]) == 24
    assert len(runs.fault_ids()) == 160 + 27 * 4
    cases, faults = runs.meta_set()
    assert len(cases) == 3 and len(faults) == 4


# ---- ANALYTIC ---------------------------------------------------------------------

def test_the_exact_time_outside_by_hand():
    rows = {(r["trace"], r["band"]): r for r in traces.measure()}
    # 50 km/h held; the speed rises at 1 km/h/s from t = 10 to 56 km/h, holds,
    # falls back from t = 20: above 50 + 3.218688 from 13.218688 s to 22.781312 s
    assert rows[("plateau excursion", "epa")]["exact"] == str(F("9.562624"))
    assert rows[("plateau excursion", "wltp")]["exact"] == "12"  # above 52 from 12 s to 24 s


def _sampled_outside(target, speed, tol, h=F(1, 200)) -> float:
    """The time outside by brute force: both traces sampled every h, the
    band from the target's samples within 1 s (a sliding window)."""
    t0, t1 = target[0][0], target[-1][0]
    n = int((t1 - t0) / h)
    ts = [t0 + k * h for k in range(n + 1)]
    tgt = [float(traces.at(target, t)) for t in ts]
    w = int(traces.WINDOW / h)
    lo, hi, out = deque(), deque(), 0.0
    j = -1
    for i in range(n + 1):
        while j + 1 <= min(n, i + w):
            j += 1
            while lo and tgt[lo[-1]] >= tgt[j]:
                lo.pop()
            lo.append(j)
            while hi and tgt[hi[-1]] <= tgt[j]:
                hi.pop()
            hi.append(j)
        for dq in (lo, hi):
            while dq[0] < i - w:
                dq.popleft()
        v = float(traces.at(speed, ts[i]))
        if v > tgt[hi[0]] + float(tol) or v < tgt[lo[0]] - float(tol):
            out += float(h)
    return out


@pytest.mark.parametrize("trace", traces.TRACES, ids=[t[0] for t in traces.TRACES])
def test_the_exact_time_outside_agrees_with_a_fine_sampling(trace):
    _, target, speed = trace
    for tol in traces.BANDS.values():
        assert float(traces.exact_outside(target, speed, tol)) == pytest.approx(
            _sampled_outside(target, speed, tol), abs=0.02)


# ---- the diff ------------------------------------------------------------------------

def _result(value=10.0, channel=(0.0, 1.0, 2.0), **more):
    res = {"caseId": "c", "status": "success", "messages": [{"level": "info", "text": "ran 3 steps"}],
           "channels": [{"elementId": "el", "portId": "p", "label": "El · P", "unit": "kW",
                         "timeSeries": [{"t": float(k), "value": v} for k, v in enumerate(channel)]}],
           "summary": [{"key": "energy_kwh", "label": "Energy", "value": value, "unit": "kWh"}],
           "partEnergy": [], "energy": None, "duty": [], "limits": None, "references": []}
    res.update(more)
    return res


def test_identical_results_differ_nowhere():
    assert diff.compare("x", _result(), _result(), _result()) == []


def test_a_figure_is_outside_beyond_its_band():
    normal, fine = _result(10.0), _result(10.01)  # band max(2 x 0.01, 0.1 % of 10.01)
    inside = diff.compare("x", normal, fine, _result(10.025))
    outside = diff.compare("x", normal, fine, _result(10.06))
    assert inside == []
    assert [(d["field"], d["what"]) for d in outside] == [("summary.value", "value")]
    assert outside[0]["x"] == pytest.approx(0.05 / 0.02)


def test_messages_compare_without_their_numbers():
    other = _result(messages=[{"level": "info", "text": "ran 7 steps"}])
    assert diff.compare("x", _result(), _result(), other) == []


def test_each_difference_takes_its_class():
    part = {"elementId": "el", "label": "El", "part": "motor.emotor", "energyIn": 1.0, "energyOut": 0.9,
            "losses": 0.1, "stored": 0.0}
    today = _result(partEnergy=[part])
    new = _result(messages=[{"level": "info", "text": "ran 3 steps"},
                            {"level": "info", "text": "Run on the new engine (LIGHTSIM_ENGINE=new)."},
                            {"level": "warning", "text": "Something else."}])
    ds = diff.compare("x", today, today, new)
    diff.classify(ds, {})
    got = {(d["field"], d["what"]): (d["cls"], d["rule"]) for d in ds}
    assert got[("partEnergy", "missing")] == ("a", "new-path-lacks-reports")
    assert got[("messages", "extra")] in {("c", "engine-note"), ("unclassified", None)}
    assert sorted(d["cls"] for d in ds) == ["a", "c", "unclassified"]


def test_where_a_difference_sits():
    today = [float(k) for k in range(101)]
    new = list(today)
    new[40] += 50.0  # one point far off
    got = diff.where(today, today, new, band=1.0, times=today)
    assert got["points"] == 1 and got["worst_t"][0] == 40.0
    assert got["above"] == pytest.approx(1 / 100)


def test_rules_that_read_the_runs():
    note = "v of 'A' (# V) and v of 'B' (# V) carry different start guesses."
    ds = [diff.Diff(item="x", field="messages", key=f"warning: {note}", what="extra", x=None,
                    today=0, new=1),
          diff.Diff(item="x", field="status", key="", what="text", x=None, today="success", new="warning"),
          diff.Diff(item="x", field="times", key="", what="grid", x=None, today=401, new=401)]
    diff.classify(ds, {"x": {"notes": {note}, "status_without_notes": "success",
                             "line_end": {"today": 75.306, "new": 75.3000007, "line": 75.3}}})
    assert [(d["cls"], d["rule"]) for d in ds] == [
        ("a", "preparation-note-as-warning"), ("a", "preparation-note-status"),
        ("b", "acceleration-ends-at-the-line")]
    diff.classify(ds, {"x": {"line_end": {"today": 75.3001, "new": 75.3005, "line": 75.3}}})
    assert [d["cls"] for d in ds] == ["unclassified"] * 3


def _channel_result(values, stats=None):
    ch = {"elementId": "el-vehicle", "portId": "sig_speed", "label": "Vehicle · Speed", "unit": "km/h",
          "timeSeries": [{"t": float(k), "value": v} for k, v in enumerate(values)]}
    if stats is not None:
        ch.update(min=stats, max=stats, mean=stats)
    return _result(channels=[ch], summary=[], messages=[])


def test_a_new_run_without_its_points_differs():
    # the path writes None for not a number (new_engine._number): today's
    # finite points the new run lacks are a difference, not skipped
    today = [float(k) for k in range(100)]
    for broken in (today[:50] + [None] * 50, [None] * 100, today[:50] + [math.nan] * 50):
        ds = diff.compare("x", _channel_result(today, today), _channel_result(today, today),
                          _channel_result(broken, broken))
        got = {(d["field"], d["what"]) for d in ds}
        assert ("channels.values", "not finite") in got, broken[-1]
        assert {("channels.min", "not finite"), ("channels.mean", "not finite")} <= got
        first = next(d for d in ds if d["field"] == "channels.values")
        assert first["first_t"] == (1.0 if broken[1] is None else 50.0)
    # a gap today's own runs have too is no difference
    gap = today[:50] + [None] * 50
    assert diff.compare("x", _channel_result(gap), _channel_result(gap), _channel_result(gap)) == []


def _models(model, seen=None) -> dict:
    """Every pydantic model a SimResult is made of, by name."""
    import typing

    from pydantic import BaseModel
    seen = {} if seen is None else seen
    seen[model.__name__] = model

    def types(a):
        yield a
        for x in typing.get_args(a):
            yield from types(x)
    for f in model.model_fields.values():
        for t in types(f.annotation):
            if isinstance(t, type) and issubclass(t, BaseModel) and t.__name__ not in seen:
                _models(t, seen)
    return seen


def test_the_diff_says_how_it_compares_every_field_of_every_model():
    # a field SimResult gains, or a model it gains, fails here until the diff compares it
    from app.schemas import SimResult
    models = _models(SimResult)
    assert {k: set(m.model_fields) for k, m in models.items()} == \
        {k: set(v) for k, v in diff.FIELDS.items()}


FULL = {  # every field of every model filled in
    "caseId": "c", "status": "warning",
    "messages": [{"level": "warning", "text": "Cycle not followed for 12 s."}],
    "channels": [{"elementId": "el-m", "portId": "p", "label": "Motor · P", "unit": "kW",
                  "timeSeries": [{"t": float(k), "value": 1.0 + k} for k in range(4)],
                  "min": [0.5 + k for k in range(4)], "max": [1.5 + k for k in range(4)],
                  "mean": [1.0 + k for k in range(4)]}],
    "summary": [{"key": "time_s", "label": "Time", "value": 4.0, "unit": "s", "notValid": "estimates",
                 "limit": 5.0, "passed": True}],
    "partEnergy": [{"elementId": "el-m", "label": "Motor", "part": "motor.emotor", "energyIn": 1.0,
                    "energyOut": 0.9, "losses": 0.1, "stored": 0.0, "energyInReverse": 0.2,
                    "peakPower": 30.0, "meanPower": 5.0, "rmsPower": 8.0, "terms": {"copper": 0.05}}],
    "energy": {"parts": [{"elementId": "el-m", "label": "Motor", "kind": "motor.emotor", "inKWh": 1.0,
                          "outKWh": 0.9, "lostKWh": 0.1, "storedKWh": 0.0, "lostPct": 10.0}],
               "sources": [{"label": "Battery", "kWh": 1.0, "group": "source", "elementId": "el-b"}],
               "sinks": [{"label": "Road", "kWh": 0.8, "group": "road", "elementId": "el-v"}],
               "sourceKWh": 1.0, "remainderKWh": 0.01, "remainderPct": 1.0, "balanceErrorPct": 0.02},
    "duty": [{"elementId": "el-m", "label": "Motor", "kind": "motor.emotor",
              "rows": [{"quantity": "Torque", "unit": "N·m", "max": 1.0, "min": 0.0, "mean": 0.5,
                        "rms": 0.6}]}],
    "limits": {"states": ["free", "torque"], "tEnd": 3.0,
               "lanes": [{"label": "Motor", "elementIds": ["el-m"], "changes": [[0.0, 0.0], [1.0, 1.0]],
                          "seconds": {"free": 1.0, "torque": 2.0}}]},
    "references": [{"label": "Range", "value": 400.0, "reference": 410.0, "unit": "km", "difference": -10.0,
                    "differencePct": -2.4, "tolerance": 5.0, "grade": "within", "source": "EPA",
                    "automatic": True, "bound": "two-sided", "note": "a note"}],
}


def _leaves(x, path=()):
    if isinstance(x, dict):
        for k, v in x.items():
            yield from _leaves(v, path + (k,))
    elif isinstance(x, list):
        for k, v in enumerate(x):
            yield from _leaves(v, path + (k,))
    else:
        yield path, x


def test_a_change_in_any_field_is_a_difference():
    import copy

    from app.schemas import SimResult
    SimResult.model_validate(FULL)
    leaves = list(_leaves(FULL))
    assert all(v is not None for _, v in leaves)
    assert diff.compare("x", FULL, FULL, FULL) == []
    for path, value in leaves:
        if path[0] == "channels" and 0 in path[2:] and "t" not in path:
            continue  # a channel's first point is not compared (DESIGN.md 12.3)
        new = copy.deepcopy(FULL)
        at = new
        for k in path[:-1]:
            at = at[k]
        at[path[-1]] = (not value if isinstance(value, bool) else value * 3 + 7
                        if isinstance(value, (int, float)) else value + "x")
        assert diff.compare("x", FULL, FULL, new), path


def _write(out, spec, **rec):
    p = runs.job_path(out, spec)
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(json.dumps({"spec": spec, "wall_s": 1.0, **rec}), encoding="utf-8")


def test_an_item_without_its_reference_runs_is_not_judged(tmp_path, monkeypatch):
    item = {"id": "fs-electric/case-accel-75m", "expect": "run"}
    monkeypatch.setattr(runs, "items", lambda: [item])
    res = {"result": _result(caseId="case-accel-75m")}
    for fine in ({"crashed": "timeout"}, None):  # run_all's record of a timeout; a job not run yet
        out = tmp_path / str(bool(fine))
        for setting, rec in (("today-normal", res), ("today-fine", fine), ("new", res)):
            if rec is not None:
                _write(out, {"kind": "item", "id": item["id"], "setting": setting}, **rec)
        b = report.b2b(out)
        assert b["state"] == "fail" and b["missing_reference_run"] == [item["id"]]
        assert b["items"][0]["reference"] is False


def test_all_runs_every_part_the_baseline_ran():
    from benchmarks.back_to_back.__main__ import parser
    parts = parser().parse_args(["all", "OUT"]).parts
    assert len(runs.specs(set(parts.split(",")))) == 631  # the baseline's jobs, the sine probe's two among them


def test_a_record_another_code_made_is_run_again(tmp_path):
    now = {"commit": "b", "dirty": None}
    jobs = [{"kind": "item", "id": f"x/{k}", "setting": "new"} for k in range(3)]
    _write(tmp_path, jobs[1], stamp={"commit": "a", "dirty": None})
    _write(tmp_path, jobs[2], stamp=now)
    assert runs.todo(tmp_path, jobs, now) == jobs[:2]
    assert runs.stamp()["sha256"]["backend/app/solver/verdict.py"] is not None


def test_the_mutmut_copy_is_made_afresh_for_other_code(tmp_path):
    from benchmarks.back_to_back import mutation
    old = tmp_path / "mutation" / "backend" / "app" / "solver" / "verdict.py"
    old.parent.mkdir(parents=True)
    old.write_text("# the verdict.py of an earlier commit\n")
    work = mutation.prepare(tmp_path)
    assert old.read_bytes() == (runs.BACKEND / "app" / "solver" / "verdict.py").read_bytes()
    (work / "progress").write_text("mutmut's")  # kept while the code is the same: it resumes
    mutation.prepare(tmp_path)
    assert (work / "progress").exists()


def test_the_report_lists_the_code_that_made_what_it_reads(tmp_path):
    s = {"commit": "abcdef0123456789", "dirty": None, "lightsim_engine": "0.1.0",
         "lightsim_engine_sha256": "f" * 64}
    _write(tmp_path, {"kind": "item", "id": "a/b", "setting": "new"}, stamp=s)
    _write(tmp_path, {"kind": "item", "id": "a/c", "setting": "new"})
    (tmp_path / "stop.json").write_text(json.dumps({"stamp": s}))
    got = {json.dumps(m["stamp"]): m for m in report.stamps(tmp_path)}
    assert got[json.dumps(s)]["records"] == 1 and got[json.dumps(s)]["probes"] == ["stop.json"]
    assert got['"none"']["records"] == 1
    assert "commit abcdef0123" in report._stamp_text(s)


def test_s1_needs_every_point_with_every_channel(tmp_path):
    from types import SimpleNamespace
    ts = [{"t": float(k), "value": 2.0 * k} for k in range(10)]
    result = SimpleNamespace(channels=[SimpleNamespace(elementId="el", portId="p", timeSeries=ts)])
    full = [{"type": "step", "t": p["t"], "values": {"el:p": p["value"]}} for p in ts]
    assert runs.streamed(full, result)["unequal"] == 0
    empty = [{"type": "step", "t": p["t"], "values": {}} for p in ts]  # progress only
    assert runs.streamed(empty, result)["unequal"] == 10
    for points, state in ((1, "fail"), (600, "pass")):
        for iid in ("bev-car/case-city-live", "hybrid-car/case-mixed-live"):
            _write(tmp_path, {"kind": "item", "id": iid, "setting": "new"}, result=_result(),
                   stream={"points": points, "recorded": 600, "unequal": 0})
        assert report.stream(tmp_path)["S1"]["state"] == state


def test_the_label_is_read_from_the_message_a_run_writes(monkeypatch):
    import dataclasses

    from app.solver import verdict
    real = verdict.judge

    def judge(*a, **k):
        # "Cycle not followed": a comment that calls the rule LightSim's own changes no message
        return real(*a, **k)
    monkeypatch.setattr(verdict, "judge", judge)
    got = traces.labelled()
    assert got["found"] and not got["labelled"]

    def labels(*a, **k):
        v = real(*a, **k)
        return dataclasses.replace(v, messages=tuple(
            (lv, t + " The 1 %/2 s allowance is LightSim's own rule." if t.startswith("Cycle not") else t)
            for lv, t in v.messages))
    monkeypatch.setattr(verdict, "judge", labels)
    assert traces.labelled()["labelled"]


def test_scripts_reads_the_runner_each_run_names(tmp_path, monkeypatch):
    monkeypatch.delenv("LSIM_SCRATCH", raising=False)
    iid = "probe/hybrid-udds-single"
    b = {"items": [{"id": iid, "got": "run"}]}
    spec = {"kind": "item", "id": iid, "setting": "new"}
    for scripts, a in (({"Energy manager": "python"}, True), ({"Energy manager": "native"}, False),
                       (None, None)):
        _write(tmp_path, spec, result=_result(), engine={} if scripts is None else {"scripts": scripts})
        got = report.scripts(b, tmp_path)
        assert got["a_worker_on_new_path"] is a
        assert got["c_owner_page"] is None  # the owner's decisions out of reach: not measured


def test_sources_and_their_fetch_check_the_pinned_copies(tmp_path, monkeypatch):
    monkeypatch.delenv("LSIM_SCRATCH", raising=False)
    monkeypatch.delenv("LSIM_JUDGE_CACHE", raising=False)
    assert {v["state"] for v in report.sources().values()} == {"not measured"}
    monkeypatch.setenv("LSIM_SCRATCH", str(tmp_path))
    assert {v["state"] for v in report.sources().values()} == {"missing"}

    class Answer:  # what a source's server sends: not the pinned copy
        def __enter__(self):
            return self

        def __exit__(self, *exc):
            return False

        def read(self):
            return b"changed upstream"
    import urllib.request
    monkeypatch.setattr(urllib.request, "urlopen", lambda *a, **k: Answer())
    assert report.fetch(tmp_path / "judge-cache") == 1
    assert {v["state"] for v in report.sources().values()} == {"missing"}  # kept aside, not used
    src = next(iter(report.SOURCES.values()))
    assert (tmp_path / "judge-cache" / (src["file"] + ".changed")).exists()


# ---- the harness's own fixes (3e9e852) ------------------------------------------------------

def test_a_study_point_stopped_before_it_ran_shows_nothing():
    assert stream._shown({"status": "cancelled", "incomplete": "stopped at t = 0 s"}) == \
        "cancelled before running"
    assert stream._shown({"status": "cancelled", "incomplete": "stopped at t = 12.3 s"}) == \
        "cancelled while running"
    assert stream._shown({"status": "success"}) == "success"


def test_meta_on_preparation_notes_alone_is_vacuous(tmp_path):
    note = "v of 'Battery' and v of 'Load' carry different start guesses."
    cases, fault_list = runs.meta_set()
    for kind, ids in (("item", cases), ("fault", fault_list)):
        for i in ids:
            for setting in ("new", *runs.META):
                _write(tmp_path, {"kind": kind, "id": i, "setting": setting},
                       result=_result(status="warning", summary=[],
                                      messages=[{"level": "warning", "text": note}]),
                       engine={"build_warnings": [{"code": "START-ALIAS-CONFLICT", "message": note}]})
    assert report.meta(tmp_path)["state"] == "vacuous"


def test_status_counts_no_verdict_while_meta_is_vacuous():
    b = {"items": [{"got": "run"}], "diffs": []}
    lines = {"META": {"state": "vacuous"}, "FAULTS": {"violations": [], "hidden": []},
             "STREAM": {"S1": {"state": "fail"}, "S2": {"state": "fail"}},
             "FMU": {"state": "fail"}, "TEXT": {"state": "not available"}, "QUICK": {"state": "not available"}}
    assert report.status(b, lines)["each"]["the verdict"] is False
    lines["META"]["state"] = "pass"
    assert report.status(b, lines)["each"]["the verdict"] is True


def test_the_fmu_probe_builds_into_an_empty_folder(tmp_path, monkeypatch):
    import sys
    import zipfile
    sys.path.insert(0, str(runs.BACKEND / "tests"))
    from fmu import build as fmu_build

    def build(target_dir):  # writes into its folder as the real one does (it needs a C compiler)
        path = target_dir / "LightSimTest.fmu"
        with zipfile.ZipFile(path, "w") as zf:
            zf.writestr("modelDescription.xml", "<fmiModelDescription/>")
        return path
    monkeypatch.setattr(fmu_build, "build", build)
    monkeypatch.setenv("LIGHTSIM_DATA_DIR", str(tmp_path / "data"))
    monkeypatch.setenv("LIGHTSIM_PROJECTS_DIR", str(tmp_path / "data" / "projects"))
    item = next(i for i in runs.items() if i["id"] == "probe/bev-fmu")
    project, _, _ = runs.item_project(item, tmp_path / "empty")
    assert any(e["componentDefId"] == "signal.fmu" for e in project["systems"][0]["elements"])


def test_mutation_testing_takes_its_tests_and_a_memory_cap():
    from benchmarks.back_to_back import mutation
    assert "source_paths = app/solver/verdict.py" in mutation.CONFIG
    for t in mutation.TESTS:
        assert f"    {t}\n" in mutation.CONFIG
    assert 0 < mutation.MEMORY <= 4_000_000_000


# ---- the report's measures ---------------------------------------------------------------

def test_a_refusal_is_read_from_the_first_message():
    refused = {"messages": [{"level": "error", "text": "Not run [NEW-LAP]: it does not run lap cases; ..."}]}
    ran = {"messages": [{"level": "info", "text": "Run on the new engine (LIGHTSIM_ENGINE=new)."}]}
    uncoded = {"messages": [{"level": "error", "text": "Not run [lap]: ..."}]}
    assert report.refusal(refused) == "NEW-LAP"
    assert report.refusal(ran) is None
    assert report.refusal(uncoded) == "uncoded"


def test_a_preparation_note_does_not_count_as_the_verdict():
    note = "v of 'Battery' and v of 'Load' carry different start guesses."
    rec = {"result": {"status": "warning", "messages": [{"level": "warning", "text": note}]},
           "engine": {"build_warnings": [{"code": "START-ALIAS-CONFLICT", "message": note}]}}
    assert report._without_notes(rec) == "success"
    rec["result"]["messages"].append({"level": "warning", "text": "Cycle not followed: ..."})
    assert report._without_notes(rec) == "warning"


def test_p95_and_a_stop_that_shows_nothing():
    assert stream.p95([0.1] * 19 + [5.0]) == 0.1
    assert stream.p95([0.1] * 18 + [5.0, 5.0]) == 5.0
    assert stream._summary([], 0, 20, 0.25)["state"] == "not measured"
    assert stream._summary([0.1] * 9, 9, 20, 0.25)["state"] == "not measured"
    assert stream._summary([0.1] * 10, 10, 20, 0.25)["state"] == "pass"


# ---- end to end ------------------------------------------------------------------------

@needs_engine
def test_both_engines_run_the_acceleration_test_and_the_diff_classes_it(tmp_path):
    jobs = [{"kind": "item", "id": "fs-electric/case-accel-75m", "setting": s}
            for s in ("today-normal", "today-fine", "new")]
    runs.run_all(tmp_path, jobs, 3)
    rec = [runs.load(tmp_path, j) for j in jobs]
    assert all("result" in r for r in rec), [r.get("crashed") for r in rec]
    assert all(r["stamp"] == runs.stamp() for r in rec)  # each record says which code made it
    assert runs.todo(tmp_path, jobs, runs.stamp()) == []
    new = rec[2]["result"]
    assert report.refusal(new) is None, new["messages"][0]
    assert new["messages"][0]["text"].startswith("Run on the new engine")
    assert rec[2]["engine"]["energy"]["throughput_J"] > 0
    ds = diff.compare("fs", rec[0]["result"], rec[1]["result"], new)
    diff.classify(ds, {})
    assert ("a", "new-path-lacks-reports") in {(d["cls"], d["rule"]) for d in ds}
    import jsonschema
    schema = json.loads((runs.REPO / "docs" / "spec" / "schemas" / "result.schema.json").read_text())
    for r in rec:
        jsonschema.Draft202012Validator(schema).validate(r["result"])
