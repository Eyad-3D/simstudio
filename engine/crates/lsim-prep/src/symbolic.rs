//! Symbolic algebra on flat expressions: simplification, differentiation
//! with respect to an unknown, differentiation in time (what index
//! reduction does to an equation), solving an equation for an unknown it
//! is affine in, and the sign analysis behind the pivot checks (can a
//! coefficient be zero for an allowed parameter value?).
//!
//! The generated Jacobians do not depend on this (codegen differentiates
//! in forward mode).

use lsim_ir::VarId;
use lsim_ir::expr::{BinaryOp, Builtin, Expr};
use lsim_ir::prepared::Slot;

fn is_const(e: &Expr, v: f64) -> bool {
    matches!(e, Expr::Const(x) if *x == v)
}

/// Folds constants and removes neutral elements.
pub fn simplify(e: Expr) -> Expr {
    e.rewrite(&mut |x| match x {
        Expr::Neg(a) => match *a {
            Expr::Const(v) => Expr::Const(-v),
            Expr::Neg(b) => *b,
            other => Expr::Neg(Box::new(other)),
        },
        Expr::Binary(op, a, b) => {
            if let (Expr::Const(x), Expr::Const(y)) = (&*a, &*b) {
                let v = match op {
                    BinaryOp::Add => x + y,
                    BinaryOp::Sub => x - y,
                    BinaryOp::Mul => x * y,
                    BinaryOp::Div => x / y,
                    BinaryOp::Pow => x.powf(*y),
                };
                if v.is_finite() {
                    return Expr::Const(v);
                }
            }
            match op {
                BinaryOp::Add if is_const(&a, 0.0) => *b,
                BinaryOp::Add | BinaryOp::Sub if is_const(&b, 0.0) => *a,
                BinaryOp::Sub if is_const(&a, 0.0) => Expr::Neg(b),
                BinaryOp::Add => match *b {
                    Expr::Neg(nb) => Expr::Binary(BinaryOp::Sub, a, nb),
                    other => Expr::Binary(BinaryOp::Add, a, Box::new(other)),
                },
                BinaryOp::Sub => match *b {
                    Expr::Neg(nb) => Expr::Binary(BinaryOp::Add, a, nb),
                    other => Expr::Binary(BinaryOp::Sub, a, Box::new(other)),
                },
                BinaryOp::Mul if is_const(&a, 0.0) || is_const(&b, 0.0) => Expr::Const(0.0),
                BinaryOp::Mul if is_const(&a, 1.0) => *b,
                BinaryOp::Mul if is_const(&b, 1.0) => *a,
                BinaryOp::Mul if is_const(&a, -1.0) => Expr::Neg(b),
                BinaryOp::Mul if is_const(&b, -1.0) => Expr::Neg(a),
                BinaryOp::Div if is_const(&b, 1.0) => *a,
                BinaryOp::Mul | BinaryOp::Div
                    if matches!(*a, Expr::Neg(_)) || matches!(*b, Expr::Neg(_)) =>
                {
                    let strip = |x: Box<Expr>| match *x {
                        Expr::Neg(inner) => (inner, true),
                        other => (Box::new(other), false),
                    };
                    let ((na, sa), (nb, sb)) = (strip(a), strip(b));
                    let inner = Expr::Binary(op, na, nb);
                    if sa != sb { Expr::Neg(Box::new(inner)) } else { inner }
                }
                BinaryOp::Div if is_const(&a, 0.0) => Expr::Const(0.0),
                BinaryOp::Pow if is_const(&b, 1.0) => *a,
                BinaryOp::Pow if is_const(&b, 0.0) => Expr::Const(1.0),
                _ => Expr::Binary(op, a, b),
            }
        }
        other => other,
    })
}

/// Whether `e` refers to `s`.
pub fn contains(e: &Expr, s: Slot) -> bool {
    e.any(&mut |x| match (x, s) {
        (Expr::Var(v), Slot::Var(w)) => *v == w,
        (Expr::Der(v), Slot::Der(w)) => *v == w,
        _ => false,
    })
}

/// Replaces `s` by `with` everywhere in `e`.
pub fn substitute(e: Expr, s: Slot, with: &Expr) -> Expr {
    e.rewrite(&mut |x| match (&x, s) {
        (Expr::Var(v), Slot::Var(w)) if *v == w => with.clone(),
        (Expr::Der(v), Slot::Der(w)) if *v == w => with.clone(),
        _ => x,
    })
}

/// ∂e/∂s, not simplified. `None` where the derivative does not exist as an
/// expression (relations are piecewise constant: their derivative is 0).
pub fn diff(e: &Expr, s: Slot) -> Expr {
    use Expr::*;
    let d = |x: &Expr| diff(x, s);
    let b = |x: Expr| Box::new(x);
    match e {
        Var(v) => Const(if matches!(s, Slot::Var(w) if w == *v) { 1.0 } else { 0.0 }),
        Der(v) => Const(if matches!(s, Slot::Der(w) if w == *v) { 1.0 } else { 0.0 }),
        Const(_) | Time | Name(_) | Param(_) | Pre(_) | Compare(..) | And(..) | Or(..) | Not(_) => {
            Const(0.0)
        }
        Neg(a) => Neg(b(d(a))),
        NoEvent(a) => d(a),
        Binary(op, x, y) => {
            let (dx, dy) = (d(x), d(y));
            match op {
                BinaryOp::Add => dx + dy,
                BinaryOp::Sub => dx - dy,
                BinaryOp::Mul => dx * (**y).clone() + (**x).clone() * dy,
                BinaryOp::Div => (dx - e.clone() * dy) / (**y).clone(),
                BinaryOp::Pow => {
                    if let Const(n) = **y {
                        Const(n) * Binary(BinaryOp::Pow, x.clone(), b(Const(n - 1.0))) * dx
                    } else {
                        // d(x^y) = x^y (y' ln x + y x'/x)
                        e.clone()
                            * (dy * Call(Builtin::Log, vec![(**x).clone()])
                                + (**y).clone() * dx / (**x).clone())
                    }
                }
            }
        }
        If(c, x, y) => If(c.clone(), b(d(x)), b(d(y))),
        Call(f, args) => {
            let a = || args[0].clone();
            let da = || d(&args[0]);
            let call = |g: Builtin, x: Expr| Call(g, vec![x]);
            match f {
                Builtin::Sin => call(Builtin::Cos, a()) * da(),
                Builtin::Cos => -(call(Builtin::Sin, a()) * da()),
                Builtin::Tan => da() / (call(Builtin::Cos, a()) * call(Builtin::Cos, a())),
                Builtin::Asin => da() / call(Builtin::Sqrt, Const(1.0) - a() * a()),
                Builtin::Acos => -(da() / call(Builtin::Sqrt, Const(1.0) - a() * a())),
                Builtin::Atan => da() / (Const(1.0) + a() * a()),
                Builtin::Atan2 => {
                    let (y, x) = (args[0].clone(), args[1].clone());
                    (x.clone() * d(&y) - y.clone() * d(&x)) / (x.clone() * x + y.clone() * y)
                }
                Builtin::Sinh => call(Builtin::Cosh, a()) * da(),
                Builtin::Cosh => call(Builtin::Sinh, a()) * da(),
                Builtin::Tanh => {
                    (Const(1.0) - call(Builtin::Tanh, a()) * call(Builtin::Tanh, a())) * da()
                }
                Builtin::Exp => e.clone() * da(),
                Builtin::Log => da() / a(),
                Builtin::Sqrt => da() / (Const(2.0) * e.clone()),
                Builtin::Abs => call(Builtin::Sign, a()) * da(),
                Builtin::Sign | Builtin::Der | Builtin::Pre => Const(0.0),
                Builtin::Min | Builtin::Max => {
                    let op =
                        if *f == Builtin::Min { lsim_ir::CmpOp::Lt } else { lsim_ir::CmpOp::Gt };
                    If(
                        b(Compare(op, b(args[0].clone()), b(args[1].clone()))),
                        b(da()),
                        b(d(&args[1])),
                    )
                }
                Builtin::Limit => {
                    // inside the band: dx; outside: the bound's derivative
                    let (x, lo, hi) = (&args[0], &args[1], &args[2]);
                    If(
                        b(Compare(lsim_ir::CmpOp::Lt, b(x.clone()), b(lo.clone()))),
                        b(d(lo)),
                        b(If(
                            b(Compare(lsim_ir::CmpOp::Gt, b(x.clone()), b(hi.clone()))),
                            b(d(hi)),
                            b(da()),
                        )),
                    )
                }
            }
        }
        Table { .. } => Const(f64::NAN),
    }
}

/// Solves `residual = 0` for `s` when the residual is affine in `s`
/// (`a·s + b` with `a` free of `s`): returns `-b / a`, simplified.
pub fn solve_for(residual: &Expr, s: Slot) -> Option<Expr> {
    solve_affine(residual, s).map(|(_, sol)| sol)
}

/// The time derivative of `e` by the chain rule: `dvar(v)` gives the
/// derivative of variable `v` (another variable, or 0 for a piecewise
/// constant one). Relations are piecewise constant (derivative 0), so an
/// `if` keeps its condition; `min`, `max` and `limit` differentiate into
/// `noEvent` choices, since they make no events themselves. Tables cannot
/// be differentiated yet.
pub fn time_derivative(e: &Expr, dvar: &mut dyn FnMut(VarId) -> Expr) -> Result<Expr, String> {
    use Expr::*;
    let b = |x: Expr| Box::new(x);
    Ok(match e {
        Const(_) | Param(_) | Pre(_) | Compare(..) | And(..) | Or(..) | Not(_) => Const(0.0),
        Time => Const(1.0),
        Var(v) => dvar(*v),
        Der(_) | Name(_) => return Err(format!("cannot differentiate {e} in time here")),
        Table { .. } => {
            return Err("a table would have to be differentiated in time (index reduction \
                        through a table is not supported yet)"
                .into());
        }
        Neg(a) => Neg(b(time_derivative(a, dvar)?)),
        NoEvent(a) => NoEvent(b(time_derivative(a, dvar)?)),
        If(c, x, y) => If(c.clone(), b(time_derivative(x, dvar)?), b(time_derivative(y, dvar)?)),
        Binary(op, x, y) => {
            let dx = time_derivative(x, dvar)?;
            let dy = time_derivative(y, dvar)?;
            let (xc, yc) = ((**x).clone(), (**y).clone());
            match op {
                BinaryOp::Add => dx + dy,
                BinaryOp::Sub => dx - dy,
                BinaryOp::Mul => dx * yc + xc * dy,
                BinaryOp::Div => (dx - e.clone() * dy) / yc,
                BinaryOp::Pow => {
                    if is_const(&dy, 0.0) {
                        // d(x^n) = n x^(n-1) dx, n constant in time
                        let n1 = simplify(yc.clone() - Const(1.0));
                        yc * Binary(BinaryOp::Pow, x.clone(), b(n1)) * dx
                    } else {
                        e.clone() * (dy * Call(Builtin::Log, vec![xc.clone()]) + yc * dx / xc)
                    }
                }
            }
        }
        Call(f, args) => {
            let a = args[0].clone();
            let da = time_derivative(&args[0], dvar)?;
            let call = |g: Builtin, x: Expr| Call(g, vec![x]);
            match f {
                Builtin::Der | Builtin::Pre => {
                    return Err(format!("cannot differentiate {e} in time here"));
                }
                Builtin::Sin => call(Builtin::Cos, a) * da,
                Builtin::Cos => -(call(Builtin::Sin, a) * da),
                Builtin::Tan => da / (call(Builtin::Cos, a.clone()) * call(Builtin::Cos, a)),
                Builtin::Asin => da / call(Builtin::Sqrt, Const(1.0) - a.clone() * a),
                Builtin::Acos => -(da / call(Builtin::Sqrt, Const(1.0) - a.clone() * a)),
                Builtin::Atan => da / (Const(1.0) + a.clone() * a),
                Builtin::Atan2 => {
                    let x = args[1].clone();
                    let dx = time_derivative(&args[1], dvar)?;
                    (x.clone() * da - a.clone() * dx) / (x.clone() * x + a.clone() * a)
                }
                Builtin::Sinh => call(Builtin::Cosh, a) * da,
                Builtin::Cosh => call(Builtin::Sinh, a) * da,
                Builtin::Tanh => {
                    (Const(1.0) - call(Builtin::Tanh, a.clone()) * call(Builtin::Tanh, a)) * da
                }
                Builtin::Exp => e.clone() * da,
                Builtin::Log => da / a,
                Builtin::Sqrt => da / (Const(2.0) * e.clone()),
                Builtin::Abs => NoEvent(b(call(Builtin::Sign, a))) * da,
                Builtin::Sign => Const(0.0),
                Builtin::Min | Builtin::Max => {
                    let db = time_derivative(&args[1], dvar)?;
                    let op =
                        if *f == Builtin::Min { lsim_ir::CmpOp::Lt } else { lsim_ir::CmpOp::Gt };
                    NoEvent(b(If(b(Compare(op, b(a), b(args[1].clone()))), b(da), b(db))))
                }
                Builtin::Limit => {
                    let (lo, hi) = (&args[1], &args[2]);
                    let dlo = time_derivative(lo, dvar)?;
                    let dhi = time_derivative(hi, dvar)?;
                    NoEvent(b(If(
                        b(Compare(lsim_ir::CmpOp::Lt, b(a.clone()), b(lo.clone()))),
                        b(dlo),
                        b(If(b(Compare(lsim_ir::CmpOp::Gt, b(a), b(hi.clone()))), b(dhi), b(da))),
                    )))
                }
            }
        }
    })
}

/// `e` as `a·s + b` (coefficient `None`: zero), when it is affine in `s`
/// with `a` and `b` free of `s`; built directly, without differentiating.
fn split(e: &Expr, s: Slot) -> Option<(Option<Expr>, Expr)> {
    use Expr::*;
    let hit = |x: &Expr| match (x, s) {
        (Var(v), Slot::Var(w)) | (Der(v), Slot::Der(w)) => *v == w,
        _ => false,
    };
    if hit(e) {
        return Some((Some(Const(1.0)), Const(0.0)));
    }
    let b = |x: Expr| Box::new(x);
    match e {
        Neg(x) => {
            let (a, r) = split(x, s)?;
            Some((a.map(|a| Neg(b(a))), Neg(b(r))))
        }
        NoEvent(x) => split(x, s),
        Binary(op @ (BinaryOp::Add | BinaryOp::Sub), x, y) => {
            let (a1, b1) = split(x, s)?;
            let (a2, b2) = split(y, s)?;
            let a = match (a1, a2) {
                (None, None) => None,
                (Some(a), None) => Some(a),
                (None, Some(a)) => Some(if *op == BinaryOp::Sub { Neg(b(a)) } else { a }),
                (Some(a1), Some(a2)) => Some(Binary(*op, b(a1), b(a2))),
            };
            Some((a, Binary(*op, b(b1), b(b2))))
        }
        Binary(BinaryOp::Mul, x, y) => {
            let (a1, b1) = split(x, s)?;
            let (a2, b2) = split(y, s)?;
            match (a1, a2) {
                (None, None) => Some((None, e.clone())),
                (Some(a1), None) => Some((Some(a1 * b2.clone()), b1 * b2)),
                (None, Some(a2)) => Some((Some(b1.clone() * a2), b1 * b2)),
                (Some(_), Some(_)) => None,
            }
        }
        Binary(BinaryOp::Div, x, y) => {
            if contains(y, s) {
                return None;
            }
            let (a1, b1) = split(x, s)?;
            Some((a1.map(|a| a / (**y).clone()), b1 / (**y).clone()))
        }
        If(c, x, y) => {
            if contains(c, s) {
                return None;
            }
            let (a1, b1) = split(x, s)?;
            let (a2, b2) = split(y, s)?;
            let a = match (a1, a2) {
                (None, None) => None,
                (a1, a2) => {
                    Some(If(c.clone(), b(a1.unwrap_or(Const(0.0))), b(a2.unwrap_or(Const(0.0)))))
                }
            };
            Some((a, If(c.clone(), b(b1), b(b2))))
        }
        other => {
            if contains(other, s) {
                None
            } else {
                Some((None, other.clone()))
            }
        }
    }
}

/// When `residual = 0` is affine in `s` with a coefficient that is not
/// zero: the coefficient `a` and the solution `-b / a`, simplified.
pub fn solve_affine(residual: &Expr, s: Slot) -> Option<(Expr, Expr)> {
    let (a, b) = split(residual, s)?;
    let a = simplify(a?);
    if is_const(&a, 0.0) || a.any(&mut |x| matches!(x, Expr::Const(v) if v.is_nan())) {
        return None;
    }
    let b = simplify(b);
    let sol = match &a {
        Expr::Const(x) if *x == 1.0 => -b,
        Expr::Const(x) if *x == -1.0 => b,
        _ => -b / a.clone(),
    };
    Some((a, simplify(sol)))
}

/// The coefficient `a` when `residual` is affine in `s` (`a·s + b` with
/// `a` and `b` free of `s`), simplified; `None` when it is not, or when
/// `s` does not appear.
pub fn affine_coefficient(residual: &Expr, s: Slot) -> Option<Expr> {
    let (a, _) = split(residual, s)?;
    let a = simplify(a?);
    if is_const(&a, 0.0) || a.any(&mut |x| matches!(x, Expr::Const(v) if v.is_nan())) {
        return None;
    }
    Some(a)
}

/// The signs an expression can take: a subset of {negative, zero,
/// positive}, and whether that relied on a parameter keeping the sign it
/// has now (a parameter declared without a range).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Signs {
    /// it can be negative
    pub neg: bool,
    /// it can be zero
    pub zero: bool,
    /// it can be positive
    pub pos: bool,
    /// the answer assumes some parameter keeps its present sign
    pub assumed: bool,
}

impl Signs {
    /// Any value.
    pub const ANY: Signs = Signs { neg: true, zero: true, pos: true, assumed: false };

    fn exact(v: f64) -> Signs {
        if v.is_nan() {
            return Signs::ANY;
        }
        Signs { neg: v < 0.0, zero: v == 0.0, pos: v > 0.0, assumed: false }
    }

    fn with(self, assumed: bool) -> Signs {
        Signs { assumed: self.assumed || assumed, ..self }
    }

    fn union(self, o: Signs) -> Signs {
        Signs {
            neg: self.neg || o.neg,
            zero: self.zero || o.zero,
            pos: self.pos || o.pos,
            assumed: self.assumed || o.assumed,
        }
    }

    fn neg_(self) -> Signs {
        Signs { neg: self.pos, pos: self.neg, ..self }
    }

    fn mul(self, o: Signs) -> Signs {
        Signs {
            neg: (self.neg && o.pos) || (self.pos && o.neg),
            pos: (self.pos && o.pos) || (self.neg && o.neg),
            zero: self.zero || o.zero,
            assumed: self.assumed || o.assumed,
        }
    }

    fn add(self, o: Signs) -> Signs {
        Signs {
            neg: self.neg || o.neg,
            pos: self.pos || o.pos,
            zero: (self.zero && o.zero) || (self.pos && o.neg) || (self.neg && o.pos),
            assumed: self.assumed || o.assumed,
        }
    }

    /// Whether it can never be zero.
    pub fn nonzero(self) -> bool {
        !self.zero && (self.neg || self.pos)
    }
}

/// What the sign analysis knows about the leaves of an expression.
pub trait SignEnv {
    /// a parameter's signs
    fn param(&self, p: lsim_ir::ParamId) -> Signs;
    /// a variable's signs (usually any)
    fn var(&self, v: VarId) -> Signs;
}

/// The signs `e` can take.
pub fn signs(e: &Expr, env: &dyn SignEnv) -> Signs {
    use Expr::*;
    let s = |x: &Expr| signs(x, env);
    match e {
        Const(v) => Signs::exact(*v),
        Param(p) => env.param(*p),
        Var(v) | Pre(v) => env.var(*v),
        Time | Name(_) | Der(_) | Table { .. } => Signs::ANY,
        Neg(a) => s(a).neg_(),
        NoEvent(a) => s(a),
        Compare(..) | And(..) | Or(..) | Not(_) => {
            Signs { neg: false, zero: true, pos: true, assumed: false }
        }
        If(_, a, b) => s(a).union(s(b)),
        Binary(op, a, b) => {
            let (sa, sb) = (s(a), s(b));
            match op {
                BinaryOp::Add => sa.add(sb),
                BinaryOp::Sub => sa.add(sb.neg_()),
                BinaryOp::Mul => sa.mul(sb),
                BinaryOp::Div => {
                    if sb.zero {
                        Signs::ANY.with(sa.assumed || sb.assumed)
                    } else {
                        sa.mul(sb)
                    }
                }
                BinaryOp::Pow => match &**b {
                    Const(n) if n.fract() == 0.0 => {
                        let n = *n as i64;
                        if n == 0 {
                            Signs { neg: false, zero: false, pos: true, assumed: false }
                        } else if n < 0 && sa.zero {
                            Signs::ANY.with(sa.assumed)
                        } else if n % 2 == 0 {
                            Signs { neg: false, zero: sa.zero, pos: sa.neg || sa.pos, ..sa }
                        } else {
                            sa
                        }
                    }
                    Const(_) if !sa.neg => Signs { neg: false, ..sa },
                    _ => Signs::ANY.with(sa.assumed || sb.assumed),
                },
            }
        }
        Call(f, args) => {
            let sa = s(&args[0]);
            match f {
                Builtin::Exp | Builtin::Cosh => {
                    Signs { neg: false, zero: false, pos: true, assumed: sa.assumed }
                }
                Builtin::Sqrt => Signs { neg: false, zero: sa.zero || sa.neg, ..sa },
                Builtin::Abs => Signs { neg: false, pos: sa.neg || sa.pos, ..sa },
                Builtin::Sign | Builtin::Sinh | Builtin::Tanh | Builtin::Atan | Builtin::Asin => sa,
                Builtin::Min | Builtin::Max => sa.union(s(&args[1])),
                Builtin::Limit => sa.union(s(&args[1])).union(s(&args[2])),
                _ => Signs::ANY.with(sa.assumed),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsim_ir::eval::{SliceEnv, eval};
    use lsim_ir::expr::{c, call};
    use lsim_ir::flat::{ParamId, VarId};

    fn v(i: u32) -> Expr {
        Expr::Var(VarId(i))
    }

    #[test]
    fn solves_ohms_law_for_the_current() {
        // v0 = R * v1  →  v1 = v0 / R
        let r = v(0) - Expr::Param(ParamId(0)) * v(1);
        let s = solve_for(&r, Slot::Var(VarId(1))).unwrap();
        assert_eq!(s.to_string(), "v[0] / p[0]");
        let env = SliceEnv { t: 0.0, vars: &[10.0, 0.0], ders: &[0.0, 0.0], params: &[4.0] };
        assert_eq!(eval(&s, &env), 2.5);
        // nonlinear in the unknown: not solved
        let r2 = v(0) - call(Builtin::Exp, vec![v(1)]);
        assert!(solve_for(&r2, Slot::Var(VarId(1))).is_none());
    }

    #[test]
    fn derivatives_match_finite_differences() {
        let x = v(0);
        let exprs = vec![
            call(Builtin::Sin, vec![x.clone() * c(2.0)]) / (c(1.0) + x.clone() * x.clone()),
            call(Builtin::Exp, vec![-x.clone()]) * call(Builtin::Sqrt, vec![x.clone() + c(3.0)]),
            Expr::bin(BinaryOp::Pow, x.clone(), c(3.0)) - call(Builtin::Tanh, vec![x.clone()]),
            call(Builtin::Atan2, vec![x.clone(), c(2.0) - x.clone()]),
        ];
        for e in exprs {
            let de = simplify(diff(&e, Slot::Var(VarId(0))));
            for x0 in [0.3, 1.1, 2.0] {
                let at =
                    |x: f64| eval(&e, &SliceEnv { t: 0.0, vars: &[x], ders: &[0.0], params: &[] });
                let h = 1e-6;
                let fd = (at(x0 + h) - at(x0 - h)) / (2.0 * h);
                let an = eval(&de, &SliceEnv { t: 0.0, vars: &[x0], ders: &[0.0], params: &[] });
                assert!((fd - an).abs() < 1e-7 * an.abs().max(1.0), "{e}: {fd} vs {an}");
            }
        }
    }
}
