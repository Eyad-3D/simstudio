//! Conditions on explicit functions of time ([`crate::TimeFunction`]):
//! their sign changes found ahead, without integrating, and reached
//! exactly; the extrema of the time terms of conditions that read
//! continuous variables too, as stop times.
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
//! stays with root finding, which needs a sign change at a step's end: the
//! run loop stops at every extremum of its terms in time alone, so that
//! each is monotone within a step and a pulse they make spans a stop.
//!
//! What cannot be searched (a 2-D table, `atan2` or a derivative moving
//! with time; a function not defined where the search starts; a search
//! that makes no headway) is said in a warning and left to root finding.

use super::{Loop, same_instant};
use crate::ad::{self, Dual, DualEnv};
use crate::info::{ChannelEnv, TimeFunction, table_at};
use crate::interval::{Cx, Found, Iv, enclose, first_change, supported};
use crate::{Integrator, SolveError};
use lsim_ir::runtime::ModelFunctions;
use lsim_ir::{Expr, ParamId, VarId};

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

/// A mixed condition's term in time alone: the next extremum ahead.
struct Term<'a> {
    root: usize,
    e: &'a Expr,
    reads: Reads,
    /// the next extremum after now (NaN: none before the end, or not
    /// searched yet)
    next: f64,
    /// searched at least once
    done: bool,
    clears: u32,
    off: bool,
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
    terms: Vec<Term<'a>>,
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
            terms: vec![],
            t_end: f64::INFINITY,
        }
    }

    /// Whether root function `k` is searched ahead.
    pub(super) fn searched(&self, k: usize) -> bool {
        self.searched.get(k).copied().unwrap_or(false)
    }

    /// Whether there is anything to search.
    pub(super) fn any(&self) -> bool {
        self.searched.iter().any(|x| *x) || !self.terms.is_empty()
    }

    /// The next stop the time functions ask for after `t`: where a search
    /// ran out, a mixed term's extremum.
    pub(super) fn next_stop(&self, t: f64) -> f64 {
        let ahead = |x: f64| x > t && !same_instant(x, t);
        self.clear
            .iter()
            .copied()
            .chain(self.terms.iter().map(|x| x.next))
            .filter(|x| ahead(*x))
            .fold(f64::INFINITY, f64::min)
    }
}

/// A term of time at a time: its value and rate (the variables it reads
/// constant between events).
struct Along<'a> {
    t: f64,
    vars: &'a [f64],
    params: &'a [f64],
    model: &'a dyn ModelFunctions,
}

impl DualEnv for Along<'_> {
    fn var(&self, v: VarId) -> Result<Dual, ()> {
        Ok(Dual { v: self.vars.get(v.0 as usize).copied().unwrap_or(f64::NAN), d: 0.0 })
    }
    fn param(&self, p: ParamId) -> f64 {
        self.params[p.0 as usize]
    }
    fn time(&self) -> Result<Dual, ()> {
        Ok(Dual { v: self.t, d: 1.0 })
    }
    fn table(&self, k: u32, args: &[f64]) -> Option<(f64, [f64; 2])> {
        table_at(self.model, k, args)
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
    /// searches (returned: the root functions to mask), the mixed ones'
    /// terms; warns about the others.
    pub(super) fn time_functions_start(&mut self, t_end: f64) -> Vec<bool> {
        let info = self.info;
        let n = self.tf.searched.len();
        let mut mask = vec![false; n];
        self.tf.t_end = t_end;
        let unsupported = "a 2-D table, atan2 or a derivative in time";
        for (k, f) in info.time_functions.iter().enumerate().take(n) {
            match f {
                Some(TimeFunction::Pure(g)) => {
                    if supported(g, &info.table_breaks) {
                        mask[k] = true;
                        self.tf.searched[k] = true;
                        self.tf.stale[k] = true;
                        self.tf.reads[k] = Reads::of(g);
                    } else {
                        self.warn_unsearched(k, unsupported);
                    }
                }
                Some(TimeFunction::Mixed(terms)) => {
                    for e in terms {
                        if self.tf.terms.iter().any(|x| x.e == e) {
                            continue;
                        }
                        if supported(e, &info.table_breaks) {
                            self.tf.terms.push(Term {
                                root: k,
                                e,
                                reads: Reads::of(e),
                                next: f64::NAN,
                                done: false,
                                clears: 0,
                                off: false,
                            });
                        } else {
                            self.warn_unsearched(k, unsupported);
                        }
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
        let cx = Cx { params, vars, model, breaks: &self.info.table_breaks };
        let mut p = |s: f64| lsim_ir::eval::eval(g, &ChannelEnv { t: s, vars, params, model });
        let mut enc = |l: f64, r: f64| {
            let j = enclose(g, &cx, Iv { lo: l, hi: r });
            (j.v, j.d)
        };
        first_change(&mut p, &mut enc, t, self.tf.t_end, BUDGET)
    }

    /// The next extremum of mixed term `e` after `t` (where its rate
    /// changes sign).
    fn extremum(&self, e: &Expr, t: f64) -> Found {
        let (params, vars, model) = (&self.info.params[..], &self.vars[..], self.model);
        let cx = Cx { params, vars, model, breaks: &self.info.table_breaks };
        let mut p = |s: f64| {
            ad::dual(e, &Along { t: s, vars, params, model }).map(|r| r.d).unwrap_or(f64::NAN)
        };
        let mut enc = |l: f64, r: f64| {
            let j = enclose(e, &cx, Iv { lo: l, hi: r });
            (j.d, j.dd)
        };
        first_change(&mut p, &mut enc, t, self.tf.t_end, BUDGET)
    }

    /// Schedules the time functions' next sign changes, and the mixed
    /// terms' next extrema, after `t` (the channels just sampled): those
    /// not searched yet, those that fired or ran out of steps here or
    /// whose extremum is passed, and those that read a value that changed.
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
        for j in 0..self.tf.terms.len() {
            let x = &mut self.tf.terms[j];
            if x.off {
                continue;
            }
            let passed = x.next.is_finite() && (x.next < t || here(x.next));
            let changed = x.reads.changed(&self.vars);
            if x.done && !passed && !changed {
                continue;
            }
            x.done = true;
            let (e, root) = (x.e, x.root);
            let found = self.extremum(e, t);
            let x = &mut self.tf.terms[j];
            x.clears = if matches!(found, Found::Clear(_)) { x.clears + 1 } else { 0 };
            x.next = match found {
                Found::Change { at, .. } | Found::Clear(at) => at,
                Found::Nothing | Found::Undefined => f64::NAN,
            };
            if x.clears > CRAWL {
                x.off = true;
                x.next = f64::NAN;
                let why = format!("the search of its terms makes no headway at t = {t:.6} s");
                self.warn_unsearched(root, &why);
            } else if matches!(found, Found::Undefined) {
                self.warn_undefined(root, t);
            }
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
