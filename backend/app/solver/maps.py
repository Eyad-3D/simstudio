"""Shared lookup-table parsing and interpolation.

Table parameters are stored as dicts keyed by the independent variable
(JSON object keys, i.e. numeric text): table1d = {x: y}, table2d =
{x_outer: {x_inner: y}}. Interpolation is linear. Each axis of a table has
an "outside the data" setting (MOD-18): Error stops the run, Clamp holds the
edge value (the flat extrapolation every table had before 0.3) and Linear
extends the edge segment's slope. An axis with a single point is a
constant: it is never outside. Every map-based component goes through this
module so behavior cannot drift between components.
"""
from __future__ import annotations

from dataclasses import dataclass

Points1D = list[tuple[float, float]]
Sheets2D = list[tuple[float, Points1D]]


class TableError(ValueError):
    """Raised when tabular parameter data cannot be parsed."""


class OutsideDataError(RuntimeError):
    """A table was read outside the data of an axis set to Error."""


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


def interp1(points: Points1D, x: float, linear: bool = False) -> float:
    """Piecewise-linear lookup; beyond the ends flat, or with ``linear`` on
    the slope of the end segment."""
    if not points:
        return 0.0
    if x <= points[0][0]:
        if linear and len(points) > 1 and x < points[0][0]:
            (xa, ya), (xb, yb) = points[0], points[1]
            return ya + (yb - ya) * (x - xa) / (xb - xa)
        return points[0][1]
    if x >= points[-1][0] or x != x:  # NaN: no segment holds it
        if linear and len(points) > 1 and x > points[-1][0]:
            (xa, ya), (xb, yb) = points[-2], points[-1]
            return yb + (yb - ya) * (x - xb) / (xb - xa)
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


def interp2(sheets: Sheets2D, x_outer: float, x_inner: float,
            linear: tuple[bool, bool] = (False, False)) -> float:
    """Bilinear lookup: interpolate along the inner axis on the two outer
    sheets bracketing x_outer, then linearly between them. Outside the grid
    each axis is held flat, or extended on its edge slope where ``linear``
    (outer, inner) says so."""
    if not sheets:
        return 0.0
    lin_outer, lin_inner = linear
    if not sheets[0][0] < x_outer < sheets[-1][0]:  # on or past an edge, or NaN
        edge = 0 if x_outer <= sheets[0][0] else -1
        if (not lin_outer or len(sheets) < 2 or x_outer == sheets[edge][0]
                or x_outer != x_outer):
            return interp1(sheets[edge][1], x_inner, lin_inner)
        lo = 1 if edge == 0 else len(sheets) - 1  # extend the edge pair of sheets
    else:
        lo, hi = 1, len(sheets) - 1  # bisection, as in interp1
        while lo < hi:
            mid = (lo + hi) // 2
            if sheets[mid][0] < x_outer:
                lo = mid + 1
            else:
                hi = mid
    xa, pa = sheets[lo - 1]
    xb, pb = sheets[lo]
    ya = interp1(pa, x_inner, lin_inner)
    yb = interp1(pb, x_inner, lin_inner)
    if xb == xa:
        return yb
    return ya + (yb - ya) * (x_outer - xa) / (xb - xa)


@dataclass
class MapUse:
    """How far a run went past the data of one table axis, or past a
    machine's maximum speed (``what`` = "maximum speed"). Counted at the
    operating point the run used (not at trial lookups), for each solver
    step it was outside, whichever edge; ``value`` is the furthest point
    past ``edge`` and ``t`` when it was reached, both meaningful once
    ``outside_s`` > 0. An axis set to Error never counts: the run stops.
    RunContext.map_use holds every record of a run."""

    el_id: str
    what: str  # "'Full-Load Torque' table" or "maximum speed"
    axis: str  # the axis name, e.g. "Speed"
    unit: str  # its unit, e.g. "1/min"
    edge: float  # the edge of the data the furthest point lies past
    outside_s: float = 0.0  # solver seconds outside
    value: float = 0.0
    t: float = 0.0

    def count(self, value: float, edge: float, t: float, dt: float) -> bool:
        """Add a step outside at ``value``; True the first time."""
        first = self.outside_s == 0.0
        self.outside_s += dt
        if first or abs(value - edge) > abs(self.value - self.edge):
            self.value, self.edge, self.t = value, edge, t
        return first


def _span(xs: list[float]) -> tuple[float, float] | None:
    return (xs[0], xs[-1]) if len(xs) > 1 else None


class Map:
    """A parsed table with its "outside the data" setting per axis
    ('error', 'clamp' or 'linear') and a MapUse record per axis. ``at``
    reads it (an Error axis raises OutsideDataError outside its data);
    ``count`` books the point a step's result was read at."""

    def __init__(self, pts: list, name: str, policy: list[str], uses: list[MapUse]):
        self.name = name  # e.g. "E-Motor 'E-Motor' Full-Load Torque"
        self.policy = policy
        self.uses = uses
        self.linear = tuple(p == "linear" for p in policy)
        self.error = "error" in policy
        self.set(pts)

    def set(self, pts: list) -> None:
        """New data (a live edit); the counts carry on."""
        self.pts = pts
        # the data's range per axis; a 2D table's inner range is its
        # narrowest sheet's, so no sheet is read beyond its own data
        self.ranges = [_span([x for x, _ in pts])]
        if len(self.policy) == 2:
            inner = [p for _, p in pts if len(p) > 1]
            self.ranges.append((max(p[0][0] for p in inner), min(p[-1][0] for p in inner))
                               if inner else None)

    def edge(self, i: int, x: float) -> float | None:
        """The edge of axis i's data that x lies past, or None inside it
        (float noise at an edge, such as a motor at its last speed point,
        is inside)."""
        r = self.ranges[i]
        if r is None:
            return None
        tol = 1e-9 * max(abs(r[0]), abs(r[1]))
        return r[0] if x < r[0] - tol else r[1] if x > r[1] + tol else None

    def at(self, x: float, y: float | None = None) -> float:
        if self.error:
            for i, v in enumerate((x,) if y is None else (x, y)):
                if self.policy[i] == "error" and self.edge(i, v) is not None:
                    lo, hi = self.ranges[i]
                    use = self.uses[i]
                    raise OutsideDataError(
                        f"{self.name}: {use.axis} {v:.6g} {use.unit} is outside its data "
                        f"({lo:g} to {hi:g} {use.unit})")
        if y is None:
            return interp1(self.pts, x, self.linear[0])
        return interp2(self.pts, x, y, self.linear)  # type: ignore[arg-type]

    def count(self, t: float, dt: float, *point: float) -> list[MapUse]:
        """Book ``point`` as where this step's result was read; returns the
        records first left now. (An Error axis never gets here outside its
        data: ``at`` stopped the run.)"""
        first = []
        for i, v in enumerate(point):
            e = self.edge(i, v)
            if e is not None and self.policy[i] != "error" and self.uses[i].count(v, e, t, dt):
                first.append(self.uses[i])
        return first
