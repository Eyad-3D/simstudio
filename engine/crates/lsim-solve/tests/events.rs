//! The run loop on hand-written models with exact answers: modes (a rotor
//! that sticks when Coulomb friction stops it: `mech_inertia_coastdown`
//! from the reference suite), time events, event iteration across chained
//! `when` clauses, event storms, sampled blocks (one that changes nothing,
//! one that changes its output at every tick), the 10× tighter check,
//! parallel sweeps and the initialisation's homotopy.

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
                assert!(t >= 1.0 && t < 1.01, "{t}");
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
