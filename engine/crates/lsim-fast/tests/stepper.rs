//! The fast-mode stepper against exact answers, on hand-written models
//! (test doubles of the compiled inverse model): convergence order, stiff
//! components, iteration variables, a wrong Jacobian, limit flags, events
//! and a sampled block.

use lsim_fast::{
    BoundBlock, FastError, FastOptions, FastProblem, FastSolver, FixedMethod, LimitSite,
    RosenbrockW, Trace, WhenInfo, run,
};
use lsim_ir::prepared::Direction;
use lsim_ir::runtime::{DiscreteBlock, EvalInput, Layout, ModelFunctions};

type Fun = Box<dyn Fn(&EvalInput<'_>, &mut [f64]) + Send + Sync>;
type WhenFun = Box<dyn Fn(&EvalInput<'_>, &[f64], &mut [f64]) + Send + Sync>;

/// A model given by closures; the Jacobian-vector product by central
/// differences, optionally scaled (a deliberately wrong Jacobian).
struct Doubled {
    layout: Layout,
    f: Fun,
    vars: Fun,
    roots: Fun,
    when: Option<WhenFun>,
    y0: Vec<f64>,
    d0: Vec<f64>,
    jac_scale: f64,
}

impl Doubled {
    fn new(n_x: usize, n_z: usize, n_u: usize, n_vars: usize, f: Fun, vars: Fun) -> Self {
        Doubled {
            layout: Layout { n_x, n_z, n_u, n_vars, n_work: 1, ..Default::default() },
            f,
            vars,
            roots: Box::new(|_, _| {}),
            when: None,
            y0: vec![0.0; n_x + n_z],
            d0: vec![],
            jac_scale: 1.0,
        }
    }
}

impl ModelFunctions for Doubled {
    fn layout(&self) -> &Layout {
        &self.layout
    }
    fn residual(&self, inp: &EvalInput<'_>, _w: &mut [f64], out: &mut [f64]) {
        (self.f)(inp, out)
    }
    fn jvp(&self, inp: &EvalInput<'_>, v: &[f64], _w: &mut [f64], out: &mut [f64]) {
        let n = inp.y.len();
        let scale = inp.y.iter().fold(1.0f64, |m, x| m.max(x.abs()));
        let eps = 1e-6 * scale;
        let mut yp = inp.y.to_vec();
        let mut ym = inp.y.to_vec();
        for i in 0..n {
            yp[i] += eps * v[i];
            ym[i] -= eps * v[i];
        }
        let mut fp = vec![0.0; n];
        let mut fm = vec![0.0; n];
        (self.f)(&EvalInput { y: &yp, ..*inp }, &mut fp);
        (self.f)(&EvalInput { y: &ym, ..*inp }, &mut fm);
        for i in 0..n {
            out[i] = self.jac_scale * (fp[i] - fm[i]) / (2.0 * eps);
        }
    }
    fn roots(&self, inp: &EvalInput<'_>, _w: &mut [f64], out: &mut [f64]) {
        (self.roots)(inp, out)
    }
    fn vars(&self, inp: &EvalInput<'_>, _w: &mut [f64], out: &mut [f64]) {
        (self.vars)(inp, out)
    }
    fn when(&self, inp: &EvalInput<'_>, fired: &[f64], _w: &mut [f64], d_out: &mut [f64]) {
        if let Some(w) = &self.when {
            w(inp, fired, d_out)
        }
    }
    fn start(&self, _p: &[f64], y0: &mut [f64], d0: &mut [f64]) {
        y0.copy_from_slice(&self.y0);
        d0.copy_from_slice(&self.d0);
    }
}

/// x' = -λ (x - v(t)) with v the prescribed input; vars = [x, v].
fn relaxation(lambda: f64) -> Doubled {
    let mut m = Doubled::new(
        1,
        0,
        2,
        2,
        Box::new(move |i, out| out[0] = -lambda * (i.y[0] - i.u[0])),
        Box::new(|i, out| {
            out[0] = i.y[0];
            out[1] = i.u[0];
        }),
    );
    m.y0 = vec![1.0];
    m
}

/// The exact answer of `relaxation` for v = c0 + c1 t, x(0) = 1.
fn relaxation_exact(lambda: f64, c0: f64, c1: f64, t: f64) -> f64 {
    let xp = |t: f64| (c0 + c1 * t) - c1 / lambda;
    xp(t) + (1.0 - xp(0.0)) * (-lambda * t).exp()
}

fn last(r: &lsim_fast::FastResult, j: usize) -> f64 {
    let n = r.names.len();
    r.values[(r.times.len() - 1) * n + j]
}

#[test]
fn ros34pw2_converges_at_third_order() {
    let lambda = 0.7;
    let model = relaxation(lambda);
    let trace = Trace::new(vec![0.0, 4.0], vec![2.0, 2.0 + 4.0 * 1.5]).unwrap();
    let exact = relaxation_exact(lambda, 2.0, 1.5, 4.0);
    let mut errs = vec![];
    for h in [0.5, 0.25, 0.125] {
        let o = FastOptions { step: h, ..Default::default() };
        let r = RosenbrockW.run(&model, &[], std::slice::from_ref(&trace), &o).unwrap();
        errs.push((last(&r, 0) - exact).abs());
    }
    let orders: Vec<f64> = errs.windows(2).map(|w| (w[0] / w[1]).log2()).collect();
    println!("errors {errs:?}, orders {orders:?}");
    for o in &orders {
        assert!((2.7..3.5).contains(o), "observed order {o}: {errs:?}");
    }
    // implicit Euler is first order
    let mut e1 = vec![];
    for h in [0.5, 0.25] {
        let o =
            FastOptions { step: h, method: FixedMethod::LinearImplicitEuler, ..Default::default() };
        let r = RosenbrockW.run(&model, &[], std::slice::from_ref(&trace), &o).unwrap();
        e1.push((last(&r, 0) - exact).abs());
    }
    let o1 = (e1[0] / e1[1]).log2();
    assert!((0.8..1.3).contains(&o1), "implicit Euler order {o1}");
}

#[test]
fn a_wrong_jacobian_keeps_the_order() {
    // a W-method: any matrix in place of the Jacobian keeps order 3
    let lambda = 0.7;
    let mut model = relaxation(lambda);
    model.jac_scale = 1.6;
    let trace = Trace::new(vec![0.0, 4.0], vec![2.0, 8.0]).unwrap();
    let exact = relaxation_exact(lambda, 2.0, 1.5, 4.0);
    let mut errs = vec![];
    for h in [0.25, 0.125, 0.0625] {
        let o = FastOptions { step: h, ..Default::default() };
        let r = RosenbrockW.run(&model, &[], std::slice::from_ref(&trace), &o).unwrap();
        errs.push((last(&r, 0) - exact).abs());
    }
    let orders: Vec<f64> = errs.windows(2).map(|w| (w[0] / w[1]).log2()).collect();
    println!("wrong Jacobian: errors {errs:?}, orders {orders:?}");
    for o in &orders {
        assert!((2.6..3.6).contains(o), "observed order {o}: {errs:?}");
    }
}

#[test]
fn a_stiff_component_is_damped_at_one_second_steps() {
    // an RC-like pole at -10 000 1/s stepped at 1 s: L-stability gives the
    // quasi-static answer, no ringing
    let lambda = 1e4;
    let model = relaxation(lambda);
    let trace = Trace::new(vec![0.0, 10.0, 20.0], vec![0.0, 50.0, 50.0]).unwrap();
    let r = RosenbrockW
        .run(&model, &[], std::slice::from_ref(&trace), &FastOptions::default())
        .unwrap();
    let x = r.channel("v[0]").unwrap();
    let v = r.channel("v[1]").unwrap();
    for k in 2..r.times.len() {
        // x = v - c1/λ on the ramp, x = v once it is flat (to 1e-6 relative)
        let lag = if r.times[k] <= 10.0 { 5.0 / lambda } else { 0.0 };
        assert!(
            (x[k] - (v[k] - lag)).abs() < 1e-3,
            "t = {}: x = {} v = {}",
            r.times[k],
            x[k],
            v[k]
        );
    }
    assert!(r.report.max_error.is_finite());
}

/// x' = -z, 0 = z + z³ - x (an iteration variable from a cubic); vars = [x, z, g].
fn cubic_dae() -> Doubled {
    let mut m = Doubled::new(
        1,
        1,
        2,
        3,
        Box::new(|i, out| {
            let (x, z) = (i.y[0], i.y[1]);
            out[0] = -z + 0.1 * i.u[0];
            out[1] = z + z * z * z - x;
        }),
        Box::new(|i, out| {
            let (x, z) = (i.y[0], i.y[1]);
            out[0] = x;
            out[1] = z;
            out[2] = z + z * z * z - x;
        }),
    );
    m.y0 = vec![2.0, 0.0];
    m
}

/// The reduced ODE x' = -z(x) + 0.1 u by RK4 with tiny steps.
fn cubic_reference(t_end: f64, u: impl Fn(f64) -> f64) -> f64 {
    let z_of = |x: f64| {
        let mut z = x.cbrt();
        for _ in 0..60 {
            let g = z + z * z * z - x;
            z -= g / (1.0 + 3.0 * z * z);
        }
        z
    };
    let f = |t: f64, x: f64| -z_of(x) + 0.1 * u(t);
    let n = 40_000;
    let h = t_end / n as f64;
    let mut x = 2.0;
    for k in 0..n {
        let t = k as f64 * h;
        let k1 = f(t, x);
        let k2 = f(t + 0.5 * h, x + 0.5 * h * k1);
        let k3 = f(t + 0.5 * h, x + 0.5 * h * k2);
        let k4 = f(t + h, x + h * k3);
        x += h / 6.0 * (k1 + 2.0 * k2 + 2.0 * k3 + k4);
    }
    x
}

#[test]
fn iteration_variables_are_solved_at_every_point() {
    let model = cubic_dae();
    let trace = Trace::new(vec![0.0, 4.0], vec![0.0, 8.0]).unwrap();
    let reference = cubic_reference(4.0, |t| 2.0 * t);
    let mut errs = vec![];
    for h in [0.5, 0.25, 0.125] {
        let o = FastOptions { step: h, ..Default::default() };
        let r = RosenbrockW.run(&model, &[], std::slice::from_ref(&trace), &o).unwrap();
        errs.push((last(&r, 0) - reference).abs());
        // every recorded point satisfies the algebraic equation
        let g = r.channel("v[2]").unwrap();
        for (k, gk) in g.iter().enumerate() {
            assert!(gk.abs() < 1e-9, "g = {gk} at t = {}", r.times[k]);
        }
        assert_eq!(r.report.n_z, 1);
    }
    let orders: Vec<f64> = errs.windows(2).map(|w| (w[0] / w[1]).log2()).collect();
    println!("DAE: errors {errs:?}, orders {orders:?}");
    for o in &orders {
        assert!((2.5..3.6).contains(o), "observed order {o}: {errs:?}");
    }
}

#[test]
fn limits_are_flagged_where_the_value_leaves_its_band() {
    // a purely algebraic model: x = v(t), band [-1, 2]
    let model = Doubled::new(
        0,
        0,
        2,
        3,
        Box::new(|_, _| {}),
        Box::new(|i, out| {
            out[0] = i.u[0];
            out[1] = -1.0;
            out[2] = 2.0;
        }),
    );
    let trace = Trace::new(vec![0.0, 4.0, 6.0, 10.0], vec![0.0, 4.0, 4.0, -4.0]).unwrap();
    let site = LimitSite {
        label: "'Motor': torque".into(),
        part: "motor".into(),
        demand: 0,
        lower: 1,
        upper: 2,
        unit: "N.m".into(),
    };
    let o = FastOptions { step: 1.0, ..Default::default() };
    let sites = [site];
    let traces = [trace];
    let mut p = FastProblem::new(&model, &[], &traces, &o);
    p.limits = &sites;
    let r = run(&p, &mut []).unwrap();
    assert_eq!(r.flags.len(), 2, "{:?}", r.flags);
    let up = &r.flags[0];
    assert!(
        up.upper && (up.t_start - 2.0).abs() < 1e-12 && (up.t_end - 7.0).abs() < 1e-12,
        "{up:?}"
    );
    assert!((up.worst_excess - 2.0).abs() < 1e-12);
    let down = &r.flags[1];
    assert!(!down.upper && (down.t_start - 8.5).abs() < 1e-12 && down.t_end == 10.0, "{down:?}");
    assert!((down.worst_excess - 3.0).abs() < 1e-12);
}

#[test]
fn a_zero_crossing_is_located_and_its_clause_runs() {
    // x' = 1 + d, x(0) = 0; when x >= 2.5 then d := 1  ->  x(4) = 2.5 + 2 * 1.5
    let mut m = Doubled::new(
        1,
        0,
        2,
        2,
        Box::new(|i, out| out[0] = 1.0 + i.d[0]),
        Box::new(|i, out| {
            out[0] = i.y[0];
            out[1] = i.d[0];
        }),
    );
    m.layout.n_d = 1;
    m.layout.n_roots = 1;
    m.layout.n_whens = 1;
    m.d0 = vec![0.0];
    m.roots = Box::new(|i, out| out[0] = i.y[0] - 2.5);
    m.when = Some(Box::new(|_, fired, d| {
        if fired[0] != 0.0 {
            d[0] = 1.0;
        }
    }));
    let traces = [Trace::new(vec![0.0, 4.0], vec![0.0, 0.0]).unwrap()];
    let o = FastOptions::default();
    let whens =
        [WhenInfo { crossing: 0, direction: Direction::Rising, label: "'Brake': engages".into() }];
    let mut p = FastProblem::new(&m, &[], &traces, &o);
    p.whens = &whens;
    let r = run(&p, &mut []).unwrap();
    assert_eq!(r.events.len(), 1, "{:?}", r.events);
    assert!((r.events[0].t - 2.5).abs() < 1e-9, "{:?}", r.events);
    assert!((last(&r, 0) - 5.5).abs() < 1e-9, "x(4) = {}", last(&r, 0));
    // the output interval [2, 3] holds both sides of the event
    let d = r.column(&r.max, 1);
    assert_eq!(d[3], 1.0);
    let dmin = r.column(&r.min, 1);
    assert_eq!(dmin[3], 0.0);
}

/// Samples its input and holds it.
struct Hold {
    ticks: usize,
}

impl DiscreteBlock for Hold {
    fn name(&self) -> &str {
        "hold"
    }
    fn period(&self) -> f64 {
        1.0
    }
    fn init(&mut self, _t0: f64, inputs: &[f64], outputs: &mut [f64]) -> Result<(), String> {
        outputs[0] = inputs[0];
        Ok(())
    }
    fn tick(&mut self, _t: f64, inputs: &[f64], outputs: &mut [f64]) -> Result<(), String> {
        self.ticks += 1;
        outputs[0] = inputs[0];
        Ok(())
    }
}

#[test]
fn a_sampled_block_ticks_and_holds() {
    // x' = d with d the held x: x doubles every second
    let mut m = Doubled::new(
        1,
        0,
        2,
        1,
        Box::new(|i, out| out[0] = i.d[0]),
        Box::new(|i, out| out[0] = i.y[0]),
    );
    m.layout.n_d = 1;
    m.d0 = vec![0.0];
    m.y0 = vec![1.0];
    let traces = [Trace::new(vec![0.0, 3.0], vec![0.0, 0.0]).unwrap()];
    let o = FastOptions { step: 0.5, ..Default::default() };
    let p = FastProblem::new(&m, &[], &traces, &o);
    let mut blocks =
        [BoundBlock { block: Box::new(Hold { ticks: 0 }), inputs: vec![0], outputs: vec![0] }];
    let r = run(&p, &mut blocks).unwrap();
    assert!((last(&r, 0) - 8.0).abs() < 1e-12, "x(3) = {}", last(&r, 0));
    assert_eq!(r.report.block_changes, 2, "{:?}", r.report);
}

#[test]
fn mismatched_inputs_are_refused() {
    let model = relaxation(1.0);
    let o = FastOptions::default();
    let err = RosenbrockW.run(&model, &[], &[], &o).unwrap_err();
    assert!(matches!(err, FastError::Inputs(_)), "{err}");
    let bad = Trace { t: vec![0.0, 0.0], value: vec![1.0, 1.0] };
    let err = RosenbrockW.run(&model, &[], &[bad], &o).unwrap_err();
    assert!(matches!(err, FastError::Trace(_)), "{err}");
}

#[test]
fn means_are_exact_for_cubics() {
    // vars = v(t)^3 on a linear trace: Simpson's rule is exact for cubics
    let model = Doubled::new(
        0,
        0,
        2,
        1,
        Box::new(|_, _| {}),
        Box::new(|i, out| out[0] = i.u[0] * i.u[0] * i.u[0]),
    );
    let traces = [Trace::new(vec![0.0, 4.0], vec![1.0, 9.0]).unwrap()];
    let o = FastOptions { step: 2.0, ..Default::default() };
    let r = RosenbrockW.run(&model, &[], &traces, &o).unwrap();
    // mean of (1 + 2t)^3 over [0, 2] = ((5^4 - 1^4) / 8) / 2
    let mean = r.column(&r.mean, 0);
    assert!((mean[1] - (625.0 - 1.0) / 16.0).abs() < 1e-9, "{mean:?}");
    assert_eq!(r.column(&r.max, 0)[1], 125.0);
}
