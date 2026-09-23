"""Shared lookup-table parsing and interpolation.

Table parameters are stored as dicts keyed by the independent variable
(JSON object keys, i.e. numeric text): table1d = {x: y}, table2d =
{x_outer: {x_inner: y}}. Interpolation is linear with clamped (flat)
extrapolation beyond the grid. Every map-based component goes through
this module so behavior cannot drift between components.
"""
from __future__ import annotations

Points1D = list[tuple[float, float]]
Sheets2D = list[tuple[float, Points1D]]


class TableError(ValueError):
    """Raised when tabular parameter data cannot be parsed."""


def parse_table1d(raw: object) -> Points1D:
    if not isinstance(raw, dict) or not raw:
        raise TableError("table1d data must be a non-empty {x: value} mapping")
    points: Points1D = []
    for k, v in raw.items():
        try:
            points.append((float(k), float(v)))  # type: ignore[arg-type]
        except (TypeError, ValueError):
            raise TableError(f"table1d entry '{k}: {v}' is not numeric")
    points.sort(key=lambda p: p[0])
    for (xa, _), (xb, _) in zip(points, points[1:]):
        if xa == xb:
            raise TableError(f"table1d has duplicate key {xa:g}")
    return points


def parse_table2d(raw: object) -> Sheets2D:
    if not isinstance(raw, dict) or not raw:
        raise TableError("table2d data must be a non-empty {x: {y: value}} mapping")
    sheets: Sheets2D = []
    for k, inner in raw.items():
        try:
            x = float(k)  # type: ignore[arg-type]
        except (TypeError, ValueError):
            raise TableError(f"table2d outer key '{k}' is not numeric")
        sheets.append((x, parse_table1d(inner)))
    sheets.sort(key=lambda s: s[0])
    for (xa, _), (xb, _) in zip(sheets, sheets[1:]):
        if xa == xb:
            raise TableError(f"table2d has duplicate outer key {xa:g}")
    return sheets


def interp1(points: Points1D, x: float) -> float:
    """Piecewise-linear lookup with flat extrapolation."""
    if not points:
        return 0.0
    if x <= points[0][0]:
        return points[0][1]
    if x >= points[-1][0] or x != x:  # NaN: no segment holds it
        return points[-1][1]
    # bisection for the first point at or beyond x: it ends the segment that
    # holds x (an interior grid point belongs to the segment it ends)
    lo, hi = 1, len(points) - 1
    while lo < hi:
        mid = (lo + hi) // 2
        if points[mid][0] < x:
            lo = mid + 1
        else:
            hi = mid
    xa, ya = points[lo - 1]
    xb, yb = points[lo]
    if xb == xa:
        return yb
    return ya + (yb - ya) * (x - xa) / (xb - xa)


def interp2(sheets: Sheets2D, x_outer: float, x_inner: float) -> float:
    """Bilinear lookup: interpolate along the inner axis on the two outer
    sheets bracketing x_outer, then linearly between them. Flat clamp on
    both axes outside the grid."""
    if not sheets:
        return 0.0
    if x_outer <= sheets[0][0]:
        return interp1(sheets[0][1], x_inner)
    if x_outer >= sheets[-1][0] or x_outer != x_outer:
        return interp1(sheets[-1][1], x_inner)
    lo, hi = 1, len(sheets) - 1  # bisection, as in interp1
    while lo < hi:
        mid = (lo + hi) // 2
        if sheets[mid][0] < x_outer:
            lo = mid + 1
        else:
            hi = mid
    xa, pa = sheets[lo - 1]
    xb, pb = sheets[lo]
    ya = interp1(pa, x_inner)
    yb = interp1(pb, x_inner)
    if xb == xa:
        return yb
    return ya + (yb - ya) * (x_outer - xa) / (xb - xa)
