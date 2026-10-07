"""Projects the user trusts to run code (STD-02).

A project can carry code that runs on the user's computer when the model
runs: Script blocks, and attached FMUs, AI models or programs. Before such a
project's first run the UI asks once whether the user trusts it, and
remembers the answer here, by a fingerprint of that code (a SHA-256 hash the
UI computes over the scripts and the attached files' hashes). The same code
is not asked about again; changed code (a teammate's new FMU, an edited
script pulled with git) is. ``.trusted`` in the projects folder keeps the
last :data:`KEEP` fingerprints.
"""
from __future__ import annotations

import json
import re
import threading

from .fileio import write_atomic
from .paths import projects_dir

KEEP = 1000
FINGERPRINT = re.compile(r"[0-9a-f]{64}")
_FILE = ".trusted"
_lock = threading.Lock()


def _read() -> list[str]:
    try:
        raw = json.loads((projects_dir() / _FILE).read_bytes())
    except (OSError, ValueError):
        return []
    items = raw.get("trusted") if isinstance(raw, dict) else None
    return [f for f in items if isinstance(f, str) and FINGERPRINT.fullmatch(f)] if isinstance(items, list) else []


def _check(fingerprint: str) -> str:
    if not isinstance(fingerprint, str) or not FINGERPRINT.fullmatch(fingerprint):
        raise ValueError(f"Invalid fingerprint: {fingerprint!r}")
    return fingerprint


def is_trusted(fingerprint: str) -> bool:
    _check(fingerprint)
    with _lock:
        return fingerprint in _read()


def trust(fingerprint: str) -> None:
    _check(fingerprint)
    with _lock:
        kept = [f for f in _read() if f != fingerprint]
        folder = projects_dir()
        folder.mkdir(parents=True, exist_ok=True)
        data = {"trusted": [*kept[-(KEEP - 1):], fingerprint]}
        write_atomic(folder / _FILE, (json.dumps(data, indent=1) + "\n").encode("utf-8"))
