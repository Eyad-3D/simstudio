"""Send all of a model's parameters to one spreadsheet and back (STD-36).

Export: one .xlsx workbook. Its *Parameters* sheet has a row per parameter
of every part (part, part id, parameter, key, value, unit, default, whether
it was changed, and free Source and Notes columns for the team); each table,
map and drive profile has a sheet of its own, laid out so the table import
(:mod:`app.dataio.tables`) reads it back. A CSV export holds the Parameters
sheet only, with tables as JSON text in their Value cell.

Import: the rows are matched to the model by part id and parameter key; a
part or key the model does not have, a unit other than the parameter's, or
a value that is not one the parameter can hold is refused with its row
number. Nothing changes in the model here: the result lists the changes for
the app to preview and apply as one undo step.
"""
from __future__ import annotations

import json
import time
from dataclasses import dataclass, field
from typing import Any, Optional

from ..library import library_by_id
from ..schemas import ComponentDef, ParameterDef, Project
from ..version import VERSION
from . import units as U
from .sheets import OutSheet, Sheet, csv_bytes, read_file, sheet_title, xlsx_bytes
from .tables import Target, _q, import_table

PARAMS_SHEET = "Parameters"
HEADER = ["Part", "Part ID", "Part type", "Parameter", "Key", "Value", "Unit", "Default",
          "Changed", "Source", "Notes"]
_REQUIRED = ("Part ID", "Key", "Value")
_SEE_SHEET = "see sheet "
_TRUE = {"true", "yes", "1", "on", "wahr", "ja"}
_FALSE = {"false", "no", "0", "off", "falsch", "nein"}


def _elements(project: Project):
    for s in project.systems:
        yield from s.elements


def _value(el, p: ParameterDef) -> Any:
    return el.parameterOverrides.get(p.key, p.default)


def _is_profile(p: ParameterDef) -> bool:
    return p.type == "string" and p.key == "profile"


def _is_table(p: ParameterDef) -> bool:
    return p.type in ("table1d", "table2d") or _is_profile(p)


def _same(a: Any, b: Any) -> bool:
    if isinstance(a, (int, float)) and isinstance(b, (int, float)) \
            and not isinstance(a, bool) and not isinstance(b, bool):
        return float(a) == float(b)
    return json.dumps(a, sort_keys=True) == json.dumps(b, sort_keys=True)


def _norm_table(p: ParameterDef, v: Any) -> Any:
    """A table or profile in a form where equal data compares equal
    ("1500" and "1500.0" keys, "0:0;30:50" and "0:0; 30:50")."""
    def keyed(d: Any) -> Any:
        if not isinstance(d, dict):
            return d
        try:
            return sorted((float(k), keyed(x)) for k, x in d.items())
        except (TypeError, ValueError):
            return d
    if _is_profile(p) and isinstance(v, str):
        pts = []
        for chunk in v.replace("\n", ";").split(";"):
            if chunk.strip():
                a, _, b = chunk.partition(":" if ":" in chunk else ",")
                try:
                    pts.append((float(a), float(b)))
                except ValueError:
                    pts.append((a.strip(), b.strip()))
        return pts
    return keyed(v)


def _cell(v: Any) -> Any:
    if isinstance(v, bool):
        return "TRUE" if v else "FALSE"
    return v


def _target(el, cdef: ComponentDef, p: ParameterDef) -> Target:
    if _is_profile(p):
        if cdef.id == "signal.road_profile":
            mode = el.parameterOverrides.get("mode", next(
                (q.default for q in cdef.parameters if q.key == "mode"), "distance"))
            axis = _q("Time", "s") if mode == "time" else _q("Distance", "m")
            return Target("profile", [axis], _q("Grade", "%"), True, p.label)
        return Target("profile", [_q("Time", "s")], _q("Target Speed", "km/h"), True, p.label)
    axes = [_q(a.name, a.unit) for a in (p.axes or [])]
    return Target(p.type, axes, _q(p.label, p.unit), label=p.label)


def _table_rows(el, cdef: ComponentDef, p: ParameterDef, value: Any) -> list[list[Any]]:
    """A table's sheet in the layout the importer reads: a title row, then
    for a 1-D table or profile an x and a y column, for a map the column
    values along a row and the row values down the first column."""
    t = _target(el, cdef, p)
    title = [f"{el.label} · {p.label}"]
    if t.kind == "table2d":
        outer, inner = t.axes
        outer_keys = sorted(value, key=float)
        inner_keys = sorted({k for o in value.values() for k in o}, key=float)
        rows = [[f"{t.value.name} [{t.value.unit}]"],
                [f"{inner.name} [{inner.unit}] \\ {outer.name} [{outer.unit}]",
                 *(float(k) for k in outer_keys)]]
        for ik in inner_keys:
            rows.append([float(ik), *(value[ok].get(ik) for ok in outer_keys)])
        return [title, *rows]
    if t.kind == "profile":
        pts = []
        for chunk in str(value).replace("\n", ";").split(";"):
            if not chunk.strip():
                continue
            sep = ":" if ":" in chunk else ","
            a, _, b = chunk.partition(sep)
            try:
                pts.append([float(a), float(b)])
            except ValueError:
                pts.append([a.strip(), b.strip()])
    else:
        pts = [[float(k), v] for k, v in sorted(value.items(), key=lambda kv: float(kv[0]))]
    x, y = t.axes[0], t.value
    return [title, [f"{x.name} [{x.unit}]", f"{y.name} [{y.unit}]"], *pts]


@dataclass
class _Row:
    el: Any
    cdef: ComponentDef
    p: ParameterDef
    value: Any


def _rows(project: Project) -> list[_Row]:
    lib = library_by_id()
    out = []
    for el in _elements(project):
        cdef = lib.get(el.componentDefId)
        if cdef is None:
            continue
        for p in cdef.parameters:
            if p.type == "code":
                continue  # scripts stay in the project
            out.append(_Row(el, cdef, p, _value(el, p)))
    return out


def export_sheets(project: Project, tables_inline: bool = False) -> list[OutSheet]:
    """The workbook's sheets: Parameters, then a sheet per table, map and
    profile (or, with ``tables_inline``, the tables as JSON in the Value
    cell, for CSV)."""
    main: list[list[Any]] = [HEADER]
    extra: list[OutSheet] = []
    taken = {PARAMS_SHEET.lower(), "about"}
    for r in _rows(project):
        p = r.p
        changed = "yes" if p.key in r.el.parameterOverrides and not _same(
            r.value, p.default) else ""
        if _is_table(p) and not tables_inline:
            name = sheet_title(f"{r.el.label} - {p.label.split(' (')[0]}", taken)
            extra.append(OutSheet(name, _table_rows(r.el, r.cdef, p, r.value), header_rows=0))
            value: Any = f"{_SEE_SHEET}'{name}'"
            default: Any = ""
        elif _is_table(p):
            value = json.dumps(r.value, separators=(",", ":")) if not isinstance(
                r.value, str) else r.value
            default = ""
        else:
            value, default = _cell(r.value), _cell(p.default)
        main.append([r.el.label, r.el.id, r.cdef.name, p.label, p.key, value, p.unit, default,
                     changed, "", ""])
    about = OutSheet("About", [
        ["LightSim parameter sheet"],
        ["Project", project.name],
        ["Project ID", project.id],
        ["Exported", time.strftime("%Y-%m-%d %H:%M UTC", time.gmtime())],
        ["LightSim", VERSION],
        [],
        ["How to use it"],
        ["Change values in the Value column of the Parameters sheet, or the numbers on a "
         "table's own sheet. Keep the Part ID, Key and Unit columns as they are: LightSim "
         "matches rows by Part ID and Key and refuses a row whose unit differs."],
        ["Source and Notes are for your team (where a number comes from); LightSim does "
         "not read them back."],
        ["Import the file in LightSim with Import sheet on the Parameters tab: it lists every "
         "change before applying it."],
    ], header_rows=0, widths=[18, 90])
    widths = [22, 16, 18, 30, 22, 22, 9, 16, 9, 28, 28]
    return [OutSheet(PARAMS_SHEET, main, widths=widths), *extra, about]


def export_xlsx(project: Project) -> bytes:
    return xlsx_bytes(export_sheets(project))


def export_csv(project: Project) -> bytes:
    return csv_bytes(export_sheets(project, tables_inline=True)[0].rows)


# ---- import ----------------------------------------------------------------------

@dataclass
class Change:
    elementId: str
    element: str
    key: str
    parameter: str
    unit: str
    old: Any
    new: Any
    row: Optional[int] = None
    sheet: Optional[str] = None

    def as_dict(self) -> dict:
        return {k: v for k, v in self.__dict__.items() if v is not None}


@dataclass
class SheetImport:
    changes: list[Change] = field(default_factory=list)
    errors: list[dict] = field(default_factory=list)
    warnings: list[dict] = field(default_factory=list)
    rows: int = 0
    unchanged: int = 0

    def as_dict(self) -> dict:
        return {"ok": not self.errors, "changes": [c.as_dict() for c in self.changes],
                "errors": self.errors, "warnings": self.warnings, "rows": self.rows,
                "unchanged": self.unchanged}


def _find_header(sheet: Sheet) -> Optional[tuple[int, dict[str, int]]]:
    for r, row in enumerate(sheet.rows[:20]):
        cols = {str(c).strip().lower(): i for i, c in enumerate(row) if isinstance(c, str)}
        if all(h.lower() in cols for h in _REQUIRED):
            return r, {h: cols[h.lower()] for h in HEADER if h.lower() in cols}
    return None


def _parse(p: ParameterDef, raw: Any) -> tuple[Any, Optional[str]]:
    """(value, problem) for a cell read for parameter ``p``."""
    if p.type == "number":
        if isinstance(raw, float):
            return (int(raw) if raw.is_integer() and isinstance(p.default, int)
                    and not isinstance(p.default, bool) else raw), None
        return None, f"'{raw}' is not a number"
    if p.type == "boolean":
        t = str(raw if not isinstance(raw, float) else int(raw)).strip().lower()
        if t in _TRUE:
            return True, None
        if t in _FALSE:
            return False, None
        return None, f"'{raw}' is not TRUE or FALSE"
    if p.type == "enum":
        t = str(raw).strip()
        match = next((o for o in p.options or [] if o.lower() == t.lower()), None)
        if match is None:
            return None, f"'{t}' is not one of: {', '.join(p.options or [])}"
        return match, None
    if isinstance(raw, float):
        return (str(int(raw)) if raw.is_integer() else str(raw)), None
    return str(raw), None


def import_sheet(project: Project, data: bytes, filename: str) -> SheetImport:
    """The changes a parameter sheet makes to ``project``, or why it is refused."""
    out = SheetImport()
    sheets = read_file(data, filename)
    by_name = {s.name.lower(): s for s in sheets}
    main = None
    for s in sheets:
        found = _find_header(s)
        if found:
            main, (hr, cols) = s, found
            break
    if main is None:
        out.errors.append({"text": "No parameter list found: a sheet needs the columns "
                                   "Part ID, Key and Value (as LightSim exports them)."})
        return out
    lib = library_by_id()
    elements = {e.id: e for e in _elements(project)}
    labels: dict[str, list] = {}
    for e in elements.values():
        labels.setdefault(e.label.strip().lower(), []).append(e)
    seen: dict[tuple[str, str], int] = {}

    def err(row: int, text: str) -> None:
        out.errors.append({"row": row, "sheet": main.name, "text": f"Row {row}: {text}"})

    def get(row: list, name: str) -> Any:
        i = cols.get(name)
        return row[i] if i is not None and i < len(row) else None

    for r in range(hr + 1, len(main.rows)):
        row = main.rows[r]
        n = r + 1
        pid, key, raw = get(row, "Part ID"), get(row, "Key"), get(row, "Value")
        if all(v is None for v in (pid, key, raw)):
            continue
        out.rows += 1
        el = elements.get(str(pid).strip()) if pid is not None else None
        if el is None:
            part = get(row, "Part")
            same = labels.get(str(part).strip().lower(), []) if part is not None else []
            if pid is None and len(same) == 1:
                el = same[0]
            else:
                err(n, f"the model has no part with id '{pid}'"
                    + (f" ({part})" if part else "") + ".")
                continue
        cdef = lib.get(el.componentDefId)
        p = next((q for q in cdef.parameters if q.key == str(key).strip()), None) if cdef else None
        if p is None:
            err(n, f"{el.label} has no parameter '{key}'.")
            continue
        if (el.id, p.key) in seen:
            err(n, f"{el.label} · {p.label} is also in row {seen[(el.id, p.key)]}.")
            continue
        seen[(el.id, p.key)] = n
        unit = get(row, "Unit")
        if unit is not None and str(unit).strip() and _unit_differs(str(unit).strip(), p.unit):
            err(n, f"{el.label} · {p.label} is in {p.unit}, but the row says '{unit}'. "
                   f"Write the value in {p.unit}.")
            continue
        if p.type == "code":
            err(n, f"{el.label} · {p.label} is a script; edit it in LightSim.")
            continue
        if raw is None and p.type == "string" and not _is_profile(p):
            raw = ""
        if raw is None:
            err(n, f"{el.label} · {p.label} has no value.")
            continue
        current = _value(el, p)
        if _is_table(p):
            text = str(raw).strip()
            if text.lower().startswith(_SEE_SHEET):
                name = text[len(_SEE_SHEET):].strip().strip("'\"")
                sheet = by_name.get(name.lower())
                if sheet is None:
                    err(n, f"the sheet '{name}' for {el.label} · {p.label} is missing.")
                    continue
                res = import_table(sheet, _target(el, cdef, p))
                if res.errors:
                    for e in res.errors:
                        out.errors.append({"sheet": sheet.name, "row": e.row,
                                           "text": f"Sheet '{sheet.name}': {e.text}"})
                    continue
                new: Any = res.value
                where = sheet.name
            else:
                try:
                    new = json.loads(text) if p.type != "string" else text
                except ValueError:
                    err(n, f"{el.label} · {p.label} must be a table; give it a sheet of its "
                           "own (export to .xlsx to see how).")
                    continue
                if p.type != "string" and not isinstance(new, dict):
                    err(n, f"{el.label} · {p.label} must be a table.")
                    continue
                where = main.name
            if _norm_table(p, new) == _norm_table(p, current):
                out.unchanged += 1
            else:
                out.changes.append(Change(el.id, el.label, p.key, p.label, p.unit, current, new,
                                          n, where))
            continue
        new, problem = _parse(p, raw)
        if problem:
            err(n, f"{el.label} · {p.label}: {problem}.")
            continue
        if p.type == "number":
            limit = p.range_problem(float(new))
            if limit:
                out.warnings.append({"row": n, "sheet": main.name,
                                     "text": f"Row {n}: {el.label} · {p.label} {limit} "
                                             "(Data Checks will report it)."})
        if _same(new, current):
            out.unchanged += 1
        else:
            out.changes.append(Change(el.id, el.label, p.key, p.label, p.unit, current, new, n,
                                      main.name))
    return out


def _unit_differs(written: str, unit: str) -> bool:
    if written == unit:
        return False
    a, b = U.canonical(written), U.canonical(unit)
    if a is not None and a == b:
        return False
    return written.replace(" ", "") != unit.replace(" ", "")
