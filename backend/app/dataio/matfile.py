"""A small writer for MATLAB .mat files (Level 5, the format MATLAB and
SciPy's ``scipy.io.loadmat`` both read).

LightSim writes results as structs of number columns and text, so this
writer handles exactly that: real double arrays (vectors and matrices),
text (char rows), cell arrays and structs (one or many), nested to any
depth. Each top-level variable is zlib-compressed, as MATLAB 7 and later
write by default. No MATLAB, NumPy or SciPy is needed to write a file.

Format: MATLAB "MAT-File Format" (MathWorks, Level 5 MAT-files).
"""
from __future__ import annotations

import math
import re
import struct
import time
import zlib
from array import array
from typing import Any, Mapping, Sequence

# data types
_miINT8, _miINT32, _miUINT16, _miUINT32, _miDOUBLE, _miMATRIX, _miCOMPRESSED, _miUTF16 = (
    1, 5, 4, 6, 9, 14, 15, 17)
# array classes
_mxCELL, _mxSTRUCT, _mxCHAR, _mxDOUBLE = 1, 2, 4, 6

#: Longest field or variable name MATLAB accepts.
MAX_NAME = 63
_IDENT = re.compile(r"[^A-Za-z0-9_]+")


class Matrix:
    """A 2-D double array given row by row (``rows[i][j]``); None is NaN."""

    def __init__(self, rows: Sequence[Sequence[float | None]]):
        self.rows = [list(r) for r in rows]


class StructArray:
    """A 1-by-n struct array: every element has the same fields."""

    def __init__(self, items: Sequence[Mapping[str, Any]]):
        self.items = list(items)


class Cell:
    """A cell array given row by row (``rows[i][j]``), any values inside."""

    def __init__(self, rows: Sequence[Sequence[Any]]):
        self.rows = [list(r) for r in rows]


def identifier(text: str, taken: set[str] | None = None, prefix: str = "x") -> str:
    """A valid MATLAB name for ``text``: letters, digits and underscores,
    starting with a letter, at most 63 characters, unique among ``taken``
    (which it is added to). "HV Battery Pack" -> "HV_Battery_Pack"."""
    name = _IDENT.sub("_", text.strip()).strip("_")
    if not name or not name[0].isalpha():
        name = f"{prefix}_{name}" if name else prefix
    # cut where a space or dash was, the name would end in "_": not a
    # name identifier() gives back, which mat_bytes() refuses
    name = name[:MAX_NAME].rstrip("_")
    if taken is None:
        return name
    base, n = name, 2
    while name.lower() in taken:
        suffix = f"_{n}"
        name = base[: MAX_NAME - len(suffix)].rstrip("_") + suffix
        n += 1
    taken.add(name.lower())
    return name


def _pad8(n: int) -> int:
    return (8 - n % 8) % 8


def _element(dtype: int, payload: bytes) -> bytes:
    """A data element: tag (type, byte count), data, padding to 8 bytes."""
    return struct.pack("<II", dtype, len(payload)) + payload + b"\0" * _pad8(len(payload))


def _doubles(values: Sequence[float | None]) -> bytes:
    a = array("d", (math.nan if v is None else float(v) for v in values))
    if struct.pack("=H", 1) != struct.pack("<H", 1):  # pragma: no cover - big-endian host
        a.byteswap()
    return a.tobytes()


def _header_parts(cls: int, dims: tuple[int, int], name: str) -> bytes:
    flags = _element(_miUINT32, struct.pack("<II", cls, 0))
    dims_el = _element(_miINT32, struct.pack("<ii", *dims))
    name_el = _element(_miINT8, name.encode("ascii"))
    return flags + dims_el + name_el


def _matrix(value: Any, name: str = "") -> bytes:
    """One value as a miMATRIX element named ``name`` ("" inside structs and cells)."""
    if isinstance(value, bool):
        value = float(value)
    if value is None:
        body = _header_parts(_mxDOUBLE, (0, 0), name) + _element(_miDOUBLE, b"")
    elif isinstance(value, (int, float)):
        body = _header_parts(_mxDOUBLE, (1, 1), name) + _element(_miDOUBLE, _doubles([value]))
    elif isinstance(value, str):
        # text as MATLAB writes it (16-bit characters); text with characters
        # beyond ASCII ("N·m", "°C") is marked UTF-16, so SciPy reads it as
        # written too, not only MATLAB
        units = value.encode("utf-16-le")
        n = len(units) // 2
        body = _header_parts(_mxCHAR, (1 if n else 0, n), name) + _element(_miUINT16 if value.isascii() else _miUTF16, units)
    elif isinstance(value, Matrix):
        rows = value.rows
        r = len(rows)
        c = len(rows[0]) if r else 0
        # column-major
        flat = [rows[i][j] for j in range(c) for i in range(r)]
        body = _header_parts(_mxDOUBLE, (r, c), name) + _element(_miDOUBLE, _doubles(flat))
    elif isinstance(value, Cell):
        rows = value.rows
        r = len(rows)
        c = len(rows[0]) if r else 0
        body = _header_parts(_mxCELL, (r, c), name) + b"".join(
            _matrix(rows[i][j]) for j in range(c) for i in range(r))
    elif isinstance(value, (Mapping, StructArray)):
        items = value.items if isinstance(value, StructArray) else [value]
        fields: list[str] = []
        for it in items:
            for k in it:
                if k not in fields:
                    fields.append(k)
        for f in fields:
            if f != identifier(f) or len(f) > MAX_NAME:
                raise ValueError(f"'{f}' is not a valid MATLAB field name")
        # each name in a fixed-width slot with its terminating zero, as MATLAB writes
        width = 32 if max((len(f) for f in fields), default=0) < 32 else 64
        names = b"".join(f.encode("ascii").ljust(width, b"\0") for f in fields)
        body = (_header_parts(_mxSTRUCT, (1, len(items)), name)
                # the name length as a "small data element" (type and size in
                # 4 bytes, then the value), as MATLAB writes it and Octave needs
                + struct.pack("<HHi", _miINT32, 4, width)
                + _element(_miINT8, names)
                + b"".join(_matrix(it.get(f)) for it in items for f in fields))
    elif isinstance(value, (list, tuple)):
        if all(isinstance(v, (int, float)) or v is None for v in value):
            # a number list is a column vector, as MATLAB's tables and plots expect
            body = (_header_parts(_mxDOUBLE, (len(value), 1 if value else 0), name)
                    + _element(_miDOUBLE, _doubles(value)))
        else:
            return _matrix(Cell([[v] for v in value]), name)
    else:
        raise TypeError(f"cannot write a {type(value).__name__} to a .mat file")
    return _element(_miMATRIX, body)


def mat_bytes(variables: Mapping[str, Any], description: str = "") -> bytes:
    """A complete .mat file holding ``variables`` (name -> value).

    Values: a number, a string, a list of numbers (a column vector; None is
    NaN), a :class:`Matrix`, a dict (a struct, its keys valid MATLAB names),
    a :class:`StructArray`, a :class:`Cell` or a list of other values (a
    column cell array)."""
    text = description or (
        "MATLAB 5.0 MAT-file, written by LightSim, "
        + time.strftime("%a %b %d %H:%M:%S %Y", time.gmtime()))
    head = text.encode("ascii", errors="replace")[:116].ljust(116, b" ")
    out = [head, b"\0" * 8, struct.pack("<H", 0x0100), b"IM"]
    for name, value in variables.items():
        if name != identifier(name):
            raise ValueError(f"'{name}' is not a valid MATLAB variable name")
        out.append(_compressed(_matrix(value, name)))
    return b"".join(out)


def _compressed(element: bytes) -> bytes:
    data = zlib.compress(element, 6)
    # a compressed element is not padded: its byte count is the stream's
    return struct.pack("<II", _miCOMPRESSED, len(data)) + data
