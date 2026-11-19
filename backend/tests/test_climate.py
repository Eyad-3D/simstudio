"""Climate Control: heating and air-conditioning power from the outside
temperature (MOD-41).

Metric: at −7 °C with a PTC heater the Battery Electric Car's WLTC energy
rises into the band of published cold tests, a heat pump lowers it, and the
load is its own line in the run summary. The band: AAA's 2019 tests (via a
search summary, unverified) lost 41 % of range at −6.7 °C (20 °F) with the
heating on, that is 1 / (1 − 0.41) = 1.69 times the energy per km; ANL's
2024 cold tests report up to about 50 % at −18 °C. We ask for 1.45-1.95
times the 23 °C figure: a steady-state demand table cannot match a warm-up.
"""
from __future__ import annotations

from functools import lru_cache

import pytest
from helpers import bev_axle, conn, dbc, el, series

from app.solver import simulate
from app.solver.climate import HEAT_PUMP, PTC, carnot_share_cop, climate_power
from app.storage import load_example
from app.validation import validate_project


def test_cop_is_a_share_of_carnot():
    # heating at −7 °C into 21 °C: exchangers at 36 and −22 °C
    cop = carnot_share_cop(36.0, -22.0, 0.35, True)
    assert cop == pytest.approx(0.35 * 309.15 / 58.0)
    assert 1.8 < cop < 2.0
    # cooling at 35 °C to 21 °C: exchangers at 50 and 6 °C
    assert carnot_share_cop(50.0, 6.0, 0.35, False) == pytest.approx(0.35 * 279.15 / 44.0)
    # never worse than a resistance heater
    assert carnot_share_cop(36.0, -60.0, 0.1, True) == 1.0


def test_climate_power():
    p = {"heat_source": PTC, "fan_power_kW": 0.2}
    assert climate_power(0.0, -7.0, p) == (0.0, 0.0)  # off: no fan either
    assert climate_power(4.0, -7.0, p) == (pytest.approx(4.2), 1.0)
    hp = {**p, "heat_source": HEAT_PUMP, "heat_pump_min_C": -10}
    p_hp, cop = climate_power(4.0, -7.0, hp)
    assert cop > 1.8 and p_hp == pytest.approx(4.0 / cop + 0.2)
    # below its minimum the PTC heater takes over
    assert climate_power(4.0, -12.0, hp) == (pytest.approx(4.2), 1.0)
    # cooling runs the compressor whatever the heat source
    p_ac, cop_ac = climate_power(-2.65, 35.0, p)
    assert cop_ac > 2 and p_ac == pytest.approx(2.65 / cop_ac + 0.2)


def _axle_with_climate(temp: float | None, **climate):
    proj = bev_axle(profile="0:0; 5:50; 60:50")
    proj.cases[0].duration, proj.cases[0].timeStep = 60, 0.5
    root = proj.systems[0]
    root.elements.append(el("clim", "electric.climate", "Climate", **climate))
    root.connections.append(conn(50, "hvbus", "t4", "clim", "pos"))
    if temp is not None:
        root.elements.append(el("amb", "boundary.ambient", "Ambient", temperature_C=temp))
    return proj


def test_climate_draws_its_power_from_the_bus():
    r = simulate(_axle_with_climate(-10.0), "case")
    assert r.status == "success", [m.text for m in r.messages]
    power = series(r, "clim", "sig_power")
    heat = series(r, "clim", "sig_heat")
    # the default table: 4.5 kW of heat at −10 °C, a PTC heater, 0.2 kW blower
    assert power[-1]["value"] == pytest.approx(4.7)
    assert heat[-1]["value"] == pytest.approx(4.5)
    assert series(r, "clim", "sig_cop")[-1]["value"] == pytest.approx(1.0)
    s = {x.label: x.value for x in r.summary}
    assert s["Climate — energy used"] == pytest.approx(4.7 * 60 / 3600, abs=2e-3)
    assert s["Climate — heating delivered"] == pytest.approx(4.5 * 60 / 3600, abs=2e-3)


def test_enable_input_switches_it_off():
    proj = _axle_with_climate(-10.0)
    root = proj.systems[0]
    root.elements.append(el("off", "signal.constant", "Off", value=0))
    proj.dataBusConnections.append(dbc(60, "off", "sig_out", "clim", "sig_on_in"))
    r = simulate(proj, "case")
    assert max(abs(p["value"]) for p in series(r, "clim", "sig_power")) == 0.0


def test_without_an_ambient_it_is_20_degrees_and_says_so():
    proj = _axle_with_climate(None)
    r = simulate(proj, "case")
    assert max(abs(p["value"]) for p in series(r, "clim", "sig_power")) == 0.0
    texts = [c.text for c in validate_project(proj) if c.level == "info"]
    assert any("neither heats nor cools" in t for t in texts)


@lru_cache(maxsize=None)
def _bev_wltc(temp: float, source: str) -> dict[str, float]:
    p = load_example("bev-car")
    root = p.systems[0]
    amb = next((e for e in root.elements if e.componentDefId == "boundary.ambient"), None)
    if amb is None:
        root.elements.append(el("el-amb", "boundary.ambient", "Ambient", temperature_C=temp))
    else:  # the example has one (CON-30): set its temperature for every case
        amb.parameterOverrides["temperature_C"] = temp
        for c in p.cases:
            c.parameterOverrides.get(amb.id, {}).pop("temperature_C", None)
    clim = next((e for e in root.elements if e.componentDefId == "electric.climate"), None)
    if clim is None:
        root.elements.append(el("el-clim", "electric.climate", "Climate Control",
                                heat_source=source))
        root.connections.append(conn(901, "el-hvbus", "t3", "el-clim", "pos"))
        root.connections.append(conn(902, "el-clim", "neg", "el-ground", "t3"))
    else:  # the example has its own (its winter and hot-day cases use it)
        clim.parameterOverrides["heat_source"] = source
    r = simulate(p, "case-wltc")
    assert r.status == "success", [m.text for m in r.messages if m.level != "info"]
    return {s.label: s.value for s in r.summary}


def test_cold_wltc_rises_into_the_published_band_and_a_heat_pump_lowers_it():
    warm = _bev_wltc(23.0, PTC)
    ptc = _bev_wltc(-7.0, PTC)
    hp = _bev_wltc(-7.0, HEAT_PUMP)
    assert warm["Climate Control — energy used"] == 0.0  # 23 °C: in the comfort band
    ratio = ptc["Consumption"] / warm["Consumption"]
    assert 1.45 < ratio < 1.95, ratio
    assert hp["Consumption"] < ptc["Consumption"] - 2.0
    # its own line in the summary, and it is most of the extra energy
    def net(s):  # (recuperated energy now feeds the heater first)
        return s["HV Battery Pack — energy delivered"] - s["HV Battery Pack — energy recuperated"]
    extra = net(ptc) - net(warm)
    assert 0.8 * extra < ptc["Climate Control — energy used"] < extra


def test_a_live_edit_of_the_demand_table_applies():
    calls = {"n": 0}

    def control():
        calls["n"] += 1
        if calls["n"] != 20:
            return []
        return [{"type": "set_param", "elementId": "clim", "key": "demand_table",
                 "value": {"-20": 1.0, "40": 1.0}},
                {"type": "set_param", "elementId": "clim", "key": "fan_power_kW", "value": 0.5}]

    r = simulate(_axle_with_climate(-10.0), "case", control=control)
    power = {round(p["t"], 3): p["value"] for p in series(r, "clim", "sig_power")}
    assert power[5.0] == pytest.approx(4.7)
    # 1 kW of heat from a PTC heater and the 0.5 kW blower
    assert power[30.0] == pytest.approx(1.5)
    assert power[60.0] == pytest.approx(1.5)
