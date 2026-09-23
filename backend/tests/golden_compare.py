"""Golden fixtures v2: snapshots of the bundled examples, the comparison
with tolerance bands, and a readable diff report.

Each golden case is run at the example's shipped case step (the solver step
is then 10 ms) and at a finer 5 ms solver step, recorded every second in
both, so every fixture shares one time grid. A fixture stores the status,
the messages, the summary ("headline") numbers and every tenth recorded
point of every channel, and names the CHANGES.md entry that produced it.

Regression, not validation: these tests ask "did the behaviour change?",
not "is it right?" (that is the plausibility tests' job). A comparison
passes when

- status and messages are the same;
- every headline number is within its band (HEADLINE_REL of the value, at
  least HEADLINE_ABS for its unit — a few steps of the summary's rounding);
- every stored reference point lies inside a tube around the new curve:
  some new point within ±TUBE_T recorded steps is within TUBE_REL of the
  channel's range (the csv-compare method used for Modelica libraries).

With SIMSTUDIO_GOLDEN_EXACT=1 every stored number must also match to 1e-6,
as before v2: use it to prove a pure refactor changes nothing.
"""
from __future__ import annotations

import json
import os
import sys
from dataclasses import dataclass, field
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent))

from app.solver import simulate  # noqa: E402
from app.storage import load_project  # noqa: E402

GOLDEN_DIR = Path(__file__).parent / "golden"
CHANGES = GOLDEN_DIR / "CHANGES.md"
FORMAT = 2

#: (project, case) pairs frozen as fixtures
EXAMPLES = [("bev-car", "case-city"), ("hybrid-car", "case-mixed")]
#: variant → (case step, store every, solver step): the shipped step, and a
#: finer solver step recorded on the same 1 s grid (the examples are
#: converged at 10 ms: their headline numbers agree at 5 and 2.5 ms)
VARIANTS = {"shipped": (None, None), "fine": (0.005, 200)}
CASES = [(p, c, v) for p, c in EXAMPLES for v in VARIANTS]
STRIDE = 10  # keep every Nth recorded point (plus the last) in the fixture

HEADLINE_REL = 0.002  # 0.2 % of the value …
HEADLINE_ABS = {  # … but at least this, per unit (levels: this only)
    "%": 0.02, "kWh": 0.002, "kg": 0.002, "km": 0.002, "s": 0.02,
    "kWh/100km": 0.02, "l/100km": 0.02, "g/km": 0.2,
}
LEVEL_UNITS = {"%"}  # SOC and shares: absolute bands only
TUBE_REL = 0.005  # value band: 0.5 % of the channel's range in the fixture
TUBE_ABS = 1e-6
TUBE_T = 1  # time band: ± this many recorded steps
EXACT = 1e-6  # the exact mode's relative and absolute tolerance


def exact_mode() -> bool:
    return os.environ.get("SIMSTUDIO_GOLDEN_EXACT", "") not in ("", "0")


def fixture_path(project_id: str, case_id: str, variant: str = "shipped") -> Path:
    suffix = "" if variant == "shipped" else f"__{variant}"
    return GOLDEN_DIR / f"{project_id}__{case_id}{suffix}.json"


def run(project_id: str, case_id: str, variant: str = "shipped"):
    """The case's result at full recorded resolution."""
    project = load_project(project_id)
    case = next(c for c in project.cases if c.id == case_id)
    step, every = VARIANTS[variant]
    if step is not None:
        case.timeStep, case.outputEvery = step, every
    return simulate(project, case_id)


def snapshot(project_id: str, case_id: str, variant: str = "shipped", result=None,
             change: str = "") -> dict:
    """Reduce a run to a fixture (every STRIDE-th point of each channel)."""
    if result is None:
        result = run(project_id, case_id, variant)
    project = load_project(project_id)
    case = next(c for c in project.cases if c.id == case_id)
    step, every = VARIANTS[variant]
    times = [p["t"] for p in result.channels[0].timeSeries] if result.channels else []
    idx = list(range(0, len(times), STRIDE))
    if idx and idx[-1] != len(times) - 1:
        idx.append(len(times) - 1)
    return {
        "format": FORMAT,
        "project": project_id,
        "case": case_id,
        "variant": variant,
        "case_step_s": step if step is not None else case.timeStep,
        "store_every": every if every is not None else (case.outputEvery or 1),
        "change": change,
        "status": result.status,
        "messages": [f"{m.level}: {m.text}" for m in result.messages],
        "summary": {s.label: [round(s.value, 6), s.unit] for s in result.summary},
        "times": [times[i] for i in idx],
        "channels": {
            f"{c.elementId}:{c.portId}": {
                "unit": c.unit,
                "values": [None if c.timeSeries[i]["value"] is None
                           else round(c.timeSeries[i]["value"], 6) for i in idx],
            }
            for c in result.channels
        },
    }


def full_channels(result) -> tuple[list[float], dict[str, tuple[str, list]]]:
    """(times, {channel: (unit, values)}) of a run at full resolution."""
    times = [p["t"] for p in result.channels[0].timeSeries] if result.channels else []
    return times, {f"{c.elementId}:{c.portId}": (c.unit, [p["value"] for p in c.timeSeries])
                   for c in result.channels}


def dump(data: dict) -> str:
    """Fixture JSON with one line per message, headline number and channel,
    so a regeneration's git diff shows what moved."""
    out = ["{"]
    keys = list(data)
    for n, key in enumerate(keys):
        value = data[key]
        comma = "," if n < len(keys) - 1 else ""
        if isinstance(value, dict) and value:
            out.append(f" {json.dumps(key)}: {{")
            items = list(value.items())
            for m, (k, v) in enumerate(items):
                tail = "," if m < len(items) - 1 else ""
                out.append(f"  {json.dumps(k, ensure_ascii=False)}: "
                           f"{json.dumps(v, ensure_ascii=False)}{tail}")
            out.append(f" }}{comma}")
        elif key == "messages" and value:
            out.append(f" {json.dumps(key)}: [")
            for m, text in enumerate(value):
                tail = "," if m < len(value) - 1 else ""
                out.append(f"  {json.dumps(text, ensure_ascii=False)}{tail}")
            out.append(f" ]{comma}")
        else:
            out.append(f" {json.dumps(key)}: {json.dumps(value, ensure_ascii=False)}{comma}")
    out.append("}")
    return "\n".join(out) + "\n"


def load(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


# ---- comparison -------------------------------------------------------------

@dataclass
class Headline:
    label: str
    unit: str
    old: float | None
    new: float | None
    tol: float

    @property
    def ok(self) -> bool:
        if self.old is None or self.new is None:
            return False
        return abs(self.new - self.old) <= self.tol + 1e-12

    @property
    def changed(self) -> bool:
        """Moved at all (beyond the exact mode's 1e-6)."""
        if self.old is None or self.new is None:
            return self.old != self.new
        return abs(self.new - self.old) > EXACT * max(1.0, abs(self.old))


@dataclass
class ChannelMiss:
    key: str
    unit: str
    t: float  # the reference point furthest outside its tube
    old: float | None
    new: float | None  # the new value at that time
    miss: float  # distance from the tube's nearest new value
    tol: float
    points: int  # reference points outside the tube


@dataclass
class Comparison:
    name: str
    status: tuple[str, str]
    messages_added: list[str] = field(default_factory=list)
    messages_removed: list[str] = field(default_factory=list)
    headlines: list[Headline] = field(default_factory=list)
    misses: list[ChannelMiss] = field(default_factory=list)
    channels_added: list[str] = field(default_factory=list)
    channels_removed: list[str] = field(default_factory=list)
    grid_changed: bool = False
    # channels whose stored points moved at all (beyond 1e-6), inside or
    # outside their tube: the exact mode's check
    moved: list[str] = field(default_factory=list)

    @property
    def within_tolerance(self) -> bool:
        return (self.status[0] == self.status[1] and not self.messages_added
                and not self.messages_removed and all(h.ok for h in self.headlines)
                and not self.misses and not self.channels_added
                and not self.channels_removed and not self.grid_changed)

    @property
    def identical(self) -> bool:
        return (self.within_tolerance and not self.moved
                and not any(h.changed for h in self.headlines))

    @property
    def ok(self) -> bool:
        """What the golden test asks for: within tolerance, or identical in
        the exact mode."""
        return self.identical if exact_mode() else self.within_tolerance


def headline_tol(unit: str, value: float) -> float:
    base = HEADLINE_ABS.get(unit, 1e-6)
    return base if unit in LEVEL_UNITS else max(base, HEADLINE_REL * abs(value))


def compare(expected: dict, result, name: str = "") -> Comparison:
    """Compare a fixture with a run (the run at full recorded resolution)."""
    actual = snapshot(expected["project"], expected["case"], expected["variant"], result=result)
    cmp = Comparison(name=name or f"{expected['project']} / {expected['case']} "
                                  f"({expected['variant']})",
                     status=(expected["status"], actual["status"]))
    old_msgs, new_msgs = expected["messages"], actual["messages"]
    cmp.messages_added = [m for m in new_msgs if m not in old_msgs]
    cmp.messages_removed = [m for m in old_msgs if m not in new_msgs]

    for label in list(expected["summary"]) + [k for k in actual["summary"]
                                              if k not in expected["summary"]]:
        old = expected["summary"].get(label)
        new = actual["summary"].get(label)
        unit = (old or new)[1]
        cmp.headlines.append(Headline(label, unit, old[0] if old else None,
                                      new[0] if new else None,
                                      headline_tol(unit, old[0] if old else new[0])))

    times, channels = full_channels(result)
    cmp.channels_added = sorted(set(channels) - set(expected["channels"]))
    cmp.channels_removed = sorted(set(expected["channels"]) - set(channels))
    cmp.grid_changed = actual["times"] != expected["times"]
    index = {t: i for i, t in enumerate(times)}
    for key, ref in expected["channels"].items():
        if key not in channels:
            continue
        unit, values = channels[key]
        finite = [v for v in ref["values"] if v is not None]
        span = (max(finite) - min(finite)) if finite else 0.0
        tol = max(TUBE_ABS, TUBE_REL * span)
        worst: ChannelMiss | None = None
        outside = 0
        for t, v_ref in zip(expected["times"], ref["values"]):
            i = index.get(t)
            if i is None:
                continue
            window = [values[j] for j in range(max(0, i - TUBE_T), min(len(values), i + TUBE_T + 1))]
            if v_ref is None:
                miss = 0.0 if None in window else float("inf")
            else:
                gaps = [abs(v - v_ref) for v in window if v is not None]
                miss = min(gaps) if gaps else float("inf")
            if miss > tol:
                outside += 1
                if worst is None or miss > worst.miss:
                    worst = ChannelMiss(key, unit, t, v_ref, values[i], miss, tol, 0)
        if worst is not None:
            worst.points = outside
            cmp.misses.append(worst)
        new_ref = actual["channels"][key]["values"]
        if any((a is None) != (b is None) or (a is not None and abs(a - b) > EXACT * max(1.0, abs(a)))
               for a, b in zip(ref["values"], new_ref)):
            cmp.moved.append(key)
    cmp.misses.sort(key=lambda m: -m.miss / m.tol if m.tol else 0.0)
    return cmp


# ---- report -------------------------------------------------------------------

def _num(x: float | None, unit: str = "") -> str:
    if x is None:
        return "—"
    return f"{x:.6g}{(' ' + unit) if unit else ''}"


def report(cmp: Comparison, verbose: bool = False) -> str:
    """What changed, old → new and by how much, readable in a terminal and
    as Markdown (the CI job summary)."""
    moved = [h for h in cmp.headlines if h.changed]
    if cmp.identical:
        verdict = "identical"
    elif cmp.within_tolerance:
        verdict = (f"within tolerance ({len(moved)} headline numbers and {len(cmp.moved)} "
                   f"channels moved inside their bands)")
        if exact_mode():
            verdict += " — NOT IDENTICAL (exact mode)"
    else:
        verdict = "OUT OF TOLERANCE"
    out = [f"### {cmp.name}: {verdict}"]
    if cmp.status[0] != cmp.status[1]:
        out.append(f"- status: {cmp.status[0]} → {cmp.status[1]}")
    for m in cmp.messages_removed:
        out.append(f"- message gone: {m}")
    for m in cmp.messages_added:
        out.append(f"- message new: {m}")
    if moved or verbose:
        out += ["", "| Headline number | Old | New | Change | Band | |",
                "|---|---|---|---|---|---|"]
        for h in (cmp.headlines if verbose else moved):
            if h.old is not None and h.new is not None:
                delta = h.new - h.old
                rel = f" ({100 * delta / h.old:+.2f} %)" if h.old else ""
                change = f"{delta:+.6g}{rel}"
            else:
                change = "new row" if h.old is None else "row gone"
            out.append(f"| {h.label} | {_num(h.old, h.unit)} | {_num(h.new, h.unit)} | {change} "
                       f"| ±{h.tol:.3g} | {'ok' if h.ok else '**out**'} |")
    if cmp.channels_added or cmp.channels_removed:
        out.append(f"- channels new: {', '.join(cmp.channels_added) or '—'}; "
                   f"gone: {', '.join(cmp.channels_removed) or '—'}")
    if cmp.grid_changed:
        out.append("- the recorded time grid changed")
    if cmp.misses:
        out += ["", f"Channels outside their tube (±{100 * TUBE_REL:g} % of the channel's range, "
                    f"±{TUBE_T} recorded step):", "",
                "| Channel | Worst at | Old | New | Off by | Band | Points out |",
                "|---|---|---|---|---|---|---|"]
        for m in cmp.misses[:15]:
            out.append(f"| {m.key} | t = {m.t:g} s | {_num(m.old, m.unit)} | {_num(m.new, m.unit)} "
                       f"| {m.miss:.4g} | ±{m.tol:.3g} | {m.points} |")
        if len(cmp.misses) > 15:
            out.append(f"| … and {len(cmp.misses) - 15} more | | | | | | |")
    if cmp.moved and (verbose or exact_mode()):
        out.append(f"- channels that moved (beyond 1e-6): {', '.join(cmp.moved[:20])}"
                   + (" …" if len(cmp.moved) > 20 else ""))
    return "\n".join(out)
