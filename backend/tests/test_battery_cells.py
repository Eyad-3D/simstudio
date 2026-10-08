"""Battery v2 (L1): a pack built from cells, with current, voltage and power
limits (MOD-08).

Metric: a 96s2p pack built from a cell reproduces the hand-calculated
capacity, resistance and mass within 0.5 %; the 2/10/30 s power signals
match the closed-form limits within 0.5 %; pack current and cell voltages
never exceed their limits. (The pulse test against a published cell set is
not here: no licence-clear set is bundled yet, see CON-19.)
"""
from __future__ import annotations

import pytest
from helpers import conn, el, example_result, project, series

from app.library import library_by_id
from app.solver import simulate
from app.solver.battery import CELLS
from app.solver.maps import interp1, interp2, parse_table1d, parse_table2d
from app.storage import load_example
from app.validation import validate_project

LIB = {p.key: p.default for p in library_by_id()["battery.generic"].parameters}
CELL_OCV = parse_table1d(LIB["cell_ocv_table"])
R_FACTOR = parse_table2d(LIB["cell_resistance_factor"])
T_FACTOR = parse_table1d(LIB["cell_temperature_factor"])


def r_cell(soc_pct, t_c, pulse_s, dcr=0.02):
    """The cell resistance by hand: DCR × pulse/SOC factor × temperature factor."""
    return dcr * interp2(R_FACTOR, pulse_s, soc_pct) * interp1(T_FACTOR, t_c)


def bench(load_kw: float, duration: float = 60.0, temp: float | None = None, **battery):
    """A battery feeding a constant load (a Power Consumer), nothing else."""
    elements = [el("batt", "battery.generic", "Pack", **{"pack_model": CELLS, **battery}),
                el("load", "electric.constant_drive", "Load", power_kW=load_kw)]
    if temp is not None:
        elements.append(el("amb", "boundary.ambient", "Ambient", temperature_C=temp))
    proj = project(elements, [conn(1, "batt", "pos", "load", "pos"),
                              conn(2, "batt", "neg", "load", "neg")], [],
                   duration=duration, time_step=0.1)
    return proj


def _summary(r) -> dict:
    return {s.label: s.value for s in r.summary}


def test_96s2p_reproduces_hand_calculated_capacity_resistance_and_mass():
    layout = dict(series_cells=96, parallel_cells=2, cell_capacity_Ah=5.0,
                  cell_resistance_ohm=0.02, interconnect_resistance_ohm=0.0002,
                  contactor_resistance_ohm=0.0005, cell_mass_kg=0.07, packaging_factor=1.4,
                  initial_soc_pct=50, cell_peak_discharge_A=0, cell_max_discharge_A=0)
    r = simulate(bench(5.0, duration=12.0, **layout), "case")
    assert r.status == "success", [m.text for m in r.messages]
    s = _summary(r)
    assert s["Pack — charge capacity"] == pytest.approx(10.0, rel=0.005)  # 2 × 5 Ah
    assert s["Pack — pack mass (estimate)"] == pytest.approx(96 * 2 * 0.07 * 1.4, rel=0.005)
    # resistance: from the terminal voltage at a 10 s pulse, 50 % SOC, 20 °C
    v = series(r, "batt", "sig_voltage")
    i = series(r, "batt", "sig_current")
    k = next(n for n, x in enumerate(v) if x["t"] == pytest.approx(10.0))
    soc = series(r, "batt", "sig_soc")[k]["value"]
    ocv = 96 * interp1(CELL_OCV, soc)
    r_meas = (ocv - v[k]["value"]) / i[k]["value"]
    # by hand: 96 × R_cell / 2 + 96 × 0.2 mΩ + 0.5 mΩ, R_cell at 10 s, 50 % and 20 °C
    # (the pulse has lasted 10 s at the end of the step that point closes)
    r_hand = 96 * r_cell(soc, 20.0, 10.0) / 2 + 96 * 0.0002 + 0.0005
    assert r_meas == pytest.approx(r_hand, rel=0.005)
    # and at 25 °C, 10 s, 50 %: the datasheet values themselves
    assert 96 * r_cell(50, 25, 10) / 2 + 96 * 0.0002 + 0.0005 == pytest.approx(0.9797, rel=1e-9)
    texts = [c.text for c in validate_project(bench(5.0, **layout)) if c.level == "info"]
    assert any("96s2p of 5 Ah gives 10 Ah" in t and "979.7 mΩ" in t for t in texts)


def test_state_of_power_matches_the_closed_form():
    r = simulate(bench(2.0, duration=5.0, series_cells=96, parallel_cells=2,
                       initial_soc_pct=60), "case")
    k = -1
    soc = series(r, "batt", "sig_soc")[k]["value"]
    ocv_cell = interp1(CELL_OCV, soc)
    for d in (2, 10, 30):
        rc = r_cell(soc, 20.0, d)
        r_pack = 96 * rc / 2 + 96 * 0.0002 + 0.0005
        i_map = 2 * (30.0 if d <= 10 else 15.0)  # peak 30 A for 10 s, then 15 A, × 2
        i_volt = 2 * (ocv_cell - 2.5) / rc  # the cell at its 2.5 V minimum
        i = min(i_map, i_volt, 96 * ocv_cell / (2 * r_pack))
        p_dis = i * (96 * ocv_cell - i * r_pack)
        i_ch = min(2 * (10.0 if d <= 10 else 5.0), 2 * (4.2 - ocv_cell) / rc)
        p_ch = i_ch * (96 * ocv_cell + i_ch * r_pack)
        assert series(r, "batt", f"sig_p_dis_{d}s")[k]["value"] == pytest.approx(
            p_dis / 1000, rel=0.005)
        assert series(r, "batt", f"sig_p_ch_{d}s")[k]["value"] == pytest.approx(
            p_ch / 1000, rel=0.005)


def test_current_limit_holds_and_says_so():
    # 96s2p: 60 A peak for 10 s, then 30 A; asked for 30 kW (about 85 A)
    r = simulate(bench(30.0, duration=30.0, series_cells=96, parallel_cells=2), "case")
    i = series(r, "batt", "sig_current")
    for x in i:
        lim = 60.0 if x["t"] <= 10.0 + 1e-9 else 30.0
        assert x["value"] <= lim * (1 + 1e-6)
    assert max(x["value"] for x in i) == pytest.approx(60.0, rel=1e-3)
    assert i[-1]["value"] == pytest.approx(30.0, rel=1e-3)
    s = _summary(r)
    assert s["Pack — time at discharge current limit"] > 25
    assert any("held at its discharge limit" in m.text and m.level == "info" for m in r.messages)


def test_cold_cells_hit_their_voltage_limit_but_never_go_below_it():
    # at −20 °C the resistance is 4 × and a 3.5 V minimum binds before the current
    r = simulate(bench(30.0, duration=60.0, temp=-20.0, series_cells=96, parallel_cells=10,
                       cell_min_voltage_V=3.5, initial_soc_pct=30), "case")
    vmin = series(r, "batt", "sig_v_cell_min")
    assert min(x["value"] for x in vmin) >= 3.5 - 2e-3  # (the OCV falls a little over a step)
    s = _summary(r)
    assert s["Pack — time at discharge cell voltage limit"] > 0
    assert s["Pack — lowest cell voltage"] == pytest.approx(3.5, abs=2e-3)


def test_a_weak_group_sags_most_and_sets_the_limit():
    common = dict(series_cells=96, parallel_cells=10, cell_min_voltage_V=3.3, initial_soc_pct=20)
    even = simulate(bench(20.0, duration=120.0, **common), "case")
    weak = simulate(bench(20.0, duration=120.0, weak_cell_capacity_pct=90,
                          weak_cell_resistance_pct=150, **common), "case")
    assert min(x["value"] for x in series(weak, "batt", "sig_v_cell_min")) >= 3.3 - 2e-3
    # its group reaches the floor first, so the pack is held there longer
    held = "Pack — time at discharge cell voltage limit"
    assert _summary(weak)[held] > _summary(even).get(held, 0.0)
    # it holds the pack's discharge back sooner
    e_even = _summary(even)["Pack — energy delivered"]
    e_weak = _summary(weak)["Pack — energy delivered"]
    assert e_weak < e_even


def test_long_pulses_sag_more():
    r = simulate(bench(10.0, duration=120.0, series_cells=96, parallel_cells=10,
                       initial_soc_pct=60), "case")
    v = series(r, "batt", "sig_voltage")
    at = {round(x["t"]): x["value"] for x in v}
    # the same power: the voltage falls as the pulse lengthens (2 → 30 → 120 s)
    assert at[2] > at[30] > at[120]


def test_pack_values_with_a_current_limit():
    p = load_example("bev-car")
    for e in p.systems[0].elements:
        if e.id == "el-battery":
            e.parameterOverrides["max_discharge_current_A"] = 300
    r = simulate(p, "case-wltc")
    assert max(x["value"] for x in series(r, "el-battery", "sig_current")) <= 300 * (1 + 1e-6)
    lim = series(r, "el-battery", "sig_i_dis_limit")
    assert all(x["value"] == 300 for x in lim if x["value"] is not None)


def test_examples_unchanged_and_keep_their_limits():
    """The examples are packs without current limits: nothing to hold; their
    current and voltage stay where they were."""
    for pid, cid in (("bev-car", "case-city"), ("hybrid-car", "case-mixed")):
        r = example_result(pid, cid)
        assert not any(s.label.endswith("limit") and "time at" in s.label for s in r.summary)


def test_fs_acceleration_from_cells_keeps_to_the_cells_limits():
    p = load_example("fs-electric")
    for e in p.systems[0].elements:
        if e.id == "el-battery":
            e.parameterOverrides.update({
                "pack_model": CELLS, "series_cells": 138, "parallel_cells": 4,
                "cell_capacity_Ah": 3.5, "cell_resistance_ohm": 0.015,
                "cell_max_discharge_A": 20, "cell_peak_discharge_A": 30})
    r = simulate(p, "case-accel-75m")
    assert r.status in ("success", "warning"), [m.text for m in r.messages]
    i = series(r, "el-battery", "sig_current")
    assert max(x["value"] for x in i) <= 4 * 30 * (1 + 1e-6)  # 120 A for its first 10 s
    assert min(x["value"] for x in series(r, "el-battery", "sig_v_cell_min")) >= 2.5
    s = _summary(r)
    assert s["Accumulator — time at discharge current limit"] > 1.0
    # slower than the example's 3.74 s, held to 120 A
    assert s["Time to 75 m"] > 3.8


def _pack_bench(load_kw: float, duration: float = 60.0, **battery):
    """A pack-values battery feeding a constant load."""
    elements = [el("batt", "battery.generic", "Pack", **battery),
                el("load", "electric.constant_drive", "Load", power_kW=load_kw)]
    return project(elements, [conn(1, "batt", "pos", "load", "pos"),
                              conn(2, "batt", "neg", "load", "neg")], [],
                   duration=duration, time_step=0.1)


def test_an_rc_pair_counts_against_the_pack_voltage_limit():
    """The RC pair's voltage adds to R0's drop: the minimum pack voltage holds
    with it too (it fell 13 V past it before)."""
    r = simulate(_pack_bench(80.0, duration=120.0, capacity_kWh=20, initial_soc_pct=50,
                             min_voltage_V=320, rc_resistance_ohm=0.05,
                             rc_time_constant_s=20), "case")
    v = series(r, "batt", "sig_voltage")
    assert min(x["value"] for x in v) >= 320 - 2e-3
    assert _summary(r)["Pack — time at discharge pack voltage limit"] > 60


def test_an_rc_pair_counts_against_the_cell_voltage_limit():
    """Built from cells, the RC pair's voltage is shared by the series cells:
    the cells keep to their minimum with it, and the lowest cell voltage
    reported is the one the pack's terminal voltage implies."""
    r = simulate(bench(30.0, duration=60.0, series_cells=96, parallel_cells=10,
                       cell_min_voltage_V=3.5, initial_soc_pct=30, rc_resistance_ohm=0.1,
                       rc_time_constant_s=20), "case")
    v, i = series(r, "batt", "sig_voltage"), series(r, "batt", "sig_current")
    vmin = series(r, "batt", "sig_v_cell_min")
    wiring = 96 * 0.0002 + 0.0005  # interconnects and contactors carry no cell voltage
    for k in range(1, len(v)):
        implied = (v[k]["value"] + i[k]["value"] * wiring) / 96
        assert vmin[k]["value"] == pytest.approx(implied, abs=1e-6)
        assert implied >= 3.5 - 2e-3
    s = _summary(r)
    assert s["Pack — lowest cell voltage"] == pytest.approx(3.5, abs=2e-3)
    assert s["Pack — time at discharge cell voltage limit"] > 0


def test_an_empty_weak_group_stops_the_pack():
    """In series the same current flows through every group, so the string
    holds the weak group's charge: at 50 % of the capacity it empties
    (50 → 10 %) when the pack has given half of that, at 30 %."""
    r = simulate(bench(40.0, duration=400.0, series_cells=96, parallel_cells=2,
                       cell_capacity_Ah=5.0, weak_cell_capacity_pct=50, initial_soc_pct=50,
                       min_soc_pct=10), "case")
    soc, i = series(r, "batt", "sig_soc"), series(r, "batt", "sig_current")
    assert min(x["value"] for x in soc) == pytest.approx(30.0, abs=1e-6)
    assert all(x["value"] <= 1e-9 for x in i if x["t"] >= 240)
    assert any("weakest group reached minimum SOC (10 %)" in m.text and m.level == "warning"
               for m in r.messages)


def test_the_derating_band_without_a_current_limit_lowers_the_maximum_power_current():
    """Pack values with a derating band and no current limit: the band lowers
    the maximum-power-point current to 0 (it did nothing before the minimum
    SOC), and Data Checks say how far above the load that is."""
    r0 = 1.0  # a resistive pack, so a 20 kW load meets the band
    proj = _pack_bench(20.0, duration=150.0, capacity_kWh=2, initial_soc_pct=40,
                       min_soc_pct=10, soc_derate_band_pct=20, internal_resistance_ohm=r0)
    r = simulate(proj, "case")
    ocv = parse_table1d(LIB["ocv_table"])
    soc = series(r, "batt", "sig_soc")
    lim = series(r, "batt", "sig_i_dis_limit")
    for k in range(1, len(soc)):
        s_pct = soc[k - 1]["value"]  # the limit is set at the step's start
        k_band = min(1.0, max(0.0, (s_pct - 10.0) / 20.0))
        if 0.05 < k_band < 1.0:
            assert lim[k]["value"] == pytest.approx(
                k_band * interp1(ocv, s_pct) / (2 * r0), rel=0.01)
    assert _summary(r)["Pack — time at discharge SOC derating limit"] > 5
    assert min(x["value"] for x in soc) > 10.0  # the band holds it off the minimum
    texts = [c.text for c in validate_project(proj) if c.level == "info"]
    assert any("SOC Derating Band but no Max Discharge Current" in t for t in texts)

