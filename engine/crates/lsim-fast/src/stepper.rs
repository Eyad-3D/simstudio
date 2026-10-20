//! The fixed-step inverse-model stepper.
//!
//! The model is `x' = f(t, x, z, p, d, u)`, `0 = g(t, x, z, p, d, u)`. The
//! stepper treats it in *state-space form*: wherever it evaluates the model
//! it first solves `g = 0` for the iteration variables `z`, so every
//! evaluated point is consistent and every recorded channel and flag comes
//! from a point that satisfies all the equations (to the Newton tolerance).
//! The remaining states `x` are stepped by the Rosenbrock-W method with the
//! Jacobian of the state-space form, `S = f_x - f_z g_z⁻¹ g_x`, kept over
//! many steps: a W-method keeps its order with any matrix there, so a stale
//! Jacobian costs nothing in order and only matters for the stability of
//! stiff components; it is refreshed when the embedded error estimate says
//! the old one has gone bad (and, optionally, every so many steps).
//!
//! **Solving for z.** Newton's method with the inverse of `∂g/∂z` kept
//! current by Broyden updates (fresh from the model's exact derivatives
//! when convergence slows), started from a prediction made with the stored
//! derivatives — `z ≈ z_ref - G (x - x_ref) - G_u (u - u_ref)` with
//! `G = g_z⁻¹ g_x` and `G_u = g_z⁻¹ g_u` — and stopped when the error left
//! after the current Newton step, estimated from the contraction rate, is
//! within the tolerance (the step is then applied and `x'` corrected to
//! first order instead of evaluating the model again). The inputs carry an
//! inverse model's time dependence, so the prediction follows the trace
//! between stages and across the jumps of its slope. Every solve keeps to
//! the root that continues from its consistent reference point (the
//! equations may have others: a battery's power has two currents, one
//! with the bus voltage near 0): a prediction farther from that point
//! than twice the variables' size so far (their largest magnitudes at
//! consistent points) is dropped for the point itself, its derivatives
//! stale (they are refreshed at the next step); a solution that far is
//! checked by a solve from the point itself, and the root nearer it kept;
//! a failed solve is tried again from there with fresh derivatives; a
//! solution off by ten orders of magnitude is no root. A singular
//! `∂g/∂z` names the iteration variables it leaves free.
//!
//! **Steps.** Each step `[t_n, t_{n+1}]` lies on one piece of every
//! trace (the uniform grid is split at every breakpoint inside a step:
//! every sample where a trace's slope changes, [`Trace::breakpoints`]), so
//! the prescribed speed is linear and its derivative constant within it.
//! The recorded value at `t_{n+1}` is the left limit (the end of the step,
//! with the step's own acceleration); each output interval's minimum,
//! maximum and mean come from the start, the middle (cubic Hermite
//! interpolation of the states, made consistent) and the end of every step
//! inside it, the mean by Simpson's rule.
//!
//! **Events.** A zero crossing between the start and the end of a step is
//! located on the Hermite interpolant (Illinois method, every point made
//! consistent), the step is redone up to the event, the `when` clause's
//! actions run, and stepping goes on from there. **Sampled blocks**
//! ([`lsim_ir::DiscreteBlock`]) tick at the start of the first step at or
//! after each of their tick times.
//!
//! **Input events** (FMI 3.0: a continuous input with a discontinuity).
//! At a breakpoint the inputs' slopes jump, and with them the inputs that
//! are a trace's derivatives (an inverse model's `der(body.v)`); a block's
//! tick that changes its outputs jumps discrete values. A condition that
//! reads them jumps too, between two steps, where no root is to be found.
//! There the stepper enters event mode: it takes the roots just before the
//! jump (the last step's end) and just after it, fires each `when` whose
//! condition became true (as the forward run loop: `x >= 0` holds at
//! zero, a strict `x > 0` does not), runs the actions, sets every mode
//! from its relation, and repeats until nothing changes (each round's
//! roots compared with the last), before stepping on. A located event's
//! actions are followed by the same iteration. After every step it also
//! checks the conditions whose crossings were exactly 0 at the step's
//! start and left it (crossing tests cannot see a sign change from 0): a
//! mode whose relation that changes, a strict `when` that becomes true.
//! It enters event mode at the step's start with those crossings taken on
//! the side they leave to and those modes held there, so what depends on
//! them follows at once, and redoes the step. A mode wrong for any other
//! reason is set at the step's end, in event mode.

#![allow(clippy::needless_range_loop)] // index loops read like the formulas

use crate::limits::{self, DerSrc, FlagTracker, LimitSite};
use crate::lu::Lu;
use crate::ros::Tableau;
use crate::{
    BoundBlock, FastError, FastEvent, FastOptions, FastProblem, FastReport, FastResult,
    FixedMethod, Trace, WhenInfo,
};
use lsim_ir::prepared::Direction;
use lsim_ir::runtime::{EvalInput, ModelFunctions};
use std::time::Instant;

const MAX_NEWTON: usize = 30;
const MAX_EVENTS_PER_STEP: usize = 64;
/// An iteration variable this many times its size so far (its largest
/// magnitude at a consistent point, at least its nominal) has run away:
/// its solve fails there instead of calling a step small beside such a
/// value converged. Ten orders of magnitude: a machine of megawatts that
/// starts at rest stays below it, a pedal at 10¹⁵ (the FTP-75's, before)
/// does not.
const RUNAWAY: f64 = 1e10;

/// A consistent point the stepper predicts from.
#[derive(Clone, Debug, Default)]
struct Point {
    t: f64,
    x: Vec<f64>,
    z: Vec<f64>,
    u: Vec<f64>,
    /// x' there
    f: Vec<f64>,
}

impl Point {
    fn new(nx: usize, nz: usize, nu: usize) -> Point {
        Point { t: 0.0, x: vec![0.0; nx], z: vec![0.0; nz], u: vec![0.0; nu], f: vec![0.0; nx] }
    }
}

struct Stepper<'a> {
    m: &'a dyn ModelFunctions,
    p: &'a [f64],
    traces: &'a [Trace],
    o: &'a FastOptions,
    y_names: &'a [String],
    tab: Tableau,
    nx: usize,
    nz: usize,
    ny: usize,
    nu: usize,
    nom: Vec<f64>,
    d: Vec<f64>,
    u: Vec<f64>,
    work: Vec<f64>,
    out: Vec<f64>,
    y: Vec<f64>,
    seg: Vec<usize>,
    /// each trace's breakpoints (the ends of its pieces)
    bps: Vec<Vec<usize>>,
    /// for each input: its trace and derivative order
    inmap: Vec<(usize, u8)>,
    // derivatives
    jac: Vec<f64>,
    schur: Vec<f64>,
    wbuf: Vec<f64>,
    /// the inverse of ∂g/∂z, kept current between refreshes by Broyden updates
    hinv: Vec<f64>,
    /// ∂f/∂z (n_x × n_z): the linear correction of x' after a last Newton step
    fz: Vec<f64>,
    /// g_z⁻¹ g_x (n_z × n_x): how z follows x
    gmat: Vec<f64>,
    /// g_z⁻¹ g_u (n_z × n_u): how z follows the inputs
    gu: Vec<f64>,
    /// the Newton iteration's contraction rate, carried between solves
    theta: f64,
    /// each iteration variable's largest magnitude at a consistent point
    /// so far (at least its nominal): the size a change is judged against
    zmag: Vec<f64>,
    /// a prediction went far off: `G` and `G_u` are stale, refresh them at
    /// the next step's start
    stale_g: bool,
    /// modes held at their values just after the current instant (their
    /// crossings are exactly 0 here, and leave it): (discrete, value)
    held: Vec<(usize, f64)>,
    gz_valid: bool,
    wlu: Vec<(f64, Lu)>,
    jac_age: usize,
    err_ref: f64,
    // scratch
    stage: Vec<Vec<f64>>,
    rhs: Vec<f64>,
    dz: Vec<f64>,
    sz: Vec<f64>,
    g_old: Vec<f64>,
    hy: Vec<f64>,
    jv: Vec<f64>,
    jo: Vec<f64>,
    base: Vec<f64>,
    x1: Vec<f64>,
    err: Vec<f64>,
    /// the consistent stage points of the last step (index 0 unused)
    stage_pts: Vec<Point>,
    rep: FastReport,
}

impl<'a> Stepper<'a> {
    fn set_inputs(&mut self, t: f64) {
        for (j, &(i, order)) in self.inmap.iter().enumerate() {
            let (v, s) = self.traces[i].on_piece(&self.bps[i], self.seg[i], t);
            self.u[j] = match order {
                0 => v,
                1 => s,
                // piecewise linear: no curvature inside a segment
                _ => 0.0,
            };
        }
    }

    /// Sets every mode from its relation at the current point
    /// (`ModelFunctions::modes`, which decides the value exactly at zero;
    /// a mode in [`Self::held`] takes its value just after the instant),
    /// solving for `z` again while a mode changes (a few rounds at most).
    fn settle_modes(&mut self, t: f64) -> Result<(), FastError> {
        if self.d.is_empty() {
            return Ok(());
        }
        let mut d_new = self.d.clone();
        for _ in 0..5 {
            {
                let inp = EvalInput { t, y: &self.y, p: self.p, d: &self.d, u: &self.u };
                self.m.modes(&inp, &mut self.work, &mut d_new);
            }
            for &(i, v) in &self.held {
                d_new[i] = v;
            }
            if d_new == self.d {
                return Ok(());
            }
            self.d.copy_from_slice(&d_new);
            self.gz_valid = false;
            self.solve_z(t)?;
        }
        Ok(())
    }

    fn residual(&mut self, t: f64) {
        let inp = EvalInput { t, y: &self.y, p: self.p, d: &self.d, u: &self.u };
        self.m.residual(&inp, &mut self.work, &mut self.out);
        self.rep.residual_evals += 1;
    }

    fn vars_into(&mut self, t: f64, out: &mut [f64]) {
        let inp = EvalInput { t, y: &self.y, p: self.p, d: &self.d, u: &self.u };
        self.m.vars(&inp, &mut self.work, out);
        self.rep.vars_evals += 1;
    }

    fn roots_into(&mut self, t: f64, out: &mut [f64]) {
        let inp = EvalInput { t, y: &self.y, p: self.p, d: &self.d, u: &self.u };
        self.m.roots(&inp, &mut self.work, out);
    }

    fn y_name(&self, i: usize) -> String {
        self.y_names.get(i).cloned().unwrap_or_else(|| format!("y[{i}]"))
    }

    /// The error for a singular `∂g/∂z`, naming the iteration variables the
    /// equations leave free there: the column the factorisation stopped at
    /// and those it is a combination of.
    fn singular(&self, t: f64, lu: &Lu, block: &[f64], k: usize) -> FastError {
        let nz = self.nz;
        let what = "the algebraic equations' Jacobian with respect to the iteration variables";
        let free = if block.iter().all(|v| v.is_finite()) {
            let v = lu.null_vector(k);
            let big = v.iter().fold(0.0f64, |m, x| m.max(x.abs()));
            (0..nz)
                .filter(|&j| v[j].abs() > 1e-9 * big)
                .map(|j| self.y_name(self.nx + j))
                .collect::<Vec<_>>()
        } else {
            vec![]
        };
        let detail = match free.as_slice() {
            [] => format!(
                ": it holds a value that is not a number (in the column of {})",
                self.y_name(self.nx + k)
            ),
            [one] => format!(": the equations do not determine {one} there"),
            many => {
                format!(": the equations determine only a combination of {} there", many.join(", "))
            }
        };
        FastError::Singular { t, what: what.into(), detail }
    }

    /// The explicit inverse of `∂g/∂z` from its block (column-major).
    fn set_inverse(&mut self, block: &[f64], t: f64) -> Result<(), FastError> {
        let nz = self.nz;
        let mut lu = Lu::new(nz);
        if let Err(crate::lu::Singular(k)) = lu.factor(block) {
            return Err(self.singular(t, &lu, block, k));
        }
        for c in 0..nz {
            let col = &mut self.hinv[c * nz..(c + 1) * nz];
            col.iter_mut().for_each(|v| *v = 0.0);
            col[c] = 1.0;
            lu.solve(col);
        }
        self.gz_valid = true;
        Ok(())
    }

    /// Refreshes `∂g/∂z` (and `∂f/∂z`) at the current point.
    fn refresh_gz(&mut self, t: f64) -> Result<(), FastError> {
        let (nx, nz) = (self.nx, self.nz);
        let mut block = vec![0.0; nz * nz];
        for k in 0..nz {
            self.jv.iter_mut().for_each(|v| *v = 0.0);
            self.jv[nx + k] = 1.0;
            let inp = EvalInput { t, y: &self.y, p: self.p, d: &self.d, u: &self.u };
            self.m.jvp(&inp, &self.jv, &mut self.work, &mut self.jo);
            block[k * nz..(k + 1) * nz].copy_from_slice(&self.jo[nx..]);
            self.fz[k * nx..(k + 1) * nx].copy_from_slice(&self.jo[..nx]);
        }
        self.rep.gz_refreshes += 1;
        self.set_inverse(&block, t)
    }

    /// Predicts `z` at the current `x` (`y[..nx]`) and inputs from a
    /// consistent reference point.
    fn predict(&mut self, r: &Point) {
        let (nx, nz, nu) = (self.nx, self.nz, self.nu);
        for k in 0..nz {
            let mut z = r.z[k];
            for c in 0..nx {
                z -= self.gmat[c * nz + k] * (self.y[c] - r.x[c]);
            }
            for j in 0..nu {
                z -= self.gu[j * nz + k] * (self.u[j] - r.u[j]);
            }
            self.y[nx + k] = z;
        }
    }

    /// Predicts `z` from whichever of `a` and this step's first `n_stages`
    /// stage points is nearest in time to `t`.
    fn predict_near(&mut self, a: &Point, n_stages: usize, t: f64) {
        let mut best = 0usize;
        let mut dist = (t - a.t).abs();
        for i in 1..n_stages {
            let d = (t - self.stage_pts[i].t).abs();
            if d < dist {
                dist = d;
                best = i;
            }
        }
        if best == 0 {
            self.predict(a);
        } else {
            let r = std::mem::take(&mut self.stage_pts[best]);
            self.predict(&r);
            self.stage_pts[best] = r;
        }
    }

    /// `dz = -H g` with `g` the algebraic residuals in `out`.
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

    /// Broyden's ("good") update of the inverse Jacobian after the step
    /// `sz` changed the residual from `g_old` to `out[nx..]`.
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
        // H += (s - H y)(sᵀ H) / (sᵀ H y)
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

    /// Solves `g(t, x, z) = 0` for `z` (x = `y[..nx]`, z starts from
    /// `y[nx..]`); on return `out[..nx]` holds `x'` at the accepted point.
    fn solve_z(&mut self, t: f64) -> Result<(), FastError> {
        self.residual(t);
        if self.nz == 0 {
            return Ok(());
        }
        let (nx, nz) = (self.nx, self.nz);
        if !self.gz_valid {
            self.refresh_gz(t)?;
        }
        let tol = self.o.newton_tol;
        let mut prev: Option<f64> = None;
        let mut refreshes = 0;
        for _ in 0..MAX_NEWTON {
            if self.out[nx..].iter().any(|v| !v.is_finite()) {
                return Err(FastError::NoConvergence {
                    t,
                    what: "the algebraic equations give a value that is not a number".into(),
                });
            }
            self.newton_step();
            let mut norm = 0.0f64;
            // a variable gone off by RUNAWAY: no root here, though the
            // relative test would call a step small beside such a value
            let mut away = false;
            for k in 0..nz {
                let a = self.y[nx + k].abs();
                away |= a > RUNAWAY * self.zmag[k];
                norm = norm.max(self.dz[k].abs() / a.max(self.nom[nx + k]));
            }
            if norm <= tol {
                return if away { Err(self.ran_away(t)) } else { Ok(()) };
            }
            let theta = match prev {
                Some(p) => {
                    let th = norm / p;
                    self.theta = th;
                    th
                }
                None => self.theta.max(0.05),
            };
            // slow (or no) convergence: fresh derivatives (full Newton)
            if prev.is_some() && theta > 0.5 && refreshes < 8 {
                self.refresh_gz(t)?;
                refreshes += 1;
                prev = None;
                continue;
            }
            self.rep.newton_iterations += 1;
            if theta < 0.5 && theta / (1.0 - theta) * norm <= tol {
                // the last step: apply it, correct x' to first order
                for k in 0..nz {
                    let dz = self.dz[k];
                    self.y[nx + k] += dz;
                    if dz != 0.0 {
                        for r in 0..nx {
                            self.out[r] += self.fz[k * nx + r] * dz;
                        }
                    }
                }
                return if away { Err(self.ran_away(t)) } else { Ok(()) };
            }
            // apply, halving the step while the residual is not finite
            self.g_old.copy_from_slice(&self.out[nx..]);
            let mut lambda = 1.0;
            for _ in 0..40 {
                for k in 0..nz {
                    self.y[nx + k] += lambda * self.dz[k];
                }
                self.residual(t);
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
        // the variable whose last step was the largest beside its size
        let k = (0..nz)
            .max_by(|&i, &j| {
                let r = |k: usize| self.dz[k].abs() / self.y[nx + k].abs().max(self.nom[nx + k]);
                r(i).total_cmp(&r(j))
            })
            .unwrap_or(0);
        Err(FastError::NoConvergence {
            t,
            what: format!(
                "the algebraic equations did not converge in {MAX_NEWTON} Newton iterations \
                 ({} the slowest, at {:e})",
                self.y_name(nx + k),
                self.y[nx + k]
            ),
        })
    }

    /// The error for a solve whose iteration variables have run away
    /// (gone off by [`RUNAWAY`]), naming the first that has.
    fn ran_away(&self, t: f64) -> FastError {
        let nx = self.nx;
        let k = (0..self.nz).find(|&k| self.y[nx + k].abs() > RUNAWAY * self.zmag[k]).unwrap_or(0);
        FastError::NoConvergence {
            t,
            what: format!(
                "the algebraic equations' Newton iteration ran away ({} reached {:e})",
                self.y_name(nx + k),
                self.y[nx + k]
            ),
        }
    }

    /// [`Self::solve_z`] from the prediction, kept to the root that
    /// continues from the consistent point `r`: the prediction's
    /// derivatives were taken at another point, and where the equations
    /// switch branches between there and here (the pedal from throttle to
    /// recuperation, a share that fades with the speed) they can put the
    /// first iterate far off, where Newton's method does not come back
    /// from or finds another root. A prediction farther from `r` than twice
    /// the variables' size is dropped for `r`'s own iteration variables; a
    /// solution that far is checked by a solve from them, with fresh
    /// derivatives, and the root nearer `r` kept; a failed solve is tried
    /// again from them, with fresh derivatives.
    fn solve_z_or_from(&mut self, t: f64, r: &Point) -> Result<(), FastError> {
        if self.nz == 0 {
            return self.solve_z(t);
        }
        // a prediction farther from the consistent point than twice the
        // variables' size: its derivatives are stale (taken where the
        // equations behaved otherwise); start from the point itself
        let pred_far = self.far(&r.z);
        if pred_far {
            self.y[self.nx..].copy_from_slice(&r.z);
            self.stale_g = true;
        }
        match self.solve_z(t) {
            Ok(()) if !pred_far && self.far(&r.z) => {
                // a solution that far from the consistent point may be
                // another root of the equations (a battery's power has two
                // currents): solve again from the point itself, with fresh
                // derivatives, and keep the root that continues from it
                let (z1, out1) = (self.y[self.nx..].to_vec(), self.out.clone());
                self.y[self.nx..].copy_from_slice(&r.z);
                self.gz_valid = false;
                self.rep.newton_restarts += 1;
                let again = self.solve_z(t).is_ok();
                if !again || self.distance(&r.z) > dist(&z1, &r.z, &self.zmag) {
                    self.y[self.nx..].copy_from_slice(&z1);
                    self.out.copy_from_slice(&out1);
                    self.gz_valid = false;
                } else if dist(&z1, &self.y[self.nx..], &self.zmag) > 1e-6 {
                    self.stale_g = true;
                }
                Ok(())
            }
            Ok(()) => Ok(()),
            Err(first) => {
                self.y[self.nx..].copy_from_slice(&r.z);
                self.gz_valid = false;
                self.rep.newton_restarts += 1;
                self.solve_z(t).map_err(|_| first)
            }
        }
    }

    /// Whether the iteration variables are farther from `z` than twice
    /// their size: more than any swing within the range they have had so
    /// far (a pedal from full throttle to full brake is 2). Every solve
    /// asks, so it divides nothing.
    fn far(&self, z: &[f64]) -> bool {
        let y = &self.y[self.nx..];
        let (z, m) = (&z[..y.len()], &self.zmag[..y.len()]);
        let mut f = false;
        for k in 0..y.len() {
            f |= (y[k] - z[k]).abs() > 2.0 * m[k];
        }
        f
    }

    /// The iteration variables' distance from `z`, each against its size.
    fn distance(&self, z: &[f64]) -> f64 {
        dist(&self.y[self.nx..], z, &self.zmag)
    }

    /// Refreshes every derivative at the current, consistent point: the
    /// Jacobian, `∂g/∂u` by differences in the inputs, the inverse of
    /// `∂g/∂z`, `G`, `G_u` and the state-space Jacobian `S`.
    fn refresh_jacobian(&mut self, t: f64) -> Result<(), FastError> {
        let (nx, nz, ny, nu) = (self.nx, self.nz, self.ny, self.nu);
        {
            let inp = EvalInput { t, y: &self.y, p: self.p, d: &self.d, u: &self.u };
            self.m.jacobian_dense(&inp, &mut self.work, &mut self.jac);
        }
        self.rep.jacobian_evals += 1;
        // the inputs' effect, by forward differences
        let mut gu_raw = vec![0.0; nz * nu];
        if nz > 0 && nu > 0 {
            self.residual(t);
            self.base.copy_from_slice(&self.out);
            for j in 0..nu {
                let u0 = self.u[j];
                let du = 1e-7 * u0.abs().max(1.0);
                self.u[j] = u0 + du;
                self.residual(t);
                self.u[j] = u0;
                for k in 0..nz {
                    gu_raw[j * nz + k] = (self.out[nx + k] - self.base[nx + k]) / du;
                }
            }
            self.out.copy_from_slice(&self.base);
        }
        let mut block = vec![0.0; nz * nz];
        for c in 0..nz {
            for r in 0..nz {
                block[c * nz + r] = self.jac[(nx + c) * ny + nx + r];
            }
            for r in 0..nx {
                self.fz[c * nx + r] = self.jac[(nx + c) * ny + r];
            }
        }
        if nz > 0 {
            self.set_inverse(&block, t)?;
        }
        // G_u = H g_u, G = H g_x, S = f_x - f_z G
        for j in 0..nu {
            for r in 0..nz {
                let mut v = 0.0;
                for k in 0..nz {
                    v += self.hinv[k * nz + r] * gu_raw[j * nz + k];
                }
                self.gu[j * nz + r] = v;
            }
        }
        for c in 0..nx {
            for r in 0..nz {
                let mut v = 0.0;
                for k in 0..nz {
                    v += self.hinv[k * nz + r] * self.jac[c * ny + nx + k];
                }
                self.gmat[c * nz + r] = v;
            }
            for r in 0..nx {
                let mut v = self.jac[c * ny + r];
                for k in 0..nz {
                    v -= self.jac[(nx + k) * ny + r] * self.gmat[c * nz + k];
                }
                self.schur[c * nx + r] = v;
            }
        }
        self.wlu.clear();
        self.jac_age = 0;
        Ok(())
    }

    /// The LU of `W = I/(γh) - S` for step `h`.
    fn w_index(&mut self, h: f64, t: f64) -> Result<usize, FastError> {
        if let Some(i) = self.wlu.iter().position(|(hh, _)| (hh - h).abs() <= 1e-12 * h) {
            return Ok(i);
        }
        let nx = self.nx;
        let g = 1.0 / (self.tab.gamma * h);
        for c in 0..nx {
            for r in 0..nx {
                self.wbuf[c * nx + r] = if r == c { g } else { 0.0 } - self.schur[c * nx + r];
            }
        }
        let mut lu = Lu::new(nx);
        lu.factor(&self.wbuf).map_err(|_| FastError::Singular {
            t,
            what: "the step matrix (I/(γh) - J) of the states".into(),
            detail: String::new(),
        })?;
        self.rep.lu_factorisations += 1;
        if self.wlu.len() >= 4 {
            self.wlu.remove(0);
        }
        self.wlu.push((h, lu));
        Ok(self.wlu.len() - 1)
    }

    fn scaled_error(&self, x0: &[f64]) -> f64 {
        let n = self.nx;
        if n == 0 || self.tab.s == 1 {
            return 0.0;
        }
        let mut s = 0.0;
        for i in 0..n {
            let w = self.o.atol * self.nom[i] + self.o.rtol * x0[i].abs().max(self.x1[i].abs());
            let r = self.err[i] / w;
            s += r * r;
        }
        (s / n as f64).sqrt()
    }

    /// Stores the current (consistent) point at `t` in `pt`.
    fn capture(&mut self, t: f64, pt: &mut Point) {
        let z = &self.y[self.nx..];
        let m = &mut self.zmag[..z.len()];
        for k in 0..z.len() {
            m[k] = if z[k].abs() > m[k] { z[k].abs() } else { m[k] };
        }
        pt.t = t;
        pt.x.copy_from_slice(&self.y[..self.nx]);
        pt.z.copy_from_slice(&self.y[self.nx..]);
        pt.u.copy_from_slice(&self.u);
        pt.f.copy_from_slice(&self.out[..self.nx]);
    }

    /// One Rosenbrock-W step from the consistent point `a` to `t1`; on
    /// return `b` is the consistent end point (left limit at `t1`).
    fn advance(&mut self, a: &Point, t1: f64, b: &mut Point) -> Result<(), FastError> {
        let (nx, s) = (self.nx, self.tab.s);
        let t0 = a.t;
        let h = t1 - t0;
        let mut redone = false;
        loop {
            if self.stale_g
                || (self.o.max_jacobian_age > 0 && self.jac_age >= self.o.max_jacobian_age)
            {
                self.stale_g = false;
                self.y[..nx].copy_from_slice(&a.x);
                self.y[nx..].copy_from_slice(&a.z);
                self.u.copy_from_slice(&a.u);
                self.refresh_jacobian(t0)?;
                self.err_ref = f64::INFINITY;
            }
            if nx > 0 {
                let wi = self.w_index(h, t0)?;
                self.stage[0].copy_from_slice(&a.f);
                self.wlu[wi].1.solve(&mut self.stage[0]);
                for i in 1..s {
                    for k in 0..nx {
                        let mut v = a.x[k];
                        for j in 0..i {
                            v += self.tab.a[i][j] * self.stage[j][k];
                        }
                        self.y[k] = v;
                    }
                    let ti = t0 + self.tab.alpha[i] * h;
                    self.set_inputs(ti);
                    self.predict_near(a, i, ti);
                    self.solve_z_or_from(ti, a)?;
                    {
                        let mut sp = std::mem::take(&mut self.stage_pts[i]);
                        self.capture(ti, &mut sp);
                        self.stage_pts[i] = sp;
                    }
                    for k in 0..nx {
                        let mut v = self.out[k];
                        for j in 0..i {
                            v += self.tab.c[i][j] / h * self.stage[j][k];
                        }
                        self.rhs[k] = v;
                    }
                    self.wlu[wi].1.solve(&mut self.rhs);
                    self.stage[i].copy_from_slice(&self.rhs);
                }
                for k in 0..nx {
                    let mut v = a.x[k];
                    let mut e = 0.0;
                    for i in 0..s {
                        v += self.tab.m[i] * self.stage[i][k];
                        e += self.tab.e[i] * self.stage[i][k];
                    }
                    self.x1[k] = v;
                    self.err[k] = e;
                }
                if self.x1.iter().any(|v| !v.is_finite()) {
                    return Err(FastError::NoConvergence {
                        t: t1,
                        what: "the step produced a state that is not a number".into(),
                    });
                }
            }
            let e = self.scaled_error(&a.x);
            // a kept Jacobian that has gone bad: refresh it and redo the step once
            if !redone && self.jac_age > 0 && e > 1.0 && e > 4.0 * self.err_ref {
                self.y[..nx].copy_from_slice(&a.x);
                self.y[nx..].copy_from_slice(&a.z);
                self.u.copy_from_slice(&a.u);
                self.refresh_jacobian(t0)?;
                self.rep.step_redos += 1;
                redone = true;
                continue;
            }
            if self.jac_age == 0 {
                self.err_ref = e;
            }
            self.jac_age += 1;
            if e > self.rep.max_error {
                self.rep.max_error = e;
                self.rep.max_error_at = t1;
            }
            break;
        }
        // the end point: z predicted from the last stage (at t1 for a
        // stiffly accurate method) by how z follows x
        let nz = self.nz;
        self.set_inputs(t1);
        if s > 1 && nx > 0 && self.tab.alpha[s - 1] == 1.0 {
            for k in 0..nz {
                let mut dz = 0.0;
                for c in 0..nx {
                    dz += self.gmat[c * nz + k] * (self.x1[c] - self.y[c]);
                }
                self.y[nx + k] -= dz;
            }
            self.y[..nx].copy_from_slice(&self.x1);
        } else {
            self.y[..nx].copy_from_slice(&self.x1);
            self.predict(a);
        }
        self.solve_z_or_from(t1, a)?;
        self.capture(t1, b);
        Ok(())
    }

    /// Makes the stepper's point the consistent Hermite interpolant at
    /// `θ ∈ [0, 1]` of the step from `a` to `b`.
    fn hermite_point(&mut self, a: &Point, b: &Point, th: f64) -> Result<f64, FastError> {
        let nx = self.nx;
        let h = b.t - a.t;
        let (t2, t3) = (th * th, th * th * th);
        let (h00, h10, h01, h11) =
            (2.0 * t3 - 3.0 * t2 + 1.0, t3 - 2.0 * t2 + th, -2.0 * t3 + 3.0 * t2, t3 - t2);
        for k in 0..nx {
            self.y[k] = h00 * a.x[k] + h10 * h * a.f[k] + h01 * b.x[k] + h11 * h * b.f[k];
        }
        let t = a.t + th * h;
        self.set_inputs(t);
        if (b.t - t).abs() < (t - a.t).abs() && self.stage_pts.len() <= 1 {
            self.predict(b);
        } else {
            let n =
                if self.stage_pts.first().is_some_and(|_| self.nx > 0) { self.tab.s } else { 1 };
            self.predict_near(a, n, t);
        }
        let near_b = (b.t - t).abs() < (t - a.t).abs();
        self.solve_z_or_from(t, if near_b { b } else { a })?;
        Ok(t)
    }

    /// Locates where `when` clause `w` fires inside the step from `a` to
    /// `b`, its crossing going from `r0` to `r1` (Illinois method on the
    /// consistent Hermite interpolant); returns θ, the first point found
    /// past it.
    #[allow(clippy::too_many_arguments)]
    fn locate(
        &mut self,
        a: &Point,
        b: &Point,
        w: &WhenInfo,
        r0: f64,
        r1: f64,
        r: &mut [f64],
    ) -> Result<f64, FastError> {
        let c = w.crossing;
        let (mut lo, mut flo, mut hi, mut fhi) = (0.0f64, r0, 1.0f64, r1);
        let mut side = 0i32;
        let h = b.t - a.t;
        // past it: its condition holds (0 is past a crossing to `x >= 0`:
        // one that reaches 0 at the step's end is there; not past one to
        // `x > 0`); in both directions, the start's sign left
        let past = |f: f64| match w.direction {
            Direction::Both => {
                if r0 > 0.0 {
                    f <= 0.0
                } else {
                    f >= 0.0
                }
            }
            _ => holds(w, f),
        };
        for _ in 0..100 {
            if (hi - lo) * h <= 1e-12 * b.t.abs().max(1.0) {
                break;
            }
            let th = (lo * fhi - hi * flo) / (fhi - flo);
            let th = if th.is_finite() && th > lo && th < hi { th } else { 0.5 * (lo + hi) };
            let t = self.hermite_point(a, b, th)?;
            self.roots_into(t, r);
            let ft = r[c];
            if past(ft) {
                hi = th;
                fhi = ft;
                if side == 1 {
                    flo *= 0.5;
                }
                side = 1;
            } else {
                lo = th;
                flo = ft;
                if side == -1 {
                    fhi *= 0.5;
                }
                side = -1;
            }
        }
        Ok(hi)
    }
}

/// The largest distance between `a` and `b`, each entry against `size`.
fn dist(a: &[f64], b: &[f64], size: &[f64]) -> f64 {
    a.iter().zip(b).zip(size).map(|((x, y), m)| (x - y).abs() / m).fold(0.0, f64::max)
}

/// Whether `when` clause `w`'s condition holds where its crossing is `g`
/// (as the forward run loop has it): `x >= 0` (rising, not strict) holds
/// at zero, `x > 0` (strict) does not; a falling clause's condition is
/// `x <= 0` or `x < 0`.
fn holds(w: &WhenInfo, g: f64) -> bool {
    match (w.direction, w.strict) {
        (Direction::Rising | Direction::Both, true) => g > 0.0,
        (Direction::Rising | Direction::Both, false) => g >= 0.0,
        (Direction::Falling, true) => g < 0.0,
        (Direction::Falling, false) => g <= 0.0,
    }
}

/// Whether `when` clause `w` fires as its crossing goes from `r0` to `r1`:
/// its condition changes from false to true (Modelica's `when`); one
/// watched in both directions fires at every change of sign.
fn fires(w: &WhenInfo, r0: f64, r1: f64) -> bool {
    match w.direction {
        Direction::Both => (r0 < 0.0 && r1 >= 0.0) || (r0 > 0.0 && r1 <= 0.0),
        _ => !holds(w, r0) && holds(w, r1),
    }
}

/// A mode its crossing no longer agrees with (as the forward run loop
/// tests it: at zero either value agrees).
fn stale(g: f64, d: f64) -> bool {
    (g > 0.0 && d == 0.0) || (g < 0.0 && d != 0.0)
}

/// The buffers of event mode.
struct EventBuffers {
    /// the roots after each round
    r_now: Vec<f64>,
    /// which `when` clauses fire
    fired: Vec<f64>,
    /// the discrete values the actions set
    d_new: Vec<f64>,
    /// the discrete values before a round
    d_before: Vec<f64>,
}

/// Event mode at `t` (FMI 3.0's, after an input event or a located
/// event's actions): fires each `when` whose condition became true from
/// `r_pre` (its crossing changed sign in its direction), runs the
/// actions, sets the modes from their relations (the iteration variables
/// solved again), and repeats, each round's roots against the last, until
/// no discrete value changes. A crossing in `side` that is exactly 0 here
/// is taken on the side it leaves to (just after the instant: a condition
/// that leaves zero within the next step). On return `r_pre` holds the
/// roots after the event; the result says whether a discrete value
/// changed.
#[allow(clippy::too_many_arguments)]
fn event_mode(
    st: &mut Stepper<'_>,
    whens: &[WhenInfo],
    t: f64,
    r_pre: &mut [f64],
    buf: &mut EventBuffers,
    events: &mut Vec<FastEvent>,
    side: &[(usize, f64)],
) -> Result<bool, FastError> {
    let mut changed = false;
    for _ in 0..MAX_EVENTS_PER_STEP {
        if !r_pre.is_empty() {
            st.roots_into(t, &mut buf.r_now);
            for &(c, s) in side {
                if buf.r_now[c] == 0.0 {
                    buf.r_now[c] = s;
                }
            }
        }
        buf.d_before.copy_from_slice(&st.d);
        let mut any = false;
        for (k, wi) in whens.iter().enumerate() {
            let f = fires(wi, r_pre[wi.crossing], buf.r_now[wi.crossing]);
            buf.fired[k] = if f { 1.0 } else { 0.0 };
            any |= f;
        }
        if any {
            buf.d_new.copy_from_slice(&st.d);
            {
                let inp = EvalInput { t, y: &st.y, p: st.p, d: &st.d, u: &st.u };
                st.m.when(&inp, &buf.fired, &mut st.work, &mut buf.d_new);
            }
            for (k, f) in buf.fired.iter().enumerate() {
                if *f != 0.0 {
                    events.push(FastEvent { t, when: k, label: whens[k].label.clone() });
                }
            }
            st.rep.events += 1;
            if buf.d_new != st.d {
                st.d.copy_from_slice(&buf.d_new);
                st.gz_valid = false;
                st.solve_z(t)?;
            }
        }
        st.settle_modes(t)?;
        r_pre.copy_from_slice(&buf.r_now);
        if st.d == buf.d_before {
            return Ok(changed);
        }
        changed = true;
    }
    Err(FastError::NoConvergence {
        t,
        what: format!(
            "events keep firing (more than {MAX_EVENTS_PER_STEP} rounds of event iteration at \
             one instant)"
        ),
    })
}

/// Each recorded channel's running minimum, maximum and integral over the
/// current output interval.
struct Accumulator<'r> {
    rec: &'r [usize],
    lo: Vec<f64>,
    hi: Vec<f64>,
    int: Vec<f64>,
    t0: f64,
}

impl Accumulator<'_> {
    fn point(&mut self, v: &[f64]) {
        for (j, &i) in self.rec.iter().enumerate() {
            self.lo[j] = self.lo[j].min(v[i]);
            self.hi[j] = self.hi[j].max(v[i]);
        }
    }

    fn reset(&mut self, t0: f64) {
        self.lo.iter_mut().for_each(|v| *v = f64::INFINITY);
        self.hi.iter_mut().for_each(|v| *v = f64::NEG_INFINITY);
        self.int.iter_mut().for_each(|v| *v = 0.0);
        self.t0 = t0;
    }
}

/// The evaluated points of a (sub)step: start (already evaluated), middle
/// and end; feeds flags and statistics.
struct Recording<'s, 't> {
    need_vars: bool,
    stats: bool,
    v_start: Vec<f64>,
    v_mid: Vec<f64>,
    v_end: Vec<f64>,
    acc: Accumulator<'s>,
    tracker: FlagTracker<'t>,
    sites: &'t [LimitSite],
    ders: &'t [Option<DerSrc>],
    lim: Vec<[f64; 3]>,
}

/// Evaluates every limit at a point and hands it to the tracker.
#[allow(clippy::too_many_arguments)]
fn track(
    tracker: &mut FlagTracker<'_>,
    sites: &[LimitSite],
    ders: &[Option<DerSrc>],
    lim: &mut [[f64; 3]],
    st: &Stepper<'_>,
    t: f64,
    vars: &[f64],
    xdot: &[f64],
) {
    if sites.is_empty() {
        return;
    }
    let pt = limits::Point { t, vars, p: st.p, y: &st.y, xdot, ders, model: st.m };
    for (l, s) in lim.iter_mut().zip(sites) {
        *l = pt.site(s);
    }
    tracker.point(t, lim);
}

impl Recording<'_, '_> {
    /// Records the step from `a` to `b` (`v_start` holds the values at `a`).
    fn step(&mut self, st: &mut Stepper<'_>, a: &Point, b: &Point) -> Result<(), FastError> {
        if !self.need_vars {
            return Ok(());
        }
        let h = b.t - a.t;
        let nx = st.nx;
        if self.stats {
            let t = st.hermite_point(a, b, 0.5)?;
            st.vars_into(t, &mut self.v_mid);
            let st: &Stepper<'_> = st;
            track(
                &mut self.tracker,
                self.sites,
                self.ders,
                &mut self.lim,
                st,
                t,
                &self.v_mid,
                &st.out[..nx],
            );
        }
        st.y[..nx].copy_from_slice(&b.x);
        st.y[nx..].copy_from_slice(&b.z);
        st.u.copy_from_slice(&b.u);
        st.vars_into(b.t, &mut self.v_end);
        track(&mut self.tracker, self.sites, self.ders, &mut self.lim, st, b.t, &self.v_end, &b.f);
        let rec = self.acc.rec;
        for (j, &i) in rec.iter().enumerate() {
            let (s, e) = (self.v_start[i], self.v_end[i]);
            let mut lo = s.min(e);
            let mut hi = s.max(e);
            let area = if self.stats {
                let m = self.v_mid[i];
                lo = lo.min(m);
                hi = hi.max(m);
                h * (s + 4.0 * m + e) / 6.0
            } else {
                0.5 * h * (s + e)
            };
            self.acc.lo[j] = self.acc.lo[j].min(lo);
            self.acc.hi[j] = self.acc.hi[j].max(hi);
            self.acc.int[j] += area;
        }
        Ok(())
    }

    /// A new start point (after an event or a slope change) at `t`.
    fn restart(&mut self, st: &mut Stepper<'_>, t: f64) {
        if self.need_vars {
            st.vars_into(t, &mut self.v_start);
            let st: &Stepper<'_> = st;
            track(
                &mut self.tracker,
                self.sites,
                self.ders,
                &mut self.lim,
                st,
                t,
                &self.v_start,
                &st.out[..st.nx],
            );
            self.acc.point(&self.v_start);
        }
    }
}

/// Ticks the blocks due at `t` (all of them, with `init`, when `first`);
/// true if an output changed.
#[allow(clippy::too_many_arguments)]
fn tick(
    st: &mut Stepper<'_>,
    blocks: &mut [BoundBlock],
    next_tick: &mut [f64],
    t: f64,
    first: bool,
    v: &[f64],
    step: f64,
) -> Result<bool, FastError> {
    let mut changed = false;
    let tol = 1e-9 * step.max(1.0);
    for (bi, bb) in blocks.iter_mut().enumerate() {
        if !first && t < next_tick[bi] - tol {
            continue;
        }
        let ins: Vec<f64> = bb.inputs.iter().map(|&i| v[i]).collect();
        let mut outs: Vec<f64> = bb.outputs.iter().map(|&k| st.d[k]).collect();
        let r = if first {
            bb.block.init(t, &ins, &mut outs)
        } else {
            bb.block.tick(t, &ins, &mut outs)
        };
        r.map_err(|e| FastError::Block { name: bb.block.name().to_string(), message: e })?;
        if !first {
            let period = bb.block.period();
            next_tick[bi] = if period > 0.0 {
                let mut n = next_tick[bi];
                while n <= t + tol {
                    n += period;
                }
                n
            } else {
                t
            };
        }
        for (k, val) in bb.outputs.iter().zip(&outs) {
            if st.d[*k] != *val {
                st.d[*k] = *val;
                changed = true;
            }
        }
    }
    Ok(changed)
}

/// Runs fast mode.
pub fn run(prob: &FastProblem<'_>, blocks: &mut [BoundBlock]) -> Result<FastResult, FastError> {
    let started = Instant::now();
    let o = prob.opts;
    let m = prob.model;
    let l = *m.layout();
    let (nx, nz, ny, nu) = (l.n_x, l.n_z, l.n_y(), l.n_u);
    let inmap: Vec<(usize, u8)> = if prob.inputs.is_empty() {
        (0..prob.traces.len()).flat_map(|i| [(i, 0), (i, 1)]).collect()
    } else {
        prob.inputs.to_vec()
    };
    if nu != inmap.len() {
        return Err(FastError::Inputs(format!(
            "the inverse model takes {} inputs (a value and its derivatives per prescribed \
             variable) but {} are filled from the traces",
            nu,
            inmap.len()
        )));
    }
    if let Some((i, _)) = inmap.iter().find(|(i, _)| *i >= prob.traces.len()) {
        return Err(FastError::Inputs(format!(
            "an input is filled from trace {} but {} traces were given",
            i + 1,
            prob.traces.len()
        )));
    }
    if prob.params.len() != l.n_p {
        return Err(FastError::Inputs(format!(
            "the model has {} parameters but {} values were given",
            l.n_p,
            prob.params.len()
        )));
    }
    for tr in prob.traces {
        tr.check()?;
    }
    if !(o.step > 0.0 && o.step.is_finite()) {
        return Err(FastError::Inputs(format!("the step must be positive, not {}", o.step)));
    }
    if prob.traces.is_empty() && (o.t_start.is_none() || o.t_end.is_none()) {
        return Err(FastError::Inputs("without traces, give the start and end times".into()));
    }
    let t_start = o
        .t_start
        .unwrap_or_else(|| prob.traces.iter().map(|t| t.start()).fold(f64::NEG_INFINITY, f64::max));
    let t_end = o
        .t_end
        .unwrap_or_else(|| prob.traces.iter().map(|t| t.end()).fold(f64::INFINITY, f64::min));
    if t_end.partial_cmp(&t_start) != Some(std::cmp::Ordering::Greater) {
        return Err(FastError::Inputs(format!(
            "nothing to run: the traces span [{t_start}, {t_end}] s"
        )));
    }
    let tab = match o.method {
        FixedMethod::RosenbrockW => Tableau::ros34pw2(),
        FixedMethod::LinearImplicitEuler => Tableau::linear_implicit_euler(),
    };

    // the step grid: uniform, split at every breakpoint inside a step
    let bps: Vec<Vec<usize>> = prob.traces.iter().map(|t| t.breakpoints()).collect();
    let n_out = ((t_end - t_start) / o.step - 1e-9).ceil().max(1.0) as usize;
    let grid: Vec<f64> = (0..=n_out).map(|k| (t_start + k as f64 * o.step).min(t_end)).collect();
    let mut bounds: Vec<(f64, bool)> = grid.iter().map(|t| (*t, true)).collect();
    let eps = 1e-9 * o.step;
    for (tr, bp) in prob.traces.iter().zip(&bps) {
        for tk in bp.iter().map(|&k| tr.t[k]) {
            if tk > t_start + eps && tk < t_end - eps {
                let k = ((tk - t_start) / o.step).floor() as usize;
                let near = grid.get(k).is_some_and(|g| (g - tk).abs() <= eps)
                    || grid.get(k + 1).is_some_and(|g| (g - tk).abs() <= eps);
                if !near {
                    bounds.push((tk, false));
                }
            }
        }
    }
    bounds.sort_by(|a, b| a.0.total_cmp(&b.0));
    bounds.dedup_by(|b, a| {
        (b.0 - a.0).abs() <= eps && {
            a.1 |= b.1;
            true
        }
    });

    let nom: Vec<f64> =
        (0..ny).map(|i| prob.y_nominal.get(i).copied().unwrap_or(1.0).abs().max(1e-300)).collect();
    let zmag = nom[nx..].to_vec();
    let mut y0 = vec![0.0; ny];
    let mut d0 = vec![0.0; l.n_d];
    m.start(prob.params, &mut y0, &mut d0);
    let s = tab.s;
    let mut st = Stepper {
        m,
        p: prob.params,
        traces: prob.traces,
        o,
        y_names: prob.y_names,
        tab,
        nx,
        nz,
        ny,
        nu,
        nom,
        d: d0,
        u: vec![0.0; nu],
        work: vec![0.0; l.n_work],
        out: vec![0.0; ny],
        y: y0,
        seg: prob.traces.iter().map(|_| 0).collect(),
        bps,
        inmap,
        jac: vec![0.0; ny * ny],
        schur: vec![0.0; nx * nx],
        wbuf: vec![0.0; nx * nx],
        hinv: vec![0.0; nz * nz],
        fz: vec![0.0; nx * nz],
        gmat: vec![0.0; nz * nx],
        gu: vec![0.0; nz * nu],
        theta: 0.5,
        zmag,
        stale_g: false,
        held: vec![],
        gz_valid: false,
        wlu: vec![],
        jac_age: 0,
        err_ref: f64::INFINITY,
        stage: vec![vec![0.0; nx]; s],
        rhs: vec![0.0; nx],
        dz: vec![0.0; nz],
        sz: vec![0.0; nz],
        g_old: vec![0.0; nz],
        hy: vec![0.0; nz],
        jv: vec![0.0; ny],
        jo: vec![0.0; ny],
        base: vec![0.0; ny],
        x1: vec![0.0; nx],
        err: vec![0.0; nx],
        stage_pts: (0..s).map(|_| Point::new(nx, nz, nu)).collect(),
        rep: FastReport { method: "", n_x: nx, n_z: nz, ..Default::default() },
    };
    st.rep.method = st.tab.name;

    // what to record
    let all: Vec<usize>;
    let rec: &[usize] = match prob.record {
        Some(r) => r,
        None => {
            all = (0..l.n_vars).collect();
            &all
        }
    };
    let nrec = rec.len();
    let names: Vec<String> = rec
        .iter()
        .map(|&i| prob.names.get(i).cloned().unwrap_or_else(|| format!("v[{i}]")))
        .collect();
    let nv = l.n_vars;
    let mut out = Recording {
        need_vars: nrec > 0 || !prob.limits.is_empty() || !blocks.is_empty(),
        stats: o.interval_stats,
        v_start: vec![0.0; nv],
        v_mid: vec![0.0; nv],
        v_end: vec![0.0; nv],
        acc: Accumulator {
            rec,
            lo: vec![f64::INFINITY; nrec],
            hi: vec![f64::NEG_INFINITY; nrec],
            int: vec![0.0; nrec],
            t0: t_start,
        },
        tracker: FlagTracker::new(prob.limits, o.flag_rtol),
        sites: prob.limits,
        ders: prob.ders,
        lim: vec![[0.0; 3]; prob.limits.len()],
    };
    let mut times = Vec::with_capacity(n_out + 1);
    let mut values = Vec::with_capacity((n_out + 1) * nrec);
    let mut vmin = Vec::with_capacity((n_out + 1) * nrec);
    let mut vmax = Vec::with_capacity((n_out + 1) * nrec);
    let mut vmean = Vec::with_capacity((n_out + 1) * nrec);
    let mut events: Vec<FastEvent> = vec![];
    let nr = l.n_roots;
    let mut r_start = vec![0.0; nr];
    let mut r_end = vec![0.0; nr];
    let mut r_tmp = vec![0.0; nr];
    let mut fired = vec![0.0; l.n_whens];
    // crossings exactly 0 at a step's start that leave it: the side
    let mut side: Vec<(usize, f64)> = vec![];
    let mut buf = EventBuffers {
        r_now: vec![0.0; nr],
        fired: vec![0.0; l.n_whens],
        d_new: vec![0.0; l.n_d],
        d_before: vec![0.0; l.n_d],
    };
    let mut next_tick: Vec<f64> = blocks.iter().map(|b| t_start + b.block.offset()).collect();
    let mut a = Point::new(nx, nz, nu);
    let mut b = Point::new(nx, nz, nu);
    let mut e = Point::new(nx, nz, nu);
    let seg_of = |tr: &Trace, bp: &[usize], ta: f64, tb: f64| tr.piece(bp, 0.5 * (ta + tb));
    // each mode's `when` clauses (rising on its crossing sets it, falling
    // on the copy after it clears it: DESIGN.md 5.6), for its events
    let mode_whens: Vec<(usize, usize)> = prob
        .modes
        .iter()
        .map(|md| {
            let find = |c: usize, dir: Direction| {
                prob.whens.iter().position(|w| w.crossing == c && w.direction == dir)
            };
            (
                find(md.crossing, Direction::Rising).unwrap_or(usize::MAX),
                find(md.crossing + 1, Direction::Falling).unwrap_or(usize::MAX),
            )
        })
        .collect();
    let check_modes = nr > 0 && !prob.modes.is_empty();

    // the first point: consistent at the start, the blocks' initial outputs
    let b1 = bounds.get(1).map(|x| x.0).unwrap_or(t_end);
    for (i, tr) in prob.traces.iter().enumerate() {
        st.seg[i] = seg_of(tr, &st.bps[i], t_start, b1);
    }
    st.set_inputs(t_start);
    st.solve_z(t_start)?;
    st.settle_modes(t_start)?;
    if !blocks.is_empty() {
        st.vars_into(t_start, &mut out.v_start);
        if tick(&mut st, blocks, &mut next_tick, t_start, true, &out.v_start, o.step)? {
            st.solve_z(t_start)?;
            // no `when` fires at the start; the modes follow the outputs
            st.settle_modes(t_start)?;
        }
    }
    st.refresh_jacobian(t_start)?;
    st.capture(t_start, &mut a);
    out.restart(&mut st, t_start);
    if nr > 0 {
        st.roots_into(t_start, &mut r_start);
    }
    times.push(t_start);
    for &i in rec {
        let v = out.v_start[i];
        values.push(v);
        vmin.push(v);
        vmax.push(v);
        vmean.push(v);
    }
    out.acc.reset(t_start);

    // a mode found wrong at the end of the last step, to set there
    let mut stale_pending = false;
    for w in 0..bounds.len() - 1 {
        let (ta, tb) = (bounds[w].0, bounds[w + 1].0);
        let is_output = bounds[w + 1].1;
        // a new slope makes a new start point
        let mut new_seg = false;
        for (i, tr) in prob.traces.iter().enumerate() {
            let k = seg_of(tr, &st.bps[i], ta, tb);
            if k != st.seg[i] {
                st.seg[i] = k;
                new_seg = true;
            }
        }
        let due =
            blocks.iter().enumerate().any(|(bi, _)| ta >= next_tick[bi] - 1e-9 * o.step.max(1.0));
        if new_seg || due || stale_pending {
            stale_pending = false;
            // r_start holds the roots just before: the last step's end
            st.y[..nx].copy_from_slice(&a.x);
            st.y[nx..].copy_from_slice(&a.z);
            st.u.copy_from_slice(&a.u);
            st.set_inputs(ta);
            st.predict(&a);
            st.solve_z_or_from(ta, &a)?;
            if due {
                st.vars_into(ta, &mut out.v_start);
                if tick(&mut st, blocks, &mut next_tick, ta, false, &out.v_start, o.step)? {
                    st.rep.block_changes += 1;
                    st.solve_z(ta)?;
                }
            }
            // the input event: the conditions across the jump
            event_mode(&mut st, prob.whens, ta, &mut r_start, &mut buf, &mut events, &[])?;
            st.capture(ta, &mut a);
            out.restart(&mut st, ta);
        }

        // the step, split at events
        let mut n_events = 0;
        // the values just before an event at the step's end (recorded there)
        let mut left: Option<Vec<f64>> = None;
        loop {
            st.advance(&a, tb, &mut b)?;
            let mut te_found: Option<f64> = None;
            if nr > 0 && (check_modes || !prob.whens.is_empty()) {
                st.roots_into(tb, &mut r_end);
            }
            // conditions exactly at zero at the step's start that leave it
            // within the step (no crossing test sees a sign change from 0):
            // a mode whose relation that changes, a strict `when` that
            // becomes true. They change at the start: event mode there,
            // each such crossing taken on the side it leaves to (as just
            // after the instant), then the step again
            side.clear();
            st.held.clear();
            // (most steps have no crossing exactly at zero: one pass to see)
            if r_start.iter().fold(false, |z, &g| z | (g == 0.0)) {
                let leave = |c: usize, up: bool, side: &mut Vec<(usize, f64)>| {
                    if !side.iter().any(|s| s.0 == c) {
                        side.push((c, if up { f64::MIN_POSITIVE } else { -f64::MIN_POSITIVE }));
                    }
                };
                if check_modes {
                    for (mi, md) in prob.modes.iter().enumerate() {
                        let (g0, g1) = (r_start[md.crossing], r_end[md.crossing]);
                        if g0 != 0.0 || !stale(g1, st.d[md.discrete]) {
                            continue;
                        }
                        let on = g1 > 0.0;
                        st.held.push((md.discrete, if on { 1.0 } else { 0.0 }));
                        // the crossing and the copy its falling clause watches
                        for k in [mode_whens[mi].0, mode_whens[mi].1] {
                            if let Some(w) = prob.whens.get(k) {
                                leave(w.crossing, on, &mut side);
                            }
                        }
                        leave(md.crossing, on, &mut side);
                        let k = if on { mode_whens[mi].0 } else { mode_whens[mi].1 };
                        let label = prob.whens.get(k).map_or(md.label.clone(), |w| w.label.clone());
                        events.push(FastEvent { t: a.t, when: k, label });
                    }
                }
                for wi in prob.whens {
                    let (g0, g1) = (r_start[wi.crossing], r_end[wi.crossing]);
                    if g0 == 0.0 && g1 != 0.0 && fires(wi, g0, g1) {
                        leave(wi.crossing, g1 > 0.0, &mut side);
                    }
                }
            }
            if !side.is_empty() {
                n_events += 1;
                if n_events > MAX_EVENTS_PER_STEP {
                    return Err(FastError::NoConvergence {
                        t: a.t,
                        what: "events keep firing (more than 64 in one step)".into(),
                    });
                }
                if !st.held.is_empty() {
                    st.rep.events += 1;
                }
                stale_pending = false;
                let ta = a.t;
                st.y[..nx].copy_from_slice(&a.x);
                st.y[nx..].copy_from_slice(&a.z);
                st.u.copy_from_slice(&a.u);
                for k in 0..st.held.len() {
                    let (i, v) = st.held[k];
                    st.d[i] = v;
                }
                st.gz_valid = false;
                st.solve_z(ta)?;
                // r_start: the roots before the change; the modes that
                // depend on these settle, the `when`s they make true fire
                event_mode(&mut st, prob.whens, ta, &mut r_start, &mut buf, &mut events, &side)?;
                st.held.clear();
                st.capture(ta, &mut a);
                out.restart(&mut st, ta);
                continue;
            }
            if nr > 0 && !prob.whens.is_empty() {
                let mut best: Option<f64> = None;
                for wi in prob.whens {
                    let (r0, r1) = (r_start[wi.crossing], r_end[wi.crossing]);
                    if fires(wi, r0, r1) {
                        let th = st.locate(&a, &b, wi, r0, r1, &mut r_tmp)?;
                        best = Some(best.map_or(th, |x: f64| x.min(th)));
                    }
                }
                if let Some(th) = best {
                    let te = st.hermite_point(&a, &b, th)?;
                    st.roots_into(te, &mut r_tmp);
                    let mut any = false;
                    for (k, wi) in prob.whens.iter().enumerate() {
                        let f = fires(wi, r_start[wi.crossing], r_tmp[wi.crossing]);
                        fired[k] = if f { 1.0 } else { 0.0 };
                        any |= f;
                    }
                    if any {
                        te_found = Some(te);
                    }
                }
            }
            let Some(te) = te_found else {
                // the modes against their crossings at the step's end: one
                // wrong (not from zero: that is above) is set at the end,
                // in event mode at the next step's start
                if check_modes {
                    for md in prob.modes {
                        if stale(r_end[md.crossing], st.d[md.discrete]) {
                            stale_pending = true;
                        }
                    }
                }
                out.step(&mut st, &a, &b)?;
                std::mem::swap(&mut a, &mut b);
                if nr > 0 {
                    r_start.copy_from_slice(&r_end);
                }
                std::mem::swap(&mut out.v_start, &mut out.v_end);
                break;
            };
            n_events += 1;
            if n_events > MAX_EVENTS_PER_STEP {
                return Err(FastError::NoConvergence {
                    t: te,
                    what: "events keep firing (more than 64 in one step)".into(),
                });
            }
            // an event at the step's end ends the step there (no step of
            // no length after it)
            let at_end = tb - te <= 1e-12 * tb.abs().max(1.0);
            let te = if at_end { tb } else { te };
            // redo the step up to the event and record it
            st.advance(&a, te, &mut e)?;
            out.step(&mut st, &a, &e)?;
            // the clauses' actions at te, then the point after them
            st.y[..nx].copy_from_slice(&e.x);
            st.y[nx..].copy_from_slice(&e.z);
            st.u.copy_from_slice(&e.u);
            // the conditions as they stand at the event, before its actions
            st.roots_into(te, &mut r_start);
            buf.d_new.copy_from_slice(&st.d);
            {
                let inp = EvalInput { t: te, y: &st.y, p: st.p, d: &st.d, u: &st.u };
                st.m.when(&inp, &fired, &mut st.work, &mut buf.d_new);
            }
            st.d.copy_from_slice(&buf.d_new);
            for (k, f) in fired.iter().enumerate() {
                if *f != 0.0 {
                    events.push(FastEvent { t: te, when: k, label: prob.whens[k].label.clone() });
                }
            }
            st.rep.events += 1;
            st.gz_valid = false;
            st.solve_z(te)?;
            // the event iteration from there (the modes settled, a `when`
            // the actions make true fired)
            event_mode(&mut st, prob.whens, te, &mut r_start, &mut buf, &mut events, &[])?;
            st.capture(te, &mut a);
            out.restart(&mut st, te);
            if at_end {
                left = Some(out.v_end.clone());
                break;
            }
        }
        st.rep.steps += 1;
        if is_output {
            times.push(tb);
            let len = tb - out.acc.t0;
            let at_tb = left.as_deref().unwrap_or(&out.v_start);
            for (j, &i) in rec.iter().enumerate() {
                let v = at_tb[i];
                values.push(v);
                vmin.push(out.acc.lo[j]);
                vmax.push(out.acc.hi[j]);
                vmean.push(if len > 0.0 { out.acc.int[j] / len } else { v });
            }
            out.acc.reset(tb);
        }
    }
    let flags = out.tracker.finish(t_end);
    Ok(FastResult {
        times,
        names,
        record: rec.to_vec(),
        values,
        min: vmin,
        max: vmax,
        mean: vmean,
        flags,
        events,
        report: st.rep,
        wall_seconds: started.elapsed().as_secs_f64(),
    })
}
