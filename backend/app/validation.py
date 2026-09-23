"""Data Checks — pre-run model validation.

Structural solvability is delegated to the solver's model extraction
(build_model): anything it cannot reduce becomes an error check here,
and its advisory notes become warnings. On top of that this module
checks reference integrity, parameter sanity (ranges, table data,
script compilation) and wiring conventions.
"""
from __future__ import annotations

import math

from .library import library_by_id
from .schemas import DataCheck, ElementInstance, Project
from .solver import (
    ModelError,
    ScriptError,
    TableError,
    build_model,
    compile_script,
    parse_table1d,
    parse_table2d,
    profile_problems,
)

# param key → (label, min exclusive, max inclusive)
NUMERIC_RANGES: dict[str, tuple[str, float, float]] = {
    "efficiency_pct": ("Efficiency", 0.0, 100.0),
    "initial_soc_pct": ("Initial SOC", 0.0, 100.0),
    "min_soc_pct": ("Minimum SOC", 0.0, 100.0),
    "regen_weight_pct": ("Recuperation weight", -0.001, 100.0),
    "q4_torque_scale_pct": ("Generator torque scale", -0.001, 200.0),
    "vehicle_load_share_pct": ("Vehicle load share", 0.0, 100.0),
    "torque_split_a_pct": ("Torque split", -0.001, 100.0),
    "initial_fill_pct": ("Initial fill", -0.001, 100.0),
}

POSITIVE_PARAMS = {
    "capacity_kWh": "capacity",
    "mass_kg": "mass",
    "radius_m": "wheel radius",
    "ratio": "transmission ratio",
}

# Script / PID / Lookup blocks may run slower than the solver step; above
# this their sampling starts to shape the results (real vehicle controllers
# run every 10-100 ms).
COARSE_SAMPLE_TIME_S = 0.1


def validate_project(project: Project) -> list[DataCheck]:
    checks: list[DataCheck] = []
    defs = library_by_id()

    def add(level: str, text: str, el: ElementInstance | None = None) -> None:
        checks.append(DataCheck(
            level=level,  # type: ignore[arg-type]
            text=text,
            elementId=el.id if el else None,
            elementLabel=el.label if el else None,
        ))

    all_elements = {el.id: el for s in project.systems for el in s.elements}

    # -- reference integrity ---------------------------------------------------
    def ports_of(el: ElementInstance):
        cdef = defs.get(el.componentDefId)
        if cdef is None:
            return []
        ports = list(cdef.ports)
        if cdef.allowDynamicPorts and el.dynamicPorts:
            ports.extend(el.dynamicPorts)
        return ports

    def port_of(el_id: str, port_id: str):
        el = all_elements.get(el_id)
        if el is None:
            return None
        return next((p for p in ports_of(el) if p.id == port_id), None)

    for el in all_elements.values():
        cdef = defs.get(el.componentDefId)
        if cdef is None:
            add("error", f"Element references unknown component type '{el.componentDefId}'.", el)
            continue
        if el.dynamicPorts:
            if not cdef.allowDynamicPorts:
                add("error", f"'{el.label}' has custom ports but '{cdef.name}' does not allow them.", el)
            seen_ids = set(p.id for p in cdef.ports)
            for p in el.dynamicPorts:
                if p.kind != "signal":
                    add("error", f"Custom port '{p.name}' of '{el.label}' must be a signal port.", el)
                if p.id in seen_ids:
                    add("error", f"Duplicate port id '{p.id}' on '{el.label}'.", el)
                seen_ids.add(p.id)

    for system in project.systems:
        for conn in system.connections:
            for el_id, port_id in ((conn.sourceElementId, conn.sourcePortId),
                                   (conn.targetElementId, conn.targetPortId)):
                if el_id not in all_elements:
                    add("error", f"Connection '{conn.id}' references missing element '{el_id}'.")
                elif port_of(el_id, port_id) is None:
                    add("error",
                        f"Connection '{conn.id}' references missing port '{port_id}' "
                        f"on '{all_elements[el_id].label}'.")
            pa = port_of(conn.sourceElementId, conn.sourcePortId)
            pb = port_of(conn.targetElementId, conn.targetPortId)
            if pa and pb and pa.kind != pb.kind and "signal" not in (pa.kind, pb.kind):
                add("error",
                    f"Connection between '{all_elements[conn.sourceElementId].label}' and "
                    f"'{all_elements[conn.targetElementId].label}' mixes incompatible port "
                    f"kinds ({pa.kind} ↔ {pb.kind}).")
            elif pa and pb and pa.kind == pb.kind and pa.kind in ("thermal", "fluid"):
                add("warning",
                    f"Connection between '{all_elements[conn.sourceElementId].label}' and "
                    f"'{all_elements[conn.targetElementId].label}' is a {pa.kind} connection — "
                    f"this version has no {pa.kind} solver, so it is ignored during simulation.")

    for dbc in project.dataBusConnections:
        p1 = port_of(dbc.element1Id, dbc.port1Id)
        p2 = port_of(dbc.element2Id, dbc.port2Id)
        if p1 is None or p2 is None:
            add("error", "Data bus connection references a missing element or port.")
            continue
        if p1.direction == p2.direction and p1.direction in ("input", "output"):
            add("warning",
                f"Data bus connection links two {p1.direction}s "
                f"('{all_elements[dbc.element1Id].label}.{p1.name}' ↔ "
                f"'{all_elements[dbc.element2Id].label}.{p2.name}') — no data will flow.")

    # -- parameter sanity --------------------------------------------------------
    for el in all_elements.values():
        cdef = defs.get(el.componentDefId)
        if cdef is None:
            continue
        params = {p.key: p.default for p in cdef.parameters}
        params.update(el.parameterOverrides)
        pdef_by_key = {p.key: p for p in cdef.parameters}

        for key, (label, lo, hi) in NUMERIC_RANGES.items():
            if key in params and pdef_by_key.get(key) and pdef_by_key[key].type == "number":
                try:
                    val = float(params[key])  # type: ignore[arg-type]
                except (TypeError, ValueError):
                    add("error", f"{label} of '{el.label}' is not a number.", el)
                    continue
                if not (lo < val <= hi):
                    add("error", f"{label} of '{el.label}' must be in ({lo:g}, {hi:g}] — got {val:g}.", el)
        for key, label in POSITIVE_PARAMS.items():
            if key in params and pdef_by_key.get(key) and pdef_by_key[key].type == "number":
                try:
                    if float(params[key]) <= 0:  # type: ignore[arg-type]
                        add("error", f"'{el.label}' has a non-positive {label}.", el)
                except (TypeError, ValueError):
                    add("error", f"'{el.label}': {label} is not a number.", el)

        for pdef in cdef.parameters:
            value = params.get(pdef.key)
            if pdef.type == "table1d":
                try:
                    parse_table1d(value)
                except TableError as e:
                    add("error", f"'{el.label}.{pdef.label}': {e}", el)
            elif pdef.type == "table2d":
                try:
                    parse_table2d(value)
                except TableError as e:
                    add("error", f"'{el.label}.{pdef.label}': {e}", el)
            elif pdef.type == "code":
                try:
                    compile_script(str(value or ""), el.label)
                except ScriptError as e:
                    add("error", str(e), el)

        if cdef.id in ("signal.driving_task", "signal.road_profile"):
            for level, text in profile_problems(str(params.get("profile", ""))):
                add(level, f"'{el.label}' profile: {text}.", el)
        if "sample_time_s" in pdef_by_key:
            try:
                ts = float(params.get("sample_time_s", 0) or 0)  # type: ignore[arg-type]
            except (TypeError, ValueError):
                add("error", f"Sample Time of '{el.label}' is not a number.", el)
            else:
                if not math.isfinite(ts):
                    add("error", f"Sample Time of '{el.label}' must be a finite number — got {ts:g}.", el)
                elif ts < 0:
                    add("error", f"Sample Time of '{el.label}' must not be negative — got {ts:g} s.", el)
                elif ts > COARSE_SAMPLE_TIME_S:
                    add("warning",
                        f"'{el.label}' runs only every {ts:g} s (its Sample Time); real vehicle "
                        f"controllers run every 10-100 ms, so results may depend on this "
                        f"setting.", el)

    # -- structural solvability (delegated to model extraction) ------------------
    model = None
    try:
        model = build_model(project)
    except ModelError as e:
        for text in e.errors:
            add("error", text)
    if model is not None:
        for text in model.warnings:
            add("warning", text)

        for el_id in model.floating_returns:
            el = all_elements.get(el_id)
            if el is not None:
                add("info",
                    f"'{el.label}' has an unconnected negative (−) terminal — using an "
                    f"implicit ground return. Wire it to Ground for an explicit return path.",
                    el)

        # wiring conventions
        for el_id, cdef in model.cdef_of.items():
            el = all_elements[el_id]
            if cdef.id == "driver.driver" and (el_id, "sig_target_in") not in model.signal_route:
                add("warning",
                    f"Driver '{el.label}' has no Target Speed signal — it will hold 0 km/h.", el)
            if cdef.id == "mech.brake" and (el_id, "sig_demand_in") not in model.signal_route:
                add("info",
                    f"Brake '{el.label}' has no Brake Command signal — it will never apply.", el)
        sources_in_dl = {
            src.el_id
            for dl in model.drivelines
            for seg in dl.segments
            for src in seg.sources
        }
        motor_on_bus = {m for bus in model.buses for m in bus.motors}
        for el_id, cdef in model.cdef_of.items():
            el = all_elements[el_id]
            if cdef.id == "motor.emotor":
                if el_id not in motor_on_bus:
                    add("error", f"E-Motor '{el.label}' has no live electrical connection.", el)
                if el_id not in sources_in_dl:
                    add("warning", f"E-Motor '{el.label}' has no mechanical connection — it will idle.", el)
            elif cdef.id == "engine.combustion":
                if el_id not in sources_in_dl:
                    add("warning", f"Engine '{el.label}' has no mechanical connection — it will idle.", el)

        if not model.drivelines and not any(b.consumers for b in model.buses):
            add("info", "Model has no driveline and no electrical loads — nothing will happen.")

    if not any(s.elements for s in project.systems):
        add("info", "Model is empty — drag components from the library onto the canvas.")
    if not project.cases:
        add("warning", "Project has no simulation case defined.")

    if not checks:
        add("info", "All data checks passed — model is ready to run.")
    return checks
