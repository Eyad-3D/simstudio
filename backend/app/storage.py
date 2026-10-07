"""Project persistence — one JSON file per project (spec §1: single-file projects).

The user's projects live in :func:`app.paths.projects_dir`, a per-user
app-data folder in the packaged desktop app and ``backend/dev-projects``
during development. A project can also be a ``.lightsim`` file anywhere on
disk that the user opened or saved through the desktop app (PLT-33,
:mod:`app.files`); :func:`location` says where each project's file, runs,
backups and attached files are. The examples are read straight from the
app's own read-only copy (:data:`app.paths.EXAMPLES_DIR`) and never written:
the UI opens one as an unsaved copy with an id of its own, so saving it
makes a new project, and each update of the app brings its new and
corrected examples to every install. Examples the user does not want in the
Open menu are hidden, not deleted; ``.hidden-examples`` in the projects
folder lists them.

Every file read is upgraded to the current format (:mod:`app.migrations`,
PLT-07); every save writes the current format and the app version. A file
from a newer LightSim opens read-only and cannot be saved over, so nothing
it holds that this build does not understand is lost. The first save over a
file in an older format also keeps it as ``pre-migration-v<n>`` in the
project's backups, for good.

Saves are all-or-nothing: the new file is written next to the old one and
swapped in with an atomic rename, and the version it replaces is kept as
``<id>.json.bak`` (projects folder only). That version is also added to the
project's backups (``.backups/<id>/``, or ``<name>.lightsim-backups/``),
which keep the last :data:`KEEP_BACKUPS` versions for the UI to restore (as
a copy: a restore never writes the project file). Each file version has a
*revision* (a hash of its bytes); a save can name the revision it was based
on and is refused with :class:`ConflictError` if the file changed since, so
two windows (or another program, or a git pull) never silently overwrite
each other.
"""
from __future__ import annotations

import contextlib
import hashlib
import json
import os
import re
import threading
import time
from dataclasses import dataclass, field
from pathlib import Path

from . import files
from .fileio import SAFE_ID, safe_id, write_atomic
from .migrations import CURRENT_VERSION, NewerFileError, file_version, migrate
from .paths import EXAMPLES_DIR, projects_dir
from .schemas import Project
from .version import VERSION

# one save at a time, so the revision check and the write cannot interleave
_save_lock = threading.Lock()
# hiding and restoring examples read and rewrite one file
_examples_lock = threading.Lock()

# kept under their old names for the modules that import them from here
_write_atomic = write_atomic
__all__ = ["SAFE_ID", "safe_id"]


class ConflictError(Exception):
    """The file on disk is not the version the save was based on."""


#: Earlier versions kept per project, and the name of their folder (hidden,
#: and outside the project list, which only reads ``*.json`` at the top).
KEEP_BACKUPS = 20
_BACKUPS = ".backups"
#: A backup's id: when it was taken (epoch ms) and the revision it holds.
BACKUP_ID = re.compile(r"(\d{13})-([0-9a-f]{16})")
#: Ignores the folder it is in (runs and backups beside a .lightsim file).
_GITIGNORE = "# Made by LightSim: run results and backups stay out of git.\n*\n"


def _safe_name(project_id: str) -> str:
    return safe_id(project_id, "project")


def user_dir() -> Path:
    """The folder the user's projects (and their runs and backups) are kept
    in. Reading creates nothing in it; the first save creates the folder."""
    return projects_dir()


@dataclass(frozen=True)
class Location:
    """Where one project's files are."""

    id: str
    #: the project file
    file: Path
    #: its run history and study tables (app.run_store)
    runs: Path
    #: earlier versions of the file
    backups: Path
    #: attached files (app.attachments)
    resources: Path
    #: a .lightsim file the user picked, outside the projects folder
    external: bool = False

    def make_dir(self, folder: Path) -> Path:
        """Create `folder` (its runs or backups folder). Beside a .lightsim
        file the folder gets a .gitignore that keeps it out of git."""
        folder.mkdir(parents=True, exist_ok=True)
        if self.external and folder in (self.runs, self.backups):
            ignore = folder / ".gitignore"
            if not ignore.exists():
                with contextlib.suppress(OSError):
                    ignore.write_text(_GITIGNORE, encoding="utf-8")
        return folder


def location(project_id: str) -> Location:
    """Where the project's file, runs, backups and attached files are: beside
    its .lightsim file if the user opened or saved it as one, else in the
    projects folder."""
    pid = _safe_name(project_id)
    path = files.lookup(pid)
    if path is not None:
        return Location(
            id=pid,
            file=path,
            runs=path.with_name(path.name + "-runs"),
            backups=path.with_name(path.name + "-backups"),
            resources=path.with_name(path.name + "-resources"),
            external=True,
        )
    folder = user_dir()
    return Location(
        id=pid,
        file=folder / f"{pid}.json",
        runs=folder / "runs" / pid,
        backups=folder / _BACKUPS / pid,
        resources=folder / "resources" / pid,
    )


def project_path(project_id: str) -> Path:
    return location(project_id).file


def list_projects() -> list[dict]:
    """The user's projects in the projects folder (the examples are listed by
    :func:`list_examples`, the .lightsim files by :func:`list_files`)."""
    return _listing(user_dir())


def _listing(folder: Path) -> list[dict]:
    """Id, name and description of each project file in `folder`, and for
    the Start page when it was saved, its number of parts and a sketch."""
    out = []
    for f in sorted(folder.glob("*.json")):
        try:
            raw = json.loads(f.read_text(encoding="utf-8"))
            project_id = raw.get("id", f.stem)
            if not isinstance(project_id, str) or not SAFE_ID.fullmatch(project_id):
                continue  # a hand-edited id the API could not serve safely
            out.append({
                "id": project_id,
                "name": raw.get("name", f.stem),
                "description": raw.get("description"),
                "modified": f.stat().st_mtime_ns // 1_000_000,
                **_size_and_sketch(raw),
            })
        except (json.JSONDecodeError, OSError, AttributeError):
            continue
    return out


def list_files() -> list[dict]:
    """The .lightsim files the user opened or saved (Recent files), most
    recently opened first: each one's id, path and, while the file can be
    read, what :func:`list_projects` gives for a project."""
    out = []
    for entry in files.recent():
        item: dict = {"id": entry.id, "path": str(entry.path), "opened": entry.openedAt,
                      "exists": False, "name": entry.path.stem, "description": None}
        try:
            raw = json.loads(entry.path.read_bytes())
            item.update({
                "exists": True,
                "modified": entry.path.stat().st_mtime_ns // 1_000_000,
                "name": raw.get("name") if isinstance(raw.get("name"), str) else entry.path.stem,
                "description": raw.get("description"),
                **_size_and_sketch(raw),
            })
        except (OSError, ValueError, AttributeError):
            pass
        out.append(item)
    return out


def _size_and_sketch(raw: dict) -> dict:
    """The number of parts in all systems, and the positions of the top
    system's parts (at most 300) for a thumbnail. What a malformed file
    does not give is left out rather than dropping it from the list."""
    out: dict = {"elements": None, "thumb": []}
    try:
        systems = raw.get("systems") or []
        out["elements"] = sum(len(s.get("elements") or []) for s in systems)
        top = next(s for s in systems if s.get("parentId") is None)
        out["thumb"] = [[round(e["position"]["x"]), round(e["position"]["y"])]
                        for e in (top.get("elements") or [])[:300]]
    except (AttributeError, KeyError, TypeError, ValueError, OverflowError, StopIteration):
        pass
    return out


#: Lists the examples the user hid from the Open menu (in the projects folder).
_HIDDEN = ".hidden-examples"
#: Earlier versions copied these examples (all they shipped) into a new
#: projects folder and wrote this marker, so that one the user deleted did
#: not come back.
_SEEDED_MARKER = ".seeded"
_SEEDED_EXAMPLES = ("bev-car", "hybrid-car")


def example_path(example_id: str) -> Path:
    return EXAMPLES_DIR / f"{safe_id(example_id, 'example')}.json"


def load_example(example_id: str) -> Project:
    """An example as the app ships it, in the current format."""
    path = example_path(example_id)
    if not path.is_file():
        raise FileNotFoundError(example_id)
    return read_project(path.read_bytes()).project


def list_examples() -> list[dict]:
    """The examples shipped with the app, each with `hidden`: the user hid it."""
    with _examples_lock:
        hidden = _hidden_examples()
    return [{**e, "hidden": e["id"] in hidden} for e in _listing(EXAMPLES_DIR)]


def hide_example(example_id: str) -> None:
    """Leave an example out of the Open menu until the examples are restored."""
    if not example_path(example_id).is_file():
        raise FileNotFoundError(example_id)
    with _examples_lock:
        hidden = _hidden_examples()
        if example_id not in hidden:
            _write_hidden(hidden | {example_id})


def restore_examples() -> list[str]:
    """Show every hidden example again; returns the ids of those shipped."""
    with _examples_lock:
        hidden = _hidden_examples()
        if hidden:
            _write_hidden(set())
    return sorted(i for i in hidden if example_path(i).is_file())


def _hidden_examples() -> set[str]:
    """Ids of the examples the user hid; call with _examples_lock held.

    A projects folder that earlier versions seeded still holds the copies of
    the examples it got, which stay the user's own projects. An example whose
    copy is gone was deleted by the user, who did not want it back, so it
    starts out hidden. That is decided once and written down, so restoring
    the examples brings it back for good.
    """
    folder = user_dir()
    try:
        raw = json.loads((folder / _HIDDEN).read_bytes())
    except FileNotFoundError:
        raw = None
    except (OSError, ValueError):
        return set()  # unreadable: hide nothing rather than fail the list
    if raw is not None:
        ids = raw.get("hidden") if isinstance(raw, dict) else None
        if not isinstance(ids, list):
            return set()
        return {i for i in ids if isinstance(i, str) and SAFE_ID.fullmatch(i)}
    if not (folder / _SEEDED_MARKER).is_file():
        return set()
    hidden = {e for e in _SEEDED_EXAMPLES if not (folder / f"{e}.json").exists()}
    with contextlib.suppress(OSError):
        _write_hidden(hidden)
    return hidden


def _write_hidden(hidden: set[str]) -> None:
    folder = user_dir()
    folder.mkdir(parents=True, exist_ok=True)
    data = json.dumps({"hidden": sorted(hidden)}, indent=2) + "\n"
    write_atomic(folder / _HIDDEN, data.encode("utf-8"))


def revision_of(data: bytes) -> str:
    """Identifies one version of a project file (changes whenever its bytes do)."""
    return hashlib.sha256(data).hexdigest()[:16]


@dataclass
class ReadProject:
    """A project file read and upgraded to the current format."""

    project: Project
    #: the format version the file was in (CURRENT_VERSION: nothing to upgrade)
    from_version: int = CURRENT_VERSION
    #: studies an upgrade took out of the file, for the run store (PLT-34)
    studies: list[dict] = field(default_factory=list)
    #: why the project cannot be saved (a file from a newer LightSim), or None
    read_only: str | None = None

    @property
    def upgraded_from(self) -> int | None:
        """The older format the file was upgraded from, or None."""
        return self.from_version if self.from_version < CURRENT_VERSION else None


def read_project(data: bytes | str | dict) -> ReadProject:
    """Parse a project file's bytes (or its parsed JSON), upgrading it to the
    current format. A file from a newer LightSim is read as it is, marked
    read-only; one this build cannot make sense of raises NewerFileError.
    Raises ValueError (pydantic's ValidationError is one) for anything that
    is not a project."""
    raw = data if isinstance(data, dict) else json.loads(data)
    try:
        migrated = migrate(raw)
    except NewerFileError as newer:
        try:
            project = Project.model_validate(raw)
        except ValueError:
            raise newer from None
        return ReadProject(project, newer.version, read_only=str(newer))
    project = Project.model_validate(migrated.data)
    return ReadProject(project, migrated.from_version, migrated.studies)


@dataclass
class LoadedProject(ReadProject):
    revision: str = ""
    location: Location | None = None


def load_project_file(project_id: str) -> LoadedProject:
    """The project (upgraded to the current format) and the revision of the
    file it was read from."""
    loc = location(project_id)
    if not loc.file.exists():
        raise FileNotFoundError(project_id)
    data = loc.file.read_bytes()
    read = read_project(data)
    if loc.external and read.project.id != loc.id:
        # a copy of a file that is open under its own id: this one goes by
        # the id it was given when opened (written into it on the next save)
        read.project.id = loc.id
    return LoadedProject(read.project, read.from_version, read.studies, read.read_only,
                         revision=revision_of(data), location=loc)


def load_project(project_id: str) -> Project:
    return load_project_file(project_id).project


def current_revision(project_id: str) -> str | None:
    """The revision of the project's file as it is on disk now (None: no file).
    The UI asks every few seconds to notice a change made outside the app
    (a git pull, a second window)."""
    try:
        return revision_of(location(project_id).file.read_bytes())
    except FileNotFoundError:
        return None


def _format_of(data: bytes) -> tuple[int, str | None]:
    """The format version and app version a file's bytes say (1, None when
    it says neither or cannot be read)."""
    try:
        raw = json.loads(data)
        saved_with = raw.get("savedWith")
        return file_version(raw), saved_with if isinstance(saved_with, str) else None
    except (ValueError, AttributeError):
        return 1, None


def save_project(
    project: Project, expected_revision: str | None = None, create_only: bool = False
) -> str:
    """Save atomically and return the new revision.

    `expected_revision` is the revision the caller's copy was loaded from
    ("*" = any, the file must just exist); `create_only` refuses to replace an
    existing file. Either raises ConflictError when the file on disk does not
    match. With neither, the save overwrites unconditionally. A file on disk
    from a newer LightSim is never replaced (NewerFileError).
    """
    loc = location(project.id)
    path = loc.file
    project.schemaVersion = CURRENT_VERSION
    project.savedWith = VERSION
    data = project.model_dump_json(indent=2).encode("utf-8")
    with _save_lock:
        path.parent.mkdir(parents=True, exist_ok=True)
        current = path.read_bytes() if path.exists() else None
        if create_only and current is not None:
            raise ConflictError(f"A project with id '{project.id}' already exists on disk.")
        if expected_revision is not None:
            if current is None:
                raise ConflictError(
                    f"Project '{project.id}' was deleted on disk after it was loaded.")
            if expected_revision != "*" and revision_of(current) != expected_revision:
                raise ConflictError(
                    f"Project '{project.id}' changed on disk after it was loaded "
                    f"(another window or program saved it).")
        if current is not None:
            version, saved_with = _format_of(current)
            if version > CURRENT_VERSION:
                raise NewerFileError(version, saved_with)
            if version < CURRENT_VERSION:
                _keep_pre_migration(loc, version, current)
            if not loc.external:
                write_atomic(path.with_name(path.name + ".bak"), current)
            if current != data:
                _keep_backup(loc, path, current)
        write_atomic(path, data)
    return revision_of(data)


def _keep_pre_migration(loc: Location, version: int, previous: bytes) -> None:
    """Keep the file as it was before its first save in the current format,
    for good (it is not one of the KEEP_BACKUPS that rotate out)."""
    keep = loc.make_dir(loc.backups) / f"pre-migration-v{version}.json"
    if not keep.exists():
        write_atomic(keep, previous)


def _backup_files(folder: Path) -> list[Path]:
    """The backups in `folder`, oldest first."""
    return sorted(f for f in folder.glob("*.json") if BACKUP_ID.fullmatch(f.stem))


def _keep_backup(loc: Location, path: Path, previous: bytes) -> None:
    """Add `previous`, the version at `path` a save is about to replace, to
    the project's backups and drop all but the newest KEEP_BACKUPS. The copy
    keeps the file's time, so it is listed by when that version was saved.
    Call with _save_lock held."""
    folder = loc.make_dir(loc.backups)
    kept = _backup_files(folder)
    revision = revision_of(previous)
    if kept and kept[-1].stem.endswith(revision):
        return  # the newest backup holds these bytes already
    saved_ns = path.stat().st_mtime_ns
    backup = folder / f"{time.time_ns() // 1_000_000:013d}-{revision}.json"
    write_atomic(backup, previous)
    os.utime(backup, ns=(saved_ns, saved_ns))
    for old in kept[: max(0, len(kept) + 1 - KEEP_BACKUPS)]:
        with contextlib.suppress(OSError):
            old.unlink()


def list_backups(project_id: str) -> list[dict]:
    """The project's backups, newest first: each one's id, when that version
    was saved (epoch ms), its revision and size, and the project name and
    element count it holds (None where the file cannot be read)."""
    folder = location(project_id).backups
    out = []
    for f in reversed(_backup_files(folder)) if folder.is_dir() else []:
        st = f.stat()
        entry = {
            "id": f.stem,
            "savedAt": st.st_mtime_ns // 1_000_000,
            "revision": BACKUP_ID.fullmatch(f.stem).group(2),
            "bytes": st.st_size,
            "name": None,
            "elements": None,
        }
        try:
            raw = json.loads(f.read_bytes())
            entry["name"] = raw["name"] if isinstance(raw.get("name"), str) else None
            entry["elements"] = sum(len(s["elements"]) for s in raw["systems"])
        except (OSError, ValueError, KeyError, TypeError, AttributeError):
            pass
        out.append(entry)
    return out


def load_backup(project_id: str, backup_id: str) -> Project:
    """One earlier version of the project, as it was saved (upgraded to the
    current format; its studies, if an old version had any, are left out:
    they are kept with the project's runs)."""
    if not isinstance(backup_id, str) or not BACKUP_ID.fullmatch(backup_id):
        raise ValueError(f"Invalid backup id: {backup_id!r}")
    path = location(project_id).backups / f"{backup_id}.json"
    if not path.is_file():
        raise FileNotFoundError(backup_id)
    return read_project(path.read_bytes()).project


def delete_project(project_id: str) -> bool:
    """Delete a project in the projects folder. A .lightsim file elsewhere is
    the user's own: the app takes it off Recent files instead
    (:func:`app.files.forget`) and never deletes it."""
    loc = location(project_id)
    if loc.external:
        raise ValueError(
            f"'{loc.file.name}' is a file of yours outside the projects folder; "
            "LightSim does not delete it (remove it from Recent files instead).")
    if loc.file.exists():
        loc.file.unlink()
        return True
    return False
