"""The Script sandbox worker: the second isolation layer for user Script code.

One worker process is started per simulation run that has Script blocks (never
per call — scripts run every 10 ms). It talks to the engine over a socket: the
engine sends a step's inputs, the worker runs the block's ``step()`` and sends
the outputs back. The block's ``state`` dict lives here, in the worker, for the
whole run.

Isolation. Before it runs any user code the worker drops the privileges it does
not need, as strongly as each platform allows without root:

* **Linux** — closes every inherited file descriptor except the engine socket;
  caps address space, file size (to 0 — nothing can be written to a regular
  file) and the open-file count with ``resource.setrlimit``; clears the
  environment; and, when the kernel offers it (Landlock, Linux ≥ 5.13, via
  ``ctypes`` — no root, no seccomp), forbids the process every filesystem
  access and every outbound/*listening* TCP connection. Landlock is inherited
  across ``execve``, so even a program the code manages to start stays caged.
  What Landlock does *not* cover: UDP and Unix-domain sockets, and, on kernels
  without Landlock, only the resource limits, the closed descriptors and the
  restricted namespace apply — see ``docs/KNOWN-LIMITS.md``.
* **Windows** — the engine puts the worker in a Job object (via ``ctypes``)
  with a memory cap and kill-on-close, holding its only handle, so the worker
  and anything it starts die with the engine; the worker clears its
  environment. The Windows standard library offers no portable way to filter
  filesystem or network syscalls, so there the real guarantees are the memory
  cap, the parent's kill-on-timeout, and the in-process restriction (the
  allow-listed builtins and import block, which the engine applies before this
  layer). This is stated honestly rather than papered over.

Seccomp is deliberately *not* used: a syscall-level allow-list strict enough to
matter is easy to get wrong (one missing syscall kills the worker with SIGSYS
on a code path we did not test), whereas Landlock denies filesystem and network
access declaratively and fails closed without that risk.

The hard time limit is enforced by the *engine*, not here: it waits at most
``time_limit`` (plus a small margin) for each reply and, if the worker has not
answered — a hung script, or a C-level endless loop such as ``list(iter(int,
1))`` that no in-process tracer can interrupt — kills the worker and fails the
run. A memory blow-up hits the address-space cap (a ``MemoryError`` the worker
reports, or a clean allocation failure) instead of the machine.
"""
from __future__ import annotations

import os
import struct
import sys
import time

# ---- wire protocol -----------------------------------------------------------
# Every frame is a 4-byte big-endian length followed by that many bytes; the
# first byte of the body is the message type. Requests (engine -> worker) and
# replies (worker -> engine) never cross: the worker only ever replies to the
# request it is handling, so the socket needs no request ids.
#
# Engine -> worker:
#   b"I" + json   init: compile every script, run its top-level code
#   b"S" + packed step: run one script's step() for this solver step
#   b"P" + json   param: replace one script's stored params (a live edit)
#   b"Q"          quit
# Worker -> engine:
#   b"I" + json   init result: per-script null (ok) or an error string
#   b"R" + packed step outputs (name -> value)
#   b"E" + utf-8  the script failed this step (message)

MSG_INIT = b"I"
MSG_STEP = b"S"
MSG_PARAM = b"P"
MSG_QUIT = b"Q"
MSG_STEP_OK = b"R"
MSG_STEP_ERR = b"E"

_LEN = struct.Struct(">I")
_STEP_HEAD = struct.Struct(">Hdd")  # script index, t, dt
_U16 = struct.Struct(">H")
_F64 = struct.Struct(">d")


# Scripts run every 10 ms of simulated time, and between two requests the
# engine spends only a couple of hundred microseconds on the rest of the step. A
# process that blocks pays the scheduler a wake-up (tens of µs) on every
# request; a short busy-wait first keeps both ends "hot" so a round trip costs a
# few µs. The window is bounded, so a paced (real-time) run, which sleeps
# between recorded steps, falls back to a blocking read there. A run going flat
# out never waits that long, so a spinning worker keeps one core busy for the
# whole run (see worker_main for when it spins).
SPIN_WINDOW_S = 0.001


class Conn:
    """A length-prefixed frame channel over a stream socket, with one buffered
    ``recv`` per frame in the common case (a whole small frame arrives at once).
    Used by both the engine and the worker so the hot per-step path is one send
    and one receive on each side. With ``spin`` set, a read busy-waits up to
    ``SPIN_WINDOW_S`` before blocking, to avoid the per-reply scheduler wake-up."""

    __slots__ = ("sock", "_buf", "_spin")

    def __init__(self, sock, spin: bool = False):
        self.sock = sock
        self._buf = bytearray()
        self._spin = spin

    def send(self, body: bytes) -> None:
        self.sock.sendall(_LEN.pack(len(body)) + body)

    def _fill(self) -> None:
        sock = self.sock
        if self._spin:
            sock.setblocking(False)
            deadline = time.perf_counter() + SPIN_WINDOW_S
            try:
                while True:
                    try:
                        chunk = sock.recv(65536)
                        break
                    except (BlockingIOError, InterruptedError):
                        if time.perf_counter() >= deadline:
                            sock.setblocking(True)
                            chunk = sock.recv(65536)
                            break
            finally:
                sock.setblocking(True)
        else:
            chunk = sock.recv(65536)
        if not chunk:
            raise EOFError("sandbox socket closed")
        self._buf += chunk

    def recv(self) -> bytes:
        buf = self._buf
        while len(buf) < 4:
            self._fill()
        (n,) = _LEN.unpack_from(buf, 0)
        need = 4 + n
        while len(buf) < need:
            self._fill()
        frame = bytes(buf[4:need])
        del buf[:need]
        return frame


def _pack_step(idx: int, t: float, dt: float, values: list[float]) -> bytes:
    out = [MSG_STEP, _STEP_HEAD.pack(idx, t, dt), _U16.pack(len(values))]
    out.append(struct.pack(f">{len(values)}d", *values))
    return b"".join(out)


def _unpack_step(body: memoryview):
    idx, t, dt = _STEP_HEAD.unpack_from(body, 1)
    (n,) = _U16.unpack_from(body, 1 + _STEP_HEAD.size)
    off = 1 + _STEP_HEAD.size + _U16.size
    values = list(struct.unpack_from(f">{n}d", body, off))
    return idx, t, dt, values


def _pack_outputs(out: dict) -> bytes:
    parts = [MSG_STEP_OK, _U16.pack(len(out))]
    for key, value in out.items():
        kb = str(key).encode("utf-8")
        parts.append(_U16.pack(len(kb)))
        parts.append(kb)
        parts.append(_F64.pack(value))
    return b"".join(parts)


def unpack_outputs(body: memoryview) -> dict:
    """Decode a b"R" reply into {name: float}. Used by the engine side."""
    (count,) = _U16.unpack_from(body, 1)
    off = 1 + _U16.size
    out: dict[str, float] = {}
    for _ in range(count):
        (klen,) = _U16.unpack_from(body, off)
        off += _U16.size
        key = bytes(body[off:off + klen]).decode("utf-8")
        off += klen
        (val,) = _F64.unpack_from(body, off)
        off += _F64.size
        out[key] = val
    return out


# ---- isolation ---------------------------------------------------------------
# The memory cap: enough for ordinary signal math and the interpreter, small
# enough that a runaway allocation fails instead of taking the machine down.
DEFAULT_MEM_BYTES = 512 * 1024 * 1024
# No CPU-time cap: the worker busy-waits briefly between steps (see Conn), so
# its CPU time grows with the run's length, and a cap would end long runs that
# are fine. A runaway step is stopped by the engine's wall-clock kill, and the
# worker dies with the engine (PR_SET_PDEATHSIG / the Windows job).
MAX_OPEN_FILES = 64

# Modules pulled in transitively that user code has no business reaching. The
# restricted builtins already block ``import`` of anything but ``math``; this is
# a second line for the trusted path and for anything that slips a reference
# through. os/socket/struct/marshal are kept: the worker's own loop needs them.
_DROP_MODULES = (
    "subprocess", "shutil", "pathlib", "tempfile", "http", "urllib",
    "urllib.request", "ftplib", "smtplib", "socketserver", "asyncio",
    "multiprocessing", "pickle", "shelve", "webbrowser", "pty", "tty",
)


def _close_inherited_fds(keep: set[int]) -> None:
    try:
        fds = [int(x) for x in os.listdir("/proc/self/fd")]
    except OSError:
        try:
            maxfd = os.sysconf("SC_OPEN_MAX")
        except (AttributeError, ValueError):
            maxfd = 4096
        fds = range(3, maxfd)
    for fd in fds:
        if fd in keep:
            continue
        try:
            os.close(fd)
        except OSError:
            pass


def _set_rlimits(mem_bytes: int) -> None:
    import resource  # POSIX only

    def _cap(res: int, soft, hard=None):
        hard = soft if hard is None else hard
        try:
            cur_soft, cur_hard = resource.getrlimit(res)
            if cur_hard != resource.RLIM_INFINITY:
                hard = min(hard, cur_hard)
                soft = min(soft, hard)
            resource.setrlimit(res, (soft, hard))
        except (ValueError, OSError):
            pass

    _cap(resource.RLIMIT_AS, mem_bytes)          # address space (memory)
    _cap(resource.RLIMIT_FSIZE, 0)               # cannot write a regular file
    _cap(resource.RLIMIT_NOFILE, MAX_OPEN_FILES)  # few open files
    try:
        _cap(resource.RLIMIT_CORE, 0)            # no core dumps
    except AttributeError:
        pass


# Landlock (Linux ≥ 5.13). Numbers are stable across architectures.
_SYS_landlock_create_ruleset = 444
_SYS_landlock_restrict_self = 446
_PR_SET_NO_NEW_PRIVS = 38
_LANDLOCK_CREATE_RULESET_VERSION = 1
# All filesystem access rights, by the ABI that introduced each. Handling a
# right while adding no rule that grants it denies it everywhere.
_FS_RIGHTS_BY_ABI = {
    1: (1 << 0) | (1 << 1) | (1 << 2) | (1 << 3) | (1 << 4) | (1 << 5)
       | (1 << 6) | (1 << 7) | (1 << 8) | (1 << 9) | (1 << 10) | (1 << 11)
       | (1 << 12),
    2: (1 << 13),   # REFER
    3: (1 << 14),   # TRUNCATE
    5: (1 << 15),   # IOCTL_DEV
}
_NET_BIND_TCP = 1 << 0
_NET_CONNECT_TCP = 1 << 1


def _apply_landlock(note) -> str:
    """Best effort. Returns a short description of what was enforced."""
    import ctypes

    libc = ctypes.CDLL(None, use_errno=True)
    libc.syscall.restype = ctypes.c_long

    abi = libc.syscall(_SYS_landlock_create_ruleset, None, ctypes.c_size_t(0),
                       ctypes.c_uint32(_LANDLOCK_CREATE_RULESET_VERSION))
    if abi < 0:
        return "landlock: unavailable (kernel too old); relying on rlimits"

    handled_fs = 0
    for ver, bits in _FS_RIGHTS_BY_ABI.items():
        if abi >= ver:
            handled_fs |= bits
    handled_net = (_NET_BIND_TCP | _NET_CONNECT_TCP) if abi >= 4 else 0

    if abi >= 4:
        attr = struct.pack("=QQ", handled_fs, handled_net)
    else:
        attr = struct.pack("=Q", handled_fs)
    buf = ctypes.create_string_buffer(attr, len(attr))

    ruleset_fd = libc.syscall(_SYS_landlock_create_ruleset, buf,
                              ctypes.c_size_t(len(attr)), ctypes.c_uint32(0))
    if ruleset_fd < 0:
        return f"landlock: create_ruleset failed (errno {ctypes.get_errno()})"

    # No rules are added, so every handled access is denied.
    if libc.prctl(_PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0:
        os.close(ruleset_fd)
        return "landlock: PR_SET_NO_NEW_PRIVS failed"
    rc = libc.syscall(_SYS_landlock_restrict_self, ctypes.c_int(ruleset_fd),
                      ctypes.c_uint32(0))
    os.close(ruleset_fd)
    if rc != 0:
        return f"landlock: restrict_self failed (errno {ctypes.get_errno()})"
    net = "no TCP" if abi >= 4 else "TCP not covered (ABI<4)"
    return f"landlock ABI {abi}: no filesystem access, {net}"


def windows_job(process_handle: int, mem_bytes: int):
    """Put the worker in a Windows Job object that caps its memory and kills it
    when the job's last handle closes. Called by the *engine* with the worker's
    process handle, so the engine holds that handle: when the engine exits, even
    by a crash, Windows closes it and the worker (and anything it started) dies
    too. Returns (job handle or None, a one-line note)."""
    import ctypes
    from ctypes import wintypes

    k32 = ctypes.WinDLL("kernel32", use_last_error=True)

    JOB_OBJECT_LIMIT_PROCESS_MEMORY = 0x00000100
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE = 0x00002000
    JobObjectExtendedLimitInformation = 9

    class JOBOBJECT_BASIC_LIMIT_INFORMATION(ctypes.Structure):
        _fields_ = [
            ("PerProcessUserTimeLimit", ctypes.c_int64),
            ("PerJobUserTimeLimit", ctypes.c_int64),
            ("LimitFlags", wintypes.DWORD),
            ("MinimumWorkingSetSize", ctypes.c_size_t),
            ("MaximumWorkingSetSize", ctypes.c_size_t),
            ("ActiveProcessLimit", wintypes.DWORD),
            ("Affinity", ctypes.c_size_t),
            ("PriorityClass", wintypes.DWORD),
            ("SchedulingClass", wintypes.DWORD),
        ]

    class IO_COUNTERS(ctypes.Structure):
        _fields_ = [(n, ctypes.c_uint64) for n in (
            "ReadOperationCount", "WriteOperationCount", "OtherOperationCount",
            "ReadTransferCount", "WriteTransferCount", "OtherTransferCount")]

    class JOBOBJECT_EXTENDED_LIMIT_INFORMATION(ctypes.Structure):
        _fields_ = [
            ("BasicLimitInformation", JOBOBJECT_BASIC_LIMIT_INFORMATION),
            ("IoInfo", IO_COUNTERS),
            ("ProcessMemoryLimit", ctypes.c_size_t),
            ("JobMemoryLimit", ctypes.c_size_t),
            ("PeakProcessMemoryUsed", ctypes.c_size_t),
            ("PeakJobMemoryUsed", ctypes.c_size_t),
        ]

    k32.CreateJobObjectW.restype = wintypes.HANDLE
    k32.AssignProcessToJobObject.argtypes = (wintypes.HANDLE, wintypes.HANDLE)
    k32.SetInformationJobObject.argtypes = (
        wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD)
    k32.CloseHandle.argtypes = (wintypes.HANDLE,)
    job = k32.CreateJobObjectW(None, None)
    if not job:
        return None, "windows job: CreateJobObject failed"
    info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION()
    info.BasicLimitInformation.LimitFlags = (
        JOB_OBJECT_LIMIT_PROCESS_MEMORY | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE)
    info.ProcessMemoryLimit = mem_bytes
    if not k32.SetInformationJobObject(
            job, JobObjectExtendedLimitInformation, ctypes.byref(info),
            ctypes.sizeof(info)):
        k32.CloseHandle(job)
        return None, f"windows job: SetInformationJobObject failed ({ctypes.get_last_error()})"
    if not k32.AssignProcessToJobObject(job, process_handle):
        k32.CloseHandle(job)
        return None, f"windows job: AssignProcessToJobObject failed ({ctypes.get_last_error()})"
    return job, "windows job: memory-capped, killed with the engine"


def close_windows_job(job) -> None:
    import ctypes
    from ctypes import wintypes

    k32 = ctypes.WinDLL("kernel32", use_last_error=True)
    k32.CloseHandle.argtypes = (wintypes.HANDLE,)
    k32.CloseHandle(job)


def harden(sock_fd: int, mem_bytes: int) -> str:
    """Drop privileges before any user code runs. Returns a one-line summary of
    what was enforced (the engine logs it for the record)."""
    notes = []
    if sys.platform == "win32":
        # The memory cap and kill-with-the-engine come from the job object the
        # engine put this process in (windows_job); the rest is done here.
        os.environ.clear()  # no secrets from the engine's environment
        _drop_modules()
        return "windows: environment cleared, modules dropped"

    # POSIX
    _close_inherited_fds(keep={0, 1, 2, sock_fd})
    try:
        _set_rlimits(mem_bytes)
        notes.append("rlimits: mem/fsize=0/nofile")
    except Exception as e:  # noqa: BLE001
        notes.append(f"rlimits: not applied ({e})")

    # Die if the engine dies, so a killed engine never leaves a worker behind.
    try:
        import ctypes
        _PR_SET_PDEATHSIG = 1
        import signal
        ctypes.CDLL(None).prctl(_PR_SET_PDEATHSIG, signal.SIGKILL, 0, 0, 0)
    except Exception:  # noqa: BLE001
        pass

    os.environ.clear()  # no secrets from the engine's environment

    if sys.platform.startswith("linux"):
        try:
            notes.append(_apply_landlock(notes))
        except Exception as e:  # noqa: BLE001
            notes.append(f"landlock: not applied ({e})")

    _drop_modules()
    return "; ".join(notes)


def _drop_modules() -> None:
    for name in _DROP_MODULES:
        sys.modules.pop(name, None)
        # Block a later import from succeeding cheaply.
        sys.modules[name] = None  # type: ignore[assignment]


# ---- the run loop ------------------------------------------------------------

class _Script:
    __slots__ = ("label", "fn", "input_keys", "params", "state")

    def __init__(self, label, fn, input_keys, params):
        self.label = label
        self.fn = fn
        self.input_keys = input_keys
        self.params = params
        self.state: dict = {}


def _compile(scripting, spec: dict, trusted: bool):
    """Return (script, error). ``trusted`` bypasses the AST allow-list and runs
    the code with real builtins — used only to prove the OS-level isolation
    holds on its own; ordinary runs always go through the checked path."""
    label = spec["label"]
    code = spec["code"]
    if trusted:
        import builtins
        namespace: dict = {"__builtins__": builtins.__dict__}
        try:
            exec(compile(code, f"<trusted:{label}>", "exec"), namespace)  # noqa: S102
        except Exception as e:  # noqa: BLE001
            return None, f"Script '{label}' failed to compile: {e}"
        fn = namespace.get("step")
        if not callable(fn):
            return None, f"Script '{label}' must define step()."
    else:
        try:
            fn = scripting.compile_script(code, label)
        except scripting.ScriptError as e:
            return None, str(e)
    return _Script(label, fn, spec.get("input_keys", []), spec.get("params", {})), None


def _run_step(scripting, script: _Script, t: float, dt: float,
              inputs: dict) -> dict:
    """Call step() and coerce its result exactly as scripting.run_script does,
    but without the in-process wall-clock tracer: the engine enforces the time
    limit from outside by killing the worker, which also stops the C-level
    loops the tracer never could."""
    try:
        out = script.fn(t, dt, inputs, script.state, dict(script.params))
    except Exception as e:  # noqa: BLE001
        raise scripting.ScriptError(
            f"Script '{script.label}' raised at t = {t:g} s: {e}") from e
    if out is None:
        return {}
    if not isinstance(out, dict):
        raise scripting.ScriptError(
            f"Script '{script.label}' must return a dict of output values, "
            f"got {type(out).__name__}.")
    result: dict[str, float] = {}
    for key, value in out.items():
        try:
            result[str(key)] = float(value)
        except (TypeError, ValueError):
            raise scripting.ScriptError(
                f"Script '{script.label}' output '{key}' is not numeric "
                f"({value!r}).") from None
    return result


def worker_main(sock, mem_bytes: int = DEFAULT_MEM_BYTES) -> None:
    """Entry point of the sandbox worker process (spawned by the engine)."""
    import json

    sock_fd = sock.fileno()
    # Busy-waiting between requests keeps this process on a core for the whole
    # run: about 20 % faster for a scripted run going flat out, for a second
    # busy core (a paced run sleeps between recorded steps, so it spins little).
    # On a machine with fewer than 4 cores that core is needed by the engine and
    # the app's window, so there the worker blocks instead.
    spin = (os.cpu_count() or 1) >= 4
    summary = harden(sock_fd, mem_bytes)
    try:
        sys.stderr.write(f"[sandbox] {summary}\n")
        sys.stderr.flush()
    except Exception:  # noqa: BLE001
        pass

    # Imported after hardening: everything it needs (ast, dis, math, …) is
    # loaded here, so nothing has to be opened from disk once Landlock is on.
    from . import scripting

    conn = Conn(sock, spin=spin)
    scripts: list[_Script] = []
    try:
        while True:
            body = conn.recv()
            kind = body[:1]
            if kind == MSG_STEP:
                idx, t, dt, values = _unpack_step(memoryview(body))
                script = scripts[idx]
                inputs = dict(zip(script.input_keys, values))
                try:
                    out = _run_step(scripting, script, t, dt, inputs)
                except scripting.ScriptError as e:
                    conn.send(MSG_STEP_ERR + str(e).encode("utf-8"))
                else:
                    conn.send(_pack_outputs(out))
            elif kind == MSG_QUIT:
                return
            elif kind == MSG_INIT:
                payload = json.loads(body[1:].decode("utf-8"))
                scripting.TIME_LIMIT_S = float(payload.get("time_limit",
                                                            scripting.TIME_LIMIT_S))
                trusted = bool(payload.get("trusted", False))
                errors = []
                scripts = []
                for spec in payload["scripts"]:
                    script, err = _compile(scripting, spec, trusted)
                    errors.append(err)
                    scripts.append(script)  # None on error; init reply stops the run
                conn.send(MSG_INIT + json.dumps({"errors": errors}).encode("utf-8"))
            elif kind == MSG_PARAM:
                payload = json.loads(body[1:].decode("utf-8"))
                scripts[payload["idx"]].params = payload["params"]
    except (EOFError, ConnectionError, BrokenPipeError):
        return
    finally:
        try:
            sock.close()
        except Exception:  # noqa: BLE001
            pass
