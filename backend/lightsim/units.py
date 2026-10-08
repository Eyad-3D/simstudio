"""Values with units, for :meth:`lightsim.Project.set`.

A parameter keeps its value in the unit the library gives it (the unit the
app shows). ``Project.set("E-Motor.max_power_kw", "150 kW")`` checks that
"kW" measures the same thing as the parameter's unit and converts it:
"0.15 MW" stores 150, "150 km/h" for a power raises :class:`UnitError`.
"""
from __future__ import annotations

import math
import re

# unit → (dimension, factor to the dimension's base unit). Dimensions are
# names, not SI exponents: enough to tell kW from km/h, and to convert the
# units LightSim's library uses with their common alternatives.
_UNITS: dict[str, tuple[str, float]] = {}


def _add_named(dimension: str, units: dict[str, float]) -> None:
    for name, factor in units.items():
        _UNITS[name] = (dimension, factor)


_add_named("power", {"W": 1.0, "kW": 1e3, "MW": 1e6, "hp": 745.6999, "PS": 735.49875})
_add_named("energy", {"J": 1.0, "kJ": 1e3, "MJ": 1e6, "Wh": 3600.0, "kWh": 3.6e6, "MWh": 3.6e9})
_add_named("voltage", {"V": 1.0, "kV": 1e3, "mV": 1e-3})
_add_named("current", {"A": 1.0, "kA": 1e3, "mA": 1e-3})
_add_named("charge", {"Ah": 3600.0, "mAh": 3.6, "C": 1.0})
_add_named("resistance", {"Ω": 1.0, "ohm": 1.0, "mΩ": 1e-3, "mohm": 1e-3, "kΩ": 1e3})
_add_named("length", {"m": 1.0, "mm": 1e-3, "cm": 1e-2, "km": 1e3, "in": 0.0254, "ft": 0.3048})
_add_named("area", {"m²": 1.0, "m2": 1.0, "cm²": 1e-4, "cm2": 1e-4})
_add_named("mass", {"kg": 1.0, "g": 1e-3, "t": 1e3, "lb": 0.45359237})
_add_named("time", {"s": 1.0, "ms": 1e-3, "min": 60.0, "h": 3600.0})
_add_named("speed", {"m/s": 1.0, "km/h": 1 / 3.6, "mph": 0.44704})
_add_named("rotation", {"1/min": 1.0, "rpm": 1.0, "rad/s": 60 / (2 * math.pi), "1/s": 60.0})
_add_named("torque", {"N·m": 1.0, "Nm": 1.0, "N*m": 1.0, "kN·m": 1e3, "kNm": 1e3})
_add_named("force", {"N": 1.0, "kN": 1e3})
_add_named("inertia", {"kg·m²": 1.0, "kgm2": 1.0, "kg*m^2": 1.0, "kg·m2": 1.0})
_add_named("pressure", {"Pa": 1.0, "kPa": 1e3, "MPa": 1e6, "bar": 1e5, "mbar": 100.0})
_add_named("percent", {"%": 1.0})
_add_named("mass_flow", {"kg/h": 1.0, "g/s": 3.6, "kg/s": 3600.0})
_add_named("density", {"kg/l": 1.0, "kg/m³": 1e-3, "kg/m3": 1e-3, "g/cm³": 1.0})

# Temperatures have offsets, so they are not in the table.
_TEMPERATURE = {"°C": (1.0, 0.0), "degC": (1.0, 0.0), "K": (1.0, -273.15),
                "°F": (5 / 9, -32 * 5 / 9)}

_VALUE = re.compile(r"^\s*([-+]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][-+]?\d+)?)\s*(.*?)\s*$")


class UnitError(ValueError):
    """A value's unit does not measure what the parameter does, or is unknown."""


def parse(text: str) -> tuple[float, str]:
    """``"150 kW"`` → ``(150.0, "kW")``; ``"0.3"`` → ``(0.3, "")``."""
    m = _VALUE.match(text)
    if not m:
        raise UnitError(f"'{text}' is not a number with an optional unit, such as '150 kW'.")
    return float(m.group(1)), m.group(2)


def convert(value: float, unit: str, to: str) -> float:
    """``value`` in ``unit`` expressed in ``to``. Raises UnitError when they
    measure different things or either is unknown."""
    if unit == to:
        return value
    if unit in _TEMPERATURE and to in _TEMPERATURE:
        a, b = _TEMPERATURE[unit]
        c, d = _TEMPERATURE[to]
        return (value * a + b - d) / c
    src, dst = _UNITS.get(unit), _UNITS.get(to)
    if src is None:
        raise UnitError(f"Unknown unit '{unit}'. Give the value in {to}.")
    if dst is None or src[0] != dst[0]:
        raise UnitError(f"'{unit}' does not measure what this parameter does: it takes {to}.")
    return value * src[1] / dst[1]


def to_unit(text: str, unit: str) -> float:
    """A typed value such as ``"150 kW"`` in ``unit``; a bare number is taken
    to be in ``unit`` already."""
    value, given = parse(text)
    return value if not given or (given == "-" and unit == "-") else convert(value, given, unit)
