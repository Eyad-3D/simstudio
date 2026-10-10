//! Compiled kernels of the conditions the run loop checks along its steps
//! (DESIGN.md §5.8, *compiled condition kernels*): those that read time
//! and the integrator's variables.
//!
//! * The **point kernel** of a condition is the lowering of its chain of
//!   assignments and of the condition with the interpreter's arithmetic
//!   ([`crate::lower::Exact::Interpreter`]), recorded on a tape: bitwise
//!   `lsim_ir::eval`, and bitwise the compiled `roots` output.
//! * The **interval kernel** is a flat program over the interval
//!   interpreter's own rules (`lsim_ir::interval`: `binary_j2`,
//!   `call_j2`, `table_j2`, `if_j2`, `cut_j2` …), in the IR's order, so its
//!   enclosures are bitwise `lsim_ir::interval::enclose`'s. The tree walk,
//!   the closures that look leaves up and the allocations are gone: each
//!   chain step is a register, each `if`'s cut branches are compiled for
//!   the variables its condition compares. The steps that do not move
//!   along a step (they read neither time nor the integrator's variables)
//!   are computed once for every condition that reads them and kept in
//!   the caller's scratch while the discrete values, inputs and
//!   parameters they read keep their bits.
//!
//! A condition is declined (interpreted) when it reads a derivative or an
//! unresolved name, or when its program would grow past a bound (`if`s
//! whose branches are cut nest).

use crate::analysis::{Ctx, Src, System};
use crate::tables::TableStore;
use lsim_ir::expr::{BinaryOp, Builtin, CmpOp, Expr};
use lsim_ir::interval::{self as iv, Cx, Grid2, Iv, J2};
use lsim_ir::runtime::{Enclosure, ModelFunctions};
use lsim_ir::{PreparedModel, VarId};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

/// The most operations one condition's program may have.
const MAX_OPS: usize = 50_000;

/// One operation of an interval program; registers index the scratch.
#[derive(Clone, Debug)]
enum JOp {
    Konst(u32, f64),
    Param(u32, u32),
    D(u32, u32),
    U(u32, u32),
    /// dst, entry of y
    Leaf(u32, u32),
    Time(u32),
    All(u32),
    Copy(u32, u32),
    Neg(u32, u32),
    Bin(u32, BinaryOp, u32, u32),
    Cmp(u32, CmpOp, u32, u32),
    And(u32, u32, u32),
    Or(u32, u32, u32),
    Not(u32, u32),
    /// dst, function, arguments, how many
    Call(u32, Builtin, [u32; 3], u8),
    /// dst, table, arguments, how many
    Table(u32, u32, [u32; 2], u8),
    If(u32, Box<IfOp>),
}

/// A program and the register of its result.
#[derive(Clone, Debug, Default)]
struct Prog {
    ops: Vec<JOp>,
    out: u32,
}

/// The bound a comparison puts on a variable, from the other side's
/// register.
#[derive(Clone, Copy, Debug)]
enum Bound {
    /// at most the other side's largest value
    Below(u32),
    /// at least its least value
    Above(u32),
}

/// A branch with the variables its `if`'s condition compares cut to the
/// bounds the condition puts on them where the branch is taken.
#[derive(Clone, Debug)]
struct Cut {
    /// (the cut variable's register, its register in the context)
    init: Vec<(u32, u32)>,
    /// (the cut variable's register, the bound), in the interpreter's order
    bounds: Vec<(u32, Bound)>,
    body: Prog,
}

#[derive(Clone, Debug)]
struct IfOp {
    c: u32,
    comparison: bool,
    a: Prog,
    b: Prog,
    /// `None`: the condition bounds no variable that moves (the branch
    /// is the plain one)
    a_cut: Option<Cut>,
    b_cut: Option<Cut>,
}

/// What a key entry reads.
#[derive(Clone, Copy, Debug, PartialEq)]
enum KeySrc {
    D(u32),
    U(u32),
    P(u32),
}

/// A compiled condition.
#[derive(Clone, Debug)]
struct Cond {
    /// loads its leaves, computes its moving steps, then the condition
    prog: Prog,
}

/// The interval kernels of a model's conditions.
pub(crate) struct Kernels {
    /// per zero crossing
    conds: Vec<Option<Cond>>,
    /// the steps that do not move, of every condition, in order
    fixed: Prog,
    /// what they read (the cache's key)
    key: Vec<KeySrc>,
    /// registers
    n_regs: usize,
    /// this compilation (the cache in a caller's scratch is ours only)
    id: u64,
}

/// The scratch's first entries: the cache's mark and key.
const HEADER: usize = 1;

fn j2(e: &Enclosure) -> J2 {
    J2 {
        v: Iv { lo: e.v[0], hi: e.v[1] },
        d: Iv { lo: e.d[0], hi: e.d[1] },
        dd: Iv { lo: e.dd[0], hi: e.dd[1] },
    }
}

fn enc(j: J2) -> Enclosure {
    Enclosure { v: [j.v.lo, j.v.hi], d: [j.d.lo, j.d.hi], dd: [j.dd.lo, j.dd.hi] }
}

/// The program builder of one model.
struct Builder<'a> {
    cx: &'a Ctx<'a>,
    sys: &'a System<'a>,
    next: u32,
    /// per assignment: its register (allocated on first use)
    step_reg: HashMap<usize, u32>,
    /// per entry of y: its leaf register
    leaf_reg: HashMap<u32, u32>,
    ops: usize,
}

/// Where a variable is read in a program.
#[derive(Clone, Copy)]
enum At {
    Reg(u32),
    D(u32),
    U(u32),
    Konst(f64),
}

impl Builder<'_> {
    fn reg(&mut self) -> u32 {
        let r = self.next;
        self.next += 1;
        r
    }

    fn step(&mut self, k: usize) -> u32 {
        if let Some(&r) = self.step_reg.get(&k) {
            return r;
        }
        let r = self.reg();
        self.step_reg.insert(k, r);
        r
    }

    fn leaf(&mut self, i: u32) -> u32 {
        if let Some(&r) = self.leaf_reg.get(&i) {
            return r;
        }
        let r = self.reg();
        self.leaf_reg.insert(i, r);
        r
    }

    /// Where `v` is read (outside any cut).
    fn var(&mut self, v: VarId) -> Result<At, String> {
        let s = self.sys.resolve(self.cx, v, false).map_err(|e| e.to_string())?;
        Ok(match s {
            Src::Y(i) => At::Reg(self.leaf(i as u32)),
            Src::Work(k) => At::Reg(self.step(k)),
            Src::D(k) => At::D(k as u32),
            Src::U(k) => At::U(k as u32),
            Src::Const(c) => At::Konst(c),
        })
    }

    /// Whether a variable read at `at` is one the interpreter's leaf
    /// lookup gives (a state or iteration variable, or a chain step).
    fn moves(at: At) -> bool {
        matches!(at, At::Reg(_))
    }

    fn emit(&mut self, out: &mut Vec<JOp>, op: JOp) -> Result<(), String> {
        self.ops += 1;
        if self.ops > MAX_OPS {
            return Err("its interval program grows too large".into());
        }
        out.push(op);
        Ok(())
    }

    /// Compiles `e` into `out` in the context `cuts` (variables cut to
    /// other registers); returns its register. `cmp` records the
    /// registers of the comparisons' sides (for the bounds of `if`s).
    fn expr(
        &mut self,
        e: &Expr,
        cuts: &HashMap<u32, u32>,
        out: &mut Vec<JOp>,
        cmp: &mut HashMap<*const Expr, (u32, u32)>,
    ) -> Result<u32, String> {
        let r = match e {
            Expr::Const(c) => {
                let r = self.reg();
                self.emit(out, JOp::Konst(r, *c))?;
                r
            }
            Expr::Param(p) => {
                let r = self.reg();
                self.emit(out, JOp::Param(r, p.0))?;
                r
            }
            Expr::Var(v) | Expr::Pre(v) => {
                if let Some(&c) = cuts.get(&v.0) {
                    return Ok(c);
                }
                match self.var(*v)? {
                    At::Reg(r) => r,
                    At::D(k) => {
                        let r = self.reg();
                        self.emit(out, JOp::D(r, k))?;
                        r
                    }
                    At::U(k) => {
                        let r = self.reg();
                        self.emit(out, JOp::U(r, k))?;
                        r
                    }
                    At::Konst(c) => {
                        let r = self.reg();
                        self.emit(out, JOp::Konst(r, c))?;
                        r
                    }
                }
            }
            Expr::Time => {
                let r = self.reg();
                self.emit(out, JOp::Time(r))?;
                r
            }
            Expr::Der(_) | Expr::Name(_) => {
                return Err("it reads a derivative or an unresolved name".into());
            }
            Expr::Neg(a) => {
                let a = self.expr(a, cuts, out, cmp)?;
                let r = self.reg();
                self.emit(out, JOp::Neg(r, a))?;
                r
            }
            Expr::NoEvent(a) => self.expr(a, cuts, out, cmp)?,
            Expr::Binary(op, a, b) => {
                let a = self.expr(a, cuts, out, cmp)?;
                let b = self.expr(b, cuts, out, cmp)?;
                let r = self.reg();
                self.emit(out, JOp::Bin(r, *op, a, b))?;
                r
            }
            Expr::Compare(op, a, b) => {
                let ra = self.expr(a, cuts, out, cmp)?;
                let rb = self.expr(b, cuts, out, cmp)?;
                cmp.insert(e as *const Expr, (ra, rb));
                let r = self.reg();
                self.emit(out, JOp::Cmp(r, *op, ra, rb))?;
                r
            }
            Expr::And(a, b) | Expr::Or(a, b) => {
                let ra = self.expr(a, cuts, out, cmp)?;
                let rb = self.expr(b, cuts, out, cmp)?;
                let r = self.reg();
                let op = if matches!(e, Expr::And(..)) {
                    JOp::And(r, ra, rb)
                } else {
                    JOp::Or(r, ra, rb)
                };
                self.emit(out, op)?;
                r
            }
            Expr::Not(a) => {
                let a = self.expr(a, cuts, out, cmp)?;
                let r = self.reg();
                self.emit(out, JOp::Not(r, a))?;
                r
            }
            Expr::Call(f, args) => {
                if matches!(f, Builtin::Der | Builtin::Pre) {
                    return Err("it reads der or pre in component scope".into());
                }
                // the arguments the interpreter's rule reads
                let n = match f {
                    Builtin::Atan2 | Builtin::Min | Builtin::Max => 2,
                    Builtin::Limit => 3,
                    _ => 1,
                };
                let mut regs = [u32::MAX; 3];
                for (i, slot) in regs.iter_mut().enumerate().take(n) {
                    *slot = match args.get(i) {
                        Some(a) => self.expr(a, cuts, out, cmp)?,
                        None => {
                            let r = self.reg();
                            self.emit(out, JOp::All(r))?;
                            r
                        }
                    };
                }
                let r = self.reg();
                self.emit(out, JOp::Call(r, *f, regs, n as u8))?;
                r
            }
            Expr::Table { table, args } => {
                let mut regs = [u32::MAX; 2];
                if args.len() > 2 {
                    return Err("a table of more than two arguments".into());
                }
                for (i, a) in args.iter().enumerate() {
                    regs[i] = self.expr(a, cuts, out, cmp)?;
                }
                let r = self.reg();
                self.emit(out, JOp::Table(r, *table, regs, args.len() as u8))?;
                r
            }
            Expr::If(c, a, b) => {
                let rc = self.expr(c, cuts, out, cmp)?;
                let mut pa = Prog::default();
                pa.out = self.expr(a, cuts, &mut pa.ops, cmp)?;
                let mut pb = Prog::default();
                pb.out = self.expr(b, cuts, &mut pb.ops, cmp)?;
                let a_cut = self.cut(c, true, a, cuts, cmp)?;
                let b_cut = self.cut(c, false, b, cuts, cmp)?;
                let r = self.reg();
                let op =
                    IfOp { c: rc, comparison: iv::is_comparison(c), a: pa, b: pb, a_cut, b_cut };
                self.emit(out, JOp::If(r, Box::new(op)))?;
                r
            }
        };
        Ok(r)
    }

    /// The bounds `c` puts on the variables it compares where it holds
    /// (`holds`) or fails, as the interpreter's `bounds` walks it:
    /// (variable, bound), in its order.
    fn bounds(
        &self,
        c: &Expr,
        holds: bool,
        cmp: &HashMap<*const Expr, (u32, u32)>,
        out: &mut Vec<(VarId, Bound)>,
    ) {
        match c {
            Expr::NoEvent(a) => self.bounds(a, holds, cmp, out),
            Expr::Not(a) => self.bounds(a, !holds, cmp, out),
            Expr::And(a, b) if holds => {
                self.bounds(a, true, cmp, out);
                self.bounds(b, true, cmp, out);
            }
            Expr::Or(a, b) if !holds => {
                self.bounds(a, false, cmp, out);
                self.bounds(b, false, cmp, out);
            }
            Expr::Compare(op, a, b) => {
                let Some(&(ra, rb)) = cmp.get(&(c as *const Expr)) else { return };
                // x below y (or equal) when it holds
                let below = matches!(op, CmpOp::Lt | CmpOp::Le) == holds;
                let ((lo, rlo), (hi, rhi)) =
                    if below { ((a, ra), (b, rb)) } else { ((b, rb), (a, ra)) };
                if let Expr::Var(v) = &**lo {
                    out.push((*v, Bound::Below(rhi)));
                }
                if let Expr::Var(v) = &**hi {
                    out.push((*v, Bound::Above(rlo)));
                }
            }
            _ => {}
        }
    }

    /// Branch `e` of an `if` on `c` with the variables `c` compares cut
    /// (`None`: it bounds none that moves).
    fn cut(
        &mut self,
        c: &Expr,
        holds: bool,
        e: &Expr,
        cuts: &HashMap<u32, u32>,
        cmp: &mut HashMap<*const Expr, (u32, u32)>,
    ) -> Result<Option<Cut>, String> {
        let mut b = vec![];
        self.bounds(c, holds, cmp, &mut b);
        // only the variables that move (the interpreter's leaf lookup
        // gives no others)
        let mut kept = vec![];
        for (v, bound) in b {
            let at = match cuts.get(&v.0) {
                Some(&r) => At::Reg(r),
                None => self.var(v)?,
            };
            if Self::moves(at) {
                let At::Reg(r) = at else { unreachable!() };
                kept.push((v, r, bound));
            }
        }
        if kept.is_empty() {
            return Ok(None);
        }
        let mut inner = cuts.clone();
        let mut init = vec![];
        let mut bounds = vec![];
        for (v, ctx_reg, bound) in kept {
            let r = match init.iter().find(|(w, _, _)| *w == v.0) {
                Some(&(_, r, _)) => r,
                None => {
                    let r = self.reg();
                    init.push((v.0, r, ctx_reg));
                    r
                }
            };
            inner.insert(v.0, r);
            bounds.push((r, bound));
        }
        let mut body = Prog::default();
        body.out = self.expr(e, &inner, &mut body.ops, cmp)?;
        Ok(Some(Cut { init: init.into_iter().map(|(_, r, c)| (r, c)).collect(), bounds, body }))
    }
}

/// What a kernel's evaluation reads.
struct Env<'a> {
    t: Iv,
    d: &'a [f64],
    p: &'a [f64],
    u: &'a [f64],
    cx: &'a Cx<'a>,
}

fn get(regs: &[Enclosure], r: u32) -> J2 {
    j2(&regs[r as usize])
}

fn set(regs: &mut [Enclosure], r: u32, j: J2) {
    regs[r as usize] = enc(j);
}

/// Runs a program.
fn run(ops: &[JOp], env: &Env<'_>, y: &[Enclosure], regs: &mut [Enclosure]) {
    let k = |v: f64| J2::konst(Iv::point(v));
    for op in ops {
        match op {
            JOp::Konst(d, c) => set(regs, *d, k(*c)),
            JOp::Param(d, i) => set(regs, *d, k(env.p[*i as usize])),
            JOp::D(d, i) => set(regs, *d, k(env.d[*i as usize])),
            JOp::U(d, i) => set(regs, *d, k(env.u[*i as usize])),
            JOp::Leaf(d, i) => regs[*d as usize] = y[*i as usize],
            JOp::Time(d) => set(regs, *d, iv::time_j2(env.t)),
            JOp::All(d) => set(regs, *d, J2::all()),
            JOp::Copy(d, a) => regs[*d as usize] = regs[*a as usize],
            JOp::Neg(d, a) => set(regs, *d, get(regs, *a).neg()),
            JOp::Bin(d, op, a, b) => {
                set(regs, *d, iv::binary_j2(*op, get(regs, *a), get(regs, *b)))
            }
            JOp::Cmp(d, op, a, b) => {
                set(regs, *d, iv::compare_j2(*op, get(regs, *a), get(regs, *b)))
            }
            JOp::And(d, a, b) => set(regs, *d, iv::and_j2(get(regs, *a), get(regs, *b))),
            JOp::Or(d, a, b) => set(regs, *d, iv::or_j2(get(regs, *a), get(regs, *b))),
            JOp::Not(d, a) => set(regs, *d, iv::not_j2(get(regs, *a))),
            JOp::Call(d, f, args, n) => {
                let mut at = [J2::all(); 3];
                for i in 0..*n as usize {
                    at[i] = get(regs, args[i]);
                }
                set(regs, *d, iv::call_j2(*f, &at[..*n as usize]))
            }
            JOp::Table(d, t, args, n) => {
                let mut at = [J2::all(); 2];
                for i in 0..*n as usize {
                    at[i] = get(regs, args[i]);
                }
                set(regs, *d, iv::table_j2(env.cx, *t, &at[..*n as usize]))
            }
            JOp::If(d, op) => {
                let j = run_if(op, env, y, regs);
                set(regs, *d, j)
            }
        }
    }
}

/// A branch where it is taken (`None`: never).
fn branch(
    cut: &Option<Cut>,
    plain: &Prog,
    env: &Env<'_>,
    y: &[Enclosure],
    regs: &mut [Enclosure],
) -> Option<J2> {
    let Some(c) = cut else {
        run(&plain.ops, env, y, regs);
        return Some(get(regs, plain.out));
    };
    for &(r, from) in &c.init {
        regs[r as usize] = regs[from as usize];
    }
    for &(r, bound) in &c.bounds {
        let b = match bound {
            Bound::Below(h) => iv::below_j2(get(regs, h)),
            Bound::Above(l) => iv::above_j2(get(regs, l)),
        };
        let j = iv::cut_j2(get(regs, r), b)?;
        set(regs, r, j);
    }
    run(&c.body.ops, env, y, regs);
    Some(get(regs, c.body.out))
}

fn run_if(op: &IfOp, env: &Env<'_>, y: &[Enclosure], regs: &mut [Enclosure]) -> J2 {
    let cj = get(regs, op.c);
    let cv = cj.v;
    let zero = Iv { lo: 0.0, hi: 0.0 };
    if !(cv.lo <= 0.0 && cv.hi >= 0.0) {
        run(&op.a.ops, env, y, regs);
        return get(regs, op.a.out);
    } else if cv == zero {
        run(&op.b.ops, env, y, regs);
        return get(regs, op.b.out);
    }
    let ra = branch(&op.a_cut, &op.a, env, y, regs);
    let rb = branch(&op.b_cut, &op.b, env, y, regs);
    let (a, b) = match (ra, rb) {
        (Some(a), Some(b)) => (a, b),
        (Some(a), None) => return a,
        (None, Some(b)) => return b,
        (None, None) => {
            run(&op.a.ops, env, y, regs);
            run(&op.b.ops, env, y, regs);
            (get(regs, op.a.out), get(regs, op.b.out))
        }
    };
    iv::if_j2(op.comparison, cj, a, b)
}

/// The entries of y that `e` reads anywhere (in comparisons too: what
/// the interval interpreter's leaves are), not only where a derivative
/// can flow.
fn y_reads(sys: &System<'_>, cx: &Ctx<'_>, e: &Expr) -> Vec<u32> {
    let mut out = vec![];
    e.walk(&mut |x| {
        if let Expr::Var(v) | Expr::Pre(v) = x
            && let Ok(Src::Y(i)) = sys.resolve(cx, *v, false)
        {
            out.push(i as u32);
        }
    });
    out.sort_unstable();
    out.dedup();
    out
}

/// Per assignment: whether it reads y (anywhere, through what it reads).
fn reads_y(sys: &System<'_>, cx: &Ctx<'_>) -> Vec<bool> {
    let n = sys.exprs.len();
    let mut r = vec![false; n];
    for k in 0..n {
        r[k] = !y_reads(sys, cx, sys.exprs[k]).is_empty() || sys.refs[k].iter().any(|&j| r[j]);
    }
    r
}

/// The zero crossings worth a kernel: those whose closure reads time and
/// the integrator's variables.
pub(crate) fn candidates(m: &PreparedModel, sys: &System<'_>, cx: &Ctx<'_>) -> Vec<u32> {
    // per assignment: whether it reads time (through what it reads)
    let n = sys.exprs.len();
    let mut time = vec![false; n];
    for k in 0..n {
        time[k] = sys.exprs[k].any(&mut |x| matches!(x, Expr::Time))
            || sys.refs[k].iter().any(|&j| time[j]);
    }
    let ry = reads_y(sys, cx);
    let mut out = vec![];
    for (k, z) in m.zero_crossings.iter().enumerate() {
        let Ok(refs) = sys.refs_of(cx, &z.expr) else { continue };
        let reads_time =
            z.expr.any(&mut |x| matches!(x, Expr::Time)) || refs.iter().any(|&j| time[j]);
        let reads_y = !y_reads(sys, cx, &z.expr).is_empty() || refs.iter().any(|&j| ry[j]);
        if reads_time && reads_y {
            out.push(k as u32);
        }
    }
    out
}

impl Kernels {
    /// The interval kernels of the crossings `ks` (each declined that
    /// cannot be compiled).
    pub(crate) fn build(m: &PreparedModel, sys: &System<'_>, cx: &Ctx<'_>, ks: &[u32]) -> Kernels {
        let n = sys.exprs.len();
        // per assignment: whether it moves along a step (reads time or y,
        // directly or through what it reads)
        let mut moving = vec![false; n];
        for k in 0..n {
            moving[k] = !y_reads(sys, cx, sys.exprs[k]).is_empty()
                || sys.exprs[k].any(&mut |x| matches!(x, Expr::Time))
                || sys.refs[k].iter().any(|&j| moving[j]);
        }
        let mut b = Builder {
            cx,
            sys,
            next: 0,
            step_reg: HashMap::new(),
            leaf_reg: HashMap::new(),
            ops: 0,
        };
        let mut conds: Vec<Option<Cond>> = vec![None; m.zero_crossings.len()];
        let mut fixed_steps: Vec<usize> = vec![];
        for &k in ks {
            let z = &m.zero_crossings[k as usize];
            let Ok(refs) = sys.refs_of(cx, &z.expr) else { continue };
            let steps = sys.closure(refs);
            let start_ops = b.ops;
            b.ops = 0;
            let built = (|| -> Result<Cond, String> {
                let mut prog = Prog::default();
                let mut cmp = HashMap::new();
                // its leaves: the entries of y it reads, loaded first
                let mut leaves: Vec<u32> = vec![];
                for &s in &steps {
                    leaves.extend(y_reads(sys, cx, sys.exprs[s]));
                }
                leaves.extend(y_reads(sys, cx, &z.expr));
                leaves.sort_unstable();
                leaves.dedup();
                for &i in &leaves {
                    let r = b.leaf(i);
                    b.emit(&mut prog.ops, JOp::Leaf(r, i))?;
                }
                for &s in &steps {
                    if moving[s] {
                        let mut ops = vec![];
                        let r = b.expr(sys.exprs[s], &HashMap::new(), &mut ops, &mut cmp)?;
                        // (the step's value lands in its register)
                        let dst = b.step(s);
                        relabel(&mut ops, r, dst);
                        prog.ops.extend(ops);
                    }
                }
                prog.out = b.expr(&z.expr, &HashMap::new(), &mut prog.ops, &mut cmp)?;
                Ok(Cond { prog })
            })();
            b.ops += start_ops;
            if let Err(why) = &built
                && std::env::var_os("LSIM_CODEGEN_TRACE").is_some()
            {
                eprintln!("lsim-codegen: no kernel for zero crossing {k}: {why}");
            }
            if let Ok(c) = built {
                for &s in &steps {
                    if !moving[s] && !fixed_steps.contains(&s) {
                        fixed_steps.push(s);
                    }
                }
                conds[k as usize] = Some(c);
            }
        }
        // the steps that do not move, in the model's order
        fixed_steps.sort_unstable();
        let mut fixed = Prog::default();
        let mut key = vec![];
        let mut cmp = HashMap::new();
        let mut ok = true;
        for &s in &fixed_steps {
            let mut ops = vec![];
            match b.expr(sys.exprs[s], &HashMap::new(), &mut ops, &mut cmp) {
                Ok(r) => {
                    let dst = b.step(s);
                    relabel(&mut ops, r, dst);
                    fixed.ops.extend(ops);
                }
                Err(_) => ok = false,
            }
            sys.exprs[s].walk(&mut |x| match x {
                Expr::Param(p) => key.push(KeySrc::P(p.0)),
                Expr::Var(v) | Expr::Pre(v) => match sys.resolve(cx, *v, false) {
                    Ok(Src::D(k)) => key.push(KeySrc::D(k as u32)),
                    Ok(Src::U(k)) => key.push(KeySrc::U(k as u32)),
                    _ => {}
                },
                _ => {}
            });
        }
        if !ok {
            // (a fixed step that cannot be compiled: none of its readers is)
            conds.iter_mut().for_each(|c| *c = None);
            fixed = Prog::default();
            key.clear();
        }
        key.dedup();
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Kernels {
            conds,
            fixed,
            key,
            n_regs: b.next as usize,
            id: NEXT.fetch_add(1, Ordering::Relaxed),
        }
    }

    pub(crate) fn covers(&self, k: usize) -> bool {
        self.conds.get(k).is_some_and(|c| c.is_some())
    }

    pub(crate) fn any(&self) -> bool {
        self.conds.iter().any(|c| c.is_some())
    }

    /// The enclosures of scratch it needs.
    pub(crate) fn scratch(&self) -> usize {
        HEADER + self.key.len().div_ceil(6) + self.n_regs
    }

    /// A copy for other table data (its cache is not the original's).
    pub(crate) fn fresh(&self) -> Kernels {
        static NEXT: AtomicU64 = AtomicU64::new(1 << 40);
        Kernels {
            conds: self.conds.clone(),
            fixed: self.fixed.clone(),
            key: self.key.clone(),
            n_regs: self.n_regs,
            id: NEXT.fetch_add(1, Ordering::Relaxed),
        }
    }

    /// Condition `k` over the times `t`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn enclose(
        &self,
        k: usize,
        t: [f64; 2],
        y: &[Enclosure],
        d: &[f64],
        p: &[f64],
        u: &[f64],
        work: &mut [Enclosure],
        tables: &TableTools<'_>,
    ) -> Enclosure {
        let Some(Some(c)) = self.conds.get(k) else {
            return enc(J2::all());
        };
        assert!(work.len() >= self.scratch(), "the kernels' scratch is too short");
        let nk = self.key.len().div_ceil(6);
        let (head, regs) = work.split_at_mut(HEADER + nk);
        let cx = Cx {
            params: p,
            vars: &[],
            model: tables.model,
            breaks: tables.breaks,
            grids: tables.grids,
            leaf: &|_| None,
        };
        let env = Env { t: Iv { lo: t[0], hi: t[1] }, d, p, u, cx: &cx };
        // the steps that do not move: as long as what they read keeps its
        // bits (and the scratch is this compilation's)
        let key = |s: &KeySrc| match *s {
            KeySrc::D(i) => d[i as usize],
            KeySrc::U(i) => u[i as usize],
            KeySrc::P(i) => p[i as usize],
        };
        let mark = f64::from_bits(self.id);
        let held =
            head[0].v[0].to_bits() == mark.to_bits()
                && self.key.iter().enumerate().all(|(i, s)| {
                    key_slot(&head[HEADER + i / 6], i % 6).to_bits() == key(s).to_bits()
                });
        if !held {
            run(&self.fixed.ops, &env, y, regs);
            head[0].v[0] = mark;
            for (i, s) in self.key.iter().enumerate() {
                set_key_slot(&mut head[HEADER + i / 6], i % 6, key(s));
            }
        }
        run(&c.prog.ops, &env, y, regs);
        regs[c.prog.out as usize]
    }
}

/// Entry `i` (of six) of an enclosure used as storage.
fn key_slot(e: &Enclosure, i: usize) -> f64 {
    match i {
        0 => e.v[0],
        1 => e.v[1],
        2 => e.d[0],
        3 => e.d[1],
        4 => e.dd[0],
        _ => e.dd[1],
    }
}

fn set_key_slot(e: &mut Enclosure, i: usize, x: f64) {
    match i {
        0 => e.v[0] = x,
        1 => e.v[1] = x,
        2 => e.d[0] = x,
        3 => e.d[1] = x,
        4 => e.dd[0] = x,
        _ => e.dd[1] = x,
    }
}

/// What the interval kernels read the tables through.
pub(crate) struct TableTools<'a> {
    pub model: &'a dyn ModelFunctions,
    pub breaks: &'a [Vec<f64>],
    pub grids: &'a [Option<Grid2>],
}

/// Per table: a 1-D table's breakpoints (empty for 2-D), a 2-D table's
/// grid.
pub(crate) type Tools = (Vec<Vec<f64>>, Vec<Option<Grid2>>);

/// The tables' breakpoints and grids as the run loop builds them from a
/// model's tables (`RunInfo::table_breaks`, its 2-D grids): a 1-D
/// table's points, a 2-D table's grid.
pub(crate) fn table_tools(store: &TableStore) -> Tools {
    let mut breaks = vec![];
    let mut grids = vec![];
    for k in 0..store.len() {
        let t = store.get(k);
        if t.dims() == 2 {
            breaks.push(vec![]);
            grids.push(Grid2::new(k as u32, t.points(0), t.points(1)));
        } else {
            breaks.push(t.points(0).to_vec());
            grids.push(None);
        }
    }
    (breaks, grids)
}

/// Makes `to` the register of what `from` holds, `ops` being the
/// operations of one step: the last of them, when it wrote `from`, writes
/// `to` instead; otherwise (`from` is a register written elsewhere, a
/// leaf or another step) a copy.
fn relabel(ops: &mut Vec<JOp>, from: u32, to: u32) {
    if from == to {
        return;
    }
    match ops.last_mut() {
        Some(op) if dst_of(op) == from => set_dst(op, to),
        _ => ops.push(JOp::Copy(to, from)),
    }
}

fn dst_of(op: &JOp) -> u32 {
    match op {
        JOp::Konst(d, _)
        | JOp::Param(d, _)
        | JOp::D(d, _)
        | JOp::U(d, _)
        | JOp::Leaf(d, _)
        | JOp::Time(d)
        | JOp::All(d)
        | JOp::Neg(d, _)
        | JOp::Bin(d, ..)
        | JOp::Cmp(d, ..)
        | JOp::And(d, ..)
        | JOp::Or(d, ..)
        | JOp::Not(d, _)
        | JOp::Call(d, ..)
        | JOp::Table(d, ..)
        | JOp::Copy(d, _)
        | JOp::If(d, _) => *d,
    }
}

fn set_dst(op: &mut JOp, to: u32) {
    match op {
        JOp::Konst(d, _)
        | JOp::Param(d, _)
        | JOp::D(d, _)
        | JOp::U(d, _)
        | JOp::Leaf(d, _)
        | JOp::Time(d)
        | JOp::All(d)
        | JOp::Neg(d, _)
        | JOp::Bin(d, ..)
        | JOp::Cmp(d, ..)
        | JOp::And(d, ..)
        | JOp::Or(d, ..)
        | JOp::Not(d, _)
        | JOp::Call(d, ..)
        | JOp::Table(d, ..)
        | JOp::Copy(d, _)
        | JOp::If(d, _) => *d = to,
    }
}
