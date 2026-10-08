//! Lowering of flat expressions to Cranelift IR, with forward-mode
//! tangents (dual numbers) over one or many directions.
//!
//! Values cross assignments through memory: each assignment's value (and
//! tangent) is kept in an SSA register only within a short *segment* of
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

use crate::CodegenError;
use crate::analysis::{Ctx, Src, System, inline_power};
use cranelift_codegen::ir::condcodes::FloatCC;
use cranelift_codegen::ir::types::{F64, I64};
use cranelift_codegen::ir::{
    FuncRef, InstBuilder, MemFlagsData, StackSlot, StackSlotData, StackSlotKind, Value,
};
use cranelift_frontend::FunctionBuilder;
use lsim_ir::expr::{BinaryOp, Builtin, CmpOp, Expr};
use std::collections::HashMap;

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

/// One tangent entry: exactly one, or a computed value.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Tv {
    One,
    V(Value),
}

/// A sparse tangent: (direction, entry), directions increasing.
pub(crate) type Tan = Vec<(u32, Tv)>;

/// A value and its tangent.
#[derive(Clone, Debug)]
pub(crate) struct D {
    pub v: Value,
    pub t: Tan,
}

/// The function's parameters.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Args {
    pub t: Value,
    pub y: Value,
    pub p: Value,
    pub d: Value,
    pub u: Value,
    pub v: Value,
    pub work: Value,
    pub out: Value,
    pub tabs: Value,
}

/// Imported functions (math library, table runtime), declared in a
/// function on first use.
pub(crate) struct Imports<'a> {
    pub decls: &'a HashMap<&'static str, crate::jit::Import>,
    pub refs: HashMap<&'static str, FuncRef>,
}

impl Imports<'_> {
    fn get(&mut self, b: &mut FunctionBuilder<'_>, name: &'static str) -> FuncRef {
        if let Some(r) = self.refs.get(name) {
            return *r;
        }
        let imp = &self.decls[name];
        let sig = b.func.import_signature(imp.sig.clone());
        let user = b.func.declare_imported_user_function(cranelift_codegen::ir::UserExternalName {
            namespace: 0,
            index: imp.id.as_u32(),
        });
        let r = b.func.import_function(cranelift_codegen::ir::ExtFuncData {
            name: cranelift_codegen::ir::ExternalName::user(user),
            signature: sig,
            colocated: false,
            patchable: false,
        });
        self.refs.insert(name, r);
        r
    }
}

fn mem() -> MemFlagsData {
    MemFlagsData::trusted()
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

/// The state of lowering one function.
pub(crate) struct Lw<'a, 'f> {
    pub b: &'a mut FunctionBuilder<'f>,
    pub cx: &'a Ctx<'a>,
    pub sys: &'a System<'a>,
    pub imp: Imports<'a>,
    pub a: Args,
    pub mode: TanMode<'a>,
    /// the `when` function: a discrete variable reads its value as
    /// updated so far (`d_out`), `pre` reads the value before the event
    pub when: bool,
    /// whether the target has a fused multiply-add instruction
    pub fma: bool,
    /// which assignments are kept in `work`
    pub keep: &'a [bool],
    pub tan: &'a TanLayout,
    seg_vals: SegCache<D>,
    /// loads of y, p, d, u, v (by array, then index)
    seg_loads: [SegCache<Value>; 5],
    /// constants materialised in this segment (by bit pattern)
    seg_consts: HashMap<u64, Value>,
    scratch: Option<StackSlot>,
}

/// What a function lowering needs besides the builder.
pub(crate) struct LwSetup<'a> {
    pub cx: &'a Ctx<'a>,
    pub sys: &'a System<'a>,
    pub decls: &'a HashMap<&'static str, crate::jit::Import>,
    pub mode: TanMode<'a>,
    pub when: bool,
    pub fma: bool,
    pub keep: &'a [bool],
    pub tan: &'a TanLayout,
}

impl<'a, 'f> Lw<'a, 'f> {
    pub(crate) fn new(b: &'a mut FunctionBuilder<'f>, s: LwSetup<'a>, a: Args) -> Self {
        Lw {
            b,
            cx: s.cx,
            sys: s.sys,
            imp: Imports { decls: s.decls, refs: HashMap::new() },
            a,
            mode: s.mode,
            when: s.when,
            fma: s.fma,
            keep: s.keep,
            tan: s.tan,
            seg_vals: SegCache::new(),
            seg_loads: [
                SegCache::new(),
                SegCache::new(),
                SegCache::new(),
                SegCache::new(),
                SegCache::new(),
            ],
            seg_consts: HashMap::new(),
            scratch: None,
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
    }

    pub(crate) fn cst(&mut self, x: f64) -> Value {
        if let Some(v) = self.seg_consts.get(&x.to_bits()) {
            return *v;
        }
        let v = self.b.ins().f64const(x);
        self.seg_consts.insert(x.to_bits(), v);
        v
    }

    /// array: 0 y, 1 p, 2 d, 3 u, 4 v
    fn load(&mut self, which: usize, base: Value, i: usize) -> Value {
        if let Some(v) = self.seg_loads[which].get(i) {
            return *v;
        }
        let v = self.b.ins().load(F64, mem(), base, (8 * i) as i32);
        self.seg_loads[which].put(i, v);
        v
    }

    pub(crate) fn store(&mut self, base: Value, i: usize, v: Value) {
        self.b.ins().store(mem(), v, base, (8 * i) as i32);
    }

    fn seed(&mut self, i: usize) -> Tan {
        match self.mode {
            TanMode::None => vec![],
            TanMode::Jvp => {
                let w = self.load(4, self.a.v, i);
                vec![(0, Tv::V(w))]
            }
            TanMode::Colours(c) => vec![(c[i], Tv::One)],
        }
    }

    /// The value (and tangent) of a source; `pre`: a discrete variable's
    /// value before the event.
    pub(crate) fn src(&mut self, s: Src, pre: bool) -> Result<D, CodegenError> {
        Ok(match s {
            Src::Y(i) => {
                let v = self.load(0, self.a.y, i);
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
                let v = self.b.ins().load(F64, mem(), self.a.work, (8 * k) as i32);
                let mut t = vec![];
                if !matches!(self.mode, TanMode::None) {
                    let (at, dirs) = &self.tan.slots[k];
                    let at = *at;
                    t.reserve(dirs.len());
                    for (i, &dir) in dirs.iter().enumerate() {
                        let x = self.b.ins().load(
                            F64,
                            mem(),
                            self.a.work,
                            (8 * (at as usize + i)) as i32,
                        );
                        t.push((dir, Tv::V(x)));
                    }
                }
                let d = D { v, t };
                self.seg_vals.put(k, d.clone());
                d
            }
            Src::D(k) => {
                let v = if self.when && !pre {
                    // updated by earlier assignments of this event: no reuse
                    self.b.ins().load(F64, mem(), self.a.out, (8 * k) as i32)
                } else {
                    self.load(2, self.a.d, k)
                };
                D { v, t: vec![] }
            }
            Src::U(k) => D { v: self.load(3, self.a.u, k), t: vec![] },
            Src::Const(c) => D { v: self.cst(c), t: vec![] },
        })
    }

    /// Records an assignment's value for reuse in this segment, and
    /// writes it (and its tangent) to `work` when it is kept (later
    /// segments or other functions read it).
    pub(crate) fn define(&mut self, k: usize, d: D) {
        if self.keep[k] {
            self.store(self.a.work, k, d.v);
            if !matches!(self.mode, TanMode::None) {
                let (at, dirs) = &self.tan.slots[k];
                let at = *at as usize;
                for (i, dir) in dirs.iter().enumerate() {
                    let v = match d.t.iter().find(|x| x.0 == *dir) {
                        Some(&(_, x)) => self.tv(x),
                        None => self.cst(0.0),
                    };
                    self.store(self.a.work, at + i, v);
                }
            }
        }
        self.seg_vals.put(k, d);
    }

    fn dual(&self) -> bool {
        !matches!(self.mode, TanMode::None)
    }

    pub(crate) fn tv(&mut self, x: Tv) -> Value {
        match x {
            Tv::One => self.cst(1.0),
            Tv::V(v) => v,
        }
    }

    /// f · t
    fn scale(&mut self, f: Value, t: &Tan) -> Tan {
        t.iter()
            .map(|&(d, x)| {
                let v = match x {
                    Tv::One => f,
                    Tv::V(w) => self.b.ins().fmul(f, w),
                };
                (d, Tv::V(v))
            })
            .collect()
    }

    fn neg1(&mut self, x: Tv) -> Tv {
        Tv::V(match x {
            Tv::One => self.cst(-1.0),
            Tv::V(w) => self.b.ins().fneg(w),
        })
    }

    fn neg_t(&mut self, t: &Tan) -> Tan {
        t.iter().map(|&(d, x)| (d, self.neg1(x))).collect()
    }

    /// a ± b
    fn add_t(&mut self, a: &Tan, b: &Tan, sub: bool) -> Tan {
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
                let v = if sub { self.b.ins().fsub(x, y) } else { self.b.ins().fadd(x, y) };
                out.push((da, Tv::V(v)));
                i += 1;
                j += 1;
            }
        }
        out
    }

    /// fa · ta + fb · tb
    fn lin2(&mut self, fa: Value, ta: &Tan, fb: Value, tb: &Tan) -> Tan {
        let x = self.scale(fa, ta);
        let y = self.scale(fb, tb);
        self.add_t(&x, &y, false)
    }

    /// if c then ta else tb, per direction
    fn select_t(&mut self, c: Value, ta: &Tan, tb: &Tan) -> Tan {
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
            out.push((dir, Tv::V(self.b.ins().select(c, x, y))));
        }
        out
    }

    fn call(&mut self, name: &'static str, args: &[Value]) -> Value {
        let f = self.imp.get(self.b, name);
        let inst = self.b.ins().call(f, args);
        self.b.inst_results(inst)[0]
    }

    fn truth(&mut self, cond: Value) -> Value {
        let one = self.cst(1.0);
        let zero = self.cst(0.0);
        self.b.ins().select(cond, one, zero)
    }

    /// Whether a truth value holds (non-zero; NaN counts as true, as in
    /// the interpreter).
    pub(crate) fn is_true(&mut self, x: Value) -> Value {
        let zero = self.cst(0.0);
        self.b.ins().fcmp(FloatCC::NotEqual, x, zero)
    }

    fn sign(&mut self, x: Value) -> Value {
        let zero = self.cst(0.0);
        let one = self.cst(1.0);
        let minus = self.cst(-1.0);
        let pos = self.b.ins().fcmp(FloatCC::GreaterThan, x, zero);
        let neg = self.b.ins().fcmp(FloatCC::LessThan, x, zero);
        let m = self.b.ins().select(neg, minus, zero);
        self.b.ins().select(pos, one, m)
    }

    /// x^n, n ≥ 1, by repeated squaring (for derivatives).
    fn powi_plain(&mut self, x: Value, n: u32) -> Value {
        debug_assert!(n >= 1);
        let bits = 32 - n.leading_zeros();
        let mut acc = x;
        for i in (0..bits - 1).rev() {
            acc = self.b.ins().fmul(acc, acc);
            if (n >> i) & 1 == 1 {
                acc = self.b.ins().fmul(acc, x);
            }
        }
        acc
    }

    /// x^n for an integer |n| ≥ 3, within one rounding of the exact
    /// power: repeated squaring in double-double arithmetic (exact
    /// products from fused multiply-adds), then one rounding. Overflow,
    /// underflow to zero and non-finite x fall back to the plain product.
    fn powi_exact(&mut self, x: Value, n: i32) -> Value {
        let m = n.unsigned_abs();
        let bits = 32 - m.leading_zeros();
        let (mut h, mut l): (Value, Option<Value>) = (x, None);
        let mut last_p = x;
        for i in (0..bits - 1).rev() {
            // square: h² + 2hl
            let p = self.b.ins().fmul(h, h);
            let np = self.b.ins().fneg(p);
            let mut e = self.b.ins().fma(h, h, np);
            if let Some(lo) = l {
                let h2 = self.b.ins().fadd(h, h);
                e = self.b.ins().fma(h2, lo, e);
            }
            (h, l) = self.fast_two_sum(p, e);
            last_p = p;
            if (m >> i) & 1 == 1 {
                // times x: hx + lx
                let p = self.b.ins().fmul(h, x);
                let np = self.b.ins().fneg(p);
                let mut e = self.b.ins().fma(h, x, np);
                if let Some(lo) = l {
                    e = self.b.ins().fma(lo, x, e);
                }
                (h, l) = self.fast_two_sum(p, e);
                last_p = p;
            }
        }
        let (r, plain) = if n < 0 {
            // 1 / (h + l), corrected once
            let one = self.cst(1.0);
            let q = self.b.ins().fdiv(one, h);
            let nq = self.b.ins().fneg(q);
            let rem = self.b.ins().fma(nq, h, one);
            let rem = match l {
                Some(lo) => self.b.ins().fma(nq, lo, rem),
                None => rem,
            };
            let r = self.b.ins().fma(q, rem, q);
            let plain = self.b.ins().fdiv(one, last_p);
            (r, plain)
        } else {
            (h, last_p)
        };
        // keep r when it is finite and the power is not zero
        let zero = self.cst(0.0);
        let rr = self.b.ins().fsub(r, r);
        let finite = self.b.ins().fcmp(FloatCC::Equal, rr, zero);
        let nonzero = self.b.ins().fcmp(FloatCC::NotEqual, last_p, zero);
        let ok = self.b.ins().band(finite, nonzero);
        self.b.ins().select(ok, r, plain)
    }

    /// (s, e) with s = fl(p + e) and s + e' = p + e exactly (|p| ≥ |e|).
    fn fast_two_sum(&mut self, p: Value, e: Value) -> (Value, Option<Value>) {
        let s = self.b.ins().fadd(p, e);
        let d = self.b.ins().fsub(s, p);
        let lo = self.b.ins().fsub(e, d);
        (s, Some(lo))
    }

    /// a^n for a constant n.
    fn pow_const(&mut self, a: &D, n: f64) -> D {
        let (va, ta) = (a.v, &a.t);
        let dual = self.dual() && !ta.is_empty();
        if n == 0.0 {
            return D { v: self.cst(1.0), t: vec![] };
        }
        if n == 1.0 {
            return a.clone();
        }
        let (v, dv) = if n == 2.0 {
            let v = self.b.ins().fmul(va, va);
            let d = dual.then(|| {
                let two = self.cst(2.0);
                self.b.ins().fmul(two, va)
            });
            (v, d)
        } else if n == -1.0 {
            let one = self.cst(1.0);
            let v = self.b.ins().fdiv(one, va);
            let d = dual.then(|| {
                let v2 = self.b.ins().fmul(v, v);
                self.b.ins().fneg(v2)
            });
            (v, d)
        } else if n == 0.5 {
            // pow(-0, 0.5) = +0 and pow(-inf, 0.5) = +inf, unlike sqrt
            let s = self.b.ins().sqrt(va);
            let zero = self.cst(0.0);
            let s = self.b.ins().fadd(s, zero);
            let ninf = self.cst(f64::NEG_INFINITY);
            let pinf = self.cst(f64::INFINITY);
            let is_ninf = self.b.ins().fcmp(FloatCC::Equal, va, ninf);
            let v = self.b.ins().select(is_ninf, pinf, s);
            let d = dual.then(|| {
                let half = self.cst(0.5);
                self.b.ins().fdiv(half, v)
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
                    self.b.ins().fdiv(one, p)
                };
                let cn = self.cst(n);
                self.b.ins().fmul(cn, xm)
            });
            (v, d)
        } else {
            let cn = self.cst(n);
            let v = self.call("lsim_pow", &[va, cn]);
            let d = dual.then(|| {
                let cm = self.cst(n - 1.0);
                let pm1 = self.call("lsim_pow", &[va, cm]);
                self.b.ins().fmul(cn, pm1)
            });
            (v, d)
        };
        let t = match dv {
            Some(d) => self.scale(d, ta),
            None => vec![],
        };
        D { v, t }
    }

    /// Lowers an expression to its value and tangent.
    pub(crate) fn lower(&mut self, e: &Expr) -> Result<D, CodegenError> {
        Ok(match e {
            Expr::Const(x) => D { v: self.cst(*x), t: vec![] },
            Expr::Time => D { v: self.a.t, t: vec![] },
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
            Expr::Param(p) => D { v: self.load(1, self.a.p, p.0 as usize), t: vec![] },
            Expr::Name(n) => {
                return Err(CodegenError::Unsupported(format!("unresolved name '{n}'")));
            }
            Expr::Neg(a) => {
                let a = self.lower(a)?;
                let v = self.b.ins().fneg(a.v);
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
                    CmpOp::Lt => FloatCC::LessThan,
                    CmpOp::Le => FloatCC::LessThanOrEqual,
                    CmpOp::Gt => FloatCC::GreaterThan,
                    CmpOp::Ge => FloatCC::GreaterThanOrEqual,
                };
                let c = self.b.ins().fcmp(cc, a.v, b.v);
                D { v: self.truth(c), t: vec![] }
            }
            Expr::And(a, b) | Expr::Or(a, b) => {
                let a = self.lower(a)?;
                let b = self.lower(b)?;
                let (ca, cb) = (self.is_true(a.v), self.is_true(b.v));
                let c = if matches!(e, Expr::And(..)) {
                    self.b.ins().band(ca, cb)
                } else {
                    self.b.ins().bor(ca, cb)
                };
                D { v: self.truth(c), t: vec![] }
            }
            Expr::Not(a) => {
                let a = self.lower(a)?;
                let zero = self.cst(0.0);
                let c = self.b.ins().fcmp(FloatCC::Equal, a.v, zero);
                D { v: self.truth(c), t: vec![] }
            }
            Expr::If(c, a, b) => {
                let c = self.lower(c)?;
                let cond = self.is_true(c.v);
                let a = self.lower(a)?;
                let b = self.lower(b)?;
                let v = self.b.ins().select(cond, a.v, b.v);
                let t = self.select_t(cond, &a.t, &b.t);
                D { v, t }
            }
            Expr::Table { table, args } => self.table(*table, args)?,
        })
    }

    fn binary(&mut self, op: BinaryOp, a: D, b: D) -> D {
        match op {
            BinaryOp::Add | BinaryOp::Sub => {
                let sub = op == BinaryOp::Sub;
                let v = if sub { self.b.ins().fsub(a.v, b.v) } else { self.b.ins().fadd(a.v, b.v) };
                let t = self.add_t(&a.t, &b.t, sub);
                D { v, t }
            }
            BinaryOp::Mul => {
                let v = self.b.ins().fmul(a.v, b.v);
                // d(ab) = b da + a db
                let t = self.lin2(b.v, &a.t, a.v, &b.t);
                D { v, t }
            }
            BinaryOp::Div => {
                let v = self.b.ins().fdiv(a.v, b.v);
                // d(a/b) = (da - (a/b) db) / b
                let t = if a.t.is_empty() && b.t.is_empty() {
                    vec![]
                } else {
                    let nq = self.b.ins().fneg(v);
                    let x = self.scale(nq, &b.t);
                    let s = self.add_t(&a.t, &x, false);
                    s.into_iter()
                        .map(|(d, x)| {
                            let xv = self.tv(x);
                            (d, Tv::V(self.b.ins().fdiv(xv, b.v)))
                        })
                        .collect()
                };
                D { v, t }
            }
            BinaryOp::Pow => {
                let v = self.call("lsim_pow", &[a.v, b.v]);
                let mut t = vec![];
                if self.dual() && !a.t.is_empty() {
                    // b a^(b-1) da
                    let one = self.cst(1.0);
                    let bm1 = self.b.ins().fsub(b.v, one);
                    let pm1 = self.call("lsim_pow", &[a.v, bm1]);
                    let f = self.b.ins().fmul(b.v, pm1);
                    t = self.scale(f, &a.t);
                }
                if self.dual() && !b.t.is_empty() {
                    // a^b ln(a) db
                    let ln = self.call("lsim_log", &[a.v]);
                    let f = self.b.ins().fmul(v, ln);
                    let tb = self.scale(f, &b.t);
                    t = self.add_t(&t, &tb, false);
                }
                D { v, t }
            }
        }
    }

    fn builtin(&mut self, f: Builtin, args: &[Expr]) -> Result<D, CodegenError> {
        let mut vals = Vec::with_capacity(args.len());
        for a in args {
            vals.push(self.lower(a)?);
        }
        let a = vals[0].clone();
        let dual = self.dual() && !a.t.is_empty();
        let unary = |s: &mut Self, name: &'static str| s.call(name, &[a.v]);
        // the value, and d(value)/d(argument) when a tangent is needed
        let (v, dv): (Value, Option<Value>) = match f {
            Builtin::Der | Builtin::Pre => {
                return Err(CodegenError::Unsupported("der/pre in component scope".into()));
            }
            Builtin::Sqrt => {
                let v = self.b.ins().sqrt(a.v);
                let d = dual.then(|| {
                    let half = self.cst(0.5);
                    self.b.ins().fdiv(half, v)
                });
                (v, d)
            }
            Builtin::Abs => {
                let v = self.b.ins().fabs(a.v);
                let d = dual.then(|| self.sign(a.v));
                (v, d)
            }
            Builtin::Sign => (self.sign(a.v), None),
            Builtin::Exp => {
                let v = unary(self, "lsim_exp");
                (v, dual.then_some(v))
            }
            Builtin::Log => {
                let v = unary(self, "lsim_log");
                let d = dual.then(|| {
                    let one = self.cst(1.0);
                    self.b.ins().fdiv(one, a.v)
                });
                (v, d)
            }
            Builtin::Sin => {
                let v = unary(self, "lsim_sin");
                let d = dual.then(|| unary(self, "lsim_cos"));
                (v, d)
            }
            Builtin::Cos => {
                let v = unary(self, "lsim_cos");
                let d = dual.then(|| {
                    let s = unary(self, "lsim_sin");
                    self.b.ins().fneg(s)
                });
                (v, d)
            }
            Builtin::Tan => {
                let v = unary(self, "lsim_tan");
                let d = dual.then(|| {
                    let one = self.cst(1.0);
                    let v2 = self.b.ins().fmul(v, v);
                    self.b.ins().fadd(one, v2)
                });
                (v, d)
            }
            Builtin::Asin | Builtin::Acos => {
                let name = if f == Builtin::Asin { "lsim_asin" } else { "lsim_acos" };
                let v = unary(self, name);
                let d = dual.then(|| {
                    let one = self.cst(1.0);
                    let a2 = self.b.ins().fmul(a.v, a.v);
                    let s = self.b.ins().fsub(one, a2);
                    let r = self.b.ins().sqrt(s);
                    let q = self.b.ins().fdiv(one, r);
                    if f == Builtin::Acos { self.b.ins().fneg(q) } else { q }
                });
                (v, d)
            }
            Builtin::Atan => {
                let v = unary(self, "lsim_atan");
                let d = dual.then(|| {
                    let one = self.cst(1.0);
                    let a2 = self.b.ins().fmul(a.v, a.v);
                    let s = self.b.ins().fadd(one, a2);
                    self.b.ins().fdiv(one, s)
                });
                (v, d)
            }
            Builtin::Sinh => {
                let v = unary(self, "lsim_sinh");
                let d = dual.then(|| unary(self, "lsim_cosh"));
                (v, d)
            }
            Builtin::Cosh => {
                let v = unary(self, "lsim_cosh");
                let d = dual.then(|| unary(self, "lsim_sinh"));
                (v, d)
            }
            Builtin::Tanh => {
                let v = unary(self, "lsim_tanh");
                let d = dual.then(|| {
                    let one = self.cst(1.0);
                    let v2 = self.b.ins().fmul(v, v);
                    self.b.ins().fsub(one, v2)
                });
                (v, d)
            }
            Builtin::Atan2 => {
                let x = vals[1].clone();
                let v = self.call("lsim_atan2", &[a.v, x.v]);
                let t = if self.dual() && !(a.t.is_empty() && x.t.is_empty()) {
                    // (x dy - y dx) / (x² + y²)
                    let x2 = self.b.ins().fmul(x.v, x.v);
                    let y2 = self.b.ins().fmul(a.v, a.v);
                    let den = self.b.ins().fadd(x2, y2);
                    let fy = self.b.ins().fdiv(x.v, den);
                    let ny = self.b.ins().fneg(a.v);
                    let fx = self.b.ins().fdiv(ny, den);
                    self.lin2(fy, &a.t, fx, &x.t)
                } else {
                    vec![]
                };
                return Ok(D { v, t });
            }
            Builtin::Min | Builtin::Max | Builtin::Limit => {
                return Ok(match f {
                    Builtin::Min => self.pick(FloatCC::LessThan, &vals[0], &vals[1]),
                    Builtin::Max => self.pick(FloatCC::GreaterThan, &vals[0], &vals[1]),
                    _ => {
                        let lo = self.pick(FloatCC::GreaterThan, &vals[0], &vals[1]);
                        self.pick(FloatCC::LessThan, &lo, &vals[2])
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

    /// Rust's `f64::max` (`GreaterThan`) or `f64::min` (`LessThan`): the
    /// other argument when one is NaN.
    fn pick(&mut self, cc: FloatCC, a: &D, b: &D) -> D {
        let c1 = self.b.ins().fcmp(cc, a.v, b.v);
        let bnan = self.b.ins().fcmp(FloatCC::Unordered, b.v, b.v);
        let c = self.b.ins().bor(c1, bnan);
        let v = self.b.ins().select(c, a.v, b.v);
        let t = self.select_t(c, &a.t, &b.t);
        D { v, t }
    }

    fn scratch(&mut self) -> Value {
        let ss = match self.scratch {
            Some(s) => s,
            None => {
                let s = self.b.create_sized_stack_slot(StackSlotData::new(
                    StackSlotKind::ExplicitSlot,
                    16,
                    3,
                ));
                self.scratch = Some(s);
                s
            }
        };
        self.b.ins().stack_addr(I64, ss, 0)
    }

    fn table_ptr(&mut self, table: u32) -> Value {
        self.b.ins().load(I64, mem(), self.a.tabs, (8 * table) as i32)
    }

    fn table(&mut self, table: u32, args: &[Expr]) -> Result<D, CodegenError> {
        let mut vals = Vec::with_capacity(args.len());
        for a in args {
            vals.push(self.lower(a)?);
        }
        let ptr = self.table_ptr(table);
        let need_d = self.dual() && vals.iter().any(|d| !d.t.is_empty());
        let mut call_args = vec![ptr];
        call_args.extend(vals.iter().map(|d| d.v));
        let name = match (vals.len(), need_d) {
            (1, false) => "lsim_tab1",
            (1, true) => "lsim_tab1d",
            (_, false) => "lsim_tab2",
            (_, true) => "lsim_tab2d",
        };
        if !need_d {
            let v = self.call(name, &call_args);
            return Ok(D { v, t: vec![] });
        }
        let addr = self.scratch();
        call_args.push(addr);
        let v = self.call(name, &call_args);
        let mut t = vec![];
        for (k, d) in vals.iter().enumerate() {
            if d.t.is_empty() {
                continue;
            }
            let g = self.b.ins().load(F64, mem(), addr, (8 * k) as i32);
            let s = self.scale(g, &d.t);
            t = self.add_t(&t, &s, false);
        }
        Ok(D { v, t })
    }

    /// A table axis's guard at argument `x`.
    pub(crate) fn table_guard(&mut self, table: u32, axis: u8, x: Value) -> Value {
        let ptr = self.table_ptr(table);
        let ax = self.b.ins().iconst(I64, axis as i64);
        self.call("lsim_tab_guard", &[ptr, ax, x])
    }
}
