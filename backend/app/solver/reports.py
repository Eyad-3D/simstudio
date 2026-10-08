"""Run reports: where the energy went (RES-22), each part's duty (RES-39)
and what held the car back at every moment (RES-38).

The energy report is made from the run's energy book (MOD-10, energy.py),
in which every part keeps its own books every solver step: what went in,
what came out, what it lost and the change of what it stores. Its table is
those books, one row per part (each battery, tank, engine, fuel cell,
E-Motor, DC-DC converter, consumer, clutch, gear, differential, brake,
propeller, wheel, each driveline's spinning parts and the Vehicle); its
Sankey chart takes the sources from the batteries, tanks and voltage
sources and the sinks from the parts' losses and the Vehicle's road load,
height and speed. What is left, *not accounted for*, is only where the
books together do not close (the flow out of one part that is not quite
the flow into the next: the solver's step), the summary's *Energy balance
residual*. A lap case's gears and friction brakes are booked by the lap's
own energy pass, as the Vehicle's terms.

A :class:`RunRecorder` looks at the run's state after every fourth solver
step for the duty and the limit band; it changes nothing in the run.
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
from .energy import ORDER
from .maps import interp1
from .runtime import SPEED_LIMIT_BAND, tyre_mu

J_TO_WH = 1.0 / 3600.0

# What held a driveline back in a solver step, in the order they are
# checked: the first that applies names the step.
LIMIT_STATES = ["braking", "grip", "set_limit", "supply", "machine", "coasting", "demand"]
# The duty and the limit band sample every fourth solver step (40 ms at the
# usual 10 ms), which keeps them near 2 % of a run's time (the CI's budget is
# 10 %); their totals still cover the whole run. (The energy book is kept
# every solver step by the parts themselves, MOD-10.)
SAMPLE_EVERY = 4
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
    at the end. The books are kept in plain lists, so a step costs little
    next to the physics. The energy report is made at the end, from the
    run's energy book (``energy`` False: none)."""

    def __init__(self, ctx, energy: bool = True):
        self.ctx = ctx
        self.energy_on = energy
        model = ctx.model
        self.lap = False
        self.t = 0.0  # time booked
        self.pending = 0.0  # time solved since
        self.n_steps = 0
        self.label = {el_id: el.label for el_id, el in model.elements.items()}
        self.kind = {el_id: cdef.id for el_id, cdef in model.cdef_of.items()}
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
        self.traction_key = (model.driver, "sig_traction_cmd") if model.driver else None

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
            return
        self.book()

    def book(self) -> None:
        dt, self.pending = self.pending, 0.0
        self.t += dt
        self.book_duty(dt)
        if self.lanes:
            self.book_limits(dt)

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
        # the driver brakes: the brake pedal, or a negative traction demand
        driver_braking = bool(
            (self.brake_key and (values.get(self.brake_key) or 0.0) > 1e-6)
            or (self.traction_key and (values.get(self.traction_key) or 0.0) < -1e-6))
        forces = ctx.last_forces
        limited_prev = self.limited_prev
        for lane in self.lanes:
            code = 6  # the driver's demand met
            asked = False
            full = False
            held = 0
            braking = driver_braking
            # a motor asked for negative torque while an engine on this
            # driveline pulls is charging the battery (a hybrid's strategy),
            # not braking; with no engine pulling it is regenerative braking
            engine_pulls = any(ec.torque > 0 for ec in lane.engines)
            for mc, key, bat in lane.motors:
                d = (values.get(key) or 0.0) if key is not None else 0.0
                if d < 0:
                    if not engine_pulls:
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
        """The three reports. Run once the energy book is closed (and a lap
        case's mechanics added to it), so the energy report has the whole
        run."""
        if self.pending > 0:
            self.book()
        ctx = self.ctx
        energy = None
        if self.energy_on:
            energy = book_report(ctx.book.flows.values(),
                                 fuel_tank=bool(ctx.model.fuel_tank),
                                 h2_tank=bool(ctx.model.h2_tank),
                                 residual_wh=ctx.residual_wh, throughput_wh=ctx.throughput_wh)
        return energy, self.duty_parts(), self.limit_report()

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
                rows = [DutyRow(quantity=q, unit=u, max=s[2], min=s[3],
                                mean=s[0] / t, rms=math.sqrt(max(0.0, s[1] / t)))
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


# what a part's loss is called in the Sankey chart, and its group
_LOSS_NAME = {
    "motor.emotor": "{} — losses", "engine.combustion": "{} — losses",
    "fuelcell.stack": "{} — losses", "controller.dcdc": "{} — losses",
    "mech.clutch": "{} — slip", "propulsion.wheel": "{} — tyre slip",
}
_LOAD_PARTS = ("electric.constant_drive", "electric.climate")
_SOURCE_TANKS = {"fuel.tank": "Fuel", "fuel.h2_tank": "Hydrogen"}
# the Vehicle's terms (energy.ROAD_TERMS, and a lap case's, energy.add_lap):
# (its name in the chart, its group)
_VEHICLE_TERMS = {
    "air drag": ("Air drag", "road"),
    "rolling resistance": ("Rolling resistance", "road"),
    "road load": ("Air drag and rolling resistance", "road"),
    "friction brakes": ("Friction brakes", "brakes"),
    "gears": ("Gears and spinning parts", "losses"),
    "climbing": ("Climbing (height gained)", "stored"),
    "acceleration": ("Speed at the end (kinetic energy)", "stored"),
}


def _rank(f) -> int:
    return ORDER.index(f.part) if f.part in ORDER else len(ORDER)


def book_report(flows, fuel_tank: bool = True, h2_tank: bool = True,
                residual_wh: float = 0.0, throughput_wh: float = 0.0) -> EnergyReport:
    """The energy report of a run's energy book (energy.Flow, J): one table
    row per part as it booked itself, and the Sankey chart's sources and
    sinks.

    The sources are the batteries (what their cells gave: what came out at
    the terminals and what they lost; what was charged back is a sink of its
    own), the fuel and hydrogen tanks (an engine's fuel or a fuel cell's
    hydrogen when the model has no tank) and the voltage sources. The sinks
    are every part's loss (a consumer's use, a brake's heat, a tyre's slip,
    a gear's or a motor's loss), the Vehicle's air drag, rolling resistance,
    height and speed, and the spinning parts' speed. With every part's
    books closed, the sources less the sinks is where the parts' books
    together do not close: the flows the solver's step lost or made where
    one part meets the next, the summary's Energy balance residual."""
    flows = sorted(flows, key=_rank)
    parts: list[PartEnergy] = []
    sources: list[tuple[str, float, Optional[str]]] = []  # (label, Wh, element)
    sinks: list[tuple[str, float, str, Optional[str]]] = []  # (label, Wh, group, element)
    for f in flows:
        k, el = f.part, f.el_id
        p = PartEnergy(label=f.label, kind=k, element_id=el, in_wh=f.in_j * J_TO_WH,
                       out_wh=f.out_j * J_TO_WH, lost_wh=f.loss_j * J_TO_WH,
                       stored_wh=f.stored_j * J_TO_WH)
        parts.append(p)
        if k == "battery.generic":
            sources.append((p.label, p.out_wh + p.lost_wh, el))
            sinks.append((f"{p.label} — internal losses", p.lost_wh, "losses", el))
            sinks.append((f"{p.label} — charged back", p.in_wh, "recovered", el))
        elif k in _SOURCE_TANKS:
            sources.append((f"{_SOURCE_TANKS[k]} ({p.label})", p.out_wh - p.in_wh, el))
        elif k == "electric.voltage_source":
            sources.append((p.label, p.out_wh - p.in_wh, el))
        elif k == "vehicle.body" and f.terms:
            for term, joules in f.terms.items():
                name, group = _VEHICLE_TERMS.get(term, (f"{p.label} — {term}", "losses"))
                sinks.append((name, joules * J_TO_WH, group, el))
        elif k == "driveline.inertia":  # its spinning parts' speed, no one part's
            sinks.append((f"{p.label}: speed at the end", p.stored_wh, "stored", el))
        else:
            if k == "engine.combustion" and not fuel_tank:
                sources.append((f"Fuel ({p.label})", f.fuel_j * J_TO_WH, el))
            elif k == "fuelcell.stack" and not h2_tank:
                sources.append((f"Hydrogen ({p.label})", p.in_wh, el))
            group = ("loads" if k in _LOAD_PARTS else "brakes" if k == "mech.brake"
                     else "losses")
            sinks.append((_LOSS_NAME.get(k, "{}").format(p.label), p.lost_wh, group, el))
            if p.stored_wh:
                sinks.append((f"{p.label} — stored", p.stored_wh, "stored", el))
    return energy_report(parts, sources, sinks, residual_wh, throughput_wh)


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
    # the chart leaves out flows under 1.8 J and the table parts that moved
    # less; every number shown keeps full precision (ENG-16)
    src = [f for f in src if f.kWh > 5e-7]
    snk = [f for f in snk if f.kWh > 5e-7]
    share = 100.0 / total_in if total_in > 0 else 0.0
    rows = [EnergyPart(
        elementId=p.element_id, label=p.label, kind=p.kind,
        inKWh=p.in_wh / 1000.0, outKWh=p.out_wh / 1000.0,
        lostKWh=p.lost_wh / 1000.0, storedKWh=p.stored_wh / 1000.0,
        lostPct=p.lost_wh / 1000.0 * share)
        for p in parts if max(abs(p.in_wh), abs(p.out_wh), abs(p.lost_wh), abs(p.stored_wh)) > 5e-4]
    return EnergyReport(
        parts=rows, sources=src, sinks=snk, sourceKWh=total_in,
        remainderKWh=remainder,
        remainderPct=100.0 * remainder / total_in if total_in > 0 else 0.0,
        balanceErrorPct=(100.0 * residual_wh / throughput_wh if throughput_wh > 0 else None))


def _given(label: str) -> str:
    if label.endswith(": speed at the end"):
        return label.replace(": speed at the end", ": speed at the start")
    return {"Climbing (height gained)": "Downhill (height lost)",
            "Speed at the end (kinetic energy)": "Speed at the start (kinetic energy)"}.get(
                label, f"{label} (gave energy)")
