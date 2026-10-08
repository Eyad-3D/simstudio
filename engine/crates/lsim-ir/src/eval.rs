//! A reference interpreter for flat-scope expressions: the yardstick the
//! generated code is tested against, and constant folding's evaluator.
//! Truth values are 1.0 and 0.0.

use crate::expr::{BinaryOp, Builtin, CmpOp, Expr};
use crate::flat::{ParamId, VarId};

/// Where an expression's references get their values.
pub trait Env {
    /// time, s
    fn time(&self) -> f64;
    /// a variable's value
    fn var(&self, v: VarId) -> f64;
    /// a state's derivative
    fn der(&self, v: VarId) -> f64;
    /// a discrete variable's value before the event
    fn pre(&self, v: VarId) -> f64 {
        self.var(v)
    }
    /// a parameter's value
    fn param(&self, p: ParamId) -> f64;
}

fn truth(b: bool) -> f64 {
    if b { 1.0 } else { 0.0 }
}

/// Evaluates `e`. Unresolved names and tables give NaN.
pub fn eval(e: &Expr, env: &dyn Env) -> f64 {
    let ev = |x: &Expr| eval(x, env);
    match e {
        Expr::Const(v) => *v,
        Expr::Time => env.time(),
        Expr::Name(_) | Expr::Table { .. } => f64::NAN,
        Expr::Var(v) => env.var(*v),
        Expr::Param(p) => env.param(*p),
        Expr::Der(v) => env.der(*v),
        Expr::Pre(v) => env.pre(*v),
        Expr::Neg(a) => -ev(a),
        Expr::Binary(op, a, b) => {
            let (a, b) = (ev(a), ev(b));
            match op {
                BinaryOp::Add => a + b,
                BinaryOp::Sub => a - b,
                BinaryOp::Mul => a * b,
                BinaryOp::Div => a / b,
                BinaryOp::Pow => a.powf(b),
            }
        }
        Expr::Call(f, args) => {
            let a = args.first().map(ev).unwrap_or(f64::NAN);
            let b = || ev(&args[1]);
            match f {
                Builtin::Der | Builtin::Pre => f64::NAN,
                Builtin::Sin => a.sin(),
                Builtin::Cos => a.cos(),
                Builtin::Tan => a.tan(),
                Builtin::Asin => a.asin(),
                Builtin::Acos => a.acos(),
                Builtin::Atan => a.atan(),
                Builtin::Atan2 => a.atan2(b()),
                Builtin::Sinh => a.sinh(),
                Builtin::Cosh => a.cosh(),
                Builtin::Tanh => a.tanh(),
                Builtin::Exp => a.exp(),
                Builtin::Log => a.ln(),
                Builtin::Sqrt => a.sqrt(),
                Builtin::Abs => a.abs(),
                Builtin::Sign => {
                    if a > 0.0 {
                        1.0
                    } else if a < 0.0 {
                        -1.0
                    } else {
                        0.0
                    }
                }
                Builtin::Min => a.min(b()),
                Builtin::Max => a.max(b()),
                Builtin::Limit => a.max(b()).min(ev(&args[2])),
            }
        }
        Expr::Compare(op, a, b) => {
            let (a, b) = (ev(a), ev(b));
            truth(match op {
                CmpOp::Lt => a < b,
                CmpOp::Le => a <= b,
                CmpOp::Gt => a > b,
                CmpOp::Ge => a >= b,
            })
        }
        Expr::And(a, b) => truth(ev(a) != 0.0 && ev(b) != 0.0),
        Expr::Or(a, b) => truth(ev(a) != 0.0 || ev(b) != 0.0),
        Expr::Not(a) => truth(ev(a) == 0.0),
        Expr::If(c, a, b) => {
            if ev(c) != 0.0 {
                ev(a)
            } else {
                ev(b)
            }
        }
        Expr::NoEvent(a) => ev(a),
    }
}

/// An [`Env`] over plain slices: variables, derivatives, parameters.
pub struct SliceEnv<'a> {
    /// time
    pub t: f64,
    /// variable values by `VarId`
    pub vars: &'a [f64],
    /// derivative values by `VarId` (NaN where not a state)
    pub ders: &'a [f64],
    /// parameter values by `ParamId`
    pub params: &'a [f64],
}

impl Env for SliceEnv<'_> {
    fn time(&self) -> f64 {
        self.t
    }
    fn var(&self, v: VarId) -> f64 {
        self.vars[v.0 as usize]
    }
    fn der(&self, v: VarId) -> f64 {
        self.ders[v.0 as usize]
    }
    fn param(&self, p: ParamId) -> f64 {
        self.params[p.0 as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::{c, call, cmp, if_};

    #[test]
    fn evaluates_arithmetic_and_conditions() {
        let x = Expr::Var(VarId(0));
        let p = Expr::Param(ParamId(0));
        let e = if_(
            cmp(CmpOp::Ge, x.clone(), c(1.0)),
            x.clone() * p.clone() + call(Builtin::Exp, vec![c(0.0)]),
            -x.clone(),
        );
        let env = SliceEnv { t: 0.0, vars: &[2.0], ders: &[f64::NAN], params: &[3.0] };
        assert_eq!(eval(&e, &env), 7.0);
        let env = SliceEnv { t: 0.0, vars: &[0.5], ders: &[f64::NAN], params: &[3.0] };
        assert_eq!(eval(&e, &env), -0.5);
        let lim = call(Builtin::Limit, vec![x, c(-1.0), c(0.25)]);
        assert_eq!(eval(&lim, &env), 0.25);
    }
}
