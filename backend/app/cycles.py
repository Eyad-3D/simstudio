"""Standard drive cycles bundled with the app (read-only): one
``t_s,speed_kmh`` CSV each in ``cycles/``, described in ``cycles/cycles.json``.
A Driving Task names one in its ``cycle`` parameter; build_model then drives
that trace instead of the typed ``profile``. Duration, distance and top speed
are computed from the trace itself, so what the UI shows is what the solver
drives. Sources and licences: docs/data-register.csv (the ``register`` rows).
"""
from __future__ import annotations

import csv
import json
from functools import lru_cache
from pathlib import Path

DIR = Path(__file__).parent / "cycles"
CYCLES: dict[str, dict] = json.loads((DIR / "cycles.json").read_text(encoding="utf-8"))["cycles"]


@lru_cache(maxsize=None)
def trace(cycle_id: str) -> tuple[tuple[float, float], ...]:
    """The cycle's (time s, speed km/h) points; KeyError for an unknown id."""
    if cycle_id not in CYCLES:
        raise KeyError(cycle_id)
    with (DIR / f"{cycle_id}.csv").open(encoding="utf-8", newline="") as f:
        return tuple((float(r["t_s"]), float(r["speed_kmh"])) for r in csv.DictReader(f))


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
    meta = {k: v for k, v in CYCLES[cycle_id].items() if k != "published"}
    return {"id": cycle_id, **meta, "duration_s": pts[-1][0] - pts[0][0],
            "distance_km": round(km, 3), "vmax_kmh": max(v for _, v in pts)}


def listing() -> list[dict]:
    return [info(c) for c in CYCLES]
