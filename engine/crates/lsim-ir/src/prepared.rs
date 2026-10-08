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

impl InverseSpec {
    /// The names of an inverse model's inputs, in the order of
    /// [`PreparedModel::inputs`]: for each prescribed variable, its value
    /// and then its time derivative (`body.v`, `der(body.v)`, …). The
    /// fast-mode stepper (lsim-fast) fills them in this order from one
    /// piecewise-linear trace per prescribed variable.
    pub fn input_names(&self) -> Vec<String> {
        self.prescribed.iter().flat_map(|p| [p.clone(), format!("der({p})")]).collect()
    }
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_inverse_models_inputs_are_each_value_then_its_derivative() {
        let spec = InverseSpec {
            prescribed: vec!["body.v".into(), "road.h".into()],
            freed: vec!["motor.tau_dem".into()],
        };
        assert_eq!(spec.input_names(), ["body.v", "der(body.v)", "road.h", "der(road.h)"]);
    }
}
