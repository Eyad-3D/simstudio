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


def _fake_runs(monkeypatch, speed=None, status="success", error=""):
    """Every test run gives this Vehicle speed trace (t → km/h), or fails."""
    from app.schemas import Channel, SimMessage, SimResult

    def run(project, case, extra=None):
        veh = next(e.id for s in project.systems for e in s.elements
                   if e.componentDefId == "vehicle.body")
        channels = [Channel(elementId=veh, portId="sig_speed", label="Speed", unit="km/h",
                            timeSeries=[{"t": t / 10, "value": speed(t / 10)}
                                        for t in range(int(case.duration * 10) + 1)])
                    ] if speed else []
        messages = [SimMessage(level="error", text=error)] if error else []
        return SimResult(caseId=case.id, status=status, messages=messages, channels=channels)

    monkeypatch.setattr(vehicle_tests, "_run", run)


def test_a_top_speed_still_falling_is_not_settled_and_the_peak_stands_apart(monkeypatch):
    """A hybrid's battery boost fades: its highest speed is not one it holds,
    and 'limited by the power' would not be the reason."""
    _fake_runs(monkeypatch, lambda t: min(2 * t, 204 - 0.12 * max(0.0, t - 102)))
    rows = _rows("bev-car", "top_speed")
    assert rows["Top speed"]["value"] is None
    assert rows["Top speed"]["note"].startswith("not settled in 120 s: still falling")
    assert rows["Highest speed reached"]["value"] == pytest.approx(204, abs=0.5)
    _fake_runs(monkeypatch, lambda t: min(3 * t, 150.0))  # held: the mean of the last 10 s
    rows = _rows("bev-car", "top_speed")
    assert rows["Top speed"]["value"] == 150.0 and "Highest speed reached" not in rows


def test_a_failed_run_gives_no_figure_and_says_why(monkeypatch):
    _fake_runs(monkeypatch, status="failed", error="Script sandbox stopped during start-up")
    rows = _rows("bev-car", "top_speed", "constant_speed", "coast_down", "gradeability")
    for what in ("Top speed", "Consumption at 50 km/h", "Coast-down A (f0)"):
        assert rows[what]["value"] is None and "Script sandbox stopped" in rows[what]["note"], what
    assert "limited by" not in rows["Top speed"]["note"]
    assert rows["Steepest grade at 30 km/h"]["note"].startswith("the run on the flat failed")
