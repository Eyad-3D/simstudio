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

/// A point of a run to choose the states at.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChoicePoint {
    /// the time
    pub t: f64,
    /// the values by name: every channel, and each state's derivative as
    /// `der(<state>)` (the integrator's)
    pub values: Vec<(String, f64)>,
    /// the parameter values, in the flat system's order
    pub params: Vec<f64>,
    /// the channels that are derivatives (`der(…)`) of the model the run
    /// started with: the model switched to keeps them as variables
    pub channels: Vec<String>,
    /// the factor the run chose again at (its switch threshold): the
    /// pivoting there takes a preferred column only within it of a row's
    /// largest entry, so the choice it leaves is not taken again
    pub window: f64,
}

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
/// share it across threads).
pub struct Rechooser {
    prepare: Box<PrepareAt>,
    compile: Box<Compile>,
    kept: Mutex<Vec<Kept>>,
}

/// A choice compiled: its states, the channels it keeps, the model.
type Kept = (Vec<String>, Vec<String>, Arc<Variant>);

impl Rechooser {
    /// From how to prepare and how to compile.
    pub fn new(prepare: Box<PrepareAt>, compile: Box<Compile>) -> Rechooser {
        Rechooser { prepare, compile, kept: Mutex::new(vec![]) }
    }

    /// The choices made so far (their states).
    pub fn kept(&self) -> Vec<Vec<String>> {
        self.kept.lock().map(|k| k.iter().map(|(s, _, _)| s.clone()).collect()).unwrap_or_default()
    }
}

impl StateChoices for Rechooser {
    fn choose(&self, at: &ChoicePoint) -> Result<Arc<Variant>, String> {
        let prepared = (self.prepare)(at)?;
        let states: Vec<String> =
            prepared.states.iter().map(|v| prepared.flat.var(*v).name.clone()).collect();
        {
            let kept = self.kept.lock().map_err(|_| "a run choosing states failed".to_string())?;
            if let Some((_, _, v)) = kept.iter().find(|(s, c, _)| *s == states && *c == at.channels)
            {
                return Ok(v.clone());
            }
        }
        let model = (self.compile)(&prepared)?;
        let info = RunInfo::from_prepared(&prepared);
        let v = Arc::new(Variant { model, info, states: states.clone() });
        let mut kept = self.kept.lock().map_err(|_| "a run choosing states failed".to_string())?;
        kept.push((states, at.channels.clone(), v.clone()));
        Ok(v)
    }
}
