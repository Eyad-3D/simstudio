//! Interval enclosures of flat-scope expressions over an interval of time
//! (or a box of the variables): the interpreter of the run loop's checks
//! of conditions on functions of time and of conditions that mix time
//! and states (DESIGN.md §8.2), which compiled condition kernels must
//! reproduce bitwise (§5.8, *compiled condition kernels*).
//!
//! [`J2`] encloses a function, its rate and its second rate over a time
//! interval, rigorously: interval arithmetic rounded outwards, a 1-D table
//! by its cubic pieces, a 2-D one by its cells' polynomials ([`Grid2`]),
//! each branch of an `if` with the variables its condition compares
//! bounded as the condition says there.
//!
//! Rounding: every operation's bounds are widened outwards, by an ulp for
//! the exactly rounded ones (`+`, `−`, `×`, `÷`, `sqrt`; a product with a
//! zero bound is exactly zero), by 2 ulps for the
//! platform's `sin`, `cos`, `tan`, `asin`, `acos`, `atan`, `sinh`, `cosh`,
//! `tanh`, `exp`, `ln` and `powf`, which assumes each is within 1 ulp of
//! the exact value (glibc's are; lsim-solve's `tests/libm.rs` checks `sin`,
//! `cos`, `exp` and `ln` on the platform), and relatively by n ε for
//! `powi` (repeated squaring: up to (n − 1) ε / 2).
//!
//! (Moved here unchanged from lsim-solve, so that the code generator's
//! kernels can use the very same operations.)

use crate::expr::{BinaryOp, Builtin, CmpOp, Expr};
use crate::runtime::ModelFunctions;
use std::f64::consts::{FRAC_PI_2, PI, TAU};

mod table2;
pub use table2::{Grid2, Partials, regions};

/// Table `k` of a model at `args` (one or two), as the model interpolates
/// it: its value and partial derivatives (`None` when the model does not
/// give its tables).
pub fn table_at(model: &dyn ModelFunctions, k: u32, args: &[f64]) -> Option<(f64, [f64; 2])> {
    match args {
        [x] => model.eval_table(k, [*x, 0.0]),
        [x, y] => model.eval_table(k, [*x, *y]),
        _ => None,
    }
}

/// A closed interval `[lo, hi]` (bounds may be infinite).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Iv {
    /// the lower bound
    pub lo: f64,
    /// the upper bound
    pub hi: f64,
}

/// Every number: `[−∞, ∞]`.
pub const ALL: Iv = Iv { lo: f64::NEG_INFINITY, hi: f64::INFINITY };
/// Exactly zero.
pub const ZERO: Iv = Iv { lo: 0.0, hi: 0.0 };
/// Exactly one.
pub const ONE: Iv = Iv { lo: 1.0, hi: 1.0 };
/// `[0, 1]` (a truth value that may be either).
pub const UNIT: Iv = Iv { lo: 0.0, hi: 1.0 };

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

// (the operations round outwards: they are not the operators `+`, `−`, `×`)
#[allow(clippy::should_implement_trait)]
impl Iv {
    /// `[lo, hi]` (all numbers when a bound is NaN or `lo > hi`).
    pub fn new(lo: f64, hi: f64) -> Iv {
        if lo.is_nan() || hi.is_nan() || lo > hi { ALL } else { Iv { lo, hi } }
    }

    /// The point `[v, v]`.
    pub fn point(v: f64) -> Iv {
        Iv::new(v, v)
    }

    /// `[lo, hi]` widened by `n` ulps on each side (round-off)
    pub fn wide(lo: f64, hi: f64, n: u32) -> Iv {
        Iv::new(down(lo, n), up(hi, n))
    }

    /// The smallest interval holding both.
    pub fn hull(self, o: Iv) -> Iv {
        Iv::new(self.lo.min(o.lo), self.hi.max(o.hi))
    }

    /// Whether it holds zero.
    pub fn has_zero(self) -> bool {
        self.lo <= 0.0 && self.hi >= 0.0
    }

    /// Whether it is a point.
    pub fn is_point(self) -> bool {
        self.lo == self.hi
    }

    /// The largest magnitude in it.
    pub fn mag(self) -> f64 {
        self.lo.abs().max(self.hi.abs())
    }

    /// The sum, rounded outwards (exact for two points whose sum is).
    pub fn add(self, o: Iv) -> Iv {
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

    /// The difference, rounded outwards.
    pub fn sub(self, o: Iv) -> Iv {
        self.add(o.neg())
    }

    /// The negation (exact).
    pub fn neg(self) -> Iv {
        Iv { lo: -self.hi, hi: -self.lo }
    }

    /// The product, rounded outwards (exact for two points whose product is; a zero bound's products exactly zero).
    pub fn mul(self, o: Iv) -> Iv {
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

    /// The product with a number, rounded outwards.
    pub fn scale(self, k: f64) -> Iv {
        if self.is_point() || !k.is_finite() {
            return self.mul(Iv::point(k));
        }
        if k == 0.0 || self == ZERO {
            return ZERO;
        }
        // a bound of zero stays exactly zero; the others rounded outwards
        let (p, q) = (self.lo * k, self.hi * k);
        let (lo, lo0, hi, hi0) = if k > 0.0 {
            (p, self.lo == 0.0, q, self.hi == 0.0)
        } else {
            (q, self.hi == 0.0, p, self.lo == 0.0)
        };
        Iv::new(if lo0 { 0.0 } else { lo.next_down() }, if hi0 { 0.0 } else { hi.next_up() })
    }

    /// The reciprocal (all numbers when it holds zero).
    pub fn recip(self) -> Iv {
        if self.has_zero() { ALL } else { Iv::wide(1.0 / self.hi, 1.0 / self.lo, 1) }
    }

    /// The square, rounded outwards.
    pub fn sqr(self) -> Iv {
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
    pub fn incr(self, f: fn(f64) -> f64) -> Iv {
        Iv::wide(f(self.lo), f(self.hi), 2)
    }

    /// A decreasing function's enclosure.
    pub fn decr(self, f: fn(f64) -> f64) -> Iv {
        Iv::wide(f(self.hi), f(self.lo), 2)
    }
}

/// Whether `[lo, hi]` holds a point `phase + k · period`; generous by the
/// round-off of reducing the bounds (it may say yes for a point a few
/// ulps outside).
pub fn holds_phase(a: Iv, phase: f64, period: f64) -> bool {
    let slack = 8.0 * f64::EPSILON * a.mag().max(1.0);
    let (lo, hi) = (a.lo - slack, a.hi + slack);
    let k = ((lo - phase) / period).ceil();
    phase + k * period <= hi
}

/// `sin` over an interval.
pub fn sin_iv(a: Iv) -> Iv {
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

/// `cos` over an interval.
pub fn cos_iv(a: Iv) -> Iv {
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
pub fn powf_iv(a: Iv, e: f64) -> Iv {
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
pub fn powi_iv(a: Iv, n: i32) -> Iv {
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
pub struct J2 {
    /// the value
    pub v: Iv,
    /// its rate
    pub d: Iv,
    /// its second rate
    pub dd: Iv,
}

#[allow(clippy::should_implement_trait)]
impl J2 {
    /// A constant (zero rates).
    pub fn konst(v: Iv) -> J2 {
        J2 { v, d: ZERO, dd: ZERO }
    }

    /// Nothing known: every number, every rate.
    pub fn all() -> J2 {
        J2 { v: ALL, d: ALL, dd: ALL }
    }

    /// A value whose rates are unknown (a jump inside the interval, or a
    /// variable known only to lie in `v`).
    pub fn jumps(v: Iv) -> J2 {
        J2 { v, d: ALL, dd: ALL }
    }

    /// Whether its rates are zero.
    pub fn is_const(&self) -> bool {
        self.d == ZERO && self.dd == ZERO
    }

    /// Whether its rates are unknown ([`J2::jumps`]).
    pub fn is_jumps(&self) -> bool {
        self.d == ALL && self.dd == ALL
    }

    /// f(self), given f's enclosures over `self.v`: f, f', f''.
    pub fn chain(self, f: Iv, f1: Iv, f2: Iv) -> J2 {
        if self.is_const() {
            return J2::konst(f);
        }
        if self.is_jumps() && f1 != ZERO {
            // (what the general case gives, without the work)
            return J2::jumps(f);
        }
        J2 { v: f, d: f1.mul(self.d), dd: f2.mul(self.d.sqr()).add(f1.mul(self.dd)) }
    }

    /// The sum.
    pub fn add(self, o: J2) -> J2 {
        J2 { v: self.v.add(o.v), d: self.d.add(o.d), dd: self.dd.add(o.dd) }
    }

    /// The negation.
    pub fn neg(self) -> J2 {
        J2 { v: self.v.neg(), d: self.d.neg(), dd: self.dd.neg() }
    }

    /// The difference.
    pub fn sub(self, o: J2) -> J2 {
        self.add(o.neg())
    }

    /// The product.
    pub fn mul(self, o: J2) -> J2 {
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

    /// The reciprocal.
    pub fn recip(self) -> J2 {
        let v = self.v;
        let r = v.recip();
        self.chain(r, v.sqr().recip().neg(), v.sqr().mul(v).recip().scale(2.0))
    }

    /// The natural logarithm.
    pub fn ln(self) -> J2 {
        let v = self.v;
        if v.lo < 0.0 {
            return J2::all();
        }
        let f =
            if v.lo == 0.0 { Iv::wide(f64::NEG_INFINITY, v.hi.ln(), 2) } else { v.incr(f64::ln) };
        self.chain(f, v.recip(), v.sqr().recip().neg())
    }

    /// The exponential.
    pub fn exp(self) -> J2 {
        let e = self.v.incr(f64::exp);
        self.chain(e, e, e)
    }

    /// `self ^ b`.
    pub fn pow(self, b: J2) -> J2 {
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
pub struct Cx<'a> {
    /// the parameters
    pub params: &'a [f64],
    /// the channels of the variables constant between events
    pub vars: &'a [f64],
    /// the model (its tables)
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

/// Whether [`enclose`] can bound `e` usefully over a time interval: no
/// derivative, unresolved name or `atan2` whose arguments move (`moves`:
/// with time, or with the variables that move along a step), and every
/// table whose arguments move known (a 1-D table's breakpoints, a 2-D
/// table's grid: `grids`, per table).
pub fn supported(
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
pub fn enclose(e: &Expr, cx: &Cx<'_>, t: Iv) -> J2 {
    let ev = |x: &Expr| enclose(x, cx, t);
    let k = |v: f64| J2::konst(Iv::point(v));
    match e {
        Expr::Const(v) => k(*v),
        Expr::Param(p) => k(cx.params[p.0 as usize]),
        Expr::Var(v) | Expr::Pre(v) => match (cx.leaf)(v.0 as usize) {
            Some(j) => j,
            None => k(cx.vars.get(v.0 as usize).copied().unwrap_or(f64::NAN)),
        },
        Expr::Time => time_j2(t),
        Expr::Der(_) | Expr::Name(_) => J2::all(),
        Expr::Neg(a) => ev(a).neg(),
        Expr::NoEvent(a) => ev(a),
        Expr::Binary(op, a, b) => binary_j2(*op, ev(a), ev(b)),
        Expr::Compare(op, a, b) => compare_j2(*op, ev(a), ev(b)),
        Expr::And(a, b) => and_j2(ev(a), ev(b)),
        Expr::Or(a, b) => or_j2(ev(a), ev(b)),
        Expr::Not(a) => not_j2(ev(a)),
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
            if_j2(is_comparison(c), cj, a, b)
        }
        Expr::Table { table, args } => {
            let at: Vec<J2> = args.iter().map(ev).collect();
            table_j2(cx, *table, &at)
        }
        Expr::Call(f, args) => {
            let arg = |i: usize| args.get(i).map(ev).unwrap_or_else(J2::all);
            match f {
                // (the second and third arguments of these only)
                Builtin::Atan2 | Builtin::Min | Builtin::Max => call_j2(*f, &[arg(0), arg(1)]),
                Builtin::Limit => call_j2(*f, &[arg(0), arg(1), arg(2)]),
                _ => call_j2(*f, &[arg(0)]),
            }
        }
    }
}

/// Time over the times `t`: its rate one.
pub fn time_j2(t: Iv) -> J2 {
    J2 { v: t, d: ONE, dd: ZERO }
}

/// `a op b`.
pub fn binary_j2(op: BinaryOp, a: J2, b: J2) -> J2 {
    match op {
        BinaryOp::Add => a.add(b),
        BinaryOp::Sub => a.sub(b),
        BinaryOp::Mul => a.mul(b),
        BinaryOp::Div => a.mul(b.recip()),
        BinaryOp::Pow => a.pow(b),
    }
}

/// The comparison `aj op bj`: 1 or 0 where decided, else a truth that
/// may flip (once, one way, where the sides' difference is strictly
/// monotone).
pub fn compare_j2(op: CmpOp, aj: J2, bj: J2) -> J2 {
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
        let d = if rises { Iv::new(0.0, f64::INFINITY) } else { Iv::new(-f64::INFINITY, 0.0) };
        return J2 { v: UNIT, d, dd: ALL };
    }
    truth(false, false)
}

/// `a and b` (truths).
pub fn and_j2(a: J2, b: J2) -> J2 {
    let (a, b) = (a.v, b.v);
    truth(!a.has_zero() && !b.has_zero(), a == ZERO || b == ZERO)
}

/// `a or b` (truths).
pub fn or_j2(a: J2, b: J2) -> J2 {
    let (a, b) = (a.v, b.v);
    truth(!a.has_zero() || !b.has_zero(), a == ZERO && b == ZERO)
}

/// `not a` (a truth).
pub fn not_j2(a: J2) -> J2 {
    let a = a.v;
    truth(a == ZERO, !a.has_zero())
}

/// Whether an `if`'s condition is a comparison (through `noEvent`).
pub fn is_comparison(c: &Expr) -> bool {
    let mut x = c;
    while let Expr::NoEvent(i) = x {
        x = i;
    }
    matches!(x, Expr::Compare(..))
}

/// An `if` whose condition `cj` is undecided over the interval, its
/// branches `a` and `b` enclosed where each is taken: their hull (with a
/// rate where the condition, a comparison, flips once, one way).
pub fn if_j2(comparison: bool, cj: J2, a: J2, b: J2) -> J2 {
    let v = a.v.hull(b.v);
    // a comparison that flips once at most, one way: b + c (a − b),
    // c a step between 0 and 1 (its rate a jump of known sign)
    let one_way = cj.d.lo >= 0.0 || cj.d.hi <= 0.0;
    if comparison && one_way && cj.v == UNIT {
        let diff = a.sub(b);
        let d = b.d.add(cj.d.mul(diff.v)).add(UNIT.mul(diff.d));
        return J2 { v, d, dd: ALL };
    }
    J2::jumps(v)
}

/// A built-in function (not `der`, `pre`) of its arguments' enclosures
/// (`args[0]` the first; a missing one is unknown).
pub fn call_j2(f: Builtin, args: &[J2]) -> J2 {
    let arg = |i: usize| args.get(i).copied().unwrap_or_else(J2::all);
    let k = |v: f64| J2::konst(Iv::point(v));
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
            if f == Builtin::Asin {
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

/// The bound a comparison puts on a variable below (or equal to) the
/// side enclosed by `h`: at most its largest value.
pub fn below_j2(h: J2) -> Iv {
    Iv { lo: f64::NEG_INFINITY, hi: h.v.hi }
}

/// The bound a comparison puts on a variable above (or equal to) the
/// side enclosed by `l`: at least its least value.
pub fn above_j2(l: J2) -> Iv {
    Iv { lo: l.v.lo, hi: f64::INFINITY }
}

/// A variable's enclosure `j` cut to the bound `r` a condition puts on
/// it where a branch is taken (`None`: nothing is left, the branch is
/// never taken).
pub fn cut_j2(j: J2, r: Iv) -> Option<J2> {
    let (lo, hi) = (j.v.lo.max(r.lo), j.v.hi.min(r.hi));
    if lo > hi {
        return None;
    }
    Some(J2 { v: Iv { lo, hi }, ..j })
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
                out.push((v.0 as usize, below_j2(enclose(hi, cx, t))));
            }
            if let Expr::Var(v) = &**hi {
                out.push((v.0 as usize, above_j2(enclose(lo, cx, t))));
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
        let j = cut_j2(j, r)?;
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

/// `sqrt` over an interval (all numbers below zero).
pub fn sqrt_iv(x: Iv) -> Iv {
    if x.lo < 0.0 { ALL } else { x.incr(f64::sqrt) }
}

/// `cosh` over an interval.
pub fn cosh_iv(x: Iv) -> Iv {
    if x.lo >= 0.0 {
        x.incr(f64::cosh)
    } else if x.hi <= 0.0 {
        x.decr(f64::cosh)
    } else {
        Iv::new(1.0, up(x.lo.cosh().max(x.hi.cosh()), 2))
    }
}

/// A truth value: 1 or 0 when decided, else [0, 1] (it jumps inside).
pub fn truth(yes: bool, no: bool) -> J2 {
    if yes {
        J2::konst(ONE)
    } else if no {
        J2::konst(ZERO)
    } else {
        J2::jumps(UNIT)
    }
}

/// `min` as the evaluators take it (the first on a tie).
pub fn min_j2(a: J2, b: J2) -> J2 {
    if a.v.hi <= b.v.lo {
        a
    } else if b.v.hi < a.v.lo {
        b
    } else {
        // a kink inside: the rate is one of the two
        J2 { v: Iv::new(a.v.lo.min(b.v.lo), a.v.hi.min(b.v.hi)), d: a.d.hull(b.d), dd: ALL }
    }
}

/// `max` as the evaluators take it (the first on a tie).
pub fn max_j2(a: J2, b: J2) -> J2 {
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
pub fn table_j2(cx: &Cx<'_>, k: u32, at: &[J2]) -> J2 {
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
pub fn table_enclosure(model: &dyn ModelFunctions, k: u32, xs: &[f64], u: Iv) -> (Iv, Iv, Iv) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::Builtin;
    use crate::runtime::{EvalInput, Layout};

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
        impl crate::eval::Env for At {
            fn time(&self) -> f64 {
                self.0
            }
            fn var(&self, _: crate::VarId) -> f64 {
                f64::NAN
            }
            fn der(&self, _: crate::VarId) -> f64 {
                f64::NAN
            }
            fn param(&self, _: crate::ParamId) -> f64 {
                f64::NAN
            }
        }
        crate::eval::eval(e, &At(t))
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
}
