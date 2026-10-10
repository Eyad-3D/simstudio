//! The run loop's event handling on hand-written models with exact
//! answers: `when` conditions made true by a sample tick, by the start, by
//! another `when` through an iteration variable; mode changes a clock
//! schedules; a tick just before the end; the cost of a restart; time
//! events located exactly, re-armed or set to now at their own instant, or
//! coinciding with a root; the momentum kept at a change of a rigid
//! coupling.

use lsim_ir::expr::Expr;
use lsim_ir::prepared::Direction;
use lsim_ir::runtime::{DiscreteBlock, EvalInput, Layout, ModelFunctions};
use lsim_ir::{ParamId, VarId};
use lsim_solve::{
    Backend, BlockInfo, EnergyInfo, EnergyPart, EngagementInfo, EventKind, ImpulseInfo,
    ImpulseLink, Integrator, ModeInfo, OutputGrid, RunInfo, SimResult, SolveError, SolverOptions,
    SolverStats, Step, TimeCrossing, VarSource, run_loop, simulate,
};
use std::sync::Arc;

type F = Box<dyn Fn(&EvalInput<'_>, &mut [f64]) + Send + Sync>;
type FV = Box<dyn Fn(&EvalInput<'_>, &[f64], &mut [f64]) + Send + Sync>;

/// A model written by hand: `y = [x; z]`, residual `[x'; g]`.
struct Hand {
    layout: Layout,
    f: F,
    jvp: FV,
    roots: F,
    vars: F,
    when: FV,
    modes: Option<FV>,
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
    fn modes(&self, inp: &EvalInput<'_>, _w: &mut [f64], d_out: &mut [f64]) {
        if let Some(m) = &self.modes {
            m(inp, &[], d_out)
        }
    }
    fn start(&self, _p: &[f64], y0: &mut [f64], d0: &mut [f64]) {
        y0.copy_from_slice(&self.y0);
        d0.copy_from_slice(&self.d0);
    }
}

#[allow(clippy::too_many_arguments)]
fn layout(
    n_x: usize,
    n_z: usize,
    n_p: usize,
    n_d: usize,
    n_roots: usize,
    n_whens: usize,
    n_vars: usize,
) -> Layout {
    Layout { n_x, n_z, n_p, n_d, n_u: 0, n_roots, n_whens, n_vars, n_work: 1 }
}

fn nothing_v() -> FV {
    Box::new(|_, _, _| {})
}

fn backends() -> Vec<Backend> {
    let mut b = vec![Backend::Sundials];
    if cfg!(feature = "diffsol") {
        b.push(Backend::Diffsol);
    }
    b
}

/// A sampled block: applies `law` to its inputs at each tick.
struct Sampled {
    period: f64,
    offset: f64,
    law: fn(f64, &[f64], &mut [f64]),
}

impl DiscreteBlock for Sampled {
    fn name(&self) -> &str {
        "'Controller'"
    }
    fn period(&self) -> f64 {
        self.period
    }
    fn offset(&self) -> f64 {
        self.offset
    }
    fn init(&mut self, _t: f64, _i: &[f64], _o: &mut [f64]) -> Result<(), String> {
        Ok(())
    }
    fn tick(&mut self, t: f64, i: &[f64], o: &mut [f64]) -> Result<(), String> {
        (self.law)(t, i, o);
        Ok(())
    }
}

fn block(inputs: Vec<usize>, outputs: Vec<usize>, period: f64) -> BlockInfo {
    BlockInfo {
        name: "'Controller'".into(),
        chains: vec![None; inputs.len()],
        inputs,
        outputs,
        period,
    }
}

fn whens(info: &mut RunInfo, w: &[(usize, Direction, &str)]) {
    info.whens = w.iter().map(|(c, d, _)| (*c, *d)).collect();
    info.when_labels = w.iter().map(|(_, _, l)| l.to_string()).collect();
}

fn at(run: &SimResult, t: f64) -> usize {
    run.times.iter().position(|x| (x - t).abs() < 1e-12).expect("a grid time")
}

/// A sample tick sets an output that makes a `when` condition true: the
/// `when` fires at that tick (its condition's value before the event is
/// the one before the tick).
#[test]
fn a_when_made_true_by_a_ticks_output_fires_at_the_tick() {
    // y' = 0; d = [u (the block's output), latched];
    // when u > 0.5: latched := 1; the block sets u = 1 from t = 0.3 on
    let model = Hand {
        layout: layout(1, 0, 0, 2, 1, 1, 3),
        f: Box::new(|_, out| out[0] = 0.0),
        jvp: Box::new(|_, _, out| out[0] = 0.0),
        roots: Box::new(|i, out| out[0] = i.d[0] - 0.5),
        vars: Box::new(|i, out| {
            out[0] = i.y[0];
            out[1] = i.d[0];
            out[2] = i.d[1];
        }),
        when: Box::new(|_, fired, d| {
            if fired[0] != 0.0 {
                d[1] = 1.0;
            }
        }),
        modes: None,
        y0: vec![0.0],
        d0: vec![0.0, 0.0],
    };
    let mut info = RunInfo::bare(1, 3, vec![]);
    info.root_dirs = vec![1];
    whens(&mut info, &[(0, Direction::Rising, "'Latch': set")]);
    info.var_sources = vec![VarSource::Y(0), VarSource::D(0), VarSource::D(1)];
    info.blocks = vec![block(vec![0], vec![0], 0.1)];
    for backend in backends() {
        let opts = SolverOptions { backend, ..Default::default() };
        let mut blocks: Vec<Box<dyn DiscreteBlock>> = vec![Box::new(Sampled {
            period: 0.1,
            offset: 0.0,
            law: |t, _, o| o[0] = if t > 0.25 { 1.0 } else { 0.0 },
        })];
        let grid = OutputGrid { t0: 0.0, t_end: 1.0, dt: 0.1 };
        let run = simulate(&model, &info, &opts, grid, &mut blocks).unwrap();
        let fired: Vec<f64> =
            run.events.iter().filter(|e| e.kind == EventKind::When(0)).map(|e| e.t).collect();
        assert_eq!(fired.len(), 1, "{backend:?}: {:?}", run.events);
        assert!((fired[0] - 0.3).abs() < 1e-12, "{backend:?}: at the tick, not {}", fired[0]);
        assert_eq!(run.values[2][at(&run, 0.2)], 0.0, "{backend:?}");
        assert_eq!(run.values[2][at(&run, 0.3)], 1.0, "{backend:?}: latched at the tick");
    }
}

/// Mode changes a clock tick makes are scheduled by the clock: a block
/// that switches a mode at each of its ticks is not an event storm.
#[test]
fn mode_changes_a_tick_makes_are_no_event_storm() {
    // y' = m with the mode m: u > 0.5; the block toggles u at every tick
    // (2000 ticks a second: 200 within the storm window of 0.1 s)
    let model = Hand {
        layout: layout(1, 0, 0, 2, 2, 2, 3),
        f: Box::new(|i, out| out[0] = i.d[1]),
        jvp: Box::new(|_, _, out| out[0] = 0.0),
        roots: Box::new(|i, out| {
            out[0] = i.d[0] - 0.5;
            out[1] = i.d[0] - 0.5;
        }),
        vars: Box::new(|i, out| {
            out[0] = i.y[0];
            out[1] = i.d[0];
            out[2] = i.d[1];
        }),
        when: Box::new(|_, fired, d| {
            if fired[0] != 0.0 {
                d[1] = 1.0;
            }
            if fired[1] != 0.0 {
                d[1] = 0.0;
            }
        }),
        modes: Some(Box::new(|i, _, d| d[1] = if i.d[0] > 0.5 { 1.0 } else { 0.0 })),
        y0: vec![0.0],
        d0: vec![0.0, 0.0],
    };
    let mut info = RunInfo::bare(1, 3, vec![]);
    info.root_dirs = vec![1, -1];
    whens(
        &mut info,
        &[(0, Direction::Rising, "'Switch': on"), (1, Direction::Falling, "'Switch': off")],
    );
    info.modes = vec![ModeInfo { crossing: 0, discrete: 1, label: "'Switch': on".into() }];
    info.var_sources = vec![VarSource::Y(0), VarSource::D(0), VarSource::D(1)];
    info.blocks = vec![block(vec![0], vec![0], 5e-4)];
    for backend in backends() {
        let opts = SolverOptions { backend, storm_window: 0.1, ..Default::default() };
        let mut blocks: Vec<Box<dyn DiscreteBlock>> = vec![Box::new(Sampled {
            period: 5e-4,
            offset: 0.0,
            law: |t, _, o| o[0] = if (t / 5e-4).round() as i64 % 2 == 0 { 1.0 } else { 0.0 },
        })];
        let grid = OutputGrid { t0: 0.0, t_end: 1.0, dt: 0.1 };
        let run = simulate(&model, &info, &opts, grid, &mut blocks)
            .unwrap_or_else(|e| panic!("{backend:?}: {e}"));
        // on half of the time: y(1) = 0.5
        assert!((run.values[0].last().unwrap() - 0.5).abs() < 1e-9, "{backend:?}");
        let flips = run.events.iter().filter(|e| matches!(e.kind, EventKind::Mode(_))).count();
        assert!(flips >= 1999, "{backend:?}: {flips} mode changes");
    }
}

/// Event iteration re-checks the conditions with the iteration variables
/// solved again for the new discrete values: a `when` whose condition
/// reads an iteration variable that another `when` just moved fires at
/// the same instant.
#[test]
fn a_condition_on_an_iteration_variable_is_rechecked_with_its_new_value() {
    // x' = 1; 0 = z - (x + 10 d0);
    // when x >= 1: d0 := 1  (z jumps from 1 to 11)
    // when z >= 5: d1 := 1  (true from that instant)
    let model = Hand {
        layout: layout(1, 1, 0, 2, 2, 2, 4),
        f: Box::new(|i, out| {
            out[0] = 1.0;
            out[1] = i.y[1] - (i.y[0] + 10.0 * i.d[0]);
        }),
        jvp: Box::new(|_, v, out| {
            out[0] = 0.0;
            out[1] = v[1] - v[0];
        }),
        roots: Box::new(|i, out| {
            out[0] = i.y[0] - 1.0;
            out[1] = i.y[1] - 5.0;
        }),
        vars: Box::new(|i, out| {
            out[0] = i.y[0];
            out[1] = i.y[1];
            out[2] = i.d[0];
            out[3] = i.d[1];
        }),
        when: Box::new(|_, fired, d| {
            if fired[0] != 0.0 {
                d[0] = 1.0;
            }
            if fired[1] != 0.0 {
                d[1] = 1.0;
            }
        }),
        modes: None,
        y0: vec![0.0, 0.0],
        d0: vec![0.0, 0.0],
    };
    let mut info = RunInfo::bare(2, 4, vec![]);
    info.root_dirs = vec![1, 1];
    whens(&mut info, &[(0, Direction::Rising, "first"), (1, Direction::Rising, "second")]);
    for backend in backends() {
        let opts = SolverOptions { backend, rtol: 1e-9, atol: 1e-12, ..Default::default() };
        let grid = OutputGrid { t0: 0.0, t_end: 2.0, dt: 0.5 };
        let run = simulate(&model, &info, &opts, grid, &mut []).unwrap();
        let labels: Vec<_> = run.events.iter().map(|e| e.label.as_str()).collect();
        assert_eq!(labels, ["first", "second"], "{backend:?}: {:?}", run.events);
        assert_eq!(run.events[0].t, run.events[1].t, "{backend:?}: one instant");
        assert!((run.events[0].t - 1.0).abs() < 1e-8, "{backend:?}");
        assert_eq!(*run.values[3].last().unwrap(), 1.0, "{backend:?}");
    }
}

/// The DAE of the restart tests: y' = u - y, 0 = z - 2 y, the block's
/// output u a discrete variable it sets from y.
fn held_dae() -> (Hand, RunInfo) {
    let model = Hand {
        layout: layout(1, 1, 0, 1, 0, 0, 3),
        f: Box::new(|i, out| {
            out[0] = i.d[0] - i.y[0];
            out[1] = i.y[1] - 2.0 * i.y[0];
        }),
        jvp: Box::new(|_, v, out| {
            out[0] = -v[0];
            out[1] = v[1] - 2.0 * v[0];
        }),
        roots: Box::new(|_, _| {}),
        vars: Box::new(|i, out| {
            out[0] = i.y[0];
            out[1] = i.y[1];
            out[2] = i.d[0];
        }),
        when: nothing_v(),
        modes: None,
        y0: vec![1.0, 2.0],
        d0: vec![0.0],
    };
    let mut info = RunInfo::bare(2, 3, vec![]);
    info.var_sources = vec![VarSource::Y(0), VarSource::Y(1), VarSource::D(0)];
    (model, info)
}

/// A tick that changes its output a few ulps before the end time leaves
/// an interval the integrators cannot step: the run ends there.
#[test]
fn a_tick_just_before_the_end_ends_the_run() {
    // ticks every 0.03 s: the eleventh is at 0.32999999999999996, the end
    // at 0.33
    assert!(11.0 * std::hint::black_box(0.03) < 0.33);
    let (model, mut info) = held_dae();
    info.blocks = vec![block(vec![0], vec![0], 0.03)];
    for backend in backends() {
        let opts = SolverOptions { backend, rtol: 1e-9, atol: 1e-12, ..Default::default() };
        let mut blocks: Vec<Box<dyn DiscreteBlock>> =
            vec![Box::new(Sampled { period: 0.03, offset: 0.0, law: |t, _, o| o[0] = t })];
        let grid = OutputGrid { t0: 0.0, t_end: 0.33, dt: 0.03 };
        let run = simulate(&model, &info, &opts, grid, &mut blocks)
            .unwrap_or_else(|e| panic!("{backend:?}: {e}"));
        assert_eq!(*run.times.last().unwrap(), 0.33);
        assert_eq!(run.report.block_changes, 11, "{backend:?}");
        // the last output point carries the last tick's output
        assert_eq!(*run.values[2].last().unwrap(), 11.0 * 0.03, "{backend:?}");
    }
}

/// Modelica's `when`: it fires when its condition changes from false to
/// true. A condition already true at the start does not fire there (the
/// start takes it as it is, as `pre(c) = c` after Modelica's
/// initialisation), and fires as soon as it becomes true again after
/// having been false.
#[test]
fn a_when_true_at_the_start_fires_only_when_it_becomes_true_again() {
    // p'' = -p, p(0) = 1.5: p = 1.5 cos t, positive at the start, false
    // from pi/2, true again at 3 pi/2; when p > 0: n := n + 1
    let model = Hand {
        layout: layout(2, 0, 0, 1, 1, 1, 3),
        f: Box::new(|i, out| {
            out[0] = i.y[1];
            out[1] = -i.y[0];
        }),
        jvp: Box::new(|_, v, out| {
            out[0] = v[1];
            out[1] = -v[0];
        }),
        roots: Box::new(|i, out| out[0] = i.y[0]),
        vars: Box::new(|i, out| {
            out[0] = i.y[0];
            out[1] = i.y[1];
            out[2] = i.d[0];
        }),
        when: Box::new(|i, fired, d| {
            if fired[0] != 0.0 {
                d[0] = i.d[0] + 1.0;
            }
        }),
        modes: None,
        y0: vec![1.5, 0.0],
        d0: vec![0.0],
    };
    let mut info = RunInfo::bare(2, 3, vec![]);
    info.root_dirs = vec![1];
    whens(&mut info, &[(0, Direction::Rising, "'Counter': up")]);
    for backend in backends() {
        let opts = SolverOptions { backend, rtol: 1e-9, atol: 1e-12, ..Default::default() };
        let grid = OutputGrid { t0: 0.0, t_end: 6.0, dt: 0.5 };
        let run = simulate(&model, &info, &opts, grid, &mut []).unwrap();
        let fired: Vec<f64> =
            run.events.iter().filter(|e| e.kind == EventKind::When(0)).map(|e| e.t).collect();
        assert_eq!(fired.len(), 1, "{backend:?}: {:?}", run.events);
        let t_up = 1.5 * std::f64::consts::PI;
        assert!((fired[0] - t_up).abs() < 1e-7, "{backend:?}: {}", fired[0]);
        assert_eq!(run.values[2][at(&run, 4.5)], 0.0, "{backend:?}");
        assert_eq!(*run.values[2].last().unwrap(), 1.0, "{backend:?}");
    }
}

/// A condition exactly at its threshold at the start counts as true there
/// (as `p >= 0` is): it fires once it has been false and becomes true
/// again, not as it leaves the threshold upwards at the start.
#[test]
fn a_when_at_its_threshold_at_the_start_counts_as_true() {
    // p = sin t: p'' = -p, p(0) = 0, p'(0) = 1; when p >= 0: n := n + 1
    let model = Hand {
        layout: layout(2, 0, 0, 1, 1, 1, 3),
        f: Box::new(|i, out| {
            out[0] = i.y[1];
            out[1] = -i.y[0];
        }),
        jvp: Box::new(|_, v, out| {
            out[0] = v[1];
            out[1] = -v[0];
        }),
        roots: Box::new(|i, out| out[0] = i.y[0]),
        vars: Box::new(|i, out| {
            out[0] = i.y[0];
            out[1] = i.y[1];
            out[2] = i.d[0];
        }),
        when: Box::new(|i, fired, d| {
            if fired[0] != 0.0 {
                d[0] = i.d[0] + 1.0;
            }
        }),
        modes: None,
        y0: vec![0.0, 1.0],
        d0: vec![0.0],
    };
    let mut info = RunInfo::bare(2, 3, vec![]);
    info.root_dirs = vec![1];
    whens(&mut info, &[(0, Direction::Rising, "'Counter': up")]);
    for backend in backends() {
        let opts = SolverOptions { backend, rtol: 1e-9, atol: 1e-12, ..Default::default() };
        let grid = OutputGrid { t0: 0.0, t_end: 7.0, dt: 0.5 };
        let run = simulate(&model, &info, &opts, grid, &mut []).unwrap();
        let fired: Vec<f64> =
            run.events.iter().filter(|e| e.kind == EventKind::When(0)).map(|e| e.t).collect();
        assert_eq!(fired.len(), 1, "{backend:?}: {:?}", run.events);
        let t_up = 2.0 * std::f64::consts::PI;
        assert!((fired[0] - t_up).abs() < 1e-7, "{backend:?}: {}", fired[0]);
    }
}

/// A restart after a tick that changed an output costs a few steps, not
/// the ramp-up from a tiny first step at first order.
#[test]
fn a_restart_after_a_tick_costs_a_few_steps() {
    // a sample-and-hold controller u_k = 1 - y(t_k) every 10 ms over 10 s
    let (model, mut info) = held_dae();
    info.blocks = vec![block(vec![0], vec![0], 0.01)];
    for (backend, n_z) in [(Backend::Sundials, 1), (Backend::Sundials, 0)] {
        let mut model_x = Hand { ..held_dae().0 };
        let mut info_x = info.clone();
        if n_z == 0 {
            // the same as an ODE (CVODE): y' = u - y
            model_x.layout.n_z = 0;
            model_x.f = Box::new(|i, out| out[0] = i.d[0] - i.y[0]);
            model_x.jvp = Box::new(|_, v, out| out[0] = -v[0]);
            model_x.vars = Box::new(|i, out| {
                out[0] = i.y[0];
                out[1] = 2.0 * i.y[0];
                out[2] = i.d[0];
            });
            model_x.y0 = vec![1.0];
            info_x = RunInfo { y_nominal: vec![1.0], y_names: vec!["y0".into()], ..info_x };
        }
        let (m, inf) = if n_z == 1 { (&model, &info) } else { (&model_x, &info_x) };
        let opts = SolverOptions { backend, rtol: 1e-6, atol: 1e-8, ..Default::default() };
        let mut blocks: Vec<Box<dyn DiscreteBlock>> =
            vec![Box::new(Sampled { period: 0.01, offset: 0.0, law: |_, i, o| o[0] = 1.0 - i[0] })];
        let grid = OutputGrid { t0: 0.0, t_end: 10.0, dt: 0.1 };
        let run = simulate(m, inf, &opts, grid, &mut blocks).unwrap();
        let per_tick = run.stats.steps as f64 / run.report.block_changes as f64;
        println!(
            "{} (n_z = {n_z}): {} steps, {} changing ticks: {per_tick:.1} steps a tick",
            run.backend, run.stats.steps, run.report.block_changes
        );
        // the discrete-time solution: y_{k+1} = y_k e^-T + (1 - y_k)(1 - e^-T)
        let e = (-0.01f64).exp();
        let mut y = 1.0;
        for _ in 0..1000 {
            y = y * e + (1.0 - y) * (1.0 - e);
        }
        let err = (run.values[0].last().unwrap() - y).abs();
        assert!(err < 1e-5, "{}: error {err:e}", run.backend);
        assert!(per_tick <= 5.0, "{}: {per_tick:.1} steps a tick", run.backend);
    }
}

/// With light restarts on (they are opt-in), a tick whose output change is
/// a rounding error goes on with the integration's history (no restart),
/// and the run is the one without the change to within the tolerance.
#[test]
fn a_slight_change_at_a_tick_needs_no_restart() {
    let (model, mut info) = held_dae();
    info.blocks = vec![block(vec![0], vec![0], 0.01)];
    let opts =
        SolverOptions { rtol: 1e-8, atol: 1e-10, light_restarts: true, ..Default::default() };
    let grid = OutputGrid { t0: 0.0, t_end: 2.0, dt: 0.1 };
    // u = 0.5 throughout, but each tick moves it by a few ulps
    let mut blocks: Vec<Box<dyn DiscreteBlock>> = vec![Box::new(Sampled {
        period: 0.01,
        offset: 0.0,
        law: |t, _, o| o[0] = 0.5 + 1e-15 * (t * 100.0).round(),
    })];
    let run = simulate(&model, &info, &opts, grid, &mut blocks).unwrap();
    println!(
        "{} changing ticks, {} light, {} restarts, {} steps",
        run.report.block_changes, run.report.light_restarts, run.stats.restarts, run.stats.steps
    );
    // the tick at the start sets u from 0 to 0.5 and restarts; every other
    // tick ends a step (a changing tick makes the next one a stop time) and
    // goes on
    assert_eq!(run.report.block_changes, 201);
    assert_eq!(run.report.light_restarts, 200);
    // y' = 0.5 - y from y(0) = 1: y = 0.5 + 0.5 e^-t
    for (k, t) in run.times.iter().enumerate() {
        let exact = 0.5 + 0.5 * (-t).exp();
        assert!((run.values[0][k] - exact).abs() < 1e-7, "t = {t}: {}", run.values[0][k]);
    }
    // a changing tick makes the next tick a stop time: a step ends at each
    // of the 200 ticks; restarting at each would take several more
    assert!(run.stats.steps < 300, "{} steps", run.stats.steps);
}

/// `when time >= 1`: a time event, reached exactly as a stop time (root
/// finding lands a few ulps after it), and the output point at 1 shows the
/// value just after it.
#[test]
fn a_when_on_time_fires_exactly_at_its_time() {
    // y' = d; when time >= 1: d := 1 (the crossing: time - 1)
    let model = Hand {
        layout: layout(1, 0, 0, 1, 1, 1, 2),
        f: Box::new(|i, out| out[0] = i.d[0]),
        jvp: Box::new(|_, _, out| out[0] = 0.0),
        roots: Box::new(|i, out| out[0] = i.t - 1.0),
        vars: Box::new(|i, out| {
            out[0] = i.y[0];
            out[1] = i.d[0];
        }),
        when: Box::new(|_, fired, d| {
            if fired[0] != 0.0 {
                d[0] = 1.0;
            }
        }),
        modes: None,
        y0: vec![0.0],
        d0: vec![0.0],
    };
    let mut info = RunInfo::bare(1, 2, vec![]);
    info.root_dirs = vec![1];
    whens(&mut info, &[(0, Direction::Rising, "'Step': on")]);
    info.time_crossings = vec![Some(TimeCrossing { at: Expr::Const(1.0), rising: true })];
    for backend in backends() {
        let opts = SolverOptions { backend, rtol: 1e-9, atol: 1e-12, ..Default::default() };
        let grid = OutputGrid { t0: 0.0, t_end: 2.0, dt: 0.25 };
        let run = simulate(&model, &info, &opts, grid, &mut []).unwrap();
        let fired: Vec<f64> =
            run.events.iter().filter(|e| e.kind == EventKind::When(0)).map(|e| e.t).collect();
        assert_eq!(fired, [1.0], "{backend:?}: exactly at 1: {:?}", run.events);
        assert_eq!(run.values[1][at(&run, 1.0)], 1.0, "{backend:?}: after the event at 1");
        assert_eq!(run.values[1][at(&run, 0.75)], 0.0, "{backend:?}");
        for (k, t) in run.times.iter().enumerate() {
            let exact = (t - 1.0).max(0.0);
            assert!((run.values[0][k] - exact).abs() < 1e-9, "{backend:?} t = {t}");
        }
    }
}

/// A mode of `time > 1` (false at 1 exactly, true right after) switches at
/// 1 exactly, and the output point at 1 shows its value just after.
#[test]
fn a_mode_on_time_switches_exactly_at_its_time() {
    // y' = m, m: time > 1 (crossing time - 1; its falling copy too)
    let model = Hand {
        layout: layout(1, 0, 0, 1, 2, 2, 2),
        f: Box::new(|i, out| out[0] = i.d[0]),
        jvp: Box::new(|_, _, out| out[0] = 0.0),
        roots: Box::new(|i, out| {
            out[0] = i.t - 1.0;
            out[1] = i.t - 1.0;
        }),
        vars: Box::new(|i, out| {
            out[0] = i.y[0];
            out[1] = i.d[0];
        }),
        when: Box::new(|_, fired, d| {
            if fired[0] != 0.0 {
                d[0] = 1.0;
            }
            if fired[1] != 0.0 {
                d[0] = 0.0;
            }
        }),
        modes: Some(Box::new(|i, _, d| d[0] = if i.t > 1.0 { 1.0 } else { 0.0 })),
        y0: vec![0.0],
        d0: vec![0.0],
    };
    let mut info = RunInfo::bare(1, 2, vec![]);
    info.root_dirs = vec![1, -1];
    whens(&mut info, &[(0, Direction::Rising, "on"), (1, Direction::Falling, "off")]);
    info.modes = vec![ModeInfo { crossing: 0, discrete: 0, label: "'Timer': on".into() }];
    let tc = Some(TimeCrossing { at: Expr::Const(1.0), rising: true });
    info.time_crossings = vec![tc.clone(), tc];
    for backend in backends() {
        let opts = SolverOptions { backend, rtol: 1e-9, atol: 1e-12, ..Default::default() };
        let grid = OutputGrid { t0: 0.0, t_end: 2.0, dt: 0.25 };
        let run = simulate(&model, &info, &opts, grid, &mut []).unwrap();
        let flips: Vec<f64> = run
            .events
            .iter()
            .filter(|e| matches!(e.kind, EventKind::Mode(_)))
            .map(|e| e.t)
            .collect();
        assert_eq!(flips, [1.0], "{backend:?}: {:?}", run.events);
        assert_eq!(run.values[1][at(&run, 1.0)], 1.0, "{backend:?}");
        assert!((run.values[0].last().unwrap() - 1.0).abs() < 1e-9, "{backend:?}");
    }
}

/// A gear (its ratio the discrete d0) between a motor and a wheel whose
/// tyre drives a vehicle: y = [w (wheel), v (vehicle)]; channels [w, the
/// motor's speed d0·w, v, d0]. The gear declares that a change of its
/// ratio is a rigid engagement; the tyre keeps its slip velocity `w R - v`
/// through an impulse while `grips`.
struct Shift {
    jm: f64,
    jw: f64,
    m: f64,
    rr: f64,
}

impl Shift {
    const CAR: Shift = Shift { jm: 0.05, jw: 1.2, m: 1500.0, rr: 0.3 };

    /// The kinetic energy at wheel speed w, ratio r, vehicle speed v.
    fn energy(&self, w: f64, r: f64, v: f64) -> f64 {
        0.5 * self.jm * (r * w) * (r * w) + 0.5 * self.jw * w * w + 0.5 * self.m * v * v
    }

    /// Stage 1: the motor and the wheel meet, the vehicle keeps its speed.
    fn rigid(&self, w0: f64, r1: f64, r2: f64) -> f64 {
        (self.jm * r1 * r2 + self.jw) * w0 / (self.jm * r2 * r2 + self.jw)
    }

    /// Both stages: the momentum of all three kept, the slip kept.
    fn through(&self, w0: f64, r1: f64, r2: f64) -> f64 {
        let mr2 = self.m * self.rr * self.rr;
        (self.jm * r1 * r2 + self.jw + mr2) * w0 / (self.jm * r2 * r2 + self.jw + mr2)
    }

    /// The model, shifting from r1 to r2 at t = 1, and its run info. `flag`:
    /// a `when v > v_thr` that sets d1 (its threshold), else none.
    fn model(
        &self,
        r1: f64,
        r2: f64,
        y0: [f64; 2],
        grips: bool,
        flag: Option<f64>,
    ) -> (Hand, RunInfo) {
        let n_d = if flag.is_some() { 2 } else { 1 };
        let n_roots = if flag.is_some() { 2 } else { 1 };
        let v_thr = flag.unwrap_or(0.0);
        let model = Hand {
            layout: layout(2, 0, 1, n_d, n_roots, n_roots, 3 + n_d),
            f: Box::new(|_, out| {
                out[0] = 0.0;
                out[1] = 0.0;
            }),
            jvp: Box::new(|_, _, out| {
                out[0] = 0.0;
                out[1] = 0.0;
            }),
            roots: Box::new(move |i, out| {
                out[0] = i.t - 1.0;
                if out.len() > 1 {
                    out[1] = i.y[1] - v_thr;
                }
            }),
            vars: Box::new(|i, out| {
                out[0] = i.y[0];
                out[1] = i.d[0] * i.y[0];
                out[2] = i.y[1];
                out[3] = i.d[0];
                if out.len() > 4 {
                    out[4] = i.d[1];
                }
            }),
            when: Box::new(move |_, fired, d| {
                if fired[0] != 0.0 {
                    d[0] = r2;
                }
                if fired.len() > 1 && fired[1] != 0.0 {
                    d[1] = 1.0;
                }
            }),
            modes: None,
            y0: y0.to_vec(),
            d0: if flag.is_some() { vec![r1, 0.0] } else { vec![r1] },
        };
        let mut info = RunInfo::bare(2, 3 + n_d, vec![self.rr]);
        info.root_dirs = vec![1; n_roots];
        let mut w = vec![(0, Direction::Rising, "'Gearbox': shift")];
        if flag.is_some() {
            w.push((1, Direction::Rising, "'Flag': v > v_thr"));
        }
        whens(&mut info, &w);
        info.when_strict = vec![false, true][..n_roots].to_vec();
        info.time_crossings = vec![Some(TimeCrossing { at: Expr::Const(1.0), rising: true })];
        if flag.is_some() {
            info.time_crossings.push(None);
        }
        info.y_nominal = vec![100.0, 10.0];
        info.var_sources =
            vec![VarSource::Y(0), VarSource::Computed, VarSource::Y(1), VarSource::D(0)];
        if flag.is_some() {
            info.var_sources.push(VarSource::D(1));
        }
        let v = |k: u32| Expr::Var(VarId(k));
        let half = |c: f64, k: u32| Expr::Const(0.5 * c) * v(k) * v(k);
        let part = |path: &str, stored: Option<Expr>| EnergyPart {
            path: path.into(),
            name: format!("'{path}'"),
            power: Expr::Const(0.0),
            loss: None,
            stored,
        };
        info.energy = Some(Arc::new(EnergyInfo {
            parts: vec![
                part("motor", Some(half(self.jm, 1))),
                part("wheel", Some(half(self.jw, 0))),
                part("body", Some(half(self.m, 2))),
                part("gearbox", None),
                part("tyre", None),
            ],
        }));
        info.impulse = Some(Arc::new(ImpulseInfo::new(
            vec![(0, vec![1]), (1, vec![0]), (2, vec![2])],
            vec![EngagementInfo { changes: v(3), part: Some(3) }],
            vec![ImpulseLink {
                keep: v(0) * Expr::Param(ParamId(0)) - v(2),
                active: Expr::Const(if grips { 1.0 } else { 0.0 }),
                part: Some(4),
            }],
            vec![(1, false, v(3) * v(0))],
            vec![],
            &info.var_sources,
            2,
        )));
        (model, info)
    }
}

/// The shift's books: (gearbox, tyre, all) lost at the engagement.
fn shift_books(run: &SimResult) -> (f64, f64, f64) {
    let books = run.energy.as_ref().unwrap();
    let book = |path: &str| books.parts.iter().find(|p| p.path == path).unwrap().impulse_lost;
    (book("gearbox"), book("tyre"), books.impulse_loss)
}

/// A motor geared to a wheel whose tyre grips a vehicle; the gear's ratio
/// steps from r1 to r2 at t = 1. The momentum of all three is kept, the
/// vehicle's reflected through the gripping tyre (its slip velocity is
/// kept): w⁺ = (J_m r1 r2 + J_w + m R²) w⁻ / (J_m r2² + J_w + m R²). The
/// loss splits as the physics does: the motor and the wheel meet first
/// (a rigid engagement, the vehicle not yet involved: the gearbox's loss),
/// then the tyre's slip relaxes back, passing the momentum on to the
/// vehicle (the tyre's loss). At its grip limit the tyre passes nothing:
/// the vehicle keeps its speed and the gearbox's loss is all there is.
#[test]
fn a_shift_keeps_the_momentum_through_a_tyre_that_grips() {
    let s = Shift::CAR;
    let (r1, r2) = (12.0, 7.0);
    let (w0, v0) = (60.0, 60.0 * s.rr - 0.2);
    for grips in [true, false] {
        let (model, info) = s.model(r1, r2, [w0, v0], grips, None);
        let opts = SolverOptions { rtol: 1e-10, atol: 1e-10, ..Default::default() };
        let run =
            simulate(&model, &info, &opts, OutputGrid { t0: 0.0, t_end: 2.0, dt: 0.5 }, &mut [])
                .unwrap();
        let wa = s.rigid(w0, r1, r2);
        let (w1, v1) = if grips {
            let w1 = s.through(w0, r1, r2);
            (w1, v0 + s.rr * (w1 - w0))
        } else {
            (wa, v0)
        };
        let k = at(&run, 1.0);
        let (gear, tyre, all) = shift_books(&run);
        let in_gear = s.energy(w0, r1, v0) - s.energy(wa, r2, v0);
        let in_tyre = s.energy(wa, r2, v0) - s.energy(w1, r2, v1);
        println!(
            "grips {grips}: w {} (exact {w1}), v {} (exact {v1}), {} impulses; lost: gearbox \
             {gear} J (exact {in_gear}), tyre {tyre} J (exact {in_tyre})",
            run.values[0][k], run.values[2][k], run.report.impulses
        );
        assert_eq!(run.report.impulses, 1);
        assert!((run.values[0][k] - w1).abs() < 1e-12 * w1, "grips {grips}: w");
        assert!((run.values[2][k] - v1).abs() < 1e-12 * v1, "grips {grips}: v");
        let lost = in_gear + in_tyre;
        assert!(in_gear > 0.0 && in_tyre >= 0.0);
        assert!((gear - in_gear).abs() < 1e-9 * lost, "gearbox {gear} vs {in_gear}");
        assert!((tyre - in_tyre).abs() < 1e-9 * lost, "tyre {tyre} vs {in_tyre}");
        assert!((all - lost).abs() < 1e-9 * lost);
        let books = run.energy.as_ref().unwrap();
        assert!((books.impulse_link_loss - in_tyre).abs() < 1e-9 * lost);
    }
}

/// A downshift while the tyre drives (slip velocity +0.2 m/s): the rotor
/// must speed up, so the motor and the wheel meet at a wheel speed well
/// below the vehicle's, and the tyre's slip, relaxing back to +0.2 m/s,
/// decelerates the vehicle. Both shares are losses (the review found the
/// tyre booking a negative one, -113 J, when it took the impulse times its
/// slip before the event): a friction contact cannot return energy.
#[test]
fn a_gripping_tyre_books_no_negative_loss_on_a_downshift() {
    let s = Shift::CAR;
    let (r1, r2) = (7.0, 12.0);
    let (w0, v0) = (60.0, 60.0 * s.rr - 0.2);
    let (model, info) = s.model(r1, r2, [w0, v0], true, None);
    let opts = SolverOptions { rtol: 1e-10, atol: 1e-10, ..Default::default() };
    let run = simulate(&model, &info, &opts, OutputGrid { t0: 0.0, t_end: 2.0, dt: 0.5 }, &mut [])
        .unwrap();
    let k = at(&run, 1.0);
    let (gear, tyre, all) = shift_books(&run);
    let wa = s.rigid(w0, r1, r2);
    let w1 = s.through(w0, r1, r2);
    let v1 = v0 + s.rr * (w1 - w0);
    let lost = s.energy(w0, r1, v0) - s.energy(w1, r2, v1);
    println!(
        "downshift: v {v0} -> {} m/s (exact {v1}), slip after the rigid stage {:.4} m/s, kept \
         0.2 m/s; lost {all} J (exact {lost}): gearbox {gear} J, tyre {tyre} J",
        run.values[2][k],
        wa * s.rr - v0
    );
    assert!((run.values[2][k] - v1).abs() < 1e-12 * v1);
    assert!(gear > 0.0, "gearbox {gear}");
    assert!(tyre >= 0.0, "a friction contact books a negative loss: {tyre} J");
    assert!((all - lost).abs() < 1e-9 * lost);
    let in_tyre = s.energy(wa, r2, v0) - s.energy(w1, r2, v1);
    assert!((tyre - in_tyre).abs() < 1e-9 * lost);
}

/// The upshift of the test above with `when v > v_thr: flag := 1` for
/// v⁻ < v_thr < v⁺: the projection moves v across the threshold at the
/// shift, and event iteration goes on from the moved states (as Modelica
/// re-checks every condition after a reinit at the same instant), so the
/// `when` fires at the shift. (The review found it never firing: the
/// projection ran after the iteration had settled.)
#[test]
fn a_condition_the_projection_crosses_fires_at_the_shift() {
    let s = Shift::CAR;
    let (r1, r2) = (12.0, 7.0);
    let (w0, v0) = (60.0, 60.0 * s.rr - 0.2);
    let w1 = s.through(w0, r1, r2);
    let v1 = v0 + s.rr * (w1 - w0);
    let v_thr = 0.5 * (v0 + v1);
    let (model, info) = s.model(r1, r2, [w0, v0], true, Some(v_thr));
    let opts = SolverOptions { rtol: 1e-10, atol: 1e-10, ..Default::default() };
    let run = simulate(&model, &info, &opts, OutputGrid { t0: 0.0, t_end: 2.0, dt: 0.5 }, &mut [])
        .unwrap();
    let k = at(&run, 1.0);
    let events: Vec<(String, f64)> = run.events.iter().map(|e| (e.label.clone(), e.t)).collect();
    println!(
        "v {v0} -> {} (threshold {v_thr}); flag at 1: {}, at the end: {}; events {events:?}",
        run.values[2][k],
        run.values[4][k],
        run.values[4].last().unwrap()
    );
    assert!((run.values[2][k] - v1).abs() < 1e-9, "the projection moved v");
    assert_eq!(run.values[4][k], 1.0, "the when on v > v_thr fires at the shift");
    assert!(events.iter().any(|(l, t)| l.contains("v > v_thr") && *t == 1.0), "{events:?}");
}

/// The two-stage split against the physics it stands for: a tyre with a
/// linear, stiff force law F = k (w R - v) and no projection through it
/// (the gear's rigid engagement only), its slip transient integrated, a
/// constant motor torque driving. The tyre's loss over the transient (its
/// ∫ F (w R - v) dt, an extra state) and the speeds after it approach the
/// projection's stage-2 loss and end state as k grows; the gearbox's loss
/// is the same rigid stage in both.
#[test]
fn the_tyres_share_is_what_a_stiff_tyre_dissipates() {
    let s = Shift::CAR;
    let (r1, r2, torque) = (12.0, 7.0, 50.0);
    let t_after = 1.05;
    // y = [w, v, q]: q the tyre's dissipated energy
    let model = |k: f64| {
        let (jm, jw, m, rr) = (s.jm, s.jw, s.m, s.rr);
        let f = move |i: &EvalInput<'_>| k * (i.y[0] * rr - i.y[1]);
        Hand {
            layout: layout(3, 0, 1, 1, 1, 1, 5),
            f: Box::new(move |i, out| {
                let (r, force) = (i.d[0], f(i));
                out[0] = (r * torque - rr * force) / (jm * r * r + jw);
                out[1] = force / m;
                out[2] = force * (i.y[0] * rr - i.y[1]);
            }),
            jvp: Box::new(move |i, v, out| {
                let r = i.d[0];
                let ds = v[0] * rr - v[1];
                let s_now = i.y[0] * rr - i.y[1];
                out[0] = -rr * k * ds / (jm * r * r + jw);
                out[1] = k * ds / m;
                out[2] = 2.0 * k * s_now * ds;
            }),
            roots: Box::new(|i, out| out[0] = i.t - 1.0),
            vars: Box::new(|i, out| {
                out[0] = i.y[0];
                out[1] = i.d[0] * i.y[0];
                out[2] = i.y[1];
                out[3] = i.d[0];
                out[4] = i.y[2];
            }),
            when: Box::new(move |_, fired, d| {
                if fired[0] != 0.0 {
                    d[0] = r2;
                }
            }),
            modes: None,
            y0: vec![60.0, 60.0 * rr, 0.0],
            d0: vec![r1],
        }
    };
    let run = |k: f64, link: bool| {
        let (_, mut info) = s.model(r1, r2, [0.0, 0.0], link, None);
        info.y_nominal = vec![100.0, 10.0, 1.0];
        info.y_names = vec!["w".into(), "v".into(), "q".into()];
        info.var_names.push("q".into());
        info.var_sources.push(VarSource::Y(2));
        let imp = info.impulse.as_ref().unwrap();
        info.impulse = Some(Arc::new(ImpulseInfo::new(
            imp.parts.clone(),
            imp.engagements.clone(),
            if link { imp.links.clone() } else { vec![] },
            imp.chain.clone(),
            vec![],
            &info.var_sources,
            3,
        )));
        let opts = SolverOptions { rtol: 1e-11, atol: 1e-11, ..Default::default() };
        let grid = OutputGrid { t0: 0.0, t_end: t_after, dt: 0.05 };
        simulate(&model(k), &info, &opts, grid, &mut []).unwrap()
    };
    let mut gaps = vec![];
    for k in [2e4, 2e5, 2e6] {
        let (resolved, projected) = (run(k, false), run(k, true));
        let (i0, i1) = (at(&resolved, 1.0) - 1, resolved.times.len() - 1);
        assert!((resolved.times[i0] - 0.95).abs() < 1e-12);
        // the tyre's dissipation over the transient, beyond what the
        // projected run dissipates over the same time
        let q = |r: &SimResult| r.values[4][i1] - r.values[4][i0];
        let (gear_r, tyre_r, _) = shift_books(&resolved);
        let (gear_p, tyre_p, _) = shift_books(&projected);
        let resolved_tyre = q(&resolved);
        let projected_tyre = tyre_p + q(&projected);
        let dv = (resolved.values[2][i1] - projected.values[2][i1]).abs();
        let gap = (resolved_tyre - projected_tyre).abs() / projected_tyre;
        println!(
            "k {k:.0e}: tyre lost {resolved_tyre:.6} J resolved, {projected_tyre:.6} J with the \
             projection ({tyre_p:.6} J at the shift), relative gap {gap:.2e}; gearbox {gear_r:.6} \
             / {gear_p:.6} J; v after {:.9} / {:.9} m/s",
            resolved.values[2][i1], projected.values[2][i1]
        );
        assert_eq!(tyre_r, 0.0, "no link, no projection through the tyre");
        assert!((gear_r - gear_p).abs() < 1e-9 * gear_p, "the same rigid stage");
        gaps.push((gap, dv));
    }
    // the gap closes as the tyre stiffens (about tenfold per decade)
    assert!(gaps[1].0 < 0.2 * gaps[0].0 && gaps[2].0 < 0.2 * gaps[1].0, "{gaps:?}");
    assert!(gaps[2].0 < 1e-3, "{gaps:?}");
    assert!(gaps[2].1 < 1e-4, "{gaps:?}");
}

/// A rigid engagement and a `reinit` at the same event: a latch stops the
/// load (`reinit(load.w, 0)`) while the gear between it and the motor
/// shifts. The load's speed stays where the reinit put it (the review found
/// the projection moving a restarted state back); without the reinit the
/// projection keeps the momentum: J_l w⁺ + r2 J_m (r2 w⁺) = J_l w⁻ + r2 J_m
/// (r1 w⁻).
#[test]
fn a_state_a_reinit_sets_stays_where_it_put_it() {
    let (jm, jl, r1, r2, w0) = (0.05, 2.0, 12.0, 7.0, 10.0);
    for latch in [true, false] {
        // y = [load.w.continuous]; d = [ratio, load.w.jump]
        // channels: 0 load.w.continuous, 1 ratio, 2 jump, 3 load.w, 4 motor.w
        let model = Hand {
            layout: layout(1, 0, 0, 2, 1, 1, 5),
            f: Box::new(|_, out| out[0] = 0.0),
            jvp: Box::new(|_, _, out| out[0] = 0.0),
            roots: Box::new(|i, out| out[0] = i.t - 1.0),
            vars: Box::new(|i, out| {
                out[0] = i.y[0];
                out[1] = i.d[0];
                out[2] = i.d[1];
                out[3] = i.y[0] + i.d[1];
                out[4] = i.d[0] * (i.y[0] + i.d[1]);
            }),
            when: Box::new(move |i, fired, d| {
                if fired[0] != 0.0 {
                    d[0] = r2;
                    if latch {
                        d[1] = -i.y[0];
                    }
                }
            }),
            modes: None,
            y0: vec![w0],
            d0: vec![r1, 0.0],
        };
        let v = |k: u32| Expr::Var(VarId(k));
        let half = |c: f64, k: u32| Expr::Const(0.5 * c) * v(k) * v(k);
        let mut info = RunInfo::bare(1, 5, vec![]);
        info.root_dirs = vec![1];
        whens(&mut info, &[(0, Direction::Rising, "'Gear': shift; 'Latch': stop")]);
        info.time_crossings = vec![Some(TimeCrossing { at: Expr::Const(1.0), rising: true })];
        info.var_sources = vec![
            VarSource::Y(0),
            VarSource::D(0),
            VarSource::D(1),
            VarSource::Computed,
            VarSource::Computed,
        ];
        let part = |path: &str, stored: Option<Expr>| EnergyPart {
            path: path.into(),
            name: format!("'{path}'"),
            power: Expr::Const(0.0),
            loss: None,
            stored,
        };
        info.energy = Some(Arc::new(EnergyInfo {
            parts: vec![
                part("motor", Some(half(jm, 4))),
                part("load", Some(half(jl, 3))),
                part("gear", None),
            ],
        }));
        info.impulse = Some(Arc::new(ImpulseInfo::new(
            vec![(0, vec![4]), (1, vec![3])],
            vec![EngagementInfo { changes: v(1), part: Some(2) }],
            vec![],
            vec![(3, false, v(0) + v(2)), (4, false, v(1) * v(3))],
            vec![(0, 1)],
            &info.var_sources,
            1,
        )));
        let opts = SolverOptions { rtol: 1e-10, atol: 1e-10, ..Default::default() };
        let run =
            simulate(&model, &info, &opts, OutputGrid { t0: 0.0, t_end: 2.0, dt: 0.5 }, &mut [])
                .unwrap();
        let k = at(&run, 1.0);
        let w = run.values[3][k];
        let exact = if latch { 0.0 } else { (jl + r1 * r2 * jm) * w0 / (jl + r2 * r2 * jm) };
        println!(
            "latch {latch}: load.w(1) = {w} (exact {exact}), {} projections",
            run.report.impulses
        );
        assert!((w - exact).abs() < 1e-12 * w0, "latch {latch}: {w}");
        assert_eq!(run.report.impulses, if latch { 0 } else { 1 });
        assert!((run.values[3].last().unwrap() - exact).abs() < 1e-12 * w0);
    }
}

/// The projection keeps the momentum of energies that are not quadratic
/// exactly, by Newton's method on exact derivatives: two flywheels whose
/// stored energies stiffen with speed, E = ½ J w² + ¼ c w⁴ (momentum
/// J w + c w³), joined by a gear whose ratio steps from r1 to r2. Exact:
/// r2 p_a(r2 x) + p_b(x) = r2 p_a(w_a⁻) + p_b(w_b⁻), solved here by
/// bisection.
#[test]
fn the_momentum_of_a_non_quadratic_energy_is_kept() {
    let (ja, ca, jb, cb, r1, r2) = (0.05, 2e-6, 3.0, 1e-4, 12.0, 7.0);
    let x0 = 20.0;
    let pa = |w: f64| ja * w + ca * w * w * w;
    let pb = |w: f64| jb * w + cb * w * w * w;
    let target = r2 * pa(r1 * x0) + pb(x0);
    let (mut lo, mut hi) = (0.0, 10.0 * x0);
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if r2 * pa(r2 * mid) + pb(mid) > target {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    let exact = 0.5 * (lo + hi);
    // y = [x (b's speed)]; d = [ratio]; channels: 0 x, 1 ratio, 2 a's speed
    let model = Hand {
        layout: layout(1, 0, 0, 1, 1, 1, 3),
        f: Box::new(|_, out| out[0] = 0.0),
        jvp: Box::new(|_, _, out| out[0] = 0.0),
        roots: Box::new(|i, out| out[0] = i.t - 1.0),
        vars: Box::new(|i, out| {
            out[0] = i.y[0];
            out[1] = i.d[0];
            out[2] = i.d[0] * i.y[0];
        }),
        when: Box::new(move |_, fired, d| {
            if fired[0] != 0.0 {
                d[0] = r2;
            }
        }),
        modes: None,
        y0: vec![x0],
        d0: vec![r1],
    };
    let v = |k: u32| Expr::Var(VarId(k));
    let e = |j: f64, c: f64, k: u32| {
        Expr::Const(0.5 * j) * v(k) * v(k) + Expr::Const(0.25 * c) * v(k) * v(k) * v(k) * v(k)
    };
    let mut info = RunInfo::bare(1, 3, vec![]);
    info.root_dirs = vec![1];
    whens(&mut info, &[(0, Direction::Rising, "'Gear': shift")]);
    info.time_crossings = vec![Some(TimeCrossing { at: Expr::Const(1.0), rising: true })];
    info.var_sources = vec![VarSource::Y(0), VarSource::D(0), VarSource::Computed];
    let part = |path: &str, stored: Option<Expr>| EnergyPart {
        path: path.into(),
        name: format!("'{path}'"),
        power: Expr::Const(0.0),
        loss: None,
        stored,
    };
    info.energy = Some(Arc::new(EnergyInfo {
        parts: vec![
            part("a", Some(e(ja, ca, 2))),
            part("b", Some(e(jb, cb, 0))),
            part("gear", None),
        ],
    }));
    info.impulse = Some(Arc::new(ImpulseInfo::new(
        vec![(0, vec![2]), (1, vec![0])],
        vec![EngagementInfo { changes: v(1), part: Some(2) }],
        vec![],
        vec![(2, false, v(1) * v(0))],
        vec![],
        &info.var_sources,
        1,
    )));
    let opts = SolverOptions { rtol: 1e-10, atol: 1e-10, ..Default::default() };
    let run = simulate(&model, &info, &opts, OutputGrid { t0: 0.0, t_end: 2.0, dt: 0.5 }, &mut [])
        .unwrap();
    let k = at(&run, 1.0);
    let x = run.values[0][k];
    let momentum = r2 * pa(r2 * x) + pb(x);
    println!(
        "x after {x} (exact {exact}, {:.1e}); momentum {momentum} (before {target}, {:.1e})",
        (x - exact) / exact,
        (momentum - target) / target
    );
    assert!((x - exact).abs() < 1e-13 * exact, "{x} vs {exact}");
    assert!((momentum - target).abs() < 1e-13 * target);
}

/// A state event and a time crossing at the same instant (within
/// SUNDIALS' root tolerance): `when x >= ...: a := 1` (root finding) and
/// `when time >= 1: b := 1` (a time crossing, scheduled exactly). Both
/// must fire.
#[test]
fn a_time_crossing_coinciding_with_a_root_still_fires() {
    let model = Hand {
        layout: layout(1, 0, 0, 2, 2, 2, 3),
        f: Box::new(|_, out| out[0] = 1.0),
        jvp: Box::new(|_, _, out| out[0] = 0.0),
        roots: Box::new(|i, out| {
            out[0] = i.y[0] - 1.0;
            out[1] = i.t - 1.0;
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
        modes: None,
        y0: vec![0.0],
        d0: vec![0.0, 0.0],
    };
    let mut info = RunInfo::bare(1, 3, vec![]);
    info.root_dirs = vec![1, 1];
    whens(
        &mut info,
        &[(0, Direction::Rising, "'A': y >= 1"), (1, Direction::Rising, "'B': time >= 1")],
    );
    info.time_crossings = vec![None, Some(TimeCrossing { at: Expr::Const(1.0), rising: true })];
    // the same with `when time >= (1 - 1e-15) + x` for a state x that
    // rests at 0 (x' = 0): its root lies 1e-15 s before the stop time 1,
    // inside SUNDIALS' root tolerance, so the integrator reports it at 1
    // exactly, as a root (not as the stop time)
    let resting = Hand {
        f: Box::new(|_, out| out[0] = 0.0),
        roots: Box::new(|i, out| {
            out[0] = i.t - (1.0 - 1e-15) - i.y[0];
            out[1] = i.t - 1.0;
        }),
        ..model
    };
    let resting = Hand { y0: vec![0.0], ..resting };
    for (dt, model) in [(0.25, &resting), (0.3, &resting)] {
        let opts = SolverOptions { rtol: 1e-9, atol: 1e-12, ..Default::default() };
        let grid = OutputGrid { t0: 0.0, t_end: 2.0, dt };
        let run = simulate(model, &info, &opts, grid, &mut []).unwrap();
        let fired: Vec<(usize, f64)> = run
            .events
            .iter()
            .filter_map(|e| match e.kind {
                EventKind::When(k) => Some((k, e.t)),
                _ => None,
            })
            .collect();
        println!("dt {dt}: whens fired {fired:?}; b at the end {}", run.values[2].last().unwrap());
        assert_eq!(*run.values[1].last().unwrap(), 1.0, "A fired");
        assert_eq!(*run.values[2].last().unwrap(), 1.0, "B (time >= 1) fired: {fired:?}");
    }
}

/// `if time > t_last` with `t_last := time` set by an event at t = 1 (a
/// timer that restarts): the relation is false at 1 exactly and true right
/// after. y' = m (the mode), so y(2) = 1. The time crossing `time - t_last`
/// is taken out of root finding; after the event its time is now, so it is
/// not scheduled, and the mode is not checked at step ends either.
#[test]
fn a_time_mode_whose_time_is_set_to_now_switches_right_after() {
    // d = [m, t_last]; crossings: 0 `time - 1` (when: t_last := time),
    // 1 and 2 `time - t_last` (the mode and its falling copy)
    let model = Hand {
        layout: layout(1, 0, 0, 2, 3, 3, 3),
        f: Box::new(|i, out| out[0] = i.d[0]),
        jvp: Box::new(|_, _, out| out[0] = 0.0),
        roots: Box::new(|i, out| {
            out[0] = i.t - 1.0;
            out[1] = i.t - i.d[1];
            out[2] = i.t - i.d[1];
        }),
        vars: Box::new(|i, out| {
            out[0] = i.y[0];
            out[1] = i.d[0];
            out[2] = i.d[1];
        }),
        when: Box::new(|i, fired, d| {
            if fired[0] != 0.0 {
                d[1] = i.t;
            }
            if fired[1] != 0.0 {
                d[0] = 1.0;
            }
            if fired[2] != 0.0 {
                d[0] = 0.0;
            }
        }),
        modes: Some(Box::new(|i, _, d| d[0] = if i.t > i.d[1] { 1.0 } else { 0.0 })),
        y0: vec![0.0],
        d0: vec![0.0, 5.0],
    };
    let mut info = RunInfo::bare(1, 3, vec![]);
    info.root_dirs = vec![1, 1, -1];
    whens(
        &mut info,
        &[
            (0, Direction::Rising, "'Timer': restart"),
            (1, Direction::Rising, "on"),
            (2, Direction::Falling, "off"),
        ],
    );
    info.modes =
        vec![ModeInfo { crossing: 1, discrete: 0, label: "'Timer': time > t_last".into() }];
    let t_last = || Expr::Var(VarId(2));
    let tcs = vec![
        Some(TimeCrossing { at: Expr::Const(1.0), rising: true }),
        Some(TimeCrossing { at: t_last(), rising: true }),
        Some(TimeCrossing { at: t_last(), rising: true }),
    ];
    for timed in [false, true] {
        info.time_crossings = if timed { tcs.clone() } else { vec![] };
        let opts = SolverOptions { rtol: 1e-9, atol: 1e-12, ..Default::default() };
        let grid = OutputGrid { t0: 0.0, t_end: 2.0, dt: 0.25 };
        let run = simulate(&model, &info, &opts, grid, &mut []).unwrap();
        let y_end = *run.values[0].last().unwrap();
        println!(
            "time crossings scheduled: {timed}: mode at the end {}, y(2) = {y_end} (exact 1), \
             events {:?}",
            run.values[1].last().unwrap(),
            run.events.iter().map(|e| (e.label.clone(), e.t)).collect::<Vec<_>>()
        );
        assert!((y_end - 1.0).abs() < 1e-6, "timed {timed}: y(2) = {y_end}, exact 1");
    }
}

/// A periodic timer: `when time >= t_next: t_next := t_next + 0.5`, and
/// `y' = if time >= t_next then 1 else 0`. At each instant t_next the
/// when moves t_next on, so the relation re-evaluated in the event
/// iteration is false again: y stays 0 (Modelica). The mode's crossing is
/// due at the same instant as the when's, and `right_limits` sets it to
/// its value just after the *old* t_next.
#[test]
fn a_time_mode_rearmed_at_its_own_instant() {
    // d = [mode, t_next]; crossings: 0 when, 1 mode, 2 its falling copy
    let model = Hand {
        layout: layout(1, 0, 0, 2, 3, 3, 3),
        f: Box::new(|i, out| out[0] = i.d[0]),
        jvp: Box::new(|_, _, out| out[0] = 0.0),
        roots: Box::new(|i, out| {
            out[0] = i.t - i.d[1];
            out[1] = i.t - i.d[1];
            out[2] = i.t - i.d[1];
        }),
        vars: Box::new(|i, out| {
            out[0] = i.y[0];
            out[1] = i.d[0];
            out[2] = i.d[1];
        }),
        when: Box::new(|i, fired, d| {
            if fired[0] != 0.0 {
                d[1] = i.d[1] + 0.5;
            }
            if fired[1] != 0.0 {
                d[0] = 1.0;
            }
            if fired[2] != 0.0 {
                d[0] = 0.0;
            }
        }),
        modes: Some(Box::new(|i, _, d| d[0] = if i.t >= i.d[1] { 1.0 } else { 0.0 })),
        y0: vec![0.0],
        d0: vec![0.0, 0.5],
    };
    let mut info = RunInfo::bare(1, 3, vec![]);
    info.root_dirs = vec![1, 1, -1];
    whens(
        &mut info,
        &[
            (0, Direction::Rising, "'Timer': t_next += 0.5"),
            (1, Direction::Rising, "on"),
            (2, Direction::Falling, "off"),
        ],
    );
    info.modes =
        vec![ModeInfo { crossing: 1, discrete: 0, label: "'Timer': time >= t_next".into() }];
    let tc = || Some(TimeCrossing { at: Expr::Var(VarId(2)), rising: true });
    for timed in [false, true] {
        info.time_crossings = if timed { vec![tc(), tc(), tc()] } else { vec![] };
        let opts = SolverOptions { rtol: 1e-9, atol: 1e-12, ..Default::default() };
        let grid = OutputGrid { t0: 0.0, t_end: 2.2, dt: 0.1 };
        let run = simulate(&model, &info, &opts, grid, &mut []).unwrap();
        let y_end = *run.values[0].last().unwrap();
        println!(
            "time crossings scheduled: {timed}: y(2.2) = {y_end:.6} (Modelica 0), t_next = {}, \
             mode at 0.6: {}",
            run.values[2].last().unwrap(),
            run.values[1][6]
        );
        assert!(y_end.abs() < 1e-6, "timed {timed}: y(2.2) = {y_end}");
    }
}

/// A sampled block's law: (t, inputs, outputs).
type Law = Box<dyn Fn(f64, &[f64], &mut [f64]) + Send>;

/// A sampled block whose law is a closure.
struct SampledBy {
    period: f64,
    law: Law,
}

impl DiscreteBlock for SampledBy {
    fn name(&self) -> &str {
        "'Controller'"
    }
    fn period(&self) -> f64 {
        self.period
    }
    fn init(&mut self, _t: f64, _i: &[f64], _o: &mut [f64]) -> Result<(), String> {
        Ok(())
    }
    fn tick(&mut self, t: f64, i: &[f64], o: &mut [f64]) -> Result<(), String> {
        (self.law)(t, i, o);
        Ok(())
    }
}

/// The SUNDIALS integrator that never resumes (`resume` left at the
/// trait's default, false): every changing tick restarts it.
struct NoResume<'m>(lsim_solve::sundials::Sundials<'m>);

impl Integrator for NoResume<'_> {
    fn name(&self) -> &'static str {
        self.0.name()
    }
    fn step(&mut self, t_stop: f64) -> Result<Step, SolveError> {
        self.0.step(t_stop)
    }
    fn y(&self) -> &[f64] {
        self.0.y()
    }
    fn interpolate(&mut self, t: f64, out: &mut [f64]) -> Result<(), SolveError> {
        self.0.interpolate(t, out)
    }
    fn interpolate_select(
        &mut self,
        t: f64,
        idx: &[usize],
        out: &mut [f64],
    ) -> Result<(), SolveError> {
        self.0.interpolate_select(t, idx, out)
    }
    fn discrete_mut(&mut self) -> &mut [f64] {
        self.0.discrete_mut()
    }
    fn restart(&mut self, t: f64, y: &[f64]) -> Result<(), SolveError> {
        self.0.restart(t, y)
    }
    fn planned_step(&self) -> f64 {
        self.0.planned_step()
    }
    fn consistent_z(&mut self, t: f64, y: &mut [f64], d: &[f64]) -> Result<(), SolveError> {
        self.0.consistent_z(t, y, d)
    }
    fn stats(&self) -> SolverStats {
        self.0.stats()
    }
    fn quadrature(&mut self, t: f64, out: &mut [f64]) -> Result<(), SolveError> {
        self.0.quadrature(t, out)
    }
    fn local_error(&mut self, out: &mut [f64]) -> bool {
        self.0.local_error(out)
    }
    fn set_root_sides(&mut self, sides: &[f64]) {
        self.0.set_root_sides(sides)
    }
    fn set_root_mask(&mut self, mask: &[bool]) -> bool {
        self.0.set_root_mask(mask)
    }
    fn method(&self) -> String {
        self.0.method()
    }
}

/// How a run treats a changing tick: restart every time, the default, or
/// with light restarts on.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Ticks {
    Restart,
    Default,
    Light,
}

fn run_ticks(
    model: &Hand,
    info: &RunInfo,
    opts: &SolverOptions,
    grid: OutputGrid,
    blocks: &mut [Box<dyn DiscreteBlock>],
    how: Ticks,
) -> SimResult {
    match how {
        Ticks::Default => simulate(model, info, opts, grid, blocks).unwrap(),
        Ticks::Light => {
            let opts = SolverOptions { light_restarts: true, ..opts.clone() };
            simulate(model, info, &opts, grid, blocks).unwrap()
        }
        Ticks::Restart => {
            let l = *model.layout();
            let mut y0 = vec![0.0; l.n_y()];
            let mut d0 = vec![0.0; l.n_d];
            model.start(&info.params, &mut y0, &mut d0);
            let integ =
                lsim_solve::sundials::Sundials::new(model, info, opts, grid, &y0, d0, vec![], None)
                    .unwrap();
            let mut integ = NoResume(integ);
            run_loop(model, info, opts, grid, &mut integ, &[], blocks, std::time::Instant::now())
                .unwrap()
        }
    }
}

/// Sample-and-hold drive of a pure integrator, x' = u_k (a position from a
/// held velocity command), and of a first-order lag, x' = (u_k - x)/tau,
/// each as an ODE (CVODE) and with an algebraic copy z = x (IDA).
/// u_k = A sin(w t_k) changes a little at every tick: each changing tick is
/// a kink of x'. Exact: piecewise from the held values. The default run
/// restarts at each changing tick and is as accurate as forced restarts;
/// light restarts (opt-in) carry each kink into the next steps through the
/// integration's history, and the error that leaves accumulates (from the
/// review: 5e-6 against 3e-15 at rtol 1e-6, when light restarts were the
/// default).
#[test]
fn light_restarts_cost_accuracy_against_an_exact_answer() {
    let period = 0.01;
    let t_end = 20.0;
    let (amp, w) = (1e-3, 1.0);
    for tau in [f64::INFINITY, 0.5] {
        for n_z in [0usize, 1] {
            for rtol in [1e-6, 1e-8] {
                let model = Hand {
                    layout: layout(1, n_z, 0, 1, 0, 0, 2),
                    f: Box::new(move |i, out| {
                        out[0] = if tau.is_finite() { (i.d[0] - i.y[0]) / tau } else { i.d[0] };
                        if out.len() > 1 {
                            out[1] = i.y[1] - i.y[0];
                        }
                    }),
                    jvp: Box::new(move |_, v, out| {
                        out[0] = if tau.is_finite() { -v[0] / tau } else { 0.0 };
                        if out.len() > 1 {
                            out[1] = v[1] - v[0];
                        }
                    }),
                    roots: Box::new(|_, _| {}),
                    vars: Box::new(|i, out| {
                        out[0] = i.y[0];
                        out[1] = i.d[0];
                    }),
                    when: Box::new(|_, _, _| {}),
                    modes: None,
                    y0: if n_z == 1 { vec![1.0, 1.0] } else { vec![1.0] },
                    d0: vec![0.0],
                };
                let mut info = RunInfo::bare(1 + n_z, 2, vec![]);
                info.var_sources = vec![VarSource::Y(0), VarSource::D(0)];
                info.blocks = vec![BlockInfo {
                    name: "'Controller'".into(),
                    chains: vec![],
                    inputs: vec![],
                    outputs: vec![0],
                    period,
                }];
                let opts = SolverOptions { rtol, atol: rtol, ..Default::default() };
                let grid = OutputGrid { t0: 0.0, t_end, dt: 0.5 };
                // the exact answer on the grid, from the held values
                let u_k = |k: usize| amp * (w * k as f64 * period).sin();
                let exact = |t: f64| {
                    let mut x = 1.0;
                    let mut k = 0usize;
                    loop {
                        let t0 = k as f64 * period;
                        let t1 = ((k + 1) as f64 * period).min(t);
                        if t1 <= t0 {
                            return x;
                        }
                        let h = t1 - t0;
                        x = if tau.is_finite() {
                            u_k(k) + (x - u_k(k)) * (-h / tau).exp()
                        } else {
                            x + u_k(k) * h
                        };
                        k += 1;
                    }
                };
                let mut res = vec![];
                for how in [Ticks::Restart, Ticks::Default, Ticks::Light] {
                    let mut blocks: Vec<Box<dyn DiscreteBlock>> = vec![Box::new(SampledBy {
                        period,
                        law: Box::new(move |t, _, o| o[0] = amp * (w * t).sin()),
                    })];
                    let run = run_ticks(&model, &info, &opts, grid, &mut blocks, how);
                    let err = run
                        .times
                        .iter()
                        .enumerate()
                        .map(|(k, t)| (run.values[0][k] - exact(*t)).abs())
                        .fold(0.0f64, f64::max);
                    res.push((err, run.stats.steps, run.report.light_restarts));
                }
                println!(
                    "tau {tau}, n_z {n_z}, rtol {rtol:.0e}: restarts: error {:.2e} ({} steps); \
                     default: {:.2e} ({} steps, {} light); light restarts on: {:.2e} ({} \
                     steps, {} light)",
                    res[0].0, res[0].1, res[1].0, res[1].1, res[1].2, res[2].0, res[2].1, res[2].2
                );
                // the default restarts at every changing tick: as accurate
                // as forced restarts
                assert_eq!(res[1].2, 0, "no light restart by default");
                assert!(
                    res[1].0 <= res[0].0 * (1.0 + 1e-9) + 1e-15,
                    "{} vs {}",
                    res[1].0,
                    res[0].0
                );
            }
        }
    }
}

/// A long run of a pure integrator driven by a slowly ramping held
/// command (a distance or a state of charge from a sampled current
/// command): x' = u_k, u_k = k·delta, 10 000 ticks, against the exact
/// sample-and-hold answer. The kink error of a light restart has the same
/// sign at every tick here, so it accumulates to about half a tick times
/// the command's whole change (from the review: 57 tolerance units at rtol
/// 1e-6 when light restarts were the default). The default stays within
/// one tolerance unit, as forced restarts do.
#[test]
fn light_restarts_on_a_long_ramp() {
    let period = 0.01;
    for (t_end, delta, rtol) in [(100.0, 1e-4, 1e-6), (100.0, 1e-4, 1e-4), (100.0, 1e-6, 1e-6)] {
        let model = Hand {
            layout: layout(1, 0, 0, 1, 0, 0, 2),
            f: Box::new(|i, out| out[0] = i.d[0]),
            jvp: Box::new(|_, _, out| out[0] = 0.0),
            roots: Box::new(|_, _| {}),
            vars: Box::new(|i, out| {
                out[0] = i.y[0];
                out[1] = i.d[0];
            }),
            when: Box::new(|_, _, _| {}),
            modes: None,
            y0: vec![0.0],
            d0: vec![0.0],
        };
        let mut info = RunInfo::bare(1, 2, vec![]);
        info.var_sources = vec![VarSource::Y(0), VarSource::D(0)];
        info.blocks = vec![BlockInfo {
            name: "'Controller'".into(),
            chains: vec![],
            inputs: vec![],
            outputs: vec![0],
            period,
        }];
        let opts = SolverOptions { rtol, atol: rtol, ..Default::default() };
        let grid = OutputGrid { t0: 0.0, t_end, dt: 1.0 };
        // x(t) = sum over whole periods of u_k T, plus the part period
        let exact = |t: f64| {
            let n = (t / period + 1e-9).floor() as u64;
            let full: f64 = (0..n).map(|k| k as f64 * delta * period).sum();
            full + n as f64 * delta * (t - n as f64 * period)
        };
        let mut res = vec![];
        for how in [Ticks::Restart, Ticks::Default, Ticks::Light] {
            let mut blocks: Vec<Box<dyn DiscreteBlock>> = vec![Box::new(SampledBy {
                period,
                law: Box::new(move |t, _, o| o[0] = (t / period).round() * delta),
            })];
            let run = run_ticks(&model, &info, &opts, grid, &mut blocks, how);
            let (mut err, mut rel) = (0.0f64, 0.0f64);
            for (k, t) in run.times.iter().enumerate() {
                let x = exact(*t);
                err = err.max((run.values[0][k] - x).abs());
                rel = rel.max((run.values[0][k] - x).abs() / (rtol * x.abs() + rtol));
            }
            res.push((err, rel, run.stats.steps, run.report.light_restarts));
        }
        println!(
            "ramp {delta:.0e}/tick, rtol {rtol:.0e}, x(T) = {:.3}: restarts: error {:.2e} ({:.2} \
             tol, {} steps); default: {:.2e} ({:.2} tol, {} steps, {} light); light restarts \
             on: {:.2e} ({:.2} tol, {} steps, {} light)",
            exact(t_end),
            res[0].0,
            res[0].1,
            res[0].2,
            res[1].0,
            res[1].1,
            res[1].2,
            res[1].3,
            res[2].0,
            res[2].1,
            res[2].2,
            res[2].3
        );
        // the global error of forced restarts is within one tolerance unit;
        // the default's must be too
        assert!(res[1].1 <= 1.0, "the default: {:.1} tolerance units", res[1].1);
    }
}

/// A tick whose outputs reach nothing the integrator integrates (a value
/// only shown as a channel) goes on without a restart, by default: the
/// solution is exactly the one without the tick.
#[test]
fn a_tick_that_reaches_nothing_integrated_needs_no_restart() {
    // x' = -x; the block's output d0 is only a channel
    let model = Hand {
        layout: layout(1, 0, 0, 1, 0, 0, 2),
        f: Box::new(|i, out| out[0] = -i.y[0]),
        jvp: Box::new(|_, v, out| out[0] = -v[0]),
        roots: Box::new(|_, _| {}),
        vars: Box::new(|i, out| {
            out[0] = i.y[0];
            out[1] = i.d[0];
        }),
        when: Box::new(|_, _, _| {}),
        modes: None,
        y0: vec![1.0],
        d0: vec![0.0],
    };
    let mut info = RunInfo::bare(1, 2, vec![]);
    info.var_sources = vec![VarSource::Y(0), VarSource::D(0)];
    info.blocks = vec![block(vec![], vec![0], 0.01)];
    info.dynamic_discretes = vec![false];
    let opts = SolverOptions { rtol: 1e-8, atol: 1e-10, ..Default::default() };
    let grid = OutputGrid { t0: 0.0, t_end: 2.0, dt: 0.1 };
    let mut blocks: Vec<Box<dyn DiscreteBlock>> =
        vec![Box::new(Sampled { period: 0.01, offset: 0.0, law: |t, _, o| o[0] = t })];
    let run = simulate(&model, &info, &opts, grid, &mut blocks).unwrap();
    println!(
        "{} changing ticks, {} without a restart, {} restarts, {} steps",
        run.report.block_changes, run.report.light_restarts, run.stats.restarts, run.stats.steps
    );
    // (the first tick, at the start, changes nothing: its output is 0)
    assert_eq!(run.report.block_changes, 200);
    assert_eq!(run.report.light_restarts, 200);
    assert_eq!(run.stats.restarts, 0);
    for (k, t) in run.times.iter().enumerate() {
        assert!((run.values[0][k] - (-t).exp()).abs() < 1e-7, "t = {t}");
    }
    // the channel shows the held output: its last tick at or before t
    assert!((run.values[1].last().unwrap() - 2.0).abs() < 1e-12);
    assert!(run.stats.steps < 300, "{} steps", run.stats.steps);
}
