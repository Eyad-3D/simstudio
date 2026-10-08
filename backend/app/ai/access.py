"""What an AI assistant may do without asking, and the local record of what
it did (AI-01's intent, built for AI-03).

- Reading a model, its checks and its results needs no confirmation.
- Saving an edit, and running a project that contains Script blocks (code
  the user or someone else wrote), each need the user's confirmation in the
  AI app: the MCP server asks through the app (an ``input_required`` result
  on the 2026-07-28 protocol, an elicitation request on older ones) and
  refuses when the app cannot ask. LightSim enforces this itself; the tool
  annotations it publishes are only hints to the AI app.
- On Windows the Script sandbox is much weaker, so an assistant never runs a
  project with Script blocks there unless the user started the connection
  with ``--trust-scripts``. (AI-01 replaces this with a per-project
  "trusted" mark set in LightSim.)
- Every tool call goes into a local audit log, JSON lines in
  ``.ai/audit.jsonl`` next to the user's projects. It is never uploaded.
"""
from __future__ import annotations

import hashlib
import hmac
import json
import os
import secrets
import sys
import threading
import time
from pathlib import Path
from typing import Optional

#: A run an assistant starts stops after this much wall-clock time.
DEFAULT_MAX_RUN_SECONDS = 300.0
#: The audit log is started afresh (the old one kept as .1) past this size.
AUDIT_MAX_BYTES = 5 * 1024 * 1024


class Policy:
    """The rules one server process applies, set when the AI app starts it."""

    def __init__(self, *, allow_edits: bool = True, trust_scripts: bool = False,
                 max_run_seconds: float = DEFAULT_MAX_RUN_SECONDS,
                 platform: str = sys.platform) -> None:
        self.allow_edits = allow_edits
        self.trust_scripts = trust_scripts
        self.max_run_seconds = max_run_seconds
        self.platform = platform
        self._key = secrets.token_bytes(32)

    def script_run_refusal(self) -> Optional[str]:
        """Why a project with Script blocks may not run at all, or None."""
        if self.platform == "win32" and not self.trust_scripts:
            return ("This project has Script blocks (Python code). On Windows LightSim's script "
                    "sandbox is weak, so an AI assistant may not run such a project unless you "
                    "start the connection with --trust-scripts. Run it in the LightSim app "
                    "instead, or ask the user.")
        return None

    def seal(self, purpose: str, payload: dict) -> str:
        """A fingerprint tying a confirmation to exactly one action: the
        user confirms *this* edit or *this* run, and an answer cannot be
        reused for another."""
        body = json.dumps({"p": purpose, **payload}, sort_keys=True, default=str).encode()
        return hmac.new(self._key, body, hashlib.sha256).hexdigest()


class AuditLog:
    """Appends one JSON line per tool call. Failing to write it never fails
    the call (a read-only disk must not stop the assistant)."""

    def __init__(self, folder: Path) -> None:
        self.path = Path(folder) / ".ai" / "audit.jsonl"
        self._lock = threading.Lock()

    def record(self, **entry: object) -> None:
        line = json.dumps({"t": round(time.time(), 3), **entry}, default=str)
        with self._lock:
            try:
                self.path.parent.mkdir(parents=True, exist_ok=True)
                if self.path.exists() and self.path.stat().st_size > AUDIT_MAX_BYTES:
                    os.replace(self.path, self.path.with_suffix(".jsonl.1"))
                with self.path.open("a", encoding="utf-8") as f:
                    f.write(line + "\n")
            except OSError:
                pass

    def last_connection(self) -> Optional[dict]:
        """The newest entry, for Help > Connect an AI assistant to show
        whether an assistant has been in touch."""
        return last_audit_entry(self.path.parent.parent)


def last_audit_entry(folder: Path) -> Optional[dict]:
    path = Path(folder) / ".ai" / "audit.jsonl"
    try:
        with path.open("rb") as f:
            f.seek(0, os.SEEK_END)
            size = f.tell()
            f.seek(max(0, size - 8192))
            lines = f.read().decode("utf-8", "replace").strip().splitlines()
    except OSError:
        return None
    for line in reversed(lines):
        try:
            entry = json.loads(line)
        except ValueError:
            continue
        if isinstance(entry, dict):
            return entry
    return None
