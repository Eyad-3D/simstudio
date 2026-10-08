//! # lsim-engine: build and run models
//!
//! The front door the Python bindings (lsim-py) and the app use:
//!
//! ```text
//! let engine = Engine::standard();
//! let model = engine.build(&top, &BuildOptions::default())?;          // prepare + JIT
//! let run = model.simulate(&SolverOptions::default(), grid)?;         // full dynamic
//! let runs = model.sweep(&sets, &SolverOptions::default(), grid, 4);  // in parallel
//! let inverse = engine.build_inverse(&top, &spec, &BuildOptions::default())?;
//! let fast = inverse.fast(&traces, &FastOptions::default())?;         // fast mode
//! ```
//!
//! * **Build.** Every `limit(x, lo, hi)` is made visible first
//!   ([`lsim_fast::limits`]): a full dynamic model clamps and records the
//!   demand and its band, an inverse model passes the demand through. The
//!   prepared model is looked up in the [`cache`] by a key of its inputs
//!   *without* runtime parameter values, then compiled to machine code.
//! * **Parameters** are runtime inputs: [`Model::set_param`] changes one
//!   without rebuilding, and runs with other values ([`Model::sweep`])
//!   share the compiled model. Start values that are expressions of
//!   parameters (a battery's initial SOC) are re-evaluated for each run.
//! * **Fast mode**: [`Engine::build_inverse`] prepares the inverse model of
//!   an [`InverseSpec`] ([`inverse`], a stopgap until work package 2's
//!   `prepare_inverse`), and [`Model::fast`] steps it ([`lsim_fast`]).
//!   [`Model::limit_hits`] reads the same limits from a full dynamic run,
//!   which is what fast mode's flags are checked against.
//!
//! [`spike`] holds the Stage 1 end-to-end model and its exact answer;
//! [`testcar`] a hand-written battery-electric car for fast mode, its
//! tests and the speed gates, until the example cars import.

pub mod cache;
pub mod inverse;
pub mod spike;
pub mod testcar;

use lsim_codegen::{CodegenOptions, JitModel};
use lsim_fast::limits::{FlagTracker, LimitMode, LimitSite};
use lsim_fast::{FastError, FastOptions, FastProblem, FastResult, LimitFlag, Trace, WhenInfo};
use lsim_ir::component::{ComponentDef, Library};
use lsim_ir::eval::{Env, eval};
use lsim_ir::expr::Expr;
use lsim_ir::prepared::{AliasTarget, Slot};
use lsim_ir::runtime::{EvalInput, Layout, ModelFunctions};
use lsim_ir::{Diagnostic, InverseSpec, ParamId, PreparedModel, VarId};
use lsim_prep::PrepOptions;
use lsim_solve::{OutputGrid, RunInfo, SimResult, SolveError, SolverOptions};
use std::sync::Arc;
use std::time::Instant;

pub use lsim_fast;
pub use lsim_ir;
pub use lsim_solve;

/// Options for [`Engine::build`].
#[derive(Clone, Debug, Default)]
pub struct BuildOptions {
    /// see [`PrepOptions::force_implicit`]
    pub force_implicit: bool,
    /// where prepared models are cached; `None`: no cache
    pub cache: Option<std::path::PathBuf>,
}

/// What building took and produced.
#[derive(Clone, Debug)]
pub struct BuildReport {
    /// `"full"` (full dynamic) or `"fast"` (an inverse model)
    pub mode: &'static str,
    /// preparation (flatten → sort), s; 0 on a cache hit
    pub prepare_seconds: f64,
    /// whether the prepared model came from the cache
    pub cache_hit: bool,
    /// the cache key (inputs without runtime parameter values)
    pub cache_key: String,
    /// compilation to machine code, s
    pub compile_seconds: f64,
    /// the whole build, s
    pub build_seconds: f64,
    /// machine code size, bytes
    pub code_bytes: usize,
    /// states, iteration variables, explicit assignments, aliases
    pub sizes: (usize, usize, usize, usize),
    /// flat variables and equations before alias elimination
    pub flat: (usize, usize),
    /// limits found (`limit(x, lo, hi)` in the equations)
    pub limits: usize,
    /// the structure key
    pub structure_key: String,
}

/// A model ready to run.
pub struct Model {
    /// the prepared model (names, units, origins)
    pub prepared: PreparedModel,
    /// its machine code
    pub jit: JitModel,
    /// what the run loop needs (parameter values included)
    pub info: RunInfo,
    /// how it was built
    pub report: BuildReport,
    /// fast mode's specification, for an inverse model
    pub inverse: Option<InverseSpec>,
    /// its limits
    pub limits: Vec<LimitSite>,
    /// its `when` clauses, for fast mode
    pub whens: Vec<WhenInfo>,
    starts: Vec<StartRule>,
}

/// Why a run could not start or failed.
#[derive(Debug, Clone, thiserror::Error)]
pub enum RunError {
    /// a parameter name or value is wrong
    #[error("{0}")]
    Param(String),
    /// the model was built for the other mode
    #[error("{0}")]
    Mode(String),
    /// the full dynamic run failed
    #[error(transparent)]
    Solve(#[from] SolveError),
    /// the fast-mode run failed
    #[error(transparent)]
    Fast(#[from] FastError),
}

/// A start value that is an expression of parameters (flat scope).
#[derive(Clone, Debug)]
struct StartRule {
    /// position in y (`Ok`) or in d (`Err`)
    slot: Result<usize, usize>,
    expr: Expr,
}

struct ParamsOnly<'a>(&'a [f64]);

impl Env for ParamsOnly<'_> {
    fn time(&self) -> f64 {
        f64::NAN
    }
    fn var(&self, _: VarId) -> f64 {
        f64::NAN
    }
    fn der(&self, _: VarId) -> f64 {
        f64::NAN
    }
    fn param(&self, p: ParamId) -> f64 {
        self.0[p.0 as usize]
    }
}

/// The compiled model with start values for one run's parameters.
struct Started<'a> {
    m: &'a JitModel,
    y0: Vec<f64>,
    d0: Vec<f64>,
}

impl ModelFunctions for Started<'_> {
    fn layout(&self) -> &Layout {
        self.m.layout()
    }
    fn residual(&self, inp: &EvalInput<'_>, work: &mut [f64], out: &mut [f64]) {
        self.m.residual(inp, work, out)
    }
    fn jvp(&self, inp: &EvalInput<'_>, v: &[f64], work: &mut [f64], out: &mut [f64]) {
        self.m.jvp(inp, v, work, out)
    }
    fn roots(&self, inp: &EvalInput<'_>, work: &mut [f64], out: &mut [f64]) {
        self.m.roots(inp, work, out)
    }
    fn vars(&self, inp: &EvalInput<'_>, work: &mut [f64], out: &mut [f64]) {
        self.m.vars(inp, work, out)
    }
    fn when(&self, inp: &EvalInput<'_>, fired: &[f64], work: &mut [f64], d_out: &mut [f64]) {
        self.m.when(inp, fired, work, d_out)
    }
    fn start(&self, _p: &[f64], y0: &mut [f64], d0: &mut [f64]) {
        y0.copy_from_slice(&self.y0);
        d0.copy_from_slice(&self.d0);
    }
}

/// The engine: a library to build models from.
pub struct Engine {
    lib: Library,
}

impl Engine {
    /// An engine over `lib`.
    pub fn new(lib: Library) -> Self {
        Engine { lib }
    }

    /// An engine over the standard library.
    pub fn standard() -> Self {
        Engine::new(lsim_lib::library())
    }

    /// The library.
    pub fn library(&self) -> &Library {
        &self.lib
    }

    /// Prepares (or fetches from the cache) and compiles `top` for full
    /// dynamic runs.
    pub fn build(&self, top: &ComponentDef, opts: &BuildOptions) -> Result<Model, Vec<Diagnostic>> {
        self.build_mode(top, None, opts)
    }

    /// Prepares (or fetches from the cache) and compiles the inverse model
    /// of `top` for fast mode.
    pub fn build_inverse(
        &self,
        top: &ComponentDef,
        spec: &InverseSpec,
        opts: &BuildOptions,
    ) -> Result<Model, Vec<Diagnostic>> {
        self.build_mode(top, Some(spec), opts)
    }

    /// Flattening, units and structure only: the model's faults, or none.
    pub fn diagnose(&self, top: &ComponentDef) -> Vec<Diagnostic> {
        let ins = lsim_fast::limits::instrument(&self.lib, top, LimitMode::Clamp);
        match lsim_prep::prepare(&ins.lib, &ins.top, &PrepOptions::default()) {
            Ok(_) => vec![],
            Err(d) => d,
        }
    }

    fn build_mode(
        &self,
        top: &ComponentDef,
        inverse: Option<&InverseSpec>,
        opts: &BuildOptions,
    ) -> Result<Model, Vec<Diagnostic>> {
        let started = Instant::now();
        let mode = if inverse.is_some() { LimitMode::PassThrough } else { LimitMode::Clamp };
        let ins = lsim_fast::limits::instrument(&self.lib, top, mode);
        let cache = opts.cache.as_ref().map(cache::DiskCache::new);
        let key = cache::input_key(&ins.lib, &ins.top, opts.force_implicit, inverse);
        let popts = PrepOptions { force_implicit: opts.force_implicit };
        let hit = cache.as_ref().and_then(|c| c.get(&key));
        let cache_hit = hit.is_some();
        let mut prepared = match hit {
            Some(mut m) => {
                let fresh = lsim_prep::flatten::flatten(&ins.lib, &ins.top)?;
                cache::refresh(&mut m, &fresh);
                m
            }
            None => {
                let m = match inverse {
                    None => lsim_prep::prepare(&ins.lib, &ins.top, &popts)?,
                    Some(spec) => inverse::prepare_inverse(&ins.lib, &ins.top, spec, &popts)?,
                };
                if let Some(c) = &cache {
                    c.put(&key, &m);
                }
                m
            }
        };
        let prepare_seconds = if cache_hit { 0.0 } else { started.elapsed().as_secs_f64() };
        propagate_guesses(&mut prepared);
        let jit = lsim_codegen::compile(&prepared, &CodegenOptions::default())
            .map_err(|e| vec![Diagnostic::error("CODEGEN", e.to_string())])?;
        let info = RunInfo::from_prepared(&prepared);
        let limits = lsim_fast::limits::sites(&prepared.flat, &ins.limits);
        let whens = prepared
            .whens
            .iter()
            .zip(&info.when_labels)
            .map(|(w, l)| WhenInfo {
                crossing: w.crossing,
                direction: w.direction,
                label: l.clone(),
            })
            .collect();
        let starts = start_rules(&ins.lib, &ins.top, &prepared);
        let report = BuildReport {
            mode: if inverse.is_some() { "fast" } else { "full" },
            prepare_seconds,
            cache_hit,
            cache_key: key,
            compile_seconds: jit.compile_seconds,
            build_seconds: started.elapsed().as_secs_f64(),
            code_bytes: jit.code_bytes,
            sizes: (
                prepared.states.len(),
                prepared.algebraics.len(),
                prepared.assignments.len(),
                prepared.aliases.len(),
            ),
            flat: (prepared.stats.flat_vars, prepared.stats.flat_equations),
            limits: limits.len(),
            structure_key: prepared.structure_key.clone(),
        };
        Ok(Model { prepared, jit, info, report, inverse: inverse.cloned(), limits, whens, starts })
    }
}

/// Gives an iteration variable without a start value (a guess) the start
/// value of a variable eliminated as its alias, so Newton's method starts
/// from the guess the component writer gave (a bus voltage near the
/// battery's, not zero).
fn propagate_guesses(m: &mut PreparedModel) {
    for a in &m.aliases {
        if let AliasTarget::Var { var, negated } = a.target {
            let from = m.flat.var(a.var).start;
            let to = &mut m.flat.vars[var.0 as usize];
            if to.start.is_none()
                && let Some(s) = from
            {
                to.start = Some(if negated { -s } else { s });
            }
        }
    }
}

/// The start values that follow from parameters: each state's, iteration
/// variable's and discrete variable's declared start expression when it is
/// not a plain number, in flat scope.
fn start_rules(lib: &Library, top: &ComponentDef, m: &PreparedModel) -> Vec<StartRule> {
    let flat = &m.flat;
    let n_x = m.states.len();
    let mut slots: Vec<(VarId, Result<usize, usize>)> = vec![];
    for (i, v) in m.states.iter().enumerate() {
        slots.push((*v, Ok(i)));
    }
    for (k, s) in m.algebraics.iter().enumerate() {
        if let Slot::Var(v) = s {
            slots.push((*v, Ok(n_x + k)));
        }
    }
    for (k, v) in m.discretes.iter().enumerate() {
        slots.push((*v, Err(k)));
    }
    let mut rules = vec![];
    for (v, slot) in slots {
        let var = flat.var(v);
        let inst = flat.instance(var.instance);
        let def = if var.instance.0 == 0 { Some(top) } else { lib.components.get(&inst.def) };
        let Some(def) = def else { continue };
        let local = var.name.strip_prefix(inst.path.as_str()).unwrap_or(&var.name);
        let local = local.trim_start_matches('.');
        let Some(decl) = def.vars.iter().find(|d| d.name == local) else { continue };
        let Some(start) = &decl.start else { continue };
        if matches!(start, Expr::Const(_)) {
            continue;
        }
        let mut ok = true;
        let resolved = start.clone().rewrite(&mut |x| match x {
            Expr::Name(n) => {
                let full =
                    if inst.path.is_empty() { n.clone() } else { format!("{}.{n}", inst.path) };
                match flat.find_param(&full) {
                    Some(p) => Expr::Param(p),
                    None => {
                        ok = false;
                        Expr::Name(n)
                    }
                }
            }
            other => other,
        });
        if ok {
            rules.push(StartRule { slot, expr: resolved });
        }
    }
    rules
}

/// Runs `f(i)` for `i` in `0..n` on up to `threads` threads, in order.
fn par_map<T: Send>(n: usize, threads: usize, f: impl Fn(usize) -> T + Sync) -> Vec<T> {
    let threads = threads.clamp(1, n.max(1));
    if threads == 1 {
        return (0..n).map(f).collect();
    }
    let next = std::sync::atomic::AtomicUsize::new(0);
    let mut out: Vec<Option<T>> = (0..n).map(|_| None).collect();
    let slots: Vec<std::sync::Mutex<&mut Option<T>>> =
        out.iter_mut().map(std::sync::Mutex::new).collect();
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if i >= n {
                        break;
                    }
                    let r = f(i);
                    **slots[i].lock().expect("a slot") = Some(r);
                }
            });
        }
    });
    drop(slots);
    out.into_iter().map(|x| x.expect("every index ran")).collect()
}

impl Model {
    /// The parameters' names, in the order of their values.
    pub fn param_names(&self) -> Vec<&str> {
        self.prepared.flat.params.iter().map(|p| p.name.as_str()).collect()
    }

    /// A parameter's current value, SI.
    pub fn param(&self, name: &str) -> Option<f64> {
        let id = self.prepared.flat.find_param(name)?;
        Some(self.info.params[id.0 as usize])
    }

    /// The parameter values with `changes` applied (by full name, SI) and
    /// the parameters bound to them re-evaluated.
    pub fn params_with(&self, changes: &[(String, f64)]) -> Result<Vec<f64>, String> {
        let flat = &self.prepared.flat;
        let mut p = self.info.params.clone();
        for (name, value) in changes {
            let id = flat.find_param(name).ok_or_else(|| format!("no parameter '{name}'"))?;
            if flat.params[id.0 as usize].binding.is_some() {
                return Err(format!("'{name}' is bound to another parameter; set that one"));
            }
            if !value.is_finite() {
                return Err(format!("'{name}' must be a number, not {value}"));
            }
            p[id.0 as usize] = *value;
        }
        // bindings refer only to parameters created before them (a parent's
        // parameters precede its children's), so one pass in order suffices
        for (i, prm) in flat.params.iter().enumerate() {
            if let Some(b) = &prm.binding {
                p[i] = eval(b, &ParamsOnly(&p));
            }
        }
        Ok(p)
    }

    /// Sets a parameter by its full name, in SI, and re-evaluates the
    /// parameters bound to it (a sub-component's value given by its
    /// parent's parameter). No recompilation: parameters are inputs of the
    /// machine code.
    pub fn set_param(&mut self, name: &str, value: f64) -> Result<(), String> {
        self.info.params = self.params_with(&[(name.to_string(), value)])?;
        Ok(())
    }

    /// Sets several parameters (see [`Model::set_param`]).
    pub fn set_params(&mut self, changes: &[(String, f64)]) -> Result<(), String> {
        self.info.params = self.params_with(changes)?;
        Ok(())
    }

    fn started(&self, params: &[f64]) -> Started<'_> {
        let l = *self.jit.layout();
        let mut y0 = vec![0.0; l.n_y()];
        let mut d0 = vec![0.0; l.n_d];
        self.jit.start(params, &mut y0, &mut d0);
        for r in &self.starts {
            let v = eval(&r.expr, &ParamsOnly(params));
            match r.slot {
                Ok(i) => y0[i] = v,
                Err(k) => d0[k] = v,
            }
        }
        Started { m: &self.jit, y0, d0 }
    }

    /// A variable's index (its channel), by full name.
    pub fn var_index(&self, name: &str) -> Option<usize> {
        self.info.var_names.iter().position(|n| n == name)
    }

    /// Runs the model over `grid` (full dynamic).
    pub fn simulate(
        &self,
        opts: &SolverOptions,
        grid: OutputGrid,
    ) -> Result<SimResult, SolveError> {
        match self.simulate_with(&self.info.params, opts, grid) {
            Ok(r) => Ok(r),
            Err(RunError::Solve(e)) => Err(e),
            Err(e) => Err(SolveError::Integrator { t: grid.t0, message: e.to_string() }),
        }
    }

    /// Runs the model with other parameter values (the whole vector, as
    /// [`Model::params_with`] gives it).
    pub fn simulate_with(
        &self,
        params: &[f64],
        opts: &SolverOptions,
        grid: OutputGrid,
    ) -> Result<SimResult, RunError> {
        if self.inverse.is_some() {
            return Err(RunError::Mode(
                "this is fast mode's inverse model; build the model itself for a full dynamic run"
                    .into(),
            ));
        }
        if params.len() != self.info.params.len() {
            return Err(RunError::Param("wrong number of parameter values".into()));
        }
        let m = self.started(params);
        let info = RunInfo { params: params.to_vec(), ..self.info.clone() };
        Ok(lsim_solve::simulate(&m, &info, opts, grid)?)
    }

    /// Runs fast mode over the prescribed traces (one per prescribed
    /// variable, in the [`InverseSpec`]'s order).
    pub fn fast(&self, traces: &[Trace], o: &FastOptions) -> Result<FastResult, FastError> {
        match self.fast_with(&self.info.params, traces, o, None, &mut []) {
            Ok(r) => Ok(r),
            Err(RunError::Fast(e)) => Err(e),
            Err(e) => Err(FastError::Inputs(e.to_string())),
        }
    }

    /// Runs fast mode with other parameter values, recording only the
    /// variables in `record` (`None`: all), with sampled blocks.
    pub fn fast_with(
        &self,
        params: &[f64],
        traces: &[Trace],
        o: &FastOptions,
        record: Option<&[usize]>,
        blocks: &mut [lsim_fast::BoundBlock],
    ) -> Result<FastResult, RunError> {
        if self.inverse.is_none() {
            return Err(RunError::Mode(
                "this model was built for full dynamic runs; build its inverse model for fast mode"
                    .into(),
            ));
        }
        if params.len() != self.info.params.len() {
            return Err(RunError::Param("wrong number of parameter values".into()));
        }
        let m = self.started(params);
        let prob = FastProblem {
            model: &m,
            params,
            traces,
            opts: o,
            y_nominal: &self.info.y_nominal,
            names: &self.info.var_names,
            record,
            limits: &self.limits,
            whens: &self.whens,
        };
        Ok(lsim_fast::run(&prob, blocks)?)
    }

    /// Full dynamic runs for several parameter sets, in parallel on up to
    /// `threads` threads, sharing the compiled model.
    pub fn sweep(
        &self,
        sets: &[Vec<(String, f64)>],
        opts: &SolverOptions,
        grid: OutputGrid,
        threads: usize,
    ) -> Vec<Result<SimResult, RunError>> {
        par_map(sets.len(), threads, |i| {
            let p = self.params_with(&sets[i]).map_err(RunError::Param)?;
            self.simulate_with(&p, opts, grid)
        })
    }

    /// Fast-mode runs for several parameter sets, in parallel.
    pub fn fast_sweep(
        &self,
        sets: &[Vec<(String, f64)>],
        traces: &[Trace],
        o: &FastOptions,
        record: Option<&[usize]>,
        threads: usize,
    ) -> Vec<Result<FastResult, RunError>> {
        par_map(sets.len(), threads, |i| {
            let p = self.params_with(&sets[i]).map_err(RunError::Param)?;
            self.fast_with(&p, traces, o, record, &mut [])
        })
    }

    /// The stretches of a full dynamic run when a limit was hit (its demand
    /// outside its band while the limit clamped), read from the output
    /// points: what fast mode's flags are checked against.
    pub fn limit_hits(&self, run: &SimResult, rtol: f64) -> Vec<LimitFlag> {
        let mut tracker = FlagTracker::new(&self.limits, rtol);
        let mut vars = vec![f64::NAN; run.names.len()];
        let needed: Vec<usize> =
            self.limits.iter().flat_map(|s| [s.demand, s.lower, s.upper]).collect();
        for (k, t) in run.times.iter().enumerate() {
            for &i in &needed {
                vars[i] = run.values[i][k];
            }
            tracker.point(*t, &vars);
        }
        tracker.finish(run.times.last().copied().unwrap_or(0.0))
    }
}

/// A shareable engine and model, for callers that keep both (the Python
/// module).
pub type SharedModel = Arc<Model>;
