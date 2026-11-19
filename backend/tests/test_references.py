"""Expected values and the automatic hand calculations (VAL-35)."""
from __future__ import annotations

import pytest
from helpers import dbc, el

from app.schemas import Project, ReferenceValue, SummaryValue
from app.solver import core, simulate
from app.solver.references import check_references, grade_gap
from app.storage import load_example


def _row(label="Time to 100 km/h", value=7.10, unit="s", not_valid=None):
    return SummaryValue(label=label, value=value, unit=unit, notValid=not_valid)


@pytest.mark.parametrize("diff,tol,grade", [
    (0.0, 1.0, "within"), (1.0, 1.0, "within"), (-1.0, 1.0, "within"),
    (1.5, 1.0, "near"), (-2.0, 1.0, "near"), (2.01, 1.0, "outside"), (0.1, 0.0, "outside"),
])
def test_grades_are_green_within_amber_to_twice_red_beyond(diff, tol, grade):
    """frontend/src/references.test.ts holds the same table."""
    assert grade_gap(diff, tol) == grade


def test_a_reference_in_percent_and_one_in_the_unit():
    pct = ReferenceValue(kpi="Time to 100 km/h", value=7.3, tolerance=5, source="maker's figure")
    abs_ = ReferenceValue(kpi="Time to 100 km/h", value=7.3, tolerance=0.1, tolerancePct=False)
    a, b = check_references([pct, abs_], [_row()])
    assert (a.value, a.reference, a.unit, a.difference) == (7.10, 7.3, "s", pytest.approx(-0.2))
    assert a.differencePct == pytest.approx(-2.74, abs=0.01)
    assert a.tolerance == pytest.approx(0.365)
    assert (a.grade, a.source, a.automatic) == ("within", "maker's figure", False)
    assert (b.tolerance, b.grade) == (pytest.approx(0.1), "near")


def test_a_missing_or_not_valid_value_says_so():
    ref = ReferenceValue(kpi="Time to 100 km/h", value=7.3)
    assert check_references([ref], [])[0].grade == "missing"
    checked = check_references([ref], [_row(not_valid="cycle not followed")])[0]
    assert checked.grade == "not valid" and "cycle not followed" in checked.note


def test_references_travel_with_the_case_and_come_back_graded():
    proj = load_example("bev-car")
    case = proj.cases[0]
    case.references = [ReferenceValue(kpi="Consumption", value=11.0, tolerance=2,
                                      source="hand calculation, 2026")]
    case.duration = 60
    back = Project.model_validate(proj.model_dump())  # saved and opened again
    assert back.cases[0].references[0].source == "hand calculation, 2026"
    result = simulate(back, case.id)
    mine = [r for r in result.references if not r.automatic]
    assert [(r.label, r.reference) for r in mine] == [("Consumption", 11.0)]


def test_every_run_gets_the_hand_calculations_and_the_examples_keep_to_them():
    result = simulate(load_example("bev-car"), "case-city")
    auto = [r for r in result.references if r.automatic]
    assert [r.bound for r in auto] == ["at most", "at least"]
    assert all(r.grade == "within" for r in auto)
    top = auto[0]
    # 16,000 1/min ÷ 12.8 × 0.3488 m = 164.4 km/h
    assert top.reference == pytest.approx(16000 / 12.8 * 2 * 3.14159265 / 60 * 0.3488 * 3.6,
                                          rel=1e-3)


def test_a_motor_driven_past_its_maximum_speed_is_flagged():
    """Probe: no brakes, a 30 % downhill and a 250 km/h target drive the
    E-Motor far past its 16,000 1/min."""
    proj = load_example("bev-car")
    proj.systems[0].elements.append(el("el-road", "signal.road_profile", "Road",
                                       profile="0:-30; 5000:-30"))
    proj.dataBusConnections += [dbc(91, "el-vehicle", "sig_distance", "el-road", "sig_distance_in"),
                                dbc(92, "el-road", "sig_grade", "el-vehicle", "sig_grade_in")]
    for e in proj.systems[0].elements:
        if e.id == "el-task":
            e.parameterOverrides = {"profile": "0:0; 5:250; 120:250"}
        if e.componentDefId == "mech.brake":
            e.parameterOverrides["max_torque_Nm"] = 0
    proj.cases[0].duration = 60
    top = next(r for r in simulate(proj, "case-city").references if r.bound == "at most")
    assert top.grade == "outside" and top.value > 2 * top.reference


def test_energy_from_nowhere_is_flagged(monkeypatch):
    """Probe: an engine fault that loses the battery's energy count (the car
    still drives) must not pass the energy bound."""
    real = core.hand_checks

    def no_battery_energy(ctx, channels):
        for b in ctx.batteries.values():
            b.energy_out_wh = b.energy_in_wh = 0.0
        return real(ctx, channels)

    monkeypatch.setattr(core, "hand_checks", no_battery_energy)
    proj = load_example("bev-car")
    proj.cases[0].duration = 120
    energy = next(r for r in simulate(proj, "case-city").references if r.bound == "at least")
    assert energy.grade == "outside" and energy.value == 0
