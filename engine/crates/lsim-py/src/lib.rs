//! # lsim-py: the Python module `lightsim_engine`
//!
//! Built into a wheel with maturin (`maturin build --release -m
//! engine/crates/lsim-py/Cargo.toml --features extension-module`); the
//! `lightsim` Python package and the app server wrap it. DESIGN.md §13
//! and this crate's README describe the API:
//!
//! ```python
//! import lightsim_engine as le
//! model = le.build(project_json, case_id, cache_dir=None)   # import + prepare (cached) + compile
//! model.report                                              # sizes, times, cache hit, warnings
//! model.set_params({"el-battery.Q": 756000.0})              # runtime, no rebuild
//! run = model.simulate(rtol=1e-6, atol=1e-8, output_step=None, t_end=None, scripts=None,
//!                      native_scripts=True)
//! run.times, run.channels["el-battery:sig_soc"]             # NumPy arrays, today's names
//! run.min / run.max / run.mean / run.events / run.report / run.energy / run.figures
//! runs = model.sweep([{…}, {…}], threads=4)                 # parallel, GIL released
//! le.diagnose(project_json, case_id)                        # structural and unit checks
//! ```
//!
//! A Script part runs the Python function the caller gives for it
//! (`scripts={part id or label: f}`), always; a key that names no Script
//! part, or a part named by two keys (its id and its label), is a
//! `ValueError`. A part given none runs its
//! own code on the engine's runner of a subset of Python (lsim-script:
//! bitwise the app's Script runner's results, reaching nothing outside
//! the script) when `native_scripts=True`, the default; a part whose code
//! that runner does not run fails the call with a `ModelError` saying so.
//! With `native_scripts=False` a part without a function is a
//! `ValueError`. `run.report["scripts"]` says which ran.
//!
//! The work itself is in [`core`] (no Python in it); this file is the
//! conversion to and from Python objects. The GIL is released while the
//! engine builds and runs; a Script block's callable takes it back for the
//! length of one call.

#![allow(missing_docs)] // the #[pyclass] and #[pymodule] expansions have undocumented items

pub mod core;

use crate::core::{Failure, RunOptions, RunOutput, Series};
use lsim_engine::spike::{CHANNELS, Exact, Params, battery_drive};
use lsim_engine::{BuildOptions, Engine};
use lsim_ir::runtime::DiscreteBlock;
use lsim_ir::{Diagnostic, Severity};
use lsim_project::golden::Figure;
use lsim_project::import::{SampledKind, SampledSpec};
use lsim_solve::{EnergyBooks, OutputGrid, SolverOptions};
use numpy::PyArray1;
use pyo3::exceptions::{PyMemoryError, PyRuntimeError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::{PyDict, PyList};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pyo3::create_exception!(
    lightsim_engine,
    ModelError,
    PyValueError,
    "A model that cannot be built or run. `.diagnostics` is a list of dicts \
     (code, message, parts, hint, detail); `str()` is the plain messages."
);

// ---- errors and diagnostics ------------------------------------------------

fn diag_dict<'py>(py: Python<'py>, d: &Diagnostic) -> PyResult<Bound<'py, PyDict>> {
    let out = PyDict::new(py);
    out.set_item(
        "severity",
        match d.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
        },
    )?;
    out.set_item("code", &d.code)?;
    out.set_item("message", &d.message)?;
    out.set_item("parts", &d.parts)?;
    out.set_item("hint", d.hint.as_deref())?;
    out.set_item("detail", &d.detail)?;
    Ok(out)
}

fn diag_list<'py>(py: Python<'py>, ds: &[Diagnostic]) -> PyResult<Bound<'py, PyList>> {
    let items = ds.iter().map(|d| diag_dict(py, d)).collect::<PyResult<Vec<_>>>()?;
    PyList::new(py, items)
}

fn to_err(py: Python<'_>, f: Failure) -> PyErr {
    match f {
        Failure::Value(m) => PyValueError::new_err(m),
        Failure::Model(ds) => {
            let message = ds.iter().map(|d| d.message.as_str()).collect::<Vec<_>>().join("\n");
            let err = ModelError::new_err(message);
            if let Ok(list) = diag_list(py, &ds) {
                let _ = err.value(py).setattr("diagnostics", list);
            }
            err
        }
    }
}

// ---- Script blocks ---------------------------------------------------------

/// The Python callables of a run's Script blocks, by part id or label.
type Scripts = HashMap<String, Py<PyAny>>;

fn scripts_of(scripts: Option<&Bound<'_, PyAny>>) -> PyResult<Scripts> {
    let mut out = Scripts::new();
    let Some(scripts) = scripts.filter(|s| !s.is_none()) else { return Ok(out) };
    let dict = scripts.cast::<PyDict>().map_err(|_| {
        PyTypeError::new_err("scripts must be a dict of part id (or label) → function")
    })?;
    for (k, v) in dict.iter() {
        let key: String =
            k.extract().map_err(|_| PyTypeError::new_err("scripts: keys must be text"))?;
        if !v.is_callable() {
            return Err(PyTypeError::new_err(format!("scripts['{key}'] is not callable")));
        }
        out.insert(key, v.unbind());
    }
    Ok(out)
}

/// A Script block run by a Python callable `f(t, dt, inputs) -> dict | None`
/// every period (and once at the start), its inputs and outputs in today's
/// display numbers (the importer's conversions see to that); outputs the
/// callable does not return hold their last value. The block runs as today's
/// does: a first call at the start, then every period.
struct PyScriptHost {
    name: String,
    period: f64,
    inputs: Vec<String>,
    outputs: Vec<String>,
    f: Py<PyAny>,
}

impl PyScriptHost {
    fn call(&mut self, t: f64, inputs: &[f64], outputs: &mut [f64]) -> Result<(), String> {
        Python::attach(|py| {
            let fail = |e: PyErr| format!("Script '{}' failed at t = {t} s: {e}", self.name);
            let ins = PyDict::new(py);
            for (k, v) in self.inputs.iter().zip(inputs) {
                ins.set_item(k, *v).map_err(fail)?;
            }
            let ret = self.f.bind(py).call1((t, self.period, ins)).map_err(fail)?;
            if ret.is_none() {
                return Ok(());
            }
            let ret = ret.cast::<PyDict>().map_err(|_| {
                format!(
                    "Script '{}' must return a dict of output values (or None), got {} at t = {t} s.",
                    self.name,
                    ret.get_type().name().map(|n| n.to_string()).unwrap_or_default()
                )
            })?;
            for (k, o) in self.outputs.iter().zip(outputs.iter_mut()) {
                if let Some(v) = ret.get_item(k).map_err(fail)? {
                    *o = v.extract::<f64>().map_err(|_| {
                        format!(
                            "Script '{}' output '{k}' is not a number at t = {t} s ({}).",
                            self.name,
                            v.repr().map(|r| r.to_string()).unwrap_or_default()
                        )
                    })?;
                }
            }
            Ok(())
        })
    }
}

impl DiscreteBlock for PyScriptHost {
    fn name(&self) -> &str {
        &self.name
    }
    fn period(&self) -> f64 {
        self.period
    }
    // today runs a Script at t = 0 (the start) and then every period
    fn offset(&self) -> f64 {
        self.period
    }
    fn init(&mut self, t0: f64, inputs: &[f64], outputs: &mut [f64]) -> Result<(), String> {
        self.call(t0, inputs, outputs)
    }
    fn tick(&mut self, t: f64, inputs: &[f64], outputs: &mut [f64]) -> Result<(), String> {
        self.call(t, inputs, outputs)
    }
}

/// The host of a Script block: the one place it is chosen. The caller's
/// Python function for the part (`scripts`, by the part's id or label) is
/// what runs, when it gives one. A part given none runs its own code on
/// lsim-script's runner of a subset of Python with `native` (the caller's
/// choice: that runner reaches nothing outside the script; its results
/// are bitwise the app's Script runner's), and fails plainly when its code
/// is outside that subset; without `native` it is a plain error.
fn script_host(
    spec: &SampledSpec,
    scripts: &Scripts,
    native: bool,
) -> Result<core::ScriptHost, Failure> {
    let Some(f) = scripts.get(&spec.element).or_else(|| scripts.get(&spec.label)) else {
        if native && let SampledKind::Script { code } = &spec.kind {
            return match lsim_script::ScriptBlock::new(
                code,
                &spec.label,
                spec.period,
                spec.inputs.clone(),
                spec.outputs.clone(),
                &spec.params,
            ) {
                Ok(block) => {
                    let flag = block.outside_flag();
                    Ok((Box::new(block), "native", Some(flag)))
                }
                Err(lsim_script::ScriptError::Unsupported(m)) => {
                    Err(core::script_outside(&spec.element, m))
                }
                Err(lsim_script::ScriptError::Failed(m)) => {
                    Err(core::script_failed(&spec.element, m))
                }
            };
        }
        return Err(Failure::Value(format!(
            "The Script part '{}' (id '{}') needs a Python function: pass \
             scripts={{'{}': f}} to simulate (f(t, dt, inputs) returns a dict of outputs or None), \
             or native_scripts=True to run its own code on the engine's runner.",
            spec.label, spec.element, spec.element
        )));
    };
    let host = PyScriptHost {
        name: spec.label.clone(),
        period: spec.period,
        inputs: spec.inputs.clone(),
        outputs: spec.outputs.clone(),
        f: Python::attach(|py| f.clone_ref(py)),
    };
    Ok((Box::new(host), "python", None))
}

fn host_factory(
    scripts: Scripts,
) -> impl Fn(&core::Built, bool) -> Result<core::Hosts, Failure> + Sync {
    move |built, native| core::block_hosts(built, &|spec, n| script_host(spec, &scripts, n), native)
}

// ---- results ---------------------------------------------------------------

fn array<'py>(py: Python<'py>, v: Vec<f64>) -> Bound<'py, PyAny> {
    PyArray1::from_vec(py, v).into_any()
}

/// A copy of a run's series as a NumPy array: a `MemoryError` when the
/// memory left does not hold it.
fn copy<'py>(
    py: Python<'py>,
    v: &[f64],
    what: impl FnOnce() -> String,
) -> PyResult<Bound<'py, PyAny>> {
    Ok(array(py, core::copied(v, |x| x, what).map_err(no_room)?))
}

fn no_room(e: core::NoRoom) -> PyErr {
    PyMemoryError::new_err(e.0)
}

fn arrays<'py>(py: Python<'py>, items: Vec<(String, Vec<f64>)>) -> PyResult<Bound<'py, PyDict>> {
    let out = PyDict::new(py);
    for (k, v) in items {
        out.set_item(k, array(py, v))?;
    }
    Ok(out)
}

fn energy_dict<'py>(py: Python<'py>, e: &EnergyBooks) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("supplied_J", e.supplied)?;
    d.set_item("lost_J", e.lost)?;
    d.set_item("event_loss_J", e.event_loss)?;
    d.set_item("impulse_loss_J", e.impulse_loss)?;
    d.set_item("impulse_link_loss_J", e.impulse_link_loss)?;
    d.set_item("stored_integral_J", e.stored_integral)?;
    d.set_item("stored_change_J", e.stored_change)?;
    d.set_item("throughput_J", e.throughput)?;
    d.set_item("closure_J", e.closure)?;
    d.set_item("relative_closure", e.relative_closure)?;
    d.set_item("drift_J", e.drift)?;
    d.set_item("relative_drift", e.relative_drift)?;
    d.set_item("error_controlled", e.error_controlled)?;
    d.set_item("summary", e.summary())?;
    let parts = PyList::empty(py);
    for p in &e.parts {
        let q = PyDict::new(py);
        q.set_item("path", &p.path)?;
        q.set_item("name", &p.name)?;
        q.set_item("declared", p.declared)?;
        q.set_item("energy_in_J", p.energy_in)?;
        q.set_item("throughput_J", p.throughput)?;
        q.set_item("lost_J", p.lost)?;
        q.set_item("stored_change_J", p.stored_change)?;
        q.set_item("event_change_J", p.event_change)?;
        q.set_item("stored_integral_J", p.stored_integral)?;
        q.set_item("impulse_lost_J", p.impulse_lost)?;
        q.set_item("closure_J", p.closure)?;
        q.set_item("drift_J", p.drift)?;
        let series = |what: &str| format!("{}'s {what}", p.path);
        q.set_item("energy_in_t", copy(py, &p.energy_in_t, || series("energy in"))?)?;
        q.set_item("lost_t", copy(py, &p.lost_t, || series("losses"))?)?;
        q.set_item("stored_t", copy(py, &p.stored_t, || series("stored energy"))?)?;
        parts.append(q)?;
    }
    d.set_item("parts", parts)?;
    Ok(d)
}

/// A finished run.
#[pyclass(module = "lightsim_engine", frozen)]
struct Run {
    built: Arc<core::Built>,
    out: Arc<RunOutput>,
    times: PyOnceLock<Py<PyAny>>,
    channels: PyOnceLock<Py<PyDict>>,
    engine_channels: PyOnceLock<Py<PyDict>>,
    min: PyOnceLock<Py<PyDict>>,
    max: PyOnceLock<Py<PyDict>>,
    mean: PyOnceLock<Py<PyDict>>,
}

impl Run {
    fn new(built: Arc<core::Built>, out: RunOutput) -> Run {
        Run {
            built,
            out: Arc::new(out),
            times: PyOnceLock::new(),
            channels: PyOnceLock::new(),
            engine_channels: PyOnceLock::new(),
            min: PyOnceLock::new(),
            max: PyOnceLock::new(),
            mean: PyOnceLock::new(),
        }
    }

    fn series(
        &self,
        py: Python<'_>,
        cell: &PyOnceLock<Py<PyDict>>,
        which: Series,
    ) -> PyResult<Py<PyDict>> {
        if let Some(d) = cell.get(py) {
            return Ok(d.clone_ref(py));
        }
        let channels =
            core::today_channels(&self.built, &self.out.result, which).map_err(no_room)?;
        let d = arrays(py, channels)?.unbind();
        // another thread may have filled it meanwhile: keep that one
        let _ = cell.set(py, d.clone_ref(py));
        Ok(cell.get(py).map_or(d, |d| d.clone_ref(py)))
    }
}

#[pymethods]
impl Run {
    /// The output times, s (NumPy float64 array).
    #[getter]
    fn times(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        if let Some(t) = self.times.get(py) {
            return Ok(t.clone_ref(py));
        }
        let t = copy(py, &self.out.result.times, || "the output times".into())?.unbind();
        let _ = self.times.set(py, t.clone_ref(py));
        Ok(self.times.get(py).map_or(t, |t| t.clone_ref(py)))
    }

    /// Today's channels (`element:port`) in today's display units, as
    /// NumPy arrays over `times`.
    #[getter]
    fn channels(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        self.series(py, &self.channels, Series::Values)
    }

    /// The engine's own channels (full variable names, SI).
    #[getter]
    fn engine_channels(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        if let Some(d) = self.engine_channels.get(py) {
            return Ok(d.clone_ref(py));
        }
        let r = &self.out.result;
        let d = PyDict::new(py);
        for (name, v) in r.names.iter().zip(&r.values) {
            d.set_item(name, copy(py, v, || format!("the variable {name}"))?)?;
        }
        let d = d.unbind();
        let _ = self.engine_channels.set(py, d.clone_ref(py));
        Ok(self.engine_channels.get(py).map_or(d, |d| d.clone_ref(py)))
    }

    /// Lowest value of each channel over the output interval ending at
    /// each time (like `channels`).
    #[getter]
    fn min(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        self.series(py, &self.min, Series::Min)
    }

    /// Highest value over each output interval.
    #[getter]
    fn max(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        self.series(py, &self.max, Series::Max)
    }

    /// Time-mean over each output interval.
    #[getter]
    fn mean(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        self.series(py, &self.mean, Series::Mean)
    }

    /// The events, in order: dicts with `t`, `label` and `kind`.
    #[getter]
    fn events<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        let out = PyList::empty(py);
        for e in &self.out.result.events {
            let d = PyDict::new(py);
            d.set_item("t", e.t)?;
            d.set_item("label", &e.label)?;
            d.set_item(
                "kind",
                match e.kind {
                    lsim_solve::EventKind::When(_) => "when",
                    lsim_solve::EventKind::Mode(_) => "mode",
                    lsim_solve::EventKind::Block(_) => "block",
                    lsim_solve::EventKind::Time(_) => "time",
                    lsim_solve::EventKind::States(_) => "states",
                    lsim_solve::EventKind::Cell(_) => "cell",
                },
            )?;
            out.append(d)?;
        }
        Ok(out)
    }

    /// What the run did: steps, events, backend, wall seconds, notes,
    /// warnings, the integrator's error estimate and the energy closure.
    #[getter]
    fn report<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let (o, r) = (&self.out, &self.out.result);
        let s = &r.report;
        let d = PyDict::new(py);
        d.set_item("steps", r.stats.steps)?;
        d.set_item("events", r.events.len())?;
        d.set_item("backend", r.backend)?;
        d.set_item("method", &s.method)?;
        d.set_item("wall_seconds", o.wall_seconds)?;
        d.set_item("rtol", s.rtol)?;
        d.set_item("atol", s.atol)?;
        d.set_item("t_end", o.t_end)?;
        d.set_item("output_step", o.output_step)?;
        d.set_item("reran_to_finish", o.reran)?;
        let scripts = PyDict::new(py);
        for (label, runner) in &o.scripts {
            scripts.set_item(label, *runner)?;
        }
        d.set_item("scripts", scripts)?;
        d.set_item("rhs_evals", r.stats.rhs_evals)?;
        d.set_item("jac_evals", r.stats.jac_evals)?;
        d.set_item("err_test_fails", r.stats.err_test_fails)?;
        d.set_item("restarts", r.stats.restarts)?;
        d.set_item("block_ticks", s.block_ticks)?;
        d.set_item("impulses", s.impulses)?;
        d.set_item("notes", &s.notes)?;
        d.set_item("warnings", &s.warnings)?;
        let acc = PyDict::new(py);
        acc.set_item("worst_local", s.error.worst_local)?;
        acc.set_item("worst_local_var", &s.error.worst_local_var)?;
        d.set_item("accuracy", acc)?;
        d.set_item("energy_closure", r.energy.as_ref().map(|e| e.relative_closure))?;
        Ok(d)
    }

    /// The energy books (see DESIGN.md), or None when the run kept none.
    #[getter]
    fn energy<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyDict>>> {
        self.out.result.energy.as_ref().map(|e| energy_dict(py, e)).transpose()
    }

    /// Today's summary figures: dicts with `key`, `label`, `value`, `unit`.
    #[getter]
    fn figures<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        let out = PyList::empty(py);
        for Figure { key, label, value, unit } in &self.out.figures {
            let d = PyDict::new(py);
            d.set_item("key", key)?;
            d.set_item("label", label)?;
            d.set_item("value", *value)?;
            d.set_item("unit", unit)?;
            out.append(d)?;
        }
        Ok(out)
    }

    fn __repr__(&self) -> String {
        let r = &self.out.result;
        format!(
            "<lightsim_engine.Run {} points to t = {} s, {} steps>",
            r.times.len(),
            r.times.last().copied().unwrap_or(0.0),
            r.stats.steps
        )
    }
}

// ---- the model -------------------------------------------------------------

/// The parameter values a model runs with: what `set_params` set (kept by
/// name so that the model's own base values stay the base) and the whole
/// vector they give.
struct ParamState {
    set: BTreeMap<String, f64>,
    values: Vec<f64>,
}

/// A case built and ready to run.
#[pyclass(module = "lightsim_engine", frozen)]
struct Model {
    built: Arc<core::Built>,
    params: Mutex<ParamState>,
}

impl Model {
    fn state(&self) -> std::sync::MutexGuard<'_, ParamState> {
        self.params.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn run_options(
    rtol: f64,
    atol: f64,
    output_step: Option<f64>,
    t_end: Option<f64>,
    native_scripts: bool,
) -> RunOptions {
    RunOptions { rtol, atol, output_step, t_end, native_scripts }
}

#[pymethods]
impl Model {
    /// How it was built: sizes, prepare and compile seconds, cache hit,
    /// structure key, warnings (a list of diagnostic dicts), the case.
    #[getter]
    fn report<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let b = &self.built;
        let r = b.build_report();
        let d = PyDict::new(py);
        let sizes = PyDict::new(py);
        sizes.set_item("states", r.sizes.0)?;
        sizes.set_item("iteration_variables", r.sizes.1)?;
        sizes.set_item("assignments", r.sizes.2)?;
        sizes.set_item("aliases", r.sizes.3)?;
        sizes.set_item("flat_variables", r.flat.0)?;
        sizes.set_item("flat_equations", r.flat.1)?;
        sizes.set_item("parameters", b.model.info.params.len())?;
        sizes.set_item("code_bytes", r.code_bytes)?;
        d.set_item("sizes", sizes)?;
        d.set_item("import_seconds", b.import_seconds)?;
        d.set_item("prepare_seconds", r.prepare_seconds)?;
        d.set_item("compile_seconds", r.compile_seconds)?;
        d.set_item("build_seconds", r.build_seconds + b.import_seconds)?;
        d.set_item("cache_hit", r.cache_hit)?;
        d.set_item("cache_key", &r.cache_key)?;
        d.set_item("structure_key", &r.structure_key)?;
        d.set_item("warnings", diag_list(py, &b.warnings())?)?;
        let run = &b.rep.run;
        let case = PyDict::new(py);
        case.set_item("id", &b.case_id)?;
        case.set_item("name", &run.name)?;
        case.set_item("kind", format!("{:?}", run.kind).to_lowercase())?;
        case.set_item("duration", run.duration)?;
        case.set_item("output_step", run.output_dt())?;
        let blocks = PyList::empty(py);
        for s in &b.rep.sampled {
            let x = PyDict::new(py);
            x.set_item("id", &s.element)?;
            x.set_item("label", &s.label)?;
            x.set_item(
                "kind",
                match s.kind {
                    lsim_project::import::SampledKind::Script { .. } => "script",
                    lsim_project::import::SampledKind::Fmu { .. } => "fmu",
                    lsim_project::import::SampledKind::TractionControl => "traction_control",
                },
            )?;
            x.set_item("period", s.period)?;
            blocks.append(x)?;
        }
        case.set_item("sampled_blocks", blocks)?;
        d.set_item("case", case)?;
        Ok(d)
    }

    /// The engine names of the parameters.
    fn param_names(&self) -> Vec<String> {
        self.built.model.param_names().into_iter().map(String::from).collect()
    }

    /// The parameters' current values (SI) by engine name: the model's
    /// own, with what `set_params` set.
    fn params<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let values = self.state().values.clone();
        let out = PyDict::new(py);
        for (name, v) in self.built.model.param_names().into_iter().zip(values) {
            out.set_item(name, v)?;
        }
        Ok(out)
    }

    /// Sets parameters by their engine names (SI values) for the runs that
    /// follow; no rebuild. All or none: a bad name or value raises
    /// ValueError with the engine's message and changes nothing.
    fn set_params(&self, values: &Bound<'_, PyAny>) -> PyResult<()> {
        let changes = changes_of(values)?.map_err(PyValueError::new_err)?;
        let mut st = self.state();
        let mut set = st.set.clone();
        set.extend(changes);
        let list: Vec<(String, f64)> = set.iter().map(|(k, v)| (k.clone(), *v)).collect();
        let values = self.built.model.params_with(&list).map_err(PyValueError::new_err)?;
        *st = ParamState { set, values };
        Ok(())
    }

    /// Runs the case and returns a Run. The GIL is released while the
    /// engine runs. `scripts` maps a Script part's id (or label) to a
    /// function `f(t, dt, inputs: dict) -> dict | None`, which is what runs
    /// for it; with `native_scripts` (the default) a part given none runs
    /// its own code on the engine's runner of a subset of Python, and fails
    /// plainly when the code is outside it. `run.report["scripts"]` says
    /// which ran; a key of `scripts` that names no Script part, or a part
    /// named by two keys (its id and its label), is a ValueError. A grid of
    /// more than 1e7 output points, or whose results
    /// would take more than 4 GB, is refused; results read without the
    /// memory left for them raise MemoryError.
    #[pyo3(signature = (rtol=1e-6, atol=1e-8, output_step=None, t_end=None, scripts=None, native_scripts=true))]
    #[allow(clippy::too_many_arguments)]
    fn simulate(
        &self,
        py: Python<'_>,
        rtol: f64,
        atol: f64,
        output_step: Option<f64>,
        t_end: Option<f64>,
        scripts: Option<&Bound<'_, PyAny>>,
        native_scripts: bool,
    ) -> PyResult<Run> {
        let scripts = scripts_of(scripts)?;
        core::check_script_keys(&self.built, scripts.keys()).map_err(|f| to_err(py, f))?;
        let factory = host_factory(scripts);
        let params = self.state().values.clone();
        let built = Arc::clone(&self.built);
        let opts = run_options(rtol, atol, output_step, t_end, native_scripts);
        let out =
            py.detach(|| core::run(&built, &params, &opts, &factory)).map_err(|f| to_err(py, f))?;
        Ok(Run::new(built, out))
    }

    /// Runs the case for several parameter sets (dicts of engine names →
    /// SI values, on top of what `set_params` set), in parallel on up to
    /// `threads` threads with the GIL released. Returns a list with a Run,
    /// or the exception it would have raised, for each set. Script parts
    /// run as in `simulate` (a key of `scripts` that names no Script part,
    /// or a part named twice, is a ValueError); a Script function is shared
    /// by all the runs: give
    /// a sweep stateless functions.
    #[pyo3(signature = (sets, threads=4, rtol=1e-6, atol=1e-8, output_step=None, t_end=None, scripts=None, native_scripts=true))]
    #[allow(clippy::too_many_arguments)]
    fn sweep(
        &self,
        py: Python<'_>,
        sets: &Bound<'_, PyAny>,
        threads: usize,
        rtol: f64,
        atol: f64,
        output_step: Option<f64>,
        t_end: Option<f64>,
        scripts: Option<&Bound<'_, PyAny>>,
        native_scripts: bool,
    ) -> PyResult<Vec<Py<PyAny>>> {
        if threads == 0 {
            return Err(PyValueError::new_err("threads must be at least 1"));
        }
        let mut parsed: Vec<Result<Vec<(String, f64)>, String>> = vec![];
        for item in sets.try_iter()? {
            parsed.push(changes_of(&item?)?);
        }
        let scripts = scripts_of(scripts)?;
        core::check_script_keys(&self.built, scripts.keys()).map_err(|f| to_err(py, f))?;
        let factory = host_factory(scripts);
        let current: Vec<(String, f64)> =
            self.state().set.iter().map(|(k, v)| (k.clone(), *v)).collect();
        let built = Arc::clone(&self.built);
        let opts = run_options(rtol, atol, output_step, t_end, native_scripts);
        let results = py.detach(|| {
            core::par_map(parsed.len(), threads, |i| {
                let changes = parsed[i].clone().map_err(Failure::Value)?;
                let mut all = current.clone();
                all.extend(changes);
                let params = built.model.params_with(&all).map_err(Failure::Value)?;
                core::run(&built, &params, &opts, &factory)
            })
        });
        results
            .into_iter()
            .map(|r| match r {
                Ok(out) => Ok(Py::new(py, Run::new(Arc::clone(&self.built), out))?.into_any()),
                Err(f) => Ok(to_err(py, f).into_value(py).into_any()),
            })
            .collect()
    }

    fn __repr__(&self) -> String {
        let b = &self.built;
        format!("<lightsim_engine.Model case '{}' ({})>", b.case_id, b.rep.run.name)
    }
}

/// A dict of parameter changes: Err (inside) when a value is not a number,
/// so a sweep can report it for its set alone.
fn changes_of(item: &Bound<'_, PyAny>) -> PyResult<Result<Vec<(String, f64)>, String>> {
    let dict = item
        .cast::<PyDict>()
        .map_err(|_| PyTypeError::new_err("a parameter set must be a dict of name → value"))?;
    let mut out = vec![];
    for (k, v) in dict.iter() {
        let name: String =
            k.extract().map_err(|_| PyTypeError::new_err("parameter names must be text"))?;
        match v.extract::<f64>() {
            Ok(x) => out.push((name, x)),
            Err(_) => return Ok(Err(format!("'{name}' must be a number"))),
        }
    }
    Ok(Ok(out))
}

// ---- functions -------------------------------------------------------------

/// Imports, prepares (through `cache_dir` when given) and compiles a case
/// of a project (its JSON text). The GIL is released. `cycles_dir` is the
/// folder of the bundled drive cycles (default: the repository's).
#[pyfunction]
#[pyo3(signature = (project_json, case_id, cache_dir=None, cycles_dir=None))]
fn build(
    py: Python<'_>,
    project_json: &str,
    case_id: &str,
    cache_dir: Option<PathBuf>,
    cycles_dir: Option<PathBuf>,
) -> PyResult<Model> {
    let built = py
        .detach(|| core::build(project_json, case_id, cache_dir, cycles_dir))
        .map_err(|f| to_err(py, f))?;
    let values = built.model.info.params.clone();
    Ok(Model {
        built: Arc::new(built),
        params: Mutex::new(ParamState { set: BTreeMap::new(), values }),
    })
}

/// Structural and unit checks of a case, without building it: a list of
/// diagnostic dicts (empty when there is nothing to say).
#[pyfunction]
#[pyo3(signature = (project_json, case_id, cycles_dir=None))]
fn diagnose<'py>(
    py: Python<'py>,
    project_json: &str,
    case_id: &str,
    cycles_dir: Option<PathBuf>,
) -> PyResult<Bound<'py, PyList>> {
    let ds = py
        .detach(|| core::diagnose(project_json, case_id, cycles_dir))
        .map_err(|f| to_err(py, f))?;
    diag_list(py, &ds)
}

/// The engine's version.
#[pyfunction]
fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Builds and runs the Stage 1 spike at `rtol` (the GIL is released
/// while it runs). Returns (solve seconds, worst relative error against
/// the exact answer, event time error in s).
#[pyfunction]
fn spike(py: Python<'_>, rtol: f64) -> PyResult<(f64, f64, f64)> {
    py.detach(|| {
        let p = Params::default();
        let model = Engine::standard()
            .build(&battery_drive(&p), &BuildOptions::default())
            .map_err(|d| PyRuntimeError::new_err(format!("{d:?}")))?;
        let grid = OutputGrid { t0: 0.0, t_end: 4.0, dt: 0.01 };
        let run = model
            .simulate(&SolverOptions { rtol, atol: rtol, ..Default::default() }, grid)
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        let exact = Exact::new(&p);
        let mut worst = 0.0f64;
        for (j, name) in CHANNELS.iter().enumerate() {
            let ch = run.channel(name).expect("channel");
            let scale = run.times.iter().map(|t| exact.at(*t)[j].abs()).fold(0.0, f64::max);
            for (k, t) in run.times.iter().enumerate() {
                worst = worst.max((ch[k] - exact.at(*t)[j]).abs() / scale);
            }
        }
        let event = run.events.first().map(|e| (e.t - exact.t_event).abs()).unwrap_or(f64::NAN);
        Ok((run.wall_seconds, worst, event))
    })
}

/// The Python module.
#[pymodule]
fn lightsim_engine(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // the arrays need NumPy: without it a clear ImportError now, not a
    // panic at the first result
    m.py().import("numpy")?;
    m.add_function(wrap_pyfunction!(build, m)?)?;
    m.add_function(wrap_pyfunction!(diagnose, m)?)?;
    m.add_function(wrap_pyfunction!(version, m)?)?;
    m.add_function(wrap_pyfunction!(spike, m)?)?;
    m.add_class::<Model>()?;
    m.add_class::<Run>()?;
    m.add("ModelError", m.py().get_type::<ModelError>())?;
    Ok(())
}
