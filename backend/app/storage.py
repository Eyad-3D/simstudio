"""Project persistence — one JSON file per project (spec §1: single-file projects).

The user's projects live in :func:`app.paths.projects_dir`, a per-user
app-data folder in the packaged desktop app and ``backend/dev-projects``
during development. The examples are read straight from the app's own
read-only copy (:data:`app.paths.EXAMPLES_DIR`) and never written: the UI
opens one as an unsaved copy with an id of its own, so saving it makes a new
project, and each update of the app brings its new and corrected examples to
every install. Examples the user does not want in the Open menu are hidden,
not deleted; ``.hidden-examples`` in the projects folder lists them.

Saves are all-or-nothing: the new file is written next to the old one and
swapped in with an atomic rename, and the version it replaces is kept as
``<id>.json.bak``. That version is also added to the project's backups in
``.backups/<id>/``, which keep the last :data:`KEEP_BACKUPS` versions for
the UI to restore (as a copy: a restore never writes the project file).
Each file version has a *revision* (a hash of its bytes);
a save can name the revision it was based on and is refused with
:class:`ConflictError` if the file changed since, so two windows (or another
program) never silently overwrite each other.
"""
from __future__ import annotations

import contextlib
import hashlib
import json
import os
import re
import secrets
import threading
import time
from pathlib import Path

from .paths import EXAMPLES_DIR, projects_dir
from .schemas import Project

# one save at a time, so the revision check and the write cannot interleave
_save_lock = threading.Lock()
# hiding and restoring examples read and rewrite one file
_examples_lock = threading.Lock()


class ConflictError(Exception):
    """The file on disk is not the version the save was based on."""


#: A project or run id: it names a file or folder, so it is a plain name of
#: at most 128 characters that does not start or end with a dot (that rules
#: out "." and "..", and names Windows would shorten).
SAFE_ID = re.compile(r"[A-Za-z0-9_-](?:[A-Za-z0-9._-]{0,126}[A-Za-z0-9_-])?")


#: Earlier versions kept per project, and the name of their folder (hidden,
#: and outside the project list, which only reads ``*.json`` at the top).
KEEP_BACKUPS = 20
_BACKUPS = ".backups"
#: A backup's id: when it was taken (epoch ms) and the revision it holds.
BACKUP_ID = re.compile(r"(\d{13})-([0-9a-f]{16})")


def safe_id(value: str, what: str) -> str:
    if not isinstance(value, str) or not SAFE_ID.fullmatch(value):
        raise ValueError(f"Invalid {what} id: {value!r}")
    return value


def _safe_name(project_id: str) -> str:
    return safe_id(project_id, "project")


def user_dir() -> Path:
    """The folder the user's projects (and their runs and backups) are kept
    in. Reading creates nothing in it; the first save creates the folder."""
    return projects_dir()


def project_path(project_id: str) -> Path:
    return user_dir() / f"{_safe_name(project_id)}.json"


def list_projects() -> list[dict]:
    """The user's projects (the examples are listed by :func:`list_examples`)."""
    return _listing(user_dir())


def _listing(folder: Path) -> list[dict]:
    """Id, name and description of each project file in `folder`."""
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
            })
        except (json.JSONDecodeError, OSError, AttributeError):
            continue
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
    """An example as the app ships it."""
    path = example_path(example_id)
    if not path.is_file():
        raise FileNotFoundError(example_id)
    return Project.model_validate_json(path.read_bytes())


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
    _write_atomic(folder / _HIDDEN, data.encode("utf-8"))


def revision_of(data: bytes) -> str:
    """Identifies one version of a project file (changes whenever its bytes do)."""
    return hashlib.sha256(data).hexdigest()[:16]


def load_project_file(project_id: str) -> tuple[Project, str]:
    """The project and the revision of the file it was read from."""
    path = project_path(project_id)
    if not path.exists():
        raise FileNotFoundError(project_id)
    data = path.read_bytes()
    return Project.model_validate_json(data), revision_of(data)


def load_project(project_id: str) -> Project:
    return load_project_file(project_id)[0]


def _replace(src: str, dst: Path) -> None:
    # Windows refuses the rename while another process (a virus scanner, a
    # sync client) briefly holds the target open; give it a moment.
    for attempt in range(5):
        try:
            os.replace(src, dst)
            return
        except PermissionError:
            if attempt == 4:
                raise
            time.sleep(0.05 * (attempt + 1))


def _write_atomic(path: Path, data: bytes) -> None:
    """Write `path` so that it holds either its old or its new bytes, never a
    mix: write a temp file beside it, flush it to disk, then rename it over."""
    tmp = path.with_name(f".{path.name}.{secrets.token_hex(4)}.tmp")
    try:
        with open(tmp, "xb") as f:
            f.write(data)
            f.flush()
            os.fsync(f.fileno())
        _replace(str(tmp), path)
    except BaseException:
        with contextlib.suppress(OSError):
            tmp.unlink()
        raise


def save_project(
    project: Project, expected_revision: str | None = None, create_only: bool = False
) -> str:
    """Save atomically and return the new revision.

    `expected_revision` is the revision the caller's copy was loaded from
    ("*" = any, the file must just exist); `create_only` refuses to replace an
    existing file. Either raises ConflictError when the file on disk does not
    match. With neither, the save overwrites unconditionally.
    """
    path = project_path(project.id)
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
            _write_atomic(path.with_name(path.name + ".bak"), current)
            if current != data:
                _keep_backup(project.id, path, current)
        _write_atomic(path, data)
    return revision_of(data)


def _backups_dir(project_id: str) -> Path:
    return user_dir() / _BACKUPS / _safe_name(project_id)


def _backup_files(folder: Path) -> list[Path]:
    """The backups in `folder`, oldest first."""
    return sorted(f for f in folder.glob("*.json") if BACKUP_ID.fullmatch(f.stem))


def _keep_backup(project_id: str, path: Path, previous: bytes) -> None:
    """Add `previous`, the version at `path` a save is about to replace, to
    the project's backups and drop all but the newest KEEP_BACKUPS. The copy
    keeps the file's time, so it is listed by when that version was saved.
    Call with _save_lock held."""
    folder = _backups_dir(project_id)
    folder.mkdir(parents=True, exist_ok=True)
    kept = _backup_files(folder)
    revision = revision_of(previous)
    if kept and kept[-1].stem.endswith(revision):
        return  # the newest backup holds these bytes already
    saved_ns = path.stat().st_mtime_ns
    backup = folder / f"{time.time_ns() // 1_000_000:013d}-{revision}.json"
    _write_atomic(backup, previous)
    os.utime(backup, ns=(saved_ns, saved_ns))
    for old in kept[: max(0, len(kept) + 1 - KEEP_BACKUPS)]:
        with contextlib.suppress(OSError):
            old.unlink()


def list_backups(project_id: str) -> list[dict]:
    """The project's backups, newest first: each one's id, when that version
    was saved (epoch ms), its revision and size, and the project name and
    element count it holds (None where the file cannot be read)."""
    folder = _backups_dir(project_id)
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
    """One earlier version of the project, as it was saved."""
    if not isinstance(backup_id, str) or not BACKUP_ID.fullmatch(backup_id):
        raise ValueError(f"Invalid backup id: {backup_id!r}")
    path = _backups_dir(project_id) / f"{backup_id}.json"
    if not path.is_file():
        raise FileNotFoundError(backup_id)
    return Project.model_validate_json(path.read_bytes())


def delete_project(project_id: str) -> bool:
    path = project_path(project_id)
    if path.exists():
        path.unlink()
        return True
    return False
