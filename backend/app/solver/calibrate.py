"""Calibrate lap mode on a team's logged lap and check it on another (VAL-38).

From a logged lap (distance or time, speed and lateral acceleration; pack
power optional), LightSim builds the track the car drove: its curvature is
the lateral acceleration over the speed squared, smoothed over SMOOTH_M.
It then finds the grip scale (all wheels' μ, lateral μ and load sensitivity times one
factor) and
the downforce (the Vehicle's CzA) whose lap mode speed fits the logged
speed against distance best (least squares, on a coarse grid refined
twice around its best point). The prediction drives another logged lap,
blind, with those values, and reports the lap time error, the speed
trace's RMS error and, when the log has the pack power, the energy error.

The logs stay on the team's computer: they are read in the request and not
stored. The fit varies only the grip and the downforce; the powertrain,
mass and drag are the model's own.
"""
from __future__ import annotations

import math
from dataclasses import dataclass, field
from typing import Optional

from .. import laplog
from ..schemas import Project, SimCase
from . import lapsim
from .core import simulate
from .domains import RunContext
from .maps import OutsideDataError
from .network import build_model
from .runtime import Runtime

SMOOTH_M = 3.0  # m: the moving average over which the curvature is smoothed
POINT_M = 1.0  # m: the track table's spacing
V_FLOOR = 5.0  # m/s: below this a curvature from ay/v² is noise
MU_GRID = (0.6, 1.5)
CZA_GRID = (0.0, 5.0)
CASE_ID = "__calibrate__"


@dataclass
class LoggedLap:
    """A logged lap against distance, every POINT_M."""
    s: list[float]  # m
    v: list[float]  # m/s
    kappa: list[float]  # 1/m
    time_s: float  # the lap's logged time
    energy_kwh: Optional[float] = None  # from the pack power, if logged
    warnings: list[str] = field(default_factory=list)


ROLES = {
    "time": ["time", "t", "elapsed time"],
    "distance": ["distance", "lap distance", "dist", "s"],
    "speed": ["speed", "ground speed", "gps speed", "v", "vehicle speed"],
    "lat_accel": ["lateral acceleration", "lat accel", "g lat", "lateral g", "ay", "acc lat",
                  "gps lat acc", "latacc"],
    "power": ["pack power", "battery power", "power", "ts power"],
    "lap": ["lap number", "lap"],
}


def read_logged_lap(text: str, columns: Optional[dict] = None, lap: Optional[int] = None,
                    speed_unit: str = "km/h", accel_unit: Optional[str] = None,
                    power_unit: str = "kW") -> LoggedLap:
    """A logged lap's speed, curvature and energy against distance. The
    lateral acceleration is in g when its values stay within ±5 (unless
    ``accel_unit`` says "m/s²" or "g"); the power in kW (or W)."""
    header, _, data = laplog._table(text)
    columns = {k: v for k, v in (columns or {}).items() if v}
    picked = {r: columns.get(r) or laplog._find(header, names) for r, names in ROLES.items()}
    if picked["speed"] is None or picked["lat_accel"] is None:
        raise laplog.LapLogError("Calibration needs a speed and a lateral acceleration column: "
                                 "pick them.")
    if picked["time"] is None and picked["distance"] is None:
        raise laplog.LapLogError("No time or distance column was found: pick one.")
    idx = {r: header.index(c) if c else None for r, c in picked.items()}
    if speed_unit not in laplog.SPEED_UNITS:
        raise laplog.LapLogError(f"Unknown speed unit '{speed_unit}'.")

    def val(r, row):
        i = idx[r]
        return row[i] if i is not None and i < len(row) else None

    # rows with a number in every column the lap is read from (a logger
    # often leaves the distance blank for its first samples)
    used = [r for r in ("speed", "lat_accel", "time", "distance") if idx[r] is not None]
    rows = [r for r in data if all(val(k, r) is not None for k in used)]
    if idx["lap"] is not None:
        laps = sorted({int(val("lap", r)) for r in rows if val("lap", r) is not None})
        if lap is None and len(laps) > 2:
            # the fastest full lap, as the lap import takes it
            def span(k):
                rs = [r for r in rows if val("lap", r) == k]
                key = "time" if idx["time"] is not None else "distance"
                return val(key, rs[-1]) - val(key, rs[0])
            lap = min(laps[1:-1], key=span) if idx["time"] is not None else max(laps[1:-1], key=span)
        if lap is not None:
            rows = [r for r in rows if val("lap", r) == lap]
    if len(rows) < 10:
        raise laplog.LapLogError("The lap has fewer than 10 rows with numbers.")
    k_v = laplog.SPEED_UNITS[speed_unit] / 3.6
    v = [max(0.0, val("speed", r) * k_v) for r in rows]
    ay = [val("lat_accel", r) for r in rows]
    in_g = accel_unit == "g" or (accel_unit is None and max(abs(a) for a in ay) <= 5.0)
    ay = [a * 9.80665 if in_g else a for a in ay]
    # distance and time
    if idx["distance"] is not None:
        s = [val("distance", r) - val("distance", rows[0]) for r in rows]
    else:
        tt = [val("time", r) for r in rows]
        s = [0.0]
        for i in range(1, len(rows)):
            s.append(s[-1] + 0.5 * (v[i - 1] + v[i]) * (tt[i] - tt[i - 1]))
    if idx["time"] is not None:
        tt = [val("time", r) - val("time", rows[0]) for r in rows]
    else:
        tt = [0.0]
        for i in range(1, len(rows)):
            tt.append(tt[-1] + (s[i] - s[i - 1]) / max(0.5 * (v[i - 1] + v[i]), 0.1))
    if any(b < a for a, b in zip(s, s[1:])):
        raise laplog.LapLogError("The distance goes backwards: pick one lap.")
    energy = None
    if idx["power"] is not None:
        pw = [(val("power", r) or 0.0) * (1.0 if power_unit == "kW" else 1e-3) for r in rows]
        energy = sum(0.5 * (pw[i - 1] + pw[i]) * (tt[i] - tt[i - 1])
                     for i in range(1, len(rows))) / 3600.0
    # resample every POINT_M, then smooth the curvature
    length = s[-1]
    if not length > 0:
        raise laplog.LapLogError("The lap's distance does not change (or the car does not "
                                 "move): pick the distance or speed column of a lap driven.")
    n = max(5, int(length / POINT_M))
    grid = [length * k / n for k in range(n + 1)]
    pairs_v = list(zip(s, v))
    pairs_k = list(zip(s, [a / max(x, V_FLOOR) ** 2 for a, x in zip(ay, v)]))
    vg = [_interp(pairs_v, x) for x in grid]
    kg = [_interp(pairs_k, x) for x in grid]
    half = max(1, int(SMOOTH_M / (2 * length / n)))
    ks = [sum(kg[max(0, i - half):i + half + 1]) / len(kg[max(0, i - half):i + half + 1])
          for i in range(len(kg))]
    ks = [max(-lapsim.KAPPA_MAX, min(lapsim.KAPPA_MAX, k)) for k in ks]
    # the sign: the rules of lap mode do not care which way a corner turns
    return LoggedLap(s=grid, v=vg, kappa=ks, time_s=tt[-1], energy_kwh=energy)


def _interp(pts: list[tuple[float, float]], x: float) -> float:
    lo, hi = 0, len(pts) - 1
    if x <= pts[0][0]:
        return pts[0][1]
    if x >= pts[-1][0]:
        return pts[-1][1]
    while hi - lo > 1:
        mid = (lo + hi) // 2
        if pts[mid][0] <= x:
            lo = mid
        else:
            hi = mid
    (x0, y0), (x1, y1) = pts[lo], pts[hi]
    return y0 if x1 == x0 else y0 + (y1 - y0) * (x - x0) / (x1 - x0)


def _overrides(project: Project, log: LoggedLap, mu_scale: float, cza: float) -> dict:
    """Case overrides: the logged track on the Race Track, the grip scaled
    on every wheel and the Vehicle's downforce."""
    els = [e for s in project.systems for e in s.elements]
    track = next((e for e in els if e.componentDefId == "track.lap"), None)
    veh = next((e for e in els if e.componentDefId == "vehicle.body"), None)
    if track is None or veh is None:
        raise laplog.LapLogError("Calibration needs a Vehicle and a Race Track in the model.")
    ov: dict = {track.id: {
        "layout": "Custom", "laps": 1, "closed": True, "sector_ends": "",
        "curvature_table": {f"{x:g}": round(k, 6) for x, k in zip(log.s, log.kappa)},
        "elevation_table": {"0": 0}}, veh.id: {"downforce_cza_m2": cza}}
    base = build_model(project, {}, {}).params_of  # (the library's defaults filled in)
    for e in els:
        if e.componentDefId == "propulsion.wheel":
            b = base[e.id]
            # the whole grip curve scales: μ, μ_y and the load sensitivity
            ov[e.id] = {key: float(b.get(key, 0) or 0) * mu_scale
                        for key in ("mu", "mu_lateral", "mu_load_sensitivity_per_kN")
                        if float(b.get(key, 0) or 0) != 0}
    return ov


def _speed_rms(project: Project, log: LoggedLap, mu_scale: float, cza: float) -> float:
    """RMS of lap mode's speed against the logged speed, m/s, at the log's
    points (the lap from the logged start speed)."""
    ov = _overrides(project, log, mu_scale, cza)
    gear_of: dict = {}
    model = build_model(project, gear_of, ov)
    ctx = RunContext(project, model, Runtime(model, None), gear_of, ov)
    ctx.v = log.v[0]
    lap = lapsim.LapRun(ctx, 2.0 * POINT_M)  # (the fit at 2 m: half the cost)
    prof = lap.solve(log.v[0])
    pts = [(i * lap.track.ds, v) for i, v in enumerate(prof.v)]
    errs = [_interp(pts, x) - v for x, v in zip(log.s, log.v)]
    return math.sqrt(sum(e * e for e in errs) / len(errs))


def calibrate(project: Project, log: LoggedLap) -> dict:
    """The grip scale and CzA that fit ``log`` best: {"mu_scale", "cza",
    "rms_kmh", "evaluations"}."""
    best = (math.inf, 1.0, 0.0)
    evals = 0
    mu_lo, mu_hi = MU_GRID
    c_lo, c_hi = CZA_GRID
    for level in range(3):
        n = 7 if level == 0 else 5
        for i in range(n):
            mu = mu_lo + (mu_hi - mu_lo) * i / (n - 1)
            for j in range(n):
                cz = c_lo + (c_hi - c_lo) * j / (n - 1)
                try:
                    err = _speed_rms(project, log, mu, cz)
                except (lapsim.LapError, OutsideDataError):  # (a run that cannot drive it)
                    err = math.inf
                evals += 1
                if err < best[0]:
                    best = (err, mu, cz)
        d_mu, d_c = (mu_hi - mu_lo) / (n - 1), (c_hi - c_lo) / (n - 1)
        mu_lo, mu_hi = max(0.3, best[1] - d_mu), best[1] + d_mu
        c_lo, c_hi = max(0.0, best[2] - d_c), best[2] + d_c
    if not math.isfinite(best[0]):
        raise laplog.LapLogError("Lap mode cannot drive the logged lap with any grip and "
                                 "downforce tried: check the log's units and the model's "
                                 "maps.")
    return {"mu_scale": round(best[1], 4), "cza": round(best[2], 3),
            "rms_kmh": round(best[0] * 3.6, 3), "evaluations": evals}


def predict(project: Project, log: LoggedLap, mu_scale: float, cza: float) -> dict:
    """Drive ``log``'s track with the calibrated values (a full lap case:
    the energy too) and compare: lap time, speed RMS and energy errors."""
    ov = _overrides(project, log, mu_scale, cza)
    proj = project.model_copy(deep=True)
    proj.cases = [SimCase(id=CASE_ID, name="Calibration check", kind="lap", outputEvery=1,
                          parameterOverrides=ov)]
    veh = next(e for s in proj.systems for e in s.elements if e.componentDefId == "vehicle.body")
    veh.parameterOverrides["initial_speed_kmh"] = log.v[0] * 3.6
    res = simulate(proj, CASE_ID)
    rows = {s.label: s.value for s in res.summary}
    out = {"status": res.status, "lap_time_log_s": round(log.time_s, 3),
           "messages": [m.text for m in res.messages if m.level in ("warning", "error")]}
    if "Lap time" not in rows:
        return out
    t = rows["Lap time"]
    out.update(lap_time_model_s=t, lap_time_error_pct=round(100.0 * (t - log.time_s) / log.time_s, 2),
               speed_rms_kmh=round(_speed_rms(project, log, mu_scale, cza) * 3.6, 3))
    e = rows.get("Energy per lap")
    if e is not None:
        out["energy_model_kwh"] = e
    if log.energy_kwh is not None and e is not None and log.energy_kwh > 0:
        out.update(energy_log_kwh=round(log.energy_kwh, 4),
                   energy_error_pct=round(100.0 * (e - log.energy_kwh) / log.energy_kwh, 2))
    return out
