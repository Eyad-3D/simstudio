"""Model extraction: normalize a project topology into solvable structure.

The causal solver does not traverse the raw graph at simulation time.
This module reduces it once, at build time, to:

- *Drivelines*: mechanical connected subgraphs, split into rigid segments
  at joint elements (differential, transfer case, clutch). Within a rigid
  segment every element's speed is a fixed multiple (gear ratio product)
  of the segment reference axis. The solver treats each driveline as a
  small Lagrangian system: independent coordinates live on the leaf
  segments, and every segment's speed is a linear function of them
  (g-vectors), assembled per sub-step into an n×n mass matrix (n ≤ ~5).
- *Buses*: electrical connected groups; sources are batteries, voltage
  sources, fuel cells, or DC-DC outputs. Ground rails carry no power.
- *Signal routes* and the evaluation order of signal blocks (Script,
  PID, Lookup, Road Profile).

New component types plug in here: a gearbox is a (signal-selected) ratio
element inside a segment, an engine is a torque source like the motor,
clutch/transfer case are joints.
"""
from __future__ import annotations

from collections import defaultdict
from dataclasses import dataclass, field
from typing import Callable, Optional

from ..library import library_by_id
from ..schemas import ComponentDef, ElementInstance, PortDef, Project

JOINT_TYPES = {"mech.differential", "mech.transfer_case", "mech.clutch"}
SOURCE_TYPES = {"motor.emotor": "motor", "engine.combustion": "engine"}
SIGNAL_BLOCK_TYPES = ("signal.script", "control.pid", "signal.lookup", "signal.road_profile")

# Model advisories that Data Checks treat as errors, because the vehicle
# cannot move (the run itself only reports them as warnings).
NO_VEHICLE = ("Wheels present but no Vehicle element — wheels carry no load "
              "and produce no traction.")
NO_WHEELS = "Vehicle present but no connected wheels — it will not move."
NO_DRIVER = "No Driver element — nothing commands the powertrain unless you wire demands yourself."


class ModelError(Exception):
    """Topology cannot be reduced to a solvable model."""

    def __init__(self, errors: list[str]):
        super().__init__("; ".join(errors))
        self.errors = errors


@dataclass
class WheelRef:
    el_id: str
    m: float  # ω_wheel = m · ω_ref
    radius: float
    load_share: float
    mu: float
    c_slip: float
    c_rr: float


@dataclass
class BrakeRef:
    el_id: str
    m: float
    max_torque: float


@dataclass
class PropRef:
    el_id: str
    m: float
    t_ref: float
    n_ref: float


@dataclass
class SourceRef:
    el_id: str
    kind: str  # "motor" | "engine"
    m: float  # ω_source = m · ω_ref
    region: int  # the segment region it sits in (Segment.stages)
    eff: float = 1.0  # gear-chain efficiency source → segment output


@dataclass
class GearStage:
    """A lossy element (gear, final drive, shaft with an efficiency) seen
    from the segment region behind it: power crossing it towards the
    segment's output passes ``eff`` of itself on to region ``parent``;
    power crossing it back needs 1/``eff`` of what reaches the region."""
    el_id: str
    eff: float
    parent: int


@dataclass
class GearboxRef:
    el_id: str
    # walk-time ratio (from default/current gear); core rebuilds reflections
    # when the selected gear changes
    ratio: float


@dataclass
class Segment:
    inertia: float = 0.0  # ΣJ·m² at the reference axis
    wheels: list[WheelRef] = field(default_factory=list)
    brakes: list[BrakeRef] = field(default_factory=list)
    props: list[PropRef] = field(default_factory=list)
    sources: list[SourceRef] = field(default_factory=list)
    gearboxes: list[GearboxRef] = field(default_factory=list)
    element_ms: dict[str, float] = field(default_factory=dict)  # el_id → m
    port_ms: dict[tuple[str, str], float] = field(default_factory=dict)
    # Its lossy elements cut a segment into regions (0 = the walk entry's).
    # Power leaves it at its output (a split's input, a wheel, a clutch …),
    # so each other region reaches the output through its stage and its
    # parents' stages: stages[r] is region r's (None at the output), and
    # stage_order lists the regions outside in.
    port_region: dict[tuple[str, str], int] = field(default_factory=dict)
    links: list[tuple[str, float, int, int]] = field(default_factory=list)  # (el, eff, r, r2)
    out_region: int = 0
    stages: list[Optional[GearStage]] = field(default_factory=lambda: [None])
    stage_order: list[int] = field(default_factory=list)

    def path_eff(self, region: int) -> float:
        """Efficiency of the way from ``region`` to the segment's output."""
        eff = 1.0
        stage = self.stages[region]
        while stage is not None:
            eff *= stage.eff
            stage = self.stages[stage.parent]
        return eff

    def orient(self, out_port: tuple[str, str] | None) -> None:
        """Take ``out_port``'s region as the output (the entry's region
        without one) and point every stage towards it."""
        out = self.port_region.get(out_port, 0) if out_port else 0
        adj: dict[int, list[tuple[int, str, float]]] = defaultdict(list)
        for el_id, eff, r, r2 in self.links:
            adj[r].append((r2, el_id, eff))
            adj[r2].append((r, el_id, eff))
        self.out_region = out
        self.stages = [None] * (len(self.links) + 1)
        self.stage_order = []
        seen, frontier = {out}, [out]
        for r in frontier:  # breadth-first from the output; grows while iterating
            for r2, el_id, eff in adj[r]:
                if r2 not in seen:
                    seen.add(r2)
                    frontier.append(r2)
                    self.stages[r2] = GearStage(el_id=el_id, eff=eff, parent=r)
                    self.stage_order.append(r2)
        for src in self.sources:
            src.eff = self.path_eff(src.region)


@dataclass
class Joint:
    el_id: str
    kind: str  # "split" | "clutch"
    # split (differential / transfer case): ω_in = ratio·((1−f)ω_a + f·ω_b)
    ratio: float = 1.0
    eff: float = 1.0
    locked: bool = False
    f_b: float = 0.5  # torque/speed weight of output B
    parent_seg: int = -1
    parent_m: float = 1.0  # m of the segment port attached to flange_in
    child_a: int = -1
    child_a_m: float = 1.0
    child_b: int = -1
    child_b_m: float = 1.0
    # region of the child segments' ports attached to it (Segment.stages)
    child_a_region: int = 0
    child_b_region: int = 0
    # clutch: sides a/b reuse child_a/child_b (+ their m's); capacity from params


@dataclass
class Driveline:
    segments: list[Segment] = field(default_factory=list)
    joints: list[Joint] = field(default_factory=list)
    # per-segment ratio elements are baked into the walk; the core rebuilds
    # this driveline's numbers when a gearbox changes gear
    element_group: list[str] = field(default_factory=list)  # all element ids


@dataclass
class Bus:
    id: int
    grounded: bool = False
    battery: str | None = None
    vsource: str | None = None
    fuelcell: str | None = None
    consumers: list[str] = field(default_factory=list)
    motors: list[str] = field(default_factory=list)
    dcdc_in: list[str] = field(default_factory=list)
    dcdc_out: list[str] = field(default_factory=list)
    nodes: list[str] = field(default_factory=list)

    def __hash__(self) -> int:  # identity semantics — kept in sets/dicts
        return id(self)

    def __eq__(self, other: object) -> bool:
        return self is other


@dataclass
class Model:
    elements: dict[str, ElementInstance]
    cdef_of: dict[str, ComponentDef]
    params_of: dict[str, dict]
    signal_route: dict[tuple[str, str], tuple[str, str]]
    drivelines: list[Driveline]
    buses: list[Bus]  # in solve order (supplied buses before their suppliers)
    vehicle: str | None
    driver: str | None
    fuel_tank: str | None
    h2_tank: str | None
    signal_blocks: list[str]  # Script/PID/Lookup/RoadProfile ids in eval order
    floating_returns: list[str] = field(default_factory=list)  # unwired − terminals
    warnings: list[str] = field(default_factory=list)
    # re-extracts a driveline for new gears from the current (live) params_of
    rewalk: Optional[Callable[[Driveline, dict[str, float]], Optional[Driveline]]] = None


def resolve_params(el: ElementInstance, cdef: ComponentDef) -> dict:
    params = {p.key: p.default for p in cdef.parameters}
    params.update(el.parameterOverrides)
    return params


def ports_of(el: ElementInstance, cdef: ComponentDef) -> list[PortDef]:
    if cdef.allowDynamicPorts and el.dynamicPorts:
        return [*cdef.ports, *el.dynamicPorts]
    return list(cdef.ports)


def gearbox_ratio(params: dict, gear: float | None = None) -> float:
    """Selected gear ratio; nearest defined gear wins."""
    raw = params.get("ratios") or {}
    try:
        entries = sorted((float(k), float(v)) for k, v in raw.items())
    except (TypeError, ValueError):
        entries = []
    if not entries:
        return 1.0
    g = gear if gear is not None else float(params.get("default_gear", 1) or 1)
    best = min(entries, key=lambda kv: abs(kv[0] - g))
    return best[1] or 1.0


def build_model(
    project: Project,
    gear_of: dict[str, float] | None = None,
    case_overrides: dict[str, dict] | None = None,
) -> Model:
    """Reduce the project. `gear_of` optionally overrides gearbox gears;
    after a shift the solver re-extracts a driveline with `Model.rewalk`,
    which reads the live parameter set. `case_overrides`
    ({elementId: {paramKey: value}}) layers per-case parameter values on top
    of each element's own overrides (used by parameter sweeps / per-case tweaks)."""
    defs = library_by_id()
    errors: list[str] = []
    warnings: list[str] = []
    gear_of = gear_of or {}
    case_overrides = case_overrides or {}

    elements: dict[str, ElementInstance] = {}
    for system in project.systems:
        for el in system.elements:
            elements[el.id] = el

    cdef_of: dict[str, ComponentDef] = {}
    for el_id, el in elements.items():
        cdef = defs.get(el.componentDefId)
        if cdef is None:
            errors.append(f"'{el.label}' references unknown component type '{el.componentDefId}'.")
            continue
        cdef_of[el_id] = cdef
        if el.dynamicPorts and not cdef.allowDynamicPorts:
            errors.append(f"'{el.label}' has custom ports but '{cdef.name}' does not allow them.")

    params_of = {el_id: resolve_params(elements[el_id], cdef) for el_id, cdef in cdef_of.items()}
    for el_id, ov in case_overrides.items():
        if el_id in params_of and isinstance(ov, dict):
            params_of[el_id].update(ov)

    port_def: dict[tuple[str, str], PortDef] = {}
    for el_id, cdef in cdef_of.items():
        for p in ports_of(elements[el_id], cdef):
            port_def[(el_id, p.id)] = p

    # ---- connectivity ------------------------------------------------------
    mech_adj: dict[tuple[str, str], list[tuple[str, str]]] = defaultdict(list)
    elec_links: list[tuple[tuple[str, str], tuple[str, str]]] = []
    signal_route: dict[tuple[str, str], tuple[str, str]] = {}

    def add_signal(a: tuple[str, str], b: tuple[str, str]) -> None:
        pa, pb = port_def.get(a), port_def.get(b)
        if pa is None or pb is None:
            return
        if pa.direction == "output" and pb.direction != "output":
            signal_route[b] = a
        elif pb.direction == "output" and pa.direction != "output":
            signal_route[a] = b

    for system in project.systems:
        for conn in system.connections:
            a = (conn.sourceElementId, conn.sourcePortId)
            b = (conn.targetElementId, conn.targetPortId)
            pa, pb = port_def.get(a), port_def.get(b)
            if pa is None or pb is None:
                continue
            if pa.kind == "signal" or pb.kind == "signal":
                add_signal(a, b)
            elif pa.kind == "mechanical" and pb.kind == "mechanical":
                mech_adj[a].append(b)
                mech_adj[b].append(a)
            elif pa.kind == "electrical" and pb.kind == "electrical":
                elec_links.append((a, b))
    for dbc in project.dataBusConnections:
        add_signal((dbc.element1Id, dbc.port1Id), (dbc.element2Id, dbc.port2Id))

    # ---- drivelines --------------------------------------------------------
    mech_el_adj: dict[str, set[str]] = defaultdict(set)
    for (el_a, _), peers in mech_adj.items():
        for el_b, _ in peers:
            if el_a != el_b:
                mech_el_adj[el_a].add(el_b)
                mech_el_adj[el_b].add(el_a)
    mech_elements = {el_id for el_id, cdef in cdef_of.items()
                     if any(p.kind == "mechanical" for p in cdef.ports)}
    wired_mech = {el for el in mech_elements if mech_el_adj.get(el)}
    joint_ids = {el for el in wired_mech
                 if cdef_of.get(el) and cdef_of[el].id in JOINT_TYPES}

    def mech_component(start: str) -> set[str]:
        seen = {start}
        stack = [start]
        while stack:
            cur = stack.pop()
            for nxt in mech_el_adj.get(cur, ()):
                if nxt not in seen:
                    seen.add(nxt)
                    stack.append(nxt)
        return seen

    def walk_segment(entries: list[tuple[str, str]], gears: dict[str, float]) -> Segment:
        """Collapse a rigid region into a Segment. `entries` are (el, port)
        vertices on the reference axis (m = 1); joint elements are never
        crossed. Each lossy element crossed starts a new region
        (``Segment.links``); ``Segment.orient`` later points them towards
        the segment's output."""
        seg = Segment()
        seen_ports = seg.port_region
        # (element, port, m, region it is reached from, efficiency of the
        # element crossed to reach it — None when none was crossed)
        queue: list[tuple[str, str, float, int, float | None]] = [
            (e, p, 1.0, 0, None) for e, p in entries]
        visited_el: set[str] = set()

        def enqueue_peers(el_id: str, pid: str, m: float, region: int) -> None:
            for peer in mech_adj.get((el_id, pid), ()):  # rigid joint: same axis
                if peer[0] not in joint_ids and peer not in seen_ports:
                    queue.append((peer[0], peer[1], m, region, None))

        while queue:
            el_id, pid, m, region, crossed = queue.pop()
            key = (el_id, pid)
            if key in seen_ports or el_id in joint_ids:
                continue
            if crossed is not None and crossed < 1.0:  # a lossy element: new region
                seg.links.append((el_id, crossed, region, len(seg.links) + 1))
                region = len(seg.links)
            seen_ports[key] = region
            seg.port_ms[key] = m
            cdef = cdef_of.get(el_id)
            if cdef is None:
                continue
            t = cdef.id
            p = params_of[el_id]
            first_visit = el_id not in visited_el
            visited_el.add(el_id)
            seg.element_ms.setdefault(el_id, m)

            if t == "mech.shaft":
                if first_visit:
                    seg.inertia += float(p.get("inertia_kgm2", 0)) * m * m
                other = "flange_b" if pid == "flange_a" else "flange_a"
                eta = max(1e-3, float(p.get("efficiency_pct", 100)) / 100.0)
                if (el_id, other) not in seen_ports:
                    queue.append((el_id, other, m, region, eta))
                enqueue_peers(el_id, pid, m, region)
            elif t in ("mech.final_drive", "mech.gearbox"):
                if t == "mech.gearbox":
                    ratio = gearbox_ratio(p, gears.get(el_id))
                    if first_visit:
                        seg.gearboxes.append(GearboxRef(el_id=el_id, ratio=ratio))
                else:
                    ratio = float(p.get("ratio", 1.0)) or 1.0
                eta = max(1e-3, float(p.get("efficiency_pct", 100)) / 100.0)
                if pid == "flange_out":
                    m_in, m_out = m * ratio, m
                    other, m_other = "flange_in", m * ratio
                else:
                    m_in, m_out = m, m / ratio
                    other, m_other = "flange_out", m / ratio
                if first_visit:
                    seg.inertia += float(p.get("inertia_in_kgm2", 0)) * m_in * m_in
                    seg.inertia += float(p.get("inertia_out_kgm2", 0)) * m_out * m_out
                    seg.element_ms[el_id] = m_out  # record output-axis speed
                if (el_id, other) not in seen_ports:
                    queue.append((el_id, other, m_other, region, eta))
                enqueue_peers(el_id, pid, m, region)
            elif t == "mech.node":
                for pid2 in ("f1", "f2", "f3", "f4"):
                    if (el_id, pid2) not in seen_ports:
                        queue.append((el_id, pid2, m, region, None))
                enqueue_peers(el_id, pid, m, region)
            elif t in SOURCE_TYPES:
                if first_visit:
                    seg.inertia += float(p.get("inertia_kgm2", 0)) * m * m
                    seg.sources.append(SourceRef(
                        el_id=el_id, kind=SOURCE_TYPES[t], m=m, region=region))
                enqueue_peers(el_id, pid, m, region)
            elif t == "propulsion.wheel":
                if first_visit:
                    seg.inertia += float(p.get("inertia_kgm2", 0)) * m * m
                    seg.wheels.append(WheelRef(
                        el_id=el_id,
                        m=m,
                        radius=max(1e-3, float(p.get("radius_m", 0.3))),
                        load_share=max(0.0, float(p.get("vehicle_load_share_pct", 25)) / 100.0),
                        mu=max(0.0, float(p.get("mu", 1.0))),
                        c_slip=max(0.1, float(p.get("slip_stiffness", 10))),
                        c_rr=max(0.0, float(p.get("rolling_resistance", 0.012))),
                    ))
                enqueue_peers(el_id, pid, m, region)
            elif t == "mech.brake":
                if first_visit:
                    seg.inertia += float(p.get("inertia_kgm2", 0)) * m * m
                    seg.brakes.append(BrakeRef(
                        el_id=el_id, m=m,
                        max_torque=max(0.0, float(p.get("max_torque_Nm", 0))),
                    ))
                enqueue_peers(el_id, pid, m, region)
            elif t == "propulsion.propeller":
                if first_visit:
                    seg.inertia += float(p.get("inertia_kgm2", 0)) * m * m
                    seg.props.append(PropRef(
                        el_id=el_id, m=m,
                        t_ref=max(0.0, float(p.get("torque_ref_Nm", 0))),
                        n_ref=max(1.0, float(p.get("ref_speed_rpm", 1000))),
                    ))
                enqueue_peers(el_id, pid, m, region)
            else:
                enqueue_peers(el_id, pid, m, region)
        return seg

    def extract_driveline(group: set[str], gears: dict[str, float],
                          errors: list[str]) -> Driveline | None:
        """One mechanical connected group as a Driveline, with its gearboxes
        in ``gears`` (their default gear where absent). Reads the parameters
        from ``params_of``, so a later call (a gear shift during a run) sees
        the live values."""
        dl = Driveline(element_group=sorted(group))
        group_joints = sorted(g for g in group if g in joint_ids)

        # walk segments from every joint attachment (and pick up joint-free groups)
        seg_of_port: dict[tuple[str, str], int] = {}

        def segment_for(entry: tuple[str, str]) -> int:
            """Segment containing the element-port `entry` (walk on demand)."""
            if entry in seg_of_port:
                return seg_of_port[entry]
            if entry[0] in joint_ids:
                # two joints bolted directly together (e.g. axle differential
                # on a transfer case output): synthesize a mass-less coupling
                # segment carrying both attachment ports
                seg = Segment(inertia=1e-3)
                seg.port_ms[entry] = 1.0
                for peer in mech_adj.get(entry, ()):  # the requesting joint's port
                    seg.port_ms[peer] = 1.0
            else:
                seg = walk_segment([entry], gears)
            idx = len(dl.segments)
            dl.segments.append(seg)
            for key in seg.port_ms:
                seg_of_port[key] = idx
            return idx

        ok = True
        split_in: dict[int, tuple[str, str]] = {}  # segment → its port on a split's input
        clutch_ports: dict[int, list[tuple[str, str]]] = defaultdict(list)
        for j_el in group_joints:
            cdef = cdef_of[j_el]
            p = params_of[j_el]
            if cdef.id == "mech.clutch":
                pa = mech_adj.get((j_el, "flange_a"), [])
                pb = mech_adj.get((j_el, "flange_b"), [])
                if not pa or not pb:
                    errors.append(f"Clutch '{elements[j_el].label}' needs both flanges connected.")
                    ok = False
                    continue
                sa, sb = segment_for(pa[0]), segment_for(pb[0])
                if sa == sb:
                    errors.append(f"Clutch '{elements[j_el].label}' short-circuits a rigid "
                                  f"segment — that loop is not supported.")
                    ok = False
                    continue
                dl.joints.append(Joint(
                    el_id=j_el, kind="clutch",
                    child_a=sa, child_a_m=dl.segments[sa].port_ms[pa[0]],
                    child_b=sb, child_b_m=dl.segments[sb].port_ms[pb[0]],
                    child_a_region=dl.segments[sa].port_region.get(pa[0], 0),
                    child_b_region=dl.segments[sb].port_region.get(pb[0], 0),
                ))
                clutch_ports[sa].append(pa[0])
                clutch_ports[sb].append(pb[0])
            else:  # differential / transfer case → split
                pin = mech_adj.get((j_el, "flange_in"), [])
                pa = mech_adj.get((j_el, "flange_out_a"), [])
                pb = mech_adj.get((j_el, "flange_out_b"), [])
                if not pa or not pb:
                    errors.append(f"'{elements[j_el].label}' needs both outputs connected.")
                    ok = False
                    continue
                sa, sb = segment_for(pa[0]), segment_for(pb[0])
                sp = segment_for(pin[0]) if pin else -1
                if sa == sb or sa == sp or sb == sp:
                    errors.append(f"'{elements[j_el].label}' outputs reconnect mechanically "
                                  f"— that loop is not supported.")
                    ok = False
                    continue
                f_b = 0.5
                if cdef.id == "mech.transfer_case":
                    f_b = 1.0 - max(0.0, min(1.0, float(p.get("torque_split_a_pct", 50)) / 100.0))
                joint = Joint(
                    el_id=j_el, kind="split",
                    ratio=float(p.get("ratio", 1.0)) or 1.0,
                    eff=max(1e-3, float(p.get("efficiency_pct", 100)) / 100.0),
                    locked=bool(p.get("locked", False)),
                    f_b=f_b,
                    parent_seg=sp,
                    parent_m=dl.segments[sp].port_ms[pin[0]] if pin else 1.0,
                    child_a=sa, child_a_m=dl.segments[sa].port_ms[pa[0]],
                    child_b=sb, child_b_m=dl.segments[sb].port_ms[pb[0]],
                    child_a_region=dl.segments[sa].port_region.get(pa[0], 0),
                    child_b_region=dl.segments[sb].port_region.get(pb[0], 0),
                )
                # split carrier inertia lives on the input axis of its parent segment
                if sp >= 0:
                    dl.segments[sp].inertia += (float(p.get("inertia_kgm2", 0))
                                                * joint.parent_m ** 2)
                    split_in.setdefault(sp, pin[0])
                dl.joints.append(joint)
        if not ok:
            return None
        if not group_joints:
            # joint-free driveline: one segment; pick a stable reference axis
            entry: tuple[str, str] | None = None
            for e in sorted(group):
                cdef = cdef_of.get(e)
                if cdef and cdef.id == "propulsion.wheel":
                    entry = (e, "shaft")
                    break
            if entry is None:
                for e in sorted(group):
                    cdef = cdef_of.get(e)
                    if cdef and cdef.id in SOURCE_TYPES:
                        entry = (e, "shaft")
                        break
            if entry is None:
                for key in mech_adj:
                    if key[0] in group:
                        entry = key
                        break
            if entry is None:
                return None
            dl.segments.append(walk_segment([entry], gears))

        # gear losses follow the power, whichever port a segment was walked
        # from: each segment's output is where its power leaves towards the
        # road (a split's input, else a wheel, propeller or brake, else a
        # clutch)
        for s_idx, seg in enumerate(dl.segments):
            out = split_in.get(s_idx)
            if out is None:
                loads = [x.el_id for x in (*seg.wheels, *seg.props, *seg.brakes)]
                out = next((key for el_id in loads for key in seg.port_region
                            if key[0] == el_id), None)
            if out is None and clutch_ports.get(s_idx):
                out = clutch_ports[s_idx][0]
            seg.orient(out)

        # sanity: a segment must not be the parent of two open splits (its
        # speed would be doubly determined) — checked here structurally,
        # ignoring lock state (locks may change at runtime)
        parent_count: dict[int, int] = defaultdict(int)
        for j in dl.joints:
            if j.kind == "split" and j.parent_seg >= 0:
                parent_count[j.parent_seg] += 1
        for seg_idx, n in parent_count.items():
            if n > 1:
                errors.append("A rigid section feeds the input of two splits "
                              "(differential/transfer case) — that is kinematically "
                              "over-constrained and not supported.")
        return dl

    drivelines: list[Driveline] = []
    assigned: set[str] = set()
    for start_el in sorted(wired_mech):
        if start_el in assigned:
            continue
        group = mech_component(start_el)
        assigned |= group
        dl = extract_driveline(group, gear_of, errors)
        if dl is not None:
            drivelines.append(dl)

    # ---- electrical buses --------------------------------------------------
    parent: dict[tuple[str, str], tuple[str, str]] = {}

    def find(x: tuple[str, str]) -> tuple[str, str]:
        while parent.setdefault(x, x) != x:
            parent[x] = parent[parent[x]]
            x = parent[x]
        return x

    def union(a: tuple[str, str], b: tuple[str, str]) -> None:
        parent[find(a)] = find(b)

    connected_ports: set[tuple[str, str]] = set()
    for a, b in elec_links:
        union(a, b)
        connected_ports.add(a)
        connected_ports.add(b)
    for el_id, cdef in cdef_of.items():
        if cdef.id in ("electric.node", "boundary.ground"):
            eports = [p.id for p in cdef.ports if p.kind == "electrical"]
            for p1, p2 in zip(eports, eports[1:]):
                union((el_id, p1), (el_id, p2))

    groups: dict[tuple[str, str], list[tuple[str, str]]] = defaultdict(list)
    for el_id, cdef in cdef_of.items():
        for p in cdef.ports:
            if p.kind == "electrical":
                groups[find((el_id, p.id))].append((el_id, p.id))

    buses: list[Bus] = []
    bus_of_port: dict[tuple[str, str], Bus] = {}
    for i, (_, members) in enumerate(sorted(groups.items(), key=lambda kv: str(kv[0]))):
        bus = Bus(id=i)
        for el_id, pid in members:
            bus_of_port[(el_id, pid)] = bus
            cdef = cdef_of[el_id]
            if cdef.id == "boundary.ground":
                bus.grounded = True
            elif cdef.id == "electric.node":
                if el_id not in bus.nodes:
                    bus.nodes.append(el_id)
        buses.append(bus)

    def attached_bus(el_id: str, pid: str) -> Bus | None:
        if (el_id, pid) not in connected_ports:
            return None
        return bus_of_port.get((el_id, pid))

    def positive_bus(el_id: str, pid: str = "pos") -> Bus | None:
        """Bus behind a positive (+) terminal, if wired to a live rail.
        Power is delivered/drawn here; the negative terminal is the return."""
        bus = attached_bus(el_id, pid)
        return bus if (bus and not bus.grounded) else None

    # A component's negative (−) terminal defines its return path. The
    # simplified solver balances power on the positive rail only, so an
    # unwired return is allowed (implicit ground); we record it so the
    # data checks can note it.
    floating_returns: list[str] = []
    for el_id, cdef in cdef_of.items():
        neg_ports = [p.id for p in cdef.ports
                     if p.kind == "electrical" and p.polarity == "negative"]
        if neg_ports and not any((el_id, pid) in connected_ports for pid in neg_ports):
            floating_returns.append(el_id)

    for el_id, cdef in cdef_of.items():
        t = cdef.id
        label = elements[el_id].label
        if t == "battery.generic":
            bus = positive_bus(el_id)
            if bus is not None:
                if bus.battery:
                    errors.append(f"Bus has two batteries ('{label}' and "
                                  f"'{elements[bus.battery].label}') — not supported yet.")
                else:
                    bus.battery = el_id
        elif t == "electric.voltage_source":
            bus = positive_bus(el_id)
            if bus is not None:
                if bus.vsource:
                    errors.append("Bus has two voltage sources — not supported.")
                else:
                    bus.vsource = el_id
        elif t == "fuelcell.stack":
            bus = positive_bus(el_id)
            if bus is not None:
                if bus.fuelcell:
                    errors.append("Bus has two fuel cells — not supported yet.")
                else:
                    bus.fuelcell = el_id
        elif t == "electric.constant_drive":
            bus = positive_bus(el_id)
            if bus is not None:
                bus.consumers.append(el_id)
        elif t == "motor.emotor":
            bus = positive_bus(el_id)
            if bus is not None:
                bus.motors.append(el_id)
        elif t == "controller.dcdc":
            bus_a = positive_bus(el_id, "a_pos")
            bus_b = positive_bus(el_id, "b_pos")
            if bus_a is not None:
                bus_a.dcdc_in.append(el_id)
            if bus_b is not None:
                bus_b.dcdc_out.append(el_id)

    for bus in buses:
        primary = [s for s in (bus.battery, bus.vsource, bus.fuelcell) if s]
        if len(primary) > 1:
            errors.append("A bus with more than one primary source (battery / voltage "
                          "source / fuel cell) is not supported.")

    # solve order: a bus supplied by a DC-DC must be solved before the bus
    # the DC-DC draws from (its demand becomes load there)
    dcdc_a_bus: dict[str, Bus] = {}
    dcdc_b_bus: dict[str, Bus] = {}
    for bus in buses:
        for d in bus.dcdc_in:
            dcdc_a_bus[d] = bus
        for d in bus.dcdc_out:
            dcdc_b_bus[d] = bus
    order: list[Bus] = []
    pending = [b for b in buses if not b.grounded]
    deps: dict[int, set[int]] = {b.id: set() for b in pending}
    for d, bus_a in dcdc_a_bus.items():
        bus_b = dcdc_b_bus.get(d)
        if bus_b and bus_a.id != bus_b.id and bus_a.id in deps:
            deps[bus_a.id].add(bus_b.id)
    while pending:
        ready = [b for b in pending if not (deps[b.id] - {o.id for o in order})]
        if not ready:
            errors.append("DC-DC converters form a loop between buses — not supported.")
            order.extend(pending)
            break
        for b in ready:
            order.append(b)
            pending.remove(b)

    # ---- vehicle & tanks -----------------------------------------------------
    def single(type_id: str, what: str) -> str | None:
        found = [el_id for el_id, cdef in cdef_of.items() if cdef.id == type_id]
        if len(found) > 1:
            errors.append(f"Only one {what} element per model is supported.")
        return found[0] if found else None

    vehicle = single("vehicle.body", "Vehicle")
    driver = single("driver.driver", "Driver")
    fuel_tank = single("fuel.tank", "Fuel Tank")
    h2_tank = single("fuel.h2_tank", "Hydrogen Tank")

    any_wheels = any(seg.wheels for dl in drivelines for seg in dl.segments)
    if any_wheels and not vehicle:
        warnings.append(NO_VEHICLE)
    if vehicle and not any_wheels:
        warnings.append(NO_WHEELS)
    if vehicle and any_wheels and not driver:
        warnings.append(NO_DRIVER)
    has_engine = any(cdef.id == "engine.combustion" for cdef in cdef_of.values())
    if has_engine and not fuel_tank:
        warnings.append("Combustion engine without a Fuel Tank — running on infinite fuel.")
    has_fc = any(cdef.id == "fuelcell.stack" for cdef in cdef_of.values())
    if has_fc and not h2_tank:
        warnings.append("Fuel cell without a Hydrogen Tank — running on infinite hydrogen.")

    # ---- signal-block evaluation order ---------------------------------------
    block_ids = [el_id for el_id, cdef in cdef_of.items()
                 if cdef.id in SIGNAL_BLOCK_TYPES]
    block_deps: dict[str, set[str]] = {s: set() for s in block_ids}
    for (in_el, _), (out_el, _) in signal_route.items():
        if in_el in block_deps and out_el in block_deps and in_el != out_el:
            block_deps[in_el].add(out_el)
    ordered: list[str] = []
    remaining = set(block_ids)
    while remaining:
        ready_s = sorted(s for s in remaining if not (block_deps[s] & remaining))
        if not ready_s:  # cycle: evaluate in stable order with one-step delay
            warnings.append("Signal blocks form a loop — resolved with a one-step delay.")
            ready_s = sorted(remaining)
        for s in ready_s:
            ordered.append(s)
            remaining.discard(s)

    if errors:
        raise ModelError(errors)

    return Model(
        elements=elements,
        cdef_of=cdef_of,
        params_of=params_of,
        signal_route=signal_route,
        drivelines=drivelines,
        buses=order,
        vehicle=vehicle,
        driver=driver,
        fuel_tank=fuel_tank,
        h2_tank=h2_tank,
        signal_blocks=ordered,
        floating_returns=floating_returns,
        warnings=warnings,
        rewalk=lambda dl, gears: extract_driveline(set(dl.element_group), gears, []),
    )
