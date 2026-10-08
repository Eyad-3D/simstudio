"""Projects kept as .lightsim files anywhere on disk (PLT-33).

A project need not live in the app's projects folder: the user can open a
``<name>.lightsim`` file from any folder (a git repository, a course folder,
a team share) and save one anywhere. The file is the same JSON as a project
in the projects folder. Its runs, backups and attached files are kept beside
it::

    car.lightsim              the model (commit this)
    car.lightsim-resources/   attached files: FMUs, AI models, data (commit this)
    car.lightsim-runs/        run history and study tables (ignored by git)
    car.lightsim-backups/     earlier versions (ignored by git)

The runs and backups folders each hold a ``.gitignore`` that ignores the
folder, so git leaves them out without a change to the repository's own
``.gitignore``.

The engine never takes a path from the page. Only the desktop shell, which
shows the system's Open and Save dialogs (or receives a file the user
double-clicked or dropped on the window), names a path: :func:`open_file`
and :func:`save_as` record it under a project id in ``.open-files.json`` in
the projects folder, and from then on the page refers to the file by that id
only, through the same /api/projects/{id} routes as any project. The list is
also the app's *Recent files*; taking a file off Recent files only hides its
entry, so a project open from it still saves to it.

A Save As is recorded as *pending* until the project's first save to the new
file succeeds (:func:`confirm`); a save that fails drops it (:func:`cancel`),
so Recent files never lists a file that was not written and a project's
runs only move once its file exists.

The project id is kept inside the file. A file whose id is already taken
(by a project in the projects folder, or by another file, for example a
copy of it) gets an id of its own when it is opened; the file is not
changed until it is saved.
"""
from __future__ import annotations

import json
import os
import re
import secrets
import threading
import time
from dataclasses import dataclass
from pathlib import Path

from .fileio import SAFE_ID, write_atomic
from .paths import projects_dir

#: The extension of a project file outside the projects folder.
SUFFIX = ".lightsim"
#: Files listed (and remembered) at most; the oldest are forgotten first.
KEEP_RECENT = 50
#: A Save As whose first save has not come within this long is forgotten, ms.
PENDING_MS = 10 * 60 * 1000
_REGISTRY = ".open-files.json"
_lock = threading.Lock()


@dataclass(frozen=True)
class OpenFile:
    id: str
    path: Path
    openedAt: int  # epoch ms
    #: on Recent files; False once the user took it off the list (the id
    #: still stands for the file, for a window that has it open)
    listed: bool = True
    #: a Save As waiting for its first save (:func:`confirm`)
    pending: bool = False
    #: the runs folder of the never-saved project a pending Save As is for,
    #: moved beside the file when it is first written
    runs_from: Path | None = None


def _registry_path() -> Path:
    return projects_dir() / _REGISTRY


def _read() -> list[OpenFile]:
    """The remembered files, most recently opened first; call with _lock held."""
    try:
        raw = json.loads(_registry_path().read_bytes())
    except (OSError, ValueError):
        return []
    out = []
    now = time.time_ns() // 1_000_000
    for e in raw.get("files", []) if isinstance(raw, dict) else []:
        try:
            pid, path, at = e["id"], e["path"], e.get("openedAt", 0)
            listed, pending = e.get("listed", True), e.get("pending", False)
            runs_from = e.get("runsFrom")
            if not (isinstance(pid, str) and SAFE_ID.fullmatch(pid) and isinstance(path, str)
                    and os.path.isabs(path) and isinstance(at, int)
                    and isinstance(listed, bool) and isinstance(pending, bool)):
                continue
            if pending and not 0 <= now - at <= PENDING_MS:
                continue  # a Save As that never got its save
            if not (isinstance(runs_from, str) and os.path.isabs(runs_from)):
                runs_from = None
            out.append(OpenFile(pid, Path(path), at, listed, pending,
                                Path(runs_from) if runs_from else None))
        except (KeyError, TypeError, AttributeError):
            continue
    return out


def _as_json(e: OpenFile) -> dict:
    out: dict = {"id": e.id, "path": str(e.path), "openedAt": e.openedAt}
    if not e.listed:
        out["listed"] = False
    if e.pending:
        out["pending"] = True
    if e.runs_from is not None:
        out["runsFrom"] = str(e.runs_from)
    return out


def _write(entries: list[OpenFile]) -> None:
    folder = projects_dir()
    folder.mkdir(parents=True, exist_ok=True)
    data = {"files": [_as_json(e) for e in entries[:KEEP_RECENT]]}
    write_atomic(folder / _REGISTRY, (json.dumps(data, indent=2) + "\n").encode("utf-8"))


def lookup(project_id: str) -> Path | None:
    """The file a project id stands for (on Recent files or not, or chosen
    by a pending Save As), or None for a project in the projects folder (or
    no project at all)."""
    with _lock:
        entries = sorted(_read(), key=lambda e: not e.pending)
        return next((e.path for e in entries if e.id == project_id), None)


def recent() -> list[OpenFile]:
    """The files on Recent files, most recently opened first."""
    with _lock:
        return [e for e in _read() if e.listed and not e.pending]


def checked_path(path: str, *, must_exist: bool) -> Path:
    """`path` made absolute and checked: a .lightsim file (or, when saving,
    one whose folder exists). Raises ValueError with a reason to show."""
    if not isinstance(path, str) or not path or "\x00" in path or len(path) > 4096:
        raise ValueError("Not a file path.")
    p = Path(path)
    if not p.is_absolute():
        raise ValueError("The file path must be absolute.")
    p = Path(os.path.realpath(p))
    if p.suffix.lower() != SUFFIX:
        raise ValueError(f"A LightSim project file ends in {SUFFIX}: {p.name}")
    if must_exist and not p.is_file():
        raise FileNotFoundError(str(p))
    if not must_exist and not p.parent.is_dir():
        raise FileNotFoundError(str(p.parent))
    if p.is_dir():
        raise ValueError(f"{p} is a folder.")
    return p


def _id_taken(project_id: str, path: Path, entries: list[OpenFile]) -> bool:
    """Another project already goes by this id: one in the projects folder,
    or another file that is still there."""
    if (projects_dir() / f"{project_id}.json").exists():
        return True
    return any(e.id == project_id and e.path != path and e.path.exists() for e in entries)


def fresh_id(base: str) -> str:
    """A new project id based on `base` ("my project" -> "my-project-1a2b3c")."""
    if SAFE_ID.fullmatch(base or ""):
        stem = base[:100].rstrip("._-")
    else:  # keep what a file name can carry
        stem = re.sub(r"[^A-Za-z0-9_-]+", "-", base if isinstance(base, str) else "")
        stem = stem[:100].strip("_-")
    return f"{stem or 'project'}-{secrets.token_hex(3)}"


def _remember(entries: list[OpenFile], project_id: str, path: Path) -> OpenFile:
    """Put (project_id, path) first in the list, dropping any other entry for
    that path or that id; call with _lock held."""
    entry = OpenFile(project_id, path, time.time_ns() // 1_000_000)
    rest = [e for e in entries if e.path != path and e.id != project_id]
    _write([entry, *rest])
    return entry


def open_file(path: str, file_id: str | None) -> OpenFile:
    """Remember an existing .lightsim file the user picked, under the id it
    carries (`file_id`, read from it by the caller) or, when that id is taken
    or unusable, an id of its own. Opening the same file again gives the same
    id."""
    p = checked_path(path, must_exist=True)
    with _lock:
        entries = _read()
        known = next((e for e in entries if e.path == p and not e.pending), None)
        if known is not None:
            return _remember(entries, known.id, p)
        usable = isinstance(file_id, str) and SAFE_ID.fullmatch(file_id)
        pid = file_id if usable and not _id_taken(file_id, p, entries) else fresh_id(file_id or "")
        return _remember(entries, pid, p)


def save_as(path: str, project_id: str, runs_from: Path | None = None) -> OpenFile:
    """Note a .lightsim file the user chose to save `project_id` as, pending
    until the save that follows writes it (:func:`confirm`, :func:`cancel`).
    The project keeps its id unless another project goes by it, then it gets
    one of its own. `runs_from`: the runs of a project never saved, to move
    beside the file once it is written."""
    p = checked_path(path, must_exist=False)
    with _lock:
        # an earlier Save As to this file, or of this project, that never finished
        entries = [e for e in _read() if not (e.pending and (e.path == p or e.id == project_id))]
        settled = [e for e in entries if not e.pending]
        in_folder = (projects_dir() / f"{project_id}.json").exists()
        current = next((e for e in settled if e.id == project_id), None)
        taken = in_folder or (current is not None and current.path != p and current.path.exists())
        pid = fresh_id(project_id) if taken or not SAFE_ID.fullmatch(project_id) else project_id
        entry = OpenFile(pid, p, time.time_ns() // 1_000_000, listed=False, pending=True,
                         runs_from=runs_from)
        _write([entry, *(e for e in entries if not (e.pending and e.id == pid))])
        return entry


def confirm(project_id: str) -> OpenFile | None:
    """The save after a Save As wrote the file: put it on Recent files (in
    place of any other entry for that file or id). Returns the pending entry,
    or None when there was none."""
    with _lock:
        entries = _read()
        entry = next((e for e in entries if e.id == project_id and e.pending), None)
        if entry is None:
            return None
        _remember([e for e in entries if e is not entry], entry.id, entry.path)
        return entry


def cancel(project_id: str) -> OpenFile | None:
    """The save after a Save As failed: forget the file it was to write (the
    project is where it was before). Returns the dropped entry, if any."""
    with _lock:
        entries = _read()
        entry = next((e for e in entries if e.id == project_id and e.pending), None)
        if entry is not None:
            _write([e for e in entries if e is not entry])
        return entry


def forget(project_id: str) -> bool:
    """Take a file off Recent files (the file itself is left alone). Its id
    keeps standing for the file, so a window that has the project open still
    saves to it rather than to the projects folder; opening the file again
    puts it back on the list under the same id. The entry goes when
    :data:`KEEP_RECENT` newer files push it off the end."""
    with _lock:
        entries = _read()
        if not any(e.id == project_id and e.listed and not e.pending for e in entries):
            return False
        _write([OpenFile(e.id, e.path, e.openedAt, False)
                if e.id == project_id and not e.pending else e for e in entries])
        return True
