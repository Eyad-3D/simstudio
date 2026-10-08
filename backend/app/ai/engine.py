"""The thin adapter between the AI tools and LightSim's engine.

Every tool in :mod:`app.ai.mcp_server` reaches projects, runs, Data Checks
and the solver through :class:`Engine` and nothing else. Today it calls the
engine's own modules in-process (no web server, no network). When the
automation lane's ``lightsim`` Python package (AI-02) lands, this is the one
file to switch over to it.

Which projects an assistant may see follows the user's AI access settings
(AI-01, ``lightsim/ai_access.py``, shared with the ``lightsim ai`` command):

- nothing at all while AI access is off (the default); connecting an AI app
  from LightSim (Help > Connect an AI assistant, or ``lightsim-backend mcp
  install``) turns it on and allows the folder the app saves to,
- the user's saved projects, when that folder is allowed,
- the examples shipped with the app, read-only, unless the user hid them,
- project files in the allowed folders and in folders listed when the
  assistant was connected (``--allow-folder``), and nothing outside them
  (a link is followed first: one to a file elsewhere is not shown),
- never a project whose file sets ``"noAi": true`` (or the older
  ``"noAI"``): it is left out of every list and reads as not found.
"""
from __future__ import annotations

import gzip
import hashlib
import json
import os
import re
import sys
import threading
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, Optional

from .. import files, run_store, storage
from ..library import library_by_id
from ..paths import projects_dir
from ..schemas import ComponentDef, DataCheck, Project, SimResult, StoredRun
from ..validation import run_blockers, validate_project

#: The project-file flag that hides a project from every AI connection
#: (docs/spec/project.md); "noAI" was its spelling before 0.3.0 was released.
NO_AI_FLAG = "noAi"
NO_AI_FLAGS = (NO_AI_FLAG, "noAI")
#: Prefix of an example's reference ("example:bev-car").
EXAMPLE_PREFIX = "example:"
#: Prefix of a blank project to build ("new:My car"); saved only by an edit.
NEW_PREFIX = "new:"
#: The library part that runs user-written Python.
SCRIPT_COMPONENT = "signal.script"


class NotFound(LookupError):
    """No project or run by that name that the assistant may see."""


@dataclass(frozen=True)
class ProjectHandle:
    """Where a project the assistant asked for lives."""

    ref: str  # what the assistant calls it: "bev-car", "example:bev-car" or a path
    kind: str  # "user", "example", "folder" or "new"
    path: Path
    #: the id the app keeps the project's runs under; None when the app keeps
    #: none (an example, a new project, a file the user never opened in the app)
    project_id: Optional[str]
    #: the folder under .ai/runs holding the assistant's own runs of it: one
    #: per project, so two projects never share a run history (a file's is
    #: named after its path, as two folders can hold a car.json each)
    runs_key: str

    @property
    def read_only(self) -> bool:
        return self.kind in ("example", "new")

    @property
    def backups(self) -> Optional[Path]:
        """Where a save keeps the version it replaces: beside a file in an
        allowed folder (as for a .lightsim file the app saves), or the
        projects folder's backups. None for an example or a new project,
        which an edit saves as a new project."""
        if self.kind == "folder":
            return _file_location(self.path).backups
        if self.kind == "user":
            return storage.location(self.ref).backups
        return None


def default_projects_dir() -> Path:
    """The folder the desktop app saves projects to.

    The app passes it to its engine in ``LIGHTSIM_PROJECTS_DIR``; an AI app
    starting the engine does not, so this works out the same place Electron
    uses (its per-user data folder, then ``projects``).
    """
    if os.environ.get("LIGHTSIM_PROJECTS_DIR"):
        return projects_dir()
    if getattr(sys, "frozen", False) or os.environ.get("LIGHTSIM_AI_DESKTOP_DATA"):
        home = Path.home()
        if sys.platform == "win32":
            base = Path(os.environ.get("APPDATA") or home / "AppData" / "Roaming")
        elif sys.platform == "darwin":
            base = home / "Library" / "Application Support"
        else:
            base = Path(os.environ.get("XDG_CONFIG_HOME") or home / ".config")
        return base / "LightSim" / "projects"
    return projects_dir()  # a source checkout: backend/dev-projects


def _hidden_from_ai(path: Path) -> bool:
    try:
        raw = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return False  # unreadable: loading it reports the problem
    return isinstance(raw, dict) and any(raw.get(f) is True for f in NO_AI_FLAGS)


def _listable(path: Path) -> Optional[dict]:
    """A project file's contents for the list, or None when it cannot be
    read or is hidden from AI."""
    try:
        raw = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return None
    if not isinstance(raw, dict) or any(raw.get(f) is True for f in NO_AI_FLAGS):
        return None
    return raw


def access_policy():
    """The user's AI access settings (AI-01), read afresh for every call, so
    'lightsim ai off' stops a connected assistant at once."""
    from lightsim.ai_access import Policy

    return Policy.load()


ACCESS_OFF = ("AI access to LightSim is off. The user can turn it on by connecting this AI app "
              "in LightSim (Help > Connect an AI assistant) or with 'lightsim ai on'.")


def grant_folder(folder: Path) -> None:
    """Turn AI access on and allow ``folder``: what connecting an AI app from
    LightSim means (AI-29)."""
    from lightsim.ai_access import Policy

    policy = Policy.load()
    policy.enabled = True
    real = str(Path(folder).expanduser().resolve())
    if real not in policy.folders:
        policy.folders.append(real)
    policy.save()


class Engine:
    """Projects, runs, Data Checks and the solver, as the AI tools see them."""

    def __init__(
        self,
        user_folder: Optional[Path] = None,
        allowed_folders: tuple[Path, ...] = (),
        include_examples: bool = True,
    ) -> None:
        self.user_folder = Path(user_folder) if user_folder else default_projects_dir()
        self.allowed_folders = tuple(Path(f).expanduser().resolve() for f in allowed_folders)
        self.include_examples = include_examples
        # the engine's own modules read the projects folder from here
        os.environ["LIGHTSIM_PROJECTS_DIR"] = str(self.user_folder)

    # -- projects ---------------------------------------------------------

    def access_refusal(self) -> Optional[str]:
        """Why an assistant may do nothing now, or None."""
        return None if access_policy().enabled else ACCESS_OFF

    def _folders(self, policy) -> list[Path]:
        """Folders whose project files the assistant may list: those given
        when it was connected and those the user allows (not the projects
        folder itself, listed as the user's projects)."""
        user = self.user_folder.expanduser().resolve()
        out = list(self.allowed_folders)
        for f in policy.folders:
            real = Path(f).expanduser().resolve()
            if real != user and real not in out:
                out.append(real)
        return out

    def _user_projects_visible(self, policy) -> bool:
        return policy.enabled and policy._in_folders(self.user_folder)

    def _may_open(self, path: Path, policy) -> bool:
        """True when the project file at ``path``, after following links, is
        in a folder the assistant may see. A link in an allowed folder to a
        file elsewhere is neither listed nor opened."""
        try:
            real = path.resolve()
        except (OSError, RuntimeError):
            return False
        return (real.suffix in (".json", ".lightsim")
                and (any(real.parent == f for f in self.allowed_folders) or policy._in_folders(real)))

    def list_projects(self) -> list[dict]:
        policy = access_policy()
        if not policy.enabled:
            raise NotFound(ACCESS_OFF)
        out = []
        if self._user_projects_visible(policy):
            ids: set[str] = set()
            for entry in storage.list_projects():
                # what opening the id reads (not what a file naming that id
                # holds), so a link or a stray copy shows nothing else
                pid = entry["id"]
                path = storage.project_path(pid)
                if pid in ids or not self._may_open(path, policy):
                    continue
                ids.add(pid)
                raw = _listable(path)
                if raw is not None:
                    out.append(self._entry(pid, "user", raw))
        if self.include_examples and policy.examples:
            for entry in storage.list_examples():
                if not _hidden_from_ai(storage.example_path(entry["id"])):
                    out.append(self._entry(EXAMPLE_PREFIX + entry["id"], "example", entry))
        seen: set[Path] = set()
        for folder in self._folders(policy):
            for f in sorted([*folder.glob("*.json"), *folder.glob("*.lightsim")]):
                if not self._may_open(f, policy):
                    continue
                real = f.resolve()
                if real in seen:
                    continue
                seen.add(real)
                raw = _listable(real)
                if raw is not None:
                    out.append(self._entry(str(real), "folder", raw))
        return out

    @staticmethod
    def _entry(ref: str, kind: str, raw: dict) -> dict:
        return {
            "project": ref,
            "name": str(raw.get("name", "")),
            "kind": kind,
            "description": raw.get("description"),
        }

    def resolve(self, ref: str) -> ProjectHandle:
        """The project the assistant named, if it may see it."""
        ref = (ref or "").strip()
        if not ref:
            raise NotFound("Name a project: lightsim_list_projects lists them.")
        policy = access_policy()
        if not policy.enabled:
            raise NotFound(ACCESS_OFF)
        try:
            if ref.startswith(EXAMPLE_PREFIX):
                if not (self.include_examples and policy.examples):
                    raise NotFound(ref)
                ex_id = ref[len(EXAMPLE_PREFIX):]
                # the app opens an example as a copy with an id of its own,
                # so it keeps no runs under the example's id
                handle = ProjectHandle(ref, "example", storage.example_path(ex_id), None,
                                       "example/" + storage.safe_id(ex_id, "example"))
            elif storage.SAFE_ID.fullmatch(ref):
                path = storage.project_path(ref)
                if not (self._user_projects_visible(policy) and self._may_open(path, policy)):
                    raise NotFound(ref)
                handle = ProjectHandle(ref, "user", path, ref, "project/" + ref)
            else:
                path = Path(ref).expanduser().resolve()
                if not self._may_open(path, policy):
                    raise NotFound(ref)
                user = self._user_project_at(path)
                if user is not None and self._user_projects_visible(policy):
                    return self.resolve(user)  # a saved project, named by its path
                handle = ProjectHandle(str(path), "folder", path, _app_id_of(path),
                                       "file/" + _file_key(path))
        except ValueError:
            raise NotFound(ref) from None
        if not handle.path.is_file() or _hidden_from_ai(handle.path):
            raise NotFound(
                f"No project '{ref}' that the assistant may see: lightsim_list_projects lists them."
            )
        return handle

    def _user_project_at(self, path: Path) -> Optional[str]:
        """The id of the saved project whose file is ``path``, if it is one."""
        if path.parent != self.user_folder.expanduser().resolve() or path.suffix != ".json":
            return None
        pid = path.stem
        if not storage.SAFE_ID.fullmatch(pid):
            return None
        try:
            return pid if storage.project_path(pid).resolve() == path else None
        except (OSError, ValueError):
            return None

    def load(self, ref: str) -> tuple[ProjectHandle, Project, Optional[str]]:
        """The project and the revision of its file (None for an example
        or a new project)."""
        if (ref or "").strip().startswith(NEW_PREFIX):
            name = ref.strip()[len(NEW_PREFIX):].strip()[:80] or "New Project"
            handle = ProjectHandle(ref.strip(), "new", Path(), None, "new/" + _slug(name))
            return handle, blank_project(name), None
        handle = self.resolve(ref)
        data = handle.path.read_bytes()
        try:
            project = Project.model_validate_json(data)
        except ValueError as e:
            raise NotFound(f"Project '{ref}' could not be read: {e}") from None
        return handle, project, None if handle.read_only else storage.revision_of(data)

    def save(self, handle: ProjectHandle, project: Project, expected: Optional[str]) -> tuple[str, str]:
        """Save an edited project; returns (its reference, new revision).

        An example is never written: the edit is saved as a new project of
        the user's, as the app's Save does with an example's copy. A file in
        an allowed folder is replaced the same safe way the app saves, and
        the version it replaces is kept in ``<file>-backups`` beside it (the
        newest :data:`storage.KEEP_BACKUPS`), as for a .lightsim file the
        app saves.
        """
        if handle.kind == "example":
            project = project.model_copy(update={"id": _free_id(project.id + "-ai"),
                                                 "name": f"{project.name} (AI edit)"})
            return project.id, storage.save_project(project, None, create_only=True)
        if handle.kind == "new":
            project = project.model_copy(update={"id": _free_id(project.name.lower())})
            return project.id, storage.save_project(project, None, create_only=True)
        if handle.kind == "user":
            if project.id != handle.project_id:
                project = project.model_copy(update={"id": handle.project_id})
            return handle.ref, storage.save_project(project, expected)
        data = json.dumps(project.model_dump(mode="json"), indent=2).encode("utf-8")
        with storage._save_lock:  # the check, the backup and the write in one go
            current = handle.path.read_bytes()
            if expected and storage.revision_of(current) != expected:
                raise storage.ConflictError("The file changed since it was read.")
            if current != data:
                storage._keep_backup(_file_location(handle.path), handle.path, current)
            storage._write_atomic(handle.path, data)
        return handle.ref, storage.revision_of(data)

    # -- library, checks and runs ----------------------------------------

    @staticmethod
    def library() -> dict[str, ComponentDef]:
        return library_by_id()

    @staticmethod
    def checks(project: Project) -> list[DataCheck]:
        return validate_project(project)

    @staticmethod
    def has_scripts(project: Project) -> bool:
        return any(e.componentDefId == SCRIPT_COMPONENT for s in project.systems for e in s.elements)

    @staticmethod
    def simulate(
        project: Project,
        case_id: str,
        *,
        max_seconds: float,
        cancel: threading.Event,
        progress: Optional[Callable[[float], None]] = None,
    ) -> SimResult:
        """Run a case as fast as the machine allows (a case's real-time
        pacing is for watching a live run, not for an assistant), stopping
        it after ``max_seconds`` of wall-clock time or when ``cancel`` is set.
        Data Checks run first, as the app's Run does. A paced case stays
        balanced or not as in the app, so its figures are the app's."""
        from ..solver import simulate
        from ..solver.balance import unpaced

        errors = run_blockers(validate_project(project), case_id)
        if errors:
            return SimResult(
                caseId=case_id, status="failed", channels=[],
                messages=[{"level": "error", "text": f"Data check failed: {c.text}"} for c in errors],
            )
        fast = project.model_copy(update={"cases": [unpaced(c) for c in project.cases]})
        deadline = time.monotonic() + max_seconds

        def control() -> list[dict]:
            if cancel.is_set() or time.monotonic() > deadline:
                return [{"type": "cancel"}]
            return []

        def emit(event: dict) -> None:
            if progress and event.get("type") == "step" and isinstance(event.get("pct"), (int, float)):
                progress(float(event["pct"]))

        return simulate(fast, case_id, emit, control)

    @staticmethod
    def stored_runs(handle: ProjectHandle) -> list[dict]:
        """The app's stored runs of the project, newest first."""
        if handle.project_id is None:
            return []
        try:
            return run_store.list_runs(handle.project_id)
        except ValueError:
            return []

    @staticmethod
    def stored_run(handle: ProjectHandle, run_id: str) -> StoredRun:
        if handle.project_id is None:
            raise NotFound(f"No run '{run_id}' of this project.")
        try:
            data = run_store.run_path(handle.project_id, run_id).read_bytes()
        except (FileNotFoundError, ValueError):
            raise NotFound(f"No run '{run_id}' of this project.") from None
        return StoredRun.model_validate_json(gzip.decompress(data))


def blank_project(name: str) -> Project:
    """An empty project as the app's Start > Blank project makes it."""
    return Project.model_validate({
        "id": "new", "name": name,
        "systems": [{"id": "sys-root", "name": name, "parentId": None, "elements": [], "connections": []}],
        "dataBusConnections": [],
        "cases": [{"id": "case-1", "name": "Case 1", "duration": 600, "timeStep": 1}],
    })


def _slug(text: str) -> str:
    return re.sub(r"[^A-Za-z0-9_-]+", "-", text).strip("-")[:100] or "project"


def _file_key(path: Path) -> str:
    """A project file's own name for its run history: its name and a hash of
    its full path (A/car.json and B/car.json are two projects)."""
    digest = hashlib.sha256(os.path.normcase(str(path)).encode("utf-8")).hexdigest()[:12]
    return f"{_slug(path.stem)[:60]}-{digest}"


def _file_location(path: Path) -> storage.Location:
    """Where the app would keep a project file's runs and backups had the
    user opened it as a .lightsim file: beside it (app.files)."""
    return storage.Location(
        id=_file_key(path), file=path,
        runs=path.with_name(path.name + "-runs"),
        backups=path.with_name(path.name + "-backups"),
        resources=path.with_name(path.name + "-resources"),
        external=True,
    )


def _app_id_of(path: Path) -> Optional[str]:
    """The id the app knows a project file by (and keeps its runs under),
    if the user opened or saved it in the app."""
    try:
        return next((f.id for f in files.recent() if f.path == path), None)
    except OSError:
        return None


def _free_id(base: str) -> str:
    base = _slug(base)
    candidate, n = base, 2
    while storage.project_path(candidate).exists():
        candidate, n = f"{base}-{n}", n + 1
    return candidate
