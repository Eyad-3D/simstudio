//! # lsim-engine: build and run models
//!
//! The front door the Python bindings (lsim-py) and the app use:
//!
//! ```text
//! let engine = Engine::standard();
//! let model = engine.build(&top, &BuildOptions::default())?;   // prepare + JIT
//! let run = model.simulate(&SolverOptions::default(), grid)?;  // as often as wanted
//! ```
//!
//! [`Engine::build`] looks the prepared model up in the [`cache`] by a key
//! of its inputs before preparing it; compiling to machine code takes
//! milliseconds and is always done. Parameters are runtime inputs:
//! [`Model::set_param`] changes one without rebuilding.
//!
//! [`spike`] holds the Stage 1 end-to-end model and its exact answer.

pub mod cache;
pub mod spike;

use lsim_codegen::{CodegenOptions, JitModel};
use lsim_ir::{ComponentDef, Diagnostic, Library, PreparedModel};
use lsim_prep::PrepOptions;
use lsim_solve::{OutputGrid, RunInfo, SimResult, SolveError, SolverOptions};
use std::time::Instant;

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
    /// preparation (flatten → sort), s; 0 on a cache hit
    pub prepare_seconds: f64,
    /// whether the prepared model came from the cache
    pub cache_hit: bool,
    /// compilation to machine code, s
    pub compile_seconds: f64,
    /// machine code size, bytes
    pub code_bytes: usize,
    /// states, iteration variables, explicit assignments, aliases
    pub sizes: (usize, usize, usize, usize),
    /// flat variables and equations before alias elimination
    pub flat: (usize, usize),
    /// the structure key
    pub structure_key: String,
}

/// A model ready to run.
pub struct Model {
    /// the prepared model (names, units, origins)
    pub prepared: PreparedModel,
    /// its machine code
    pub jit: JitModel,
    /// what the run loop needs
    pub info: RunInfo,
    /// how it was built
    pub report: BuildReport,
}

struct ParamsOnly<'a>(&'a [f64]);

impl lsim_ir::eval::Env for ParamsOnly<'_> {
    fn time(&self) -> f64 {
        f64::NAN
    }
    fn var(&self, _: lsim_ir::VarId) -> f64 {
        f64::NAN
    }
    fn der(&self, _: lsim_ir::VarId) -> f64 {
        f64::NAN
    }
    fn param(&self, p: lsim_ir::ParamId) -> f64 {
        self.0[p.0 as usize]
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

    /// Prepares (or fetches from the cache) and compiles `top`.
    pub fn build(&self, top: &ComponentDef, opts: &BuildOptions) -> Result<Model, Vec<Diagnostic>> {
        let started = Instant::now();
        let cache = opts.cache.as_ref().map(cache::DiskCache::new);
        let key = cache::input_key(&self.lib, top, opts.force_implicit);
        let (prepared, hit) = match cache.as_ref().and_then(|c| c.get(&key)) {
            Some(m) => (m, true),
            None => {
                let m = lsim_prep::prepare(
                    &self.lib,
                    top,
                    &PrepOptions { force_implicit: opts.force_implicit },
                )?;
                if let Some(c) = &cache {
                    c.put(&key, &m);
                }
                (m, false)
            }
        };
        let prepare_seconds = if hit { 0.0 } else { started.elapsed().as_secs_f64() };
        let jit = lsim_codegen::compile(&prepared, &CodegenOptions::default())
            .map_err(|e| vec![Diagnostic::error("CODEGEN", e.to_string())])?;
        let info = RunInfo::from_prepared(&prepared);
        let report = BuildReport {
            prepare_seconds,
            cache_hit: hit,
            compile_seconds: jit.compile_seconds,
            code_bytes: jit.code_bytes,
            sizes: (
                prepared.states.len(),
                prepared.algebraics.len(),
                prepared.assignments.len(),
                prepared.aliases.len(),
            ),
            flat: (prepared.stats.flat_vars, prepared.stats.flat_equations),
            structure_key: prepared.structure_key.clone(),
        };
        Ok(Model { prepared, jit, info, report })
    }
}

impl Model {
    /// Sets a parameter by its full name, in SI, and re-evaluates the
    /// parameters bound to it (a sub-component's value given by its
    /// parent's parameter). No recompilation: parameters are inputs of the
    /// machine code.
    pub fn set_param(&mut self, name: &str, value: f64) -> Result<(), String> {
        let flat = &self.prepared.flat;
        let id = flat.find_param(name).ok_or_else(|| format!("no parameter '{name}'"))?;
        if flat.params[id.0 as usize].binding.is_some() {
            return Err(format!("'{name}' is bound to another parameter; set that one"));
        }
        self.info.params[id.0 as usize] = value;
        // bindings refer only to parameters created before them (a parent's
        // parameters precede its children's), so one pass in order suffices
        for (i, prm) in flat.params.iter().enumerate() {
            if let Some(b) = &prm.binding {
                let v = lsim_ir::eval::eval(b, &ParamsOnly(&self.info.params));
                self.info.params[i] = v;
            }
        }
        Ok(())
    }

    /// Runs the model over `grid`.
    pub fn simulate(
        &self,
        opts: &SolverOptions,
        grid: OutputGrid,
    ) -> Result<SimResult, SolveError> {
        lsim_solve::simulate(&self.jit, &self.info, opts, grid)
    }
}
