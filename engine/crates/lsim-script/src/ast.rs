//! The syntax tree of the Python subset, with names resolved: a function's
//! own names are slots of its frame (Python decides at compile time which
//! names are local: those it assigns and does not declare `global`), every
//! other name is a global, looked up at run time and then among the
//! builtins, as Python does.

use crate::value::Value;
use std::collections::HashMap;
use std::rc::Rc;

/// A binary operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `*`
    Mul,
    /// `/`
    Div,
    /// `//`
    FloorDiv,
    /// `%`
    Mod,
    /// `**`
    Pow,
}

/// A comparison operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CmpOp {
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `>=`
    Ge,
    /// `==`
    Eq,
    /// `!=`
    Ne,
    /// `in`
    In,
    /// `not in`
    NotIn,
    /// `is`
    Is,
    /// `is not`
    IsNot,
}

/// A resolved name.
#[derive(Clone, Debug, PartialEq)]
pub enum Name {
    /// slot `k` of the current function's frame
    Local(usize, Rc<str>),
    /// global `k` (then the builtin of that name, if any)
    Global(usize, Rc<str>),
}

/// An expression.
#[derive(Clone, Debug)]
pub enum Expr {
    /// a literal
    Const(Value),
    /// a name
    Name(Name),
    /// `(a, b)`
    Tuple(Vec<Expr>),
    /// `[a, b]`
    List(Vec<Expr>),
    /// `{k: v}`
    Dict(Vec<(Expr, Expr)>),
    /// `-a`
    Neg(Box<Expr>),
    /// `+a`
    Pos(Box<Expr>),
    /// `not a`
    Not(Box<Expr>),
    /// `a op b`
    Bin(BinOp, Box<Expr>, Box<Expr>),
    /// `a and b`
    And(Box<Expr>, Box<Expr>),
    /// `a or b`
    Or(Box<Expr>, Box<Expr>),
    /// `a < b <= c`: the first operand and each further (op, operand)
    Compare(Box<Expr>, Vec<(CmpOp, Expr)>),
    /// `a if cond else b`: (cond, a, b)
    IfExp(Box<Expr>, Box<Expr>, Box<Expr>),
    /// `f(args, name=value)`
    Call(Box<Expr>, Vec<Expr>, Vec<(Rc<str>, Expr)>),
    /// `a.name`
    Attr(Box<Expr>, Rc<str>),
    /// `a[i]`
    Index(Box<Expr>, Box<Expr>),
}

/// What an assignment assigns to.
#[derive(Clone, Debug)]
pub enum Target {
    /// a name
    Name(Name),
    /// `a[i]`
    Index(Expr, Expr),
    /// `a, b` (unpacking)
    Tuple(Vec<Target>),
}

/// A statement, with its line for messages.
#[derive(Clone, Debug)]
pub struct Stmt {
    /// what it does
    pub kind: StmtKind,
    /// its line
    pub line: usize,
}

/// What a statement does.
#[derive(Clone, Debug)]
pub enum StmtKind {
    /// an expression evaluated for its effect
    Expr(Expr),
    /// `a = b = value`
    Assign(Vec<Target>, Expr),
    /// `a op= value`
    AugAssign(Target, BinOp, Expr),
    /// `if`/`elif` branches, then `else`
    If(Vec<(Expr, Vec<Stmt>)>, Vec<Stmt>),
    /// `while cond: body else: orelse`
    While(Expr, Vec<Stmt>, Vec<Stmt>),
    /// `for target in iter: body else: orelse`
    For(Target, Expr, Vec<Stmt>, Vec<Stmt>),
    /// `def`: binds the function (its defaults evaluated now) to a name
    Def(Name, Rc<FuncDef>, Vec<Expr>),
    /// `return value`
    Return(Option<Expr>),
    /// `pass`, `global` (a declaration only)
    Pass,
    /// `break`
    Break,
    /// `continue`
    Continue,
    /// `import math` / `from math import x as y`: (the name bound, the
    /// module attribute, or none for the module itself)
    Import(Name, Option<Rc<str>>),
}

/// A function definition.
#[derive(Debug)]
pub struct FuncDef {
    /// its name
    pub name: Rc<str>,
    /// its parameters (slots 0..n of its frame)
    pub params: Vec<Rc<str>>,
    /// the parameters' slots by name, for binding keywords, when there are
    /// more than a few (fewer are looked through in order)
    pub by_name: HashMap<Rc<str>, usize>,
    /// how many parameters have defaults (the last ones)
    pub n_defaults: usize,
    /// the size of its frame
    pub n_locals: usize,
    /// its body
    pub body: Vec<Stmt>,
}

/// A compiled script: its top-level code and its globals' names.
#[derive(Debug)]
pub struct Module {
    /// the top-level code
    pub body: Vec<Stmt>,
    /// the globals' names (index = `Name::Global`'s)
    pub globals: Vec<Rc<str>>,
}
