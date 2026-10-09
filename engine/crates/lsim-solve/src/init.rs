//! Initialisation (DESIGN.md §8.3).
//!
//! * **The start**: when the compiled model has an initialisation system
//!   ([`ModelFunctions::init`]), damped Newton on its unknowns `w` with its
//!   exact sparse Jacobian, then `finish` gives the start vector; every mode
//!   is then set from its relation at the solution and the system solved
//!   again if a mode changed ([`initialise`]).
//! * **Consistent iteration variables**: `g(t, x, z) = 0` for z at the
//!   start and after every event ([`consistent_z`]).
//!
//! Both solve by
//!
//! 1. damped Newton with the exact Jacobian (dense LU up to 100 unknowns,
//!    sparse LU above), a backtracking line search on the row-equilibrated
//!    residual, convergence judged like the integrator's own (the update's
//!    weighted norm far below the tolerance, or the residual at round-off
//!    after full Newton steps);
//! 2. if Newton fails, a homotopy. Without a simplified model from the
//!    component writer, the Newton homotopy `H(w, λ) = F(w) - (1 - λ)
//!    F(w0)` (solved by `w0` at λ = 0, by the answer at λ = 1) is followed
//!    in λ with steps controlled by the Newton iteration count;
//! 3. if that fails too, an error naming the equations with the largest
//!    residuals and their parts.
//!
//! The integrator then refines (IDA's `IDACalcIC` for the derivatives).

use crate::SolveError;
use crate::info::RunInfo;
use crate::jac::JacStructure;
use lsim_ir::runtime::{EvalInput, InitFunctions, ModelFunctions};

/// How a system was solved.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InitOutcome {
    /// Newton iterations (all phases)
    pub newton_iterations: usize,
    /// homotopy steps (0: Newton alone sufficed)
    pub homotopy_steps: usize,
    /// the largest residual at the end
    pub residual: f64,
}

impl InitOutcome {
    /// In words, for the report.
    pub fn describe(&self) -> String {
        if self.newton_iterations == 0 {
            "the start values were already consistent".into()
        } else if self.homotopy_steps == 0 {
            format!("damped Newton in {} iteration(s)", self.newton_iterations)
        } else {
            format!(
                "Newton failed; homotopy in {} steps ({} Newton iterations)",
                self.homotopy_steps, self.newton_iterations
            )
        }
    }
}

/// The settings of the solve.
#[derive(Clone, Copy, Debug)]
pub struct InitSettings {
    /// relative tolerance of the run
    pub rtol: f64,
    /// absolute tolerance of the run (times each nominal)
    pub atol: f64,
    /// Newton iterations per solve
    pub max_iterations: usize,
}

/// A square nonlinear system `F(w) = 0`.
trait System {
    fn n(&self) -> usize;
    fn residual(&mut self, w: &[f64], out: &mut [f64]);
    /// `∂F/∂w` as (row, column, value) triplets
    fn jacobian(&mut self, w: &[f64]) -> Vec<(usize, usize, f64)>;
    /// the scale of the unknown k (its nominal value)
    fn nominal(&self, k: usize) -> f64;
}

/// The iteration variables of the model, the states held.
struct ZBlock<'a> {
    m: &'a dyn ModelFunctions,
    t: f64,
    p: &'a [f64],
    d: &'a [f64],
    u: &'a [f64],
    n_x: usize,
    n_z: usize,
    y: Vec<f64>,
    work: Vec<f64>,
    full: Vec<f64>,
    jac: &'a JacStructure,
    jvals: Vec<f64>,
    seed: Vec<f64>,
    jout: Vec<f64>,
    nominal: &'a [f64],
}

impl System for ZBlock<'_> {
    fn n(&self) -> usize {
        self.n_z
    }

    fn residual(&mut self, w: &[f64], out: &mut [f64]) {
        self.y[self.n_x..].copy_from_slice(w);
        let inp = EvalInput { t: self.t, y: &self.y, p: self.p, d: self.d, u: self.u };
        self.m.residual(&inp, &mut self.work, &mut self.full);
        out.copy_from_slice(&self.full[self.n_x..]);
    }

    fn jacobian(&mut self, w: &[f64]) -> Vec<(usize, usize, f64)> {
        self.y[self.n_x..].copy_from_slice(w);
        let inp = EvalInput { t: self.t, y: &self.y, p: self.p, d: self.d, u: self.u };
        self.jac.eval(
            self.m,
            &inp,
            &mut self.work,
            &mut self.seed,
            &mut self.jout,
            &mut self.jvals,
        );
        let mut trip = vec![];
        for j in self.n_x..self.n_x + self.n_z {
            for k in self.jac.col_ptr[j]..self.jac.col_ptr[j + 1] {
                let i = self.jac.row_idx[k];
                if i >= self.n_x {
                    trip.push((i - self.n_x, j - self.n_x, self.jvals[k]));
                }
            }
        }
        trip
    }

    fn nominal(&self, k: usize) -> f64 {
        self.nominal[self.n_x + k]
    }
}

/// The compiled initialisation system.
struct InitBlock<'a> {
    f: &'a dyn InitFunctions,
    t: f64,
    p: &'a [f64],
    d: &'a [f64],
    u: &'a [f64],
    work: Vec<f64>,
    vals: Vec<f64>,
}

impl System for InitBlock<'_> {
    fn n(&self) -> usize {
        self.f.n_w()
    }

    fn residual(&mut self, w: &[f64], out: &mut [f64]) {
        let inp = EvalInput { t: self.t, y: w, p: self.p, d: self.d, u: self.u };
        self.f.residual(&inp, &mut self.work, out);
    }

    fn jacobian(&mut self, w: &[f64]) -> Vec<(usize, usize, f64)> {
        let inp = EvalInput { t: self.t, y: w, p: self.p, d: self.d, u: self.u };
        self.f.jacobian_sparse(&inp, &mut self.work, &mut self.vals);
        let pat = self.f.sparsity();
        let mut trip = Vec::with_capacity(self.vals.len());
        for j in 0..pat.n {
            for k in pat.col_ptr[j]..pat.col_ptr[j + 1] {
                trip.push((pat.row_idx[k], j, self.vals[k]));
            }
        }
        trip
    }

    fn nominal(&self, _k: usize) -> f64 {
        1.0
    }
}

/// Solves `A x = b` for an `n × n` matrix given as triplets; `None` when
/// singular.
pub(crate) fn lin_solve(n: usize, trip: &[(usize, usize, f64)], b: &[f64]) -> Option<Vec<f64>> {
    use faer::linalg::solvers::SolveCore;
    let mut x = faer::Mat::<f64>::zeros(n, 1);
    for i in 0..n {
        x[(i, 0)] = b[i];
    }
    if n <= 100 {
        let mut a = faer::Mat::<f64>::zeros(n, n);
        for &(i, j, v) in trip {
            a[(i, j)] += v;
        }
        a.partial_piv_lu().solve_in_place_with_conj(faer::Conj::No, x.as_mut());
    } else {
        let t: Vec<faer::sparse::Triplet<usize, usize, f64>> =
            trip.iter().map(|&(i, j, v)| faer::sparse::Triplet::new(i, j, v)).collect();
        let a = faer::sparse::SparseColMat::<usize, f64>::try_new_from_triplets(n, n, &t).ok()?;
        a.sp_lu().ok()?.solve_in_place_with_conj(faer::Conj::No, x.as_mut());
    }
    let x: Vec<f64> = (0..n).map(|i| x[(i, 0)]).collect();
    x.iter().all(|v| v.is_finite()).then_some(x)
}

/// Damped Newton on `F(w) - shift = 0`. Returns the iterations used, or
/// `None` when it did not converge (w is then left at the best point).
fn newton(sys: &mut dyn System, w: &mut [f64], shift: &[f64], s: &InitSettings) -> Option<usize> {
    let n = sys.n();
    let mut f = vec![0.0; n];
    let mut trial = w.to_vec();
    let mut ft = vec![0.0; n];
    let weights = |w: &[f64], sys: &dyn System| -> Vec<f64> {
        (0..n).map(|k| 1.0 / (s.rtol * w[k].abs() + s.atol * sys.nominal(k))).collect()
    };
    let mut full_steps = 0;
    for it in 0..s.max_iterations {
        sys.residual(w, &mut f);
        for k in 0..n {
            f[k] -= shift[k];
        }
        let trip = sys.jacobian(w);
        // row equilibration for the merit function
        let mut row_scale = vec![0.0f64; n];
        for &(i, _, v) in &trip {
            row_scale[i] = row_scale[i].max(v.abs());
        }
        for r in &mut row_scale {
            *r = if *r > 0.0 { 1.0 / *r } else { 1.0 };
        }
        let merit = |f: &[f64]| f.iter().zip(&row_scale).map(|(a, b)| (a * b).powi(2)).sum::<f64>();
        let phi0 = merit(&f);
        let neg: Vec<f64> = f.iter().map(|v| -v).collect();
        let dw = lin_solve(n, &trip, &neg)?;
        let wt = weights(w, sys);
        let norm = (dw.iter().zip(&wt).map(|(v, k)| (v * k).powi(2)).sum::<f64>()
            / n.max(1) as f64)
            .sqrt();
        // an update far below the tolerance: converged
        if norm < 1e-3 {
            for k in 0..n {
                w[k] += dw[k];
            }
            return Some(it + 1);
        }
        let mut lambda = 1.0;
        loop {
            for k in 0..n {
                trial[k] = w[k] + lambda * dw[k];
            }
            sys.residual(&trial, &mut ft);
            for k in 0..n {
                ft[k] -= shift[k];
            }
            let phi = merit(&ft);
            if phi.is_finite() && (phi <= (1.0 - 1e-4 * lambda) * phi0 || phi == 0.0) {
                break;
            }
            lambda *= 0.5;
            if lambda < 1e-6 {
                // no decrease: at round-off after full Newton steps, with
                // the update already inside the tolerance, that is
                // convergence; otherwise a failure
                return (full_steps > 0 && norm < 1.0).then_some(it);
            }
        }
        w.copy_from_slice(&trial);
        if lambda == 1.0 {
            full_steps += 1;
        }
    }
    None
}

/// Newton, then the Newton homotopy. On failure: the λ reached and the
/// residual at the end.
fn solve(
    sys: &mut dyn System,
    w: &mut [f64],
    s: &InitSettings,
) -> Result<InitOutcome, (f64, Vec<f64>)> {
    let n = sys.n();
    let zero = vec![0.0; n];
    let start = w.to_vec();
    let mut f0 = vec![0.0; n];
    sys.residual(w, &mut f0);
    let residual = |sys: &mut dyn System, w: &[f64]| {
        let mut f = vec![0.0; n];
        sys.residual(w, &mut f);
        f
    };
    if let Some(it) = newton(sys, w, &zero, s) {
        let f = residual(sys, w);
        return Ok(InitOutcome {
            newton_iterations: it,
            homotopy_steps: 0,
            residual: f.iter().fold(0.0, |a, b| a.max(b.abs())),
        });
    }
    // homotopy from the start values: H(w, λ) = F(w) - (1 - λ) F(w0)
    w.copy_from_slice(&start);
    let mut lambda = 0.0f64;
    let mut dl = 0.1f64;
    let mut out = InitOutcome::default();
    let mut shift = vec![0.0; n];
    let mut last_good = w.to_vec();
    let settings = InitSettings { max_iterations: 8, ..*s };
    while lambda < 1.0 {
        let next = (lambda + dl).min(1.0);
        for k in 0..n {
            shift[k] = (1.0 - next) * f0[k];
        }
        match newton(sys, w, &shift, &settings) {
            Some(it) => {
                out.newton_iterations += it;
                out.homotopy_steps += 1;
                lambda = next;
                last_good.copy_from_slice(w);
                if it <= 3 {
                    dl = (dl * 2.0).min(0.5);
                }
            }
            None => {
                w.copy_from_slice(&last_good);
                dl *= 0.25;
                if dl < 1e-8 {
                    break;
                }
            }
        }
    }
    if lambda >= 1.0 {
        // polish at λ = 1
        if let Some(it) = newton(sys, w, &zero, s) {
            out.newton_iterations += it;
            let f = residual(sys, w);
            out.residual = f.iter().fold(0.0, |a, b| a.max(b.abs()));
            return Ok(out);
        }
    }
    Err((lambda, residual(sys, w)))
}

fn worst(f: &[f64], name: impl Fn(usize) -> String) -> String {
    let mut order: Vec<usize> = (0..f.len()).collect();
    order.sort_by(|a, b| f[*b].abs().total_cmp(&f[*a].abs()));
    order
        .iter()
        .take(3)
        .map(|&k| format!("{} (residual {:.3e})", name(k), f[k]))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Makes the iteration variables of `y` consistent with its states at `t`.
#[allow(clippy::too_many_arguments)]
pub fn consistent_z(
    m: &dyn ModelFunctions,
    info: &RunInfo,
    jac: &JacStructure,
    t: f64,
    y: &mut [f64],
    p: &[f64],
    d: &[f64],
    u: &[f64],
    s: &InitSettings,
) -> Result<InitOutcome, SolveError> {
    let l = *m.layout();
    let (n_x, n_z) = (l.n_x, l.n_z);
    if n_z == 0 {
        return Ok(InitOutcome::default());
    }
    let n = l.n_y();
    let mut sys = ZBlock {
        m,
        t,
        p,
        d,
        u,
        n_x,
        n_z,
        y: y.to_vec(),
        work: vec![0.0; l.n_work],
        full: vec![0.0; n],
        jac,
        jvals: vec![0.0; jac.nnz()],
        seed: vec![0.0; n],
        jout: vec![0.0; n],
        nominal: &info.y_nominal,
    };
    let mut z = y[n_x..].to_vec();
    match solve(&mut sys, &mut z, s) {
        Ok(out) => {
            y[n_x..].copy_from_slice(&z);
            Ok(out)
        }
        Err((lambda, f)) => Err(SolveError::Initialisation {
            t,
            message: format!(
                "neither Newton nor the homotopy (reached λ = {lambda:.3}) made the algebraic equations hold; the worst: {}",
                worst(&f, |k| {
                    let eq = info
                        .residual_labels
                        .get(k)
                        .cloned()
                        .unwrap_or_else(|| format!("residual {k}"));
                    let var = info.y_names.get(n_x + k).cloned().unwrap_or_default();
                    format!("{eq}, solving for {var}")
                })
            ),
        }),
    }
}

/// The rates `y' = [x'; z']` at a consistent point `(t, y)`: `x'` from the
/// model, `z'` from the algebraic equations differentiated along the
/// solution, `0 = g_x x' + g_z z' + g_t` (`g_t` by a forward difference in
/// time, for time-dependent inputs). IDA restarts from them: with `z' = 0`
/// its predictor misses the iteration variables' motion by `h z'` and the
/// error test holds the first steps to a fraction of the tolerance. When
/// `g_z` is singular (it cannot be at a consistent point of an index-1
/// model) `z'` stays 0.
#[allow(clippy::too_many_arguments, clippy::needless_range_loop)]
pub fn rates(
    m: &dyn ModelFunctions,
    jac: &JacStructure,
    t: f64,
    y: &[f64],
    p: &[f64],
    d: &[f64],
    u: &[f64],
    yp: &mut [f64],
) {
    let l = *m.layout();
    let (n_x, n_z) = (l.n_x, l.n_z);
    let n = l.n_y();
    let mut work = vec![0.0; l.n_work];
    let mut f = vec![0.0; n];
    let inp = EvalInput { t, y, p, d, u };
    m.residual(&inp, &mut work, &mut f);
    yp[..n_x].copy_from_slice(&f[..n_x]);
    yp[n_x..].fill(0.0);
    if n_z == 0 {
        return;
    }
    let mut vals = vec![0.0; jac.nnz()];
    let (mut seed, mut out) = (vec![0.0; n], vec![0.0; n]);
    jac.eval(m, &inp, &mut work, &mut seed, &mut out, &mut vals);
    // g_t: the residuals one small time step on, the unknowns held
    let dt = 1e-7 * t.abs().max(1.0);
    let mut f_dt = vec![0.0; n];
    m.residual(&EvalInput { t: t + dt, ..inp }, &mut work, &mut f_dt);
    let mut rhs: Vec<f64> = (0..n_z).map(|k| -(f_dt[n_x + k] - f[n_x + k]) / dt).collect();
    let mut trip = vec![];
    for j in 0..n {
        for k in jac.col_ptr[j]..jac.col_ptr[j + 1] {
            let i = jac.row_idx[k];
            if i < n_x {
                continue;
            }
            if j < n_x {
                rhs[i - n_x] -= vals[k] * f[j];
            } else {
                trip.push((i - n_x, j - n_x, vals[k]));
            }
        }
    }
    if let Some(zp) = lin_solve(n_z, &trip, &rhs) {
        yp[n_x..].copy_from_slice(&zp);
    }
}

/// The first step after a restart at `(t, y)` with the rates `yp`, as
/// CVODE sizes its own first step: the order-1 error of a step h is about
/// `h² ‖x''‖ / 2` (weighted as the error test weighs it), so `h0 = ½
/// √(2 / ‖x''‖)`, `x''` a difference of `x'` along the solution over a
/// trial step (shortened once when the estimate says it is too long). No
/// longer than `h_cap` (the step the integrator had planned: the
/// solution's other time scales do not change at an event), nor than
/// `h_max` (0: none).
#[allow(clippy::too_many_arguments)]
pub fn first_step(
    m: &dyn ModelFunctions,
    t: f64,
    y: &[f64],
    yp: &[f64],
    p: &[f64],
    d: &[f64],
    u: &[f64],
    s: &InitSettings,
    nominal: &[f64],
    h_cap: f64,
) -> f64 {
    let l = *m.layout();
    let (n_x, n) = (l.n_x, l.n_y());
    if n_x == 0 || h_cap <= 0.0 {
        return h_cap;
    }
    let mut work = vec![0.0; l.n_work];
    let mut f = vec![0.0; n];
    let mut ys = vec![0.0; n];
    let mut hg = h_cap;
    let mut h_new = h_cap;
    for _ in 0..2 {
        for i in 0..n {
            ys[i] = y[i] + hg * yp[i];
        }
        m.residual(&EvalInput { t: t + hg, y: &ys, p, d, u }, &mut work, &mut f);
        let mut sum = 0.0;
        for i in 0..n_x {
            let w = 1.0 / (s.rtol * y[i].abs() + s.atol * nominal[i]);
            sum += ((f[i] - yp[i]) / hg * w).powi(2);
        }
        let ydd = (sum / n_x as f64).sqrt();
        if !ydd.is_finite() {
            return h_cap * 1e-3;
        }
        h_new = if ydd * hg * hg > 2.0 { (2.0 / ydd).sqrt() } else { hg };
        if h_new >= hg {
            break;
        }
        hg = h_new;
    }
    (0.5 * h_new).min(h_cap)
}

/// The start of a run: solves the model's initialisation system when it
/// has one, writing the start vector into `y0`; then sets every mode from
/// its relation at the start, solving again (a few times at most) while a
/// mode changes. Returns what was done, for the report.
#[allow(clippy::too_many_arguments)]
pub fn initialise(
    m: &dyn ModelFunctions,
    info: &RunInfo,
    t0: f64,
    y0: &mut [f64],
    d0: &mut [f64],
    u: &[f64],
    s: &InitSettings,
) -> Result<String, SolveError> {
    let l = *m.layout();
    let p = &info.params;
    let mut work = vec![0.0; l.n_work];
    let mut what = String::from("start values as declared");
    for _round in 0..5 {
        if let Some(f) = m.init() {
            let n_w = f.n_w();
            let mut w = vec![0.0; n_w];
            f.guess(p, &mut w);
            if n_w > 0 {
                let mut sys = InitBlock {
                    f,
                    t: t0,
                    p,
                    d: d0,
                    u,
                    work: vec![0.0; l.n_work],
                    vals: vec![0.0; f.sparsity().row_idx.len()],
                };
                match solve(&mut sys, &mut w, s) {
                    Ok(out) => {
                        what = format!(
                            "the initialisation system ({n_w} unknowns) by {}",
                            out.describe()
                        )
                    }
                    Err((lambda, r)) => {
                        return Err(SolveError::Initialisation {
                            t: t0,
                            message: format!(
                                "the initialisation system did not converge (homotopy reached λ = {lambda:.3}); the worst: {}",
                                worst(&r, |k| info
                                    .init_labels
                                    .get(k)
                                    .cloned()
                                    .unwrap_or_else(|| format!("start equation {k}")))
                            ),
                        });
                    }
                }
            } else {
                what = "the initialisation system (explicit)".into();
            }
            let inp = EvalInput { t: t0, y: &w, p, d: d0, u };
            f.finish(&inp, &mut work, y0);
        }
        // every mode from its relation at the start
        let before = d0.to_vec();
        let inp = EvalInput { t: t0, y: y0, p, d: &before, u };
        m.modes(&inp, &mut work, d0);
        if d0 == before.as_slice() || m.init().is_none() {
            return Ok(what);
        }
    }
    Ok(format!("{what}; the modes still changed after five rounds"))
}
