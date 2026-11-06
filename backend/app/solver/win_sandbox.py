"""Windows AppContainer worker launch for a strict agent run (step 6 (b), PLT-35).

When an AI agent runs a project on Windows, its Script and FMU workers run
inside a low-privilege AppContainer (LPAC) with **no capabilities**: no
network, and file access only to the folders whose ACL grants the container's
SID (the read-only runtime the worker's Python needs, and — for an FMU — its
unpacked folder). Nothing else of the user's is reachable, and nothing is
writable.

A capability-less AppContainer cannot use the loopback TCP socketpair that
Windows ``multiprocessing``/``socket`` use, so the engine<->worker channel is
an **inherited anonymous pipe pair** (proven to cross the boundary). The worker
started there finds itself confined (``TokenIsAppContainer``), so the engine's
strict check passes and the project's code runs; where the AppContainer cannot
be set up, :func:`app.solver.sandbox.why_no_strict_sandbox` refuses the run
with the stable code instead — never a weaker fall-back.

Only the documented Windows APIs are used, through ``ctypes`` (no admin, no new
dependency): ``CreateAppContainerProfile``, ``SECURITY_CAPABILITIES`` with no
capabilities, ``PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES`` and
``…_HANDLE_LIST`` on ``CreateProcessW``, and ``icacls`` for the ACL grants
(removed, and the profile deleted, when the worker closes). The job object
(``sandbox_worker.windows_job``) still caps memory and kills the worker with
the engine.

Runs at once. ``icacls`` reads each file's ACL, changes it and writes it back,
so two grants or removals at once on the shared runtime lose each other's
entries, and a worker then dies at start-up with STATUS_ACCESS_DENIED (three
runs at once on Windows 11: one of three started). Every ACL change and every
launch therefore holds one named mutex, shared by every LightSim process of
the user's session (:func:`_serialised`): the MCP server runs tool calls on
several threads, and an agent may start several ``lightsim run`` at once.

In the frozen (PyInstaller) engine the worker is the engine executable again,
started with :data:`WORKER_ARG` (``run_backend.py`` hands it to
:func:`child_main`), since a frozen engine has no ``python -c``.
"""
from __future__ import annotations

import contextlib
import os
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Optional

from . import sandbox_worker as _sw

_WIN = sys.platform == "win32"

#: The first argument that starts the frozen engine as an AppContainer worker
#: (run_backend.py) instead of the server or the command-line tool.
WORKER_ARG = "--lightsim-appcontainer-worker"

#: What the source worker runs with ``python -I -c``: set the sys.path it was
#: given (base64, then the separator), then serve.
_BOOT = ("import sys,base64;"
         "sys.path[:0]=base64.b64decode(sys.argv[3]).decode().split(sys.argv[4]);"
         "from app.solver.win_sandbox import child_main;"
         "child_main(sys.argv[1:3])")

if _WIN:
    import ctypes
    import msvcrt
    from ctypes import wintypes

    _userenv = ctypes.WinDLL("userenv", use_last_error=True)
    _k32 = ctypes.WinDLL("kernel32", use_last_error=True)
    _adv = ctypes.WinDLL("advapi32", use_last_error=True)

    PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES = 0x00020009
    PROC_THREAD_ATTRIBUTE_HANDLE_LIST = 0x00020002
    EXTENDED_STARTUPINFO_PRESENT = 0x00080000
    CREATE_NO_WINDOW = 0x08000000
    STARTF_USESTDHANDLES = 0x00000100
    HANDLE_FLAG_INHERIT = 0x00000001
    GENERIC_WRITE = 0x40000000
    FILE_SHARE_READ = 0x1
    FILE_SHARE_WRITE = 0x2
    CREATE_ALWAYS = 2
    FILE_ATTRIBUTE_TEMPORARY = 0x100
    FILE_FLAG_DELETE_ON_CLOSE = 0x04000000
    INVALID_HANDLE_VALUE = ctypes.c_void_p(-1).value
    STILL_ACTIVE = 259
    WAIT_OBJECT_0 = 0x0
    WAIT_ABANDONED = 0x80
    _k32.CreateMutexW.restype = wintypes.HANDLE
    _k32.CreateMutexW.argtypes = (ctypes.c_void_p, wintypes.BOOL, wintypes.LPCWSTR)

    class SECURITY_CAPABILITIES(ctypes.Structure):
        _fields_ = [("AppContainerSid", ctypes.c_void_p), ("Capabilities", ctypes.c_void_p),
                    ("CapabilityCount", wintypes.DWORD), ("Reserved", wintypes.DWORD)]

    class STARTUPINFOW(ctypes.Structure):
        _fields_ = [("cb", wintypes.DWORD), ("lpReserved", wintypes.LPWSTR),
                    ("lpDesktop", wintypes.LPWSTR), ("lpTitle", wintypes.LPWSTR),
                    ("dwX", wintypes.DWORD), ("dwY", wintypes.DWORD), ("dwXSize", wintypes.DWORD),
                    ("dwYSize", wintypes.DWORD), ("dwXCountChars", wintypes.DWORD),
                    ("dwYCountChars", wintypes.DWORD), ("dwFillAttribute", wintypes.DWORD),
                    ("dwFlags", wintypes.DWORD), ("wShowWindow", wintypes.WORD),
                    ("cbReserved2", wintypes.WORD), ("lpReserved2", ctypes.c_void_p),
                    ("hStdInput", wintypes.HANDLE), ("hStdOutput", wintypes.HANDLE),
                    ("hStdError", wintypes.HANDLE)]

    class STARTUPINFOEXW(ctypes.Structure):
        _fields_ = [("StartupInfo", STARTUPINFOW), ("lpAttributeList", ctypes.c_void_p)]

    class PROCESS_INFORMATION(ctypes.Structure):
        _fields_ = [("hProcess", wintypes.HANDLE), ("hThread", wintypes.HANDLE),
                    ("dwProcessId", wintypes.DWORD), ("dwThreadId", wintypes.DWORD)]

    class SECURITY_ATTRIBUTES(ctypes.Structure):
        _fields_ = [("nLength", wintypes.DWORD), ("lpSecurityDescriptor", ctypes.c_void_p),
                    ("bInheritHandle", wintypes.BOOL)]


class SandboxSetupError(Exception):
    """The AppContainer could not be set up; the caller refuses the run with
    the stable code rather than run it less safely."""


def _sid_string(sid) -> str:
    _adv.ConvertSidToStringSidW.argtypes = (ctypes.c_void_p, ctypes.POINTER(wintypes.LPWSTR))
    out = wintypes.LPWSTR()
    if not _adv.ConvertSidToStringSidW(sid, ctypes.byref(out)):
        raise SandboxSetupError("ConvertSidToStringSid failed")
    try:
        return out.value
    finally:
        _k32.LocalFree(out)


def _create_profile(name: str):
    """(sid pointer, sid string) of a fresh per-run AppContainer profile."""
    _userenv.DeleteAppContainerProfile(name)  # a leftover from a crash
    sid = ctypes.c_void_p()
    _userenv.CreateAppContainerProfile.restype = ctypes.c_long
    hr = _userenv.CreateAppContainerProfile(name, name, "LightSim agent sandbox",
                                            None, 0, ctypes.byref(sid))
    if hr != 0 or not sid.value:
        raise SandboxSetupError(f"CreateAppContainerProfile failed (hr=0x{hr & 0xFFFFFFFF:08X})")
    return sid, _sid_string(sid.value)


def _delete_profile(name: str) -> None:
    try:
        _userenv.DeleteAppContainerProfile(name)
    except Exception:  # noqa: BLE001
        pass


#: Held around every ACL change and launch, by every LightSim process of this
#: user's session (see the module's "Runs at once").
_SETUP_MUTEX = "Local\\LightSim-AppContainer-Setup"
_SETUP_WAIT_MS = 10 * 60 * 1000


@contextlib.contextmanager
def _serialised():
    """Hold the set-up mutex (threads and processes alike). A mutex whose last
    owner died is handed on (WAIT_ABANDONED), so a crashed run cannot block
    the next. ponytail: one lock for every set-up and tear-down, so runs queue
    for each other's ``icacls``; granting the runtime once per process would
    end the queue (and the seconds each grant takes)."""
    h = _k32.CreateMutexW(None, False, _SETUP_MUTEX)
    if not h:
        raise SandboxSetupError(f"CreateMutexW failed ({ctypes.get_last_error()})")
    try:
        got = _k32.WaitForSingleObject(wintypes.HANDLE(h), _SETUP_WAIT_MS)
        if got not in (WAIT_OBJECT_0, WAIT_ABANDONED):
            raise SandboxSetupError("another run's sandbox set-up did not finish within 10 minutes")
        try:
            yield
        finally:
            _k32.ReleaseMutex(wintypes.HANDLE(h))
    finally:
        _k32.CloseHandle(wintypes.HANDLE(h))


def _icacls(path: str, args: list[str]) -> bool:
    try:
        r = subprocess.run(["icacls", path, *args], capture_output=True, text=True,
                           creationflags=CREATE_NO_WINDOW)
        return r.returncode == 0
    except OSError:
        return False


def _grant(path: str, sid_str: str) -> bool:
    return _icacls(path, ["/grant", f"*{sid_str}:(OI)(CI)(RX)", "/T", "/C", "/Q"])


def _revoke(path: str, sid_str: str) -> None:
    _icacls(path, ["/remove:g", f"*{sid_str}", "/T", "/C", "/Q"])


def _base_python() -> str:
    """The real interpreter to launch, not the venv redirector: a venv's
    ``python.exe`` is a stub that re-execs the base interpreter, and that
    re-exec fails inside the AppContainer, so we launch the base one directly
    and set the child's ``sys.path`` ourselves."""
    return getattr(sys, "_base_executable", None) or sys.executable


def _backend_dir() -> str:
    """The folder that holds the ``app`` package (so the child can import it)."""
    return str(Path(__file__).resolve().parents[2])


def _site_dirs() -> list[str]:
    return [p for p in sys.path if p and ("site-packages" in p.lower()
                                          or "dist-packages" in p.lower())]


def _runtime_dirs(kind: str) -> list[str]:
    """Folders the worker's Python must read and load code from after the
    container is on. A Script needs only the interpreter and the backend
    package; an FMU also needs the site-packages that hold FMPy and NumPy. A
    frozen build ships its runtime inside the app folder (reachable by app
    packages already), so only that folder is granted."""
    if getattr(sys, "frozen", False):
        dirs = {str(Path(sys.executable).resolve().parent)}
        base = getattr(sys, "_MEIPASS", None)
        if base:
            dirs.add(str(Path(base).resolve()))
        return sorted(dirs)
    dirs = {str(Path(sys.base_prefix).resolve()), _backend_dir()}
    if kind == "fmu":
        dirs.add(str(Path(sys.prefix).resolve()))
        dirs.update(str(Path(p).resolve()) for p in _site_dirs())
    return sorted(dirs)


def _child_syspath(kind: str) -> str:
    """The sys.path the child sets before importing, base64-encoded so it
    crosses the command line with no quoting trouble (paths with spaces,
    backslashes)."""
    import base64
    paths = [_backend_dir(), *( _site_dirs() if kind == "fmu" else [])]
    return base64.b64encode(os.pathsep.join(paths).encode("utf-8")).decode("ascii")


def worker_argv(kind: str, mem_bytes: int) -> list[str]:
    """The worker's command line. From source: the base interpreter with
    ``-I -c`` and the child's sys.path. In the frozen engine: the engine
    executable with :data:`WORKER_ARG`, which ``run_backend.py`` hands to
    :func:`child_main` before anything else (a frozen engine has no ``-c``:
    it would take ``-I`` for one of the server's options and stop)."""
    if getattr(sys, "frozen", False):
        return [sys.executable, WORKER_ARG, kind, str(mem_bytes)]
    return [_base_python(), "-I", "-c", _BOOT, kind, str(mem_bytes), _child_syspath(kind), os.pathsep]


class _Proc:
    """A CreateProcessW child, with the subset of multiprocessing.Process the
    sandboxes use (kill/join/is_alive); ``sentinel`` is its process handle, for
    the job object."""

    def __init__(self, h_process: int, pid: int):
        self._h = h_process
        self.pid = pid
        self.sentinel = h_process

    def start(self) -> None:  # already started by CreateProcessW
        pass

    def is_alive(self) -> bool:
        code = wintypes.DWORD()
        if not _k32.GetExitCodeProcess(wintypes.HANDLE(self._h), ctypes.byref(code)):
            return False
        return code.value == STILL_ACTIVE

    def join(self, timeout: Optional[float] = None) -> None:
        ms = 0xFFFFFFFF if timeout is None else int(max(0.0, timeout) * 1000)
        _k32.WaitForSingleObject(wintypes.HANDLE(self._h), ms)

    def kill(self) -> None:
        try:
            _k32.TerminateProcess(wintypes.HANDLE(self._h), 1)
        except Exception:  # noqa: BLE001
            pass

    def close_handle(self) -> None:
        try:
            _k32.CloseHandle(wintypes.HANDLE(self._h))
        except Exception:  # noqa: BLE001
            pass


class AppContainerWorker:
    """Engine side of an AppContainer worker: a :class:`sandbox_worker.Conn`
    over the inherited pipe (``conn``), a process wrapper (``proc``), the job
    handle (``job``), and ``close`` to tear the container down."""

    def __init__(self, conn, proc: _Proc, job, profile: str, sid_str: str,
                 granted: list[str], stderr_path: str):
        self.conn = conn
        self.proc = proc
        self.job = job
        self._profile = profile
        self._sid_str = sid_str
        self._granted = granted
        self._stderr_path = stderr_path
        self._closed = False

    def stderr_text(self) -> str:
        try:
            return Path(self._stderr_path).read_text(encoding="utf-8", errors="replace")
        except OSError:
            return ""

    def close(self) -> None:
        if self._closed:
            return
        self._closed = True
        try:
            self.conn.sock.close()
        except Exception:  # noqa: BLE001
            pass
        self.proc.close_handle()
        try:
            with _serialised():
                for path in self._granted:
                    _revoke(path, self._sid_str)
        except SandboxSetupError:
            pass  # an entry left for a deleted profile's SID grants nobody anything
        _delete_profile(self._profile)
        try:
            os.unlink(self._stderr_path)
        except OSError:
            pass


def _pipe(worker_end_is_read: bool):
    """A one-way anonymous pipe. Returns (worker_handle, engine_fd): the
    worker's end is left inheritable (it crosses into the container), the
    engine's end is made non-inheritable and handed back as a C file
    descriptor."""
    sa = SECURITY_ATTRIBUTES(ctypes.sizeof(SECURITY_ATTRIBUTES), None, True)
    r = wintypes.HANDLE()
    w = wintypes.HANDLE()
    if not _k32.CreatePipe(ctypes.byref(r), ctypes.byref(w), ctypes.byref(sa), 0):
        raise SandboxSetupError(f"CreatePipe failed ({ctypes.get_last_error()})")
    worker_h = r.value if worker_end_is_read else w.value
    engine_h = w.value if worker_end_is_read else r.value
    # the engine keeps its end private (not inherited by the child)
    if not _k32.SetHandleInformation(wintypes.HANDLE(engine_h), HANDLE_FLAG_INHERIT, 0):
        raise SandboxSetupError(f"SetHandleInformation failed ({ctypes.get_last_error()})")
    flags = os.O_RDONLY if not worker_end_is_read else os.O_WRONLY
    engine_fd = msvcrt.open_osfhandle(engine_h, flags | os.O_BINARY)
    return worker_h, engine_fd


def _open_stderr_file() -> tuple[int, str]:
    """An inheritable handle to a temp file for the child's stderr, so a
    start-up failure (a traceback) can be read back. Deleted on close."""
    path = os.path.join(tempfile.gettempdir(), f"lightsim-ac-{os.getpid()}-{_token()}.err")
    sa = SECURITY_ATTRIBUTES(ctypes.sizeof(SECURITY_ATTRIBUTES), None, True)
    _k32.CreateFileW.restype = wintypes.HANDLE
    h = _k32.CreateFileW(path, GENERIC_WRITE, FILE_SHARE_READ | FILE_SHARE_WRITE,
                         ctypes.byref(sa), CREATE_ALWAYS, FILE_ATTRIBUTE_TEMPORARY, None)
    if h == INVALID_HANDLE_VALUE or not h:
        raise SandboxSetupError(f"CreateFileW (stderr) failed ({ctypes.get_last_error()})")
    return h, path


def _token() -> str:
    import secrets
    return secrets.token_hex(4)


def launch(kind: str, mem_bytes: int, project_dirs: tuple[str, ...] = ()) -> AppContainerWorker:
    """Start a ``kind`` ("script" or "fmu") worker inside a fresh AppContainer
    and return an :class:`AppContainerWorker`. ``project_dirs`` are the extra
    folders the worker may read (an FMU's unpacked folder). Raises
    :class:`SandboxSetupError` on any failure, so the caller refuses the run."""
    if not _WIN:
        raise SandboxSetupError("the AppContainer sandbox is Windows-only")
    with _serialised():
        return _launch(kind, mem_bytes, project_dirs)


def _launch(kind: str, mem_bytes: int, project_dirs: tuple[str, ...]) -> AppContainerWorker:
    profile = f"lightsim-agent-{os.getpid()}-{_token()}"
    sid, sid_str = _create_profile(profile)
    granted: list[str] = []
    worker_in = worker_out = stderr_h = None
    attr_buf = None
    try:
        for d in [*_runtime_dirs(kind), *project_dirs]:
            if d and os.path.isdir(d) and _grant(d, sid_str):
                granted.append(d)
        worker_in, engine_write_fd = _pipe(worker_end_is_read=True)    # engine -> worker
        worker_out, engine_read_fd = _pipe(worker_end_is_read=False)   # worker -> engine
        stderr_h, stderr_path = _open_stderr_file()

        # the attribute list: the security capabilities (no caps) + exactly the
        # three handles the child may inherit (its two pipe ends and stderr)
        count = 2
        size = ctypes.c_size_t(0)
        _k32.InitializeProcThreadAttributeList(None, count, 0, ctypes.byref(size))
        attr_buf = (ctypes.c_byte * size.value)()
        attr = ctypes.cast(attr_buf, ctypes.c_void_p)
        if not _k32.InitializeProcThreadAttributeList(attr, count, 0, ctypes.byref(size)):
            raise SandboxSetupError(f"InitializeProcThreadAttributeList failed ({ctypes.get_last_error()})")
        caps = SECURITY_CAPABILITIES(sid.value, None, 0, 0)
        if not _k32.UpdateProcThreadAttribute(attr, 0, PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES,
                                              ctypes.byref(caps), ctypes.sizeof(caps), None, None):
            raise SandboxSetupError(f"UpdateProcThreadAttribute (caps) failed ({ctypes.get_last_error()})")
        handles = (ctypes.c_void_p * 3)(worker_in, worker_out, stderr_h)
        if not _k32.UpdateProcThreadAttribute(attr, 0, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
                                              handles, ctypes.sizeof(handles), None, None):
            raise SandboxSetupError(f"UpdateProcThreadAttribute (handles) failed ({ctypes.get_last_error()})")

        si = STARTUPINFOEXW()
        si.StartupInfo.cb = ctypes.sizeof(STARTUPINFOEXW)
        si.StartupInfo.dwFlags = STARTF_USESTDHANDLES
        si.StartupInfo.hStdInput = worker_in
        si.StartupInfo.hStdOutput = worker_out
        si.StartupInfo.hStdError = stderr_h
        si.lpAttributeList = attr
        pi = PROCESS_INFORMATION()
        cmd = ctypes.create_unicode_buffer(subprocess.list2cmdline(worker_argv(kind, mem_bytes)))
        ok = _k32.CreateProcessW(None, cmd, None, None, True,
                                 EXTENDED_STARTUPINFO_PRESENT | CREATE_NO_WINDOW,
                                 None, None, ctypes.byref(si), ctypes.byref(pi))
        if not ok:
            raise SandboxSetupError(f"CreateProcessW failed ({ctypes.get_last_error()})")
        _k32.CloseHandle(pi.hThread)
        proc = _Proc(pi.hProcess, pi.dwProcessId)
        # the child owns its ends now; the engine keeps only its fds
        for h in (worker_in, worker_out, stderr_h):
            _k32.CloseHandle(wintypes.HANDLE(h))
        worker_in = worker_out = stderr_h = None
        try:
            job, _note = _sw.windows_job(pi.hProcess, mem_bytes)
        except Exception:  # noqa: BLE001
            job = None
        conn = _sw.Conn(_sw.PipeEnd(engine_read_fd, engine_write_fd))
        return AppContainerWorker(conn, proc, job, profile, sid_str, granted, stderr_path)
    except BaseException:
        for h in (worker_in, worker_out, stderr_h):
            if h:
                _k32.CloseHandle(wintypes.HANDLE(h))
        for path in granted:
            _revoke(path, sid_str)
        _delete_profile(profile)
        raise


def child_main(argv: list[str]) -> int:
    """Entry point of the worker inside the AppContainer; ``argv`` is
    ``[kind, mem_bytes]``. Its stdin and stdout are the inherited pipe ends; it
    hardens, then serves over them exactly as the socket worker does."""
    kind = argv[0]
    mem_bytes = int(argv[1])
    read_fd = sys.stdin.fileno()
    write_fd = sys.stdout.fileno()
    try:
        msvcrt.setmode(read_fd, os.O_BINARY)
        msvcrt.setmode(write_fd, os.O_BINARY)
    except (OSError, AttributeError):
        pass
    conn = _sw.Conn(_sw.PipeEnd(read_fd, write_fd))
    if kind == "fmu":
        from ..fmu import worker as fw
        fw._import_fmpy()
        summary, confined = _sw.harden(-1, mem_bytes, read_paths=(), drop_modules=False)
        _sw._log_summary(summary)
        fw.serve(conn, confined)
    else:
        summary, confined = _sw.harden(-1, mem_bytes)
        _sw._log_summary(summary)
        _sw.serve(conn, confined)
    return 0


_PROBE: Optional[bool] = None


def appcontainer_available() -> bool:
    """True when a confined AppContainer worker can really be launched here —
    the profile, the ACL grants and the launch all work, and the worker reports
    itself confined. Run once and cached, so the strict gate
    (:func:`why_no_strict_sandbox`) can trust it. Any failure returns False, so
    the run is refused rather than run unconfined."""
    global _PROBE
    if _PROBE is not None:
        return _PROBE
    _PROBE = False
    if not _WIN:
        return _PROBE
    worker = None
    try:
        import json
        worker = launch("script", _sw.DEFAULT_MEM_BYTES)
        payload = {"time_limit": 2.0, "trusted": False, "strict": True, "scripts": []}
        worker.conn.send(_sw.MSG_INIT + json.dumps(payload).encode("utf-8"))
        # a short watchdog: kill the worker if the reply does not come
        import threading
        timer = threading.Timer(20.0, worker.proc.kill)
        timer.start()
        try:
            reply = json.loads(worker.conn.recv()[1:].decode("utf-8"))
        finally:
            timer.cancel()
        _PROBE = bool(reply.get("confined"))
    except Exception:  # noqa: BLE001
        _PROBE = False
    finally:
        if worker is not None:
            try:
                worker.conn.send(_sw.MSG_QUIT)
            except Exception:  # noqa: BLE001
                pass
            worker.proc.kill()
            worker.close()
    return _PROBE
