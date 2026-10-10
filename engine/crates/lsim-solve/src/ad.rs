//! Exact derivatives of flat-scope expressions by forward-mode automatic
//! differentiation: each value carries its gradient, and when asked its
//! Hessian, with respect to chosen seeds (the impulse projection's states,
//! a stored energy's velocities). No step sizes, no truncation error: the
//! derivatives are those of the expressions as written, to round-off.
//!
//! Relations, `if`, `min`, `max` and `limit` take the branch the values
//! choose (their derivatives are the chosen branch's); `sign` is piecewise
//! constant. A table whose arguments depend on a seed is not
//! differentiated (an error, for the caller to report).

use lsim_ir::expr::{BinaryOp, Builtin, CmpOp, Expr};
use lsim_ir::{ParamId, VarId};

/// A value with its first and (when `second`) second derivatives with
/// respect to `n` seeds.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Jet {
    /// the value
    pub v: f64,
    /// ∂/∂seed_i
    pub g: Vec<f64>,
    /// ∂²/∂seed_i∂seed_j, row-major n × n (empty: first order only)
    pub h: Vec<f64>,
}

impl Jet {
    /// A value that does not depend on the seeds.
    pub fn constant(v: f64, n: usize, second: bool) -> Jet {
        Jet { v, g: vec![0.0; n], h: if second { vec![0.0; n * n] } else { vec![] } }
    }

    /// Seed `i` itself.
    pub fn seed(v: f64, i: usize, n: usize, second: bool) -> Jet {
        let mut j = Jet::constant(v, n, second);
        j.g[i] = 1.0;
        j
    }

    /// A value with a given gradient and no curvature.
    pub fn linear(v: f64, g: Vec<f64>, second: bool) -> Jet {
        let n = g.len();
        Jet { v, g, h: if second { vec![0.0; n * n] } else { vec![] } }
    }

    fn n(&self) -> usize {
        self.g.len()
    }

    /// Whether it depends on a seed.
    pub fn varies(&self) -> bool {
        self.g.iter().any(|x| *x != 0.0) || self.h.iter().any(|x| *x != 0.0)
    }

    /// f(self) with f' = `f1`, f'' = `f2` at `self.v`.
    fn chain(&self, f0: f64, f1: f64, f2: f64) -> Jet {
        let n = self.n();
        let mut out = Jet {
            v: f0,
            g: self.g.iter().map(|x| f1 * x).collect(),
            h: self.h.iter().map(|x| f1 * x).collect(),
        };
        if !out.h.is_empty() && f2 != 0.0 {
            for i in 0..n {
                for j in 0..n {
                    out.h[i * n + j] += f2 * self.g[i] * self.g[j];
                }
            }
        }
        out
    }

    /// f(a, b) with its partial derivatives at (a.v, b.v).
    #[allow(clippy::too_many_arguments)]
    fn chain2(a: &Jet, b: &Jet, f0: f64, fa: f64, fb: f64, faa: f64, fab: f64, fbb: f64) -> Jet {
        let n = a.n();
        let mut out = Jet {
            v: f0,
            g: a.g.iter().zip(&b.g).map(|(x, y)| fa * x + fb * y).collect(),
            h: a.h.iter().zip(&b.h).map(|(x, y)| fa * x + fb * y).collect(),
        };
        if !out.h.is_empty() {
            for i in 0..n {
                for j in 0..n {
                    out.h[i * n + j] += faa * a.g[i] * a.g[j]
                        + fab * (a.g[i] * b.g[j] + b.g[i] * a.g[j])
                        + fbb * b.g[i] * b.g[j];
                }
            }
        }
        out
    }

    pub fn add(&self, b: &Jet) -> Jet {
        Jet::chain2(self, b, self.v + b.v, 1.0, 1.0, 0.0, 0.0, 0.0)
    }

    pub fn sub(&self, b: &Jet) -> Jet {
        Jet::chain2(self, b, self.v - b.v, 1.0, -1.0, 0.0, 0.0, 0.0)
    }

    pub fn mul(&self, b: &Jet) -> Jet {
        Jet::chain2(self, b, self.v * b.v, b.v, self.v, 0.0, 1.0, 0.0)
    }

    pub fn div(&self, b: &Jet) -> Jet {
        let (x, y) = (self.v, b.v);
        let q = x / y;
        Jet::chain2(self, b, q, 1.0 / y, -q / y, 0.0, -1.0 / (y * y), 2.0 * q / (y * y))
    }

    pub fn neg(&self) -> Jet {
        self.chain(-self.v, -1.0, 0.0)
    }

    fn pow(&self, b: &Jet) -> Jet {
        let (x, y) = (self.v, b.v);
        if !b.varies() {
            // x^c: c x^(c-1), c (c-1) x^(c-2)
            let f0 = x.powf(y);
            let f1 = if y == 0.0 { 0.0 } else { y * x.powf(y - 1.0) };
            let f2 = if y == 0.0 || y == 1.0 { 0.0 } else { y * (y - 1.0) * x.powf(y - 2.0) };
            return self.chain(f0, f1, f2);
        }
        // x^y = exp(y ln x)
        let l = x.ln();
        let f0 = x.powf(y);
        Jet::chain2(
            self,
            b,
            f0,
            y * f0 / x,
            f0 * l,
            y * (y - 1.0) * f0 / (x * x),
            f0 * (1.0 + y * l) / x,
            f0 * l * l,
        )
    }
}

/// Where an expression's references get their values (and derivatives).
pub(crate) trait JetEnv {
    /// the number of seeds
    fn n(&self) -> usize;
    /// whether second derivatives are wanted
    fn second(&self) -> bool;
    /// time, s
    fn time(&self) -> f64;
    /// a variable's value and derivatives
    fn var(&self, v: VarId) -> Result<Jet, String>;
    /// a parameter's value
    fn param(&self, p: ParamId) -> f64;
    /// a derivative's value (not differentiated)
    fn der(&self, _v: VarId) -> f64 {
        f64::NAN
    }
    /// a table interpolated at `args` (NaN when there are no tables)
    fn table(&self, _k: u32, _args: &[f64]) -> f64 {
        f64::NAN
    }
}

fn truth(b: bool) -> f64 {
    if b { 1.0 } else { 0.0 }
}

/// Evaluates `e` with its derivatives.
pub(crate) fn eval(e: &Expr, env: &dyn JetEnv) -> Result<Jet, String> {
    let (n, second) = (env.n(), env.second());
    let k = |v: f64| Jet::constant(v, n, second);
    let ev = |x: &Expr| eval(x, env);
    Ok(match e {
        Expr::Const(v) => k(*v),
        Expr::Time => k(env.time()),
        Expr::Name(x) => return Err(format!("an unresolved name '{x}'")),
        Expr::Param(p) => k(env.param(*p)),
        Expr::Var(v) => env.var(*v)?,
        Expr::Pre(v) => k(env.var(*v)?.v),
        Expr::Der(v) => k(env.der(*v)),
        Expr::Neg(a) => ev(a)?.neg(),
        Expr::NoEvent(a) => ev(a)?,
        Expr::Table { table, args } => {
            let at: Vec<Jet> = args.iter().map(ev).collect::<Result<_, _>>()?;
            if at.iter().any(Jet::varies) {
                return Err("a table whose argument moves with the states".into());
            }
            let vals: Vec<f64> = at.iter().map(|j| j.v).collect();
            k(env.table(*table, &vals))
        }
        Expr::Binary(op, a, b) => {
            let (a, b) = (ev(a)?, ev(b)?);
            match op {
                BinaryOp::Add => a.add(&b),
                BinaryOp::Sub => a.sub(&b),
                BinaryOp::Mul => a.mul(&b),
                BinaryOp::Div => a.div(&b),
                BinaryOp::Pow => a.pow(&b),
            }
        }
        Expr::Compare(op, a, b) => {
            let (a, b) = (ev(a)?.v, ev(b)?.v);
            k(truth(match op {
                CmpOp::Lt => a < b,
                CmpOp::Le => a <= b,
                CmpOp::Gt => a > b,
                CmpOp::Ge => a >= b,
            }))
        }
        Expr::And(a, b) => k(truth(ev(a)?.v != 0.0 && ev(b)?.v != 0.0)),
        Expr::Or(a, b) => k(truth(ev(a)?.v != 0.0 || ev(b)?.v != 0.0)),
        Expr::Not(a) => k(truth(ev(a)?.v == 0.0)),
        Expr::If(c, a, b) => {
            if ev(c)?.v != 0.0 {
                ev(a)?
            } else {
                ev(b)?
            }
        }
        Expr::Call(f, args) => {
            let arg = |i: usize| -> Result<Jet, String> {
                args.get(i).map(ev).unwrap_or_else(|| Err(format!("{f:?} needs more arguments")))
            };
            let a = arg(0)?;
            let x = a.v;
            match f {
                Builtin::Der | Builtin::Pre => k(f64::NAN),
                Builtin::Sin => a.chain(x.sin(), x.cos(), -x.sin()),
                Builtin::Cos => a.chain(x.cos(), -x.sin(), -x.cos()),
                Builtin::Tan => {
                    let t = x.tan();
                    a.chain(t, 1.0 + t * t, 2.0 * t * (1.0 + t * t))
                }
                Builtin::Asin => {
                    let r = 1.0 - x * x;
                    a.chain(x.asin(), 1.0 / r.sqrt(), x / (r * r.sqrt()))
                }
                Builtin::Acos => {
                    let r = 1.0 - x * x;
                    a.chain(x.acos(), -1.0 / r.sqrt(), -x / (r * r.sqrt()))
                }
                Builtin::Atan => {
                    let r = 1.0 + x * x;
                    a.chain(x.atan(), 1.0 / r, -2.0 * x / (r * r))
                }
                Builtin::Atan2 => {
                    // atan2(y = a, x = b)
                    let b = arg(1)?;
                    let (yv, xv) = (a.v, b.v);
                    let r2 = xv * xv + yv * yv;
                    let r4 = r2 * r2;
                    Jet::chain2(
                        &a,
                        &b,
                        yv.atan2(xv),
                        xv / r2,
                        -yv / r2,
                        -2.0 * xv * yv / r4,
                        (yv * yv - xv * xv) / r4,
                        2.0 * xv * yv / r4,
                    )
                }
                Builtin::Sinh => a.chain(x.sinh(), x.cosh(), x.sinh()),
                Builtin::Cosh => a.chain(x.cosh(), x.sinh(), x.cosh()),
                Builtin::Tanh => {
                    let t = x.tanh();
                    a.chain(t, 1.0 - t * t, -2.0 * t * (1.0 - t * t))
                }
                Builtin::Exp => {
                    let ex = x.exp();
                    a.chain(ex, ex, ex)
                }
                Builtin::Log => a.chain(x.ln(), 1.0 / x, -1.0 / (x * x)),
                Builtin::Sqrt => {
                    let s = x.sqrt();
                    a.chain(s, 0.5 / s, -0.25 / (x * s))
                }
                Builtin::Abs => {
                    let s = if x > 0.0 {
                        1.0
                    } else if x < 0.0 {
                        -1.0
                    } else {
                        0.0
                    };
                    a.chain(x.abs(), s, 0.0)
                }
                Builtin::Sign => k(if x > 0.0 {
                    1.0
                } else if x < 0.0 {
                    -1.0
                } else {
                    0.0
                }),
                Builtin::Min | Builtin::Max => {
                    let b = arg(1)?;
                    // the value f64::min/max takes (the first on a tie)
                    let take_b = if *f == Builtin::Min { b.v < a.v } else { b.v > a.v };
                    if take_b { b } else { a }
                }
                Builtin::Limit => {
                    let (lo, hi) = (arg(1)?, arg(2)?);
                    if a.v < lo.v {
                        lo
                    } else if a.v > hi.v {
                        hi
                    } else {
                        a
                    }
                }
            }
        }
    })
}

/// A value and its rate along one direction: the first-order, one-seed
/// case of [`Jet`] without its allocations (the energy books take every
/// stored energy's rate this way at each step).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Dual {
    /// the value
    pub v: f64,
    /// its rate
    pub d: f64,
}

impl Dual {
    fn k(v: f64) -> Dual {
        Dual { v, d: 0.0 }
    }
    fn f(self, f0: f64, f1: f64) -> Dual {
        Dual { v: f0, d: f1 * self.d }
    }
}

/// Where an expression's references get their values and rates.
pub(crate) trait DualEnv {
    /// a variable's value and rate (`Err`: it has none here)
    fn var(&self, v: VarId) -> Result<Dual, ()>;
    /// a parameter's value
    fn param(&self, p: ParamId) -> f64;
}

/// `e` with its rate, by the rules of [`eval`]; `Err` for what has no rate
/// here: the time, a derivative, a previous value, a table, a name.
pub(crate) fn dual(e: &Expr, env: &dyn DualEnv) -> Result<Dual, ()> {
    let ev = |x: &Expr| dual(x, env);
    let k = Dual::k;
    Ok(match e {
        Expr::Const(v) => k(*v),
        Expr::Param(p) => k(env.param(*p)),
        Expr::Var(v) => env.var(*v)?,
        Expr::Time | Expr::Name(_) | Expr::Pre(_) | Expr::Der(_) | Expr::Table { .. } => {
            return Err(());
        }
        Expr::Neg(a) => {
            let a = ev(a)?;
            Dual { v: -a.v, d: -a.d }
        }
        Expr::NoEvent(a) => ev(a)?,
        Expr::Binary(op, a, b) => {
            let (a, b) = (ev(a)?, ev(b)?);
            match op {
                BinaryOp::Add => Dual { v: a.v + b.v, d: a.d + b.d },
                BinaryOp::Sub => Dual { v: a.v - b.v, d: a.d - b.d },
                BinaryOp::Mul => Dual { v: a.v * b.v, d: b.v * a.d + a.v * b.d },
                BinaryOp::Div => {
                    let q = a.v / b.v;
                    Dual { v: q, d: a.d / b.v - q / b.v * b.d }
                }
                BinaryOp::Pow => {
                    let (x, y) = (a.v, b.v);
                    let f0 = x.powf(y);
                    if b.d == 0.0 {
                        let f1 = if y == 0.0 { 0.0 } else { y * x.powf(y - 1.0) };
                        a.f(f0, f1)
                    } else {
                        Dual { v: f0, d: y * f0 / x * a.d + f0 * x.ln() * b.d }
                    }
                }
            }
        }
        Expr::Compare(op, a, b) => {
            let (a, b) = (ev(a)?.v, ev(b)?.v);
            k(truth(match op {
                CmpOp::Lt => a < b,
                CmpOp::Le => a <= b,
                CmpOp::Gt => a > b,
                CmpOp::Ge => a >= b,
            }))
        }
        Expr::And(a, b) => k(truth(ev(a)?.v != 0.0 && ev(b)?.v != 0.0)),
        Expr::Or(a, b) => k(truth(ev(a)?.v != 0.0 || ev(b)?.v != 0.0)),
        Expr::Not(a) => k(truth(ev(a)?.v == 0.0)),
        Expr::If(c, a, b) => {
            if ev(c)?.v != 0.0 {
                ev(a)?
            } else {
                ev(b)?
            }
        }
        Expr::Call(f, args) => {
            let arg = |i: usize| args.get(i).map(ev).unwrap_or(Err(()));
            let a = arg(0)?;
            let x = a.v;
            match f {
                Builtin::Der | Builtin::Pre => return Err(()),
                Builtin::Sin => a.f(x.sin(), x.cos()),
                Builtin::Cos => a.f(x.cos(), -x.sin()),
                Builtin::Tan => {
                    let t = x.tan();
                    a.f(t, 1.0 + t * t)
                }
                Builtin::Asin => a.f(x.asin(), 1.0 / (1.0 - x * x).sqrt()),
                Builtin::Acos => a.f(x.acos(), -1.0 / (1.0 - x * x).sqrt()),
                Builtin::Atan => a.f(x.atan(), 1.0 / (1.0 + x * x)),
                Builtin::Atan2 => {
                    let b = arg(1)?;
                    let r2 = b.v * b.v + a.v * a.v;
                    Dual { v: a.v.atan2(b.v), d: (b.v * a.d - a.v * b.d) / r2 }
                }
                Builtin::Sinh => a.f(x.sinh(), x.cosh()),
                Builtin::Cosh => a.f(x.cosh(), x.sinh()),
                Builtin::Tanh => {
                    let t = x.tanh();
                    a.f(t, 1.0 - t * t)
                }
                Builtin::Exp => {
                    let ex = x.exp();
                    a.f(ex, ex)
                }
                Builtin::Log => a.f(x.ln(), 1.0 / x),
                Builtin::Sqrt => {
                    let r = x.sqrt();
                    a.f(r, 0.5 / r)
                }
                Builtin::Abs => {
                    let s = if x > 0.0 {
                        1.0
                    } else if x < 0.0 {
                        -1.0
                    } else {
                        0.0
                    };
                    a.f(x.abs(), s)
                }
                Builtin::Sign => k(if x > 0.0 {
                    1.0
                } else if x < 0.0 {
                    -1.0
                } else {
                    0.0
                }),
                Builtin::Min | Builtin::Max => {
                    let b = arg(1)?;
                    let take_b = if *f == Builtin::Min { b.v < a.v } else { b.v > a.v };
                    if take_b { b } else { a }
                }
                Builtin::Limit => {
                    let (lo, hi) = (arg(1)?, arg(2)?);
                    if a.v < lo.v {
                        lo
                    } else if a.v > hi.v {
                        hi
                    } else {
                        a
                    }
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsim_ir::expr::{BinaryOp, Builtin, Expr};

    /// Two seeds, x0 and x1, at the given values.
    struct Two([f64; 2], bool);

    impl JetEnv for Two {
        fn n(&self) -> usize {
            2
        }
        fn second(&self) -> bool {
            self.1
        }
        fn time(&self) -> f64 {
            0.0
        }
        fn var(&self, v: VarId) -> Result<Jet, String> {
            Ok(Jet::seed(self.0[v.0 as usize], v.0 as usize, 2, self.1))
        }
        fn param(&self, _p: ParamId) -> f64 {
            3.0
        }
    }

    fn bin(op: BinaryOp, a: Expr, b: Expr) -> Expr {
        Expr::Binary(op, Box::new(a), Box::new(b))
    }

    fn call(f: Builtin, a: Vec<Expr>) -> Expr {
        Expr::Call(f, a)
    }

    /// The scalar rate (`dual`) is the full evaluator's gradient taken along
    /// a direction, value and rate alike, on every rule.
    #[test]
    fn the_scalar_rate_is_the_gradient_along_its_direction() {
        let x = || Expr::Var(VarId(0));
        let y = || Expr::Var(VarId(1));
        let c = Expr::Const;
        let cmp = |op, a, b| Expr::Compare(op, Box::new(a), Box::new(b));
        let exprs = vec![
            bin(BinaryOp::Mul, bin(BinaryOp::Mul, c(0.5), x()), bin(BinaryOp::Mul, x(), y())),
            bin(BinaryOp::Div, x(), bin(BinaryOp::Add, y(), Expr::Param(ParamId(0)))),
            bin(BinaryOp::Pow, x(), c(3.0)),
            bin(BinaryOp::Pow, x(), y()),
            call(Builtin::Sin, vec![bin(BinaryOp::Mul, x(), y())]),
            call(Builtin::Cos, vec![x()]),
            call(Builtin::Tan, vec![y()]),
            call(Builtin::Asin, vec![bin(BinaryOp::Div, x(), c(4.0))]),
            call(Builtin::Acos, vec![bin(BinaryOp::Div, y(), c(4.0))]),
            call(Builtin::Atan, vec![bin(BinaryOp::Sub, x(), y())]),
            call(Builtin::Atan2, vec![y(), x()]),
            call(Builtin::Sinh, vec![x()]),
            call(Builtin::Cosh, vec![y()]),
            call(Builtin::Tanh, vec![bin(BinaryOp::Mul, x(), y())]),
            call(Builtin::Exp, vec![bin(BinaryOp::Mul, x(), y())]),
            call(Builtin::Log, vec![bin(BinaryOp::Mul, x(), y())]),
            call(Builtin::Sqrt, vec![bin(BinaryOp::Add, x(), y())]),
            call(Builtin::Abs, vec![bin(BinaryOp::Sub, y(), x())]),
            call(Builtin::Sign, vec![bin(BinaryOp::Sub, y(), x())]),
            call(Builtin::Max, vec![x(), y()]),
            call(Builtin::Min, vec![x(), y()]),
            call(Builtin::Limit, vec![bin(BinaryOp::Mul, x(), y()), c(0.0), c(1.0)]),
            Expr::Neg(Box::new(bin(BinaryOp::Sub, x(), y()))),
            Expr::If(
                Box::new(cmp(CmpOp::Gt, x(), y())),
                Box::new(bin(BinaryOp::Mul, x(), x())),
                Box::new(y()),
            ),
            Expr::NoEvent(Box::new(bin(BinaryOp::Mul, x(), y()))),
        ];
        struct Along([f64; 2], [f64; 2]);
        impl DualEnv for Along {
            fn var(&self, v: VarId) -> Result<Dual, ()> {
                let i = v.0 as usize;
                Ok(Dual { v: self.0[i], d: self.1[i] })
            }
            fn param(&self, _p: ParamId) -> f64 {
                3.0
            }
        }
        let (at, dir) = ([1.3, 0.7], [0.37, -1.9]);
        for e in &exprs {
            let j = eval(e, &Two(at, false)).unwrap();
            let r = dual(e, &Along(at, dir)).unwrap();
            let along = j.g[0] * dir[0] + j.g[1] * dir[1];
            assert_eq!(r.v, j.v, "{e}");
            assert!((r.d - along).abs() <= 1e-14 * (1.0 + along.abs()), "{e}: {} vs {along}", r.d);
        }
        // what has no rate here
        for e in [Expr::Time, Expr::Der(VarId(0)), Expr::Pre(VarId(0))] {
            assert!(dual(&e, &Along(at, dir)).is_err(), "{e}");
        }
    }

    /// Every rule against central differences of the interpreter (whose
    /// truncation error the tolerance allows for), first and second order.
    #[test]
    fn the_derivatives_are_those_of_the_expression() {
        let x = || Expr::Var(VarId(0));
        let y = || Expr::Var(VarId(1));
        let exprs = vec![
            bin(
                BinaryOp::Mul,
                bin(BinaryOp::Mul, Expr::Const(0.5), x()),
                bin(BinaryOp::Mul, x(), y()),
            ),
            bin(BinaryOp::Div, x(), bin(BinaryOp::Add, y(), Expr::Param(ParamId(0)))),
            bin(BinaryOp::Pow, x(), Expr::Const(3.0)),
            bin(BinaryOp::Pow, x(), y()),
            call(Builtin::Sin, vec![bin(BinaryOp::Mul, x(), y())]),
            call(Builtin::Cos, vec![x()]),
            call(Builtin::Tan, vec![y()]),
            call(Builtin::Asin, vec![bin(BinaryOp::Div, x(), Expr::Const(4.0))]),
            call(Builtin::Acos, vec![bin(BinaryOp::Div, y(), Expr::Const(4.0))]),
            call(Builtin::Atan, vec![bin(BinaryOp::Sub, x(), y())]),
            call(Builtin::Atan2, vec![y(), x()]),
            call(Builtin::Sinh, vec![x()]),
            call(Builtin::Cosh, vec![y()]),
            call(Builtin::Tanh, vec![bin(BinaryOp::Mul, x(), y())]),
            call(Builtin::Exp, vec![bin(BinaryOp::Mul, x(), y())]),
            call(Builtin::Log, vec![bin(BinaryOp::Mul, x(), y())]),
            call(Builtin::Sqrt, vec![bin(BinaryOp::Add, x(), y())]),
            call(Builtin::Abs, vec![bin(BinaryOp::Sub, y(), x())]),
            call(Builtin::Max, vec![x(), y()]),
            Expr::Neg(Box::new(bin(BinaryOp::Sub, x(), y()))),
        ];
        let at = [1.3, 0.7];
        for e in &exprs {
            let j = eval(e, &Two(at, true)).unwrap();
            let f = |p: [f64; 2]| {
                let env = lsim_ir::eval::SliceEnv { t: 0.0, vars: &p, ders: &[], params: &[3.0] };
                lsim_ir::eval::eval(e, &env)
            };
            assert_eq!(j.v, f(at), "{e}");
            let h = 1e-5;
            for i in 0..2 {
                let mut p = at;
                p[i] += h;
                let up = f(p);
                p[i] -= 2.0 * h;
                let dn = f(p);
                let fd = (up - dn) / (2.0 * h);
                assert!(
                    (j.g[i] - fd).abs() < 1e-8 * (1.0 + fd.abs()),
                    "{e}: d/dx{i} {} vs {fd}",
                    j.g[i]
                );
                for k in 0..2 {
                    let df = |q: [f64; 2]| {
                        let mut a = q;
                        a[k] += h;
                        let u = f(a);
                        a[k] -= 2.0 * h;
                        (u - f(a)) / (2.0 * h)
                    };
                    let mut p = at;
                    p[i] += h;
                    let up = df(p);
                    p[i] -= 2.0 * h;
                    let fd2 = (up - df(p)) / (2.0 * h);
                    let got = j.h[i * 2 + k];
                    assert!(
                        (got - fd2).abs() < 1e-4 * (1.0 + fd2.abs()),
                        "{e}: d2/dx{i}dx{k} {got} vs {fd2}"
                    );
                }
            }
            // first order only: the same gradient
            let j1 = eval(e, &Two(at, false)).unwrap();
            assert_eq!(j1.g, j.g, "{e}");
            assert!(j1.h.is_empty());
        }
    }

    /// A quadratic form's Hessian is its matrix, exactly.
    #[test]
    fn a_kinetic_energy_has_its_masses_as_hessian() {
        // ½·2·x² + ½·5·y²
        let x = || Expr::Var(VarId(0));
        let y = || Expr::Var(VarId(1));
        let half = |m: f64, v: Expr| {
            bin(BinaryOp::Mul, bin(BinaryOp::Mul, Expr::Const(0.5 * m), v.clone()), v)
        };
        let e = bin(BinaryOp::Add, half(2.0, x()), half(5.0, y()));
        let j = eval(&e, &Two([3.7, -11.2], true)).unwrap();
        assert_eq!(j.h, vec![2.0, 0.0, 0.0, 5.0]);
        assert_eq!(j.g, vec![2.0 * 3.7, 5.0 * -11.2]);
    }
}
