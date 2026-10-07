"""The FMU worker: runs one FMU's native code in its own locked-down process.

An FMU is compiled code from a third party. It is never loaded into the
engine: each FMU block in a run gets one worker process (started once per
run), which loads the FMU through FMPy and steps it on the engine's request
over a socket. If the FMU crashes, hangs or eats memory, only the worker
dies; the engine reports the failure in plain words and stays up.

Before the FMU's library is loaded the worker drops what it does not need,
with the same tools as the Script sandbox (``solver/sandbox_worker.harden``):
every inherited file descriptor but the engine socket closed, a memory cap,
no files written, no core dumps, an empty environment, death with the
engine, and on Linux with Landlock read-only access to the unpacked FMU and
the system libraries only (no other files, no TCP). FMPy and NumPy are
imported first, while the filesystem is still open. On Windows and macOS
the guarantees are the ones the Script sandbox documents (memory cap and
kill-with-the-engine on Windows; resource limits on macOS) — see
docs/KNOWN-LIMITS.md.

Wire protocol (frames as in ``sandbox_worker.Conn``):

  engine -> worker   b"I" + json   init: load, instantiate, set start values,
                                   initialise
                     b"S" + packed  step: input values, then doStep(t, h)
                     b"Q"           quit (terminate and free the instance)
  worker -> engine   b"I" + json   {"error": null | "text"}
                     b"R" + packed  output values, in the order init named them
                     b"E" + utf-8   the step failed (message)
"""
from __future__ import annotations

import struct
import sys

from ..solver import sandbox_worker as _sw

MSG_INIT = b"I"
MSG_STEP = b"S"
MSG_QUIT = b"Q"
MSG_STEP_OK = b"R"
MSG_STEP_ERR = b"E"

_STEP_HEAD = struct.Struct(">ddH")  # t, h, number of input values
_U16 = struct.Struct(">H")

#: Native models (tables, solvers) need more room than Script code.
DEFAULT_MEM_BYTES = 2 * 1024 * 1024 * 1024

#: Folders FMU code may need to read after the lock: the C runtime and other
#: system libraries it links against, and the dynamic linker's cache.
_SYSTEM_READ_PATHS = ("/lib", "/lib64", "/usr/lib", "/usr/lib64", "/usr/local/lib",
                      "/etc/ld.so.cache")


def pack_step(t: float, h: float, values: list[float]) -> bytes:
    return b"".join((MSG_STEP, _STEP_HEAD.pack(t, h, len(values)),
                     struct.pack(f">{len(values)}d", *values)))


def unpack_step(body: bytes) -> tuple[float, float, list[float]]:
    t, h, n = _STEP_HEAD.unpack_from(body, 1)
    return t, h, list(struct.unpack_from(f">{n}d", body, 1 + _STEP_HEAD.size))


def pack_values(kind: bytes, values: list[float]) -> bytes:
    return kind + _U16.pack(len(values)) + struct.pack(f">{len(values)}d", *values)


def unpack_values(body: bytes) -> list[float]:
    (n,) = _U16.unpack_from(body, 1)
    return list(struct.unpack_from(f">{n}d", body, 1 + _U16.size))


# ---- typed access -------------------------------------------------------------

_FMI2_GROUP = {"Real": "Real", "Integer": "Integer", "Enumeration": "Integer",
               "Boolean": "Boolean"}
_FMI3_GROUP = {"Enumeration": "Int64"}  # FMI 3.0 enumerations are Int64


def _group(fmi3: bool, type_name: str) -> str:
    if fmi3:
        return _FMI3_GROUP.get(type_name, type_name)
    return _FMI2_GROUP[type_name]


class _Access:
    """Get/set a fixed list of variables of mixed types as floats."""

    def __init__(self, fmu, fmi3: bool, variables: list[tuple[int, str]]):
        self.fmu = fmu
        self.n = len(variables)
        self.groups: list[tuple[str, list[int], list[int]]] = []
        by: dict[str, tuple[list[int], list[int]]] = {}
        for i, (vr, type_name) in enumerate(variables):
            vrs, idx = by.setdefault(_group(fmi3, type_name), ([], []))
            vrs.append(int(vr))
            idx.append(i)
        for g, (vrs, idx) in by.items():
            self.groups.append((g, vrs, idx))

    def get(self) -> list[float]:
        out = [0.0] * self.n
        for g, vrs, idx in self.groups:
            values = getattr(self.fmu, "get" + g)(vrs)
            for i, v in zip(idx, values):
                out[i] = float(v)
        return out

    def set(self, values: list[float]) -> None:
        for g, vrs, idx in self.groups:
            vals = [values[i] for i in idx]
            if g == "Boolean":
                vals = [bool(v >= 0.5) for v in vals]
            elif g not in ("Real", "Float32", "Float64"):
                vals = [int(round(v)) for v in vals]
            getattr(self.fmu, "set" + g)(vrs, vals)


# ---- the FMU --------------------------------------------------------------------

class _Instance:
    def __init__(self, spec: dict):
        self.label = spec["label"]
        self.fmi3 = str(spec["fmiVersion"]).startswith("3")
        unzip = spec["unzipDir"]
        lib = spec["libraryPath"]
        kwargs = dict(guid=spec["guid"], unzipDirectory=unzip,
                      modelIdentifier=spec["modelIdentifier"],
                      instanceName=spec.get("instanceName") or spec["modelIdentifier"],
                      libraryPath=lib,
                      # load what is there; a missing optional function only
                      # fails if LightSim calls it
                      requireFunctions=False)
        if self.fmi3:
            from fmpy.fmi3 import FMU3Slave
            self.fmu = FMU3Slave(**kwargs)
            self.fmu.instantiate()
        else:
            from fmpy.fmi2 import FMU2Slave
            self.fmu = FMU2Slave(**kwargs)
            self.fmu.instantiate()
        self.instantiated = True
        t0 = float(spec.get("t0", 0.0))
        starts = [(int(vr), tn) for vr, tn, _ in spec.get("starts", [])]
        start_values = [float(v) for _, _, v in spec.get("starts", [])]
        if self.fmi3:
            # FMI 3.0: start values are set in Instantiated or Initialization
            # mode; set them before initialising, as FMPy's simulate_fmu does
            if starts:
                _Access(self.fmu, True, starts).set(start_values)
            self.fmu.enterInitializationMode(startTime=t0)
        else:
            self.fmu.setupExperiment(startTime=t0)
            if starts:
                _Access(self.fmu, False, starts).set(start_values)
            self.fmu.enterInitializationMode()
        self.fmu.exitInitializationMode()
        self.inputs = _Access(self.fmu, self.fmi3,
                              [(int(vr), tn) for vr, tn in spec.get("inputs", [])])
        self.outputs = _Access(self.fmu, self.fmi3,
                               [(int(vr), tn) for vr, tn in spec.get("outputs", [])])
        self.time = t0

    def step(self, t: float, h: float, values: list[float]) -> list[float]:
        # The FMU keeps its own clock: step from where it is to t + h, so it
        # never sees a gap or an overlap even when the engine's sample times
        # do not divide its solver step exactly.
        if self.inputs.n:
            self.inputs.set(values)
        end = t + h
        dt = end - self.time
        if dt > 1e-12 * max(1.0, abs(end)):
            if self.fmi3:
                _event, terminate, _early, _last = self.fmu.doStep(self.time, dt)
                if terminate:
                    raise RuntimeError("the FMU asked to stop the simulation")
            else:
                self.fmu.doStep(self.time, dt)
            self.time = end
        return self.outputs.get()

    def close(self) -> None:
        if not getattr(self, "instantiated", False):
            return
        try:
            self.fmu.terminate()
        except Exception:  # noqa: BLE001
            pass
        try:
            self.fmu.freeInstance()
        except Exception:  # noqa: BLE001
            pass
        self.instantiated = False


def _describe_error(e: BaseException) -> str:
    text = str(e).strip() or type(e).__name__
    return text[:500]


def worker_main(sock, mem_bytes: int, read_paths: list[str]) -> None:
    """Entry point of an FMU worker process (spawned by the engine)."""
    import json

    # Everything Python will need is imported while the filesystem is open.
    import fmpy  # noqa: F401
    import fmpy.fmi2  # noqa: F401
    import fmpy.fmi3  # noqa: F401

    summary = _sw.harden(sock.fileno(), mem_bytes,
                         read_paths=[*read_paths, *_SYSTEM_READ_PATHS],
                         drop_modules=False)
    try:
        sys.stderr.write(f"[fmu sandbox] {summary}\n")
        sys.stderr.flush()
    except Exception:  # noqa: BLE001
        pass

    conn = _sw.Conn(sock)
    inst: _Instance | None = None
    try:
        while True:
            body = conn.recv()
            kind = body[:1]
            if kind == MSG_STEP:
                t, h, values = unpack_step(body)
                try:
                    out = inst.step(t, h, values)  # type: ignore[union-attr]
                except Exception as e:  # noqa: BLE001
                    conn.send(MSG_STEP_ERR + _describe_error(e).encode("utf-8"))
                else:
                    conn.send(pack_values(MSG_STEP_OK, out))
            elif kind == MSG_INIT:
                spec = json.loads(body[1:].decode("utf-8"))
                err = None
                try:
                    inst = _Instance(spec)
                except Exception as e:  # noqa: BLE001
                    err = _describe_error(e)
                conn.send(MSG_INIT + json.dumps({"error": err}).encode("utf-8"))
            elif kind == MSG_QUIT:
                return
    except (EOFError, ConnectionError, BrokenPipeError):
        return
    finally:
        if inst is not None:
            inst.close()
        try:
            sock.close()
        except Exception:  # noqa: BLE001
            pass
