"""Tests of the Python module `lightsim_engine` (the Python API, DESIGN.md §13).

Run them against the built module (see ../README.md):

    python -m pytest engine/crates/lsim-py/tests/test_api.py

The comparisons with the golden path run `golden::run_case` through the
Rust example `dump_case` (needs cargo; skipped without it) and compare the
two runs bit by bit.
"""
from __future__ import annotations

import json
import os
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


def test_build_table_cells(tmp_path):
    # which table reads at a state are held in their cells (DESIGN.md §8.2):
    # the BEV reads its tables at states as values only, which no option but
    # "kinks" holds, so "auto" (the default), "off" and "jumps" are the same
    # run, bit for bit
    plain = le.build(BEV, WLTC).simulate(t_end=60.0)
    for cells in ("auto", "off", "jumps"):
        run = le.build(BEV, WLTC, table_cells=cells).simulate(t_end=60.0)
        assert set(run.channels) == set(plain.channels)
        assert all(_same_bits(run.channels[k], plain.channels[k]) for k in plain.channels), cells
    held = le.build(BEV, WLTC, table_cells="kinks").simulate(t_end=60.0)
    assert not all(_same_bits(held.channels[k], plain.channels[k]) for k in plain.channels)
    with pytest.raises(ValueError, match="table_cells"):
        le.build(BEV, WLTC, table_cells="always")
    # the option is in the cache's key
    first = le.build(BEV, WLTC, cache_dir=str(tmp_path), table_cells="kinks")
    second = le.build(BEV, WLTC, cache_dir=str(tmp_path))
    assert first.report["cache_hit"] is False and second.report["cache_hit"] is False


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


def _dump_case(tmp_path, project_path, case, python=None, scripts="native"):
    cargo = shutil.which("cargo")
    if cargo is None:
        pytest.skip("cargo is not available")
    out = tmp_path / f"{case}.bin"
    cmd = [cargo, "run", "--release", "--locked", "-q", "-p", "lsim-project", "--example",
           "dump_case", "--", str(project_path), case, str(out)]
    if python:
        cmd.append(python)
    env = dict(os.environ, LSIM_SCRIPTS=scripts)
    done = subprocess.run(cmd, cwd=ENGINE, capture_output=True, text=True, timeout=1800, env=env)
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


def test_an_output_grid_too_large_is_refused():
    """More output points than a run records (1e7), from the caller's grid
    or the case's own, is a ValueError, not an abort of the interpreter
    (run in a process of its own: the abort would end the tests)."""
    code = (
        "import sys, lightsim_engine as le\n"
        "model = le.build(open(sys.argv[1], encoding='utf-8').read(), 'case-wltc')\n"
        "for kw in ({'output_step': 1e-7, 't_end': 5.0}, {'output_step': 1e-9},\n"
        "           {'t_end': 1e8}):\n"
        "    try:\n"
        "        model.simulate(**kw)\n"
        "        print('ran', kw)\n"
        "    except ValueError as e:\n"
        "        print('refused', kw, e)\n"
        "print(len(model.simulate(output_step=0.5, t_end=5.0).times))\n")
    done = subprocess.run([sys.executable, "-c", code, str(BACKEND / "projects" / "bev-car.json")],
                          capture_output=True, text=True, timeout=600)
    assert done.returncode == 0, done.stderr[-2000:]
    lines = done.stdout.splitlines()
    assert len(lines) == 4 and all(x.startswith("refused") for x in lines[:3]), done.stdout
    assert "output points" in lines[0] and lines[3] == "11"


def test_results_too_large_for_memory_never_abort_the_interpreter():
    """A grid whose results would take more than a run records (4 GB: every
    variable's value, min, max and mean at each output time) is a
    ValueError before the run, however few points it has; a run whose
    results do not fit fails with RUN-MEMORY, and results read into Python
    without the room for them raise MemoryError: nothing aborts the
    interpreter. Run in a process of its own whose memory is then capped a
    little above what it holds (`resource`), as a small Windows commit
    limit would be."""
    resource = pytest.importorskip("resource")
    del resource
    if not Path("/proc/self/statm").exists():
        pytest.skip("needs /proc to cap the memory a little above what is held")
    code = (
        "import resource, sys, lightsim_engine as le\n"
        "model = le.build(open(sys.argv[1], encoding='utf-8').read(), 'case-wltc')\n"
        "run = model.simulate(t_end=20.0, output_step=4e-4)\n"
        "held = int(open('/proc/self/statm').read().split()[0]) * resource.getpagesize()\n"
        "resource.setrlimit(resource.RLIMIT_AS, (held + 100_000_000, resource.RLIM_INFINITY))\n"
        "# a million points: under the point cap, past the bytes a run records\n"
        "try:\n"
        "    model.simulate(t_end=20.0, output_step=2e-5)\n"
        "    print('ran')\n"
        "except le.ModelError as e:\n"
        "    print('model', e.diagnostics[0]['code'])\n"
        "except ValueError as e:\n"
        "    print('refused', 'GB' in str(e))\n"
        "# 50 001 points: no room for their results now\n"
        "try:\n"
        "    model.simulate(t_end=20.0, output_step=4e-4)\n"
        "    print('ran')\n"
        "except le.ModelError as e:\n"
        "    print('model', e.diagnostics[0]['code'])\n"
        "except MemoryError:\n"
        "    print('memory')\n"
        "# and the first run's results read with too little room for them\n"
        "for name in ('times', 'channels', 'min', 'max', 'mean', 'engine_channels', 'energy'):\n"
        "    try:\n"
        "        getattr(run, name)\n"
        "        print('read', name)\n"
        "    except MemoryError:\n"
        "        print('memory', name)\n"
        "print('done')\n")
    done = subprocess.run([sys.executable, "-c", code, str(BACKEND / "projects" / "bev-car.json")],
                          capture_output=True, text=True, timeout=600)
    assert done.returncode == 0, (done.returncode, done.stdout, done.stderr[-2000:])
    lines = done.stdout.splitlines()
    assert lines[:2] in (["refused True", "model RUN-MEMORY"], ["refused True", "memory"]), done.stdout
    assert lines[-1] == "done" and len(lines) == 10, done.stdout
    assert all(x.split()[0] in ("read", "memory") for x in lines[2:-1]), done.stdout
    assert any(x.startswith("memory") for x in lines[2:-1]), done.stdout


def test_no_panics_for_plain_mistakes(bev):
    with pytest.raises(TypeError):
        bev.simulate(scripts=42)
    with pytest.raises(TypeError):
        bev.simulate(scripts={"x": "not callable"})
    with pytest.raises(TypeError):
        le.build(5, WLTC)


# ---- Script blocks ---------------------------------------------------------

def _compiled_hcu():
    """The hybrid car's Hybrid Control Unit as a callable f(t, dt, inputs), run
    as the app runs it: its Script worker's call of step()
    (backend/app/solver/sandbox_worker.py's `_run_step`, a fresh copy of the
    parameters each call) with the inputs as the engine passes them
    (`float(x or 0.0)`)."""
    sys.path.insert(0, str(BACKEND))
    try:
        from app.solver import sandbox_worker, scripting
    except Exception as e:  # pragma: no cover - the backend's requirements are missing
        pytest.skip(f"today's script runner is not importable: {e}")
    finally:
        sys.path.remove(str(BACKEND))
    el = next(el for s in json.loads(HYBRID)["systems"] for el in s["elements"]
              if el["id"] == "el-hcu")
    spec = {"label": "Hybrid Control Unit", "code": el["parameterOverrides"]["code"],
            "input_keys": [], "params": {}}
    script, err = sandbox_worker._compile(scripting, spec, False)
    assert script is not None, err
    calls = []

    def step(t, dt, inputs):
        calls.append(t)
        ins = {k: float(v or 0.0) for k, v in inputs.items()}
        return sandbox_worker._run_step(scripting, script, t, dt, ins)

    step.calls = calls
    return step


def _hybrid_with_hcu(code):
    """The hybrid car with other code in its Hybrid Control Unit."""
    project = json.loads(HYBRID)
    for s in project["systems"]:
        for el in s["elements"]:
            if el["id"] == "el-hcu":
                el["parameterOverrides"]["code"] = code
    return json.dumps(project)


def test_script_block_runs_a_python_callable():
    model = le.build(HYBRID, "case-mixed")
    blocks = model.report["case"]["sampled_blocks"]
    assert [b["kind"] for b in blocks] == ["script"] and blocks[0]["id"] == "el-hcu"
    step = _compiled_hcu()
    run = model.simulate(t_end=30.0, scripts={"el-hcu": step}, native_scripts=False)
    assert run.report["scripts"] == {"Hybrid Control Unit": "python"}
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
    run2 = model.simulate(t_end=30.0, scripts={"Hybrid Control Unit": step2}, native_scripts=False)
    assert _same_bits(run.channels["el-vehicle:sig_speed"], run2.channels["el-vehicle:sig_speed"])


def test_a_function_given_for_a_script_part_is_what_runs():
    """A function the caller gives is what runs, with native_scripts on
    (the default) too: a replacement controller is never set aside for
    the project's own code."""
    model = le.build(HYBRID, "case-mixed")
    calls = []

    def mine(t, dt, inputs):
        calls.append(t)
        return {}

    for kw in ({}, {"native_scripts": True}, {"native_scripts": False}):
        calls.clear()
        run = model.simulate(t_end=5.0, scripts={"el-hcu": mine}, **kw)
        assert run.report["scripts"] == {"Hybrid Control Unit": "python"}, kw
        assert calls and calls[0] == 0.0, kw


def test_a_script_part_without_a_function_runs_its_own_code_natively():
    """With native_scripts (the default), a part given no function runs its
    own code on the engine's runner; every channel is bitwise the run of
    the app's Script runner."""
    model = le.build(HYBRID, "case-mixed")
    native = model.simulate(t_end=60.0)
    assert native.report["scripts"] == {"Hybrid Control Unit": "native"}
    step = _compiled_hcu()
    python = model.simulate(t_end=60.0, scripts={"el-hcu": step})
    assert python.report["scripts"] == {"Hybrid Control Unit": "python"} and step.calls
    differ = [k for k in python.channels if not _same_bits(native.channels[k], python.channels[k])]
    assert not differ, differ[:10]


def test_a_script_part_the_native_runner_does_not_run_says_so():
    """A part without a function whose code is outside the native runner's
    subset, when it is read or part-way through the run, fails plainly,
    naming the part and what to do."""
    head = "import math\ndef step(t, dt, inputs, state, params):\n"
    for code in (head + "    return {'x': math.sin(t)}\n",
                 head + "    return {'x': math.copysign(1.0, float('nan')) if t > 1.0 else 0.0}\n"):
        model = le.build(_hybrid_with_hcu(code), "case-mixed")
        with pytest.raises(le.ModelError) as e:
            model.simulate(t_end=5.0)
        d = e.value.diagnostics[0]
        assert d["code"] == "RUN-SCRIPT" and d["parts"] == ["el-hcu"], d
        assert "Hybrid Control Unit" in d["message"], d
        assert "scripts={'el-hcu': f}" in d["hint"], d


def test_a_function_for_no_script_part_is_refused():
    """A key of `scripts` that is neither the id nor the label of a Script
    part of the case (a misspelt id, the engine's own naming of the part,
    a label with other capitals) raises a ValueError that names it and the
    parts there are, in simulate and sweep, with native_scripts=True (the
    default) too; so do two keys that name one part (its id and its
    label): a function given is never set aside unsaid."""
    model = le.build(HYBRID, "case-mixed")
    calls = []

    def mine(t, dt, inputs):
        calls.append(t)
        return {}

    for key in ("el_hcu", "Hybrid control unit", "HCU"):
        for scripts in ({key: mine}, {"el-hcu": mine, key: mine}):
            for call in (lambda: model.simulate(t_end=5.0, scripts=scripts),
                         lambda: model.sweep([{}], scripts=scripts)):
                with pytest.raises(ValueError) as e:
                    call()
                assert type(e.value) is ValueError, e.value
                text = str(e.value)
                assert f"'{key}'" in text and "'el-hcu'" in text and "Hybrid Control Unit" in text, text
    assert not calls

    def other(t, dt, inputs):
        calls.append(-t)
        return {}

    for scripts in ({"el-hcu": mine, "Hybrid Control Unit": other},
                    {"Hybrid Control Unit": other, "el-hcu": mine}):
        for call in (lambda: model.simulate(t_end=5.0, scripts=scripts),
                     lambda: model.sweep([{}], scripts=scripts)):
            with pytest.raises(ValueError) as e:
                call()
            assert type(e.value) is ValueError, e.value
            text = str(e.value)
            assert "'el-hcu'" in text and "'Hybrid Control Unit'" in text and "twice" in text, text
    assert not calls
    with pytest.raises(ValueError, match="no Script parts"):
        le.build(BEV, WLTC).simulate(t_end=5.0, scripts={"el-hcu": mine})


def test_script_block_without_a_callable_is_a_plain_error():
    model = le.build(HYBRID, "case-mixed")
    with pytest.raises(ValueError, match="Hybrid Control Unit"):
        model.simulate(t_end=5.0, native_scripts=False)
    with pytest.raises(ValueError, match="el-hcu"):
        model.simulate(t_end=5.0, scripts={"other": lambda t, dt, i: None}, native_scripts=False)


def test_script_that_fails_names_the_part_and_time():
    model = le.build(HYBRID, "case-mixed")

    def broken(t, dt, inputs):
        if t > 1.0:
            raise RuntimeError("boom")
        return {}

    with pytest.raises(le.ModelError) as e:
        model.simulate(t_end=5.0, scripts={"el-hcu": broken}, native_scripts=False)
    assert "Hybrid Control Unit" in str(e.value) and "boom" in str(e.value)

    with pytest.raises(le.ModelError, match="dict"):
        model.simulate(t_end=5.0, scripts={"el-hcu": lambda t, dt, i: 5}, native_scripts=False)
    with pytest.raises(le.ModelError, match="not a number"):
        # an output the block has, with a value that is no number
        out = next(iter(_outputs(model)))
        model.simulate(t_end=5.0, scripts={"el-hcu": lambda t, dt, i: {out: "x"}},
                       native_scripts=False)


def _outputs(model):
    step = _compiled_hcu()
    seen = {}

    def spy(t, dt, inputs):
        r = step(t, dt, inputs)
        seen.update(r)
        return r

    model.simulate(t_end=1.0, scripts={"el-hcu": spy}, native_scripts=False)
    return seen


def test_hybrid_mixed_cycle_is_bitwise_the_golden_path(tmp_path):
    """The whole mixed cycle with the callable against golden::run_case with
    the app's Script runner in a Python process, and on the native runner
    against golden::run_case on it."""
    project = BACKEND / "projects" / "hybrid-car.json"
    model = le.build(HYBRID, "case-mixed")
    head, times, channels = _dump_case(tmp_path, project, "case-mixed", sys.executable, "python")
    run = model.simulate(scripts={"el-hcu": _compiled_hcu()}, native_scripts=False)
    _assert_golden(run, head, times, channels)
    head, times, channels = _dump_case(tmp_path, project, "case-mixed", sys.executable, "native")
    run = model.simulate()
    assert run.report["scripts"] == {"Hybrid Control Unit": "native"}
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
