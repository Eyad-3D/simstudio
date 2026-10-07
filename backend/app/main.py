"""LightSim backend — FastAPI app.

Endpoints:
  GET  /api/library            component library definitions
  GET  /api/cycles             the bundled standard drive cycles
  GET  /api/cycles/{id}        one drive cycle with its trace (t, v)
  GET  /api/projects           the user's saved projects
  GET  /api/projects/{id}      load a project (+ its file revision, also as ETag),
                               upgraded to the current file format (PLT-07)
  PUT  /api/projects/{id}      save a project (If-Match: the revision it was
                               loaded from → 409 if the file changed since;
                               If-None-Match: * → 409 if it already exists;
                               409 too over a file from a newer LightSim)
  DELETE /api/projects/{id}    delete a project (and its stored runs)
  GET  /api/projects/{id}/revision     the revision of its file on disk now
  POST /api/projects/upgrade   a project file's JSON in the current format
  GET  /api/files              .lightsim files opened or saved (Recent files)
  POST /api/files/open         remember a .lightsim file the user picked (shell only)
  POST /api/files/save-as      remember where to save a project as a .lightsim
                               file (shell only)
  DELETE /api/files/{id}       take a file off Recent files (the file stays)
  GET  /api/projects/{id}/studies          the project's parameter studies
  PUT  /api/projects/{id}/studies/{study}  store a study (PLT-34)
  DELETE /api/projects/{id}/studies/{study}
  GET  /api/projects/{id}/attachments          files attached to the project
  POST /api/projects/{id}/attachments?name=…   attach a file (body: its bytes)
  GET  /api/projects/{id}/attachments/{name}   an attached file's bytes
  DELETE /api/projects/{id}/attachments/{name}
  POST /api/bundle             the project as one zip file (project + attachments)
  POST /api/bundle/import      unpack a bundle (body: the zip file)
  GET  /api/trust/{fingerprint}   whether the user trusts this code to run
  POST /api/trust/{fingerprint}   remember that the user does
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
"""
from __future__ import annotations

import asyncio
import gzip
import os
import shutil
import tempfile
import threading
from pathlib import Path

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
from fastapi.responses import FileResponse
from fastapi.staticfiles import StaticFiles
from pydantic import BaseModel, ValidationError

from . import attachments, cycles, files, run_store, security, storage, trust
from .library import load_library, unit_groups
from .migrations import NewerFileError
from .paths import static_dir
from .schemas import (
    DataCheck,
    Project,
    SimResult,
    SimulateRequest,
    StoredRun,
    Study,
    ValidateRequest,
)
from .solver import simulate
from .validation import validate_project
from .version import VERSION

app = FastAPI(title="LightSim API", version=VERSION)

class ErrorBody(BaseModel):
    detail: str


def _errors(**codes: str) -> dict:
    """`responses=` for a route: the error statuses it answers with on
    purpose (e.g. _errors(e404="No such project")), each with its reason."""
    return {int(k[1:]): {"description": v, "model": ErrorBody} for k, v in codes.items()}


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
    allow_headers=["Content-Type", "Authorization", "If-Match", "If-None-Match", "X-LightSim-Shell"],
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


#: What GET /api/projects/{id} adds to a project, which is bookkeeping and
#: never part of the file: a save drops it.
BOOKKEEPING = ("revision", "readOnly", "upgradedFrom", "filePath")


@app.get("/api/projects/{project_id}")
def get_project(project_id: str, response: Response) -> dict:
    """The project plus `revision`, which identifies this version of its file.
    Send it back on save (If-Match) so a save never overwrites newer work.
    Also `filePath` for a .lightsim file outside the projects folder,
    `upgradedFrom` when the file was in an older format (it is upgraded; the
    next save writes the new format), and `readOnly` with the reason when the
    file is from a newer LightSim and must not be saved over."""
    try:
        loaded = storage.load_project_file(project_id)
    except FileNotFoundError:
        raise HTTPException(status_code=404, detail=f"Project '{project_id}' not found")
    except NewerFileError as e:
        raise HTTPException(status_code=409, detail=str(e))
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))
    if loaded.studies:
        run_store.adopt_studies(loaded.project.id, loaded.studies)
    response.headers["ETag"] = _etag(loaded.revision)
    extra: dict = {"revision": loaded.revision}
    if loaded.location is not None and loaded.location.external:
        extra["filePath"] = str(loaded.location.file)
    if loaded.upgraded_from is not None:
        extra["upgradedFrom"] = loaded.upgraded_from
    if loaded.read_only:
        extra["readOnly"] = loaded.read_only
    return {**loaded.project.model_dump(mode="json"), **extra}


@app.get("/api/projects/{project_id}/revision", responses=_errors(e400="Invalid project id"))
def get_revision(project_id: str) -> dict:
    """The revision of the project's file on disk now (None: there is none).
    The UI asks every few seconds, to offer a reload when the file changed
    outside the app (a git pull, another window)."""
    try:
        return {"revision": storage.current_revision(project_id)}
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))


@app.post("/api/projects/upgrade", responses=_errors(e400="Not a project", e409="A file from a newer LightSim that this one cannot read"))
def upgrade_project(raw: dict) -> dict:
    """A project file's JSON (an imported file) in the current format, with
    the studies an upgrade took out of it (to store with the project's runs)
    and, for a file from a newer LightSim, why it is read-only."""
    for key in BOOKKEEPING:
        raw.pop(key, None)
    try:
        read = storage.read_project(raw)
    except NewerFileError as e:
        raise HTTPException(status_code=409, detail=str(e))
    except ValueError as e:
        raise HTTPException(status_code=400, detail=f"Not a LightSim project: {e}")
    out: dict = {"project": read.project.model_dump(mode="json"), "studies": read.studies,
                 "upgradedFrom": read.upgraded_from, "readOnly": read.read_only}
    return out


@app.put("/api/projects/{project_id}")
def put_project(
    project_id: str,
    body: dict,
    response: Response,
    if_match: str | None = Header(None),
    if_none_match: str | None = Header(None),
) -> dict:
    # the revision GET returned is bookkeeping, never part of the file; a
    # client that sends it back in the body gets it checked like If-Match
    body_revision = body.pop("revision", None)
    for key in BOOKKEEPING:
        body.pop(key, None)
    try:
        read = storage.read_project(body)
    except NewerFileError as e:
        raise HTTPException(status_code=409, detail=str(e))
    except ValidationError as e:
        raise HTTPException(status_code=422, detail=e.errors(include_url=False, include_context=False))
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))
    project = read.project
    if read.read_only:
        raise HTTPException(status_code=409, detail=read.read_only)
    if project.id != project_id:
        raise HTTPException(status_code=400, detail="Project id mismatch")
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
    except (storage.ConflictError, NewerFileError) as e:
        raise HTTPException(status_code=409, detail=str(e))
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))
    if read.studies:  # an old file's studies, sent back by an older UI
        run_store.adopt_studies(project.id, read.studies)
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
    run_store.clear_all(project_id)
    shutil.rmtree(storage.location(project_id).resources, ignore_errors=True)
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


# ---- studies (PLT-34) -------------------------------------------------------

@app.get("/api/projects/{project_id}/studies", responses=_errors(e400="Invalid project id"))
def get_studies(project_id: str) -> list[dict]:
    try:
        return run_store.list_studies(project_id)
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))


@app.put("/api/projects/{project_id}/studies/{study_id}", responses=_errors(e400="Invalid id or id mismatch"))
def put_study(project_id: str, study_id: str, study: Study) -> dict:
    if study.id != study_id:
        raise HTTPException(status_code=400, detail="Study id mismatch")
    try:
        run_store.save_study(project_id, study)
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))
    return {"saved": study_id}


@app.delete("/api/projects/{project_id}/studies/{study_id}", responses=_errors(e400="Invalid id", e404="No such study"))
def remove_study(project_id: str, study_id: str) -> dict:
    try:
        deleted = run_store.delete_study(project_id, study_id)
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))
    if not deleted:
        raise HTTPException(status_code=404, detail=f"Study '{study_id}' not found")
    return {"deleted": study_id}


# ---- .lightsim files anywhere (PLT-33) ------------------------------------

class FileOpenRequest(BaseModel):
    path: str


class FileSaveAsRequest(BaseModel):
    path: str
    projectId: str


def _shell_only(request: Request) -> None:
    if not security.may_name_paths(request.headers):
        raise HTTPException(
            status_code=403,
            detail="Only the LightSim desktop app can open or save files by path "
                   "(it shows the system's file dialogs).")


def _file_info(entry: files.OpenFile, name: str | None = None) -> dict:
    return {"id": entry.id, "path": str(entry.path), "name": name or entry.path.stem}


@app.get("/api/files")
def get_files() -> list[dict]:
    return storage.list_files()


@app.post("/api/files/open", responses=_errors(e400="Not a .lightsim project file", e403="Only the desktop shell names paths", e404="No such file", e409="A file from a newer LightSim that this one cannot read"))
def open_file(req: FileOpenRequest, request: Request) -> dict:
    """Remember a .lightsim file the user picked in the shell's Open dialog
    (or double-clicked, or dropped on the window); the page then loads it
    as project `id`."""
    _shell_only(request)
    try:
        path = files.checked_path(req.path, must_exist=True)
        read = storage.read_project(path.read_bytes())
        raw_id = read.project.id
        entry = files.open_file(str(path), raw_id)
    except FileNotFoundError:
        raise HTTPException(status_code=404, detail=f"File not found: {req.path}")
    except NewerFileError as e:
        raise HTTPException(status_code=409, detail=str(e))
    except (ValueError, OSError) as e:
        raise HTTPException(status_code=400, detail=f"Cannot open {req.path}: {e}")
    return _file_info(entry, read.project.name)


@app.post("/api/files/save-as", responses=_errors(e400="Not a .lightsim path, or an invalid project id", e403="Only the desktop shell names paths", e404="No such folder"))
def save_file_as(req: FileSaveAsRequest, request: Request) -> dict:
    """Remember the .lightsim file the user chose in the shell's Save dialog
    for project `projectId`. Returns the id to save the project under (its
    own, or a new one when the project is already saved elsewhere: then the
    new file is a copy). Attached files are copied along; the runs of a
    project that was never saved move with it. The page then saves (PUT)."""
    _shell_only(request)
    try:
        old = storage.location(req.projectId)
        was_saved = old.file.exists()
        entry = files.save_as(req.path, req.projectId)
    except FileNotFoundError:
        raise HTTPException(status_code=404, detail=f"Folder not found: {req.path}")
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))
    new = storage.location(entry.id)
    attachments.copy_all(old, new)
    runs_moved = False
    if (not was_saved and old.runs != new.runs and old.runs.is_dir()
            and not new.runs.exists()):
        shutil.move(str(old.runs), str(new.runs))
        new.make_dir(new.runs)
        runs_moved = True
    return {**_file_info(entry), "runsMoved": runs_moved}


@app.delete("/api/files/{project_id}", responses=_errors(e404="Not a recent file"))
def forget_file(project_id: str) -> dict:
    if not files.forget(project_id):
        raise HTTPException(status_code=404, detail=f"No recent file with id '{project_id}'")
    return {"forgotten": project_id}


# ---- attached files (STD-02) ----------------------------------------------

@app.get("/api/projects/{project_id}/attachments", responses=_errors(e400="Invalid project id"))
def get_attachments(project_id: str) -> list[dict]:
    try:
        return attachments.list_files(project_id)
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))


@app.post("/api/projects/{project_id}/attachments", responses=_errors(e400="Invalid project id or file name", e413="The file is too large"))
async def post_attachment(project_id: str, name: str, request: Request) -> dict:
    """Attach a file: the body is its bytes, `name` its file name. Returns
    its path ("resources/<name>"), SHA-256, size and kind; the UI then adds
    it to the project's list of attachments, which the next save keeps."""
    try:
        storage.location(project_id)
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))
    fd, tmp = tempfile.mkstemp(prefix="lightsim-upload-")
    try:
        size = 0
        with os.fdopen(fd, "wb") as f:
            async for chunk in request.stream():
                size += len(chunk)
                if size > attachments.MAX_FILE_BYTES:
                    raise HTTPException(status_code=413, detail="The file is too large to attach.")
                f.write(chunk)
        return await asyncio.to_thread(attachments.add, project_id, name, Path(tmp))
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))
    finally:
        Path(tmp).unlink(missing_ok=True)


@app.get("/api/projects/{project_id}/attachments/{name}",
         response_class=FileResponse,
         responses={200: {"content": {"application/octet-stream": {}}},
                    **_errors(e400="Invalid project id or file name", e404="No such attached file")})
def get_attachment(project_id: str, name: str) -> FileResponse:
    try:
        path = attachments.path_of(project_id, attachments.PREFIX + name)
    except FileNotFoundError:
        raise HTTPException(status_code=404, detail=f"No attached file '{name}'")
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))
    return FileResponse(path, media_type="application/octet-stream", filename=name)


@app.delete("/api/projects/{project_id}/attachments/{name}", responses=_errors(e400="Invalid project id or file name", e404="No such attached file"))
def remove_attachment(project_id: str, name: str) -> dict:
    try:
        deleted = attachments.remove(project_id, attachments.PREFIX + name)
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))
    if not deleted:
        raise HTTPException(status_code=404, detail=f"No attached file '{name}'")
    return {"deleted": name}


class BundleRequest(BaseModel):
    project: Project


@app.post("/api/bundle", response_class=Response,
          responses={200: {"content": {"application/zip": {}}}, **_errors(e400="Invalid project id")})
def export_bundle(req: BundleRequest) -> Response:
    """The project (as the UI holds it) and its attached files as one zip
    file, to send to someone."""
    try:
        data = attachments.export_bundle(req.project)
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))
    return Response(data, media_type="application/zip")


@app.post("/api/bundle/import", responses=_errors(e400="Not a bundle", e409="A file from a newer LightSim that this one cannot read"))
async def import_bundle(request: Request) -> dict:
    """Unpack a bundle made by POST /api/bundle: its project in the current
    format (as /api/projects/upgrade gives it), with its attached files put
    in that project's resources folder. A project whose id is taken gets a
    new one, so its files never mix with another project's."""
    data = await request.body()
    try:
        raw, resources = attachments.read_bundle(data)
        if not isinstance(raw, dict):
            raise ValueError("project.json is not a project")
        for key in BOOKKEEPING:
            raw.pop(key, None)
        read = storage.read_project(raw)
    except NewerFileError as e:
        raise HTTPException(status_code=409, detail=str(e))
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))
    project = read.project
    if storage.location(project.id).file.exists() or files.lookup(project.id) is not None:
        project.id = files.fresh_id(project.id)
    renamed: dict[str, str] = {}
    for name, content in resources.items():
        info = await asyncio.to_thread(attachments.add, project.id, name, content)
        if info["name"] != name:
            renamed[attachments.PREFIX + name] = info["path"]
    if renamed:
        attachments.rename_refs(project, renamed)
    return {"project": project.model_dump(mode="json"), "studies": read.studies,
            "upgradedFrom": read.upgraded_from, "readOnly": read.read_only}


# ---- trust (STD-02) --------------------------------------------------------

@app.get("/api/trust/{fingerprint}", responses=_errors(e400="Not a fingerprint"))
def get_trust(fingerprint: str) -> dict:
    try:
        return {"trusted": trust.is_trusted(fingerprint)}
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))


@app.post("/api/trust/{fingerprint}", responses=_errors(e400="Not a fingerprint"))
def post_trust(fingerprint: str) -> dict:
    try:
        trust.trust(fingerprint)
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))
    return {"trusted": True}


def _checks(project: Project) -> list[DataCheck]:
    """Data Checks, with those for the project's attached files."""
    return [*validate_project(project), *attachments.check(project)]


@app.post("/api/validate")
def validate(req: ValidateRequest) -> list[DataCheck]:
    return _checks(req.project)


@app.post("/api/simulate")
def run_simulation(req: SimulateRequest) -> SimResult:
    checks = _checks(req.project)
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

    checks = _checks(project)
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
