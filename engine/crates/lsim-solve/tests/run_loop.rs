//! The run loop's event handling on hand-written models with exact
//! answers: `when` conditions made true by a sample tick, by the start, by
//! another `when` through an iteration variable; mode changes a clock
//! schedules; a tick just before the end; the cost of a restart; time
//! events located exactly; the momentum kept at a change of a rigid
//! coupling.

use lsim_ir::prepared::Direction;
use lsim_ir::runtime::{DiscreteBlock, EvalInput, Layout, ModelFunctions};
use lsim_solve::{
    Backend, BlockInfo, EventKind, ModeInfo, OutputGrid, RunInfo, SimResult, SolverOptions,
    VarSource, simulate,
};

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

/// A tick whose output change is a rounding error goes on with the
/// integration's history (no restart), and the run is the one without the
/// change to within the tolerance.
#[test]
fn a_slight_change_at_a_tick_needs_no_restart() {
    let (model, mut info) = held_dae();
    info.blocks = vec![block(vec![0], vec![0], 0.01)];
    let opts = SolverOptions { rtol: 1e-8, atol: 1e-10, ..Default::default() };
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
