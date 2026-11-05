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


def test_an_agent_script_run_is_refused_when_the_sandbox_is_unavailable(monkeypatch):
    # the agent path (STRICT) on a platform where the sandbox cannot be set up
    from app.solver import domains
    monkeypatch.setattr(domains, "why_no_strict_sandbox",
                        lambda *a: f"no sandbox here [{SANDBOX_UNAVAILABLE}]")
    token = STRICT.set(True)
    try:
        result = simulate(_scripted(STEP), "case")
    finally:
        STRICT.reset(token)
    assert result.status == "failed"
    assert SANDBOX_UNAVAILABLE in " ".join(m.text for m in result.messages)


def test_an_app_run_is_not_refused_when_strict_is_off(monkeypatch):
    # a person's run (STRICT unset) keeps today's behaviour even where the
    # strict sandbox would be unavailable
    from app.solver import domains
    monkeypatch.setattr(domains, "why_no_strict_sandbox",
                        lambda *a: f"no sandbox here [{SANDBOX_UNAVAILABLE}]")
    result = simulate(_scripted(STEP), "case")
    assert result.status in ("success", "warning"), [m.text for m in result.messages]


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

    secret = os.path.join(tempfile.mkdtemp(prefix="lsim-sec-"), "s.txt")
    with open(secret, "w", encoding="utf-8") as f:
        f.write("SECRET")
    written = os.path.join(tempfile.gettempdir(), f"lsim-ac-escape-{os.getpid()}.txt")
    attempts = {
        "read a user file": f"open(r{secret}).read()",
        "write a file": f"open(r{written}, w).write(x)",
        "open a TCP connection": "import socket\n    s = socket.socket(); s.settimeout(3); "
                                 "s.connect((127.0.0.1, 9))",
    }
    for what, body in attempts.items():
        code = f"def step(t, dt, inputs, state, params):\n    {body}\n    return {{cmd_out: 0.0}}\n"
        sb = ScriptSandbox([ScriptSpec("e", "E", code, [], {})], strict=True, trusted=True)
        try:
            with pytest.raises(ScriptError):
                sb.run("e", "E", 0.0, 0.01, {}, {})
        finally:
            sb.close()
    assert open(secret, encoding="utf-8").read() == "SECRET", what
    assert not os.path.exists(written), "the worker could not write a file"
