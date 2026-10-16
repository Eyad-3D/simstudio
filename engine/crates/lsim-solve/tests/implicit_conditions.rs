//! Conditions on iteration variables. The integrator locates a crossing on
//! its interpolant of everything it integrates, the iteration variables
//! too, which agree with their equations only to its tolerance; the event
//! solves them again from the states, and a condition on one of them
//! (`x > 0.5` with `x³ + x = z`) can come out a little short of zero
//! there, on the side it crossed from. Before, the integrator, restarted
//! there, saw it cross again a moment later: the `when` fired twice for
//! one crossing (`n = 2` at rtol 1e-7 and 1e-9 with z = t; three times
//! for `x³ + x = 2.3 time`, no state, at 1e-10; 43 of 140 runs over the
//! rates and tolerances below), and a mode on it, set back by its
//! relation where nothing else changed, stayed off until the
//! integrator's next step end (its integral 1.6e-2 short at rtol 1e-5).
//! OpenModelica 1.27.1 counts one crossing at every tolerance on the same
//! models (the outside judge's probes, `engine/judge`). Now the condition
//! is held on the side it crossed to until it gets there itself, and a
//! mode set back restarts the integrator, which sees it cross again.

mod common;

use lsim_ir::component::build::{discrete, eq, param, state, var};
use lsim_ir::expr::{Builtin, CmpOp, Expr, c, call, cmp, der, name as n};
use lsim_ir::{ComponentDef, Equation, EquationDecl, VarDecl, WhenAction};
use lsim_solve::{EventKind, OutputGrid, SimResult, SolverOptions, simulate};

const T_END: f64 = 2.0;

/// An iteration variable `x` (start 0, not fixed).
fn x_var() -> VarDecl {
    VarDecl { start: Some(c(0.0)), ..var("x", "1", "solved from x³ + x = …") }
}

/// `when cond then n = pre(n) + 1`, and more actions.
fn counting(cond: Expr, more: Vec<WhenAction>) -> EquationDecl {
    let mut actions = vec![WhenAction::Assign {
        var: "n".into(),
        value: call(Builtin::Pre, vec![n("n")]) + c(1.0),
    }];
    actions.extend(more);
    EquationDecl {
        eq: Equation::When { condition: cond, actions },
        label: Some("it counts".into()),
    }
}

fn cubed(x: Expr) -> Expr {
    x.clone() * x.clone() * x
}

/// `x³ + x = rhs`, with `z` integrated at the rate `a` where `with_state`
/// (else `rhs` reads time), counting `cond`'s crossings.
fn counter(name: &str, a: f64, with_state: bool, rhs: Expr, cond: Expr) -> ComponentDef {
    let mut vars = vec![x_var(), discrete("n", "1", 0.0, "the crossings counted")];
    let mut equations =
        vec![eq(cubed(n("x")) + n("x"), rhs, "x is solved from it"), counting(cond, vec![])];
    if with_state {
        vars.push(state("z", "1", 0.0, "a ramp"));
        equations.push(eq(der("z"), n("a"), "the ramp's rate"));
    }
    ComponentDef {
        name: name.into(),
        params: vec![param("a", "1/s", a, "the rate")],
        vars,
        equations,
        ..Default::default()
    }
}

fn run(top: &ComponentDef, rtols: &[f64]) -> Vec<(f64, SimResult)> {
    let built = common::build(&common::library(), top, false);
    let grid = OutputGrid { t0: 0.0, t_end: T_END, dt: 0.1 };
    rtols
        .iter()
        .map(|&rtol| {
            let opts = SolverOptions { rtol, atol: 1e-2 * rtol, ..Default::default() };
            let r = simulate(&built.jit, &built.info, &opts, grid, &mut [])
                .unwrap_or_else(|e| panic!("{} at rtol {rtol:e}: {e}", top.name));
            (rtol, r)
        })
        .collect()
}

fn last(r: &SimResult, name: &str) -> f64 {
    let k = r.names.iter().position(|x| x == name).unwrap_or_else(|| panic!("{name}"));
    *r.values[k].last().expect("values")
}

/// The times the `when` fired at.
fn fired(r: &SimResult) -> Vec<f64> {
    r.events.iter().filter(|e| matches!(e.kind, EventKind::When(_))).map(|e| e.t).collect()
}

/// Checks one crossing at `exact`, located to `10 rtol`.
fn once(what: &str, runs: &[(f64, SimResult)], exact: f64) -> Vec<String> {
    let mut bad = vec![];
    for (rtol, r) in runs {
        let at = fired(r);
        let count = last(r, "n");
        if count != 1.0 || at.len() != 1 || (at[0] - exact).abs() > 10.0 * rtol {
            bad.push(format!(
                "{what}, rtol {rtol:e}: n = {count}, fired at {at:?} (exact {exact})"
            ));
        }
    }
    bad
}

#[test]
fn a_when_on_an_iteration_variable_fires_once_per_crossing() {
    let rtols = [1e-5, 1e-7, 1e-9, 1e-10];
    let mut bad = vec![];
    for a in [0.7, 1.0, 2.3] {
        // x reaches 0.5 where x³ + x = 0.625
        let exact = 0.625 / a;
        let up = cmp(CmpOp::Gt, n("x"), c(0.5));
        let ramp = counter("Test.ImplicitRamp", a, true, n("z"), up.clone());
        bad.extend(once(&format!("z = {a} t"), &run(&ramp, &rtols), exact));
        let timed = counter("Test.ImplicitTime", a, false, n("a") * Expr::Time, up);
        bad.extend(once(&format!("no state, {a} t"), &run(&timed, &rtols), exact));
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn a_falling_when_on_an_iteration_variable_fires_once() {
    // x³ + x = 2 - t falls through 0.5 at t = 1.375
    let down = cmp(CmpOp::Lt, n("x"), c(0.5));
    let mut m = counter("Test.ImplicitFall", 1.0, true, c(2.0) - n("z"), down);
    m.vars[0].start = Some(c(1.0));
    let bad = once("falling", &run(&m, &[1e-6, 1e-7, 1e-8, 1e-10]), 1.375);
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn a_when_on_an_oscillation_read_through_an_implicit_equation_fires_once() {
    // z = 0.5 sin(3 t); x > 0.3 where z > 0.327, rising once before 2 s
    let w = 3.0;
    let m = ComponentDef {
        name: "Test.ImplicitOscillation".into(),
        params: vec![param("w", "rad/s", w, "")],
        vars: vec![
            state("z", "1", 0.0, ""),
            state("v", "1/s", 1.5, ""),
            x_var(),
            discrete("n", "1", 0.0, ""),
        ],
        equations: vec![
            eq(der("z"), n("v"), "z moves with v"),
            eq(der("v"), -(n("w") * n("w") * n("z")), "it oscillates"),
            eq(cubed(n("x")) + n("x"), n("z"), "x is solved from it"),
            counting(cmp(CmpOp::Gt, n("x"), c(0.3)), vec![]),
        ],
        ..Default::default()
    };
    let exact = lsim_ir::math::asin((0.3 * 0.3 * 0.3 + 0.3) / 0.5) / w;
    let bad = once("oscillation", &run(&m, &[1e-5, 1e-6, 1e-7, 1e-8, 1e-9, 1e-10]), exact);
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn a_mode_on_an_iteration_variable_switches_where_it_crosses() {
    // der(w) = if x > 0.5 then 1 else 0: w(2) = 2 - 0.625
    let m = ComponentDef {
        name: "Test.ImplicitMode".into(),
        vars: vec![state("z", "1", 0.0, ""), state("w", "s", 0.0, ""), x_var()],
        equations: vec![
            eq(der("z"), c(1.0), "a ramp"),
            eq(cubed(n("x")) + n("x"), n("z"), "x is solved from it"),
            eq(
                der("w"),
                Expr::If(
                    Box::new(cmp(CmpOp::Gt, n("x"), c(0.5))),
                    Box::new(c(1.0)),
                    Box::new(c(0.0)),
                ),
                "the time x spends above 0.5",
            ),
        ],
        ..Default::default()
    };
    let mut bad = vec![];
    for (rtol, r) in run(&m, &[1e-5, 1e-6, 1e-8, 1e-9, 1e-10]) {
        let w = last(&r, "w");
        if (w - 1.375).abs() > 10.0 * rtol {
            bad.push(format!("rtol {rtol:e}: w(2) = {w} (exact 1.375)"));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// Where the event itself sends the condition back (its own discrete
/// value moves x, or a `reinit` the state), nothing is held: it fires at
/// each crossing.
#[test]
fn a_condition_its_event_sends_back_fires_at_every_crossing() {
    let up = cmp(CmpOp::Gt, n("x"), c(0.5));
    // x³ + x = z - 0.6 n: crossings at 0.625, 1.225, 1.825
    let stepped = counter("Test.ImplicitSteps", 1.0, true, n("z") - c(0.6) * n("n"), up.clone());
    // reinit(z, 0): crossings at 0.625, 1.25, 1.875
    let mut saw = counter("Test.ImplicitSaw", 1.0, true, n("z"), up.clone());
    saw.equations[1] = counting(up, vec![WhenAction::Reinit { var: "z".into(), value: c(0.0) }]);
    for (m, exact) in [(stepped, [0.625, 1.225, 1.825]), (saw, [0.625, 1.25, 1.875])] {
        for (rtol, r) in run(&m, &[1e-5, 1e-7, 1e-10]) {
            let at = fired(&r);
            assert_eq!(at.len(), 3, "{} at rtol {rtol:e}: {at:?}", m.name);
            for (t, e) in at.iter().zip(exact) {
                assert!((t - e).abs() < 10.0 * rtol, "{} at rtol {rtol:e}: {at:?}", m.name);
            }
        }
    }
}
