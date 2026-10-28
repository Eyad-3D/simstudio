"""The both-engines diff's own tests: guards of the harness, not judges
(the judges are today's engine, the regulations' text, the schema).

The end-to-end test runs the new engine and needs the module
lightsim_engine; with LIGHTSIM_REQUIRE_NEW_ENGINE=1 (CI's Windows engine
job) a missing module fails it instead of skipping it (review finding 18).
"""
from __future__ import annotations

import json
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
