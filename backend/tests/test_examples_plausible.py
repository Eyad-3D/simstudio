"""The shipped examples give believable numbers (VAL-07).

The golden fixtures freeze what the examples compute; these tests check that
what they compute is believable. Each headline number must fall inside a
band taken from real vehicles of the same class, so a regression that makes
an example implausible (a 1 s control step, a 2.5 kW auxiliary default, an
engine that never stops) fails here even when the fixtures are regenerated.

Reference values (sources in docs/data-register.csv, DR-01 and DR-02):

- Battery Electric Car = 2021 Cupra Born 58 kWh (VW ID.3 class). Cars of
  this class are rated about 15-16 kWh/100 km on WLTP at the charging socket
  (background knowledge, unverified), about 13.5-14.5 at the battery after
  charging losses; band 13-17 kWh/100 km at the battery terminals. 0-60 mph
  in 6.8 s (FASTSim's vehicle file), 0-100 km/h in 7.3 s and 160 km/h top
  speed (Cupra's figures, background knowledge); bands +/-10 % and +/-2 %.
- P2 Hybrid Car = Hyundai Ioniq Hybrid Blue: EPA 2022 Test Car List FTP
  80.8 mpg (2.91 l/100 km), HWFE 80.1 mpg (2.94 l/100 km). Band: at most
  5 l/100 km on each EPA cycle and at most 4.5 after the battery-charge
  correction (CON-02), charge-sustaining within 1 % SOC. The lower limit of
  2 l/100 km is ours: no production car does much better than the Ioniq's
  2.7 l/100 km on a warm UDDS (its FTP bags 2 and 3), so a result below it
  means energy from nowhere.
"""
from __future__ import annotations

import pytest
from helpers import example_result, series

from app.schemas import Project
from app.solver import simulate
from app.solver.maps import parse_table2d
from app.solver.network import build_model
from app.solver.runtime import RPM
from app.solver.verdict import trace_metrics
from app.storage import load_example

EXAMPLES = ("bev-car", "hybrid-car")
MPH = 1.609344
# 1 kWh put into (taken from) the hybrid's battery is worth this much fuel
# the engine burnt (saved): petrol 42.9 MJ/kg x 0.745 kg/l = 8.88 kWh/l,
# turned into stored charge at about 30 % (engine about 36 %, generator and
# battery about 85 %).
L_PER_KWH_STORED = 1.0 / (0.30 * 8.88)


def _summary(result) -> dict[str, float]:
    return {s.label: s.value for s in result.summary}


def _case(project: Project, case_id: str):
    return next(c for c in project.cases if c.id == case_id)


def _params(project_id: str, case_id: str) -> dict:
    """Element parameters as the case runs them (element and case overrides
    over the library defaults)."""
    project = load_example(project_id)
    return build_model(project, {}, _case(project, case_id).parameterOverrides).params_of


def _run_cases():
    """Every shipped case except the live (paced) copies, which the next
    test compares with the case they copy."""
    return [(pid, c.id) for pid in EXAMPLES for c in load_example(pid).cases
            if not c.realtimeFactor]


# ---- every case -----------------------------------------------------------------

@pytest.mark.parametrize("project_id,case_id", _run_cases())
def test_every_shipped_case_succeeds_and_follows_its_cycle(project_id, case_id):
    result = example_result(project_id, case_id)
    assert result.status == "success", [m.text for m in result.messages]
    assert [m.text for m in result.messages if m.level != "info"] == []
    assert [s.label for s in result.summary if s.notValid] == []
    speed = series(result, "el-vehicle", "sig_speed")
    target = series(result, "el-task", "sig_demand")
    m = trace_metrics([p["t"] for p in speed], [p["value"] for p in target],
                      [p["value"] for p in speed])
    assert m.rms_kmh < 1.0
    assert m.outside_wltp_s == 0
    assert _summary(result)["Distance driven"] == pytest.approx(m.cycle_km, rel=0.01)


@pytest.mark.parametrize("project_id", EXAMPLES)
def test_live_cases_are_paced_copies_of_a_checked_case(project_id):
    cases = load_example(project_id).cases
    for live in (c for c in cases if c.realtimeFactor):
        twins = [c for c in cases if not c.realtimeFactor
                 and c.model_dump(exclude={"id", "name", "realtimeFactor"})
                 == live.model_dump(exclude={"id", "name", "realtimeFactor"})]
        assert twins, f"{live.name} does not copy any non-live case"


# ---- Battery Electric Car ---------------------------------------------------------

def _consumption(case_id: str) -> float:
    return _summary(example_result("bev-car", case_id))["Consumption"]


def test_bev_wltc_consumption_is_that_of_a_compact_bev():
    assert 13.0 <= _consumption("case-wltc") <= 17.0


def test_bev_heating_case_adds_its_heating_load():
    """The heating/air-con case differs only by the 2.5 kW Power Consumer
    (0.25 kW otherwise), so it uses that extra load's energy more."""
    base, hvac = _params("bev-car", "case-wltc"), _params("bev-car", "case-wltc-hvac")
    extra_kw = hvac["el-consumer"]["power_kW"] - base["el-consumer"]["power_kW"]
    assert extra_kw == pytest.approx(2.25)
    km = _summary(example_result("bev-car", "case-wltc"))["Distance driven"]
    expected = extra_kw * 1800 / 3600 / km * 100
    assert _consumption("case-wltc-hvac") - _consumption("case-wltc") == pytest.approx(
        expected, rel=0.05)


@pytest.fixture(scope="module")
def bev_full_power():
    """The BEV with its target held far above its top speed for 100 s."""
    project = load_example("bev-car")
    case = _case(project, "case-city")
    case.duration, case.timeStep = 100, 0.1
    case.parameterOverrides = {"el-task": {"profile": "0:0; 0.1:250; 100:250"}}
    return simulate(project, case.id), build_model(project).params_of


def _time_to(result, kmh: float) -> float:
    pts = series(result, "el-vehicle", "sig_speed")
    for a, b in zip(pts, pts[1:]):
        if a["value"] < kmh <= b["value"]:
            return a["t"] + (kmh - a["value"]) / (b["value"] - a["value"]) * (b["t"] - a["t"])
    raise AssertionError(f"never reached {kmh} km/h")


def test_bev_accelerates_like_the_reference_car(bev_full_power):
    result, _ = bev_full_power
    assert _time_to(result, 60 * MPH) == pytest.approx(6.8, rel=0.10)
    assert _time_to(result, 100) == pytest.approx(7.3, rel=0.10)


def test_bev_top_speed_is_the_reference_cars_and_within_the_motors_limit(bev_full_power):
    result, params = bev_full_power
    v_max = max(p["value"] for p in series(result, "el-vehicle", "sig_speed"))
    assert v_max == pytest.approx(160, rel=0.02)
    # the motor's maximum speed: its full-load curve ends there at zero
    # torque (its Maximum Speed is left at 0, which means that last point)
    full_load = parse_table2d(params["el-motor"]["full_load_torque"])
    assert all(curve[-1][1] == 0 for _, curve in full_load)
    n_max = max(curve[-1][0] for _, curve in full_load)
    motor_rpm = max(p["value"] for p in series(result, "el-motor", "sig_speed"))
    assert motor_rpm <= n_max
    ratio = params["el-final-drive"]["ratio"] * params["el-diff"]["ratio"]
    v_at_n_max = n_max / RPM / ratio * params["el-wheel-fl"]["radius_m"] * 3.6
    assert v_max <= v_at_n_max


# ---- P2 Hybrid Car ----------------------------------------------------------------

def _hybrid(case_id: str):
    result = example_result("hybrid-car", case_id)
    soc = series(result, "el-battery", "sig_soc")
    capacity = _params("hybrid-car", case_id)["el-battery"]["capacity_kWh"]
    stored_kwh = (soc[-1]["value"] - soc[0]["value"]) / 100 * capacity
    s = _summary(result)
    litres_per_100 = s["Fuel consumption"]
    corrected = litres_per_100 - stored_kwh * L_PER_KWH_STORED * 100 / s["Distance driven"]
    return result, soc, litres_per_100, corrected


@pytest.mark.parametrize("case_id", ["case-udds", "case-hwfet"])
def test_hybrid_uses_the_fuel_of_a_real_p2_hybrid(case_id):
    _, soc, litres, corrected = _hybrid(case_id)
    assert 2.0 <= litres <= 5.0
    assert 2.0 <= corrected <= 4.5
    assert abs(soc[-1]["value"] - soc[0]["value"]) < 1.0  # charge-sustaining


def test_hybrid_holds_its_charge_from_any_start():
    """The strategy, not the chosen start SOC, sustains the charge: started
    5 % above its usual start, the Mixed Cycle still ends where it usually
    ends (the shipped cases start there, as a preconditioning drive would
    leave them)."""
    project = load_example("hybrid-car")
    case = _case(project, "case-mixed")
    usual = case.parameterOverrides["el-battery"]["initial_soc_pct"]
    case.parameterOverrides = {"el-battery": {"initial_soc_pct": usual + 5}}
    soc = series(simulate(project, case.id), "el-battery", "sig_soc")
    assert soc[-1]["value"] == pytest.approx(usual, abs=1.0)


@pytest.mark.parametrize("case_id", ["case-udds", "case-hwfet", "case-mixed"])
def test_hybrid_engine_stops_at_standstill_and_never_free_revs(case_id):
    result, soc, _, _ = _hybrid(case_id)
    speed = series(result, "el-vehicle", "sig_speed")
    on = series(result, "el-hcu", "engine_on")
    clutch = series(result, "el-hcu", "clutch_cmd")
    fuel = series(result, "el-engine", "sig_fuel_rate")
    n_eng = series(result, "el-engine", "sig_speed")
    n_in = series(result, "el-motor", "sig_speed")
    stopped = [i for i, p in enumerate(speed) if p["value"] < 0.5]
    assert stopped and all(on[i]["value"] == 0 and fuel[i]["value"] == 0 for i in stopped)
    starts = sum(1 for a, b in zip(on, on[1:]) if a["value"] < 0.5 <= b["value"])
    assert starts >= 1
    # declutched, the fired engine only ever runs up to the input shaft
    for i, p in enumerate(on):
        if p["value"] > 0.5 and clutch[i]["value"] < 0.5:
            assert n_eng[i]["value"] <= n_in[i]["value"] + 200
    # the battery stays within 55 +/- 5 %, the charge the strategy holds
    assert all(50.0 <= p["value"] <= 60.0 for p in soc)
