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
use lsim_ir::runtime::DiscreteBlock;
use lsim_ir::{ComponentDef, Equation, EquationDecl, VarDecl, WhenAction};
use lsim_solve::{
    Backend, BlockInfo, EventKind, OutputGrid, SimResult, SolverOptions, VarSource, simulate,
};

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
    run_on(top, false, Backend::Sundials, rtols)
}

/// [`run`] on IDA (`implicit`: every block kept implicit) or diffsol.
fn run_on(
    top: &ComponentDef,
    implicit: bool,
    backend: Backend,
    rtols: &[f64],
) -> Vec<(f64, SimResult)> {
    let built = common::build(&common::library(), top, implicit);
    let grid = OutputGrid { t0: 0.0, t_end: T_END, dt: 0.1 };
    rtols
        .iter()
        .map(|&rtol| {
            let opts = SolverOptions { rtol, atol: 1e-2 * rtol, backend, ..Default::default() };
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

/// `when cond then var = value` labelled `label`.
fn setting(cond: Expr, var: &str, value: f64, label: &str) -> EquationDecl {
    EquationDecl {
        eq: Equation::When {
            condition: cond,
            actions: vec![WhenAction::Assign { var: var.into(), value: c(value) }],
        },
        label: Some(label.into()),
    }
}

/// `x³ + x = rhs` (`z`, `w` integrated where `states` gives their starts:
/// z' = w, w' = `accel` k; else `rhs` reads time; the parameters a = 1/s
/// and k = 1/s² for its units) with complementary
/// conditions on x: `when x > 0.5` counts `n` ("rise"), `when x < 0.5`
/// counts `m` ("fall"), and `when x > 0.5 then on = 1; elsewhen x < 0.5
/// then on = 0` (lowered as Modelica's `elsewhen` is: one clause per
/// branch, the first branch last so that it wins where both fire).
fn complementary(name: &str, rhs: Expr, states: Option<(f64, f64, f64)>) -> ComponentDef {
    let up = || cmp(CmpOp::Gt, n("x"), c(0.5));
    let down = || cmp(CmpOp::Lt, n("x"), c(0.5));
    let mut vars = vec![
        x_var(),
        discrete("n", "1", 0.0, "the rises counted"),
        discrete("m", "1", 0.0, "the falls counted"),
        discrete("on", "1", 0.0, "x above 0.5, by the elsewhen"),
    ];
    let mut equations = vec![
        eq(cubed(n("x")) + n("x"), rhs, "x is solved from it"),
        EquationDecl {
            eq: Equation::When {
                condition: up(),
                actions: vec![WhenAction::Assign {
                    var: "n".into(),
                    value: call(Builtin::Pre, vec![n("n")]) + c(1.0),
                }],
            },
            label: Some("rise".into()),
        },
        EquationDecl {
            eq: Equation::When {
                condition: down(),
                actions: vec![WhenAction::Assign {
                    var: "m".into(),
                    value: call(Builtin::Pre, vec![n("m")]) + c(1.0),
                }],
            },
            label: Some("fall".into()),
        },
        setting(down(), "on", 0.0, "elsewhen x < 0.5"),
        setting(up(), "on", 1.0, "when x > 0.5"),
    ];
    if let Some((z0, w0, accel)) = states {
        vars.push(state("z", "1", z0, "what x is solved from"));
        vars.push(state("w", "1/s", w0, "z's rate"));
        equations.push(eq(der("z"), n("w"), "z moves with w"));
        equations.push(eq(der("w"), c(accel) * n("k"), "w's rate"));
    }
    ComponentDef {
        name: name.into(),
        params: vec![param("a", "1/s", 1.0, "a rate"), param("k", "1/s2", 1.0, "a curvature")],
        vars,
        equations,
        ..Default::default()
    }
}

/// The complementary conditions' runs against their exact crossings: x
/// rises through 0.5 at `rise` (and falls back at `fall`, where given), x'
/// there `slope_rise` (`slope_fall`). Each clause fires once per crossing
/// in its direction, located to 10 rtol in x (that over |x'| in time), and
/// `on` is 1 at every output point between the crossings and 0 elsewhere.
fn complementary_bad(
    what: &str,
    runs: &[(f64, SimResult)],
    rise: (f64, f64),
    fall: Option<(f64, f64)>,
) -> Vec<String> {
    let mut bad = vec![];
    for (rtol, r) in runs {
        let at = |label: &str| -> Vec<f64> {
            r.events.iter().filter(|e| e.label.ends_with(label)).map(|e| e.t).collect()
        };
        let (rises, falls) = (at("rise"), at("fall"));
        let (n_end, m_end) = (last(r, "n"), last(r, "m"));
        let near = |got: &[f64], (exact, slope): (f64, f64)| {
            got.len() == 1 && (got[0] - exact).abs() <= 10.0 * rtol / slope.abs()
        };
        let falls_ok = match fall {
            Some(f) => near(&falls, f) && m_end == 1.0,
            None => falls.is_empty() && m_end == 0.0,
        };
        if !(near(&rises, rise) && n_end == 1.0 && falls_ok) {
            bad.push(format!(
                "{what}, rtol {rtol:e}: n = {n_end}, m = {m_end}, rises at {rises:?}, falls at \
                 {falls:?} (exact {rise:?}, {fall:?})"
            ));
        }
        let k = r.names.iter().position(|x| x == "on").expect("on");
        let wrong: Vec<f64> = r
            .times
            .iter()
            .zip(&r.values[k])
            .filter(|&(&t, &v)| {
                // (the points next to a crossing: either side)
                let close = |e: f64| (t - e).abs() <= 1e-6;
                let inside = t > rise.0 && fall.is_none_or(|f| t < f.0);
                !close(rise.0) && !fall.is_some_and(|f| close(f.0)) && v != f64::from(inside)
            })
            .map(|(&t, _)| t)
            .collect();
        if !wrong.is_empty() {
            bad.push(format!("{what}, rtol {rtol:e}: `on` wrong at t = {wrong:?}"));
        }
    }
    bad
}

const ALL_RTOLS: [f64; 7] = [1e-4, 1e-5, 1e-6, 1e-7, 1e-8, 1e-9, 1e-10];

/// `when x > 0.5`, `when x < 0.5` and `when x > 0.5 … elsewhen x < 0.5` on
/// one iteration variable: at its crossing the integrator reports the
/// rising functions; the event solves x again from the states, and where
/// the crossing was located a little early x comes out a little short of
/// 0.5. Event iteration evaluated the falling conditions there and fired
/// them too: `m = 1` and, the elsewhen's second branch firing last, `on =
/// 0` for the rest of the run while x > 0.5 (4 of 14 runs with x³ + x = t
/// and with x³ + x = z, z' = 1: the outside review's probes). Now event
/// iteration holds each condition on the side the integrator saw it on,
/// within the hold the integrator gets after the event.
#[test]
fn complementary_whens_on_an_iteration_variable_fire_once_per_crossing() {
    let mut bad = vec![];
    // x reaches 0.5 where x³ + x = 0.625: x' = rate / (3 x² + 1) = rate / 1.75
    let timed = complementary("Test.PairTime", n("a") * Expr::Time, None);
    bad.extend(complementary_bad("no state", &run(&timed, &ALL_RTOLS), (0.625, 1.0 / 1.75), None));
    let ramp = complementary("Test.PairRamp", n("z"), Some((0.0, 1.0, 0.0)));
    bad.extend(complementary_bad("z' = 1", &run(&ramp, &ALL_RTOLS), (0.625, 1.0 / 1.75), None));
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// The same clauses on a peaked x (x³ + x = 0.625 + δ − (t − 1)², with t
/// or with a state z, z'' = −2): x rises through 0.5 at 1 − √δ and falls
/// back at 1 + √δ; each crossing fires its own direction's clauses only.
/// Before, the event at the rise fired "fall" too, or the one at the fall
/// "rise" (38 of 42 runs over δ = 1e-2, 1e-3, 1e-4).
#[test]
fn complementary_whens_on_a_peaked_iteration_variable_fire_once_each_way() {
    let mut bad = vec![];
    for delta in [1e-2, 1e-3] {
        let (r, f) = (1.0 - lsim_ir::math::sqrt(delta), 1.0 + lsim_ir::math::sqrt(delta));
        // x' = −2 (t − 1) / 1.75 at the crossings
        let slope = 2.0 * lsim_ir::math::sqrt(delta) / 1.75;
        let s = Expr::Time - c(1.0) / n("a");
        let timed = complementary("Test.PeakTime", c(0.625 + delta) - n("k") * s.clone() * s, None);
        let runs = run(&timed, &ALL_RTOLS);
        bad.extend(complementary_bad(
            &format!("no state, δ = {delta:e}"),
            &runs,
            (r, slope),
            Some((f, -slope)),
        ));
        let peak = complementary("Test.PeakState", n("z"), Some((0.625 + delta - 1.0, 2.0, -2.0)));
        bad.extend(complementary_bad(
            &format!("z'' = −2, δ = {delta:e}"),
            &run(&peak, &ALL_RTOLS),
            (r, slope),
            Some((f, -slope)),
        ));
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// A hold already in force at a later event: the rise located a little
/// early is held on its side (x, solved again, a little short of 0.5);
/// at 0.625 s, before the integrator's next step ends, a time event sets
/// `k`, which moves x up by 6e-13 (x³ + x = a t + 1e-12 k). Event
/// iteration saw x short of 0.5 before that event and over it after, and
/// fired the rise a second time; it now sees the condition as the
/// integrator does, held on the side it crossed to: one crossing, one
/// firing.
#[test]
fn a_held_condition_does_not_fire_again_at_the_next_event() {
    let up = cmp(CmpOp::Gt, n("x"), c(0.5));
    let at = cmp(CmpOp::Ge, Expr::Time, n("t1"));
    let mut bad = vec![];
    for with_state in [false, true] {
        let rhs = if with_state { n("z") } else { n("a") * Expr::Time } + n("e") * n("k");
        let mut m = counter("Test.HeldNextEvent", 1.0, with_state, rhs, up.clone());
        m.params.push(param("e", "1", 1e-12, "how far k moves x³ + x"));
        m.params.push(param("t1", "s", 0.625, "when k is set"));
        m.vars.push(discrete("k", "1", 0.0, "set at t1"));
        m.equations.push(setting(at.clone(), "k", 1.0, "k set"));
        let what = if with_state { "z = t" } else { "no state" };
        for (rtol, r) in run(&m, &ALL_RTOLS) {
            let at: Vec<f64> =
                r.events.iter().filter(|e| e.label.ends_with("it counts")).map(|e| e.t).collect();
            let count = last(&r, "n");
            if count != 1.0 || at.len() != 1 || (at[0] - 0.625).abs() > 10.0 * rtol {
                bad.push(format!("{what}, rtol {rtol:e}: n = {count}, fired at {at:?}"));
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// `x³ + x = rhs` counting `cond`'s crossings, with `k` set to 1 at `t1 =
/// 1 s`: by a time event, or where `crossing` where a state `s`, s' = 1,
/// passes `t1` (another function's crossing); `z` integrated at `z_rate`
/// where given.
fn moved(name: &str, rhs: Expr, cond: Expr, crossing: bool, z_rate: Option<Expr>) -> ComponentDef {
    let mut m = counter(name, 1.0, false, rhs, cond);
    m.params.push(param("t1", "s", 1.0, "when k is set"));
    m.vars.push(discrete("k", "1", 0.0, "set at t1"));
    let trigger = if crossing {
        m.vars.push(state("s", "s", 0.0, "a clock"));
        m.equations.push(eq(der("s"), c(1.0), "the clock's rate"));
        cmp(CmpOp::Gt, n("s"), n("t1"))
    } else {
        cmp(CmpOp::Ge, Expr::Time, n("t1"))
    };
    m.equations.push(setting(trigger, "k", 1.0, "k set"));
    if let Some(r) = z_rate {
        m.vars.push(state("z", "1", 0.0, "what x is solved from, besides"));
        m.equations.push(eq(der("z"), r, "its rate"));
    }
    m
}

/// An event whose own discrete change moves a condition on an iteration
/// variable across its threshold fires the condition's `when` there,
/// however little it moves it (x³ + x = 0.625 − e + 2 e k: x across 0.5 by
/// e/1.75, e = 1e-12 … 1e-5; x³ + x = 0.5 + (0.125 + e) k: x from 0.41 to
/// 0.5 + e/1.75, onto 0.5 itself where e = 0, `when x >= 0.5`), with k set
/// by a time event or at another function's crossing, x constant after it
/// (or rising on, 0.1 a k: the `when` fires once, not again where x leaves
/// what would have been its hold), on CVODE, IDA and diffsol, with a state
/// and without (IDA). Event iteration and the integrator held such a
/// condition on the side it was seen on where it landed within its hold:
/// the `when` never fired while x stayed there (116 of 273 runs of an
/// outside recheck's probes), or fired late, where x left the hold, or at
/// the event and again there. Only a crossing that the integrator's own
/// inconsistency located a little early is held.
#[test]
fn a_condition_its_event_moves_across_fires_there() {
    let up = || cmp(CmpOp::Gt, n("x"), c(0.5));
    let reached = || cmp(CmpOp::Ge, n("x"), c(0.5));
    let z = || n("z");
    let ramp = || c(0.1) * n("a") * n("k") * (Expr::Time - n("t1"));
    let mut models: Vec<(String, ComponentDef, bool)> = vec![];
    for e in [1e-12, 1e-9, 1e-7, 1e-5] {
        let nudge = || c(0.625) - c(e) + c(2.0 * e) * n("k");
        models.push((
            format!("nudge {e:e}"),
            moved("Test.Nudge", nudge(), up(), false, None),
            false,
        ));
        let with_z = moved("Test.NudgeZ", nudge() + z(), up(), false, Some(c(0.0)));
        models.push((format!("nudge {e:e}, z' = 0"), with_z, true));
    }
    for e in [0.0, 1e-9, 1e-6] {
        let land = || c(0.5) + c(0.125 + e) * n("k");
        models.push((
            format!("land {e:e}"),
            moved("Test.Land", land(), reached(), false, None),
            false,
        ));
        let with_z = moved("Test.LandZ", land() + z(), reached(), false, Some(c(0.0)));
        models.push((format!("land {e:e}, z' = 0"), with_z, true));
        let on = moved("Test.LandOn", land() + ramp(), reached(), false, None);
        models.push((format!("land {e:e}, rising on"), on, false));
        let on_z =
            moved("Test.LandOnZ", land() + c(0.1) * z(), reached(), false, Some(n("a") * n("k")));
        models.push((format!("land {e:e}, rising on with z"), on_z, true));
    }
    for e in [1e-9, 1e-6] {
        let other = moved("Test.Other", c(0.625) - c(e) + c(2.0 * e) * n("k"), up(), true, None);
        models.push((format!("other {e:e}"), other, true));
    }
    let mut bad = vec![];
    for (what, m, with_state) in &models {
        let crossing = what.starts_with("other");
        let mut on = vec![("", run_on(m, false, Backend::Sundials, &ALL_RTOLS))];
        if *with_state {
            on.push((" on IDA", run_on(m, true, Backend::Sundials, &ALL_RTOLS)));
            on.push((" on diffsol", run_on(m, false, Backend::Diffsol, &ALL_RTOLS)));
        }
        for (backend, runs) in on {
            for (rtol, r) in runs {
                let at: Vec<f64> = r
                    .events
                    .iter()
                    .filter(|e| e.label.ends_with("it counts"))
                    .map(|e| e.t)
                    .collect();
                let count = last(&r, "n");
                let near = if crossing { 10.0 * rtol } else { 0.0 };
                if count != 1.0 || at.len() != 1 || (at[0] - 1.0).abs() > near {
                    bad.push(format!(
                        "{what}{backend}, rtol {rtol:e}: n = {count}, fired at {at:?}"
                    ));
                }
            }
        }
    }
    assert!(bad.is_empty(), "{} wrong:\n{}", bad.len(), bad.join("\n"));
}

/// A `when x > 0.5` beside a mode on the same relation (`y = if x > 0.5
/// then 1 else 0`): the crossing located a little early fires the `when`
/// and holds it on its side; the mode, set back by its relation as x is
/// solved again, restarts the integrator, which meets the crossing again
/// where x is exactly 0.5 (x³ + x = t at t = 0.625: an exact zero of the
/// `when`'s function, which `x > 0.5` is false at). The hold was dropped
/// there, the `when`'s side taken as its zero's, and it fired again as x
/// went on past 0.5 (rate 1, no state, rtol 1e-10: at 0.6249999999967 and
/// 0.625000000000008, an outside recheck's probe); a hold now keeps an
/// exact zero on its side, as a release does.
#[test]
fn a_when_beside_a_mode_on_its_relation_fires_once() {
    let up = || cmp(CmpOp::Gt, n("x"), c(0.5));
    let mut bad = vec![];
    for a in [0.7, 1.0, 2.3, 4.4] {
        for with_state in [false, true] {
            let rhs = if with_state { n("z") } else { n("a") * Expr::Time };
            let mut m = counter("Test.WhenBesideMode", a, with_state, rhs, up());
            m.vars.push(var("y", "1", "1 above 0.5, by a mode"));
            let mode = Expr::If(Box::new(up()), Box::new(c(1.0)), Box::new(c(0.0)));
            m.equations.push(eq(n("y"), mode, "the mode on x > 0.5"));
            let what = if with_state { format!("z = {a} t") } else { format!("no state, {a} t") };
            for (rtol, r) in run(&m, &ALL_RTOLS) {
                let at: Vec<f64> = r
                    .events
                    .iter()
                    .filter(|e| e.label.ends_with("it counts"))
                    .map(|e| e.t)
                    .collect();
                let count = last(&r, "n");
                // (x' = a / 1.75 at the crossing)
                let near = 10.0 * rtol * 1.75 / a;
                if count != 1.0 || at.len() != 1 || (at[0] - 0.625 / a).abs() > near {
                    bad.push(format!("{what}, rtol {rtol:e}: n = {count}, fired at {at:?}"));
                }
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// What sets `k` at t2 in [`back_across`].
#[derive(Clone, Copy, PartialEq, Debug)]
enum Trigger {
    /// a time event
    Time,
    /// a state `s`, s' = 1, crossing t2
    State,
    /// a sampled block's tick, inside one of the integrator's steps
    Tick,
}

/// `x³ + x = r − k (r − 0.625 + e) + k2 (0.1 + e)`, `r = a t` or a state
/// `z` (z' = a) where `with_state`: x rises through 0.5 at t1 = 0.625 / a;
/// k, set at t2 = t1 + δ by `trigger`, puts it back under 0.5 by e / 1.75
/// and keeps it there, until k2, set at tb = t1 + 0.5 by a time event,
/// lifts it 0.1 / 1.75 over again. `when x > 0.5` counts n ("rise"), `when
/// x < 0.5` m ("fall").
fn back_across(a: f64, delta: f64, e: f64, trigger: Trigger, with_state: bool) -> ComponentDef {
    let t1 = 0.625 / a;
    let mut m = complementary(
        "Test.BackAcross",
        if with_state { n("z") } else { n("a") * Expr::Time },
        None,
    );
    let r = || if with_state { n("z") } else { n("a") * Expr::Time };
    m.equations[0] = eq(
        cubed(n("x")) + n("x"),
        r() - n("k") * (r() - c(0.625) + n("e")) + n("k2") * (c(0.1) + n("e")),
        "x is solved from it",
    );
    // (the rise and the fall alone)
    m.equations.truncate(3);
    m.vars.retain(|v| v.name != "on");
    m.params = vec![
        param("a", "1/s", a, "the rate"),
        param("e", "1", e, "how far k puts x³ + x back"),
        param("t2", "s", t1 + delta, "when k is set"),
        param("tb", "s", t1 + 0.5, "when k2 is set"),
        param("never", "s", 1e3, "after the run"),
    ];
    m.vars.push(discrete("k", "1", 0.0, "set at t2"));
    m.vars.push(discrete("k2", "1", 0.0, "set at tb"));
    if with_state {
        m.vars.push(state("z", "1", 0.0, "the ramp"));
        m.equations.push(eq(der("z"), n("a"), "the ramp's rate"));
    }
    let at_t2 = match trigger {
        Trigger::Time => cmp(CmpOp::Ge, Expr::Time, n("t2")),
        Trigger::State => {
            m.vars.push(state("s", "s", 0.0, "a clock"));
            m.equations.push(eq(der("s"), c(1.0), "the clock's rate"));
            cmp(CmpOp::Gt, n("s"), n("t2"))
        }
        // (the block sets k; the clause only declares it set)
        Trigger::Tick => cmp(CmpOp::Ge, Expr::Time, n("never")),
    };
    m.equations.push(setting(at_t2, "k", 1.0, "k set"));
    m.equations.push(setting(cmp(CmpOp::Ge, Expr::Time, n("tb")), "k2", 1.0, "k2 set"));
    m
}

/// One tick at `at` that sets its output to 1.
struct TickAt(f64);

impl DiscreteBlock for TickAt {
    fn name(&self) -> &str {
        "'Tick'"
    }
    fn period(&self) -> f64 {
        1e3
    }
    fn offset(&self) -> f64 {
        self.0
    }
    fn init(&mut self, _t: f64, _i: &[f64], _o: &mut [f64]) -> Result<(), String> {
        Ok(())
    }
    fn tick(&mut self, _t: f64, _i: &[f64], o: &mut [f64]) -> Result<(), String> {
        o[0] = 1.0;
        Ok(())
    }
}

/// [`back_across`]'s runs (where `trigger` is a tick, a block at t2 sets
/// k), and what is wrong in them: rises at t1 (located to 10 rtol in x)
/// and tb (exact), a fall at t2 (exact; to 10 rtol for a state crossing).
/// Where δ is shorter than the rise's location tolerance the integrator
/// may locate the rise after t2, past the event that undoes it: then
/// neither fires (the rise at tb alone), the known limit of two crossings
/// closer than the tolerance.
#[allow(clippy::too_many_arguments)]
fn back_across_bad(
    what: &str,
    a: f64,
    delta: f64,
    e_of: impl Fn(f64) -> f64,
    trigger: Trigger,
    with_state: bool,
    implicit: bool,
    backend: Backend,
) -> Vec<String> {
    let (t1, t2, tb) = (0.625 / a, 0.625 / a + delta, 0.625 / a + 0.5);
    let mut bad = vec![];
    for rtol in ALL_RTOLS {
        let m = back_across(a, delta, e_of(rtol), trigger, with_state);
        let mut built = common::build(&common::library(), &m, implicit);
        let mut blocks: Vec<Box<dyn DiscreteBlock>> = vec![];
        if trigger == Trigger::Tick {
            let p = built.info.var_names.iter().position(|v| v == "k").expect("k");
            let VarSource::D(k) = built.info.var_sources[p] else { panic!("k is discrete") };
            built.info.blocks = vec![BlockInfo {
                name: "'Tick'".into(),
                inputs: vec![],
                outputs: vec![k],
                period: 1e3,
                chains: vec![],
            }];
            blocks.push(Box::new(TickAt(t2)));
        }
        let grid = OutputGrid { t0: 0.0, t_end: T_END, dt: 0.1 };
        let opts = SolverOptions { rtol, atol: 1e-2 * rtol, backend, ..Default::default() };
        let r = simulate(&built.jit, &built.info, &opts, grid, &mut blocks)
            .unwrap_or_else(|e| panic!("{what} at rtol {rtol:e}: {e}"));
        let at = |label: &str| -> Vec<f64> {
            r.events.iter().filter(|e| e.label.ends_with(label)).map(|e| e.t).collect()
        };
        let (rises, falls) = (at("rise"), at("fall"));
        let (n_end, m_end) = (last(&r, "n"), last(&r, "m"));
        let loc = 10.0 * rtol * 1.75 / a;
        let fall_tol = if trigger == Trigger::State { 10.0 * rtol } else { 1e-12 };
        let right = rises.len() == 2
            && (rises[0] - t1).abs() <= loc
            && (rises[1] - tb).abs() <= 1e-12
            && falls.len() == 1
            && (falls[0] - t2).abs() <= fall_tol
            && (n_end, m_end) == (2.0, 1.0);
        let limit = delta < loc
            && rises.len() == 1
            && (rises[0] - tb).abs() <= 1e-12
            && falls.is_empty()
            && (n_end, m_end) == (1.0, 0.0);
        if !right && !limit {
            bad.push(format!(
                "{what}, rtol {rtol:e}: n = {n_end}, m = {m_end}, rises at {rises:?}, falls at \
                 {falls:?} (exact {t1}, {tb}; {t2})"
            ));
        }
    }
    bad
}

/// A hold made at a crossing located a little early, and an event before
/// the integrator's next step end that puts the condition back across
/// (k set by a time event, at a state's crossing, or by a sampled block's
/// tick inside a step: x from 0.5 + a δ / 1.75 to 0.5 − 5.7e-13). The
/// hold was never released at an event's point: event iteration and the
/// integrator kept x on the side it rose to, `when x < 0.5` never fired,
/// nor `when x > 0.5` where k2 lifts x again (87 of an outside recheck's
/// 515 IDA runs, 4e3afdd right in all of them; 103 of these 378 at
/// f29ed27). A hold now stands at an event's point only where the
/// integrator's solution there is still short of its side within the
/// hold, as at a step's end.
#[test]
fn a_hold_from_an_earlier_crossing_lets_an_event_put_it_back() {
    let mut bad = vec![];
    for trigger in [Trigger::Time, Trigger::State, Trigger::Tick] {
        for (with_state, implicit, how) in
            [(true, false, "z' = a"), (true, true, "z' = a, IDA"), (false, false, "no state")]
        {
            for a in [0.7, 2.3, 4.4] {
                for delta in [1e-6, 1e-4] {
                    let what = format!("{trigger:?}, {how}, a = {a}, δ = {delta:e}");
                    bad.extend(back_across_bad(
                        &what,
                        a,
                        delta,
                        |_| 1e-12,
                        trigger,
                        with_state,
                        implicit,
                        Backend::Sundials,
                    ));
                }
            }
        }
    }
    assert!(bad.is_empty(), "{} wrong:\n{}", bad.len(), bad.join("\n"));
}

/// The same on diffsol, with x put back by its tolerance (e = 1.75 rtol:
/// half the hold; 5.7e-13 is below the noise of diffsol's iteration
/// variable). Diffsol's x lags its equation by a tenth to a quarter of its
/// tolerance: at an event 1e-6 s after the crossing it can still be short
/// of 0.5, a hold standing there although x, solved for the discrete
/// values before the event, is past it; carried over, the hold kept x on
/// the side it rose to (`when x < 0.5` never fired; 8 of these 56 runs at
/// f29ed27). A hold standing at an event's point that the event moves
/// across (x solved for the discrete values before the event on its held
/// side already) holds nothing after it, as a new one would not.
#[test]
fn on_diffsol_an_event_that_puts_a_held_condition_back_fires_there() {
    let mut bad = vec![];
    for a in [0.7, 1.0, 2.3, 4.4] {
        for delta in [1e-6, 1e-4] {
            let what = format!("diffsol, a = {a}, δ = {delta:e}");
            bad.extend(back_across_bad(
                &what,
                a,
                delta,
                |rtol| 1.75 * rtol,
                Trigger::Time,
                true,
                false,
                Backend::Diffsol,
            ));
        }
    }
    assert!(bad.is_empty(), "{} wrong:\n{}", bad.len(), bad.join("\n"));
}

/// `x³ + x = 1.25 − z` (z' = a: x falls through 0.5 at 0.625 / a, `when x
/// < 0.5` counts n) beside `q³ + q = 1 + k`, k set where a state s = t
/// passes 1e-6 s later (nothing x reads moves). On diffsol, whose x lags,
/// the integrator had x still over 0.5 at that event, and the event, x
/// solved again past it, held x there (the crossing located early, by the
/// look of it); the hold ended at a step's end where x had gone past it,
/// the integrator, told it was held, reporting no crossing: n stayed 0 (4
/// of these 28 runs at f29ed27, at rtol 1e-4 and 1e-5). A condition past
/// its hold crossed: where the integrator did not report it, the run loop
/// does.
#[test]
fn a_condition_past_its_hold_fires_where_the_integrator_did_not_report_it() {
    let mut bad = vec![];
    for a in [0.7, 1.0, 2.3, 4.4] {
        let t1 = 0.625 / a;
        let down = cmp(CmpOp::Lt, n("x"), c(0.5));
        let mut m = counter("Test.PastItsHold", a, true, c(1.25) - n("z"), down);
        m.vars[0].start = Some(c(0.78));
        m.params.push(param("t2", "s", t1 + 1e-6, "when k is set"));
        m.vars.push(discrete("k", "1", 0.0, "set at t2"));
        m.vars.push(VarDecl { start: Some(c(0.68)), ..var("q", "1", "solved beside x") });
        m.equations.push(eq(cubed(n("q")) + n("q"), c(1.0) + n("k"), "q reads k"));
        m.vars.push(state("s", "s", 0.0, "a clock"));
        m.equations.push(eq(der("s"), c(1.0), "the clock's rate"));
        m.equations.push(setting(cmp(CmpOp::Gt, n("s"), n("t2")), "k", 1.0, "k set"));
        for (rtol, r) in run_on(&m, false, Backend::Diffsol, &ALL_RTOLS) {
            let at: Vec<f64> =
                r.events.iter().filter(|e| e.label.ends_with("it counts")).map(|e| e.t).collect();
            let count = last(&r, "n");
            if count != 1.0 || at.len() != 1 || (at[0] - t1).abs() > 10.0 * rtol * 1.75 / a {
                bad.push(format!("a = {a}, rtol {rtol:e}: n = {count}, fired at {at:?}"));
            }
        }
    }
    assert!(bad.is_empty(), "{} wrong:\n{}", bad.len(), bad.join("\n"));
}

// ---------------------------------------------------------------------------
// The hold rule (round 4). `engine/judge/holds.py` has every shape whole, at
// every rate, tolerance and backend; these are a sample of each finding.
// ---------------------------------------------------------------------------

/// Every relative tolerance the hold rule's samples run at.
const HOLD_RTOLS: [f64; 4] = [1e-4, 1e-6, 1e-8, 1e-10];

/// How a holds-suite shape is integrated: x on an iteration variable (x³ + x
/// = R) with r a state z, the same with every block kept implicit, with r =
/// a t (no state), on diffsol, or x written explicitly with r = a t (no
/// state, no iteration variable: the stateless backend).
#[derive(Clone, Copy, PartialEq, Debug)]
enum Config {
    Ida,
    IdaImplicit,
    NoState,
    Diffsol,
    Stateless,
}

impl Config {
    fn state(self) -> bool {
        !matches!(self, Config::NoState | Config::Stateless)
    }
}

/// A shape of the holds suite: R = 0.625 + s (dd + `phi`), dd = r − 0.625
/// (s = 1 rising, −1 falling; r = z, z' = `z_rate`, or a t), x³ + x = R (or x
/// = 0.5 + (R − 0.625) / 1.75), so x crosses 0.5 at t1 = 0.625 / a while
/// `phi` is 0. `when x > 0.5` ("forth", `<` falling; `>=`, `<=` where not
/// strict) counts n and does `forth`; `when x < 0.5` ("back") counts m and
/// does `back`; `more` are further clauses. `params` beside a; `discretes`
/// beside n and m.
#[allow(clippy::too_many_arguments)]
fn holds_shape(
    up: bool,
    strict: bool,
    config: Config,
    phi: &dyn Fn(&dyn Fn() -> Expr) -> Expr,
    z_rate: Expr,
    forth: Vec<WhenAction>,
    back: Vec<WhenAction>,
    more: Vec<EquationDecl>,
    params: &[(&str, &str, f64)],
    discretes: &[(&str, &str)],
) -> ComponentDef {
    let with_state = config.state();
    let r = || if with_state { n("z") } else { n("a") * Expr::Time };
    let dd = || r() - c(0.625);
    let s = if up { 1.0 } else { -1.0 };
    let rr = c(0.625) + c(s) * (dd() + phi(&dd));
    let x_eq = if config == Config::Stateless {
        eq(n("x"), c(0.5) + (rr - c(0.625)) / c(1.75), "x follows R")
    } else {
        eq(cubed(n("x")) + n("x"), rr, "x is solved from it")
    };
    let (fop, bop) = match (up, strict) {
        (true, true) => (CmpOp::Gt, CmpOp::Lt),
        (true, false) => (CmpOp::Ge, CmpOp::Le),
        (false, true) => (CmpOp::Lt, CmpOp::Gt),
        (false, false) => (CmpOp::Le, CmpOp::Ge),
    };
    let clause = |op: CmpOp, counter: &str, more: Vec<WhenAction>, label: &str| {
        let mut actions = vec![WhenAction::Assign {
            var: counter.into(),
            value: call(Builtin::Pre, vec![n(counter)]) + c(1.0),
        }];
        actions.extend(more);
        EquationDecl {
            eq: Equation::When { condition: cmp(op, n("x"), c(0.5)), actions },
            label: Some(label.into()),
        }
    };
    let mut vars = vec![
        VarDecl { start: Some(c(if up { 0.0 } else { 0.78 })), ..var("x", "1", "the condition's") },
        discrete("n", "1", 0.0, "forth counted"),
        discrete("m", "1", 0.0, "back counted"),
    ];
    for (d, unit) in discretes {
        vars.push(discrete(d, unit, 0.0, "a discrete value"));
    }
    let mut equations =
        vec![x_eq, clause(fop, "n", forth, "forth"), clause(bop, "m", back, "back")];
    equations.extend(more);
    if with_state {
        vars.push(state("z", "1", 0.0, "the ramp"));
        equations.push(eq(der("z"), z_rate, "the ramp's rate"));
    }
    let mut ps = vec![param("a", "1/s", 1.0, "the rate")];
    for (p, unit, v) in params {
        ps.push(param(p, unit, *v, "a parameter"));
    }
    ComponentDef {
        name: "Test.HoldsShape".into(),
        params: ps,
        vars,
        equations,
        ..Default::default()
    }
}

/// `when cond then var = value` with a value of any expression.
fn assign_when(cond: Expr, var: &str, value: Expr, label: &str) -> EquationDecl {
    EquationDecl {
        eq: Equation::When {
            condition: cond,
            actions: vec![WhenAction::Assign { var: var.into(), value }],
        },
        label: Some(label.into()),
    }
}

/// `var = value`, as a clause's action.
fn set(var: &str, value: Expr) -> WhenAction {
    WhenAction::Assign { var: var.into(), value }
}

/// A built shape, run at its parameters `ps` (beside the defaults) and
/// `rtol` on `config`'s backend: the times forth and back fired, and n and
/// m at the end; or the run's error.
struct HoldsRun {
    forth: Vec<f64>,
    back: Vec<f64>,
    n: f64,
    m: f64,
}

fn holds_run(
    built: &common::Built,
    config: Config,
    ps: &[(&str, f64)],
    rtol: f64,
) -> Result<HoldsRun, String> {
    let mut info = built.info.clone();
    for (name, v) in ps {
        let k =
            built.prepared.flat.params.iter().position(|p| p.name == *name).expect("a parameter");
        info.params[k] = *v;
    }
    let backend = if config == Config::Diffsol { Backend::Diffsol } else { Backend::Sundials };
    let opts = SolverOptions { rtol, atol: 1e-2 * rtol, backend, ..Default::default() };
    let grid = OutputGrid { t0: 0.0, t_end: T_END, dt: 0.1 };
    let r = simulate(&built.jit, &info, &opts, grid, &mut []).map_err(|e| e.to_string())?;
    let at = |label: &str| -> Vec<f64> {
        r.events
            .iter()
            .filter(|e| matches!(e.kind, EventKind::When(_)) && e.label.ends_with(label))
            .map(|e| e.t)
            .collect()
    };
    Ok(HoldsRun { forth: at("forth"), back: at("back"), n: last(&r, "n"), m: last(&r, "m") })
}

/// A shape built on `config` (every block implicit for [`Config::IdaImplicit`]).
fn holds_build(m: &ComponentDef, config: Config) -> common::Built {
    common::build(&common::library(), m, config == Config::IdaImplicit)
}

/// Whether `got` fired at the times `want` gives, each within its tolerance.
fn at_times(got: &[f64], want: &[(f64, f64)]) -> bool {
    got.len() == want.len() && got.iter().zip(want).all(|(g, (t, tol))| (g - t).abs() <= *tol)
}

/// The crossing's location tolerance at a, rtol: 10 rtol in x over |x'|.
fn loc(a: f64, rtol: f64) -> f64 {
    10.0 * rtol * 1.75 / a + 1e-12
}

/// The crossing's own event, or a sibling `when` on its relation, or a
/// `reinit`, puts the condition back by ex = 0.5 and 1.5 rtol (recheck 3's
/// own-move probe): R = 0.625 + s (dd − k (dd + E) + k2 (0.1 + E)), E = 1.75
/// ex, k set by forth (own) or by another `when x > 0.5` (sep); or forth
/// reinits z to 0.625 − E and stops it (reinit). After k, x is back across
/// by ex: event iteration fires back at that same instant, and forth fires
/// again at tb = t1 + 0.5, where k2 lifts R by 0.1 + E (Modelica's event
/// iteration; recheck 3's truth). Where the crossing was located a little
/// early the hold hid the event's own move (ex is less than the hold's
/// width): neither fired (IDA: 1 616 of recheck 3's 4 480 runs at
/// 4d24528). The hold rule reads the event's change where the crossing
/// really is, and holds only the location error.
#[test]
fn an_event_at_its_crossing_that_puts_the_condition_back_fires_back_there() {
    let mut bad = vec![];
    let mut runs = 0;
    for kind in ["own", "sep", "reinit"] {
        for config in [Config::Ida, Config::IdaImplicit, Config::NoState] {
            if kind == "reinit" && !config.state() {
                continue;
            }
            for up in [true, false] {
                let e = || c(1.75) * n("ex");
                let phi = |dd: &dyn Fn() -> Expr| {
                    if kind == "reinit" {
                        n("k2") * (c(0.1) + e())
                    } else {
                        c(0.0) - n("k") * (dd() + e()) + n("k2") * (c(0.1) + e())
                    }
                };
                let rate = if kind == "reinit" { n("a") * (c(1.0) - n("k")) } else { n("a") };
                let mut forth = vec![];
                let mut more =
                    vec![assign_when(cmp(CmpOp::Ge, Expr::Time, n("tb")), "k2", c(1.0), "k2 set")];
                match kind {
                    "own" => forth.push(set("k", c(1.0))),
                    "sep" => more.push(assign_when(
                        cmp(if up { CmpOp::Gt } else { CmpOp::Lt }, n("x"), c(0.5)),
                        "k",
                        c(1.0),
                        "k set",
                    )),
                    _ => {
                        forth.push(set("k", c(1.0)));
                        forth.push(WhenAction::Reinit { var: "z".into(), value: c(0.625) - e() });
                    }
                }
                let m = holds_shape(
                    up,
                    true,
                    config,
                    &phi,
                    rate,
                    forth,
                    vec![],
                    more,
                    &[("tb", "s", 1.0), ("ex", "1", 1e-6)],
                    &[("k", "1"), ("k2", "1")],
                );
                let built = holds_build(&m, config);
                for a in [0.7, 2.3] {
                    for exk in [0.5, 1.5] {
                        for rtol in HOLD_RTOLS {
                            let (t1, tb) = (0.625 / a, 0.625 / a + 0.5);
                            let ps = [("a", a), ("tb", tb), ("ex", exk * rtol)];
                            runs += 1;
                            let what =
                                format!("{kind} {config:?} up {up} a {a} ex {exk} rtol {rtol:e}");
                            match holds_run(&built, config, &ps, rtol) {
                                Ok(r) => {
                                    let right =
                                        at_times(&r.forth, &[(t1, loc(a, rtol)), (tb, 1e-12)])
                                            && r.back.len() == 1
                                            && r.back[0] == r.forth[0]
                                            && (r.n, r.m) == (2.0, 1.0);
                                    if !right {
                                        bad.push(format!(
                                            "{what}: forth {:?}, back {:?} (t1 {t1}, tb {tb})",
                                            r.forth, r.back
                                        ));
                                    }
                                }
                                Err(e) => bad.push(format!("{what}: {e}")),
                            }
                        }
                    }
                }
            }
        }
    }
    assert!(bad.is_empty(), "{} of {runs} wrong:\n{}", bad.len(), bad.join("\n"));
}

/// The same with the move far beyond any hold (ex = 100 rtol), on diffsol and
/// on the stateless backend (x explicit, no state: the run loop crosses the
/// time crossing exactly). Their crossings are reported in either direction
/// (diffsol classifies every root function; the run loop reports a time
/// crossing in its own direction), and a reported crossing marked every
/// clause on its function decided: the back clause, its function crossing
/// the other way, was never checked in round 0, and back never fired (every
/// such run at every commit since 36c8a0d8: recheck 3's RC3-B2). A report
/// decides only the clauses it fires.
#[test]
fn on_diffsol_and_the_stateless_backend_an_event_that_puts_its_crossing_back_fires_back() {
    let mut bad = vec![];
    let mut runs = 0;
    for kind in ["own", "sep"] {
        for config in [Config::Diffsol, Config::Stateless] {
            for up in [true, false] {
                let e = || c(1.75) * n("ex");
                let phi = |dd: &dyn Fn() -> Expr| {
                    c(0.0) - n("k") * (dd() + e()) + n("k2") * (c(0.1) + e())
                };
                let mut forth = vec![];
                let mut more =
                    vec![assign_when(cmp(CmpOp::Ge, Expr::Time, n("tb")), "k2", c(1.0), "k2 set")];
                if kind == "own" {
                    forth.push(set("k", c(1.0)));
                } else {
                    more.push(assign_when(
                        cmp(if up { CmpOp::Gt } else { CmpOp::Lt }, n("x"), c(0.5)),
                        "k",
                        c(1.0),
                        "k set",
                    ));
                }
                let m = holds_shape(
                    up,
                    true,
                    config,
                    &phi,
                    n("a"),
                    forth,
                    vec![],
                    more,
                    &[("tb", "s", 1.0), ("ex", "1", 1e-6)],
                    &[("k", "1"), ("k2", "1")],
                );
                let built = holds_build(&m, config);
                for a in [0.7, 2.3] {
                    for rtol in [1e-5, 1e-7, 1e-9] {
                        let (t1, tb) = (0.625 / a, 0.625 / a + 0.5);
                        let ps = [("a", a), ("tb", tb), ("ex", 100.0 * rtol)];
                        runs += 1;
                        let what = format!("{kind} {config:?} up {up} a {a} rtol {rtol:e}");
                        match holds_run(&built, config, &ps, rtol) {
                            Ok(r) => {
                                let right = at_times(&r.forth, &[(t1, loc(a, rtol)), (tb, 1e-12)])
                                    && r.back.len() == 1
                                    && r.back[0] == r.forth[0]
                                    && (r.n, r.m) == (2.0, 1.0);
                                if !right {
                                    bad.push(format!(
                                        "{what}: forth {:?}, back {:?}",
                                        r.forth, r.back
                                    ));
                                }
                            }
                            Err(e) => bad.push(format!("{what}: {e}")),
                        }
                    }
                }
            }
        }
    }
    assert!(bad.is_empty(), "{} of {runs} wrong:\n{}", bad.len(), bad.join("\n"));
}

/// The crossing's own event slows x by K (review 3's R3-B1): z' = a (1 − c
/// k), c = 1 − 1/K (no state: R reads a t − k c a (t − tk), tk the event's
/// time), k set by forth (slow), by a sibling `when` on the same relation
/// (slowsib), or by forth with back setting it back (slowr: fast below,
/// slow above). In truth x reaches 0.5 at t1 and goes on past it, slower:
/// forth once, at t1, back never. A crossing located a little early, held
/// (the event changes no value x reads there), needs K times as long to
/// arrive; a time limit from the rate before the event expired first and
/// fired back, then forth again up to 98 location tolerances later (or a
/// chatter, slowr). The time limit of a hold the crossing itself made takes
/// the rate after the event. Strict and not (`>=`, `<=`).
#[test]
fn a_crossing_whose_own_event_slows_it_fires_once() {
    let mut bad = vec![];
    let mut runs = 0;
    for kind in ["slow", "slowsib", "slowr"] {
        for config in [Config::Ida, Config::NoState, Config::Diffsol] {
            for (up, strict) in [(true, true), (false, true), (true, false), (false, false)] {
                let state = config.state();
                let phi = |_: &dyn Fn() -> Expr| {
                    if state {
                        c(0.0)
                    } else {
                        c(0.0) - n("k") * n("cs") * n("a") * (Expr::Time - n("tk"))
                    }
                };
                let rate = n("a") * (c(1.0) - n("cs") * n("k"));
                let slowing = vec![set("k", c(1.0)), set("tk", Expr::Time)];
                let (forth, back, more) = match kind {
                    "slow" => (slowing, vec![], vec![]),
                    "slowr" => (slowing, vec![set("k", c(0.0))], vec![]),
                    _ => (
                        vec![],
                        vec![],
                        vec![EquationDecl {
                            eq: Equation::When {
                                condition: cmp(
                                    match (up, strict) {
                                        (true, true) => CmpOp::Gt,
                                        (true, false) => CmpOp::Ge,
                                        (false, true) => CmpOp::Lt,
                                        (false, false) => CmpOp::Le,
                                    },
                                    n("x"),
                                    c(0.5),
                                ),
                                actions: slowing,
                            },
                            label: Some("slows".into()),
                        }],
                    ),
                };
                let m = holds_shape(
                    up,
                    strict,
                    config,
                    &phi,
                    rate,
                    forth,
                    back,
                    more,
                    &[("cs", "1", 0.0)],
                    &[("k", "1"), ("tk", "s")],
                );
                let built = holds_build(&m, config);
                for a in [0.5, 2.3] {
                    for big in [100.0, 1e4] {
                        for rtol in HOLD_RTOLS {
                            let t1 = 0.625 / a;
                            let ps = [("a", a), ("cs", 1.0 - 1.0 / big)];
                            runs += 1;
                            let what = format!(
                                "{kind} {config:?} up {up} strict {strict} a {a} K {big} rtol {rtol:e}"
                            );
                            match holds_run(&built, config, &ps, rtol) {
                                Ok(r) => {
                                    let right = at_times(&r.forth, &[(t1, loc(a, rtol))])
                                        && r.back.is_empty()
                                        && (r.n, r.m) == (1.0, 0.0);
                                    if !right {
                                        bad.push(format!(
                                            "{what}: forth {:?}, back {:?}",
                                            r.forth, r.back
                                        ));
                                    }
                                }
                                Err(e) => bad.push(format!("{what}: {e}")),
                            }
                        }
                    }
                }
            }
        }
    }
    assert!(bad.is_empty(), "{} of {runs} wrong:\n{}", bad.len(), bad.join("\n"));
}

/// A hold on a function an event stops short of its side ends (review 2's
/// R2-B1): x falls through 0.5 at t1 = 0.625 / a; at t2 = t1 − d (d = q
/// band / |x'|, between a crossing located early and the true one), where
/// a state w = t passes t2, R is reset to its own value there (bcoinw) or z
/// stops (bfreezew): x stays 0.5 + q band, short of 0.5, until k2 at tb =
/// t1 + 0.5 takes it past. Truth: forth at tb, once; back never. Where the
/// crossing was located before t2, forth fires there and the hold made
/// stood at t2 with no end: the function never arrives, and the lift's
/// forth was lost ("forth off by 5e11 tolerances": 202 of review 2's 7 056
/// runs per shape at 4d24528). Every hold now ends: back fires at t2 (the
/// event read as a reset) or at the hold's expiry, and forth at tb.
#[test]
fn a_hold_on_a_condition_an_event_stops_short_ends_at_its_time_limit() {
    let mut bad = vec![];
    let mut runs = 0;
    for kind in ["bcoinw", "bfreezew"] {
        let config = Config::Ida;
        let phi = |dd: &dyn Fn() -> Expr| {
            if kind == "bcoinw" {
                c(0.0) - n("k") * (dd() + n("e")) + n("k2") * (c(0.1) + n("e"))
            } else {
                n("k2") * c(0.1)
            }
        };
        let rate = if kind == "bfreezew" { n("a") * (c(1.0) - n("k")) } else { n("a") };
        let more = vec![
            assign_when(cmp(CmpOp::Gt, n("w"), n("t2")), "k", c(1.0), "k set"),
            assign_when(cmp(CmpOp::Ge, Expr::Time, n("tb")), "k2", c(1.0), "k2 set"),
        ];
        let mut m = holds_shape(
            false,
            true,
            config,
            &phi,
            rate,
            vec![],
            vec![],
            more,
            &[("t2", "s", 1.0), ("tb", "s", 1.0), ("e", "1", 1e-12)],
            &[("k", "1"), ("k2", "1")],
        );
        m.vars.push(state("w", "s", 0.0, "a clock"));
        m.equations.push(eq(der("w"), c(1.0), "the clock's rate"));
        let built = holds_build(&m, config);
        for (a, q, rtol) in [
            (0.7, 0.4, 1e-6),
            (1.3, 0.5, 1e-10),
            (2.3, 0.4, 1e-10),
            (6.0, 0.4, 1e-8),
            (7.0, 0.5, 1e-8),
            (1.0, 0.35, 1e-5),
            (4.4, 0.35, 1e-7),
        ] {
            let band = 0.51 * rtol;
            let d = q * band * 1.75 / a;
            let t1 = 0.625 / a;
            let (t2, tb) = (t1 - d, t1 + 0.5);
            let ps = [("a", a), ("t2", t2), ("tb", tb), ("e", a * d)];
            runs += 1;
            let what = format!("{kind} a {a} q {q} rtol {rtol:e}");
            match holds_run(&built, config, &ps, rtol) {
                Ok(r) => {
                    let truth = at_times(&r.forth, &[(tb, 1e-12)]) && r.back.is_empty();
                    // (located before t2: forth there, back at t2 or within the
                    // hold's time limit after it, forth at tb)
                    let mirror = r.forth.len() == 2
                        && r.forth[0] <= t2 + 1e-12
                        && t1 - r.forth[0] <= loc(a, rtol)
                        && (r.forth[1] - tb).abs() <= 1e-12
                        && r.back.len() == 1
                        && r.back[0] >= t2 - 1e-12
                        && r.back[0] - t2 <= loc(a, rtol);
                    let counted = (r.n, r.m) == (r.forth.len() as f64, r.back.len() as f64);
                    if !(truth || mirror) || !counted {
                        bad.push(format!(
                            "{what}: forth {:?}, back {:?} (t1 {t1}, t2 {t2}, tb {tb})",
                            r.forth, r.back
                        ));
                    }
                }
                Err(e) => bad.push(format!("{what}: {e}")),
            }
        }
    }
    assert!(bad.is_empty(), "{} of {runs} wrong:\n{}", bad.len(), bad.join("\n"));
}

/// Rounding from terms that are not iteration variables (review 2's R2-B2):
/// x³ + x + (B t + k C) − B t − k C = R, exactly x³ + x = R; forth sets k
/// (noisy3) or, with k from 1, clears it (noisy3r: the large term switched
/// off, review 3's R3-S1). The event changes x only by the solve's
/// rounding, up to about ε (B t + C). Read as a move, it fired back and
/// forth again within the crossing's location tolerance (revision 1 of the
/// rule: 43 noisy3 runs). The rounding bound counts every term's magnitude,
/// at the discrete values before and after the event. Truth: forth once.
/// (Where the solve cannot meet the tolerance at all, the integrator stops:
/// those runs are left out, as they stop the same way without holds.)
#[test]
fn rounding_in_large_cancelling_terms_is_not_read_as_a_move() {
    let mut bad = vec![];
    let mut runs = 0;
    for (kind, k0, k1) in [("noisy3", 0.0, 1.0), ("noisy3r", 1.0, 0.0)] {
        for config in [Config::Ida, Config::NoState] {
            let state = config.state();
            let r = || if state { n("z") } else { n("a") * Expr::Time };
            let big = n("bb") * Expr::Time + n("k") * n("cc");
            let lhs = cubed(n("x")) + n("x") + big - n("bb") * Expr::Time - n("k") * n("cc");
            let mut m = holds_shape(
                true,
                true,
                config,
                &|_: &dyn Fn() -> Expr| c(0.0),
                n("a"),
                vec![set("k", c(k1))],
                vec![],
                vec![],
                &[("bb", "1/s", 0.0), ("cc", "1", 0.0)],
                &[],
            );
            m.equations[0] = eq(lhs, r(), "x is solved from it");
            m.vars.push(discrete("k", "1", k0, "the large term's switch"));
            let built = holds_build(&m, config);
            for a in [0.7, 2.3] {
                for (b, cc) in [(1e5, 0.3), (1e3, 1e5), (0.0, 1e6)] {
                    for rtol in [1e-5, 1e-7, 1e-9] {
                        let t1 = 0.625 / a;
                        let ps = [("a", a), ("bb", b), ("cc", cc)];
                        let what = format!("{kind} {config:?} a {a} B {b} C {cc} rtol {rtol:e}");
                        match holds_run(&built, config, &ps, rtol) {
                            Ok(r) => {
                                runs += 1;
                                let right = at_times(&r.forth, &[(t1, loc(a, rtol))])
                                    && r.back.is_empty()
                                    && (r.n, r.m) == (1.0, 0.0);
                                if !right {
                                    bad.push(format!(
                                        "{what}: forth {:?}, back {:?}",
                                        r.forth, r.back
                                    ));
                                }
                            }
                            Err(e)
                                if e.contains("could not go on")
                                    || e.contains("made no progress")
                                    || e.contains("could not be initialised") => {}
                            Err(e) => bad.push(format!("{what}: {e}")),
                        }
                    }
                }
            }
        }
    }
    assert!(runs >= 40, "only {runs} runs went through");
    assert!(bad.is_empty(), "{} of {runs} wrong:\n{}", bad.len(), bad.join("\n"));
}

/// A condition that reads time and an iteration variable (`x > 0.5 + c
/// (t − t1)`: a mixed one, checked along the dense output after every
/// step; review 1's B3). The mixed scan read the held condition raw, saw
/// the crossing's early location as a sign change and fired forth again
/// (14 wrong and 240 bounded of review 1's 756 runs at 4d24528). The scan
/// does not look at a held condition. Truth: forth once at t1, back never.
#[test]
fn a_held_mixed_condition_is_not_scanned_again() {
    let mut bad = vec![];
    let mut runs = 0;
    for config in [Config::Ida, Config::NoState] {
        let th = || c(0.5) + n("cm") * (Expr::Time - n("t1p"));
        let mut m = holds_shape(
            true,
            true,
            config,
            &|_: &dyn Fn() -> Expr| c(0.0),
            n("a"),
            vec![],
            vec![],
            vec![],
            &[("cm", "1/s", 0.05), ("t1p", "s", 1.0)],
            &[],
        );
        for (k, op) in [(1, CmpOp::Gt), (2, CmpOp::Lt)] {
            if let Equation::When { condition, .. } = &mut m.equations[k].eq {
                *condition = cmp(op, n("x"), th());
            }
        }
        let built = holds_build(&m, config);
        for a in [0.5, 1.0, 2.3, 4.4, 7.0] {
            for rtol in [1e-4, 1e-5, 1e-6, 1e-7, 1e-8, 1e-9, 1e-10] {
                let t1 = 0.625 / a;
                runs += 1;
                let what = format!("{config:?} a {a} rtol {rtol:e}");
                match holds_run(&built, config, &[("a", a), ("t1p", t1)], rtol) {
                    Ok(r) => {
                        let tol = 10.0 * rtol / (a / 1.75 - 0.05) + 1e-12;
                        if !(at_times(&r.forth, &[(t1, tol)]) && r.back.is_empty()) {
                            bad.push(format!("{what}: forth {:?}, back {:?}", r.forth, r.back));
                        }
                    }
                    Err(e) => bad.push(format!("{what}: {e}")),
                }
            }
        }
    }
    assert!(bad.is_empty(), "{} of {runs} wrong:\n{}", bad.len(), bad.join("\n"));
}

/// A reset that lands x on the value the consistent x had at the located
/// instant (review 1's B2, coin): the own shape above with ex = a (t1 − tf)
/// / 1.75, tf the instant a first run (ex = 1 rtol) located the crossing
/// at. At the event's point the reset changes nothing (g1 = g0), so a test
/// of the change there alone held it, and neither back nor the second
/// forth fired. Read at the instant the crossing really is (the second
/// point), the reset is one: back at the crossing, forth again at tb.
#[test]
fn a_reset_that_lands_on_the_located_value_is_still_a_reset() {
    let mut bad = vec![];
    let mut tried = 0;
    let e = || c(1.75) * n("ex");
    let phi = |dd: &dyn Fn() -> Expr| c(0.0) - n("k") * (dd() + e()) + n("k2") * (c(0.1) + e());
    let more = || vec![assign_when(cmp(CmpOp::Ge, Expr::Time, n("tb")), "k2", c(1.0), "k2 set")];
    for config in [Config::Ida, Config::IdaImplicit] {
        let m = holds_shape(
            true,
            true,
            config,
            &phi,
            n("a"),
            vec![set("k", c(1.0))],
            vec![],
            more(),
            &[("tb", "s", 1.0), ("ex", "1", 1e-6)],
            &[("k", "1"), ("k2", "1")],
        );
        let built = holds_build(&m, config);
        for a in [0.5, 0.7, 1.0, 1.3, 2.3, 3.1, 4.4, 6.0, 7.0] {
            for rtol in [1e-4, 1e-5, 1e-6, 1e-7, 1e-8, 1e-9, 1e-10] {
                let (t1, tb) = (0.625 / a, 0.625 / a + 0.5);
                let first = holds_run(&built, config, &[("a", a), ("tb", tb), ("ex", rtol)], rtol)
                    .unwrap_or_else(|e| panic!("{config:?} a {a} rtol {rtol:e}: {e}"));
                let Some(&tf) = first.forth.first() else { continue };
                if tf >= t1 {
                    continue;
                }
                tried += 1;
                let ex = a * (t1 - tf) / 1.75;
                let what = format!("{config:?} a {a} rtol {rtol:e} ex {ex:e}");
                match holds_run(&built, config, &[("a", a), ("tb", tb), ("ex", ex)], rtol) {
                    Ok(r) => {
                        // (ex at the solve's resolution: the run moves only
                        // within it, the noise limit's)
                        let noise = ex <= (0.51 * rtol) / 3.0;
                        let right = at_times(&r.forth, &[(t1, loc(a, rtol)), (tb, 1e-12)])
                            && r.back.len() == 1
                            && r.back[0] == r.forth[0];
                        if !right && !noise {
                            bad.push(format!(
                                "{what}: forth {:?}, back {:?} (tf {tf})",
                                r.forth, r.back
                            ));
                        }
                    }
                    Err(e) => bad.push(format!("{what}: {e}")),
                }
            }
        }
    }
    assert!(tried >= 20, "only {tried} crossings located early");
    assert!(bad.is_empty(), "{} of {tried} wrong:\n{}", bad.len(), bad.join("\n"));
}
