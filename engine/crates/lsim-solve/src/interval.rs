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
//! interval, rigorously (interval arithmetic rounded outwards, a 1-D table
//! by its cubic pieces, a 2-D one by its cells' polynomials; each branch
//! of an `if` with the variables its condition compares bounded as the
//! condition says there), and [`first_change`] walks forward in time.
//! It skips an interval whose enclosure keeps the sign, or where the
//! function is monotone with the same sign at both ends (a comparison
//! whose sides' difference is strictly monotone over the interval flips
//! once at most, one way: a step of known sign, monotone too); otherwise it
//! advances by what the bound on the rate allows (a function of value g
//! and rate at most L cannot reach zero within |g| / L), shrinking the
//! interval as it nears a zero, and bisects to adjacent floats once a sign
//! change is bracketed. A grazing touch without a sign change is passed.
//!
//! Rounding: every operation's bounds are widened outwards, by an ulp for
//! the exactly rounded ones (`+`, `−`, `×`, `÷`, `sqrt`; a product with a
//! zero bound is exactly zero), by 2 ulps for the
//! platform's `sin`, `cos`, `tan`, `asin`, `acos`, `atan`, `sinh`, `cosh`,
//! `tanh`, `exp`, `ln` and `powf`, which assumes each is within 1 ulp of
//! the exact value (glibc's are; `tests/libm.rs` checks `sin`, `cos`,
//! `exp` and `ln` on the platform), and relatively by n ε for `powi`
//! (repeated squaring: up to (n − 1) ε / 2).

use crate::info::table_at;
use lsim_ir::expr::{BinaryOp, Builtin, CmpOp, Expr};
use lsim_ir::runtime::ModelFunctions;
use std::f64::consts::{FRAC_PI_2, PI, TAU};

mod table2;
pub(crate) use table2::Grid2;

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
    pub(crate) fn new(lo: f64, hi: f64) -> Iv {
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
        // the products of nonzero bounds rounded outwards; a zero bound's
        // products are exactly zero (a step's rate [0, ∞] stays one-signed)
        let (mut lo, mut hi, mut zero) = (f64::INFINITY, f64::NEG_INFINITY, false);
        for (a, b) in [(self.lo, o.lo), (self.lo, o.hi), (self.hi, o.lo), (self.hi, o.hi)] {
            if a == 0.0 || b == 0.0 {
                zero = true;
            } else {
                let x = a * b;
                lo = lo.min(x);
                hi = hi.max(x);
            }
        }
        let (mut lo, mut hi) = (down(lo, 1), up(hi, 1));
        if zero {
            (lo, hi) = (lo.min(0.0), hi.max(0.0));
        }
        Iv::new(lo, hi)
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
    // `powi` multiplies by repeated squaring (compiler-rt's __powidf2):
    // its relative error is at most (n - 1) u (u = ε / 2), measured at 4.6
    // ulps for n = 8, 10.3 for 16 and 44 for 64: widened by n ε relatively
    let (p0, p1) = (a.lo.powi(n), a.hi.powi(n));
    let r = n as f64 * f64::EPSILON;
    let lo_of = |x: f64| down(x - r * x.abs(), 1);
    let hi_of = |x: f64| up(x + r * x.abs(), 1);
    if n % 2 == 1 || a.lo >= 0.0 {
        Iv::new(lo_of(p0), hi_of(p1))
    } else if a.hi <= 0.0 {
        Iv::new(lo_of(p1), hi_of(p0))
    } else {
        Iv::new(0.0, hi_of(p0.max(p1)))
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
    pub(crate) fn konst(v: Iv) -> J2 {
        J2 { v, d: ZERO, dd: ZERO }
    }

    fn all() -> J2 {
        J2 { v: ALL, d: ALL, dd: ALL }
    }

    /// A value whose rates are unknown (a jump inside the interval, or a
    /// variable known only to lie in `v`).
    pub(crate) fn jumps(v: Iv) -> J2 {
        J2 { v, d: ALL, dd: ALL }
    }

    fn is_const(&self) -> bool {
        self.d == ZERO && self.dd == ZERO
    }

    /// Whether its rates are unknown ([`J2::jumps`]).
    fn is_jumps(&self) -> bool {
        self.d == ALL && self.dd == ALL
    }

    /// f(self), given f's enclosures over `self.v`: f, f', f''.
    fn chain(self, f: Iv, f1: Iv, f2: Iv) -> J2 {
        if self.is_const() {
            return J2::konst(f);
        }
        if self.is_jumps() && f1 != ZERO {
            // (what the general case gives, without the work)
            return J2::jumps(f);
        }
        J2 { v: f, d: f1.mul(self.d), dd: f2.mul(self.d.sqr()).add(f1.mul(self.dd)) }
    }

    fn add(self, o: J2) -> J2 {
        J2 { v: self.v.add(o.v), d: self.d.add(o.d), dd: self.dd.add(o.dd) }
    }

    pub(crate) fn neg(self) -> J2 {
        J2 { v: self.v.neg(), d: self.d.neg(), dd: self.dd.neg() }
    }

    fn sub(self, o: J2) -> J2 {
        self.add(o.neg())
    }

    fn mul(self, o: J2) -> J2 {
        if (self.is_jumps() && o.v != ZERO) || (o.is_jumps() && self.v != ZERO) {
            // (what the general case gives, without the work)
            return J2::jumps(self.v.mul(o.v));
        }
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
    /// per table: a 2-D table's grid (`None`: 1-D, or not known)
    pub grids: &'a [Option<Grid2>],
    /// the variables that move (along a step, or within a box), by
    /// channel: their enclosures (`None`: constant, its channel's value)
    pub leaf: &'a dyn Fn(usize) -> Option<J2>,
}

/// A variable along a step: the polynomial through the integrator's dense
/// output, `Σ a_k u^k` in `u = (τ − c) / s` on `[-1, 1]`, and a bound
/// `err` on its distance from that output (round-off, a dense output that
/// is not a polynomial of that degree).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Poly {
    pub c: f64,
    pub s: f64,
    pub a: Vec<f64>,
    pub err: f64,
}

impl Poly {
    /// The value at `t`.
    pub(crate) fn at(&self, t: f64) -> f64 {
        let u = (t - self.c) / self.s;
        self.a.iter().rev().fold(0.0, |p, a| p * u + a)
    }

    /// The value over `t` alone (as [`Poly::j2`] gives it).
    pub(crate) fn value(&self, t: Iv) -> Iv {
        let u = Iv::wide((t.lo - self.c) / self.s, (t.hi - self.c) / self.s, 2);
        let v = self.a.iter().rev().fold(ZERO, |p, a| p.mul(u).add(Iv::point(*a)));
        Iv::new(v.lo - self.err, v.hi + self.err)
    }

    /// The value, rate and second rate over `t` (interval Horner; the
    /// rates' error bounded from `err` by Markov's inequality, n² per
    /// derivative for a polynomial of degree n on [-1, 1]).
    pub(crate) fn j2(&self, t: Iv) -> J2 {
        let u = Iv::wide((t.lo - self.c) / self.s, (t.hi - self.c) / self.s, 2);
        let horner =
            |coef: &[f64]| coef.iter().rev().fold(ZERO, |p, a| p.mul(u).add(Iv::point(*a)));
        let n = self.a.len();
        let d1: Vec<f64> = (1..n).map(|k| k as f64 * self.a[k]).collect();
        let d2: Vec<f64> = (2..n).map(|k| (k * (k - 1)) as f64 * self.a[k]).collect();
        let m = ((n.max(2) - 1) * (n.max(2) - 1)) as f64;
        let e = self.err;
        let widen = |x: Iv, e: f64| Iv::new(x.lo - e, x.hi + e);
        let s = self.s;
        J2 {
            v: widen(horner(&self.a), e),
            d: widen(horner(&d1).scale(1.0 / s), m * e / s),
            dd: widen(horner(&d2).scale(1.0 / (s * s)), m * m * e / (s * s)),
        }
    }
}

/// Whether [`enclose`] can bound `e` usefully over a time interval: no
/// derivative, unresolved name or `atan2` whose arguments move (`moves`:
/// with time, or with the variables that move along a step), and every
/// table whose arguments move known (a 1-D table's breakpoints, a 2-D
/// table's grid: `grids`, per table).
pub(crate) fn supported(
    e: &Expr,
    breaks: &[Vec<f64>],
    grids: &[Option<Grid2>],
    moves: &dyn Fn(&Expr) -> bool,
) -> bool {
    let mut ok = true;
    e.walk(&mut |x| match x {
        Expr::Der(_) | Expr::Name(_) => ok = false,
        Expr::Call(Builtin::Der | Builtin::Pre, _) => ok = false,
        Expr::Call(Builtin::Atan2, args) if args.iter().any(moves) => ok = false,
        Expr::Table { table, args } if args.iter().any(moves) => {
            let k = *table as usize;
            let known = match args.len() {
                1 => breaks.get(k).is_some_and(|b| !b.is_empty()),
                2 => grids.get(k).is_some_and(|g| g.is_some()),
                _ => false,
            };
            ok &= known;
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
        Expr::Var(v) | Expr::Pre(v) => match (cx.leaf)(v.0 as usize) {
            Some(j) => j,
            None => k(cx.vars.get(v.0 as usize).copied().unwrap_or(f64::NAN)),
        },
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
            let (aj, bj) = (ev(a), ev(b));
            let (a, b) = (aj.v, bj.v);
            let (yes, no) = match op {
                CmpOp::Lt => (a.hi < b.lo, a.lo >= b.hi),
                CmpOp::Le => (a.hi <= b.lo, a.lo > b.hi),
                CmpOp::Gt => (a.lo > b.hi, a.hi <= b.lo),
                CmpOp::Ge => (a.lo >= b.hi, a.hi < b.lo),
            };
            if yes || no {
                return truth(yes, no);
            }
            // undecided: where a − b is strictly monotone it crosses zero
            // once at most, so the truth flips once at most, one way (its
            // rate a jump of known sign)
            let r = aj.d.sub(bj.d);
            if r.lo > 0.0 || r.hi < 0.0 {
                let rises = (r.lo > 0.0) == matches!(op, CmpOp::Gt | CmpOp::Ge);
                let d =
                    if rises { Iv::new(0.0, f64::INFINITY) } else { Iv::new(-f64::INFINITY, 0.0) };
                return J2 { v: UNIT, d, dd: ALL };
            }
            truth(false, false)
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
            let cj = ev(c);
            let cv = cj.v;
            if !cv.has_zero() {
                return ev(a);
            } else if cv == ZERO {
                return ev(b);
            }
            // each branch where it is taken: the variables the condition
            // compares bounded as it says (a branch it rules out entirely
            // is never taken)
            let (a, b) = match (branch(c, true, a, cx, t), branch(c, false, b, cx, t)) {
                (Some(a), Some(b)) => (a, b),
                (Some(a), None) => return a,
                (None, Some(b)) => return b,
                (None, None) => (ev(a), ev(b)),
            };
            let v = a.v.hull(b.v);
            // a comparison that flips once at most, one way: b + c (a − b),
            // c a step between 0 and 1 (its rate a jump of known sign)
            let mut x = &**c;
            while let Expr::NoEvent(i) = x {
                x = i;
            }
            let one_way = cj.d.lo >= 0.0 || cj.d.hi <= 0.0;
            if matches!(x, Expr::Compare(..)) && one_way && cj.v == UNIT {
                let diff = a.sub(b);
                let d = b.d.add(cj.d.mul(diff.v)).add(UNIT.mul(diff.d));
                return J2 { v, d, dd: ALL };
            }
            J2::jumps(v)
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

/// The bounds condition `c` puts on the variables it compares with
/// something else when it holds (`holds`) or fails: `x < y` bounds `x`
/// above by `y`'s largest value and `y` below by `x`'s least; through
/// `noEvent`, `not`, `and` (both hold) and `or` (both fail).
fn bounds(c: &Expr, holds: bool, cx: &Cx<'_>, t: Iv, out: &mut Vec<(usize, Iv)>) {
    match c {
        Expr::NoEvent(a) => bounds(a, holds, cx, t, out),
        Expr::Not(a) => bounds(a, !holds, cx, t, out),
        Expr::And(a, b) if holds => {
            bounds(a, true, cx, t, out);
            bounds(b, true, cx, t, out);
        }
        Expr::Or(a, b) if !holds => {
            bounds(a, false, cx, t, out);
            bounds(b, false, cx, t, out);
        }
        Expr::Compare(op, a, b) => {
            // x below y (or equal) when it holds
            let below = matches!(op, CmpOp::Lt | CmpOp::Le) == holds;
            let (lo, hi) = if below { (a, b) } else { (b, a) };
            if let Expr::Var(v) = &**lo {
                let h = enclose(hi, cx, t).v;
                out.push((v.0 as usize, Iv { lo: f64::NEG_INFINITY, hi: h.hi }));
            }
            if let Expr::Var(v) = &**hi {
                let l = enclose(lo, cx, t).v;
                out.push((v.0 as usize, Iv { lo: l.lo, hi: f64::INFINITY }));
            }
        }
        _ => {}
    }
}

/// Branch `e` of an `if` on `c` where it is taken (`holds`: the condition
/// holds there): its variables bounded as `c` says; `None` when the
/// bounds leave a variable no value (the branch is never taken).
fn branch(c: &Expr, holds: bool, e: &Expr, cx: &Cx<'_>, t: Iv) -> Option<J2> {
    let mut b: Vec<(usize, Iv)> = vec![];
    bounds(c, holds, cx, t, &mut b);
    // only the variables that move (a constant one decides the condition)
    b.retain(|(v, _)| (cx.leaf)(*v).is_some());
    if b.is_empty() {
        return Some(enclose(e, cx, t));
    }
    let mut cut: Vec<(usize, J2)> = vec![];
    for (v, r) in b {
        let j = cut.iter().find(|(w, _)| *w == v).map(|(_, j)| *j).or_else(|| (cx.leaf)(v))?;
        let (lo, hi) = (j.v.lo.max(r.lo), j.v.hi.min(r.hi));
        if lo > hi {
            return None;
        }
        let j = J2 { v: Iv { lo, hi }, ..j };
        match cut.iter_mut().find(|(w, _)| *w == v) {
            Some(x) => x.1 = j,
            None => cut.push((v, j)),
        }
    }
    let leaf =
        |v: usize| cut.iter().find(|(w, _)| *w == v).map(|(_, j)| *j).or_else(|| (cx.leaf)(v));
    let inner = Cx { leaf: &leaf, ..*cx };
    Some(enclose(e, &inner, t))
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

/// A table at `at`: constant arguments give its value; a table whose
/// arguments move is enclosed piece by piece (a 2-D table by its grid's
/// patches).
fn table_j2(cx: &Cx<'_>, k: u32, at: &[J2]) -> J2 {
    if at.iter().all(|a| a.is_const() && a.v.is_point()) {
        let args: Vec<f64> = at.iter().map(|a| a.v.lo).collect();
        return match table_at(cx.model, k, &args) {
            Some((v, _)) => J2::konst(Iv::point(v)),
            None => J2::all(),
        };
    }
    if let [a, b] = at {
        let Some(Some(grid)) = cx.grids.get(k as usize) else { return J2::all() };
        let Some([f, fx, fy, fxx, fxy, fyy]) = grid.enclose(cx.model, a.v, b.v) else {
            return J2::all();
        };
        // f(a(t), b(t)): the rate f_x a' + f_y b', the second rate
        // f_xx a'² + 2 f_xy a' b' + f_yy b'² + f_x a'' + f_y b''
        let d = fx.mul(a.d).add(fy.mul(b.d));
        let dd = fxx
            .mul(a.d.sqr())
            .add(fxy.mul(a.d.mul(b.d)).scale(2.0))
            .add(fyy.mul(b.d.sqr()))
            .add(fx.mul(a.dd))
            .add(fy.mul(b.dd));
        return J2 { v: f, d, dd };
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
        // (monotone in the wide sense: a step of known sign counts)
        let monotone = s0 != 0 && (d.lo >= 0.0 || d.hi <= 0.0);
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
        let cx = Cx { params: &[], vars: &[], model: &m, breaks: &[], grids: &[], leaf: &|_| None };
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

    /// An integer power's enclosure holds the power however `powi`
    /// rounds (repeated squaring is off by up to 44 ulps at n = 64): the
    /// exact power, from `powf` (within an ulp) and from the product
    /// computed in double-double, at many points.
    #[test]
    fn an_integer_power_encloses_the_exact_power() {
        // x^n in double-double: (hi, lo) with hi + lo the product to ~1e-32
        fn dd_pow(x: f64, n: i32) -> f64 {
            let (mut hi, mut lo) = (1.0f64, 0.0f64);
            for _ in 0..n {
                let p = hi * x;
                let e = hi.mul_add(x, -p) + lo * x;
                let s = p + e;
                lo = e - (s - p);
                hi = s;
            }
            hi + lo
        }
        let mut worst = 0.0f64;
        for k in 0..20_000 {
            let x = 0.5 + 1.5 * (k as f64 / 20_000.0) + 1e-7 * (k as f64).sin();
            for n in [3, 8, 16, 31, 64] {
                let iv = powi_iv(Iv::point(x), n);
                let exact = dd_pow(x, n);
                let ulps = (x.powi(n) - exact).abs() / (exact.next_up() - exact);
                worst = worst.max(ulps);
                assert!(iv.lo <= exact && exact <= iv.hi, "{x}^{n}: {exact} not in {iv:?}");
                assert!(iv.lo <= x.powf(n as f64) && x.powf(n as f64) <= iv.hi, "{x}^{n}");
                let neg = powi_iv(Iv::point(-x), n);
                let e = if n % 2 == 0 { exact } else { -exact };
                assert!(neg.lo <= e && e <= neg.hi, "-{x}^{n}");
            }
        }
        println!("powi: worst {worst:.1} ulps from the exact power");
    }

    /// A variable along a step, as a polynomial: its enclosures hold its
    /// value and rates everywhere inside, and its error widens them.
    #[test]
    fn a_polynomial_leaf_holds_its_values_and_rates() {
        let p =
            Poly { c: 3.0, s: 0.5, a: vec![1.0, -2.0, 0.5, 3.0, -1.0, 0.25, 0.1, -0.05], err: 0.0 };
        let du = |u: f64, k: usize| -> f64 {
            // the k-th derivative in u
            (k..p.a.len())
                .map(|i| {
                    let f: f64 = ((i - k + 1)..=i).map(|x| x as f64).product();
                    f * p.a[i] * u.powi((i - k) as i32)
                })
                .sum()
        };
        for (lo, hi) in [(2.5, 3.5), (2.9, 3.1), (3.2, 3.2), (2.5, 2.6)] {
            let j = p.j2(Iv::new(lo, hi));
            for k in 0..=100 {
                let t = lo + (hi - lo) * k as f64 / 100.0;
                let u = (t - p.c) / p.s;
                let (v, d, dd) = (du(u, 0), du(u, 1) / p.s, du(u, 2) / (p.s * p.s));
                assert!((p.at(t) - v).abs() < 1e-12);
                assert!(j.v.lo <= v && v <= j.v.hi, "[{lo}, {hi}] at {t}: {v} {:?}", j.v);
                assert!(j.d.lo <= d && d <= j.d.hi, "[{lo}, {hi}] at {t}: {d} {:?}", j.d);
                assert!(j.dd.lo <= dd && dd <= j.dd.hi, "[{lo}, {hi}] at {t}: {dd} {:?}", j.dd);
            }
        }
        let wide = Poly { err: 1e-3, ..p.clone() }.j2(Iv::new(3.0, 3.0));
        assert!(wide.v.hi - wide.v.lo >= 2e-3 && wide.d.hi - wide.d.lo >= 2e-3 / p.s);
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
        let cx = Cx { params: &[], vars: &[], model: &m, breaks: &[], grids: &[], leaf: &|_| None };
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
        let cx = Cx { params: &[], vars: &[], model: &m, breaks: &[], grids: &[], leaf: &|_| None };
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
