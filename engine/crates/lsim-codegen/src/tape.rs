//! The tape: a function's operations recorded in order and interpreted
//! by a tight loop instead of compiled to machine code. The lowering
//! emits exactly the operations it emits into Cranelift IR ([`Emit`]), so
//! a tape computes bitwise what the compiled function computes; it costs
//! no compilation, and a few nanoseconds per operation when it runs. The
//! functions a run calls a handful of times (the initialisation's, the
//! Jacobian-vector product) are taped rather than compiled.

use crate::backend::{Base, Cc, Emit, Lib};
use crate::tables::TableStore;

/// No register.
const NONE: u32 = u32::MAX;

/// One operation; registers are indices into the register file.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Op {
    Const(u32, f64),
    Time(u32),
    /// dst, array, index
    Load(u32, Base, u32),
    /// array, index, src
    Store(Base, u32, u32),
    Add(u32, u32, u32),
    Sub(u32, u32, u32),
    Mul(u32, u32, u32),
    Div(u32, u32, u32),
    Neg(u32, u32),
    Abs(u32, u32),
    Sqrt(u32, u32),
    Fma(u32, u32, u32, u32),
    Cmp(u32, Cc, u32, u32),
    And(u32, u32, u32),
    Or(u32, u32, u32),
    Select(u32, u32, u32, u32),
    BitsAnd(u32, u32, u32),
    BitsOr(u32, u32, u32),
    Lib1(u32, Lib, u32),
    Lib2(u32, Lib, u32, u32),
    /// value, table, x, the derivative's register (or NONE)
    Tab1(u32, u32, u32, u32),
    /// value, table, x, y, the derivatives' first register (or NONE)
    Tab2(u32, u32, u32, u32, u32),
    /// dst, table, axis, x
    Guard(u32, u32, u8, u32),
}

/// A recorded function.
#[derive(Clone, Debug, Default)]
pub(crate) struct Tape {
    pub ops: Vec<Op>,
    /// registers it uses
    pub regs: usize,
}

/// Records the operations emitted.
#[derive(Default)]
pub(crate) struct Recorder {
    ops: Vec<Op>,
    next: u32,
}

impl Recorder {
    fn reg(&mut self) -> u32 {
        let r = self.next;
        self.next += 1;
        r
    }

    fn push(&mut self, f: impl FnOnce(u32) -> Op) -> u32 {
        let r = self.reg();
        self.ops.push(f(r));
        r
    }

    pub(crate) fn finish(self) -> Tape {
        Tape { ops: self.ops, regs: self.next as usize }
    }
}

impl Emit for Recorder {
    type V = u32;

    fn new_segment(&mut self) {}

    fn konst(&mut self, x: f64) -> u32 {
        self.push(|r| Op::Const(r, x))
    }

    fn time(&mut self) -> u32 {
        self.push(Op::Time)
    }

    fn load(&mut self, arr: Base, i: usize) -> u32 {
        self.push(|r| Op::Load(r, arr, i as u32))
    }

    fn store(&mut self, arr: Base, i: usize, v: u32) {
        self.ops.push(Op::Store(arr, i as u32, v));
    }

    fn add(&mut self, a: u32, b: u32) -> u32 {
        self.push(|r| Op::Add(r, a, b))
    }

    fn sub(&mut self, a: u32, b: u32) -> u32 {
        self.push(|r| Op::Sub(r, a, b))
    }

    fn mul(&mut self, a: u32, b: u32) -> u32 {
        self.push(|r| Op::Mul(r, a, b))
    }

    fn div(&mut self, a: u32, b: u32) -> u32 {
        self.push(|r| Op::Div(r, a, b))
    }

    fn neg(&mut self, a: u32) -> u32 {
        self.push(|r| Op::Neg(r, a))
    }

    fn abs(&mut self, a: u32) -> u32 {
        self.push(|r| Op::Abs(r, a))
    }

    fn sqrt(&mut self, a: u32) -> u32 {
        self.push(|r| Op::Sqrt(r, a))
    }

    fn fma(&mut self, a: u32, b: u32, c: u32) -> u32 {
        self.push(|r| Op::Fma(r, a, b, c))
    }

    fn cmp(&mut self, cc: Cc, a: u32, b: u32) -> u32 {
        self.push(|r| Op::Cmp(r, cc, a, b))
    }

    fn and(&mut self, a: u32, b: u32) -> u32 {
        self.push(|r| Op::And(r, a, b))
    }

    fn or(&mut self, a: u32, b: u32) -> u32 {
        self.push(|r| Op::Or(r, a, b))
    }

    fn select(&mut self, c: u32, a: u32, b: u32) -> u32 {
        self.push(|r| Op::Select(r, c, a, b))
    }

    fn bits_and(&mut self, a: u32, b: u32) -> u32 {
        self.push(|r| Op::BitsAnd(r, a, b))
    }

    fn bits_or(&mut self, a: u32, b: u32) -> u32 {
        self.push(|r| Op::BitsOr(r, a, b))
    }

    fn call(&mut self, f: Lib, args: &[u32]) -> u32 {
        match f.arity() {
            1 => self.push(|r| Op::Lib1(r, f, args[0])),
            _ => self.push(|r| Op::Lib2(r, f, args[0], args[1])),
        }
    }

    fn table(&mut self, k: u32, args: &[u32], derivs: bool) -> (u32, [Option<u32>; 2]) {
        let v = self.reg();
        let n = args.len();
        let d0 = if derivs {
            let d0 = self.reg();
            if n == 2 {
                self.reg();
            }
            d0
        } else {
            NONE
        };
        if n == 1 {
            self.ops.push(Op::Tab1(v, k, args[0], d0));
        } else {
            self.ops.push(Op::Tab2(v, k, args[0], args[1], d0));
        }
        let g = if derivs { [Some(d0), (n == 2).then_some(d0 + 1)] } else { [None, None] };
        (v, g)
    }

    fn table_guard(&mut self, k: u32, axis: u8, x: u32) -> u32 {
        self.push(|r| Op::Guard(r, k, axis, x))
    }
}

/// What a tape reads and writes.
pub(crate) struct Arrays<'a> {
    pub t: f64,
    pub y: &'a [f64],
    pub p: &'a [f64],
    pub d: &'a [f64],
    pub u: &'a [f64],
    pub v: &'a [f64],
    pub work: &'a mut [f64],
    pub out: &'a mut [f64],
}

fn truth(b: bool) -> f64 {
    if b { 1.0 } else { 0.0 }
}

impl Tape {
    /// Runs the tape (`regs`: at least [`Tape::regs`] values).
    pub(crate) fn run(&self, a: Arrays<'_>, tables: &TableStore, regs: &mut [f64]) {
        let r = &mut regs[..self.regs];
        for op in &self.ops {
            match *op {
                Op::Const(d, x) => r[d as usize] = x,
                Op::Time(d) => r[d as usize] = a.t,
                Op::Load(d, arr, i) => {
                    let i = i as usize;
                    r[d as usize] = match arr {
                        Base::Y => a.y[i],
                        Base::P => a.p[i],
                        Base::D => a.d[i],
                        Base::U => a.u[i],
                        Base::V => a.v[i],
                        Base::Work => a.work[i],
                        Base::Out => a.out[i],
                    }
                }
                Op::Store(arr, i, s) => {
                    let x = r[s as usize];
                    match arr {
                        Base::Work => a.work[i as usize] = x,
                        Base::Out => a.out[i as usize] = x,
                        _ => unreachable!("only work and out are written"),
                    }
                }
                Op::Add(d, x, y) => r[d as usize] = r[x as usize] + r[y as usize],
                Op::Sub(d, x, y) => r[d as usize] = r[x as usize] - r[y as usize],
                Op::Mul(d, x, y) => r[d as usize] = r[x as usize] * r[y as usize],
                Op::Div(d, x, y) => r[d as usize] = r[x as usize] / r[y as usize],
                Op::Neg(d, x) => r[d as usize] = -r[x as usize],
                Op::Abs(d, x) => r[d as usize] = r[x as usize].abs(),
                Op::Sqrt(d, x) => r[d as usize] = r[x as usize].sqrt(),
                Op::Fma(d, x, y, z) => {
                    r[d as usize] = r[x as usize].mul_add(r[y as usize], r[z as usize])
                }
                Op::Cmp(d, cc, x, y) => {
                    r[d as usize] = truth(cc.eval(r[x as usize], r[y as usize]))
                }
                Op::And(d, x, y) => {
                    r[d as usize] = truth(r[x as usize] != 0.0 && r[y as usize] != 0.0)
                }
                Op::Or(d, x, y) => {
                    r[d as usize] = truth(r[x as usize] != 0.0 || r[y as usize] != 0.0)
                }
                Op::Select(d, c, x, y) => {
                    r[d as usize] = if r[c as usize] != 0.0 { r[x as usize] } else { r[y as usize] }
                }
                Op::BitsAnd(d, x, y) => {
                    r[d as usize] =
                        f64::from_bits(r[x as usize].to_bits() & r[y as usize].to_bits())
                }
                Op::BitsOr(d, x, y) => {
                    r[d as usize] =
                        f64::from_bits(r[x as usize].to_bits() | r[y as usize].to_bits())
                }
                Op::Lib1(d, f, x) => r[d as usize] = f.eval(r[x as usize], 0.0),
                Op::Lib2(d, f, x, y) => r[d as usize] = f.eval(r[x as usize], r[y as usize]),
                Op::Tab1(d, k, x, g) => {
                    let (v, gr) = tables.get(k as usize).eval([r[x as usize], 0.0]);
                    r[d as usize] = v;
                    if g != NONE {
                        r[g as usize] = gr[0];
                    }
                }
                Op::Tab2(d, k, x, y, g) => {
                    let (v, gr) = tables.get(k as usize).eval([r[x as usize], r[y as usize]]);
                    r[d as usize] = v;
                    if g != NONE {
                        r[g as usize] = gr[0];
                        r[g as usize + 1] = gr[1];
                    }
                }
                Op::Guard(d, k, axis, x) => {
                    r[d as usize] = tables.get(k as usize).guard(axis as usize, r[x as usize])
                }
            }
        }
    }
}
