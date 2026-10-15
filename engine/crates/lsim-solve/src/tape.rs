//! A sampled block's computed input as a flat tape: the chain of
//! assignments [`InputChain`] holds, lowered once to a list of operations
//! on a small register file, so that a tick evaluates it without walking
//! expression trees (DESIGN.md §9: an idle tick's cost).
//!
//! Every operation is the one [`lsim_ir::eval::eval`] performs for its
//! node, on the same operands in the same order, so a tape gives the
//! interpreter's values exactly (tested bit for bit on random expressions,
//! NaN and signed zeros included). Two things Rust leaves unspecified are
//! the exception in principle: which NaN comes out of an operation on
//! NaNs, and the sign of `min`/`max` of +0 and −0; this toolchain gives
//! the same bits there too (the test). Evaluation is pure, so the branches of an `if`
//! and the right operand of `and`/`or` are evaluated eagerly and then
//! selected: the value is the one the interpreter's short-circuit gives.
//! What the interpreter does not define the same way everywhere (a
//! function called with too few arguments, a table, an unresolved name) is
//! not lowered: such a chain keeps the interpreter.

use crate::info::InputChain;
use lsim_ir::expr::{BinaryOp, Builtin, CmpOp, Expr};
use lsim_ir::math;
use std::collections::HashMap;

/// One operation: its destination register first, then its operands'.
#[derive(Clone, Copy, Debug)]
enum Op {
    Neg(u32, u32),
    Add(u32, u32, u32),
    Sub(u32, u32, u32),
    Mul(u32, u32, u32),
    Div(u32, u32, u32),
    Pow(u32, u32, u32),
    Call1(Builtin, u32, u32),
    Call2(Builtin, u32, u32, u32),
    Limit(u32, u32, u32, u32),
    Cmp(CmpOp, u32, u32, u32),
    And(u32, u32, u32),
    Or(u32, u32, u32),
    Not(u32, u32),
    Select(u32, u32, u32, u32),
    Copy(u32, u32),
}

/// A lowered [`InputChain`].
#[derive(Clone, Debug)]
pub(crate) struct ChainTape {
    ops: Vec<Op>,
    /// the registers: the chain's variables, constants and parameters
    /// (written once), time, temporaries
    init: Vec<f64>,
    time: u32,
    /// the register of each entry of y the chain reads, in `from_y`'s order
    from_y: Vec<u32>,
    /// (register, entry of d)
    from_d: Vec<(u32, usize)>,
    /// (register, entry of u)
    from_u: Vec<(u32, usize)>,
    result: (u32, f64),
}

struct Lower<'a> {
    ops: Vec<Op>,
    regs: Vec<f64>,
    slot: HashMap<(usize, bool), u32>,
    consts: HashMap<u64, u32>,
    params: HashMap<u32, u32>,
    values: &'a [f64],
    time: u32,
}

impl Lower<'_> {
    fn reg(&mut self, v: f64) -> u32 {
        self.regs.push(v);
        (self.regs.len() - 1) as u32
    }
    fn slot(&mut self, var: usize, der: bool) -> u32 {
        if let Some(&r) = self.slot.get(&(var, der)) {
            return r;
        }
        let r = self.reg(f64::NAN);
        self.slot.insert((var, der), r);
        r
    }
    fn konst(&mut self, v: f64) -> u32 {
        if let Some(&r) = self.consts.get(&v.to_bits()) {
            return r;
        }
        let r = self.reg(v);
        self.consts.insert(v.to_bits(), r);
        r
    }
    /// The register holding `e`'s value.
    fn expr(&mut self, e: &Expr) -> Option<u32> {
        Some(match e {
            Expr::Const(v) => self.konst(*v),
            Expr::Time => self.time,
            Expr::Var(v) | Expr::Pre(v) => self.slot(v.0 as usize, false),
            Expr::Der(v) => self.slot(v.0 as usize, true),
            Expr::Param(p) => {
                if let Some(&r) = self.params.get(&p.0) {
                    return Some(r);
                }
                let r = self.reg(*self.values.get(p.0 as usize)?);
                self.params.insert(p.0, r);
                r
            }
            Expr::NoEvent(a) => self.expr(a)?,
            Expr::Neg(a) => {
                let a = self.expr(a)?;
                let r = self.reg(0.0);
                self.ops.push(Op::Neg(r, a));
                r
            }
            Expr::Binary(op, a, b) => {
                let (a, b) = (self.expr(a)?, self.expr(b)?);
                let r = self.reg(0.0);
                self.ops.push(match op {
                    BinaryOp::Add => Op::Add(r, a, b),
                    BinaryOp::Sub => Op::Sub(r, a, b),
                    BinaryOp::Mul => Op::Mul(r, a, b),
                    BinaryOp::Div => Op::Div(r, a, b),
                    BinaryOp::Pow => Op::Pow(r, a, b),
                });
                r
            }
            Expr::Call(f, args) => match f {
                Builtin::Der | Builtin::Pre => self.konst(f64::NAN),
                Builtin::Atan2 | Builtin::Min | Builtin::Max => {
                    let [a, b] = args.as_slice() else { return None };
                    let (a, b) = (self.expr(a)?, self.expr(b)?);
                    let r = self.reg(0.0);
                    self.ops.push(Op::Call2(*f, r, a, b));
                    r
                }
                Builtin::Limit => {
                    let [a, b, c] = args.as_slice() else { return None };
                    let (a, b, c) = (self.expr(a)?, self.expr(b)?, self.expr(c)?);
                    let r = self.reg(0.0);
                    self.ops.push(Op::Limit(r, a, b, c));
                    r
                }
                _ => {
                    let [a] = args.as_slice() else { return None };
                    let a = self.expr(a)?;
                    let r = self.reg(0.0);
                    self.ops.push(Op::Call1(*f, r, a));
                    r
                }
            },
            Expr::Compare(op, a, b) => {
                let (a, b) = (self.expr(a)?, self.expr(b)?);
                let r = self.reg(0.0);
                self.ops.push(Op::Cmp(*op, r, a, b));
                r
            }
            Expr::And(a, b) | Expr::Or(a, b) => {
                let (x, y) = (self.expr(a)?, self.expr(b)?);
                let r = self.reg(0.0);
                self.ops.push(if matches!(e, Expr::And(..)) {
                    Op::And(r, x, y)
                } else {
                    Op::Or(r, x, y)
                });
                r
            }
            Expr::Not(a) => {
                let a = self.expr(a)?;
                let r = self.reg(0.0);
                self.ops.push(Op::Not(r, a));
                r
            }
            Expr::If(c, a, b) => {
                let (c, a, b) = (self.expr(c)?, self.expr(a)?, self.expr(b)?);
                let r = self.reg(0.0);
                self.ops.push(Op::Select(r, c, a, b));
                r
            }
            Expr::Table { .. } | Expr::Name(_) => return None,
        })
    }
}

impl Op {
    fn dst(&self) -> u32 {
        match *self {
            Op::Neg(o, _)
            | Op::Add(o, ..)
            | Op::Sub(o, ..)
            | Op::Mul(o, ..)
            | Op::Div(o, ..)
            | Op::Pow(o, ..)
            | Op::Call1(_, o, _)
            | Op::Call2(_, o, ..)
            | Op::Limit(o, ..)
            | Op::Cmp(_, o, ..)
            | Op::And(o, ..)
            | Op::Or(o, ..)
            | Op::Not(o, _)
            | Op::Select(o, ..)
            | Op::Copy(o, _) => o,
        }
    }
    /// The registers it reads.
    fn operands(&self) -> Vec<u32> {
        match *self {
            Op::Neg(_, a) | Op::Call1(_, _, a) | Op::Not(_, a) | Op::Copy(_, a) => vec![a],
            Op::Add(_, a, b)
            | Op::Sub(_, a, b)
            | Op::Mul(_, a, b)
            | Op::Div(_, a, b)
            | Op::Pow(_, a, b)
            | Op::Call2(_, _, a, b)
            | Op::Cmp(_, _, a, b)
            | Op::And(_, a, b)
            | Op::Or(_, a, b) => vec![a, b],
            Op::Limit(_, a, b, c) | Op::Select(_, a, b, c) => vec![a, b, c],
        }
    }
    fn set_dst(&mut self, d: u32) {
        match self {
            Op::Neg(o, _)
            | Op::Add(o, ..)
            | Op::Sub(o, ..)
            | Op::Mul(o, ..)
            | Op::Div(o, ..)
            | Op::Pow(o, ..)
            | Op::Call1(_, o, _)
            | Op::Call2(_, o, ..)
            | Op::Limit(o, ..)
            | Op::Cmp(_, o, ..)
            | Op::And(o, ..)
            | Op::Or(o, ..)
            | Op::Not(o, _)
            | Op::Select(o, ..)
            | Op::Copy(o, _) => *o = d,
        }
    }
}

/// The most lanes [`ChainTape::eval_lanes`] runs.
pub(crate) const TAPE_LANES: usize = 64;

/// The registers' rows of [`ChainTape::eval_lanes`]: `lanes` apart, `n`
/// used.
struct Rows {
    lanes: usize,
    n: usize,
}

impl Rows {
    fn at<'r>(&self, r: &'r [f64], k: u32) -> &'r [f64] {
        &r[k as usize * self.lanes..][..self.n]
    }
}

/// A short row copied in line (no call of `memcpy` for a few values).
fn copy(out: &mut [f64], a: &[f64]) {
    for (x, a) in out.iter_mut().zip(a) {
        *x = *a;
    }
}

fn un(out: &mut [f64], a: &[f64], f: impl Fn(f64) -> f64) {
    for (x, a) in out.iter_mut().zip(a) {
        *x = f(*a);
    }
}

fn bin(out: &mut [f64], a: &[f64], b: &[f64], f: impl Fn(f64, f64) -> f64) {
    for ((x, a), b) in out.iter_mut().zip(a).zip(b) {
        *x = f(*a, *b);
    }
}

fn truth(b: bool) -> f64 {
    if b { 1.0 } else { 0.0 }
}

/// A one-argument builtin, as the interpreter computes it.
fn call1(f: Builtin, a: f64) -> f64 {
    match f {
        Builtin::Sin => math::sin(a),
        Builtin::Cos => math::cos(a),
        Builtin::Tan => math::tan(a),
        Builtin::Asin => math::asin(a),
        Builtin::Acos => math::acos(a),
        Builtin::Atan => math::atan(a),
        Builtin::Sinh => math::sinh(a),
        Builtin::Cosh => math::cosh(a),
        Builtin::Tanh => math::tanh(a),
        Builtin::Exp => math::exp(a),
        Builtin::Log => math::ln(a),
        Builtin::Sqrt => a.sqrt(),
        Builtin::Abs => a.abs(),
        Builtin::Sign => {
            if a > 0.0 {
                1.0
            } else if a < 0.0 {
                -1.0
            } else {
                0.0
            }
        }
        // (lowered as constants or calls of two or three arguments)
        Builtin::Der
        | Builtin::Pre
        | Builtin::Atan2
        | Builtin::Min
        | Builtin::Max
        | Builtin::Limit => f64::NAN,
    }
}

/// A two-argument builtin, as the interpreter computes it.
fn call2(f: Builtin, a: f64, b: f64) -> f64 {
    match f {
        Builtin::Atan2 => math::atan2(a, b),
        Builtin::Min => a.min(b),
        _ => a.max(b),
    }
}

fn compare(op: CmpOp, a: f64, b: f64) -> f64 {
    truth(match op {
        CmpOp::Lt => a < b,
        CmpOp::Le => a <= b,
        CmpOp::Gt => a > b,
        CmpOp::Ge => a >= b,
    })
}

impl ChainTape {
    /// Lowers `chain` with the run's parameter values (fixed for the run);
    /// `None` where the chain needs the interpreter.
    pub(crate) fn new(chain: &InputChain, params: &[f64]) -> Option<Self> {
        let mut lw = Lower {
            ops: vec![],
            regs: vec![f64::NAN],
            slot: HashMap::new(),
            consts: HashMap::new(),
            params: HashMap::new(),
            values: params,
            time: 0,
        };
        let from_y: Vec<u32> = chain.from_y.iter().map(|&(v, der, _)| lw.slot(v, der)).collect();
        let from_d: Vec<(u32, usize)> =
            chain.from_d.iter().map(|&(v, i)| (lw.slot(v, false), i)).collect();
        let from_u: Vec<(u32, usize)> =
            chain.from_u.iter().map(|&(v, i)| (lw.slot(v, false), i)).collect();
        for (var, der, e) in &chain.steps {
            let n_regs = lw.regs.len();
            let r = lw.expr(e)?;
            let dst = lw.slot(*var, *der);
            // the step's last operation writes the variable itself when its
            // result is a temporary of this step (nothing else reads it)
            match lw.ops.last_mut() {
                Some(op) if r as usize >= n_regs && op.dst() == r => op.set_dst(dst),
                _ => lw.ops.push(Op::Copy(dst, r)),
            }
        }
        let result = (lw.slot(chain.result.0, false), chain.result.1);
        // every register read is written before it, in each evaluation:
        // what the chain reads, constants, parameters, time, earlier
        // operations (else a value would carry over from the evaluation
        // before, as the interpreter's scratch carries it: the
        // interpreter keeps such a chain)
        let mut written = vec![false; lw.regs.len()];
        written[lw.time as usize] = true;
        for &r in lw.consts.values().chain(lw.params.values()) {
            written[r as usize] = true;
        }
        for &r in &from_y {
            written[r as usize] = true;
        }
        for &(r, _) in from_d.iter().chain(&from_u) {
            written[r as usize] = true;
        }
        for op in &lw.ops {
            if !op.operands().iter().all(|&a| written[a as usize]) {
                return None;
            }
            written[op.dst() as usize] = true;
        }
        if !written[result.0 as usize] {
            return None;
        }
        Some(ChainTape {
            ops: lw.ops,
            init: lw.regs,
            time: lw.time,
            from_y,
            from_d,
            from_u,
            result,
        })
    }

    /// Scratch registers for [`Self::eval`].
    pub(crate) fn registers(&self) -> Vec<f64> {
        self.init.clone()
    }

    /// Scratch registers for [`Self::eval_lanes`] with `lanes` lanes.
    pub(crate) fn lane_registers(&self, lanes: usize) -> Vec<f64> {
        self.init.iter().flat_map(|&v| std::iter::repeat_n(v, lanes)).collect()
    }

    /// [`Self::eval`] at several times at once, the same bits in each
    /// lane: `out[i]` at `ts[i]` with `yv[i·stride + pos[k]]` the entry of
    /// y `from_y[k]` reads there; `r` from [`Self::lane_registers`] with
    /// `lanes` lanes, `ts.len() ≤ lanes ≤` [`TAPE_LANES`]. The discrete values and inputs are the
    /// same for every lane.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn eval_lanes(
        &self,
        ts: &[f64],
        yv: &[f64],
        stride: usize,
        pos: &[usize],
        d: &[f64],
        u: &[f64],
        lanes: usize,
        r: &mut [f64],
        out: &mut [f64],
    ) {
        let n = ts.len();
        let row = |k: u32| k as usize * lanes;
        copy(&mut r[row(self.time)..][..n], ts);
        for (&k, &p) in self.from_y.iter().zip(pos) {
            for (x, y) in r[row(k)..][..n].iter_mut().zip(yv.iter().skip(p).step_by(stride)) {
                *x = *y;
            }
        }
        for &(k, i) in &self.from_d {
            r[row(k)..][..n].fill(d[i]);
        }
        for &(k, i) in &self.from_u {
            r[row(k)..][..n].fill(u[i]);
        }
        // each operation into `tmp` from its operands' rows, then into its
        // destination's row (lane by lane the interpreter's operation)
        let mut tmp = [0.0; TAPE_LANES];
        let tmp = &mut tmp[..n];
        let rows = Rows { lanes, n };
        for op in &self.ops {
            let o = match *op {
                Op::Copy(o, a) => {
                    copy(tmp, rows.at(r, a));
                    o
                }
                Op::Neg(o, a) => {
                    un(tmp, rows.at(r, a), |a| -a);
                    o
                }
                Op::Add(o, a, b) => {
                    bin(tmp, rows.at(r, a), rows.at(r, b), |a, b| a + b);
                    o
                }
                Op::Sub(o, a, b) => {
                    bin(tmp, rows.at(r, a), rows.at(r, b), |a, b| a - b);
                    o
                }
                Op::Mul(o, a, b) => {
                    bin(tmp, rows.at(r, a), rows.at(r, b), |a, b| a * b);
                    o
                }
                Op::Div(o, a, b) => {
                    bin(tmp, rows.at(r, a), rows.at(r, b), |a, b| a / b);
                    o
                }
                Op::Pow(o, a, b) => {
                    bin(tmp, rows.at(r, a), rows.at(r, b), math::pow);
                    o
                }
                Op::Call1(f, o, a) => {
                    un(tmp, rows.at(r, a), |a| call1(f, a));
                    o
                }
                Op::Call2(f, o, a, b) => {
                    bin(tmp, rows.at(r, a), rows.at(r, b), |a, b| call2(f, a, b));
                    o
                }
                Op::Limit(o, a, b, c) => {
                    bin(tmp, rows.at(r, a), rows.at(r, b), |a, b| a.max(b));
                    let c = rows.at(r, c);
                    for (x, c) in tmp.iter_mut().zip(c) {
                        *x = x.min(*c);
                    }
                    o
                }
                Op::Cmp(op, o, a, b) => {
                    bin(tmp, rows.at(r, a), rows.at(r, b), |a, b| compare(op, a, b));
                    o
                }
                Op::And(o, a, b) => {
                    bin(tmp, rows.at(r, a), rows.at(r, b), |a, b| truth(a != 0.0 && b != 0.0));
                    o
                }
                Op::Or(o, a, b) => {
                    bin(tmp, rows.at(r, a), rows.at(r, b), |a, b| truth(a != 0.0 || b != 0.0));
                    o
                }
                Op::Not(o, a) => {
                    un(tmp, rows.at(r, a), |a| truth(a == 0.0));
                    o
                }
                Op::Select(o, c, a, b) => {
                    let (c, a, b) = (rows.at(r, c), rows.at(r, a), rows.at(r, b));
                    for (i, x) in tmp.iter_mut().enumerate() {
                        *x = if c[i] != 0.0 { a[i] } else { b[i] };
                    }
                    o
                }
            };
            copy(&mut r[o as usize * lanes..][..n], tmp);
        }
        let (k, sign) = self.result;
        for (x, &v) in out.iter_mut().zip(&r[row(k)..][..n]) {
            *x = sign * v;
        }
    }

    /// The input's value at `t`, as [`InputChain::eval`] gives it:
    /// `yv[pos[k]]` holds the entry of y `from_y[k]` reads; `r` is from
    /// [`Self::registers`].
    pub(crate) fn eval(
        &self,
        t: f64,
        yv: &[f64],
        pos: &[usize],
        d: &[f64],
        u: &[f64],
        r: &mut [f64],
    ) -> f64 {
        r[self.time as usize] = t;
        for (&k, &p) in self.from_y.iter().zip(pos) {
            r[k as usize] = yv[p];
        }
        for &(k, i) in &self.from_d {
            r[k as usize] = d[i];
        }
        for &(k, i) in &self.from_u {
            r[k as usize] = u[i];
        }
        for op in &self.ops {
            let (dst, x) = match *op {
                Op::Copy(o, a) => (o, r[a as usize]),
                Op::Neg(o, a) => (o, -r[a as usize]),
                Op::Add(o, a, b) => (o, r[a as usize] + r[b as usize]),
                Op::Sub(o, a, b) => (o, r[a as usize] - r[b as usize]),
                Op::Mul(o, a, b) => (o, r[a as usize] * r[b as usize]),
                Op::Div(o, a, b) => (o, r[a as usize] / r[b as usize]),
                Op::Pow(o, a, b) => (o, math::pow(r[a as usize], r[b as usize])),
                Op::Call1(f, o, a) => (o, call1(f, r[a as usize])),
                Op::Call2(f, o, a, b) => (o, call2(f, r[a as usize], r[b as usize])),
                Op::Limit(o, a, b, c) => (o, r[a as usize].max(r[b as usize]).min(r[c as usize])),
                Op::Cmp(op, o, a, b) => (o, compare(op, r[a as usize], r[b as usize])),
                Op::And(o, a, b) => (o, truth(r[a as usize] != 0.0 && r[b as usize] != 0.0)),
                Op::Or(o, a, b) => (o, truth(r[a as usize] != 0.0 || r[b as usize] != 0.0)),
                Op::Not(o, a) => (o, truth(r[a as usize] == 0.0)),
                Op::Select(o, c, a, b) => {
                    (o, if r[c as usize] != 0.0 { r[a as usize] } else { r[b as usize] })
                }
            };
            r[dst as usize] = x;
        }
        self.result.1 * r[self.result.0 as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsim_ir::flat::{ParamId, VarId};

    /// xorshift64*: reproducible, no dependency
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }
        /// values that tell bits apart: signed zeros, infinities, NaN,
        /// tiny and huge numbers, ordinary ones
        fn value(&mut self) -> f64 {
            match self.below(12) {
                0 => 0.0,
                1 => -0.0,
                2 => f64::INFINITY,
                3 => f64::NEG_INFINITY,
                4 => f64::NAN,
                5 => 1e-310,
                6 => 1e300,
                7 => 1.0,
                _ => (self.next() as f64 / u64::MAX as f64 - 0.5) * 8.0,
            }
        }
    }

    const BUILTINS: [Builtin; 20] = [
        Builtin::Der,
        Builtin::Pre,
        Builtin::Sin,
        Builtin::Cos,
        Builtin::Tan,
        Builtin::Asin,
        Builtin::Acos,
        Builtin::Atan,
        Builtin::Atan2,
        Builtin::Sinh,
        Builtin::Cosh,
        Builtin::Tanh,
        Builtin::Exp,
        Builtin::Log,
        Builtin::Sqrt,
        Builtin::Abs,
        Builtin::Sign,
        Builtin::Min,
        Builtin::Max,
        Builtin::Limit,
    ];

    /// A random expression reading the variables `vars` (and their
    /// derivatives `ders`) and parameters 0..3.
    fn expr(g: &mut Rng, depth: usize, vars: &[usize], ders: &[usize]) -> Expr {
        let leaf = depth == 0 || g.below(4) == 0;
        if leaf {
            return match g.below(6) {
                0 => Expr::Const(g.value()),
                1 => Expr::Time,
                2 => Expr::Param(ParamId(g.below(3) as u32)),
                3 if !ders.is_empty() => Expr::Der(VarId(ders[g.below(ders.len())] as u32)),
                4 => Expr::Pre(VarId(vars[g.below(vars.len())] as u32)),
                _ => Expr::Var(VarId(vars[g.below(vars.len())] as u32)),
            };
        }
        let ops = [BinaryOp::Add, BinaryOp::Sub, BinaryOp::Mul, BinaryOp::Div, BinaryOp::Pow];
        let cmps = [CmpOp::Lt, CmpOp::Le, CmpOp::Gt, CmpOp::Ge];
        let kind = g.below(9);
        let sub = |g: &mut Rng| Box::new(expr(g, depth - 1, vars, ders));
        match kind {
            0 => Expr::Neg(sub(g)),
            1 | 2 => {
                let op = ops[g.below(5)];
                Expr::Binary(op, sub(g), sub(g))
            }
            3 => {
                let f = BUILTINS[g.below(BUILTINS.len())];
                let args = (0..f.arity()).map(|_| *sub(g)).collect();
                Expr::Call(f, args)
            }
            4 => {
                let op = cmps[g.below(4)];
                Expr::Compare(op, sub(g), sub(g))
            }
            5 => {
                if g.below(2) == 0 {
                    Expr::And(sub(g), sub(g))
                } else {
                    Expr::Or(sub(g), sub(g))
                }
            }
            6 => Expr::Not(sub(g)),
            7 => Expr::If(sub(g), sub(g), sub(g)),
            _ => Expr::NoEvent(sub(g)),
        }
    }

    /// Random chains: the tape gives the interpreter's bits, one time
    /// at a time and across many at once (lanes), whatever the values.
    #[test]
    fn a_tape_gives_the_interpreters_bits() {
        let mut g = Rng(0x9e37_79b9_7f4a_7c15);
        let n_vars = 12;
        let mut lowered = 0;
        for case in 0..400 {
            // variables 0..3 from y (2 as a derivative), 4 from d, 5 from
            // u; the steps assign 6.. in order, each reading what is known
            let from_y = vec![(0, false, 0), (1, false, 1), (2, true, 2), (3, false, 3)];
            let mut known = vec![0, 1, 3, 4, 5];
            let ders = vec![2];
            let mut steps = vec![];
            let n_steps = 1 + g.below(5);
            for k in 0..n_steps {
                let target = 6 + k;
                let depth = 1 + g.below(4);
                steps.push((target, false, expr(&mut g, depth, &known, &ders)));
                known.push(target);
            }
            let sign = if g.below(2) == 0 { 1.0 } else { -1.0 };
            let chain = InputChain {
                result: (6 + n_steps - 1, sign),
                steps,
                from_y,
                from_d: vec![(4, 1)],
                from_u: vec![(5, 0)],
            };
            let params: Vec<f64> = (0..3).map(|_| g.value()).collect();
            let tape = ChainTape::new(&chain, &params).expect("a chain without tables lowers");
            lowered += 1;
            let mut regs = tape.registers();
            let (mut vals, mut ders_s) = (vec![0.0; n_vars], vec![0.0; n_vars]);
            let lanes = 1 + g.below(TAPE_LANES);
            let ts: Vec<f64> = (0..lanes).map(|_| g.value()).collect();
            let ys: Vec<f64> = (0..lanes * 4).map(|_| g.value()).collect();
            let d = [g.value(), g.value()];
            let u = [g.value()];
            let pos = [0, 1, 2, 3];
            let mut lane_regs = tape.lane_registers(TAPE_LANES);
            let mut out = vec![0.0; lanes];
            tape.eval_lanes(&ts, &ys, 4, &pos, &d, &u, TAPE_LANES, &mut lane_regs, &mut out);
            for i in 0..lanes {
                let yv = &ys[i * 4..][..4];
                let want = chain.eval(ts[i], yv, &d, &u, &params, &mut vals, &mut ders_s);
                let one = tape.eval(ts[i], yv, &pos, &d, &u, &mut regs);
                assert_eq!(one.to_bits(), want.to_bits(), "case {case} lane {i}: {one} vs {want}");
                assert_eq!(
                    out[i].to_bits(),
                    want.to_bits(),
                    "case {case} lane {i} of {lanes}: {} vs {want}",
                    out[i]
                );
            }
        }
        assert_eq!(lowered, 400);
    }

    /// What the interpreter does not define the same way (a function
    /// with too few arguments, a table) keeps the interpreter; so does a
    /// chain that reads a variable before a step writes it.
    #[test]
    fn what_a_tape_does_not_lower() {
        let chain = |e: Expr| InputChain {
            result: (1, 1.0),
            steps: vec![(1, false, e)],
            from_y: vec![(0, false, 0)],
            from_d: vec![],
            from_u: vec![],
        };
        let x = || Expr::Var(VarId(0));
        assert!(ChainTape::new(&chain(Expr::Call(Builtin::Min, vec![x()])), &[]).is_none());
        assert!(ChainTape::new(&chain(Expr::Call(Builtin::Sin, vec![])), &[]).is_none());
        let table = Expr::Table { table: 0, args: vec![x()] };
        assert!(ChainTape::new(&chain(table), &[]).is_none());
        assert!(ChainTape::new(&chain(Expr::Param(ParamId(3))), &[1.0]).is_none());
        // variable 2 read by the first step, written by the second
        let late = InputChain {
            result: (1, 1.0),
            steps: vec![(1, false, Expr::Var(VarId(2))), (2, false, x())],
            from_y: vec![(0, false, 0)],
            from_d: vec![],
            from_u: vec![],
        };
        assert!(ChainTape::new(&late, &[]).is_none());
        let sum = Expr::Binary(BinaryOp::Add, Box::new(x()), Box::new(Expr::Const(1.0)));
        assert!(ChainTape::new(&chain(sum), &[]).is_some());
    }
}
