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
from typing import Callable, Iterable

from .library import library_by_id
from .schemas import DataCheck, ElementInstance, ParameterDef, Project
from .solver import (
    Model,
    ModelError,
    ScriptError,
    TableError,
    build_model,
    check_script,
    interp1,
    lapsim,
    motor_max_rpm,
    parse_table1d,
    parse_table2d,
    profile_problems,
)
from .solver.battery import CELLS
from .solver.maps import inner_range
from .solver.network import (
    AXLE_GEAR_TYPES,
    NO_DRIVER,
    NO_VEHICLE,
    NO_WHEELS,
    ROAD_LOAD_ABC,
    SIGNAL_BLOCK_TYPES,
    ports_of,
)
from .solver.runtime import AMBIENT_C, AMBIENT_KPA, GRAVITY, RPM, air_density, ocv_mean
from .solver.scaling import VALID_RANGE, max_speed_rpm, scale_keys, scaled
from .tyre import parse_tyre_code

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
WHEEL_SHARE_TOL_PCT = 1.0  # wheel load shares within 100 ± this are left unremarked

Add = Callable[..., None]

# Script / PID / Lookup blocks may run slower than the solver step; above
# this their sampling starts to shape the results (real vehicle controllers
# run every 10-100 ms).
COARSE_SAMPLE_TIME_S = 0.1


def _as_number(value) -> float | None:
    try:
        return float(value)
    except (TypeError, ValueError, OverflowError):
        return None


def _name(pdef: ParameterDef) -> str:
    """A parameter's label without its note: 'Charge Capacity (0 = …)'."""
    return pdef.label.split(" (")[0]


def _number_problem(pdef: ParameterDef, value) -> str | None:
    """What is wrong with a number parameter's value, in words, or None."""
    v = _as_number(value)
    if v is None:
        return "is not a number"
    problem = pdef.range_problem(v) if math.isfinite(v) else "must be a finite number"
    return f"{problem} — got {v:g}" if problem else None


def validate_project(project: Project) -> list[DataCheck]:
    checks: list[DataCheck] = []
    defs = library_by_id()

    def add(level: str, text: str, el: ElementInstance | None = None, *,
            ids: Iterable[str | None] = (), fix: str | None = None) -> None:
        # every part it is about, `el` first
        about = list(dict.fromkeys(i for i in (el.id if el else None, *ids) if i in all_elements))
        first = el or (all_elements[about[0]] if about else None)
        checks.append(DataCheck(
            level=level,  # type: ignore[arg-type]
            text=text,
            elementId=first.id if first else None,
            elementLabel=first.label if first else None,
            elementIds=about,
            fix=fix,
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
            # the diagram cannot draw such a wire, so it goes with a part at its end
            ends = [all_elements[i] for i in (conn.sourceElementId, conn.targetElementId)
                    if i in all_elements]
            for el_id, port_id in ((conn.sourceElementId, conn.sourcePortId),
                                   (conn.targetElementId, conn.targetPortId)):
                if el_id not in all_elements:
                    add("error", f"Connection '{conn.id}' references missing element '{el_id}'.",
                        ids=(conn.sourceElementId, conn.targetElementId),
                        fix=f"The diagram cannot show this wire: delete '{ends[0].label}' (the "
                            "wire goes with it) and add it again." if ends else None)
                elif port_of(el_id, port_id) is None:
                    add("error",
                        f"Connection '{conn.id}' references missing port '{port_id}' "
                        f"on '{all_elements[el_id].label}'.", all_elements[el_id],
                        fix=f"The diagram cannot show this wire: delete '{all_elements[el_id].label}' "
                            "(the wire goes with it) and add it again.")
            pa = port_of(conn.sourceElementId, conn.sourcePortId)
            pb = port_of(conn.targetElementId, conn.targetPortId)
            if pa and pb and pa.kind != pb.kind and "signal" not in (pa.kind, pb.kind):
                add("error",
                    f"Connection between '{all_elements[conn.sourceElementId].label}' and "
                    f"'{all_elements[conn.targetElementId].label}' mixes incompatible port "
                    f"kinds ({pa.kind} ↔ {pb.kind}).",
                    ids=(conn.sourceElementId, conn.targetElementId))
            elif pa and pb and pa.kind == pb.kind and pa.kind in ("thermal", "fluid"):
                add("warning",
                    f"Connection between '{all_elements[conn.sourceElementId].label}' and "
                    f"'{all_elements[conn.targetElementId].label}' is a {pa.kind} connection — "
                    f"this version has no {pa.kind} solver, so it is ignored during simulation.",
                    ids=(conn.sourceElementId, conn.targetElementId))

    for dbc in project.dataBusConnections:
        p1 = port_of(dbc.element1Id, dbc.port1Id)
        p2 = port_of(dbc.element2Id, dbc.port2Id)
        if p1 is None or p2 is None:
            add("error", "Data bus connection references a missing element or port.",
                ids=(dbc.element1Id, dbc.element2Id),
                fix="Remove it in Data Bus Connections and link the signal again.")
            continue
        if p1.direction == p2.direction and p1.direction in ("input", "output"):
            add("warning",
                f"Data bus connection links two {p1.direction}s "
                f"('{all_elements[dbc.element1Id].label}.{p1.name}' ↔ "
                f"'{all_elements[dbc.element2Id].label}.{p2.name}') — no data will flow.",
                ids=(dbc.element1Id, dbc.element2Id),
                fix="Remove it in Data Bus Connections: a link runs from an output to an input.")

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
        for pdef in cdef.parameters:  # number limits, from the catalogue
            if pdef.type == "number" and (problem := _number_problem(pdef, params[pdef.key])):
                add("error", f"{_name(pdef)} of '{el.label}' {problem}.", el)

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

        # a Driving Task on a drive cycle drives the cycle, not its typed
        # profile; an unknown cycle is build_model's error (below)
        if cdef.id == "signal.road_profile" or (cdef.id == "signal.driving_task"
                                                 and not params.get("cycle")):
            for level, text in profile_problems(str(params.get("profile", ""))):
                add(level, f"'{el.label}' profile: {text}.", el)
        ts = _as_number(params.get("sample_time_s", 0))  # a wrong one is an error above
        if ts is not None and COARSE_SAMPLE_TIME_S < ts < math.inf:
            add("warning",
                f"'{el.label}' runs only every {ts:g} s (its Sample Time); real vehicle "
                f"controllers run every 10-100 ms, so results may depend on this "
                f"setting.", el)

    # a case's own values (Cases & Parameters) keep to the same limits
    for case in project.cases:
        for el_id, values in case.parameterOverrides.items():
            el = all_elements.get(el_id)
            cdef = defs.get(el.componentDefId) if el else None
            for pdef in cdef.parameters if cdef else ():
                if pdef.type == "number" and pdef.key in values \
                        and (problem := _number_problem(pdef, values[pdef.key])):
                    add("error", f"{_name(pdef)} of '{el.label}' in case '{case.name}' {problem}.",
                        el, fix="Change or remove the override in Cases & Parameters.")

    # -- structural solvability (delegated to model extraction) ------------------
    model = None
    try:
        model = build_model(project)
    except ModelError as e:
        for text in e.errors:
            add("error", text, ids=e.involved.get(text, ()), fix=_model_fix(text))
    if model is not None:
        replaced = _drive_checks(project, model, add)
        for text in model.warnings:
            if text not in replaced:
                add("warning", text, ids=model.involved.get(text, ()), fix=_model_fix(text))

        for el_id in model.floating_returns:
            el = all_elements.get(el_id)
            if el is not None:
                add("info",
                    f"'{el.label}' has an unconnected negative (−) terminal — using an "
                    f"implicit ground return. Wire it to Ground for an explicit return path.",
                    el)

        _plausibility_checks(model, add)
        _map_checks(model, add)
        _lap_checks(project, add)

        if not model.drivelines and not any(b.consumers for b in model.buses):
            add("info", "Model has no driveline and no electrical loads — nothing will happen.")

    if not any(s.elements for s in project.systems):
        add("info", "Model is empty — drag components from the library onto the canvas.")
    if not project.cases:
        add("warning", "Project has no simulation case defined.",
            fix="On the Simulations tab, click + (Add case).")

    if not checks:
        add("info", "All data checks passed: wiring, power supply, drive path, command "
                    "signals and key parameter ranges were checked. Data Checks cannot tell "
                    "whether the results will be plausible — review them after the run.")
    return checks


# what to do about build_model's (and lapsim.problems') messages, by words they contain
# ponytail: word match; codes on the messages replace it with VAL-10
MODEL_FIXES = {
    "needs both outputs connected": "Connect a shaft or wheel to each of its outputs.",
    "needs both flanges connected": "Connect both of its flanges to the driveline.",
    "without a Fuel Tank": "Add a Fuel Tank from the library (ICE Powertrain).",
    "without a Hydrogen Tank": "Add a Hydrogen Tank from the library (Fuel Cell).",
    "element per model is supported": "Delete the extra ones.",
    "Bus has two": "Keep one source per bus, or split the bus with a DC-DC Converter.",
    "more than one primary source": "Keep one source per bus, or split the bus with a DC-DC "
                                    "Converter.",
    "which this version of LightSim does not include": "In Properties, choose a Drive Cycle "
                                                       "from the list, or Custom profile.",
    "A lap case needs a Vehicle": "Add a Vehicle from the library (Vehicle).",
    "A lap case needs a Driver": "Add a Driver from the library (Vehicle).",
    "A lap case needs an E-Motor": "Connect an E-Motor to the wheels' driveline.",
    "drives E-Motors only": "Set the case's Kind to Cycle, or drive the wheels with E-Motors "
                            "only.",
    "does not shift gears": "Set its Default Gear to the gear the lap should be driven in.",
    "has no layout": "Choose a Layout from the list in Properties.",
}


def _model_fix(text: str) -> str | None:
    return next((fix for words, fix in MODEL_FIXES.items() if words in text), None)


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

    def err(el_id: str, text: str, fix: str | None = None) -> None:
        add("error", text, elements[el_id], fix=fix)

    def warn(el_id: str, text: str, fix: str | None = None) -> None:
        add("warning", text, elements[el_id], fix=fix)

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
                        f"it out{note}.",
                 fix="Wire it into the model, or delete it.")
        elif plus:
            loose = [p.name for p in plus if (el_id, p.id) not in wired]
            if loose:
                warn(el_id, f"'{label}' has nothing connected to its {' or '.join(loose)}, so "
                            f"the simulation leaves it out.",
                     fix="Wire it to the electrical bus (a battery's + terminal or an Electric "
                         "Node).")
        elif cdef.id == "electric.node" and not any(e == el_id for e, _ in wired):
            warn(el_id, f"'{label}' is not connected to anything.",
                 fix="Wire it into the bus, or delete it.")

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
            err(m, f"E-Motor '{elements[m].label}' has no live electrical connection.",
                fix="Wire its + terminal to a bus that has a battery, fuel cell or voltage source.")
    for bus in model.buses:
        primary = bus.battery or bus.vsource or bus.fuelcell
        if primary and not (bus.motors or bus.consumers or bus.dcdc_in):
            warn(primary, f"'{elements[primary].label}' supplies nothing: no E-Motor, load or "
                          f"DC-DC converter is connected to its bus.",
                 fix="Wire an E-Motor, a Power Consumer or a DC-DC Converter to its bus, or "
                     "delete it.")
        if supplied(bus):
            continue
        for m in bus.motors:
            err(m, f"E-Motor '{elements[m].label}' has no power source: nothing on its "
                   f"electrical bus is a battery, fuel cell or voltage source, so it "
                   f"produces no torque.",
                fix="Add a battery, fuel cell or voltage source to its bus, or feed the bus "
                    "from one through a DC-DC Converter.")
        for c in bus.consumers:
            warn(c, f"'{elements[c].label}' is on an electrical bus with no power source — "
                    f"its demand is not met.",
                 fix="Add a battery, fuel cell or voltage source to its bus.")

    # -- drive path: motors and engines must reach the wheels ---------------------
    for text in model.warnings:
        if text in (NO_VEHICLE, NO_WHEELS):
            add("error", text, ids=model.involved.get(text, ()),
                fix="Add a Vehicle from the library (Vehicle group); it carries the wheels."
                if text == NO_VEHICLE else "Connect the wheels to the driveline.")
            replaced.add(text)
    path_errors = False
    for src in sources:
        kind, label = PROPULSION[typ(src)][0], elements[src].label
        dl = dl_of.get(src)
        if dl is None:
            err(src, f"{kind} '{label}' is not mechanically connected — it cannot drive "
                     f"anything.",
                fix="Wire its shaft into the driveline that leads to the wheels.")
            path_errors = True
        elif (vehicle_model and not reaches_load(dl)
              and sum(1 for e in dl.element_group if typ(e) in PROPULSION) < 2):
            # (two sources on a wheel-less shaft are a generator set, which is fine)
            err(src, f"{kind} '{label}' is not connected to any wheel — it cannot move the "
                     f"vehicle.",
                fix="Connect its driveline to the wheels (through a Final Drive and "
                    "Differential).")
            path_errors = True
    any_wheels = any(seg.wheels for dl in model.drivelines for seg in dl.segments)
    driven = any(reaches_load(dl) and any(typ(e) in PROPULSION for e in dl.element_group)
                 for dl in model.drivelines)
    if vehicle_model and any_wheels and not driven and not path_errors:
        add("error", "No E-Motor or Engine is connected to the wheels — the vehicle cannot "
                     "move." if sources else
                     "The model has no E-Motor or Engine — nothing drives the wheels.",
            ids=sources or [model.vehicle],
            fix="Connect a motor or engine shaft through the driveline to the wheels." if sources
            else "Add an E-Motor or Engine from the library and connect it to the wheels.")

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
                        f"wheels have one) — the Driver's brake command cannot slow it.",
                     fix="Add a Brake on this wheel's shaft.")
    for b in of_type("mech.brake"):
        if (b, "sig_demand_in") not in route:
            warn(b, f"Brake '{elements[b].label}' has no Brake Command signal — it will "
                    f"never apply.",
                 fix=f"In Data Bus Connections, pick the Driver's Brake Command as the source "
                     f"of {elements[b].label} · Brake Command.")

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
            err(src, f"{kind} '{elements[src].label}' has no {name} signal — {effect}.",
                fix=f"In Data Bus Connections, pick a source for {elements[src].label} · {name} "
                    f"(usually the Driver's Traction Command, or a controller's output).")
    tasks = of_type("signal.driving_task")
    drv = model.driver
    if drv is not None:
        label = elements[drv].label
        # an acceleration test holds full throttle and a lap case follows its
        # Race Track: neither reads a target
        if ((drv, "sig_target_in") not in route
                and any(c.kind not in ("acceleration", "lap") for c in project.cases)):
            err(drv, f"Driver '{label}' has no Target Speed signal — it will hold 0 km/h, "
                     f"so the vehicle will not move.",
                fix=f"In Data Bus Connections, pick a Driving Task's Target Speed as the source "
                    f"of {label} · Target Speed.")
        if demands & route.keys() and not commanded(drv) & demands:
            err(drv, f"Driver '{label}' does not command any E-Motor or Engine — wire its "
                     f"Traction Command to them, directly or through a controller.")
        for t in tasks:
            if not fed_by.get(t):
                warn(t, f"Driving Task '{elements[t].label}' is not wired to anything — its "
                        f"speed profile is not used.",
                     fix=f"In Data Bus Connections, pick it as the source of {label} · "
                         f"Target Speed.")
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
                    f"gear {gear} for the whole run.",
                 fix=f"In Data Bus Connections, pick a source for {elements[g].label} · Gear "
                     f"Select (a Script, Lookup Table or Constant).")
    for c in of_type("mech.clutch"):
        if c in dl_of and (c, "sig_engage_in") not in route:
            warn(c, f"Clutch '{elements[c].label}' has no Engagement signal — it stays fully "
                    f"engaged for the whole run.",
                 fix=f"In Data Bus Connections, pick a source for {elements[c].label} · "
                     f"Engagement, or leave it engaged.")
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
                      f"{'are' if len(loose) > 1 else 'is'} not connected — it reads 0 there.",
                 fix=f"In Data Bus Connections, pick a source for each (search the list for "
                     f"{elements[blk].label}).")
    return replaced


def _lap_checks(project: Project, add: Add) -> None:
    """What a lap case refuses or does differently (lapsim.problems), for
    the Race Track, layout and laps that case sets."""
    for case in project.cases:
        if case.kind != "lap":
            continue
        try:
            model = build_model(project, {}, case.parameterOverrides)
        except ModelError:
            continue  # reported above
        track = model.elements.get(model.track) if model.track else None
        for level, text, parts in lapsim.problems(model, case.outputEvery):
            add(level, f"Case '{case.name}': {text}", None if parts else track, ids=parts,
                fix=_model_fix(text))


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
            h, wheelbase = num(p, "cg_height_m"), num(p, "wheelbase_m")
            if h is not None and wheelbase is not None and 0 < wheelbase < h:
                add("warning", f"Vehicle '{el.label}' has a Centre of Gravity Height of {h:g} m, "
                               f"above its Wheelbase of {wheelbase:g} m — check the value and "
                               f"its unit (a car's is about a fifth to a quarter of its "
                               f"wheelbase).", el)
            mass = num(p, "mass_kg")
            lo, hi = VEHICLE_MASS_KG
            if mass is not None and mass > 0 and not lo <= mass <= hi:
                add("warning", f"Vehicle '{el.label}' has a mass of {mass:g} kg, outside the "
                               f"range of road vehicles ({lo:g} kg to {hi / 1000:g} t) — check "
                               f"the value and its unit.", el)
            pushing = [f"{name} of {v:g} {unit}" for name, key, unit in (
                ("A", "road_load_a_N", "N"), ("C", "road_load_c_N_per_kmh2", "N/(km/h)²"))
                if (v := num(p, key)) is not None and v < 0]
            if p.get("road_load_mode") == ROAD_LOAD_ABC and pushing:
                add("warning", f"Vehicle '{el.label}' has a road-load {' and '.join(pushing)} "
                               f"— a negative A or C pushes the car along, so it speeds up "
                               f"when it coasts. Check the sign.", el)
            if (p.get("road_load_mode") == ROAD_LOAD_ABC
                    and not p.get("abc_include_driveline_losses", True)):
                gears = [(g, num(model.params_of[g], "efficiency_pct"))
                         for g, c in model.cdef_of.items() if c.id in AXLE_GEAR_TYPES]
                lossy = [f"'{model.elements[g].label}' {eff:g} %"
                         for g, eff in gears if eff is not None and eff < 100]
                if lossy:
                    add("warning", f"Vehicle '{el.label}' takes its road load from coefficients "
                                   f"A/B/C, which a coast-down measures with the axle's drag in "
                                   f"them, and the axle's gears lose it again: "
                                   f"{', '.join(lossy)}. Tick 'Coefficients Include Driveline "
                                   f"Losses' unless these are dyno-set coefficients.", el)
        elif cdef.id == "boundary.ambient" and el_id == model.ambient:  # the others are unused
            t_c, p_kpa = num(p, "temperature_C"), num(p, "pressure_kPa")
            if t_c is not None and p_kpa is not None and t_c > -273.15 and p_kpa > 0:
                out = []
                if not AMBIENT_C[0] <= t_c <= AMBIENT_C[1]:
                    out.append(f"a temperature of {t_c:g} °C ({AMBIENT_C[0]:g} to {AMBIENT_C[1]:g} "
                               f"°C is usual)")
                if not AMBIENT_KPA[0] <= p_kpa <= AMBIENT_KPA[1]:
                    out.append(f"a pressure of {p_kpa:g} kPa ({AMBIENT_KPA[0]:g} to "
                               f"{AMBIENT_KPA[1]:g} kPa is usual; 1 bar = 100 kPa)")
                if out:
                    add("warning", f"'{el.label}' has {' and '.join(out)}, which gives the "
                                   f"Vehicle's drag an air density of "
                                   f"{air_density(t_c, p_kpa):.3g} kg/m³ — check the value "
                                   f"and its unit.", el)
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
            cells = p.get("pack_model") == CELLS
            n_s = max(1, round(num(p, "series_cells") or 96)) if cells else 1
            table = "cell_ocv_table" if cells else "ocv_table"
            if cells:
                _cell_pack_checks(p, el, add)
            v_class = num(p, "voltage_class_V")
            if v_class is not None and v_class > 0:
                try:  # read at 100 % as the run reads it
                    ocv = parse_table1d(p.get(table))
                except TableError:
                    ocv = []  # reported with the other parameters
                linear = (el.tableOutside.get(table) or [""])[0] == "linear"
                v_full = n_s * interp1(ocv, 100.0, linear) if ocv else 0.0
                if v_full > v_class:
                    add("warning", f"'{el.label}' reaches {v_full:g} V open-circuit at 100 % "
                                   f"SOC, above its Voltage Class of {v_class:g} V — fewer cells "
                                   f"in series, or check the class.", el)
        elif cdef.id == "electric.constant_drive" and model.vehicle is not None:
            kw = num(p, "power_kW")
            if kw is not None and kw > AUX_LOAD_MAX_KW:
                add("warning", f"'{el.label}' draws a constant {kw:g} kW — vehicle "
                               f"auxiliaries use about 0.3–5 kW (up to about 30 kW for a "
                               f"bus's heating and air conditioning). Check the value and "
                               f"its unit.", el)
        elif cdef.id in ("motor.emotor", "engine.combustion") and any(
                (num(p, k) or 100.0) != 100.0 for k in scale_keys(cdef.id)):
            _scale_checks(cdef.id, p, el, add)
        elif cdef.id == "electric.climate" and model.ambient is None:
            add("info", f"'{el.label}' takes the outside temperature from an Ambient, and the "
                        f"model has none: it runs at 20 °C, where it neither heats nor cools. "
                        f"Add an Ambient (Boundaries) and set its Temperature.", el)
        elif cdef.id == "mech.final_drive":
            ratio = num(p, "ratio")
            if ratio is not None and ratio > FINAL_DRIVE_MAX_RATIO:
                add("warning", f"'{el.label}' has a ratio of {ratio:g}, far above road "
                               f"vehicles' final drives (about 2–15) — check the value.", el)

    wheels = [w for dl in model.drivelines for seg in dl.segments for w in seg.wheels]
    total = sum(w.load_share for w in wheels) * 100.0
    if model.vehicle is not None and wheels and total <= 0:
        add("error", "Wheel load shares add up to 0 % — no wheel carries the vehicle's "
                     "weight, so it has no grip and cannot move. Give the wheels their share "
                     "of the weight (together 100 %).", ids=[w.el_id for w in wheels])
    elif model.vehicle is not None and wheels and abs(total - 100.0) > WHEEL_SHARE_TOL_PCT:
        split = ", ".join(f"'{model.elements[w.el_id].label}' {w.load_share * 1e4 / total:.3g} %"
                          for w in wheels)
        add("warning", f"Wheel load shares add up to {total:g} %, not 100 % — the solver scales "
                       f"them so the wheels carry the vehicle's whole weight: {split}. Set "
                       f"them to add up to 100 % to choose the split yourself.",
            ids=[w.el_id for w in wheels])
    _tyre_checks(model, wheels, add)
    h = num(model.params_of[model.vehicle], "cg_height_m") if model.vehicle is not None else None
    if wheels and h is not None and h > 0 and len({w.axle for w in wheels}) < 2:
        veh, on = model.elements[model.vehicle], wheels[0].axle
        fix = "Front on the front" if on == "Rear" else "Rear on the rear"
        add("error", f"Vehicle '{veh.label}' has a Centre of Gravity Height of {h:g} m, but all "
                     f"its wheels are on the {on} axle, so no load can shift between "
                     f"axles. Set Axle to {fix} wheels.", veh, ids=[w.el_id for w in wheels])


def _tyre_checks(model: Model, wheels: list, add: Add) -> None:
    """Wheels with a Tyre Code (MOD-48): a code that is not one, a radius
    far from the code's, and a wheel carrying more than its load index."""
    total = sum(w.load_share for w in wheels)
    mass = None
    if model.vehicle is not None:
        try:
            mass = float(model.params_of[model.vehicle].get("mass_kg", 1800))
        except (TypeError, ValueError):
            mass = None
    for w in wheels:
        p, el = model.params_of[w.el_id], model.elements[w.el_id]
        code = str(p.get("tyre_code", "") or "").strip()
        if not code:
            continue
        spec = parse_tyre_code(code)
        if spec is None:
            add("warning", f"'{el.label}' has a Tyre Code '{code}' LightSim cannot read: write it "
                           f"as on the sidewall, e.g. 205/55 R16 91V or 20.5x7.0-13.", el)
            continue
        try:
            factor = float(p.get("rolling_radius_factor", 0.97))
        except (TypeError, ValueError):
            factor = 0.97
        expected = spec.unloaded_radius_m * factor
        if abs(w.radius - expected) > 0.03 * expected:
            add("info", f"'{el.label}' has a Wheel Radius of {w.radius:g} m, but its tyre "
                        f"{spec.code} rolls on about {expected:.3f} m — retype the code to fill "
                        f"it in, or check the radius.", el)
        max_load = spec.max_load_n
        if max_load and mass and total > 0:
            load = mass * GRAVITY * w.load_share / total
            if load > max_load:
                add("warning", f"'{el.label}' carries {load / GRAVITY:,.0f} kg standing still, more "
                               f"than its tyre's load index {spec.load_index} allows "
                               f"({max_load / GRAVITY:,.0f} kg): fit a tyre with a higher load "
                               f"index, or check the Vehicle Mass and the load shares.", el)


def _scale_checks(part: str, p: dict, el, add: Add) -> None:
    """A resized motor or engine (MOD-47): the scaled machine next to the
    original (info), and scales outside the range the rules hold for."""
    lo, hi = VALID_RANGE
    for key in scale_keys(part):
        try:
            k = float(p.get(key, 100)) / 100.0
        except (TypeError, ValueError):
            continue
        if k > 0 and not lo <= k <= hi:
            add("warning", f"'{el.label}' is scaled to {k * 100:g} % ({key.replace('_pct', '')}), "
                           f"outside the {lo * 100:g}–{hi * 100:g} % the scaling rules are meant "
                           f"for: a machine this much smaller or larger is built differently. "
                           f"Use its own maps instead.", el)
    try:
        if part == "motor.emotor":
            raw = parse_table2d(p.get("full_load_torque"))
            new = scaled(part, "full_load_torque", raw, p)
            peak = (max(t for _, row in raw for _, t in row), max(t for _, row in new for _, t in row))
            power = tuple(max(t * n / RPM for _, row in fl for n, t in row) / 1000.0
                          for fl in (raw, new))
            n_top = (motor_max_rpm(raw, p.get("max_speed_rpm", 0)), motor_max_rpm(new, max_speed_rpm(p)))
            add("info", f"'{el.label}' is resized: peak torque {peak[1]:,.0f} N·m (was "
                        f"{peak[0]:,.0f}), maximum speed {n_top[1]:,.0f} 1/min (was "
                        f"{n_top[0]:,.0f}), peak power {power[1]:,.0f} kW (was {power[0]:,.0f}); "
                        f"its loss map, drag and inertia are scaled with it.", el)
        else:
            raw = parse_table1d(p.get("full_load_torque"))
            new = scaled(part, "full_load_torque", raw, p)
            power = tuple(max(t * n / RPM for n, t in fl) / 1000.0 for fl in (raw, new))
            add("info", f"'{el.label}' is resized: peak torque {max(t for _, t in new):,.0f} N·m "
                        f"(was {max(t for _, t in raw):,.0f}), peak power {power[1]:,.0f} kW "
                        f"(was {power[0]:,.0f}); its fuel map, drag and inertia are scaled with "
                        f"it, at the same fuel use per kWh.", el)
    except (TableError, ValueError):
        pass  # reported with the other parameters


def _cell_pack_checks(p: dict, el, add: Add) -> None:
    """A battery built from cells (MOD-08): what its layout gives (info),
    and cell limits that leave the cells no room (warnings)."""
    def num(key: str, default: float) -> float:
        try:
            return float(p.get(key, default))
        except (TypeError, ValueError):
            return default

    n_s, n_p = max(1, round(num("series_cells", 96))), max(1, round(num("parallel_cells", 30)))
    try:
        ocv = parse_table1d(p.get("cell_ocv_table"))
    except TableError:
        return  # reported with the other parameters
    cap = num("cell_capacity_Ah", 5.0)
    r_pack = (n_s * num("cell_resistance_ohm", 0.02) / n_p + n_s * num("interconnect_resistance_ohm", 0.0002)
              + num("contactor_resistance_ohm", 0.0005))
    v_lo, v_hi = n_s * ocv[0][1], n_s * ocv[-1][1]
    kwh = n_p * cap * n_s * ocv_mean(ocv) / 1000.0
    mass = n_s * n_p * num("cell_mass_kg", 0.07) * max(1.0, num("packaging_factor", 1.4))
    add("info", f"'{el.label}' is built from cells: {n_s}s{n_p}p of {cap:g} Ah gives "
                f"{n_p * cap:g} Ah, {kwh:.1f} kWh, {v_lo:.0f}–{v_hi:.0f} V open-circuit, "
                f"{r_pack * 1000:.1f} mΩ for a 10 s pulse at 25 °C and 50 % SOC, and about "
                f"{mass:.0f} kg (an estimate; the Vehicle Mass is not changed).", el)
    v_min, v_max = num("cell_min_voltage_V", 2.5), num("cell_max_voltage_V", 4.2)
    if v_min > 0 and v_min >= ocv[-1][1]:
        add("warning", f"'{el.label}' has a Cell Minimum Voltage of {v_min:g} V, at or above "
                       f"the cell's open-circuit voltage when full ({ocv[-1][1]:g} V): it can "
                       f"give no current. Check the value.", el)
    if v_max > 0 and v_max <= ocv[0][1]:
        add("warning", f"'{el.label}' has a Cell Maximum Voltage of {v_max:g} V, at or below "
                       f"the cell's open-circuit voltage when empty ({ocv[0][1]:g} V): it can "
                       f"take no charge. Check the value.", el)


def _map_checks(model: Model, add: Add) -> None:
    """Do the maps fit each other and the parts around them? A run stops where
    it reads a table outside the data of an axis set to Error (the library's
    setting for motor and engine speed and torque), so these say before the
    run where the data does not reach: a motor's maximum speed and loss map
    against its full-load map, each bus's voltage against its motors' voltage
    axis, an engine's fuel map against its full-load curve and a fuel cell's
    Maximum Current against its curve. They also name outside-the-data
    settings that do not fit their table (error) and tables set not to stop
    where the library stops (info). Tables that do not parse are reported
    with the other parameters."""
    elements, params = model.elements, model.params_of

    def table(el_id: str, key: str, two_d: bool) -> list | None:
        try:  # as the run reads it: a resized machine's maps scaled (MOD-47)
            pts = (parse_table2d if two_d else parse_table1d)(params[el_id][key])
        except (KeyError, TableError):
            return None
        return scaled(model.cdef_of[el_id].id, key, pts, params[el_id])

    def num(el_id: str, key: str, default: float) -> float:
        try:
            return float(params[el_id].get(key, default))
        except (TypeError, ValueError):
            return default

    def fmt(x: float) -> str:
        return f"{x:,.0f}" if abs(x) >= 1000 else f"{x:g}"

    def span(lo: float, hi: float) -> str:
        return fmt(lo) if lo == hi else f"{fmt(lo)}–{fmt(hi)}"

    for el_id, cdef in model.cdef_of.items():
        el, label = elements[el_id], elements[el_id].label
        for key, own in el.tableOutside.items():
            pdef = next((p for p in cdef.parameters
                         if p.key == key and p.axes and p.axes[0].outside), None)
            if pdef is None or len(own) != len(pdef.axes or []):
                add("error", f"'{label}' has an outside-the-data setting for '{key}' that does "
                             f"not fit its tables (one setting per axis of a table) — the run "
                             f"ignores it.", el)
                continue
            relaxed = [f"{a.name} ({pol.capitalize()})" for a, pol in zip(pdef.axes, own)
                       if a.outside == "error" and pol != "error"]
            if relaxed:
                add("info", f"'{label}.{pdef.label}' does not stop the run outside its "
                            f"{' and '.join(relaxed)} data, as the library does — the run "
                            f"summary says how long and how far it went outside.", el)

        if cdef.id == "motor.emotor":
            fl, loss = table(el_id, "full_load_torque", True), table(el_id, "power_loss", True)
            if not fl or not loss:
                continue
            n_curve = motor_max_rpm(fl, 0)
            n_max = motor_max_rpm(fl, max_speed_rpm(params[el_id]))
            if n_max > n_curve:
                add("warning", f"E-Motor '{label}': its Maximum Speed ({fmt(n_max)} 1/min) is "
                               f"beyond its full-load data, which ends at {fmt(n_curve)} 1/min — "
                               f"lower it or extend the map.", el)
            rng = inner_range(fl)
            if rng and rng[0] > 0:
                add("warning", f"E-Motor '{label}': its full-load data starts at "
                               f"{fmt(rng[0])} 1/min but the motor starts from 0 — extend the "
                               f"map down to 0 1/min.", el)
            if len(loss) > 1 and math.isfinite(n_max) and (loss[0][0] > 0 or loss[-1][0] < n_max):
                add("warning", f"E-Motor '{label}': its loss map covers {span(loss[0][0], loss[-1][0])} "
                               f"1/min but the motor runs from 0 to its maximum speed of "
                               f"{fmt(n_max)} 1/min — extend the map.", el)
            rng = inner_range(loss)
            need = max(v for _, p in fl for _, v in p) * max(
                1.0, num(el_id, "q4_torque_scale_pct", 100.0) / 100.0)
            if rng and (rng[0] > 0 or rng[1] < need):
                add("warning", f"E-Motor '{label}': its loss map covers {span(*rng)} N·m but the "
                               f"motor gives up to {fmt(need)} N·m (full-load peak × generator "
                               f"torque scale) — extend the map.", el)
        elif cdef.id == "engine.combustion":
            efl, fm = table(el_id, "full_load_torque", False), table(el_id, "fuel_map", True)
            if not efl or not fm:
                continue
            if len(fm) > 1 and (fm[0][0] > efl[0][0] or fm[-1][0] < efl[-1][0]):
                add("warning", f"Engine '{label}': its fuel map covers {span(fm[0][0], fm[-1][0])} "
                               f"1/min but its full-load curve runs "
                               f"{span(efl[0][0], efl[-1][0])} 1/min — extend the map.", el)
            rng, peak = inner_range(fm), max(v for _, v in efl)
            if rng and (rng[0] > 0 or rng[1] < peak):
                add("warning", f"Engine '{label}': its fuel map covers {span(*rng)} N·m but the "
                               f"engine gives up to {fmt(peak)} N·m — extend the map.", el)
        elif cdef.id == "fuelcell.stack":
            pol, i_max = table(el_id, "polarization", False), num(el_id, "max_current_A", 400.0)
            if pol and len(pol) > 1 and pol[0][0] > 0:
                add("warning", f"Fuel cell '{label}': its polarization curve starts at "
                               f"{fmt(pol[0][0])} A but the stack starts from 0 A — extend the "
                               f"curve down to 0 A.", el)
            if pol and len(pol) > 1 and pol[-1][0] < i_max:
                add("warning", f"Fuel cell '{label}': its polarization curve covers "
                               f"{span(pol[0][0], pol[-1][0])} A but its Maximum Current is "
                               f"{fmt(i_max)} A — extend the curve or lower the Maximum Current.", el)

    # the voltage each bus's source holds against its motors' voltage axis
    for bus in model.buses:
        src = bus.battery or bus.vsource or bus.fuelcell or (bus.dcdc_out or [None])[0]
        if src is None or not bus.motors:
            continue
        if bus.battery:
            cells = params[src].get("pack_model") == CELLS
            ocv = table(src, "cell_ocv_table" if cells else "ocv_table", False)
            if not ocv:
                continue
            n_s = max(1, round(num(src, "series_cells", 96))) if cells else 1
            volts = (n_s * min(v for _, v in ocv), n_s * max(v for _, v in ocv))
        elif bus.fuelcell:
            pol = table(src, "polarization", False)
            if not pol:
                continue
            volts = (interp1(pol, num(src, "max_current_A", 400.0)), interp1(pol, 0.0))
        else:
            v = num(src, "voltage_V" if bus.vsource else "output_voltage_V", 0.0)
            volts = (v, v)
        for m in bus.motors:
            fl = table(m, "full_load_torque", True)
            if fl and len(fl) > 1 and (volts[0] < fl[0][0] or volts[1] > fl[-1][0]):
                add("warning", f"E-Motor '{elements[m].label}' is fed {span(*volts)} V by "
                               f"'{elements[src].label}', outside the "
                               f"{span(fl[0][0], fl[-1][0])} V of its full-load data.",
                    elements[m])
