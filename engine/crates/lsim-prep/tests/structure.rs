//! What preparation hands the solver besides the equations: the Jacobian's
//! sparsity pattern (checked against finite differences of the compiled
//! residual) and the tick order of sampled external blocks.

mod common;

use common::*;
use lsim_ir::component::build::{connect, param, sub};
use lsim_ir::expr::c;
use lsim_ir::runtime::{EvalInput, ModelFunctions};
use lsim_ir::{ComponentDef, Library};
use lsim_prep::{PrepOptions, Settings, prepare, prepare_with_report};

/// A rectifier stage (a source, a resistor and a Shockley diode: an
/// implicit loop), two RC stages and a constant-power load: states and
/// iteration variables, a nonlinear coupling.
fn rectifier(stages: usize) -> ComponentDef {
    let mut top = ComponentDef {
        name: "Test.Rectifier".into(),
        components: vec![
            sub("src", "Electrical.SineVoltage", &[("V0", c(10.0)), ("f", c(50.0))]),
            sub("r1", "Electrical.Resistor", &[("R", c(1.0))]),
            sub("d", "Electrical.Diode", &[]),
            sub("c1", "Electrical.Capacitor", &[("C", c(1e-3))]),
            sub("r2", "Electrical.Resistor", &[("R", c(100.0))]),
            sub("r3", "Electrical.Resistor", &[("R", c(2.0))]),
            sub("c2", "Electrical.Capacitor", &[("C", c(2e-3))]),
            sub("load", "Electrical.PowerLoad", &[("P", c(5.0))]),
            sub("gnd", "Electrical.Ground", &[]),
        ],
        connections: vec![
            connect("src.p", "r1.p"),
            connect("r1.n", "d.p"),
            connect("d.n", "c1.p"),
            connect("c1.p", "r2.p"),
            connect("c1.p", "r3.p"),
            connect("r3.n", "c2.p"),
            connect("c2.p", "load.p"),
            connect("src.n", "gnd.p"),
            connect("c1.n", "gnd.p"),
            connect("r2.n", "gnd.p"),
            connect("c2.n", "gnd.p"),
            connect("load.n", "gnd.p"),
        ],
        ..Default::default()
    };
    // more RC sections after the load, each with a diode to the next
    let mut prev = "c2.p".to_string();
    for k in 0..stages {
        let (r, d, cap) = (format!("rs{k}"), format!("ds{k}"), format!("cs{k}"));
        top.components.push(sub(&r, "Electrical.Resistor", &[("R", c(1.0 + k as f64))]));
        top.components.push(sub(&d, "Electrical.Diode", &[]));
        top.components.push(sub(&cap, "Electrical.Capacitor", &[("C", c(1e-3))]));
        top.connections.push(connect(&prev, &format!("{r}.p")));
        top.connections.push(connect(&format!("{r}.n"), &format!("{d}.p")));
        top.connections.push(connect(&format!("{d}.n"), &format!("{cap}.p")));
        top.connections.push(connect(&format!("{cap}.n"), "gnd.p"));
        prev = format!("{cap}.p");
    }
    top
}

#[test]
fn the_sparsity_pattern_holds_every_nonzero_of_the_jacobian() {
    check_pattern(&rectifier(0));
    check_pattern(&rectifier(8));
}

fn check_pattern(top: &ComponentDef) {
    let (m, report) = prepare_with_report(&library(), top, None, &Settings::default())
        .unwrap_or_else(|d| panic!("{d:#?}"));
    println!("{} blocks", report.blocks.len());
    let jit = lsim_codegen::compile(&m, &Default::default()).expect("compiles");
    let l = *jit.layout();
    let n = l.n_y();
    assert!(l.n_x >= 2 && l.n_z >= 1, "states and iteration variables: {l:?}");
    let pat = &m.jac_pattern;
    assert_eq!(pat.n, n, "the pattern covers y");
    let mut in_pattern = vec![false; n * n];
    for j in 0..n {
        for &i in pat.col(j) {
            in_pattern[i * n + j] = true;
        }
    }
    let p: Vec<f64> = m.flat.params.iter().map(|q| q.value).collect();
    let d = vec![0.0; l.n_d];
    let mut work = vec![0.0; l.n_work];
    // several points, so no entry is zero by chance
    let mut nonzeros = 0;
    let mut seen = vec![false; n * n];
    for (k, t) in [0.001, 0.0037, 0.011, 0.002, 0.005, 0.013].into_iter().enumerate() {
        // capacitor voltages falling along the chain and small iteration
        // variables: every diode conducts, so no entry vanishes
        let y: Vec<f64> = (0..n)
            .map(|i| {
                if k >= 3 {
                    // and small values everywhere
                    0.01 + 0.007 * i as f64 + 0.003 * k as f64
                } else if i < l.n_x {
                    8.0 - 0.55 * i as f64 + 0.01 * k as f64
                } else {
                    0.02 + 0.01 * k as f64
                }
            })
            .collect();
        let f = |y: &[f64], work: &mut [f64]| {
            let mut out = vec![0.0; n];
            jit.residual(&EvalInput { t, y, p: &p, d: &d, u: &[] }, work, &mut out);
            out
        };
        let f0 = f(&y, &mut work);
        for j in 0..n {
            let mut y2 = y.clone();
            let h = 1e-7 * y[j].abs().max(1.0);
            y2[j] += h;
            let f1 = f(&y2, &mut work);
            for i in 0..n {
                let dfdx = (f1[i] - f0[i]) / h;
                let scale = f0[i].abs().max(1.0);
                if dfdx.abs() > 1e-6 * scale {
                    assert!(
                        in_pattern[i * n + j],
                        "∂F{i}/∂y{j} = {dfdx} at t = {t} is not in the pattern ({} by {})",
                        info_row(&m, i),
                        m.algebraics
                            .get(j.wrapping_sub(l.n_x))
                            .map(|s| format!("{s:?}"))
                            .unwrap_or_default()
                    );
                    if !seen[i * n + j] {
                        seen[i * n + j] = true;
                        nonzeros += 1;
                    }
                }
            }
        }
    }
    println!("n = {n}: pattern {} entries, finite differences found {nonzeros}", pat.nnz());
    assert!(nonzeros > 0);
    // the pattern is structural, a superset, but a tight one
    assert!(nonzeros * 10 >= pat.nnz() * 9, "{nonzeros} of {} seen", pat.nnz());
}

fn info_row(m: &lsim_ir::PreparedModel, i: usize) -> String {
    if i < m.states.len() {
        format!("der({})", m.flat.var(m.states[i]).name)
    } else {
        format!("residual {}", i - m.states.len())
    }
}

/// Sampled blocks tick in the order their inputs depend on each other's
/// outputs through the equations: `late` reads `early`'s output through a
/// gain, so it ticks after it, wherever it was declared.
#[test]
fn sampled_blocks_tick_in_dependency_order() {
    let ext = ComponentDef {
        name: "External.Script".into(),
        ports: vec![signal("u", false, "1"), signal("y", true, "1")],
        params: vec![param("period", "s", 0.01, "")],
        ..Default::default()
    };
    let mut lib: Library = library();
    lib.add(ext);
    let top = ComponentDef {
        name: "Test.Chain".into(),
        components: vec![
            sub("late", "External.Script", &[]),
            sub("k", "Signal.Gain", &[("k", c(2.0))]),
            sub("early", "External.Script", &[]),
            sub("alone", "External.Script", &[]),
            sub("one", "Signal.Constant", &[]),
            sub("two", "Signal.Constant", &[]),
        ],
        connections: vec![
            connect("one.y", "early.u"),
            connect("early.y", "k.u"),
            connect("k.y", "late.u"),
            connect("two.y", "alone.u"),
        ],
        ..Default::default()
    };
    let m = prepare(&lib, &top, &PrepOptions::default()).unwrap_or_else(|d| panic!("{d:#?}"));
    let order: Vec<&str> =
        m.external.iter().map(|b| m.flat.instance(b.instance).path.as_str()).collect();
    println!("tick order {order:?}");
    let pos = |name: &str| order.iter().position(|p| *p == name).unwrap();
    assert_eq!(order.len(), 3);
    assert!(pos("early") < pos("late"), "{order:?}");
    assert!(m.warnings.iter().all(|w| w.code != "EXTERNAL-LOOP"), "{:#?}", m.warnings);
    // each block's outputs are discrete, its period its parameter's
    for b in &m.external {
        assert_eq!(b.period, 0.01);
        assert!(b.outputs.iter().all(|v| m.discretes.contains(v)));
    }
}
