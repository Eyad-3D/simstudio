"""Read a table, a 2-D map or a drive profile from a sheet of cells (STD-10).

A motor map from a supplier, a battery table from a test lab or a speed
trace from a logger arrives as a spreadsheet. This module finds the data in
a sheet (the header row, the axes, the units written in the headers),
converts it to the unit the parameter is stored in, and checks it: a text
cell, an empty cell, an axis that does not increase or a unit of the wrong
kind is refused with the row or cell it is in.

The result is what the app shows as a preview before anything changes; the
importer never edits a project itself.
"""
from __future__ import annotations

import re
from dataclasses import dataclass, field
from typing import Any, Optional

from ..library import library_by_id
from . import units as U
from .sheets import Cell, Sheet, cell_name, column_letter

#: Fewest points an imported profile may have (a table may have one: it is
#: then a constant).
MIN_POINTS = 2


@dataclass
class Quantity:
    """An axis or the values of a target: its name and catalog unit, and
    other words a file may use for it."""

    name: str
    unit: str
    words: tuple[str, ...] = ()


@dataclass
class Target:
    """What the import fills: a 1-D table, a 2-D map or a profile."""

    kind: str  # "table1d" | "table2d" | "profile"
    axes: list[Quantity]  # 1 (table1d, profile) or 2: [outer = columns, inner = rows]
    value: Quantity
    #: profiles may repeat an x (a step); tables may not
    allow_repeats: bool = False
    label: str = ""


_WORDS = {
    "Speed": ("speed", "n", "rpm", "drehzahl", "omega", "velocity", "v"),
    "Torque": ("torque", "trq", "tq", "m", "moment", "drehmoment"),
    "Voltage": ("voltage", "u", "v", "volt", "spannung"),
    "Current": ("current", "i", "amps", "strom"),
    "SOC": ("soc", "state of charge", "charge"),
    "Time": ("time", "t", "zeit", "time_s", "seconds", "sec"),
    "Distance": ("distance", "dist", "s", "x", "position", "weg"),
    "Target Speed": ("speed", "v", "velocity", "vehicle speed", "geschwindigkeit"),
    "Grade": ("grade", "slope", "gradient", "steigung", "incline"),
    "Gear": ("gear", "gang"),
    "Elevation": ("elevation", "altitude", "height", "z"),
    "Curvature": ("curvature", "kappa", "k"),
}


def _q(name: str, unit: str) -> Quantity:
    return Quantity(name, unit, _WORDS.get(name, (name.lower(),)))


def target_for(component_def_id: str, param_key: str, mode: str | None = None) -> Target:
    """The import target for a parameter of a library part. Raises KeyError
    for a part or parameter that does not exist, ValueError for one that is
    not a table or profile."""
    cdef = library_by_id()[component_def_id]
    p = next((q for q in cdef.parameters if q.key == param_key), None)
    if p is None:
        raise KeyError(param_key)
    if p.type == "string" and p.key == "profile":
        if component_def_id == "signal.road_profile":
            axis = _q("Time", "s") if mode == "time" else _q("Distance", "m")
            return Target("profile", [axis], _q("Grade", "%"), True, "Road Profile")
        return Target("profile", [_q("Time", "s")], _q("Target Speed", "km/h"), True,
                      "Driving Task profile")
    if p.type == "table1d" and p.axes and len(p.axes) == 1:
        return Target("table1d", [_q(p.axes[0].name, p.axes[0].unit)], _q(p.label, p.unit),
                      label=p.label)
    if p.type == "table2d" and p.axes and len(p.axes) == 2:
        return Target("table2d", [_q(a.name, a.unit) for a in p.axes], _q(p.label, p.unit),
                      label=p.label)
    raise ValueError(f"'{p.label}' is not a table, map or profile")


@dataclass
class Problem:
    text: str
    row: Optional[int] = None  # 1-based, as the spreadsheet shows it
    cell: Optional[str] = None

    def as_dict(self) -> dict:
        return {k: v for k, v in (("text", self.text), ("row", self.row), ("cell", self.cell))
                if v is not None}


@dataclass
class UnitChoice:
    """The unit a column, axis or the values were read in."""

    used: str  # the unit the numbers were read in
    target: str  # the parameter's unit
    written: Optional[str] = None  # as written in the file
    how: str = "header"  # "header" | "chosen" | "assumed" | "guessed"
    options: list[str] = field(default_factory=list)
    question: Optional[str] = None  # asked when the unit was guessed

    def as_dict(self) -> dict:
        return {"used": self.used, "target": self.target, "written": self.written,
                "how": self.how, "options": self.options, "question": self.question}


@dataclass
class ImportResult:
    target: Target
    sheet: str
    value: Any = None  # table1d / table2d dict, or the profile string
    points: int = 0
    range: str = ""
    columns: list[dict] = field(default_factory=list)
    x_column: Optional[int] = None
    y_column: Optional[int] = None
    transpose: bool = False
    units: dict[str, UnitChoice] = field(default_factory=dict)
    notes: list[str] = field(default_factory=list)
    errors: list[Problem] = field(default_factory=list)
    warnings: list[Problem] = field(default_factory=list)
    #: the imported points in the parameter's units, for the preview chart:
    #: [[x, y], ...] or, for a map, {"cols": [outer], "rows": [inner],
    #: "values": [[...]]} with values[row][col]
    preview: Any = None
    #: a CSV file's decimal mark as read ("comma" or "point"), and the
    #: question when its cells do not show it (sheets.Sheet)
    decimal: Optional[str] = None
    decimal_question: Optional[str] = None

    def as_dict(self) -> dict:
        ok = not self.errors
        return {
            "kind": self.target.kind, "sheet": self.sheet, "ok": ok,
            "value": self.value if ok else None, "points": self.points, "range": self.range,
            "columns": self.columns, "xColumn": self.x_column, "yColumn": self.y_column,
            "transpose": self.transpose,
            "units": {k: u.as_dict() for k, u in self.units.items()},
            "axes": [{"name": a.name, "unit": a.unit} for a in self.target.axes],
            "valueName": self.target.value.name, "valueUnit": self.target.value.unit,
            "notes": self.notes, "errors": [p.as_dict() for p in self.errors],
            "warnings": [p.as_dict() for p in self.warnings], "preview": self.preview,
            "decimal": self.decimal, "decimalQuestion": self.decimal_question,
        }


# ---- locating data -------------------------------------------------------------

_RANGE = re.compile(r"^\s*([A-Za-z]{1,3})(\d+)\s*(?::\s*([A-Za-z]{1,3})(\d+))?\s*$")


def _col_index(letters: str) -> int:
    n = 0
    for ch in letters.upper():
        n = n * 26 + ord(ch) - 64
    return n - 1


def parse_range(text: str) -> tuple[int, int, int, int]:
    """(row0, col0, row1, col1), 0-based and inclusive, of "B3:F20" (or "B3",
    to the end of the sheet). Raises ValueError for anything else."""
    m = _RANGE.match(text)
    if not m:
        raise ValueError(f"'{text}' is not a cell range such as A1:D20")
    r0, c0 = int(m.group(2)) - 1, _col_index(m.group(1))
    if m.group(3):
        r1, c1 = int(m.group(4)) - 1, _col_index(m.group(3))
    else:
        r1, c1 = 10**9, 10**9
    if r1 < r0 or c1 < c0:
        raise ValueError(f"'{text}' ends before it starts")
    return r0, c0, r1, c1


def _num(c: Cell) -> bool:
    return isinstance(c, float)


def _text(c: Cell) -> bool:
    return isinstance(c, str) and c.strip() != ""


class _Grid:
    """A sheet seen through an optional range: cells by absolute position."""

    def __init__(self, sheet: Sheet, rng: Optional[tuple[int, int, int, int]]):
        self.rows = sheet.rows
        self.r0, self.c0, self.r1, self.c1 = rng or (0, 0, 10**9, 10**9)
        self.r1 = min(self.r1, len(self.rows) - 1)

    def get(self, r: int, c: int) -> Cell:
        if not (self.r0 <= r <= self.r1 and self.c0 <= c <= self.c1):
            return None
        row = self.rows[r] if 0 <= r < len(self.rows) else []
        return row[c] if 0 <= c < len(row) else None

    def row_cols(self, r: int) -> range:
        row = self.rows[r] if 0 <= r < len(self.rows) else []
        return range(self.c0, min(self.c1, len(row) - 1) + 1)

    def numbers_in(self, r: int) -> list[int]:
        return [c for c in self.row_cols(r) if _num(self.get(r, c))]


def _first_data_row(g: _Grid, min_numbers: int) -> Optional[int]:
    """The first row with at least ``min_numbers`` numbers."""
    for r in range(g.r0, g.r1 + 1):
        if len(g.numbers_in(r)) >= min_numbers:
            return r
    return None


def _match_score(name: str, q: Quantity) -> int:
    """How well a column name fits a quantity: 3 exact, 2 a word of it, 0 none."""
    n = name.strip().lower()
    if not n:
        return 0
    if n == q.name.lower() or n in q.words:
        return 3
    tokens = set(re.split(r"[\s_\-./]+", n))
    if any(w in tokens or (len(w) > 2 and w in n) for w in q.words if w):
        return 2
    return 0


# ---- unit handling -------------------------------------------------------------

def _unit_choice(written_unit: Optional[str], written: Optional[str], q: Quantity,
                 override: Optional[str], values: list[float], what: str,
                 problems: list[Problem]) -> Optional[UnitChoice]:
    """The unit to read a column in: the one the user chose, else the one
    written in the file, else a guess from the values (speed and grade),
    else the parameter's own."""
    options = U.alternatives(q.unit) if U.group(q.unit) not in (None, "none") else [q.unit]
    if q.name == "Grade":
        options = ["%", "fraction"]
    if override:
        canon = U.canonical(override) or (override if override == "fraction" else None)
        if canon is None or (canon not in options and not U.same_group(canon, q.unit)):
            problems.append(Problem(f"{what}: '{override}' cannot be read as {q.name} "
                                    f"({', '.join(options)})."))
            return None
        return UnitChoice(canon, q.unit, written, "chosen", options)
    if written_unit:
        if written_unit == q.unit or U.same_group(written_unit, q.unit):
            return UnitChoice(written_unit, q.unit, written, "header", options)
        if q.name == "Grade" and written_unit == "-":
            return UnitChoice("fraction", q.unit, written, "header", options)
        if q.unit == "-":
            return UnitChoice(q.unit, q.unit, written, "header", options)
        problems.append(Problem(
            f"{what} is in {written_unit}, which is not a unit of {q.name.lower()} "
            f"({', '.join(options)}). Choose the right column or its unit."))
        return None
    if written:  # in brackets, but not a unit LightSim knows
        if q.unit == "-":
            return UnitChoice(q.unit, q.unit, written, "header", options)
        problems.append(Problem(f"{what}: the unit '{written}' is not one LightSim knows. "
                                f"Choose its unit ({', '.join(options)})."))
        return None
    finite = [abs(v) for v in values]
    peak = max(finite, default=0.0)
    if U.group(q.unit) == "speed" and q.unit == "km/h" and peak > 0:
        guess = "m/s" if peak <= 45 else "km/h"
        return UnitChoice(guess, q.unit, None, "guessed", options,
                          f"No unit found for {q.name.lower()}. The values reach {peak:g}, "
                          f"so they were read in {guess}. Is that right?")
    if q.name == "Grade" and peak > 0:
        guess = "fraction" if peak <= 0.3 else "%"
        shown = "a fraction (0.05 = 5 %)" if guess == "fraction" else "%"
        return UnitChoice(guess, q.unit, None, "guessed", options,
                          f"No unit found for the grade. The values reach {peak:g}, so they "
                          f"were read as {shown}. Is that right?")
    return UnitChoice(q.unit, q.unit, None, "assumed", options)


def _converter(choice: UnitChoice):
    if choice.used == "fraction":
        return lambda v: v * 100.0 if choice.target == "%" else v
    if choice.used == choice.target or U.canonical(choice.used) is None \
            or U.canonical(choice.target) is None:
        return lambda v: v
    s, o = U.factor(choice.used, choice.target)
    return lambda v: U.tidy(v * s + o)


# ---- 1-D tables and profiles ---------------------------------------------------

def _header_rows(g: _Grid, d: int) -> tuple[dict[int, str], dict[int, str], Optional[int]]:
    """Column names and units from the rows just above the data: a header row,
    optionally followed by a row of units (as loggers write them)."""
    names: dict[int, str] = {}
    unit_row: dict[int, str] = {}
    above = [r for r in range(max(g.r0, d - 3), d)]
    texts = {r: {c: str(g.get(r, c)) for c in g.row_cols(r) if _text(g.get(r, c))}
             for r in above}
    rows_with_text = [r for r in above if texts[r]]
    if not rows_with_text:
        return names, unit_row, None
    last = rows_with_text[-1]
    is_units = all(U.canonical(t.strip("[]() ")) for t in texts[last].values())
    if is_units and len(rows_with_text) >= 2:
        unit_row = {c: t.strip("[]() ") for c, t in texts[last].items()}
        return texts[rows_with_text[-2]], unit_row, rows_with_text[-2]
    return texts[last], unit_row, last


def _read_1d(sheet: Sheet, target: Target, opts: dict) -> ImportResult:
    res = ImportResult(target, sheet.name)
    g = _Grid(sheet, opts.get("_range"))
    d = _first_data_row(g, 2)
    if d is None:
        res.errors.append(Problem(
            "No data found: the sheet needs at least two columns of numbers "
            f"({target.axes[0].name} and {target.value.name})."))
        return res
    names, unit_row, header_row = _header_rows(g, d)
    # the data block: from d down to the first row with no numbers
    end = d
    while end + 1 <= g.r1 and g.numbers_in(end + 1):
        end += 1
    if end + 1 <= g.r1 and any(g.numbers_in(r) for r in range(end + 2, g.r1 + 1)):
        res.notes.append(f"Read rows {d + 1} to {end + 1}; the rows after the empty row "
                         f"{end + 2} were left out.")
    width_cols = sorted({c for r in range(d, end + 1) for c in g.row_cols(r)
                         if _num(g.get(r, c))})
    cols: list[dict] = []
    for c in width_cols:
        header = names.get(c, "")
        name, unit, written = U.split_header(header) if header else ("", None, None)
        if c in unit_row:
            written = unit_row[c]
            unit = U.canonical(written)
        cols.append({"index": c, "letter": column_letter(c), "header": header,
                     "name": name or column_letter(c), "unit": unit, "written": written})
    res.columns = cols
    by_index = {c["index"]: c for c in cols}

    def pick(q: Quantity, avoid: set[int]) -> Optional[int]:
        best, score = None, 0
        for c in cols:
            if c["index"] in avoid:
                continue
            s = _match_score(c["name"], q) * 2
            if c["unit"] and U.same_group(c["unit"], q.unit):
                s += 3
            if s > score:
                best, score = c["index"], s
        return best

    x_q, y_q = target.axes[0], target.value
    x = opts.get("xColumn")
    y = opts.get("yColumn")
    for key, val in (("xColumn", x), ("yColumn", y)):
        if val is not None and val not in by_index:
            res.errors.append(Problem(f"Column {column_letter(val)} holds no numbers."))
            return res
    if x is None:
        x = pick(x_q, {y} if y is not None else set())
    if x is None:
        x = next(c["index"] for c in cols if c["index"] != y)
    if y is None:
        y = pick(y_q, {x})
    if y is None:
        y = next((c["index"] for c in cols if c["index"] != x), None)
    if y is None:
        res.errors.append(Problem(
            f"Only one column of numbers was found; {target.label or 'the table'} needs "
            f"{x_q.name} and {y_q.name}."))
        return res
    res.x_column, res.y_column = x, y
    res.range = f"{cell_name(d, min(x, y))}:{cell_name(end, max(x, y))}"
    if header_row is not None:
        res.notes.append(f"Header in row {header_row + 1}"
                         + (f", units in row {header_row + 2}" if unit_row else "")
                         + f"; data from row {d + 1}.")
    else:
        res.notes.append(f"No header row; data from row {d + 1}.")

    xs: list[float] = []
    ys: list[float] = []
    rows: list[int] = []
    for r in range(d, end + 1):
        cx, cy = g.get(r, x), g.get(r, y)
        if cx is None and cy is None:
            continue
        bad = False
        for c, v, q in ((x, cx, x_q), (y, cy, y_q)):
            if v is None:
                res.errors.append(Problem(f"Row {r + 1}: cell {cell_name(r, c)} "
                                          f"({q.name}) is empty.", r + 1, cell_name(r, c)))
                bad = True
            elif not _num(v):
                res.errors.append(Problem(f"Row {r + 1}: '{v}' in cell {cell_name(r, c)} "
                                          f"({q.name}) is not a number.", r + 1, cell_name(r, c)))
                bad = True
        if not bad:
            xs.append(float(cx))  # type: ignore[arg-type]
            ys.append(float(cy))  # type: ignore[arg-type]
            rows.append(r + 1)
        if len(res.errors) >= 20:
            break
    if res.errors:
        return res

    xc, yc = by_index[x], by_index[y]
    ux = _unit_choice(xc["unit"], xc["written"], x_q, (opts.get("units") or {}).get("x"),
                      xs, f"Column {xc['letter']} ({x_q.name})", res.errors)
    uy = _unit_choice(yc["unit"], yc["written"], y_q, (opts.get("units") or {}).get("y"),
                      ys, f"Column {yc['letter']} ({y_q.name})", res.errors)
    if ux is None or uy is None:
        return res
    res.units = {"x": ux, "y": uy}
    fx, fy = _converter(ux), _converter(uy)
    xs = [fx(v) for v in xs]
    ys = [fy(v) for v in ys]

    least = MIN_POINTS if target.kind == "profile" else 1
    if len(xs) < least:
        res.errors.append(Problem(f"Only {len(xs)} row of data was found; at least "
                                  f"{least} are needed."))
        return res
    for i in range(1, len(xs)):
        if xs[i] < xs[i - 1] or (xs[i] == xs[i - 1] and not target.allow_repeats):
            what = "repeats" if xs[i] == xs[i - 1] else "is below"
            res.errors.append(Problem(
                f"Row {rows[i]}: {x_q.name} {xs[i]:g} {x_q.unit} {what} the row before "
                f"({xs[i - 1]:g} {x_q.unit}); {x_q.name} must increase down the column.",
                rows[i], cell_name(rows[i] - 1, x)))
            if len(res.errors) >= 20:
                break
        elif xs[i] == xs[i - 1]:
            res.warnings.append(Problem(
                f"Row {rows[i]}: {x_q.name} {xs[i]:g} repeats; the profile steps there.",
                rows[i]))
    if res.errors:
        return res
    res.points = len(xs)
    res.preview = [[a, b] for a, b in zip(xs, ys)]
    if target.kind == "profile":
        res.value = "; ".join(f"{a:.12g}:{b:.12g}" for a, b in zip(xs, ys))
    else:
        res.value = {f"{a:.12g}": b for a, b in zip(xs, ys)}
    return res


# ---- 2-D maps --------------------------------------------------------------------

_CORNER_SPLIT = re.compile(r"\s*[\\]\s*|\s+/\s+|\s*\|\s*")


def _hint(text: str) -> tuple[str, Optional[str], Optional[str]]:
    return U.split_header(text)


def _axis_for(name: str, unit: Optional[str], axes: list[Quantity]) -> Optional[int]:
    """Which target axis a file's label names: by unit kind, else by name."""
    if unit:
        same = [i for i, a in enumerate(axes) if U.same_group(unit, a.unit)]
        if len(same) == 1:
            return same[0]
    scores = [_match_score(name, a) for a in axes]
    if max(scores) > 0 and scores.count(max(scores)) == 1:
        return scores.index(max(scores))
    return None


def _read_2d(sheet: Sheet, target: Target, opts: dict) -> ImportResult:
    res = ImportResult(target, sheet.name)
    g = _Grid(sheet, opts.get("_range"))
    outer, inner = target.axes  # outer = the editor's columns, inner = its rows
    # the column-axis row: the first row with 2+ numbers, whose next row starts
    # with a number one column to the left of the first of them
    hr = hc = None
    for r in range(g.r0, g.r1 + 1):
        nums = g.numbers_in(r)
        if len(nums) < 2:
            continue
        c = nums[0] - 1
        if c >= g.c0 and _num(g.get(r + 1, c)) and not _num(g.get(r, c)):
            hr, hc = r, c
            break
    if hr is None:
        # perhaps the first column holds the row values and the corner is a number
        res.errors.append(Problem(
            "No map found. Lay it out with the column values (for example speeds) along "
            "one row, the row values (for example torques) down the column to their left, "
            "the map's values in between, and the cell where they meet empty or labelled."))
        return res
    cols = [c for c in g.row_cols(hr) if c > hc and g.get(hr, c) is not None]
    # the column axis ends at its first gap
    col_axis: list[int] = []
    for c in range(hc + 1, (cols[-1] if cols else hc) + 1):
        if g.get(hr, c) is None:
            break
        col_axis.append(c)
    row_axis: list[int] = []
    r = hr + 1
    while r <= g.r1 and g.get(r, hc) is not None:
        row_axis.append(r)
        r += 1
    res.range = f"{cell_name(hr, hc)}:{cell_name(row_axis[-1], col_axis[-1])}"
    res.notes.append(f"Map found in {res.range}: {len(col_axis)} columns × {len(row_axis)} rows.")

    # labels: the corner cell ("Torque [Nm] \ Speed [rpm]" = rows \ columns),
    # text above the column axis and text left of the row axis
    col_hint: Optional[tuple] = None
    row_hint: Optional[tuple] = None
    val_hint: Optional[tuple] = None
    corner = g.get(hr, hc)
    if _text(corner):
        parts = _CORNER_SPLIT.split(str(corner))
        if len(parts) == 2:
            row_hint, col_hint = _hint(parts[0]), _hint(parts[1])
        else:
            h = _hint(str(corner))
            val_hint = h
    above = [str(g.get(rr, c)) for rr in range(max(g.r0, hr - 3), hr)
             for c in range(hc, col_axis[-1] + 1) if _text(g.get(rr, c))]
    left = [str(g.get(rr, c)) for rr in row_axis for c in range(max(g.c0, hc - 2), hc)
            if _text(g.get(rr, c))]
    # labels with a unit first: a title row above the map says less
    hints = sorted((_hint(t) for t in above), key=lambda h: h[1] is None and h[2] is None)
    for h in hints:
        idx = _axis_for(h[0], h[1], target.axes)
        if h[1] and (U.same_group(h[1], target.value.unit) or _match_score(h[0], target.value)) \
                and idx is None and val_hint is None:
            val_hint = h
        elif col_hint is None and (h[1] or idx is not None):
            col_hint = h
        elif val_hint is None and h[1]:
            val_hint = h
    for t in left:
        if row_hint is None:
            row_hint = _hint(t)

    # which way round: the file's columns are the editor's columns (outer)
    # unless its labels say they are the rows' quantity
    transpose = opts.get("transpose")
    if transpose is None:
        transpose = False
        # each label votes "swap" (True) or "as it is" (False); a label
        # LightSim does not recognise abstains (None)
        votes: list[Optional[bool]] = []
        for hint, swapped_axis in ((col_hint, 1), (row_hint, 0)):
            if hint:
                idx = _axis_for(hint[0], hint[1], target.axes)
                votes.append(None if idx is None else idx == swapped_axis)
        sure = [v for v in votes if v is not None]
        as_read = (f"columns read as {outer.name.lower()}, rows as {inner.name.lower()}. "
                   "Swap them if that is wrong.")
        if sure and all(sure):
            transpose = True
            res.notes.append(f"The file's columns hold {inner.name.lower()}, so rows and "
                             "columns were swapped to match LightSim's map.")
        elif any(sure):
            res.notes.append("The axis labels disagree (they name the same quantity): "
                             + as_read)
        elif not votes:
            res.notes.append("No axis labels found: " + as_read)
        elif not sure:
            res.notes.append("The axis labels do not say which axis is which: " + as_read)
    res.transpose = bool(transpose)
    file_cols_q, file_rows_q = (inner, outer) if transpose else (outer, inner)

    # check every cell
    col_vals: list[float] = []
    for c in col_axis:
        v = g.get(hr, c)
        if not _num(v):
            res.errors.append(Problem(f"Cell {cell_name(hr, c)}: '{v}' is not a number "
                                      f"({file_cols_q.name}, the column values).",
                                      hr + 1, cell_name(hr, c)))
        else:
            col_vals.append(float(v))  # type: ignore[arg-type]
    row_vals: list[float] = []
    for rr in row_axis:
        v = g.get(rr, hc)
        if not _num(v):
            res.errors.append(Problem(f"Row {rr + 1}: '{v}' in cell {cell_name(rr, hc)} is not "
                                      f"a number ({file_rows_q.name}, the row values).",
                                      rr + 1, cell_name(rr, hc)))
        else:
            row_vals.append(float(v))  # type: ignore[arg-type]
    body: list[list[float]] = []
    for rr in row_axis:
        line: list[float] = []
        for c in col_axis:
            v = g.get(rr, c)
            if v is None:
                res.errors.append(Problem(f"Row {rr + 1}: cell {cell_name(rr, c)} is empty; "
                                          "every point of the map needs a value.",
                                          rr + 1, cell_name(rr, c)))
            elif not _num(v):
                res.errors.append(Problem(f"Row {rr + 1}: '{v}' in cell {cell_name(rr, c)} "
                                          "is not a number.", rr + 1, cell_name(rr, c)))
            else:
                line.append(float(v))  # type: ignore[arg-type]
            if len(res.errors) >= 20:
                return res
        body.append(line)
    if res.errors:
        return res

    def increasing(vals: list[float], q: Quantity, where: str, at) -> None:
        for i in range(1, len(vals)):
            if vals[i] <= vals[i - 1]:
                what = "repeats" if vals[i] == vals[i - 1] else "is below"
                res.errors.append(Problem(
                    f"{where(i)}: {q.name} {vals[i]:g} {what} the one before ({vals[i - 1]:g}); "
                    f"{q.name} must increase.", *at(i)))

    increasing(col_vals, file_cols_q, lambda i: f"Cell {cell_name(hr, col_axis[i])}",
               lambda i: (hr + 1, cell_name(hr, col_axis[i])))
    increasing(row_vals, file_rows_q, lambda i: f"Row {row_axis[i] + 1}",
               lambda i: (row_axis[i] + 1, cell_name(row_axis[i], hc)))
    if len(col_vals) < MIN_POINTS and len(row_vals) < MIN_POINTS:
        res.errors.append(Problem("The map has a single point; it needs at least two rows "
                                  "or two columns."))
    if res.errors:
        return res

    # units
    ov = opts.get("units") or {}
    uc = _unit_choice(col_hint[1] if col_hint else None, col_hint[2] if col_hint else None,
                      file_cols_q, ov.get("cols"), col_vals, "The column values", res.errors)
    ur = _unit_choice(row_hint[1] if row_hint else None, row_hint[2] if row_hint else None,
                      file_rows_q, ov.get("rows"), row_vals, "The row values", res.errors)
    flat = [v for line in body for v in line]
    uv = _unit_choice(val_hint[1] if val_hint else None, val_hint[2] if val_hint else None,
                      target.value, ov.get("value"), flat, "The map's values", res.errors)
    if uc is None or ur is None or uv is None:
        return res
    res.units = {"cols": uc, "rows": ur, "value": uv}
    fc, fr, fv = _converter(uc), _converter(ur), _converter(uv)
    col_vals = [fc(v) for v in col_vals]
    row_vals = [fr(v) for v in row_vals]
    body = [[fv(v) for v in line] for line in body]

    # to LightSim's {outer: {inner: value}}
    table: dict[str, dict[str, float]] = {}
    if not transpose:  # file columns = outer
        for j, ov_ in enumerate(col_vals):
            table[f"{ov_:.12g}"] = {f"{iv:.12g}": body[i][j] for i, iv in enumerate(row_vals)}
        outer_vals, inner_vals = col_vals, row_vals
        grid = body
    else:  # file rows = outer
        for i, ov_ in enumerate(row_vals):
            table[f"{ov_:.12g}"] = {f"{iv:.12g}": body[i][j] for j, iv in enumerate(col_vals)}
        outer_vals, inner_vals = row_vals, col_vals
        grid = [[body[i][j] for i in range(len(row_vals))] for j in range(len(col_vals))]
    res.value = table
    res.points = len(col_vals) * len(row_vals)
    res.preview = {"cols": outer_vals, "rows": inner_vals, "values": grid}
    return res


def import_table(sheet: Sheet, target: Target, opts: Optional[dict] = None) -> ImportResult:
    """Read ``target`` from ``sheet``. ``opts``: ``range`` ("B3:F20"),
    ``xColumn`` / ``yColumn`` (0-based column indexes, 1-D), ``transpose``
    (2-D), ``units`` ({"x", "y"} or {"cols", "rows", "value"}: the unit to
    read each in, overriding what the file says)."""
    opts = dict(opts or {})
    if opts.get("range"):
        try:
            opts["_range"] = parse_range(str(opts["range"]))
        except ValueError as e:
            res = ImportResult(target, sheet.name)
            res.errors.append(Problem(str(e)))
            return res
    if not sheet.rows:
        res = ImportResult(target, sheet.name)
        res.errors.append(Problem(f"The sheet '{sheet.name}' is empty."))
        return res
    res = _read_2d(sheet, target, opts) if target.kind == "table2d" else \
        _read_1d(sheet, target, opts)
    res.notes = sheet.notes + res.notes
    res.decimal, res.decimal_question = sheet.decimal, sheet.question
    return res
