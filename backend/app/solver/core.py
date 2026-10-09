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
live lock/unlock toggles at recording boundaries; the speeds jump to the
new constraints keeping the driveline's angular momentum, and the kinetic
energy that loses is booked (domains.RunContext.carry_over).

A lap case (SimCase.kind "lap") runs lapsim's slave set instead, one master
step per stretch of the Race Track (lapsim.run_laps), and is recorded and
summarised by the same code.
"""
from __future__ import annotations

import math
import operator
import time
from typing import Callable, Iterator, Optional

from .. import cycles
from ..library import unit_groups
from ..schemas import Channel, Project, SimMessage, SimResult, SummaryValue
from . import balance, fs_events, lapsim
from .battery import SOP_PULSES, sop
from .domains import ModelInitError, RunContext, build_slaves
from .energy import add_lap, energy_flows
from .keys import fill_keys
from .labfig import LabLog, lab_rows
from .maps import OutsideDataError, slug
from .master import Master, SlaveStepError
from .network import ModelError, build_model
from .profiles import distance_axis, lap_length
from .references import check_references, hand_checks
from .reports import RunRecorder
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
    MovingAverage,
    Runtime,
    SingularMatrixError,
    TankState,
    _sign,
    make_plan,
    solve_linear,
    usable_energy_left_wh,
)
from .slave import var_name
from .stability import solver_step
from .verdict import CycleTrace, judge, terminal_checks


def simulate(
    project: Project,
    case_id: str,
    emit: Optional[EmitFn] = None,
    control: Optional[ControlFn] = None,
) -> SimResult:
    """Run a case: once, or, for a hybrid's charge-balanced case, until its
    battery ends where it started (balance.py, ENG-33). Every summary row
    has a stable key (keys.py, AI-07)."""
    result = balance.simulate_balanced(project, case_id, emit, control, run_case)
    fill_keys(result.summary, project)
    return result


def run_case(
    project: Project,
    case_id: str,
    emit: Optional[EmitFn] = None,
    control: Optional[ControlFn] = None,
    totals: Optional[dict] = None,
) -> SimResult:
    """One run of a case. ``totals``, when given, is filled with what charge
    balancing needs: each battery's start and end SOC and stored energy
    change, and the fuel burnt."""
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
    # the solver step: MAX_SUBSTEP, or less when a part is too stiff for it (ENG-14)
    choice = solver_step(model, MAX_SUBSTEP) if case.kind != "lap" else None
    h_max = choice.step if choice else MAX_SUBSTEP
    if choice and choice.step < MAX_SUBSTEP:
        rt.message("info", f"Solver step reduced to {choice.step * 1000:.3g} ms (from "
                           f"{MAX_SUBSTEP * 1000:g} ms) because {choice.reason} is too stiff "
                           f"for the larger step: the run takes about "
                           f"{MAX_SUBSTEP / choice.step:.3g} times as long.")
    for text, _ in choice.warnings if choice else ():
        rt.message("warning", text)
    n_sub = max(1, math.ceil(dt_rec / h_max - 1e-9))
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
    soc_start = {el_id: b.soc for el_id, b in ctx.batteries.items()}
    ctx.performance = case.kind != "cycle"  # the trace is sampled every solver step
    ctx.full_throttle = case.kind == "acceleration"
    if case.fsEvent:  # its power rule check, on the rules' own average (MOD-43)
        for b in ctx.batteries.values():
            b.rule_avg = MovingAverage(fs_events.RULE_WINDOW_S)
    if case.kind == "cycle":  # say when the figures cannot be compared (CON-26)
        for el_id, cdef in model.cdef_of.items():
            if cdef.id != "signal.driving_task":
                continue
            task_p = model.params_of[el_id]
            if not task_p.get("cycle"):
                rt.message("info", f"Driving Task '{model.elements[el_id].label}' follows a typed "
                                   f"profile (a demo or your own points), not a standard "
                                   f"drive cycle: compare its "
                                   f"figures only with runs on the same profile, not with "
                                   f"published ones.")
            elif cycles.Catalogue.of(project).is_own(str(task_p["cycle"])):
                name = cycles.Catalogue.of(project).name(str(task_p["cycle"]))
                rt.message("info", f"Driving Task '{model.elements[el_id].label}' drives "
                                   f"'{name}', a drive cycle of this project's own, not a "
                                   f"standard one: compare its figures only with runs on the "
                                   f"same cycle, not with published ones.")
            elif why := cycles.not_as_published(task_p, t_end):
                name = cycles.CYCLES.get(str(task_p["cycle"]), {}).get("name", task_p["cycle"])
                rt.message("info", f"Driving Task '{model.elements[el_id].label}' drives {name} "
                                   f"{why}, not the standard drive cycle: compare its figures "
                                   f"only with runs on the same settings, not with published "
                                   f"ones. The run has no per-phase figures.")
    # the run ends when the vehicle has driven this far, m (None: at the duration)
    end_d = (max(0.0, case.startLine) + case.endDistance
             if case.endDistance and case.endDistance > 0 else None)
    if end_d is None and case.endLaps and case.endLaps > 0 and case.kind in ("cycle", "performance"):
        end_d = _laps_distance(ctx, case.endLaps)
        if end_d is None:
            rt.message("warning", f"Case '{case.name}' asks for {case.endLaps:g} laps, but the "
                                  f"Driver does not follow a Driving Task whose Profile Axis is "
                                  f"Distance (with at least two points), so the run lasts its "
                                  f"{t_end:g} s duration.")
        else:
            end_d += max(0.0, case.startLine)
    ctx.end_distance = end_d

    try:
        lap = None
        if case.kind == "lap":  # the Race Track sets the run (lapsim)
            found = lapsim.problems(model, output_every)
            if any(level == "error" for level, _, _ in found):
                return SimResult(caseId=case_id, status="failed", channels=[], messages=[
                    SimMessage(level="error", text=t) for level, t, _ in found if level == "error"])
            for level, text, _ in found:
                rt.message(level, text)
            rt.message("info", "Lap mode results are quasi-steady-state estimates: an ideal "
                               "driver at the tyres' limit on the given line, with no "
                               "transients, suspension, yaw or tyre slip, ideal brake balance "
                               "and regeneration held to the driven wheels' grip. They are "
                               "usually optimistic: calibrate the tyres' μ and μ_y and the "
                               "downforce against a lap your car has driven.")
            lap = ctx.lap = lapsim.LapRun(ctx)
            if case.fsEvent == "endurance" and lap.laps >= 2:  # the driver change
                lap.stop_after = lap.laps // 2 - 1
            for level, text in fs_events.lap_check(case, lap.laps, lap.track.length):
                rt.message(level, text)
        # Phase 1.4: the wholesale-wrapped slaves share all coupling through the
        # RunContext, so the master runs with an empty route table for now; the
        # declared-variable pool takes over as per-component models are extracted.
        master = Master(lapsim.slaves(ctx, lap) if lap else build_slaves(ctx), routes={})
        master.initialize(0.0)

        # Signals that feed a block input: the slaves publish their own outputs,
        # and the ones computed from states (SOC, speeds, levels …) are refreshed
        # after every solver step so controllers never read a stale value.
        # Only the getters with a wired port are evaluated; the list is rebuilt
        # when a gear shift or a lock toggle changes the drivelines.
        routed = set(model.signal_route.values())
        # the getters with a wired channel, and each one's wired channels
        routed_groups: list[tuple[Callable[[], tuple | list], list[tuple[int, ChannelKey]]]] = []
        routed_layout = -1

        def publish_routed_states() -> None:
            nonlocal routed_groups, routed_layout
            if routed_layout != ctx.layout_version:
                routed_layout = ctx.layout_version
                routed_groups = []
                for keys, fn, _ in _state_channel_groups(ctx, gear_of):
                    wired = [(k, key) for k, key in enumerate(keys) if key in routed]
                    if wired:
                        routed_groups.append((fn, wired))
            for fn, wired in routed_groups:
                values = fn()
                for k, (el_id, port_id) in wired:
                    value = values[k]
                    if value is not None:
                        rt.publish(el_id, port_id, value)

        publish_routed_states()
        # each point's lowest, highest and time-averaged value over the output
        # interval it ends, solver step by solver step (ENG-16): kept when an
        # interval holds more than one solver step (a lap case records every
        # stretch it solves)
        env = (_Envelope(ctx, gear_of)
               if lap is None and (n_sub > 1 or output_every > 1) else None)
        # the energy, duty and limit reports, booked after every solver step
        recorder = RunRecorder(ctx, energy=getattr(case, "energyReport", True))
        recorder.lap = lap is not None

        def after_step() -> None:
            publish_routed_states()
            recorder.step()

        lablog = LabLog.for_run(model, case.kind, t_end)  # per-phase totals (CON-05)
        lablog.sample(0.0, ctx)
        trace = CycleTrace(ctx)  # target vs vehicle speed, for the run verdict
        if lap is None:  # a lap case follows no target
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
        arrived = False  # the vehicle reached end_d: the run ends in this step

        # the recorded channels: those on the bus, and the state channels'
        # getters, kept while the drivelines stay as they are
        rec_bus = [(el_id, port_id) for el_id, port_id, _ in _bus_channels(ctx)]
        rec_groups: list[ChannelGroup] = []
        rec_layout = -1

        def record(t: float, pct: float) -> None:
            """Store the channels as the point at time t, and stream them."""
            nonlocal rec_groups, rec_layout
            if rec_layout != ctx.layout_version:
                rec_layout = ctx.layout_version
                rec_groups = list(_state_channel_groups(ctx, gear_of))
            times.append(t)
            rec_index = len(times) - 1
            rec = rt.series
            bus = rt.signal_values
            values = [(key, bus.get(key, 0.0)) for key in rec_bus]
            for keys, fn, _ in rec_groups:
                values += [(key, value) for key, value in zip(keys, fn())
                           if value is not None]  # (None: no data yet)
            for key, value in values:
                lst = rec[key]
                while len(lst) < rec_index:
                    lst.append(None)  # no data yet — a gap, not a zero
                lst.append(value)
                if env is not None:
                    env.record(key, rec_index, value)
            if env is not None:
                env.reset()
            if emit:
                emit({
                    "type": "step",
                    "t": t,
                    "pct": pct,
                    "values": {f"{el}:{port}": val[-1]
                               for (el, port), val in rec.items() if len(val) == rec_index + 1},
                })

        if lap is not None:  # the laps, one step per stretch of track
            try:
                solved, stopped = lapsim.run_laps(ctx, master, lap, record, after_step,
                                                  control, apply_control_msg, output_every)
            except SlaveStepError:
                solved = failed_at = ctx.t  # the failing slave already emitted its error message
            except lapsim.LapError as e:
                rt.message("error", str(e))
                solved = failed_at = ctx.t
            except OutsideDataError as e:
                rt.message("error",
                           f"{e} at t = {ctx.t:.2f} s — the run stopped because this "
                           f"axis is set to stop the run (Error): extend the table, or set "
                           f"its outside-the-data setting to Clamp or Linear.")
                solved = failed_at = ctx.t
        for step in range(0 if lap else steps + 1):  # (a lap case ran its laps above)
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
                    n = max(1, math.ceil(h_last / h_max - 1e-9))
                    h_sub = h_last / n
                ctx.dt = h_sub
                try:
                    for j in range(n):
                        ctx.t = t_prev + j * h_sub
                        master.step(ctx.t, h_sub)
                        solved = t_prev + (j + 1) * h_sub
                        after_step()
                        if env is not None:
                            env.add(h_sub)
                        arrived = end_d is not None and ctx.distance >= end_d
                        trace.sample(solved, last=arrived or (step == steps and j == n - 1))
                        if arrived:
                            break
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
                if arrived:
                    t = solved  # the last point: the end of the solver step that got there
                lablog.sample(solved, ctx)

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
            if step % output_every == 0 or step == steps or arrived:
                record(t, round(100.0 * step / steps, 1) if steps and not arrived else 100.0)
            if arrived:
                break

        # ---- assemble result -------------------------------------------------------
        # a Lookup block's table is a controller's own schedule: held at its
        # edge it gives what the controller asks for, not physics past its
        # data, so only its summary rows say it left the table
        verdict = judge(trace, ctx.distance, rt.series, ctx.performance, solved,
                        [u for u in ctx.map_use if model.cdef_of[u.el_id].id != "signal.lookup"],
                        stopped or failed_at is not None, case)
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
            lo, hi, mean = env.series(el_id, port_id, len(values)) if env else (None,) * 3
            channels.append(Channel(
                elementId=el_id,
                portId=port_id,
                label=f"{el.label} · {pdef.name}",
                unit=unit,
                timeSeries=[{"t": times[i], "value": vv} for i, vv in enumerate(values)],
                min=lo, max=hi, mean=mean,
            ))

        summary: list[SummaryValue] = []
        lab_base: dict[str, str] = {}  # a lab-style row → the row whose validity it shares
        for b in ctx.batteries.values():
            label = model.elements[b.el_id].label
            summary.append(SummaryValue(key=f"{b.el_id}.final_soc_pct", label=f"{label} — final SOC", value=b.soc * 100.0, unit="%"))
            summary.append(SummaryValue(key=f"{b.el_id}.energy_delivered_kwh", label=f"{label} — energy delivered", value=b.energy_out_wh / 1000.0, unit="kWh"))
            summary.append(SummaryValue(key=f"{b.el_id}.energy_recuperated_kwh", label=f"{label} — energy recuperated", value=b.energy_in_wh / 1000.0, unit="kWh"))
            summary.append(SummaryValue(key=f"{b.el_id}.internal_losses_kwh", label=f"{label} — internal losses", value=b.loss_wh / 1000.0, unit="kWh"))
            cp = b.cells
            if cp is not None and cp.cells:  # built from cells (MOD-08)
                summary.append(SummaryValue(key=f"{b.el_id}.layout_cells", label=f"{label} — layout",
                                            value=float(cp.ns * cp.np), unit="cells"))
                summary.append(SummaryValue(key=f"{b.el_id}.charge_capacity_ah", label=f"{label} — charge capacity",
                                            value=b.q_ah, unit="Ah"))
                summary.append(SummaryValue(key=f"{b.el_id}.pack_mass_kg", label=f"{label} — pack mass (estimate)",
                                            value=cp.mass_kg, unit="kg"))
                if cp.v_cell_low < math.inf:
                    summary.append(SummaryValue(key=f"{b.el_id}.lowest_cell_voltage_v", label=f"{label} — lowest cell voltage",
                                                value=cp.v_cell_low, unit="V",
                                                limit=cp.v_min or None))
                    summary.append(SummaryValue(key=f"{b.el_id}.highest_cell_voltage_v", label=f"{label} — highest cell voltage",
                                                value=cp.v_cell_high, unit="V",
                                                limit=cp.v_max or None))
            if cp is not None:
                for what, secs in sorted(cp.limit_s.items()):
                    summary.append(SummaryValue(key=f"{b.el_id}.time_at_{slug(what)}_limit_s",
                                            label=f"{label} — time at {what} limit",
                                                value=secs, unit="s"))
            if b.check is not None:  # an Output Power Limit or a Voltage Class
                rows, problems = terminal_checks(b.el_id, label, b.check, usable_energy_left_wh(b) / 1000.0,
                                                 b.depleted_flagged)
                summary += [SummaryValue(key=k, label=row_label, value=v, unit=u, limit=lim,
                                         passed=ok)
                            for row_label, v, u, lim, ok, k in rows]
                for text in problems:
                    rt.message("warning", text)
        for mc in ctx.motors.values():
            if mc.limited_s > 0:
                summary.append(SummaryValue(
                    key=f"{mc.el_id}.time_limited_by_supply_s",
                    label=f"{model.elements[mc.el_id].label} — time limited by supply",
                    value=mc.limited_s, unit="s"))
            if mc.regen_lost_wh > 0:
                # recuperation its command asked for that the supply could not
                # take (a full or charge-limited battery, a fuel cell, a one-way
                # DC-DC): the motor braked that much less
                summary.append(SummaryValue(
                    key=f"{mc.el_id}.regen_not_recovered_kwh",
                    label=f"{model.elements[mc.el_id].label} — regeneration not recovered",
                    value=mc.regen_lost_wh / 1000.0, unit="kWh"))
        for ec in ctx.engines.values():
            summary.append(SummaryValue(
                key=f"{ec.el_id}.fuel_used_kg",
                label=f"{model.elements[ec.el_id].label} — fuel used",
                value=ec.fuel_used_kg, unit="kg"))
        for fc in ctx.fuelcells.values():
            summary.append(SummaryValue(
                key=f"{fc.el_id}.energy_supplied_kwh",
                label=f"{model.elements[fc.el_id].label} — energy supplied",
                value=fc.energy_wh / 1000.0, unit="kWh"))
        for c_id, (_, cl) in ctx.climate.items():
            label = model.elements[c_id].label
            summary.append(SummaryValue(label=f"{label} — energy used",
                                        value=cl.energy_j / 3.6e6, unit="kWh"))
            if cl.heat_j > 0:
                summary.append(SummaryValue(label=f"{label} — heating delivered",
                                            value=cl.heat_j / 3.6e6, unit="kWh"))
            if cl.cool_j > 0:
                summary.append(SummaryValue(label=f"{label} — cooling delivered",
                                            value=cl.cool_j / 3.6e6, unit="kWh"))
        for vs_id, e_wh in ctx.vsource_energy_wh.items():
            summary.append(SummaryValue(
                key=f"{vs_id}.energy_supplied_kwh",
                label=f"{model.elements[vs_id].label} — energy supplied",
                value=e_wh / 1000.0, unit="kWh"))
        if ctx.veh_id:
            summary.append(SummaryValue(key="distance_km", label="Distance driven", value=ctx.distance / 1000.0, unit="km"))
            net_wh = sum(b.energy_out_wh - b.energy_in_wh for b in ctx.batteries.values())
            if ctx.distance > 100 and net_wh > 0:
                summary.append(SummaryValue(
                    key="consumption_kwh_per_100km", label="Consumption", value=net_wh / 10.0 / (ctx.distance / 1000.0),
                    unit="kWh/100km"))
            fuel_kg = sum(ec.fuel_used_kg for ec in ctx.engines.values())
            density = 0.745  # gasoline default when no tank declares one
            if ctx.distance > 100 and fuel_kg > 0:
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
                    key="fuel_consumption_l_per_100km", label="Fuel consumption",
                    value=liters * 100.0 / (ctx.distance / 1000.0), unit="l/100km"))
                summary.append(SummaryValue(
                    key="co2_g_per_km", label="CO₂ emissions",
                    value=fuel_kg * co2_per_kg * 1000.0 / (ctx.distance / 1000.0),
                    unit="g/km"))
            # at the socket, range, MPGe, charge-corrected fuel, per phase
            # (a charge-balanced hybrid case gets its correction from
            # balance.py, ENG-33)
            lab, lab_base = lab_rows(ctx, model, lablog, density,
                                     balanced=balance.applies(project, case))
            summary += lab
        ctx.close_book()
        if lap is not None and ctx.veh_id:  # its mechanics come from the lap's energy pass
            add_lap(ctx.book, lap.book, ctx.veh_id, model.elements[ctx.veh_id].label)
        released = ctx.book.released_j()
        if lap is None and released > 0:
            # where every part's books together do not close: the flows out
            # of one part that are not the flows into the next (MOD-10)
            summary.append(SummaryValue(
                label="Energy balance residual",
                value=100.0 * ctx.book.residual_j() / released, unit="%"))
        if ctx.throughput_wh > 0:
            # energy no source supplied or absorbed (last-resort clamps), as a
            # share of all the energy that went through the buses
            summary.append(SummaryValue(
                key="energy_balance_error_pct", label="Electrical energy balance error",
                value=100.0 * ctx.residual_wh / ctx.throughput_wh, unit="%"))
        # tables the run went past (listed only then, like the rows above):
        # for how long, as a share of the time solved, and how far; per
        # E-Motor or Engine the time above its maximum speed and the highest
        # speed
        edge_rows: set[str] = {"Energy balance residual"}  # time and speeds, not energy figures
        for use in ctx.map_use:
            if use.outside_s <= 0:
                continue
            label = model.elements[use.el_id].label
            if use.what == "maximum speed":
                share_key = f"{use.el_id}.time_above_max_speed_pct"
                share_label = f"{label} — time above maximum speed"
                far = SummaryValue(key=f"{use.el_id}.highest_speed_rpm",
                                   label=f"{label} — highest speed",
                                   value=use.value, unit="1/min")
            else:
                stem = f"{use.el_id}.outside_{use.table}_{slug(use.axis)}"
                share_key = f"{stem}_time_pct"
                share_label = f"{label} — time outside its {use.what} ({use.axis})"
                far = SummaryValue(key=f"{stem}_furthest",
                                   label=f"{label} — furthest {use.axis} outside its {use.what}",
                                   value=use.value, unit=use.unit)
            edge_rows |= {share_label, far.label}
            summary.append(SummaryValue(
                key=share_key, label=share_label, value=100.0 * use.outside_s / max(solved, 1e-9),
                unit="%"))
            summary.append(far)
        rows = [SummaryValue(key=k, label=label, value=v, unit=u, limit=lim, passed=ok)
                for label, v, u, lim, ok, k in verdict.rows]
        edge_rows |= {r.label for r in rows if r.unit == "%"}  # a test's time shares
        if ctx.full_throttle:  # an acceleration test's own figures come first
            summary[:0] = rows
        else:
            summary += rows
        if lap is not None:  # and so do a lap case's
            summary[:0] = [SummaryValue(key=k, label=label, value=v, unit=u)
                           for label, v, u, k in lap.rows()]
            edge_rows.add("Lap energy balance error")
        summary.append(SummaryValue(key="simulated_duration_s", label="Simulated duration", value=times[-1] if times else 0.0, unit="s"))

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
            for s in summary:  # a check it passed so far, not over the whole run
                if s.passed:
                    not_valid.setdefault(s.label, why)
            if lap is not None:  # the laps it finished, and energy from the one it did not
                for label, *_ in lap.rows():
                    not_valid.setdefault(label, why)
        # a lap case's rows, but its balance error (a check of the solver)
        lap_rows = ([label for label, *_ in lap.rows() if label != "Lap energy balance error"]
                    if lap is not None else [])
        if lap is not None and lap.lap_times:
            error = lap.balance_pct()
            if any(b.depleted_flagged for b in ctx.batteries.values()):
                for label in lap_rows:  # the laps were solved with power it no longer had
                    not_valid.setdefault(label, "the battery reached its minimum SOC")
            if lap.book.brake_short_s > 0:  # the laps were solved with braking it did not have
                for label in lap_rows:
                    not_valid.setdefault(label, "the brakes could not follow the lap's speed")
                rt.message("warning", f"The friction brakes and the regeneration could not slow "
                                      f"the car as fast as the lap's speed asks, for "
                                      f"{lap.book.brake_short_s:.2f} s of the laps "
                                      f"({lap.book.brake_short / 3600.0:.2f} Wh of braking "
                                      f"missing): the brakes' Max Torque, or the regeneration "
                                      f"the battery could take, held them back. The lap times "
                                      f"are optimistic and not valid; raise the brakes' Max "
                                      f"Torque if the car's brakes are stronger.")
            if abs(error) > lapsim.BALANCE_PCT:
                not_valid.setdefault("Energy per lap", "the lap energy balance does not close")
                # the motors gave less than the speed trace asked for, for at
                # least half the error: the lap is slower than its times say
                short = lap.book.shortfall > 0.005 * abs(error) * abs(lap.source_net_j())
                if short:
                    for label in lap_rows:
                        not_valid.setdefault(label, "the motors fell short of the lap's speed")
                short_text = (f"; the motors gave {lap.book.shortfall / 3600.0:.1f} Wh less than "
                              f"the speed trace asked for (their supply or the battery voltage "
                              f"held them back more than the lap solver expected), so the lap "
                              f"times are optimistic and not valid" if short else "")
                rt.message("warning", f"Lap energy balance: the energy the laps took (kinetic, "
                                      f"road load, slope, brakes, gear and motor losses, "
                                      f"consumers) differs from what the sources gave by "
                                      f"{error:+.2f} %, more than {lapsim.BALANCE_PCT:g} %"
                                      f"{short_text}. The Energy per lap is not valid.")
        if verdict.beyond_reason:
            # a machine or source ran past its data: what depends on how it ran
            for label in ("Consumption", "Fuel consumption", "CO₂ emissions",
                          *(row[0] for row in verdict.rows), *lap_rows):
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
            if s.notValid is None and s.label in lab_base:  # shares its base row's validity
                s.notValid = not_valid.get(lab_base[s.label])
        event = case.fsEvent
        if event and case.kind != ("acceleration" if event == "acceleration" else "lap") \
                and not (event == "endurance" and case.kind == "cycle"):
            rt.message("info", f"The case stands for the Formula Student "
                               f"{fs_events.NAMES[event]} event, but its kind is "
                               f"{case.kind.capitalize()}: the event is scored from "
                               f"{'an Acceleration' if event == 'acceleration' else 'a Lap'} "
                               f"case only, so this run has no event rows.")
        elif event and times:
            rows_ev, msgs_ev = fs_events.event_rows(
                case, ctx, lap, summary, finished=not stopped and failed_at is None)
            summary[:0] = [SummaryValue(label=label, value=v, unit=u, limit=lim, passed=ok,
                                        notValid=nv) for label, v, u, lim, ok, nv in rows_ev]
            for level, text in msgs_ev:
                rt.message(level, text)

        has_error = any(m.level == "error" for m in rt.messages)
        has_warning = any(m.level == "warning" for m in rt.messages)
        status = ("failed" if has_error else "cancelled" if stopped
                  else "warning" if has_warning else "success")
        rec_note = f", stored every {output_every}" if output_every > 1 else ""
        last_note = f", the last one {h_last:g} s" if steps and short_last else ""
        n_steps = f"{steps}"
        if arrived:
            n_steps = f"{step} of {steps}"  # the steps solved up to the line
            last_note = f", ended at {end_d:g} m driven at t = {times[-1]:g} s"
        text = (f"Case '{case.name}' solved: {n_steps} steps × {dt_rec:g} s{last_note} "
                f"({n_sub} sub-steps each), {len(times)} points recorded{rec_note}, "
                f"{len(channels)} result channels.")
        if lap is not None:
            tr = lap.track
            text = (f"Case '{case.name}' solved in lap mode: {lap.laps} lap"
                    f"{'s' if lap.laps > 1 else ''} of the Race Track's {tr.name} layout "
                    f"({tr.length:,.0f} m, a point every {tr.ds:.2f} m), {len(times)} points "
                    f"recorded{rec_note}, {len(channels)} result channels.")
        rt.messages.insert(0, SimMessage(level="info", text=text))
        if totals is not None:
            totals.update(balance.run_totals(ctx, soc_start))
        energy, duty, limits = recorder.finish(lap)
        return SimResult(
            caseId=case_id,
            status=status,
            messages=rt.messages,
            channels=channels,
            summary=summary,
            partEnergy=energy_flows(ctx.book),
            energy=energy,
            duty=duty,
            limits=limits,
            references=check_references(case.references, summary) + hand_checks(ctx, channels),
        )
    finally:
        ctx.close_sandboxes()


def _laps_distance(ctx: RunContext, laps: float) -> Optional[float]:
    """The distance ``laps`` passes through the profile of the Driving Task
    the Driver follows take, m; None when the Driver follows no Driving Task
    over distance (or its profile has no length)."""
    model = ctx.model
    src = model.signal_route.get((model.driver, "sig_target_in")) if model.driver else None
    if src is None or model.cdef_of[src[0]].id != "signal.driving_task" \
            or not distance_axis(ctx.params(src[0])):
        return None
    length = lap_length(ctx.profile_points(src[0]))
    return laps * length if length > 0 else None


class _Envelope:
    """Each channel's lowest, highest and time-averaged value over every
    output interval (ENG-16), from its value at the end of each solver step,
    so a peak shorter than the interval between recorded points (a regen
    burst, a torque spike) is kept. A point's interval runs from the point
    before it; point 0, the initial state, is its own value.

    Each solver step only appends its values to a buffer; the buffer is
    reduced (min, max and the time-weighted mean, in C) when a point is
    recorded or the drivelines change."""

    def __init__(self, ctx: RunContext, gear_of: dict[str, float]):
        self.ctx, self.gear_of = ctx, gear_of
        self.bus_keys = [(el, port) for el, port, _ in _bus_channels(ctx)]
        # the bus channels in one call, once every one has a value (until
        # then a channel with none reads 0.0)
        self.bus_read: Optional[Callable[[dict], tuple]] = None
        self.layout = -1
        self.keys: list[ChannelKey] = []  # the buffered rows' channels
        self.fns: list[Callable[[], tuple | list]] = []  # the state channels' getters
        self.rows: list[list[Optional[float]]] = []
        self.hs: list[float] = []
        self.out: dict[tuple[str, str], tuple[list, list, list]] = {}
        self.acc: dict[tuple[str, str], list[float]] = {}  # [lo, hi, ∫v dt, span]

    def add(self, h: float) -> None:
        """Buffer the channels' values at the end of a solver step of ``h`` s."""
        ctx = self.ctx
        if self.layout != ctx.layout_version:
            self._fold()
            self.layout = ctx.layout_version
            # a channel worked out once per recorded point keeps its recorded
            # value as its envelope (record): too costly for every step
            groups = [g for g in _state_channel_groups(ctx, self.gear_of) if not g[2]]
            self.keys = self.bus_keys + [key for keys, _, _ in groups for key in keys]
            self.fns = [fn for _, fn, _ in groups]
        bus = ctx.rt.signal_values
        if self.bus_read is not None:
            row = list(self.bus_read(bus))
        else:
            get = bus.get
            row = [get(k, 0.0) for k in self.bus_keys]
        for fn in self.fns:
            row += fn()
        self.rows.append(row)
        self.hs.append(h)

    def _fold(self) -> None:
        """Reduce the buffered rows into the interval's accumulators."""
        if not self.rows:
            return
        hs, acc, mul = self.hs, self.acc, operator.mul
        span_all = sum(hs)  # a full column's span
        for key, col in zip(self.keys, zip(*self.rows)):
            # min() raises TypeError on two or more values with a None among
            # them, so a full column (the usual case) is not scanned for None
            try:
                lo, hi = min(col), max(col)
            except TypeError:
                lo = None
            if lo is not None:
                area, span = sum(map(mul, col, hs)), span_all
            else:  # no data yet in part of the interval
                pairs = [(v, h) for v, h in zip(col, hs) if v is not None]
                if not pairs:
                    continue
                col, h_col = tuple(v for v, _ in pairs), [h for _, h in pairs]
                lo, hi = min(col), max(col)
                area, span = sum(map(mul, col, h_col)), sum(h_col)
            a = acc.get(key)
            if a is None:
                acc[key] = [lo, hi, area, span]
            else:
                a[0], a[1] = min(a[0], lo), max(a[1], hi)
                a[2] += area
                a[3] += span
        self.rows, self.hs = [], []

    def record(self, key: tuple[str, str], index: int, value: float) -> None:
        """Store the channel's envelope for point ``index``, recorded with
        ``value``; call reset() once every channel of the point is stored."""
        self._fold()
        lists = self.out.get(key)
        if lists is None:
            lists = self.out[key] = ([], [], [])
        for lst in lists:
            while len(lst) < index:
                lst.append(None)  # no data yet, as in the channel
        a = self.acc.get(key)
        if a is not None and a[3] > 0:
            lo, hi, mean = min(a[0], value), max(a[1], value), a[2] / a[3]
        else:
            lo = hi = mean = value
        lists[0].append(lo)
        lists[1].append(hi)
        lists[2].append(mean)

    def reset(self) -> None:
        """Start the next output interval."""
        self.acc = {}
        keys = self.bus_keys
        if self.bus_read is None and len(keys) > 1:  # (the bus never drops a value)
            bus = self.ctx.rt.signal_values
            if all(k in bus for k in keys):
                self.bus_read = operator.itemgetter(*keys)

    def series(self, el_id: str, port_id: str, n: int) -> tuple[list, list, list]:
        lists = self.out.get((el_id, port_id), ([], [], []))
        return tuple(lst + [None] * (n - len(lst)) for lst in lists)  # type: ignore[return-value]


ChannelValue = tuple[str, str, float]  # (element id, port id, value)


def _bus_channels(ctx: RunContext) -> Iterator[ChannelValue]:
    """Recorded channels the slaves already publish on the signal bus
    (signal blocks, vehicle, driver, electrical consumers, brakes)."""
    model, bus = ctx.model, ctx.rt.signal_values
    for el_id, cdef in model.cdef_of.items():
        tdef = cdef.id
        if tdef in ("signal.constant", "control.pid", "signal.lookup", "control.traction"):
            ports: tuple[str, ...] = ("sig_out",)
        elif tdef == "signal.driving_task":
            ports = ("sig_demand",)
        elif tdef in ("signal.script", "signal.fmu"):
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
        elif tdef == "electric.climate":
            ports = ("sig_power", "sig_heat", "sig_cop")
        elif tdef == "electric.voltage_source":
            ports = ("sig_power", "sig_voltage")
        elif tdef == "mech.brake":
            ports = ("sig_torque",)
        elif tdef == "track.lap" and ctx.lap is not None:  # recorded in lap cases only
            ports = lapsim.TRACK_PORTS
        else:
            continue
        for port_id in ports:
            yield el_id, port_id, bus.get((el_id, port_id), 0.0)


ChannelKey = tuple[str, str]  # (element id, port id)
# a getter of several channels' values in one call (an element's, or a
# driveline segment's, which share their reads), None = no data yet: (the
# channels, the getter, whether it is worked out only once per recorded point)
ChannelGroup = tuple[tuple[ChannelKey, ...], Callable[[], "tuple | list"], bool]


_N_LOAD = operator.attrgetter("n_load")


def _keys(el_id: str, *ports: str) -> tuple[ChannelKey, ...]:
    return tuple((el_id, port) for port in ports)


def _state_channel_groups(ctx: RunContext, gear_of: dict[str, float]) -> Iterator[ChannelGroup]:
    """The state channels as getters, so a caller can keep them and read them
    again: valid until the drivelines change (``ctx.layout_version``). A
    group marked per point is too costly to work out at every solver step."""
    model = ctx.model
    for el_id, cdef in model.cdef_of.items():
        tdef = cdef.id
        if tdef == "battery.generic" and el_id in ctx.batteries:
            b = ctx.batteries[el_id]
            # (its losses: what the cells give up less what reaches the terminals)
            yield _keys(el_id, "sig_soc", "sig_voltage", "sig_current", "sig_power",
                        "sig_losses"), (
                lambda b=b: (b.soc * 100.0, b.v_term, b.current, b.power_w / 1000.0,
                             (b.chem_w - b.power_w) / 1000.0)), False
            cp = b.cells
            if cp is not None:  # its BMS's limits (MOD-08)
                yield _keys(el_id, "sig_i_dis_limit", "sig_i_ch_limit"), (
                    lambda b=b: (b.i_dis_lim if b.i_dis_lim < math.inf else None,
                                 b.i_ch_lim if b.i_ch_lim < math.inf else None)), False
            if cp is not None and cp.cells:
                yield _keys(el_id, "sig_v_cell_min", "sig_v_cell_max"), (
                    lambda cp=cp: (cp.v_cell[0], cp.v_cell[1])), False
                ports = [port for d in SOP_PULSES
                         for port in (f"sig_p_dis_{d:g}s", f"sig_p_ch_{d:g}s")]

                def state_of_power(b=b, n=len(SOP_PULSES)) -> tuple:
                    sp = _state_of_power(ctx, b)
                    return tuple(sp[k][side] / 1000.0 for k in range(n) for side in (0, 1))
                yield _keys(el_id, *ports), state_of_power, True
        elif tdef == "motor.emotor" and el_id in ctx.motors:
            mc = ctx.motors[el_id]
            yield _keys(el_id, "sig_speed", "sig_torque", "sig_mech_power", "sig_elec_power",
                        "sig_losses"), (
                lambda mc=mc: (mc.rpm, mc.torque, mc.p_mech_w / 1000.0, mc.p_elec_w / 1000.0,
                               mc.p_loss_w / 1000.0)), False
        elif tdef == "engine.combustion" and el_id in ctx.engines:
            ec = ctx.engines[el_id]
            yield _keys(el_id, "sig_speed", "sig_torque", "sig_fuel_rate", "sig_power",
                        "sig_fuel_power", "sig_losses"), (
                lambda ec=ec: (ec.rpm, ec.torque, ec.fuel_kgh, ec.p_mech_w / 1000.0,
                               ec.fuel_kgh / 3600.0 * ctx.fuel_lhv / 1000.0,
                               (ec.fuel_kgh / 3600.0 * ctx.fuel_lhv - ec.p_mech_w) / 1000.0)), False
        elif tdef == "fuelcell.stack" and el_id in ctx.fuelcells:
            fc = ctx.fuelcells[el_id]
            yield _keys(el_id, "sig_voltage", "sig_current", "sig_power", "sig_h2_rate"), (
                lambda fc=fc: (fc.voltage, fc.current, fc.power_w / 1000.0, fc.h2_kgh)), False
        elif tdef in ("fuel.tank", "fuel.h2_tank") and el_id in ctx.tanks:
            tk = ctx.tanks[el_id]
            yield _keys(el_id, "sig_level", "sig_mass"), (
                lambda tk=tk: (100.0 * tk.mass_kg / tk.capacity_kg, tk.mass_kg)), False
        elif tdef == "vehicle.body" and el_id == ctx.veh_id:
            # (the axle loads summed in C: the same sums, sooner)
            if ctx.lap is None:  # and the road load's terms
                yield _keys(el_id, "sig_load_front", "sig_load_rear", "sig_p_aero",
                            "sig_p_roll", "sig_p_grade", "sig_p_accel"), (
                    lambda: (sum(map(_N_LOAD, ctx.axle_wheels[0])),
                             sum(map(_N_LOAD, ctx.axle_wheels[1])),
                             ctx.road_w[0] / 1000.0, ctx.road_w[1] / 1000.0,
                             ctx.road_w[2] / 1000.0, ctx.road_w[3] / 1000.0)), False
            else:
                yield _keys(el_id, "sig_load_front", "sig_load_rear"), (
                    lambda: (sum(map(_N_LOAD, ctx.axle_wheels[0])),
                             sum(map(_N_LOAD, ctx.axle_wheels[1])))), False
        if tdef in FLOW_PORTS:  # from the energy book: no data until the part is first booked
            # a driveline's gears and clutches have their powers worked out
            # per recorded point (gear_powers); brakes and stacks every step
            gears = tdef not in ("mech.brake", "fuelcell.stack")
            yield (_keys(el_id, *(port_id for port_id, _ in FLOW_PORTS[tdef])),
                   _flow_getter(ctx, el_id, [attr for _, attr in FLOW_PORTS[tdef]], gears), gears)
        if tdef == "controller.dcdc":  # no data until the converter first runs
            yield _keys(el_id, "sig_power_in", "sig_power_out", "sig_losses"), (
                lambda el_id=el_id: _dcdc_kw(ctx, el_id)), False

    for st in ctx.dls:
        plan = st.plan
        for j in st.dl.joints:
            if j.kind == "split":
                yield _keys(j.el_id, "sig_torque_a", "sig_torque_b", "sig_speed_in"), (
                    lambda st=st, j=j: (st.joint_torque_a.get(j.el_id, 0.0),
                                        st.joint_torque_b.get(j.el_id, 0.0),
                                        abs(st.joint_speed_in.get(j.el_id, 0.0)) * RPM)), False
            else:
                yield _keys(j.el_id, "sig_torque", "sig_slip_speed"), (
                    lambda st=st, j=j: (st.clutch_torque.get(j.el_id, 0.0),
                                        st.clutch_slip.get(j.el_id, 0.0) * RPM)), False
        if plan.over_constrained or not plan.n:
            continue
        losses = ctx.lap is None and bool(ctx.veh_id)  # a wheel's tyre force × its slip speed
        for s_idx, seg in enumerate(st.dl.segments):
            # the segment's wheels, its nodes', final drives' and gearboxes'
            # output speeds (and gear) and its propellers, at its speed
            keys: list[ChannelKey] = []
            for w in seg.wheels:
                keys += _keys(w.el_id, "sig_speed", "sig_slip", "sig_force", "sig_torque",
                              "sig_normal_load", *(("sig_slip_losses",) if losses else ()))
            shafts = []  # (ratio, the gearbox's id or None)
            for el_id2, m2 in seg.element_ms.items():
                tdef2 = model.cdef_of[el_id2].id
                if tdef2 == "mech.node":
                    keys.append((el_id2, "sig_speed"))
                elif tdef2 == "mech.final_drive":
                    keys.append((el_id2, "sig_speed_out"))
                elif tdef2 == "mech.gearbox":
                    keys += _keys(el_id2, "sig_speed_out", "sig_gear")
                else:
                    continue
                shafts.append((m2, el_id2 if tdef2 == "mech.gearbox" else None))
            for pr in seg.props:
                keys += _keys(pr.el_id, "sig_speed", "sig_shaft_power")
            if not keys:
                continue

            def segment(st=st, s_idx=s_idx, wheels=tuple(seg.wheels), shafts=tuple(shafts),
                        props=tuple(seg.props), losses=losses) -> list:
                o = ctx.seg_speed(st, s_idx)
                values: list = []
                for w in wheels:
                    omega_w = w.m * o
                    force = ctx.last_forces.get(w.el_id, 0.0)
                    v_den = max(abs(ctx.v), V_EPS)
                    values += (abs(omega_w) * RPM,
                               (omega_w * w.radius - ctx.v) / v_den if ctx.veh_id else 0.0,
                               force, force * w.radius, w.n_load)
                    if losses:
                        values.append(ctx.wheel_end_force.get(w.el_id, 0.0)
                                      * (w.radius * w.m * o - ctx.v_mid) / 1000.0)
                for m2, gearbox in shafts:
                    values.append(abs(m2 * o) * RPM)
                    if gearbox is not None:
                        g = gear_of.get(gearbox)  # (its default worked out only when unset)
                        values.append(g if g is not None else
                                      float(ctx.params(gearbox).get("default_gear", 1) or 1))
                for pr in props:
                    omega_p = pr.m * o
                    rpm_p = abs(omega_p) * RPM
                    values += (rpm_p, abs(pr.t_ref * (rpm_p / pr.n_ref) ** 2 * omega_p) / 1000.0)
                return values
            yield tuple(keys), segment, False


# channels read from the energy book (MOD-10): (port, Flow attribute, W)
FLOW_PORTS: dict[str, tuple[tuple[str, str], ...]] = {
    **{t: (("sig_power", "p_w"), ("sig_losses", "p_loss_w"))
       for t in ("mech.shaft", "mech.final_drive", "mech.gearbox", "mech.differential",
                 "mech.transfer_case")},
    "mech.clutch": (("sig_losses", "p_loss_w"),),
    "mech.brake": (("sig_power", "p_in_w"),),
    "fuelcell.stack": (("sig_losses", "p_loss_w"),),
}


def _flow_getter(ctx: RunContext, el_id: str, attrs: list[str],
                 gears: bool) -> Callable[[], "tuple | list"]:
    """The getter of a part's channels from the energy book (_flow_kw)."""
    if len(attrs) == 1:  # (a brake's: read every solver step)
        attr = attrs[0]
        return lambda: (_flow_kw(ctx, el_id, attr, gears),)
    return lambda: [_flow_kw(ctx, el_id, attr, gears) for attr in attrs]


def _flow_kw(ctx: RunContext, el_id: str, attr: str, gears: bool = True) -> Optional[float]:
    if gears:
        ctx.gear_powers()
    p = ctx.book.power(el_id, attr)
    if p is None:  # not booked yet: 0 at point 0, as computed values read there
        return None if ctx.lap is not None else 0.0
    return p / 1000.0


def _state_of_power(ctx: RunContext, b) -> list[tuple[float, float]]:
    """A battery built from cells: its state of power now (battery.sop),
    worked out once per recorded point."""
    key = (ctx.book.n, b.soc, b.v_rc)
    if b.cells.sop_at[0] != key:
        b.cells.sop_at = (key, sop(b.cells, b.soc, b.min_soc, ctx.ambient_c(), b.ocv(), b.v_rc,
                                   b.r1, b.tau))
    return b.cells.sop_at[1]


def _dcdc_kw(ctx: RunContext, el_id: str) -> tuple[Optional[float], ...]:
    """A DC-DC converter's power in, out and its losses, kW (None until it
    first runs)."""
    flows = ctx.dcdc_flows.get(el_id)
    if flows is None:
        return None, None, None
    p_in, p_out = flows
    return p_in / 1000.0, p_out / 1000.0, (p_in - p_out) / 1000.0
