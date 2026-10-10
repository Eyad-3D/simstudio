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
//! A condition that also reads continuous variables (`x > sin(ω time)`,
//! a driving cycle's target against a speed) stays with root finding,
//! which sees a sign change only between a step's ends: two inside one
//! step, a pulse, are invisible to it. Every step is checked for one
//! ([`super::mixed`]): a certificate that the condition keeps its sign
//! over a window and a box of its states clears most steps with a few
//! comparisons; elsewhere the states and iteration variables the
//! condition reads, as the polynomials through the integrator's dense
//! output (sampled at Chebyshev points, the fit checked at one more),
//! enclose it over any part of the step, and the same search finds its
//! first sign change there. One that root finding did not report ends the
//! step, as a root does. One whose value moves only where a `noEvent`
//! comparison flips is left to root finding, as `noEvent` asks.
//!
//! What cannot be searched (`atan2` or a derivative that moves, a table
//! whose points are not known; a function not defined where the search
//! starts; a search that makes no headway) is said in a warning and left
//! to root finding.

use super::mixed::Kind;
use super::{Loop, same_instant};
use crate::info::{ChannelEnv, TimeFunction};
use crate::interval::{Cx, Found, Grid2, Iv, enclose, first_change, supported};
use crate::{Integrator, SolveError};
use lsim_ir::Expr;

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
    pub(super) mixed: Vec<super::mixed::Mixed<'a>>,
    pub(super) t_end: f64,
    /// per table: a 2-D table's grid, its patches fitted as enclosures
    /// need them
    pub(super) grids: Vec<Option<Grid2>>,
    /// the entries of y the mixed conditions read
    pub(super) mixed_y: Vec<usize>,
    /// scratch of their check: the states they read at a step's end, at
    /// its middle (and those alone), and where each may stray within it
    pub(super) y1: Vec<f64>,
    pub(super) ym: Vec<f64>,
    pub(super) mid: Vec<f64>,
    pub(super) span: Vec<[f64; 2]>,
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
            grids: vec![],
            mixed_y: vec![],
            y1: vec![],
            ym: vec![],
            mid: vec![],
            span: vec![],
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

impl<'a> Loop<'a> {
    /// What a condition on root function `k` is, in words.
    pub(super) fn crossing_label(&self, k: usize) -> String {
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
        self.tf.grids = info
            .table_axes
            .iter()
            .enumerate()
            .map(|(k, [x, y])| Grid2::new(k as u32, x, y))
            .collect();
        let unsupported = "atan2 or a derivative in time, or a table whose points are not known";
        for (k, f) in info.time_functions.iter().enumerate().take(n) {
            match f {
                Some(TimeFunction::Pure(g)) => {
                    let moves = |e: &Expr| e.any(&mut |x| matches!(x, Expr::Time));
                    if supported(g, &info.table_breaks, &self.tf.grids, &moves) {
                        mask[k] = true;
                        self.tf.searched[k] = true;
                        self.tf.stale[k] = true;
                        self.tf.reads[k] = Reads::of(g);
                    } else {
                        self.warn_unsearched(k, unsupported);
                    }
                }
                Some(TimeFunction::Mixed { chain, g }) => {
                    if let Some(m) = self.tf.mixed.iter_mut().find(|m| m.same(chain, g)) {
                        m.roots.push(k);
                        continue;
                    }
                    match self.mixed_of(chain, g) {
                        Kind::Scan(mut m) => {
                            m.roots.push(k);
                            self.tf.mixed.push(*m);
                        }
                        Kind::Switched => {}
                        Kind::Unsupported => self.warn_unsearched(k, unsupported),
                    }
                }
                Some(TimeFunction::Unhandled) => {
                    self.warn_unsearched(k, "its definition is too large or too deep to resolve")
                }
                None => {}
            }
        }
        let mut ys: Vec<usize> = self.tf.mixed.iter().flat_map(|m| m.reads_y()).copied().collect();
        ys.sort_unstable();
        ys.dedup();
        self.tf.mixed_y = ys;
        mask
    }

    /// The next sign change of time function `k` after `t` (the discrete
    /// values as the channels hold them now).
    fn search(&self, k: usize, t: f64) -> Found {
        let Some(Some(TimeFunction::Pure(g))) = self.info.time_functions.get(k) else {
            return Found::Nothing;
        };
        let (params, vars, model) = (&self.info.params[..], &self.vars[..], self.model);
        let cx = Cx {
            params,
            vars,
            model,
            breaks: &self.info.table_breaks,
            grids: &self.tf.grids,
            leaf: &|_| None,
        };
        let mut p = |s: f64| lsim_ir::eval::eval(g, &ChannelEnv { t: s, vars, params, model });
        let mut enc = |l: f64, r: f64| {
            let j = enclose(g, &cx, Iv { lo: l, hi: r });
            (j.v, j.d)
        };
        first_change(&mut p, &mut enc, t, self.tf.t_end, BUDGET)
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
