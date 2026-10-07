"""A hybrid's cycle case runs charge-balanced (ENG-33): it is run again from
the charge its battery ended with until the battery's stored energy changes
by less than 1 % of the fuel's energy, so its fuel figure no longer depends
on the start SOC."""
from __future__ import annotations

import pytest

from app.solver import balance, simulate
from app.storage import load_example
from tests.helpers import example_result

CASE = "case-mixed"


def value(result, label: str) -> float:
    return next(s.value for s in result.summary if s.label == label)


def hybrid(start_soc=None, balanced=None):
    p = load_example("hybrid-car")
    case = next(c for c in p.cases if c.id == CASE)
    case.chargeBalance = balanced
    if start_soc is not None:
        batt = next(e for s in p.systems for e in s.elements if e.componentDefId == "battery.generic")
        case.parameterOverrides.setdefault(batt.id, {})["initial_soc_pct"] = start_soc
    return p


def test_the_hand_balanced_example_needs_one_run():
    r = example_result("hybrid-car", CASE)
    assert value(r, "Charge balance runs") == 1
    assert abs(value(r, "Battery energy change, share of fuel energy")) < 1.0
    assert "Charge-balanced in 1 run" in r.messages[1].text


@pytest.mark.parametrize("soc", [30, 70])
def test_any_start_soc_gives_the_balanced_fuel_figure(soc):
    hand = value(example_result("hybrid-car", CASE), "Fuel consumption")
    r = simulate(hybrid(soc), CASE)
    assert r.status == "success"
    assert value(r, "Fuel consumption") == pytest.approx(hand, rel=0.01)
    assert value(r, "Charge balance runs") >= 2
    assert value(r, "HV Battery — charge-balanced start SOC") == pytest.approx(52.0, abs=0.5)


def test_switched_off_it_runs_once_from_the_set_charge():
    r = simulate(hybrid(30, balanced=False), CASE)
    assert not any(s.label == "Charge balance runs" for s in r.summary)
    hand = value(example_result("hybrid-car", CASE), "Fuel consumption")
    assert value(r, "Fuel consumption") > hand * 1.05  # it refills the battery with fuel


def test_an_electric_car_is_not_balanced():
    r = example_result("bev-car", "case-city")
    assert not any("charge-balanced" in s.label for s in r.summary)


def test_unsettled_runs_give_a_charge_corrected_figure(monkeypatch):
    monkeypatch.setattr(balance, "MAX_RUNS", 2)
    monkeypatch.setattr(balance, "BALANCE_SHARE", 1e-12)
    r = simulate(hybrid(30), CASE)
    assert r.status == "warning"
    assert any("did not settle" in m.text for m in r.messages)
    hand = value(example_result("hybrid-car", CASE), "Fuel consumption")
    assert value(r, "Fuel consumption, charge-corrected") == pytest.approx(hand, rel=0.01)
    soc = next(s for s in r.summary if s.label.endswith("charge-balanced start SOC"))
    assert soc.notValid == "charge balancing did not settle"


def test_the_line_through_the_runs():
    assert balance._fit_zero([(-1.0, 3.0), (1.0, 5.0)]) == pytest.approx(4.0)
    assert balance._fit_zero([(1.0, 5.0)]) is None
