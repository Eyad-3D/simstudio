"""An engine's answer to a reference problem, held against the exact one.

Engine-neutral: an adapter (benchmarks/engines/) turns a problem into a run
of its engine and returns a :class:`Trace` in the problem's quantities and
SI units; :func:`compare` measures it.

- Signals are compared at the problem's output times (t > 0, outside the
  exact solution's breaks): the largest and the RMS error, and the largest
  as a share of the scale (the largest |exact value| unless the problem
  gives one).
- Events are the engine's own event times when it reports them, else the
  first crossing of the event's signal and level in the engine's finest
  output (linear interpolation between samples).
- Energy terms are the engine's books at t_end, against the problem's
  energy scale (the largest |exact term| at t_end); the closure is the
  engine's own balance, sources − sinks, from its terms when it reports
  them all, else the residual the engine states.
"""
from __future__ import annotations

import math
from dataclasses import asdict, dataclass, field

from . import Problem
from .exact import Exact, solve


@dataclass
class Trace:
    """What an engine gave for one problem, in the problem's quantities."""

    times: list[float]  # its finest output times, s
    signals: dict[str, list[float | None]]  # quantity → value at each time (SI)
    events: dict[str, float] = field(default_factory=dict)  # event times it reports itself
    energy: dict[str, float] = field(default_factory=dict)  # energy terms at t_end, J
    closure_j: float | None = None  # its own balance residual, J, when it states one
    wall_s: float = 0.0
    sim_s: float = 0.0  # the time the engine simulated (a preamble included), s
    steps: int | None = None  # solver steps taken
    status: str = "success"
    messages: list[str] = field(default_factory=list)


@dataclass
class Row:
    name: str
    kind: str
    unit: str
    error_max: float | None = None  # signal: largest |error|; event/energy: |error|
    error_rms: float | None = None  # signal only
    error_rel: float | None = None  # error_max / scale
    tolerance: float | None = None  # in the quantity's unit
    passed: bool | None = None  # None: the engine does not give it
    at: float | None = None  # signal: where the largest error is, s
    exact: float | None = None  # event/energy: the exact value
    value: float | None = None  # event/energy: the engine's
    note: str = ""


@dataclass
class Outcome:
    problem: str
    rows: list[Row]
    closure_rel: float | None
    closure_tol: float
    closure_passed: bool | None
    energy_scale: float

    @property
    def passed(self) -> bool:
        """Every quantity the engine gives within its tolerance, and its
        energy balance closed (when it can be checked)."""
        rows_ok = all(r.passed for r in self.rows if r.passed is not None)
        return rows_ok and self.closure_passed is not False

    @property
    def worst_rel(self) -> float | None:
        rels = [r.error_rel for r in self.rows if r.error_rel is not None]
        return max(rels) if rels else None

    def row(self, name: str) -> Row:
        return next(r for r in self.rows if r.name == name)

    def to_dict(self) -> dict:
        return {"problem": self.problem, "passed": self.passed, "worst_rel": self.worst_rel,
                "energy_scale_j": self.energy_scale, "closure_rel": self.closure_rel,
                "closure_tol": self.closure_tol, "closure_passed": self.closure_passed,
                "rows": [asdict(r) for r in self.rows]}


def _on_grid(times: list[float], values: list[float | None], grid: list[float]
             ) -> list[float | None]:
    """values at the grid's times (the trace must hold every one of them)."""
    index = {round(t, 9): k for k, t in enumerate(times)}
    out = []
    for t in grid:
        k = index.get(round(t, 9))
        if k is None:
            raise ValueError(f"the trace has no sample at t = {t:g} s")
        out.append(values[k])
    return out


def first_crossing(times: list[float], values: list[float | None], level: float,
                   direction: str = "either") -> float | None:
    """The first time after t = 0 the samples reach ``level`` going
    ``direction`` ("rising", "falling", or "either": away from the side the
    first sample after t = 0 is on), by linear interpolation between the
    samples around it; None if they never do."""
    pts = [(t, v) for t, v in zip(times, values) if v is not None]
    start = next((k for k, (t, _) in enumerate(pts) if t > 0), None)
    if start is None:
        return None
    if direction == "either":
        side = pts[start][1] - level
        if side == 0:
            return pts[start][0]
        direction = "falling" if side > 0 else "rising"
    for k in range(max(1, start), len(pts)):
        (ta, va), (tb, vb) = pts[k - 1], pts[k]
        da, db = va - level, vb - level
        hit = (da < 0 <= db) if direction == "rising" else (da > 0 >= db)
        if hit:
            return ta + (tb - ta) * da / (da - db)
    return None


def energy_scale(problem: Problem, ex: Exact) -> float:
    return max(abs(ex.at(n, problem.t_end)) for n in problem.energy_terms)


def compare(problem: Problem, trace: Trace, ex: Exact | None = None) -> Outcome:
    ex = ex or solve(problem)
    grid = [t for t in problem.output_times()
            if t > 0 and all(abs(t - b) > 1e-9 for b in ex.breaks)]
    e_scale = energy_scale(problem, ex)
    rows = []
    for c in problem.compare:
        row = Row(c.name, c.kind, c.unit, note=c.note)
        if c.kind == "signal":
            if c.name in trace.signals:
                got = _on_grid(trace.times, trace.signals[c.name], grid)
                pairs = [(t, g, ex.at(c.name, t)) for t, g in zip(grid, got) if g is not None]
                if pairs:
                    errs = [(abs(g - e), t) for t, g, e in pairs]
                    err, at = max(errs)
                    scale = c.scale or max(abs(e) for _, _, e in pairs) or 1.0
                    row.error_max, row.at = err, at
                    row.error_rms = math.sqrt(sum(e * e for e, _ in errs) / len(errs))
                    row.error_rel = err / scale
                    row.tolerance = c.atol + c.rtol * scale
                    row.passed = err <= row.tolerance
        elif c.kind == "event":
            exact_t = ex.events[c.name]
            got = trace.events.get(c.name)
            given = c.signal is not None and c.signal in trace.signals
            if got is None and given:
                got = first_crossing(trace.times, trace.signals[c.signal], c.level, c.direction)
            row.exact, row.tolerance = exact_t, c.atol + c.rtol * abs(exact_t)
            if got is not None:
                row.value = got
                row.error_max = abs(got - exact_t)
                row.error_rel = row.error_max / abs(exact_t)
                row.passed = row.error_max <= row.tolerance
            elif given:
                row.passed = False  # the engine gives the signal but never gets there
                row.note = (row.note + "; " if row.note else "") + "never reached"
        else:  # energy at t_end
            exact_e = ex.at(c.name, problem.t_end)
            row.exact, row.tolerance = exact_e, c.atol + c.rtol * e_scale
            if c.name in trace.energy:
                row.value = trace.energy[c.name]
                row.error_max = abs(row.value - exact_e)
                row.error_rel = row.error_max / e_scale
                row.passed = row.error_max <= row.tolerance
        rows.append(row)
    closure = None
    if all(n in trace.energy for n in problem.energy_terms):
        closure = (sum(trace.energy[n] for n in problem.sources)
                   - sum(trace.energy[n] for n in problem.sinks))
    elif trace.closure_j is not None:
        closure = trace.closure_j
    closure_rel = abs(closure) / e_scale if closure is not None else None
    return Outcome(problem.id, rows, closure_rel, problem.closure_rtol,
                   None if closure_rel is None else closure_rel <= problem.closure_rtol, e_scale)
