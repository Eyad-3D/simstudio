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
    /// the condition is strict (`x > 0`, `x < 0`): exactly at zero it does
    /// not hold. Otherwise (`x >= 0`, `x <= 0`) it holds there. The zero
    /// crossing is the same either way; Modelica's semantics differ only
    /// at an exact zero (a strict `when x > 0` from x = 0 fires as x
    /// leaves zero, a `when x >= 0` counts as already true)
    #[serde(default)]
    pub strict: bool,
}

impl PreparedWhen {
    /// A `when` clause on zero crossing `crossing` (not strict).
    pub fn new(
        crossing: usize,
        direction: Direction,
        assign: Vec<(VarId, Expr)>,
        origin: Origin,
    ) -> Self {
        PreparedWhen { crossing, direction, assign, origin, strict: false }
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
///   only `when` clauses keeps every mode right between events while the
///   inputs are continuous. Where an input jumps (a prescribed trace's
///   slope at a sample, a sampled block's output at its tick) a crossing
///   that reads it jumps between two steps, where no root is to be found,
///   and one exactly 0 as it starts to move shows no sign change: there
///   the run loop must enter event mode at the jump (fire the clauses
///   whose conditions changed across it, set the modes, iterate) and check
///   every mode against its crossing after each step (DESIGN.md 5.6 and
///   section 10).
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

/// A table read at a state, held in its cell during every step: Park and
/// Barton's discontinuity locking (DESIGN.md §8.2, *Tables read at a
/// state*; on request, `lsim_prep::hold::TableCells`). Where the read (or
/// the derivative of it the model reads) jumps at the table's breakpoints
/// and its argument moves with the states, preparation writes it in the
/// equations as its cell's own function,
/// continued smoothly beyond the cell's edges, with discrete coefficients,
/// and watches the cell's edges as zero crossings; the run sets them for
/// the cell the argument is in at the start and whenever it crosses an
/// edge (an event, after which the integrator restarts), so that no step
/// integrates across a jump.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct HeldRead {
    /// the read as the model wrote it (flat scope, an [`Expr::Table`]):
    /// the table, its arguments and the derivative read
    pub read: Expr,
    /// one per argument of the read, in order
    pub axes: Vec<HeldAxis>,
    /// the polynomials' degree along each argument (their cells' degree,
    /// the derivative read taken off: 1 for a linear table's value, 0 for
    /// its first derivative, 3 for a cubic's value; 0 along a second
    /// argument a 1-D table does not have)
    pub degree: [u8; 2],
    /// the discrete coefficients: [`HeldCells::Points`], that of
    /// `(a₁ − m₁)^i (a₂ − m₂)^j` at `i · (degree[1] + 1) + j`;
    /// [`HeldCells::Edges`], those of the first data cell's polynomial in
    /// powers of `(a − m)` (`m` the axis's `anchor`), then those of the
    /// last's (at [`HeldEdges::anchor_end`]), `degree[0] + 1` each
    pub coef: Vec<VarId>,
    /// where the cells' edges are
    pub cells: HeldCells,
    /// 1 once the run has released the read (DESIGN.md §8.2: its jumps
    /// within the tolerance, or its crossings too frequent): the equations
    /// then read the table itself, stepped over, and its edges are open;
    /// 0 while it is held
    pub released: VarId,
    /// the reads of one table at the same arguments share a group, held
    /// and released together (their edges are crossed at the same
    /// instants); numbered from 0 in the reads' order
    pub group: u32,
    /// whether the read itself jumps where it is held (read differentiated
    /// more often than the table is continuously differentiable there);
    /// false for a read held beside one that does, or held only for
    /// comparison (`TableCells::Kinks`), which the run never releases
    pub jumps: bool,
    /// whether it is held while the run is on the explicit form (no
    /// iteration variable: CVODES). False only for a read that a model whose
    /// states a run chooses again holds because a choice of the implicit form
    /// needs it, which the explicit form steps over by default
    /// (`TableCells::Auto`, DESIGN.md §8.2): every choice of such a model
    /// holds the same reads, so that the run can switch between the forms,
    /// and the run releases these while it is on the explicit form
    pub explicit: bool,
}

/// Where a [`HeldRead`]'s cells have their edges.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub enum HeldCells {
    /// at every breakpoint (the read jumps at the points inside the data):
    /// the read is its cell's polynomial, continued beyond its edges
    Points,
    /// at the data's two edges only (a 1-D table whose read is smooth
    /// enough between its points but jumps where the table is held at its
    /// edges: a monotone cubic's first derivative, `outside = clamp`):
    /// three cells, below, inside and above the data. Inside, the read is
    /// the table's own at the argument held between the edges, continued
    /// beyond each edge by its edge cell's polynomial; outside, the table's
    /// held value
    Edges(HeldEdges),
}

/// The discrete variables of a [`HeldCells::Edges`] read beside its axis's:
/// the read is `T(max(from, min(to, a))) + offset + (P_last(max(a, to)) −
/// P_last(to)) + (P_first(min(a, from)) − P_first(from))`, inside the data
/// `from` and `to` its edges, `offset` 0 and the polynomials its edge
/// cells' (so that it is the table's read there, bitwise); outside, `from`
/// and `to` the edge the argument left by, the polynomials 0 and `offset`
/// the held value less the table's at the edge.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct HeldEdges {
    /// the lower end of where the table is read
    pub from: VarId,
    /// its upper end
    pub to: VarId,
    /// what is added to the table's read
    pub offset: VarId,
    /// the point of the last data cell's polynomial (the first's is the
    /// axis's `anchor`)
    pub anchor_end: VarId,
}

/// An argument of a [`HeldRead`]: the discrete variables that hold its
/// cell, and the zero crossings of the cell's edges.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct HeldAxis {
    /// the cell (its index, as a number: [`HeldCells::Points`], 0 below
    /// the data, i between its points i − 1 and i, n above its last point
    /// n − 1; [`HeldCells::Edges`], 0 below, 1 inside, 2 above; a negative
    /// number before the run sets it)
    pub cell: VarId,
    /// the point of the cell the polynomial is written at
    pub anchor: VarId,
    /// the cell's lower and upper edge (±1e300 where it is open)
    pub lo: VarId,
    /// (see `lo`)
    pub hi: VarId,
    /// the zero crossing `a − lo`, positive inside the cell (index into
    /// [`PreparedModel::zero_crossings`])
    pub below: usize,
    /// the zero crossing `hi − a`
    pub above: usize,
}

impl HeldRead {
    /// Every discrete variable of the read, in a fixed order: per axis its
    /// cell, anchor and edges, then the coefficients, then (edges only)
    /// `from`, `to`, `offset` and `anchor_end`, then `released`.
    pub fn discretes(&self) -> Vec<VarId> {
        let mut v: Vec<VarId> =
            self.axes.iter().flat_map(|a| [a.cell, a.anchor, a.lo, a.hi]).collect();
        v.extend(&self.coef);
        if let HeldCells::Edges(e) = &self.cells {
            v.extend([e.from, e.to, e.offset, e.anchor_end]);
        }
        v.push(self.released);
        v
    }
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

/// A choice index reduction made among states (DESIGN.md §6, step 5):
/// constraints differentiated in time, the highest derivatives they could
/// decide, and the one chosen for each by pivoting at the start (a dummy
/// derivative, which stops the variable it is the derivative of being a
/// state). The choice holds for the whole run; the run watches its pivots
/// and stops, naming the constraint, where it no longer works.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct StateSet {
    /// the constraints (the rows): for each, the equation it was
    /// differentiated from, in the order they were pivoted
    pub rows: Vec<Origin>,
    /// each row's equation in words (`'pendulum' (the rod keeps its
    /// length)`)
    pub words: Vec<String>,
    /// the diagram parts the rows belong to
    pub parts: Vec<String>,
    /// the candidates (the columns): for each, the variable whose
    /// derivative it is (`x` for `der(der(x))`)
    pub candidates: Vec<String>,
    /// rows × columns: each row's derivative by each column, in flat scope
    /// (`Const(0)` where a row does not read a column)
    pub entries: Vec<Vec<Expr>>,
    /// per row, the column chosen for it
    pub pivots: Vec<usize>,
    /// the columns left out of the choice (kept above all: `stateSelect =
    /// always`, a state `reinit` restarts)
    pub masked: Vec<bool>,
    /// per column, whether the model marks it to be kept as a state
    /// (`stateSelect = prefer` or `always`, a state a `reinit` restarts):
    /// while a choice keeps such a column, the marks lead it, and a run
    /// makes it again only where it nears the run's limit (dynamic state
    /// selection)
    ///
    /// This field and the next three have no default: a set read without
    /// them (a prepared model an older engine stored) is not a set a run
    /// can choose again by, and is not read (the engine's cache prepares
    /// the model again).
    pub kept: Vec<bool>,
    /// which set it is: index reduction's equations (the rows) and
    /// candidates (the columns) in its own numbering, the same in every
    /// preparation of the model whatever states it chose (a run that
    /// chooses its states again keeps the other sets by them:
    /// [`crate::choice::KeepSet`])
    pub eqs: Vec<usize>,
    /// (its candidates, in index reduction's own numbering: with `eqs`,
    /// which set it is)
    pub nodes: Vec<usize>,
    /// the equations its rows were differentiated from (index reduction's
    /// numbering, sorted): the sets that share one are the same
    /// constraints at index reduction's other levels, chosen again
    /// together
    pub roots: Vec<usize>,
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
    /// the choices index reduction made among states, which the run
    /// watches (none when nothing was differentiated)
    #[serde(default)]
    pub state_sets: Vec<StateSet>,
    /// the table reads held in their cells during every step (no default:
    /// a model an older engine prepared, which stepped over its tables'
    /// breakpoints, is prepared again)
    pub held: Vec<HeldRead>,
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
                state_select: Default::default(),
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
            state_sets: vec![],
            held: vec![],
        };
        assert_eq!(m.input_names(), ["body.v", "der(body.v)"]);
        assert!(m.init.is_empty());
    }
}
