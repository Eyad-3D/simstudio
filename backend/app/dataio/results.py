"""Export a run's results for MATLAB, Python and spreadsheets (STD-09).

One module turns a stored run into columns plus a *run card*: what made the
run (project, case, app version, model fingerprint, the parameters that
differ from the library's defaults, the live edits), how it ended (status,
messages, the figures its checks rule out) and the unit of every column.
The app, the command line and scripts all export through it, so a .mat file
and a CSV file of the same run hold the same numbers and details.

- :func:`to_mat`: a MATLAB .mat file (Level 5, read by MATLAB and by SciPy's
  ``loadmat``): one struct per part, holding ``t`` and that part's channels
  as column vectors, and a ``meta`` struct with the units and the run card.
- :func:`to_csv`: CSV that Excel opens as written (UTF-8 with a byte-order
  mark, quoted fields, units in the header); the run card goes next to it
  as JSON (:func:`run_card_json`).
"""
from __future__ import annotations

import json
import math
import time
from dataclasses import dataclass, field
from typing import Any, Optional

from ..library import library_by_id
from ..schemas import Project, StoredRun
from ..version import VERSION
from . import matfile
from .sheets import csv_bytes

RUN_CARD_FORMAT = "lightsim-run-card"
RUN_CARD_VERSION = 1


@dataclass
class Column:
    element_id: str
    element: str  # the part's label
    name: str  # the channel's name without the part ("SOC")
    label: str  # as the app shows it ("HV Battery Pack · SOC")
    unit: str
    port_id: str
    values: list[Optional[float]]
    t: list[float]
    #: names in the files, set by :func:`table`
    csv_header: str = ""
    mat_struct: str = ""
    mat_field: str = ""


@dataclass
class RunTable:
    columns: list[Column]
    card: dict[str, Any]
    #: the time of each row (the first channel's), s
    t: list[float] = field(default_factory=list)


def _iso(epoch_ms: int | float | None) -> Optional[str]:
    """The time as ISO 8601 UTC, or None when there is none or it is out of
    the range the platform's clock can show (a run file's startedAt is not
    bounded)."""
    if not epoch_ms:
        return None
    try:
        return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(epoch_ms / 1000))
    except (OverflowError, OSError, ValueError):
        return None


def _elements(project: Optional[Project]) -> dict[str, Any]:
    if project is None:
        return {}
    return {e.id: e for s in project.systems for e in s.elements}


def _value_text(v: Any) -> Any:
    """A parameter value for the run card: numbers, text and true/false as
    they are; a table as "table (n points)"."""
    if isinstance(v, dict):
        n = sum(len(x) if isinstance(x, dict) else 1 for x in v.values())
        return f"table ({n} points)"
    return v


def changed_parameters(project: Optional[Project], case_overrides: dict) -> list[dict]:
    """The parameters of the run's model that differ from the library's
    defaults: set on the part, or by the case (the case's value is the one
    that ran)."""
    if project is None:
        return []
    lib = library_by_id()
    out: list[dict] = []
    for el in _elements(project).values():
        cdef = lib.get(el.componentDefId)
        params = {p.key: p for p in cdef.parameters} if cdef else {}
        merged = dict(el.parameterOverrides)
        from_case = case_overrides.get(el.id, {}) or {}
        merged.update(from_case)
        for key, value in merged.items():
            p = params.get(key)
            if p is not None and p.type == "code":
                where = "case" if key in from_case else "part"
                out.append({"element": el.label, "elementId": el.id, "parameter": p.label,
                            "key": key, "value": "(script)", "unit": "-", "set": where})
                continue
            if p is not None and json.dumps(p.default, sort_keys=True) == json.dumps(
                    value, sort_keys=True):
                continue
            out.append({
                "element": el.label, "elementId": el.id,
                "parameter": p.label if p else key, "key": key,
                "value": _value_text(value), "unit": p.unit if p else "",
                "set": "case" if key in from_case else "part",
            })
    return out


def run_card(run: StoredRun) -> dict[str, Any]:
    """Everything about the run except its channel data, as plain JSON."""
    snap = run.snapshot
    project = snap.project if snap else None
    case = snap.case if snap else None
    elements = _elements(project)
    lib = library_by_id()

    def param_label(element_id: str, key: str) -> tuple[str, str]:
        el = elements.get(element_id)
        cdef = lib.get(el.componentDefId) if el else None
        p = next((q for q in cdef.parameters if q.key == key), None) if cdef else None
        return (p.label, p.unit) if p else (key, "")

    live = []
    for e in (snap.liveEdits if snap else []):
        label, unit = param_label(e.elementId, e.key)
        el = elements.get(e.elementId)
        live.append({"t": e.t, "element": el.label if el else e.elementId,
                     "elementId": e.elementId, "parameter": label, "key": e.key,
                     "value": e.value, "unit": unit})
    summary = [s.model_dump(exclude_none=True) for s in run.result.summary]
    return {
        "format": RUN_CARD_FORMAT,
        "formatVersion": RUN_CARD_VERSION,
        "project": {"id": project.id, "name": project.name} if project else None,
        "case": ({"id": case.id, "name": case.name, "kind": case.kind,
                  "duration_s": case.duration, "timeStep_s": case.timeStep}
                 if case else {"id": run.caseId, "name": run.caseName}),
        "run": {
            "id": run.id, "name": run.name, "note": run.note,
            "startedAt": _iso(run.startedAt), "status": run.status,
            "incomplete": run.incomplete,
            "sweep": ({"parameter": run.sweepParam, "value": run.sweepValue,
                       "unit": run.sweepUnit} if run.sweepId else None),
        },
        "appVersion": snap.appVersion if snap else None,
        "exportedWith": VERSION,
        "modelHash": snap.modelHash if snap else None,
        "changedParameters": changed_parameters(
            project, case.parameterOverrides if case else {}),
        "liveEdits": live,
        "summary": summary,
        "notValid": [{"label": s.label, "why": s.notValid}
                     for s in run.result.summary if s.notValid],
        "messages": [m.model_dump() for m in run.result.messages],
    }


def table(run: StoredRun) -> RunTable:
    """The run as columns (one per channel, in the run's order) and its run
    card, with each column's name in the CSV and .mat files."""
    elements = _elements(run.snapshot.project if run.snapshot else None)
    columns: list[Column] = []
    for ch in run.result.channels:
        el = elements.get(ch.elementId)
        part, _, name = ch.label.partition(" · ")
        if not name:
            part, name = (el.label if el else ch.elementId), ch.label
        columns.append(Column(
            element_id=ch.elementId, element=el.label if el else part, name=name,
            label=ch.label, unit=ch.unit, port_id=ch.portId,
            values=[p.get("value") for p in ch.timeSeries],
            t=[float(p.get("t") or 0.0) for p in ch.timeSeries]))
    # names in the files: a struct per part, a field per channel
    structs: dict[str, str] = {}
    taken_structs = {"meta", "t"}
    fields: dict[str, set[str]] = {}
    for c in columns:
        if c.element_id not in structs:
            structs[c.element_id] = matfile.identifier(c.element, taken_structs, prefix="part")
            fields[c.element_id] = {"t"}
        c.mat_struct = structs[c.element_id]
        c.mat_field = matfile.identifier(c.name, fields[c.element_id], prefix="signal")
        c.csv_header = f"{c.label} [{c.unit}]"
    card = run_card(run)
    card["channels"] = [
        {"element": c.element, "elementId": c.element_id, "port": c.port_id, "label": c.label,
         "unit": c.unit, "csvColumn": c.csv_header, "matStruct": c.mat_struct,
         "matField": c.mat_field}
        for c in columns]
    return RunTable(columns=columns, card=card, t=columns[0].t if columns else [])


def _clean(v: Optional[float]) -> Optional[float]:
    return None if v is None or not math.isfinite(v) else v


def run_card_json(rt: RunTable) -> bytes:
    return (json.dumps(rt.card, indent=2, ensure_ascii=False) + "\n").encode("utf-8")


def to_csv(rt: RunTable) -> bytes:
    """Every channel along time: the first column t [s], then one column per
    channel headed "Part · Channel [unit]"."""
    n = max((len(c.values) for c in rt.columns), default=0)
    t = rt.t if len(rt.t) == n else next((c.t for c in rt.columns if len(c.t) == n), [])
    rows: list[list[Any]] = [["t [s]", *(c.csv_header for c in rt.columns)]]
    for i in range(n):
        rows.append([_clean(t[i]), *(_clean(c.values[i]) if i < len(c.values) else None
                                     for c in rt.columns)])
    return csv_bytes(rows)


def _structs(items: list[dict]) -> matfile.StructArray | list:
    """A list of flat dicts as a struct array with MATLAB-safe field names."""
    if not items:
        return []
    keys: list[str] = []
    for it in items:
        for k in it:
            if k not in keys:
                keys.append(k)
    names = {k: matfile.identifier(k) for k in keys}
    return matfile.StructArray([
        {names[k]: _mat_value(it.get(k)) for k in keys} for it in items])


def _mat_value(v: Any) -> Any:
    if v is None:
        return ""
    if isinstance(v, (dict, list)):
        return json.dumps(v, ensure_ascii=False)
    return v


def to_mat(rt: RunTable) -> bytes:
    """A MATLAB .mat file: a struct per part (``t`` and its channels as
    column vectors, NaN where a channel has no value) and ``meta``."""
    variables: dict[str, Any] = {}
    units: dict[str, dict[str, str]] = {}
    labels: dict[str, dict[str, str]] = {}
    parts: dict[str, str] = {}
    for c in rt.columns:
        s = variables.setdefault(c.mat_struct, {"t": [_clean(x) for x in c.t]})
        if len(c.t) != len(s["t"]):
            # a channel on its own time base keeps it next to it (cut so
            # that a long name keeps its "_t" and does not replace the channel)
            t_name = matfile.identifier(c.mat_field[: matfile.MAX_NAME - 2] + "_t",
                                        {k.lower() for k in s} | {c.mat_field.lower()})
            s[t_name] = [_clean(x) for x in c.t]
        s[c.mat_field] = [_clean(v) for v in c.values]
        units.setdefault(c.mat_struct, {"t": "s"})[c.mat_field] = c.unit
        labels.setdefault(c.mat_struct, {"t": "Time"})[c.mat_field] = c.label
        parts[c.mat_struct] = c.element
    card = rt.card
    run = card["run"]
    meta: dict[str, Any] = {
        "format": RUN_CARD_FORMAT,
        "format_version": RUN_CARD_VERSION,
        "project": (card["project"] or {}).get("name", ""),
        "project_id": (card["project"] or {}).get("id", ""),
        "case_name": card["case"].get("name", ""),
        "case_id": card["case"].get("id", ""),
        "case_kind": card["case"].get("kind", ""),
        "run_id": run["id"],
        "run_name": run.get("name") or "",
        "run_note": run.get("note") or "",
        "started": run.get("startedAt") or "",
        "status": run["status"],
        "incomplete": run.get("incomplete") or "",
        "app_version": card.get("appVersion") or "",
        "exported_with": card["exportedWith"],
        "model_hash": card.get("modelHash") or "",
        "parts": parts,
        "units": units,
        "labels": labels,
        "summary": _structs(card["summary"]),
        "changed_parameters": _structs(card["changedParameters"]),
        "live_edits": _structs(card["liveEdits"]),
        "not_valid": [f"{x['label']}: {x['why']}" for x in card["notValid"]],
        "messages": [f"{m['level']}: {m['text']}" for m in card["messages"]],
        "run_card_json": json.dumps(card, ensure_ascii=False),
    }
    if not meta["not_valid"]:
        meta["not_valid"] = matfile.Cell([])
    if not meta["messages"]:
        meta["messages"] = matfile.Cell([])
    out = {"meta": meta, **variables}
    title = (f"MATLAB 5.0 MAT-file, LightSim {VERSION} results: "
             f"{(card['project'] or {}).get('name', '')} / {card['case'].get('name', '')}")
    return matfile.mat_bytes(out, description=title)


def file_stem(run: StoredRun) -> str:
    """A file name for the run's export, without extension:
    "<project> - <case> - <run name or start time>"."""
    snap = run.snapshot
    parts = [snap.project.name if snap else "", run.caseName,
             run.name or (_iso(run.startedAt) or run.id).replace(":", "-")]
    stem = " - ".join(p for p in parts if p)
    bad = '<>:"/\\|?*'
    return "".join("_" if ch in bad or ord(ch) < 32 else ch for ch in stem)[:150] or "run"
