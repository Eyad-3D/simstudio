"""Step 2's item 5 lines (acceptance.toml): FMU, TEXT and QUICK, each a
probe of its own that writes OUT/<line>.json with the code that made it.

* FMU: every co-simulation Reference FMU v0.0.41 in a one-block project on
  the new path against fmusim v0.7.0 run alone and FMPy 0.3.32, at the
  same communication step, every pinned output at every point
  (backend/tests/test_new_engine_fmu.compare_reference). It needs
  LIGHTSIM_REFERENCE_FMUS and LIGHTSIM_FMUSIM; without them the line is
  "not measured".
* TEXT: every part of the four examples the engine's import builds from
  equations, written as a text component (lightsim_engine.part_text) in a
  file of its own, one part at a time, on each example's first case the
  new path runs, strict: the result's SHA-256 against the built-in part's.
* QUICK: every case of the four examples in quick mode, strict: it runs
  or is refused with a listed code; each that runs against the same case
  in full on the new path, every figure within 1 % (the final SOC as the
  charge used), a case quick mode flags listed with its flags and not
  judged; the help and Known issues name every QUICK-* code.
"""
from __future__ import annotations

import json
import os
import re
import sys
from pathlib import Path

from . import runs
from .stream import _done_before, _Env, sha256_of

# TEXT's cases: each example's first case the new path runs (a hybrid's
# single run: chargeBalance false)
TEXT_CASES = {"aero-bev": "case-udds", "bev-car": "case-city", "fs-electric": "case-accel-75m",
              "hybrid-car": "case-udds"}
BAND = 0.01  # QUICK: every figure within 1 % of the full run's
FLAG = "Quick mode: "  # the start of quick mode's own messages (a flag's warning)


def _project(name: str, case_id: str):
    from app.schemas import Project
    project = Project.model_validate(runs.example(name))
    case = next(c for c in project.cases if c.id == case_id)
    if name == "hybrid-car":
        case.chargeBalance = False
    return project


# ---- TEXT ------------------------------------------------------------------------

def text_probe(out: Path) -> None:
    import lightsim_engine as le
    from app.solver import simulate
    path, now = out / "text.json", runs.stamp()
    if _done_before(path, now):
        print("text probe: done before, at this code")
        return
    folder = out / "text"
    folder.mkdir(parents=True, exist_ok=True)
    res = {"stamp": now, "cases": []}
    for name, case_id in TEXT_CASES.items():
        with _Env("new"):
            plain = simulate(_project(name, case_id), case_id)
        rec = {"case": f"{name}/{case_id}", "status": plain.status, "sha256": sha256_of(plain),
               "parts": [], "not_cases": []}
        base = _project(name, case_id)
        for el in [e for s in base.systems for e in s.elements]:
            try:
                text = le.part_text(base.model_dump_json(), case_id, el.id)
            except le.ModelError as e:
                codes = sorted({d["code"] for d in e.diagnostics})
                rec["not_cases"].append({"part": el.id, "kind": el.componentDefId, "codes": codes})
                continue
            file = folder / f"{name}__{el.id}.mo"
            file.write_text(text, encoding="utf-8")
            project = _project(name, case_id)
            next(e for s in project.systems for e in s.elements if e.id == el.id).textFile = str(file)
            with _Env("new"):
                got = simulate(project, case_id)
            rec["parts"].append({"part": el.id, "kind": el.componentDefId, "status": got.status,
                                 "sha256": sha256_of(got), "same": sha256_of(got) == rec["sha256"],
                                 "first": got.messages[0].text if got.messages else ""})
        res["cases"].append(rec)
        print(f"text probe {rec['case']}: {sum(p['same'] for p in rec['parts'])} of {len(rec['parts'])} the same, "
              f"{len(rec['not_cases'])} not cases", flush=True)
    path.write_text(json.dumps(res, indent=1), encoding="utf-8")


def text_line(out: Path) -> dict:
    path = out / "text.json"
    if not path.exists():
        return {"state": "not measured", "why": "no text probe (python -m benchmarks.back_to_back text OUT)"}
    got = json.loads(path.read_text(encoding="utf-8"))
    parts = [p for c in got["cases"] for p in c["parts"]]
    differ = [f"{c['case']}:{p['part']}" for c in got["cases"] for p in c["parts"] if not p["same"]]
    # the parts that are no case, each with its code: a sampled part (TEXT-SAMPLED) or a
    # sub-system's container (TEXT-NO-PART) only
    wrong = [f"{c['case']}:{n['part']} {n['codes']}" for c in got["cases"] for n in c["not_cases"]
             if not (n["codes"] == ["TEXT-SAMPLED"] and n["kind"] in ("signal.script", "signal.fmu",
                                                                      "control.traction")
                     or n["codes"] == ["TEXT-NO-PART"] and n["kind"] == "container.system")]
    ok = bool(parts) and not differ and not wrong and all(c["status"] != "failed" for c in got["cases"])
    return {"state": "pass" if ok else "fail", "parts": len(parts), "same": len(parts) - len(differ),
            "different": differ, "not_cases": {c["case"]: [f"{n['part']} {','.join(n['codes'])}"
                                                            for n in c["not_cases"]] for c in got["cases"]},
            "not_cases_unexpected": wrong, "stamp": got["stamp"]}


# ---- QUICK -----------------------------------------------------------------------

def _soc0(result, el_id: str):
    ch = next((c for c in result.channels if c.elementId == el_id and c.portId == "sig_soc"), None)
    return ch.timeSeries[0]["value"] if ch and ch.timeSeries else None


def quick_probe(out: Path) -> None:
    from app.solver import new_engine, simulate
    path, now = out / "quick.json", runs.stamp()
    if _done_before(path, now):
        print("quick probe: done before, at this code")
        return
    res = {"stamp": now, "band": BAND, "cases": []}
    for name in ("aero-bev", "bev-car", "fs-electric", "hybrid-car"):
        for case in runs.example(name)["cases"]:
            from app.schemas import Project
            project = Project.model_validate(runs.example(name))
            next(c for c in project.cases if c.id == case["id"]).mode = "quick"
            with _Env("new"):
                q = simulate(project, case["id"])
            first = q.messages[0].text if q.messages else ""
            rec = {"case": f"{name}/{case['id']}", "status": q.status, "first": first}
            m = re.match(r"Not run \[([A-Z-]+)\]", first)
            if m:
                rec["refused"] = m.group(1)
                rec["listed"] = m.group(1) in new_engine.CODES and m.group(1).startswith("QUICK-")
                res["cases"].append(rec)
                print(f"quick probe {rec['case']}: refused {rec['refused']}", flush=True)
                continue
            rec["ran"] = (q.solverOptions or {}).get("quickStep") is not None
            rec["flags"] = [x.text for x in q.messages if x.level == "warning" and x.text.startswith(FLAG)]
            with _Env("new"):
                full = simulate(Project.model_validate(runs.example(name)), case["id"])
            rec["full_status"] = full.status
            fs = {s.key: s.value for s in full.summary}
            rows = []
            for s in q.summary:
                f = fs.get(s.key)
                row = {"key": s.key, "quick": s.value, "full": f}
                if f is None:
                    row["state"] = "not in the full run"
                else:
                    if s.key.endswith(".final_soc_pct"):
                        soc0 = _soc0(full, s.key.split(".")[0])
                        a, b = soc0 - s.value, soc0 - f
                        row["as"] = "charge used"
                    else:
                        a, b = s.value, f
                    row["rel"] = (a / b - 1.0) if b else (0.0 if a == b else None)
                    row["state"] = ("not judged (flagged)" if rec["flags"] else
                                    "in" if row["rel"] is not None and abs(row["rel"]) <= BAND else "out")
                rows.append(row)
            rec["rows"] = rows
            res["cases"].append(rec)
            out_rows = [r["key"] for r in rows if r["state"] == "out"]
            print(f"quick probe {rec['case']}: ran {rec['ran']}, {len(rec['flags'])} flags, "
                  f"{len(rows) - len(out_rows)} of {len(rows)} in the band"
                  + (f", out: {', '.join(out_rows)}" if out_rows else ""), flush=True)
    res["docs"] = _quick_docs()
    path.write_text(json.dumps(res, indent=1), encoding="utf-8")


def _quick_docs() -> dict:
    """Whether the help page and Known issues name every QUICK-* code."""
    from app.solver import new_engine
    docs = runs.REPO / "docs"
    page = (docs / "help" / "how-to" / "run-in-quick-mode.md").read_text(encoding="utf-8")
    limits = (docs / "KNOWN-LIMITS.md").read_text(encoding="utf-8")
    codes = [c for c in new_engine.CODES if c.startswith("QUICK-")]
    return {"codes": codes, "missing_in_help": [c for c in codes if f"`{c}`" not in page],
            "missing_in_known_limits": [c for c in codes if f"`{c}`" not in limits]}


def quick_line(out: Path) -> dict:
    path = out / "quick.json"
    if not path.exists():
        return {"state": "not measured", "why": "no quick probe (python -m benchmarks.back_to_back quick OUT)"}
    got = json.loads(path.read_text(encoding="utf-8"))
    cases = got["cases"]
    unlisted = [c["case"] for c in cases if "refused" in c and not c["listed"]]
    not_ran = [c["case"] for c in cases if "refused" not in c and not c.get("ran")]
    out_rows = [f"{c['case']}:{r['key']} {r['rel']:+.4%}" if r.get("rel") is not None else f"{c['case']}:{r['key']}"
                for c in cases for r in c.get("rows", []) if r["state"] == "out"]
    missing = [f"{c['case']}:{r['key']}" for c in cases for r in c.get("rows", []) if r["state"] == "not in the full run"]
    docs = got["docs"]
    ok = (not unlisted and not not_ran and not out_rows and not missing
          and not docs["missing_in_help"] and not docs["missing_in_known_limits"]
          and any(c.get("ran") for c in cases))
    return {"state": "pass" if ok else "fail", "cases": len(cases),
            "ran": [c["case"] for c in cases if c.get("ran")],
            "refused": {c["case"]: c["refused"] for c in cases if "refused" in c},
            "unlisted": unlisted, "flagged": {c["case"]: c["flags"] for c in cases if c.get("flags")},
            "out_of_band": out_rows, "not_in_full": missing, "docs": docs, "stamp": got["stamp"]}


# ---- FMU -------------------------------------------------------------------------

def fmu_probe(out: Path) -> None:
    path, now = out / "fmu.json", runs.stamp()
    reference, fmusim = os.environ.get("LIGHTSIM_REFERENCE_FMUS"), os.environ.get("LIGHTSIM_FMUSIM")
    if not (reference and fmusim):
        print("fmu probe: not measured (LIGHTSIM_REFERENCE_FMUS and LIGHTSIM_FMUSIM not set)", flush=True)
        return
    if _done_before(path, now):
        print("fmu probe: done before, at this code")
        return
    sys.path.insert(0, str(runs.BACKEND / "tests"))
    import test_new_engine_fmu as t  # the line's own comparison (and its cases)

    work = out / "fmu"
    work.mkdir(parents=True, exist_ok=True)
    os.environ.setdefault("LIGHTSIM_PROJECTS_DIR", str(work / "user"))
    res = {"stamp": now, "reference": reference, "fmusim": fmusim, "cases": []}
    for which, (h, stop) in sorted(t.CASES.items()):
        with _Env("new"):
            rec = t.compare_reference(Path(reference) / f"{which}.fmu", h, stop, fmusim, work)
        rec["differences"] = rec["differences"][:50]
        res["cases"].append(rec)
        print(f"fmu probe {which}: {rec['state']}", flush=True)
    path.write_text(json.dumps(res, indent=1, default=str), encoding="utf-8")


def fmu_line(out: Path, b: dict) -> dict:
    row = next(r for r in b["items"] if r["id"] == "probe/bev-fmu")
    path = out / "fmu.json"
    if not path.exists():
        return {"state": "not measured", "probe": row["got"],
                "why": "the Reference FMUs and fmusim were not run here (LIGHTSIM_REFERENCE_FMUS and "
                       "LIGHTSIM_FMUSIM not set): the line is measured on Windows CI "
                       "(backend/tests/test_new_engine_fmu.py)"}
    got = json.loads(path.read_text(encoding="utf-8"))
    judged = [c for c in got["cases"] if c["state"] != "nothing to compare"]
    ok = bool(judged) and all(c["state"] == "pass" for c in judged) and row["got"] == "run"
    return {"state": "pass" if ok else "fail", "probe": row["got"],
            "cases": {c["fmu"]: c["state"] for c in got["cases"]},
            "differences": {c["fmu"]: c["differences"][:3] for c in judged if c["state"] != "pass"},
            "stamp": got["stamp"]}
