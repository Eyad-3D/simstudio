"""A LightSim project you can read, change, check, run and save from Python.

Parts and their values are named the way the app shows them: a part by its
label (``"E-Motor"``) or its id (``"el-motor"``), a parameter by its key
after a dot (``"E-Motor.max_power_kw"``), a port the same way
(``"E-Motor.shaft"``). ``lightsim parts`` (or :func:`lightsim.library`)
lists the keys and ports of every part type.
"""
from __future__ import annotations

import copy
import os
import secrets
from pathlib import Path
from typing import Any, Callable, Iterable, Optional, Union

from . import units
from ._engine import engine
from .result import Check, Result, from_sim_result

PathLike = Union[str, os.PathLike]


class LightSimError(Exception):
    """A project, part, parameter, port or case that is not there, or a value
    that does not fit. The message says which, and what there is."""


def _uid(prefix: str) -> str:
    # the app's own ids are prefix-<7 base-36 characters>; any unique text works
    return f"{prefix}-{secrets.token_hex(4)}"


def read_project_file(path: Path):
    """The engine's Project read from a project file. The one place a file
    is turned into a project, so a newer file format (and its migrations)
    plugs in here."""
    schemas = engine("schemas")
    try:
        return schemas.Project.model_validate_json(path.read_bytes())
    except ValueError as e:
        raise LightSimError(f"'{path}' is not a LightSim project file: {e}") from None


def examples() -> list[str]:
    """The ids of the examples that come with LightSim (``bev-car`` …)."""
    paths = engine("paths")
    return sorted(p.stem for p in paths.EXAMPLES_DIR.glob("*.json"))


def library() -> dict[str, Any]:
    """Every part type: ``{component id: ComponentDef}`` (its parameters with
    key, label, unit and default, and its ports)."""
    return engine("library").library_by_id()


class Project:
    """A project, loaded with :meth:`load`. Changes stay in memory until
    :meth:`save`. ``model`` is the engine's own data (docs/spec/project.md)."""

    def __init__(self, model, path: Optional[Path] = None) -> None:
        self.model = model
        self.path = path

    # -- loading and saving ---------------------------------------------------
    @classmethod
    def load(cls, source: Union[PathLike, "Project"]) -> "Project":
        """A project from a file path, or an example by id (``"bev-car"``)."""
        if isinstance(source, Project):
            return source
        path = Path(source)
        if path.is_file():
            return cls(read_project_file(path), path)
        if str(source) in examples():
            ex = engine("paths").EXAMPLES_DIR / f"{source}.json"
            return cls(read_project_file(ex), None)  # an example: save() needs a path
        raise LightSimError(f"No project file '{source}' (and no example of that id; examples: "
                            f"{', '.join(examples())}).")

    def save(self, path: Optional[PathLike] = None) -> Path:
        """Write the project to ``path`` (default: the file it came from) in
        one step, so a crash never leaves half a file."""
        target = Path(path) if path is not None else self.path
        if target is None:
            raise LightSimError("This project came from an example: give save() a file path.")
        data = self.model.model_dump_json(indent=2).encode("utf-8")
        target.parent.mkdir(parents=True, exist_ok=True)
        engine("storage")._write_atomic(target, data)
        self.path = target
        return target

    def copy(self) -> "Project":
        """An independent copy (for variants in a loop)."""
        return Project(self.model.model_copy(deep=True), self.path)

    def to_dict(self) -> dict:
        return self.model.model_dump(mode="json")

    # -- what is in it ----------------------------------------------------------
    @property
    def name(self) -> str:
        return self.model.name

    @property
    def id(self) -> str:
        return self.model.id

    @property
    def elements(self) -> list:
        """Every part in every system (sub-systems included)."""
        return [e for s in self.model.systems for e in s.elements]

    @property
    def cases(self) -> list:
        return list(self.model.cases)

    def element(self, ref: str):
        """A part by id or label. Raises LightSimError when there is none, or
        when several parts share the label (use the id then)."""
        els = self.elements
        for e in els:
            if e.id == ref:
                return e
        found = [e for e in els if e.label == ref]
        if len(found) == 1:
            return found[0]
        if found:
            raise LightSimError(f"{len(found)} parts are labelled '{ref}': use an id "
                                f"({', '.join(e.id for e in found)}).")
        raise LightSimError(f"No part '{ref}'. Parts: {', '.join(sorted(e.label for e in els))}.")

    def case(self, ref: Optional[str] = None):
        """A case by id or name (names compare without case); None gives the
        first case."""
        cases = self.model.cases
        if not cases:
            raise LightSimError(f"Project '{self.name}' has no cases.")
        if ref is None:
            return cases[0]
        for c in cases:
            if c.id == ref:
                return c
        found = [c for c in cases if c.name.casefold() == str(ref).casefold()]
        if len(found) == 1:
            return found[0]
        names = ", ".join(f"'{c.name}' ({c.id})" for c in cases)
        raise LightSimError(f"No case '{ref}' in '{self.name}'. Cases: {names}.")

    def _definition(self, el):
        cdef = library().get(el.componentDefId)
        if cdef is None:
            raise LightSimError(f"Part '{el.label}' has the unknown type '{el.componentDefId}'.")
        return cdef

    def _param(self, ref: str):
        el_ref, _, key = ref.rpartition(".")
        if not el_ref:
            raise LightSimError(f"'{ref}' needs a part and a parameter: 'Part.parameter_key'.")
        el = self.element(el_ref)
        cdef = self._definition(el)
        pdef = next((p for p in cdef.parameters if p.key == key), None)
        if pdef is None:
            keys = ", ".join(p.key for p in cdef.parameters)
            raise LightSimError(f"Part '{el.label}' ({cdef.name}) has no parameter '{key}'. "
                                f"Its parameters: {keys}.")
        return el, pdef

    def get(self, ref: str, case: Optional[str] = None) -> Any:
        """A parameter's value (``"Vehicle.mass_kg"``): the case's own value
        when ``case`` is given and sets one, else the part's, else the
        library default. Numbers are in the parameter's unit (:meth:`unit`)."""
        el, pdef = self._param(ref)
        if case is not None:
            own = self.case(case).parameterOverrides.get(el.id, {})
            if pdef.key in own:
                return copy.deepcopy(own[pdef.key])
        return copy.deepcopy(el.parameterOverrides.get(pdef.key, pdef.default))

    def unit(self, ref: str) -> str:
        """The unit a parameter's value is in ("-" when it has none)."""
        return self._param(ref)[1].unit

    def _value(self, pdef, value: Any) -> Any:
        if pdef.type == "number":
            if isinstance(value, bool):
                raise LightSimError(f"'{pdef.label}' takes a number in {pdef.unit}.")
            if isinstance(value, str):
                try:
                    value = units.to_unit(value, pdef.unit)
                except units.UnitError as e:
                    raise LightSimError(f"'{pdef.label}': {e}") from None
            if not isinstance(value, (int, float)):
                raise LightSimError(f"'{pdef.label}' takes a number in {pdef.unit}.")
            problem = pdef.range_problem(float(value))
            if problem:
                raise LightSimError(f"'{pdef.label}' {problem}; got {value:g}.")
            return value
        if pdef.type == "boolean":
            if not isinstance(value, bool):
                raise LightSimError(f"'{pdef.label}' takes True or False.")
        elif pdef.type == "enum":
            if value not in (pdef.options or []):
                raise LightSimError(f"'{pdef.label}' takes one of: {', '.join(pdef.options or [])}.")
        elif pdef.type in ("string", "code"):
            if not isinstance(value, str):
                raise LightSimError(f"'{pdef.label}' takes text.")
        elif pdef.type in ("table1d", "table2d"):
            if not isinstance(value, dict):
                raise LightSimError(f"'{pdef.label}' takes a table: a dict keyed by the axis "
                                    f"value, as in the project file.")
        return value

    def set(self, ref: str, value: Any, case: Optional[str] = None) -> "Project":
        """Set a parameter (``"E-Motor.max_power_kw"``). A number is in the
        parameter's unit; text with a unit (``"150 kW"``, ``"0.15 MW"``) is
        converted and refused if the unit measures something else. Limits
        from the library are checked. With ``case``, the value is that
        case's own (the part keeps its value for other cases)."""
        el, pdef = self._param(ref)
        value = self._value(pdef, value)
        if case is None:
            el.parameterOverrides[pdef.key] = value
        else:
            self.case(case).parameterOverrides.setdefault(el.id, {})[pdef.key] = value
        return self

    # -- editing the diagram ------------------------------------------------------
    def _system(self, ref: Optional[str]):
        systems = self.model.systems
        if ref is None:
            return next((s for s in systems if s.parentId is None), systems[0])
        for s in systems:
            if ref in (s.id, s.name):
                return s
        raise LightSimError(f"No system '{ref}'.")

    def add(self, component: str, label: Optional[str] = None, system: Optional[str] = None,
            position: tuple[float, float] = (0.0, 0.0), **values: Any) -> str:
        """Add a part of type ``component`` (``"motor.emotor"``; see
        :func:`lightsim.library`) and return its id. ``values`` set its
        parameters as :meth:`set` does. The label defaults to the type's
        name, numbered if taken."""
        schemas = engine("schemas")
        cdef = library().get(component)
        if cdef is None:
            raise LightSimError(f"No part type '{component}'. Types: {', '.join(sorted(library()))}.")
        taken = {e.label for e in self.elements}
        if label is None:
            label, k = cdef.name, 2
            while label in taken:
                label, k = f"{cdef.name} {k}", k + 1
        el = schemas.ElementInstance(id=_uid("el"), componentDefId=component, label=label,
                                     position={"x": float(position[0]), "y": float(position[1])})
        self._system(system).elements.append(el)
        for key, value in values.items():
            self.set(f"{el.id}.{key}", value)
        return el.id

    def remove(self, ref: str) -> "Project":
        """Remove a part with its wires, signal links and case values (and a
        sub-system it opens, with everything in it)."""
        el = self.element(ref)
        doomed_systems: set[str] = set()

        def collect(sys_id: str) -> None:
            doomed_systems.add(sys_id)
            for s in self.model.systems:
                if s.parentId == sys_id:
                    collect(s.id)

        if el.subSystemId:
            collect(el.subSystemId)
        doomed = {el.id} | {e.id for s in self.model.systems if s.id in doomed_systems
                            for e in s.elements}
        self.model.systems = [s for s in self.model.systems if s.id not in doomed_systems]
        for s in self.model.systems:
            s.elements = [e for e in s.elements if e.id not in doomed]
            s.connections = [c for c in s.connections
                             if c.sourceElementId not in doomed and c.targetElementId not in doomed]
        self.model.dataBusConnections = [d for d in self.model.dataBusConnections
                                         if d.element1Id not in doomed and d.element2Id not in doomed]
        for c in self.model.cases:
            for el_id in doomed:
                c.parameterOverrides.pop(el_id, None)
        return self

    def _port(self, ref: str):
        el_ref, _, port_id = ref.rpartition(".")
        if not el_ref:
            raise LightSimError(f"'{ref}' needs a part and a port: 'Part.port_id'.")
        el = self.element(el_ref)
        ports = list(self._definition(el).ports) + list(el.dynamicPorts or [])
        port = next((p for p in ports if port_id in (p.id, p.name)), None)
        if port is None:
            raise LightSimError(f"Part '{el.label}' has no port '{port_id}'. Its ports: "
                                f"{', '.join(p.id for p in ports)}.")
        return el, port

    def _owner(self, el_id: str):
        return next(s for s in self.model.systems if any(e.id == el_id for e in s.elements))

    def connect(self, a: str, b: str) -> str:
        """Wire two physical ports (``"HV Battery Pack.pos"``,
        ``"HV Bus.t1"``) and return the wire's id. Both must be of one kind
        (electrical, mechanical …) and in the same system. Signals are
        linked with :meth:`route`."""
        schemas = engine("schemas")
        el_a, pa = self._port(a)
        el_b, pb = self._port(b)
        if pa.kind == "signal" or pb.kind == "signal":
            raise LightSimError("Signal ports are linked with route(output, input), not connect().")
        if pa.kind != pb.kind:
            raise LightSimError(f"'{a}' is {pa.kind} and '{b}' is {pb.kind}: only ports of one "
                                f"kind connect.")
        system = self._owner(el_a.id)
        if self._owner(el_b.id) is not system:
            raise LightSimError(f"'{el_a.label}' and '{el_b.label}' are in different systems.")
        wire = schemas.Connection(id=_uid("c"), sourceElementId=el_a.id, sourcePortId=pa.id,
                                  targetElementId=el_b.id, targetPortId=pb.id)
        system.connections.append(wire)
        return wire.id

    def route(self, output: str, input: str) -> str:
        """Link a signal output to a signal input on the data bus
        (``route("Driver.sig_traction_cmd", "E-Motor.sig_demand_in")``) and
        return the link's id. An input takes one signal: an existing link
        into it is replaced."""
        schemas = engine("schemas")
        el_o, po = self._port(output)
        el_i, pi = self._port(input)
        if po.kind != "signal" or pi.kind != "signal":
            raise LightSimError("route() links signal ports; wire physical ports with connect().")
        if po.direction != "output" or pi.direction != "input":
            raise LightSimError(f"route() takes an output then an input: '{output}' is an "
                                f"{po.direction} and '{input}' an {pi.direction}.")
        self.model.dataBusConnections = [
            d for d in self.model.dataBusConnections
            if not ((d.element1Id, d.port1Id) == (el_i.id, pi.id)
                    or (d.element2Id, d.port2Id) == (el_i.id, pi.id))]
        link = schemas.DataBusConnection(id=_uid("dbc"), element1Id=el_o.id, port1Id=po.id,
                                         element2Id=el_i.id, port2Id=pi.id)
        self.model.dataBusConnections.append(link)
        return link.id

    def disconnect(self, ref: str) -> "Project":
        """Remove a wire or signal link by its id."""
        before = sum(len(s.connections) for s in self.model.systems) + len(
            self.model.dataBusConnections)
        for s in self.model.systems:
            s.connections = [c for c in s.connections if c.id != ref]
        self.model.dataBusConnections = [d for d in self.model.dataBusConnections if d.id != ref]
        after = sum(len(s.connections) for s in self.model.systems) + len(
            self.model.dataBusConnections)
        if before == after:
            raise LightSimError(f"No wire or signal link '{ref}'.")
        return self

    def add_case(self, name: str, duration: float = 600.0, time_step: float = 1.0,
                 kind: str = "cycle", values: Optional[dict[str, Any]] = None,
                 **fields: Any) -> str:
        """Add a case and return its id. ``values`` maps ``"Part.key"`` to the
        case's own values (checked as :meth:`set` checks them); ``fields`` are
        other case settings by their file name (``endDistance=75``)."""
        schemas = engine("schemas")
        try:
            case = schemas.SimCase(id=_uid("case"), name=name, duration=duration,
                                   timeStep=time_step, kind=kind, **fields)
        except ValueError as e:
            raise LightSimError(f"Case '{name}': {e}") from None
        self.model.cases.append(case)
        for ref, value in (values or {}).items():
            self.set(ref, value, case=case.id)
        return case.id

    # -- checking and running -------------------------------------------------------
    def check(self) -> list[Check]:
        """The Data Checks (what the app's *Problems* list shows before a run).
        They never run Script code: scripts are only compiled."""
        found = engine("validation").validate_project(self.model)
        return [Check(c.level, c.text, c.fix, tuple(c.elementIds or ([c.elementId] if c.elementId
                                                                     else [])))
                for c in found]

    def run(self, case: Optional[str] = None, *, check: bool = True,
            on_step: Optional[Callable[[dict], None]] = None,
            time_limit_s: Optional[float] = None) -> Result:
        """Run a case (by id or name; the first one if None) in this process
        and return its Result. No server, window or network is involved.

        With ``check`` (the default) the Data Checks run first, as in the app:
        any error stops the run and the Result is ``failed`` with the checks
        in ``Result.checks``. ``on_step`` gets each recorded step
        (``{"t", "pct", "values"}``). ``time_limit_s`` stops the run after
        that many seconds of wall-clock time (it ends ``cancelled``)."""
        import time as _time

        c = self.case(case)
        schemas = engine("schemas")
        if check:
            checks = self.check()
            errors = [k for k in checks if k.level == "error"]
            if errors:
                sim = schemas.SimResult(caseId=c.id, status="failed", channels=[], messages=[
                    {"level": "error", "text": f"Data check failed: {k.text}"} for k in errors])
                return from_sim_result(sim, c.name, self.name, checks)
        control = None
        if time_limit_s is not None:
            deadline = _time.monotonic() + max(0.0, float(time_limit_s))
            sent = False

            def control() -> list[dict]:
                nonlocal sent
                if not sent and _time.monotonic() >= deadline:
                    sent = True
                    return [{"type": "cancel"}]
                return []

        emit = (lambda event: on_step(event) if event.get("type") == "step" else None) \
            if on_step else None
        sim = engine("solver").simulate(self.model, c.id, emit, control)
        return from_sim_result(sim, c.name, self.name)

    def has_scripts(self) -> bool:
        """True when a part runs user Python code (a Script block)."""
        return any(e.componentDefId == "signal.script" for e in self.elements)

    def __repr__(self) -> str:
        return (f"<lightsim.Project {self.name!r} parts={len(self.elements)} "
                f"cases={[c.name for c in self.model.cases]}>")


def iter_params(project: Project) -> Iterable[tuple[str, str, Any, str]]:
    """(part label, key, value, unit) for every parameter of every part."""
    for el in project.elements:
        cdef = library().get(el.componentDefId)
        if cdef is None:
            continue
        for p in cdef.parameters:
            yield el.label, p.key, el.parameterOverrides.get(p.key, p.default), p.unit
