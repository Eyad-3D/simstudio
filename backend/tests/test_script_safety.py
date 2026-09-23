"""Script component safety: Data Checks never run script code, and a run only
lets scripts do signal math (no imports beyond math, no builtins that reach
files or the interpreter, no dunder/frame tricks, a per-call time limit)."""
import sys
import time

import pytest
from fastapi.testclient import TestClient
from helpers import bev_axle, sig_port

from app.main import app
from app.schemas import ElementInstance
from app.solver import ScriptError, check_script, compile_script, scripting, simulate
from app.validation import validate_project

client = TestClient(app)

STEP = "def step(t, dt, inputs, state, params):\n    return {'cmd_out': 0.1}\n"


def _scripted(code: str):
    proj = bev_axle()
    proj.systems[0].elements.append(ElementInstance(
        id="scr", componentDefId="signal.script", label="Probe",
        position={"x": 0, "y": 0}, parameterOverrides={"code": code},
        dynamicPorts=[sig_port("cmd_in", "input"), sig_port("cmd_out", "output")],
    ))
    return proj


def _errors(checks):
    return [c.text for c in checks if c.level == "error"]


# ---- Data Checks compile, never execute ---------------------------------------

def test_validate_never_runs_top_level_code(tmp_path):
    marker = tmp_path / "marker.txt"
    code = f"import os\nopen({str(marker)!r}, 'w').write(str(os.getuid()))\n" + STEP
    checks = client.post("/api/validate", json={"project": _scripted(code).model_dump()}).json()
    assert not marker.exists(), "Data Checks executed the script's top-level code"
    assert any("importing 'os' is not allowed" in c["text"] for c in checks if c["level"] == "error")


def test_validate_does_not_evaluate_legal_top_level_code():
    # Allowed code that would fail if it ran: only a run may surface that.
    proj = _scripted("x = 1 / 0\n" + STEP)
    assert not [e for e in _errors(validate_project(proj)) if "Probe" in e]
    result = simulate(proj, "case")
    assert result.status == "failed"
    assert any("division by zero" in m.text for m in result.messages)


def test_validate_returns_immediately_for_a_top_level_loop():
    t0 = time.perf_counter()
    validate_project(_scripted("while True:\n    pass\n" + STEP))
    assert time.perf_counter() - t0 < 2.0


def test_validate_still_reports_syntax_errors_and_missing_step():
    assert any("failed to compile" in e for e in _errors(validate_project(_scripted("def step(:\n"))))
    assert any("must define a function 'step" in e
               for e in _errors(validate_project(_scripted("x = 1\n"))))


# ---- what a script may not do -------------------------------------------------

ESCAPES = {
    "import os": ("import os\n", "importing 'os' is not allowed"),
    "import subprocess": ("import subprocess\n", "importing 'subprocess' is not allowed"),
    "from os import system": ("from os import system\n", "importing 'os' is not allowed"),
    "nested import": ("def step(t, dt, inputs, state, params):\n    import socket\n    return {}\n",
                      "importing 'socket' is not allowed"),
    "open": ("open('/etc/passwd').read()\n", "'open' is not available"),
    "eval": ("eval('1 + 1')\n", "'eval' is not available"),
    "exec": ("exec('x = 1')\n", "'exec' is not available"),
    "getattr": ("getattr((), '__cl' + 'ass__')\n", "'getattr' is not available"),
    "__import__": ("__import__('os')\n", "'__import__' is not allowed"),
    "__builtins__": ("b = __builtins__\n", "'__builtins__' is not allowed"),
    "__class__ walk": ("subs = ().__class__.__bases__[0].__subclasses__()\n",
                       "'.__class__' is not allowed"),
    "function globals": ("g = clamp.__globals__\n", "'.__globals__' is not allowed"),
    "generator frame": ("g = (i for i in [1])\nf = g.gi_frame.f_back\n", "'.gi_frame' is not allowed"),
    "match attribute": ("match 1:\n    case int(__class__=c):\n        pass\n",
                        "'.__class__' is not allowed"),
    "class": ("class A:\n    pass\n", "class definitions are not supported"),
    "patch math": ("math.sqrt = abs\n", "changing '.sqrt' is not allowed"),
    # .mro() reaches BaseException, which could catch the time limit
    "mro": ("BE = ValueError.mro()[-2]\n", "'.mro' is not allowed"),
}


@pytest.mark.parametrize("name", list(ESCAPES))
def test_escape_attempts_are_rejected_with_a_clear_message(name):
    prelude, expected = ESCAPES[name]
    with pytest.raises(ScriptError) as exc:
        check_script(prelude + STEP, "Probe")
    assert expected in str(exc.value)
    assert "line" in str(exc.value)

    # the same message reaches Data Checks and blocks a run
    proj = _scripted(prelude + STEP)
    assert any(expected in e for e in _errors(validate_project(proj)))
    result = simulate(proj, "case")
    assert result.status == "failed"
    assert any(expected in m.text for m in result.messages)


def test_restricted_runtime_backs_up_the_static_check():
    # Even if something slipped past check_script, the code runs with a small
    # set of builtins, and imports go through the allow-list.
    fn = compile_script(STEP, "S")
    run_builtins = fn.__globals__["__builtins__"]
    for name in ("open", "eval", "exec", "compile", "getattr", "globals", "type", "__build_class__"):
        assert name not in run_builtins
    with pytest.raises(ImportError):
        run_builtins["__import__"]("os")
    assert run_builtins["__import__"]("math").sqrt(4) == 2


# ---- what a script may still do ----------------------------------------------

def test_ordinary_scripts_keep_working():
    code = (
        "import math\n"
        "from math import sqrt as root\n"
        "GAIN = 0.5\n"
        "def helper(x):\n"
        "    return [v * GAIN for v in (x, -x)]\n"
        "def step(t, dt, inputs, state, params):\n"
        "    try:\n"
        "        ratio = inputs['cmd_in'] / 0.0\n"
        "    except ZeroDivisionError:\n"
        "        ratio = 0.0\n"
        "    state['n'] = state.get('n', 0) + 1\n"
        "    pos = max(helper(inputs.get('cmd_in', 0.0)))\n"
        "    lut = interp({'0': 0, '1': 1}, 0.5)\n"
        "    out = clamp(pos + ratio + math.sin(0) + root(0) + lut * 0, -1, 0.25)\n"
        "    return {'cmd_out': out}\n"
    )
    assert _errors(validate_project(_scripted(code))) == []
    result = simulate(_scripted(code), "case")
    assert result.status in ("success", "warning")


def test_harmless_builtins_are_available():
    code = ("def step(t, dt, inputs, state, params):\n"
            "    if not hasattr(state, 'get') or not callable(clamp):\n"
            "        raise NotImplementedError('unreachable')\n"
            "    code = ord(chr(65)) + int(hex(1), 16) + int(bin(1), 2) + int(oct(1), 8)\n"
            "    try:\n"
            "        assert code == 68\n"
            "    except (AssertionError, AttributeError, NameError):\n"
            "        code = 0\n"
            "    return {'cmd_out': abs(complex(code, 0)) / 1000}\n")
    assert scripting.run_script(compile_script(code, "S"), "S", 0.0, 0.01, {}, {}, {}) == {"cmd_out": 0.068}


def test_a_deeply_nested_script_is_reported_not_crashed():
    # ast.parse gives up at about 1000 levels; that must be a Data Check
    # error, not a 500 from /api/validate and /api/simulate
    chain = "".join(f"    elif x == {i}:\n        pass\n" for i in range(1100))
    code = "def step(t, dt, inputs, state, params):\n    x = 0\n    if x == -1:\n        pass\n" + chain + "    return {}\n"
    with pytest.raises(ScriptError, match="too deeply nested"):
        check_script(code, "Deep")
    response = client.post("/api/validate", json={"project": _scripted(code).model_dump()})
    assert response.status_code == 200
    assert any("too deeply nested" in c["text"] for c in response.json() if c["level"] == "error")


def test_names_a_script_defines_itself_are_not_flagged():
    # 'input' and 'vars' are builtins scripts do not get, but binding them is fine.
    code = ("def step(t, dt, inputs, state, params):\n"
            "    input = inputs.get('cmd_in', 0.0)\n"
            "    vars = [input]\n"
            "    return {'cmd_out': vars[0]}\n")
    check_script(code, "S")


def test_example_hybrid_controller_passes():
    proj = client.get("/api/projects/hybrid-car").json()
    checks = client.post("/api/validate", json={"project": proj}).json()
    assert not [c for c in checks if c["level"] == "error"]


# ---- per-call time limit -------------------------------------------------------

@pytest.fixture
def short_limit(monkeypatch):
    monkeypatch.setattr(scripting, "TIME_LIMIT_S", 0.2)


@pytest.mark.parametrize("body", [
    "    while True:\n        pass\n",
    # a script's own `except Exception` must not swallow the time limit
    "    while True:\n        try:\n            pass\n        except Exception:\n            pass\n",
    # nor a bare `except:` inside the loop
    "    while True:\n        try:\n            pass\n        except:\n            continue\n",
    # a `return` in `finally:` swallows it, but the call is still reported
    "    try:\n        while True:\n            pass\n    finally:\n        return {}\n",
])
def test_a_step_that_never_returns_fails_the_run(short_limit, body):
    code = "def step(t, dt, inputs, state, params):\n" + body
    t0 = time.perf_counter()
    result = simulate(_scripted(code), "case")
    assert time.perf_counter() - t0 < 10
    assert result.status == "failed"
    assert any("did not return within 0.2 s" in m.text for m in result.messages)


def test_top_level_code_that_never_finishes_fails_the_run(short_limit):
    result = simulate(_scripted("while True:\n    pass\n" + STEP), "case")
    assert result.status == "failed"
    assert any("did not finish within 0.2 s" in m.text for m in result.messages)


def test_time_limit_restores_an_existing_tracer():
    def tracer(frame, event, arg):
        return None

    fn = compile_script(STEP, "S")
    previous = sys.gettrace()
    sys.settrace(tracer)
    try:
        scripting.run_script(fn, "S", 0.0, 0.1, {}, {}, {})
        assert sys.gettrace() is tracer
    finally:
        sys.settrace(previous)


@pytest.mark.parametrize("helpers", [
    # no loop anywhere: every call into script code checks the clock
    "def spin(n):\n    return 0 if n == 0 else spin(n - 1) + spin(n - 1)\n",
    # the loop lives in a generator expression, a code object of its own
    "def spin(n):\n    return sum(1 for _ in iter(int, 1))\n",
    # the loop lives in a helper that step() calls
    "def spin(n):\n    while n >= 0:\n        n += 1\n    return n\n",
])
def test_code_without_loops_is_not_line_traced_but_still_time_limited(short_limit, helpers):
    code = helpers + "def step(t, dt, inputs, state, params):\n    return {'cmd_out': spin(60)}\n"
    t0 = time.perf_counter()
    result = simulate(_scripted(code), "case")
    assert time.perf_counter() - t0 < 10
    assert result.status == "failed"
    assert any("did not return within 0.2 s" in m.text for m in result.messages)


def test_loop_free_code_is_found_per_code_object():
    code = check_script(
        "A = [i for i in range(3)]\n"
        "def plain(x):\n    return x + 1 if x else 0\n"
        "def looping(x):\n    while x:\n        x -= 1\n    return x\n" + STEP, "S")
    names = {c.co_name for c in scripting._loop_free_code(code)}
    assert names == {"<module>", "plain", "step"}


def test_interp_parses_a_table_once_and_follows_edits(monkeypatch):
    """A table defined at the top of a script is parsed on first use, not
    on every step; a table the script changes is parsed again."""
    parsed = []
    real = scripting.parse_table1d
    monkeypatch.setattr(scripting, "parse_table1d", lambda raw: parsed.append(1) or real(raw))
    fn = compile_script(
        "TABLE = {'0': 0, '10': 1}\n"
        "def step(t, dt, inputs, state, params):\n"
        "    if t >= 5:\n"
        "        TABLE['10'] = 2\n"
        "    return {'y': interp(TABLE, 5)}\n", "S")
    ys = [scripting.run_script(fn, "S", float(t), 1.0, {}, {}, {})["y"] for t in range(10)]
    assert ys == [0.5] * 5 + [1.0] * 5
    assert len(parsed) == 2
