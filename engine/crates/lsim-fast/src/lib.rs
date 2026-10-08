//! # lsim-fast: fast mode (inverse model)
//!
//! A drive-cycle case can run in two modes, the user's choice:
//!
//! * **full dynamic** — the driver block closes the loop and the model is
//!   integrated forward (lsim-solve);
//! * **fast** — the *same* equations solved backwards: the vehicle speed
//!   follows the cycle exactly (a prescribed trajectory), the driver block
//!   is removed and its commands become unknowns, and the model is stepped
//!   at the cycle's own spacing (about 1 s) with a fixed-step, L-stable
//!   implicit method. Every `limit(x, lo, hi)` in the model (motor torque
//!   at speed, battery power, brake capacity) passes its value through and
//!   *flags* the moments it is outside its band instead of clamping: fast
//!   mode never falls back to forward simulation, it reports where the
//!   vehicle could not have followed.
//!
//! The inverse model is prepared by lsim-prep from an [`InverseSpec`]
//! (the prescribed speed and its derivative become known inputs, the
//! driver's commands unknowns; index reduction differentiates what the
//! prescription constrains), compiled by lsim-codegen like any model, and
//! stepped here with a linearly implicit Rosenbrock-W method: one linear
//! solve per stage, no Newton iteration, a Jacobian that may be reused over
//! many steps, L-stable for the stiff electrical states. With the speed
//! trace piecewise linear, each 1 s step sees the constant acceleration the
//! standard backward-facing (quasi-static) method uses.
//! Target: about 10⁶ × real time on WLTC (1800 s in about 2 ms).
//!
//! Stage 1 fixes the interfaces; work package 6 implements them.

pub use lsim_ir::InverseSpec;
use lsim_ir::runtime::ModelFunctions;

/// A prescribed trajectory: piecewise-linear samples (one per prescribed
/// variable of the [`InverseSpec`], in its order).
#[derive(Clone, Debug, PartialEq)]
pub struct Trace {
    /// times, s, increasing
    pub t: Vec<f64>,
    /// values, SI
    pub value: Vec<f64>,
}

/// The fixed-step method.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FixedMethod {
    /// ROS34PW2: Rosenbrock-W, order 3, L-stable, tolerant of an old
    /// Jacobian (default)
    #[default]
    RosenbrockW,
    /// 2-stage Radau IIA with Newton: order 3, L-stable (the reference the
    /// default is tested against)
    Radau2,
    /// implicit Euler: order 1 (for cross-checks with simple tools)
    ImplicitEuler,
}

/// Fast-mode settings.
#[derive(Clone, Debug, PartialEq)]
pub struct FastOptions {
    /// step, s (the cycle's spacing)
    pub step: f64,
    /// method
    pub method: FixedMethod,
}

impl Default for FastOptions {
    fn default() -> Self {
        FastOptions { step: 1.0, method: FixedMethod::RosenbrockW }
    }
}

/// A stretch of time when a limit was exceeded.
#[derive(Clone, Debug, PartialEq)]
pub struct LimitFlag {
    /// start, s
    pub t_start: f64,
    /// end, s
    pub t_end: f64,
    /// which limit, in words (`'Front Motor': torque above its maximum at speed`)
    pub label: String,
    /// the largest excess, in the limited quantity's SI unit
    pub worst_excess: f64,
}

/// Fast-mode results.
#[derive(Clone, Debug, Default)]
pub struct FastResult {
    /// the step times
    pub times: Vec<f64>,
    /// channel names
    pub names: Vec<String>,
    /// `values[channel][k]`
    pub values: Vec<Vec<f64>>,
    /// the moments the vehicle could not follow
    pub flags: Vec<LimitFlag>,
    /// wall-clock time, s
    pub wall_seconds: f64,
}

/// Why fast mode could not run.
#[derive(Clone, Debug, PartialEq)]
pub enum FastError {
    /// not built yet (Stage 1)
    NotYet,
    /// the implicit step did not converge at this time
    NoConvergence {
        /// where
        t: f64,
    },
}

/// A fast-mode stepper over a compiled inverse model.
pub trait FastSolver {
    /// Steps the inverse model over the prescribed traces.
    fn run(
        &self,
        model: &dyn ModelFunctions,
        params: &[f64],
        inputs: &[Trace],
        opts: &FastOptions,
    ) -> Result<FastResult, FastError>;
}
