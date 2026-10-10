//! CVODE under `Method::Auto` switches from Adams to BDF when Adams
//! struggles (many convergence failures on the steps it got through). The
//! review of the ninth round found the switch made inside `step`, before
//! the step that asked for it was returned: the step just taken lost its
//! memory, and with it its dense output (`dense_output` and `interpolate`
//! gave the state at its end, constant over the whole step), which the
//! mixed conditions' check, the output samples, the block ticks and the
//! books at output times read. The switch is now made before the next step
//! (or at a restart, if one comes first). The first two tests are the
//! review's.

use lsim_ir::expr::{Builtin, Expr};
use lsim_ir::runtime::{DiscreteBlock, EvalInput, Layout, ModelFunctions};
use lsim_solve::sundials::Sundials;
use lsim_solve::{
    BlockInfo, DenseOutput, EnergyInfo, EnergyPart, Integrator, Method, OutputGrid, RunInfo,
    SolverOptions, Step, VarSource,
};
use std::sync::{Arc, Mutex};

/// x' = -k(t) (x - sin t), k rising from 1 to 1e5 around t = 2: non-stiff
/// at the start (Auto picks Adams), stiff later (Adams' fixed-point
/// iteration fails to converge, again and again).
struct Stiffening {
    layout: Layout,
}

fn k(t: f64) -> f64 {
    1.0 + 5e4 * (1.0 + (10.0 * (t - 2.0)).tanh())
}

impl ModelFunctions for Stiffening {
    fn layout(&self) -> &Layout {
        &self.layout
    }
    fn residual(&self, inp: &EvalInput<'_>, _: &mut [f64], out: &mut [f64]) {
        out[0] = -k(inp.t) * (inp.y[0] - inp.t.sin());
    }
    fn jvp(&self, inp: &EvalInput<'_>, v: &[f64], _: &mut [f64], out: &mut [f64]) {
        out[0] = -k(inp.t) * v[0];
    }
    fn roots(&self, _: &EvalInput<'_>, _: &mut [f64], _: &mut [f64]) {}
    fn vars(&self, inp: &EvalInput<'_>, _: &mut [f64], out: &mut [f64]) {
        out[0] = inp.y[0];
        // (a sampled block's held reading of x, when there is one)
        if let Some(v) = out.get_mut(1) {
            *v = inp.d[0];
        }
    }
    fn when(&self, _: &EvalInput<'_>, _: &[f64], _: &mut [f64], _: &mut [f64]) {}
    fn start(&self, _: &[f64], y0: &mut [f64], _: &mut [f64]) {
        y0[0] = 1.0;
    }
}

fn model() -> Stiffening {
    let layout = Layout {
        n_x: 1,
        n_z: 0,
        n_p: 0,
        n_d: 0,
        n_u: 0,
        n_roots: 0,
        n_whens: 0,
        n_vars: 1,
        n_work: 0,
    };
    Stiffening { layout }
}

fn info() -> RunInfo {
    let mut info = RunInfo::bare(1, 1, vec![]);
    info.var_sources = vec![VarSource::Y(0)];
    info
}

const T_END: f64 = 4.0;

fn integrator<'m>(model: &'m Stiffening, info: &RunInfo) -> Sundials<'m> {
    let opts = SolverOptions {
        rtol: 1e-8,
        atol: 1e-8,
        method: Method::Auto,
        energy_books: false,
        ..Default::default()
    };
    let grid = OutputGrid { t0: 0.0, t_end: T_END, dt: T_END };
    let integ =
        Sundials::new(model, info, &opts, grid, &[1.0], vec![], vec![], None).expect("sets up");
    assert_eq!(integ.method(), "Adams", "Auto starts on Adams here");
    integ
}

/// The dense output over the step `[t, t1]` starts at the state `x` there
/// (up to the rounding of the step's start, t_n − h, as the integrator
/// places it) and ends at the state `x1` at its end.
fn starts_and_ends(dense: &DenseOutput, (t, x): (f64, f64), (t1, x1): (f64, f64)) {
    let a = dense.coefficients(0);
    let size: f64 = a.iter().map(|a| a.abs()).sum();
    let slope = if a.len() > 1 { (a[1] / dense.scales[0]).abs() } else { 0.0 };
    let tol = 64.0 * f64::EPSILON * size + 4.0 * f64::EPSILON * t.abs() * slope;
    assert!(
        (dense.at(0, t) - x).abs() <= tol,
        "the dense output over [{t}, {t1}] (degree {}) starts at {}, not at the state {x} there",
        dense.degree(),
        dense.at(0, t)
    );
    assert_eq!(dense.at(0, t1), x1, "[{t}, {t1}]");
}

/// Every step's dense output, the one before the switch included, starts
/// at the state at the step's start: the review found the step before the
/// switch given the constant x(t1).
#[test]
fn review6_the_step_before_an_adams_to_bdf_switch_has_a_constant_dense_output() {
    let (model, info) = (model(), info());
    let mut integ = integrator(&model, &info);
    let mut dense = DenseOutput::default();
    let (mut t, mut x) = (0.0, 1.0);
    let mut last_degree = 0;
    let mut switches = 0;
    loop {
        let before = integ.method();
        let st = integ.step(T_END).expect("steps");
        let t1 = st.time();
        let x1 = integ.y()[0];
        if integ.method() != before {
            // the switch the step before asked for, made before this one:
            // that step kept its polynomial (checked when it was taken)
            switches += 1;
            println!(
                "switch '{}' before the step [{t}, {t1}]; the step before it had a dense output \
                 of degree {last_degree}",
                integ.method()
            );
            assert!(last_degree >= 1);
        }
        assert!(integ.dense_output(t, &[0], &mut dense).expect("dense output"));
        starts_and_ends(&dense, (t, x), (t1, x1));
        // the polynomial is the integrator's own interpolant
        let mut mid = [0.0];
        integ.interpolate(0.5 * (t + t1), &mut mid).unwrap();
        assert!((dense.at(0, 0.5 * (t + t1)) - mid[0]).abs() <= 64.0 * f64::EPSILON);
        last_degree = dense.degree();
        t = t1;
        x = x1;
        if matches!(st, Step::Stopped(_)) || t >= T_END {
            break;
        }
    }
    println!("methods: {}", integ.method());
    assert_eq!(switches, 1, "{}", integ.method());
}

/// The same switch in a whole run: output samples that fall inside the
/// step before the switch read the step's interpolant (the review found
/// them the state at the step's end, 6.4e-5 off at t = 1.8458). Compared
/// with a BDF run at a tighter tolerance.
#[test]
fn review6_outputs_inside_the_step_before_the_switch_are_the_steps_end_value() {
    let (model, info) = (model(), info());
    let grid = OutputGrid { t0: 0.0, t_end: T_END, dt: 1e-5 };
    let run = |method, tol| {
        let opts = SolverOptions {
            rtol: tol,
            atol: tol,
            method,
            energy_books: false,
            ..Default::default()
        };
        lsim_solve::simulate(&model, &info, &opts, grid, &mut []).expect("runs")
    };
    let auto = run(Method::Auto, 1e-8);
    let reference = run(Method::Bdf, 1e-12);
    println!("auto: {}", auto.report.method);
    assert!(auto.report.method.contains("then BDF"), "{}", auto.report.method);
    let (mut worst, mut at) = (0.0f64, 0.0);
    for (k, (a, r)) in auto.values[0].iter().zip(&reference.values[0]).enumerate() {
        let e = (a - r).abs();
        if e > worst {
            (worst, at) = (e, k as f64 * 1e-5);
        }
    }
    println!("largest output error {worst:.3e} at t = {at:.6}");
    assert!(worst < 1e-6, "an output {worst:.3e} off at t = {at:.6} (rtol 1e-8)");
}

/// A restart right after the step that asked for the switch (an event at
/// its end) makes the switch there: the new memory is BDF's, and its
/// first step starts at the restart's state.
#[test]
fn a_restart_after_the_struggling_step_switches_there() {
    let (model, info) = (model(), info());
    // how many steps come before the switch
    let mut integ = integrator(&model, &info);
    let mut n = 0;
    loop {
        let before = integ.method();
        let st = integ.step(T_END).expect("steps");
        if integ.method() != before {
            break;
        }
        n += 1;
        assert!(!matches!(st, Step::Stopped(_)), "no switch");
    }
    // the same steps again, then a restart from the state there
    let mut integ = integrator(&model, &info);
    let mut t = 0.0;
    for _ in 0..n {
        t = integ.step(T_END).expect("steps").time();
    }
    assert_eq!(integ.method(), "Adams");
    let y = integ.y().to_vec();
    integ.restart(t, &y).expect("restarts");
    assert_eq!(integ.method(), format!("Adams, then BDF from t = {t:.6} s"));
    let notes = integ.setup_notes();
    assert!(
        notes.iter().any(|n| n.contains("switched from Adams to BDF")
            && n.contains("repeated convergence failures")),
        "{notes:?}"
    );
    let mut dense = DenseOutput::default();
    assert!(integ.dense_output(t, &[0], &mut dense).unwrap());
    assert_eq!((dense.degree(), dense.at(0, t)), (0, y[0]), "the state, before a step");
    let st = integ.step(T_END).expect("steps");
    let (t1, x1) = (st.time(), integ.y()[0]);
    assert!(integ.dense_output(t, &[0], &mut dense).unwrap());
    starts_and_ends(&dense, (t, y[0]), (t1, x1));
    // no second switch
    assert_eq!(integ.method(), format!("Adams, then BDF from t = {t:.6} s"));
}

/// The energy integrals carry over the switch, and the books at output
/// times inside the step before it read that step's integrals: a part
/// taking in sin(t) W has taken in 1 − cos(t) J at t.
#[test]
fn the_integrals_carry_over_the_switch() {
    let model = model();
    let mut info = info();
    info.energy = Some(Arc::new(EnergyInfo {
        parts: vec![EnergyPart {
            path: "k".into(),
            name: "'k'".into(),
            power: Expr::Call(Builtin::Sin, vec![Expr::Time]),
            loss: None,
            stored: None,
        }],
    }));
    let opts = SolverOptions { rtol: 1e-8, atol: 1e-8, method: Method::Auto, ..Default::default() };
    let grid = OutputGrid { t0: 0.0, t_end: T_END, dt: 1e-5 };
    let run = lsim_solve::simulate(&model, &info, &opts, grid, &mut []).expect("runs");
    assert!(run.report.method.contains("then BDF"), "{}", run.report.method);
    let books = run.energy.as_ref().expect("books");
    let e = books.parts[0].energy_in;
    let exact = 1.0 - T_END.cos();
    println!("{}: {e} J against {exact}", run.report.method);
    assert!((e - exact).abs() < 1e-6 * exact.abs(), "{e} against {exact}");
    let (mut worst, mut at) = (0.0f64, 0.0);
    for (t, e) in run.times.iter().zip(&books.parts[0].energy_in_t) {
        let d = (e - (1.0 - t.cos())).abs();
        if d > worst {
            (worst, at) = (d, *t);
        }
    }
    println!("books at output times: largest error {worst:.3e} J at t = {at:.6}");
    assert!(worst < 1e-6, "{worst:.3e} J off at t = {at:.6}");
}

/// A sampled block reading x every 10 µs (its output reaching nothing the
/// integrator integrates: its ticks do not restart it, and the switch
/// comes as without it) reads the steps' interpolant at its ticks, the
/// step before the switch included.
#[test]
fn block_ticks_inside_the_step_before_the_switch_read_its_interpolant() {
    type Readings = Arc<Mutex<Vec<(f64, f64)>>>;
    struct Reader(Readings);
    impl DiscreteBlock for Reader {
        fn name(&self) -> &str {
            "'Reader'"
        }
        fn period(&self) -> f64 {
            1e-5
        }
        fn init(&mut self, t: f64, i: &[f64], o: &mut [f64]) -> Result<(), String> {
            self.tick(t, i, o)
        }
        fn tick(&mut self, t: f64, i: &[f64], o: &mut [f64]) -> Result<(), String> {
            self.0.lock().unwrap().push((t, i[0]));
            o[0] = i[0];
            Ok(())
        }
    }
    let layout = Layout {
        n_x: 1,
        n_z: 0,
        n_p: 0,
        n_d: 1,
        n_u: 0,
        n_roots: 0,
        n_whens: 0,
        n_vars: 2,
        n_work: 0,
    };
    let model = Stiffening { layout };
    let mut info = RunInfo::bare(1, 2, vec![]);
    info.var_sources = vec![VarSource::Y(0), VarSource::D(0)];
    info.dynamic_discretes = vec![false];
    let opts = |method, tol| SolverOptions {
        rtol: tol,
        atol: tol,
        method,
        energy_books: false,
        ..Default::default()
    };
    // the readings, on Auto
    let mut sampled = info.clone();
    sampled.blocks = vec![BlockInfo {
        name: "'Reader'".into(),
        inputs: vec![0],
        outputs: vec![0],
        period: 1e-5,
        chains: vec![None],
    }];
    let readings: Readings = Arc::new(Mutex::new(vec![]));
    let mut blocks: Vec<Box<dyn DiscreteBlock>> = vec![Box::new(Reader(readings.clone()))];
    let grid = OutputGrid { t0: 0.0, t_end: T_END, dt: 0.1 };
    let auto = lsim_solve::simulate(&model, &sampled, &opts(Method::Auto, 1e-8), grid, &mut blocks)
        .expect("runs");
    println!("auto: {}, {} restarts", auto.report.method, auto.stats.restarts);
    assert!(auto.report.method.contains("then BDF"), "{}", auto.report.method);
    // x on BDF at a tight tolerance, every 10 µs (read between those
    // points by linear interpolation: within 1e-9 here)
    let fine = OutputGrid { t0: 0.0, t_end: T_END, dt: 1e-5 };
    let reference =
        lsim_solve::simulate(&model, &info, &opts(Method::Bdf, 1e-12), fine, &mut []).unwrap();
    let x = |t: f64| {
        let k = ((t / 1e-5) as usize).min(reference.times.len() - 2);
        let (t0, t1) = (reference.times[k], reference.times[k + 1]);
        let (a, b) = (reference.values[0][k], reference.values[0][k + 1]);
        a + (b - a) * (t - t0) / (t1 - t0)
    };
    let readings = readings.lock().unwrap();
    assert!(readings.len() > 390_000, "{} ticks", readings.len());
    let (mut worst, mut at) = (0.0f64, 0.0);
    for &(t, read) in readings.iter() {
        let e = (read - x(t)).abs();
        if e > worst {
            (worst, at) = (e, t);
        }
    }
    println!("largest error of a reading {worst:.3e} at t = {at:.6}");
    assert!(worst < 1e-6, "a reading {worst:.3e} off at t = {at:.6} (rtol 1e-8)");
}
