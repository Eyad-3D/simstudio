"""HTTP routes for exporting results and importing tables and parameter
sheets (STD-09, STD-10, STD-36). Mounted by app.main.

  GET  /api/projects/{id}/runs/{run}/export?format=mat|csv|json
                               a stored run as a .mat file, CSV, or its run card
  POST /api/export/run?format=mat|csv|json
                               the same for a run sent in the body
  POST /api/import/table       read a table, map or profile from a CSV or .xlsx
                               file: the values, ready to apply, or why not
  POST /api/import/cycle       read a drive cycle of one's own (speed and grade
                               against time or distance) from a CSV or .xlsx file
  POST /api/params/export      every parameter of a project as .xlsx or CSV
  POST /api/params/import      the changes a parameter sheet would make
  GET  /api/params/template    the Formula Student example's parameter sheet
"""
from __future__ import annotations

import base64
import binascii
import gzip
from typing import Literal, Optional
from urllib.parse import quote

from fastapi import APIRouter, HTTPException, Query
from fastapi.responses import Response
from pydantic import BaseModel, Field, ValidationError

from .. import run_store, storage
from ..schemas import Project, StoredRun
from . import cycle_import, params, results, tables
from .sheets import MAX_BYTES, SheetError, read_file

router = APIRouter()

_ERROR = {"application/json": {"schema": {
    "type": "object", "properties": {"detail": {"type": "string"}}, "required": ["detail"]}}}

ExportFormat = Literal["mat", "csv", "json"]
_TYPES = {"mat": "application/x-matlab-data", "csv": "text/csv; charset=utf-8",
          "json": "application/json", "xlsx": "application/vnd.openxmlformats-officedocument."
                                              "spreadsheetml.sheet"}
_EXT = {"mat": ".mat", "csv": ".csv", "json": ".runcard.json", "xlsx": ".xlsx"}


def _binary(kinds: list[str], errors: dict[int, str]) -> dict:
    out: dict = {200: {"content": {_TYPES[k].split(";")[0]: {} for k in kinds},
                       "description": "The file"}}
    for status, text in errors.items():
        out[status] = {"description": text, "content": _ERROR}
    return out


def _download(data: bytes, kind: str, stem: str) -> Response:
    name = stem + _EXT[kind]
    ascii_name = name.encode("ascii", "replace").decode().replace("?", "_").replace('"', "_")
    return Response(data, media_type=_TYPES[kind], headers={
        "Content-Disposition": f"attachment; filename=\"{ascii_name}\"; "
                               f"filename*=UTF-8''{quote(name)}"})


def _export(run: StoredRun, format: str) -> Response:
    rt = results.table(run)
    stem = results.file_stem(run)
    if format == "mat":
        return _download(results.to_mat(rt), "mat", stem)
    if format == "csv":
        return _download(results.to_csv(rt), "csv", stem)
    return _download(results.run_card_json(rt), "json", stem)


@router.get("/api/projects/{project_id}/runs/{run_id}/export",
            responses=_binary(["mat", "csv", "json"],
                              {400: "Invalid id or unreadable run", 404: "No such run"}))
def export_stored_run(project_id: str, run_id: str,
                      format: ExportFormat = Query("mat")) -> Response:
    """A stored run as a MATLAB .mat file, CSV (UTF-8 with a byte-order
    mark), or its run card as JSON."""
    try:
        data = run_store.run_path(project_id, run_id).read_bytes()
        run = StoredRun.model_validate_json(gzip.decompress(data))
    except FileNotFoundError:
        raise HTTPException(status_code=404, detail=f"Run '{run_id}' not found")
    except (ValueError, OSError, EOFError, ValidationError) as e:
        raise HTTPException(status_code=400, detail=f"The run cannot be read: {e}"[:300])
    return _export(run, format)


@router.post("/api/export/run",
             responses=_binary(["mat", "csv", "json"], {400: "Unreadable request body"}))
def export_run(run: StoredRun, format: ExportFormat = Query("mat")) -> Response:
    """The run in the body as a .mat file, CSV, or its run card."""
    return _export(run, format)


def _decode(data: str) -> bytes:
    try:
        raw = base64.b64decode(data, validate=True)
    except (binascii.Error, ValueError):
        raise HTTPException(status_code=400, detail="The file's content is not valid base64.")
    if len(raw) > MAX_BYTES:
        raise HTTPException(status_code=400, detail="The file is too large.")
    return raw


class TableImportRequest(BaseModel):
    filename: str = Field(max_length=500)
    #: the file's bytes, base64
    data: str
    componentDefId: str = Field(max_length=200)
    paramKey: str = Field(max_length=200)
    #: the Road Profile's mode ("distance" or "time")
    mode: Optional[str] = Field(None, max_length=50)
    sheet: Optional[str] = Field(None, max_length=200)
    range: Optional[str] = Field(None, max_length=50)
    xColumn: Optional[int] = Field(None, ge=0, le=100000)
    yColumn: Optional[int] = Field(None, ge=0, le=100000)
    transpose: Optional[bool] = None
    units: Optional[dict[str, str]] = None
    #: a CSV file's decimal mark, when the user has chosen it
    decimal: Optional[Literal["comma", "point"]] = None


_PREVIEW_ROWS, _PREVIEW_COLS = 40, 16


@router.post("/api/import/table",
             responses={400: {"description": "Unreadable file or unknown parameter",
                              "content": _ERROR}})
def import_table(req: TableImportRequest) -> dict:
    """Read a table, 2-D map or drive profile for a part's parameter from a
    CSV or .xlsx file. Answers with the sheets in the file, the first cells
    of the one read, and the values in the parameter's units (``ok``) or the
    problems that stop the import, each with its row."""
    raw = _decode(req.data)
    try:
        target = tables.target_for(req.componentDefId, req.paramKey, req.mode)
    except KeyError:
        raise HTTPException(status_code=400,
                            detail=f"'{req.componentDefId}' has no parameter '{req.paramKey}'")
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))
    try:
        sheets = read_file(raw, req.filename, req.decimal)
    except SheetError as e:
        raise HTTPException(status_code=400, detail=str(e))
    sheet = next((s for s in sheets if s.name == req.sheet), None)
    if sheet is None:
        # the first sheet with numbers in it
        sheet = next((s for s in sheets if any(isinstance(c, float) for r in s.rows[:200]
                                                for c in r)), sheets[0])
    opts = req.model_dump(include={"range", "xColumn", "yColumn", "transpose", "units"},
                          exclude_none=True)
    res = tables.import_table(sheet, target, opts)
    cells = [[c for c in r[:_PREVIEW_COLS]] for r in sheet.rows[:_PREVIEW_ROWS]]
    return {**res.as_dict(), "sheets": [{"name": s.name, "rows": len(s.rows), "cols": s.width}
                                        for s in sheets],
            "cells": cells, "target": target.label}


class CycleImportRequest(BaseModel):
    filename: str = Field(max_length=500)
    #: the file's bytes, base64
    data: str
    #: what the cycle's points are against; found from the headers when absent
    axis: Optional[Literal["time", "distance"]] = None
    sheet: Optional[str] = Field(None, max_length=200)
    range: Optional[str] = Field(None, max_length=50)
    #: 0-based column indexes; -1 for none (speed against distance, grade)
    xColumn: Optional[int] = Field(None, ge=0, le=100000)
    speedColumn: Optional[int] = Field(None, ge=-1, le=100000)
    gradeColumn: Optional[int] = Field(None, ge=-1, le=100000)
    #: the unit to read each column in ("x", "speed", "grade")
    units: Optional[dict[str, str]] = None
    decimal: Optional[Literal["comma", "point"]] = None


@router.post("/api/import/cycle",
             responses={400: {"description": "Unreadable file", "content": _ERROR}})
def import_cycle(req: CycleImportRequest) -> dict:
    """Read a drive cycle of the user's own from a CSV or .xlsx file (CON-11):
    a speed, and optionally a road grade, against time, or a speed and/or a
    grade against distance, in km/h and %. Answers with the columns found,
    the units each was read in and the cycle (``ok``), or the problems that
    stop the import, each with its row. Nothing is stored: the app keeps the
    cycle in the project."""
    raw = _decode(req.data)
    try:
        sheets = read_file(raw, req.filename, req.decimal)
    except SheetError as e:
        raise HTTPException(status_code=400, detail=str(e))
    sheet = next((s for s in sheets if s.name == req.sheet), None)
    if sheet is None:
        sheet = next((s for s in sheets if any(isinstance(c, float) for r in s.rows[:200]
                                                for c in r)), sheets[0])
    opts = req.model_dump(include={"range", "axis", "xColumn", "speedColumn", "gradeColumn",
                                   "units"}, exclude_none=True)
    res = cycle_import.import_cycle(sheet, opts)
    cells = [[c for c in r[:_PREVIEW_COLS]] for r in sheet.rows[:_PREVIEW_ROWS]]
    return {**res.as_dict(), "sheets": [{"name": s.name, "rows": len(s.rows), "cols": s.width}
                                        for s in sheets],
            "cells": cells, "target": "drive cycle"}


class ParamsExportRequest(BaseModel):
    project: Project
    format: Literal["xlsx", "csv"] = "xlsx"


@router.post("/api/params/export",
             responses=_binary(["xlsx", "csv"], {400: "Unreadable request body"}))
def export_params(req: ParamsExportRequest) -> Response:
    """Every parameter of the project's parts: an .xlsx workbook (tables on
    sheets of their own) or one CSV sheet."""
    stem = f"{req.project.name or req.project.id} - parameters"
    stem = "".join("_" if ch in '<>:"/\\|?*' or ord(ch) < 32 else ch for ch in stem)[:150]
    if req.format == "csv":
        return _download(params.export_csv(req.project), "csv", stem)
    return _download(params.export_xlsx(req.project), "xlsx", stem)


class ParamsImportRequest(BaseModel):
    project: Project
    filename: str = Field(max_length=500)
    data: str


@router.post("/api/params/import",
             responses={400: {"description": "Unreadable file", "content": _ERROR}})
def import_params(req: ParamsImportRequest) -> dict:
    """The changes a parameter sheet would make to the project (nothing is
    changed here), or the rows that stop it, each with its row number."""
    raw = _decode(req.data)
    try:
        return params.import_sheet(req.project, raw, req.filename).as_dict()
    except SheetError as e:
        raise HTTPException(status_code=400, detail=str(e))


@router.get("/api/params/template", responses=_binary(["xlsx"], {}))
def params_template() -> Response:
    """The parameter sheet of the Formula Student example, as a template a
    team fills in with its own car's numbers."""
    project = storage.load_example("fs-electric")
    return _download(params.export_xlsx(project), "xlsx",
                     "LightSim Formula Student parameter template")
