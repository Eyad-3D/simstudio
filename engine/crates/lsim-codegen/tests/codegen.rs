//! The generated code against the reference interpreter (values) and
//! against central finite differences (Jacobian-vector products), on a
//! model that uses every arithmetic operator and built-in function.

use lsim_codegen::{CodegenOptions, compile};
use lsim_ir::component::build::{eq, param, state, var};
use lsim_ir::eval::{SliceEnv, eval};
use lsim_ir::expr::{Builtin, CmpOp, Expr, c, call, cmp, der, if_, name as n};
use lsim_ir::runtime::{EvalInput, ModelFunctions};
use lsim_ir::{BinaryOp, ComponentDef, Slot};
use lsim_prep::{PrepOptions, prepare};

fn f(b: Builtin, args: Vec<Expr>) -> Expr {
    call(b, args)
}

fn pow(a: Expr, b: Expr) -> Expr {
    Expr::bin(BinaryOp::Pow, a, b)
}

fn functions_model() -> ComponentDef {
    let (x, y) = (|| n("x"), || n("y"));
    ComponentDef {
        name: "Test.Functions".into(),
        params: vec![param("rate", "1/s", 1.0, "makes the derivatives' units right")],
        vars: vec![
            state("x", "1", 0.7, ""),
            state("y", "1", 1.3, ""),
            var("a", "1", ""),
            var("b", "1", ""),
            var("k", "1", ""),
            var("e", "1", ""),
            var("g", "1", ""),
            var("h", "1", ""),
        ],
        equations: vec![
            eq(
                n("a"),
                f(Builtin::Sin, vec![x()]) * f(Builtin::Cos, vec![y()])
                    + f(Builtin::Exp, vec![-(x() * y())]),
                "",
            ),
            eq(
                n("b"),
                f(Builtin::Log, vec![c(1.0) + x() * x()]) + f(Builtin::Sqrt, vec![c(2.0) + y()])
                    - f(Builtin::Tanh, vec![x() - y()]),
                "",
            ),
            eq(
                n("k"),
                f(Builtin::Atan2, vec![x(), y() + c(2.0)]) + f(Builtin::Max, vec![x(), y()])
                    - f(Builtin::Min, vec![x(), c(0.5) * y()])
                    + f(Builtin::Atan, vec![x() - c(0.2)]),
                "",
            ),
            eq(n("e"), pow(x(), c(3.0)) + pow(y(), c(2.5)) + pow(x(), y()) + pow(y(), c(-2.0)), ""),
            eq(
                n("g"),
                Expr::NoEvent(Box::new(if_(
                    cmp(CmpOp::Gt, x(), y()),
                    x() * y(),
                    x() / (c(1.0) + y()),
                ))),
                "",
            ),
            eq(
                n("h"),
                f(Builtin::Sinh, vec![c(0.3) * x()])
                    + f(Builtin::Cosh, vec![c(0.2) * y()])
                    + f(Builtin::Tan, vec![c(0.1) * x()])
                    + f(Builtin::Asin, vec![c(0.3) * x()])
                    + f(Builtin::Acos, vec![c(0.2) * y()]),
                "",
            ),
            eq(der("x"), n("rate") * (n("a") - n("b") + c(0.1) * n("k") + c(0.01) * n("h")), ""),
            eq(
                der("y"),
                n("rate")
                    * (c(0.01) * n("e") - n("g") + f(Builtin::Limit, vec![x(), c(-0.5), c(0.5)])),
                "",
            ),
        ],
        ..Default::default()
    }
}

#[test]
fn generated_code_matches_the_interpreter_and_finite_differences() {
    let lib = lsim_lib::library();
    let model = prepare(&lib, &functions_model(), &PrepOptions::default()).expect("prepares");
    assert_eq!(model.states.len(), 2);
    assert_eq!(model.algebraics.len(), 0);
    let jit = compile(&model, &CodegenOptions::default()).expect("compiles");
    let l = *jit.layout();
    let p: Vec<f64> = model.flat.params.iter().map(|q| q.value).collect();
    let (d, u) = (vec![0.0; l.n_d], vec![0.0; l.n_u]);
    let mut work = vec![0.0; l.n_work];
    for (x0, y0) in [(0.7, 1.3), (1.9, 0.4), (0.3, 2.2)] {
        let y = [x0, y0];
        let inp = EvalInput { t: 0.0, y: &y, p: &p, d: &d, u: &u };
        let mut out = [0.0; 2];
        jit.residual(&inp, &mut work, &mut out);

        // the interpreter, assignment by assignment
        let nv = model.flat.vars.len();
        let mut vars = vec![f64::NAN; nv];
        let mut ders = vec![f64::NAN; nv];
        for (i, s) in model.states.iter().enumerate() {
            vars[s.0 as usize] = y[i];
        }
        for a in &model.assignments {
            let v = eval(&a.expr, &SliceEnv { t: 0.0, vars: &vars, ders: &ders, params: &p });
            match a.target {
                Slot::Var(v_) => vars[v_.0 as usize] = v,
                Slot::Der(v_) => ders[v_.0 as usize] = v,
            }
        }
        for (i, s) in model.states.iter().enumerate() {
            let want = ders[s.0 as usize];
            assert!((out[i] - want).abs() <= 1e-14 * want.abs().max(1.0), "{} vs {want}", out[i]);
        }

        // jvp against central differences
        for j in 0..2 {
            let mut v = [0.0; 2];
            v[j] = 1.0;
            let mut jv = [0.0; 2];
            jit.jvp(&inp, &v, &mut work, &mut jv);
            let h = 1e-6;
            let (mut yp, mut ym) = (y, y);
            yp[j] += h;
            ym[j] -= h;
            let (mut fp, mut fm) = ([0.0; 2], [0.0; 2]);
            jit.residual(&EvalInput { y: &yp, ..inp }, &mut work, &mut fp);
            jit.residual(&EvalInput { y: &ym, ..inp }, &mut work, &mut fm);
            for i in 0..2 {
                let fd = (fp[i] - fm[i]) / (2.0 * h);
                assert!(
                    (jv[i] - fd).abs() < 1e-7 * fd.abs().max(1.0),
                    "d f{i}/d y{j} at {y:?}: {} vs {fd}",
                    jv[i]
                );
            }
        }
    }
}

#[test]
fn implicit_form_has_a_consistent_jacobian() {
    // the same model with every block implicit: 2 states + 8 iteration
    // variables; the jvp must match finite differences of the residual
    let lib = lsim_lib::library();
    let model = prepare(&lib, &functions_model(), &PrepOptions { force_implicit: true }).unwrap();
    let jit = compile(&model, &CodegenOptions::default()).unwrap();
    let l = *jit.layout();
    let n = l.n_y();
    assert_eq!((l.n_x, l.n_z), (2, 8));
    let p: Vec<f64> = model.flat.params.iter().map(|q| q.value).collect();
    let (d, u) = (vec![0.0; l.n_d], vec![0.0; l.n_u]);
    let mut work = vec![0.0; l.n_work];
    let y: Vec<f64> = (0..n).map(|i| 0.3 + 0.17 * i as f64).collect();
    let inp = EvalInput { t: 0.0, y: &y, p: &p, d: &d, u: &u };
    let mut jac = vec![0.0; n * n];
    jit.jacobian_dense(&inp, &mut work, &mut jac);
    for j in 0..n {
        let h = 1e-6;
        let (mut yp, mut ym) = (y.clone(), y.clone());
        yp[j] += h;
        ym[j] -= h;
        let (mut fp, mut fm) = (vec![0.0; n], vec![0.0; n]);
        jit.residual(&EvalInput { y: &yp, ..inp }, &mut work, &mut fp);
        jit.residual(&EvalInput { y: &ym, ..inp }, &mut work, &mut fm);
        for i in 0..n {
            let fd = (fp[i] - fm[i]) / (2.0 * h);
            let a = jac[j * n + i];
            assert!(
                (a - fd).abs() < 1e-6 * fd.abs().max(1.0),
                "J[{i}][{j}] = {a}, differences say {fd}"
            );
        }
    }
}
