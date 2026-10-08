"""CON-32: the US window-sticker estimate from UDDS and HWFET runs, as
FASTSim's simdrivelabel computes it, every step shown, never 'certified'."""
from __future__ import annotations

import pytest
from fastapi.testclient import TestClient

from app import label
from app.label import LabFigures, adjust
from app.main import app
from app.storage import load_example

# FASTSim 2.1.5's longparams.json, 'LD_FE_Adj_Coef', and its constants
FASTSIM_COEF = {"2008": {"City Intercept": 0.003259, "City Slope": 1.1805,
                         "Highway Intercept": 0.001376, "Highway Slope": 1.3466},
                "2017": {"City Intercept": 0.004091, "City Slope": 1.1601,
                         "Highway Intercept": 0.003191, "Highway Slope": 1.2945}}
KWH_PER_GGE, CHG_EFF, MAX_EPA_ADJ = 33.7, 0.86, 0.3


def fastsim_label(veh_year, bev, udds, hwy, ess_kwh=0.0):
    """FASTSim 2.1.5 simdrivelabel.get_label_fe, non-PHEV branch, retyped
    line for line (udds/hwy: mpgge, or battery kWh per mile for a BEV)."""
    adj = FASTSIM_COEF["2008"] if veh_year < 2017 else FASTSIM_COEF["2017"]
    out = {}
    if not bev:
        out["labCombMpgge"] = 1 / (0.55 / udds + 0.45 / hwy)
        out["adjUddsMpgge"] = 1 / (adj["City Intercept"] + adj["City Slope"] / udds)
        out["adjHwyMpgge"] = 1 / (adj["Highway Intercept"] + adj["Highway Slope"] / hwy)
        out["adjCombMpgge"] = 1 / (0.55 / out["adjUddsMpgge"] + 0.45 / out["adjHwyMpgge"])
    else:
        out["adjUddsKwhPerMile"] = (1 / max(
            (1 / (adj["City Intercept"] + (adj["City Slope"] / ((1 / udds) * KWH_PER_GGE)))),
            (1 / udds) * KWH_PER_GGE * (1 - MAX_EPA_ADJ))) * KWH_PER_GGE / CHG_EFF
        out["adjHwyKwhPerMile"] = (1 / max(
            (1 / (adj["Highway Intercept"] + (adj["Highway Slope"] / ((1 / hwy) * KWH_PER_GGE)))),
            (1 / hwy) * KWH_PER_GGE * (1 - MAX_EPA_ADJ))) * KWH_PER_GGE / CHG_EFF
        out["adjCombKwhPerMile"] = 0.55 * out["adjUddsKwhPerMile"] + 0.45 * out["adjHwyKwhPerMile"]
        out["adjCombEssKwhPerMile"] = out["adjCombKwhPerMile"] * CHG_EFF
        out["netRangeMiles"] = ess_kwh / out["adjCombEssKwhPerMile"]
    return out


@pytest.mark.parametrize(("year", "bev", "udds", "hwy", "ess"), [
    (2022, True, 0.18, 0.21, 59.5),   # a compact BEV, where the 0.7 floor holds
    (2022, True, 0.40, 0.45, 100.0),  # a heavy BEV, where the derived equation holds
    (2016, False, 30.0, 42.0, 0.0),   # a petrol car on the 2008 coefficients
    (2022, False, 58.0, 60.0, 0.0),   # a hybrid
])
def test_label_matches_fastsim_for_the_same_lab_figures(year, bev, udds, hwy, ess):
    lab = (LabFigures(city_kwh_per_mi=udds, hwy_kwh_per_mi=hwy) if bev
           else LabFigures(city_mpg=udds, hwy_mpg=hwy))
    ours = adjust(lab, bev, year, CHG_EFF, ess)
    for key, value in fastsim_label(year, bev, udds, hwy, ess).items():
        assert ours[key] == pytest.approx(value, rel=5e-3), key  # within 0.5 %


def test_the_model_year_picks_the_coefficients():
    assert label.coefficients(2016)[0] == 2008 and label.coefficients(2017)[0] == 2017


@pytest.fixture(scope="module")
def bev_label():
    return TestClient(app).post("/api/label-estimate", json={
        "project": load_example("bev-car").model_dump(mode="json")}).json()


def test_a_bev_gets_a_label_with_every_step_and_not_certified(bev_label):
    assert bev_label["notCertified"] == "Simulated estimate, not a certified value."
    assert bev_label["electric"] and bev_label["problems"] == []
    # the BEV has no UDDS or HWFET case: copies of its first Cycle case run them
    assert bev_label["cases"] == {"udds": "US label: EPA city (UDDS)",
                                  "hwfet": "US label: EPA highway (HWFET)"}
    whats = [s["what"] for s in bev_label["steps"]]
    assert whats[:2] == ["Lab city energy (battery)", "Lab highway energy (battery)"]
    assert "Label combined MPGe" in whats and "Label range" in whats
    assert all(s["how"] for s in bev_label["steps"])
    f = bev_label["figures"]
    assert f["adjCombKwhPerMile"] == pytest.approx(
        0.55 * f["adjUddsKwhPerMile"] + 0.45 * f["adjHwyKwhPerMile"])
    # a label figure is never better than the lab one
    assert f["adjUddsKwhPerMile"] * 0.86 > f["labUddsKwhPerMile"]


def test_a_project_without_a_cycle_case_is_refused():
    project = load_example("fs-electric")  # acceleration and lap cases only
    r = TestClient(app).post("/api/label-estimate",
                             json={"project": project.model_dump(mode="json")})
    assert r.status_code == 400 and "case of kind Cycle" in r.json()["detail"]


def test_a_fuel_cell_car_gets_no_label():
    """Its fuel cell's energy is in neither the battery's Consumption nor
    the fuel consumption, so a label would show MPGe from the battery's
    share alone; the same for a model with a Voltage Source."""
    from helpers import el, fuel_cell_car

    project = fuel_cell_car(setpoint_kW=1.5, cycle="udds")
    with pytest.raises(ValueError, match="Fuel Cell Stack 'Fuel Cell'"):
        label.estimate(project)
    r = TestClient(app).post("/api/label-estimate",
                             json={"project": project.model_dump(mode="json")})
    assert r.status_code == 400 and "battery electric cars, hybrids" in r.json()["detail"]
    bev = load_example("bev-car")
    bev.systems[0].elements.append(el("vs", "electric.voltage_source", "Bench Supply"))
    with pytest.raises(ValueError, match="Voltage Source 'Bench Supply'"):
        label.estimate(bev)


def test_an_own_case_runs_only_when_it_drives_the_cycle_as_published():
    """A scaled, repeated or shortened UDDS case is not UDDS: a copy on the
    cycle as published runs instead, with a note; copies drop the base
    case's scale and repeat."""
    project = load_example("hybrid-car")
    udds = next(c for c in project.cases if c.id == "case-udds")
    udds.parameterOverrides["el-task"]["scale_pct"] = 50
    notes: list[str] = []
    chosen = label.label_cases(project, "case-udds", notes)
    assert chosen["udds"].id == "label-udds"
    assert chosen["udds"].parameterOverrides["el-task"] == {
        "cycle": "udds", "scale_pct": 100, "repeat": False, "mode": "time"}
    assert chosen["hwfet"].id == "case-hwfet"  # the HWFET case is as published
    assert notes == ["Case 'EPA city (UDDS)' drives EPA city (UDDS) scaled to 50 %; the label "
                     "runs the cycle as published instead."]
    udds.parameterOverrides["el-task"]["scale_pct"] = 100
    udds.duration = 600
    notes.clear()
    assert label.label_cases(project, None, notes)["udds"].id == "label-udds"
    assert "for 600 s only" in notes[0]

