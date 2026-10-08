//! The expression tree of equations.
//!
//! One type serves two scopes:
//!
//! * **component scope** (a [`crate::ComponentDef`]'s equations): names are
//!   text, [`Expr::Name`] (`"p.v"`, `"R"`, `"R0.n.i"`); a derivative is
//!   `Call(Builtin::Der, [Name])` and a pre-value `Call(Builtin::Pre, [Name])`;
//! * **flat scope** (after flattening): every name is resolved to
//!   [`Expr::Var`], [`Expr::Param`], [`Expr::Der`] or [`Expr::Pre`].
//!
//! The operator set maps one-to-one onto Base Modelica's scalar expressions
//! (DESIGN.md, *Equation format*), plus two engine built-ins: `limit` (a
//! saturation that fast mode turns into a flag) and table interpolation.

use crate::flat::{ParamId, VarId};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Binary arithmetic operators.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum BinaryOp {
    /// a + b
    Add,
    /// a - b
    Sub,
    /// a * b
    Mul,
    /// a / b
    Div,
    /// a ^ b
    Pow,
}

/// Relational operators. In an `if` or `when` condition a relation becomes
/// a zero-crossing function (`lhs - rhs`) and the solver stops exactly where
/// it changes, unless wrapped in `noEvent`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum CmpOp {
    /// a < b
    Lt,
    /// a <= b
    Le,
    /// a > b
    Gt,
    /// a >= b
    Ge,
}

/// Built-in functions.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum Builtin {
    /// der(x): the time derivative (component scope only)
    Der,
    /// pre(x): the value just before the current event (component scope only)
    Pre,
    /// sin
    Sin,
    /// cos
    Cos,
    /// tan
    Tan,
    /// asin
    Asin,
    /// acos
    Acos,
    /// atan
    Atan,
    /// atan2(y, x)
    Atan2,
    /// sinh
    Sinh,
    /// cosh
    Cosh,
    /// tanh
    Tanh,
    /// exp
    Exp,
    /// natural logarithm
    Log,
    /// square root
    Sqrt,
    /// absolute value (an event at 0 unless in noEvent)
    Abs,
    /// sign (an event at 0 unless in noEvent)
    Sign,
    /// min(a, b)
    Min,
    /// max(a, b)
    Max,
    /// limit(x, lo, hi): forward mode clamps; fast (inverse) mode passes x
    /// through and flags every moment it is outside [lo, hi]
    Limit,
}

impl Builtin {
    /// The name in the text format and in Base Modelica.
    pub fn name(self) -> &'static str {
        use Builtin::*;
        match self {
            Der => "der",
            Pre => "pre",
            Sin => "sin",
            Cos => "cos",
            Tan => "tan",
            Asin => "asin",
            Acos => "acos",
            Atan => "atan",
            Atan2 => "atan2",
            Sinh => "sinh",
            Cosh => "cosh",
            Tanh => "tanh",
            Exp => "exp",
            Log => "log",
            Sqrt => "sqrt",
            Abs => "abs",
            Sign => "sign",
            Min => "min",
            Max => "max",
            Limit => "limit",
        }
    }

    /// How many arguments it takes.
    pub fn arity(self) -> usize {
        use Builtin::*;
        match self {
            Atan2 | Min | Max => 2,
            Limit => 3,
            _ => 1,
        }
    }
}

/// An expression.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub enum Expr {
    /// A number (dimension adopted from context in sums and comparisons).
    Const(f64),
    /// Simulation time, s.
    Time,
    /// An unresolved name (component scope).
    Name(String),
    /// A flat variable.
    Var(VarId),
    /// A parameter: a runtime input of the compiled model.
    Param(ParamId),
    /// The derivative of a flat variable.
    Der(VarId),
    /// The value of a discrete flat variable just before the event.
    Pre(VarId),
    /// -a
    Neg(Box<Expr>),
    /// a op b
    Binary(BinaryOp, Box<Expr>, Box<Expr>),
    /// f(args)
    Call(Builtin, Vec<Expr>),
    /// a op b, a truth value
    Compare(CmpOp, Box<Expr>, Box<Expr>),
    /// a and b
    And(Box<Expr>, Box<Expr>),
    /// a or b
    Or(Box<Expr>, Box<Expr>),
    /// not a
    Not(Box<Expr>),
    /// if c then a else b
    If(Box<Expr>, Box<Expr>, Box<Expr>),
    /// noEvent(a): relations inside do not create events
    NoEvent(Box<Expr>),
    /// interpolation in table parameter `table` at the given abscissae
    /// (1 or 2), with the table's own method (monotone cubic by default)
    Table {
        /// the table's index in the flat system's tables
        table: u32,
        /// the abscissae
        args: Vec<Expr>,
    },
}

/// A name reference (component scope).
pub fn name(s: &str) -> Expr {
    Expr::Name(s.to_string())
}

/// A constant.
pub fn c(v: f64) -> Expr {
    Expr::Const(v)
}

/// der(name)
pub fn der(s: &str) -> Expr {
    Expr::Call(Builtin::Der, vec![name(s)])
}

/// A call of a built-in function.
pub fn call(f: Builtin, args: Vec<Expr>) -> Expr {
    Expr::Call(f, args)
}

/// A comparison.
pub fn cmp(op: CmpOp, a: Expr, b: Expr) -> Expr {
    Expr::Compare(op, Box::new(a), Box::new(b))
}

/// if c then a else b
pub fn if_(cond: Expr, a: Expr, b: Expr) -> Expr {
    Expr::If(Box::new(cond), Box::new(a), Box::new(b))
}

impl Expr {
    /// a op b
    pub fn bin(op: BinaryOp, a: Expr, b: Expr) -> Expr {
        Expr::Binary(op, Box::new(a), Box::new(b))
    }

    /// The direct children, in order.
    pub fn children(&self) -> Vec<&Expr> {
        match self {
            Expr::Const(_)
            | Expr::Time
            | Expr::Name(_)
            | Expr::Var(_)
            | Expr::Param(_)
            | Expr::Der(_)
            | Expr::Pre(_) => vec![],
            Expr::Neg(a) | Expr::Not(a) | Expr::NoEvent(a) => vec![a],
            Expr::Binary(_, a, b) | Expr::Compare(_, a, b) | Expr::And(a, b) | Expr::Or(a, b) => {
                vec![a, b]
            }
            Expr::Call(_, args) | Expr::Table { args, .. } => args.iter().collect(),
            Expr::If(c, a, b) => vec![c, a, b],
        }
    }

    /// Visits this expression and every sub-expression, parents first.
    pub fn walk(&self, f: &mut impl FnMut(&Expr)) {
        f(self);
        for ch in self.children() {
            ch.walk(f);
        }
    }

    /// Rebuilds the expression bottom-up, letting `f` replace any node
    /// (its children already rebuilt).
    pub fn rewrite(self, f: &mut impl FnMut(Expr) -> Expr) -> Expr {
        let rebuilt = match self {
            Expr::Neg(a) => Expr::Neg(Box::new(a.rewrite(f))),
            Expr::Not(a) => Expr::Not(Box::new(a.rewrite(f))),
            Expr::NoEvent(a) => Expr::NoEvent(Box::new(a.rewrite(f))),
            Expr::Binary(op, a, b) => {
                Expr::Binary(op, Box::new(a.rewrite(f)), Box::new(b.rewrite(f)))
            }
            Expr::Compare(op, a, b) => {
                Expr::Compare(op, Box::new(a.rewrite(f)), Box::new(b.rewrite(f)))
            }
            Expr::And(a, b) => Expr::And(Box::new(a.rewrite(f)), Box::new(b.rewrite(f))),
            Expr::Or(a, b) => Expr::Or(Box::new(a.rewrite(f)), Box::new(b.rewrite(f))),
            Expr::Call(op, args) => {
                Expr::Call(op, args.into_iter().map(|a| a.rewrite(f)).collect())
            }
            Expr::Table { table, args } => {
                Expr::Table { table, args: args.into_iter().map(|a| a.rewrite(f)).collect() }
            }
            Expr::If(c, a, b) => {
                Expr::If(Box::new(c.rewrite(f)), Box::new(a.rewrite(f)), Box::new(b.rewrite(f)))
            }
            leaf => leaf,
        };
        f(rebuilt)
    }

    /// Whether `pred` holds for any node.
    pub fn any(&self, pred: &mut impl FnMut(&Expr) -> bool) -> bool {
        if pred(self) {
            return true;
        }
        self.children().into_iter().any(|c| c.any(pred))
    }

    /// The number of nodes (a size measure for codegen budgets).
    pub fn size(&self) -> usize {
        let mut n = 0;
        self.walk(&mut |_| n += 1);
        n
    }
}

macro_rules! impl_op {
    ($tr:ident, $f:ident, $op:expr) => {
        impl std::ops::$tr for Expr {
            type Output = Expr;
            fn $f(self, rhs: Expr) -> Expr {
                Expr::bin($op, self, rhs)
            }
        }
        impl std::ops::$tr<f64> for Expr {
            type Output = Expr;
            fn $f(self, rhs: f64) -> Expr {
                Expr::bin($op, self, Expr::Const(rhs))
            }
        }
        impl std::ops::$tr<Expr> for f64 {
            type Output = Expr;
            fn $f(self, rhs: Expr) -> Expr {
                Expr::bin($op, Expr::Const(self), rhs)
            }
        }
    };
}
impl_op!(Add, add, BinaryOp::Add);
impl_op!(Sub, sub, BinaryOp::Sub);
impl_op!(Mul, mul, BinaryOp::Mul);
impl_op!(Div, div, BinaryOp::Div);

impl std::ops::Neg for Expr {
    type Output = Expr;
    fn neg(self) -> Expr {
        Expr::Neg(Box::new(self))
    }
}

fn prec(e: &Expr) -> u8 {
    match e {
        Expr::If(..) => 0,
        Expr::Or(..) => 1,
        Expr::And(..) => 2,
        Expr::Not(..) => 3,
        Expr::Compare(..) => 4,
        Expr::Binary(BinaryOp::Add | BinaryOp::Sub, ..) => 5,
        Expr::Binary(BinaryOp::Mul | BinaryOp::Div, ..) | Expr::Neg(_) => 6,
        Expr::Binary(BinaryOp::Pow, ..) => 7,
        _ => 8,
    }
}

struct Paren<'a>(&'a Expr, u8);

impl fmt::Display for Paren<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if prec(self.0) < self.1 { write!(f, "({})", self.0) } else { write!(f, "{}", self.0) }
    }
}

/// Prints in the text format's (Base Modelica's) syntax; flat references
/// print as `v[3]`, `p[1]`, `der(v[2])`.
impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let p = prec(self);
        match self {
            Expr::Const(v) => write!(f, "{v}"),
            Expr::Time => write!(f, "time"),
            Expr::Name(n) => write!(f, "{n}"),
            Expr::Var(v) => write!(f, "v[{}]", v.0),
            Expr::Param(q) => write!(f, "p[{}]", q.0),
            Expr::Der(v) => write!(f, "der(v[{}])", v.0),
            Expr::Pre(v) => write!(f, "pre(v[{}])", v.0),
            Expr::Neg(a) => write!(f, "-{}", Paren(a, p + 1)),
            Expr::Binary(op, a, b) => {
                let s = match op {
                    BinaryOp::Add => "+",
                    BinaryOp::Sub => "-",
                    BinaryOp::Mul => "*",
                    BinaryOp::Div => "/",
                    BinaryOp::Pow => "^",
                };
                write!(f, "{} {s} {}", Paren(a, p), Paren(b, p + 1))
            }
            Expr::Call(fun, args) => {
                write!(f, "{}(", fun.name())?;
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{a}")?;
                }
                write!(f, ")")
            }
            Expr::Compare(op, a, b) => {
                let s = match op {
                    CmpOp::Lt => "<",
                    CmpOp::Le => "<=",
                    CmpOp::Gt => ">",
                    CmpOp::Ge => ">=",
                };
                write!(f, "{} {s} {}", Paren(a, p + 1), Paren(b, p + 1))
            }
            Expr::And(a, b) => write!(f, "{} and {}", Paren(a, p), Paren(b, p + 1)),
            Expr::Or(a, b) => write!(f, "{} or {}", Paren(a, p), Paren(b, p + 1)),
            Expr::Not(a) => write!(f, "not {}", Paren(a, p + 1)),
            Expr::If(c, a, b) => write!(f, "if {c} then {a} else {b}"),
            Expr::NoEvent(a) => write!(f, "noEvent({a})"),
            Expr::Table { table, args } => {
                write!(f, "table{table}(")?;
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{a}")?;
                }
                write!(f, ")")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prints_with_minimal_parentheses() {
        let e = (name("a") + name("b")) * name("c") - c(2.0) / (name("x") - name("y"));
        assert_eq!(e.to_string(), "(a + b) * c - 2 / (x - y)");
        let d = der("w") * name("J");
        assert_eq!(d.to_string(), "der(w) * J");
        let n = -(name("a") + name("b"));
        assert_eq!(n.to_string(), "-(a + b)");
    }

    #[test]
    fn rewrite_replaces_names() {
        let e = name("a") * (name("a") + c(1.0));
        let r = e.rewrite(&mut |x| match x {
            Expr::Name(n) if n == "a" => c(2.0),
            other => other,
        });
        assert_eq!(r.to_string(), "2 * (2 + 1)");
        assert_eq!(r.size(), 5);
    }
}
