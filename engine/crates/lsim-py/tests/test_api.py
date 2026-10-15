"""Tests of the Python module `lightsim_engine` (the Python API, DESIGN.md §13).

Run them against the built module (see ../README.md):

    python -m pytest engine/crates/lsim-py/tests/test_api.py

The comparisons with the golden path run `golden::run_case` through the
Rust example `dump_case` (needs cargo; skipped without it) and compare the
two runs bit by bit.
"""
from __future__ import annotations

import json
import shutil
import subprocess
import sys
import threading
import time
from pathlib import Path

import numpy as np
import pytest

import lightsim_engine as le

ENGINE = Path(__file__).resolve().parents[3]
REPO = ENGINE.parent
BACKEND = REPO / "backend"
BEV = (BACKEND / "projects" / "bev-car.json").read_text(encoding="utf-8")
HYBRID = (BACKEND / "projects" / "hybrid-car.json").read_text(encoding="utf-8")
WLTC = "case-wltc"

# the engine's names of two parameters of the BEV project
CD = "el_vehicle.cd"  # the drag coefficient: a runtime parameter
MASS = "el_vehicle.mass_kg"  # structural: a change needs a new build
SOC0 = "el_battery.initial_soc_pct"


@pytest.fixture(scope="module")
def bev():
    return le.build(BEV, WLTC)


@pytest.fixture(scope="module")
def bev_run(bev):
    return bev.simulate()


def figure(run, key):
    return next(f["value"] for f in run.figures if f["key"] == key)


# ---- build -----------------------------------------------------------------

def test_module_basics():
    assert isinstance(le.version(), str)
    assert issubclass(le.ModelError, ValueError)


def test_build_report(bev):
    r = bev.report
    assert r["sizes"]["states"] > 0 and r["sizes"]["flat_variables"] > 0
    assert r["prepare_seconds"] >= 0 and r["compile_seconds"] > 0
    assert r["cache_hit"] is False
    assert len(r["structure_key"]) == 64
    assert r["case"]["id"] == WLTC and r["case"]["duration"] == 1800.0
    assert isinstance(r["warnings"], list)
    for w in r["warnings"]:
        assert set(w) == {"severity", "code", "message", "parts", "hint", "detail"}


def test_param_names(bev):
    names = bev.param_names()
    assert MASS in names and SOC0 in names
    assert all(isinstance(n, str) for n in names)


def test_build_cache(tmp_path):
    first = le.build(BEV, WLTC, cache_dir=str(tmp_path))
    second = le.build(BEV, WLTC, cache_dir=str(tmp_path))
    assert first.report["cache_hit"] is False
    assert second.report["cache_hit"] is True
    assert second.report["structure_key"] == first.report["structure_key"]
    # a model from the cache is the model: the same run, bit for bit
    plain = le.build(BEV, WLTC).simulate(t_end=60.0)
    for model in (first, second):
        run = model.simulate(t_end=60.0)
        assert set(run.channels) == set(plain.channels)
        assert all(_same_bits(run.channels[k], plain.channels[k]) for k in plain.channels)


# ---- simulate --------------------------------------------------------------

def test_simulate_is_sane(bev_run):
    run = bev_run
    assert run.times.dtype == np.float64
    assert run.times[0] == 0.0 and run.times[-1] == 1800.0 and len(run.times) == 1801
    soc = run.channels["el-battery:sig_soc"]
    assert soc.shape == run.times.shape
    assert soc[0] == pytest.approx(90.0) and 80.0 < soc[-1] < soc[0]
    speed = run.channels["el-vehicle:sig_speed"]  # km/h, today's unit
    assert 130.0 < speed.max() < 133.0  # the WLTC's top speed, 131.3 km/h
    assert abs(figure(run, "distance_km") - 23.27) < 0.1
    assert 10.0 < figure(run, "consumption_kwh_per_100km") < 20.0
    assert figure(run, "el-battery.final_soc_pct") == pytest.approx(soc[-1], abs=1e-9)
    assert figure(run, "simulated_duration_s") == 1800.0
    assert all(set(f) == {"key", "label", "value", "unit"} for f in run.figures)


def test_run_shapes(bev_run):
    run = bev_run
    n = len(run.times)
    for series in (run.channels, run.min, run.max, run.mean):
        assert set(series) == set(run.channels)
        assert all(a.shape == (n,) for a in series.values())
    soc = "el-battery:sig_soc"
    assert np.all(run.min[soc][1:] <= run.mean[soc][1:] + 1e-12)
    assert np.all(run.mean[soc][1:] <= run.max[soc][1:] + 1e-12)
    # the engine's own names, in SI, beside today's
    assert len(run.engine_channels) > len(run.channels)
    assert all(a.shape == (n,) for a in run.engine_channels.values())
    assert "el_vehicle.v" in run.engine_channels or any(
        k.endswith(".v") for k in run.engine_channels)


def test_run_report_events_energy(bev_run):
    rep = bev_run.report
    assert rep["steps"] > 100 and rep["events"] == len(bev_run.events)
    assert "IDA" in rep["backend"] and rep["wall_seconds"] > 0
    assert isinstance(rep["notes"], list) and isinstance(rep["warnings"], list)
    assert rep["energy_closure"] < 1e-6
    for e in bev_run.events:
        assert isinstance(e["t"], float) and isinstance(e["label"], str)
    en = bev_run.energy
    assert en is not None and en["relative_closure"] < 1e-6
    assert en["supplied_J"] != 0 and len(en["parts"]) > 0
    assert len(en["parts"][0]["energy_in_t"]) == len(bev_run.times)


def test_output_step_and_t_end(bev):
    run = bev.simulate(t_end=100.0, output_step=5.0)
    assert run.times[-1] == pytest.approx(100.0) and len(run.times) == 21
    assert run.report["t_end"] == 100.0 and run.report["output_step"] == 5.0


def test_tighter_tolerance_changes_little(bev, bev_run):
    tight = bev.simulate(rtol=1e-8, atol=1e-10)
    a, b = figure(tight, "consumption_kwh_per_100km"), figure(bev_run, "consumption_kwh_per_100km")
    assert a != b and abs(a - b) < 0.01 * b


def _dump_case(tmp_path, project_path, case, python=None):
    cargo = shutil.which("cargo")
    if cargo is None:
        pytest.skip("cargo is not available")
    out = tmp_path / f"{case}.bin"
    cmd = [cargo, "run", "--release", "--locked", "-q", "-p", "lsim-project", "--example",
           "dump_case", "--", str(project_path), case, str(out)]
    if python:
        cmd.append(python)
    done = subprocess.run(cmd, cwd=ENGINE, capture_output=True, text=True, timeout=1800)
    assert done.returncode == 0, done.stderr[-2000:]
    raw = out.read_bytes()
    head, _, body = raw.partition(b"\n")
    head = json.loads(head)
    data = np.frombuffer(body, dtype="<f8")
    n = head["points"]
    assert len(data) == n * (1 + len(head["channels"]))
    channels = {name: data[n * (i + 1): n * (i + 2)] for i, name in enumerate(head["channels"])}
    return head, data[:n], channels


def _same_bits(a, b):
    a, b = np.ascontiguousarray(a, dtype="<f8"), np.ascontiguousarray(b, dtype="<f8")
    return a.shape == b.shape and np.array_equal(a.view("<u8"), b.view("<u8"))


def _assert_golden(run, head, times, channels):
    assert _same_bits(run.times, times)
    assert set(run.channels) == set(channels)
    differ = [k for k in channels if not _same_bits(run.channels[k], channels[k])]
    assert not differ, f"channels that differ from golden::run_case: {differ[:10]}"
    assert run.report["steps"] == head["steps"]
    mine = [(f["key"], f["label"], f["value"], f["unit"]) for f in run.figures]
    gold = [(f["key"], f["label"], f["value"], f["unit"]) for f in head["figures"]]
    assert mine == gold


def test_bev_wltc_is_bitwise_the_golden_path(bev, bev_run, tmp_path):
    head, times, channels = _dump_case(tmp_path, BACKEND / "projects" / "bev-car.json", WLTC)
    _assert_golden(bev_run, head, times, channels)


# ---- parameters ------------------------------------------------------------

def test_set_params_changes_the_result(bev_run):
    model = le.build(BEV, WLTC)
    base = model.params()[CD]
    model.set_params({CD: base * 1.25})
    assert model.params()[CD] == base * 1.25
    heavy = model.simulate()
    assert figure(heavy, "consumption_kwh_per_100km") > figure(
        bev_run, "consumption_kwh_per_100km") * 1.03
    # the model keeps what was set; setting it back gives the first run again
    model.set_params({CD: base})
    again = model.simulate()
    assert _same_bits(again.channels["el-battery:sig_soc"], bev_run.channels["el-battery:sig_soc"])


def test_set_params_errors_change_nothing():
    model = le.build(BEV, WLTC)
    before = model.params()
    with pytest.raises(ValueError, match="no parameter 'nope'"):
        model.set_params({CD: 0.3, "nope": 1.0})
    with pytest.raises(ValueError, match="changes the model's structure"):
        model.set_params({MASS: 1500.0})
    with pytest.raises(ValueError):
        model.set_params({CD: float("nan")})
    with pytest.raises(ValueError):
        model.set_params({CD: "heavy"})
    with pytest.raises(TypeError):
        model.set_params([1, 2])
    assert model.params() == before


# ---- sweep -----------------------------------------------------------------

def test_sweep(bev, bev_run):
    base = bev.params()[CD]
    sets = [{CD: base * 0.8}, {CD: base}, {CD: base * 1.3}, {"nope": 1.0}]
    runs = bev.sweep(sets, threads=3)
    assert len(runs) == 4
    assert isinstance(runs[3], ValueError) and "nope" in str(runs[3])
    cons = [figure(r, "consumption_kwh_per_100km") for r in runs[:3]]
    assert cons[0] < cons[1] < cons[2]
    # a set that changes nothing gives the plain run, bit for bit
    assert _same_bits(runs[1].channels["el-battery:sig_soc"],
                      bev_run.channels["el-battery:sig_soc"])
    # sweeps are in order and do not depend on the thread count
    single = bev.sweep(sets[:1], threads=1)[0]
    assert _same_bits(single.times, runs[0].times)
    assert _same_bits(single.channels["el-battery:sig_soc"], runs[0].channels["el-battery:sig_soc"])


def test_sweep_sits_on_set_params():
    model = le.build(BEV, WLTC)
    base = model.params()[CD]
    model.set_params({CD: base * 1.2})
    heavy, lighter = model.sweep([{}, {CD: base * 0.9}], t_end=300.0)
    assert figure(heavy, "consumption_kwh_per_100km") > figure(lighter, "consumption_kwh_per_100km")


def test_sweep_bad_arguments(bev):
    with pytest.raises(ValueError):
        bev.sweep([{}], threads=0)
    with pytest.raises(TypeError):
        bev.sweep([1])
    out = bev.sweep([{CD: "heavy"}])
    assert isinstance(out[0], ValueError)


# ---- errors ----------------------------------------------------------------

def test_bad_json():
    with pytest.raises(ValueError, match="not valid JSON"):
        le.build("{not json", WLTC)
    with pytest.raises(ValueError, match="not valid JSON"):
        le.diagnose("", WLTC)


def test_unknown_case():
    with pytest.raises(le.ModelError) as e:
        le.build(BEV, "case-nope")
    assert "case-nope" in str(e.value)
    d = e.value.diagnostics
    assert d[0]["code"] == "PROJECT-NO-CASE" and set(d[0]) >= {
        "code", "message", "parts", "hint", "detail"}


def _broken_project():
    project = json.loads(BEV)
    for system in project["systems"]:
        system["connections"] = [c for c in system["connections"] if c["id"] != "c-3"]
    return json.dumps(project)


def test_broken_model_gives_diagnostics():
    broken = _broken_project()
    with pytest.raises(le.ModelError) as e:
        le.build(broken, WLTC)
    d = e.value.diagnostics
    assert d and all(isinstance(x["code"], str) and x["message"] for x in d)
    assert any(x["parts"] for x in d)
    assert str(e.value) == "\n".join(x["message"] for x in d)
    # diagnose gives the same faults without building
    again = le.diagnose(broken, WLTC)
    assert [x["code"] for x in again] == [x["code"] for x in d]


def test_diagnose_clean_model():
    assert [d for d in le.diagnose(BEV, WLTC) if d["severity"] == "error"] == []


def test_bad_run_arguments(bev):
    for kw in ({"rtol": 0.0}, {"atol": -1.0}, {"output_step": 0.0}, {"t_end": -5.0},
               {"rtol": float("nan")}):
        with pytest.raises(ValueError, match="positive number"):
            bev.simulate(**kw)


def test_no_panics_for_plain_mistakes(bev):
    with pytest.raises(TypeError):
        bev.simulate(scripts=42)
    with pytest.raises(TypeError):
        bev.simulate(scripts={"x": "not callable"})
    with pytest.raises(TypeError):
        le.build(5, WLTC)


# ---- Script blocks ---------------------------------------------------------

def _compiled_hcu():
    """The hybrid car's Hybrid Control Unit as a callable f(t, dt, inputs), built
    from today's script runner (backend/app/solver/scripting.py)."""
    sys.path.insert(0, str(BACKEND))
    try:
        from app.solver.scripting import compile_script, run_script
    except Exception as e:  # pragma: no cover - the backend's requirements are missing
        pytest.skip(f"today's script runner is not importable: {e}")
    finally:
        sys.path.remove(str(BACKEND))
    code = next(
        el["parameterOverrides"]["code"]
        for s in json.loads(HYBRID)["systems"] for el in s["elements"] if el["id"] == "el-hcu")
    fn = compile_script(code, "Hybrid Control Unit")
    state: dict = {}
    calls = []

    def step(t, dt, inputs):
        calls.append(t)
        return run_script(fn, "Hybrid Control Unit", t, dt, inputs, state, {})

    step.calls = calls
    return step


def test_script_block_runs_a_python_callable():
    model = le.build(HYBRID, "case-mixed")
    blocks = model.report["case"]["sampled_blocks"]
    assert [b["kind"] for b in blocks] == ["script"] and blocks[0]["id"] == "el-hcu"
    step = _compiled_hcu()
    run = model.simulate(t_end=30.0, scripts={"el-hcu": step})
    # a call at the start, then every period
    period = blocks[0]["period"]
    assert step.calls[0] == 0.0 and step.calls[1] == pytest.approx(period)
    assert len(step.calls) == pytest.approx(30.0 / period + 1, abs=2)
    assert run.times[-1] == pytest.approx(30.0)
    assert np.all(np.isfinite(run.channels["el-vehicle:sig_speed"]))
    assert run.channels["el-vehicle:sig_speed"].max() > 10.0  # the car moves
    assert run.energy["relative_closure"] < 1e-6
    # the label works as the key too, and gives the same run
    step2 = _compiled_hcu()
    run2 = model.simulate(t_end=30.0, scripts={"Hybrid Control Unit": step2})
    assert _same_bits(run.channels["el-vehicle:sig_speed"], run2.channels["el-vehicle:sig_speed"])


def test_script_block_without_a_callable_is_a_plain_error():
    model = le.build(HYBRID, "case-mixed")
    with pytest.raises(ValueError, match="Hybrid Control Unit"):
        model.simulate(t_end=5.0)
    with pytest.raises(ValueError, match="el-hcu"):
        model.simulate(t_end=5.0, scripts={"other": lambda t, dt, i: None})


def test_script_that_fails_names_the_part_and_time():
    model = le.build(HYBRID, "case-mixed")

    def broken(t, dt, inputs):
        if t > 1.0:
            raise RuntimeError("boom")
        return {}

    with pytest.raises(le.ModelError) as e:
        model.simulate(t_end=5.0, scripts={"el-hcu": broken})
    assert "Hybrid Control Unit" in str(e.value) and "boom" in str(e.value)

    with pytest.raises(le.ModelError, match="dict"):
        model.simulate(t_end=5.0, scripts={"el-hcu": lambda t, dt, i: 5})
    with pytest.raises(le.ModelError, match="not a number"):
        # an output the block has, with a value that is no number
        out = next(iter(_outputs(model)))
        model.simulate(t_end=5.0, scripts={"el-hcu": lambda t, dt, i: {out: "x"}})


def _outputs(model):
    step = _compiled_hcu()
    seen = {}

    def spy(t, dt, inputs):
        r = step(t, dt, inputs)
        seen.update(r)
        return r

    model.simulate(t_end=1.0, scripts={"el-hcu": spy})
    return seen


def test_hybrid_mixed_cycle_is_bitwise_the_golden_path(tmp_path):
    """The whole mixed cycle with the callable against golden::run_case with
    today's script runner in a Python process."""
    head, times, channels = _dump_case(
        tmp_path, BACKEND / "projects" / "hybrid-car.json", "case-mixed", sys.executable)
    model = le.build(HYBRID, "case-mixed")
    run = model.simulate(scripts={"el-hcu": _compiled_hcu()})
    _assert_golden(run, head, times, channels)


# ---- the GIL ---------------------------------------------------------------

def test_the_gil_is_released_while_the_engine_runs(bev):
    count = [0]
    stop = threading.Event()

    def spin():
        while not stop.is_set():
            count[0] += 1

    thread = threading.Thread(target=spin)
    thread.start()
    try:
        time.sleep(0.05)
        before = count[0]
        run = bev.simulate()  # about two seconds
        during = count[0] - before
    finally:
        stop.set()
        thread.join()
    assert len(run.times) == 1801
    # the thread kept counting while the engine ran: with the GIL held it
    # would have counted nothing until the run ended
    assert during > 10_000, during


def test_the_gil_is_released_while_building_and_sweeping(bev):
    count = [0]
    stop = threading.Event()

    def spin():
        while not stop.is_set():
            count[0] += 1

    thread = threading.Thread(target=spin)
    thread.start()
    try:
        time.sleep(0.05)
        before = count[0]
        runs = bev.sweep([{}, {}, {}], threads=2, t_end=900.0)
        during = count[0] - before
    finally:
        stop.set()
        thread.join()
    assert all(not isinstance(r, Exception) for r in runs)
    assert during > 10_000, during


def test_runs_from_several_python_threads(bev):
    results = []

    def go():
        results.append(bev.simulate(t_end=200.0).channels["el-battery:sig_soc"][-1])

    threads = [threading.Thread(target=go) for _ in range(3)]
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    assert len(results) == 3 and len(set(results)) == 1
