"""Standard drive cycles bundled with the app (read-only): one
``t_s,speed_kmh[,grade_pct]`` CSV each in ``cycles/``, described in
``cycles/cycles.json`` (CON-04, CON-11). A Driving Task names one in its
``cycle`` parameter; build_model then drives that trace instead of the typed
``profile``. A Road Profile can name a cycle that carries a grade column, and
then takes the road's slope from it, placed along the cycle's own distance.
Duration, distance and top speed are computed from the trace itself, so what
the UI shows is what the solver drives. Sources and licences:
docs/data-register.csv (the ``register`` rows) and DATA-REGISTER.md.

A project may also carry cycles of its own (``Project.cycles``, CON-11): a
speed, and optionally a grade, against time, or a speed and/or a grade
against distance, imported from the user's file. Their ids start with
``own:`` so they never meet a bundled id: a LightSim that does not know the
project's cycles reports an unknown cycle instead of driving another one.
:class:`Catalogue` answers for both kinds; the module functions above it
know only the bundled ones.
"""
from __future__ import annotations

import csv
import json
import math
from functools import lru_cache
from pathlib import Path
from typing import Any, Iterable

DIR = Path(__file__).parent / "cycles"
CYCLES: dict[str, dict] = json.loads((DIR / "cycles.json").read_text(encoding="utf-8"))["cycles"]


@lru_cache(maxsize=None)
def _rows(cycle_id: str) -> tuple[tuple[float, float, float | None], ...]:
    if cycle_id not in CYCLES:
        raise KeyError(cycle_id)
    with (DIR / f"{cycle_id}.csv").open(encoding="utf-8", newline="") as f:
        return tuple((float(r["t_s"]), float(r["speed_kmh"]),
                      float(r["grade_pct"]) if r.get("grade_pct") not in (None, "") else None)
                     for r in csv.DictReader(f))


def trace(cycle_id: str) -> tuple[tuple[float, float], ...]:
    """The cycle's (time s, speed km/h) points; KeyError for an unknown id."""
    return tuple((t, v) for t, v, _ in _rows(cycle_id))


def has_grade(cycle_id: str) -> bool:
    return _rows(cycle_id)[0][2] is not None


@lru_cache(maxsize=None)
def grade_by_distance(cycle_id: str) -> tuple[tuple[float, float], ...]:
    """The cycle's road grade (%) against the distance (m) the trace has
    covered at each point, so a car that lags the trace still meets each hill
    where it is. Empty when the cycle has no grade column."""
    rows = _rows(cycle_id)
    if rows[0][2] is None:
        return ()
    return _grade_along(rows)


def _grade_along(rows) -> tuple[tuple[float, float], ...]:
    """(time s, speed km/h, grade %) rows as grade against the distance the
    speed covers."""
    out, x = [(0.0, rows[0][2])], 0.0
    for (t1, v1, _), (t2, v2, g2) in zip(rows, rows[1:]):
        x += (t2 - t1) * (v1 + v2) / 7.2  # km/h * s / 3.6 / 2
        if x > out[-1][0]:  # standing still adds no distance: keep the first grade
            out.append((x, g2 or 0.0))
    return tuple(out)


def _pairs_text(pairs: Iterable[tuple[float, float]]) -> str:
    return "; ".join(f"{a!r}:{b!r}" for a, b in pairs)


def grade_profile_text(cycle_id: str) -> str:
    """The grade as a Road Profile's 'distance m:grade %' pairs."""
    return _pairs_text((round(x, 3), g) for x, g in grade_by_distance(cycle_id))


def phases(cycle_id: str) -> list[tuple[str, float, float]]:
    return [(str(n), float(a), float(b)) for n, a, b in CYCLES[cycle_id].get("phases", [])]


def not_as_published(params: dict, duration_s: float) -> str:
    """Why a Driving Task set to a bundled cycle drives it differently from
    the published trace, or '' when it drives it as published: a Scale other
    than 100 %, Repeat Profile on a run longer than one pass, or a Profile
    Axis of Distance. Only the published trace has the cycle's phases and
    compares with published results (CON-26). ``params``: the Driving
    Task's values with the case's own applied."""
    cycle_id = str(params.get("cycle") or "")
    try:
        scale = float(params.get("scale_pct", 100))
    except (TypeError, ValueError):
        scale = 100.0
    if abs(scale - 100.0) > 1e-9:
        return f"scaled to {scale:g} %"
    if (bool(params.get("repeat", False)) and cycle_id in CYCLES
            and duration_s > info(cycle_id)["duration_s"] + 1e-6):
        return "repeated (Repeat Profile on a run longer than the cycle)"
    if str(params.get("mode", "time")) == "distance":
        return "read against distance (Profile Axis: Distance)"
    return ""


def profile_text(cycle_id: str) -> str:
    """The trace as a Driving Task profile ('t:speed; ...'). repr keeps every
    digit, so the solver parses exactly the numbers in the file."""
    return _pairs_text(trace(cycle_id))


@lru_cache(maxsize=None)
def info(cycle_id: str) -> dict:
    """What the picker shows: the metadata plus the trace's duration,
    distance (trapezoids, as the solver interpolates) and top speed."""
    pts = trace(cycle_id)
    km = sum((t2 - t1) * (v1 + v2) / 2 for (t1, v1), (t2, v2) in zip(pts, pts[1:])) / 3600
    meta = {k: v for k, v in CYCLES[cycle_id].items() if k not in ("published", "fingerprint")}
    return {"id": cycle_id, **meta, "duration_s": pts[-1][0] - pts[0][0],
            "distance_km": round(km, 3), "vmax_kmh": max(v for _, v in pts),
            "grade": has_grade(cycle_id)}


def listing() -> list[dict]:
    return [info(c) for c in CYCLES]


# ---- a project's own cycles (CON-11) ------------------------------------------

#: The id of a project's own cycle starts with this; no bundled id does.
OWN_PREFIX = "own:"
#: The most points a cycle of one's own may have (a 30-minute log at 50 Hz).
MAX_POINTS = 100_000


def _get(cycle: Any, key: str, default: Any = None) -> Any:
    """A field of a project cycle, given as the schema's model or as a dict."""
    if isinstance(cycle, dict):
        return cycle.get(key, default)
    return getattr(cycle, key, default)


def own_problems(cycle: Any) -> list[str]:
    """Why a project's own cycle cannot be driven, in words; [] when it can.
    The schema checks the shape of each field; these are the checks across
    them."""
    x = list(_get(cycle, "x") or [])
    speed, grade = _get(cycle, "speed"), _get(cycle, "grade")
    axis = _get(cycle, "axis", "time")
    unit = "s" if axis == "time" else "m"
    out: list[str] = []
    if len(x) < 2:
        out.append("it has fewer than two points")
    if speed is None and grade is None:
        out.append("it has neither a speed nor a grade")
    elif speed is None and axis == "time":
        out.append("a cycle against time needs a speed")
    for name, col in (("speed", speed), ("grade", grade)):
        if col is not None and len(col) != len(x):
            out.append(f"it has {len(col)} {name} values for {len(x)} points")
    x_name = "time" if axis == "time" else "distance"
    for name, col in ((x_name, x), ("speed", speed or []), ("grade", grade or [])):
        bad = next((v for v in col if not isinstance(v, (int, float)) or not math.isfinite(v)), None)
        if bad is not None:
            out.append(f"a {name} value is not a number ({bad!r})")
    if out:
        return out
    back = next((i for i in range(1, len(x)) if x[i] < x[i - 1]), None)
    if back is not None:
        out.append(f"its {x_name} goes back from "
                   f"{x[back - 1]:g} {unit} to {x[back]:g} {unit}")
    elif x[-1] <= x[0]:
        out.append(f"it does not move on from {x[0]:g} {unit}")
    if speed is not None and min(speed) < 0:
        out.append(f"a speed is below 0 km/h ({min(speed):g})")
    return out


def own_info(cycle: Any) -> dict:
    """What the picker shows for a project's own cycle, in the bundled
    cycles' terms (``info``) plus its ``axis``, ``speed`` (whether it has
    one) and ``own``. A cycle against distance has no duration of its own;
    ``duration_s`` is then the time its speeds take to cover it, and 0
    without a speed."""
    x = [float(v) for v in _get(cycle, "x") or []]
    speed, grade = _get(cycle, "speed"), _get(cycle, "grade")
    axis = _get(cycle, "axis", "time")
    span = x[-1] - x[0] if len(x) > 1 else 0.0
    if axis == "time":
        km = sum((t2 - t1) * (a + b) / 2 for t1, t2, a, b
                 in zip(x, x[1:], speed, speed[1:])) / 3600 if speed else 0.0
        duration = span
    else:
        km = span / 1000
        duration = sum(2 * (x2 - x1) / ((a + b) / 3.6) for x1, x2, a, b
                       in zip(x, x[1:], speed, speed[1:]) if a + b > 0) if speed else 0.0
    return {"id": _get(cycle, "id"), "name": _get(cycle, "name"), "region": "This project",
            "register": "", "phases": [], "source": _get(cycle, "source", "") or "",
            "note": _get(cycle, "note", "") or "", "duration_s": round(duration, 3),
            "distance_km": round(km, 3), "vmax_kmh": max(speed) if speed else 0.0,
            "grade": grade is not None, "speed": speed is not None, "axis": axis, "own": True}


class Catalogue:
    """The cycles a project can name: the bundled ones and its own. Built
    per model (``Catalogue.of(project)``); a bundled id always means the
    bundled cycle."""

    def __init__(self, own: Iterable[Any] = ()):
        self.own: dict[str, Any] = {}
        for c in own or ():
            self.own.setdefault(str(_get(c, "id")), c)  # Data Checks flag a repeated id

    @classmethod
    def of(cls, project: Any) -> "Catalogue":
        return cls(getattr(project, "cycles", None) or ())

    def knows(self, cycle_id: str) -> bool:
        return cycle_id in CYCLES or cycle_id in self.own

    def is_own(self, cycle_id: str) -> bool:
        return cycle_id not in CYCLES and cycle_id in self.own

    def name(self, cycle_id: str) -> str:
        if cycle_id in CYCLES:
            return CYCLES[cycle_id]["name"]
        return str(_get(self.own.get(cycle_id), "name", "") or cycle_id)

    def axis(self, cycle_id: str) -> str:
        """"time" or "distance": what the cycle's points are against."""
        return "time" if cycle_id in CYCLES else str(_get(self.own[cycle_id], "axis", "time"))

    def problems(self, cycle_id: str) -> list[str]:
        return [] if cycle_id in CYCLES else own_problems(self.own[cycle_id])

    def has_speed(self, cycle_id: str) -> bool:
        return cycle_id in CYCLES or _get(self.own[cycle_id], "speed") is not None

    def has_grade(self, cycle_id: str) -> bool:
        if cycle_id in CYCLES:
            return has_grade(cycle_id)
        return _get(self.own[cycle_id], "grade") is not None

    def profile_text(self, cycle_id: str) -> str:
        """The speed as a Driving Task profile: against time, or against
        distance for a cycle whose axis is distance."""
        if cycle_id in CYCLES:
            return profile_text(cycle_id)
        c = self.own[cycle_id]
        return _pairs_text(zip(map(float, _get(c, "x")), map(float, _get(c, "speed"))))

    def grade_profile_text(self, cycle_id: str) -> str:
        """The grade as a Road Profile's 'distance m:grade %' pairs: a cycle
        against time places it along the distance its speed covers."""
        if cycle_id in CYCLES:
            return grade_profile_text(cycle_id)
        c = self.own[cycle_id]
        x, grade = list(map(float, _get(c, "x"))), list(map(float, _get(c, "grade")))
        if _get(c, "axis", "time") == "distance":
            return _pairs_text(zip(x, grade))
        rows = tuple(zip(x, map(float, _get(c, "speed")), grade))
        return _pairs_text((round(d, 3), g) for d, g in _grade_along(rows))

    def info(self, cycle_id: str) -> dict:
        if cycle_id in CYCLES:
            return {**info(cycle_id), "axis": "time", "speed": True, "own": False}
        return own_info(self.own[cycle_id])
