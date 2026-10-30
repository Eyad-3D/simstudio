//! The part of the Python module that knows nothing of Python: importing a
//! project's case, building it, and running it exactly as the golden
//! comparison does (`lsim_project::golden::run_case`), with the engine's
//! front door (`lsim_engine`) instead of the plain path. Kept free of
//! PyO3 so that Rust tests can check it against the golden path.

use lsim_engine::{BuildOptions, BuildReport, Engine, RunError};
use lsim_ir::Diagnostic;
use lsim_ir::runtime::DiscreteBlock;
use lsim_project::golden::{Figure, TractionHost, figures_with, sampled_spec};
use lsim_project::import::{ImportOptions, ImportReport, SampledKind, SampledSpec};
use lsim_project::{import_case, standard_registry};
use lsim_solve::{OutputGrid, SimResult, SolveError, SolverOptions};
use serde_json::Value;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::time::Instant;

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
    table_cells: lsim_engine::TableCells,
) -> Result<Built, Failure> {
    guarded(|| {
        let t0 = Instant::now();
        let (project, top, rep) = import(project_json, case_id, cycles_dir)?;
        let import_seconds = t0.elapsed().as_secs_f64();
        let engine = Engine::new(rep.library());
        let model = engine
            .build(&top, &BuildOptions { cache, table_cells, ..Default::default() })
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
        RunOptions { rtol: 1e-6, atol: 1e-8, output_step: None, t_end: None, native_scripts: true }
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
        let opts = SolverOptions { rtol: o.rtol, atol: o.atol, ..Default::default() };
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
        let t_fin = run.timing.as_ref().and_then(|tm| {
            first
                .channel(&tm.t_finish)
                .and_then(|c| c.last().copied())
                .filter(|t| *t > 0.0 && *t < t_end)
        });
        let (result, reran) = match t_fin {
            // (the first run's results go before the second's are made)
            Some(t_fin) => {
                drop(first);
                (go(t_fin)?, true)
            }
            None => (first, false),
        };
        let wall_seconds = started.elapsed().as_secs_f64();
        let flat = &built.model.prepared.flat;
        let param =
            |name: &str| flat.find_param(name).and_then(|id| params.get(id.0 as usize).copied());
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
