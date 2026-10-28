"""Every acceptance line measured from the raw results in OUT, written to
OUT/report.json (every number) and OUT/report.md (the tables), with the
raw files' SHA-256 in OUT/SHA256SUMS, the tools' versions and the
machine's fingerprint (rule 6: evidence an outsider can rerun).

A line's state is "pass", "fail", "vacuous" (it passes only because
nothing was there to judge), "cannot be measured" (what it judges is not
on the new path at all) or "not available" (the feature it judges does
not exist yet); only "pass" passes.
"""
from __future__ import annotations

import hashlib
import json
import platform
import re
import subprocess
import sys
from collections import Counter
from pathlib import Path

from . import diff, runs, traces

CODE = re.compile(r"\[(NEW-[A-Z-]+)\]")
NOTE_CODES = ("START-NOT-SOLVED", "INIT-UNDECIDED", "INIT-START-IGNORED")
CFR = Path("/home/eyada/scratch/judge-cache/cfr")  # judge.py fetch's cache (engine/judge)


def refusal(result: dict | None) -> str | None:
    """The code of a strict-mode refusal ("Not run [CODE]: ..."), or None
    when the new path ran the case itself."""
    if not result or not result.get("messages"):
        return None
    first = result["messages"][0]["text"]
    if first.startswith("Not run ["):
        m = CODE.search(first)
        return m.group(1) if m else "uncoded"
    return None


def _ok(rec: dict | None) -> bool:
    return bool(rec) and "result" in rec


# ---- B2B --------------------------------------------------------------------

def b2b(out: Path) -> dict:
    rows, all_diffs = [], []
    for item in runs.items():
        rec = {s: runs.load(out, {"kind": "item", "id": item["id"], "setting": s})
               for s in ("today-normal", "today-fine", "new")}
        new = rec["new"]["result"] if _ok(rec["new"]) else None
        got = "missing" if new is None else (refusal(new) or "run")
        expect = item["expect"]
        row = {"id": item["id"], "expect": expect, "got": got,
               "as_expected": got == expect.removeprefix("refuse:") if expect.startswith("refuse:")
               else got == "run",
               "today": {s: (rec[s]["result"]["status"] if _ok(rec[s]) else rec[s] and rec[s].get("crashed"))
                         for s in ("today-normal", "today-fine")},
               "new_status": new and new["status"], "classes": {}}
        if got == "run" and _ok(rec["today-normal"]) and _ok(rec["today-fine"]):
            n, f = rec["today-normal"]["result"], rec["today-fine"]["result"]
            ds = diff.compare(item["id"], n, f, new)
            ctx = {"notes": {diff.masked(d["message"]) for d in (rec["new"].get("engine") or {})
                             .get("build_warnings", [])},
                   "status_without_notes": _without_notes(rec["new"])}
            if item["id"] != "probe/bev-fmu":
                project, case_id, _ = runs.item_project(item, out / "user")
                ctx["gears"] = diff.gear_rebuild(project, n, f, new)
                case = runs._case(project, case_id)
                if case.get("kind") == "acceleration" and case.get("endDistance"):
                    last = {k: next(c["timeSeries"][-1]["value"] for c in r["channels"]
                                    if c["elementId"] == "el-vehicle" and c["portId"] == "sig_distance")
                            for k, r in (("today", f), ("new", new))}
                    ctx["line_end"] = {**last, "line": (case.get("startLine") or 0.0) + case["endDistance"]}
            diff.classify(ds, {item["id"]: ctx})
            row["classes"] = dict(Counter(d["cls"] for d in ds))
            row["wall_s"] = {"today-normal": rec["today-normal"]["wall_s"],
                             "today-fine": rec["today-fine"]["wall_s"], "new": rec["new"]["wall_s"]}
            all_diffs += ds
        rows.append(row)
    by_class = Counter(d["cls"] for d in all_diffs)
    by_rule = Counter((d["cls"], d["rule"]) for d in all_diffs)
    unclassified = [d for d in all_diffs if d["cls"] == "unclassified"]
    expected = sum(r["as_expected"] for r in rows)
    state = "pass" if expected == len(rows) and not unclassified and not by_class.get("a") else "fail"
    return {"state": state, "items": rows, "as_expected": expected, "of": len(rows),
            "differences": len(all_diffs), "by_class": dict(by_class),
            "by_rule": {f"{c} {r}": k for (c, r), k in sorted(by_rule.items(), key=str)},
            "unclassified_by_field": dict(Counter(f"{d['item']} | {d['field']} | {d['what']}"
                                                  for d in unclassified).most_common()),
            "unclassified": unclassified, "diffs": all_diffs}


# ---- STRICT, SCHEMA, FAULTS ----------------------------------------------------

def _all_records(out: Path):
    for path in sorted((out / "raw").glob("*/*.json")):
        yield path, json.loads(path.read_text(encoding="utf-8"))


def strict(out: Path, b: dict) -> dict:
    codes = set(runs.ACCEPTANCE["codes"])
    seen, bad = Counter(), []
    for path, rec in _all_records(out):
        spec = rec.get("spec", {})
        if not spec.get("setting", "").startswith(("new", "meta")):
            continue
        if not _ok(rec):
            bad.append({"job": spec, "why": f"the run's process ended without a result ({rec.get('crashed')})"})
            continue
        code = refusal(rec["result"])
        if code is None:
            continue
        seen[code] += 1
        if code not in codes:
            bad.append({"job": spec, "why": f"code {code} is not listed"})
        elif code == "NEW-ERROR":
            bad.append({"job": spec, "why": rec["result"]["messages"][0]["text"]})
        note = rec.get("fallback_note")
        if code not in ("NEW-FAILED", "NEW-BUILD", "NEW-ERROR") and not (note and f"[{code}]" in note):
            bad.append({"job": spec, "why": f"the fall-back message lacks [{code}]: {note!r}"})
    wrong = [{"id": r["id"], "expect": r["expect"], "got": r["got"]} for r in b["items"] if not r["as_expected"]]
    return {"state": "pass" if not wrong and not bad else "fail", "refusals": dict(seen),
            "refusals_total": sum(seen.values()), "wrong_items": wrong, "bad": bad}


SCHEMA = runs.REPO / "docs" / "spec" / "schemas" / "result.schema.json"


def _validate(path: Path) -> tuple[str, str, list[dict]]:
    """(file, its SHA-256, the schema errors of its result) — a few seconds a
    large result in pure Python, so the report runs them on several cores
    and keeps them by checksum (OUT/schema.json) for the next report."""
    import jsonschema
    raw = path.read_bytes()
    rec = json.loads(raw)
    errors = []
    if _ok(rec):
        validator = jsonschema.Draft202012Validator(json.loads(SCHEMA.read_text(encoding="utf-8")))
        errors = [{"at": "/".join(map(str, e.path)), "error": e.message[:200]}
                  for e in validator.iter_errors(rec["result"])]
    return str(path), hashlib.sha256(raw).hexdigest() + ("" if _ok(rec) else ":no result"), errors


def schema(out: Path) -> dict:
    from concurrent.futures import ProcessPoolExecutor
    from importlib.metadata import version
    cache_path = out / "schema.json"
    key = hashlib.sha256(SCHEMA.read_bytes()).hexdigest()
    cache = json.loads(cache_path.read_text(encoding="utf-8")) if cache_path.exists() else {}
    if cache.get("schema") != key:
        cache = {"schema": key, "files": {}}
    paths = sorted((out / "raw").glob("*/*.json"))
    sums = {str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in paths}
    todo = [p for p in paths if not str(cache["files"].get(str(p), {}).get("sha", "")).startswith(sums[str(p)])]
    with ProcessPoolExecutor(max_workers=6) as pool:
        for name, sha, errs in pool.map(_validate, todo, chunksize=4):
            cache["files"][name] = {"sha": sha, "errors": errs}
    cache_path.write_text(json.dumps(cache), encoding="utf-8")
    files = {k: v for k, v in cache["files"].items() if k in sums}
    checked = [k for k, v in files.items() if not v["sha"].endswith(":no result")]
    errors = [{"file": "/".join(Path(k).parts[-2:]), **e} for k in checked for e in files[k]["errors"]]
    return {"state": "pass" if checked and not errors else "fail", "results": len(checked),
            "errors": len(errors), "first_errors": errors[:20],
            "validator": f"jsonschema {version('jsonschema')} (Draft 2020-12)"}


def _without_notes(rec: dict) -> str:
    """The new run's status without the engine's preparation notes: its build
    report's messages (the start-guess note of finding 36 among them)."""
    res = rec["result"]
    if res["status"] != "warning":
        return res["status"]
    notes = {d["message"] for d in (rec.get("engine") or {}).get("build_warnings", [])}
    left = [m for m in res["messages"] if m["level"] == "warning" and m["text"] not in notes]
    return "warning" if left else "success"


def faults(out: Path) -> dict:
    rows = []
    for fault in runs.fault_ids():
        t = runs.load(out, {"kind": "fault", "id": fault, "setting": "today-normal"})
        v = runs.load(out, {"kind": "fault", "id": fault, "setting": "new"})
        if not (_ok(t) and _ok(v)):
            rows.append({"fault": fault, "missing": True})
            continue
        code = refusal(v["result"])
        today, new = t["result"]["status"], v["result"]["status"]
        bare = new if code else _without_notes(v)
        notes = sorted({d["code"] for d in (v.get("engine") or {}).get("build_warnings", [])})
        rows.append({"fault": fault, "checks": t["checks"], "today": today, "new": new, "refused": code,
                     "new_without_notes": bare, "engine_notes": notes,
                     "violation": today in ("failed", "warning") and new == "success",
                     "hidden": today in ("failed", "warning") and new == "warning" and bare == "success"})
    mut_path = out / "mutation.json"
    mut = json.loads(mut_path.read_text(encoding="utf-8")) if mut_path.exists() else None
    done = [r for r in rows if not r.get("missing")]
    viol = [r["fault"] for r in done if r["violation"]]
    hidden = [r["fault"] for r in done if r["hidden"]]
    flagged = [r for r in done if r["today"] in ("failed", "warning")]
    mut_ok = bool(mut) and mut["complete"] and not mut["without_reason"]
    state = "pass" if not viol and not hidden and mut_ok and len(done) == len(rows) else "fail"
    return {"state": state, "faults": len(rows), "measured": len(done),
            "today_fails_or_warns": len(flagged), "violations": viol, "hidden": hidden,
            "refused": dict(Counter(r["refused"] for r in done if r["refused"])),
            "today": dict(Counter(r["today"] for r in done)), "new": dict(Counter(r["new"] for r in done)),
            "engine_notes": dict(Counter(c for r in done for c in r["engine_notes"])),
            "mutation": mut and {k: mut[k] for k in ("tool", "mutants", "by_state", "complete")}
            | {"survived": len(mut["survived"]), "without_reason": len(mut["without_reason"])},
            "rows": rows}


# ---- META ------------------------------------------------------------------------

def _verdict(res: dict) -> tuple:
    nv = sorted((s["key"], " ".join((s["notValid"] or "").split()[:3])) for s in res["summary"] if s.get("notValid"))
    passed = sorted((s["key"], s["passed"]) for s in res["summary"] if s.get("passed") is not None)
    return (res["status"], tuple(nv), tuple(passed))


def meta(out: Path) -> dict:
    cases, fault_list = runs.meta_set()
    rows, judged = [], False
    for kind, ids in (("item", cases), ("fault", fault_list)):
        for i in ids:
            got = {}
            for setting in ("new", *runs.META):
                rec = runs.load(out, {"kind": kind, "id": i, "setting": setting})
                if _ok(rec):
                    res = rec["result"]
                    got[setting] = {"verdict": _verdict(res), "refused": refusal(res),
                                    "status_without_notes": _without_notes(rec)}
                    v = got[setting]["verdict"]
                    if not got[setting]["refused"] and (got[setting]["status_without_notes"] != "success"
                                                        or v[1] or any(not p for _, p in v[2])):
                        judged = True
            same = len({json.dumps(g["verdict"]) for g in got.values()}) == 1 and len(got) == 1 + len(runs.META)
            rows.append({"id": i, "same": same, "settings": got})
    allsame = all(r["same"] for r in rows)
    state = "fail" if not allsame else ("pass" if judged else "vacuous")
    return {"state": state, "rows": rows, "judged_something": judged}


# ---- ANALYTIC ---------------------------------------------------------------------

def _sha(path: Path) -> str | None:
    return hashlib.sha256(path.read_bytes()).hexdigest() if path.exists() else None


def analytic(out: Path) -> dict:
    rows = traces.measure()
    label = traces.labelled()
    worst = max(r["error_s"] for r in rows)
    wltp = Path("/home/eyada/scratch/judge-cache/step2/wltp/sub6.html")
    return {"state": "pass" if worst <= 1e-6 and label["labelled"] else "fail", "worst_error_s": worst,
            "mark_s": 1e-6, "rows": rows, "label": label,
            "sources": {"40 CFR 1066.425 (2024-07-01)": _sha(CFR / "CFR-2024-title40-vol37-sec1066-425.xml"),
                        "EU 2017/1151 Annex XXI Sub-Annex 6 (legislation.gov.uk)": _sha(wltp)}}


# ---- ENERGY, STREAM, FMU, TEXT, QUICK, SCRIPTS, NOSKIP, STATUS ---------------------

def energy(out: Path, b: dict) -> dict:
    e1 = []
    for row in b["items"]:
        if row["got"] != "run":
            continue
        rec = runs.load(out, {"kind": "item", "id": row["id"], "setting": "new"})
        res, books = rec["result"], (rec.get("engine") or {}).get("energy")
        if not res["partEnergy"] or not books:
            e1.append({"id": row["id"], "state": "cannot be measured",
                       "why": "no part energies on the new path" if not res["partEnergy"] else "no books"})
            continue
        kwh = 1.0 / 3.6e6
        rows = res["partEnergy"]
        sums = {k: sum(r[k] for r in rows) for k in ("energyIn", "energyOut", "losses", "stored")}
        through = books["throughput_J"] * kwh
        gap = abs(sums["losses"] - books["lost_J"] * kwh) + abs(sums["stored"] - books["stored_change_J"] * kwh)
        e1.append({"id": row["id"], "state": "pass" if gap <= 1e-12 * through else "fail",
                   "gap_rel": gap / through if through else None})
    sine = {s: runs.load(out, {"kind": "probe", "id": "energy/sine", "setting": s}) for s in ("today-normal", "new")}
    q = runs.SINE
    exact = (q["P0"] ** 2 + q["A"] ** 2 / 2) ** 0.5

    def rms_of(rec):
        if not _ok(rec):
            return None
        for p in rec["result"]["duty"]:
            for r in p["rows"]:
                if p["elementId"] == "el-battery" and r["quantity"] == "Power":
                    return r["rms"]
        return None
    e2 = {"exact_kw": exact, "today_kw": rms_of(sine["today-normal"]), "new_kw": rms_of(sine["new"]),
          "new_status": _ok(sine["new"]) and sine["new"]["result"]["status"]}
    e2["state"] = ("cannot be measured" if e2["new_kw"] is None else
                   "pass" if abs(e2["new_kw"] - exact) <= 1e-9 * exact else "fail")
    e3 = {"state": "cannot be measured",
          "why": "the new path makes no limits report, and fast mode is not reachable from Python "
                 "(lightsim_engine has no fast-mode call: engine/crates/lsim-py/README.md)"}
    parts = [x["state"] for x in e1] + [e2["state"], e3["state"]]
    state = ("pass" if all(s == "pass" for s in parts) else
             "cannot be measured" if all(s == "cannot be measured" for s in parts) else "fail")
    return {"state": state, "E1": e1, "E2": e2, "E3": e3}


def stream(out: Path) -> dict:
    s1 = {}
    for iid in ("bev-car/case-city-live", "hybrid-car/case-mixed-live"):
        for setting in ("today-normal", "new"):
            rec = runs.load(out, {"kind": "item", "id": iid, "setting": setting})
            if _ok(rec):
                s1[f"{iid} {setting}"] = {"refused": refusal(rec["result"]), "stream": rec.get("stream")}
    s1_new = [v for k, v in s1.items() if k.endswith(" new")]
    s1_state = ("pass" if s1_new and all(not v["refused"] and v["stream"] and v["stream"]["points"]
                                         and not v["stream"]["unequal"] for v in s1_new) else "fail")
    stop = json.loads((out / "stop.json").read_text(encoding="utf-8")) if (out / "stop.json").exists() else None
    replay = json.loads((out / "replay.json").read_text(encoding="utf-8")) if (out / "replay.json").exists() else None
    s2_state = "not measured" if not stop else (
        "pass" if all(stop["new"][k]["state"] == "pass" for k in ("live", "study")) else "fail")
    s3_state = "not measured" if not replay else (
        "pass" if all(r["same"] for r in replay["runs"]) and replay["records_engine"]
        and replay["records_engine_version"] and replay["records_solver_options"] else "fail")
    states = [s1_state, s2_state, s3_state]
    return {"state": "pass" if all(s == "pass" for s in states) else "fail",
            "S1": {"state": s1_state, "runs": s1},
            "S2": {"state": s2_state, **({k: {e: {kk: vv for kk, vv in stop[e][k].items() if kk != "latencies_s"}
                                             for e in ("today", "new")} for k in ("live", "study")}
                                         if stop else {}), "machine": stop and stop.get("machine")},
            "S3": {"state": s3_state, **(replay or {})}}


def fmu(b: dict) -> dict:
    row = next(r for r in b["items"] if r["id"] == "probe/bev-fmu")
    return {"state": "fail", "probe": row["got"],
            "why": f"the new path refuses a project with an FMU part ({row['got']}): nothing to compare with "
                   f"fmusim or FMPy" if row["got"] != "run" else "the fmusim and FMPy comparison is not built yet"}


def text_line() -> dict:
    lib = json.loads((runs.BACKEND / "app" / "library" / "components.json").read_text(encoding="utf-8"))
    has = any(c["id"].startswith("text.") or c.get("source") == "text" for c in lib["components"])
    return {"state": "fail" if has else "not available",
            "why": "the app's library has no part written as text, and the backend reads no text components"
                   if not has else "to measure"}


def quick() -> dict:
    from app.schemas import SimCase
    has = "mode" in SimCase.model_fields
    return {"state": "fail" if has else "not available",
            "why": "a case has no run option for quick mode (SimCase has no 'mode'), and the new path runs "
                   "the full dynamic engine only" if not has else "to measure"}


def scripts(b: dict) -> dict:
    src = (runs.BACKEND / "app" / "solver" / "new_engine.py").read_text(encoding="utf-8")
    hybrid = [r for r in b["items"] if r["id"].startswith("probe/hybrid-")]
    a = "native_scripts=False" in src and all(r["got"] == "run" for r in hybrid)
    records = [p for p in (runs.REPO / "docs").rglob("*.md") if "script" in p.name.lower() and "decision" in p.name.lower()]
    page = Path("/home/eyada/scratch/decisions/script-runner-explained.md").exists()
    got = {"a_worker_on_new_path": a, "b_decision_record": [str(p.relative_to(runs.REPO)) for p in records],
           "c_owner_page": page}
    n = sum([a, bool(records), page])
    return {"state": "pass" if n == 3 else "fail", "of_three": n, **got}


WINDOWS_ONLY = "judged on Windows only"


def _pytest(where: Path, args: list[str], env: dict, xml: Path) -> dict:
    """Run pytest, read its JUnit XML: tests, failed, skipped (and why)."""
    import os
    import xml.etree.ElementTree as ET
    subprocess.run([sys.executable, "-m", "pytest", *args, "-q", "-p", "no:cacheprovider",
                    f"--junitxml={xml}"], cwd=where, env={**os.environ, **env},
                   capture_output=True, text=True)
    root = ET.parse(xml).getroot()
    cases = list(root.iter("testcase"))
    skipped = [c.find("skipped").get("message", "") for c in cases if c.find("skipped") is not None]
    failed = [c.get("name") for c in cases if c.find("failure") is not None or c.find("error") is not None]
    return {"tests": len(cases), "failed": failed, "skipped": skipped}


def noskip_probe(out: Path) -> dict:
    """NOSKIP (review finding 18): the new engine's tests with
    LIGHTSIM_REQUIRE_NEW_ENGINE=1, with the module (a) and with it hidden
    behind a package that refuses to import (b); and (c) whether CI builds
    the module where it runs them with that variable."""
    import tempfile
    tests = ["tests/test_new_engine_flag.py", "tests/test_stop_latency.py"]
    harness = ["benchmarks/back_to_back/tests"]
    got = {}
    with tempfile.TemporaryDirectory() as tmp:
        shadow = Path(tmp) / "shadow" / "lightsim_engine"
        shadow.mkdir(parents=True)
        (shadow / "__init__.py").write_text('raise ImportError("hidden by the NOSKIP probe")\n')
        for tag, env in (("a", {"LIGHTSIM_REQUIRE_NEW_ENGINE": "1"}),
                         ("b", {"LIGHTSIM_REQUIRE_NEW_ENGINE": "1", "PYTHONPATH": str(shadow.parent)})):
            r1 = _pytest(runs.BACKEND, tests, env, Path(tmp) / f"{tag}1.xml")
            r2 = _pytest(runs.REPO, harness, env, Path(tmp) / f"{tag}2.xml")
            got[tag] = {"tests": r1["tests"] + r2["tests"], "failed": r1["failed"] + r2["failed"],
                        "skipped": r1["skipped"] + r2["skipped"]}
    ci = (runs.REPO / ".github" / "workflows" / "ci.yml").read_text(encoding="utf-8")
    job = ci[ci.find("engine-windows:"):ci.find("\n  perf:")]
    c = ("maturin build" in job and "LIGHTSIM_REQUIRE_NEW_ENGINE" in job
         and job.find("maturin build") < job.find("LIGHTSIM_REQUIRE_NEW_ENGINE"))
    res = {"a_tests": got["a"]["tests"], "a_failed": len(got["a"]["failed"]),
           "a_skipped": sum(WINDOWS_ONLY not in m for m in got["a"]["skipped"]),
           "a_windows_only": sum(WINDOWS_ONLY in m for m in got["a"]["skipped"]),
           "b_tests": got["b"]["tests"], "b_failed": len(got["b"]["failed"]),
           "b_skipped_engine": sum(WINDOWS_ONLY not in m for m in got["b"]["skipped"]),
           "c_ci": c, "detail": got}
    (out / "noskip.json").write_text(json.dumps(res, indent=1), encoding="utf-8")
    print(f"noskip: {res['a_tests']} tests, {res['a_skipped']} skipped with the module; "
          f"{res['b_failed']} failed and {res['b_skipped_engine']} skipped without; CI {c}", flush=True)
    return res


def noskip(out: Path) -> dict:
    path = out / "noskip.json"
    if not path.exists():
        return {"state": "not measured"}
    got = json.loads(path.read_text(encoding="utf-8"))
    ok = got["a_skipped"] == 0 and got["a_failed"] == 0 and got["b_failed"] > 0 and got["b_skipped_engine"] == 0 \
        and got["c_ci"]
    return {"state": "pass" if ok else "fail", **got}


def status(b: dict, lines: dict) -> dict:
    ran = any(r["got"] == "run" for r in b["items"])
    fields = {"the energy report": "energy", "part energies": "partEnergy", "duty": "duty",
              "limits": "limits", "references": "references"}
    present = {name: ran and not any(d["field"] == field and d["what"] == "missing" for d in b["diffs"])
               for name, field in fields.items()}
    # the verdict: none of today's verdict missing or a note standing in for it, a verdict that
    # judges something (META) and no planted fault let through (FAULTS)
    verdict_rules = {"new-path-lacks-verdict", "new-path-lacks-messages", "preparation-note-as-warning",
                     "preparation-note-status"}
    present["the verdict"] = (ran and not any(d["rule"] in verdict_rules for d in b["diffs"])
                              and lines["META"]["state"] == "pass"
                              and not lines["FAULTS"]["violations"] and not lines["FAULTS"]["hidden"])
    present["live streaming"] = lines["STREAM"]["S1"]["state"] == "pass"
    present["study stop and progress"] = lines["STREAM"]["S2"]["state"] == "pass"
    present["FMU parts"] = lines["FMU"]["state"] == "pass"
    present["text components"] = lines["TEXT"]["state"] == "pass"
    present["quick mode"] = lines["QUICK"]["state"] == "pass"
    n = sum(present.values())
    return {"state": "pass" if n == 11 else "fail", "present": n, "of": 11, "each": present,
            "documents": "checked by hand at each part's review (not measured here)"}


# ---- the report ----------------------------------------------------------------------

def fingerprint() -> dict:
    def run(*cmd):
        try:
            return subprocess.run(cmd, capture_output=True, text=True, cwd=runs.REPO).stdout.strip()
        except OSError:
            return ""
    try:
        import lightsim_engine
        engine = lightsim_engine.version()
    except ImportError:
        engine = None
    cpu = ""
    try:
        cpu = next(line.split(":", 1)[1].strip() for line in open("/proc/cpuinfo") if line.startswith("model name"))
    except (OSError, StopIteration):
        pass
    return {"commit": run("git", "rev-parse", "HEAD"), "dirty": bool(run("git", "status", "--porcelain")),
            "python": sys.version.split()[0], "platform": platform.platform(), "cpu": cpu,
            "lightsim_engine": engine}


def write(out: Path) -> dict:
    b = b2b(out)
    lines = {"B2B": b, "STRICT": strict(out, b), "SCHEMA": schema(out), "FAULTS": faults(out),
             "META": meta(out), "ANALYTIC": analytic(out), "ENERGY": energy(out, b), "STREAM": stream(out),
             "FMU": fmu(b), "TEXT": text_line(), "QUICK": quick(), "SCRIPTS": scripts(b), "NOSKIP": noskip(out)}
    lines["STATUS"] = status(b, lines)
    sums = [f"{hashlib.sha256(p.read_bytes()).hexdigest()}  {p.relative_to(out)}"
            for p in sorted(out.rglob("*.json")) if p.name != "report.json" and "mutation/backend" not in str(p)]
    (out / "SHA256SUMS").write_text("\n".join(sums) + "\n", encoding="utf-8")
    report = {"fingerprint": fingerprint(), "lines": lines}
    (out / "report.json").write_text(json.dumps(report, indent=1, default=str), encoding="utf-8")
    (out / "report.md").write_text(markdown(report), encoding="utf-8")
    print("\n".join(f"{k:9} {v['state']}" for k, v in lines.items()), flush=True)
    return report


def markdown(report: dict) -> str:
    L = report["lines"]
    b, f = L["B2B"], L["FAULTS"]
    out = ["# Step 2's acceptance lines, measured", "",
           f"Commit {report['fingerprint']['commit'][:10]}{' (with changes)' if report['fingerprint']['dirty'] else ''}, "
           f"Python {report['fingerprint']['python']}, lightsim_engine {report['fingerprint']['lightsim_engine']}, "
           f"{report['fingerprint']['cpu']}.", "", "| line | state |", "|---|---|"]
    out += [f"| {k} | {v['state']} |" for k, v in L.items()]
    out += ["", "## B2B", "", f"{b['as_expected']} of {b['of']} items as expected; {b['differences']} differences: "
            + ", ".join(f"{k} {v}" for k, v in sorted(b["by_class"].items())) + ".", "",
            "| item | expect | got | today 10 ms / 1 ms | new | a | b | c | unclassified |", "|---|---|---|---|---|--:|--:|--:|--:|"]
    for r in b["items"]:
        c = r["classes"]
        out.append(f"| {r['id']} | {r['expect']} | {r['got']}{'' if r['as_expected'] else ' ✗'} | "
                   f"{r['today']['today-normal']} / {r['today']['today-fine']} | {r['new_status']} | "
                   f"{c.get('a', '')} | {c.get('b', '')} | {c.get('c', '')} | {c.get('unclassified', '')} |")
    out += ["", "By rule: " + "; ".join(f"{k}: {v}" for k, v in b["by_rule"].items()), "",
            "Unclassified, by item and field:", ""]
    out += [f"- {k}: {v}" for k, v in list(b["unclassified_by_field"].items())[:60]]
    s = L["STRICT"]
    out += ["", "## STRICT", "", f"{s['refusals_total']} refusals: " + ", ".join(f"{k} {v}" for k, v in sorted(s["refusals"].items()))
            + ".", "", "Items not as expected: " + (", ".join(f"{w['id']} ({w['got']}, expected {w['expect']})"
                                                       for w in s["wrong_items"]) or "none") + ".",
            "", f"Other failures: {len(s['bad'])}."]
    out += [f"- {x['job']['setting']} {x['job']['id']}: {x['why'][:200]}" for x in s["bad"][:20]]
    sc = L["SCHEMA"]
    out += ["", "## SCHEMA", "", f"{sc['errors']} errors in {sc['results']} results ({sc['validator']})."]
    out += ["", "## FAULTS", "", f"{f['measured']} of {f['faults']} faults measured; today's engine fails or warns on "
            f"{f['today_fails_or_warns']}; violations (today fails or warns, the new path says success): "
            f"{len(f['violations'])}; hidden by the engine's preparation notes: {len(f['hidden'])}.", "",
            f"Today: {f['today']}. New path: {f['new']}. Refused: {f['refused']}. Engine notes on the new runs: "
            f"{f['engine_notes']}.", "", f"Mutation: {f['mutation']}.", "",
            "Violations: " + (", ".join(f["violations"]) or "none") + ".", "",
            "Hidden: " + (", ".join(f["hidden"]) or "none") + "."]
    m = L["META"]
    out += ["", "## META", "", f"{sum(r['same'] for r in m['rows'])} of {len(m['rows'])} the same at every setting; "
            f"something judged: {m['judged_something']}."]
    for r in m["rows"]:
        out.append(f"- {r['id']}: " + "; ".join(f"{k} {v['refused'] or v['verdict'][0]}"
                                                 f"{' nv' + str(len(v['verdict'][1])) if v['verdict'][1] else ''}"
                                                 for k, v in r["settings"].items()))
    a = L["ANALYTIC"]
    out += ["", "## ANALYTIC", "", "| trace | band | exact, s | verdict, s | error, s |", "|---|---|--:|--:|--:|"]
    out += [f"| {r['trace']} | {r['band']} | {r['exact_s']:.9g} | {r['verdict_s']:.9g} | {r['error_s']:.3g} |" for r in a["rows"]]
    out += ["", f"Worst error {a['worst_error_s']:.3g} s against {a['mark_s']:g} s; the 1 %/2 s rule labelled as "
            f"LightSim's own: {a['label']['labelled']}. Sources (SHA-256): {a['sources']}."]
    e = L["ENERGY"]
    out += ["", "## ENERGY", "", f"E1: {Counter(x['state'] for x in e['E1'])}. E2: {e['E2']}. E3: {e['E3']}."]
    st = L["STREAM"]
    out += ["", "## STREAM", "", f"S1 {st['S1']['state']}: " + "; ".join(
        f"{k}: {v['refused'] or v['stream']}" for k, v in st["S1"]["runs"].items()),
        "", f"S2 {st['S2']['state']}: " + json.dumps({k: v for k, v in st["S2"].items() if k != "state"}, default=str),
        "", f"S3 {st['S3']['state']}: " + json.dumps({k: st['S3'].get(k) for k in
                                                     ("records_engine", "records_engine_version", "records_solver_options", "runs")}, default=str)]
    for k in ("FMU", "TEXT", "QUICK", "SCRIPTS", "NOSKIP", "STATUS"):
        out += ["", f"## {k}", "", json.dumps(L[k], default=str)]
    return "\n".join(out) + "\n"
