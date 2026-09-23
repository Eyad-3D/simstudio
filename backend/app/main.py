"""SimStudio backend — FastAPI app.

Endpoints:
  GET  /api/library            component library definitions
  GET  /api/projects           saved project list
  GET  /api/projects/{id}      load a project (+ its file revision, also as ETag)
  PUT  /api/projects/{id}      save a project (If-Match: the revision it was
                               loaded from → 409 if the file changed since;
                               If-None-Match: * → 409 if it already exists)
  DELETE /api/projects/{id}    delete a project (and its stored runs)
  GET  /api/projects/{id}/runs         the project's stored runs, newest first
  GET  /api/projects/{id}/runs/{run}   one stored run (gzip-encoded JSON)
  PUT  /api/projects/{id}/runs/{run}   store a finished run
  DELETE /api/projects/{id}/runs/{run} delete one stored run
  DELETE /api/projects/{id}/runs       delete all of the project's stored runs
  POST /api/validate           run Data Checks on a project
  POST /api/simulate           run a simulation case, returns SimResult
  WS   /api/simulate/run       live run: streams progress/steps, accepts
                               set_param and cancel while running
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

from . import run_store, security, storage
from .library import load_library, unit_groups
from .paths import static_dir
from .schemas import DataCheck, Project, SimResult, SimulateRequest, StoredRun, ValidateRequest
from .solver import simulate
from .validation import validate_project
from .version import VERSION

app = FastAPI(title="SimStudio API", version=VERSION)

# Built frontend bundle (produced by `npm run build` → frontend/dist). When it
# exists we serve it below so the whole app runs from this one process at :8000
# with no Vite dev server. The packaged desktop app points SIMSTUDIO_STATIC_DIR
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
    return {"status": "ok", "service": "simstudio-backend", "version": VERSION}


@app.get("/api/library")
def get_library() -> dict:
    return {
        "components": [c.model_dump() for c in load_library()],
        "unitGroups": unit_groups(),
    }


@app.get("/api/projects")
def get_projects() -> list[dict]:
    return storage.list_projects()


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


# Serve the built single-page app. Registered last so every /api/* route and the
# WebSocket above are matched first; StaticFiles(html=True) then serves index.html
# at "/" and hashed assets from /assets/*. Skipped when the bundle hasn't been
# built (dev runs Vite separately; the test suite has no dist) so nothing breaks.
if FRONTEND_DIST is not None:
    app.mount("/", StaticFiles(directory=str(FRONTEND_DIST), html=True), name="spa")
