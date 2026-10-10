//! # lsim-solve: the solver layer
//!
//! * [`Integrator`] — what the run loop needs from a time integrator:
//!   variable-step stepping that never passes a stop time, exact location
//!   of zero crossings, dense output, restart after an event, quadratures.
//!   Backends: SUNDIALS CVODES (models with no iteration variables: ODEs;
//!   BDF or Adams, chosen automatically) and IDAS (index-1 DAEs) in
//!   [`sundials`], built in-tree by `lsim-sundials-sys` with dense, band or
//!   sparse (faer, [`faer_ls`]) LU; diffsol (pure Rust, feature `diffsol`)
//!   as the independent second backend for cross-checks.
//! * [`simulate`] — the run loop ([`run`]): consistent initialisation
//!   (Newton, then homotopy: [`init`]), stepping, `when` clauses and modes
//!   at located zero crossings with event iteration, time events, sampled
//!   blocks on their clocks (restarting the integrator only when an output
//!   changed), event-storm detection, re-initialisation, the [`Recorder`]
//!   that samples every channel on the output grid from the dense output
//!   with each interval's min, max and time-mean, the energy books
//!   ([`energy`]) and the run report ([`SolverReport`]).
//! * [`accuracy_check`] — the one-click check: the same run 10× tighter
//!   and how far every channel moved.
//! * [`sweep`] — parameter sets in parallel (rayon), one compiled model.

pub mod accuracy;
mod ad;
#[cfg(feature = "diffsol")]
pub mod diffsol_backend;
pub mod energy;
#[cfg(feature = "sundials")]
pub mod faer_ls;
pub mod info;
pub mod init;
mod interval;
pub mod jac;
mod recorder;
pub mod run;
#[cfg(feature = "sundials")]
pub mod sundials;
pub mod sweep;

pub use accuracy::{AccuracyReport, ChannelChange, accuracy_check, compare_runs};
pub use energy::{EnergyBooks, PartBooks};
pub use info::{
    AssertInfo, BlockInfo, EnergyInfo, EnergyPart, EngagementInfo, ImpulseInfo, ImpulseLink,
    InputChain, ModeInfo, RunInfo, StoredRates, TimeCrossing, TimeFunction, VarSource,
};
pub use recorder::Recorder;
pub use run::run_loop;
pub use sweep::sweep;

use lsim_ir::runtime::{DiscreteBlock, ModelFunctions};
use std::time::Instant;

/// Why a run failed.
#[derive(Debug, Clone, thiserror::Error)]
pub enum SolveError {
    /// The integrator gave up (step size too small, Newton failures …).
    #[error("the solver stopped at t = {t} s: {message}")]
    Integrator {
        /// where
        t: f64,
        /// what it said
        message: String,
    },
    /// The start values could not be made consistent.
    #[error("the model could not be initialised at t = {t} s: {message}")]
    Initialisation {
        /// where
        t: f64,
        /// what failed, naming the equations with the largest residuals
        message: String,
    },
    /// Events piled up: a mode or `when` condition chattering.
    #[error("event storm at t = {t} s: {message}")]
    EventStorm {
        /// where
        t: f64,
        /// which conditions, naming the parts
        message: String,
        /// the conditions' labels, most frequent first
        parts: Vec<String>,
    },
    /// The energy books did not close.
    #[error("the energy books do not close: {message}")]
    EnergyBooks {
        /// how far, and which parts
        message: String,
    },
    /// A table was read outside its data on an axis that forbids it.
    #[error("at t = {t} s, {message}")]
    TableOutside {
        /// where
        t: f64,
        /// which table and axis, in words
        message: String,
    },
    /// A model's `assert` (an error) failed.
    #[error("at t = {t} s, {message}")]
    Assert {
        /// where
        t: f64,
        /// the assert's message, naming the part
        message: String,
    },
    /// A sampled block failed.
    #[error("sampled block {block} failed at t = {t} s: {message}")]
    Block {
        /// the block
        block: String,
        /// when
        t: f64,
        /// what it said
        message: String,
    },
    /// No integrator backend is compiled in.
    #[error("no integrator backend is built into this engine")]
    NoBackend,
}

/// Integration method.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Method {
    /// stiff BDF unless the model is plainly non-stiff, then Adams; checked
    /// again after events, and switched back on convergence trouble (the
    /// default)
    #[default]
    Auto,
    /// stiff BDF (variable order 1-5) with Newton iteration
    Bdf,
    /// non-stiff Adams-Moulton (variable order 1-12) with fixed-point
    /// iteration (CVODE only; a DAE always uses BDF)
    Adams,
}

/// The linear solver inside the Newton iteration.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum LinearSolver {
    /// dense LU up to 40 unknowns, band LU when the Jacobian's band is
    /// narrow, sparse LU otherwise (the default)
    #[default]
    Auto,
    /// dense LU with partial pivoting (SUNDIALS)
    Dense,
    /// band LU (SUNDIALS), from the Jacobian's band widths
    Band,
    /// sparse LU (faer: fill-reducing ordering, partial pivoting)
    Sparse,
}

/// Which integrator library runs the model.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Backend {
    /// SUNDIALS CVODES/IDAS (the default)
    #[default]
    Sundials,
    /// diffsol BDF (pure Rust; the cross-check)
    Diffsol,
}

/// Solver settings.
#[derive(Clone, Debug)]
pub struct SolverOptions {
    /// relative tolerance
    pub rtol: f64,
    /// absolute tolerance, multiplied by each variable's nominal value
    pub atol: f64,
    /// largest internal step, s (0: no limit)
    pub max_step: f64,
    /// the method
    pub method: Method,
    /// give up after this many internal steps
    pub max_steps: u64,
    /// the linear solver
    pub linear_solver: LinearSolver,
    /// the integrator library
    pub backend: Backend,
    /// keep the energy books (port powers, losses, stored energy)
    pub energy_books: bool,
    /// fail the run when the books close worse than this share of the
    /// energy throughput (0: report only)
    pub energy_tolerance: f64,
    /// put the energy integrals under the integrator's error control, so
    /// they are as accurate as the states (the default; the step size then
    /// also serves them). Off, they ride on the states' steps, which the
    /// states alone choose: a fast-decaying loss came out 100× less
    /// accurate than the tolerance, and where the states are exact on long
    /// steps (a constant torque on an inertia: 1 s steps from the start)
    /// an integral can be off by tens of percent (9 J supplied came out as
    /// 15.55 J). The closure compares the integrals with each other and
    /// can be zero all the same; the drift (stored energy from the states
    /// against its integral) shows the error, and the run warns that the
    /// books were not under error control
    /// ([`EnergyBooks::error_controlled`])
    pub energy_error_control: bool,
    /// event iterations allowed at one instant before the run stops with
    /// an event storm: rounds of re-checking the conditions after a change,
    /// and, counted on their own, the rigid engagements one instant's
    /// events chain (a shift whose new speeds fire the next shift, each
    /// one projected); raise it for a cascade that is meant
    pub max_event_iterations: usize,
    /// an event storm: more than this many state events (zero crossings and
    /// modes; sample ticks and time events do not count, nor what they
    /// change at their instant) …
    pub storm_events: usize,
    /// … within this share of the run's length (at least 1 µs)
    pub storm_window: f64,
    /// IDA: leave the iteration variables out of the local error test
    pub suppress_algebraic_error: bool,
    /// at a rigid engagement a part declares (a gear shift), move the
    /// states to keep the momentum of everything the engagement ties
    /// together (an impulse projection, [`RunInfo::impulse`]); off: the
    /// states stay and the speeds they set jump to the new couplings
    pub impulses: bool,
    /// let a sample tick whose change the next step's error test can
    /// absorb go on with the integration's history instead of restarting
    /// (a light restart). Off by default: the history carries the kink the
    /// tick put in the derivatives into the next steps, and the error that
    /// leaves has the same sign at every tick of a steadily moving command,
    /// so it accumulates beyond the tolerance over a long run (a 10 000-tick
    /// ramp at rtol 1e-6: 57 tolerance units). A tick whose outputs reach
    /// nothing the integrator integrates or watches goes on without a
    /// restart either way ([`RunInfo::dynamic_discretes`]): that is exact
    pub light_restarts: bool,
    /// time the check of the conditions that mix time and states along
    /// each step ([`SolverReport::mixed_seconds`]: two clock readings a
    /// step), for the benchmark of its share of the run (lsim-project's
    /// `scan_share` example). Off by default
    pub time_mixed_checks: bool,
}

impl Default for SolverOptions {
    fn default() -> Self {
        SolverOptions {
            rtol: 1e-6,
            atol: 1e-8,
            max_step: 0.0,
            method: Method::Auto,
            max_steps: 10_000_000,
            linear_solver: LinearSolver::Auto,
            backend: Backend::Sundials,
            energy_books: true,
            energy_tolerance: 0.0,
            energy_error_control: true,
            max_event_iterations: 50,
            storm_events: 100,
            storm_window: 1e-3,
            suppress_algebraic_error: false,
            impulses: true,
            light_restarts: false,
            time_mixed_checks: false,
        }
    }
}

impl SolverOptions {
    /// The same settings with tolerances 10× tighter: the one-click
    /// accuracy check (DESIGN.md, *Accuracy*).
    pub fn tighter(&self) -> Self {
        SolverOptions { rtol: self.rtol / 10.0, atol: self.atol / 10.0, ..self.clone() }
    }
}

/// What one internal step ended with.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// an ordinary internal step, ending at this time
    Internal(f64),
    /// the stop time was reached
    Stopped(f64),
    /// zero crossings were located at this time; one entry per crossing:
    /// +1 rising, -1 falling, 0 none
    Root(f64, Vec<i32>),
}

impl Step {
    /// When the step ended.
    pub fn time(&self) -> f64 {
        match self {
            Step::Internal(t) | Step::Stopped(t) | Step::Root(t, _) => *t,
        }
    }
}

/// Work counters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SolverStats {
    /// internal steps
    pub steps: u64,
    /// residual (right-hand side) evaluations
    pub rhs_evals: u64,
    /// Jacobian evaluations
    pub jac_evals: u64,
    /// local error test failures
    pub err_test_fails: u64,
    /// nonlinear solver convergence failures
    pub nonlin_fails: u64,
    /// restarts after events
    pub restarts: u64,
    /// Newton-matrix factorisations
    pub lin_setups: u64,
}

impl std::ops::AddAssign for SolverStats {
    fn add_assign(&mut self, o: Self) {
        self.steps += o.steps;
        self.rhs_evals += o.rhs_evals;
        self.jac_evals += o.jac_evals;
        self.err_test_fails += o.err_test_fails;
        self.nonlin_fails += o.nonlin_fails;
        self.restarts += o.restarts;
        self.lin_setups += o.lin_setups;
    }
}

/// The polynomial an integrator's dense output is over its last step, for
/// some entries of y, in Newton form:
///
/// `y(t) = Σ_j a_j Π_{i<j} (τ − x_i) / s_i`, `τ = t − origin`,
///
/// the nodes `x_i` and scales `s_i` shared by the entries (CVODE's
/// Nordsieck array: every `x_i = 0`, `s_i = h`; IDA's modified divided
/// differences: `x_0 = 0`, `x_i = −ψ_{i−1}`, `s_i = ψ_i`). The run loop
/// encloses it over any part of the step with outward rounding.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DenseOutput {
    /// τ's origin
    pub origin: f64,
    /// the nodes `x_i`, as many as the degree
    pub nodes: Vec<f64>,
    /// the scales `s_i`
    pub scales: Vec<f64>,
    /// per entry, its `degree + 1` coefficients `a_j`, entry after entry
    /// (any values after the last entry's are not part of it)
    pub coef: Vec<f64>,
    /// per entry, a bound on its distance from the integrator's own
    /// interpolant over the part of the step asked for (0: it is that
    /// interpolant, as the integrator holds it)
    pub err: Vec<f64>,
}

impl DenseOutput {
    /// The polynomial's degree.
    pub fn degree(&self) -> usize {
        self.nodes.len()
    }

    /// Entry `m`'s coefficients.
    pub fn coefficients(&self, m: usize) -> &[f64] {
        let k = self.degree() + 1;
        &self.coef[m * k..(m + 1) * k]
    }

    /// Entry `m` at `t`, in floating point (as the integrators evaluate
    /// their dense output: the nested Newton form).
    pub fn at(&self, m: usize, t: f64) -> f64 {
        let tau = t - self.origin;
        let a = self.coefficients(m);
        let q = self.degree();
        let mut acc = a[q];
        for j in (0..q).rev() {
            acc = a[j] + (tau - self.nodes[j]) / self.scales[j] * acc;
        }
        acc
    }

    /// The constant `y` at `t` (an integrator that has not stepped yet).
    pub fn constant(&mut self, t: f64, y: impl Iterator<Item = f64>) {
        self.origin = t;
        self.nodes.clear();
        self.scales.clear();
        self.coef.clear();
        self.coef.extend(y);
        self.err.clear();
        self.err.resize(self.coef.len(), 0.0);
    }
}

/// A time integrator for `x' = f(t, x, z)`, `0 = g(t, x, z)`.
pub trait Integrator {
    /// The backend's name, for the run report.
    fn name(&self) -> &'static str;
    /// One internal step, never past `t_stop`.
    fn step(&mut self, t_stop: f64) -> Result<Step, SolveError>;
    /// y at the end of the last step.
    fn y(&self) -> &[f64];
    /// y at `t`, inside the last step (dense output).
    fn interpolate(&mut self, t: f64, out: &mut [f64]) -> Result<(), SolveError>;
    /// Selected entries of y at `t`, inside the last step: `out[m] =
    /// y(t)[idx[m]]` (a sampled block's inputs, without interpolating the
    /// whole state).
    fn interpolate_select(
        &mut self,
        t: f64,
        idx: &[usize],
        out: &mut [f64],
    ) -> Result<(), SolveError> {
        let mut y = self.y().to_vec();
        self.interpolate(t, &mut y)?;
        for (o, i) in out.iter_mut().zip(idx) {
            *o = y[*i];
        }
        Ok(())
    }
    /// The polynomial the dense output is over the last step, for the
    /// entries `idx` of y ([`DenseOutput`]), to be read on `[t0, the
    /// step's end]` (inside the last step). False when the backend does
    /// not give it (the default) or `t0` lies before the last step: the run
    /// loop then cannot check a condition along the step, and says so.
    fn dense_output(
        &mut self,
        _t0: f64,
        _idx: &[usize],
        _out: &mut DenseOutput,
    ) -> Result<bool, SolveError> {
        Ok(false)
    }
    /// The discrete variables the model functions read.
    fn discrete_mut(&mut self) -> &mut [f64];
    /// Restarts at `t` from `y` (iteration variables are made consistent),
    /// after the discrete variables changed. `t` may lie inside the last
    /// step (the rest of the step is dropped).
    fn restart(&mut self, t: f64, y: &[f64]) -> Result<(), SolveError>;
    /// Goes on from `t`, the end of the last step, keeping the integration
    /// history (no restart), after an event that changed discrete values
    /// so little that the next step's error test hardly sees it. False when
    /// the backend cannot (its last step does not end at `t`, or it has
    /// none): the run loop then restarts it.
    fn resume(&mut self, _t: f64) -> bool {
        false
    }
    /// The step the integrator plans next, s (0: none yet).
    fn planned_step(&self) -> f64 {
        0.0
    }
    /// Makes the iteration variables of `y` consistent at `t` with the
    /// discrete values `d`, the states held (event iteration re-checks the
    /// conditions with them after a discrete value changed). The run loop
    /// calls it only for a model with iteration variables, so an
    /// integrator of ODEs never sees it. The default fails: an integrator
    /// that solves DAEs and does not implement it stops the run there
    /// instead of going on with iteration variables that no longer hold.
    fn consistent_z(&mut self, t: f64, _y: &mut [f64], _d: &[f64]) -> Result<(), SolveError> {
        Err(SolveError::Integrator {
            t,
            message: format!(
                "the {} integrator cannot make a model's iteration variables consistent after \
                 an event (Integrator::consistent_z is not implemented)",
                self.name()
            ),
        })
    }
    /// Work done so far.
    fn stats(&self) -> SolverStats;
    /// The energy integrals at `t` (inside the last step), when the backend
    /// integrates them (see [`energy::Integrand`]).
    fn quadrature(&mut self, _t: f64, _out: &mut [f64]) -> Result<(), SolveError> {
        Ok(())
    }
    /// The last step's estimated local error per entry of y, as a share of
    /// that entry's tolerance (|e| / (rtol |y| + atol)); false when the
    /// backend does not give it.
    fn local_error(&mut self, _out: &mut [f64]) -> bool {
        false
    }
    /// For each root function, the side an exact zero counts as (+1 or -1;
    /// 0: none): the run loop sets it after every event so a function that
    /// rests at zero after its crossing (a held value) does not fire again.
    fn set_root_sides(&mut self, _sides: &[f64]) {}
    /// The root functions the integrator must not watch (true): the run
    /// loop schedules them itself as exact time events. A backend that
    /// watches every root function must say so (false).
    fn set_root_mask(&mut self, mask: &[bool]) -> bool {
        !mask.iter().any(|m| *m)
    }
    /// The method now in use, for the report.
    fn method(&self) -> String {
        "BDF".into()
    }
    /// How the integrator was set up (method choice, linear solver,
    /// initialisation), for the report.
    fn setup_notes(&self) -> Vec<String> {
        vec![]
    }
}

/// The output grid.
#[derive(Clone, Copy, Debug)]
pub struct OutputGrid {
    /// start time, s
    pub t0: f64,
    /// end time, s
    pub t_end: f64,
    /// spacing, s (the last interval may be shorter)
    pub dt: f64,
}

impl OutputGrid {
    /// The grid's times: from `t0` every `dt`, the last one `t_end`
    /// exactly (also when `n·dt` rounds a few ulps short of it).
    pub fn times(&self) -> Vec<f64> {
        let n = ((self.t_end - self.t0) / self.dt - 1e-9).ceil().max(0.0) as usize;
        let mut t: Vec<f64> =
            (0..=n)
                .map(|k| {
                    if k == n { self.t_end } else { (self.t0 + k as f64 * self.dt).min(self.t_end) }
                })
                .collect();
        t.dedup();
        t
    }
}

/// What made an event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    /// a `when` clause fired (index into [`RunInfo::whens`])
    When(usize),
    /// a mode flipped (index into [`RunInfo::modes`])
    Mode(usize),
    /// a sampled block's output changed (index into the blocks)
    Block(usize),
    /// a time event (index into [`RunInfo::time_events`])
    Time(usize),
}

/// An event that happened.
#[derive(Clone, Debug, PartialEq)]
pub struct EventRecord {
    /// when, s (located to the integrator's root-finding precision, or
    /// exact for time events and sample ticks)
    pub t: f64,
    /// which `when` clause fired (`usize::MAX` for other kinds)
    pub when: usize,
    /// what it is
    pub label: String,
    /// what made it
    pub kind: EventKind,
}

/// The achieved accuracy as the integrator estimated it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ErrorEstimate {
    /// the largest local error estimate of any step, as a share of the
    /// tolerance (the error test passes at ≤ 1)
    pub worst_local: f64,
    /// the entry of y it was in
    pub worst_local_var: String,
    /// for each entry of y: the local error estimates summed over every
    /// step, relative to the entry's largest magnitude (an upper bound of
    /// the global error that ignores damping), largest first
    pub accumulated: Vec<(String, f64)>,
}

/// What the solver did, for the run report (DESIGN.md, *Solver report*).
#[derive(Clone, Debug, Default)]
pub struct SolverReport {
    /// the integrator
    pub backend: &'static str,
    /// the method(s) used
    pub method: String,
    /// how the integrator was set up: method choice and why, linear
    /// solver, initialisation path
    pub notes: Vec<String>,
    /// relative tolerance
    pub rtol: f64,
    /// absolute tolerance (times each variable's nominal value)
    pub atol: f64,
    /// the work done
    pub stats: SolverStats,
    /// events
    pub events: usize,
    /// sampled-block ticks
    pub block_ticks: u64,
    /// ticks that changed an output (and so restarted the integrator,
    /// unless they are counted below)
    pub block_changes: u64,
    /// of those, the ticks whose outputs reach nothing the integrator
    /// integrates or watches ([`RunInfo::dynamic_discretes`]): the step
    /// went on, exactly, with no restart and no cut
    pub inert_ticks: u64,
    /// of those, with [`SolverOptions::light_restarts`] (opt-in), the ticks
    /// so slight that the integration went on with its history instead of
    /// restarting, the next step's error test checking the change
    pub light_restarts: u64,
    /// impulse projections at rigid engagements (gear shifts)
    pub impulses: u64,
    /// iteration variables solved again between the ticks of one instant
    /// (a block reading what a block before it moved)
    pub z_solves: u64,
    /// steps ended at a sign change of a condition that mixes time and
    /// states, found along the step, that root finding did not see
    pub pulses_found: u64,
    /// steps of such conditions a certificate cleared (a few comparisons
    /// per state they read), summed over the conditions
    pub mixed_certified: u64,
    /// steps of such conditions taken along the dense output, where no
    /// certificate held, summed over the conditions
    pub mixed_scanned: u64,
    /// wall-clock time the check of such conditions took, s (0 unless
    /// [`SolverOptions::time_mixed_checks`])
    pub mixed_seconds: f64,
    /// the integrator's own error estimate
    pub error: ErrorEstimate,
    /// warnings for the user
    pub warnings: Vec<String>,
}

/// A run's results.
#[derive(Clone, Debug)]
pub struct SimResult {
    /// the output times
    pub times: Vec<f64>,
    /// the channels' names, one per flat variable
    pub names: Vec<String>,
    /// `values[channel][k]`: the value at `times[k]`
    pub values: Vec<Vec<f64>>,
    /// lowest value over the interval ending at `times[k]` (at k = 0, the value)
    pub min: Vec<Vec<f64>>,
    /// highest value over the interval ending at `times[k]`
    pub max: Vec<Vec<f64>>,
    /// time-mean over the interval ending at `times[k]`
    pub mean: Vec<Vec<f64>>,
    /// the output times that fall exactly on an event, in order of the
    /// output point: the values just before it (the left limit) of the
    /// channels the event changed; `values[c][k]` holds the value just
    /// after the event, as everywhere ([`SimResult::before`] gives either
    /// side's for any channel). Both sides of the event at the same time,
    /// as Modelica tools write them to their result files (the left one
    /// first)
    pub left_limits: Vec<LeftLimit>,
    /// the events, in order
    pub events: Vec<EventRecord>,
    /// work counters
    pub stats: SolverStats,
    /// which integrator ran
    pub backend: &'static str,
    /// the tolerances it ran with
    pub options: SolverOptions,
    /// wall-clock time of the run (not of compilation), s
    pub wall_seconds: f64,
    /// the energy books, when kept
    pub energy: Option<EnergyBooks>,
    /// what the solver did
    pub report: SolverReport,
}

/// The values just before an event that falls on an output time: only
/// those of the channels the event changed (the others' are the values
/// after it, [`SimResult::values`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LeftLimit {
    /// the output point: `times[k]`
    pub k: usize,
    /// the channels whose value just before the event differs from the
    /// value just after, increasing
    pub channels: Vec<u32>,
    /// their values just before it
    pub before: Vec<f64>,
}

impl LeftLimit {
    /// The changes from `left` (every channel just before) to `right`
    /// (just after) at output point `k`.
    pub fn new(k: usize, left: &[f64], right: &[f64]) -> LeftLimit {
        let mut out = LeftLimit { k, channels: vec![], before: vec![] };
        for (c, (a, b)) in left.iter().zip(right).enumerate() {
            if a.to_bits() != b.to_bits() && !(a.is_nan() && b.is_nan()) {
                out.channels.push(c as u32);
                out.before.push(*a);
            }
        }
        out.channels.shrink_to_fit();
        out.before.shrink_to_fit();
        out
    }

    /// Channel `c`'s value just before the event, when the event changed it.
    pub fn get(&self, c: usize) -> Option<f64> {
        let c = u32::try_from(c).ok()?;
        self.channels.binary_search(&c).ok().map(|i| self.before[i])
    }
}

impl SimResult {
    /// A channel's values by name.
    pub fn channel(&self, name: &str) -> Option<&[f64]> {
        self.names.iter().position(|n| n == name).map(|i| self.values[i].as_slice())
    }

    /// Channel `c`'s value just before output point `k`: its left limit
    /// when an event falls there, else its value (`values[c][k]`).
    pub fn before(&self, c: usize, k: usize) -> f64 {
        let left = self
            .left_limits
            .binary_search_by_key(&k, |l| l.k)
            .ok()
            .and_then(|i| self.left_limits[i].get(c));
        left.unwrap_or(self.values[c][k])
    }
}

/// Runs `model` over `grid` with the backend of `opts` — CVODES when the
/// model has no iteration variables, IDAS otherwise (or diffsol) —
/// driving the sampled `blocks` (one per [`RunInfo::blocks`] entry, in
/// order).
pub fn simulate(
    model: &dyn ModelFunctions,
    info: &RunInfo,
    opts: &SolverOptions,
    grid: OutputGrid,
    blocks: &mut [Box<dyn DiscreteBlock>],
) -> Result<SimResult, SolveError> {
    let started = Instant::now();
    let l = *model.layout();
    let mut y0 = vec![0.0; l.n_y()];
    let mut d0 = vec![0.0; l.n_d];
    model.start(&info.params, &mut y0, &mut d0);
    let u = vec![0.0; l.n_u];
    let start = init::initialise(
        model,
        info,
        grid.t0,
        &mut y0,
        &mut d0,
        &u,
        &init::InitSettings { rtol: opts.rtol, atol: opts.atol, max_iterations: 50 },
    )?;
    let result = run_backend(model, info, opts, grid, y0, d0, u, blocks, started);
    result.map(|mut r| {
        r.report.notes.insert(0, format!("start: {start}"));
        r
    })
}

#[allow(clippy::too_many_arguments)]
fn run_backend(
    model: &dyn ModelFunctions,
    info: &RunInfo,
    opts: &SolverOptions,
    grid: OutputGrid,
    y0: Vec<f64>,
    d0: Vec<f64>,
    u: Vec<f64>,
    blocks: &mut [Box<dyn DiscreteBlock>],
    started: Instant,
) -> Result<SimResult, SolveError> {
    let l = *model.layout();
    let quad = if opts.energy_books {
        info.energy
            .as_ref()
            .filter(|e| !e.parts.is_empty())
            .map(|e| energy::Integrand::new(e, info, &l))
    } else {
        None
    };
    match opts.backend {
        Backend::Sundials => {
            #[cfg(feature = "sundials")]
            {
                let mut integ =
                    sundials::Sundials::new(model, info, opts, grid, &y0, d0, u.clone(), quad)?;
                run_loop(model, info, opts, grid, &mut integ, &u, blocks, started)
            }
            #[cfg(not(feature = "sundials"))]
            {
                let _ = (y0, d0, quad, blocks, started);
                Err(SolveError::NoBackend)
            }
        }
        Backend::Diffsol => {
            #[cfg(feature = "diffsol")]
            {
                diffsol_backend::simulate(
                    model, info, opts, grid, &y0, d0, &u, quad, blocks, started,
                )
            }
            #[cfg(not(feature = "diffsol"))]
            {
                let _ = (y0, d0, quad, blocks, started);
                Err(SolveError::NoBackend)
            }
        }
    }
}
