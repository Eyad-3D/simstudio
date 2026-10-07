"""The reference suite (VAL-05): real electric cars against EPA's tests.

Each case of backend/validation/ runs the EPA city (UDDS) and highway
(HWFET) cycles and must land within its tolerance (15 %, blind) of EPA's
unadjusted energy at the wall; a virtual coast-down must give back EPA's
road load within 2 %; the exact-answer tier must be within 0.5 %; and
halving the solver step must move the city energy by less than 0.5 %
(checked on one car here, on all four for docs/VALIDATION-STATUS.md).
The measured gaps are in docs/VALIDATION-STATUS.md."""
from __future__ import annotations

from functools import lru_cache

import pytest

from validation import suite

CASES = {c["id"]: c for c in suite.load_cases()}


@lru_cache(maxsize=None)
def _run(case_id: str, cycle: str):
    return suite.run_cycle(CASES[case_id], cycle)


def test_the_suite_lists_its_files_and_every_case_states_its_basis():
    assert len(CASES) == len(suite.SUITE["cases"]) >= 3
    for case in CASES.values():
        assert case["mode"] in ("blind", "calibrated") and case["mode_note"]
        assert case["tolerance_pct"] > 0
        for key in ("source",):
            assert case["epa"][key] in suite.SUITE["sources"]
            assert case["fastsim"][key] in suite.SUITE["sources"]
            assert case["target"][key] in suite.SUITE["sources"]


@pytest.mark.parametrize("case_id", CASES)
def test_the_drive_cycles_are_the_ones_the_case_was_written_for(case_id):
    for cycle, sha in CASES[case_id]["cycles"].items():
        assert suite.cycle_sha256(cycle) == sha


def test_mpge_turns_into_wh_per_km_as_epa_counts_it():
    # the Model 3 RWD's 185.3 mpge on the UDDS is 113.0 Wh/km at the wall
    assert suite.mpge_to_wh_per_km(185.3) == pytest.approx(113.02, abs=0.01)


@pytest.mark.parametrize("case_id", CASES)
@pytest.mark.parametrize("cycle", ["udds", "hwfet"])
def test_energy_at_the_wall_is_within_the_case_tolerance_of_epa(case_id, cycle):
    r = _run(case_id, cycle)
    assert r.status == "success", r.messages
    assert abs(r.gap_pct) <= CASES[case_id]["tolerance_pct"], (
        f"{case_id} {cycle}: LightSim {r.wh_per_km:.1f} Wh/km against EPA "
        f"{r.target:.1f} ({r.gap_pct:+.1f} %)")


@pytest.mark.parametrize("case_id", CASES)
def test_a_virtual_coast_down_gives_back_epa_road_load(case_id):
    result = suite.coastdown(CASES[case_id])
    assert result["points"] > 50
    assert result["worst_pct"] <= suite.RULES["coastdown_pct"]


@pytest.mark.parametrize("ex", suite.EXACT["cases"], ids=lambda e: e["id"])
def test_exact_answer_tier(ex):
    r = suite.run_exact(ex)
    assert abs(r["gap_pct"]) <= suite.EXACT["tolerance_pct"], r


def test_halving_the_solver_step_moves_the_city_energy_by_less_than_half_a_percent():
    case = CASES["epa-2022-tesla-model3-rwd"]
    fine = suite.run_cycle(case, "udds", step=0.005, every=200)
    coarse = _run(case["id"], "udds")
    change = 100 * (coarse.wh_per_km - fine.wh_per_km) / fine.wh_per_km
    assert abs(change) <= suite.RULES["step_halving_pct"]
