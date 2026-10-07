"""Standard drive cycles bundled with the app (read-only): one
``t_s,speed_kmh[,grade_pct]`` CSV each in ``cycles/``, described in
``cycles/cycles.json`` (CON-04, CON-11). A Driving Task names one in its
``cycle`` parameter; build_model then drives that trace instead of the typed
``profile``. A Road Profile can name a cycle that carries a grade column, and
then takes the road's slope from it, placed along the cycle's own distance.
Duration, distance and top speed are computed from the trace itself, so what
the UI shows is what the solver drives. Sources and licences:
docs/data-register.csv (the ``register`` rows) and DATA-REGISTER.md.
"""
from __future__ import annotations

import csv
import json
from functools import lru_cache
from pathlib import Path

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
    out, x = [(0.0, rows[0][2])], 0.0
    for (t1, v1, _), (t2, v2, g2) in zip(rows, rows[1:]):
        x += (t2 - t1) * (v1 + v2) / 7.2  # km/h * s / 3.6 / 2
        if x > out[-1][0]:  # standing still adds no distance: keep the first grade
            out.append((x, g2 or 0.0))
    return tuple(out)


def grade_profile_text(cycle_id: str) -> str:
    """The grade as a Road Profile's 'distance m:grade %' pairs."""
    return "; ".join(f"{round(x, 3)!r}:{g!r}" for x, g in grade_by_distance(cycle_id))


def phases(cycle_id: str) -> list[tuple[str, float, float]]:
    return [(str(n), float(a), float(b)) for n, a, b in CYCLES[cycle_id].get("phases", [])]


def profile_text(cycle_id: str) -> str:
    """The trace as a Driving Task profile ('t:speed; ...'). repr keeps every
    digit, so the solver parses exactly the numbers in the file."""
    return "; ".join(f"{t!r}:{v!r}" for t, v in trace(cycle_id))


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
