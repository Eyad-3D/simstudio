"""Stable keys for the run summary's rows (AI-07).

The rows docs/spec/results.md lists set their key where they are made. Any
other row gets one here, after the run, by the same rule: a part's figure
("<part label> — <figure>") is "<part id>.<figure>_<unit>", a run's figure
"<figure>_<unit>", in lower case with underscores. A part's key follows its
id, so renaming the part keeps it.
"""
from __future__ import annotations

from ..schemas import Project, SummaryValue
from .maps import slug

UNITS = {
    "%": "pct", "kWh/100km": "kwh_per_100km", "l/100km": "l_per_100km", "g/km": "g_per_km",
    "1/min": "rpm", "km/h": "kmh", "N·m": "nm", "-": "", "": "",
}


def unit_slug(unit: str) -> str:
    """A unit as the end of a key: "%" → "pct", "kWh" → "kwh", "l/100km" →
    "l_per_100km"."""
    if unit in UNITS:
        return UNITS[unit]
    return slug(unit.replace("/", " per "))


def fill_keys(summary: list[SummaryValue], project: Project) -> None:
    """Give every row without a key one, unique within the summary."""
    ids: dict[str, str] = {}
    seen_labels: set[str] = set()
    for system in project.systems:
        for el in system.elements:
            if el.label in seen_labels:  # two parts with one label: neither is named by it
                ids.pop(el.label, None)
            else:
                ids[el.label] = el.id
                seen_labels.add(el.label)
    used = {s.key for s in summary if s.key}
    for s in summary:
        if s.key:
            continue
        part, sep, figure = s.label.partition(" — ")
        base = f"{ids[part]}.{slug(figure)}" if sep and part in ids else slug(s.label)
        unit = unit_slug(s.unit)
        key = f"{base}_{unit}" if unit and not base.endswith(f"_{unit}") else base
        n, unique = 2, key
        while unique in used:
            unique, n = f"{key}_{n}", n + 1
        s.key = unique
        used.add(unique)
