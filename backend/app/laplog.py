"""Import a lap from a data logger or a lap simulator as a drive cycle (STD-35).

A small CSV reader for one job: a lap's speed trace, against time or
distance, from a team's logger export or lap simulator output, made into a
Driving Task profile (time s : speed km/h). It is kept apart from the
general CSV and Excel import (the data-import area's STD-10) on purpose.

Distance-based traces are turned into time here (t = ∫ ds / v), because
the Driving Task follows speed against time; driving speed against
distance (ENG-34) is another area's work, and once it exists a
distance-based lap can be driven as it is.

The presets name the columns each tool is likely to write. The MoTeC i2,
AiM Race Studio, OpenLAP and TUM laptime-simulation layouts are LightSim's
reading of those tools, not checked against teams' files: when a preset
does not find a column, pick it by hand (``columns``).
"""
from __future__ import annotations

import csv
import io
import math
import re
from dataclasses import dataclass, field
from typing import Optional

MAX_BYTES = 20_000_000  # a logger export larger than this is refused
SPEED_UNITS = {"km/h": 1.0, "m/s": 3.6, "mph": 1.609344, "kn": 1.852}
STEP_S = 0.2  # the profile's time step, s
G = 9.80665
ACCEL_SPIKE = 2.5 * G  # a change of speed faster than this is a spike, m/s²
GAP_S = 1.0  # a time step longer than this is a gap in the log
ENDURANCE_M = 22_000.0  # FS Rules 2026 v1.1 (FSG) D 7.1.3
RAMP = 8.0  # m/s²: how hard the trace stops and restarts for a driver change

# column names each preset looks for, in order (lower case; a name matches
# a column that equals it or starts with it followed by a space, "[" or "(")
PRESETS: dict[str, dict] = {
    "Generic": {
        "time": ["time", "t", "t_s", "time_s", "elapsed time"],
        "distance": ["distance", "dist", "s", "distance_m", "lap distance"],
        "speed": ["speed", "v", "velocity", "speed_kmh", "vehicle speed", "gps speed"],
        "lap": ["lap", "lap number", "lap_no", "lapnumber"],
        "speed_unit": "km/h",
        "note": "Any CSV with a time or distance column and a speed column.",
    },
    "GPS logger": {
        "time": ["time", "utc time", "timestamp", "elapsed"],
        "distance": ["distance", "dist"],
        "speed": ["speed", "gps speed", "speed (km/h)", "velocity"],
        "lap": ["lap", "lap number"],
        "speed_unit": "km/h",
        "note": "A GPS logger's CSV: time and GPS speed, a lap column if it has one.",
    },
    "MoTeC i2 CSV": {
        "time": ["time"],
        "distance": ["distance", "lap distance"],
        "speed": ["ground speed", "corr speed", "gps speed", "speed", "vehicle speed"],
        "lap": ["lap number", "lap"],
        "speed_unit": "km/h",
        "note": "MoTeC i2's CSV export: metadata lines, then a header row and a units row. "
                "Layout not yet checked against teams' files.",
    },
    "AiM Race Studio CSV": {
        "time": ["time"],
        "distance": ["distance", "dist on gps speed"],
        "speed": ["gps speed", "speed", "vehicle speed", "wheel speed"],
        "lap": ["lap", "lap number"],
        "speed_unit": "km/h",
        "note": "AiM Race Studio's CSV export: metadata lines, then a header row and a units "
                "row. Layout not yet checked against teams' files.",
    },
    "OpenLAP": {
        "time": ["time"],
        "distance": ["distance"],
        "speed": ["speed"],
        "lap": [],
        "speed_unit": "km/h",
        "note": "OpenLAP's exported results: distance and speed columns. Layout not yet "
                "checked against teams' files.",
    },
    "TUM laptime-simulation": {
        "time": ["t", "time"],
        "distance": ["s", "distance"],
        "speed": ["v", "vel", "speed"],
        "lap": [],
        "speed_unit": "m/s",
        "note": "TUM laptime-simulation's output: s (m), t (s), v (m/s). Layout not yet "
                "checked against teams' files.",
    },
}


class LapLogError(ValueError):
    """The file cannot be read as a lap."""


@dataclass
class LapImport:
    columns: list[str]
    units: dict[str, str]
    preset: str
    picked: dict[str, Optional[str]]  # role → column
    laps: list[dict]  # {lap, duration_s, distance_m}
    lap: Optional[int]  # the lap taken (None: the whole file)
    points: list[tuple[float, float]]  # (t s, speed km/h), every STEP_S and at the end
    distance_m: float  # of the profile, as the solver integrates it
    source_distance_m: Optional[float]  # the lap's own distance column, if any
    warnings: list[str] = field(default_factory=list)
    repeated: int = 1

    def profile(self) -> str:
        return "; ".join(f"{t:g}:{v:g}" for t, v in self.points)

    def as_dict(self) -> dict:
        return {
            "columns": self.columns, "units": self.units, "preset": self.preset,
            "picked": self.picked, "laps": self.laps, "lap": self.lap,
            "duration_s": self.points[-1][0] if self.points else 0.0,
            "distance_m": round(self.distance_m, 2),
            "source_distance_m": (round(self.source_distance_m, 2)
                                  if self.source_distance_m is not None else None),
            "repeated": self.repeated,
            "profile": self.profile(),
            "preview": [[round(t, 2), round(v, 2)] for t, v in self.points[::max(
                1, len(self.points) // 600)]],
            "warnings": self.warnings,
        }


def _number(text: str) -> Optional[float]:
    s = text.strip().strip('"')
    if not s:
        return None
    try:
        return float(s)
    except ValueError:
        pass
    if re.fullmatch(r"-?\d+,\d+", s):  # a decimal comma
        return float(s.replace(",", "."))
    m = re.fullmatch(r"(\d+):(\d{1,2})(?::(\d{1,2}(?:\.\d+)?))?", s)  # m:ss or h:mm:ss
    if m:
        a, b, c = m.groups()
        return (int(a) * 3600 + int(b) * 60 + float(c)) if c else int(a) * 60 + float(b)
    return None


def _rows(text: str) -> list[list[str]]:
    sample = text.splitlines()[:50]

    def score(d: str) -> int:
        """Lines with the most common field count above 1 (a decimal comma
        splits a semicolon file's lines unevenly)."""
        counts = [len(r) for r in csv.reader(sample, delimiter=d) if r]
        top = max(set(counts), key=counts.count) if counts else 1
        return counts.count(top) if top > 1 else 0

    delim = max((";", "\t", ","), key=score)
    return [r for r in csv.reader(io.StringIO(text), delimiter=delim)]


def _table(text: str) -> tuple[list[str], dict[str, str], list[list[Optional[float]]]]:
    """(header, units, numeric rows): the header is the last row before the
    numbers start with two or more fields that are not numbers; a row of
    units right under it is kept."""
    rows = [r for r in _rows(text) if any(c.strip() for c in r)]
    first = None
    for i, r in enumerate(rows):
        nums = [_number(c) for c in r]
        if sum(x is not None for x in nums) >= 2 and sum(x is not None for x in nums) >= len(r) / 2:
            # numbers here and on the next row: the data starts
            nxt = rows[i + 1] if i + 1 < len(rows) else r
            if sum(_number(c) is not None for c in nxt) >= 2:
                first = i
                break
    if first is None or first == 0:
        raise LapLogError("No header row with column names above the numbers was found.")
    header_i = first - 1
    units: dict[str, str] = {}
    # a units row (short texts like km/h, s, m) between the header and the data
    if header_i >= 1 and all(len(c.strip()) <= 8 for c in rows[header_i]) and \
            sum(_number(c) is None for c in rows[header_i - 1]) >= 2 and \
            any(c.strip() in SPEED_UNITS or c.strip() in ("s", "m", "sec") for c in rows[header_i]):
        units_row, header_i = rows[header_i], header_i - 1
    else:
        units_row = []
    header = [c.strip().strip('"') for c in rows[header_i]]
    for name, u in zip(header, units_row):
        if u.strip():
            units[name] = u.strip()
    data = [[_number(c) for c in r] + [None] * (len(header) - len(r)) for r in rows[first:]]
    return header, units, data


def _find(header: list[str], names: list[str]) -> Optional[str]:
    low = [h.lower() for h in header]
    for name in names:
        for h, lh in zip(header, low):
            if lh == name or re.match(re.escape(name) + r"\s*[\[(]", lh):
                return h
    return None


def _unit_of(col: str, units: dict[str, str]) -> Optional[str]:
    if col in units and units[col] in SPEED_UNITS:
        return units[col]
    m = re.search(r"[\[(]\s*([^\])]+?)\s*[\])]", col)
    if m:
        u = m.group(1).lower().replace("kph", "km/h").replace("mps", "m/s")
        return u if u in SPEED_UNITS else None
    return None


def read_lap(text: str, preset: str = "Generic", columns: Optional[dict] = None,
             speed_unit: Optional[str] = None, lap: Optional[int] = None,
             repeat_to_km: float = 0.0, driver_change_s: float = 0.0) -> LapImport:
    """Read a lap from ``text``; see the module docstring. ``columns`` picks
    {"time"|"distance"|"speed"|"lap": column name} over the preset's;
    ``lap`` the lap number to take (None: the fastest full lap when the file
    has a lap column, else the whole file). ``repeat_to_km`` > 0 repeats the
    lap to that distance (22 for a Formula Student endurance), with a stop
    of ``driver_change_s`` at half distance when it is above 0."""
    if len(text) > MAX_BYTES:
        raise LapLogError(f"The file is larger than {MAX_BYTES // 1_000_000} MB.")
    if not 0 <= repeat_to_km <= 100 or not 0 <= driver_change_s <= 3600:
        raise LapLogError("Repeat to at most 100 km, with a stop of 0 to 3,600 s.")
    if preset not in PRESETS:
        raise LapLogError(f"Unknown preset '{preset}'.")
    pre = PRESETS[preset]
    header, units, data = _table(text)
    columns = {k: v for k, v in (columns or {}).items() if v}
    picked = {role: columns.get(role) or _find(header, pre[role])
              for role in ("time", "distance", "speed", "lap")}
    for role, col in picked.items():
        if col is not None and col not in header:
            raise LapLogError(f"There is no column '{col}' for the {role}.")
    if picked["speed"] is None:
        raise LapLogError("No speed column was found: pick it.")
    if picked["time"] is None and picked["distance"] is None:
        raise LapLogError("No time or distance column was found: pick one.")
    unit = speed_unit or _unit_of(picked["speed"], units) or pre["speed_unit"]
    if unit not in SPEED_UNITS:
        raise LapLogError(f"Unknown speed unit '{unit}' (km/h, m/s, mph or kn).")
    idx = {role: header.index(c) if c else None for role, c in picked.items()}
    warnings: list[str] = []

    def col(role, r):
        i = idx[role]
        return r[i] if i is not None and i < len(r) else None

    recs = []
    dropped = 0
    for r in data:
        v = col("speed", r)
        x = col("time", r) if idx["time"] is not None else col("distance", r)
        if v is None or x is None or not math.isfinite(v) or not math.isfinite(x):
            dropped += 1
            continue
        recs.append((x, v * SPEED_UNITS[unit] / 3.6, col("distance", r), col("lap", r)))
    if dropped:
        warnings.append(f"{dropped} rows without a number in the time, distance or speed "
                        f"column were left out.")
    if len(recs) < 3:
        raise LapLogError("The file has fewer than 3 rows with numbers.")
    neg = sum(1 for _, v, _, _ in recs if v < 0)
    if neg:
        warnings.append(f"{neg} negative speeds were set to 0.")
        recs = [(x, max(0.0, v), d, k) for x, v, d, k in recs]

    # the laps, from the lap column
    laps_info: list[dict] = []
    chosen = None
    if idx["lap"] is not None:
        groups: dict[int, list] = {}
        for rec in recs:
            if rec[3] is not None:
                groups.setdefault(int(rec[3]), []).append(rec)
        for k, g in sorted(groups.items()):
            span = g[-1][0] - g[0][0]
            laps_info.append({"lap": k, "span": round(span, 3), "points": len(g)})
        if lap is None and len(groups) > 2:
            # the fastest of the full laps: not the first and last (out and in laps)
            inner = laps_info[1:-1]
            if idx["time"] is not None:
                chosen = min(inner, key=lambda li: li["span"])["lap"]
            else:  # distance-based: the longest is the most complete
                chosen = max(inner, key=lambda li: li["span"])["lap"]
        elif lap is not None:
            if lap not in groups:
                raise LapLogError(f"There is no lap {lap} in the file (laps "
                                  f"{', '.join(str(li['lap']) for li in laps_info)}).")
            chosen = lap
        if chosen is not None:
            recs = groups[chosen]

    # to time against speed
    if idx["time"] is not None:
        ts = [x for x, *_ in recs]
        if any(b < a for a, b in zip(ts, ts[1:])):
            raise LapLogError("The time column goes backwards: pick a lap, or another column.")
        gaps = sum(1 for a, b in zip(ts, ts[1:]) if b - a > GAP_S)
        if gaps:
            warnings.append(f"{gaps} gaps longer than {GAP_S:g} s in the time column were "
                            f"bridged with a straight line.")
        t0 = ts[0]
        tv = [(x - t0, v) for x, v, _, _ in recs]
    else:
        ds = [x for x, *_ in recs]
        if any(b < a for a, b in zip(ds, ds[1:])):
            raise LapLogError("The distance column goes backwards: pick a lap, or another "
                              "column.")
        t = 0.0
        tv = [(0.0, recs[0][1])]
        for (s0, v0, _, _), (s1, v1, _, _) in zip(recs, recs[1:]):
            vm = max(0.5 * (v0 + v1), 0.1)
            t += (s1 - s0) / vm
            tv.append((t, v1))
        warnings.append("The trace is against distance: its time comes from the speed "
                        "(t = ∫ ds / v). LightSim drives speed against time for now.")
    # spikes: a speed change faster than 2.5 g
    spikes = sum(1 for (ta, va), (tb, vb) in zip(tv, tv[1:])
                 if tb > ta and abs(vb - va) / (tb - ta) > ACCEL_SPIKE)
    if spikes:
        warnings.append(f"{spikes} steps change speed faster than 2.5 g: check the speed "
                        f"column for spikes (wheel spin or a lost GPS fix).")
    # resample every STEP_S, and at the trace's own end (the last part of a
    # step, cut off, lost up to 0.2 s of driving: 8 % of a 75 m run)
    pts: list[tuple[float, float]] = []
    j = 0
    t_end = tv[-1][0]
    n = math.floor(t_end / STEP_S + 1e-9)
    times = [k * STEP_S for k in range(n + 1)]
    if round(t_end, 4) > round(times[-1], 4):
        times.append(t_end)
    for t in times:
        while j + 1 < len(tv) and tv[j + 1][0] < t:
            j += 1
        (ta, va), (tb, vb) = tv[j], tv[min(j + 1, len(tv) - 1)]
        v = va if tb <= ta else va + (vb - va) * (t - ta) / (tb - ta)
        pts.append((round(t, 4), round(v * 3.6, 3)))
    src_d = None
    if idx["distance"] is not None:
        dd = [d for _, _, d, _ in recs if d is not None]
        if len(dd) >= 2:
            src_d = dd[-1] - dd[0]
    one = _distance(pts)
    reps = 1
    if repeat_to_km > 0 and one > 0:
        reps = max(1, round(repeat_to_km * 1000.0 / one))
        pts = _repeat(pts, reps, driver_change_s)
    out = LapImport(columns=header, units=units, preset=preset, picked=picked, laps=laps_info,
                    lap=chosen, points=pts, distance_m=_distance(pts),
                    source_distance_m=src_d, warnings=warnings, repeated=reps)
    if src_d and src_d > 0 and reps == 1:
        err = 100.0 * (out.distance_m - src_d) / src_d
        if abs(err) > 0.5:
            warnings.append(f"The imported trace drives {out.distance_m:,.1f} m, {err:+.2f} % "
                            f"from the file's own distance ({src_d:,.1f} m).")
    return out


def _distance(pts: list[tuple[float, float]]) -> float:
    """The trace's distance, m, by trapezoids (as the solver interpolates)."""
    return sum((t1 - t0) * (v0 + v1) / 2.0 for (t0, v0), (t1, v1) in zip(pts, pts[1:])) / 3.6


def _repeat(pts: list[tuple[float, float]], reps: int, stop_s: float) -> list[tuple[float, float]]:
    """The lap ``reps`` times, one after the other (each lap from the time
    the one before ended); with ``stop_s`` > 0, after half the laps the
    trace slows to rest at RAMP, stands for stop_s and speeds up to the
    next lap's start speed at RAMP (the endurance's driver change)."""
    out = list(pts)
    half = reps // 2
    for k in range(1, reps):
        t0 = out[-1][0]
        if stop_s > 0 and k == half:
            v_end, v_start = out[-1][1] / 3.6, pts[0][1] / 3.6
            t_down = v_end / RAMP
            out.append((round(t0 + max(t_down, STEP_S), 4), 0.0))
            t1 = out[-1][0] + stop_s
            out.append((round(t1, 4), 0.0))
            t0 = t1 + max(v_start / RAMP, STEP_S)
            out.append((round(t0, 4), pts[0][1]))
        out += [(round(t0 + t - pts[0][0], 4), v) for t, v in pts[1:]]
    return out
