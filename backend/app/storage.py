"""Project persistence — one JSON file per project (spec §1: single-file projects).

Projects live in :func:`app.paths.projects_dir`, which is the repo's
``backend/projects`` during development and a per-user app-data folder in the
packaged desktop app. On first use of a fresh location the bundled example
projects are copied in, so a new install never opens empty.
"""
from __future__ import annotations

import json
import re
import shutil
from pathlib import Path

from .paths import SEED_PROJECTS_DIR, projects_dir
from .schemas import Project

_seeded: set[Path] = set()


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


def load_project(project_id: str) -> Project:
    path = project_path(project_id)
    if not path.exists():
        raise FileNotFoundError(project_id)
    return Project.model_validate_json(path.read_text(encoding="utf-8"))


def save_project(project: Project) -> None:
    path = project_path(project.id)
    path.write_text(project.model_dump_json(indent=2), encoding="utf-8")


def delete_project(project_id: str) -> bool:
    path = project_path(project_id)
    if path.exists():
        path.unlink()
        return True
    return False
