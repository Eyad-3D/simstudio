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

The new engine's run is turned into today's SimResult: its channels and
figures are already in today's names and units; this module adds the labels
and units of the channels, the messages and the status. The result has no
energy report, part energies, duty, limits or references yet.
"""
from __future__ import annotations

import math
import os
from typing import Callable, NamedTuple, Optional

from .. import cycles, script_trust
from ..library import unit_groups
from ..schemas import Channel, Project, SimMessage, SimResult, SummaryValue
from . import balance
from .battery import CELLS
from .keys import fill_keys
from .network import ModelError, build_model
from .runtime import ControlFn, EmitFn
from .sandbox import ScriptSandbox, ScriptSpec

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


def run_new(project: Project, case_id: str) -> SimResult:
    """The case on the new engine, as today's SimResult. Raises when the new
    engine cannot build or run it."""
    import lightsim_engine

    case = next(c for c in project.cases if c.id == case_id)
    # today's model gives what the engine's run lacks: the labels and units of
    # the channels, and the Script blocks' code, ports and parameters
    model = build_model(project, {}, getattr(case, "parameterOverrides", None) or {})
    built = lightsim_engine.build(project.model_dump_json(), case_id, cycles_dir=str(cycles.DIR))
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
    if not specs:
        run = built.simulate()
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
                def step(t, dt, inputs):
                    return sandbox.run(spec.el_id, spec.label, t, dt, dict(inputs), spec.params)
                return step
            run = built.simulate(scripts={sp.el_id: runner(sp) for sp in specs},
                                 native_scripts=False)

    times = [float(t) for t in run.times]
    unit_map = unit_groups()
    ports = {}
    for el_id, cdef in model.cdef_of.items():
        for p in list(cdef.ports) + list(model.elements[el_id].dynamicPorts or []):
            ports[(el_id, p.id)] = p
    channels: list[Channel] = []
    # in today's order (core.py: by element, then port); the new engine
    # records a few more channels than today's, which are kept
    named = sorted((tuple(name.split(":", 1)), name) for name in run.channels if ":" in name)
    for (el_id, port_id), name in named:
        values = run.channels[name]
        pdef = ports.get((el_id, port_id))
        if pdef is None:
            continue
        channels.append(Channel(
            elementId=el_id,
            portId=port_id,
            label=f"{model.elements[el_id].label} · {pdef.name}",
            unit=unit_map.get(getattr(pdef, "unitGroup", None) or "No Unit", "-"),
            timeSeries=[{"t": t, "value": _number(v)} for t, v in zip(times, values)],
            min=_numbers(run.min.get(name)),
            max=_numbers(run.max.get(name)),
            mean=_numbers(run.mean.get(name)),
        ))
    summary = [SummaryValue(key=f["key"], label=f["label"], value=f["value"], unit=f["unit"])
               for f in run.figures]
    messages = [SimMessage(level="warning", text=str(d["message"]))
                for d in built.report["warnings"] if d.get("severity") == "warning"]
    result = SimResult(caseId=case_id, status="success", channels=channels,
                       summary=summary, messages=messages)
    fill_keys(result.summary, project)
    _refresh_status(result)
    return result


def _refresh_status(result: SimResult) -> None:
    """A warning message makes a successful run a warning one."""
    if result.status == "success" and any(m.level == "warning" for m in result.messages):
        result.status = "warning"


def _number(v) -> Optional[float]:
    v = float(v)
    return v if math.isfinite(v) else None


def _numbers(values) -> Optional[list[Optional[float]]]:
    return None if values is None else [_number(v) for v in values]
