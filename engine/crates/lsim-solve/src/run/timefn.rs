//! Conditions on explicit functions of time ([`crate::TimeFunction`]):
//! their sign changes found ahead, without integrating, and reached
//! exactly; conditions that read continuous variables too checked along
//! every step for a pulse root finding cannot see.
//!
//! A function of time, parameters and discrete values alone is left out of
//! root finding (masked, as a time crossing is). [`crate::interval::
//! first_change`] finds its next sign change from the current instant on,
//! bisected to adjacent floats; the integrator stops there exactly, and
//! the crossing fires in the direction it changes sign. It is searched
//! again after it fired, and when a value it reads changed at an event. A
//! search that runs out of steps leaves a stop where it got to, and goes on
//! from there.
//!
//! A condition that also reads continuous variables (`x > sin(ω time)`)
//! stays with root finding, which sees a sign change only between a step's
//! ends: two inside one step, a pulse, are invisible to it. After every
//! step the run loop takes the step along the integrator's dense output:
//! the states and iteration variables the condition reads, as the
//! polynomials through that output (sampled at Chebyshev points, the fit
//! checked at one more), enclose the condition over any part of the step,
//! and the same search finds its first sign change there. One that root
//! finding did not report ends the step, as a root does.
//!
//! What cannot be searched (a 2-D table, `atan2` or a derivative that
//! moves; a function not defined where the search starts; a search that
//! makes no headway) is said in a warning and left to root finding.

use super::{Loop, same_instant};
use crate::info::{ChannelEnv, TimeFunction, VarSource, table_at};
use crate::interval::{Cx, Found, Iv, Poly, enclose, first_change, supported};
use crate::{Integrator, SolveError};
use lsim_ir::runtime::ModelFunctions;
use lsim_ir::{Expr, ParamId, VarId};
use std::sync::OnceLock;

/// The steps one search may take before it leaves a stop where it got to.
const BUDGET: usize = 4000;

/// Searches in a row that run out of steps before a function is left to
/// root finding (a warning says so).
const CRAWL: u32 = 64;

/// The variables an expression reads, and their values at its last search
/// (constant between events: when they change, it is searched again).
#[derive(Default)]
struct Reads {
    vars: Vec<usize>,
    at: Vec<f64>,
}

impl Reads {
    fn of(e: &Expr) -> Reads {
        let mut vars = vec![];
        e.walk(&mut |x| {
            if let Expr::Var(v) | Expr::Pre(v) = x {
                vars.push(v.0 as usize);
            }
        });
        vars.sort_unstable();
        vars.dedup();
        Reads { at: vec![f64::NAN; vars.len()], vars }
    }

    /// Whether a value changed since the last search (noted now).
    fn changed(&mut self, channels: &[f64]) -> bool {
        let mut changed = false;
        for (v, at) in self.vars.iter().zip(&mut self.at) {
            let now = channels.get(*v).copied().unwrap_or(f64::NAN);
            if now.to_bits() != at.to_bits() {
                *at = now;
                changed = true;
            }
        }
        changed
    }
}

/// The points a step is sampled at along its dense output: Chebyshev
/// (Lobatto) points of `[-1, 1]`, the ends included.
const NODES: usize = 8;

fn nodes() -> [f64; NODES] {
    std::array::from_fn(|j| -(j as f64 * std::f64::consts::PI / (NODES - 1) as f64).cos())
}

/// The inverse of the Vandermonde matrix at [`nodes`]: the coefficients of
/// the polynomial of degree 7 through values there (a dense output of
/// order up to 7 exactly).
fn inverse_vandermonde() -> &'static [[f64; NODES]; NODES] {
    static INV: OnceLock<[[f64; NODES]; NODES]> = OnceLock::new();
    INV.get_or_init(|| {
        let u = nodes();
        // [V | I] → [I | V⁻¹], partial pivoting
        let mut m = [[0.0; 2 * NODES]; NODES];
        for (i, row) in m.iter_mut().enumerate() {
            for (k, x) in row.iter_mut().take(NODES).enumerate() {
                *x = u[i].powi(k as i32);
            }
            row[NODES + i] = 1.0;
        }
        for col in 0..NODES {
            let piv = (col..NODES)
                .max_by(|a, b| m[*a][col].abs().total_cmp(&m[*b][col].abs()))
                .unwrap_or(col);
            m.swap(col, piv);
            let p = m[col][col];
            for x in m[col].iter_mut() {
                *x /= p;
            }
            for r in 0..NODES {
                if r != col {
                    let f = m[r][col];
                    if f != 0.0 {
                        let pivot_row = m[col];
                        for (x, pv) in m[r].iter_mut().zip(pivot_row) {
                            *x -= f * pv;
                        }
                    }
                }
            }
        }
        std::array::from_fn(|i| std::array::from_fn(|k| m[i][NODES + k]))
    })
}

/// A mixed condition: its function, the root functions that are it, and
/// what it reads along a step.
struct Mixed<'a> {
    roots: Vec<usize>,
    g: &'a Expr,
    /// the entries of y it reads
    y_idx: Vec<usize>,
    /// per variable read from y: (variable, its entry in `y_idx`, sign)
    leaves: Vec<(usize, usize, f64)>,
    warned: bool,
}

/// The run loop's state of the time functions.
pub(super) struct TimeFns<'a> {
    /// per root function: searched ahead ([`TimeFunction::Pure`])
    searched: Vec<bool>,
    /// per root function: the direction of the change `t_star` holds
    pub(super) dir: Vec<i32>,
    /// per root function: where a search ran out (a stop; on from there)
    clear: Vec<f64>,
    /// per root function: to be searched (again: it fired)
    pub(super) stale: Vec<bool>,
    /// per root function: searches in a row that ran out
    clears: Vec<u32>,
    /// per root function: warned that it is not defined
    warned: Vec<bool>,
    reads: Vec<Reads>,
    mixed: Vec<Mixed<'a>>,
    t_end: f64,
}

impl TimeFns<'_> {
    pub(super) fn new(n: usize) -> Self {
        TimeFns {
            searched: vec![false; n],
            dir: vec![0; n],
            clear: vec![f64::NAN; n],
            stale: vec![false; n],
            clears: vec![0; n],
            warned: vec![false; n],
            reads: (0..n).map(|_| Reads::default()).collect(),
            mixed: vec![],
            t_end: f64::INFINITY,
        }
    }

    /// Whether root function `k` is searched ahead.
    pub(super) fn searched(&self, k: usize) -> bool {
        self.searched.get(k).copied().unwrap_or(false)
    }

    /// Whether there is anything to search ahead.
    pub(super) fn any(&self) -> bool {
        self.searched.iter().any(|x| *x)
    }

    /// The next stop the time functions ask for after `t`: where a search
    /// ran out.
    pub(super) fn next_stop(&self, t: f64) -> f64 {
        let ahead = |x: f64| x > t && !same_instant(x, t);
        self.clear.iter().copied().filter(|x| ahead(*x)).fold(f64::INFINITY, f64::min)
    }
}

/// A condition along a step: time, the variables read from y as their
/// polynomials, the others as the channels hold them.
struct Along<'a> {
    t: f64,
    along: &'a [(usize, Poly)],
    vars: &'a [f64],
    params: &'a [f64],
    model: &'a dyn ModelFunctions,
}

impl lsim_ir::eval::Env for Along<'_> {
    fn time(&self) -> f64 {
        self.t
    }
    fn var(&self, v: VarId) -> f64 {
        let k = v.0 as usize;
        match self.along.iter().find(|(i, _)| *i == k) {
            Some((_, p)) => p.at(self.t),
            None => self.vars.get(k).copied().unwrap_or(f64::NAN),
        }
    }
    fn der(&self, _: VarId) -> f64 {
        f64::NAN
    }
    fn param(&self, p: ParamId) -> f64 {
        self.params[p.0 as usize]
    }
    fn table(&self, k: u32, args: &[f64]) -> f64 {
        table_at(self.model, k, args).map_or(f64::NAN, |(v, _)| v)
    }
}

impl<'a> Loop<'a> {
    /// What a condition on root function `k` is, in words.
    fn crossing_label(&self, k: usize) -> String {
        let info = self.info;
        if let Some(w) = info.whens.iter().position(|(c, _)| *c == k) {
            return info.when_labels.get(w).cloned().unwrap_or_default();
        }
        if let Some(m) = info.modes.iter().find(|m| m.crossing == k || m.crossing + 1 == k) {
            return m.label.clone();
        }
        format!("zero-crossing function {k}")
    }

    fn warn_unsearched(&mut self, k: usize, why: &str) {
        let label = self.crossing_label(k);
        let w = format!(
            "'{label}': its condition reads time in a way the run cannot search ahead ({why}); it is \
             left to root finding, which sees a condition only at the integrator's step ends and can \
             step over a pulse shorter than a step (a smaller max_step guards against that)"
        );
        if !self.warnings.contains(&w) {
            self.warnings.push(w);
        }
    }

    /// A time function not defined (NaN) where its search starts: it is
    /// not searched again before a value it reads changes (warned once).
    fn warn_undefined(&mut self, k: usize, t: f64) {
        if !std::mem::replace(&mut self.tf.warned[k], true) {
            let label = self.crossing_label(k);
            self.warnings.push(format!(
                "'{label}': its condition is not defined at t = {t:.6} s (a NaN): it is not \
                 watched until a value it reads changes"
            ));
        }
    }

    /// Sets up the time functions at the start: which the run loop
    /// searches (returned: the root functions to mask), what the mixed
    /// ones read along a step; warns about the others.
    pub(super) fn time_functions_start(&mut self, t_end: f64) -> Vec<bool> {
        let info = self.info;
        let n = self.tf.searched.len();
        let mut mask = vec![false; n];
        self.tf.t_end = t_end;
        let unsupported = "a 2-D table, atan2 or a derivative in time";
        for (k, f) in info.time_functions.iter().enumerate().take(n) {
            match f {
                Some(TimeFunction::Pure(g)) => {
                    let moves = |e: &Expr| e.any(&mut |x| matches!(x, Expr::Time));
                    if supported(g, &info.table_breaks, &moves) {
                        mask[k] = true;
                        self.tf.searched[k] = true;
                        self.tf.stale[k] = true;
                        self.tf.reads[k] = Reads::of(g);
                    } else {
                        self.warn_unsearched(k, unsupported);
                    }
                }
                Some(TimeFunction::Mixed(g)) => {
                    if let Some(m) = self.tf.mixed.iter_mut().find(|m| m.g == g) {
                        m.roots.push(k);
                        continue;
                    }
                    match self.mixed_of(g) {
                        Some(m) => self.tf.mixed.push(Mixed { roots: vec![k], ..m }),
                        None => self.warn_unsearched(k, unsupported),
                    }
                }
                Some(TimeFunction::Unhandled) => {
                    self.warn_unsearched(k, "its definition is too large or too deep to resolve")
                }
                None => {}
            }
        }
        mask
    }

    /// The next sign change of time function `k` after `t` (the discrete
    /// values as the channels hold them now).
    fn search(&self, k: usize, t: f64) -> Found {
        let Some(Some(TimeFunction::Pure(g))) = self.info.time_functions.get(k) else {
            return Found::Nothing;
        };
        let (params, vars, model) = (&self.info.params[..], &self.vars[..], self.model);
        let cx = Cx { params, vars, model, breaks: &self.info.table_breaks, along: &[] };
        let mut p = |s: f64| lsim_ir::eval::eval(g, &ChannelEnv { t: s, vars, params, model });
        let mut enc = |l: f64, r: f64| {
            let j = enclose(g, &cx, Iv { lo: l, hi: r });
            (j.v, j.d)
        };
        first_change(&mut p, &mut enc, t, self.tf.t_end, BUDGET)
    }

    /// What a mixed condition reads along a step (`None`: a variable it
    /// reads moves but is not an entry of y, or what it computes cannot
    /// be enclosed).
    fn mixed_of(&self, g: &'a Expr) -> Option<Mixed<'a>> {
        let info = self.info;
        let mut y_idx: Vec<usize> = vec![];
        let mut leaves: Vec<(usize, usize, f64)> = vec![];
        let mut ok = true;
        g.walk(&mut |x| {
            let (Expr::Var(v) | Expr::Pre(v)) = x else { return };
            let k = v.0 as usize;
            let (i, sign) = match info.var_sources.get(k).copied() {
                Some(VarSource::Y(i)) => (i, 1.0),
                Some(VarSource::NegY(i)) => (i, -1.0),
                Some(
                    VarSource::D(_) | VarSource::NegD(_) | VarSource::Const(_) | VarSource::U(_),
                ) => {
                    return;
                }
                _ => {
                    ok = false;
                    return;
                }
            };
            if leaves.iter().any(|(w, _, _)| *w == k) {
                return;
            }
            let pos = y_idx.iter().position(|j| *j == i).unwrap_or_else(|| {
                y_idx.push(i);
                y_idx.len() - 1
            });
            leaves.push((k, pos, sign));
        });
        let read: Vec<usize> = leaves.iter().map(|x| x.0).collect();
        let moves = |e: &Expr| {
            e.any(&mut |x| match x {
                Expr::Time => true,
                Expr::Var(v) | Expr::Pre(v) => read.contains(&(v.0 as usize)),
                _ => false,
            })
        };
        (ok && supported(g, &info.table_breaks, &moves)).then_some(Mixed {
            roots: vec![],
            g,
            y_idx,
            leaves,
            warned: false,
        })
    }

    /// The first sign change of a mixed condition inside the step `(t0,
    /// t1]` that root finding did not report: along the integrator's dense
    /// output, before `t1` by more than an instant, in a direction one of
    /// its root functions is watched in. Returns its time, and per root
    /// function the direction (+1, -1; 0 for the others).
    pub(super) fn scan_mixed(
        &mut self,
        integ: &mut dyn Integrator,
        t0: f64,
        t1: f64,
        d: &[f64],
    ) -> Result<Option<(f64, Vec<i32>)>, SolveError> {
        if self.tf.mixed.is_empty() || t1 <= t0 || same_instant(t0, t1) {
            return Ok(None);
        }
        let (c, s) = (0.5 * (t0 + t1), 0.5 * (t1 - t0));
        let u = nodes();
        let inv = inverse_vandermonde();
        let mut best: Option<(f64, usize, bool)> = None;
        for j in 0..self.tf.mixed.len() {
            let m = &self.tf.mixed[j];
            let ny = m.y_idx.len();
            // the entries of y it reads at the nodes, and at one more
            let mut f = vec![[0.0; NODES]; ny];
            let mut buf = vec![0.0; ny];
            for (q, uq) in u.iter().enumerate() {
                let tau = match q {
                    0 => t0,
                    _ if q == NODES - 1 => t1,
                    _ => c + s * uq,
                };
                integ.interpolate_select(tau, &m.y_idx, &mut buf)?;
                for (fi, b) in f.iter_mut().zip(&buf) {
                    fi[q] = *b;
                }
            }
            let check = c + 0.37 * s;
            integ.interpolate_select(check, &m.y_idx, &mut buf)?;
            let polys: Vec<Poly> = f
                .iter()
                .zip(&buf)
                .map(|(fi, at_check)| {
                    let a: Vec<f64> =
                        (0..NODES).map(|k| (0..NODES).map(|q| inv[k][q] * fi[q]).sum()).collect();
                    let mut p = Poly { c, s, a, err: 0.0 };
                    let scale: f64 = p.a.iter().map(|x| x.abs()).sum::<f64>()
                        + fi.iter().fold(0.0f64, |x, y| x.max(y.abs()));
                    p.err = 256.0 * f64::EPSILON * scale + 4.0 * (p.at(check) - at_check).abs();
                    p
                })
                .collect();
            let along: Vec<(usize, Poly)> = m
                .leaves
                .iter()
                .map(|(v, pos, sign)| {
                    let p = &polys[*pos];
                    let a = p.a.iter().map(|x| sign * x).collect();
                    (*v, Poly { a, ..p.clone() })
                })
                .collect();
            let (params, vars, model) = (&self.info.params[..], &self.vars[..], self.model);
            let g = m.g;
            let cx = Cx { params, vars, model, breaks: &self.info.table_breaks, along: &along };
            let mut p =
                |t: f64| lsim_ir::eval::eval(g, &Along { t, along: &along, vars, params, model });
            let mut enc = |l: f64, r: f64| {
                let e = enclose(g, &cx, Iv { lo: l, hi: r });
                (e.v, e.d)
            };
            let limit = best.map_or(t1, |b| b.0);
            let mut from = t0;
            let watched = |rising: bool| {
                m.roots.iter().any(|k| {
                    let d = self.info.root_dirs.get(*k).copied().unwrap_or(0);
                    d == 0 || (d > 0) == rising
                })
            };
            let mut trouble = None;
            loop {
                match first_change(&mut p, &mut enc, from, limit, BUDGET) {
                    Found::Change { at, rising } => {
                        if same_instant(at, limit) || at >= limit {
                            break;
                        }
                        if watched(rising) {
                            best = Some((at, j, rising));
                            break;
                        }
                        from = at;
                    }
                    Found::Nothing => break,
                    Found::Clear(at) => {
                        trouble = Some(format!(
                            "its search along the step makes no headway at t = {at:.6} s"
                        ));
                        break;
                    }
                    Found::Undefined => {
                        trouble = Some(format!("it is not defined at t = {from:.6} s"));
                        break;
                    }
                }
            }
            if let Some(why) = trouble
                && !std::mem::replace(&mut self.tf.mixed[j].warned, true)
            {
                let k = self.tf.mixed[j].roots[0];
                let label = self.crossing_label(k);
                self.warnings.push(format!(
                    "'{label}': a step could not be checked for a pulse of its condition ({why}); \
                     root finding alone watches it there, and can step over a pulse shorter than a \
                     step (a smaller max_step guards against that)"
                ));
            }
        }
        let Some((at, j, rising)) = best else { return Ok(None) };
        let mut dirs = vec![0; self.sides.len()];
        for &k in &self.tf.mixed[j].roots {
            let dk = self.info.root_dirs.get(k).copied().unwrap_or(0);
            if dk == 0 || (dk > 0) == rising {
                dirs[k] = if rising { 1 } else { -1 };
            }
        }
        // where the model's own root function, on the dense output, is on
        // the new side (the polynomials agree with it to round-off; the
        // event iteration decides the conditions with it)
        let k = dirs.iter().position(|x| *x != 0).unwrap_or(self.tf.mixed[j].roots[0]);
        Ok(self.model_side(integ, k, at, rising, t1, d)?.map(|at| (at, dirs)))
    }

    /// The first time from `at` on (to `limit`) at which root function `k`
    /// of the model, on the dense output, is on the side a change in
    /// direction `rising` leads to; `None` when it is not before `limit`.
    fn model_side(
        &mut self,
        integ: &mut dyn Integrator,
        k: usize,
        at: f64,
        rising: bool,
        limit: f64,
        d: &[f64],
    ) -> Result<Option<f64>, SolveError> {
        let mut y = integ.y().to_vec();
        let mut g = |lp: &mut Self, t: f64| -> Result<f64, SolveError> {
            integ.interpolate(t, &mut y)?;
            lp.eval_raw(t, &y, d);
            Ok(lp.raw[k])
        };
        // (strictly: an exact zero counts as the side it came from)
        let there = |x: f64| if rising { x > 0.0 } else { x < 0.0 };
        if there(g(self, at)?) {
            return Ok(Some(at));
        }
        // forward, doubling, then bisected to adjacent floats
        let (mut lo, mut step) = (at, at.next_up() - at);
        let mut hi = loop {
            let hi = (lo + step).min(limit);
            if there(g(self, hi)?) {
                break hi;
            }
            if hi >= limit {
                return Ok(None);
            }
            lo = hi;
            step *= 2.0;
        };
        loop {
            let m = lo + 0.5 * (hi - lo);
            if m <= lo || m >= hi {
                return Ok(Some(hi));
            }
            if there(g(self, m)?) {
                hi = m;
            } else {
                lo = m;
            }
        }
    }

    /// Schedules the time functions' next sign changes after `t` (the
    /// channels just sampled): those not searched yet, those that fired or
    /// ran out of steps here, and those that read a value that changed.
    pub(super) fn schedule_functions(
        &mut self,
        integ: &mut dyn Integrator,
        t: f64,
        y: &[f64],
    ) -> Result<(), SolveError> {
        if !self.tf.any() {
            return Ok(());
        }
        let here = |x: f64| x == t || same_instant(x, t);
        let mut given_up = false;
        for k in 0..self.tf.searched.len() {
            if !self.tf.searched[k] {
                continue;
            }
            let ran_out = here(self.tf.clear[k]);
            let changed = self.tf.reads[k].changed(&self.vars);
            let stale = std::mem::take(&mut self.tf.stale[k]);
            if !(changed || ran_out || stale) {
                continue;
            }
            self.tf.clear[k] = f64::NAN;
            self.t_star[k] = f64::NAN;
            match self.search(k, t) {
                Found::Change { at, rising } => {
                    self.t_star[k] = at;
                    self.tf.dir[k] = if rising { 1 } else { -1 };
                    self.tf.clears[k] = 0;
                }
                Found::Clear(at) => {
                    self.tf.clear[k] = at;
                    self.tf.clears[k] += 1;
                    if self.tf.clears[k] > CRAWL {
                        // no headway: root finding takes it on
                        self.tf.searched[k] = false;
                        self.tf.clear[k] = f64::NAN;
                        given_up = true;
                        let why = format!("its search makes no headway at t = {t:.6} s");
                        self.warn_unsearched(k, &why);
                    }
                }
                Found::Nothing => self.tf.clears[k] = 0,
                Found::Undefined => self.warn_undefined(k, t),
            }
            if !self.use_timed && self.t_star[k].is_finite() {
                // (a backend that watches every root function: the sign
                // change is a stop, where root finding sees it)
                self.tf.clear[k] = self.t_star[k];
                self.t_star[k] = f64::NAN;
            }
        }
        if given_up && self.use_timed {
            // (the integrator watches it again from a restart here, so
            // that its root values are taken afresh)
            let mask: Vec<bool> = (0..self.sides.len()).map(|c| self.timed(c)).collect();
            integ.set_root_mask(&mask);
            integ.restart(t, y)?;
        }
        Ok(())
    }

    /// Whether time function `k`'s sign changes at `t` with the discrete
    /// values the channels hold now, and the side it takes just after:
    /// `Some(true)` positive.
    pub(super) fn function_now(&self, k: usize, t: f64) -> Option<bool> {
        let Some(Some(TimeFunction::Pure(g))) = self.info.time_functions.get(k) else {
            return None;
        };
        let (params, vars, model) = (&self.info.params[..], &self.vars[..], self.model);
        let at = |s: f64| lsim_ir::eval::eval(g, &ChannelEnv { t: s, vars, params, model });
        let dt = (16.0 * f64::EPSILON * t.abs()).max(f64::MIN_POSITIVE);
        let (before, after) = (at(t - dt), at(t + dt));
        let changes = (before < 0.0 && after >= 0.0) || (before > 0.0 && after <= 0.0);
        changes.then_some(after > 0.0)
    }
}
