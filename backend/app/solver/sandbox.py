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

import json
import multiprocessing
import socket
import threading
import time
from dataclasses import dataclass, field

from . import sandbox_worker as _w
from . import scripting
from .scripting import ScriptError


@dataclass
class ScriptSpec:
    """One Script block to run in the sandbox."""
    el_id: str
    label: str
    code: str
    input_keys: list[str] = field(default_factory=list)
    params: dict = field(default_factory=dict)


# How long past the script time limit the engine waits before giving up on the
# worker. Covers scheduling jitter and the round trip; well under the test
# budgets.
_KILL_MARGIN_S = 0.5


class ScriptSandbox:
    """A worker process running a run's Script blocks under OS-level isolation."""

    def __init__(self, specs: list[ScriptSpec], *, time_limit: float | None = None,
                 trusted: bool = False, mem_bytes: int = _w.DEFAULT_MEM_BYTES):
        self._specs = specs
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

        ctx = multiprocessing.get_context("spawn")
        parent, child = socket.socketpair(socket.AF_UNIX, socket.SOCK_STREAM)
        self._sock = parent
        self._conn = _w.Conn(parent)
        self._step_timeout = self._time_limit + _KILL_MARGIN_S
        self._proc = ctx.Process(
            target=_w.worker_main, args=(child, mem_bytes),
            name="lightsim-script-sandbox", daemon=True)
        self._proc.start()
        child.close()  # the worker holds the only other end now

        self._init(trusted)  # uses a socket timeout; the loop below does not

        # Fast per-step path: busy-wait briefly before blocking (see Conn), and
        # let the watchdog enforce the time limit instead of a per-read timeout.
        parent.settimeout(None)
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
            "scripts": [
                {"label": s.label, "code": s.code,
                 "input_keys": s.input_keys, "params": s.params}
                for s in self._specs
            ],
        }
        # Top-level code runs during init; give it the time limit per script
        # (plus a floor) before deciding the worker is wedged.
        self._sock.settimeout(max(5.0, len(self._specs)
                                  * (self._time_limit + _KILL_MARGIN_S)))
        try:
            self._conn.send(_w.MSG_INIT + json.dumps(payload).encode("utf-8"))
            body = self._conn.recv()
        except socket.timeout:
            self._kill()
            raise ScriptError(
                f"Script top-level code did not finish within {self._time_limit:g} s.") from None
        except (EOFError, OSError) as e:
            self._kill()
            raise ScriptError(f"Script sandbox stopped during start-up: {e}") from e
        errors = json.loads(body[1:].decode("utf-8"))["errors"]
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
