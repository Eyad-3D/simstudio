//! Enclosures of flat-scope functions of time over time intervals, and the
//! search for their next sign change (DESIGN.md §8.2, *Conditions on
//! explicit functions of time*).
//!
//! Root finding sees a condition only by its sign at the ends of the
//! integrator's steps: a pulse narrower than a step (`sin(2π time / T) >
//! 0.95` while nothing integrated moves and the steps grow long) is stepped
//! over. A function of time alone between events (with parameters and
//! discrete values) needs no integration to find where it changes sign:
//! [`J2`] encloses the function, its rate and its second rate over a time
//! interval, rigorously (interval arithmetic rounded outwards, a table by
//! its monotone cubic pieces), and [`first_change`] walks forward in time.
//! It skips an interval whose enclosure keeps the sign, or where the
//! function is monotone with the same sign at both ends; otherwise it
//! advances by what the bound on the rate allows (a function of value g
//! and rate at most L cannot reach zero within |g| / L), shrinking the
//! interval as it nears a zero, and bisects to adjacent floats once a sign
//! change is bracketed. A grazing touch without a sign change is passed.

use crate::info::table_at;
use lsim_ir::expr::{BinaryOp, Builtin, CmpOp, Expr};
use lsim_ir::runtime::ModelFunctions;
use std::f64::consts::{FRAC_PI_2, PI, TAU};

/// A closed interval `[lo, hi]` (bounds may be infinite).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Iv {
    pub lo: f64,
    pub hi: f64,
}

pub(crate) const ALL: Iv = Iv { lo: f64::NEG_INFINITY, hi: f64::INFINITY };
const ZERO: Iv = Iv { lo: 0.0, hi: 0.0 };
const ONE: Iv = Iv { lo: 1.0, hi: 1.0 };
const UNIT: Iv = Iv { lo: 0.0, hi: 1.0 };

fn down(mut x: f64, n: u32) -> f64 {
    for _ in 0..n {
        x = x.next_down();
    }
    x
}

fn up(mut x: f64, n: u32) -> f64 {
    for _ in 0..n {
        x = x.next_up();
    }
    x
}

impl Iv {
    fn new(lo: f64, hi: f64) -> Iv {
        if lo.is_nan() || hi.is_nan() || lo > hi { ALL } else { Iv { lo, hi } }
    }

    pub(crate) fn point(v: f64) -> Iv {
        Iv::new(v, v)
    }

    /// `[lo, hi]` widened by `n` ulps on each side (round-off)
    fn wide(lo: f64, hi: f64, n: u32) -> Iv {
        Iv::new(down(lo, n), up(hi, n))
    }

    fn hull(self, o: Iv) -> Iv {
        Iv::new(self.lo.min(o.lo), self.hi.max(o.hi))
    }

    fn has_zero(self) -> bool {
        self.lo <= 0.0 && self.hi >= 0.0
    }

    fn is_point(self) -> bool {
        self.lo == self.hi
    }

    /// The largest magnitude in it.
    pub(crate) fn mag(self) -> f64 {
        self.lo.abs().max(self.hi.abs())
    }

    fn add(self, o: Iv) -> Iv {
        if o == ZERO {
            return self;
        }
        if self == ZERO {
            return o;
        }
        if self.is_point() && o.is_point() {
            // exact when the rounding error is zero (TwoSum): constants
            // stay points, a difference of equal values exactly zero
            let (a, b) = (self.lo, o.lo);
            let s = a + b;
            let bb = s - a;
            if s.is_finite() && (a - (s - bb)) + (b - bb) == 0.0 {
                return Iv::point(s);
            }
        }
        Iv::wide(self.lo + o.lo, self.hi + o.hi, 1)
    }

    fn sub(self, o: Iv) -> Iv {
        self.add(o.neg())
    }

    fn neg(self) -> Iv {
        Iv { lo: -self.hi, hi: -self.lo }
    }

    fn mul(self, o: Iv) -> Iv {
        // (0 · ∞ is 0 here: an unbounded factor times zero is zero)
        if self == ZERO || o == ZERO {
            return ZERO;
        }
        if self.is_point() && o.is_point() {
            // exact when the rounding error is zero (an FMA's residual)
            let (a, b) = (self.lo, o.lo);
            let p = a * b;
            if p.is_finite() && a.mul_add(b, -p) == 0.0 {
                return Iv::point(p);
            }
        }
        let p = |a: f64, b: f64| if a == 0.0 || b == 0.0 { 0.0 } else { a * b };
        let c = [p(self.lo, o.lo), p(self.lo, o.hi), p(self.hi, o.lo), p(self.hi, o.hi)];
        let lo = c.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = c.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        Iv::wide(lo, hi, 1)
    }

    fn scale(self, k: f64) -> Iv {
        self.mul(Iv::point(k))
    }

    fn recip(self) -> Iv {
        if self.has_zero() { ALL } else { Iv::wide(1.0 / self.hi, 1.0 / self.lo, 1) }
    }

    fn sqr(self) -> Iv {
        if self == ZERO {
            return ZERO;
        }
        let (a, b) = (self.lo * self.lo, self.hi * self.hi);
        if self.lo >= 0.0 {
            Iv::wide(a, b, 1)
        } else if self.hi <= 0.0 {
            Iv::wide(b, a, 1)
        } else {
            Iv::new(0.0, up(a.max(b), 1))
        }
    }

    /// An increasing function's enclosure (`f` accurate to an ulp).
    fn incr(self, f: fn(f64) -> f64) -> Iv {
        Iv::wide(f(self.lo), f(self.hi), 2)
    }

    /// A decreasing function's enclosure.
    fn decr(self, f: fn(f64) -> f64) -> Iv {
        Iv::wide(f(self.hi), f(self.lo), 2)
    }
}

/// Whether `[lo, hi]` holds a point `phase + k · period`; generous by the
/// round-off of reducing the bounds (it may say yes for a point a few
/// ulps outside).
fn holds_phase(a: Iv, phase: f64, period: f64) -> bool {
    let slack = 8.0 * f64::EPSILON * a.mag().max(1.0);
    let (lo, hi) = (a.lo - slack, a.hi + slack);
    let k = ((lo - phase) / period).ceil();
    phase + k * period <= hi
}

fn sin_iv(a: Iv) -> Iv {
    if !(a.lo.is_finite() && a.hi.is_finite()) || a.hi - a.lo >= TAU {
        return Iv::new(-1.0, 1.0);
    }
    let (s0, s1) = (a.lo.sin(), a.hi.sin());
    let mut r = Iv::wide(s0.min(s1), s0.max(s1), 2);
    if holds_phase(a, FRAC_PI_2, TAU) {
        r.hi = 1.0;
    }
    if holds_phase(a, -FRAC_PI_2, TAU) {
        r.lo = -1.0;
    }
    Iv::new(r.lo.max(-1.0), r.hi.min(1.0))
}

fn cos_iv(a: Iv) -> Iv {
    if !(a.lo.is_finite() && a.hi.is_finite()) || a.hi - a.lo >= TAU {
        return Iv::new(-1.0, 1.0);
    }
    let (c0, c1) = (a.lo.cos(), a.hi.cos());
    let mut r = Iv::wide(c0.min(c1), c0.max(c1), 2);
    if holds_phase(a, 0.0, TAU) {
        r.hi = 1.0;
    }
    if holds_phase(a, PI, TAU) {
        r.lo = -1.0;
    }
    Iv::new(r.lo.max(-1.0), r.hi.min(1.0))
}

/// `x^e` for `x ≥ 0` (`0^e` with e < 0 is infinite).
fn powf_iv(a: Iv, e: f64) -> Iv {
    if a.lo < 0.0 {
        return ALL;
    }
    if e == 0.0 {
        return ONE;
    }
    let (p0, p1) = (a.lo.powf(e), a.hi.powf(e));
    if e > 0.0 { Iv::wide(p0, p1, 2) } else { Iv::wide(p1, p0, 2) }
}

/// `x^n` for an integer `n`.
fn powi_iv(a: Iv, n: i32) -> Iv {
    if n == 0 {
        return ONE;
    }
    if n < 0 {
        return powi_iv(a, -n).recip();
    }
    let (p0, p1) = (a.lo.powi(n), a.hi.powi(n));
    if n % 2 == 1 || a.lo >= 0.0 {
        Iv::wide(p0, p1, 2)
    } else if a.hi <= 0.0 {
        Iv::wide(p1, p0, 2)
    } else {
        Iv::new(0.0, up(p0.max(p1), 2))
    }
}

/// A function of time over a time interval: enclosures of its value, its
/// rate and its second rate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct J2 {
    pub v: Iv,
    pub d: Iv,
    pub dd: Iv,
}

impl J2 {
    fn konst(v: Iv) -> J2 {
        J2 { v, d: ZERO, dd: ZERO }
    }

    fn all() -> J2 {
        J2 { v: ALL, d: ALL, dd: ALL }
    }

    /// A value whose rates are unknown (a jump inside the interval).
    fn jumps(v: Iv) -> J2 {
        J2 { v, d: ALL, dd: ALL }
    }

    fn is_const(&self) -> bool {
        self.d == ZERO && self.dd == ZERO
    }

    /// f(self), given f's enclosures over `self.v`: f, f', f''.
    fn chain(self, f: Iv, f1: Iv, f2: Iv) -> J2 {
        if self.is_const() {
            return J2::konst(f);
        }
        J2 { v: f, d: f1.mul(self.d), dd: f2.mul(self.d.sqr()).add(f1.mul(self.dd)) }
    }

    fn add(self, o: J2) -> J2 {
        J2 { v: self.v.add(o.v), d: self.d.add(o.d), dd: self.dd.add(o.dd) }
    }

    fn neg(self) -> J2 {
        J2 { v: self.v.neg(), d: self.d.neg(), dd: self.dd.neg() }
    }

    fn sub(self, o: J2) -> J2 {
        self.add(o.neg())
    }

    fn mul(self, o: J2) -> J2 {
        J2 {
            v: self.v.mul(o.v),
            d: self.d.mul(o.v).add(self.v.mul(o.d)),
            dd: self.dd.mul(o.v).add(self.d.mul(o.d).scale(2.0)).add(self.v.mul(o.dd)),
        }
    }

    fn recip(self) -> J2 {
        let v = self.v;
        let r = v.recip();
        self.chain(r, v.sqr().recip().neg(), v.sqr().mul(v).recip().scale(2.0))
    }

    fn ln(self) -> J2 {
        let v = self.v;
        if v.lo < 0.0 {
            return J2::all();
        }
        let f =
            if v.lo == 0.0 { Iv::wide(f64::NEG_INFINITY, v.hi.ln(), 2) } else { v.incr(f64::ln) };
        self.chain(f, v.recip(), v.sqr().recip().neg())
    }

    fn exp(self) -> J2 {
        let e = self.v.incr(f64::exp);
        self.chain(e, e, e)
    }

    fn pow(self, b: J2) -> J2 {
        let a = self;
        if b.is_const() && b.v.is_point() {
            let c = b.v.lo;
            if c == 0.0 {
                return J2::konst(ONE);
            }
            if c == c.trunc() && c.abs() <= 64.0 {
                let n = c as i32;
                return a.chain(
                    powi_iv(a.v, n),
                    powi_iv(a.v, n - 1).scale(c),
                    powi_iv(a.v, n - 2).scale(c * (c - 1.0)),
                );
            }
            if a.v.lo < 0.0 {
                return J2::all();
            }
            return a.chain(
                powf_iv(a.v, c),
                powf_iv(a.v, c - 1.0).scale(c),
                powf_iv(a.v, c - 2.0).scale(c * (c - 1.0)),
            );
        }
        // x^y = exp(y ln x)
        if a.v.lo <= 0.0 {
            return J2::all();
        }
        b.mul(a.ln()).exp()
    }
}

/// Where a function of time gets its values: the parameters, the
/// variables it reads (discrete values, constant between events: their
/// channels), the model's tables and the breakpoints of the 1-D ones.
pub(crate) struct Cx<'a> {
    pub params: &'a [f64],
    pub vars: &'a [f64],
    pub model: &'a dyn ModelFunctions,
    /// per table: the first axis's breakpoints of a 1-D table (empty: not
    /// known, or 2-D)
    pub breaks: &'a [Vec<f64>],
}

/// Whether [`enclose`] can bound `e` usefully as a function of time: no
/// derivative, unresolved name, 2-D table or `atan2` whose arguments move
/// with time, and every 1-D table's breakpoints known.
pub(crate) fn supported(e: &Expr, breaks: &[Vec<f64>]) -> bool {
    let reads_time = |e: &Expr| e.any(&mut |x| matches!(x, Expr::Time));
    let mut ok = true;
    e.walk(&mut |x| match x {
        Expr::Der(_) | Expr::Name(_) => ok = false,
        Expr::Call(Builtin::Der | Builtin::Pre, _) => ok = false,
        Expr::Call(Builtin::Atan2, args) if args.iter().any(reads_time) => ok = false,
        Expr::Table { table, args } if args.iter().any(reads_time) => {
            if args.len() != 1 || breaks.get(*table as usize).is_none_or(|b| b.is_empty()) {
                ok = false;
            }
        }
        _ => {}
    });
    ok
}

/// `e` over the times `t` (with its first and second rates).
pub(crate) fn enclose(e: &Expr, cx: &Cx<'_>, t: Iv) -> J2 {
    let ev = |x: &Expr| enclose(x, cx, t);
    let k = |v: f64| J2::konst(Iv::point(v));
    match e {
        Expr::Const(v) => k(*v),
        Expr::Param(p) => k(cx.params[p.0 as usize]),
        Expr::Var(v) | Expr::Pre(v) => k(cx.vars.get(v.0 as usize).copied().unwrap_or(f64::NAN)),
        Expr::Time => J2 { v: t, d: ONE, dd: ZERO },
        Expr::Der(_) | Expr::Name(_) => J2::all(),
        Expr::Neg(a) => ev(a).neg(),
        Expr::NoEvent(a) => ev(a),
        Expr::Binary(op, a, b) => {
            let (a, b) = (ev(a), ev(b));
            match op {
                BinaryOp::Add => a.add(b),
                BinaryOp::Sub => a.sub(b),
                BinaryOp::Mul => a.mul(b),
                BinaryOp::Div => a.mul(b.recip()),
                BinaryOp::Pow => a.pow(b),
            }
        }
        Expr::Compare(op, a, b) => {
            let (a, b) = (ev(a).v, ev(b).v);
            let (yes, no) = match op {
                CmpOp::Lt => (a.hi < b.lo, a.lo >= b.hi),
                CmpOp::Le => (a.hi <= b.lo, a.lo > b.hi),
                CmpOp::Gt => (a.lo > b.hi, a.hi <= b.lo),
                CmpOp::Ge => (a.lo >= b.hi, a.hi < b.lo),
            };
            truth(yes, no)
        }
        Expr::And(a, b) => {
            let (a, b) = (ev(a).v, ev(b).v);
            truth(!a.has_zero() && !b.has_zero(), a == ZERO || b == ZERO)
        }
        Expr::Or(a, b) => {
            let (a, b) = (ev(a).v, ev(b).v);
            truth(!a.has_zero() || !b.has_zero(), a == ZERO && b == ZERO)
        }
        Expr::Not(a) => {
            let a = ev(a).v;
            truth(a == ZERO, !a.has_zero())
        }
        Expr::If(c, a, b) => {
            let c = ev(c).v;
            if !c.has_zero() {
                ev(a)
            } else if c == ZERO {
                ev(b)
            } else {
                J2::jumps(ev(a).v.hull(ev(b).v))
            }
        }
        Expr::Table { table, args } => {
            let at: Vec<J2> = args.iter().map(ev).collect();
            table_j2(cx, *table, &at)
        }
        Expr::Call(f, args) => {
            let arg = |i: usize| args.get(i).map(ev).unwrap_or_else(J2::all);
            let a = arg(0);
            let x = a.v;
            match f {
                Builtin::Der | Builtin::Pre => J2::all(),
                Builtin::Sin => a.chain(sin_iv(x), cos_iv(x), sin_iv(x).neg()),
                Builtin::Cos => a.chain(cos_iv(x), sin_iv(x).neg(), cos_iv(x).neg()),
                Builtin::Tan => {
                    if holds_phase(x, FRAC_PI_2, PI) || !(x.lo.is_finite() && x.hi.is_finite()) {
                        return J2::all();
                    }
                    let t = x.incr(f64::tan);
                    let s = ONE.add(t.sqr());
                    a.chain(t, s, t.mul(s).scale(2.0))
                }
                Builtin::Asin | Builtin::Acos => {
                    if x.lo < -1.0 || x.hi > 1.0 {
                        return J2::all();
                    }
                    let r = ONE.sub(x.sqr());
                    let rs = sqrt_iv(r);
                    let f1 = rs.recip();
                    let f2 = x.mul(r.mul(rs).recip());
                    if *f == Builtin::Asin {
                        a.chain(x.incr(f64::asin), f1, f2)
                    } else {
                        a.chain(x.decr(f64::acos), f1.neg(), f2.neg())
                    }
                }
                Builtin::Atan => {
                    let r = ONE.add(x.sqr());
                    a.chain(x.incr(f64::atan), r.recip(), x.scale(-2.0).mul(r.sqr().recip()))
                }
                Builtin::Atan2 => {
                    let b = arg(1);
                    if a.is_const() && b.is_const() && x.is_point() && b.v.is_point() {
                        return k(x.lo.atan2(b.v.lo));
                    }
                    J2::all()
                }
                Builtin::Sinh => {
                    let c = cosh_iv(x);
                    a.chain(x.incr(f64::sinh), c, x.incr(f64::sinh))
                }
                Builtin::Cosh => a.chain(cosh_iv(x), x.incr(f64::sinh), cosh_iv(x)),
                Builtin::Tanh => {
                    let t = x.incr(f64::tanh);
                    let s = ONE.sub(t.sqr());
                    a.chain(t, s, t.mul(s).scale(-2.0))
                }
                Builtin::Exp => a.exp(),
                Builtin::Log => a.ln(),
                Builtin::Sqrt => {
                    if x.lo < 0.0 {
                        return J2::all();
                    }
                    let s = sqrt_iv(x);
                    a.chain(s, s.recip().scale(0.5), x.mul(s).recip().scale(-0.25))
                }
                Builtin::Abs => {
                    if x.lo >= 0.0 {
                        a
                    } else if x.hi <= 0.0 {
                        a.neg()
                    } else {
                        J2 { v: Iv::new(0.0, x.mag()), d: a.d.hull(a.d.neg()), dd: ALL }
                    }
                }
                Builtin::Sign => {
                    if x.lo > 0.0 {
                        k(1.0)
                    } else if x.hi < 0.0 {
                        k(-1.0)
                    } else if x == ZERO {
                        k(0.0)
                    } else {
                        J2::jumps(Iv::new(-1.0, 1.0))
                    }
                }
                Builtin::Min => min_j2(a, arg(1)),
                Builtin::Max => max_j2(a, arg(1)),
                Builtin::Limit => min_j2(max_j2(a, arg(1)), arg(2)),
            }
        }
    }
}

fn sqrt_iv(x: Iv) -> Iv {
    if x.lo < 0.0 { ALL } else { x.incr(f64::sqrt) }
}

fn cosh_iv(x: Iv) -> Iv {
    if x.lo >= 0.0 {
        x.incr(f64::cosh)
    } else if x.hi <= 0.0 {
        x.decr(f64::cosh)
    } else {
        Iv::new(1.0, up(x.lo.cosh().max(x.hi.cosh()), 2))
    }
}

/// A truth value: 1 or 0 when decided, else [0, 1] (it jumps inside).
fn truth(yes: bool, no: bool) -> J2 {
    if yes {
        J2::konst(ONE)
    } else if no {
        J2::konst(ZERO)
    } else {
        J2::jumps(UNIT)
    }
}

/// `min` as the evaluators take it (the first on a tie).
fn min_j2(a: J2, b: J2) -> J2 {
    if a.v.hi <= b.v.lo {
        a
    } else if b.v.hi < a.v.lo {
        b
    } else {
        // a kink inside: the rate is one of the two
        J2 { v: Iv::new(a.v.lo.min(b.v.lo), a.v.hi.min(b.v.hi)), d: a.d.hull(b.d), dd: ALL }
    }
}

fn max_j2(a: J2, b: J2) -> J2 {
    if a.v.lo >= b.v.hi {
        a
    } else if b.v.lo > a.v.hi {
        b
    } else {
        J2 { v: Iv::new(a.v.lo.max(b.v.lo), a.v.hi.max(b.v.hi)), d: a.d.hull(b.d), dd: ALL }
    }
}

/// A table at `at`: constant arguments give its value; a 1-D table whose
/// argument moves is enclosed piece by piece.
fn table_j2(cx: &Cx<'_>, k: u32, at: &[J2]) -> J2 {
    if at.iter().all(|a| a.is_const() && a.v.is_point()) {
        let args: Vec<f64> = at.iter().map(|a| a.v.lo).collect();
        return match table_at(cx.model, k, &args) {
            Some((v, _)) => J2::konst(Iv::point(v)),
            None => J2::all(),
        };
    }
    let xs = match cx.breaks.get(k as usize) {
        Some(b) if !b.is_empty() && at.len() == 1 => b,
        _ => return J2::all(),
    };
    let u = at[0];
    let (f, f1, f2) = table_enclosure(cx.model, k, xs, u.v);
    u.chain(f, f1, f2)
}

/// A 1-D table's value, first and second derivatives over `u`: on each
/// piece between breakpoints `x_i`, `x_i+1` the interpolant is a cubic
/// whose derivative is the quadratic `q` with `q(x_i)`, `q(x_i+1)` the
/// slopes there and mean the secant; outside the data it continues as the
/// table says (a constant or a line).
fn table_enclosure(model: &dyn ModelFunctions, k: u32, xs: &[f64], u: Iv) -> (Iv, Iv, Iv) {
    let at = |x: f64| table_at(model, k, &[x]).unwrap_or((f64::NAN, [f64::NAN; 2]));
    if !(u.lo.is_finite() && u.hi.is_finite()) {
        return (ALL, ALL, ALL);
    }
    let n = xs.len();
    let (first, last) = (xs[0], xs[n - 1]);
    let (v0, d0) = at(u.lo);
    let (v1, d1) = at(u.hi);
    // constant over u (flat pieces, a clamp outside): exactly
    let mut flat = v0 == v1 && d0[0] == 0.0 && d1[0] == 0.0;
    let mut val = Iv::new(v0.min(v1), v0.max(v1));
    let mut der = Iv::new(d0[0].min(d1[0]), d0[0].max(d1[0]));
    // the round-off of the quadratics' coefficients (the secant of values
    // that may be large beside their difference)
    let (mut err_q, mut err_dq) = (0.0f64, 0.0f64);
    let mut sec = Iv::new(f64::INFINITY, f64::NEG_INFINITY);
    let mut sec_any = false;
    let add_sec = |s: &mut Iv, x: f64| {
        *s = Iv { lo: s.lo.min(x), hi: s.hi.max(x) };
    };
    // outside the data: a constant or a line (its derivative constant);
    // across the data's end the derivative may jump: no second derivative
    if u.lo < first || u.hi > last {
        if u.lo < first && u.hi > first || u.lo < last && u.hi > last {
            sec = ALL;
            sec_any = true;
        } else {
            add_sec(&mut sec, 0.0);
            sec_any = true;
        }
    }
    // the pieces it overlaps
    let i0 = xs.partition_point(|x| *x <= u.lo).saturating_sub(1);
    let mut i = i0;
    while i + 1 < n && xs[i] < u.hi {
        let (xa, xb) = (xs[i], xs[i + 1]);
        let (s0, s1) = (u.lo.max(xa), u.hi.min(xb));
        if s0 < s1 || (s0 == s1 && u.lo == u.hi) {
            let (ya, ga) = at(xa);
            let (yb, gb) = at(xb);
            let h = xb - xa;
            let (a, b) = (ga[0], gb[0]);
            flat &= ya == v0 && yb == v0 && a == 0.0 && b == 0.0;
            let m = (yb - ya) / h;
            let alpha = 6.0 * (m - 0.5 * (a + b)) / (h * h);
            let q = |s: f64| a + (b - a) * s / h + alpha * s * (h - s);
            let dq = |s: f64| (b - a) / h + alpha * (h - 2.0 * s);
            let (sa, sb) = (s0 - xa, s1 - xa);
            let (qa, qb) = (q(sa), q(sb));
            der = der.hull(Iv::new(qa.min(qb), qa.max(qb)));
            if alpha != 0.0 {
                let vertex = 0.5 * h + (b - a) / (2.0 * alpha * h);
                if vertex > sa && vertex < sb {
                    der = der.hull(Iv::point(q(vertex)));
                }
            }
            let em = 8.0 * f64::EPSILON * (ya.abs().max(yb.abs()) / h + m.abs());
            let eq = 16.0 * (em + 8.0 * f64::EPSILON * (a.abs() + b.abs()));
            err_q = err_q.max(eq);
            err_dq = err_dq.max(6.0 * eq / h);
            let (da, db) = (dq(sa), dq(sb));
            add_sec(&mut sec, da);
            add_sec(&mut sec, db);
            sec_any = true;
            // the value: at the breakpoints inside, and where q is zero
            // inside (an extremum of the cubic)
            for x in [xa, xb] {
                if x > u.lo && x < u.hi {
                    let (v, _) = at(x);
                    val = val.hull(Iv::point(v));
                }
            }
            // q(s) = a + c1 s - alpha s², c1 = (b - a) / h + alpha h
            let c1 = (b - a) / h + alpha * h;
            let roots: Vec<f64> = if alpha == 0.0 {
                if c1 != 0.0 { vec![-a / c1] } else { vec![] }
            } else {
                let disc = c1 * c1 + 4.0 * alpha * a;
                if disc < 0.0 {
                    vec![]
                } else {
                    let r = disc.sqrt();
                    vec![(c1 - r) / (2.0 * alpha), (c1 + r) / (2.0 * alpha)]
                }
            };
            for s in roots {
                if s > sa && s < sb {
                    let (v, _) = at(xa + s);
                    val = val.hull(Iv::point(v));
                }
            }
        }
        i += 1;
    }
    if flat {
        return (Iv::point(v0), ZERO, ZERO);
    }
    if !sec_any {
        sec = ALL;
    }
    // round-off: the tabulated values and slopes to a few ulps, the
    // secant and the quadratic's coefficients to the size of the values
    // over the piece
    let vm = val.mag().max(f64::MIN_POSITIVE);
    let val = Iv::new(val.lo - 16.0 * f64::EPSILON * vm, val.hi + 16.0 * f64::EPSILON * vm);
    let dm = 1e-12 * der.mag() + err_q;
    let der = Iv::new(der.lo - dm, der.hi + dm);
    let sm = 1e-12 * sec.mag() + err_dq;
    let sec = if sm.is_finite() { Iv::new(sec.lo - sm, sec.hi + sm) } else { ALL };
    (val, der, sec)
}

/// What [`first_change`] found after the start.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Found {
    /// the first time the sign differs from the start's (the next float
    /// before it has the start's sign): rising when it goes up
    Change { at: f64, rising: bool },
    /// no change up to this time; the search ran out of steps there
    Clear(f64),
    /// no change before the end
    Nothing,
    /// the function is not defined (NaN) at the start
    Undefined,
}

fn sign(x: f64) -> i8 {
    if x > 0.0 {
        1
    } else if x < 0.0 {
        -1
    } else {
        0
    }
}

/// The first time after the instant `a` (not within a few ulps of it,
/// [`crate::run::same_instant`]) and up to `b` at which `p` has another
/// sign than just after `a` and keeps it (an exact zero that `p` leaves
/// with the sign it had is a touch, passed). `enc(l, r)` encloses p and
/// its rate over `[l, r]`. At most `budget` steps.
pub(crate) fn first_change(
    p: &mut dyn FnMut(f64) -> f64,
    enc: &mut dyn FnMut(f64, f64) -> (Iv, Iv),
    a: f64,
    b: f64,
    budget: usize,
) -> Found {
    // the first time that is not this instant (near zero, on the scale
    // of the horizon: no stop a few hundred ulps of the end time after 0)
    let scale = b.abs();
    let after = |a: f64| {
        let mut t = a + 17.0 * f64::EPSILON * a.abs().max(scale);
        while crate::run::same_instant(a, t) || t <= a {
            t = t.next_up();
        }
        t
    };
    let mut t = after(a);
    if t > b {
        return Found::Nothing;
    }
    let pt = p(t);
    if pt.is_nan() {
        return Found::Undefined;
    }
    let s0 = sign(pt);
    // p(t) when known
    let mut g = Some(pt);
    // the window ahead of t
    let mut w = b - t;
    let mut steps = 0;
    loop {
        if t >= b {
            return Found::Nothing;
        }
        steps += 1;
        if steps > budget {
            return Found::Clear(t);
        }
        // a few ulps: the least window (the integrators' instant)
        let least = 32.0 * f64::EPSILON * t.abs().max(scale).max(f64::MIN_POSITIVE);
        w = w.max(least);
        let minimal = w <= least;
        let r = (t + w).min(b);
        let (v, d) = enc(t, r);
        let keeps = match s0 {
            1 => v.lo > 0.0,
            -1 => v.hi < 0.0,
            _ => v == ZERO,
        };
        if keeps {
            t = r;
            g = None;
            w *= 2.0;
            continue;
        }
        // a sign change bracketed in (lo, hi]: down to adjacent floats
        let bracket = |p: &mut dyn FnMut(f64) -> f64, mut lo: f64, mut hi: f64| {
            loop {
                let m = lo + 0.5 * (hi - lo);
                if m <= lo || m >= hi {
                    break;
                }
                if sign(p(m)) == s0 {
                    lo = m;
                } else {
                    hi = m;
                }
            }
            let ph = p(hi);
            // an exact zero: the sign after it decides
            let s1 = if ph == 0.0 { sign(p(after(hi))) } else { sign(ph) };
            if ph == 0.0 && s1 == s0 && s0 != 0 { Err(after(hi)) } else { Ok((hi, s1 > s0)) }
        };
        let pr = p(r);
        let monotone = s0 != 0 && (d.lo > 0.0 || d.hi < 0.0);
        let found = if monotone && sign(pr) == s0 {
            t = r;
            g = Some(pr);
            w *= 2.0;
            continue;
        } else if monotone || (minimal && sign(pr) != s0) {
            Some(bracket(p, t, r))
        } else if minimal {
            // at round-off: none inside
            t = r;
            g = Some(pr);
            continue;
        } else {
            // a safe advance: |p| cannot fall to zero within |p(t)| / L
            let gt = *g.get_or_insert_with(|| p(t));
            let l = d.mag();
            let step = if s0 != 0 && l.is_finite() { gt.abs() / l } else { 0.0 };
            if step >= r - t {
                t = r;
                g = Some(pr);
                w *= 2.0;
                continue;
            }
            let span = r - t;
            let mut out = None;
            let advance = step > least;
            if advance {
                let t1 = t + step * (1.0 - 1e-12);
                let p1 = p(t1);
                if sign(p1) != s0 {
                    out = Some(bracket(p, t, t1));
                } else {
                    t = t1;
                    g = Some(p1);
                }
            }
            // a bound loose over the window (the advance small beside it):
            // a narrower one
            w = if !advance || step < span / 8.0 { 0.5 * span } else { (r - t).max(least) };
            out
        };
        match found {
            Some(Ok((at, rising))) => return Found::Change { at, rising },
            Some(Err(on)) => {
                // a touch: on from just after it
                t = on;
                g = None;
                w = least;
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsim_ir::expr::Builtin;
    use lsim_ir::runtime::{EvalInput, Layout};

    struct NoModel(Layout);
    impl ModelFunctions for NoModel {
        fn layout(&self) -> &Layout {
            &self.0
        }
        fn residual(&self, _: &EvalInput<'_>, _: &mut [f64], _: &mut [f64]) {}
        fn jvp(&self, _: &EvalInput<'_>, _: &[f64], _: &mut [f64], _: &mut [f64]) {}
        fn roots(&self, _: &EvalInput<'_>, _: &mut [f64], _: &mut [f64]) {}
        fn vars(&self, _: &EvalInput<'_>, _: &mut [f64], _: &mut [f64]) {}
        fn when(&self, _: &EvalInput<'_>, _: &[f64], _: &mut [f64], _: &mut [f64]) {}
        fn start(&self, _: &[f64], _: &mut [f64], _: &mut [f64]) {}
    }

    fn no_model() -> NoModel {
        NoModel(Layout {
            n_x: 0,
            n_z: 0,
            n_p: 0,
            n_d: 0,
            n_u: 0,
            n_roots: 0,
            n_whens: 0,
            n_vars: 0,
            n_work: 0,
        })
    }

    fn bin(op: BinaryOp, a: Expr, b: Expr) -> Expr {
        Expr::Binary(op, Box::new(a), Box::new(b))
    }

    fn call(f: Builtin, a: Vec<Expr>) -> Expr {
        Expr::Call(f, a)
    }

    fn point(e: &Expr, t: f64) -> f64 {
        struct At(f64);
        impl lsim_ir::eval::Env for At {
            fn time(&self) -> f64 {
                self.0
            }
            fn var(&self, _: lsim_ir::VarId) -> f64 {
                f64::NAN
            }
            fn der(&self, _: lsim_ir::VarId) -> f64 {
                f64::NAN
            }
            fn param(&self, _: lsim_ir::ParamId) -> f64 {
                f64::NAN
            }
        }
        lsim_ir::eval::eval(e, &At(t))
    }

    /// The enclosures hold every value, rate and second rate sampled
    /// inside the interval, on every rule (the rates by central
    /// differences of the point evaluation, with their truncation error).
    #[test]
    fn the_enclosures_hold_the_function_and_its_rates() {
        let t = || Expr::Time;
        let c = Expr::Const;
        let exprs = vec![
            bin(BinaryOp::Sub, call(Builtin::Sin, vec![bin(BinaryOp::Mul, c(0.7), t())]), c(0.95)),
            call(Builtin::Cos, vec![bin(BinaryOp::Mul, c(3.1), t())]),
            call(Builtin::Tan, vec![bin(BinaryOp::Mul, c(0.1), t())]),
            call(Builtin::Asin, vec![bin(BinaryOp::Div, t(), c(12.0))]),
            call(Builtin::Acos, vec![bin(BinaryOp::Div, t(), c(12.0))]),
            call(Builtin::Atan, vec![bin(BinaryOp::Sub, t(), c(3.0))]),
            call(Builtin::Sinh, vec![bin(BinaryOp::Sub, t(), c(3.0))]),
            call(Builtin::Cosh, vec![bin(BinaryOp::Sub, t(), c(3.0))]),
            call(Builtin::Tanh, vec![bin(BinaryOp::Sub, t(), c(3.0))]),
            call(Builtin::Exp, vec![bin(BinaryOp::Mul, c(-0.5), t())]),
            call(Builtin::Log, vec![bin(BinaryOp::Add, t(), c(0.1))]),
            call(Builtin::Sqrt, vec![bin(BinaryOp::Add, t(), c(0.1))]),
            bin(BinaryOp::Pow, bin(BinaryOp::Sub, t(), c(2.0)), c(3.0)),
            bin(BinaryOp::Pow, bin(BinaryOp::Add, t(), c(1.0)), c(-1.5)),
            bin(BinaryOp::Pow, bin(BinaryOp::Add, t(), c(1.0)), t()),
            bin(BinaryOp::Div, c(1.0), bin(BinaryOp::Add, t(), c(0.5))),
            call(Builtin::Abs, vec![bin(BinaryOp::Sub, t(), c(2.5))]),
            call(Builtin::Min, vec![call(Builtin::Sin, vec![t()]), call(Builtin::Cos, vec![t()])]),
            call(
                Builtin::Limit,
                vec![call(Builtin::Sin, vec![bin(BinaryOp::Mul, c(2.0), t())]), c(-0.5), c(0.5)],
            ),
            bin(
                BinaryOp::Mul,
                call(Builtin::Exp, vec![bin(BinaryOp::Mul, c(-0.2), t())]),
                call(Builtin::Sin, vec![bin(BinaryOp::Mul, c(5.0), t())]),
            ),
        ];
        let m = no_model();
        let cx = Cx { params: &[], vars: &[], model: &m, breaks: &[] };
        for e in &exprs {
            for (lo, hi) in [(0.0, 0.3), (0.3, 1.7), (1.9, 2.1), (2.4, 2.6), (0.0, 9.0), (5.0, 5.0)]
            {
                let j = enclose(e, &cx, Iv::new(lo, hi));
                for k in 0..=200 {
                    let s = lo + (hi - lo) * k as f64 / 200.0;
                    let h = 1e-4;
                    let (f0, fp, fm) = (point(e, s), point(e, s + h), point(e, s - h));
                    if !(f0.is_finite() && fp.is_finite() && fm.is_finite()) {
                        continue;
                    }
                    let d = (fp - fm) / (2.0 * h);
                    let dd = (fp - 2.0 * f0 + fm) / (h * h);
                    let slack = |x: f64| 1e-5 * (1.0 + x.abs());
                    assert!(
                        j.v.lo <= f0 && f0 <= j.v.hi,
                        "{e} on [{lo}, {hi}] at {s}: {f0} {:?}",
                        j.v
                    );
                    // (abs, min, limit: kinks inside, where the rate is
                    // one side's or the other's, and there is no second)
                    let kink = j.dd == ALL;
                    assert!(
                        j.d.lo - slack(d) <= d && d <= j.d.hi + slack(d),
                        "{e} on [{lo}, {hi}] at {s}: rate {d} {:?}",
                        j.d
                    );
                    if !kink {
                        assert!(
                            j.dd.lo - 1e-2 * (1.0 + dd.abs()) <= dd
                                && dd <= j.dd.hi + 1e-2 * (1.0 + dd.abs()),
                            "{e} on [{lo}, {hi}] at {s}: second rate {dd} {:?}",
                            j.dd
                        );
                    }
                }
            }
        }
    }

    /// A sine pulse: every sign change found in order, exactly (the float
    /// before has the other sign), and nothing after the last.
    #[test]
    fn every_sign_change_of_a_pulse_is_found_in_order() {
        let period = 10.0;
        let e = bin(
            BinaryOp::Sub,
            call(
                Builtin::Sin,
                vec![bin(BinaryOp::Mul, Expr::Const(2.0 * PI / period), Expr::Time)],
            ),
            Expr::Const(0.95),
        );
        let m = no_model();
        let cx = Cx { params: &[], vars: &[], model: &m, breaks: &[] };
        let mut p = |t: f64| point(&e, t);
        let mut enc = |l: f64, r: f64| {
            let j = enclose(&e, &cx, Iv::new(l, r));
            (j.v, j.d)
        };
        let half = (PI - 2.0 * 0.95f64.asin()) / (2.0 * PI) * period;
        let mut t = 0.0;
        let mut found = vec![];
        loop {
            match first_change(&mut p, &mut enc, t, 100.0, 1000) {
                Found::Change { at, rising } => {
                    found.push((at, rising));
                    t = at;
                }
                Found::Nothing => break,
                other => panic!("{other:?} after {t}"),
            }
        }
        assert_eq!(found.len(), 20, "{found:?}");
        for (k, (at, rising)) in found.iter().enumerate() {
            assert_eq!(*rising, k % 2 == 0);
            let centre = period / 4.0 + period * (k / 2) as f64;
            let want = if *rising { centre - half / 2.0 } else { centre + half / 2.0 };
            assert!((at - want).abs() < 1e-12 * period, "{k}: {at} vs {want}");
            let (before, here) = (p(at.next_down()), p(*at));
            assert!(before.signum() != here.signum() || here == 0.0, "{k}: {before} {here}");
        }
    }

    /// A pulse far narrower than the search's first window, a touch that
    /// does not change the sign, and a jump.
    #[test]
    fn a_narrow_pulse_and_a_jump_are_found_and_a_touch_is_passed() {
        let m = no_model();
        let cx = Cx { params: &[], vars: &[], model: &m, breaks: &[] };
        let run = |e: &Expr, a: f64, b: f64| {
            let mut p = |t: f64| point(e, t);
            let mut enc = |l: f64, r: f64| {
                let j = enclose(e, &cx, Iv::new(l, r));
                (j.v, j.d)
            };
            first_change(&mut p, &mut enc, a, b, 2000)
        };
        // exp(-((t - 500) / 1e-3)²) - 0.5: zeros at 500 ∓ 1e-3 √ln 2
        let z = bin(
            BinaryOp::Div,
            bin(BinaryOp::Sub, Expr::Time, Expr::Const(500.0)),
            Expr::Const(1e-3),
        );
        let gauss = bin(
            BinaryOp::Sub,
            call(Builtin::Exp, vec![Expr::Neg(Box::new(bin(BinaryOp::Mul, z.clone(), z)))]),
            Expr::Const(0.5),
        );
        let w = 1e-3 * 2f64.ln().sqrt();
        match run(&gauss, 0.0, 1000.0) {
            Found::Change { at, rising: true } => assert!((at - (500.0 - w)).abs() < 1e-12, "{at}"),
            other => panic!("{other:?}"),
        }
        match run(&gauss, 500.0, 1000.0) {
            Found::Change { at, rising: false } => {
                assert!((at - (500.0 + w)).abs() < 1e-12, "{at}")
            }
            other => panic!("{other:?}"),
        }
        // (t - 3)² touches zero at 3 without changing sign
        let touch =
            bin(BinaryOp::Pow, bin(BinaryOp::Sub, Expr::Time, Expr::Const(3.0)), Expr::Const(2.0));
        assert_eq!(run(&touch, 0.0, 10.0), Found::Nothing);
        // a jump at 4
        let jump = Expr::If(
            Box::new(Expr::Compare(CmpOp::Gt, Box::new(Expr::Time), Box::new(Expr::Const(4.0)))),
            Box::new(Expr::Const(1.0)),
            Box::new(Expr::Const(-1.0)),
        );
        match run(&jump, 0.0, 10.0) {
            Found::Change { at, rising: true } => assert_eq!(at, 4f64.next_up()),
            other => panic!("{other:?}"),
        }
    }
}
