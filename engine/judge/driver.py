#!/usr/bin/env python3
"""The library Driver's outside judge: a car run at every mass on every
driving cycle (the map), each driven trace judged by the US EPA's rule for
chassis-dynamometer tests, 40 CFR 1066.425(b), and the run's outcome by
whether it ends with a named diagnostic.

    driver.py cells CACHE RUNS.jsonl [...] --out CELLS.jsonl
    driver.py score CACHE RUNS.jsonl [...] [--before RUNS0.jsonl ...] [--today TODAY.jsonl ...]
                    [--out FILE]
    driver.py today PROJECT CASE OUT.jsonl --masses m1,m2 --cycles c1,c2 [--dt 0.1]

RUNS are what the engine's `driver_map` example writes (one JSON line a
run, with its driven trace: `cargo run --release -p lsim-project --example
driver_map -- PROJECT CASE OUT.jsonl --masses ... --cycles ...`); `today`
runs the same cases on today's engine (run it with the backend's Python),
so `score --today` puts its verdicts beside the new engine's. `cells`
judges the runs and keeps each one's verdict without its trace (a map's
traces take some 300 MB); `score` takes such cells as well as runs. CACHE
holds the rule's text (`judge.py fetch`). Standard library only.

The rule, (b)(1)-(2): the speed must stay within 2.0 mi/h (3.218688 km/h)
above the highest and below the lowest point of the schedule within 1.0 s
of each instant. (b)(4)(i): an excursion beyond them is allowed for up to
2.0 s on any occasion. (b)(4)(ii), with 1066.415(e)(5): a car that cannot
accelerate at the specified rate is driven "at maximum available power
until the vehicle speed reaches the value prescribed", and that shortfall
does not count. Here a point below the lower limit is excused while the
accelerator is fully down (the Driver's command at full throttle); an
occasion longer than 2.0 s that is left counts against the trace: a
"brakes" occasion if the car was too fast with the brake pedal fully down
throughout (the car cannot brake as the schedule asks, which the rule does
not excuse but no driver could help), else a "driver" occasion. An
occasion's length is its output points times the output step (0.1 s), so
it is known to a step.
"""
from __future__ import annotations

import argparse
import json
import math
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import judge  # noqa: E402

MPH = 1.609344  # km/h
TOL_KMH = 2.0 * MPH  # (b)(1), (b)(2)
WINDOW_S = 1.0  # (b)(1), (b)(2): within 1.0 s of the given time
ALLOWED_S = 2.0  # (b)(4)(i)
FULL = 1.0 - 1e-9  # a pedal fully down
STILL_KMH = 0.5  # standing still
STOOD_S = 10.0  # standing at full throttle that long, the car has given out
RULE = ["cfr/CFR-2024-title40-vol37-sec1066-425.xml", "cfr/CFR-2024-title40-vol37-sec1066-415.xml"]
CYCLES = judge.ENGINE.parent / "backend" / "app" / "cycles"


def limits(target: list[float], dt: float) -> tuple[list[float], list[float]]:
    """The band at each output time: 2 mi/h above the highest and below the
    lowest point of the schedule within 1.0 s (the schedule is linear
    between its points, which are on the output grid, so its extremes over
    a window are at grid points)."""
    w = round(WINDOW_S / dt)
    n = len(target)
    lo, hi = [], []
    for k in range(n):
        seg = target[max(0, k - w):min(n, k + w + 1)]
        lo.append(min(seg) - TOL_KMH)
        hi.append(max(seg) + TOL_KMH)
    return lo, hi


def score_trace(dt: float, speed: list[float], target: list[float], accel: list[float],
                brake: list[float]) -> dict:
    """A driven trace against the rule: its occasions outside the band
    (each a run of output points, its length the points times dt), which
    count (longer than 2.0 s, not excused), the time excused at full
    throttle, and the verdict: "pass", "brakes" (only occasions too fast at
    full brake count) or "driver"; "stood" for a pass that relied on the
    allowance for a car at its maximum available power while it stood
    still at full throttle for more than STOOD_S with the schedule moving
    (its battery empty: a test would end there)."""
    lo, hi = limits(target, dt)
    occasions, cur = [], None
    excused = stood = 0.0
    worst_under = worst_over = 0.0
    for k, v in enumerate(speed):
        side = None
        if v < lo[k]:
            worst_under = max(worst_under, lo[k] - v)
            if accel[k] >= FULL:
                excused += dt  # (b)(4)(ii): behind at maximum available power
                if abs(v) < STILL_KMH and target[k] > TOL_KMH:
                    stood += dt
            else:
                side = "under"
        elif v > hi[k]:
            worst_over = max(worst_over, v - hi[k])
            side = "over"
        if side and cur and cur["side"] == side and cur["end"] == k - 1:
            cur["end"] = k
            cur["at_limit"] &= side == "over" and brake[k] >= FULL
        elif side:
            cur = {"side": side, "start": k, "end": k,
                   "at_limit": side == "over" and brake[k] >= FULL}
            occasions.append(cur)
    counted = []
    for o in occasions:
        length = (o["end"] - o["start"] + 1) * dt
        if length > ALLOWED_S + 1e-9:
            counted.append({"side": o["side"], "t_start": o["start"] * dt, "length_s": length,
                            "kind": "brakes" if o["at_limit"] else "driver"})
    if not counted:
        verdict = "stood" if stood > STOOD_S else "pass"
    elif all(c["kind"] == "brakes" for c in counted):
        verdict = "brakes"
    else:
        verdict = "driver"
    return {"verdict": verdict, "counted": counted, "occasions": len(occasions),
            "excused_s": excused, "stood_s": stood, "worst_under_kmh": worst_under,
            "worst_over_kmh": worst_over}


def schedule_check(run: dict, cycles: Path) -> float | None:
    """The largest difference of the run's target from the cycle's own
    points (km/h): the band is the published schedule's."""
    if run["cycle"] == "own":
        return None
    pts = {}
    for line in (cycles / f"{run['cycle']}.csv").read_text().splitlines()[1:]:
        t, v = line.split(",")[:2]
        pts[round(float(t))] = float(v)
    dt, tgt = run["dt"], run["target_kmh"]
    worst = 0.0
    for k, v in enumerate(tgt):
        t = k * dt
        if abs(t - round(t)) < 1e-9 and round(t) in pts:
            worst = max(worst, abs(v - pts[round(t)]))
    return worst


def outcome(run: dict, cycles: Path) -> dict:
    """A run's cell of the map: how it ended, and its trace's verdict."""
    key = {"cycle": run["cycle"], "mass_kg": run["mass_kg"], "engine": run.get("engine", "new")}
    if not run["ok"]:
        e = run["error"]
        stop = ("storm" if "event storm" in e else
                "flag" if "SUNDIALS flag" in e else
                "no progress" if "no progress" in e or "made no progress" in e else "stop")
        return {**key, "cell": stop, "error": e, "cpu_s": run.get("cpu_s")}
    tr = score_trace(run["dt"], run["speed_kmh"], run["target_kmh"], run["accel_pedal"],
                     run["brake_pedal"])
    out = {**key, "cell": tr["verdict"], **tr, "schedule_dev_kmh": schedule_check(run, cycles)}
    for k in ("cpu_s", "steps", "events", "err_test_fails", "books_closure", "figures"):
        out[k] = run.get(k)
    return out


SYMBOL = {"pass": "✓", "stood": "e", "brakes": "b", "driver": "✗", "storm": "S", "flag": "F",
          "no progress": "P", "stop": "E", None: " "}


def read_cells(paths: list[Path], cycles: Path) -> dict:
    """Each run's cell by (cycle, mass): judged here from its trace, or as
    `cells` judged it (a line with a "cell"); a later line for the same run
    replaces an earlier one."""
    out = {}
    for p in paths:
        with p.open() as f:
            for line in f:
                r = json.loads(line)
                out[(r["cycle"], r["mass_kg"])] = r if "cell" in r else outcome(r, cycles)
    return out


def cells(cache: Path, runs: list[Path], out: Path) -> None:
    for rel in RULE:
        judge.cached(cache, rel)
    with out.open("a") as f:
        for v in read_cells(runs, CYCLES).values():
            f.write(json.dumps(v) + "\n")


def map_table(cells: dict, cycles: list[str], masses: list[float]) -> list[str]:
    md = ["| t | " + " | ".join(cycles) + " |", "|---|" + "---|" * len(cycles)]
    for m in masses:
        row = [SYMBOL[cells.get((c, m), {}).get("cell")] for c in cycles]
        md.append(f"| {m / 1000:g} | " + " | ".join(row) + " |")
    return md


def counts(cells: dict) -> dict:
    c: dict[str, int] = {}
    for v in cells.values():
        c[v["cell"]] = c.get(v["cell"], 0) + 1
    return c


def score(cache: Path, runs: list[Path], before: list[Path], today: list[Path],
          out: Path | None) -> None:
    for rel in RULE:
        judge.cached(cache, rel)  # the rule's text is the one cited
    after = read_cells(runs, CYCLES)
    prior = read_cells(before, CYCLES)
    cycles = sorted({c for c, _ in after}, key=lambda c: (c == "own", c))
    masses = sorted({m for _, m in after})
    md = ["## The Driver's map: the car at every mass on every cycle\n",
          "✓ inside 40 CFR 1066.425(b) with its allowances; e inside, but only by its "
          "allowance for a car at maximum available power, having stood still at full "
          "throttle for more than 10 s while the schedule moved (its battery empty); "
          "b outside only where the car "
          "was too fast with the brake pedal fully down (it cannot brake as the schedule "
          "asks); ✗ outside where the Driver did not push a pedal to its limit; S the run "
          "stopped with an event storm; F with a bare SUNDIALS flag; P made no progress; "
          "E another stop. Columns: the bundled cycles, and `own` the case's own trace.\n"]
    for title, cells in (("Before", prior), ("After", after)):
        if not cells:
            continue
        c = counts(cells)
        md.append(f"### {title}: " + ", ".join(f"{SYMBOL[k]} {n}" for k, n in sorted(c.items()))
                  + f" of {len(cells)}\n")
        md += map_table(cells, cycles, masses)
        md.append("")
    # the solver's work on the runs that ran on both
    both = [k for k in after if k in prior and "steps" in after[k] and "steps" in prior[k]
            and after[k]["steps"] is not None and prior[k]["steps"] is not None]
    if both:
        tot = lambda d, key: sum(d[k][key] or 0 for k in both)  # noqa: E731
        md += ["### The runs that ran before and after\n",
               "| | runs | steps | error-test failures | events | CPU s |", "|---|---|---|---|---|---|",
               f"| before | {len(both)} | {tot(prior, 'steps')} | {tot(prior, 'err_test_fails')} | "
               f"{tot(prior, 'events')} | {tot(prior, 'cpu_s'):.1f} |",
               f"| after | {len(both)} | {tot(after, 'steps')} | {tot(after, 'err_test_fails')} | "
               f"{tot(after, 'events')} | {tot(after, 'cpu_s'):.1f} |", ""]
    stops = [(k, v) for k, v in after.items() if v["cell"] in ("storm", "flag", "no progress", "stop")]
    if stops:
        md += ["### The runs that stop (after)\n", "| cycle | t | message |", "|---|---|---|"]
        for (c, m), v in sorted(stops, key=lambda x: (x[0][0], x[0][1])):
            md.append(f"| {c} | {m / 1000:g} | {' '.join(v['error'].split())[:220]} |")
        md.append("")
    drivers = [(k, v) for k, v in after.items() if v["cell"] == "driver"]
    if drivers:
        md += ["### The traces outside the rule where the Driver was not at a limit (after)\n",
               "| cycle | t | counted occasions (side, start s, length s) |", "|---|---|---|"]
        for (c, m), v in sorted(drivers):
            occ = "; ".join(f"{o['side']} {o['t_start']:.1f} {o['length_s']:.1f}"
                            for o in v["counted"] if o["kind"] == "driver")
            md.append(f"| {c} | {m / 1000:g} | {occ} |")
        md.append("")
    dev = [v["schedule_dev_kmh"] for v in after.values() if v.get("schedule_dev_kmh") is not None]
    if dev:
        md.append(f"The runs' targets are the cycles' published points to {max(dev):.1e} km/h.\n")
    if today:
        t_runs = read_cells(today, CYCLES)
        md += ["### Today's engine beside the new one\n",
               "Today's engine: semi-implicit Euler at 10 ms, its Driver's integral frozen "
               "while the command is held at a limit it pushes against. Occasions: side, "
               "start s + length s, kind. Worst: how far below the lower and above the upper "
               "limit the speed went, km/h (excused or not).\n",
               "| cycle | t | today | new | today: counted occasions | new: counted occasions | "
               "worst under / over, today | new | kWh/100 km, today | new | CPU s, today | new |",
               "|---|---|---|---|---|---|---|---|---|---|---|---|"]
        occ = lambda v: "; ".join(f"{o['side']} {o['t_start']:.0f}+{o['length_s']:.1f} ({o['kind']})"  # noqa: E731
                                  for o in v.get("counted", [])) or "–"
        worst = lambda v: (f"{v['worst_under_kmh']:.2f} / {v['worst_over_kmh']:.2f}"  # noqa: E731
                           if "worst_under_kmh" in v else "–")
        cons = lambda v: judge.fmt((v.get("figures") or {}).get("consumption_kwh_per_100km"), ".4f")  # noqa: E731
        for k in sorted(t_runs):
            a, n = t_runs[k], after.get(k, {})
            md.append(f"| {k[0]} | {k[1] / 1000:g} | {SYMBOL[a['cell']]} | {SYMBOL[n.get('cell')]} | "
                      f"{occ(a)} | {occ(n)} | {worst(a)} | {worst(n)} | {cons(a)} | {cons(n)} | "
                      f"{judge.fmt(a.get('cpu_s'), '.1f')} | {judge.fmt(n.get('cpu_s'), '.2f')} |")
        md.append("")
    text = "\n".join(md)
    if out:
        out.write_text(text)
        out.with_suffix(".json").write_text(json.dumps(
            {"after": {f"{c}|{m}": v for (c, m), v in after.items()},
             "before": {f"{c}|{m}": v for (c, m), v in prior.items()}}, indent=1))
    else:
        print(text)


def today(project: Path, case_id: str, out: Path, masses: list[float], cycles: list[str],
          dt: float) -> None:
    """The same runs on today's engine (its 10 ms step), its trace on the
    same grid: run with the backend's Python, from anywhere."""
    import copy
    import time
    backend = judge.ENGINE.parent / "backend"
    sys.path.insert(0, str(backend))
    from app.schemas import Project  # noqa: E402
    from app.solver import core  # noqa: E402
    from app.solver.keys import fill_keys  # noqa: E402

    raw = json.loads(project.read_text())
    ids = {}
    for s in raw["systems"]:
        for el in s["elements"]:
            ids.setdefault(el["componentDefId"], el["id"])
    veh, task, drv = ids["vehicle.body"], ids["signal.driving_task"], ids["driver.driver"]
    with out.open("a") as f:
        for cycle in cycles:
            for mass in masses:
                data = copy.deepcopy(raw)
                case = next(c for c in data["cases"] if c["id"] == case_id)
                over = case.setdefault("parameterOverrides", {})
                over.setdefault(veh, {})["mass_kg"] = mass
                if cycle != "own":
                    over.setdefault(task, {})["cycle"] = cycle
                    csv = (backend / "app" / "cycles" / f"{cycle}.csv").read_text().splitlines()
                    case["duration"] = float(csv[-1].split(",")[0])
                case["timeStep"] = dt
                case["realtimeFactor"] = 0
                started = time.process_time()
                res = core.run_case(Project.model_validate(data), case_id)
                cpu = time.process_time() - started
                fill_keys(res.summary, Project.model_validate(data))
                ch = {f"{c.elementId}:{c.portId}": [p["value"] for p in c.timeSeries]
                      for c in res.channels}
                line = {"cycle": cycle, "mass_kg": mass, "engine": "today", "dt": dt,
                        "cpu_s": cpu, "ok": res.status != "failed",
                        "messages": [m.text for m in res.messages],
                        "figures": {s.key: s.value for s in res.summary if s.key},
                        "speed_kmh": ch.get(f"{veh}:sig_speed"),
                        "target_kmh": ch.get(f"{task}:sig_demand"),
                        "accel_pedal": ch.get(f"{drv}:sig_accel_pedal"),
                        "brake_pedal": ch.get(f"{drv}:sig_brake_pedal")}
                if not line["ok"]:
                    line["error"] = " | ".join(line["messages"])
                f.write(json.dumps(line) + "\n")
                print(f"{cycle} {mass} kg: {res.status} ({cpu:.1f} s CPU)", file=sys.stderr)


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("score")
    p.add_argument("cache", type=Path)
    p.add_argument("runs", type=Path, nargs="+")
    p.add_argument("--before", type=Path, nargs="*", default=[])
    p.add_argument("--today", type=Path, nargs="*", default=[])
    p.add_argument("--out", type=Path)
    p = sub.add_parser("cells")
    p.add_argument("cache", type=Path)
    p.add_argument("runs", type=Path, nargs="+")
    p.add_argument("--out", type=Path, required=True)
    p = sub.add_parser("today")
    p.add_argument("project", type=Path)
    p.add_argument("case")
    p.add_argument("out", type=Path)
    p.add_argument("--masses", required=True)
    p.add_argument("--cycles", required=True)
    p.add_argument("--dt", type=float, default=0.1)
    a = ap.parse_args()
    if a.cmd == "score":
        score(a.cache, a.runs, a.before, a.today, a.out)
    elif a.cmd == "cells":
        cells(a.cache, a.runs, a.out)
    else:
        today(a.project, a.case, a.out, [float(m) for m in a.masses.split(",")],
              a.cycles.split(","), a.dt)


if __name__ == "__main__":
    main()
