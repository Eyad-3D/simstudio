//! Index reduction (DESIGN.md, *Preparation pipeline*, step 5).
//!
//! When states are constrained — two inertias rigidly coupled, a capacitor
//! directly across a voltage source, a pendulum's length, a speed
//! prescribed in fast mode — the equations cannot be matched to the
//! highest derivatives: the model has index 2 or more.
//!
//! 1. **Pantelides' algorithm** finds each structurally singular subset by
//!    augmenting paths and differentiates its equations in time until every
//!    equation can be matched to a highest derivative.
//! 2. **Dummy derivatives** (Mattsson and Söderlind): going down from the
//!    highest differentiated equations, as many highest derivatives as
//!    there are such equations become ordinary algebraic unknowns ("dummy
//!    derivatives"), and the variables they are derivatives of stop being
//!    states. The original equations and all their derivatives are kept,
//!    so the constraints hold exactly (no drift). Which derivatives become
//!    dummies is chosen by Gaussian elimination with threshold pivoting on
//!    the Jacobian of the differentiated equations evaluated at the
//!    initial point, preferring to keep as states the variables marked so
//!    (Modelica's `stateSelect`), then those whose start values are fixed.
//!    The choice is static: if the initial point makes every choice
//!    singular (a pendulum started exactly horizontal in the coordinate the
//!    selection needs), the preparation says so instead of switching states
//!    during the run (risk R3). What was chosen, and among what
//!    ([`ChosenSet`]), goes with the prepared model, so the run can watch
//!    the choice's pivots and stop where they degrade.

use crate::graph::{Bipartite, NONE, groups, hopcroft_karp};
use crate::symbolic::diff;
use crate::system::{NodeEnv, NodeKind, Sys};
use lsim_ir::VarId;
use lsim_ir::eval::eval;
use lsim_ir::prepared::Slot;

/// Why index reduction stopped.
#[derive(Clone, Debug)]
pub enum IndexFault {
    /// an equation had to be differentiated more often than any physical
    /// model needs (the system is structurally singular in a way the
    /// earlier checks missed)
    TooHigh {
        /// the equation
        eq: usize,
    },
    /// an equation could not be differentiated
    Differentiate {
        /// the equation
        eq: usize,
        /// why
        why: String,
    },
    /// a table would be differentiated more often than its interpolant is
    /// smooth: a derivative below the one read jumps somewhere
    TableNotSmooth {
        /// the differentiated equation that reads it
        eq: usize,
        /// the table's index in the flat system
        table: u32,
        /// how often it is differentiated (in all, over its arguments)
        order: u32,
        /// how many times it is continuously differentiable
        continuity: u32,
    },
    /// no static choice of dummy derivatives is regular at the initial point
    Singular {
        /// the differentiated equations involved
        eqs: Vec<usize>,
        /// the candidate nodes
        nodes: Vec<usize>,
    },
}

/// The deepest differentiation allowed: mechanical models reach index 3.
const MAX_ORDER: u32 = 8;

/// Pantelides' algorithm on `sys`. Returns how many equations were
/// differentiated. `continuity[k]` is how many times table `k`'s
/// interpolant is continuously differentiable
/// ([`lsim_ir::interp::Table::continuity`]): a table read may be
/// differentiated once more than that (the highest derivative read is then
/// piecewise continuous, one-sided where it jumps), not more.
pub fn pantelides(sys: &mut Sys, continuity: &[u32]) -> Result<usize, IndexFault> {
    let n_eq0 = sys.eqs.len();
    let mut assign: Vec<usize> = vec![NONE; sys.nodes.len()];
    let unmatched: Vec<usize> = {
        let rows: Vec<Vec<usize>> = (0..n_eq0)
            .map(|e| sys.eqs[e].inc.iter().copied().filter(|&n| sys.highest_unknown(n)).collect())
            .collect();
        let g = Bipartite::from_rows(sys.nodes.len(), &rows);
        let m = hopcroft_karp(&g);
        for e in 0..n_eq0 {
            if m.row[e] != NONE {
                assign[m.row[e]] = e;
            }
        }
        (0..n_eq0).filter(|&e| m.row[e] == NONE).collect()
    };
    let mut differentiated = 0;
    let mut eq_mark: Vec<u32> = vec![0; sys.eqs.len()];
    let mut node_mark: Vec<u32> = vec![0; sys.nodes.len()];
    let mut stamp = 0u32;
    for k in unmatched {
        let mut i = k;
        loop {
            stamp += 1;
            eq_mark.resize(sys.eqs.len(), 0);
            node_mark.resize(sys.nodes.len(), 0);
            assign.resize(sys.nodes.len(), NONE);
            let mut colored_eqs = vec![];
            let mut colored_nodes = vec![];
            if augment(
                sys,
                &mut assign,
                i,
                stamp,
                &mut eq_mark,
                &mut node_mark,
                &mut colored_eqs,
                &mut colored_nodes,
            ) {
                break;
            }
            for &j in &colored_nodes {
                if sys.nodes[j].order + 1 > MAX_ORDER {
                    return Err(IndexFault::TooHigh { eq: i });
                }
                sys.deriv_node(j);
            }
            for &l in &colored_eqs {
                let d =
                    sys.differentiate(l).map_err(|why| IndexFault::Differentiate { eq: l, why })?;
                smooth_enough(&sys.eqs[d].res, d, continuity)?;
                differentiated += 1;
            }
            assign.resize(sys.nodes.len(), NONE);
            for &j in &colored_nodes {
                let a = sys.nodes[j].deriv.expect("just made");
                let e = assign[j];
                assign[a] = sys.eqs[e].derived.expect("its equation was differentiated");
            }
            i = sys.eqs[i].derived.expect("just differentiated");
            if sys.eq_level(i) > MAX_ORDER {
                return Err(IndexFault::TooHigh { eq: i });
            }
        }
    }
    Ok(differentiated)
}

/// Whether every table read of `res` (equation `eq`) is differentiated at
/// most once more than its table is continuously differentiable.
///
/// Why once more is right: the equation differentiated `r` times holds on
/// each side of a point where the table's `r`-th derivative jumps (the
/// derivatives below it are continuous, so the lower equations, the states
/// and the dummy derivatives they decide stay continuous); what it decides
/// (a dummy derivative, a constraint force) may jump there, at an isolated
/// instant, which leaves the integrated states as the original model's.
/// Read once more still, a derivative that jumps would sit in an equation
/// that decides a continuous quantity: the motion would need an impulse
/// the model does not have.
fn smooth_enough(res: &lsim_ir::Expr, eq: usize, continuity: &[u32]) -> Result<(), IndexFault> {
    let mut bad = None;
    res.walk(&mut |x| {
        if let lsim_ir::Expr::Table { table, order, .. } = x {
            let o = u32::from(order[0]) + u32::from(order[1]);
            let c = continuity.get(*table as usize).copied().unwrap_or(0);
            if o > c.saturating_add(1) && bad.is_none() {
                bad =
                    Some(IndexFault::TableNotSmooth { eq, table: *table, order: o, continuity: c });
            }
        }
    });
    bad.map_or(Ok(()), Err)
}

/// Pantelides' augmenting path from equation `i` over the highest
/// unknowns, colouring what it visits; iterative.
#[allow(clippy::too_many_arguments)]
fn augment(
    sys: &Sys,
    assign: &mut [usize],
    i: usize,
    stamp: u32,
    eq_mark: &mut [u32],
    node_mark: &mut [u32],
    colored_eqs: &mut Vec<usize>,
    colored_nodes: &mut Vec<usize>,
) -> bool {
    // frames: (equation, position in its incidence); via[k]: the node that
    // led from frame k to frame k + 1
    let mut stack: Vec<(usize, usize)> = vec![];
    let mut via: Vec<usize> = vec![];
    let enter = |e: usize,
                 eq_mark: &mut [u32],
                 colored_eqs: &mut Vec<usize>,
                 assign: &[usize]|
     -> Option<usize> {
        eq_mark[e] = stamp;
        colored_eqs.push(e);
        sys.eqs[e].inc.iter().copied().find(|&n| sys.highest_unknown(n) && assign[n] == NONE)
    };
    if let Some(n) = enter(i, eq_mark, colored_eqs, assign) {
        assign[n] = i;
        return true;
    }
    stack.push((i, 0));
    while let Some(top) = stack.last_mut() {
        let (e, pos) = *top;
        let inc = &sys.eqs[e].inc;
        if pos < inc.len() {
            top.1 += 1;
            let n = inc[pos];
            if !sys.highest_unknown(n) || node_mark[n] == stamp {
                continue;
            }
            node_mark[n] = stamp;
            colored_nodes.push(n);
            let e2 = assign[n];
            via.push(n);
            if eq_mark[e2] == stamp {
                via.pop();
                continue;
            }
            if let Some(free) = enter(e2, eq_mark, colored_eqs, assign) {
                assign[free] = e2;
                // shift the assignments along the path
                for k in (0..via.len()).rev() {
                    assign[via[k]] = stack[k].0;
                }
                return true;
            }
            stack.push((e2, 0));
        } else {
            stack.pop();
            via.pop();
        }
    }
    false
}

/// How strongly a node is kept a state when there is a choice
/// ([`dummy_derivatives`]' `keep`), weakest first: `stateSelect = never`,
/// `avoid`, the default, the default with a fixed start value, `prefer`,
/// and kept above all ([`KEEP_STRONG`]).
pub const KEEP_NEVER: u8 = 0;
/// `stateSelect = avoid`
pub const KEEP_AVOID: u8 = 1;
/// the default
pub const KEEP_DEFAULT: u8 = 2;
/// the default, with a fixed start value
pub const KEEP_FIXED: u8 = 3;
/// `stateSelect = prefer`
pub const KEEP_PREFER: u8 = 4;
/// `stateSelect = always`, and the continuous part of a state `reinit`
/// restarts: chosen as a dummy only when no regular choice avoids it
pub const KEEP_STRONG: u8 = 5;

/// How far inside the run's limit (`lsim_ir::choice::DEGRADED`: a decided
/// candidate moving a hundred times as much as a state) a choice the
/// model asks for (`stateSelect`, a state a `reinit` restarts) must start:
/// at half of it. The choice by pivoting starts within a tenth of it (the
/// factor 10 of its window).
pub const MARGIN: f64 = 2.0;

/// The widest window a column marked to be demoted before others
/// (`stateSelect`) is chosen in: a fiftieth of the largest entry. A choice
/// the model's marks ask for is taken only where no decided candidate
/// moves more than `1 / WIDE` (50) times as much as a state: half the
/// run's limit, so the run, which stops a choice past 100, does not stop
/// one that starts at the window's edge at its first wobble (a pendulum's
/// turning point, reached a little further on by the integrator's error)
/// at one tolerance and not at another.
pub const WIDE: f64 = MARGIN * lsim_ir::choice::DEGRADED;

/// What [`dummy_derivatives`] chose.
pub struct Selection {
    /// per node: it is a dummy derivative
    pub dummy: Vec<bool>,
    /// per node: it is a state of the reduced model
    pub is_state: Vec<bool>,
    /// each block of the choice, highest derivatives first
    pub sets: Vec<ChosenSet>,
    /// the choices the model asked for that only [`MARGIN`] kept from
    /// being taken
    pub unkept: Vec<Unkept>,
}

/// A choice of states the model asked for (`stateSelect`, a `reinit`) that
/// would be regular within the run's limit but starts outside [`WIDE`],
/// and so was not taken.
#[derive(Clone, Debug)]
pub struct Unkept {
    /// the variables (nodes) marked to be kept as states that it would keep
    /// and the choice taken decides
    pub nodes: Vec<usize>,
    /// the factor it would start at: its decided candidate moving that many
    /// times as much as a state
    pub factor: f64,
}

/// One block of the choice: differentiated equations, the highest
/// derivatives they could decide, and the pivot chosen for each equation
/// (the column whose node became a dummy derivative, demoting the node it
/// is the derivative of).
#[derive(Clone, Debug)]
pub struct ChosenSet {
    /// the equations (rows), in the order they were pivoted
    pub rows: Vec<usize>,
    /// the candidate nodes (columns)
    pub cols: Vec<usize>,
    /// per row, the column chosen for it
    pub pivots: Vec<usize>,
    /// the columns left out of the choice (kept above all) when a regular
    /// choice did without them
    pub masked: Vec<bool>,
    /// whether the model marks a column (`stateSelect`, a `reinit`)
    pub marked: bool,
}

/// Chooses the dummy derivatives after [`pantelides`]. `vals` are node
/// values at the initial point (or generic values when the initial point
/// is not known; or a point of a run, at time `t`, where a run chooses
/// again), `params` the parameter values, `keep[n]` how strongly
/// node `n` is kept a state when there is a choice ([`KEEP_NEVER`] …
/// [`KEEP_STRONG`]). `tables` are the tables' interpolants (empty: none
/// read). `within` is the pivoting's window ([`WITHIN`] at the start).
pub fn dummy_derivatives(
    sys: &Sys,
    t: f64,
    vals: &[f64],
    params: &[f64],
    tables: &[lsim_ir::interp::Table],
    keep: &[u8],
    within: f64,
) -> Result<Selection, IndexFault> {
    let n_nodes = sys.nodes.len();
    let mut dummy = vec![false; n_nodes];
    let mut sets = vec![];
    let mut unkept = vec![];
    // the highest equations obtained by differentiation
    let mut g: Vec<usize> = (0..sys.eqs.len())
        .filter(|&e| sys.eqs[e].derived.is_none() && sys.eqs[e].diff_of.is_some())
        .collect();
    let mut z: Vec<usize> = {
        let mut z: Vec<usize> = g
            .iter()
            .flat_map(|&e| sys.eqs[e].inc.iter().copied())
            .filter(|&n| sys.highest_unknown(n))
            .collect();
        z.sort_unstable();
        z.dedup();
        z
    };
    let env = NodeEnv { t, vals, params, tables };
    while !g.is_empty() {
        // independent groups of equations (sharing candidate columns)
        let mut col_of = vec![NONE; n_nodes];
        for (k, &n) in z.iter().enumerate() {
            col_of[n] = k;
        }
        let keys: Vec<Vec<usize>> = g
            .iter()
            .map(|&e| {
                sys.eqs[e].inc.iter().filter(|&&n| col_of[n] != NONE).map(|&n| col_of[n]).collect()
            })
            .collect();
        let mut chosen_all = vec![];
        for group in groups(z.len(), &keys) {
            let rows: Vec<usize> = group.iter().map(|&k| g[k]).collect();
            let mut cols: Vec<usize> =
                group.iter().flat_map(|&k| keys[k].iter().copied()).collect();
            cols.sort_unstable();
            cols.dedup();
            let col_nodes: Vec<usize> = cols.iter().map(|&c| z[c]).collect();
            let mut m = vec![vec![0.0; cols.len()]; rows.len()];
            // whether the entries move with the solution: they read time,
            // or a variable that is not held between events
            let mut moves = false;
            for (r, &e) in rows.iter().enumerate() {
                for (c, &nd) in col_nodes.iter().enumerate() {
                    if sys.eqs[e].inc.binary_search(&nd).is_ok() {
                        let d = diff(&sys.eqs[e].res, Slot::Var(VarId(nd as u32)));
                        moves |= d.any(&mut |x| matches!(x, lsim_ir::Expr::Time))
                            || crate::system::nodes_of(&d)
                                .iter()
                                .any(|&n| sys.nodes[n].kind != NodeKind::Discrete);
                        m[r][c] = eval(&d, &env);
                    }
                }
            }
            // choosing a dummy demotes the node it is the derivative of:
            // prefer to demote nodes kept least strongly (`keep`), then
            // derivative nodes (keeping declared variables as states),
            // then later variables
            let pref: Vec<(u8, u32, u32)> = col_nodes
                .iter()
                .map(|&nd| {
                    let i = sys.nodes[nd].integral.unwrap_or(nd);
                    (u8::MAX - keep[i], sys.nodes[i].order, sys.nodes[nd].var.0)
                })
                .collect();
            // the columns' stateSelect (a fixed start is a preference
            // within the default, not a class of its own)
            let classes: Vec<u8> = col_nodes
                .iter()
                .map(|&nd| match keep[sys.nodes[nd].integral.unwrap_or(nd)] {
                    KEEP_FIXED => KEEP_DEFAULT,
                    k => k,
                })
                .collect();
            let classes = (classes.iter().any(|&k| k != KEEP_DEFAULT)).then_some(classes);
            // a column kept above all (a restarted state's, `always`) is
            // chosen only when no regular choice avoids it
            let strong: Vec<bool> = col_nodes
                .iter()
                .map(|&nd| keep[sys.nodes[nd].integral.unwrap_or(nd)] >= KEEP_STRONG)
                .collect();
            let none = vec![false; cols.len()];
            // the choice the model's marks ask for, its window `wide` (a
            // column marked to be kept competes within `wide` of the
            // largest entry): taken only where it is regular well enough
            // for the run, where the entries move no decided candidate
            // moving more than 1 / wide times as much as a state (a
            // parameter's scale is no degeneracy)
            let marked = |wide: f64| -> Option<(Vec<usize>, Vec<bool>)> {
                let regular = |sel: &Vec<usize>, left_out: &[bool]| {
                    !moves || {
                        let mut mm = m.clone();
                        let ps = lsim_ir::choice::pivot_state(
                            sel,
                            left_out,
                            &mut mm,
                            &mut Default::default(),
                        );
                        ps.worst_exchange() * wide <= 1.0
                    }
                };
                if strong.contains(&true) {
                    // the strong columns left out, the classes heeded and
                    // then not (regular as judged by the whole row: a choice
                    // that avoids the strong columns only by a pivot at
                    // round-off is none)
                    let left_out: Vec<Vec<f64>> = m
                        .iter()
                        .map(|row| {
                            row.iter()
                                .zip(&strong)
                                .map(|(&x, &s)| if s { 0.0 } else { x })
                                .collect()
                        })
                        .collect();
                    let scales: Vec<f64> = m
                        .iter()
                        .map(|row| row.iter().fold(0.0f64, |a, x| a.max(x.abs())))
                        .collect();
                    let with = [classes.as_deref(), None];
                    for cl in &with[..if classes.is_some() { 2 } else { 1 }] {
                        let sel = select_columns_among(
                            &left_out,
                            &pref,
                            Some(&scales),
                            *cl,
                            wide,
                            within,
                        )
                        .filter(|sel| regular(sel, &strong));
                        if let Some(sel) = sel {
                            return Some((sel, strong.clone()));
                        }
                    }
                }
                if classes.is_some() {
                    let sel =
                        select_columns_among(&m, &pref, None, classes.as_deref(), wide, within)
                            .filter(|sel| regular(sel, &none));
                    if let Some(sel) = sel {
                        return Some((sel, none.clone()));
                    }
                }
                None
            };
            // within the margin, else the choice by pivoting alone
            let selected = marked(WIDE).or_else(|| {
                select_columns_among(&m, &pref, None, None, WIDE, within).map(|s| (s, none.clone()))
            });
            // a choice the model asks for that only the margin kept from
            // being taken (regular within the run's limit): the variables
            // it would keep that are decided now, and its factor, are told
            if moves
                && (classes.is_some() || strong.contains(&true))
                && let (Some((sel, _)), Some((limit, left_out))) =
                    (&selected, marked(lsim_ir::choice::DEGRADED))
            {
                let nodes: Vec<usize> = sel
                    .iter()
                    .filter(|c| !limit.contains(c))
                    .map(|&c| sys.nodes[col_nodes[c]].integral.unwrap_or(col_nodes[c]))
                    .filter(|&i| keep[i] >= KEEP_PREFER)
                    .collect();
                if !nodes.is_empty() {
                    let mut mm = m.clone();
                    let ps = lsim_ir::choice::pivot_state(
                        &limit,
                        &left_out,
                        &mut mm,
                        &mut Default::default(),
                    );
                    unkept.push(Unkept { nodes, factor: ps.worst_exchange() });
                }
            }
            let marked = classes.is_some() || strong.contains(&true);
            match selected {
                Some((sel, masked)) => {
                    chosen_all.extend(sel.iter().map(|&c| col_nodes[c]));
                    sets.push(ChosenSet { rows, cols: col_nodes, pivots: sel, masked, marked });
                }
                None => {
                    return Err(IndexFault::Singular { eqs: rows.clone(), nodes: col_nodes });
                }
            }
        }
        for &nd in &chosen_all {
            dummy[nd] = true;
        }
        let mut g_next: Vec<usize> = g
            .iter()
            .filter_map(|&e| sys.eqs[e].diff_of)
            .filter(|&p| sys.eqs[p].diff_of.is_some())
            .collect();
        g_next.sort_unstable();
        g_next.dedup();
        let mut z_next: Vec<usize> =
            chosen_all.iter().filter_map(|&nd| sys.nodes[nd].integral).collect();
        z_next.sort_unstable();
        z_next.dedup();
        g = g_next;
        z = z_next;
    }
    let is_state = (0..n_nodes)
        .map(|n| {
            let nd = &sys.nodes[n];
            nd.kind == NodeKind::Unknown && nd.deriv.is_some_and(|d| !dummy[d])
        })
        .collect();
    Ok(Selection { dummy, is_state, sets, unkept })
}

/// Chooses as many columns of `m` (rows × columns) as it has rows, so that
/// they form a regular matrix: Gaussian elimination row by row, picking in
/// each row, among the columns within a factor 10 of the row's largest
/// entry, the most preferred. `None` when the rows are dependent.
pub fn select_columns<P: Ord>(m: &[Vec<f64>], pref: &[P]) -> Option<Vec<usize>> {
    select_columns_within(m, pref, None, None, WIDE)
}

/// [`select_columns`] with each row's size given (`scales`: its largest
/// entry before some columns were left out of `m`), against which a row
/// left with nothing but round-off is dependent, and the columns'
/// `classes` (their `stateSelect`, most demotable lowest): a column then
/// competes within a factor 10 only with the columns of its class or a
/// lower one, and within a factor `wide` with every column ([`WIDE`]: half
/// the run's limit), so a variable marked `prefer` stays a state where the
/// choice that keeps it starts well inside the run's limit.
pub fn select_columns_within<P: Ord>(
    m: &[Vec<f64>],
    pref: &[P],
    scales: Option<&[f64]>,
    classes: Option<&[u8]>,
    wide: f64,
) -> Option<Vec<usize>> {
    select_columns_among(m, pref, scales, classes, wide, WITHIN)
}

/// The window the choice by pivoting takes a preferred column within at
/// the start: a tenth of the row's largest entry.
pub const WITHIN: f64 = 0.1;

/// [`select_columns_within`] with the window `within` (a share of the
/// row's largest entry) in place of [`WITHIN`]: where a run chooses again,
/// its window is its switch threshold's, so the choice it leaves (just
/// past that threshold) is not taken again.
pub fn select_columns_among<P: Ord>(
    m: &[Vec<f64>],
    pref: &[P],
    scales: Option<&[f64]>,
    classes: Option<&[u8]>,
    wide: f64,
    within: f64,
) -> Option<Vec<usize>> {
    let rows = m.len();
    let cols = pref.len();
    let mut w: Vec<Vec<f64>> = m.to_vec();
    let mut used = vec![false; cols];
    let mut chosen = vec![];
    for r in 0..rows {
        let scale =
            scales.map_or_else(|| m[r].iter().fold(0.0f64, |a, x| a.max(x.abs())), |s| s[r]);
        let amax = (0..cols).filter(|&c| !used[c]).fold(0.0f64, |a, c| a.max(w[r][c].abs()));
        if amax.is_nan() || amax <= 1e-10 * scale.max(f64::MIN_POSITIVE) || !amax.is_finite() {
            return None;
        }
        let window = |c: usize| match classes {
            None => within * amax,
            Some(k) => {
                let peers = (0..cols)
                    .filter(|&o| !used[o] && k[o] <= k[c])
                    .fold(0.0f64, |a, o| a.max(w[r][o].abs()));
                (within * peers).max(wide * amax)
            }
        };
        let c = (0..cols)
            .filter(|&c| !used[c] && w[r][c].abs() >= window(c))
            .max_by(|&a, &b| pref[a].cmp(&pref[b]).then(w[r][a].abs().total_cmp(&w[r][b].abs())))
            .expect("the largest entry qualifies");
        used[c] = true;
        chosen.push(c);
        let pivot_row = w[r].clone();
        for row in w.iter_mut().skip(r + 1) {
            let f = row[c] / pivot_row[c];
            if f != 0.0 {
                for (x, p) in row.iter_mut().zip(&pivot_row) {
                    *x -= f * p;
                }
            }
        }
    }
    Some(chosen)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_selection_pivots_and_prefers() {
        // x² + y² = L², differentiated: [2x 2y]; at x = 0 only y can go
        let m = vec![vec![0.0, -2.0]];
        let pref = [(false, 0), (false, 1)];
        assert_eq!(select_columns(&m, &pref), Some(vec![1]));
        // equal magnitudes: the preferred (free start) column is chosen
        let m = vec![vec![1.0, -1.0]];
        assert_eq!(select_columns(&m, &[(true, 0), (false, 1)]), Some(vec![0]));
        assert_eq!(select_columns(&m, &[(false, 0), (false, 1)]), Some(vec![1]));
        // dependent rows
        let m = vec![vec![1.0, 2.0], vec![2.0, 4.0]];
        assert_eq!(select_columns(&m, &[(false, 0), (false, 1)]), None);
    }

    #[test]
    fn a_class_to_demote_first_is_chosen_within_a_fiftieth() {
        // the rod at 1.5 rad: [2x 2y], y's entry 7 % of x's; pivoting alone
        // decides x (keeps y)
        let m = vec![vec![1.99, 0.14]];
        let pref = [(u8::MAX - KEEP_PREFER, 0), (u8::MAX - KEEP_DEFAULT, 1)];
        assert_eq!(select_columns_within(&m, &pref, None, None, WIDE), Some(vec![0]));
        // x marked prefer: y is decided (x stays a state)
        let classes = [KEEP_PREFER, KEEP_DEFAULT];
        assert_eq!(select_columns_within(&m, &pref, None, Some(&classes), WIDE), Some(vec![1]));
        // but not below a fiftieth (the rod at atan 99: within the run's
        // limit of a hundredth, outside its margin): x is decided after all
        let m = vec![vec![1.98, 0.02]];
        assert_eq!(select_columns_within(&m, &pref, None, Some(&classes), WIDE), Some(vec![0]));
        let at_limit = lsim_ir::choice::DEGRADED;
        assert_eq!(select_columns_within(&m, &pref, None, Some(&classes), at_limit), Some(vec![1]));
        // the same classes: as without them
        let same = [KEEP_DEFAULT, KEEP_DEFAULT];
        let m = vec![vec![1.99, 0.14]];
        assert_eq!(select_columns_within(&m, &pref, None, Some(&same), WIDE), Some(vec![0]));
    }
}
