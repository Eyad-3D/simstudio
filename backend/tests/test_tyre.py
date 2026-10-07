"""A tyre from its size code and load index (MOD-48).

Metric: the radius from '205/55 R16' matches the ISO formula within 1 mm;
an overloaded wheel is flagged; the estimated slip stiffness lies within the
range of Chrono's sample tyres (its TMeasy guesses give 17.8-19.5 times the
load: truck and passenger car, nominal and twice the load).
"""
from __future__ import annotations

from pathlib import Path

import pytest
from helpers import bev_axle

from app import tyre
from app.tyre import LOAD_INDEX_KG, parse_tyre_code, tyre_values
from app.validation import validate_project

TS = Path(__file__).parent.parent.parent / "frontend" / "src" / "tyre.ts"


@pytest.mark.parametrize("code,radius_mm,li,speed", [
    ("205/55 R16 91V", 203.2 + 112.75, 91, "V"),
    ("P205/55R16", 203.2 + 112.75, None, None),
    ("225/45ZR17 94W XL", 215.9 + 101.25, 94, "W"),
    ("195/75 R16C 107/105R", 203.2 + 146.25, 107, "R"),
    ("LT245/75R16 120/116S", 203.2 + 183.75, 120, "S"),
    ("20.5x7.0-13", 260.35, None, None),
    ("18x7.5-10 R25B", 228.6, None, None),
])
def test_codes_read_as_the_iso_formula(code, radius_mm, li, speed):
    spec = parse_tyre_code(code)
    assert spec is not None
    assert spec.unloaded_radius_m * 1000 == pytest.approx(radius_mm, abs=1.0)
    assert spec.load_index == li and spec.speed_symbol == speed


def test_not_a_code():
    for code in ("", "hello", "205/55", "205/55 R1"):
        assert parse_tyre_code(code) is None


def test_load_index_table():
    assert len(LOAD_INDEX_KG) == 280
    assert LOAD_INDEX_KG[91] == 615 and LOAD_INDEX_KG[100] == 800 and LOAD_INDEX_KG[279] == 136000
    assert parse_tyre_code("205/55 R16 91V").max_load_n == pytest.approx(615 * 9.81)


def test_estimates_lie_in_chronos_range():
    values = tyre_values(parse_tyre_code("205/55 R16 91V"))
    assert values["radius_m"] == pytest.approx(0.97 * 0.31595, abs=1e-4)
    assert 17.8 <= values["slip_stiffness"] <= 19.5
    assert values["mu_nominal_load_N"] == pytest.approx(0.5 * 615 * 9.81, abs=0.1)
    # 1.129 at the nominal load, 1.090 at twice it
    pn = values["mu_nominal_load_N"]
    assert values["mu"] + values["mu_load_sensitivity_per_kN"] * pn / 1000 == pytest.approx(
        1.0896, abs=1e-3)
    assert "slip_stiffness" not in tyre_values(parse_tyre_code("20.5x7.0-13"))


def _with_tyres(code: str, mass: float = 1500.0, radius: float | None = None):
    proj = bev_axle()
    for e in proj.systems[0].elements:
        if e.componentDefId == "propulsion.wheel":
            e.parameterOverrides["tyre_code"] = code
            e.parameterOverrides["radius_m"] = radius if radius is not None else tyre_values(
                parse_tyre_code(code))["radius_m"]
        if e.componentDefId == "vehicle.body":
            e.parameterOverrides["mass_kg"] = mass
    return proj


def test_overloaded_wheel_is_flagged():
    # two wheels carry 1,500 kg: 750 kg each, a load index 91 tyre carries 615 kg
    texts = [c.text for c in validate_project(_with_tyres("205/55 R16 91V")) if c.level == "warning"]
    assert any("more than its tyre's load index 91 allows (615 kg)" in t for t in texts)
    ok = [c.text for c in validate_project(_with_tyres("205/55 R16 91V", mass=1200))
          if "load index" in c.text]
    assert not ok


def test_unreadable_code_and_mismatched_radius_are_reported():
    checks = validate_project(_with_tyres("two-oh-five", radius=0.3))
    assert any("cannot read" in c.text for c in checks if c.level == "warning")
    checks = validate_project(_with_tyres("205/55 R16 91V", mass=1000, radius=0.36))
    assert any("rolls on about 0.306 m" in c.text for c in checks if c.level == "info")


def test_the_form_reads_the_same_data():
    """frontend/src/tyre.ts fills the form from the copy of tyres.json that
    sync-data.mjs makes: it must be the backend's."""
    backend = (Path(tyre.__file__).parent / "library" / "tyres.json").read_text(encoding="utf-8")
    assert (TS.parent / "data" / "tyres.json").read_text(encoding="utf-8") == backend
