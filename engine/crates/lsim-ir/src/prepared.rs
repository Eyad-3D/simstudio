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
use crate::runtime::SparsityPattern;
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

impl PreparedWhen {
    /// A `when` clause on zero crossing `crossing`.
    pub fn new(
        crossing: usize,
        direction: Direction,
        assign: Vec<(VarId, Expr)>,
        origin: Origin,
    ) -> Self {
        PreparedWhen { crossing, direction, assign, origin }
    }
}

/// A mode (DESIGN.md, *Events and modes*): the held truth value of one
/// relation of the equations (an `if` condition, or the sign test inside
/// `abs` or `sign`) outside `noEvent`. Between events the equations read
/// the discrete variable `var` (1 true, 0 false) instead of the relation,
/// so the integrator never sees a discontinuity.
///
/// The contract between preparation, the code generator and the run loop:
///
/// * `zero_crossings[crossing]` is positive where the relation holds and
///   negative where it does not (`lhs - rhs` for `>` and `>=`, `rhs - lhs`
///   for `<` and `<=`), so away from its zero `var = 1` exactly when the
///   crossing is positive, and a run loop may flip `var` with the
///   crossing's sign;
/// * at the start and after every event the run loop sets `var` from the
///   relation itself ([`crate::ModelFunctions::modes`] evaluates every
///   mode's relation), which also decides the value exactly at zero
///   (`>=` holds there, `>` does not);
/// * preparation also adds two `when` clauses per mode: a rising one on
///   `crossing` itself that sets `var` to 1, and a falling one on a copy
///   of it at `crossing + 1` that sets `var` to 0; so a run loop that knows
///   only `when` clauses keeps every mode right between events.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Mode {
    /// the discrete variable holding the relation's value (1 true, 0
    /// false); one of [`PreparedModel::discretes`]
    pub var: VarId,
    /// the relation as written (flat scope, a comparison), evaluated as it
    /// stands
    pub relation: Expr,
    /// the index of its zero-crossing function in
    /// [`PreparedModel::zero_crossings`]: positive where the relation holds
    pub crossing: usize,
    /// where it came from
    pub origin: Origin,
}

/// The initialisation system (DESIGN.md, *Initialisation*): it computes a
/// consistent start of a run from the parameters (and the inputs at the
/// start time) with its own sorted equations — the model's equations,
/// including those index reduction differentiated, its initial equations
/// and the start values that must hold — solved by Newton on `unknowns`
/// with the assignments explicit in between, as the model itself is.
///
/// After a solve every slot of the model's `y` (each state `Var(x)` and
/// each iteration variable) has a value, being either one of `unknowns` or
/// the target of one of `assignments`; so does every state's derivative.
/// Relations are evaluated as they stand (no mode is held yet).
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
pub struct InitSystem {
    /// the Newton unknowns (the tearing variables of the start problem)
    pub unknowns: Vec<Slot>,
    /// a first guess for each unknown: an expression of the parameters
    pub guesses: Vec<Expr>,
    /// explicit assignments in evaluation order; they read the unknowns,
    /// parameters, inputs, discrete start values and earlier targets
    pub assignments: Vec<Assignment>,
    /// one residual per unknown
    pub residuals: Vec<Residual>,
    /// the start value of each discrete variable, in
    /// [`PreparedModel::discretes`] order: an expression of the parameters,
    /// except for a mode's variable, whose start is its relation evaluated
    /// at the solution
    pub discrete_starts: Vec<Expr>,
}

impl InitSystem {
    /// Whether there is nothing to solve or compute: the start values hold
    /// as they are.
    pub fn is_empty(&self) -> bool {
        self.unknowns.is_empty() && self.assignments.is_empty() && self.residuals.is_empty()
    }
}

/// Where an external sampled block (a [`crate::runtime::DiscreteBlock`])
/// sits in the model. A part whose definition's name begins with
/// `External.` is one: it has no equations, its signal outputs are
/// discrete variables the host sets at each tick, its signal inputs are
/// read at each tick, and its parameter `period` is the tick spacing.
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

/// A `limit(value, lo, hi)` of an inverse model (DESIGN.md, *Fast mode*):
/// the inverse model passes `value` through instead of clamping it, and the
/// fast-mode stepper flags each stretch of time it is outside `[lo, hi]`.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct LimitSite {
    /// the limited quantity (flat scope)
    pub value: Expr,
    /// its lower bound
    pub lo: Expr,
    /// its upper bound
    pub hi: Expr,
    /// the equation (and so the part) that holds the limit
    pub origin: Origin,
}

/// A condition on the parameters that preparation relied on: an equation
/// was solved explicitly by dividing by `expr`, an expression of the
/// parameters only, which therefore must not be zero. A parameter change
/// that makes it zero needs a new preparation (the equation then becomes
/// implicit); the run checks the guards whenever parameters change.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct ParamGuard {
    /// must not be zero (flat scope, parameters only)
    pub expr: Expr,
    /// the equation that was solved by dividing by it
    pub origin: Origin,
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
    /// u: inputs set from outside. In an inverse model (fast mode), for
    /// each variable of [`InverseSpec::prescribed`] in its order: the
    /// variable, then its time derivatives as deep as the model needs
    /// them (`der(body.v)`, and `der(der(body.v))` where index reduction
    /// differentiated twice); each is a flat variable of that name
    /// ([`PreparedModel::input_names`])
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
    /// the structural sparsity of `∂[x'; g]/∂y` through the assignments
    /// (rows and columns in `y = [x; z]` order), for colouring and sparse
    /// LU; empty (`n` = 0) when not computed, and the code generator then
    /// derives it from the equations itself
    #[serde(default)]
    pub jac_pattern: SparsityPattern,
    /// the modes of `if` relations (each one's variable is among
    /// `discretes`)
    #[serde(default)]
    pub modes: Vec<Mode>,
    /// the initialisation system
    #[serde(default)]
    pub init: InitSystem,
    /// inverse models only: every `limit` of the equations, passed through
    /// and to be flagged (a forward model clamps and lists none)
    #[serde(default)]
    pub limits: Vec<LimitSite>,
    /// the parameter expressions explicit solutions divide by
    #[serde(default)]
    pub guards: Vec<ParamGuard>,
    /// what preparation noticed that does not stop the model from running
    /// (a start value that cannot hold, a loop through controllers …),
    /// told like every other diagnostic
    #[serde(default)]
    pub warnings: Vec<crate::diag::Diagnostic>,
}

impl PreparedModel {
    /// The inputs' names, in [`PreparedModel::inputs`] order: what the
    /// caller fills `u` from (an inverse model's prescribed variables and
    /// their derivatives, `body.v`, `der(body.v)` …).
    pub fn input_names(&self) -> Vec<String> {
        self.inputs.iter().map(|v| self.flat.var(*v).name.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component::VarKind;
    use crate::flat::{FlatVar, InstanceId, VarRole};
    use crate::units::Unit;

    #[test]
    fn inputs_are_named_by_their_flat_variables() {
        let mut flat = FlatSystem::default();
        for name in ["body.v", "der(body.v)", "x"] {
            flat.vars.push(FlatVar {
                name: name.into(),
                unit: Unit::ONE,
                unit_text: "1".into(),
                kind: VarKind::Continuous,
                start: None,
                fixed: false,
                nominal: 1.0,
                instance: InstanceId(0),
                role: VarRole::Local,
            });
        }
        let m = PreparedModel {
            flat,
            states: vec![VarId(2)],
            algebraics: vec![],
            discretes: vec![],
            inputs: vec![VarId(0), VarId(1)],
            external: vec![],
            assignments: vec![],
            residuals: vec![],
            aliases: vec![],
            zero_crossings: vec![],
            whens: vec![],
            structure_key: String::new(),
            stats: PrepStats::default(),
            jac_pattern: SparsityPattern::default(),
            modes: vec![],
            init: InitSystem::default(),
            limits: vec![],
            guards: vec![],
            warnings: vec![],
        };
        assert_eq!(m.input_names(), ["body.v", "der(body.v)"]);
        assert!(m.init.is_empty());
    }
}
