"""LightSim backend — FastAPI app.

Endpoints:
  GET  /docs                   this API's reference page (offline; /openapi.json)
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
  POST /api/fmus               import an FMU file (raw bytes; ?name=, ?allow=)
  POST /api/fmus/describe      what an FMU block's file is (variables, platforms)
  POST /api/fmus/{sha}/allow   allow an FMU to run on this computer
  GET  /api/policy             the settings the machine-wide policy file fixes
  POST /api/scripts/check      a project's Script code, each marked approved or not
  POST /api/scripts/approve    approve Script code to run (the user said yes)
  POST /api/validate           run Data Checks on a project
  POST /api/simulate           run a simulation case, returns SimResult
  POST /api/label-estimate     US window-sticker estimate from UDDS and HWFET (CON-32)
  POST /api/vehicle-tests      one-click vehicle tests: acceleration, top speed, ... (CON-06)
  GET  /api/templates          vehicle templates with slots and a form (CON-18)
  POST /api/templates/{id}/new a new project from a template and the form's values
  POST /api/templates          save a model as a template; DELETE one of the user's
  WS   /api/simulate/run       live run: streams progress/steps, accepts
                               set_param and cancel while running
  POST /api/studies            run a study's points on all cores, returns
                               each point's summary (runs are stored)
  WS   /api/studies/run        the same, streaming each point as it ends,
                               accepts cancel
  Results export, table and parameter-sheet import: app/dataio/api.py
  POST /api/ai/overview        the Markdown summary Copy for AI puts on the
                               clipboard (the MCP lightsim_overview text)
  GET  /api/ai/connect         the AI apps LightSim is set up in, and when an
                               assistant last used it
  PUT  /api/ai/connect/{client}    add LightSim to that AI app's MCP settings
  DELETE /api/ai/connect/{client}  remove it again
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
from fastapi.responses import FileResponse, HTMLResponse
from fastapi.staticfiles import StaticFiles
from pydantic import BaseModel, ValidationError

from . import (
    api_docs,
    attachments,
    cycles,
    files,
    label,
    laplog,
    reference_results,
    run_store,
    script_trust,
    security,
    storage,
    studies,
    templates,
    trust,
    vehicle_tests,
)
from .dataio.api import router as dataio_router
from .fmu import info as fmu_info
from .fmu import store as fmu_store
from .library import load_library, unit_groups
from .migrations import NewerFileError
from .paths import static_dir
from .schemas import (
    AiConnection,
    CalibrateRequest,
    DataCheck,
    ErrorDetail,
    FmuImport,
    FmuRef,
    LabelEstimateRequest,
    LapLogRequest,
    OverviewRequest,
    OverviewText,
    Project,
    SimCase,
    SimResult,
    SimulateRequest,
    StoredRun,
    Study,
    StudyRequest,
    TemplateNewRequest,
    TemplateSaveRequest,
    ValidateRequest,
    VehicleTestsRequest,
)
from .solver import calibrate as lap_calibration
from .solver import simulate
from .solver.domains import ModelInitError
from .solver.lapsim import LapError
from .solver.network import ModelError
from .sources import RunSources, sources_of
from .validation import run_blockers, validate_project
from .version import VERSION

# FastAPI's own /docs and /redoc load their scripts from a CDN; ours is
# served from here so it works offline (AI-07)
app = FastAPI(title="LightSim API", version=VERSION, docs_url=None, redoc_url=None)

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


app.include_router(dataio_router)


@app.get("/docs", include_in_schema=False)
def api_reference() -> HTMLResponse:
    return HTMLResponse(api_docs.render(app.openapi()))


@app.get("/api/health")
def health() -> dict:
    return {"status": "ok", "service": "lightsim-backend", "version": VERSION}


# At start, before anything is imported and saved: on the first start with
# the script check, the projects already saved here count as approved.
try:
    script_trust.trusted_hashes()
except OSError:
    pass


@app.get("/api/policy")
def get_policy() -> dict:
    """What the machine-wide policy file fixes (PLT-36), for the UI to show
    as managed by the organisation."""
    return {"settings": script_trust.policy(), "scriptTrust": script_trust.mode()}


class ScriptCheckRequest(BaseModel):
    project: dict


class ScriptReview(BaseModel):
    elementId: str
    label: str
    code: str
    hash: str
    approved: bool


class ScriptCheckReply(BaseModel):
    mode: str
    scripts: list[ScriptReview]
    unapproved: int


class ScriptApproveRequest(BaseModel):
    codes: list[str]


class ScriptApproveReply(BaseModel):
    approved: list[str]


@app.post("/api/scripts/check")
def check_scripts(req: ScriptCheckRequest) -> ScriptCheckReply:
    """Every Script code the project would run, each with `approved`. The UI
    asks before running a project whose scripts are not all approved."""
    trusted = script_trust.trusted_hashes()
    off = script_trust.mode() == "off"
    scripts = []
    for s in script_trust.project_scripts(req.project):
        h = script_trust.code_hash(s["code"])
        scripts.append(ScriptReview(**s, hash=h, approved=off or h in trusted))
    return ScriptCheckReply(mode=script_trust.mode(), scripts=scripts,
                            unapproved=sum(not s.approved for s in scripts))


@app.post("/api/scripts/approve")
def approve_scripts(req: ScriptApproveRequest) -> ScriptApproveReply:
    """Approve code to run: what the user typed in a Script block, or what
    they reviewed and chose Run scripts for."""
    return ScriptApproveReply(approved=script_trust.approve(req.codes))


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
    # The policy file can leave the examples out of a lab's PCs (PLT-36).
    if script_trust.policy().get("examples") is False:
        return []
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


@app.get("/api/examples/{example_id}/reference")
def example_reference(example_id: str) -> list[dict]:
    """The example's stored reference runs (CON-15): what the app shows in
    Results when the example opens, before anything is run."""
    try:
        if not storage.example_path(example_id).is_file():
            raise FileNotFoundError(example_id)
    except (FileNotFoundError, ValueError):
        raise HTTPException(status_code=404, detail=f"Example '{example_id}' not found")
    return reference_results.stored_results(example_id)


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


@app.get("/api/templates")
def get_templates() -> list[dict]:
    return templates.listing()


@app.post("/api/templates/{template_id}/new",
          responses=_errors(e400="The form's values do not fit the template", e404="No such template"))
def new_from_template(template_id: str, req: TemplateNewRequest) -> dict:
    """A new, unsaved project from a template, the form's values written in."""
    try:
        return templates.instantiate(template_id, req.values, req.name).model_dump(mode="json")
    except FileNotFoundError:
        raise HTTPException(status_code=404, detail=f"Template '{template_id}' not found")
    except ValueError as e:  # TemplateError, or a bad id
        raise HTTPException(status_code=400, detail=str(e))


@app.post("/api/templates", responses=_errors(e400="The model cannot be saved as a template"))
def save_template(req: TemplateSaveRequest) -> dict:
    try:
        form = [templates.FormField.model_validate(f) for f in req.form]
        t = templates.save_user_template(req.project, req.name, req.description, form, req.slots)
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))
    return t.model_dump(mode="json", exclude={"project"})


@app.delete("/api/templates/{template_id}",
            responses=_errors(e400="A built-in template cannot be deleted", e404="No such template"))
def delete_template(template_id: str) -> dict:
    try:
        templates.delete_user_template(template_id)
    except FileNotFoundError:
        raise HTTPException(status_code=404, detail=f"Template '{template_id}' not found "
                                                    f"among your own (built-in ones stay)")
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))
    return {"deleted": template_id}


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


@app.get("/api/laplog/presets")
def get_laplog_presets() -> list[dict]:
    """The logger and lap simulator layouts the lap import knows (STD-35)."""
    return [{"name": k, "note": v["note"], "speedUnit": v["speed_unit"]}
            for k, v in laplog.PRESETS.items()]


@app.post("/api/laplog/read", responses={400: {"description": "The file cannot be read as a lap",
                                               "model": ErrorDetail}})
def read_laplog(req: LapLogRequest) -> dict:
    """A lap from a logger or lap simulator CSV as a Driving Task profile."""
    try:
        return laplog.read_lap(req.text, req.preset, req.columns, req.speedUnit, req.lap,
                               req.repeatToKm, req.driverChangeS).as_dict()
    except laplog.LapLogError as e:
        raise HTTPException(status_code=400, detail=str(e))


@app.post("/api/laplog/calibrate", responses={400: {"description": "A log cannot be read",
                                                    "model": ErrorDetail}})
def calibrate_lap(req: CalibrateRequest) -> dict:
    """Fit lap mode's grip scale and downforce to a logged lap, and predict
    another logged lap with them (VAL-38). The logs are not stored."""
    try:
        logs = [lap_calibration.read_logged_lap(x.text, x.columns, x.lap, x.speedUnit)
                for x in (req.calibration, req.check) if x is not None]
        fit = lap_calibration.calibrate(req.project, logs[0])
        out = {"fit": fit, "calibration_lap": lap_calibration.predict(req.project, logs[0], fit["mu_scale"],
                                                          fit["cza"])}
        if len(logs) > 1:
            out["check_lap"] = lap_calibration.predict(req.project, logs[1], fit["mu_scale"], fit["cza"])
        return out
    except (laplog.LapLogError, KeyError, ValueError, LapError, ModelError, ModelInitError) as e:
        raise HTTPException(status_code=400, detail=str(e))


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


_FMU_ERRORS = {400: {"description": "Not an FMU, too big, unknown or an unreadable body",
                      "model": ErrorDetail}}


@app.post("/api/fmus", responses=_FMU_ERRORS, openapi_extra={
    "requestBody": {"required": True,
                    "content": {"application/octet-stream": {"schema": {"type": "string",
                                                                          "format": "binary"}}}}})
async def import_fmu(request: Request, name: str = "model.fmu", allow: bool = False) -> FmuImport:
    """Keep an FMU file the user chose (and, with ``allow``, allow it to run:
    the UI asks first). Returns where it is kept and what it is."""
    too_big = HTTPException(status_code=400, detail=(
        f"The FMU is larger than {fmu_store.MAX_FMU_BYTES >> 20} MB."))
    size = request.headers.get("content-length")
    if size and size.isdigit() and int(size) > fmu_store.MAX_FMU_BYTES:
        raise too_big
    chunks, total = [], 0
    async for chunk in request.stream():
        total += len(chunk)
        if total > fmu_store.MAX_FMU_BYTES:
            raise too_big
        chunks.append(chunk)
    data = b"".join(chunks)
    try:
        sha, path = await asyncio.to_thread(fmu_store.store_bytes, data)
    except fmu_store.FmuFileError as e:
        raise HTTPException(status_code=400, detail=str(e))
    file_name = Path(name.replace("\\", "/")).name[:200] or "model.fmu"
    if allow:
        fmu_store.allow(sha, file_name)
    info = await asyncio.to_thread(fmu_info.describe, path)
    return FmuImport(sha256=sha, path=str(path), name=file_name,
                     allowed=fmu_store.is_allowed(sha), info=info)


@app.post("/api/fmus/describe", responses=_FMU_ERRORS)
def describe_fmu(ref: FmuRef) -> FmuImport:
    """What an FMU block's file is, read without running it; ``found`` is
    False (with the reason in ``problem``) when the file is not there."""
    params = {"fmu_path": ref.fmuPath, "fmu_sha256": ref.fmuSha256, "fmu_name": ref.fmuName}
    try:
        path = fmu_store.locate(params)
    except fmu_store.FmuFileError as e:
        return FmuImport(found=False, problem=str(e), name=ref.fmuName,
                         sha256=ref.fmuSha256, path=ref.fmuPath)
    sha = fmu_store.sha256_of_cached(path)
    return FmuImport(sha256=sha, path=str(path), name=ref.fmuName or path.name,
                     allowed=fmu_store.is_allowed(sha), info=fmu_info.describe(path))


@app.post("/api/fmus/{sha256}/allow", responses=_FMU_ERRORS)
def allow_fmu(sha256: str, name: str = "") -> dict:
    """Allow an FMU (by its fingerprint) to run on this computer."""
    try:
        fmu_store.allow(sha256, name or sha256[:12])
    except fmu_store.FmuFileError as e:
        raise HTTPException(status_code=400, detail=str(e))
    return {"allowed": True}


@app.post("/api/validate")
def validate(req: ValidateRequest) -> list[DataCheck]:
    return _checks(req.project)


@app.post("/api/sources")
def run_sources(req: SimulateRequest) -> RunSources:
    """The data and methods a run of the case rests on, with their licences,
    credits and citations (VAL-37). Run info asks with the run's snapshot."""
    return sources_of(req.project, req.caseId)


@app.post("/api/simulate")
def run_simulation(req: SimulateRequest) -> SimResult:
    checks = _checks(req.project)
    errors = run_blockers(checks, req.caseId)  # (not another case's own errors)
    if errors:
        return SimResult(
            caseId=req.caseId,
            status="failed",
            messages=[{"level": "error", "text": f"Data check failed: {c.text}"} for c in errors],
            channels=[],
        )
    return simulate(req.project, req.caseId)


@app.post("/api/label-estimate", responses=_errors(e400="Data Checks fail, or the model has no case to base the estimate on"))
def us_label_estimate(req: LabelEstimateRequest) -> dict:
    """CON-32: the model on EPA's city and highway cycles, adjusted to a US
    window-sticker estimate, every step shown; not a certified value."""
    # the model's errors, and those of the cases the estimate runs (or copies)
    try:
        runs = {c.id for c in label.label_cases(req.project, req.caseId).values()}
    except ValueError:
        runs = set()  # (the estimate says why it cannot run)
    base = label.base_case(req.project, req.caseId)
    runs |= {base.id} if base else set()
    errors = [c.text for c in validate_project(req.project) if c.level == "error"
              and (c.caseId is None or c.caseId in runs)]
    if errors:
        raise HTTPException(status_code=400, detail=f"Data check failed: {errors[0]}")
    try:
        return label.estimate(req.project, req.caseId, req.modelYear)
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e)) from e


@app.post("/api/vehicle-tests", responses=_errors(e400="Data Checks fail, or a test cannot be set up on this model"))
def run_vehicle_tests(req: VehicleTestsRequest) -> dict:
    """CON-06: 0-100 and 80-120 km/h, top speed, constant-speed consumption,
    gradeability and a virtual coast-down on the model as it is."""
    # the tests set up cases of their own (Performance cases on the Driving
    # Task): the project's cases' own errors do not stop them, a Performance
    # case's do
    trial = req.project.model_copy(update={"cases": [SimCase(
        id="vehicle-tests", name="Vehicle tests", kind="performance", duration=60.0,
        timeStep=0.1)]})
    errors = [c.text for c in run_blockers(validate_project(trial), "vehicle-tests")]
    if errors:
        raise HTTPException(status_code=400, detail=f"Data check failed: {errors[0]}")
    try:
        return vehicle_tests.run_tests(req.project, req.tests)
    except vehicle_tests.TestSetupError as e:
        raise HTTPException(status_code=400, detail=str(e)) from e


# An error answer as FastAPI sends it, for the routes below to declare.
_ERROR = {"content": {"application/json": {"schema": {
    "type": "object", "properties": {"detail": {"type": "string"}}, "required": ["detail"]}}}}


@app.post("/api/ai/overview", responses={400: {"description": "Unreadable request body", **_ERROR}})
def ai_overview(req: OverviewRequest) -> OverviewText:
    """Copy for AI (AI-30): nothing is sent anywhere; the UI copies the text."""
    from .ai.overview import model_overview

    run = None
    if req.run is not None:
        run = {"caseName": req.run.caseName, "status": req.run.status,
               "incomplete": req.run.incomplete,
               "result": {"summary": [s.model_dump() for s in req.run.summary],
                          "messages": [m.model_dump() for m in req.run.messages]}}
    text = model_overview(req.project, run=run, checks=validate_project(req.project),
                          hide_values=req.hideValues)
    return OverviewText(text=text, bytes=len(text.encode("utf-8")))


def _ai_connection() -> AiConnection:
    from .ai import install
    from .ai.access import last_audit_entry
    from .paths import projects_dir

    return AiConnection(
        command=install.server_command(),
        warning=install.command_warning(),
        clients=[{"id": k, "title": c.title, "installed": install.is_installed(k),
                  "configPath": str(c.config_path())} for k, c in install.CLIENTS.items()],
        lastUsed=last_audit_entry(projects_dir()),
    )


@app.get("/api/ai/connect")
def ai_connection() -> AiConnection:
    return _ai_connection()


@app.put("/api/ai/connect/{client}", responses={
    404: {"description": "No such AI app", **_ERROR},
    409: {"description": "Its settings file could not be changed safely", **_ERROR}})
def ai_connect(client: str) -> AiConnection:
    """Add LightSim to the AI app's MCP settings (AI-29). The entry points
    at this engine and at the folder this app saves projects to."""
    from .ai import install
    from .paths import projects_dir

    if client not in install.CLIENTS:
        raise HTTPException(status_code=404, detail=f"No AI app '{client}'")
    try:
        install.install(client, extra_args=["--projects-dir", str(projects_dir())])
    except install.InstallError as e:
        raise HTTPException(status_code=409, detail=str(e))
    # connecting is the user's yes to AI access for their projects (AI-01)
    from .ai.engine import grant_folder

    grant_folder(projects_dir())
    return _ai_connection()


@app.delete("/api/ai/connect/{client}", responses={
    404: {"description": "No such AI app", **_ERROR},
    409: {"description": "Its settings file could not be changed safely", **_ERROR}})
def ai_disconnect(client: str) -> AiConnection:
    from .ai import install

    if client not in install.CLIENTS:
        raise HTTPException(status_code=404, detail=f"No AI app '{client}'")
    try:
        install.uninstall(client)
    except install.InstallError as e:
        raise HTTPException(status_code=409, detail=str(e))
    return _ai_connection()


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
    errors = run_blockers(checks, case_id)
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
    errors = run_blockers(validate_project(req.project), req.caseId)
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
