//! The impulse projection at a rigid engagement (DESIGN.md, *Events*;
//! [`crate::ImpulseInfo`]).
//!
//! An engagement's `changes` took a new value at this event (a gearbox's
//! selected ratio): the speeds its rigid coupling ties together jump as an
//! instantaneous, rigid engagement makes them, in two stages.
//!
//! 1. **The rigid engagement.** The states rigidly coupled to what the
//!    engagement changed (through the assignments: a rotor's speed follows
//!    the wheels' through the gears) move so that the momentum the stored
//!    energies weigh is kept: `Bᵀ (∇E(U(x)) − ∇E(U⁻)) = 0` over those
//!    states `x`, with `U` the variables the stored energies read, `U⁻`
//!    their values before the event, `B = ∂U/∂x`. For kinetic energies
//!    `½ Uᵀ M U` that is `Bᵀ M (U − U⁻) = 0`: the perfectly inelastic
//!    engagement, the nearest consistent velocities in the metric of the
//!    masses; for a gear between two inertias `w_out⁺ = (J_out w_out⁻ +
//!    r J_in w_in⁻) / (J_out + r² J_in)`. The kinetic energy that loses is
//!    the engaging part's (`E⁻ − E_a ≥ 0`: a projection).
//! 2. **The couplings that pass the impulse on relax.** Only a coupling a
//!    model declares stiff and unbounded (an [`crate::ImpulseLink`])
//!    passes an impulse on. A part whose forces are bounded passes none in
//!    zero time, a tyre included (its force is at most μ N, within its grip
//!    as at it): the integrator follows its relative velocity after the
//!    event with the part's own law, and its loss is its own. A link whose
//!    `active` holds at the state stage 1 leaves relaxes to its relative
//!    velocity before the event, exchanging momentum between what it
//!    couples: the same balance over the states the active links reach,
//!    each keeping `keep_l = κ⁻_l` with its impulse `λ_l`. A link whose
//!    `active` comes to hold where the others' relaxation leaves the
//!    states joins them and the stage is solved again (the set only grows,
//!    so this ends). The kinetic energy that loses is the link's when one
//!    link passes the impulse on; when several do, how they share it
//!    depends on their stiffnesses, which they do not declare, and the
//!    event books it as a whole (a warning says so). The two stages end
//!    where one projection keeping every active link would (stage 1's
//!    change is orthogonal, in the masses' metric, to everything stage 2
//!    can move), so the total loss is the same; a fully resolved stiff,
//!    unbounded link approaches the split as its stiffness grows.
//!
//! The derivatives are exact: `B` and the links' gradients by forward-mode
//! differentiation through the model's assignments (iteration variables'
//! sensitivities `∂z/∂x = −g_z⁻¹ g_x` from the compiled Jacobian), the
//! stored energies' gradients and Hessians by second-order forward
//! differentiation; Newton's method solves the balance (one step for
//! quadratic energies and linear kinematics, a few for others). States a
//! `reinit` set at this event stay where it put them. The losses are the
//! stored energies of the coupled parts before and after each stage:
//! nothing else that jumps at the same instant is counted.

use super::Loop;
use crate::ad::{self, Jet, JetEnv};
use crate::energy::Engagement;
use crate::info::{ChannelEnv, ImpulseInfo, VarSource};
use crate::{Integrator, SolveError};
use lsim_ir::runtime::EvalInput;
use lsim_ir::{Expr, ParamId, VarId};
use std::collections::{BTreeSet, HashMap};

/// The variables a projection reads, with their derivatives with respect to
/// the free states, computed from y, d and u through the chain.
struct Store<'a> {
    t: f64,
    params: &'a [f64],
    y: &'a [f64],
    d: &'a [f64],
    u: &'a [f64],
    n_x: usize,
    sources: &'a [VarSource],
    /// entry of y → its seed
    seed_of: &'a HashMap<usize, usize>,
    /// per iteration variable: ∂z/∂(free states)
    zsens: Option<&'a [Vec<f64>]>,
    n: usize,
    vals: HashMap<usize, Jet>,
    ders: HashMap<usize, f64>,
}

impl Store<'_> {
    fn y_jet(&self, i: usize, sign: f64) -> Jet {
        let v = sign * self.y[i];
        if let Some(&s) = self.seed_of.get(&i) {
            let mut j = Jet::seed(v, s, self.n, false);
            j.g[s] = sign;
            j
        } else if i >= self.n_x
            && let Some(zs) = self.zsens
        {
            Jet::linear(v, zs[i - self.n_x].iter().map(|x| sign * x).collect(), false)
        } else {
            Jet::constant(v, self.n, false)
        }
    }

    fn leaf(&self, k: usize) -> Result<Jet, String> {
        let c = |v: f64| Jet::constant(v, self.n, false);
        Ok(match self.sources.get(k).copied() {
            Some(VarSource::Y(i)) => self.y_jet(i, 1.0),
            Some(VarSource::NegY(i)) => self.y_jet(i, -1.0),
            Some(VarSource::D(i)) => c(self.d[i]),
            Some(VarSource::NegD(i)) => c(-self.d[i]),
            Some(VarSource::U(i)) => c(self.u[i]),
            Some(VarSource::Const(x)) => c(x),
            _ => return Err(format!("variable {k} is computed by nothing the projection knows")),
        })
    }

    /// Runs the chain's steps `steps`.
    fn run(&mut self, imp: &ImpulseInfo, steps: &[usize]) -> Result<(), String> {
        for &k in steps {
            let (v, der, e) = &imp.chain[k];
            let j = ad::eval(e, self)?;
            if !j.v.is_finite() {
                return Err(format!("variable {v} is not finite at the event"));
            }
            if *der {
                self.ders.insert(*v, j.v);
            } else {
                self.vals.insert(*v, j);
            }
        }
        Ok(())
    }

    fn get(&self, v: usize) -> Result<Jet, String> {
        match self.vals.get(&v) {
            Some(j) => Ok(j.clone()),
            None => self.leaf(v),
        }
    }
}

impl JetEnv for Store<'_> {
    fn n(&self) -> usize {
        self.n
    }
    fn second(&self) -> bool {
        false
    }
    fn time(&self) -> f64 {
        self.t
    }
    fn var(&self, v: VarId) -> Result<Jet, String> {
        self.get(v.0 as usize)
    }
    fn param(&self, p: ParamId) -> f64 {
        self.params[p.0 as usize]
    }
    fn der(&self, v: VarId) -> f64 {
        self.ders.get(&(v.0 as usize)).copied().unwrap_or(f64::NAN)
    }
}

/// A stored energy as a function of the variables it reads, with its
/// gradient and Hessian (second-order seeds on those variables).
struct EnergyEnv<'a> {
    t: f64,
    params: &'a [f64],
    at: &'a [usize],
    vals: &'a [f64],
    second: bool,
}

impl JetEnv for EnergyEnv<'_> {
    fn n(&self) -> usize {
        self.at.len()
    }
    fn second(&self) -> bool {
        self.second
    }
    fn time(&self) -> f64 {
        self.t
    }
    fn var(&self, v: VarId) -> Result<Jet, String> {
        let k = v.0 as usize;
        match self.at.iter().position(|x| *x == k) {
            Some(i) => Ok(Jet::seed(self.vals[i], i, self.at.len(), self.second)),
            None => Err(format!("variable {k} is not one the stored energy reads")),
        }
    }
    fn param(&self, p: ParamId) -> f64 {
        self.params[p.0 as usize]
    }
}

/// The balance a projection solves: the coupled stored energies (part,
/// the variables it reads, its gradient before the event), the links with
/// their targets.
struct Balance<'a> {
    parts: Vec<(usize, Vec<usize>, Vec<f64>)>,
    links: &'a [(usize, f64)],
}

impl Loop<'_> {
    /// The impulse projection at `t` when a rigid engagement's `changes`
    /// took a new value since `ref_vars` (the channels before, with the
    /// discrete values `ref_d`; `d` now). Moves the states of `y` and
    /// returns where the kinetic energy that lost went, or `None` when no
    /// engagement changed or nothing could move. `y`'s iteration variables
    /// are left for the caller to solve.
    pub(super) fn engage(
        &mut self,
        integ: &mut dyn Integrator,
        t: f64,
        y: &mut [f64],
        d: &[f64],
        ref_vars: &[f64],
        ref_d: &[f64],
    ) -> Result<Option<Engagement>, SolveError> {
        let Some(imp) = self.info.impulse.clone() else { return Ok(None) };
        if !self.opts.impulses || !imp.may_engage(d, ref_d) {
            return Ok(None);
        }
        let l = *self.model.layout();
        let reads_z = l.n_z > 0 && imp.reads_z();
        if reads_z && !self.info.events_read_z {
            integ.consistent_z(t, y, d)?;
        }
        self.sample(t, y, d);
        let cur = self.vars.clone();
        let params = self.info.params.clone();
        let at = |vars: &[f64], e: &Expr| {
            lsim_ir::eval::eval(e, &ChannelEnv { t, vars, params: &params })
        };
        let changed: Vec<usize> = (0..imp.engagements.len())
            .filter(|&k| {
                let e = &imp.engagements[k].changes;
                let (a, b) = (at(ref_vars, e), at(&cur, e));
                a.is_finite() && b.is_finite() && a != b
            })
            .collect();
        if changed.is_empty() {
            return Ok(None);
        }
        let moved = |u: usize| {
            let (a, b) = (ref_vars[u], cur[u]);
            a.is_finite() && b.is_finite() && (a - b).abs() > 1e-12 * a.abs().max(b.abs())
        };
        let pinned: BTreeSet<usize> =
            imp.restarts.iter().filter(|(_, j)| d[*j] != ref_d[*j]).map(|(s, _)| *s).collect();

        // stage 1: what each engagement couples rigidly; engagements whose
        // couplings share a state are one engagement
        let mut groups: Vec<(Vec<usize>, BTreeSet<usize>, BTreeSet<usize>)> = vec![];
        for &k in &changed {
            let dk = imp.expr_discretes(&imp.engagements[k].changes);
            let start: Vec<usize> = imp
                .vars()
                .iter()
                .copied()
                .filter(|&u| {
                    let dep = imp.deps(u);
                    moved(u) && (dep.z || dep.discretes.iter().any(|x| dk.contains(x)))
                })
                .collect();
            if start.is_empty() {
                continue;
            }
            let (mut vars, mut states) = imp.coupled(&start, &[]);
            let mut engs = vec![k];
            let mut g = 0;
            while g < groups.len() {
                if groups[g].2.iter().any(|s| states.contains(s)) {
                    let (e, v, s) = groups.remove(g);
                    engs.extend(e);
                    vars.extend(v);
                    states.extend(s);
                } else {
                    g += 1;
                }
            }
            groups.push((engs, vars, states));
        }
        let x_before: Vec<f64> = y[..l.n_x].to_vec();
        let mut losses: Vec<(Option<usize>, f64, bool)> = vec![];
        let mut count = 0u64;
        let mut union: BTreeSet<usize> = BTreeSet::new();
        for (engs, vars, states) in &groups {
            let free: Vec<usize> = states.iter().filter(|s| !pinned.contains(s)).copied().collect();
            if free.is_empty() {
                continue;
            }
            let parts = imp.parts_touching(vars);
            let balance = match self.balance(t, &imp, &parts, ref_vars, &[]) {
                Ok(b) => b,
                Err(why) => {
                    self.warnings.push(format!(
                        "at t = {t:.6} s a rigid engagement could not keep the momentum ({why}): \
                         the speeds jumped to the new couplings as they stood"
                    ));
                    continue;
                }
            };
            match self.solve(integ, t, y, d, &imp, &free, &balance)? {
                Ok(_) => {}
                Err(why) => {
                    y[..l.n_x].copy_from_slice(&x_before);
                    self.warnings.push(format!(
                        "at t = {t:.6} s a rigid engagement could not keep the momentum ({why}): \
                         the speeds jumped to the new couplings as they stood"
                    ));
                    return Ok(None);
                }
            }
            if reads_z {
                integ.consistent_z(t, y, d)?;
            }
            self.sample(t, y, d);
            let after = self.vars.clone();
            let lost = self.stored(t, &parts, ref_vars) - self.stored(t, &parts, &after);
            let part = imp.engagements[engs[0]].part;
            if engs.len() > 1 {
                self.warnings.push(format!(
                    "at t = {t:.6} s {} rigid engagements changed together on one coupling: the \
                     kinetic energy their engagement lost ({lost:.6e} J) is booked to the first",
                    engs.len()
                ));
            }
            losses.push((part, lost, false));
            union.extend(vars.iter().copied());
            count += 1;
        }
        if count == 0 {
            y[..l.n_x].copy_from_slice(&x_before);
            return Ok(None);
        }

        // stage 2: the links that pass the impulse on, judged at the state
        // the rigid stage left, relax to their relative velocity before the
        // event
        let y_a = y.to_vec();
        self.sample(t, y, d);
        let vars_a = self.vars.clone();
        let start: Vec<usize> = union.iter().copied().collect();
        let mut on: Vec<usize> =
            (0..imp.links.len()).filter(|&lk| at(&vars_a, &imp.links[lk].active) != 0.0).collect();
        loop {
            let (vars_b, states_b) = imp.coupled(&start, &on);
            let links_in: Vec<usize> = on
                .iter()
                .copied()
                .filter(|&lk| imp.link_vars(lk).iter().any(|u| vars_b.contains(u)))
                .collect();
            if links_in.is_empty() {
                break;
            }
            let free: Vec<usize> =
                states_b.iter().filter(|s| !pinned.contains(s)).copied().collect();
            let targets: Vec<(usize, f64)> =
                links_in.iter().map(|&lk| (lk, at(ref_vars, &imp.links[lk].keep))).collect();
            let parts = imp.parts_touching(&vars_b);
            let balance = match self.balance(t, &imp, &parts, &vars_a, &targets) {
                Ok(b) => b,
                Err(why) => {
                    self.warnings.push(format!(
                        "at t = {t:.6} s the couplings that pass a rigid engagement's impulse \
                         on could not relax ({why}): what they couple kept its speed"
                    ));
                    break;
                }
            };
            y.copy_from_slice(&y_a);
            let lambda = match self.solve(integ, t, y, d, &imp, &free, &balance)? {
                Ok(lam) => lam,
                Err(why) => {
                    y.copy_from_slice(&y_a);
                    self.warnings.push(format!(
                        "at t = {t:.6} s the couplings that pass a rigid engagement's impulse \
                         on could not relax ({why}): what they couple kept its speed"
                    ));
                    break;
                }
            };
            if reads_z {
                integ.consistent_z(t, y, d)?;
            }
            self.sample(t, y, d);
            let after = self.vars.clone();
            // a link that did not pass the impulse on at the state the rigid
            // stage left, but does where the others' relaxation leaves it,
            // takes part too (the set only grows: this ends)
            let joins: Vec<usize> = (0..imp.links.len())
                .filter(|lk| !on.contains(lk) && at(&after, &imp.links[*lk].active) != 0.0)
                .collect();
            if !joins.is_empty() {
                on.extend(joins);
                y.copy_from_slice(&y_a);
                continue;
            }
            let lost = self.stored(t, &parts, &vars_a) - self.stored(t, &parts, &after);
            // the links that passed an impulse: one books what the stage
            // lost; how several share it depends on their stiffnesses,
            // which they do not declare, so the event books it as a whole
            let scale: f64 = lambda.iter().map(|l| l.abs()).sum();
            let passed: Vec<usize> = targets
                .iter()
                .zip(&lambda)
                .filter(|(_, l)| l.abs() > 1e-12 * scale)
                .map(|((lk, _), _)| *lk)
                .collect();
            match passed.as_slice() {
                [] => {}
                [lk] => losses.push((imp.links[*lk].part, lost, true)),
                _ => {
                    losses.push((None, lost, true));
                    if !self.warned_links {
                        self.warned_links = true;
                        self.warnings.push(format!(
                            "at t = {t:.6} s {} couplings passed a rigid engagement's impulse on \
                             together: the kinetic energy their relaxation lost ({lost:.6e} J) is \
                             booked to the event, not to them one by one (how they share it \
                             depends on their stiffnesses); so at every such event of this run",
                            passed.len()
                        ));
                    }
                }
            }
            break;
        }
        Ok(Some(Engagement { losses, count }))
    }

    /// The stored energies of `parts` on the channels `vars`.
    fn stored(&self, t: f64, parts: &[usize], vars: &[f64]) -> f64 {
        let Some(imp) = &self.info.impulse else { return 0.0 };
        let Some(energy) = &self.info.energy else { return 0.0 };
        let env = ChannelEnv { t, vars, params: &self.info.params };
        parts
            .iter()
            .filter_map(|&p| energy.parts[imp.parts[p].0].stored.as_ref())
            .map(|e| lsim_ir::eval::eval(e, &env))
            .sum()
    }

    /// The momentum before the event of each part in `parts` (its stored
    /// energy's gradient on the channels `before`), and the links to keep.
    fn balance<'b>(
        &self,
        t: f64,
        imp: &ImpulseInfo,
        parts: &[usize],
        before: &[f64],
        links: &'b [(usize, f64)],
    ) -> Result<Balance<'b>, String> {
        let energy = self.info.energy.as_ref().ok_or("the model keeps no energy books")?;
        let mut out = vec![];
        for &p in parts {
            let (k, at) = &imp.parts[p];
            let Some(e) = &energy.parts[*k].stored else { continue };
            let vals: Vec<f64> = at.iter().map(|v| before[*v]).collect();
            let env = EnergyEnv { t, params: &self.info.params, at, vals: &vals, second: false };
            out.push((p, at.clone(), ad::eval(e, &env)?.g));
        }
        Ok(Balance { parts: out, links })
    }

    /// Newton's method on the momentum balance over the `free` states:
    /// `Bᵀ (∇E(U(x)) − p⁻) + Aᵀ λ = 0` and `keep_l(x) = κ_l`. Moves `y`'s
    /// states; returns the links' impulses λ, or why it could not.
    #[allow(clippy::too_many_arguments, clippy::needless_range_loop)]
    fn solve(
        &mut self,
        integ: &mut dyn Integrator,
        t: f64,
        y: &mut [f64],
        d: &[f64],
        imp: &ImpulseInfo,
        free: &[usize],
        b: &Balance<'_>,
    ) -> Result<Result<Vec<f64>, String>, SolveError> {
        let info = self.info;
        let l = *self.model.layout();
        let n_x = l.n_x;
        let reads_z = l.n_z > 0 && imp.reads_z();
        let nl = b.links.len();
        let mut need: BTreeSet<usize> = BTreeSet::new();
        for (_, at, _) in &b.parts {
            need.extend(at.iter().copied());
        }
        for (lk, _) in b.links {
            need.extend(imp.link_vars(*lk).iter().copied());
        }
        let steps = imp.steps_for(&need);
        let mut lam = vec![0.0; nl];
        let mut free: Vec<usize> = free.to_vec();
        let u = self.u.to_vec();
        for iteration in 0..25 {
            let nf = free.len();
            if nf == 0 {
                return Ok(Err("nothing to move".into()));
            }
            let seed_of: HashMap<usize, usize> =
                free.iter().enumerate().map(|(i, s)| (*s, i)).collect();
            let zsens = if reads_z {
                integ.consistent_z(t, y, d)?;
                match self.z_sensitivity(t, y, d, &free) {
                    Some(z) => Some(z),
                    None => return Ok(Err("the iteration variables' Jacobian is singular".into())),
                }
            } else {
                None
            };
            let mut store = Store {
                t,
                params: &info.params,
                y,
                d,
                u: &u,
                n_x,
                sources: &info.var_sources,
                seed_of: &seed_of,
                zsens: zsens.as_deref(),
                n: nf,
                vals: HashMap::new(),
                ders: HashMap::new(),
            };
            if let Err(why) = store.run(imp, &steps) {
                return Ok(Err(why));
            }
            // r1 = Bᵀ (∇E(U) − p⁻) + Aᵀ λ, J = Bᵀ ∇²E B
            let mut r1 = vec![0.0; nf];
            let mut jm = vec![0.0; nf * nf];
            let energy = info.energy.as_ref().expect("a projection needs the energy books");
            for (p, at, g_before) in &b.parts {
                let e = energy.parts[imp.parts[*p].0].stored.as_ref().expect("a stored energy");
                let mut jets = vec![];
                for v in at {
                    match store.get(*v) {
                        Ok(j) => jets.push(j),
                        Err(why) => return Ok(Err(why)),
                    }
                }
                let vals: Vec<f64> = jets.iter().map(|j| j.v).collect();
                let env = EnergyEnv { t, params: &info.params, at, vals: &vals, second: true };
                let ej = match ad::eval(e, &env) {
                    Ok(j) => j,
                    Err(why) => return Ok(Err(why)),
                };
                let m = at.len();
                for a in 0..m {
                    let ga = ej.g[a] - g_before[a];
                    for i in 0..nf {
                        r1[i] += jets[a].g[i] * ga;
                    }
                    for c in 0..m {
                        let h = ej.h[a * m + c];
                        if h == 0.0 {
                            continue;
                        }
                        for i in 0..nf {
                            let bi = jets[a].g[i];
                            if bi == 0.0 {
                                continue;
                            }
                            for j in 0..nf {
                                jm[i * nf + j] += bi * h * jets[c].g[j];
                            }
                        }
                    }
                }
            }
            // the links: keep_l(x) − κ_l and its gradient
            let mut r2 = vec![0.0; nl];
            let mut am = vec![0.0; nl * nf];
            for (li, (lk, target)) in b.links.iter().enumerate() {
                let kj = match ad::eval(&imp.links[*lk].keep, &store) {
                    Ok(j) => j,
                    Err(why) => return Ok(Err(why)),
                };
                r2[li] = kj.v - target;
                am[li * nf..(li + 1) * nf].copy_from_slice(&kj.g);
                for i in 0..nf {
                    r1[i] += kj.g[i] * lam[li];
                }
            }
            // states with no inertia and no link stay out
            let moving: Vec<bool> = (0..nf)
                .map(|i| {
                    (0..nf).any(|j| jm[i * nf + j] != 0.0)
                        || (0..nl).any(|li| am[li * nf + i] != 0.0)
                })
                .collect();
            if moving.iter().any(|m| !m) {
                free = free.iter().zip(&moving).filter(|(_, m)| **m).map(|(s, _)| *s).collect();
                continue;
            }
            // [J Aᵀ; A 0] [dx; dλ] = −[r1; r2]
            let n = nf + nl;
            let mut trip = vec![];
            let mut rhs = vec![0.0; n];
            for i in 0..nf {
                for j in 0..nf {
                    if jm[i * nf + j] != 0.0 {
                        trip.push((i, j, jm[i * nf + j]));
                    }
                }
                rhs[i] = -r1[i];
            }
            for li in 0..nl {
                for i in 0..nf {
                    let a = am[li * nf + i];
                    if a != 0.0 {
                        trip.push((nf + li, i, a));
                        trip.push((i, nf + li, a));
                    }
                }
                rhs[nf + li] = -r2[li];
            }
            let Some(step) = crate::init::lin_solve(n, &trip, &rhs) else {
                return Ok(Err("the engagement's balance is singular".into()));
            };
            let mut size = 0.0f64;
            let mut scale = 0.0f64;
            for (i, s) in free.iter().enumerate() {
                y[*s] += step[i];
                size = size.max(step[i].abs());
                scale = scale.max(y[*s].abs()).max(info.y_nominal[*s] * self.opts.atol);
            }
            for li in 0..nl {
                lam[li] += step[nf + li];
            }
            // (a quadratic energy over linear kinematics is solved by the
            // first step; the second confirms it to round-off)
            if iteration > 0 && size <= 1e-13 * scale {
                return Ok(Ok(lam));
            }
        }
        Ok(Err("Newton's method did not converge".into()))
    }

    /// ∂z/∂x over the `free` states, z the iteration variables (solved
    /// consistent with the states held): `−g_z⁻¹ g_x`, from the compiled
    /// Jacobian. One row per iteration variable.
    fn z_sensitivity(
        &mut self,
        t: f64,
        y: &[f64],
        d: &[f64],
        free: &[usize],
    ) -> Option<Vec<Vec<f64>>> {
        let l = *self.model.layout();
        let (n, n_x, n_z) = (l.n_y(), l.n_x, l.n_z);
        let mut jac = vec![0.0; n * n];
        let inp = EvalInput { t, y, p: &self.info.params, d, u: self.u };
        self.model.jacobian_dense(&inp, &mut self.work, &mut jac);
        // column-major: ∂row/∂col at jac[col * n + row]
        let mut trip = vec![];
        for j in 0..n_z {
            for i in 0..n_z {
                let v = jac[(n_x + j) * n + n_x + i];
                if v != 0.0 {
                    trip.push((i, j, v));
                }
            }
        }
        let mut out = vec![vec![0.0; free.len()]; n_z];
        for (f, &s) in free.iter().enumerate() {
            let rhs: Vec<f64> = (0..n_z).map(|i| -jac[s * n + n_x + i]).collect();
            let sol = crate::init::lin_solve(n_z, &trip, &rhs)?;
            for i in 0..n_z {
                out[i][f] = sol[i];
            }
        }
        Some(out)
    }
}
