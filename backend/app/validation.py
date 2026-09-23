"""Data Checks — pre-run model validation.

Structural solvability is delegated to the solver's model extraction
(build_model): anything it cannot reduce becomes an error check here,
and its advisory notes become warnings. On top of that this module
checks reference integrity, parameter sanity (ranges, table data,
script compilation), signal wiring, whether the model can do its job
(an energy source for every motor, a mechanical path from the motors
and engines to the wheels, a speed demand that reaches them) and the
plausibility of key vehicle parameters.
"""
from __future__ import annotations

import math
from collections import defaultdict
from typing import Callable

from .library import library_by_id
from .schemas import DataCheck, ElementInstance, Project
from .solver import (
    Model,
    ModelError,
    ScriptError,
    TableError,
    build_model,
    check_script,
    parse_table1d,
    parse_table2d,
    profile_problems,
)
from .solver.network import NO_DRIVER, NO_VEHICLE, NO_WHEELS, SIGNAL_BLOCK_TYPES, ports_of

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

# Propulsion sources: type → (label, demand input, its name, what happens unwired)
PROPULSION = {
    "motor.emotor": ("E-Motor", "sig_demand_in", "Traction Command",
                     "it will never produce torque"),
    "engine.combustion": ("Engine", "sig_throttle_in", "Throttle", "it will only idle"),
}
# Blocks that turn the signals they receive into commands (traced when asking
# whether the Driver or a Driving Task reaches the powertrain).
CONTROLLER_TYPES = {"driver.driver", *SIGNAL_BLOCK_TYPES}
LOAD_TYPES = {"propulsion.wheel", "propulsion.propeller"}
SPLIT_TYPES = {"mech.differential", "mech.transfer_case"}

# Plausibility bounds (warnings only), generous enough for e-bikes to trucks.
VEHICLE_MASS_KG = (30.0, 60_000.0)
BATTERY_KWH = (0.1, 2_000.0)
AUX_LOAD_MAX_KW = 50.0  # a constant load beyond this is not an auxiliary
FINAL_DRIVE_MAX_RATIO = 25.0

Add = Callable[..., None]

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

    # -- signal fan-in: an input takes one source; the solver keeps the last ----
    sources_of: dict[tuple[str, str], list[tuple[str, str]]] = {}

    def note_signal(a: tuple[str, str], b: tuple[str, str]) -> None:
        pa, pb = port_of(*a), port_of(*b)
        if pa is None or pb is None or "signal" not in (pa.kind, pb.kind):
            return
        if pa.direction == "output" and pb.direction != "output":
            src, dst = a, b
        elif pb.direction == "output" and pa.direction != "output":
            src, dst = b, a
        else:
            return
        srcs = sources_of.setdefault(dst, [])
        if src not in srcs:
            srcs.append(src)

    # same order as the solver's model extraction: canvas wires, then data bus
    for system in project.systems:
        for conn in system.connections:
            note_signal((conn.sourceElementId, conn.sourcePortId),
                        (conn.targetElementId, conn.targetPortId))
    for dbc in project.dataBusConnections:
        note_signal((dbc.element1Id, dbc.port1Id), (dbc.element2Id, dbc.port2Id))

    def signal_name(el_id: str, port_id: str) -> str:
        return f"'{all_elements[el_id].label}.{port_of(el_id, port_id).name}'"

    for (el_id, port_id), srcs in sources_of.items():
        if len(srcs) > 1:
            names = ", ".join(signal_name(*s) for s in srcs[:-1]) + f" and {signal_name(*srcs[-1])}"
            add("error",
                f"{signal_name(el_id, port_id)} has {len(srcs)} sources: {names} — an input "
                f"takes one signal and the run would silently use only the last link. "
                f"Remove all but one of them.",
                all_elements[el_id])

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
                try:  # compile only: Data Checks never run script code
                    check_script(str(value or ""), el.label)
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
        replaced = _drive_checks(project, model, add)
        for text in model.warnings:
            if text not in replaced:
                add("warning", text)

        for el_id in model.floating_returns:
            el = all_elements.get(el_id)
            if el is not None:
                add("info",
                    f"'{el.label}' has an unconnected negative (−) terminal — using an "
                    f"implicit ground return. Wire it to Ground for an explicit return path.",
                    el)

        _plausibility_checks(model, add)

        if not model.drivelines and not any(b.consumers for b in model.buses):
            add("info", "Model has no driveline and no electrical loads — nothing will happen.")

    if not any(s.elements for s in project.systems):
        add("info", "Model is empty — drag components from the library onto the canvas.")
    if not project.cases:
        add("warning", "Project has no simulation case defined.")

    if not checks:
        add("info", "All data checks passed: wiring, power supply, drive path, command "
                    "signals and key parameter ranges were checked. Data Checks cannot tell "
                    "whether the results will be plausible — review them after the run.")
    return checks


def _drive_checks(project: Project, model: Model, add: Add) -> set[str]:
    """Can this model do its job? Checks that a run would otherwise "pass"
    while the vehicle never moves or parts silently do nothing: unconnected
    parts, motors without an energy source, motors and engines that reach no
    wheel, open differentials with a free output, missing command signals,
    and a speed demand that reaches no motor or engine. Returns the model
    advisories (build_model warnings) it reports as errors instead."""
    elements, cdef_of, route = model.elements, model.cdef_of, model.signal_route
    replaced: set[str] = set()

    def typ(el_id: str) -> str:
        return cdef_of[el_id].id

    def of_type(*types: str) -> list[str]:
        return [e for e, c in cdef_of.items() if c.id in types]

    def err(el_id: str, text: str) -> None:
        add("error", text, elements[el_id])

    def warn(el_id: str, text: str) -> None:
        add("warning", text, elements[el_id])

    vehicle_model = model.vehicle is not None
    sources = of_type(*PROPULSION)
    dl_of = {e: dl for dl in model.drivelines for e in dl.element_group}

    def reaches_load(dl) -> bool:
        return any(seg.wheels or seg.props for seg in dl.segments)

    wired: set[tuple[str, str]] = set()
    mech_peers: dict[tuple[str, str], list[str]] = defaultdict(list)
    mech_nbrs: dict[str, set[str]] = defaultdict(set)
    for system in project.systems:
        for c in system.connections:
            a, b = (c.sourceElementId, c.sourcePortId), (c.targetElementId, c.targetPortId)
            if a[0] not in cdef_of or b[0] not in cdef_of or a[0] == b[0]:
                continue
            wired |= {a, b}
            kinds = {p.kind for e, pid in (a, b) for p in cdef_of[e].ports if p.id == pid}
            if kinds == {"mechanical"}:
                mech_peers[a].append(b[0])
                mech_peers[b].append(a[0])
                mech_nbrs[a[0]].add(b[0])
                mech_nbrs[b[0]].add(a[0])

    # -- parts the solver leaves out because nothing is connected to them ------
    for el_id, cdef in cdef_of.items():
        if cdef.id in PROPULSION or cdef.id == "boundary.ground":
            continue  # motors/engines are checked below; an idle ground is harmless
        label = elements[el_id].label
        mech = any(p.kind == "mechanical" for p in cdef.ports)
        plus = [p for p in cdef.ports if p.kind == "electrical" and p.polarity == "positive"]
        if mech and el_id not in dl_of:
            note = (" — it carries none of the vehicle's weight or rolling resistance"
                    if cdef.id == "propulsion.wheel" else "")
            warn(el_id, f"'{label}' is not connected to anything, so the simulation leaves "
                        f"it out{note}.")
        elif plus:
            loose = [p.name for p in plus if (el_id, p.id) not in wired]
            if loose:
                warn(el_id, f"'{label}' has nothing connected to its {' or '.join(loose)}, so "
                            f"the simulation leaves it out.")
        elif cdef.id == "electric.node" and not any(e == el_id for e, _ in wired):
            warn(el_id, f"'{label}' is not connected to anything.")

    # -- energy: every E-Motor needs a source on its bus -------------------------
    fed_from = {d: bus for bus in model.buses for d in bus.dcdc_in}

    def supplied(bus, seen: tuple = ()) -> bool:
        if bus.battery or bus.vsource or bus.fuelcell:
            return True
        return any(d in fed_from and fed_from[d] not in seen
                   and supplied(fed_from[d], (*seen, bus)) for d in bus.dcdc_out)

    motor_on_bus = {m for bus in model.buses for m in bus.motors}
    for m in of_type("motor.emotor"):
        if m not in motor_on_bus:
            err(m, f"E-Motor '{elements[m].label}' has no live electrical connection.")
    for bus in model.buses:
        primary = bus.battery or bus.vsource or bus.fuelcell
        if primary and not (bus.motors or bus.consumers or bus.dcdc_in):
            warn(primary, f"'{elements[primary].label}' supplies nothing: no E-Motor, load or "
                          f"DC-DC converter is connected to its bus.")
        if supplied(bus):
            continue
        for m in bus.motors:
            err(m, f"E-Motor '{elements[m].label}' has no power source: nothing on its "
                   f"electrical bus is a battery, fuel cell or voltage source, so it "
                   f"produces no torque.")
        for c in bus.consumers:
            warn(c, f"'{elements[c].label}' is on an electrical bus with no power source — "
                    f"its demand is not met.")

    # -- drive path: motors and engines must reach the wheels ---------------------
    for text in model.warnings:
        if text in (NO_VEHICLE, NO_WHEELS):
            add("error", text)
            replaced.add(text)
    path_errors = False
    for src in sources:
        kind, label = PROPULSION[typ(src)][0], elements[src].label
        dl = dl_of.get(src)
        if dl is None:
            err(src, f"{kind} '{label}' is not mechanically connected — it cannot drive "
                     f"anything.")
            path_errors = True
        elif (vehicle_model and not reaches_load(dl)
              and sum(1 for e in dl.element_group if typ(e) in PROPULSION) < 2):
            # (two sources on a wheel-less shaft are a generator set, which is fine)
            err(src, f"{kind} '{label}' is not connected to any wheel — it cannot move the "
                     f"vehicle.")
            path_errors = True
    any_wheels = any(seg.wheels for dl in model.drivelines for seg in dl.segments)
    driven = any(reaches_load(dl) and any(typ(e) in PROPULSION for e in dl.element_group)
                 for dl in model.drivelines)
    if vehicle_model and any_wheels and not driven and not path_errors:
        add("error", "No E-Motor or Engine is connected to the wheels — the vehicle cannot "
                     "move." if sources else
                     "The model has no E-Motor or Engine — nothing drives the wheels.")

    def side_reaches_load(joint: str, port: str) -> bool:
        seen, stack = {joint}, list(mech_peers.get((joint, port), []))
        while stack:
            e = stack.pop()
            if e in seen:
                continue
            seen.add(e)
            if typ(e) in LOAD_TYPES:
                return True
            stack.extend(mech_nbrs[e] - seen)
        return False

    for j in of_type(*SPLIT_TYPES):
        if j not in dl_of or bool(model.params_of[j].get("locked", False)):
            continue
        outs = [p for p in cdef_of[j].ports if p.id in ("flange_out_a", "flange_out_b")]
        free = [p.name for p in outs if not side_reaches_load(j, p.id)]
        if len(free) == 1:
            err(j, f"'{elements[j].label}': {free[0]} reaches no wheel — an open "
                   f"{cdef_of[j].name.lower()} then passes no drive torque to its other "
                   f"output either. Connect a wheel there or lock it.")

    # -- brakes ---------------------------------------------------------------------
    wheels = [(w.el_id, bool(seg.brakes))
              for dl in model.drivelines for seg in dl.segments for w in seg.wheels]
    n_braked = sum(1 for _, braked in wheels if braked)
    if vehicle_model and 0 < n_braked < len(wheels):
        for w, braked in wheels:
            if not braked:
                warn(w, f"'{elements[w].label}' has no brake ({n_braked} of {len(wheels)} "
                        f"wheels have one) — the Driver's brake command cannot slow it.")
    for b in of_type("mech.brake"):
        if (b, "sig_demand_in") not in route:
            warn(b, f"Brake '{elements[b].label}' has no Brake Command signal — it will "
                    f"never apply.")

    # -- commands: a speed demand must reach the motors and engines ----------------
    fed_by: dict[str, list[tuple[str, str]]] = defaultdict(list)  # element → inputs it feeds
    for dst, src_port in route.items():
        fed_by[src_port[0]].append(dst)

    def commanded(start: str) -> set[tuple[str, str]]:
        """Inputs `start` feeds, directly or through controllers."""
        seen, stack, hit = {start}, [start], set()
        while stack:
            for dst in fed_by.get(stack.pop(), ()):
                hit.add(dst)
                if dst[0] not in seen and typ(dst[0]) in CONTROLLER_TYPES:
                    seen.add(dst[0])
                    stack.append(dst[0])
        return hit

    demands = {(s, PROPULSION[typ(s)][1]) for s in sources}
    for src in sources:
        kind, port, name, effect = PROPULSION[typ(src)]
        if (src, port) not in route:
            err(src, f"{kind} '{elements[src].label}' has no {name} signal — {effect}.")
    tasks = of_type("signal.driving_task")
    drv = model.driver
    if drv is not None:
        label = elements[drv].label
        if (drv, "sig_target_in") not in route:
            err(drv, f"Driver '{label}' has no Target Speed signal — it will hold 0 km/h, "
                     f"so the vehicle will not move.")
        if demands & route.keys() and not commanded(drv) & demands:
            err(drv, f"Driver '{label}' does not command any E-Motor or Engine — wire its "
                     f"Traction Command to them, directly or through a controller.")
        for t in tasks:
            if not fed_by.get(t):
                warn(t, f"Driving Task '{elements[t].label}' is not wired to anything — its "
                        f"speed profile is not used.")
    elif vehicle_model and tasks and sources and not any(commanded(t) & demands for t in tasks):
        err(tasks[0], f"No Driver follows the Driving Task '{elements[tasks[0]].label}' — add "
                      f"a Driver, wire the task to its Target Speed and its Traction Command "
                      f"to the powertrain.")
        replaced.add(NO_DRIVER)

    # -- actuator and block inputs that silently fall back to a fixed value --------
    for g in of_type("mech.gearbox"):
        if g in dl_of and (g, "sig_gear_in") not in route:
            gear = model.params_of[g].get("default_gear", 1)
            warn(g, f"Gearbox '{elements[g].label}' has no Gear Select signal — it stays in "
                    f"gear {gear} for the whole run.")
    for c in of_type("mech.clutch"):
        if c in dl_of and (c, "sig_engage_in") not in route:
            warn(c, f"Clutch '{elements[c].label}' has no Engagement signal — it stays fully "
                    f"engaged for the whole run.")
    for blk in model.signal_blocks:
        t = typ(blk)
        if t == "signal.road_profile":
            continue  # reads the vehicle's distance when unwired
        ins = [p for p in ports_of(elements[blk], cdef_of[blk])
               if p.kind == "signal" and p.direction == "input"]
        if t == "signal.lookup" and str(model.params_of[blk].get("mode", "1D")) != "2D":
            ins = [p for p in ins if p.id != "sig_y_in"]
        loose = [p.name for p in ins if (blk, p.id) not in route]
        if loose:
            names = ", ".join(f"'{n}'" for n in loose)
            warn(blk, f"'{elements[blk].label}' input{'s' if len(loose) > 1 else ''} {names} "
                      f"{'are' if len(loose) > 1 else 'is'} not connected — it reads 0 there.")
    return replaced


def _plausibility_checks(model: Model, add: Add) -> None:
    """Warnings for vehicle parameters far outside what real vehicles use."""
    def num(params: dict, key: str) -> float | None:
        try:
            return float(params[key])
        except (KeyError, TypeError, ValueError):
            return None

    for el_id, cdef in model.cdef_of.items():
        p, el = model.params_of[el_id], model.elements[el_id]
        if cdef.id == "vehicle.body":
            mass = num(p, "mass_kg")
            lo, hi = VEHICLE_MASS_KG
            if mass is not None and mass > 0 and not lo <= mass <= hi:
                add("warning", f"Vehicle '{el.label}' has a mass of {mass:g} kg, outside the "
                               f"range of road vehicles ({lo:g} kg to {hi / 1000:g} t) — check "
                               f"the value and its unit.", el)
        elif cdef.id == "battery.generic":
            cap = num(p, "capacity_kWh")
            lo, hi = BATTERY_KWH
            if cap is not None and 0 < cap < lo:
                add("warning", f"'{el.label}' has a capacity of {cap:g} kWh, too small for a "
                               f"traction battery (below {lo:g} kWh) — it would be empty "
                               f"within seconds.", el)
            elif cap is not None and cap > hi:
                add("warning", f"'{el.label}' has a capacity of {cap:g} kWh, far above any "
                               f"vehicle battery ({hi:g} kWh) — check the value and its "
                               f"unit.", el)
            soc0, soc_min = num(p, "initial_soc_pct"), num(p, "min_soc_pct")
            if soc0 is not None and soc_min is not None and soc0 <= soc_min:
                add("warning", f"'{el.label}' starts at {soc0:g} % SOC, at or below its "
                               f"minimum of {soc_min:g} % — it can deliver no energy.", el)
        elif cdef.id == "electric.constant_drive" and model.vehicle is not None:
            kw = num(p, "power_kW")
            if kw is not None and kw > AUX_LOAD_MAX_KW:
                add("warning", f"'{el.label}' draws a constant {kw:g} kW — vehicle "
                               f"auxiliaries use about 0.3–5 kW (up to about 30 kW for a "
                               f"bus's heating and air conditioning). Check the value and "
                               f"its unit.", el)
        elif cdef.id == "mech.final_drive":
            ratio = num(p, "ratio")
            if ratio is not None and ratio > FINAL_DRIVE_MAX_RATIO:
                add("warning", f"'{el.label}' has a ratio of {ratio:g}, far above road "
                               f"vehicles' final drives (about 2–15) — check the value.", el)

    shares = [w.load_share for dl in model.drivelines for seg in dl.segments for w in seg.wheels]
    total = sum(shares) * 100.0
    if model.vehicle is not None and shares and abs(total - 100.0) > 0.5:
        add("warning", f"Wheel load shares add up to {total:g} %, not 100 % — the solver rests "
                       f"only that share of the vehicle's weight on the wheels, which scales "
                       f"rolling resistance and grip.")
