//! A recursive-descent parser for the Python subset, with Python's grammar
//! and operator precedence, and the resolution of names into frame slots
//! and globals. Anything outside the subset is refused with the line and
//! what it is ([`Unsupported`]): such a script runs on Python instead.

use crate::Unsupported;
use crate::ast::{BinOp, CmpOp, Expr, FuncDef, Module, Name, Stmt, StmtKind, Target};
use crate::lexer::{Tok, Token, tokens};
use crate::value::Value;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// How deeply expressions and blocks may nest (a recursive parser must not
/// run out of stack on hostile input; the stack it uses is measured too,
/// against [`crate::STACK_BUDGET`]).
const MAX_DEPTH: usize = 100;

/// The attributes a script may read: the math module's (the functions
/// whose results are exact, or correctly rounded alike in every C runtime
/// Python is built with) and the methods of dicts and lists.
pub const MATH_ATTRS: [&str; 16] = [
    "pi", "e", "tau", "inf", "nan", "sqrt", "fabs", "floor", "ceil", "trunc", "copysign", "isnan",
    "isinf", "isfinite", "radians", "degrees",
];
const METHODS: [&str; 15] = [
    "get",
    "update",
    "keys",
    "values",
    "items",
    "setdefault",
    "pop",
    "copy",
    "clear",
    "append",
    "extend",
    "insert",
    "index",
    "count",
    "remove",
];

fn unsupported(line: usize, what: impl Into<String>) -> Unsupported {
    Unsupported { line, what: what.into() }
}

fn raw(name: &str) -> Name {
    Name::Global(usize::MAX, Rc::from(name))
}

/// A set of names.
type Names = HashSet<Rc<str>>;

struct Parser {
    toks: Vec<Token>,
    pos: usize,
    depth: usize,
    /// inside a function: the names it assigns, and those it declares global
    func: Option<(Names, Names)>,
    /// inside a function: every name it has read or written so far
    seen: Names,
    /// loops around the current statement (in this function)
    loops: usize,
    /// where on the stack parsing started
    stack_base: usize,
}

type R<T> = Result<T, Unsupported>;

impl Parser {
    fn peek(&self) -> &Tok {
        &self.toks[self.pos].tok
    }

    fn line(&self) -> usize {
        self.toks[self.pos].line
    }

    fn next(&mut self) -> Tok {
        let t = self.toks[self.pos].tok.clone();
        if self.pos + 1 < self.toks.len() {
            self.pos += 1;
        }
        t
    }

    fn is_op(&self, op: &str) -> bool {
        matches!(self.peek(), Tok::Op(o) if *o == op)
    }

    fn is_kw(&self, kw: &str) -> bool {
        matches!(self.peek(), Tok::Name(n) if n == kw)
    }

    fn eat_op(&mut self, op: &str) -> bool {
        if self.is_op(op) {
            self.next();
            true
        } else {
            false
        }
    }

    fn eat_kw(&mut self, kw: &str) -> bool {
        if self.is_kw(kw) {
            self.next();
            true
        } else {
            false
        }
    }

    fn expect_op(&mut self, op: &str) -> R<()> {
        if self.eat_op(op) {
            Ok(())
        } else {
            Err(unsupported(
                self.line(),
                format!("a syntax this runner does not read (expected '{op}')"),
            ))
        }
    }

    fn expect_newline(&mut self) -> R<()> {
        match self.peek() {
            Tok::Newline => {
                self.next();
                Ok(())
            }
            Tok::End => Ok(()),
            _ => Err(unsupported(self.line(), "a syntax this runner does not read")),
        }
    }

    fn name(&mut self) -> R<Rc<str>> {
        match self.next() {
            Tok::Name(n) if !is_keyword(&n) => Ok(Rc::from(n.as_str())),
            _ => Err(unsupported(
                self.line(),
                "a syntax this runner does not read (expected a name)",
            )),
        }
    }

    fn enter(&mut self) -> R<()> {
        self.depth += 1;
        if self.depth > MAX_DEPTH
            || self.stack_base.abs_diff(crate::value::stack_here()) > crate::STACK_BUDGET
        {
            return Err(unsupported(self.line(), "code nested this deeply"));
        }
        Ok(())
    }

    fn leave(&mut self) {
        self.depth -= 1;
    }

    fn assigned(&mut self, n: &Rc<str>) {
        if let Some((a, _)) = &mut self.func {
            a.insert(n.clone());
        }
    }

    fn target_names(&mut self, t: &Target) {
        match t {
            Target::Name(Name::Global(_, n) | Name::Local(_, n)) => {
                let n = n.clone();
                self.assigned(&n)
            }
            Target::Index(..) => {}
            Target::Tuple(ts) => {
                for t in ts {
                    self.target_names(t);
                }
            }
        }
    }

    // ---- statements

    fn block(&mut self) -> R<Vec<Stmt>> {
        self.enter()?;
        let mut out = if matches!(self.peek(), Tok::Newline) {
            self.next();
            if !matches!(self.peek(), Tok::Indent) {
                return Err(unsupported(self.line(), "a block without an indented body"));
            }
            self.next();
            let mut body = vec![];
            while !matches!(self.peek(), Tok::Dedent | Tok::End) {
                body.extend(self.statement()?);
            }
            if matches!(self.peek(), Tok::Dedent) {
                self.next();
            }
            body
        } else {
            self.simple_line()?
        };
        // (the tree is held all the run: no spare room in it)
        out.shrink_to_fit();
        self.leave();
        Ok(out)
    }

    /// A loop's body (Python allows 20 loops nested in one function).
    fn loop_body(&mut self) -> R<Vec<Stmt>> {
        if self.loops >= 19 {
            return Err(unsupported(self.line(), "loops nested this deeply"));
        }
        self.loops += 1;
        let b = self.block();
        self.loops -= 1;
        b
    }

    fn statement(&mut self) -> R<Vec<Stmt>> {
        let line = self.line();
        let Tok::Name(word) = self.peek().clone() else {
            return self.simple_line();
        };
        let kind = match word.as_str() {
            "if" => {
                self.next();
                let mut branches = vec![];
                let cond = self.test()?;
                self.expect_op(":")?;
                branches.push((cond, self.block()?));
                let mut orelse = vec![];
                loop {
                    if self.eat_kw("elif") {
                        let c = self.test()?;
                        self.expect_op(":")?;
                        branches.push((c, self.block()?));
                    } else if self.eat_kw("else") {
                        self.expect_op(":")?;
                        orelse = self.block()?;
                        break;
                    } else {
                        break;
                    }
                }
                branches.shrink_to_fit();
                StmtKind::If(branches, orelse)
            }
            "while" => {
                self.next();
                let cond = self.test()?;
                self.expect_op(":")?;
                let body = self.loop_body()?;
                let orelse = if self.eat_kw("else") {
                    self.expect_op(":")?;
                    self.block()?
                } else {
                    vec![]
                };
                StmtKind::While(cond, body, orelse)
            }
            "for" => {
                self.next();
                let target = self.target_list()?;
                self.target_names(&target);
                if !self.eat_kw("in") {
                    return Err(unsupported(line, "a 'for' without 'in'"));
                }
                let iter = self.test_list()?;
                self.expect_op(":")?;
                let body = self.loop_body()?;
                let orelse = if self.eat_kw("else") {
                    self.expect_op(":")?;
                    self.block()?
                } else {
                    vec![]
                };
                StmtKind::For(target, iter, body, orelse)
            }
            "def" => {
                self.next();
                if self.func.is_some() {
                    return Err(unsupported(line, "a function defined inside a function"));
                }
                let name = self.name()?;
                self.expect_op("(")?;
                let mut params: Vec<Rc<str>> = vec![];
                let mut defaults = vec![];
                while !self.is_op(")") {
                    if self.is_op("*") || self.is_op("**") || self.is_op("/") {
                        return Err(unsupported(line, "*args, **kwargs and positional-only marks"));
                    }
                    let p = self.name()?;
                    if self.is_op(":") {
                        return Err(unsupported(line, "annotations"));
                    }
                    if self.eat_op("=") {
                        defaults.push(self.test()?);
                    } else if !defaults.is_empty() {
                        return Err(unsupported(
                            line,
                            "a parameter without a default after one with",
                        ));
                    }
                    if params.contains(&p) {
                        return Err(unsupported(line, "a parameter named twice"));
                    }
                    params.push(p);
                    if !self.eat_op(",") {
                        break;
                    }
                }
                self.expect_op(")")?;
                if self.is_op("->") {
                    return Err(unsupported(line, "annotations"));
                }
                self.expect_op(":")?;
                self.func = Some((params.iter().cloned().collect(), HashSet::new()));
                self.seen = params.iter().cloned().collect();
                let loops = std::mem::replace(&mut self.loops, 0);
                let body = self.block();
                self.loops = loops;
                self.seen.clear();
                let (assigned, globals) = self.func.take().expect("inside the function");
                let mut body = body?;
                // frame slots: the parameters, then the other local names in
                // a fixed order
                let mut slots: HashMap<Rc<str>, usize> = HashMap::new();
                for (k, p) in params.iter().enumerate() {
                    if globals.contains(p) {
                        return Err(unsupported(line, "a parameter declared global"));
                    }
                    slots.insert(p.clone(), k);
                }
                let mut rest: Vec<&Rc<str>> = assigned
                    .iter()
                    .filter(|n| !slots.contains_key(*n) && !globals.contains(*n))
                    .collect();
                rest.sort();
                for n in rest {
                    let k = slots.len();
                    slots.insert(n.clone(), k);
                }
                for s in &mut body {
                    resolve_stmt(s, Some(&slots));
                }
                let def = FuncDef {
                    name: name.clone(),
                    n_defaults: defaults.len(),
                    params,
                    n_locals: slots.len(),
                    body,
                };
                StmtKind::Def(raw(&name), Rc::new(def), defaults)
            }
            "class" | "try" | "with" | "async" => {
                return Err(unsupported(line, format!("'{word}' statements")));
            }
            _ => return self.simple_line(),
        };
        if let StmtKind::Def(Name::Global(_, n), ..) = &kind {
            let n = n.clone();
            self.assigned(&n);
        }
        Ok(vec![Stmt { kind, line }])
    }

    /// Small statements separated by `;`, to the end of the line.
    fn simple_line(&mut self) -> R<Vec<Stmt>> {
        let mut out = vec![];
        loop {
            out.push(self.small()?);
            if !self.eat_op(";") {
                break;
            }
            if matches!(self.peek(), Tok::Newline | Tok::End) {
                break;
            }
        }
        self.expect_newline()?;
        Ok(out)
    }

    fn small(&mut self) -> R<Stmt> {
        let line = self.line();
        let kind = if self.eat_kw("pass") {
            StmtKind::Pass
        } else if self.eat_kw("break") {
            if self.loops == 0 {
                return Err(unsupported(line, "'break' outside a loop"));
            }
            StmtKind::Break
        } else if self.eat_kw("continue") {
            if self.loops == 0 {
                return Err(unsupported(line, "'continue' outside a loop"));
            }
            StmtKind::Continue
        } else if self.eat_kw("return") {
            if self.func.is_none() {
                return Err(unsupported(line, "'return' outside a function"));
            }
            if matches!(self.peek(), Tok::Newline | Tok::End) || self.is_op(";") {
                StmtKind::Return(None)
            } else {
                StmtKind::Return(Some(self.test_list()?))
            }
        } else if self.eat_kw("global") {
            loop {
                let n = self.name()?;
                if let Some((assigned, globals)) = &mut self.func {
                    if assigned.contains(&n) || self.seen.contains(&n) {
                        return Err(unsupported(
                            line,
                            "a name used before its 'global' declaration",
                        ));
                    }
                    globals.insert(n);
                }
                if !self.eat_op(",") {
                    break;
                }
            }
            StmtKind::Pass
        } else if self.eat_kw("import") {
            let module = self.name()?;
            if &*module != "math" {
                return Err(unsupported(line, format!("importing '{module}'")));
            }
            let bound = if self.eat_kw("as") { self.name()? } else { module };
            if self.is_op(",") || self.is_op(".") {
                return Err(unsupported(line, "importing more than math"));
            }
            self.assigned(&bound);
            StmtKind::Import(raw(&bound), None)
        } else if self.eat_kw("from") {
            let module = self.name()?;
            if &*module != "math" || !self.eat_kw("import") {
                return Err(unsupported(line, format!("importing from '{module}'")));
            }
            let what = self.name()?;
            if !MATH_ATTRS.contains(&&*what) {
                return Err(unsupported(line, format!("math.{what}")));
            }
            let bound = if self.eat_kw("as") { self.name()? } else { what.clone() };
            if self.is_op(",") {
                return Err(unsupported(line, "importing several names on one line"));
            }
            self.assigned(&bound);
            StmtKind::Import(raw(&bound), Some(what))
        } else if let Tok::Name(w) = self.peek()
            && matches!(
                w.as_str(),
                "del"
                    | "raise"
                    | "assert"
                    | "lambda"
                    | "yield"
                    | "await"
                    | "nonlocal"
                    | "class"
                    | "try"
                    | "with"
                    | "async"
                    | "if"
                    | "while"
                    | "for"
                    | "def"
                    | "else"
                    | "elif"
                    | "except"
                    | "finally"
            )
        {
            return Err(unsupported(line, format!("'{w}'")));
        } else {
            let first = self.test_list_star()?;
            if let Tok::Op(op) = self.peek().clone()
                && let Some(bop) = aug_op(op)
            {
                self.next();
                let target = self.to_target(first, line)?;
                if matches!(target, Target::Tuple(_)) {
                    return Err(unsupported(line, "an augmented assignment to several names"));
                }
                self.target_names(&target);
                let value = self.test_list()?;
                StmtKind::AugAssign(target, bop, value)
            } else if self.is_op("=") {
                let mut targets = vec![self.to_target(first, line)?];
                let mut value;
                loop {
                    self.expect_op("=")?;
                    value = self.test_list_star()?;
                    if !self.is_op("=") {
                        break;
                    }
                    targets.push(self.to_target(value, line)?);
                }
                for t in &targets {
                    self.target_names(t);
                }
                StmtKind::Assign(targets, value)
            } else if self.is_op(":") {
                return Err(unsupported(line, "annotations"));
            } else {
                StmtKind::Expr(first)
            }
        };
        Ok(Stmt { kind, line })
    }

    fn to_target(&self, e: Expr, line: usize) -> R<Target> {
        Ok(match e {
            Expr::Name(n) => Target::Name(n),
            Expr::Index(a, i) => Target::Index(*a, *i),
            Expr::Tuple(xs) | Expr::List(xs) => Target::Tuple(
                xs.into_iter().map(|x| self.to_target(x, line)).collect::<R<Vec<_>>>()?,
            ),
            Expr::Attr(..) => return Err(unsupported(line, "assigning to an attribute")),
            _ => return Err(unsupported(line, "assigning to an expression")),
        })
    }

    fn target_list(&mut self) -> R<Target> {
        let line = self.line();
        let mut xs = vec![self.or_expr_target()?];
        let mut tuple = false;
        while self.eat_op(",") {
            tuple = true;
            if self.is_kw("in") {
                break;
            }
            xs.push(self.or_expr_target()?);
        }
        let e = if tuple { Expr::Tuple(xs) } else { xs.pop().expect("one") };
        self.to_target(e, line)
    }

    fn or_expr_target(&mut self) -> R<Expr> {
        // a target is a primary (name, subscript) or a bracketed list of them
        self.arith()
    }

    // ---- expressions

    /// `a, b, c` (a tuple when there is a comma)
    fn test_list(&mut self) -> R<Expr> {
        let first = self.test()?;
        if !self.is_op(",") {
            return Ok(first);
        }
        let mut xs = vec![first];
        while self.eat_op(",") {
            if self.ends_list() {
                break;
            }
            xs.push(self.test()?);
        }
        Ok(Expr::Tuple(xs))
    }

    fn test_list_star(&mut self) -> R<Expr> {
        if self.is_op("*") {
            return Err(unsupported(self.line(), "starred expressions"));
        }
        self.test_list()
    }

    fn ends_list(&self) -> bool {
        matches!(self.peek(), Tok::Newline | Tok::End)
            || self.is_op("=")
            || self.is_op(")")
            || self.is_op("]")
            || self.is_op("}")
            || self.is_op(":")
            || self.is_op(";")
            || matches!(self.peek(), Tok::Op(o) if aug_op(o).is_some())
    }

    fn test(&mut self) -> R<Expr> {
        self.enter()?;
        if self.is_kw("lambda") {
            return Err(unsupported(self.line(), "lambda"));
        }
        let a = self.or_test()?;
        let out = if self.eat_kw("if") {
            let cond = self.or_test()?;
            if !self.eat_kw("else") {
                return Err(unsupported(self.line(), "a conditional expression without 'else'"));
            }
            let b = self.test()?;
            Expr::IfExp(Box::new(cond), Box::new(a), Box::new(b))
        } else {
            a
        };
        if self.is_op(":=") {
            return Err(unsupported(self.line(), "assignment expressions"));
        }
        self.leave();
        Ok(out)
    }

    /// Leaves `n` levels entered for the links of a chain (`a + b + c`
    /// nests to the left as deep as it is long).
    fn leave_n(&mut self, n: usize) {
        self.depth -= n;
    }

    fn or_test(&mut self) -> R<Expr> {
        let mut a = self.and_test()?;
        let mut n = 0;
        while self.eat_kw("or") {
            self.enter()?;
            n += 1;
            let b = self.and_test()?;
            a = Expr::Or(Box::new(a), Box::new(b));
        }
        self.leave_n(n);
        Ok(a)
    }

    fn and_test(&mut self) -> R<Expr> {
        let mut a = self.not_test()?;
        let mut n = 0;
        while self.eat_kw("and") {
            self.enter()?;
            n += 1;
            let b = self.not_test()?;
            a = Expr::And(Box::new(a), Box::new(b));
        }
        self.leave_n(n);
        Ok(a)
    }

    fn not_test(&mut self) -> R<Expr> {
        if self.eat_kw("not") {
            self.enter()?;
            let a = self.not_test()?;
            self.leave();
            return Ok(Expr::Not(Box::new(a)));
        }
        self.comparison()
    }

    fn comp_op(&mut self) -> Option<CmpOp> {
        let op = match self.peek() {
            Tok::Op("<") => CmpOp::Lt,
            Tok::Op("<=") => CmpOp::Le,
            Tok::Op(">") => CmpOp::Gt,
            Tok::Op(">=") => CmpOp::Ge,
            Tok::Op("==") => CmpOp::Eq,
            Tok::Op("!=") => CmpOp::Ne,
            Tok::Name(n) if n == "in" => CmpOp::In,
            Tok::Name(n) if n == "is" => {
                self.next();
                return Some(if self.eat_kw("not") { CmpOp::IsNot } else { CmpOp::Is });
            }
            Tok::Name(n)
                if n == "not"
                    && matches!(self.toks.get(self.pos + 1).map(|t| &t.tok), Some(Tok::Name(m)) if m == "in") =>
            {
                self.next();
                CmpOp::NotIn
            }
            _ => return None,
        };
        self.next();
        Some(op)
    }

    fn comparison(&mut self) -> R<Expr> {
        let a = self.bit_or()?;
        let mut rest = vec![];
        while let Some(op) = self.comp_op() {
            if rest.len() >= MAX_DEPTH {
                return Err(unsupported(self.line(), "a comparison chain this long"));
            }
            rest.push((op, self.bit_or()?));
        }
        Ok(if rest.is_empty() { a } else { Expr::Compare(Box::new(a), rest) })
    }

    fn bit_or(&mut self) -> R<Expr> {
        let a = self.arith()?;
        for op in ["|", "^", "&", "<<", ">>", "@"] {
            if self.is_op(op) {
                return Err(unsupported(self.line(), format!("the operator '{op}'")));
            }
        }
        Ok(a)
    }

    fn arith(&mut self) -> R<Expr> {
        let mut a = self.term()?;
        let mut n = 0;
        loop {
            let op = if self.eat_op("+") {
                BinOp::Add
            } else if self.eat_op("-") {
                BinOp::Sub
            } else {
                break;
            };
            self.enter()?;
            n += 1;
            let b = self.term()?;
            a = Expr::Bin(op, Box::new(a), Box::new(b));
        }
        self.leave_n(n);
        Ok(a)
    }

    fn term(&mut self) -> R<Expr> {
        let mut a = self.factor()?;
        let mut n = 0;
        loop {
            let op = if self.eat_op("*") {
                BinOp::Mul
            } else if self.eat_op("/") {
                BinOp::Div
            } else if self.eat_op("//") {
                BinOp::FloorDiv
            } else if self.eat_op("%") {
                BinOp::Mod
            } else {
                break;
            };
            self.enter()?;
            n += 1;
            let b = self.factor()?;
            a = Expr::Bin(op, Box::new(a), Box::new(b));
        }
        self.leave_n(n);
        Ok(a)
    }

    fn factor(&mut self) -> R<Expr> {
        self.enter()?;
        let out = if self.eat_op("-") {
            Expr::Neg(Box::new(self.factor()?))
        } else if self.eat_op("+") {
            Expr::Pos(Box::new(self.factor()?))
        } else if self.is_op("~") {
            return Err(unsupported(self.line(), "the operator '~'"));
        } else {
            self.power()?
        };
        self.leave();
        Ok(out)
    }

    fn power(&mut self) -> R<Expr> {
        let a = self.primary()?;
        if self.eat_op("**") {
            let b = self.factor()?;
            return Ok(Expr::Bin(BinOp::Pow, Box::new(a), Box::new(b)));
        }
        Ok(a)
    }

    fn primary(&mut self) -> R<Expr> {
        let mut a = self.atom()?;
        let mut links = 0;
        loop {
            let line = self.line();
            if self.is_op("(") || self.is_op("[") || self.is_op(".") {
                self.enter()?;
                links += 1;
            }
            if self.eat_op("(") {
                let mut args = vec![];
                let mut kwargs: Vec<(Rc<str>, Expr)> = vec![];
                while !self.is_op(")") {
                    if self.is_op("*") || self.is_op("**") {
                        return Err(unsupported(line, "*args and **kwargs in a call"));
                    }
                    if let Tok::Name(n) = self.peek().clone()
                        && matches!(self.toks.get(self.pos + 1).map(|t| &t.tok), Some(Tok::Op("=")))
                    {
                        if is_keyword(&n) {
                            return Err(unsupported(line, format!("'{n}' as an argument's name")));
                        }
                        self.next();
                        self.next();
                        let v = self.test()?;
                        if kwargs.iter().any(|(k, _)| **k == *n) {
                            return Err(unsupported(line, "a keyword argument given twice"));
                        }
                        kwargs.push((Rc::from(n.as_str()), v));
                    } else {
                        if !kwargs.is_empty() {
                            return Err(unsupported(
                                line,
                                "a positional argument after a keyword one",
                            ));
                        }
                        args.push(self.test()?);
                        if self.is_kw("for") {
                            return Err(unsupported(line, "generator expressions"));
                        }
                    }
                    if !self.eat_op(",") {
                        break;
                    }
                }
                self.expect_op(")")?;
                a = Expr::Call(Box::new(a), args, kwargs);
            } else if self.eat_op("[") {
                if self.is_op(":") {
                    return Err(unsupported(line, "slices"));
                }
                let i = self.test_list()?;
                if self.is_op(":") {
                    return Err(unsupported(line, "slices"));
                }
                self.expect_op("]")?;
                a = Expr::Index(Box::new(a), Box::new(i));
            } else if self.eat_op(".") {
                let n = self.name()?;
                if n.starts_with('_') {
                    return Err(unsupported(line, format!("the attribute '{n}'")));
                }
                if !MATH_ATTRS.contains(&&*n) && !METHODS.contains(&&*n) {
                    return Err(unsupported(line, format!("the attribute '.{n}'")));
                }
                a = Expr::Attr(Box::new(a), n);
            } else {
                break;
            }
        }
        self.leave_n(links);
        Ok(a)
    }

    fn atom(&mut self) -> R<Expr> {
        let line = self.line();
        match self.next() {
            Tok::Int(v) => Ok(Expr::Const(Value::Int(v))),
            Tok::Float(v) => Ok(Expr::Const(Value::Float(v))),
            Tok::Str(s) => Ok(Expr::Const(Value::string(s))),
            Tok::Name(n) => match n.as_str() {
                "True" => Ok(Expr::Const(Value::Bool(true))),
                "False" => Ok(Expr::Const(Value::Bool(false))),
                "None" => Ok(Expr::Const(Value::None)),
                w if is_keyword(w) => Err(unsupported(line, format!("'{w}' here"))),
                w if w.starts_with("__") => Err(unsupported(line, format!("the name '{w}'"))),
                w => {
                    if self.func.is_some() {
                        self.seen.insert(Rc::from(w));
                    }
                    Ok(Expr::Name(raw(w)))
                }
            },
            Tok::Op("(") => {
                if self.eat_op(")") {
                    return Ok(Expr::Tuple(vec![]));
                }
                let first = self.test()?;
                if self.is_kw("for") {
                    return Err(unsupported(line, "generator expressions"));
                }
                let out = if self.is_op(",") {
                    let mut xs = vec![first];
                    while self.eat_op(",") {
                        if self.is_op(")") {
                            break;
                        }
                        xs.push(self.test()?);
                    }
                    Expr::Tuple(xs)
                } else {
                    first
                };
                self.expect_op(")")?;
                Ok(out)
            }
            Tok::Op("[") => {
                let mut xs = vec![];
                while !self.is_op("]") {
                    xs.push(self.test()?);
                    if self.is_kw("for") {
                        return Err(unsupported(line, "list comprehensions"));
                    }
                    if !self.eat_op(",") {
                        break;
                    }
                }
                self.expect_op("]")?;
                Ok(Expr::List(xs))
            }
            Tok::Op("{") => {
                let mut items = vec![];
                while !self.is_op("}") {
                    if self.is_op("**") {
                        return Err(unsupported(line, "'**' in a dict"));
                    }
                    let k = self.test()?;
                    if !self.eat_op(":") {
                        return Err(unsupported(line, "sets"));
                    }
                    let v = self.test()?;
                    if self.is_kw("for") {
                        return Err(unsupported(line, "dict comprehensions"));
                    }
                    items.push((k, v));
                    if !self.eat_op(",") {
                        break;
                    }
                }
                self.expect_op("}")?;
                Ok(Expr::Dict(items))
            }
            Tok::Op("...") => Err(unsupported(line, "'...'")),
            _ => Err(unsupported(line, "a syntax this runner does not read")),
        }
    }
}

fn aug_op(op: &str) -> Option<BinOp> {
    Some(match op {
        "+=" => BinOp::Add,
        "-=" => BinOp::Sub,
        "*=" => BinOp::Mul,
        "/=" => BinOp::Div,
        "//=" => BinOp::FloorDiv,
        "%=" => BinOp::Mod,
        "**=" => BinOp::Pow,
        _ => return None,
    })
}

fn is_keyword(w: &str) -> bool {
    matches!(
        w,
        "False"
            | "None"
            | "True"
            | "and"
            | "as"
            | "assert"
            | "async"
            | "await"
            | "break"
            | "class"
            | "continue"
            | "def"
            | "del"
            | "elif"
            | "else"
            | "except"
            | "finally"
            | "for"
            | "from"
            | "global"
            | "if"
            | "import"
            | "in"
            | "is"
            | "lambda"
            | "nonlocal"
            | "not"
            | "or"
            | "pass"
            | "raise"
            | "return"
            | "try"
            | "while"
            | "with"
            | "yield"
    )
}

/// Interns global names while resolving.
struct Globals {
    names: Vec<Rc<str>>,
    index: HashMap<Rc<str>, usize>,
}

impl Globals {
    fn get(&mut self, n: &Rc<str>) -> usize {
        if let Some(k) = self.index.get(n) {
            return *k;
        }
        let k = self.names.len();
        self.names.push(n.clone());
        self.index.insert(n.clone(), k);
        k
    }
}

thread_local! {
    static GLOBALS: std::cell::RefCell<Option<Globals>> = const { std::cell::RefCell::new(None) };
}

fn global(n: &Rc<str>) -> usize {
    GLOBALS.with(|g| g.borrow_mut().as_mut().expect("resolving").get(n))
}

fn resolve_name(n: &mut Name, slots: Option<&HashMap<Rc<str>, usize>>) {
    if let Name::Global(k, s) = n
        && *k == usize::MAX
    {
        *n = match slots.and_then(|m| m.get(s)) {
            Some(slot) => Name::Local(*slot, s.clone()),
            None => Name::Global(global(s), s.clone()),
        };
    }
}

fn resolve_expr(e: &mut Expr, slots: Option<&HashMap<Rc<str>, usize>>) {
    match e {
        Expr::Const(_) => {}
        Expr::Name(n) => resolve_name(n, slots),
        Expr::Tuple(xs) | Expr::List(xs) => xs.iter_mut().for_each(|x| resolve_expr(x, slots)),
        Expr::Dict(kv) => {
            for (k, v) in kv {
                resolve_expr(k, slots);
                resolve_expr(v, slots);
            }
        }
        Expr::Neg(a) | Expr::Pos(a) | Expr::Not(a) | Expr::Attr(a, _) => resolve_expr(a, slots),
        Expr::Bin(_, a, b) | Expr::And(a, b) | Expr::Or(a, b) | Expr::Index(a, b) => {
            resolve_expr(a, slots);
            resolve_expr(b, slots);
        }
        Expr::Compare(a, rest) => {
            resolve_expr(a, slots);
            rest.iter_mut().for_each(|(_, x)| resolve_expr(x, slots));
        }
        Expr::IfExp(c, a, b) => {
            resolve_expr(c, slots);
            resolve_expr(a, slots);
            resolve_expr(b, slots);
        }
        Expr::Call(f, args, kwargs) => {
            resolve_expr(f, slots);
            args.iter_mut().for_each(|x| resolve_expr(x, slots));
            kwargs.iter_mut().for_each(|(_, x)| resolve_expr(x, slots));
        }
    }
}

fn resolve_target(t: &mut Target, slots: Option<&HashMap<Rc<str>, usize>>) {
    match t {
        Target::Name(n) => resolve_name(n, slots),
        Target::Index(a, i) => {
            resolve_expr(a, slots);
            resolve_expr(i, slots);
        }
        Target::Tuple(ts) => ts.iter_mut().for_each(|t| resolve_target(t, slots)),
    }
}

fn resolve_block(b: &mut [Stmt], slots: Option<&HashMap<Rc<str>, usize>>) {
    b.iter_mut().for_each(|s| resolve_stmt(s, slots));
}

fn resolve_stmt(s: &mut Stmt, slots: Option<&HashMap<Rc<str>, usize>>) {
    match &mut s.kind {
        StmtKind::Expr(e) => resolve_expr(e, slots),
        StmtKind::Assign(ts, v) => {
            ts.iter_mut().for_each(|t| resolve_target(t, slots));
            resolve_expr(v, slots);
        }
        StmtKind::AugAssign(t, _, v) => {
            resolve_target(t, slots);
            resolve_expr(v, slots);
        }
        StmtKind::If(branches, orelse) => {
            for (c, b) in branches {
                resolve_expr(c, slots);
                resolve_block(b, slots);
            }
            resolve_block(orelse, slots);
        }
        StmtKind::While(c, b, o) => {
            resolve_expr(c, slots);
            resolve_block(b, slots);
            resolve_block(o, slots);
        }
        StmtKind::For(t, it, b, o) => {
            resolve_target(t, slots);
            resolve_expr(it, slots);
            resolve_block(b, slots);
            resolve_block(o, slots);
        }
        // a function's body was resolved with its own frame; its defaults
        // are evaluated where it is defined
        StmtKind::Def(n, _, defaults) => {
            resolve_name(n, slots);
            defaults.iter_mut().for_each(|d| resolve_expr(d, slots));
        }
        StmtKind::Return(Some(e)) => resolve_expr(e, slots),
        StmtKind::Import(n, _) => resolve_name(n, slots),
        StmtKind::Return(None) | StmtKind::Pass | StmtKind::Break | StmtKind::Continue => {}
    }
}

/// Parses and resolves a script.
pub fn parse(src: &str) -> Result<Module, Unsupported> {
    let toks = tokens(src)?;
    GLOBALS.with(|g| *g.borrow_mut() = Some(Globals { names: vec![], index: HashMap::new() }));
    let result = (|| {
        let mut p = Parser {
            toks,
            pos: 0,
            depth: 0,
            func: None,
            seen: Names::new(),
            loops: 0,
            stack_base: crate::value::stack_here(),
        };
        let mut body = vec![];
        while !matches!(p.peek(), Tok::End) {
            if matches!(p.peek(), Tok::Indent) {
                return Err(unsupported(p.line(), "an unexpected indent"));
            }
            if matches!(p.peek(), Tok::Dedent | Tok::Newline) {
                p.next();
                continue;
            }
            body.extend(p.statement()?);
        }
        body.shrink_to_fit();
        resolve_block(&mut body, None);
        Ok(body)
    })();
    let globals = GLOBALS.with(|g| g.borrow_mut().take()).expect("set above");
    Ok(Module { body: result?, globals: globals.names })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_subset() {
        let m = parse(
            "A, B = 1.0, 2\nT = {1: 2, 'x': (3, 4)}\n\ndef step(t, dt=0.1):\n    global A\n    x = A if t > 0 else -B\n    A += 1\n    return {'y': x * 2 // 3 % 4, 'z': not (1 < x <= 3) and x in T or x is None}\n",
        )
        .unwrap();
        assert_eq!(m.body.len(), 3);
        let StmtKind::Def(_, f, defaults) = &m.body[2].kind else { panic!() };
        assert_eq!(f.params.len(), 2);
        assert_eq!(defaults.len(), 1);
        assert_eq!(f.n_locals, 3, "t, dt and x; A is global");
    }

    #[test]
    fn refuses_what_is_outside_it() {
        for src in [
            "import os\n",
            "from math import sin\n",
            "x = [i for i in range(3)]\n",
            "x = lambda: 1\n",
            "class A:\n    pass\n",
            "try:\n    x = 1\nexcept Exception:\n    pass\n",
            "x = a[1:2]\n",
            "x = a.__class__\n",
            "x = a.real\n",
            "x = 1 | 2\n",
            "x = f'{a}'\n",
            "def f():\n    def g():\n        pass\n",
            "def f(*a):\n    pass\n",
            "x: int = 1\n",
            "del x\n",
            "with a:\n    pass\n",
            "x = (yield)\n",
        ] {
            assert!(parse(src).is_err(), "accepted: {src}");
        }
        let deep = format!("x = {}1{}\n", "(".repeat(500), ")".repeat(500));
        assert!(parse(&deep).is_err());
    }
}
