"""Run reports gathered while a run goes: where the energy went (RES-22),
each part's duty (RES-39) and what held the car back at every moment
(RES-38).

A :class:`RunRecorder` looks at the run's state after every solver step
(the same steps the physics integrates, not the stored points) and adds up
what the parts already report: each E-Motor's electrical, shaft and lost
power, each battery's terminal energy and losses, each engine's fuel and
shaft power, the DC-DC converters' flows, the consumers, the friction
brakes, the tyres and the Vehicle's road load. It changes nothing in the
run.

The energy report is built by one function, :func:`energy_report`, from a
list of :class:`PartEnergy` rows and the Vehicle's terms. Today the rows
come from the recorder; once every part reports its own power in, out and
lost (MOD-10) the same function takes those, and the driveline row that is
now worked out as a remainder becomes one row per gear.
"""
from __future__ import annotations

import math
from dataclasses import dataclass, field
from typing import Optional

from ..schemas import (
    DutyPart,
    DutyRow,
    EnergyFlow,
    EnergyPart,
    EnergyReport,
    LimitLane,
    LimitReport,
)
from .maps import interp1
from .runtime import GRAVITY, RPM, SPEED_LIMIT_BAND, tyre_mu

WH = 1.0 / 3600.0  # W·s → Wh
# Lower heating values, kWh/kg (background knowledge, unverified): petrol
# about 43 MJ/kg; hydrogen 33.3 kWh/kg, as the fuel cell's help says. A
# Fuel Tank with a 'lhv_MJ_per_kg' value uses that instead.
LHV_FUEL_KWH_PER_KG = 43.0 / 3.6
LHV_H2_KWH_PER_KG = 33.3

# What held a driveline back in a solver step, in the order they are
# checked: the first that applies names the step.
LIMIT_STATES = ["braking", "grip", "set_limit", "supply", "machine", "coasting", "demand"]
# The books sample every second solver step (20 ms at the usual 10 ms),
# which keeps them under 5 % of a run's time; their totals still cover the
# whole run, and the energy remainder shows what sampling misses.
SAMPLE_EVERY = 2
MAX_CHANGES = 4000  # per lane; a busier lane is drawn from bins (the times stay exact)


@dataclass
class PartEnergy:
    """One part's energy over a run, Wh: what went in, what came out, what
    it lost (or used, for a consumer), and the change of what it stores.
    in − out − lost − stored is 0 for a part that keeps its books."""
    label: str
    kind: str
    element_id: Optional[str] = None
    in_wh: float = 0.0
    out_wh: float = 0.0
    lost_wh: float = 0.0
    stored_wh: float = 0.0


@dataclass
class VehicleEnergy:
    """The Vehicle's side of the books, Wh (signed: a negative grade term is
    height lost, a negative kinetic term speed lost over the run)."""
    aero_wh: float = 0.0
    rolling_wh: float = 0.0
    road_wh: float = 0.0  # air drag and rolling together (lap mode books them so)
    grade_wh: float = 0.0
    kinetic_wh: float = 0.0
    traction_in_wh: float = 0.0  # from the tyres, driving
    traction_out_wh: float = 0.0  # to the tyres, braking or coasting


def _acc(s: list, x: float, dt: float) -> None:
    """Add a sample to time-weighted statistics [Σx·dt, Σx²·dt, max, min]."""
    s[0] += x * dt
    s[1] += x * x * dt
    if x > s[2]:
        s[2] = x
    if x < s[3]:
        s[3] = x


def _stat() -> list:
    return [0.0, 0.0, -math.inf, math.inf]


@dataclass
class _Lane:
    label: str
    element_ids: list[str]
    motors: list  # (MotorCache, demand route key, battery or None)
    engines: list  # EngineCache
    wheels: list  # WheelRef
    seconds: list[float] = field(default_factory=lambda: [0.0] * len(LIMIT_STATES))
    changes: list[tuple[float, int]] = field(default_factory=list)
    state: int = -1


class RunRecorder:
    """Watches a run's solver steps; :meth:`step` after each, :meth:`finish`
    at the end. The books are kept in plain lists of W·s, so a step costs
    little next to the physics."""

    def __init__(self, ctx, energy: bool = True):
        self.ctx = ctx
        self.energy_on = energy
        model = ctx.model
        self.lap = False
        self.v_prev = ctx.v
        self.t = 0.0  # time booked
        self.pending = 0.0  # time solved since
        self.v_step = ctx.v  # the car's speed at the start of the last solver step
        self.n_steps = 0
        self.label = {el_id: el.label for el_id, el in model.elements.items()}
        self.kind = {el_id: cdef.id for el_id, cdef in model.cdef_of.items()}
        # energy, W·s: motors [electrical in, electrical out, shaft in, shaft
        # out]; engines [shaft in, shaft out]; DC-DC [in, out]; consumers,
        # brakes, propellers [in]; wheels, net [from the shaft, to the car]
        self.e_motors = [(mc, [0.0] * 4) for mc in ctx.motors.values()]
        self.e_engines = [(ec, [0.0] * 2) for ec in ctx.engines.values()]
        self.e_dcdc: dict[str, list[float]] = {}
        self.e_consumers: dict[str, list[float]] = {}
        self.e_brakes = []  # (driveline state, segment index, BrakeRef, its torque key, [in])
        self.e_props = []  # (driveline state, segment index, PropRef, [in])
        for st in ctx.dls:
            for s_idx, seg in enumerate(st.dl.segments):
                self.e_brakes += [(st, s_idx, br, (br.el_id, "sig_torque"), [0.0]) for br in seg.brakes]
                self.e_props += [(st, s_idx, pr, [0.0]) for pr in seg.props]
        self.e_wheels: dict[str, list[float]] = {}
        self.wheel_books: Optional[list] = None
        self.vehicle = VehicleEnergy()  # (W·s while the run goes)
        self.parts: dict[str, PartEnergy] = {}
        # duty: per part its quantities' statistics, and the time
        self.d_motors = [(mc, [_stat() for _ in range(5)],
                          getattr(ctx.motor_bus.get(mc.el_id), "id", None)) for mc in ctx.motors.values()]
        self.d_batteries = [(b, [_stat() for _ in range(3)]) for b in ctx.batteries.values()]
        self.d_engines = [(ec, [_stat() for _ in range(3)]) for ec in ctx.engines.values()]
        self.d_fuelcells = [(fc, [_stat() for _ in range(2)]) for fc in ctx.fuelcells.values()]
        self.d_dcdc: dict[str, list] = {}
        self.dcdc_volts = {d: float(ctx.params(d).get("output_voltage_V", 400) or 0)
                           for d, cdef in model.cdef_of.items() if cdef.id == "controller.dcdc"}
        # what holds each driveline back
        self.lanes: list[_Lane] = []
        bus_root: dict[int, object] = {}
        for nodes in ctx.bus_tree.values():
            for node, _, _ in nodes:
                bus_root[node.id] = nodes[0][0]
        for st in ctx.dls:
            motors, engines, wheels, ids = [], [], [], []
            for seg in st.dl.segments:
                wheels.extend(seg.wheels)
                for src in seg.sources:
                    if src.kind == "motor" and src.el_id in ctx.motors:
                        bus = ctx.motor_bus.get(src.el_id)
                        root = bus_root.get(bus.id, bus) if bus is not None else None
                        bat = ctx.batteries.get(root.battery) if root is not None and root.battery else None
                        key = model.signal_route.get((src.el_id, "sig_demand_in"))
                        motors.append((ctx.motors[src.el_id], key, bat))
                        ids.append(src.el_id)
                    elif src.kind == "engine" and src.el_id in ctx.engines:
                        engines.append(ctx.engines[src.el_id])
                        ids.append(src.el_id)
            if ids:  # (a driveline nothing drives has nothing to hold it back)
                self.lanes.append(_Lane(label=", ".join(self.label[i] for i in ids), element_ids=ids,
                                        motors=motors, engines=engines, wheels=wheels))
        self.limited_prev = {mc.el_id: mc.limited_s for mc in ctx.motors.values()}
        self.brake_key = (model.driver, "sig_brake_cmd") if model.driver else None

    # ---- every solver step ----------------------------------------------------------

    def step(self) -> None:
        """Book the solver step that just ended (ctx.dt long): every
        SAMPLE_EVERY-th one, for the time since the last (a lap's stretches
        each)."""
        dt = self.ctx.dt
        if dt <= 0:
            return
        self.pending += dt
        self.n_steps += 1
        if self.n_steps % SAMPLE_EVERY and not self.lap:
            self.v_step = self.ctx.v  # (the next step's start speed)
            return
        self.book()
        self.v_step = self.ctx.v

    def book(self) -> None:
        dt, self.pending = self.pending, 0.0
        self.t += dt
        if self.energy_on:
            self.book_energy(dt)
        self.book_duty(dt)
        if self.lanes:
            self.book_limits(dt)

    def book_energy(self, dt: float) -> None:
        ctx = self.ctx
        for mc, a in self.e_motors:
            pe, pm = mc.p_elec_w, mc.p_mech_w
            if pe > 0:
                a[0] += pe * dt
            else:
                a[1] -= pe * dt
            if pm > 0:
                a[3] += pm * dt
            else:
                a[2] -= pm * dt
        for ec, a in self.e_engines:
            pm = ec.p_mech_w
            if pm > 0:
                a[1] += pm * dt
            else:
                a[0] -= pm * dt  # dragged round, unfired
        if ctx.dcdc_flows:
            for d_id, (p_in, p_out) in ctx.dcdc_flows.items():
                a = self.e_dcdc.get(d_id) or self.e_dcdc.setdefault(d_id, [0.0, 0.0])
                a[0] += p_in * dt
                a[1] += p_out * dt
        if ctx.consumer_w:
            for c_id, p_w in ctx.consumer_w.items():
                a = self.e_consumers.get(c_id) or self.e_consumers.setdefault(c_id, [0.0])
                a[0] += p_w * dt
        if self.lap:
            return  # the lap solver books the driveline and the Vehicle (finish)
        bus = ctx.rt.signal_values
        for st, s_idx, br, key, a in self.e_brakes:  # at the speed the step ended on
            torque = bus.get(key)
            if torque:
                a[0] += abs(torque * br.m * ctx.seg_speed(st, s_idx)) * dt
        for st, s_idx, pr, a in self.e_props:
            omega_p = pr.m * ctx.seg_speed(st, s_idx)
            a[0] += abs(pr.t_ref * (abs(omega_p) * RPM / pr.n_ref) ** 2 * omega_p) * dt
        if not ctx.veh_id:
            return
        v0 = self.v_prev
        v1 = self.v_prev = ctx.v
        v_avg = 0.5 * (v0 + v1)
        # each tyre's force as the Vehicle took it: at the speeds the step
        # ended on and the car's speed at its start (the driveline's slip is
        # implicit, so its force at the step's start is not what acted)
        if self.wheel_books is None:  # (the wheels are known once the run has started)
            self.wheel_books = [(st, s_idx, w, self.e_wheels.setdefault(w.el_id, [0.0, 0.0]))
                                for st in ctx.dls for s_idx, seg in enumerate(st.dl.segments)
                                for w in seg.wheels]
        f_roll = 0.0
        ctx.v = self.v_step
        try:
            for st, s_idx, w, a in self.wheel_books:
                f_roll += w.c_rr * w.n_load
                if st.plan.over_constrained or not st.plan.n:
                    continue
                omega = st.omega_end[s_idx]
                f = ctx.wheel_force(w, omega, damping=False)[0]
                if f:  # net energy from its shaft, and on to the car
                    a[0] += f * w.radius * w.m * omega * dt
                    a[1] += f * v_avg * dt
        finally:
            ctx.v = v1
        if v0 == 0.0 and v1 == 0.0:
            return
        # the Vehicle's own equation (MechanicalSlave): road load at the
        # step's start speed, the forces over the mean speed, so their work
        # adds up to the change in kinetic energy exactly
        f_aero, f_roll = ctx.road_load(v0, f_roll, ctx.slope_cos)
        if v0 < 0.3:
            f_roll *= max(0.0, v0 / 0.3)
        f_grade = ctx.veh_mass * GRAVITY * ctx.slope_sin
        ve = self.vehicle
        ve.aero_wh += f_aero * v_avg * dt
        ve.rolling_wh += f_roll * v_avg * dt
        ve.grade_wh += f_grade * v_avg * dt
        d_ke = 0.5 * ctx.veh_mass * (v1 * v1 - v0 * v0)
        ve.kinetic_wh += d_ke
        p_car = d_ke + (f_aero + f_roll + f_grade) * v_avg * dt  # what the tyres gave the car
        if p_car > 0:
            ve.traction_in_wh += p_car
        else:
            ve.traction_out_wh -= p_car

    def book_duty(self, dt: float) -> None:
        ctx = self.ctx
        volts_of = ctx.bus_voltage
        for mc, s, bus in self.d_motors:
            pe = mc.p_elec_w
            volts = volts_of.get(bus, 0.0)
            for st, x in zip(s, (mc.p_mech_w * 1e-3, pe * 1e-3, mc.torque,
                                 pe / volts if volts > 1.0 else 0.0, mc.p_loss_w * 1e-3)):
                st[0] += x * dt
                st[1] += x * x * dt
                if x > st[2]:
                    st[2] = x
                if x < st[3]:
                    st[3] = x
        for b, s in self.d_batteries:
            i = b.current
            for st, x in zip(s, (b.power_w * 1e-3, i, i * i * b.r0 * 1e-3)):
                st[0] += x * dt
                st[1] += x * x * dt
                if x > st[2]:
                    st[2] = x
                if x < st[3]:
                    st[3] = x
        for ec, s in self.d_engines:
            _acc(s[0], ec.p_mech_w * 1e-3, dt)
            _acc(s[1], ec.torque, dt)
            _acc(s[2], ec.fuel_kgh, dt)
        for fc, s in self.d_fuelcells:
            _acc(s[0], fc.power_w * 1e-3, dt)
            _acc(s[1], fc.current, dt)
        if ctx.dcdc_flows:
            for d_id, (p_in, p_out) in ctx.dcdc_flows.items():
                s = self.d_dcdc.get(d_id) or self.d_dcdc.setdefault(d_id, [_stat() for _ in range(4)])
                _acc(s[0], p_in * 1e-3, dt)
                _acc(s[1], p_out * 1e-3, dt)
                _acc(s[2], (p_in - p_out) * 1e-3, dt)
                volts = self.dcdc_volts.get(d_id, 0.0)
                _acc(s[3], p_out / volts if volts > 1.0 else 0.0, dt)

    def book_limits(self, dt: float) -> None:
        ctx = self.ctx
        values = ctx.rt.signal_values
        braking = bool(self.brake_key and (values.get(self.brake_key) or 0.0) > 1e-6)
        forces = ctx.last_forces
        limited_prev = self.limited_prev
        for lane in self.lanes:
            code = 6  # the driver's demand met
            asked = False
            full = False
            held = 0
            for mc, key, bat in lane.motors:
                d = (values.get(key) or 0.0) if key is not None else 0.0
                if d < 0:
                    braking = True
                elif d > 0:
                    asked = True
                    if d >= 0.999 or mc.rpm > mc.max_rpm * (1.0 - SPEED_LIMIT_BAND):
                        full = True
                if mc.limited_s > limited_prev[mc.el_id]:
                    limited_prev[mc.el_id] = mc.limited_s
                    at_cap = (bat is not None and bat.check is not None and bat.check.enforced
                              and bat.capped and bat.power_w >= bat.p_cap_w * (1.0 - 1e-6))
                    held = 2 if at_cap else (held or 3)
            for ec in lane.engines:
                if ec.torque > 0:
                    asked = True
                    full = full or _engine_full(ec)
            if braking:
                code = 0
            elif asked:
                for w in lane.wheels:
                    n = w.n_load
                    f = forces.get(w.el_id)
                    if f and n > 0 and abs(f) >= (tyre_mu(w, n) if w.dmu_per_n else w.mu) * n:
                        code = 1
                        break
                else:
                    code = held or (4 if full else 6)
            else:
                code = 5
            lane.seconds[code] += dt
            if code != lane.state:
                lane.state = code
                lane.changes.append((round(self.t - dt, 4), code))

    # ---- at the end -------------------------------------------------------------------

    def finish(self, lap=None) -> tuple[Optional[EnergyReport], list[DutyPart], Optional[LimitReport]]:
        if self.pending > 0:
            self.book()
        return (self.energy(lap) if self.energy_on else None, self.duty_parts(), self.limit_report())

    def part(self, el_id: str) -> PartEnergy:
        p = self.parts.get(el_id)
        if p is None:
            p = self.parts[el_id] = PartEnergy(label=self.label.get(el_id, el_id),
                                               kind=self.kind.get(el_id, ""), element_id=el_id)
        return p

    def collect(self) -> None:
        """The step books as parts, Wh."""
        for mc, (e_in, e_out, m_in, m_out) in self.e_motors:
            p = self.part(mc.el_id)
            p.in_wh, p.out_wh = (e_in + m_in) * WH, (e_out + m_out) * WH
            p.lost_wh = p.in_wh - p.out_wh  # electrical less shaft power, as the motor books it
        for ec, (m_in, m_out) in self.e_engines:
            p = self.part(ec.el_id)
            p.in_wh, p.out_wh = m_in * WH, m_out * WH  # (its fuel is added in energy())
        for d_id, (e_in, e_out) in self.e_dcdc.items():
            p = self.part(d_id)
            p.in_wh, p.out_wh, p.lost_wh = e_in * WH, e_out * WH, (e_in - e_out) * WH
        for el_id, (e_in,) in self.e_consumers.items():
            p = self.part(el_id)
            p.in_wh = p.lost_wh = e_in * WH
        for *_, br, _, (e_in,) in self.e_brakes:
            p = self.part(br.el_id)
            p.in_wh = p.lost_wh = e_in * WH
        for *_, pr, (e_in,) in self.e_props:
            p = self.part(pr.el_id)
            p.in_wh = p.lost_wh = e_in * WH
        for el_id, (e_in, e_out) in self.e_wheels.items():  # (net: driving less braking)
            p = self.part(el_id)
            p.in_wh, p.out_wh, p.lost_wh = e_in * WH, e_out * WH, (e_in - e_out) * WH
        ve = self.vehicle
        for k in ("aero_wh", "rolling_wh", "grade_wh", "kinetic_wh", "traction_in_wh", "traction_out_wh"):
            setattr(ve, k, getattr(ve, k) * WH)
        self.mech_sources_wh = sum(a[3] - a[2] for _, a in self.e_motors) * WH + sum(
            a[1] - a[0] for _, a in self.e_engines) * WH
        self.wheel_shaft_wh = sum(a[0] for a in self.e_wheels.values()) * WH
        self.brakes_wh = sum(a[0] for *_, a in self.e_brakes) * WH
        self.props_wh = sum(a[0] for *_, a in self.e_props) * WH

    def energy(self, lap=None) -> EnergyReport:
        ctx = self.ctx
        model = ctx.model
        self.collect()
        parts: list[PartEnergy] = []
        sources: list[tuple[str, float, Optional[str]]] = []  # (label, Wh, element)
        for b in ctx.batteries.values():
            p = PartEnergy(label=self.label[b.el_id], kind="battery.generic", element_id=b.el_id,
                           in_wh=b.energy_in_wh, out_wh=b.energy_out_wh, lost_wh=b.loss_wh)
            p.stored_wh = p.in_wh - p.out_wh - p.lost_wh
            parts.insert(0, p)
        lhv = LHV_FUEL_KWH_PER_KG
        if model.fuel_tank:
            try:
                lhv = float(ctx.params(model.fuel_tank).get("lhv_MJ_per_kg", 43.0)) / 3.6
            except (TypeError, ValueError):
                pass
        fuel_wh: dict[str, float] = {}  # per engine
        for ec in ctx.engines.values():
            p = self.part(ec.el_id)
            fuel_wh[ec.el_id] = ec.fuel_used_kg * lhv * 1000.0
            p.in_wh += fuel_wh[ec.el_id]
            p.lost_wh = p.in_wh - p.out_wh  # (with what dragging it round took)
        for fc in ctx.fuelcells.values():
            p = self.part(fc.el_id)
            h2_kg = fc.energy_wh / 1000.0 * fc.h2_g_per_kwh / 1000.0
            p.in_wh = max(fc.energy_wh, h2_kg * LHV_H2_KWH_PER_KG * 1000.0)
            p.out_wh = fc.energy_wh
            p.lost_wh = p.in_wh - p.out_wh
        for vs_id, e_wh in ctx.vsource_energy_wh.items():
            p = self.part(vs_id)
            p.out_wh, p.in_wh = max(0.0, e_wh), max(0.0, -e_wh)
            p.stored_wh = -e_wh
        parts += [p for p in self.parts.values() if p not in parts]

        ve = self.vehicle
        rest = PartEnergy(label="Gears, clutches and spinning parts", kind="driveline")
        if lap is not None:  # the lap solver's own books, J
            bk = lap.book
            ve.road_wh, ve.grade_wh, ve.kinetic_wh = bk.road * WH, bk.grade * WH, bk.kinetic * WH
            self.brakes_wh = bk.friction * WH
            rest.in_wh = rest.lost_wh = bk.gears * WH
            brakes = [b for st in ctx.dls for seg in st.dl.segments for b in seg.brakes]
            cap = sum(b.max_torque * b.m for b in brakes) or 1.0
            for b in brakes:  # shared as their torques are
                p = self.part(b.el_id)
                p.in_wh = p.lost_wh = self.brakes_wh * b.max_torque * b.m / cap
                if p not in parts:
                    parts.append(p)
        else:
            # what the shafts gave and the wheels, brakes and propellers did
            # not take: the gears' losses, clutch slip and the spinning parts'
            # change in speed, not yet measured part by part (MOD-10)
            gap = self.mech_sources_wh - self.wheel_shaft_wh - self.brakes_wh - self.props_wh
            rest.in_wh = rest.lost_wh = gap
        if ctx.veh_id:
            veh = PartEnergy(label=self.label[ctx.veh_id], kind="vehicle.body", element_id=ctx.veh_id,
                             in_wh=ve.traction_in_wh, out_wh=ve.traction_out_wh,
                             lost_wh=ve.aero_wh + ve.rolling_wh + ve.road_wh,
                             stored_wh=ve.kinetic_wh + ve.grade_wh)
            if lap is not None:
                veh.in_wh = max(0.0, veh.lost_wh + veh.stored_wh)
                veh.out_wh = max(0.0, -(veh.lost_wh + veh.stored_wh))
            parts.append(veh)
        if abs(rest.lost_wh) > 0 or ctx.dls:
            parts.append(rest)

        # sources and sinks of the whole car
        sinks: list[tuple[str, float, str, Optional[str]]] = []  # (label, Wh, group, element)
        for p in parts:
            k = p.kind
            if k == "battery.generic":
                sources.append((p.label, p.out_wh + p.lost_wh, p.element_id))
                sinks.append((f"{p.label} — internal losses", p.lost_wh, "losses", p.element_id))
                sinks.append((f"{p.label} — charged back", p.in_wh, "recovered", p.element_id))
            elif k == "engine.combustion":
                sources.append((f"Fuel ({p.label})", fuel_wh.get(p.element_id or "", 0.0), p.element_id))
                sinks.append((f"{p.label} — losses", p.lost_wh, "losses", p.element_id))
            elif k == "fuelcell.stack":
                sources.append((f"Hydrogen ({p.label})", p.in_wh, p.element_id))
                sinks.append((f"{p.label} — losses", p.lost_wh, "losses", p.element_id))
            elif k == "electric.voltage_source":
                sources.append((p.label, p.out_wh - p.in_wh, p.element_id))
            elif k == "electric.constant_drive":
                sinks.append((p.label, p.lost_wh, "loads", p.element_id))
            elif k == "vehicle.body":
                if ve.road_wh:
                    sinks.append(("Air drag and rolling resistance", ve.road_wh, "road", p.element_id))
                else:
                    sinks.append(("Air drag", ve.aero_wh, "road", p.element_id))
                    sinks.append(("Rolling resistance", ve.rolling_wh, "road", p.element_id))
                sinks.append(("Climbing (height gained)", ve.grade_wh, "stored", p.element_id))
                sinks.append(("Speed at the end (kinetic energy)", ve.kinetic_wh, "stored", p.element_id))
            elif k == "mech.brake":
                sinks.append((p.label, p.lost_wh, "brakes", p.element_id))
            else:
                group = "losses"
                sinks.append((p.label if k != "motor.emotor" else f"{p.label} — losses",
                               p.lost_wh, group, p.element_id))
        return energy_report(parts, sources, sinks, ctx.residual_wh, ctx.throughput_wh)

    def duty_parts(self) -> list[DutyPart]:
        t = self.t
        out = []
        quantities = (
            ([(mc, st) for mc, st, _ in self.d_motors], (("Shaft power", "kW"), ("Electrical power", "kW"), ("Torque", "N·m"),
                             ("DC current", "A"), ("Losses", "kW"))),
            (self.d_batteries, (("Power", "kW"), ("Current", "A"), ("Losses", "kW"))),
            (self.d_engines, (("Power", "kW"), ("Torque", "N·m"), ("Fuel rate", "kg/h"))),
            (self.d_fuelcells, (("Power", "kW"), ("Current", "A"))),
            ([(type("_", (), {"el_id": d})(), s) for d, s in self.d_dcdc.items()],
             (("Power in", "kW"), ("Power out", "kW"), ("Losses", "kW"), ("Output current", "A"))),
        )
        if t <= 0:
            return out
        for items, names in quantities:
            for obj, stats in items:
                rows = [DutyRow(quantity=q, unit=u, max=round(s[2], 4), min=round(s[3], 4),
                                mean=round(s[0] / t, 4), rms=round(math.sqrt(max(0.0, s[1] / t)), 4))
                        for (q, u), s in zip(names, stats) if s[2] > -math.inf]
                if rows:
                    el_id = obj.el_id
                    out.append(DutyPart(elementId=el_id, label=self.label.get(el_id, el_id),
                                        kind=self.kind.get(el_id, ""), rows=rows))
        return out

    def limit_report(self) -> Optional[LimitReport]:
        if not self.lanes or self.t <= 0:
            return None
        lanes = []
        for lane in self.lanes:
            changes = lane.changes
            if len(changes) > MAX_CHANGES:
                changes = _binned(changes, self.t, MAX_CHANGES)
            lanes.append(LimitLane(
                label=lane.label, elementIds=lane.element_ids,
                changes=[[t, c] for t, c in changes],
                seconds={LIMIT_STATES[i]: round(v, 4) for i, v in enumerate(lane.seconds) if v > 0}))
        return LimitReport(states=LIMIT_STATES, lanes=lanes, tEnd=round(self.t, 4))


def _engine_full(ec) -> bool:
    try:
        full = interp1(ec.full_load.pts, ec.rpm, False)
    except (IndexError, TypeError, ValueError):
        return False
    return full > 0 and ec.torque >= 0.99 * full


def _binned(changes: list[tuple[float, int]], t_end: float, n: int) -> list[tuple[float, int]]:
    """The state that held longest in each of n equal bins, as changes."""
    width = t_end / n
    out: list[tuple[float, int]] = []
    i = 0
    for b in range(n):
        lo, hi = b * width, (b + 1) * width
        held: dict[int, float] = {}
        while i + 1 < len(changes) and changes[i + 1][0] <= lo:
            i += 1
        j = i
        while j < len(changes) and changes[j][0] < hi:
            start = max(lo, changes[j][0])
            end = min(hi, changes[j + 1][0] if j + 1 < len(changes) else t_end)
            held[changes[j][1]] = held.get(changes[j][1], 0.0) + max(0.0, end - start)
            j += 1
        if held:
            code = max(held, key=held.get)
            if not out or out[-1][1] != code:
                out.append((round(lo, 4), code))
    return out


def energy_report(parts: list[PartEnergy],
                  sources: list[tuple[str, float, Optional[str]]],
                  sinks: list[tuple[str, float, str, Optional[str]]],
                  residual_wh: float = 0.0, throughput_wh: float = 0.0) -> EnergyReport:
    """The Energy tab's numbers: the per-part table, and the car's sources
    and sinks for the Sankey chart with what does not add up as its own
    band. A sink below zero (height or speed lost) gives energy, so it is
    drawn on the source side, and a source below zero on the sink side.
    Values are Wh in, kWh out."""
    src: list[EnergyFlow] = []
    snk: list[EnergyFlow] = []
    for label, wh, el in sources:
        if wh >= 0:
            src.append(EnergyFlow(label=label, kWh=wh / 1000.0, group="source", elementId=el))
        else:
            snk.append(EnergyFlow(label=label, kWh=-wh / 1000.0, group="recovered", elementId=el))
    for label, wh, group, el in sinks:
        if wh >= 0:
            snk.append(EnergyFlow(label=label, kWh=wh / 1000.0, group=group, elementId=el))
        else:
            src.append(EnergyFlow(label=_given(label), kWh=-wh / 1000.0, group="released", elementId=el))
    total_in = sum(f.kWh for f in src)
    total_out = sum(f.kWh for f in snk)
    remainder = total_in - total_out  # what the books do not explain, kWh
    src = [f for f in src if f.kWh > 5e-7]
    snk = [f for f in snk if f.kWh > 5e-7]
    for f in src + snk:
        f.kWh = round(f.kWh, 6)
    share = 100.0 / total_in if total_in > 0 else 0.0
    rows = [EnergyPart(
        elementId=p.element_id, label=p.label, kind=p.kind,
        inKWh=round(p.in_wh / 1000.0, 6), outKWh=round(p.out_wh / 1000.0, 6),
        lostKWh=round(p.lost_wh / 1000.0, 6), storedKWh=round(p.stored_wh / 1000.0, 6),
        lostPct=round(p.lost_wh / 1000.0 * share, 3))
        for p in parts if max(abs(p.in_wh), abs(p.out_wh), abs(p.lost_wh), abs(p.stored_wh)) > 5e-4]
    return EnergyReport(
        parts=rows, sources=src, sinks=snk, sourceKWh=round(total_in, 6),
        remainderKWh=round(remainder, 6),
        remainderPct=round(100.0 * remainder / total_in, 4) if total_in > 0 else 0.0,
        balanceErrorPct=(round(100.0 * residual_wh / throughput_wh, 4) if throughput_wh > 0 else None))


def _given(label: str) -> str:
    return {"Climbing (height gained)": "Downhill (height lost)",
            "Speed at the end (kinetic energy)": "Speed at the start (kinetic energy)"}.get(
                label, f"{label} (gave energy)")
