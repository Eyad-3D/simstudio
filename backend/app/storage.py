"""Project persistence — one JSON file per project (spec §1: single-file projects).

Projects live in :func:`app.paths.projects_dir`, which is the repo's
``backend/projects`` during development and a per-user app-data folder in the
packaged desktop app. On first use of a fresh location the bundled example
projects are copied in, so a new install never opens empty.

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
import shutil
import threading
import time
from pathlib import Path

from .paths import SEED_PROJECTS_DIR, projects_dir
from .schemas import Project

_seeded: set[Path] = set()
# one save at a time, so the revision check and the write cannot interleave
_save_lock = threading.Lock()


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


def _ensure_dir() -> Path:
    """Create the projects dir, copying in the examples on first use.

    A marker file records that seeding happened, so a user who deletes an
    example project does not find it back on the next launch.
    """
    target = projects_dir().resolve()
    target.mkdir(parents=True, exist_ok=True)
    if target in _seeded:
        return target
    _seeded.add(target)

    marker = target / ".seeded"
    seed = SEED_PROJECTS_DIR.resolve()
    if marker.exists() or seed == target or not seed.is_dir():
        return target
    for src in seed.glob("*.json"):
        dest = target / src.name
        if not dest.exists():
            shutil.copyfile(src, dest)
    marker.write_text(
        "SimStudio copied its example projects here on first run.\n", encoding="utf-8"
    )
    return target


def project_path(project_id: str) -> Path:
    return _ensure_dir() / f"{_safe_name(project_id)}.json"


def list_projects() -> list[dict]:
    out = []
    for f in sorted(_ensure_dir().glob("*.json")):
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
    return _ensure_dir() / _BACKUPS / _safe_name(project_id)


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
