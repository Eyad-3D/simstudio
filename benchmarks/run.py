"""Run the Stage 0 yardstick on today's engine and write the report.

    python -m benchmarks.run                      (from the repository root)
    python -m benchmarks.run --skip-speed         reference problems only (about a minute)
    python -m benchmarks.run --skip-reference --repeats 3
    python -m benchmarks.run --write-baseline     also refresh benchmarks/baselines/
    python -m benchmarks.run --speed-from results/X.json   re-run the problems, keep X's timings
    python -m benchmarks.run --render results/X.json       re-write X.md from X.json

Writes benchmarks/results/<engine>-<date>.md and .json: every reference
problem against its exact answer (or why the engine cannot express it),
with the error at the engine's default solver step and at half of it (the
observed order of convergence), and the speed of the example cars' main
cases against the targets of benchmarks/targets.toml.
"""
from __future__ import annotations

import argparse
import datetime as dt
import json
import math
import sys
from pathlib import Path

from . import speed
from .engines import lightsim_py as engine
from .reference import Problem, load_all, targets
from .reference.compare import Outcome, compare
from .reference.exact import solve

HERE = Path(__file__).parent
RESULTS = HERE / "results"
BASELINE = HERE / "baselines" / f"{engine.NAME}.toml"


# ---- reference problems --------------------------------------------------------------------

def run_reference(problems: list[Problem], half_step: bool = True, progress=print) -> list[dict]:
    rows = []
    for prob in problems:
        if prob.id in engine.CANNOT:
            rows.append({"problem": prob.id, "title": prob.title, "domain": prob.domain,
                         "expressible": False, "why_not": engine.CANNOT[prob.id]})
            progress(f"  {prob.id:26s} cannot be expressed: {engine.CANNOT[prob.id]}")
            continue
        ex = solve(prob)
        for expr in engine.EXPRESSIONS[prob.id]:
            trace = engine.run(prob, expr)
            out = compare(prob, trace, ex)
            row = {"problem": prob.id, "title": prob.title, "domain": prob.domain,
                   "expressible": True, "expression": expr.label, "how": expr.how,
                   "outcome": out.to_dict(), "wall_s": trace.wall_s, "steps": trace.steps,
                   "simulated_s": trace.sim_s, "status": trace.status,
                   "messages": trace.messages, "step_s": _solver_step(trace)}
            if half_step:
                fine = engine.run(prob, expr, step=engine.MAX_STEP / 2)
                out2 = compare(prob, fine, ex)
                row["half_step"] = {"step_s": _solver_step(fine), "outcome": out2.to_dict()}
                row["order"] = _order(out, out2, row["step_s"], row["half_step"]["step_s"])
            rows.append(row)
            worst = out.worst_rel
            progress(f"  {prob.id:26s} [{expr.label}] worst {_fmt(worst)}, "
                     f"{'passes' if out.passed else 'misses'} the target, {trace.wall_s:.2f} s")
    return rows


def _solver_step(trace) -> float | None:
    return trace.sim_s / trace.steps if trace.steps else None


def _signal_worst(out: Outcome) -> float | None:
    rels = [r.error_rel for r in out.rows if r.kind == "signal" and r.error_rel is not None]
    return max(rels) if rels else None


def _order(a: Outcome, b: Outcome, h_a: float | None, h_b: float | None) -> float | None:
    """The observed order of convergence of the worst signal error."""
    ea, eb = _signal_worst(a), _signal_worst(b)
    if not ea or not eb or not h_a or not h_b or abs(h_a - h_b) < 1e-12 * h_a:
        return None
    return math.log(ea / eb) / math.log(h_a / h_b)


def _fmt(x: float | None, digits: int = 2) -> str:
    if x is None:
        return "-"
    if x == 0:
        return "0"
    return f"{x:.{digits}e}"


# ---- the report ------------------------------------------------------------------------------

def markdown(meta: dict, ref: list[dict], spd: speed.SpeedReport | None) -> str:
    acc, sp = targets()["accuracy"], targets()["speed"]
    L = [f"# LightSim yardstick: {meta['engine']['engine']} ({meta['date']})", "",
         f"Engine **{meta['engine']['engine']}** {meta['engine']['version']} (commit "
         f"`{meta['engine']['commit']}`), default solver step "
         f"{meta['engine']['default_step_s'] * 1000:g} ms. Machine: {meta['machine']['cpu']}, "
         f"{meta['machine']['cpus']} CPUs, load average at the start "
         f"{meta['machine']['load_at_start'][0]:.2f}, Python {meta['machine']['python']}.", "",
         "Targets (benchmarks/targets.toml): a signal within "
         f"{acc['signal_rtol']:g} of its scale, an event within {acc['event_atol_s'] * 1000:g} ms "
         f"+ {acc['event_rtol']:g} of its time, an energy term within {acc['energy_rtol']:g} and "
         f"the energy balance within {acc['closure_rtol']:g} of the energy scale; at least "
         f"{sp['dynamic_min_x_realtime']:g}x real time in full dynamic simulation and about "
         f"{sp['fast_wltc_x_realtime']:.0e}x on the WLTC in a fast mode.", ""]
    if ref:
        L += ["## Reference problems", "",
              "Errors are shares of each quantity's scale (see benchmarks/README.md). "
              "*Order* is the observed order of convergence of the worst signal error between "
              "the default step and half of it: about 1 for a first-order integrator, near 0 "
              "when the error comes from how the model is built rather than from the step.", "",
              "| Problem | Expressed as | Worst signal | Worst event | Worst energy | Energy "
              "balance | Target | Wall, s | x real time | Steps | Order |",
              "|---|---|---|---|---|---|---|---|---|---|---|"]
        for r in ref:
            if not r["expressible"]:
                L.append(f"| {r['problem']} | cannot be expressed | | | | | | | | | |")
                continue
            o = r["outcome"]

            def worst(kind, o=o):
                rels = [x["error_rel"] for x in o["rows"] if x["kind"] == kind]
                if any(x["kind"] == kind and x["passed"] is False and x["error_rel"] is None
                       for x in o["rows"]):
                    return "never reached"
                rels = [x for x in rels if x is not None]
                return _fmt(max(rels)) if rels else "-"
            order = r.get("order")
            L.append(f"| {r['problem']} | {r['expression']} | {worst('signal')} | "
                     f"{worst('event')} | {worst('energy')} | {_fmt(o['closure_rel'])} | "
                     f"{'pass' if o['passed'] else 'miss'} | {r['wall_s']:.2f} | "
                     f"{r['simulated_s'] / r['wall_s']:.0f} | {r['steps'] or '-'} | "
                     f"{'-' if order is None else f'{order:.2f}'.replace('-0.00', '0.00')} |")
        cannot = [r for r in ref if not r["expressible"]]
        if cannot:
            L += ["", "Today's engine cannot express:", ""]
            L += [f"- **{r['problem']}** ({r['title']}): {r['why_not']}." for r in cannot]
        for r in ref:
            if not r["expressible"]:
                continue
            o = r["outcome"]
            L += ["", f"### {r['problem']}: {r['title']} [{r['expression']}]", "",
                  f"Expressed as: {r['how']}. Solver step {(r['step_s'] or 0) * 1000:.3g} ms, "
                  f"{r['steps']} steps, {r['wall_s']:.3f} s wall "
                  f"({r['simulated_s'] / r['wall_s']:.0f}x real time).", "",
                  "| Quantity | Kind | Largest error | RMS error | Share of scale | Tolerance "
                  "| Exact | Engine | Pass | Half step: share |",
                  "|---|---|---|---|---|---|---|---|---|---|"]
            half = {x["name"]: x for x in r.get("half_step", {}).get("outcome", {}).get("rows", [])}
            for x in o["rows"]:
                passed = {True: "yes", False: "no", None: "not given"}[x["passed"]]
                if x["passed"] is False and x["error_rel"] is None:
                    passed = "never reached"
                at = f" at {x['at']:g} s" if x.get("at") is not None else ""
                h = half.get(x["name"], {}).get("error_rel")
                L.append(f"| {x['name']} | {x['kind']} | {_fmt(x['error_max'], 3)}{at} | "
                         f"{_fmt(x['error_rms'], 3)} | {_fmt(x['error_rel'])} | "
                         f"{_fmt(x['tolerance'], 2)} | {_fmt(x['exact'], 6)} | "
                         f"{_fmt(x['value'], 6)} | {passed} | {_fmt(h)} |")
            L.append(f"| energy balance | closure | | | {_fmt(o['closure_rel'])} | "
                     f"{o['closure_tol']:g} | | | "
                     f"{ {True: 'yes', False: 'no', None: 'not given'}[o['closure_passed']] } | |")
            if r["messages"]:
                L += ["", "Engine messages: " + "; ".join(sorted(set(r["messages"])))[:1500]]
    if spd is not None:
        L += ["", "## Speed", "",
              f"Median of {spd.targets['repeats']} warm runs (one warm-up run first, untimed), "
              "wall clock, one process. *CPU share* is the lowest CPU time / wall time of the "
              "runs (well under 1: the run waited for a CPU; above 1: a worker process, such "
              "as a Script block's sandbox, ran alongside and its CPU time is counted). Steps "
              "are the solver's (master) steps of one run; *Load* is the machine's one-minute "
              "load average around the case (other jobs included).", "",
              "| Case | Kind | Simulated, s | Median wall, s | x real time | Steps | Steps/s | "
              f"CPU share | Load before -> after | >= {spd.targets['dynamic_min_x_realtime']:g}x |",
              "|---|---|---|---|---|---|---|---|---|---|"]
        for c in spd.cases:
            ok = "yes" if c.x_realtime >= spd.targets["dynamic_min_x_realtime"] else "no"
            L.append(f"| {c.label} | {c.kind} | {c.simulated_s:g} | {c.wall_s:.3f} | "
                     f"{c.x_realtime:.1f} | {c.steps} | {c.steps_per_s:.0f} | {c.cpu_share:.2f} | "
                     f"{c.load_before[0]:.2f} -> {c.load_after[0]:.2f} | {ok} |")
        wltc = [c for c in spd.cases if c.fast_target]
        if wltc:
            L += ["", f"Fast mode target (about {spd.targets['fast_wltc_x_realtime']:.0e}x real "
                  f"time on the WLTC): today's engine has no fast mode for drive cycles; its full "
                  f"run of {wltc[0].label} is {wltc[0].x_realtime:.1f}x, "
                  f"{spd.targets['fast_wltc_x_realtime'] / wltc[0].x_realtime:,.0f} times short "
                  f"of it."]
    return "\n".join(L) + "\n"


# ---- the baseline (the ratchet tests/test_reference_problems.py holds today's engine to) ------

def _ceiling(x: float) -> str:
    """2 x the measured error, rounded up to two significant digits, and
    never below 1e-9 (rounding noise, not a measured error)."""
    y = max(2.0 * x, 1e-9)
    mag = 10 ** math.floor(math.log10(y))
    return f"{math.ceil(round(y / mag * 10, 9)) / 10 * mag:.2g}"


def write_baseline(ref: list[dict], path: Path = BASELINE) -> None:
    lines = [f"# Today's engine ({engine.NAME}) on the reference problems: the ceiling of every",
             "# error (a share of its scale, see benchmarks/README.md), twice what was measured.",
             "# backend/tests/test_reference_problems.py fails when an error grows past its",
             "# ceiling. When the engine gets more accurate, regenerate this file with",
             "#   python -m benchmarks.run --skip-speed --write-baseline",
             "# and commit it. \"never\" marks an event the engine does not reach today.", ""]
    for r in ref:
        if not r["expressible"]:
            continue
        lines.append(f"[{r['problem']}.\"{r['expression']}\"]")
        o = r["outcome"]
        for x in o["rows"]:
            if x["error_rel"] is not None:
                lines.append(f"{x['name']} = {_ceiling(x['error_rel'])}  "
                             f"# measured {x['error_rel']:.3g}")
            elif x["passed"] is False:
                lines.append(f"{x['name']} = \"never\"")
        if o["closure_rel"] is not None:
            lines.append(f"closure = {_ceiling(o['closure_rel'])}  "
                         f"# measured {o['closure_rel']:.3g}")
        lines.append("")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines), encoding="utf-8")


def _speed_of(data: dict) -> speed.SpeedReport | None:
    """The speed part of a JSON report, as a SpeedReport."""
    sp = data.get("speed")
    if not sp:
        return None
    fields = speed.CaseSpeed.__dataclass_fields__
    return speed.SpeedReport(machine=sp["machine"], targets=sp["targets"], cases=[
        speed.CaseSpeed(**{k: v for k, v in c.items() if k in fields}) for c in sp["cases"]])


def render(path: Path) -> int:
    """Write the Markdown report next to a JSON one (after a change of the
    report's layout, without running anything again)."""
    data = json.loads(path.read_text(encoding="utf-8"))
    path.with_suffix(".md").write_text(markdown(data, data["reference"], _speed_of(data)),
                                       encoding="utf-8")
    print(f"wrote {path.with_suffix('.md')}")
    return 0


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description="LightSim Stage 0 yardstick")
    ap.add_argument("--skip-reference", action="store_true")
    ap.add_argument("--skip-speed", action="store_true")
    ap.add_argument("--problems", help="comma-separated problem ids")
    ap.add_argument("--cases", help="comma-separated speed case labels")
    ap.add_argument("--repeats", type=int)
    ap.add_argument("--no-half-step", action="store_true")
    ap.add_argument("--write-baseline", action="store_true")
    ap.add_argument("--out", type=Path, default=RESULTS)
    ap.add_argument("--tag", default="", help="added to the report's file name")
    ap.add_argument("--render", type=Path, help="only write the Markdown of this JSON report")
    ap.add_argument("--speed-from", type=Path,
                    help="take the speed part from this JSON report instead of timing again")
    args = ap.parse_args(argv)
    if args.render:
        return render(args.render)

    date = dt.date.today().isoformat()
    meta = {"date": date, "engine": engine.version(), "machine": speed.machine(),
            "targets": targets()}
    ref: list[dict] = []
    if not args.skip_reference:
        wanted = set(args.problems.split(",")) if args.problems else None
        probs = [p for p in load_all() if wanted is None or p.id in wanted]
        print(f"Reference problems on {engine.NAME}:")
        ref = run_reference(probs, half_step=not args.no_half_step)
        if args.write_baseline:
            write_baseline(ref)
            print(f"wrote {BASELINE}")
    spd = None
    if args.speed_from:
        spd = _speed_of(json.loads(args.speed_from.read_text(encoding="utf-8")))
    elif not args.skip_speed:
        print(f"Speed on {engine.NAME}:")
        spd = speed.run(args.repeats, args.cases.split(",") if args.cases else None)
    args.out.mkdir(parents=True, exist_ok=True)
    stem = f"{engine.NAME}-{date}{'-' + args.tag if args.tag else ''}"
    data = {**meta, "reference": ref,
            "speed": None if spd is None else {"machine": spd.machine, "targets": spd.targets,
                                               "cases": [c.to_dict() for c in spd.cases]}}
    (args.out / f"{stem}.json").write_text(json.dumps(data, indent=1, default=str),
                                            encoding="utf-8")
    (args.out / f"{stem}.md").write_text(markdown(meta, ref, spd), encoding="utf-8")
    print(f"wrote {args.out / stem}.md and .json")
    return 0


if __name__ == "__main__":
    sys.exit(main())
