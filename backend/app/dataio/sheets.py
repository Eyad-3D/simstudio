"""Read CSV and Excel (.xlsx) files into plain grids of cells, and write them.

Every importer (tables, maps and drive profiles, STD-10; the parameter
sheet, STD-36; logged laps, STD-35) starts from the same thing: a list of
sheets, each a list of rows, each cell a number, a text or empty. This
module turns a file into that, whatever its delimiter, decimal mark or
encoding, so the importers only interpret cells.

The .xlsx reader and writer are small and built in (the zip and XML parts
of Python's standard library), so the engine needs no extra package: they
read cell values (numbers, text, true/false, the cached result of a
formula) and ignore formatting, charts and pictures.
"""
from __future__ import annotations

import csv
import io
import math
import re
import zipfile
from dataclasses import dataclass, field
from typing import Iterable, Optional, Union
from xml.etree import ElementTree as ET
from xml.sax.saxutils import escape

Cell = Union[float, str, None]

#: Largest file an import reads, bytes (a 20x30 map is a few kB; a 10 Hz
#: logged endurance run of 20 channels is about 10 MB as CSV).
MAX_BYTES = 50 * 1024 * 1024
#: Largest number of cells read from one file (all sheets together).
MAX_CELLS = 5_000_000
#: Largest uncompressed size of an .xlsx file's parts, bytes.
MAX_XLSX_UNPACKED = 300 * 1024 * 1024


class SheetError(ValueError):
    """The file cannot be read as a table; the message says why, in words a
    user can act on."""


@dataclass
class Sheet:
    name: str
    rows: list[list[Cell]] = field(default_factory=list)
    #: how the file was read, for the preview ("semicolon-separated, decimal comma")
    notes: list[str] = field(default_factory=list)
    #: a CSV file's decimal mark as read: "comma" or "point" (None for .xlsx)
    decimal: Optional[str] = None
    #: set when the cells do not show the decimal mark (1,000 can be 1 or
    #: 1000): what was assumed, to ask the user
    question: Optional[str] = None

    @property
    def width(self) -> int:
        return max((len(r) for r in self.rows), default=0)


def column_letter(index: int) -> str:
    """0 -> "A", 25 -> "Z", 26 -> "AA" (spreadsheet column names)."""
    s = ""
    n = index + 1
    while n:
        n, rem = divmod(n - 1, 26)
        s = chr(65 + rem) + s
    return s


def cell_name(row: int, col: int) -> str:
    """The spreadsheet name of a 0-based cell: (0, 0) -> "A1"."""
    return f"{column_letter(col)}{row + 1}"


# ---- reading -----------------------------------------------------------------

def read_file(data: bytes, filename: str, decimal: Optional[str] = None) -> list[Sheet]:
    """The sheets of a CSV, TSV or .xlsx file (by its name, or its content
    when the name says neither). ``decimal`` ("comma" or "point") is a CSV
    file's decimal mark when the user has chosen it; by default it is read
    from the cells."""
    if len(data) > MAX_BYTES:
        raise SheetError(f"The file is {len(data) / 1e6:.0f} MB; LightSim reads files up to "
                         f"{MAX_BYTES / 1e6:.0f} MB.")
    name = filename.lower()
    if name.endswith((".xlsx", ".xlsm")) or data[:4] == b"PK\x03\x04":
        return read_xlsx(data)
    if name.endswith(".xls"):
        raise SheetError("This is an old Excel 97-2003 file (.xls). Open it in Excel and save it "
                         "as .xlsx or CSV, then import that.")
    if name.endswith(".ods"):
        raise SheetError("OpenDocument spreadsheets (.ods) cannot be read yet. Save the sheet "
                         "as .xlsx or CSV, then import that.")
    return [read_csv(data, sheet_name=filename.rsplit("/", 1)[-1] or "CSV", decimal=decimal)]


def decode_text(data: bytes) -> tuple[str, str]:
    """(text, encoding): UTF-8 or UTF-16 with a byte-order mark, UTF-8, or
    Windows-1252 (what Excel on Windows writes for "CSV")."""
    if data.startswith(b"\xef\xbb\xbf"):
        return data[3:].decode("utf-8", errors="replace"), "UTF-8"
    if data.startswith((b"\xff\xfe", b"\xfe\xff")):
        return data.decode("utf-16", errors="replace"), "UTF-16"
    if b"\x00" in data[:4096]:
        raise SheetError("This does not look like a text (CSV) file or an .xlsx workbook.")
    try:
        return data.decode("utf-8"), "UTF-8"
    except UnicodeDecodeError:
        return data.decode("cp1252", errors="replace"), "Windows-1252"


_DELIMITERS = {",": "comma", ";": "semicolon", "\t": "tab", "|": "vertical bar"}
_DECIMAL_COMMA = re.compile(r"^[-+]?\d+,\d+(?:[eE][-+]?\d+)?$")
_DECIMAL_POINT = re.compile(r"^[-+]?\d*\.\d+(?:[eE][-+]?\d+)?$")
# digits in groups of three with a dot between (and a decimal comma after):
# 1.000 or 1.234.567,5, as German or French Excel formats a large number
_DOT_THOUSANDS = re.compile(r"^[-+]?[1-9]\d{0,2}(?:\.\d{3})+(?:,\d+)?$")
# the same with commas (and a decimal point after): 1,000 or 1,234,567.5
_COMMA_THOUSANDS = re.compile(r"^[-+]?[1-9]\d{0,2}(?:,\d{3})+(?:\.\d+)?$")
# 1,000 or 1.000 alone cannot say its decimal mark: 1 or 1000
_AMBIGUOUS = re.compile(r"^[-+]?[1-9]\d{0,2}[.,]\d{3}$")


def _sniff_delimiter(lines: list[str]) -> str:
    """The delimiter that splits the first lines into the most rows of the
    same width (more than one column)."""
    best, best_score = ",", (-1, 0)
    for d in _DELIMITERS:
        widths = [len(r) for r in csv.reader(lines, delimiter=d) if r]
        if not widths:
            continue
        common = max(set(widths), key=widths.count)
        if common < 2:
            continue
        score = (widths.count(common), common)
        if score > best_score:
            best, best_score = d, score
    return best


def parse_number(text: str, decimal_comma: bool = False) -> Optional[float]:
    """The number in a cell's text, or None when it is not a finite number
    ("1.5", "-2e3", " 42 "; "1,5" and "1.000" (= 1000) with a decimal comma;
    "1,000" (= 1000) with a decimal point)."""
    t = text.strip()
    if not t:
        return None
    if decimal_comma:
        if _DOT_THOUSANDS.match(t):
            t = t.replace(".", "").replace(",", ".")
        elif _DECIMAL_COMMA.match(t):
            t = t.replace(",", ".")
    elif _COMMA_THOUSANDS.match(t):
        t = t.replace(",", "")
    try:
        v = float(t)
    except ValueError:
        return None
    return v if math.isfinite(v) else None


def _decimal_mark(raw: list[list[str]], delim: str) -> tuple[bool, Optional[str]]:
    """(decimal comma?, question) for a CSV file's cells. A cell such as 12,5
    or 1.234,5 shows a decimal comma, and 12.5 or 1,234.5 a decimal point.
    1,000 and 1.000 show neither (1 or 1000): when only such cells are found,
    a semicolon-separated file is read with a decimal comma and any other
    with a decimal point, and the question says so."""
    comma = point = unsure = None
    for i, r in enumerate(raw[:200]):
        for j, c in enumerate(r):
            t = c.strip()
            if not t or not (t[0].isdigit() or t[0] in "+-."):
                continue
            if _AMBIGUOUS.match(t):
                # in a comma-separated file 1.000 has a decimal point: a
                # decimal comma there needs every number in quotes
                if delim == "," and "." in t:
                    point = point or (i, j, t)
                else:
                    unsure = unsure or (i, j, t)
            elif _DECIMAL_COMMA.match(t) or _DOT_THOUSANDS.match(t):
                comma = comma or (i, j, t)
            elif _DECIMAL_POINT.match(t) or _COMMA_THOUSANDS.match(t):
                point = point or (i, j, t)
    if comma and not point:
        return True, None
    if point and not comma:
        return False, None
    if comma and point:  # a mixed file: as before, by the delimiter
        decimal_comma = delim != ","
    elif unsure:  # as the delimiter suggests
        decimal_comma = delim == ";"
    else:  # whole numbers only
        return False, None
    if unsure is None:
        return decimal_comma, None
    i, j, t = unsure
    as_comma, as_point = parse_number(t, True), parse_number(t, False)
    used = "a decimal comma" if decimal_comma else "a decimal point"
    read_as = as_comma if decimal_comma else as_point
    return decimal_comma, (
        f"Cell {cell_name(i, j)} holds {t}, which is {as_comma:g} with a decimal comma and "
        f"{as_point:g} with a decimal point; the file does not show which it uses. It was "
        f"read with {used} ({t} = {read_as:g}).")


def read_csv(data: bytes, sheet_name: str = "CSV", decimal: Optional[str] = None) -> Sheet:
    """A CSV or TSV file as one sheet: any of the delimiters , ; tab |, a
    decimal comma (1,5 = 1.5, with 1.000 = 1000) or point (with 1,000 =
    1000) as the cells show (:func:`_decimal_mark`) or ``decimal`` says
    ("comma" or "point"), and the encodings of :func:`decode_text`."""
    text, encoding = decode_text(data)
    lines = text.splitlines()
    sample = [ln for ln in lines[:50] if ln.strip()]
    if not sample:
        raise SheetError("The file is empty.")
    delim = _sniff_delimiter(sample)
    raw = list(csv.reader(io.StringIO(text), delimiter=delim))
    sheet = Sheet(sheet_name)
    if decimal in ("comma", "point"):
        decimal_comma = decimal == "comma"
    else:
        decimal_comma, sheet.question = _decimal_mark(raw, delim)
    sheet.decimal = "comma" if decimal_comma else "point"
    grouped = _DOT_THOUSANDS if decimal_comma else _COMMA_THOUSANDS
    thousands = False
    cells = 0
    for r in raw:
        cells += len(r)
        if cells > MAX_CELLS:
            raise SheetError(f"The file has more than {MAX_CELLS:,} cells.")
        row: list[Cell] = []
        for c in r:
            t = c.strip()
            if t == "":
                row.append(None)
                continue
            n = parse_number(t, decimal_comma)
            if n is None:
                row.append(t)
                continue
            row.append(n)
            if not thousands and grouped.match(t):
                thousands = True
        sheet.rows.append(row)
    _trim(sheet)
    sheet.notes.append(
        f"{_DELIMITERS[delim]}-separated, {encoding}"
        + (", decimal comma" if decimal_comma else "")
        + ((", dots between thousands (1.000 = 1000)" if decimal_comma else
            ", commas between thousands (1,000 = 1000)") if thousands else ""))
    return sheet


def _trim(sheet: Sheet) -> None:
    """Drop empty cells at the end of each row and empty rows at the end."""
    for r in sheet.rows:
        while r and r[-1] is None:
            r.pop()
    while sheet.rows and not sheet.rows[-1]:
        sheet.rows.pop()


_NS = {"m": "http://schemas.openxmlformats.org/spreadsheetml/2006/main"}
_REL_NS = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
_PKG_REL = "{http://schemas.openxmlformats.org/package/2006/relationships}Relationship"
_CELL_REF = re.compile(r"^([A-Z]+)(\d+)$")


def _xml(zf: zipfile.ZipFile, name: str) -> ET.Element:
    data = zf.read(name)
    # an Excel part never declares a DTD; refusing one rules out entity tricks
    if b"<!DOCTYPE" in data[:4096].upper():
        raise SheetError("The workbook contains a part LightSim does not read (a DTD).")
    return ET.fromstring(data)


def _col_index(letters: str) -> int:
    n = 0
    for ch in letters:
        n = n * 26 + ord(ch) - 64
    return n - 1


def read_xlsx(data: bytes) -> list[Sheet]:
    """The worksheets of an .xlsx workbook, in their tab order."""
    try:
        zf = zipfile.ZipFile(io.BytesIO(data))
    except zipfile.BadZipFile:
        raise SheetError("The file is not a valid .xlsx workbook (it could not be unzipped).")
    with zf:
        if sum(i.file_size for i in zf.infolist()) > MAX_XLSX_UNPACKED:
            raise SheetError("The workbook is too large to read.")
        names = set(zf.namelist())
        if "xl/workbook.xml" not in names:
            raise SheetError("The file is not an Excel .xlsx workbook (no xl/workbook.xml). "
                             "Save it from Excel as 'Excel Workbook (.xlsx)'.")
        try:
            shared = _shared_strings(zf) if "xl/sharedStrings.xml" in names else []
            targets = {}
            if "xl/_rels/workbook.xml.rels" in names:
                for rel in _xml(zf, "xl/_rels/workbook.xml.rels").iter(_PKG_REL):
                    target = rel.get("Target", "")
                    target = target.lstrip("/") if target.startswith("/") else "xl/" + target
                    targets[rel.get("Id")] = target
            sheets: list[Sheet] = []
            cells = [0]
            book = _xml(zf, "xl/workbook.xml")
            for i, s in enumerate(book.iterfind("m:sheets/m:sheet", _NS)):
                rid = s.get(f"{{{_REL_NS}}}id")
                path = targets.get(rid) or f"xl/worksheets/sheet{i + 1}.xml"
                if path not in names:
                    continue
                sheet = Sheet(s.get("name") or f"Sheet{i + 1}")
                sheet.rows = _sheet_rows(zf.read(path), shared, cells)
                _trim(sheet)
                sheets.append(sheet)
        except (ET.ParseError, KeyError, ValueError) as e:
            if isinstance(e, SheetError):
                raise
            raise SheetError(f"The workbook could not be read ({type(e).__name__}).")
    if not sheets:
        raise SheetError("The workbook has no worksheets.")
    return sheets


def _shared_strings(zf: zipfile.ZipFile) -> list[str]:
    root = _xml(zf, "xl/sharedStrings.xml")
    out = []
    for si in root.iterfind("m:si", _NS):
        # plain text, or rich text in runs: join every <t>
        out.append("".join(t.text or "" for t in si.iter(f"{{{_NS['m']}}}t")))
    return out


def _sheet_rows(data: bytes, shared: list[str], count: list[int]) -> list[list[Cell]]:
    if b"<!DOCTYPE" in data[:4096].upper():
        raise SheetError("The workbook contains a part LightSim does not read (a DTD).")
    rows: dict[int, dict[int, Cell]] = {}
    m = f"{{{_NS['m']}}}"
    next_row = 0
    for _, el in ET.iterparse(io.BytesIO(data)):
        if el.tag != m + "row":
            continue
        r_attr = el.get("r")
        r = int(r_attr) - 1 if r_attr else next_row
        next_row = r + 1
        row: dict[int, Cell] = {}
        next_col = 0
        for c in el.iterfind(m + "c"):
            ref = c.get("r")
            match = _CELL_REF.match(ref) if ref else None
            col = _col_index(match.group(1)) if match else next_col
            next_col = col + 1
            value = _cell_value(c, shared, m)
            if value is not None:
                row[col] = value
                count[0] += 1
                if count[0] > MAX_CELLS:
                    raise SheetError(f"The workbook has more than {MAX_CELLS:,} cells.")
        if row:
            rows[r] = row
        el.clear()
    if not rows:
        return []
    height = max(rows) + 1
    out: list[list[Cell]] = []
    for r in range(height):
        row = rows.get(r, {})
        width = max(row) + 1 if row else 0
        out.append([row.get(c) for c in range(width)])
    return out


def _cell_value(c: ET.Element, shared: list[str], m: str) -> Cell:
    t = c.get("t", "n")
    if t == "inlineStr":
        node = c.find(m + "is")
        text = "".join(x.text or "" for x in node.iter(m + "t")) if node is not None else ""
        return text if text.strip() else None
    v = c.find(m + "v")
    raw = v.text if v is not None else None
    if raw is None:
        return None
    if t == "s":
        idx = int(raw)
        text = shared[idx] if 0 <= idx < len(shared) else ""
        return text if text.strip() else None
    if t == "b":
        return "TRUE" if raw.strip() == "1" else "FALSE"
    if t in ("str", "e", "d"):
        return raw if raw.strip() else None
    n = parse_number(raw)
    return n if n is not None else raw


# ---- writing -----------------------------------------------------------------

def csv_bytes(rows: Iterable[Iterable[object]]) -> bytes:
    """Rows as CSV that Excel opens correctly: UTF-8 with a byte-order mark
    (so "N·m" and "°C" show as written), every field that holds a comma,
    a quote or a line break in double quotes (RFC 4180), CRLF line ends."""
    buf = io.StringIO()
    w = csv.writer(buf, lineterminator="\r\n")
    for r in rows:
        w.writerow(["" if v is None else _csv_text(v) for v in r])
    return b"\xef\xbb\xbf" + buf.getvalue().encode("utf-8")


def _csv_text(v: object) -> object:
    if isinstance(v, float):
        if not math.isfinite(v):
            return ""
        return repr(v) if v != int(v) or abs(v) >= 1e16 else str(int(v))
    return v


@dataclass
class OutSheet:
    """A worksheet to write: rows of cells, the first `header_rows` bold
    and frozen, and optional column widths in characters."""

    name: str
    rows: list[list[object]]
    header_rows: int = 1
    widths: list[float] = field(default_factory=list)


_BAD_SHEET_CHARS = re.compile(r"[\[\]:*?/\\]")


def sheet_title(name: str, taken: set[str]) -> str:
    """A valid, unique worksheet name (at most 31 characters, none of []:*?/\\)."""
    base = _BAD_SHEET_CHARS.sub("_", name).strip("'") or "Sheet"
    base = base[:31].rstrip(" -")
    title, n = base, 2
    while title.lower() in taken:
        suffix = f" ({n})"
        title = base[: 31 - len(suffix)] + suffix
        n += 1
    taken.add(title.lower())
    return title


def xlsx_bytes(sheets: list[OutSheet]) -> bytes:
    """A minimal .xlsx workbook (Office Open XML) that Excel, LibreOffice
    and Google Sheets open: values only, with bold, frozen header rows."""
    buf = io.BytesIO()
    taken: set[str] = set()
    titles = [sheet_title(s.name, taken) for s in sheets]
    with zipfile.ZipFile(buf, "w", zipfile.ZIP_DEFLATED) as zf:
        zf.writestr("[Content_Types].xml", _content_types(len(sheets)))
        zf.writestr("_rels/.rels", (
            '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
            '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
            '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/'
            'relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>'))
        sheet_tags = "".join(
            f'<sheet name="{escape(t, {chr(34): "&quot;"})}" sheetId="{i + 1}" r:id="rId{i + 1}"/>'
            for i, t in enumerate(titles))
        zf.writestr("xl/workbook.xml", (
            '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
            '<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" '
            'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">'
            f'<sheets>{sheet_tags}</sheets></workbook>'))
        rels = "".join(
            f'<Relationship Id="rId{i + 1}" Type="http://schemas.openxmlformats.org/'
            f'officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{i + 1}.xml"/>'
            for i in range(len(sheets)))
        rels += (f'<Relationship Id="rId{len(sheets) + 1}" Type="http://schemas.openxmlformats.org/'
                 'officeDocument/2006/relationships/styles" Target="styles.xml"/>')
        zf.writestr("xl/_rels/workbook.xml.rels", (
            '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
            '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
            f'{rels}</Relationships>'))
        zf.writestr("xl/styles.xml", _STYLES)
        for i, s in enumerate(sheets):
            zf.writestr(f"xl/worksheets/sheet{i + 1}.xml", _sheet_xml(s))
    return buf.getvalue()


def _content_types(n: int) -> str:
    overrides = "".join(
        f'<Override PartName="/xl/worksheets/sheet{i + 1}.xml" ContentType="application/'
        'vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>' for i in range(n))
    return (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
        '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
        '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.'
        'relationships+xml"/><Default Extension="xml" ContentType="application/xml"/>'
        '<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-'
        'officedocument.spreadsheetml.sheet.main+xml"/>'
        '<Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-'
        f'officedocument.spreadsheetml.styles+xml"/>{overrides}</Types>')


_STYLES = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
    '<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">'
    '<fonts count="2"><font><sz val="11"/><name val="Calibri"/></font>'
    '<font><b/><sz val="11"/><name val="Calibri"/></font></fonts>'
    '<fills count="2"><fill><patternFill patternType="none"/></fill>'
    '<fill><patternFill patternType="gray125"/></fill></fills>'
    '<borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders>'
    '<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>'
    '<cellXfs count="2"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/>'
    '<xf numFmtId="0" fontId="1" fillId="0" borderId="0" xfId="0" applyFont="1"/></cellXfs>'
    '<cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles>'
    '</styleSheet>')

# characters XML 1.0 cannot hold
_XML_BAD = re.compile("[\x00-\x08\x0b\x0c\x0e-\x1f]")


def _sheet_xml(s: OutSheet) -> str:
    parts = ['<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
             '<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">']
    if s.header_rows:
        parts.append('<sheetViews><sheetView workbookViewId="0"><pane ySplit="'
                     f'{s.header_rows}" topLeftCell="A{s.header_rows + 1}" activePane="bottomLeft" '
                     'state="frozen"/></sheetView></sheetViews>')
    if s.widths:
        parts.append("<cols>" + "".join(
            f'<col min="{i + 1}" max="{i + 1}" width="{w:g}" customWidth="1"/>'
            for i, w in enumerate(s.widths)) + "</cols>")
    parts.append("<sheetData>")
    for r, row in enumerate(s.rows):
        style = ' s="1"' if r < s.header_rows else ""
        cells = []
        for c, v in enumerate(row):
            if v is None or v == "":
                continue
            ref = cell_name(r, c)
            if isinstance(v, bool):
                cells.append(f'<c r="{ref}"{style} t="b"><v>{int(v)}</v></c>')
            elif isinstance(v, (int, float)):
                if not math.isfinite(v):
                    continue
                cells.append(f'<c r="{ref}"{style}><v>{_num(v)}</v></c>')
            else:
                text = escape(_XML_BAD.sub("", str(v)))
                cells.append(f'<c r="{ref}"{style} t="inlineStr"><is><t xml:space="preserve">'
                             f'{text}</t></is></c>')
        parts.append(f'<row r="{r + 1}">{"".join(cells)}</row>')
    parts.append("</sheetData></worksheet>")
    return "".join(parts)


def _num(v: float) -> str:
    if isinstance(v, int) or (isinstance(v, float) and v.is_integer() and abs(v) < 1e15):
        return str(int(v))
    return repr(float(v))
