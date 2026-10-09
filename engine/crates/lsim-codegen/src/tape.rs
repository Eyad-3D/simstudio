//! The coloured Jacobian of a large model, evaluated from a tape instead
//! of compiled code: the same exact forward-mode derivative rules and the
//! same structural sparsity (one direction per colour, only possible
//! entries), run by a tight loop over a flat list of operations. Building
//! the tape takes milliseconds where compiling the Jacobian's code would
//! take as long as the residual's; a large stiff model evaluates its
//! Jacobian only every few dozen steps, so the residual (compiled) stays
//! what the run time depends on.

use crate::CodegenError;
use crate::analysis::{Ctx, Row, Src, System};
use crate::tables::TableStore;
use lsim_ir::expr::{BinaryOp, Builtin, CmpOp, Expr};
use lsim_ir::runtime::EvalInput;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq)]
enum K {
    Const,
    Time,
    Y,
    P,
    D,
    U,
    Neg,
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    PowC,
    Sqrt,
    Abs,
    Sign,
    Exp,
    Log,
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Sinh,
    Cosh,
    Tanh,
    Atan2,
    Min,
    Max,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    Not,
    If,
    Tab1,
    Tab2,
}

/// One operation: `vals[dst] = k(vals[a], vals[b], vals[c])` (dst is its
/// index), and its tangent entries `tans[t0..t0 + nt]`.
#[derive(Clone, Copy, Debug)]
struct Op {
    k: K,
    a: u32,
    b: u32,
    c: u32,
    /// a constant, an index (y, p, d, u, table) or an exponent
    x: f64,
    t0: u32,
    nt: u32,
}

/// No source entry.
const NONE: u32 = u32::MAX;

/// A compiled-free coloured Jacobian.
pub(crate) struct Tape {
    ops: Vec<Op>,
    /// per tangent entry of each op: the source entry in each operand
    /// (a, b, c), or NONE
    merge: Vec<[u32; 3]>,
    /// per op: its tangent's directions (for building only)
    n_tans: usize,
    /// (values position, tangent entry) for every Jacobian value written
    out: Vec<(u32, u32)>,
    /// Jacobian positions that are structurally zero here
    zeros: Vec<u32>,
    /// the y entries' seeds: tangent entries set to one
    seeds: Vec<u32>,
}

struct Builder<'a> {
    cx: &'a Ctx<'a>,
    sys: &'a System<'a>,
    colour: &'a [u32],
    ops: Vec<Op>,
    dirs: Vec<Vec<u32>>,
    merge: Vec<[u32; 3]>,
    n_tans: usize,
    seeds: Vec<u32>,
    assign_slot: HashMap<usize, u32>,
    leaf: HashMap<(u8, u64), u32>,
}

impl Builder<'_> {
    fn push(&mut self, k: K, args: [u32; 3], x: f64, tangent: bool) -> u32 {
        let dst = self.ops.len() as u32;
        let n_args = match k {
            K::Const | K::Time | K::Y | K::P | K::D | K::U => 0,
            K::Neg
            | K::Sqrt
            | K::Abs
            | K::Sign
            | K::Exp
            | K::Log
            | K::Sin
            | K::Cos
            | K::Tan
            | K::Asin
            | K::Acos
            | K::Atan
            | K::Sinh
            | K::Cosh
            | K::Tanh
            | K::Not
            | K::PowC
            | K::Tab1 => 1,
            K::If => 3,
            _ => 2,
        };
        // structural directions: the union of the operands' (none for
        // truth values and sign, which have zero derivatives)
        let mut d: Vec<u32> = vec![];
        if tangent {
            // an If's condition does not contribute
            let first = if k == K::If { 1 } else { 0 };
            for &s in &args[first..n_args] {
                d.extend_from_slice(&self.dirs[s as usize]);
            }
            d.sort_unstable();
            d.dedup();
        }
        if k == K::Y {
            d = vec![self.colour[x as usize]];
        }
        let t0 = self.n_tans as u32;
        for dir in &d {
            let mut m = [NONE; 3];
            if k != K::Y {
                for (o, &s) in args.iter().enumerate().take(n_args) {
                    if let Ok(i) = self.dirs[s as usize].binary_search(dir) {
                        m[o] = self.ops[s as usize].t0 + i as u32;
                    }
                }
            }
            self.merge.push(m);
        }
        self.n_tans += d.len();
        if k == K::Y {
            self.seeds.push(t0);
        }
        self.ops.push(Op { k, a: args[0], b: args[1], c: args[2], x, t0, nt: d.len() as u32 });
        self.dirs.push(d);
        dst
    }

    fn leaf(&mut self, tag: u8, k: K, x: f64) -> u32 {
        if let Some(&s) = self.leaf.get(&(tag, x.to_bits())) {
            return s;
        }
        let s = self.push(k, [0; 3], x, false);
        self.leaf.insert((tag, x.to_bits()), s);
        s
    }

    fn src(&mut self, s: Src) -> u32 {
        match s {
            Src::Y(i) => self.leaf(1, K::Y, i as f64),
            Src::Work(k) => self.assign_slot[&k],
            Src::D(k) => self.leaf(2, K::D, k as f64),
            Src::U(k) => self.leaf(3, K::U, k as f64),
            Src::Const(c) => self.leaf(0, K::Const, c),
        }
    }

    fn expr(&mut self, e: &Expr) -> Result<u32, CodegenError> {
        let t = true;
        Ok(match e {
            Expr::Const(c) => self.leaf(0, K::Const, *c),
            Expr::Time => self.leaf(4, K::Time, 0.0),
            Expr::Param(p) => self.leaf(5, K::P, p.0 as f64),
            Expr::Var(v) | Expr::Pre(v) => {
                let s = self.sys.resolve(self.cx, *v, false)?;
                self.src(s)
            }
            Expr::Der(v) => {
                let s = self.sys.resolve(self.cx, *v, true)?;
                self.src(s)
            }
            Expr::Name(n) => {
                return Err(CodegenError::Unsupported(format!("unresolved name '{n}'")));
            }
            Expr::Neg(a) => {
                let a = self.expr(a)?;
                self.push(K::Neg, [a, 0, 0], 0.0, t)
            }
            Expr::NoEvent(a) => self.expr(a)?,
            Expr::Binary(op, a, b) => {
                if let (BinaryOp::Pow, Expr::Const(n)) = (op, &**b) {
                    let a = self.expr(a)?;
                    if *n == 0.0 {
                        return Ok(self.leaf(0, K::Const, 1.0));
                    }
                    if *n == 1.0 {
                        return Ok(a);
                    }
                    return Ok(self.push(K::PowC, [a, 0, 0], *n, t));
                }
                let a = self.expr(a)?;
                let b = self.expr(b)?;
                let k = match op {
                    BinaryOp::Add => K::Add,
                    BinaryOp::Sub => K::Sub,
                    BinaryOp::Mul => K::Mul,
                    BinaryOp::Div => K::Div,
                    BinaryOp::Pow => K::Pow,
                };
                self.push(k, [a, b, 0], 0.0, t)
            }
            Expr::Call(f, args) => {
                let mut s = vec![];
                for a in args {
                    s.push(self.expr(a)?);
                }
                let k = match f {
                    Builtin::Der | Builtin::Pre => {
                        return Err(CodegenError::Unsupported("der/pre in component scope".into()));
                    }
                    Builtin::Sqrt => K::Sqrt,
                    Builtin::Abs => K::Abs,
                    Builtin::Sign => K::Sign,
                    Builtin::Exp => K::Exp,
                    Builtin::Log => K::Log,
                    Builtin::Sin => K::Sin,
                    Builtin::Cos => K::Cos,
                    Builtin::Tan => K::Tan,
                    Builtin::Asin => K::Asin,
                    Builtin::Acos => K::Acos,
                    Builtin::Atan => K::Atan,
                    Builtin::Sinh => K::Sinh,
                    Builtin::Cosh => K::Cosh,
                    Builtin::Tanh => K::Tanh,
                    Builtin::Atan2 => K::Atan2,
                    Builtin::Min => K::Min,
                    Builtin::Max => K::Max,
                    Builtin::Limit => {
                        let lo = self.push(K::Max, [s[0], s[1], 0], 0.0, t);
                        return Ok(self.push(K::Min, [lo, s[2], 0], 0.0, t));
                    }
                };
                let tangent = k != K::Sign;
                self.push(k, [s[0], s.get(1).copied().unwrap_or(0), 0], 0.0, tangent)
            }
            Expr::Compare(op, a, b) => {
                let a = self.expr(a)?;
                let b = self.expr(b)?;
                let k = match op {
                    CmpOp::Lt => K::Lt,
                    CmpOp::Le => K::Le,
                    CmpOp::Gt => K::Gt,
                    CmpOp::Ge => K::Ge,
                };
                self.push(k, [a, b, 0], 0.0, false)
            }
            Expr::And(a, b) | Expr::Or(a, b) => {
                let a = self.expr(a)?;
                let b = self.expr(b)?;
                let k = if matches!(e, Expr::And(..)) { K::And } else { K::Or };
                self.push(k, [a, b, 0], 0.0, false)
            }
            Expr::Not(a) => {
                let a = self.expr(a)?;
                self.push(K::Not, [a, 0, 0], 0.0, false)
            }
            Expr::If(c, a, b) => {
                let c = self.expr(c)?;
                let a = self.expr(a)?;
                let b = self.expr(b)?;
                self.push(K::If, [c, a, b], 0.0, t)
            }
            Expr::Table { table, args } => {
                let mut s = vec![];
                for a in args {
                    s.push(self.expr(a)?);
                }
                if s.len() == 1 {
                    self.push(K::Tab1, [s[0], 0, 0], *table as f64, t)
                } else {
                    self.push(K::Tab2, [s[0], s[1], 0], *table as f64, t)
                }
            }
        })
    }
}

impl Tape {
    /// Records the assignments `list` and the system's rows.
    pub(crate) fn build(
        cx: &Ctx<'_>,
        sys: &System<'_>,
        list: &[usize],
        colour: &[u32],
        pos: &HashMap<(usize, u32), usize>,
        nnz: usize,
    ) -> Result<Tape, CodegenError> {
        let mut b = Builder {
            cx,
            sys,
            colour,
            ops: vec![],
            dirs: vec![],
            merge: vec![],
            n_tans: 0,
            seeds: vec![],
            assign_slot: HashMap::new(),
            leaf: HashMap::new(),
        };
        for &k in list {
            let s = b.expr(sys.exprs[k])?;
            b.assign_slot.insert(k, s);
        }
        let mut out = vec![];
        let mut written = vec![false; nnz];
        for (i, r) in sys.rows.iter().enumerate() {
            let s = match r {
                Row::Slot(s) => {
                    let src = sys.row_src(*s)?;
                    b.src(src)
                }
                Row::Expr(e) => b.expr(e)?,
            };
            let op = b.ops[s as usize];
            for (e, dir) in b.dirs[s as usize].iter().enumerate() {
                let Some(&p) = pos.get(&(i, *dir)) else {
                    return Err(CodegenError::Backend(format!(
                        "internal: a derivative of row {i} lies outside the Jacobian's pattern"
                    )));
                };
                out.push((p as u32, op.t0 + e as u32));
                written[p] = true;
            }
        }
        let zeros = (0..nnz).filter(|&p| !written[p]).map(|p| p as u32).collect();
        Ok(Tape { ops: b.ops, merge: b.merge, n_tans: b.n_tans, out, zeros, seeds: b.seeds })
    }

    /// The scratch values it needs.
    pub(crate) fn scratch(&self) -> usize {
        self.ops.len() + self.n_tans
    }

    /// Evaluates the Jacobian's values (column-compressed) using
    /// `scratch` (at least [`Tape::scratch`] values).
    pub(crate) fn eval(
        &self,
        inp: &EvalInput<'_>,
        tables: &TableStore,
        scratch: &mut [f64],
        values: &mut [f64],
    ) {
        let (vals, tans) = scratch.split_at_mut(self.ops.len());
        let tans = &mut tans[..self.n_tans];
        for &s in &self.seeds {
            tans[s as usize] = 1.0;
        }
        let truth = |b: bool| if b { 1.0 } else { 0.0 };
        for (i, op) in self.ops.iter().enumerate() {
            let a = vals.get(op.a as usize).copied().unwrap_or(0.0);
            let b = vals.get(op.b as usize).copied().unwrap_or(0.0);
            // the value, and the partial derivatives by a, b (c for If)
            let (v, pa, pb) = match op.k {
                K::Const => (op.x, 0.0, 0.0),
                K::Time => (inp.t, 0.0, 0.0),
                K::Y => (inp.y[op.x as usize], 0.0, 0.0),
                K::P => (inp.p[op.x as usize], 0.0, 0.0),
                K::D => (inp.d[op.x as usize], 0.0, 0.0),
                K::U => (inp.u[op.x as usize], 0.0, 0.0),
                K::Neg => (-a, -1.0, 0.0),
                K::Add => (a + b, 1.0, 1.0),
                K::Sub => (a - b, 1.0, -1.0),
                K::Mul => (a * b, b, a),
                K::Div => {
                    let q = a / b;
                    (q, 1.0 / b, -q / b)
                }
                K::Pow => {
                    let v = a.powf(b);
                    let pa = if op.nt > 0 { b * a.powf(b - 1.0) } else { 0.0 };
                    let pb = if op.nt > 0 { v * a.ln() } else { 0.0 };
                    (v, pa, pb)
                }
                K::PowC => {
                    let n = op.x;
                    let v = if n == 2.0 { a * a } else { a.powf(n) };
                    let d = if n == 2.0 { 2.0 * a } else { n * a.powf(n - 1.0) };
                    (v, d, 0.0)
                }
                K::Sqrt => {
                    let v = a.sqrt();
                    (v, 0.5 / v, 0.0)
                }
                K::Abs => (a.abs(), sign(a), 0.0),
                K::Sign => (sign(a), 0.0, 0.0),
                K::Exp => {
                    let v = a.exp();
                    (v, v, 0.0)
                }
                K::Log => (a.ln(), 1.0 / a, 0.0),
                K::Sin => (a.sin(), a.cos(), 0.0),
                K::Cos => (a.cos(), -a.sin(), 0.0),
                K::Tan => {
                    let v = a.tan();
                    (v, 1.0 + v * v, 0.0)
                }
                K::Asin => (a.asin(), 1.0 / (1.0 - a * a).sqrt(), 0.0),
                K::Acos => (a.acos(), -1.0 / (1.0 - a * a).sqrt(), 0.0),
                K::Atan => (a.atan(), 1.0 / (1.0 + a * a), 0.0),
                K::Sinh => (a.sinh(), a.cosh(), 0.0),
                K::Cosh => (a.cosh(), a.sinh(), 0.0),
                K::Tanh => {
                    let v = a.tanh();
                    (v, 1.0 - v * v, 0.0)
                }
                K::Atan2 => {
                    let den = b * b + a * a;
                    (a.atan2(b), b / den, -a / den)
                }
                K::Min => {
                    let c = a < b || b.is_nan();
                    (if c { a } else { b }, truth(c), truth(!c))
                }
                K::Max => {
                    let c = a > b || b.is_nan();
                    (if c { a } else { b }, truth(c), truth(!c))
                }
                K::Lt => (truth(a < b), 0.0, 0.0),
                K::Le => (truth(a <= b), 0.0, 0.0),
                K::Gt => (truth(a > b), 0.0, 0.0),
                K::Ge => (truth(a >= b), 0.0, 0.0),
                K::And => (truth(a != 0.0 && b != 0.0), 0.0, 0.0),
                K::Or => (truth(a != 0.0 || b != 0.0), 0.0, 0.0),
                K::Not => (truth(a == 0.0), 0.0, 0.0),
                K::If => {
                    let c = a != 0.0;
                    let x = vals[op.c as usize];
                    // operands: (cond, then = b, else = c)
                    (if c { b } else { x }, truth(c), truth(!c))
                }
                K::Tab1 => {
                    let (v, g) = tables.get(op.x as usize).eval([a, 0.0]);
                    (v, g[0], 0.0)
                }
                K::Tab2 => {
                    let (v, g) = tables.get(op.x as usize).eval([a, b]);
                    (v, g[0], g[1])
                }
            };
            vals[i] = v;
            if op.nt == 0 || op.k == K::Y {
                continue;
            }
            let m = &self.merge[op.t0 as usize..(op.t0 + op.nt) as usize];
            let base = op.t0 as usize;
            match op.k {
                // a selection: the chosen operand's entry (no products,
                // so an infinite entry on the other side does not leak)
                K::Min | K::Max | K::If => {
                    let (first, second) = if op.k == K::If { (1, 2) } else { (0, 1) };
                    let pick_first = pa != 0.0;
                    for (e, src) in m.iter().enumerate() {
                        let s = if pick_first { src[first] } else { src[second] };
                        tans[base + e] = if s == NONE { 0.0 } else { tans[s as usize] };
                    }
                }
                _ => {
                    for (e, src) in m.iter().enumerate() {
                        let mut t = 0.0;
                        if src[0] != NONE {
                            t += pa * tans[src[0] as usize];
                        }
                        if src[1] != NONE {
                            t += pb * tans[src[1] as usize];
                        }
                        tans[base + e] = t;
                    }
                }
            }
        }
        for &(p, e) in &self.out {
            values[p as usize] = tans[e as usize];
        }
        for &p in &self.zeros {
            values[p as usize] = 0.0;
        }
    }
}

fn sign(a: f64) -> f64 {
    if a > 0.0 {
        1.0
    } else if a < 0.0 {
        -1.0
    } else {
        0.0
    }
}
