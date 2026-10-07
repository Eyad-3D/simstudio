"""Engine side of an FMU block: one locked-down worker process per FMU.

:class:`FmuSandbox` starts the worker (``worker.py``) when a run is built,
sends it the FMU to load, then per step the block's input values and gets
its output values back. It never loads the FMU's code itself.

A step that takes longer than ``step_limit`` seconds (a hung FMU) gets the
worker killed and the run stopped with a plain message; a crashing FMU ends
its worker, which the engine reports the same way. On Windows the worker is
held in a Job object (memory cap, dies with the engine), as Script blocks.
"""
from __future__ import annotations

import json
import logging
import multiprocessing
import socket
import sys
from dataclasses import dataclass, field

from ..solver import sandbox_worker as _sw
from . import FmuError
from . import worker as _w

log = logging.getLogger(__name__)

#: How long one FMU step may take before the worker counts as hung, and how
#: long loading and initialising the FMU may take.
STEP_LIMIT_S = 30.0
INIT_LIMIT_S = 60.0


@dataclass
class FmuSpec:
    """One FMU block to run: what to load and how its pins map to variables."""
    el_id: str
    label: str
    unzip_dir: str
    library_path: str
    fmi_version: str
    guid: str
    model_identifier: str
    #: (value reference, FMI type name) per input pin, in pin order
    inputs: list[tuple[int, str]] = field(default_factory=list)
    #: (value reference, FMI type name) per output pin, in pin order
    outputs: list[tuple[int, str]] = field(default_factory=list)
    #: (value reference, FMI type name, value) set before initialisation
    starts: list[tuple[int, str, float]] = field(default_factory=list)


class FmuSandbox:
    def __init__(self, spec: FmuSpec, *, t0: float = 0.0,
                 step_limit: float = STEP_LIMIT_S, init_limit: float = INIT_LIMIT_S,
                 mem_bytes: int = _w.DEFAULT_MEM_BYTES):
        self.spec = spec
        self._dead = False
        self._step_limit = step_limit
        self._job = None
        ctx = multiprocessing.get_context("spawn")
        parent, child = socket.socketpair()
        self._sock = parent
        self._conn = _sw.Conn(parent)
        self._proc = ctx.Process(
            target=_w.worker_main, args=(child, mem_bytes, [spec.unzip_dir]),
            name="lightsim-fmu-sandbox", daemon=True)
        self._proc.start()
        child.close()
        if sys.platform == "win32":
            try:
                self._job, note = _sw.windows_job(self._proc.sentinel, mem_bytes)
            except Exception as e:  # noqa: BLE001 — best effort, never fatal
                note = f"windows job: not applied ({e})"
            if self._job is None:
                log.warning("fmu sandbox: %s", note)

        payload = {
            "label": spec.label,
            "unzipDir": spec.unzip_dir,
            "libraryPath": spec.library_path,
            "fmiVersion": spec.fmi_version,
            "guid": spec.guid,
            "modelIdentifier": spec.model_identifier,
            "instanceName": spec.label,
            "t0": t0,
            "inputs": spec.inputs,
            "outputs": spec.outputs,
            "starts": spec.starts,
        }
        parent.settimeout(init_limit)
        try:
            self._conn.send(_w.MSG_INIT + json.dumps(payload).encode("utf-8"))
            body = self._conn.recv()
        except socket.timeout:
            self._kill()
            raise FmuError(f"FMU '{spec.label}' did not finish loading within "
                           f"{init_limit:g} s.") from None
        except (EOFError, OSError):
            self._kill()
            raise FmuError(f"FMU '{spec.label}' crashed while loading (its process "
                           f"stopped).") from None
        err = json.loads(body[1:].decode("utf-8")).get("error")
        if err:
            self._kill()
            raise FmuError(f"FMU '{spec.label}' could not start: {err}")
        parent.settimeout(step_limit)

    # ---- per step ----------------------------------------------------------------
    def step(self, t: float, h: float, values: list[float]) -> list[float]:
        """Set the inputs, advance the FMU to ``t + h`` and read the outputs."""
        label = self.spec.label
        if self._dead:
            raise FmuError(f"FMU '{label}': its process is no longer running.")
        try:
            self._conn.send(_w.pack_step(t, h, values))
            body = self._conn.recv()
        except socket.timeout:
            self._kill()
            raise FmuError(f"FMU '{label}' did not finish its step at t = {t:g} s within "
                           f"{self._step_limit:g} s, so it was stopped.") from None
        except (EOFError, OSError):
            self._kill()
            raise FmuError(f"FMU '{label}' crashed at t = {t:g} s (its process stopped; "
                           f"LightSim itself is fine). Check the FMU with its supplier.") from None
        kind = body[:1]
        if kind == _w.MSG_STEP_OK:
            return _w.unpack_values(body)
        if kind == _w.MSG_STEP_ERR:
            raise FmuError(f"FMU '{label}' reported an error at t = {t:g} s: "
                           f"{body[1:].decode('utf-8', 'replace')}")
        self._kill()
        raise FmuError(f"FMU '{label}': malformed reply from its process.")

    # ---- lifecycle -----------------------------------------------------------------
    def close(self) -> None:
        if self._dead:
            return
        try:
            self._sock.settimeout(5.0)
            self._conn.send(_w.MSG_QUIT)
        except OSError:
            pass
        self._proc.join(timeout=5.0)
        self._kill()

    def _kill(self) -> None:
        self._dead = True
        try:
            if self._proc.is_alive():
                self._proc.kill()
            self._proc.join(timeout=2.0)
        except (AttributeError, ValueError, OSError, AssertionError):
            pass
        try:
            self._sock.close()
        except OSError:
            pass
        job, self._job = self._job, None
        if job is not None:
            try:
                _sw.close_windows_job(job)
            except Exception:  # noqa: BLE001
                pass

    def __enter__(self) -> "FmuSandbox":
        return self

    def __exit__(self, *exc) -> None:
        self.close()
