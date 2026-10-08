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
//! The inverse model is prepared by lsim-prep with a different known set
//! (the prescribed speed and its derivative known, the driver's outputs
//! unknown; index reduction differentiates what the prescription
//! constrains), compiled by lsim-codegen like any model, and stepped here.
//! Target: about 10⁶ × real time on WLTC (1800 s in about 2 ms).
//!
//! Stage 1 fixes the interfaces; work package 6 implements them.

use lsim_ir::runtime::ModelFunctions;

/// A prescribed trajectory: piecewise-linear samples.
#[derive(Clone, Debug, PartialEq)]
pub struct Trace {
    /// times, s, increasing
    pub t: Vec<f64>,
    /// values, SI
    pub value: Vec<f64>,
}

/// What the inverse model prescribes and frees.
#[derive(Clone, Debug, PartialEq)]
pub struct InverseSpec {
    /// variable (full flat name) → its prescribed trajectory; usually the
    /// vehicle body's speed
    pub prescribed: Vec<(String, Trace)>,
    /// signal inputs that become unknowns (the driver's commands)
    pub freed: Vec<String>,
}

/// The fixed-step method.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FixedMethod {
    /// 2-stage Radau IIA: order 3, L-stable (default)
    #[default]
    Radau2,
    /// implicit Euler: order 1, L-stable (reference)
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
        FastOptions { step: 1.0, method: FixedMethod::Radau2 }
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
