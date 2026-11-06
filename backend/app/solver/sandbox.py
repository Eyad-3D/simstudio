"""Engine side of the Script sandbox.

A :class:`ScriptSandbox` owns one worker process for a whole simulation run (see
``sandbox_worker.py`` for the worker and what it locks down). The engine sends
each solver step's inputs and gets the block's outputs back; the block's
``state`` lives in the worker. The worker is reused for every step of every
Script block in the run — never started per call — so the only per-step cost is
one small round trip over a socket.

The hard time limit lives here: the engine waits at most ``time_limit`` plus a
margin for each reply and, if the worker has not answered, kills it and fails
the run. That is what stops a C-level endless loop or a hang that the worker's
own (in-process) tracer cannot; a memory blow-up trips the worker's address-
space cap and shows up here as the worker stopping.

Only runs that actually have Script blocks build a sandbox; a model without
them never starts a worker.
"""
from __future__ import annotations

import contextvars
import json
import logging
import multiprocessing
import os
import socket
import sys
import threading
import time
from dataclasses import dataclass, field
from typing import Optional

from . import sandbox_worker as _w
from . import scripting
from .scripting import ScriptError

#: The stable code an agent's run reports when the OS sandbox for its Script
#: blocks or FMUs cannot be set up (defined in sandbox_worker, the module the
#: FMU side also imports, so there is one source of truth).
SANDBOX_UNAVAILABLE = _w.SANDBOX_UNAVAILABLE

#: Set True for an agent's run (app/ai): its Script and FMU blocks must run
#: OS-confined (no network, no file access outside the project), and the run
#: is refused when that confinement cannot be set up. A person's run in the
#: app leaves it False and keeps today's best-effort isolation. A ContextVar,
#: so it is per-run and needs no new parameter through the solver.
STRICT = contextvars.ContextVar("lightsim_strict_sandbox", default=False)


def command_line_strict() -> bool:
    """Whether a run of the command-line tool (``lightsim run``, and the
    desktop engine's ``lightsim-backend run``) is strict. It is, unless
    ``LIGHTSIM_STRICT_SANDBOX`` is 0, false, no or off: the tool cannot tell an
    agent from a person, and an agent must never wait on a person's approval
    (the owner's hands-off decision, 1 November 2026), so its runs follow the
    MCP server's rules. The variable set to 0 gives a person the app's rules
    (script approval, best-effort isolation); an agent gains nothing by it but
    the approval it cannot give."""
    return os.environ.get("LIGHTSIM_STRICT_SANDBOX", "").strip().lower() not in ("0", "false", "no", "off")


def why_no_strict_sandbox(platform: Optional[str] = None) -> Optional[str]:
    """None when the strict agent sandbox (file-confined, no network) can be
    set up on this platform, otherwise a plain message ending with the stable
    code, for the refused run to report. ``platform`` overrides the real one
    (the tests, and the AI policy's simulated platform)."""
    platform = platform or sys.platform
    who = "A run by an AI agent, or from the command line,"
    tail = (f"The run is refused rather than run less safely [{SANDBOX_UNAVAILABLE}]. "
            "Run the project in the LightSim app instead (from the command line, "
            "LIGHTSIM_STRICT_SANDBOX=0 runs it under the app's rules).")
    if platform == "win32":
        # The Windows sandbox is an AppContainer with no capabilities
        # (win_sandbox.py). It is allowed only when a runtime probe confirms a
        # confined worker can really be launched here (so the claim is never
        # made falsely); on any other machine (including when the real
        # platform is not Windows) the probe cannot run and the run is refused.
        from . import win_sandbox
        if win_sandbox.appcontainer_available():
            return None
        return (f"{who} can run a project's Script blocks or FMUs only inside the Windows "
                "sandbox — an AppContainer with no network and no access to your files — "
                f"which could not be set up on this computer. {tail}")
    if platform.startswith("linux") and _w.landlock_abi() < 0:
        return (f"{who} can run a project's Script blocks or FMUs only inside the sandbox "
                "(no network, no file access outside the project), which needs Landlock "
                f"(Linux 5.13 or newer); this kernel does not offer it. {tail}")
    return None


@dataclass
class ScriptSpec:
    """One Script block to run in the sandbox."""
    el_id: str
    label: str
    code: str
    input_keys: list[str] = field(default_factory=list)
    params: dict = field(default_factory=dict)


log = logging.getLogger(__name__)

# How long past the script time limit the engine waits before giving up on the
# worker. Covers scheduling jitter and the round trip; well under the test
# budgets.
_KILL_MARGIN_S = 0.5
# What an AppContainer worker (Windows, a strict run) may add to that wait for
# its own start-up: a fresh interpreter inside the container.
_AC_START_S = 30.0


class ScriptSandbox:
    """A worker process running a run's Script blocks under OS-level isolation."""

    def __init__(self, specs: list[ScriptSpec], *, time_limit: float | None = None,
                 trusted: bool = False, mem_bytes: int = _w.DEFAULT_MEM_BYTES,
                 strict: bool = False):
        self._specs = specs
        self._strict = strict
        self._time_limit = scripting.TIME_LIMIT_S if time_limit is None else time_limit
        self._idx = {s.el_id: i for i, s in enumerate(specs)}
        self._input_keys = [s.input_keys for s in specs]
        self._param_snapshots = [dict(s.params) for s in specs]
        self._dead = False
        self._timed_out = False
        # The in-flight step's monotonic deadline (None between steps); a
        # watchdog thread watches it. Kept off the hot path so the per-step
        # socket read is a plain blocking recv — the cheapest option — with no
        # per-call timeout syscall.
        self._deadline: float | None = None

        self._step_timeout = self._time_limit + _KILL_MARGIN_S
        # A strict run on Windows runs the worker inside an AppContainer (no
        # network, file access only to the folders its SID is granted), over an
        # inherited pipe, since a capability-less AppContainer cannot use the
        # loopback TCP socketpair. The run is refused when that cannot be set
        # up. Every other run uses the spawn + socketpair worker.
        self._ac = None
        if strict and sys.platform == "win32":
            from . import win_sandbox
            try:
                self._ac = win_sandbox.launch("script", mem_bytes)
            except win_sandbox.SandboxSetupError as e:
                raise ScriptError(
                    "This project's Script blocks cannot be run OS-confined (no network, no file "
                    f"access outside the project) on this computer [{SANDBOX_UNAVAILABLE}], so the "
                    f"run is refused rather than run less safely ({e}).") from e
            self._sock = self._ac
            self._conn = self._ac.conn
            self._proc = self._ac.proc
            self._job = self._ac.job
        else:
            ctx = multiprocessing.get_context("spawn")
            # The platform's default pair: Unix-domain on Linux, a loopback TCP
            # pair on Windows (whose Python has no AF_UNIX).
            parent, child = socket.socketpair()
            self._sock = parent
            self._conn = _w.Conn(parent)
            self._proc = ctx.Process(
                target=_w.worker_main, args=(child, mem_bytes),
                name="lightsim-script-sandbox", daemon=True)
            self._proc.start()
            child.close()  # the worker holds the only other end now
            # Windows: the engine holds the worker's job object (memory cap, and
            # the worker dies when the engine does). Set before any script runs.
            self._job = None
            if sys.platform == "win32":
                try:
                    self._job, note = _w.windows_job(self._proc.sentinel, mem_bytes)
                except Exception as e:  # noqa: BLE001 — best effort, never fatal
                    note = f"windows job: not applied ({e})"
                if self._job is None:
                    log.warning("script sandbox: %s", note)

        self._init(trusted)  # uses a socket timeout (or a timer on the pipe)

        # Fast per-step path: busy-wait briefly before blocking (see Conn), and
        # let the watchdog enforce the time limit instead of a per-read timeout.
        # The AppContainer pipe reads block, so there the watchdog is the only
        # time limit (no spin).
        if self._ac is None:
            self._sock.settimeout(None)
            self._conn._spin = True
        self._watch_stop = threading.Event()
        self._watch = threading.Thread(
            target=self._watchdog, name="lightsim-sandbox-watchdog", daemon=True)
        self._watch.start()

    def _watchdog(self) -> None:
        """Kill the worker if a step overruns. Polls a deadline the run loop
        sets per step, so the run loop itself pays nothing for the time limit."""
        while not self._watch_stop.wait(0.05):
            dl = self._deadline
            if dl is not None and not self._dead and time.monotonic() > dl:
                self._timed_out = True
                self._kill()
                return

    # ---- lifecycle -----------------------------------------------------------
    def _init(self, trusted: bool) -> None:
        payload = {
            "time_limit": self._time_limit,
            "trusted": trusted,
            "strict": self._strict,  # the worker runs no code unconfined under this
            "scripts": [
                {"label": s.label, "code": s.code,
                 "input_keys": s.input_keys, "params": s.params}
                for s in self._specs
            ],
        }
        # Top-level code runs during init; give it the time limit per script
        # (plus a floor) before deciding the worker is wedged. The socket path
        # uses a socket timeout; the AppContainer pipe blocks, so a timer kills
        # the worker, which ends the read.
        init_timeout = max(5.0, len(self._specs) * (self._time_limit + _KILL_MARGIN_S))
        timer = None
        if self._ac is None:
            self._sock.settimeout(init_timeout)
        else:
            # the container's Python starts within this wait too, and takes
            # seconds more while other runs' ACL grants keep the disk busy
            timer = threading.Timer(init_timeout + _AC_START_S, self._kill)
            timer.start()
        try:
            self._conn.send(_w.MSG_INIT + json.dumps(payload).encode("utf-8"))
            body = self._conn.recv()
        except socket.timeout:
            self._kill()
            raise ScriptError(
                f"Script top-level code did not finish within {self._time_limit:g} s.") from None
        except (EOFError, OSError) as e:
            self._kill()
            detail = ""
            if self._ac is not None and self._ac.stderr_text():
                detail = f" ({self._ac.stderr_text().strip()[:200]})"
            raise ScriptError(f"Script sandbox stopped during start-up: {e}{detail}") from e
        finally:
            if timer is not None:
                timer.cancel()
        reply = json.loads(body[1:].decode("utf-8"))
        if self._strict and not reply.get("confined"):
            # a strict agent run must be OS-confined; never fall back silently
            self._kill()
            raise ScriptError(
                "This project's Script blocks cannot be run OS-confined (no network, no file "
                f"access outside the project) on this computer [{SANDBOX_UNAVAILABLE}], so the "
                "run is refused rather than run less safely.")
        errors = reply["errors"]
        for err in errors:
            if err:
                # A bad script fails the build (as RunContext did before). __init__
                # is aborting and nobody will hold this object to close() it, so
                # stop the worker now rather than leave it running.
                self._kill()
                raise ScriptError(err)

    def close(self) -> None:
        self._watch_stop.set()
        if self._dead:
            return
        try:
            self._conn.send(_w.MSG_QUIT)
        except OSError:
            pass
        self._proc.join(timeout=_KILL_MARGIN_S)
        if self._proc.is_alive():
            self._kill()
        else:
            self._dead = True
            try:
                self._sock.close()
            except OSError:
                pass
            self._release_job()

    def _release_job(self) -> None:
        job, self._job = getattr(self, "_job", None), None
        if job is not None:
            try:
                _w.close_windows_job(job)  # kill-on-close: ends a worker still running
            except Exception:  # noqa: BLE001
                pass

    def _kill(self) -> None:
        self._dead = True
        try:
            self._watch_stop.set()
        except AttributeError:
            pass
        try:
            self._proc.kill()
        except (AttributeError, ValueError, OSError):
            pass
        try:
            self._proc.join(timeout=1.0)
        except (AssertionError, ValueError):
            pass
        try:
            self._sock.close()
        except OSError:
            pass
        self._release_job()

    def __enter__(self) -> "ScriptSandbox":
        return self

    def __exit__(self, *exc) -> None:
        self.close()

    # ---- per-step ------------------------------------------------------------
    def run(self, el_id: str, label: str, t: float, dt: float,
            inputs: dict, params: dict) -> dict:
        """Run one Script block's step() in the worker and return its outputs.
        Mirrors scripting.run_script's signature (minus the state dict, which
        lives in the worker) so the call site barely changes."""
        if self._dead:
            raise ScriptError(f"Script '{label}': the sandbox is no longer running.")
        idx = self._idx[el_id]

        if params != self._param_snapshots[idx]:
            self._param_snapshots[idx] = dict(params)
            self._send(
                _w.MSG_PARAM + json.dumps({"idx": idx, "params": params}).encode("utf-8"),
                label, t)

        keys = self._input_keys[idx]
        values = [float(inputs.get(k, 0.0) or 0.0) for k in keys]
        self._deadline = time.monotonic() + self._step_timeout
        try:
            self._conn.send(_w._pack_step(idx, t, dt, values))
            body = self._conn.recv()
        except (EOFError, OSError):
            # The watchdog kills a worker that overran, which ends this recv;
            # tell the two cases apart for the message.
            if self._timed_out:
                raise ScriptError(
                    f"Script '{label}' did not return within {self._time_limit:g} s "
                    f"at t = {t:g} s — check it for an endless loop.") from None
            self._kill()
            raise ScriptError(
                f"Script '{label}': the sandbox worker stopped at t = {t:g} s "
                f"(it may have exceeded the memory limit).") from None
        finally:
            self._deadline = None
        kind = body[:1]
        if kind == _w.MSG_STEP_OK:
            return _w.unpack_outputs(memoryview(body))
        if kind == _w.MSG_STEP_ERR:
            raise ScriptError(body[1:].decode("utf-8"))
        self._kill()
        raise ScriptError(f"Script '{label}': malformed reply from the sandbox worker.")

    def _send(self, body: bytes, label: str, t: float) -> None:
        try:
            self._conn.send(body)
        except OSError:
            self._kill()
            raise ScriptError(
                f"Script '{label}': the sandbox worker stopped at t = {t:g} s "
                f"(it may have exceeded the memory limit).") from None
