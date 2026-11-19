//! Script blocks on the native runner against the app's own Script runner
//! (the golden host's Python process, called as the app calls it): each
//! case runs twice and every channel, interval statistic, event and count
//! must be bitwise the same.
//!
//! ```text
//! LSIM_PYTHON=$(which python3) cargo run --release -p lsim-project --example script_parity -- [project/case ...]
//! ```
//!
//! Without arguments: every case of the hybrid car (its Hybrid Control
//! Unit is a Script block). Prints each run's time on both runners.

use lsim_project::golden::{self, PythonHost, ScriptRunner};
use lsim_solve::{SimResult, SolverOptions};
use serde_json::Value;
use std::path::PathBuf;

fn same(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.to_bits() == y.to_bits())
}

fn compare(a: &SimResult, b: &SimResult) -> Vec<String> {
    let mut diffs = vec![];
    if !same(&a.times, &b.times) {
        diffs.push("output times".into());
    }
    for (c, name) in a.names.iter().enumerate() {
        for (what, x, y) in [
            ("value", &a.values, &b.values),
            ("min", &a.min, &b.min),
            ("max", &a.max, &b.max),
            ("mean", &a.mean, &b.mean),
        ] {
            if !same(&x[c], &y[c]) {
                diffs.push(format!("{name} ({what})"));
            }
        }
    }
    if a.left_limits.len() != b.left_limits.len()
        || a.left_limits
            .iter()
            .zip(&b.left_limits)
            .any(|(p, q)| format!("{p:?}") != format!("{q:?}"))
    {
        diffs.push("left limits at events".into());
    }
    if a.events.len() != b.events.len()
        || a.events
            .iter()
            .zip(&b.events)
            .any(|(p, q)| p.t.to_bits() != q.t.to_bits() || format!("{p:?}") != format!("{q:?}"))
    {
        diffs.push("events".into());
    }
    if format!("{:?}", a.stats) != format!("{:?}", b.stats) {
        diffs.push(format!("counts: {:?} vs {:?}", a.stats, b.stats));
    }
    if format!("{:?}", a.energy) != format!("{:?}", b.energy) {
        diffs.push("energy books".into());
    }
    diffs
}

fn main() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let python = PythonHost {
        python: PathBuf::from(std::env::var("LSIM_PYTHON").expect("LSIM_PYTHON: today's Python")),
        backend: repo.join("backend"),
    };
    let mut cases: Vec<String> = std::env::args().skip(1).collect();
    if cases.is_empty() {
        cases = ["case-hwfet", "case-mixed", "case-mixed-live", "case-udds"]
            .iter()
            .map(|c| format!("hybrid-car/{c}"))
            .collect();
    }
    let solver = SolverOptions { rtol: 1e-6, atol: 1e-8, ..Default::default() };
    let mut failed = false;
    for tag in cases {
        let (project, case) = tag.split_once('/').expect("project/case");
        let pj: Value = serde_json::from_str(
            &std::fs::read_to_string(repo.join(format!("backend/projects/{project}.json")))
                .expect("project"),
        )
        .expect("JSON");
        let py = golden::run_case_with(&pj, case, &solver, Some(&python), ScriptRunner::Python)
            .expect("Python run");
        let nat = golden::run_case_with(&pj, case, &solver, Some(&python), ScriptRunner::Native)
            .expect("native run");
        let diffs = compare(&py.result, &nat.result);
        let figs = py.figures == nat.figures;
        println!(
            "{tag}: runners {:?} vs {:?}; Python {:.2} s, native {:.2} s; {} steps, {} events; {}",
            py.scripts,
            nat.scripts,
            py.run_seconds,
            nat.run_seconds,
            nat.result.stats.steps,
            nat.result.events.len(),
            if diffs.is_empty() && figs {
                "bitwise the same".to_string()
            } else {
                format!("DIFFERENT: {diffs:?} figures same: {figs}")
            }
        );
        failed |= !diffs.is_empty() || !figs;
    }
    if failed {
        std::process::exit(1);
    }
}
