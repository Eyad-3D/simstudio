//! 10 000 random expressions against the reference interpreter
//! (DESIGN.md §16, WP3's acceptance): every arithmetic operation, every
//! built-in, comparisons, `if`, `noEvent`, tables and powers, nested five
//! deep, as the assignments and zero crossings of random models.
//!
//! * The functions the run loop compares with the interpreter (the zero
//!   crossings) are bitwise `lsim_ir::eval`, library calls included (they
//!   call the interpreter's own functions).
//! * The residual's group (here the channels, `vars`) is bitwise the
//!   interpreter but for one shortcut it may take: a constant integer
//!   power multiplied out within one rounding of the exact power (and
//!   `x^0.5` a square root). A mirror of the interpreter that takes the
//!   same shortcut agrees bitwise; the shortcut itself is within 1 ulp of
//!   `powf` (the "library call" tolerance), and closer than `powf` to the
//!   exact power.

#[path = "common/random.rs"]
mod random;
#[path = "common/synth.rs"]
mod synth;

use lsim_codegen::{CodegenOptions, JitModel, compile, native_fma};
use lsim_ir::eval::{Env, eval};
use lsim_ir::expr::{BinaryOp, Expr};
use lsim_ir::runtime::{EvalInput, ModelFunctions};
use lsim_ir::{ParamId, PreparedModel, Slot, VarId};
use synth::Rng;

/// Equal bit for bit (NaNs alike).
fn same(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
}

/// The values of a model's variables at a point, as an interpreter.
struct Point<'a> {
    t: f64,
    vars: &'a [f64],
    params: &'a [f64],
    model: &'a JitModel,
}

impl Env for Point<'_> {
    fn time(&self) -> f64 {
        self.t
    }
    fn var(&self, v: VarId) -> f64 {
        self.vars[v.0 as usize]
    }
    fn der(&self, _: VarId) -> f64 {
        f64::NAN
    }
    fn param(&self, p: ParamId) -> f64 {
        self.params[p.0 as usize]
    }
    fn table(&self, k: u32, args: &[f64]) -> f64 {
        lsim_ir::interval::table_at(self.model, k, args).map_or(f64::NAN, |(v, _)| v)
    }
}

/// `x^n` for an integer |n| ≥ 3 as the generated code multiplies it out
/// (`lower.rs`, `powi_exact`): double-double squaring with fused
/// multiply-adds, one rounding; the plain product where that is not
/// finite or the power underflows.
fn powi_exact(x: f64, n: i32) -> f64 {
    let two_sum = |p: f64, e: f64| {
        let s = p + e;
        let d = s - p;
        (s, e - d)
    };
    let m = n.unsigned_abs();
    let bits = 32 - m.leading_zeros();
    let (mut h, mut l): (f64, Option<f64>) = (x, None);
    let mut pl = x;
    for i in (0..bits - 1).rev() {
        let p = h * h;
        let mut e = h.mul_add(h, -p);
        if let Some(lo) = l {
            e = (h + h).mul_add(lo, e);
        }
        let (s, lo) = two_sum(p, e);
        (h, l) = (s, Some(lo));
        pl *= pl;
        if (m >> i) & 1 == 1 {
            let p = h * x;
            let mut e = h.mul_add(x, -p);
            if let Some(lo) = l {
                e = lo.mul_add(x, e);
            }
            let (s, lo) = two_sum(p, e);
            (h, l) = (s, Some(lo));
            pl *= x;
        }
    }
    let (r, plain) = if n < 0 {
        let q = 1.0 / h;
        let rem = (-q).mul_add(h, 1.0);
        let rem = match l {
            Some(lo) => (-q).mul_add(lo, rem),
            None => rem,
        };
        (q.mul_add(rem, q), 1.0 / pl)
    } else {
        (h, pl)
    };
    if r.is_finite() && pl != 0.0 { r } else { plain }
}

/// The residual's shortcut for `x^n` (`lower.rs`, `pow_const` without
/// the interpreter's exactness), on a target with (`fma`) or without a
/// fused multiply-add.
fn fast_pow(x: f64, n: f64, fma: bool) -> f64 {
    let inline = n == 0.0 || n == 1.0 || n == 2.0 || n == -1.0 || n == 0.5 || {
        n.fract() == 0.0 && (3.0..=16.0).contains(&n.abs())
    };
    if n == 0.0 {
        1.0
    } else if n == 1.0 {
        x
    } else if n == 2.0 {
        x * x
    } else if n == -1.0 {
        1.0 / x
    } else if n == 0.5 {
        if x == f64::NEG_INFINITY { f64::INFINITY } else { x.sqrt() + 0.0 }
    } else if inline && fma {
        powi_exact(x, n as i32)
    } else {
        x.powf(n)
    }
}

/// The interpreter (a copy of `lsim_ir::eval`) with the residual's power
/// shortcut: a constant power taken as the generated code takes it.
fn fast_eval(e: &Expr, env: &dyn Env, fma: bool) -> f64 {
    use lsim_ir::expr::{Builtin, CmpOp};
    let ev = |x: &Expr| fast_eval(x, env, fma);
    let truth = |b: bool| if b { 1.0 } else { 0.0 };
    match e {
        Expr::Const(v) => *v,
        Expr::Time => env.time(),
        Expr::Name(_) => f64::NAN,
        Expr::Table { table, args } => {
            let at: Vec<f64> = args.iter().map(ev).collect();
            env.table(*table, &at)
        }
        Expr::Var(v) => env.var(*v),
        Expr::Param(p) => env.param(*p),
        Expr::Der(v) => env.der(*v),
        Expr::Pre(v) => env.pre(*v),
        Expr::Neg(a) => -ev(a),
        Expr::Binary(BinaryOp::Pow, a, b) if matches!(**b, Expr::Const(_)) => {
            let Expr::Const(n) = **b else { unreachable!() };
            fast_pow(ev(a), n, fma)
        }
        Expr::Binary(op, a, b) => {
            let (a, b) = (ev(a), ev(b));
            match op {
                BinaryOp::Add => a + b,
                BinaryOp::Sub => a - b,
                BinaryOp::Mul => a * b,
                BinaryOp::Div => a / b,
                BinaryOp::Pow => a.powf(b),
            }
        }
        Expr::Call(f, args) => {
            let a = args.first().map(ev).unwrap_or(f64::NAN);
            let b = || ev(&args[1]);
            match f {
                Builtin::Der | Builtin::Pre => f64::NAN,
                Builtin::Sin => a.sin(),
                Builtin::Cos => a.cos(),
                Builtin::Tan => a.tan(),
                Builtin::Asin => a.asin(),
                Builtin::Acos => a.acos(),
                Builtin::Atan => a.atan(),
                Builtin::Atan2 => a.atan2(b()),
                Builtin::Sinh => a.sinh(),
                Builtin::Cosh => a.cosh(),
                Builtin::Tanh => a.tanh(),
                Builtin::Exp => a.exp(),
                Builtin::Log => a.ln(),
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
                Builtin::Min => a.min(b()),
                Builtin::Max => a.max(b()),
                Builtin::Limit => a.max(b()).min(ev(&args[2])),
            }
        }
        Expr::Compare(op, a, b) => {
            let (a, b) = (ev(a), ev(b));
            truth(match op {
                CmpOp::Lt => a < b,
                CmpOp::Le => a <= b,
                CmpOp::Gt => a > b,
                CmpOp::Ge => a >= b,
            })
        }
        Expr::And(a, b) => truth(ev(a) != 0.0 && ev(b) != 0.0),
        Expr::Or(a, b) => truth(ev(a) != 0.0 || ev(b) != 0.0),
        Expr::Not(a) => truth(ev(a) == 0.0),
        Expr::If(c, a, b) => {
            if ev(c) != 0.0 {
                ev(a)
            } else {
                ev(b)
            }
        }
        Expr::NoEvent(a) => ev(a),
    }
}

/// Whether `e` has a constant power.
fn has_power(e: &Expr) -> bool {
    e.any(&mut |x| matches!(x, Expr::Binary(BinaryOp::Pow, _, b) if matches!(**b, Expr::Const(_))))
}

#[test]
fn ten_thousand_random_expressions_agree_with_the_interpreter() {
    let fma = native_fma();
    let mut r = Rng(2026);
    let (mut exprs, mut checks, mut with_powers) = (0usize, 0usize, 0usize);
    for model_no in 0..100 {
        let m: PreparedModel = random::model(&mut r, 3, 100, 8, 5, random::ALL_OPS);
        let jit = compile(&m, &CodegenOptions::default()).expect("compiles");
        let l = *jit.layout();
        let p: Vec<f64> = m.flat.params.iter().map(|q| q.value).collect();
        exprs += m.assignments.len() + m.zero_crossings.len();
        with_powers += m.assignments.iter().filter(|a| has_power(&a.expr)).count();
        let mut work = vec![0.0; l.n_work];
        for _ in 0..5 {
            let t = r.range(-3.0, 3.0);
            let y: Vec<f64> = (0..l.n_y()).map(|_| random::constant(&mut r)).collect();
            let d: Vec<f64> = (0..l.n_d).map(|_| random::constant(&mut r)).collect();
            let u = vec![0.0; l.n_u];
            let inp = EvalInput { t, y: &y, p: &p, d: &d, u: &u };
            // the interpreter (and its mirror with the power shortcut),
            // assignment by assignment
            let nv = m.flat.vars.len();
            let (mut exact, mut fast) = (vec![f64::NAN; nv], vec![f64::NAN; nv]);
            for (i, s) in m.states.iter().enumerate() {
                exact[s.0 as usize] = y[i];
                fast[s.0 as usize] = y[i];
            }
            for (i, v) in m.discretes.iter().enumerate() {
                exact[v.0 as usize] = d[i];
                fast[v.0 as usize] = d[i];
            }
            for a in &m.assignments {
                let Slot::Var(v) = a.target else { continue };
                let x = eval(&a.expr, &Point { t, vars: &exact, params: &p, model: &jit });
                let f = fast_eval(&a.expr, &Point { t, vars: &fast, params: &p, model: &jit }, fma);
                exact[v.0 as usize] = x;
                fast[v.0 as usize] = f;
            }
            // the zero crossings: bitwise the interpreter
            let mut roots = vec![0.0; l.n_roots];
            jit.roots(&inp, &mut work, &mut roots);
            for (k, z) in m.zero_crossings.iter().enumerate() {
                let want = eval(&z.expr, &Point { t, vars: &exact, params: &p, model: &jit });
                assert!(
                    same(roots[k], want),
                    "model {model_no}, crossing {k} at t = {t}: {:e} vs {want:e}\n{}",
                    roots[k],
                    z.expr
                );
                checks += 1;
            }
            // the channels: bitwise the mirror; and bitwise the
            // interpreter itself wherever no constant power was multiplied
            // out on the way
            let mut vars = vec![0.0; l.n_vars];
            jit.vars(&inp, &mut work, &mut vars);
            let mut powered = vec![false; nv];
            for a in &m.assignments {
                let Slot::Var(v) = a.target else { continue };
                let i = v.0 as usize;
                let reads_powered = {
                    let mut any = false;
                    a.expr.walk(&mut |x| {
                        if let Expr::Var(w) = x {
                            any |= powered[w.0 as usize];
                        }
                    });
                    any
                };
                powered[i] = reads_powered || has_power(&a.expr);
                assert!(
                    same(vars[i], fast[i]),
                    "model {model_no}, {} at t = {t}: {:e} vs {:e}\n{}",
                    m.flat.vars[i].name,
                    vars[i],
                    fast[i],
                    a.expr
                );
                if !powered[i] {
                    assert!(same(vars[i], exact[i]), "{}: {:e} vs {:e}", a.expr, vars[i], exact[i]);
                }
                checks += 1;
            }
        }
    }
    assert!(exprs >= 10_000, "{exprs} expressions");
    println!(
        "{exprs} random expressions ({with_powers} with a constant power), {checks} values compared"
    );
}

/// Each power the residual multiplies out is within 1 ulp of `powf`, and
/// as close as `powf` to the exact power (double-double) in all but at
/// most one case in a thousand (both are within an ulp of it), over many
/// arguments, the special values included.
#[test]
fn multiplied_out_powers_are_within_an_ulp_of_powf() {
    let fma = native_fma();
    let ulps = |a: f64, b: f64| -> f64 {
        if a == b || (a.is_nan() && b.is_nan()) {
            return 0.0;
        }
        if !(a.is_finite() && b.is_finite()) {
            return f64::INFINITY;
        }
        (a - b).abs() / (b.abs().next_up() - b.abs()).max(f64::MIN_POSITIVE)
    };
    // x^n in double-double, rounded once (the exact power to ~1e-30)
    let dd_pow = |x: f64, n: i32| -> f64 {
        let (mut hi, mut lo) = (1.0f64, 0.0f64);
        for _ in 0..n.unsigned_abs() {
            let p = hi * x;
            let e = hi.mul_add(x, -p) + lo * x;
            let s = p + e;
            lo = e - (s - p);
            hi = s;
        }
        if n < 0 { 1.0 / (hi + lo) } else { hi + lo }
    };
    let mut r = Rng(99);
    let mut worst: f64 = 0.0;
    let mut worse_than_powf = 0;
    let mut n_checked = 0;
    for n in [-16, -7, -3, -2, -1, 2, 3, 4, 5, 7, 8, 9, 13, 16] {
        for k in 0..20_000 {
            let x = match k % 4 {
                0 => r.range(-3.0, 3.0),
                1 => r.range(0.5, 2.0),
                2 => (r.below(1 << 27) as f64) / (1u64 << 20) as f64,
                _ => r.range(-1e5, 1e5),
            };
            let f = fast_pow(x, n as f64, fma);
            let p = x.powf(n as f64);
            let u = ulps(f, p);
            worst = worst.max(u);
            assert!(u <= 1.0, "{x}^{n}: {f:e} vs powf {p:e} ({u} ulps)");
            let exact = dd_pow(x, n);
            if exact.is_finite() && exact != 0.0 && ulps(f, exact) > ulps(p, exact) + 1e-9 {
                worse_than_powf += 1;
            }
            n_checked += 1;
        }
    }
    for x in [0.0, -0.0, f64::INFINITY, f64::NEG_INFINITY, f64::NAN, 1e-300, 1e300, -1e300] {
        for n in [-3.0, -1.0, 0.5, 2.0, 3.0, 16.0] {
            let (f, p) = (fast_pow(x, n, fma), x.powf(n));
            assert!(same(f, p) || ulps(f, p) <= 1.0, "{x}^{n}: {f:e} vs {p:e}");
        }
    }
    println!(
        "{n_checked} powers: within {worst} ulp of powf; {worse_than_powf} less exact than powf"
    );
    assert!(worse_than_powf * 1000 <= n_checked, "{worse_than_powf} of {n_checked}");
}
