//! The tree-walking interpreter: statements and expressions evaluated in
//! Python's order, calls, the builtins, the methods of dicts and lists,
//! iteration (lists read live, as Python's list iterator does; a dict that
//! changes size while it is iterated is Python's RuntimeError), and the
//! table lookup `interp` exactly as app/solver/maps.py computes it.
//!
//! Every operation that Python supports but this interpreter does not is
//! [`Fault::Outside`]: the caller then runs the script on Python instead,
//! so a result never differs from Python's.

use crate::ast::{BinOp, CmpOp, Expr, FuncDef, Module, Name, Stmt, StmtKind, Target};
use crate::value::{
    self, Builtin, Closure, DictCell, ListCell, MathFn, TupleCell, Value, binary, contains,
    dict_find, dict_get, dict_set, eq, index, insertable, order, range_len, set_index, to_float,
    to_int,
};
use crate::{Fault, Fault::*, MAX_ITEMS};
use std::cell::RefCell;
use std::cmp::Ordering;
use std::rc::Rc;
use std::time::Instant;

/// How deeply script functions may call each other here.
const MAX_CALL_DEPTH: usize = 50;

/// Statements, loop rounds and calls between looks at the clock.
const CLOCK_EVERY: u64 = 1024;

enum Flow {
    Normal,
    Return(Value),
    Break,
    Continue,
}

/// An iterator's state.
pub enum Iter {
    /// a list, read live by index (as Python's list iterator)
    List(Rc<ListCell>, usize),
    /// a tuple
    Tuple(Rc<TupleCell>, usize),
    /// a range: the next value, the step, how many are left
    Range(i64, i64, u64),
    /// a dict's keys, values or items (0, 1, 2): the next entry, and its
    /// size and key set at the start
    Dict(Rc<DictCell>, usize, usize, u64, u8),
    /// a string's characters
    Chars(Vec<char>, usize),
    /// `enumerate`
    Enumerate(Box<Iter>, i64),
    /// `zip`
    Zip(Vec<Iter>),
    /// an iterator object shared with everything that iterates it
    Shared(Rc<RefCell<Iter>>),
}

impl Iter {
    /// The next item, if any.
    pub fn next(&mut self) -> Result<Option<Value>, Fault> {
        Ok(match self {
            Iter::List(l, k) => {
                let l = l.borrow();
                let v = l.get(*k).cloned();
                *k += 1;
                v
            }
            Iter::Tuple(t, k) => {
                let v = t.get(*k).cloned();
                *k += 1;
                v
            }
            Iter::Range(cur, step, left) => {
                if *left == 0 {
                    None
                } else {
                    let v = *cur;
                    *left -= 1;
                    *cur = cur.wrapping_add(*step);
                    Some(Value::Int(v))
                }
            }
            Iter::Dict(d, k, n, keys, kind) => {
                let dm = d.borrow();
                if dm.entries.len() != *n {
                    return Err(Raise("dictionary changed size during iteration".into()));
                }
                if dm.keys != *keys {
                    return Err(Outside("a dict whose keys change while it is iterated".into()));
                }
                let v = dm.entries.get(*k).map(|(key, val)| match kind {
                    0 => key.clone(),
                    1 => val.clone(),
                    _ => Value::tuple(vec![key.clone(), val.clone()]),
                });
                *k += 1;
                v
            }
            Iter::Chars(c, k) => {
                let v = c.get(*k).map(|ch| Value::str(&ch.to_string()));
                *k += 1;
                v
            }
            Iter::Enumerate(it, n) => match it.next()? {
                Some(v) => {
                    let i = *n;
                    *n =
                        n.checked_add(1).ok_or_else(|| Outside("an enumerate this long".into()))?;
                    Some(Value::tuple(vec![Value::Int(i), v]))
                }
                None => None,
            },
            Iter::Zip(its) => {
                let mut out = Vec::with_capacity(its.len());
                for it in its.iter_mut() {
                    match it.next()? {
                        Some(v) => out.push(v),
                        None => return Ok(None),
                    }
                }
                if out.is_empty() { None } else { Some(Value::tuple(out)) }
            }
            Iter::Shared(rc) => rc.borrow_mut().next()?,
        })
    }

    /// The values it reads from (for the checks against cycles).
    pub fn sources(&self, out: &mut Vec<Value>) {
        match self {
            Iter::List(l, _) => out.push(Value::List(l.clone())),
            Iter::Tuple(t, _) => out.push(Value::Tuple(t.clone())),
            Iter::Dict(d, ..) => out.push(Value::Dict(d.clone())),
            Iter::Enumerate(it, _) => it.sources(out),
            Iter::Zip(its) => its.iter().for_each(|i| i.sources(out)),
            Iter::Shared(rc) => out.push(Value::Iter(rc.clone())),
            Iter::Range(..) | Iter::Chars(..) => {}
        }
    }
}

fn dict_iter(d: &Rc<DictCell>, kind: u8) -> Result<Iter, Fault> {
    let dm = d.borrow();
    if dm.unordered {
        return Err(Outside(
            "iterating the parameters (Python keeps the project's order, the import does not)"
                .into(),
        ));
    }
    Ok(Iter::Dict(d.clone(), 0, dm.entries.len(), dm.keys, kind))
}

/// An iterator over `v`.
fn iter_of(v: &Value) -> Result<Iter, Fault> {
    Ok(match v {
        Value::List(l) => Iter::List(l.clone(), 0),
        Value::Tuple(t) => Iter::Tuple(t.clone(), 0),
        Value::Range(a, b, s) => Iter::Range(*a, *s, range_len(*a, *b, *s) as u64),
        Value::Dict(d) => dict_iter(d, 0)?,
        Value::Str(s) => Iter::Chars(s.chars().collect(), 0),
        Value::View(d, kind) => dict_iter(d, *kind)?,
        // an iterator object: iterating it goes on from where it is
        Value::Iter(it) => Iter::Shared(it.clone()),
        _ => {
            return Err(Raise(format!("'{}' object is not iterable", v.type_name())));
        }
    })
}

/// Every item of `v`, at most [`MAX_ITEMS`].
fn collect(v: &Value) -> Result<Vec<Value>, Fault> {
    let mut it = iter_of(v)?;
    let mut out = vec![];
    while let Some(x) = it.next()? {
        if out.len() >= MAX_ITEMS {
            return Err(Outside("a sequence this long".into()));
        }
        out.push(x);
    }
    Ok(out)
}

/// The running script: its globals, and the clock and depth checks.
pub struct Machine {
    globals: Vec<Option<Value>>,
    names: Vec<Rc<str>>,
    deadline: Instant,
    ops: u64,
    depth: usize,
    limit_s: f64,
    cache: Vec<CacheEntry>,
}

type R<T> = Result<T, Fault>;

fn type_error(what: String) -> Fault {
    Raise(what)
}

/// A call with arguments this runner does not take (Python may take them,
/// or raise its own TypeError): left to Python.
fn args_error(f: &str, what: &str) -> Fault {
    Outside(format!("{f}() called with arguments that {what}"))
}

impl Machine {
    /// A machine for `m`, with the names every script is given (`math`,
    /// `clamp`, `interp`) set.
    pub fn new(m: &Module, limit_s: f64) -> Machine {
        let mut globals = vec![None; m.globals.len()];
        for (k, n) in m.globals.iter().enumerate() {
            globals[k] = match &**n {
                "math" => Some(Value::Math),
                "clamp" => Some(Value::Builtin(Builtin::Clamp)),
                "interp" => Some(Value::Builtin(Builtin::Interp)),
                _ => None,
            };
        }
        Machine {
            globals,
            names: m.globals.clone(),
            deadline: Instant::now(),
            ops: 0,
            depth: 0,
            limit_s,
            cache: vec![],
        }
    }

    /// The scripts' `interp(table, x, y=None)`, with maps.py's cache: a
    /// table found there whose copy still compares equal to it is not
    /// parsed again (even if it changed to equal values, as -0.0 for 0.0:
    /// Python's results are those of the parse it kept).
    fn interp(&mut self, args: &[Value]) -> R<Value> {
        let (table, x, y) = match args {
            [t, x] => (t, x, None),
            [t, x, Value::None] => (t, x, None),
            [t, x, y] => (t, x, Some(y)),
            _ => return Err(args_error("interp", "are not 2 or 3 values")),
        };
        let two = y.is_some();
        let parsed = match table {
            Value::Dict(d) => {
                let at = self.cache.iter().position(|e| Rc::ptr_eq(&e.table, d) && e.two == two);
                let hit = match at {
                    Some(i) => {
                        let e = &self.cache[i];
                        // Python compares the copy's items by identity first
                        if value::has_nan(table)? || value::has_nan(&e.copy)? {
                            return Err(Outside("a looked-up table holding a NaN".into()));
                        }
                        eq(&e.copy, table)?.then(|| e.parsed.clone())
                    }
                    None => None,
                };
                match hit {
                    Some(p) => p,
                    None => {
                        let p = Rc::new(if two {
                            Parsed::Two(parse_2d(table)?)
                        } else {
                            Parsed::One(parse_1d(table)?)
                        });
                        if self.cache.len() >= INTERP_CACHE_SIZE {
                            self.cache.clear();
                        }
                        let entry = CacheEntry {
                            table: d.clone(),
                            two,
                            copy: table_copy(d, two)?,
                            parsed: p.clone(),
                        };
                        match self
                            .cache
                            .iter()
                            .position(|e| Rc::ptr_eq(&e.table, d) && e.two == two)
                        {
                            Some(i) => self.cache[i] = entry,
                            None => self.cache.push(entry),
                        }
                        p
                    }
                }
            }
            _ => {
                return Err(table_error(if two {
                    "table2d data must be a non-empty {x: {y: value}} mapping".into()
                } else {
                    "table1d data must be a non-empty {x: value} mapping".into()
                }));
            }
        };
        let xv = lookup_arg(x)?;
        Ok(Value::Float(match (&*parsed, y) {
            (Parsed::One(p), None) => interp1(p, xv),
            (Parsed::Two(s), Some(y)) => interp2(s, xv, lookup_arg(y)?),
            _ => unreachable!("parsed for the lookup asked"),
        }))
    }

    /// A global's value by name.
    pub fn global(&self, name: &str) -> Option<Value> {
        let k = self.names.iter().position(|n| &**n == name)?;
        self.globals[k].clone()
    }

    fn start_clock(&mut self) {
        self.deadline = Instant::now() + std::time::Duration::from_secs_f64(self.limit_s);
        self.ops = 0;
    }

    fn tick(&mut self) -> R<()> {
        self.ops += 1;
        if self.ops.is_multiple_of(CLOCK_EVERY) && Instant::now() > self.deadline {
            return Err(Limit);
        }
        Ok(())
    }

    /// Runs the top-level code.
    pub fn run_module(&mut self, m: &Module) -> R<()> {
        self.start_clock();
        let mut frame: Vec<Option<Value>> = vec![];
        match self.block(&m.body, &mut frame)? {
            Flow::Normal => Ok(()),
            _ => Err(Outside("'return', 'break' or 'continue' at the top level".into())),
        }
    }

    /// Calls `f` with positional arguments (a call from the host: the clock
    /// starts now).
    pub fn call_top(&mut self, f: &Value, args: Vec<Value>) -> R<Value> {
        self.start_clock();
        self.call(f, args, vec![])
    }

    fn block(&mut self, b: &[Stmt], frame: &mut Vec<Option<Value>>) -> R<Flow> {
        for s in b {
            match self.stmt(s, frame) {
                Ok(Flow::Normal) => {}
                Ok(other) => return Ok(other),
                // name the line of the innermost statement
                Err(Outside(w)) if !w.starts_with("line ") => {
                    return Err(Outside(format!("line {}: {w}", s.line)));
                }
                Err(e) => return Err(e),
            }
        }
        Ok(Flow::Normal)
    }

    fn stmt(&mut self, s: &Stmt, frame: &mut Vec<Option<Value>>) -> R<Flow> {
        self.tick()?;
        match &s.kind {
            StmtKind::Expr(e) => {
                self.eval(e, frame)?;
            }
            StmtKind::Assign(targets, value) => {
                let v = self.eval(value, frame)?;
                for t in targets {
                    self.assign(t, v.clone(), frame)?;
                }
            }
            StmtKind::AugAssign(t, op, value) => match t {
                Target::Name(n) => {
                    let old = self.load(n, frame)?;
                    let v = self.eval(value, frame)?;
                    let new = self.inplace(*op, &old, &v)?;
                    self.store(n, new, frame);
                }
                Target::Index(c, i) => {
                    let c = self.eval(c, frame)?;
                    let i = self.eval(i, frame)?;
                    let old = index(&c, &i)?;
                    let v = self.eval(value, frame)?;
                    let new = self.inplace(*op, &old, &v)?;
                    set_index(&c, i, new)?;
                }
                Target::Tuple(_) => {
                    return Err(Outside("augmented assignment to several names".into()));
                }
            },
            StmtKind::If(branches, orelse) => {
                for (c, body) in branches {
                    if self.eval(c, frame)?.truthy() {
                        return self.block(body, frame);
                    }
                }
                return self.block(orelse, frame);
            }
            StmtKind::While(c, body, orelse) => loop {
                self.tick()?;
                if !self.eval(c, frame)?.truthy() {
                    return self.block(orelse, frame);
                }
                match self.block(body, frame)? {
                    Flow::Break => break,
                    Flow::Return(v) => return Ok(Flow::Return(v)),
                    Flow::Normal | Flow::Continue => {}
                }
            },
            StmtKind::For(t, it, body, orelse) => {
                let v = self.eval(it, frame)?;
                let mut it = iter_of(&v)?;
                loop {
                    self.tick()?;
                    let Some(x) = it.next()? else {
                        return self.block(orelse, frame);
                    };
                    self.assign(t, x, frame)?;
                    match self.block(body, frame)? {
                        Flow::Break => break,
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                        Flow::Normal | Flow::Continue => {}
                    }
                }
            }
            StmtKind::Def(n, def, defaults) => {
                let mut ds = Vec::with_capacity(defaults.len());
                for d in defaults {
                    ds.push(self.eval(d, frame)?);
                }
                let f = Value::Func(Rc::new(Closure { def: def.clone(), defaults: ds }));
                self.store(n, f, frame);
            }
            StmtKind::Return(e) => {
                let v = match e {
                    Some(e) => self.eval(e, frame)?,
                    None => Value::None,
                };
                return Ok(Flow::Return(v));
            }
            StmtKind::Pass => {}
            StmtKind::Break => return Ok(Flow::Break),
            StmtKind::Continue => return Ok(Flow::Continue),
            StmtKind::Import(n, attr) => {
                let v = match attr {
                    None => Value::Math,
                    Some(a) => Builtin::math(a).ok_or_else(|| Outside(format!("math.{a}")))?,
                };
                self.store(n, v, frame);
            }
        }
        Ok(Flow::Normal)
    }

    /// `old op= v`: a list's `+=` extends it in place (the same object).
    fn inplace(&mut self, op: BinOp, old: &Value, v: &Value) -> R<Value> {
        if op == BinOp::Add
            && let Value::List(l) = old
        {
            let items = collect(v)?;
            for x in &items {
                insertable(old, x)?;
            }
            let mut lm = l.borrow_mut();
            if lm.len() + items.len() > MAX_ITEMS {
                return Err(Outside("a list this long".into()));
            }
            lm.extend(items);
            return Ok(old.clone());
        }
        binary(op, old, v)
    }

    fn load(&self, n: &Name, frame: &[Option<Value>]) -> R<Value> {
        match n {
            Name::Local(k, name) => frame.get(*k).cloned().flatten().ok_or_else(|| {
                Raise(format!(
                    "cannot access local variable '{name}' where it is not \
                     associated with a value"
                ))
            }),
            Name::Global(k, name) => match &self.globals[*k] {
                Some(v) => Ok(v.clone()),
                None => match Builtin::of(name) {
                    Some(b) => Ok(Value::Builtin(b)),
                    // Python may know it (another builtin): let Python run it
                    None => Err(Outside(format!("the name '{name}'"))),
                },
            },
        }
    }

    fn store(&mut self, n: &Name, v: Value, frame: &mut Vec<Option<Value>>) {
        match n {
            Name::Local(k, _) => {
                if frame.len() <= *k {
                    frame.resize(*k + 1, None);
                }
                frame[*k] = Some(v);
            }
            Name::Global(k, _) => self.globals[*k] = Some(v),
        }
    }

    fn assign(&mut self, t: &Target, v: Value, frame: &mut Vec<Option<Value>>) -> R<()> {
        match t {
            Target::Name(n) => {
                self.store(n, v, frame);
                Ok(())
            }
            Target::Index(c, i) => {
                let c = self.eval(c, frame)?;
                let i = self.eval(i, frame)?;
                set_index(&c, i, v)
            }
            Target::Tuple(ts) => {
                let mut it = iter_of(&v)?;
                let mut vals = Vec::with_capacity(ts.len());
                while let Some(x) = it.next()? {
                    if vals.len() == ts.len() {
                        return Err(Raise(format!(
                            "too many values to unpack (expected {})",
                            ts.len()
                        )));
                    }
                    vals.push(x);
                }
                if vals.len() < ts.len() {
                    return Err(Raise(format!(
                        "not enough values to unpack (expected {}, got {})",
                        ts.len(),
                        vals.len()
                    )));
                }
                for (t, x) in ts.iter().zip(vals) {
                    self.assign(t, x, frame)?;
                }
                Ok(())
            }
        }
    }

    fn eval(&mut self, e: &Expr, frame: &mut Vec<Option<Value>>) -> R<Value> {
        Ok(match e {
            Expr::Const(v) => v.clone(),
            Expr::Name(n) => self.load(n, frame)?,
            Expr::Tuple(xs) => {
                let mut v = Vec::with_capacity(xs.len());
                for x in xs {
                    v.push(self.eval(x, frame)?);
                }
                Value::tuple(v)
            }
            Expr::List(xs) => {
                let mut v = Vec::with_capacity(xs.len());
                for x in xs {
                    v.push(self.eval(x, frame)?);
                }
                Value::list(v)
            }
            Expr::Dict(kv) => {
                let d = Value::new_dict();
                for (k, v) in kv {
                    let k = self.eval(k, frame)?;
                    let v = self.eval(v, frame)?;
                    dict_set(&d, k, v)?;
                }
                Value::Dict(d)
            }
            Expr::Neg(a) => value::neg(&self.eval(a, frame)?)?,
            Expr::Pos(a) => value::pos(&self.eval(a, frame)?)?,
            Expr::Not(a) => Value::Bool(!self.eval(a, frame)?.truthy()),
            Expr::Bin(op, a, b) => {
                let a = self.eval(a, frame)?;
                let b = self.eval(b, frame)?;
                binary(*op, &a, &b)?
            }
            Expr::And(a, b) => {
                let a = self.eval(a, frame)?;
                if !a.truthy() { a } else { self.eval(b, frame)? }
            }
            Expr::Or(a, b) => {
                let a = self.eval(a, frame)?;
                if a.truthy() { a } else { self.eval(b, frame)? }
            }
            Expr::Compare(first, rest) => {
                let mut left = self.eval(first, frame)?;
                for (op, right) in rest {
                    let right = self.eval(right, frame)?;
                    if !compare(*op, &left, &right)? {
                        return Ok(Value::Bool(false));
                    }
                    left = right;
                }
                Value::Bool(true)
            }
            Expr::IfExp(c, a, b) => {
                if self.eval(c, frame)?.truthy() {
                    self.eval(a, frame)?
                } else {
                    self.eval(b, frame)?
                }
            }
            Expr::Call(f, args, kwargs) => {
                let f = self.eval(f, frame)?;
                let mut a = Vec::with_capacity(args.len());
                for x in args {
                    a.push(self.eval(x, frame)?);
                }
                let mut kw = Vec::with_capacity(kwargs.len());
                for (k, x) in kwargs {
                    kw.push((k.clone(), self.eval(x, frame)?));
                }
                self.call(&f, a, kw)?
            }
            Expr::Attr(a, name) => {
                let a = self.eval(a, frame)?;
                attribute(&a, name)?
            }
            Expr::Index(c, i) => {
                let c = self.eval(c, frame)?;
                let i = self.eval(i, frame)?;
                index_view(&c, &i)?
            }
        })
    }

    fn call(&mut self, f: &Value, args: Vec<Value>, kwargs: Vec<(Rc<str>, Value)>) -> R<Value> {
        self.tick()?;
        match f {
            Value::Func(c) => self.call_func(c, args, kwargs),
            Value::Builtin(b) => self.builtin(*b, args, kwargs),
            Value::Method(m) => method(&m.0, &m.1, args, kwargs),
            _ => Err(type_error(format!("'{}' object is not callable", f.type_name()))),
        }
    }

    fn call_func(
        &mut self,
        c: &Rc<Closure>,
        args: Vec<Value>,
        kwargs: Vec<(Rc<str>, Value)>,
    ) -> R<Value> {
        let def: &FuncDef = &c.def;
        let np = def.params.len();
        if args.len() > np {
            return Err(type_error(format!(
                "{}() takes {} positional arguments but {} were given",
                def.name,
                np,
                args.len()
            )));
        }
        let mut frame: Vec<Option<Value>> = vec![None; def.n_locals.max(np)];
        for (k, a) in args.into_iter().enumerate() {
            frame[k] = Some(a);
        }
        for (k, v) in kwargs {
            let Some(slot) = def.params.iter().position(|p| *p == k) else {
                return Err(type_error(format!(
                    "{}() got an unexpected keyword argument '{k}'",
                    def.name
                )));
            };
            if frame[slot].is_some() {
                return Err(type_error(format!(
                    "{}() got multiple values for argument '{k}'",
                    def.name
                )));
            }
            frame[slot] = Some(v);
        }
        let first_default = np - def.n_defaults;
        for (k, slot) in frame.iter_mut().enumerate().take(np) {
            if slot.is_none() {
                if k >= first_default {
                    *slot = Some(c.defaults[k - first_default].clone());
                } else {
                    return Err(type_error(format!(
                        "{}() missing a required argument: '{}'",
                        def.name, def.params[k]
                    )));
                }
            }
        }
        if self.depth >= MAX_CALL_DEPTH {
            return Err(Outside("functions calling each other this deeply".into()));
        }
        self.depth += 1;
        let r = self.block(&def.body, &mut frame);
        self.depth -= 1;
        Ok(match r? {
            Flow::Return(v) => v,
            _ => Value::None,
        })
    }

    fn builtin(&mut self, b: Builtin, args: Vec<Value>, kwargs: Vec<(Rc<str>, Value)>) -> R<Value> {
        if !kwargs.is_empty() && !matches!(b, Builtin::Dict | Builtin::Enumerate) {
            return Err(Outside(format!("keyword arguments to {b:?}")));
        }
        let n = args.len();
        let one = |name: &str| -> R<Value> {
            if n != 1 {
                return Err(args_error(name, &format!("takes exactly one argument ({n} given)")));
            }
            Ok(args[0].clone())
        };
        Ok(match b {
            Builtin::Abs => {
                let x = one("abs")?;
                match &x {
                    Value::Float(f) => Value::Float(f.abs()),
                    _ => match x.as_int() {
                        Some(i) => Value::Int(
                            i.checked_abs()
                                .ok_or_else(|| Outside("an integer beyond 64 bits".into()))?,
                        ),
                        None => {
                            return Err(type_error(format!(
                                "bad operand type for abs(): '{}'",
                                x.type_name()
                            )));
                        }
                    },
                }
            }
            Builtin::Min | Builtin::Max => {
                let name = if b == Builtin::Min { "min" } else { "max" };
                let items = match n {
                    0 => {
                        return Err(type_error(format!(
                            "{name} expected at least 1 argument, got 0"
                        )));
                    }
                    1 => collect(&args[0])?,
                    _ => args,
                };
                let mut it = items.into_iter();
                let Some(mut best) = it.next() else {
                    return Err(Raise(format!("{name}() iterable argument is empty")));
                };
                let want = if b == Builtin::Min { Ordering::Less } else { Ordering::Greater };
                let op = if b == Builtin::Min { "<" } else { ">" };
                for x in it {
                    if order(&x, &best, op)? == Some(want) {
                        best = x;
                    }
                }
                best
            }
            Builtin::Clamp => {
                if n != 3 {
                    return Err(args_error(
                        "clamp",
                        &format!("takes 3 positional arguments but {n} were given"),
                    ));
                }
                // max(lo, min(hi, x)), with min's and max's rules
                let (x, lo, hi) = (&args[0], &args[1], &args[2]);
                let m = if order(x, hi, "<")? == Some(Ordering::Less) { x } else { hi };
                if order(m, lo, ">")? == Some(Ordering::Greater) { m.clone() } else { lo.clone() }
            }
            Builtin::Float => match n {
                0 => Value::Float(0.0),
                1 => Value::Float(to_float(&args[0])?),
                _ => return Err(args_error("float", "takes at most 1 argument")),
            },
            Builtin::Int => match n {
                0 => Value::Int(0),
                1 => Value::Int(to_int(&args[0])?),
                _ => return Err(Outside("int() with a base".into())),
            },
            Builtin::Bool => match n {
                0 => Value::Bool(false),
                1 => Value::Bool(args[0].truthy()),
                _ => return Err(args_error("bool", "takes at most 1 argument")),
            },
            Builtin::Len => {
                let x = one("len")?;
                Value::Int(match &x {
                    Value::Str(s) => s.chars().count() as i64,
                    Value::Tuple(t) => t.len() as i64,
                    Value::List(l) => l.borrow().len() as i64,
                    Value::Dict(d) | Value::View(d, _) => d.borrow().entries.len() as i64,
                    Value::Range(a, b, s) => i64::try_from(range_len(*a, *b, *s))
                        .map_err(|_| Outside("the length of a range beyond 64 bits".into()))?,
                    _ => {
                        return Err(type_error(format!(
                            "object of type '{}' has no len()",
                            x.type_name()
                        )));
                    }
                })
            }
            Builtin::Range => {
                let ints: Vec<i64> = args
                    .iter()
                    .map(|a| {
                        a.as_int().ok_or_else(|| {
                            type_error(format!(
                                "'{}' object cannot be interpreted as an integer",
                                a.type_name()
                            ))
                        })
                    })
                    .collect::<R<_>>()?;
                let (a, z, s) = match ints.as_slice() {
                    [z] => (0, *z, 1),
                    [a, z] => (*a, *z, 1),
                    [a, z, s] => (*a, *z, *s),
                    _ => return Err(args_error("range", "expected 1 to 3 arguments")),
                };
                if s == 0 {
                    return Err(Raise("range() arg 3 must not be zero".into()));
                }
                Value::Range(a, z, s)
            }
            Builtin::Round => {
                if n != 1 {
                    return Err(Outside("round() with a number of digits".into()));
                }
                match &args[0] {
                    Value::Float(f) => {
                        if f.is_nan() {
                            return Err(Raise("cannot convert float NaN to integer".into()));
                        }
                        if f.is_infinite() {
                            return Err(Raise("cannot convert float infinity to integer".into()));
                        }
                        Value::Int(to_int(&Value::Float(f.round_ties_even()))?)
                    }
                    x => match x.as_int() {
                        Some(i) => Value::Int(i),
                        None => {
                            return Err(type_error(format!(
                                "type {} doesn't define __round__ method",
                                x.type_name()
                            )));
                        }
                    },
                }
            }
            Builtin::Enumerate => {
                let mut start = 0i64;
                for (k, v) in &kwargs {
                    if &**k == "start" {
                        start = v.as_int().ok_or_else(|| Outside("a non-integer start".into()))?;
                    } else {
                        return Err(Outside("enumerate()'s other keywords".into()));
                    }
                }
                if matches!(args.first(), Some(Value::Iter(_))) {
                    return Err(Outside("enumerate() of an iterator object".into()));
                }
                let (it, start) = match n {
                    1 => (iter_of(&args[0])?, start),
                    2 if kwargs.is_empty() => (
                        iter_of(&args[0])?,
                        args[1].as_int().ok_or_else(|| Outside("a non-integer start".into()))?,
                    ),
                    _ => return Err(args_error("enumerate", "takes 1 or 2 arguments")),
                };
                Value::Iter(Rc::new(RefCell::new(Iter::Enumerate(Box::new(it), start))))
            }
            Builtin::Zip => {
                if args.iter().any(|a| matches!(a, Value::Iter(_))) {
                    return Err(Outside("zip() of an iterator object".into()));
                }
                let its = args.iter().map(iter_of).collect::<R<Vec<_>>>()?;
                Value::Iter(Rc::new(RefCell::new(Iter::Zip(its))))
            }
            Builtin::List => match n {
                0 => Value::list(vec![]),
                1 => Value::list(collect(&args[0])?),
                _ => return Err(args_error("list", "expected at most 1 argument")),
            },
            Builtin::Tuple => match n {
                0 => Value::tuple(vec![]),
                1 => Value::tuple(collect(&args[0])?),
                _ => return Err(args_error("tuple", "expected at most 1 argument")),
            },
            Builtin::Dict => {
                let d = Value::new_dict();
                match n {
                    0 => {}
                    1 => update_from(&d, &args[0])?,
                    _ => return Err(args_error("dict", "expected at most 1 argument")),
                }
                for (k, v) in kwargs {
                    dict_set(&d, Value::Str(k), v)?;
                }
                Value::Dict(d)
            }
            Builtin::Any | Builtin::All => {
                let x = one(if b == Builtin::Any { "any" } else { "all" })?;
                let mut it = iter_of(&x)?;
                let want = b == Builtin::Any;
                let mut found = !want;
                while let Some(v) = it.next()? {
                    self.tick()?;
                    if v.truthy() == want {
                        found = want;
                        break;
                    }
                }
                Value::Bool(found)
            }
            // print shows nothing here and changes no result; what it would
            // print must not fail on Python either (a number, a string)
            Builtin::Print => {
                if args.iter().any(|a| {
                    !matches!(
                        a,
                        Value::None
                            | Value::Bool(_)
                            | Value::Int(_)
                            | Value::Float(_)
                            | Value::Str(_)
                    )
                }) {
                    return Err(Outside("print() of a container or an object".into()));
                }
                Value::None
            }
            Builtin::Interp => self.interp(&args)?,
            Builtin::Math(m) => math(m, &args)?,
        })
    }
}

/// `a op b` for comparisons.
fn compare(op: CmpOp, a: &Value, b: &Value) -> R<bool> {
    Ok(match op {
        _ if matches!(a, Value::View(..) | Value::Iter(_))
            || matches!(b, Value::View(..) | Value::Iter(_)) =>
        {
            return Err(Outside("comparing a dict view or an iterator".into()));
        }
        // containers compare their items by identity first (see has_nan)
        CmpOp::Eq | CmpOp::Ne
            if matches!(a, Value::Tuple(_) | Value::List(_) | Value::Dict(_))
                && (value::has_nan(a)? || value::has_nan(b)?) =>
        {
            return Err(Outside("comparing containers that hold a NaN".into()));
        }
        CmpOp::Eq => eq(a, b)?,
        CmpOp::Ne => !eq(a, b)?,
        CmpOp::Lt => order(a, b, "<")? == Some(Ordering::Less),
        CmpOp::Le => matches!(order(a, b, "<=")?, Some(Ordering::Less | Ordering::Equal)),
        CmpOp::Gt => order(a, b, ">")? == Some(Ordering::Greater),
        CmpOp::Ge => matches!(order(a, b, ">=")?, Some(Ordering::Greater | Ordering::Equal)),
        CmpOp::In => contains_view(b, a)?,
        CmpOp::NotIn => !contains_view(b, a)?,
        CmpOp::Is => value::is(a, b)?,
        CmpOp::IsNot => !value::is(a, b)?,
    })
}

fn contains_view(c: &Value, x: &Value) -> R<bool> {
    match c {
        Value::View(d, 0) => Ok(dict_get(d, x)?.is_some()),
        Value::View(..) | Value::Iter(_) => {
            Err(Outside("'in' on a dict view or an iterator".into()))
        }
        _ => contains(c, x),
    }
}

fn index_view(c: &Value, i: &Value) -> R<Value> {
    match c {
        Value::View(..) | Value::Iter(_) => {
            Err(Raise(format!("'{}' object is not subscriptable", c.type_name())))
        }
        _ => index(c, i),
    }
}

/// `a.name`.
fn attribute(a: &Value, name: &Rc<str>) -> R<Value> {
    match a {
        Value::Math => Builtin::math(name).ok_or_else(|| Outside(format!("math.{name}"))),
        Value::Dict(_)
            if matches!(
                &**name,
                "get"
                    | "update"
                    | "keys"
                    | "values"
                    | "items"
                    | "setdefault"
                    | "pop"
                    | "copy"
                    | "clear"
            ) =>
        {
            Ok(Value::Method(Rc::new((a.clone(), name.clone()))))
        }
        Value::List(_)
            if matches!(
                &**name,
                "append"
                    | "extend"
                    | "insert"
                    | "pop"
                    | "index"
                    | "count"
                    | "remove"
                    | "copy"
                    | "clear"
            ) =>
        {
            Ok(Value::Method(Rc::new((a.clone(), name.clone()))))
        }
        Value::Tuple(_) if matches!(&**name, "index" | "count") => {
            Ok(Value::Method(Rc::new((a.clone(), name.clone()))))
        }
        // Python may have it (a str method, a float's): let Python run it
        _ => Err(Outside(format!("'.{name}' on a {}", a.type_name()))),
    }
}

/// `d.update(x)`'s and `dict(x)`'s argument: a dict, or pairs.
fn update_from(d: &Rc<DictCell>, x: &Value) -> R<()> {
    match x {
        Value::Dict(src) => {
            let (entries, unordered) = {
                let s = src.borrow();
                (s.entries.clone(), s.unordered)
            };
            // inserted in the source's order: unknown if that is
            d.borrow_mut().unordered |= unordered;
            for (k, v) in entries {
                dict_set(d, k, v)?;
            }
            Ok(())
        }
        Value::List(_) | Value::Tuple(_) => {
            for pair in collect(x)? {
                let kv = collect(&pair)?;
                if kv.len() != 2 {
                    return Err(Raise(format!(
                        "dictionary update sequence element has length {}; 2 is required",
                        kv.len()
                    )));
                }
                let mut kv = kv.into_iter();
                let (k, v) = (kv.next().expect("two"), kv.next().expect("two"));
                dict_set(d, k, v)?;
            }
            Ok(())
        }
        _ => Err(Outside(format!("updating a dict from a {}", x.type_name()))),
    }
}

/// A method call.
fn method(obj: &Value, name: &str, args: Vec<Value>, kwargs: Vec<(Rc<str>, Value)>) -> R<Value> {
    let n = args.len();
    if !(kwargs.is_empty() || name == "update" && matches!(obj, Value::Dict(_))) {
        return Err(Outside(format!("keyword arguments to .{name}()")));
    }
    let arity = |lo: usize, hi: usize| -> R<()> {
        if n < lo || n > hi {
            return Err(args_error(name, &format!("are not {lo} to {hi} values")));
        }
        Ok(())
    };
    match obj {
        Value::Dict(d) => match name {
            "get" => {
                arity(1, 2)?;
                Ok(dict_get(d, &args[0])?
                    .unwrap_or_else(|| args.get(1).cloned().unwrap_or(Value::None)))
            }
            "update" => {
                arity(0, 1)?;
                if let Some(x) = args.first() {
                    update_from(d, x)?;
                }
                for (k, v) in kwargs {
                    dict_set(d, Value::Str(k), v)?;
                }
                Ok(Value::None)
            }
            "keys" => {
                arity(0, 0)?;
                Ok(Value::View(d.clone(), 0))
            }
            "values" => {
                arity(0, 0)?;
                Ok(Value::View(d.clone(), 1))
            }
            "items" => {
                arity(0, 0)?;
                Ok(Value::View(d.clone(), 2))
            }
            "setdefault" => {
                arity(1, 2)?;
                match dict_get(d, &args[0])? {
                    Some(v) => Ok(v),
                    None => {
                        let v = args.get(1).cloned().unwrap_or(Value::None);
                        dict_set(d, args[0].clone(), v.clone())?;
                        Ok(v)
                    }
                }
            }
            "pop" => {
                arity(1, 2)?;
                match dict_find(d, &args[0])? {
                    Some(k) => Ok(d.borrow_mut().remove(k).1),
                    None => match args.get(1) {
                        Some(v) => Ok(v.clone()),
                        None => Err(Raise(args[0].repr())),
                    },
                }
            }
            "copy" => {
                arity(0, 0)?;
                let (entries, unordered) = {
                    let dm = d.borrow();
                    (dm.entries.clone(), dm.unordered)
                };
                let c = Value::dict(entries)?;
                if let Value::Dict(cd) = &c {
                    cd.borrow_mut().unordered = unordered;
                }
                Ok(c)
            }
            "clear" => {
                arity(0, 0)?;
                d.borrow_mut().clear();
                Ok(Value::None)
            }
            _ => Err(Outside(format!("dict.{name}"))),
        },
        Value::List(l) => match name {
            "append" => {
                arity(1, 1)?;
                insertable(obj, &args[0])?;
                let mut lm = l.borrow_mut();
                if lm.len() >= MAX_ITEMS {
                    return Err(Outside("a list this long".into()));
                }
                lm.push(args[0].clone());
                Ok(Value::None)
            }
            "extend" => {
                arity(1, 1)?;
                let items = collect(&args[0])?;
                for x in &items {
                    insertable(obj, x)?;
                }
                let mut lm = l.borrow_mut();
                if lm.len() + items.len() > MAX_ITEMS {
                    return Err(Outside("a list this long".into()));
                }
                lm.extend(items);
                Ok(Value::None)
            }
            "insert" => {
                arity(2, 2)?;
                let i = args[0].as_int().ok_or_else(|| {
                    type_error(format!(
                        "'{}' object cannot be interpreted as an integer",
                        args[0].type_name()
                    ))
                })?;
                insertable(obj, &args[1])?;
                let mut lm = l.borrow_mut();
                if lm.len() >= MAX_ITEMS {
                    return Err(Outside("a list this long".into()));
                }
                let len = lm.len() as i64;
                let k = if i < 0 { (i + len).max(0) } else { i.min(len) };
                lm.insert(k as usize, args[1].clone());
                Ok(Value::None)
            }
            "pop" => {
                arity(0, 1)?;
                let mut lm = l.borrow_mut();
                if lm.is_empty() {
                    return Err(Raise("pop from empty list".into()));
                }
                let len = lm.len() as i64;
                let i = match args.first() {
                    Some(v) => v.as_int().ok_or_else(|| {
                        type_error(format!(
                            "'{}' object cannot be interpreted as an integer",
                            v.type_name()
                        ))
                    })?,
                    None => len - 1,
                };
                let k = if i < 0 { i + len } else { i };
                if k < 0 || k >= len {
                    return Err(Raise("pop index out of range".into()));
                }
                Ok(lm.remove(k as usize))
            }
            "index" | "count" | "remove" => {
                arity(1, 1)?;
                value::searchable(&args[0])?;
                let items = l.borrow().clone();
                let hits = matches(&items, &args[0], name != "count")?;
                let pos = hits.first().copied();
                match name {
                    "count" => Ok(Value::Int(hits.len() as i64)),
                    "index" => pos
                        .map(|k| Value::Int(k as i64))
                        .ok_or_else(|| Raise(format!("{} is not in list", args[0].repr()))),
                    _ => match pos {
                        Some(k) => {
                            l.borrow_mut().remove(k);
                            Ok(Value::None)
                        }
                        None => Err(Raise("list.remove(x): x not in list".into())),
                    },
                }
            }
            "copy" => {
                arity(0, 0)?;
                Ok(Value::list(l.borrow().clone()))
            }
            "clear" => {
                arity(0, 0)?;
                l.borrow_mut().clear();
                Ok(Value::None)
            }
            _ => Err(Outside(format!("list.{name}"))),
        },
        Value::Tuple(t) => match name {
            "count" => {
                arity(1, 1)?;
                Ok(Value::Int(matches(t, &args[0], false)?.len() as i64))
            }
            "index" => {
                arity(1, 1)?;
                matches(t, &args[0], true)?
                    .first()
                    .map(|k| Value::Int(*k as i64))
                    .ok_or_else(|| Raise("tuple.index(x): x not in tuple".into()))
            }
            _ => Err(Outside(format!("tuple.{name}"))),
        },
        _ => Err(Outside(format!(".{name}() on a {}", obj.type_name()))),
    }
}

/// The positions of the items equal to `x` (the first only, with `first`),
/// compared in order as Python compares them.
fn matches(items: &[Value], x: &Value, first: bool) -> R<Vec<usize>> {
    value::searchable(x)?;
    let mut out = vec![];
    for (k, v) in items.iter().enumerate() {
        if eq(v, x)? {
            out.push(k);
            if first {
                break;
            }
        }
    }
    Ok(out)
}

/// A number for the math module (`TypeError` otherwise).
fn real(x: &Value) -> R<f64> {
    x.as_float().ok_or_else(|| type_error(format!("must be real number, not {}", x.type_name())))
}

/// The math module's functions, as CPython computes them.
fn math(m: MathFn, args: &[Value]) -> R<Value> {
    let want = match m {
        MathFn::Copysign => 2,
        _ => 1,
    };
    if args.len() != want {
        return Err(type_error(format!("math function takes {want} argument(s)")));
    }
    let to_int_result = |f: f64| -> R<Value> {
        if f.is_nan() {
            return Err(Raise("cannot convert float NaN to integer".into()));
        }
        if f.is_infinite() {
            return Err(Raise("cannot convert float infinity to integer".into()));
        }
        Ok(Value::Int(to_int(&Value::Float(f))?))
    };
    Ok(match m {
        MathFn::Sqrt => {
            let x = real(&args[0])?;
            if x < 0.0 {
                return Err(Raise("math domain error".into()));
            }
            // IEEE square root: correctly rounded on every platform
            Value::Float(x.sqrt())
        }
        MathFn::Fabs => Value::Float(real(&args[0])?.abs()),
        MathFn::Floor | MathFn::Ceil | MathFn::Trunc => {
            if let Some(i) = args[0].as_int() {
                return Ok(Value::Int(i));
            }
            let x = real(&args[0])?;
            to_int_result(match m {
                MathFn::Floor => x.floor(),
                MathFn::Ceil => x.ceil(),
                _ => x.trunc(),
            })?
        }
        MathFn::Copysign => Value::Float(real(&args[0])?.copysign(real(&args[1])?)),
        MathFn::IsNan => Value::Bool(real(&args[0])?.is_nan()),
        MathFn::IsInf => Value::Bool(real(&args[0])?.is_infinite()),
        MathFn::IsFinite => Value::Bool(real(&args[0])?.is_finite()),
        // CPython: x * degToRad and x * radToDeg, constants computed in double
        MathFn::Radians => Value::Float(real(&args[0])? * (std::f64::consts::PI / 180.0)),
        MathFn::Degrees => Value::Float(real(&args[0])? * (180.0 / std::f64::consts::PI)),
    })
}

fn table_error(what: String) -> Fault {
    Raise(what)
}

/// A 2-D table: sheets `(x, [(y, v)])` sorted by x.
type Sheets = Vec<(f64, Vec<(f64, f64)>)>;

/// maps.py's `parse_table1d`.
fn parse_1d(raw: &Value) -> R<Vec<(f64, f64)>> {
    let Value::Dict(d) = raw else {
        return Err(table_error("table1d data must be a non-empty {x: value} mapping".into()));
    };
    let entries = d.borrow().entries.clone();
    if entries.is_empty() {
        return Err(table_error("table1d data must be a non-empty {x: value} mapping".into()));
    }
    let mut pts = Vec::with_capacity(entries.len());
    for (k, v) in &entries {
        match (to_float(k), to_float(v)) {
            (Ok(x), Ok(y)) => pts.push((x, y)),
            _ => {
                return Err(table_error(format!(
                    "table1d entry '{}: {}' is not numeric",
                    k.repr(),
                    v.repr()
                )));
            }
        }
    }
    if pts.iter().any(|p| p.0.is_nan()) {
        return Err(Outside("a table key that is not a number".into()));
    }
    // a stable sort, as Python's
    pts.sort_by(|a, b| a.0.partial_cmp(&b.0).expect("no NaN"));
    for w in pts.windows(2) {
        if w[0].0 == w[1].0 {
            return Err(table_error(format!("table1d has duplicate key {}", w[0].0)));
        }
    }
    Ok(pts)
}

/// maps.py's `parse_table2d`.
fn parse_2d(raw: &Value) -> R<Sheets> {
    let Value::Dict(d) = raw else {
        return Err(table_error("table2d data must be a non-empty {x: {y: value}} mapping".into()));
    };
    let entries = d.borrow().entries.clone();
    if entries.is_empty() {
        return Err(table_error("table2d data must be a non-empty {x: {y: value}} mapping".into()));
    }
    let mut sheets = Vec::with_capacity(entries.len());
    for (k, inner) in &entries {
        let x = to_float(k)
            .map_err(|_| table_error(format!("table2d outer key '{}' is not numeric", k.repr())))?;
        sheets.push((x, parse_1d(inner)?));
    }
    if sheets.iter().any(|s| s.0.is_nan()) {
        return Err(Outside("a table key that is not a number".into()));
    }
    sheets.sort_by(|a, b| a.0.partial_cmp(&b.0).expect("no NaN"));
    for w in sheets.windows(2) {
        if w[0].0 == w[1].0 {
            return Err(table_error(format!("table2d has duplicate outer key {}", w[0].0)));
        }
    }
    Ok(sheets)
}

/// A table as `interp` reads it (app/solver/maps.py).
pub enum Parsed {
    /// `[(x, y)]` sorted by x
    One(Vec<(f64, f64)>),
    /// sheets `(x, [(y, v)])` sorted by x
    Two(Sheets),
}

/// One entry of `interp`'s cache, as maps.py's `_make_interp` keeps it:
/// the table, a copy of it as it was (one level deeper for a 2-D table),
/// and its parsed points.
pub struct CacheEntry {
    table: Rc<DictCell>,
    two: bool,
    copy: Value,
    parsed: Rc<Parsed>,
}

/// The tables one script's `interp` keeps parsed (maps.py's
/// `_INTERP_CACHE_SIZE`): emptied when full.
const INTERP_CACHE_SIZE: usize = 32;

/// `dict(table)`, or `{k: dict(v) for k, v in table.items()}` for 2-D.
fn table_copy(d: &Rc<DictCell>, two: bool) -> R<Value> {
    let entries = d.borrow().entries.clone();
    let mut out = Vec::with_capacity(entries.len());
    for (k, v) in entries {
        let v = match (&v, two) {
            (Value::Dict(inner), true) => Value::dict(inner.borrow().entries.clone())?,
            _ => v,
        };
        out.push((k, v));
    }
    Value::dict(out)
}

/// `bisect_left(points, x, 1, len - 1, key=x of a point)`.
fn bisect_left<T>(pts: &[(f64, T)], x: f64) -> usize {
    let (mut lo, mut hi) = (1usize, pts.len() - 1);
    while lo < hi {
        let mid = (lo + hi) / 2;
        if pts[mid].0 < x {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo
}

/// A lookup argument: compared exactly as given, used as a float.
fn lookup_arg(x: &Value) -> R<f64> {
    match x {
        Value::Float(f) => Ok(*f),
        _ => match x.as_int() {
            Some(i) if i.unsigned_abs() <= 1 << 53 => Ok(i as f64),
            Some(_) => Err(Outside("a table looked up at an integer beyond 2**53".into())),
            None => Err(type_error(format!(
                "'<=' not supported between instances of '{}' and 'float'",
                x.type_name()
            ))),
        },
    }
}

/// maps.py's `interp1` (flat outside the data).
fn interp1(pts: &[(f64, f64)], x: f64) -> f64 {
    if pts.is_empty() {
        return 0.0;
    }
    if x <= pts[0].0 {
        return pts[0].1;
    }
    if x >= pts[pts.len() - 1].0 || x.is_nan() {
        return pts[pts.len() - 1].1;
    }
    let lo = bisect_left(pts, x);
    let (xa, ya) = pts[lo - 1];
    let (xb, yb) = pts[lo];
    if xb == xa {
        return yb;
    }
    ya + (yb - ya) * (x - xa) / (xb - xa)
}

/// maps.py's `interp2` (flat outside the data).
fn interp2(sheets: &Sheets, xo: f64, xi: f64) -> f64 {
    if sheets.is_empty() {
        return 0.0;
    }
    let first = sheets[0].0;
    let last = sheets[sheets.len() - 1].0;
    if !(first < xo && xo < last) {
        let edge = if xo <= first { 0 } else { sheets.len() - 1 };
        return interp1(&sheets[edge].1, xi);
    }
    let lo = bisect_left(sheets, xo);
    let (xa, pa) = (&sheets[lo - 1].0, &sheets[lo - 1].1);
    let (xb, pb) = (&sheets[lo].0, &sheets[lo].1);
    let ya = interp1(pa, xi);
    let yb = interp1(pb, xi);
    if xb == xa {
        return yb;
    }
    ya + (yb - ya) * (xo - xa) / (xb - xa)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lookup_is_maps_py() {
        let pts = vec![(800.0, 105.0), (1000.0, 112.0), (1500.0, 124.0)];
        assert_eq!(interp1(&pts, 500.0), 105.0);
        assert_eq!(interp1(&pts, 2000.0), 124.0);
        assert_eq!(interp1(&pts, 1000.0), 112.0);
        assert_eq!(
            interp1(&pts, 900.0),
            105.0 + (112.0 - 105.0) * (900.0 - 800.0) / (1000.0 - 800.0)
        );
        assert_eq!(interp1(&pts, f64::NAN), 124.0);
        assert_eq!(bisect_left(&pts, 1000.0), 1);
        assert_eq!(bisect_left(&pts, 1000.5), 2);
    }
}
