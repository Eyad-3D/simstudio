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
                    r[d as usize] = fused(r[x as usize], r[y as usize], r[z as usize])
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
                    let t = tables.get(k as usize);
                    if g == NONE {
                        r[d as usize] = t.value([r[x as usize], 0.0]);
                    } else {
                        let (v, gr) = t.eval([r[x as usize], 0.0]);
                        r[d as usize] = v;
                        r[g as usize] = gr[0];
                    }
                }
                Op::Tab2(d, k, x, y, g) => {
                    let t = tables.get(k as usize);
                    if g == NONE {
                        r[d as usize] = t.value([r[x as usize], r[y as usize]]);
                    } else {
                        let (v, gr) = t.eval([r[x as usize], r[y as usize]]);
                        r[d as usize] = v;
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

/// `a × b + c` rounded once, by the CPU's own fused multiply-add: the
/// instruction the machine code runs (`vfmadd`), not a library's `fma`
/// (`f64::mul_add` calls one on x86-64, the C runtime's on Windows). A
/// tape has fused multiply-adds only where the CPU has them (the lowering
/// emits them only then).
#[inline]
fn fused(a: f64, b: f64, c: f64) -> f64 {
    #[cfg(target_arch = "x86_64")]
    {
        if std::arch::is_x86_feature_detected!("fma") {
            // SAFETY: the CPU has the instruction (checked just now).
            return unsafe { fused_x86(a, b, c) };
        }
    }
    // (elsewhere `mul_add` is the instruction itself: AArch64's `fmadd`)
    a.mul_add(b, c)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "fma")]
fn fused_x86(a: f64, b: f64, c: f64) -> f64 {
    use std::arch::x86_64::{_mm_cvtsd_f64, _mm_fmadd_sd, _mm_set_sd};
    _mm_cvtsd_f64(_mm_fmadd_sd(_mm_set_sd(a), _mm_set_sd(b), _mm_set_sd(c)))
}

#[cfg(test)]
mod tests {
    /// The fused multiply-add is rounded once: on products whose exact
    /// low part a separate rounding would lose, and against the exact
    /// error of a product (Dekker's), at random.
    #[test]
    fn the_fused_multiply_add_rounds_once() {
        let mut seed = 0x853c_49e6_748f_ea9bu64;
        let mut rnd = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        // (1 + 2^-52)² - (1 + 2^-51) = 2^-104, which a product rounded
        // first loses
        let a = 1.0 + f64::EPSILON;
        assert_eq!(super::fused(a, a, -(1.0 + 2.0 * f64::EPSILON)), 2f64.powi(-104));
        for _ in 0..100_000 {
            let (x, y) = (rnd() * 4.0 - 2.0, rnd() * 4.0 - 2.0);
            let p = x * y;
            // Dekker's exact error of the product
            let split = |v: f64| {
                let c = 134_217_729.0 * v;
                let h = c - (c - v);
                (h, v - h)
            };
            let ((xh, xl), (yh, yl)) = (split(x), split(y));
            let e = ((xh * yh - p) + xh * yl + xl * yh) + xl * yl;
            assert_eq!(super::fused(x, y, -p).to_bits(), e.to_bits(), "{x} × {y}");
        }
    }
}
