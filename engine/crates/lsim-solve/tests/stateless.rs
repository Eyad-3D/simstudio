//! A model with no state (nothing to integrate: every variable a function
//! of time, the parameters and the discrete values) runs as any other
//! does: its outputs on the grid, a `when` on time and one on an
//! algebraic condition, a sampled block whose output feeds the circuit,
//! its energy books; against the exact answers, on both backends.

mod common;

use lsim_ir::component::build::{connect, discrete, eq, param, sub};
use lsim_ir::expr::{Builtin, CmpOp, c, call, cmp, name as n};
use lsim_ir::runtime::DiscreteBlock;
use lsim_ir::{ComponentDef, Equation, EquationDecl, PortDecl, PortKind, WhenAction};
use lsim_solve::{Backend, EventKind, OutputGrid, SolverOptions, simulate};
use std::f64::consts::PI;

const AMP: f64 = 10.0; // the source's amplitude, V
const W: f64 = 5.0; // its angular frequency, rad/s
const T_STEP: f64 = 0.7; // when the resistance steps from 2 to 4 Ω
const PERIOD: f64 = 0.25; // the sampled block's
const T_END: f64 = 2.0;

fn signal(name: &str, output: bool, unit: &str) -> PortDecl {
    let unit = unit.to_string();
    PortDecl {
        name: name.into(),
        kind: if output { PortKind::Output { unit } } else { PortKind::Input { unit } },
        doc: String::new(),
    }
}

/// The test's own parts: a sine source signal, a resistance that steps at
/// `t_step`, and a sampled block (its outputs set by the host at its ticks).
fn parts() -> Vec<ComponentDef> {
    vec![
        ComponentDef {
            name: "Test.Sine".into(),
            ports: vec![signal("y", true, "V")],
            params: vec![param("A", "V", AMP, ""), param("w", "rad/s", W, "")],
            equations: vec![eq(
                n("y"),
                n("A") * call(Builtin::Sin, vec![n("w") * lsim_ir::Expr::Time]),
                "a sine of time",
            )],
            ..Default::default()
        },
        ComponentDef {
            name: "Test.SteppedResistance".into(),
            ports: vec![signal("y", true, "Ohm")],
            params: vec![
                param("R0", "Ohm", 2.0, ""),
                param("R1", "Ohm", 4.0, ""),
                param("t_step", "s", T_STEP, ""),
            ],
            vars: vec![discrete("after", "1", 0.0, "")],
            equations: vec![
                eq(n("y"), n("R0") + (n("R1") - n("R0")) * n("after"), "before or after"),
                EquationDecl {
                    eq: Equation::When {
                        condition: cmp(CmpOp::Ge, lsim_ir::Expr::Time, n("t_step")),
                        actions: vec![WhenAction::Assign { var: "after".into(), value: c(1.0) }],
                    },
                    label: Some("it steps".into()),
                },
            ],
            ..Default::default()
        },
        ComponentDef {
            name: "Test.PhaseCounter".into(),
            params: vec![param("w", "rad/s", W, ""), param("phase", "1", 1.0, "")],
            vars: vec![discrete("count", "1", 0.0, "")],
            equations: vec![EquationDecl {
                eq: Equation::When {
                    // the phase of the sine, wrapped to (-π, π]: atan2 is
                    // not searched ahead, so root finding has it
                    condition: cmp(
                        CmpOp::Ge,
                        call(
                            Builtin::Atan2,
                            vec![
                                call(Builtin::Sin, vec![n("w") * lsim_ir::Expr::Time]),
                                call(Builtin::Cos, vec![n("w") * lsim_ir::Expr::Time]),
                            ],
                        ),
                        n("phase"),
                    ),
                    actions: vec![WhenAction::Assign {
                        var: "count".into(),
                        value: call(Builtin::Pre, vec![n("count")]) + c(1.0),
                    }],
                },
                label: Some("the phase passes 1 rad".into()),
            }],
            ..Default::default()
        },
        ComponentDef {
            name: "External.Sampler".into(),
            ports: vec![signal("u", false, "A"), signal("y", true, "V")],
            params: vec![param("period", "s", PERIOD, "")],
            ..Default::default()
        },
    ]
}

/// A sine source across a resistor whose resistance steps, a level sensor
/// on the source's voltage, and a sampled block that holds 2 Ω times the
/// resistor's current (as an ammeter reads it) as the voltage of a second
/// source across 1 Ω.
fn circuit() -> ComponentDef {
    ComponentDef {
        name: "Test.NoState".into(),
        components: vec![
            sub("sine", "Test.Sine", &[]),
            sub("src", "Electrical.SignalVoltage", &[]),
            sub("rv", "Electrical.VariableResistor", &[]),
            sub("meter", "Electrical.CurrentSensor", &[]),
            sub("steps", "Test.SteppedResistance", &[]),
            sub("lvl", "Test.VoltageLevel", &[("level", c(0.5 * AMP))]),
            sub("hold", "External.Sampler", &[]),
            sub("phase", "Test.PhaseCounter", &[]),
            sub("src2", "Electrical.SignalVoltage", &[]),
            sub("r2", "Electrical.Resistor", &[("R", c(1.0))]),
            sub("gnd", "Electrical.Ground", &[]),
        ],
        connections: vec![
            connect("sine.y", "src.v_in"),
            connect("steps.y", "rv.R_in"),
            connect("src.p", "meter.p"),
            connect("meter.n", "rv.p"),
            connect("src.p", "lvl.p"),
            connect("src.n", "rv.n"),
            connect("src.n", "lvl.n"),
            connect("src.n", "gnd.p"),
            connect("meter.i", "hold.u"),
            connect("hold.y", "src2.v_in"),
            connect("src2.p", "r2.p"),
            connect("src2.n", "r2.n"),
            connect("src2.n", "gnd.p"),
        ],
        ..Default::default()
    }
}

/// Holds 2 Ω times its input.
struct Hold;

impl DiscreteBlock for Hold {
    fn name(&self) -> &str {
        "hold"
    }
    fn period(&self) -> f64 {
        PERIOD
    }
    fn init(&mut self, _t: f64, i: &[f64], o: &mut [f64]) -> Result<(), String> {
        o[0] = 2.0 * i[0];
        Ok(())
    }
    fn tick(&mut self, _t: f64, i: &[f64], o: &mut [f64]) -> Result<(), String> {
        o[0] = 2.0 * i[0];
        Ok(())
    }
}

fn resistance(t: f64) -> f64 {
    if t < T_STEP { 2.0 } else { 4.0 }
}

/// The exact current through the stepped resistor.
fn current(t: f64) -> f64 {
    AMP * lsim_ir::math::sin(W * t) / resistance(t)
}

/// ∫ₐᵇ of the current through the stepped resistor.
fn charge(a: f64, b: f64) -> f64 {
    // -A cos(w s) / (w R) on each side of the step
    let g = |s: f64, r: f64| -AMP * lsim_ir::math::cos(W * s) / (W * r);
    if b <= T_STEP || a >= T_STEP {
        let r = resistance(a);
        g(b, r) - g(a, r)
    } else {
        g(T_STEP, 2.0) - g(a, 2.0) + g(b, 4.0) - g(T_STEP, 4.0)
    }
}

/// ∫₀ᵗ v² / R: the stepped resistor's loss.
fn loss(t: f64) -> f64 {
    let f = |s: f64| AMP * AMP * (s / 2.0 - lsim_ir::math::sin(2.0 * W * s) / (4.0 * W));
    if t < T_STEP { f(t) / 2.0 } else { f(T_STEP) / 2.0 + (f(t) - f(T_STEP)) / 4.0 }
}

/// The block's held voltage at `t` (the value after a tick at `t`).
fn held(t: f64) -> f64 {
    let k = ((t + 1e-12) / PERIOD).floor();
    2.0 * current(k * PERIOD)
}

/// ∫₀ᵗ held²: the second resistor's loss (1 Ω).
fn held_loss(t: f64) -> f64 {
    let mut acc = 0.0;
    let mut a = 0.0;
    while a < t {
        let b = (a + PERIOD).min(t);
        acc += held(a) * held(a) * (b - a);
        a += PERIOD;
    }
    acc
}

#[test]
fn a_model_with_no_state_runs_as_any_other() {
    let mut lib = common::library();
    for part in parts() {
        lib.add(part);
    }
    let built = common::build(&lib, &circuit(), false);
    let l = *lsim_ir::runtime::ModelFunctions::layout(&built.jit);
    assert_eq!((l.n_x, l.n_z), (0, 0), "the model has no state");
    let grid = OutputGrid { t0: 0.0, t_end: T_END, dt: 0.1 };
    for backend in [Backend::Sundials, Backend::Diffsol] {
        let opts = SolverOptions { rtol: 1e-8, atol: 1e-10, backend, ..Default::default() };
        let mut blocks: Vec<Box<dyn DiscreteBlock>> = vec![Box::new(Hold)];
        let run = simulate(&built.jit, &built.info, &opts, grid, &mut blocks)
            .unwrap_or_else(|e| panic!("{backend:?}: {e}"));
        println!(
            "{backend:?}: {} ({}), {} steps, {} channel evaluations, {} restarts, {} events",
            run.backend,
            run.report.method,
            run.stats.steps,
            run.stats.rhs_evals,
            run.stats.restarts,
            run.events.len()
        );
        // the same shape as any run: every channel on every output time
        assert_eq!(run.times.len(), 21);
        assert_eq!(run.times.last(), Some(&T_END));
        for v in [&run.values, &run.min, &run.max, &run.mean] {
            assert_eq!(v.len(), run.names.len());
            assert!(v.iter().all(|c| c.len() == run.times.len()));
        }
        let ch = |name: &str| run.channel(name).unwrap_or_else(|| panic!("no channel {name}"));
        let at = |k: usize| run.times[k];
        // the outputs: the model's own values at the output times
        let mut worst: f64 = 0.0;
        for k in 0..run.times.len() {
            let t = at(k);
            worst = worst.max((ch("src.v")[k] - AMP * lsim_ir::math::sin(W * t)).abs() / AMP);
            worst = worst.max((ch("rv.i")[k] - current(t)).abs() / AMP);
            worst = worst.max((ch("r2.i")[k] - held(t)).abs() / AMP);
        }
        println!("  outputs: worst {worst:.1e} of the amplitude");
        assert!(worst < 1e-14, "{worst:e}");
        // the `when` on time at its time exactly, the one on the source's
        // voltage (half its amplitude) where the sine crosses it
        let times = |label: &str| -> Vec<f64> {
            run.events.iter().filter(|e| e.label.contains(label)).map(|e| e.t).collect()
        };
        assert_eq!(times("it steps"), vec![T_STEP]);
        let levels = times("the voltage reaches its level");
        let exact = [PI / 30.0, PI / 30.0 + 2.0 * PI / W];
        println!("  level events at {levels:?} (exact {exact:?})");
        assert_eq!(levels.len(), 2);
        for (got, want) in levels.iter().zip(exact) {
            assert!((got - want).abs() <= 4.0 * f64::EPSILON * want, "{got} against {want}");
        }
        // the one root finding has (on the model's own functions: exact to
        // its tolerance, 100 ε (|t| + |h|)), and only as it rises
        let phases = times("the phase passes 1 rad");
        let exact = [1.0 / W, (1.0 + 2.0 * PI) / W];
        println!("  phase events at {phases:?} (exact {exact:?})");
        assert_eq!(phases.len(), 2);
        for (got, want) in phases.iter().zip(exact) {
            assert!(*got >= want && got - want <= 400.0 * f64::EPSILON * T_END, "{got} {want}");
        }
        assert_eq!(*ch("phase.count").last().unwrap(), 2.0);
        assert!(
            run.report.warnings.iter().any(|w| w.contains("atan2")),
            "{:?}",
            run.report.warnings
        );
        let reached = ch("lvl.reached");
        assert_eq!(reached[1], 0.0);
        assert_eq!(reached[2], 1.0, "latched at 0.105 s");
        // the sampled block: a tick every 0.25 s, its output feeding r2
        let ticks = run.events.iter().filter(|e| matches!(e.kind, EventKind::Block(_))).count();
        assert_eq!(run.report.block_ticks, 9, "ticks at 0, 0.25, …, 2.0");
        assert!(ticks >= 7, "{ticks} ticks changed the held voltage");
        // the recorder's interval statistics, from the values at the steps'
        // ends (as for any model: the steps follow the channels here as an
        // integrator's follow its states, so they are only as fine as a
        // fifth-order method's): every one a value the current takes in
        // the interval, the mean the trapezoid over them
        let col = |name: &str| run.names.iter().position(|x| x == name).expect("a channel");
        let i = col("rv.i");
        let (mut w_mean, mut w_max, mut w_min) = (0.0f64, 0.0f64, 0.0f64);
        for k in 1..run.times.len() {
            let (a, b) = (at(k - 1), at(k));
            let mean = charge(a, b) / (b - a);
            w_mean = w_mean.max((run.mean[i][k] - mean).abs() / AMP);
            let mut fine: Vec<f64> =
                (0..=1000).map(|j| current(a + (b - a) * j as f64 / 1000.0)).collect();
            if a < T_STEP && T_STEP <= b {
                // (and its left limit at the step)
                fine.push(AMP * lsim_ir::math::sin(W * T_STEP) / 2.0);
            }
            let hi = fine.iter().fold(f64::NEG_INFINITY, |m, x| m.max(*x));
            let lo = fine.iter().fold(f64::INFINITY, |m, x| m.min(*x));
            assert!(run.max[i][k] <= hi + 1e-12 && run.min[i][k] >= lo - 1e-12);
            w_max = w_max.max((hi - run.max[i][k]) / AMP);
            w_min = w_min.max((run.min[i][k] - lo) / AMP);
        }
        println!(
            "  interval means within {w_mean:.1e}, maxima within {w_max:.1e}, minima within \
             {w_min:.1e} of the amplitude"
        );
        assert!(w_mean < 5e-3 && w_max < 5e-3 && w_min < 5e-3, "{w_mean:e} {w_max:e} {w_min:e}");
        // the energy books: each resistor's loss exactly, and closed
        let e = run.energy.as_ref().expect("books");
        let part = |path: &str| e.parts.iter().find(|p| p.path == path).expect("a part");
        let (got, want) = (part("rv").lost, loss(T_END));
        let (got2, want2) = (part("r2").lost, held_loss(T_END));
        println!(
            "  books: rv lost {got:.12} J ({:.1e} off), r2 {got2:.12} J ({:.1e} off), closure \
             {:.1e}",
            (got - want).abs() / want,
            (got2 - want2).abs() / want2,
            e.relative_closure
        );
        assert!((got - want).abs() < 1e-9 * want, "{got} against {want}");
        assert!((got2 - want2).abs() < 1e-9 * want2, "{got2} against {want2}");
        assert!(e.relative_closure < 1e-9, "{}", e.relative_closure);
    }
}

/// A channel no step resolves at the start, a jump at t0 that no event
/// marks (`y = if noEvent(time > 0) then 1 else 0`): the steps halve down
/// to the time's resolution and grow again. That resolution is 16 ulps of
/// the run's length too; before, at t = 0 it fell with the step itself,
/// down to 1e-322 s (1 071 steps and 12 853 evaluations of the channels
/// where the same jump at 0.5 s takes 51 steps).
#[test]
fn a_channel_no_step_resolves_at_the_start_halves_to_the_run_s_resolution() {
    use lsim_ir::Expr;
    for at in [0.0, 0.5] {
        let m = ComponentDef {
            name: "Test.JumpAtStart".into(),
            params: vec![param("t_jump", "s", at, "where y jumps")],
            vars: vec![lsim_ir::component::build::var("y", "1", "0, then 1")],
            equations: vec![eq(
                n("y"),
                Expr::If(
                    Box::new(Expr::NoEvent(Box::new(cmp(CmpOp::Gt, Expr::Time, n("t_jump"))))),
                    Box::new(c(1.0)),
                    Box::new(c(0.0)),
                ),
                "a jump no event marks",
            )],
            ..Default::default()
        };
        let built = common::build(&common::library(), &m, false);
        let grid = OutputGrid { t0: 0.0, t_end: T_END, dt: 0.25 };
        for rtol in [1e-4, 1e-8] {
            let opts = SolverOptions { rtol, atol: 1e-2 * rtol, ..Default::default() };
            let run = simulate(&built.jit, &built.info, &opts, grid, &mut []).expect("runs");
            let y = run.channel("y").unwrap();
            println!("jump at {at} s, rtol {rtol:e}: {} steps", run.stats.steps);
            for (t, y) in run.times.iter().zip(y) {
                assert_eq!(*y, if *t > at { 1.0 } else { 0.0 }, "y at {t}");
            }
            assert!(run.stats.steps < 150, "jump at {at} s: {} steps", run.stats.steps);
        }
    }
}
