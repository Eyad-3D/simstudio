"""The comparison: which model and effort level is the cheapest that drives
LightSim successfully (the owner's question of 30 October). README.md in
this folder says how to run it.

    python -m benchmarks.agent.compare --out DIR [--name NAME] [--stage all] [--jobs 3]

A setting is a model and an effort level. Stage A runs every setting on
every task once, through LightSim's MCP server. Stage B gives two more tries
(three in all) to the ceiling, to the target (Sonnet 5.5 at medium effort)
and to every setting that solved at least the ceiling's count minus 5 in
stage A. Stage C runs the command line once on every task, for the cheapest
working setting and the ceiling. Tries go task by task across the settings,
``--jobs`` of them side by side.

Each try's record is written when it ends (``tries/<id>.json``), and running
the same comparison again skips the finished ones. A try stopped by a plan
or rate limit, an overloaded or network error, or the runner's own fault is
not scored: it is noted in ``notrun.jsonl`` and retried (after the limit's
reset time, else a back-off of up to 30 minutes). A setting the plan or the
tool refuses is recorded as unavailable and not run again. The report
(``report.md`` and ``report.json``) and its two answers are worked out from
the records.
"""
from __future__ import annotations

import argparse
import collections
import json
import os
import shutil
import sys
import tempfile
import threading
import time
import traceback
from dataclasses import dataclass
from pathlib import Path
from typing import Optional

from . import KINDS, ROOT, Task, heldout, isolate, load_tasks
from .run import CAP_ENV, SCORED, preflight, run_try


@dataclass(frozen=True)
class Setting:
    name: str
    model: str
    effort: str
    label: str


#: cheapest first; the last is the ceiling, which tells a task that is too
#: hard or broken from a model that is too weak
SETTINGS = (
    Setting("haiku-low", "claude-haiku-5-5", "low", "Haiku 5.5, low"),
    Setting("haiku-medium", "claude-haiku-5-5", "medium", "Haiku 5.5, medium"),
    Setting("haiku-high", "claude-haiku-5-5", "high", "Haiku 5.5, high"),
    Setting("sonnet-low", "claude-sonnet-5-5", "low", "Sonnet 5.5, low"),
    Setting("sonnet-medium", "claude-sonnet-5-5", "medium", "Sonnet 5.5, medium"),
    Setting("sonnet-high", "claude-sonnet-5-5", "high", "Sonnet 5.5, high"),
    Setting("sonnet-xhigh", "claude-sonnet-5-5", "xhigh", "Sonnet 5.5, extra-high"),
    Setting("opus-high", "claude-opus-5-5", "high", "Opus 5.5, high"),
)
CEILING, TARGET = "opus-high", "sonnet-medium"
TRIES = 3            # pass^3
STAGE_B_MARGIN = 5   # stage B: solved at least the ceiling's count minus this in stage A
WORKING_MARGIN = 2   # working: pass^3 at most this many tasks below the ceiling's
HANDS_OFF = 0.9      # hands-off: pass^3 on this share of all tasks and of the held-out ones
BACKOFF_S = 60.0     # the first wait after a limit with no reset time; it doubles, up to
MAX_WAIT_S = 1800.0  # 30 minutes
LIMIT_MARGIN_S = 30.0  # how long after a limit's reset time the next try starts
GIVE_UP = {"runner": 3, "service": 10}  # not-run tries of one kind before the comparison stops


@dataclass(frozen=True)
class Spec:
    task: Task
    mode: str
    setting: Setting
    n: int

    @property
    def id(self) -> str:
        return f"{self.task.id}.{self.mode}.{self.setting.name}.{self.n}"


# -- the stages ----------------------------------------------------------------------
def stage_a(tasks: list[Task], settings: list[Setting]) -> list[Spec]:
    return [Spec(t, "mcp", s, 1) for t in tasks for s in settings]


def stage_b_settings(records: dict, tasks: list[Task], settings: list[Setting]) -> list[Setting]:
    """The ceiling, the target, and every setting that solved at least the
    ceiling's stage A count minus ``STAGE_B_MARGIN``; none that is unavailable."""
    ids, out = {t.id for t in tasks}, unavailable(records)

    def solved(name: str) -> int:
        return sum(r["class"] == "solved" for r in records.values()
                   if r["setting"] == name and r["mode"] == "mcp" and r["try"] == 1 and r["task"] in ids)

    floor = solved(CEILING) - STAGE_B_MARGIN
    return [s for s in settings if s.name not in out
            and (s.name in (CEILING, TARGET) or solved(s.name) >= floor)]


def stage_b(tasks: list[Task], settings: list[Setting]) -> list[Spec]:
    return [Spec(t, "mcp", s, n) for t in tasks for s in settings for n in range(2, TRIES + 1)]


def stage_c(tasks: list[Task], settings: list[Setting]) -> list[Spec]:
    return [Spec(t, "cli", s, 1) for t in tasks for s in settings]


def unavailable(records: dict) -> dict[str, str]:
    return {r["setting"]: r["why"] for r in records.values() if r["class"] == "unavailable"}


def finished(specs: list[Spec], records: dict) -> bool:
    out = unavailable(records)
    return all(s.id in records or s.setting.name in out for s in specs)


def plan(stage: str, records: dict, tasks: list[Task], settings: list[Setting]) -> tuple[list[Spec], str]:
    """The tries of a stage, or ([], why it cannot start yet)."""
    a = stage_a(tasks, settings)
    if stage == "A":
        return a, ""
    names = {s.name for s in settings}
    if CEILING not in names:
        return [], f"stage {stage} needs the ceiling ({CEILING}) among the settings"
    if not finished(a, records):
        return [], f"stage {stage} waits for stage A to finish"
    b = stage_b(tasks, stage_b_settings(records, tasks, settings))
    if stage == "B":
        return b, ""
    if not finished(b, records):
        return [], "stage C waits for stage B to finish"
    best = cheapest_working(summarize(records, [], tasks, settings), tasks)["setting"]
    if best is None:
        return [], "stage C needs the cheapest working setting, and there is none yet"
    return stage_c(tasks, [s for s in settings if s.name in (best, CEILING)]), ""


# -- the numbers -----------------------------------------------------------------------
def _split(by_task: dict[str, bool], tasks: list[Task]) -> dict:
    """Counts of ``by_task`` (task id -> passed) on all, visible and held-out
    tasks and per kind: ``[passed, of]``."""
    held = heldout()
    groups = {"all": tasks, "visible": [t for t in tasks if t.id not in held],
              "heldout": [t for t in tasks if t.id in held]}
    groups.update({k: [t for t in tasks if t.kind == k] for k in KINDS})
    return {g: [sum(by_task[t.id] for t in ts if t.id in by_task), sum(t.id in by_task for t in ts)]
            for g, ts in groups.items()}


def summarize(records: dict, notruns: list[dict], tasks: list[Task], settings: list[Setting]) -> dict:
    """Per setting and mode (``"<setting>/<mode>"``): solved once, pass^3,
    cost, tokens, tool calls and errors, time and not-run retries."""
    ids, out = {t.id for t in tasks}, {}
    for s in settings:
        for mode in ("mcp", "cli"):
            rs = [r for r in records.values() if r["setting"] == s.name and r["mode"] == mode
                  and r["task"] in ids and r["class"] in SCORED]
            nr = [r for r in notruns if r.get("setting") == s.name and r.get("mode") == mode
                  and r.get("task") in ids]
            if not rs and not nr:
                continue
            tries = collections.defaultdict(list)
            for r in rs:
                tries[r["task"]].append(r["class"] == "solved")
            once = {r["task"]: r["class"] == "solved" for r in rs if r["try"] == 1}
            pass3 = {t: all(v) for t, v in tries.items() if len(v) >= TRIES}
            n, solved = len(rs), sum(r["class"] == "solved" for r in rs)
            cost = sum(r["costUsd"] for r in rs)
            mean = (lambda f: sum(f(r) for r in rs) / n) if n else (lambda f: 0.0)
            out[f"{s.name}/{mode}"] = {
                "setting": s.name, "label": s.label, "model": s.model, "effort": s.effort, "mode": mode,
                "tries": n, "solvedTries": solved, "once": _split(once, tasks), "onceByTask": once,
                "pass3": _split(pass3, tasks), "pass3ByTask": pass3, "pass3Complete": len(pass3) == len(tasks),
                "meanCostUsd": round(mean(lambda r: r["costUsd"]), 6), "totalCostUsd": round(cost, 6),
                "costPerSolveUsd": round(cost / solved, 6) if solved else None,
                "cliCostUsd": round(sum(r.get("cliCostUsd") or 0.0 for r in rs), 6),
                "meanTokens": {k: round(mean(lambda r: r["tokens"][k])) for k in
                               ("input", "output", "cacheRead", "cacheWrite")},
                "meanToolCalls": round(mean(lambda r: r["toolCalls"]), 1),
                "meanToolErrors": round(mean(lambda r: r["toolErrors"]), 1),
                "meanSeconds": round(mean(lambda r: r["seconds"])),
                "notRunRetries": len(nr), "notRunCostUsd": round(sum(r.get("costUsd", 0.0) for r in nr), 6),
            }
    return out


def cheapest_working(summary: dict, tasks: list[Task]) -> dict:
    """Answer 1: the lowest mean cost per try among the settings whose pass^3
    count on all tasks is at most ``WORKING_MARGIN`` below the ceiling's, with
    the paired comparison against the ceiling."""
    ceiling = summary.get(f"{CEILING}/mcp")
    if not ceiling or not ceiling["pass3Complete"]:
        return {"setting": None, "why": "needs the ceiling's three tries on every task (stage B)"}
    floor = ceiling["pass3"]["all"][0] - WORKING_MARGIN
    ok = [s for s in summary.values() if s["mode"] == "mcp" and s["pass3Complete"]
          and s["pass3"]["all"][0] >= floor]
    best = min(ok, key=lambda s: s["meanCostUsd"])
    c, b = ceiling["pass3ByTask"], best["pass3ByTask"]
    return {"setting": best["setting"], "label": best["label"], "meanCostUsd": best["meanCostUsd"],
            "pass3": best["pass3"]["all"], "ceilingPass3": ceiling["pass3"]["all"],
            "ceilingMeanCostUsd": ceiling["meanCostUsd"],
            "candidates": [s["setting"] for s in ok],
            "onlyCeilingSolves": sorted(t for t in c if c[t] and not b[t]),
            "onlyItSolves": sorted(t for t in c if b[t] and not c[t])}


def hands_off(summary: dict, tasks: list[Task]) -> dict:
    """Answer 2: the settings with pass^3 on at least ``HANDS_OFF`` of all
    tasks and of the held-out tasks."""
    done = [s for s in summary.values() if s["mode"] == "mcp" and s["pass3Complete"]]
    if not done:
        return {"settings": None, "why": "needs three tries on every task (stage B)"}

    def share(s, group):
        passed, of = s["pass3"][group]
        return passed / of if of else 0.0

    good = [s["setting"] for s in done if share(s, "all") >= HANDS_OFF and share(s, "heldout") >= HANDS_OFF]
    best = max(done, key=lambda s: (share(s, "all"), share(s, "heldout")))
    target = summary.get(f"{TARGET}/mcp")
    return {"settings": good, "best": best["setting"], "bestPass3": best["pass3"],
            "target": target["pass3"] if target and target["pass3Complete"] else None}


# -- the report ----------------------------------------------------------------------------
def _frac(pair: list) -> str:
    return f"{pair[0]}/{pair[1]}" if pair[1] else "–"


def report(records: dict, notruns: list[dict], tasks: list[Task], settings: list[Setting],
           note: str = "") -> tuple[str, dict]:
    ids = {t.id for t in tasks}
    summary = summarize(records, notruns, tasks, settings)
    a1, a2 = cheapest_working(summary, tasks), hands_off(summary, tasks)
    rs = [r for r in records.values() if r["task"] in ids]
    every = rs + [r for r in notruns if r.get("task") in ids]
    total = sum(r.get("costUsd", 0.0) for r in every)
    held = heldout()
    data = {
        "tasks": len(tasks), "heldout": sum(t.id in held for t in tasks),
        "scoredTries": sum(r["class"] in SCORED for r in rs), "notRunTries": len(every) - len(rs),
        "unavailable": unavailable(records),
        "totalCostUsd": round(total, 4), "notRunCostUsd": round(sum(r.get("costUsd", 0.0) for r in notruns
                                                                     if r.get("task") in ids), 4),
        "cliCostUsd": round(sum(r.get("cliCostUsd") or 0.0 for r in every), 4),
        "cliCostBasis": sorted({b for r in every for b in r.get("cliCostBasis") or []}),
        "tokens": {k: sum((r.get("tokens") or {}).get(k, 0) for r in every)
                   for k in ("input", "output", "cacheRead", "cacheWrite")},
        "trySeconds": round(sum(r.get("seconds", 0.0) for r in every)),
        "wallSeconds": round(max((r["finished"] for r in every if "finished" in r), default=0)
                             - min((r["finished"] - r.get("seconds", 0) for r in every if "finished" in r),
                                   default=0)),
        "claudeCode": sorted({r["claudeCode"] for r in every if r.get("claudeCode")}),
        "billing": sorted({r["billing"] for r in every if r.get("billing")}),
        "modelsReported": sorted({m for r in every for m in (r.get("reported") or {}).get("answered", [])}),
        "costIncomplete": sum(not r.get("costComplete", True) for r in rs),
        "settings": summary, "cheapestWorking": a1, "handsOff": a2,
    }
    label = {s.name: s.label for s in SETTINGS}
    lines = ["# Model and effort comparison", "",
             f"{len(tasks)} tasks ({data['heldout']} held out); {data['scoredTries']} scored tries, "
             f"{data['notRunTries']} not-run tries retried. Claude Code {', '.join(data['claudeCode']) or '–'}; "
             f"billing: {', '.join(data['billing']) or '–'}; the models that answered: "
             f"{', '.join(data['modelsReported']) or '–'}.", "",
             f"Cost at API list price: **{total:.2f} USD** in all ({data['notRunCostUsd']:.2f} of it on "
             f"not-run tries); the command-line tool's own figure is {data['cliCostUsd']:.2f} USD (its price "
             f"basis: {', '.join(data['cliCostBasis']) or '–'}). Tokens: "
             + ", ".join(f"{k} {data['tokens'][key]:,}" for k, key in (
                 ("input", "input"), ("output", "output"), ("cache read", "cacheRead"),
                 ("cache write", "cacheWrite")))
             + f". Time: {data['trySeconds'] / 3600:.1f} h of tries, {data['wallSeconds'] / 3600:.1f} h from "
             f"the first try's start to the last one's end."
             + (f" {data['costIncomplete']} tries have only part of their cost (killed, or a model with no "
                f"price here)." if data["costIncomplete"] else ""), ""]
    if note:
        lines += [note, ""]
    lines += ["## The answers", ""]
    if a1["setting"] is None:
        lines.append(f"1. The cheapest working setting: not known yet; it {a1['why']}.")
    else:
        lines.append(
            f"1. The cheapest working setting: **{a1['label']}**, {a1['meanCostUsd']:.4f} USD a try at list "
            f"price, pass^3 on {_frac(a1['pass3'])} tasks; the ceiling ({label[CEILING]}) has "
            f"{_frac(a1['ceilingPass3'])} at {a1['ceilingMeanCostUsd']:.4f} USD. Tasks only the ceiling "
            f"solves on all three tries: {', '.join(a1['onlyCeilingSolves']) or 'none'}; only "
            f"{a1['label']}: {', '.join(a1['onlyItSolves']) or 'none'}. (Working: pass^3 at most "
            f"{WORKING_MARGIN} tasks below the ceiling's; candidates: "
            f"{', '.join(label[x] for x in a1['candidates'])}.)")
    if a2["settings"] is None:
        lines.append(f"2. Good enough for hands-off work: not known yet; it {a2['why']}.")
    elif a2["settings"]:
        lines.append(f"2. Good enough for hands-off work (pass^3 on at least {HANDS_OFF:.0%} of all tasks "
                     f"and of the held-out ones): {', '.join(label[x] for x in a2['settings'])}.")
    else:
        b = a2["bestPass3"]
        lines.append(
            f"2. **No setting is good enough for hands-off work**: that needs pass^3 on at least "
            f"{HANDS_OFF:.0%} of all tasks and of the held-out ones, and the best, {label[a2['best']]}, has "
            f"{_frac(b['all'])} of all and {_frac(b['heldout'])} held out. That gap is what step 6's later "
            f"tool changes must close, with {label[TARGET]} as the target"
            + (f" (it has {_frac(a2['target']['all'])} and {_frac(a2['target']['heldout'])})."
               if a2["target"] else "."))
    if data["unavailable"]:
        lines += ["", "Unavailable settings: " + "; ".join(f"{label.get(k, k)}: {v}"
                                                          for k, v in data["unavailable"].items()) + "."]
    head = ("| Setting | Solved once (all / visible / held out) | pass^3 (all / visible / held out) | "
            "Mean cost a try, USD | Total, USD | Per solve, USD | Mean tokens in / out / cache read / "
            "cache write | Tool calls | Tool errors | Time, s | Not-run retried |")
    for mode, title in (("mcp", "MCP server"), ("cli", "Command line (stage C)")):
        rows = [s for s in summary.values() if s["mode"] == mode]
        if not rows:
            continue
        lines += ["", f"## {title}", "", head, "|" + "---|" * 11]
        for s in rows:
            t, per = s["meanTokens"], s["costPerSolveUsd"]
            lines.append(
                f"| {s['label']} | {' / '.join(_frac(s['once'][g]) for g in ('all', 'visible', 'heldout'))} | "
                f"{' / '.join(_frac(s['pass3'][g]) for g in ('all', 'visible', 'heldout'))} | "
                f"{s['meanCostUsd']:.4f} | {s['totalCostUsd']:.2f} | {'–' if per is None else f'{per:.4f}'} | "
                f"{t['input']:,} / {t['output']:,} / {t['cacheRead']:,} / {t['cacheWrite']:,} | "
                f"{s['meanToolCalls']} | {s['meanToolErrors']} | {s['meanSeconds']} | {s['notRunRetries']} |")
        lines += ["", f"Per kind, solved once; pass^3 ({title}):", "",
                  "| Setting | " + " | ".join(KINDS) + " |", "|" + "---|" * (len(KINDS) + 1)]
        for s in rows:
            lines.append(f"| {s['label']} | " + " | ".join(
                f"{_frac(s['once'][k])}; {_frac(s['pass3'][k])}" for k in KINDS) + " |")
    return "\n".join(lines) + "\n", data


# -- running it ------------------------------------------------------------------------------
class Comparison:
    def __init__(self, folder: Path, args: argparse.Namespace, cap: Optional[float] = None) -> None:
        self.dir, self.args, self.cap = folder, args, cap
        (folder / "tries").mkdir(parents=True, exist_ok=True)
        (folder / "runs").mkdir(exist_ok=True)
        self.records = {p.stem: json.loads(p.read_text(encoding="utf-8")) for p in (folder / "tries").glob("*.json")}
        log = folder / "notrun.jsonl"
        self.notruns = [json.loads(x) for x in log.read_text(encoding="utf-8").splitlines()] if log.exists() else []
        self.spent = sum(r.get("costUsd", 0.0) for r in [*self.records.values(), *self.notruns])
        self.lock = threading.Lock()
        self.pause_until, self.backoffs = 0.0, 0
        self.stopped: Optional[str] = None

    def run(self, specs: list[Spec]) -> None:
        todo = collections.deque(s for s in specs if s.id not in self.records)
        workers = [threading.Thread(target=self._work, args=(todo,)) for _ in range(max(1, self.args.jobs))]
        for w in workers:
            w.start()
        for w in workers:
            w.join()

    def _next(self, todo: collections.deque) -> Optional[Spec]:
        while True:
            with self.lock:
                out = unavailable(self.records)
                while todo and todo[0].setting.name in out:
                    todo.popleft()
                if self.cap is not None and self.spent >= self.cap and not self.stopped:
                    self.stopped = f"the cap of {self.cap:g} USD was reached (spent {self.spent:.4f} USD)"
                if self.stopped or not todo:
                    return None
                wait = self.pause_until - time.time()
                if wait <= 0:
                    return todo.popleft()
            time.sleep(min(wait, 5.0))

    def _work(self, todo: collections.deque) -> None:
        while (spec := self._next(todo)) is not None:
            with self.lock:
                attempt = 1 + sum(n["id"] == spec.id for n in self.notruns)
                budget = None if self.cap is None else self.cap - self.spent
            base = {"id": spec.id, "task": spec.task.id, "mode": spec.mode, "setting": spec.setting.name,
                    "try": spec.n, "attempt": attempt}
            try:
                r = run_try(spec.task, spec.mode, spec.setting.model, spec.setting.effort, self.dir / "runs",
                            f"{spec.id}.a{attempt}", claude=self.args.claude, timeout=self.args.timeout,
                            auth=self.args.auth, budget=budget)
            except Exception as e:  # noqa: BLE001 - the runner's fault: noted and retried
                r = {"class": "not-run", "why": f"runner: {type(e).__name__}: {e}"[:300], "costUsd": 0.0,
                     "trace": traceback.format_exc()[-2000:], "finished": time.time()}
            r.update(base)
            with self.lock:
                self.spent += r.get("costUsd", 0.0)
                if r["class"] == "not-run":
                    self._not_run(spec, r, todo)
                else:
                    r["retries"] = attempt - 1
                    (self.dir / "tries" / f"{spec.id}.json").write_text(json.dumps(r, indent=1), encoding="utf-8")
                    self.records[spec.id] = r
                    self.backoffs = 0
            print(f"{spec.id}: {r['class']}" + (f" ({r['why']})" if r["class"] not in SCORED else "")
                  + f", {r.get('toolCalls', 0)} tool calls, {r.get('costUsd', 0.0):.4f} USD, "
                  f"{r.get('seconds', 0):.0f} s", flush=True)

    def _not_run(self, spec: Spec, r: dict, todo: collections.deque) -> None:
        """Note a not-run try and put it back first in line, after the wait
        its kind calls for; stop when it cannot go on."""
        with open(self.dir / "notrun.jsonl", "a", encoding="utf-8") as f:
            f.write(json.dumps(r) + "\n")
        self.notruns.append(r)
        kind = r["why"].split(":", 1)[0]
        same = sum(n["id"] == spec.id and n["why"].startswith(kind) for n in self.notruns)
        if kind == "stop":
            self.stopped = r["why"]
        elif same >= GIVE_UP.get(kind, sys.maxsize):
            self.stopped = f"{spec.id}: {same} not-run tries ({r['why']})"
        if self.stopped:
            return
        now = time.time()
        if kind in ("limit", "service"):
            if r.get("limitReset"):
                wait = float(r["limitReset"]) - now + LIMIT_MARGIN_S
            else:
                wait = min(MAX_WAIT_S, BACKOFF_S * 2 ** self.backoffs)
                if now >= self.pause_until:  # a new hit, not one more try of the same pause
                    self.backoffs += 1
            self.pause_until = max(self.pause_until, now + wait)
        todo.appendleft(spec)


def main(argv=None) -> int:
    p = argparse.ArgumentParser(prog="python -m benchmarks.agent.compare", description=__doc__.split("\n\n")[0])
    p.add_argument("--out", default=str(Path(tempfile.gettempdir()) / "lightsim-bench-runs"),
                   help="where the comparison's folder goes (outside the repository)")
    p.add_argument("--name", default="comparison", help="the comparison's folder in --out: the same name resumes it")
    p.add_argument("--stage", choices=("A", "B", "C", "all"), default="all")
    p.add_argument("--tasks", help="task ids, comma-separated (default: all)")
    p.add_argument("--settings", help="setting names, comma-separated (default: all eight): "
                                      + ", ".join(s.name for s in SETTINGS))
    p.add_argument("--jobs", type=int, default=3, help="tries side by side")
    p.add_argument("--timeout", type=float, default=1800, help="seconds per try")
    p.add_argument("--auth", choices=("session", "api"), default="session",
                   help="session: the Claude plan claude is signed in to (no cap); api: ANTHROPIC_API_KEY, "
                        f"with an optional cap {CAP_ENV}")
    p.add_argument("--claude", default=shutil.which("claude") or "claude")
    p.add_argument("--report", action="store_true", help="only write the report from the records")
    args = p.parse_args(argv)
    isolate()
    if Path(args.out).resolve().is_relative_to(ROOT):
        print("--out must be outside the repository: transcripts are never committed.", file=sys.stderr)
        return 2
    tasks = load_tasks()
    wanted = args.tasks.split(",") if args.tasks else []
    named = args.settings.split(",") if args.settings else []
    unknown = (set(wanted) - {t.id for t in tasks}) | (set(named) - {s.name for s in SETTINGS})
    if unknown:
        print(f"No such task or setting: {', '.join(sorted(unknown))}.", file=sys.stderr)
        return 2
    tasks = [t for t in tasks if not wanted or t.id in wanted]
    settings = [s for s in SETTINGS if not named or s.name in named]
    cap = None
    if args.auth == "api" and os.environ.get(CAP_ENV):
        cap = float(os.environ[CAP_ENV])
    elif os.environ.get(CAP_ENV):
        print(f"{CAP_ENV} is ignored: a plan run has no cap (it applies to --auth api only).", file=sys.stderr)
    folder = Path(args.out).resolve() / args.name
    comp = Comparison(folder, args, cap)
    note = ""
    if not args.report:
        problem = preflight(args.claude, args.auth)
        if problem:
            print(f"Not run: {problem}.", file=sys.stderr)
            return 2
        print(f"{folder}: {args.jobs} jobs on CPUs {sorted(os.sched_getaffinity(0))}", flush=True)
        for stage in (("A", "B", "C") if args.stage == "all" else (args.stage,)):
            specs, why = plan(stage, comp.records, tasks, settings)
            if why:
                note = f"Stopped before stage {stage}: {why}."
                break
            comp.run(specs)
            if comp.stopped:
                note = f"Stopped in stage {stage}: {comp.stopped}. Run the same command again to go on."
                break
    text, data = report(comp.records, comp.notruns, tasks, settings, note)
    (folder / "report.md").write_text(text, encoding="utf-8")
    (folder / "report.json").write_text(json.dumps(data, indent=1), encoding="utf-8")
    print(text)
    return 1 if comp.stopped else 0


if __name__ == "__main__":
    sys.exit(main())
