//! # lsim-solve: the solver layer
//!
//! * [`Integrator`] — what the run loop needs from a time integrator:
//!   variable-step stepping that never passes a stop time, exact location
//!   of zero crossings, dense output, restart after an event. One backend
//!   per implementation: SUNDIALS CVODE (models with no iteration
//!   variables: ODEs) and IDA (index-1 DAEs) in [`sundials`]; work package
//!   4 adds diffsol as a pure-Rust second backend for cross-checks.
//! * [`simulate`] — the run loop: start values and consistent
//!   initialisation, stepping, `when` clauses at located zero crossings,
//!   re-initialisation, and the [`Recorder`] that samples every channel on
//!   the output grid from the dense output, with each interval's min, max
//!   and time-mean taken over every internal step.
//!
//! Stage 1 implements the paths the spike needs; DESIGN.md (work package 4)
//! lists the rest: sampled clocks for Script blocks and FMUs, homotopy
//! initialisation, sparse linear algebra, energy quadratures, the solver
//! error report with a 10× tighter re-run, and parallel sweeps.

mod recorder;
#[cfg(feature = "sundials")]
pub mod sundials;

pub use recorder::Recorder;

use lsim_ir::prepared::{Direction, PreparedModel};
use lsim_ir::runtime::{EvalInput, ModelFunctions};
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
    /// No integrator backend is compiled in.
    #[error("no integrator backend is built into this engine")]
    NoBackend,
}

/// Integration method.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Method {
    /// stiff BDF (variable order 1-5), the default
    #[default]
    Bdf,
    /// non-stiff Adams-Moulton (CVODE only)
    Adams,
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
}

impl Default for SolverOptions {
    fn default() -> Self {
        SolverOptions {
            rtol: 1e-6,
            atol: 1e-8,
            max_step: 0.0,
            method: Method::Bdf,
            max_steps: 10_000_000,
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
}

impl std::ops::AddAssign for SolverStats {
    fn add_assign(&mut self, o: Self) {
        self.steps += o.steps;
        self.rhs_evals += o.rhs_evals;
        self.jac_evals += o.jac_evals;
        self.err_test_fails += o.err_test_fails;
        self.nonlin_fails += o.nonlin_fails;
        self.restarts += o.restarts;
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
    /// The discrete variables the model functions read.
    fn discrete_mut(&mut self) -> &mut [f64];
    /// Restarts at `t` from `y` (iteration variables are made consistent),
    /// after the discrete variables changed.
    fn restart(&mut self, t: f64, y: &[f64]) -> Result<(), SolveError>;
    /// Work done so far.
    fn stats(&self) -> SolverStats;
}

/// What the run loop needs to know about a model besides its functions.
#[derive(Clone, Debug)]
pub struct RunInfo {
    /// each flat variable's name (the channel names)
    pub var_names: Vec<String>,
    /// nominal magnitude of each entry of y (absolute tolerance scale)
    pub y_nominal: Vec<f64>,
    /// for each `when` clause: its zero crossing and direction
    pub whens: Vec<(usize, Direction)>,
    /// the direction each zero crossing is watched in: +1, -1 or 0 (both)
    pub root_dirs: Vec<i32>,
    /// for each `when` clause: what it is, in words
    pub when_labels: Vec<String>,
    /// parameter values, SI
    pub params: Vec<f64>,
}

impl RunInfo {
    /// Gathers the run information from a prepared model.
    pub fn from_prepared(m: &PreparedModel) -> RunInfo {
        let flat = &m.flat;
        let nominal = |v: lsim_ir::VarId| flat.var(v).nominal.abs().max(1e-30);
        let mut y_nominal: Vec<f64> = m.states.iter().map(|v| nominal(*v)).collect();
        for s in &m.algebraics {
            y_nominal.push(match s {
                lsim_ir::Slot::Var(v) | lsim_ir::Slot::Der(v) => nominal(*v),
            });
        }
        let mut root_dirs = vec![0; m.zero_crossings.len()];
        for w in &m.whens {
            root_dirs[w.crossing] = match w.direction {
                Direction::Rising => 1,
                Direction::Falling => -1,
                Direction::Both => 0,
            };
        }
        RunInfo {
            var_names: flat.vars.iter().map(|v| v.name.clone()).collect(),
            y_nominal,
            whens: m.whens.iter().map(|w| (w.crossing, w.direction)).collect(),
            root_dirs,
            when_labels: m
                .whens
                .iter()
                .map(|w| {
                    let who = flat.instance_name(w.origin.instance);
                    match &w.origin.label {
                        Some(l) => format!("{who}: {l}"),
                        None => who,
                    }
                })
                .collect(),
            params: flat.params.iter().map(|p| p.value).collect(),
        }
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
    /// The grid's times.
    pub fn times(&self) -> Vec<f64> {
        let n = ((self.t_end - self.t0) / self.dt - 1e-9).ceil().max(0.0) as usize;
        let mut t: Vec<f64> =
            (0..=n).map(|k| (self.t0 + k as f64 * self.dt).min(self.t_end)).collect();
        t.dedup();
        t
    }
}

/// An event that happened.
#[derive(Clone, Debug, PartialEq)]
pub struct EventRecord {
    /// when, s (located to the integrator's root-finding precision)
    pub t: f64,
    /// which `when` clause fired
    pub when: usize,
    /// what it is
    pub label: String,
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
}

impl SimResult {
    /// A channel's values by name.
    pub fn channel(&self, name: &str) -> Option<&[f64]> {
        self.names.iter().position(|n| n == name).map(|i| self.values[i].as_slice())
    }
}

/// Runs `model` over `grid` with the default backend: CVODE when the model
/// has no iteration variables, IDA otherwise.
#[cfg(feature = "sundials")]
pub fn simulate(
    model: &dyn ModelFunctions,
    info: &RunInfo,
    opts: &SolverOptions,
    grid: OutputGrid,
) -> Result<SimResult, SolveError> {
    let started = Instant::now();
    let l = *model.layout();
    let mut y0 = vec![0.0; l.n_y()];
    let mut d0 = vec![0.0; l.n_d];
    model.start(&info.params, &mut y0, &mut d0);
    let u = vec![0.0; l.n_u];
    let mut integ = sundials::Sundials::new(model, info, opts, grid, &y0, d0, u.clone())?;
    run_loop(model, info, opts, grid, &mut integ, &u, started)
}

/// Without a backend, runs fail.
#[cfg(not(feature = "sundials"))]
pub fn simulate(
    _model: &dyn ModelFunctions,
    _info: &RunInfo,
    _opts: &SolverOptions,
    _grid: OutputGrid,
) -> Result<SimResult, SolveError> {
    Err(SolveError::NoBackend)
}

/// The run loop, for any [`Integrator`].
pub fn run_loop(
    model: &dyn ModelFunctions,
    info: &RunInfo,
    opts: &SolverOptions,
    grid: OutputGrid,
    integ: &mut dyn Integrator,
    u: &[f64],
    started: Instant,
) -> Result<SimResult, SolveError> {
    let l = *model.layout();
    let times = grid.times();
    let mut rec = Recorder::new(l.n_vars, &times);
    let mut work = vec![0.0; l.n_work];
    let mut vars = vec![0.0; l.n_vars];
    let mut y = vec![0.0; l.n_y()];
    let p = &info.params;
    let mut events = vec![];

    let mut d = integ.discrete_mut().to_vec();
    let sample = |d: &[f64], t: f64, y: &[f64], work: &mut [f64], vars: &mut [f64]| {
        model.vars(&EvalInput { t, y, p, d, u }, work, vars);
    };

    // the consistent start
    y.copy_from_slice(integ.y());
    sample(&d, grid.t0, &y, &mut work, &mut vars);
    rec.start(grid.t0, &vars);
    let mut t = grid.t0;
    while t < grid.t_end {
        let st = integ.step(grid.t_end)?;
        let t_new = match &st {
            Step::Internal(t) | Step::Stopped(t) | Step::Root(t, _) => *t,
        };
        // grid points inside the step, from the dense output
        while let Some(tk) = rec.next_grid_time() {
            if tk > t_new || (tk == t_new && matches!(st, Step::Root(..))) {
                break;
            }
            if tk == t_new {
                y.copy_from_slice(integ.y());
            } else {
                integ.interpolate(tk, &mut y)?;
            }
            sample(&d, tk, &y, &mut work, &mut vars);
            rec.grid_point(tk, &vars);
        }
        // the step's end point, for min/max/mean
        y.copy_from_slice(integ.y());
        sample(&d, t_new, &y, &mut work, &mut vars);
        rec.interior(t_new, &vars);
        t = t_new;
        if let Step::Root(te, dirs) = st {
            let mut fired = vec![0.0; l.n_whens];
            let mut any = false;
            for (k, (crossing, dir)) in info.whens.iter().enumerate() {
                let r = dirs[*crossing];
                let hit = match dir {
                    Direction::Rising => r > 0,
                    Direction::Falling => r < 0,
                    Direction::Both => r != 0,
                };
                if hit {
                    fired[k] = 1.0;
                    any = true;
                    events.push(EventRecord { t: te, when: k, label: info.when_labels[k].clone() });
                }
            }
            if any {
                let mut d_new = d.clone();
                model.when(&EvalInput { t: te, y: &y, p, d: &d, u }, &fired, &mut work, &mut d_new);
                d.copy_from_slice(&d_new);
                integ.discrete_mut().copy_from_slice(&d);
                integ.restart(te, &y)?;
                // the right limit at the event time
                y.copy_from_slice(integ.y());
                sample(&d, te, &y, &mut work, &mut vars);
                rec.interior(te, &vars);
                // a grid point exactly at the event takes the value after it
                if rec.next_grid_time() == Some(te) {
                    rec.grid_point(te, &vars);
                }
            }
        }
    }
    let (values, min, max, mean) = rec.finish();
    Ok(SimResult {
        times,
        names: info.var_names.clone(),
        values,
        min,
        max,
        mean,
        events,
        stats: integ.stats(),
        backend: integ.name(),
        options: opts.clone(),
        wall_seconds: started.elapsed().as_secs_f64(),
    })
}
