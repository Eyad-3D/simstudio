//! Limits: where an inverse model passes a value through, and the flags.
//!
//! Preparation of an inverse model (`lsim_prep::prepare_inverse`) replaces
//! every `limit(x, lo, hi)` of the equations by `x` and lists it in
//! `PreparedModel::limits` (DESIGN.md §5.8): the limited value and its two
//! bounds as flat expressions, and the equation that holds the limit. Fast
//! mode so computes what it would *take* to follow the trace, and the
//! [`FlagTracker`] records each stretch of time a value was outside its
//! band as a [`LimitFlag`]. Fast mode only flags: it never changes the
//! answer.
//!
//! [`sites`] turns the prepared model's list into [`LimitSite`]s, each
//! expression in a form that is cheap to evaluate at every point the
//! stepper records (a variable, a parameter or a constant directly; any
//! other expression through the IR's interpreter, its tables read through
//! the compiled model's `eval_table`).

use crate::LimitFlag;
use lsim_ir::eval::{Env, eval};
use lsim_ir::expr::Expr;
use lsim_ir::flat::{FlatSystem, ParamId, VarId};
use lsim_ir::prepared::{PreparedModel, Slot};
use lsim_ir::runtime::ModelFunctions;

/// One of a limit's three values, ready to evaluate.
#[derive(Clone, Debug, PartialEq)]
pub enum Term {
    /// a number
    Const(f64),
    /// a variable: its index in the model's variables (`ModelFunctions::vars`)
    Var(usize),
    /// a parameter
    Param(usize),
    /// any other expression (flat scope), interpreted
    Expr(Expr),
}

impl Term {
    fn of(e: &Expr) -> Term {
        match e {
            Expr::Const(c) => Term::Const(*c),
            Expr::Var(v) => Term::Var(v.0 as usize),
            Expr::Param(p) => Term::Param(p.0 as usize),
            other => Term::Expr(other.clone()),
        }
    }

    fn reads_der(&self) -> bool {
        matches!(self, Term::Expr(e) if e.any(&mut |x| matches!(x, Expr::Der(_))))
    }
}

/// Where a variable's time derivative is, for a limit that reads one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DerSrc {
    /// `x'[i]` of state `i`
    State(usize),
    /// entry `i` of `y` (an iteration variable that is a derivative)
    Y(usize),
}

/// A limit of an inverse model.
#[derive(Clone, Debug, PartialEq)]
pub struct LimitSite {
    /// what it is, naming the part: `'E-Motor': the torque follows the demand within its limits`
    pub label: String,
    /// the part on the user's diagram (an instance path), for highlighting
    pub part: String,
    /// the limited value (the demand)
    pub value: Term,
    /// the lower bound
    pub lo: Term,
    /// the upper bound
    pub hi: Term,
    /// the limited value's unit
    pub unit: String,
}

/// Every limit of a prepared inverse model (none for a forward model,
/// whose limits clamp).
pub fn sites(m: &PreparedModel) -> Vec<LimitSite> {
    let flat = &m.flat;
    m.limits
        .iter()
        .map(|l| {
            let who = flat.instance_name(l.origin.instance);
            let what = l.origin.label.clone().unwrap_or_else(|| "a limit".into());
            LimitSite {
                label: format!("{who}: {what}"),
                part: flat.instance(flat.top_part(l.origin.instance)).path.clone(),
                value: Term::of(&l.value),
                lo: Term::of(&l.lo),
                hi: Term::of(&l.hi),
                unit: unit_of(flat, &l.value),
            }
        })
        .collect()
}

/// The unit text of a limited value: its variable's, when it is one.
fn unit_of(flat: &FlatSystem, e: &Expr) -> String {
    let mut unit = None;
    e.any(&mut |x| {
        if let Expr::Var(v) = x {
            unit = Some(flat.var(*v).unit_text.clone());
            true
        } else {
            false
        }
    });
    unit.unwrap_or_else(|| "1".into())
}

/// Where each variable's derivative is (indexed by variable), for limits
/// that read `der(v)`; empty when none does.
pub fn der_sources(m: &PreparedModel, sites: &[LimitSite]) -> Vec<Option<DerSrc>> {
    if !sites.iter().any(|s| [&s.value, &s.lo, &s.hi].iter().any(|t| t.reads_der())) {
        return vec![];
    }
    let mut out = vec![None; m.flat.vars.len()];
    for (i, v) in m.states.iter().enumerate() {
        out[v.0 as usize] = Some(DerSrc::State(i));
    }
    let nx = m.states.len();
    for (k, s) in m.algebraics.iter().enumerate() {
        if let Slot::Der(v) = s {
            out[v.0 as usize] = Some(DerSrc::Y(nx + k));
        }
    }
    out
}

/// One evaluated point of the model, as the limits read it.
pub struct Point<'a> {
    /// time, s
    pub t: f64,
    /// every variable (`ModelFunctions::vars`)
    pub vars: &'a [f64],
    /// the parameters
    pub p: &'a [f64],
    /// `y = [x; z]`
    pub y: &'a [f64],
    /// `x'`
    pub xdot: &'a [f64],
    /// where derivatives are ([`der_sources`])
    pub ders: &'a [Option<DerSrc>],
    /// the model, for its tables
    pub model: &'a dyn ModelFunctions,
}

impl Env for Point<'_> {
    fn time(&self) -> f64 {
        self.t
    }
    fn var(&self, v: VarId) -> f64 {
        self.vars.get(v.0 as usize).copied().unwrap_or(f64::NAN)
    }
    fn der(&self, v: VarId) -> f64 {
        match self.ders.get(v.0 as usize).copied().flatten() {
            Some(DerSrc::State(i)) => self.xdot.get(i).copied().unwrap_or(f64::NAN),
            Some(DerSrc::Y(i)) => self.y.get(i).copied().unwrap_or(f64::NAN),
            None => f64::NAN,
        }
    }
    fn param(&self, p: ParamId) -> f64 {
        self.p.get(p.0 as usize).copied().unwrap_or(f64::NAN)
    }
    fn table(&self, k: u32, args: &[f64]) -> f64 {
        let a = [args.first().copied().unwrap_or(0.0), args.get(1).copied().unwrap_or(0.0)];
        self.model.eval_table(k, a).map_or(f64::NAN, |(v, _)| v)
    }
}

impl Point<'_> {
    fn term(&self, t: &Term) -> f64 {
        match t {
            Term::Const(c) => *c,
            Term::Var(i) => self.vars.get(*i).copied().unwrap_or(f64::NAN),
            Term::Param(i) => self.p.get(*i).copied().unwrap_or(f64::NAN),
            Term::Expr(e) => eval(e, self),
        }
    }

    /// A site's value and bounds here.
    pub fn site(&self, s: &LimitSite) -> [f64; 3] {
        [self.term(&s.value), self.term(&s.lo), self.term(&s.hi)]
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Side {
    /// open stretch: its start and worst excess so far
    open: Option<(f64, f64)>,
    /// the excess at the previous point
    last: f64,
}

/// Turns a sequence of evaluated points into flags: each stretch of time a
/// limit's value is outside its band (beyond a relative tolerance), with
/// its start and end located by linear interpolation of the excess between
/// the points either side. A value that is not a number at a point is
/// itself flagged (it cannot be said to be inside its band).
pub struct FlagTracker<'a> {
    sites: &'a [LimitSite],
    rtol: f64,
    sides: Vec<[Side; 2]>,
    nan: Vec<Option<f64>>,
    last_t: f64,
    started: bool,
    flags: Vec<LimitFlag>,
}

fn crossing(t0: f64, e0: f64, t1: f64, e1: f64) -> f64 {
    if t1 <= t0 || !(e0.is_finite() && e1.is_finite()) || e1 == e0 {
        return t1;
    }
    (t0 + (t1 - t0) * (-e0) / (e1 - e0)).clamp(t0, t1)
}

impl<'a> FlagTracker<'a> {
    /// A tracker for `sites`; values outside a band by less than `rtol`
    /// of the band's (or the value's) magnitude do not count.
    pub fn new(sites: &'a [LimitSite], rtol: f64) -> Self {
        FlagTracker {
            sites,
            rtol,
            sides: vec![[Side::default(); 2]; sites.len()],
            nan: vec![None; sites.len()],
            last_t: f64::NEG_INFINITY,
            started: false,
            flags: vec![],
        }
    }

    /// Whether there is anything to track.
    pub fn is_empty(&self) -> bool {
        self.sites.is_empty()
    }

    /// One evaluated point: each site's `[value, lo, hi]`, in order.
    /// Points come in time order (two at the same time are the two sides
    /// of a jump).
    pub fn point(&mut self, t: f64, vals: &[[f64; 3]]) {
        for (s, site) in self.sites.iter().enumerate() {
            let [x, lo, hi] = vals[s];
            if x.is_nan() || lo.is_nan() || hi.is_nan() {
                self.nan[s].get_or_insert(t);
                continue;
            }
            if let Some(t0) = self.nan[s].take() {
                self.flags.push(nan_flag(site, t0, t));
            }
            let scale =
                [x, lo, hi].iter().filter(|v| v.is_finite()).fold(0.0f64, |m, v| m.max(v.abs()));
            let tol = self.rtol * scale;
            for (k, e) in [x - hi, lo - x].into_iter().enumerate() {
                let side = &mut self.sides[s][k];
                let outside = e > tol;
                match (outside, side.open) {
                    (true, None) => {
                        let t0 =
                            if self.started { crossing(self.last_t, side.last, t, e) } else { t };
                        side.open = Some((t0, e));
                    }
                    (true, Some((t0, w))) => side.open = Some((t0, w.max(e))),
                    (false, Some((t0, w))) => {
                        let t1 = crossing(self.last_t, side.last, t, e);
                        self.flags.push(flag(site, k == 0, t0, t1, w));
                        side.open = None;
                    }
                    (false, None) => {}
                }
                side.last = e;
            }
        }
        self.last_t = t;
        self.started = true;
    }

    /// The flags, every open one closed at `t_end`, in order of start.
    pub fn finish(mut self, t_end: f64) -> Vec<LimitFlag> {
        for (s, site) in self.sites.iter().enumerate() {
            for k in 0..2 {
                if let Some((t0, w)) = self.sides[s][k].open {
                    self.flags.push(flag(site, k == 0, t0, t_end, w));
                }
            }
            if let Some(t0) = self.nan[s] {
                self.flags.push(nan_flag(site, t0, t_end));
            }
        }
        self.flags.sort_by(|a, b| a.t_start.total_cmp(&b.t_start));
        self.flags
    }
}

fn flag(site: &LimitSite, upper: bool, t0: f64, t1: f64, worst: f64) -> LimitFlag {
    LimitFlag {
        t_start: t0,
        t_end: t1,
        label: format!(
            "{} — {}",
            site.label,
            if upper { "above its upper limit" } else { "below its lower limit" }
        ),
        part: site.part.clone(),
        upper,
        worst_excess: worst,
        unit: site.unit.clone(),
    }
}

fn nan_flag(site: &LimitSite, t0: f64, t1: f64) -> LimitFlag {
    LimitFlag {
        t_start: t0,
        t_end: t1,
        label: format!("{} — its value or a bound is not a number", site.label),
        part: site.part.clone(),
        upper: true,
        worst_excess: f64::NAN,
        unit: site.unit.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site() -> LimitSite {
        LimitSite {
            label: "'Motor': torque".into(),
            part: "motor".into(),
            value: Term::Var(0),
            lo: Term::Var(1),
            hi: Term::Var(2),
            unit: "N.m".into(),
        }
    }

    #[test]
    fn tracks_stretches_outside_the_band() {
        let sites = [site()];
        let mut tr = FlagTracker::new(&sites, 1e-9);
        // x = t - 1 on [0, 4], band [-0.5, 2]: above from t = 3, below until t = 0.5
        for k in 0..=8 {
            let t = k as f64 * 0.5;
            tr.point(t, &[[t - 1.0, -0.5, 2.0]]);
        }
        let flags = tr.finish(4.0);
        assert_eq!(flags.len(), 2, "{flags:?}");
        assert!(!flags[0].upper && flags[0].t_start == 0.0);
        assert!((flags[0].t_end - 0.5).abs() < 1e-12, "{flags:?}");
        assert!(flags[1].upper && (flags[1].t_start - 3.0).abs() < 1e-12);
        assert_eq!(flags[1].t_end, 4.0);
        assert!((flags[1].worst_excess - 1.0).abs() < 1e-12);
        assert!(flags[1].label.ends_with("above its upper limit"));
    }

    #[test]
    fn a_value_that_is_not_a_number_is_flagged() {
        let sites = [site()];
        let mut tr = FlagTracker::new(&sites, 1e-9);
        tr.point(0.0, &[[0.0, -1.0, 1.0]]);
        tr.point(1.0, &[[f64::NAN, -1.0, 1.0]]);
        tr.point(2.0, &[[0.0, -1.0, 1.0]]);
        let flags = tr.finish(3.0);
        assert_eq!(flags.len(), 1, "{flags:?}");
        assert_eq!((flags[0].t_start, flags[0].t_end), (1.0, 2.0));
        assert!(flags[0].label.contains("not a number"));
    }
}
