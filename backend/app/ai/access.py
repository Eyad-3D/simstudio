"""What an AI assistant may do, and the local record of what it did (AI-01's
intent, built for AI-03).

- An agent works hands-off: the MCP server never asks the user to confirm an
  action (the owner's decision, 1 November 2026). Reading, checking,
  comparing, saving an edit and running a project all go through without a
  question. Safety comes from the design instead (step 6): an agent's Script
  blocks and FMUs run only in an OS sandbox (file access to the project's
  folder only, and no network a Script can open — an FMU's native code is
  held off by the OS; docs/KNOWN-LIMITS.md has the per-platform detail), and
  the run is refused when that sandbox cannot be set up
  (:meth:`Policy.strict_sandbox_refusal`) rather than run less safely.
  Every edit it saves is recoverable from the project's backups (a new
  project for an example). The tool annotations it publishes are only hints
  to the AI app.
- The per-project trust list that ``lightsim ai trust`` keeps
  (``lightsim/ai_access.py``) is **not** read here: only ``AgentSession``
  reads it. The engine's own approval of Script code
  (``app/script_trust.py``) is **not** applied to an agent's run either: a
  strict run is made safe by the OS sandbox, so a brand-new script the agent
  writes runs in the sandbox without the app's **Run scripts** approval (the
  owner's hands-off decision). That approval still guards a run a person
  starts in the app (``STRICT`` off).
- Each tool call that reaches :class:`~app.ai.tools.Tools` goes into a local
  audit log, JSON lines in ``.ai/audit.jsonl`` next to the user's projects,
  done or not. A run sent as a task that is refused before it starts, and a
  call the server cannot read (an unknown tool, arguments that are not an
  object) stop in the MCP server first and are not logged. It is never
  uploaded.
"""
from __future__ import annotations

import json
import os
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

    def __init__(self, *, allow_edits: bool = True,
                 max_run_seconds: float = DEFAULT_MAX_RUN_SECONDS,
                 platform: str = sys.platform) -> None:
        self.allow_edits = allow_edits
        self.max_run_seconds = max_run_seconds
        self.platform = platform

    def strict_sandbox_refusal(self) -> Optional[str]:
        """Why an agent may not run a project with Script blocks or FMUs on
        this platform — the OS sandbox cannot be set up, so the run is refused
        rather than run less safely — or None when it can. The message ends
        with the stable code ``sandbox_unavailable``."""
        from ..solver.sandbox import why_no_strict_sandbox

        return why_no_strict_sandbox(self.platform)


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


def recent_audit_entries(folder: Path, n: int = 20) -> list[dict]:
    """The last ``n`` entries of the audit log of the projects in ``folder``,
    oldest first (AI-01: the app's AI access page lists them)."""
    from lightsim.ai_access import read_audit

    return read_audit(Path(folder) / ".ai" / "audit.jsonl", n)


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
