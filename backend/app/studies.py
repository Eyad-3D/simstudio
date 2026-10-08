"""Parameter studies on all CPU cores (ENG-05).

A study is one case run at many points, each point a set of per-case
parameter values. The points run side by side in a pool of worker
processes, so a study of dozens to hundreds of points finishes in about
(points ÷ workers) single-run times, and the local server stays free to
answer while it goes: the solver runs in the workers, not in its threads.

Every finished point is stored as a run of its own (run_store, with its disk
budget), and only its summary values come back to the caller at once; the
full time traces are read from the run store on request.

The pool defaults to the computer's processor count less one, and to fewer
when memory is short: each worker holds an engine (WORKER_MB) and, for a
model with Script blocks, each of its runs starts its own locked-down Script
worker with a memory cap of its own (sandbox_worker.DEFAULT_MEM_BYTES).
Workers start with the "spawn" method, as the Script sandbox does, so the
frozen desktop engine runs them too (run_backend.py calls freeze_support).
"""
from __future__ import annotations

import asyncio
import math
import multiprocessing
import os
import time
import uuid
from concurrent.futures import ProcessPoolExecutor
from dataclasses import dataclass, field
from typing import Awaitable, Callable, Optional

from . import run_store
from .schemas import Project, RunSnapshot, SimResult, StoredRun
from .solver.sandbox_worker import DEFAULT_MEM_BYTES
from .version import VERSION

WORKER_MB = 250  # an engine process with a large result in memory, MB
MAX_POINTS = 2000

# ---- worker side -------------------------------------------------------------
_cancel = None  # the study's stop flag (a multiprocessing Event), per worker


def _init_worker(cancel_event) -> None:
    global _cancel
    _cancel = cancel_event


def _control() -> list[dict]:
    return [{"type": "cancel"}] if _cancel is not None and _cancel.is_set() else []


def run_point(project_json: str, case_id: str) -> tuple[str, float]:
    """Run one point in a worker: (its SimResult as JSON, wall time in s)."""
    from .solver import simulate  # imported once per worker, on its first point

    t0 = time.perf_counter()
    if _cancel is not None and _cancel.is_set():
        result = SimResult(caseId=case_id, status="cancelled", channels=[], messages=[
            {"level": "info", "text": "Simulation cancelled by user at t = 0 s."}])
    else:
        result = simulate(Project.model_validate_json(project_json), case_id, None, _control)
    return result.model_dump_json(), time.perf_counter() - t0


# ---- pool size ---------------------------------------------------------------
def _memory_bytes() -> Optional[int]:
    """The computer's physical memory, or None where it cannot be read."""
    try:
        return os.sysconf("SC_PAGE_SIZE") * os.sysconf("SC_PHYS_PAGES")
    except (AttributeError, ValueError, OSError):
        pass
    try:  # Windows
        import ctypes

        class _Mem(ctypes.Structure):
            _fields_ = [("dwLength", ctypes.c_ulong), ("dwMemoryLoad", ctypes.c_ulong),
                        ("ullTotalPhys", ctypes.c_ulonglong), ("ullAvailPhys", ctypes.c_ulonglong),
                        ("ullTotalPageFile", ctypes.c_ulonglong),
                        ("ullAvailPageFile", ctypes.c_ulonglong),
                        ("ullTotalVirtual", ctypes.c_ulonglong),
                        ("ullAvailVirtual", ctypes.c_ulonglong),
                        ("ullAvailExtendedVirtual", ctypes.c_ulonglong)]
        m = _Mem()
        m.dwLength = ctypes.sizeof(_Mem)
        if ctypes.windll.kernel32.GlobalMemoryStatusEx(ctypes.byref(m)):  # type: ignore[attr-defined]
            return int(m.ullTotalPhys)
    except Exception:  # noqa: BLE001 — best effort
        pass
    return None


def default_workers(project: Project, points: int) -> int:
    """Processors less one (at least 1), no more than the points, and no more
    than half the memory holds: an engine each, plus a Script worker each
    when the model has Script blocks."""
    n = max(1, (os.cpu_count() or 2) - 1)
    has_scripts = any(el.componentDefId == "signal.script"
                      for s in project.systems for el in s.elements)
    mem = _memory_bytes()
    if mem:
        per = WORKER_MB * 2**20 + (DEFAULT_MEM_BYTES if has_scripts else 0)
        n = min(n, max(1, int(0.5 * mem // per)))
    return max(1, min(n, points))


# ---- the study ---------------------------------------------------------------
@dataclass
class StudyPointSpec:
    """One point: the case values it sets ({element id: {key: value}}), the
    factor values it shows in the study table, and its run's name."""
    overrides: dict
    values: list[float] = field(default_factory=list)
    label: str = ""


@dataclass
class StudyOptions:
    sweep_id: str = ""
    sweep_param: Optional[str] = None
    sweep_unit: Optional[str] = None
    workers: Optional[int] = None
    store: bool = True  # store each point as a run of the project


def _point_project(project: Project, case_id: str, overrides: dict) -> Project:
    p = project.model_copy(deep=True)
    case = next(c for c in p.cases if c.id == case_id)
    for el_id, values in overrides.items():
        case.parameterOverrides.setdefault(el_id, {}).update(values)
    return p


def _incomplete(result: SimResult) -> Optional[str]:
    """As the app words it (projectStore incompleteReason)."""
    if result.status == "failed":
        return "failed"
    if result.status != "cancelled":
        return None
    for m in result.messages:
        if "cancel" in m.text.lower() and "t = " in m.text:
            return "stopped at t = " + m.text.split("t = ", 1)[1].split(" s", 1)[0] + " s"
    return "stopped"


Send = Callable[[dict], Awaitable[None]]


async def run_study(project: Project, case_id: str, points: list[StudyPointSpec],
                    options: StudyOptions, send: Send,
                    stop: asyncio.Event) -> dict:
    """Run the points in a process pool, sending {"type": "point", …} for
    each as it finishes (in finishing order) and storing its run; returns the
    study's totals. Setting ``stop`` stops the running points (each ends as
    *cancelled*) and leaves the others not run."""
    case = next(c for c in project.cases if c.id == case_id)
    workers = max(1, min(options.workers or default_workers(project, len(points)), len(points)))
    ctx = multiprocessing.get_context("spawn")
    cancel_event = ctx.Event()
    loop = asyncio.get_running_loop()
    t0 = time.perf_counter()
    walls: list[float] = []
    pool = ProcessPoolExecutor(max_workers=workers, mp_context=ctx,
                               initializer=_init_worker, initargs=(cancel_event,))
    try:
        futures = {}
        for i, pt in enumerate(points):
            pj = _point_project(project, case_id, pt.overrides)
            # (the pool's own future: cancelling it fails once its point runs,
            # where cancelling an asyncio wrapper would drop a running point)
            cf = pool.submit(run_point, pj.model_dump_json(), case_id)
            futures[asyncio.wrap_future(cf, loop=loop)] = (i, pt, pj, cf)
        stopper = asyncio.ensure_future(stop.wait())
        pending = set(futures)
        while pending:
            done, _ = await asyncio.wait(pending | {stopper}, return_when=asyncio.FIRST_COMPLETED)
            if stopper in done and not cancel_event.is_set():
                cancel_event.set()  # running points stop at their next step
                for f in pending:
                    futures[f][3].cancel()  # points not started yet are not run
            for fut in done - {stopper}:
                pending.discard(fut)
                i, pt, pj, _ = futures[fut]
                if fut.cancelled():
                    await send({"type": "point", "index": i, "values": pt.values,
                                "status": "not run"})
                    continue
                try:
                    result_json, wall = fut.result()
                    # (decoded beside the event loop: a long run's result is
                    # megabytes, and the server must keep answering)
                    result = await asyncio.to_thread(SimResult.model_validate_json, result_json)
                except Exception as e:  # noqa: BLE001 — a worker died: that point failed
                    result, wall = SimResult(
                        caseId=case_id, status="failed", channels=[],
                        messages=[{"level": "error", "text": f"The run's worker stopped: {e}"}]), 0.0
                walls.append(wall)
                # a sweep point keeps its recorded points, not the peaks between
                # them (ENG-16): those make a run about 5 times larger, and a
                # 200-point sweep would push older runs out of the disk budget
                for ch in result.channels:
                    ch.min = ch.max = ch.mean = None
                run_id = None
                pruned: list[str] = []
                incomplete = _incomplete(result)
                if options.store:
                    run_id = f"run-{uuid.uuid4().hex[:12]}"
                    pcase = next(c for c in pj.cases if c.id == case_id)
                    run = StoredRun(
                        id=run_id, caseId=case_id, caseName=pt.label or case.name,
                        startedAt=int(time.time() * 1000), status=result.status, result=result,
                        sweepId=options.sweep_id or None, sweepParam=options.sweep_param,
                        sweepValue=pt.values[0] if len(pt.values) == 1 else None,
                        sweepUnit=options.sweep_unit, incomplete=incomplete,
                        # (set, so the run store, which leaves out unset
                        # fields, keeps the empty list the app reads)
                        # the model as it was and the case with the point's
                        # value, as a run from the app keeps them (the swept
                        # value is the run's own, not a change to the model)
                        snapshot=RunSnapshot(project=project, case=pcase, appVersion=VERSION,
                                             liveEdits=[]))
                    try:
                        _, pruned = await asyncio.to_thread(run_store.save_run, project.id, run)
                    except ValueError:
                        run_id = None  # an id no run can be stored under
                await send({
                    "type": "point", "index": i, "values": pt.values, "runId": run_id,
                    "status": result.status, "incomplete": incomplete, "wallS": wall,
                    "pruned": pruned,
                    "summary": [s.model_dump() for s in result.summary
                                if math.isfinite(s.value)],
                    # each part's duty, for the study table's RMS and peak
                    # columns (RES-39)
                    "duty": [d.model_dump() for d in result.duty],
                })
        stopper.cancel()
    finally:
        cancel_event.set()
        pool.shutdown(wait=False, cancel_futures=True)
    total = time.perf_counter() - t0
    return {"workers": workers, "wallS": total, "pointWallS": sum(walls),
            "speedup": sum(walls) / total if total > 0 else 0.0}
