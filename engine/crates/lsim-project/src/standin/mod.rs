//! Stand-ins for what work packages 2–4 are building, so the library and
//! the importer can be checked end to end now (exact answers, energy
//! books, golden comparisons). Each is the simplest correct version and is
//! meant to be replaced, not grown:
//!
//! | stand-in | replaced by |
//! |---|---|
//! | [`prep`]: relations under `noEvent`, index reduction of state constraints (one differentiation), energy meters as states, pivot check | WP2 (modes, Pantelides with dummy derivatives, tearing), WP4 (energy quadratures) |
//! | [`run`]: event iteration, impulse re-initialisation at constraint changes, sampled blocks, stop condition | WP4 (run loop), WP6 (front door) |
//! | tables expanded into expressions (`lsim_lib::table`) | WP1/WP3 runtime tables |

pub mod prep;
pub mod run;

use lsim_codegen::{CodegenOptions, JitModel};
use lsim_ir::eval::Env;
use lsim_ir::{ComponentDef, Diagnostic, Library, ParamId, VarId};
pub use prep::{Options, Prepared};
pub use run::{RunResult, RunSpec, Sampled};

/// A model prepared and compiled with the stand-ins.
pub struct Built {
    /// the prepared model and its constraints, meters, stored energies
    pub prep: Prepared,
    /// its machine code
    pub jit: JitModel,
    /// parameter values, SI
    pub params: Vec<f64>,
}

struct Params<'a>(&'a [f64]);
impl Env for Params<'_> {
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

impl Built {
    /// Prepares and compiles `top`.
    pub fn new(lib: &Library, top: &ComponentDef, o: &Options) -> Result<Built, Vec<Diagnostic>> {
        let prep = prep::prepare(lib, top, o)?;
        let jit = lsim_codegen::compile(&prep.model, &CodegenOptions::default())
            .map_err(|e| vec![Diagnostic::error("CODEGEN", e.to_string())])?;
        let params = prep.model.flat.params.iter().map(|p| p.value).collect();
        Ok(Built { prep, jit, params })
    }

    /// Sets a parameter by its full name (SI) and re-evaluates the ones
    /// bound to it.
    pub fn set_param(&mut self, name: &str, value: f64) -> Result<(), String> {
        let flat = &self.prep.model.flat;
        let id = flat.find_param(name).ok_or_else(|| format!("no parameter '{name}'"))?;
        self.params[id.0 as usize] = value;
        for (i, prm) in flat.params.iter().enumerate() {
            if let Some(b) = &prm.binding {
                self.params[i] = lsim_ir::eval::eval(b, &Params(&self.params));
            }
        }
        Ok(())
    }

    /// A flat variable by name.
    pub fn var(&self, name: &str) -> Option<VarId> {
        self.prep.model.flat.find_var(name)
    }

    /// Runs it.
    pub fn run(&self, spec: &RunSpec, blocks: &mut [Sampled]) -> Result<RunResult, String> {
        run::simulate(&self.prep, &self.jit, &self.params, spec, blocks)
    }
}
