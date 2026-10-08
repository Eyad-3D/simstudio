"""CON-06: one-click vehicle tests on the model as it is."""
from __future__ import annotations

import pytest
from fastapi.testclient import TestClient

from app import vehicle_tests
from app.main import app
from app.storage import load_example


def _rows(example: str, *tests: str) -> dict:
    out = vehicle_tests.run_tests(load_example(example), list(tests))
    return {r["what"]: r for r in out["rows"]}


def test_bev_acceleration_matches_its_performance_case_and_top_speed_names_the_limit():
    rows = _rows("bev-car", "accel_0_100", "accel_80_120", "top_speed")
    # the hand-built performance run gives 7.10 s (VAL-39); within 0.2 s
    assert rows["0-100 km/h"]["value"] == pytest.approx(7.10, abs=0.2)
    assert 3.0 < rows["80-120 km/h"]["value"] < rows["0-100 km/h"]["value"]
    assert rows["Top speed"]["value"] == pytest.approx(160, abs=1)
    assert rows["Top speed"]["note"] == "limited by E-Motor's maximum speed (16,000 1/min)"
    assert all(not r["note"] for w, r in rows.items() if w != "Top speed")


def test_constant_speed_consumption_rises_with_speed():
    rows = _rows("bev-car", "constant_speed")
    kwh = [rows[f"Consumption at {v} km/h"]["value"] for v in vehicle_tests.STEADY_SPEEDS]
    assert kwh == sorted(kwh) and 8 < kwh[0] < kwh[-1] < 25
    assert rows["Range at 120 km/h"]["value"] < rows["Range at 50 km/h"]["value"]


def test_the_coast_down_returns_the_road_load_the_model_was_built_from():
    """The hybrid takes EPA's coefficients as its road load (A 68.64 N,
    B 0.9093 N/(km/h), C 0.025078 N/(km/h)²): a coast-down gives them back."""
    rows = _rows("hybrid-car", "coast_down")
    assert rows["Coast-down A (f0)"]["value"] == pytest.approx(68.64, rel=0.02)
    assert rows["Coast-down B (f1)"]["value"] == pytest.approx(0.9093, rel=0.02)
    assert rows["Coast-down C (f2)"]["value"] == pytest.approx(0.025078, rel=0.02)


def test_gradeability_finds_a_grade_the_car_holds():
    rows = _rows("bev-car", "gradeability")
    grade = rows["Steepest grade at 30 km/h"]["value"]
    assert 20 < grade <= 60


def test_the_api_runs_chosen_tests_and_refuses_unknown_ones():
    c = TestClient(app)
    body = {"project": load_example("bev-car").model_dump(mode="json"), "tests": ["accel_0_100"]}
    r = c.post("/api/vehicle-tests", json=body).json()
    assert [row["what"] for row in r["rows"]] == ["0-100 km/h"] and "not a certified" in r["note"]
    body["tests"] = ["moon_landing"]
    assert c.post("/api/vehicle-tests", json=body).status_code == 400


def test_a_fuel_cell_car_gets_no_range_from_its_battery_alone(monkeypatch):
    from helpers import fuel_cell_car

    monkeypatch.setattr(vehicle_tests, "STEADY_SPEEDS", (50,))
    rows = {r["what"]: r for r in vehicle_tests.run_tests(fuel_cell_car(setpoint_kW=1.5), ["constant_speed"])["rows"]}
    assert "Range at 50 km/h" not in rows
    assert "without the energy of Fuel Cell Stack 'Fuel Cell'" in rows["Consumption at 50 km/h"]["note"]
