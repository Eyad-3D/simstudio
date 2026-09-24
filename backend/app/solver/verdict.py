"""Run verdict: did the vehicle actually drive the cycle?

A run used to be a "success" whenever no warning text was printed, so a
car with its motor deleted reported success after driving 0 km. The
verdict is computed from the recorded data at the end of every run (and a
light version of it while the run goes, so a live run warns early):

- speed trace against the Driver's target: largest and RMS error, and the
  time outside the WLTP tolerance band (±2 km/h, ±1 s) and the EPA band
  (±2 mph, ±1 s). Each point's band spans the target's lowest and highest
  value within ±1 s of it, widened by the speed tolerance. The trace is
  sampled every TRACE_STEP_S of solver time, not at the case step, so the
  verdict does not depend on how often results are stored;
- distance driven against the distance the target asks for;
- non-finite (NaN / inf) values in any channel.

A run whose trace leaves the WLTP band for more than 1 % of its duration
(at least 2 s) did not follow the cycle: its status is at best "warning"
and the figures per distance are flagged not valid. A run that covers less
than 5 % of the cycle's distance, or produces non-finite values, "failed".

A performance-test case (SimCase.kind "performance": a step in the target
speed, driven at full throttle until the car gets there) is not judged on
the band: it reports its maximum speed and the time from t = 0 to the
target's highest value, or says that it never got there. Its trace is
sampled at every solver step, so that time is read where the speed crosses
the target, not on a line to a 0.1 s sample taken after the Driver lifted
off.

The physics stayed in range: a motor, engine, battery or fuel cell that ran
past the data of one of its tables, or above its maximum speed (the
counters in RunContext.map_use), for longer than the trace's allowance
(1 % of the run, at least 2 s) makes the run at best "warning", and the
figures per distance and a performance test's rows are flagged not valid,
naming the element, how far past and for how long.
"""
from __future__ import annotations

import math
from collections import deque
from dataclasses import dataclass
from typing import Callable, Iterable

from .maps import MapUse

WLTP_TOL_KMH = 2.0
EPA_TOL_KMH = 2.0 * 1.609344  # ±2 mph
BAND_S = 1.0  # ± time tolerance of both bands
OUTSIDE_SHARE = 0.01  # share of the duration the trace may spend outside the band
OUTSIDE_MIN_S = 2.0
NOT_DRIVEN_SHARE = 0.05  # below this share of the cycle distance the car did not drive
LIVE_AFTER_S = 60.0  # the live check waits this long before it judges
TRACE_STEP_S = 0.1  # the trace's sampling cadence, independent of the case step


@dataclass
class TraceMetrics:
    duration_s: float
    cycle_km: float
    max_err_kmh: float
    t_max_err: float
    rms_kmh: float
    outside_wltp_s: float
    outside_epa_s: float


class CycleTrace:
    """Samples the Driver's target and the vehicle speed of a run that has
    both, every TRACE_STEP_S of solver time (at the first solver step on or
    after each multiple of it; at every solver step in a performance test)
    and at the run's first and last instant."""

    def __init__(self, ctx):
        model = ctx.model
        self.ctx = ctx
        self.src = (model.signal_route.get((model.driver, "sig_target_in"))
                    if model.driver and ctx.veh_id else None)
        # a Constant or Driving Task target is evaluated at the sample's own
        # time; any other source is read as last published (≤ 1 solver step old)
        self.src_kind = dict(ctx.sources).get(self.src[0]) if self.src else None
        self.times: list[float] = []
        self.target: list[float] = []
        self.speed: list[float] = []
        self.next_t = 0.0
        self.cycle_m = 0.0  # distance the target asks for so far
        self.live_warned = False

    def sample(self, t: float, last: bool = False) -> None:
        """Called after every solver step with its end time ``t``; records a
        sample when ``t`` reaches the next multiple of TRACE_STEP_S, or when
        ``last`` (the run's final instant)."""
        if self.src is None:
            return
        if (t < self.next_t - 1e-9 and not self.ctx.performance
                and not (last and self.times and t > self.times[-1] + 1e-9)):
            return
        self.next_t = (math.floor(t / TRACE_STEP_S + 1e-6) + 1) * TRACE_STEP_S
        if self.src_kind is not None:
            target = self.ctx.source_value(self.src[0], self.src_kind, t)
        else:
            target = self.ctx.rt.signal_values.get(self.src)
        if target is None:
            return
        if self.times:
            self.cycle_m += 0.5 * (target + self.target[-1]) / 3.6 * (t - self.times[-1])
        self.times.append(t)
        self.target.append(target)
        self.speed.append(self.ctx.v * 3.6)

    def live_problem(self) -> str | None:
        """A problem worth telling the user while a live run goes (once)."""
        if self.live_warned or not self.times or self.times[-1] < LIVE_AFTER_S:
            return None
        if self.cycle_m > 100.0 and self.ctx.distance < NOT_DRIVEN_SHARE * self.cycle_m:
            self.live_warned = True
            return (f"No distance covered after {self.times[-1]:.0f} s although the drive "
                    f"cycle asks for {self.cycle_m / 1000.0:.2f} km so far — check that a motor "
                    f"or engine drives the wheels and gets a command.")
        return None

    def metrics(self) -> TraceMetrics | None:
        return trace_metrics(self.times, self.target, self.speed)


def trace_metrics(ts: list[float], tgt: list[float], spd: list[float]) -> TraceMetrics | None:
    """Trace error of vehicle speeds ``spd`` against targets ``tgt`` (km/h)
    sampled at times ``ts``; point 0 is the initial state and not judged."""
    n = len(ts)
    if n < 2:
        return None
    # sliding window of the samples within ±BAND_S of point i, with the
    # indices of its lowest and highest target kept in monotonic deques
    lows: deque[int] = deque()
    highs: deque[int] = deque()
    hi = -1
    out_wltp = out_epa = sq = 0.0
    worst, t_worst = 0.0, ts[0]
    for i in range(1, n):
        while hi + 1 < n and ts[hi + 1] <= ts[i] + BAND_S + 1e-9:
            hi += 1
            while lows and tgt[lows[-1]] >= tgt[hi]:
                lows.pop()
            lows.append(hi)
            while highs and tgt[highs[-1]] <= tgt[hi]:
                highs.pop()
            highs.append(hi)
        for dq in (lows, highs):
            while ts[dq[0]] < ts[i] - BAND_S - 1e-9:
                dq.popleft()
        t_lo, t_hi = tgt[lows[0]], tgt[highs[0]]
        dt = ts[i] - ts[i - 1]
        if spd[i] < t_lo - WLTP_TOL_KMH or spd[i] > t_hi + WLTP_TOL_KMH:
            out_wltp += dt
        if spd[i] < t_lo - EPA_TOL_KMH or spd[i] > t_hi + EPA_TOL_KMH:
            out_epa += dt
        err = spd[i] - tgt[i]
        sq += err * err * dt
        if abs(err) > abs(worst):
            worst, t_worst = err, ts[i]
    duration = ts[-1] - ts[0]
    cycle_m = sum(0.5 * (tgt[k] + tgt[k - 1]) / 3.6 * (ts[k] - ts[k - 1]) for k in range(1, n))
    return TraceMetrics(
        duration_s=duration,
        cycle_km=cycle_m / 1000.0,
        max_err_kmh=worst,
        t_max_err=t_worst,
        rms_kmh=math.sqrt(sq / duration) if duration > 0 else 0.0,
        outside_wltp_s=out_wltp,
        outside_epa_s=out_epa,
    )


@dataclass
class Verdict:
    # (level, text): an "error" fails the run, a "warning" keeps it from success
    messages: tuple[tuple[str, str], ...] = ()
    cycle_not_followed: bool = False
    broke_down: bool = False  # non-finite values: no number of the run is valid
    # summary rows of a performance test: (label, value, unit)
    rows: tuple[tuple[str, float, str], ...] = ()
    beyond_reason: str = ""  # why data-dependent figures are not valid, or ""


def _num(x: float) -> str:
    return f"{x:,.0f}" if abs(x) >= 10 else f"{x:.2g}"


def beyond_data(uses: Iterable[MapUse], duration_s: float,
                name: Callable[[str], str]) -> list[tuple[str, str]]:
    """(warning, reason) for each element that spent longer outside one of
    its tables' data, or above its maximum speed, than the trace may spend
    outside its band; judged on the element's longest record (its tables
    share one operating point, so their times are not added up)."""
    allowance = max(OUTSIDE_MIN_S, OUTSIDE_SHARE * duration_s)
    longest: dict[str, MapUse] = {}
    for use in sorted(uses, key=lambda u: u.outside_s):
        longest[use.el_id] = use
    out = []
    for use in longest.values():
        if use.outside_s <= allowance:
            continue
        up = use.value > use.edge
        reason = (f"{name(use.el_id)} ran {_num(abs(use.value - use.edge))} {use.unit} past "
                  f"its {use.what} for {_num(use.outside_s)} s")
        limit = ("its maximum speed is" if use.what == "maximum speed"
                 else f"its data {'ends' if up else 'starts'} at")
        out.append((f"{reason} of {_num(duration_s)} s ({use.axis} {'up' if up else 'down'} to "
                    f"{_num(use.value)} {use.unit} at t = {use.t:.1f} s; {limit} "
                    f"{_num(use.edge)} {use.unit}). Consumption figures per distance are not "
                    f"valid.", reason))
    return out


def _time_to(ts: list[float], spd: list[float], level: float) -> float | None:
    """When the speed first reaches ``level``: linear between the sample
    before and the first one at or above it."""
    for k, v in enumerate(spd):
        if v >= level:
            if k == 0:
                return ts[0]
            return ts[k - 1] + (level - spd[k - 1]) / (v - spd[k - 1]) * (ts[k] - ts[k - 1])
    return None


def judge(trace: CycleTrace, distance_m: float, series: dict, performance: bool = False,
          duration_s: float = 0.0, uses: Iterable[MapUse] = (), stopped: bool = False) -> Verdict:
    """The checks a finished run must pass to be called a success; ``uses``
    are the run's MapUse records, ``duration_s`` the time it solved and
    ``stopped`` whether a stop cut it short."""
    messages: list[tuple[str, str]] = []
    rows: list[tuple[str, float, str]] = []
    not_followed = False

    bad = sorted({f"{el}:{port}" for (el, port), values in series.items()
                  if any(v is not None and not math.isfinite(v) for v in values)})
    if bad:
        messages.append(("error", f"Non-finite values (NaN or infinity) in {', '.join(bad[:5])}"
                                  f"{' …' if len(bad) > 5 else ''} — the solution broke down, "
                                  f"so no result of this run is valid."))

    m = trace.metrics()
    if m is not None and m.cycle_km > 0.1:
        km = distance_m / 1000.0
        if km < NOT_DRIVEN_SHARE * m.cycle_km:
            not_followed = True
            messages.append(("error", f"The vehicle did not drive the cycle: {km:.2f} of "
                                      f"{m.cycle_km:.2f} km covered. Check that a motor or engine "
                                      f"drives the wheels and gets a command; no result of this "
                                      f"run is valid."))
        elif not performance and m.outside_wltp_s > max(OUTSIDE_MIN_S,
                                                       OUTSIDE_SHARE * m.duration_s):
            not_followed = True
            messages.append(("warning", (
                f"Cycle not followed: the speed was outside the ±2 km/h, ±1 s trace tolerance "
                f"for {m.outside_wltp_s:.0f} s of {m.duration_s:.0f} s (EPA ±2 mph band: "
                f"{m.outside_epa_s:.0f} s), up to {abs(m.max_err_kmh):.1f} km/h "
                f"{'above' if m.max_err_kmh > 0 else 'below'} the target at "
                f"t = {m.t_max_err:g} s (RMS {m.rms_kmh:.2f} km/h); it drove {km:.2f} of "
                f"{m.cycle_km:.2f} km. Consumption figures per distance are not valid.")))
    if performance and trace.times:
        level, v_max = max(trace.target), max(trace.speed)
        rows.append(("Maximum speed", round(v_max, 2), "km/h"))
        t_level = _time_to(trace.times, trace.speed, level)
        if t_level is None:
            if not stopped:  # a stopped run only did not get there yet
                messages.append(("info", f"Performance test: the vehicle did not reach the "
                                         f"{level:.4g} km/h target; its maximum speed was "
                                         f"{v_max:.1f} km/h."))
        elif t_level > trace.times[0]:  # no time when it started at the target or above
            rows.append((f"Time to {level:.4g} km/h", round(t_level, 2), "s"))
    model = trace.ctx.model
    beyond = beyond_data(uses, duration_s,
                         lambda el: f"{model.cdef_of[el].name} '{model.elements[el].label}'")
    messages += [("warning", text) for text, _ in beyond]
    return Verdict(messages=tuple(messages), cycle_not_followed=not_followed, broke_down=bool(bad),
                   rows=tuple(rows), beyond_reason="; ".join(reason for _, reason in beyond))
