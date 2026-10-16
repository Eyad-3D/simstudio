//! The integrator of a model with no state: no state and no iteration
//! variable, so every variable is a function of time, the parameters, the
//! inputs and the discrete values, which the model evaluates exactly at any
//! time. Nothing is integrated but the energy books; what the run loop
//! needs from an integrator is still there, so such a model runs as any
//! other does (outputs on the grid, events and `when`s on time and on
//! algebraic conditions, sampled blocks, energy books) and its result has
//! the same shape.
//!
//! * **Steps** go to the run loop's stop time, halved until they follow
//!   the channels as an integrator's steps follow its states: a step is
//!   accepted when the polynomial of degree 4 through every channel at its
//!   ends and quarter points gives the channel at an eighth of the step
//!   from either end to the run's tolerance (the error a fifth-order
//!   method's estimate would see), and the next may be twice as long. The
//!   recorder's min, max and mean read the steps' ends, as for any model;
//!   the values on the output grid are the model's own at those times.
//! * **Root finding**: the zero-crossing functions the run loop does not
//!   schedule itself (it schedules time crossings and the functions of time
//!   it can search ahead) are checked at each step's end, in the directions
//!   the model gives, an exact zero counting as the side the run loop sets;
//!   a sign change is located by bisection on the model's own functions
//!   (exact at any time: there is no interpolant) to SUNDIALS' tolerance,
//!   `100 ε (|t| + |h|)`, and the step ends just past it, as there.
//! * **Energy books**: integrated by adaptive Gauss–Kronrod (7 and 15
//!   points) over each step, each piece accepted when the two rules agree
//!   to the run's tolerance (as SUNDIALS' quadratures hold theirs), or one
//!   15-point rule per step without error control
//!   ([`SolverOptions::energy_error_control`]); inside a step, from the
//!   last point read.

use crate::energy::Integrand;
use crate::{
    DenseOutput, Integrator, OutputGrid, RunInfo, SolveError, SolverOptions, SolverStats, Step,
};
use lsim_ir::runtime::{EvalInput, ModelFunctions};

/// Kronrod's 15 nodes on [-1, 1] (the positive half and 0) and their
/// weights; the odd ones are Gauss's 7 nodes, with `GAUSS7` (QUADPACK's
/// `qk15`).
const KRONROD_X: [f64; 8] = [
    0.991_455_371_120_812_6,
    0.949_107_912_342_758_5,
    0.864_864_423_359_769_1,
    0.741_531_185_599_394_4,
    0.586_087_235_467_691_1,
    0.405_845_151_377_397_2,
    0.207_784_955_007_898_5,
    0.0,
];
const KRONROD_W: [f64; 8] = [
    0.022_935_322_010_529_225,
    0.063_092_092_629_978_55,
    0.104_790_010_322_250_18,
    0.140_653_259_715_525_92,
    0.169_004_726_639_267_9,
    0.190_350_578_064_785_4,
    0.204_432_940_075_298_9,
    0.209_482_141_084_727_83,
];
const GAUSS7: [f64; 4] = [
    0.129_484_966_168_869_7,
    0.279_705_391_489_276_7,
    0.381_830_050_505_118_9,
    0.417_959_183_673_469_4,
];

/// The most pieces one step's integrals are split into.
const MOST_PIECES: usize = 4096;

/// Where a step's channels are read inside it, as shares of the step: the
/// quarter points, then an eighth from either end.
const INNER: [f64; 5] = [0.25, 0.5, 0.75, 0.125, 0.875];

/// The weights at `s` of the polynomial through `nodes` (Lagrange's).
fn lagrange(nodes: &[f64; 5], s: f64) -> [f64; 5] {
    let mut w = [1.0; 5];
    for (i, wi) in w.iter_mut().enumerate() {
        for (j, xj) in nodes.iter().enumerate() {
            if i != j {
                *wi *= (s - xj) / (nodes[i] - xj);
            }
        }
    }
    w
}

/// The integrator of a model with no state (see the module documentation).
pub struct Stateless<'m> {
    model: &'m dyn ModelFunctions,
    p: Vec<f64>,
    d: Vec<f64>,
    u: Vec<f64>,
    work: Vec<f64>,
    rtol: f64,
    atol: f64,
    max_step: f64,
    error_control: bool,
    /// the last step's start and end
    t0: f64,
    t: f64,
    /// no step since the start or the last restart
    fresh: bool,
    /// the step the next one tries
    h_next: f64,
    /// the channels at the step's end, and inside it (at the points of
    /// `INNER`)
    v_end: Vec<f64>,
    v_in: Vec<f64>,
    /// the weights of the polynomial through the ends and quarter points
    /// at an eighth of the step from its start
    eighth: [f64; 5],
    /// the discrete values the channels and the root functions at the
    /// step's end were evaluated with (a tick that changed only what
    /// nothing integrated reads goes on without a restart), and those the
    /// last step was taken with (its integrals inside it read them, also
    /// after an event changed the discrete values there)
    at_d: Vec<f64>,
    step_d: Vec<f64>,
    /// the root functions (the model's, then the table guards) at the
    /// last step's end, mapped as the run loop asks; their directions, the
    /// sides of exact zeros, the ones the run loop schedules itself
    g: Vec<f64>,
    g_try: Vec<f64>,
    root_dirs: Vec<i32>,
    zero_side: Vec<f64>,
    zero_hold: Vec<f64>,
    root_mask: Vec<bool>,
    /// the energy integrand, and the integrals at the step's start, at its
    /// end and at the last point read inside it
    quad: Option<Integrand>,
    q0: Vec<f64>,
    q1: Vec<f64>,
    q_at: (f64, Vec<f64>),
    f_buf: Vec<f64>,
    empty: Vec<f64>,
    stats: SolverStats,
}

impl<'m> Stateless<'m> {
    /// Sets up the run at the grid's start with the discrete values `d0`,
    /// the inputs `u` and the energy integrand `quad`.
    pub fn new(
        model: &'m dyn ModelFunctions,
        info: &RunInfo,
        opts: &SolverOptions,
        grid: OutputGrid,
        d0: Vec<f64>,
        u: Vec<f64>,
        quad: Option<Integrand>,
    ) -> Self {
        let l = *model.layout();
        debug_assert_eq!(l.n_y(), 0, "a model with no state");
        let n_roots = l.n_roots + model.table_guard_list().len();
        let n_q = quad.as_ref().map(|q| q.len()).unwrap_or(0);
        let t0 = grid.t0;
        Stateless {
            model,
            p: info.params.clone(),
            d: d0,
            u,
            work: vec![0.0; l.n_work],
            rtol: opts.rtol,
            atol: opts.atol,
            max_step: opts.max_step,
            error_control: opts.energy_error_control,
            t0,
            t: t0,
            fresh: true,
            h_next: (grid.t_end - t0).abs(),
            v_end: vec![0.0; l.n_vars],
            v_in: vec![0.0; INNER.len() * l.n_vars],
            eighth: lagrange(&[0.0, 0.25, 0.5, 0.75, 1.0], 0.125),
            at_d: vec![],
            step_d: vec![],
            g: vec![0.0; n_roots],
            g_try: vec![0.0; n_roots],
            root_dirs: info
                .root_dirs
                .iter()
                .copied()
                .chain(std::iter::repeat(0))
                .take(n_roots)
                .collect(),
            zero_side: vec![0.0; n_roots],
            zero_hold: vec![0.0; n_roots],
            root_mask: vec![false; n_roots],
            quad,
            q0: vec![0.0; n_q],
            q1: vec![0.0; n_q],
            q_at: (t0, vec![0.0; n_q]),
            f_buf: vec![0.0; n_q],
            empty: vec![],
            stats: SolverStats::default(),
        }
    }

    /// The channels at `t` into the step's end (`at` None) or its quarter
    /// point `at`.
    fn channels_at(&mut self, t: f64, at: Option<usize>) {
        let inp = EvalInput { t, y: &[], p: &self.p, d: &self.d, u: &self.u };
        let n = self.v_end.len();
        let out = match at {
            None => &mut self.v_end[..],
            Some(k) => &mut self.v_in[k * n..(k + 1) * n],
        };
        self.model.vars(&inp, &mut self.work, out);
        self.stats.rhs_evals += 1;
    }

    /// The root functions at `t`, mapped as the backends map them (exact
    /// zeros to their sides, the scheduled ones held), into `g_try`.
    fn roots_at(&mut self, t: f64) {
        let nr = self.model.layout().n_roots;
        let inp = EvalInput { t, y: &[], p: &self.p, d: &self.d, u: &self.u };
        self.model.roots(&inp, &mut self.work, &mut self.g_try[..nr]);
        if self.g_try.len() > nr {
            self.model.table_guards(&inp, &mut self.work, &mut self.g_try[nr..]);
        }
        crate::run::apply_zero_sides(
            &mut self.g_try,
            &self.zero_side,
            &self.zero_hold,
            &self.root_mask,
        );
    }

    /// The direction root function `i` crossed in from `a` to `b` (0: it
    /// did not, or not in the direction the model watches it in).
    fn crossed(&self, i: usize, a: f64, b: f64) -> i32 {
        let dir = if a < 0.0 && b >= 0.0 {
            1
        } else if a > 0.0 && b <= 0.0 {
            -1
        } else {
            return 0;
        };
        let want = self.root_dirs[i];
        if want == 0 || want == dir { dir } else { 0 }
    }

    /// The first crossing in `(t, te]` of the root functions (`g` at `t`,
    /// `g_try` at `te` on entry), to SUNDIALS' tolerance: its time, just
    /// past it, and each function's direction there; `None` when none
    /// crossed.
    fn first_crossing(&mut self, t: f64, te: f64) -> Option<(f64, Vec<i32>)> {
        let any = |s: &Self, lo: &[f64], hi: &[f64]| {
            (0..lo.len()).any(|i| s.crossed(i, lo[i], hi[i]) != 0)
        };
        if self.g.is_empty() || !any(self, &self.g, &self.g_try) {
            return None;
        }
        let ttol = 100.0 * f64::EPSILON * (t.abs() + (te - t).abs());
        let (mut lo, mut hi) = (t, te);
        let mut g_lo = self.g.clone();
        let mut g_hi = self.g_try.clone();
        while hi - lo > ttol {
            let mid = 0.5 * (lo + hi);
            if mid <= lo || mid >= hi {
                break;
            }
            self.roots_at(mid);
            if any(self, &g_lo, &self.g_try) {
                hi = mid;
                g_hi.copy_from_slice(&self.g_try);
            } else {
                lo = mid;
                g_lo.copy_from_slice(&self.g_try);
            }
        }
        let dirs = (0..g_lo.len()).map(|i| self.crossed(i, g_lo[i], g_hi[i])).collect();
        self.g_try.copy_from_slice(&g_hi);
        Some((hi, dirs))
    }

    /// The integrands at `t` into `f_buf`, with the discrete values of the
    /// last step.
    fn integrand(&mut self, t: f64) {
        let Some(q) = self.quad.as_mut() else { return };
        let inp = EvalInput { t, y: &[], p: &self.p, d: &self.step_d, u: &self.u };
        q.eval(self.model, &inp, &[], &mut self.work, &mut self.f_buf);
    }

    /// The 15-point rule on `[a, b]`, the 7-point one, and the 15-point
    /// rule of the integrands' magnitudes, added to `k`, `g`, `abs`.
    fn kronrod(&mut self, a: f64, b: f64, k: &mut [f64], g: &mut [f64], abs: &mut [f64]) {
        let (m, r) = (0.5 * (a + b), 0.5 * (b - a));
        for j in 0..KRONROD_X.len() {
            let x = KRONROD_X[j];
            let sides: &[f64] = if x == 0.0 { &[0.0] } else { &[-1.0, 1.0] };
            for s in sides {
                self.integrand(m + s * x * r);
                // the Gauss nodes are Kronrod's odd ones (the last the middle)
                let wg = if j % 2 == 1 { GAUSS7[j / 2] } else { 0.0 };
                for (c, f) in self.f_buf.iter().enumerate() {
                    k[c] += KRONROD_W[j] * f * r;
                    g[c] += wg * f * r;
                    abs[c] += KRONROD_W[j] * f.abs() * r;
                }
            }
        }
    }

    /// The integrals over `[a, b]` (inside the last step), added to `out`:
    /// split until each piece's two rules agree to the tolerance its share
    /// of the interval allows, or one rule without error control.
    fn integrate(&mut self, a: f64, b: f64, out: &mut [f64]) {
        let n = out.len();
        if n == 0 || b <= a {
            return;
        }
        let span = b - a;
        let mut pieces = vec![(a, b)];
        let (mut k, mut g, mut abs) = (vec![0.0; n], vec![0.0; n], vec![0.0; n]);
        let mut count = 0usize;
        while let Some((lo, hi)) = pieces.pop() {
            k.fill(0.0);
            g.fill(0.0);
            abs.fill(0.0);
            self.kronrod(lo, hi, &mut k, &mut g, &mut abs);
            count += 1;
            let share = (hi - lo) / span;
            let good = (0..n).all(|c| {
                let err = (k[c] - g[c]).abs();
                !err.is_finite() || err <= self.rtol * abs[c] + self.atol * share
            });
            let mid = 0.5 * (lo + hi);
            let splits = self.error_control && !good && count + pieces.len() < MOST_PIECES;
            if splits && mid > lo && mid < hi {
                // (the later half first on the stack: pieces in time order)
                pieces.push((mid, hi));
                pieces.push((lo, mid));
            } else {
                for c in 0..n {
                    out[c] += k[c];
                }
            }
        }
    }
}

impl Integrator for Stateless<'_> {
    fn name(&self) -> &'static str {
        "none (the model has no state)"
    }

    fn step(&mut self, t_stop: f64) -> Result<Step, SolveError> {
        let t = self.t;
        if t_stop <= t {
            return Ok(Step::Stopped(t));
        }
        if self.fresh || self.at_d != self.d {
            // the channels and the root functions at the start, just after
            // a restart, or with the discrete values a tick changed
            self.channels_at(t, None);
            self.roots_at(t);
            self.g.copy_from_slice(&self.g_try);
            self.at_d.clone_from(&self.d);
        }
        let mut h = self.h_next.min(t_stop - t);
        if self.max_step > 0.0 {
            h = h.min(self.max_step);
        }
        let v_start = self.v_end.clone();
        let n = v_start.len();
        // the longest step whose channels the polynomial through its ends
        // and quarter points gives at its eighths, to the tolerance (a
        // channel that is not finite has nothing to resolve)
        let te = loop {
            let te = if h >= t_stop - t { t_stop } else { t + h };
            self.channels_at(te, None);
            for (k, s) in INNER.iter().enumerate() {
                self.channels_at(t + s * (te - t), Some(k));
            }
            let w = self.eighth;
            let v = &self.v_in;
            let resolved = (0..n).all(|c| {
                let (a, q1, q2, q3, b) = (v_start[c], v[c], v[n + c], v[2 * n + c], self.v_end[c]);
                let (e1, e7) = (v[3 * n + c], v[4 * n + c]);
                // (at 7/8 the weights run the other way)
                let p1 = w[0] * a + w[1] * q1 + w[2] * q2 + w[3] * q3 + w[4] * b;
                let p7 = w[4] * a + w[3] * q1 + w[2] * q2 + w[1] * q3 + w[0] * b;
                let size = [a, q1, q2, q3, b, e1, e7].iter().fold(0.0f64, |m, x| m.max(x.abs()));
                let gap = (e1 - p1).abs().max((e7 - p7).abs());
                !gap.is_finite() || gap <= self.rtol * size + self.atol
            });
            let shortest = 16.0 * f64::EPSILON * t.abs().max(te.abs()).max(f64::MIN_POSITIVE);
            if resolved || te - t <= shortest {
                break te;
            }
            h = 0.5 * (te - t);
        };
        self.roots_at(te);
        let root = self.first_crossing(t, te);
        let end = root.as_ref().map_or(te, |(tr, _)| *tr);
        if end != te {
            self.channels_at(end, None);
        }
        // the integrals over the step, from those at its start
        self.step_d.clone_from(&self.d);
        if !self.fresh {
            std::mem::swap(&mut self.q0, &mut self.q1);
        }
        let mut q = self.q0.clone();
        self.integrate(t, end, &mut q);
        self.q1.copy_from_slice(&q);
        self.q_at = (t, self.q0.clone());
        self.g.copy_from_slice(&self.g_try);
        self.t0 = t;
        self.t = end;
        self.fresh = false;
        self.h_next = 2.0 * (end - t);
        self.stats.steps += 1;
        Ok(match root {
            Some((tr, dirs)) => Step::Root(tr, dirs),
            None if end == t_stop => Step::Stopped(end),
            None => Step::Internal(end),
        })
    }

    fn y(&self) -> &[f64] {
        &self.empty
    }

    fn interpolate(&mut self, _t: f64, _out: &mut [f64]) -> Result<(), SolveError> {
        Ok(())
    }

    fn interpolate_select(
        &mut self,
        _t: f64,
        _idx: &[usize],
        _out: &mut [f64],
    ) -> Result<(), SolveError> {
        Ok(())
    }

    fn interpolate_select_many(
        &mut self,
        _ts: &[f64],
        _idx: &[usize],
        _out: &mut [f64],
    ) -> Result<(), SolveError> {
        Ok(())
    }

    fn dense_output(
        &mut self,
        _t0: f64,
        _idx: &[usize],
        out: &mut DenseOutput,
    ) -> Result<bool, SolveError> {
        out.constant(self.t, std::iter::empty());
        Ok(true)
    }

    fn discrete_mut(&mut self) -> &mut [f64] {
        &mut self.d
    }

    fn restart(&mut self, t: f64, _y: &[f64]) -> Result<(), SolveError> {
        // the integrals at t become the start's
        let mut q = vec![0.0; self.q0.len()];
        self.quadrature(t, &mut q)?;
        self.q0.copy_from_slice(&q);
        self.q_at = (t, q);
        self.t0 = t;
        self.t = t;
        self.fresh = true;
        self.stats.restarts += 1;
        Ok(())
    }

    fn resume(&mut self, t: f64) -> bool {
        // (nothing is kept between steps but the integrals and the root
        // functions' signs, which a slight change leaves as they are)
        !self.fresh && t == self.t
    }

    fn consistent_z(&mut self, _t: f64, _y: &mut [f64], _d: &[f64]) -> Result<(), SolveError> {
        Ok(())
    }

    fn stats(&self) -> SolverStats {
        self.stats
    }

    fn quadrature(&mut self, t: f64, out: &mut [f64]) -> Result<(), SolveError> {
        if out.is_empty() {
            return Ok(());
        }
        if self.fresh || t <= self.t0 {
            out.copy_from_slice(&self.q0);
            return Ok(());
        }
        if t >= self.t {
            out.copy_from_slice(&self.q1);
            return Ok(());
        }
        // from the last point read inside the step when it lies before t
        let (from, mut q) = if self.q_at.0 <= t {
            (self.q_at.0, self.q_at.1.clone())
        } else {
            (self.t0, self.q0.clone())
        };
        self.integrate(from, t, &mut q);
        out.copy_from_slice(&q);
        self.q_at = (t, q);
        Ok(())
    }

    fn set_root_sides(&mut self, sides: &[f64], holds: &[f64]) {
        self.zero_side.copy_from_slice(sides);
        self.zero_hold.copy_from_slice(holds);
        // (the signs at the step's end as the new sides map them)
        if !self.fresh {
            let t = self.t;
            self.roots_at(t);
            self.g.copy_from_slice(&self.g_try);
        }
    }

    fn set_root_mask(&mut self, mask: &[bool]) -> bool {
        for (m, x) in self.root_mask.iter_mut().zip(mask) {
            *m = *x;
        }
        true
    }

    fn method(&self) -> String {
        "none (no state to integrate)".into()
    }

    fn setup_notes(&self) -> Vec<String> {
        vec![
            "the model has no state: nothing is integrated; the steps resolve the channels, the \
             conditions are found on the model's own functions and the energy books by \
             Gauss-Kronrod quadrature"
                .into(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rule's constants: the 15-point rule is exact for polynomials of
    /// degree 22, the 7-point one for degree 13 (odd degrees vanish by
    /// symmetry), and both weights sum to 2.
    #[test]
    fn the_kronrod_rule_is_exact_on_polynomials() {
        // ∫_{-1}^{1} x^k over the nodes (each but the middle twice)
        let rule = |w: &[f64], x: &[f64], k: i32| -> f64 {
            w.iter()
                .zip(x)
                .map(|(w, x)| {
                    if *x == 0.0 {
                        w * lsim_ir::math::powi(*x, k)
                    } else {
                        2.0 * w * lsim_ir::math::powi(*x, k)
                    }
                })
                .sum()
        };
        let gx = [KRONROD_X[1], KRONROD_X[3], KRONROD_X[5], KRONROD_X[7]];
        for k in (0..=22).step_by(2) {
            let exact = 2.0 / (k + 1) as f64;
            let kr = rule(&KRONROD_W, &KRONROD_X, k);
            assert!((kr - exact).abs() < 4e-16, "Kronrod, degree {k}: {kr} against {exact}");
            if k <= 13 {
                let g = rule(&GAUSS7, &gx, k);
                assert!((g - exact).abs() < 4e-16, "Gauss, degree {k}: {g} against {exact}");
            }
        }
    }
}
