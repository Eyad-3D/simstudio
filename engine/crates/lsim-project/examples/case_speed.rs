//! The run time of example cases on the new engine (release build), as the
//! golden comparison runs them (rtol 1e-6, atol 1e-8, IDA, energy books):
//!
//! ```text
//! cargo run --release -p lsim-project --example case_speed -- project/case [repeats]
//! ```
//!
//! Prints the best of `repeats` runs (default 3), its multiple of real
//! time, and what the run did. Script blocks run on the runner
//! `LSIM_SCRIPTS` picks (native by default; `LSIM_PYTHON` for Python).

use lsim_project::golden::{self, PythonHost};
use lsim_solve::SolverOptions;
use serde_json::Value;
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let tag = args.first().expect("project/case");
    let repeats: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(3);
    let (project, case) = tag.split_once('/').expect("project/case");
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let python = std::env::var("LSIM_PYTHON")
        .ok()
        .map(|p| PythonHost { python: PathBuf::from(p), backend: repo.join("backend") });
    let pj: Value = serde_json::from_str(
        &std::fs::read_to_string(repo.join(format!("backend/projects/{project}.json")))
            .expect("project"),
    )
    .expect("JSON");
    let solver = SolverOptions { rtol: 1e-6, atol: 1e-8, ..Default::default() };
    let mut best = f64::INFINITY;
    for _ in 0..repeats {
        let run = golden::run_case(&pj, case, &solver, python.as_ref()).expect("the case runs");
        let r = &run.result;
        let duration =
            r.times.last().copied().unwrap_or(0.0) - r.times.first().copied().unwrap_or(0.0);
        best = best.min(run.run_seconds);
        println!(
            "{tag}: run {:.3} s = {:.0} x real time; {} steps, {} events, {:?}; scripts {:?}; books closure {:?}",
            run.run_seconds,
            duration / run.run_seconds,
            r.stats.steps,
            r.events.len(),
            r.stats,
            run.scripts,
            r.energy.as_ref().map(|e| e.relative_closure),
        );
    }
    println!("best {best:.3} s");
}
