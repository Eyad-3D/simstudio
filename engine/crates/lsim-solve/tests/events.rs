//! The run loop on hand-written models with exact answers: modes (a rotor
//! that sticks when Coulomb friction stops it: `mech_inertia_coastdown`
//! from the reference suite), time events, event iteration across chained
//! `when` clauses, event storms, sampled blocks (one that changes nothing,
//! one that changes its output at every tick), the 10× tighter check,
//! parallel sweeps and the initialisation's homotopy; the order contract
//! of the compiled roots; a condition a table of time drives, not stepped
//! over.

mod common;

use common::Problem;
use lsim_ir::expr::Expr;
use lsim_ir::prepared::Direction;
use lsim_ir::runtime::{DiscreteBlock, EvalInput, Layout, ModelFunctions};
use lsim_ir::{ParamId, VarId};
use lsim_solve::{
    Backend, BlockInfo, EnergyInfo, EnergyPart, EventKind, ModeInfo, OutputGrid, RunInfo,
    SolveError, SolverOptions, VarSource, accuracy_check, simulate, sweep,
};
use std::sync::Arc;

type F = Box<dyn Fn(&EvalInput<'_>, &mut [f64]) + Send + Sync>;
type FV = Box<dyn Fn(&EvalInput<'_>, &[f64], &mut [f64]) + Send + Sync>;

/// A model written by hand.
struct Hand {
    layout: Layout,
    f: F,
    jvp: FV,
    roots: F,
    vars: F,
    when: FV,
    y0: Vec<f64>,
    d0: Vec<f64>,
}

impl ModelFunctions for Hand {
    fn layout(&self) -> &Layout {
        &self.layout
    }
    fn residual(&self, inp: &EvalInput<'_>, _w: &mut [f64], out: &mut [f64]) {
        (self.f)(inp, out)
    }
    fn jvp(&self, inp: &EvalInput<'_>, v: &[f64], _w: &mut [f64], out: &mut [f64]) {
        (self.jvp)(inp, v, out)
    }
    fn roots(&self, inp: &EvalInput<'_>, _w: &mut [f64], out: &mut [f64]) {
        (self.roots)(inp, out)
    }
    fn vars(&self, inp: &EvalInput<'_>, _w: &mut [f64], out: &mut [f64]) {
        (self.vars)(inp, out)
    }
    fn when(&self, inp: &EvalInput<'_>, fired: &[f64], _w: &mut [f64], d_out: &mut [f64]) {
        (self.when)(inp, fired, d_out)
    }
    fn start(&self, _p: &[f64], y0: &mut [f64], d0: &mut [f64]) {
        y0.copy_from_slice(&self.y0);
        d0.copy_from_slice(&self.d0);
    }
}

fn layout(
    n_x: usize,
    n_p: usize,
    n_d: usize,
    n_roots: usize,
    n_whens: usize,
    n_vars: usize,
) -> Layout {
    Layout { n_x, n_z: 0, n_p, n_d, n_u: 0, n_roots, n_whens, n_vars, n_work: 1 }
}

fn nothing() -> F {
    Box::new(|_, _| {})
}

fn nothing_v() -> FV {
    Box::new(|_, _, _| {})
}

fn v(k: u32) -> Expr {
    Expr::Var(VarId(k))
}

fn backends() -> Vec<Backend> {
    let mut b = vec![Backend::Sundials];
    if cfg!(feature = "diffsol") {
        b.push(Backend::Diffsol);
    }
    b
}

/// A rotor coasting against viscous and Coulomb friction; a mode holds it
/// once stopped (`mech_inertia_coastdown`).
fn coastdown(p: &Problem) -> (Hand, RunInfo) {
    let params = vec![p.p("J"), p.p("c"), p.p("T_c")];
    let model = Hand {
        // y = [ω, θ]; d = [moving]; vars = [ω, θ, moving, c·ω, T_c·moving]
        layout: layout(2, 3, 1, 1, 0, 5),
        f: Box::new(|i, out| {
            let (j, c, tc) = (i.p[0], i.p[1], i.p[2]);
            out[0] = i.d[0] * (-c * i.y[0] - tc) / j;
            out[1] = i.y[0];
        }),
        jvp: Box::new(|i, v, out| {
            out[0] = i.d[0] * (-i.p[1] / i.p[0]) * v[0];
            out[1] = v[0];
        }),
        // moving: the speed; stuck: the torque margin (nothing drives it)
        roots: Box::new(|i, out| out[0] = if i.d[0] != 0.0 { i.y[0] } else { -i.p[2] }),
        vars: Box::new(|i, out| {
            out[0] = i.y[0];
            out[1] = i.y[1];
            out[2] = i.d[0];
            out[3] = i.p[1] * i.y[0];
            out[4] = i.p[2] * i.d[0];
        }),
        when: nothing_v(),
        y0: vec![p.initial["omega"], p.initial["theta"]],
        d0: vec![1.0],
    };
    let mut info = RunInfo::bare(2, 5, params);
    info.var_names =
        ["omega", "theta", "moving", "tau_visc", "tau_coul"].map(String::from).to_vec();
    info.root_dirs = vec![0];
    info.modes = vec![ModeInfo { crossing: 0, discrete: 0, label: "'Rotor': stuck".into() }];
    info.y_nominal = vec![300.0, 1000.0];
    let j = Expr::Param(ParamId(0));
    info.energy = Some(Arc::new(EnergyInfo {
        parts: vec![
            EnergyPart {
                path: "rotor".into(),
                name: "'Rotor'".into(),
                power: -((v(3) + v(4)) * v(0)),
                loss: None,
                stored: Some(Expr::Const(0.5) * j * v(0) * v(0)),
            },
            EnergyPart {
                path: "viscous".into(),
                name: "'Viscous friction'".into(),
                power: v(3) * v(0),
                loss: Some(v(3) * v(0)),
                stored: None,
            },
            EnergyPart {
                path: "coulomb".into(),
                name: "'Coulomb friction'".into(),
                power: v(4) * v(0),
                loss: Some(v(4) * v(0)),
                stored: None,
            },
        ],
    }));
    (model, info)
}

#[test]
fn a_mode_holds_the_rotor_once_coulomb_friction_stops_it() {
    let p = Problem::load("mech_inertia_coastdown");
    let (model, info) = coastdown(&p);
    let grid = OutputGrid { t0: 0.0, t_end: p.t_end, dt: p.output_dt };
    let t_stop = p.events["t_stop"];
    for backend in backends() {
        for rtol in [1e-6, 1e-8, 1e-10] {
            let opts = SolverOptions { rtol, atol: rtol, backend, ..Default::default() };
            let run = simulate(&model, &info, &opts, grid, &mut []).expect("runs");
            let stops: Vec<_> =
                run.events.iter().filter(|e| matches!(e.kind, EventKind::Mode(0))).collect();
            assert_eq!(stops.len(), 1, "one stop: {:?}", run.events);
            let dt = (stops[0].t - t_stop).abs();
            let mut worst: f64 = 0.0;
            for (name, ch) in [("omega", 0), ("theta", 1)] {
                let exact = &p.exact[name];
                let scale = exact.iter().fold(0.0f64, |a, b| a.max(b.abs()));
                for (k, tc) in p.times.iter().enumerate() {
                    let i = run.times.iter().position(|t| (t - tc).abs() < 1e-9).unwrap();
                    worst = worst.max((run.values[ch][i] - exact[k]).abs() / scale);
                }
            }
            let books = run.energy.as_ref().unwrap();
            let lost = |path: &str| books.parts.iter().find(|b| b.path == path).unwrap().lost;
            let e_scale = 22500.0;
            let e_err = [
                (lost("viscous") - p.exact["E_viscous"][3]).abs(),
                (lost("coulomb") - p.exact["E_coulomb"][3]).abs(),
                (books.parts[0].stored_change - p.exact["E_kin"][3]).abs(),
            ]
            .iter()
            .fold(0.0f64, |a, b| a.max(*b))
                / e_scale;
            println!(
                "{:<30} rtol {rtol:.0e}: stop off by {dt:.1e} s ({:.1} rtol), signals {worst:.1e}, energies {e_err:.1e}, closure {:.1e}, drift {:.1e}, {} steps",
                run.backend,
                dt / (rtol * t_stop),
                books.relative_closure,
                books.relative_drift,
                run.stats.steps
            );
            assert!(dt <= 10.0 * rtol * t_stop, "stop time off by {dt:e}");
            assert!(worst <= 100.0 * rtol, "signals off by {worst:e}");
            assert!(e_err <= 100.0 * rtol, "energies off by {e_err:e}");
            assert!(books.relative_closure <= 1e-6, "closure {}", books.relative_closure);
            // it stays stopped
            assert!(run.values[0].last().unwrap().abs() < 1e-6 * 300.0);
        }
    }
}

#[test]
fn a_time_event_is_reached_exactly() {
    // y' = 1 before t = 1, -2 after: a kink the integrator must not step over
    let model = Hand {
        layout: layout(1, 0, 0, 0, 0, 1),
        f: Box::new(|i, out| out[0] = if i.t < 1.0 { 1.0 } else { -2.0 }),
        jvp: Box::new(|_, _, out| out[0] = 0.0),
        roots: nothing(),
        vars: Box::new(|i, out| out[0] = i.y[0]),
        when: nothing_v(),
        y0: vec![0.0],
        d0: vec![],
    };
    let mut info = RunInfo::bare(1, 1, vec![]);
    info.time_events = vec![1.0];
    for backend in backends() {
        let opts = SolverOptions { rtol: 1e-8, atol: 1e-10, backend, ..Default::default() };
        let run =
            simulate(&model, &info, &opts, OutputGrid { t0: 0.0, t_end: 2.0, dt: 0.25 }, &mut [])
                .unwrap();
        let ev: Vec<_> = run.events.iter().filter(|e| e.kind == EventKind::Time(0)).collect();
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].t, 1.0, "exactly at the time event");
        for (k, t) in run.times.iter().enumerate() {
            let exact = if *t <= 1.0 { *t } else { 1.0 - 2.0 * (t - 1.0) };
            assert!((run.values[0][k] - exact).abs() < 1e-9, "{backend:?} t = {t}");
        }
    }
}

#[test]
fn event_iteration_fires_a_chain_of_whens_at_one_instant() {
    // y' = 1 - 2 d1; when y >= 1: d0 := 1; when d0 >= 0.5: d1 := 1
    let model = Hand {
        layout: layout(1, 0, 2, 2, 2, 3),
        f: Box::new(|i, out| out[0] = 1.0 - 2.0 * i.d[1]),
        jvp: Box::new(|_, _, out| out[0] = 0.0),
        roots: Box::new(|i, out| {
            out[0] = i.y[0] - 1.0;
            out[1] = i.d[0] - 0.5;
        }),
        vars: Box::new(|i, out| {
            out[0] = i.y[0];
            out[1] = i.d[0];
            out[2] = i.d[1];
        }),
        when: Box::new(|_, fired, d| {
            if fired[0] != 0.0 {
                d[0] = 1.0;
            }
            if fired[1] != 0.0 {
                d[1] = 1.0;
            }
        }),
        y0: vec![0.0],
        d0: vec![0.0, 0.0],
    };
    let mut info = RunInfo::bare(1, 3, vec![]);
    info.whens = vec![(0, Direction::Rising), (1, Direction::Rising)];
    info.root_dirs = vec![1, 1];
    info.when_labels = vec!["first".into(), "second".into()];
    for backend in backends() {
        let opts = SolverOptions { rtol: 1e-9, atol: 1e-12, backend, ..Default::default() };
        let run =
            simulate(&model, &info, &opts, OutputGrid { t0: 0.0, t_end: 2.0, dt: 0.5 }, &mut [])
                .unwrap();
        let labels: Vec<_> = run.events.iter().map(|e| e.label.as_str()).collect();
        assert_eq!(labels, ["first", "second"], "{backend:?}: {:?}", run.events);
        assert!((run.events[0].t - 1.0).abs() < 1e-8);
        assert_eq!(run.events[0].t, run.events[1].t, "one instant");
        // y rises to 1, then falls at rate 1: y(2) = 0
        assert!((run.values[0].last().unwrap()).abs() < 1e-7, "{backend:?}");
        assert_eq!(*run.values[2].last().unwrap(), 1.0);
    }
}

#[test]
fn a_chattering_mode_is_named_in_an_event_storm() {
    // an ideal relay: y' = -1 while y > 0, +1 otherwise; at y = 0 it
    // chatters
    let model = Hand {
        layout: layout(1, 0, 1, 1, 0, 1),
        f: Box::new(|i, out| out[0] = if i.d[0] != 0.0 { -1.0 } else { 1.0 }),
        jvp: Box::new(|_, _, out| out[0] = 0.0),
        roots: Box::new(|i, out| out[0] = i.y[0]),
        vars: Box::new(|i, out| out[0] = i.y[0]),
        when: nothing_v(),
        y0: vec![1.0],
        d0: vec![1.0],
    };
    let mut info = RunInfo::bare(1, 1, vec![]);
    info.root_dirs = vec![0];
    info.modes = vec![ModeInfo { crossing: 0, discrete: 0, label: "'Relay': on".into() }];
    for backend in backends() {
        let opts = SolverOptions { backend, ..Default::default() };
        let err =
            simulate(&model, &info, &opts, OutputGrid { t0: 0.0, t_end: 2.0, dt: 0.1 }, &mut [])
                .expect_err("chatters");
        println!("{backend:?}: {err}");
        match err {
            SolveError::EventStorm { t, parts, .. } => {
                assert!((1.0..1.01).contains(&t), "{t}");
                assert_eq!(parts, vec!["'Relay': on".to_string()]);
            }
            other => panic!("not a storm: {other}"),
        }
    }
}

/// A sampled block: reads its inputs, applies `law` to set its outputs.
struct Sampled {
    period: f64,
    law: fn(f64, &[f64], &mut [f64]),
    ticks: usize,
}

impl DiscreteBlock for Sampled {
    fn name(&self) -> &str {
        "'Controller'"
    }
    fn period(&self) -> f64 {
        self.period
    }
    fn init(&mut self, _t: f64, _i: &[f64], _o: &mut [f64]) -> Result<(), String> {
        self.ticks = 0;
        Ok(())
    }
    fn tick(&mut self, t: f64, i: &[f64], o: &mut [f64]) -> Result<(), String> {
        self.ticks += 1;
        (self.law)(t, i, o);
        Ok(())
    }
}

/// y' = u - a y with u a discrete variable a block sets from y.
fn held(a: f64) -> (Hand, RunInfo) {
    let model = Hand {
        layout: layout(1, 1, 1, 0, 0, 2),
        f: Box::new(|i, out| out[0] = i.d[0] - i.p[0] * i.y[0]),
        jvp: Box::new(|i, v, out| out[0] = -i.p[0] * v[0]),
        roots: nothing(),
        vars: Box::new(|i, out| {
            out[0] = i.y[0];
            out[1] = i.d[0];
        }),
        when: nothing_v(),
        y0: vec![1.0],
        d0: vec![0.0],
    };
    let mut info = RunInfo::bare(1, 2, vec![a]);
    info.var_sources = vec![VarSource::Y(0), VarSource::D(0)];
    info.blocks = vec![BlockInfo {
        name: "'Controller'".into(),
        inputs: vec![0],
        outputs: vec![0],
        period: 0.01,
        chains: vec![None],
    }];
    (model, info)
}

#[test]
fn a_block_that_changes_nothing_leaves_the_solution_and_the_steps_alone() {
    let (model, info) = held(0.3);
    let mut plain = info.clone();
    plain.blocks.clear();
    let grid = OutputGrid { t0: 0.0, t_end: 10.0, dt: 0.1 };
    for backend in backends() {
        let opts = SolverOptions { rtol: 1e-8, atol: 1e-10, backend, ..Default::default() };
        let without = simulate(&model, &plain, &opts, grid, &mut []).unwrap();
        let mut blocks: Vec<Box<dyn DiscreteBlock>> =
            vec![Box::new(Sampled { period: 0.01, law: |_, _, o| o[0] = 0.0, ticks: 0 })];
        let with = simulate(&model, &info, &opts, grid, &mut blocks).unwrap();
        assert_eq!(with.values, without.values, "{backend:?}: identical results");
        assert_eq!(with.stats.steps, without.stats.steps, "{backend:?}: no extra steps");
        assert_eq!(with.report.block_ticks, 1001, "a tick every 10 ms, both ends included");
        assert_eq!(with.report.block_changes, 0);
        assert!(with.events.is_empty());
    }
}

#[test]
fn a_sample_and_hold_controller_matches_its_discrete_time_solution() {
    // y' = u, u_k = -K y(t_k) held for T: y(t_k) = (1 - K T)^k exactly
    let (model, info) = held(0.0);
    let mut info = info;
    info.blocks[0].period = 0.1;
    let grid = OutputGrid { t0: 0.0, t_end: 2.0, dt: 0.1 };
    for backend in backends() {
        let opts = SolverOptions { rtol: 1e-9, atol: 1e-12, backend, ..Default::default() };
        let mut blocks: Vec<Box<dyn DiscreteBlock>> =
            vec![Box::new(Sampled { period: 0.1, law: |_, i, o| o[0] = -2.0 * i[0], ticks: 0 })];
        let run = simulate(&model, &info, &opts, grid, &mut blocks).unwrap();
        let mut worst: f64 = 0.0;
        for (k, t) in run.times.iter().enumerate() {
            let exact = 0.8f64.powi((t / 0.1).round() as i32);
            worst = worst.max((run.values[0][k] - exact).abs());
        }
        println!(
            "{backend:?}: worst {worst:.1e}, {} ticks, {} changed, {} steps, {} restarts",
            run.report.block_ticks, run.report.block_changes, run.stats.steps, run.stats.restarts
        );
        assert!(worst < 1e-9, "{backend:?}: {worst:e}");
        assert_eq!(run.report.block_ticks, 21);
        assert_eq!(run.report.block_changes, 21, "every tick changes the output");
    }
}

#[test]
fn the_tighter_check_estimates_the_error_it_reports() {
    let p = Problem::load("mech_inertia_coastdown");
    let (model, info) = coastdown(&p);
    let grid = OutputGrid { t0: 0.0, t_end: p.t_end, dt: p.output_dt };
    let opts = SolverOptions { rtol: 1e-5, atol: 1e-5, ..Default::default() };
    let check = accuracy_check(&model, &info, &opts, grid, &mut []).unwrap();
    println!("{}", check.summary());
    let omega = check.channels.iter().find(|c| c.name == "omega").unwrap();
    // the true error of the base run at the checkpoints
    let mut err: f64 = 0.0;
    for (k, tc) in p.times.iter().enumerate() {
        let i = check.base.times.iter().position(|t| (t - tc).abs() < 1e-9).unwrap();
        err = err.max((check.base.values[0][i] - p.exact["omega"][k]).abs());
    }
    println!("omega moved {:.3e}, its error at the checkpoints {err:.3e}", omega.max_abs);
    assert!(check.same_events);
    assert!(omega.max_abs >= 0.3 * err, "the movement must not hide the error");
    assert!(omega.max_abs <= 30.0 * err.max(1e-12) || omega.relative < 1e-4);
}

#[test]
fn a_sweep_gives_the_same_runs_as_one_by_one() {
    let p = Problem::load("mech_inertia_coastdown");
    let (model, info) = coastdown(&p);
    let grid = OutputGrid { t0: 0.0, t_end: 60.0, dt: 0.5 };
    let opts = SolverOptions::default();
    let sets: Vec<Vec<f64>> = (0..8).map(|k| vec![0.3 + 0.05 * k as f64, 0.01, 2.0]).collect();
    let par = sweep(&model, &info, &sets, &opts, grid, 4);
    for (set, r) in sets.iter().zip(par) {
        let one = simulate(&model, &info.with_params(set), &opts, grid, &mut []).unwrap();
        let r = r.unwrap();
        assert_eq!(r.values, one.values);
        // a lighter rotor stops sooner: t_stop = (J/c) ln(1 + c w0 / T_c)
        let exact = set[0] / set[1] * (1.0 + set[1] * 300.0 / set[2]).ln();
        assert!((r.events[0].t - exact).abs() < 1e-4 * exact);
    }
}

#[test]
fn the_homotopy_initialises_what_newton_alone_cannot() {
    use lsim_solve::init::{InitSettings, consistent_z};
    use lsim_solve::jac::JacStructure;
    // one iteration variable: 0 = atan(z - 3), from z = 40, with a Newton
    // budget too small for the damped iteration from so far
    let model = Hand {
        layout: Layout {
            n_x: 0,
            n_z: 1,
            n_p: 0,
            n_d: 0,
            n_u: 0,
            n_roots: 0,
            n_whens: 0,
            n_vars: 1,
            n_work: 1,
        },
        f: Box::new(|i, out| out[0] = (i.y[0] - 3.0).atan()),
        jvp: Box::new(|i, v, out| out[0] = v[0] / (1.0 + (i.y[0] - 3.0).powi(2))),
        roots: nothing(),
        vars: Box::new(|i, out| out[0] = i.y[0]),
        when: nothing_v(),
        y0: vec![40.0],
        d0: vec![],
    };
    let info = RunInfo::bare(1, 1, vec![]);
    let jac = JacStructure::new(None, 1);
    let mut y = vec![40.0];
    let s = InitSettings { rtol: 1e-8, atol: 1e-10, max_iterations: 4 };
    let out = consistent_z(&model, &info, &jac, 0.0, &mut y, &[], &[], &[], &s).unwrap();
    println!("{} ({out:?}), z = {}", out.describe(), y[0]);
    assert!(out.homotopy_steps > 0, "Newton alone should not have made it");
    assert!((y[0] - 3.0).abs() < 1e-8);
}

/// A `when time >= t_step` of a prepared model (the library's step) is a
/// time event the run loop reaches exactly: the step happens at t_step,
/// not a few ulps after it, and the output point there shows it.
#[test]
fn a_prepared_step_in_time_happens_exactly_at_its_time() {
    use lsim_ir::component::build::{connect, sub};
    use lsim_ir::expr::c;
    let top = lsim_ir::ComponentDef {
        name: "StepAndSpin".into(),
        components: vec![
            sub("step", "Signal.Step", &[("y0", c(0.0)), ("y1", c(1.0)), ("t_step", c(0.5))]),
            sub("drive", "Rotational.ConstantTorque", &[("tau", c(2.0))]),
            sub("rotor", "Rotational.Inertia", &[("J", c(1.0))]),
        ],
        connections: vec![connect("drive.flange", "rotor.a")],
        ..Default::default()
    };
    let built = common::build(&common::library(), &top, false);
    assert!(
        built.info.time_crossings.iter().any(Option::is_some),
        "the step's crossing is a time crossing"
    );
    for backend in backends() {
        let opts = SolverOptions { backend, ..Default::default() };
        let grid = OutputGrid { t0: 0.0, t_end: 1.0, dt: 0.125 };
        let run = simulate(&built.jit, &built.info, &opts, grid, &mut []).unwrap();
        let steps: Vec<f64> = run
            .events
            .iter()
            .filter(|e| matches!(e.kind, EventKind::When(_)))
            .map(|e| e.t)
            .collect();
        assert_eq!(steps, [0.5], "{backend:?}: {:?}", run.events);
        let y = run.channel("step.y").unwrap();
        let k = run.times.iter().position(|t| *t == 0.5).expect("0.5 on the grid");
        assert_eq!((y[k - 1], y[k]), (0.0, 1.0), "{backend:?}: the output at 0.5 is after it");
    }
}

/// The order contract between preparation, the code generator and the run
/// loop (DESIGN.md 5.8): the compiled `roots` evaluates
/// `PreparedModel::zero_crossings` in their order, and the run loop indexes
/// every per-crossing table by it (`RunInfo::time_crossings`, the modes'
/// and the `when` clauses' crossings). A model with time crossings at
/// distinct times between a state crossing and a mode: each compiled root
/// that `time_crossings` says crosses at `at` must change sign there, in
/// its direction. A code generator that reordered the roots would fail
/// this.
#[test]
fn compiled_roots_follow_the_zero_crossings_order() {
    use lsim_ir::component::build::{discrete, eq, param, state, sub, var};
    use lsim_ir::expr::{CmpOp, cmp, der, if_, name as n};
    use lsim_ir::{ComponentDef, Equation, EquationDecl, WhenAction};
    let when = |condition: Expr, var: &str, label: &str| EquationDecl {
        eq: Equation::When {
            condition,
            actions: vec![WhenAction::Assign { var: var.into(), value: Expr::Const(1.0) }],
        },
        label: Some(label.into()),
    };
    let clock = ComponentDef {
        name: "Test.Clock".into(),
        params: vec![
            param("rate", "1/s", 1.0, ""),
            param("t1", "s", 0.3, ""),
            param("t2", "s", 0.7, ""),
            param("t3", "s", 0.9, ""),
        ],
        vars: vec![
            state("x", "1", 0.0, ""),
            discrete("a", "1", 0.0, ""),
            discrete("b", "1", 0.0, ""),
            discrete("c", "1", 0.0, ""),
            var("y", "1", ""),
        ],
        equations: vec![
            eq(der("x"), n("rate"), "x rises"),
            when(cmp(CmpOp::Ge, Expr::Time, n("t1")), "a", "a at t1"),
            when(cmp(CmpOp::Ge, n("x"), Expr::Const(0.5)), "b", "b at x = 0.5"),
            eq(
                n("y"),
                if_(cmp(CmpOp::Gt, Expr::Time, n("t2")), Expr::Const(1.0), Expr::Const(0.0)),
                "y after t2",
            ),
            when(cmp(CmpOp::Lt, n("t3"), Expr::Time), "c", "c after t3"),
        ],
        ..Default::default()
    };
    let mut lib = common::library();
    lib.add(clock);
    let top = ComponentDef {
        name: "Test.Top".into(),
        components: vec![sub("k", "Test.Clock", &[])],
        ..Default::default()
    };
    let built = common::build(&lib, &top, false);
    let (m, info, jit) = (&built.prepared, &built.info, &built.jit);
    let n_roots = m.zero_crossings.len();
    assert_eq!(jit.layout().n_roots, n_roots, "one compiled root per zero crossing");
    assert_eq!(info.time_crossings.len(), n_roots);
    let l = *jit.layout();
    let (mut y0, mut d0) = (vec![0.0; l.n_y()], vec![0.0; l.n_d]);
    jit.start(&info.params, &mut y0, &mut d0);
    let mut work = vec![0.0; l.n_work];
    let mut vars = vec![0.0; l.n_vars];
    let mut out = vec![0.0; n_roots];
    let mut root = |t: f64, k: usize| {
        let inp = EvalInput { t, y: &y0, p: &info.params, d: &d0, u: &[] };
        jit.roots(&inp, &mut work, &mut out);
        out[k]
    };
    let inp = EvalInput { t: 0.0, y: &y0, p: &info.params, d: &d0, u: &[] };
    jit.vars(&inp, &mut vec![0.0; l.n_work], &mut vars);
    let env = lsim_ir::eval::SliceEnv { t: 0.0, vars: &vars, ders: &[], params: &info.params };
    let mut seen = vec![];
    for (k, tc) in info.time_crossings.iter().enumerate() {
        let Some(tc) = tc else {
            // the state crossing x - 0.5: -0.5 at the start, whatever the time
            assert_eq!(root(0.0, k), -0.5, "crossing {k}");
            assert_eq!(root(1.0, k), -0.5, "crossing {k}");
            continue;
        };
        let at = lsim_ir::eval::eval(&tc.at, &env);
        let (before, after) = (root(at - 1e-6, k), root(at + 1e-6, k));
        println!("crossing {k}: {} at {at}: {before:e} → {after:e}", m.zero_crossings[k].expr);
        assert!(root(at, k).abs() < 1e-12, "crossing {k} is not zero at its time {at}");
        if tc.rising {
            assert!(before < 0.0 && after > 0.0, "crossing {k} does not rise at {at}");
        } else {
            assert!(before > 0.0 && after < 0.0, "crossing {k} does not fall at {at}");
        }
        seen.push((at * 10.0).round() as i64);
    }
    seen.sort();
    seen.dedup();
    assert_eq!(seen, [3, 7, 9], "the three times");
    // the modes' and the when clauses' crossings are indices into the same
    for md in &info.modes {
        assert!(info.time_crossings[md.crossing].is_some(), "the mode of `time > t2`");
    }
}

/// A condition driven by a table of time while nothing the integrator
/// integrates moves: a target that rises from 0 to 1 between 10 and 11 s
/// and falls back by 12 s, `when target > 0.5` latching a flag, the only
/// state at rest (x' = 0). The integrator, seeing nothing move, takes steps
/// of many seconds and finds the condition false at both ends of the one
/// that spans 10–12 s: root finding sees no sign change and the event is
/// lost (the golden comparison's Battery Electric Car in winter stood 21 s
/// at a start: its motor's switch-on, driven by the driver's command from
/// the cycle's target, was stepped over). The table's breakpoints are stop
/// times (`RunInfo::time_tables`, found through the assignment that sets
/// the position it is read at, as the library's profiles set theirs): no
/// step spans one, and the flag latches at 10.5 s.
#[test]
fn a_condition_a_time_table_drives_is_not_stepped_over() {
    use lsim_ir::component::build::{discrete, eq, state, var};
    use lsim_ir::expr::{CmpOp, cmp, der, name as n};
    use lsim_ir::{ComponentDef, Equation, EquationDecl, WhenAction};
    let mut profile = lsim_ir::table::TableData::new_1d(
        vec![0.0, 10.0, 11.0, 12.0, 30.0],
        vec![0.0, 0.0, 1.0, 0.0, 0.0],
    );
    profile.axis_units[0] = "s".into();
    let cycle = ComponentDef {
        name: "Test.Cycle".into(),
        params: vec![lsim_lib::table::table_param("profile", "1", profile, "the target by time")],
        vars: vec![
            state("x", "1", 1.0, "at rest"),
            var("at", "s", "where it reads its profile"),
            var("target", "1", "the target"),
            discrete("seen", "1", 0.0, "1 once the target passed 0.5"),
        ],
        equations: vec![
            eq(der("x"), Expr::Const(0.0), "nothing moves"),
            eq(n("at"), Expr::Time, "it reads its profile at the time (as the library's do)"),
            eq(n("target"), lsim_ir::expr::table("profile", vec![n("at")]), "the target now"),
            EquationDecl {
                eq: Equation::When {
                    condition: cmp(CmpOp::Gt, n("target"), Expr::Const(0.5)),
                    actions: vec![WhenAction::Assign {
                        var: "seen".into(),
                        value: Expr::Const(1.0),
                    }],
                },
                label: Some("the target passes 0.5".into()),
            },
        ],
        ..Default::default()
    };
    let mut lib = common::library();
    lib.add(cycle);
    let top = ComponentDef {
        name: "Test.Top".into(),
        components: vec![lsim_ir::component::build::sub("k", "Test.Cycle", &[])],
        ..Default::default()
    };
    let built = common::build(&lib, &top, false);
    let info = &built.info;
    println!("time tables: {:?}", info.time_tables);
    assert_eq!(info.time_tables.len(), 1);
    assert_eq!(info.time_tables[0].at, [0.0, 10.0, 11.0, 12.0, 30.0]);
    assert_eq!(info.time_tables[0].c, 1.0);
    let seen = info.var_names.iter().position(|x| x == "k.seen").unwrap();
    let opts = SolverOptions::default();
    let grid = OutputGrid { t0: 0.0, t_end: 30.0, dt: 10.0 };
    let run = simulate(&built.jit, info, &opts, grid, &mut []).unwrap();
    let events: Vec<(String, f64)> = run.events.iter().map(|e| (e.label.clone(), e.t)).collect();
    println!(
        "{} steps; flag at the end {}; events {events:?}",
        run.stats.steps, run.values[seen][3]
    );
    assert_eq!(run.values[seen][3], 1.0, "the excursion was stepped over");
    assert!(events.iter().any(|(_, t)| (t - 10.5).abs() < 1e-6), "{events:?}");
    // the condition reads time alone (through the table): its sign change
    // is also found ahead and reached exactly, without the stops
    assert!(
        matches!(info.time_functions.as_slice(), [Some(lsim_solve::TimeFunction::Pure(_))]),
        "{:?}",
        info.time_functions
    );
    let mut searched = info.clone();
    searched.time_tables.clear();
    let run = simulate(&built.jit, &searched, &opts, grid, &mut []).unwrap();
    let at: Vec<f64> =
        run.events.iter().filter(|e| e.kind == EventKind::When(0)).map(|e| e.t).collect();
    println!("searched, without the stops: {} steps; at {at:?}", run.stats.steps);
    assert_eq!(run.values[seen][3], 1.0);
    assert!(at.len() == 1 && (at[0] - 10.5).abs() < 1e-14, "{at:?}");
    // without either the integrator steps over it
    let mut bare = searched;
    bare.time_functions.clear();
    let run = simulate(&built.jit, &bare, &opts, grid, &mut []).unwrap();
    println!(
        "without the stops or the search: {} steps; flag at the end {}",
        run.stats.steps, run.values[seen][3]
    );
    assert_eq!(run.values[seen][3], 0.0, "the failure this guards against");
}

/// The test cycle of [`a_condition_a_time_table_drives_is_not_stepped_over`]
/// (a flag latched when its profile, read at the time, passes 0.5), built.
fn cycle_model() -> common::Built {
    use lsim_ir::component::build::{discrete, eq, state, var};
    use lsim_ir::expr::{CmpOp, cmp, der, name as n};
    use lsim_ir::{ComponentDef, Equation, EquationDecl, WhenAction};
    let mut profile = lsim_ir::table::TableData::new_1d(
        vec![0.0, 10.0, 11.0, 12.0, 30.0],
        vec![0.0, 0.0, 1.0, 0.0, 0.0],
    );
    profile.axis_units[0] = "s".into();
    let cycle = ComponentDef {
        name: "Test.Cycle".into(),
        params: vec![lsim_lib::table::table_param("profile", "1", profile, "the target by time")],
        vars: vec![
            state("x", "1", 1.0, "at rest"),
            var("at", "s", "where it reads its profile"),
            var("target", "1", "the target"),
            discrete("seen", "1", 0.0, "1 once the target passed 0.5"),
        ],
        equations: vec![
            eq(der("x"), Expr::Const(0.0), "nothing moves"),
            eq(n("at"), Expr::Time, "it reads its profile at the time (as the library's do)"),
            eq(n("target"), lsim_ir::expr::table("profile", vec![n("at")]), "the target now"),
            EquationDecl {
                eq: Equation::When {
                    condition: cmp(CmpOp::Gt, n("target"), Expr::Const(0.5)),
                    actions: vec![WhenAction::Assign {
                        var: "seen".into(),
                        value: Expr::Const(1.0),
                    }],
                },
                label: Some("the target passes 0.5".into()),
            },
        ],
        ..Default::default()
    };
    let mut lib = common::library();
    lib.add(cycle);
    let top = ComponentDef {
        name: "Test.Top".into(),
        components: vec![lsim_ir::component::build::sub("k", "Test.Cycle", &[])],
        ..Default::default()
    };
    common::build(&lib, &top, false)
}

/// A compiled model given other table data after preparation
/// (`JitModel::with_tables`: the cycle's pulse moved from 10–12 s to
/// 20–22 s, the run information still the prepared one's): the run takes
/// the tables' breakpoints from the model (`ModelFunctions::table_axes`),
/// so its stops and its search ahead follow the new data, and the flag
/// latches at 20.5 s. A model that does not give its breakpoints is run on
/// the prepared ones: its stops at 10–12 s and its search, over pieces the
/// new data do not have, step over the moved pulse; the run warns of it.
#[test]
fn a_model_given_other_tables_is_run_on_their_breakpoints() {
    let built = cycle_model();
    let info = &built.info;
    let mut moved = lsim_ir::table::TableData::new_1d(
        vec![0.0, 20.0, 21.0, 22.0, 30.0],
        vec![0.0, 0.0, 1.0, 0.0, 0.0],
    );
    moved.axis_units[0] = "s".into();
    let jit = built.jit.with_tables(&[moved]).expect("takes the new data");
    assert_eq!(
        ModelFunctions::table_axes(&jit, 0),
        Some([vec![0.0, 20.0, 21.0, 22.0, 30.0], vec![]])
    );
    let seen = info.var_names.iter().position(|x| x == "k.seen").unwrap();
    let opts = SolverOptions::default();
    let grid = OutputGrid { t0: 0.0, t_end: 30.0, dt: 10.0 };
    let run = simulate(&jit, info, &opts, grid, &mut []).unwrap();
    let at: Vec<f64> =
        run.events.iter().filter(|e| e.kind == EventKind::When(0)).map(|e| e.t).collect();
    println!("{} steps; at {at:?}", run.stats.steps);
    assert_eq!(run.values[seen][3], 1.0, "the moved pulse was stepped over");
    assert!(at.len() == 1 && (at[0] - 20.5).abs() < 1e-12, "{at:?}");
    // the same model hiding its breakpoints: the prepared ones mislead
    struct Hidden<'a>(&'a lsim_codegen::JitModel);
    impl ModelFunctions for Hidden<'_> {
        fn layout(&self) -> &Layout {
            self.0.layout()
        }
        fn residual(&self, inp: &EvalInput<'_>, w: &mut [f64], out: &mut [f64]) {
            self.0.residual(inp, w, out)
        }
        fn jvp(&self, inp: &EvalInput<'_>, v: &[f64], w: &mut [f64], out: &mut [f64]) {
            self.0.jvp(inp, v, w, out)
        }
        fn roots(&self, inp: &EvalInput<'_>, w: &mut [f64], out: &mut [f64]) {
            self.0.roots(inp, w, out)
        }
        fn vars(&self, inp: &EvalInput<'_>, w: &mut [f64], out: &mut [f64]) {
            self.0.vars(inp, w, out)
        }
        fn when(&self, inp: &EvalInput<'_>, f: &[f64], w: &mut [f64], d: &mut [f64]) {
            self.0.when(inp, f, w, d)
        }
        fn start(&self, p: &[f64], y0: &mut [f64], d0: &mut [f64]) {
            self.0.start(p, y0, d0)
        }
        fn jacobian_dense(&self, inp: &EvalInput<'_>, w: &mut [f64], out: &mut [f64]) {
            self.0.jacobian_dense(inp, w, out)
        }
        fn sparsity(&self) -> Option<&lsim_ir::runtime::SparsityPattern> {
            ModelFunctions::sparsity(self.0)
        }
        fn jacobian_sparse(&self, inp: &EvalInput<'_>, w: &mut [f64], v: &mut [f64]) {
            ModelFunctions::jacobian_sparse(self.0, inp, w, v)
        }
        fn modes(&self, inp: &EvalInput<'_>, w: &mut [f64], d: &mut [f64]) {
            self.0.modes(inp, w, d)
        }
        fn init(&self) -> Option<&dyn lsim_ir::runtime::InitFunctions> {
            self.0.init()
        }
        fn table_guard_list(&self) -> &[lsim_ir::runtime::TableGuard] {
            self.0.table_guard_list()
        }
        fn table_guards(&self, inp: &EvalInput<'_>, w: &mut [f64], out: &mut [f64]) {
            self.0.table_guards(inp, w, out)
        }
        fn eval_table(&self, k: u32, args: [f64; 2]) -> Option<(f64, [f64; 2])> {
            self.0.eval_table(k, args)
        }
    }
    let run = simulate(&Hidden(&jit), info, &opts, grid, &mut []).unwrap();
    println!("breakpoints hidden: {} steps; flag {}", run.stats.steps, run.values[seen][3]);
    assert_eq!(run.values[seen][3], 0.0, "the failure this guards against");
    // ... and says so, once for the table
    let said: Vec<&String> =
        run.report.warnings.iter().filter(|w| w.contains("ModelFunctions::table_axes")).collect();
    println!("{said:?}");
    assert_eq!(said.len(), 1, "{:?}", run.report.warnings);
    // the model that gives them warns of nothing
    let run = simulate(&jit, info, &opts, grid, &mut []).unwrap();
    assert!(run.report.warnings.is_empty(), "{:?}", run.report.warnings);
}

/// A condition on a table read at a position that moves with time but not
/// affinely: `profile(10 + 8 sin(ω time)) > 0.5`, the profile up and down
/// between its breakpoints. Its sign changes are searched ahead through
/// the table's monotone cubic pieces (the compiled interpolant's), and the
/// time it held agrees with the crossings bisected here on the compiled
/// table itself.
#[test]
fn a_condition_on_a_table_of_a_function_of_time_is_found_exactly() {
    use lsim_ir::ComponentDef;
    use lsim_ir::component::build::{eq, param, state, var};
    use lsim_ir::expr::{Builtin, CmpOp, call, cmp, der, if_, name as n};
    let w = 2.0 * std::f64::consts::PI / 10.0;
    let mut profile = lsim_ir::table::TableData::new_1d(
        vec![0.0, 3.0, 6.0, 9.0, 12.0, 15.0, 20.0],
        vec![0.0, 1.0, 0.2, 0.9, 0.1, 1.0, 0.0],
    );
    profile.axis_units[0] = "1".into();
    let wobble = ComponentDef {
        name: "Test.Wobble".into(),
        params: vec![
            param("w", "rad/s", w, "the sine's angular frequency"),
            lsim_lib::table::table_param("profile", "1", profile, "a level by position"),
        ],
        vars: vec![
            var("at", "1", "where it reads its profile"),
            var("level", "1", "the level there"),
            state("x", "s", 0.0, "how long the level was above one half"),
        ],
        equations: vec![
            eq(
                n("at"),
                Expr::Const(10.0)
                    + Expr::Const(8.0) * call(Builtin::Sin, vec![n("w") * Expr::Time]),
                "it sweeps its profile",
            ),
            eq(n("level"), lsim_ir::expr::table("profile", vec![n("at")]), "the level"),
            eq(
                der("x"),
                if_(
                    cmp(CmpOp::Gt, n("level"), Expr::Const(0.5)),
                    Expr::Const(1.0),
                    Expr::Const(0.0),
                ),
                "on while the level is above one half",
            ),
        ],
        ..Default::default()
    };
    let mut lib = common::library();
    lib.add(wobble);
    let top = ComponentDef {
        name: "Test.Top".into(),
        components: vec![lsim_ir::component::build::sub("k", "Test.Wobble", &[])],
        ..Default::default()
    };
    let built = common::build(&lib, &top, false);
    let info = &built.info;
    assert!(
        info.time_functions.iter().any(|f| matches!(f, Some(lsim_solve::TimeFunction::Pure(_)))),
        "{:?}",
        info.time_functions
    );
    let t_end = 40.0;
    let run = simulate(
        &built.jit,
        info,
        &SolverOptions::default(),
        OutputGrid { t0: 0.0, t_end, dt: 10.0 },
        &mut [],
    )
    .unwrap();
    let x = *run.channel("k.x").unwrap().last().unwrap();
    // the crossings, bisected on the compiled table
    let table = built.jit.table(0);
    let h = |t: f64| table.eval([10.0 + 8.0 * (w * t).sin(), 0.0]).0 - 0.5;
    let (mut held, mut since, mut count, n) = (0.0, None, 0, 4_000_000);
    if h(0.0) > 0.0 {
        since = Some(0.0);
    }
    for k in 0..n {
        let (a, b) = (t_end * k as f64 / n as f64, t_end * (k + 1) as f64 / n as f64);
        if (h(a) > 0.0) != (h(b) > 0.0) {
            count += 1;
            let (mut lo, mut hi) = (a, b);
            while hi - lo > 1e-15 * hi {
                let m = 0.5 * (lo + hi);
                if (h(m) > 0.0) == (h(a) > 0.0) {
                    lo = m;
                } else {
                    hi = m;
                }
            }
            match since.take() {
                None => since = Some(hi),
                Some(t0) => held += hi - t0,
            }
        }
    }
    if let Some(t0) = since {
        held += t_end - t0;
    }
    let modes = run.events.iter().filter(|e| matches!(e.kind, EventKind::Mode(_))).count();
    println!(
        "held {x:.12} s (exact {held:.12}), {count} crossings, {modes} mode events, {} steps, {:?}",
        run.stats.steps, run.report.warnings
    );
    assert!(count > 8, "the profile is crossed often: {count}");
    assert!((x - held).abs() < 1e-9 * held, "{x} against {held}");
    assert!(run.report.warnings.is_empty());
}

/// Conditions on explicit functions of time in a prepared model, found by
/// preparation through the assignments: `sin(ω time) > 0.95` reads time
/// alone (its sign changes are searched ahead and reached exactly);
/// `sin(ω time) > x` reads a state too (the run loop stops at the sine's
/// extrema, and root finding locates the crossings). Both run with the
/// default options, nothing integrated moving fast, and agree with the
/// exact answers: the time each held.
#[test]
fn conditions_on_functions_of_time_are_found_in_a_prepared_model() {
    use lsim_ir::ComponentDef;
    use lsim_ir::component::build::{eq, param, state, var};
    use lsim_ir::expr::{Builtin, CmpOp, call, cmp, der, if_, name as n};
    let period = 10.0;
    let w = 2.0 * std::f64::consts::PI / period;
    let pulse = ComponentDef {
        name: "Test.Pulses".into(),
        params: vec![param("w", "rad/s", w, "the sine's angular frequency")],
        vars: vec![
            var("phase", "rad", "the sine's phase"),
            var("wave", "1", "the sine"),
            state("on", "s", 0.0, "how long the wave was above 0.95"),
            state("x", "1", 1.0, "a level falling slowly"),
            state("above", "s", 0.0, "how long the wave was above the level"),
        ],
        equations: vec![
            eq(n("phase"), n("w") * Expr::Time, "the phase"),
            eq(n("wave"), call(Builtin::Sin, vec![n("phase")]), "the sine"),
            eq(
                der("on"),
                if_(
                    cmp(CmpOp::Gt, n("wave"), Expr::Const(0.95)),
                    Expr::Const(1.0),
                    Expr::Const(0.0),
                ),
                "on while the wave is above 0.95",
            ),
            eq(der("x"), Expr::Const(-0.01), "the level falls"),
            eq(
                der("above"),
                if_(cmp(CmpOp::Gt, n("wave"), n("x")), Expr::Const(1.0), Expr::Const(0.0)),
                "on while the wave is above the level",
            ),
        ],
        ..Default::default()
    };
    let mut lib = common::library();
    lib.add(pulse);
    let top = ComponentDef {
        name: "Test.Top".into(),
        components: vec![lsim_ir::component::build::sub("k", "Test.Pulses", &[])],
        ..Default::default()
    };
    let built = common::build(&lib, &top, false);
    let info = &built.info;
    println!("time functions: {:?}", info.time_functions);
    let pure = info
        .time_functions
        .iter()
        .filter(|f| matches!(f, Some(lsim_solve::TimeFunction::Pure(_))))
        .count();
    let mixed = info
        .time_functions
        .iter()
        .filter(|f| matches!(f, Some(lsim_solve::TimeFunction::Mixed { .. })))
        .count();
    assert!(pure >= 1 && mixed >= 1, "{:?}", info.time_functions);
    let t_end = 100.0;
    let opts = SolverOptions::default();
    let run = simulate(&built.jit, info, &opts, OutputGrid { t0: 0.0, t_end, dt: 10.0 }, &mut [])
        .unwrap();
    let ch = |name: &str| *run.channel(name).unwrap().last().unwrap();
    let exact_on = 10.0 * (std::f64::consts::PI - 2.0 * 0.95f64.asin()) / w;
    // the time sin(w t) > 1 - 0.01 t held: its crossings, bisected
    let h = |t: f64| (w * t).sin() - (1.0 - 0.01 * t);
    let (mut held, mut since, n) = (0.0, None, 1_000_000);
    for k in 0..n {
        let (a, b) = (t_end * k as f64 / n as f64, t_end * (k + 1) as f64 / n as f64);
        if (h(a) > 0.0) != (h(b) > 0.0) {
            let (mut lo, mut hi) = (a, b);
            while hi - lo > 1e-15 * hi {
                let m = 0.5 * (lo + hi);
                if (h(m) > 0.0) == (h(a) > 0.0) {
                    lo = m;
                } else {
                    hi = m;
                }
            }
            match since.take() {
                None => since = Some(hi),
                Some(t0) => held += hi - t0,
            }
        }
    }
    println!(
        "on {} s (exact {exact_on}), above {} s (exact {held}), {} steps, warnings {:?}",
        ch("k.on"),
        ch("k.above"),
        run.stats.steps,
        run.report.warnings
    );
    // (the default tolerances: the states integrate constants exactly)
    assert!((ch("k.on") - exact_on).abs() < 1e-9 * exact_on);
    assert!((ch("k.above") - held).abs() < 1e-6 * held);
    assert!(run.report.warnings.is_empty(), "{:?}", run.report.warnings);
}
