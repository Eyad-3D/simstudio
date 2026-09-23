"""Profile parsing for the Driving Task and Road Profile ('x:value; x:value; …').

The solver parses each profile once per run and looks points up by binary
search, so a long drive cycle costs next to nothing per solver step.
"""
from __future__ import annotations

import math
from bisect import bisect_left
from operator import itemgetter

_time = itemgetter(0)


def _read_profile(profile: str) -> tuple[list[tuple[float, float]], list[str]]:
    """Points in written order, plus the entries that are not pairs of
    finite numbers (skipped)."""
    points: list[tuple[float, float]] = []
    skipped: list[str] = []
    for chunk in profile.replace("\n", ";").split(";"):
        chunk = chunk.strip()
        if not chunk:
            continue
        sep = ":" if ":" in chunk else ","
        try:
            t_str, v_str = chunk.split(sep, 1)
            point = (float(t_str), float(v_str))
        except ValueError:
            skipped.append(chunk)
            continue
        if math.isfinite(point[0]) and math.isfinite(point[1]):
            points.append(point)
        else:
            skipped.append(chunk)
    return points, skipped


def parse_profile(profile: str) -> list[tuple[float, float]]:
    """Parse 't:value; t:value; …' (also accepts ',' as pair separator)."""
    points, _ = _read_profile(profile)
    points.sort(key=_time)
    return points


def profile_problems(profile: str) -> list[tuple[str, str]]:
    """Data-check findings (level, text) for a profile string: entries the
    parser would skip and points out of order are errors (the solver would
    silently drop or re-sort them); a repeated x is a warning (a step)."""
    points, skipped = _read_profile(profile)
    problems: list[tuple[str, str]] = []
    if len(skipped) == 1:
        problems.append(("error", f"'{skipped[0]}' is not an 'x:value' pair of numbers "
                                  f"and would be ignored"))
    elif skipped:
        others = "one other entry" if len(skipped) == 2 else f"{len(skipped) - 1} other entries"
        problems.append(("error", f"'{skipped[0]}' and {others} are not 'x:value' pairs of "
                                  f"numbers and would be ignored"))
    backward = next(((a, b) for (a, _), (b, _) in zip(points, points[1:]) if b < a), None)
    if backward:
        problems.append(("error", f"points are not in ascending order ({backward[1]:g} comes "
                                  f"after {backward[0]:g})"))
    repeated = sorted({a for (a, _), (b, _) in zip(points, points[1:]) if b == a})
    if repeated:
        problems.append(("warning", f"two points at {', '.join(f'{x:g}' for x in repeated[:3])}"
                                    f"{' …' if len(repeated) > 3 else ''} — the value jumps "
                                    f"there"))
    return problems


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
