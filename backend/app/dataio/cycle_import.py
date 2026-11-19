"""Read a drive cycle of the user's own from a CSV or .xlsx sheet (CON-11).

A speed against time, with an optional road-grade column, or a speed and/or
a grade against distance (a logged lap, a route). The sheet is read the way
the table import reads a profile (tables.py: the header row, units in the
headers or a units row, empty and text cells refused with their row), but
into up to three columns at once. The result is a cycle the app keeps in
the project (``Project.cycles``); the importer never edits a project itself.
"""
from __future__ import annotations

from dataclasses import dataclass, field
from typing import Optional

from .. import cycles
from . import units as U
from .sheets import Sheet, cell_name, column_letter
from .tables import (
    Problem,
    Quantity,
    UnitChoice,
    _converter,
    _first_data_row,
    _Grid,
    _header_rows,
    _match_score,
    _num,
    _q,
    _unit_choice,
    parse_range,
)

#: what each role is called, and its unit
TIME, DISTANCE = _q("Time", "s"), _q("Distance", "m")
SPEED, GRADE = _q("Target Speed", "km/h"), _q("Grade", "%")


@dataclass
class CycleImport:
    sheet: str
    axis: str = "time"
    columns: list[dict] = field(default_factory=list)
    x_column: Optional[int] = None
    speed_column: Optional[int] = None
    grade_column: Optional[int] = None
    range: str = ""
    units: dict[str, UnitChoice] = field(default_factory=dict)
    x: list[float] = field(default_factory=list)
    speed: Optional[list[float]] = None
    grade: Optional[list[float]] = None
    notes: list[str] = field(default_factory=list)
    errors: list[Problem] = field(default_factory=list)
    warnings: list[Problem] = field(default_factory=list)
    decimal: Optional[str] = None
    decimal_question: Optional[str] = None

    def as_dict(self) -> dict:
        ok = not self.errors
        cycle = {"axis": self.axis, "x": self.x, "speed": self.speed, "grade": self.grade}
        shown = SPEED if self.speed is not None or self.grade is None else GRADE
        values = self.speed if self.speed is not None else self.grade
        return {
            "kind": "cycle", "sheet": self.sheet, "ok": ok, "axis": self.axis,
            "columns": self.columns, "xColumn": self.x_column,
            "speedColumn": self.speed_column, "gradeColumn": self.grade_column,
            "range": self.range, "points": len(self.x) if ok else 0,
            "units": {k: u.as_dict() for k, u in self.units.items()},
            "notes": self.notes, "errors": [p.as_dict() for p in self.errors],
            "warnings": [p.as_dict() for p in self.warnings],
            "cycle": cycle if ok else None,
            "info": cycles.own_info({"id": "", "name": "", **cycle}) if ok else None,
            # for the preview chart, in the table import's terms
            "axes": [{"name": (TIME if self.axis == "time" else DISTANCE).name,
                      "unit": "s" if self.axis == "time" else "m"}],
            "valueName": "Speed" if shown is SPEED else "Grade", "valueUnit": shown.unit,
            "preview": [[a, b] for a, b in zip(self.x, values)] if ok and values else None,
            "decimal": self.decimal, "decimalQuestion": self.decimal_question,
        }


def _best(cols: list[dict], q: Quantity, avoid: set[int]) -> tuple[Optional[int], int]:
    """The column that best fits ``q`` by its name and unit, and its score."""
    best, score = None, 0
    for c in cols:
        if c["index"] in avoid:
            continue
        s = _match_score(c["name"], q) * 2
        if c["unit"] and U.same_group(c["unit"], q.unit):
            s += 3
        if s > score:
            best, score = c["index"], s
    return best, score


def import_cycle(sheet: Sheet, opts: Optional[dict] = None) -> CycleImport:
    """Read a cycle from ``sheet``. ``opts``: ``range`` ("B3:F20"), ``axis``
    ("time" or "distance"; found from the headers when not given),
    ``xColumn``, ``speedColumn`` and ``gradeColumn`` (0-based column
    indexes; -1 for none; found from the headers when not given) and
    ``units`` ({"x", "speed", "grade"}: the unit to read each in)."""
    opts = dict(opts or {})
    res = CycleImport(sheet.name)
    res.decimal, res.decimal_question = sheet.decimal, sheet.question
    rng = None
    if opts.get("range"):
        try:
            rng = parse_range(str(opts["range"]))
        except ValueError as e:
            res.errors.append(Problem(str(e)))
            return res
    if not sheet.rows:
        res.errors.append(Problem(f"The sheet '{sheet.name}' is empty."))
        return res
    _read(sheet, rng, opts, res)
    res.notes = sheet.notes + res.notes
    return res


def _read(sheet: Sheet, rng, opts: dict, res: CycleImport) -> None:
    g = _Grid(sheet, rng)
    d = _first_data_row(g, 2)
    if d is None:
        res.errors.append(Problem(
            "No data found: a cycle needs at least two columns of numbers (time or distance, "
            "and the speed or the grade)."))
        return
    names, unit_row, header_row = _header_rows(g, d)
    end = d
    while end + 1 <= g.r1 and g.numbers_in(end + 1):
        end += 1
    if end + 1 <= g.r1 and any(g.numbers_in(r) for r in range(end + 2, g.r1 + 1)):
        res.notes.append(f"Read rows {d + 1} to {end + 1}; the rows after the empty row "
                         f"{end + 2} were left out.")
    cols: list[dict] = []
    for c in sorted({c for r in range(d, end + 1) for c in g.row_cols(r) if _num(g.get(r, c))}):
        header = names.get(c, "")
        name, unit, written = U.split_header(header) if header else ("", None, None)
        if c in unit_row:
            written = unit_row[c]
            unit = U.canonical(written)
        cols.append({"index": c, "letter": column_letter(c), "header": header,
                     "name": name or column_letter(c), "unit": unit, "written": written})
    res.columns = cols
    by_index = {c["index"]: c for c in cols}
    for key in ("xColumn", "speedColumn", "gradeColumn"):
        val = opts.get(key)
        if val is not None and val != -1 and val not in by_index:
            res.errors.append(Problem(f"Column {column_letter(val)} holds no numbers."))
            return

    # time or distance: as asked, else the axis a column's header names best
    axis = opts.get("axis")
    if axis not in ("time", "distance"):
        t_col, t_score = _best(cols, TIME, set())
        d_col, d_score = _best(cols, DISTANCE, set())
        axis = "distance" if d_score > t_score else "time"
    res.axis = axis
    x_q = TIME if axis == "time" else DISTANCE
    x = opts.get("xColumn")
    if x is None or x == -1:
        x = _best(cols, x_q, set())[0]
        if x is None:
            x = cols[0]["index"]
    speed = opts.get("speedColumn")
    grade = opts.get("gradeColumn")
    taken = {x} | {c for c in (speed, grade) if c is not None and c != -1}
    if grade is None:  # only a column whose name says it is a grade (not any %)
        grade = next((c["index"] for c in cols
                      if c["index"] not in taken and _match_score(c["name"], GRADE)), -1)
        taken.add(grade)
    if speed is None:
        found, score = _best(cols, SPEED, taken)
        if found is None and axis == "time":
            found = next((c["index"] for c in cols if c["index"] not in taken), None)
        speed = found if found is not None else -1
    if speed == -1 and axis == "time":
        res.errors.append(Problem("A cycle against time needs a speed column; choose it."))
    if speed == -1 and grade == -1:
        res.errors.append(Problem("Choose the speed column, the grade column, or both."))
    if len({c for c in (x, speed, grade) if c != -1}) < len([c for c in (x, speed, grade)
                                                             if c != -1]):
        res.errors.append(Problem("Choose a different column for each of "
                                  f"{x_q.name.lower()}, speed and grade."))
    res.x_column, res.speed_column, res.grade_column = x, speed, grade
    if res.errors:
        return
    roles = [("x", x, x_q)] + [(k, c, q) for k, c, q in (("speed", speed, SPEED),
                                                          ("grade", grade, GRADE)) if c != -1]
    used = [c for _, c, _ in roles]
    res.range = f"{cell_name(d, min(used))}:{cell_name(end, max(used))}"
    if header_row is not None:
        res.notes.append(f"Header in row {header_row + 1}"
                         + (f", units in row {header_row + 2}" if unit_row else "")
                         + f"; data from row {d + 1}.")
    else:
        res.notes.append(f"No header row; data from row {d + 1}.")

    values: dict[str, list[float]] = {k: [] for k, _, _ in roles}
    rows: list[int] = []
    for r in range(d, end + 1):
        cells = [(k, c, q, g.get(r, c)) for k, c, q in roles]
        if all(v is None for *_, v in cells):
            continue
        bad = False
        for _, c, q, v in cells:
            if v is None:
                res.errors.append(Problem(f"Row {r + 1}: cell {cell_name(r, c)} ({q.name}) is "
                                          f"empty.", r + 1, cell_name(r, c)))
                bad = True
            elif not _num(v):
                res.errors.append(Problem(f"Row {r + 1}: '{v}' in cell {cell_name(r, c)} "
                                          f"({q.name}) is not a number.", r + 1,
                                          cell_name(r, c)))
                bad = True
        if not bad:
            for k, _, _, v in cells:
                values[k].append(float(v))  # type: ignore[arg-type]
            rows.append(r + 1)
        if len(res.errors) >= 20:
            return
    if res.errors:
        return
    if len(rows) < 2:
        res.errors.append(Problem(f"Only {len(rows)} row of data was found; a cycle needs at "
                                  f"least 2."))
        return
    if len(rows) > cycles.MAX_POINTS:
        res.errors.append(Problem(f"The cycle has {len(rows):,} points; LightSim keeps at most "
                                  f"{cycles.MAX_POINTS:,} in a project. Thin the file first "
                                  f"(for example one point every 0.1 s or every metre)."))
        return

    asked = opts.get("units") or {}
    for k, c, q in roles:
        col = by_index[c]
        choice = _unit_choice(col["unit"], col["written"], q, asked.get(k), values[k],
                              f"Column {col['letter']} ({q.name})", res.errors)
        if choice is None:
            return
        res.units[k] = choice
        conv = _converter(choice)
        values[k] = [conv(v) for v in values[k]]

    xs = values["x"]
    for i in range(1, len(xs)):
        if xs[i] < xs[i - 1]:
            res.errors.append(Problem(
                f"Row {rows[i]}: {x_q.name} {xs[i]:g} {x_q.unit} is below the row before "
                f"({xs[i - 1]:g} {x_q.unit}); {x_q.name.lower()} must increase down the "
                f"column.", rows[i], cell_name(rows[i] - 1, x)))
            if len(res.errors) >= 20:
                return
    if not res.errors and xs[-1] <= xs[0]:
        res.errors.append(Problem(f"{x_q.name} does not increase: every row is at "
                                  f"{xs[0]:g} {x_q.unit}."))
    for i, v in enumerate(values.get("speed", [])):
        if v < 0:
            res.errors.append(Problem(f"Row {rows[i]}: the speed {v:g} km/h is below zero.",
                                      rows[i], cell_name(rows[i] - 1, speed)))
            if len(res.errors) >= 20:
                return
    if res.errors:
        return
    repeats = [rows[i] for i in range(1, len(xs)) if xs[i] == xs[i - 1]]
    if repeats:
        res.warnings.append(Problem(
            f"Row {repeats[0]}{' and others' if len(repeats) > 1 else ''}: {x_q.name.lower()} "
            f"repeats; the cycle steps there.", repeats[0]))
    if axis == "distance" and "speed" in values:
        stops = [rows[i] for i, v in enumerate(values["speed"][:-1]) if v <= 0]
        if stops:
            res.warnings.append(Problem(
                f"Row {stops[0]}: 0 km/h before the end. Against distance the car stops there "
                f"and does not drive on; give that point a small speed (for example 5 km/h).",
                stops[0]))
    res.x = xs
    res.speed = values.get("speed")
    res.grade = values.get("grade")

