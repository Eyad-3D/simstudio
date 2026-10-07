"""CON-30: ready-made weather presets on the Ambient, each with its source,
and the Battery Electric Car's winter and hot-day cases."""
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


def _consumption(case_id: str) -> float:
    rows = {s.label: s for s in simulate(load_example("bev-car"), case_id).summary}
    assert not rows["Consumption"].notValid
    return rows["Consumption"].value


def test_the_bev_winter_and_hot_day_cases_follow_the_air():
    mild = _consumption("case-wltc-hvac")  # 20 °C with the same 2.5 kW load
    winter, summer = _consumption("case-wltc-winter"), _consumption("case-wltc-summer")
    # denser cold air, thinner hot air; the bands are the example's own
    assert 19.0 < winter < 20.0 and 18.2 < summer < 18.9
    assert summer < mild < winter
