"""The app version, read from the repo's single source of truth.

The root ``VERSION`` file is authoritative; ``scripts/sync-version.mjs`` copies
it into the npm manifests that electron-builder and Vite insist on reading.
This module keeps the API's reported version from drifting out of that set.
"""
from __future__ import annotations

from .paths import BUNDLE_DIR

FALLBACK = "0.0.0+unknown"


def _read() -> str:
    # Frozen builds carry VERSION at the bundle root; a source checkout has it
    # one level up from backend/.
    for candidate in (BUNDLE_DIR / "VERSION", BUNDLE_DIR.parent / "VERSION"):
        try:
            text = candidate.read_text(encoding="utf-8").strip()
        except OSError:
            continue
        if text:
            return text
    return FALLBACK


VERSION = _read()
