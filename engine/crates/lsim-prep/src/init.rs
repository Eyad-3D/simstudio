//! The initialisation system (DESIGN.md, *Preparation pipeline*, step 9):
//! which equations decide the start of a run.
//!
//! Its unknowns are every unknown node of the index-reduced system, the
//! states included; its equations are the model's (with every derivative
//! index reduction made), the initial equations, and start values:
//!
//! 1. the model's and the initial equations must all be matched, or the
//!    initial equations contradict the model (`INIT-OVER`);
//! 2. the `fixed` start values are added one at a time by augmenting
//!    paths, in the order of the variables; one that cannot be matched is
//!    redundant — what it fixes is decided already (a capacitor directly
//!    across a source, the second of two rigidly coupled inertias) — and
//!    is dropped (checked against the solution at preparation);
//! 3. degrees of freedom left over take the start values of the remaining
//!    states (as Modelica tools do), then of the other variables.
//!
//! Modes are not held at the start: the relation stands in for its mode.

use crate::flatten::Extras;
use crate::graph::{Bipartite, Matching, NONE, augment, hopcroft_karp, over_part};
use crate::system::{NodeKind, Sys, node};
use lsim_ir::expr::Expr;
use lsim_ir::flat::{FlatSystem, Origin, OriginKind, VarId};
use lsim_ir::prepared::{AliasEntry, AliasTarget};
use std::collections::HashMap;

/// What a row of the initialisation system is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RowKind {
    /// equation k of the system
    Model(usize),
    /// an initial equation
    Initial,
    /// a fixed start value
    Fixed(VarId),
    /// a start value taken to fill a degree of freedom
    Assumed(VarId),
}

/// The initialisation system over nodes.
pub struct InitBuild {
    /// residuals
    pub eqs: Vec<Expr>,
    /// their origins
    pub origins: Vec<Origin>,
    /// what each one is
    pub kinds: Vec<RowKind>,
    /// the unknown nodes
    pub unknowns: Vec<usize>,
    /// fixed start values that could not be used: (variable, its node,
    /// the start expression)
    pub dropped: Vec<(VarId, usize, Expr)>,
    /// fixed start values of variables replaced by constants: (variable,
    /// start, constant)
    pub constant_conflicts: Vec<(VarId, f64, f64)>,
}

/// Where the initialisation system fails.
pub struct InitFault {
    /// rows (into the mandatory equations) of the over-determined part
    pub rows: Vec<usize>,
    /// their origins
    pub origins: Vec<Origin>,
}

/// A start value's origin: the variable's instance, labelled.
pub fn start_origin(flat: &FlatSystem, v: VarId) -> Origin {
    let var = flat.var(v);
    let path = &flat.instance(var.instance).path;
    let local = var.name.strip_prefix(path.as_str()).unwrap_or(&var.name).trim_start_matches('.');
    Origin {
        instance: var.instance,
        kind: OriginKind::Component { index: usize::MAX },
        label: Some(format!("the start value of {local}")),
    }
}

/// Builds the initialisation system. `model_res[k]` is equation k's
/// residual with modes replaced by their relations; `initial` the initial
/// equations over nodes.
#[allow(clippy::too_many_arguments)]
pub fn build(
    sys: &Sys,
    flat: &FlatSystem,
    extras: &Extras,
    aliases: &[AliasEntry],
    model_res: Vec<Expr>,
    initial: Vec<(Expr, Origin)>,
) -> Result<InitBuild, InitFault> {
    let n_nodes = sys.nodes.len();
    let unknowns: Vec<usize> =
        (0..n_nodes).filter(|&n| sys.nodes[n].kind == NodeKind::Unknown).collect();
    let mut col_of = vec![NONE; n_nodes];
    for (c, &n) in unknowns.iter().enumerate() {
        col_of[n] = c;
    }
    let cols_of = |e: &Expr| -> Vec<usize> {
        crate::system::nodes_of(e)
            .into_iter()
            .filter(|&n| col_of[n] != NONE)
            .map(|n| col_of[n])
            .collect()
    };
    let mut eqs = vec![];
    let mut origins = vec![];
    let mut kinds = vec![];
    for (k, r) in model_res.into_iter().enumerate() {
        eqs.push(r);
        origins.push(sys.eqs[k].origin.clone());
        kinds.push(RowKind::Model(k));
    }
    for (r, o) in initial {
        eqs.push(r);
        origins.push(o);
        kinds.push(RowKind::Initial);
    }
    // the mandatory rows, matching non-state columns first so the states
    // stay free for their start values
    let state_like = |c: usize| sys.nodes[unknowns[c]].deriv.is_some();
    let rows: Vec<Vec<usize>> = eqs
        .iter()
        .map(|e| {
            let mut r = cols_of(e);
            r.sort_by_key(|&c| (state_like(c), c));
            r
        })
        .collect();
    let mut g = Bipartite::from_rows(unknowns.len(), &rows);
    let m0 = hopcroft_karp(&g);
    if m0.row.contains(&NONE) {
        let (rows, _) = over_part(&g, &m0);
        return Err(InitFault {
            origins: rows.iter().map(|&r| origins[r].clone()).collect(),
            rows,
        });
    }
    // the start-value candidates, by variable
    let start_of = |v: usize| -> Option<Expr> {
        extras.start.get(v).cloned().flatten().or_else(|| flat.vars[v].start.map(Expr::Const))
    };
    let mut alias_of: HashMap<u32, AliasTarget> = HashMap::new();
    for a in aliases {
        alias_of.insert(a.var.0, a.target);
    }
    let mut constant_conflicts = vec![];
    let mut fixed_rows: Vec<(VarId, usize, Expr)> = vec![];
    for (i, var) in flat.vars.iter().enumerate() {
        if !var.fixed || var.kind != lsim_ir::VarKind::Continuous {
            continue;
        }
        let Some(start) = start_of(i) else { continue };
        let (target_node, value) = match (sys.base[i], alias_of.get(&(i as u32))) {
            (Some(n), _) => (n, start),
            (None, Some(AliasTarget::Var { var: r, negated })) => match sys.base[r.0 as usize] {
                Some(n) => (n, if *negated { -start } else { start }),
                None => continue,
            },
            (None, Some(AliasTarget::Const(c))) => {
                let s = var.start.unwrap_or(f64::NAN);
                if (s - c).abs() > 1e-9 * (1.0 + c.abs()) {
                    constant_conflicts.push((VarId(i as u32), s, *c));
                }
                continue;
            }
            (None, None) => continue,
        };
        if sys.nodes[target_node].kind != NodeKind::Unknown {
            continue;
        }
        fixed_rows.push((VarId(i as u32), target_node, value));
    }
    // grow the matching row by row: a matched row stays matched
    let mut m = m0;
    let mut seen = vec![0u32; unknowns.len()];
    let mut stamp = 0u32;
    let mut dropped = vec![];
    let mut has_start_row = vec![false; n_nodes];
    let mut try_row = |c: usize, g: &mut Bipartite, m: &mut Matching| -> bool {
        g.push_row(&[c]);
        m.row.push(NONE);
        let r = m.row.len() - 1;
        if m.col[c] == NONE {
            m.row[r] = c;
            m.col[c] = r;
            return true;
        }
        stamp += 1;
        if augment(g, m, r, &mut seen, stamp) {
            true
        } else {
            g.pop_row();
            m.row.pop();
            false
        }
    };
    for (v, n, value) in fixed_rows {
        if try_row(col_of[n], &mut g, &mut m) {
            eqs.push(node(n) - value);
            origins.push(start_origin(flat, v));
            kinds.push(RowKind::Fixed(v));
            has_start_row[n] = true;
        } else {
            dropped.push((v, n, value));
        }
    }
    // degrees of freedom left: the start values of states, lowest order
    // first, then of anything else
    let mut free = m.col.iter().filter(|&&r| r == NONE).count();
    if free > 0 {
        let mut candidates: Vec<usize> = unknowns
            .iter()
            .copied()
            .filter(|&n| sys.nodes[n].deriv.is_some() && !has_start_row[n])
            .collect();
        candidates.sort_by_key(|&n| (sys.nodes[n].order, n));
        let rest: Vec<usize> = unknowns
            .iter()
            .copied()
            .filter(|&n| sys.nodes[n].deriv.is_none() && !has_start_row[n])
            .collect();
        for n in candidates.into_iter().chain(rest) {
            if free == 0 {
                break;
            }
            if m.col[col_of[n]] != NONE && sys.nodes[n].deriv.is_none() {
                continue;
            }
            if try_row(col_of[n], &mut g, &mut m) {
                let nd = &sys.nodes[n];
                let guess = if nd.order == 0 {
                    start_of(nd.var.0 as usize).unwrap_or(Expr::Const(0.0))
                } else {
                    Expr::Const(0.0)
                };
                eqs.push(node(n) - guess);
                origins.push(start_origin(flat, nd.var));
                kinds.push(RowKind::Assumed(nd.var));
                free -= 1;
            }
        }
    }
    Ok(InitBuild { eqs, origins, kinds, unknowns, dropped, constant_conflicts })
}
