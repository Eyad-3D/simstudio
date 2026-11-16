//! What the fuzz targets check (benchmarks/fuzz/PLAN.md): every input ends
//! in a result or a coded refusal, and a run that succeeds holds no NaN or
//! infinity. A breach panics, which libFuzzer reports as a crash.

use lsim_engine::{BuildOptions, Engine, Model, RunError};
use lsim_ir::runtime::DiscreteBlock;
use lsim_ir::{ComponentDef, Diagnostic, Library};
use lsim_project::import::{ImportOptions, ImportReport};
use lsim_project::{import_case, standard_registry};
use lsim_solve::{OutputGrid, SimResult, SolverOptions};
use serde_json::Value;
use std::sync::OnceLock;
use std::time::Duration;

/// With `LSIM_FUZZ_TRACE` set, says what the harness did (to triage an
/// input: `LSIM_FUZZ_TRACE=1 target file`).
pub fn trace(what: &str) {
    if std::env::var_os("LSIM_FUZZ_TRACE").is_some() {
        eprintln!("[lsim-fuzz] {what}");
    }
}

/// The limits of a run inside the harness.
pub fn solver() -> SolverOptions {
    SolverOptions {
        time_limit: Some(Duration::from_secs(10)),
        memory_limit: Some(256 << 20),
        ..Default::default()
    }
}

/// The standard library (made once).
pub fn std_lib() -> &'static Library {
    static LIB: OnceLock<Library> = OnceLock::new();
    LIB.get_or_init(lsim_lib::library)
}

/// A refusal: some diagnostics, each with a code and a message.
pub fn coded(d: &[Diagnostic]) {
    assert!(!d.is_empty(), "a refusal with no diagnostic");
    for x in d {
        assert!(!x.code.trim().is_empty() && !x.message.trim().is_empty(), "uncoded: {x:?}");
    }
}

/// A refusal of the text format: each error with a code and a message.
pub fn lang_coded(e: &[lsim_lang::LangError]) {
    assert!(!e.is_empty(), "a refusal with no error");
    for x in e {
        assert!(!x.code.trim().is_empty() && !x.message.trim().is_empty(), "uncoded: {x:?}");
    }
}

/// A plain refusal of a value (the Python module's ValueError).
pub fn plain(m: &str) {
    assert!(!m.trim().is_empty(), "a refusal with no message");
}

/// A successful run: no NaN or infinity in any channel's value, minimum,
/// maximum or mean.
pub fn finite(r: &SimResult) {
    for (c, name) in r.names.iter().enumerate() {
        for (what, col) in
            [("value", &r.values), ("min", &r.min), ("max", &r.max), ("mean", &r.mean)]
        {
            if let Some(k) = col.get(c).and_then(|v| v.iter().position(|x| !x.is_finite())) {
                panic!(
                    "NaN as success: channel {name}'s {what} is {} at t = {}",
                    col[c][k],
                    r.times.get(k).copied().unwrap_or(f64::NAN)
                );
            }
        }
    }
}

/// Runs `model` at `params` over `grid` and checks the outcome.
pub fn run(model: &Model, params: &[f64], grid: OutputGrid, blocks: &mut [Box<dyn DiscreteBlock>]) {
    let t0 = std::time::Instant::now();
    let r = model.simulate_with(params, &solver(), grid, blocks);
    trace(&format!(
        "run {:.3} s: {}",
        t0.elapsed().as_secs_f64(),
        match &r {
            Ok(r) => format!("ok, {} steps", r.stats.steps),
            Err(e) => e.to_string(),
        }
    ));
    match r {
        Ok(r) => finite(&r),
        // each kind of SolveError is a code of its own (RUN-…)
        Err(RunError::Solve(e)) => plain(&e.to_string()),
        Err(RunError::Param(m) | RunError::Mode(m)) => plain(&m),
        Err(RunError::Fast(e)) => plain(&e.to_string()),
    }
}

/// Builds `top` against `lib` and runs it for `t_end` s (a model with
/// sampled blocks, which needs their hosts, is built only).
pub fn build_and_run(lib: Library, top: &ComponentDef, t_end: f64) {
    let t0 = std::time::Instant::now();
    let built = Engine::new(lib).build(top, &BuildOptions::default());
    trace(&format!("build {:.3} s: {}", t0.elapsed().as_secs_f64(), built.is_ok()));
    match built {
        Err(d) => coded(&d),
        Ok(model) if model.info.blocks.is_empty() => run(
            &model,
            &model.info.params,
            OutputGrid { t0: 0.0, t_end, dt: t_end / 50.0 },
            &mut [],
        ),
        Ok(_) => {}
    }
}

/// A case's short run: at most 2 s at its own output step; `None` where
/// the Python module would refuse the case's own grid (a duration or step
/// that is not a positive number).
pub fn short_grid(rep: &ImportReport) -> Option<OutputGrid> {
    let (d, dt) = (rep.run.duration, rep.run.output_dt());
    (d.is_finite() && d > 0.0 && dt.is_finite() && dt > 0.0).then(|| OutputGrid {
        t0: 0.0,
        t_end: d.min(2.0),
        dt,
    })
}

/// Imports a project's case, builds it and makes its short run, as the
/// Python module does (Script parts on the native runner).
pub fn project_case(project: &Value, case_id: Option<&str>) {
    let reg = standard_registry();
    let (top, rep) = match import_case(project, case_id, &reg, &ImportOptions::default()) {
        Err(d) => return coded(&d),
        Ok(x) => x,
    };
    let model = match Engine::new(rep.library()).build(&top, &BuildOptions::default()) {
        Err(d) => return coded(&d),
        Ok(m) => m,
    };
    run_case(&model, &rep, &model.info.params);
}

/// The short run of a built case at `params`.
pub fn run_case(model: &Model, rep: &ImportReport, params: &[f64]) {
    let Some(grid) = short_grid(rep) else { return };
    let hosts = lsim_project::golden::hosts_for(
        &model.info,
        rep,
        None,
        lsim_project::golden::ScriptRunner::Native,
    );
    match hosts {
        Err(m) => plain(&m),
        Ok(mut h) => run(model, params, grid, &mut h.blocks),
    }
}

/// The four example projects, parsed.
pub fn examples() -> &'static [(String, Value)] {
    static P: OnceLock<Vec<(String, Value)>> = OnceLock::new();
    P.get_or_init(|| {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../backend/projects");
        ["aero-bev", "bev-car", "fs-electric", "hybrid-car"]
            .iter()
            .map(|n| {
                let text = std::fs::read_to_string(format!("{dir}/{n}.json")).expect("project");
                (n.to_string(), serde_json::from_str(&text).expect("json"))
            })
            .collect()
    })
}

/// A project's case ids.
pub fn case_ids(project: &Value) -> Vec<String> {
    project["cases"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| c["id"].as_str().map(String::from))
        .collect()
}
