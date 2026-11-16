"""LIGHTSIM_ENGINE=new runs the cases the new engine can on it, behind a flag
(app/solver/new_engine.py); today's engine stays the default.

The tests that run the new engine need the compiled module ``lightsim_engine``
(engine/crates/lsim-py, built with maturin). CI's backend jobs do not build
it, so they skip there; the Windows engine job builds and installs it and
runs them with ``LIGHTSIM_REQUIRE_NEW_ENGINE=1``, where a missing module
fails them instead of skipping them (review finding 18)."""
from __future__ import annotations

import os
import re

import pytest

from app import studies
from app.schemas import ElementInstance
from app.solver import new_engine, simulate
from app.solver.network import ModelError
from app.storage import load_example
from tests.helpers import series
from tests.test_distance_profile import lap_project

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


def _performance(profile: str, **vehicle):
    """The BEV car's WLTC case as a performance test: its Driving Task a step
    to the target (the registered probe of benchmarks/back_to_back, and what
    /api/vehicle-tests builds)."""
    project = load_example("bev-car")
    case = next(c for c in project.cases if c.id == WLTC)
    case.kind, case.duration = "performance", 40.0
    case.parameterOverrides = {"el-task": {"cycle": "", "profile": profile}}
    if vehicle:
        case.parameterOverrides["el-vehicle"] = vehicle
    return project


PERFORMANCE = {
    "a step from the start": ("0:100; 40:100", {}),
    "a step at 5 s": ("0:0; 5:100; 40:100", {}),
    "two steps": ("0:50; 10:150; 40:150", {}),
    "a car that starts at its target": ("0:100; 20:50; 40:50", {"initial_speed_kmh": 100}),
}


def _masked(result) -> list[str]:
    """The run's warnings and errors, their numbers left out."""
    return sorted(re.sub(r"[-+]?\d[\d.,]*(e[-+]?\d+)?", "#", m.text)
                  for m in result.messages if m.level in ("warning", "error"))


@needs_engine
@pytest.mark.parametrize("name", PERFORMANCE)
def test_new_engine_runs_a_performance_test_like_today(monkeypatch, name):
    """A performance test (full throttle up to the target, the PI after it)
    runs on the new engine and gives today's rows, status and warnings, its
    time to the target and its maximum speed within the two engines' own
    difference (0.3 % and 0.05 % on the probe: today's 10 ms against 1 ms
    steps differ by 0.35 %). Strict: nothing falls back to today's."""
    profile, vehicle = PERFORMANCE[name]
    project = _performance(profile, **vehicle)
    monkeypatch.setenv(new_engine.ENV_VAR, "today")
    today = simulate(project, WLTC)
    monkeypatch.setenv(new_engine.ENV_VAR, "new")
    monkeypatch.setenv(new_engine.STRICT_VAR, "1")
    assert new_engine.unsupported(project, WLTC) is None
    result = simulate(project, WLTC)
    assert result.messages[0].text.startswith("Run on the new engine"), texts(result)
    assert result.status == today.status, texts(result)
    assert _masked(result) == _masked(today)
    # today's two checks of its own integrator, which the new engine replaces
    # with its energy books' closure (golden/README.md)
    checks = {"energy_balance_residual_pct", "energy_balance_error_pct"}
    assert [(s.key, s.label, s.unit, s.notValid) for s in result.summary] \
        == [(s.key, s.label, s.unit, s.notValid) for s in today.summary if s.key not in checks]
    now = {s.key: s.value for s in result.summary}
    for s in today.summary:
        if s.key in ("time_to_target_s", "max_speed_kmh", "distance_km"):
            assert now[s.key] == pytest.approx(s.value, rel=0.01), s.key
    assert [(c.elementId, c.portId) for c in today.channels] \
        == [(c.elementId, c.portId) for c in result.channels
            if (c.elementId, c.portId) in {(t.elementId, t.portId) for t in today.channels}]


def _ends_at(project, case_id: str, **case):
    c = next(c for c in project.cases if c.id == case_id)
    for k, v in case.items():
        setattr(c, k, v)
    return project


@needs_engine
@pytest.mark.parametrize("name", ["a performance test to a distance", "a cycle to a distance"])
def test_new_engine_ends_a_case_at_its_distance_like_today(monkeypatch, name):
    """A case's Distance (or its Laps, a Driving Task over distance) ends the
    run where the vehicle has driven it, as today's: the engine ran the whole
    duration and gave a distance and consumption of the whole run before."""
    if name == "a performance test to a distance":
        project, case_id, far = _ends_at(_performance("0:100; 40:100"), WLTC, endDistance=300.0), WLTC, 0.3
    else:
        project, case_id, far = _ends_at(load_example("bev-car"), "case-city", endDistance=3000.0), "case-city", 3.0
    monkeypatch.setenv(new_engine.ENV_VAR, "today")
    today = simulate(project, case_id)
    monkeypatch.setenv(new_engine.ENV_VAR, "new")
    monkeypatch.setenv(new_engine.STRICT_VAR, "1")
    result = simulate(project, case_id)
    assert result.messages[0].text.startswith("Run on the new engine"), texts(result)
    assert result.status == today.status, texts(result)
    now, then = ({s.key: s.value for s in r.summary} for r in (result, today))
    assert now["distance_km"] == pytest.approx(far, abs=0.002)
    assert then["distance_km"] == pytest.approx(far, abs=0.002)
    # (today's last step ends past the line, the new run's at it: a step of 1 s or 10 ms)
    assert now["simulated_duration_s"] == pytest.approx(then["simulated_duration_s"], abs=1.0)
    assert now["consumption_kwh_per_100km"] == pytest.approx(then["consumption_kwh_per_100km"], rel=0.02)


@needs_engine
def test_new_engine_ends_a_case_after_its_laps_like_today(monkeypatch):
    """Two laps of a Driving Task over distance (2400 m): the case's Laps are
    given to the engine as the distance they make (test_distance_profile)."""
    project = lap_project()
    monkeypatch.setenv(new_engine.ENV_VAR, "today")
    today = simulate(project, "case")
    monkeypatch.setenv(new_engine.ENV_VAR, "new")
    monkeypatch.setenv(new_engine.STRICT_VAR, "1")
    result = simulate(project, "case")
    assert result.messages[0].text.startswith("Run on the new engine"), texts(result)
    assert result.status == today.status, texts(result)
    dist = [p["value"] for p in series(result, "veh", "sig_distance")]
    assert dist[-1] == pytest.approx(2 * 1200, abs=1.0)
    now, then = ({s.key: s.value for s in r.summary} for r in (result, today))
    assert now["simulated_duration_s"] == pytest.approx(then["simulated_duration_s"], abs=0.5)


@needs_engine
def test_a_performance_test_holds_full_throttle_up_to_its_target(monkeypatch):
    """The pedal is down from the step in the target until the speed gets
    there, and the car then holds the target (no overshoot of the speed)."""
    monkeypatch.setenv(new_engine.ENV_VAR, "new")
    monkeypatch.setenv(new_engine.STRICT_VAR, "1")
    result = simulate(_performance("0:0; 5:100; 40:100"), WLTC)
    series = {c.portId: [(p["t"], p["value"]) for p in c.timeSeries]
              for c in result.channels if c.elementId in ("el-vehicle", "el-driver")}
    reached = next(t for t, v in series["sig_speed"] if v >= 99.0)
    assert 6.0 <= reached <= 9.0  # 0-100 km/h in about 7 s (the profile ramps to 100 at 5 s)
    pedal = dict(series["sig_accel_pedal"])
    assert all(pedal[t] == 1.0 for t, _ in series["sig_speed"] if 1.0 < t < reached - 1.0)
    assert all(pedal[t] < 1.0 for t, _ in series["sig_speed"] if t > reached + 3.0)
    assert max(v for _, v in series["sig_speed"]) < 101.0


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
    single = _changed("hybrid-car", "case-udds", chargeBalance=False)
    sampled = load_example("bev-car")
    sampled.systems[0].elements.append(ElementInstance(
        id="pid", componentDefId="control.pid", label="PID", position={"x": 0, "y": 0},
        parameterOverrides={"sample_time_s": 0.5}))
    seen = {
        "NEW-NOT-FOUND": new_engine.unsupported(load_example("bev-car"), "no-such-case"),
        "NEW-LAP": new_engine.unsupported(load_example("fs-electric"), "case-autocross"),
        # (a kind the schema does not have yet: the four it has are all handled above it)
        "NEW-CASE-KIND": new_engine.unsupported(_changed("bev-car", WLTC, kind="drift"), WLTC),
        "NEW-FS-EVENT": new_engine.unsupported(
            _changed("fs-electric", "case-accel-75m", fsEvent="acceleration"), "case-accel-75m"),
        "NEW-BALANCE": new_engine.unsupported(load_example("hybrid-car"), "case-udds"),
        "NEW-CELLS": new_engine.unsupported(cells, WLTC),
        "NEW-SAMPLED": new_engine.unsupported(sampled, WLTC),
        "NEW-LIMITS": new_engine.unsupported(studies._point_project(
            load_example("bev-car"), WLTC, {"el-ambient": {"pressure_kPa": -50.0}}), WLTC),
        # quick mode's (a case's mode "quick")
        "QUICK-NOT-FOUND": new_engine.quick_unsupported(load_example("bev-car"), "no-such-case"),
        "QUICK-KIND": new_engine.quick_unsupported(load_example("fs-electric"), "case-accel-75m"),
        "QUICK-BALANCE": new_engine.quick_unsupported(load_example("hybrid-car"), "case-udds"),
        "QUICK-SAMPLED": new_engine.quick_unsupported(single, "case-udds"),
        "QUICK-END": new_engine.quick_unsupported(_changed("bev-car", WLTC, endLaps=2.0), WLTC),
        "QUICK-CELLS": new_engine.quick_unsupported(cells, WLTC),
        "QUICK-LIMITS": new_engine.quick_unsupported(studies._point_project(
            load_example("bev-car"), WLTC, {"el-ambient": {"pressure_kPa": -50.0}}), WLTC),
    }
    assert {code: why.code for code, why in seen.items()} == {code: code for code in seen}
    assert all(why.why for why in seen.values())
    # an FMU part is no reason any more (step 2's item 5)
    assert new_engine.unsupported(fmu, WLTC) is None or new_engine.unsupported(fmu, WLTC).code == "NEW-MISSING"
    assert new_engine.quick_unsupported(fmu, WLTC).code == "QUICK-SAMPLED"
    assert new_engine.quick_unsupported(sampled, WLTC).code == "QUICK-SAMPLED"
    assert set(seen) | {"NEW-MISSING", "NEW-BUILD", "NEW-FAILED", "NEW-ERROR", "NEW-TEXT", "QUICK-MISSING",
                        "QUICK-BUILD", "QUICK-FAILED", "QUICK-TABLE-TIME", "QUICK-ERROR",
                                "NEW-LIVE-ERROR-AXIS", "NEW-LIVE-PID", "NEW-LIVE-RESTART"} == set(new_engine.CODES)


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
