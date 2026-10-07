"""LightSim backend — FastAPI app.

Endpoints:
  GET  /api/library            component library definitions
  GET  /api/cycles             the bundled standard drive cycles
  GET  /api/cycles/{id}        one drive cycle with its trace (t, v)
  GET  /api/projects           the user's saved projects
  GET  /api/projects/{id}      load a project (+ its file revision, also as ETag)
  PUT  /api/projects/{id}      save a project (If-Match: the revision it was
                               loaded from → 409 if the file changed since;
                               If-None-Match: * → 409 if it already exists)
  DELETE /api/projects/{id}    delete a project (and its stored runs)
  GET  /api/projects/{id}/backups          earlier versions kept by saves, newest first
  GET  /api/projects/{id}/backups/{backup} one earlier version of the project
  GET  /api/projects/{id}/runs         the project's stored runs, newest first
  GET  /api/projects/{id}/runs/{run}   one stored run (gzip-encoded JSON)
  PUT  /api/projects/{id}/runs/{run}   store a finished run
  DELETE /api/projects/{id}/runs/{run} delete one stored run
  DELETE /api/projects/{id}/runs       delete all of the project's stored runs
  GET  /api/examples           the examples shipped with the app (+ hidden flag)
  GET  /api/examples/{id}      one example, read-only (opened as a copy)
  POST /api/examples/{id}/hide leave an example out of the Open menu
  POST /api/examples/restore   show every hidden example again
  POST /api/validate           run Data Checks on a project
  POST /api/simulate           run a simulation case, returns SimResult
  WS   /api/simulate/run       live run: streams progress/steps, accepts
                               set_param and cancel while running
  POST /api/studies            run a study's points on all cores, returns
                               each point's summary (runs are stored)
  WS   /api/studies/run        the same, streaming each point as it ends,
                               accepts cancel
"""
from __future__ import annotations

import asyncio
import gzip
import threading

from fastapi import (
    FastAPI,
    Header,
    HTTPException,
    Request,
    Response,
    WebSocket,
    WebSocketDisconnect,
)
from fastapi.middleware.cors import CORSMiddleware
from fastapi.staticfiles import StaticFiles
from pydantic import ValidationError

from . import cycles, run_store, security, storage, studies
from .library import load_library, unit_groups
from .paths import static_dir
from .schemas import (
    DataCheck,
    Project,
    SimResult,
    SimulateRequest,
    StoredRun,
    StudyRequest,
    ValidateRequest,
)
from .solver import simulate
from .validation import validate_project
from .version import VERSION

app = FastAPI(title="LightSim API", version=VERSION)

# Built frontend bundle (produced by `npm run build` → frontend/dist). When it
# exists we serve it below so the whole app runs from this one process at :8000
# with no Vite dev server. The packaged desktop app points LIGHTSIM_STATIC_DIR
# at its own copy; otherwise this resolves to the repo's frontend/dist.
FRONTEND_DIST = static_dir()

# Set by the desktop shell: a per-launch secret every /api call must carry.
LAUNCH_TOKEN = security.launch_token()

app.add_middleware(
    CORSMiddleware,
    # Only the Vite dev server calls from another origin (and it proxies /api
    # anyway). The desktop app and a built bundle are served from this origin
    # and need no CORS at all.
    allow_origins=[] if LAUNCH_TOKEN else list(security.DEV_ORIGINS),
    allow_methods=["GET", "POST", "PUT", "DELETE"],
    allow_headers=["Content-Type", "Authorization", "If-Match", "If-None-Match"],
    expose_headers=["ETag"],
)
# Added last so it runs first: other hosts, other origins and (in the desktop
# app) requests without the launch token never reach CORS or the routes.
app.add_middleware(
    security.LocalOnlyMiddleware,
    token=LAUNCH_TOKEN,
    hosts=security.allowed_hosts(),
    origins=() if LAUNCH_TOKEN else security.DEV_ORIGINS,
)


@app.get("/api/health")
def health() -> dict:
    return {"status": "ok", "service": "lightsim-backend", "version": VERSION}


@app.get("/api/library")
def get_library() -> dict:
    return {
        "components": [c.model_dump() for c in load_library()],
        "unitGroups": unit_groups(),
    }


@app.get("/api/cycles")
def get_cycles() -> list[dict]:
    return cycles.listing()


@app.get("/api/cycles/{cycle_id}")
def get_cycle(cycle_id: str) -> dict:
    try:
        pts = cycles.trace(cycle_id)
    except KeyError:
        raise HTTPException(status_code=404, detail=f"Drive cycle '{cycle_id}' not found")
    return {**cycles.info(cycle_id), "t": [p[0] for p in pts], "v": [p[1] for p in pts]}


@app.get("/api/projects")
def get_projects() -> list[dict]:
    return storage.list_projects()


@app.get("/api/examples")
def get_examples() -> list[dict]:
    return storage.list_examples()


@app.get("/api/examples/{example_id}")
def get_example(example_id: str) -> dict:
    """An example as the app ships it. It carries no revision: it is not a
    file of the user's, and the UI opens it as an unsaved copy with an id of
    its own, so saving it makes a new project and never writes the example."""
    try:
        return storage.load_example(example_id).model_dump(mode="json")
    except FileNotFoundError:
        raise HTTPException(status_code=404, detail=f"Example '{example_id}' not found")
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))


@app.post("/api/examples/restore")
def restore_examples() -> dict:
    return {"restored": storage.restore_examples()}


@app.post("/api/examples/{example_id}/hide")
def hide_example(example_id: str) -> dict:
    try:
        storage.hide_example(example_id)
    except FileNotFoundError:
        raise HTTPException(status_code=404, detail=f"Example '{example_id}' not found")
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))
    return {"hidden": example_id}


def _etag(revision: str) -> str:
    return f'"{revision}"'


def _revisions(header: str) -> list[str]:
    """Entity tags listed in an If-Match / If-None-Match header, unquoted."""
    tags = [t.strip() for t in header.split(",")]
    return [t.removeprefix("W/").strip('"') for t in tags if t]


@app.get("/api/projects/{project_id}")
def get_project(project_id: str, response: Response) -> dict:
    """The project plus `revision`, which identifies this version of its file.
    Send it back on save (If-Match) so a save never overwrites newer work."""
    try:
        project, revision = storage.load_project_file(project_id)
    except FileNotFoundError:
        raise HTTPException(status_code=404, detail=f"Project '{project_id}' not found")
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))
    response.headers["ETag"] = _etag(revision)
    return {**project.model_dump(mode="json"), "revision": revision}


@app.put("/api/projects/{project_id}")
def put_project(
    project_id: str,
    project: Project,
    response: Response,
    if_match: str | None = Header(None),
    if_none_match: str | None = Header(None),
) -> dict:
    if project.id != project_id:
        raise HTTPException(status_code=400, detail="Project id mismatch")
    # the revision GET returned is bookkeeping, never part of the file; a
    # client that sends it back in the body gets it checked like If-Match
    body_revision = (project.model_extra or {}).pop("revision", None)
    expected = None
    if if_match:
        tags = _revisions(if_match)
        expected = "*" if "*" in tags else (tags[0] if len(tags) == 1 else None)
        if expected is None:
            raise HTTPException(status_code=400, detail="If-Match must name one revision")
    elif isinstance(body_revision, str):
        expected = body_revision
    create_only = bool(if_none_match) and "*" in _revisions(if_none_match)
    try:
        revision = storage.save_project(project, expected, create_only=create_only)
    except storage.ConflictError as e:
        raise HTTPException(status_code=409, detail=str(e))
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))
    response.headers["ETag"] = _etag(revision)
    return {"saved": project_id, "revision": revision}


@app.delete("/api/projects/{project_id}")
def remove_project(project_id: str) -> dict:
    try:
        deleted = storage.delete_project(project_id)
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))
    if not deleted:
        raise HTTPException(status_code=404, detail=f"Project '{project_id}' not found")
    try:
        run_store.clear_runs(project_id)
    except ValueError:
        pass  # an id no run could have been stored under
    return {"deleted": project_id}


@app.get("/api/projects/{project_id}/backups")
def get_backups(project_id: str) -> list[dict]:
    try:
        return storage.list_backups(project_id)
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))


@app.get("/api/projects/{project_id}/backups/{backup_id}")
def get_backup(project_id: str, backup_id: str) -> dict:
    """An earlier version of the project. It carries no revision: it is not
    the file on disk, and a save of it must not replace that file unasked."""
    try:
        return storage.load_backup(project_id, backup_id).model_dump(mode="json")
    except FileNotFoundError:
        raise HTTPException(status_code=404, detail=f"Backup '{backup_id}' not found")
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))


@app.get("/api/projects/{project_id}/runs")
def get_runs(project_id: str) -> list[dict]:
    try:
        return run_store.list_runs(project_id)
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))


@app.get("/api/projects/{project_id}/runs/{run_id}")
def get_run(project_id: str, run_id: str, request: Request) -> Response:
    try:
        data = run_store.run_path(project_id, run_id).read_bytes()
    except FileNotFoundError:
        raise HTTPException(status_code=404, detail=f"Run '{run_id}' not found")
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))
    # stored gzip-compressed: send it as is and let the client inflate it
    if "gzip" in request.headers.get("accept-encoding", ""):
        return Response(data, media_type="application/json", headers={"Content-Encoding": "gzip"})
    return Response(gzip.decompress(data), media_type="application/json")


@app.put("/api/projects/{project_id}/runs/{run_id}")
def put_run(project_id: str, run_id: str, run: StoredRun) -> dict:
    if run.id != run_id:
        raise HTTPException(status_code=400, detail="Run id mismatch")
    try:
        runs, pruned = run_store.save_run(project_id, run)
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))
    return {
        "saved": run_id,
        "stored": len(runs),
        "bytes": sum(r.get("bytes", 0) for r in runs),
        "budget": run_store.BUDGET_BYTES,
        "pruned": pruned,
    }


@app.delete("/api/projects/{project_id}/runs/{run_id}")
def remove_run(project_id: str, run_id: str) -> dict:
    try:
        deleted = run_store.delete_run(project_id, run_id)
        stored = len(run_store.list_runs(project_id))
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))
    if not deleted:
        raise HTTPException(status_code=404, detail=f"Run '{run_id}' not found")
    return {"deleted": run_id, "stored": stored}


@app.delete("/api/projects/{project_id}/runs")
def remove_runs(project_id: str) -> dict:
    try:
        count = run_store.clear_runs(project_id)
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))
    return {"deleted": count, "stored": 0}


@app.post("/api/validate")
def validate(req: ValidateRequest) -> list[DataCheck]:
    return validate_project(req.project)


@app.post("/api/simulate")
def run_simulation(req: SimulateRequest) -> SimResult:
    checks = validate_project(req.project)
    errors = [c for c in checks if c.level == "error"]
    if errors:
        return SimResult(
            caseId=req.caseId,
            status="failed",
            messages=[{"level": "error", "text": f"Data check failed: {c.text}"} for c in errors],
            channels=[],
        )
    return simulate(req.project, req.caseId)


@app.websocket("/api/simulate/run")
async def run_simulation_live(ws: WebSocket) -> None:
    """Live simulation channel.

    Client → server: {"type": "start", "project": …, "caseId": …}, then
    optionally {"type": "set_param", "elementId", "key", "value"} and
    {"type": "cancel"}. Server → client: "step" / "message" events while
    running, then a final {"type": "done", "result": SimResult}.
    """
    await ws.accept()
    try:
        first = await ws.receive_json()
    except (WebSocketDisconnect, ValueError):
        return
    if first.get("type") != "start":
        await ws.send_json({"type": "error", "detail": "First message must be 'start'."})
        await ws.close()
        return
    try:
        project = Project.model_validate(first.get("project"))
    except ValidationError as e:
        await ws.send_json({"type": "error", "detail": f"Invalid project: {e.error_count()} schema error(s)."})
        await ws.close()
        return
    case_id = str(first.get("caseId", ""))

    checks = validate_project(project)
    errors = [c for c in checks if c.level == "error"]
    if errors:
        failed = SimResult(
            caseId=case_id,
            status="failed",
            messages=[{"level": "error", "text": f"Data check failed: {c.text}"} for c in errors],
            channels=[],
        )
        await ws.send_json({"type": "done", "result": failed.model_dump()})
        await ws.close()
        return

    loop = asyncio.get_running_loop()
    events: asyncio.Queue = asyncio.Queue()
    pending_control: list[dict] = []
    control_lock = threading.Lock()

    def emit(event: dict) -> None:  # called from the solver thread
        loop.call_soon_threadsafe(events.put_nowait, event)

    def control() -> list[dict]:  # polled by the solver thread
        with control_lock:
            msgs = list(pending_control)
            pending_control.clear()
        return msgs

    async def receive_loop() -> None:
        try:
            while True:
                msg = await ws.receive_json()
                kind = msg.get("type")
                if kind in ("set_param", "cancel"):
                    with control_lock:
                        pending_control.append(msg)
        except (WebSocketDisconnect, ValueError, RuntimeError):
            # client gone or socket closed — stop the run
            with control_lock:
                pending_control.append({"type": "cancel"})

    sim_future = asyncio.create_task(asyncio.to_thread(simulate, project, case_id, emit, control))
    recv_task = asyncio.create_task(receive_loop())
    client_gone = False
    try:
        while not (sim_future.done() and events.empty()):
            try:
                event = await asyncio.wait_for(events.get(), timeout=0.1)
            except asyncio.TimeoutError:
                continue
            if not client_gone:
                try:
                    await ws.send_json(event)
                except (WebSocketDisconnect, RuntimeError):
                    client_gone = True
                    with control_lock:
                        pending_control.append({"type": "cancel"})
        result = await sim_future
        if not client_gone:
            await ws.send_json({"type": "done", "result": result.model_dump()})
    finally:
        recv_task.cancel()
        if not client_gone:
            try:
                await ws.close()
            except RuntimeError:
                pass


def _study_args(req: StudyRequest):
    points = [studies.StudyPointSpec(overrides=p.overrides, values=p.values, label=p.label)
              for p in req.points]
    options = studies.StudyOptions(sweep_id=req.sweepId, sweep_param=req.sweepParam,
                                   sweep_unit=req.sweepUnit, workers=req.workers,
                                   store=req.store)
    return points, options


def _study_blocked(req: StudyRequest) -> str | None:
    """Why the study cannot start (no such case, Data Check errors), or None."""
    if not any(c.id == req.caseId for c in req.project.cases):
        return f"Simulation case '{req.caseId}' not found."
    errors = [c for c in validate_project(req.project) if c.level == "error"]
    if errors:
        return "Data check failed: " + "; ".join(c.text for c in errors[:5])
    return None


@app.post("/api/studies")
async def run_study(req: StudyRequest) -> dict:
    """Run every point (in a pool of worker processes) and answer once all
    are done: each point's status, run id and summary, and the pool's size,
    wall time and speed-up. For a headless caller; the app uses the
    WebSocket below to see points as they end and to stop a study."""
    blocked = _study_blocked(req)
    if blocked:
        raise HTTPException(status_code=400, detail=blocked)
    points, options = _study_args(req)
    found: list[dict] = []

    async def collect(event: dict) -> None:
        found.append(event)

    totals = await studies.run_study(req.project, req.caseId, points, options, collect,
                                     asyncio.Event())
    return {**totals, "points": sorted(found, key=lambda e: e["index"])}


@app.websocket("/api/studies/run")
async def run_study_live(ws: WebSocket) -> None:
    """Study channel. Client → server: {"type": "start", …StudyRequest},
    then optionally {"type": "cancel"}. Server → client: {"type": "started",
    "workers"}, a {"type": "point", …} as each point ends (in the order they
    end), then {"type": "done", "workers", "wallS", "pointWallS",
    "speedup"}; {"type": "error", "detail"} when it cannot start."""
    await ws.accept()
    try:
        first = await ws.receive_json()
    except (WebSocketDisconnect, ValueError):
        return
    try:
        if first.get("type") != "start":
            raise ValueError("First message must be 'start'.")
        req = StudyRequest.model_validate(first)
    except (ValueError, ValidationError) as e:
        detail = (f"Invalid study: {e.error_count()} schema error(s)."
                  if isinstance(e, ValidationError) else str(e))
        await ws.send_json({"type": "error", "detail": detail})
        await ws.close()
        return
    blocked = await asyncio.to_thread(_study_blocked, req)
    if blocked:
        await ws.send_json({"type": "error", "detail": blocked})
        await ws.close()
        return
    points, options = _study_args(req)
    stop = asyncio.Event()
    gone = False

    async def send(event: dict) -> None:
        nonlocal gone
        if gone:
            return
        try:
            await ws.send_json(event)
        except (WebSocketDisconnect, RuntimeError):
            gone = True
            stop.set()

    async def receive_loop() -> None:
        try:
            while True:
                msg = await ws.receive_json()
                if msg.get("type") == "cancel":
                    stop.set()
        except (WebSocketDisconnect, ValueError, RuntimeError):
            stop.set()  # client gone: stop the study

    options.workers = options.workers or studies.default_workers(req.project, len(points))
    await send({"type": "started", "workers": min(options.workers, len(points)),
                "points": len(points)})
    recv_task = asyncio.create_task(receive_loop())
    try:
        totals = await studies.run_study(req.project, req.caseId, points, options, send, stop)
        await send({"type": "done", **totals})
    finally:
        recv_task.cancel()
        if not gone:
            try:
                await ws.close()
            except RuntimeError:
                pass


# Serve the built single-page app. Registered last so every /api/* route and the
# WebSocket above are matched first; StaticFiles(html=True) then serves index.html
# at "/" and hashed assets from /assets/*. Skipped when the bundle hasn't been
# built (dev runs Vite separately; the test suite has no dist) so nothing breaks.
if FRONTEND_DIST is not None:
    app.mount("/", StaticFiles(directory=str(FRONTEND_DIST), html=True), name="spa")
