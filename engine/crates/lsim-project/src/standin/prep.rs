//! Stand-in preparation: Stage 1's flatten → units → aliases → causalize,
//! with what work package 2 will provide done here in the simplest correct
//! way, so models with friction modes, rigid drivelines and tables run
//! today:
//!
//! * **relations** in equations (if-expressions, `abs`, `sign`) are wrapped
//!   in `noEvent`: they are evaluated as they stand (no modes yet); the
//!   friction elements' modes are discrete variables set by `when` clauses,
//!   so they are exact already;
//! * **index reduction** for constraints between states (two inertias on
//!   one shaft, an ideal gear between them, a friction element's own speed
//!   on its shaft): every equation that involves states and no unknowns is
//!   a constraint; one of its states is demoted (its derivative becomes an
//!   ordinary unknown) and the constraint's time derivative is added —
//!   Pantelides' algorithm with dummy derivatives, for the constraints of
//!   index two that rigid mechanical and electrical couplings give;
//! * **energy meters** (optional): per instance, the integrals of its port
//!   powers, its loss and its port powers' magnitude as extra states, and
//!   its stored energy as a variable, for the books (work package 4 makes
//!   these quadratures);
//! * **pivot check**: an equation solved explicitly for an unknown whose
//!   coefficient is mode-dependent (and can be zero) is kept implicit.

use lsim_ir::component::{ComponentDef, Library};
use lsim_ir::expr::{BinaryOp, Builtin, CmpOp, Expr};
use lsim_ir::flat::*;
use lsim_ir::prepared::{PreparedModel, Slot};
use lsim_ir::units::parse_unit;
use lsim_ir::{Diagnostic, VarKind};
use lsim_prep::structure::{CausalOptions, max_matching};
use lsim_prep::symbolic::{diff, simplify};
use std::collections::{BTreeSet, HashMap};

/// Options for [`prepare`].
#[derive(Clone, Debug, Default)]
pub struct Options {
    /// keep every block implicit (exercises the DAE path)
    pub force_implicit: bool,
    /// add energy meters for every instance with books
    pub energy_meters: bool,
}

/// A constraint between states found by the index reduction.
#[derive(Clone, Debug)]
pub struct Constraint {
    /// its residual, flat scope (zero when it holds)
    pub residual: Expr,
    /// the state variables in it (kept or demoted)
    pub vars: Vec<VarId>,
    /// the variable demoted for it
    pub demoted: VarId,
    /// the instance whose equation it is (it books an impulsive loss)
    pub instance: InstanceId,
}

/// One instance's energy meters (flat variable indices).
#[derive(Clone, Debug)]
pub struct Meter {
    /// the instance
    pub instance: InstanceId,
    /// ∫ Σ port power dt, J
    pub port_energy: Option<VarId>,
    /// ∫ loss dt, J
    pub loss_energy: Option<VarId>,
    /// ∫ Σ |port power| dt, J (its throughput, the scale of its books)
    pub throughput: Option<VarId>,
    /// its stored energy now, J
    pub stored: Option<VarId>,
}

/// The prepared model and what the stand-in run loop needs besides.
#[derive(Clone, Debug)]
pub struct Prepared {
    /// the prepared model (Stage 1 form)
    pub model: PreparedModel,
    /// the constraints between states (for impulse re-initialisation)
    pub constraints: Vec<Constraint>,
    /// energy meters
    pub meters: Vec<Meter>,
    /// stored-energy expressions per instance (flat scope, aliases
    /// substituted), for the mass metric of impulse re-initialisation
    pub stored: Vec<(InstanceId, Expr)>,
}

fn walk_any(e: &Expr, f: &mut impl FnMut(&Expr) -> bool) -> bool {
    e.any(f)
}

/// Whether `e` has a relation outside `noEvent`.
fn has_event_relation(e: &Expr) -> bool {
    match e {
        Expr::NoEvent(_) => false,
        Expr::Compare(..) => true,
        Expr::Call(Builtin::Abs | Builtin::Sign, _) => true,
        other => other.children().into_iter().any(has_event_relation),
    }
}

fn wrap(e: Expr) -> Expr {
    if has_event_relation(&e) { Expr::NoEvent(Box::new(e)) } else { e }
}

/// Adds the energy meters of every instance that has port powers or books.
fn add_meters(flat: &mut FlatSystem) -> Vec<Meter> {
    // per instance: its port powers, its loss, its stored energy
    type Books = (Vec<Expr>, Option<Expr>, Option<Expr>);
    let mut by_inst: HashMap<InstanceId, Books> = HashMap::new();
    for pp in &flat.port_powers {
        by_inst.entry(pp.instance).or_default().0.push(pp.power.clone());
    }
    for en in &flat.energy {
        let e = by_inst.entry(en.instance).or_default();
        e.1 = en.loss.clone();
        e.2 = en.stored.clone();
    }
    let mut insts: Vec<InstanceId> = by_inst.keys().copied().collect();
    insts.sort();
    let joule = parse_unit("J").expect("J");
    let watt_s = joule;
    let mut meters = vec![];
    for inst in insts {
        let (powers, loss, stored) = by_inst.remove(&inst).expect("listed");
        let path = flat.instance(inst).path.clone();
        let new_state = |flat: &mut FlatSystem, what: &str, rate: Expr| -> VarId {
            flat.vars.push(FlatVar {
                name: format!("{}{}__energy_{what}", path, if path.is_empty() { "" } else { "." }),
                unit: watt_s,
                unit_text: "J".into(),
                kind: VarKind::Continuous,
                start: Some(0.0),
                fixed: true,
                nominal: 1.0,
                instance: inst,
                role: VarRole::Local,
            });
            let v = VarId(flat.vars.len() as u32 - 1);
            flat.equations.push(FlatEquation {
                lhs: Expr::Der(v),
                rhs: rate,
                origin: Origin {
                    instance: inst,
                    kind: OriginKind::Component { index: usize::MAX },
                    label: Some(format!("energy meter: {what}")),
                },
            });
            v
        };
        let port_energy = (!powers.is_empty()).then(|| {
            let total = powers.iter().cloned().reduce(|a, b| a + b).expect("non-empty");
            new_state(flat, "ports", total)
        });
        let throughput = (!powers.is_empty()).then(|| {
            let total = powers
                .iter()
                .map(|p| Expr::Call(Builtin::Abs, vec![p.clone()]))
                .reduce(|a, b| a + b)
                .expect("non-empty");
            new_state(flat, "throughput", total)
        });
        let loss_energy = loss.map(|l| new_state(flat, "loss", l));
        let stored_var = stored.map(|s| {
            flat.vars.push(FlatVar {
                name: format!("{}{}__energy_stored", path, if path.is_empty() { "" } else { "." }),
                unit: joule,
                unit_text: "J".into(),
                kind: VarKind::Continuous,
                start: None,
                fixed: false,
                nominal: 1.0,
                instance: inst,
                role: VarRole::Local,
            });
            let v = VarId(flat.vars.len() as u32 - 1);
            flat.equations.push(FlatEquation {
                lhs: Expr::Var(v),
                rhs: s,
                origin: Origin {
                    instance: inst,
                    kind: OriginKind::Component { index: usize::MAX },
                    label: Some("energy meter: stored".into()),
                },
            });
            v
        });
        meters.push(Meter {
            instance: inst,
            port_energy,
            loss_energy,
            throughput,
            stored: stored_var,
        });
    }
    meters
}

fn states_of(flat: &FlatSystem) -> Vec<bool> {
    let mut is_state = vec![false; flat.vars.len()];
    for e in &flat.equations {
        for side in [&e.lhs, &e.rhs] {
            side.walk(&mut |x| {
                if let Expr::Der(v) = x {
                    is_state[v.0 as usize] = true;
                }
            });
        }
    }
    is_state
}

/// The time derivative of `g`, where `d(v)` gives each state's derivative
/// (relations, discrete variables and parameters are constant between
/// events). `None` when `g` has the time or a non-state variable in it.
fn time_derivative(g: &Expr, vars_in: &[VarId], d_of: &dyn Fn(VarId) -> Expr) -> Option<Expr> {
    if walk_any(g, &mut |x| matches!(x, Expr::Time)) {
        return None;
    }
    let mut total: Option<Expr> = None;
    for &v in vars_in {
        let coef = simplify(diff(g, Slot::Var(v)));
        if matches!(coef, Expr::Const(c) if c == 0.0) {
            continue;
        }
        let term = coef * d_of(v);
        total = Some(match total {
            None => term,
            Some(t) => t + term,
        });
    }
    Some(total.unwrap_or(Expr::Const(0.0)))
}

/// Index reduction for constraints between states (see the module doc).
///
/// Constraints are found by a fixpoint: every algebraic variable that one
/// equation determines from states, discrete variables and parameters
/// alone (explicitly) is known; an equation left over whose variables are
/// all known — and has no derivative in it — over-determines the states:
/// it is a constraint. Its variables are then expanded into states, so
/// it can be differentiated.
fn reduce_index(
    flat: &mut FlatSystem,
    eliminated: &[bool],
) -> Result<Vec<Constraint>, Vec<Diagnostic>> {
    let is_state = states_of(flat);
    let n_vars = flat.vars.len();
    let unknown = |v: VarId| -> bool {
        let i = v.0 as usize;
        i < n_vars && !eliminated[i] && flat.vars[i].kind == VarKind::Continuous && !is_state[i]
    };
    let has_der = |e: &FlatEquation| {
        e.lhs.any(&mut |x| matches!(x, Expr::Der(_)))
            || e.rhs.any(&mut |x| matches!(x, Expr::Der(_)))
    };
    let vars_of = |e: &FlatEquation| {
        let mut set = BTreeSet::new();
        for side in [&e.lhs, &e.rhs] {
            side.walk(&mut |x| {
                if let Expr::Var(v) = x {
                    set.insert(*v);
                }
            });
        }
        set
    };
    let mut defs: HashMap<VarId, Expr> = HashMap::new();
    let mut used = vec![false; flat.equations.len()];
    // each round takes the best usable equation: one without states first,
    // then one whose coefficient on the variable it defines is free of
    // variables (so the definition never divides by a state)
    loop {
        let mut best: Option<(u8, usize, VarId, Expr)> = None;
        for (k, e) in flat.equations.iter().enumerate() {
            if used[k] || has_der(e) {
                continue;
            }
            let vs = vars_of(e);
            let unk: Vec<VarId> =
                vs.iter().copied().filter(|v| unknown(*v) && !defs.contains_key(v)).collect();
            if unk.len() != 1 {
                continue;
            }
            let has_state = vs.iter().any(|v| (v.0 as usize) < n_vars && is_state[v.0 as usize]);
            let r = simplify(e.lhs.clone() - e.rhs.clone());
            let coef = simplify(diff(&r, Slot::Var(unk[0])));
            let coef_var = coef.any(&mut |x| matches!(x, Expr::Var(_)));
            let rank = (has_state as u8) * 2 + coef_var as u8;
            if best.as_ref().is_some_and(|b| b.0 <= rank) {
                continue;
            }
            if let Some(sol) = lsim_prep::symbolic::solve_for(&r, Slot::Var(unk[0])) {
                best = Some((rank, k, unk[0], sol));
                if rank == 0 {
                    break;
                }
            }
        }
        let Some((_, k, v, sol)) = best else { break };
        defs.insert(v, sol);
        used[k] = true;
    }
    // a known variable in terms of states, discrete variables and parameters
    fn expand(e: &Expr, defs: &HashMap<VarId, Expr>, depth: usize) -> Expr {
        e.clone().rewrite(&mut |x| match x {
            Expr::Var(v) if depth < 64 && defs.contains_key(&v) => {
                expand(&defs[&v], defs, depth + 1)
            }
            other => other,
        })
    }
    let mut cons: Vec<(usize, Vec<VarId>, Expr)> = vec![];
    for (k, e) in flat.equations.iter().enumerate() {
        if used[k] || has_der(e) {
            continue;
        }
        let vs = vars_of(e);
        if vs.is_empty() || vs.iter().any(|v| unknown(*v) && !defs.contains_key(v)) {
            continue;
        }
        let g = simplify(expand(&(e.lhs.clone() - e.rhs.clone()), &defs, 0));
        let mut states = BTreeSet::new();
        g.walk(&mut |x| {
            if let Expr::Var(v) = x
                && (v.0 as usize) < n_vars
                && is_state[v.0 as usize]
            {
                states.insert(*v);
            }
        });
        if !states.is_empty() {
            cons.push((k, states.into_iter().collect(), g));
        }
    }
    if cons.is_empty() {
        return Ok(vec![]);
    }
    // which state each constraint demotes: a matching, preferring states
    // that store no energy themselves (a friction element's own speed)
    let has_store: BTreeSet<InstanceId> =
        flat.energy.iter().filter(|e| e.stored.is_some()).map(|e| e.instance).collect();
    let mut state_index: HashMap<VarId, usize> = HashMap::new();
    let mut state_list: Vec<VarId> = vec![];
    for (_, vs, _) in &cons {
        for v in vs {
            state_index.entry(*v).or_insert_with(|| {
                state_list.push(*v);
                state_list.len() - 1
            });
        }
    }
    let inc: Vec<Vec<usize>> = cons
        .iter()
        .map(|(_, vs, _)| {
            let mut c: Vec<usize> = vs.iter().map(|v| state_index[v]).collect();
            // non-stores first: the greedy start takes them
            c.sort_by_key(|&i| {
                let v = state_list[i];
                (has_store.contains(&flat.var(v).instance), std::cmp::Reverse(v.0))
            });
            c
        })
        .collect();
    let (match_eq, _) = max_matching(state_list.len(), &inc);
    if let Some(bad) = match_eq.iter().position(|m| m.is_none()) {
        let e = &flat.equations[cons[bad].0];
        let who = flat.instance_name(e.origin.instance);
        return Err(vec![Diagnostic::error(
            "INDEX-TOO-HIGH",
            format!(
                "{who}: the constraint “{} = {}” ties states that other constraints already \
                 tie (a higher-index system); the stand-in index reduction handles one \
                 differentiation only.",
                e.lhs, e.rhs
            ),
        )]);
    }
    let demoted: Vec<VarId> = match_eq.iter().map(|m| state_list[m.expect("matched")]).collect();
    // a variable for each demoted state's derivative
    let mut dvar: HashMap<VarId, VarId> = HashMap::new();
    for &v in &demoted {
        let rec = flat.var(v).clone();
        let unit = lsim_ir::units::Unit::new(rec.unit.dim / lsim_ir::units::Dim::TIME, 1.0);
        flat.vars.push(FlatVar {
            name: format!("der({})", rec.name),
            unit,
            unit_text: format!("({})/s", rec.unit_text),
            kind: VarKind::Continuous,
            start: None,
            fixed: false,
            nominal: rec.nominal,
            instance: rec.instance,
            role: VarRole::Local,
        });
        dvar.insert(v, VarId(flat.vars.len() as u32 - 1));
    }
    // the demoted states' derivatives become those variables
    for e in flat.equations.iter_mut() {
        for side in [&mut e.lhs, &mut e.rhs] {
            *side = std::mem::replace(side, Expr::Const(0.0)).rewrite(&mut |x| match x {
                Expr::Der(v) if dvar.contains_key(&v) => Expr::Var(dvar[&v]),
                other => other,
            });
        }
    }
    for &v in &demoted {
        // its start value is now only a guess
        flat.vars[v.0 as usize].fixed = false;
    }
    let d_of = |v: VarId| -> Expr {
        match dvar.get(&v) {
            Some(d) => Expr::Var(*d),
            None => Expr::Der(v),
        }
    };
    let mut out = vec![];
    for ((k, vs, g), &dem) in cons.iter().zip(&demoted) {
        let e = flat.equations[*k].clone();
        let g = g.clone();
        let Some(dg) = time_derivative(&g, vs, &d_of) else {
            let who = flat.instance_name(e.origin.instance);
            return Err(vec![Diagnostic::error(
                "INDEX-TIME",
                format!(
                    "{who}: the constraint “{} = {}” depends on time; the stand-in index \
                     reduction differentiates only constraints between states.",
                    e.lhs, e.rhs
                ),
            )]);
        };
        let mut origin = e.origin.clone();
        origin.label = Some(format!(
            "{} (differentiated)",
            origin.label.clone().unwrap_or_else(|| "a constraint".into())
        ));
        flat.equations.push(FlatEquation { lhs: Expr::Const(0.0), rhs: dg, origin });
        out.push(Constraint {
            residual: g,
            vars: vs.clone(),
            demoted: dem,
            instance: e.origin.instance,
        });
    }
    Ok(out)
}

/// A term that is always 0 but makes `solve_for` refuse the equation (its
/// derivative is not a number), so it stays implicit.
fn poison(e: &FlatEquation) -> Expr {
    let mut any_var: Option<Expr> = None;
    for side in [&e.lhs, &e.rhs] {
        side.walk(&mut |x| {
            if matches!(x, Expr::Var(_) | Expr::Der(_)) {
                let t = x.clone();
                any_var = Some(match any_var.take() {
                    None => t,
                    Some(a) => a + t,
                });
            }
        });
    }
    let never = Expr::Compare(CmpOp::Lt, Box::new(Expr::Const(1.0)), Box::new(Expr::Const(0.0)));
    Expr::NoEvent(Box::new(Expr::If(
        Box::new(never),
        Box::new(Expr::Const(f64::NAN) * any_var.unwrap_or(Expr::Const(1.0))),
        Box::new(Expr::Const(0.0)),
    )))
}

/// Whether an explicit assignment divides by a mode-dependent expression
/// that can be zero (an if-expression with a zero branch).
fn bad_pivot(e: &Expr) -> bool {
    e.any(&mut |x| match x {
        Expr::Binary(BinaryOp::Div, _, den) => den.any(&mut |y| match y {
            Expr::If(_, a, b) => {
                matches!(**a, Expr::Const(v) if v == 0.0)
                    || matches!(**b, Expr::Const(v) if v == 0.0)
            }
            _ => false,
        }),
        _ => false,
    })
}

/// Prepares `top` against `lib` (see the module doc).
pub fn prepare(
    lib: &Library,
    top: &ComponentDef,
    o: &Options,
) -> Result<Prepared, Vec<Diagnostic>> {
    let mut flat = lsim_prep::flatten::flatten(lib, top)?;
    let meters = if o.energy_meters { add_meters(&mut flat) } else { vec![] };
    let faults = lsim_prep::units_check::check(&flat, lib, top);
    if !faults.is_empty() {
        return Err(faults);
    }
    for e in flat.equations.iter_mut().chain(flat.initial_equations.iter_mut()) {
        e.lhs = wrap(std::mem::replace(&mut e.lhs, Expr::Const(0.0)));
        e.rhs = wrap(std::mem::replace(&mut e.rhs, Expr::Const(0.0)));
    }
    let (flat_vars, flat_equations) = (flat.vars.len(), flat.equations.len());
    let aliases = lsim_prep::alias::eliminate(&mut flat);
    let mut eliminated = vec![false; flat.vars.len()];
    for a in &aliases {
        eliminated[a.var.0 as usize] = true;
    }
    let constraints = reduce_index(&mut flat, &eliminated)?;
    // stored energies with the aliases substituted, for the mass metric
    let stored = flat
        .energy
        .iter()
        .filter_map(|e| e.stored.clone().map(|s| (e.instance, substitute_aliases(s, &aliases))))
        .collect();
    let mut poisoned: BTreeSet<usize> = BTreeSet::new();
    loop {
        let mut f2 = flat.clone();
        for &k in &poisoned {
            let t = poison(&f2.equations[k]);
            f2.equations[k].rhs = std::mem::replace(&mut f2.equations[k].rhs, Expr::Const(0.0)) + t;
        }
        let mut model = lsim_prep::structure::causalize(
            f2,
            aliases.clone(),
            &CausalOptions { force_implicit: o.force_implicit },
            lib,
        )?;
        let mut more = false;
        for a in &model.assignments {
            if bad_pivot(&a.expr)
                && let Some(k) = flat.equations.iter().position(|e| e.origin == a.origin)
                && poisoned.insert(k)
            {
                more = true;
            }
        }
        if !more {
            model.stats.flat_vars = flat_vars;
            model.stats.flat_equations = flat_equations;
            return Ok(Prepared { model, constraints, meters, stored });
        }
    }
}

/// Replaces eliminated variables in `e` by what they equal.
pub fn substitute_aliases(e: Expr, aliases: &[lsim_ir::AliasEntry]) -> Expr {
    let map: HashMap<VarId, lsim_ir::AliasTarget> =
        aliases.iter().map(|a| (a.var, a.target)).collect();
    e.rewrite(&mut |x| match x {
        Expr::Var(v) => match map.get(&v) {
            Some(lsim_ir::AliasTarget::Var { var, negated }) => {
                if *negated {
                    -Expr::Var(*var)
                } else {
                    Expr::Var(*var)
                }
            }
            Some(lsim_ir::AliasTarget::Const(c)) => Expr::Const(*c),
            None => Expr::Var(v),
        },
        other => other,
    })
}
