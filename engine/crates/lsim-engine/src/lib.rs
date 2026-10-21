//! # lsim-engine: build and run models
//!
//! The front door the Python bindings (lsim-py) and the app use:
//!
//! ```text
//! let engine = Engine::standard();
//! let model = engine.build(&top, &BuildOptions::default())?;              // prepare + JIT
//! let run = model.simulate(&SolverOptions::default(), grid)?;             // full dynamic
//! let runs = model.sweep(&sets, &SolverOptions::default(), grid, 4);      // in parallel
//! let inverse = engine.build_inverse(&top, &spec, &BuildOptions::default())?;
//! let fast = inverse.fast(&traces, &FastOptions::default())?;             // fast mode
//! ```
//!
//! * **Build.** The prepared model is looked up in the [`cache`] by a key
//!   of all its inputs, parameter values included, prepared on a miss
//!   (`lsim_prep::prepare`, or `prepare_inverse` for fast mode), then
//!   compiled to machine code. Preparation's warnings are in the
//!   [`BuildReport`].
//! * **Parameters** are runtime inputs: [`Model::set_param`] changes one
//!   without rebuilding, and runs with other values ([`Model::sweep`])
//!   share the compiled model. Start values that follow from parameters
//!   are worked out by the model's initialisation at the start of each
//!   run. A change that makes a divisor preparation relied on zero (a
//!   `ParamGuard`) is refused with the reason: such a model must be
//!   prepared again ([`Engine::build`]). Index reduction's choice of
//!   states is made at the values the model was prepared at; a change at
//!   which that choice does not work at the start is refused the same way,
//!   naming the constraint (the model's initialisation at the new values,
//!   then the chosen pivots there: [`lsim_solve::states`]), and a run whose
//!   choice degrades along the way stops there, naming it.
//! * **Fast mode**: [`Engine::build_inverse`] prepares the inverse model
//!   of an [`InverseSpec`], and [`Model::fast`] steps it ([`lsim_fast`]),
//!   flagging every limit preparation passed through (it never changes the
//!   answer). [`Model::limit_hits`] reads the same limits from a full
//!   dynamic run's channels, which is what fast mode's flags are checked
//!   against.
//! * Every run goes through [`Started`], which forwards every method of
//!   the compiled model (DESIGN.md §5.8).
//!
//! [`spike`] holds the Stage 1 end-to-end model and its exact answer;
//! [`testcar`] a hand-written battery-electric car for fast mode, its
//! tests and the speed gates.

pub mod cache;
pub mod spike;
mod started;
pub mod testcar;

pub use started::Started;

use lsim_codegen::{CodegenOptions, JitModel};
use lsim_fast::limits::{DerSrc, FlagTracker, LimitSite, Term};
use lsim_fast::{
    FastError, FastOptions, FastProblem, FastResult, LimitFlag, ModeInfo, Trace, WhenInfo,
};
use lsim_ir::component::{ComponentDef, Library};
use lsim_ir::eval::{Env, eval};
use lsim_ir::expr::Expr;
use lsim_ir::flat::FlatSystem;
use lsim_ir::runtime::{DiscreteBlock, ModelFunctions};
use lsim_ir::{Diagnostic, InverseSpec, ParamId, PreparedModel, VarId};
use lsim_prep::PrepOptions;
use lsim_solve::{OutputGrid, RunInfo, SimResult, SolveError, SolverOptions};
use std::collections::HashMap;
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
    /// the cache key (a digest of every input)
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
    /// limits fast mode flags (an inverse model's; 0 for a forward model)
    pub limits: usize,
    /// the structure key
    pub structure_key: String,
    /// what preparation noticed that does not stop the model from running
    pub warnings: Vec<Diagnostic>,
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
    /// its limits (an inverse model's)
    pub limits: Vec<LimitSite>,
    /// where the derivatives its limits read are
    ders: Vec<Option<DerSrc>>,
    /// its `when` clauses, for fast mode
    pub whens: Vec<WhenInfo>,
    /// its modes, for fast mode's check of them after every step
    pub modes: Vec<ModeInfo>,
    /// for each input (an inverse model's): the trace that fills it and
    /// which derivative of it
    pub inputs: Vec<(usize, u8)>,
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

/// The first parameter guard of `m` that is zero (or not a number) at the
/// parameter values `p`: what preparation divided by.
fn broken_guard(m: &PreparedModel, p: &[f64]) -> Option<String> {
    m.guards.iter().find_map(|g| {
        let v = eval(&g.expr, &ParamsOnly(p));
        (v == 0.0 || !v.is_finite()).then(|| {
            let who = m.flat.instance_name(g.origin.instance);
            let what = g.origin.label.clone().unwrap_or_else(|| "an equation".into());
            format!(
                "with these values {who} ({what}) would divide by {g_expr} = {v}: the model must \
                 be prepared again for them (build it again)",
                g_expr = g.expr
            )
        })
    })
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
        match lsim_prep::prepare(&self.lib, top, &PrepOptions::default()) {
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
        let cache = opts.cache.as_ref().map(cache::DiskCache::new);
        let key = cache::input_key(&self.lib, top, opts.force_implicit, inverse);
        let popts = PrepOptions { force_implicit: opts.force_implicit };
        let prepare = || match inverse {
            None => lsim_prep::prepare(&self.lib, top, &popts),
            Some(spec) => lsim_prep::prepare_inverse(&self.lib, top, spec, &popts),
        };
        // the key holds every input, parameter values included: a hit is
        // the model preparation would give (cache.rs)
        let hit = cache.as_ref().and_then(|c| c.get(&key));
        let cache_hit = hit.is_some();
        let prepared = match hit {
            Some(m) => m,
            None => {
                let m = prepare()?;
                if let Some(c) = &cache {
                    c.put(&key, &m);
                }
                m
            }
        };
        let prepare_seconds = if cache_hit { 0.0 } else { started.elapsed().as_secs_f64() };
        let jit = lsim_codegen::compile(&prepared, &CodegenOptions::default())
            .map_err(|e| vec![Diagnostic::error("CODEGEN", e.to_string())])?;
        let info = RunInfo::from_prepared(&prepared);
        let limits = lsim_fast::limits::sites(&prepared);
        let ders = lsim_fast::limits::der_sources(&prepared, &limits);
        let whens = prepared
            .whens
            .iter()
            .zip(&info.when_labels)
            .map(|(w, l)| WhenInfo {
                crossing: w.crossing,
                direction: w.direction,
                strict: w.strict,
                label: l.clone(),
            })
            .collect();
        let modes = info
            .modes
            .iter()
            .map(|m| ModeInfo {
                crossing: m.crossing,
                discrete: m.discrete,
                label: m.label.clone(),
            })
            .collect();
        let inputs = match inverse {
            Some(spec) => input_map(&prepared, spec)?,
            None => vec![],
        };
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
            warnings: prepared.warnings.clone(),
        };
        Ok(Model {
            prepared,
            jit,
            info,
            report,
            inverse: inverse.cloned(),
            limits,
            ders,
            whens,
            modes,
            inputs,
        })
    }
}

/// Which trace and which derivative fills each input of an inverse model
/// (`PreparedModel::input_names()`: `body.v`, `der(body.v)`, …).
fn input_map(m: &PreparedModel, spec: &InverseSpec) -> Result<Vec<(usize, u8)>, Vec<Diagnostic>> {
    m.input_names()
        .iter()
        .map(|name| {
            let mut s = name.as_str();
            let mut order = 0u8;
            while let Some(inner) = s.strip_prefix("der(").and_then(|r| r.strip_suffix(')')) {
                s = inner;
                order += 1;
            }
            spec.prescribed.iter().position(|p| p == s).map(|i| (i, order)).ok_or_else(|| {
                vec![Diagnostic::error(
                    "INVERSE-INPUT",
                    format!(
                        "Fast mode cannot fill the inverse model's input '{name}' from the \
                         prescribed traces (an engine fault; please report the model)."
                    ),
                )]
            })
        })
        .collect()
}

/// An expression of one flat system rewritten into another's (variables,
/// parameters and tables matched by name); `None` when something it reads
/// is not there.
fn remap(e: &Expr, from: &FlatSystem, to: &FlatSystem) -> Option<Expr> {
    let mut ok = true;
    let var = |v: VarId, ok: &mut bool| {
        to.find_var(&from.var(v).name).unwrap_or_else(|| {
            *ok = false;
            v
        })
    };
    let out = e.clone().rewrite(&mut |x| match x {
        Expr::Var(v) => Expr::Var(var(v, &mut ok)),
        Expr::Der(v) => Expr::Der(var(v, &mut ok)),
        Expr::Pre(v) => Expr::Pre(var(v, &mut ok)),
        Expr::Param(p) => match to.find_param(&from.params[p.0 as usize].name) {
            Some(q) => Expr::Param(q),
            None => {
                ok = false;
                Expr::Param(p)
            }
        },
        Expr::Table { table, args, order } => {
            let name = &from.tables[table as usize].name;
            match to.tables.iter().position(|t| &t.name == name) {
                Some(k) => Expr::Table { table: k as u32, args, order },
                None => {
                    ok = false;
                    Expr::Table { table, args, order }
                }
            }
        }
        other => other,
    });
    ok.then_some(out)
}

/// One output point of a full dynamic run, as an expression environment.
struct RunPoint<'a> {
    run: &'a SimResult,
    channel: &'a [usize],
    k: usize,
    p: &'a [f64],
    model: &'a dyn ModelFunctions,
}

impl Env for RunPoint<'_> {
    fn time(&self) -> f64 {
        self.run.times[self.k]
    }
    fn var(&self, v: VarId) -> f64 {
        self.channel.get(v.0 as usize).map_or(f64::NAN, |&c| self.run.values[c][self.k])
    }
    fn der(&self, _: VarId) -> f64 {
        f64::NAN
    }
    fn param(&self, p: ParamId) -> f64 {
        self.p[p.0 as usize]
    }
    fn table(&self, k: u32, order: [u8; 2], args: &[f64]) -> f64 {
        lsim_ir::interval::table_at(self.model, k, order, args).map_or(f64::NAN, |(v, _)| v)
    }
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
            if flat.tables.iter().any(|t| t.param == Some(id)) {
                return Err(format!("'{name}' is a table: give its data as a table, not a number"));
            }
            if flat.params[id.0 as usize].structural {
                return Err(format!(
                    "'{name}' changes the model's structure: build the model again with it"
                ));
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
        if let Some(why) = broken_guard(&self.prepared, &p) {
            return Err(why);
        }
        if self.inverse.is_none()
            && let Some(why) = self.state_choice_at_start(&p)
        {
            return Err(why);
        }
        Ok(p)
    }

    /// Whether index reduction's choice of states fails at the start with
    /// the parameter values `p`: the model's initialisation at them (at
    /// t = 0, the default tolerances), then the chosen pivots there. `None`
    /// when it works, or when the start cannot be solved (the run tells
    /// why).
    fn state_choice_at_start(&self, p: &[f64]) -> Option<String> {
        use lsim_solve::SetReads;
        let watched = |r: SetReads| matches!(r, SetReads::Variables | SetReads::Parameters);
        if !self.info.state_sets.iter().any(|s| watched(s.reads)) {
            return None;
        }
        let l = *self.jit.layout();
        let (mut y0, mut d0, u) = (vec![0.0; l.n_y()], vec![0.0; l.n_d], vec![0.0; l.n_u]);
        self.jit.start(p, &mut y0, &mut d0);
        let info = self.info.with_params(p);
        let o = SolverOptions::default();
        let settings =
            lsim_solve::init::InitSettings { rtol: o.rtol, atol: o.atol, max_iterations: 50 };
        let _ =
            lsim_solve::init::initialise(&self.jit, &info, 0.0, &mut y0, &mut d0, &u, &settings);
        match lsim_solve::states::check_start(&self.jit, &info, 0.0, &y0, &d0, &u) {
            Err(SolveError::StateSelection { message, .. }) => {
                Some(format!("with these values, {message}"))
            }
            _ => None,
        }
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
        match self.simulate_with(&self.info.params, opts, grid, &mut []) {
            Ok(r) => Ok(r),
            Err(RunError::Solve(e)) => Err(e),
            Err(e) => Err(SolveError::Integrator { t: grid.t0, message: e.to_string() }),
        }
    }

    /// Runs the model with other parameter values (the whole vector, as
    /// [`Model::params_with`] gives it), driving the sampled `blocks` (one
    /// per `info.blocks` entry, in order).
    pub fn simulate_with(
        &self,
        params: &[f64],
        opts: &SolverOptions,
        grid: OutputGrid,
        blocks: &mut [Box<dyn DiscreteBlock>],
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
        let m = Started::new(&self.jit);
        Ok(lsim_solve::simulate(&m, &self.info.with_params(params), opts, grid, blocks)?)
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

    /// The consistent start of a fast-mode run: the model's initialisation
    /// at the traces' first point.
    fn fast_start(
        &self,
        params: &[f64],
        traces: &[Trace],
        o: &FastOptions,
    ) -> Result<(Vec<f64>, Vec<f64>), RunError> {
        let l = *self.jit.layout();
        let t0 = o
            .t_start
            .unwrap_or_else(|| traces.iter().map(|t| t.start()).fold(f64::NEG_INFINITY, f64::max));
        let mut u = vec![0.0; l.n_u];
        for (j, &(i, order)) in self.inputs.iter().enumerate() {
            let Some(tr) = traces.get(i) else {
                return Err(RunError::Fast(FastError::Inputs(format!(
                    "the inverse model prescribes {} variables but {} traces were given",
                    self.inverse.as_ref().map_or(0, |s| s.prescribed.len()),
                    traces.len()
                ))));
            };
            tr.check()?;
            // as the stepper reads the trace (the line of its piece)
            let (v, s) = tr.at(t0);
            u[j] = match order {
                0 => v,
                1 => s,
                _ => 0.0,
            };
        }
        let mut y0 = vec![0.0; l.n_y()];
        let mut d0 = vec![0.0; l.n_d];
        self.jit.start(params, &mut y0, &mut d0);
        let info = self.info.with_params(params);
        let settings =
            lsim_solve::init::InitSettings { rtol: o.rtol, atol: o.atol, max_iterations: 50 };
        lsim_solve::init::initialise(&self.jit, &info, t0, &mut y0, &mut d0, &u, &settings)?;
        Ok((y0, d0))
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
        // a sampled block runs only with its host: without one its outputs
        // would hold their start values for the whole run (a full dynamic
        // run refuses the same, lsim-solve)
        if blocks.len() != self.info.blocks.len() {
            return Err(RunError::Mode(format!(
                "the model has {} sampled block(s) (Script, FMU or Traction Control parts) and \
                 {} given: run them with their hosts",
                self.info.blocks.len(),
                blocks.len()
            )));
        }
        let (y0, d0) = self.fast_start(params, traces, o)?;
        let m = Started::with_start(&self.jit, y0, d0);
        let prob = FastProblem {
            model: &m,
            params,
            traces,
            opts: o,
            y_nominal: &self.info.y_nominal,
            names: &self.info.var_names,
            y_names: &self.info.y_names,
            record,
            limits: &self.limits,
            ders: &self.ders,
            inputs: &self.inputs,
            whens: &self.whens,
            modes: &self.modes,
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
        let params: Vec<Result<Vec<f64>, String>> =
            sets.iter().map(|s| self.params_with(s)).collect();
        let good: Vec<Vec<f64>> = params.iter().filter_map(|p| p.clone().ok()).collect();
        if self.inverse.is_some() {
            return sets
                .iter()
                .map(|_| Err(RunError::Mode("this is fast mode's inverse model".into())))
                .collect();
        }
        let m = Started::new(&self.jit);
        let mut runs = lsim_solve::sweep(&m, &self.info, &good, opts, grid, threads).into_iter();
        params
            .into_iter()
            .map(|p| match p {
                Ok(_) => runs.next().expect("one run per good set").map_err(RunError::Solve),
                Err(e) => Err(RunError::Param(e)),
            })
            .collect()
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

    /// The stretches of a full dynamic run (of this model) when a limit of
    /// `inverse` (its fast-mode model) was hit, read from the output
    /// points: the limited value outside its band, which the forward model
    /// clamps. What fast mode's flags are checked against. Fails, naming
    /// the limit, when the run does not have a channel a limit reads.
    pub fn limit_hits(
        &self,
        inverse: &Model,
        run: &SimResult,
        rtol: f64,
    ) -> Result<Vec<LimitFlag>, String> {
        let (from, to) = (&inverse.prepared.flat, &self.prepared.flat);
        let term = |t: &Term, label: &str| -> Result<Term, String> {
            let e = match t {
                Term::Const(c) => return Ok(Term::Const(*c)),
                Term::Var(i) => Expr::Var(VarId(*i as u32)),
                Term::Param(i) => Expr::Param(ParamId(*i as u32)),
                Term::Expr(e) => e.clone(),
            };
            match remap(&e, from, to) {
                Some(Expr::Var(v)) => Ok(Term::Var(v.0 as usize)),
                Some(Expr::Param(p)) => Ok(Term::Param(p.0 as usize)),
                Some(e) if !e.any(&mut |x| matches!(x, Expr::Der(_))) => Ok(Term::Expr(e)),
                _ => Err(format!("the full dynamic run cannot tell when {label} was hit")),
            }
        };
        let mut sites = vec![];
        for s in &inverse.limits {
            sites.push(LimitSite {
                value: term(&s.value, &s.label)?,
                lo: term(&s.lo, &s.label)?,
                hi: term(&s.hi, &s.label)?,
                ..s.clone()
            });
        }
        let by_name: HashMap<&str, usize> =
            run.names.iter().enumerate().map(|(i, n)| (n.as_str(), i)).collect();
        let channel: Vec<usize> = to
            .vars
            .iter()
            .map(|v| by_name.get(v.name.as_str()).copied().unwrap_or(usize::MAX))
            .collect();
        let mut tracker = FlagTracker::new(&sites, rtol);
        let mut vals = vec![[0.0; 3]; sites.len()];
        for k in 0..run.times.len() {
            let pt = RunPoint { run, channel: &channel, k, p: &self.info.params, model: &self.jit };
            let get = |t: &Term| match t {
                Term::Const(c) => *c,
                Term::Var(i) => pt.var(VarId(*i as u32)),
                Term::Param(i) => self.info.params[*i],
                Term::Expr(e) => eval(e, &pt),
            };
            for (v, s) in vals.iter_mut().zip(&sites) {
                *v = [get(&s.value), get(&s.lo), get(&s.hi)];
            }
            tracker.point(run.times[k], &vals);
        }
        Ok(tracker.finish(run.times.last().copied().unwrap_or(0.0)))
    }
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

/// A shareable model, for callers that keep it (the Python module).
pub type SharedModel = Arc<Model>;
