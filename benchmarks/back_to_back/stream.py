"""The STREAM line's probes that are not single runs: stop latency (S2)
and replay from the run's snapshot (S3). The streamed-points check (S1)
is made in each live item's own run (runs.streamed).

Stop latency is the wall time from a stop asked, at a random moment of the
run, to the run (or the study) returning. A stop shows something only when
the run it reached ends *cancelled* (a study: a point that was running
ends cancelled); a stop that finds nothing running, or that the run does
not hear, shows nothing. Numbers taken here are from a shared machine and
are labelled so; the pass mark is judged on Windows CI
(backend/tests/test_stop_latency.py).
"""
from __future__ import annotations

import asyncio
import hashlib
import json
import math
import os
import random
import re
import statistics
import threading
import time
from pathlib import Path

from . import runs

ENGINES = {"today": {}, "new": {"LIGHTSIM_ENGINE": "new", "LIGHTSIM_ENGINE_STRICT": "1"}}
SHOWS_ENOUGH = 0.5  # fewer stops than this share showing something: not measured
STUDY_MASSES = [1200.0 + 100.0 * k for k in range(16)]


class _Env:
    """The engine's environment for the code run in this process (and the
    study's workers, which inherit it)."""

    def __init__(self, engine: str):
        self.set = ENGINES[engine]

    def __enter__(self):
        self.saved = {k: os.environ.get(k) for k in ("LIGHTSIM_ENGINE", "LIGHTSIM_ENGINE_STRICT")}
        for k in self.saved:
            os.environ.pop(k, None)
        os.environ.update(self.set)

    def __exit__(self, *exc):
        for k, v in self.saved.items():
            os.environ.pop(k, None)
            if v is not None:
                os.environ[k] = v


def p95(xs: list[float]) -> float | None:
    if not xs:
        return None
    xs = sorted(xs)
    return xs[max(0, math.ceil(0.95 * len(xs)) - 1)]  # the nearest rank


def _summary(lat: list[float], shown: int, stops: int, mark: float) -> dict:
    measured = stops > 0 and shown >= SHOWS_ENOUGH * stops
    p = p95(lat)
    return {"stops": stops, "shown": shown, "p95_s": p, "median_s": statistics.median(lat) if lat else None,
            "max_s": max(lat) if lat else None, "mark_s": mark,
            "state": "not measured" if not measured else ("pass" if p is not None and p <= mark else "fail"),
            "latencies_s": lat}


def live_stops(engine: str, stops: int, seed: int = 2) -> dict:
    """bev-car/case-wltc as a live run (streamed through an emit), stopped
    at random moments through its control (the app's cancel message)."""
    from app.schemas import Project
    from app.solver import simulate
    project = Project.model_validate(runs.example("bev-car"))
    rng = random.Random(seed)
    with _Env(engine):
        t0 = time.perf_counter()
        whole = simulate(project, "case-wltc", lambda m: None)
        length = time.perf_counter() - t0
        lat, shown = [], 0
        for _ in range(stops):
            asked: list[float] = []
            at = rng.uniform(0.2, 0.8) * length
            start = time.perf_counter()
            timer = threading.Timer(at, lambda: asked.append(time.perf_counter()))
            timer.start()

            def control():
                return [{"type": "cancel"}] if asked else []

            res = simulate(project, "case-wltc", lambda m: None, control)
            end = time.perf_counter()
            timer.cancel()
            if asked and res.status == "cancelled":
                shown += 1
                lat.append(end - asked[0])
            elif not asked:
                pass  # it ended before the stop: nothing to show
            del start
    out = _summary(lat, shown, stops, 0.25)
    out.update(unstopped_s=length, unstopped_status=whole.status,
               unstopped_first=whole.messages[0].text if whole.messages else "")
    return out


def study_stops(engine: str, stops: int, seed: int = 3) -> dict:
    """A 16-point study of bev-car/case-city (the vehicle's mass from 1200 to
    2700 kg), stopped at random moments; each stop's latency to the study's
    return, and whether a point that was running ends cancelled."""
    from app import studies
    from app.schemas import Project
    project = Project.model_validate(runs.example("bev-car"))
    points = [studies.StudyPointSpec(overrides={"el-vehicle": {"mass_kg": m}}, values=[m])
              for m in STUDY_MASSES]
    options = studies.StudyOptions(store=False, workers=4)
    rng = random.Random(seed)

    async def one(at: float | None) -> tuple[float, float | None, list[str]]:
        stop = asyncio.Event()
        seen: list[str] = []

        async def send(msg: dict) -> None:
            seen.append(_shown(msg))

        asked: list[float] = []

        async def stopper():
            await asyncio.sleep(at)
            asked.append(time.perf_counter())
            stop.set()

        task = asyncio.ensure_future(stopper()) if at is not None else None
        t0 = time.perf_counter()
        await studies.run_study(project, "case-city", points, options, send, stop)
        end = time.perf_counter()
        if task:
            task.cancel()
        return end - t0, (end - asked[0]) if asked else None, seen

    with _Env(engine):
        length, _, first = asyncio.run(one(None))
        lat, shown = [], 0
        statuses = []
        for _ in range(stops):
            _, latency, seen = asyncio.run(one(rng.uniform(0.2, 0.8) * length))
            statuses.append(seen)
            if latency is not None and "cancelled while running" in seen:
                shown += 1
                lat.append(latency)
    out = _summary(lat, shown, stops, 1.0)
    out.update(unstopped_s=length, unstopped_statuses=sorted(set(first)),
               stopped_statuses=[sorted(set(s)) for s in statuses])
    return out


def _shown(msg: dict) -> str:
    """A study point's message as the stop probe counts it: a point the stop
    reached while it ran ends cancelled at some t > 0; one cancelled at t = 0
    never ran (its worker saw the stop before it started)."""
    status = msg.get("status")
    if status == "cancelled":
        m = re.search(r"t = ([-\d.e+]+) s", msg.get("incomplete") or "")
        return "cancelled while running" if m and float(m.group(1)) > 0 else "cancelled before running"
    return str(status)


def _done_before(path: Path, now: dict) -> bool:
    """Whether ``path`` holds a probe's result made by this same code."""
    return path.exists() and json.loads(path.read_text(encoding="utf-8")).get("stamp") == now


def stop_probe(out: Path, stops: int) -> None:
    path, now = out / "stop.json", runs.stamp()
    if _done_before(path, now):
        print("stop probe: done before, at this code")
        return
    res = {"stamp": now,
           "machine": "shared (the step 1 lane may be running): an indication, not the judged number"}
    for engine in ENGINES:
        res[engine] = {"live": live_stops(engine, stops), "study": study_stops(engine, stops)}
        print(f"stop probe {engine}: live {res[engine]['live']['state']}, "
              f"study {res[engine]['study']['state']}", flush=True)
    path.write_text(json.dumps(res, indent=1), encoding="utf-8")


def sha256_of(result) -> str:
    return hashlib.sha256(json.dumps(result.model_dump(mode="json"), sort_keys=True,
                                     separators=(",", ":")).encode()).hexdigest()


# S3's runs: the registered two, and on the new engine's path a live run
# with a live edit (the Vehicle's drag coefficient once the run passes 300
# s; acceptance.toml's dated change of 2026-10-31). Today's engine does not
# replay live edits (it applies one at the output step its polling saw it,
# which nothing records): its replay of such a run fails, saying so
REPLAYED = ("bev-car/case-city", "fs-electric/case-accel-75m")
LIVE_EDIT = ("bev-car/case-wltc", 300.0,
             {"type": "set_param", "elementId": "el-vehicle", "key": "cd", "value": 0.35})


def _live_edit():
    """A live run's emit and control that gives LIVE_EDIT's edit once a
    streamed point has passed its time."""
    seen, sent = [], []

    def emit(msg: dict) -> None:
        if msg.get("type") == "step":
            seen.append(msg["t"])

    def control() -> list[dict]:
        if not sent and seen and seen[-1] >= LIVE_EDIT[1]:
            sent.append(True)
            return [dict(LIVE_EDIT[2])]
        return []
    return emit, control


def replay_probe(out: Path) -> None:
    """S3: a run, its RunSnapshot as the app keeps it (the project, the case
    as run, the app's version, and what the result says ran it: the engine,
    its version, its solver options, its live edits: RunSnapshot.of),
    through its JSON, and the run made again from the snapshot alone by the
    app's replay (whatever LIGHTSIM_ENGINE says then): the same SHA-256?
    And does each snapshot say which engine ran it, its version and its
    solver options (their values, not only the fields)?"""
    from app.schemas import Project, RunSnapshot
    from app.solver import replay, simulate
    from app.version import VERSION
    path, now = out / "replay.json", runs.stamp()
    if _done_before(path, now):
        print("replay probe: done before, at this code")
        return
    res = {"stamp": now, "snapshot_fields": sorted(RunSnapshot.model_fields), "runs": []}
    for engine in ENGINES:
        cases = [(c, None) for c in REPLAYED] + ([(LIVE_EDIT[0], LIVE_EDIT)] if engine == "new" else [])
        for case, live in cases:
            name, case_id = case.split("/")
            project = Project.model_validate(runs.example(name))
            with _Env(engine):
                first = simulate(project, case_id, *(_live_edit() if live else ()))
            pcase = next(c for c in project.cases if c.id == case_id)
            snap = RunSnapshot.model_validate_json(
                RunSnapshot.of(project, pcase, first, VERSION).model_dump_json())
            again = replay(snap)
            res["runs"].append({"engine": engine, "case": case + (" live edit" if live else ""),
                                "status": first.status,
                                "first": first.messages[0].text if first.messages else "",
                                "snapshot": {"engine": snap.engine, "engineVersion": snap.engineVersion,
                                             "solverOptions": snap.solverOptions,
                                             "liveEdits": [e.model_dump() for e in snap.liveEdits]},
                                "sha256": sha256_of(first), "replayed": sha256_of(again),
                                "same": sha256_of(first) == sha256_of(again)})
    snaps = [r["snapshot"] for r in res["runs"]]
    res["records_engine"] = all(s["engine"] for s in snaps)
    res["records_engine_version"] = all(s["engineVersion"] for s in snaps)
    res["records_solver_options"] = all(s["solverOptions"] for s in snaps)
    path.write_text(json.dumps(res, indent=1), encoding="utf-8")
    print("replay probe: " + ", ".join(f"{r['engine']} {r['case']} {'same' if r['same'] else 'DIFFERENT'}"
                                       for r in res["runs"]), flush=True)
