//! The inverse model of fast mode (DESIGN.md, *Fast mode*).
//!
//! The same equations, with a different known set ([`InverseSpec`]):
//!
//! * each **prescribed** variable (usually the vehicle body's speed)
//!   becomes an input, and so do its time derivatives: alias elimination
//!   keeps it as the representative of everything equal to it, and index
//!   reduction differentiates whatever the prescription constrains (rigid
//!   driveline speeds follow the wheels', and their accelerations the
//!   prescribed one);
//! * each **freed** signal input (usually what the driver commands) keeps
//!   its link but loses its source: the part that drove it — the driver —
//!   is removed with all its equations, and its output that drove the
//!   freed input stays, now computed backwards from the motion. A removed
//!   part's other outputs must not be needed by anything else;
//! * every `limit(x, lo, hi)` passes `x` through and is listed for the
//!   fast-mode stepper to flag ([`LimitSite`]).
//!
//! Inputs of the prepared inverse model, in order: for each prescribed
//! variable in the spec's order, the variable itself and then its
//! derivatives (`der(…)`, `der(der(…))` …; at least the first).

#![allow(clippy::needless_range_loop)] // parallel arrays indexed in step

use crate::flatten::Extras;
use lsim_ir::expr::{Builtin, Expr};
use lsim_ir::flat::{FlatSystem, InstanceId, OriginKind, VarId};
use lsim_ir::prepared::{InverseSpec, LimitSite};
use lsim_ir::{Diagnostic, VarKind};

/// What the inverse set-up decided.
pub struct InverseSetup {
    /// per flat variable: a prescribed input
    pub input: Vec<bool>,
    /// the prescribed variables, in the spec's order
    pub prescribed: Vec<VarId>,
    /// the removed parts' paths (the drivers)
    pub removed: Vec<String>,
}

fn part_label(flat: &FlatSystem, i: InstanceId) -> String {
    flat.instance_name(flat.top_part(i))
}

/// Applies `spec` to the flat system (before alias elimination).
pub fn apply(
    flat: &mut FlatSystem,
    extras: &mut Extras,
    spec: &InverseSpec,
) -> Result<InverseSetup, Vec<Diagnostic>> {
    let mut diags = vec![];
    let mut prescribed = vec![];
    for name in &spec.prescribed {
        match flat.find_var(name) {
            Some(v) if flat.var(v).kind == VarKind::Continuous => prescribed.push(v),
            Some(v) => diags.push(
                Diagnostic::error(
                    "INVERSE-PRESCRIBED",
                    format!(
                        "Fast mode cannot prescribe '{name}' of {}: it is a discrete variable.",
                        part_label(flat, flat.var(v).instance)
                    ),
                )
                .with_hint("Prescribe the vehicle's speed (a continuous variable)."),
            ),
            None => diags.push(
                Diagnostic::error(
                    "INVERSE-UNKNOWN-NAME",
                    format!(
                        "Fast mode is asked to prescribe '{name}', which the model does not have."
                    ),
                )
                .with_hint("Name the vehicle body's speed variable by its full path."),
            ),
        }
    }
    // the links into the freed inputs, and the parts that drove them
    let mut kept_outputs: Vec<VarId> = vec![];
    let mut drivers: Vec<InstanceId> = vec![];
    for name in &spec.freed {
        let Some(f) = flat.find_var(name) else {
            diags.push(
                Diagnostic::error(
                    "INVERSE-UNKNOWN-NAME",
                    format!("Fast mode is asked to free '{name}', which the model does not have."),
                )
                .with_hint("Name the signal input the driver's command goes into."),
            );
            continue;
        };
        let link = flat.equations.iter().find(|e| {
            matches!(&e.origin.kind, OriginKind::SignalLink { .. }) && e.lhs == Expr::Var(f)
        });
        let Some(link) = link else {
            diags.push(Diagnostic::error(
                "INVERSE-NOT-INPUT",
                format!(
                    "Fast mode is asked to free '{name}' of {}, which is not a linked signal input.",
                    part_label(flat, flat.var(f).instance)
                ),
            ));
            continue;
        };
        let Expr::Var(src) = link.rhs else { continue };
        kept_outputs.push(src);
        let src_part = flat.top_part(flat.var(src).instance);
        let sink_part = flat.top_part(flat.var(f).instance);
        if src_part.0 != 0 && src_part != sink_part && !drivers.contains(&src_part) {
            drivers.push(src_part);
        }
    }
    if !diags.is_empty() {
        return Err(diags);
    }
    // the instances inside the drivers
    let n_inst = flat.instances.len();
    let mut removed_inst = vec![false; n_inst];
    for i in 0..n_inst {
        let id = InstanceId(i as u32);
        if i != 0 && drivers.contains(&flat.top_part(id)) {
            removed_inst[i] = true;
        }
    }
    let n = flat.vars.len();
    let mut removed = vec![false; n];
    for (i, v) in flat.vars.iter().enumerate() {
        if removed_inst[v.instance.0 as usize] && !kept_outputs.contains(&VarId(i as u32)) {
            removed[i] = true;
        }
    }
    let refers_removed = |e: &Expr| {
        e.any(&mut |x| matches!(x, Expr::Var(v) | Expr::Der(v) | Expr::Pre(v) if removed[v.0 as usize]))
    };
    let mut kept_eqs = vec![];
    for e in std::mem::take(&mut flat.equations) {
        if removed_inst[e.origin.instance.0 as usize] {
            continue;
        }
        let uses = refers_removed(&e.lhs) || refers_removed(&e.rhs);
        if uses {
            if matches!(e.origin.kind, OriginKind::SignalLink { .. }) {
                // the driver's own inputs (its target and measured speed)
                let sink_removed = matches!(e.lhs, Expr::Var(v) if removed[v.0 as usize]);
                if sink_removed {
                    continue;
                }
            }
            let user = part_label(flat, e.origin.instance);
            let mut d = Diagnostic::error(
                "INVERSE-DRIVER-USED",
                format!(
                    "Fast mode removes {}, but {user} still needs one of its outputs: {} = {}.",
                    drivers.iter().map(|d| flat.instance_name(*d)).collect::<Vec<_>>().join(", "),
                    e.lhs,
                    e.rhs
                ),
            )
            .with_hint(
                "In fast mode only the commands the motion decides can come from the driver: \\
                 free that input too, or feed it from another block.",
            );
            d.parts = vec![flat.instance(flat.top_part(e.origin.instance)).path.clone()];
            d.parts.extend(drivers.iter().map(|d| flat.instance(*d).path.clone()));
            diags.push(d);
            continue;
        }
        kept_eqs.push(e);
    }
    flat.equations = kept_eqs;
    flat.initial_equations.retain(|e| {
        !removed_inst[e.origin.instance.0 as usize]
            && !refers_removed(&e.lhs)
            && !refers_removed(&e.rhs)
    });
    flat.whens.retain(|w| !removed_inst[w.origin.instance.0 as usize]);
    for w in &flat.whens {
        if refers_removed(&w.condition) || w.assign.iter().any(|(_, x)| refers_removed(x)) {
            diags.push(Diagnostic::error(
                "INVERSE-DRIVER-USED",
                format!(
                    "Fast mode removes the driver, but an event of {} reads it.",
                    part_label(flat, w.origin.instance)
                ),
            ));
        }
    }
    if !diags.is_empty() {
        return Err(diags);
    }
    flat.energy.retain(|en| !removed_inst[en.instance.0 as usize]);
    flat.port_powers.retain(|p| !removed_inst[p.instance.0 as usize]);
    extras.external.retain(|b| !removed_inst[b.instance.0 as usize]);
    // compact the variables
    let mut new_id = vec![u32::MAX; n];
    let mut next = 0u32;
    for i in 0..n {
        if !removed[i] {
            new_id[i] = next;
            next += 1;
        }
    }
    let remap = |e: Expr| {
        e.rewrite(&mut |x| match x {
            Expr::Var(v) => Expr::Var(VarId(new_id[v.0 as usize])),
            Expr::Der(v) => Expr::Der(VarId(new_id[v.0 as usize])),
            Expr::Pre(v) => Expr::Pre(VarId(new_id[v.0 as usize])),
            other => other,
        })
    };
    let take = |e: &mut Expr| *e = remap(std::mem::replace(e, Expr::Const(0.0)));
    for e in flat.equations.iter_mut().chain(flat.initial_equations.iter_mut()) {
        take(&mut e.lhs);
        take(&mut e.rhs);
    }
    for w in &mut flat.whens {
        take(&mut w.condition);
        for (v, x) in w.assign.iter_mut().chain(w.reinit.iter_mut()) {
            *v = VarId(new_id[v.0 as usize]);
            take(x);
        }
    }
    for en in &mut flat.energy {
        for x in [&mut en.stored, &mut en.loss].into_iter().flatten() {
            take(x);
        }
    }
    for p in &mut flat.port_powers {
        take(&mut p.power);
    }
    for b in &mut extras.external {
        for v in b.inputs.iter_mut().chain(b.outputs.iter_mut()) {
            *v = VarId(new_id[v.0 as usize]);
        }
    }
    let vars = std::mem::take(&mut flat.vars);
    let starts = std::mem::take(&mut extras.start);
    for (i, (v, s)) in vars.into_iter().zip(starts).enumerate() {
        if !removed[i] {
            flat.vars.push(v);
            extras.start.push(s.map(&remap));
        }
    }
    let prescribed: Vec<VarId> = prescribed.iter().map(|v| VarId(new_id[v.0 as usize])).collect();
    let mut input = vec![false; flat.vars.len()];
    for v in &prescribed {
        input[v.0 as usize] = true;
    }
    Ok(InverseSetup {
        input,
        prescribed,
        removed: drivers.iter().map(|d| flat.instance(*d).path.clone()).collect(),
    })
}

/// Replaces every `limit(x, lo, hi)` of the equations by `x` and lists it.
pub fn pass_limits(flat: &mut FlatSystem) -> Vec<LimitSite> {
    let mut sites = vec![];
    for e in &mut flat.equations {
        let origin = e.origin.clone();
        for side in [&mut e.lhs, &mut e.rhs] {
            *side = std::mem::replace(side, Expr::Const(0.0)).rewrite(&mut |x| match x {
                Expr::Call(Builtin::Limit, mut args) if args.len() == 3 => {
                    let hi = args.pop().expect("three arguments");
                    let lo = args.pop().expect("three arguments");
                    let value = args.pop().expect("three arguments");
                    sites.push(LimitSite { value: value.clone(), lo, hi, origin: origin.clone() });
                    value
                }
                other => other,
            });
        }
    }
    sites
}
