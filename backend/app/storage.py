"""Project persistence — one JSON file per project (spec §1: single-file projects).

Projects live in :func:`app.paths.projects_dir`, which is the repo's
``backend/projects`` during development and a per-user app-data folder in the
packaged desktop app. On first use of a fresh location the bundled example
projects are copied in, so a new install never opens empty.

Saves are all-or-nothing: the new file is written next to the old one and
swapped in with an atomic rename, and the version it replaces is kept as
``<id>.json.bak``. Each file version has a *revision* (a hash of its bytes);
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


def _safe_name(project_id: str) -> str:
    if not re.fullmatch(r"[A-Za-z0-9._-]+", project_id):
        raise ValueError(f"Invalid project id: {project_id!r}")
    return project_id


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
            out.append({
                "id": raw.get("id", f.stem),
                "name": raw.get("name", f.stem),
                "description": raw.get("description"),
            })
        except (json.JSONDecodeError, OSError):
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
        _write_atomic(path, data)
    return revision_of(data)


def delete_project(project_id: str) -> bool:
    path = project_path(project_id)
    if path.exists():
        path.unlink()
        return True
    return False
