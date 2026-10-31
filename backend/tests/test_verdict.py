"""Run verdict: a run is a "success" only when the vehicle actually drove
the cycle. Before, the status reflected only whether a warning had been
printed, so a car with its motor deleted "succeeded" after 0 km."""
import copy
import re

import pytest
from helpers import bev_axle, dbc, el, example_result, series, sig_port

from app.schemas import ElementInstance, StoredRun, StudyPoint
from app.solver import MapUse, simulate
from app.solver.verdict import beyond_data, trace_metrics
from app.storage import load_example


def _summary(result):
    return {s.label: s for s in result.summary}


def _without(proj, el_id):
    proj = copy.deepcopy(proj)
    system = proj.systems[0]
    system.elements = [e for e in system.elements if e.id != el_id]
    system.connections = [c for c in system.connections
                          if el_id not in (c.sourceElementId, c.targetElementId)]
    proj.dataBusConnections = [c for c in proj.dataBusConnections
                               if el_id not in (c.element1Id, c.element2Id)]
    return proj


def test_trace_band_allows_one_second_of_lag_but_not_two():
    """The WLTP band: ±2 km/h around the target's range within ±1 s."""
    ts = [i * 0.5 for i in range(121)]  # 60 s, a 2 km/h/s ramp to 60 km/h then 60
    target = [min(60.0, 2.0 * t) for t in ts]

    def lagging(lag):
        return [min(60.0, max(0.0, 2.0 * (t - lag))) for t in ts]

    close = trace_metrics(ts, target, lagging(0.9))
    assert close.max_err_kmh == pytest.approx(-1.8)
    assert close.outside_wltp_s == 0.0
    late = trace_metrics(ts, target, lagging(2.5))
    assert late.max_err_kmh == pytest.approx(-5.0)  # 3 km/h beyond the 1 s band
    # from t = 2 s (the band's lower edge 2 (t - 1) - 2 rises above the
    # standing car) to 31.5 s (the speed 2 (t - 2.5) reaches the plateau's
    # 60 - 2), exactly: before, each 0.5 s sample outside counted its step
    # (29.0 s, from 2.5 to 31 s)
    assert late.outside_wltp_s == pytest.approx(29.5, abs=1e-12)
    assert late.outside_epa_s == 0.0  # 3 km/h off is inside the wider ±2 mph band
    assert late.cycle_km == pytest.approx(close.cycle_km)
    assert close.cycle_km == pytest.approx((30 * 30 + 60 * 30) / 3600)  # ramp + plateau


def test_the_time_outside_the_band_is_exact_on_the_samples_lines():
    """The time outside each band is that of the lines through the trace's
    samples, exactly (40 CFR 1066.425(b)(1)-(2), WLTP 2.6.8.3.1: at each
    instant the band spans the target's lowest and highest point within 1 s
    of it): 50 km/h held, the speed rising at 1 km/h/s from 10 s to 56 km/h,
    held, falling back from 20 s. Above 52 km/h from 12 to 24 s; above 50 +
    3.218688 (2 mi/h) from 13.218688 to 22.781312 s. Before, each 0.1 s
    sample outside counted its whole step (EPA: 9.6 s)."""
    ts = [k / 10 for k in range(601)]
    target = [50.0] * len(ts)
    speed = [50.0 + max(0.0, min(6.0, t - 10.0, 26.0 - t)) for t in ts]
    m = trace_metrics(ts, target, speed)
    assert m.outside_wltp_s == pytest.approx(12.0, abs=1e-9)
    assert m.outside_epa_s == pytest.approx(9.562624, abs=1e-9)
    # a lag on a ramp, the band's edge moving: the target 0 to 60 km/h from 10
    # to 40 s, the speed 3 s behind it. The lower edge, 2 (t - 11) - 2, rises
    # above the standing car at 12 s; the speed, 2 (t - 13), stays below it
    # until the edge holds at 58 km/h (from 41 s) and the speed reaches 58 at
    # 42 s: 30 s
    target = [min(60.0, max(0.0, 2.0 * (t - 10.0))) for t in ts]
    speed = [min(60.0, max(0.0, 2.0 * (t - 13.0))) for t in ts]
    m = trace_metrics(ts, target, speed)
    assert m.outside_wltp_s == pytest.approx(30.0, abs=1e-9)


def test_the_cycle_not_followed_message_says_whose_rule_its_allowance_is():
    """The 1 % (at least 2 s) allowed outside the band is LightSim's own
    rule, not the EPA's (2.0 s on any one occasion, 40 CFR
    1066.425(b)(4)(i)) nor the WLTP's (1 s on any one occasion, ten a cycle,
    2.6.8.3.1): the message says so."""
    result = simulate(bev_axle(profile="0:0; 3:200; 30:200"), "case")
    text = next(m.text for m in result.messages if m.text.startswith("Cycle not followed"))
    assert "LightSim's own rule" in text
    assert "40 CFR 1066.425(b)(4)(i)" in text and "2.6.8.3.1" in text


def test_following_the_cycle_is_a_success():
    result = simulate(bev_axle(profile="0:0; 5:60; 30:60"), "case")
    assert result.status == "success", [m.text for m in result.messages]
    assert all(s.notValid is None for s in result.summary)


def test_a_car_that_cannot_keep_up_is_not_a_success():
    """The target asks for more than the car can do: before, 'success'."""
    proj = bev_axle(profile="0:0; 3:200; 30:200")
    result = simulate(proj, "case")
    assert result.status == "warning"
    texts = [m.text for m in result.messages if m.level == "warning"]
    assert any(t.startswith("Cycle not followed: the speed was outside the ±2 km/h, ±1 s")
               for t in texts), texts
    s = _summary(result)
    assert s["Consumption"].notValid == "cycle not followed"
    assert s["Distance driven"].notValid is None


@pytest.mark.parametrize("profile, duration", [
    # lags each 4 s ramp to 40 km/h by less than 1 s: inside the band
    ("; ".join(f"{20 * k}:0; {20 * k + 4}:40; {20 * k + 14}:40; {20 * k + 18}:0"
               for k in range(5)) + "; 100:0", 100),
    ("0:0; 3:200; 30:200", 30),  # cannot keep up: outside the band
])
def test_the_verdict_does_not_depend_on_the_output_step(profile, duration):
    """The trace is sampled every 0.1 s of solver time, whatever the case
    step. Before, it was sampled at the case step, so the ±1 s band came from
    stored points up to 2 s apart: the lagging ramps were a success at 0.1 s
    and 1 s but 'Cycle not followed' at 1.5 s and 2 s."""
    verdicts = {}
    for step in (0.1, 1.0, 1.5, 2.0):
        proj = bev_axle(profile=profile)
        proj.cases[0].duration = duration
        proj.cases[0].timeStep = step
        result = simulate(proj, "case")
        verdicts[step] = (result.status,
                          [m.text for m in result.messages if m.level != "info"],
                          {s.label: s.notValid for s in result.summary})
    for step in (1.0, 1.5, 2.0):
        assert verdicts[step] == verdicts[0.1], step


def test_a_car_that_does_not_move_fails_and_says_so_early():
    """The bundled BEV with its E-Motor deleted: before, 'success' after
    driving 0.0 of 7.29 km."""
    proj = _without(load_example("bev-car"), "el-motor")
    proj.cases[0].duration = 120
    streamed = []
    result = simulate(proj, proj.cases[0].id, emit=streamed.append)
    assert result.status == "failed"
    assert any(m.level == "error" and m.text.startswith("The vehicle did not drive the cycle: "
                                                        "0.00 of")
               for m in result.messages)
    # the live stream warns with the t = 60 s point, long before the run ends
    first = next(i for i, ev in enumerate(streamed)
                 if ev["type"] == "message" and ev["text"].startswith("No distance covered after"))
    assert next(ev["t"] for ev in streamed[first:] if ev["type"] == "step") == 60.0


def test_the_check_that_the_car_drove_by_60_s_on_a_finished_trace():
    """verdict.first_not_driven, the new engine's path's walk of its trace:
    the target's distance by the trapezoid rule (0 to 14 km/h over 60 s:
    117 m, not the 175 m or 58 m of either rectangle), from 60 s on, at the
    first traced point that meets it, in today's words (not_driven, which
    today's CycleTrace asks too)."""
    from app.solver.verdict import Trace, first_not_driven, not_driven
    trace = Trace(ctx=None, times=[0.0, 30.0, 60.0, 60.1], target=[0.0, 7.0, 14.0, 14.0],
                  speed=[0.0] * 4, dist=[0.0, 0.0, 0.0, 0.0])
    assert first_not_driven(trace) == not_driven(60.0, 116.67, 0.0) == (
        "No distance covered after 60 s although the drive cycle asks for 0.12 km so far — check that "
        "a motor or engine drives the wheels and gets a command.")
    trace.dist = [0.0, 0.0, 5.9, 6.0]  # 5 % of 117 m driven: none
    assert first_not_driven(trace) is None
    assert not_driven(59.9, 500.0, 0.0) is None and not_driven(60.0, 100.0, 0.0) is None


def test_an_empty_battery_flags_the_consumption():
    """The bundled BEV started just above its minimum SOC (10 % then, 4 %
    since the Cupra Born rework): before, it drove the whole cycle on energy
    it did not have and reported a consumption 13 times too low with only a
    warning."""
    proj = load_example("bev-car")
    battery = next(e for e in proj.systems[0].elements if e.id == "el-battery")
    floor = float(battery.parameterOverrides.get("min_soc_pct", 10))
    proj.cases[0].duration = 120
    proj.cases[0].parameterOverrides = {"el-battery": {"initial_soc_pct": floor + 0.2}}
    result = simulate(proj, proj.cases[0].id)
    assert result.status == "warning"
    s = _summary(result)
    assert s["Consumption"].notValid in ("cycle not followed", "the battery reached its minimum SOC")
    assert any(m.text.startswith("Cycle not followed") for m in result.messages)


def test_non_finite_values_fail_the_run():
    proj = bev_axle(profile="0:0; 5:30; 10:30")
    proj.cases[0].duration = 10
    code = ("def step(t, dt, inputs, state, params):\n"
            "    return {'y': float('nan') if t > 5 else 0.0}\n")
    proj.systems[0].elements.append(ElementInstance(
        id="nan", componentDefId="signal.script", label="NaN maker",
        position={"x": 0, "y": 0}, parameterOverrides={"code": code},
        dynamicPorts=[sig_port("y", "output")]))
    proj.systems[0].elements.append(el("mon", "signal.monitor", "Monitor"))
    proj.dataBusConnections.append(dbc(50, "nan", "y", "mon", "sig_in"))
    result = simulate(proj, "case")
    assert result.status == "failed"
    assert any("Non-finite values" in m.text and "nan:y" in m.text for m in result.messages)
    assert series(result, "nan", "y")[-1]["value"] != series(result, "nan", "y")[-1]["value"]
    # no headline number is shown unflagged (before: Consumption, energies
    # and the rest showed plain numbers); how far the run got stays valid
    s = _summary(result)
    assert any(label.endswith("energy delivered") for label in s)
    assert {label: row.notValid for label, row in s.items() if label != "Simulated duration"} == {
        label: "the solution broke down" for label in s if label != "Simulated duration"}
    assert s["Simulated duration"].notValid is None


def test_a_cancelled_run_flags_its_figures_per_distance():
    """A run stopped part-way ends "cancelled" (before, "warning", the status
    of a run that finished with a problem), and its Consumption does not show
    as a plain number for a cycle it did not finish. The run history and a
    study keep the status."""
    proj = load_example("bev-car")
    calls = {"n": 0}

    def control():
        calls["n"] += 1
        return [{"type": "cancel"}] if calls["n"] == 31 else []

    result = simulate(proj, proj.cases[0].id, control=control)
    assert result.status == "cancelled"
    s = _summary(result)
    assert s["Simulated duration"].value == 30
    assert s["Consumption"].notValid == "run cancelled at t = 30 s"
    assert s["Distance driven"].notValid is None
    StoredRun(id="r", caseId=result.caseId, caseName="c", startedAt=0, status=result.status,
              result=result)
    StudyPoint(values=[1.0], status=result.status)


def test_a_stop_after_the_last_step_leaves_a_complete_run():
    """A stop that arrives while the last step is paced comes too late to cut
    anything short. Before, the complete run said "warning" with no warning
    message."""
    proj = bev_axle(profile="0:0; 5:60; 30:60")
    proj.cases[0].duration = 1.0
    proj.cases[0].timeStep = 1.0
    proj.cases[0].realtimeFactor = 1.0
    calls = {"n": 0}

    def control():
        calls["n"] += 1  # the 2nd poll is in the last step's pacing wait
        return [{"type": "cancel"}] if calls["n"] >= 2 else []

    result = simulate(proj, "case", control=control)
    assert calls["n"] >= 2
    assert result.status == "success", [m.text for m in result.messages]
    assert not any("cancelled" in m.text for m in result.messages)
    assert all(s.notValid is None for s in result.summary)


def test_a_performance_test_is_timed_only_from_below_its_target_and_to_the_end():
    """A stopped performance test was stopped, it did not fall short of its
    target; a car that starts at its target or above has no time to it.
    Before, the first said "did not reach the 100 km/h target" and the
    second got "Time to 100 km/h" = 0.0 s."""
    proj = bev_axle(profile="0:100; 30:100")
    proj.cases[0].kind = "performance"
    calls = {"n": 0}

    def control():
        calls["n"] += 1
        return [{"type": "cancel"}] if calls["n"] == 3 else []

    stopped = simulate(proj, "case", control=control)
    assert stopped.status == "cancelled"
    assert not any("did not reach" in m.text for m in stopped.messages)
    veh = next(e for e in proj.systems[0].elements if e.id == "veh")
    veh.parameterOverrides["initial_speed_kmh"] = 120
    rolling = simulate(proj, "case")
    assert rolling.status == "success", [m.text for m in rolling.messages]
    assert not [s.label for s in rolling.summary if s.label.startswith("Time to")]
    assert not any("did not reach" in m.text for m in rolling.messages)


def test_beyond_data_allowance_is_one_percent_of_the_run_and_at_least_2_s():
    """The trace's allowance, per element, on its longest record."""
    def reasons(duration_s, *outside_s):
        uses = [MapUse("m", "'Full-Load Torque' table", "Speed", "1/min", 12000.0, s, 21333.0, 80.0)
                for s in outside_s]
        return [reason for _, reason in beyond_data(uses, duration_s, lambda el: "E-Motor 'M'")]

    assert reasons(600, 42) == ["E-Motor 'M' ran 9,333 1/min past its 'Full-Load Torque' "
                                "table for 42 s"]
    assert reasons(600, 6.0) == []  # 1 % of 600 s
    assert len(reasons(600, 6.1)) == 1
    assert reasons(100, 1.9) == []  # at least 2 s
    assert len(reasons(100, 2.5)) == 1
    assert reasons(600, 5, 42) == reasons(600, 42)  # not 47 s: one operating point


def test_a_motor_run_past_its_voltage_data_is_named_with_excess_and_time():
    """A battery above the motor map's 396 V for the whole run. Before, it
    was a success (its voltage axis holds the edge value, MOD-18), with
    Consumption shown as valid. The first touch is info, not a warning
    (which made a run 'warning' however briefly it happened): the verdict
    judges the time outside."""
    proj = bev_axle(profile="0:0; 5:60; 30:60")
    batt = next(e for e in proj.systems[0].elements if e.id == "batt")
    batt.parameterOverrides["ocv_table"] = {"0": 430, "100": 440}
    result = simulate(proj, "case")
    assert result.status == "warning"
    [warning] = [m.text for m in result.messages if m.level == "warning"]
    assert warning.startswith("E-Motor 'E-Motor' ran ")
    assert int(re.search(r"V past its 'Full-Load Torque' table for (\d+) s of 30 s",
                         warning).group(1)) >= 25
    s = _summary(result)
    assert s["Consumption"].notValid.startswith("E-Motor 'E-Motor' ran")
    assert s["Distance driven"].notValid is None
    assert any("Full-Load Torque: Voltage 4" in m.text and "(396 V)" in m.text
               for m in result.messages if m.level == "info")
    # once per solver step: the handshake's and the Driver's trial reads of
    # the map would take it past 100 %
    assert 80 < s["E-Motor — time outside its 'Full-Load Torque' table (Voltage)"].value <= 100
    assert s["E-Motor — furthest Voltage outside its 'Full-Load Torque' table"].value > 420


def test_a_motor_above_its_maximum_speed_is_named():
    """A top-speed test started at 140 km/h with the motor's maximum speed
    set to 10,000 1/min: the car drives it past for 8 s before it slows
    down. Before, the performance test was a success with valid figures."""
    proj = bev_axle(profile="0:150; 60:150")
    proj.cases[0].duration, proj.cases[0].kind = 60, "performance"
    for e in proj.systems[0].elements:
        if e.id == "mot":
            e.parameterOverrides["max_speed_rpm"] = 10000
        elif e.id == "veh":
            e.parameterOverrides["initial_speed_kmh"] = 140
    result = simulate(proj, "case")
    assert result.status == "warning"
    [warning] = [m.text for m in result.messages if m.level == "warning"]
    assert warning.startswith("E-Motor 'E-Motor' ran ")
    assert " 1/min past its maximum speed for " in warning
    assert "its maximum speed is 10,000 1/min" in warning
    s = _summary(result)
    assert s["Maximum speed"].notValid == s["Consumption"].notValid == warning.split(" of 60 s")[0]


def test_bundled_examples_follow_their_cycles():
    for pid in ("bev-car", "hybrid-car"):
        result = example_result(pid, load_example(pid).cases[0].id)
        assert result.status != "failed", pid
        assert not any(m.text.startswith("Cycle not followed") for m in result.messages), pid
        assert _summary(result)["Distance driven"].value > 7.0


def test_the_time_outside_the_band_is_the_same_with_and_without_numpy(monkeypatch):
    """band_outside proves most pieces add nothing with NumPy where it is
    there (the new engine's module brings it) before working out the rest
    one by one: the same bits as working out every piece, on traces that
    follow their target and leave the band, with a not-a-number and signed
    zeros in them."""
    import math
    import random
    import sys

    from app.solver import verdict

    rnd = random.Random(7)
    traces = []
    for n in (2, 3, 60, 700):
        ts = [0.0]
        for _ in range(n - 1):
            ts.append(ts[-1] + rnd.choice((0.1, 0.1, 0.05, 0.3, 1e-9)))
        tgt = [max(0.0, 50.0 + 30.0 * math.sin(t / 7.0) + rnd.uniform(-1, 1)) for t in ts]
        spd = [g + (rnd.uniform(-6.0, 6.0) if rnd.random() < 0.1 else rnd.uniform(-0.5, 0.5)) for g in tgt]
        traces.append((ts, tgt, spd))
    ts, tgt, spd = (list(x) for x in traces[2])
    tgt[10], spd[20], tgt[0], spd[1] = float("nan"), -0.0, -0.0, 0.0
    traces.append((ts, tgt, spd))
    tols = (verdict.WLTP_TOL_KMH, verdict.EPA_TOL_KMH)

    def bits(rows):
        return [[float.hex(x) for x in r] for r in rows]

    fast = bits(verdict.band_outside(*tr, tols) for tr in traces)
    assert any(x != "0x0.0p+0" for r in fast for x in r)  # (some time outside)
    monkeypatch.setitem(sys.modules, "numpy", None)  # (import numpy fails)
    assert verdict._zero_pieces(*traces[3], tols, [0.0, 1.0], verdict._RangeMinMax(traces[3][1])) is None
    assert bits(verdict.band_outside(*tr, tols) for tr in traces) == fast
