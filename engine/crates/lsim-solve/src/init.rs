//! Consistent initialisation (DESIGN.md, *Initialisation*): the iteration
//! variables z made consistent with the states, `g(t, x, z) = 0`, at the
//! start and after every event.
//!
//! 1. Damped Newton with the exact Jacobian `∂g/∂z` (dense LU up to 100
//!    unknowns, sparse LU above), a backtracking line search on the
//!    row-equilibrated residual, convergence judged like the integrator's
//!    own (the update's weighted norm far below the tolerance).
//! 2. If Newton fails: a homotopy. Without a simplified model from the
//!    component writer, the Newton homotopy `H(z, λ) = g(z) - (1 - λ)
//!    g(z0)` (solved by `z0` at λ = 0, by the answer at λ = 1) is followed
//!    in λ with steps controlled by the Newton iteration count.
//! 3. If that fails too: an error naming the equations with the largest
//!    residuals and their parts.
//!
//! The integrator then refines (IDA's `IDACalcIC` for the derivatives).

use crate::SolveError;
use crate::info::RunInfo;
use crate::jac::JacStructure;
use lsim_ir::runtime::{EvalInput, ModelFunctions};

/// How the start was made consistent.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InitOutcome {
    /// Newton iterations (all phases)
    pub newton_iterations: usize,
    /// homotopy steps (0: Newton alone sufficed)
    pub homotopy_steps: usize,
    /// the largest scaled residual at the end
    pub residual: f64,
}

impl InitOutcome {
    /// In words, for the report.
    pub fn describe(&self) -> String {
        if self.newton_iterations == 0 {
            "the iteration variables' start values were already consistent".into()
        } else if self.homotopy_steps == 0 {
            format!(
                "consistent iteration variables by damped Newton in {} iterations",
                self.newton_iterations
            )
        } else {
            format!(
                "Newton failed; consistent iteration variables by homotopy in {} steps ({} Newton iterations)",
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

struct Ctx<'a> {
    m: &'a dyn ModelFunctions,
    t: f64,
    p: &'a [f64],
    d: &'a [f64],
    u: &'a [f64],
    n_x: usize,
    n_z: usize,
    work: Vec<f64>,
    full: Vec<f64>,
    jac: &'a JacStructure,
    jvals: Vec<f64>,
    seed: Vec<f64>,
    jout: Vec<f64>,
}

impl Ctx<'_> {
    fn g(&mut self, y: &[f64], out: &mut [f64]) {
        let inp = EvalInput { t: self.t, y, p: self.p, d: self.d, u: self.u };
        self.m.residual(&inp, &mut self.work, &mut self.full);
        out.copy_from_slice(&self.full[self.n_x..]);
    }

    /// ∂g/∂z as (row, col, value) triplets in z-local indices.
    fn jac_z(&mut self, y: &[f64]) -> Vec<(usize, usize, f64)> {
        let inp = EvalInput { t: self.t, y, p: self.p, d: self.d, u: self.u };
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
}

/// Solves `A x = b` for an `n × n` matrix given as triplets; `None` when
/// singular.
fn lin_solve(n: usize, trip: &[(usize, usize, f64)], b: &[f64]) -> Option<Vec<f64>> {
    if n <= 100 {
        let mut a = faer::Mat::<f64>::zeros(n, n);
        for &(i, j, v) in trip {
            a[(i, j)] += v;
        }
        let lu = a.partial_piv_lu();
        let mut x = faer::Mat::<f64>::zeros(n, 1);
        for i in 0..n {
            x[(i, 0)] = b[i];
        }
        use faer::linalg::solvers::SolveCore;
        lu.solve_in_place_with_conj(faer::Conj::No, x.as_mut());
        let x: Vec<f64> = (0..n).map(|i| x[(i, 0)]).collect();
        x.iter().all(|v| v.is_finite()).then_some(x)
    } else {
        let t: Vec<faer::sparse::Triplet<usize, usize, f64>> =
            trip.iter().map(|&(i, j, v)| faer::sparse::Triplet::new(i, j, v)).collect();
        let a = faer::sparse::SparseColMat::<usize, f64>::try_new_from_triplets(n, n, &t).ok()?;
        let lu = a.sp_lu().ok()?;
        let mut x = faer::Mat::<f64>::zeros(n, 1);
        for i in 0..n {
            x[(i, 0)] = b[i];
        }
        use faer::linalg::solvers::SolveCore;
        lu.solve_in_place_with_conj(faer::Conj::No, x.as_mut());
        let x: Vec<f64> = (0..n).map(|i| x[(i, 0)]).collect();
        x.iter().all(|v| v.is_finite()).then_some(x)
    }
}

/// Damped Newton on `g(z) - shift = 0`. Returns the iterations used, or
/// `None` when it did not converge (y is then left at the best point).
fn newton(
    c: &mut Ctx<'_>,
    y: &mut [f64],
    shift: &[f64],
    s: &InitSettings,
    nominal: &[f64],
) -> Option<usize> {
    let (n_x, n_z) = (c.n_x, c.n_z);
    let mut f = vec![0.0; n_z];
    let mut trial = y.to_vec();
    let mut ft = vec![0.0; n_z];
    let weight = |z: f64, k: usize| 1.0 / (s.rtol * z.abs() + s.atol * nominal[n_x + k]);
    for it in 0..s.max_iterations {
        c.g(y, &mut f);
        for k in 0..n_z {
            f[k] -= shift[k];
        }
        let trip = c.jac_z(y);
        // row equilibration for the merit function
        let mut row_scale = vec![0.0f64; n_z];
        for &(i, _, v) in &trip {
            row_scale[i] = row_scale[i].max(v.abs());
        }
        for r in &mut row_scale {
            *r = if *r > 0.0 { 1.0 / *r } else { 1.0 };
        }
        let merit = |f: &[f64]| f.iter().zip(&row_scale).map(|(a, b)| (a * b).powi(2)).sum::<f64>();
        let phi0 = merit(&f);
        let neg: Vec<f64> = f.iter().map(|v| -v).collect();
        let dz = lin_solve(n_z, &trip, &neg)?;
        // converged when the full Newton update is far below the tolerance
        let norm =
            (dz.iter().enumerate().map(|(k, v)| (v * weight(y[n_x + k], k)).powi(2)).sum::<f64>()
                / n_z.max(1) as f64)
                .sqrt();
        let mut lambda = 1.0;
        loop {
            trial.copy_from_slice(y);
            for k in 0..n_z {
                trial[n_x + k] += lambda * dz[k];
            }
            c.g(&trial, &mut ft);
            for k in 0..n_z {
                ft[k] -= shift[k];
            }
            let phi = merit(&ft);
            if phi.is_finite() && (phi <= (1.0 - 1e-4 * lambda) * phi0 || phi == 0.0) {
                break;
            }
            lambda *= 0.5;
            if lambda < 1e-6 {
                return None;
            }
        }
        y.copy_from_slice(&trial);
        if norm < 1e-3 && lambda == 1.0 {
            return Some(it + 1);
        }
    }
    None
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
    let mut c = Ctx {
        m,
        t,
        p,
        d,
        u,
        n_x,
        n_z,
        work: vec![0.0; l.n_work],
        full: vec![0.0; n],
        jac,
        jvals: vec![0.0; jac.nnz()],
        seed: vec![0.0; n],
        jout: vec![0.0; n],
    };
    let nominal = &info.y_nominal;
    let zero = vec![0.0; n_z];
    let start = y.to_vec();
    // already consistent?
    let mut f0 = vec![0.0; n_z];
    c.g(y, &mut f0);
    if let Some(it) = newton(&mut c, y, &zero, s, nominal) {
        let mut out = InitOutcome { newton_iterations: it, ..Default::default() };
        let mut f = vec![0.0; n_z];
        c.g(y, &mut f);
        out.residual = f.iter().fold(0.0, |a, b| a.max(b.abs()));
        return Ok(out);
    }
    // homotopy from the start values: H(z, λ) = g(z) - (1 - λ) g(z0)
    y.copy_from_slice(&start);
    let mut lambda = 0.0f64;
    let mut dl = 0.1f64;
    let mut out = InitOutcome::default();
    let mut shift = vec![0.0; n_z];
    let mut last_good = y.to_vec();
    while lambda < 1.0 {
        let next = (lambda + dl).min(1.0);
        for k in 0..n_z {
            shift[k] = (1.0 - next) * f0[k];
        }
        let settings = InitSettings { max_iterations: 8, ..*s };
        match newton(&mut c, y, &shift, &settings, nominal) {
            Some(it) => {
                out.newton_iterations += it;
                out.homotopy_steps += 1;
                lambda = next;
                last_good.copy_from_slice(y);
                if it <= 3 {
                    dl = (dl * 2.0).min(0.5);
                }
            }
            None => {
                y.copy_from_slice(&last_good);
                dl *= 0.25;
                if dl < 1e-8 {
                    break;
                }
            }
        }
    }
    let mut f = vec![0.0; n_z];
    c.g(y, &mut f);
    if lambda >= 1.0 {
        // polish at λ = 1
        if let Some(it) = newton(&mut c, y, &zero, s, nominal) {
            out.newton_iterations += it;
            c.g(y, &mut f);
            out.residual = f.iter().fold(0.0, |a, b| a.max(b.abs()));
            return Ok(out);
        }
    }
    // name the worst equations
    let mut order: Vec<usize> = (0..n_z).collect();
    order.sort_by(|a, b| f[*b].abs().total_cmp(&f[*a].abs()));
    let worst: Vec<String> = order
        .iter()
        .take(3)
        .map(|&k| {
            let eq =
                info.residual_labels.get(k).cloned().unwrap_or_else(|| format!("residual {k}"));
            let var = info.y_names.get(n_x + k).cloned().unwrap_or_default();
            format!("{eq} (residual {:.3e}, solving for {var})", f[k])
        })
        .collect();
    Err(SolveError::Initialisation {
        t,
        message: format!(
            "neither Newton nor the homotopy (reached λ = {lambda:.3}) made the algebraic equations hold; the worst: {}",
            worst.join("; ")
        ),
    })
}
