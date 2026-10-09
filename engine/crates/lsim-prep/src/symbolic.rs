//! Symbolic algebra on flat expressions: simplification, differentiation
//! with respect to an unknown, and solving an equation for an unknown it
//! is linear in.
//!
//! Work package 2 extends this with the rules index reduction needs
//! (differentiating whole equations in time) and a stronger simplifier;
//! the generated Jacobians do not depend on it (codegen differentiates in
//! forward mode).

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
    if !contains(residual, s) {
        return None;
    }
    let a = simplify(diff(residual, s));
    if contains(&a, s) || a.any(&mut |x| matches!(x, Expr::Const(v) if v.is_nan())) {
        return None;
    }
    if is_const(&a, 0.0) {
        return None;
    }
    let b = simplify(substitute(residual.clone(), s, &Expr::Const(0.0)));
    let sol = match (&a, &b) {
        (Expr::Const(x), _) if *x == 1.0 => -b,
        (Expr::Const(x), _) if *x == -1.0 => b,
        _ => -b / a,
    };
    Some(simplify(sol))
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
