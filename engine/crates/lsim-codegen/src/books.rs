//! The energy books' integrands compiled (DESIGN.md §11; lsim-solve's
//! `energy::Integrand`): each part's power in and its absolute value, its
//! loss, and its stored energy's rate along `(1, y')`, from the model's
//! channels.
//!
//! The run interprets them at every quadrature call: the powers and
//! losses by `lsim_ir::eval`, the rates by lsim-solve's dual numbers
//! (`ad::dual`) through the chain of assignments the stored energies read.
//! This lowering emits the same operations in the same order into an
//! [`Emit`] backend (a tape at once, machine code on another thread), so
//! the results are bitwise the interpreter's (a NaN is a NaN on both
//! sides, its sign and payload not defined: the optimiser may drop a pair
//! of negations around a product, and no decision reads them):
//!
//! * **values** (powers, losses) by the rules of `lsim_ir::eval`: the
//!   library's `pow` for every power, `f64::min`/`max` with the platform's
//!   rule at a tie of signed zeros, a table's value as the model
//!   interpolates it;
//! * **rates** by the rules of `ad::dual`, which differ from `eval`'s
//!   where they must: `min`, `max` and `limit` pick by `<` and `>` (a NaN
//!   argument picks differently), a product's rate is `times(b, da) +
//!   times(a, db)` with `times(f, d)` zero when `d` is (no `inf · 0`), a
//!   power whose exponent's rate is zero takes the constant-exponent rule,
//!   a table's rate is the sum of its partial derivatives times its
//!   arguments' rates (`Iterator::sum`'s start included).
//!
//! A rate that is structurally a constant (zero, or the time's one) is
//! kept as that constant and folded with the same IEEE operations, so the
//! code holds no operation whose result is known; signed zeros are kept
//! as the interpreter's operations make them.
//!
//! **Declining.** The interpreter's dual numbers give up (`Err`) on a
//! derivative, a previous value, a name, a variable with no rate, a
//! table it cannot read, a missing argument; `eval` gives NaN or stops
//! there. An integrand that reads any of these, anywhere (even in a
//! branch not taken), is not compiled and stays the interpreter's: the
//! caller interprets what [`BooksCover`] leaves out.

use crate::backend::{Base, Cc, Clif, Emit, Lib};
use crate::lower::{TieRule, tie_rule};
use crate::no_executable_memory;
use crate::tables::TableStore;
use crate::tape::Recorder;
use crate::{CancelOnDrop, Code, CodeMemory, CodegenError, RawFn, TieredCode, jit};
use cranelift_codegen::ir::{Function, InstBuilder, UserFuncName};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{FuncId, Linkage, Module};
use lsim_ir::expr::{BinaryOp, Builtin, CmpOp, Expr};
use lsim_ir::fenv::DefaultFloatEnv;
use lsim_ir::runtime::{BooksCover, BooksKernel, BooksSpec, EvalInput, RateSource};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};

/// The sizes the integrands are checked against.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Sizes {
    pub n_vars: usize,
    pub n_p: usize,
    pub n_y: usize,
    /// how many tables the model has
    pub n_tables: usize,
}

/// What is compiled of a spec.
#[derive(Clone, Debug)]
pub(crate) struct Plan {
    pub cover: Vec<BooksCover>,
    /// the steps of the chain the covered rates read, in the chain's order
    pub steps: Vec<usize>,
    /// expression nodes compiled (a size measure)
    pub nodes: usize,
}

/// Per flat variable: its step in the chain (the last one assigning it,
/// as the interpreter's lookup has it), or `usize::MAX`.
fn positions(spec: &BooksSpec, n_vars: usize) -> Vec<usize> {
    let mut pos = vec![usize::MAX; n_vars];
    for (k, (v, _)) in spec.chain.iter().enumerate() {
        if let Some(p) = pos.get_mut(*v) {
            *p = k;
        }
    }
    pos
}

/// How many arguments a function's rule reads (a call with any other
/// number is declined: every node of a compiled integrand is checked, so
/// every step of the chain it reads is compiled too).
fn arity(f: Builtin) -> usize {
    match f {
        Builtin::Atan2 | Builtin::Min | Builtin::Max => 2,
        Builtin::Limit => 3,
        _ => 1,
    }
}

/// Whether `eval` of `e` computes what the lowering can (nothing it gives
/// NaN for or stops at).
fn value_ok(e: &Expr, s: &Sizes) -> bool {
    match e {
        Expr::Const(_) | Expr::Time => true,
        Expr::Name(_) | Expr::Der(_) => false,
        Expr::Var(v) | Expr::Pre(v) => (v.0 as usize) < s.n_vars,
        Expr::Param(p) => (p.0 as usize) < s.n_p,
        Expr::Table { table, args } => {
            (*table as usize) < s.n_tables
                && (1..=2).contains(&args.len())
                && args.iter().all(|a| value_ok(a, s))
        }
        Expr::Neg(a) | Expr::NoEvent(a) | Expr::Not(a) => value_ok(a, s),
        Expr::Binary(_, a, b) | Expr::Compare(_, a, b) | Expr::And(a, b) | Expr::Or(a, b) => {
            value_ok(a, s) && value_ok(b, s)
        }
        Expr::If(c, a, b) => value_ok(c, s) && value_ok(a, s) && value_ok(b, s),
        Expr::Call(f, args) => {
            !matches!(f, Builtin::Der | Builtin::Pre)
                && args.len() == arity(*f)
                && args.iter().all(|a| value_ok(a, s))
        }
    }
}

/// What the rates' check reads.
struct RateCx<'a> {
    spec: &'a BooksSpec,
    s: Sizes,
    pos: &'a [usize],
    /// per step of the chain: whether its dual number is always computed
    step_ok: &'a [bool],
}

/// Whether the dual number of `e` is always computed (never `Err`) when
/// it reads the steps of the chain before `upto`.
fn rate_ok(e: &Expr, cx: &RateCx<'_>, upto: usize) -> bool {
    let ok = |x: &Expr| rate_ok(x, cx, upto);
    match e {
        Expr::Const(_) | Expr::Time => true,
        Expr::Name(_) | Expr::Pre(_) | Expr::Der(_) => false,
        Expr::Param(p) => (p.0 as usize) < cx.s.n_p,
        Expr::Var(v) => {
            let k = v.0 as usize;
            if k >= cx.s.n_vars {
                return false;
            }
            let at = cx.pos[k];
            if at != usize::MAX {
                return at < upto && cx.step_ok[at];
            }
            match cx.spec.rates.get(k) {
                Some(RateSource::Y(i)) | Some(RateSource::NegY(i)) => *i < cx.s.n_y,
                Some(RateSource::Zero) => true,
                Some(RateSource::Computed) | None => false,
            }
        }
        Expr::Table { table, args } => {
            (*table as usize) < cx.s.n_tables
                && (1..=2).contains(&args.len())
                && args.iter().all(ok)
        }
        Expr::Neg(a) | Expr::NoEvent(a) | Expr::Not(a) => ok(a),
        Expr::Binary(_, a, b) | Expr::Compare(_, a, b) | Expr::And(a, b) | Expr::Or(a, b) => {
            ok(a) && ok(b)
        }
        Expr::If(c, a, b) => ok(c) && ok(a) && ok(b),
        Expr::Call(f, args) => {
            !matches!(f, Builtin::Der | Builtin::Pre)
                && args.len() == arity(*f)
                && args.iter().all(ok)
        }
    }
}

/// The steps of the chain `e` reads (directly).
fn steps_read(e: &Expr, pos: &[usize], out: &mut Vec<usize>) {
    e.walk(&mut |x| {
        if let Expr::Var(v) = x
            && let Some(&at) = pos.get(v.0 as usize)
            && at != usize::MAX
        {
            out.push(at);
        }
    });
}

/// Decides what is compiled of `spec` for a model of these sizes.
pub(crate) fn plan(spec: &BooksSpec, s: Sizes) -> Plan {
    let pos = positions(spec, s.n_vars);
    let mut step_ok = vec![false; spec.chain.len()];
    for k in 0..spec.chain.len() {
        let ok = {
            let cx = RateCx { spec, s, pos: &pos, step_ok: &step_ok };
            rate_ok(&spec.chain[k].1, &cx, k)
        };
        step_ok[k] = ok;
    }
    let cx = RateCx { spec, s, pos: &pos, step_ok: &step_ok };
    let in_out = |at: usize| at < spec.n_out;
    let mut nodes = 0;
    let mut size = |e: &Expr| {
        let mut n = 0;
        e.walk(&mut |_| n += 1);
        nodes += n;
    };
    let mut cover = vec![];
    let mut wanted: Vec<usize> = vec![];
    for p in &spec.parts {
        let mut c = BooksCover::default();
        if value_ok(&p.power, &s) && in_out(p.power_at) && in_out(p.abs_at) {
            c.power = true;
            size(&p.power);
        }
        if let (Some(e), Some(at)) = (&p.loss, p.loss_at)
            && value_ok(e, &s)
            && in_out(at)
        {
            c.loss = true;
            size(e);
        }
        if let (Some(e), Some(at)) = (&p.stored, p.stored_at)
            && p.exact
            && in_out(at)
            && rate_ok(e, &cx, spec.chain.len())
        {
            c.stored = true;
            size(e);
            steps_read(e, &pos, &mut wanted);
        }
        cover.push(c);
    }
    // the steps the covered rates read, through each other
    let mut need = vec![false; spec.chain.len()];
    while let Some(k) = wanted.pop() {
        if !std::mem::replace(&mut need[k], true) {
            steps_read(&spec.chain[k].1, &pos, &mut wanted);
        }
    }
    let steps: Vec<usize> = (0..spec.chain.len()).filter(|&k| need[k]).collect();
    debug_assert!(steps.iter().all(|&k| step_ok[k]), "a needed step was declined");
    for &k in &steps {
        size(&spec.chain[k].1);
    }
    Plan { cover, steps, nodes }
}

/// A rate: a constant the lowering knows, or a value of the code.
#[derive(Clone, Copy, Debug)]
enum R<V> {
    K(f64),
    V(V),
}

/// A value and its rate.
type Dual<V> = (V, R<V>);

/// The lowering of one spec.
struct Lower<'a, E: Emit> {
    e: E,
    spec: &'a BooksSpec,
    pos: Vec<usize>,
    ties: TieRule,
    /// what `Iterator::sum` of f64 starts from (a table's rate is such a
    /// sum)
    sum_start: f64,
    consts: HashMap<u64, E::V>,
    /// loads of the channels, the parameters and the rates of y
    loads: [HashMap<usize, E::V>; 3],
    /// per step of the chain: its value and rate, once computed
    steps: Vec<Option<Dual<E::V>>>,
}

impl<'a, E: Emit> Lower<'a, E> {
    fn cst(&mut self, x: f64) -> E::V {
        if let Some(v) = self.consts.get(&x.to_bits()) {
            return *v;
        }
        let v = self.e.konst(x);
        self.consts.insert(x.to_bits(), v);
        v
    }

    /// 0: a channel, 1: a parameter, 2: a rate of y
    fn load(&mut self, which: usize, i: usize) -> E::V {
        if let Some(v) = self.loads[which].get(&i) {
            return *v;
        }
        let arr = [Base::Y, Base::P, Base::V][which];
        let v = self.e.load(arr, i);
        self.loads[which].insert(i, v);
        v
    }

    fn mat(&mut self, r: R<E::V>) -> E::V {
        match r {
            R::K(x) => self.cst(x),
            R::V(v) => v,
        }
    }

    fn truth(&mut self, c: E::V) -> E::V {
        let one = self.cst(1.0);
        let zero = self.cst(0.0);
        self.e.select(c, one, zero)
    }

    /// Whether a truth value holds (non-zero; NaN counts as true).
    fn is_true(&mut self, x: E::V) -> E::V {
        let zero = self.cst(0.0);
        self.e.cmp(Cc::Ne, x, zero)
    }

    fn is_zero(&mut self, x: E::V) -> E::V {
        let zero = self.cst(0.0);
        self.e.cmp(Cc::Eq, x, zero)
    }

    /// `sign` as the interpreter has it (0 for NaN).
    fn sign(&mut self, x: E::V) -> E::V {
        let zero = self.cst(0.0);
        let one = self.cst(1.0);
        let minus = self.cst(-1.0);
        let pos = self.e.cmp(Cc::Gt, x, zero);
        let neg = self.e.cmp(Cc::Lt, x, zero);
        let m = self.e.select(neg, minus, zero);
        self.e.select(pos, one, m)
    }

    fn cc(op: CmpOp) -> Cc {
        match op {
            CmpOp::Lt => Cc::Lt,
            CmpOp::Le => Cc::Le,
            CmpOp::Gt => Cc::Gt,
            CmpOp::Ge => Cc::Ge,
        }
    }

    fn lib(f: Builtin) -> Option<Lib> {
        Some(match f {
            Builtin::Sin => Lib::Sin,
            Builtin::Cos => Lib::Cos,
            Builtin::Tan => Lib::Tan,
            Builtin::Asin => Lib::Asin,
            Builtin::Acos => Lib::Acos,
            Builtin::Atan => Lib::Atan,
            Builtin::Sinh => Lib::Sinh,
            Builtin::Cosh => Lib::Cosh,
            Builtin::Tanh => Lib::Tanh,
            Builtin::Exp => Lib::Exp,
            Builtin::Log => Lib::Log,
            _ => return None,
        })
    }

    /// Rust's `f64::max` (`max`) or `f64::min` as the interpreter calls
    /// them: the other argument when one is NaN, and between zeros of
    /// opposite signs the platform's rule (`lower.rs`'s `pick`).
    fn pick(&mut self, max: bool, a: E::V, b: E::V) -> E::V {
        let bnan = self.e.cmp(Cc::Uno, b, b);
        let (strict, wide) = if max { (Cc::Gt, Cc::Ge) } else { (Cc::Lt, Cc::Le) };
        match self.ties {
            TieRule::First => {
                let c1 = self.e.cmp(wide, a, b);
                let c = self.e.or(c1, bnan);
                self.e.select(c, a, b)
            }
            TieRule::Second => {
                let c1 = self.e.cmp(strict, a, b);
                let c = self.e.or(c1, bnan);
                self.e.select(c, a, b)
            }
            TieRule::SignAware => {
                let c1 = self.e.cmp(strict, a, b);
                let c = self.e.or(c1, bnan);
                let v = self.e.select(c, a, b);
                let eq = self.e.cmp(Cc::Eq, a, b);
                let z = if max { self.e.bits_and(a, b) } else { self.e.bits_or(a, b) };
                self.e.select(eq, z, v)
            }
        }
    }

    /// `e` by the rules of `lsim_ir::eval`, the variables the channels.
    fn value(&mut self, e: &Expr) -> E::V {
        match e {
            Expr::Const(x) => self.cst(*x),
            Expr::Time => self.e.time(),
            Expr::Var(v) | Expr::Pre(v) => self.load(0, v.0 as usize),
            Expr::Param(p) => self.load(1, p.0 as usize),
            Expr::Name(_) | Expr::Der(_) => unreachable!("declined"),
            Expr::Table { table, args } => {
                let at: Vec<E::V> = args.iter().map(|a| self.value(a)).collect();
                // (the value `eval_table` gives, `Table::eval`'s: the value
                // alone, `Table::value`, differs where a slope overflows)
                self.e.table(*table, &at, true).0
            }
            Expr::Neg(a) => {
                let a = self.value(a);
                self.e.neg(a)
            }
            Expr::NoEvent(a) => self.value(a),
            Expr::Binary(op, a, b) => {
                let (a, b) = (self.value(a), self.value(b));
                match op {
                    BinaryOp::Add => self.e.add(a, b),
                    BinaryOp::Sub => self.e.sub(a, b),
                    BinaryOp::Mul => self.e.mul(a, b),
                    BinaryOp::Div => self.e.div(a, b),
                    BinaryOp::Pow => self.e.call(Lib::Pow, &[a, b]),
                }
            }
            Expr::Call(f, args) => {
                let a = self.value(&args[0]);
                if let Some(l) = Self::lib(*f) {
                    return self.e.call(l, &[a]);
                }
                match f {
                    Builtin::Sqrt => self.e.sqrt(a),
                    Builtin::Abs => self.e.abs(a),
                    Builtin::Sign => self.sign(a),
                    Builtin::Atan2 => {
                        let b = self.value(&args[1]);
                        self.e.call(Lib::Atan2, &[a, b])
                    }
                    Builtin::Min | Builtin::Max => {
                        let b = self.value(&args[1]);
                        self.pick(*f == Builtin::Max, a, b)
                    }
                    Builtin::Limit => {
                        let lo = self.value(&args[1]);
                        let m = self.pick(true, a, lo);
                        let hi = self.value(&args[2]);
                        self.pick(false, m, hi)
                    }
                    _ => unreachable!("declined or a library function"),
                }
            }
            Expr::Compare(op, a, b) => {
                let (a, b) = (self.value(a), self.value(b));
                let c = self.e.cmp(Self::cc(*op), a, b);
                self.truth(c)
            }
            Expr::And(a, b) | Expr::Or(a, b) => {
                let (a, b) = (self.value(a), self.value(b));
                let (ca, cb) = (self.is_true(a), self.is_true(b));
                let c =
                    if matches!(e, Expr::And(..)) { self.e.and(ca, cb) } else { self.e.or(ca, cb) };
                self.truth(c)
            }
            Expr::Not(a) => {
                let a = self.value(a);
                let c = self.is_zero(a);
                self.truth(c)
            }
            Expr::If(c, a, b) => {
                let c = self.value(c);
                let c = self.is_true(c);
                let (a, b) = (self.value(a), self.value(b));
                self.e.select(c, a, b)
            }
        }
    }

    // the rates' arithmetic, constants folded

    fn add_r(&mut self, a: R<E::V>, b: R<E::V>) -> R<E::V> {
        if let (R::K(x), R::K(y)) = (a, b) {
            return R::K(x + y);
        }
        let (x, y) = (self.mat(a), self.mat(b));
        R::V(self.e.add(x, y))
    }

    fn sub_r(&mut self, a: R<E::V>, b: R<E::V>) -> R<E::V> {
        if let (R::K(x), R::K(y)) = (a, b) {
            return R::K(x - y);
        }
        let (x, y) = (self.mat(a), self.mat(b));
        R::V(self.e.sub(x, y))
    }

    fn neg_r(&mut self, a: R<E::V>) -> R<E::V> {
        match a {
            R::K(x) => R::K(-x),
            R::V(v) => R::V(self.e.neg(v)),
        }
    }

    /// `ad::times(f, d)`: `f` is computed only when `d` may be non-zero.
    fn times(&mut self, f: impl FnOnce(&mut Self) -> E::V, d: R<E::V>) -> R<E::V> {
        match d {
            // (-0 too)
            R::K(x) => {
                if x == 0.0 {
                    return R::K(0.0);
                }
                let f = f(self);
                let k = self.cst(x);
                R::V(self.e.mul(f, k))
            }
            R::V(dv) => {
                let f = f(self);
                let p = self.e.mul(f, dv);
                let z = self.is_zero(dv);
                let zero = self.cst(0.0);
                R::V(self.e.select(z, zero, p))
            }
        }
    }

    /// `if c { a } else { b }` of two rates.
    fn select_r(&mut self, c: E::V, a: R<E::V>, b: R<E::V>) -> R<E::V> {
        if let (R::K(x), R::K(y)) = (a, b)
            && x.to_bits() == y.to_bits()
        {
            return R::K(x);
        }
        let (x, y) = (self.mat(a), self.mat(b));
        R::V(self.e.select(c, x, y))
    }

    /// `if d == 0 { first } else { second }` for a rate `d`, each branch
    /// computed only where it may be taken.
    fn by_zero(
        &mut self,
        d: R<E::V>,
        first: impl FnOnce(&mut Self) -> R<E::V>,
        second: impl FnOnce(&mut Self) -> R<E::V>,
    ) -> R<E::V> {
        match d {
            // (-0 too)
            R::K(x) => {
                if x == 0.0 {
                    first(self)
                } else {
                    second(self)
                }
            }
            R::V(dv) => {
                let a = first(self);
                let b = second(self);
                let z = self.is_zero(dv);
                self.select_r(z, a, b)
            }
        }
    }

    /// The dual number of variable `v`.
    fn var(&mut self, v: usize) -> Dual<E::V> {
        let at = self.pos[v];
        if at != usize::MAX {
            return self.steps[at].expect("the chain's steps come first, in order");
        }
        let x = self.load(0, v);
        let d = match self.spec.rates[v] {
            RateSource::Y(i) => R::V(self.load(2, i)),
            RateSource::NegY(i) => {
                let r = self.load(2, i);
                R::V(self.e.neg(r))
            }
            RateSource::Zero => R::K(0.0),
            RateSource::Computed => unreachable!("declined"),
        };
        (x, d)
    }

    /// `e` and its rate by the rules of lsim-solve's `ad::dual`.
    fn dual(&mut self, e: &Expr) -> Dual<E::V> {
        let k0 = R::K(0.0);
        match e {
            Expr::Const(x) => (self.cst(*x), k0),
            Expr::Param(p) => (self.load(1, p.0 as usize), k0),
            Expr::Var(v) => self.var(v.0 as usize),
            Expr::Time => (self.e.time(), R::K(1.0)),
            Expr::Name(_) | Expr::Pre(_) | Expr::Der(_) => unreachable!("declined"),
            Expr::Table { table, args } => {
                let at: Vec<Dual<E::V>> = args.iter().map(|a| self.dual(a)).collect();
                let vals: Vec<E::V> = at.iter().map(|a| a.0).collect();
                let (v, g) = self.e.table(*table, &vals, true);
                // Σ times(g_j, da_j), as `Iterator::sum` adds them
                let mut d = R::K(self.sum_start);
                for (j, (_, da)) in at.iter().enumerate() {
                    let gj = g[j].expect("asked for");
                    let t = self.times(|_| gj, *da);
                    d = self.add_r(d, t);
                }
                (v, d)
            }
            Expr::Neg(a) => {
                let (v, d) = self.dual(a);
                let nv = self.e.neg(v);
                (nv, self.neg_r(d))
            }
            Expr::NoEvent(a) => self.dual(a),
            Expr::Binary(op, a, b) => {
                let (a, b) = (self.dual(a), self.dual(b));
                self.binary(*op, a, b)
            }
            Expr::Compare(op, a, b) => {
                let (a, b) = (self.dual(a).0, self.dual(b).0);
                let c = self.e.cmp(Self::cc(*op), a, b);
                (self.truth(c), k0)
            }
            Expr::And(a, b) | Expr::Or(a, b) => {
                let (a, b) = (self.dual(a).0, self.dual(b).0);
                let (ca, cb) = (self.is_true(a), self.is_true(b));
                let c =
                    if matches!(e, Expr::And(..)) { self.e.and(ca, cb) } else { self.e.or(ca, cb) };
                (self.truth(c), k0)
            }
            Expr::Not(a) => {
                let a = self.dual(a).0;
                let c = self.is_zero(a);
                (self.truth(c), k0)
            }
            Expr::If(c, a, b) => {
                let c = self.dual(c).0;
                let c = self.is_true(c);
                let (a, b) = (self.dual(a), self.dual(b));
                let v = self.e.select(c, a.0, b.0);
                (v, self.select_r(c, a.1, b.1))
            }
            Expr::Call(f, args) => self.call(*f, args),
        }
    }

    fn binary(&mut self, op: BinaryOp, (av, ad): Dual<E::V>, (bv, bd): Dual<E::V>) -> Dual<E::V> {
        match op {
            BinaryOp::Add => (self.e.add(av, bv), self.add_r(ad, bd)),
            BinaryOp::Sub => (self.e.sub(av, bv), self.sub_r(ad, bd)),
            BinaryOp::Mul => {
                let v = self.e.mul(av, bv);
                let x = self.times(|_| bv, ad);
                let y = self.times(|_| av, bd);
                (v, self.add_r(x, y))
            }
            BinaryOp::Div => {
                let q = self.e.div(av, bv);
                // da = if a.d == 0 { 0 } else { a.d / b.v }
                let da = self.by_zero(
                    ad,
                    |_| R::K(0.0),
                    |s| {
                        let x = s.mat(ad);
                        R::V(s.e.div(x, bv))
                    },
                );
                let t = self.times(|s| s.e.div(q, bv), bd);
                (q, self.sub_r(da, t))
            }
            BinaryOp::Pow => {
                let (x, y) = (av, bv);
                let f0 = self.e.call(Lib::Pow, &[x, y]);
                let d = self.by_zero(
                    bd,
                    // x^c: c x^(c-1) (0 for c = 0)
                    |s| {
                        s.times(
                            |s| {
                                let one = s.cst(1.0);
                                let ym1 = s.e.sub(y, one);
                                let p = s.e.call(Lib::Pow, &[x, ym1]);
                                let f1 = s.e.mul(y, p);
                                let yz = s.is_zero(y);
                                let zero = s.cst(0.0);
                                s.e.select(yz, zero, f1)
                            },
                            ad,
                        )
                    },
                    // x^y: (y f0 / x) dx + (f0 ln x) dy, zero where it is
                    // zero; at x = 0 the derivative in x from x's own power
                    |s| {
                        let fa = |s: &mut Self| {
                            let one = s.cst(1.0);
                            let ym1 = s.e.sub(y, one);
                            let p = s.e.call(Lib::Pow, &[x, ym1]);
                            let at0 = s.e.mul(y, p);
                            let yf = s.e.mul(y, f0);
                            let other = s.e.div(yf, x);
                            let xz = s.is_zero(x);
                            s.e.select(xz, at0, other)
                        };
                        let ta = s.times(fa, ad);
                        let fb = |s: &mut Self| {
                            let l = s.e.call(Lib::Log, &[x]);
                            let p = s.e.mul(f0, l);
                            let fz = s.is_zero(f0);
                            let zero = s.cst(0.0);
                            s.e.select(fz, zero, p)
                        };
                        let tb = s.times(fb, bd);
                        s.add_r(ta, tb)
                    },
                );
                (f0, d)
            }
        }
    }

    fn call(&mut self, f: Builtin, args: &[Expr]) -> Dual<E::V> {
        let (x, ad) = self.dual(&args[0]);
        let unary = |s: &mut Self, l: Lib| s.e.call(l, &[x]);
        let one = |s: &mut Self| s.cst(1.0);
        match f {
            Builtin::Der | Builtin::Pre => unreachable!("declined"),
            Builtin::Sin => {
                let v = unary(self, Lib::Sin);
                (v, self.times(|s| unary(s, Lib::Cos), ad))
            }
            Builtin::Cos => {
                let v = unary(self, Lib::Cos);
                let d = self.times(
                    |s| {
                        let si = unary(s, Lib::Sin);
                        s.e.neg(si)
                    },
                    ad,
                );
                (v, d)
            }
            Builtin::Tan => {
                let t = unary(self, Lib::Tan);
                let d = self.times(
                    |s| {
                        let t2 = s.e.mul(t, t);
                        let o = one(s);
                        s.e.add(o, t2)
                    },
                    ad,
                );
                (t, d)
            }
            Builtin::Asin | Builtin::Acos => {
                let asin = f == Builtin::Asin;
                let v = unary(self, if asin { Lib::Asin } else { Lib::Acos });
                let d = self.times(
                    |s| {
                        let x2 = s.e.mul(x, x);
                        let o = one(s);
                        let r = s.e.sub(o, x2);
                        let r = s.e.sqrt(r);
                        let n = s.cst(if asin { 1.0 } else { -1.0 });
                        s.e.div(n, r)
                    },
                    ad,
                );
                (v, d)
            }
            Builtin::Atan => {
                let v = unary(self, Lib::Atan);
                let d = self.times(
                    |s| {
                        let x2 = s.e.mul(x, x);
                        let o = one(s);
                        let r = s.e.add(o, x2);
                        s.e.div(o, r)
                    },
                    ad,
                );
                (v, d)
            }
            Builtin::Atan2 => {
                let (bv, bd) = self.dual(&args[1]);
                let v = self.e.call(Lib::Atan2, &[x, bv]);
                // if a.d == 0 && b.d == 0 { 0 } else { (times(b, da) -
                // times(a, db)) / (b² + a²) }
                let general = |s: &mut Self| {
                    let b2 = s.e.mul(bv, bv);
                    let a2 = s.e.mul(x, x);
                    let r2 = s.e.add(b2, a2);
                    let t1 = s.times(|_| bv, ad);
                    let t2 = s.times(|_| x, bd);
                    let n = s.sub_r(t1, t2);
                    let n = s.mat(n);
                    s.e.div(n, r2)
                };
                let d = match (ad, bd) {
                    (R::K(p), R::K(q)) if p == 0.0 && q == 0.0 => R::K(0.0),
                    (R::K(p), _) | (_, R::K(p)) if p != 0.0 => R::V(general(self)),
                    _ => {
                        let g = general(self);
                        let az = match ad {
                            R::K(_) => None,
                            R::V(a) => Some(self.is_zero(a)),
                        };
                        let bz = match bd {
                            R::K(_) => None,
                            R::V(b) => Some(self.is_zero(b)),
                        };
                        // (a constant rate here is zero)
                        let both = match (az, bz) {
                            (Some(a), Some(b)) => self.e.and(a, b),
                            (Some(a), None) => a,
                            (None, Some(b)) => b,
                            (None, None) => unreachable!("both constant: above"),
                        };
                        let zero = self.cst(0.0);
                        R::V(self.e.select(both, zero, g))
                    }
                };
                (v, d)
            }
            Builtin::Sinh => {
                let v = unary(self, Lib::Sinh);
                (v, self.times(|s| unary(s, Lib::Cosh), ad))
            }
            Builtin::Cosh => {
                let v = unary(self, Lib::Cosh);
                (v, self.times(|s| unary(s, Lib::Sinh), ad))
            }
            Builtin::Tanh => {
                let t = unary(self, Lib::Tanh);
                let d = self.times(
                    |s| {
                        let t2 = s.e.mul(t, t);
                        let o = one(s);
                        s.e.sub(o, t2)
                    },
                    ad,
                );
                (t, d)
            }
            Builtin::Exp => {
                let ex = unary(self, Lib::Exp);
                (ex, self.times(|_| ex, ad))
            }
            Builtin::Log => {
                let v = unary(self, Lib::Log);
                let d = self.times(
                    |s| {
                        let o = one(s);
                        s.e.div(o, x)
                    },
                    ad,
                );
                (v, d)
            }
            Builtin::Sqrt => {
                let r = self.e.sqrt(x);
                let d = self.times(
                    |s| {
                        let h = s.cst(0.5);
                        s.e.div(h, r)
                    },
                    ad,
                );
                (r, d)
            }
            Builtin::Abs => {
                let v = self.e.abs(x);
                (v, self.times(|s| s.sign(x), ad))
            }
            Builtin::Sign => (self.sign(x), R::K(0.0)),
            Builtin::Min | Builtin::Max => {
                let (bv, bd) = self.dual(&args[1]);
                // b when it is smaller (larger), else a: NaNs pick a
                let cc = if f == Builtin::Min { Cc::Lt } else { Cc::Gt };
                let take_b = self.e.cmp(cc, bv, x);
                let v = self.e.select(take_b, bv, x);
                (v, self.select_r(take_b, bd, ad))
            }
            Builtin::Limit => {
                let (lo, lod) = self.dual(&args[1]);
                let (hi, hid) = self.dual(&args[2]);
                let below = self.e.cmp(Cc::Lt, x, lo);
                let above = self.e.cmp(Cc::Gt, x, hi);
                let v = self.e.select(above, hi, x);
                let d = self.select_r(above, hid, ad);
                let v = self.e.select(below, lo, v);
                (v, self.select_r(below, lod, d))
            }
        }
    }
}

/// Emits the covered integrands of `spec` into `e` (the channels read as
/// `y`, the rates of y as `v`, the integrands written to `out`).
pub(crate) fn emit<E: Emit>(e: E, spec: &BooksSpec, plan: &Plan, n_vars: usize) -> E {
    let mut lw = Lower {
        e,
        spec,
        pos: positions(spec, n_vars),
        ties: tie_rule(),
        sum_start: std::iter::empty::<f64>().sum(),
        consts: HashMap::new(),
        loads: [HashMap::new(), HashMap::new(), HashMap::new()],
        steps: vec![None; spec.chain.len()],
    };
    for (p, c) in spec.parts.iter().zip(&plan.cover) {
        if c.power {
            let v = lw.value(&p.power);
            lw.e.store(Base::Out, p.power_at, v);
            let a = lw.e.abs(v);
            lw.e.store(Base::Out, p.abs_at, a);
        }
        if c.loss
            && let (Some(l), Some(at)) = (&p.loss, p.loss_at)
        {
            let v = lw.value(l);
            lw.e.store(Base::Out, at, v);
        }
    }
    for &k in &plan.steps {
        let d = lw.dual(&spec.chain[k].1);
        lw.steps[k] = Some(d);
    }
    for (p, c) in spec.parts.iter().zip(&plan.cover) {
        if c.stored
            && let (Some(s), Some(at)) = (&p.stored, p.stored_at)
        {
            let (_, d) = lw.dual(s);
            let d = lw.mat(d);
            lw.e.store(Base::Out, at, d);
        }
    }
    lw.e
}

/// How a model's books compile: what its own compilation was told.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Settings {
    /// machine code at all (no: the system refused executable memory for
    /// the model, or a test denies it)
    pub machine_code: bool,
    pub deny_executable_memory: bool,
    /// above this many nodes: Cranelift's fast settings
    pub fast_above: usize,
}

/// Whether two specs are the same to the bit (constants compared by their
/// bits: `-0` is not `0`, and a NaN is itself).
pub(crate) fn same_spec(a: &BooksSpec, b: &BooksSpec) -> bool {
    let opt = |x: &Option<Expr>, y: &Option<Expr>| match (x, y) {
        (Some(x), Some(y)) => same_expr(x, y),
        (None, None) => true,
        _ => false,
    };
    a.n_out == b.n_out
        && a.rates == b.rates
        && a.parts.len() == b.parts.len()
        && a.parts.iter().zip(&b.parts).all(|(p, q)| {
            (p.exact, p.power_at, p.abs_at, p.loss_at, p.stored_at)
                == (q.exact, q.power_at, q.abs_at, q.loss_at, q.stored_at)
                && same_expr(&p.power, &q.power)
                && opt(&p.loss, &q.loss)
                && opt(&p.stored, &q.stored)
        })
        && a.chain.len() == b.chain.len()
        && a.chain.iter().zip(&b.chain).all(|((v, x), (w, y))| v == w && same_expr(x, y))
}

/// [`same_spec`] of two expressions.
fn same_expr(a: &Expr, b: &Expr) -> bool {
    use Expr as E;
    let all = |x: &[Expr], y: &[Expr]| {
        x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same_expr(p, q))
    };
    match (a, b) {
        (E::Const(x), E::Const(y)) => x.to_bits() == y.to_bits(),
        (E::Time, E::Time) => true,
        (E::Name(x), E::Name(y)) => x == y,
        (E::Var(x), E::Var(y)) | (E::Der(x), E::Der(y)) | (E::Pre(x), E::Pre(y)) => x == y,
        (E::Param(x), E::Param(y)) => x == y,
        (E::Neg(x), E::Neg(y)) | (E::Not(x), E::Not(y)) | (E::NoEvent(x), E::NoEvent(y)) => {
            same_expr(x, y)
        }
        (E::Binary(o, x1, x2), E::Binary(p, y1, y2)) => {
            o == p && same_expr(x1, y1) && same_expr(x2, y2)
        }
        (E::Compare(o, x1, x2), E::Compare(p, y1, y2)) => {
            o == p && same_expr(x1, y1) && same_expr(x2, y2)
        }
        (E::And(x1, x2), E::And(y1, y2)) | (E::Or(x1, x2), E::Or(y1, y2)) => {
            same_expr(x1, y1) && same_expr(x2, y2)
        }
        (E::If(x1, x2, x3), E::If(y1, y2, y3)) => {
            same_expr(x1, y1) && same_expr(x2, y2) && same_expr(x3, y3)
        }
        (E::Call(f, x), E::Call(g, y)) => f == g && all(x, y),
        (E::Table { table: k, args: x }, E::Table { table: l, args: y }) => k == l && all(x, y),
        _ => false,
    }
}

/// A spec's compiled kernels, and the model's cache of them.
pub(crate) type Cache = Mutex<Vec<(BooksSpec, Option<Arc<CompiledBooks>>)>>;

/// The machine code of a spec's books, compiled on another thread.
#[derive(Default)]
struct Upgrade {
    /// the memory the machine code lives in, set before the code switches
    /// to it
    memory: OnceLock<Arc<CodeMemory>>,
    done: Mutex<Option<Result<(), String>>>,
    over: Condvar,
}

impl Upgrade {
    fn finish(&self, r: Result<(), String>) {
        *self.done.lock().unwrap_or_else(|e| e.into_inner()) = Some(r);
        self.over.notify_all();
    }
}

/// The energy books' integrands of one spec compiled for one model
/// ([`lsim_ir::BooksKernel`]): on a tape at once, as machine code once
/// another thread has compiled it (bitwise the same either way).
pub struct CompiledBooks {
    code: Code,
    cover: Vec<BooksCover>,
    scratch: usize,
    n_vars: usize,
    n_p: usize,
    n_y: usize,
    n_out: usize,
    tables: Arc<TableStore>,
    upgrade: Option<Arc<Upgrade>>,
    _cancel: Option<Arc<CancelOnDrop>>,
}

impl CompiledBooks {
    /// Compiles what can be of `spec` for a model of these sizes reading
    /// `tables`; `None` when nothing can.
    pub(crate) fn new(
        spec: &BooksSpec,
        s: Sizes,
        tables: Arc<TableStore>,
        settings: Settings,
    ) -> Option<CompiledBooks> {
        let plan = plan(spec, s);
        if plan.cover.iter().all(|c| *c == BooksCover::default()) {
            return None;
        }
        let tape = emit(Recorder::default(), spec, &plan, s.n_vars).finish();
        let scratch = tape.regs;
        let taped = Code::Tape { tape: Arc::new(tape), regs_at: 0 };
        let mut books = CompiledBooks {
            code: taped,
            cover: plan.cover.clone(),
            scratch,
            n_vars: s.n_vars,
            n_p: s.n_p,
            n_y: s.n_y,
            n_out: spec.n_out,
            tables,
            upgrade: None,
            _cancel: None,
        };
        if settings.machine_code {
            books.upgrade(spec, plan, s.n_vars, settings);
        }
        Some(books)
    }

    /// Compiles the machine code on another thread, to which the code
    /// switches once it is in.
    fn upgrade(&mut self, spec: &BooksSpec, plan: Plan, n_vars: usize, settings: Settings) {
        let tiered =
            Arc::new(TieredCode { tape: std::mem::take(&mut self.code), machine: OnceLock::new() });
        self.code = Code::Tiered(tiered.clone());
        let up = Arc::new(Upgrade::default());
        self.upgrade = Some(up.clone());
        let cancel = Arc::new(AtomicBool::new(false));
        self._cancel = Some(Arc::new(CancelOnDrop(cancel.clone())));
        let spec = spec.clone();
        let up2 = up.clone();
        let spawned =
            std::thread::Builder::new().name("lsim-codegen-books".into()).spawn(move || {
                let _env = DefaultFloatEnv::enter();
                let compiled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    machine(&spec, &plan, n_vars, settings, &cancel)
                }));
                let r = match compiled {
                    Ok(r) => r.map(|(memory, f)| {
                        let _ = up2.memory.set(memory);
                        let _ = tiered.machine.set(Code::Machine(vec![f]));
                    }),
                    Err(_) => Err("internal: the books' compilation panicked".to_string()),
                };
                if let Err(e) = &r
                    && !cancel.load(Ordering::Relaxed)
                    && std::env::var_os("LSIM_CODEGEN_TRACE").is_some()
                {
                    eprintln!("lsim-codegen: the energy books stay on their tape: {e}");
                }
                up2.finish(r);
            });
        if let Err(e) = spawned {
            up.finish(Err(format!("no thread to compile on: {e}")));
        }
    }

    /// Waits for the machine code: `Ok` once it runs as machine code, or
    /// why it stays on its tape; `None` for books on a tape for good.
    pub fn wait_machine_code(&self) -> Option<Result<(), String>> {
        let u = self.upgrade.as_ref()?;
        let mut done = u.done.lock().unwrap_or_else(|e| e.into_inner());
        while done.is_none() {
            done = u.over.wait(done).unwrap_or_else(|e| e.into_inner());
        }
        done.clone()
    }

    /// The same books on their tape alone (for tests of the tiers).
    #[doc(hidden)]
    pub fn tape_only(&self) -> CompiledBooks {
        let code = match &self.code {
            Code::Tiered(t) => t.tape.clone(),
            c => c.clone(),
        };
        CompiledBooks {
            code,
            cover: self.cover.clone(),
            scratch: self.scratch,
            n_vars: self.n_vars,
            n_p: self.n_p,
            n_y: self.n_y,
            n_out: self.n_out,
            tables: self.tables.clone(),
            upgrade: None,
            _cancel: None,
        }
    }

    /// Whether they run as machine code now.
    pub fn machine_code_ready(&self) -> bool {
        match &self.code {
            Code::Tiered(t) => t.machine.get().is_some(),
            _ => false,
        }
    }
}

impl BooksKernel for CompiledBooks {
    fn cover(&self, j: usize) -> BooksCover {
        self.cover.get(j).copied().unwrap_or_default()
    }

    fn scratch(&self) -> usize {
        self.scratch
    }

    fn eval(
        &self,
        t: f64,
        vars: &[f64],
        ydot: &[f64],
        p: &[f64],
        work: &mut [f64],
        out: &mut [f64],
    ) {
        // (the code reads and writes within these lengths, unchecked)
        assert!(
            vars.len() == self.n_vars && p.len() == self.n_p && ydot.len() >= self.n_y,
            "input vectors do not match the compiled books' layout"
        );
        assert!(out.len() >= self.n_out, "the integrands' vector is too short");
        assert!(work.len() >= self.scratch, "the compiled books' scratch is too short");
        let inp = EvalInput { t, y: vars, p, d: &[], u: &[] };
        self.code.call(&inp, ydot, work, out, &self.tables);
    }
}

/// Compiles the books' machine code: its memory and its function.
fn machine(
    spec: &BooksSpec,
    plan: &Plan,
    n_vars: usize,
    settings: Settings,
    cancel: &AtomicBool,
) -> Result<(Arc<CodeMemory>, RawFn), String> {
    let fast = plan.nodes > settings.fast_above;
    let (opt, ra) = if fast { ("none", "single_pass") } else { ("speed", "backtracking") };
    let isa = jit::isa_for(None, opt, ra).map_err(|e| e.to_string())?;
    let (mut module, decls) =
        jit::module(&isa, settings.deny_executable_memory).map_err(|e| e.to_string())?;
    let sig = jit::eval_signature(&*isa);
    let id = module.declare_function("books", Linkage::Local, &sig).map_err(|e| e.to_string())?;
    let fc = isa.frontend_config();
    let build = |_: usize, _: usize, id: FuncId| -> Result<Function, CodegenError> {
        let mut fctx = FunctionBuilderContext::new();
        let mut func =
            Function::with_name_signature(UserFuncName::user(0, id.as_u32()), sig.clone());
        {
            let mut b = FunctionBuilder::new(&mut func, &mut fctx);
            let block = b.create_block();
            b.append_block_params_for_function_params(block);
            b.switch_to_block(block);
            let ctx = b.block_params(block)[0];
            {
                let e = Clif::new(&mut b, ctx, &decls);
                emit(e, spec, plan, n_vars);
            }
            b.ins().return_(&[]);
            b.seal_all_blocks();
            b.finalize(fc);
        }
        Ok(func)
    };
    let (compiled, _, _) =
        jit::build_and_compile(&isa, vec![(0, 0, id, plan.nodes)], 1, Some(cancel), build)
            .map_err(|e| e.to_string())?;
    // from here on the memory is given back however this returns
    let mut memory = CodeMemory(Some(module));
    {
        let module = memory.0.as_mut().expect("the module lives until the memory is dropped");
        for c in &compiled {
            module
                .define_function_bytes(c.id, c.align, &c.bytes, &c.relocs)
                .map_err(|e| no_executable_memory(&e))?;
        }
        module.finalize_definitions().map_err(|e| no_executable_memory(&e))?;
    }
    let memory = Arc::new(memory);
    let ptr = memory.module().get_finalized_function(id);
    // SAFETY: a function just compiled with exactly this signature
    // (`eval_signature`).
    let f = unsafe { std::mem::transmute::<*const u8, RawFn>(ptr) };
    Ok((memory, f))
}
