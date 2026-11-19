#!/usr/bin/env python3
"""The outside judge of fast mode: the inverse model on the example
projects' drive-cycle cases against their full dynamic runs, and on EPA's
reference cars against FASTSim, the U.S. national laboratory's quasi-static
vehicle simulator, given the same cars.

    fast.py cases CASES                        the EPA cars' project files (run with
                                               the backend's Python, as LSIM_PYTHON)
    fast.py cycles CASES                       the battery electric example cars on
                                               every bundled cycle (any Python)
    fast.py run   CASES RESULTS [--runner EXE] [--replay DT] [--perturb REL]
    fast.py score RESULTS --fastsim FASTSIM_JSON [--before RESULTS0] [--out FILE]

`run` takes the runner built by `cargo build --release -p lsim-project
--example fast` (from CARGO_TARGET_DIR, or --runner) and runs every case of
the example projects, every case of the project files `cycles` wrote (each
battery electric example car on each of the 27 bundled cycles) and every
EPA car on the UDDS and HWFET: fast mode, the full dynamic run (rtol 1e-6,
the golden comparison's setting), fast mode again on the speed that run
drove (--replay, every 0.1 s by default: the driver's tracking taken out)
and fast mode again on the cycle changed at round-off level (--perturb,
every speed 1e-9 of itself up or down by turns: no flag may appear or go,
and the charge used may move by a millionth at most; --perturb 0 leaves
it out, for a runner from before it had one). FASTSIM_JSON is what
`benchmarks/fastsim/compare.py fastsim` writes in FASTSim 3.1.0's own
virtualenv; its "aligned" runs are FASTSim given LightSim's inputs
(benchmarks/fastsim/README.md). CASES and RESULTS are folders outside the
repository.
"""
from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
# Fast mode against the full dynamic run of the same car on the same cycle,
# where fast mode raises no flag: the charge used must agree within BAND
# (DESIGN.md section 10: the test car's band). Fixed before scoring.
BAND = 0.01
EXAMPLES = ["aero-bev", "bev-car", "hybrid-car", "fs-electric"]
CYCLES = ["udds", "hwfet"]
# the battery electric example cars, which `cycles` puts on every bundled cycle
BEV = ["aero-bev", "bev-car"]
# a change of the cycle at round-off level moves the charge used by at most this share
PERTURBED = 1e-6


def default_runner() -> Path:
    target = Path(os.environ.get("CARGO_TARGET_DIR", str(HERE.parent / "target")))
    return target / "release" / "examples" / "fast"


def cases(out: Path) -> None:
    """Writes each EPA car of the reference suite as a project file per cycle
    (the suite's own model, `validation.suite.build_project`)."""
    out.mkdir(parents=True, exist_ok=True)
    sys.path.insert(0, str(REPO / "backend"))
    os.chdir(REPO / "backend")
    from validation import suite  # noqa: PLC0415 (the backend's Python only)

    for case in suite.load_cases():
        for cycle in CYCLES:
            proj = suite.build_project(case, cycle)
            path = out / f"{case['id']}-{cycle}.json"
            path.write_text(proj.model_dump_json(by_alias=True), encoding="utf-8")
            target = suite.mpge_to_wh_per_km(case["target"][f"{cycle}_mpge"])
            meta = {"car": case["id"], "name": case["name"], "cycle": cycle,
                    "target_wh_per_km": target}
            path.with_suffix(".meta.json").write_text(json.dumps(meta), encoding="utf-8")
            print(path)


def cycles(out: Path) -> None:
    """Writes each battery electric example car as a project with one
    drive-cycle case per bundled cycle (its first case on a bundled cycle,
    the cycle swapped, over the cycle's whole length)."""
    out.mkdir(parents=True, exist_ok=True)
    folder = REPO / "backend" / "app" / "cycles"
    ids = sorted(p.stem for p in folder.glob("*.csv"))
    for name in BEV:
        proj = json.loads((REPO / "backend" / "projects" / f"{name}.json").read_text(encoding="utf-8"))
        base = next(c for c in proj["cases"]
                    if isinstance(c.get("parameterOverrides", {}).get("el-task", {}).get("cycle"), str))
        cases = []
        for cid in ids:
            rows = [r for r in (folder / f"{cid}.csv").read_text(encoding="utf-8").splitlines() if r.strip()]
            c = json.loads(json.dumps(base))
            c.update(id=f"case-{cid}", name=cid, duration=float(rows[-1].split(",")[0]),
                     parameterOverrides={"el-task": {"cycle": cid}})
            c.pop("references", None)
            cases.append(c)
        proj["cases"] = cases
        path = out / f"{name}-cycles.json"
        path.write_text(json.dumps(proj), encoding="utf-8")
        print(path, len(cases))


def example_cases() -> list[str]:
    tags = []
    for p in EXAMPLES:
        proj = json.loads((REPO / "backend" / "projects" / f"{p}.json").read_text(encoding="utf-8"))
        tags += [f"{p}/{c['id']}" for c in proj.get("cases", [])]
    return tags


def run(cases_dir: Path, results: Path, runner: Path, replay: float, perturb: float) -> None:
    results.mkdir(parents=True, exist_ok=True)
    tags = example_cases()
    for p in sorted(cases_dir.glob("*.json")):
        if p.name.endswith(".meta.json"):
            continue
        many = json.loads(p.read_text(encoding="utf-8")).get("cases")
        tags += [f"{p}/{c['id']}" for c in many] if many else [f"{p}/case"]
    rows = []
    for tag in tags:
        # --perturb 0: no perturbed run (a runner from before it had one)
        cmd = [str(runner), "--forward", "--replay", str(replay)]
        cmd += ["--perturb", str(perturb)] if perturb else []
        cmd.append(tag)
        r = subprocess.run(cmd, capture_output=True, text=True, check=False)
        line = r.stdout.strip().splitlines()[-1] if r.stdout.strip() else ""
        try:
            row = json.loads(line)
        except json.JSONDecodeError:
            row = {"project": tag, "fast": {"error": (r.stderr or "no output").strip()[-400:]}}
        meta = Path(tag.rsplit("/", 1)[0]).with_suffix(".meta.json")
        if meta.is_file():
            row["meta"] = json.loads(meta.read_text(encoding="utf-8"))
        rows.append(row)
        f = row.get("fast", {})
        print(f"{tag[-60:]:60s} {f.get('charge_used', f.get('error', ''))!s:.80}", flush=True)
    (results / "fast.json").write_text(json.dumps(rows, indent=1), encoding="utf-8")


def rel(a, b):
    try:
        return (a - b) / abs(b)
    except (TypeError, ZeroDivisionError):
        return None


def pct(x, digits=2):
    return "" if x is None else f"{100 * x:+.{digits}f} %"


def short(e: str) -> str:
    e = e.split("] ", 1)[-1]
    return e if len(e) < 110 else e[:107] + "..."


def score(results: Path, fastsim: Path | None, before: Path | None, out: Path | None) -> None:
    rows = json.loads((results / "fast.json").read_text(encoding="utf-8"))
    old = {}
    if before:
        for r in json.loads((before / "fast.json").read_text(encoding="utf-8")):
            old[(r["project"], r["case"] if "case" in r else "")] = r
    md = ["## Fast mode against the full dynamic run (example projects)", "",
          f"Charge used over the cycle; inside the band when fast mode raises no flag and "
          f"its charge used is within {100 * BAND:.0f} % of the full dynamic run's, and the "
          f"cycle changed at round-off level (*perturbed*) gives the same flags and a charge "
          f"used within {PERTURBED:.0e} of it. "
          "*Replayed*: fast mode on the speed the full dynamic run drove (every 0.1 s).", ""]
    head = "| case | fast | full dynamic | difference | replayed | perturbed | flags | events (at samples) | steps | fast run | full run | verdict |"
    if old:
        head = head.replace("| verdict |", "| before | verdict |")
    md += [head, "|" + "---|" * (head.count("|") - 1)]
    for r in rows:
        if "meta" in r:
            continue
        tag = f"{r['project']}/{r.get('case', '')}"
        f, w, rp = r.get("fast", {}), r.get("forward", {}), r.get("replay", {})
        if "error" in f:
            md.append(f"| {tag} | {short(f['error'])} |" + " |" * (head.count("|") - 3))
            continue
        d, dr = rel(f["charge_used"], w.get("charge_used")), rel(rp.get("charge_used"), w.get("charge_used"))
        pe = r.get("perturbed")
        dp = rel(pe.get("charge_used"), f["charge_used"]) if pe else None
        same = pe is None or (dp is not None and abs(dp) <= PERTURBED and pe["flags"] == f["flags"])
        ok = not f["flags"] and d is not None and abs(d) <= BAND and same
        pcell = "" if pe is None else (short(pe["error"]) if "error" in pe else
                                       f"{dp:+.1e}" + ("" if pe["flags"] == f["flags"] else ", **flags differ**"))
        cells = [tag, f"{f['charge_used']:.6f}", f"{w.get('charge_used', float('nan')):.6f}", pct(d),
                 pct(dr), pcell, str(len(f["flags"])), f"{f['events']} ({f['events_at_samples']})",
                 str(f["steps"]), f"{1e3 * f['run_s']:.1f} ms", f"{w.get('run_s', float('nan')):.2f} s"]
        if old:
            o = old.get((r["project"], r.get("case", "")), {})
            of = o.get("fast", {})
            ow = o.get("forward", w)
            cells.append("" if not of else short(of["error"]) if "error" in of else
                         f"{pct(rel(of.get('charge_used'), ow.get('charge_used')))}, {of.get('events', '')} events")
        if "error" in w:
            # nothing to judge fast mode against (outside fast mode's scope)
            cells[2] = "stopped: " + short(w["error"])
            cells.append("no reference")
        else:
            cells.append("inside" if ok else "**outside**")
        md.append("| " + " | ".join(cells) + " |")
    fs = {}
    if fastsim:
        for x in json.loads(fastsim.read_text(encoding="utf-8"))["results"]:
            fs[(x["case"], x["cycle"], x["variant"])] = x
    md += ["", "## Fast mode against FASTSim 3.1.0 (EPA's reference cars)", "",
           "Energy at the wall, Wh/km (terminal energy net of recuperation plus the battery's "
           "internal losses, over a charger efficiency of 0.86, per km driven: the reference "
           "suite's count, applied to every tool). FASTSim *with LightSim's inputs* is FASTSim "
           "given the suite's mass, road load, motor, battery and auxiliaries "
           "(benchmarks/fastsim/README.md, section 1b); *own* is its own vehicle file.", ""]
    head = ("| car | cycle | EPA | fast mode | full dynamic | FASTSim, LightSim's inputs | "
            "fast − FASTSim | FASTSim, own file | fast − full dynamic | flags | fast run |")
    if old:
        head = head.replace("| flags |", "| before | flags |")
    md += [head, "|" + "---|" * (head.count("|") - 1)]
    diffs = []
    for r in rows:
        if "meta" not in r:
            continue
        m, f, w = r["meta"], r.get("fast", {}), r.get("forward", {})
        a = fs.get((m["car"], m["cycle"], "aligned"), {}).get("wall_wh_per_km")
        own = fs.get((m["car"], m["cycle"], "own"), {}).get("wall_wh_per_km")
        if "error" in f:
            md.append(f"| {m['name']} | {m['cycle']} | {short(f['error'])} |" + " |" * 8)
            continue
        d = rel(f["wall_wh_per_km"], a)
        if d is not None:
            diffs.append(abs(d))
        cells = [m["name"], m["cycle"].upper(), f"{m['target_wh_per_km']:.1f}",
                 f"{f['wall_wh_per_km']:.2f}", f"{w.get('wall_wh_per_km', float('nan')):.2f}",
                 "" if a is None else f"{a:.2f}", pct(d, 1), "" if own is None else f"{own:.2f}",
                 pct(rel(f["wall_wh_per_km"], w.get("wall_wh_per_km")))]
        if old:
            o = old.get((r["project"], r.get("case", "")), {}).get("fast", {})
            cells.append("" if not o else short(o["error"]) if "error" in o else
                         f"{o.get('wall_wh_per_km', float('nan')):.2f}")
        cells += [str(len(f["flags"])), f"{1e3 * f['run_s']:.1f} ms"]
        md.append("| " + " | ".join(cells) + " |")
    if diffs:
        md += ["", f"Fast mode against FASTSim with LightSim's inputs: mean absolute difference "
               f"{100 * sum(diffs) / len(diffs):.2f} %, largest {100 * max(diffs):.2f} %."]
    text = "\n".join(md) + "\n"
    if out:
        out.write_text(text, encoding="utf-8")
    print(text)


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("cases")
    p.add_argument("cases", type=Path)
    p = sub.add_parser("cycles")
    p.add_argument("cases", type=Path)
    p = sub.add_parser("run")
    p.add_argument("cases", type=Path)
    p.add_argument("results", type=Path)
    p.add_argument("--runner", type=Path, default=None)
    p.add_argument("--replay", type=float, default=0.1)
    p.add_argument("--perturb", type=float, default=1e-9)
    p = sub.add_parser("score")
    p.add_argument("results", type=Path)
    p.add_argument("--fastsim", type=Path)
    p.add_argument("--before", type=Path)
    p.add_argument("--out", type=Path)
    a = ap.parse_args()
    if a.cmd == "cases":
        cases(a.cases.resolve())
    elif a.cmd == "cycles":
        cycles(a.cases.resolve())
    elif a.cmd == "run":
        run(a.cases.resolve(), a.results, a.runner or default_runner(), a.replay, a.perturb)
    elif a.cmd == "score":
        score(a.results, a.fastsim, a.before, a.out)


if __name__ == "__main__":
    main()
