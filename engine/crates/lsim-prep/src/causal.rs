//! Sorting and solving a square system (DESIGN.md, *Preparation
//! pipeline*, steps 4, 6 and 7), used for the model and for its
//! initialisation system alike:
//!
//! 1. **Matching** by Hopcroft–Karp; a system that has none is handed back
//!    for the structural diagnostics.
//! 2. **Block-lower-triangular order** by Tarjan's algorithm.
//! 3. A one-equation block affine in its unknown becomes an explicit
//!    assignment `x := -b/a` when the **pivot** `a` cannot be zero: a
//!    non-zero constant, an expression of parameters that the parameters'
//!    declared ranges keep away from zero (or, for a parameter declared
//!    without a range, its present sign — recorded as a guard the run
//!    checks), or an expression of the variables (then dividing by it is
//!    no worse than Newton's iteration on the same equation).
//! 4. A larger block is **torn** (Cellier's heuristics: equations with one
//!    unknown are solved for it, a variable left in one equation is solved
//!    from it last, and when neither applies the variable that makes the
//!    most equations solvable becomes a tearing variable; then each tearing
//!    variable the block can do without is given back). Inner equations
//!    are solved only where their pivot is safe. A block **linear** in a
//!    single tearing variable is then solved for it symbolically, so it
//!    needs no iteration at all; otherwise the tearing variables become
//!    iteration variables `z` and the leftover equations their residuals.

#![allow(clippy::needless_range_loop)] // parallel arrays indexed in step

use crate::graph::{Bipartite, Matching, NONE, hopcroft_karp, scc};
use crate::symbolic::{
    SignEnv, Signs, affine_coefficient, contains, signs, simplify, solve_affine,
};
use crate::system::{NodeKind, node, nodes_of};
use lsim_ir::expr::Expr;
use lsim_ir::prepared::Slot;
use lsim_ir::{ParamId, VarId};

/// What the sign analysis knows about a parameter.
#[derive(Clone, Copy, Debug)]
pub struct ParamInfo {
    /// its value at preparation
    pub value: f64,
    /// declared lowest value
    pub min: Option<f64>,
    /// declared highest value
    pub max: Option<f64>,
}

/// What sorting needs besides the equations.
pub struct Ctx<'a> {
    /// per parameter
    pub params: &'a [ParamInfo],
    /// per node: what it is
    pub kinds: &'a [NodeKind],
    /// per node: it holds a mode (0 or 1)
    pub is_mode: &'a [bool],
    /// keep every block implicit
    pub force_implicit: bool,
}

impl SignEnv for Ctx<'_> {
    fn param(&self, p: ParamId) -> Signs {
        let i = &self.params[p.0 as usize];
        if i.min.is_some() || i.max.is_some() {
            let lo = i.min.unwrap_or(f64::NEG_INFINITY);
            let hi = i.max.unwrap_or(f64::INFINITY);
            return Signs {
                neg: lo < 0.0,
                zero: lo <= 0.0 && hi >= 0.0,
                pos: hi > 0.0,
                assumed: false,
            };
        }
        let v = i.value;
        if v.is_nan() {
            return Signs::ANY;
        }
        Signs { neg: v < 0.0, zero: v == 0.0, pos: v > 0.0, assumed: v != 0.0 }
    }
    fn var(&self, v: VarId) -> Signs {
        if self.is_mode.get(v.0 as usize).copied().unwrap_or(false) {
            Signs { neg: false, zero: true, pos: true, assumed: false }
        } else {
            Signs::ANY
        }
    }
}

/// How safe it is to divide by a coefficient.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pivot {
    /// never zero
    Safe,
    /// never zero while the parameters keep their signs: needs a guard
    Guarded,
    /// depends on variables (or discrete values): fine for a single
    /// equation, not inside a torn block
    Variable,
    /// can be zero for an allowed parameter value
    Unsafe,
}

/// Classifies coefficient `a`.
pub fn pivot(a: &Expr, ctx: &Ctx<'_>) -> Pivot {
    let mut variable = false;
    a.walk(&mut |x| match x {
        Expr::Var(v) | Expr::Pre(v) => {
            let n = v.0 as usize;
            if ctx.kinds[n] != NodeKind::Discrete || !ctx.is_mode[n] {
                variable = true;
            }
        }
        Expr::Time => variable = true,
        _ => {}
    });
    let s = signs(a, ctx);
    if s.nonzero() {
        if s.assumed { Pivot::Guarded } else { Pivot::Safe }
    } else if variable {
        Pivot::Variable
    } else {
        Pivot::Unsafe
    }
}

/// One block of the sorted system, for reports and the numeric solver.
#[derive(Clone, Debug, Default)]
pub struct BlockInfo {
    /// its equations (indices into the system's equation list)
    pub eqs: Vec<usize>,
    /// its unknowns (nodes)
    pub nodes: Vec<usize>,
    /// how many tearing variables tearing chose
    pub torn: usize,
    /// whether its torn residuals are linear in the tearing variables
    pub linear: bool,
    /// its assignments: `Sorted::assignments[assign.0..assign.1]`
    pub assign: (usize, usize),
    /// its iteration variables and residuals: `[iter.0..iter.1]` of both
    pub iter: (usize, usize),
}

/// The sorted system.
#[derive(Clone, Debug, Default)]
pub struct Sorted {
    /// node := expression, from equation k, in evaluation order
    pub assignments: Vec<(usize, Expr, usize)>,
    /// iteration variables (nodes)
    pub iteration: Vec<usize>,
    /// residual and its equation, one per iteration variable
    pub residuals: Vec<(Expr, usize)>,
    /// the blocks, in order
    pub blocks: Vec<BlockInfo>,
    /// parameter expressions divided by, and the equation that needed it
    pub guards: Vec<(Expr, usize)>,
    /// explicit solutions whose pivot depends on variables: (pivot,
    /// equation), for the check at the initial point
    pub variable_pivots: Vec<(Expr, usize)>,
}

/// A system without a perfect matching, for the diagnostics.
#[derive(Debug)]
pub struct Unmatched {
    /// equations → unknown columns
    pub graph: Bipartite,
    /// a maximum matching
    pub matching: Matching,
}

/// Sorts and solves `eqs` (residuals over nodes) for `unknowns` (nodes).
pub fn sort(eqs: &[&Expr], unknowns: &[usize], ctx: &Ctx<'_>) -> Result<Sorted, Unmatched> {
    let mut col_of = vec![NONE; ctx.kinds.len()];
    for (c, &n) in unknowns.iter().enumerate() {
        col_of[n] = c;
    }
    let rows: Vec<Vec<usize>> = eqs
        .iter()
        .map(|e| {
            nodes_of(e).into_iter().filter(|&n| col_of[n] != NONE).map(|n| col_of[n]).collect()
        })
        .collect();
    let graph = Bipartite::from_rows(unknowns.len(), &rows);
    let matching = hopcroft_karp(&graph);
    if !matching.is_perfect() || eqs.len() != unknowns.len() {
        return Err(Unmatched { graph, matching });
    }
    let adj: Vec<Vec<usize>> = (0..eqs.len())
        .map(|e| graph.row(e).iter().map(|&c| matching.col[c]).filter(|&e2| e2 != e).collect())
        .collect();
    let mut out = Sorted::default();
    for block in scc(&adj) {
        let nodes: Vec<usize> = block.iter().map(|&e| unknowns[matching.row[e]]).collect();
        solve_block(eqs, &block, &nodes, &col_of, ctx, &mut out);
    }
    Ok(out)
}

fn solve_block(
    eqs: &[&Expr],
    block: &[usize],
    nodes: &[usize],
    col_of: &[usize],
    ctx: &Ctx<'_>,
    out: &mut Sorted,
) {
    let a0 = out.assignments.len();
    let i0 = out.iteration.len();
    let mut info = BlockInfo { eqs: block.to_vec(), nodes: nodes.to_vec(), ..Default::default() };
    let implicit = |out: &mut Sorted| {
        for (k, &e) in block.iter().enumerate() {
            out.iteration.push(nodes[k]);
            out.residuals.push((eqs[e].clone(), e));
        }
    };
    if ctx.force_implicit {
        implicit(out);
        info.torn = block.len();
    } else if block.len() == 1 {
        let (e, n) = (block[0], nodes[0]);
        if !solve_one(eqs[e], n, e, ctx, true, out) {
            out.iteration.push(n);
            out.residuals.push((eqs[e].clone(), e));
            info.torn = 1;
        }
    } else {
        tear_block(eqs, block, nodes, col_of, ctx, out, &mut info);
    }
    info.assign = (a0, out.assignments.len());
    info.iter = (i0, out.iteration.len());
    out.blocks.push(info);
}

/// Solves `res = 0` for node `n` if its pivot allows; pushes the
/// assignment (and its guard). `variable_ok`: a pivot that depends on
/// variables is acceptable.
fn solve_one(
    res: &Expr,
    n: usize,
    e: usize,
    ctx: &Ctx<'_>,
    variable_ok: bool,
    out: &mut Sorted,
) -> bool {
    let s = Slot::Var(VarId(n as u32));
    let Some((a, sol)) = solve_affine(res, s) else { return false };
    let p = pivot(&a, ctx);
    match p {
        Pivot::Unsafe => return false,
        Pivot::Variable if !variable_ok => return false,
        _ => {}
    }
    match p {
        Pivot::Guarded => out.guards.push((a, e)),
        Pivot::Variable => out.variable_pivots.push((a, e)),
        _ => {}
    }
    out.assignments.push((n, sol, e));
    true
}

/// Whether node `n` can be computed from equation `res` inside a torn
/// block: affine with a pivot that cannot vanish.
fn safely_solvable(res: &Expr, n: usize, ctx: &Ctx<'_>) -> bool {
    affine_coefficient(res, Slot::Var(VarId(n as u32)))
        .is_some_and(|a| matches!(pivot(&a, ctx), Pivot::Safe | Pivot::Guarded))
}

/// The causal order tearing found inside a block (local indices).
struct Causal {
    /// (local node, local equation), in evaluation order
    order: Vec<(usize, usize)>,
    /// tearing variables (local nodes), in the order chosen
    torn: Vec<usize>,
    /// residual equations (local)
    residuals: Vec<usize>,
}

/// Propagation with the tearing set `torn` fixed (and, when `choose` is
/// set, new tearing variables chosen whenever propagation stalls). `None`
/// when the fixed set does not suffice.
fn causalize(
    edges: &[Vec<(usize, bool)>],
    var_rows: &[Vec<usize>],
    torn_in: &[usize],
    choose: bool,
) -> Option<Causal> {
    let m = edges.len();
    let mut assigned = vec![false; m];
    let mut used = vec![false; m];
    let mut cnt: Vec<usize> = edges.iter().map(Vec::len).collect();
    let mut col_cnt: Vec<usize> = var_rows.iter().map(Vec::len).collect();
    let mut torn = vec![];
    let mut forward = vec![];
    let mut back = vec![];
    let mut q1: Vec<usize> = vec![];
    let mut q2: Vec<usize> = vec![];
    let mut n_assigned = 0;
    let solvable = |e: usize, n: usize| edges[e].iter().any(|&(x, s)| x == n && s);

    fn mark(
        n: usize,
        assigned: &mut [bool],
        used: &[bool],
        cnt: &mut [usize],
        var_rows: &[Vec<usize>],
        q1: &mut Vec<usize>,
    ) {
        assigned[n] = true;
        for &e in &var_rows[n] {
            if !used[e] {
                cnt[e] -= 1;
                if cnt[e] == 1 {
                    q1.push(e);
                }
            }
        }
    }
    for &t in torn_in {
        mark(t, &mut assigned, &used, &mut cnt, var_rows, &mut q1);
        torn.push(t);
        n_assigned += 1;
    }
    for e in 0..m {
        if cnt[e] == 1 {
            q1.push(e);
        }
    }
    for n in 0..m {
        if col_cnt[n] == 1 {
            q2.push(n);
        }
    }
    loop {
        let mut progress = true;
        while progress {
            progress = false;
            while let Some(e) = q1.pop() {
                if used[e] || cnt[e] != 1 {
                    continue;
                }
                let Some(&(n, s)) = edges[e].iter().find(|&&(x, _)| !assigned[x]) else {
                    continue;
                };
                if !s {
                    continue;
                }
                used[e] = true;
                for &(x, _) in &edges[e] {
                    if !assigned[x] && x != n {
                        col_cnt[x] -= 1;
                        if col_cnt[x] == 1 {
                            q2.push(x);
                        }
                    }
                }
                col_cnt[n] -= 1;
                mark(n, &mut assigned, &used, &mut cnt, var_rows, &mut q1);
                forward.push((n, e));
                n_assigned += 1;
                progress = true;
            }
            while let Some(n) = q2.pop() {
                if assigned[n] || col_cnt[n] != 1 {
                    continue;
                }
                let Some(&e) = var_rows[n].iter().find(|&&e| !used[e]) else { continue };
                if !solvable(e, n) {
                    continue;
                }
                used[e] = true;
                assigned[n] = true;
                for &(x, _) in &edges[e] {
                    if !assigned[x] {
                        col_cnt[x] -= 1;
                        if col_cnt[x] == 1 {
                            q2.push(x);
                        }
                    }
                }
                back.push((n, e));
                n_assigned += 1;
                progress = true;
                if !q1.is_empty() {
                    break;
                }
            }
        }
        if n_assigned == m {
            break;
        }
        if !choose {
            return None;
        }
        // stalled: the variable whose tearing makes most equations
        // solvable, then the one in most open equations
        let mut best: Option<(usize, usize, usize)> = None;
        let mut gain = vec![0usize; m];
        for e in 0..m {
            if used[e] || cnt[e] != 2 {
                continue;
            }
            let open: Vec<(usize, bool)> =
                edges[e].iter().copied().filter(|&(x, _)| !assigned[x]).collect();
            if let [(a, sa), (b, sb)] = open[..] {
                if sb {
                    gain[a] += 1;
                }
                if sa {
                    gain[b] += 1;
                }
            }
        }
        for n in 0..m {
            if assigned[n] {
                continue;
            }
            let key = (gain[n], col_cnt[n], n);
            if best.is_none_or(|b| (key.0, key.1) > (b.0, b.1)) {
                best = Some(key);
            }
        }
        let (_, _, t) = best.expect("an unassigned variable remains");
        mark(t, &mut assigned, &used, &mut cnt, var_rows, &mut q1);
        torn.push(t);
        n_assigned += 1;
    }
    let mut order = forward;
    order.extend(back.into_iter().rev());
    let residuals: Vec<usize> = (0..m).filter(|&e| !used[e]).collect();
    debug_assert_eq!(residuals.len(), torn.len());
    Some(Causal { order, torn, residuals })
}

/// The expression size up to which a linear block's residual is solved
/// symbolically for its tearing variable.
const SUBSTITUTION_BUDGET: usize = 20_000;

fn tear_block(
    eqs: &[&Expr],
    block: &[usize],
    nodes: &[usize],
    col_of: &[usize],
    ctx: &Ctx<'_>,
    out: &mut Sorted,
    info: &mut BlockInfo,
) {
    let m = block.len();
    let _ = col_of;
    let local_of: std::collections::HashMap<usize, usize> =
        nodes.iter().enumerate().map(|(k, &n)| (n, k)).collect();
    let mut edges: Vec<Vec<(usize, bool)>> = Vec::with_capacity(m);
    let mut var_rows: Vec<Vec<usize>> = vec![vec![]; m];
    for (le, &e) in block.iter().enumerate() {
        let mut row = vec![];
        for n in nodes_of(eqs[e]) {
            if let Some(&k) = local_of.get(&n) {
                row.push((k, safely_solvable(eqs[e], n, ctx)));
                var_rows[k].push(le);
            }
        }
        edges.push(row);
    }
    let mut c = causalize(&edges, &var_rows, &[], true).expect("choosing never fails");
    // give back every tearing variable the block can do without
    let work: usize = edges.iter().map(Vec::len).sum::<usize>() * c.torn.len();
    if c.torn.len() > 1 && work <= 20_000_000 {
        let mut k = c.torn.len();
        while k > 0 {
            k -= 1;
            if c.torn.len() <= 1 {
                break;
            }
            let mut fewer = c.torn.clone();
            fewer.remove(k);
            if let Some(better) = causalize(&edges, &var_rows, &fewer, false) {
                c = better;
                k = k.min(c.torn.len());
            }
        }
    }
    info.torn = c.torn.len();
    // the inner equations, solved
    let mut inner: Vec<(usize, Expr, usize)> = Vec::with_capacity(c.order.len());
    let mut guards = vec![];
    for &(ln, le) in &c.order {
        let (n, e) = (nodes[ln], block[le]);
        let s = Slot::Var(VarId(n as u32));
        let (a, sol) = solve_affine(eqs[e], s).expect("safely solvable");
        if pivot(&a, ctx) == Pivot::Guarded {
            guards.push((a, e));
        }
        inner.push((n, sol, e));
    }
    let torn_nodes: Vec<usize> = c.torn.iter().map(|&k| nodes[k]).collect();
    let resid_eqs: Vec<usize> = c.residuals.iter().map(|&k| block[k]).collect();
    // linear in the tearing variables? substitute the inner solutions
    let substituted = substitute_inner(&inner, &resid_eqs, eqs);
    info.linear = substituted.as_ref().is_some_and(|rs| {
        rs.iter().all(|r| {
            torn_nodes.iter().all(|&t| {
                let s = Slot::Var(VarId(t as u32));
                !contains(r, s)
                    || affine_coefficient(r, s).is_some_and(|a| {
                        !torn_nodes.iter().any(|&u| contains(&a, Slot::Var(VarId(u as u32))))
                    })
            })
        })
    });
    if info.linear
        && torn_nodes.len() == 1
        && let Some(rs) = &substituted
    {
        let t = torn_nodes[0];
        let mut tmp = Sorted::default();
        if solve_one(&rs[0], t, resid_eqs[0], ctx, true, &mut tmp) {
            out.assignments.extend(tmp.assignments);
            out.guards.extend(tmp.guards);
            out.variable_pivots.extend(tmp.variable_pivots);
            out.guards.extend(guards);
            out.assignments.extend(inner);
            return;
        }
    }
    out.guards.extend(guards);
    out.assignments.extend(inner);
    for (k, &t) in torn_nodes.iter().enumerate() {
        out.iteration.push(t);
        out.residuals.push((eqs[resid_eqs[k]].clone(), resid_eqs[k]));
    }
}

/// The residuals with the inner assignments substituted, while the
/// expressions stay within the budget.
fn substitute_inner(
    inner: &[(usize, Expr, usize)],
    resid_eqs: &[usize],
    eqs: &[&Expr],
) -> Option<Vec<Expr>> {
    let mut sub: std::collections::HashMap<usize, Expr> = Default::default();
    let mut total = 0usize;
    let mut apply = |e: &Expr, sub: &std::collections::HashMap<usize, Expr>| -> Option<Expr> {
        let r = simplify(e.clone().rewrite(&mut |x| match x {
            Expr::Var(v) => sub.get(&(v.0 as usize)).cloned().unwrap_or(x),
            other => other,
        }));
        total += r.size();
        (total <= SUBSTITUTION_BUDGET).then_some(r)
    };
    for (n, expr, _) in inner {
        let s = apply(expr, &sub)?;
        sub.insert(*n, s);
    }
    resid_eqs.iter().map(|&e| apply(eqs[e], &sub)).collect()
}

/// A reference to node `n` (re-exported for the modules that build
/// residuals).
pub fn var(n: usize) -> Expr {
    node(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsim_ir::expr::c;

    fn ctx<'a>(kinds: &'a [NodeKind], modes: &'a [bool]) -> Ctx<'a> {
        Ctx { params: &[], kinds, is_mode: modes, force_implicit: false }
    }

    #[test]
    fn a_ring_of_resistors_tears_to_one_variable() {
        // currents i0..i3 around a loop of four resistors with one source:
        // v_k = R i_k, i_k = i_{k+1}, sum of v = V: one tearing variable,
        // and linear, so it is solved explicitly
        let v = |k: usize| var(k);
        let i = |k: usize| var(4 + k);
        let eqs: Vec<Expr> = vec![
            v(0) - c(2.0) * i(0),
            v(1) - c(3.0) * i(1),
            v(2) - c(4.0) * i(2),
            v(3) - c(5.0) * i(3),
            i(0) - i(1),
            i(1) - i(2),
            i(2) - i(3),
            v(0) + v(1) + v(2) + v(3) - c(14.0),
        ];
        let refs: Vec<&Expr> = eqs.iter().collect();
        let kinds = vec![NodeKind::Unknown; 8];
        let modes = vec![false; 8];
        let s = sort(&refs, &(0..8).collect::<Vec<_>>(), &ctx(&kinds, &modes)).unwrap();
        assert_eq!(s.blocks.len(), 1);
        assert_eq!(s.blocks[0].torn, 1);
        assert!(s.blocks[0].linear);
        assert!(s.iteration.is_empty(), "solved explicitly: {:?}", s.iteration);
        // evaluate: the current is 14 / (2+3+4+5) = 1
        let mut vals = vec![f64::NAN; 8];
        for (n, e, _) in &s.assignments {
            let env = crate::system::NodeEnv { t: 0.0, vals: &vals, params: &[] };
            vals[*n] = lsim_ir::eval::eval(e, &env);
        }
        for k in 0..4 {
            assert!((vals[4 + k] - 1.0).abs() < 1e-14, "{vals:?}");
        }
    }

    #[test]
    fn a_nonlinear_loop_keeps_its_tearing_variable() {
        // x = exp(-y), y = x: one tearing variable, not linear
        let eqs: Vec<Expr> = vec![
            var(0) - lsim_ir::expr::call(lsim_ir::Builtin::Exp, vec![-var(1)]),
            var(1) - var(0),
        ];
        let refs: Vec<&Expr> = eqs.iter().collect();
        let kinds = vec![NodeKind::Unknown; 2];
        let modes = vec![false; 2];
        let s = sort(&refs, &[0, 1], &ctx(&kinds, &modes)).unwrap();
        assert_eq!(s.iteration.len(), 1);
        assert_eq!(s.residuals.len(), 1);
        assert!(!s.blocks[0].linear);
    }
}
