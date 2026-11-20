"""Find table data in a sheet, whatever its layout, and fit it to a table.

One importer for a file (.xlsx, CSV, text) and for pasted text (the
clipboard as Excel copies it is tab-separated text, read like a CSV file):
the same detection, the same preview, the same result.

The layouts it recognises (docs/help/how-to/import-a-table-from-a-file.md):

* two columns: an axis and its values;
* a matrix (a pivot table): one axis's breakpoints along the first row,
  another's down the first column, values in the body; with two or more
  key columns on the left (an Excel pivot table with several row fields,
  blank where the key repeats), each key column is an axis;
* a list: a column per axis and one or more value columns, rows in any
  order (the parameter sheet's long form);
* several of these, one per breakpoint of further axes, one under another,
  side by side or a worksheet each, each with a label such as
  ``Temperature = 25 °C``, ``T [°C]: 25`` or ``T = 25 °C, U = 400 V`` in a
  cell above it (or the sheet's name);
* any of these with title rows, a units row under the header row, empty
  rows and columns around the data and notes beside it.

Every number block (cells of numbers that touch) is a table; texts above
and beside it name its columns, rows and labels. What was found is a list
of *fields*: the axes and value columns of the data, each with its title,
unit and where it was read. The fields are matched to the target's axes and
values by name and by the kind of unit (or as the user chose), converted to
the stored units, and laid out as the full grid the target stores
(docs/spec/tables.md). Nothing is guessed silently: what was matched by
order alone is asked, and every problem names its cell.
"""
from __future__ import annotations

import re
from collections import deque
from dataclasses import dataclass, field
from math import prod
from typing import Any, Optional

from ..library import library_by_id, unit_groups
from ..schemas import TABLE_TYPES
from . import units as U
from .sheets import Cell, Sheet, cell_name, column_letter
from .tables import Quantity, _match_score, _q

#: Most grid points an import builds (a 30 x 30 x 10 x 10 map is 90,000).
MAX_POINTS = 2_000_000
#: Most problems listed
MAX_PROBLEMS = 20

_NUM = r"[-+]?(?:\d+(?:[.,]\d+)?|[.,]\d+)(?:[eE][-+]?\d+)?"
# "Temperature = 25 °C", "T [°C]: 25", "T=25degC", "U (V) = 400"
_LABEL_EQ = re.compile(
    rf"^\s*(?P<name>[^=:\[\]()]*?)\s*(?:[\[(](?P<u1>[^\])]*)[\])])?\s*[=:]\s*(?P<num>{_NUM})"
    r"\s*(?P<u2>[^\s\d][^,;]*?)?\s*$")
# "25 °C", "Temperature 25 °C", "25degC": the unit is then needed
_LABEL_BARE = re.compile(rf"^\s*(?:(?P<name>[A-Za-z][^\d=:]*?)\s+)?(?P<num>{_NUM})\s*(?P<u2>\S+)\s*$")
_LABEL_SPLIT = re.compile(r"\s*;\s*|,\s+(?=\D)|\s+&\s+|\s+and\s+")
_NAME_SLASH = re.compile(r"^(.*?\S)\s+/\s+(\S+)$")
_CORNER_SPLIT = re.compile(r"\s*\\\s*|\s+/\s+|\s*\|\s*")


# ---- the target ------------------------------------------------------------------

@dataclass
class Slot:
    """An axis or a table of the target: its name and the unit its numbers
    are stored in. A Map port with a unit text keeps its numbers as they are
    (``convert`` False)."""

    key: str
    name: str
    unit: str
    convert: bool = True
    words: tuple[str, ...] = ()


@dataclass
class DataTarget:
    label: str
    axes: list[Slot]
    values: list[Slot]
    #: a Map block: its values are its outputs, and the import may set its
    #: inputs and outputs from the data
    map: bool = False


def _slot(key: str, name: str, unit: str, convert: bool = True) -> Slot:
    return Slot(key, name, unit, convert, _q(name, unit).words)


def data_target(component_def_id: str, param_key: str,
                ports: Optional[list[dict]] = None) -> DataTarget:
    """The target of a library table (any number of axes) or of a Map
    block's maps (``ports``: its dynamic ports). Raises KeyError for a part
    or parameter that does not exist, ValueError for one that is not a
    table."""
    cdef = library_by_id()[component_def_id]
    p = next((q for q in cdef.parameters if q.key == param_key), None)
    if p is None:
        raise KeyError(param_key)
    if p.type == "maps":
        groups = unit_groups()

        def slot(port: dict) -> Slot:
            text = port.get("unitText")
            if text:
                return _slot(port["id"], port.get("name") or port["id"], text, False)
            return _slot(port["id"], port.get("name") or port["id"],
                         groups.get(port.get("unitGroup") or "No Unit", "-"))
        ports = ports or []
        return DataTarget(cdef.name, [slot(q) for q in ports if q.get("direction") == "input"],
                          [slot(q) for q in ports if q.get("direction") == "output"], True)
    if p.type in TABLE_TYPES and p.axes:
        return DataTarget(p.label, [_slot(str(i), a.name, a.unit) for i, a in enumerate(p.axes)],
                          [_slot("value", p.label, p.unit)])
    raise ValueError(f"'{p.label}' is not a table")


# ---- what was found ----------------------------------------------------------------

@dataclass
class Problem:
    text: str
    level: str = "error"  # "error" | "warning" | "question"
    cell: Optional[str] = None
    sheet: Optional[str] = None

    def as_dict(self) -> dict:
        return {k: v for k, v in (("text", self.text), ("level", self.level),
                                  ("cell", self.cell), ("sheet", self.sheet)) if v is not None}


@dataclass
class Field:
    """An axis or a column of values found in the data."""

    title: str  # as written: "Speed [rpm]"; "" when nothing names it
    name: str
    unit: Optional[str]  # canonical
    written: Optional[str]  # the unit as written
    where: str  # "column B", "row 3", "the labels"
    role: str = "value"  # "axis" | "value"
    #: the layout says what it is (a matrix's axes and values, a label)
    fixed: bool = False


@dataclass
class Label:
    name: str
    value: float
    unit: Optional[str]
    written: Optional[str]
    text: str
    cell: str


@dataclass
class Block:
    sheet: str
    box: tuple[int, int, int, int]
    kind: str  # "list" | "matrix"
    fields: list[Field]
    #: a record per point: (cell, cell name) per field
    records: list[list[tuple[Cell, str]]]
    labels: list[Label] = field(default_factory=list)

    @property
    def range(self) -> str:
        r0, c0, r1, c1 = self.box
        return f"{cell_name(r0, c0)}:{cell_name(r1, c1)}"


def _text(c: Cell) -> bool:
    return isinstance(c, str) and c.strip() != ""


def _num(c: Cell) -> bool:
    return isinstance(c, float)


def _number(t: str) -> float:
    return float(t.replace(",", ".") if "." not in t else t.replace(",", ""))


def split_title(title: str) -> tuple[str, Optional[str], Optional[str]]:
    """(name, unit, unit as written) of a column or axis title: "Speed
    [rpm]", "Speed (1/min)", "Speed / rpm", "Speed in rad/s", "Torque_Nm"."""
    t = str(title).strip()
    m = _NAME_SLASH.match(t)
    if m and U.canonical(m.group(2)):
        return m.group(1), U.canonical(m.group(2)), m.group(2)
    name, unit, written = U.split_header(t)
    if unit is None and written and " " in written.strip():
        return t, None, None  # words in brackets: "Loss map (supplier data)"
    return name, unit, written


def parse_label(text: str) -> Optional[list[tuple[str, float, Optional[str], Optional[str]]]]:
    """The (name, value, unit, unit as written) parts of a label such as
    ``Temperature = 25 °C``, ``T [°C]: 25``, ``25 °C`` or ``T = 25 °C, U =
    400 V``; None when the text is not a label."""
    parts = []
    for part in _LABEL_SPLIT.split(str(text).strip()):
        m = _LABEL_EQ.match(part)
        if m:
            written = (m.group("u1") or m.group("u2") or "").strip() or None
            parts.append((m.group("name").strip(), _number(m.group("num")),
                          U.canonical(written) if written else None, written))
            continue
        m = _LABEL_BARE.match(part)
        if m and U.canonical(m.group("u2")) and U.canonical(m.group("u2")) != "-":
            parts.append(((m.group("name") or "").strip(), _number(m.group("num")),
                          U.canonical(m.group("u2")), m.group("u2")))
            continue
        return None
    return parts or None


class _Grid:
    def __init__(self, sheet: Sheet):
        self.rows = sheet.rows
        self.name = sheet.name
        #: cells read otherwise than written: a label in two cells, "T [°C]" | 25
        self.over: dict[tuple[int, int], Cell] = {}

    def get(self, r: int, c: int) -> Cell:
        if (r, c) in self.over:
            return self.over[(r, c)]
        if 0 <= r < len(self.rows) and 0 <= c < len(self.rows[r]):
            return self.rows[r][c]
        return None


def _components(g: _Grid) -> list[list[tuple[int, int]]]:
    """The groups of number cells that touch (above, below, left, right)."""
    seen: set[tuple[int, int]] = set()
    out = []
    for r, row in enumerate(g.rows):
        for c, v in enumerate(row):
            if not _num(v) or (r, c) in seen:
                continue
            seen.add((r, c))
            stack, cells = [(r, c)], []
            while stack:
                a, b = stack.pop()
                cells.append((a, b))
                for x, y in ((a + 1, b), (a - 1, b), (a, b + 1), (a, b - 1)):
                    if (x, y) not in seen and _num(g.get(x, y)):
                        seen.add((x, y))
                        stack.append((x, y))
            out.append(cells)
    return out


def _monotone(vals: list[Cell]) -> bool:
    nums = [v for v in vals if _num(v)]
    if len(nums) != len(vals):
        return False
    inc = all(b > a for a, b in zip(nums, nums[1:]))
    dec = all(b < a for a, b in zip(nums, nums[1:]))
    return inc or dec


def _field(title: str, where: str, role: str, fixed: bool) -> Field:
    name, unit, written = split_title(title) if title else ("", None, None)
    return Field(title, name, unit, written, where, role, fixed)


def _texts_above(g: _Grid, r0: int, c0: int, c1: int) -> list[tuple[int, dict[int, str]]]:
    """The rows of text just above a block (nearest first), up to the row
    before, stopping at a row that holds numbers."""
    out = []
    for r in range(r0 - 1, max(-1, r0 - 7), -1):
        cells = {c: g.get(r, c) for c in range(max(0, c0 - 1), c1 + 1)}
        if any(_num(v) for c, v in cells.items() if c >= c0):
            break
        texts = {c: str(v).strip() for c, v in cells.items() if _text(v)}
        if texts:
            out.append((r, texts))
    return out


def _labels_of(texts: dict[int, str], r: int) -> tuple[list[Label], dict[int, str]]:
    """The labels among a row's texts, and the texts that are not labels."""
    labels, rest = [], {}
    for c, t in texts.items():
        parts = parse_label(t)
        if parts:
            labels += [Label(n, v, u, w, t, cell_name(r, c)) for n, v, u, w in parts]
        else:
            rest[c] = t
    return labels, rest


def _is_units_row(texts: dict[int, str]) -> bool:
    return bool(texts) and all(U.canonical(t.strip("[]() ")) for t in texts.values())


def _list_block(g: _Grid, box: tuple[int, int, int, int]) -> Block:
    r0, c0, r1, c1 = box
    labels: list[Label] = []
    names: dict[int, str] = {}
    unit_row: dict[int, str] = {}
    rows = _texts_above(g, r0, c0, c1)
    width = c1 - c0 + 1
    header_found = False
    for i, (r, texts) in enumerate(rows):
        found, rest = _labels_of(texts, r)
        labels += found
        rest = {c: t for c, t in rest.items() if c >= c0}
        if header_found or not rest:
            continue
        nxt = next((t for _, t in rows[i + 1:i + 2]), None)
        if not unit_row and not names and _is_units_row(rest) and nxt \
                and len({c: t for c, t in nxt.items() if c >= c0}) >= max(1, width // 2):
            unit_row = {c: t.strip("[]() ") for c, t in rest.items()}
            continue
        if len(rest) >= max(1, width // 2) or len(rest) == width:
            names = rest
            header_found = True
    fields = []
    for c in range(c0, c1 + 1):
        f = _field(names.get(c, ""), f"column {column_letter(c)}", "value", False)
        if c in unit_row:
            f.written, f.unit = unit_row[c], U.canonical(unit_row[c])
            f.title = f"{f.title} [{unit_row[c]}]".strip()
        fields.append(f)
    records = [[(g.get(r, c), cell_name(r, c)) for c in range(c0, c1 + 1)]
               for r in range(r0, r1 + 1)
               if any(g.get(r, c) is not None for c in range(c0, c1 + 1))]
    return Block(g.name, box, "list", fields, records, labels)


def _looks_like(f: Field, slots: list[Slot]) -> int:
    return max((_score(f, s) for s in slots), default=0)


def _matrix_block(g: _Grid, box: tuple[int, int, int, int], k: int,
                  target: Optional[DataTarget]) -> Block:
    """A matrix whose top row holds a column axis from column c0 + k, with k
    key columns on the left (the last of them the row axis)."""
    r0, c0, r1, c1 = box
    hc = c0 + k  # first column of the column axis
    labels: list[Label] = []
    titles: list[str] = []
    for r, texts in _texts_above(g, r0, c0, c1):
        found, rest = _labels_of(texts, r)
        labels += found
        titles += [t for c, t in sorted(rest.items()) if c >= c0]
    corner = g.get(r0, hc - 1)
    row_t = col_t = val_t = ""
    if _text(corner):
        ct = str(corner).strip()
        parts = parse_label(ct)
        if parts:
            labels += [Label(n, v, u, w, ct, cell_name(r0, hc - 1)) for n, v, u, w in parts]
        else:
            m = _NAME_SLASH.match(ct)
            pieces = [] if m and U.canonical(m.group(2)) else _CORNER_SPLIT.split(ct)
            if len(pieces) == 2 and all(pieces):  # "Torque [Nm] \ Speed [rpm]": rows \ columns
                row_t, col_t = pieces
            elif k > 1:  # beside the other key columns' names: the row axis's
                row_t = ct
            else:
                val_t = ct
    left = [str(g.get(r, hc - 2)).strip() for r in range(r0 + 1, r1 + 1)
            if k == 1 and _text(g.get(r, hc - 2))]
    if not row_t and left:
        row_t = left[0]
    axes = target.axes if target else []
    vals = target.values if target else []
    # a single corner text names the values, unless it names an axis better
    if val_t and target and _looks_like(_field(val_t, "", "axis", True), axes) > \
            _looks_like(_field(val_t, "", "value", True), vals):
        row_t, val_t = val_t, ""
    for t in sorted(titles, key=lambda t: split_title(t)[1] is None):
        f = _field(t, "", "axis", True)
        if not col_t and (f.unit or _looks_like(f, axes)) and (
                not target or _looks_like(f, axes) >= _looks_like(f, vals)):
            col_t = t
        elif not val_t:
            val_t = t
    keys = [str(g.get(r0, c)).strip() if _text(g.get(r0, c)) else "" for c in range(c0, hc - 1)]
    fields = [_field(t, f"column {column_letter(c0 + i)}", "axis", True)
              for i, t in enumerate(keys)]
    fields.append(_field(col_t, f"row {r0 + 1}", "axis", True))
    fields.append(_field(row_t, f"column {column_letter(hc - 1)}", "axis", True))
    fields.append(_field(val_t, f"{cell_name(r0 + 1, hc)}:{cell_name(r1, c1)}", "value", True))
    records = []
    last: list[Cell] = [None] * k
    for r in range(r0 + 1, r1 + 1):
        if all(g.get(r, c) is None for c in range(c0, c1 + 1)):
            continue
        key = []
        for i in range(k):
            v = g.get(r, c0 + i)
            # a pivot table leaves a repeated key blank
            if v is None and k > 1 and i < k - 1:
                v = last[i]
            last[i] = v
            key.append((v, cell_name(r, c0 + i)))
        for c in range(hc, c1 + 1):
            records.append([*key[:-1], (g.get(r0, c), cell_name(r0, c)), key[-1],
                            (g.get(r, c), cell_name(r, c))])
    return Block(g.name, box, "matrix", fields, records, labels)


def find_blocks(sheet: Sheet, target: Optional[DataTarget]) -> tuple[list[Block], list[str]]:
    """The tables in a sheet, top to bottom, left to right; and notes on
    the cells left out."""
    g = _Grid(sheet)
    comps = _components(g)
    blocks: list[Block] = []
    notes: list[str] = []
    for cells in sorted(comps, key=lambda cs: min(cs)):
        rs = [r for r, _ in cells]
        cs = [c for _, c in cells]
        box = (min(rs), min(cs), max(rs), max(cs))
        r0, c0, r1, c1 = box
        if len(cells) == 1:
            # a number with a text to its left is a label: "T [°C]" | 25
            left = g.get(r0, c0 - 1)
            if _text(left):
                parts = parse_label(f"{str(left).strip()} = {g.get(r0, c0)}")
                if parts:
                    g.over[(r0, c0 - 1)] = f"{str(left).strip()} = {g.get(r0, c0):g}"
                    g.over[(r0, c0)] = None
                    continue
            notes.append(f"Cell {cell_name(r0, c0)} (a number on its own) was left out.")
            continue
        top = [g.get(r0, c) for c in range(c0, c1 + 1)]
        k = 0
        while k < len(top) and not _num(top[k]):
            k += 1
        if 1 <= k < len(top) and r1 > r0 and _monotone(top[k:]):
            blocks.append(_matrix_block(g, box, k, target))
        else:
            blocks.append(_list_block(g, box))
    # a list broken by an empty row is one list
    merged: list[Block] = []
    for b in blocks:
        p = merged[-1] if merged else None
        if p and p.kind == b.kind == "list" and p.box[1] == b.box[1] and p.box[3] == b.box[3] \
                and 0 < b.box[0] - p.box[2] <= 2 and not b.labels \
                and not any(f.title for f in b.fields):
            p.records += b.records
            p.box = (p.box[0], p.box[1], b.box[2], p.box[3])
            notes.append(f"The empty row {p.box[0] + 1 + len(p.records) - len(b.records)} "
                         "inside the list was skipped.")
            continue
        merged.append(b)
    return [b for b in merged if b.records], notes


# ---- matching ----------------------------------------------------------------------

def _score(f: Field, s: Slot) -> int:
    n = _match_score(f.name, Quantity(s.name, s.unit, s.words)) if f.name else 0
    if s.convert and s.unit != "-":
        u = 2 if f.unit and U.same_group(f.unit, s.unit) else 0
    else:
        u = 2 if s.unit != "-" and (f.written or f.unit) in (s.unit,) else 0
    return n + u


def _how(f: Field, s: Slot) -> str:
    n = _match_score(f.name, Quantity(s.name, s.unit, s.words)) if f.name else 0
    u = _score(f, s) - n
    return "name and unit" if n and u else "name" if n else "unit" if u else "order"


def _assign(fields: list[int], slots: list[Slot], all_fields: list[Field],
            chosen: dict[int, Optional[int]]) -> tuple[dict[int, tuple[int, str]], list[int]]:
    """Fields to slots: as the user chose, then by score, then in order.
    Returns {field: (slot, how)} and the fields left over."""
    out: dict[int, tuple[int, str]] = {}
    taken: set[int] = set()
    free = []
    for i in fields:
        if i in chosen:
            if chosen[i] is not None and chosen[i] not in taken:
                out[i] = (chosen[i], "chosen")
                taken.add(chosen[i])
        else:
            free.append(i)
    pairs = sorted(((_score(all_fields[i], s), i, j) for i in free for j, s in enumerate(slots)
                    if j not in taken), key=lambda p: (-p[0], p[1], p[2]))
    for score, i, j in pairs:
        if score <= 0 or i in out or j in taken:
            continue
        out[i] = (j, _how(all_fields[i], slots[j]))
        taken.add(j)
    rest = [i for i in free if i not in out]
    open_slots = [j for j in range(len(slots)) if j not in taken]
    for i, j in zip(rest, open_slots):
        out[i] = (j, "order")
    return out, [i for i in rest if i not in out]


def _key_prefix(records: list[list[tuple[Cell, str]]], width: int) -> int:
    """How many columns from the left tell the rows apart: the axes of a list."""
    for k in range(1, width):
        seen = set()
        for rec in records:
            t = tuple(rec[i][0] for i in range(k))
            if t in seen:
                break
            seen.add(t)
        else:
            return k
    return max(1, width - 1)


# ---- the import ----------------------------------------------------------------------

def _fmt(v: float) -> str:
    return f"{v:.12g}"


def _slugify(name: str) -> str:
    return re.sub(r"^_+|_+$", "", re.sub(r"[^a-z0-9]+", "_", name.strip().lower())) or "port"


def _port_for(f: Field, unit: Optional[str], groups: dict[str, str]) -> dict:
    """A Map port for a field: the library's unit group of its unit (the
    numbers converted to the group's unit, the field's own unit shown), or
    its unit as a unit text."""
    if unit and unit != "-":
        same = [g for g, u in groups.items() if U.canonical(u) == unit] or \
               [g for g, u in groups.items() if u != "-" and U.same_group(u, unit)]
        if same:
            return {"unitGroup": same[0], "lib": groups[same[0]],
                    "display": unit if unit != groups[same[0]] else None}
        return {"unitGroup": "No Unit", "unitText": unit, "lib": unit}
    if f.written and not unit:
        return {"unitGroup": "No Unit", "unitText": f.written, "lib": f.written}
    return {"unitGroup": "No Unit", "lib": "-"}


def _fill_nearest(axes: list[list[float]], have: dict[tuple, float]) -> tuple[dict[tuple, float], int]:
    """Every missing point of the grid from its nearest point that has a
    value (fewest steps along the axes; on a tie, the first in the grid's
    order)."""
    index = [{v: i for i, v in enumerate(a)} for a in axes]
    shape = [len(a) for a in axes]
    vals: dict[tuple[int, ...], float] = {tuple(index[d][k[d]] for d in range(len(axes))): v
                                          for k, v in have.items()}
    queue = deque(sorted(vals))
    filled = 0
    while queue:
        p = queue.popleft()
        for d in range(len(shape)):
            for step in (-1, 1):
                q = p[:d] + (p[d] + step,) + p[d + 1:]
                if 0 <= q[d] < shape[d] and q not in vals:
                    vals[q] = vals[p]
                    filled += 1
                    queue.append(q)
    return {tuple(axes[d][i[d]] for d in range(len(axes))): v for i, v in vals.items()}, filled


def _nest(axes: list[list[float]], get, prefix: tuple = ()) -> Any:
    d = len(prefix)
    if d == len(axes) - 1:
        return {_fmt(x): get(prefix + (x,)) for x in axes[d]}
    return {_fmt(x): _nest(axes, get, prefix + (x,)) for x in axes[d]}


def _label_fields(blocks: list[Block]) -> tuple[list[Field], list[Problem]]:
    first = blocks[0].labels
    problems = []
    fields = []
    for i, lab in enumerate(first):
        title = lab.name + (f" [{lab.written}]" if lab.written else "")
        f = Field(title, lab.name, lab.unit, lab.written, "the labels", "axis", True)
        fields.append(f)
        for b in blocks[1:]:
            other = b.labels[i]
            if other.name and lab.name and other.name.lower() != lab.name.lower() and not (
                    other.unit and lab.unit and U.same_group(other.unit, lab.unit)):
                problems.append(Problem(
                    f"The labels do not agree: '{lab.text}' ({lab.cell}) and '{other.text}' "
                    f"({other.cell}) name different things.", cell=other.cell, sheet=b.sheet))
    return fields, problems


def read_data(sheets: list[Sheet], target: DataTarget, opts: Optional[dict] = None) -> dict:
    """Find the data in ``sheets`` and fit it to ``target``. ``opts``:
    ``sheet`` (a sheet's name, or "*" for every sheet), ``block`` (which of
    several unlabelled tables), ``fields`` ({field index: "axis:<i>" |
    "value:<i>" | "skip"}, as the user matched them), ``units`` ({field
    index: unit to read it in}), ``fill`` ("refuse" or "nearest", for a grid
    with holes), ``build`` (a Map block: its inputs and outputs from the
    data). Answers with what was found and how it was matched, the problems,
    and the tables ready to store when there are none."""
    opts = dict(opts or {})
    problems: list[Problem] = []
    notes: list[str] = []
    res: dict[str, Any] = {"ok": False, "layout": "", "fields": [], "notes": notes,
                           "problems": [], "points": 0, "holes": 0, "filled": 0,
                           "targetAxes": [{"name": s.name, "unit": s.unit} for s in target.axes],
                           "targetValues": [{"key": s.key, "name": s.name, "unit": s.unit}
                                            for s in target.values],
                           "blocks": [], "block": None, "sheet": None, "sheetLabels": False}

    def done() -> dict:
        res["problems"] = [p.as_dict() for p in problems[:MAX_PROBLEMS]]
        res["ok"] = not any(p.level == "error" for p in problems) and res.get("tables") is not None
        return res

    # the tables in every sheet
    found: list[tuple[Sheet, list[Block]]] = []
    for s in sheets:
        bs, ns = find_blocks(s, target)
        if bs:
            found.append((s, bs))
            notes += [f"{n}" if len(sheets) == 1 else f"Sheet '{s.name}': {n}" for n in ns]
    if not found:
        problems.append(Problem("No numbers found: the data needs at least one column of "
                                "numbers with its axis."))
        return done()
    for s, bs in found:
        if len(bs) == 1 and not bs[0].labels:
            parts = parse_label(s.name)
            if parts:
                bs[0].labels = [Label(n, v, u, w, s.name, f"sheet '{s.name}'")
                                for n, v, u, w in parts]
    def labelled(bs: list[Block]) -> bool:
        return all(b.labels for b in bs)
    sheet = opts.get("sheet")
    if sheet is None:
        sheet = "*" if len(found) > 1 and all(labelled(bs) for _, bs in found) else found[0][0].name
    chosen = found if sheet == "*" else [(s, bs) for s, bs in found if s.name == sheet] or found[:1]
    res["sheet"] = "*" if sheet == "*" else chosen[0][0].name
    res["sheets"] = [{"name": s.name, "tables": len(bs)} for s, bs in found]
    blocks = [b for _, bs in chosen for b in bs]
    res["blocks"] = [{"sheet": b.sheet, "range": b.range, "kind": b.kind,
                      "label": ", ".join(lab.text for lab in b.labels) or None} for b in blocks]
    if len(blocks) > 1 and not (labelled(blocks) and len({len(b.labels) for b in blocks}) == 1):
        pick = opts.get("block")
        pick = pick if isinstance(pick, int) and 0 <= pick < len(blocks) else 0
        if opts.get("block") is None:
            problems.append(Problem(
                f"{len(blocks)} tables were found, and not every one has a label (such as "
                f"'Temperature = 25 °C') saying which breakpoint it is: the first one, "
                f"{blocks[0].range}, was read. Choose another below, or give each a label.",
                "question"))
        res["block"] = pick
        blocks = [blocks[pick]]
    kinds = {b.kind for b in blocks}
    widths = {len(b.fields) for b in blocks}
    if len(kinds) > 1 or len(widths) > 1:
        problems.append(Problem("The tables are not laid out alike: " + "; ".join(
            f"{b.range} is a {b.kind} of {len(b.fields)} columns" for b in blocks)
            + ". Import them one at a time."))
        return done()
    if res["sheet"] == "*" and len(chosen) > 1:
        res["sheetLabels"] = True

    # the fields: labels first (outermost), then the tables' own
    head = blocks[0]
    label_fields: list[Field] = []
    if len(blocks) > 1 or head.labels:
        label_fields, lp = _label_fields(blocks)
        problems += lp
    fields = label_fields + head.fields
    nl = len(label_fields)
    records = [[(lab.value, lab.cell) for lab in b.labels[:nl]] + rec
               for b in blocks for rec in b.records]
    # a list: which columns are axes
    roles = {int(k): str(v) for k, v in (opts.get("fields") or {}).items()
             if str(k).isdigit() and int(k) < len(fields)}
    if head.kind == "list":
        k = _key_prefix([r[nl:] for r in records], len(head.fields))
        for i, f in enumerate(head.fields):
            f.role = "axis" if i < k else "value"
            if target.axes or target.values:
                a = max((_score(f, s) for s in target.axes), default=0)
                v = max((_score(f, s) for s in target.values), default=0)
                if a >= 2 and a > v:
                    f.role = "axis"
                elif v >= 2 and v > a:
                    f.role = "value"
    for i, f in enumerate(fields):
        want = roles.get(i)
        if want and not f.fixed and want != "skip":
            f.role = want.split(":")[0]

    layout = ("two columns" if head.kind == "list" and len(head.fields) == 2 else
              "a list, a column per axis" if head.kind == "list" else
              "a matrix (pivot table)" if len([f for f in head.fields if f.role == "axis"]) == 2
              else "a pivot table with several key columns")
    if len(blocks) > 1:
        where = "worksheets" if res["sheetLabels"] else "tables"
        res["layout"] = (f"{len(blocks)} {where}, each {layout}, one per "
                         + ", ".join(f.name or f.unit or "label" for f in label_fields))
    else:
        res["layout"] = layout[0].upper() + layout[1:] + (
            f", labelled {', '.join(lab.text for lab in head.labels)}" if head.labels else "")
    res["layout"] += f" ({', '.join(b.range for b in blocks[:4])}{' …' if len(blocks) > 4 else ''})"

    # an axis of one breakpoint the target does not have is left out
    def distinct(i: int) -> list[float]:
        return sorted({v for r in records for v in [r[i][0]] if _num(v)})

    # match the fields to the target
    skip = {i for i, w in roles.items() if w == "skip"}
    axis_fields = [i for i, f in enumerate(fields) if f.role == "axis" and i not in skip]
    value_fields = [i for i, f in enumerate(fields) if f.role == "value" and i not in skip]
    build = bool(target.map and opts.get("build"))
    groups = unit_groups()
    ports: list[dict] = []
    if build:
        axis_fields = [i for i in axis_fields if len(distinct(i)) > 1 or len(axis_fields) == 1]
        slots_a, slots_v = [], []
        taken: set[str] = set()
        # a port the block has, by the same name: its id, so its wires stay
        had = {**{("input", q.name.lower()): q.key for q in target.axes},
               **{("output", q.name.lower()): q.key for q in target.values}}
        for n, i in enumerate(axis_fields + value_fields):
            f = fields[i]
            unit = (opts.get("units") or {}).get(str(i)) or f.unit
            unit = U.canonical(unit) or unit if unit else None
            inp = n < len(axis_fields)
            name = f.name or (f"Input {n + 1}" if inp else f"Output {n - len(axis_fields) + 1}")
            pid, m = had.get(("input" if inp else "output", name.lower()), _slugify(name)), 2
            while pid in taken:
                pid, m = f"{_slugify(name)}_{m}", m + 1
            taken.add(pid)
            p = _port_for(f, unit, groups)
            port = {"id": pid, "name": name, "direction": "input" if inp else "output",
                    "kind": "signal", "unitGroup": p["unitGroup"]}
            if "unitText" in p:
                port["unitText"] = p["unitText"]
            ports.append({**port, "display": p.get("display")})
            (slots_a if inp else slots_v).append(
                _slot(pid, name, p["lib"], "unitText" not in p))
        tgt = DataTarget(target.label, slots_a, slots_v, True)
        a_map = {i: (j, "build") for j, i in enumerate(axis_fields)}
        v_map = {i: (j, "build") for j, i in enumerate(value_fields)}
        a_left: list[int] = []
        v_left: list[int] = []
    elif target.map and not target.axes:
        tgt = target
        a_map, v_map, a_left, v_left = {}, {}, [], []
        problems.append(Problem(
            f"The Map block has no inputs yet: let the import set its inputs ({len(axis_fields)}) "
            f"and outputs ({len(value_fields)}) from the data."))
    else:
        tgt = target

        def chosen(role: str, n: int) -> dict[int, Optional[int]]:
            out: dict[int, Optional[int]] = {}
            for i, w in roles.items():
                if fields[i].role != role or not (w == "skip" or w.startswith(role + ":")):
                    continue
                j = w.split(":")[1] if ":" in w else ""
                out[i] = int(j) if j.isdigit() and int(j) < n else None
            return out
        chosen_a = chosen("axis", len(tgt.axes))
        chosen_v = chosen("value", len(tgt.values))
        a_map, a_left = _assign(axis_fields, tgt.axes, fields, chosen_a)
        v_map, v_left = _assign(value_fields, tgt.values, fields, chosen_v)
        # an extra axis of a single breakpoint is a slice: left out
        for i in list(a_left):
            if len(distinct(i)) <= 1:
                a_left.remove(i)
                skip.add(i)
                v = distinct(i)
                notes.append(f"{fields[i].title or fields[i].where} has one breakpoint"
                             + (f" ({v[0]:g})" if v else "") + f", and {target.label} has no "
                             "such axis: it was left out.")
        for i in a_left:
            f = fields[i]
            problems.append(Problem(
                f"The data has an axis {f.title or ''} ({f.where}, {len(distinct(i))} "
                f"breakpoints) that {target.label} does not have"
                f"{' (its axes: ' + ', '.join(s.name for s in tgt.axes) + ')' if tgt.axes else ''}."
                " Choose what it is below, or leave it out by importing one slice."))
        missing = [s.name for j, s in enumerate(tgt.axes) if j not in {m[0] for m in a_map.values()}]
        if missing:
            problems.append(Problem(
                f"The data has no {', '.join(missing)} axis: {target.label} has "
                f"{len(tgt.axes)} axes ({', '.join(s.name for s in tgt.axes)}) and the data "
                f"{len(axis_fields)}." + (" Let the import set the Map block's inputs and "
                                          "outputs from the data." if target.map else "")))
        missing_v = [s.name for j, s in enumerate(tgt.values)
                     if j not in {m[0] for m in v_map.values()}]
        if missing_v and target.map:
            problems.append(Problem(
                f"No values found for {', '.join(missing_v)}: choose their column below, or "
                "let the import set the Map block's inputs and outputs from the data."))
        elif missing_v:
            problems.append(Problem(f"No values found for {', '.join(missing_v)}."))
        for i in v_left:
            notes.append(f"{fields[i].title or fields[i].where} is not used.")
        if target.map and (a_left or missing or v_left or missing_v):
            res["canBuild"] = True
    if target.map and not target.axes:
        res["canBuild"] = True
    res["build"] = build
    res["ports"] = ports if build else None

    # units: each field in the unit of its slot
    conv: dict[int, Any] = {}
    out_fields = []
    for i, f in enumerate(fields):
        info = {"index": i, "title": f.title, "name": f.name, "where": f.where, "role": f.role,
                "fixed": f.fixed, "unit": f.unit, "written": f.written,
                "count": len(distinct(i)) if f.role == "axis" else None}
        slot_of = a_map.get(i) if f.role == "axis" else v_map.get(i)
        if i in skip or slot_of is None:
            info["slot"] = "skip"
            out_fields.append(info)
            continue
        j, how = slot_of
        s = (tgt.axes if f.role == "axis" else tgt.values)[j]
        info["slot"] = f"{f.role}:{j}"
        slots = tgt.axes if f.role == "axis" else tgt.values
        info["how"] = "only" if how == "order" and len(slots) == 1 else how
        info["target"] = s.name
        chosen_unit = (opts.get("units") or {}).get(str(i))
        used = U.canonical(chosen_unit) if chosen_unit else f.unit
        info["options"] = U.alternatives(s.unit) if s.convert and U.group(s.unit) else [s.unit]
        what = f"{f.title or f.where} ({s.name})"
        if how == "order" and not build and len(tgt.axes if f.role == "axis" else tgt.values) > 1:
            problems.append(Problem(
                f"Nothing in the data says what {f.title or f.where} is: it was read as "
                f"{s.name}. Choose another below if that is wrong.", "question"))
        if chosen_unit and used is None:
            problems.append(Problem(f"{what}: '{chosen_unit}' is not a unit LightSim knows."))
            conv[i] = None
            info["bad"] = True
        elif not s.convert or s.unit == "-":
            conv[i] = lambda v: v
            used = used or s.unit
            if f.unit and f.unit not in (s.unit, "-") and not build:
                notes.append(f"{what} is in {f.written}; {s.name} has no unit LightSim converts, "
                             "so the numbers were taken as they are.")
        elif used is None and f.written:
            problems.append(Problem(f"{what}: the unit '{f.written}' is not one LightSim knows. "
                                    f"Choose its unit below ({', '.join(info['options'])})."))
            conv[i] = None
            info["bad"] = True
        elif used is None:
            used = s.unit
            info["assumed"] = True
            conv[i] = lambda v: v
        elif not U.same_group(used, s.unit):
            problems.append(Problem(
                f"{what} is in {f.written or used}, which is not a unit of "
                f"{s.name} ({', '.join(info['options'])}). Choose the right column or its unit "
                "below."))
            conv[i] = None
            info["bad"] = True
        else:
            sc, of = U.factor(used, s.unit)
            conv[i] = (lambda v: v) if (sc, of) == (1.0, 0.0) else (
                lambda v, sc=sc, of=of: U.tidy(v * sc + of))
        info["used"] = used
        info["stored"] = s.unit
        out_fields.append(info)
    res["fields"] = out_fields
    if any(p.level == "error" for p in problems):
        return done()

    # the points
    a_order = sorted((a_map[i][0], i) for i in a_map if i not in skip)
    v_order = sorted((v_map[i][0], i) for i in v_map if i not in skip)
    depth = len(tgt.axes)
    have: list[dict[tuple, float]] = [{} for _ in tgt.values]
    first_cell: dict[tuple, str] = {}
    holes_at: dict[tuple, str] = {}
    errors = 0
    for rec in records:
        key = []
        bad = False
        for _, i in a_order:
            v, at = rec[i]
            if v is None:
                problems.append(Problem(f"Cell {at} ({fields[i].name or fields[i].where}) is "
                                        "empty: every point needs its breakpoint.", cell=at))
                bad = True
            elif not _num(v):
                problems.append(Problem(f"Cell {at}: '{v}' is not a number "
                                        f"({fields[i].name or fields[i].where}).", cell=at))
                bad = True
            else:
                key.append(conv[i](float(v)))
        if bad:
            errors += 1
            if errors >= MAX_PROBLEMS:
                break
            continue
        k = tuple(key)
        for j, i in v_order:
            v, at = rec[i]
            if v is None:
                holes_at.setdefault(k, at)
                continue
            if not _num(v):
                problems.append(Problem(f"Cell {at}: '{v}' is not a number.", cell=at))
                errors += 1
                continue
            val = conv[i](float(v))
            if k in have[j]:
                if have[j][k] == val:
                    problems.append(Problem(
                        f"Cell {at}: the point {_point(tgt, k)} is in the data twice "
                        f"(also {first_cell[(j, k)]}), with the same value: kept once.",
                        "warning", at))
                else:
                    problems.append(Problem(
                        f"Cell {at}: the point {_point(tgt, k)} is in the data twice, as "
                        f"{have[j][k]:g} ({first_cell[(j, k)]}) and {val:g}. Keep one of them.",
                        cell=at))
                    errors += 1
                continue
            have[j][k] = val
            first_cell[(j, k)] = at
        if errors >= MAX_PROBLEMS:
            break
    if any(p.level == "error" for p in problems):
        return done()
    keys = set().union(*(set(h) for h in have)) | set(holes_at)
    axes_vals = [sorted({k[d] for k in keys}) for d in range(depth)]
    size = prod(len(a) for a in axes_vals)
    if size > MAX_POINTS or (size > 10_000 and size > 4 * len(keys)):
        problems.append(Problem(
            f"The points are not on a grid: their breakpoints make {size:,} points and the data "
            f"has {len(keys):,}. Every axis must have the same breakpoints at every point."))
        return done()
    fill = opts.get("fill") or "refuse"
    holes = max(size - len(h) for h in have) if have else 0
    res["holes"] = holes
    if holes:
        first = next(k for k in _grid_keys(axes_vals) if any(k not in h for h in have))
        at = holes_at.get(first)
        if fill == "nearest":
            total = 0
            for j in range(len(have)):
                have[j], n = _fill_nearest(axes_vals, have[j])
                total += n
            res["filled"] = total
            notes.append(f"{total:,} missing point{'s' if total != 1 else ''} filled from the "
                         "nearest breakpoint that has a value.")
        else:
            problems.append(Problem(
                f"{holes:,} point{'s' if holes != 1 else ''} of the grid "
                f"{'are' if holes != 1 else 'is'} missing, the first at {_point(tgt, first)}"
                + (f" (cell {at} is empty)" if at else "") + ". A table needs a value at every "
                "point: fill the data in, or fill the missing points from the nearest breakpoint "
                "below.", cell=at))
            return done()
    res["tables"] = {s.key: _nest(axes_vals, lambda k, h=have[j]: h[k])
                     for j, s in enumerate(tgt.values)}
    res["axes"] = [{"name": s.name, "unit": s.unit, "count": len(a), "min": a[0], "max": a[-1]}
                   for s, a in zip(tgt.axes, axes_vals)]
    res["points"] = size
    return done()


def _grid_keys(axes: list[list[float]]):
    if not axes:
        yield ()
        return
    for x in axes[0]:
        for rest in _grid_keys(axes[1:]):
            yield (x, *rest)


def _point(t: DataTarget, k: tuple) -> str:
    return ", ".join(f"{s.name} {v:g}" + (f" {s.unit}" if s.unit not in ("-", "") else "")
                     for s, v in zip(t.axes, k))
