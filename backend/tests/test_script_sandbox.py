"""The second Script isolation layer: the subprocess sandbox.

These prove the OS-level guarantees that back up the in-process restriction
(covered in test_script_safety.py). Where a test needs code the in-process AST
filter would reject (opening a file, a socket), it drives the worker directly
with ``trusted=True``, which bypasses that first layer — so what stops the code
here is the worker's own isolation, not the allow-list.
"""
import multiprocessing
import sys
import time

import pytest
from helpers import bev_axle, sig_port

from app.schemas import ElementInstance
from app.solver import ScriptError, simulate
from app.solver.sandbox import ScriptSandbox, ScriptSpec

STEP = "def step(t, dt, inputs, state, params):\n    return {'cmd_out': 0.1}\n"


def _scripted(code: str):
    proj = bev_axle()
    proj.systems[0].elements.append(ElementInstance(
        id="scr", componentDefId="signal.script", label="Probe",
        position={"x": 0, "y": 0}, parameterOverrides={"code": code},
        dynamicPorts=[sig_port("cmd_in", "input"), sig_port("cmd_out", "output")],
    ))
    return proj


def _trusted_run(body: str, time_limit: float = 0.4):
    """Run raw step() code in the worker with the in-process layer bypassed."""
    code = "def step(t, dt, inputs, state, params):\n" + body
    sb = ScriptSandbox([ScriptSpec("s", "Probe", code, [], {})],
                       time_limit=time_limit, trusted=True)
    try:
        return sb.run("s", "Probe", 0.0, 0.01, {}, {})
    finally:
        sb.close()


def _landlock_available() -> bool:
    if not sys.platform.startswith("linux"):
        return False
    try:
        import ctypes
        libc = ctypes.CDLL(None, use_errno=True)
        libc.syscall.restype = ctypes.c_long
        return libc.syscall(444, None, ctypes.c_size_t(0), ctypes.c_uint32(1)) >= 0
    except Exception:  # noqa: BLE001
        return False


landlock = pytest.mark.skipif(
    not _landlock_available(),
    reason="filesystem/network confinement needs Landlock (Linux >= 5.13)")


# ---- the worker cannot touch the filesystem or network ------------------------

def test_worker_cannot_write_a_file(tmp_path):
    marker = tmp_path / "marker.txt"
    with pytest.raises(ScriptError):
        _trusted_run(f"    open({str(marker)!r}, 'w').write('x')\n    return {{}}\n")
    assert not marker.exists() or marker.stat().st_size == 0


@landlock
def test_worker_cannot_read_a_file():
    with pytest.raises(ScriptError) as exc:
        _trusted_run("    return {'y': float(len(open('/etc/hostname').read()))}\n")
    assert "denied" in str(exc.value).lower() or "permitted" in str(exc.value).lower()


@landlock
def test_worker_cannot_list_a_directory():
    with pytest.raises(ScriptError) as exc:
        _trusted_run("    import os\n    return {'y': float(len(os.listdir('/')))}\n")
    assert "denied" in str(exc.value).lower() or "permitted" in str(exc.value).lower()


@landlock
def test_worker_cannot_open_a_tcp_socket():
    body = ("    import socket\n"
            "    s = socket.socket(); s.settimeout(2)\n"
            "    s.connect(('127.0.0.1', 9))\n"
            "    return {'y': 1.0}\n")
    with pytest.raises(ScriptError) as exc:
        _trusted_run(body)
    assert "denied" in str(exc.value).lower() or "permitted" in str(exc.value).lower()


def test_worker_cannot_read_the_environment(monkeypatch):
    # A secret in the engine's environment is not visible to the worker.
    monkeypatch.setenv("LIGHTSIM_SANDBOX_SECRET", "top-secret")
    out = _trusted_run(
        "    import os\n"
        "    return {'y': 1.0 if os.getenv('LIGHTSIM_SANDBOX_SECRET') else 0.0}\n")
    assert out == {"y": 0.0}


# ---- runaway scripts fail the run within the limit ----------------------------

def test_c_level_endless_loop_fails_within_the_limit():
    # list(iter(int, 1)) loops in C; the in-process tracer can never stop it, so
    # only the worker being killed can. It passes the AST filter, so this runs
    # as an ordinary (checked) script through simulate().
    import app.solver.scripting as scripting
    orig = scripting.TIME_LIMIT_S
    scripting.TIME_LIMIT_S = 0.3
    try:
        t0 = time.perf_counter()
        result = simulate(_scripted(
            "def step(t, dt, inputs, state, params):\n"
            "    return {'cmd_out': float(len(list(iter(int, 1))))}\n"), "case")
        assert time.perf_counter() - t0 < 10
    finally:
        scripting.TIME_LIMIT_S = orig
    assert result.status == "failed"
    assert any("did not return within 0.3 s" in m.text for m in result.messages)


def test_memory_bomb_fails_the_run():
    # A huge allocation hits the worker's address-space cap (a MemoryError, or
    # the worker stopping) instead of taking the machine down.
    result = simulate(_scripted(
        "def step(t, dt, inputs, state, params):\n"
        "    x = [0] * (10 ** 9)\n"
        "    return {'cmd_out': float(len(x))}\n"), "case")
    assert result.status == "failed"
    assert any("Probe" in m.text for m in result.messages if m.level == "error")


# ---- lifecycle: no orphan workers ---------------------------------------------

def test_no_worker_for_a_model_without_scripts():
    from app.solver.core import RunContext  # noqa: PLC0415
    from app.solver.network import build_model  # noqa: PLC0415
    proj = bev_axle()
    gear_of: dict = {}
    model = build_model(proj, gear_of, {})
    from app.solver.runtime import Runtime  # noqa: PLC0415
    ctx = RunContext(proj, model, Runtime(model, None), gear_of, {})
    assert ctx.sandbox is None


def test_worker_is_gone_after_a_normal_run():
    before = set(multiprocessing.active_children())
    result = simulate(_scripted(STEP), "case")
    assert result.status in ("success", "warning")
    after = set(multiprocessing.active_children())
    assert not (after - before), "a sandbox worker was left running after the run"


def test_no_orphan_when_a_script_fails_to_compile():
    # RunContext aborts before it can hold the sandbox, so the worker must be
    # stopped from inside its start-up, not left for a later close().
    before = set(multiprocessing.active_children())
    result = simulate(_scripted("import os\n" + STEP), "case")
    assert result.status == "failed"
    time.sleep(0.2)
    after = set(multiprocessing.active_children())
    assert not (after - before), "a sandbox worker survived a failed compile"


def test_worker_is_gone_after_a_cancelled_run():
    before = set(multiprocessing.active_children())
    calls = {"n": 0}

    def control():
        calls["n"] += 1
        return [{"type": "cancel"}] if calls["n"] >= 2 else []

    result = simulate(_scripted(STEP), "case", control=control)
    assert any("cancelled" in m.text.lower() for m in result.messages)
    time.sleep(0.2)
    after = set(multiprocessing.active_children())
    assert not (after - before), "a sandbox worker survived a cancelled run"


def test_sandbox_result_matches_the_in_process_value():
    # The worker runs the very same compiled step(), so outputs are identical.
    sb = ScriptSandbox([ScriptSpec(
        "s", "P",
        "def step(t, dt, inputs, state, params):\n"
        "    state['n'] = state.get('n', 0) + 1\n"
        "    return {'y': inputs['x'] * 2.0 + state['n']}\n",
        ["x"], {})])
    try:
        assert sb.run("s", "P", 0.0, 0.01, {"x": 3.0}, {}) == {"y": 7.0}
        assert sb.run("s", "P", 0.01, 0.01, {"x": 3.0}, {}) == {"y": 8.0}
    finally:
        sb.close()


def test_live_param_edit_reaches_the_worker():
    sb = ScriptSandbox([ScriptSpec(
        "s", "P",
        "def step(t, dt, inputs, state, params):\n"
        "    return {'y': float(params.get('gain', 1.0))}\n",
        [], {"gain": 1.0})])
    try:
        assert sb.run("s", "P", 0.0, 0.01, {}, {"gain": 1.0}) == {"y": 1.0}
        assert sb.run("s", "P", 0.01, 0.01, {}, {"gain": 5.0}) == {"y": 5.0}
    finally:
        sb.close()
