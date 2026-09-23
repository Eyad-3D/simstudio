"""Script (Function) component execution.

User code defines `def step(t, dt, inputs, state, params)` and returns a
dict of output values keyed by output-port id. The code is compiled once
per run and executed every solver step (dt = the solver step, ≤ 10 ms),
or at the block's Sample Time if that is longer.

Scripts are for signal math, and are held to that:

* check_script() parses and compiles the code without running any of it —
  all Data Checks ever do — and rejects imports other than math, names and
  attributes that lead into the interpreter (dunders, frames), attribute
  assignment, class definitions, and builtins scripts are not given
  (open, eval, getattr, ...).
* At run time the code gets only a small allow-listed set of builtins,
  imports go through the same allow-list, and every call into user code
  has a wall-clock limit, so an endless loop fails the run instead of
  hanging the engine.

This is NOT a security sandbox. It runs inside the engine process, and
in-process Python cannot be fully contained: a single huge operation
(e.g. `10 ** 10 ** 9`) is not interrupted, memory is not capped, and an
endless loop inside a `finally:` block that runs after the time limit
fired keeps going. It stops the obvious ways a script could reach files,
processes or the network; the real protection is that only the
SimStudio window can reach the engine (see app/security.py).
"""
from __future__ import annotations

import ast
import builtins
import math
import sys
import time
from contextlib import contextmanager
from typing import Callable

from .maps import interp1, interp2, parse_table1d, parse_table2d

#: Modules a script may import. clamp() and interp() are provided directly.
ALLOWED_IMPORTS = frozenset({"math"})

#: Wall-clock budget for one call into user code (its top-level code at run
#: start, or one step() call). Real control scripts take microseconds; this
#: only catches a script that never returns.
TIME_LIMIT_S = 2.0

#: Attributes without a leading underscore that still hand out interpreter
#: internals (a frame reaches every module's globals). `mro` hands out
#: BaseException: a script that catches the time limit once stops being
#: traced (Python drops a frame's tracer when it raises) and runs forever.
_FORBIDDEN_ATTRS = frozenset({
    "gi_frame", "gi_code", "gi_yieldfrom", "cr_frame", "cr_code", "cr_await",
    "ag_frame", "ag_code", "ag_await", "f_back", "f_builtins", "f_code",
    "f_globals", "f_locals", "tb_frame", "tb_next", "mro",
})

_SAFE_BUILTIN_NAMES = (
    "abs", "all", "any", "bin", "bool", "callable", "chr", "complex", "dict",
    "divmod", "enumerate", "filter", "float", "format", "frozenset", "hasattr",
    "hex", "int", "isinstance", "iter", "len", "list", "map", "max", "min", "next",
    "oct", "ord", "pow", "print", "range", "repr", "reversed", "round", "set",
    "slice", "sorted", "str", "sum", "tuple", "zip",
    "ArithmeticError", "AssertionError", "AttributeError", "Exception",
    "FloatingPointError", "IndexError", "KeyError", "LookupError", "NameError",
    "NotImplementedError", "OverflowError", "RuntimeError", "StopIteration",
    "TypeError", "ValueError", "ZeroDivisionError",
)


class ScriptError(RuntimeError):
    """Compilation or runtime failure of a Script element."""


class _Overrun(BaseException):
    """Raised inside user code when a call exceeds TIME_LIMIT_S. Derived from
    BaseException so a script's own `except Exception:` does not catch it."""


def _clamp(x: float, lo: float, hi: float) -> float:
    return max(lo, min(hi, x))


def _interp(table: dict, x: float, y: float | None = None) -> float:
    """Convenience lookup for scripts: 1D {x: v} or 2D {x: {y: v}} tables."""
    if y is None:
        return interp1(parse_table1d(table), x)
    return interp2(parse_table2d(table), x, y)


def _guarded_import(name, globals=None, locals=None, fromlist=(), level=0):
    if level != 0 or name not in ALLOWED_IMPORTS:
        raise ImportError(f"importing '{name}' is not allowed in scripts")
    return builtins.__import__(name, globals, locals, fromlist, level)


_SAFE_BUILTINS: dict = {name: getattr(builtins, name) for name in _SAFE_BUILTIN_NAMES}
_SAFE_BUILTINS["__import__"] = _guarded_import

#: Names every script can use without defining them.
_PROVIDED = {"math": math, "clamp": _clamp, "interp": _interp}


def _bound_names(tree: ast.AST) -> set[str]:
    """Every name the script binds anywhere (ignores scope, so errs lenient)."""
    names: set[str] = set()
    for node in ast.walk(tree):
        if isinstance(node, ast.Name) and not isinstance(node.ctx, ast.Load):
            names.add(node.id)
        elif isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef)):
            names.add(node.name)
        elif isinstance(node, ast.arg):
            names.add(node.arg)
        elif isinstance(node, ast.alias):
            names.add((node.asname or node.name).split(".")[0])
        elif isinstance(node, (ast.ExceptHandler, ast.MatchAs, ast.MatchStar)) and node.name:
            names.add(node.name)
        elif isinstance(node, ast.MatchMapping) and node.rest:
            names.add(node.rest)
    return names


def _attr_problem(attr: str) -> str | None:
    if attr.startswith("_") or attr in _FORBIDDEN_ATTRS:
        return f"'.{attr}' is not allowed in scripts"
    return None


def _problems(tree: ast.AST) -> list[tuple[tuple[int, int, int], str]]:
    """What the script uses that scripts may not, sorted by position."""
    bound = _bound_names(tree) | set(_PROVIDED)
    found = []

    def add(node: ast.AST, text: str) -> None:
        pos = (getattr(node, "lineno", 0), getattr(node, "col_offset", 0),
               getattr(node, "end_col_offset", 0) or 0)
        found.append((pos, text))

    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            for alias in node.names:
                if alias.name not in ALLOWED_IMPORTS:
                    add(node, f"importing '{alias.name}' is not allowed — scripts can import only math")
        elif isinstance(node, ast.ImportFrom):
            module = "." * node.level + (node.module or "")
            if module not in ALLOWED_IMPORTS:
                add(node, f"importing '{module}' is not allowed — scripts can import only math")
            for alias in node.names:
                if alias.name.startswith("_"):
                    add(node, f"importing '{alias.name}' is not allowed in scripts")
        elif isinstance(node, ast.Attribute):
            problem = _attr_problem(node.attr)
            if problem:
                add(node, problem)
            elif not isinstance(node.ctx, ast.Load):
                # math and the helpers are shared with the engine itself
                add(node, f"changing '.{node.attr}' is not allowed in scripts")
        elif isinstance(node, ast.MatchClass):
            for attr in node.kwd_attrs:
                problem = _attr_problem(attr)
                if problem:
                    add(node, problem)
        elif isinstance(node, ast.Name):
            if node.id.startswith("__"):
                add(node, f"'{node.id}' is not allowed in scripts")
            elif (isinstance(node.ctx, ast.Load) and node.id not in bound
                  and node.id not in _SAFE_BUILTINS and hasattr(builtins, node.id)):
                add(node, f"'{node.id}' is not available in scripts")
        elif isinstance(node, ast.ClassDef):
            add(node, "class definitions are not supported in scripts")
    return sorted(found)


def check_script(code: str, label: str):
    """Parse and compile a Script without running any of it.

    Returns the code object. Raises ScriptError for syntax errors, for code
    that reaches outside what scripts may use, and when no step() is defined.
    """
    filename = f"<script:{label}>"
    try:
        tree = ast.parse(code, filename, "exec")
        for node in ast.walk(tree):
            if isinstance(node, ast.ExceptHandler) and node.type is None:
                # a bare `except:` means `except Exception:` here, so it
                # cannot swallow the time limit (_Overrun)
                node.type = ast.copy_location(ast.Name("Exception", ast.Load()), node)
        compiled = compile(tree, filename, "exec")
        problems = _problems(tree)
    except (SyntaxError, ValueError) as e:
        raise ScriptError(f"Script '{label}' failed to compile: {e}") from e
    except (RecursionError, MemoryError):
        raise ScriptError(
            f"Script '{label}' is too deeply nested to check — split long "
            "if/elif chains or expressions into smaller parts.") from None
    if problems:
        (line, _, _), text = problems[0]
        raise ScriptError(f"Script '{label}' line {line}: {text}.")
    if "step" not in _bound_names(tree):
        raise ScriptError(f"Script '{label}' must define a function 'step(t, dt, inputs, state, params)'.")
    return compiled


@contextmanager
def _time_limit(seconds: float):
    """Raise _Overrun inside user code once `seconds` have passed.

    A line tracer on the script's own frames watches the clock; the helpers
    it calls (interp, math) run untraced. Whatever tracer the thread had
    before (a debugger, coverage) is put back afterwards.
    """
    deadline = time.perf_counter() + seconds
    overran = False

    def local(frame, event, arg):
        nonlocal overran
        if time.perf_counter() > deadline:
            overran = True
            raise _Overrun
        return local

    def trace(frame, event, arg):
        if not frame.f_code.co_filename.startswith("<script:"):
            return None
        return local(frame, event, arg)

    previous = sys.gettrace()
    sys.settrace(trace)
    try:
        yield
    finally:
        sys.settrace(previous)
    if overran:  # the script swallowed _Overrun (`return` in a `finally:`)
        raise _Overrun


def compile_script(code: str, label: str) -> Callable:
    """Check a Script, run its top-level code in the restricted namespace and
    return its step(). Only the solver calls this, at the start of a run."""
    compiled = check_script(code, label)
    namespace: dict = {"__builtins__": _SAFE_BUILTINS, **_PROVIDED}
    try:
        with _time_limit(TIME_LIMIT_S):
            exec(compiled, namespace)
    except _Overrun:
        raise ScriptError(
            f"Script '{label}': top-level code did not finish within {TIME_LIMIT_S:g} s.") from None
    except Exception as e:  # noqa: BLE001 — surface any user-code failure
        raise ScriptError(f"Script '{label}' failed to compile: {e}") from e
    fn = namespace.get("step")
    if not callable(fn):
        raise ScriptError(f"Script '{label}' must define a function 'step(t, dt, inputs, state, params)'.")
    return fn


def run_script(
    fn: Callable,
    label: str,
    t: float,
    dt: float,
    inputs: dict[str, float],
    state: dict,
    params: dict,
) -> dict[str, float]:
    try:
        with _time_limit(TIME_LIMIT_S):
            out = fn(t, dt, inputs, state, params)
    except _Overrun:
        raise ScriptError(
            f"Script '{label}' did not return within {TIME_LIMIT_S:g} s at t = {t:g} s "
            f"— check it for an endless loop.") from None
    except Exception as e:  # noqa: BLE001
        raise ScriptError(f"Script '{label}' raised at t = {t:g} s: {e}") from e
    if out is None:
        return {}
    if not isinstance(out, dict):
        raise ScriptError(f"Script '{label}' must return a dict of output values, got {type(out).__name__}.")
    result: dict[str, float] = {}
    for key, value in out.items():
        try:
            result[str(key)] = float(value)
        except (TypeError, ValueError):
            raise ScriptError(f"Script '{label}' output '{key}' is not numeric ({value!r}).")
    return result
