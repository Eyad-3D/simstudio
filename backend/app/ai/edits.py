"""Model edits an assistant proposes (the MCP tool ``model_edit``, AI-03).

A batch of operations is applied to a copy of the project, all or nothing:
one bad operation and nothing changes. The caller gets back what changed
in words and the Data Checks of the edited model, so the assistant (and the
user approving it) can see the effect before anything is saved.

Operations (``op``):

- ``set``: ``element``, ``param``, ``value`` (in the parameter's own unit;
  a text such as "150 kW" must name that unit), optional ``case`` to change
  it for one case only.
- ``add``: ``type`` (a library part id such as "motor.emotor"), ``label``,
  optional ``id`` and ``system``.
- ``remove``: ``element`` (its wires and signals go with it).
- ``connect`` / ``disconnect``: ``from`` and ``to`` as "element:port". Two
  signal ports make a data-bus signal; anything else a wire.
- ``add_port``: ``element`` (a Script or Monitor), ``name``, ``direction``
  ("input" or "output"): a signal port of that part.
- ``add_case``: ``name``, optional ``id``, ``copy_of``, ``kind``,
  ``duration``, ``timeStep``.
- ``set_case``: ``case`` and any of ``name``, ``kind``, ``duration``,
  ``timeStep``, ``endDistance``.

Elements are named by id or by their exact label.
"""
from __future__ import annotations

import re
from typing import Any, Optional

from ..library import library_by_id
from ..schemas import (
    ComponentDef,
    Connection,
    DataBusConnection,
    ElementInstance,
    PortDef,
    Project,
    SimCase,
    SystemNode,
)
from .overview import fmt_value


class EditError(ValueError):
    """An operation that cannot be applied; nothing was changed."""


_CASE_FIELDS = {"name": str, "kind": str, "duration": float, "timeStep": float, "endDistance": float}
_UNIT_VALUE = re.compile(r"^\s*([-+]?\d[\d,]*(?:\.\d+)?(?:[eE][-+]?\d+)?)\s*(.*?)\s*$")


def _norm_unit(u: str) -> str:
    return u.replace("·", "").replace("*", "").replace(" ", "").replace("²", "^2").lower()


class _Model:
    def __init__(self, project: Project) -> None:
        self.p = project
        self.lib = library_by_id()

    def elements(self) -> list[tuple[SystemNode, ElementInstance]]:
        return [(s, e) for s in self.p.systems for e in s.elements]

    def element(self, name: Any) -> tuple[SystemNode, ElementInstance]:
        name = str(name or "")
        hits = [(s, e) for s, e in self.elements() if e.id == name]
        hits = hits or [(s, e) for s, e in self.elements() if e.label == name]
        if not hits:
            raise EditError(f"No part '{name}' (name a part by its id or exact label).")
        if len(hits) > 1:
            raise EditError(f"Several parts are labelled '{name}': name it by id "
                            f"({', '.join(e.id for _, e in hits)}).")
        return hits[0]

    def cdef(self, el: ElementInstance) -> ComponentDef:
        cdef = self.lib.get(el.componentDefId)
        if cdef is None:
            raise EditError(f"Part '{el.label}' has an unknown type {el.componentDefId}.")
        return cdef

    def port(self, ref: Any) -> tuple[SystemNode, ElementInstance, str, str]:
        """(system, element, port id, port kind) of "element:port"."""
        text = str(ref or "")
        if ":" not in text:
            raise EditError(f"Name a port as 'element:port', not '{text}'.")
        el_name, port_name = text.rsplit(":", 1)
        system, el = self.element(el_name)
        ports = list(self.cdef(el).ports) + list(el.dynamicPorts)
        port = next((p for p in ports if p.id == port_name), None) or \
            next((p for p in ports if p.name.lower() == port_name.lower()), None)
        if port is None:
            names = ", ".join(p.id for p in ports) or "none"
            raise EditError(f"'{el.label}' has no port '{port_name}' (its ports: {names}).")
        return system, el, port.id, port.kind

    def case(self, case_id: Any) -> SimCase:
        case = next((c for c in self.p.cases if c.id == case_id or c.name == case_id), None)
        if case is None:
            raise EditError(f"No case '{case_id}'.")
        return case

    def free_id(self, base: str) -> str:
        taken = {e.id for _, e in self.elements()} | {c.id for c in self.p.cases} | \
            {c.id for s in self.p.systems for c in s.connections} | {d.id for d in self.p.dataBusConnections}
        base = re.sub(r"[^A-Za-z0-9_-]+", "-", base).strip("-") or "item"
        candidate, n = base, 2
        while candidate in taken:
            candidate, n = f"{base}-{n}", n + 1
        return candidate


def _parse_value(cdef: ComponentDef, key: str, value: Any) -> tuple[Any, str]:
    """The value checked against the parameter: its type, its options, its
    unit (a text "150 kW" must name the parameter's unit) and its limits."""
    pdef = next((p for p in cdef.parameters if p.key == key), None)
    if pdef is None:
        keys = ", ".join(p.key for p in cdef.parameters)
        raise EditError(f"A {cdef.name} has no parameter '{key}' (its parameters: {keys}).")
    if pdef.type == "number":
        if isinstance(value, str):
            m = _UNIT_VALUE.match(value)
            if not m:
                raise EditError(f"{pdef.label} needs a number in {pdef.unit}, not '{value}'.")
            unit = m.group(2)
            if unit and _norm_unit(unit) != _norm_unit(pdef.unit):
                raise EditError(f"{pdef.label} is in {pdef.unit}, not {unit}: convert the value first.")
            value = float(m.group(1).replace(",", ""))
        if isinstance(value, bool) or not isinstance(value, (int, float)):
            raise EditError(f"{pdef.label} needs a number in {pdef.unit}.")
        problem = pdef.range_problem(float(value))
        if problem:
            raise EditError(f"{pdef.label} {problem}.")
    elif pdef.type == "boolean":
        if not isinstance(value, bool):
            raise EditError(f"{pdef.label} is on or off (true or false).")
    elif pdef.type == "enum":
        if value not in (pdef.options or []):
            raise EditError(f"{pdef.label} must be one of: {', '.join(pdef.options or [])}.")
    elif pdef.type in ("table1d", "table2d"):
        if not isinstance(value, dict) or not value:
            raise EditError(f"{pdef.label} is a table: give it as {{x: value}}"
                            + (" nested one level" if pdef.type == "table2d" else "") + ".")
    elif not isinstance(value, str):
        raise EditError(f"{pdef.label} is text.")
    return value, f"{pdef.label} {fmt_value(value, pdef.unit)}"


def apply_operations(project: Project, operations: list[dict]) -> tuple[Project, list[str]]:
    """The edited copy of the project and a line per change; raises
    :class:`EditError` (naming the operation) if any operation fails."""
    if not isinstance(operations, list) or not operations:
        raise EditError("Give a list of one or more operations.")
    if len(operations) > 200:
        raise EditError("At most 200 operations at a time.")
    m = _Model(project.model_copy(deep=True))
    changes: list[str] = []
    for i, op in enumerate(operations, 1):
        try:
            if not isinstance(op, dict):
                raise EditError("An operation is an object with an 'op'.")
            changes.append(_apply(m, op))
        except EditError as e:
            raise EditError(f"Operation {i} ({op.get('op') if isinstance(op, dict) else op}): {e}") from None
    return m.p, changes


def _apply(m: _Model, op: dict) -> str:
    kind = op.get("op")
    if kind == "set":
        _, el = m.element(op.get("element"))
        key = str(op.get("param") or "")
        value, text = _parse_value(m.cdef(el), key, op.get("value"))
        if op.get("case"):
            case = m.case(op["case"])
            case.parameterOverrides.setdefault(el.id, {})[key] = value
            return f'Case "{case.name}": "{el.label}" {text}'
        el.parameterOverrides[key] = value
        return f'"{el.label}": {text}'
    if kind == "add":
        type_id = str(op.get("type") or "")
        cdef = m.lib.get(type_id)
        if cdef is None or type_id == "container.system":
            raise EditError(f"No library part '{type_id}' (lightsim://components lists them).")
        system_id = op.get("system")
        system = next((s for s in m.p.systems if s.id == system_id), None) if system_id else \
            next((s for s in m.p.systems if s.parentId is None), m.p.systems[0])
        if system is None:
            raise EditError(f"No system '{system_id}'.")
        label = str(op.get("label") or cdef.name)
        el_id = m.free_id(str(op.get("id") or "el-" + label.lower()))
        x = max((e.position.get("x", 0) for e in system.elements), default=0) + 160
        system.elements.append(ElementInstance(id=el_id, componentDefId=type_id, label=label,
                                               position={"x": x, "y": 40}))
        return f'Added "{label}" ({cdef.name}, id {el_id})'
    if kind == "remove":
        system, el = m.element(op.get("element"))
        system.elements = [e for e in system.elements if e.id != el.id]
        for s in m.p.systems:
            s.connections = [c for c in s.connections
                             if el.id not in (c.sourceElementId, c.targetElementId)]
        m.p.dataBusConnections = [d for d in m.p.dataBusConnections
                                  if el.id not in (d.element1Id, d.element2Id)]
        for case in m.p.cases:
            case.parameterOverrides.pop(el.id, None)
        return f'Removed "{el.label}" and its wires and signals'
    if kind in ("connect", "disconnect"):
        sa, ea, pa, ka = m.port(op.get("from"))
        sb, eb, pb, kb = m.port(op.get("to"))
        signal = ka == "signal" and kb == "signal"
        if (ka == "signal") != (kb == "signal"):
            raise EditError("A signal port connects only to another signal port.")
        what = f'"{ea.label}".{pa} → "{eb.label}".{pb}'
        if kind == "disconnect":
            before = len(m.p.dataBusConnections) + sum(len(s.connections) for s in m.p.systems)
            m.p.dataBusConnections = [d for d in m.p.dataBusConnections if {
                (d.element1Id, d.port1Id), (d.element2Id, d.port2Id)} != {(ea.id, pa), (eb.id, pb)}]
            for s in m.p.systems:
                s.connections = [c for c in s.connections if {
                    (c.sourceElementId, c.sourcePortId), (c.targetElementId, c.targetPortId)}
                    != {(ea.id, pa), (eb.id, pb)}]
            if before == len(m.p.dataBusConnections) + sum(len(s.connections) for s in m.p.systems):
                raise EditError(f"{what} is not connected.")
            return f"Disconnected {what}"
        if signal:
            m.p.dataBusConnections.append(DataBusConnection(
                id=m.free_id("db-ai"), element1Id=ea.id, port1Id=pa, element2Id=eb.id, port2Id=pb))
            return f"Signal {what}"
        if sa.id != sb.id:
            raise EditError("A wire joins two parts of the same system.")
        sa.connections.append(Connection(id=m.free_id("c-ai"), sourceElementId=ea.id, sourcePortId=pa,
                                         targetElementId=eb.id, targetPortId=pb))
        return f"Wire {what}"
    if kind == "add_port":
        _, el = m.element(op.get("element"))
        if not m.cdef(el).allowDynamicPorts:
            raise EditError(f"'{el.label}' has fixed ports; only Script and Monitor parts take new ones.")
        direction = op.get("direction")
        if direction not in ("input", "output"):
            raise EditError("A port's direction is input or output.")
        port_id = re.sub(r"[^a-z0-9_]+", "_", str(op.get("name") or "").strip().lower()).strip("_")
        if not port_id:
            raise EditError("Give the port a name.")
        if any(p.id == port_id for p in el.dynamicPorts):
            raise EditError(f"'{el.label}' already has a port '{port_id}'.")
        el.dynamicPorts.append(PortDef(id=port_id, name=port_id, direction=direction,
                                       kind="signal", unitGroup="No Unit"))
        return f'"{el.label}": {direction} port {port_id}'
    if kind == "add_case":
        source = m.case(op["copy_of"]) if op.get("copy_of") else None
        name = str(op.get("name") or "New case")
        case = source.model_copy(deep=True) if source else SimCase(id="x", name=name)
        case.id = m.free_id(str(op.get("id") or "case-" + name.lower()))
        case.name = name
        _set_case_fields(case, op)
        m.p.cases.append(case)
        return f'Added case "{name}" (id {case.id})'
    if kind == "set_case":
        case = m.case(op.get("case"))
        changed = _set_case_fields(case, op)
        if not changed:
            raise EditError(f"Nothing to change: give any of {', '.join(_CASE_FIELDS)}.")
        return f'Case "{case.name}": ' + ", ".join(changed)
    raise EditError("Unknown 'op': use set, add, remove, connect, disconnect, add_port, add_case or set_case.")


def _set_case_fields(case: SimCase, op: dict) -> list[str]:
    changed = []
    for field, kind in _CASE_FIELDS.items():
        if field not in op or field == "name" and op.get("op") == "add_case":
            continue
        value = op[field]
        try:
            value = kind(value)
        except (TypeError, ValueError):
            raise EditError(f"Case {field} must be a {kind.__name__}.") from None
        if field == "kind" and value not in ("cycle", "performance", "acceleration", "lap"):
            raise EditError("Case kind is cycle, performance, acceleration or lap.")
        if field in ("duration", "timeStep") and not value > 0:
            raise EditError(f"Case {field} must be above 0 s.")
        setattr(case, field, value)
        changed.append(f"{field} {value}")
    return changed


def check_counts(checks: list) -> dict[str, int]:
    out = {"error": 0, "warning": 0, "info": 0}
    for c in checks:
        out[c.level] = out.get(c.level, 0) + 1
    return out


def describe_check(c: Any) -> dict:
    row: dict[str, Optional[str]] = {"level": c.level, "text": c.text}
    if c.elementLabel:
        row["part"] = c.elementLabel
    if c.fix:
        row["fix"] = c.fix
    return row
