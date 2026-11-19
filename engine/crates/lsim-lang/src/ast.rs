//! The syntax tree both formats parse into, with spans, before names are
//! resolved and units checked.

use crate::Span;

/// Binary operators of the source (more than the IR has: `==`, `<>`,
/// `and`, `or` are lowered to IR forms).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    And,
    Or,
}

/// A function-call argument list: positional, then named.
#[derive(Clone, Debug, Default)]
pub(crate) struct Args {
    pub positional: Vec<Expr>,
    pub named: Vec<(String, Span, Expr)>,
}

#[derive(Clone, Debug)]
pub(crate) enum ExprKind {
    Num(f64),
    Str(String),
    Bool(bool),
    /// a component reference `a.b.c` (each part unquoted)
    Ref(Vec<String>),
    /// `f(args)`; `f` may be dotted
    Call(Vec<String>, Args),
    Neg(Box<Expr>),
    Not(Box<Expr>),
    Bin(BinOp, Box<Expr>, Box<Expr>),
    /// if c1 then a1 elseif c2 then a2 … else b
    If(Vec<(Expr, Expr)>, Box<Expr>),
    /// `{a, b, c}`
    Array(Vec<Expr>),
    /// `[a, b; c, d]`
    Matrix(Vec<Vec<Expr>>),
    /// `( e )`: kept so `-(2)` and `-2` stay apart
    Paren(Box<Expr>),
}

#[derive(Clone, Debug)]
pub(crate) struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

impl Expr {
    /// The expression without enclosing parentheses.
    pub fn strip(&self) -> &Expr {
        match &self.kind {
            ExprKind::Paren(e) => e.strip(),
            _ => self,
        }
    }
}

/// A modification argument: `name = value`, `name(…) = value`, with an
/// optional description.
#[derive(Clone, Debug)]
pub(crate) struct ModArg {
    pub name: Vec<String>,
    pub span: Span,
    pub mods: Vec<ModArg>,
    pub value: Option<Expr>,
}

/// Type prefixes of a declaration.
#[derive(Clone, Debug, Default)]
pub(crate) struct Prefixes {
    pub structural: bool,
    pub parameter: bool,
    pub constant: bool,
    pub discrete: bool,
    pub input: bool,
    pub output: bool,
    pub flow: bool,
}

/// A declaration inside a class.
#[derive(Clone, Debug)]
pub(crate) struct Element {
    pub span: Span,
    pub prefixes: Prefixes,
    /// `connector p: Pin` (the text format's physical port)
    pub port: bool,
    pub protected: bool,
    pub type_name: Vec<String>,
    pub type_span: Span,
    pub name: String,
    pub name_span: Span,
    pub mods: Vec<ModArg>,
    pub binding: Option<Expr>,
    pub doc: Option<String>,
    pub annotation: Vec<ModArg>,
}

#[derive(Clone, Debug)]
pub(crate) enum EqKind {
    Simple(Expr, Expr),
    Connect(Expr, Expr),
    /// `assert(…)`, `reinit(…)`, `terminate(…)`
    Call(Expr),
    If(Vec<(Expr, Vec<Equation>)>, Vec<Equation>),
    /// `when c1 then … elsewhen c2 then … end when`
    When(Vec<(Expr, Vec<Equation>)>),
}

#[derive(Clone, Debug)]
pub(crate) struct Equation {
    pub kind: EqKind,
    pub span: Span,
    pub doc: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) enum StmtKind {
    Assign(Expr, Expr),
    If(Vec<(Expr, Vec<Stmt>)>, Vec<Stmt>),
    Return,
    Call(Expr),
}

#[derive(Clone, Debug)]
pub(crate) struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClassKind {
    Model,
    Block,
    Connector,
    Record,
    Function,
    Type,
    Package,
    Class,
}

impl ClassKind {
    pub fn word(self) -> &'static str {
        match self {
            ClassKind::Model => "model",
            ClassKind::Block => "block",
            ClassKind::Connector => "connector",
            ClassKind::Record => "record",
            ClassKind::Function => "function",
            ClassKind::Type => "type",
            ClassKind::Package => "package",
            ClassKind::Class => "class",
        }
    }
}

/// `type X = …`
#[derive(Clone, Debug)]
pub(crate) enum Short {
    /// `enumeration(A "doc", B)`
    Enumeration(Vec<(String, Option<String>, Span)>),
    /// `Real(unit = "V")` or another type with modifications
    Alias { base: Vec<String>, base_span: Span, mods: Vec<ModArg> },
}

#[derive(Clone, Debug)]
pub(crate) struct ClassDef {
    pub kind: ClassKind,
    pub name: String,
    pub name_span: Span,
    pub span: Span,
    pub doc: Option<String>,
    pub elements: Vec<Element>,
    pub classes: Vec<ClassDef>,
    pub equations: Vec<Equation>,
    pub initial_equations: Vec<Equation>,
    pub algorithm: Vec<Stmt>,
    pub annotation: Vec<ModArg>,
    pub short: Option<Short>,
}
