//! # lsim-fast: fast mode (inverse model)
//!
//! A drive-cycle case can run in two modes, the user's choice:
//!
//! * **full dynamic** — the driver block closes the loop and the model is
//!   integrated forward (lsim-solve);
//! * **fast** — the *same* equations solved backwards: the vehicle speed
//!   follows the cycle exactly (a prescribed trajectory), the driver block
//!   is removed and its commands become unknowns, and the model is stepped
//!   at the cycle's own spacing (about 1 s) with a fixed-step, L-stable,
//!   linearly implicit method. Every `limit(x, lo, hi)` in the model (motor
//!   torque at speed, battery power, brake capacity, tyre grip) passes its
//!   value through and *flags* the moments it is outside its band instead
//!   of clamping: fast mode never falls back to forward simulation, it
//!   reports where the vehicle could not have followed.
//!
//! The inverse model is prepared from an [`InverseSpec`] (the prescribed
//! speed and its derivative become known inputs, the driver's commands
//! unknowns; index reduction differentiates what the prescription
//! constrains), compiled like any model, and stepped here:
//!
//! * [`stepper`] — the Rosenbrock-W method ROS34PW2 (order 3, L-stable,
//!   stiffly accurate) with Jacobian reuse, on the state-space form of the
//!   model: the iteration variables are solved by Newton at every evaluated
//!   point, so every recorded value satisfies every equation;
//! * [`limits`] — the rewrite that makes each limit's value and band
//!   channels, and the [`limits::FlagTracker`] that turns them into
//!   [`LimitFlag`]s (the same channels give a full dynamic run's limit hits,
//!   which the flags are checked against);
//! * [`ros`], [`lu`], [`trace`] — the method's coefficients, a small dense
//!   LU, the piecewise-linear traces.
//!
//! **Inputs.** The inverse model's inputs `u` hold, for each prescribed
//! variable in the [`InverseSpec`]'s order, its value and then its time
//! derivative ([`InverseSpec::input_names`]); fast mode fills them from one
//! [`Trace`] per prescribed variable. Each step lies on one segment of every
//! trace, so the derivative is constant within it: the constant
//! acceleration the standard backward-facing method assumes.
//!
//! Target: about 10⁶ × real time on WLTC (1800 s in about 2 ms).

pub mod limits;
pub mod lu;
pub mod ros;
pub mod stepper;
pub mod trace;

pub use limits::{LimitMode, LimitSite};
pub use lsim_ir::InverseSpec;
pub use trace::Trace;

use lsim_ir::prepared::Direction;
use lsim_ir::runtime::{DiscreteBlock, ModelFunctions};

/// The fixed-step method.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FixedMethod {
    /// ROS34PW2: Rosenbrock-W, order 3, L-stable, stiffly accurate,
    /// tolerant of an old Jacobian (default)
    #[default]
    RosenbrockW,
    /// linearly implicit Euler: order 1 (a cross-check with simple
    /// quasi-static tools)
    LinearImplicitEuler,
}

/// Fast-mode settings.
#[derive(Clone, Debug, PartialEq)]
pub struct FastOptions {
    /// step, s (the cycle's spacing); the step grid is also split at every
    /// trace sample that falls inside a step
    pub step: f64,
    /// method
    pub method: FixedMethod,
    /// start time, s (default: where every trace has begun)
    pub t_start: Option<f64>,
    /// end time, s (default: where the first trace ends)
    pub t_end: Option<f64>,
    /// relative tolerance of the embedded error estimate, which reports
    /// the step's accuracy and decides when the kept Jacobian has gone bad
    pub rtol: f64,
    /// absolute tolerance of the error estimate, times each state's nominal value
    pub atol: f64,
    /// the iteration variables' Newton tolerance, relative to their magnitude
    pub newton_tol: f64,
    /// evaluate the middle of every step too: each output interval's
    /// min, max and Simpson mean, and limit flags that cannot miss a peak
    /// inside a step (default true)
    pub interval_stats: bool,
    /// a value outside its band by less than this share of its magnitude
    /// is not flagged
    pub flag_rtol: f64,
    /// refresh the Jacobian after this many steps at the latest (0: only
    /// when the error estimate says so)
    pub max_jacobian_age: usize,
}

impl Default for FastOptions {
    fn default() -> Self {
        FastOptions {
            step: 1.0,
            method: FixedMethod::RosenbrockW,
            t_start: None,
            t_end: None,
            rtol: 1e-6,
            atol: 1e-9,
            newton_tol: 1e-10,
            interval_stats: true,
            flag_rtol: 1e-9,
            max_jacobian_age: 0,
        }
    }
}

/// A stretch of time when a limit was exceeded.
#[derive(Clone, Debug, PartialEq)]
pub struct LimitFlag {
    /// start, s
    pub t_start: f64,
    /// end, s
    pub t_end: f64,
    /// which limit, in words (`'Front Motor': the torque follows the demand within its limits — above its upper limit`)
    pub label: String,
    /// the part on the user's diagram (instance path)
    pub part: String,
    /// above the upper bound (true) or below the lower one
    pub upper: bool,
    /// the largest excess, in the limited quantity's SI unit
    pub worst_excess: f64,
    /// that unit
    pub unit: String,
}

/// An event that happened in a fast-mode run.
#[derive(Clone, Debug, PartialEq)]
pub struct FastEvent {
    /// when, s
    pub t: f64,
    /// which `when` clause
    pub when: usize,
    /// what it is
    pub label: String,
}

/// What a fast-mode run did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FastReport {
    /// the method
    pub method: &'static str,
    /// states stepped
    pub n_x: usize,
    /// iteration variables solved at every point
    pub n_z: usize,
    /// steps (including splits at trace samples)
    pub steps: u64,
    /// model residual evaluations
    pub residual_evals: u64,
    /// channel evaluations
    pub vars_evals: u64,
    /// full Jacobian evaluations
    pub jacobian_evals: u64,
    /// LU factorisations of the step matrix
    pub lu_factorisations: u64,
    /// Newton iterations on the iteration variables
    pub newton_iterations: u64,
    /// refreshes of the iteration variables' Jacobian alone
    pub gz_refreshes: u64,
    /// steps redone with a fresh Jacobian
    pub step_redos: u64,
    /// events
    pub events: u64,
    /// sampled-block ticks that changed an output
    pub block_changes: u64,
    /// the largest scaled local error estimate (1 = at the tolerances)
    pub max_error: f64,
    /// where it was, s
    pub max_error_at: f64,
}

/// Fast-mode results. Channel data are stored time-major: the value of
/// recorded channel `j` at `times[k]` is `values[k * names.len() + j]`.
#[derive(Clone, Debug, Default)]
pub struct FastResult {
    /// the output times
    pub times: Vec<f64>,
    /// the recorded channels' names
    pub names: Vec<String>,
    /// the recorded channels' variable indices
    pub record: Vec<usize>,
    /// values (left limit at each output time; the first, the start)
    pub values: Vec<f64>,
    /// lowest value over the interval ending at each output time
    pub min: Vec<f64>,
    /// highest value over that interval
    pub max: Vec<f64>,
    /// time-mean over that interval
    pub mean: Vec<f64>,
    /// the moments the vehicle could not follow
    pub flags: Vec<LimitFlag>,
    /// the events
    pub events: Vec<FastEvent>,
    /// what the run did
    pub report: FastReport,
    /// wall-clock time, s
    pub wall_seconds: f64,
}

impl FastResult {
    /// A recorded channel's values, by name.
    pub fn channel(&self, name: &str) -> Option<Vec<f64>> {
        let j = self.names.iter().position(|n| n == name)?;
        Some(self.column(&self.values, j))
    }

    /// Column `j` of one of the time-major tables (`values`, `min`, `max`, `mean`).
    pub fn column(&self, table: &[f64], j: usize) -> Vec<f64> {
        let n = self.names.len();
        (0..self.times.len()).map(|k| table[k * n + j]).collect()
    }
}

/// Why fast mode could not run.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum FastError {
    /// the inputs do not fit the model
    #[error("fast mode: {0}")]
    Inputs(String),
    /// a trace is malformed
    #[error("fast mode: {0}")]
    Trace(String),
    /// an iteration did not converge
    #[error("fast mode stopped at t = {t} s: {what}")]
    NoConvergence {
        /// where
        t: f64,
        /// what
        what: String,
    },
    /// a matrix was singular
    #[error("fast mode stopped at t = {t} s: {what} is singular")]
    Singular {
        /// where
        t: f64,
        /// which matrix
        what: String,
    },
    /// a sampled block failed
    #[error("fast mode: block '{name}' failed: {message}")]
    Block {
        /// the block
        name: String,
        /// what it said
        message: String,
    },
}

/// A `when` clause as fast mode sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct WhenInfo {
    /// its zero-crossing function
    pub crossing: usize,
    /// which way it must cross
    pub direction: Direction,
    /// what it is, in words
    pub label: String,
}

/// A sampled block and where it sits in the model.
pub struct BoundBlock {
    /// the block
    pub block: Box<dyn DiscreteBlock>,
    /// the variables it reads (indices into the model's variables)
    pub inputs: Vec<usize>,
    /// the discrete variables it sets (indices into `d`)
    pub outputs: Vec<usize>,
}

/// Everything one fast-mode run needs.
#[derive(Clone, Copy)]
pub struct FastProblem<'a> {
    /// the compiled inverse model
    pub model: &'a dyn ModelFunctions,
    /// its parameters, SI
    pub params: &'a [f64],
    /// one trace per prescribed variable
    pub traces: &'a [Trace],
    /// settings
    pub opts: &'a FastOptions,
    /// each state's and iteration variable's nominal magnitude (empty: 1)
    pub y_nominal: &'a [f64],
    /// the model's variable names (empty: `v[i]`)
    pub names: &'a [String],
    /// which variables to record (`None`: all)
    pub record: Option<&'a [usize]>,
    /// the limits to flag
    pub limits: &'a [LimitSite],
    /// the `when` clauses
    pub whens: &'a [WhenInfo],
}

impl<'a> FastProblem<'a> {
    /// A problem with defaults for everything but the model, parameters,
    /// traces and options.
    pub fn new(
        model: &'a dyn ModelFunctions,
        params: &'a [f64],
        traces: &'a [Trace],
        opts: &'a FastOptions,
    ) -> Self {
        FastProblem {
            model,
            params,
            traces,
            opts,
            y_nominal: &[],
            names: &[],
            record: None,
            limits: &[],
            whens: &[],
        }
    }
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

/// The Rosenbrock-W stepper (the method is chosen in [`FastOptions`]).
#[derive(Clone, Copy, Debug, Default)]
pub struct RosenbrockW;

impl FastSolver for RosenbrockW {
    fn run(
        &self,
        model: &dyn ModelFunctions,
        params: &[f64],
        inputs: &[Trace],
        opts: &FastOptions,
    ) -> Result<FastResult, FastError> {
        stepper::run(&FastProblem::new(model, params, inputs, opts), &mut [])
    }
}

/// Runs fast mode on a full problem, with sampled blocks.
pub fn run(problem: &FastProblem<'_>, blocks: &mut [BoundBlock]) -> Result<FastResult, FastError> {
    stepper::run(problem, blocks)
}
