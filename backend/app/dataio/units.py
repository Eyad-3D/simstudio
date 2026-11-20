"""Units in file headers: read them and convert to the catalog's unit.

A file from a supplier or a test lab names its units in many spellings
("Nm", "N.m", "rpm", "U/min", "kph", "speed_meters_per_second"). This module
maps those spellings to one canonical unit, groups units that measure the
same thing (a speed in km/h, m/s or mph) and converts between them. Full
unit dimensions come later (STD-16); this table covers the units LightSim's
parameters and channels use.

Shared by the table, profile and parameter-sheet imports (STD-10, STD-36)
and the logger-lap import (STD-35).
"""
from __future__ import annotations

import json
import math
import re
from pathlib import Path

# group -> {canonical unit: (scale, offset)}: value_in_base = v * scale + offset.
# The table is units.json, which the frontend's unit chooser reads too: one
# table, so the two cannot disagree. A group's first unit is the one the
# component library stores.
_GROUPS: dict[str, dict[str, tuple[float, float]]] = {
    g: {u: (float(s), float(o)) for u, (s, o) in units.items()}
    for g, units in json.loads(
        (Path(__file__).parent / "units.json").read_text(encoding="utf-8"))["groups"].items()
}

#: unit -> group
GROUP_OF: dict[str, str] = {u: g for g, units in _GROUPS.items() for u in units}

# Spellings, lower-cased and stripped of spaces, -> canonical unit
_ALIASES: dict[str, str] = {
    "kmh": "km/h", "km/hr": "km/h", "kph": "km/h", "kmph": "km/h",
    "kilometersperhour": "km/h", "kilometresperhour": "km/h",
    "ms-1": "m/s", "m.s-1": "m/s", "m/sec": "m/s", "meterspersecond": "m/s",
    "metrespersecond": "m/s", "mps": "m/s", "milesperhour": "mph",
    "rpm": "1/min", "u/min": "1/min", "min-1": "1/min", "min^-1": "1/min", "r/min": "1/min",
    "rev/min": "1/min", "revpermin": "1/min",
    "rad/sec": "rad/s", "rads": "rad/s", "1/sec": "1/s", "hz": "1/s", "rps": "1/s",
    "nm": "N·m", "n.m": "N·m", "n*m": "N·m", "n-m": "N·m", "n·m": "N·m", "n⋅m": "N·m",
    "knm": "kN·m", "kn.m": "kN·m", "kn·m": "kN·m", "lbft": "lbf·ft", "lbf.ft": "lbf·ft",
    "lb-ft": "lbf·ft", "ftlb": "lbf·ft", "ft-lb": "lbf·ft", "lbf·ft": "lbf·ft",
    "kw": "kW", "w": "W", "hp": "hp", "bhp": "hp", "ps": "PS",
    "kwh": "kWh", "wh": "Wh", "j": "J", "kj": "kJ",
    "v": "V", "kv": "kV", "volt": "V", "volts": "V",
    "a": "A", "ka": "kA", "amp": "A", "amps": "A", "ampere": "A",
    "ah": "Ah", "mah": "mAh",
    "kg": "kg", "g": "g", "t": "t", "tonne": "t", "lb": "lb", "lbs": "lb",
    "kg/h": "kg/h", "kg/hr": "kg/h", "kg/s": "kg/s", "g/s": "g/s", "g/h": "g/h",
    "m": "m", "km": "km", "mm": "mm", "cm": "cm", "mi": "mi", "mile": "mi", "miles": "mi",
    "ft": "ft", "meters": "m", "metres": "m", "meter": "m", "metre": "m",
    "kilometers": "km", "kilometres": "km",
    "s": "s", "sec": "s", "secs": "s", "second": "s", "seconds": "s",
    "min": "min", "mins": "min", "minute": "min", "minutes": "min",
    "h": "h", "hr": "h", "hrs": "h", "hour": "h", "hours": "h", "ms": "ms",
    "°c": "°C", "degc": "°C", "deg c": "°C", "degreesc": "°C", "celsius": "°C", "c°": "°C",
    "k": "K", "kelvin": "K", "°f": "°F", "degf": "°F", "fahrenheit": "°F",
    "n": "N", "kn": "kN", "lbf": "lbf",
    "kpa": "kPa", "pa": "Pa", "bar": "bar", "mpa": "MPa", "psi": "psi",
    "1/m": "1/m", "m-1": "1/m", "1/km": "1/km",
    "ohm": "Ω", "ω": "Ω",
    "%": "%", "percent": "%", "pct": "%",
    "-": "-", "": "-", "1": "-",
}

# FASTSim-style column suffixes: speed_meters_per_second, time_seconds
_SUFFIXES: dict[str, str] = {
    "meters_per_second": "m/s", "metres_per_second": "m/s", "miles_per_hour": "mph",
    "kilometers_per_hour": "km/h", "kilometres_per_hour": "km/h", "seconds": "s",
    "meters": "m", "metres": "m", "kilometers": "km", "kilometres": "km", "kmh": "km/h",
    "kph": "km/h", "mps": "m/s", "mph": "mph", "rpm": "1/min", "nm": "N·m", "kw": "kW",
    "percent": "%", "pct": "%", "degc": "°C",
    # short ones last: "t_s", "distance_m" (as LightSim's own CSV export writes)
    "s": "s", "m": "m", "km": "km", "min": "min", "h": "h",
}

# units where only the case of the first letter tells milli (mW) from mega
# (MW), a factor of 10^9: never folded, read by that letter
_MILLI_OR_MEGA: dict[str, str] = {"w": "W", "wh": "Wh", "j": "J", "ω": "Ω", "ohm": "Ω"}

# headers that are units by themselves ("rpm", "kW") but not these: a
# letter alone names a quantity as often as a unit (t, v, s, m, n), and
# "min" is as often a minimum
_NOT_A_BARE_UNIT = {"min"}

_BRACKETS = re.compile(r"^(.*?)\s*[\[(]\s*([^\])]*?)\s*[\])]\s*$")
_IN_UNIT = re.compile(r"^(.*?)\s+in\s+(\S+)\s*$", re.IGNORECASE)


def canonical(unit: str) -> str | None:
    """The canonical spelling of ``unit`` ("Nm" -> "N·m"), or None when it is
    not a unit this module knows."""
    u = unit.strip()
    if u in GROUP_OF:
        return u
    key = u.lower().replace(" ", "").replace("²", "^2")
    key = key.replace("⋅", "·")
    if key[:1] == "m" and key[1:] in _MILLI_OR_MEGA:
        c = u[0] + _MILLI_OR_MEGA[key[1:]]
        return c if c in GROUP_OF else None
    if key in _ALIASES:
        return _ALIASES[key]
    return None


def split_header(text: str) -> tuple[str, str | None, str | None]:
    """(name, unit, unit as written) of a column header.

    "speed [km/h]" -> ("speed", "km/h", "km/h"); "n (rpm)" -> ("n", "1/min",
    "rpm"); "speed_meters_per_second" -> ("speed", "m/s", ...); a header that
    is only a unit, "rad/s" -> ("rad/s", "rad/s", "rad/s"); "Torque" ->
    ("Torque", None, None). A bracketed unit this module does not know comes
    back as (name, None, written) so the caller can name it in a message.
    """
    t = str(text).strip()
    m = _BRACKETS.match(t)
    if m:
        written = m.group(2)
        return m.group(1).strip(), canonical(written), written
    m = _IN_UNIT.match(t)
    if m and canonical(m.group(2)):
        return m.group(1).strip(), canonical(m.group(2)), m.group(2)
    low = t.lower()
    for suffix, unit in _SUFFIXES.items():
        if low.endswith("_" + suffix) and len(low) > len(suffix) + 1:
            return t[: -len(suffix) - 1], unit, t[-len(suffix):]
    if (len(t) > 1 or not t.isalpha()) and low not in _NOT_A_BARE_UNIT:
        unit = canonical(t)
        if unit and unit != "-":
            return t, unit, t
    return t, None, None


def group(unit: str) -> str | None:
    c = canonical(unit)
    return GROUP_OF.get(c) if c else None


def same_group(a: str, b: str) -> bool:
    ga, gb = group(a), group(b)
    return ga is not None and ga == gb


def alternatives(unit: str) -> list[str]:
    """The units ``unit`` can be converted from (its group), itself first."""
    c = canonical(unit) or unit
    g = GROUP_OF.get(c)
    if g is None:
        return [unit]
    return [c] + [u for u in _GROUPS[g] if u != c]


def factor(src: str, dst: str) -> tuple[float, float]:
    """(scale, offset) so that value_dst = value_src * scale + offset.

    Raises ValueError when the two are not the same kind of quantity."""
    a, b = canonical(src), canonical(dst)
    if a is None:
        raise ValueError(f"'{src}' is not a unit LightSim knows")
    if b is None:
        raise ValueError(f"'{dst}' is not a unit LightSim knows")
    if a == b:
        return 1.0, 0.0
    ga, gb = GROUP_OF[a], GROUP_OF[b]
    if ga != gb:
        raise ValueError(f"{src} cannot be converted to {dst}")
    sa, oa = _GROUPS[ga][a]
    sb, ob = _GROUPS[gb][b]
    # base = v*sa + oa ; v_dst = (base - ob) / sb
    return sa / sb, (oa - ob) / sb


def convert(value: float, src: str, dst: str) -> float:
    s, o = factor(src, dst)
    return value * s + o


def tidy(value: float) -> float:
    """A converted value without float noise (60.00000000000001 -> 60.0)."""
    if value == 0 or not math.isfinite(value):
        return value
    return float(f"{value:.12g}")
