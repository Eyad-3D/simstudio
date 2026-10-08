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

A Driving Task whose Profile Axis is Distance sets a speed against the
distance driven: its trace is judged against distance (distance_trace_metrics),
each point's band spanning the target's lowest and highest value within the
distance the car covers in ±1 s at the target speed (at least ±BAND_MIN_M).

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

A battery with an Output Power Limit or a Voltage Class gets its checks as
summary rows with the limit and pass/fail (terminal_checks): its terminal
power (V·I), averaged over the check window, against the limit; its highest
voltage (open-circuit at 100 % SOC, or at its terminals while recuperating)
against the class; and whether it kept energy above its minimum SOC. A
failed power or voltage check makes the run at best "warning".
"""
from __future__ import annotations

import math
from bisect import bisect_left, bisect_right
from collections import deque
from dataclasses import dataclass
from typing import Callable, Iterable

from .maps import MapUse
from .profiles import distance_axis
from .runtime import TerminalCheck

WLTP_TOL_KMH = 2.0
EPA_TOL_KMH = 2.0 * 1.609344  # ±2 mph
BAND_S = 1.0  # ± time tolerance of both bands
BAND_MIN_M = 2.0  # ± distance tolerance at least, for a trace against distance
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
        # a Driving Task's speed against distance is judged against distance
        self.by_distance = (self.src_kind == "signal.driving_task"
                            and distance_axis(ctx.params(self.src[0])))
        self.times: list[float] = []
        self.target: list[float] = []
        self.speed: list[float] = []
        self.dist: list[float] = []  # distance driven, m
        self.next_t = 0.0
        self.cycle_m = 0.0  # distance the target asks for so far
        self.live_warned = False

    def sample(self, t: float, last: bool = False) -> None:
        """Called after every solver step with its end time ``t``; records a
        sample when ``t`` reaches the next multiple of TRACE_STEP_S, or when
        ``last`` (the run's final instant)."""
        full = self.ctx.full_throttle  # an acceleration test needs no target
        if self.src is None and not full:
            return
        if (t < self.next_t - 1e-9 and not self.ctx.performance
                and not (last and self.times and t > self.times[-1] + 1e-9)):
            return
        self.next_t = (math.floor(t / TRACE_STEP_S + 1e-6) + 1) * TRACE_STEP_S
        if full:
            target = 0.0  # not driven to: no cycle distance to judge
        elif self.src_kind is not None:
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
        self.dist.append(self.ctx.distance)

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
        if self.by_distance:
            return distance_trace_metrics(self.times, self.dist, self.target, self.speed)
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


class _RangeMinMax:
    """Lowest and highest of a list over any index range, in O(1) per query
    (sparse tables)."""

    def __init__(self, values: list[float]):
        self.lo, self.hi = [values], [values]
        k = 1
        while 2 * k <= len(values):
            lo, hi = self.lo[-1], self.hi[-1]
            self.lo.append([min(lo[i], lo[i + k]) for i in range(len(lo) - k)])
            self.hi.append([max(hi[i], hi[i + k]) for i in range(len(hi) - k)])
            k *= 2

    def __call__(self, a: int, b: int) -> tuple[float, float]:
        """(min, max) of values[a:b], b > a."""
        j = (b - a).bit_length() - 1
        lo, hi = self.lo[j], self.hi[j]
        return min(lo[a], lo[b - (1 << j)]), max(hi[a], hi[b - (1 << j)])


def distance_trace_metrics(ts: list[float], xs: list[float], tgt: list[float],
                           spd: list[float]) -> TraceMetrics | None:
    """Trace error of a run whose target is a speed against distance: as
    trace_metrics, but each point's band spans the target's lowest and
    highest value over the samples within the distance driven in ±BAND_S at
    the point's target speed (at least ±BAND_MIN_M) of its distance ``xs``
    (m, never falling). The time outside the band still counts in s."""
    n = len(ts)
    if n < 2:
        return None
    rng = _RangeMinMax(tgt)
    out_wltp = out_epa = sq = 0.0
    worst, t_worst = 0.0, ts[0]
    for i in range(1, n):
        w = max(BAND_MIN_M, tgt[i] / 3.6 * BAND_S)
        a, b = bisect_left(xs, xs[i] - w - 1e-9), bisect_right(xs, xs[i] + w + 1e-9)
        t_lo, t_hi = rng(a, b)
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


# label, value, unit, limit, passed, and the row's stable key (SummaryValue.key)
CheckRow = tuple[str, float, str, float | None, bool | None, str]


@dataclass
class Verdict:
    # (level, text): an "error" fails the run, a "warning" keeps it from success
    messages: tuple[tuple[str, str], ...] = ()
    cycle_not_followed: bool = False
    broke_down: bool = False  # non-finite values: no number of the run is valid
    # summary rows of a performance or acceleration test: (label, value,
    # unit, limit, passed, key)
    rows: tuple[CheckRow, ...] = ()
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


def terminal_checks(el_id: str, label: str, chk: TerminalCheck, left_kwh: float,
                    depleted: bool) -> tuple[list[CheckRow], list[str]]:
    """Summary rows and warnings of battery ``label`` (element ``el_id``)'s TerminalCheck; its
    usable energy left, kWh, passes unless it reached its minimum SOC
    (``depleted``). The power rows need a limit, the maximum voltage a class."""
    rows: list[CheckRow] = []
    warnings: list[str] = []
    if chk.limit_w > 0:
        limit_kw = chk.limit_w / 1000.0
        ok = chk.avg_peak_w <= chk.limit_w * (1.0 + 1e-9)
        rows += [
            (f"{label} — peak terminal power", chk.peak_w / 1000.0, "kW", None, None,
             f"{el_id}.peak_terminal_power_kw"),
            (f"{label} — peak terminal power, averaged", chk.avg_peak_w / 1000.0, "kW",
             limit_kw, ok, f"{el_id}.peak_terminal_power_averaged_kw"),
            (f"{label} — time {'held at' if chk.enforced else 'over'} the output power limit",
             chk.limit_s, "s", None, None, f"{el_id}.time_at_power_limit_s"),
        ]
        if not ok:
            over = f", averaged over {chk.window_s:g} s," if chk.window_s > 0 else ""
            warnings.append(f"Battery '{label}' broke its Output Power Limit: its terminal "
                            f"power{over} reached {chk.avg_peak_w / 1000.0:.1f} kW at "
                            f"t = {chk.t_avg_peak:.2f} s against {limit_kw:g} kW.")
    if chk.v_class > 0:
        v_max = max(chk.v_full, chk.v_peak)
        ok = v_max <= chk.v_class * (1.0 + 1e-9)
        rows.append((f"{label} — maximum pack voltage", v_max, "V", chk.v_class, ok,
                     f"{el_id}.max_pack_voltage_v"))
        if not ok:
            where = ("open-circuit at 100 % SOC" if chk.v_full >= chk.v_peak
                     else f"at its terminals {'while recuperating ' if chk.p_at_v_peak < 0 else ''}"
                          f"at t = {chk.t_v_peak:.2f} s")
            warnings.append(f"Battery '{label}' exceeds its Voltage Class of {chk.v_class:g} V: "
                            f"{v_max:.1f} V {where}.")
    rows += [
        (f"{label} — minimum pack voltage", chk.v_min, "V", None, None,
         f"{el_id}.min_pack_voltage_v"),
        (f"{label} — usable energy left", left_kwh, "kWh", None, not depleted,
         f"{el_id}.usable_energy_left_kwh"),
    ]
    return rows, warnings


def _time_to(ts: list[float], spd: list[float], level: float,
             also: list[float] | None = None) -> float | None:
    """When the speed (or the distance) first reaches ``level``: linear
    between the sample before and the first one at or above it; with
    ``also``, that series' value there instead of the time."""
    ys = ts if also is None else also
    for k, v in enumerate(spd):
        if v >= level:
            if k == 0:
                return ys[0]
            return ys[k - 1] + (level - spd[k - 1]) / (v - spd[k - 1]) * (ys[k] - ys[k - 1])
    return None


def judge(trace: CycleTrace, distance_m: float, series: dict, performance: bool = False,
          duration_s: float = 0.0, uses: Iterable[MapUse] = (), stopped: bool = False,
          case=None) -> Verdict:
    """The checks a finished run must pass to be called a success; ``uses``
    are the run's MapUse records, ``duration_s`` the time it solved,
    ``stopped`` whether a stop or an error cut it short and ``case`` the
    SimCase (for an acceleration test's line and reference time)."""
    messages: list[tuple[str, str]] = []
    rows: list[CheckRow] = []
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
                f"Cycle not followed: the speed was outside the ±2 km/h, "
                f"{'±1 s of travel' if trace.by_distance else '±1 s'} trace tolerance "
                f"for {m.outside_wltp_s:.0f} s of {m.duration_s:.0f} s (EPA ±2 mph band: "
                f"{m.outside_epa_s:.0f} s), up to {abs(m.max_err_kmh):.1f} km/h "
                f"{'above' if m.max_err_kmh > 0 else 'below'} the target at "
                f"t = {m.t_max_err:g} s (RMS {m.rms_kmh:.2f} km/h); it drove {km:.2f} of "
                f"{m.cycle_km:.2f} km. Consumption figures per distance are not valid.")))
    end_d = trace.ctx.end_distance
    if (end_d is not None and not trace.ctx.full_throttle and not stopped
            and distance_m < end_d - 1e-6):
        # a case that ends at a distance or a lap count ran out of time first
        what = (f"{case.endLaps:g} laps ({end_d:,.0f} m)"
                if case is not None and not case.endDistance and case.endLaps else f"{end_d:,.0f} m")
        messages.append(("warning", f"The vehicle did not drive the case's {what} within its "
                                    f"{duration_s:g} s duration; it drove {distance_m:,.0f} m. "
                                    f"Make the Duration longer: it is the run's time limit "
                                    f"here."))
    if trace.ctx.full_throttle and trace.times:
        rows += _acceleration(trace, case, stopped, messages)
    elif performance and trace.times:
        level, v_max = max(trace.target), max(trace.speed)
        rows.append(("Maximum speed", v_max, "km/h", None, None, "max_speed_kmh"))
        t_level = _time_to(trace.times, trace.speed, level)
        if t_level is None:
            if not stopped:  # a stopped run only did not get there yet
                messages.append(("info", f"Performance test: the vehicle did not reach the "
                                         f"{level:.4g} km/h target; its maximum speed was "
                                         f"{v_max:.1f} km/h."))
        elif t_level > trace.times[0]:  # no time when it started at the target or above
            rows.append((f"Time to {level:.4g} km/h", t_level, "s", None, None,
                         "time_to_target_s"))
    model = trace.ctx.model
    beyond = beyond_data(uses, duration_s,
                         lambda el: f"{model.cdef_of[el].name} '{model.elements[el].label}'")
    messages += [("warning", text) for text, _ in beyond]
    return Verdict(messages=tuple(messages), cycle_not_followed=not_followed, broke_down=bool(bad),
                   rows=tuple(rows), beyond_reason="; ".join(reason for _, reason in beyond))


def _acceleration(trace: CycleTrace, case, stopped: bool,
                  messages: list[tuple[str, str]]) -> list[CheckRow]:
    """The rows of an acceleration test (SimCase.kind "acceleration"): the
    time from the start line to the line ``endDistance`` past it, with the
    case duration as its limit, and the speed there, each read inside the
    solver step that crossed the line; the time to 100 km/h from t = 0; each
    battery's peak terminal power (when its power check does not give it)
    and mean terminal power; and the share of the run a driven wheel spent
    at the tyres' grip limit."""
    ctx, rows = trace.ctx, []
    ts, dist = trace.times, trace.dist
    start = max(0.0, case.startLine)
    d = case.endDistance if case.endDistance and case.endDistance > 0 else None
    t_end = _time_to(ts, dist, start + d) if d else None
    if t_end is not None:
        timed = t_end - _time_to(ts, dist, start)
        rows += [(f"Time to {d:g} m", timed, "s", case.duration, True,
                  "accel_time_s"),
                 (f"Speed at {d:g} m", _time_to(ts, dist, start + d, trace.speed),
                  "km/h", None, None, "accel_end_speed_kmh")]
        if case.referenceTime and case.referenceTime > 0:  # the form marks 0 or less red
            rows.append(("Gap to reference time", timed - case.referenceTime, "s",
                         None, None, "accel_gap_to_reference_s"))
    elif d and not stopped:  # a stopped run only did not get there yet
        messages.append(("warning", f"Acceleration test: the vehicle did not reach the {d:g} m "
                                    f"line within the case's {case.duration:g} s (its time "
                                    f"limit); it drove {max(0.0, dist[-1] - start):.1f} m of "
                                    f"the {d:g} m."))
    t100 = _time_to(ts, trace.speed, 100.0)
    if t100 is not None and t100 > ts[0]:
        rows.append(("Time to 100 km/h", t100, "s", None, None, "time_to_100_kmh_s"))
    run_s = ts[-1] - ts[0]
    for b in ctx.batteries.values():
        label = ctx.model.elements[b.el_id].label
        if b.check is None or b.check.limit_w <= 0:
            rows.append((f"{label} — peak terminal power", b.p_peak_w / 1000.0, "kW",
                         None, None, f"{b.el_id}.peak_terminal_power_kw"))
        if run_s > 0:
            rows.append((f"{label} — mean terminal power",
                         (b.energy_out_wh - b.energy_in_wh) * 3.6 / run_s, "kW",
                         None, None, f"{b.el_id}.mean_terminal_power_kw"))
    if run_s > 0:
        rows.append(("Time at the tyres' grip limit",
                     100.0 * ctx.grip_limited_s / run_s, "%", None, None,
                     "grip_limit_time_pct"))
    if ctx.batteries and not any(b.check and b.check.limit_w > 0 for b in ctx.batteries.values()):
        messages.append(("info", "Acceleration test: no battery has an Output Power Limit, so "
                                 "the terminal power was not checked against one. Formula "
                                 "Student allows 80 kW at the accumulator outlet, judged on a "
                                 "500 ms moving average (FS Rules 2026 v1.1 (FSG) EV 2.2.1 and "
                                 "D 10.4.1; FSUK and FSAE may differ, check the current "
                                 "season's rules): apply the battery's 'Formula Student "
                                 "Electric' preset."))
    h = float(ctx.params(ctx.veh_id).get("cg_height_m", 0) or 0) if ctx.veh_id else 0.0
    load = ("load transfer is included, one solver step (up to 10 ms) behind" if h > 0 else
            "the wheel loads do not shift as the car accelerates (the Vehicle's Centre of "
            "Gravity Height is 0), so a rear-driven car's launch grip is pessimistic and a "
            "front-driven car's optimistic")
    messages.append(("info", f"Acceleration test results are estimates: {load}; the tyres' grip "
                             f"depends on their load only through a Wheel's Load Sensitivity; and "
                             f"nothing limits wheelspin, so at "
                             f"the grip limit the power figures include the power that spins "
                             f"the wheels."))
    return rows
