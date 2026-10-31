"""LIGHTSIM_ENGINE=new runs the cases the new engine can on it, behind a flag
(app/solver/new_engine.py); today's engine stays the default.

The tests that run the new engine need the compiled module ``lightsim_engine``
(engine/crates/lsim-py, built with maturin). CI's backend jobs do not build
it, so they skip there; the Windows engine job builds and installs it and
runs them with ``LIGHTSIM_REQUIRE_NEW_ENGINE=1``, where a missing module
fails them instead of skipping them (review finding 18)."""
from __future__ import annotations

import os

import pytest

from app import studies
from app.schemas import ElementInstance
from app.solver import new_engine, simulate
from app.solver.network import ModelError
from app.storage import load_example

try:
    import lightsim_engine  # noqa: F401
    NEW_ENGINE_MISSING = None
except ImportError as e:
    NEW_ENGINE_MISSING = f"the compiled module lightsim_engine is not installed ({e})"

# where the module must be there, nothing skips for want of it
REQUIRED = os.environ.get("LIGHTSIM_REQUIRE_NEW_ENGINE", "").strip() == "1"
needs_engine = pytest.mark.skipif(NEW_ENGINE_MISSING is not None and not REQUIRED,
                                  reason=NEW_ENGINE_MISSING or "")

WLTC = "case-wltc"


def texts(result) -> str:
    return "\n".join(f"{m.level}: {m.text}" for m in result.messages)


def test_the_module_is_there_where_it_must_be():
    if not REQUIRED:
        pytest.skip("LIGHTSIM_REQUIRE_NEW_ENGINE is not set: the module may be missing here")
    assert NEW_ENGINE_MISSING is None, NEW_ENGINE_MISSING


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
    assert result.messages[0].level == "info"
    assert result.messages[0].text.startswith("Run on the new engine")
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
    assert result.messages[0].level == "info"
    assert result.messages[0].text.startswith("Run on the new engine")
    assert result.channels and result.summary


def test_a_lap_case_falls_back_with_a_message(monkeypatch):
    monkeypatch.setenv(new_engine.ENV_VAR, "new")
    result = simulate(load_example("fs-electric"), "case-autocross")
    first = result.messages[0]
    assert first.level == "info"
    assert "today's engine" in first.text and "lap" in first.text
    assert "[NEW-LAP]" in first.text
    assert result.channels  # and it ran


def test_a_hybrid_balanced_case_falls_back(monkeypatch):
    monkeypatch.setenv(new_engine.ENV_VAR, "new")
    why = new_engine.unsupported(load_example("hybrid-car"), "case-mixed")
    assert why.code == "NEW-BALANCE" and "charge-balance" in why.why




def _changed(name: str, case_id: str, **case):
    project = load_example(name)
    c = next(c for c in project.cases if c.id == case_id)
    for k, v in case.items():
        setattr(c, k, v)
    return project


def test_every_refusal_has_its_code():
    """Each reason the path does not run a case comes with a code of CODES."""
    cells = load_example("bev-car")
    battery = next(el for s in cells.systems for el in s.elements if el.id == "el-battery")
    battery.parameterOverrides["pack_model"] = "Cells"
    fmu = load_example("bev-car")
    fmu.systems[0].elements.append(
        ElementInstance(id="fmu", componentDefId="signal.fmu", label="Lag", position={"x": 0, "y": 0}))
    seen = {
        "NEW-NOT-FOUND": new_engine.unsupported(load_example("bev-car"), "no-such-case"),
        "NEW-LAP": new_engine.unsupported(load_example("fs-electric"), "case-autocross"),
        "NEW-CASE-KIND": new_engine.unsupported(
            _changed("bev-car", WLTC, kind="performance"), WLTC),
        "NEW-FS-EVENT": new_engine.unsupported(
            _changed("fs-electric", "case-accel-75m", fsEvent="acceleration"), "case-accel-75m"),
        "NEW-FMU": new_engine.unsupported(fmu, WLTC),
        "NEW-BALANCE": new_engine.unsupported(load_example("hybrid-car"), "case-udds"),
        "NEW-CELLS": new_engine.unsupported(cells, WLTC),
        "NEW-LIMITS": new_engine.unsupported(studies._point_project(
            load_example("bev-car"), WLTC, {"el-ambient": {"pressure_kPa": -50.0}}), WLTC),
    }
    assert {code: why.code for code, why in seen.items()} == {code: code for code in seen}
    assert all(why.why for why in seen.values())
    assert set(seen) | {"NEW-MISSING", "NEW-BUILD", "NEW-FAILED", "NEW-ERROR"} == set(new_engine.CODES)


def test_a_failure_is_coded_by_who_refused():
    assert new_engine.failure(ModelError(["No vehicle."])).code == "NEW-BUILD"
    assert new_engine.failure(KeyError("el-x")).code == "NEW-ERROR"


def _no_today(*args, **kwargs):
    raise AssertionError("strict mode ran the case on today's engine")


def test_strict_mode_runs_nothing_on_today_engine(monkeypatch):
    monkeypatch.setenv(new_engine.ENV_VAR, "new")
    monkeypatch.setenv(new_engine.STRICT_VAR, "1")
    result = new_engine.simulate(load_example("fs-electric"), "case-autocross", None, None, _no_today)
    assert result.status == "failed" and not result.channels
    assert [m.level for m in result.messages] == ["error"]
    assert "[NEW-LAP]" in result.messages[0].text


def test_strict_mode_does_not_fall_back_after_a_failure(monkeypatch):
    def broken(project, case_id, *live):
        raise KeyError("el-x")

    monkeypatch.setenv(new_engine.ENV_VAR, "new")
    monkeypatch.setenv(new_engine.STRICT_VAR, "1")
    monkeypatch.setattr(new_engine, "unsupported", lambda *args: None)
    monkeypatch.setattr(new_engine, "run_new", broken)
    result = new_engine.simulate(load_example("bev-car"), WLTC, None, None, _no_today)
    assert result.status == "failed" and "[NEW-ERROR]" in result.messages[0].text
    monkeypatch.delenv(new_engine.STRICT_VAR)
    fell = new_engine.simulate(load_example("bev-car"), WLTC, None, None,
                               lambda *args: new_engine.refused(WLTC, new_engine.Refusal("X", "y")))
    assert "[NEW-ERROR]" in fell.messages[0].text and "today's engine" in fell.messages[0].text


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
    assert "[NEW-MISSING]" in result.messages[0].text


@needs_engine
def test_a_failing_new_engine_falls_back_with_a_warning(monkeypatch, today):
    monkeypatch.setenv(new_engine.ENV_VAR, "new")

    def broken(*args, **kwargs):
        e = lightsim_engine.ModelError("[RUN-INTEGRATOR] the integrator gave up at t = 12 s")
        e.diagnostics = [{"code": "RUN-INTEGRATOR", "severity": "error"}]
        raise e

    monkeypatch.setattr(lightsim_engine, "build", broken)
    result = simulate(load_example("bev-car"), WLTC)
    first = result.messages[0]
    assert first.level == "warning"
    assert "RUN-INTEGRATOR" in first.text and "today's engine" in first.text
    assert "[NEW-FAILED]" in first.text
    monkeypatch.setenv(new_engine.STRICT_VAR, "1")
    strict = simulate(load_example("bev-car"), WLTC)
    assert strict.status == "failed" and not strict.channels
    assert "[NEW-FAILED]" in strict.messages[0].text and "RUN-INTEGRATOR" in strict.messages[0].text
    assert result.status == "warning"
    assert [s.key for s in result.summary] == [s.key for s in today.summary]
    assert [s.value for s in result.summary] == [s.value for s in today.summary]
