"""Compact answers about runs for the AI tools (AI-03).

One result of the Battery Electric Car is about 750 KB of JSON, far too much
for an assistant to read. These helpers turn a run into statistics per
channel (minimum, maximum, mean and end value over a time window), a thinned
series of at most :data:`MAX_POINTS` points that keeps each stretch's peaks,
and the differences between two runs.
"""
from __future__ import annotations

import json
import math
from typing import Optional

from ..schemas import Channel, Project, SimCase, SimResult, SummaryValue
from .overview import fmt_number

MAX_POINTS = 500


def kpi_key(label: str) -> str:
    """A stable-looking key for a summary row until the run summary carries
    its own keys (AI-07): the label, lower case, words joined by dots and
    underscores ("HV Battery Pack — final SOC" → "hv_battery_pack.final_soc")."""
    parts = [p.strip() for p in label.replace("—", "|").split("|")]
    words = [
        "_".join("".join(ch if ch.isalnum() else " " for ch in p.lower()).split())
        for p in parts
    ]
    return ".".join(w for w in words if w) or "value"


def summary_rows(summary: list[SummaryValue]) -> list[dict]:
    rows = []
    for s in summary:
        row: dict = {"key": kpi_key(s.label), "label": s.label, "value": s.value, "unit": s.unit}
        if s.notValid:
            row["notValid"] = s.notValid
        if s.limit is not None:
            row["limit"] = s.limit
        if s.passed is not None:
            row["passed"] = s.passed
        rows.append(row)
    return rows


def channel_id(ch: Channel) -> str:
    return f"{ch.elementId}:{ch.portId}"


def find_channels(result: SimResult, wanted: Optional[list[str]]) -> tuple[list[Channel], list[str]]:
    """The channels asked for (by "element:port" id, or a word of their
    label), and the requests that matched nothing."""
    if not wanted:
        return list(result.channels), []
    found: list[Channel] = []
    missing = []
    for w in wanted:
        w_low = w.strip().lower()
        hits = [c for c in result.channels if channel_id(c).lower() == w_low]
        if not hits:
            hits = [c for c in result.channels if w_low and w_low in c.label.lower()]
        if not hits:
            missing.append(w)
        for c in hits:
            if c not in found:
                found.append(c)
    return found, missing


def _window(ch: Channel, t_from: Optional[float], t_to: Optional[float]) -> list[tuple[float, float]]:
    pts = []
    for p in ch.timeSeries:
        t, v = p.get("t"), p.get("value")
        if t is None or v is None or not math.isfinite(v):
            continue
        if t_from is not None and t < t_from:
            continue
        if t_to is not None and t > t_to:
            continue
        pts.append((t, v))
    return pts


def channel_stats(ch: Channel, t_from: Optional[float] = None, t_to: Optional[float] = None) -> dict:
    """Minimum, maximum, time-weighted mean and end value of the channel in
    the window (gaps, where the channel had no data yet, are skipped)."""
    pts = _window(ch, t_from, t_to)
    out: dict = {"channel": channel_id(ch), "label": ch.label, "unit": ch.unit, "points": len(pts)}
    if not pts:
        return out
    values = [v for _, v in pts]
    if len(pts) > 1 and pts[-1][0] > pts[0][0]:
        area = sum((t1 - t0) * (v0 + v1) / 2 for (t0, v0), (t1, v1) in zip(pts, pts[1:]))
        mean = area / (pts[-1][0] - pts[0][0])
    else:
        mean = sum(values) / len(values)
    i_min = min(range(len(values)), key=values.__getitem__)
    i_max = max(range(len(values)), key=values.__getitem__)
    out.update({
        "min": _round(values[i_min]), "tAtMin": pts[i_min][0],
        "max": _round(values[i_max]), "tAtMax": pts[i_max][0],
        "mean": _round(mean),
        "end": _round(values[-1]), "tEnd": pts[-1][0],
    })
    return out


def thinned(ch: Channel, t_from: Optional[float], t_to: Optional[float], points: int) -> list[list[float]]:
    """At most ``points`` [t, value] pairs: the window cut into buckets, and
    each bucket's lowest and highest point kept in time order, so peaks and
    dips survive the thinning."""
    pts = _window(ch, t_from, t_to)
    points = max(2, min(MAX_POINTS, points))
    if len(pts) <= points:
        return [[t, _round(v)] for t, v in pts]
    buckets = max(1, points // 2)
    size = len(pts) / buckets
    out: list[tuple[float, float]] = []
    for b in range(buckets):
        chunk = pts[int(b * size): int((b + 1) * size)] or pts[-1:]
        lo = min(chunk, key=lambda p: p[1])
        hi = max(chunk, key=lambda p: p[1])
        out.extend(sorted({lo, hi}))
    if out[-1] != pts[-1]:
        out[-1] = pts[-1]
    return [[t, _round(v)] for t, v in out[:points]]


def _round(v: float) -> float:
    if v == 0 or not math.isfinite(v):
        return v
    digits = max(0, 6 - int(math.floor(math.log10(abs(v)))) - 1)
    return round(v, digits)


def effective_values(project: Project, case: Optional[SimCase]) -> dict[tuple[str, str], object]:
    """(element label, parameter key) → value as the run used it: the
    part's own value with the case's change on top."""
    out: dict[tuple[str, str], object] = {}
    overrides = (case.parameterOverrides if case else None) or {}
    for system in project.systems:
        for el in system.elements:
            values = {**el.parameterOverrides, **overrides.get(el.id, {})}
            for key, value in values.items():
                out[(el.label, key)] = value
    return out


def model_differences(a: Optional[tuple[Project, SimCase]], b: Optional[tuple[Project, SimCase]],
                      limit: int = 60) -> list[str]:
    """What differs between the models and cases two runs were made with."""
    if a is None or b is None:
        return ["One of the runs keeps no copy of its model (runs from before 0.3.0), "
                "so the model differences are unknown."]
    (pa, ca), (pb, cb) = a, b
    out: list[str] = []
    labels_a = {e.label for s in pa.systems for e in s.elements}
    labels_b = {e.label for s in pb.systems for e in s.elements}
    out += [f'Part "{lbl}" only in run A' for lbl in sorted(labels_a - labels_b)]
    out += [f'Part "{lbl}" only in run B' for lbl in sorted(labels_b - labels_a)]
    va, vb = effective_values(pa, ca), effective_values(pb, cb)
    for key in sorted(set(va) | set(vb)):
        x, y = va.get(key, "(default)"), vb.get(key, "(default)")
        if json.dumps(x, sort_keys=True) != json.dumps(y, sort_keys=True):
            out.append(f'"{key[0]}" {key[1]}: {_short(x)} → {_short(y)}')
    for field in ("kind", "duration", "timeStep", "endDistance", "startLine"):
        x, y = getattr(ca, field, None), getattr(cb, field, None)
        if x != y:
            out.append(f"Case {field}: {x} → {y}")
    if len(out) > limit:
        out = out[:limit] + [f"… {len(out) - limit} more differences not shown"]
    return out or ["No differences in the model or the case."]


def _short(value: object) -> str:
    if isinstance(value, bool):
        return "on" if value else "off"
    if isinstance(value, (int, float)):
        return fmt_number(float(value))
    if isinstance(value, dict):
        return f"table ({len(value)} rows)"
    text = str(value)
    return f"code, {text.count(chr(10)) + 1} lines" if "\n" in text else json.dumps(text[:60])


def kpi_changes(a: list[SummaryValue], b: list[SummaryValue]) -> list[dict]:
    """Each key result of either run, with both values and the change."""
    rows_a = {kpi_key(s.label): s for s in a}
    rows_b = {kpi_key(s.label): s for s in b}
    out = []
    for key in list(rows_a) + [k for k in rows_b if k not in rows_a]:
        sa, sb = rows_a.get(key), rows_b.get(key)
        row: dict = {"key": key, "label": (sa or sb).label, "unit": (sa or sb).unit,
                     "a": sa.value if sa else None, "b": sb.value if sb else None}
        if sa and sb:
            row["change"] = _round(sb.value - sa.value)
            if sa.value:
                row["changePct"] = round(100 * (sb.value - sa.value) / abs(sa.value), 2)
        for side, s in (("a", sa), ("b", sb)):
            if s and s.notValid:
                row[f"notValid_{side}"] = s.notValid
        out.append(row)
    return out
