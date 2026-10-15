# lsim-py: the engine in Python

This crate builds the Python module `lightsim_engine`. It lets Python import
one of today's project files, build it on the new engine, run it, change
parameters without rebuilding and run many parameter sets in parallel. The
app's backend and the `lightsim` package call it; the contract is in
`engine/DESIGN.md`, section 13.

```python
import lightsim_engine as le

project = open("backend/projects/bev-car.json").read()
model = le.build(project, "case-wltc")      # import, prepare, compile
print(model.report["sizes"], model.report["warnings"])

run = model.simulate()                      # rtol 1e-6, atol 1e-8, the case's own grid
run.times                                   # NumPy array, seconds
run.channels["el-battery:sig_soc"]          # today's channel names, today's units
run.figures                                 # today's summary figures
run.energy["relative_closure"]              # the energy books

model.set_params({"el_vehicle.cd": 0.30})   # engine names, SI values; no rebuild
runs = model.sweep([{"el_vehicle.cd": 0.25}, {"el_vehicle.cd": 0.35}], threads=4)
```

## What a call gives

* `build(project_json, case_id, cache_dir=None, cycles_dir=None)` returns a
  `Model`. With `cache_dir`, the prepared model is cached on disk (a second
  build of the same car, even with other parameter values, skips
  preparation). `cycles_dir` is the folder of the bundled drive cycles
  (`backend/app/cycles` of the repository is the default). The GIL is
  released while it works.
* `model.report`: sizes, import/prepare/compile seconds, whether the cache
  was hit, the structure key, `warnings` (a list of diagnostics) and the
  case (id, name, duration, the sampled blocks).
* `model.param_names()`, `model.params()` (current values, SI),
  `model.set_params({...})`: a bad name or value raises `ValueError` with
  the engine's own message and changes nothing. A parameter that changes
  the model's structure (the vehicle's mass, for one) cannot be set; build
  again with the changed project.
* `model.simulate(rtol=1e-6, atol=1e-8, output_step=None, t_end=None,
  scripts=None)` returns a `Run`. The GIL is released while the engine
  runs, so other Python threads go on. It is the golden comparison's run
  (`lsim_project::golden::run_case`): the same solver and tolerances, the
  energy books on, and an acceleration test runs again to the moment it
  crosses the finish. The results are bit for bit the same.
* `Run`: `times`, `channels`, `min`, `max`, `mean` (today's names and units,
  NumPy arrays; `min`/`max`/`mean` are per output interval),
  `engine_channels` (the engine's own names, SI), `events`, `report`,
  `energy` (the books, or `None`) and `figures`.
* `model.sweep(sets, threads=4, ...)` runs each set (on top of what
  `set_params` set) in parallel. The list it returns has a `Run` for each
  set that ran and the exception object for each that failed, in order.
* `le.diagnose(project_json, case_id)`: the structural and unit checks
  only, a list of diagnostics.
* `le.ModelError` (a `ValueError`): `.diagnostics` is a list of dicts with
  `severity`, `code`, `message`, `parts`, `hint` and `detail`; `str()` is
  the messages. It is raised for a project that cannot be imported or
  built, and for a run that fails (an integrator that gives up, an event
  storm, a Script that raises). A wrong argument is a plain `ValueError` or
  `TypeError`.

## Script blocks

A Script part is run by a Python function you give:
`scripts={"el-hcu": f}`, keyed by the part's id or its label, with
`f(t, dt, inputs: dict[str, float]) -> dict[str, float] | None`. It is
called once at the start and then every period; outputs it does not return
keep their last value. A Script part without a function makes `simulate`
raise a `ValueError` that names the part. FMU parts are not supported yet
(`ModelError`). A function given to a `sweep` is shared by all its runs, so
it should keep no state of its own.

`tests/test_api.py` builds such a function from today's script runner
(`backend/app/solver/scripting.py`), as the hybrid car's Hybrid Control Unit
needs.

## Build and test

You need Rust (the version in `engine/rust-toolchain.toml`), a C compiler,
Python 3.11 or newer and a virtual environment with maturin, NumPy and
pytest:

```
python -m venv .venv
.venv/bin/pip install maturin numpy pytest        # on Windows: .venv\Scripts\pip
export CARGO_INCREMENTAL=0                        # as for every engine build
.venv/bin/maturin develop --release -m engine/crates/lsim-py/Cargo.toml
.venv/bin/python -m pytest engine/crates/lsim-py/tests/test_api.py
```

`maturin develop` builds the module and installs it in the virtual
environment (use `--release`: the debug build is slow). To make a wheel
instead: `maturin build --release -m engine/crates/lsim-py/Cargo.toml --out wheels`
and `pip install wheels/lightsim_engine-*.whl`.

Without maturin: `cargo build --release -p lsim-py --features extension-module`
and copy `engine/target/release/liblightsim_engine.so` to a folder on
`PYTHONPATH` as `lightsim_engine.so` (on Windows `lightsim_engine.dll`
becomes `lightsim_engine.pyd`).

The tests need NumPy and, for the Script block tests, the backend's
requirements (`pip install -r backend/requirements.txt`). Two tests run the
golden comparison's own path through the Rust example
`engine/crates/lsim-project/examples/dump_case.rs` (they call `cargo`, and
are skipped when it is not installed) and compare the Python run with it bit
by bit: the BEV car's WLTC case, and the hybrid car's mixed cycle with the
Hybrid Control Unit.

The module's Rust has no tests of its own (`cargo test` has nothing to run
here); `engine/check.sh` still builds and lints it.
