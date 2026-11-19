//! Expressions from the syntax tree to the IR (component scope).
//!
//! The IR's operator set is Base Modelica's scalar set; the few Modelica
//! forms it lacks are written with what it has, exactly:
//!
//! * `a == b` is `a >= b and a <= b`, `a <> b` is `a < b or a > b` (the
//!   printer writes them back as `==` and `<>`);
//! * `true` and `false` are 1 and 0;
//! * `log10(x)` is `log(x) / log(10)`, `edge(b)` is `b and not pre(b)`,
//!   `change(v)` is `v <> pre(v)`, `semiLinear(x, a, b)` is
//!   `if x >= 0 then a * x else b * x`;
//! * `smooth(n, e)` is `e`, `Integer(e)` of an enumeration value is the
//!   value (enumeration options stand for their position);
//! * `homotopy(actual, simplified)` is `actual`: the engine's
//!   initialisation does its own continuation.

use crate::ast::{self, Args, BinOp, ExprKind};
use crate::{LangError, Span};
use lsim_ir::expr::{BinaryOp, Builtin, CmpOp, Expr};

/// What the context adds to the built-ins: names and calls it resolves.
pub(crate) trait LowerCx {
    /// A component reference: usually `Expr::Name`, but a context may
    /// substitute (a constant's value, a function's local).
    fn reference(&self, parts: &[String], span: Span) -> Result<Expr, LangError>;
    /// A call of a name that is not a built-in: a table read, an inlined
    /// function. `None`: not known here.
    fn call(&self, name: &[String], args: &Args, span: Span) -> Option<Result<Expr, LangError>>;
}

pub(crate) fn builtin(name: &str) -> Option<Builtin> {
    use Builtin::*;
    Some(match name {
        "der" => Der,
        "pre" => Pre,
        "sin" => Sin,
        "cos" => Cos,
        "tan" => Tan,
        "asin" => Asin,
        "acos" => Acos,
        "atan" => Atan,
        "atan2" => Atan2,
        "sinh" => Sinh,
        "cosh" => Cosh,
        "tanh" => Tanh,
        "exp" => Exp,
        "log" => Log,
        "sqrt" => Sqrt,
        "abs" => Abs,
        "sign" => Sign,
        "min" => Min,
        "max" => Max,
        "limit" => Limit,
        _ => return None,
    })
}

fn b(e: Expr) -> Box<Expr> {
    Box::new(e)
}

pub(crate) fn equal(a: Expr, c: Expr) -> Expr {
    Expr::And(
        b(Expr::Compare(CmpOp::Ge, b(a.clone()), b(c.clone()))),
        b(Expr::Compare(CmpOp::Le, b(a), b(c))),
    )
}

pub(crate) fn not_equal(a: Expr, c: Expr) -> Expr {
    Expr::Or(
        b(Expr::Compare(CmpOp::Lt, b(a.clone()), b(c.clone()))),
        b(Expr::Compare(CmpOp::Gt, b(a), b(c))),
    )
}

fn arity_error(name: &str, want: &str, got: usize, span: Span) -> LangError {
    LangError::new("CALL-ARGS", span, format!("{name}() takes {want}, but is given {got}"))
}

fn positional_only(name: &str, args: &Args, span: Span) -> Result<(), LangError> {
    if let Some((n, s, _)) = args.named.first() {
        let _ = span;
        return Err(LangError::new(
            "CALL-ARGS",
            *s,
            format!("{name}() takes no named argument '{n}'"),
        ));
    }
    Ok(())
}

/// Lowers `e` in the context `cx`.
pub(crate) fn lower(e: &ast::Expr, cx: &dyn LowerCx) -> Result<Expr, LangError> {
    let span = e.span;
    Ok(match &e.kind {
        ExprKind::Num(v) => Expr::Const(*v),
        ExprKind::Bool(v) => Expr::Const(if *v { 1.0 } else { 0.0 }),
        ExprKind::Str(_) => {
            return Err(LangError::new(
                "TEXT-VALUE",
                span,
                "text in quotes cannot be a value in an equation".into(),
            ));
        }
        ExprKind::Ref(parts) if parts.len() == 1 && parts[0] == "time" => Expr::Time,
        ExprKind::Ref(parts) => cx.reference(parts, span)?,
        ExprKind::Paren(a) => lower(a, cx)?,
        ExprKind::Neg(a) => Expr::Neg(b(lower(a, cx)?)),
        ExprKind::Not(a) => Expr::Not(b(lower(a, cx)?)),
        ExprKind::Bin(op, x, y) => {
            let (l, r) = (lower(x, cx)?, lower(y, cx)?);
            let arith = |o| Expr::Binary(o, b(l.clone()), b(r.clone()));
            let cmp = |o| Expr::Compare(o, b(l.clone()), b(r.clone()));
            match op {
                BinOp::Add => arith(BinaryOp::Add),
                BinOp::Sub => arith(BinaryOp::Sub),
                BinOp::Mul => arith(BinaryOp::Mul),
                BinOp::Div => arith(BinaryOp::Div),
                BinOp::Pow => arith(BinaryOp::Pow),
                BinOp::Lt => cmp(CmpOp::Lt),
                BinOp::Le => cmp(CmpOp::Le),
                BinOp::Gt => cmp(CmpOp::Gt),
                BinOp::Ge => cmp(CmpOp::Ge),
                BinOp::Eq => equal(l, r),
                BinOp::Ne => not_equal(l, r),
                BinOp::And => Expr::And(b(l), b(r)),
                BinOp::Or => Expr::Or(b(l), b(r)),
            }
        }
        ExprKind::If(branches, otherwise) => {
            let mut out = lower(otherwise, cx)?;
            for (c, v) in branches.iter().rev() {
                out = Expr::If(b(lower(c, cx)?), b(lower(v, cx)?), b(out));
            }
            out
        }
        ExprKind::Array(_) | ExprKind::Matrix(_) => {
            return Err(LangError::new(
                "ARRAY",
                span,
                "arrays are not supported in equations (scalars only); lists of numbers belong \
                 in a table(...)"
                    .into(),
            ));
        }
        ExprKind::Call(name, args) => lower_call(name, args, span, cx)?,
    })
}

fn lower_call(
    name: &[String],
    args: &Args,
    span: Span,
    cx: &dyn LowerCx,
) -> Result<Expr, LangError> {
    let full = name.join(".");
    let n = args.positional.len();
    let arg = |k: usize| lower(&args.positional[k], cx);
    if name.len() == 1 {
        let f = name[0].as_str();
        if let Some(bi) = builtin(f) {
            positional_only(f, args, span)?;
            if bi == Builtin::Der || bi == Builtin::Pre {
                if n != 1 {
                    return Err(arity_error(f, "one variable", n, span));
                }
                let a = arg(0)?;
                if !matches!(a, Expr::Name(_)) {
                    return Err(LangError::new(
                        "DER-ARG",
                        args.positional[0].span,
                        format!(
                            "{f}() applies to a variable's name, not to an expression; introduce \
                             a variable for the expression and an equation for it"
                        ),
                    ));
                }
                return Ok(Expr::Call(bi, vec![a]));
            }
            if n != bi.arity() {
                let want = match bi.arity() {
                    1 => "one argument".to_string(),
                    k => format!("{k} arguments"),
                };
                return Err(arity_error(f, &want, n, span));
            }
            let a: Vec<Expr> = (0..n).map(arg).collect::<Result<_, _>>()?;
            return Ok(Expr::Call(bi, a));
        }
        let one = |what: &str| -> Result<Expr, LangError> {
            positional_only(f, args, span)?;
            if n != 1 {
                return Err(arity_error(f, what, n, span));
            }
            arg(0)
        };
        match f {
            "noEvent" => return Ok(Expr::NoEvent(b(one("one argument")?))),
            "log10" => {
                return Ok(Expr::Binary(
                    BinaryOp::Div,
                    b(Expr::Call(Builtin::Log, vec![one("one argument")?])),
                    b(Expr::Const(std::f64::consts::LN_10)),
                ));
            }
            "Integer" => return one("one argument"),
            "edge" => {
                let x = one("one Boolean variable")?;
                return Ok(Expr::And(
                    b(x.clone()),
                    b(Expr::Not(b(Expr::Call(Builtin::Pre, vec![x])))),
                ));
            }
            "change" => {
                let x = one("one variable")?;
                return Ok(not_equal(x.clone(), Expr::Call(Builtin::Pre, vec![x])));
            }
            "smooth" => {
                positional_only(f, args, span)?;
                if n != 2 {
                    return Err(arity_error(
                        f,
                        "2 arguments (an order and an expression)",
                        n,
                        span,
                    ));
                }
                return arg(1);
            }
            "homotopy" => {
                let actual = args
                    .named
                    .iter()
                    .find(|(k, _, _)| k == "actual")
                    .map(|(_, _, v)| v)
                    .or(args.positional.first());
                let Some(actual) = actual else {
                    return Err(arity_error(
                        f,
                        "the actual and the simplified expression",
                        n,
                        span,
                    ));
                };
                return lower(actual, cx);
            }
            "semiLinear" => {
                positional_only(f, args, span)?;
                if n != 3 {
                    return Err(arity_error(f, "3 arguments", n, span));
                }
                let (x, pos, neg) = (arg(0)?, arg(1)?, arg(2)?);
                return Ok(Expr::If(
                    b(Expr::Compare(CmpOp::Ge, b(x.clone()), b(Expr::Const(0.0)))),
                    b(Expr::Binary(BinaryOp::Mul, b(pos), b(x.clone()))),
                    b(Expr::Binary(BinaryOp::Mul, b(neg), b(x))),
                ));
            }
            _ => {}
        }
    }
    if let Some(r) = cx.call(name, args, span) {
        return r;
    }
    let why = match full.as_str() {
        "initial" | "terminal" => "events at the start or end are not supported".to_string(),
        "sample" | "hold" | "Clock" | "subSample" | "superSample" | "shiftSample"
        | "backSample" | "previous" | "interval" => {
            "clocked (sampled) equations are not supported; use a when-clause on time".to_string()
        }
        "delay" | "spatialDistribution" => "delays are not supported".to_string(),
        "floor" | "ceil" | "integer" | "div" | "mod" | "rem" => {
            "rounding functions are not supported (their jumps would need events)".to_string()
        }
        "cardinality" | "isPresent" => "this function is not supported".to_string(),
        "String" => "text values are only allowed in assert messages".to_string(),
        "sum" | "product" | "size" | "ndims" | "fill" | "zeros" | "ones" | "linspace" | "cat"
        | "transpose" | "cross" | "identity" | "diagonal" | "scalar" | "vector" | "matrix" => {
            "array functions are not supported (scalars only)".to_string()
        }
        _ => format!(
            "it is not a built-in function{}",
            if name.len() == 1 {
                ", a table of this component or a function defined here"
            } else {
                ""
            }
        ),
    };
    Err(LangError::new("UNKNOWN-FUNCTION", span, format!("'{full}()' cannot be used: {why}")))
}
