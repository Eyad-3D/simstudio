"""Formula Student events and points (MOD-43).

The scoring formulas are checked against hand calculations from FS Rules
2026 v1.1 (FSG) D 9.1.1, table 11 and D 9.4, and the FSG 2020 efficiency
against the FSG score calculator's formulas (the calculator ships no test
values, so these are hand calculations of its formulas). The event runs
are checked on the FS example car and the test FS car."""
import time

import pytest
from helpers import fs_car, series

from app.schemas import SimCase
from app.solver import fs_events, simulate
from app.storage import load_example


def _rows(result):
    return {s.label: s for s in result.summary}


def test_dynamic_points_follow_d_9_1_1():
    # at Tmin the full points, at and beyond Tmax the minimum (Pmin = 5 % of 50)
    assert fs_events.points("acceleration", 4.0, 4.0) == pytest.approx(50.0)
    assert fs_events.points("acceleration", 6.8, 4.0) == pytest.approx(2.5)
    assert fs_events.points("acceleration", 9.0, 4.0) == pytest.approx(2.5)
    # Tmax = 1.7 × 4 = 6.8 s: 47.5 × ((6.8 − 5.2) / 2.8)² + 2.5
    assert fs_events.points("acceleration", 5.2, 4.0) == pytest.approx(18.0102, abs=1e-4)
    # skidpad Tmax 1.35 Tmin, autocross 1.4 (Pmin 10 %), endurance 1.5 (Pmin 10 %)
    assert fs_events.points("skidpad", 5.0, 4.8) == pytest.approx(
        47.5 * ((6.48 - 5.0) / (6.48 - 4.8)) ** 2 + 2.5)
    assert fs_events.points("autocross", 60.0, 50.0) == pytest.approx(90 * (10 / 20) ** 2 + 10)
    assert fs_events.points("endurance", 1500.0, 1200.0) == pytest.approx(
        225 * (300 / 600) ** 2 + 25)
    # faster than the reference: the full points, not more
    assert fs_events.points("autocross", 45.0, 50.0) == pytest.approx(100.0)


def test_efficiency_follows_d_9_4():
    ef_min = fs_events.efficiency_factor(1200.0, 4.0)
    assert ef_min == pytest.approx(1200.0 ** 2 * 4.0)
    assert fs_events.efficiency_points(ef_min, ef_min) == pytest.approx(75.0)
    assert fs_events.efficiency_points(1.5 * ef_min, ef_min) == pytest.approx(18.75)
    assert fs_events.efficiency_points(2.5 * ef_min, ef_min) == 0.0
    assert fs_events.efficiency_points(0.5 * ef_min, ef_min) == pytest.approx(75.0)


def test_fsg_2020_efficiency_as_the_score_calculator():
    # (tMin·enMin²)/(tTeam·enTeam²) and 100·((0.1/eTeam − 1)/(0.1/eMax − 1))
    ef = fs_events.efficiency_factor_2020(1500.0, 1300.0, 6.0, 4.5)
    assert ef == pytest.approx(1300 * 4.5 ** 2 / (1500 * 6.0 ** 2))
    assert ef == pytest.approx(0.4875)
    assert fs_events.efficiency_score_2020(0.4875, 0.8) == pytest.approx(
        100 * ((0.1 / 0.4875 - 1) / (0.1 / 0.8 - 1)))
    assert fs_events.efficiency_score_2020(0.8, 0.8) == pytest.approx(100.0)


def test_score_without_a_reference_says_what_to_set():
    out = fs_events.score(fs_events.EventResult("autocross", 60.0), None)
    assert out["points"] is None
    assert any("Reference time" in n for n in out["notes"])
    dq = fs_events.score(fs_events.EventResult("autocross", 60.0, breach="power over 80 kW"), 50.0)
    assert dq["points"] == 0.0
    dnf = fs_events.score(fs_events.EventResult("endurance", 1500.0, finished=False,
                                                energy_kwh=5.0), 1200.0, 4.0)
    assert dnf["points"] == 0.0 and dnf["efficiency_points"] == 0.0


def _events(proj):
    return [
        SimCase(id="acc", name="Acceleration", kind="acceleration", duration=25, timeStep=0.01,
                endDistance=75, startLine=0.3, fsEvent="acceleration", referenceTime=3.9),
        SimCase(id="skid", name="Skidpad", kind="lap", fsEvent="skidpad", referenceTime=4.9,
                parameterOverrides={"el-track": {"layout": "Skidpad", "laps": 2}}),
        SimCase(id="ax", name="Autocross", kind="lap", fsEvent="autocross", referenceTime=55.0,
                parameterOverrides={"el-track": {"layout": "Autocross", "laps": 1}}),
        SimCase(id="end", name="Endurance", kind="lap", fsEvent="endurance", outputEvery=5,
                referenceTime=1300.0, referenceEnergy=4.5,
                parameterOverrides={"el-track": {"layout": "Autocross", "laps": 22},
                                    "el-battery": {"output_power_limit_kW": 30}}),
    ]


def test_four_events_score_on_the_fs_example_in_time():
    """The item's metric: the four events run from one template in under
    10 s for the sample car (measured 8.9 s on the development machine; the
    test allows 20 s for slower CI runners)."""
    proj = load_example("fs-electric")
    proj.cases = _events(proj)
    t0 = time.monotonic()
    results = {c.id: simulate(proj, c.id) for c in proj.cases}
    took = time.monotonic() - t0
    assert took < 20.0
    for cid, r in results.items():
        assert r.status == "success", [m.text for m in r.messages if m.level != "info"]
    acc, skid, ax, end = (_rows(results[k]) for k in ("acc", "skid", "ax", "end"))
    assert acc["Acceleration time (FS Rules 2026 v1.1 (FSG))"].value == acc["Time to 75 m"].value
    # skidpad: the mean of the timed right and left circles (D 4.2.2)
    assert skid["Skidpad time (FS Rules 2026 v1.1 (FSG))"].value == pytest.approx(
        (skid["Sector 1 time"].value + skid["Sector 2 time"].value) / 2, abs=2e-3)
    assert ax["Autocross time (FS Rules 2026 v1.1 (FSG))"].value == ax["Lap time"].value
    for rows, name, p_max in ((acc, "Acceleration", 50), (skid, "Skidpad", 50),
                              (ax, "Autocross", 100), (end, "Endurance", 250)):
        pts = rows[f"{name} points (estimate)"]
        assert 0 < pts.value <= p_max and pts.limit == p_max
        assert rows["Rule check: voltage (EV 4.1.1)"].passed
        assert rows["Rule check: current (EV 2.2.2)"].passed
    assert 0 < end["Efficiency points (estimate)"].value <= 75
    # the endurance energy counts regeneration at 90 % (D 7.9.5)
    out = end["Accumulator — energy delivered"].value
    back = end["Accumulator — energy recuperated"].value
    assert end["Endurance energy (regeneration × 0.9)"].value == pytest.approx(
        out - 0.9 * back, abs=2e-3)
    assert end["Endurance finished on its energy"].passed


def test_endurance_stops_for_the_driver_change():
    proj = fs_car("Autocross", 4, battery={"output_power_limit_kW": 40, "initial_soc_pct": 60})
    case = proj.cases[0]
    case.fsEvent, case.referenceTime = "endurance", 200.0
    r = simulate(proj, "case")
    assert r.status == "success", [m.text for m in r.messages if m.level != "info"]
    rows = _rows(r)
    speed = {p["t"]: p["value"] for p in series(r, "veh", "sig_speed")}
    laps = series(r, "trk", "sig_lap")
    # the speed where lap 2 ends and lap 3 starts is 0
    end_2 = max(p["t"] for p in laps if p["value"] == 2)
    assert speed[end_2] == pytest.approx(0.0, abs=1e-6)
    # the event time leaves out the restart lap (lap 3)
    lap_ends = [0.0] + [max(p["t"] for p in laps if p["value"] == k) for k in (1, 2, 3, 4)]
    lap3 = lap_ends[3] - lap_ends[2]
    assert rows["Endurance time (FS Rules 2026 v1.1 (FSG))"].value == pytest.approx(
        rows["Total time"].value - lap3, abs=0.02)
    assert any("driver change after lap 2" in m.text for m in r.messages)


def test_a_rule_breach_scores_nothing():
    """Recuperating into the nearly full test pack lifts its terminals over
    600 V, which breaks EV 4.1.1: the run is disqualified (D 10.4.2) and
    scores 0 points, with a warning. Its power check, with no Output Power
    Limit, takes the peak over a solver step."""
    proj = fs_car("Autocross", 1)
    case = proj.cases[0]
    case.fsEvent, case.referenceTime = "autocross", 50.0
    r = simulate(proj, "case")
    rows = _rows(r)
    assert rows["Rule check: power (EV 2.2.1)"].passed is True
    volts = rows["Rule check: voltage (EV 4.1.1)"]
    assert volts.value > 600 and volts.passed is False
    assert rows["Autocross points (estimate)"].value == 0.0
    assert r.status == "warning"
    assert any("disqualifies" in m.text for m in r.messages if m.level == "warning")


def test_running_out_of_energy_does_not_finish():
    proj = fs_car("Autocross", 6, battery={"capacity_kWh": 1.0, "output_power_limit_kW": 80})
    case = proj.cases[0]
    case.fsEvent, case.referenceTime, case.referenceEnergy = "endurance", 300.0, 2.0
    r = simulate(proj, "case")
    rows = _rows(r)
    assert rows["Endurance finished on its energy"].passed is False
    assert rows["Endurance points (estimate)"].value == 0.0
    assert rows["Efficiency points (estimate)"].value == 0.0


def test_an_event_on_the_wrong_kind_of_case_is_not_scored():
    proj = fs_car("Autocross", 1)
    case = proj.cases[0]
    case.kind, case.fsEvent = "cycle", "autocross"
    r = simulate(proj, "case")
    assert not any("points" in s.label for s in r.summary)
    assert any("scored from a Lap case only" in m.text for m in r.messages)


def test_endurance_laps_make_22_km():
    assert fs_events.endurance_laps(978.915) == 22
    assert fs_events.endurance_laps(1500.0) == 15


# ---- lift-and-coast to an energy target (MOD-44) ----------------------------


def _endurance(track: dict):
    proj = load_example("fs-electric")
    case = next(c for c in proj.cases if c.id == "case-endurance")
    case.parameterOverrides["el-track"] = {"layout": "Autocross", "laps": 23, **track}
    return proj, simulate(proj, "case-endurance")


def test_lift_and_coast_trades_lap_time_for_energy():
    """The trade-off curve: more lift-and-coast before braking takes less
    energy and more time, monotonically (4 laps of the test car)."""
    times, energies = [], []
    for pct in (0, 10, 25, 40, 60):
        proj = fs_car("Autocross", 4, battery={"output_power_limit_kW": 40,
                                                "initial_soc_pct": 60})
        trk = next(e for e in proj.systems[0].elements if e.id == "trk")
        trk.parameterOverrides["coast_pct"] = pct
        r = simulate(proj, "case")
        assert r.status == "success", [m.text for m in r.messages if m.level != "info"]
        rows = _rows(r)
        times.append(rows["Total time"].value)
        energies.append(rows["Energy per lap"].value)
        assert abs(rows["Lap energy balance error"].value) < 0.5
        if pct:
            assert rows["Time limited by lift-and-coast"].value > 0
            assert rows["Lift-and-coast, mean share"].value == pct
        else:
            assert "Time limited by lift-and-coast" not in rows
    assert times == sorted(times) and len(set(times)) == len(times)
    assert energies == sorted(energies, reverse=True) and len(set(energies)) == len(energies)


@pytest.mark.parametrize("target", [5.0, 4.5])
def test_an_energy_target_is_met_within_2_percent(target):
    """The item's metric: on the FS example's endurance (23 laps, 30 kW),
    the strategy ends within 2 % of the energy target (5.0 kWh: −0.54 %,
    4.5 kWh: −0.22 %, against 5.33 kWh with no lift-and-coast)."""
    _, r = _endurance({"energy_target_kWh": target})
    assert r.status == "success", [m.text for m in r.messages if m.level != "info"]
    rows = _rows(r)
    assert rows["Energy target"].value == target
    assert abs(rows["Energy used against the target"].value) < 2.0
    assert 0 < rows["Lift-and-coast, mean share"].value < 100
    net = rows["Energy per lap"].value * 23
    assert abs(net - target) / target < 0.02


def test_an_energy_target_out_of_reach_says_so():
    _, r = _endurance({"energy_target_kWh": 2.0})
    rows = _rows(r)
    assert rows["Lift-and-coast, mean share"].value == 100
    assert rows["Energy used against the target"].value > 2.0
    assert any("Energy Target" in m.text for m in r.messages if m.level == "warning")


# ---- the endurance energy study (STU-38) -------------------------------------


def test_an_endurance_from_a_lap_trace_reports_its_energy():
    """The item's metric: from one imported lap trace (here repeated to
    5 km to keep the test short), the run reports the net energy, the end
    SOC and the RMS power, and the rule checks; no points, as its time is
    the trace's."""
    from helpers import dbc, el
    from test_laplog import _motec

    from app import laplog
    lap = laplog.read_lap(_motec(), "MoTeC i2 CSV", repeat_to_km=5, driver_change_s=60)
    proj = load_example("fs-electric")
    proj.systems[0].elements.append(el("task", "signal.driving_task", "Imported lap"))
    drv = next(e for e in proj.systems[0].elements if e.componentDefId == "driver.driver")
    proj.dataBusConnections.append(dbc(99, "task", "sig_demand", drv.id, "sig_target_in"))
    case = proj.cases[0]
    case.kind, case.endDistance, case.startLine, case.fsEvent = "cycle", None, 0.0, "endurance"
    case.duration, case.timeStep, case.outputEvery = lap.points[-1][0], 0.1, 10
    case.parameterOverrides = {"task": {"profile": lap.profile(), "cycle": ""}}
    r = simulate(proj, case.id)
    assert r.status == "success", [m.text for m in r.messages if m.level != "info"]
    rows = _rows(r)
    out = rows["Accumulator — energy delivered"].value
    back = rows["Accumulator — energy recuperated"].value
    assert rows["Net battery energy (out − back in)"].value == pytest.approx(out - back, abs=2e-3)
    assert rows["Endurance energy (regeneration × 0.9)"].value == pytest.approx(
        out - 0.9 * back, abs=2e-3)
    assert 5 < rows["RMS battery power"].value < 80
    assert 400 < rows["Lowest pack voltage"].value < 600
    assert "Accumulator — final SOC" in rows
    assert rows["Endurance finished on its energy"].passed
    assert not any("points" in label for label in rows)
    assert any("trace's own" in m.text for m in r.messages)


def test_a_lap_endurance_reports_net_energy_and_lowest_voltage():
    proj = fs_car("Autocross", 2, battery={"output_power_limit_kW": 40, "initial_soc_pct": 60})
    proj.cases[0].fsEvent = "endurance"
    rows = _rows(simulate(proj, "case"))
    assert rows["Net battery energy (out − back in)"].value == pytest.approx(
        2 * rows["Energy per lap"].value, abs=2e-3)
    assert rows["Lowest pack voltage"].value < rows["Rule check: voltage (EV 4.1.1)"].value
