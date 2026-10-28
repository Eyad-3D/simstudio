"""The ANALYTIC line: made-up speed traces whose exact time outside the
speed-trace band is known from the regulations' own words, against the
time the run verdict (app/solver/verdict.py) gives for them.

The rules, as published:

* 40 CFR 1066.425(b)(1)-(2) (annual edition of 1 July 2024): "The upper
  limit is 2.0 mi/hr higher than the highest point on the trace within 1.0
  s of the given point in time. The lower limit is 2.0 mi/hr lower than the
  lowest point on the trace within 1.0 s of the given time."
* Regulation (EU) 2017/1151, Annex XXI, Sub-Annex 6, 2.6.8.3.1 (WLTP):
  "Upper limit: 2,0 km/h higher than the highest point of the trace within
  ± 1,0 second of the given point in time; Lower limit: 2,0 km/h lower than
  the lowest point of the trace within ± 1,0 second of the given time."

So at every instant t the band is [min - tol, max + tol] of the target
over [t - 1 s, t + 1 s] (cut to the trace's span), and the time outside is
the measure of the instants where the driven speed is beyond it. Both the
target and the driven speed here are piecewise linear, so the band's edges
are piecewise linear too, and every crossing is the root of a linear
function: the exact time outside is computed in rational arithmetic
(fractions), with 2.0 mi/h = 3.218688 km/h exactly (1 mi = 1.609344 km).
"""
from __future__ import annotations

from fractions import Fraction as F

MPH = F("1.609344")
BANDS = {"epa": 2 * MPH, "wltp": F(2)}  # km/h
WINDOW = F(1)  # s
STEP = F(1, 10)  # the verdict's sampling of a run's trace (verdict.TRACE_STEP_S)


def pwl(points):
    return [(F(t), F(v)) for t, v in points]


def at(f, t):
    """The piecewise-linear function ``f`` (knots (t, v)) at ``t`` (held
    beyond its ends)."""
    if t <= f[0][0]:
        return f[0][1]
    for (t0, v0), (t1, v1) in zip(f, f[1:]):
        if t <= t1:
            return v0 + (v1 - v0) * (t - t0) / (t1 - t0)
    return f[-1][1]


def _lines(f, t0, t1):
    """The target's candidates for its extreme over the window of every t
    in (t0, t1): its knots inside the window (constants) and its values at
    the window's two ends (lines in t); as (slope, intercept)."""
    lo, hi = f[0][0], f[-1][0]
    mid = (t0 + t1) / 2
    a, b = max(lo, mid - WINDOW), min(hi, mid + WINDOW)
    out = [(F(0), v) for t, v in f if a < t < b]
    for edge, shift in ((a, -WINDOW), (b, WINDOW)):
        if edge == lo or edge == hi:  # the window is cut at the trace's end there
            out.append((F(0), at(f, edge)))
        else:  # f(t + shift) on the piece of f that holds it
            for (u0, v0), (u1, v1) in zip(f, f[1:]):
                if u0 <= mid + shift <= u1:
                    s = (v1 - v0) / (u1 - u0)
                    out.append((s, v0 + s * (shift - u0)))
                    break
    return out


def exact_outside(target, speed, tol) -> F:
    """The measure of the instants in the trace's span where ``speed`` is
    above the highest or below the lowest point of ``target`` within 1 s,
    by more than ``tol``: exact."""
    lo, hi = target[0][0], target[-1][0]
    cuts = {lo, hi}
    for t, _ in target:
        cuts |= {t - WINDOW, t, t + WINDOW}
    cuts |= {t for t, _ in speed}
    cuts = sorted(c for c in cuts if lo <= c <= hi)
    total = F(0)
    for t0, t1 in zip(cuts, cuts[1:]):
        cands = _lines(target, t0, t1)
        # the speed on this piece, as a line
        s0, s1 = at(speed, t0), at(speed, t1)
        vs = ((s1 - s0) / (t1 - t0), s0 - (s1 - s0) / (t1 - t0) * t0)
        # where any two of the lines cross inside, the extremes may switch
        pts = {t0, t1}
        lines = cands + [vs]
        for i, (m1, c1) in enumerate(lines):
            for m2, c2 in lines[i + 1:]:
                if m1 != m2:
                    x = (c2 - c1) / (m1 - m2)
                    if t0 < x < t1:
                        pts.add(x)
        # the edges themselves, shifted by tol, crossed by the speed
        for m, c in cands:
            for shift in (tol, -tol):
                if m != vs[0]:
                    x = (c + shift - vs[1]) / (vs[0] - m)
                    if t0 < x < t1:
                        pts.add(x)
        pts = sorted(pts)
        for a, b in zip(pts, pts[1:]):
            mid = (a + b) / 2
            vals = [m * mid + c for m, c in cands]
            v = vs[0] * mid + vs[1]
            if v > max(vals) + tol or v < min(vals) - tol:
                total += b - a
    return total


# the five traces of the acceptance file: (name, target knots, speed knots), km/h against s
TRACES = [
    ("plateau excursion", pwl([(0, 50), (60, 50)]),
     pwl([(0, 50), (10, 50), (16, 56), (20, 56), (26, 50), (60, 50)])),
    ("lag on a ramp", pwl([(0, 0), (10, 0), (40, 60), (60, 60)]),
     pwl([(0, 0), (13, 0), (43, 60), (60, 60)])),
    ("overshoot at a peak", pwl([(0, 30), (20, 30), (30, 70), (40, 30), (60, 30)]),
     pwl([(0, 30), (20, 30), (31, 76.5), (41, 30), (60, 30)])),
    ("short excursion", pwl([(0, 80), (60, 80)]),
     pwl([(0, 80), (20, 80), (20.5, 85), (21.9, 85), (22.4, 80), (60, 80)])),
    ("slow car below a ramp", pwl([(0, 0), (5, 0), (15, 100), (60, 100)]),
     pwl([(0, 0), (5, 0), (25, 100), (60, 100)])),
]


def measure() -> list[dict]:
    """Each trace's exact time outside the two bands against the verdict's,
    sampled every STEP as a run's trace is."""
    import sys
    from pathlib import Path
    sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "backend"))
    from app.solver import verdict

    rows = []
    for name, target, speed in TRACES:
        n = int((target[-1][0] - target[0][0]) / STEP)
        ts = [target[0][0] + k * STEP for k in range(n + 1)]
        m = verdict.trace_metrics([float(t) for t in ts], [float(at(target, t)) for t in ts],
                                  [float(at(speed, t)) for t in ts])
        for band, tol in BANDS.items():
            exact = exact_outside(target, speed, tol)
            got = m.outside_epa_s if band == "epa" else m.outside_wltp_s
            rows.append({"trace": name, "band": band, "exact_s": float(exact),
                         "exact": str(exact), "verdict_s": got, "error_s": abs(got - float(exact))})
    return rows


def labelled() -> dict:
    """Whether a run's "cycle not followed" message says its 1 %/2 s
    allowance is LightSim's own rule: the message of a run that leaves the
    band, as the verdict writes it."""
    import sys
    from pathlib import Path
    sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "backend"))
    import inspect

    from app.solver import verdict
    text = inspect.getsource(verdict.judge)
    start = text.find("Cycle not followed")
    words = text[start:start + 900] if start >= 0 else ""
    return {"found": start >= 0, "labelled": "LightSim's own" in words or "LightSim's rule" in words}
