//! Tables read at a state, held in their cells during every step (DESIGN.md
//! §8.2, *Tables read at a state*): Park and Barton's discontinuity locking
//! (ACM TOMACS 6(2), 1996).
//!
//! Where a table read in the run's equations has a kink or a jump at its
//! breakpoints (its value where the table is only continuous: a linear
//! table, a table held at its edges; or a derivative index reduction reads
//! one order more often than the table is smooth) and its argument moves
//! with the states, the integrator would step across the breakpoint as
//! over any piecewise-smooth right-hand side. Measured on exact answers
//! (`engine/judge`, the `kink-…` cases), that error is not proportional to
//! the tolerance: a jump read through index reduction (a gear whose ratio
//! is a linear table of the load's speed) ends up to 178 rtol off where
//! the same function with its breakpoints located is within 3 (5 261 in a
//! sweep of a cubic held at its edges), and many kinks of a value read (a
//! spring whose force is a linear table of its position, a table of two
//! states) cost up to 5 or 6 times the located run's error. So the read is replaced in the equations by its cell's
//! polynomial in powers of `(a − m)`, its coefficients and the point `m`
//! discrete variables, and the cell's two edges become zero crossings
//! (`a − lo`, `hi − a`, positive inside); the run loop sets them for the
//! cell the argument lies in at the start and at every crossing of an edge
//! (an event: the integrator restarts there), from the table the model
//! interpolates (its data may be swapped after the model was built: the
//! coefficients come from the model's own tables, not from here). Inside
//! its cell the polynomial is the table, to rounding; across an edge it is
//! the cell's own polynomial continued, so every step integrates a smooth
//! right-hand side and the crossing is found by root finding.
//!
//! Not held: a read whose argument moves with time alone (the run stops at
//! its breakpoints, `RunInfo::time_tables`) or not at all between events
//! (parameters and discrete values); the reads of the initialisation
//! system, of `when` clauses and of zero crossings (none integrates them);
//! a table smooth enough for the read (a monotone cubic's value, continued
//! linearly outside: C¹); an inverse model (fast mode, which does not
//! integrate), and a model with no state.

use lsim_ir::component::VarKind;
use lsim_ir::flat::{FlatVar, VarRole};
use lsim_ir::units::Unit;
use lsim_ir::{
    Expr, HeldAxis, HeldRead, Interpolation, Origin, PreparedModel, Slot, VarId, ZeroCrossing,
};
use std::collections::{BTreeMap, HashMap, HashSet};

/// An open edge of a cell (below the data's first point, above its last):
/// finite, so that the zero crossings stay numbers.
pub const OPEN: f64 = 1e300;

/// Holds the table reads of `model`'s assignments and residuals that kink
/// or jump at their breakpoints ([`HeldRead`]). The reads are made in the
/// order of their keys (the table, the derivative and the arguments, an
/// inner held read by its own key), so that every preparation of a model
/// (one that chooses its states again at a point of a run, `lsim_prep::
/// prepare_choosing`) holds the same reads in the same discrete variables
/// and zero crossings.
pub fn apply(model: &mut PreparedModel) {
    if model.states.is_empty() {
        return;
    }
    let assigned: HashMap<VarId, Expr> = model
        .assignments
        .iter()
        .filter_map(|a| match a.target {
            Slot::Var(v) => Some((v, a.expr.clone())),
            Slot::Der(_) => None,
        })
        .collect();
    let mut solved: HashSet<VarId> = model.states.iter().copied().collect();
    solved.extend(model.algebraics.iter().filter_map(|s| match s {
        Slot::Var(v) => Some(*v),
        Slot::Der(_) => None,
    }));
    let mut st = State { reads: BTreeMap::new(), assigned, solved, moving: HashMap::new() };
    // every expression with its held reads as names (`held:<key>`)
    let named_a: Vec<Expr> = (0..model.assignments.len())
        .map(|i| {
            let origin = model.assignments[i].origin.clone();
            named(model.assignments[i].expr.clone(), model, &mut st, &origin)
        })
        .collect();
    let named_r: Vec<Expr> = (0..model.residuals.len())
        .map(|i| {
            let origin = model.residuals[i].origin.clone();
            named(model.residuals[i].expr.clone(), model, &mut st, &origin)
        })
        .collect();
    if st.reads.is_empty() {
        return;
    }
    // the held reads' variables and zero crossings, in their keys' order
    let keys: Vec<String> = st.reads.keys().cloned().collect();
    let index: HashMap<String, usize> =
        keys.iter().enumerate().map(|(k, key)| (key.clone(), k)).collect();
    let mut counts: HashMap<u32, usize> = HashMap::new();
    let mut held = vec![];
    for key in &keys {
        held.push(hold(model, &st.reads[key], &mut counts));
    }
    // the names as the cells' polynomials, the zero crossings' arguments
    let mut args: Vec<Option<Vec<Expr>>> = vec![None; keys.len()];
    let mut ex =
        Expand { keys: &keys, index: &index, reads: &st.reads, held: &held, args: &mut args };
    for (a, e) in model.assignments.iter_mut().zip(named_a) {
        a.expr = ex.expand(e);
    }
    for (r, e) in model.residuals.iter_mut().zip(named_r) {
        r.expr = ex.expand(e);
    }
    let all: Vec<Vec<Expr>> = (0..keys.len()).map(|k| ex.args_of(k)).collect();
    for (h, a) in held.iter_mut().zip(all) {
        for (j, ax) in h.axes.iter().enumerate() {
            model.zero_crossings[ax.below].expr =
                Expr::bin(lsim_ir::BinaryOp::Sub, a[j].clone(), Expr::Var(ax.lo));
            model.zero_crossings[ax.above].expr =
                Expr::bin(lsim_ir::BinaryOp::Sub, Expr::Var(ax.hi), a[j].clone());
        }
        let Expr::Table { table, order, .. } = h.read else { unreachable!("a table read") };
        h.read = Expr::Table { table, args: a, order };
    }
    model.held = held;
}

/// A read to hold: its table, derivative, arguments (inner held reads as
/// names) and the equation it was first found in.
struct Pending {
    table: u32,
    order: [u8; 2],
    args: Vec<Expr>,
    origin: Origin,
}

struct State {
    /// the reads to hold, by key
    reads: BTreeMap<String, Pending>,
    /// what each computed variable is computed from (the assignments as
    /// preparation made them)
    assigned: HashMap<VarId, Expr>,
    /// the states and iteration variables
    solved: HashSet<VarId>,
    /// per computed variable: whether it moves with them (memoised)
    moving: HashMap<VarId, bool>,
}

/// The name a held read stands as until its polynomial is written.
const NAME: &str = "held:";

/// Whether `e` moves with the states between events: through the
/// assignments it reads a state, an iteration variable, a derivative or a
/// held read. Not one that moves with time alone (a driving cycle's
/// position: the run stops at its breakpoints), nor one of parameters and
/// discrete values.
fn moves(e: &Expr, st: &mut State, depth: usize) -> bool {
    match e {
        Expr::Der(_) => true,
        Expr::Name(n) => n.starts_with(NAME),
        Expr::Var(v) if st.solved.contains(v) => true,
        Expr::Var(v) => {
            if let Some(&m) = st.moving.get(v) {
                return m;
            }
            let m = match st.assigned.get(v).cloned() {
                Some(def) if depth < 256 => moves(&def, st, depth + 1),
                Some(_) => true,
                // (a discrete value or an input)
                None => false,
            };
            st.moving.insert(*v, m);
            m
        }
        _ => e.children().iter().any(|c| moves(c, st, depth)),
    }
}

/// `e` with each read to hold as its name, innermost first.
fn named(e: Expr, model: &PreparedModel, st: &mut State, origin: &Origin) -> Expr {
    e.rewrite(&mut |node| {
        let Expr::Table { table, args, order } = &node else { return node };
        let data = &model.flat.tables[*table as usize].data;
        let dims = data.dims();
        let points = [data.x.len(), data.y.len()];
        let smooth = lsim_ir::interp::continuity(data);
        let read = u32::from(order[0]) + u32::from(order[1]);
        if smooth == u32::MAX
            || read < smooth
            || args.len() != dims
            || points[..dims].iter().any(|&n| n < 2)
            || !args.iter().any(|a| moves(a, st, 0))
        {
            return node;
        }
        let key = format!("{table}:{order:?}:{args:?}");
        st.reads.entry(key.clone()).or_insert_with(|| Pending {
            table: *table,
            order: *order,
            args: args.clone(),
            origin: origin.clone(),
        });
        Expr::Name(format!("{NAME}{key}"))
    })
}

/// The names of held reads written as their cells' polynomials.
struct Expand<'a> {
    keys: &'a [String],
    index: &'a HashMap<String, usize>,
    reads: &'a BTreeMap<String, Pending>,
    held: &'a [HeldRead],
    /// each read's arguments, written (memoised)
    args: &'a mut Vec<Option<Vec<Expr>>>,
}

impl Expand<'_> {
    fn args_of(&mut self, k: usize) -> Vec<Expr> {
        if let Some(a) = &self.args[k] {
            return a.clone();
        }
        let named = self.reads[&self.keys[k]].args.clone();
        let a: Vec<Expr> = named.into_iter().map(|e| self.expand(e)).collect();
        self.args[k] = Some(a.clone());
        a
    }

    fn expand(&mut self, e: Expr) -> Expr {
        e.rewrite(&mut |node| match node {
            Expr::Name(n) if n.starts_with(NAME) => {
                let k = self.index[&n[NAME.len()..]];
                let a = self.args_of(k);
                polynomial(&self.held[k], &a)
            }
            other => other,
        })
    }
}

/// Makes a held read's discrete variables and its zero crossings (their
/// functions written once its arguments are).
fn hold(model: &mut PreparedModel, r: &Pending, counts: &mut HashMap<u32, usize>) -> HeldRead {
    let ft = &model.flat.tables[r.table as usize];
    let instance = ft.instance;
    let base: u8 = if ft.data.interpolation == Interpolation::Linear { 1 } else { 3 };
    let path = model.flat.instance(instance).path.clone();
    let n = counts.entry(instance.0).or_insert(0);
    let prefix = if path.is_empty() { format!("cell[{n}]") } else { format!("{path}.cell[{n}]") };
    *n += 1;
    let starts = model.init.discrete_starts.len() == model.discretes.len()
        && !(model.init.discrete_starts.is_empty() && model.init.is_empty());
    let var = |model: &mut PreparedModel, name: String, start: f64| -> VarId {
        model.flat.vars.push(FlatVar {
            name,
            unit: Unit::ONE,
            unit_text: "1".into(),
            kind: VarKind::Discrete,
            start: Some(start),
            fixed: false,
            nominal: 1.0,
            instance,
            role: VarRole::Local,
            state_select: Default::default(),
        });
        let v = VarId(model.flat.vars.len() as u32 - 1);
        model.discretes.push(v);
        if starts {
            model.init.discrete_starts.push(Expr::Const(start));
        }
        v
    };
    let mut degree = [0u8; 2];
    let mut axes = vec![];
    for (j, dj) in degree.iter_mut().enumerate().take(r.args.len()) {
        *dj = base.saturating_sub(r.order[j]);
        let cell = var(model, format!("{prefix}.cell{}", j + 1), -1.0);
        let anchor = var(model, format!("{prefix}.at{}", j + 1), 0.0);
        let lo = var(model, format!("{prefix}.lo{}", j + 1), 0.0);
        let hi = var(model, format!("{prefix}.hi{}", j + 1), 0.0);
        let below = model.zero_crossings.len();
        let above = below + 1;
        for _ in 0..2 {
            model
                .zero_crossings
                .push(ZeroCrossing { expr: Expr::Const(1.0), origin: r.origin.clone() });
        }
        axes.push(HeldAxis { cell, anchor, lo, hi, below, above });
    }
    let mut coef = vec![];
    for i in 0..=degree[0] {
        for jj in 0..=degree[1] {
            coef.push(var(model, format!("{prefix}.c{i}{jj}"), 0.0));
        }
    }
    let read = Expr::Table { table: r.table, args: vec![], order: r.order };
    HeldRead { read, axes, degree, coef }
}

/// The held read's polynomial at `args`: `Σ c_ij (a₁ − m₁)^i (a₂ − m₂)^j`
/// in Horner's form.
fn polynomial(h: &HeldRead, args: &[Expr]) -> Expr {
    use lsim_ir::BinaryOp::{Add, Mul, Sub};
    let s: Vec<Expr> = args
        .iter()
        .zip(&h.axes)
        .map(|(a, ax)| Expr::bin(Sub, a.clone(), Expr::Var(ax.anchor)))
        .collect();
    let horner = |cs: &[Expr], x: &Expr| -> Expr {
        let mut it = cs.iter().rev();
        let mut acc = it.next().cloned().unwrap_or(Expr::Const(0.0));
        for c in it {
            acc = Expr::bin(Add, c.clone(), Expr::bin(Mul, x.clone(), acc));
        }
        acc
    };
    let width = usize::from(h.degree[1]) + 1;
    let rows: Vec<Expr> = (0..=usize::from(h.degree[0]))
        .map(|i| {
            let cs: Vec<Expr> = (0..width).map(|j| Expr::Var(h.coef[i * width + j])).collect();
            match s.get(1) {
                Some(t) => horner(&cs, t),
                None => cs[0].clone(),
            }
        })
        .collect();
    horner(&rows, &s[0])
}
