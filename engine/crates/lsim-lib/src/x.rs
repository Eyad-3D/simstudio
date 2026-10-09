//! Short builders for component definitions: expressions (`ite`, `max`,
//! `limit` …), ports, parameters with display units, `when` clauses.
//!
//! Every helper builds the plain IR types of `lsim-ir`; nothing here is
//! more than shorthand, so a definition written with them prints and
//! flattens exactly as one written out in full.

use lsim_ir::component::build::param;
use lsim_ir::expr::{Builtin, CmpOp, Expr, call, cmp};
use lsim_ir::{Equation, EquationDecl, ParamDecl, ParamValue, PortDecl, PortKind, WhenAction};

pub use lsim_ir::component::build::{connect, discrete, eq, port, state, sub, var};
pub use lsim_ir::expr::{c, der, name as n};

/// if `cond` then `a` else `b`
pub fn ite(cond: Expr, a: Expr, b: Expr) -> Expr {
    Expr::If(Box::new(cond), Box::new(a), Box::new(b))
}
/// a > b
pub fn gt(a: Expr, b: Expr) -> Expr {
    cmp(CmpOp::Gt, a, b)
}
/// a >= b
pub fn ge(a: Expr, b: Expr) -> Expr {
    cmp(CmpOp::Ge, a, b)
}
/// a < b
pub fn lt(a: Expr, b: Expr) -> Expr {
    cmp(CmpOp::Lt, a, b)
}
/// a <= b
pub fn le(a: Expr, b: Expr) -> Expr {
    cmp(CmpOp::Le, a, b)
}
/// a and b
pub fn and(a: Expr, b: Expr) -> Expr {
    Expr::And(Box::new(a), Box::new(b))
}
/// a or b
pub fn or(a: Expr, b: Expr) -> Expr {
    Expr::Or(Box::new(a), Box::new(b))
}
/// not a
pub fn not(a: Expr) -> Expr {
    Expr::Not(Box::new(a))
}
/// noEvent(a): its relations do not create events
pub fn noev(a: Expr) -> Expr {
    Expr::NoEvent(Box::new(a))
}
/// max(a, b)
pub fn max(a: Expr, b: Expr) -> Expr {
    call(Builtin::Max, vec![a, b])
}
/// min(a, b)
pub fn min(a: Expr, b: Expr) -> Expr {
    call(Builtin::Min, vec![a, b])
}
/// min(max(x, lo), hi), as plain arithmetic: a saturation that is part of
/// the physics (fast mode does not flag it, unlike [`limit`])
pub fn clamp(x: Expr, lo: Expr, hi: Expr) -> Expr {
    min(max(x, lo), hi)
}
/// limit(x, lo, hi): a limit the model enforces in full dynamic mode and
/// fast mode flags
pub fn limit(x: Expr, lo: Expr, hi: Expr) -> Expr {
    call(Builtin::Limit, vec![x, lo, hi])
}
/// |a|
pub fn abs(a: Expr) -> Expr {
    call(Builtin::Abs, vec![a])
}
/// sign(a)
pub fn sign(a: Expr) -> Expr {
    call(Builtin::Sign, vec![a])
}
/// sqrt(a)
pub fn sqrt(a: Expr) -> Expr {
    call(Builtin::Sqrt, vec![a])
}
/// exp(a)
pub fn exp(a: Expr) -> Expr {
    call(Builtin::Exp, vec![a])
}
/// atan(a)
pub fn atan(a: Expr) -> Expr {
    call(Builtin::Atan, vec![a])
}
/// sin(a)
pub fn sin(a: Expr) -> Expr {
    call(Builtin::Sin, vec![a])
}
/// cos(a)
pub fn cos(a: Expr) -> Expr {
    call(Builtin::Cos, vec![a])
}
/// pre(name): a discrete variable's value just before the event
pub fn pre(s: &str) -> Expr {
    call(Builtin::Pre, vec![n(s)])
}
/// a ^ k
pub fn pow(a: Expr, k: f64) -> Expr {
    Expr::bin(lsim_ir::BinaryOp::Pow, a, c(k))
}
/// Simulation time.
pub fn time() -> Expr {
    Expr::Time
}

/// A parameter with a display unit (the unit the app shows and takes it in).
pub fn pd(name: &str, unit: &str, display: &str, default: f64, doc: &str) -> ParamDecl {
    ParamDecl { display_unit: Some(display.into()), ..param(name, unit, default, doc) }
}
/// A parameter (SI unit, no separate display unit).
pub fn p(name: &str, unit: &str, default: f64, doc: &str) -> ParamDecl {
    param(name, unit, default, doc)
}
/// A structural parameter (today's `variability: fixed`).
pub fn ps(name: &str, unit: &str, display: Option<&str>, default: f64, doc: &str) -> ParamDecl {
    ParamDecl {
        display_unit: display.map(str::to_string),
        structural: true,
        ..param(name, unit, default, doc)
    }
}
/// A parameter whose default is an expression of the same scope's parameters.
pub fn pe(name: &str, unit: &str, default: Expr, doc: &str) -> ParamDecl {
    ParamDecl { default: ParamValue::Real(default), ..param(name, unit, 0.0, doc) }
}
/// A signal input.
pub fn input(name: &str, unit: &str, doc: &str) -> PortDecl {
    PortDecl { name: name.into(), kind: PortKind::Input { unit: unit.into() }, doc: doc.into() }
}
/// A signal output.
pub fn output(name: &str, unit: &str, doc: &str) -> PortDecl {
    PortDecl { name: name.into(), kind: PortKind::Output { unit: unit.into() }, doc: doc.into() }
}

/// when `cond` then `assignments`.
pub fn when(cond: Expr, actions: &[(&str, Expr)], label: &str) -> EquationDecl {
    EquationDecl {
        eq: Equation::When {
            condition: cond,
            actions: actions
                .iter()
                .map(|(v, e)| WhenAction::Assign { var: (*v).into(), value: e.clone() })
                .collect(),
        },
        label: Some(label.into()),
    }
}

/// An assertion: `cond` must hold; `error` stops the run, otherwise it warns.
pub fn assert_that(cond: Expr, message: &str, error: bool) -> EquationDecl {
    EquationDecl {
        eq: Equation::Assert { condition: cond, message: message.into(), error },
        label: Some(message.into()),
    }
}

/// A sum of expressions (0 when empty).
pub fn sum(terms: impl IntoIterator<Item = Expr>) -> Expr {
    let mut it = terms.into_iter();
    match it.next() {
        None => c(0.0),
        Some(first) => it.fold(first, |acc, t| acc + t),
    }
}

/// An identifier-safe version of any text (letters, digits, `_`).
pub fn ident(text: &str) -> String {
    let mut s: String =
        text.chars().map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' }).collect();
    if s.chars().next().is_none_or(|ch| ch.is_ascii_digit()) {
        s.insert(0, '_');
    }
    s
}
