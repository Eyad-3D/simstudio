//! Index reduction's choice of states made again during a run: dynamic
//! state selection (DESIGN.md §6 step 5).
//!
//! Preparation chooses the states once, at the start, by pivoting on the
//! constraints' derivatives there ([`lsim_ir::StateSet`]). Along the
//! motion the choice can degrade: a decided variable comes to move far
//! faster than the states it is decided from, and a little further on is
//! not decided by them at all (a pendulum in x and y whose x is decided
//! from y, at the bottom of its swing). The run watches the choice
//! ([`crate::states`]); where it passes the switch threshold
//! ([`crate::SolverOptions::state_switch`]), the run asks the model's
//! [`StateChoices`] for the model with the states chosen again at that
//! point, by the same pivoting and the same `stateSelect` marks as at the
//! start, and goes on with it: an event at which the integrator restarts
//! on the new states from the solution there (the constraints and their
//! derivatives hold, the new iteration variables solved again with the new
//! states held). Every channel of the model the run started with stays a
//! channel of the one it switches to (matched by name), so the results,
//! the energy books and the events read on as one run.
//!
//! The models to switch to are prepared and compiled where a run first
//! needs them and kept for the model's later runs ([`Rechooser`]): a run
//! whose states never degrade does exactly what it did without dynamic
//! selection, bit for bit.

use crate::RunInfo;
use lsim_ir::PreparedModel;
use lsim_ir::runtime::ModelFunctions;
use std::sync::{Arc, Mutex};

pub use lsim_ir::choice::{ChoicePoint, KeepSet};

/// A model with the states chosen again.
pub struct Variant {
    /// its compiled functions
    pub model: Box<dyn ModelFunctions>,
    /// what the run loop needs to know about it (the parameter values are
    /// the prepared ones: a run sets its own)
    pub info: RunInfo,
    /// its states, by name, in order (what identifies the choice)
    pub states: Vec<String>,
}

impl std::fmt::Debug for Variant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Variant").field("states", &self.states).finish_non_exhaustive()
    }
}

/// Where a run gets the model with its states chosen again.
pub trait StateChoices: Send + Sync {
    /// The model with index reduction's states chosen at `at`, or why
    /// there is none (no regular choice there: the parts in a singular
    /// position).
    fn choose(&self, at: &ChoicePoint) -> Result<Arc<Variant>, String>;
}

/// A model's [`StateChoices`], as [`RunInfo::choices`] carries it.
#[derive(Clone)]
pub struct Choices(pub Arc<dyn StateChoices>);

impl std::fmt::Debug for Choices {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Choices(..)")
    }
}

/// How a model is prepared with its states chosen at a point.
pub type PrepareAt = dyn Fn(&ChoicePoint) -> Result<PreparedModel, String> + Send + Sync;

/// How a prepared model is compiled.
pub type Compile = dyn Fn(&PreparedModel) -> Result<Box<dyn ModelFunctions>, String> + Send + Sync;

/// [`StateChoices`] by preparing the model again with its states chosen
/// at the point (`prepare`) and compiling it (`compile`); each choice is
/// compiled once and kept, for this and later runs of the model (sweeps
/// share it across threads), the [`Rechooser::KEPT`] used last.
pub struct Rechooser {
    prepare: Box<PrepareAt>,
    compile: Box<Compile>,
    /// the choices kept, the one used last at the end
    kept: Mutex<Vec<Kept>>,
}

/// A choice compiled: its states, the channels it keeps, the model.
type Kept = (Vec<String>, Vec<String>, Arc<Variant>);

impl Rechooser {
    /// How many choices it keeps: a choice used less recently than the
    /// last this many is compiled again where a run takes it again (a run
    /// keeps every model it went on with for its own length).
    pub const KEPT: usize = 64;

    /// From how to prepare and how to compile.
    pub fn new(prepare: Box<PrepareAt>, compile: Box<Compile>) -> Rechooser {
        Rechooser { prepare, compile, kept: Mutex::new(vec![]) }
    }

    /// The choices kept (their states), the one used last at the end.
    pub fn kept(&self) -> Vec<Vec<String>> {
        self.kept.lock().map(|k| k.iter().map(|(s, _, _)| s.clone()).collect()).unwrap_or_default()
    }

    /// The choice `states` keeping the channels `channels`, where it is
    /// kept (made the one used last).
    fn take(kept: &mut Vec<Kept>, states: &[String], channels: &[String]) -> Option<Arc<Variant>> {
        let i = kept.iter().position(|(s, c, _)| s == states && c == channels)?;
        let found = kept.remove(i);
        let v = found.2.clone();
        kept.push(found);
        Some(v)
    }
}

impl StateChoices for Rechooser {
    fn choose(&self, at: &ChoicePoint) -> Result<Arc<Variant>, String> {
        let prepared = (self.prepare)(at)?;
        let states: Vec<String> =
            prepared.states.iter().map(|v| prepared.flat.var(*v).name.clone()).collect();
        let lock = || self.kept.lock().map_err(|_| "a run choosing states failed".to_string());
        if let Some(v) = Rechooser::take(&mut *lock()?, &states, &at.channels) {
            return Ok(v);
        }
        // (compiled outside the lock: other runs of a sweep go on meanwhile)
        let model = (self.compile)(&prepared)?;
        let info = RunInfo::from_prepared(&prepared);
        let v = Arc::new(Variant { model, info, states: states.clone() });
        let mut kept = lock()?;
        // (another run kept the same choice meanwhile: that one, so each
        // choice is kept once)
        if let Some(other) = Rechooser::take(&mut kept, &states, &at.channels) {
            return Ok(other);
        }
        kept.push((states, at.channels.clone(), v.clone()));
        if kept.len() > Rechooser::KEPT {
            kept.remove(0);
        }
        Ok(v)
    }
}

/// The [`Choices`] of a model whose run information is `info`: the model
/// prepared again with its states chosen at a run's point (`prepare`),
/// compiled (`compile`), each choice once ([`Rechooser`]). `None` where no
/// set of its choice moves with its solution (a run then never chooses
/// again). The engine's builders give every model theirs by it.
pub fn rechoosing(
    info: &RunInfo,
    prepare: Box<PrepareAt>,
    compile: Box<Compile>,
) -> Option<Choices> {
    if !info.state_sets.iter().any(|s| s.reads == crate::SetReads::Variables) {
        return None;
    }
    Some(Choices(Arc::new(Rechooser::new(prepare, compile))))
}
