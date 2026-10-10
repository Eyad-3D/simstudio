//! The dense output's polynomial ([`Integrator::dense_output`]): on
//! CVODE (Adams and BDF) and IDA it is the integrator's own interpolant,
//! read on every step; Adams' order, and so its degree, stays at 7 or
//! below (the review of rounds 7 and 8: CVODE's default of 12 would take
//! a slow cosine to order 11).

use lsim_ir::runtime::{EvalInput, Layout, ModelFunctions};
use lsim_solve::sundials::Sundials;
use lsim_solve::{
    DenseOutput, Integrator, Method, OutputGrid, RunInfo, SolverOptions, Step, VarSource,
};

/// `x' = f(t, x, z)` (the first `n_x` outputs), `0 = g(t, x, z)` (the rest).
struct Dae {
    layout: Layout,
    f: fn(f64, &[f64], &mut [f64]),
    y0: Vec<f64>,
}

impl ModelFunctions for Dae {
    fn layout(&self) -> &Layout {
        &self.layout
    }
    fn residual(&self, inp: &EvalInput<'_>, _: &mut [f64], out: &mut [f64]) {
        (self.f)(inp.t, inp.y, out)
    }
    fn jvp(&self, inp: &EvalInput<'_>, v: &[f64], _: &mut [f64], out: &mut [f64]) {
        // central differences: these models are smooth and small
        let n = v.len();
        let h = 1e-6;
        let (mut a, mut b) = (inp.y.to_vec(), inp.y.to_vec());
        for i in 0..n {
            a[i] += h * v[i];
            b[i] -= h * v[i];
        }
        let (mut fa, mut fb) = (vec![0.0; n], vec![0.0; n]);
        (self.f)(inp.t, &a, &mut fa);
        (self.f)(inp.t, &b, &mut fb);
        for i in 0..n {
            out[i] = (fa[i] - fb[i]) / (2.0 * h);
        }
    }
    fn roots(&self, _: &EvalInput<'_>, _: &mut [f64], _: &mut [f64]) {}
    fn vars(&self, inp: &EvalInput<'_>, _: &mut [f64], out: &mut [f64]) {
        out.copy_from_slice(inp.y)
    }
    fn when(&self, _: &EvalInput<'_>, _: &[f64], _: &mut [f64], _: &mut [f64]) {}
    fn start(&self, _: &[f64], y0: &mut [f64], _: &mut [f64]) {
        y0.copy_from_slice(&self.y0)
    }
}

fn dae(n_x: usize, n_z: usize, f: fn(f64, &[f64], &mut [f64]), y0: Vec<f64>) -> Dae {
    let n = n_x + n_z;
    let layout =
        Layout { n_x, n_z, n_p: 0, n_d: 0, n_u: 0, n_roots: 0, n_whens: 0, n_vars: n, n_work: 0 };
    Dae { layout, f, y0 }
}

/// Steps `model` to `t_end` with `method` at `rtol`; on every step checks
/// that the dense output's polynomial gives the integrator's interpolant
/// (to a few ulps of its size) at points across the step. Returns the
/// largest degree seen.
fn check(model: &Dae, method: Method, rtol: f64, t_end: f64) -> usize {
    let n = model.layout.n_y();
    let mut info = RunInfo::bare(n, n, vec![]);
    info.var_sources = (0..n).map(VarSource::Y).collect();
    let opts =
        SolverOptions { rtol, atol: rtol, method, energy_books: false, ..Default::default() };
    let grid = OutputGrid { t0: 0.0, t_end, dt: t_end };
    let mut integ =
        Sundials::new(model, &info, &opts, grid, &model.y0, vec![], vec![], None).expect("sets up");
    let idx: Vec<usize> = (0..n).collect();
    let mut dense = DenseOutput::default();
    let (mut full, mut t, mut degree, mut steps) = (vec![0.0; n], 0.0, 0, 0);
    loop {
        let st = integ.step(t_end).expect("steps");
        let t1 = st.time();
        assert!(integ.dense_output(t, &idx, &mut dense).expect("its dense output"));
        degree = degree.max(dense.degree());
        for f in [0.0, 0.13, 0.5, 0.77, 1.0] {
            let tk = t + f * (t1 - t);
            integ.interpolate(tk, &mut full).unwrap();
            for (m, y) in full.iter().enumerate() {
                let p = dense.at(m, tk);
                let size: f64 = dense.coefficients(m).iter().map(|a| a.abs()).sum();
                assert!(
                    (p - y).abs() <= 64.0 * f64::EPSILON * size.max(y.abs()),
                    "{method:?} entry {m} at t = {tk} (step to {t1}): {p} against {y}"
                );
            }
        }
        steps += 1;
        t = t1;
        if matches!(st, Step::Stopped(_)) || t >= t_end {
            break;
        }
    }
    assert!(steps > 20, "{steps} steps");
    degree
}

#[test]
fn the_dense_output_polynomial_is_the_integrators_interpolant() {
    // a slow cosine (Adams' highest orders), an oscillator on BDF, a DAE
    // on IDA
    let cosine = dae(1, 0, |t, _, d| d[0] = (0.001 * t).cos(), vec![0.0]);
    let oscillator = dae(
        2,
        0,
        |_, y, d| {
            d[0] = y[1];
            d[1] = -y[0];
        },
        vec![1.0, 0.0],
    );
    let lag = dae(
        1,
        1,
        |t, y, d| {
            d[0] = -y[0] + y[1];
            d[1] = y[1] - (0.5 * t).sin();
        },
        vec![0.0, 0.0],
    );
    let adams = check(&cosine, Method::Adams, 1e-12, 10_000.0);
    println!("Adams on a slow cosine: degree up to {adams}");
    assert_eq!(adams, 7, "Adams reaches its cap, and no more");
    for rtol in [1e-6, 1e-10] {
        let bdf = check(&oscillator, Method::Bdf, rtol, 50.0);
        let ida = check(&lag, Method::Auto, rtol, 50.0);
        println!("rtol {rtol:.0e}: BDF degree up to {bdf}, IDA up to {ida}");
        assert!(bdf <= 5 && ida <= 5);
    }
}
