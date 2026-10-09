//! Clocked partitions of sampled external blocks (DESIGN.md, *Causal
//! blocks, Script blocks and FMUs*).
//!
//! A sampled block's outputs are discrete variables, held between its
//! ticks; its inputs are read from the solution at each tick. Blocks that
//! tick together must tick in order when one block's inputs depend,
//! through the explicit assignments (not through states), on another's
//! outputs: then the second sees the first's new outputs in the same tick.
//! [`order`] sorts the blocks so; blocks whose inputs and outputs feed
//! each other directly are reported (`EXTERNAL-LOOP`), as each would then
//! see the other's values of the previous tick.

use crate::diagnose::warning;
use lsim_ir::expr::Expr;
use lsim_ir::prepared::{AliasTarget, ExternalBlock, PreparedModel, Slot};
use lsim_ir::{Diagnostic, VarId};
use std::collections::{BTreeSet, HashMap};

/// The discrete variables each variable depends on through assignments.
fn discrete_deps(m: &PreparedModel, watched: &BTreeSet<VarId>) -> HashMap<Slot, BTreeSet<VarId>> {
    let mut deps: HashMap<Slot, BTreeSet<VarId>> = HashMap::new();
    for a in &m.assignments {
        let mut set = BTreeSet::new();
        a.expr.walk(&mut |x| {
            let s = match x {
                Expr::Var(v) | Expr::Pre(v) => {
                    if watched.contains(v) {
                        set.insert(*v);
                        return;
                    }
                    Slot::Var(*v)
                }
                Expr::Der(v) => Slot::Der(*v),
                _ => return,
            };
            if let Some(d) = deps.get(&s) {
                set.extend(d.iter().copied());
            }
        });
        deps.insert(a.target, set);
    }
    deps
}

/// Sorts the blocks into tick order; reports direct loops between them.
pub fn order(
    m: &PreparedModel,
    blocks: Vec<ExternalBlock>,
) -> (Vec<ExternalBlock>, Vec<Diagnostic>) {
    if blocks.len() < 2 {
        return (blocks, vec![]);
    }
    let watched: BTreeSet<VarId> = blocks.iter().flat_map(|b| b.outputs.iter().copied()).collect();
    let deps = discrete_deps(m, &watched);
    let alias: HashMap<VarId, AliasTarget> = m.aliases.iter().map(|a| (a.var, a.target)).collect();
    let reads = |v: VarId| -> BTreeSet<VarId> {
        let v = match alias.get(&v) {
            Some(AliasTarget::Var { var, .. }) => *var,
            Some(AliasTarget::Const(_)) => return BTreeSet::new(),
            None => v,
        };
        if watched.contains(&v) {
            return [v].into_iter().collect();
        }
        deps.get(&Slot::Var(v)).cloned().unwrap_or_default()
    };
    let n = blocks.len();
    let mut after: Vec<Vec<usize>> = vec![vec![]; n];
    for (b, blk) in blocks.iter().enumerate() {
        let mut seen = BTreeSet::new();
        for &i in &blk.inputs {
            seen.extend(reads(i));
        }
        for (a, other) in blocks.iter().enumerate() {
            if a != b && other.outputs.iter().any(|o| seen.contains(o)) {
                after[b].push(a);
            }
        }
    }
    let mut done = vec![false; n];
    let mut out = vec![];
    let mut diags = vec![];
    while out.len() < n {
        let next = (0..n).find(|&b| !done[b] && after[b].iter().all(|&a| done[a]));
        let b = match next {
            Some(b) => b,
            None => {
                let stuck: Vec<usize> = (0..n).filter(|&b| !done[b]).collect();
                let names: Vec<String> =
                    stuck.iter().map(|&b| m.flat.instance_name(blocks[b].instance)).collect();
                let mut d = warning(
                    "EXTERNAL-LOOP",
                    format!(
                        "The sampled blocks {} feed each other directly: at each tick each sees \
                         the others' outputs of the tick before.",
                        names.join(", ")
                    ),
                )
                .with_hint("Expected for controllers in a loop; otherwise give one a delay.");
                d.parts = stuck
                    .iter()
                    .map(|&b| m.flat.instance(m.flat.top_part(blocks[b].instance)).path.clone())
                    .collect();
                diags.push(d);
                stuck[0]
            }
        };
        done[b] = true;
        out.push(b);
    }
    let mut blocks: Vec<Option<ExternalBlock>> = blocks.into_iter().map(Some).collect();
    (out.into_iter().map(|b| blocks[b].take().expect("each once")).collect(), diags)
}
