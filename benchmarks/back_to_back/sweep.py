"""The mismatch sweep (the owner, 6 November): faults built on purpose and
run on both engines at once, instead of one new case per check.

The faults, from the four example cars and the judge's 16 cases
(acceptance.toml's items that are not probes):

- table: every table of every part the new engine reads
  (lightsim_engine.table_sites, new_engine.BRANCH_READS) cut short at each
  end of each axis, by one point ("edge") and to its inner half ("half"),
  with that axis's out-of-range setting on each of Error, Clamp and Linear;
  a Driving Task's or Road Profile's profile (no setting) cut short at each
  end;
On each of the judge's three fault bases and each live case (live_bases,
added 7 November: a live run the new engine fails is not run again, so only
a live case's own run shows it):

- alone: one part of every library component placed on the car and left
  connected to nothing (no bus, no shaft, no signal), with its defaults, and
  each table the engine reads of it cut as above;
- sampled: a PID, a 1D Lookup and a 2D Lookup fed the car's speed, with a
  Sample Time of 0, 5 ms and 15 ms (today's solver step is 10 ms), half, one
  and three of the case's output steps, and 5 s (the first three added
  7 November).

Each fault runs three times (a live case a fourth: the new path not strict,
with a view, as the app runs it; compare() counts a live run it fails where
today's does not) through the app's own app.solver.simulate:
today's engine (no flag), the new path in strict mode (LIGHTSIM_ENGINE=new,
LIGHTSIM_ENGINE_STRICT=1) and quick mode in strict mode (the case's mode
"quick"). A strict run refused with a listed code is a run handed back to
today's engine (counted by code, never a mismatch); any other is compared
with today's run (compare(): acceptance.toml's [sweep] says what counts).

    python -m benchmarks.back_to_back sweep OUT [--workers N] [--only SUBSTRING]

writes OUT/raw/<fault>.json (resumed: a record of this same code is not run
again) and OUT/sweep.json, OUT/sweep.md with the counts and every mismatch.
"""
from __future__ import annotations

import json
import math
import os
import re
import subprocess
import sys
import time
from concurrent.futures import ThreadPoolExecutor
from functools import lru_cache
from pathlib import Path

from . import runs
from .diff import masked

POLICIES = ("error", "clamp", "linear")
DEPTHS = ("edge", "half")
ENDS = ("lo", "hi")
ALONE_BASES = tuple(f"{n}/{c}" for n, c in runs.FAULT_EXAMPLES.items())  # the judge's fault bases
# (the case's output step is 1 s on a cycle, over today's 10 ms solver step: "0", "5ms" and
# "15ms" are at and around that step, so the blocks the new path runs are compared on every base)
SAMPLE_TIMES = ("0", "5ms", "15ms", "half-step", "step", "3-steps", "5s")
SPARE = "x-spare"  # the id of a fault's added part
CODE = re.compile(r"\[((?:NEW|QUICK|FAST)-[A-Z0-9-]+)\]")
REFUSED = re.compile(r"Not run \[([A-Z0-9-]+)\]")
FLAG = "Quick mode: "  # the start of quick mode's own messages
# today's speed trace verdict (verdict.py): quick mode prescribes the speed,
# so its flags stand in for it (acceptance.toml [sweep])
TRACE = "outside the WLTP"
OUTPUT_BAND = 0.02  # a sampled block's outputs: within 2 % of their range on today's run


# ---- the cases and the library -------------------------------------------------

def bases() -> list[str]:
    """The judge's 16 cases: its items that are not probes nor changed cases."""
    return [i["id"] for i in runs.items() if not i["id"].startswith("probe/") and "base" not in i]


def base_project(base: str) -> tuple[dict, str]:
    """A base case's project as the judge runs it: a live case paced as the
    judge paces it (and run live: run_fault), a hybrid's case as a single
    run (as the judge's faults)."""
    name, case_id = base.split("/")
    project = runs.example(name)
    case = runs._case(project, case_id)
    if (case.get("realtimeFactor") or 0) > 0:
        case["realtimeFactor"] = runs.LIVE_PACE
    if name == "hybrid-car":
        case["chargeBalance"] = False
    return project, case_id


@lru_cache(maxsize=1)
def library() -> dict[str, dict]:
    lib = json.loads((runs.BACKEND / "app" / "library" / "components.json").read_text(encoding="utf-8"))
    return {c["id"]: c for c in lib["components"]}


@lru_cache(maxsize=1)
def sites() -> dict[str, list[str]]:
    """component -> the keys of its tables the new engine reads
    (lightsim_engine.table_sites; a Lookup's "table" is its Table 1D or 2D)."""
    import lightsim_engine
    out: dict[str, list[str]] = {}
    for s in lightsim_engine.table_sites():
        keys = ["table_1d", "table_2d"] if s["component"] == "signal.lookup" else [s["table"]]
        for k in keys:
            out.setdefault(s["component"], [])
            if k not in out[s["component"]]:
                out[s["component"]].append(k)
    return out


def _pdef(cdef_id: str, key: str) -> dict:
    return next(p for p in library()[cdef_id]["parameters"] if p["key"] == key)


def _table(project: dict, case_id: str, el: dict, key: str):
    """A part's table as the case runs it: the case's own, the part's, else
    the library's default."""
    case = runs._case(project, case_id)
    own = ((case.get("parameterOverrides") or {}).get(el["id"]) or {}).get(key)
    if own is None:
        own = (el.get("parameterOverrides") or {}).get(key)
    return own if own is not None else _pdef(el["componentDefId"], key).get("default")


def _shown(el: dict, key: str) -> bool:
    """Whether the part uses the table (a Lookup's mode, a battery's pack model)."""
    cdef = el["componentDefId"]
    over = el.get("parameterOverrides") or {}
    if cdef == "signal.lookup":
        return key == ("table_2d" if str(over.get("mode", "1D")) == "2D" else "table_1d")
    show = _pdef(cdef, key).get("showIf")
    if not show:
        return True
    value = over.get(show["key"], _pdef(cdef, show["key"]).get("default"))
    return value in show["values"]


def axis_keys(table: dict, axis: int) -> list[str]:
    if axis == 0:
        return sorted(table, key=float)
    return sorted({k for row in table.values() for k in row}, key=float)


def cut(table: dict, axis: int, end: str, depth: str) -> dict | None:
    """The table cut short at one end of one axis: by its outermost point
    ("edge") or to its inner half ("half"); None when fewer than two points
    would be left."""
    keys = axis_keys(table, axis)
    drop = 1 if depth == "edge" else len(keys) // 2
    if len(keys) - drop < 2:
        return None
    gone = set(keys[:drop] if end == "lo" else keys[-drop:])
    if axis == 0:
        return {k: v for k, v in table.items() if k not in gone}
    return {r: {k: v for k, v in row.items() if k not in gone} for r, row in table.items()}


def _policies(el: dict, key: str, axis: int, policy: str) -> list[str]:
    """The table's out-of-range settings with one axis's set to ``policy``,
    the others as the part has them (else the library's)."""
    axes = _pdef(el["componentDefId"], key)["axes"]
    own = list((el.get("tableOutside") or {}).get(key) or [])
    now = [own[i] if i < len(own) else (a.get("outside") or "clamp") for i, a in enumerate(axes)]
    now[axis] = policy
    return now


def _tables_of(el: dict) -> list[tuple[str, int]]:
    """(key, axis) of every table of a part the new engine reads."""
    out = []
    for key in sites().get(el["componentDefId"], []):
        pdef = next((p for p in library()[el["componentDefId"]]["parameters"] if p["key"] == key), None)
        if pdef is None or pdef.get("type") not in ("table1d", "table2d") or not _shown(el, key):
            continue
        out += [(key, a) for a in range(len(pdef["axes"]))]
    return out


def _profiles_of(el: dict) -> bool:
    """Whether a part's profile is what it runs (no cycle named)."""
    return el["componentDefId"] in ("signal.driving_task", "signal.road_profile")


# ---- the faults -----------------------------------------------------------------

def _elements(project: dict) -> list[dict]:
    return [e for s in project["systems"] for e in s["elements"]]


def faults() -> list[str]:
    """Every fault of the sweep, by id:
    none:<base>                                   the case as it is (a reference)
    table:<base>:<el>.<key>:<axis>:<end>:<depth>:<policy>
    profile:<base>:<el>:<end>                     a Driving Task's or Road Profile's profile
    alone:<base>:<component>                      a part on nothing, its defaults
    alone:<base>:<component>#<key>:<axis>:<end>:<depth>:<policy>
    sampled:<base>:<block>:<sample time>"""
    out = []
    for base in bases():
        out.append(f"none:{base}")
        project, case_id = base_project(base)
        for el in _elements(project):
            for key, axis in _tables_of(el):
                table = _table(project, case_id, el, key)
                for end in ENDS:
                    for depth in DEPTHS:
                        if cut(table, axis, end, depth) is not None:
                            out += [f"table:{base}:{el['id']}.{key}:{axis}:{end}:{depth}:{p}" for p in POLICIES]
            if _profiles_of(el) and _profile_runs(project, case_id, el):
                out += [f"profile:{base}:{el['id']}:{end}" for end in ENDS]
        out += [f"profile:{base}:{SPARE}:{end}" for end in ENDS]  # a Road Profile on the grade
    for base in (*ALONE_BASES, *live_bases()):  # (a live run the new engine fails is not run again)
        for cdef_id in library():
            if cdef_id == "container.system":  # (a subsystem's box, not a part)
                continue
            out.append(f"alone:{base}:{cdef_id}")
            for key in sites().get(cdef_id, []):
                spare = _alone(cdef_id, key)
                for k, axis in _tables_of(spare):
                    if k != key:
                        continue
                    table = (spare["parameterOverrides"].get(key) or _pdef(cdef_id, key)["default"])
                    for end in ENDS:
                        for depth in DEPTHS:
                            if cut(table, axis, end, depth) is not None:
                                out += [f"alone:{base}:{cdef_id}#{key}:{axis}:{end}:{depth}:{p}"
                                        for p in POLICIES]
        for block in ("pid", "lookup1d", "lookup2d"):
            out += [f"sampled:{base}:{block}:{ts}" for ts in SAMPLE_TIMES]
    return out


def live_bases() -> list[str]:
    """The judge's live cases (run live, as the app runs them: run_fault)."""
    return [b for b in bases() if (runs._case(*base_project(b)).get("realtimeFactor") or 0) > 0]


def _profile_runs(project: dict, case_id: str, el: dict) -> bool:
    case = runs._case(project, case_id)
    over = {**(el.get("parameterOverrides") or {}),
            **((case.get("parameterOverrides") or {}).get(el["id"]) or {})}
    return not over.get("cycle")


def _profile_points(text: str) -> list[tuple[float, float]]:
    pts = []
    for part in text.split(";"):
        if part.strip():
            x, y = part.split(":")
            pts.append((float(x), float(y)))
    return pts


def _spare(cdef_id: str, **over) -> dict:
    return {"id": SPARE, "componentDefId": cdef_id, "label": f"Spare {library()[cdef_id]['name']}",
            "position": {"x": 0, "y": 0}, "parameterOverrides": dict(over)}


def _alone(cdef_id: str, key: str = "") -> dict:
    """A part placed on nothing: its defaults; a Lookup in the mode of the
    table cut, with tables of more points than its defaults' two."""
    if cdef_id != "signal.lookup":
        return _spare(cdef_id)
    return _spare(cdef_id, mode="2D" if key == "table_2d" else "1D",
                  table_1d={str(x): x for x in range(0, 101, 25)},
                  table_2d={str(x): {str(y): x + y for y in range(0, 101, 25)} for x in range(0, 101, 25)})


def fault_project(fid: str) -> tuple[dict, str]:
    """(project JSON, case id) of a fault (faults())."""
    kind, base, *rest = fid.split(":", 2)
    rest = rest[0].split(":") if rest else []
    project, case_id = base_project(base)
    case = runs._case(project, case_id)
    system = project["systems"][0]
    if kind == "none":
        return project, case_id
    if kind == "table":
        target, axis, end, depth, policy = rest
        el_id, key = target.split(".", 1)
        el = runs._element(project, el_id)
        _put_cut(project, case_id, el, key, int(axis), end, depth, policy)
        return project, case_id
    if kind == "profile":
        el_id, end = rest
        if el_id == SPARE:  # a Road Profile feeding the Vehicle its grade, by its distance
            el = _spare("signal.road_profile")
            system["elements"].append(el)
            veh = next(e["id"] for e in _elements(project) if e["componentDefId"] == "vehicle.body")
            project["dataBusConnections"] += [
                {"id": "db-x1", "element1Id": veh, "port1Id": "sig_distance",
                 "element2Id": SPARE, "port2Id": "sig_distance_in"},
                {"id": "db-x2", "element1Id": SPARE, "port1Id": "sig_grade",
                 "element2Id": veh, "port2Id": "sig_grade_in"}]
        el = runs._element(project, el_id)
        over = {**(el.get("parameterOverrides") or {}),
                **((case.get("parameterOverrides") or {}).get(el_id) or {})}
        text = over.get("profile") or _pdef(el["componentDefId"], "profile")["default"]
        pts = _profile_points(text)
        keep = pts[len(pts) // 2:] if end == "lo" else pts[:len(pts) // 2 + 1]
        el.setdefault("parameterOverrides", {})["profile"] = "; ".join(f"{x:g}:{y:g}" for x, y in keep)
        ((case.get("parameterOverrides") or {}).get(el_id) or {}).pop("profile", None)
        return project, case_id
    if kind == "alone":
        cdef_id, _, key = rest[0].partition("#")
        el = _alone(cdef_id, key)
        system["elements"].append(el)
        if key:
            axis, end, depth, policy = rest[1:]
            _put_cut(project, case_id, el, key, int(axis), end, depth, policy)
        return project, case_id
    if kind == "sampled":
        block, ts = rest
        dt = float(case.get("timeStep") or 1.0)
        sample = {"0": 0.0, "5ms": 0.005, "15ms": 0.015, "half-step": dt / 2, "step": dt, "3-steps": 3 * dt, "5s": 5.0}[ts]
        veh = next(e["id"] for e in _elements(project) if e["componentDefId"] == "vehicle.body")
        wires = [(veh, "sig_speed", SPARE, "sig_x_in")]
        if block == "pid":
            el = _spare("control.pid", sample_time_s=sample, kp=0.01, ki=0.002, kd=0.0,
                        out_min=-10.0, out_max=10.0)
            system["elements"].append(_spare("signal.constant", value=10.0) | {"id": "x-setpoint"})
            wires = [("x-setpoint", "sig_out", SPARE, "sig_setpoint_in"), (veh, "sig_speed", SPARE, "sig_feedback_in")]
        elif block == "lookup1d":
            el = _spare("signal.lookup", sample_time_s=sample, table_1d={"0": 0, "50": 20, "150": 100})
        else:
            el = _spare("signal.lookup", sample_time_s=sample, mode="2D",
                        table_2d={"0": {"0": 0, "100000": 10}, "150": {"0": 100, "100000": 200}})
            wires.append((veh, "sig_distance", SPARE, "sig_y_in"))
        system["elements"].append(el)
        project["dataBusConnections"] += [
            {"id": f"db-x{k}", "element1Id": a, "port1Id": ap, "element2Id": b, "port2Id": bp}
            for k, (a, ap, b, bp) in enumerate(wires)]
        return project, case_id
    raise ValueError(f"no such fault kind: {fid}")


def _put_cut(project: dict, case_id: str, el: dict, key: str, axis: int, end: str, depth: str,
             policy: str) -> None:
    table = _table(project, case_id, el, key)
    el.setdefault("parameterOverrides", {})[key] = cut(table, axis, end, depth)
    case_own = (runs._case(project, case_id).get("parameterOverrides") or {}).get(el["id"]) or {}
    case_own.pop(key, None)
    el["tableOutside"] = {**(el.get("tableOutside") or {}), key: _policies(el, key, axis, policy)}


# ---- one fault on both engines ---------------------------------------------------

SETTINGS = ("new", "quick", "live", "today")  # ("live": a live case as the app runs it, see run_fault)


def digest(result, block: str | None = None) -> dict:
    """What the sweep compares of a run (compare())."""
    texts = [m.text for m in result.messages]
    refused = REFUSED.match(texts[0]) if texts else None
    out = {
        "status": result.status, "engine": result.engine,
        "refused": refused.group(1) if refused else None,
        "first": texts[0][:600] if texts else "",
        "codes": sorted({c for m in result.messages if m.level in ("warning", "error")
                         for c in CODE.findall(m.text)}),
        "notes": sorted({c for m in result.messages if m.level == "info" for c in CODE.findall(m.text)}),
        "warnings": sorted({m.text for m in result.messages if m.level == "warning"}),
        "errors": sorted({m.text for m in result.messages if m.level == "error"}),
        "notValid": sorted({s.key for s in result.summary if s.notValid}),
        "rows": sorted({s.key for s in result.summary}),
    }
    if block:
        ch = next((c for c in result.channels if c.elementId == block and c.portId == "sig_out"), None)
        out["output"] = [[p["t"], p["value"]] for p in ch.timeSeries] if ch else None
    return out


def run_fault(fid: str) -> dict:
    """The fault's runs (SETTINGS), each digested; today's is not run when
    neither strict run ran and no live run is made (nothing to compare it
    with). A live case is run a third time as the app runs it: the new path
    not strict, with a view (a live run the new engine fails is not run
    again on today's engine, so a strict run's hand-back would hide it)."""
    from app.schemas import Project
    from app.solver import simulate

    raw, case_id = fault_project(fid)
    block = SPARE if fid.startswith("sampled:") else None
    # a live case runs live, as the app runs it: its points to a view
    live = (runs._case(raw, case_id).get("realtimeFactor") or 0) > 0
    got: dict = {}
    for setting in SETTINGS:
        if setting == "live" and not live:
            continue
        if setting == "today" and "live" not in got and all(
                got[s].get("refused") or got[s].get("raised") for s in ("new", "quick")):
            continue
        for k in ("LIGHTSIM_ENGINE", "LIGHTSIM_ENGINE_STRICT"):
            os.environ.pop(k, None)
        if setting != "today":
            os.environ.update(LIGHTSIM_ENGINE="new")
            if setting != "live":
                os.environ.update(LIGHTSIM_ENGINE_STRICT="1")
        project = Project.model_validate(json.loads(json.dumps(raw)))
        if setting == "quick":
            next(c for c in project.cases if c.id == case_id).mode = "quick"
        t0 = time.perf_counter()
        try:
            got[setting] = digest(simulate(project, case_id, _view if live else None), block)
        except Exception as e:  # noqa: BLE001 — told as the run's result
            got[setting] = {"raised": f"{type(e).__name__}: {e}"[:600]}
        got[setting]["wall_s"] = round(time.perf_counter() - t0, 2)
    for k in ("LIGHTSIM_ENGINE", "LIGHTSIM_ENGINE_STRICT"):
        os.environ.pop(k, None)
    return got


def _view(msg: dict) -> None:
    """A live run's view: it takes each point and shows nothing."""


# ---- comparing ------------------------------------------------------------------

LEVEL = {"success": 0, "warning": 1, "failed": 2}


def _own(text: str) -> bool:
    """A message of the path or of quick mode about itself, not of the run."""
    return text.startswith((FLAG, "Run on the new engine", "Not run in quick mode"))


def compare(fid: str, setting: str, today: dict | None, other: dict) -> dict:
    """One strict run against today's: {"handed": code} for one handed back
    with a listed code, else {"diffs": [...]} (empty: they agree)."""
    from app.solver import new_engine
    if "raised" in other:
        return {"diffs": [f"raised: {other['raised']}"]}
    if other.get("refused"):
        code = other["refused"]
        bad = code not in new_engine.CODES or code.endswith("-ERROR")
        return {"diffs": [f"refused with {code}, not a listed hand-back"]} if bad else {"handed": code}
    if today is None or "raised" in today:
        return {"diffs": [f"today's run: {today and today.get('raised')}"]}
    if setting == "live":  # (the app's live run: only its verdict, see run_fault)
        if other["status"] == "failed" and today["status"] != "failed":
            return {"diffs": [f"live run: failed on the new path ({other['first'][:300]}), today's {today['status']}"]}
        return {"diffs": []}
    diffs = []
    if other["status"] != today["status"]:
        worse = LEVEL[other["status"]] < LEVEL[today["status"]]
        diffs.append(f"verdict: today {today['status']}, {setting} {other['status']}"
                     + (" (a better verdict than today's)" if worse else ""))
    if other["codes"]:
        diffs.append(f"codes in its warnings or errors: {other['codes']}")
    if "failed" in (today["status"], other["status"]):  # (the verdict says it: a failed run has no rows)
        return {"diffs": diffs}
    t_w = {masked(w) for w in today["warnings"]}
    o_w = {masked(w) for w in other["warnings"] if not _own(w)}
    if setting == "quick":  # (its flags stand in for the speed trace's verdict)
        flagged = any(w.startswith(FLAG) for w in other["warnings"])
        trace = {w for w in t_w if TRACE in w}
        if bool(trace) != flagged:
            diffs.append(f"speed trace: today {'warns' if trace else 'does not warn'}, quick mode "
                         f"{'flags' if flagged else 'does not flag'}")
        t_w -= trace
    if t_w - o_w:
        diffs.append(f"warnings only today: {sorted(t_w - o_w)}")
    if o_w - t_w:
        diffs.append(f"warnings only on {setting}: {sorted(o_w - t_w)}")
    rows = set(other["rows"]) if setting == "quick" else set(today["rows"]) | set(other["rows"])
    t_nv, o_nv = set(today["notValid"]) & rows, set(other["notValid"]) & rows
    if t_nv != o_nv:
        diffs.append(f"notValid: only today {sorted(t_nv - o_nv)}, only {setting} {sorted(o_nv - t_nv)}")
    if fid.startswith("sampled:"):
        d = _outputs(today.get("output"), other.get("output"))
        if d:
            diffs.append(d)
    return {"diffs": diffs}


def _outputs(today, other) -> str | None:
    """A sampled block's outputs at the times both runs give: within
    OUTPUT_BAND of their range on today's run; the first, which today's
    engine records before its first step, while the blocks still read 0,
    within that band of today's first two values."""
    if not today or not other:
        return f"outputs: today {'none' if not today else len(today)}, other {'none' if not other else len(other)}"
    t = {round(x, 6): v for x, v in today if v is not None and math.isfinite(v)}
    o = {round(x, 6): v for x, v in other if v is not None and math.isfinite(v)}
    common = sorted(set(t) & set(o))
    if not common:
        return "outputs: no common times"
    vals = [t[x] for x in common]
    band = OUTPUT_BAND * max(max(vals) - min(vals), 1e-9) + 1e-9
    if len(common) > 1:  # the first point as the judge has it (classes.toml: today-first-point-before-its-step)
        a, b = t[common[0]], t[common[1]]
        if min(a, b) - band <= o[common[0]] <= max(a, b) + band:
            common = common[1:]
    worst = max(common, key=lambda x: abs(t[x] - o[x]))
    if abs(t[worst] - o[worst]) > band:
        return (f"outputs between samples: at t = {worst:g} s today {t[worst]:.6g}, other {o[worst]:.6g} "
                f"(band {band:.3g})")
    return None


# ---- the sweep --------------------------------------------------------------------

def _file(out: Path, fid: str) -> Path:
    return out / "raw" / (re.sub(r"[^A-Za-z0-9_.-]", "~", fid) + ".json")


TIMEOUT_S = 900.0  # a fault's three runs at most (the hybrid's take about a minute)


def job(fid: str, out: Path) -> None:
    """One fault's runs, in a process of its own (`sweep-job`): its record."""
    import warnings
    warnings.filterwarnings("ignore")
    t0 = time.perf_counter()
    try:
        got = run_fault(fid)
    except Exception as e:  # noqa: BLE001 — the fault itself could not be built
        got = {"built": f"{type(e).__name__}: {e}"[:600]}
    _write(out, fid, {"fault": fid, "stamp": runs.stamp(), "wall_s": round(time.perf_counter() - t0, 2),
                      "runs": got})


def _write(out: Path, fid: str, rec: dict) -> None:
    path = _file(out, fid)
    tmp = path.with_suffix(".part")
    tmp.write_text(json.dumps(rec), encoding="utf-8")
    tmp.replace(path)


def sweep(out: Path, workers: int, only: str = "") -> dict:
    """Runs every fault (faults(); ``only``: those whose id holds it) not
    run before at this code, each in a process of its own (a run that hangs
    is stopped at TIMEOUT_S and recorded so), then writes the summary
    (summarise())."""
    out.mkdir(parents=True, exist_ok=True)
    (out / "raw").mkdir(exist_ok=True)
    (out / "logs").mkdir(exist_ok=True)
    now = runs.stamp()
    ids = [f for f in faults() if only in f]

    def done(fid):
        p = _file(out, fid)
        return p.exists() and json.loads(p.read_text(encoding="utf-8")).get("stamp") == now
    todo = [f for f in ids if not done(f)]
    print(f"{len(ids)} faults, {len(ids) - len(todo)} done before at this code, {len(todo)} to run", flush=True)
    # the slow ones first (the hybrid, the long cycles), so the pool ends together
    todo.sort(key=lambda f: ("hybrid" not in f, "wltc" not in f and "udds" not in f))
    user = out / "user"
    env = {k: v for k, v in os.environ.items() if k not in ("LIGHTSIM_ENGINE", "LIGHTSIM_ENGINE_STRICT")}
    env.update(LIGHTSIM_DATA_DIR=str(user), LIGHTSIM_PROJECTS_DIR=str(user / "projects"), PYTHONHASHSEED="0")
    t0, count = time.monotonic(), [0]

    def one(fid: str) -> None:
        log = out / "logs" / (_file(out, fid).stem + ".log")
        started = time.monotonic()
        with open(log, "w", encoding="utf-8") as f:
            try:
                code = subprocess.run([sys.executable, "-m", "benchmarks.back_to_back", "sweep-job", str(out), fid],
                                      cwd=runs.REPO, env=env, stdout=f, stderr=subprocess.STDOUT,
                                      timeout=TIMEOUT_S).returncode
            except subprocess.TimeoutExpired:
                code = "timeout"
        if code != 0:  # (no record of this run, or a stale one: say why, as its result)
            _write(out, fid, {"fault": fid, "stamp": now, "wall_s": round(time.monotonic() - started, 2),
                              "runs": {"built": f"its process ended with {code} (log: {log.name})"}})
        count[0] += 1
        if count[0] % 25 == 0 or count[0] == len(todo):
            print(f"{count[0]}/{len(todo)} ({time.monotonic() - t0:.0f} s) {fid}", flush=True)
    with ThreadPoolExecutor(max_workers=workers) as pool:
        list(pool.map(one, todo))
    return summarise(out, ids)


def summarise(out: Path, ids: list[str] | None = None) -> dict:
    """The counts and every mismatch: OUT/sweep.json and OUT/sweep.md."""
    ids = ids if ids is not None else faults()
    res = {"faults": len(ids), "by_kind": {}, "runs": 0, "handed": {}, "mismatches": [], "missing": []}
    for fid in ids:
        p = _file(out, fid)
        kind, base = fid.split(":")[:2]
        res["by_kind"][kind] = res["by_kind"].get(kind, 0) + 1
        if not p.exists():
            res["missing"].append(fid)
            continue
        rec = json.loads(p.read_text(encoding="utf-8"))
        if "built" in rec["runs"]:
            res["mismatches"].append({"fault": fid, "setting": "-", "diffs": [f"not built: {rec['runs']['built']}"]})
            continue
        got = rec["runs"]
        for setting in ("new", "quick", "live"):
            if setting not in got:
                if setting != "live" or base.split("/")[1].endswith("-live"):
                    res["mismatches"].append({"fault": fid, "setting": setting, "diffs": [
                        f"no {setting} run in its record" + (" (a live case is also run live)" if setting == "live" else "")]})
                continue
            res["runs"] += 1
            c = compare(fid, setting, got.get("today"), got[setting])
            if "handed" in c:
                key = f"{setting}:{c['handed']}"
                res["handed"][key] = res["handed"].get(key, 0) + 1
            elif c["diffs"]:
                res["mismatches"].append({"fault": fid, "setting": setting, "diffs": c["diffs"]})
    res["mismatched_faults"] = len({m["fault"] for m in res["mismatches"]})
    (out / "sweep.json").write_text(json.dumps(res, indent=1), encoding="utf-8")
    lines = ["# Mismatch sweep", "", f"{res['faults']} faults ({res['by_kind']}), {res['runs']} strict runs "
             f"compared; {len(res['mismatches'])} mismatched runs in {res['mismatched_faults']} faults; "
             f"{len(res['missing'])} not run.", "", "Handed back to today's engine, by code:", ""]
    lines += [f"- {k}: {v}" for k, v in sorted(res["handed"].items())]
    lines += ["", "Mismatches:", ""]
    for m in res["mismatches"]:
        lines.append(f"- `{m['fault']}` ({m['setting']}): " + "; ".join(m["diffs"])[:1500])
    (out / "sweep.md").write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"{res['faults']} faults, {len(res['mismatches'])} mismatched runs in {res['mismatched_faults']} "
          f"faults, handed back {sum(res['handed'].values())}, missing {len(res['missing'])}", flush=True)
    return res
