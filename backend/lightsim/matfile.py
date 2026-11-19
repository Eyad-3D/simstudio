"""A small writer for MATLAB's Level 5 MAT-file format (``save -v6``).

Writes what a run's results need and nothing more: real double matrices,
character rows and scalar structs of those. MATLAB, GNU Octave and SciPy's
``scipy.io.loadmat`` read the files. No compression, so a file is about
8 bytes a number. Format: "MAT-File Format", The MathWorks, 2023 (Level 5
MAT-files), the same layout ``save -v6`` writes.
"""
from __future__ import annotations

import math
import re
import struct
from pathlib import Path
from typing import Mapping, Sequence, Union

# data types and array classes of the Level 5 format
_MI_INT8, _MI_INT32, _MI_UINT32, _MI_DOUBLE, _MI_MATRIX, _MI_UTF16 = 1, 5, 6, 9, 14, 17
_MX_STRUCT, _MX_CHAR, _MX_DOUBLE = 2, 4, 6

#: the longest variable or field name MATLAB accepts (namelengthmax)
MAX_NAME = 63

Value = Union[float, int, str, Sequence[float], Mapping[str, "Value"]]


def _pad8(data: bytes) -> bytes:
    return data + b"\0" * (-len(data) % 8)


def _element(mi_type: int, data: bytes) -> bytes:
    return struct.pack("<II", mi_type, len(data)) + _pad8(data)


def _matrix(name: str, value: Value) -> bytes:
    name_b = name.encode("ascii")
    if isinstance(value, Mapping):
        fields = list(value)
        width = max([len(f) for f in fields] + [1]) + 1  # each name and a NUL
        body = (_element(_MI_UINT32, struct.pack("<II", _MX_STRUCT, 0))
                + _element(_MI_INT32, struct.pack("<ii", 1, 1))
                + _element(_MI_INT8, name_b)
                # the field-name length uses the small element format
                + struct.pack("<HHi", _MI_INT32, 4, width)
                + _element(_MI_INT8, b"".join(f.encode("ascii").ljust(width, b"\0")
                                              for f in fields))
                + b"".join(_matrix("", value[f]) for f in fields))
    elif isinstance(value, str):
        units = value.encode("utf-16-le")
        n = len(units) // 2
        body = (_element(_MI_UINT32, struct.pack("<II", _MX_CHAR, 0))
                + _element(_MI_INT32, struct.pack("<ii", 1 if n else 0, n))
                + _element(_MI_INT8, name_b)
                + _element(_MI_UTF16, units))
    else:
        column = [float(value)] if isinstance(value, (int, float)) else [float(v) for v in value]
        body = (_element(_MI_UINT32, struct.pack("<II", _MX_DOUBLE, 0))
                + _element(_MI_INT32, struct.pack("<ii", len(column), 1))
                + _element(_MI_INT8, name_b)
                + _element(_MI_DOUBLE, struct.pack(f"<{len(column)}d", *column)))
    return struct.pack("<II", _MI_MATRIX, len(body)) + body


def valid_name(text: str, taken: set[str] | None = None) -> str:
    """``text`` as a MATLAB variable or field name: letters, digits and
    underscores, starting with a letter, at most 63 characters, and not one
    of ``taken`` (which it is added to)."""
    name = re.sub(r"[^0-9A-Za-z_]+", "_", text).strip("_") or "x"
    if not name[0].isalpha():
        name = "x_" + name
    name = name[:MAX_NAME]
    if taken is not None:
        base, k = name, 2
        while name in taken:
            suffix = f"_{k}"
            name = base[:MAX_NAME - len(suffix)] + suffix
            k += 1
        taken.add(name)
    return name


def write(path: str | Path, variables: Mapping[str, Value],
          description: str = "LightSim results") -> None:
    """Write ``variables`` (name → number, list of numbers (a column), text
    or a dict of those (a struct)) to ``path``. None in a list becomes NaN."""
    header = f"MATLAB 5.0 MAT-file, {description}".encode("ascii", "replace")[:116]
    out = [header.ljust(116, b" "), b"\0" * 8, struct.pack("<H", 0x0100), b"IM"]
    for name, value in variables.items():
        out.append(_matrix(valid_name(name), _nan(value)))
    Path(path).write_bytes(b"".join(out))


def _nan(value: Value) -> Value:
    if isinstance(value, Mapping):
        return {valid_name(k): _nan(v) for k, v in value.items()}
    if isinstance(value, (str, int, float)):
        return value
    return [math.nan if v is None else v for v in value]
