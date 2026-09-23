"""Filesystem locations, resolved at runtime.

In development everything lives inside the repo. In the packaged desktop app
the code is read-only (frozen inside the executable) while saved projects must
land in the user's own app-data folder, so both are indirected through here.
"""
from __future__ import annotations

import os
import sys
from pathlib import Path

#: Directory the bundled read-only resources live in. Under PyInstaller this is
#: the temporary extraction dir; in a source checkout it is ``backend/``.
BUNDLE_DIR = Path(getattr(sys, "_MEIPASS", Path(__file__).parent.parent))

#: Example projects shipped with the app. The engine serves them read-only
#: (app/storage.py) and never writes to this folder: in a source checkout it
#: holds the repo's examples, which the golden tests also read.
EXAMPLES_DIR = BUNDLE_DIR / "projects"

#: Where projects are saved during development (ignored by git): beside the
#: examples, never among them.
DEV_PROJECTS_DIR = BUNDLE_DIR / "dev-projects"


def projects_dir() -> Path:
    """Where user projects are read from and written to.

    The desktop shell sets ``LIGHTSIM_PROJECTS_DIR`` to a per-user app-data
    path. Without it (development) we fall back to ``backend/dev-projects``.
    """
    override = os.environ.get("LIGHTSIM_PROJECTS_DIR")
    return Path(override).expanduser() if override else DEV_PROJECTS_DIR


def static_dir() -> Path | None:
    """Built frontend to serve, or ``None`` when running API-only.

    ``LIGHTSIM_STATIC_DIR`` is set by the desktop shell; otherwise we look for
    a local ``frontend/dist`` so ``uvicorn app.main:app`` also serves a UI once
    the frontend has been built.
    """
    override = os.environ.get("LIGHTSIM_STATIC_DIR")
    candidate = (
        Path(override).expanduser()
        if override
        else BUNDLE_DIR.parent / "frontend" / "dist"
    )
    return candidate if (candidate / "index.html").is_file() else None
