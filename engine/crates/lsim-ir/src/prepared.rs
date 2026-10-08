//! The prepared model: the flat system after alias elimination, structural
//! analysis (matching, block-lower-triangular sorting, tearing) and index
//! reduction, in the form the code generator compiles.
//!
//! The model is a semi-explicit index-1 DAE in `y = [x; z]`:
//!
//! ```text
//!   x' = f(t, x, z, p, d, u)      (one per state)
//!   0  = g(t, x, z, p, d, u)      (one per iteration variable z)
//! ```
//!
//! where every other unknown is computed, in order, by an explicit
//! [`Assignment`] from what comes before it. Explicitly solvable models have
//! no `z` and are plain ODEs; an algebraic loop that could not be solved
//! symbolically keeps its tearing variables in `z`, so the integrator's own
//! Newton iteration solves them with the step (no nested Newton).

use crate::expr::Expr;
use crate::flat::{FlatSystem, Origin, VarId};
use serde::{Deserialize, Serialize};

/// An unknown of the sorted system: a variable or a state's derivative.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub enum Slot {
    /// a variable
    Var(VarId),
    /// the derivative of a state
    Der(VarId),
}

/// target := expr, computed in order.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Assignment {
    /// what it computes
    pub target: Slot,
    /// from what (flat scope; refers only to states, iteration variables,
    /// parameters, discrete variables, inputs and earlier targets)
    pub expr: Expr,
    /// the equation it was solved from
    pub origin: Origin,
}

/// A residual `0 = expr` that the integrator drives to zero.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Residual {
    /// the residual
    pub expr: Expr,
    /// the equation it is
    pub origin: Origin,
}

/// What an eliminated variable equals.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub enum AliasTarget {
    /// another variable, or its negation
    Var {
        /// the variable kept
        var: VarId,
        /// true: alias = -var
        negated: bool,
    },
    /// a constant
    Const(f64),
}

/// An eliminated variable; its values are still recorded under its name.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct AliasEntry {
    /// the eliminated variable
    pub var: VarId,
    /// what it equals
    pub target: AliasTarget,
}

/// Which way a zero crossing must go to fire.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Direction {
    /// from negative to non-negative
    Rising,
    /// from positive to non-positive
    Falling,
    /// either way
    Both,
}

/// A zero-crossing function: the solver locates its sign changes exactly.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct ZeroCrossing {
    /// the function (flat scope), e.g. `w - w_on`
    pub expr: Expr,
    /// where it came from
    pub origin: Origin,
}

/// A `when` clause, tied to its zero crossing.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct PreparedWhen {
    /// index into [`PreparedModel::zero_crossings`]
    pub crossing: usize,
    /// which way it must cross
    pub direction: Direction,
    /// discrete variable := value
    pub assign: Vec<(VarId, Expr)>,
    /// where it came from
    pub origin: Origin,
}

/// A relation of an `if` expression held as a discrete Boolean between
/// events (DESIGN.md, *Events and modes*): the equations read the held
/// value (`if m then … else …`, `m` a discrete variable), so the
/// integrator never sees the discontinuity; the run loop stops where the
/// relation's zero crossing changes sign and re-evaluates the relation.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Mode {
    /// the discrete variable holding the relation's truth value (1 or 0);
    /// one of [`PreparedModel::discretes`]
    pub var: VarId,
    /// the relation (flat scope), e.g. `w > 0`, evaluated at events to set
    /// `var`
    pub relation: Expr,
    /// its zero-crossing function's index in [`PreparedModel::zero_crossings`]
    pub crossing: usize,
    /// where it came from
    pub origin: Origin,
}

/// The initialisation problem (DESIGN.md, *Initialisation*): the
/// equations at the start time with the `fixed` start values and the
/// initial equations, sorted like the model itself. Newton iterates on
/// `unknowns` (the initialisation's own iteration variables `w`) until the
/// residuals vanish; everything else is computed by the assignments, in
/// order. A variable the system leaves alone holds its start value.
/// Empty: the start values are consistent as they stand.
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
pub struct InitSystem {
    /// w: what Newton iterates on, in order
    pub unknowns: Vec<Slot>,
    /// explicit assignments, in evaluation order (flat scope; they refer
    /// to `unknowns`, parameters, discrete variables, inputs, earlier
    /// targets and start values)
    pub assignments: Vec<Assignment>,
    /// the residuals, one per unknown
    pub residuals: Vec<Residual>,
}

impl InitSystem {
    /// Whether there is nothing to solve or compute.
    pub fn is_empty(&self) -> bool {
        self.unknowns.is_empty() && self.assignments.is_empty() && self.residuals.is_empty()
    }
}

/// Where an external sampled block (a [`crate::runtime::DiscreteBlock`])
/// sits in the model.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct ExternalBlock {
    /// the instance it is (its path names the host's implementation)
    pub instance: crate::flat::InstanceId,
    /// the variables it reads at each tick
    pub inputs: Vec<VarId>,
    /// the discrete variables it sets
    pub outputs: Vec<VarId>,
    /// its period, s (a parameter's value at preparation)
    pub period: f64,
}

/// What fast mode prescribes and frees (DESIGN.md, *Fast mode*): the
/// contract between the inverse-model preparation (lsim-prep) and the
/// fast-mode stepper (lsim-fast).
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
pub struct InverseSpec {
    /// variables (full flat names) whose values follow a given trajectory,
    /// usually the vehicle body's speed; they and their derivatives become
    /// known inputs
    pub prescribed: Vec<String>,
    /// signal inputs (full flat names) that become unknowns, usually the
    /// driver's commands
    pub freed: Vec<String>,
}

/// Counts that describe the preparation, for the run report.
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct PrepStats {
    /// variables after flattening
    pub flat_vars: usize,
    /// equations after flattening
    pub flat_equations: usize,
    /// variables removed as aliases
    pub aliases: usize,
    /// blocks of the block-lower-triangular form
    pub blocks: usize,
    /// the largest block
    pub largest_block: usize,
    /// equations solved explicitly
    pub explicit: usize,
}

/// The prepared model.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct PreparedModel {
    /// the flat system (names, units, origins, parameters)
    pub flat: FlatSystem,
    /// x: the states, in order
    pub states: Vec<VarId>,
    /// z: the iteration variables, in order
    pub algebraics: Vec<Slot>,
    /// d: the discrete variables
    pub discretes: Vec<VarId>,
    /// u: inputs set from outside (prescribed trajectories, and their
    /// derivatives in an inverse model)
    pub inputs: Vec<VarId>,
    /// sampled blocks run outside the equations
    pub external: Vec<ExternalBlock>,
    /// the explicit assignments, in evaluation order
    pub assignments: Vec<Assignment>,
    /// the residuals g, one per iteration variable
    pub residuals: Vec<Residual>,
    /// eliminated variables
    pub aliases: Vec<AliasEntry>,
    /// zero-crossing functions
    pub zero_crossings: Vec<ZeroCrossing>,
    /// `when` clauses
    pub whens: Vec<PreparedWhen>,
    /// hex digest of everything that shapes the generated code (equations,
    /// structural parameter values, sizes) but not the values of runtime
    /// parameters: the compiled-code cache key
    pub structure_key: String,
    /// counts for the report
    pub stats: PrepStats,
    /// the structure of `∂[x'; g]/∂y` through the assignments (empty, with
    /// `n` = 0, when not computed: the code generator then derives it from
    /// the equations itself)
    #[serde(default)]
    pub jac_pattern: crate::runtime::SparsityPattern,
    /// the initialisation problem (empty: the start values hold as they
    /// are)
    #[serde(default)]
    pub init: InitSystem,
    /// the `if` relations held as discrete Booleans between events
    #[serde(default)]
    pub modes: Vec<Mode>,
}
