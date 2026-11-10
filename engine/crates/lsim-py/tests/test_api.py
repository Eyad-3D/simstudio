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


# ---- a run's control: stop, its points as it records them, live changes ----

def _start(model, control, **kw):
    """`model.simulate(control=control, **kw)` in a thread of its own; once
    the run has started."""
    out = {}

    def work():
        try:
            out["run"] = model.simulate(control=control, **kw)
        except BaseException as e:  # noqa: BLE001 — told by the test
            out["error"] = e

    th = threading.Thread(target=work)
    th.start()
    while not control.channels and th.is_alive():  # (set as the run starts)
        time.sleep(0.001)
    return th, out


def _drive(model, control, during=None, **kw):
    """The run with its points read as it goes; `during(control)` between
    reads."""
    th, out = _start(model, control, **kw)
    points = []
    while True:
        got = control.points(0.05)
        points += got
        if during:
            during(control)
        if not th.is_alive() and not got:
            break
    th.join()
    if "error" in out:
        raise out["error"]
    return out["run"], points


def _bits(a) -> bytes:
    return np.asarray(a, dtype=np.float64).tobytes()


def test_a_control_streams_every_point_of_the_result(bev, bev_run):
    """Every output point, as the run records it, through a queue the run
    waits on when it is full (8 points here): the result's, bit for bit."""
    c = le.RunControl(queue=8)
    run, points = _drive(bev, c)
    assert run.report["stopped"] is None and c.finished and not c.cancelled
    assert _bits(run.times) == _bits(bev_run.times)  # (a control changes nothing)
    assert all(_bits(v) == _bits(bev_run.channels[n]) for n, v in run.channels.items())
    assert len(points) == len(run.times)
    for k, (t, values) in enumerate(points):
        assert t == run.times[k]
        for name, v in zip(c.channels, values):
            assert _bits([v]) == _bits(run.channels[name][k:k + 1]), (name, k)
    assert set(c.channels) == set(run.channels)


def test_a_control_stops_the_run_where_it_is(bev):
    """A stop ends the run at the end of the step it takes: its points up
    to there and a last one there, the books closed; a control with no
    queue (the default) keeps no points and holds nothing up."""
    c = le.RunControl()
    th, out = _start(bev, c)
    while c.t < 500 and th.is_alive():
        time.sleep(0.001)
    asked = time.perf_counter()
    c.cancel()
    th.join()
    took = time.perf_counter() - asked
    run = out["run"]
    stopped = run.report["stopped"]
    assert 500 <= stopped < 600 and run.times[-1] == stopped, stopped
    assert run.times[-2] == np.floor(stopped) and len(run.times) < 1801
    assert all(len(v) == len(run.times) for v in run.channels.values())
    assert run.energy["relative_closure"] < 1e-8
    assert c.points(0.0) == [] and c.cancelled
    assert took < 0.5, took  # (stopped within a step; not the judged latency)


def test_the_run_goes_on_while_python_holds_its_lock(bev):
    """The engine never waits for the interpreter's lock while it runs: it
    goes on while this thread keeps the lock (a switch interval of 10 s and
    a busy loop)."""
    c = le.RunControl()
    th, _ = _start(bev, c)
    while c.t <= 0 and th.is_alive():
        time.sleep(0.001)
    before = sys.getswitchinterval()
    sys.setswitchinterval(10.0)
    try:
        t0, start = time.perf_counter(), c.t
        while time.perf_counter() - t0 < 0.3:
            pass
        moved = c.t - start
    finally:
        sys.setswitchinterval(before)
    th.join()
    assert moved > 0 or c.t >= 1800


def test_a_live_change_is_made_at_the_next_output_time_and_made_again_exactly(bev):
    """A live change (`RunControl.set_params`) is taken at the run's next
    step and made at the first output time after it, as an event; the run
    given it again (`simulate(edits=...)`, the time it was taken) is the
    same bit for bit. What `set_params` refuses: a structural parameter, a
    bound one given otherwise than its binding makes it, a control that
    serves no run, a control given to a second run."""
    asked = {}

    def change(c):
        if not asked and c.t >= 300:
            asked["at"] = c.t  # (before: the run goes on meanwhile)
            c.set_params({CD: 0.35})

    c = le.RunControl()
    run, _ = _drive(bev, c, change)
    (e,) = run.report["edits"]
    assert e["params"] == {CD: 0.35}
    assert asked["at"] <= e["t"] < e["at"] == np.floor(e["t"]) + 1.0  # (the case's 1 s grid)
    k = list(run.times).index(e["at"])
    assert any(ev["kind"] == "params" and ev["t"] == e["at"] for ev in run.events)
    again = bev.simulate(edits=[(e["t"], e["params"])])
    assert again.report["edits"] == run.report["edits"]
    for name, v in run.channels.items():
        assert _bits(v) == _bits(again.channels[name]), name
    assert _bits(run.times[:k]) == _bits(bev.simulate().times[:k])

    with pytest.raises(ValueError, match="serves no run"):
        le.RunControl().set_params({CD: 0.3})
    with pytest.raises(ValueError, match="one each"):
        bev.simulate(control=c)
    c = le.RunControl()
    th, _ = _start(bev, c)
    try:
        with pytest.raises(ValueError, match="structure"):
            c.set_params({MASS: 1500.0})
        mu = "el_wheel_fl.mu"
        with pytest.raises(ValueError, match="bound"):
            c.set_params({mu + "0": 0.5})
        c.set_params({mu: 0.9, mu + "0": 0.9})  # (as its binding makes it)
    finally:
        c.cancel()
        th.join()


def test_an_acceleration_test_streams_its_last_run_and_takes_no_change_in_it():
    """An acceleration test runs again to the moment it crosses the finish:
    a control is told the points of that last run only (the result's, bit
    for bit), which takes the changes its first run made, at their times,
    and no others. Its changes made again from the times its first run took
    them give the same run."""
    fs = le.build((BACKEND / "projects" / "fs-electric.json").read_text(encoding="utf-8"),
                  "case-accel-75m")
    refused = []

    def late(c):
        if not refused:
            try:
                c.set_params({"el_vehicle.cd": 1.0})
            except ValueError as e:
                refused.append(str(e))

    c = le.RunControl(queue=4)
    run, points = _drive(fs, c, late)
    assert run.report["reran_to_finish"]
    assert [p[0] for p in points] == list(run.times)
    for k, (_, values) in enumerate(points):
        for name, v in zip(c.channels, values):
            assert _bits([v]) == _bits(run.channels[name][k:k + 1]), (name, k)
    assert refused and "next run" in refused[0], refused

    changed = fs.simulate(edits=[(0.5, {"el_vehicle.cd": 1.0})])
    (e,) = changed.report["edits"]
    assert changed.report["reran_to_finish"] and e["at"] == 0.51
    assert changed.times[-1] != fs.simulate().times[-1]  # (another drag: another finish)
    again = fs.simulate(edits=[(e["t"], e["params"])])
    assert _bits(again.times) == _bits(changed.times)
    for name, v in changed.channels.items():
        assert _bits(v) == _bits(again.channels[name]), name


# ---- FMU parts --------------------------------------------------------------

def _fmu_project(h=0.1, wired=True, duration=1.0):
    """A project of an FMU block (its file is never read here: the caller's
    function is its host) and, when `wired`, a Constant 2.0 into its input;
    its Communication Step `h`."""
    sig = {"kind": "signal", "unitGroup": "No Unit"}
    pos = {"x": 0, "y": 0}
    fmu = {"id": "el-fmu", "componentDefId": "signal.fmu", "label": "Lag", "position": pos,
           "parameterOverrides": {"fmu_path": "lag.fmu", "sample_time_s": h},
           "dynamicPorts": [{"id": "fmu_u", "name": "u", "direction": "input", **sig},
                            {"id": "fmu_y", "name": "y", "direction": "output", **sig}]}
    elements = [fmu]
    links = []
    if wired:
        elements.append({"id": "el-src", "componentDefId": "signal.constant", "label": "Source",
                         "position": pos, "parameterOverrides": {"value": 2.0}})
        links.append({"id": "db-1", "element1Id": "el-src", "port1Id": "sig_out",
                      "element2Id": "el-fmu", "port2Id": "fmu_u"})
    return json.dumps({"id": "fmu", "name": "FMU", "systems": [
        {"id": "sys", "name": "Main", "elements": elements, "connections": []}],
        "dataBusConnections": links,
        "cases": [{"id": "case", "name": "Case", "duration": duration, "timeStep": h}]})


def test_an_fmu_part_runs_the_callers_function_at_its_communication_points():
    """An FMU part's host is the caller's function: called once at the
    start and at every multiple k*h of its Communication Step from 0 s (the
    times exactly k*h, as an FMU host's communication points), its inputs
    read there, its outputs held until the next call and recorded at an
    output time on a call as the call left them."""
    h = 0.1
    model = le.build(_fmu_project(h), "case")
    (block,) = model.report["case"]["sampled_blocks"]
    assert block["kind"] == "fmu" and block["id"] == "el-fmu" and block["period"] == h
    calls = []

    def lag(t, dt, inputs):
        calls.append((t, dt, dict(inputs)))
        return {"fmu_y": 10.0 * t + inputs["fmu_u"]}

    run = model.simulate(fmus={"el-fmu": lag})
    times = [c[0] for c in calls]
    assert times[0] == 0.0  # the start
    assert times[1:] == [k * h for k in range(len(times) - 1)], times
    assert times[-1] == pytest.approx(1.0)
    assert all(c[2] == {"fmu_u": 2.0} for c in calls), calls[:3]
    assert _bits(run.times) == _bits([k * h for k in range(11)])
    assert _bits(run.channels["el-fmu:fmu_y"]) == _bits([10.0 * k * h + 2.0 for k in range(11)])
    # the label is a key too
    again = le.build(_fmu_project(h), "case").simulate(fmus={"Lag": lag})
    assert _bits(again.channels["el-fmu:fmu_y"]) == _bits(run.channels["el-fmu:fmu_y"])


def test_an_fmu_part_without_a_function_or_with_a_wrong_key_is_refused():
    """The engine loads no FMU's code: an FMU part given no function fails
    the run with RUN-FMU naming the part, and a key of `fmus` that names no
    FMU part (or an FMU part's key under `scripts`) is a ValueError."""
    model = le.build(_fmu_project(), "case")
    with pytest.raises(le.ModelError) as e:
        model.simulate()
    (d,) = e.value.diagnostics
    assert d["code"] == "RUN-FMU" and d["parts"] == ["el-fmu"] and "fmus={'el-fmu': f}" in d["hint"]
    with pytest.raises(ValueError, match="names no FMU part") as e:
        model.simulate(fmus={"el-lag": lambda t, h, i: {}})
    assert "'el-fmu' (Lag)" in str(e.value)
    with pytest.raises(ValueError, match="no FMU parts"):
        le.build(BEV, WLTC).simulate(t_end=5.0, fmus={"el-fmu": lambda t, h, i: {}})
    with pytest.raises(TypeError, match="fmus"):
        model.simulate(fmus=42)
    with pytest.raises(ValueError, match="no Script parts"):
        model.simulate(scripts={"el-fmu": lambda t, h, i: {}})


# ---- fast mode ---------------------------------------------------------------

def test_fast_mode_runs_a_drive_cycle_case_with_todays_channels_and_figures(bev_run):
    """Fast mode on the BEV's WLTC: today's channels and figures, the
    cycle's speed followed (prescribed, so the Vehicle's speed is the
    cycle's at its samples), no flag, and its charge used within 1 % of the
    full run's (DESIGN.md section 10's band)."""
    run = le.fast(BEV, WLTC)
    fast = run.report["fast"]
    assert fast["step"] == 1.0 and fast["flags"] == [] and fast["steps"] > 1000
    assert run.times[0] == 0.0 and run.times[-1] == pytest.approx(1800.0)
    assert "el-battery:sig_soc" in run.channels and "el-vehicle:sig_speed" in run.channels
    keys = [f["key"] for f in run.figures]
    assert keys[:3] == [f["key"] for f in bev_run.figures][:3]

    def used(r):
        return 100.0 - figure(r, "el-battery.final_soc_pct")  # (the BEV starts full)
    start = bev_run.channels["el-battery:sig_soc"][0]
    assert run.channels["el-battery:sig_soc"][0] == start
    full, quick = start - figure(bev_run, "el-battery.final_soc_pct"), start - figure(run, "el-battery.final_soc_pct")
    assert abs(quick / full - 1.0) < 0.01, (quick, full)
    assert figure(run, "distance_km") == pytest.approx(figure(bev_run, "distance_km"), rel=0.01)


def test_fast_mode_refuses_what_it_cannot_run_with_its_codes():
    """An acceleration test has no cycle (FAST-NO-CYCLE); the hybrid's
    supervisory Script reads the pedal (FAST-SAMPLED-CONTROL); a sampled
    part elsewhere has no host in fast mode (FAST-SAMPLED); a bad step is a
    ValueError."""
    fs = (BACKEND / "projects" / "fs-electric.json").read_text(encoding="utf-8")
    with pytest.raises(le.ModelError) as e:
        le.fast(fs, "case-accel-75m")
    assert [d["code"] for d in e.value.diagnostics] == ["FAST-NO-CYCLE"]
    with pytest.raises(le.ModelError) as e:
        le.fast(HYBRID, "case-udds")
    assert "FAST-SAMPLED-CONTROL" in [d["code"] for d in e.value.diagnostics]
    p = json.loads(BEV)
    p["systems"][0]["elements"].append(
        {"id": "el-s", "componentDefId": "signal.script", "label": "Spare", "position": {"x": 0, "y": 0},
         "parameterOverrides": {"code": "pass", "sample_time_s": 0.1}, "dynamicPorts": []})
    with pytest.raises(le.ModelError) as e:
        le.fast(json.dumps(p), WLTC)
    assert [d["code"] for d in e.value.diagnostics] == ["FAST-SAMPLED"] and "Spare" in str(e.value)
    with pytest.raises(ValueError, match="step"):
        le.fast(BEV, WLTC, step=0.0)



@pytest.mark.parametrize("ambient_c", [65.0, -70.0])
def test_fast_mode_reports_the_tables_it_read_past_their_data(ambient_c):
    """The BEV's city cycle with its Ambient past the Climate Control's
    demand table (-20 to 40 C): fast mode's table_outside records, from its
    points, give the full run's: the table, its axis, how long, from when,
    how far and its edge. At 20 C it reads none."""
    p = json.loads(BEV)
    case = next(c for c in p["cases"] if c["id"] == "case-city")
    keys = ("table", "part", "axis", "total", "first", "furthest", "edge", "lo", "hi")

    def records(run):
        return [{k: d[k] for k in keys} for d in run.table_outside]
    case.setdefault("parameterOverrides", {}).setdefault("el-ambient", {})["temperature_C"] = ambient_c
    j = json.dumps(p)
    fast, full = records(le.fast(j, "case-city")), records(le.build(j, "case-city").simulate())
    assert [d["table"] for d in fast] == ["el_climate.demand_table"]
    assert fast == full
    case["parameterOverrides"]["el-ambient"]["temperature_C"] = 20.0
    assert le.fast(json.dumps(p), "case-city").table_outside == []


@pytest.mark.parametrize("axis", [0, 1])
def test_fast_mode_stops_at_a_table_axis_set_to_error(axis):
    """The BEV's E-Motor Power Loss map, both of whose axes are set to
    Error, cut to 4000 1/min (its first axis) or 50 N.m (its second) on the
    WLTC: a full run stops where the motor leaves the data; so does fast
    mode, FAST-TABLE-ERROR naming the table and the axis (before: it ran the
    whole cycle, its record a table read past its data). With the map whole
    neither stops."""
    p = json.loads(BEV)
    motor = next(e for e in p["systems"][0]["elements"] if e["id"] == "el-motor")
    loss = motor["parameterOverrides"]["power_loss"]
    motor["parameterOverrides"]["power_loss"] = (
        {k: v for k, v in loss.items() if float(k) <= 4000.0} if axis == 0
        else {k: {x: y for x, y in v.items() if float(x) <= 50.0} for k, v in loss.items()})
    j = json.dumps(p)
    with pytest.raises(le.ModelError) as e:
        le.fast(j, WLTC)
    assert [d["code"] for d in e.value.diagnostics] == ["FAST-TABLE-ERROR"]
    which = "first" if axis == 0 else "second"
    assert f"the table 'el_motor.power_loss' was read outside its data on its {which} axis" in str(e.value)
    with pytest.raises(le.ModelError, match="outside = Error"):
        le.build(j, WLTC).simulate()
    assert le.fast(BEV, WLTC).table_outside == []


@pytest.mark.parametrize("part,table,keep,edge", [
    ("el-motor", "power_loss", 1000.0, 1000.0 * np.pi / 30.0),
    ("el-battery", "ocv_table", 89.5, 0.9)])
def test_a_table_whose_data_stop_short_of_where_it_is_read_is_watched_there(part, table, keep, edge):
    """The E-Motor's Power Loss map from 1000 1/min (the motor stands at 0
    at the start), or the battery's OCV table for 90-100 % SOC (the city
    cycle starts at 90 % and goes below), set to Error: a full run and fast
    mode stop, as today's engine does, FAST-TABLE-ERROR in fast mode
    (before: the engine padded the table past its first point, so neither
    watched it there and both ran the whole cycle). With Clamp, fast mode
    and the full run record the same table read past that first point."""
    p = json.loads(BEV)
    el = next(e for e in p["systems"][0]["elements"] if e["id"] == part)
    lib = json.loads((BACKEND / "app" / "library" / "components.json").read_text(encoding="utf-8"))
    kind = next(c for c in lib["components"] if c["id"] == el["componentDefId"])
    data = el["parameterOverrides"].get(table) or next(
        x["default"] for x in kind["parameters"] if x["key"] == table)
    el["parameterOverrides"][table] = {k: v for k, v in data.items() if float(k) >= keep}
    axes = 2 if table == "power_loss" else 1
    for policy in ("error", "clamp"):
        el["tableOutside"] = {table: [policy] * axes}
        j = json.dumps(p)
        if policy == "error":
            with pytest.raises(le.ModelError) as e:
                le.fast(j, "case-city")
            assert [d["code"] for d in e.value.diagnostics] == ["FAST-TABLE-ERROR"]
            with pytest.raises(le.ModelError, match="outside = Error"):
                le.build(j, "case-city").simulate()
            continue
        fast = le.fast(j, "case-city").table_outside
        full = le.build(j, "case-city").simulate().table_outside
        # (one record per read of the table in the part's equations)
        assert {(d["table"], d["axis"]) for d in fast} == {(f"{part.replace('-', '_')}.{table}", 0)}
        assert all(d["edge"] == d["lo"] == pytest.approx(edge) and d["total"] > 0 for d in fast)
        assert {(d["table"], d["axis"], d["edge"]) for d in full} == {
            (d["table"], d["axis"], d["edge"]) for d in fast}


def test_fast_mode_counts_float_noise_at_an_edge_as_inside():
    """The BEV's Ambient at 40 °C + 1e-9, its climate demand table's last
    point being 40 °C and its axis set to Error: today's engine counts a
    point within 1e-9 of the larger end past an edge as inside, and runs
    the case; so does fast mode, with no record (before: FAST-TABLE-ERROR
    at t = 0)."""
    p = json.loads(BEV)
    els = {e["id"]: e for e in p["systems"][0]["elements"]}
    case = next(c for c in p["cases"] if c["id"] == "case-city")
    case.setdefault("parameterOverrides", {}).setdefault("el-ambient", {})["temperature_C"] = 40.0 + 1e-9
    els["el-climate"]["tableOutside"] = {"demand_table": ["error"]}
    assert le.fast(json.dumps(p), "case-city").table_outside == []
    case["parameterOverrides"]["el-ambient"]["temperature_C"] = 40.001  # (past it: stops)
    with pytest.raises(le.ModelError) as e:
        le.fast(json.dumps(p), "case-city")
    assert [d["code"] for d in e.value.diagnostics] == ["FAST-TABLE-ERROR"]


# ---- text components ---------------------------------------------------------

def test_a_part_built_from_its_text_gives_the_same_run(bev_run):
    """part_text prints the definition the import makes for a part; the
    part built from that text (build(texts=...)) gives the same run, every
    channel bit for bit."""
    text = le.part_text(BEV, WLTC, "el-battery")
    assert text.lstrip().startswith(("model", "block", "partial", "connector", "within", "//", "\""))
    run = le.build(BEV, WLTC, texts={"el-battery": text}).simulate()
    assert _bits(run.times) == _bits(bev_run.times)
    for name, v in bev_run.channels.items():
        assert _bits(run.channels[name]) == _bits(v), name


def test_a_changed_text_changes_only_its_own_part():
    """A text that changes the part's equations is that part's own: the
    run reads it (the drag coefficient's term doubled moves the result),
    and a definition another part shares is not touched."""
    text = le.part_text(BEV, WLTC, "el-vehicle")
    plain = le.build(BEV, WLTC).simulate(t_end=60.0)
    changed_text = text.replace("max(cd, 0)", "2 * max(cd, 0)", 1)  # (twice the drag)
    assert changed_text != text
    changed = le.build(BEV, WLTC, texts={"el-vehicle": changed_text}).simulate(t_end=60.0)
    assert _bits(changed.channels["el-battery:sig_soc"]) != _bits(plain.channels["el-battery:sig_soc"])
    # the four wheels share one definition: one wheel's changed text (twice
    # its rolling resistance) is not the four's
    wheels = ["el-wheel-fl", "el-wheel-fr", "el-wheel-rl", "el-wheel-rr"]
    wheel = le.part_text(BEV, WLTC, wheels[0])
    assert all(le.part_text(BEV, WLTC, w) == wheel for w in wheels)
    doubled = wheel.replace("f_rr = c_rr * N", "f_rr = 2 * c_rr * N", 1)
    assert doubled != wheel

    def soc(texts):
        return _bits(le.build(BEV, WLTC, texts=texts).simulate(t_end=60.0).channels["el-battery:sig_soc"])
    one, four = soc({wheels[0]: doubled}), soc({w: doubled for w in wheels})
    assert len({one, four, _bits(plain.channels["el-battery:sig_soc"])}) == 3


def test_a_text_that_does_not_fit_its_part_is_refused_with_its_code():
    text = le.part_text(BEV, WLTC, "el-battery")
    cases = {
        "TEXT-PARSE": ("el-battery", "model Broken\n  Real x\nend Broken;"),
        "TEXT-NO-PART": ("el-nothing", text),
    }
    for code, (el, t) in cases.items():
        with pytest.raises(le.ModelError) as e:
            le.build(BEV, WLTC, texts={el: t})
        assert [d["code"] for d in e.value.diagnostics] == [code], (code, str(e.value))
    # another part's text: its ports are not the battery's
    other = le.part_text(BEV, WLTC, "el-vehicle")
    with pytest.raises(le.ModelError) as e:
        le.build(BEV, WLTC, texts={"el-battery": other})
    assert [d["code"] for d in e.value.diagnostics] == ["TEXT-PORTS"]
    with pytest.raises(le.ModelError) as e:
        le.part_text(HYBRID, "case-udds", "el-hcu")
    assert [d["code"] for d in e.value.diagnostics] == ["TEXT-SAMPLED"]


def test_table_sites_lists_every_table_read_of_the_blocks():
    """le.table_sites() lists every table read in the library's blocks, in
    every configuration, once each: its component id, its table and the
    abscissa of each axis as the block's equations write it. A Fuel Cell
    Stack reads its curve at its current and at its Maximum Current (its
    most power), an E-Motor with a supply window its loss map at no torque
    too, a Lookup in the Mode 2D its table at both its inputs."""
    sites = le.table_sites()
    got = {(s["component"], s["table"], tuple(s["at"])) for s in sites}
    assert len(got) == len(sites)
    assert {("fuelcell.stack", "polarization", ("I",)),
            ("fuelcell.stack", "polarization", ("i_max",)),
            ("motor.emotor", "power_loss", ("wa", "0 * unit_N_m")),
            ("signal.lookup", "table", ("sig_x_in", "sig_y_in"))} <= got


@pytest.mark.parametrize("fast", [False, True])
def test_a_runs_table_reads_are_each_reads_abscissa(bev_run, fast):
    """run.table_reads gives each read of a table in the model, one per
    place a part reads it and per axis, with the abscissa it read at each
    output point, from the model itself: the BEV's E-Motor reads its drag
    table at its speed's magnitude and its loss map on both axes, its
    battery its OCV table at its SOC held to 0-1. Fast mode's alike."""
    run = le.fast(BEV, WLTC) if fast else bev_run
    reads = run.table_reads
    assert reads and all(r["values"].shape == run.times.shape for r in reads)
    ch = run.engine_channels
    drag = [r for r in reads if r["table"] == "el_motor.drag_torque"]
    assert [(r["part"], r["axis"]) for r in drag] == [("el-motor", 0)]
    np.testing.assert_array_equal(drag[0]["values"], ch["el_motor.wa"])
    ocv = [r for r in reads if r["table"] == "el_battery.ocv_table"]
    np.testing.assert_array_equal(ocv[0]["values"], ch["el_battery.soc_c"])
    assert {r["axis"] for r in reads if r["table"] == "el_motor.power_loss"} == {0, 1}
    assert not any(np.isnan(r["values"]).any() for r in reads)
