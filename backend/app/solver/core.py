"""Solver orchestration: simulate() drives the co-simulation master.

Per recorded step (the case timeStep) the master steps the wrapped domain
slaves — control (signals, ÷n_sub) → gear (÷n_sub) → driver → mechanical
+ vehicle → electrical — over n_sub micro-steps (semi-implicit Euler,
MAX_SUBSTEP), then this module records channels, streams progress, paces
against real time and applies live control messages. Domain physics lives
in domains.py (RunContext + slaves); shared numerics in runtime.py.

Gear shifts and live lock/unlock toggles rebuild the driveline plan at
recording boundaries, carrying rotational states over via per-element
anchor speeds.
"""
from __future__ import annotations

import math
import time
from typing import Optional

from ..library import unit_groups
from ..schemas import Channel, Project, SimMessage, SimResult, SummaryValue
from .domains import ModelInitError, RunContext, build_slaves
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
        ctx = RunContext(project, model, rt, gear_of, case_overrides, dt_rec, n_sub, dt)
    except ModelInitError as e:
        return SimResult(
            caseId=case_id, status="failed", channels=[],
            messages=[SimMessage(level="error", text=t) for t in e.messages],
        )

    # Phase 1.4: the wholesale-wrapped slaves share all coupling through the
    # RunContext, so the master runs with an empty route table for now; the
    # declared-variable pool takes over as per-component models are extracted.
    master = Master(build_slaves(ctx), routes={})
    master.initialize(0.0)

    def apply_control_msg(msg: dict) -> None:
        master.set_parameter(
            var_name(str(msg.get("elementId")), str(msg.get("key"))), msg.get("value"))

    # ---- main loop -----------------------------------------------------------
    # Point 0 is the initial state at t = 0; every later point is recorded at
    # the end time of the step that produced it, and the last step ends
    # exactly at the case duration.
    cancelled = False
    times: list[float] = []
    t_start_wall = time.monotonic()
    h_last = t_end - (steps - 1) * dt_rec if steps else 0.0
    short_last = abs(h_last - dt_rec) > 1e-9 * dt_rec
    t = 0.0

    for step in range(steps + 1):
        if step > 0:
            t_prev = t
            t = t_end if step == steps else step * dt_rec
            ctx.t_rec = t_prev

            if control:
                for msg in control():
                    if msg.get("type") == "cancel":
                        cancelled = True
                    elif msg.get("type") == "set_param":
                        apply_control_msg(msg)
            if cancelled:
                rt.message("info", f"Simulation cancelled by user at t = {t_prev:g} s.")
                break

            # -- sub-steps (master schedules control/gear at recorded cadence) --
            h, n, h_sub = dt_rec, n_sub, dt
            if step == steps and short_last:
                h = h_last
                n = max(1, math.ceil(h / MAX_SUBSTEP - 1e-9))
                h_sub = h / n
            ctx.dt_rec, ctx.dt = h, h_sub
            try:
                for j in range(n):
                    master.step(t_prev + j * h_sub, h_sub)
            except SlaveStepError:
                # the failing slave already emitted its error message
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

        # -- record / publish --------------------------------------------------
        # Always publish (so signal routing stays fresh for the next step); only
        # append to the series on recorded steps, so "store every N steps"
        # decimates the stored/streamed output. The last step is always kept.
        do_record = (step % output_every == 0) or (step == steps)
        if do_record:
            times.append(t)
        rec_index = len(times) - 1
        rec = rt.series

        def record(el_id: str, port_id: str, value: float) -> None:
            rt.publish(el_id, port_id, value)
            if not do_record:
                return
            lst = rec[(el_id, port_id)]
            while len(lst) < rec_index:
                lst.append(None)  # no data yet — a gap, not a zero
            lst.append(value)

        for el_id, cdef in model.cdef_of.items():
            tdef = cdef.id
            if tdef == "signal.constant":
                record(el_id, "sig_out", rt.signal_values.get((el_id, "sig_out"), 0.0))
            elif tdef == "signal.driving_task":
                record(el_id, "sig_demand", rt.signal_values.get((el_id, "sig_demand"), 0.0))
            elif tdef == "signal.script":
                for po in (model.elements[el_id].dynamicPorts or []):
                    if po.direction == "output":
                        record(el_id, po.id, rt.signal_values.get((el_id, po.id), 0.0))
            elif tdef in ("control.pid", "signal.lookup"):
                record(el_id, "sig_out", rt.signal_values.get((el_id, "sig_out"), 0.0))
            elif tdef == "signal.road_profile":
                record(el_id, "sig_grade", rt.signal_values.get((el_id, "sig_grade"), 0.0))
            elif tdef == "vehicle.body":
                for pid in ("sig_speed", "sig_distance"):
                    record(el_id, pid, rt.signal_values.get((el_id, pid), 0.0))
            elif tdef == "driver.driver":
                for pid in ("sig_traction_cmd", "sig_brake_cmd",
                            "sig_accel_pedal", "sig_brake_pedal"):
                    record(el_id, pid, rt.signal_values.get((el_id, pid), 0.0))
            elif tdef == "battery.generic" and el_id in ctx.batteries:
                b = ctx.batteries[el_id]
                record(el_id, "sig_soc", b.soc * 100.0)
                record(el_id, "sig_voltage", b.v_term)
                record(el_id, "sig_current", b.current)
                record(el_id, "sig_power", b.power_w / 1000.0)
            elif tdef == "motor.emotor" and el_id in ctx.motors:
                mc = ctx.motors[el_id]
                record(el_id, "sig_speed", mc.rpm)
                record(el_id, "sig_torque", mc.torque)
                record(el_id, "sig_mech_power", mc.p_mech_w / 1000.0)
                record(el_id, "sig_elec_power", mc.p_elec_w / 1000.0)
                record(el_id, "sig_losses", mc.p_loss_w / 1000.0)
            elif tdef == "engine.combustion" and el_id in ctx.engines:
                ec = ctx.engines[el_id]
                record(el_id, "sig_speed", ec.rpm)
                record(el_id, "sig_torque", ec.torque)
                record(el_id, "sig_fuel_rate", ec.fuel_kgh)
                record(el_id, "sig_power", ec.p_mech_w / 1000.0)
            elif tdef == "fuelcell.stack" and el_id in ctx.fuelcells:
                fc = ctx.fuelcells[el_id]
                record(el_id, "sig_voltage", fc.voltage)
                record(el_id, "sig_current", fc.current)
                record(el_id, "sig_power", fc.power_w / 1000.0)
                record(el_id, "sig_h2_rate", fc.h2_kgh)
            elif tdef in ("fuel.tank", "fuel.h2_tank") and el_id in ctx.tanks:
                tk = ctx.tanks[el_id]
                record(el_id, "sig_level", 100.0 * tk.mass_kg / tk.capacity_kg)
                record(el_id, "sig_mass", tk.mass_kg)
            elif tdef == "electric.constant_drive":
                record(el_id, "sig_power", rt.signal_values.get((el_id, "sig_power"), 0.0))
            elif tdef == "electric.voltage_source":
                record(el_id, "sig_power", rt.signal_values.get((el_id, "sig_power"), 0.0))
                record(el_id, "sig_voltage", rt.signal_values.get((el_id, "sig_voltage"), 0.0))
            elif tdef == "electric.node":
                record(el_id, "sig_power", rt.signal_values.get((el_id, "sig_power"), 0.0))
            elif tdef == "controller.dcdc" and el_id in ctx.dcdc_flows:
                p_in, p_out = ctx.dcdc_flows[el_id]
                record(el_id, "sig_power_in", p_in / 1000.0)
                record(el_id, "sig_power_out", p_out / 1000.0)
                record(el_id, "sig_losses", (p_in - p_out) / 1000.0)
            elif tdef == "mech.brake":
                record(el_id, "sig_torque", rt.signal_values.get((el_id, "sig_torque"), 0.0))

        for st in ctx.dls:
            plan = st.plan
            for j in st.dl.joints:
                if j.kind == "split":
                    record(j.el_id, "sig_torque_a", st.joint_torque_a.get(j.el_id, 0.0))
                    record(j.el_id, "sig_torque_b", st.joint_torque_b.get(j.el_id, 0.0))
                    record(j.el_id, "sig_speed_in",
                           abs(st.joint_speed_in.get(j.el_id, 0.0)) * RPM)
                else:
                    record(j.el_id, "sig_torque", st.clutch_torque.get(j.el_id, 0.0))
                    record(j.el_id, "sig_slip_speed",
                           st.clutch_slip.get(j.el_id, 0.0) * RPM)
            if plan.over_constrained or not plan.n:
                continue
            for s_idx, seg in enumerate(st.dl.segments):
                omega_ref = ctx.seg_speed(st, s_idx)
                for w in seg.wheels:
                    omega_w = w.m * omega_ref
                    force = ctx.last_forces.get(w.el_id, 0.0)
                    v_den = max(abs(ctx.v), V_EPS)
                    slip = (omega_w * w.radius - ctx.v) / v_den if ctx.veh_id else 0.0
                    record(w.el_id, "sig_speed", abs(omega_w) * RPM)
                    record(w.el_id, "sig_slip", slip)
                    record(w.el_id, "sig_force", force)
                    record(w.el_id, "sig_torque", force * w.radius)
                for el_id2, m2 in seg.element_ms.items():
                    tdef2 = model.cdef_of[el_id2].id
                    if tdef2 == "mech.node":
                        record(el_id2, "sig_speed", abs(m2 * omega_ref) * RPM)
                    elif tdef2 == "mech.final_drive":
                        record(el_id2, "sig_speed_out", abs(m2 * omega_ref) * RPM)
                        record(el_id2, "sig_power", st.chain_power_w / 1000.0)
                    elif tdef2 == "mech.gearbox":
                        record(el_id2, "sig_speed_out", abs(m2 * omega_ref) * RPM)
                        record(el_id2, "sig_gear", gear_of.get(
                            el_id2, float(ctx.params(el_id2).get("default_gear", 1) or 1)))
                    elif tdef2 == "mech.shaft":
                        record(el_id2, "sig_power", st.chain_power_w / 1000.0)
                for pr in seg.props:
                    record(pr.el_id, "sig_speed", abs(pr.m * omega_ref) * RPM)
                    omega_p = pr.m * omega_ref
                    rpm_p = abs(omega_p) * RPM
                    record(pr.el_id, "sig_shaft_power",
                           abs(pr.t_ref * (rpm_p / pr.n_ref) ** 2 * omega_p) / 1000.0)

        if emit and do_record:
            emit({
                "type": "step",
                "t": t,
                "pct": round(100.0 * step / steps, 1) if steps else 100.0,
                "values": {f"{el}:{port}": round(val[-1], 5)
                           for (el, port), val in rec.items() if len(val) == rec_index + 1},
            })

    # ---- assemble result -------------------------------------------------------
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
            if model.fuel_tank:
                try:
                    density = max(1e-3, float(
                        ctx.params(model.fuel_tank).get("density_kg_per_l", density)))
                except (TypeError, ValueError):
                    pass
            liters = fuel_kg / density
            summary.append(SummaryValue(
                label="Fuel consumption",
                value=round(liters * 100.0 / (ctx.distance / 1000.0), 2), unit="l/100km"))
    summary.append(SummaryValue(label="Simulated duration", value=times[-1] if times else 0.0, unit="s"))

    has_error = any(m.level == "error" for m in rt.messages)
    has_warning = any(m.level == "warning" for m in rt.messages) or cancelled
    status = "failed" if has_error else ("warning" if has_warning else "success")
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
