//! Index reduction's choice of states, watched along the run
//! (`lsim_solve::states`, DESIGN.md §6 step 5): a pendulum in x and y
//! (index 3) whose chosen states degenerate stops before they do, naming
//! the rod's constraint and the variables, at every tolerance; one whose
//! states hold runs to the end; `stateSelect` decides the choice.

mod common;

use lsim_ir::component::build::{discrete, eq, param, state, var};
use lsim_ir::expr::{Builtin, CmpOp, Expr, c, call, cmp, der, name as n};
use lsim_ir::runtime::DiscreteBlock;
use lsim_ir::{ComponentDef, Equation, EquationDecl, Severity, StateSelect, VarDecl, WhenAction};
use lsim_solve::{BlockInfo, OutputGrid, RunInfo, SolveError, SolverOptions, VarSource, simulate};

/// The IFToMM planar simple pendulum's equations: a point mass on a rod of
/// length `L`, in x and y, released at rest at `theta0` from the downward
/// vertical; `x_select` is x's `stateSelect`.
fn pendulum(theta0: f64, x_select: StateSelect) -> ComponentDef {
    moving_pendulum(theta0, 0.0, x_select)
}

/// [`pendulum`] given the speed `v0` at the start, at right angles to the
/// rod.
fn moving_pendulum(theta0: f64, v0: f64, x_select: StateSelect) -> ComponentDef {
    let start = |name: &str, unit: &str, value: Expr, fixed: bool| VarDecl {
        start: Some(value),
        fixed,
        ..var(name, unit, "")
    };
    let theta0e = || n("theta0");
    ComponentDef {
        name: "Test.Pendulum".into(),
        params: vec![
            param("m", "kg", 1.0, "mass"),
            param("L", "m", 1.0, "rod length"),
            param("g", "m/s2", 9.81, "gravity"),
            param("theta0", "1", theta0, "release angle"),
            param("v0", "m/s", v0, "speed at the start"),
        ],
        vars: vec![
            VarDecl {
                state_select: x_select,
                ..start("x", "m", n("L") * call(Builtin::Sin, vec![theta0e()]), true)
            },
            start("y", "m", -(n("L") * call(Builtin::Cos, vec![theta0e()])), false),
            start("vx", "m/s", n("v0") * call(Builtin::Cos, vec![theta0e()]), true),
            start("vy", "m/s", n("v0") * call(Builtin::Sin, vec![theta0e()]), false),
            start("F", "N", n("m") * n("g"), false),
        ],
        equations: vec![
            eq(der("x"), n("vx"), "x changes with vx"),
            eq(der("y"), n("vy"), "y changes with vy"),
            eq(n("m") * der("vx"), -(n("x") * n("F") / n("L")), "the rod pulls along x"),
            eq(
                n("m") * der("vy"),
                -(n("y") * n("F") / n("L")) - n("m") * n("g"),
                "the rod and gravity pull along y",
            ),
            eq(n("x") * n("x") + n("y") * n("y"), n("L") * n("L"), "the rod keeps its length"),
        ],
        ..Default::default()
    }
}

fn states(b: &common::Built) -> Vec<&str> {
    b.prepared.states.iter().map(|v| b.prepared.flat.var(*v).name.as_str()).collect()
}

const RTOLS: [f64; 7] = [1e-4, 1e-5, 1e-6, 1e-7, 1e-8, 1e-9, 1e-10];

/// The time the pendulum (L = 1 m, g = 9.81 m/s²) takes from the bottom of
/// its swing to the angle `theta` from the downward vertical, its angular
/// speed at the bottom `w0`: ∫₀^θ dφ / √(w0² − 2g(1 − cos φ)), by
/// Simpson's rule on 2000 intervals (the integrand is smooth and bounded
/// here: the error is at round-off).
fn time_from_bottom(w0: f64, theta: f64) -> f64 {
    use lsim_ir::math::cos;
    let f = |phi: f64| 1.0 / (w0 * w0 - 2.0 * 9.81 * (1.0 - cos(phi))).sqrt();
    let n = 2000;
    let h = theta / n as f64;
    let inner: f64 =
        (1..n).map(|k| if k % 2 == 1 { 4.0 } else { 2.0 } * f(k as f64 * h)).sum::<f64>();
    h / 3.0 * (f(0.0) + inner + f(theta))
}

/// Runs `built` to `t_end` at every tolerance: each run stops, naming the
/// rod deciding `decided` in place of `state`, at the time `want` (where
/// the decided variable first moves a hundred times as much as the state,
/// from the closed form) within 100 rtol seconds (the solution's own error
/// moves the point it reaches the limit at); the stop times, by tolerance.
fn stops_at(built: &common::Built, t_end: f64, decided: &str, state: &str, want: f64) -> Vec<f64> {
    let at = stop_times(built, &built.info, &Vec::new, t_end, decided, state);
    for (rtol, t) in RTOLS.into_iter().zip(&at) {
        println!("rtol {rtol:e}: {:+.2e} s from {want}", t - want);
        assert!((t - want).abs() < 100.0 * rtol, "rtol {rtol:e}: stops at {t}, not at {want}");
    }
    at
}

/// Runs `built` with the run information `info` (the model's, with
/// sampled blocks), the blocks made afresh for each run by `blocks`, to
/// `t_end` at every tolerance: each run stops, naming the rod deciding
/// `decided` in place of `state`; the stop times, by tolerance.
fn stop_times(
    built: &common::Built,
    info: &RunInfo,
    blocks: &dyn Fn() -> Vec<Box<dyn DiscreteBlock>>,
    t_end: f64,
    decided: &str,
    state: &str,
) -> Vec<f64> {
    let mut at = vec![];
    for rtol in RTOLS {
        let opts = SolverOptions { rtol, atol: rtol * 1e-2, ..Default::default() };
        let grid = OutputGrid { t0: 0.0, t_end, dt: 0.01 };
        let err = match simulate(&built.jit, info, &opts, grid, &mut blocks()) {
            Ok(r) => panic!("rtol {rtol:e}: ran to the end ({} events)", r.events.len()),
            Err(e) => e,
        };
        let SolveError::StateSelection { t, message, .. } = &err else {
            panic!("rtol {rtol:e}: {err}")
        };
        assert!(
            message.contains(&format!("(the rod keeps its length) decides {decided}")),
            "{err}"
        );
        assert!(message.contains(&format!("in place of {state}")), "{err}");
        at.push(*t);
    }
    at
}

/// Released level with its pivot (the IFToMM problem, θ0 = −π/2), index
/// reduction keeps y and vy (x moves most at the start); at the bottom of
/// the swing they no longer decide x (x = ±√(L² − y²)). Before the watch
/// the run stopped there on IDA's flag −4 (rtol 1e-4 … 1e-6), or with no
/// step passing (1e-7 … 1e-10), or passed the bottom on the wrong branch
/// and returned a wrong answer without a word (the outside judge's
/// iftomm-pendulum at 1e-4 … 1e-6: the energy off by up to 29 J). Now
/// every tolerance stops just before the bottom, where x first moves a
/// hundred times as much as y (|x| = |y| / 100, θ = atan(0.01) before the
/// bottom: 2.26 ms), naming the rod and the variables.
#[test]
fn a_pendulum_whose_chosen_states_degenerate_stops_naming_its_rod() {
    let built = common::build(
        &common::library(),
        &pendulum(-std::f64::consts::FRAC_PI_2, StateSelect::Default),
        false,
    );
    assert_eq!(states(&built), ["y", "vy"], "the states chosen at the start");
    // a quarter of the period: K(sin²(π/4)) / √(g/L)
    let quarter = 1.854_074_677_301_372 / 9.81f64.sqrt();
    let want = quarter - time_from_bottom((2.0f64 * 9.81).sqrt(), lsim_ir::math::atan(0.01));
    // (1.8e-4 s off at 1e-4, 7.8e-6 at 1e-7 where y and vy hardly decide
    // the motion near the bottom, 5.9e-10 at 1e-10)
    stops_at(&built, 10.0, "x", "y", want);
}

/// Given the speed to loop at the bottom (m = 4gL/v0² = 0.5), the choice
/// keeps x and vx (the rod pulls straight up there, along y), which no
/// longer decide y where the rod passes the horizontal. Before the watch
/// the run returned a wrong answer without a word at rtol 1e-4 (2 m off),
/// and stopped on IDA's flag −4 or with no step passing at the others,
/// where the error had grown far enough (0.19 to 3.8 s); now every
/// tolerance stops where y first moves a hundred times as much as x (θ =
/// π/2 − atan(0.01)), at rtol 1e-4 too, whose step crosses the horizontal
/// in one (its step ends show only that the block's sign changed: the
/// step's dense output shows where).
#[test]
fn a_looping_pendulum_stops_where_its_states_degenerate_at_every_tolerance() {
    let v0 = (4.0f64 * 9.81 / 0.5).sqrt();
    let built =
        common::build(&common::library(), &moving_pendulum(0.0, v0, StateSelect::Default), false);
    assert_eq!(states(&built), ["x", "vx"], "the states chosen at the start");
    let want = time_from_bottom(v0, std::f64::consts::FRAC_PI_2 - lsim_ir::math::atan(0.01));
    // (8.2e-6 s off at 1e-4, 1.6e-10 at 1e-10)
    stops_at(&built, 10.0, "y", "x", want);
}

/// The quarter period of the pendulum (L = 1 m, g = 9.81 m/s²) released at
/// rest at `theta0`: K(sin²(θ0/2)) / √(g/L), K by the arithmetic-geometric
/// mean, K(k) = π / (2 agm(1, √(1 − k²))) (exact to round-off).
fn quarter(theta0: f64) -> f64 {
    let (mut a, mut b) = (1.0f64, lsim_ir::math::cos(0.5 * theta0));
    for _ in 0..60 {
        let (a1, b1) = (0.5 * (a + b), (a * b).sqrt());
        if a1 == a && b1 == b {
            break;
        }
        (a, b) = (a1, b1);
    }
    std::f64::consts::PI / (2.0 * a) / 9.81f64.sqrt()
}

/// Released at rest at 2.8 rad (above the pivot's level), the choice keeps
/// x and vx (y moves least there), which no longer decide y where the rod
/// passes the horizontal on the way down: the model, the time the run
/// should stop at (where y first moves a hundred times as much as x, θ =
/// π/2 + atan(0.01)) and the time the rod is level (θ = π/2), from the
/// closed form.
fn high_release() -> (common::Built, f64, f64) {
    let theta0 = 2.8;
    let built = common::build(&common::library(), &pendulum(theta0, StateSelect::Default), false);
    assert_eq!(states(&built), ["x", "vx"], "the states chosen at the start");
    let w0 = (2.0 * 9.81 * (1.0 - lsim_ir::math::cos(theta0))).sqrt();
    let at = |theta: f64| quarter(theta0) - time_from_bottom(w0, theta);
    let half_pi = std::f64::consts::FRAC_PI_2;
    (built, at(half_pi + lsim_ir::math::atan(0.01)), at(half_pi))
}

/// The pendulum `def` with a `when` on `cond` that sets to 1 a discrete
/// value nothing reads, starting at `start` (1: the assignment changes
/// nothing).
fn with_when(mut def: ComponentDef, cond: Expr, start: f64) -> ComponentDef {
    def.vars.push(discrete("hit", "1", start, "set once, read by nothing"));
    def.equations.push(EquationDecl {
        eq: Equation::When {
            condition: cond,
            actions: vec![WhenAction::Assign { var: "hit".into(), value: c(1.0) }],
        },
        label: Some("a crossing".into()),
    });
    def
}

/// Each run of `at` (stop times by tolerance) stops after the event at
/// `event` and before the rod is level (at `level`, where y changes sign):
/// on the branch the run started on, the states still deciding y.
fn before_level(at: &[f64], event: f64, level: f64) {
    for (rtol, t) in RTOLS.into_iter().zip(at) {
        println!("rtol {rtol:e}: stops {:.2e} s before the rod is level", level - t);
        assert!(*t > event && *t < level, "rtol {rtol:e}: stops at {t}, the rod level at {level}");
    }
}

/// An event a little before the states degenerate leaves the stop where it
/// is, at every tolerance: a `when` on y as the rod nears the horizontal
/// (2.3 ms before), or on time (2 ms before), whose assignment changes
/// nothing (the integration goes on: the stop is the plain run's), or that
/// sets a value nothing reads (the integrator restarts there: the stop is
/// on the same branch, before the rod is level; restarted that close to
/// the point the solution itself is off by up to a few hundred rtol).
/// Before, the step after any event went unwatched (its sign was compared
/// with nothing): at rtol 1e-4 the step after the `when` on y crossed the
/// horizontal and ended with the factor back below 100, and the run went on
/// with y on the wrong branch and returned 2.45e-2 m off at 1.28 s without
/// a word.
#[test]
fn an_event_just_before_the_states_degenerate_leaves_the_stop_where_it_is() {
    let (plain, want, level) = high_release();
    let alone = stops_at(&plain, 1.28, "y", "x", want);
    let lib = common::library();
    // (a root changes no step; a time event ends one at its time, which
    // moves the stop within the tolerance)
    let on_y = cmp(CmpOp::Lt, n("y"), c(0.02));
    let on_time = cmp(CmpOp::Gt, Expr::Time, c(want - 0.002));
    for (cond, exact) in [(on_y, true), (on_time, false)] {
        let def = |start| with_when(pendulum(2.8, StateSelect::Default), cond.clone(), start);
        let same = common::build(&lib, &def(1.0), false);
        assert_eq!(states(&same), ["x", "vx"]);
        let at = stops_at(&same, 1.28, "y", "x", want);
        if exact {
            assert_eq!(at, alone, "a root that changes nothing moves the stop");
        }
        let set = common::build(&lib, &def(0.0), false);
        before_level(&stop_times(&set, &set.info, &Vec::new, 1.28, "y", "x"), want - 0.0025, level);
    }
}

/// A sampled block that shows x (its output reaches nothing the integrator
/// integrates: the step stands across its ticks).
struct Display(f64);

impl DiscreteBlock for Display {
    fn name(&self) -> &str {
        "'Display'"
    }
    fn period(&self) -> f64 {
        self.0
    }
    fn init(&mut self, _: f64, _: &[f64], _: &mut [f64]) -> Result<(), String> {
        Ok(())
    }
    fn tick(&mut self, _: f64, inputs: &[f64], outputs: &mut [f64]) -> Result<(), String> {
        outputs[0] = inputs[0];
        Ok(())
    }
}

/// The run information of `built` with a [`Display`] of period `period`
/// sampling x into the discrete value `shown`.
fn sampling(built: &common::Built, shown: &str, period: f64) -> RunInfo {
    let mut info = built.info.clone();
    let ix = info.var_names.iter().position(|v| v == "x").expect("x");
    let is = info.var_names.iter().position(|v| v == shown).expect(shown);
    let VarSource::D(id) = info.var_sources[is] else { panic!("{:?}", info.var_sources[is]) };
    info.blocks = vec![BlockInfo {
        name: "'Display'".into(),
        inputs: vec![ix],
        outputs: vec![id],
        period,
        chains: vec![None],
    }];
    info
}

/// A sampled block ticking through the point where the states degenerate
/// leaves the stop where it is, at every tolerance: one whose output
/// reaches nothing the integrator integrates (each tick an event inside a
/// step that stands: the stop is the run's without the block, to the last
/// bit), and one whose output drives a lag of its own (each tick an event
/// that restarts the integrator: the stop is on the same branch, before the
/// rod is level). Before, a step with a tick inside went unwatched: at rtol
/// 1e-4 the run went on past the horizontal and returned 2.45e-2 m off
/// without a word (every period from 3 to 10 ms), and at 1e-5 it stopped
/// 1.6e-3 s late (period 5 ms).
#[test]
fn a_sampled_block_ticking_through_the_point_leaves_the_stop_where_it_is() {
    let (plain, want, level) = high_release();
    let alone = stops_at(&plain, 1.28, "y", "x", want);
    let lib = common::library();
    let display = |period: f64| move || vec![Box::new(Display(period)) as Box<dyn DiscreteBlock>];
    // shown: x as the display last showed it
    let shown = with_sampled(pendulum(2.8, StateSelect::Default), "shown");
    let built = common::build(&lib, &shown, false);
    assert_eq!(states(&built), ["x", "vx"]);
    for period in [0.003, 0.005, 0.007, 0.01] {
        let info = sampling(&built, "shown", period);
        let at = stop_times(&built, &info, &display(period), 1.28, "y", "x");
        assert_eq!(at, alone, "a display every {period} s moves the stop");
    }
    // held: x sampled and held, which a lag follows (z' = (held − z) / τ)
    let mut lag = with_sampled(pendulum(2.8, StateSelect::Default), "held");
    lag.params.push(param("tau", "s", 0.05, "the lag's time constant"));
    lag.vars.push(state("z", "m", 0.0, "a lag on the held x"));
    lag.equations.push(eq(der("z"), (n("held") - n("z")) / n("tau"), "z follows the held x"));
    let built = common::build(&lib, &lag, false);
    let info = sampling(&built, "held", 0.005);
    before_level(&stop_times(&built, &info, &display(0.005), 1.28, "y", "x"), want - 0.005, level);
}

/// The pendulum `def` with a discrete value `name` that a sampled block
/// sets (a `when` that never fires makes it one of the model's).
fn with_sampled(mut def: ComponentDef, name: &str) -> ComponentDef {
    def.vars.push(discrete(name, "m", 0.0, "x as a sampled block last read it"));
    def.equations.push(EquationDecl {
        eq: Equation::When {
            condition: cmp(CmpOp::Gt, Expr::Time, c(1000.0)),
            actions: vec![WhenAction::Assign { var: name.into(), value: c(0.0) }],
        },
        label: Some("never".into()),
    });
    def
}

/// Released at 1 rad the choice keeps x and vx, and y never comes near
/// zero: the run goes to its end at every tolerance, right.
#[test]
fn a_pendulum_whose_chosen_states_hold_runs_to_the_end() {
    let built = common::build(&common::library(), &pendulum(1.0, StateSelect::Default), false);
    assert_eq!(states(&built), ["x", "vx"]);
    runs_right(&built, 1.0, 10.0, 1e3);
}

/// The pendulum's angle θ'' = −(g/L) sin θ from rest at `theta0`,
/// integrated independently (classical Runge–Kutta, 1e-4 s steps: 1e-13
/// on these swings), at every 0.01 s up to `t_end`: (x, y).
fn reference(theta0: f64, t_end: f64) -> Vec<(f64, f64)> {
    use lsim_ir::math::{cos, sin};
    let f = |th: f64, w: f64| (w, -9.81 * sin(th));
    let (mut th, mut w) = (theta0, 0.0);
    let h = 1e-4;
    let mut out = vec![(sin(th), -cos(th))];
    for k in 1..=((t_end / h).round() as usize) {
        let (a1, b1) = f(th, w);
        let (a2, b2) = f(th + 0.5 * h * a1, w + 0.5 * h * b1);
        let (a3, b3) = f(th + 0.5 * h * a2, w + 0.5 * h * b2);
        let (a4, b4) = f(th + h * a3, w + h * b3);
        th += h / 6.0 * (a1 + 2.0 * a2 + 2.0 * a3 + a4);
        w += h / 6.0 * (b1 + 2.0 * b2 + 2.0 * b3 + b4);
        if k % 100 == 0 {
            out.push((sin(th), -cos(th)));
        }
    }
    out
}

/// Runs `built` (released at `theta0`) to `t_end` at every tolerance: it
/// goes to the end, its positions within `within` rtol of the reference.
/// (Over five periods the integrator's global error grows to several
/// hundred rtol on its own: 840 rtol at 1e-10 released at 1.5 rad, where
/// OpenModelica's dynamic state selection on the same problem is 980 rtol
/// off, the outside judge's figures.)
fn runs_right(built: &common::Built, theta0: f64, t_end: f64, within: f64) {
    let want = reference(theta0, t_end);
    for rtol in RTOLS {
        let opts = SolverOptions { rtol, atol: rtol * 1e-2, ..Default::default() };
        let grid = OutputGrid { t0: 0.0, t_end, dt: 0.01 };
        let r = simulate(&built.jit, &built.info, &opts, grid, &mut [])
            .unwrap_or_else(|e| panic!("rtol {rtol:e}: {e}"));
        let ch = |name: &str| &r.values[r.names.iter().position(|x| x == name).expect(name)];
        let (x, y) = (ch("x"), ch("y"));
        let err = want
            .iter()
            .enumerate()
            .map(|(k, (wx, wy))| (x[k] - wx).abs().max((y[k] - wy).abs()))
            .fold(0.0, f64::max);
        println!(
            "θ0 {theta0}: rtol {rtol:e}: {} steps, position error {err:.1e} m = {:.0} rtol",
            r.stats.steps,
            err / rtol
        );
        assert!(err < within * rtol, "rtol {rtol:e}: the position is {err:e} m off");
    }
}

/// Released at 1.5 rad, the choice by pivoting keeps y and vy (y moves
/// fourteen times as much as x there), which degenerate at the bottom;
/// with x marked `stateSelect = prefer` (or `always`) it keeps x and vx,
/// which hold for the whole swing (y never reaches zero below the
/// horizontal, y moves at most fourteen times as much as x), and the run
/// goes to its end, right. Before, `stateSelect` was read and ignored.
#[test]
fn state_select_prefer_keeps_the_variable_the_model_marks() {
    let lib = common::library();
    let plain = common::build(&lib, &pendulum(1.5, StateSelect::Default), false);
    assert_eq!(states(&plain), ["y", "vy"]);
    for select in [StateSelect::Prefer, StateSelect::Always] {
        let marked = common::build(&lib, &pendulum(1.5, select), false);
        assert_eq!(states(&marked), ["x", "vx"], "{select:?}");
        assert!(marked.prepared.warnings.iter().all(|w| !w.code.starts_with("STATE-SELECT")));
        runs_right(&marked, 1.5, 10.0, 1e3);
    }
    // avoid (or never) on x at 0.4 rad, where pivoting alone keeps x: y
    // and vy instead
    for select in [StateSelect::Avoid, StateSelect::Never] {
        let low = common::build(&lib, &pendulum(0.4, select), false);
        assert_eq!(states(&low), ["y", "vy"], "{select:?}");
    }
    assert_eq!(
        states(&common::build(&lib, &pendulum(0.4, StateSelect::Default), false)),
        ["x", "vx"]
    );
}

/// A choice the marks ask for is taken only where it starts well inside
/// the run's limit, at half of it (a decided variable moving at most 50
/// times as much as a state): released at atan 45 the pendulum with x
/// marked `prefer` keeps x and vx (y moves 45 times as much as x at the
/// release, the swing's turning point) and runs to its end at every
/// tolerance; at atan 99, inside the run's limit of 100 but outside the
/// margin, the mark is refused and told (STATE-SELECT-PREFER), and the
/// choice by pivoting, y and vy, stops near the bottom of the swing at
/// every tolerance, naming the rod. Before, the mark was taken up to the
/// run's limit itself: at atan 99 the run stopped at its far turning point
/// at rtol 1e-5 (the first wobble past 100) and ended 297 to 3 664 rtol
/// off without a word at the six other tolerances.
#[test]
fn a_marked_choice_is_taken_only_well_inside_the_run_s_limit() {
    let lib = common::library();
    let inside = lsim_ir::math::atan(45.0);
    let kept = common::build(&lib, &pendulum(inside, StateSelect::Prefer), false);
    assert_eq!(states(&kept), ["x", "vx"]);
    assert!(kept.prepared.warnings.iter().all(|w| !w.code.starts_with("STATE-SELECT")));
    // (y carries up to 45 times x's error: 290 to 2 040 rtol over five
    // periods, where the same pendulum in its angle, an ODE, is 33 to 200
    // rtol off at 1.55 rad, the outside judge's figures)
    runs_right(&kept, inside, 10.0, 3e3);
    let edge = lsim_ir::math::atan(99.0);
    let refused = common::build(&lib, &pendulum(edge, StateSelect::Prefer), false);
    assert_eq!(states(&refused), ["y", "vy"], "the choice by pivoting");
    let told: Vec<_> =
        refused.prepared.warnings.iter().filter(|w| w.code == "STATE-SELECT-PREFER").collect();
    assert_eq!(told.len(), 1, "{:#?}", refused.prepared.warnings);
    assert!(told[0].message.contains("x of the model") && told[0].message.contains("99 times"));
    let w0 = (2.0 * 9.81 * (1.0 - lsim_ir::math::cos(edge))).sqrt();
    let want = quarter(edge) - time_from_bottom(w0, lsim_ir::math::atan(0.01));
    stops_at(&refused, 10.0, "x", "y", want);
}

/// `always` that no regular choice can keep, and `never` that has to be a
/// state, are told, and the model runs with the states that work.
#[test]
fn state_select_that_cannot_be_met_is_told() {
    let lib = common::library();
    // released exactly level, x = L: the rod's constraint does not depend
    // on y there, so x cannot be a state
    let level =
        common::build(&lib, &pendulum(std::f64::consts::FRAC_PI_2, StateSelect::Always), false);
    assert_eq!(states(&level), ["y", "vy"]);
    let w: Vec<_> =
        level.prepared.warnings.iter().filter(|w| w.code == "STATE-SELECT-ALWAYS").collect();
    assert_eq!(w.len(), 1, "{:#?}", level.prepared.warnings);
    assert_eq!(w[0].severity, Severity::Warning);
    assert!(w[0].message.contains("x of the model"), "{}", w[0].message);
    // a plain ODE whose one variable is marked never
    let decay = ComponentDef {
        name: "Test.Decay".into(),
        params: vec![param("k", "1/s", 1.0, "rate")],
        vars: vec![VarDecl { state_select: StateSelect::Never, ..state("v", "1", 1.0, "") }],
        equations: vec![eq(der("v"), -(n("k") * n("v")), "it decays")],
        ..Default::default()
    };
    let d = common::build(&lib, &decay, false);
    assert!(
        d.prepared.warnings.iter().any(|w| w.code == "STATE-SELECT-NEVER"),
        "{:#?}",
        d.prepared.warnings
    );
}

// ---------------------------------------------------------------------------
// Dynamic state selection (DESIGN.md §6 step 5, `lsim_solve::choice`): with
// the model's choices given (the engine's builders give them), a run whose
// states degrade chooses them again there and goes on.
// ---------------------------------------------------------------------------

/// The pendulum's angle θ'' = −(g/L) sin θ from `theta0` with angular speed
/// `w0`, integrated independently (classical Runge–Kutta, 1e-4 s steps:
/// 1e-13 on these motions), at every 0.01 s up to `t_end`: (x, y).
fn reference_from(theta0: f64, w0: f64, t_end: f64) -> Vec<(f64, f64)> {
    use lsim_ir::math::{cos, sin};
    let f = |th: f64, w: f64| (w, -9.81 * sin(th));
    let (mut th, mut w) = (theta0, w0);
    let h = 1e-4;
    let mut out = vec![(sin(th), -cos(th))];
    for k in 1..=((t_end / h).round() as usize) {
        let (a1, b1) = f(th, w);
        let (a2, b2) = f(th + 0.5 * h * a1, w + 0.5 * h * b1);
        let (a3, b3) = f(th + 0.5 * h * a2, w + 0.5 * h * b2);
        let (a4, b4) = f(th + h * a3, w + h * b3);
        th += h / 6.0 * (a1 + 2.0 * a2 + 2.0 * a3 + a4);
        w += h / 6.0 * (b1 + 2.0 * b2 + 2.0 * b3 + b4);
        if k % 100 == 0 {
            out.push((sin(th), -cos(th)));
        }
    }
    out
}

/// The states chosen again in a run (its `States` events).
fn switches(r: &lsim_solve::SimResult) -> usize {
    r.events.iter().filter(|e| matches!(e.kind, lsim_solve::EventKind::States(_))).count()
}

/// Runs `built` to `t_end` at every tolerance: each run goes to its end,
/// choosing its states again at least once, with the channels of the model
/// it started on, its positions within `within` rtol of `want` over the
/// whole grid; the errors in rtol, by tolerance.
fn runs_through(
    built: &common::Built,
    info: &RunInfo,
    want: &[(f64, f64)],
    t_end: f64,
    within: f64,
) -> Vec<f64> {
    let mut out = vec![];
    for rtol in RTOLS {
        let opts = SolverOptions { rtol, atol: rtol * 1e-2, ..Default::default() };
        let grid = OutputGrid { t0: 0.0, t_end, dt: 0.01 };
        let r = simulate(&built.jit, info, &opts, grid, &mut [])
            .unwrap_or_else(|e| panic!("rtol {rtol:e}: {e}"));
        assert_eq!(r.names, info.var_names, "the channels of the model the run started on");
        let ch = |name: &str| &r.values[r.names.iter().position(|x| x == name).expect(name)];
        let (x, y) = (ch("x"), ch("y"));
        let err = want
            .iter()
            .enumerate()
            .map(|(k, (wx, wy))| (x[k] - wx).abs().max((y[k] - wy).abs()))
            .fold(0.0, f64::max);
        println!(
            "rtol {rtol:e}: {} steps, {} switches, position error {err:.1e} m = {:.0} rtol",
            r.stats.steps,
            switches(&r),
            err / rtol
        );
        assert!(switches(&r) > 0, "rtol {rtol:e}: no switch");
        assert!(r.values.iter().flatten().all(|v| v.is_finite()), "rtol {rtol:e}: a NaN");
        assert!(err < within * rtol, "rtol {rtol:e}: the position is {err:e} m off");
        out.push(err / rtol);
    }
    out
}

/// Released level with its pivot (the IFToMM problem), the states chosen
/// at the start (y, vy) degenerate at the bottom of the swing: with the
/// model's choices the run chooses them again just before (x, vx), again
/// near each horizontal (y, vy) and so on, and runs its five periods at
/// every tolerance, its position as near the angle integrated
/// independently as the same pendulum in its angle comes (an ODE, no
/// choice of states: 120 to 350 rtol off over 10 s at these tolerances,
/// the outside judge's figures). Without them (and before dynamic
/// selection) it stops at the bottom, naming the rod
/// (`a_pendulum_whose_chosen_states_degenerate_stops_naming_its_rod`).
#[test]
fn a_pendulum_whose_states_degenerate_chooses_them_again_and_runs_on() {
    let lib = common::library();
    let def = pendulum(-std::f64::consts::FRAC_PI_2, StateSelect::Default);
    let built = common::build_dynamic(&lib, &def);
    assert_eq!(states(&built), ["y", "vy"], "the states chosen at the start");
    let want = reference(-std::f64::consts::FRAC_PI_2, 10.0);
    runs_through(&built, &built.info, &want, 10.0, 2e3);
}

/// Given the speed to loop (m = 4gL/v0² = 0.5), the choice keeps x and vx,
/// which no longer decide y where the rod passes the horizontal: the run
/// chooses y and vy there, and x and vx again near the top and the bottom,
/// through every loop. Before, it stopped at the first horizontal.
#[test]
fn a_looping_pendulum_chooses_its_states_again_through_every_loop() {
    let v0 = (4.0f64 * 9.81 / 0.5).sqrt();
    let lib = common::library();
    let built = common::build_dynamic(&lib, &moving_pendulum(0.0, v0, StateSelect::Default));
    assert_eq!(states(&built), ["x", "vx"]);
    let want = reference_from(0.0, v0, 10.0);
    // (ten loops: the same pendulum in its angle, an ODE, comes 191 to
    // 2 215 rtol off at these tolerances, the outside judge's figures)
    runs_through(&built, &built.info, &want, 10.0, 1e4);
}

/// Preparation chooses the states at a point of a run by the pivoting it
/// chooses them by at the start: the pendulum prepared level (y, vy chosen,
/// x moving most), chosen again at the bottom of its swing, keeps x and vx,
/// and keeps the first model's derivative channels as variables of its own
/// (the one a state's derivative is set from it).
#[test]
fn preparation_chooses_the_states_at_a_point_of_a_run() {
    let lib = common::library();
    let def = pendulum(-std::f64::consts::FRAC_PI_2, StateSelect::Default);
    let first = common::build(&lib, &def, false);
    assert_eq!(states(&first), ["y", "vy"]);
    let names = first.info.var_names.clone();
    let channels: Vec<String> = names.iter().filter(|n| n.starts_with("der(")).cloned().collect();
    // the bottom of the swing: x = 0, y = −L, moving along x
    let values = [("x", 0.0), ("y", -1.0), ("vx", 4.43), ("vy", 0.0), ("F", 29.4)];
    let at = lsim_prep::ChoosePoint {
        t: 0.5,
        values: values.iter().map(|(n, v)| (n.to_string(), *v)).collect(),
        params: first.info.params.clone(),
        channels: channels.clone(),
        window: lsim_solve::STATE_SWITCH,
        ..Default::default()
    };
    let again = lsim_prep::prepare_choosing(&lib, &def, &Default::default(), at).expect("prepares");
    let st: Vec<&str> = again.states.iter().map(|v| again.flat.var(*v).name.as_str()).collect();
    assert_eq!(st, ["x", "vx"]);
    for c in &channels {
        assert!(again.flat.find_var(c).is_some(), "{c} kept");
    }
    // the same flattening: every variable of the first at the same place
    for (k, n) in names.iter().enumerate().take(5) {
        assert_eq!(&again.flat.vars[k].name, n);
    }
}

/// Released exactly level, x = L and y = 0 to the bit, at rest: the rod's
/// velocity constraint `x·vx + y·vy = 0` says nothing of vy there (y = 0,
/// vx = 0), which keeps its start value (0) instead of 0 / 0, and only the
/// states y, vy are regular at the start. Before, the start's vy was NaN
/// and preparation refused the model (STATE-SELECT-SINGULAR: every fixed
/// choice singular at the start); now it prepares, and with the model's
/// choices runs its five periods.
#[test]
fn a_pendulum_released_exactly_level_prepares_and_runs() {
    let mut def = pendulum(std::f64::consts::FRAC_PI_2, StateSelect::Default);
    for v in &mut def.vars {
        match v.name.as_str() {
            "x" => v.start = Some(n("L")),
            "y" => v.start = Some(c(0.0)),
            _ => {}
        }
    }
    let lib = common::library();
    let built = common::build_dynamic(&lib, &def);
    assert_eq!(states(&built), ["y", "vy"]);
    let want = reference(std::f64::consts::FRAC_PI_2, 10.0);
    runs_through(&built, &built.info, &want, 10.0, 2e3);
    // the release is a double root of the rod's length in y (x fixed at
    // L): the start's Newton takes it although its Jacobian is singular
    // there, and the run says so, naming the equation and the variable
    let opts = SolverOptions { rtol: 1e-6, atol: 1e-8, ..Default::default() };
    let grid = OutputGrid { t0: 0.0, t_end: 1.0, dt: 0.01 };
    let r = simulate(&built.jit, &built.info, &opts, grid, &mut []).expect("runs");
    let told: Vec<&String> =
        r.report.warnings.iter().filter(|w| w.contains("exactly at the start values")).collect();
    assert_eq!(told.len(), 1, "{:?}", r.report.warnings);
    assert!(told[0].contains("the rod keeps its length") && told[0].contains(" by y "), "{told:?}");
}

/// The initialisation's 0 / 0 guard is told where it decides: released
/// exactly level, the rod's velocity constraint `x·vx + y·vy = 0` says
/// nothing of vy (y = 0, vx = 0), which starts at the initialisation's
/// guess for it, 0 (`vy` is `der(y)` after aliasing: a start value given to
/// it, 3 here, is not read): preparation warns, naming the equation and vy
/// (`INIT-UNDECIDED`). Released at 1 rad moving, where the equation
/// decides vy, nothing is told and vy starts where the rod's constraint
/// puts it (x·vx + y·vy = 0, to round-off).
#[test]
fn the_initialisation_s_guard_is_told_where_it_decides_and_left_out_where_it_does_not() {
    let lib = common::library();
    let mut level = pendulum(std::f64::consts::FRAC_PI_2, StateSelect::Default);
    for v in &mut level.vars {
        match v.name.as_str() {
            "x" => v.start = Some(n("L")),
            "y" => v.start = Some(c(0.0)),
            "vy" => v.start = Some(c(3.0)),
            _ => {}
        }
    }
    let built = common::build(&lib, &level, false);
    let told: Vec<&lsim_ir::Diagnostic> =
        built.prepared.warnings.iter().filter(|d| d.code == "INIT-UNDECIDED").collect();
    assert_eq!(told.len(), 1, "{:#?}", built.prepared.warnings);
    let m = &told[0].message;
    assert!(m.contains("the rod keeps its length") && m.contains("vy"), "{m}");
    assert!(m.contains("It starts at 0, ") && m.contains("is not read"), "{m}");
    // where the equation decides it: no word, the start on the constraint
    let moving = moving_pendulum(1.0, 2.0, StateSelect::Default);
    let built = common::build(&lib, &moving, false);
    assert!(
        built.prepared.warnings.iter().all(|d| d.code != "INIT-UNDECIDED"),
        "{:#?}",
        built.prepared.warnings
    );
    let opts = SolverOptions { rtol: 1e-8, atol: 1e-10, ..Default::default() };
    let grid = OutputGrid { t0: 0.0, t_end: 0.01, dt: 0.01 };
    let r = simulate(&built.jit, &built.info, &opts, grid, &mut []).expect("runs");
    let at0 = |name: &str| r.values[r.names.iter().position(|x| x == name).expect(name)][0];
    let (x, y, vx, vy) = (at0("x"), at0("y"), at0("vx"), at0("vy"));
    assert!((x * vx + y * vy).abs() < 1e-15, "{x} {y} {vx} {vy}");
    assert!((vy - 2.0 * lsim_ir::math::sin(1.0)).abs() < 1e-15, "{vy}");
}

/// Two equations that depend on each other everywhere (`x² + y = 0` and
/// `k x² + k y = 0`: their Jacobian has rank one), at start values that
/// solve them exactly (x = y = 0): the start is one of a continuum of
/// roots, which the start's Newton takes, and from which the integrator
/// cannot start. The run stops with the start's failure, naming the
/// equations and the variable they do not decide. Before, the same start
/// ended on IDA's flag (`IDACalcIC`, flag −14), and before the start's
/// Newton took exact roots at a singular Jacobian (for the pendulum
/// released exactly level), on the initialisation's failing to converge.
#[test]
fn a_start_its_equations_do_not_decide_stops_naming_them() {
    let start0 = |name: &str| VarDecl { start: Some(c(0.0)), ..var(name, "1", "") };
    let def = ComponentDef {
        name: "Test.Dependent".into(),
        params: vec![param("k", "1", 2.0, ""), param("tau", "s", 1.0, "")],
        vars: vec![start0("x"), start0("y"), state("z", "1", 0.0, "")],
        equations: vec![
            eq(n("tau") * der("z"), n("x") * n("z") + n("y"), "z follows them"),
            eq(n("x") * n("x") + n("y"), c(0.0), "the first"),
            eq(n("k") * n("x") * n("x") + n("k") * n("y"), c(0.0), "the second, k times it"),
        ],
        ..Default::default()
    };
    let lib = common::library();
    let built = common::build(&lib, &def, false);
    let opts = SolverOptions { rtol: 1e-6, atol: 1e-8, ..Default::default() };
    let grid = OutputGrid { t0: 0.0, t_end: 1.0, dt: 0.01 };
    let e = simulate(&built.jit, &built.info, &opts, grid, &mut []).expect_err("cannot start");
    let SolveError::Initialisation { t, message } = &e else { panic!("{e}") };
    assert_eq!(*t, 0.0);
    // (the equation the dependence shows in: the other is solved for y)
    assert!(message.contains("the first") || message.contains("the second"), "{e}");
    assert!(message.contains("by x ") && message.contains("not decide it"), "{e}");
}

/// A run whose states hold (released at 1 rad: the choice keeps x and vx,
/// y moves at most 1.6 times as much as x) never chooses them again, and is
/// the run without the model's choices to the last bit.
#[test]
fn a_run_whose_states_hold_is_the_run_without_choices_to_the_bit() {
    let lib = common::library();
    let def = pendulum(1.0, StateSelect::Default);
    let dynamic = common::build_dynamic(&lib, &def);
    let plain = common::build(&lib, &def, false);
    for rtol in RTOLS {
        let opts = SolverOptions { rtol, atol: rtol * 1e-2, ..Default::default() };
        let grid = OutputGrid { t0: 0.0, t_end: 10.0, dt: 0.01 };
        let a = simulate(&dynamic.jit, &dynamic.info, &opts, grid, &mut []).expect("runs");
        let b = simulate(&plain.jit, &plain.info, &opts, grid, &mut []).expect("runs");
        assert_eq!(switches(&a), 0);
        assert_eq!(a.stats, b.stats, "rtol {rtol:e}");
        for (u, v) in a.values.iter().flatten().zip(b.values.iter().flatten()) {
            assert_eq!(u.to_bits(), v.to_bits(), "rtol {rtol:e}");
        }
    }
}

/// Where no other choice can be had (the model's choices fail), the run
/// goes on with the states it has and stops where they degrade past the
/// run's limit, naming the rod, as without dynamic selection, at the same
/// point to the bit.
#[test]
fn where_no_other_choice_works_the_run_stops_as_before() {
    struct Refuse;
    impl lsim_solve::choice::StateChoices for Refuse {
        fn choose(
            &self,
            _: &lsim_solve::choice::ChoicePoint,
        ) -> Result<std::sync::Arc<lsim_solve::choice::Variant>, String> {
            Err("no regular choice here".into())
        }
    }
    let lib = common::library();
    let built =
        common::build(&lib, &pendulum(-std::f64::consts::FRAC_PI_2, StateSelect::Default), false);
    let mut refusing = built.info.clone();
    refusing.choices = Some(lsim_solve::choice::Choices(std::sync::Arc::new(Refuse)));
    let alone = stop_times(&built, &built.info, &Vec::new, 10.0, "x", "y");
    let refused = stop_times(&built, &refusing, &Vec::new, 10.0, "x", "y");
    assert_eq!(alone, refused);
}

/// The energy books read on across the switches. Two parts read the
/// pendulum's height: one takes in a power `vy` (its energy in, an
/// integral the integrators carry, is then y's change), the other stores
/// the energy `y` (its stored energy's integral, taken along each model the
/// run is on, against its change from the channels: the drift; the
/// switches' jumps in it, where the iteration variables are solved again on
/// the new states, booked as events'). Across the switches the integrals go
/// on from where they were (a reset would put the energy in off by the
/// height at the switch, metres), the jumps are within the tolerance, and
/// both agree with the height as the run without switches does.
#[test]
fn the_energy_books_read_on_across_the_switches() {
    let lib = common::library();
    let books = |built: &common::Built, rtol: f64| {
        let mut info = built.info.clone();
        let v = |name: &str| {
            let k = info.var_names.iter().position(|x| x == name).expect(name);
            Expr::Var(lsim_ir::VarId(k as u32))
        };
        let parts = vec![
            lsim_solve::EnergyPart {
                path: "lift".into(),
                name: "'lift'".into(),
                power: v("vy"),
                loss: None,
                stored: None,
            },
            lsim_solve::EnergyPart {
                path: "height".into(),
                name: "'height'".into(),
                power: c(0.0),
                loss: None,
                stored: Some(v("y")),
            },
        ];
        info.energy = Some(std::sync::Arc::new(lsim_solve::EnergyInfo { parts }));
        info.stored_rates = None;
        let opts = SolverOptions { rtol, atol: rtol * 1e-2, ..Default::default() };
        let grid = OutputGrid { t0: 0.0, t_end: 10.0, dt: 0.01 };
        let r = simulate(&built.jit, &info, &opts, grid, &mut []).expect("runs");
        let y = &r.values[r.names.iter().position(|x| x == "y").expect("y")];
        let rise = y[y.len() - 1] - y[0];
        let b = r.energy.expect("books");
        let (lift, height) = (&b.parts[0], &b.parts[1]);
        // in rtol: the energy in against the rise, the drift, the jumps
        let off = [(lift.energy_in - rise).abs(), height.drift.abs(), height.event_change.abs()]
            .map(|x| x / rtol);
        (switches_of(&r.events), off)
    };
    for rtol in [1e-6, 1e-8, 1e-10] {
        let plain = common::build(&lib, &pendulum(1.0, StateSelect::Default), false);
        let (_, without) = books(&plain, rtol);
        let level = pendulum(-std::f64::consts::FRAC_PI_2, StateSelect::Default);
        let (n, with) = books(&common::build_dynamic(&lib, &level), rtol);
        println!(
            "rtol {rtol:e}: {n} switches; energy in against the rise, drift, jumps (rtol): \
             {with:.1?}; at 1 rad without switches {without:.1?}"
        );
        assert!(n > 0);
        assert!(with.iter().all(|x| *x < 100.0), "rtol {rtol:e}: {with:?}");
    }
}

/// [`switches`] of a run's events.
fn switches_of(events: &[lsim_solve::EventRecord]) -> usize {
    events.iter().filter(|e| matches!(e.kind, lsim_solve::EventKind::States(_))).count()
}

/// `stateSelect` keeps leading the choice: x marked `prefer` at 1.55 rad
/// (y moves 48 times as much as x at the release, inside the 50 a marked
/// choice may reach) keeps x and vx for the whole swing, and the run is
/// the run without the model's choices, to the bit.
#[test]
fn a_marked_choice_is_not_left_while_it_holds() {
    let lib = common::library();
    let def = pendulum(1.55, StateSelect::Prefer);
    let dynamic = common::build_dynamic(&lib, &def);
    let plain = common::build(&lib, &def, false);
    assert_eq!(states(&dynamic), ["x", "vx"]);
    for rtol in [1e-4, 1e-7, 1e-10] {
        let opts = SolverOptions { rtol, atol: rtol * 1e-2, ..Default::default() };
        let grid = OutputGrid { t0: 0.0, t_end: 10.0, dt: 0.01 };
        let a = simulate(&dynamic.jit, &dynamic.info, &opts, grid, &mut []).expect("runs");
        let b = simulate(&plain.jit, &plain.info, &opts, grid, &mut []).expect("runs");
        assert_eq!(switches(&a), 0);
        for (u, v) in a.values.iter().flatten().zip(b.values.iter().flatten()) {
            assert_eq!(u.to_bits(), v.to_bits(), "rtol {rtol:e}");
        }
    }
}

/// An event a little before the states degenerate (a `when` on y as the rod
/// nears the horizontal that sets a value nothing reads: the integrator
/// restarts there) and a sampled block ticking through the point: the run
/// chooses the states again after them and goes on, right.
#[test]
fn events_and_ticks_near_the_point_leave_the_run_choosing_again() {
    let lib = common::library();
    let on_y = cmp(CmpOp::Lt, n("y"), c(0.02));
    let def = with_when(pendulum(2.8, StateSelect::Default), on_y, 0.0);
    let built = common::build_dynamic(&lib, &def);
    let want = reference(2.8, 3.0);
    runs_through(&built, &built.info, &want, 3.0, 3e3);
    let shown = with_sampled(pendulum(2.8, StateSelect::Default), "shown");
    let built = common::build_dynamic(&lib, &shown);
    let info = sampling(&built, "shown", 0.005);
    for rtol in [1e-4, 1e-7, 1e-10] {
        let opts = SolverOptions { rtol, atol: rtol * 1e-2, ..Default::default() };
        let grid = OutputGrid { t0: 0.0, t_end: 3.0, dt: 0.01 };
        let mut blocks = vec![Box::new(Display(0.005)) as Box<dyn DiscreteBlock>];
        let r = simulate(&built.jit, &info, &opts, grid, &mut blocks).expect("runs");
        let x = &r.values[r.names.iter().position(|v| v == "x").expect("x")];
        // (its ticks change nothing the integrator integrates: the run is
        // the run without the display, to the bit)
        let alone = simulate(&built.jit, &built.info, &opts, grid, &mut []).expect("runs");
        let x_alone = &alone.values[r.names.iter().position(|v| v == "x").expect("x")];
        assert!(x.iter().zip(x_alone).all(|(a, b)| a.to_bits() == b.to_bits()), "rtol {rtol:e}");
        let shown = &r.values[r.names.iter().position(|v| v == "shown").expect("shown")];
        let err = want.iter().zip(x).map(|((wx, _), x)| (x - wx).abs()).fold(0.0, f64::max);
        println!("rtol {rtol:e}: {} switches, x {:.0} rtol off", switches(&r), err / rtol);
        assert!(switches(&r) > 0 && err < 3e3 * rtol, "rtol {rtol:e}: {err:e}");
        // the display still shows x, a tick at most behind
        // (from the first tick on: before it, its start value)
        let off = shown.iter().zip(x).skip(1).map(|(s, x)| (s - x).abs()).fold(0.0, f64::max);
        assert!(off < 0.05, "rtol {rtol:e}: the display {off} m off x");
    }
}

/// A sampled block whose output steps once, at `.0`, from 0 to 1 (`.1` its
/// period).
struct StepAt(f64, f64);

impl DiscreteBlock for StepAt {
    fn name(&self) -> &str {
        "'Step'"
    }
    fn period(&self) -> f64 {
        self.1
    }
    fn init(&mut self, _: f64, _: &[f64], _: &mut [f64]) -> Result<(), String> {
        Ok(())
    }
    fn tick(&mut self, t: f64, _: &[f64], outputs: &mut [f64]) -> Result<(), String> {
        outputs[0] = if t >= self.0 { 1.0 } else { 0.0 };
        Ok(())
    }
}

/// A tick that restarts the run inside the step where the states are to be
/// chosen again, before the point: the switch is made after the tick, from
/// where the run is there (the watch looks again from the point after it),
/// not from the point the step's look found. A block whose output steps
/// once, read by a lag the integrator integrates, idle until then (its
/// ticks inside the integrator's steps), the step at every 0.1 ms in the 5
/// ms before the first switch (at 0.6550 s at rtol 1e-4): before, the run
/// went on from 0.6500 … 0.6549 s with the states of 0.6550 s, the pendulum
/// put 5 ms ahead and 2.7e-2 to 3.1e-2 m off; now 3.2e-4 m.
#[test]
fn a_tick_that_restarts_the_run_inside_a_switch_s_step_leaves_the_switch_to_after_it() {
    let lib = common::library();
    let mut lag = with_sampled(pendulum(2.8, StateSelect::Default), "held");
    lag.params.push(param("tau", "s", 0.05, "the lag's time constant"));
    lag.vars.push(state("z", "m", 0.0, "a lag on the held step"));
    lag.equations.push(eq(der("z"), (n("held") - n("z")) / n("tau"), "z follows the step"));
    let built = common::build_dynamic(&lib, &lag);
    let want = reference(2.8, 1.0);
    let period = 1e-4;
    let info = sampling(&built, "held", period);
    let rtol = 1e-4;
    let opts = SolverOptions { rtol, atol: rtol * 1e-2, ..Default::default() };
    let grid = OutputGrid { t0: 0.0, t_end: 1.0, dt: 0.01 };
    for k in 0..50 {
        let at = 0.65 + 1e-4 * k as f64;
        let mut blocks: Vec<Box<dyn DiscreteBlock>> = vec![Box::new(StepAt(at, period))];
        let r = simulate(&built.jit, &info, &opts, grid, &mut blocks).expect("runs");
        let col = |name: &str| &r.values[r.names.iter().position(|v| v == name).expect(name)];
        let (x, y) = (col("x"), col("y"));
        let err = want
            .iter()
            .zip(x.iter().zip(y))
            .map(|((wx, wy), (x, y))| (x - wx).abs().max((y - wy).abs()))
            .fold(0.0, f64::max);
        assert!(switches(&r) > 0 && err < 10.0 * rtol, "the step at {at:.4} s: {err:e} m off");
    }
}

/// The pendulum released at 1.50 rad (it chooses its states again 17 times
/// in 10 s) beside an integral of a table read along time, `der(z) =
/// rate(time)`, the table linear on {0, 5} s → {0, 1} m/s and held at its
/// edges: z(10 s) = 7.5 m.
fn with_rate() -> (common::Built, lsim_ir::table::TableData) {
    use lsim_ir::table::{Interpolation, Outside, TableData};
    let mut data = TableData::new_1d(vec![0.0, 5.0], vec![0.0, 1.0]);
    data.interpolation = Interpolation::Linear;
    data.outside = [Outside::Clamp, Outside::Clamp];
    data.axis_units[0] = "s".into();
    let mut def = pendulum(1.5, StateSelect::Default);
    def.params.push(lsim_lib::table::table_param("rate", "m/s", data.clone(), "a rate by time"));
    def.vars.push(state("z", "m", 0.0, "its integral"));
    def.equations.push(eq(
        der("z"),
        lsim_ir::expr::table("rate", vec![Expr::Time]),
        "z integrates the rate",
    ));
    (common::build_dynamic(&common::library(), &def), data)
}

/// A model given other table data after it was built
/// (`JitModel::with_tables`) keeps the detection: each model with the
/// states chosen again is compiled from the data the model was prepared
/// with, so the run would read one table between switches and the other
/// after them. Given its own data again, it switches and z(10 s) is 7.5 m;
/// given the table continued along its slope beyond its data (the same
/// values at its breakpoints: z(10 s) would be 10 m) or interpolated by a
/// monotone cubic, it does not switch and stops where the states degrade,
/// naming the rod, z as the new data say up to there. Before, the guard
/// compared the tables at their breakpoints only, and the run with the
/// rule outside changed went through 17 switches to z(10 s) = 9.0963 m
/// (rtol 1e-6) without a word.
#[test]
fn a_model_given_other_tables_keeps_the_detection() {
    use lsim_ir::table::{Interpolation, Outside};
    let (built, data) = with_rate();
    let opts = SolverOptions { rtol: 1e-6, atol: 1e-8, ..Default::default() };
    let grid = OutputGrid { t0: 0.0, t_end: 10.0, dt: 0.01 };
    let same = built.jit.with_tables(std::slice::from_ref(&data)).expect("takes the data");
    let r = simulate(&same, &built.info, &opts, grid, &mut []).expect("runs");
    let z = &r.values[r.names.iter().position(|x| x == "z").expect("z")];
    assert!(switches(&r) > 0);
    assert!((z[z.len() - 1] - 7.5).abs() < 1e-5, "z(10 s) = {}", z[z.len() - 1]);
    let mut beyond = data.clone();
    beyond.outside = [Outside::Linear, Outside::Linear];
    let mut cubic = data;
    cubic.interpolation = Interpolation::MonotoneCubic;
    for other in [beyond, cubic] {
        let jit = built.jit.with_tables(std::slice::from_ref(&other)).expect("takes the data");
        let e = match simulate(&jit, &built.info, &opts, grid, &mut []) {
            Ok(r) => panic!(
                "{other:?}: ran to the end through {} switches, z(10 s) = {}",
                switches(&r),
                r.values[r.names.iter().position(|x| x == "z").expect("z")].last().unwrap()
            ),
            Err(e) => e,
        };
        let SolveError::StateSelection { t, message, .. } = &e else { panic!("{e}") };
        assert!((t - 0.5804).abs() < 1e-3, "{e}");
        assert!(message.contains("(the rod keeps its length) decides"), "{e}");
        assert!(message.contains("tables were changed after it was built"), "{e}");
    }
}

/// Every way the run keeps its states where they degrade stops it there,
/// naming the rod, and says why it did not choose them again: the model
/// given no other choice; the run on diffsol (the cross-check);
/// dynamic selection switched off (`state_switch` 0); the model's
/// choices failing there. Before, the last three said "choosing the
/// states again here gave none that works better" alike, though on
/// diffsol and with the switch off the run never tried.
#[test]
fn each_fallback_stops_naming_why_the_states_were_not_chosen_again() {
    struct Refuse;
    impl lsim_solve::choice::StateChoices for Refuse {
        fn choose(
            &self,
            _: &lsim_solve::choice::ChoicePoint,
        ) -> Result<std::sync::Arc<lsim_solve::choice::Variant>, String> {
            Err("no regular choice here".into())
        }
    }
    let lib = common::library();
    let def = pendulum(1.5, StateSelect::Default);
    let plain = common::build(&lib, &def, false);
    let dynamic = common::build_dynamic(&lib, &def);
    let mut refusing = plain.info.clone();
    refusing.choices = Some(lsim_solve::choice::Choices(std::sync::Arc::new(Refuse)));
    let base = SolverOptions { rtol: 1e-6, atol: 1e-8, ..Default::default() };
    let diffsol = SolverOptions { backend: lsim_solve::Backend::Diffsol, ..base.clone() };
    let off = SolverOptions { state_switch: 0.0, ..base.clone() };
    let grid = OutputGrid { t0: 0.0, t_end: 10.0, dt: 0.01 };
    let cases: [(&RunInfo, &SolverOptions, &str); 4] = [
        (&plain.info, &base, "the model was given no other choice of states to switch to"),
        (&dynamic.info, &diffsol, "the run is on diffsol, the cross-check"),
        (&dynamic.info, &off, "SolverOptions::state_switch is 0"),
        (&refusing, &base, "did not work here: no regular choice here"),
    ];
    for (info, opts, why) in cases {
        let e = simulate(&plain.jit, info, opts, grid, &mut []).expect_err("stops");
        let SolveError::StateSelection { t, message, .. } = &e else { panic!("{e}") };
        assert!((t - 0.5804).abs() < 1e-3, "{e}");
        assert!(message.contains("(the rod keeps its length) decides x"), "{e}");
        assert!(message.contains(why), "not {why:?}: {e}");
    }
}

/// `angles.len()` x-y pendulums released at rest at `angles`, independent
/// of each other in one model (`m`, `L`, `g` shared): index reduction's
/// choice has a set per rod (at each of its levels).
fn pendulums(angles: &[f64]) -> ComponentDef {
    let start = |name: String, unit: &str, value: Expr, fixed: bool| VarDecl {
        start: Some(value),
        fixed,
        ..var(&name, unit, "")
    };
    let mut def = ComponentDef {
        name: "Test.Pendulums".into(),
        params: vec![
            param("m", "kg", 1.0, "mass"),
            param("L", "m", 1.0, "rod length"),
            param("g", "m/s2", 9.81, "gravity"),
        ],
        ..Default::default()
    };
    for (i, &th) in angles.iter().enumerate() {
        let v = |s: &str| format!("{s}{i}");
        let (sin, cos) = (lsim_ir::math::sin(th), lsim_ir::math::cos(th));
        def.vars.extend([
            start(v("x"), "m", n("L") * c(sin), true),
            start(v("y"), "m", -(n("L") * c(cos)), false),
            start(v("vx"), "m/s", c(0.0), true),
            start(v("vy"), "m/s", c(0.0), false),
            start(v("F"), "N", n("m") * n("g"), false),
        ]);
        let (x, y, vx, vy, f) = (n(&v("x")), n(&v("y")), n(&v("vx")), n(&v("vy")), n(&v("F")));
        def.equations.extend([
            eq(der(&v("x")), vx.clone(), &format!("x{i} changes with vx{i}")),
            eq(der(&v("y")), vy.clone(), &format!("y{i} changes with vy{i}")),
            eq(
                n("m") * der(&v("vx")),
                -(x.clone() * f.clone() / n("L")),
                &format!("rod {i} along x"),
            ),
            eq(
                n("m") * der(&v("vy")),
                -(y.clone() * f / n("L")) - n("m") * n("g"),
                &format!("rod {i} and gravity along y"),
            ),
            eq(x.clone() * x + y.clone() * y, n("L") * n("L"), &format!("rod {i}")),
        ]);
    }
    def
}

/// Runs the pendulums released at `angles` at `rtols`, each to its end: per
/// tolerance, each rod's position error against its reference (in rtol)
/// and its switches, and the distinct choices of states the run took.
fn pendulums_run(angles: &[f64], rtols: &[f64]) -> Vec<(Vec<f64>, Vec<usize>, usize)> {
    let built = common::build_dynamic(&common::library(), &pendulums(angles));
    let want: Vec<Vec<(f64, f64)>> = angles.iter().map(|&a| reference(a, 10.0)).collect();
    let mut out = vec![];
    for &rtol in rtols {
        let opts = SolverOptions { rtol, atol: rtol * 1e-2, ..Default::default() };
        let grid = OutputGrid { t0: 0.0, t_end: 10.0, dt: 0.01 };
        let r = simulate(&built.jit, &built.info, &opts, grid, &mut [])
            .unwrap_or_else(|e| panic!("rtol {rtol:e}: {e}"));
        let ch = |name: String| &r.values[r.names.iter().position(|x| *x == name).expect("chan")];
        let errs: Vec<f64> = (0..angles.len())
            .map(|i| {
                let (x, y) = (ch(format!("x{i}")), ch(format!("y{i}")));
                want[i]
                    .iter()
                    .enumerate()
                    .map(|(k, (wx, wy))| (x[k] - wx).abs().max((y[k] - wy).abs()))
                    .fold(0.0, f64::max)
                    / rtol
            })
            .collect();
        let per: Vec<usize> = (0..angles.len())
            .map(|i| {
                r.events
                    .iter()
                    .filter(|e| matches!(e.kind, lsim_solve::EventKind::States(_)))
                    .filter(|e| e.label.contains(&format!("(rod {i})")))
                    .count()
            })
            .collect();
        let mut choices: Vec<&str> = r
            .events
            .iter()
            .filter(|e| matches!(e.kind, lsim_solve::EventKind::States(_)))
            .filter_map(|e| e.label.split(": ").nth(1)?.split(" in place of").next())
            .collect();
        choices.sort_unstable();
        choices.dedup();
        println!(
            "rtol {rtol:e}: {} steps, switches per rod {per:?}, {} choices; errors (rtol) {:.0?}",
            r.stats.steps,
            choices.len(),
            errs
        );
        out.push((errs, per, choices.len()));
    }
    out
}

/// The pendulum released at `theta0` alone at `rtol`: its position error
/// against its reference (in rtol) and its switches.
fn alone(theta0: f64, rtol: f64) -> (f64, usize) {
    let built = common::build_dynamic(&common::library(), &pendulum(theta0, StateSelect::Default));
    let want = reference(theta0, 10.0);
    let opts = SolverOptions { rtol, atol: rtol * 1e-2, ..Default::default() };
    let grid = OutputGrid { t0: 0.0, t_end: 10.0, dt: 0.01 };
    let r = simulate(&built.jit, &built.info, &opts, grid, &mut []).expect("runs");
    let ch = |name: &str| &r.values[r.names.iter().position(|x| x == name).expect(name)];
    let (x, y) = (ch("x"), ch("y"));
    let err = want
        .iter()
        .enumerate()
        .map(|(k, (wx, wy))| (x[k] - wx).abs().max((y[k] - wy).abs()))
        .fold(0.0, f64::max);
    (err / rtol, switches(&r))
}

/// Two pendulums in one model, released at 1.50 and 1.80 rad: each
/// chooses its states again where its own degrade, the other's kept, as
/// often as it does alone, each about as near its reference as alone (the
/// integrator's steps are the two's: not to the bit). Before, the run
/// chose every set again with the window of the one that degraded, so the
/// other rod (not degrading) was pivoted onto a choice it did not need,
/// and held every set to the base threshold: every switch was refused, and
/// the run stopped at 0.2135 s naming rod 1, as without dynamic selection.
#[test]
fn independent_pendulums_choose_their_states_again_each_on_its_own() {
    let rtols = [1e-6, 1e-8, 1e-10];
    for (rtol, (errs, per, _)) in rtols.iter().zip(pendulums_run(&[1.5, 1.8], &rtols)) {
        for (i, theta0) in [1.5, 1.8].into_iter().enumerate() {
            let (err, n) = alone(theta0, *rtol);
            assert_eq!(per[i], n, "rtol {rtol:e}: rod {i}");
            assert!(
                errs[i] < 10.0 * err.max(100.0),
                "rtol {rtol:e}: rod {i} {errs:?}, alone {err}"
            );
        }
    }
}

/// Six pendulums released at 1.5 … 3.05 rad: 64 choices of states, of
/// which the run takes more than the 32 it used to keep (after them it
/// refused every switch). Each rod switches as often as alone, about as
/// near its reference as alone.
#[test]
fn six_pendulums_take_more_choices_than_a_run_used_to_keep() {
    let angles = [1.5, 1.9, 2.3, 2.6, 2.9, 3.05];
    for (errs, per, choices) in pendulums_run(&angles, &[1e-8]) {
        assert!(choices > 32, "{choices} choices");
        for (i, &theta0) in angles.iter().enumerate() {
            let (err, n) = alone(theta0, 1e-8);
            assert_eq!(per[i], n, "rod {i}");
            assert!(errs[i] < 10.0 * err.max(100.0), "rod {i}: {errs:?}, alone {err}");
        }
    }
}

/// A double pendulum in x and y (two rods, one set of constraints at each
/// level of index reduction: coupled), released at rest at 1.2 and 1.6
/// rad from the vertical, against its angles integrated independently
/// (classical Runge–Kutta, 1e-4 s steps): it chooses its states again 9
/// times in 5 s and stays within 1 000 rtol at 1e-6 … 1e-10 (the same
/// double pendulum in its angles, an ODE: 11 to 22 rtol; the x-y form's
/// error grows smoothly through the switches, a DOP853 reference at 1e-13
/// agrees with this one to 1e-12).
#[test]
fn a_double_pendulum_in_x_and_y_chooses_its_states_again() {
    use lsim_ir::math::{cos, sin};
    let start = |name: &str, unit: &str, value: Expr, fixed: bool| VarDecl {
        start: Some(value),
        fixed,
        ..var(name, unit, "")
    };
    let (a1, a2) = (1.2f64, 1.6f64);
    let def = ComponentDef {
        name: "Test.Double".into(),
        params: vec![param("g", "m/s2", 9.81, "gravity")],
        vars: vec![
            start("x1", "m", c(sin(a1)), true),
            start("y1", "m", c(-cos(a1)), false),
            start("x2", "m", c(sin(a1) + sin(a2)), true),
            start("y2", "m", c(-cos(a1) - cos(a2)), false),
            start("vx1", "m/s", c(0.0), true),
            start("vy1", "m/s", c(0.0), false),
            start("vx2", "m/s", c(0.0), true),
            start("vy2", "m/s", c(0.0), false),
            start("F1", "1/s2", c(20.0), false),
            start("F2", "1/s2", c(10.0), false),
        ],
        equations: vec![
            eq(der("x1"), n("vx1"), "x1"),
            eq(der("y1"), n("vy1"), "y1"),
            eq(der("x2"), n("vx2"), "x2"),
            eq(der("y2"), n("vy2"), "y2"),
            eq(der("vx1"), -(n("x1") * n("F1")) + (n("x2") - n("x1")) * n("F2"), "mass 1 along x"),
            eq(
                der("vy1"),
                -(n("y1") * n("F1")) + (n("y2") - n("y1")) * n("F2") - n("g"),
                "mass 1 along y",
            ),
            eq(der("vx2"), -((n("x2") - n("x1")) * n("F2")), "mass 2 along x"),
            eq(der("vy2"), -((n("y2") - n("y1")) * n("F2")) - n("g"), "mass 2 along y"),
            eq(n("x1") * n("x1") + n("y1") * n("y1"), c(1.0), "the upper rod"),
            eq(
                (n("x2") - n("x1")) * (n("x2") - n("x1"))
                    + (n("y2") - n("y1")) * (n("y2") - n("y1")),
                c(1.0),
                "the lower rod",
            ),
        ],
        ..Default::default()
    };
    // the angles: (m1 + m2) L1 θ1'' + m2 L2 θ2'' cos(θ1 − θ2) + m2 L2 θ2'²
    // sin(θ1 − θ2) + (m1 + m2) g sin θ1 = 0, and L2 θ2'' + L1 θ1'' cos(θ1 −
    // θ2) − L1 θ1'² sin(θ1 − θ2) + g sin θ2 = 0 (unit masses and rods)
    let f = |s: [f64; 4]| {
        let (t1, t2, w1, w2) = (s[0], s[1], s[2], s[3]);
        let d = t1 - t2;
        let (a, b, cc, dd) = (2.0, cos(d), cos(d), 1.0);
        let r1 = -w2 * w2 * sin(d) - 2.0 * 9.81 * sin(t1);
        let r2 = w1 * w1 * sin(d) - 9.81 * sin(t2);
        let det = a * dd - b * cc;
        [w1, w2, (r1 * dd - b * r2) / det, (a * r2 - cc * r1) / det]
    };
    let mut st = [a1, a2, 0.0, 0.0];
    let h = 1e-4;
    let pos = |s: [f64; 4]| {
        let (x1, y1) = (sin(s[0]), -cos(s[0]));
        [x1, y1, x1 + sin(s[1]), y1 - cos(s[1])]
    };
    let mut want = vec![pos(st)];
    for k in 1..=50_000 {
        let add = |s: [f64; 4], d: [f64; 4], c: f64| std::array::from_fn(|i| s[i] + c * d[i]);
        let k1 = f(st);
        let k2 = f(add(st, k1, 0.5 * h));
        let k3 = f(add(st, k2, 0.5 * h));
        let k4 = f(add(st, k3, h));
        st = std::array::from_fn(|i| st[i] + h / 6.0 * (k1[i] + 2.0 * k2[i] + 2.0 * k3[i] + k4[i]));
        if k % 100 == 0 {
            want.push(pos(st));
        }
    }
    let built = common::build_dynamic(&common::library(), &def);
    for rtol in [1e-6, 1e-8, 1e-10] {
        let opts = SolverOptions { rtol, atol: rtol * 1e-2, ..Default::default() };
        let grid = OutputGrid { t0: 0.0, t_end: 5.0, dt: 0.01 };
        let r = simulate(&built.jit, &built.info, &opts, grid, &mut [])
            .unwrap_or_else(|e| panic!("rtol {rtol:e}: {e}"));
        let ch = |name: &str| &r.values[r.names.iter().position(|x| x == name).expect(name)];
        let got = [ch("x1"), ch("y1"), ch("x2"), ch("y2")];
        let err = want
            .iter()
            .enumerate()
            .map(|(k, w)| (0..4).map(|i| (got[i][k] - w[i]).abs()).fold(0.0, f64::max))
            .fold(0.0, f64::max);
        println!("rtol {rtol:e}: {} switches, {:.0} rtol off", switches(&r), err / rtol);
        assert!(switches(&r) > 0);
        assert!(err < 1e3 * rtol, "rtol {rtol:e}: {err:e} off");
    }
}

/// A model's choices that give the model with the states chosen again but
/// tell its watch the choice it left (each set's pivots swapped), so that
/// no switch to it holds: the switch is refused only once the integrator
/// has been started on it, at the step's end it was found at.
struct Worse(lsim_solve::choice::Choices);

/// The model a [`Worse`] gives: the real one.
struct Real(std::sync::Arc<lsim_solve::choice::Variant>);

impl lsim_ir::runtime::ModelFunctions for Real {
    fn layout(&self) -> &lsim_ir::runtime::Layout {
        self.0.model.layout()
    }
    fn residual(&self, i: &lsim_ir::runtime::EvalInput<'_>, w: &mut [f64], o: &mut [f64]) {
        self.0.model.residual(i, w, o)
    }
    fn jvp(&self, i: &lsim_ir::runtime::EvalInput<'_>, v: &[f64], w: &mut [f64], o: &mut [f64]) {
        self.0.model.jvp(i, v, w, o)
    }
    fn roots(&self, i: &lsim_ir::runtime::EvalInput<'_>, w: &mut [f64], o: &mut [f64]) {
        self.0.model.roots(i, w, o)
    }
    fn vars(&self, i: &lsim_ir::runtime::EvalInput<'_>, w: &mut [f64], o: &mut [f64]) {
        self.0.model.vars(i, w, o)
    }
    fn when(&self, i: &lsim_ir::runtime::EvalInput<'_>, f: &[f64], w: &mut [f64], d: &mut [f64]) {
        self.0.model.when(i, f, w, d)
    }
    fn start(&self, p: &[f64], y0: &mut [f64], d0: &mut [f64]) {
        self.0.model.start(p, y0, d0)
    }
    fn sparsity(&self) -> Option<&lsim_ir::runtime::SparsityPattern> {
        self.0.model.sparsity()
    }
    fn jacobian_sparse(&self, i: &lsim_ir::runtime::EvalInput<'_>, w: &mut [f64], v: &mut [f64]) {
        self.0.model.jacobian_sparse(i, w, v)
    }
    fn modes(&self, i: &lsim_ir::runtime::EvalInput<'_>, w: &mut [f64], d: &mut [f64]) {
        self.0.model.modes(i, w, d)
    }
}

impl lsim_solve::choice::StateChoices for Worse {
    fn choose(
        &self,
        at: &lsim_solve::choice::ChoicePoint,
    ) -> Result<std::sync::Arc<lsim_solve::choice::Variant>, String> {
        let v = self.0.0.choose(at)?;
        let mut info = v.info.clone();
        for set in &mut info.state_sets {
            if set.pivots.len() == 1 && set.candidates.len() == 2 {
                set.pivots[0] = 1 - set.pivots[0];
            }
        }
        let states = v.states.clone();
        Ok(std::sync::Arc::new(lsim_solve::choice::Variant {
            model: Box::new(Real(v)),
            info,
            states,
        }))
    }
}

/// A switch refused once the integrator has been started on the new
/// states (here: the model's choices tell their watch the choice the run
/// leaves, so no switch to them holds) leaves the run as it was: the step
/// is not cut back to the point (a switch is readied before the step is
/// cut, and a refused one changes nothing), the step's end is checked as
/// any other, and the run stops where the run without the model's choices
/// stops, to the bit, naming the rod and why the states were not chosen
/// again. Before, a refusal in a step cut back to its point restarted the
/// integrator there, from the step's dense output: up to 1.4e-7 s off that
/// point at rtol 1e-10 (1 370 rtol), 3.0e-6 at 1e-8, where the run without
/// choices is 6e-10 and 7e-7 off the closed form's; and a refusal at a
/// step's end left that point's holds, kink and stale modes unchecked.
#[test]
fn a_refused_switch_leaves_the_run_as_it_was() {
    let lib = common::library();
    let def = pendulum(-std::f64::consts::FRAC_PI_2, StateSelect::Default);
    let built = common::build(&lib, &def, false);
    let mut worse = built.info.clone();
    worse.choices =
        Some(lsim_solve::choice::Choices(std::sync::Arc::new(Worse(common::choices(&lib, &def)))));
    let quarter = 1.854_074_677_301_372 / 9.81f64.sqrt();
    let want = quarter - time_from_bottom((2.0f64 * 9.81).sqrt(), lsim_ir::math::atan(0.01));
    let alone = stop_times(&built, &built.info, &Vec::new, 10.0, "x", "y");
    for (rtol, t0) in RTOLS.into_iter().zip(&alone) {
        let opts = SolverOptions { rtol, atol: rtol * 1e-2, ..Default::default() };
        let grid = OutputGrid { t0: 0.0, t_end: 10.0, dt: 0.01 };
        let e = simulate(&built.jit, &worse, &opts, grid, &mut []).expect_err("stops");
        let SolveError::StateSelection { t, message, .. } = &e else { panic!("{e}") };
        println!(
            "rtol {rtol:e}: stops {:+.2e} s from the closed form's point, without choices {:+.2e}",
            t - want,
            t0 - want
        );
        assert!(message.contains("choosing the states again did not work here"), "{e}");
        assert_eq!(t.to_bits(), t0.to_bits(), "rtol {rtol:e}: {t} against {t0}: {e}");
    }
}

/// A model's choices are shared by the runs of a sweep: two runs that
/// choose the same states at once (both compiling it, outside the lock)
/// get the same model, kept once. Before, each kept its own.
#[test]
fn a_choice_two_runs_compile_at_once_is_kept_once() {
    use lsim_solve::choice::{ChoicePoint, Rechooser, StateChoices};
    use std::sync::{Arc, Barrier};
    let lib = common::library();
    let def = pendulum(-std::f64::consts::FRAC_PI_2, StateSelect::Default);
    let first = common::build(&lib, &def, false);
    let (l2, d2) = (lib.clone(), def.clone());
    let prepare = move |at: &ChoicePoint| {
        lsim_prep::prepare_choosing(&l2, &d2, &Default::default(), at.clone())
            .map_err(|d| format!("{d:?}"))
    };
    // (both compile before either keeps its model)
    let both = Arc::new(Barrier::new(2));
    let gate = both.clone();
    let compile = move |p: &lsim_ir::PreparedModel| {
        gate.wait();
        lsim_codegen::compile(p, &Default::default())
            .map(|j| Box::new(j) as Box<dyn lsim_ir::runtime::ModelFunctions>)
            .map_err(|e| e.to_string())
    };
    let chooser = Arc::new(Rechooser::new(Box::new(prepare), Box::new(compile)));
    let values = [("x", 0.0), ("y", -1.0), ("vx", 4.43), ("vy", 0.0), ("F", 29.4)];
    let at = ChoicePoint {
        t: 0.5,
        values: values.iter().map(|(n, v)| (n.to_string(), *v)).collect(),
        params: first.info.params.clone(),
        window: lsim_solve::STATE_SWITCH,
        ..Default::default()
    };
    let runs: Vec<_> = (0..2)
        .map(|_| {
            let (c, at) = (chooser.clone(), at.clone());
            std::thread::spawn(move || c.choose(&at).expect("a choice"))
        })
        .collect();
    let got: Vec<_> = runs.into_iter().map(|h| h.join().expect("ran")).collect();
    assert_eq!(got[0].states, ["x", "vx"]);
    assert!(Arc::ptr_eq(&got[0], &got[1]), "two models for one choice");
    assert_eq!(chooser.kept().len(), 1);
}
