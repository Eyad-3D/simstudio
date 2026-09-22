"""Profile parsing for the Driving Task and Road Profile ('x:value; x:value; …').

The solver parses each profile once per run and looks points up by binary
search, so a long drive cycle costs next to nothing per solver step.
"""
from __future__ import annotations

from bisect import bisect_left
from operator import itemgetter

_time = itemgetter(0)


def parse_profile(profile: str) -> list[tuple[float, float]]:
    """Parse 't:value; t:value; …' (also accepts ',' as pair separator)."""
    points: list[tuple[float, float]] = []
    for chunk in profile.replace("\n", ";").split(";"):
        chunk = chunk.strip()
        if not chunk:
            continue
        sep = ":" if ":" in chunk else ","
        try:
            t_str, v_str = chunk.split(sep, 1)
            points.append((float(t_str), float(v_str)))
        except ValueError:
            continue
    points.sort(key=_time)
    return points


def interp_profile(points: list[tuple[float, float]], t: float, repeat: bool) -> float:
    if not points:
        return 0.0
    t0, tn = points[0][0], points[-1][0]
    if repeat and tn > t0:
        t = t0 + (t - t0) % (tn - t0)
    if t <= t0:
        return points[0][1]
    if t >= tn:
        return points[-1][1]
    # first point at or after t; its predecessor is before t (same pair a
    # linear scan would find, including at repeated times)
    i = bisect_left(points, t, key=_time)
    (ta, va), (tb, vb) = points[i - 1], points[i]
    if tb == ta:
        return vb
    return va + (vb - va) * (t - ta) / (tb - ta)
