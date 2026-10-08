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
//!    initial point, preferring to keep as states the variables whose start
//!    values are fixed. The choice is static: if the initial point makes
//!    every choice singular (a pendulum started exactly horizontal in the
//!    coordinate the selection needs), the preparation says so instead of
//!    switching states during the run (risk R3).

use crate::graph::{Bipartite, NONE, groups, hopcroft_karp};
use crate::symbolic::diff;
use crate::system::{NodeEnv, NodeKind, Sys};
use lsim_ir::eval::eval;
use lsim_ir::prepared::Slot;
use lsim_ir::{Origin, VarId};

/// Why index reduction stopped.
#[derive(Clone, Debug)]
pub enum IndexFault {
    /// an equation had to be differentiated more often than any physical
    /// model needs (the system is structurally singular in a way the
    /// earlier checks missed)
    TooHigh {
        /// the equation
        origin: Origin,
    },
    /// an equation could not be differentiated
    Differentiate {
        /// the equation
        origin: Origin,
        /// why
        why: String,
    },
    /// no static choice of dummy derivatives is regular at the initial point
    Singular {
        /// the differentiated equations involved
        origins: Vec<Origin>,
        /// the candidate nodes
        nodes: Vec<usize>,
    },
}

/// The deepest differentiation allowed: mechanical models reach index 3.
const MAX_ORDER: u32 = 8;

/// Pantelides' algorithm on `sys`. Returns how many equations were
/// differentiated.
pub fn pantelides(sys: &mut Sys) -> Result<usize, IndexFault> {
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
                    return Err(IndexFault::TooHigh { origin: sys.eqs[i].origin.clone() });
                }
                sys.deriv_node(j);
            }
            for &l in &colored_eqs {
                sys.differentiate(l).map_err(|why| IndexFault::Differentiate {
                    origin: sys.eqs[l].origin.clone(),
                    why,
                })?;
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
                return Err(IndexFault::TooHigh { origin: sys.eqs[i].origin.clone() });
            }
        }
    }
    Ok(differentiated)
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

/// What [`dummy_derivatives`] chose.
pub struct Selection {
    /// per node: it is a dummy derivative
    pub dummy: Vec<bool>,
    /// per node: it is a state of the reduced model
    pub is_state: Vec<bool>,
}

/// Chooses the dummy derivatives after [`pantelides`]. `vals` are node
/// values at the initial point (or generic values when the initial point
/// is not known), `params` the parameter values, `fixed[n]` whether node
/// `n`'s start value is fixed (such variables are kept as states when
/// there is a choice).
pub fn dummy_derivatives(
    sys: &Sys,
    vals: &[f64],
    params: &[f64],
    fixed: &[bool],
) -> Result<Selection, IndexFault> {
    let n_nodes = sys.nodes.len();
    let mut dummy = vec![false; n_nodes];
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
    let env = NodeEnv { t: 0.0, vals, params };
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
            for (r, &e) in rows.iter().enumerate() {
                for (c, &nd) in col_nodes.iter().enumerate() {
                    if sys.eqs[e].inc.binary_search(&nd).is_ok() {
                        let d = diff(&sys.eqs[e].res, Slot::Var(VarId(nd as u32)));
                        m[r][c] = eval(&d, &env);
                    }
                }
            }
            // choosing a dummy demotes the node it is the derivative of:
            // prefer to demote nodes without a fixed start, then derivative
            // nodes (keeping declared variables as states), then later
            // variables
            let pref: Vec<(bool, u32, u32)> = col_nodes
                .iter()
                .map(|&nd| {
                    let i = sys.nodes[nd].integral.unwrap_or(nd);
                    (!fixed[i], sys.nodes[i].order, sys.nodes[nd].var.0)
                })
                .collect();
            match select_columns(&m, &pref) {
                Some(sel) => chosen_all.extend(sel.into_iter().map(|c| col_nodes[c])),
                None => {
                    return Err(IndexFault::Singular {
                        origins: rows.iter().map(|&e| sys.eqs[e].origin.clone()).collect(),
                        nodes: col_nodes,
                    });
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
    Ok(Selection { dummy, is_state })
}

/// Chooses as many columns of `m` (rows × columns) as it has rows, so that
/// they form a regular matrix: Gaussian elimination row by row, picking in
/// each row, among the columns within a factor 10 of the row's largest
/// entry, the most preferred. `None` when the rows are dependent.
pub fn select_columns<P: Ord>(m: &[Vec<f64>], pref: &[P]) -> Option<Vec<usize>> {
    let rows = m.len();
    let cols = pref.len();
    let mut w: Vec<Vec<f64>> = m.to_vec();
    let mut used = vec![false; cols];
    let mut chosen = vec![];
    for r in 0..rows {
        let scale = m[r].iter().fold(0.0f64, |a, x| a.max(x.abs()));
        let amax = (0..cols).filter(|&c| !used[c]).fold(0.0f64, |a, c| a.max(w[r][c].abs()));
        if amax.is_nan() || amax <= 1e-10 * scale.max(f64::MIN_POSITIVE) || !amax.is_finite() {
            return None;
        }
        let c = (0..cols)
            .filter(|&c| !used[c] && w[r][c].abs() >= 0.1 * amax)
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
}
