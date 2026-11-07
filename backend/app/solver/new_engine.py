"""Running a case on the new engine, behind a flag (LIGHTSIM_ENGINE).

``LIGHTSIM_ENGINE=new`` makes ``simulate`` (core.py) try the new engine, the
Rust module ``lightsim_engine``, for the cases it can run: a drive cycle or an
acceleration test whose values keep to their parameters' limits (a study's
point can hold one Data Checks never saw), live or not. A live run (an emit,
a control: the app's runs, a study's points) runs the engine in a thread of
its own with a ``lightsim_engine.RunControl`` (_Live): its output points
streamed as today's "step" messages as the engine records them, the
result's values bit for bit, at the case's pace; a cancel stops it where it
is; a set_param is a live edit of a runtime parameter, an event at the
run's next output time (a structural one, one the path's checks after the
run read as at its start, takes effect on the next run, and the result's
note of it says so with the code NEW-LIVE-NEXT-RUN). The result says which
engine ran it, its version, its solver options and the live edits it took
(``replay`` makes them again). Everything else, and any failure of the new
engine, runs on today's engine (but a live run the new engine fails after
it has shown a point to a live view, not only to a progress bar, or read a
Stop or an edit: that run fails with its code), and the result's messages say which engine ran the case and,
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
import contextlib
import math
import os
import re
import threading
import time
from pathlib import Path
from types import SimpleNamespace
from typing import Callable, NamedTuple, Optional

from .. import cycles, script_trust
from ..library import unit_groups
from ..schemas import Channel, LiveEdit, Project, SimMessage, SimResult, SummaryValue
from . import balance
from .battery import CELLS
from .domains import FMU_COMPONENT, ModelInitError, RunContext
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
from .verdict import (
    TRACE_STEP_S,
    Trace,
    Verdict,
    beyond_data,
    first_not_driven,
    judge,
    terminal_checks,
)

ENV_VAR = "LIGHTSIM_ENGINE"
STRICT_VAR = "LIGHTSIM_ENGINE_STRICT"
KINDS = ("cycle", "acceleration")  # the case kinds the new engine runs

# why the new engine's path does not run a case (benchmarks/back_to_back/acceptance.toml)
CODES = {
    "NEW-NOT-FOUND": "the case is not in the project",
    "NEW-CASE-KIND": "a case kind the new engine's path does not run",
    "NEW-LAP": "a lap case",
    "NEW-FS-EVENT": "a Formula Student event case",
    "NEW-BALANCE": "a hybrid's charge-balanced case",
    "NEW-CELLS": "a battery built from cells",
    "NEW-LIMITS": "a value outside its parameter's limits (a study's point can hold one)",
    "NEW-MISSING": "the module lightsim_engine is not installed",
    "NEW-BUILD": "today's model builder refused the model",
    "NEW-FAILED": "the new engine refused or stopped (its own codes follow)",
    "NEW-ERROR": "an exception that is not the engine's own refusal",
    "NEW-TEXT": "a part from a text file in a case the path does not run, or whose file it cannot read "
                "(today's engine reads none)",
    "NEW-SAMPLED": "a PID or Lookup block with a Sample Time longer than today's solver step (today's "
                   "engine holds its output between samples; the new engine runs it at every step)",
    "NEW-LIVE-ERROR-AXIS": "a live run of a project with a table that today's engine reads only in one "
                           "branch of its part, with an axis set to Error whose data does not cover every "
                           "value the new engine reads it at (a live run that has shown points cannot "
                           "start again on today's engine)",
    "NEW-LIVE-PID": "a live run of a project with a PID block the new engine runs at every step, which "
                    "can stop it at its limit (RUN-EVENT-STORM) where today's engine finishes (a live "
                    "run that has shown points cannot start again on today's engine)",
    # quick mode's (a case's mode "quick": the new engine's fast mode)
    "QUICK-NOT-FOUND": "quick mode: the case is not in the project",
    "QUICK-KIND": "quick mode: a case that drives no drive cycle",
    "QUICK-SAMPLED": "quick mode: a project with a sampled part (a Script, an FMU, a Traction Control, a "
                     "PID or Lookup with a Sample Time longer than today's solver step)",
    "QUICK-CELLS": "quick mode: a battery built from cells",
    "QUICK-LIMITS": "quick mode: a value outside its parameter's limits",
    "QUICK-MISSING": "quick mode: the module lightsim_engine is not installed",
    "QUICK-BALANCE": "quick mode: a hybrid's charge-balanced case",
    "QUICK-END": "quick mode: a case that ends at a distance or a lap count",
    "QUICK-BUILD": "quick mode: today's model builder refused the model, or a text component's file "
                   "could not be read",
    "QUICK-FAILED": "quick mode: the engine's fast mode refused or stopped (its own codes follow)",
    "QUICK-TABLE-TIME": "quick mode: a table read past its data for a time its points once a second "
                        "cannot tell from what the run allows (today's verdict could go either way)",
    "QUICK-ERROR": "quick mode: an exception that is not the engine's own refusal",
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
    why = unsupported(project, case_id)
    view = emit is not None and not getattr(emit, "progress_only", False)
    if why is None and view:
        why = live_error_axis(project, case_id)
    if why is None:
        # what a live run did that today's engine, run instead from t = 0,
        # would undo: "shown" (a point or a note to a live view: an emit not
        # marked progress_only, as the AI tools' is, whose bar may start
        # again), "cancel" (a Stop read), "set_param" (a live edit read). A run
        # the new engine fails after any of them is not run again on today's.
        did: set[str] = set()

        def shown(msg: dict) -> None:
            did.add("shown")
            emit(msg)

        def read() -> list[dict]:
            got = list(control())
            did.update(str(m.get("type")) for m in got)
            return got
        try:
            return _ran(run_new(project, case_id, shown if view else emit, control and read), project)
        except Exception as e:  # noqa: BLE001 — the new engine must never break a run
            why = failure(e)
            if texts_of(project):  # (in either mode: today's engine reads none)
                return no_text(case_id, why, project, str(e))
            if strict():
                return refused(case_id, why, str(e))
            if did:
                lost = " and ".join(w for k, w in (("cancel", "the Stop"), ("set_param", "the live edits"))
                                    if k in did)
                return SimResult(caseId=case_id, status="failed", channels=[], messages=[SimMessage(
                    level="error", text=f"The new engine could not finish this live run "
                                        f"[{why.code}]: {why.why} ({e}). It is not run again on "
                                        f"today's engine, which would start it again at t = 0 "
                                        + (f"without {lost} it was given" if lost else
                                           "after the points it has shown")
                                        + f": run the case again (with {ENV_VAR}=today for "
                                          f"today's engine).")])
            result = today(project, case_id, emit, control)
            result.messages.insert(0, SimMessage(
                level="warning", text=f"The new engine could not run this case [{why.code}] "
                                      f"({e}); it ran on today's engine instead."))
            _refresh_status(result)
            return result
    if texts_of(project):
        return no_text(case_id, why, project)
    if strict():
        return refused(case_id, why)
    result = today(project, case_id, emit, control)
    result.messages.insert(0, SimMessage(
        level="info", text=f"Run on today's engine, not the new one ({ENV_VAR}=new) "
                           f"[{why.code}]: {why.why}."))
    return result


def live_error_axis(project: Project, case_id: str) -> Optional[Refusal]:
    """A live run's refusal, decided before the run (nothing is run first,
    so a Stop is read from the start): a table that today's engine reads
    only in one branch of its part (BRANCH_READS) with an axis set to Error
    whose data does not cover every value the new engine reads it at, which
    it reads in every branch (_covers): the new engine would stop at its
    first read past the data, today's only when its own read is, and a live
    run that has shown points is not run again on today's engine. Or a PID
    block the new engine runs at every step (one with a longer Sample Time
    is refused before, NEW-SAMPLED): at its limit for a long stretch it can
    stop the new engine (RUN-EVENT-STORM) where today's engine finishes."""
    pid = next((el for s in project.systems for el in s.elements if el.componentDefId == "control.pid"), None)
    if pid is not None:
        return Refusal("NEW-LIVE-PID", f"the PID block {pid.label!r}, which the new engine runs at "
                                       f"every step, can stop it at its limit (RUN-EVENT-STORM)")
    try:
        ctx = _context(project, case_id)
    except Exception:  # noqa: BLE001 — the run says it (NEW-BUILD)
        return None
    for (el_id, key), m in ctx.maps.items():
        rule = BRANCH_READS.get((ctx.model.cdef_of[el_id].id, key))
        for a in m.error_axes:
            if rule is not None and a < len(rule.axes) and rule.axes[a][1] not in (SAME, NEVER) \
                    and not _covers(ctx, el_id, key, a):
                lo, hi = m.ranges[a] or (0.0, 0.0)
                return Refusal("NEW-LIVE-ERROR-AXIS", (
                    f"{m.name}: its {m.uses[a].axis} axis is set to Error and its data ({lo:g} to "
                    f"{hi:g} {m.uses[a].unit}) does not cover every value the new engine reads it at "
                    f"(today's engine reads it {rule.where})"))
    return None


def _covers(ctx, el_id: str, key: str, axis: int) -> bool:
    """Whether a table's axis covers every value the new engine reads it
    at, in every branch of its part: an E-Motor's full-load torque and loss
    map from 0 to its maximum speed (above it the inverter is off: see
    KNOWN-LIMITS for a motor driven above it), its loss map from 0 to the
    most torque it gives or takes (its full-load torque, as a generator
    times its Q4 share, or its drag torque; with no axis of those two set
    to Linear, which reads past their values); an Engine's fuel map between
    its full-load curve's first and last speed (it is read at its speed held
    there) and from 0 to its most full-load torque. Any other: False."""
    m = ctx.maps[(el_id, key)]
    mc, ec = ctx.motors.get(el_id), ctx.engines.get(el_id)
    if mc is not None and (key, axis) in (("full_load_torque", 1), ("power_loss", 0)):
        lo, hi = 0.0, mc.max_rpm
    elif mc is not None and (key, axis) == ("power_loss", 1) and not any(mc.full_load.linear + mc.drag.linear):
        fl = max(v for _, row in mc.full_load.pts for _, v in row) if len(mc.full_load.policy) == 2 \
            else max(v for _, v in mc.full_load.pts)
        lo, hi = 0.0, max(abs(fl) * max(1.0, abs(mc.q4_scale)), max(abs(v) for _, v in mc.drag.pts))
    elif ec is not None and key == "fuel_map" and ec.full_load.ranges[0] is not None:
        lo, hi = ec.full_load.ranges[0] if axis == 0 else (0.0, max(v for _, v in ec.full_load.pts))
    else:
        return False
    return m.edge(axis, lo) is None and m.edge(axis, hi) is None


def _ran(result: SimResult, project: Optional[Project] = None) -> SimResult:
    if not wanted() and project is not None and (texts := texts_of(project)):
        label = next(iter(texts.values()))[0]
        text = (f"Run on the new engine: today's engine reads no text components ({label!r} "
                f"reads its equations from a file).")
    else:
        text = f"Run on the new engine ({ENV_VAR}=new)."
    result.messages.insert(0, SimMessage(level="info", text=text))
    return result


# ---- text components ----------------------------------------------------------

TEXT_LIMIT_BYTES = 1_000_000  # a text component's file at most (one component is a few kB)


class TextFileError(Exception):
    """A text component's file that cannot be read (NEW-TEXT)."""


def texts_of(project: Project) -> dict[str, tuple[str, str]]:
    """The parts that read their equations from a text file: element id ->
    (its label, the file's path as the project names it)."""
    return {el.id: (el.label, el.textFile) for s in project.systems for el in s.elements
            if getattr(el, "textFile", None)}


def read_texts(project: Project) -> dict[str, str]:
    """Each text component's text, read from its file (a path that is not
    absolute from the user's projects folder). Raises TextFileError."""
    from ..paths import projects_dir

    out = {}
    for el_id, (label, name) in texts_of(project).items():
        path = Path(name).expanduser()
        if not path.is_absolute():
            path = projects_dir() / path
        try:
            if path.stat().st_size > TEXT_LIMIT_BYTES:
                raise TextFileError(f"the text file of {label!r} ({path}) is over "
                                    f"{TEXT_LIMIT_BYTES // 1000} kB")
            out[el_id] = path.read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError) as e:
            raise TextFileError(f"the text file of {label!r} could not be read ({path}: "
                                f"{getattr(e, 'strerror', None) or e})") from e
    return out


def no_text(case_id: str, why: Refusal, project: Project, detail: str = "") -> SimResult:
    """A case with a text component the new engine's path did not run: it
    cannot run on today's engine, which would run the built-in part in the
    text's place without a word."""
    label = next(iter(texts_of(project).values()))[0]
    more = f" ({detail})" if detail else ""
    codes = f"[{why.code}]" + (" [NEW-TEXT]" if why.code != "NEW-TEXT" else "")
    return SimResult(caseId=case_id, status="failed", channels=[], messages=[SimMessage(
        level="error", text=f"Not run {codes}: {why.why}{more}; {label!r} reads its "
                            f"equations from a text file, which only the new engine reads.")])


def replay(project: Project, case_id: str, options: dict, edits: list[LiveEdit]) -> SimResult:
    """A run of the new engine made again (core.replay): with its solver
    options, each live edit taken again where the run took it, so the same
    result bit for bit. A run the engine cannot make again fails with the
    reason's code."""
    try:
        if "quickStep" in options:  # (a quick run)
            return run_quick(project, case_id, float(options["quickStep"]))
        return _ran(run_new(project, case_id, edits=edits, options=options), project)
    except Exception as e:  # noqa: BLE001 — told as the reason's code
        return refused(case_id, failure(e), str(e))


# ---- quick mode ----------------------------------------------------------------

# the summary rows quick mode gives: the engine's figures that sum or
# integrate over the run (its key, without a part's id), not the peaks,
# minima and times at a limit, which follow the full run's driver
QUICK_ROWS = re.compile(
    r"^(?:[^.]+\.)?(?:final_soc_pct|energy_delivered_kwh|energy_recuperated_kwh|internal_losses_kwh"
    r"|energy_used_kwh|fuel_used_kg)$"
    r"|^(?:distance_km|consumption_kwh_per_100km|consumption_ac_kwh_per_100km|mpge_ac|range_km"
    r"|fuel_consumption_l_per_100km|fuel_consumption_corrected_l_per_100km|co2_g_per_km"
    r"|battery_energy_change_pct_of_fuel|simulated_duration_s"
    r"|phase\d+_(?:distance_km|consumption_kwh_per_100km|fuel_consumption_l_per_100km))$")
SAMPLED = ("signal.script", FMU_COMPONENT, "control.traction")


def quick_unsupported(project: Project, case_id: str) -> Optional[Refusal]:
    """Why quick mode does not run this case, or None when it does."""
    case = next((c for c in project.cases if c.id == case_id), None)
    if case is None:
        return Refusal("QUICK-NOT-FOUND", "the case was not found")
    if case.kind != "cycle" or case.fsEvent:
        kind = (f"a Formula Student {case.fsEvent} event" if case.fsEvent else
                {"acceleration": "an acceleration test", "performance": "a performance test"}.get(
                    case.kind, f"a {case.kind} case"))
        return Refusal("QUICK-KIND", f"it prescribes a drive cycle's speed, and {kind} drives none")
    if (case.endDistance or 0) > 0 or (case.endLaps or 0) > 0:
        return Refusal("QUICK-END", "it runs the cycle's whole duration, not to a case's Distance "
                                    "or Laps (from its Start line)")
    elements = [el for s in project.systems for el in s.elements]
    if balance.applies(project, case):
        return Refusal("QUICK-BALANCE", "it does not charge-balance a hybrid's case (set Charge "
                                        "balance off to run it once)")
    sampled = next((el for el in elements if el.componentDefId in SAMPLED), None)
    if sampled is not None:
        return Refusal("QUICK-SAMPLED", f"it runs no sampled part (a Script, an FMU, a Traction "
                                        f"Control: {sampled.label!r})")
    if (held := held_block(project, case)) is not None:
        return Refusal("QUICK-SAMPLED", f"it runs every block at every step, and {held}")
    overrides = getattr(case, "parameterOverrides", None) or {}
    if any(el.componentDefId == "battery.generic"
           and {**el.parameterOverrides, **overrides.get(el.id, {})}.get("pack_model") == CELLS
           for el in elements):
        return Refusal("QUICK-CELLS", "it does not run a battery built from cells")
    from ..validation import value_problems  # (validation imports this package)
    if problems := value_problems(project, case_id):
        return Refusal("QUICK-LIMITS", f"{'; '.join(problems)} (outside the limits Data Checks refuse "
                                       f"a single run for)")
    try:
        import lightsim_engine  # noqa: F401
    except ImportError:
        return Refusal("QUICK-MISSING", "the new engine is not installed")
    return None


HELD = ("control.pid", "signal.lookup")  # the blocks today's engine samples that the new engine runs


def held_block(project: Project, case) -> Optional[str]:
    """A PID or Lookup block whose Sample Time is longer than today's solver
    step, in words, or None: today's engine runs it only at multiples of its
    Sample Time and holds its output in between (domains.ControlSlave), the
    new engine and its fast mode run it at every step."""
    from . import core  # (core imports this module; its solver step, as today's run takes it)
    from .stability import solver_step

    overrides = getattr(case, "parameterOverrides", None) or {}
    timed = [(el, float({**el.parameterOverrides, **overrides.get(el.id, {})}.get("sample_time_s") or 0))
             for s in project.systems for el in s.elements if el.componentDefId in HELD]
    timed = [(el, ts) for el, ts in timed if ts > 0]
    if not timed:
        return None
    try:
        choice = solver_step(build_model(project, {}, overrides), core.MAX_SUBSTEP)
    except ModelError:
        return None  # (the path says it: NEW-BUILD)
    step = choice.step if choice else core.MAX_SUBSTEP
    el, ts = max(timed, key=lambda x: x[1])
    if ts <= step:
        return None
    return (f"today's engine holds the output of {el.label!r} between its samples (its Sample Time "
            f"{ts:g} s, today's solver step {step * 1000:g} ms)")


class Undecided(Exception):
    """Quick mode's time past a table's data too near what the run allows
    for its points to tell (_decided)."""


def _decided(ctx, stretches: list, t_end: float, step: float) -> None:
    """Raises Undecided where a table's time past its data, counted at fast
    mode's points (once a second), lies within two of its steps a stretch
    (its stretches past the data at those points: ``stretches``) of what
    the verdict allows (verdict.beyond_data), where today's count, every
    solver step, could fall on either side of it."""
    from .verdict import OUTSIDE_MIN_S, OUTSIDE_SHARE
    allowance = max(OUTSIDE_MIN_S, OUTSIDE_SHARE * t_end)
    for use, runs in stretches:
        if ctx.model.cdef_of[use.el_id].id != "signal.lookup" and use.outside_s > 0.0 and abs(use.outside_s - allowance) <= 2.0 * step * max(1, runs):
            raise Undecided(f"the {use.what} of {ctx.model.elements[use.el_id].label!r}: {use.outside_s:.3g} s "
                            f"past its data at fast mode's points, in {runs} stretch(es), against the "
                            f"{allowance:.3g} s the run allows")


def quick_failure(e: Exception) -> Refusal:
    """The reason code of an exception out of ``run_quick``."""
    if isinstance(e, Undecided):
        return Refusal("QUICK-TABLE-TIME", "its time past a table's data is too near what the run allows "
                                           "to judge at its points once a second")
    why = failure(e)
    code = {"NEW-FAILED": "QUICK-FAILED", "NEW-BUILD": "QUICK-BUILD", "NEW-TEXT": "QUICK-BUILD"}
    return Refusal(code.get(why.code, "QUICK-ERROR"), why.why.replace("the new engine", "fast mode"))


def quick(project: Project, case_id: str, emit: Optional[EmitFn], control: Optional[ControlFn],
          full: TodayFn) -> SimResult:
    """A case's quick mode (its mode "quick"), whatever LIGHTSIM_ENGINE
    says: the new engine's fast mode (run_quick). A case it does not run
    runs in ``full`` instead, with a warning carrying the reason's code (in
    strict mode it fails with it)."""
    why, detail = quick_unsupported(project, case_id), ""
    if why is None:
        try:
            return run_quick(project, case_id)
        except Exception as e:  # noqa: BLE001 — told as the reason's code
            why, detail = quick_failure(e), str(e)
    if strict():
        return refused(case_id, why, detail)
    result = full(project, case_id, emit, control)
    more = f" ({detail})" if detail else ""
    result.messages.insert(0, SimMessage(
        level="warning", text=f"Not run in quick mode [{why.code}]: {why.why}{more}; it ran in full "
                              f"instead."))
    _refresh_status(result)
    return result


def run_quick(project: Project, case_id: str, step: Optional[float] = None) -> SimResult:
    """Quick mode's run of a case: the new engine's fast mode (DESIGN.md
    section 10: the cycle's speed prescribed, the Driver's pedal worked out
    back from it, limits flagged, not acted on), as today's SimResult: its
    channels, the summary rows that sum or integrate over the run
    (QUICK_ROWS), each flag a warning, and what ran it. No verdict on the
    speed trace (it is the cycle's), no peaks, no reports. Raises when the
    engine cannot build or run it."""
    import lightsim_engine

    from . import core  # (core imports this module)

    case = next(c for c in project.cases if c.id == case_id)
    try:
        ctx = _context(project, case_id)  # (today's labels, units and checks)
    except ModelInitError as e:  # as today's engine says it
        return SimResult(caseId=case_id, status="failed", channels=[],
                         messages=[SimMessage(level="error", text=t) for t in e.messages])
    model = ctx.model
    run = lightsim_engine.fast(project.model_dump_json(), case_id, cycles_dir=str(cycles.DIR),
                               step=step, texts=read_texts(project))
    fast = run.report["fast"]
    times = [float(t) for t in run.times]
    messages = [SimMessage(level="warning", text=w) for w in core.model_warnings(model)]
    messages += [SimMessage(level="info", text=t)
                 for t in core.case_notes(project, model, case, max(0.0, float(case.duration)))]
    messages.insert(0, SimMessage(level="info", text=(
        f"Quick mode: the new engine's fast mode, every {fast['step']:g} s: the cycle's speed is "
        f"prescribed and the Driver's pedal worked out back from it, so the totals below come "
        f"without the peaks, the speed trace's verdict or the reports; a limit the car would "
        f"have met is flagged, not acted on.")))
    for f in fast["flags"]:
        messages.append(SimMessage(level="warning", text=(
            f"Quick mode: {f['label']} from t = {f['t_start']:.1f} s to {f['t_end']:.1f} s: the car "
            f"could not have followed the cycle there, so the totals take more than it could give; "
            f"run the case in full to see what it does.")))
    # today's checks of the run that fast mode's motion leaves as they are,
    # in today's words: what the RunContext said while it was built, the
    # Ambient's air outside the usual (as the full path asks it, _runtime),
    # the tables the run read past their data (fast mode's records at its
    # points, _tables), judged as today's verdict judges them, which marks
    # the consumptions not valid
    ctx.read_ambient(times[0] if times else 0.0)
    messages += ctx.rt.messages
    stretches = _tables(ctx, run, messages)
    _decided(ctx, stretches, times[-1] if times else 0.0, float(fast["step"]))
    beyond = beyond_data(core.judged_uses(ctx), times[-1] if times else 0.0,
                         lambda el: f"{model.cdef_of[el].name} '{model.elements[el].label}'")
    messages += [SimMessage(level="warning", text=text) for text, _ in beyond]
    summary = [SummaryValue(key=f["key"], label=f["label"], value=f["value"], unit=f["unit"])
               for f in run.figures if QUICK_ROWS.match(f["key"])]
    if beyond:
        fuel = sum(s.value for s in summary if s.key.endswith("fuel_used_kg"))
        core.not_valid_last(_NO_BOOKS, Verdict(beyond_reason="; ".join(r for _, r in beyond)),
                            summary, {}, set(), lab_base(summary, fuel <= 0 and not ctx.fuelcells))
    fill_keys(summary, project)
    result = SimResult(caseId=case_id, status=core.status_of(messages, False),
                       channels=_channels(model, run, times), summary=summary, messages=messages,
                       references=check_references(case.references, summary))
    result.engine, result.engineVersion = "new", lightsim_engine.version()
    result.solverOptions = {"quickStep": float(fast["step"])}  # (quick mode, its step)
    return result


# a run with no energy books: core.not_valid_last's check of their closure
# says nothing (quick mode's)
_NO_BOOKS = SimpleNamespace(throughput_wh=0.0, residual_wh=0.0)


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
    if isinstance(e, TextFileError):
        return Refusal("NEW-TEXT", str(e))
    if isinstance(e, TableError):
        return Refusal("NEW-FAILED", f"the new engine's run read a table past an Error axis ({e})")
    return Refusal("NEW-ERROR", f"{type(e).__name__} in the new engine's path")


def refused(case_id: str, why: Refusal, detail: str = "") -> SimResult:
    """Strict mode's result for a case the new engine's path did not run."""
    more = f" ({detail})" if detail else ""
    return SimResult(caseId=case_id, status="failed", channels=[], messages=[SimMessage(
        level="error", text=f"Not run [{why.code}]: {why.why}{more}; with {STRICT_VAR}=1 no "
                            f"case runs on today's engine instead.")])


def unsupported(project: Project, case_id: str) -> Optional[Refusal]:
    """Why the new engine does not run this case, or None when it does."""
    case = next((c for c in project.cases if c.id == case_id), None)
    if case is None:
        return Refusal("NEW-NOT-FOUND", "the case was not found")
    if case.kind == "lap":
        return Refusal("NEW-LAP", "it does not run lap cases")
    if case.kind not in KINDS:
        return Refusal("NEW-CASE-KIND", f"it does not run {case.kind} cases yet")
    if case.fsEvent:
        return Refusal("NEW-FS-EVENT", "it does not score Formula Student events")
    elements = [el for s in project.systems for el in s.elements]
    if balance.applies(project, case):
        return Refusal("NEW-BALANCE", "it does not charge-balance a hybrid's case")
    overrides = getattr(case, "parameterOverrides", None) or {}
    if any(el.componentDefId == "battery.generic"
           and {**el.parameterOverrides, **overrides.get(el.id, {})}.get("pack_model") == CELLS
           for el in elements):
        return Refusal("NEW-CELLS", "it does not run a battery built from cells")
    if (held := held_block(project, case)) is not None:
        return Refusal("NEW-SAMPLED", f"it runs every block at every step, and {held}")
    from ..validation import value_problems  # (validation imports this package)
    if problems := value_problems(project, case_id):  # (a study's point: Data Checks never saw it)
        return Refusal("NEW-LIMITS", f"{'; '.join(problems)} (outside the limits Data Checks refuse a "
                                     f"single run for; today's engine clamps many such values, a pressure "
                                     f"or a CG height at 0, a mass at 1 kg, where the new engine would "
                                     f"read them as typed)")
    try:
        import lightsim_engine  # noqa: F401
    except ImportError:
        return Refusal("NEW-MISSING", "the new engine is not installed")
    return None


# the gears and shafts: a part whose input power is its channel sig_power
_GEARS = ("mech.shaft", "mech.final_drive", "mech.gearbox", "mech.differential",
          "mech.transfer_case")


def run_new(project: Project, case_id: str, emit: Optional[EmitFn] = None,
            control: Optional[ControlFn] = None, edits: list[LiveEdit] = (),
            options: Optional[dict] = None) -> SimResult:
    """The case on the new engine, as today's SimResult: live (its points
    to ``emit``, its ``control`` read: _Live) when either is given; with a
    run's live ``edits`` made again where it took them and its solver
    ``options`` (a replay). Raises when the new engine cannot build or run
    it."""
    import lightsim_engine

    from . import core  # (core imports this module)

    case = next(c for c in project.cases if c.id == case_id)
    # today's model gives what the engine's run lacks: the labels and units of
    # the channels, the Script blocks' code, ports and parameters, and, in a
    # RunContext built and never stepped, the parts as today's engine reads
    # them (each battery's capacity and checks, each motor's maximum speed,
    # each driveline's parts, each table's axes, each tank's start)
    try:
        ctx = _context(project, case_id)
    except ModelInitError as e:  # as today's engine says it
        return SimResult(caseId=case_id, status="failed", channels=[],
                         messages=[SimMessage(level="error", text=t) for t in e.messages])
    model = ctx.model
    ctx.performance = case.kind != "cycle"
    ctx.full_throttle = case.kind == "acceleration"
    t_end = max(0.0, float(case.duration))
    ctx.end_distance, laps_warning = core.end_distance(ctx, case, t_end)
    lanes = RunRecorder(ctx).lanes
    texts = read_texts(project)
    built = lightsim_engine.build(project.model_dump_json(), case_id, cycles_dir=str(cycles.DIR),
                                  texts=texts)
    plan = _Plan(model, ctx, built, lanes)
    opts = options or {}
    step = float(opts.get("traceStep", ACCEL_TRACE_STEP_S if ctx.performance else TRACE_STEP_S))
    kw = dict(rtol=float(opts.get("rtol", 1e-6)), atol=float(opts.get("atol", 1e-8)),
              output_step=opts.get("outputStep"), trace=plan.names, trace_step=step,
              windows=plan.windows)
    # a replay's live edits, each as the engine's parameter changes, taken
    # where the run took them (one it did not make: its note again)
    again = _Edits(project, case_id, built, texts) if edits else None
    replayed = []  # as _Live.taken
    for e in edits:
        if e.note:
            replayed.append((e.elementId, e.key, e.value, e.t, False, e.note))
            continue
        changes = again.changes(e.elementId, e.key, e.value)
        again.keep(e.elementId, e.key, e.value, changes)
        replayed.append((e.elementId, e.key, e.value, e.t, bool(changes), None))
        if changes:
            kw.setdefault("edits", []).append((e.t, changes))
    live = _Live(project, case, model, built, emit, control, texts) if emit or control else None

    def go(**more):
        return live.drive(lambda **c: built.simulate(**kw, **more, **c)) if live else \
            built.simulate(**kw, **more)
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
    with contextlib.ExitStack() as workers:
        hosts = {}
        if specs:
            # the same checks as today's engine: only approved code runs, and it
            # runs in the locked-down worker process (script_trust.py, sandbox.py);
            # the engine's own Script runner is not used from the app until the
            # owner has decided it may replace the worker (native_scripts=False)
            try:
                script_trust.check([(sp.label, sp.code) for sp in specs])
            except script_trust.ScriptsNotApproved as e:  # as today's engine says it
                return SimResult(caseId=case_id, status="failed", channels=[],
                                 messages=[SimMessage(level="error", text=str(e))])
            sandbox = workers.enter_context(ScriptSandbox(specs))

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
            hosts.update(scripts={sp.el_id: runner(sp) for sp in specs}, native_scripts=False)
        fmus = [el_id for el_id in model.signal_blocks if model.cdef_of[el_id].id == FMU_COMPONENT]
        if fmus:
            try:
                hosts["fmus"] = {el_id: _fmu_host(model, el_id, workers) for el_id in fmus}
            except ModelInitError as e:  # as today's engine says it
                return SimResult(caseId=case_id, status="failed", channels=[],
                                 messages=[SimMessage(level="error", text=t) for t in e.messages])
        run = go(**hosts)
    stopped = run.report["stopped"] is not None
    # the live edits it took (each where the engine took it), and the notes
    # of those it did not make
    taken = live.made(run.report["edits"]) if live else _made(replayed, run.report["edits"], model)
    result = _result(project, case, model, ctx, built, run, plan, lanes, laps_warning,
                     list(dropped.values()), stopped, [e for e in taken if not e.note])
    result.messages += _notes(taken)
    # what ran it
    result.engine, result.engineVersion = "new", lightsim_engine.version()
    result.solverOptions = {"rtol": float(run.report["rtol"]), "atol": float(run.report["atol"]),
                            "outputStep": float(run.report["output_step"]), "traceStep": step}
    result.liveEdits = taken
    _refresh_status(result)
    return result


def _made(taken: list[tuple], report_edits: list, model) -> list[LiveEdit]:
    """The live edits a run took, in order, each at the simulated time the
    run had reached when it took it: one given to the engine at the time
    the engine took it (the engine takes and makes its changes in order:
    the report's are the first of those it was given; one it was given and
    did not make came too late); one the run did not make with its note,
    which says why. ``taken``: (element, key, value, the run's time, given
    to the engine, its note or None)."""
    report = iter(report_edits)
    out = []
    for el_id, key, value, t, given, note in taken:
        if given:
            m = next(report, None)
            if m is None:
                note = (f"'{model.elements[el_id].label}.{key}' changed too late in the run to "
                        f"take effect: it takes effect on the next run [{NEXT_RUN}].")
            else:
                t = float(m["t"])
        out.append(LiveEdit(t=t, elementId=el_id, key=key, value=value, note=note))
    return out


def _notes(edits: list[LiveEdit]) -> list[SimMessage]:
    """The result's notes of the live edits a run did not make, each once:
    a replay gives them again from its snapshot's list."""
    told = dict.fromkeys(e.note for e in edits if e.note)
    return [SimMessage(level="info", text=t) for t in told]


# output points the engine may run ahead of a live run's reader: a Stop or
# a live edit lands within a few output points of the point the view shows
# (and a paced run's engine is held near the clock)
LIVE_QUEUE = 2
POLL_S = 0.02  # how often a live run's control is read, s


class _Later(Exception):
    """A live edit the run cannot take: it takes effect on the next run."""


def _context(project: Project, case_id: str) -> RunContext:
    """Today's model of a case's project, in a RunContext built and never
    stepped: the parts as today's engine reads them (run_new)."""
    case = next(c for c in project.cases if c.id == case_id)
    overrides = getattr(case, "parameterOverrides", None) or {}
    model = build_model(project, {}, overrides)
    return RunContext(project, model, Runtime(model, None), {}, overrides, workers=False)


def _fmu_host(model, el_id: str, workers: contextlib.ExitStack) -> Callable:
    """An FMU part's host for the engine (its `fmus`): today's locked-down
    FMPy worker (fmu/sandbox.py, one process a part, closed with
    ``workers``), stepped as an FMU host steps a co-simulation FMU (FMPy's
    simulate_fmu and fmusim: the inputs set at a communication point, the
    step to the next, the outputs read there). The engine calls it at the
    start and at each multiple t of the part's Communication Step: it steps
    the FMU from the last call's time to t with the inputs of the last call
    (at the start it only sets them) and returns the outputs at t, which the
    engine holds until the next call. A call before the last one's time is
    a run from its start again (an acceleration test's second run, to its
    finish): the FMU starts again too, in a new worker. Raises
    ModelInitError with today's words when the FMU cannot be set up (today's
    RunContext's)."""
    from ..fmu import FmuError, FmuFileError  # (the FMU modules import this package's worker)
    from ..fmu.block import spec_for
    from ..fmu.sandbox import FmuSandbox

    el = model.elements[el_id]
    ports = list(el.dynamicPorts or [])
    try:
        spec = spec_for(el_id, el.label, model.params_of[el_id], ports)
        box = [workers.enter_context(FmuSandbox(spec))]
    except (FmuError, FmuFileError) as e:
        raise ModelInitError([str(e)]) from e
    except Exception as e:  # noqa: BLE001 — a plain message, as today's engine gives it
        raise ModelInitError([f"FMU '{el.label}' could not be set up: {e}"]) from e
    ins = [p.id for p in ports if p.direction == "input"]
    outs = [p.id for p in ports if p.direction == "output"]
    last: list[tuple[float, list[float]]] = []  # (the last call's time, its inputs)

    def step(t: float, h: float, inputs: dict) -> dict:
        u = [float(inputs.get(k) or 0.0) for k in ins]
        if last and t < last[0][0]:  # the run again from its start: the FMU from its own
            box[0].close()
            box[0] = workers.enter_context(FmuSandbox(spec))
            last.clear()
        t0, u0 = last[0] if last else (t, u)
        values = box[0].step(t0, t - t0, u0)
        last[:] = [(t, u)]
        return dict(zip(outs, values))
    return step


def _checked(ctx: RunContext) -> str:
    """What the path's checks after a run read of today's parts as they
    were at the run's start, where today's engine checks them at each step
    with the values then: the Ambient's air (_runtime), each battery's
    minimum SOC, charge power, power cap and checks (_terminal, _supply),
    each E-Motor's and Engine's maximum speed (_over_speed), each table's
    axes (_tables), the Vehicle's CG height and Downforce Area (the lift). A
    live edit that changes any of it takes effect on the next run (_Edits)."""
    amb = ctx.params(ctx.amb_id) if ctx.amb_id else {}
    return repr((amb.get("temperature_C"), amb.get("pressure_kPa"),
                 ctx.load_shift() if ctx.veh_id else None,
                 [(b.el_id, b.min_soc, b.max_charge_w, b.p_cap_w, b.check)
                  for b in ctx.batteries.values()],
                 [(el, m.speed_use.edge) for el, m in (*ctx.motors.items(), *ctx.engines.items())],
                 sorted((str(k), m.ranges) for k, m in ctx.maps.items())))


class _Edits:
    """Live edits as the new engine's parameter changes: the project with
    the edits so far built again (as cheap as a build: no run), its
    parameters against the run's. An edit the engine runs another
    structure for (or does not build), or one that changes what the path's
    checks after the run read as at its start (_checked), is for the next
    run."""

    def __init__(self, project: Project, case_id: str, built, texts: Optional[dict] = None):
        self.project = project.model_copy(deep=True)  # with the edits kept so far
        self.case_id, self.built, self.texts = case_id, built, texts or {}
        self.params = dict(built.params())  # the run's, as the edits leave them
        self.checked = _checked(_context(self.project, case_id))

    def changes(self, el_id: str, key: str, value) -> dict:
        """The engine's parameter changes an edit makes on top of those
        before it (SI, by engine name; none: nothing the engine reads), the
        edit kept for the next ones; raises _Later."""
        import lightsim_engine

        trial = self.project.model_copy(deep=True)
        case = next(c for c in trial.cases if c.id == self.case_id)
        case.parameterOverrides.setdefault(el_id, {})[key] = value
        try:
            b = lightsim_engine.build(trial.model_dump_json(), self.case_id,
                                      cycles_dir=str(cycles.DIR), texts=self.texts)
            checked = _checked(_context(trial, self.case_id))
        except Exception as e:  # noqa: BLE001 — the engine's refusal of the value
            raise _Later(f"the new engine takes it on the next run ({e})") from e
        new = b.params()
        if (b.report["structure_key"] != self.built.report["structure_key"]
                or new.keys() != self.params.keys()):
            raise _Later("the new engine builds another model for it: it takes effect on the "
                         "next run")
        if checked != self.checked:
            raise _Later("the new engine's checks after its run read it as it was at the run's "
                         "start: it takes effect on the next run")
        return {n: v for n, v in new.items()
                if v != self.params[n] and not (math.isnan(v) and math.isnan(self.params[n]))}

    def keep(self, el_id: str, key: str, value, changes: dict) -> None:
        """An edit the run took: the next ones build on it."""
        case = next(c for c in self.project.cases if c.id == self.case_id)
        case.parameterOverrides.setdefault(el_id, {})[key] = value
        self.params.update(changes)


class _Live:
    """A live run on the new engine (an emit or a control: the app's live
    run, a study's point). The engine runs in a thread of its own with a
    RunControl (one stop and progress hook, safe to use while Python holds
    its lock: its queue and flags are the engine's own) while this thread
    passes its output points on as today's "step" messages, at the case's
    pace (the engine waits on its bounded queue), and reads the run's
    control: a cancel stops the run where it is, a set_param is a live
    edit of a runtime parameter (_Edits; a structural one, as the library
    or the new engine says, takes effect on the next run, as today's says)."""

    def __init__(self, project: Project, case, model, built, emit, control, texts: dict):
        import lightsim_engine

        self.case, self.model, self.emit, self.control = case, model, emit, control
        self.pace = max(0.0, float(getattr(case, "realtimeFactor", 0.0) or 0.0))
        self.rc = lightsim_engine.RunControl(queue=LIVE_QUEUE if emit else 0)
        self.edits = _Edits(project, case.id, built, texts)
        # the edits taken, in order: (element, key, value, the run's time,
        # given to the engine, the note of one the run does not make: _made)
        self.taken: list[tuple[str, str, object, float, bool, Optional[str]]] = []
        self.said: set[str] = set()
        ports = {(el_id, p.id) for el_id, cdef in model.cdef_of.items()
                 for p in [*cdef.ports, *(model.elements[el_id].dynamicPorts or [])]}
        self.ports = ports
        self.keep: Optional[list[tuple[int, str]]] = None  # the result's channels in a point

    def drive(self, simulate):
        """Runs ``simulate(control=...)`` and returns its Run."""
        out: dict = {}

        def work():
            try:
                out["run"] = simulate(control=self.rc)
            except BaseException as e:  # noqa: BLE001 — raised again below
                out["error"] = e

        worker = threading.Thread(target=work, name="lightsim-new-engine-run", daemon=True)
        t0 = time.monotonic()
        worker.start()
        try:
            while True:
                got = self.rc.points(POLL_S)
                for t, values in got:
                    self._pace(t, t0)
                    self._step(t, values)
                self._read()
                if not worker.is_alive() and not got:
                    break
        except BaseException:
            self.rc.cancel()
            raise
        finally:
            self.rc.close()  # (from here the run waits for no reader)
            worker.join()
        if "error" in out:
            raise out["error"]
        return out["run"]

    def _pace(self, t: float, t0: float) -> None:
        while self.pace > 0 and not self.rc.cancelled:
            lag = t / self.pace - (time.monotonic() - t0)
            if lag <= 0:
                return
            time.sleep(min(POLL_S, lag))
            self._read()

    def _step(self, t: float, values) -> None:
        if self.emit is None:
            return
        if self.keep is None:  # (the channels the result keeps: those with a port)
            self.keep = [(k, n) for k, n in enumerate(self.rc.channels)
                         if ":" in n and tuple(n.split(":", 1)) in self.ports]
        pct = min(100.0, round(100.0 * t / max(float(self.case.duration), 1e-9), 1))
        self.emit({"type": "step", "t": t, "pct": pct,
                   "values": {n: _number(values[k]) for k, n in self.keep}})

    def _read(self) -> None:
        for msg in self.control() if self.control else ():
            if msg.get("type") == "cancel":
                self.rc.cancel()
            elif msg.get("type") == "set_param":
                self._edit(str(msg.get("elementId")), str(msg.get("key")), msg.get("value"))

    def _edit(self, el_id: str, key: str, value) -> None:
        model = self.model
        if el_id not in model.params_of:
            return  # (no such part: as today's, nothing)
        pdef = next((p for p in model.cdef_of[el_id].parameters if p.key == key), None)
        t = self.rc.t
        try:
            if pdef is not None and pdef.variability == "fixed":
                raise _Later("structural parameters take effect on the next run")
            if model.cdef_of[el_id].id == FMU_COMPONENT:  # (its worker read them at its start)
                raise _Later("FMU start values take effect on the next run" if key != "sample_time_s"
                             else "an FMU's Communication Step takes effect on the next run on the "
                                  "new engine")
            if not isinstance(value, (bool, int, float, str)):  # (the app sends no other)
                raise _Later("a table takes effect on the next run on the new engine")
            if key == "sample_time_s" and model.cdef_of[el_id].id in HELD and isinstance(value, (int, float)):
                trial = self.edits.project.model_copy(deep=True)
                case = next(c for c in trial.cases if c.id == self.edits.case_id)
                case.parameterOverrides.setdefault(el_id, {})[key] = value
                held = held_block(trial, case)
                if held:  # (the next run starts on today's engine for it: NEW-SAMPLED)
                    raise _Later(f"{held}: it takes effect on the next run, which runs on today's engine")
            changes = self.edits.changes(el_id, key, value)
            if changes:
                try:
                    self.rc.set_params(changes)
                except ValueError as e:  # the engine's words
                    raise _Later(f"the new engine takes it on the next run ({e})") from e
            self.taken.append((el_id, key, value, t, bool(changes), None))
            self.edits.keep(el_id, key, value, changes)
        except _Later as e:
            note = f"'{model.elements[el_id].label}.{key}' changed — {e} [{NEXT_RUN}]."
            if isinstance(value, (bool, int, float, str)):  # (a LiveEdit holds a scalar)
                self.taken.append((el_id, key, value, t, False, note))
            self._say(note)

    def _say(self, text: str) -> None:
        if text in self.said:  # (once, as today's)
            return
        self.said.add(text)
        if self.emit:
            self.emit({"type": "message", "level": "info", "text": text})

    def made(self, report_edits: list) -> list[LiveEdit]:
        """The live edits the run took (_made), the note of one that came
        too late said now."""
        edits = _made(self.taken, report_edits, self.model)
        for e in edits:
            if e.note:
                self._say(e.note)
        return edits


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
        # the Vehicle's axle loads: when its wheels lift off the road (_lift)
        self.loads = {side: want(f"{ctx.veh_id}:sig_load_{side.lower()}") if ctx.veh_id else None
                      for side in ("Front", "Rear")}
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
        # what each table rule of today's reads (BRANCH_READS' uses), on the
        # trace: its time past the data counted there (_branch_share)
        for el_id, cdef in model.cdef_of.items():
            for (comp, _), rule in BRANCH_READS.items():
                if comp == cdef.id:
                    for u in rule.uses:
                        var(el_id, u)
        # each checked battery's power: its highest average over its check's
        # window, from every point the integrator visited (_terminal)
        self.windows = {f"{b.el_id}:sig_power": b.check.window_s for b in ctx.batteries.values()
                        if b.check is not None and b.check.limit_w > 0
                        and f"{b.el_id}:sig_power" in have}


def _finite(x) -> bool:
    return isinstance(x, (int, float)) and math.isfinite(x)


def _result(project: Project, case, model, ctx, built, run, plan: _Plan, lanes,
            laps_warning: Optional[str], script_warnings: tuple[str, ...] = (),
            stopped: bool = False, made: list[LiveEdit] = ()) -> SimResult:
    """Today's SimResult of the new engine's run: its evidence judged by the
    verdict and made into the reports by today's code (evidence.py); a run
    stopped where it was (a cancel) judged as today's judges a stopped one;
    the parts read as edited by the live edits it ``made``, as today's
    RunContext has them at its run's end."""
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
    if stopped:  # (today's words)
        messages.append(SimMessage(level="info", text=f"Simulation cancelled by user at t = "
                                                      f"{solved:g} s."))
    for d in built.report["warnings"]:  # the import's and preparation's
        start = d.get("code") in START_NOTES
        level = "info" if start or d.get("severity") != "warning" else "warning"
        messages.append(SimMessage(level=level, text=f"{'Preparation note' if level == 'info' else 'Preparation'}"
                                                     f" [{d.get('code')}]: {d.get('message')}"))
    # the live edits the run made, by today's own code: what the reports
    # read of the parts at the end (a hand calculation's drag coefficient)
    # as today's has it; what its checks read at each step none of them
    # changes (_checked)
    for e in made:
        ctx.apply_set_param(e.elementId, e.key, e.value)

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
    # today's own checks of the values the run used, in its words: what the
    # RunContext said while it was built (a Wheelbase of 0 with a CG
    # height, a CG height on one axle, wheels off the road at the start, a
    # driveline over-constrained) and what a step of today's says once it
    # sees it, asked of the run (_runtime)
    _runtime(ctx, plan, run, times[0] if times else 0.0)
    messages += ctx.rt.messages
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
        if (run.report["reran_to_finish"] and not stopped and ctx.end_distance is not None
                and trace.dist):
            # the engine ran again to its finish event and ended there: the
            # line, located to its root finding's precision (75.3000007 m at
            # rtol 1e-6, a hair short of 75.3 m at 1e-8), is where the run is
            trace.dist[-1] = max(trace.dist[-1], ctx.end_distance)
    elif ctx.full_throttle and not ctx.veh_id and times:
        # an acceleration test with no Vehicle: nothing moves, as today's
        # trace of it has it (speed and distance 0), and the verdict says so
        trace.times = list(times)
        trace.target, trace.speed, trace.dist = ([0.0] * len(times) for _ in range(3))
    problem = first_not_driven(trace)  # (today's asks it while its run goes)
    if problem:
        messages.append(SimMessage(level="warning", text=problem))
    series = {tuple(name.split(":", 1)): values for name, values in raw.items() if ":" in name}
    verdict = judge(trace, ev.distance, series, ctx.performance, solved, core.judged_uses(ev),
                    stopped, case)
    messages += [SimMessage(level=level, text=text) for level, text in verdict.messages]

    channels = _channels(model, run, times)

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
    not_valid = core.not_valid_first(ev, verdict, summary, stopped, None, times)
    core.not_valid_last(ev, verdict, summary, not_valid, edge_rows, lab_base(summary, electric))
    fill_keys(summary, project)

    energy, duty, limits = make_reports(ev)
    return SimResult(caseId=case_id, status=core.status_of(messages, stopped),
                     channels=channels, summary=summary, messages=messages,
                     partEnergy=energy_flows(ev.flows, ev.time_s), energy=energy, duty=duty,
                     limits=limits,
                     references=check_references(case.references, summary)
                     + hand_checks(ev, channels))


def _channels(model, run, times: list[float]) -> list[Channel]:
    """The run's channels as today's: each of today's channels a port of
    its part has, labelled and in its unit, in today's order (core.py: by
    element, then port); the new engine records a few more channels than
    today's, which are kept."""
    raw = run.channels
    unit_map = unit_groups()
    ports = {}
    for el_id, cdef in model.cdef_of.items():
        for p in list(cdef.ports) + list(model.elements[el_id].dynamicPorts or []):
            ports[(el_id, p.id)] = p
    channels: list[Channel] = []
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
            timeSeries=[{"t": t, "value": v} for t, v in zip(times, _numbers(raw[name]))],
            min=_numbers(run.min.get(name)),
            max=_numbers(run.max.get(name)),
            mean=_numbers(run.mean.get(name)),
        ))
    return channels


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


def _tables(ctx, run, messages: list[SimMessage]) -> list:
    """The tables the run read past their data, from the engine's records,
    into today's MapUse records (in the axis's unit: the engine's axis
    mapped onto today's), each said once as today's engine says it. A
    record of a table today's engine does not read as a part's data (a
    Driving Task's schedule) is not judged: a note with its code says so
    (NOT_JUDGED); so is one of a table today's engine books no read of, or
    reads only in a branch of its part or at another point (BRANCH_READS,
    which holds every table the engine reads), where today's read never
    left its data. A record today's engine would not count is left out: an
    axis of a single point is a constant, and float noise at an edge is inside
    (Map.bounds). A record of an axis set to Error stops the run, as
    today's engine stops there (TableError: the engine stops a run
    first). The engine keeps a record for each place a part reads a table
    (an Engine reads its fuel map at three torques): the records of one
    axis are one of today's, said once (_merged). A table today's engine
    names otherwise is judged under its name (a Lookup's Table 1D or 2D).
    Returns each judged record's stretches past the data at the run's
    output points (quick mode's _decided)."""
    groups: dict[tuple[str, str, int], list] = {}
    stretches: list = []
    for rec in run.table_outside:
        el_id, key, axis = rec["part"], rec["table"].rsplit(".", 1)[-1], int(rec["axis"])
        if el_id is None:
            continue
        rule = BRANCH_READS.get((ctx.model.cdef_of[el_id].id, key))
        if rule is not None and rule.key is not None:
            key = rule.key(ctx.params(el_id))
        m = ctx.maps.get((el_id, key))
        rng = m.ranges[axis] if m is not None and axis < len(m.ranges) else None
        shown = _to_today(rec, rng)
        if m is not None and axis < len(m.bounds) and (m.bounds[axis] is None or (
                _finite(rec["furthest"]) and m.edge(axis, shown(float(rec["furthest"]))) is None)):
            continue
        groups.setdefault((el_id, key, axis), []).append((rec, shown))
    for (el_id, key, axis), got in groups.items():
        rec, shown = _merged(got)
        uses = [u for u in ctx.map_use if u.el_id == el_id and u.table == key]
        m = ctx.maps.get((el_id, key))
        why, share, at = ("today's engine does not read it as a part's data", 0.0, None) if (
            axis >= len(uses)) else _branch_share(ctx, run, rec, el_id, key, axis, shown)
        if share <= 0.0:
            messages.append(SimMessage(level="info", text=(
                f"'{ctx.model.elements[el_id].label}' {key}: the new engine read it past its data "
                f"on axis {axis + 1} for {float(rec['total']):g} s from t = "
                f"{float(rec['first']):.2f} s (to {float(rec['furthest']):g} against its edge "
                f"{float(rec['edge']):g}, in the engine's units); {why}, so the verdict does not "
                f"judge it [{NOT_JUDGED}].")))
            continue
        if m is not None and axis in m.error_axes:
            raise TableError(f"{m.name}: read past its data at t = {float(rec['first']):.2f} s on "
                             f"its {uses[axis].axis} axis, which is set to stop the run (Error)")
        use = uses[axis]
        stretches.append((use, _stretches(run, rec, axis, m, shown)))
        hi = rec["hi"]
        edge = rec["edge"] if _finite(rec["edge"]) else (hi if _finite(hi) else math.nan)
        far = rec["furthest"] if _finite(rec["furthest"]) else edge
        use.outside_s = float(rec["total"]) * share
        use.edge, use.value = shown(edge), shown(far)
        use.t = float(rec["t_furthest"]) if _finite(rec["t_furthest"]) else float(rec["first"])
        first = float(rec["first"])
        if at is not None:
            # today's engine reads it elsewhere (only in a branch, or at
            # another point): the furthest point and the first are today's
            use.value, use.t, first = at
            use.edge = m.edge(axis, use.value)
        how = ("its edge slope is extended" if m is not None and axis < len(m.linear)
               and m.linear[axis] else "its edge value is held")
        name = m.name if m is not None else f"'{ctx.model.elements[el_id].label}' {key}"
        messages.append(SimMessage(level="info", text=(
            f"{name}: {use.axis} {use.value:.6g} {use.unit} is past the edge of its data "
            f"({use.edge:g} {use.unit}) from t = {first:.2f} s — {how} there. "
            f"The run summary says for how long and how far.")))
    _held_at_zero(ctx, run)
    return stretches


def _stretches(run, rec, axis: int, m, shown: Callable[[float], float]) -> int:
    """The stretches of the run's output points where any of the engine's
    reads of a record's table axis is past the data (run.table_reads)."""
    reads = [r["values"] for r in run.table_reads if r["table"] == rec["table"] and r["axis"] == axis]
    if m is None or not reads:
        return 1
    past = [any(m.edge(axis, shown(float(r[k]))) is not None for r in reads) for k in range(len(reads[0]))]
    return sum(1 for k, p in enumerate(past) if p and (k == 0 or not past[k - 1]))


def _held_at_zero(ctx, run) -> None:
    """Today's engine reads an E-Motor's loss map at 0 N·m whenever its
    supply holds it back (its search for the largest share of the command
    that fits starts there); the new engine reads it there at every point
    of a motor with a supply window, a read at a constant its records do
    not watch. On a torque axis set to Error whose data does not reach
    0 N·m, a run in which the supply held a motor back (its time limited,
    t_limited, above 0) stops, as today's run stops there (TableError)."""
    for el_id in ctx.motors:
        m = ctx.maps.get((el_id, "power_loss"))
        if m is None or 1 not in m.error_axes or m.edge(1, 0.0) is None:
            continue
        held = run.engine_channels.get(f"{_ident(el_id)}.t_limited")
        if held is not None and len(held) and float(held[-1]) > 0.0:
            raise TableError(f"{m.name}: read at 0 N·m, outside its data, where its supply first held it "
                             f"back ({float(held[-1]):.3g} s held in all), on its torque axis, which is "
                             f"set to stop the run (Error)")


def _ident(el_id: str) -> str:
    """A part's id as the engine names its instance (lsim-project's ident)."""
    s = re.sub(r"[^A-Za-z0-9]", "_", el_id)
    return "_" + s if not s or s[0].isdigit() else s


def _merged(got: list) -> tuple[dict, Callable[[float], float]]:
    """The records of one table axis, each with its value in today's unit
    (_to_today), as one, as today's engine keeps one record of an axis:
    the point furthest past the data with its edge and time, the earliest
    first time, the longest time outside."""
    # ponytail: the reads' stretches outside overlap by an unknown amount,
    # so the longest is a floor of their union (BRANCH_READS' share then
    # judges the rule's tables at the run's points)
    if len(got) == 1:
        return got[0]

    def past(item) -> float:
        rec, shown = item
        x, e = rec["furthest"], rec["edge"]
        return abs(shown(float(x)) - shown(float(e))) if _finite(x) and _finite(e) else -1.0

    rec, shown = max(got, key=past)
    firsts = [float(r["first"]) for r, _ in got if _finite(r["first"])]
    return {**rec, "total": max(float(r["total"]) for r, _ in got),
            "first": min(firsts) if firsts else rec["first"]}, shown


def _to_today(rec, rng: Optional[tuple[float, float]]) -> Callable[[float], float]:
    """A value of a table record's axis, in the engine's unit, in today's
    (rng: today's data range on that axis): a scale, from the edge the
    record lies past (the engine pads a table only past an end its argument
    never reaches, so that edge is a point of today's data too), else from
    the ends they share; a temperature axis the engine reads in °C too."""
    if rng is None:
        return lambda x: x
    lo, hi, edge = rec["lo"], rec["hi"], rec["edge"]
    for e, r in ((edge, rng[0] if edge == lo else rng[1]), (hi, rng[1]), (lo, rng[0])):
        if _finite(e) and e != 0:
            k = r / e
            return lambda x: x * k
    return lambda x: x


# the note's code for a table record the verdict does not judge (_tables)
NOT_JUDGED = "NEW-TABLE-NOT-JUDGED"


class TableError(Exception):
    """A table record of an axis set to Error (_tables): the run it came
    from read past data that stops today's run."""


def _motor_powered(v: dict) -> bool:
    """An E-Motor powered at a point, as today's engine has it: its inverter
    on for a command other than 0 (fast mode keeps `on` at 1 while the
    supply is live, so a command of 0, to round-off, is unpowered there)."""
    return v["on"] > 0.5 and abs(v["dem"]) > 1e-9


def _engine_drags(v: dict) -> bool:
    """An Engine reading its drag table at a point, as today's engine: not
    fired, or its idle governor braking."""
    return v["fired_thr"] <= 0.5 and (v["fired_idle"] <= 0.5 or v["t_b2"] < 0.0)


class Reads(NamedTuple):
    """Where today's engine books a table the new engine reads
    (BRANCH_READS)."""
    where: str  # where today's engine reads it, in words
    axes: tuple  # per axis: (the new engine's reads of it, today's read)
    key: Optional[Callable[[dict], str]] = None  # today's name for it, from the part's values
    # why today's engine leaves this part out, so reads none of its tables
    # (from today's model and the part's id), or None where it runs it
    left_out: Optional[Callable[[object, str], Optional[str]]] = None
    # the part's variables today's read takes (traced: _Plan, _branch_share)
    uses: tuple = ()


def _on_no_bus(kind: str) -> Callable[[object, str], Optional[str]]:
    """A battery or a fuel cell stack today's engine runs: the source of
    one of its buses (its bus loop reads the part's table only there; it
    leaves out one on no bus, as Data Checks say)."""
    return lambda model, el_id: (None if any(getattr(bus, kind) == el_id for bus in model.buses)
                                 else "it is connected to no bus")


def _in_no_driveline(model, el_id: str) -> Optional[str]:
    """An E-Motor or an Engine today's engine runs: one in one of its
    drivelines (it leaves out one whose shaft is connected to nothing, as
    Data Checks say)."""
    return None if any(el_id in dl.element_group for dl in model.drivelines) else "it is in no driveline"


_W_MAP = "noEvent(min(wa, w_max))"  # the speed an E-Motor's full-load and loss maps are read at
_MOTOR_VARS = ("on", "dem", "v", "wa", "T")  # what an E-Motor's rules read (_motor_powered)
_ENGINE_VARS = ("wa", "n_map", "fired_thr", "fired_idle", "t_b1", "t_b2")  # an Engine's

SAME = "same"  # today's engine reads the axis where the new one does, at every point
NEVER = "never"  # today's engine books no read of the table


# where today's engine books each table the new engine reads: every table
# read in the engine's part equations, in every configuration
# (lightsim_engine.table_sites; the tests hold this list to it):
# (component, table) -> Reads: where today reads it, in words; per axis the
# abscissae the new engine reads it at, as the part's equations write them
# (the run gives the values at its points of each read it watches, from the
# model itself: run.table_reads; one at a constant, 0 N·m, it does not), and
# today's read: SAME, the new engine's one read, at every point (judged as
# the engine has it); NEVER, no read (not judged); or (the part's variables
# at a point, its maximum speed when the table reaches it, else inf) -> the
# value today reads it at there, in the engine's unit, or None outside the
# branch it reads it in. Past its maximum speed a motor or engine counts as
# over speed, not as past a drag table that reaches that speed: today reads
# that at the maximum speed.
BRANCH_READS = {
    ("battery.generic", "ocv_table"): Reads(
        "at its state of charge, held to 0-100 %", ((("soc_c",), SAME),),
        left_out=_on_no_bus("battery")),
    ("motor.emotor", "full_load_torque"): Reads(
        "only while the motor is powered",
        ((("v",), lambda v, top: v["v"] if _motor_powered(v) else None),
         ((_W_MAP,), lambda v, top: v["wa"] if _motor_powered(v) else None)),
        left_out=_in_no_driveline, uses=_MOTOR_VARS),
    # at its torque's magnitude (motor_torque); the engine reads it at the
    # command's torque and the shaft's, and, with a supply window, at 0 and
    # the allowed share
    ("motor.emotor", "power_loss"): Reads(
        "only while the motor is powered",
        (((_W_MAP,), lambda v, top: v["wa"] if _motor_powered(v) else None),
         (("noEvent(abs(t_cmd))", "noEvent(abs(T))", "0 * unit_N_m", "noEvent(abs(frac * t_cmd))"),
          lambda v, top: abs(v["T"]) if _motor_powered(v) else None)),
        left_out=_in_no_driveline, uses=_MOTOR_VARS),
    ("motor.emotor", "drag_torque"): Reads(
        "only while the motor is unpowered, at no more than its maximum speed when the table "
        "reaches it", ((("wa",), lambda v, top: None if _motor_powered(v) else min(v["wa"], top)),),
        left_out=_in_no_driveline, uses=_MOTOR_VARS),
    # (read at a speed held to its own first and last speed, so never past
    # its data in either engine)
    ("engine.combustion", "full_load_torque"): Reads(
        "only between its own first and last speed, and books no read of it", ((("n_map",), NEVER),)),
    ("engine.combustion", "drag_torque"): Reads(
        "only while the engine is not fired or its idle governor brakes, at no more than its "
        "maximum speed when the table reaches it",
        ((("wa",), lambda v, top: min(v["wa"], top) if _engine_drags(v) else None),),
        left_out=_in_no_driveline, uses=_ENGINE_VARS),
    # fired with throttle at its torque; fired by the idle governor at its
    # torque, or at 0 while it brakes (engine_torque)
    ("engine.combustion", "fuel_map"): Reads(
        "only while the engine is fired",
        ((("n_map",), lambda v, top: v["n_map"] if v["fired_thr"] > 0.5
          or v["fired_idle"] > 0.5 else None),
         (("t_b1", "max(t_b2, 0 * unit_N_m)", "0 * unit_N_m"),
          lambda v, top: v["t_b1"] if v["fired_thr"] > 0.5
          else max(v["t_b2"], 0.0) if v["fired_idle"] > 0.5 else None)),
        left_out=_in_no_driveline, uses=_ENGINE_VARS),
    # at the current it gives (the bus's bisection); its read at its Maximum
    # Current, the most power it gives (the bus manager's window), is unbooked
    ("fuelcell.stack", "polarization"): Reads(
        "at the current it gives, not at its Maximum Current",
        ((("I", "i_max"), lambda v, top: v["I"]),), left_out=_on_no_bus("fuelcell"), uses=("I",)),
    # (consumer_demand_w: enabled, and asking for heat or cooling there)
    ("electric.climate", "demand_table"): Reads(
        "only while the climate control is on and asks for heating or cooling",
        ((("t_out_c",), lambda v, top: v["t_out_c"] if v["q"] != 0.0 else None),), uses=("t_out_c", "q")),
    # its Table 1D, or its Table 2D in the Mode 2D (the verdict leaves it to
    # its summary rows: a controller's own schedule, core.judged_uses)
    ("signal.lookup", "table"): Reads(
        "at its inputs", ((("sig_x_in",), SAME), (("sig_y_in",), SAME)),
        key=lambda p: "table_2d" if str(p.get("mode", "1D")) == "2D" else "table_1d"),
    ("signal.driving_task", "profile"): Reads(
        "as the speeds it asks for, not as a part's data", ((("x",), NEVER),)),
    ("signal.road_profile", "profile"): Reads(
        "as the road's grade, not as a part's data", ((("x",), NEVER),)),
}


def _branch_share(ctx, run, rec, el_id: str, key: str, axis: int, shown: Callable[[float], float]
                  ) -> tuple[str, float, Optional[tuple[float, float, float]]]:
    """The share of a table record's time outside its data that today's
    engine would count, by where it books the table (BRANCH_READS; key:
    today's name for it): where the run has a trace of the rule's variables
    (Reads.uses), the time today's read is past the data there over the
    record's time; else, of the run's points where the new engine read the
    axis past the data (at any of its reads: run.table_reads, from the
    model itself; shown: in today's unit; Map.edge: float noise at an edge
    is inside), those where today's read is past it too; 1 where today
    reads it as the new engine does, or the run does not say; 0 where
    today books no read of it. With why, for a share of 0 (the record is
    then not judged), and, unless every read of the new engine is today's
    at every such point, today's furthest point past the data (today's
    unit), its time and today's first time past the data."""
    # ponytail: counted on the trace (0.1 s) where the run has one, else
    # (fast mode) the share at its points scales the engine's count; a
    # stretch in the branch shorter than that step is missed
    rule = BRANCH_READS.get((ctx.model.cdef_of[el_id].id, rec["table"].rsplit(".", 1)[-1]))
    m = ctx.maps.get((el_id, key))
    if rule is not None and rule.left_out is not None and (gone := rule.left_out(ctx.model, el_id)):
        return f"today's engine leaves this part out ({gone})", 0.0, None
    if rule is None or m is None or axis >= len(rule.axes) or rule.axes[axis][1] is SAME:
        return "", 1.0, None
    today, why = rule.axes[axis][1], f"today's engine reads it {rule.where}"
    if today is NEVER:
        return why, 0.0, None
    part = ctx.motors.get(el_id) or ctx.engines.get(el_id)
    cap, unit = (part.speed_use.edge if part is not None else math.inf), shown(1.0)
    top = cap / unit if unit and m.ranges[axis] and m.ranges[axis][1] >= cap else math.inf
    prefix, ch = rec["table"].rsplit(".", 1)[0], run.engine_channels
    tr = _Listed(getattr(run, "trace", None))
    names = {u: f"{prefix}.{u}" for u in rule.uses}
    if names and all(n in tr.trace for n in names.values()) and "times" in tr.trace \
            and float(rec["total"]) > 0.0:
        # today's read on the run's trace (0.1 s, 0.01 s in an acceleration
        # test): the time it is past the data, each point to the next
        ts, cols = tr["times"], {u: tr[n] for u, n in names.items()}
        hits = []
        for k in range(len(ts) - 1):
            x = today({u: c[k] for u, c in cols.items()}, top)
            if x is not None and m.edge(axis, shown(x)) is not None:
                hits.append((k, shown(x)))
        if not hits:
            return f"{why}: never past its data on the run's trace", 0.0, None
        held = sum(ts[k + 1] - ts[k] for k, _ in hits)
        k, x = max(hits, key=lambda r: abs(r[1] - m.edge(axis, r[1])))
        return why, held / float(rec["total"]), (x, ts[k], ts[hits[0][0]])
    reads = [r["values"] for r in run.table_reads if r["table"] == rec["table"] and r["axis"] == axis]
    out, read, same = [], [], True
    try:
        cols = {n[len(prefix) + 1:]: v for n, v in ch.items() if n.startswith(prefix + ".")}
        times = [float(t) for t in run.times]
        for k in range(len(times) if reads else 0):
            xs = tuple(float(r[k]) for r in reads)
            if all(m.edge(axis, shown(x)) is None for x in xs):
                continue
            out.append(k)
            x = today({n: float(c[k]) for n, c in cols.items()}, top)
            if x is not None and m.edge(axis, shown(x)) is not None:
                read.append((k, shown(x)))
                same = same and all(e == x for e in xs)
    except (KeyError, IndexError, TypeError, ValueError):
        return "", 1.0, None
    if not out:
        return "", 1.0, None  # (outside only between the points: the engine's count stands)
    why = f"{why}: never past its data at those points"
    if not read:
        return why, 0.0, None
    if len(read) == len(out) and same:
        return why, 1.0, None
    k, x = max(read, key=lambda r: abs(r[1] - m.edge(axis, r[1])))
    return why, len(read) / len(out), (x, times[k], times[read[0][0]])


# the codes of the path's notes on a run it ran: what of it the verdict does
# not judge, and the live edits it did not make
# (benchmarks/back_to_back/acceptance.toml's [note_codes])
NEXT_RUN = "NEW-LIVE-NEXT-RUN"  # the note's code for a live edit the run did not make
NOTE_CODES = {NOT_JUDGED: "a table the run read past its data that today's engine does not read "
                          "there (not as a part's data, or only in a branch of its part the run "
                          "did not take there, or at another point): a note, not judged",
              NEXT_RUN: "a live edit the run did not make (one the new engine builds another "
                        "model for, one its checks after the run read as at the start, a table, "
                        "a structural parameter, or one too late): it takes effect on the next run"}


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


LIFT_N = 1e-6  # an axle load at most this carries nothing, N (a lifted axle's is 0 to round-off)


def _runtime(ctx, plan: _Plan, run, t0: float) -> None:
    """Today's checks that a step of its run makes once it sees them
    (RunContext: Data Checks never see a study's point, a sweep or a live
    edit), asked of the run and said by today's own code into the
    RunContext's messages: the Ambient's air outside the usual (at the
    run's start, as today's first step: the new engine reads the case's
    Ambient throughout), with the air density the run's Vehicle used; an
    E-Motor with no live supply (its supply voltage at 1 V or less at any
    point the integrator visited); the Vehicle's wheels lifting off the
    road, which today's engine checks only with a CG height or a Downforce
    Area (an axle's load at 0 over every step; which wheels at the first
    traced point that shows it, when at the Vehicle's own axle-load event in
    that traced stretch; a lift shorter than the trace's step at the first
    such event). The start's own settling is no lift: the run's first point
    is the state before the events of t = 0, and the Vehicle at rest slides
    and sticks again within microseconds (its load transfer then moves a
    few N off a light axle for no time), which the integrals' lowest value
    holds too. So the first traced point is not read: a lift is told from
    the second on (one there from the start has no event after t0 in its
    stretch, so it is said at the start), else at a Vehicle axle-load event
    after the first traced stretch. A lift that begins and ends within the
    first step (0.1 s; 0.01 s, today's step, in an acceleration test)
    goes unsaid."""
    integ = run.integrals
    if ctx.veh_id:
        rho = integ.get(f"{plan.inst.get(ctx.veh_id, ctx.veh_id)}.rho")
        ctx.read_ambient(t0, float(rho["max"]) if rho else None)
    for el_id in ctx.motors:
        got = integ.get(f"{plan.inst.get(el_id, el_id)}.v")
        if got and got["min"] <= 1.0:
            ctx.no_supply(el_id)
    front, rear = ctx.axle_wheels
    if not ctx.veh_id or not (front or rear) or not any(ctx.load_shift()):
        return
    sides = (("Front", plan.loads["Front"]), ("Rear", plan.loads["Rear"])) if front and rear else (
        ("Both", plan.loads["Front" if front else "Rear"]),)  # one axle: all its wheels
    lifted = [(side, name) for side, name in sides
              if name and name in integ and integ[name]["min"] <= LIFT_N]
    if not lifted:
        return
    tr = run.trace or {}
    times = [float(t) for t in tr.get("times", [])]
    if len(times) < 2:  # (no stretch after the start)
        return
    who = f"'{ctx.model.elements[ctx.veh_id].label}': "
    axle_events = sorted(float(e["t"]) for e in run.events
                         if e["label"].startswith(who) and "axle" in e["label"])
    k = next((k for k in range(1, len(times))
              if any(float(tr[name][k]) <= LIFT_N for _, name in lifted)), None)
    if k is None:  # (lifted only between two traced points, after the first stretch)
        late = [t for t in axle_events if t > times[1]]
        if not late:
            return
        which, t = [side for side, _ in lifted], late[0]
    else:
        which = [side for side, name in lifted if float(tr[name][k]) <= LIFT_N]
        t = _at_event(axle_events, times[k - 1], times[k])
    ctx.wheels_lift("Both" if len(which) > 1 else which[0], t)


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
    tr = _Listed(run.trace)
    times = tr.get("times", [])
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
    import numpy as np  # (the module's own arrays: lightsim_engine needs NumPy)

    tr = run.trace or {}
    times = np.asarray(tr.get("times", []), dtype=float)
    dt = np.diff(times)
    out = {}
    for el_id, ports in plan.ports.items():
        total = back = 0.0
        q = np.zeros(len(times))
        for port, through, across in ports:
            p = np.asarray(tr[through], dtype=float) * np.asarray(tr[across], dtype=float)
            if not _road_side(port) or port == "road":
                q = q + p
            a, b = p[:-1], p[1:]
            both = (a >= 0) & (b >= 0)
            with np.errstate(divide="ignore", invalid="ignore"):  # (the line crosses zero inside)
                cross = 0.5 * np.maximum(a, b) ** 2 / np.abs(a - b) * dt
            pos = float(np.where(both, 0.5 * (a + b) * dt, np.where((a > 0) | (b > 0), cross, 0.0)).sum())
            total += pos
            if _road_side(port):
                back += pos
        integral = float((0.5 * (q[:-1] + q[1:]) * dt).sum())
        square = float(((q[:-1] ** 2 + q[:-1] * q[1:] + q[1:] ** 2) / 3.0 * dt).sum())
        out[el_id] = (total, back, float(np.abs(q).max()) if len(q) else 0.0, integral, square)
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
    tr = _Listed(run.trace)
    times = tr.get("times", [])
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


class _Listed:
    """A run's trace with each of its channels as a list of floats, made
    the first time it is read (the same values: an item of a list is
    quicker to read than one of an array, and these are read point by
    point)."""

    def __init__(self, trace):
        self.trace, self.lists = trace or {}, {}

    def __getitem__(self, name: str) -> list[float]:
        if name not in self.lists:
            self.lists[name] = [float(x) for x in self.trace[name].tolist()]
        return self.lists[name]

    def get(self, name: str, default=None):
        return self[name] if name in self.trace else default


def _number(v) -> Optional[float]:
    v = float(v)
    return v if math.isfinite(v) else None


def _numbers(values) -> Optional[list[Optional[float]]]:
    """Each value a number, or None where it is not one (a gap)."""
    if values is None:
        return None
    import numpy as np  # (the module's own arrays: lightsim_engine needs NumPy)

    a = np.asarray(values, dtype=float)
    return a.tolist() if np.isfinite(a).all() else [_number(v) for v in a.tolist()]
