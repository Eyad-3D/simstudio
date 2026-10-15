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
/// (to a few ulps of its size) at points across the step. With `restarts`,
/// restarts it every seventh step from a state moved by 0.1 %: before the
/// next step its dense output is that state, constant, and over the next
/// step it starts there. Returns the largest degree seen.
fn check(model: &Dae, method: Method, rtol: f64, t_end: f64, restarts: bool) -> usize {
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
    let close = |p: f64, y: f64, a: &[f64]| {
        let size: f64 = a.iter().map(|a| a.abs()).sum();
        (p - y).abs() <= 64.0 * f64::EPSILON * size.max(y.abs())
    };
    let mut restarted: Option<Vec<f64>> = None;
    loop {
        let st = integ.step(t_end).expect("steps");
        let t1 = st.time();
        assert!(integ.dense_output(t, &idx, &mut dense).expect("its dense output"));
        degree = degree.max(dense.degree());
        if let Some(y) = restarted.take() {
            // the first step after a restart starts at its state (up to the
            // rounding of the step's start, t_n − h, as the integrator
            // places it: a few ulps of t times the slope)
            for (m, y) in y.iter().enumerate() {
                let p = dense.at(m, t);
                let a = dense.coefficients(m);
                let slope = if a.len() > 1 { (a[1] / dense.scales[0]).abs() } else { 0.0 };
                let off = (p - y).abs() - 4.0 * f64::EPSILON * t.abs() * slope;
                assert!(off <= 0.0 || close(p, *y, a), "{method:?} entry {m}: {p} vs {y}");
            }
        }
        for f in [0.0, 0.13, 0.5, 0.77, 1.0] {
            let tk = t + f * (t1 - t);
            integ.interpolate(tk, &mut full).unwrap();
            for (m, y) in full.iter().enumerate() {
                let p = dense.at(m, tk);
                let a = dense.coefficients(m);
                let size: f64 = a.iter().map(|a| a.abs()).sum();
                // at the step's start the integrator interpolates at its own
                // start, t_n − h as it rounds it (it clamps t there), an ulp
                // or two from the end of the previous step: a few ulps of t
                // times the slope, as after a restart (above)
                let slope =
                    if f == 0.0 && a.len() > 1 { (a[1] / dense.scales[0]).abs() } else { 0.0 };
                let off = (p - y).abs() - 4.0 * f64::EPSILON * tk.abs() * slope;
                assert!(
                    off <= 64.0 * f64::EPSILON * size.max(y.abs()),
                    "{method:?} entry {m} at t = {tk} (step to {t1}): {p} against {y}"
                );
            }
        }
        steps += 1;
        t = t1;
        if matches!(st, Step::Stopped(_)) || t >= t_end {
            break;
        }
        if restarts && steps % 7 == 0 {
            let mut y = integ.y().to_vec();
            for v in &mut y[..model.layout.n_x] {
                *v *= 1.001;
            }
            integ.restart(t, &y).expect("restarts");
            // (IDA makes the iteration variables consistent: its state)
            let y = integ.y().to_vec();
            assert!(integ.dense_output(t, &idx, &mut dense).expect("its dense output"));
            assert_eq!(dense.degree(), 0, "{method:?}: before a step, the state");
            for (m, y) in y.iter().enumerate() {
                assert_eq!(dense.at(m, t), *y, "{method:?} entry {m}");
            }
            restarted = Some(y);
        }
    }
    assert!(steps > 20, "{steps} steps");
    degree
}

#[test]
fn the_dense_output_polynomial_is_the_integrators_interpolant() {
    // a slow cosine (Adams' highest orders), an oscillator on BDF, a DAE
    // on IDA
    let cosine = dae(1, 0, |t, _, d| d[0] = lsim_ir::math::cos(0.001 * t), vec![0.0]);
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
            d[1] = y[1] - lsim_ir::math::sin(0.5 * t);
        },
        vec![0.0, 0.0],
    );
    let adams = check(&cosine, Method::Adams, 1e-12, 10_000.0, false);
    println!("Adams on a slow cosine: degree up to {adams}");
    assert_eq!(adams, 7, "Adams reaches its cap, and no more");
    for rtol in [1e-6, 1e-10] {
        let bdf = check(&oscillator, Method::Bdf, rtol, 50.0, false);
        let ida = check(&lag, Method::Auto, rtol, 50.0, false);
        println!("rtol {rtol:.0e}: BDF degree up to {bdf}, IDA up to {ida}");
        assert!(bdf <= 5 && ida <= 5);
    }
}

/// Across restarts (an event's new state): before the next step the dense
/// output is the restart's state, constant; over that step it is the new
/// memory's interpolant, starting at that state. On Adams, BDF (with
/// `Method::Auto` free to switch at a restart) and IDA.
#[test]
fn the_dense_output_across_restarts() {
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
            d[1] = y[1] - lsim_ir::math::sin(0.5 * t);
        },
        vec![0.0, 0.0],
    );
    for method in [Method::Adams, Method::Bdf, Method::Auto] {
        check(&oscillator, method, 1e-8, 50.0, true);
    }
    check(&lag, Method::Auto, 1e-8, 50.0, true);
}

/// A sampled block's ticks inside a step read together
/// ([`Integrator::interpolate_select_many`]): bit for bit the values read
/// one by one ([`Integrator::interpolate_select`]), on Adams up to its
/// highest order, BDF and IDA, at times across the step (its ends, just
/// outside them, batches longer than one lane chunk), right after a
/// restart and before the first step.
#[test]
fn ticks_read_together_are_the_ticks_read_one_by_one() {
    let cosine = dae(1, 0, |t, _, d| d[0] = lsim_ir::math::cos(0.001 * t), vec![0.0]);
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
            d[1] = y[1] - lsim_ir::math::sin(0.5 * t);
        },
        vec![0.0, 0.0],
    );
    let cases: [(&Dae, Method, f64, f64); 4] = [
        (&cosine, Method::Adams, 1e-12, 10_000.0),
        (&oscillator, Method::Bdf, 1e-8, 50.0),
        (&oscillator, Method::Adams, 1e-6, 50.0),
        (&lag, Method::Auto, 1e-8, 50.0),
    ];
    for (model, method, rtol, t_end) in cases {
        let n = model.layout.n_y();
        let mut info = RunInfo::bare(n, n, vec![]);
        info.var_sources = (0..n).map(VarSource::Y).collect();
        let opts =
            SolverOptions { rtol, atol: rtol, method, energy_books: false, ..Default::default() };
        let grid = OutputGrid { t0: 0.0, t_end, dt: t_end };
        let mut integ = Sundials::new(model, &info, &opts, grid, &model.y0, vec![], vec![], None)
            .expect("sets up");
        // every entry, one entry, an entry twice and out of order
        let sets: Vec<Vec<usize>> =
            vec![(0..n).collect(), vec![n - 1], (0..n).rev().chain(0..1).collect()];
        let (mut t, mut steps, mut compared) = (0.0, 0, 0usize);
        let mut compare = |integ: &mut Sundials<'_>, ts: &[f64]| {
            for idx in &sets {
                let m = idx.len();
                let mut many = vec![f64::NAN; ts.len() * m];
                integ.interpolate_select_many(ts, idx, &mut many).expect("reads together");
                let mut one = vec![0.0; m];
                for (i, &tk) in ts.iter().enumerate() {
                    integ.interpolate_select(tk, idx, &mut one).expect("reads one");
                    for k in 0..m {
                        assert_eq!(
                            many[i * m + k].to_bits(),
                            one[k].to_bits(),
                            "{method:?} entry {} at t = {tk}: {} against {}",
                            idx[k],
                            many[i * m + k],
                            one[k]
                        );
                        compared += 1;
                    }
                }
            }
        };
        // before the first step: the state
        compare(&mut integ, &[0.0, 0.0]);
        loop {
            let st = integ.step(t_end).expect("steps");
            let t1 = st.time();
            let h = t1 - t;
            // 1, 7, 8, 9 and 19 times across the step, its ends, a few ulps
            // outside it (clamped as one by one)
            for k in [1usize, 7, 8, 9, 19] {
                let mut ts: Vec<f64> = (0..k).map(|i| t + h * i as f64 / k as f64).collect();
                if k == 9 {
                    ts.push(t1);
                    ts.push(t1 + 4.0 * f64::EPSILON * t1.abs());
                    ts.push(t - 4.0 * f64::EPSILON * t.abs());
                }
                compare(&mut integ, &ts);
            }
            steps += 1;
            t = t1;
            if matches!(st, Step::Stopped(_)) || t >= t_end {
                break;
            }
            if steps % 11 == 0 {
                let mut y = integ.y().to_vec();
                for v in &mut y[..model.layout.n_x] {
                    *v *= 1.001;
                }
                integ.restart(t, &y).expect("restarts");
                compare(&mut integ, &[t, t]);
            }
        }
        assert!(steps > 20 && compared > 1000, "{method:?}: {steps} steps, {compared} values");
    }
}
