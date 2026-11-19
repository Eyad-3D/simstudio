"""CON-07: examples that each answer one engineering question, checked
against the public data they are built from."""
from __future__ import annotations

import pytest

from app.solver import simulate
from app.storage import load_example


def _rows(example: str, case: str) -> dict:
    result = simulate(load_example(example), case)
    assert result.status == "success", [m.text for m in result.messages]
    return {s.label: s.value for s in result.summary}


@pytest.mark.parametrize(("case", "epa_mpge"), [("case-udds", 185.3), ("case-hwfet", 170.1)])
def test_the_efficient_sedan_matches_epas_lab_tests_of_the_car(case, epa_mpge):
    """EPA's 2022 Test Car List: the Model 3 RWD's charge-depleting UDDS and
    highway tests, 185.3 and 170.1 MPGe (unadjusted, at the socket)."""
    mpge = _rows("aero-bev", case)["Fuel-economy equivalent (MPGe, AC)"]
    assert mpge == pytest.approx(epa_mpge, rel=0.05)


def test_low_drag_is_what_sets_the_sedan_apart():
    """The question the example answers: on WLTC the sedan, with EPA's lower
    road load and the same kind of motor, takes about 16 % less energy."""
    sedan = _rows("aero-bev", "case-wltc")["Consumption"]
    compact = _rows("bev-car", "case-wltc")["Consumption"]
    assert 0.75 < sedan / compact < 0.92
