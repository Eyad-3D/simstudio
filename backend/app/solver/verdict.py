"""Run verdict: did the vehicle actually drive the cycle?

A run used to be a "success" whenever no warning text was printed, so a
car with its motor deleted reported success after driving 0 km. The
verdict is computed from the recorded data at the end of every run (and a
light version of it while the run goes, so a live run warns early):

- speed trace against the Driver's target: largest and RMS error, and the
  time outside the WLTP tolerance band (±2 km/h, ±1 s) and the EPA band
  (±2 mph, ±1 s). Each point's band spans the target's lowest and highest
  value within ±1 s of it, widened by the speed tolerance;
- distance driven against the distance the target asks for;
- non-finite (NaN / inf) values in any channel.

A run whose trace leaves the WLTP band for more than 1 % of its duration
(at least 2 s) did not follow the cycle: its status is at best "warning"
and the figures per distance are flagged not valid. A run that covers less
than 5 % of the cycle's distance, or produces non-finite values, "failed".
"""
from __future__ import annotations

import math
from collections import deque
from dataclasses import dataclass

WLTP_TOL_KMH = 2.0
EPA_TOL_KMH = 2.0 * 1.609344  # ±2 mph
BAND_S = 1.0  # ± time tolerance of both bands
OUTSIDE_SHARE = 0.01  # share of the duration the trace may spend outside the band
OUTSIDE_MIN_S = 2.0
NOT_DRIVEN_SHARE = 0.05  # below this share of the cycle distance the car did not drive
LIVE_AFTER_S = 60.0  # the live check waits this long before it judges


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
    """Samples the Driver's target and the vehicle speed at every output
    step of a run that has both (the case step, before any decimation of
    what is stored)."""

    def __init__(self, ctx):
        model = ctx.model
        self.ctx = ctx
        self.src = (model.signal_route.get((model.driver, "sig_target_in"))
                    if model.driver and ctx.veh_id else None)
        self.times: list[float] = []
        self.target: list[float] = []
        self.speed: list[float] = []
        self.cycle_m = 0.0  # distance the target asks for so far
        self.live_warned = False

    def sample(self, t: float) -> None:
        if self.src is None:
            return
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


def judge(trace: CycleTrace, distance_m: float, series: dict) -> Verdict:
    """The checks a finished run must pass to be called a success."""
    messages: list[tuple[str, str]] = []
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
        elif m.outside_wltp_s > max(OUTSIDE_MIN_S, OUTSIDE_SHARE * m.duration_s):
            not_followed = True
            messages.append(("warning", (
                f"Cycle not followed: the speed was outside the ±2 km/h, ±1 s trace tolerance "
                f"for {m.outside_wltp_s:.0f} s of {m.duration_s:.0f} s (EPA ±2 mph band: "
                f"{m.outside_epa_s:.0f} s), up to {abs(m.max_err_kmh):.1f} km/h "
                f"{'above' if m.max_err_kmh > 0 else 'below'} the target at "
                f"t = {m.t_max_err:g} s (RMS {m.rms_kmh:.2f} km/h); it drove {km:.2f} of "
                f"{m.cycle_km:.2f} km. Consumption figures per distance are not valid.")))
    return Verdict(messages=tuple(messages), cycle_not_followed=not_followed)
