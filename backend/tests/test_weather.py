"""CON-30: ready-made weather presets on the Ambient, each with its source,
and the Battery Electric Car's winter and hot-day cases, which its Climate
Control (MOD-41) heats and cools for."""
from __future__ import annotations

import pytest

from app.library import library_by_id
from app.solver import simulate
from app.storage import load_example

PRESETS = {
    "Cold day (−7 °C)": (-7, 101.325),
    "Standard day (23 °C)": (23, 101.325),
    "Hot and sunny day (35 °C)": (35, 101.325),
    "High altitude (1,500 m)": (5.25, 84.56),
}


def test_the_ambient_has_four_sourced_presets():
    presets = {p.name: p for p in library_by_id()["boundary.ambient"].presets}
    assert set(presets) == set(PRESETS)
    for name, (temp, pressure) in PRESETS.items():
        p = presets[name]
        assert p.values == {"temperature_C": temp, "pressure_kPa": pressure}
        assert p.note  # says where the values come from
    # the standard atmosphere at 1,500 m (ISO 2533)
    assert 101.325 * (1 - 0.0065 * 1500 / 288.15) ** 5.25588 == pytest.approx(84.56, abs=0.005)


def _rows(case_id: str) -> dict[str, float]:
    rows = {s.label: s for s in simulate(load_example("bev-car"), case_id).summary}
    assert not rows["Consumption"].notValid
    return {label: s.value for label, s in rows.items()}


def test_the_bev_winter_and_hot_day_cases_heat_and_cool_with_the_weather():
    mild = _rows("case-wltc")  # 20 °C: the Climate Control is off
    winter, summer = _rows("case-wltc-winter"), _rows("case-wltc-summer")
    assert mild["Climate Control — energy used"] == 0.0
    # its PTC heater at −7 °C: 4.05 kW of heat and the 0.2 kW blower for 1,800 s
    assert winter["Climate Control — energy used"] == pytest.approx(4.25 * 0.5, rel=1e-4)
    # its air-conditioning at 35 °C: 2.65 kW of cooling at a COP of 0.35 × 279.15 / 44,
    # and the blower
    cop = 0.35 * 279.15 / 44.0
    assert summer["Climate Control — energy used"] == pytest.approx(
        (2.65 / cop + 0.2) * 0.5, rel=1e-4)
    # the cases' hand calculations (23.73 and 16.78 kWh/100 km), within 1 %
    assert winter["Consumption"] == pytest.approx(23.73, rel=0.01)
    assert summer["Consumption"] == pytest.approx(16.78, rel=0.01)
    assert mild["Consumption"] < summer["Consumption"] < winter["Consumption"]
