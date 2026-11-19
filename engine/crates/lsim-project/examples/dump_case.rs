//! Runs one case by the golden comparison's own path
//! (`golden::run_case`) and writes what it gives, for the Python module's
//! test to compare bit by bit:
//!
//! ```text
//! cargo run --release -p lsim-project --example dump_case -- PROJECT.json CASE OUT [PYTHON]
//! ```
//!
//! `OUT` is one header line of JSON (the channels' names under today's
//! names, the figures, the output count), then the output times and every
//! channel in today's units as little-endian 64-bit floats, one after
//! another. `PYTHON` is the interpreter that runs today's script runner for
//! Script blocks (the repository's `backend` folder is found from this
//! crate).

use lsim_project::golden::{self, PythonHost};
use lsim_solve::SolverOptions;
use serde_json::json;
use std::io::Write as _;
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        eprintln!("usage: dump_case PROJECT.json CASE OUT [PYTHON]");
        std::process::exit(2);
    }
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let python =
        args.get(3).map(|p| PythonHost { python: PathBuf::from(p), backend: repo.join("backend") });
    let project: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&args[0]).expect("project file"))
            .expect("project JSON");
    let solver = SolverOptions { rtol: 1e-6, atol: 1e-8, ..Default::default() };
    let run = golden::run_case(&project, &args[1], &solver, python.as_ref()).expect("the run");
    let res = &run.result;
    let mut names = vec![];
    let mut data: Vec<f64> = res.times.clone();
    for (name, c) in &run.report.channels {
        let Some(i) = res.names.iter().position(|n| *n == c.var) else { continue };
        names.push(name.clone());
        data.extend(res.values[i].iter().map(|v| c.unit.from_si(*v)));
    }
    let figures: Vec<_> = run
        .figures
        .iter()
        .map(|f| json!({"key": f.key, "label": f.label, "value": f.value, "unit": f.unit}))
        .collect();
    let header = json!({"points": res.times.len(), "channels": names, "figures": figures,
                        "steps": res.stats.steps});
    let mut out = std::fs::File::create(&args[2]).expect("output file");
    writeln!(out, "{header}").expect("write");
    for v in data {
        out.write_all(&v.to_le_bytes()).expect("write");
    }
}
