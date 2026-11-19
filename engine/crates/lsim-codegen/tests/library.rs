//! Every path that evaluates a library function gives the same bits, and
//! they are lsim-math's correctly rounded ones: the interpreter
//! (`lsim_ir::eval`), the machine code (channels and zero crossings), the
//! tapes (a model refused executable memory runs on them), the condition
//! kernels (their points, and their enclosures against the interval
//! interpreter's, one ulp wide on each side of a point), on lsim-math's
//! hard cases of every function (`lsim-math/tests/data/hard_cases.txt`:
//! inputs closest to a rounding boundary, special values, the inputs where
//! glibc is more than an ulp off) with their correctly rounded results.

#[path = "common/synth.rs"]
mod synth;

use lsim_codegen::{CodegenOptions, JitModel, MachineCode, compile};
use lsim_ir::expr::{BinaryOp, Builtin, Expr};
use lsim_ir::flat::VarId;
use lsim_ir::interval::{self, Cx, Iv, J2};
use lsim_ir::math;
use lsim_ir::runtime::{Enclosure, EvalInput, ModelFunctions};
use lsim_solve::{RunInfo, TimeFunction};
use synth::{Builder, c, v};

/// The library functions as the IR writes them (`None`: the power
/// `x0 ^ x1`), with lsim-math's own.
#[allow(clippy::type_complexity)]
const FUNCTIONS: [(&str, Option<Builtin>, fn(f64, f64) -> f64); 13] = [
    ("sin", Some(Builtin::Sin), |x, _| math::sin(x)),
    ("cos", Some(Builtin::Cos), |x, _| math::cos(x)),
    ("tan", Some(Builtin::Tan), |x, _| math::tan(x)),
    ("asin", Some(Builtin::Asin), |x, _| math::asin(x)),
    ("acos", Some(Builtin::Acos), |x, _| math::acos(x)),
    ("atan", Some(Builtin::Atan), |x, _| math::atan(x)),
    ("atan2", Some(Builtin::Atan2), math::atan2),
    ("sinh", Some(Builtin::Sinh), |x, _| math::sinh(x)),
    ("cosh", Some(Builtin::Cosh), |x, _| math::cosh(x)),
    ("tanh", Some(Builtin::Tanh), |x, _| math::tanh(x)),
    ("exp", Some(Builtin::Exp), |x, _| math::exp(x)),
    ("log", Some(Builtin::Log), |x, _| math::ln(x)),
    ("pow", None, math::pow),
];

/// Equal bit for bit (NaNs alike).
fn same(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
}

/// The hard cases: (function, inputs, correctly rounded result).
fn hard_cases() -> Vec<(String, Vec<f64>, f64)> {
    let text = include_str!("../../lsim-math/tests/data/hard_cases.txt");
    let bits = |s: &str| f64::from_bits(u64::from_str_radix(s, 16).expect("hex"));
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let t: Vec<&str> = l.split_whitespace().collect();
            let n = t.len();
            (t[0].to_string(), t[2..n - 2].iter().map(|s| bits(s)).collect(), bits(t[n - 1]))
        })
        .filter(|(f, _, _)| f != "fma")
        .collect()
}

/// A model with two states x0 and x1, and for each function an assignment
/// `a = f(x0)` (`f(x0, x1)`, `x0 ^ x1`) and a zero crossing `a + time`,
/// which the run loop checks along steps (it reads time and a state) and
/// so has a kernel. Returns the model, the states and the assignments.
fn model() -> (lsim_ir::PreparedModel, [VarId; 2], Vec<VarId>) {
    let mut b = Builder::new();
    let x0 = b.state("x0", 0.5);
    let x1 = b.state("x1", 0.5);
    b.der(x0, c(0.0));
    b.der(x1, c(0.0));
    let mut out = vec![];
    for (name, f, _) in FUNCTIONS {
        let e = match f {
            Some(Builtin::Atan2) => Expr::Call(Builtin::Atan2, vec![v(x0), v(x1)]),
            Some(f) => Expr::Call(f, vec![v(x0)]),
            None => Expr::bin(BinaryOp::Pow, v(x0), v(x1)),
        };
        let a = b.var(name);
        b.set(a, e);
        b.crossing(Expr::bin(BinaryOp::Add, v(a), Expr::Time));
        out.push(a);
    }
    (b.finish(), [x0, x1], out)
}

/// One way to run the model: its channels and zero crossings at y.
fn evaluate(m: &JitModel, y: &[f64]) -> (Vec<f64>, Vec<f64>) {
    let l = *m.layout();
    let inp = EvalInput { t: 0.0, y, p: &[], d: &[], u: &[] };
    let mut work = vec![0.0; l.n_work];
    let (mut vars, mut roots) = (vec![0.0; l.n_vars], vec![0.0; l.n_roots]);
    m.vars(&inp, &mut work, &mut vars);
    m.roots(&inp, &mut work, &mut roots);
    (vars, roots)
}

#[test]
fn every_path_gives_the_librarys_correctly_rounded_bits() {
    let (pm, [x0, x1], assigned) = model();
    let machine = compile(&pm, &CodegenOptions::default()).expect("compiles");
    let taped =
        compile(&pm, &CodegenOptions { deny_executable_memory: true, ..Default::default() })
            .expect("compiles");
    assert!(machine.machine_code_ready(), "{:?}", machine.machine_code());
    assert!(matches!(taped.machine_code(), MachineCode::Tapes(_)), "{:?}", taped.machine_code());
    let info = RunInfo::from_prepared(&pm);
    let kern = machine.condition_kernels().expect("kernels");
    let (nw, ne) = kern.scratch();
    let mut kwork = vec![f64::NAN; nw];
    let mut ework = vec![Enclosure { v: [0.0; 2], d: [0.0; 2], dd: [0.0; 2] }; ne];
    let cases = hard_cases();
    assert!(cases.len() > 4000, "{} cases", cases.len());
    let mut wrong = vec![];
    let mut tight = 0;
    for (name, args, r) in &cases {
        let k = FUNCTIONS.iter().position(|f| f.0 == name).expect("a library function");
        let (x, y) = (args[0], args.get(1).copied().unwrap_or(0.5));
        let ys = [x, y];
        let mut what = |path: &str, got: f64, want: f64| {
            if !same(got, want) {
                wrong.push(format!("{name}({x:e}, {y:e}) by {path}: {got:e} against {want:e}"));
            }
        };
        // lsim-math itself, against the reference
        let lib = FUNCTIONS[k].2(x, y);
        what("lsim-math", lib, *r);
        // the interpreter
        let mut vals = vec![f64::NAN; pm.flat.vars.len()];
        vals[x0.0 as usize] = x;
        vals[x1.0 as usize] = y;
        let env = lsim_ir::eval::SliceEnv { t: 0.0, vars: &vals, ders: &[], params: &[] };
        let expr = &pm
            .assignments
            .iter()
            .find(|a| a.target == lsim_ir::Slot::Var(assigned[k]))
            .unwrap()
            .expr;
        what("the interpreter", lsim_ir::eval::eval(expr, &env), *r);
        // machine code and tapes: the channel, and the crossing (+ 0.0)
        for (path, m) in [("machine code", &machine), ("tapes", &taped)] {
            let (vars, roots) = evaluate(m, &ys);
            what(path, vars[assigned[k].0 as usize], *r);
            what(&format!("{path}' zero crossing"), roots[k], *r + 0.0);
        }
        // the kernel's point
        let inp = EvalInput { t: 0.0, y: &ys, p: &[], d: &[], u: &[] };
        assert!(kern.covers(k), "no kernel for {name}");
        what("the kernel", kern.point(k, &inp, &mut kwork), *r + 0.0);
        // the enclosure at the point: the kernel's is the interval
        // interpreter's, and holds the result
        let Some(TimeFunction::Mixed { chain, g }) = &info.time_functions[k] else {
            panic!("{name}'s crossing is not a mixed condition");
        };
        let leaf = |j: usize| -> Option<J2> {
            [x0, x1].iter().position(|s| s.0 as usize == j).map(|i| J2::konst(Iv::point(ys[i])))
        };
        let mut steps: Vec<(usize, J2)> = vec![];
        for (vv, e) in chain {
            let l = |j: usize| steps.iter().find(|s| s.0 == j).map(|s| s.1).or_else(|| leaf(j));
            let cx =
                Cx { params: &[], vars: &[], model: &machine, breaks: &[], grids: &[], leaf: &l };
            let j = interval::enclose(e, &cx, Iv::point(0.0));
            steps.push((*vv, j));
        }
        let l = |j: usize| steps.iter().find(|s| s.0 == j).map(|s| s.1).or_else(|| leaf(j));
        let cx = Cx { params: &[], vars: &[], model: &machine, breaks: &[], grids: &[], leaf: &l };
        let want = interval::enclose(g, &cx, Iv::point(0.0));
        // (the kernel gets the leaves the interpreter got: a NaN's point is
        // every number)
        let yenc: Vec<Enclosure> = ys
            .iter()
            .map(|&s| {
                let j = J2::konst(Iv::point(s));
                Enclosure { v: [j.v.lo, j.v.hi], d: [j.d.lo, j.d.hi], dd: [j.dd.lo, j.dd.hi] }
            })
            .collect();
        let got = kern.enclose(k, [0.0, 0.0], &yenc, &[], &[], &[], &mut ework);
        if !(same(got.v[0], want.v.lo) && same(got.v[1], want.v.hi)) {
            wrong.push(format!(
                "{name}({x:e}, {y:e}): kernel encloses {:?}, interpreter {:?}",
                got.v, want.v
            ));
        }
        if r.is_finite() && !(want.v.lo <= *r && *r <= want.v.hi) {
            wrong.push(format!("{name}({x:e}, {y:e}): {r:e} outside {:?}", want.v));
        }
        // the monotone functions on their domains: exactly one ulp each way
        let monotone = matches!(
            name.as_str(),
            "asin" | "acos" | "atan" | "sinh" | "cosh" | "tanh" | "exp" | "log"
        );
        if monotone && r.is_finite() && *r != 0.0 && x.is_finite() {
            let (lo, hi) = (r.next_down(), r.next_up());
            if !(same(want.v.lo, lo) && same(want.v.hi, hi)) {
                wrong.push(format!("{name}({x:e}): encloses {:?}, not [{lo:e}, {hi:e}]", want.v));
            }
            tight += 1;
        }
    }
    assert!(tight > 1000, "{tight} enclosures checked one ulp wide");
    assert!(wrong.is_empty(), "{} differences:\n{}", wrong.len(), wrong.join("\n"));
}
