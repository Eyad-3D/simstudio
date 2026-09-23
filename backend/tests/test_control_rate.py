"""Controllers run at the solver step, not at the case step: the case
timeStep only sets how often results are stored, so the answers do not
depend on it."""
import pytest
from helpers import bev_axle, dbc, el, project, series, sig_port

from app.schemas import ElementInstance
from app.solver import simulate
from app.storage import load_example
from app.validation import validate_project


def _kpis(result) -> dict[str, float]:
    return {s.label: s.value for s in result.summary if s.label != "Simulated duration"}


def _hybrid(step: float):
    """The bundled hybrid, started just above its engine-on SOC threshold so
    200 s cover engine starts, clutch engagement and gear shifts."""
    proj = load_example("hybrid-car")
    case = proj.cases[0]
    case.duration = 200
    case.timeStep = step
    case.parameterOverrides = {"el-battery": {"initial_soc_pct": 49}}
    return proj, case.id


def _bev(step: float):
    """The bundled BEV on a compressed cycle that also brakes to a stop."""
    proj = load_example("bev-car")
    case = proj.cases[0]
    case.duration = 100
    case.timeStep = step
    case.parameterOverrides = {"el-task": {"profile": "0:0; 15:60; 50:60; 65:0; 100:0"}}
    return proj, case.id


@pytest.mark.parametrize("make,active", [
    (_hybrid, "Engine — fuel used"),
    (_bev, "HV Battery Pack — energy recuperated"),
])
def test_demo_kpis_do_not_depend_on_the_output_step(make, active):
    results = {}
    for step in (1.0, 0.02):
        proj, case_id = make(step)
        result = simulate(proj, case_id)
        assert result.status in ("success", "warning"), [m.text for m in result.messages]
        results[step] = _kpis(result)
    coarse, fine = results[1.0], results[0.02]
    assert coarse.keys() == fine.keys()
    assert fine[active] > 0.01  # the engine ran / the car recuperated
    for label, value in fine.items():
        assert coarse[label] == pytest.approx(value, rel=5e-3, abs=1e-3), label


def _script_probe(step: float, duration: float = 2.0, sample_time: float | None = None):
    """A Script that counts its calls and reports the dt it is given, plus
    the battery SOC it reads (a state computed by the solver)."""
    code = ("def step(t, dt, inputs, state, params):\n"
            "    state['n'] = state.get('n', 0) + 1\n"
            "    seen = state.setdefault('socs', set())\n"
            "    seen.add(inputs['soc'])\n"
            "    return {'calls': state['n'], 'dt_seen': dt, 'socs_seen': len(seen)}\n")
    proj = bev_axle(profile="0:0; 2:40")
    proj.systems[0].elements.append(ElementInstance(
        id="scr", componentDefId="signal.script", label="Probe",
        position={"x": 0, "y": 0}, parameterOverrides={"code": code},
        dynamicPorts=[sig_port("soc", "input"), sig_port("calls", "output"),
                      sig_port("dt_seen", "output"), sig_port("socs_seen", "output")],
    ))
    if sample_time is not None:
        proj.systems[0].elements[-1].parameterOverrides["sample_time_s"] = sample_time
    proj.dataBusConnections.append(dbc(60, "batt", "sig_soc", "scr", "soc"))
    proj.cases[0].duration = duration
    proj.cases[0].timeStep = step
    return simulate(proj, "case")


def test_scripts_run_every_solver_step_with_its_dt():
    result = _script_probe(step=1.0)
    assert result.status in ("success", "warning"), [m.text for m in result.messages]
    last = {port: series(result, "scr", port)[-1] for port in ("calls", "dt_seen", "socs_seen")}
    assert last["calls"]["t"] == 2.0
    assert last["calls"]["value"] == 200  # 2 s of 10 ms solver steps
    assert last["dt_seen"]["value"] == pytest.approx(0.01)
    # the SOC input is refreshed every solver step, not once per case step
    assert last["socs_seen"]["value"] > 150


def test_pid_integrates_with_the_solver_step():
    """Pure integral action on an error that ramps as t: the output is
    ki × t² / 2, whatever the case step (a PID sampled once per 1 s case
    step reads ki × t (t − 1) / 2)."""
    for step in (1.0, 0.5):
        elements = [
            el("ramp", "signal.driving_task", "Ramp", profile="0:0; 10:10"),
            el("pid", "control.pid", "PID", kp=0.0, ki=0.1, out_min=-10, out_max=10),
        ]
        databus = [dbc(1, "ramp", "sig_demand", "pid", "sig_setpoint_in")]
        result = simulate(project(elements, [], databus, duration=3, time_step=step), "case")
        for p in series(result, "pid", "sig_out"):
            if p["t"] >= 1.0:
                assert p["value"] == pytest.approx(0.05 * p["t"] ** 2, rel=0.02), (step, p)


def test_block_with_a_sample_time_runs_at_that_rate_and_holds():
    result = _script_probe(step=0.05, sample_time=0.1)
    assert result.status in ("success", "warning"), [m.text for m in result.messages]
    calls = series(result, "scr", "calls")
    assert calls[-1]["value"] == 20  # t = 0, 0.1, … 1.9
    assert series(result, "scr", "dt_seen")[-1]["value"] == pytest.approx(0.1)
    # between samples the output holds: it changes only every other point
    # (a run at t = 0.1 k first shows in the point recorded at 0.1 k + 0.05)
    changes = [p["t"] for p, q in zip(calls[1:], calls) if p["value"] != q["value"]]
    assert changes == pytest.approx([0.1 * k + 0.05 for k in range(20)])


def test_sample_time_data_checks():
    def checks_for(sample_time):
        proj = bev_axle()
        proj.systems[0].elements.append(
            el("pid", "control.pid", "Speed PID", sample_time_s=sample_time))
        # only the Sample Time checks: the bare PID also gets an "inputs are
        # not connected" warning from the wiring checks
        return [(c.level, c.text) for c in validate_project(proj)
                if c.elementId == "pid" and ("Sample Time" in c.text or "runs only every" in c.text)]

    assert checks_for(0) == []
    assert checks_for(0.05) == []
    assert checks_for(0.5) == [(
        "warning", "'Speed PID' runs only every 0.5 s (its Sample Time); real vehicle "
                   "controllers run every 10-100 ms, so results may depend on this setting.")]
    assert [lv for lv, _ in checks_for(-1)] == ["error"]
    # before, NaN passed silently (the block then ran every solver step) and
    # infinity ran the block once at t = 0
    assert checks_for(float("nan")) == [
        ("error", "Sample Time of 'Speed PID' must be a finite number — got nan.")]
    assert [lv for lv, _ in checks_for(float("inf"))] == ["error"]
    assert [lv for lv, _ in checks_for("nan")] == ["error"]
