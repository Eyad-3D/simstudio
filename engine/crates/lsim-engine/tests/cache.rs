//! The prepared-model cache: a hit is the model built without it, and a
//! build at other parameter values is prepared again, since preparation
//! chooses by them (index reduction picks a pendulum's states by pivoting
//! at its start).

use lsim_engine::lsim_ir::component::VarDecl;
use lsim_engine::lsim_ir::component::build::{eq, param, state, sub, var};
use lsim_engine::lsim_ir::expr::{Builtin, Expr, c, call, der, name as n};
use lsim_engine::lsim_ir::{ComponentDef, Library};
use lsim_engine::lsim_solve::{OutputGrid, SolverOptions};
use lsim_engine::{BuildOptions, Engine, Model};

/// A pendulum on a rigid rod of length `L0`, released at rest at `theta0`
/// from the vertical: index 3, its rod's force `F` unknown.
fn pendulum() -> ComponentDef {
    let guess =
        |name: &str, unit: &str, start: Expr| VarDecl { start: Some(start), ..var(name, unit, "") };
    ComponentDef {
        name: "Test.Pendulum".into(),
        params: vec![
            param("m", "kg", 1.0, "mass"),
            param("g", "m/s2", 9.81, "gravity"),
            param("theta0", "1", 0.4, "start angle"),
            param("L0", "m", 1.0, "length"),
        ],
        vars: vec![
            VarDecl {
                start: Some(n("L0") * call(Builtin::Sin, vec![n("theta0")])),
                fixed: true,
                ..var("x", "m", "")
            },
            guess("y", "m", -(n("L0") * call(Builtin::Cos, vec![n("theta0")]))),
            state("vx", "m/s", 0.0, ""),
            guess("vy", "m/s", c(0.0)),
            guess("F", "N", n("m") * n("g")),
        ],
        equations: vec![
            eq(der("x"), n("vx"), "x"),
            eq(der("y"), n("vy"), "y"),
            eq(n("m") * der("vx"), -(n("x") * n("F") / n("L0")), "fx"),
            eq(n("m") * der("vy"), -(n("y") * n("F") / n("L0")) - n("m") * n("g"), "fy"),
            eq(n("x") * n("x") + n("y") * n("y"), n("L0") * n("L0"), "rod"),
        ],
        ..Default::default()
    }
}

fn top(theta0: f64) -> ComponentDef {
    ComponentDef {
        name: "Test.Top".into(),
        components: vec![sub("p", "Test.Pendulum", &[("theta0", c(theta0)), ("L0", c(1.0))])],
        ..Default::default()
    }
}

fn engine() -> Engine {
    let mut lib: Library = lsim_lib::library();
    lib.add(pendulum());
    Engine::new(lib)
}

/// `p.x` at 0.5, 1, 1.5 and 2 s, and the whole run's values.
fn swing(m: &Model) -> (Vec<f64>, Vec<Vec<f64>>) {
    let opts = SolverOptions { rtol: 1e-8, atol: 1e-8, ..Default::default() };
    let run = m.simulate(&opts, OutputGrid { t0: 0.0, t_end: 2.0, dt: 0.01 }).expect("runs");
    let ix = run.names.iter().position(|x| x == "p.x").expect("p.x");
    (
        vec![run.values[ix][50], run.values[ix][100], run.values[ix][150], run.values[ix][200]],
        run.values,
    )
}

#[test]
fn a_build_at_other_values_is_prepared_again_and_chooses_its_own_states() {
    let dir = std::env::temp_dir().join(format!("lsim-cache-values-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let engine = engine();
    let cached = BuildOptions { cache: Some(dir.clone()), ..Default::default() };

    let first = engine.build(&top(0.4), &cached).expect("builds");
    let again = engine.build(&top(0.4), &cached).expect("builds");
    assert!(!first.report.cache_hit && again.report.cache_hit);
    assert_eq!(swing(&first).1, swing(&again).1, "a hit is the model");

    // released horizontally, x cannot be a state (the rod's constraint
    // does not fix y from it there): preparation must choose again
    let level = engine.build(&top(std::f64::consts::FRAC_PI_2), &cached).expect("builds");
    let plain =
        engine.build(&top(std::f64::consts::FRAC_PI_2), &BuildOptions::default()).expect("builds");
    assert!(!level.report.cache_hit, "other values are not a hit");
    assert_eq!(level.info.y_names, plain.info.y_names);
    assert_ne!(level.info.y_names, first.info.y_names, "the states are chosen at the values");
    let (x, values) = swing(&level);
    assert_eq!(values, swing(&plain).1);
    // theta'' = -g sin(theta) integrated independently (rtol 1e-12)
    for (got, want) in x.iter().zip([0.391048792, -0.986291751, -0.88413236, 0.793566195]) {
        assert!((got - want).abs() < 1e-4, "x = {got}, the pendulum swings to {want}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
