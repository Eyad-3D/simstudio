"""Running a case on the new engine, behind a flag (LIGHTSIM_ENGINE).

``LIGHTSIM_ENGINE=new`` makes ``simulate`` (core.py) try the new engine, the
Rust module ``lightsim_engine``, for the cases it can run: a drive cycle or an
acceleration test. Everything else, and any failure of the new engine, runs on
today's engine, and the result's messages say which engine ran the case and,
after a fall-back, why, with a stable reason code in brackets (``[NEW-LAP]``;
CODES lists them, and benchmarks/back_to_back counts them). With
``LIGHTSIM_ENGINE_STRICT=1`` as well nothing falls back: a case the new
engine's path does not run fails with its code instead (the both-engines
diff's strict mode). Without the flag (or with ``today``) nothing here is
used.

The new engine's run is turned into today's whole SimResult: its channels
and figures are already in today's names and units; this module adds the
channels' labels and units, and gathers the run's evidence (evidence.py) for
today's own code to judge and report: the verdict (verdict.judge, the
battery checks on the engine's moving average of each battery's power over
every step, each machine's time above its maximum speed, the tables run
past that today's engine reads, today's warnings of a supply limit, the
rows a failed check makes not valid, the status), the
part energies and the energy report (from the engine's per-part energy
books), the duty (from the engine's integrals over every step it took), what
held each driveline back (today's rule on a trace of the run from its dense
output, each change at the run's own events) and the references with the
hand calculations. The engine's own warnings on its run reach the user; its
preparation's notes about the start values (START_NOTES, review finding 36)
are notes, never the run's warnings.
"""
from __future__ import annotations

import bisect
import math
import os
from typing import Callable, NamedTuple, Optional

from .. import cycles, script_trust
from ..library import unit_groups
from ..schemas import Channel, Project, SimMessage, SimResult, SummaryValue
from . import balance
from .battery import CELLS
from .domains import ModelInitError, RunContext
from .energy import BookFlow, energy_flows
from .evidence import Evidence
from .keys import fill_keys
from .labfig import lab_base
from .maps import interp1
from .network import CONSUMER_TYPES, ModelError, build_model
from .profiles import distance_axis
from .references import check_references, hand_checks
from .reports import DUTY_QUANTITIES, RunRecorder, lane_state, make_reports
from .runtime import SPEED_LIMIT_BAND, ControlFn, EmitFn, Runtime, usable_energy_left_wh
from .sandbox import ScriptSandbox, ScriptSpec
from .verdict import TRACE_STEP_S, Trace, judge, terminal_checks

ENV_VAR = "LIGHTSIM_ENGINE"
STRICT_VAR = "LIGHTSIM_ENGINE_STRICT"
KINDS = ("cycle", "acceleration")  # the case kinds the new engine runs

# why the new engine's path does not run a case (benchmarks/back_to_back/acceptance.toml)
CODES = {
    "NEW-NOT-FOUND": "the case is not in the project",
    "NEW-CASE-KIND": "a case kind the new engine's path does not run",
    "NEW-LAP": "a lap case",
    "NEW-LIVE": "a live (streamed or paced) run",
    "NEW-FS-EVENT": "a Formula Student event case",
    "NEW-FMU": "a project with an FMU part",
    "NEW-BALANCE": "a hybrid's charge-balanced case",
    "NEW-CELLS": "a battery built from cells",
    "NEW-MISSING": "the module lightsim_engine is not installed",
    "NEW-BUILD": "today's model builder refused the model",
    "NEW-FAILED": "the new engine refused or stopped (its own codes follow)",
    "NEW-ERROR": "an exception that is not the engine's own refusal",
}

TodayFn = Callable[[Project, str, Optional[EmitFn], Optional[ControlFn]], SimResult]


class Refusal(NamedTuple):
    """Why the new engine's path did not run a case: a code of CODES and
    the reason in words."""
    code: str
    why: str


def wanted() -> bool:
    """Whether the flag asks for the new engine (read at each run)."""
    return os.environ.get(ENV_VAR, "today").strip().lower() == "new"


def strict() -> bool:
    """Whether nothing may fall back to today's engine (read at each run)."""
    return os.environ.get(STRICT_VAR, "").strip() == "1"


def simulate(project: Project, case_id: str, emit: Optional[EmitFn],
             control: Optional[ControlFn], today: TodayFn) -> SimResult:
    """Run the case on the new engine when it can, otherwise on ``today``'s
    engine, with a message saying which ran it (in strict mode not at all:
    the case fails with the reason's code)."""
    why = unsupported(project, case_id, emit)
    if why is None:
        try:
            result = run_new(project, case_id)
            result.messages.insert(0, SimMessage(
                level="info", text=f"Run on the new engine ({ENV_VAR}=new)."))
            return result
        except Exception as e:  # noqa: BLE001 — the new engine must never break a run
            why = failure(e)
            if strict():
                return refused(case_id, why, str(e))
            result = today(project, case_id, emit, control)
            result.messages.insert(0, SimMessage(
                level="warning", text=f"The new engine could not run this case [{why.code}] "
                                      f"({e}); it ran on today's engine instead."))
            _refresh_status(result)
            return result
    if strict():
        return refused(case_id, why)
    result = today(project, case_id, emit, control)
    result.messages.insert(0, SimMessage(
        level="info", text=f"Run on today's engine, not the new one ({ENV_VAR}=new) "
                           f"[{why.code}]: {why.why}."))
    return result


def failure(e: Exception) -> Refusal:
    """The reason code of an exception out of ``run_new``: the new engine's
    own refusal (its diagnostics' codes in the words), today's model
    builder's, or anything else (a fault of this path)."""
    try:
        from lightsim_engine import ModelError as EngineError
    except ImportError:
        EngineError = ()  # noqa: N806 — nothing is the engine's without it
    if isinstance(e, EngineError):
        codes = sorted({str(d.get("code")) for d in getattr(e, "diagnostics", None) or []})
        return Refusal("NEW-FAILED", f"the new engine refused it ({', '.join(codes) or 'no code'})")
    if isinstance(e, ModelError):
        return Refusal("NEW-BUILD", "today's model builder refused it")
    return Refusal("NEW-ERROR", f"{type(e).__name__} in the new engine's path")


def refused(case_id: str, why: Refusal, detail: str = "") -> SimResult:
    """Strict mode's result for a case the new engine's path did not run."""
    more = f" ({detail})" if detail else ""
    return SimResult(caseId=case_id, status="failed", channels=[], messages=[SimMessage(
        level="error", text=f"Not run [{why.code}]: {why.why}{more}; with {STRICT_VAR}=1 no "
                            f"case runs on today's engine instead.")])


def unsupported(project: Project, case_id: str, emit: Optional[EmitFn]) -> Optional[Refusal]:
    """Why the new engine does not run this case, or None when it does."""
    case = next((c for c in project.cases if c.id == case_id), None)
    if case is None:
        return Refusal("NEW-NOT-FOUND", "the case was not found")
    if case.kind == "lap":
        return Refusal("NEW-LAP", "it does not run lap cases")
    if case.kind not in KINDS:
        return Refusal("NEW-CASE-KIND", f"it does not run {case.kind} cases yet")
    if emit is not None or (case.realtimeFactor or 0) > 0:
        return Refusal("NEW-LIVE", "it does not stream a live run yet")
    if case.fsEvent:
        return Refusal("NEW-FS-EVENT", "it does not score Formula Student events")
    elements = [el for s in project.systems for el in s.elements]
    if any(el.componentDefId == "signal.fmu" for el in elements):
        return Refusal("NEW-FMU", "it does not run FMU parts yet")
    if balance.applies(project, case):
        return Refusal("NEW-BALANCE", "it does not charge-balance a hybrid's case")
    overrides = getattr(case, "parameterOverrides", None) or {}
    if any(el.componentDefId == "battery.generic"
           and {**el.parameterOverrides, **overrides.get(el.id, {})}.get("pack_model") == CELLS
           for el in elements):
        return Refusal("NEW-CELLS", "it does not run a battery built from cells")
    try:
        import lightsim_engine  # noqa: F401
    except ImportError:
        return Refusal("NEW-MISSING", "the new engine is not installed")
    return None


# the gears and shafts: a part whose input power is its channel sig_power
_GEARS = ("mech.shaft", "mech.final_drive", "mech.gearbox", "mech.differential",
          "mech.transfer_case")


def run_new(project: Project, case_id: str) -> SimResult:
    """The case on the new engine, as today's SimResult. Raises when the new
    engine cannot build or run it."""
    import lightsim_engine

    from . import core  # (core imports this module)

    case = next(c for c in project.cases if c.id == case_id)
    overrides = getattr(case, "parameterOverrides", None) or {}
    # today's model gives what the engine's run lacks: the labels and units of
    # the channels, the Script blocks' code, ports and parameters, and, in a
    # RunContext built and never stepped, the parts as today's engine reads
    # them (each battery's capacity and checks, each motor's maximum speed,
    # each driveline's parts, each table's axes, each tank's start)
    model = build_model(project, {}, overrides)
    try:
        ctx = RunContext(project, model, Runtime(model, None), {}, overrides, workers=False)
    except ModelInitError as e:  # as today's engine says it
        return SimResult(caseId=case_id, status="failed", channels=[],
                         messages=[SimMessage(level="error", text=t) for t in e.messages])
    ctx.performance = case.kind != "cycle"
    ctx.full_throttle = case.kind == "acceleration"
    t_end = max(0.0, float(case.duration))
    ctx.end_distance, laps_warning = core.end_distance(ctx, case, t_end)
    lanes = RunRecorder(ctx).lanes
    built = lightsim_engine.build(project.model_dump_json(), case_id, cycles_dir=str(cycles.DIR))
    plan = _Plan(model, ctx, built, lanes)
    step = ACCEL_TRACE_STEP_S if ctx.performance else TRACE_STEP_S
    specs = [
        ScriptSpec(
            el_id=el_id,
            label=model.elements[el_id].label,
            code=str(model.params_of[el_id].get("code", "")),
            input_keys=[p.id for p in (model.elements[el_id].dynamicPorts or [])
                        if p.direction == "input"],
            params=dict(model.params_of[el_id]),
        )
        for el_id in model.signal_blocks if model.cdef_of[el_id].id == "signal.script"
    ]
    dropped: dict[tuple[str, str], str] = {}  # (Script, key): today's warning, once
    if not specs:
        run = built.simulate(trace=plan.names, trace_step=step, windows=plan.windows)
    else:
        # the same checks as today's engine: only approved code runs, and it
        # runs in the locked-down worker process (script_trust.py, sandbox.py);
        # the engine's own Script runner is not used from the app until the
        # owner has decided it may replace the worker (native_scripts=False)
        try:
            script_trust.check([(sp.label, sp.code) for sp in specs])
        except script_trust.ScriptsNotApproved as e:  # as today's engine says it
            return SimResult(caseId=case_id, status="failed", channels=[],
                             messages=[SimMessage(level="error", text=str(e))])
        with ScriptSandbox(specs) as sandbox:
            def runner(spec: ScriptSpec):
                ports = {p.id for p in model.elements[spec.el_id].dynamicPorts or []
                         if p.direction == "output"}

                def step_fn(t, dt, inputs):
                    outs = sandbox.run(spec.el_id, spec.label, t, dt, dict(inputs), spec.params)
                    for key in outs or {}:  # (the engine takes only its ports, as today's does)
                        if key not in ports:
                            dropped.setdefault((spec.el_id, key), (
                                f"Script '{spec.label}' returned '{key}' which is not one of its "
                                f"output ports — value dropped."))
                    return outs
                return step_fn
            run = built.simulate(scripts={sp.el_id: runner(sp) for sp in specs},
                                 native_scripts=False, trace=plan.names, trace_step=step,
                                 windows=plan.windows)
    return _result(project, case, model, ctx, built, run, plan, lanes, laps_warning,
                   list(dropped.values()))


ACCEL_TRACE_STEP_S = 0.01  # the trace of a test that is not a cycle (today: every solver step)

# the engine's preparation notes about the start (the start-guess note of
# review finding 36): about the start values, not about the run, so shown
# as notes, never as the run's warnings
START_NOTES = ("START-ALIAS-CONFLICT", "START-NOT-SOLVED", "INIT-START-ASSUMED",
               "INIT-UNDECIDED", "INIT-START-IGNORED")


class _Plan:
    """What the run's trace samples from the dense output (each name the
    engine has): the Driver's target, the vehicle's speed and distance, and
    what the limit lanes read (the driver's commands, each motor's demand,
    speed and time held below its command, its battery's time at its power
    cap, each engine's torque and speed, each driven wheel at its grip)."""

    def __init__(self, model, ctx, built, lanes):
        self.inst = built.report["instances"]
        engine_vars, channels = built.var_names()
        have = set(engine_vars) | set(channels)
        self.names: list[str] = []

        def want(name: Optional[str]) -> Optional[str]:
            if name is None or name not in have:
                return None
            if name not in self.names:
                self.names.append(name)
            return name

        def var(el_id: str, v: str) -> Optional[str]:
            return want(f"{self.inst[el_id]}.{v}") if el_id in self.inst else None

        self.src = (model.signal_route.get((model.driver, "sig_target_in"))
                    if model.driver and ctx.veh_id else None)
        self.target = want(f"{self.src[0]}:{self.src[1]}") if self.src else None
        self.speed = want(f"{ctx.veh_id}:sig_speed") if ctx.veh_id else None
        self.dist = var(ctx.veh_id, "dist") if ctx.veh_id else None
        drv = model.driver
        self.brake = want(f"{drv}:sig_brake_cmd") if drv else None
        self.traction = want(f"{drv}:sig_traction_cmd") if drv else None
        self.lane_parts = []
        for lane in lanes:
            motors = [(mc, want(f"{key[0]}:{key[1]}") if key else None,
                       want(f"{mc.el_id}:sig_speed"), var(mc.el_id, "t_limited"), bat,
                       var(bat.el_id, "t_held") if bat is not None else None)
                      for mc, key, bat in lane.motors]
            engines = [(ec, want(f"{ec.el_id}:sig_torque"), want(f"{ec.el_id}:sig_speed"))
                       for ec in lane.engines]
            wheels = [var(w.el_id, "at_grip") for w in lane.wheels]
            self.lane_parts.append((motors, engines, [w for w in wheels if w]))
        # each mechanical part's ports (a flange's torque and speed, a road
        # contact's force and speed): the power through each, which the
        # engine's books keep only summed over the part, for its flows each
        # way (_flows)
        self.ports: dict[str, list[tuple[str, str, str]]] = {}
        for el_id, cdef in model.cdef_of.items():
            if (cdef.id not in _MECH and cdef.id != "vehicle.body") or el_id not in self.inst:
                continue
            stem = self.inst[el_id] + "."
            for name in engine_vars:
                if not name.startswith(stem) or name.count(".") != 2:
                    continue
                port, q = name[len(stem):].split(".")
                speed = {"tau": "w", "f": "v"}.get(q)
                if speed and f"{stem}{port}.{speed}" in have:
                    self.ports.setdefault(el_id, []).append(
                        (port, want(name), want(f"{stem}{port}.{speed}")))
        # each E-Motor's and Engine's speed: its time above its maximum
        # speed (_over_speed)
        self.speeds = {el: want(f"{el}:sig_speed") for el in (*ctx.motors, *ctx.engines)}
        # what says which supply limit held a motor back or cut a consumer
        # back (_supply): each motor's time held and recuperation lost,
        # each battery's state of charge, discharge current limits, charge
        # window and time at its cap, each consumer's share served and
        # demand
        self.supply = {el: {v: var(el, v) for v in vs} for el, vs in (
            *((m, ("t_limited", "e_regen_lost")) for m in ctx.motors),
            *((b, ("soc", "i_mpp", "i_dis_lim", "p_absorb", "t_held")) for b in ctx.batteries),
            *((c, ("served", "p_dem")) for c, cd in model.cdef_of.items() if cd.id in CONSUMER_TYPES))}
        # each checked battery's power: its highest average over its check's
        # window, from every point the integrator visited (_terminal)
        self.windows = {f"{b.el_id}:sig_power": b.check.window_s for b in ctx.batteries.values()
                        if b.check is not None and b.check.limit_w > 0
                        and f"{b.el_id}:sig_power" in have}


def _finite(x) -> bool:
    return isinstance(x, (int, float)) and math.isfinite(x)


def _result(project: Project, case, model, ctx, built, run, plan: _Plan, lanes,
            laps_warning: Optional[str], script_warnings: tuple[str, ...] = ()) -> SimResult:
    """Today's SimResult of the new engine's run: its evidence judged by the
    verdict and made into the reports by today's code (evidence.py)."""
    from . import core

    case_id = case.id
    inst = plan.inst
    times = [float(t) for t in run.times]
    solved = times[-1] if times else 0.0
    integ = run.integrals
    raw = run.channels  # as the engine gave them: a value that is not a number is one

    def total(name: str, what: str = "integral") -> float:
        got = integ.get(name)
        return float(got[what]) if got else 0.0

    def last(el_id: str, v: str) -> float:  # an integral the engine keeps (it only grows)
        return total(f"{inst.get(el_id, el_id)}.{v}", "max")

    messages: list[SimMessage] = [SimMessage(level="warning", text=w)
                                  for w in core.model_warnings(model)]
    messages += [SimMessage(level="info", text=t)
                 for t in core.case_notes(project, model, case, max(0.0, float(case.duration)))]
    if laps_warning:
        messages.append(SimMessage(level="warning", text=laps_warning))
    for d in built.report["warnings"]:  # the import's and preparation's
        start = d.get("code") in START_NOTES
        level = "info" if start or d.get("severity") != "warning" else "warning"
        messages.append(SimMessage(level=level, text=f"{'Preparation note' if level == 'info' else 'Preparation'}"
                                                     f" [{d.get('code')}]: {d.get('message')}"))

    # ---- the evidence ------------------------------------------------------
    ev = Evidence.of_context(ctx, energy_report=getattr(case, "energyReport", True))
    ev.distance = last(ctx.veh_id, "dist") if ctx.veh_id else 0.0
    soc = {b: raw.get(f"{b}:sig_soc") for b in ctx.batteries}
    for b in ctx.batteries.values():
        label = model.elements[b.el_id].label
        b.energy_out_wh = last(b.el_id, "e_delivered") / 3600.0
        b.energy_in_wh = last(b.el_id, "e_recuperated") / 3600.0
        b.loss_wh = last(b.el_id, "e_losses") / 3600.0
        s = soc[b.el_id]
        if s is not None and len(s):
            b.soc = float(s[-1]) / 100.0
        b.p_peak_w = total(f"{b.el_id}:sig_power", "max") * 1000.0
        low = total(f"{b.el_id}:sig_soc", "min")
        if s is not None and len(s) and low <= b.min_soc * 100.0 + 1e-6:
            b.depleted_flagged = True
            # when: on the trace, at the run's own event (the output grid's
            # next point is up to its step late: 215 s where today's says 214)
            t_low = _first(run, plan.supply[b.el_id]["soc"], lambda x: x <= b.min_soc + 1e-8)
            if t_low is None:
                t_low = times[next((k for k, x in enumerate(s) if x <= b.min_soc * 100.0 + 1e-6),
                                   len(s) - 1)]
            messages.append(SimMessage(level="warning", text=(
                f"Battery '{label}' reached minimum SOC ({b.min_soc * 100:.0f} %) at "
                f"t = {t_low:.0f} s — no further discharge; the motors get only what is "
                f"left.")))
        if b.check is not None:
            _terminal(b.check, run, b.el_id, last(b.el_id, "t_held"))
            if b.check.enforced and b.check.avg_peak_w > b.p_cap_w:
                # an enforced cap the engine holds to its tolerance: the
                # power at the cap, not over it (80.0000020 kW at 80 kW
                # with rtol 1e-6); past 100 times the run's rtol it is over
                slack = 100.0 * float(run.report.get("rtol", 1e-6))
                if b.check.avg_peak_w <= b.p_cap_w * (1.0 + slack):
                    b.check.avg_peak_w = b.p_cap_w
            held = next((float(e["t"]) for e in run.events
                         if e["label"].startswith(f"'{label}': the time it is held")), None)
            if b.check.enforced and b.check.limit_s > 0 and held is not None:
                # today's note (domains.RunContext), at the run's own event
                margin = b.check.margin_pct
                cap = (f"its Output Power Limit less its {margin:g} % margin "
                       f"({b.p_cap_w / 1000.0:g} of {b.check.limit_w / 1000.0:g} kW at the "
                       f"terminals)" if margin > 0 else
                       f"its Output Power Limit ({b.p_cap_w / 1000.0:g} kW at the terminals)")
                messages.append(SimMessage(level="info", text=(
                    f"Battery '{label}' held at {cap} from t = {held:.2f} s — the motors get "
                    f"what is left after the other loads. The run summary says for how long.")))
    for mc in ctx.motors.values():
        mc.limited_s = last(mc.el_id, "t_limited")
        mc.regen_lost_wh = last(mc.el_id, "e_regen_lost") / 3600.0
    for ec in ctx.engines.values():
        ec.fuel_used_kg = last(ec.el_id, "fuel_used")
    tank = model.fuel_tank
    if tank and ctx.engines and total(f"{tank}:sig_mass", "min") <= 0.0 and f"{tank}:sig_mass" in integ:
        for ec in ctx.engines.values():
            ec.stalled_flagged = True
            messages.append(SimMessage(level="warning", text=(
                f"Fuel tank empty — engine '{model.elements[ec.el_id].label}' shut off.")))
    for fc in ctx.fuelcells.values():
        fc.energy_wh = total(f"{fc.el_id}:sig_power") / 3.6
    for el_id, cdef in model.cdef_of.items():
        if cdef.id == "electric.voltage_source":
            ctx.vsource_energy_wh[el_id] = total(f"{el_id}:sig_power") / 3.6
    if ctx.full_throttle:  # the share of the run a wheel was at its grip limit (figures)
        pct = next((f["value"] for f in run.figures if f["key"] == "grip_limit_time_pct"), 0.0)
        ev.grip_limited_s = pct / 100.0 * (solved - (times[0] if times else 0.0))
    ev.grip_limited_s = ev.grip_limited_s or 0.0
    _tables(ctx, run, messages)
    _over_speed(ctx, plan, run, messages)
    messages += [SimMessage(level="warning", text=t)
                 for t in _supply(ctx, plan, run) + list(script_warnings)]
    books = run.energy or {}
    ev.flows = _flows(model, inst, books, integ, _port_flows(plan, run))
    ev.time_s = ev.duty_s = solved - (times[0] if times else 0.0)
    ev.duty = _duty(ctx, inst, integ)
    ev.lanes = _lanes(lanes, plan, run)
    ev.residual_wh = abs(books.get("closure_J", 0.0)) / 3600.0
    ev.throughput_wh = books.get("throughput_J", 0.0) / 3600.0
    ev.vsource_energy_wh = ctx.vsource_energy_wh
    # the engine's own words on its run (its energy books' closure and drift,
    # an assert that did not hold, ...): the tables it read past are the
    # verdict's and the summary's (above), not repeated
    told = {d["warning"] for d in run.table_outside}
    messages += [SimMessage(level="warning", text=f"The new engine's run: {w}.")
                 for w in run.report["warnings"] if w not in told]

    # ---- the verdict -------------------------------------------------------
    tr = run.trace or {}
    src_kind = model.cdef_of[plan.src[0]].id if plan.src else None
    trace = Trace(ctx=ev, by_distance=bool(src_kind == "signal.driving_task"
                                           and distance_axis(model.params_of[plan.src[0]])))
    if tr and (plan.target or ctx.full_throttle) and plan.speed and plan.dist:
        trace.times = [float(t) for t in tr["times"]]
        trace.target = ([0.0] * len(trace.times) if ctx.full_throttle
                        else [float(v) for v in tr[plan.target]])
        trace.speed = [float(v) for v in tr[plan.speed]]
        trace.dist = [float(v) for v in tr[plan.dist]]
        if run.report["reran_to_finish"] and ctx.end_distance is not None and trace.dist:
            # the engine ran again to its finish event and ended there: the
            # line, located to its root finding's precision (75.3000007 m at
            # rtol 1e-6, a hair short of 75.3 m at 1e-8), is where the run is
            trace.dist[-1] = max(trace.dist[-1], ctx.end_distance)
    series = {tuple(name.split(":", 1)): values for name, values in raw.items() if ":" in name}
    verdict = judge(trace, ev.distance, series, ctx.performance, solved, core.judged_uses(ev),
                    False, case)
    messages += [SimMessage(level=level, text=text) for level, text in verdict.messages]

    # ---- channels ------------------------------------------------------------
    unit_map = unit_groups()
    ports = {}
    for el_id, cdef in model.cdef_of.items():
        for p in list(cdef.ports) + list(model.elements[el_id].dynamicPorts or []):
            ports[(el_id, p.id)] = p
    channels: list[Channel] = []
    # in today's order (core.py: by element, then port); the new engine
    # records a few more channels than today's, which are kept
    named = sorted((tuple(name.split(":", 1)), name) for name in raw if ":" in name)
    for (el_id, port_id), name in named:
        pdef = ports.get((el_id, port_id))
        if pdef is None:
            continue
        channels.append(Channel(
            elementId=el_id,
            portId=port_id,
            label=f"{model.elements[el_id].label} · {pdef.name}",
            unit=unit_map.get(getattr(pdef, "unitGroup", None) or "No Unit", "-"),
            timeSeries=[{"t": t, "value": _number(v)} for t, v in zip(times, raw[name])],
            min=_numbers(run.min.get(name)),
            max=_numbers(run.max.get(name)),
            mean=_numbers(run.mean.get(name)),
        ))

    # ---- the summary: the engine's figures, judged ---------------------------
    summary = [SummaryValue(key=f["key"], label=f["label"], value=f["value"], unit=f["unit"])
               for f in run.figures]
    by_key = {s.key: s for s in summary}
    for b in ctx.batteries.values():
        if b.check is None:
            continue
        label = model.elements[b.el_id].label
        rows, problems = terminal_checks(b.el_id, label, b.check,
                                         usable_energy_left_wh(b) / 1000.0, b.depleted_flagged)
        for row_label, v, u, lim, ok, k in rows:
            s = by_key.get(k)
            if s is None:
                s = by_key[k] = SummaryValue(key=k, label=row_label, value=v, unit=u)
                summary.append(s)
            # (the value its passed flag is judged on, not the engine's figure)
            s.label, s.value, s.limit, s.passed = row_label, v, lim, ok
        messages += [SimMessage(level="warning", text=t) for t in problems]
    edge_rows: set[str] = {"Energy balance residual"}
    edges = core.outside_rows(ev, solved)
    edge_rows |= {r.label for r in edges}
    duration = next((s for s in summary if s.key == "simulated_duration_s"), None)
    summary = [s for s in summary if s is not duration] + edges + ([duration] if duration else [])
    rows = [SummaryValue(key=k, label=label, value=v, unit=u, limit=lim, passed=ok)
            for label, v, u, lim, ok, k in verdict.rows]
    edge_rows |= {r.label for r in rows if r.unit == "%"}
    mine = {r.key for r in rows}
    summary = [s for s in summary if s.key not in mine]
    if ctx.full_throttle:  # an acceleration test's own figures come first, as today
        summary[:0] = rows
    else:
        summary[-1:-1] = rows
    net_wh = sum(b.energy_out_wh - b.energy_in_wh for b in ctx.batteries.values())
    electric = (net_wh > 0 and sum(ec.fuel_used_kg for ec in ctx.engines.values()) <= 0
                and not ctx.fuelcells)
    not_valid = core.not_valid_first(ev, verdict, summary, False, None, times)
    core.not_valid_last(ev, verdict, summary, not_valid, edge_rows, lab_base(summary, electric))
    fill_keys(summary, project)

    energy, duty, limits = make_reports(ev)
    return SimResult(caseId=case_id, status=core.status_of(messages, False),
                     channels=channels, summary=summary, messages=messages,
                     partEnergy=energy_flows(ev.flows, ev.time_s), energy=energy, duty=duty,
                     limits=limits,
                     references=check_references(case.references, summary)
                     + hand_checks(ev, channels))


def _terminal(chk, run, el_id: str, held_s: float) -> None:
    """A battery's TerminalCheck from the run: its terminal power's peak and
    its peak over the check window (the engine's moving average of the
    line through every point it visited, as today's is of every step:
    the output step changes neither; 0 s, the peak itself), its highest
    and lowest voltage, the time held at (or over) its limit. A judged
    number the run kept no record of is this path's fault (NEW-ERROR), not
    a pass at 0 kW."""
    times = [float(t) for t in run.times]
    power, volt = f"{el_id}:sig_power", f"{el_id}:sig_voltage"
    integ = run.integrals
    gone = [n for n, judged in ((power, chk.limit_w > 0), (volt, chk.v_class > 0))
            if judged and n not in integ]
    if chk.limit_w > 0 and power in integ and not run.windows.get(power):
        gone.append(f"the moving average of {power} over {chk.window_s:g} s")
    if gone:
        raise RuntimeError(f"the run kept no record of {', '.join(gone)}, which battery {el_id}'s "
                           f"checks are judged on")
    if power in integ:
        chk.peak_w = max(chk.peak_w, float(integ[power]["max"]) * 1000.0)
        got = run.windows.get(power)
        # (float: a NumPy number would make the passed flag NumPy's bool)
        if got and float(got["peak"]) * 1000.0 > chk.avg_peak_w:
            chk.avg_peak_w, chk.t_avg_peak = float(got["peak"]) * 1000.0, float(got["t"])
    if volt in integ:
        hi = run.max[volt]
        k = max(range(len(hi)), key=lambda i: hi[i]) if len(hi) else 0
        if len(hi) and float(hi[k]) > chk.v_peak:
            chk.v_peak, chk.t_v_peak = float(hi[k]), times[k]
            chk.p_at_v_peak = float(run.channels[power][k]) * 1000.0 if power in integ else 0.0
        chk.v_min = min(chk.v_min, float(integ[volt]["min"]))
    chk.limit_s = held_s


def _tables(ctx, run, messages: list[SimMessage]) -> None:
    """The tables the run read past their data, from the engine's records,
    into today's MapUse records (in the axis's unit: the engine's axis
    mapped onto today's), each said once as today's engine says it. A
    record of a table today's engine does not read as a part's data (a
    Driving Task's schedule) is not judged: a note with its code says so
    (NOT_JUDGED)."""
    for rec in run.table_outside:
        el_id, key, axis = rec["part"], rec["table"].rsplit(".", 1)[-1], int(rec["axis"])
        if el_id is None:
            continue
        uses = [u for u in ctx.map_use if u.el_id == el_id and u.table == key]
        m = ctx.maps.get((el_id, key))
        if axis >= len(uses):
            messages.append(SimMessage(level="info", text=(
                f"'{ctx.model.elements[el_id].label}' {key}: the new engine read it past its data "
                f"on axis {axis + 1} for {float(rec['total']):g} s from t = "
                f"{float(rec['first']):.2f} s (to {float(rec['furthest']):g} against its edge "
                f"{float(rec['edge']):g}, in the engine's units); today's engine does not read "
                f"it as a part's data, so the verdict does not judge it [{NOT_JUDGED}].")))
            continue
        use = uses[axis]
        lo, hi = rec["lo"], rec["hi"]
        rng = m.ranges[axis] if m is not None and axis < len(m.ranges) else None

        def shown(x: float, rng=rng, lo=lo, hi=hi) -> float:
            # the engine's axis against today's in its unit: a scale, from
            # the ends they share (the engine mirrors a table read at a
            # speed's magnitude to the negative speeds: its last points
            # still meet); a temperature axis the engine reads in °C too
            if rng is None:
                return x
            if _finite(hi) and hi != 0:
                return x * rng[1] / hi
            return x * rng[0] / lo if _finite(lo) and lo != 0 else x
        edge = rec["edge"] if _finite(rec["edge"]) else (hi if _finite(hi) else math.nan)
        far = rec["furthest"] if _finite(rec["furthest"]) else edge
        use.outside_s = float(rec["total"])
        use.edge, use.value = shown(edge), shown(far)
        use.t = float(rec["t_furthest"]) if _finite(rec["t_furthest"]) else float(rec["first"])
        how = ("its edge slope is extended" if m is not None and axis < len(m.linear)
               and m.linear[axis] else "its edge value is held")
        name = m.name if m is not None else f"'{ctx.model.elements[el_id].label}' {key}"
        messages.append(SimMessage(level="info", text=(
            f"{name}: {use.axis} {use.value:.6g} {use.unit} is past the edge of its data "
            f"({use.edge:g} {use.unit}) from t = {float(rec['first']):.2f} s — {how} there. "
            f"The run summary says for how long and how far.")))


# the note's code for a table record the verdict does not judge (_tables)
NOT_JUDGED = "NEW-TABLE-NOT-JUDGED"

# the codes of the path's notes on a run it ran: what of it the verdict does
# not judge (benchmarks/back_to_back/acceptance.toml's [note_codes])
NOTE_CODES = {NOT_JUDGED: "a table the run read past its data that today's engine does not read "
                          "as a part's data: a note, not judged"}


def _at_event(ev_times: list[float], t0: float, t1: float) -> float:
    """When a change seen between two traced points happened: at the run's
    last event inside that stretch (exact), else at its start."""
    i = bisect.bisect_right(ev_times, t1) - 1
    return ev_times[i] if i >= 0 and ev_times[i] > t0 else t0


def _first(run, name: Optional[str], hit: Callable[[float], bool]) -> Optional[float]:
    """When the traced ``name`` first meets ``hit``: at the run's last event
    in the traced stretch where it does (_at_event); None when it never does
    or is not traced."""
    tr = run.trace or {}
    if name is None or name not in tr:
        return None
    k = next((k for k, x in enumerate(tr[name]) if hit(float(x))), None)
    if k is None or k == 0:
        return None if k is None else float(tr["times"][0])
    return _at_event(sorted(float(e["t"]) for e in run.events),
                     float(tr["times"][k - 1]), float(tr["times"][k]))


def _over_speed(ctx, plan: _Plan, run, messages: list[SimMessage]) -> None:
    """Each E-Motor's and Engine's time above its maximum speed and its
    highest speed then (today's speed_use records, which the verdict and
    the summary judge), from the run: the time the line through its traced
    speed spends above that speed by more than 1e-6 of it (today's
    over_speed counts no step its own drive or a limiter holds it there;
    the engine's motor gives no drive torque at its maximum speed and its
    limiter switches at it, so only what drives it higher is above), its
    highest speed over every point the integrator visited and when the
    trace saw it; and, as today's engine says it, a note when it first
    went above."""
    tr = run.trace or {}
    times = [float(t) for t in tr.get("times", [])]
    integ = run.integrals
    for el_id, m in (*ctx.motors.items(), *ctx.engines.items()):
        name = plan.speeds.get(el_id)
        use, got = m.speed_use, integ.get(f"{el_id}:sig_speed")
        if name is None or not got or len(times) < 2:
            continue
        n_max = use.edge
        top = n_max * (1.0 + 1e-6)
        xs = [abs(float(x)) for x in tr[name]]
        above, first = 0.0, None
        for k in range(1, len(times)):
            a, b, dt = xs[k - 1] - top, xs[k] - top, times[k] - times[k - 1]
            if a > 0 or b > 0:
                share = 1.0 if a > 0 and b > 0 else max(a, b) / abs(a - b)
                above += share * dt
                if first is None:
                    first = times[k - 1] if a > 0 else times[k] - share * dt
        if above <= 0:
            continue
        k = max(range(len(xs)), key=xs.__getitem__)
        use.outside_s = above
        use.value = max(float(got["max"]), -float(got["min"]))
        use.t = times[k]
        label = ctx.model.elements[el_id].label
        messages.append(SimMessage(level="info", text=(
            f"E-Motor '{label}' was driven above its maximum speed ({n_max:,.0f} 1/min) at "
            f"t = {first:.2f} s; its inverter is off above it, so it gives no drive torque and "
            f"no regeneration. The run summary says for how long and how far."
            if el_id in ctx.motors else
            f"Engine '{label}' was driven above its maximum speed ({n_max:,.0f} 1/min, the "
            f"full-load curve's last point) at t = {first:.2f} s; it is not fired above it. The "
            f"run summary says for how long and how far.")))


def _supply(ctx, plan: _Plan, run) -> list[str]:
    """Today's warnings that a supply limit held a motor back or cut a
    consumer back (RunContext.limit_message and update_source_limits), each
    said once with today's words, from the run's trace: a motor held below
    its command over a traced stretch (its time held grew) recuperating
    there (its recuperation lost grew) or drawing; a consumer served less
    than its demand. Which limit, as today's says it: a battery at its
    Output Power Limit (a note, the run's own event), at its minimum SOC (a
    warning of its own) or at a current or voltage limit of its battery
    management (today's note, not said here) holds back no warning; else a
    battery at its maximum-power point ("maxp"), full ("full"), at its Max
    Charge Power ("chg"); a fuel cell at its maximum power ("fcmax") or one
    that takes nothing back ("fcback"); a bus with no source ("nosrc").

    A fuel cell whose Hydrogen Tank is empty from the start gives nothing:
    today's says so ("h2empty") and never "fcmax". One whose tank the run
    drains: the new engine's fuel cell gives its maximum power until the
    tank is empty (an event) and nothing after, where today's holds it to
    the hydrogen left, so its tank never quite reaches 0 and the stretches
    it holds the motors are "fcmax"; so are the new run's after its tank
    ran dry."""
    model = ctx.model
    out: dict[str, str] = {}
    tank = ctx.tanks.get(model.h2_tank) if model.h2_tank else None
    h2_empty = tank is not None and tank.mass_kg <= 0  # (today's RunContext at its start)
    if h2_empty and ctx.fuelcells:
        out["h2empty"] = "Hydrogen tank empty — fuel cell shut down."
    tr = run.trace or {}
    times = [float(t) for t in tr.get("times", [])]
    if len(times) < 2:
        return list(out.values())
    ev_times = sorted(float(e["t"]) for e in run.events)

    def at(el_id: str, v: str, k: int) -> Optional[float]:
        name = plan.supply.get(el_id, {}).get(v)
        return None if name is None else float(tr[name][k])

    def grew(el_id: str, v: str, k: int) -> bool:
        a, b = at(el_id, v, k - 1), at(el_id, v, k)
        return a is not None and b is not None and b > a

    root_of = {bus.id: nodes[0][0] for nodes in ctx.bus_tree.values() for bus, _, _ in nodes}

    def held_back(root, discharge: bool, k: int) -> None:
        if root.battery:
            b = ctx.batteries[root.battery]
            label = model.elements[b.el_id].label
            soc = at(b.el_id, "soc", k)
            if discharge:
                i_mpp, i_lim = at(b.el_id, "i_mpp", k), at(b.el_id, "i_dis_lim", k)
                if ((b.check is not None and b.check.enforced and grew(b.el_id, "t_held", k))
                        or (soc is not None and soc <= b.min_soc + 1e-9)
                        or (i_mpp is not None and i_lim is not None and i_lim < i_mpp)):
                    return
                out.setdefault(f"maxp:{b.el_id}", (
                    f"Battery '{label}' demand exceeds its deliverable power — motor torque "
                    f"limited at the maximum power point."))
            elif soc is not None and soc >= 1.0 - 1e-9:
                out.setdefault(f"full:{b.el_id}",
                               f"Battery '{label}' is full — regenerative torque limited.")
            elif (at(b.el_id, "p_absorb", k) or 0.0) >= b.max_charge_w * (1.0 - 1e-6):
                out.setdefault(f"chg:{b.el_id}", (
                    f"Charging exceeds max charge power of '{label}' — regenerative torque "
                    f"limited."))
        elif root.fuelcell:
            label = model.elements[root.fuelcell].label
            if not discharge:
                out.setdefault(f"fcback:{root.fuelcell}", (
                    f"Fuel cell '{label}' cannot take power back — regenerative torque "
                    f"limited."))
            elif not h2_empty:
                out.setdefault(f"fcmax:{root.fuelcell}", (
                    f"Fuel cell '{label}' demand exceeds its maximum power — motor torque "
                    f"limited."))
        elif not root.vsource:
            out.setdefault(f"nosrc:{root.id}",
                           "An electrical bus has load but no source — demand is unmet.")

    for k in range(1, len(times)):
        for m_id in ctx.motors:
            bus = ctx.motor_bus.get(m_id)
            if bus is not None and bus.id in root_of and grew(m_id, "t_limited", k):
                held_back(root_of[bus.id], not grew(m_id, "e_regen_lost", k), k)
        for bus in model.buses:
            if bus.id not in root_of:
                continue
            for c_id in bus.consumers:
                served, demand = at(c_id, "served", k), at(c_id, "p_dem", k)
                if served is None or demand is None or not (demand > 0 and served < 1.0 - 1e-9):
                    continue
                if f"shed:{c_id}" not in out:
                    t = _at_event(ev_times, times[k - 1], times[k])
                    out[f"shed:{c_id}"] = (f"Power consumer '{model.elements[c_id].label}' cut "
                                           f"back at t = {t:.0f} s — its source cannot supply it.")
                if not root_of[bus.id].fuelcell:  # (today's says which battery limit, or no source)
                    held_back(root_of[bus.id], True, k)
    return list(out.values())


# the throughput power a part's duty values are of (energy.py), by its kind
_THROUGH = {"battery.generic": "sig_power", "motor.emotor": "sig_elec_power",
            "electric.constant_drive": "sig_power", "electric.climate": "sig_power",
            "electric.voltage_source": "sig_power", "fuelcell.stack": "sig_power",
            "engine.combustion": "sig_power", "controller.dcdc": "sig_power_in",
            "mech.brake": "sig_power", **{k: "sig_power" for k in _GEARS}}


# the mechanical parts whose flows each way come from their ports' powers
_MECH = (*("mech.shaft", "mech.final_drive", "mech.gearbox", "mech.differential",
           "mech.transfer_case"), "mech.clutch", "propulsion.wheel")


def _road_side(port: str) -> bool:
    """A mechanical part's port on the road side of it (a gear's output, a
    clutch's flange b, a wheel's road contact)."""
    return "out" in port or port in ("road", "flange_b")


def _port_flows(plan: _Plan, run) -> dict[str, tuple[float, float, float, float, float]]:
    """Each mechanical part's (and the Vehicle's) ports' powers on the
    run's trace, p each port's torque times speed (or force times speed),
    along the line between its points: (∫ p⁺ dt summed over its ports, the
    share of it through its ports on the road side; and its throughput
    power's (its ports' not on the road side, or the Vehicle's road) peak,
    ∫ p dt and ∫ p² dt), J, W, J, J·W. The engine's books keep a part's net
    power through all its ports, and its channels give no port's power but
    a gear's input: these come from the trace, every TRACE_STEP_S."""
    tr = run.trace or {}
    times = [float(t) for t in tr.get("times", [])]
    out = {}
    for el_id, ports in plan.ports.items():
        total = back = 0.0
        through_p = [0.0] * len(times)
        for port, through, across in ports:
            p = [float(a) * float(b) for a, b in zip(tr[through], tr[across])]
            if not _road_side(port) or port == "road":
                through_p = [x + y for x, y in zip(through_p, p)]
            pos = 0.0
            for k in range(1, len(times)):
                a, b, dt = p[k - 1], p[k], times[k] - times[k - 1]
                if a >= 0 and b >= 0:
                    pos += 0.5 * (a + b) * dt
                elif a > 0 or b > 0:  # the line crosses zero inside
                    pos += 0.5 * max(a, b) ** 2 / abs(a - b) * dt
            total += pos
            if _road_side(port):
                back += pos
        q = through_p
        integral = sum(0.5 * (q[k - 1] + q[k]) * (times[k] - times[k - 1]) for k in range(1, len(times)))
        square = sum((q[k - 1] ** 2 + q[k - 1] * q[k] + q[k] ** 2) / 3.0 * (times[k] - times[k - 1])
                     for k in range(1, len(times)))
        out[el_id] = (total, back, max((abs(x) for x in q), default=0.0), integral, square)
    return out


def _flows(model, inst: dict, books: dict, integ: dict,
           shafts: dict[str, tuple[float, float, float, float, float]]) -> list[BookFlow]:
    # (shafts: each mechanical part's energy in through its ports and from
    # the road side, _port_flows)
    """Each part's energy over the run from the engine's books (J), one per
    element (the books' parts by their path, ImportReport.instances): its
    losses and its stored energy's change as the books have them; what went
    in and what came out, each way, from the integrals of its ports' powers
    where its channels give them (an E-Motor's electrical and shaft power),
    a gear's, a clutch's or a wheel's from its ports' powers on the trace
    (_port_flows), what came out then being what went in less its losses
    and stored energy, a brake's from its heat (today's: the braking power
    it takes in), else from the books' net power through its ports; its throughput power's peak, mean
    and RMS from that channel's integrals (a clutch's and the Vehicle's
    from the trace); the Vehicle's named terms from its road-load channels
    (its acceleration the kinetic energy its books stored). As today's books
    have them: each driveline's spinning parts' kinetic energy is one
    *Rotating parts* row of its own, not the parts' (today's
    RunContext.close_book), and what a rigid engagement took at an event
    (a gear shift: the books' impulse losses, of the part they name) is that
    part's loss. A junction (a node, a ground, the ambient), which today's
    books leave out and which loses and stores nothing, is left out too."""
    by_inst = sorted(((v, k) for k, v in inst.items()), key=lambda x: -len(x[0]))
    flows: dict[str, BookFlow] = {}
    for p in books.get("parts", []):
        path = p["path"]
        el_id = next((k for v, k in by_inst if path == v or path.startswith(v + ".")), None)
        key = el_id or path
        f = flows.get(key)
        if f is None:
            kind = model.cdef_of[el_id].id if el_id in model.cdef_of else "engine.part"
            label = model.elements[el_id].label if el_id in model.elements else p["name"]
            f = flows[key] = BookFlow(el_id, label, kind, 0.0, 0.0, 0.0, 0.0)
        e_in, thr = p["energy_in_J"], p["throughput_J"]
        f.in_j += 0.5 * (thr + e_in)
        f.out_j += 0.5 * (thr - e_in)
        f.loss_j += p["lost_J"] + p.get("impulse_lost_J", 0.0)
        f.stored_j += p["stored_change_J"]

    def flow(name: str) -> tuple[float, float]:  # ∫ p⁺ dt, ∫ p⁻ dt of a kW channel, J
        got = integ.get(name)
        if not got or not _finite(got["magnitude"]):
            return math.nan, math.nan
        return (500.0 * (got["magnitude"] + got["integral"]),
                500.0 * (got["magnitude"] - got["integral"]))
    for f in flows.values():
        el = f.el_id
        if el is None:
            continue
        if f.part == "battery.generic":  # what charged it came back (today's books)
            f.in_rev_j = f.in_j
        elif f.part == "vehicle.body":  # what it gave back to the wheels
            f.in_rev_j = f.out_j
        if f.part == "engine.combustion":  # its fuel in, and its shaft each way
            fuel = integ.get(f"{el}:sig_fuel_power", {}).get("integral", 0.0) * 1000.0
            _, dragged = flow(f"{el}:sig_power")
            if _finite(dragged):
                f.in_j, f.in_rev_j, f.fuel_j = fuel + dragged, dragged, fuel
        elif f.part == "motor.emotor":
            (pe_in, _), (_, pm_in) = flow(f"{el}:sig_elec_power"), flow(f"{el}:sig_mech_power")
            if _finite(pe_in) and _finite(pm_in):
                f.in_j, f.in_rev_j = pe_in + pm_in, pm_in
        elif el in shafts and f.part != "vehicle.body":
            f.in_j, f.in_rev_j = shafts[el][:2]
        elif f.part == "mech.brake":  # the braking power it takes in (its disc spins with the rest)
            f.in_j = f.loss_j
        if f.part == "vehicle.body":
            terms = {t: integ.get(f"{el}:{port}", {}).get("integral", 0.0) * 1000.0
                     for t, port in (("air drag", "sig_p_aero"), ("rolling resistance", "sig_p_roll"),
                                     ("climbing", "sig_p_grade"))}
            terms["acceleration"] = f.stored_j - terms["climbing"]  # (its books: exact)
            f.terms = terms
        # what came out: what went in, less what it lost and stored, so that
        # its books close exactly as today's do (the books' own closure and
        # drift, round-off to 1e-7 of the run's throughput, go to it)
        f.out_j = f.in_j - f.loss_j - f.stored_j
        port = _THROUGH.get(f.part)
        got = integ.get(f"{el}:{port}") if port else None
        if got:
            f.peak_w = max(abs(got["max"]), abs(got["min"])) * 1000.0
            f.p_int, f.p2_int = got["integral"] * 1000.0, got["square"] * 1e6
        elif el in shafts:  # (no channel gives it: the trace's ports)
            f.peak_w, f.p_int, f.p2_int = shafts[el][2:]
    # each driveline's spinning parts: their kinetic energy one row of its
    # own, as today's books have it
    for k, dl in enumerate(model.drivelines):
        ke = 0.0
        for el_id in dict.fromkeys(dl.element_group):
            f = flows.get(el_id)
            if f is None or f.part in ("vehicle.body", "battery.generic"):
                continue
            ke += f.stored_j
            f.stored_j = 0.0
            f.out_j = f.in_j - f.loss_j
        label = ("Rotating parts" if len(model.drivelines) < 2
                 else f"Rotating parts (driveline {k + 1})")
        flows[f"rotating:{k}"] = BookFlow(None, label, "driveline.inertia", max(ke, 0.0),
                                          max(-ke, 0.0), 0.0, ke)
    return [f for f in flows.values()
            if not (f.part in _JUNCTIONS and f.loss_j == 0.0 and f.stored_j == 0.0)]


_JUNCTIONS = ("mech.node", "electric.node", "boundary.ground", "boundary.ambient")


# a duty part's quantities: the channel (or the engine's variable) each is read from
_DUTY_FROM = {
    "motor.emotor": ("{el}:sig_mech_power", "{el}:sig_elec_power", "{el}:sig_torque",
                     "{inst}.i", "{el}:sig_losses"),
    "battery.generic": ("{el}:sig_power", "{el}:sig_current", "{el}:sig_losses"),
    "engine.combustion": ("{el}:sig_power", "{el}:sig_torque", "{el}:sig_fuel_rate"),
    "fuelcell.stack": ("{el}:sig_power", "{el}:sig_current"),
    "controller.dcdc": ("{el}:sig_power_in", "{el}:sig_power_out", "{el}:sig_losses",
                        "{inst}.i_out"),
}


def _duty(ctx, inst: dict, integ: dict) -> list:
    """Each part's duty (evidence.Evidence.duty), from the engine's integrals
    over every step it took and its intervals' extremes: the parts today's
    RunRecorder keeps, in its order."""
    out = []
    parts = [*ctx.motors, *ctx.batteries, *ctx.engines, *ctx.fuelcells,
             *(d for d, c in ctx.model.cdef_of.items() if c.id == "controller.dcdc")]
    for el_id in parts:
        kind = ctx.model.cdef_of[el_id].id
        rows = []
        for (q, unit), name in zip(DUTY_QUANTITIES[kind], _DUTY_FROM[kind]):
            got = integ.get(name.format(el=el_id, inst=inst.get(el_id, el_id)))
            if got and all(_finite(got[k]) for k in ("integral", "square", "max", "min")):
                rows.append((q, unit, [got["integral"], got["square"], got["max"], got["min"]]))
        out.append((el_id, rows))
    return out


def _lanes(lanes, plan: _Plan, run) -> list:
    """What held each driveline back (reports.Lane), by today's rule
    (reports.lane_state) on the run's trace from the dense output: the
    state at each traced point names the stretch before it; a change takes
    the time of the last event of the run inside that stretch (a mode of
    the motor's supply window, a wheel's grip, ...: exact), or the
    stretch's start where none was."""
    tr = run.trace or {}
    times = [float(t) for t in tr.get("times", [])]
    if len(times) < 2 or not lanes:
        return lanes
    ev_times = sorted(float(e["t"]) for e in run.events)

    def at(name: Optional[str], k: int, default: float = 0.0) -> float:
        if name is None:
            return default
        v = float(tr[name][k])
        return v if math.isfinite(v) else default

    def sign(x: float) -> float:  # a command or torque at round-off of zero is zero
        return 0.0 if abs(x) < 1e-9 else x
    for lane, (motors, engines, wheels) in zip(lanes, plan.lane_parts):
        lane.seconds = [0.0] * len(lane.seconds)
        lane.changes, lane.state = [], -1
        start = times[0]
        for k in range(1, len(times)):
            braking = at(plan.brake, k) > 1e-6 or at(plan.traction, k) < -1e-6  # (today's)
            ms = []
            for mc, demand, speed, limited, bat, held in motors:
                d = sign(at(demand, k))
                lim = limited is not None and at(limited, k) > at(limited, k - 1)
                cap = (lim and held is not None and bat.check is not None and bat.check.enforced
                       and at(held, k) > at(held, k - 1))
                ms.append((d, d >= 0.999 or at(speed, k) > mc.max_rpm * (1.0 - SPEED_LIMIT_BAND),
                           lim, cap))
            es = []
            for ec, torque, speed in engines:
                tq = sign(at(torque, k))
                try:
                    full = interp1(ec.full_load.pts, at(speed, k), False)
                except (IndexError, TypeError, ValueError):
                    full = 0.0
                es.append((tq, tq > 0 and full > 0 and tq >= 0.99 * full))
            code = lane_state(braking, ms, es,
                              lambda k=k: any(at(w, k) > 0.5 for w in wheels))
            if code != lane.state:
                if lane.state >= 0:
                    t_change = _at_event(ev_times, times[k - 1], times[k])
                    lane.seconds[lane.state] += t_change - start
                    start = t_change
                lane.state = code
                lane.changes.append((round(start, 4), code))
        lane.seconds[lane.state] += times[-1] - start
    return lanes


def _refresh_status(result: SimResult) -> None:
    """A warning message makes a successful run a warning one."""
    if result.status == "success" and any(m.level == "warning" for m in result.messages):
        result.status = "warning"


def _number(v) -> Optional[float]:
    v = float(v)
    return v if math.isfinite(v) else None


def _numbers(values) -> Optional[list[Optional[float]]]:
    return None if values is None else [_number(v) for v in values]
