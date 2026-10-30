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
    assert set(acc["note_codes"]) == set(new_engine.NOTE_CODES) and not set(acc["note_codes"]) & set(acc["codes"])
    for item in acc["item"]:
        assert item["expect"] == "run" or item["expect"].removeprefix("refuse:") in acc["codes"]
    assert len(acc["item"]) == 26
    assert len(runs.fault_ids()) == 160 + 27 * 4 + 5 * 4
    cases, faults = runs.meta_set()
    assert len(cases) == 5 and len(faults) == 4
    # (two of them with a checked battery limit, averaged over 0.5 s and over none)
    assert {"bev-car/case-city-checked", "bev-car/case-city-checked-w0"} <= set(cases)


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


def _said(text, level="warning"):
    return _result(messages=[{"level": level, "text": text}])


def test_a_message_pairs_by_its_text_and_compares_its_numbers():
    # the same message at a time inside its band (today's two runs 0.01 s apart): no difference
    normal, fine = _said("Battery 'B' empty at t = 3.01 s."), _said("Battery 'B' empty at t = 3.0 s.")
    assert diff.compare("x", normal, fine, _said("Battery 'B' empty at t = 3.015 s.")) == []
    ds = diff.compare("x", normal, fine, _said("Battery 'B' empty at t = 3.1 s."))
    assert [(d["field"], d["what"], d["number"], d["today"], d["new"]) for d in ds] == [
        ("messages", "value", 1, 3.0, 3.1)]
    assert ds[0]["key"] == "warning: Battery 'B' empty at t = # s."


POWER_LIMIT = ("Battery 'Accumulator' held at its Output Power Limit ({} kW at the terminals) from t = {} s "
               "— the motors get what is left after the other loads.")
HOLES = [  # (today's message, the new run's): the judge's re-check, recheck/diff_holes.py and message_numbers.py
    (POWER_LIMIT.format(80, 0.21), POWER_LIMIT.format(8, 0.21)),
    (POWER_LIMIT.format(80, 0.21), POWER_LIMIT.format(80, 2.71)),
    ("Battery 'HV Battery' reached minimum SOC (10 %) at t = 1450 s — no further discharge.",
     "Battery 'HV Battery' reached minimum SOC (10 %) at t = 45 s — no further discharge."),
    ("Cycle not followed: outside the band for 12 s of 1800 s.", "Cycle not followed: outside the band for 900 s of 1800 s."),
    ("The vehicle did not drive the cycle: 22.9 of 23.3 km covered.", "The vehicle did not drive the cycle: 0.1 of 23.3 km covered."),
    ("Power consumer '12 V Loads' cut back at t = 300 s.", "Power consumer '48 V Loads' cut back at t = 300 s."),
]


@pytest.mark.parametrize("today,new", HOLES)
def test_a_message_with_another_limit_time_figure_or_name_differs(today, new):
    ds = diff.compare("x", _said(today), _said(today), _said(new))
    assert ds, new
    diff.classify(ds, {})
    assert "c" not in {d["cls"] for d in ds} and "b" not in {d["cls"] for d in ds}


def test_a_quoted_name_keeps_its_numbers_and_an_apostrophe_opens_no_name():
    assert diff.masked("Power consumer '12 V Loads' cut back at t = 300 s.") == \
        "Power consumer '12 V Loads' cut back at t = # s."
    assert diff.masked("the tyres' Slip Stiffness (Wheel 'Wheel FL': 250000) at 3 s") == \
        "the tyres' Slip Stiffness (Wheel 'Wheel FL': #) at # s"
    assert diff.numbers("1,450 km in 2.5e3 s")[1] == [1450.0, 2500.0]
    # a comma after a number is the sentence's, not the number's
    assert diff.numbers("stored every 10, 70 result channels") == ("stored every #, # result channels", [10.0, 70.0])


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
    got = diff.where(today, new, band=1.0, times=today)
    assert got["points"] == 1 and got["worst_t"][0] == 40.0
    assert got["above"] == pytest.approx(1 / 100)


def test_rules_that_read_the_runs():
    note = "v of 'A' (# V) and v of 'B' (# V) carry different start guesses."
    ds = [diff.Diff(item="x", field="messages", key=f"warning: {note}", what="extra", x=None,
                    today=0, new=1),
          diff.Diff(item="x", field="status", key="", what="text", x=None, today="success", new="warning"),
          diff.Diff(item="x", field="times", key="", what="grid", x=None, today=401, new=401, first=400,
                    last=[4.0, 3.9994], step=0.01)]
    line = {"today": 75.306, "new": 75.3000007, "line": 75.3, "t_today": 4.0, "t_new": 3.9994}
    diff.classify(ds, {"x": {"notes": {note}, "status_without_notes": "success", "line_end": line}})
    assert [(d["cls"], d["rule"]) for d in ds] == [
        ("a", "preparation-note-as-warning"), ("a", "preparation-note-status"),
        ("b", "acceleration-ends-at-the-line")]
    diff.classify(ds, {"x": {"line_end": line | {"today": 75.3001, "new": 75.3005}}})
    assert [d["cls"] for d in ds] == ["unclassified"] * 3
    # a new run whose distance channel ends without a value: no class, and no crash
    diff.classify(ds[2:], {"x": {"line_end": line | {"new": None}}})
    assert ds[2]["cls"] == "unclassified"
    # a grid that ends inside today's last step but not where the distance channel ends, at the line
    # (the judge's fourth re-check, recheck4/accel_one_channel_grid.py: b from the distance alone)
    for other in ({"t_new": 3.991}, {"t_today": 3.99}, {"t_new": None}):
        diff.classify(ds[2:], {"x": {"line_end": line | other}})
        assert ds[2]["cls"] == "unclassified", other


def _accel_runs(out, new_times, new_end=75.3000007, others=None):
    """fs-electric's acceleration test as three records of its distance:
    today's on a 10 ms grid to t = 4 s, 75.306 m (past the line, 75.3 m),
    the new run's on ``new_times`` ending at ``new_end``; with ``others``
    (channel → the new run's times) more channels on the same grids, but
    the new run's on their own."""
    today = [0.01 * k for k in range(401)]
    for setting, times, end in (("today-normal", today, 75.306), ("today-fine", today, 75.306),
                                ("new", new_times, new_end)):
        chans = {"el-vehicle:sig_distance": times, **{k: today if setting != "new" else ts
                                                      for k, ts in (others or {}).items()}}
        res = _result(summary=[], messages=[], channels=[
            {"elementId": k.split(":")[0], "portId": k.split(":")[1], "label": k, "unit": "m",
             "timeSeries": [{"t": t, "value": end * (t / ts[-1]) ** 2} for t in ts]} for k, ts in sorted(chans.items())])
        _write(out, {"kind": "item", "id": "fs-electric/case-accel-75m", "setting": setting}, result=res)
    return today


@pytest.mark.parametrize("change", ["at the line", "x 1.5", "+ 1 s", "a point short", "last after today's",
                                    "last a step early", "a point more"])
def test_an_acceleration_grid_is_b_only_where_its_last_time_alone_differs(tmp_path, monkeypatch, change):
    # the judge's third re-check (recheck2/holes2.py): the time axis x 1.5 or + 1 s, the
    # distance at the line, was b from the last distance alone
    item = next(i for i in runs.items() if i["id"] == "fs-electric/case-accel-75m")
    monkeypatch.setattr(runs, "items", lambda: [item])
    today = [0.01 * k for k in range(401)]
    new = {"at the line": today[:-1] + [3.9994], "x 1.5": [1.5 * t for t in today[:-1]] + [3.9994],
           "+ 1 s": [t + 1.0 for t in today[:-1]] + [4.9994], "a point short": today[:-2] + [3.9894],
           "last after today's": today[:-1] + [4.0006], "last a step early": today[:-1] + [3.985],
           # (the judge's fourth re-check, recheck3/accel_count.py: b without the same-count condition)
           "a point more": today[:-1] + [3.9990, 3.9994]}[change]
    _accel_runs(tmp_path, new)
    grid = [d for d in report.b2b(tmp_path)["diffs"] if d["field"] == "times"]
    assert [d["cls"] for d in grid] == ["b" if change == "at the line" else "unclassified"], grid


@pytest.mark.parametrize("last", [3.9994, 3.991, 3.995])
def test_one_channel_off_the_runs_grid_is_told_on_its_own_and_not_at_the_line(tmp_path, monkeypatch, last):
    # the judge's fourth re-check (recheck4/accel_one_channel_grid.py): the motor's torque alone ending
    # 8 ms before the line, every other channel at it, was b from the distance channel's line
    item = next(i for i in runs.items() if i["id"] == "fs-electric/case-accel-75m")
    monkeypatch.setattr(runs, "items", lambda: [item])
    today = [0.01 * k for k in range(401)]
    at_line = today[:-1] + [3.9994]
    _accel_runs(tmp_path, at_line, others={"el-battery:sig_current": at_line,
                                           "el-motor:sig_torque": today[:-1] + [last]})
    grid = [(d["key"], d["last"][1], d["cls"]) for d in report.b2b(tmp_path)["diffs"] if d["field"] == "times"]
    # the run's grid once (key ""), and a channel off it again, on its own
    assert grid == [("", 3.9994, "b")] + ([] if last == 3.9994 else [("el-motor:sig_torque", last, "unclassified")])


def test_a_value_the_new_run_lacks_takes_only_a_rule_that_names_it(monkeypatch):
    gear = "el-diff:sig_power"
    inside = dict.fromkeys(diff.GEAR_X, 0.2) | {"holds": True}
    ctx = {"x": {"gears": {gear: inside, f"{gear}|min": inside}}}
    ds = [diff.Diff(item="x", field="channels.values", key=gear, what=w, x=x, today=None, new=None)
          for w, x in (("value", 5.0), ("not finite", math.inf))]
    ds.append(diff.Diff(item="x", field="channels.min", key=gear, what="missing", x=None, today=None, new=None))
    diff.classify(ds, ctx)
    assert [(d["what"], d["cls"]) for d in ds] == [("value", "b"), ("not finite", "unclassified"),
                                                 ("missing", "unclassified")]
    # any rule: one without a `what` explains no "not finite"; one that names it may
    for what, cls in ((None, "unclassified"), (["not finite"], "c")):
        rule = {"id": "r", "class": "c", "field": ["channels.values"]} | ({"what": what} if what else {})
        monkeypatch.setattr(diff, "RULES", [rule])
        diff.classify(ds[1:2], ctx)
        assert ds[1]["cls"] == cls
    # and a flag only today's 10 ms run raises (its status: a rule on the status without a `what`)
    flag = [diff.Diff(item="x", field="status", key="", what="10 ms only", x=None, today="failed", new="success")]
    for what, cls in ((None, "unclassified"), (["10 ms only"], "c")):
        monkeypatch.setattr(diff, "RULES", [{"id": "r", "class": "c", "field": ["status"]} | ({"what": what} if what else {})])
        diff.classify(flag, {})
        assert flag[0]["cls"] == cls


def _gear_runs(j_motor=0.045):
    """bev-car's driveline at 0.1 s output steps: its motor's speed and
    torque and the differential's carrier speed (sources), today's gear
    channels rebuilt from them by today's rule (golden/definitions.py) and
    the new engine's by its law, worked out here from blocks/driveline.rs's
    equations, with the rotor's inertia ``j_motor`` (bev-car's motor: the
    library's rotor, Torque Scale 100 %): (sources, today's, the new
    engine's)."""
    d = diff._definitions()
    chain, motors, clutches, node = d.driveline(runs.example("bev-car"))
    (fd, _, p_fd, eta_fd), (dif, _, p_dif, eta_dif) = chain  # a final drive, then the differential
    i_fd = p_fd["ratio"]
    rpm = [1000.0 + 50.0 * k for k in range(50)]
    torque = [100.0 - 3.0 * k for k in range(50)]  # driving, then braking
    src = {f"{motors[0]}:sig_speed": rpm, f"{motors[0]}:sig_torque": torque,
           f"{dif}:sig_speed_in": [r / i_fd for r in rpm]}
    law: dict = {}
    for k, (r, tq) in enumerate(zip(rpm, torque)):
        w, a = r / d.RPM, (50.0 / 0.1 / d.RPM if k else 0.0)  # the speed, the acceleration before the point
        # final drive: tau_x = flange_in.tau - inertia_in * i * der(w_out), flange_in.tau = T - J * der(w)
        tx = tq - (j_motor + p_fd["inertia_in_kgm2"]) * a
        e1 = eta_fd if tx * w >= 0 else 1 / eta_fd
        # differential: tau_x = flange_in.tau - inertia * der(w), what the final drive's output side passes on
        tx2 = i_fd * e1 * tx - (p_fd["inertia_out_kgm2"] + p_dif["inertia_kgm2"]) * a / i_fd
        e2 = eta_dif if tx2 * w >= 0 else 1 / eta_dif
        for name, v in ((f"{fd}:sig_power", tx * w / 1000), (f"{fd}:sig_losses", tx * w / 1000 * (1 - e1)),
                        (f"{dif}:sig_power", tx2 * w / i_fd / 1000),
                        (f"{dif}:sig_losses", tx2 * w / i_fd / 1000 * (1 - e2)),
                        (f"{dif}:sig_torque_a", 0.5 * p_dif["ratio"] * e2 * tx2),
                        (f"{dif}:sig_torque_b", 0.5 * p_dif["ratio"] * e2 * tx2)):
            law.setdefault(name, []).append(v)
    return src, d.rebuild(src, chain, motors, clutches, node), law


def _channels(chans: dict) -> dict:
    return _result(summary=[], messages=[], channels=[
        {"elementId": n.split(":")[0], "portId": n.split(":")[1], "label": n, "unit": "kW",
         "timeSeries": [{"t": 0.1 * k, "value": v} for k, v in enumerate(vs)]} for n, vs in chans.items()])


def test_the_new_engines_gear_law_is_rebuilt_from_its_sources():
    src, gears, law = _gear_runs()
    d = diff._definitions()
    chain, motors, clutches, node = d.driveline(runs.example("bev-car"))
    got = diff.teeth(d, src, chain, motors, clutches, node, 0.045, src["el-diff:sig_speed_in"],
                     [0.1 * k for k in range(50)])
    assert set(got) == set(law) == set(gears)
    for name in law:
        assert got[name] == pytest.approx(law[name], rel=1e-9, abs=1e-9), name
    today = _channels({**src, **gears})
    g = diff.gear_rebuild(runs.example("bev-car"), today, today, _channels({**src, **law}))
    assert all(g[n]["teeth"] < 1e-9 and g[n]["rule_new"] == g[n]["rule_today"] == 0.0 for n in law), g
    # a motor's Torque Scale scales its rotor's inertia (blocks/motor.rs: J = inertia_kgm2 x kt), so the
    # law of a motor at 150 % is the one with 1.5 times the rotor (the judge's fourth re-check: untested)
    import copy
    scaled = copy.deepcopy(runs.example("bev-car"))
    motor = next(e for s in scaled["systems"] for e in s["elements"] if e["id"] == motors[0])
    motor.setdefault("parameterOverrides", {})["torque_scale_pct"] = 150.0
    src, gears, law = _gear_runs(j_motor=0.045 * 1.5)
    today, new = _channels({**src, **gears}), _channels({**src, **law})
    assert all(x["teeth"] < 1e-9 for x in diff.gear_rebuild(scaled, today, today, new).values())
    assert max(x["teeth"] for x in diff.gear_rebuild(runs.example("bev-car"), today, today, new).values()) > 1.0
    # taken as motor.rs scale_pct takes it: not above 0 is 100 %, and at least 1 % (the judge's fourth
    # re-check, recheck4/revert_more.sh: v / 100 passed every test)
    for pct, kt in ((0.0, 1.0), (-20.0, 1.0), (0.5, 0.01)):
        motor["parameterOverrides"]["torque_scale_pct"] = pct
        src, gears, law = _gear_runs(j_motor=0.045 * kt)
        today, new = _channels({**src, **gears}), _channels({**src, **law})
        assert all(x["teeth"] < 1e-9 for x in diff.gear_rebuild(scaled, today, today, new).values()), pct


def test_the_gear_rule_leaves_out_a_channel_the_new_run_lacks():
    src, gears, _ = _gear_runs()
    today = _channels({**src, **gears})
    name = next(iter(gears))
    assert diff.gear_rebuild(runs.example("bev-car"), today, today, today)[name]["rule_new"] == 0.0
    lacks = _channels({**src, **gears, name: [gears[name][0]] + [None] * 49})
    assert name not in diff.gear_rebuild(runs.example("bev-car"), today, today, lacks)


GEAR_FAULTS = {"x 3": lambda v: 3 * v, "zero": lambda v: 0.0, "negated": lambda v: -v, "10 % off": lambda v: 1.1 * v}


@pytest.mark.parametrize("fault", list(GEAR_FAULTS))
def test_a_gear_channel_off_the_new_engines_law_is_not_today_wrong(tmp_path, monkeypatch, fault):
    # through report.b2b: the new engine's channels by its law are today's rule (b); one of them
    # three times off, zero, negated or 10 % off is not (the judge's third re-check,
    # recheck2/gear_wrong.py), nor are the others None after t = 0 (recheck/gear_nan.py)
    item = next(i for i in runs.items() if i["id"] == "bev-car/case-city")
    monkeypatch.setattr(runs, "items", lambda: [item])
    src, gears, law = _gear_runs()
    names = sorted(n for n in law if max(abs(x) for x in law[n]) > 0)  # (the lossless differential's loss is 0)
    off, gone = names[0], names[1]
    new = {**src, **{n: [GEAR_FAULTS[fault](v) for v in vs] if n == off else [vs[0]] + [None] * 49 if n == gone
                     else vs for n, vs in law.items()}}
    for setting, chans in (("today-normal", {**src, **gears}), ("today-fine", {**src, **gears}), ("new", new)):
        _write(tmp_path, {"kind": "item", "id": item["id"], "setting": setting}, result=_channels(chans))
    got = {d["key"]: d["cls"] for d in report.b2b(tmp_path)["diffs"] if d["field"] == "channels.values"}
    assert got.pop(off) == "unclassified" and got.pop(gone) == "unclassified"
    assert got and set(got.values()) == {"b"}, got


def test_a_gear_statistic_is_b_only_inside_the_inertias_term_and_holding_its_ends():
    gear, ok = "el-diff:sig_torque_a", {"rule_today": 0.0, "rule_new": 0.1, "teeth": 2.0, "inertia": 5.0,
                                        "new_rule": 5.5, "holds": True, "interval": 0.5}

    def cls(field, **g):
        ds = [diff.Diff(item="x", field=field, key=gear, what="value", x=6.0, today=None, new=None)]
        diff.classify(ds, {"x": {"gears": {gear: ok | g, f"{gear}|{field.split('.')[-1]}": ok | g}}})
        return ds[0]["cls"]
    assert cls("channels.max") == "b"
    assert cls("channels.max", holds=False) == cls("channels.max", teeth=6.5) == "unclassified"
    # and at every interval within one band of the law's range there (the judge's fourth re-check)
    assert cls("channels.min", interval=1.5) == cls("channels.max", interval=math.nan) == "unclassified"
    assert cls("channels.min", interval=None) == "unclassified"
    assert cls("channels.min", new_rule=6.5) == cls("channels.min", rule_new=1.5) == "unclassified"
    assert cls("channels.values") == cls("channels.mean") == "unclassified"  # the law rebuilt exactly: 2 bands off
    assert cls("channels.mean", teeth=0.9) == "b"
    # a rebuild that is not a number (the carrier's speed missing at a point) explains nothing
    for k in diff.GEAR_X:
        assert cls("channels.max", **{k: math.inf}) == cls("channels.mean", **({"teeth": 0.9} | {k: math.nan})) \
            == "unclassified", k
    # the new engine's minimum holds its values at both ends of each interval; one above them does not
    assert diff._holds("min", [0.0, 0.0, 1.5], [0.0, 2.0, 1.5]) and not diff._holds("min", [0.0, 0.0, 2.0], [0.0, 2.0, 1.5])
    assert diff._holds("max", [0.0, 2.0, 2.0], [0.0, 2.0, 1.5]) and not diff._holds("max", [0.0, 1.9, 2.0], [0.0, 2.0, 1.5])
    assert not diff._holds("max", [0.0, 2.0], [0.0, 2.0, 1.5]) and not diff._holds("max", [0.0, math.nan], [0.0, 1.0])
    src, gears, law = _gear_runs()
    today, new = _channels({**src, **gears}), _channels({**src, **law})
    for r in (today, new):  # each interval's maximum taken at its end: below a falling torque's start
        for c in r["channels"]:
            c["max"] = [p["value"] for p in c["timeSeries"]]
    got = diff.gear_rebuild(runs.example("bev-car"), today, today, new)
    assert got["el-diff:sig_torque_b|max"]["holds"] is False


def _ends(chans: dict) -> dict:
    """Channels (name → values) with each one's statistics over each output
    interval those of its two ends."""
    def stat(vs, pick):
        return [vs[0]] + [pick(a, b) for a, b in zip(vs, vs[1:])]
    return {n: {"values": vs, "min": stat(vs, min), "max": stat(vs, max), "mean": stat(vs, lambda a, b: (a + b) / 2)}
            for n, vs in chans.items()}


def _stat_channels(chans: dict, dt: float) -> dict:
    return _result(summary=[], messages=[], channels=[
        {"elementId": n.split(":")[0], "portId": n.split(":")[1], "label": n, "unit": "N·m",
         "timeSeries": [{"t": dt * k, "value": v} for k, v in enumerate(s["values"])],
         "min": s["min"], "max": s["max"], "mean": s["mean"]} for n, s in chans.items()])


def test_a_gear_minimum_one_interval_far_off_is_not_today_wrong(tmp_path, monkeypatch):
    # the judge's fourth re-check (recheck3/gear_spike.py): the bound on a minimum was an RMS over every
    # interval, so the WLTC differential torque minimum 161 bands off at one interval of 1801 was still b.
    # bev-car's motor at about 1.4 rad/s² under a slowly swinging torque, 1800 intervals of 0.1 s: the
    # inertias' term about 11 bands, each statistic its interval's ends' (so the new one holds them); the
    # acceleration changes from interval to interval, so a minimum taken at an interval's end is the law at
    # its neighbour's acceleration (with the interval's own only, 1.9 bands outside: not b)
    item = next(i for i in runs.items() if i["id"] == "bev-car/case-city")
    monkeypatch.setattr(runs, "items", lambda: [item])
    d = diff._definitions()
    chain, motors, clutches, node = d.driveline(runs.example("bev-car"))
    dt, dif = 0.1, chain[-1][0]
    times = [dt * k for k in range(1800)]
    rpm = [1000.0 + 13.4 * t + math.sin(2 * math.pi * t / 1.3) for t in times]
    src = {f"{motors[0]}:sig_speed": rpm, f"{motors[0]}:sig_torque": [2.0 * math.cos(2 * math.pi * t / 60) for t in times],
           f"{dif}:sig_speed_in": [r / chain[0][2]["ratio"] for r in rpm]}
    today = _ends({**src, **d.rebuild(src, chain, motors, clutches, node)})
    new = _ends({**src, **diff.teeth(d, src, chain, motors, clutches, node, 0.045, src[f"{dif}:sig_speed_in"], times)})
    gear = f"{dif}:sig_torque_a"

    def judged():
        for setting, chans in (("today-normal", today), ("today-fine", today), ("new", new)):
            _write(tmp_path, {"kind": "item", "id": item["id"], "setting": setting}, result=_stat_channels(chans, dt))
        b = report.b2b(tmp_path)
        assert b["items"][0]["ledger"]["balanced"]
        return next(df for df in b["diffs"] if df["key"] == gear and df["field"] == "channels.min")
    got = judged()
    assert got["cls"] == "b" and 5.0 < got["x"] < 20.0, got  # the law's minimum: today's rule (b)
    new[gear]["min"][900] -= 160 * got["band"]  # one interval 160 bands down: it still holds its ends
    got = judged()
    assert got["cls"] == "unclassified", got


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
        assert first["first_t"] == (0.0 if broken[0] is None else 50.0)
    # a gap today's own runs have too is no difference
    gap = today[:50] + [None] * 50
    assert diff.compare("x", _channel_result(gap), _channel_result(gap), _channel_result(gap)) == []


def test_a_point_the_new_run_lacks_at_the_start_differs():
    # the first point is out of the band (DESIGN.md 12.3), not out of the gap check: a standstill's 0/0
    today = [90.0 + 0.01 * k for k in range(100)]
    for at0 in (None, math.nan):
        ds = diff.compare("x", _channel_result(today, today), _channel_result(today, today),
                          _channel_result([at0] + today[1:], [at0] + today[1:]))
        got = {(d["field"], d["what"], d.get("first_t")) for d in ds}
        assert {("channels.values", "not finite", 0.0), ("channels.min", "not finite", 0.0)} <= got
    # another value at t = 0 is a figure of its own (the judge's fourth re-check: it was no difference),
    # today's 0 recorded before its first step (c) only where the new value is where today's run gets after it
    before = [0.0] + today[1:]  # today's first point before its first step, its signal still 0
    for at0, cls in ((900.0, "unclassified"), (-5.0, "unclassified"), (90.0, "c"), (0.0, None)):
        ds = diff.compare("x", _channel_result(before), _channel_result(before), _channel_result([at0] + today[1:]))
        diff.classify(ds, {})
        assert [(d["field"], d["what"], d["cls"]) for d in ds] == ([("channels.values", "first point", cls)]
                                                                 if cls else []), at0


def test_a_first_point_is_c_only_within_the_channel_band_of_todays_first_two_values():
    # the judge's fourth re-check (recheck4/first_point_slack.py): 16 of baseline-5's first points lie
    # outside today's first two values, within the channel's band; no test held either bound
    today = [90.0 + 0.01 * k for k in range(100)]
    before = [0.0] + today[1:]
    band = 0.005 * diff.rms(today[1:])  # (today's two runs alike: the channel's band its floor)
    for at0, cls in ((today[1] + 0.6 * band, "c"), (today[1] + 1.4 * band, "unclassified"),
                     (-0.6 * band, "c"), (-1.4 * band, "unclassified")):
        ds = diff.compare("x", _channel_result(before), _channel_result(before), _channel_result([at0] + today[1:]))
        diff.classify(ds, {})
        assert [(d["what"], d["cls"]) for d in ds] == [("first point", cls)], at0
    # the first point's own band is the figure's, from today's 10 ms first value: 1.0 there, 0 at 1 ms,
    # makes it 2; a first value 1.5 off is inside it, and outside the floor without the 10 ms run
    normal = _channel_result([1.0] + today[1:])
    for n, got in ((normal, []), (_result(channels=[], summary=[], messages=[]), ["first point"])):
        ds = diff.compare("x", n, _channel_result(before), _channel_result([1.5] + today[1:]))
        assert [d["what"] for d in ds] == got


def test_a_list_the_new_run_gives_at_fewer_or_more_points_differs():
    # the judge's third re-check (recheck2/holes2.py): a statistic cut to its first half was
    # compared over the points it kept, and no difference found
    today = [90.0 + 0.01 * k for k in range(100)]
    for cut in (50, 2):
        new = _channel_result(today, today)
        for stat in ("min", "max", "mean"):
            new["channels"][0][stat] = today[:cut]
        got = {(d["field"], d["what"]) for d in diff.compare("x", _channel_result(today, today),
                                                            _channel_result(today, today), new)}
        for stat in ("min", "max", "mean"):
            assert {(f"channels.{stat}", "grid"), (f"channels.{stat}", "not finite")} <= got, (cut, got)
    longer = _channel_result(today, today)
    longer["channels"][0]["mean"] = today + [today[-1]]
    assert [(d["field"], d["what"]) for d in diff.compare("x", _channel_result(today, today),
                                                          _channel_result(today, today), longer)] == [
        ("channels.mean", "grid"), ("channels.mean", "extra")]  # (the point today's lacks)
    # and a values list: its times differ, and today's points past its end are points it lacks
    got = {(d["field"], d["what"]) for d in diff.compare("x", _channel_result(today), _channel_result(today),
                                                        _channel_result(today[:50]))}
    assert got == {("times", "grid"), ("channels.values", "not finite")}


def test_todays_run_summary_explains_only_that_message():
    solved = ["Case 'City Cycle' solved: 60000 steps × 0.01 s (10 sub-steps each), 601 points recorded, "
              "70 result channels.",
              "Case 'Acceleration 75 m' solved: 400 of 3000 steps × 0.01 s, ended at 75.306 m driven at t = 4 s "
              "(10 sub-steps each), 401 points recorded, 30 result channels.",
              "Case 'WLTC' solved: 1801 steps × 1 s, the last one 0.5 s (10 sub-steps each), 1802 points "
              "recorded, stored every 10, 70 result channels."]
    # a message that only begins the same way is one the new path lacks (a), not today's summary (c)
    for text, cls in [(t, "c") for t in solved] + [
            ("Case 'City Cycle' solved: but the battery ran empty at t = 300 s.", "a")]:
        ds = diff.compare("x", _said(text, "info"), _said(text, "info"), _result(messages=[]))
        diff.classify(ds, {})
        assert [(d["what"], d["cls"]) for d in ds] == [("missing", cls)], text


def test_a_not_a_number_in_a_limit_or_a_change_of_state_differs():
    import copy
    new = copy.deepcopy(FULL)
    new["summary"][0]["limit"] = math.nan
    assert [(d["field"], d["what"]) for d in diff.compare("x", FULL, FULL, new)] == [("summary.limit", "text")]
    new = copy.deepcopy(FULL)
    new["limits"]["lanes"][0]["changes"][1][0] = math.nan
    assert [(d["field"], d["what"]) for d in diff.compare("x", FULL, FULL, new)] == [
        ("limits.changes", "not finite")]
    # and on today's side (its time compared as not a number passed any step)
    assert [(d["field"], d["what"]) for d in diff.compare("x", new, new, FULL)] == [("limits.changes", "not finite")]


# every keyed list of rows in FULL, as a path, and a number in its first row to change
ROW_LISTS = [("messages",), ("channels",), ("summary",), ("partEnergy",), ("energy", "parts"),
             ("energy", "sources"), ("energy", "sinks"), ("duty",), ("duty", 0, "rows"), ("limits", "lanes"),
             ("references",)]


def _bump(x):
    """``x`` with every number in it but the times and the flags far off."""
    if isinstance(x, dict):
        return {k: v if k == "t" else _bump(v) for k, v in x.items()}
    if isinstance(x, list):
        return [_bump(v) for v in x]
    if isinstance(x, (int, float)) and not isinstance(x, bool):
        return x * 10 + 99
    return x.replace("12", "99") if isinstance(x, str) else x  # (a message's own number)


def _rows_at(res, where):
    for k in where:
        res = res[k]
    return res


@pytest.mark.parametrize("where", ROW_LISTS, ids=["/".join(map(str, w)) for w in ROW_LISTS])
def test_a_row_the_new_run_repeats_differs(where):
    import copy
    new = copy.deepcopy(FULL)
    rows = _rows_at(new, where)
    rows.append(_bump(rows[0]))  # the same key twice, the second far off
    ds = diff.compare("x", FULL, FULL, new)
    assert {d["what"] for d in ds} & {"duplicate", "extra"} and ds.ledger.totals()["balanced"], where
    # the same key twice on both sides pairs in order: the second row is compared too
    today = copy.deepcopy(new)
    _rows_at(today, where)[1] = copy.deepcopy(rows[0])
    ds = diff.compare("x", today, today, new)
    assert {d["what"] for d in ds} - {"duplicate", "missing", "extra"} and ds.ledger.totals()["balanced"], where


def test_a_band_from_a_ten_ms_value_that_is_not_a_number_passes_nothing():
    normal = _result(value=math.nan)
    assert [(d["field"], d["what"]) for d in diff.compare("x", normal, _result(10.0), _result(99.0))] == [
        ("summary.value", "value")]


def test_an_error_that_is_not_a_number_fails_analytic(monkeypatch):
    rows = [{"error_s": 0.0}, {"error_s": math.nan}, {"error_s": 0.0}]
    monkeypatch.setattr(traces, "measure", lambda: rows)
    monkeypatch.setattr(traces, "labelled", lambda: {"labelled": True})
    got = report.analytic(None)
    assert got["state"] == "fail" and got["worst_error_s"] == math.inf


def test_the_schema_checks_are_kept_for_the_same_validator_only(tmp_path, monkeypatch):
    import importlib.metadata
    _write(tmp_path, {"kind": "item", "id": "a/b", "setting": "new"}, result=_result())
    report.schema(tmp_path)
    key = json.loads((tmp_path / "schema.json").read_text())["schema"]
    monkeypatch.setattr(importlib.metadata, "version", lambda name: "0.0-another")
    report.schema(tmp_path)
    assert json.loads((tmp_path / "schema.json").read_text())["schema"] != key


def test_a_duty_part_with_no_rows_is_compared():
    part = {"elementId": "el-m", "label": "Motor", "kind": "motor.emotor", "rows": []}
    ds = diff.compare("x", _result(duty=[part]), _result(duty=[part]), _result())
    assert [(d["field"], d["key"], d["what"]) for d in ds] == [("duty", "el-m", "missing")]
    ds = diff.compare("x", _result(), _result(), _result(duty=[part]))
    assert [(d["field"], d["what"]) for d in ds] == [("duty", "extra")]


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
    assert diff.compare("x", FULL, FULL, FULL).ledger.totals() == {
        "union": len(leaves), "compared": len(leaves), "sourced": 0, "ten_ms_only": 0, "reported": 0, "gaps": 0,
        "balanced": True, "unbalanced": 0, "first_unbalanced": [], "first_gaps": [], "first_ten_ms_only": []}
    for path, value in leaves:  # (a channel's first point too: a figure of its own since the fourth re-check)
        new = copy.deepcopy(FULL)
        at = new
        for k in path[:-1]:
            at = at[k]
        at[path[-1]] = (not value if isinstance(value, bool) else value * 3 + 7
                        if isinstance(value, (int, float)) else value + "x")
        ds = diff.compare("x", FULL, FULL, new)
        assert ds and ds.ledger.totals()["balanced"], path


def _ten() -> dict:
    """FULL with its channel at ten points."""
    import copy
    r = copy.deepcopy(FULL)
    r["channels"][0].update(timeSeries=[{"t": float(k), "value": 1.0 + k} for k in range(10)],
                            min=[0.5 + k for k in range(10)], max=[1.5 + k for k in range(10)],
                            mean=[1.0 + k for k in range(10)])
    return r


def _at(r, stat, k, wrong):
    c = r["channels"][0]
    if stat == "values":
        c["timeSeries"][k]["value"] = wrong(c["timeSeries"][k]["value"])
    else:
        c[stat][k] = wrong(c[stat][k])


def _point_more(r, wrong):
    r["channels"][0]["timeSeries"].append({"t": 10.0, "value": wrong(11.0)})


def _channel_more(*rs, wrong=lambda x: x):
    import copy
    for r in rs:
        c = copy.deepcopy(r["channels"][0])
        c["portId"] = "q"
        for p in c["timeSeries"]:
            p["value"] = wrong(p["value"])
        r["channels"].append(c)


def _wired_more(n, f, v, wrong=lambda x: x, **more):
    """bev-car's vehicle speed (a copy of the first channel) in all three
    runs, and in the new run only the vehicle monitor's speed input, which
    bev-car's data bus wires to it, with today's label for it, ``wrong`` on
    its values and statistics and ``more`` of its fields."""
    import copy
    for r in (n, f, v):
        r["channels"].append(copy.deepcopy(r["channels"][0]) | {"elementId": "el-vehicle", "portId": "sig_speed"})
    m = copy.deepcopy(v["channels"][-1]) | {"elementId": "el-veh-monitor", "portId": "speed",
                                            "label": report.wires(runs.example("bev-car"), "case-city")
                                            ["el-veh-monitor:speed"][1]}
    for p in m["timeSeries"]:
        p["value"] = wrong(p["value"])
    v["channels"].append(m | {s: [wrong(x) for x in m[s]] for s in ("min", "max", "mean")} | more)


def _row_more(*rs, wrong=lambda x: x):
    for r in rs:
        r["summary"].append({"key": "range_km", "label": "Range", "value": wrong(400.0), "unit": "km",
                             "notValid": None, "limit": None, "passed": None})


# (where, what plants the wrong value there, the field its difference is in): the judge's fourth
# re-check found points past the end of today's 10 ms list never compared (recheck3/accel_last_point.py,
# normal_short.py); today's 10 ms run here is one point short on the channel, as the acceleration test's is
PLANTS = {
    "the first point": (lambda n, f, v, w: _at(v, "values", 0, w), "channels.values"),
    "the last point": (lambda n, f, v, w: _at(v, "values", -1, w), "channels.values"),
    "the last point, the 10 ms run without the channel": (
        lambda n, f, v, w: (n.update(channels=[]), _at(v, "values", -1, w)), "channels.values"),
    "a point inside": (lambda n, f, v, w: _at(v, "values", 5, w), "channels.values"),
    "a point only the new run has": (lambda n, f, v, w: _point_more(v, w), "channels.values"),
    "a point only today's 1 ms run has": (lambda n, f, v, w: _point_more(f, w), "channels.values"),
    "a minimum": (lambda n, f, v, w: _at(v, "min", 5, w), "channels.min"),
    "a maximum": (lambda n, f, v, w: _at(v, "max", -1, w), "channels.max"),
    "a mean": (lambda n, f, v, w: _at(v, "mean", 0, w), "channels.mean"),
    "a channel only the new run has": (lambda n, f, v, w: _channel_more(v, wrong=w), "channels"),
    # (the judge's fourth re-check, recheck4/extra_channels.py: a monitor's input ten times off was c)
    "a channel only the new run has, wired to one today's runs have": (
        lambda n, f, v, w: _wired_more(n, f, v, wrong=w), "channels.values"),
    "a channel only today's runs have": (lambda n, f, v, w: _channel_more(n, f, wrong=w), "channels"),
    "a field only the new run has": (lambda n, f, v, w: v["channels"][0].update(note=w(1.0)), "channels.note"),
    "a field only today's runs have": (lambda n, f, v, w: [r["summary"][0].update(note=w(1.0)) for r in (n, f)],
                                       "summary.note"),
    "a figure": (lambda n, f, v, w: v["summary"][0].update(value=w(4.0)), "summary.value"),
    "a list of a row": (lambda n, f, v, w: v["limits"]["lanes"][0]["changes"][1].__setitem__(0, w(1.0)),
                        "limits.changes"),
    "a lane's state only today's runs have": (lambda n, f, v, w: v["limits"]["lanes"][0]["seconds"].pop("torque"),
                                              "limits.seconds"),
    "a number a point has besides its value": (
        lambda n, f, v, w: v["channels"][0]["timeSeries"][3].update(q=w(1.0)), "channels.points"),
    "a message": (lambda n, f, v, w: v["messages"][0].update(text=f"Cycle not followed for {w(12.0)} s."),
                  "messages"),
    "a record only the new run has": (lambda n, f, v, w: _row_more(v, wrong=w), "summary"),
    "a record only today's runs have": (lambda n, f, v, w: _row_more(n, f, wrong=w), "summary"),
}


@pytest.mark.parametrize("wrong", ["x 10", "None"])
@pytest.mark.parametrize("place", list(PLANTS))
def test_a_wrong_value_at_any_place_surfaces_and_the_ledger_balances(tmp_path, monkeypatch, place, wrong):
    # through report.b2b: every leaf of the three runs compared, reported or a gap (the ledger), and a
    # value ten times off, or the path's not a number, at each kind of place a difference not explained
    # away (only a channel the new engine records more is c, new-records-more)
    item = next(i for i in runs.items() if i["id"] == "bev-car/case-city")
    monkeypatch.setattr(runs, "items", lambda: [item])
    n, f, v = _ten(), _ten(), _ten()
    c = n["channels"][0]
    c.update(timeSeries=c["timeSeries"][:-1], min=c["min"][:-1], max=c["max"][:-1], mean=c["mean"][:-1])

    def judge():
        for setting, res in (("today-normal", n), ("today-fine", f), ("new", v)):
            _write(tmp_path, {"kind": "item", "id": item["id"], "setting": setting}, result=res)
        b = report.b2b(tmp_path)
        assert b["items"][0]["ledger"]["balanced"], b["items"][0]["ledger"]
        return b
    b = judge()
    assert b["state"] == "pass" and b["diffs"] == []
    plant, field = PLANTS[place]
    plant(n, f, v, (lambda x: 10 * x) if wrong == "x 10" else (lambda x: None))
    b = judge()
    # (a channel today's runs do not record has nothing to be judged by: c, which B2B lets pass)
    assert b["state"] == ("pass" if place == "a channel only the new run has" else "fail")
    got = [d for d in b["diffs"] if d["field"] == field]
    assert got, b["diffs"]
    assert not {d["cls"] for d in got} & ({"b"} if place == "a channel only the new run has" else {"b", "c"}), got


def test_a_channel_only_the_new_run_records_is_compared_with_the_port_wired_to_it(tmp_path, monkeypatch):
    # the vehicle monitor's speed input (wired to the vehicle's speed, which today's runs record) is
    # compared with it, every leaf "by source"; the battery's current limit (wired to nothing) stays c
    item = next(i for i in runs.items() if i["id"] == "bev-car/case-city")
    monkeypatch.setattr(runs, "items", lambda: [item])
    # (today's label for the port: core.py's "part · port", the part's label and the port's name; and a
    # connection read both ways, an output wired to every input it feeds)
    wires = report.wires(runs.example("bev-car"), "case-city")
    assert wires["el-veh-monitor:speed"] == (["el-vehicle:sig_speed"], "Vehicle Monitor · speed")
    assert wires["el-vehicle:sig_speed"] == (["el-driver:sig_speed_in", "el-veh-monitor:speed"],
                                             "Vehicle · Vehicle Speed")

    def judge(**more):
        n, f, v = _ten(), _ten(), _ten()
        _wired_more(n, f, v, **more)
        _channel_more(v)
        v["channels"][-1].update(elementId="el-battery", portId="sig_i_ch_limit")
        for setting, res in (("today-normal", n), ("today-fine", f), ("new", v)):
            _write(tmp_path, {"kind": "item", "id": item["id"], "setting": setting}, result=res)
        b = report.b2b(tmp_path)
        assert b["items"][0]["ledger"]["balanced"], b["items"][0]["ledger"]
        return b, [(d["field"], d["key"], d["what"], d["cls"], d.get("source")) for d in b["diffs"]]
    b, got = judge()
    assert got == [("channels", "el-battery:sig_i_ch_limit", "extra", "c", None)] and b["state"] == "pass"
    monitor = 4 + 10 * 2 + 3 * 10  # its ids, label and unit; each point's time and value; min, max and mean
    assert b["items"][0]["ledger"]["sourced"] == monitor
    # its label and unit against today's: the one core.py gives the port, the wired port's unit
    _, got = judge(label="Vehicle Monitor · Distance", unit="m")
    assert [g for g in got if g[1] == "el-veh-monitor:speed"] == [
        ("channels.label", "el-veh-monitor:speed", "text", "unclassified", "el-vehicle:sig_speed"),
        ("channels.unit", "el-veh-monitor:speed", "text", "unclassified", "el-vehicle:sig_speed")]
    # wired to a port today's runs do not record: nothing to compare it by; one only today's 1 ms run
    # records: lent to it alone (the band its floor)
    v = _ten() | {"channels": _ten()["channels"] + [{"elementId": "el-x", "portId": "q"}]}
    assert diff._lend(_ten(), _ten(), v, {"el-x:q": (["el-y:q"], "X · Q")})[2] == {}
    f = _ten()
    f["channels"][0].update(elementId="el-y", portId="q")
    n, f2, lent, _ = diff._lend(_result(channels=[]), f, v, {"el-x:q": (["el-y:q"], "X · Q")})
    assert lent == {"el-x:q": "el-y:q"} and n["channels"] == [] and \
        [(c["elementId"], c["portId"], c["label"]) for c in f2["channels"]] == [("el-y", "q", "Motor · P"), ("el-x", "q", "X · Q")]


def test_what_only_todays_10_ms_run_flags_is_a_difference_and_its_texts_are_counted(tmp_path, monkeypatch):
    # the judge's fourth re-check (recheck4/ten_ms_flags.py): today's 10 ms run (the app's own step) failed,
    # a row not valid and a check not passed there, a unit renamed, and no difference, the ledger unmoved
    item = next(i for i in runs.items() if i["id"] == "bev-car/case-city")
    monkeypatch.setattr(runs, "items", lambda: [item])
    n = _ten()
    n.update(status="failed")
    n["summary"][0].update(notValid="only at 10 ms", passed=False)
    n["channels"][0]["unit"] = "furlongs"
    n["channels"][0]["timeSeries"][3]["value"] += 1e-3  # (another number there is the band's, not counted)
    n["channels"][0]["timeSeries"][5]["value"] = math.nan  # (no number where today's 1 ms run has one: counted)
    n["messages"].append({"level": "warning", "text": "Only at 10 ms."})  # (a warning only it writes: a gap too)
    f = _ten()
    f.update(status="success")
    f["summary"][0].update(notValid=None)  # (FULL's warning and row not valid: here only at 10 ms)
    for r in (n, f):  # (not a number in both of today's runs: the same, not counted)
        r["channels"][0]["timeSeries"][6]["value"] = math.nan
    for setting, res in (("today-normal", n), ("today-fine", f), ("new", f)):
        _write(tmp_path, {"kind": "item", "id": item["id"], "setting": setting}, result=res)
    b = report.b2b(tmp_path)
    assert b["state"] == "fail" and [(d["field"], d["key"], d["what"], d["today"], d["cls"]) for d in b["diffs"]] == [
        ("status", "", "10 ms only", "failed", "unclassified"),
        ("summary.notValid", "time_s", "10 ms only", "only at 10 ms", "unclassified"),
        ("summary.passed", "time_s", "10 ms only", False, "unclassified"),
        ("messages", "warning: Only at # ms.", "10 ms only", "Only at 10 ms.", "unclassified")]
    ledger = b["items"][0]["ledger"]
    assert ledger["balanced"] and ledger["ten_ms_only"] == 5 and ledger["gaps"] == 2, ledger
    assert ledger["first_ten_ms_only"][:2] == ["channels/el-m:p/0/timeSeries/5/value: 10 ms nan, 1 ms 6.0, new 6.0",
                                               "channels/el-m:p/0/unit: 10 ms 'furlongs', 1 ms 'kW', new 'kW'"]
    md = report.markdown({"fingerprint": report.fingerprint(), "made_by": [], "lines": _lines(b)})
    assert "| 5 (channels/el-m:p/0/timeSeries/5/value: 10 ms nan" in md
    # a flag today's 1 ms run raises too is today's (the new path lacking it is a difference of its own)
    f["summary"][0].update(notValid="only at 10 ms")
    _write(tmp_path, {"kind": "item", "id": item["id"], "setting": "today-fine"}, result=f)
    assert [(d["field"], d["what"], d["cls"]) for d in report.b2b(tmp_path)["diffs"]] == [
        ("summary.notValid", "text", "a"), ("status", "10 ms only", "unclassified"),
        ("summary.passed", "10 ms only", "unclassified"), ("messages", "10 ms only", "unclassified")]


def test_an_item_whose_ledger_does_not_balance_fails(tmp_path, monkeypatch):
    # the judge fails an item where a leaf of the three runs is left out or taken twice
    item = next(i for i in runs.items() if i["id"] == "bev-car/case-city")
    monkeypatch.setattr(runs, "items", lambda: [item])
    for setting in ("today-normal", "today-fine", "new"):
        _write(tmp_path, {"kind": "item", "id": item["id"], "setting": setting}, result=_ten())
    assert report.b2b(tmp_path)["state"] == "pass"
    walk = diff._Walk.times
    for broken, why in ((lambda self, *a: None, "compared 0, reported 0"),  # the output times not walked
                        (lambda self, *a: (walk(self, *a), walk(self, *a)), "compared 2, reported 0")):  # twice
        monkeypatch.setattr(diff._Walk, "times", broken)
        b = report.b2b(tmp_path)
        assert b["state"] == "fail" and b["unbalanced_ledger"] == [item["id"]], b["items"][0]["ledger"]
        ledger = b["items"][0]["ledger"]
        assert ledger["unbalanced"] == 10 and ledger["compared"] + ledger["reported"] + ledger["gaps"] < ledger["union"]
        assert all(p.startswith("channels/el-m:p/0/timeSeries/") and p.endswith(f"/t: {why}")
                   for p in ledger["first_unbalanced"]), ledger
        assert "| no: channels/el-m:p/0/timeSeries/" in report.markdown(
            {"fingerprint": report.fingerprint(), "made_by": [], "lines": _lines(b)})


def test_what_only_todays_10_ms_run_has_is_a_gap():
    # nothing to judge the new run by, and it agrees with today's 1 ms run in lacking it: no difference,
    # each leaf counted as a gap
    n = _ten()
    _point_more(n, lambda x: x)
    n["messages"].append({"level": "info", "text": "Only at 10 ms."})
    ds = diff.compare("x", n, _ten(), _ten())
    assert ds == [] and ds.ledger.totals() | {"union": None, "compared": None} == {
        "union": None, "compared": None, "sourced": 0, "ten_ms_only": 0, "reported": 0, "gaps": 4, "balanced": True,
        "unbalanced": 0, "first_unbalanced": [], "first_ten_ms_only": [],
        "first_gaps": ["channels/el-m:p/0/timeSeries/10/t", "channels/el-m:p/0/timeSeries/10/value",
                       "messages/info: Only at # ms./0/level", "messages/info: Only at # ms./0/text"]}


def test_what_only_todays_10_ms_run_has_under_a_record_the_new_run_lacks_is_a_gap():
    # (the judge's fourth re-check, recheck4/revert_more.sh: no test held it; baseline-5's 13 gaps all lie
    # under records the new path lacks, the limits and the energy report)
    n, f = _ten(), _ten()
    _row_more(n, f)
    n["summary"][-1]["note"] = "only at 10 ms"
    ds = diff.compare("x", n, f, _ten())
    totals = ds.ledger.totals()
    assert [(d["field"], d["key"], d["what"]) for d in ds] == [("summary", "range_km", "missing")]
    assert totals["balanced"] and totals["gaps"] == 1 and totals["first_gaps"] == ["summary/range_km/0/note"], totals


def test_the_report_lists_a_gap_whose_key_holds_a_bar_in_its_cell(tmp_path, monkeypatch):
    # baseline-5: the hybrid's tyre-slip sinks only at 10 ms ("el-wheel-rl|Wheel RL ...") split the row
    import re
    item = next(i for i in runs.items() if i["id"] == "bev-car/case-city")
    monkeypatch.setattr(runs, "items", lambda: [item])
    n = _ten()
    n["energy"]["sinks"].append({"label": "Tyre slip", "kWh": 0.01, "group": "slip", "elementId": "el-w"})
    for setting, res in (("today-normal", n), ("today-fine", _ten()), ("new", _ten())):
        _write(tmp_path, {"kind": "item", "id": item["id"], "setting": setting}, result=res)
    b = report.b2b(tmp_path)
    assert b["items"][0]["ledger"]["gaps"] == 4
    row = next(line for line in report.markdown({"fingerprint": report.fingerprint(), "made_by": [],
                                                  "lines": _lines(b)}).splitlines()
               if line.startswith(f"| {item['id']} | ") and "| yes |" in line)
    assert len(re.split(r"(?<!\\)\|", row)) == 10 and "el-w\\|Tyre slip" in row, row


def test_a_channel_the_10_ms_run_lacks_is_judged_in_the_floor_band():
    # the judge's fourth re-check (recheck3/normal_short.py): its values were never compared; now in the
    # band today's 1 ms run gives it alone (0.5 % of its RMS), as its statistics were
    today = [90.0 + 0.01 * k for k in range(100)]
    no_n = _result(channels=[], summary=[], messages=[])
    for factor, got in ((1.001, []), (10.0, ["value"] * 4)):
        new = [factor * x for x in today]
        ds = diff.compare("x", no_n, _channel_result(today, today), _channel_result(new, new))
        assert [d["what"] for d in ds if d["what"] != "first point"] == got, factor


def test_the_law_s_range_bounds_each_interval():
    own, before, after = [0.0, 10.0, 10.0], [0.0, 8.0, 10.0], [0.0, 12.0, 10.0]
    values = law = [0.0, 10.0, 10.0]
    got = [diff._interval(s, [own, before, after], values, law, 1.0) for s in
           ([0.0, 9.0, 10.0], [0.0, 13.5, 10.0], [0.0, 10.0, 7.0], [0.0, 10.0, math.nan])]
    assert got == [0.0, 1.5, 3.0, math.inf]
    # widened by the law's own error on the values at the interval's two ends
    assert diff._interval([0.0, 13.5, 10.0], [own, before, after], [0.0, 10.5, 10.0], law, 1.0) == 1.0
    assert diff._interval([0.0, 13.5], [own, before, after], values, law, 1.0) == math.inf  # a list too short


def _lines(b):
    """A report's lines with B2B ``b`` and the others empty, for markdown()."""
    empty = {"state": "x", "refusals_total": 0, "refusals": {}, "wrong_items": [], "bad": [], "errors": 0, "results": 0,
             "validator": "", "measured": 0, "faults": 0, "today_fails_or_warns": 0, "violations": [], "hidden": [],
             "today": {}, "new": {}, "refused": {}, "engine_notes": {}, "mutation": None, "rows": [],
             "judged_something": False, "worst_error_s": 0.0, "mark_s": 0.0, "label": {"labelled": False},
             "sources": {}, "E1": [], "E2": {}, "E3": {}, "S1": {"state": "x", "runs": {}}, "S2": {"state": "x"},
             "S3": {"state": "x"}}
    return {"B2B": b, **{k: empty for k in ("STRICT", "SCHEMA", "FAULTS", "META", "ANALYTIC", "ENERGY", "STREAM",
                                            "FMU", "TEXT", "QUICK", "SCRIPTS", "NOSKIP", "STATUS")}}


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
    # the baseline's jobs, the sine probe's two among them, and (since the evidence part) fast mode
    # on the BEVs' eight cycle items for E3, and (since the evidence part's review) the two items
    # with a battery check, on both engines and at META's three settings, and (since its third
    # review) the 20 faults planted as a case's own values, on both engines
    assert len(runs.specs(set(parts.split(",")))) == 631 + 8 + 2 * (3 + 3) + 2 * 5 * 4


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


@pytest.mark.parametrize("changed", ["tests/helpers.py", "tests/conftest.py", "app/library/components.json",
                                     "projects/bev-car.json", "app/solver/verdict.py"])
def test_the_mutmut_copy_is_made_afresh_when_what_its_tests_read_changes(tmp_path, monkeypatch, changed):
    # what the tests read beside the code (the judge's re-check, recheck/stale_more.py)
    from benchmarks.back_to_back import mutation
    backend = tmp_path / "backend"
    tests = [t for t in mutation.TESTS if not t.startswith("-")]
    for rel in ("tests/helpers.py", "tests/conftest.py", "app/library/components.json", "projects/bev-car.json",
                "app/solver/verdict.py", *tests):
        (backend / rel).parent.mkdir(parents=True, exist_ok=True)
        (backend / rel).write_text(f"# {rel}\n")
    (backend / ".hypothesis").mkdir()
    monkeypatch.setattr(mutation, "BACKEND", backend)
    work = mutation.prepare(tmp_path / "out")
    (work / "progress").write_text("mutmut's")
    (backend / ".hypothesis" / "examples").write_text("a cache, not code")
    mutation.prepare(tmp_path / "out")
    assert (work / "progress").exists()  # a cache changed: kept
    (backend / changed).write_text("# changed\n")
    mutation.prepare(tmp_path / "out")
    assert (work / changed).read_text() == "# changed\n" and not (work / "progress").exists()


def test_the_noskip_probe_says_which_code_made_it(tmp_path, monkeypatch):
    monkeypatch.setattr(report, "_pytest", lambda *a, **k: {"tests": 1, "failed": [], "skipped": []})
    report.noskip_probe(tmp_path)
    assert json.loads((tmp_path / "noskip.json").read_text())["stamp"] == runs.stamp()
    assert [m["probes"] for m in report.stamps(tmp_path)] == [["noskip.json"]]


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

    # the verdict's own message labels its allowance (since the evidence part)
    assert traces.labelled()["labelled"]

    def unlabelled(*a, **k):
        # the same message without its label: "LightSim's own" in the source is no label
        v = real(*a, **k)
        return dataclasses.replace(v, messages=tuple(
            (lv, t.split(" Allowing 1 %")[0] if t.startswith("Cycle not") else t) for lv, t in v.messages))
    monkeypatch.setattr(verdict, "judge", unlabelled)
    got = traces.labelled()
    assert got["found"] and not got["labelled"]


def test_scripts_reads_which_runner_served_each_step(tmp_path, monkeypatch):
    monkeypatch.delenv("LSIM_SCRATCH", raising=False)
    iid = "probe/hybrid-udds-single"
    b = {"items": [{"id": iid, "got": "run"}]}
    spec = {"kind": "item", "id": iid, "setting": "new"}
    worker = {"own_pid": 100, "pids": [101]}
    for engine, a in (
            ({"scripts": {"HCU": "python"}, "script_calls": {"el-hcu": 9}, "script_served": {"el-hcu": 9},
              "script_worker": worker}, True),
            # the steps run somewhere else than today's worker (the app's own process)
            ({"scripts": {"HCU": "python"}, "script_calls": {"el-hcu": 9}, "script_served": {},
              "script_worker": {"own_pid": 100, "pids": []}}, False),
            ({"scripts": {"HCU": "python"}, "script_calls": {"el-hcu": 9}, "script_served": {"el-hcu": 4},
              "script_worker": worker}, False),
            ({"scripts": {"HCU": "native"}, "script_calls": {"el-hcu": 0}, "script_served": {},
              "script_worker": worker}, False),
            ({"scripts": {"HCU": "python"}, "script_calls": {"el-hcu": 9}, "script_served": {"el-hcu": 9},
              "script_worker": {"own_pid": 101, "pids": [101]}}, False),
            ({"scripts": {"HCU": "python"}}, None)):  # a record made before the counts: not measured
        _write(tmp_path, spec, result=_result(), engine=engine)
        got = report.scripts(b, tmp_path)
        assert got["a_worker_on_new_path"] is a, engine
        assert got["c_owner_page"] is None  # the owner's decisions out of reach: not measured


class _InProcess:
    """Today's in-process Script runner in place of the worker: the code run
    in the app's own process (the judge's re-check, recheck/scripts_inprocess.py)."""

    def __init__(self, specs, **_):
        from app.solver import scripting
        self.scripting = scripting
        self.steps = {s.el_id: (scripting.compile_script(s.code, s.label), {}) for s in specs}

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        return False

    def run(self, el_id, label, t, dt, inputs, params):
        step, state = self.steps[el_id]
        return self.scripting.run_script(step, label, t, dt, inputs, state, params)


@needs_engine
def test_scripts_tells_todays_worker_from_the_apps_own_process(tmp_path, monkeypatch):
    import lightsim_engine
    from app.solver import new_engine, sandbox
    for k, v in (("LIGHTSIM_ENGINE", "new"), ("LIGHTSIM_ENGINE_STRICT", "1"), ("LIGHTSIM_SCRIPT_TRUST", "off"),
                 ("LIGHTSIM_DATA_DIR", str(tmp_path / "user")),
                 ("LIGHTSIM_PROJECTS_DIR", str(tmp_path / "user" / "projects"))):
        monkeypatch.setenv(k, v)
    real = lightsim_engine.build, sandbox.ScriptSandbox.run  # runs.child wraps both
    spec = {"kind": "probe", "id": "energy/sine", "setting": "new"}
    try:
        for runner, served in ((None, True), (_InProcess, False)):
            if runner:
                monkeypatch.setattr(new_engine, "ScriptSandbox", runner)
            path = tmp_path / f"{served}.json"
            runs.child(spec, path)
            lightsim_engine.build, sandbox.ScriptSandbox.run = real
            rec = json.loads(path.read_text(encoding="utf-8"))
            assert report.refusal(rec["result"]) is None, rec["result"]["messages"][0]
            assert rec["engine"]["script_calls"]["el-sine"] > 0
            assert report.worker_served(rec["engine"]) is served, rec["engine"]
    finally:
        lightsim_engine.build, sandbox.ScriptSandbox.run = real


def test_scripts_b_reads_what_the_decision_record_holds(tmp_path, monkeypatch):
    monkeypatch.setattr(runs, "REPO", tmp_path)
    (tmp_path / "docs").mkdir()
    record = tmp_path / "docs" / "script-decision.md"
    record.write_text("")  # a name alone (the judge's re-check, recheck/stale_more.py)
    got = report.scripts({"items": []}, tmp_path)
    assert got["b_decision_record"] == [] and got["b_records_lack"]["docs/script-decision.md"] == \
        list(report.RECORD_PARTS)
    record.write_text("# Script blocks: the decision\n\n## Threat model (STRIDE)\n\n...\n\n"
                      "## Differential fuzzing against CPython 3.11\n\n- **Hours:** 24\n- Inputs: 1,200,000\n"
                      "- Divergences: 0\n\n## Unsafe code\n\n| cargo-geiger, unsafe expressions | 12 |\n\n"
                      "## The PLT-35 gap\n\n...\n")
    got = report.scripts({"items": []}, tmp_path)
    assert got["b_decision_record"] == ["docs/script-decision.md"], got["b_records_lack"]
    record.write_text(record.read_text().replace("- Divergences: 0\n", ""))
    assert report.scripts({"items": []}, tmp_path)["b_records_lack"]["docs/script-decision.md"] == [
        "the fuzzing's divergences"]


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
    # the path shows a build note with its code before its words (new_engine.py)
    rec["result"]["messages"] = [{"level": "warning", "text": f"Preparation [STATE-SELECT-NEVER]: {note}"}]
    assert report._without_notes(rec) == "success"
    rec["result"]["messages"] = [{"level": "warning", "text": f"Not the note: {note} and more"}]
    assert report._without_notes(rec) == "warning"


def test_e3_judges_the_lanes_stretches_at_a_limit_against_fast_mode(tmp_path, monkeypatch):
    """E3: the new run's limit lanes' stretches at a limit against fast mode's
    flags, each end within the case's output step; not measured without fast
    mode's runner; a lane that meets no limit against no flag agrees."""
    monkeypatch.setattr(runs, "fast_items", lambda: ["bev-car/case-city"])
    states = ["braking", "grip", "set_limit", "supply", "machine", "coasting", "demand"]

    def new_run(changes):
        return {"result": _result(caseId="case-city", limits={
            "states": states, "tEnd": 600.0,
            "lanes": [{"label": "E-Motor", "elementIds": ["el-motor"], "changes": changes, "seconds": {}}]})}
    spec_new = {"kind": "item", "id": "bev-car/case-city", "setting": "new"}
    spec_fast = {"kind": "probe", "id": "fast/bev-car/case-city", "setting": "fast"}
    _write(tmp_path, spec_new, **new_run([[0.0, 6], [24.4, 2], [31.6, 6], [300.1, 0], [330.0, 6]]))
    _write(tmp_path, spec_fast, fast={"error": "not measured: LSIM_FAST_BIN is not set"})
    assert report.e3_rows(tmp_path)["state"] == "not measured"
    _write(tmp_path, spec_fast, fast={"flag_spans": [{"t_start": 24.7, "t_end": 31.0}]})
    got = report.e3_rows(tmp_path)
    assert got["state"] == "pass" and got["rows"][0]["lanes"] == [[24.4, 31.6]]
    assert got["met_a_limit"] == 1
    _write(tmp_path, spec_fast, fast={"flag_spans": [{"t_start": 24.7, "t_end": 30.0}]})  # 1.6 s off: fails
    assert report.e3_rows(tmp_path)["state"] == "fail"
    _write(tmp_path, spec_fast, fast={"flag_spans": []})
    assert report.e3_rows(tmp_path)["state"] == "fail"  # a stretch fast mode does not flag
    _write(tmp_path, spec_new, **new_run([[0.0, 6], [300.1, 0], [330.0, 6]]))
    got = report.e3_rows(tmp_path)
    # nothing limited, nothing flagged: they agree on nothing, which cannot fail (before: "pass")
    assert got["state"] == "vacuous" and got["met_a_limit"] == 0
    assert report.lane_limits({"states": states, "tEnd": 10.0, "lanes": [
        {"changes": [[0.0, 3], [2.0, 4], [5.0, 6], [8.0, 1]]}]}) == [[0.0, 5.0], [8.0, 10.0]]


def test_energy_with_a_vacuous_e3_is_not_a_pass(tmp_path, monkeypatch):
    """ENERGY passes only when each of its parts does: E3 agreeing on
    nothing limited makes the line vacuous, not a pass (before: "pass" on
    eight items none of which met a limit on either side)."""
    monkeypatch.setattr(report, "e3_rows", lambda out: {"state": "vacuous", "rows": [], "met_a_limit": 0})
    b = {"items": []}
    sine = runs.SINE
    rms = (sine["P0"] ** 2 + sine["A"] ** 2 / 2) ** 0.5
    duty = [{"elementId": "el-battery", "rows": [{"quantity": "Power", "rms": rms}]}]
    for setting in ("today-normal", "new"):
        _write(tmp_path, {"kind": "probe", "id": "energy/sine", "setting": setting},
               result=_result(duty=duty))
    assert report.energy(tmp_path, b)["state"] == "vacuous"


def test_faults_lists_a_model_only_the_new_path_flags(tmp_path, monkeypatch):
    """FAULTS lists the flips beside the violations: a planted fault today's
    engine passes and the new path warns or fails on (an enforced cap held
    to the solver's tolerance read as broken, the evidence part's review),
    which neither the violations nor the hidden counted."""
    flip, same = "fs-electric:el-battery.internal_resistance_ohm:zero", "bev-car:el-motor"
    monkeypatch.setattr(runs, "fault_ids", lambda: [flip, same])
    for fid, status in ((flip, "warning"), (same, "success")):
        _write(tmp_path, {"kind": "fault", "id": fid, "setting": "today-normal"}, checks=[],
               result=_result())
        _write(tmp_path, {"kind": "fault", "id": fid, "setting": "new"}, result=_result(
            status=status, messages=[{"level": "warning" if status == "warning" else "info",
                                      "text": "Battery 'Accumulator' broke its Output Power Limit"}]))
    got = report.faults(tmp_path)
    assert got["flips"] == [flip] and got["violations"] == [] and got["hidden"] == []


def test_a_case_value_fault_is_the_case_s_own_value():
    """A case_values fault sets the case's own value, which Data Checks do
    not see, and leaves the block's as it was; a block fault sets the
    block's (the third review: the matrix planted block values only, so
    today's run-time checks of a case's values were judged by no line)."""
    project, case_id = runs.fault_project("bev-car:case/el-ambient.pressure_kPa:div100")
    case = next(c for c in project["cases"] if c["id"] == case_id)
    amb = runs._element(project, "el-ambient")
    assert case["parameterOverrides"]["el-ambient"] == {"pressure_kPa": pytest.approx(1.01325)}
    assert "pressure_kPa" not in amb.get("parameterOverrides", {})
    project, case_id = runs.fault_project("fs-electric:case/el-vehicle.cg_height_m:x100")
    case = next(c for c in project["cases"] if c["id"] == case_id)
    assert case["parameterOverrides"]["el-vehicle"] == {"cg_height_m": pytest.approx(28.0)}
    project, case_id = runs.fault_project("bev-car:el-vehicle.mass_kg:x100")
    case = next(c for c in project["cases"] if c["id"] == case_id)
    assert not case.get("parameterOverrides")
    assert runs._element(project, "el-vehicle")["parameterOverrides"]["mass_kg"] > 1e4
    assert sum(f.count(":case/") for f in runs.fault_ids()) == 5 * 4


def test_faults_lists_a_warning_today_gives_and_the_new_path_does_not(tmp_path, monkeypatch):
    """FAULTS lists each fault's unsaid: a warning or error of today's run,
    in its words without their numbers, that the new path's run does not
    give, where it ran: the Ambient's warning at 70 °C, missing beside the
    climate table's, which keeps the status the same (the third review);
    the same words at another number are said; a refused run says none."""
    amb = {"level": "warning", "text": "'Ambient' is at 70 °C and 101.325 kPa at t = 0.00 s, outside the usual"}
    table = {"level": "warning", "text": "Climate Control 'Climate Control' ran 30 °C past its table"}
    other = {"level": "warning", "text": "Climate Control 'Climate Control' ran 31 °C past its table"}
    missing, refused = "bev-car:case/el-ambient.temperature_C:x100", "bev-car:el-motor"
    monkeypatch.setattr(runs, "fault_ids", lambda: [missing, refused])
    for fid in (missing, refused):
        _write(tmp_path, {"kind": "fault", "id": fid, "setting": "today-normal"}, checks=[],
               result=_result(status="warning", messages=[amb, table]))
    _write(tmp_path, {"kind": "fault", "id": missing, "setting": "new"},
           result=_result(status="warning", messages=[other]))
    _write(tmp_path, {"kind": "fault", "id": refused, "setting": "new"}, result=_result(
        status="failed", messages=[{"level": "error", "text": "Not run [NEW-FAILED]: the new engine refused it"}]))
    got = report.faults(tmp_path)
    assert got["unsaid"] == {missing: 1} and got["violations"] == [] and got["flips"] == []
    assert got["rows"][0]["unsaid"] == [diff.ROWS[("messages",)](amb)] and got["rows"][1]["unsaid"] == []
    monkeypatch.setattr(runs, "items", lambda: [])
    md = report.markdown({"lines": {**_lines(report.b2b(tmp_path)), "FAULTS": got},
                          "made_by": [], "fingerprint": {"commit": "0" * 10, "dirty": False, "python": "",
                                                         "lightsim_engine": "", "cpu": ""}})
    assert f"- {missing}: 1" in md


def test_faults_lists_a_row_today_flags_and_the_new_path_does_not(tmp_path, monkeypatch):
    """FAULTS' unsaid holds what else today's run flags: a row it makes not
    valid and a check it fails that the new path's run, with the same status
    and the same words, does not (the fourth review: a study point at
    -300 °C, today's four rows not valid, none on the new path, counted
    nowhere); a row both flag is not listed."""
    fid = "bev-car:case/el-ambient.temperature_C:negative"
    monkeypatch.setattr(runs, "fault_ids", lambda: [fid])
    msg = {"level": "warning", "text": "'Ambient' is at -300 °C"}
    both = {"key": "range_km", "label": "Range", "unit": "km", "value": 1.0, "notValid": "the cycle was not followed"}
    rows = [both, {**both, "key": "consumption_kwh_per_100km"},
            {"key": "el-battery.peak_power_kw", "label": "Peak", "unit": "kW", "value": 30.0, "passed": False}]
    _write(tmp_path, {"kind": "fault", "id": fid, "setting": "today-normal"}, checks=[],
           result=_result(status="warning", messages=[msg], summary=rows))
    _write(tmp_path, {"kind": "fault", "id": fid, "setting": "new"},
           result=_result(status="warning", messages=[msg], summary=[both, {**rows[2], "passed": True}]))
    got = report.faults(tmp_path)
    assert got["violations"] == [] and got["unsaid"] == {fid: 2}
    assert got["rows"][0]["unsaid"] == ["summary.notValid: consumption_kwh_per_100km",
                                        "summary.passed: el-battery.peak_power_kw"]


def test_a_note_of_what_the_verdict_does_not_judge_is_counted_by_its_code(tmp_path, monkeypatch):
    """The path's note that a record of the run is not judged (the second
    review of the evidence part: in no section, counted by no line) is
    counted by its code in B2B and FAULTS, and STRICT fails on a code in a
    message that no section lists, not only in a refusal's."""
    note = {"level": "info", "text": "'Vehicle Task' profile: the new engine read it past its data on axis 1 "
                                     "... so the verdict does not judge it [NEW-TABLE-NOT-JUDGED]."}
    item, fault = {"id": "bev-car/case-wltc", "expect": "run"}, "bev-car:el-motor"
    monkeypatch.setattr(runs, "items", lambda: [item])
    monkeypatch.setattr(runs, "fault_ids", lambda: [fault])
    ran = {"level": "info", "text": "Run on the new engine (LIGHTSIM_ENGINE=new)."}
    _write(tmp_path, {"kind": "item", "id": item["id"], "setting": "new"},
           result=_result(caseId="case-wltc", messages=[ran, note]))
    _write(tmp_path, {"kind": "fault", "id": fault, "setting": "today-normal"}, checks=[], result=_result())
    _write(tmp_path, {"kind": "fault", "id": fault, "setting": "new"}, result=_result(messages=[
        ran, note, {"level": "info", "text": "something else not judged [NEW-SOMETHING-ELSE]"}]))
    b = report.b2b(tmp_path)
    assert b["notes"] == {"NEW-TABLE-NOT-JUDGED": 1} == b["items"][0]["notes"]
    f = report.faults(tmp_path)
    assert f["notes"] == {"NEW-TABLE-NOT-JUDGED": 1} == f["rows"][0]["notes"]
    s = report.strict(tmp_path, b)
    assert [x["why"].split(":")[0] for x in s["bad"]] == ["code NEW-SOMETHING-ELSE is not listed"]
    assert s["state"] == "fail"
    lines = {**_lines(b), "FAULTS": f}
    md = report.markdown({"lines": lines, "made_by": [], "fingerprint": {
        "commit": "0" * 10, "dirty": False, "python": "", "lightsim_engine": "", "cpu": ""}})
    assert md.count("NEW-TABLE-NOT-JUDGED") == 2  # (B2B's and FAULTS')


def test_the_e3_cases_are_the_bevs_cycle_cases():
    got = runs.fast_items()
    assert got == ["aero-bev/case-udds", "aero-bev/case-hwfet", "aero-bev/case-wltc", "bev-car/case-city",
                   "bev-car/case-wltc", "bev-car/case-wltc-hvac", "bev-car/case-wltc-winter",
                   "bev-car/case-wltc-summer"]
    jobs = runs.specs({"energy"})
    assert [j["id"] for j in jobs if j["setting"] == "fast"] == [f"fast/{i}" for i in got]


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
    # the new path makes today's reports (the evidence part): none lacks, its start notes are notes
    got = {(d["cls"], d["rule"]) for d in ds}
    assert ("a", "new-path-lacks-reports") not in got and ("c", "engine-preparation-notes") in got
    assert new["partEnergy"] and new["energy"] and new["duty"] and new["limits"] and new["references"]
    import jsonschema
    schema = json.loads((runs.REPO / "docs" / "spec" / "schemas" / "result.schema.json").read_text())
    for r in rec:
        jsonschema.Draft202012Validator(schema).validate(r["result"])
