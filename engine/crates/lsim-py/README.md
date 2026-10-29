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
  build of the same car with the same values skips preparation; other
  values are prepared again, since preparation chooses by them: set them
  on the built model instead, `set_params` or `sweep`). `cycles_dir` is the folder of the bundled drive cycles
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
  scripts=None, native_scripts=True)` returns a `Run`. The GIL is released
  while the engine runs, so other Python threads go on. It is the golden
  comparison's run (`lsim_project::golden::run_case`): the same solver and
  tolerances, the energy books on, and an acceleration test runs again to
  the moment it crosses the finish. The results are bit for bit the same.
  An output grid (`t_end / output_step`, the caller's or the case's own)
  of more than ten million points, or whose results would take more than
  4 GB (every recorded variable's value, min, max and mean at each output
  time), is refused with a `ValueError` before the run. A run whose results
  do not fit in the memory left fails with a `ModelError` coded
  `RUN-MEMORY`, and reading results into Python (`times`, `channels`, …,
  which copies them) without the memory for it raises `MemoryError`:
  neither ends the interpreter. `scripts` and `native_scripts` are below,
  under Script blocks.
* `Run`: `times`, `channels`, `min`, `max`, `mean` (today's names and units,
  NumPy arrays; `min`/`max`/`mean` are per output interval),
  `engine_channels` (the engine's own names, SI), `events`, `report`,
  `energy` (the books, or `None`) and `figures`; and what the app's
  verdict and reports are made from: `integrals` (each channel, in its
  display unit, and each engine variable, SI, over the run: `integral`,
  `square` and `magnitude` of the line through every point the integrator
  visited, `max`, `min`), `trace` (the channels or variables
  `simulate(trace=[...], trace_step=0.1)` asked for, sampled from the
  dense output every `trace_step` s and at the end, besides the output
  grid and without changing it; `None` without one), `windows` (for each
  name `simulate(windows={name: seconds})` asked for, its highest moving
  average over that span, `peak`, and when the span ended, `t`: of the same
  line through every point visited, read at each, nothing before the
  start; 0 s gives the highest value), `table_outside`
  (each table axis read past its data: when it first left, for how long,
  how far, its part) and `report["asserts"]` (each warning `assert` that
  did not hold). `model.report["instances"]` maps each element to its
  instance name (the energy books' parts' paths); `model.var_names()`
  lists the engine's variables and today's channels.
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

A Script part runs the Python function you give for it, whenever you give
one: `scripts={"el-hcu": f}`, keyed by the part's id or its label, with
`f(t, dt, inputs: dict[str, float]) -> dict[str, float] | None`. A key that
is neither (a misspelt id, `"el_hcu"`, the label with other capitals) makes
`simulate` and `sweep` raise a `ValueError` that names it and the case's
Script parts, and so do two keys that name one part (its id and its
label), so a function is never left out unsaid. It is
called once at the start and then every period; outputs it does not return
keep their last value. A function given to a `sweep` is shared by all its
runs, so it should keep no state of its own.

A part you give no function runs its own code (the project's Script) on
the engine's own runner of a subset of Python, `lsim-script`, when
`native_scripts=True`, the default. That runner gives the app's Script
runner's results bit for bit and reaches nothing outside the script (no
files, processes or network; a call has two seconds and 512 MB, as in the
app's worker, counting every block its values and its code hold). A script
it does not run (other imports, the
transcendental math functions, classes, … or an operation part-way through
the run that only Python has) makes `simulate` raise a `ModelError` coded
`RUN-SCRIPT` that names the part; give the part a function to run it on
Python. With `native_scripts=False` a part without a function makes
`simulate` raise a `ValueError` that names the part. `run.report["scripts"]`
says what ran each part: `"python"` (your function) or `"native"`. FMU
parts are not supported yet (`ModelError`).

`tests/test_api.py` builds such a function from the app's Script runner
(`backend/app/solver/sandbox_worker.py`'s `_run_step`, as the app calls
it), as the hybrid car's Hybrid Control Unit needs.

## Build and test

You need Rust (the version in `engine/rust-toolchain.toml`), a C compiler
(on Windows MSVC, and clang-cl for lsim-math's CORE-MATH: Visual Studio's
"C++ Clang tools for Windows", or LLVM), Python 3.11 or newer and a virtual environment with maturin, NumPy and
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
