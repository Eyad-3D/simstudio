//! The test problem and its exact answer.
//!
//! A battery (constant open-circuit voltage `E`, series resistance `R0`, one
//! RC pair `R1 || C1`) feeds an ideal DC machine (back-EMF and torque
//! constant `k`) that spins an inertia `J` with viscous loss `b`. When the
//! speed first reaches `W_ON` a brake clamps on with torque `TB` (a state
//! event). The RC pair is fast (about 0.1 ms) and the mechanics slow (about
//! 1 s), so the problem is stiff (ratio about 1e4).
//!
//! As an index-1 DAE in (v1, w, i):
//!
//! ```text
//!   C1 v1' = i - v1 / R1
//!   J  w'  = k i - b w - TB s          (s = 0 before the event, 1 after)
//!   0      = E - R0 i - v1 - k w        (algebraic: the loop's KVL)
//! ```
//!
//! Eliminating `i` gives a linear ODE `x' = A x + c(s)` in x = (v1, w),
//! whose exact solution is `x_eq + exp(A t) (x0 - x_eq)`; the event time is
//! found by Newton iteration on that closed form, to rounding.

pub const E: f64 = 400.0; // V
pub const R0: f64 = 0.05; // ohm
pub const R1: f64 = 0.01; // ohm
pub const C1: f64 = 0.01; // F   (fast pole: (1/R0 + 1/R1) / C1 = 12 000 1/s)
pub const K: f64 = 1.0; // V s/rad = N m/A
pub const J: f64 = 20.0; // kg m^2
pub const B: f64 = 0.05; // N m s/rad
pub const TB: f64 = 2000.0; // N m, brake torque once applied
pub const W_ON: f64 = 300.0; // rad/s, the speed at which the brake applies
pub const T_END: f64 = 4.0; // s

/// The ODE right-hand side in x = (v1, w), with the brake state `s`.
pub fn ode_rhs(x: &[f64], s: f64, dx: &mut [f64]) {
    let i = (E - x[0] - K * x[1]) / R0;
    dx[0] = (i - x[0] / R1) / C1;
    dx[1] = (K * i - B * x[1] - TB * s) / J;
}

/// The ODE Jacobian, row-major 2x2.
pub fn ode_jac() -> [[f64; 2]; 2] {
    let a = 1.0 / R0;
    [
        [-(a + 1.0 / R1) / C1, -a * K / C1],
        [-K * a / J, -(K * K * a + B) / J],
    ]
}

/// Current from the algebraic equation.
pub fn current(v1: f64, w: f64) -> f64 {
    (E - v1 - K * w) / R0
}

/// Closed-form solution of x' = A x + c on one segment.
#[derive(Clone, Copy, Debug)]
pub struct Segment {
    pub t0: f64,
    pub x0: [f64; 2],
    pub xeq: [f64; 2],
    pub l1: f64,
    pub l2: f64,
    pub a: [[f64; 2]; 2],
}

impl Segment {
    pub fn new(t0: f64, x0: [f64; 2], s: f64) -> Self {
        let a = ode_jac();
        let c = [E / R0 / C1, (K * E / R0 - TB * s) / J];
        // x_eq = -A^-1 c
        let det = a[0][0] * a[1][1] - a[0][1] * a[1][0];
        let xeq = [
            -(a[1][1] * c[0] - a[0][1] * c[1]) / det,
            -(-a[1][0] * c[0] + a[0][0] * c[1]) / det,
        ];
        let tr = a[0][0] + a[1][1];
        let disc = (0.25 * tr * tr - det).sqrt();
        // stable roots of l^2 - tr l + det = 0
        let l1 = 0.5 * tr - disc; // the large negative (fast) one
        let l2 = det / l1;
        Segment {
            t0,
            x0,
            xeq,
            l1,
            l2,
            a,
        }
    }

    /// exp(A tau) applied to d, via exp(At) = p(t) I + q(t) A
    fn expm_apply(&self, tau: f64, d: [f64; 2]) -> [f64; 2] {
        let (l1, l2) = (self.l1, self.l2);
        let e1 = (l1 * tau).exp();
        let e2 = (l2 * tau).exp();
        let q = (e1 - e2) / (l1 - l2);
        let p = (l1 * e2 - l2 * e1) / (l1 - l2);
        let a = self.a;
        [
            p * d[0] + q * (a[0][0] * d[0] + a[0][1] * d[1]),
            p * d[1] + q * (a[1][0] * d[0] + a[1][1] * d[1]),
        ]
    }

    pub fn at(&self, t: f64) -> [f64; 2] {
        let d = [self.x0[0] - self.xeq[0], self.x0[1] - self.xeq[1]];
        let e = self.expm_apply(t - self.t0, d);
        [self.xeq[0] + e[0], self.xeq[1] + e[1]]
    }

    pub fn deriv(&self, t: f64) -> [f64; 2] {
        let x = self.at(t);
        let a = self.a;
        let c0 = -(a[0][0] * self.xeq[0] + a[0][1] * self.xeq[1]);
        let c1 = -(a[1][0] * self.xeq[0] + a[1][1] * self.xeq[1]);
        [
            a[0][0] * x[0] + a[0][1] * x[1] + c0,
            a[1][0] * x[0] + a[1][1] * x[1] + c1,
        ]
    }
}

/// The exact answer: two segments split at the brake event.
pub struct Exact {
    pub before: Segment,
    pub after: Segment,
    pub t_event: f64,
}

impl Exact {
    pub fn new() -> Self {
        let before = Segment::new(0.0, [0.0, 0.0], 0.0);
        // Newton on w(t) = W_ON, starting from the slow-pole estimate
        let mut t = -(1.0 - W_ON / before.xeq[1]).ln() / (-before.l2);
        for _ in 0..50 {
            let f = before.at(t)[1] - W_ON;
            let df = before.deriv(t)[1];
            let dt = f / df;
            t -= dt;
            if dt.abs() < 1e-16 * t.abs() {
                break;
            }
        }
        let x_e = before.at(t);
        let after = Segment::new(t, [x_e[0], W_ON], 1.0);
        Exact {
            before,
            after,
            t_event: t,
        }
    }

    /// (v1, w, i) at t
    pub fn at(&self, t: f64) -> [f64; 3] {
        let x = if t <= self.t_event {
            self.before.at(t)
        } else {
            self.after.at(t)
        };
        [x[0], x[1], current(x[0], x[1])]
    }
}

impl Default for Exact {
    fn default() -> Self {
        Self::new()
    }
}

/// The times results are compared at (dense output): every 10 ms.
pub fn sample_times() -> Vec<f64> {
    (1..=400).map(|k| k as f64 * 0.01).collect()
}

/// What one solve gave.
#[derive(Debug, Default, Clone)]
pub struct RunStats {
    pub t_event: f64,
    pub samples: Vec<[f64; 3]>,
    pub steps: u64,
    pub rhs_evals: u64,
    pub jac_evals: u64,
    pub err_test_fails: u64,
}

/// Largest error over the samples, relative to each variable's largest
/// magnitude (v1: ~20 V, w: ~400 rad/s, i: ~8000 A).
pub fn max_rel_error(exact: &Exact, stats: &RunStats) -> [f64; 3] {
    let times = sample_times();
    let mut scale = [0.0f64; 3];
    let mut err = [0.0f64; 3];
    for (k, &t) in times.iter().enumerate() {
        let ex = exact.at(t);
        for j in 0..3 {
            scale[j] = scale[j].max(ex[j].abs());
            err[j] = err[j].max((stats.samples[k][j] - ex[j]).abs());
        }
    }
    [err[0] / scale[0], err[1] / scale[1], err[2] / scale[2]]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_solution_satisfies_the_ode() {
        let ex = Exact::new();
        for &t in &[0.0, 1e-4, 0.3, ex.t_event - 1e-9] {
            let x = ex.before.at(t);
            let mut dx = [0.0; 2];
            ode_rhs(&x, 0.0, &mut dx);
            let d = ex.before.deriv(t);
            assert!((dx[0] - d[0]).abs() < 1e-6 * d[0].abs().max(1.0));
            assert!((dx[1] - d[1]).abs() < 1e-9 * d[1].abs().max(1.0));
        }
        assert!((ex.before.at(ex.t_event)[1] - W_ON).abs() < 1e-12);
        // finite-difference check of the closed form
        let h = 1e-6;
        let t = 0.5;
        let fd = (ex.before.at(t + h)[1] - ex.before.at(t - h)[1]) / (2.0 * h);
        assert!((fd - ex.before.deriv(t)[1]).abs() < 1e-6);
    }
}
