//! The share of a run's time that the mixed conditions' check takes (the
//! conditions that read time and continuous variables, checked along every
//! step: DESIGN.md §8.2), with the condition-steps a certificate cleared
//! and those searched along the dense output, so that a regression shows:
//!
//! ```text
//! cargo run --release -p lsim-project --example scan_share -- [RUNS] [project/case ...]
//! ```
//!
//! By default the BEV's WLTC, five runs. Each run times the check with two
//! clock readings a step (`SolverOptions::time_mixed_checks`, which costs
//! well under 1 % of the run); the share is the check's time over the
//! run's. The last line per case is JSON: the median and the least share
//! of the runs and the counters. Script blocks run today's own script
//! runner: set `LSIM_PYTHON` as for the golden comparison (the BEV needs
//! none). Under callgrind (`RUNS` = 1) the check's instructions are those
//! of `scan_mixed`.

use lsim_project::golden::{self, PythonHost};
use lsim_solve::SolverOptions;
use serde_json::json;
use std::path::PathBuf;

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let runs = match args.first().and_then(|a| a.parse::<usize>().ok()) {
        Some(n) => {
            args.remove(0);
            n.max(1)
        }
        None => 5,
    };
    if args.is_empty() {
        args.push("bev-car/case-wltc".into());
    }
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let python = std::env::var("LSIM_PYTHON")
        .ok()
        .map(|p| PythonHost { python: PathBuf::from(p), backend: repo.join("backend") });
    // the golden comparison's tolerances
    let solver =
        SolverOptions { rtol: 1e-6, atol: 1e-8, time_mixed_checks: true, ..Default::default() };
    for case in &args {
        let Some((project, id)) = case.split_once('/') else {
            eprintln!("usage: scan_share [RUNS] [project/case ...]");
            std::process::exit(2);
        };
        let path = repo.join(format!("backend/projects/{project}.json"));
        let text = std::fs::read_to_string(&path).expect("the project's file");
        let pj: serde_json::Value = serde_json::from_str(&text).expect("the project's JSON");
        let mut shares = vec![];
        let mut counts = (0, 0, 0);
        for k in 1..=runs {
            let run = golden::run_case(&pj, id, &solver, python.as_ref())
                .unwrap_or_else(|e| panic!("{case}: {e}"));
            let (r, st, wall) = (&run.result.report, &run.result.stats, run.result.wall_seconds);
            let share = r.mixed_seconds / wall;
            println!(
                "{case} run {k}: {:.3} s, the check {:.3} s ({:.2} %), {} steps, \
                 certified {}, scanned {}, pulses {}",
                wall,
                r.mixed_seconds,
                100.0 * share,
                st.steps,
                r.mixed_certified,
                r.mixed_scanned,
                r.pulses_found
            );
            shares.push(share);
            counts = (st.steps, r.mixed_certified, r.mixed_scanned);
        }
        shares.sort_by(f64::total_cmp);
        let (steps, certified, scanned) = counts;
        println!(
            "{}",
            json!({
                "case": case,
                "runs": runs,
                "share_median": shares[shares.len() / 2],
                "share_least": shares[0],
                "steps": steps,
                "mixed_certified": certified,
                "mixed_scanned": scanned,
            })
        );
    }
}
