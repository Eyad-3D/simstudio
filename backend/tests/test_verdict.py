"""Run verdict: a run is a "success" only when the vehicle actually drove
the cycle. Before, the status reflected only whether a warning had been
printed, so a car with its motor deleted "succeeded" after 0 km."""
import copy

import pytest
from helpers import bev_axle, dbc, el, series, sig_port

from app.schemas import ElementInstance
from app.solver import simulate
from app.solver.verdict import trace_metrics
from app.storage import load_project


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
    assert late.outside_wltp_s == pytest.approx(29.0)  # t = 2.5 … 31 s
    assert late.outside_epa_s == 0.0  # 3 km/h off is inside the wider ±2 mph band
    assert late.cycle_km == pytest.approx(close.cycle_km)
    assert close.cycle_km == pytest.approx((30 * 30 + 60 * 30) / 3600)  # ramp + plateau


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
    proj = _without(load_project("bev-car"), "el-motor")
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


def test_an_empty_battery_flags_the_consumption():
    """The bundled BEV started just above its 10 % floor: before, it drove
    the whole cycle on energy it did not have and reported a consumption
    13 times too low with only a warning."""
    proj = load_project("bev-car")
    proj.cases[0].duration = 120
    proj.cases[0].parameterOverrides = {"el-battery": {"initial_soc_pct": 10.2}}
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


def test_bundled_examples_follow_their_cycles():
    for pid in ("bev-car", "hybrid-car"):
        proj = load_project(pid)
        result = simulate(proj, proj.cases[0].id)
        assert result.status != "failed", pid
        assert not any(m.text.startswith("Cycle not followed") for m in result.messages), pid
        assert _summary(result)["Distance driven"].value > 7.0
