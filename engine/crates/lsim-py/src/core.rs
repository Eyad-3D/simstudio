//! The part of the Python module that knows nothing of Python: importing a
//! project's case, building it, and running it exactly as the golden
//! comparison does (`lsim_project::golden::run_case`), with the engine's
//! front door (`lsim_engine`) instead of the plain path. Kept free of
//! PyO3 so that Rust tests can check it against the golden path.

use lsim_engine::{BuildOptions, BuildReport, Engine, RunError};
use lsim_ir::Diagnostic;
use lsim_ir::runtime::DiscreteBlock;
use lsim_project::golden::{Figure, TractionHost, figures_with, sampled_spec};
use lsim_project::import::Channel;
use lsim_project::import::{ImportOptions, ImportReport, SampledKind, SampledSpec};
use lsim_project::{import_case, standard_registry};
use lsim_solve::{
    OutputGrid, ParamEdit, RunControl, SimResult, SolveError, SolverOptions, TraceSpec,
};
use serde_json::Value;
use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Why something failed.
#[derive(Debug, Clone)]
pub enum Failure {
    /// the model (or its run) has faults the engine can name: they carry
    /// the parts involved and a hint
    Model(Vec<Diagnostic>),
    /// a plain mistake of the caller (a bad name or value)
    Value(String),
}

impl Failure {
    /// The plain messages, joined.
    pub fn message(&self) -> String {
        match self {
            Failure::Model(d) => d.iter().map(|d| d.message.clone()).collect::<Vec<_>>().join("; "),
            Failure::Value(m) => m.clone(),
        }
    }
}

fn model_error(code: &str, message: String) -> Failure {
    Failure::Model(vec![Diagnostic::error(code, message)])
}

/// A failed run as a diagnostic: the engine's own words, with a code that
/// says what kind of failure it was.
fn solve_failure(e: &SolveError) -> Failure {
    let (code, parts) = match e {
        SolveError::Integrator { .. } => ("RUN-INTEGRATOR", vec![]),
        SolveError::Initialisation { .. } => ("RUN-INIT", vec![]),
        SolveError::EventStorm { parts, .. } => ("RUN-EVENT-STORM", parts.clone()),
        SolveError::StateSelection { parts, .. } => ("RUN-STATE-SELECT", parts.clone()),
        SolveError::EnergyBooks { .. } => ("RUN-ENERGY", vec![]),
        SolveError::TableOutside { .. } => ("RUN-TABLE", vec![]),
        SolveError::Assert { .. } => ("RUN-ASSERT", vec![]),
        SolveError::Block { block, .. } => ("RUN-BLOCK", vec![block.clone()]),
        SolveError::Memory { .. } => ("RUN-MEMORY", vec![]),
        SolveError::NoBackend => ("RUN-NO-BACKEND", vec![]),
    };
    let mut d = Diagnostic::error(code, e.to_string());
    d.parts = parts;
    Failure::Model(vec![d])
}

fn run_failure(e: RunError) -> Failure {
    match e {
        RunError::Param(m) | RunError::Mode(m) => Failure::Value(m),
        RunError::Solve(e) => solve_failure(&e),
        RunError::Fast(e) => Failure::Value(e.to_string()),
    }
}

/// A Script part whose own code the engine's runner does not run (outside
/// its subset of Python, when it is read or part-way through the run):
/// the caller's function is what would run it.
pub fn script_outside(element: &str, message: String) -> Failure {
    let mut d = Diagnostic::error("RUN-SCRIPT", message).with_hint(format!(
        "give the part a Python function to run it: scripts={{'{element}': f}}, \
         f(t, dt, inputs) returning a dict of outputs or None"
    ));
    d.parts = vec![element.to_string()];
    Failure::Model(vec![d])
}

/// A Script part whose code fails when it is read, as it would on Python.
pub fn script_failed(element: &str, message: String) -> Failure {
    let mut d = Diagnostic::error("RUN-SCRIPT", message);
    d.parts = vec![element.to_string()];
    Failure::Model(vec![d])
}

/// Runs `f`, turning a panic inside the engine into a plain failure (a
/// fault of the engine, not of the caller's input).
pub fn guarded<T>(f: impl FnOnce() -> Result<T, Failure>) -> Result<T, Failure> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(p) => {
            let what = p
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown".into());
            Err(model_error(
                "ENGINE-FAULT",
                format!(
                    "The engine stopped on an internal fault ({what}). Please report the model."
                ),
            ))
        }
    }
}

/// A case imported and built, ready to run.
pub struct Built {
    /// the compiled model
    pub model: lsim_engine::Model,
    /// what the import found (channel map, sampled blocks, case settings)
    pub rep: ImportReport,
    /// the project
    pub project: Value,
    /// the case
    pub case_id: String,
    /// import time, s
    pub import_seconds: f64,
}

impl Built {
    /// What building produced.
    pub fn build_report(&self) -> &BuildReport {
        &self.model.report
    }

    /// The warnings of the import and of preparation, in that order.
    pub fn warnings(&self) -> Vec<Diagnostic> {
        let mut w = self.rep.warnings.clone();
        w.extend(self.model.report.warnings.iter().cloned());
        w
    }
}

fn parse_project(project_json: &str) -> Result<Value, Failure> {
    serde_json::from_str(project_json)
        .map_err(|e| Failure::Value(format!("The project is not valid JSON: {e}.")))
}

/// Imports `case_id` of a project (its JSON text), without building it.
pub fn import(
    project_json: &str,
    case_id: &str,
    cycles_dir: Option<PathBuf>,
) -> Result<(Value, lsim_ir::ComponentDef, ImportReport), Failure> {
    let project = parse_project(project_json)?;
    let reg = standard_registry();
    let (top, rep) = import_case(
        &project,
        Some(case_id),
        &reg,
        &ImportOptions { cycles_dir, ..Default::default() },
    )
    .map_err(Failure::Model)?;
    Ok((project, top, rep))
}

/// Imports, prepares (through the cache when `cache` is given) and compiles
/// a case.
pub fn build(
    project_json: &str,
    case_id: &str,
    cache: Option<PathBuf>,
    cycles_dir: Option<PathBuf>,
) -> Result<Built, Failure> {
    guarded(|| {
        let t0 = Instant::now();
        let (project, top, rep) = import(project_json, case_id, cycles_dir)?;
        let import_seconds = t0.elapsed().as_secs_f64();
        let engine = Engine::new(rep.library());
        let model = engine
            .build(&top, &BuildOptions { cache, ..Default::default() })
            .map_err(Failure::Model)?;
        Ok(Built { model, rep, project, case_id: case_id.to_string(), import_seconds })
    })
}

/// The structural and unit checks of a case: the import's findings, and
/// preparation's faults when the model has any.
pub fn diagnose(
    project_json: &str,
    case_id: &str,
    cycles_dir: Option<PathBuf>,
) -> Result<Vec<Diagnostic>, Failure> {
    guarded(|| {
        let project = parse_project(project_json)?;
        let reg = standard_registry();
        let opts = ImportOptions { cycles_dir, ..Default::default() };
        match import_case(&project, Some(case_id), &reg, &opts) {
            Err(d) => Ok(d),
            Ok((top, rep)) => {
                let mut out = rep.warnings.clone();
                out.extend(Engine::new(rep.library()).diagnose(&top));
                Ok(out)
            }
        }
    })
}

/// An output point as a run's [`Control`] passes it on: its time, and
/// each of today's channels there in its display unit (in
/// [`Control::channels`]' order), the values the result holds.
pub type Point = (f64, Vec<f64>);

/// One run's control (the engine's [`RunControl`]): what the Python
/// module's `RunControl` is. Stop is a flag the caller sets from any
/// thread; the output points go through a queue of at most `room` points
/// (none kept with a `room` of 0) that the caller empties from its own
/// thread while the run goes, the run waiting while it is full (so a
/// reader that paces itself paces the run); parameter changes wait for the
/// run's next step. Nothing in it needs Python, so the run never waits for
/// the interpreter's lock.
pub struct Control {
    room: usize,
    stop: AtomicBool,
    /// the points go to the queue (off: an acceleration test's first run,
    /// whose points its run again to the finish replaces)
    streaming: AtomicBool,
    /// the reader is gone: points are dropped, never waited for
    closed: AtomicBool,
    /// the run takes no more parameter changes (an acceleration test run
    /// again to its finish: the first run's are made again, at their times)
    fixed: AtomicBool,
    /// the run ended (no more points)
    finished: AtomicBool,
    /// the time the run reached, and its end (f64 bits)
    reached: AtomicU64,
    t_end: AtomicU64,
    /// today's channels the points carry: name, the engine's variable
    channels: Mutex<Vec<(String, usize, Channel)>>,
    queue: Mutex<VecDeque<Point>>,
    /// a point taken, or the run told to stop
    taken: Condvar,
    /// a point put, or the run ended
    put: Condvar,
    /// parameter changes the run has not taken yet
    edits: Mutex<Vec<ParamEdit>>,
}

impl std::fmt::Debug for Control {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Control")
            .field("room", &self.room)
            .field("stop", &self.stop)
            .field("reached", &self.progress())
            .finish_non_exhaustive()
    }
}

fn locked<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Control {
    /// A control whose queue holds at most `room` points (0: it keeps
    /// none, a stop and changes alone).
    pub fn new(room: usize) -> Control {
        Control {
            room,
            stop: AtomicBool::new(false),
            streaming: AtomicBool::new(true),
            closed: AtomicBool::new(false),
            fixed: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            reached: AtomicU64::new(0f64.to_bits()),
            t_end: AtomicU64::new(0f64.to_bits()),
            channels: Mutex::new(vec![]),
            queue: Mutex::new(VecDeque::new()),
            taken: Condvar::new(),
            put: Condvar::new(),
            edits: Mutex::new(vec![]),
        }
    }

    /// Stops the run where it is (at the end of the step it takes).
    pub fn cancel(&self) {
        self.stop.store(true, Ordering::SeqCst);
        let _q = locked(&self.queue);
        self.taken.notify_all();
    }

    /// Whether the run was told to stop.
    pub fn cancelled(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    /// The reader goes: no point is waited for any more.
    pub fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        let _q = locked(&self.queue);
        self.taken.notify_all();
    }

    /// Whether the run has ended (every point it gives is in the queue).
    pub fn finished(&self) -> bool {
        self.finished.load(Ordering::SeqCst)
    }

    /// The time the run reached, and its end, s.
    pub fn progress(&self) -> (f64, f64) {
        (
            f64::from_bits(self.reached.load(Ordering::Relaxed)),
            f64::from_bits(self.t_end.load(Ordering::Relaxed)),
        )
    }

    /// The channels the points carry, by name.
    pub fn channel_names(&self) -> Vec<String> {
        locked(&self.channels).iter().map(|(n, _, _)| n.clone()).collect()
    }

    /// The points put since the last call: waits up to `timeout` for the
    /// first while the run goes.
    pub fn take(&self, timeout: Duration) -> Vec<Point> {
        let mut q = locked(&self.queue);
        if q.is_empty() && !self.finished() {
            q = self.put.wait_timeout(q, timeout).unwrap_or_else(|e| e.into_inner()).0;
        }
        let out: Vec<Point> = q.drain(..).collect();
        if !out.is_empty() {
            self.taken.notify_all();
        }
        out
    }

    /// A parameter change for the run's next step: why not, when it takes
    /// no more.
    pub fn give(&self, e: ParamEdit) -> Result<(), String> {
        if self.fixed.load(Ordering::SeqCst) {
            return Err("the acceleration test has reached its finish and runs to it again with \
                        the changes made before it: this change takes effect on the next run"
                .into());
        }
        locked(&self.edits).push(e);
        Ok(())
    }

    /// Sets the channels the points carry: today's that the run records.
    fn serve(&self, built: &Built) {
        let vars = &built.model.info.var_names;
        *locked(&self.channels) = built
            .rep
            .channels
            .iter()
            .filter_map(|(name, c)| {
                vars.iter().position(|v| *v == c.var).map(|i| (name.clone(), i, c.clone()))
            })
            .collect();
    }

    /// The run ended: no more points (a reader waiting is woken).
    pub fn end(&self) {
        self.finished.store(true, Ordering::SeqCst);
        let _q = locked(&self.queue);
        self.put.notify_all();
    }

    fn push(&self, p: Point) {
        let mut q = locked(&self.queue);
        while q.len() >= self.room && !self.cancelled() && !self.closed.load(Ordering::SeqCst) {
            q = self.taken.wait(q).unwrap_or_else(|e| e.into_inner());
        }
        // (a run told to stop, or with no reader, waits for none)
        if q.len() < self.room {
            q.push_back(p);
            self.put.notify_all();
        }
    }
}

impl RunControl for Control {
    fn stop(&self) -> bool {
        self.cancelled()
    }

    fn reached(&self, t: f64, t_end: f64, first: usize, times: &[f64], values: &[Vec<f64>]) {
        self.reached.store(t.to_bits(), Ordering::Relaxed);
        self.t_end.store(t_end.to_bits(), Ordering::Relaxed);
        if self.room == 0 || !self.streaming.load(Ordering::SeqCst) || first >= times.len() {
            return;
        }
        let channels = locked(&self.channels).clone();
        for (k, &tk) in times.iter().enumerate().skip(first) {
            let v = channels.iter().map(|(_, i, c)| c.unit.from_si(values[*i][k])).collect();
            self.push((tk, v));
        }
    }

    fn edits(&self) -> Vec<ParamEdit> {
        std::mem::take(&mut *locked(&self.edits))
    }
}

/// The whole parameter vector of a run after `changes` (engine names, SI),
/// on top of `set` (the changes made before, which grows by them): as
/// [`lsim_engine::Model::params_with`] gives it. A value bound to another
/// parameter follows that one, so it may be given only as the values
/// given make it.
pub fn params_after(
    built: &Built,
    set: &mut Vec<(String, f64)>,
    changes: &[(String, f64)],
) -> Result<Vec<f64>, String> {
    let flat = &built.model.prepared.flat;
    let bound = |name: &str| {
        flat.find_param(name).is_some_and(|id| flat.params[id.0 as usize].binding.is_some())
    };
    let mut all = set.clone();
    all.extend(changes.iter().filter(|(n, _)| !bound(n)).cloned());
    let params = built.model.params_with(&all)?;
    for (name, v) in changes.iter().filter(|(n, _)| bound(n)) {
        let id = flat.find_param(name).ok_or_else(|| format!("no parameter '{name}'"))?;
        let got = params[id.0 as usize];
        if got.to_bits() != v.to_bits() {
            return Err(format!(
                "'{name}' is bound to another parameter, which makes it {got} with the values \
                 given, not {v}: set that one"
            ));
        }
    }
    *set = all;
    Ok(params)
}

/// What a run takes besides the parameters.
#[derive(Clone, Debug)]
pub struct RunOptions {
    /// relative tolerance
    pub rtol: f64,
    /// absolute tolerance (times each variable's nominal value)
    pub atol: f64,
    /// output spacing, s; `None`: the case's
    pub output_step: Option<f64>,
    /// end time, s; `None`: the case's duration
    pub t_end: Option<f64>,
    /// a Script part the caller gives no function runs its own code on the
    /// engine's runner of a subset of Python (bitwise the app's Script
    /// runner's results); a part given a function always runs that
    pub native_scripts: bool,
    /// channels sampled from the dense output at times of their own
    /// ([`lsim_solve::SolverOptions::trace`]; [`trace_channels`] finds them)
    pub trace: Option<TraceSpec>,
    /// `(channel, window s)` whose highest moving average to keep
    /// ([`lsim_solve::SolverOptions::windows`])
    pub windows: Vec<(usize, f64)>,
    /// what stops the run, takes its points as it records them and its
    /// parameter changes while it goes ([`Control`])
    pub control: Option<Arc<Control>>,
    /// parameter changes at times of their own (`(t, the whole vector)`,
    /// [`params_after`] gives it): a run's live changes made again
    pub edits: Vec<(f64, Vec<f64>)>,
}

/// The most output points a run records.
pub const MAX_OUTPUT_POINTS: f64 = 1e7;

/// The most bytes a run's recorded results may take, every recorded
/// variable's value, min, max and mean at each output time: a grid that
/// would take more is refused before the run. (Reading them into Python
/// copies at most as much again; the copies are made with the room
/// asked for first, so too little room is a `MemoryError`.)
pub const MAX_RESULT_BYTES: f64 = 4e9;

impl Default for RunOptions {
    fn default() -> Self {
        // the golden comparison's: IDA with the energy books on
        RunOptions {
            rtol: 1e-6,
            atol: 1e-8,
            output_step: None,
            t_end: None,
            native_scripts: true,
            trace: None,
            windows: vec![],
            control: None,
            edits: vec![],
        }
    }
}

impl RunOptions {
    fn check(&self) -> Result<(), Failure> {
        let positive = |name: &str, v: f64| {
            if v.is_finite() && v > 0.0 {
                Ok(())
            } else {
                Err(Failure::Value(format!("{name} must be a positive number, not {v}.")))
            }
        };
        positive("rtol", self.rtol)?;
        positive("atol", self.atol)?;
        if let Some(v) = self.output_step {
            positive("output_step", v)?;
        }
        if let Some(v) = self.t_end {
            positive("t_end", v)?;
        }
        if let Some(tr) = &self.trace {
            positive("trace_step", tr.step)?;
        }
        if let Some(&(_, w)) = self.windows.iter().find(|(_, w)| !(w.is_finite() && *w >= 0.0)) {
            return Err(Failure::Value(format!("a window must be 0 s or more, not {w}.")));
        }
        Ok(())
    }

    /// Whether the output grid from 0 to `t_end` every `output_step` (the
    /// caller's or the case's own) has at most [`MAX_OUTPUT_POINTS`] and
    /// its results for `variables` recorded variables take at most
    /// [`MAX_RESULT_BYTES`].
    pub fn check_grid(t_end: f64, output_step: f64, variables: usize) -> Result<(), Failure> {
        let points = (t_end / output_step).ceil() + 1.0;
        let bytes = points * variables as f64 * 4.0 * 8.0;
        if points.is_nan() || points > MAX_OUTPUT_POINTS || bytes > MAX_RESULT_BYTES {
            return Err(Failure::Value(format!(
                "An output every {output_step} s up to {t_end} s makes {points:.3e} output points, \
                 whose results ({variables} variables' value, min, max and mean at each) would \
                 take {:.3} GB; a run records at most {MAX_OUTPUT_POINTS:.0e} points and {:.0} GB: \
                 give a larger output_step or a shorter t_end.",
                bytes / 1e9,
                MAX_RESULT_BYTES / 1e9
            )));
        }
        Ok(())
    }
}

/// One finished run.
pub struct RunOutput {
    /// the engine's results
    pub result: SimResult,
    /// the summary figures by today's definitions
    pub figures: Vec<Figure>,
    /// the parameter values the run used (SI, in `Model::param_names()` order)
    pub params: Vec<f64>,
    /// wall-clock time of the run(s), s
    pub wall_seconds: f64,
    /// whether an acceleration test ran again to its finish
    pub reran: bool,
    /// the end time asked for, s
    pub t_end: f64,
    /// the output spacing, s
    pub output_step: f64,
    /// each Script part's label and the runner it ran on (`"native"`, its
    /// own code on the engine's runner of the Python subset, or
    /// `"python"`, the caller's function)
    pub scripts: Vec<(String, &'static str)>,
    /// the names the trace was asked for by, in its order
    pub trace_names: Option<Vec<String>>,
    /// the names the windows were asked for by, in their order
    pub window_names: Vec<String>,
    /// each parameter change made ([`ParamEdit`]): when the run took it,
    /// when it made it, the values it went on with; an acceleration test
    /// run again to its finish: its first run's, made before the finish
    /// (`simulate(edits=...)` with the times taken makes them again)
    pub edits: Vec<(f64, f64, Vec<f64>)>,
}

/// A run's sampled blocks' hosts.
pub struct Hosts {
    /// the hosts, in the run loop's order
    pub blocks: Vec<Box<dyn DiscreteBlock>>,
    /// each Script part's label and runner
    pub scripts: Vec<(String, &'static str)>,
    /// for each part on the native runner, its id and the flag it sets when
    /// it meets something outside its subset
    pub outside: Vec<(String, std::sync::Arc<std::sync::atomic::AtomicBool>)>,
}

/// A Script block's host, its runner, and its native runner's flag.
pub type ScriptHost =
    (Box<dyn DiscreteBlock>, &'static str, Option<std::sync::Arc<std::sync::atomic::AtomicBool>>);

/// Makes the hosts of a run's sampled blocks (fresh for every run).
/// (`native`: Script blocks may run on the engine's own runner).
pub type HostFactory<'a> = dyn Fn(&Built, bool) -> Result<Hosts, Failure> + Sync + 'a;

/// Makes the host of a Script block from its spec (`native` as above).
pub type ScriptMaker<'a> = dyn Fn(&SampledSpec, bool) -> Result<ScriptHost, Failure> + 'a;

/// The hosts of a model's sampled blocks, in `info.blocks` order: the
/// Traction Control block is today's rule in Rust, a Script block is made
/// by `script` (the one place a Script block's host is chosen), an FMU is
/// not supported yet.
pub fn block_hosts(
    built: &Built,
    script: &ScriptMaker<'_>,
    native: bool,
) -> Result<Hosts, Failure> {
    let mut out = Hosts { blocks: vec![], scripts: vec![], outside: vec![] };
    for b in &built.model.info.blocks {
        let spec = sampled_spec(&built.rep, &b.name).ok_or_else(|| {
            Failure::Value(format!("The engine has no host for the sampled block {}.", b.name))
        })?;
        match &spec.kind {
            SampledKind::Script { .. } => {
                let (host, runner, flag) = script(spec, native)?;
                out.blocks.push(host);
                out.scripts.push((spec.label.clone(), runner));
                out.outside.extend(flag.map(|f| (spec.element.clone(), f)));
            }
            SampledKind::TractionControl => out.blocks.push(Box::new(TractionHost::new(spec))),
            SampledKind::Fmu { .. } => {
                return Err(model_error(
                    "RUN-FMU",
                    format!(
                        "The FMU part '{}' cannot run yet: the engine does not run FMUs so far.",
                        spec.label
                    ),
                ));
            }
        }
    }
    Ok(out)
}

/// Runs the case with the parameter values `params` (the whole vector, as
/// [`lsim_engine::Model::params_with`] gives it): the output grid is the
/// case's, and an acceleration test runs again to the moment it crosses
/// the finish, where today's run ends (as `golden::run_case`). A Script
/// part on the native runner that meets what the runner does not run
/// fails the run plainly ([`script_outside`]): it has no function to run
/// on instead.
pub fn run(
    built: &Built,
    params: &[f64],
    o: &RunOptions,
    hosts: &HostFactory<'_>,
) -> Result<RunOutput, Failure> {
    o.check()?;
    guarded(|| {
        let run = &built.rep.run;
        let t_end = o.t_end.unwrap_or(run.duration);
        let output_step = o.output_step.unwrap_or_else(|| run.output_dt());
        RunOptions::check_grid(t_end, output_step, built.model.info.var_names.len())?;
        if let Some(tr) = &o.trace {
            RunOptions::check_grid(t_end, tr.step, tr.channels.len())?;
        }
        // the run's control: the caller's, or one for the changes at times
        // of their own alone. An acceleration test that runs again to its
        // finish passes on the points of its last run only (the first's
        // after the finish are not the result's); the changes its first run
        // made before the finish, at their times, are its second's too
        let ctl =
            o.control.clone().or_else(|| (!o.edits.is_empty()).then(|| Arc::new(Control::new(0))));
        let again = run.timing.is_some();
        if let Some(c) = &ctl {
            c.serve(built);
            c.streaming.store(!again && o.control.is_some(), Ordering::SeqCst);
            for (t, p) in &o.edits {
                c.give(ParamEdit { from: *t, at: None, params: p.clone() })
                    .map_err(Failure::Value)?;
            }
        }
        let opts = SolverOptions {
            rtol: o.rtol,
            atol: o.atol,
            trace: o.trace.clone(),
            windows: o.windows.clone(),
            control: ctl.clone().map(|c| c as Arc<dyn RunControl>),
            ..Default::default()
        };
        let scripts = std::cell::RefCell::new(vec![]);
        let go = |t_end: f64| -> Result<SimResult, Failure> {
            let grid = OutputGrid { t0: 0.0, t_end, dt: output_step };
            let mut h = hosts(built, o.native_scripts)?;
            let r = built.model.simulate_with(params, &opts, grid, &mut h.blocks);
            *scripts.borrow_mut() = h.scripts;
            let left = h.outside.iter().find(|(_, f)| f.load(std::sync::atomic::Ordering::SeqCst));
            match (r, left) {
                (Err(RunError::Solve(SolveError::Block { message, .. })), Some((element, _))) => {
                    Err(script_outside(element, message))
                }
                (r, _) => r.map_err(run_failure),
            }
        };
        let started = Instant::now();
        let first = go(t_end)?;
        // (a run stopped before it ends runs no more)
        let t_fin = run.timing.as_ref().filter(|_| first.report.stopped.is_none()).and_then(|tm| {
            first
                .channel(&tm.t_finish)
                .and_then(|c| c.last().copied())
                .filter(|t| *t > 0.0 && *t < t_end)
        });
        let mut edits = first.report.edits.clone();
        let (result, reran) = match t_fin {
            // (the first run's results go before the second's are made)
            Some(t_fin) => {
                edits.retain(|(_, made, _)| *made < t_fin);
                if let Some(c) = &ctl {
                    // (the first run's changes before the finish, at their
                    // times; no others: they would move the finish)
                    let _ = locked(&c.edits).drain(..);
                    for (_, made, p) in &edits {
                        let again = ParamEdit { from: 0.0, at: Some(*made), params: p.clone() };
                        c.give(again).map_err(Failure::Value)?;
                    }
                    c.fixed.store(true, Ordering::SeqCst);
                    c.streaming.store(o.control.is_some(), Ordering::SeqCst);
                }
                drop(first);
                (go(t_fin)?, true)
            }
            None => {
                // the first run is the result: its points now
                if let Some(c) = ctl.as_ref().filter(|_| again && o.control.is_some()) {
                    c.fixed.store(true, Ordering::SeqCst);
                    c.streaming.store(true, Ordering::SeqCst);
                    let t = first.times.last().copied().unwrap_or(0.0);
                    c.reached(t, t, 0, &first.times, &first.values);
                }
                (first, false)
            }
        };
        let wall_seconds = started.elapsed().as_secs_f64();
        // (the figures read the parameters the run ended with)
        let last = result.report.edits.last().map_or(params, |(_, _, p)| p.as_slice());
        let flat = &built.model.prepared.flat;
        let param =
            |name: &str| flat.find_param(name).and_then(|id| last.get(id.0 as usize).copied());
        let figures = figures_with(&built.project, &built.rep, &param, &result);
        Ok(RunOutput {
            result,
            figures,
            params: params.to_vec(),
            wall_seconds,
            reran,
            t_end,
            output_step,
            scripts: scripts.into_inner(),
            trace_names: None,
            window_names: vec![],
            edits,
        })
    })
}

/// Runs `f(i)` for `i` in `0..n` on up to `threads` threads; results in
/// order.
pub fn par_map<T: Send>(n: usize, threads: usize, f: impl Fn(usize) -> T + Sync) -> Vec<T> {
    let threads = threads.clamp(1, n.max(1));
    if threads == 1 {
        return (0..n).map(f).collect();
    }
    let next = std::sync::atomic::AtomicUsize::new(0);
    let slots: Vec<std::sync::Mutex<Option<T>>> =
        (0..n).map(|_| std::sync::Mutex::new(None)).collect();
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if i >= n {
                        break;
                    }
                    let r = f(i);
                    *slots[i].lock().unwrap_or_else(|e| e.into_inner()) = Some(r);
                }
            });
        }
    });
    slots
        .into_iter()
        .map(|m| m.into_inner().unwrap_or_else(|e| e.into_inner()).expect("every index ran"))
        .collect()
}

/// Results that do not fit in the memory left: what was being copied.
#[derive(Debug)]
pub struct NoRoom(pub String);

/// `f` of each of `v`, in room asked for first: [`NoRoom`] instead of an
/// abort when there is not enough left.
pub fn copied(
    v: &[f64],
    f: impl Fn(f64) -> f64,
    what: impl FnOnce() -> String,
) -> Result<Vec<f64>, NoRoom> {
    let mut out = Vec::new();
    out.try_reserve_exact(v.len())
        .map_err(|e| NoRoom(format!("no memory left for {} ({} values): {e}", what(), v.len())))?;
    out.extend(v.iter().map(|x| f(*x)));
    Ok(out)
}

/// A channel as today's app has it: the engine's variable and the display
/// unit's conversion (each a copy made in room asked for first).
pub fn today_channels(
    built: &Built,
    res: &SimResult,
    which: Series,
) -> Result<Vec<(String, Vec<f64>)>, NoRoom> {
    let mut out = vec![];
    for (name, c) in &built.rep.channels {
        let Some(i) = res.names.iter().position(|n| *n == c.var) else { continue };
        let v = match which {
            Series::Values => &res.values[i],
            Series::Min => &res.min[i],
            Series::Max => &res.max[i],
            Series::Mean => &res.mean[i],
        };
        out.push((
            name.clone(),
            copied(v, |x| c.unit.from_si(x), || format!("the channel {name}"))?,
        ));
    }
    Ok(out)
}

/// The flat variables of `names`, each today's channel name
/// (`element:port`) or the engine's own variable name, for a trace.
pub fn trace_channels(built: &Built, names: &[String]) -> Result<Vec<usize>, Failure> {
    let vars = &built.model.info.var_names;
    names
        .iter()
        .map(|n| {
            let var = built.rep.channels.get(n).map_or(n.as_str(), |c| c.var.as_str());
            vars.iter().position(|v| v == var).ok_or_else(|| {
                Failure::Value(format!(
                    "trace: '{n}' is neither a channel (element:port) nor a variable of the case"
                ))
            })
        })
        .collect()
}

/// A traced value in the unit the caller named it in: today's display
/// unit for a channel, SI for an engine variable.
pub fn trace_unit<'a>(built: &'a Built, name: &str) -> impl Fn(f64) -> f64 + 'a {
    let unit = built.rep.channels.get(name).map(|c| &c.unit);
    move |x| unit.map_or(x, |u| u.from_si(x))
}

/// Each of today's channels (in its display unit) and each of the engine's
/// variables (by its own name, SI) over the run: ∫ x dt (from the
/// intervals' means), ∫ x² dt and ∫ |x| dt (the recorder's, of the line
/// through every point the integrator visited), and the highest and
/// lowest value (the intervals' extremes). ∫ |x| needs a unit without an
/// offset (not °C): NaN otherwise.
pub fn integrals(built: &Built, res: &SimResult) -> Vec<(String, [f64; 5])> {
    let span = res.times.last().copied().unwrap_or(0.0) - res.times.first().copied().unwrap_or(0.0);
    let of = |i: usize, a: f64, b: f64| -> Option<[f64; 5]> {
        let (&sq, &mag) = (res.square_integral.get(i)?, res.magnitude_integral.get(i)?);
        let si: f64 =
            (1..res.times.len()).map(|k| res.mean[i][k] * (res.times[k] - res.times[k - 1])).sum();
        // (display = a·SI + b, a > 0)
        let hi = res.max[i].iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let lo = res.min[i].iter().copied().fold(f64::INFINITY, f64::min);
        // a value that is not a number anywhere is so here too (fold's max drops it)
        let nan = res.max[i].iter().chain(&res.min[i]).any(|x| x.is_nan());
        let (hi, lo) = if nan { (f64::NAN, f64::NAN) } else { (a * hi + b, a * lo + b) };
        Some([
            a * si + b * span,
            a * a * sq + 2.0 * a * b * si + b * b * span,
            if b == 0.0 { a.abs() * mag } else { f64::NAN },
            hi,
            lo,
        ])
    };
    let mut out = vec![];
    for (name, c) in &built.rep.channels {
        let Some(i) = res.names.iter().position(|n| *n == c.var) else { continue };
        let b = c.unit.from_si(0.0);
        out.extend(of(i, c.unit.from_si(1.0) - b, b).map(|v| (name.clone(), v)));
    }
    for (i, name) in res.names.iter().enumerate() {
        out.extend(of(i, 1.0, 0.0).map(|v| (name.clone(), v)));
    }
    out
}

/// The part (element id) a table, a part's path or another dotted name of
/// the engine belongs to: the element whose instance name it starts with.
pub fn part_of<'a>(built: &'a Built, name: &str) -> Option<&'a str> {
    built
        .rep
        .instances
        .iter()
        .filter(|(_, inst)| name == inst.as_str() || name.starts_with(&format!("{inst}.")))
        .max_by_key(|(_, inst)| inst.len())
        .map(|(id, _)| id.as_str())
}

/// The Script parts of the run (each block the run's loop drives that is a
/// Script), in its order.
pub fn script_parts(built: &Built) -> Vec<&SampledSpec> {
    built
        .model
        .info
        .blocks
        .iter()
        .filter_map(|b| sampled_spec(&built.rep, &b.name))
        .filter(|s| matches!(s.kind, SampledKind::Script { .. }))
        .collect()
}

/// Whether each of `keys` (of the caller's `scripts`) is the id or the
/// label of a Script part, and no part is named by two of them (its id and
/// its label): a function given for no part, or the second for a part,
/// would be set aside unsaid.
pub fn check_script_keys<'a>(
    built: &Built,
    keys: impl Iterator<Item = &'a String>,
) -> Result<(), Failure> {
    let parts = script_parts(built);
    let keys: Vec<&String> = keys.collect();
    let mut unknown: Vec<String> = keys
        .iter()
        .filter(|k| !parts.iter().any(|p| p.element == ***k || p.label == ***k))
        .map(|k| format!("'{k}'"))
        .collect();
    if unknown.is_empty() {
        for p in &parts {
            if p.element != p.label && keys.contains(&&p.element) && keys.contains(&&p.label) {
                return Err(Failure::Value(format!(
                    "scripts: '{}' and '{}' name the Script part '{}' ({}) twice, by its id and \
                     by its label; give it one function, under one of them.",
                    p.element, p.label, p.element, p.label
                )));
            }
        }
        return Ok(());
    }
    unknown.sort();
    let there = if parts.is_empty() {
        "the case has no Script parts".to_string()
    } else {
        let parts: Vec<String> =
            parts.iter().map(|p| format!("'{}' ({})", p.element, p.label)).collect();
        format!("its Script parts are {}", parts.join(", "))
    };
    Err(Failure::Value(format!(
        "scripts: {} names no Script part of the case (by its id or label); {there}.",
        unknown.join(", ")
    )))
}

/// Which series of a run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Series {
    /// the values at the output times
    Values,
    /// the lowest value over each output interval
    Min,
    /// the highest value over each output interval
    Max,
    /// the time-mean over each output interval
    Mean,
}
