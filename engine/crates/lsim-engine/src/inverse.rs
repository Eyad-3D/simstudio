//! Preparing the inverse model of fast mode, until work package 2's
//! `lsim_prep::prepare_inverse` replaces it.
//!
//! The same equations as the forward model, with a different known set
//! (DESIGN.md, *Fast mode*):
//!
//! 1. **Freed inputs.** Each signal input named in [`InverseSpec::freed`]
//!    (the driver's commands) loses the link that drives it and becomes an
//!    unknown. The blocks that drove those links (the driver) are removed,
//!    with every block whose only use was to feed them (the cycle's speed
//!    trace): their variables and equations leave the model.
//! 2. **Prescribed variables.** Each variable named in
//!    [`InverseSpec::prescribed`] (the vehicle body's speed) and its time
//!    derivative become inputs, in the order of
//!    [`InverseSpec::input_names`]; `der(v)` in the equations becomes the
//!    input `der(v)`.
//! 3. Alias elimination and causalisation as for any model (lsim-prep's
//!    own steps), so the sorted model computes the required torques,
//!    currents and powers explicitly backwards from the wheels.
//!
//! 4. **Tearing.** Stage 1's causalisation keeps every unknown of an
//!    algebraic loop as an iteration variable; [`tear`] makes all but a
//!    few of them explicit again (greedy: solve any equation with one
//!    unknown left, preferring a constant coefficient; when none is left,
//!    tear the unknown in the most equations), so the inverse model is
//!    evaluated backwards explicitly with a Newton iteration only on the
//!    tearing variables (the battery's bus voltage, the brake blend's
//!    request).
//!
//! What this stopgap does not do is index reduction: when the prescription
//! constrains a state (a wheel inertia whose speed is fixed by the
//! vehicle's), the model is structurally singular and the diagnostics say
//! that fast mode needs work package 2's index reduction for it. The test
//! car of [`crate::testcar`] lumps its rotating inertias into the body's
//! equivalent mass, as the quasi-static method does, and needs none.

use lsim_ir::component::{ComponentDef, Library};
use lsim_ir::expr::Expr;
use lsim_ir::flat::{FlatSystem, InstanceId, OriginKind, VarId};
use lsim_ir::prepared::{Assignment, Slot};
use lsim_ir::units::{Dim, Unit};
use lsim_ir::{Diagnostic, InverseSpec, PreparedModel, VarKind};
use lsim_prep::PrepOptions;
use lsim_prep::symbolic::{contains, diff, simplify, solve_for};
use std::collections::{BTreeSet, HashMap};

/// Prepares the inverse model of `top` for `spec`.
pub fn prepare_inverse(
    lib: &Library,
    top: &ComponentDef,
    spec: &InverseSpec,
    opts: &PrepOptions,
) -> Result<PreparedModel, Vec<Diagnostic>> {
    let mut flat = lsim_prep::flatten::flatten(lib, top)?;
    let faults = lsim_prep::units_check::check(&flat, lib, top);
    if !faults.is_empty() {
        return Err(faults);
    }
    let (flat_vars, flat_equations) = (flat.vars.len(), flat.equations.len());
    free_inputs(&mut flat, spec)?;
    let known = prescribe(&mut flat, spec)?;
    // the known variables are held out of alias elimination and matching
    // as if they were discrete; they become inputs afterwards
    for v in &known {
        flat.vars[v.0 as usize].kind = VarKind::Discrete;
    }
    let aliases = lsim_prep::alias::eliminate(&mut flat);
    let causal = lsim_prep::CausalOptions { force_implicit: opts.force_implicit };
    let mut model =
        lsim_prep::structure::causalize(flat, aliases, &causal, lib).map_err(|mut d| {
            d.push(
                Diagnostic::error(
                    "FAST-INDEX",
                    "Fast mode could not sort this model with the vehicle's speed prescribed. If a \
                 part's state is fixed by the prescribed speed (a wheel or shaft inertia rigidly \
                 coupled to the vehicle), fast mode needs index reduction, which comes with the \
                 model-preparation work package; until then, lump such inertias into the \
                 vehicle's equivalent mass.",
                )
                .with_hint("Check the parts named above."),
            );
            d
        })?;
    model.discretes.retain(|v| !known.contains(v));
    for v in &known {
        model.flat.vars[v.0 as usize].kind = VarKind::Continuous;
    }
    model.inputs = known;
    tear(&mut model);
    model.stats.flat_vars = flat_vars;
    model.stats.flat_equations = flat_equations;
    model.structure_key = lsim_prep::key::structure_key(&model);
    Ok(model)
}

fn find(flat: &FlatSystem, name: &str, what: &str) -> Result<VarId, Vec<Diagnostic>> {
    flat.find_var(name).ok_or_else(|| {
        vec![Diagnostic::error(
            "FAST-SPEC",
            format!("Fast mode: the model has no variable '{name}' to {what}."),
        )]
    })
}

fn under(flat: &FlatSystem, mut i: InstanceId, set: &BTreeSet<InstanceId>) -> bool {
    loop {
        if set.contains(&i) {
            return true;
        }
        match flat.instance(i).parent {
            Some(p) => i = p,
            None => return false,
        }
    }
}

fn link_ends(flat: &FlatSystem, e: &lsim_ir::FlatEquation) -> Option<(VarId, VarId)> {
    match (&e.origin.kind, &e.lhs, &e.rhs) {
        (OriginKind::SignalLink { .. }, Expr::Var(sink), Expr::Var(src)) => {
            let _ = flat;
            Some((*sink, *src))
        }
        _ => None,
    }
}

/// Frees the named inputs and removes the blocks that drove them.
fn free_inputs(flat: &mut FlatSystem, spec: &InverseSpec) -> Result<(), Vec<Diagnostic>> {
    if spec.freed.is_empty() {
        return Ok(());
    }
    let mut freed = BTreeSet::new();
    for name in &spec.freed {
        freed.insert(find(flat, name, "free")?);
    }
    // the parts that drive the freed inputs
    let mut removed: BTreeSet<InstanceId> = BTreeSet::new();
    for e in &flat.equations {
        if let Some((sink, src)) = link_ends(flat, e)
            && freed.contains(&sink)
        {
            removed.insert(flat.top_part(flat.var(src).instance));
        }
    }
    // and, repeatedly, signal-only parts whose outputs feed only removed parts
    let physical_parts: BTreeSet<InstanceId> = flat
        .vars
        .iter()
        .filter(|v| {
            matches!(v.role, lsim_ir::VarRole::Across { .. } | lsim_ir::VarRole::Through { .. })
        })
        .map(|v| flat.top_part(v.instance))
        .collect();
    loop {
        let mut feeds: HashMap<InstanceId, (bool, bool)> = HashMap::new(); // (to removed, elsewhere)
        for e in &flat.equations {
            if let Some((sink, src)) = link_ends(flat, e) {
                let from = flat.top_part(flat.var(src).instance);
                let to = flat.top_part(flat.var(sink).instance);
                if from == to || removed.contains(&from) {
                    continue;
                }
                let entry = feeds.entry(from).or_default();
                if removed.contains(&to) {
                    entry.0 = true;
                } else {
                    entry.1 = true;
                }
            }
        }
        let more: Vec<InstanceId> = feeds
            .into_iter()
            .filter(|(p, (to_removed, elsewhere))| {
                *to_removed && !*elsewhere && !physical_parts.contains(p)
            })
            .map(|(p, _)| p)
            .collect();
        if more.is_empty() {
            break;
        }
        removed.extend(more);
    }
    if removed.is_empty() {
        return Ok(());
    }
    // a removed part's output may not drive anything that stays
    let mut diags = vec![];
    for e in &flat.equations {
        if let Some((sink, src)) = link_ends(flat, e) {
            let from_removed = under(flat, flat.var(src).instance, &removed);
            let to_removed = under(flat, flat.var(sink).instance, &removed);
            if from_removed && !to_removed && !freed.contains(&sink) {
                diags.push(
                    Diagnostic::error(
                        "FAST-SPEC",
                        format!(
                            "Fast mode removes {} (it drives the freed inputs), but its output {} \
                         also drives {}, which nothing would compute then.",
                            flat.instance_name(flat.top_part(flat.var(src).instance)),
                            flat.var(src).name,
                            flat.var(sink).name
                        ),
                    )
                    .with_hint("Free that input too."),
                );
            }
        }
    }
    if !diags.is_empty() {
        return Err(diags);
    }
    // drop their equations, links into or out of them, the freed links and
    // their when clauses, energy books and port powers
    let gone_var: Vec<bool> = flat.vars.iter().map(|v| under(flat, v.instance, &removed)).collect();
    let keep_eq = |e: &lsim_ir::FlatEquation| -> bool {
        if let Some((sink, src)) = link_ends(flat, e) {
            return !(freed.contains(&sink)
                || gone_var[sink.0 as usize]
                || gone_var[src.0 as usize]);
        }
        !under(flat, e.origin.instance, &removed)
    };
    let equations: Vec<_> = flat.equations.iter().filter(|e| keep_eq(e)).cloned().collect();
    let initial: Vec<_> = flat.initial_equations.iter().filter(|e| keep_eq(e)).cloned().collect();
    flat.equations = equations;
    flat.initial_equations = initial;
    let removed_ref = &removed;
    let flat_ref: &FlatSystem = flat;
    let whens: Vec<_> = flat_ref
        .whens
        .iter()
        .filter(|w| !under(flat_ref, w.origin.instance, removed_ref))
        .cloned()
        .collect();
    let energy: Vec<_> = flat_ref
        .energy
        .iter()
        .filter(|en| !under(flat_ref, en.instance, removed_ref))
        .cloned()
        .collect();
    let powers: Vec<_> = flat_ref
        .port_powers
        .iter()
        .filter(|pp| !under(flat_ref, pp.instance, removed_ref))
        .cloned()
        .collect();
    flat.whens = whens;
    flat.energy = energy;
    flat.port_powers = powers;
    remove_vars(flat, &gone_var);
    Ok(())
}

/// Removes the marked variables, renumbering the rest everywhere.
fn remove_vars(flat: &mut FlatSystem, gone: &[bool]) {
    let mut new_id = vec![u32::MAX; gone.len()];
    let mut next = 0u32;
    for (i, g) in gone.iter().enumerate() {
        if !g {
            new_id[i] = next;
            next += 1;
        }
    }
    let map = |e: Expr| {
        e.rewrite(&mut |x| match x {
            Expr::Var(v) => Expr::Var(VarId(new_id[v.0 as usize])),
            Expr::Der(v) => Expr::Der(VarId(new_id[v.0 as usize])),
            Expr::Pre(v) => Expr::Pre(VarId(new_id[v.0 as usize])),
            other => other,
        })
    };
    let take = |e: &mut Expr| std::mem::replace(e, Expr::Const(0.0));
    for e in flat.equations.iter_mut().chain(flat.initial_equations.iter_mut()) {
        e.lhs = map(take(&mut e.lhs));
        e.rhs = map(take(&mut e.rhs));
    }
    for w in &mut flat.whens {
        w.condition = map(take(&mut w.condition));
        for (v, x) in w.assign.iter_mut().chain(w.reinit.iter_mut()) {
            *v = VarId(new_id[v.0 as usize]);
            *x = map(take(x));
        }
    }
    for en in &mut flat.energy {
        for x in [&mut en.stored, &mut en.loss].into_iter().flatten() {
            *x = map(take(x));
        }
    }
    for pp in &mut flat.port_powers {
        pp.power = map(take(&mut pp.power));
    }
    let vars = std::mem::take(&mut flat.vars);
    flat.vars = vars.into_iter().zip(gone).filter(|(_, g)| !**g).map(|(v, _)| v).collect();
}

/// Makes the prescribed variables and their derivatives known; returns
/// them in input order.
fn prescribe(flat: &mut FlatSystem, spec: &InverseSpec) -> Result<Vec<VarId>, Vec<Diagnostic>> {
    let mut known = vec![];
    for name in &spec.prescribed {
        let v = find(flat, name, "prescribe")?;
        let var = flat.var(v).clone();
        if var.kind != VarKind::Continuous {
            return Err(vec![Diagnostic::error(
                "FAST-SPEC",
                format!("Fast mode: '{name}' is a discrete variable and cannot follow a trace."),
            )]);
        }
        let dim = var.unit.dim / Dim::TIME;
        let dv = VarId(flat.vars.len() as u32);
        flat.vars.push(lsim_ir::FlatVar {
            name: format!("der({name})"),
            unit: Unit::new(dim, 1.0),
            unit_text: lsim_fast::limits::unit_text(dim),
            kind: VarKind::Continuous,
            start: None,
            fixed: false,
            nominal: var.nominal,
            instance: var.instance,
            role: lsim_ir::VarRole::Local,
        });
        let sub = |e: Expr| {
            e.rewrite(&mut |x| match x {
                Expr::Der(w) if w == v => Expr::Var(dv),
                other => other,
            })
        };
        let take = |e: &mut Expr| std::mem::replace(e, Expr::Const(0.0));
        for e in flat.equations.iter_mut().chain(flat.initial_equations.iter_mut()) {
            e.lhs = sub(take(&mut e.lhs));
            e.rhs = sub(take(&mut e.rhs));
        }
        for en in &mut flat.energy {
            for x in [&mut en.stored, &mut en.loss].into_iter().flatten() {
                *x = sub(take(x));
            }
        }
        known.push(v);
        known.push(dv);
    }
    Ok(known)
}

fn slots_in(e: &Expr, out: &mut Vec<Slot>) {
    e.walk(&mut |x| match x {
        Expr::Var(v) => out.push(Slot::Var(*v)),
        Expr::Der(v) => out.push(Slot::Der(*v)),
        _ => {}
    });
}

/// Makes the iteration variables explicit where it can (see the module
/// documentation), leaving the tearing variables and their residuals.
pub fn tear(m: &mut PreparedModel) {
    if m.algebraics.len() < 2 {
        return;
    }
    let mut unknown: BTreeSet<Slot> = m.algebraics.iter().copied().collect();
    let mut left: Vec<usize> = (0..m.residuals.len()).collect();
    let mut solved: Vec<Assignment> = vec![];
    let mut torn: Vec<Slot> = vec![];
    while !unknown.is_empty() {
        // an equation with one unknown left that it is affine in; prefer a
        // coefficient free of variables (it cannot vanish at run time)
        let mut best: Option<(usize, Slot, Expr, bool)> = None;
        for (pos, &e) in left.iter().enumerate() {
            let expr = &m.residuals[e].expr;
            let us: BTreeSet<Slot> =
                unknown.iter().copied().filter(|s| contains(expr, *s)).collect();
            if us.len() != 1 {
                continue;
            }
            let u = *us.iter().next().expect("one");
            let Some(sol) = solve_for(expr, u) else { continue };
            let coef = simplify(diff(expr, u));
            let mut vars = vec![];
            slots_in(&coef, &mut vars);
            let constant = vars.is_empty();
            if best.as_ref().is_none_or(|b| constant && !b.3) {
                best = Some((pos, u, sol, constant));
                if constant {
                    break;
                }
            }
        }
        if let Some((pos, u, sol, _)) = best {
            let e = left.remove(pos);
            solved.push(Assignment { target: u, expr: sol, origin: m.residuals[e].origin.clone() });
            unknown.remove(&u);
            continue;
        }
        // tear the unknown that appears in the most remaining equations
        let u = *unknown
            .iter()
            .max_by_key(|s| left.iter().filter(|&&e| contains(&m.residuals[e].expr, **s)).count())
            .expect("an unknown is left");
        unknown.remove(&u);
        torn.push(u);
    }
    if solved.is_empty() || left.len() != torn.len() {
        return;
    }
    // the tearing variables keep their order; their residuals are what is left
    m.algebraics.retain(|s| torn.contains(s));
    let residuals = std::mem::take(&mut m.residuals);
    m.residuals = residuals
        .into_iter()
        .enumerate()
        .filter(|(i, _)| left.contains(i))
        .map(|(_, r)| r)
        .collect();
    // every assignment after what it reads (stable topological order)
    let mut all = std::mem::take(&mut m.assignments);
    all.extend(solved);
    let index: HashMap<Slot, usize> = all.iter().enumerate().map(|(i, a)| (a.target, i)).collect();
    let deps: Vec<Vec<usize>> = all
        .iter()
        .map(|a| {
            let mut s = vec![];
            slots_in(&a.expr, &mut s);
            let mut d: Vec<usize> = s.iter().filter_map(|x| index.get(x).copied()).collect();
            d.sort_unstable();
            d.dedup();
            d
        })
        .collect();
    let mut done = vec![false; all.len()];
    let mut order = Vec::with_capacity(all.len());
    while order.len() < all.len() {
        let before = order.len();
        for i in 0..all.len() {
            if !done[i] && deps[i].iter().all(|&j| done[j] || j == i) {
                done[i] = true;
                order.push(i);
            }
        }
        if order.len() == before {
            // a cycle cannot happen (each solved unknown came from equations
            // in torn or earlier unknowns); keep what is left in order
            order.extend((0..all.len()).filter(|i| !done[*i]));
            break;
        }
    }
    let mut slots: Vec<Option<Assignment>> = all.into_iter().map(Some).collect();
    m.assignments = order.into_iter().map(|i| slots[i].take().expect("once")).collect();
}
