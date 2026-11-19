"""Stage 0's reference problems (benchmarks/reference/, see benchmarks/README.md).

The exact answers are checked three ways: each balances its own energy,
each matches a very fine numerical integration of the problem's stated
equations (benchmarks/reference/verify.py), and the checkpoints written in
the problem files are the exact answers to the last digit. Today's engine
then runs every problem it can express and must stay within the ceilings
of benchmarks/baselines/lightsim-py.toml (twice its measured errors): the
ratchet that tells when a change makes it less accurate. Whether it meets
the new engine's targets is in the report (python -m benchmarks.run), not
here."""
from __future__ import annotations

import sys
import tomllib
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:  # the benchmarks package lives at the repository root
    sys.path.insert(0, str(ROOT))

from benchmarks import reference  # noqa: E402
from benchmarks.engines import lightsim_py as engine  # noqa: E402
from benchmarks.reference import export, verify  # noqa: E402
from benchmarks.reference.compare import compare, first_crossing  # noqa: E402
from benchmarks.reference.exact import solve  # noqa: E402

PROBLEMS = reference.load_all()
BY_ID = {p.id: p for p in PROBLEMS}
BASELINE = tomllib.loads((ROOT / "benchmarks" / "baselines" / f"{engine.NAME}.toml")
                         .read_text(encoding="utf-8"))
EXPRESSED = [(p, e) for p in PROBLEMS for e in engine.EXPRESSIONS.get(p.id, [])]


def test_the_suite_covers_what_a_vehicle_simulator_must_get_right():
    assert len(BY_ID) == len(PROBLEMS)
    assert {p.domain for p in PROBLEMS} >= {"electrical", "battery", "motor", "mechanical",
                                           "vehicle", "thermal"}
    for p in PROBLEMS:
        assert p.statement and p.equations and p.solution, p.id
        assert p.compared("signal") and p.energy_terms, p.id
        for name, unit in p.units.items():
            assert unit, f"{p.id}: {name} has no unit"


@pytest.mark.parametrize("prob", PROBLEMS, ids=lambda p: p.id)
def test_every_compared_quantity_has_an_exact_answer(prob):
    ex = solve(prob)
    for c in prob.compare:
        if c.kind == "event":
            assert c.name in ex.events and c.signal in ex.signals, c.name
        else:
            assert c.name in ex.signals, c.name
    for name in prob.energy_terms:
        assert name in ex.signals, name
        assert ex.at(name, 0.0) == pytest.approx(0.0, abs=1e-9), f"{name} starts at 0"


@pytest.mark.parametrize("prob", PROBLEMS, ids=lambda p: p.id)
def test_the_exact_answer_balances_its_energy(prob):
    ex = solve(prob)
    scale = max(abs(ex.at(n, prob.t_end)) for n in prob.energy_terms)
    for t in prob.output_times()[:: max(1, len(prob.output_times()) // 200)]:
        residual = (sum(ex.at(n, t) for n in prob.sources)
                    - sum(ex.at(n, t) for n in prob.sinks))
        assert abs(residual) <= 1e-12 * scale, t


@pytest.mark.parametrize("prob", PROBLEMS, ids=lambda p: p.id)
def test_each_event_is_where_its_signal_reaches_its_level(prob):
    """The event times exact.py derives are the crossings the problem file
    defines (an engine without its own events is measured by those)."""
    ex = solve(prob)
    for c in prob.compared("event"):
        t_ev, sig = ex.events[c.name], ex.signals[c.signal]
        scale = max(abs(sig(t)) for t in prob.output_times())
        assert sig(t_ev) == pytest.approx(c.level, abs=1e-9 * scale), c.name
        before = sig(t_ev * (1 - 1e-6)) - c.level
        if c.direction == "rising":
            assert before < 0, c.name
        elif c.direction == "falling":
            assert before > 0, c.name


@pytest.mark.parametrize("prob", PROBLEMS, ids=lambda p: p.id)
def test_the_checkpoints_in_the_problem_file_are_the_exact_answer(prob):
    """Regenerate with python -m benchmarks.reference.export --checkpoints."""
    want, have = export.checkpoints(prob), prob.checkpoints
    assert have, "no [checkpoints] section"
    assert have["times"] == want["times"]
    for name in export.quantities(prob):
        assert have[name] == pytest.approx(want[name], rel=1e-12, abs=1e-12), name
    assert have.get("events", {}) == pytest.approx(want["events"], rel=1e-12)


@pytest.mark.parametrize("prob", PROBLEMS, ids=lambda p: p.id)
def test_the_exact_answer_matches_a_fine_integration(prob):
    gaps = verify.verify(prob)
    worst = max(gaps, key=gaps.get)
    assert gaps[worst] < 1e-7, f"{worst}: {gaps[worst]:.2e}"


def test_crossings_are_interpolated_between_samples():
    t = [0.0, 1.0, 2.0, 3.0, 4.0]
    up_down = [0.0, 2.0, 4.0, 2.0, 0.0]
    assert first_crossing(t, up_down, 3.0, "rising") == pytest.approx(1.5)
    assert first_crossing(t, up_down, 3.0, "falling") == pytest.approx(2.5)
    assert first_crossing(t, up_down, 3.0) == pytest.approx(1.5)
    assert first_crossing(t, up_down, 5.0) is None
    assert first_crossing(t, [9.0, 6.0, 3.0, 0.0, 0.0], 0.0) == pytest.approx(3.0)


def test_every_problem_is_either_expressed_or_explained():
    expressed, cannot = set(engine.EXPRESSIONS), set(engine.CANNOT)
    assert not expressed & cannot
    assert expressed | cannot == set(BY_ID)
    assert all(engine.CANNOT[k] for k in cannot)


@pytest.mark.parametrize("prob,expr", EXPRESSED, ids=lambda x: getattr(x, "id", None)
                         or x.label.replace(" ", "-"))
def test_todays_engine_stays_within_its_baseline(prob, expr):
    out = compare(prob, engine.run(prob, expr))
    ceilings = BASELINE[prob.id][expr.label]
    for row in out.rows:
        if row.error_rel is None and row.passed is None:
            assert row.name not in ceilings, f"{row.name} is no longer given"
            continue
        ceiling = ceilings.get(row.name)
        assert ceiling is not None, f"{row.name} has no ceiling: regenerate the baseline"
        if ceiling == "never":
            assert row.error_rel is None and row.passed is False, (
                f"{row.name} is now reached ({row.value}): regenerate the baseline")
        else:
            assert row.error_rel is not None and row.error_rel <= ceiling, (
                f"{row.name}: {row.error_rel} > {ceiling}")
    if out.closure_rel is not None:
        assert out.closure_rel <= ceilings["closure"], (out.closure_rel, ceilings["closure"])


def test_the_reference_models_are_checked_like_a_user_s():
    """The adapter runs no project past the Data Checks: a model they refuse
    stops it, and the coast-down and gear-schedule models of the reference
    problems pass them (they were refused once, which is why the adapter
    used to run past the errors)."""
    at_rest = engine.Build("at-rest").part("veh", "vehicle.body", initial_speed_kmh=0.0)
    with pytest.raises(engine.DataChecksRefused, match="will not move"):
        engine.run_project(at_rest.project(1.0, 0.1))
    coasting = engine.Build("coasting").part("veh", "vehicle.body", initial_speed_kmh=36.0)
    assert engine.run_project(coasting.project(1.0, 0.1)).result.status != "failed"
    for prob in (BY_ID["veh_coastdown"], BY_ID["mech_gear_change"]):
        for expr in engine.EXPRESSIONS[prob.id]:
            trace = engine.run(prob, expr)
            assert not [m for m in trace.messages if "Data Check" in m], (prob.id, expr.label)
