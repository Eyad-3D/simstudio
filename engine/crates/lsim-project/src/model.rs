//! A model built with the engine's own pipeline — preparation
//! (`lsim-prep`), code generation (`lsim-codegen`) and the solver
//! (`lsim-solve`) — for the importer's tests, the exact-answer cases and
//! the golden comparison. (The front door with its cache is work package
//! 6's `lsim-engine`; this is the plain path, nothing cached.)

use lsim_codegen::{CodegenOptions, JitModel};
use lsim_ir::eval::Env;
use lsim_ir::runtime::DiscreteBlock;
use lsim_ir::{ComponentDef, Diagnostic, Library, ParamId, PreparedModel, VarId};
use lsim_prep::PrepOptions;
use lsim_solve::{OutputGrid, RunInfo, SimResult, SolveError, SolverOptions};

/// A prepared and compiled model with its run information.
pub struct Model {
    /// the prepared model
    pub prepared: PreparedModel,
    /// its machine code
    pub jit: JitModel,
    /// what the run loop needs (parameter values among it)
    pub info: RunInfo,
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

/// A variable kept by alias elimination takes the start value (a guess) of
/// a variable eliminated in its favour when it has none of its own (or
/// only 0, which preparation writes where its own start solve did not
/// converge): the
/// battery's terminal voltage guess then reaches the bus voltage its node's
/// port carries. Without it the initialisation starts the bus at 0 V, where
/// `v·i = P` gives no current (a singular Jacobian). Preparation should do
/// this itself (work package 2); until then it is done here.
pub fn carry_alias_starts(m: &mut PreparedModel) {
    use lsim_ir::prepared::AliasTarget;
    let mut given: Vec<(VarId, f64)> = vec![];
    for a in &m.aliases {
        if let AliasTarget::Var { var, negated } = a.target
            && m.flat.var(var).start.is_none_or(|s| s == 0.0)
            && let Some(s) = m.flat.var(a.var).start
            && s != 0.0
            && !given.iter().any(|(v, _)| *v == var)
        {
            given.push((var, if negated { -s } else { s }));
        }
    }
    for (v, s) in given {
        m.flat.vars[v.0 as usize].start = Some(s);
    }
}

fn diags(e: impl std::fmt::Display) -> Vec<Diagnostic> {
    vec![Diagnostic::error("CODEGEN", e.to_string())]
}

impl Model {
    /// Prepares and compiles `top` against `lib`.
    pub fn build(lib: &Library, top: &ComponentDef) -> Result<Model, Vec<Diagnostic>> {
        Model::build_with(lib, top, &PrepOptions::default())
    }

    /// [`Model::build`] with preparation options.
    pub fn build_with(
        lib: &Library,
        top: &ComponentDef,
        opts: &PrepOptions,
    ) -> Result<Model, Vec<Diagnostic>> {
        let mut prepared = lsim_prep::prepare(lib, top, opts)?;
        carry_alias_starts(&mut prepared);
        let jit = lsim_codegen::compile(&prepared, &CodegenOptions::default()).map_err(diags)?;
        let info = RunInfo::from_prepared(&prepared);
        Ok(Model { prepared, jit, info })
    }

    /// Sets a parameter by its full name, SI, and re-evaluates the ones
    /// bound to it.
    pub fn set_param(&mut self, name: &str, value: f64) -> Result<(), String> {
        let flat = &self.prepared.flat;
        let id = flat.find_param(name).ok_or_else(|| format!("no parameter '{name}'"))?;
        self.info.params[id.0 as usize] = value;
        for (i, prm) in flat.params.iter().enumerate() {
            if let Some(b) = &prm.binding {
                self.info.params[i] = lsim_ir::eval::eval(b, &Params(&self.info.params));
            }
        }
        Ok(())
    }

    /// A flat variable by its full name.
    pub fn var(&self, name: &str) -> Option<VarId> {
        self.prepared.flat.find_var(name)
    }

    /// Runs it over `grid`, driving the sampled `blocks` (one per
    /// `info.blocks` entry, in order).
    pub fn run(
        &self,
        opts: &SolverOptions,
        grid: OutputGrid,
        blocks: &mut [Box<dyn DiscreteBlock>],
    ) -> Result<SimResult, SolveError> {
        lsim_solve::simulate(&self.jit, &self.info, opts, grid, blocks)
    }
}
