"""Script (Function) component execution.

User code defines `def step(t, dt, inputs, state, params)` and returns a
dict of output values keyed by output-port id. The code is compiled once
per run and executed every solver step (dt = the solver step, ≤ 10 ms).
It runs with normal Python semantics on the local backend — this is
trusted user code, the same trust level as editing the backend itself,
not a security sandbox.
"""
from __future__ import annotations

import math
from typing import Callable

from .maps import interp1, interp2, parse_table1d, parse_table2d


class ScriptError(RuntimeError):
    """Compilation or runtime failure of a Script element."""


def _clamp(x: float, lo: float, hi: float) -> float:
    return max(lo, min(hi, x))


def _interp(table: dict, x: float, y: float | None = None) -> float:
    """Convenience lookup for scripts: 1D {x: v} or 2D {x: {y: v}} tables."""
    if y is None:
        return interp1(parse_table1d(table), x)
    return interp2(parse_table2d(table), x, y)


def compile_script(code: str, label: str) -> Callable:
    namespace: dict = {"math": math, "clamp": _clamp, "interp": _interp}
    try:
        exec(compile(code, f"<script:{label}>", "exec"), namespace)
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
        out = fn(t, dt, inputs, state, params)
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
