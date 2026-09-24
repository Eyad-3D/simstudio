"""Solver orchestration: simulate() drives the co-simulation master.

Each recorded step (the case timeStep) is split into n_sub solver steps of
at most MAX_SUBSTEP (semi-implicit Euler). Every solver step the master
runs the wrapped domain slaves — control (signals and blocks) → gear →
source limits → driver → mechanical + vehicle → electrical — and this
module then refreshes the state-derived signals that feed block inputs.
At the end of a recorded
step it records channels, streams progress, paces against real time and
applies live control messages, so the case timeStep sets only how often
results are stored and live edits apply, not how often controllers run.
Domain physics lives in domains.py (RunContext + slaves); shared numerics
in runtime.py.

Gear shifts rebuild the driveline plan at the solver step they happen in,
live lock/unlock toggles at recording boundaries, carrying rotational
states over via per-element anchor speeds.
"""
from __future__ import annotations

import math
import time
from itertools import chain
from typing import Callable, Iterator, Optional

from ..library import unit_groups
from ..schemas import Channel, Project, SimMessage, SimResult, SummaryValue
from .domains import ModelInitError, RunContext, build_slaves
from .maps import OutsideDataError
from .master import Master, SlaveStepError
from .network import ModelError, build_model
from .runtime import (  # noqa: F401 — re-exported for backward compatibility
    AIR_DENSITY,
    CLUTCH_BAND,
    GRAVITY,
    MAX_SUBSTEP,
    RPM,
    V_EPS,
    W_EPS,
    BatteryState,
    ControlFn,
    DrivelineState,
    DrivePlan,
    EmitFn,
    EngineCache,
    FuelCellCache,
    MotorCache,
    Runtime,
    SingularMatrixError,
    TankState,
    _sign,
    make_plan,
    solve_linear,
)
from .slave import var_name
from .verdict import CycleTrace, judge


def simulate(
    project: Project,
    case_id: str,
    emit: Optional[EmitFn] = None,
    control: Optional[ControlFn] = None,
) -> SimResult:
    case = next((c for c in project.cases if c.id == case_id), None)
    if case is None:
        return SimResult(
            caseId=case_id, status="failed", channels=[],
            messages=[SimMessage(level="error", text=f"Simulation case '{case_id}' not found.")],
        )
    gear_of: dict[str, float] = {}
    case_overrides = getattr(case, "parameterOverrides", None) or {}
    try:
        model = build_model(project, gear_of, case_overrides)
    except ModelError as e:
        return SimResult(
            caseId=case_id, status="failed", channels=[],
            messages=[SimMessage(level="error", text=t) for t in e.errors],
        )

    rt = Runtime(model, emit)
    for w in model.warnings:
        rt.message("warning", w)

    dt_rec = max(1e-4, case.timeStep)
    t_end = max(0.0, float(case.duration))
    # recorded steps; the last one is shorter when the duration is not a whole
    # number of steps (a remainder under a millionth of a step is absorbed)
    steps = max(1, math.ceil(t_end / dt_rec - 1e-6)) if t_end > 0 else 0
    n_sub = max(1, math.ceil(dt_rec / MAX_SUBSTEP - 1e-9))
    dt = dt_rec / n_sub
    output_every = max(1, int(getattr(case, "outputEvery", 1) or 1))
    pace = max(0.0, float(getattr(case, "realtimeFactor", 0.0) or 0.0))

    try:
        ctx = RunContext(project, model, rt, gear_of, case_overrides)
    except ModelInitError as e:
        return SimResult(
            caseId=case_id, status="failed", channels=[],
            messages=[SimMessage(level="error", text=t) for t in e.messages],
        )
    ctx.performance = case.kind == "performance"

    try:
        # Phase 1.4: the wholesale-wrapped slaves share all coupling through the
        # RunContext, so the master runs with an empty route table for now; the
        # declared-variable pool takes over as per-component models are extracted.
        master = Master(build_slaves(ctx), routes={})
        master.initialize(0.0)

        # Signals that feed a block input: the slaves publish their own outputs,
        # and the ones computed from states (SOC, speeds, levels …) are refreshed
        # after every solver step so controllers never read a stale value.
        # Only the wired ports are evaluated; the list is rebuilt when a gear
        # shift or a lock toggle changes the drivelines.
        routed = set(model.signal_route.values())
        routed_els = {el_id for el_id, _ in routed}
        routed_fns: list[ChannelFn] = []
        routed_layout = -1

        def publish_routed_states() -> None:
            nonlocal routed_fns, routed_layout
            if routed_layout != ctx.layout_version:
                routed_layout = ctx.layout_version
                routed_fns = [c for c in _state_channel_fns(ctx, gear_of, routed_els)
                              if (c[0], c[1]) in routed]
            for el_id, port_id, fn in routed_fns:
                value = fn()
                if value is not None:
                    rt.publish(el_id, port_id, value)

        publish_routed_states()
        trace = CycleTrace(ctx)  # target vs vehicle speed, for the run verdict
        trace.sample(0.0)

        def apply_control_msg(msg: dict) -> None:
            master.set_parameter(
                var_name(str(msg.get("elementId")), str(msg.get("key"))), msg.get("value"))

        # ---- main loop -----------------------------------------------------------
        # Point 0 is the initial state at t = 0; every later point is recorded at
        # the end time of the step that produced it, and the last step ends
        # exactly at the case duration.
        cancelled = False  # a stop was asked for
        stopped = False  # ... and cut the run short (not one that came as it ended)
        failed_at: float | None = None  # the time an error cut the run short
        solved = 0.0  # time solved (a stopped run ends before its last point)
        times: list[float] = []
        t_start_wall = time.monotonic()
        h_last = t_end - (steps - 1) * dt_rec if steps else 0.0
        short_last = abs(h_last - dt_rec) > 1e-9 * dt_rec
        t = 0.0

        for step in range(steps + 1):
            if step > 0:
                t_prev = t
                t = t_end if step == steps else step * dt_rec

                if control:
                    for msg in control():
                        if msg.get("type") == "cancel":
                            cancelled = True
                        elif msg.get("type") == "set_param":
                            apply_control_msg(msg)
                if cancelled:
                    rt.message("info", f"Simulation cancelled by user at t = {t_prev:g} s.")
                    stopped = True
                    break

                # -- solver steps ---------------------------------------------------
                n, h_sub = n_sub, dt
                if step == steps and short_last:
                    n = max(1, math.ceil(h_last / MAX_SUBSTEP - 1e-9))
                    h_sub = h_last / n
                ctx.dt = h_sub
                try:
                    for j in range(n):
                        ctx.t = t_prev + j * h_sub
                        master.step(ctx.t, h_sub)
                        solved = t_prev + (j + 1) * h_sub
                        publish_routed_states()
                        trace.sample(solved, last=step == steps and j == n - 1)
                except SlaveStepError:
                    # the failing slave already emitted its error message
                    failed_at = ctx.t
                    break
                except OutsideDataError as e:
                    rt.message("error",
                               f"{e} at t = {ctx.t:.2f} s — the run stopped because this "
                               f"axis is set to stop the run (Error): extend the table, or set "
                               f"its outside-the-data setting to Clamp or Linear.")
                    failed_at = ctx.t
                    break

                if pace > 0:
                    target_wall = t / pace
                    while not cancelled:
                        lag = target_wall - (time.monotonic() - t_start_wall)
                        if lag <= 0:
                            break
                        time.sleep(min(0.05, lag))
                        if control:
                            for msg in control():
                                if msg.get("type") == "cancel":
                                    cancelled = True
                                elif msg.get("type") == "set_param":
                                    apply_control_msg(msg)

            # signal sources are stored with their value at the point's own time
            ctx.publish_sources(t)
            problem = trace.live_problem()
            if problem:
                rt.message("warning", problem)

            # -- record ----------------------------------------------------------
            # Only recorded steps are stored and streamed ("store every N steps"
            # decimates the output); the last step is always kept.
            if not (step % output_every == 0 or step == steps):
                continue
            times.append(t)
            rec_index = len(times) - 1
            rec = rt.series
            for el_id, port_id, value in chain(_bus_channels(ctx), _state_channels(ctx, gear_of)):
                lst = rec[(el_id, port_id)]
                while len(lst) < rec_index:
                    lst.append(None)  # no data yet — a gap, not a zero
                lst.append(value)

            if emit:
                emit({
                    "type": "step",
                    "t": t,
                    "pct": round(100.0 * step / steps, 1) if steps else 100.0,
                    "values": {f"{el}:{port}": round(val[-1], 5)
                               for (el, port), val in rec.items() if len(val) == rec_index + 1},
                })

        # ---- assemble result -------------------------------------------------------
        # a Lookup block's table is a controller's own schedule: held at its
        # edge it gives what the controller asks for, not physics past its
        # data, so only its summary rows say it left the table
        verdict = judge(trace, ctx.distance, rt.series, ctx.performance, solved,
                        [u for u in ctx.map_use if model.cdef_of[u.el_id].id != "signal.lookup"],
                        stopped or failed_at is not None)
        for level, text in verdict.messages:
            rt.message(level, text)
        unit_map = unit_groups()
        channels: list[Channel] = []
        port_lookup: dict[tuple[str, str], object] = {}
        for el_id, cdef in model.cdef_of.items():
            el = model.elements[el_id]
            for p in (list(cdef.ports) + list(el.dynamicPorts or [])):
                port_lookup[(el_id, p.id)] = p
        for (el_id, port_id), values in sorted(rt.series.items()):
            el = model.elements.get(el_id)
            pdef = port_lookup.get((el_id, port_id))
            if el is None or pdef is None:
                continue
            unit = unit_map.get(getattr(pdef, "unitGroup", None) or "No Unit", "-")
            channels.append(Channel(
                elementId=el_id,
                portId=port_id,
                label=f"{el.label} · {pdef.name}",
                unit=unit,
                timeSeries=[
                    {"t": times[i], "value": None if vv is None else round(vv, 5)}
                    for i, vv in enumerate(values)
                ],
            ))

        summary: list[SummaryValue] = []
        for b in ctx.batteries.values():
            label = model.elements[b.el_id].label
            summary.append(SummaryValue(label=f"{label} — final SOC", value=round(b.soc * 100.0, 2), unit="%"))
            summary.append(SummaryValue(label=f"{label} — energy delivered", value=round(b.energy_out_wh / 1000.0, 3), unit="kWh"))
            summary.append(SummaryValue(label=f"{label} — energy recuperated", value=round(b.energy_in_wh / 1000.0, 3), unit="kWh"))
            summary.append(SummaryValue(label=f"{label} — internal losses", value=round(b.loss_wh / 1000.0, 4), unit="kWh"))
        for mc in ctx.motors.values():
            if mc.limited_s > 0:
                summary.append(SummaryValue(
                    label=f"{model.elements[mc.el_id].label} — time limited by supply",
                    value=round(mc.limited_s, 2), unit="s"))
            if mc.regen_lost_wh > 0:
                # recuperation its command asked for that the supply could not
                # take (a full or charge-limited battery, a fuel cell, a one-way
                # DC-DC): the motor braked that much less
                summary.append(SummaryValue(
                    label=f"{model.elements[mc.el_id].label} — regeneration not recovered",
                    value=round(mc.regen_lost_wh / 1000.0, 4), unit="kWh"))
        for ec in ctx.engines.values():
            summary.append(SummaryValue(
                label=f"{model.elements[ec.el_id].label} — fuel used",
                value=round(ec.fuel_used_kg, 3), unit="kg"))
        for fc in ctx.fuelcells.values():
            summary.append(SummaryValue(
                label=f"{model.elements[fc.el_id].label} — energy supplied",
                value=round(fc.energy_wh / 1000.0, 3), unit="kWh"))
        for vs_id, e_wh in ctx.vsource_energy_wh.items():
            summary.append(SummaryValue(
                label=f"{model.elements[vs_id].label} — energy supplied",
                value=round(e_wh / 1000.0, 3), unit="kWh"))
        if ctx.veh_id:
            summary.append(SummaryValue(label="Distance driven", value=round(ctx.distance / 1000.0, 3), unit="km"))
            net_wh = sum(b.energy_out_wh - b.energy_in_wh for b in ctx.batteries.values())
            if ctx.distance > 100 and net_wh > 0:
                summary.append(SummaryValue(
                    label="Consumption", value=round(net_wh / 10.0 / (ctx.distance / 1000.0), 2),
                    unit="kWh/100km"))
            fuel_kg = sum(ec.fuel_used_kg for ec in ctx.engines.values())
            if ctx.distance > 100 and fuel_kg > 0:
                density = 0.745  # gasoline default when no tank declares one
                co2_per_kg = 3.17  # kg CO₂ per kg of gasoline, likewise
                if model.fuel_tank:
                    tank_p = ctx.params(model.fuel_tank)
                    try:
                        density = max(1e-3, float(tank_p.get("density_kg_per_l", density)))
                    except (TypeError, ValueError):
                        pass
                    try:
                        co2_per_kg = max(0.0, float(tank_p.get("co2_kg_per_kg", co2_per_kg)))
                    except (TypeError, ValueError):
                        pass
                liters = fuel_kg / density
                summary.append(SummaryValue(
                    label="Fuel consumption",
                    value=round(liters * 100.0 / (ctx.distance / 1000.0), 2), unit="l/100km"))
                summary.append(SummaryValue(
                    label="CO₂ emissions",
                    value=round(fuel_kg * co2_per_kg * 1000.0 / (ctx.distance / 1000.0), 1),
                    unit="g/km"))
        if ctx.throughput_wh > 0:
            # energy no source supplied or absorbed (last-resort clamps), as a
            # share of all the energy that went through the buses
            summary.append(SummaryValue(
                label="Electrical energy balance error",
                value=round(100.0 * ctx.residual_wh / ctx.throughput_wh, 4), unit="%"))
        # tables the run went past (listed only then, like the rows above):
        # for how long, as a share of the time solved, and how far; per
        # E-Motor or Engine the time above its maximum speed and the highest
        # speed
        edge_rows: set[str] = set()  # time and speeds, not energy figures
        for use in ctx.map_use:
            if use.outside_s <= 0:
                continue
            label = model.elements[use.el_id].label
            if use.what == "maximum speed":
                share_label = f"{label} — time above maximum speed"
                far = SummaryValue(label=f"{label} — highest speed",
                                   value=round(use.value), unit="1/min")
            else:
                share_label = f"{label} — time outside its {use.what} ({use.axis})"
                far = SummaryValue(label=f"{label} — furthest {use.axis} outside its {use.what}",
                                   value=round(use.value, 4), unit=use.unit)
            edge_rows |= {share_label, far.label}
            summary.append(SummaryValue(
                label=share_label, value=round(100.0 * use.outside_s / max(solved, 1e-9), 2),
                unit="%"))
            summary.append(far)
        summary += [SummaryValue(label=label, value=v, unit=u) for label, v, u in verdict.rows]
        summary.append(SummaryValue(label="Simulated duration", value=times[-1] if times else 0.0, unit="s"))

        # headline numbers that a failed check makes meaningless say why
        not_valid: dict[str, str] = {}
        if any(b.depleted_flagged for b in ctx.batteries.values()):
            not_valid["Consumption"] = "the battery reached its minimum SOC"
        if any(ec.stalled_flagged for ec in ctx.engines.values()):
            not_valid["Fuel consumption"] = not_valid["CO₂ emissions"] = "the fuel tank ran empty"
        if verdict.cycle_not_followed:
            for label in ("Consumption", "Fuel consumption", "CO₂ emissions"):
                not_valid[label] = "cycle not followed"
        if (stopped or failed_at is not None) and times:
            # figures per distance cover only the part of the cycle driven so
            # far, and a performance test's top speed only its speed so far
            why = (f"run cancelled at t = {times[-1]:g} s" if stopped
                   else f"run stopped by an error at t = {failed_at:.2f} s")
            for label in ("Consumption", "Fuel consumption", "CO₂ emissions", "Maximum speed"):
                not_valid.setdefault(label, why)
        if verdict.beyond_reason:
            # a machine or source ran past its data: what depends on how it ran
            for label in ("Consumption", "Fuel consumption", "CO₂ emissions",
                          *(row[0] for row in verdict.rows)):
                not_valid[label] = verdict.beyond_reason
        if ctx.throughput_wh > 0 and ctx.residual_wh > 1e-3 * ctx.throughput_wh:
            for s in summary:
                if (s.unit in ("kWh", "kWh/100km", "%") and s.label not in edge_rows
                        and s.label != "Electrical energy balance error"):
                    not_valid.setdefault(s.label, "the electrical energy balance does not close")
        if verdict.broke_down:
            for s in summary:
                if s.label != "Simulated duration":
                    not_valid[s.label] = "the solution broke down"
        for s in summary:
            s.notValid = not_valid.get(s.label)

        has_error = any(m.level == "error" for m in rt.messages)
        has_warning = any(m.level == "warning" for m in rt.messages)
        status = ("failed" if has_error else "cancelled" if stopped
                  else "warning" if has_warning else "success")
        rec_note = f", stored every {output_every}" if output_every > 1 else ""
        last_note = f", the last one {h_last:g} s" if steps and short_last else ""
        rt.messages.insert(0, SimMessage(
            level="info",
            text=f"Case '{case.name}' solved: {steps} steps × {dt_rec:g} s{last_note} "
                 f"({n_sub} sub-steps each), {len(times)} points recorded{rec_note}, "
                 f"{len(channels)} result channels.",
        ))
        return SimResult(
            caseId=case_id,
            status=status,
            messages=rt.messages,
            channels=channels,
            summary=summary,
        )
    finally:
        if ctx.sandbox is not None:
            ctx.sandbox.close()


ChannelValue = tuple[str, str, float]  # (element id, port id, value)


def _bus_channels(ctx: RunContext) -> Iterator[ChannelValue]:
    """Recorded channels the slaves already publish on the signal bus
    (signal blocks, vehicle, driver, electrical consumers, brakes)."""
    model, bus = ctx.model, ctx.rt.signal_values
    for el_id, cdef in model.cdef_of.items():
        tdef = cdef.id
        if tdef in ("signal.constant", "control.pid", "signal.lookup"):
            ports: tuple[str, ...] = ("sig_out",)
        elif tdef == "signal.driving_task":
            ports = ("sig_demand",)
        elif tdef == "signal.script":
            ports = tuple(po.id for po in (model.elements[el_id].dynamicPorts or [])
                          if po.direction == "output")
        elif tdef == "signal.road_profile":
            ports = ("sig_grade",)
        elif tdef == "vehicle.body":
            ports = ("sig_speed", "sig_distance")
        elif tdef == "driver.driver":
            ports = ("sig_traction_cmd", "sig_brake_cmd", "sig_accel_pedal", "sig_brake_pedal")
        elif tdef in ("electric.constant_drive", "electric.node"):
            ports = ("sig_power",)
        elif tdef == "electric.voltage_source":
            ports = ("sig_power", "sig_voltage")
        elif tdef == "mech.brake":
            ports = ("sig_torque",)
        else:
            continue
        for port_id in ports:
            yield el_id, port_id, bus.get((el_id, port_id), 0.0)


def _state_channels(ctx: RunContext, gear_of: dict[str, float],
                    only: Optional[set[str]] = None) -> Iterator[ChannelValue]:
    """Recorded channels computed from states and element caches (batteries,
    machines, tanks, driveline). ``only`` limits them to those elements."""
    for el_id, port_id, fn in _state_channel_fns(ctx, gear_of, only):
        value = fn()
        if value is not None:
            yield el_id, port_id, value


ChannelFn = tuple[str, str, Callable[[], Optional[float]]]  # value getter, None = no data yet


def _state_channel_fns(ctx: RunContext, gear_of: dict[str, float],
                       only: Optional[set[str]] = None) -> Iterator[ChannelFn]:
    """The state channels as getters, so a caller can keep the ones it needs
    and read them again: valid until the drivelines change
    (``ctx.layout_version``)."""
    model = ctx.model
    for el_id in (model.cdef_of if only is None else only):
        cdef = model.cdef_of.get(el_id)
        if cdef is None:
            continue
        tdef = cdef.id
        if tdef == "battery.generic" and el_id in ctx.batteries:
            b = ctx.batteries[el_id]
            yield el_id, "sig_soc", lambda b=b: b.soc * 100.0
            yield el_id, "sig_voltage", lambda b=b: b.v_term
            yield el_id, "sig_current", lambda b=b: b.current
            yield el_id, "sig_power", lambda b=b: b.power_w / 1000.0
        elif tdef == "motor.emotor" and el_id in ctx.motors:
            mc = ctx.motors[el_id]
            yield el_id, "sig_speed", lambda mc=mc: mc.rpm
            yield el_id, "sig_torque", lambda mc=mc: mc.torque
            yield el_id, "sig_mech_power", lambda mc=mc: mc.p_mech_w / 1000.0
            yield el_id, "sig_elec_power", lambda mc=mc: mc.p_elec_w / 1000.0
            yield el_id, "sig_losses", lambda mc=mc: mc.p_loss_w / 1000.0
        elif tdef == "engine.combustion" and el_id in ctx.engines:
            ec = ctx.engines[el_id]
            yield el_id, "sig_speed", lambda ec=ec: ec.rpm
            yield el_id, "sig_torque", lambda ec=ec: ec.torque
            yield el_id, "sig_fuel_rate", lambda ec=ec: ec.fuel_kgh
            yield el_id, "sig_power", lambda ec=ec: ec.p_mech_w / 1000.0
        elif tdef == "fuelcell.stack" and el_id in ctx.fuelcells:
            fc = ctx.fuelcells[el_id]
            yield el_id, "sig_voltage", lambda fc=fc: fc.voltage
            yield el_id, "sig_current", lambda fc=fc: fc.current
            yield el_id, "sig_power", lambda fc=fc: fc.power_w / 1000.0
            yield el_id, "sig_h2_rate", lambda fc=fc: fc.h2_kgh
        elif tdef in ("fuel.tank", "fuel.h2_tank") and el_id in ctx.tanks:
            tk = ctx.tanks[el_id]
            yield el_id, "sig_level", lambda tk=tk: 100.0 * tk.mass_kg / tk.capacity_kg
            yield el_id, "sig_mass", lambda tk=tk: tk.mass_kg
        elif tdef == "controller.dcdc":  # no data until the converter first runs
            yield el_id, "sig_power_in", lambda el_id=el_id: _dcdc_kw(ctx, el_id, "in")
            yield el_id, "sig_power_out", lambda el_id=el_id: _dcdc_kw(ctx, el_id, "out")
            yield el_id, "sig_losses", lambda el_id=el_id: _dcdc_kw(ctx, el_id, "loss")

    for st in ctx.dls:
        if only is not None and only.isdisjoint(st.dl.element_group):
            continue
        plan = st.plan
        for j in st.dl.joints:
            if only is not None and j.el_id not in only:
                continue
            if j.kind == "split":
                yield j.el_id, "sig_torque_a", lambda st=st, j=j: st.joint_torque_a.get(j.el_id, 0.0)
                yield j.el_id, "sig_torque_b", lambda st=st, j=j: st.joint_torque_b.get(j.el_id, 0.0)
                yield j.el_id, "sig_speed_in", (
                    lambda st=st, j=j: abs(st.joint_speed_in.get(j.el_id, 0.0)) * RPM)
            else:
                yield j.el_id, "sig_torque", lambda st=st, j=j: st.clutch_torque.get(j.el_id, 0.0)
                yield j.el_id, "sig_slip_speed", (
                    lambda st=st, j=j: st.clutch_slip.get(j.el_id, 0.0) * RPM)
        if plan.over_constrained or not plan.n:
            continue
        for s_idx, seg in enumerate(st.dl.segments):
            if only is not None and only.isdisjoint(seg.element_ms):
                continue

            def omega_ref(st=st, s_idx=s_idx) -> float:
                return ctx.seg_speed(st, s_idx)

            for w in seg.wheels:
                if only is not None and w.el_id not in only:
                    continue

                def slip(w=w, omega_ref=omega_ref) -> float:
                    omega_w = w.m * omega_ref()
                    v_den = max(abs(ctx.v), V_EPS)
                    return (omega_w * w.radius - ctx.v) / v_den if ctx.veh_id else 0.0
                yield w.el_id, "sig_speed", lambda w=w, o=omega_ref: abs(w.m * o()) * RPM
                yield w.el_id, "sig_slip", slip
                yield w.el_id, "sig_force", lambda w=w: ctx.last_forces.get(w.el_id, 0.0)
                yield w.el_id, "sig_torque", (
                    lambda w=w: ctx.last_forces.get(w.el_id, 0.0) * w.radius)
            for el_id2, m2 in seg.element_ms.items():
                if only is not None and el_id2 not in only:
                    continue
                tdef2 = model.cdef_of[el_id2].id

                def speed(m2=m2, omega_ref=omega_ref) -> float:
                    return abs(m2 * omega_ref()) * RPM
                if tdef2 == "mech.node":
                    yield el_id2, "sig_speed", speed
                elif tdef2 == "mech.final_drive":
                    yield el_id2, "sig_speed_out", speed
                    yield el_id2, "sig_power", lambda st=st: st.chain_power_w / 1000.0
                elif tdef2 == "mech.gearbox":
                    yield el_id2, "sig_speed_out", speed
                    yield el_id2, "sig_gear", lambda el_id2=el_id2: gear_of.get(
                        el_id2, float(ctx.params(el_id2).get("default_gear", 1) or 1))
                elif tdef2 == "mech.shaft":
                    yield el_id2, "sig_power", lambda st=st: st.chain_power_w / 1000.0
            for pr in seg.props:
                if only is not None and pr.el_id not in only:
                    continue
                yield pr.el_id, "sig_speed", lambda pr=pr, o=omega_ref: abs(pr.m * o()) * RPM

                def shaft_power(pr=pr, omega_ref=omega_ref) -> float:
                    omega_p = pr.m * omega_ref()
                    rpm_p = abs(omega_p) * RPM
                    return abs(pr.t_ref * (rpm_p / pr.n_ref) ** 2 * omega_p) / 1000.0
                yield pr.el_id, "sig_shaft_power", shaft_power


def _dcdc_kw(ctx: RunContext, el_id: str, which: str) -> Optional[float]:
    flows = ctx.dcdc_flows.get(el_id)
    if flows is None:
        return None
    p_in, p_out = flows
    return (p_in if which == "in" else p_out if which == "out" else p_in - p_out) / 1000.0
