"""Every example carries a card and its own stored reference results, and
checks itself against them (CON-15)."""
from __future__ import annotations

import re

import pytest
from fastapi.testclient import TestClient
from helpers import example_result

from app import reference_results
from app.main import app
from app.storage import load_example

EXAMPLES = ("aero-bev", "bev-car", "fs-electric", "hybrid-car")
CHANGES = reference_results.DIR.parents[1] / "tests" / "golden" / "CHANGES.md"
client = TestClient(app)


def _cases(project_id: str):
    return [c for c in load_example(project_id).cases if not c.realtimeFactor]


@pytest.mark.parametrize("project_id", EXAMPLES)
def test_every_example_has_a_complete_card(project_id):
    card = load_example(project_id).card
    assert card is not None, f"{project_id} has no card"
    assert card.question.endswith("?")
    assert card.learn and card.narrative and card.features and card.tags
    assert card.status in ("demo", "plausibility-checked", "validated")
    assert card.author and card.version and card.licence and card.runTimeS


@pytest.mark.parametrize("project_id", EXAMPLES)
def test_every_case_has_expected_values_with_a_band_and_a_source(project_id):
    for case in load_example(project_id).cases:
        assert case.references, f"{project_id} {case.name} has no expected value"
        for ref in case.references:
            assert ref.tolerance > 0 and ref.source.strip()


@pytest.mark.parametrize("project_id,case_id",
                         [(p, c.id) for p in EXAMPLES for c in _cases(p)])
def test_each_case_lands_within_its_expected_values(project_id, case_id):
    result = example_result(project_id, case_id)
    mine = [r for r in result.references if not r.automatic]
    assert mine and all(r.grade == "within" for r in mine), [r.model_dump() for r in mine]
    assert all(r.grade == "within" for r in result.references if r.automatic)


@pytest.mark.parametrize("project_id", EXAMPLES)
def test_the_stored_results_are_explained_in_the_changelog(project_id):
    data = reference_results.load(project_id)
    assert data is not None, "run tests/update_golden.py --reason '...'"
    note = data["creation"]["note"]
    assert re.search(rf"^## {re.escape(note)}$", CHANGES.read_text(encoding="utf-8"), re.M), (
        f"{project_id}'s stored results name '{note}', which tests/golden/CHANGES.md lacks")
    assert set(data["cases"]) == {c.id for c in _cases(project_id)}


@pytest.mark.parametrize("project_id,case_id",
                         [(p, c.id) for p in EXAMPLES for c in _cases(p)])
def test_a_fresh_run_gives_the_stored_summary(project_id, case_id):
    """A change that moves an example's results must rewrite the stored
    results (tests/update_golden.py --reason ...), with its CHANGES.md note."""
    stored = reference_results.load(project_id)["cases"][case_id]
    result = example_result(project_id, case_id)
    assert result.status == stored["status"]
    fresh = {s.label: s.value for s in result.summary}
    for row in stored["summary"]:
        assert fresh.get(row["label"]) == pytest.approx(row["value"], rel=2e-3, abs=0.02), row


def test_the_app_gets_the_stored_runs_of_an_example():
    runs = client.get("/api/examples/bev-car/reference").json()
    assert [r["caseId"] for r in runs] == [c.id for c in _cases("bev-car")]
    speed = next(c for c in runs[0]["result"]["channels"] if c["portId"] == "sig_speed"
                 and c["elementId"] == "el-vehicle")
    assert len(speed["timeSeries"]) == 601  # 600 s, every second
    assert runs[0]["result"]["messages"][0]["text"].startswith("Stored result")
    assert client.get("/api/examples/nothing/reference").status_code == 404
