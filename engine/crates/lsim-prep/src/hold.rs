//! Tables read at a state, held in their cells during every step where the
//! read jumps at the table's breakpoints (DESIGN.md §8.2, *Tables read at
//! a state*): Park and Barton's discontinuity locking (ACM TOMACS 6(2),
//! 1996).
//!
//! Where a model reads a table at an argument that moves with its states,
//! the breakpoints' times are not known ahead and the integrator steps
//! over them as over any piecewise-smooth right-hand side. Measured on
//! exact answers (`engine/judge`, the `kink-…` cases), that is
//! proportional to the tolerance where the read only kinks there (a
//! linear table's value, a monotone cubic's first derivative), but not
//! where it jumps: a derivative that index reduction reads one order more
//! often than the table is smooth (a gear whose ratio is a linear table of
//! the load's speed, whose `r'` jumps at every point, ended up to 178 rtol
//! off, its error per tolerance growing as the tolerance tightens; a
//! monotone cubic held at its edges, whose `r'` jumps there, 5 261 in a
//! sweep). Holding a read costs a restart of the integrator at each
//! crossing (about 3 to 4 rtol of error each, measured): far more than
//! stepping over a kink where crossings are many, as in a table of many
//! points, so a read that only kinks is stepped over (as Modelica tools do
//! for a table's inputs), and [`TableCells::Kinks`] holds it too, for
//! comparison only.
//!
//! A held read is replaced in the equations by its cell's own function,
//! continued smoothly beyond the cell's edges, with discrete coefficients,
//! and the cell's two edges become zero crossings (`a − lo`, `hi − a`,
//! positive inside); the run loop sets them for the cell the argument lies
//! in at the start and at every crossing of an edge (an event: the
//! integrator restarts there), from the table the model interpolates (its
//! data may be swapped after the model was built: the coefficients come
//! from the model's own tables, not from here). So every step integrates a
//! smooth right-hand side and the crossing is found by root finding. Where
//! the read jumps at every breakpoint ([`HeldCells::Points`]), the cell is
//! the one between two of them and the function its polynomial in powers
//! of `(a − m)`; where it jumps only at the data's edges (a 1-D monotone
//! cubic held there, read once differentiated: [`HeldCells::Edges`]), the
//! cells are below, inside and above the data, and inside the read is the
//! table's own, continued beyond each edge by its edge cell's polynomial.
//! The other reads of the same table at the same arguments (the value
//! beside the derivative index reduction reads) are held with it: their
//! edges are crossed at the same instants, at no further cost. A 2-D read
//! that jumps is held at every breakpoint of both its arguments.
//!
//! Not held: a read whose argument moves with time alone (the run stops at
//! its breakpoints, `RunInfo::time_tables`) or not at all between events
//! (parameters and discrete values); the reads of the initialisation
//! system, of `when` clauses and of zero crossings (none integrates them);
//! an inverse model (fast mode, which does not integrate), and a model with
//! no state.

use lsim_ir::component::VarKind;
use lsim_ir::flat::{FlatVar, VarRole};
use lsim_ir::interp::data_smoothness;
use lsim_ir::units::Unit;
use lsim_ir::{
    BinaryOp, Builtin, Expr, HeldAxis, HeldCells, HeldEdges, HeldRead, Origin, PreparedModel, Slot,
    TableData, VarId, ZeroCrossing,
};
use std::collections::{BTreeMap, HashMap, HashSet};

/// An open edge of a cell (below the data's first point, above its last):
/// finite, so that the zero crossings stay numbers.
pub const OPEN: f64 = 1e300;

/// Which table reads at a state are held in their cells
/// ([`crate::Settings::table_cells`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TableCells {
    /// none: the integrator steps over every breakpoint (for comparison
    /// only: a read that jumps is then not proportional to the tolerance)
    Off,
    /// the reads that jump at a breakpoint (read differentiated more often
    /// than the table is continuously differentiable there), and the other
    /// reads of their tables at the same arguments
    #[default]
    Jumps,
    /// also the reads that kink there (a linear table's value, a monotone
    /// cubic's first derivative), for comparison only: each crossing is
    /// then a restart, which costs more than stepping over the kink where
    /// the crossings are many
    Kinks,
}

/// Where a read needs cells along one argument (ordered: the most
/// demanding last).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Need {
    /// nowhere: smooth enough for the read
    Nowhere,
    /// at the data's edges only
    Edges,
    /// at every breakpoint
    Points,
}

/// Where the read of derivative `order` of the table `data` needs cells,
/// per argument, under `mode`.
fn need(data: &TableData, order: [u8; 2], mode: TableCells) -> [Need; 2] {
    let mut out = [Need::Nowhere; 2];
    for (j, n) in out.iter_mut().enumerate().take(data.dims()) {
        let s = data_smoothness(data, j);
        let o = u32::from(order[j]);
        // (u32::MAX: no breakpoint there)
        let past = |c: u32| match mode {
            TableCells::Off => false,
            TableCells::Jumps => o > c,
            TableCells::Kinks => c != u32::MAX && o >= c,
        };
        *n = if past(s.inside) {
            Need::Points
        } else if past(s.edges) {
            Need::Edges
        } else {
            Need::Nowhere
        };
    }
    out
}

/// Holds the table reads of `model`'s assignments and residuals that
/// `mode` asks for ([`HeldRead`]). The reads are made in the order of
/// their keys (the table, the derivative and the arguments, an inner read
/// by its own key), so that every preparation of a model (one that chooses
/// its states again at a point of a run, `lsim_prep::prepare_choosing`)
/// holds the same reads in the same discrete variables and zero crossings.
pub fn apply(model: &mut PreparedModel, mode: TableCells) {
    if model.states.is_empty() || mode == TableCells::Off {
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
    // every expression with each read at a moving argument as a name
    // (`held:<key>`), held or not
    let named_a: Vec<Expr> = (0..model.assignments.len())
        .map(|i| {
            let origin = model.assignments[i].origin.clone();
            named(model.assignments[i].expr.clone(), model, &mut st, &origin, mode)
        })
        .collect();
    let named_r: Vec<Expr> = (0..model.residuals.len())
        .map(|i| {
            let origin = model.residuals[i].origin.clone();
            named(model.residuals[i].expr.clone(), model, &mut st, &origin, mode)
        })
        .collect();
    // where the reads of a table at the same arguments need cells: the
    // most any of them needs (held together, their edges are crossed
    // together)
    let mut group: HashMap<String, [Need; 2]> = HashMap::new();
    for r in st.reads.values() {
        let g = group.entry(r.group.clone()).or_insert([Need::Nowhere; 2]);
        for (gj, rj) in g.iter_mut().zip(r.need) {
            *gj = (*gj).max(rj);
        }
    }
    let keys: Vec<String> = st.reads.keys().cloned().collect();
    let cells: Vec<Option<Cells>> = keys
        .iter()
        .map(|k| {
            let r = &st.reads[k];
            let g = group[&r.group];
            if g.iter().all(|n| *n == Need::Nowhere) {
                None
            } else if r.args.len() == 1 && g[0] == Need::Edges {
                Some(Cells::Edges)
            } else {
                Some(Cells::Points)
            }
        })
        .collect();
    if cells.iter().all(Option::is_none) {
        return;
    }
    let index: HashMap<String, usize> =
        keys.iter().enumerate().map(|(k, key)| (key.clone(), k)).collect();
    // the held reads' variables and zero crossings, in their keys' order
    let mut counts: HashMap<u32, usize> = HashMap::new();
    let mut held: Vec<Option<HeldRead>> = vec![None; keys.len()];
    for (k, key) in keys.iter().enumerate() {
        if let Some(c) = cells[k] {
            held[k] = Some(hold(model, &st.reads[key], c, &mut counts));
        }
    }
    // the names as the held reads' functions (the others as the reads
    // they stand for), the zero crossings' arguments
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
    let mut out = vec![];
    for (h, a) in held.into_iter().zip(all) {
        let Some(mut h) = h else { continue };
        for (j, ax) in h.axes.iter().enumerate() {
            model.zero_crossings[ax.below].expr =
                Expr::bin(BinaryOp::Sub, a[j].clone(), Expr::Var(ax.lo));
            model.zero_crossings[ax.above].expr =
                Expr::bin(BinaryOp::Sub, Expr::Var(ax.hi), a[j].clone());
        }
        let Expr::Table { table, order, .. } = h.read else { unreachable!("a table read") };
        h.read = Expr::Table { table, args: a, order };
        out.push(h);
    }
    model.held = out;
}

/// The cells a held read gets.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Cells {
    Points,
    Edges,
}

/// A table read at a moving argument: its table, derivative, arguments
/// (inner reads as names), the equation it was first found in, where it
/// needs cells itself, and the key of the reads at the same arguments.
struct Pending {
    table: u32,
    order: [u8; 2],
    args: Vec<Expr>,
    origin: Origin,
    need: [Need; 2],
    group: String,
}

struct State {
    /// the reads at moving arguments, by key
    reads: BTreeMap<String, Pending>,
    /// what each computed variable is computed from (the assignments as
    /// preparation made them)
    assigned: HashMap<VarId, Expr>,
    /// the states and iteration variables
    solved: HashSet<VarId>,
    /// per computed variable: whether it moves with them (memoised)
    moving: HashMap<VarId, bool>,
}

/// The name a read at a moving argument stands as until it is written out.
const NAME: &str = "held:";

/// Whether `e` moves with the states between events: through the
/// assignments it reads a state, an iteration variable, a derivative or a
/// table read at a moving argument. Not one that moves with time alone (a
/// driving cycle's position: the run stops at its breakpoints), nor one of
/// parameters and discrete values.
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

/// `e` with each table read at a moving argument as its name, innermost
/// first.
fn named(
    e: Expr,
    model: &PreparedModel,
    st: &mut State,
    origin: &Origin,
    mode: TableCells,
) -> Expr {
    e.rewrite(&mut |node| {
        let Expr::Table { table, args, order } = &node else { return node };
        let data = &model.flat.tables[*table as usize].data;
        let dims = data.dims();
        let points = [data.x.len(), data.y.len()];
        if args.len() != dims
            || points[..dims].iter().any(|&n| n < 2)
            || !args.iter().any(|a| moves(a, st, 0))
        {
            return node;
        }
        let key = format!("{table}:{order:?}:{args:?}");
        let group = format!("{table}:{args:?}");
        st.reads.entry(key.clone()).or_insert_with(|| Pending {
            table: *table,
            order: *order,
            args: args.clone(),
            origin: origin.clone(),
            need: need(data, *order, mode),
            group,
        });
        Expr::Name(format!("{NAME}{key}"))
    })
}

/// The names written out: a held read's as its function, the others' as
/// the reads they stand for.
struct Expand<'a> {
    keys: &'a [String],
    index: &'a HashMap<String, usize>,
    reads: &'a BTreeMap<String, Pending>,
    held: &'a [Option<HeldRead>],
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
                let r = &self.reads[&self.keys[k]];
                match &self.held[k] {
                    Some(h) => written(h, r.table, r.order, &a),
                    None => Expr::Table { table: r.table, args: a, order: r.order },
                }
            }
            other => other,
        })
    }
}

/// Makes a held read's discrete variables and its zero crossings (their
/// functions written once its arguments are).
fn hold(
    model: &mut PreparedModel,
    r: &Pending,
    cells: Cells,
    counts: &mut HashMap<u32, usize>,
) -> HeldRead {
    let ft = &model.flat.tables[r.table as usize];
    let instance = ft.instance;
    let base: u8 = if ft.data.interpolation == lsim_ir::Interpolation::Linear { 1 } else { 3 };
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
    let cells = match cells {
        Cells::Points => {
            for i in 0..=degree[0] {
                for jj in 0..=degree[1] {
                    coef.push(var(model, format!("{prefix}.c{i}{jj}"), 0.0));
                }
            }
            HeldCells::Points
        }
        Cells::Edges => {
            // the first data cell's polynomial, then the last's
            for i in 0..=degree[0] {
                coef.push(var(model, format!("{prefix}.c{i}"), 0.0));
            }
            for i in 0..=degree[0] {
                coef.push(var(model, format!("{prefix}.e{i}"), 0.0));
            }
            HeldCells::Edges(HeldEdges {
                from: var(model, format!("{prefix}.from"), -OPEN),
                to: var(model, format!("{prefix}.to"), OPEN),
                offset: var(model, format!("{prefix}.offset"), 0.0),
                anchor_end: var(model, format!("{prefix}.at1end"), 0.0),
            })
        }
    };
    let read = Expr::Table { table: r.table, args: vec![], order: r.order };
    HeldRead { read, axes, degree, coef, cells }
}

/// `Σ cs[i] x^i` in Horner's form.
fn horner(cs: &[Expr], x: &Expr) -> Expr {
    use BinaryOp::{Add, Mul};
    let mut it = cs.iter().rev();
    let mut acc = it.next().cloned().unwrap_or(Expr::Const(0.0));
    for c in it {
        acc = Expr::bin(Add, c.clone(), Expr::bin(Mul, x.clone(), acc));
    }
    acc
}

/// The held read `h` of derivative `order` of table `table` at `args`, as
/// the equations read it.
fn written(h: &HeldRead, table: u32, order: [u8; 2], args: &[Expr]) -> Expr {
    use BinaryOp::{Add, Sub};
    match &h.cells {
        // `Σ c_ij (a₁ − m₁)^i (a₂ − m₂)^j`
        HeldCells::Points => {
            let s: Vec<Expr> = args
                .iter()
                .zip(&h.axes)
                .map(|(a, ax)| Expr::bin(Sub, a.clone(), Expr::Var(ax.anchor)))
                .collect();
            let width = usize::from(h.degree[1]) + 1;
            let rows: Vec<Expr> = (0..=usize::from(h.degree[0]))
                .map(|i| {
                    let cs: Vec<Expr> =
                        (0..width).map(|j| Expr::Var(h.coef[i * width + j])).collect();
                    match s.get(1) {
                        Some(t) => horner(&cs, t),
                        None => cs[0].clone(),
                    }
                })
                .collect();
            horner(&rows, &s[0])
        }
        // `T(max(from, min(to, a))) + offset + (P_last(max(a, to)) −
        // P_last(to)) + (P_first(min(a, from)) − P_first(from))`: the
        // arguments of `min` and `max` in this order, so that at a tie
        // (the argument on an edge) one side's rate counts, not both
        HeldCells::Edges(e) => {
            let a = &args[0];
            let call = |f: Builtin, x: Expr, y: Expr| Expr::Call(f, vec![x, y]);
            let (from, to) = (Expr::Var(e.from), Expr::Var(e.to));
            let at = call(Builtin::Max, from.clone(), call(Builtin::Min, to.clone(), a.clone()));
            let read = Expr::Table { table, args: vec![at], order };
            let w = usize::from(h.degree[0]) + 1;
            let first: Vec<Expr> = h.coef[..w].iter().map(|v| Expr::Var(*v)).collect();
            let last: Vec<Expr> = h.coef[w..].iter().map(|v| Expr::Var(*v)).collect();
            let p = |cs: &[Expr], m: VarId, x: Expr| horner(cs, &Expr::bin(Sub, x, Expr::Var(m)));
            let up = Expr::bin(
                Sub,
                p(&last, e.anchor_end, call(Builtin::Max, a.clone(), to.clone())),
                p(&last, e.anchor_end, to),
            );
            let m0 = h.axes[0].anchor;
            let down = Expr::bin(
                Sub,
                p(&first, m0, call(Builtin::Min, a.clone(), from.clone())),
                p(&first, m0, from),
            );
            let body = Expr::bin(Add, read, Expr::Var(e.offset));
            Expr::bin(Add, Expr::bin(Add, body, up), down)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsim_ir::{Interpolation, Outside};

    fn data(interp: Interpolation, outside: Outside, n: usize) -> TableData {
        let x: Vec<f64> = (0..n).map(|i| i as f64).collect();
        let mut d = TableData::new_1d(x.clone(), x.iter().map(|v| v * v).collect());
        d.interpolation = interp;
        d.outside = [outside, outside];
        d
    }

    /// The rule: a read jumps where it is differentiated more often than
    /// the table is continuously differentiable there.
    #[test]
    fn a_read_needs_cells_where_it_jumps() {
        use Interpolation::{Linear, MonotoneCubic};
        use Need::{Edges, Nowhere, Points};
        use Outside::Clamp;
        let lin = |o| data(Linear, o, 5);
        let cub = |o| data(MonotoneCubic, o, 5);
        let j = TableCells::Jumps;
        // a linear table's value kinks (stepped over); its slope jumps
        assert_eq!(need(&lin(Clamp), [0, 0], j)[0], Nowhere);
        assert_eq!(need(&lin(Outside::Linear), [1, 0], j)[0], Points);
        assert_eq!(need(&lin(Clamp), [1, 0], j)[0], Points);
        // a monotone cubic's value and slope are continuous inside; held
        // at its edges, the slope jumps there
        assert_eq!(need(&cub(Clamp), [0, 0], j)[0], Nowhere);
        assert_eq!(need(&cub(Outside::Linear), [1, 0], j)[0], Nowhere);
        assert_eq!(need(&cub(Clamp), [1, 0], j)[0], Edges);
        assert_eq!(need(&cub(Outside::Linear), [2, 0], j)[0], Points);
        // two points: a single cell, a line continued
        assert_eq!(need(&data(Linear, Outside::Linear, 2), [1, 0], j)[0], Nowhere);
        assert_eq!(need(&data(Linear, Clamp, 2), [1, 0], j)[0], Edges);
        // kinks held too, for comparison
        let k = TableCells::Kinks;
        assert_eq!(need(&lin(Clamp), [0, 0], k)[0], Points);
        assert_eq!(need(&cub(Clamp), [0, 0], k)[0], Edges);
        assert_eq!(need(&cub(Outside::Linear), [0, 0], k)[0], Nowhere);
        assert_eq!(need(&cub(Outside::Linear), [1, 0], k)[0], Points);
        assert_eq!(need(&lin(Outside::Linear), [0, 0], k)[0], Points);
        assert_eq!(need(&data(Linear, Outside::Linear, 2), [0, 0], k)[0], Nowhere);
        assert_eq!(need(&lin(Clamp), [1, 0], TableCells::Off)[0], Nowhere);
    }
}
