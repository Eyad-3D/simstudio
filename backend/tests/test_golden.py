"""Golden regression: the bundled examples must reproduce their fixtures —
at the shipped step and at a finer solver step — within tolerance bands
(see golden_compare.py). A failure prints what moved, old → new.

Regenerate deliberately, after an intended change, with a reason that
becomes a CHANGES.md entry: python tests/update_golden.py --reason '<why>'
Set SIMSTUDIO_GOLDEN_EXACT=1 to demand identical results (pure refactors).
Set GOLDEN_REPORT=<file> to collect the diff reports (CI's job summary).
"""
import os

import pytest
from golden_compare import (
    CASES,
    CHANGES,
    EXAMPLES,
    compare,
    fixture_path,
    headline_tol,
    load,
    report,
    run,
    snapshot,
)

from app.schemas import Channel, SimMessage, SimResult, SummaryValue


def _collect(text: str) -> None:
    target = os.environ.get("GOLDEN_REPORT")
    if target:
        with open(target, "a", encoding="utf-8") as f:
            f.write(text + "\n\n")


@pytest.mark.parametrize("project_id,case_id,variant", CASES)
def test_example_matches_golden(project_id, case_id, variant):
    path = fixture_path(project_id, case_id, variant)
    assert path.exists(), f"missing fixture {path} — run tests/update_golden.py --reason '<why>'"
    cmp = compare(load(path), run(project_id, case_id, variant))
    text = report(cmp)
    _collect(text)
    if not cmp.ok:
        pytest.fail(f"{text}\n\nIf this change is intended, regenerate the fixtures: "
                    f"python tests/update_golden.py --reason '<why>'", pytrace=False)


@pytest.mark.parametrize("project_id,case_id", EXAMPLES)
def test_the_shipped_step_is_converged(project_id, case_id):
    """The headline numbers at the shipped step (10 ms solver step) agree
    with those at the 5 ms step within the headline bands."""
    shipped = load(fixture_path(project_id, case_id, "shipped"))["summary"]
    fine = load(fixture_path(project_id, case_id, "fine"))["summary"]
    assert shipped.keys() == fine.keys()
    for label, (value, unit) in fine.items():
        if label != "Simulated duration":
            assert abs(shipped[label][0] - value) <= headline_tol(unit, value), (label, shipped[label], value)


def test_every_fixture_names_its_changelog_entry():
    headings = {line[3:].strip() for line in CHANGES.read_text(encoding="utf-8").splitlines()
                if line.startswith("## ")}
    for project_id, case_id, variant in CASES:
        fixture = load(fixture_path(project_id, case_id, variant))
        assert fixture["format"] == 2
        assert fixture["change"] in headings, (
            f"{fixture_path(project_id, case_id, variant).name} was regenerated for "
            f"'{fixture['change']}', which has no entry in CHANGES.md")


# ---- the comparison itself (synthetic runs) -----------------------------------

def _result(values: dict[str, list[float]], summary: dict[str, float], status="success"):
    return SimResult(
        caseId="case-city", status=status,
        messages=[SimMessage(level="info", text="solved")],
        channels=[Channel(elementId=key.split(":")[0], portId=key.split(":")[1], label=key,
                          unit="kW", timeSeries=[{"t": float(i), "value": v} for i, v in enumerate(vs)])
                  for key, vs in values.items()],
        summary=[SummaryValue(label=k, value=v, unit="kWh") for k, v in summary.items()])


def test_the_comparison_tolerates_noise_and_shifts_but_not_changes():
    ramp = [float(min(i, 40)) for i in range(61)]  # 0 → 40, then flat
    step = [0.0 if i < 30 else 1.0 for i in range(61)]  # a gear-like step at t = 30 s
    base = {"a:power": ramp, "b:gear": step}
    fixture = snapshot("bev-car", "case-city", result=_result(base, {"Energy": 10.0}))

    same = compare(fixture, _result(base, {"Energy": 10.0}))
    assert same.identical and same.ok

    noisy = compare(fixture, _result({"a:power": [v + 0.1 for v in ramp],  # 0.25 % of the range
                                      "b:gear": [0.0] + step[:-1]},  # the step 1 s later
                                     {"Energy": 10.01}))  # 0.1 %
    assert noisy.within_tolerance and not noisy.identical
    assert "within tolerance" in report(noisy) and noisy.moved == ["a:power", "b:gear"]

    moved = compare(fixture, _result({"a:power": [v * 1.05 for v in ramp], "b:gear": step},
                                     {"Energy": 10.2}))
    assert not moved.within_tolerance
    text = report(moved)
    assert "OUT OF TOLERANCE" in text
    assert "| Energy | 10 kWh | 10.2 kWh | +0.2 (+2.00 %) | ±0.02 | **out** |" in text
    assert "| a:power | t = 50 s | 40 kW | 42 kW | 2 |" in text
