//! One instant of an inverse model at a time (lsim-lap's `point()`): the
//! algebraic equations solved for the iteration variables at given inputs
//! (the prescribed speed and its slope) and parameters, with the states held
//! where the solver was started (a lap is quasi-steady: the battery's charge
//! and the energies do not move within one point), and every limit site
//! evaluated there.
//!
//! The solve is the fast-mode stepper's (`stepper::Stepper::solve_z`):
//! Newton's method on the explicit inverse of `∂g/∂z`, kept between solves
//! and updated by Broyden's method, fresh derivatives where it converges
//! slowly; each solve starts from the last one's answer (a lap solver's
//! points follow each other), and where that fails, once more from the
//! consistent start with fresh derivatives. The modes are then set from
//! their relations and the solve repeated while one changes. A point that
//! does not solve is an error, never a value: the lap solver reads it as a
//! point the car cannot reach.

use crate::limits::{self, DerSrc, LimitSite};
use crate::lu::Lu;
use lsim_ir::runtime::{EvalInput, ModelFunctions};

const MAX_NEWTON: usize = 30;
const MAX_MODE_ROUNDS: usize = 5;
/// an iteration variable this many times its size has run away (the stepper's RUNAWAY)
const RUNAWAY: f64 = 1e10;

/// Counts of the work done.
#[derive(Clone, Copy, Debug, Default)]
pub struct PointStats {
    /// solves asked for
    pub solves: usize,
    /// of them, solved again from the consistent start
    pub cold: usize,
    /// of them, not solved
    pub failed: usize,
    /// Newton iterations
    pub iterations: usize,
    /// residual evaluations
    pub residuals: usize,
    /// refreshes of `∂g/∂z`
    pub refreshes: usize,
}

/// The solver of one instant at a time.
pub struct PointSolver<'a> {
    m: &'a dyn ModelFunctions,
    p: Vec<f64>,
    nx: usize,
    nz: usize,
    y: Vec<f64>,
    d: Vec<f64>,
    u: Vec<f64>,
    y_ref: Vec<f64>,
    d_ref: Vec<f64>,
    nom: Vec<f64>,
    zmag: Vec<f64>,
    hinv: Vec<f64>,
    valid: bool,
    work: Vec<f64>,
    out: Vec<f64>,
    jv: Vec<f64>,
    jo: Vec<f64>,
    dz: Vec<f64>,
    sz: Vec<f64>,
    g_old: Vec<f64>,
    hy: Vec<f64>,
    inputs: Vec<(usize, u8)>,
    vars: Vec<f64>,
    limits: &'a [LimitSite],
    ders: &'a [Option<DerSrc>],
    tol: f64,
    /// the work done so far
    pub stats: PointStats,
}

impl<'a> PointSolver<'a> {
    /// A solver started at the consistent point `y0`, `d0` (its states are
    /// held there), with parameters `p`; `inputs` as
    /// [`crate::FastProblem::inputs`] (empty: a value and a slope per
    /// prescribed variable, in order); `y_nominal` each entry's size
    /// (empty: 1).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        m: &'a dyn ModelFunctions,
        p: &[f64],
        y0: Vec<f64>,
        d0: Vec<f64>,
        y_nominal: &[f64],
        inputs: &[(usize, u8)],
        limits: &'a [LimitSite],
        ders: &'a [Option<DerSrc>],
        newton_tol: f64,
    ) -> Self {
        let l = *m.layout();
        let (nx, nz, ny) = (l.n_x, l.n_z, l.n_y());
        let nom: Vec<f64> =
            (0..ny).map(|i| y_nominal.get(i).copied().unwrap_or(1.0).abs().max(1e-300)).collect();
        let zmag = (0..nz).map(|k| y0[nx + k].abs().max(nom[nx + k])).collect();
        let inputs = if inputs.is_empty() {
            (0..l.n_u).map(|j| (j / 2, (j % 2) as u8)).collect()
        } else {
            inputs.to_vec()
        };
        PointSolver {
            m,
            p: p.to_vec(),
            nx,
            nz,
            y: y0.clone(),
            d: d0.clone(),
            u: vec![0.0; l.n_u],
            y_ref: y0,
            d_ref: d0,
            nom,
            zmag,
            hinv: vec![0.0; nz * nz],
            valid: false,
            work: vec![0.0; l.n_work],
            out: vec![0.0; ny],
            jv: vec![0.0; ny],
            jo: vec![0.0; ny],
            dz: vec![0.0; nz],
            sz: vec![0.0; nz],
            g_old: vec![0.0; nz],
            hy: vec![0.0; nz],
            inputs,
            vars: vec![0.0; l.n_vars],
            limits,
            ders,
            tol: newton_tol,
            stats: PointStats::default(),
        }
    }

    /// The parameters (a lap solver sets the curvature and grade here).
    pub fn params_mut(&mut self) -> &mut [f64] {
        &mut self.p
    }

    /// The parameters.
    pub fn params(&self) -> &[f64] {
        &self.p
    }

    /// Solves the instant at the prescribed variables' `(value, slope)`.
    /// Leaves the solution in place (for [`Self::vars`] and
    /// [`Self::sites`]) and starts the next solve from it.
    pub fn solve(&mut self, prescribed: &[(f64, f64)]) -> Result<(), String> {
        self.stats.solves += 1;
        for (j, &(i, order)) in self.inputs.iter().enumerate() {
            let (v, s) = prescribed.get(i).copied().unwrap_or((0.0, 0.0));
            self.u[j] = match order {
                0 => v,
                1 => s,
                _ => 0.0,
            };
        }
        let warm = (self.y.clone(), self.d.clone());
        if self.settled().is_ok() {
            return Ok(());
        }
        // once more from the consistent start, with fresh derivatives
        self.stats.cold += 1;
        self.y.copy_from_slice(&self.y_ref);
        self.d.copy_from_slice(&self.d_ref);
        self.valid = false;
        match self.settled() {
            Ok(()) => Ok(()),
            Err(e) => {
                self.stats.failed += 1;
                // the next solve starts from the last good point
                (self.y, self.d) = warm;
                self.valid = false;
                Err(e)
            }
        }
    }

    /// Every variable as [`Self::vars`] or [`Self::sites`] last read them.
    pub fn last_vars(&self) -> &[f64] {
        &self.vars
    }

    /// Every variable at the last solved point (`ModelFunctions::vars`).
    pub fn vars(&mut self) -> &[f64] {
        let inp = EvalInput { t: 0.0, y: &self.y, p: &self.p, d: &self.d, u: &self.u };
        self.m.vars(&inp, &mut self.work, &mut self.vars);
        &self.vars
    }

    /// Each limit site's `[value, lo, hi]` at the last solved point.
    pub fn sites(&mut self) -> Vec<[f64; 3]> {
        self.residual();
        let inp = EvalInput { t: 0.0, y: &self.y, p: &self.p, d: &self.d, u: &self.u };
        self.m.vars(&inp, &mut self.work, &mut self.vars);
        let pt = limits::Point {
            t: 0.0,
            vars: &self.vars,
            p: &self.p,
            y: &self.y,
            xdot: &self.out[..self.nx],
            ders: self.ders,
            model: self.m,
        };
        self.limits.iter().map(|s| pt.site(s)).collect()
    }

    fn residual(&mut self) {
        let inp = EvalInput { t: 0.0, y: &self.y, p: &self.p, d: &self.d, u: &self.u };
        self.m.residual(&inp, &mut self.work, &mut self.out);
        self.stats.residuals += 1;
    }

    /// Solves, then sets the modes from their relations, solving again
    /// while one changes.
    fn settled(&mut self) -> Result<(), String> {
        self.solve_z()?;
        if self.d.is_empty() {
            return Ok(());
        }
        let mut d_new = self.d.clone();
        for _ in 0..MAX_MODE_ROUNDS {
            {
                let inp = EvalInput { t: 0.0, y: &self.y, p: &self.p, d: &self.d, u: &self.u };
                self.m.modes(&inp, &mut self.work, &mut d_new);
            }
            if d_new == self.d {
                return Ok(());
            }
            self.d.copy_from_slice(&d_new);
            self.valid = false;
            self.solve_z()?;
        }
        Err("its modes did not settle".into())
    }

    fn refresh(&mut self) -> Result<(), String> {
        let (nx, nz) = (self.nx, self.nz);
        let mut block = vec![0.0; nz * nz];
        for k in 0..nz {
            self.jv.iter_mut().for_each(|v| *v = 0.0);
            self.jv[nx + k] = 1.0;
            let inp = EvalInput { t: 0.0, y: &self.y, p: &self.p, d: &self.d, u: &self.u };
            self.m.jvp(&inp, &self.jv, &mut self.work, &mut self.jo);
            block[k * nz..(k + 1) * nz].copy_from_slice(&self.jo[nx..]);
        }
        self.stats.refreshes += 1;
        let mut lu = Lu::new(nz);
        if lu.factor(&block).is_err() {
            return Err("the algebraic equations' Jacobian is singular there".into());
        }
        for c in 0..nz {
            let col = &mut self.hinv[c * nz..(c + 1) * nz];
            col.iter_mut().for_each(|v| *v = 0.0);
            col[c] = 1.0;
            lu.solve(col);
        }
        self.valid = true;
        Ok(())
    }

    fn newton_step(&mut self) {
        let (nx, nz) = (self.nx, self.nz);
        self.dz.iter_mut().for_each(|v| *v = 0.0);
        for c in 0..nz {
            let g = self.out[nx + c];
            if g != 0.0 {
                let col = &self.hinv[c * nz..(c + 1) * nz];
                for (d, h) in self.dz.iter_mut().zip(col) {
                    *d -= h * g;
                }
            }
        }
    }

    fn broyden(&mut self) {
        let (nx, nz) = (self.nx, self.nz);
        self.hy.iter_mut().for_each(|v| *v = 0.0);
        for c in 0..nz {
            let yc = self.out[nx + c] - self.g_old[c];
            let col = &self.hinv[c * nz..(c + 1) * nz];
            for (h, x) in self.hy.iter_mut().zip(col) {
                *h += x * yc;
            }
        }
        let shy: f64 = self.sz.iter().zip(&self.hy).map(|(a, b)| a * b).sum();
        let sn: f64 = self.sz.iter().map(|v| v * v).sum::<f64>().sqrt();
        let hn: f64 = self.hy.iter().map(|v| v * v).sum::<f64>().sqrt();
        if !(shy.is_finite() && shy.abs() > 1e-12 * sn * hn) {
            return;
        }
        for c in 0..nz {
            let col = &mut self.hinv[c * nz..(c + 1) * nz];
            let sth: f64 = self.sz.iter().zip(col.iter()).map(|(a, b)| a * b).sum();
            if sth == 0.0 {
                continue;
            }
            let f = sth / shy;
            for ((h, s), hy) in col.iter_mut().zip(&self.sz).zip(&self.hy) {
                *h += (s - hy) * f;
            }
        }
    }

    /// `g(x, z, u) = 0` for `z`, from `z` as it is.
    fn solve_z(&mut self) -> Result<(), String> {
        self.residual();
        if self.nz == 0 {
            return Ok(());
        }
        let (nx, nz) = (self.nx, self.nz);
        if !self.valid {
            self.refresh()?;
        }
        let mut prev: Option<f64> = None;
        let mut refreshes = 0;
        for _ in 0..MAX_NEWTON {
            if self.out[nx..].iter().any(|v| !v.is_finite()) {
                return Err("the algebraic equations give a value that is not a number".into());
            }
            self.newton_step();
            let mut norm = 0.0f64;
            let mut away = false;
            for k in 0..nz {
                let a = self.y[nx + k].abs();
                away |= a > RUNAWAY * self.zmag[k];
                norm = norm.max(self.dz[k].abs() / a.max(self.nom[nx + k]));
            }
            if away {
                return Err("its Newton iteration ran away".into());
            }
            if norm <= self.tol {
                return Ok(());
            }
            if let Some(p) = prev
                && norm / p > 0.5
                && refreshes < 8
            {
                self.refresh()?;
                refreshes += 1;
                prev = None;
                continue;
            }
            self.stats.iterations += 1;
            self.g_old.copy_from_slice(&self.out[nx..]);
            let mut lambda = 1.0;
            for _ in 0..40 {
                for k in 0..nz {
                    self.y[nx + k] += lambda * self.dz[k];
                }
                self.residual();
                if self.out.iter().all(|v| v.is_finite()) {
                    break;
                }
                for k in 0..nz {
                    self.y[nx + k] -= lambda * self.dz[k];
                }
                lambda *= 0.5;
            }
            for k in 0..nz {
                self.sz[k] = lambda * self.dz[k];
            }
            self.broyden();
            prev = Some(norm);
        }
        Err(format!("its algebraic equations did not converge in {MAX_NEWTON} Newton iterations"))
    }
}
