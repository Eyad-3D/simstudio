"""The strict sandbox for an AI agent's run (step 6, item 1(b)/(c)).

When an agent runs a project, its Script blocks and FMUs must run OS-confined
— no network, file access to the project only — and the run is refused, with
a stable code, when that confinement cannot be set up: never a silent
fall-back to the weaker worker. A person's run in the app keeps today's
behaviour. The Linux path, the gate and the refusal are proven here on any
platform; the Windows AppContainer tests at the end run in the "Backend on
Windows" CI job.
"""
from __future__ import annotations

import os
import sys
import tempfile

import pytest
from helpers import bev_axle, sig_port

from app.schemas import ElementInstance
from app.solver import ScriptError, simulate
from app.solver import sandbox as sandbox_mod
from app.solver import sandbox_worker as sw
from app.solver.sandbox import (
    SANDBOX_UNAVAILABLE,
    STRICT,
    ScriptSandbox,
    ScriptSpec,
    why_no_strict_sandbox,
)

landlock = pytest.mark.skipif(
    not sys.platform.startswith("linux") or sw.landlock_abi() < 0,
    reason="the strict Linux sandbox needs Landlock (Linux >= 5.13)")

try:
    import lightsim_engine  # noqa: F401
    NEW_MISSING = None
except ImportError as e:
    NEW_MISSING = f"the compiled module lightsim_engine is not installed ({e})"
needs_engine = pytest.mark.skipif(
    NEW_MISSING is not None and os.environ.get("LIGHTSIM_REQUIRE_NEW_ENGINE", "").strip() != "1",
    reason=NEW_MISSING or "")


@pytest.fixture(params=["today", pytest.param("new", marks=needs_engine)])
def engine(request, monkeypatch):
    """Each engine's path: today's, and the new one (LIGHTSIM_ENGINE=new,
    strict, so nothing falls back to today's and hides what it did)."""
    from app.solver import new_engine
    monkeypatch.setenv(new_engine.ENV_VAR, request.param)
    if request.param == "new":
        monkeypatch.setenv(new_engine.STRICT_VAR, "1")
    else:
        monkeypatch.delenv(new_engine.STRICT_VAR, raising=False)
    return request.param


@pytest.fixture
def no_sandbox(monkeypatch):
    """The strict sandbox cannot be set up here. Each engine's path imports
    the gate by name, so both are told."""
    from app.solver import domains, new_engine
    for mod in (domains, new_engine):
        monkeypatch.setattr(mod, "why_no_strict_sandbox",
                            lambda *a: f"no sandbox here [{SANDBOX_UNAVAILABLE}]")


@pytest.fixture
def built(monkeypatch):
    """``strict`` of each Script worker a run starts, on either engine's path."""
    from app.solver import domains, new_engine
    seen: list = []

    class Recorded(ScriptSandbox):
        def __init__(self, specs, *a, **k):
            seen.append(k.get("strict", False))
            super().__init__(specs, *a, **k)
    for mod in (domains, new_engine):
        monkeypatch.setattr(mod, "ScriptSandbox", Recorded)
    return seen


@pytest.fixture
def asked(monkeypatch):
    """The Script approval checks a run makes (app/script_trust.py)."""
    from app import script_trust
    seen: list = []
    real = script_trust.check

    def check(scripts):
        seen.append(len(scripts))
        return real(scripts)
    monkeypatch.setattr(script_trust, "check", check)
    return seen


def _on(engine: str, result) -> None:
    """The run went down ``engine``'s path (the new one says so first)."""
    first = result.messages[0].text if result.messages else ""
    assert first.startswith("Run on the new engine") == (engine == "new"), first


def _scripted(code: str):
    proj = bev_axle()
    proj.systems[0].elements.append(ElementInstance(
        id="scr", componentDefId="signal.script", label="Probe",
        position={"x": 0, "y": 0}, parameterOverrides={"code": code},
        dynamicPorts=[sig_port("cmd_in", "input"), sig_port("cmd_out", "output")],
    ))
    return proj


STEP = "def step(t, dt, inputs, state, params):\n    return {'cmd_out': 0.0}\n"


# -- the gate: when the strict sandbox cannot be set up ------------------------


def test_why_no_strict_sandbox_refuses_win32_when_the_appcontainer_is_unavailable(monkeypatch):
    from app.solver import win_sandbox
    monkeypatch.setattr(win_sandbox, "appcontainer_available", lambda: False)
    msg = why_no_strict_sandbox("win32")
    assert msg and SANDBOX_UNAVAILABLE in msg
    assert "AppContainer" in msg


def test_why_no_strict_sandbox_allows_win32_when_the_appcontainer_is_available(monkeypatch):
    from app.solver import win_sandbox
    monkeypatch.setattr(win_sandbox, "appcontainer_available", lambda: True)
    assert why_no_strict_sandbox("win32") is None


def test_why_no_strict_sandbox_refuses_linux_without_landlock(monkeypatch):
    monkeypatch.setattr(sw, "landlock_abi", lambda: -1)
    msg = why_no_strict_sandbox("linux")
    assert msg and SANDBOX_UNAVAILABLE in msg and "Landlock" in msg


@landlock
def test_why_no_strict_sandbox_allows_this_linux():
    assert why_no_strict_sandbox() is None


# -- the Windows gate's probe, as in a fresh engine process -------------------
# The gate probes the AppContainer once per engine process by starting a worker
# in one, which takes seconds. It used to note "no" before it probed, with no
# lock, so a run that came meanwhile (an MCP server's first run_case calls at
# once) was refused: "could not be set up on this computer" [sandbox_unavailable].
# A failed probe also stayed "no" for the life of the process. These drive the
# probe on any platform with a stand-in for the launch.


class _ConfinedWorker:
    """What the probe needs of a launched worker: it says it is confined."""

    def __init__(self):
        import types
        reply = sw.MSG_INIT + b'{"confined": true, "errors": []}'
        self.conn = types.SimpleNamespace(send=lambda body: None, recv=lambda: reply)
        self.proc = types.SimpleNamespace(kill=lambda: None)

    def close(self):
        pass


@pytest.fixture
def fresh_probe(monkeypatch):
    """win_sandbox as a fresh engine process on Windows sees it: nothing probed yet."""
    from app.solver import win_sandbox
    monkeypatch.setattr(win_sandbox, "_WIN", True)
    monkeypatch.setattr(win_sandbox, "_PROBE_OK", False)
    monkeypatch.setattr(win_sandbox, "_PROBES_DONE", 0)
    return win_sandbox


def _at_once(n, call):
    """``call()`` from ``n`` threads let go together; their answers in order."""
    import threading
    answers = [None] * n
    go = threading.Barrier(n)

    def one(i):
        go.wait()
        answers[i] = call()

    threads = [threading.Thread(target=one, args=(i,)) for i in range(n)]
    for t in threads:
        t.start()
    for t in threads:
        t.join(timeout=60)
    return answers


def test_strict_runs_at_once_in_a_fresh_engine_all_pass_the_windows_gate(fresh_probe, monkeypatch):
    import time
    launches = []

    def slow_launch(kind, mem_bytes, project_dirs=()):
        launches.append(kind)
        time.sleep(0.5)  # a worker starting in its container
        return _ConfinedWorker()

    monkeypatch.setattr(fresh_probe, "launch", slow_launch)
    assert _at_once(3, lambda: why_no_strict_sandbox("win32")) == [None, None, None]
    assert launches == ["script"], "one probe, its answer shared"
    assert why_no_strict_sandbox("win32") is None and launches == ["script"], "a yes is kept"


def test_a_failed_probe_is_shared_by_those_waiting_and_tried_again_later(fresh_probe, monkeypatch):
    import time
    launches = []

    def failing_launch(kind, mem_bytes, project_dirs=()):
        launches.append(kind)
        time.sleep(0.5)
        raise fresh_probe.SandboxSetupError("not this time")

    monkeypatch.setattr(fresh_probe, "launch", failing_launch)
    assert _at_once(3, fresh_probe.appcontainer_available) == [False, False, False]
    assert len(launches) == 1, "callers that waited for the probe take its answer"
    monkeypatch.setattr(fresh_probe, "launch", lambda *a, **k: _ConfinedWorker())
    assert fresh_probe.appcontainer_available() is True, "a no is not kept for the life of the process"


def test_the_probe_waits_for_its_worker_as_long_as_a_run_does(fresh_probe, monkeypatch):
    # its watchdog was 20 s, shorter than the 30 s start-up a run's worker is
    # given, so a probe while other runs' set-ups kept the disk busy said no
    import threading
    waits = []
    real_timer = threading.Timer

    def timer(seconds, fn):
        waits.append(seconds)
        return real_timer(seconds, fn)

    monkeypatch.setattr(fresh_probe.threading, "Timer", timer)
    monkeypatch.setattr(fresh_probe, "launch", lambda *a, **k: _ConfinedWorker())
    assert fresh_probe.appcontainer_available() is True
    # what a strict run with no scripts waits for its worker's first reply
    assert waits == [5.0 + sandbox_mod._AC_START_S]


def test_an_agent_script_run_is_refused_when_the_sandbox_is_unavailable(engine, no_sandbox, built):
    # the agent path (STRICT) on a platform where the sandbox cannot be set up:
    # refused with the code before any worker starts, on either engine
    token = STRICT.set(True)
    try:
        result = simulate(_scripted(STEP), "case")
    finally:
        STRICT.reset(token)
    assert result.status == "failed"
    assert SANDBOX_UNAVAILABLE in " ".join(m.text for m in result.messages)
    assert built == [], "no Script worker started"
    _on(engine, result)


def test_an_app_run_is_not_refused_when_strict_is_off(engine, no_sandbox, built, asked):
    # a person's run (STRICT unset) keeps today's behaviour even where the
    # strict sandbox would be unavailable: its Scripts are checked for the
    # user's approval and run in the ordinary (not strict) worker
    result = simulate(_scripted(STEP), "case")
    assert result.status in ("success", "warning"), [m.text for m in result.messages]
    assert asked, "the approval was checked"
    assert built and not any(built), built
    _on(engine, result)


@landlock
def test_an_agent_script_run_skips_approval_and_runs_in_a_strict_worker(engine, built, asked):
    # the sandbox, not a person's approval, is what makes an agent's run safe
    token = STRICT.set(True)
    try:
        result = simulate(_scripted(STEP), "case")
    finally:
        STRICT.reset(token)
    assert result.status in ("success", "warning"), [m.text for m in result.messages]
    assert asked == [], "no approval asked"
    assert built and all(built), built
    _on(engine, result)


# -- confinement is confirmed, not assumed ------------------------------------


@landlock
def test_a_strict_sandbox_builds_and_confines_on_this_linux():
    sb = ScriptSandbox([ScriptSpec("s", "Probe", STEP, [], {})], strict=True)
    try:
        assert sb.run("s", "Probe", 0.0, 0.01, {}, {}) == {"cmd_out": 0.0}
    finally:
        sb.close()


def test_a_strict_sandbox_refuses_when_the_worker_cannot_confine(monkeypatch):
    # belt-and-suspenders: the gate passed but the worker reports it could not
    # confine (e.g. restrict_self failed) — refuse, do not run less safely
    import json

    real_recv = sandbox_mod._w.Conn.recv

    def not_confined(self):
        body = real_recv(self)
        if body[:1] == sandbox_mod._w.MSG_INIT:
            payload = json.loads(body[1:].decode("utf-8"))
            payload["confined"] = False
            return sandbox_mod._w.MSG_INIT + json.dumps(payload).encode("utf-8")
        return body

    monkeypatch.setattr(sandbox_mod._w.Conn, "recv", not_confined)
    with pytest.raises(ScriptError, match=SANDBOX_UNAVAILABLE):
        ScriptSandbox([ScriptSpec("s", "Probe", STEP, [], {})], strict=True)


# -- a script that tries to escape is blocked, and the run says so ------------
# The in-process layer (test_script_safety) rejects open/socket/import at
# compile; the OS layer (test_script_sandbox, trusted=True) blocks them even
# when that is bypassed. Here: through the agent run, the attempt fails and
# the run reports it.


@pytest.mark.parametrize("attempt,needle", [
    ("open('/etc/hostname')", "open"),
    ("open('escape.txt', 'w')", "open"),
    ("__import__('socket')", "import"),
    ("__import__('subprocess')", "import"),
])
def test_an_agent_script_that_tries_to_escape_fails_and_the_run_says_so(attempt, needle):
    code = f"def step(t, dt, inputs, state, params):\n    {attempt}\n    return {{}}\n"
    token = STRICT.set(True)
    try:
        result = simulate(_scripted(code), "case")
    finally:
        STRICT.reset(token)
    assert result.status == "failed"
    text = " ".join(m.text for m in result.messages).lower()
    # where the strict sandbox is available (Linux with Landlock, or Windows
    # once the AppContainer lands) the script runs and the attempt is rejected;
    # where it is not, the run is refused before the script runs. Either way it
    # fails and the message says why — this keeps the test true on the
    # Backend-on-Windows job too.
    if why_no_strict_sandbox() is None:
        assert needle in text, text
    else:
        assert SANDBOX_UNAVAILABLE in text, text


@landlock
def test_a_strict_sandbox_blocks_a_file_open_at_the_os_layer():
    # The escape tests above go through the checked compile, so the in-process
    # allow-list already rejects open/import — they do not prove the OS layer.
    # trusted=True bypasses that allow-list, so Landlock is the only thing that
    # can stop the open; a strict sandbox must still block it (reaching the OS
    # layer, as the check asked).
    code = ("def step(t, dt, inputs, state, params):\n"
            "    open('/etc/hostname').read()\n"
            "    return {'cmd_out': 0.0}\n")
    sb = ScriptSandbox([ScriptSpec("s", "Probe", code, [], {})], strict=True, trusted=True)
    try:
        with pytest.raises(ScriptError):
            sb.run("s", "Probe", 0.0, 0.01, {}, {})
    finally:
        sb.close()


# -- confinement is settled before any of the project's code runs ------------
# The worker learns whether it is confined only after harden(); a strict run
# must send the scripts/FMU only to a confined worker, so none of the
# project's code runs in a worker that could not confine (e.g. Landlock
# present but restrict_self failed). These drive the worker directly, with
# harden() stubbed, so no Landlock/rlimits touch the test process.


def _drive_worker(target, args, init_body, monkeypatch, confined):
    import json as _json
    import socket as _socket
    import threading

    monkeypatch.setattr(sw, "harden", lambda *a, **k: ("test", confined))
    parent, child = _socket.socketpair()
    parent.settimeout(60)  # a worker that dies without a reply fails the test, not hangs it
    th = threading.Thread(target=target, args=(child, *args), daemon=True)
    th.start()
    try:
        conn = sw.Conn(parent)
        conn.send(init_body)
        reply = _json.loads(conn.recv()[1:].decode("utf-8"))
        conn.send(sw.MSG_QUIT)
    finally:
        parent.close()
        th.join(timeout=5)
    return reply


def test_a_strict_script_worker_runs_no_code_before_it_knows_it_is_confined(tmp_path, monkeypatch):
    import json as _json

    sentinel = tmp_path / "ran.txt"
    code = (f"open({str(sentinel)!r}, 'w').write('ran')\n\n"
            "def step(t, dt, inputs, state, params):\n    return {}\n")
    payload = {"time_limit": 2.0, "trusted": True, "strict": True,
               "scripts": [{"label": "P", "code": code, "input_keys": [], "params": {}}]}
    init = sw.MSG_INIT + _json.dumps(payload).encode("utf-8")
    reply = _drive_worker(sw.worker_main, (), init, monkeypatch, confined=False)
    assert reply["confined"] is False and reply["errors"] == []
    assert not sentinel.exists(), "no top-level script code ran in the unconfined strict worker"


def test_a_strict_script_worker_runs_code_once_it_is_confined(tmp_path, monkeypatch):
    import json as _json

    sentinel = tmp_path / "ran.txt"
    code = (f"open({str(sentinel)!r}, 'w').write('ran')\n\n"
            "def step(t, dt, inputs, state, params):\n    return {}\n")
    payload = {"time_limit": 2.0, "trusted": True, "strict": True,
               "scripts": [{"label": "P", "code": code, "input_keys": [], "params": {}}]}
    init = sw.MSG_INIT + _json.dumps(payload).encode("utf-8")
    reply = _drive_worker(sw.worker_main, (), init, monkeypatch, confined=True)
    assert reply["confined"] is True
    assert sentinel.read_text(encoding="utf-8") == "ran", "code runs once the worker is confined"


def test_a_strict_fmu_worker_loads_nothing_before_it_knows_it_is_confined(monkeypatch):
    import json as _json

    from app.fmu import worker as fw

    # the worker imports FMPy first; without the FMU pack it would stop before
    # its reply (and this test used to wait for that reply forever)
    pytest.importorskip("fmpy")

    spec = {"label": "X", "strict": True, "unzipDir": "/nonexistent",
            "libraryPath": "/nonexistent/x.so", "fmiVersion": "2.0", "guid": "g",
            "modelIdentifier": "X", "inputs": [], "outputs": [], "starts": []}
    init = fw.MSG_INIT + _json.dumps(spec).encode("utf-8")
    reply = _drive_worker(fw.worker_main, (fw.DEFAULT_MEM_BYTES, []), init, monkeypatch, confined=False)
    # a load attempt on the bogus FMU would set error; None proves none happened
    assert reply["confined"] is False and reply["error"] is None


# -- the example cars' scripts give the same results under the strict sandbox -


@landlock
def test_the_hybrid_example_runs_the_same_under_the_strict_sandbox():
    from app.storage import load_example

    hybrid = load_example("hybrid-car")
    case = next(c for c in hybrid.cases if "UDDS" in c.name or c.id == "case-udds")
    short = hybrid.model_copy(update={"cases": [c.model_copy(update={"duration": 5.0})
                                                for c in hybrid.cases]})
    plain = simulate(short, case.id)
    token = STRICT.set(True)
    try:
        strict = simulate(short, case.id)
    finally:
        STRICT.reset(token)
    assert strict.status == plain.status
    a = {s.key: s.value for s in plain.summary if s.key}
    b = {s.key: s.value for s in strict.summary if s.key}
    assert a and a.keys() == b.keys()
    for key in a:
        assert b[key] == pytest.approx(a[key], rel=1e-9, abs=1e-9), key


# -- the Windows AppContainer (step 6 (b)); these run in the Backend-on-Windows job
windows = pytest.mark.skipif(sys.platform != "win32", reason="the AppContainer sandbox is Windows-only")


@windows
def test_the_windows_appcontainer_is_available_on_this_machine():
    # a standard Windows machine supports per-user AppContainers with no admin;
    # this fails if the launch, the ACL grant or the pipe IPC is broken (a
    # broken sandbox would otherwise only make runs refuse, silently)
    from app.solver import win_sandbox
    assert win_sandbox.appcontainer_available() is True


@windows
def test_a_strict_script_run_is_confined_in_the_appcontainer():
    from app.solver import win_sandbox
    if not win_sandbox.appcontainer_available():
        pytest.skip("this machine cannot set up the AppContainer")
    # a trusted script (past the in-process allow-list) proves the OS layer:
    # a user file outside the project, a file write and a TCP connect are all
    # blocked by the AppContainer, and a plain script still runs.
    ok = ScriptSandbox([ScriptSpec("s", "P", STEP, [], {})], strict=True)
    try:
        assert ok.run("s", "P", 0.0, 0.01, {}, {}) == {"cmd_out": 0.0}
    finally:
        ok.close()

    import socket

    secret = os.path.join(tempfile.mkdtemp(prefix="lsim-sec-"), "s.txt")
    with open(secret, "w", encoding="utf-8") as f:
        f.write("SECRET")
    written = os.path.join(tempfile.gettempdir(), f"lsim-ac-escape-{os.getpid()}.txt")
    # a live listener: a connection to it succeeds unless the sandbox stops it
    # (a closed port would refuse it either way, proving nothing)
    listener = socket.socket()
    listener.bind(("127.0.0.1", 0))
    listener.listen(4)
    port = listener.getsockname()[1]
    # paths go in params, never baked into the script source (a Windows path's
    # backslashes never reach the compiler)
    attempts = [
        ("read a user file", "open(params['p']).read()", {"p": secret}, True),
        ("write a file", "open(params['p'], 'w').write('x')", {"p": written}, False),
        # (connect, not create_connection: the worker has no idna codec for getaddrinfo)
        ("open a TCP connection", "import socket\n    s = socket.socket(); s.settimeout(3)\n    "
         "s.connect(('127.0.0.1', int(params['port']))); s.close()", {"port": port}, True),
    ]
    try:
        for what, body, params, works_unconfined in attempts:
            code = f"def step(t, dt, inputs, state, params):\n    {body}\n    return {{'cmd_out': 0.0}}\n"
            if works_unconfined:  # the attempt itself is sound: the person's worker gets through
                plain = ScriptSandbox([ScriptSpec("e", "E", code, [], params)], trusted=True)
                try:
                    assert plain.run("e", "E", 0.0, 0.01, {}, params) == {"cmd_out": 0.0}, what
                finally:
                    plain.close()
            sb = ScriptSandbox([ScriptSpec("e", "E", code, [], params)], strict=True, trusted=True)
            try:
                with pytest.raises(ScriptError):
                    sb.run("e", "E", 0.0, 0.01, {}, params)
            finally:
                sb.close()
    finally:
        listener.close()
    assert open(secret, encoding="utf-8").read() == "SECRET"
    assert not os.path.exists(written), "the worker could not write a file"


#: one strict run in a process of its own: exit 0 when its worker started confined
_ONE_STRICT_RUN = (
    "from app.solver.sandbox import ScriptSandbox, ScriptSpec\n"
    f"sb = ScriptSandbox([ScriptSpec('s', 'P', {STEP!r}, [], {{}})], strict=True)\n"
    "assert sb.run('s', 'P', 0.0, 0.01, {}, {}) == {'cmd_out': 0.0}\n"
    "sb.close()\n")


@windows
def test_several_strict_runs_at_once_all_start_in_their_appcontainers():
    # The MCP server runs tool calls on several threads, and an agent may start
    # several `lightsim run` at once. icacls reads, changes and writes back each
    # file's ACL, so set-ups at once used to lose each other's grants on the
    # shared runtime: of three runs at once one started, the others' workers
    # died with STATUS_ACCESS_DENIED. One named mutex now holds every set-up and
    # tear-down, across threads and processes.
    import subprocess
    import threading
    from pathlib import Path

    from app.solver import win_sandbox
    if not win_sandbox.appcontainer_available():
        pytest.skip("this machine cannot set up the AppContainer")
    errors: list[str] = []

    def one() -> None:
        try:
            sb = ScriptSandbox([ScriptSpec("s", "P", STEP, [], {})], strict=True)
            try:
                assert sb.run("s", "P", 0.0, 0.01, {}, {}) == {"cmd_out": 0.0}
            finally:
                sb.close()
        except Exception as e:  # noqa: BLE001 — every failure is reported below
            errors.append(repr(e))

    threads = [threading.Thread(target=one) for _ in range(3)]
    for t in threads:
        t.start()
    for t in threads:
        t.join(timeout=600)
    assert not errors, errors
    backend = Path(__file__).resolve().parents[1]
    procs = [subprocess.Popen([sys.executable, "-c", _ONE_STRICT_RUN], cwd=backend,
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
             for _ in range(2)]
    outs = [p.communicate(timeout=600) for p in procs]
    assert [p.returncode for p in procs] == [0, 0], outs


def test_the_frozen_engine_starts_the_appcontainer_worker_it_is_given(monkeypatch):
    # In the frozen (PyInstaller) engine the worker is the engine executable
    # again. A frozen engine has no `python -c`: given `-I -c ...` its server
    # stops on an unknown option (exit 2), so every agent Script/FMU run of the
    # installed app was refused. The worker's command line there is the engine
    # with WORKER_ARG, which the frozen entry hands to child_main first.
    import runpy
    from pathlib import Path

    from app.solver import win_sandbox
    entry = str(Path(__file__).resolve().parents[1] / "run_backend.py")
    exe = r"C:\LightSim\resources\backend\lightsim-backend.exe"
    monkeypatch.setattr(sys, "frozen", True, raising=False)
    monkeypatch.setattr(sys, "executable", exe)
    argv = win_sandbox.worker_argv("fmu", 123)
    assert argv[:2] == [exe, win_sandbox.WORKER_ARG] and "-c" not in argv
    started: list[list[str]] = []
    monkeypatch.setattr(win_sandbox, "child_main", lambda a: started.append(a) or 0)
    monkeypatch.setattr(sys, "argv", argv)
    with pytest.raises(SystemExit) as done:
        runpy.run_path(entry, run_name="__main__")
    assert done.value.code == 0 and started == [["fmu", "123"]]

    # what the frozen engine did with the source worker's command line
    monkeypatch.setattr(sys, "argv", [exe, "-I", "-c", "pass", "script", "123"])
    with pytest.raises(SystemExit) as old:
        runpy.run_path(entry, run_name="__main__")
    assert old.value.code == 2 and started == [["fmu", "123"]]


#: a frozen engine to check (the "Backend on Windows" CI job freezes one)
FROZEN = os.environ.get("LIGHTSIM_FROZEN_ENGINE")


@pytest.mark.skipif(not FROZEN, reason="set LIGHTSIM_FROZEN_ENGINE to a frozen engine to check")
def test_the_frozen_engine_runs_a_new_script_in_its_sandbox(tmp_path):
    # The installed app's engine is frozen, and its strict worker is the engine
    # itself (above). A script nobody approved runs through its command line
    # only in the strict sandbox (with LIGHTSIM_STRICT_SANDBOX=0 it is refused
    # as not approved), so a success here is a worker that started confined.
    import json
    import subprocess
    from pathlib import Path

    backend = Path(__file__).resolve().parents[1]
    raw = json.loads((backend / "projects" / "hybrid-car.json").read_text(encoding="utf-8"))
    raw["cases"] = [{**raw["cases"][0], "duration": 5, "realtimeFactor": 0}]
    for el in (e for s in raw["systems"] for e in s["elements"] if e["componentDefId"] == "signal.script"):
        el["parameterOverrides"]["code"] = el["parameterOverrides"].get("code", "") + "\n# approved by nobody\n"
    proj = tmp_path / "car.json"
    proj.write_text(json.dumps(raw), encoding="utf-8")
    env = {k: v for k, v in os.environ.items() if k != "LIGHTSIM_STRICT_SANDBOX"}
    env.update(LIGHTSIM_DATA_DIR=str(tmp_path / "d"), LIGHTSIM_PROJECTS_DIR=str(tmp_path / "p"))
    command = [str(Path(FROZEN).resolve()), "run", str(proj), "--json", "--out", str(tmp_path / "r.mat")]
    done = subprocess.run(command, env=env, capture_output=True, text=True, encoding="utf-8", timeout=900)
    assert done.returncode == 0 and json.loads(done.stdout)["status"] == "success", done.stdout + done.stderr
    person = subprocess.run(command, env={**env, "LIGHTSIM_STRICT_SANDBOX": "0"}, capture_output=True,
                            text=True, encoding="utf-8", timeout=900)
    assert person.returncode == 2 and "has not been approved" in person.stdout, person.stdout + person.stderr


# -- the wheel: the engine is lightsim._app there, never `app` ----------------
# scripts/build-wheel.py ships the engine inside the lightsim package, so that
# installing it never puts a package called `app` on anyone's path. The
# AppContainer worker's boot line imported app.solver.win_sandbox, so on Windows
# every strict run from the wheel (since every `lightsim run` is strict, a
# person's `lightsim run hybrid-car` too) was refused [sandbox_unavailable].


@pytest.fixture(scope="module")
def wheel_site(tmp_path_factory):
    """The wheel, built from this checkout and unpacked as pip lays it out."""
    import importlib.util
    import zipfile
    from pathlib import Path

    root = Path(__file__).resolve().parents[2]
    spec = importlib.util.spec_from_file_location("build_wheel", root / "scripts" / "build-wheel.py")
    builder = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(builder)
    out = tmp_path_factory.mktemp("wheel")
    with zipfile.ZipFile(builder.build(out / "dist")) as wheel:
        wheel.extractall(out / "site")
    return out / "site"


def _no_path_env(tmp_path):
    env = {k: v for k, v in os.environ.items() if k != "PYTHONPATH" and not k.startswith("LIGHTSIM_")}
    env.update(LIGHTSIM_DATA_DIR=str(tmp_path / "d"), LIGHTSIM_PROJECTS_DIR=str(tmp_path / "p"))
    return env


def test_the_appcontainer_worker_boots_the_engine_from_the_wheel(wheel_site, tmp_path):
    # The worker's own command line, as the wheel's engine builds it, run up to
    # child_main (which serves the engine over a pipe, inside the container on
    # Windows; the whole run is the test below): it must import the engine.
    import json
    import subprocess

    env = _no_path_env(tmp_path)
    ask = ("import json, sys; sys.path.insert(0, sys.argv[1]); "
           "from lightsim._app.solver import win_sandbox as w; "
           "print(json.dumps(w.worker_argv(\"script\", 1)))")
    asked = subprocess.run([sys.executable, "-c", ask, str(wheel_site)], cwd=tmp_path, env=env,
                           capture_output=True, text=True, timeout=120)
    assert asked.returncode == 0, asked.stderr
    argv = json.loads(asked.stdout)
    boot = argv[3]
    up_to_serving = boot.replace("child_main(sys.argv[1:3])", "print(child_main.__module__)")
    assert up_to_serving != boot
    done = subprocess.run([argv[0], "-I", "-c", up_to_serving, *argv[4:]], cwd=tmp_path, env=env,
                          capture_output=True, text=True, timeout=120)
    assert done.returncode == 0, done.stderr
    assert done.stdout.strip() == "_app.solver.win_sandbox"


def test_the_worker_gets_the_site_packages_and_each_folder_tree_once(tmp_path, monkeypatch):
    # The engine's solver package imports pydantic, so a Script worker needs
    # the site-packages too, not only the engine's folder: from the wheel or a
    # venv, pydantic is not in the base interpreter's folder. A folder inside
    # another is granted through the outer folder's inherited entry, so it is
    # not granted again (each grant costs a second on Windows).
    import base64

    from app.solver import win_sandbox as ws
    base, venv = tmp_path / "Python311", tmp_path / "venv"
    site = venv / "Lib" / "site-packages"
    for folder in (base / "Lib" / "site-packages", site / "lightsim" / "_app"):
        folder.mkdir(parents=True)
    monkeypatch.setattr(sys, "base_prefix", str(base))
    monkeypatch.setattr(sys, "prefix", str(venv))
    monkeypatch.setattr(sys, "path", [str(base / "Lib"), str(base / "Lib" / "site-packages"), str(site)])
    monkeypatch.setattr(ws, "_ENGINE", site / "lightsim" / "_app")
    assert ws._runtime_dirs() == sorted([str(base), str(venv)])
    path = base64.b64decode(ws._child_syspath()).decode("utf-8").split(os.pathsep)
    assert path == [str(site / "lightsim"), str(base / "Lib" / "site-packages"), str(site)]


def test_lightsim_run_from_the_wheel_runs_a_new_script_in_the_strict_sandbox(wheel_site, tmp_path):
    # A script nobody approved runs through `lightsim run` only in the strict
    # sandbox (with LIGHTSIM_STRICT_SANDBOX=0 it is refused as not approved),
    # so a success here is a worker that started confined: in its AppContainer
    # on Windows, under Landlock on Linux.
    import json
    import subprocess
    from pathlib import Path

    if why_no_strict_sandbox() is not None:
        pytest.skip("this machine cannot set up the strict sandbox")
    backend = Path(__file__).resolve().parents[1]
    raw = json.loads((backend / "projects" / "hybrid-car.json").read_text(encoding="utf-8"))
    raw["cases"] = [{**raw["cases"][0], "duration": 5, "realtimeFactor": 0}]
    for el in (e for s in raw["systems"] for e in s["elements"] if e["componentDefId"] == "signal.script"):
        el["parameterOverrides"]["code"] = el["parameterOverrides"].get("code", "") + "\n# approved by nobody\n"
    proj = tmp_path / "car.json"
    proj.write_text(json.dumps(raw), encoding="utf-8")
    run = ("import sys; sys.path.insert(0, sys.argv.pop(1)); import lightsim._engine as e; "
           "assert e.BASE == \"lightsim._app\", e.BASE; from lightsim.cli import main; "
           "sys.exit(main(sys.argv[1:]))")
    command = [sys.executable, "-c", run, str(wheel_site), "run", str(proj), "--json"]
    env = _no_path_env(tmp_path)
    done = subprocess.run(command, cwd=tmp_path, env=env, capture_output=True, text=True,
                          encoding="utf-8", timeout=900)
    assert done.returncode == 0 and json.loads(done.stdout)["status"] == "success", done.stdout + done.stderr
    person = subprocess.run(command, cwd=tmp_path, env={**env, "LIGHTSIM_STRICT_SANDBOX": "0"},
                            capture_output=True, text=True, encoding="utf-8", timeout=900)
    assert person.returncode == 2 and "has not been approved" in person.stdout, person.stdout + person.stderr
