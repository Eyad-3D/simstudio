"""CON-05: consumption as labs and regulators report it — at the socket,
MPGe, range, per phase, the FTP's bag weighting and a hybrid's
charge-corrected fuel — each from its published formula."""
from __future__ import annotations

import pytest

from app import cycles
from app.solver import simulate
from app.solver.labfig import FTP_WEIGHTS, KM_PER_MILE, KWH_PER_GALLON
from app.storage import load_example


def _rows(result) -> dict:
    return {s.label: s for s in result.summary}


def _on(example: str, case_id: str, cycle_id: str | None = None, **battery):
    project = load_example(example)
    case = next(c for c in project.cases if c.id == case_id)
    if cycle_id:
        case.parameterOverrides.setdefault("el-task", {})["cycle"] = cycle_id
        case.duration = cycles.info(cycle_id)["duration_s"]
    if battery:
        el = "el-battery" if example == "hybrid-car" else next(
            e.id for s in project.systems for e in s.elements if e.componentDefId == "battery.generic")
        case.parameterOverrides.setdefault(el, {}).update(battery)
    return simulate(project, case_id)


@pytest.fixture(scope="module")
def bev_wltc():
    return _rows(_on("bev-car", "case-wltc"))


def test_a_bev_reports_socket_energy_mpge_and_range(bev_wltc):
    dc = bev_wltc["Consumption"].value
    ac = bev_wltc["Consumption at the socket (AC)"].value
    assert ac == pytest.approx(dc / 0.86, abs=0.01)  # the default Charger Efficiency, 86 %
    mpge = KWH_PER_GALLON / (ac / 100 * KM_PER_MILE)
    assert bev_wltc["Fuel-economy equivalent (MPGe, AC)"].value == pytest.approx(mpge, abs=0.1)
    # the Cupra Born's 58 kWh usable at its WLTC consumption: about 420 km,
    # its published WLTP range (background knowledge, unverified)
    assert 380 < bev_wltc["Range at this consumption"].value < 460


def test_wltc_phases_add_up_to_the_whole_cycle(bev_wltc):
    names = [p[0] for p in cycles.phases("wltc-3b")]
    km = [bev_wltc[f"Phase {n} — distance"].value for n in names]
    assert sum(km) == pytest.approx(bev_wltc["Distance driven"].value, abs=0.002)
    assert km == pytest.approx([3.095, 4.756, 7.162, 8.254], abs=0.002)  # EU 2017/1151's phases
    kwh = [bev_wltc[f"Phase {n} — consumption"].value * d / 100 for n, d in zip(names, km)]
    total = bev_wltc["Consumption"].value * bev_wltc["Distance driven"].value / 100
    assert sum(kwh) == pytest.approx(total, rel=2e-3)
    # the motorway phase takes the most energy per km
    assert bev_wltc["Phase Extra High — consumption"].value == max(
        bev_wltc[f"Phase {n} — consumption"].value for n in names)


def test_the_charger_efficiency_sets_the_socket_figure():
    rows = _rows(_on("bev-car", "case-city", charger_efficiency_pct=50))
    assert rows["Consumption at the socket (AC)"].value == pytest.approx(
        2 * rows["Consumption"].value, abs=0.01)


def test_ftp_bags_are_weighted_as_epa_does():
    rows = _rows(_on("hybrid-car", "case-udds", "ftp-75"))
    d = [rows[f"Phase Bag {i} ({w}) — distance"].value
         for i, w in ((1, "cold start"), (2, "stabilised"), (3, "hot start"))]
    lpk = [rows[f"Phase Bag {i} ({w}) — fuel consumption"].value
           for i, w in ((1, "cold start"), (2, "stabilised"), (3, "hot start"))]
    fuel = [lp * km / 100 for lp, km in zip(lpk, d)]  # litres per bag
    wc, wh = FTP_WEIGHTS
    weighted = 100 * (wc * (fuel[0] + fuel[1]) / (d[0] + d[1]) + wh * (fuel[2] + fuel[1]) / (d[2] + d[1]))
    assert rows["FTP weighted fuel consumption"].value == pytest.approx(weighted, abs=0.02)
    assert "Consumption at the socket (AC)" not in rows  # a hybrid is not charged at a socket


def test_a_hybrid_reports_its_charge_change_and_corrected_fuel():
    balanced = _rows(_on("hybrid-car", "case-udds"))
    # the example starts at its charge-balanced SOC: under SAE J1711's 1 %
    assert abs(balanced["Battery energy change, share of fuel energy"].value) < 1.0
    assert balanced["Fuel consumption, charge-corrected"].value == pytest.approx(
        balanced["Fuel consumption"].value, rel=0.05)
    # started fuller, the battery gives energy the engine did not have to
    # make: the corrected figure is higher than the raw one
    drained = _rows(_on("hybrid-car", "case-udds", initial_soc_pct=70))
    assert drained["Battery energy change, share of fuel energy"].value > 1.0
    assert (drained["Fuel consumption, charge-corrected"].value
            > drained["Fuel consumption"].value)


def test_lab_rows_share_their_base_rows_validity():
    # a battery that starts at its floor runs empty: Consumption is not valid,
    # and neither is anything computed from it
    rows = _rows(_on("bev-car", "case-city", initial_soc_pct=5))
    why = rows["Consumption"].notValid
    assert why
    for label in ("Consumption at the socket (AC)", "Fuel-economy equivalent (MPGe, AC)",
                  "Range at this consumption"):
        assert rows[label].notValid == why
