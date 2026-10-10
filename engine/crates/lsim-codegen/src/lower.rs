//! Lowering of flat expressions, with forward-mode tangents (dual numbers)
//! over one or many directions, into an [`Emit`] backend: Cranelift IR or
//! a tape.
//!
//! Values cross assignments through memory: each assignment's value (and
//! tangent) is kept in a register only within a short *segment* of
//! assignments with no call out of the code, and written to the `work`
//! buffer when a later segment reads it. Live ranges stay short, so
//! register allocation is linear in the size of the model and nothing is
//! spilled around the math library's calls (which clobber every floating
//! point register); memory traffic is one store per value read far away
//! and one L1 load per such read.
//!
//! Tangents are sparse: each value carries the directions it depends on
//! structurally, so code is generated only for derivatives that can be
//! non-zero. A Jacobian-vector product has one direction (read from `v`);
//! the coloured Jacobian has one per colour, seeded with an exact 1.
//!
//! **Semantics.** Every operation is the reference interpreter's
//! (`lsim_ir::eval`) in the IR's order: no operation is reordered or fused,
//! and the library functions are the interpreter's own. Two shortcuts are
//! taken where [`Exact::Fast`] allows them (the residual, the Jacobian,
//! the channels, the initialisation): a constant integer power is
//! multiplied out (within one rounding of the exact power, where `pow`
//! is within an ulp of it), and `x^0.5` is a square root. The functions
//! the run loop compares with the interpreter (the zero crossings, the
//! modes, the `when` clauses, the table guards, the condition kernels)
//! take none ([`Exact::Interpreter`]): they are bitwise the interpreter.

use crate::CodegenError;
use crate::analysis::{Ctx, Src, System, inline_power};
use crate::backend::{Base, Cc, Emit, Lib};
use lsim_ir::expr::{BinaryOp, Builtin, CmpOp, Expr};
use std::collections::HashMap;
use std::sync::OnceLock;

/// Which derivatives a function computes.
#[derive(Clone, Copy, Debug)]
pub(crate) enum TanMode<'a> {
    /// values only
    None,
    /// one direction, read from `v`
    Jvp,
    /// one direction per colour; `y[j]` seeds direction `colour[j]` with 1
    Colours(&'a [u32]),
}

/// Whether a function may take the arithmetic shortcuts (module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Exact {
    /// bitwise the interpreter
    Interpreter,
    /// constant powers multiplied out
    Fast,
}

/// How the platform's `f64::max` and `f64::min` (the interpreter's) treat
/// a tie between zeros of opposite signs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TieRule {
    /// the first argument (x86-64)
    First,
    /// the second argument
    Second,
    /// `max` gives +0, `min` −0 (IEEE 754-2019 `maximumNumber`, arm64)
    SignAware,
}

/// The interpreter's rule, found by asking it once.
pub(crate) fn tie_rule() -> TieRule {
    static RULE: OnceLock<TieRule> = OnceLock::new();
    *RULE.get_or_init(|| {
        let negative = |f: Builtin, a: f64, b: f64| {
            let e = Expr::Call(f, vec![Expr::Const(a), Expr::Const(b)]);
            let env = lsim_ir::eval::SliceEnv { t: 0.0, vars: &[], ders: &[], params: &[] };
            lsim_ir::eval::eval(&e, &env).is_sign_negative()
        };
        // (whether the result is −0)
        let max = [negative(Builtin::Max, -0.0, 0.0), negative(Builtin::Max, 0.0, -0.0)];
        let min = [negative(Builtin::Min, -0.0, 0.0), negative(Builtin::Min, 0.0, -0.0)];
        match (max, min) {
            ([true, false], [true, false]) => TieRule::First,
            ([false, true], [false, true]) => TieRule::Second,
            ([false, false], [true, true]) => TieRule::SignAware,
            // (an unknown rule: the differential tests say so)
            _ => TieRule::First,
        }
    })
}

/// One tangent entry: exactly one, or a computed value.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Tv<V> {
    One,
    V(V),
}

/// A sparse tangent: (direction, entry), directions increasing.
pub(crate) type Tan<V> = Vec<(u32, Tv<V>)>;

/// A value and its tangent.
#[derive(Clone, Debug)]
pub(crate) struct D<V> {
    pub v: V,
    pub t: Tan<V>,
}

/// Where each assignment's tangent lives in `work` when it is kept: fixed
/// before any code is generated, so the chunks of a function can be built
/// independently (and in parallel).
pub(crate) struct TanLayout {
    /// per assignment: the first slot and the directions stored there
    /// (structurally possible ones; an entry the code finds zero is
    /// stored as zero)
    pub slots: Vec<(u32, Vec<u32>)>,
    /// the end of the slots in `work`
    pub end: usize,
}

impl TanLayout {
    /// The layout for a tangent mode: one direction after the values
    /// (`work[n + k]`), or the colours of each assignment's dependencies.
    pub(crate) fn new(sys: &System<'_>, mode: TanMode<'_>) -> TanLayout {
        let n = sys.exprs.len();
        match mode {
            TanMode::None => TanLayout { slots: vec![], end: n },
            TanMode::Jvp => TanLayout {
                slots: (0..n)
                    .map(|k| {
                        let dirs = if sys.ydeps[k].is_empty() { vec![] } else { vec![0] };
                        ((n + k) as u32, dirs)
                    })
                    .collect(),
                end: 2 * n,
            },
            TanMode::Colours(col) => {
                let mut off = n;
                let slots = (0..n)
                    .map(|k| {
                        let mut dirs: Vec<u32> =
                            sys.ydeps[k].iter().map(|&j| col[j as usize]).collect();
                        dirs.sort_unstable();
                        dirs.dedup();
                        let at = off as u32;
                        off += dirs.len();
                        (at, dirs)
                    })
                    .collect();
                TanLayout { slots, end: off }
            }
        }
    }
}

/// A per-segment cache over a dense index range.
struct SegCache<T: Clone> {
    stamp: u32,
    items: Vec<(u32, Option<T>)>,
}

impl<T: Clone> SegCache<T> {
    fn new() -> Self {
        SegCache { stamp: 1, items: vec![] }
    }

    fn get(&self, i: usize) -> Option<&T> {
        match self.items.get(i) {
            Some((g, Some(x))) if *g == self.stamp => Some(x),
            _ => None,
        }
    }

    fn put(&mut self, i: usize, x: T) {
        if i >= self.items.len() {
            self.items.resize(i + 1, (0, None));
        }
        self.items[i] = (self.stamp, Some(x));
    }

    fn clear(&mut self) {
        self.stamp += 1;
    }
}

/// What a function's lowering needs besides its backend.
pub(crate) struct LwSetup<'a> {
    pub cx: &'a Ctx<'a>,
    pub sys: &'a System<'a>,
    pub mode: TanMode<'a>,
    pub exact: Exact,
    pub fma: bool,
    pub keep: &'a [bool],
    pub tan: &'a TanLayout,
}

/// The state of lowering one function.
pub(crate) struct Lw<'a, E: Emit> {
    pub e: E,
    pub cx: &'a Ctx<'a>,
    pub sys: &'a System<'a>,
    pub mode: TanMode<'a>,
    pub exact: Exact,
    /// a `when` clause's assignments are being lowered: a discrete
    /// variable reads its value as updated so far (`out`), `pre` the value
    /// before the event (`d`); everything else (the assignments they read)
    /// reads the values before the event
    pub when_new: bool,
    /// whether the target has a fused multiply-add instruction
    pub fma: bool,
    /// which assignments are kept in `work`
    pub keep: &'a [bool],
    pub tan: &'a TanLayout,
    ties: TieRule,
    seg_vals: SegCache<D<E::V>>,
    /// loads of y, p, d, u, v (by array, then index)
    seg_loads: [SegCache<E::V>; 5],
    /// constants materialised in this segment (by bit pattern)
    seg_consts: HashMap<u64, E::V>,
}

impl<'a, E: Emit> Lw<'a, E> {
    pub(crate) fn new(e: E, s: LwSetup<'a>) -> Self {
        Lw {
            e,
            cx: s.cx,
            sys: s.sys,
            mode: s.mode,
            exact: s.exact,
            when_new: false,
            fma: s.fma,
            keep: s.keep,
            tan: s.tan,
            ties: tie_rule(),
            seg_vals: SegCache::new(),
            seg_loads: [
                SegCache::new(),
                SegCache::new(),
                SegCache::new(),
                SegCache::new(),
                SegCache::new(),
            ],
            seg_consts: HashMap::new(),
        }
    }

    /// Starts a new segment: nothing computed before is reused from
    /// registers.
    pub(crate) fn new_segment(&mut self) {
        self.seg_vals.clear();
        for c in &mut self.seg_loads {
            c.clear();
        }
        self.seg_consts.clear();
        self.e.new_segment();
    }

    pub(crate) fn cst(&mut self, x: f64) -> E::V {
        if let Some(v) = self.seg_consts.get(&x.to_bits()) {
            return *v;
        }
        let v = self.e.konst(x);
        self.seg_consts.insert(x.to_bits(), v);
        v
    }

    /// array: 0 y, 1 p, 2 d, 3 u, 4 v
    fn load(&mut self, which: usize, i: usize) -> E::V {
        if let Some(v) = self.seg_loads[which].get(i) {
            return *v;
        }
        let arr = [Base::Y, Base::P, Base::D, Base::U, Base::V][which];
        let v = self.e.load(arr, i);
        self.seg_loads[which].put(i, v);
        v
    }

    /// `v[i]` (a `when` clause's `fired`).
    pub(crate) fn load_v(&mut self, i: usize) -> E::V {
        self.load(4, i)
    }

    /// `out[i] = v`.
    pub(crate) fn store_out(&mut self, i: usize, v: E::V) {
        self.e.store(Base::Out, i, v);
    }

    fn seed(&mut self, i: usize) -> Tan<E::V> {
        match self.mode {
            TanMode::None => vec![],
            TanMode::Jvp => {
                let w = self.load(4, i);
                vec![(0, Tv::V(w))]
            }
            TanMode::Colours(c) => vec![(c[i], Tv::One)],
        }
    }

    /// The value (and tangent) of a source; `pre`: a discrete variable's
    /// value before the event.
    pub(crate) fn src(&mut self, s: Src, pre: bool) -> Result<D<E::V>, CodegenError> {
        Ok(match s {
            Src::Y(i) => {
                let v = self.load(0, i);
                let t = self.seed(i);
                D { v, t }
            }
            Src::Work(k) => {
                if let Some(d) = self.seg_vals.get(k) {
                    return Ok(d.clone());
                }
                if !self.keep.get(k).copied().unwrap_or(false) {
                    return Err(CodegenError::Backend(format!(
                        "internal: assignment {k} is read but was not kept"
                    )));
                }
                let v = self.e.load(Base::Work, k);
                let mut t = vec![];
                if !matches!(self.mode, TanMode::None) {
                    let (at, dirs) = &self.tan.slots[k];
                    let at = *at as usize;
                    t.reserve(dirs.len());
                    for (i, &dir) in dirs.iter().enumerate() {
                        let x = self.e.load(Base::Work, at + i);
                        t.push((dir, Tv::V(x)));
                    }
                }
                let d = D { v, t };
                self.seg_vals.put(k, d.clone());
                d
            }
            Src::D(k) => {
                let v = if self.when_new && !pre {
                    // updated by earlier assignments of this event: no reuse
                    self.e.load(Base::Out, k)
                } else {
                    self.load(2, k)
                };
                D { v, t: vec![] }
            }
            Src::U(k) => D { v: self.load(3, k), t: vec![] },
            Src::Const(c) => D { v: self.cst(c), t: vec![] },
        })
    }

    /// Records an assignment's value for reuse in this segment, and
    /// writes it (and its tangent) to `work` when it is kept (later
    /// segments or other functions read it).
    pub(crate) fn define(&mut self, k: usize, d: D<E::V>) {
        if self.keep[k] {
            self.e.store(Base::Work, k, d.v);
            if !matches!(self.mode, TanMode::None) {
                let (at, dirs) = &self.tan.slots[k];
                let at = *at as usize;
                for (i, dir) in dirs.iter().enumerate() {
                    let v = match d.t.iter().find(|x| x.0 == *dir) {
                        Some(&(_, x)) => self.tv(x),
                        None => self.cst(0.0),
                    };
                    self.e.store(Base::Work, at + i, v);
                }
            }
        }
        self.seg_vals.put(k, d);
    }

    fn dual(&self) -> bool {
        !matches!(self.mode, TanMode::None)
    }

    pub(crate) fn tv(&mut self, x: Tv<E::V>) -> E::V {
        match x {
            Tv::One => self.cst(1.0),
            Tv::V(v) => v,
        }
    }

    /// f · t
    fn scale(&mut self, f: E::V, t: &Tan<E::V>) -> Tan<E::V> {
        t.iter()
            .map(|&(d, x)| {
                let v = match x {
                    Tv::One => f,
                    Tv::V(w) => self.e.mul(f, w),
                };
                (d, Tv::V(v))
            })
            .collect()
    }

    fn neg1(&mut self, x: Tv<E::V>) -> Tv<E::V> {
        Tv::V(match x {
            Tv::One => self.cst(-1.0),
            Tv::V(w) => self.e.neg(w),
        })
    }

    fn neg_t(&mut self, t: &Tan<E::V>) -> Tan<E::V> {
        t.iter().map(|&(d, x)| (d, self.neg1(x))).collect()
    }

    /// a ± b
    fn add_t(&mut self, a: &Tan<E::V>, b: &Tan<E::V>, sub: bool) -> Tan<E::V> {
        if b.is_empty() {
            return a.clone();
        }
        if a.is_empty() {
            return if sub { self.neg_t(b) } else { b.clone() };
        }
        let mut out = Vec::with_capacity(a.len().max(b.len()));
        let (mut i, mut j) = (0, 0);
        while i < a.len() || j < b.len() {
            let da = a.get(i).map_or(u32::MAX, |x| x.0);
            let db = b.get(j).map_or(u32::MAX, |x| x.0);
            if da < db {
                out.push(a[i]);
                i += 1;
            } else if db < da {
                let x = if sub { self.neg1(b[j].1) } else { b[j].1 };
                out.push((db, x));
                j += 1;
            } else {
                let (x, y) = (self.tv(a[i].1), self.tv(b[j].1));
                let v = if sub { self.e.sub(x, y) } else { self.e.add(x, y) };
                out.push((da, Tv::V(v)));
                i += 1;
                j += 1;
            }
        }
        out
    }

    /// fa · ta + fb · tb
    fn lin2(&mut self, fa: E::V, ta: &Tan<E::V>, fb: E::V, tb: &Tan<E::V>) -> Tan<E::V> {
        let x = self.scale(fa, ta);
        let y = self.scale(fb, tb);
        self.add_t(&x, &y, false)
    }

    /// if c then ta else tb, per direction
    fn select_t(&mut self, c: E::V, ta: &Tan<E::V>, tb: &Tan<E::V>) -> Tan<E::V> {
        if ta.is_empty() && tb.is_empty() {
            return vec![];
        }
        let mut out = Vec::with_capacity(ta.len().max(tb.len()));
        let (mut i, mut j) = (0, 0);
        while i < ta.len() || j < tb.len() {
            let da = ta.get(i).map_or(u32::MAX, |x| x.0);
            let db = tb.get(j).map_or(u32::MAX, |x| x.0);
            let (dir, x, y) = if da < db {
                i += 1;
                (da, Some(ta[i - 1].1), None)
            } else if db < da {
                j += 1;
                (db, None, Some(tb[j - 1].1))
            } else {
                i += 1;
                j += 1;
                (da, Some(ta[i - 1].1), Some(tb[j - 1].1))
            };
            let x = match x {
                Some(x) => self.tv(x),
                None => self.cst(0.0),
            };
            let y = match y {
                Some(y) => self.tv(y),
                None => self.cst(0.0),
            };
            out.push((dir, Tv::V(self.e.select(c, x, y))));
        }
        out
    }

    fn truth(&mut self, cond: E::V) -> E::V {
        let one = self.cst(1.0);
        let zero = self.cst(0.0);
        self.e.select(cond, one, zero)
    }

    /// Whether a truth value holds (non-zero; NaN counts as true, as in
    /// the interpreter).
    pub(crate) fn is_true(&mut self, x: E::V) -> E::V {
        let zero = self.cst(0.0);
        self.e.cmp(Cc::Ne, x, zero)
    }

    /// 1 where a truth holds, else 0.
    pub(crate) fn as_number(&mut self, cond: E::V) -> E::V {
        self.truth(cond)
    }

    fn sign(&mut self, x: E::V) -> E::V {
        let zero = self.cst(0.0);
        let one = self.cst(1.0);
        let minus = self.cst(-1.0);
        let pos = self.e.cmp(Cc::Gt, x, zero);
        let neg = self.e.cmp(Cc::Lt, x, zero);
        let m = self.e.select(neg, minus, zero);
        self.e.select(pos, one, m)
    }

    /// x^n, n ≥ 1, by repeated squaring (for derivatives).
    fn powi_plain(&mut self, x: E::V, n: u32) -> E::V {
        debug_assert!(n >= 1);
        let bits = 32 - n.leading_zeros();
        let mut acc = x;
        for i in (0..bits - 1).rev() {
            acc = self.e.mul(acc, acc);
            if (n >> i) & 1 == 1 {
                acc = self.e.mul(acc, x);
            }
        }
        acc
    }

    /// x^n for an integer |n| ≥ 3, within one rounding of the exact
    /// power: repeated squaring in double-double arithmetic (exact
    /// products from fused multiply-adds), then one rounding. Overflow,
    /// underflow to zero and non-finite x fall back to the plain product.
    fn powi_exact(&mut self, x: E::V, n: i32) -> E::V {
        let m = n.unsigned_abs();
        let bits = 32 - m.leading_zeros();
        let (mut h, mut l): (E::V, Option<E::V>) = (x, None);
        let mut last_p = x;
        for i in (0..bits - 1).rev() {
            // square: h² + 2hl
            let p = self.e.mul(h, h);
            let np = self.e.neg(p);
            let mut e = self.e.fma(h, h, np);
            if let Some(lo) = l {
                let h2 = self.e.add(h, h);
                e = self.e.fma(h2, lo, e);
            }
            (h, l) = self.fast_two_sum(p, e);
            last_p = p;
            if (m >> i) & 1 == 1 {
                // times x: hx + lx
                let p = self.e.mul(h, x);
                let np = self.e.neg(p);
                let mut e = self.e.fma(h, x, np);
                if let Some(lo) = l {
                    e = self.e.fma(lo, x, e);
                }
                (h, l) = self.fast_two_sum(p, e);
                last_p = p;
            }
        }
        let (r, plain) = if n < 0 {
            // 1 / (h + l), corrected once
            let one = self.cst(1.0);
            let q = self.e.div(one, h);
            let nq = self.e.neg(q);
            let rem = self.e.fma(nq, h, one);
            let rem = match l {
                Some(lo) => self.e.fma(nq, lo, rem),
                None => rem,
            };
            let r = self.e.fma(q, rem, q);
            let plain = self.e.div(one, last_p);
            (r, plain)
        } else {
            (h, last_p)
        };
        // keep r when it is finite and the power is not zero
        let zero = self.cst(0.0);
        let rr = self.e.sub(r, r);
        let finite = self.e.cmp(Cc::Eq, rr, zero);
        let nonzero = self.e.cmp(Cc::Ne, last_p, zero);
        let ok = self.e.and(finite, nonzero);
        self.e.select(ok, r, plain)
    }

    /// (s, e) with s = fl(p + e) and s + e' = p + e exactly (|p| ≥ |e|).
    fn fast_two_sum(&mut self, p: E::V, e: E::V) -> (E::V, Option<E::V>) {
        let s = self.e.add(p, e);
        let d = self.e.sub(s, p);
        let lo = self.e.sub(e, d);
        (s, Some(lo))
    }

    /// a^n for a constant n.
    fn pow_const(&mut self, a: &D<E::V>, n: f64) -> D<E::V> {
        let (va, ta) = (a.v, &a.t);
        let dual = self.dual() && !ta.is_empty();
        let by_pow = |s: &mut Self| {
            let cn = s.cst(n);
            let v = s.e.call(Lib::Pow, &[va, cn]);
            let d = dual.then(|| {
                let cm = s.cst(n - 1.0);
                let pm1 = s.e.call(Lib::Pow, &[va, cm]);
                s.e.mul(cn, pm1)
            });
            (v, d)
        };
        let (v, dv) = if self.exact == Exact::Interpreter {
            // the interpreter's `powf`, whatever n is
            by_pow(self)
        } else if n == 0.0 {
            return D { v: self.cst(1.0), t: vec![] };
        } else if n == 1.0 {
            return a.clone();
        } else if n == 2.0 {
            let v = self.e.mul(va, va);
            let d = dual.then(|| {
                let two = self.cst(2.0);
                self.e.mul(two, va)
            });
            (v, d)
        } else if n == -1.0 {
            let one = self.cst(1.0);
            let v = self.e.div(one, va);
            let d = dual.then(|| {
                let v2 = self.e.mul(v, v);
                self.e.neg(v2)
            });
            (v, d)
        } else if n == 0.5 {
            // pow(-0, 0.5) = +0 and pow(-inf, 0.5) = +inf, unlike sqrt
            let s = self.e.sqrt(va);
            let zero = self.cst(0.0);
            let s = self.e.add(s, zero);
            let ninf = self.cst(f64::NEG_INFINITY);
            let pinf = self.cst(f64::INFINITY);
            let is_ninf = self.e.cmp(Cc::Eq, va, ninf);
            let v = self.e.select(is_ninf, pinf, s);
            let d = dual.then(|| {
                let half = self.cst(0.5);
                self.e.div(half, v)
            });
            (v, d)
        } else if inline_power(n) && self.fma {
            let k = n as i32;
            let v = self.powi_exact(va, k);
            let d = dual.then(|| {
                // n x^(n-1)
                let xm = if k - 1 > 0 {
                    self.powi_plain(va, (k - 1) as u32)
                } else {
                    let p = self.powi_plain(va, (1 - k) as u32);
                    let one = self.cst(1.0);
                    self.e.div(one, p)
                };
                let cn = self.cst(n);
                self.e.mul(cn, xm)
            });
            (v, d)
        } else {
            by_pow(self)
        };
        let t = match dv {
            Some(d) => self.scale(d, ta),
            None => vec![],
        };
        D { v, t }
    }

    /// Lowers an expression to its value and tangent.
    pub(crate) fn lower(&mut self, e: &Expr) -> Result<D<E::V>, CodegenError> {
        Ok(match e {
            Expr::Const(x) => D { v: self.cst(*x), t: vec![] },
            Expr::Time => D { v: self.e.time(), t: vec![] },
            Expr::Var(v) => {
                let s = self.sys.resolve(self.cx, *v, false)?;
                self.src(s, false)?
            }
            Expr::Pre(v) => {
                let s = self.sys.resolve(self.cx, *v, false)?;
                self.src(s, true)?
            }
            Expr::Der(v) => {
                let s = self.sys.resolve(self.cx, *v, true)?;
                self.src(s, false)?
            }
            Expr::Param(p) => D { v: self.load(1, p.0 as usize), t: vec![] },
            Expr::Name(n) => {
                return Err(CodegenError::Unsupported(format!("unresolved name '{n}'")));
            }
            Expr::Neg(a) => {
                let a = self.lower(a)?;
                let v = self.e.neg(a.v);
                let t = self.neg_t(&a.t);
                D { v, t }
            }
            Expr::NoEvent(a) => self.lower(a)?,
            Expr::Binary(op, a, b) => {
                if let (BinaryOp::Pow, Expr::Const(n)) = (op, &**b) {
                    let a = self.lower(a)?;
                    return Ok(self.pow_const(&a, *n));
                }
                let a = self.lower(a)?;
                let b = self.lower(b)?;
                self.binary(*op, a, b)
            }
            Expr::Call(f, args) => self.builtin(*f, args)?,
            Expr::Compare(op, a, b) => {
                let a = self.lower(a)?;
                let b = self.lower(b)?;
                let cc = match op {
                    CmpOp::Lt => Cc::Lt,
                    CmpOp::Le => Cc::Le,
                    CmpOp::Gt => Cc::Gt,
                    CmpOp::Ge => Cc::Ge,
                };
                let c = self.e.cmp(cc, a.v, b.v);
                D { v: self.truth(c), t: vec![] }
            }
            Expr::And(a, b) | Expr::Or(a, b) => {
                let a = self.lower(a)?;
                let b = self.lower(b)?;
                let (ca, cb) = (self.is_true(a.v), self.is_true(b.v));
                let c =
                    if matches!(e, Expr::And(..)) { self.e.and(ca, cb) } else { self.e.or(ca, cb) };
                D { v: self.truth(c), t: vec![] }
            }
            Expr::Not(a) => {
                let a = self.lower(a)?;
                let zero = self.cst(0.0);
                let c = self.e.cmp(Cc::Eq, a.v, zero);
                D { v: self.truth(c), t: vec![] }
            }
            Expr::If(c, a, b) => {
                let c = self.lower(c)?;
                let cond = self.is_true(c.v);
                let a = self.lower(a)?;
                let b = self.lower(b)?;
                let v = self.e.select(cond, a.v, b.v);
                let t = self.select_t(cond, &a.t, &b.t);
                D { v, t }
            }
            Expr::Table { table, args } => self.table(*table, args)?,
        })
    }

    fn binary(&mut self, op: BinaryOp, a: D<E::V>, b: D<E::V>) -> D<E::V> {
        match op {
            BinaryOp::Add | BinaryOp::Sub => {
                let sub = op == BinaryOp::Sub;
                let v = if sub { self.e.sub(a.v, b.v) } else { self.e.add(a.v, b.v) };
                let t = self.add_t(&a.t, &b.t, sub);
                D { v, t }
            }
            BinaryOp::Mul => {
                let v = self.e.mul(a.v, b.v);
                // d(ab) = b da + a db
                let t = self.lin2(b.v, &a.t, a.v, &b.t);
                D { v, t }
            }
            BinaryOp::Div => {
                let v = self.e.div(a.v, b.v);
                // d(a/b) = (da - (a/b) db) / b
                let t = if a.t.is_empty() && b.t.is_empty() {
                    vec![]
                } else {
                    let nq = self.e.neg(v);
                    let x = self.scale(nq, &b.t);
                    let s = self.add_t(&a.t, &x, false);
                    s.into_iter()
                        .map(|(d, x)| {
                            let xv = self.tv(x);
                            (d, Tv::V(self.e.div(xv, b.v)))
                        })
                        .collect()
                };
                D { v, t }
            }
            BinaryOp::Pow => {
                let v = self.e.call(Lib::Pow, &[a.v, b.v]);
                let mut t = vec![];
                if self.dual() && !a.t.is_empty() {
                    // b a^(b-1) da
                    let one = self.cst(1.0);
                    let bm1 = self.e.sub(b.v, one);
                    let pm1 = self.e.call(Lib::Pow, &[a.v, bm1]);
                    let f = self.e.mul(b.v, pm1);
                    t = self.scale(f, &a.t);
                }
                if self.dual() && !b.t.is_empty() {
                    // a^b ln(a) db
                    let ln = self.e.call(Lib::Log, &[a.v]);
                    let f = self.e.mul(v, ln);
                    let tb = self.scale(f, &b.t);
                    t = self.add_t(&t, &tb, false);
                }
                D { v, t }
            }
        }
    }

    fn builtin(&mut self, f: Builtin, args: &[Expr]) -> Result<D<E::V>, CodegenError> {
        let mut vals = Vec::with_capacity(args.len());
        for a in args {
            vals.push(self.lower(a)?);
        }
        let a = vals[0].clone();
        let dual = self.dual() && !a.t.is_empty();
        let unary = |s: &mut Self, f: Lib| s.e.call(f, &[a.v]);
        // the value, and d(value)/d(argument) when a tangent is needed
        let (v, dv): (E::V, Option<E::V>) = match f {
            Builtin::Der | Builtin::Pre => {
                return Err(CodegenError::Unsupported("der/pre in component scope".into()));
            }
            Builtin::Sqrt => {
                let v = self.e.sqrt(a.v);
                let d = dual.then(|| {
                    let half = self.cst(0.5);
                    self.e.div(half, v)
                });
                (v, d)
            }
            Builtin::Abs => {
                let v = self.e.abs(a.v);
                let d = dual.then(|| self.sign(a.v));
                (v, d)
            }
            Builtin::Sign => (self.sign(a.v), None),
            Builtin::Exp => {
                let v = unary(self, Lib::Exp);
                (v, dual.then_some(v))
            }
            Builtin::Log => {
                let v = unary(self, Lib::Log);
                let d = dual.then(|| {
                    let one = self.cst(1.0);
                    self.e.div(one, a.v)
                });
                (v, d)
            }
            Builtin::Sin => {
                let v = unary(self, Lib::Sin);
                let d = dual.then(|| unary(self, Lib::Cos));
                (v, d)
            }
            Builtin::Cos => {
                let v = unary(self, Lib::Cos);
                let d = dual.then(|| {
                    let s = unary(self, Lib::Sin);
                    self.e.neg(s)
                });
                (v, d)
            }
            Builtin::Tan => {
                let v = unary(self, Lib::Tan);
                let d = dual.then(|| {
                    let one = self.cst(1.0);
                    let v2 = self.e.mul(v, v);
                    self.e.add(one, v2)
                });
                (v, d)
            }
            Builtin::Asin | Builtin::Acos => {
                let lib = if f == Builtin::Asin { Lib::Asin } else { Lib::Acos };
                let v = unary(self, lib);
                let d = dual.then(|| {
                    let one = self.cst(1.0);
                    let a2 = self.e.mul(a.v, a.v);
                    let s = self.e.sub(one, a2);
                    let r = self.e.sqrt(s);
                    let q = self.e.div(one, r);
                    if f == Builtin::Acos { self.e.neg(q) } else { q }
                });
                (v, d)
            }
            Builtin::Atan => {
                let v = unary(self, Lib::Atan);
                let d = dual.then(|| {
                    let one = self.cst(1.0);
                    let a2 = self.e.mul(a.v, a.v);
                    let s = self.e.add(one, a2);
                    self.e.div(one, s)
                });
                (v, d)
            }
            Builtin::Sinh => {
                let v = unary(self, Lib::Sinh);
                let d = dual.then(|| unary(self, Lib::Cosh));
                (v, d)
            }
            Builtin::Cosh => {
                let v = unary(self, Lib::Cosh);
                let d = dual.then(|| unary(self, Lib::Sinh));
                (v, d)
            }
            Builtin::Tanh => {
                let v = unary(self, Lib::Tanh);
                let d = dual.then(|| {
                    let one = self.cst(1.0);
                    let v2 = self.e.mul(v, v);
                    self.e.sub(one, v2)
                });
                (v, d)
            }
            Builtin::Atan2 => {
                let x = vals[1].clone();
                let v = self.e.call(Lib::Atan2, &[a.v, x.v]);
                let t = if self.dual() && !(a.t.is_empty() && x.t.is_empty()) {
                    // (x dy - y dx) / (x² + y²)
                    let x2 = self.e.mul(x.v, x.v);
                    let y2 = self.e.mul(a.v, a.v);
                    let den = self.e.add(x2, y2);
                    let fy = self.e.div(x.v, den);
                    let ny = self.e.neg(a.v);
                    let fx = self.e.div(ny, den);
                    self.lin2(fy, &a.t, fx, &x.t)
                } else {
                    vec![]
                };
                return Ok(D { v, t });
            }
            Builtin::Min | Builtin::Max | Builtin::Limit => {
                return Ok(match f {
                    Builtin::Min => self.pick(false, &vals[0], &vals[1]),
                    Builtin::Max => self.pick(true, &vals[0], &vals[1]),
                    _ => {
                        let lo = self.pick(true, &vals[0], &vals[1]);
                        self.pick(false, &lo, &vals[2])
                    }
                });
            }
        };
        let t = match dv {
            Some(d) => self.scale(d, &a.t),
            None => vec![],
        };
        Ok(D { v, t })
    }

    /// Rust's `f64::max` (`max`) or `f64::min`, as the interpreter calls
    /// them: the other argument when one is NaN, the larger (smaller)
    /// otherwise, and between zeros of opposite signs what the platform's
    /// gives ([`tie_rule`]).
    fn pick(&mut self, max: bool, a: &D<E::V>, b: &D<E::V>) -> D<E::V> {
        let bnan = self.e.cmp(Cc::Uno, b.v, b.v);
        let (strict, wide) = if max { (Cc::Gt, Cc::Ge) } else { (Cc::Lt, Cc::Le) };
        let (c, v) = match self.ties {
            TieRule::First => {
                // a when a ≥ b (a tie: the first) or b is NaN
                let c1 = self.e.cmp(wide, a.v, b.v);
                let c = self.e.or(c1, bnan);
                (c, self.e.select(c, a.v, b.v))
            }
            TieRule::Second => {
                let c1 = self.e.cmp(strict, a.v, b.v);
                let c = self.e.or(c1, bnan);
                (c, self.e.select(c, a.v, b.v))
            }
            TieRule::SignAware => {
                let c1 = self.e.cmp(strict, a.v, b.v);
                let c = self.e.or(c1, bnan);
                let v = self.e.select(c, a.v, b.v);
                // equal: the same bits, or zeros of opposite signs (+0 for
                // max: the bits' `and`; −0 for min: their `or`)
                let eq = self.e.cmp(Cc::Eq, a.v, b.v);
                let z = if max { self.e.bits_and(a.v, b.v) } else { self.e.bits_or(a.v, b.v) };
                (c, self.e.select(eq, z, v))
            }
        };
        let t = self.select_t(c, &a.t, &b.t);
        D { v, t }
    }

    fn table(&mut self, table: u32, args: &[Expr]) -> Result<D<E::V>, CodegenError> {
        let mut vals = Vec::with_capacity(args.len());
        for a in args {
            vals.push(self.lower(a)?);
        }
        let need_d = self.dual() && vals.iter().any(|d| !d.t.is_empty());
        let argv: Vec<E::V> = vals.iter().map(|d| d.v).collect();
        let (v, g) = self.e.table(table, &argv, need_d);
        if !need_d {
            return Ok(D { v, t: vec![] });
        }
        let mut t = vec![];
        for (k, d) in vals.iter().enumerate() {
            if d.t.is_empty() {
                continue;
            }
            let gk = g[k].expect("asked for");
            let s = self.scale(gk, &d.t);
            t = self.add_t(&t, &s, false);
        }
        Ok(D { v, t })
    }
}
