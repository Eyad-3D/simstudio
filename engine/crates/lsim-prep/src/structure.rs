//! Structural analysis: which equation determines which unknown, in which
//! order, and which unknowns must stay iteration variables.
//!
//! 1. The unknowns are every remaining continuous variable that is not a
//!    state, plus each state's derivative (states and parameters are known
//!    at any instant).
//! 2. A maximum matching pairs equations with unknowns. When it is not
//!    perfect the model is structurally singular; the alternating-path sets
//!    from the unmatched equations (over-determined) and unknowns
//!    (under-determined), a Dulmage–Mendelsohn decomposition, become a
//!    plain-language message naming the parts.
//! 3. Tarjan's algorithm orders the strongly connected blocks, dependencies
//!    first (block-lower-triangular form).
//! 4. A one-equation block whose equation is affine in its unknown becomes
//!    an explicit assignment; any other block keeps its unknowns as
//!    iteration variables with its equations as residuals.
//!
//! Work package 2 adds Hopcroft–Karp matching for large models, tearing of
//! larger blocks, Pantelides index reduction with dummy derivatives (a
//! perfect matching that fails only because states are constrained), and
//! numerical pivot checks of solved equations.

use crate::symbolic::{simplify, solve_for};
use lsim_ir::component::Library;
use lsim_ir::expr::{CmpOp, Expr};
use lsim_ir::flat::{FlatSystem, OriginKind, VarId};
use lsim_ir::prepared::*;
use lsim_ir::{Diagnostic, VarKind};
use std::collections::BTreeSet;

/// Options for causalisation.
#[derive(Clone, Debug, Default)]
pub struct CausalOptions {
    /// keep every block implicit (iteration variables and residuals), even
    /// when it could be solved explicitly: exercises the DAE path
    pub force_implicit: bool,
}

/// Kuhn's augmenting-path matching (iterative), greedy start.
pub fn max_matching(n_unk: usize, inc: &[Vec<usize>]) -> (Vec<Option<usize>>, Vec<Option<usize>>) {
    let mut match_eq = vec![None; inc.len()];
    let mut match_unk = vec![None; n_unk];
    for (e, us) in inc.iter().enumerate() {
        if let Some(&u) = us.iter().find(|&&u| match_unk[u].is_none()) {
            match_eq[e] = Some(u);
            match_unk[u] = Some(e);
        }
    }
    let mut visited = vec![usize::MAX; n_unk];
    for e0 in 0..inc.len() {
        if match_eq[e0].is_some() {
            continue;
        }
        let mut stack: Vec<(usize, usize)> = vec![(e0, 0)];
        let mut path: Vec<usize> = vec![];
        while let Some(top) = stack.last_mut() {
            let (e, pos) = (top.0, top.1);
            if pos < inc[e].len() {
                top.1 += 1;
                let u = inc[e][pos];
                if visited[u] == e0 {
                    continue;
                }
                visited[u] = e0;
                path.push(u);
                match match_unk[u] {
                    None => {
                        for (i, &(eq, _)) in stack.iter().enumerate() {
                            match_eq[eq] = Some(path[i]);
                            match_unk[path[i]] = Some(eq);
                        }
                        break;
                    }
                    Some(e2) => stack.push((e2, 0)),
                }
            } else {
                stack.pop();
                path.pop();
            }
        }
    }
    (match_eq, match_unk)
}

/// Strongly connected components of a directed graph, each component
/// after every component it has an edge to (dependencies first).
pub fn scc(adj: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let n = adj.len();
    let mut index = vec![usize::MAX; n];
    let mut low = vec![0; n];
    let mut on_stack = vec![false; n];
    let mut s: Vec<usize> = vec![];
    let mut out = vec![];
    let mut next = 0;
    for root in 0..n {
        if index[root] != usize::MAX {
            continue;
        }
        let mut call: Vec<(usize, usize)> = vec![(root, 0)];
        index[root] = next;
        low[root] = next;
        next += 1;
        s.push(root);
        on_stack[root] = true;
        while let Some(top) = call.last_mut() {
            let (v, pos) = (top.0, top.1);
            if pos < adj[v].len() {
                top.1 += 1;
                let w = adj[v][pos];
                if index[w] == usize::MAX {
                    index[w] = next;
                    low[w] = next;
                    next += 1;
                    s.push(w);
                    on_stack[w] = true;
                    call.push((w, 0));
                } else if on_stack[w] {
                    low[v] = low[v].min(index[w]);
                }
            } else {
                call.pop();
                if low[v] == index[v] {
                    let mut comp = vec![];
                    loop {
                        let w = s.pop().expect("v is on the stack");
                        on_stack[w] = false;
                        comp.push(w);
                        if w == v {
                            break;
                        }
                    }
                    out.push(comp);
                }
                if let Some(parent) = call.last() {
                    low[parent.0] = low[parent.0].min(low[v]);
                }
            }
        }
    }
    out
}

fn incidence(e: &Expr, slot_of: &dyn Fn(Slot) -> Option<usize>, out: &mut BTreeSet<usize>) {
    e.walk(&mut |x| {
        let s = match x {
            Expr::Var(v) => Slot::Var(*v),
            Expr::Der(v) => Slot::Der(*v),
            _ => return,
        };
        if let Some(i) = slot_of(s) {
            out.insert(i);
        }
    });
}

fn local_name(flat: &FlatSystem, v: VarId) -> String {
    let var = flat.var(v);
    let path = &flat.instance(var.instance).path;
    let local = var.name.strip_prefix(path.as_str()).unwrap_or(&var.name).trim_start_matches('.');
    format!("{local} of {}", flat.instance_name(var.instance))
}

fn slot_name(flat: &FlatSystem, s: Slot) -> String {
    match s {
        Slot::Var(v) => local_name(flat, v),
        Slot::Der(v) => format!("the rate of change of {}", local_name(flat, v)),
    }
}

fn equation_words(flat: &FlatSystem, o: &lsim_ir::Origin) -> String {
    match (&o.kind, &o.label) {
        (OriginKind::Component { .. }, Some(l)) => {
            format!("{} ({l})", flat.instance_name(o.instance))
        }
        (OriginKind::Component { index }, None) => {
            format!("{} (equation {})", flat.instance_name(o.instance), index + 1)
        }
        (OriginKind::ConnectionAcross { ports } | OriginKind::ConnectionThrough { ports }, _) => {
            format!("the connection of {}", ports.join(", "))
        }
        (OriginKind::Unconnected { port }, _) => format!("the free port {port}"),
        (OriginKind::SignalLink { input, output }, _) => format!("the link {output} → {input}"),
    }
}

fn parts_of<'a>(
    flat: &FlatSystem,
    origins: impl Iterator<Item = &'a lsim_ir::Origin>,
) -> Vec<String> {
    let mut set = BTreeSet::new();
    for o in origins {
        if o.instance.0 != 0 {
            set.insert(flat.instance(flat.top_part(o.instance)).path.clone());
        }
    }
    set.into_iter().collect()
}

/// Turns the flat system (aliases removed) into the prepared model.
pub fn causalize(
    flat: FlatSystem,
    aliases: Vec<AliasEntry>,
    opts: &CausalOptions,
    lib: &Library,
) -> Result<PreparedModel, Vec<Diagnostic>> {
    let _ = lib;
    let n = flat.vars.len();
    let mut eliminated = vec![false; n];
    for a in &aliases {
        eliminated[a.var.0 as usize] = true;
    }
    let mut is_state = vec![false; n];
    for e in &flat.equations {
        for side in [&e.lhs, &e.rhs] {
            side.walk(&mut |x| {
                if let Expr::Der(v) = x {
                    is_state[v.0 as usize] = true;
                }
            });
        }
    }
    let states: Vec<VarId> = (0..n).filter(|&i| is_state[i]).map(|i| VarId(i as u32)).collect();
    let discretes: Vec<VarId> = (0..n)
        .filter(|&i| !eliminated[i] && flat.vars[i].kind == VarKind::Discrete)
        .map(|i| VarId(i as u32))
        .collect();
    let mut unknowns: Vec<Slot> = vec![];
    let mut index_of = std::collections::HashMap::new();
    for i in 0..n {
        if eliminated[i] || flat.vars[i].kind == VarKind::Discrete {
            continue;
        }
        let s = if is_state[i] { Slot::Der(VarId(i as u32)) } else { Slot::Var(VarId(i as u32)) };
        index_of.insert(s, unknowns.len());
        unknowns.push(s);
    }
    let slot_of = |s: Slot| index_of.get(&s).copied();
    let inc: Vec<Vec<usize>> = flat
        .equations
        .iter()
        .map(|e| {
            let mut set = BTreeSet::new();
            incidence(&e.lhs, &slot_of, &mut set);
            incidence(&e.rhs, &slot_of, &mut set);
            set.into_iter().collect()
        })
        .collect();

    let (match_eq, match_unk) = max_matching(unknowns.len(), &inc);
    let unmatched_eqs: Vec<usize> = (0..inc.len()).filter(|&e| match_eq[e].is_none()).collect();
    let unmatched_unk: Vec<usize> =
        (0..unknowns.len()).filter(|&u| match_unk[u].is_none()).collect();
    if !unmatched_eqs.is_empty() || !unmatched_unk.is_empty() {
        return Err(structural_diagnostics(
            &flat,
            &unknowns,
            &inc,
            &match_eq,
            &match_unk,
            &unmatched_eqs,
            &unmatched_unk,
        ));
    }

    // block-lower-triangular order
    let adj: Vec<Vec<usize>> = (0..inc.len())
        .map(|e| inc[e].iter().filter_map(|&u| match_unk[u]).filter(|&e2| e2 != e).collect())
        .collect();
    let blocks = scc(&adj);

    let mut assignments = vec![];
    let mut algebraics = vec![];
    let mut residuals = vec![];
    let mut largest = 0;
    for b in &blocks {
        largest = largest.max(b.len());
        if b.len() == 1 && !opts.force_implicit {
            let e = &flat.equations[b[0]];
            let slot = unknowns[match_eq[b[0]].expect("perfect matching")];
            let r = simplify(e.lhs.clone() - e.rhs.clone());
            if let Some(sol) = solve_for(&r, slot) {
                assignments.push(Assignment { target: slot, expr: sol, origin: e.origin.clone() });
                continue;
            }
        }
        for &ei in b {
            let e = &flat.equations[ei];
            algebraics.push(unknowns[match_eq[ei].expect("perfect matching")]);
            residuals.push(Residual {
                expr: simplify(e.lhs.clone() - e.rhs.clone()),
                origin: e.origin.clone(),
            });
        }
    }

    // events
    let mut zero_crossings = vec![];
    let mut whens = vec![];
    let mut diags = vec![];
    for w in &flat.whens {
        let (expr, direction) = match &w.condition {
            Expr::Compare(op, a, b) => (
                simplify((**a).clone() - (**b).clone()),
                match op {
                    CmpOp::Gt | CmpOp::Ge => Direction::Rising,
                    CmpOp::Lt | CmpOp::Le => Direction::Falling,
                },
            ),
            _ => {
                diags.push(Diagnostic::error(
                    "NOT-YET",
                    format!(
                        "{}: a when-condition other than one comparison comes with work package 2.",
                        flat.instance_name(w.origin.instance)
                    ),
                ));
                continue;
            }
        };
        for (v, _) in &w.assign {
            if flat.var(*v).kind != VarKind::Discrete {
                diags.push(Diagnostic::error(
                    "WHEN-CONTINUOUS",
                    format!(
                        "{}: an event assigns {}, which is not a discrete variable.",
                        flat.instance_name(w.origin.instance),
                        local_name(&flat, *v)
                    ),
                ));
            }
        }
        zero_crossings.push(ZeroCrossing { expr, origin: w.origin.clone() });
        whens.push(PreparedWhen {
            crossing: zero_crossings.len() - 1,
            direction,
            assign: w.assign.clone(),
            origin: w.origin.clone(),
        });
    }
    // relations outside noEvent need event handling (modes, zero
    // crossings); inside noEvent they are evaluated as they stand
    fn event_relation(e: &Expr) -> bool {
        match e {
            Expr::NoEvent(_) => false,
            Expr::Compare(..) => true,
            Expr::Call(lsim_ir::Builtin::Abs | lsim_ir::Builtin::Sign, _) => true,
            other => other.children().into_iter().any(event_relation),
        }
    }
    let has_event_relations =
        flat.equations.iter().any(|e| event_relation(&e.lhs) || event_relation(&e.rhs));
    if has_event_relations {
        diags.push(Diagnostic::error(
            "NOT-YET",
            "relations in equations outside noEvent (if-expressions, abs, sign: their events) come with work packages 2 and 3.",
        ));
    }
    if !diags.is_empty() {
        return Err(diags);
    }

    let stats = PrepStats {
        flat_vars: 0,
        flat_equations: 0,
        aliases: aliases.len(),
        blocks: blocks.len(),
        largest_block: largest,
        explicit: assignments.len(),
    };
    let mut model = PreparedModel {
        flat,
        states,
        algebraics,
        discretes,
        inputs: vec![],
        external: vec![],
        assignments,
        residuals,
        aliases,
        zero_crossings,
        whens,
        structure_key: String::new(),
        stats,
        jac_pattern: Default::default(),
        modes: vec![],
        init: Default::default(),
    };
    model.structure_key = crate::key::structure_key(&model);
    Ok(model)
}

#[allow(clippy::too_many_arguments)]
fn structural_diagnostics(
    flat: &FlatSystem,
    unknowns: &[Slot],
    inc: &[Vec<usize>],
    match_eq: &[Option<usize>],
    match_unk: &[Option<usize>],
    unmatched_eqs: &[usize],
    unmatched_unk: &[usize],
) -> Vec<Diagnostic> {
    let mut out = vec![];
    if !unmatched_eqs.is_empty() {
        // equations reachable by alternating paths from the unmatched ones
        let mut seen: BTreeSet<usize> = unmatched_eqs.iter().copied().collect();
        let mut todo: Vec<usize> = unmatched_eqs.to_vec();
        let mut vars: BTreeSet<usize> = BTreeSet::new();
        while let Some(e) = todo.pop() {
            for &u in &inc[e] {
                vars.insert(u);
                if let Some(e2) = match_unk[u]
                    && seen.insert(e2)
                {
                    todo.push(e2);
                }
            }
        }
        let origins: Vec<&lsim_ir::Origin> =
            seen.iter().map(|&e| &flat.equations[e].origin).collect();
        let parts = parts_of(flat, origins.iter().copied());
        let words: Vec<String> = origins.iter().map(|o| equation_words(flat, o)).collect();
        let what: Vec<String> = vars.iter().map(|&u| slot_name(flat, unknowns[u])).collect();
        let names: Vec<String> = parts.iter().map(|p| format!("'{}'", label_of(flat, p))).collect();
        let mut d = Diagnostic::error(
            "STRUCT-OVER",
            format!(
                "{} set the same quantity more than once: {} ({}) for {} ({}).",
                join_names(&names),
                count(seen.len(), "equation"),
                words.join("; "),
                count(vars.len(), "unknown"),
                what.join(", ")
            ),
        );
        let sources = seen
            .iter()
            .filter(|&&e| {
                let def = &flat.instance(flat.equations[e].origin.instance).def;
                def.contains("Voltage") || def.contains("Battery")
            })
            .count();
        d.hint = Some(if sources >= 2 {
            "Two ideal voltage sources connected in parallel fight over one voltage: put a \
             resistance between them, or remove one."
                .into()
        } else {
            "Look for two parts that each fix the same voltage, speed or temperature and are \
             connected directly to each other."
                .into()
        });
        d.parts = parts;
        d.detail = seen
            .iter()
            .map(|&e| format!("{} = {}", flat.equations[e].lhs, flat.equations[e].rhs))
            .collect();
        out.push(d);
    }
    if !unmatched_unk.is_empty() {
        let mut seen: BTreeSet<usize> = unmatched_unk.iter().copied().collect();
        let mut todo: Vec<usize> = unmatched_unk.to_vec();
        let mut eqs: BTreeSet<usize> = BTreeSet::new();
        while let Some(u) = todo.pop() {
            for (e, us) in inc.iter().enumerate() {
                if us.contains(&u) {
                    eqs.insert(e);
                    if let Some(u2) = match_eq[e]
                        && seen.insert(u2)
                    {
                        todo.push(u2);
                    }
                }
            }
        }
        let what: Vec<String> = seen.iter().map(|&u| slot_name(flat, unknowns[u])).collect();
        let var_parts: Vec<lsim_ir::Origin> = seen
            .iter()
            .map(|&u| {
                let v = match unknowns[u] {
                    Slot::Var(v) | Slot::Der(v) => v,
                };
                lsim_ir::Origin {
                    instance: flat.var(v).instance,
                    kind: OriginKind::Component { index: 0 },
                    label: None,
                }
            })
            .collect();
        let parts = parts_of(flat, var_parts.iter());
        let names: Vec<String> = parts.iter().map(|p| format!("'{}'", label_of(flat, p))).collect();
        let mut d = Diagnostic::error(
            "STRUCT-UNDER",
            format!(
                "Nothing determines {}: {} with {} between them, in {}.",
                what.join(", "),
                count(seen.len(), "unknown"),
                if eqs.is_empty() {
                    "no equation".to_string()
                } else {
                    count(eqs.len(), "equation")
                },
                join_names(&names)
            ),
        )
        .with_hint(
            "Look for a part that is not connected, a circuit with no Ground, or a shaft with \
             nothing to drive or hold it.",
        );
        d.parts = parts;
        out.push(d);
    }
    out
}

fn label_of(flat: &FlatSystem, path: &str) -> String {
    flat.instances
        .iter()
        .find(|i| i.path == path)
        .and_then(|i| i.label.clone())
        .unwrap_or_else(|| path.to_string())
}

fn count(n: usize, what: &str) -> String {
    if n == 1 { format!("1 {what}") } else { format!("{n} {what}s") }
}

fn join_names(names: &[String]) -> String {
    match names {
        [] => "The model's equations".into(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_and_blocks() {
        // e0: u0 ; e1: u0,u1 ; e2: u1,u2 ; e3: u2,u3 ; e4: u3,u2 (a 2-block)
        let inc = vec![vec![0], vec![0, 1], vec![1, 2], vec![2, 3], vec![2, 3]];
        let (me, mu) = max_matching(4, &inc);
        assert_eq!(me[0], Some(0));
        assert!(me.iter().filter(|m| m.is_none()).count() == 1);
        assert!(mu.iter().all(|m| m.is_some()));
        // a 3-cycle and a tail
        let adj = vec![vec![1], vec![2], vec![0], vec![0]];
        let comps = scc(&adj);
        assert_eq!(comps.len(), 2);
        assert_eq!(comps[0].len(), 3);
        assert_eq!(comps[1], vec![3]);
    }

    #[test]
    fn augmenting_paths_find_a_perfect_matching() {
        // the greedy start matches e0-u0, leaving e1 (only u0) to an augmenting path
        let inc = vec![vec![0, 1], vec![0], vec![1, 2]];
        let (me, _) = max_matching(3, &inc);
        assert_eq!(me, vec![Some(1), Some(0), Some(2)]);
    }
}
