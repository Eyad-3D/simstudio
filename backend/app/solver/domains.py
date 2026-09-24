"""Wholesale-wrapped domain slaves (Phase 1.4).

The former monolithic step loop is decomposed into six slaves, all stepped
by the co-simulation master at every solver step (≤ MAX_SUBSTEP) in this
order:

    control → gear → limits → driver → mechanical+vehicle → electrical

"limits" and the start of the mechanical step form the source-limit
handshake: each electrical bus states what its source can deliver and
absorb over the step before any motor torque is applied, so motors never
draw power a source does not have and never feed back more than it takes.

The pass bodies are moved verbatim; shared physics state lives in a single
:class:`RunContext` that every slave reads and writes — the honest wrap
stage. True decoupling (declared variables exchanged through the master's
pool, per-component model extraction, tanks as their own slaves) is the
next phase; until then the physics slaves integrate with ``ctx.dt`` (the
current solver step, equal to their ``h``) and the control slave uses its
``do_step(t, h)`` arguments.
"""
from __future__ import annotations

import math
from collections import defaultdict
from dataclasses import dataclass

from .maps import Map, MapUse, TableError, interp1, parse_table1d, parse_table2d
from .network import ROAD_LOAD_ABC, BrakeRef, Driveline, Joint, Model, Segment, SourceRef
from .profiles import interp_profile, parse_profile
from .runtime import (
    AIR_DENSITY,
    AMBIENT_C,
    AMBIENT_KPA,
    CLUTCH_BAND,
    GRAVITY,
    RPM,
    SPEED_LIMIT_BAND,
    V_EPS,
    W_EPS,
    BatteryState,
    DrivelineState,
    DrivePlan,
    EngineCache,
    FuelCellCache,
    MotorCache,
    Runtime,
    SingularMatrixError,
    TankState,
    _sign,
    air_density,
    make_plan,
    motor_max_rpm,
    ocv_mean,
    solve_linear,
)
from .sandbox import ScriptSandbox, ScriptSpec
from .scripting import ScriptError
from .slave import ParamResult, Slave, StepResult, VarDef, split_var


class ModelInitError(Exception):
    """Element caches could not be built (bad tables / scripts)."""

    def __init__(self, messages: list[str]):
        super().__init__("; ".join(messages))
        self.messages = messages


@dataclass
class DrivelineLayout:
    """What the solver step needs from a driveline's plan that changes only
    when the plan is rebuilt (gear shift, lock toggle), worked out once per
    plan instead of every solver step."""

    # per segment: (i, k, J·g_i·g_k) for i ≤ k with g_i ≠ 0 — its inertia in
    # the mass matrix
    inertia: list[list[tuple[int, int, float]]]
    # per segment: (i, k, g_i, g_k) for i ≤ k with g_i ≠ 0 — for tire damping
    pairs: list[list[tuple[int, int, float, float]]]
    # per segment: the coordinate a static brake hold acts on, if any
    hold_coord: list[int | None]
    # per coordinate: a segment with brakes moves with it
    braked: list[bool]
    # clutches: (joint, g of side a, g of side b, (i, k, r_i, r_k) of a − b)
    clutches: list[tuple[Joint, list[float], list[float], list[tuple[int, int, float, float]]]]
    # per segment: the clutch torques acting on it, for its gear losses —
    # (clutch index, clutch torque → torque at the reference axis, region)
    clutch_arms: list[list[tuple[int, float, int]]]
    has_wheels: bool
    # motors for the driver's recuperation estimate: (segment, motor, ratio)
    motors: list[tuple[int, SourceRef, float]]


def through_gears(seg: Segment, t_at: list[float], omega: float) -> float:
    """The net torque that reaches a segment's output from the torques
    entering its regions (``t_at``, at the reference axis, which it uses
    up), after the losses of the gear stages on the way. Each stage passes
    on ``eff`` × the net torque through it when that power flows towards
    the output, and asks 1/``eff`` × when it flows back (regeneration, a
    dragged engine), so the losses do not depend on the axis the segment
    was walked from."""
    stages = seg.stages
    for r in reversed(seg.stage_order):  # the outermost regions first
        t = t_at[r]
        if t:
            stage = stages[r]
            t_at[stage.parent] += t * stage.eff if t * omega >= 0 else t / stage.eff
    return t_at[seg.out_region]


class RunContext:
    """All shared solver state for one run — the former closure variables of
    simulate(), promoted to attributes so the domain slaves can share them."""

    def __init__(
        self,
        project,
        model: Model,
        rt: Runtime,
        gear_of: dict[str, float],
        case_overrides: dict,
    ):
        self.project = project
        self.model = model
        self.rt = rt
        self.gear_of = gear_of
        self.case_overrides = case_overrides
        self.dt = 0.0  # current solver step (set by simulate before stepping)
        self.t = 0.0  # its start time (set by simulate before stepping)
        # How far the run went past its data, the one place to read it: a
        # maps.MapUse per axis of every table the run reads (table()) and one
        # per E-Motor and Engine for its maximum speed (what = "maximum
        # speed": a motor's Maximum Speed, an engine's full-load curve's last
        # speed). The run summary lists the records with outside_s > 0.
        self.map_use: list[MapUse] = []

        # ---- element caches --------------------------------------------------
        self.batteries: dict[str, BatteryState] = {}
        self.motors: dict[str, MotorCache] = {}
        self.engines: dict[str, EngineCache] = {}
        self.fuelcells: dict[str, FuelCellCache] = {}
        self.tanks: dict[str, TankState] = {}
        self.lookup_cache: dict[str, tuple[Map, Map]] = {}
        self.profile_cache: dict[str, list[tuple[float, float]]] = {}
        self.sources = [(el_id, cdef.id) for el_id, cdef in model.cdef_of.items()
                        if cdef.id in ("signal.constant", "signal.driving_task")]
        self.pid_state: dict[str, dict[str, float]] = {}
        table_errors: list[str] = []
        for el_id, cdef in model.cdef_of.items():
            p = self.params(el_id)
            label = model.elements[el_id].label
            try:
                if cdef.id == "battery.generic":
                    ocv_map = self.table(el_id, "ocv_table")
                    # Usable Capacity is the open-circuit energy from full to
                    # empty; without a Charge Capacity (old projects have none)
                    # the amp-hours come from it at the OCV table's mean voltage
                    q_ah = max(0.0, float(p.get("capacity_Ah", 0) or 0)) or (
                        max(1e-3, float(p.get("capacity_kWh", 60))) * 1000.0
                        / max(1e-6, ocv_mean(ocv_map.pts, ocv_map.linear[0])))
                    b = BatteryState(
                        el_id=el_id,
                        soc=float(p.get("initial_soc_pct", 90)) / 100.0,
                        q_ah=q_ah,
                        min_soc=float(p.get("min_soc_pct", 10)) / 100.0,
                        r0=max(1e-6, float(p.get("internal_resistance_ohm", 0.08))),
                        r1=max(0.0, float(p.get("rc_resistance_ohm", 0))),
                        tau=max(0.0, float(p.get("rc_time_constant_s", 0))),
                        max_charge_w=max(0.0, float(p.get("max_charge_power_kW", 120))) * 1000.0,
                        ocv_map=ocv_map,
                        eta_charge=min(1.0, max(1e-3, float(
                            p.get("coulombic_efficiency_pct", 100)) / 100.0)),
                    )
                    # read without the Error check: the first step stops the
                    # run, with its time, if the SOC is outside an Error axis
                    b.v_term = interp1(ocv_map.pts, b.soc_pct(), ocv_map.linear[0])
                    self.batteries[el_id] = b
                elif cdef.id == "motor.emotor":
                    full_load = self.table(el_id, "full_load_torque")
                    max_rpm = motor_max_rpm(full_load.pts, p.get("max_speed_rpm", 0))
                    self.motors[el_id] = MotorCache(
                        el_id=el_id,
                        full_load=full_load,
                        loss=self.table(el_id, "power_loss"),
                        drag=self.table(el_id, "drag_torque"),
                        q4_scale=max(0.0, float(p.get("q4_torque_scale_pct", 100)) / 100.0),
                        max_rpm=max_rpm,
                        speed_use=self.speed_use(el_id, max_rpm),
                    )
                elif cdef.id == "engine.combustion":
                    full_load = self.table(el_id, "full_load_torque")
                    self.engines[el_id] = EngineCache(
                        el_id=el_id,
                        full_load=full_load,
                        drag=self.table(el_id, "drag_torque"),
                        fuel_map=self.table(el_id, "fuel_map"),
                        idle_rpm=max(1.0, float(p.get("idle_speed_rpm", 800))),
                        reentry_rpm=float(p.get("fuel_cut_reentry_rpm", 1100)),
                        speed_use=self.speed_use(el_id, full_load.pts[-1][0]),
                    )
                elif cdef.id == "fuelcell.stack":
                    self.fuelcells[el_id] = FuelCellCache(
                        el_id=el_id,
                        pol=self.table(el_id, "polarization"),
                        i_max=max(1.0, float(p.get("max_current_A", 400))),
                        h2_g_per_kwh=max(0.0, float(p.get("h2_per_kwh_g", 55))),
                    )
                elif cdef.id in ("fuel.tank", "fuel.h2_tank"):
                    cap = max(1e-3, float(p.get("capacity_kg", 45)))
                    self.tanks[el_id] = TankState(
                        el_id=el_id, capacity_kg=cap,
                        mass_kg=cap * max(0.0, min(1.0, float(p.get("initial_fill_pct", 90)) / 100.0)),
                    )
                elif cdef.id == "signal.lookup":
                    self.lookup_cache[el_id] = (self.table(el_id, "table_1d"),
                                                self.table(el_id, "table_2d"))
                elif cdef.id == "control.pid":
                    self.pid_state[el_id] = {"integral": 0.0, "prev_err": 0.0}
            except TableError as e:
                table_errors.append(f"'{label}': {e}")
        if table_errors:
            raise ModelInitError(table_errors)

        # Script blocks run in a locked-down worker process (app/solver/
        # sandbox.py): one worker for the whole run, reused every step, with the
        # block's state kept in the worker. Only built when the model has Script
        # blocks, so a model without them starts no worker.
        self.sandbox: ScriptSandbox | None = None
        script_specs: list[ScriptSpec] = []
        for el_id in model.signal_blocks:
            if model.cdef_of[el_id].id != "signal.script":
                continue
            el = model.elements[el_id]
            script_specs.append(ScriptSpec(
                el_id=el_id,
                label=el.label,
                code=str(self.params(el_id).get("code", "")),
                input_keys=[p.id for p in (el.dynamicPorts or [])
                            if p.direction == "input"],
                params=dict(self.params(el_id)),
            ))
        if script_specs:
            try:
                self.sandbox = ScriptSandbox(script_specs)
            except ScriptError as e:
                raise ModelInitError([str(e)])

        # ---- driveline & vehicle states --------------------------------------
        self.veh_id = model.vehicle
        veh_p = self.params(self.veh_id) if self.veh_id else {}
        self.veh_mass = max(1.0, float(veh_p.get("mass_kg", 1800))) if self.veh_id else 0.0
        self.v = max(0.0, float(veh_p.get("initial_speed_kmh", 0)) / 3.6) if self.veh_id else 0.0
        self.distance = 0.0
        self.amb_id = model.ambient  # sets the air density (None: 20 °C, 101.325 kPa)
        # the road's slope this step (set from the grade at the start of the
        # mechanical step): the weight's share along and normal to the road
        self.slope_sin, self.slope_cos = 0.0, 1.0
        self.driver_integral = 0.0
        self.performance = False  # a performance-test case (set by simulate)

        self.dls = [DrivelineState(dl=dl) for dl in model.drivelines]
        # the gear each gearbox's driveline was built in (its default gear
        # unless the caller chose one); a first control sample asking for it
        # changes nothing
        for dl in model.drivelines:
            for seg in dl.segments:
                for gb in seg.gearboxes:
                    gear_of.setdefault(gb.el_id, float(
                        self.params(gb.el_id).get("default_gear", 1) or 1))
        self.gears_checked = False  # the first gear check is initialisation
        self.normalize_wheel_loads()
        self.el_axis_speed: dict[str, float] = {}  # anchor speeds for plan rebuilds
        # bumped whenever a driveline or its plan changes (gear shift, lock
        # toggle), so cached views of them (routed state getters) refresh
        self.layout_version = 0
        for st in self.dls:
            self.rebuild_plan(st, initial=True)

        # bus lookups
        self.motor_bus: dict[str, object] = {}
        for bus in model.buses:
            for m_id in bus.motors:
                self.motor_bus[m_id] = bus
        self.bus_motors = {bus.id: [self.motors[m] for m in bus.motors if m in self.motors]
                           for bus in model.buses}
        self.bus_voltage: dict[int, float] = {}
        for bus in model.buses:
            if bus.battery:
                self.bus_voltage[bus.id] = self.batteries[bus.battery].v_term
            elif bus.vsource:
                self.bus_voltage[bus.id] = float(self.params(bus.vsource).get("voltage_V", 400))
            elif bus.fuelcell:
                pol = self.fuelcells[bus.fuelcell].pol
                self.bus_voltage[bus.id] = interp1(pol.pts, 0.0, pol.linear[0])
            elif bus.dcdc_out:
                self.bus_voltage[bus.id] = float(
                    self.params(bus.dcdc_out[0]).get("output_voltage_V", 400))
            else:
                self.bus_voltage[bus.id] = 0.0

        self.vsource_energy_wh: dict[str, float] = defaultdict(float)
        self.dcdc_flows: dict[str, tuple[float, float]] = {}
        self.bus_loads: dict[int, float] = {}
        self.last_forces: dict[str, float] = {}

        # source trees: a bus with its own source (or none) plus the buses
        # DC-DC converters feed from it, as [(bus, feeding DC-DC, parent bus)]
        self.dcdc_in_bus = {d: bus for bus in model.buses for d in bus.dcdc_in}
        # converters feeding a battery bus run at their power setpoint
        self.setpoint_dcdcs = {d for bus in model.buses if bus.battery for d in bus.dcdc_out}
        parent_of: dict[int, tuple[str, object]] = {}
        for bus in model.buses:
            if not (bus.battery or bus.vsource or bus.fuelcell) and bus.dcdc_out:
                up = self.dcdc_in_bus.get(bus.dcdc_out[0])
                if up is not None and up is not bus:
                    parent_of[bus.id] = (bus.dcdc_out[0], up)
        self.bus_tree: dict[int, list[tuple[object, str | None, object]]] = {}
        for root in model.buses:
            if root.id in parent_of:
                continue
            nodes = [(root, None, None)]
            for node, _, _ in nodes:  # grows while iterating: breadth-first
                nodes.extend((b, parent_of[b.id][0], node) for b in model.buses
                             if b.id in parent_of and parent_of[b.id][1] is node)
            self.bus_tree[root.id] = nodes
        # handshake results for the current solver step (update_source_limits)
        self.source_window: dict[int, tuple[float, float]] = {}  # root → (deliver, absorb) W
        self.motor_room: dict[int, tuple[float, float]] = {}  # root → motor (lo, hi) W
        self.regen_room_w: dict[int, float] = {}  # bus → power its motors may feed back
        self.fixed_served_w: dict[int, float] = {}  # bus → served consumers + setpoints
        self.consumer_w: dict[str, float] = {}  # served consumer power
        self.dcdc_setpoint_w: dict[str, float] = {}  # setpoint DC-DC output allowed
        # per-step electrical energy balance: what no source supplied or took
        self.residual_wh = 0.0
        self.throughput_wh = 0.0

    # ---- shared helpers ---------------------------------------------------------

    def params(self, el_id: str) -> dict:
        return self.model.params_of[el_id]

    def table(self, el_id: str, key: str) -> Map:
        """A table parameter as a Map. Its "outside the data" setting per
        axis is the element's own (tableOutside) when that names one per
        axis, else the library's (Clamp where the library names none)."""
        el, cdef = self.model.elements[el_id], self.model.cdef_of[el_id]
        pdef = next(pp for pp in cdef.parameters if pp.key == key)
        axes = pdef.axes or []
        policy = [a.outside or "clamp" for a in axes]
        own = (el.tableOutside or {}).get(key)
        if own and len(own) == len(axes):
            policy = list(own)
        raw = self.params(el_id).get(key)
        pts = parse_table2d(raw) if pdef.type == "table2d" else parse_table1d(raw)
        uses = [MapUse(el_id, f"'{pdef.label}' table", a.name, a.unit, 0.0) for a in axes]
        self.map_use += uses
        return Map(pts, f"{cdef.name} '{el.label}' {pdef.label}", policy, uses)

    def speed_use(self, el_id: str, n_max: float) -> MapUse:
        use = MapUse(el_id, "maximum speed", "Speed", "1/min", n_max)
        self.map_use.append(use)
        return use

    def over_speed(self, m: MotorCache | EngineCache, rpm: float, n_max: float) -> bool:
        """Book a solver step a motor or engine spent above its maximum speed;
        True the first time. Its limiter acts from the step after the one
        that took it past, so a step of its own drive can overshoot the
        limit: while it falls back from there it is held at its limiter, not
        driven above it, and that is not counted. Anything that drives it
        higher is, until it is back below."""
        if rpm <= n_max * (1.0 + 1e-9):  # (float noise at the limit)
            m.overshoot_rpm = 0.0
        elif m.p_mech_w > 0.0:  # the last step's own drive took it here
            m.overshoot_rpm = rpm
        elif rpm > m.overshoot_rpm:
            m.overshoot_rpm = 0.0
            return m.speed_use.count(rpm, n_max, self.t, self.dt)
        return False

    def used(self, m: Map, *point: float) -> None:
        """Book the point this solver step's result read a table at (not the
        trial lookups before it); the first time an axis set to Clamp or
        Linear is left, say so (info: the summary says for how long)."""
        for use in m.count(self.t, self.dt, *point):
            how = ("its edge slope is extended" if m.linear[m.uses.index(use)]
                   else "its edge value is held")
            self.rt.message(
                "info",
                f"{m.name}: {use.axis} {use.value:.6g} {use.unit} is past the edge of its data "
                f"({use.edge:g} {use.unit}) at t = {use.t:.2f} s — {how} there. The run "
                f"summary says for how long and how far.")

    def normalize_wheel_loads(self) -> None:
        """Rest the vehicle's whole weight on its wheels: each connected
        wheel carries its Vehicle Load Share of the sum over all of them.
        Shares that already add up to 100 % are used as they are."""
        wheels = [w for st in self.dls for seg in st.dl.segments for w in seg.wheels]
        raw = [max(0.0, float(self.params(w.el_id).get("vehicle_load_share_pct", 25)) / 100.0)
               for w in wheels]
        total = sum(raw)
        scale = total > 0 and abs(total - 1.0) > 1e-9
        for w, share in zip(wheels, raw):
            w.load_share = share / total if scale else share

    def profile_points(self, el_id: str) -> list[tuple[float, float]]:
        """A Driving Task / Road Profile's points — parsed on first use and
        again only after a live edit of its profile, not on every step."""
        pts = self.profile_cache.get(el_id)
        if pts is None:
            pts = parse_profile(str(self.params(el_id).get("profile", "")))
            self.profile_cache[el_id] = pts
        return pts

    def anchor_of_group(self, dl: Driveline, plan: DrivePlan, coord: int) -> tuple[str, float] | None:
        root = plan.coord_root[coord]
        for s, seg in enumerate(dl.segments):
            if plan.root_of_seg[s] != root:
                continue
            for w in seg.wheels:
                return w.el_id, w.m * plan.scale_of_seg[s]
            for src in seg.sources:
                return src.el_id, src.m * plan.scale_of_seg[s]
            for pr in seg.props:
                return pr.el_id, pr.m * plan.scale_of_seg[s]
        return None

    def rebuild_plan(self, st: DrivelineState, initial: bool = False) -> None:
        st.plan = make_plan(st.dl, self.model.params_of, self.gear_of)
        st.layout = None
        self.layout_version += 1
        if st.plan.over_constrained:
            self.rt.warn_once("overconstrained",
                              "Driveline became kinematically over-constrained — "
                              "its motion is frozen.")
            return
        if initial:
            self.start_at_vehicle_speed(st)
            return
        for k in range(st.plan.n):
            anchor = self.anchor_of_group(st.dl, st.plan, k)
            if anchor is None:
                st.plan.x[k] = 0.0
                continue
            el_id, factor = anchor
            st.plan.x[k] = self.el_axis_speed.get(el_id, 0.0) / factor if factor else 0.0

    def clutch_closed_at_start(self, j: Joint) -> bool:
        """A clutch closes at t = 0 when unwired, or when a Constant or
        Driving Task engages it; one a block sets (a Script, a PID …) has
        had no say yet, so it counts as open."""
        if float(self.params(j.el_id).get("max_torque_Nm", 0)) <= 0:
            return False
        route = self.model.signal_route.get((j.el_id, "sig_engage_in"))
        if route is None:
            return True
        engage = self.rt.signal_values.get(route)
        kind = dict(self.sources).get(route[0])
        if engage is None and kind is not None:
            engage = self.source_value(route[0], kind, 0.0)
        return (engage or 0.0) > 0

    def start_at_vehicle_speed(self, st: DrivelineState) -> None:
        """Set a driveline's coordinates so that every wheel rolls at the
        vehicle's speed without slip and every clutch closed at the start
        turns both its sides together (least squares over those conditions,
        which agree for any buildable driveline); what none of them moves,
        such as an engine behind an open clutch, starts at rest."""
        plan, n = st.plan, st.plan.n
        rows: list[tuple[list[float], float]] = []  # (a, b): a · x = b, |a| = 1

        def condition(a: list[float], b: float) -> None:
            norm = math.sqrt(sum(v * v for v in a))
            if norm > 0:
                rows.append(([v / norm for v in a], b / norm))

        for s, seg in enumerate(st.dl.segments):
            for w in seg.wheels:
                condition([w.m * g for g in plan.gvec[s]], self.v / w.radius)
        for j in st.dl.joints:
            if j.kind == "clutch" and self.clutch_closed_at_start(j):
                condition([j.child_a_m * ga - j.child_b_m * gb for ga, gb
                           in zip(plan.gvec[j.child_a], plan.gvec[j.child_b])], 0.0)
        ata = [[sum(a[i] * a[k] for a, _ in rows) for k in range(n)] for i in range(n)]
        atb = [sum(a[i] * b for a, b in rows) for i in range(n)]
        # a small ridge keeps an under-determined driveline solvable; a
        # coordinate no condition touches stays at 0
        for i in range(n):
            ata[i][i] += 1e-9 if ata[i][i] > 0 else 1.0
        plan.x[:] = solve_linear(ata, atb)
        # the rotating parts' speeds are right from point 0 on (recorded
        # speeds, a script's first reading)
        for s, seg in enumerate(st.dl.segments):
            omega = self.seg_speed(st, s)
            for w in seg.wheels:
                self.el_axis_speed[w.el_id] = w.m * omega
            for src in seg.sources:
                self.el_axis_speed[src.el_id] = src.m * omega
                cache = self.motors.get(src.el_id) or self.engines.get(src.el_id)
                if cache is not None:
                    cache.rpm = abs(src.m * omega) * RPM
            for pr in seg.props:
                self.el_axis_speed[pr.el_id] = pr.m * omega

    @staticmethod
    def seg_speed(st: DrivelineState, s: int) -> float:
        plan = st.plan
        g, x, n = plan.gvec[s], plan.x, plan.n
        # the same sums, in the same order, as sum() below
        if n == 1:
            return 0.0 + g[0] * x[0]
        if n == 2:
            return 0.0 + g[0] * x[0] + g[1] * x[1]
        if n == 3:
            return 0.0 + g[0] * x[0] + g[1] * x[1] + g[2] * x[2]
        return sum(g[i] * x[i] for i in range(n))

    def layout(self, st: DrivelineState) -> DrivelineLayout:
        """The driveline's layout for its current plan (plan not
        over-constrained, n > 0)."""
        if st.layout is not None:
            return st.layout
        plan = st.plan
        n, segs = plan.n, st.dl.segments
        inertia, pairs, hold = [], [], []
        for s_idx, seg in enumerate(segs):
            g = plan.gvec[s_idx]
            j_seg = max(1e-4, seg.inertia)
            nz = [(i, k, g[i], g[k]) for i in range(n) if g[i] != 0.0 for k in range(i, n)]
            inertia.append([(i, k, j_seg * gi * gk) for i, k, gi, gk in nz])
            pairs.append(nz)
            hold.append(next((kk for kk in range(n)
                              if plan.coord_root[kk] == plan.root_of_seg[s_idx]), None))
        braked = [any(seg.brakes and plan.root_of_seg[s2] == plan.coord_root[i]
                      for s2, seg in enumerate(segs)) for i in range(n)]
        clutches = []
        clutch_arms: list[list[tuple[int, float, int]]] = [[] for _ in segs]
        for j in st.dl.joints:
            if j.kind != "clutch":
                continue
            ga = [j.child_a_m * x for x in plan.gvec[j.child_a]]
            gb = [j.child_b_m * x for x in plan.gvec[j.child_b]]
            rel = [ga[i] - gb[i] for i in range(n)]
            clutch_arms[j.child_a].append((len(clutches), -j.child_a_m, j.child_a_region))
            clutch_arms[j.child_b].append((len(clutches), j.child_b_m, j.child_b_region))
            clutches.append((j, ga, gb, [(i, k, rel[i], rel[k]) for i in range(n)
                                         if rel[i] != 0.0 for k in range(i, n)]))
        ones = [1.0] * n
        motors = [(s_idx, src, abs(src.m * sum(plan.gvec[s_idx][i] * ones[i] for i in range(n))))
                  for s_idx, seg in enumerate(segs) for src in seg.sources
                  if src.kind == "motor" and src.el_id in self.motors]
        st.layout = DrivelineLayout(
            inertia=inertia, pairs=pairs, hold_coord=hold, braked=braked, clutches=clutches,
            clutch_arms=clutch_arms, has_wheels=any(seg.wheels for seg in segs), motors=motors)
        return st.layout

    def source_value(self, el_id: str, kind: str, t: float) -> float:
        """A signal source's output (Constant, Driving Task) at time ``t`` —
        a pure function of time, so it can be evaluated at any instant
        without side effects."""
        p = self.params(el_id)
        if kind == "signal.constant":
            return float(p.get("value", 0))
        scale = float(p.get("scale_pct", 100)) / 100.0
        return interp_profile(self.profile_points(el_id), t, bool(p.get("repeat", False))) * scale

    def publish_sources(self, t: float) -> None:
        """Publish every signal source's output at time ``t``."""
        rt = self.rt
        for el_id, kind in self.sources:
            rt.publish(el_id, "sig_out" if kind == "signal.constant" else "sig_demand",
                       self.source_value(el_id, kind, t))

    # ---- live parameter updates ---------------------------------------------------

    def apply_set_param(self, el_id: str, key: str, value) -> ParamResult:
        model, rt = self.model, self.rt
        if el_id not in model.params_of:
            return "invalid"
        label = model.elements[el_id].label
        pdef = next(
            (pp for pp in model.cdef_of[el_id].parameters if pp.key == key), None)
        if pdef is not None and pdef.variability == "fixed":
            # kept out of the live set, so a later gear shift (which re-walks
            # the driveline from it) cannot apply it early either
            rt.warn_once(
                f"live-structural:{el_id}:{key}",
                f"'{label}.{key}' changed — structural parameters take effect on the next run.",
                level="info",
            )
            return "deferred"
        model.params_of[el_id][key] = value
        if key == "profile":
            self.profile_cache.pop(el_id, None)  # re-parse on next use
        if key == "locked":
            for st in self.dls:
                if any(j.el_id == el_id for j in st.dl.joints):
                    self.rebuild_plan(st)
            return "applied"
        try:
            if el_id in self.motors:
                mc = self.motors[el_id]
                p = self.params(el_id)
                mc.q4_scale = max(0.0, float(p.get("q4_torque_scale_pct", 100)) / 100.0)
                mc.full_load.set(parse_table2d(p.get("full_load_torque", {})))
                mc.loss.set(parse_table2d(p.get("power_loss", {})))
                mc.drag.set(parse_table1d(p.get("drag_torque", {})))
                mc.max_rpm = motor_max_rpm(mc.full_load.pts, p.get("max_speed_rpm", 0))
            if el_id in self.engines:
                ec = self.engines[el_id]
                p = self.params(el_id)
                ec.idle_rpm = max(1.0, float(p.get("idle_speed_rpm", ec.idle_rpm)))
                ec.reentry_rpm = float(p.get("fuel_cut_reentry_rpm", ec.reentry_rpm))
                ec.full_load.set(parse_table1d(p.get("full_load_torque", {})))
                ec.drag.set(parse_table1d(p.get("drag_torque", {})))
                ec.fuel_map.set(parse_table2d(p.get("fuel_map", {})))
            if el_id in self.fuelcells:
                fc = self.fuelcells[el_id]
                p = self.params(el_id)
                fc.i_max = max(1.0, float(p.get("max_current_A", fc.i_max)))
                fc.h2_g_per_kwh = max(0.0, float(p.get("h2_per_kwh_g", fc.h2_g_per_kwh)))
                fc.pol.set(parse_table1d(p.get("polarization", {})))
            if el_id in self.batteries:
                b = self.batteries[el_id]
                p = self.params(el_id)
                b.min_soc = float(p.get("min_soc_pct", 10)) / 100.0
                b.r0 = max(1e-6, float(p.get("internal_resistance_ohm", b.r0)))
                b.max_charge_w = max(0.0, float(p.get("max_charge_power_kW", 120))) * 1000.0
                b.ocv_map.set(parse_table1d(p.get("ocv_table", {})))
        except TableError:
            rt.warn_once(f"live-table:{el_id}", f"Live table edit on '{label}' is invalid — ignored.")
        p = self.params(el_id)
        for st in self.dls:
            for seg in st.dl.segments:
                for w in seg.wheels:
                    if w.el_id == el_id:
                        w.mu = max(0.0, float(p.get("mu", w.mu)))
                        w.c_slip = max(0.1, float(p.get("slip_stiffness", w.c_slip)))
                        w.c_rr = max(0.0, float(p.get("rolling_resistance", w.c_rr)))
                for br in seg.brakes:
                    if br.el_id == el_id:
                        br.max_torque = max(0.0, float(p.get("max_torque_Nm", br.max_torque)))
        if key == "vehicle_load_share_pct":
            self.normalize_wheel_loads()
        return "applied"

    # ---- behaviors -------------------------------------------------------------

    def motor_volts(self, mc: MotorCache) -> float:
        bus = self.motor_bus.get(mc.el_id)
        return self.bus_voltage.get(bus.id, 0.0) if bus is not None else 0.0

    def motor_command(self, mc: MotorCache, demand: float, omega_m: float) -> tuple[float, bool]:
        """(torque the traction command asks for, inverter on). The inverter
        is off for a command of exactly 0, without a live supply and above
        the motor's maximum speed (no drive, no regeneration); its drive
        torque falls to zero over the last 2 % below that speed."""
        rt, model = self.rt, self.model
        volts = self.motor_volts(mc)
        if volts <= 1.0:
            rt.warn_once(f"deadbus:{mc.el_id}",
                         f"E-Motor '{model.elements[mc.el_id].label}' has no live electrical "
                         f"supply — it produces no torque.")
            return 0.0, False
        rpm = abs(omega_m) * RPM
        if demand == 0.0 or rpm > mc.max_rpm * (1.0 + 1e-9):  # (float noise at the limit)
            return 0.0, False
        t_full = mc.full_load.at(volts, rpm)
        demand = max(-1.0, min(1.0, demand))
        if demand > 0 and rpm > mc.max_rpm * (1.0 - SPEED_LIMIT_BAND):
            t_full *= max(0.0, mc.max_rpm - rpm) / (SPEED_LIMIT_BAND * mc.max_rpm)
            if f"maxspeed:{mc.el_id}" not in rt.warned:
                note = (", the last speed point of its full-load curve"
                        if mc.max_rpm == motor_max_rpm(mc.full_load.pts, 0) else "")
                rt.warn_once(
                    f"maxspeed:{mc.el_id}",
                    f"E-Motor '{model.elements[mc.el_id].label}' reached its maximum speed "
                    f"({mc.max_rpm:,.0f} 1/min{note}) at t = {self.t:.2f} s — its drive torque "
                    f"falls to zero over the last 2 % below it.",
                    level="info",
                )
        return demand * t_full * (mc.q4_scale if demand < 0 else 1.0), True

    @staticmethod
    def motor_power(mc: MotorCache, torque: float, omega_m: float) -> float:
        """Electrical power of the powered motor: shaft power + map loss."""
        return torque * omega_m + mc.loss.at(abs(omega_m) * RPM, abs(torque)) * 1000.0

    def motor_torque(self, mc: MotorCache, demand: float, omega_m: float) -> float:
        """Shaft torque of an E-Motor for a traction command in [-1, 1].

        The loss map (motor + inverter) holds every loss of the powered
        drive, including the spin losses at zero torque, so electrical power
        = torque · speed + map loss. The drag table applies only while the
        inverter is off (a zero command or no live supply): the motor then
        coasts unpowered, draws nothing and brakes the shaft with its drag
        torque. Losses are always electrical minus shaft power.

        The torque is cut back until the electrical power fits the window the
        source-limit handshake gave this motor for the step; when not even the
        spin losses fit, the inverter shuts off. Regeneration cut this way is
        booked as not recovered (the generator power the command asked for
        minus what the bus took), so none of it disappears unreported."""
        rpm = abs(omega_m) * RPM
        req, mc.request = mc.request, None
        if req is not None and req[0] == demand and req[1] == omega_m:
            t_net, powered, p_elec = req[2:]
        else:
            t_net, powered = self.motor_command(mc, demand, omega_m)
            p_elec = self.motor_power(mc, t_net, omega_m) if powered else 0.0
        p_asked = None  # the command's electrical power, when the window cut it
        if powered:
            if not mc.p_lo_w <= p_elec <= mc.p_hi_w:
                p_asked = p_elec

                def fits(frac: float) -> bool:
                    return mc.p_lo_w <= self.motor_power(mc, frac * t_net, omega_m) <= mc.p_hi_w

                if fits(0.0):  # largest part of the command that fits
                    lo, hi = 0.0, 1.0
                    for _ in range(30):
                        mid = 0.5 * (lo + hi)
                        lo, hi = (mid, hi) if fits(mid) else (lo, mid)
                    t_net *= lo
                    p_elec = self.motor_power(mc, t_net, omega_m)
                else:
                    powered = False
                mc.limited_s += self.dt
        if not powered:  # inverter off: unpowered, drag only
            t_net = -_sign(omega_m) * mc.drag.at(rpm)
            p_elec = 0.0
        # the maps this result was read from, at the point it was read at;
        # past its maximum speed a motor counts as over speed, not as past a
        # drag table that reaches that speed
        if powered:
            self.used(mc.full_load, self.motor_volts(mc), rpm)
            self.used(mc.loss, rpm, abs(t_net))
        else:
            self.used(mc.drag, rpm if mc.drag.pts[-1][0] < mc.max_rpm else min(rpm, mc.max_rpm))
        if self.over_speed(mc, rpm, mc.max_rpm):
            self.rt.message(
                "info",
                f"E-Motor '{self.model.elements[mc.el_id].label}' was driven above its maximum "
                f"speed ({mc.max_rpm:,.0f} 1/min) at t = {self.t:.2f} s; its inverter is off "
                f"above it, so it gives no drive torque and no regeneration. The run summary "
                f"says for how long and how far.")
        if p_asked is not None and p_asked < 0:
            mc.regen_lost_wh += (p_elec - p_asked) * self.dt / 3600.0
        mc.rpm = rpm
        mc.torque = t_net
        mc.p_mech_w = t_net * omega_m
        mc.p_elec_w = p_elec
        mc.p_loss_w = p_elec - mc.p_mech_w
        return t_net

    # ---- source-limit handshake --------------------------------------------------

    def dcdc_eta(self, d_id: str) -> float:
        return max(1e-3, float(self.params(d_id).get("efficiency_pct", 97)) / 100.0)

    def battery_currents(self, b: BatteryState) -> tuple[float, float, float, float]:
        """(source voltage behind R0, maximum-power-point current, discharge
        current that reaches the minimum SOC within the step, charge current
        that reaches 100 % within it)."""
        soc_per_amp = self.dt / 3600.0 / b.q_ah  # SOC one ampere moves over the step
        a_volt = b.ocv() - b.v_rc
        return (a_volt, max(0.0, a_volt) / (2.0 * b.r0),
                max(0.0, (b.soc - b.min_soc) / soc_per_amp),
                max(0.0, (1.0 - b.soc) / (soc_per_amp * b.eta_charge)))

    def battery_full(self, b: BatteryState) -> bool:
        """Too full to take its max charge power for a whole solver step."""
        a_volt, _, _, i_full = self.battery_currents(b)
        return i_full * (a_volt + i_full * b.r0) < b.max_charge_w

    def battery_window(self, b: BatteryState) -> tuple[float, float]:
        """(deliver, absorb): the most terminal power the battery can give and
        take over the next solver step — its maximum-power point and max
        charge power, and never past its minimum SOC or 100 %."""
        a_volt, i_mpp, i_floor, i_full = self.battery_currents(b)
        i_dis = min(i_mpp, i_floor)
        return (i_dis * (a_volt - i_dis * b.r0),
                min(b.max_charge_w, i_full * (a_volt + i_full * b.r0)))

    def root_window(self, root) -> tuple[float, float]:
        """(deliver, absorb) of a source tree's own source over the step."""
        model, rt = self.model, self.rt
        if root.battery:
            return self.battery_window(self.batteries[root.battery])
        if root.vsource:
            return math.inf, math.inf
        if root.fuelcell:
            fc = self.fuelcells[root.fuelcell]
            tank = self.tanks.get(model.h2_tank) if model.h2_tank else None
            p_max = fc.pol.at(fc.i_max) * fc.i_max
            if tank is not None:
                if tank.mass_kg <= 0:
                    if not tank.empty_flagged:
                        tank.empty_flagged = True
                        rt.message("warning", "Hydrogen tank empty — fuel cell shut down.")
                    return 0.0, 0.0
                if fc.h2_g_per_kwh > 0:  # no more than the hydrogen left
                    p_max = min(p_max, tank.mass_kg * 3.6e9 / fc.h2_g_per_kwh / self.dt)
            return max(0.0, p_max), 0.0
        return 0.0, 0.0  # no source, or a DC-DC with nothing on its input

    def update_source_limits(self, t: float) -> None:
        """Source-limit handshake, first half — run every solver step before
        the driver and the mechanics. Each source tree (a bus with its
        battery, fuel cell or voltage source, plus the buses DC-DC converters
        feed from it) states what its source can deliver and absorb over the
        step. Consumers and DC-DC setpoints are served first and cut back
        when the source cannot carry them; the motors share what is left
        (allocate_motor_power)."""
        model, rt = self.model, self.rt
        for root in reversed(model.buses):  # suppliers before the buses they feed
            nodes = self.bus_tree.get(root.id)
            if nodes is None:
                continue  # fed by a DC-DC: part of its supplier's tree
            factor: dict[int, float] = {}  # W drawn at the root per W used on a bus
            own: dict[int, float] = {}  # fixed loads on each bus, W at the bus
            demand: dict[str, float] = {}
            setpoint: dict[str, float] = {}
            fixed = 0.0
            for bus, d_id, parent in nodes:
                f = 1.0 if parent is None else factor[parent.id] / self.dcdc_eta(d_id)
                factor[bus.id] = f
                load = 0.0
                for c_id in bus.consumers:
                    p_kw = rt.read_signal(c_id, "sig_demand_in")
                    if p_kw is None:
                        p_kw = float(self.params(c_id).get("power_kW", 0))
                    demand[c_id] = max(0.0, p_kw) * 1000.0
                    load += demand[c_id]
                for d in bus.dcdc_in:
                    if d in self.setpoint_dcdcs:
                        sp_kw = rt.read_signal(d, "sig_setpoint_in")
                        if sp_kw is None:
                            sp_kw = float(self.params(d).get("power_setpoint_kW", 0))
                        setpoint[d] = max(0.0, sp_kw) * 1000.0
                        load += setpoint[d] / self.dcdc_eta(d)
                own[bus.id] = load
                fixed += f * load
            deliver, absorb = self.root_window(root)
            inflow = 0.0  # setpoint converters feeding this battery (capped upstream)
            if root.battery:
                for d in root.dcdc_out:
                    if d not in self.dcdc_in_bus:
                        rt.warn_once(f"dcdc-noinput:{d}",
                                     f"DC-DC '{model.elements[d].label}' has nothing connected "
                                     f"to its input — it supplies nothing.")
                    inflow += self.dcdc_setpoint_w.get(d, 0.0)
            k = 1.0
            if fixed > deliver + inflow:
                k = (deliver + inflow) / fixed
                if not (root.battery or root.vsource or root.fuelcell):
                    rt.warn_once(f"nosrc:{root.id}",
                                 "An electrical bus has load but no source — demand is unmet.")
                else:
                    for c_id, p_w in demand.items():
                        if p_w > 0:
                            rt.warn_once(f"shed:{c_id}",
                                         f"Power consumer '{model.elements[c_id].label}' cut back "
                                         f"at t = {t:.0f} s — its source cannot supply it.")
                    if root.battery:
                        self.limit_message(root, True, t)  # say which battery limit
            for c_id, p_w in demand.items():
                self.consumer_w[c_id] = p_w * k
            for d, sp in setpoint.items():
                self.dcdc_setpoint_w[d] = sp * k
            served = fixed * k
            self.source_window[root.id] = (deliver, absorb)
            self.motor_room[root.id] = (-(absorb + served), deliver + inflow - served)
            for bus, _, parent in nodes:
                self.fixed_served_w[bus.id] = own[bus.id] * k
                # what motors here may feed back; a one-way DC-DC passes none up
                self.regen_room_w[bus.id] = absorb + served if parent is None else own[bus.id] * k

    def allocate_motor_power(self, requests: dict[str, tuple[float, float]], t: float) -> None:
        """Source-limit handshake, second half — run by the mechanics before
        any torque is applied. ``requests`` maps each motor in a driveline to
        (traction command, shaft speed). Motors that draw share their tree's
        room left after the fixed loads, in proportion to what they ask for;
        motors that recuperate feed back at most what the source absorbs plus
        the fixed loads, and behind a (one-way) DC-DC only what that bus's
        own loads use. The result is each motor's power window."""
        need: dict[str, float] = {}  # requested electrical power, W
        for m_id, mc in self.motors.items():
            mc.p_lo_w, mc.p_hi_w = -math.inf, math.inf
            req = requests.get(m_id)
            if req is None:  # not in a driveline: never stepped
                mc.request = None
                mc.torque = mc.p_mech_w = mc.p_elec_w = mc.p_loss_w = 0.0
                need[m_id] = 0.0
                continue
            t_cmd, powered = self.motor_command(mc, req[0], req[1])
            need[m_id] = self.motor_power(mc, t_cmd, req[1]) if powered else 0.0
            mc.request = (req[0], req[1], t_cmd, powered, need[m_id])
        for root_id, nodes in self.bus_tree.items():
            lo, hi = self.motor_room.get(root_id, (0.0, 0.0))
            factor: dict[int, float] = {}
            pos = neg_root = neg_fed = 0.0
            for bus, d_id, parent in nodes:
                ms = self.bus_motors[bus.id]
                if parent is None:
                    factor[bus.id] = 1.0
                    for mc in ms:
                        if need[mc.el_id] > 0:
                            pos += need[mc.el_id]
                        else:
                            neg_root += need[mc.el_id]
                    continue
                f = factor[bus.id] = factor[parent.id] / self.dcdc_eta(d_id)
                pos += f * sum(max(0.0, need[mc.el_id]) for mc in ms)
                neg = sum(min(0.0, need[mc.el_id]) for mc in ms)
                room = self.fixed_served_w.get(bus.id, 0.0)
                if neg < -room:  # behind a one-way DC-DC: feed only this bus's loads
                    for mc in ms:
                        if need[mc.el_id] < 0:
                            mc.p_lo_w = need[mc.el_id] * room / -neg
                    self.rt.warn_once(
                        f"dcdc-oneway:{d_id}",
                        f"DC-DC '{self.model.elements[d_id].label}' passes power one way only "
                        f"— regenerative torque on its output bus limited.")
                    neg = -room
                neg_fed += f * neg
            total = pos + neg_root + neg_fed
            if total > hi + 1e-9 * max(1.0, abs(hi)):
                k = max(0.0, (hi - neg_root - neg_fed) / pos)
                for bus, _, _ in nodes:
                    for mc in self.bus_motors[bus.id]:
                        if need[mc.el_id] > 0:
                            mc.p_hi_w = k * need[mc.el_id]
                self.limit_message(nodes[0][0], True, t)
            elif total < lo - 1e-9 * max(1.0, abs(lo)) and neg_root < 0:
                k = min(1.0, max(0.0, (lo - pos - neg_fed) / neg_root))
                for mc in self.bus_motors[root_id]:
                    if need[mc.el_id] < 0:
                        mc.p_lo_w = k * need[mc.el_id]
                self.limit_message(nodes[0][0], False, t)

    def limit_message(self, root, discharge: bool, t: float) -> None:
        """Say once which source limit held the motors back."""
        model, rt = self.model, self.rt
        if root.battery:
            b = self.batteries[root.battery]
            label = model.elements[b.el_id].label
            _, i_mpp, i_floor, _ = self.battery_currents(b)
            if discharge and i_floor < i_mpp:
                if not b.depleted_flagged:
                    b.depleted_flagged = True
                    rt.message("warning",
                               f"Battery '{label}' reached minimum SOC ({b.min_soc * 100:.0f} %) at "
                               f"t = {t:.0f} s — no further discharge; the motors get only what "
                               f"is left.")
            elif discharge:
                rt.warn_once(f"maxp:{b.el_id}",
                             f"Battery '{label}' demand exceeds its deliverable power — motor "
                             f"torque limited at the maximum power point.")
            elif self.battery_full(b):
                rt.warn_once(f"full:{b.el_id}",
                             f"Battery '{label}' is full — regenerative torque limited.")
            else:
                rt.warn_once(f"chg:{b.el_id}",
                             f"Charging exceeds max charge power of '{label}' — regenerative "
                             f"torque limited.")
        elif root.fuelcell:
            label = model.elements[root.fuelcell].label
            tank = self.tanks.get(model.h2_tank) if model.h2_tank else None
            if not discharge:
                rt.warn_once(f"fcback:{root.fuelcell}",
                             f"Fuel cell '{label}' cannot take power back — regenerative "
                             f"torque limited.")
            elif tank is None or tank.mass_kg > 0:
                rt.warn_once(f"fcmax:{root.fuelcell}",
                             f"Fuel cell '{label}' demand exceeds its maximum power — motor "
                             f"torque limited.")
        elif not root.vsource:
            rt.warn_once(f"nosrc:{root.id}",
                         "An electrical bus has load but no source — demand is unmet.")

    def engine_torque(self, ec: EngineCache, omega_e: float) -> float:
        """Shaft torque of the combustion engine.

        The full-load curve and fuel map are brake (net, flywheel) maps, as on
        a datasheet: fired, the engine delivers throttle × full-load torque
        and burns map(speed, torque). The drag table applies only while it is
        not fired — switched off, out of fuel, in overrun fuel cut-off (zero
        throttle above the re-entry speed) or above the full-load curve's
        last speed (rev limiter) — and then it burns nothing. At zero
        throttle below the re-entry speed the idle governor holds idle: below
        idle it adds torque, above it trims the fuel down to the drag torque,
        with fuel falling linearly from map(speed, 0) to 0 (a Willans line).
        Below the full-load curve's first speed (starting, stalling) a fired
        engine gives that point's torque and burns that point's fuel: the
        start-up rule, so the curve and the fuel map are read only between
        its first and last speed."""
        rt, model = self.rt, self.model
        rpm = abs(omega_e) * RPM
        throttle = rt.read_signal(ec.el_id, "sig_throttle_in")
        throttle = max(0.0, min(1.0, throttle if throttle is not None else 0.0))
        on_sig = rt.read_signal(ec.el_id, "sig_on_in")
        on = (on_sig is None) or (on_sig >= 0.5)  # unwired → always on
        tank = self.tanks.get(model.fuel_tank) if model.fuel_tank else None
        if on and tank is not None and tank.mass_kg <= 0:
            on = False
            if not ec.stalled_flagged:
                ec.stalled_flagged = True
                rt.message("warning",
                           f"Fuel tank empty — engine '{model.elements[ec.el_id].label}' shut off.")
        n_top = ec.full_load.pts[-1][0]
        # the start-up rule, and the rev limiter's 1e-9 1/min allowance
        n_map = min(max(rpm, ec.full_load.pts[0][0]), n_top)
        t_drag = 0.0  # read only where it is used
        t_brake = None  # stays None while the engine is not fired
        if on:
            governor = (ec.idle_rpm - rpm) / (0.25 * ec.idle_rpm)
            if rpm > n_top + 1e-9:
                rt.warn_once(
                    f"revlimit:{ec.el_id}",
                    f"Engine '{model.elements[ec.el_id].label}' reached its maximum speed "
                    f"({n_top:,.0f} 1/min, the full-load curve's last point) — "
                    f"the rev limiter cuts fuel and torque above it.",
                    level="info",
                )
            elif throttle > 0:
                t_brake = max(throttle, min(1.0, governor)) * ec.full_load.at(n_map)
            elif rpm <= ec.reentry_rpm:
                t_full = ec.full_load.at(n_map)
                t_drag = ec.drag.at(rpm)
                t_brake = max(-t_drag, min(t_full, governor * t_full))
        if t_brake is None:
            t_drag = ec.drag.at(rpm)
            t_net = -_sign(omega_e) * t_drag
            fuel = 0.0
        elif t_brake >= 0:
            t_net = t_brake
            fuel = ec.fuel_map.at(n_map, t_brake)
            self.used(ec.fuel_map, n_map, t_brake)
        else:
            t_net = t_brake  # (the idle governor read the drag: t_drag > 0)
            fuel = ec.fuel_map.at(n_map, 0.0) * (1.0 + t_brake / t_drag)
            self.used(ec.fuel_map, n_map, 0.0)
        # the drag table where it set the result (past the rev limit that
        # counts as over speed, when the table reaches it); the full-load
        # curve is read only between its first and last speed, so it is
        # never left
        if t_brake is None or t_brake < 0:
            self.used(ec.drag, rpm if ec.drag.pts[-1][0] < n_top else min(rpm, n_top))
        if self.over_speed(ec, rpm, n_top):
            rt.message(
                "info",
                f"Engine '{model.elements[ec.el_id].label}' was driven above its "
                f"maximum speed ({n_top:,.0f} 1/min, the full-load curve's last point) at "
                f"t = {self.t:.2f} s; it is not fired above it. The run summary says for how "
                f"long and how far.")
        if tank is not None and fuel > 0:
            burn = fuel / 3600.0 * self.dt
            tank.mass_kg = max(0.0, tank.mass_kg - burn)
            ec.fuel_used_kg += burn
        elif tank is None and fuel > 0:
            ec.fuel_used_kg += fuel / 3600.0 * self.dt
        ec.rpm = rpm
        ec.torque = t_net
        ec.fuel_kgh = fuel
        ec.p_mech_w = t_net * omega_e
        return t_net

    def wheel_force(self, w, omega_ref: float,
                    damping: bool = True) -> tuple[float, float, float]:
        """(tire force, its torque at the reference axis, slip damping for
        the implicit solve — 0 when not asked for)."""
        n_load = w.load_share * self.veh_mass * GRAVITY * self.slope_cos if self.veh_id else 0.0
        if n_load <= 0:
            return 0.0, 0.0, 0.0
        v = self.v
        v_den = max(abs(v), V_EPS)
        slip = (w.m * omega_ref * w.radius - v) / v_den
        mu, k_slip = w.mu, w.c_slip * slip
        force = n_load * max(-mu, min(mu, k_slip))
        if not damping or abs(k_slip) >= mu:  # saturated: no damping
            return force, -force * w.radius * w.m, 0.0
        return (force, -force * w.radius * w.m,
                n_load * w.c_slip * w.radius ** 2 * w.m ** 2 / v_den)

    def brake_capacity(self, seg: Segment) -> float:
        cap = 0.0
        for br in seg.brakes:
            cmd = self.rt.read_signal(br.el_id, "sig_demand_in") or 0.0
            cmd = max(0.0, min(1.0, cmd))
            self.rt.publish(br.el_id, "sig_torque", cmd * br.max_torque)
            cap += cmd * br.max_torque * br.m
        return cap

    def prop_torque(self, seg: Segment, omega_ref: float) -> float:
        total = 0.0
        for pr in seg.props:
            omega_p = pr.m * omega_ref
            rpm_p = abs(omega_p) * RPM
            total += -_sign(omega_p) * pr.t_ref * (rpm_p / pr.n_ref) ** 2 * pr.m
        return total

    @staticmethod
    def apply_brake(tau_other: float, omega: float, cap: float, j_over_dt: float) -> float:
        if cap <= 0:
            return 0.0
        if abs(omega) > W_EPS:
            return -_sign(omega) * cap
        return -max(-cap, min(cap, tau_other + j_over_dt * omega))

    # ---- gear selection ---------------------------------------------------------

    def check_gear_shifts(self) -> bool:
        """Re-walk every driveline whose gearbox changed gear, in the same
        solver step. Returns True when a driveline shifted (a 'reconfigured'
        event).

        A shift changes the gearbox's speed factors and the inertia and
        efficiency reflected through it, so the driveline is walked again
        from the run's live parameter set: edits made during the run (grip,
        brake torque, locks …) stay in force; edits deferred to the next
        run stay deferred. The rotating speeds carry over through the
        per-element anchors. The first check, at t = 0, only takes the gear
        the controller asks for as the starting gear: when that is the gear
        the model was built in nothing is rebuilt, otherwise the driveline
        is set up in it from the vehicle speed, as at the start of the run."""
        rt = self.rt
        first, self.gears_checked = not self.gears_checked, True
        shifted = False
        for st in self.dls:
            changed = False
            for seg in st.dl.segments:
                for gb in seg.gearboxes:
                    sig = rt.read_signal(gb.el_id, "sig_gear_in")
                    gear = round(sig) if sig is not None else float(
                        self.params(gb.el_id).get("default_gear", 1) or 1)
                    if self.gear_of.get(gb.el_id) != gear:
                        changed = True
                    self.gear_of[gb.el_id] = gear
            if not changed:
                continue
            new_dl = self.model.rewalk(st.dl, self.gear_of) if self.model.rewalk else None
            if new_dl is None:  # cannot happen for a model that built
                rt.warn_once("regear", "A driveline could not be rebuilt for a gear change "
                                       "— it keeps its previous gear.")
                continue
            st.dl = new_dl
            self.normalize_wheel_loads()  # the new wheels hold their raw shares
            self.rebuild_plan(st, initial=first)
            shifted = shifted or not first
        return shifted


class _CtxSlave(Slave):
    """Base for the wholesale-wrapped slaves: no declared variables yet —
    the shared RunContext carries all coupling until per-component models
    are extracted (then VarDefs and master-pool routing take over)."""

    def __init__(self, ctx: RunContext):
        self.ctx = ctx

    def variables(self) -> list[VarDef]:
        return []

    def setup(self, t0: float) -> None:
        pass

    def set_inputs(self, values) -> None:
        pass

    def get_outputs(self) -> dict[str, float]:
        return {}


# blocks that may run slower than the solver step (their Sample Time)
SAMPLED_BLOCKS = ("signal.script", "control.pid", "signal.lookup")


class ControlSlave(_CtxSlave):
    """Signal sources + signal blocks (Script, PID, Lookup, Road Profile),
    evaluated every solver step in the model's topological order. A Script,
    PID or Lookup whose Sample Time is longer than the solver step runs only
    at multiples of it and holds its outputs in between. Also claims live
    parameter writes for the whole context (broadcast)."""

    slave_id = "control"

    def __init__(self, ctx: RunContext):
        super().__init__(ctx)
        self.next_sample: dict[str, float] = {}  # sampled blocks' next run time
        self.lookup_at: dict[str, tuple] = {}  # Lookup → (its table, where it read it)

    def setup(self, t0: float) -> None:
        self.next_sample.clear()
        self.lookup_at.clear()

    def set_parameter(self, name: str, value: object) -> ParamResult:
        el_id, key = split_var(name)
        return self.ctx.apply_set_param(el_id, key, value)

    def do_step(self, t: float, h: float) -> StepResult:
        ctx = self.ctx
        model, rt = ctx.model, ctx.rt

        ctx.publish_sources(t)

        # -- signal blocks (topological order, every solver step) ---------------
        for el_id in model.signal_blocks:
            el = model.elements[el_id]
            kind = model.cdef_of[el_id].id
            p = ctx.params(el_id)
            dt = h  # the block's time step, passed to scripts and the PID
            if kind in SAMPLED_BLOCKS:
                ts = float(p.get("sample_time_s", 0) or 0)
                if ts > h:
                    if t < self.next_sample.get(el_id, t) - 1e-6 * h:
                        held = self.lookup_at.get(el_id)
                        if held:  # a Lookup's output holds, read where it was
                            ctx.used(*held)
                        continue  # between samples: outputs hold
                    self.next_sample[el_id] = (math.floor(t / ts + 1e-6) + 1) * ts
                    dt = ts
            if kind == "signal.script":
                inputs = {}
                for port in (el.dynamicPorts or []):
                    if port.direction == "input":
                        inputs[port.id] = rt.read_signal(el_id, port.id) or 0.0
                try:
                    outs = ctx.sandbox.run(el_id, el.label, t, dt, inputs, dict(p))
                except ScriptError as e:
                    rt.message("error", str(e))
                    return StepResult(status="error", detail=str(e))
                out_ids = {po.id for po in (el.dynamicPorts or []) if po.direction == "output"}
                for key, value in outs.items():
                    if key in out_ids:
                        rt.publish(el_id, key, value)
                    else:
                        rt.warn_once(f"script-out:{el_id}:{key}",
                                     f"Script '{el.label}' returned '{key}' which is not one "
                                     f"of its output ports — value dropped.")
            elif kind == "control.pid":
                sp = rt.read_signal(el_id, "sig_setpoint_in") or 0.0
                fb = rt.read_signal(el_id, "sig_feedback_in") or 0.0
                st_pid = ctx.pid_state[el_id]
                err = sp - fb
                kp = float(p.get("kp", 1.0))
                ki = float(p.get("ki", 0.0))
                kd = float(p.get("kd", 0.0))
                lo = float(p.get("out_min", -1.0))
                hi = float(p.get("out_max", 1.0))
                deriv = (err - st_pid["prev_err"]) / dt
                out_unsat = kp * err + ki * (st_pid["integral"] + err * dt) + kd * deriv
                if lo <= out_unsat <= hi or err * out_unsat < 0:  # anti-windup
                    st_pid["integral"] += err * dt
                out = max(lo, min(hi, kp * err + ki * st_pid["integral"] + kd * deriv))
                st_pid["prev_err"] = err
                rt.publish(el_id, "sig_out", out)
            elif kind == "signal.lookup":
                t1, t2 = ctx.lookup_cache[el_id]
                x_in = rt.read_signal(el_id, "sig_x_in") or 0.0
                if str(p.get("mode", "1D")) == "2D":
                    y_in = rt.read_signal(el_id, "sig_y_in") or 0.0
                    self.lookup_at[el_id] = (t2, x_in, y_in)
                else:
                    self.lookup_at[el_id] = (t1, x_in)
                table, *point = self.lookup_at[el_id]
                rt.publish(el_id, "sig_out", table.at(*point))
                ctx.used(table, *point)
            elif kind == "signal.road_profile":
                pts = ctx.profile_points(el_id)
                mode = str(p.get("mode", "distance"))
                if mode == "distance":
                    x_in = rt.read_signal(el_id, "sig_distance_in")
                    if x_in is None and ctx.veh_id:
                        x_in = rt.signal_values.get((ctx.veh_id, "sig_distance"), 0.0)
                    x_in = x_in or 0.0
                else:
                    x_in = t
                rt.publish(el_id, "sig_grade",
                           interp_profile(pts, x_in, bool(p.get("repeat", False))))
        return StepResult()


class GearSlave(_CtxSlave):
    """Gear-selection check every solver step (after signals, before the
    driver) — a driveline rebuild surfaces as a 'reconfigured' event."""

    slave_id = "gear"

    def do_step(self, t: float, h: float) -> StepResult:
        changed = self.ctx.check_gear_shifts()
        return StepResult(events=["reconfigured"] if changed else [])


class SourceLimitSlave(_CtxSlave):
    """Source-limit handshake, first half: before the driver and the
    mechanics, every source tree states what its source can deliver and
    absorb over this solver step and serves its fixed loads."""

    slave_id = "limits"

    def do_step(self, t: float, h: float) -> StepResult:
        self.ctx.update_source_limits(t)
        return StepResult()


class DriverSlave(_CtxSlave):
    """Speed-following PI with capability-aware recuperation blending: when
    braking, the motors recuperate what their supplies can take this step
    (up to the Recuperation Weight) and the friction brakes do the rest."""

    slave_id = "driver"

    def __init__(self, ctx: RunContext):
        super().__init__(ctx)
        self.layout_seen = -1  # ctx.layout_version the list below belongs to
        self.brakes: list[BrakeRef] = []
        self.flat_out = False  # a performance test held full throttle ...
        self.reached = False  # ... and then reached its target

    def regen_share(self, motors: list[tuple[MotorCache, float, object]], want: float) -> float:
        """The share k ≤ ``want`` of full regeneration — a traction command
        of −k to every motor — to ask for: ``want`` itself when each motor's
        bus can absorb its electrical output over this step (the handshake's
        regen room: what the source takes plus the loads it serves),
        otherwise the largest share below it that they can, found on the
        motors' own loss maps. (At low speed more command can mean less
        power fed back, so the command actually sent is what is checked.)"""
        ctx = self.ctx

        def fits(k: float) -> bool:
            fed: dict[int, float] = defaultdict(float)
            for mc, omega, bus in motors:
                torque, powered = ctx.motor_command(mc, -k, omega)
                if powered:
                    fed[bus.id] += ctx.motor_power(mc, torque, omega)
            return all(p >= -ctx.regen_room_w.get(b, 0.0) for b, p in fed.items())

        if want <= 0.0 or fits(want):
            return max(0.0, want)
        lo, hi = 0.0, want  # a command of 0 is always accepted (inverter off)
        for _ in range(20):
            mid = 0.5 * (lo + hi)
            lo, hi = (mid, hi) if fits(mid) else (lo, mid)
        return lo

    def do_step(self, t: float, h: float) -> StepResult:
        ctx = self.ctx
        model, rt, dt = ctx.model, ctx.rt, ctx.dt
        drv_id = model.driver
        if not drv_id:
            return StepResult()
        dp = ctx.params(drv_id)
        target_kmh = rt.read_signal(drv_id, "sig_target_in")
        if target_kmh is None:
            rt.warn_once("driver-no-target",
                         "Driver has no Target Speed signal — it holds 0 km/h.",
                         level="info")
            target_kmh = 0.0
        # actual-speed feedback: wired signal, else the vehicle state
        fb_kmh = rt.read_signal(drv_id, "sig_speed_in")
        if fb_kmh is None:
            fb_kmh = ctx.v * 3.6
        kp = float(dp.get("driver_kp", 0.35))
        ki = float(dp.get("driver_ki", 0.08))
        regen_w = max(0.0, min(1.0, float(dp.get("regen_weight_pct", 80)) / 100.0))
        err = target_kmh - fb_kmh
        cmd_unsat = kp * err + ki * ctx.driver_integral
        cmd = max(-1.0, min(1.0, cmd_unsat))
        if ctx.performance and not self.reached:
            if err > 0:
                # a performance test: full throttle up to the target (a PI
                # never quite reaches a step target, so no time could be taken
                # there); set before the anti-windup, so the integral does not
                # wind up
                cmd = 1.0
                self.flat_out = True
            elif self.flat_out:
                # there: the PI holds the target from now on, as in a cycle
                # (full throttle each time the car dipped below it would
                # switch between throttle and brakes, and make up the energy)
                self.reached = True
        if cmd == cmd_unsat or err * cmd_unsat < 0:
            ctx.driver_integral += err * dt

        if cmd >= 0:
            traction_cmd, brake_cmd = cmd, 0.0
        else:
            # full regeneration at the wheels: every live motor's generator
            # torque limit, reflected through its gears — regenerating, the gear
            # losses come off the torque that reaches the motor, as in the
            # mechanics (worked out only when braking: a step's hot path)
            t_motor_cap = 0.0
            regen_motors: list[tuple[MotorCache, float, object]] = []
            for st in ctx.dls:
                if st.plan.over_constrained or not st.plan.n:
                    continue
                lay = ctx.layout(st)
                if not lay.has_wheels:
                    continue
                for s_idx, src, r_eff in lay.motors:
                    mc = ctx.motors[src.el_id]
                    bus = ctx.motor_bus.get(mc.el_id)
                    if ctx.motor_volts(mc) <= 1.0:
                        continue  # no live supply: it cannot regenerate
                    omega_m = src.m * ctx.seg_speed(st, s_idx)
                    t_q4 = -ctx.motor_command(mc, -1.0, omega_m)[0]  # 0 above its maximum speed
                    t_motor_cap += t_q4 * r_eff / max(1e-3, src.eff * st.plan.eff_chain[s_idx])
                    regen_motors.append((mc, omega_m, bus))
            if self.layout_seen != ctx.layout_version:  # brakes of every driveline
                self.layout_seen = ctx.layout_version
                self.brakes = [br for st in ctx.dls for seg in st.dl.segments for br in seg.brakes]
            fr_cap = sum(br.max_torque * br.m for br in self.brakes)
            taper = max(0.0, min(1.0, ctx.v / 3.0))
            if taper >= 1.0:
                for _, _, bus in regen_motors:
                    if bus.battery and ctx.battery_full(ctx.batteries[bus.battery]):
                        rt.warn_once(f"fullbrake:{bus.battery}",
                                     f"Battery '{model.elements[bus.battery].label}' is full — "
                                     f"no recuperation while braking (t = {t:.0f} s).",
                                     level="info")
            regen_avail = t_motor_cap * taper
            d = -cmd
            t_req = d * (fr_cap + regen_w * regen_avail)
            t_rg = min(regen_w * regen_avail, t_req)
            # no more recuperation than the supplies can take this step
            # (source-limit handshake: state of charge, charge limit, one-way
            # DC-DC, fuel cell), checked on the motors' own maps so the
            # handshake never has to cut it; the friction brakes take the rest
            share = (self.regen_share(regen_motors, min(1.0, t_rg / t_motor_cap))
                     if t_motor_cap > 0 else 0.0)
            t_rg = share * t_motor_cap
            t_fr = min(fr_cap, t_req - t_rg)
            traction_cmd = -share
            brake_cmd = t_fr / max(fr_cap, 1e-6) if fr_cap > 0 else 0.0
        rt.publish(drv_id, "sig_traction_cmd", traction_cmd)
        rt.publish(drv_id, "sig_brake_cmd", brake_cmd)
        rt.publish(drv_id, "sig_accel_pedal", max(0.0, cmd))
        rt.publish(drv_id, "sig_brake_pedal", max(0.0, -cmd))
        return StepResult()


class MechanicalSlave(_CtxSlave):
    """Per-driveline Lagrangian solve (M ẋ = Q) plus the longitudinal
    vehicle integration — one slave because tire slip couples them stiffly."""

    slave_id = "mechanical"

    def setup(self, t0: float) -> None:
        # the vehicle's initial state is on the signal bus before the first step
        ctx = self.ctx
        if ctx.veh_id:
            ctx.rt.publish(ctx.veh_id, "sig_speed", ctx.v * 3.6)
            ctx.rt.publish(ctx.veh_id, "sig_distance", ctx.distance)

    def do_step(self, t: float, h: float) -> StepResult:
        ctx = self.ctx
        rt, dt = ctx.rt, ctx.dt
        if ctx.veh_id:  # the road's slope angle, for the tyres and the vehicle alike
            theta = math.atan((rt.read_signal(ctx.veh_id, "sig_grade_in") or 0.0) / 100.0)
            ctx.slope_sin, ctx.slope_cos = math.sin(theta), math.cos(theta)
        active = [st for st in ctx.dls if not st.plan.over_constrained and st.plan.n]

        # segment speeds at the start of the step, and every motor's command
        omegas = [[ctx.seg_speed(st, s) for s in range(len(st.dl.segments))] for st in active]
        requests: dict[str, tuple[float, float]] = {}
        for st, omega_seg in zip(active, omegas):
            for s_idx, seg in enumerate(st.dl.segments):
                for src in seg.sources:
                    if src.kind == "motor" and src.el_id in ctx.motors:
                        requests[src.el_id] = (rt.read_signal(src.el_id, "sig_demand_in") or 0.0,
                                               src.m * omega_seg[s_idx])
        # source-limit handshake: every motor's request against its bus first
        ctx.allocate_motor_power(requests, t)

        # mechanics ------------------------------------------------------------
        for st, omega_seg in zip(active, omegas):
            plan = st.plan
            lay = ctx.layout(st)
            n = plan.n
            m_mat = [[0.0] * n for _ in range(n)]
            q_vec = [0.0] * n
            x = plan.x
            # clutch torques at the step's start (their implicit part is
            # added below), for the gear losses of the segments they drive
            clutch_t: list[float] = []
            clutch_cap: list[float] = []
            for j, ga, gb, rel_pairs in lay.clutches:
                engage = rt.read_signal(j.el_id, "sig_engage_in")
                engage = max(0.0, min(1.0, engage if engage is not None else 1.0))
                cap_c = engage * max(0.0, float(ctx.params(j.el_id).get("max_torque_Nm", 0)))
                d_omega = (j.child_a_m * omega_seg[j.child_a]
                           - j.child_b_m * omega_seg[j.child_b])
                st.clutch_slip[j.el_id] = d_omega
                t_c = max(-cap_c, min(cap_c, cap_c / CLUTCH_BAND * d_omega)) if cap_c > 0 else 0.0
                st.clutch_torque[j.el_id] = t_c
                clutch_t.append(t_c)
                clutch_cap.append(cap_c)
            # torque-source bookkeeping for joint channels
            torque_above: dict[int, float] = defaultdict(float)  # root seg → torque at axis
            for s_idx, seg in enumerate(st.dl.segments):
                g = plan.gvec[s_idx]
                for i, k, val in lay.inertia[s_idx]:
                    m_mat[i][k] += val
                omega = omega_seg[s_idx]
                tau = 0.0
                arms = lay.clutch_arms[s_idx]
                if seg.sources or arms:
                    # the sources' and clutches' torques reach the segment's
                    # output through its gears, then the road through the
                    # splits below: each loss acts on the net torque through it
                    t_at = [0.0] * len(seg.stages)
                    for src in seg.sources:
                        omega_src = src.m * omega
                        if src.kind == "motor" and src.el_id in ctx.motors:
                            demand = rt.read_signal(src.el_id, "sig_demand_in") or 0.0
                            t_net = ctx.motor_torque(ctx.motors[src.el_id], demand, omega_src)
                        elif src.kind == "engine" and src.el_id in ctx.engines:
                            t_net = ctx.engine_torque(ctx.engines[src.el_id], omega_src)
                        else:
                            continue
                        t_at[src.region] += t_net * src.m
                    t_clutch = 0.0
                    for c_idx, arm, region in arms:
                        t_at[region] += clutch_t[c_idx] * arm
                        t_clutch += clutch_t[c_idx] * arm
                    t_out = through_gears(seg, t_at, omega)
                    eff = plan.eff_chain[s_idx]
                    # the clutches' own torques are applied with their implicit part
                    tau += (t_out * eff if t_out * omega >= 0 else t_out / eff) - t_clutch
                    torque_above[plan.root_of_seg[s_idx]] += (
                        t_out / max(1e-9, plan.scale_of_seg[s_idx]))
                for w in seg.wheels:
                    f, tq, dmp = ctx.wheel_force(w, omega)
                    tau += tq
                    ctx.last_forces[w.el_id] = f
                    if dmp > 0:
                        c = dt * dmp
                        for i, k, gi, gk in lay.pairs[s_idx]:
                            m_mat[i][k] += c * gi * gk
                tau += ctx.prop_torque(seg, omega) if seg.props else 0.0
                cap = ctx.brake_capacity(seg) if seg.brakes else 0.0
                if cap > 0:
                    # static hold only when this segment carries a coordinate
                    coord = lay.hold_coord[s_idx]
                    if coord is not None and abs(omega) <= W_EPS:
                        j_over_dt = max(m_mat[coord][coord], 1e-4) / dt
                        tau += ctx.apply_brake(tau, omega, cap, j_over_dt)
                    else:
                        tau += -_sign(omega) * cap
                for i in range(n):
                    q_vec[i] += g[i] * tau

            # clutches: smooth Coulomb coupling, implicit in Δω
            for (j, ga, gb, rel_pairs), t_c, cap_c in zip(lay.clutches, clutch_t, clutch_cap):
                if cap_c <= 0:
                    continue
                k_c = cap_c / CLUTCH_BAND
                d_omega = st.clutch_slip[j.el_id]
                for i in range(n):
                    q_vec[i] += -t_c * ga[i] + t_c * gb[i]
                if abs(k_c * d_omega) < cap_c:  # unclamped → implicit
                    c = dt * k_c
                    for i, k, ri, rk in rel_pairs:
                        m_mat[i][k] += c * ri * rk

            for i in range(n):  # symmetrize
                for k in range(i + 1, n):
                    m_mat[k][i] = m_mat[i][k]
            try:
                alpha = solve_linear(m_mat, q_vec)
            except SingularMatrixError:
                detail = (
                    f"Driveline equations became numerically singular at t = {t:g} s "
                    "(check gear ratios, inertias and joint configuration) — "
                    "solve aborted."
                )
                rt.message("error", detail)
                return StepResult(status="error", detail=detail)
            for i in range(n):
                x_new = x[i] + alpha[i] * dt
                # brake zero-crossing clamp on braked coordinates
                if lay.braked[i] and x[i] * x_new < 0:
                    x_new = 0.0
                x[i] = x_new

            # update anchors + joint channels
            omega_seg = st.omega_end = [ctx.seg_speed(st, s) for s in range(len(st.dl.segments))]
            st.chain_power_w = 0.0
            for s_idx, seg in enumerate(st.dl.segments):
                for w in seg.wheels:
                    ctx.el_axis_speed[w.el_id] = w.m * omega_seg[s_idx]
                for src in seg.sources:
                    ctx.el_axis_speed[src.el_id] = src.m * omega_seg[s_idx]
                    cache = ctx.motors.get(src.el_id) or ctx.engines.get(src.el_id)
                    if cache is not None:
                        st.chain_power_w += getattr(cache, "p_mech_w", 0.0)
                for pr in seg.props:
                    ctx.el_axis_speed[pr.el_id] = pr.m * omega_seg[s_idx]
            for j in st.dl.joints:
                if j.kind != "split":
                    continue
                omega_in = (j.parent_m * omega_seg[j.parent_seg]
                            if j.parent_seg >= 0 else 0.0)
                st.joint_speed_in[j.el_id] = omega_in
                t_cross = (torque_above.get(plan.root_of_seg[j.parent_seg], 0.0)
                           * plan.scale_of_seg[j.parent_seg] / max(1e-9, j.parent_m)
                           if j.parent_seg >= 0 else 0.0)
                locked_now = bool(ctx.params(j.el_id).get("locked", j.locked))
                t_out = t_cross * j.ratio * j.eff
                if not locked_now:
                    st.joint_torque_a[j.el_id] = (1.0 - j.f_b) * t_out
                    st.joint_torque_b[j.el_id] = j.f_b * t_out
                else:
                    # emergent split: each side consumes inertia + local loads
                    for side, child, child_m in (("a", j.child_a, j.child_a_m),
                                                 ("b", j.child_b, j.child_b_m)):
                        seg = st.dl.segments[child]
                        g = plan.gvec[child]
                        a_axis = sum(g[i] * alpha[i] for i in range(n))
                        local = sum(ctx.wheel_force(w, omega_seg[child])[1] for w in seg.wheels)
                        val = max(1e-4, seg.inertia) * a_axis - local
                        if side == "a":
                            st.joint_torque_a[j.el_id] = val
                        else:
                            st.joint_torque_b[j.el_id] = val

        # vehicle --------------------------------------------------------------
        if ctx.veh_id:
            vp = ctx.params(ctx.veh_id)
            rho = AIR_DENSITY
            if ctx.amb_id:  # read every step: live edits, case values and sweeps apply
                # (Data Checks refuse air at or below absolute zero or 0 kPa
                # and warn outside the usual air, but see only the block's own
                # values: a case value, sweep or live edit must not crash and
                # is warned about here)
                ap = ctx.params(ctx.amb_id)
                t_c = float(ap.get("temperature_C", 20))
                p_kpa = float(ap.get("pressure_kPa", 101.325))
                rho = air_density(max(-273.0, t_c), max(0.0, p_kpa))
                if not (AMBIENT_C[0] <= t_c <= AMBIENT_C[1]
                        and AMBIENT_KPA[0] <= p_kpa <= AMBIENT_KPA[1]):
                    rt.warn_once(
                        f"ambient:{ctx.amb_id}",
                        f"'{ctx.model.elements[ctx.amb_id].label}' is at {t_c:g} °C and "
                        f"{p_kpa:g} kPa at t = {t:.2f} s, outside the usual {AMBIENT_C[0]:g} to "
                        f"{AMBIENT_C[1]:g} °C and {AMBIENT_KPA[0]:g} to {AMBIENT_KPA[1]:g} kPa "
                        f"(1 bar = 100 kPa), which gives the Vehicle's drag an air density "
                        f"of {rho:.3g} kg/m³ — check the value and its unit.")
            f_tire = 0.0
            f_roll = 0.0
            for st in active:  # at the wheel speeds just integrated
                for s_idx, seg in enumerate(st.dl.segments):
                    for w in seg.wheels:
                        f_tire += ctx.wheel_force(w, st.omega_end[s_idx], damping=False)[0]
                        n_load = w.load_share * ctx.veh_mass * GRAVITY * ctx.slope_cos
                        f_roll += w.c_rr * n_load
            if vp.get("road_load_mode") == ROAD_LOAD_ABC:
                # a coast-down's A + B·v + C·v² in km/h, as test labs publish
                # them; A and B stand in for the wheels' rolling resistance, and
                # C, measured in air of AIR_DENSITY, follows the air density
                v_kmh = ctx.v * 3.6
                f_roll = (float(vp.get("road_load_a_N", 0))
                          + float(vp.get("road_load_b_N_per_kmh", 0)) * v_kmh) * ctx.slope_cos
                f_aero = float(vp.get("road_load_c_N_per_kmh2", 0)) * v_kmh * v_kmh * rho / AIR_DENSITY
            else:
                cda = max(0.0, float(vp.get("cd", 0.28))) * max(0.0, float(vp.get("frontal_area_m2", 2.2)))
                f_aero = 0.5 * rho * cda * ctx.v * ctx.v
            f_grade = ctx.veh_mass * GRAVITY * ctx.slope_sin
            roll_taper = max(0.0, min(1.0, ctx.v / 0.3))
            accel = (f_tire - f_aero - f_roll * roll_taper - f_grade) / ctx.veh_mass
            ctx.v = max(0.0, ctx.v + accel * dt)
            ctx.distance += ctx.v * dt
            rt.publish(ctx.veh_id, "sig_speed", ctx.v * 3.6)
            rt.publish(ctx.veh_id, "sig_distance", ctx.distance)
        return StepResult()


class ElectricalSlave(_CtxSlave):
    """Bus power balance in dependency order: consumers, motors, DC-DC
    bridges, then the bus source (battery ECM / voltage source / fuel cell).
    The handshake keeps every load inside what its source can do; a clamp
    here is a last resort, and what it drops is the energy residual."""

    slave_id = "electrical"

    def do_step(self, t: float, h: float) -> StepResult:
        ctx = self.ctx
        model, rt, dt = ctx.model, ctx.rt, ctx.dt
        dcdc_draw: dict[str, float] = {}
        for bus in model.buses:
            load_w = gross_w = 0.0
            for c_id in bus.consumers:
                p_w = ctx.consumer_w.get(c_id, 0.0)
                rt.publish(c_id, "sig_power", p_w / 1000.0)
                load_w += p_w
                gross_w += p_w
            for m_id in bus.motors:
                p_w = ctx.motors[m_id].p_elec_w if m_id in ctx.motors else 0.0
                load_w += p_w
                gross_w += abs(p_w)
            for d_id in bus.dcdc_in:
                load_w += dcdc_draw.get(d_id, 0.0)
                gross_w += dcdc_draw.get(d_id, 0.0)
            # DC-DC feeding a battery bus works at its power setpoint (as far as
            # its supply allows) and throttles back when the battery is full
            if bus.battery:
                sps = [(d_id, ctx.dcdc_setpoint_w.get(d_id, 0.0)) for d_id in bus.dcdc_out]
                sp_total = sum(sp for _, sp in sps)
                absorb = ctx.source_window.get(bus.id, (0.0, math.inf))[1]
                inflow = max(0.0, min(sp_total, load_w + absorb))
                for d_id, sp in sps:
                    out = sp * inflow / sp_total if sp_total > 0 else 0.0
                    dcdc_draw[d_id] = out / ctx.dcdc_eta(d_id)
                    ctx.dcdc_flows[d_id] = (dcdc_draw[d_id], out)
                load_w -= inflow
                gross_w += inflow
            ctx.bus_loads[bus.id] = load_w
            for n_id in bus.nodes:
                rt.publish(n_id, "sig_power", load_w / 1000.0)

            residual_w = 0.0  # load no source covered (+) or absorbed (−)
            if bus.battery:
                b = ctx.batteries[bus.battery]
                deliver, absorb = ctx.source_window.get(bus.id, (math.inf, math.inf))
                p_w = max(-absorb, min(deliver, load_w))
                residual_w = load_w - p_w
                ocv = b.ocv()
                a_volt = ocv - b.v_rc
                ctx.used(b.ocv_map, b.soc_pct())
                disc = max(0.0, a_volt * a_volt - 4.0 * b.r0 * p_w)
                current = (a_volt - math.sqrt(disc)) / (2.0 * b.r0)
                v_term = a_volt - current * b.r0
                if b.r1 > 0 and b.tau > 0:
                    b.v_rc = (b.v_rc + dt * current * b.r1 / b.tau) / (1.0 + dt / b.tau)
                # the SOC counts charge (A·h); only a share of the charging
                # current is stored
                eta = b.eta_charge if current < 0 else 1.0
                soc = b.soc - eta * current * dt / (3600.0 * b.q_ah)
                b.soc = max(0.0, min(1.0, soc))
                residual_w += (b.soc - soc) * b.q_ah * 3600.0 * ocv / (eta * dt)
                if current >= 0:
                    b.energy_out_wh += p_w * dt / 3600.0
                else:
                    b.energy_in_wh += -p_w * dt / 3600.0
                b.loss_wh += current * current * b.r0 * dt / 3600.0
                if current < 0:  # charge not stored
                    b.loss_wh += (1.0 - eta) * ocv * -current * dt / 3600.0
                b.current, b.power_w, b.v_term = current, p_w, v_term
                ctx.bus_voltage[bus.id] = v_term
            elif bus.vsource:
                vs_p = ctx.params(bus.vsource)
                ctx.bus_voltage[bus.id] = float(vs_p.get("voltage_V", 400))
                ctx.vsource_energy_wh[bus.vsource] += load_w * dt / 3600.0
                rt.publish(bus.vsource, "sig_power", load_w / 1000.0)
                rt.publish(bus.vsource, "sig_voltage", ctx.bus_voltage[bus.id])
            elif bus.fuelcell:
                fc = ctx.fuelcells[bus.fuelcell]
                tank = ctx.tanks.get(model.h2_tank) if model.h2_tank else None
                deliver = ctx.source_window.get(bus.id, (0.0, 0.0))[0]
                p_req = max(0.0, min(deliver, load_w))
                residual_w = load_w - p_req
                lo_i, hi_i = 0.0, fc.i_max
                # (read unchecked: the handshake read the curve at i_max)
                for _ in range(40):
                    mid = 0.5 * (lo_i + hi_i)
                    if interp1(fc.pol.pts, mid, fc.pol.linear[0]) * mid < p_req:
                        lo_i = mid
                    else:
                        hi_i = mid
                current = 0.5 * (lo_i + hi_i)
                volt = fc.pol.at(current)
                ctx.used(fc.pol, current)
                fc.current, fc.voltage, fc.power_w = current, volt, p_req
                fc.h2_kgh = p_req / 1000.0 * fc.h2_g_per_kwh / 1000.0
                fc.energy_wh += p_req * dt / 3600.0
                if tank is not None:
                    tank.mass_kg = max(0.0, tank.mass_kg - fc.h2_kgh / 3600.0 * dt)
                ctx.bus_voltage[bus.id] = volt
            elif bus.dcdc_out and bus.id not in ctx.bus_tree:  # fed through a DC-DC
                d_id = bus.dcdc_out[0]
                out = max(0.0, load_w)  # one way: it cannot take power back
                residual_w = load_w - out
                dcdc_draw[d_id] = out / ctx.dcdc_eta(d_id)
                ctx.dcdc_flows[d_id] = (dcdc_draw[d_id], out)
                ctx.bus_voltage[bus.id] = float(ctx.params(d_id).get("output_voltage_V", 400))
            else:
                if load_w > 1.0:
                    rt.warn_once(f"nosrc:{bus.id}",
                                 "An electrical bus has load but no source — demand is unmet.")
                residual_w = load_w
                ctx.bus_voltage[bus.id] = 0.0
            if abs(residual_w) > 1e-6 * max(1.0, gross_w):
                rt.warn_once(f"residual:{bus.id}",
                             f"Energy balance broken at t = {t:g} s: {residual_w / 1000.0:.2f} kW "
                             f"on an electrical bus was not covered by its source — a last-resort "
                             f"clamp fired, so energy results are not reliable.")
            ctx.residual_wh += abs(residual_w) * dt / 3600.0
            ctx.throughput_wh += gross_w * dt / 3600.0
        return StepResult()


def build_slaves(ctx: RunContext) -> list[Slave]:
    """The wholesale-wrapped slave set in the canonical solve order."""
    return [
        ControlSlave(ctx),
        GearSlave(ctx),
        SourceLimitSlave(ctx),
        DriverSlave(ctx),
        MechanicalSlave(ctx),
        ElectricalSlave(ctx),
    ]
