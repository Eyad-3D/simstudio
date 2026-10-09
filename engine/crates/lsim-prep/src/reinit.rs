//! `reinit`: a state that jumps at events.
//!
//! The integrator's states are continuous, and the run loop applies only
//! a `when` clause's assignments to discrete variables (WP4's `iterate`),
//! so a state `x` that `reinit(x, v)` restarts is split into a continuous
//! part and its jumps:
//!
//! ```text
//! x = x.continuous + x.jump        der(x) is read as der(x.continuous)
//! when …: x.jump := v - x.continuous
//! ```
//!
//! `x.jump` is discrete (start 0), so the equations, index reduction and
//! the integrator see `x.continuous` as the state and `x` as an algebraic
//! variable. The when clause's assignments are evaluated together with
//! the values just before the event, so right after it `x = v` exactly
//! while `x.continuous` integrates on undisturbed; every other assignment
//! of the clause (a gear's new ratio) takes effect at the same instant.
//!
//! A target is first resolved through the alias table (a gear's `b.w` is
//! the load's speed); the continuous part inherits the start value and
//! `fixed`, and index reduction keeps it as a state in preference to any
//! other variable. When the target is not a state at all (nothing reads
//! its derivative), or index reduction cannot keep it one (it follows from
//! another state), preparation says so (`REINIT-NOT-STATE`).

use crate::diagnose::{local_name, parts_of};
use lsim_ir::expr::Expr;
use lsim_ir::flat::{FlatEquation, FlatSystem, FlatVar, Origin, VarId, VarRole};
use lsim_ir::prepared::{AliasEntry, AliasTarget};
use lsim_ir::{Diagnostic, VarKind};

/// One restarted state.
pub struct Restarted {
    /// the variable `reinit` restarts (an alias root)
    pub var: VarId,
    /// its continuous part, which must stay a state
    pub continuous: VarId,
    /// its jumps (discrete)
    pub jump: VarId,
    /// the first `when` clause that restarts it
    pub origin: Origin,
}

/// The diagnostic of a `reinit` whose target is not a state.
pub fn not_state(flat: &FlatSystem, x: VarId, origin: &Origin, why: &str) -> Diagnostic {
    let mut d = Diagnostic::error(
        "REINIT-NOT-STATE",
        format!(
            "{} restarts {} with reinit, but {why}.",
            flat.instance_name(origin.instance),
            local_name(flat, x)
        ),
    )
    .with_hint(
        "reinit restarts a state (a quantity the model integrates, such as an inertia's speed): \
         restart that state instead.",
    );
    d.parts = parts_of(flat, [origin.instance, flat.var(x).instance].into_iter());
    d
}

/// Rewrites every `reinit` of `flat` (after alias elimination) as the
/// assignment of a jump; `start` holds the start expressions per variable
/// and grows with the new variables.
pub fn apply(
    flat: &mut FlatSystem,
    start: &mut Vec<Option<Expr>>,
    aliases: &[AliasEntry],
) -> Result<Vec<Restarted>, Vec<Diagnostic>> {
    if flat.whens.iter().all(|w| w.reinit.is_empty()) {
        return Ok(vec![]);
    }
    let n = flat.vars.len();
    start.resize(n, None);
    let mut target: Vec<Option<AliasTarget>> = vec![None; n];
    for a in aliases {
        target[a.var.0 as usize] = Some(a.target);
    }
    let mut differentiated = vec![false; n];
    let mut mark = |e: &Expr| {
        crate::walk::visit(e, &mut |x| {
            if let Expr::Der(v) = x {
                differentiated[v.0 as usize] = true;
            }
        })
    };
    for e in &flat.equations {
        mark(&e.lhs);
        mark(&e.rhs);
    }

    // the targets, resolved: (when, action, root, sign)
    let mut diags = vec![];
    let mut actions = vec![];
    for (w, when) in flat.whens.iter().enumerate() {
        for (k, (x, _)) in when.reinit.iter().enumerate() {
            let (root, sign) = match target[x.0 as usize] {
                None => (*x, 1.0),
                Some(AliasTarget::Var { var, negated }) => (var, if negated { -1.0 } else { 1.0 }),
                Some(AliasTarget::Const(_)) => {
                    diags.push(not_state(
                        flat,
                        *x,
                        &when.origin,
                        "the model fixes it to a constant",
                    ));
                    continue;
                }
            };
            let v = flat.var(root);
            if v.kind != VarKind::Continuous || !differentiated[root.0 as usize] {
                diags.push(not_state(
                    flat,
                    *x,
                    &when.origin,
                    "nothing in the model makes it change continuously (no der() of it), so \
                     there is no state to restart",
                ));
                continue;
            }
            actions.push((w, k, root, sign));
        }
    }
    if !diags.is_empty() {
        return Err(diags);
    }

    // a continuous part and a jump per restarted variable
    let mut split: Vec<Option<usize>> = vec![None; n];
    let mut restarted: Vec<Restarted> = vec![];
    for &(w, _, root, _) in &actions {
        if split[root.0 as usize].is_some() {
            continue;
        }
        split[root.0 as usize] = Some(restarted.len());
        let x = flat.var(root).clone();
        let continuous = VarId(flat.vars.len() as u32);
        flat.vars.push(FlatVar {
            name: format!("{}.continuous", x.name),
            role: VarRole::Local,
            ..x.clone()
        });
        start.push(start[root.0 as usize].clone());
        let jump = VarId(flat.vars.len() as u32);
        flat.vars.push(FlatVar {
            name: format!("{}.jump", x.name),
            kind: VarKind::Discrete,
            start: Some(0.0),
            fixed: true,
            role: VarRole::Local,
            ..x.clone()
        });
        start.push(Some(Expr::Const(0.0)));
        // the start belongs to the continuous part now (the jump starts
        // at zero): x keeps its value only as a guess
        flat.vars[root.0 as usize].fixed = false;
        let origin = flat.whens[w].origin.clone();
        flat.equations.push(FlatEquation {
            lhs: Expr::Var(root),
            rhs: Expr::Var(continuous) + Expr::Var(jump),
            origin: Origin {
                label: Some(format!(
                    "{} is its continuous part plus its jumps (reinit)",
                    local_name(flat, root)
                )),
                ..origin.clone()
            },
        });
        restarted.push(Restarted { var: root, continuous, jump, origin });
    }

    // der(x) is der(x.continuous) everywhere
    let swap = |e: &mut Expr| {
        crate::walk::mutate(e, &mut |x| {
            if let Expr::Der(v) = x
                && let Some(k) = split.get(v.0 as usize).copied().flatten()
            {
                *v = restarted[k].continuous;
            }
        })
    };
    for e in flat.equations.iter_mut().chain(flat.initial_equations.iter_mut()) {
        swap(&mut e.lhs);
        swap(&mut e.rhs);
    }
    for w in &mut flat.whens {
        swap(&mut w.condition);
        for (_, v) in w.assign.iter_mut().chain(w.reinit.iter_mut()) {
            swap(v);
        }
    }
    for a in &mut flat.asserts {
        swap(&mut a.condition);
    }
    for en in &mut flat.energy {
        for x in [&mut en.stored, &mut en.loss].into_iter().flatten() {
            swap(x);
        }
    }
    for pp in &mut flat.port_powers {
        swap(&mut pp.power);
    }

    // reinit(x, v) is x.jump := ±v - x.continuous
    for &(w, k, root, sign) in &actions {
        let r = &restarted[split[root.0 as usize].expect("split above")];
        let v = flat.whens[w].reinit[k].1.clone();
        let v = if sign < 0.0 { -v } else { v };
        let jump = crate::symbolic::simplify(v - Expr::Var(r.continuous));
        flat.whens[w].assign.push((r.jump, jump));
    }
    for w in &mut flat.whens {
        w.reinit.clear();
    }
    Ok(restarted)
}
