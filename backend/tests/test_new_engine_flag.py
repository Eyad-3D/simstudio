"""LIGHTSIM_ENGINE=new runs the cases the new engine can on it, behind a flag
(app/solver/new_engine.py); today's engine stays the default.

The tests that run the new engine need the compiled module ``lightsim_engine``
(engine/crates/lsim-py, built with maturin); CI's backend jobs do not build
it, so they skip there."""
from __future__ import annotations

import pytest

from app.solver import new_engine, simulate
from app.storage import load_example

try:
    import lightsim_engine  # noqa: F401
    NEW_ENGINE_MISSING = None
except ImportError as e:
    NEW_ENGINE_MISSING = f"the compiled module lightsim_engine is not installed ({e})"

needs_engine = pytest.mark.skipif(NEW_ENGINE_MISSING is not None, reason=NEW_ENGINE_MISSING or "")

WLTC = "case-wltc"


def texts(result) -> str:
    return "\n".join(f"{m.level}: {m.text}" for m in result.messages)


@pytest.fixture(scope="module")
def today():
    """The BEV car's WLTC case on today's engine, run once."""
    return simulate(load_example("bev-car"), WLTC)


def test_without_the_flag_nothing_changes(monkeypatch, today):
    monkeypatch.delenv(new_engine.ENV_VAR, raising=False)
    result = simulate(load_example("bev-car"), WLTC)
    assert "engine" not in texts(result).lower().replace("combustion", "")
    assert result.model_dump() == today.model_dump()
    monkeypatch.setenv(new_engine.ENV_VAR, "today")
    assert simulate(load_example("bev-car"), WLTC).model_dump() == today.model_dump()


@needs_engine
def test_new_engine_runs_a_cycle_case_like_today(monkeypatch, today):
    monkeypatch.setenv(new_engine.ENV_VAR, "new")
    result = simulate(load_example("bev-car"), WLTC)
    assert result.status in ("success", "warning")
    assert result.messages[0].level == "info" and "new engine" in result.messages[0].text
    assert "today's engine" not in texts(result)

    # every channel of today's, in today's order (the new engine records a
    # few more, such as the battery's current limits), and the same summary rows
    ids = [(c.elementId, c.portId) for c in result.channels]
    assert ids == sorted(ids)
    mine = {(c.elementId, c.portId): (c.label, c.unit) for c in result.channels}
    assert [mine.get((c.elementId, c.portId)) for c in today.channels] \
        == [(c.label, c.unit) for c in today.channels]
    # but today's two checks of its own integrator, which the new engine
    # replaces with its energy books' closure (golden/README.md)
    checks = {"energy_balance_residual_pct", "energy_balance_error_pct"}
    assert [(s.key, s.label, s.unit) for s in result.summary] \
        == [(s.key, s.label, s.unit) for s in today.summary if s.key not in checks]

    # The values are close, not equal: the new engine integrates with an
    # error control (rtol 1e-6) where today's takes fixed semi-implicit Euler
    # steps, so the difference is today's own discretisation error. 1 % is
    # roomy for that; the headline figures agree far better.
    now = {s.key: s.value for s in result.summary}
    for s in today.summary:
        if s.key in checks or s.unit == "%" or abs(s.value) < 1e-6:
            continue  # shares and residuals of the books are not comparable
        assert now[s.key] == pytest.approx(s.value, rel=0.01), s.key
    assert now["distance_km"] == pytest.approx(23.27, rel=0.01)
    by_id = {(c.elementId, c.portId): c for c in result.channels}
    for old in today.channels:
        assert len(by_id[(old.elementId, old.portId)].timeSeries) == len(old.timeSeries)
    soc_old = next(c for c in today.channels if c.portId == "sig_soc")
    soc_new = next(c for c in result.channels if c.portId == "sig_soc")
    assert soc_new.timeSeries[-1]["value"] == pytest.approx(soc_old.timeSeries[-1]["value"], abs=0.2)


@needs_engine
def test_new_engine_runs_an_acceleration_test(monkeypatch):
    monkeypatch.setenv(new_engine.ENV_VAR, "new")
    result = simulate(load_example("fs-electric"), "case-accel-75m")
    assert result.messages[0].level == "info" and "new engine" in result.messages[0].text
    assert result.channels and result.summary


def test_a_lap_case_falls_back_with_a_message(monkeypatch):
    monkeypatch.setenv(new_engine.ENV_VAR, "new")
    result = simulate(load_example("fs-electric"), "case-autocross")
    first = result.messages[0]
    assert first.level == "info"
    assert "today's engine" in first.text and "lap" in first.text
    assert result.channels  # and it ran


def test_a_hybrid_balanced_case_falls_back(monkeypatch):
    monkeypatch.setenv(new_engine.ENV_VAR, "new")
    assert "charge-balance" in new_engine.unsupported(load_example("hybrid-car"), "case-mixed", None)


def test_a_live_run_falls_back(monkeypatch):
    monkeypatch.setenv(new_engine.ENV_VAR, "new")
    why = new_engine.unsupported(load_example("bev-car"), WLTC, lambda msg: None)
    assert "live" in why


def test_without_the_module_it_falls_back(monkeypatch):
    import builtins
    real = builtins.__import__

    def refuse(name, *args, **kwargs):
        if name == "lightsim_engine":
            raise ImportError("not built")
        return real(name, *args, **kwargs)

    monkeypatch.setenv(new_engine.ENV_VAR, "new")
    monkeypatch.setattr(builtins, "__import__", refuse)
    result = simulate(load_example("fs-electric"), "case-accel-75m")
    assert "not installed" in result.messages[0].text and result.channels


@needs_engine
def test_a_failing_new_engine_falls_back_with_a_warning(monkeypatch, today):
    monkeypatch.setenv(new_engine.ENV_VAR, "new")

    def broken(*args, **kwargs):
        raise lightsim_engine.ModelError("[RUN-INTEGRATOR] the integrator gave up at t = 12 s")

    monkeypatch.setattr(lightsim_engine, "build", broken)
    result = simulate(load_example("bev-car"), WLTC)
    first = result.messages[0]
    assert first.level == "warning"
    assert "RUN-INTEGRATOR" in first.text and "today's engine" in first.text
    assert result.status == "warning"
    assert [s.key for s in result.summary] == [s.key for s in today.summary]
    assert [s.value for s in result.summary] == [s.value for s in today.summary]
