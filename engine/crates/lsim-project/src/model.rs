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
        let prepared = lsim_prep::prepare(lib, top, opts)?;
        let jit = lsim_codegen::compile(&prepared, &CodegenOptions::default()).map_err(diags)?;
        let mut info = RunInfo::from_prepared(&prepared);
        info.choices = choices(lib, top, opts, &info);
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

    /// A parameter's value by its full name, SI.
    pub fn param(&self, name: &str) -> Option<f64> {
        let id = self.prepared.flat.find_param(name)?;
        self.info.params.get(id.0 as usize).copied()
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

/// Where a run of the model prepared from `top` (its run information
/// `info`) gets the model with its states chosen again (dynamic state
/// selection, DESIGN.md §6 step 5): `top` prepared again with the states
/// chosen at the run's point, compiled, each choice once. `None` where no
/// choice of states the model has moves with its solution (the run then
/// never chooses again).
pub fn choices(
    lib: &Library,
    top: &ComponentDef,
    opts: &PrepOptions,
    info: &RunInfo,
) -> Option<lsim_solve::choice::Choices> {
    use lsim_solve::choice::{Choices, Rechooser};
    if !info.state_sets.iter().any(|s| s.reads == lsim_solve::SetReads::Variables) {
        return None;
    }
    let (lib, top, opts) = (lib.clone(), top.clone(), opts.clone());
    let prepare = move |at: &lsim_solve::choice::ChoicePoint| {
        let point = lsim_prep::ChoosePoint {
            t: at.t,
            values: at.values.iter().cloned().collect(),
            params: at.params.clone(),
            channels: at.channels.clone(),
            window: at.window,
        };
        lsim_prep::prepare_choosing(&lib, &top, &opts, point)
            .map_err(|d| d.iter().map(|x| x.to_string()).collect::<Vec<_>>().join("; "))
    };
    let compile = |p: &PreparedModel| {
        lsim_codegen::compile(p, &CodegenOptions::default())
            .map(|j| Box::new(j) as Box<dyn lsim_ir::runtime::ModelFunctions>)
            .map_err(|e| e.to_string())
    };
    Some(Choices(std::sync::Arc::new(Rechooser::new(Box::new(prepare), Box::new(compile)))))
}
