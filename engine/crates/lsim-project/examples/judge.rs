//! The outside judge's runner: one judge case (a model in the text format
//! and how to run it) at every tolerance it asks for, its results and the
//! solver's work written as JSON. `engine/judge/judge.py` writes the cases
//! from published problems, runs this once per case and scores the results
//! against the problems' own reference solutions (`engine/judge/README.md`).
//!
//! ```text
//! cargo run --release -p lsim-project --example judge -- CASE_JSON OUT_JSON
//! ```
//!
//! The case: `{"text": "<file>", "model": "<name>", "t_end": s, "dt": s,
//! "rtols": [..], "atol_factor": a, "record": ["<channel>", ..],
//! "repeat": n, "options": {"energy_books": bool, "backend": "sundials" |
//! "diffsol", "method": "auto" | "bdf" | "adams", "max_steps": n,
//! "table_cells": "jumps" | "kinks" | "off"}}`, the text's path relative
//! to the case file (`table_cells`: which tables read at a state are held
//! in their cells, `lsim_prep::hold::TableCells`; "jumps", the engine's
//! own, by default). Each run uses atol = a · rtol
//! (times each variable's nominal value, as always) and is timed `repeat`
//! times (the median is reported); a run that fails is reported with the
//! engine's own message.

use lsim_lang::{Report, parse_library};
use lsim_solve::{Backend, Method, OutputGrid, RunInfo, SimResult, SolverOptions};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::Instant;

fn median(mut xs: Vec<f64>) -> f64 {
    xs.sort_by(f64::total_cmp);
    xs[xs.len() / 2]
}

/// The recorded channels over the grid: the values after any event at an
/// output time, and where an event falls on one, the values before it.
fn series(r: &SimResult, record: &[String]) -> Value {
    let mut chans = serde_json::Map::new();
    let mut before = serde_json::Map::new();
    for name in record {
        let Some(c) = r.names.iter().position(|n| n == name) else { continue };
        chans.insert(name.clone(), json!(r.values[c]));
        let left: Vec<(usize, f64)> = r
            .left_limits
            .iter()
            .filter_map(|l| l.get(c).map(|v| (l.k, v)))
            .filter(|&(k, v)| v.to_bits() != r.values[c][k].to_bits())
            .collect();
        if !left.is_empty() {
            before.insert(name.clone(), json!(left));
        }
    }
    json!({"times": r.times, "channels": chans, "before": before})
}

fn options(case: &Value, rtol: f64) -> SolverOptions {
    let o = &case["options"];
    let mut opts = SolverOptions {
        rtol,
        atol: rtol * case["atol_factor"].as_f64().unwrap_or(1e-2),
        ..Default::default()
    };
    if let Some(b) = o["energy_books"].as_bool() {
        opts.energy_books = b;
    }
    if let Some(n) = o["max_steps"].as_u64() {
        opts.max_steps = n;
    }
    if let Some(f) = o["state_switch"].as_f64() {
        opts.state_switch = f;
    }
    match o["backend"].as_str() {
        Some("diffsol") => opts.backend = Backend::Diffsol,
        Some("sundials") | None => {}
        Some(other) => panic!("unknown backend {other}"),
    }
    match o["method"].as_str() {
        Some("bdf") => opts.method = Method::Bdf,
        Some("adams") => opts.method = Method::Adams,
        Some("auto") | None => {}
        Some(other) => panic!("unknown method {other}"),
    }
    opts
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        eprintln!("usage: judge CASE_JSON OUT_JSON");
        std::process::exit(2);
    }
    let case_path = PathBuf::from(&args[0]);
    let case: Value = serde_json::from_str(&std::fs::read_to_string(&case_path).expect("case"))
        .expect("case JSON");
    let dir = case_path.parent().unwrap_or(Path::new("."));
    let text = std::fs::read_to_string(dir.join(case["text"].as_str().expect("text")))
        .expect("model text");
    let name = case["model"].as_str().expect("model");
    let record: Vec<String> = case["record"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let repeat = case["repeat"].as_u64().unwrap_or(1).max(1);
    let grid = OutputGrid {
        t0: case["t0"].as_f64().unwrap_or(0.0),
        t_end: case["t_end"].as_f64().expect("t_end"),
        dt: case["dt"].as_f64().expect("dt"),
    };

    let write = |v: Value| {
        std::fs::write(&args[1], serde_json::to_string(&v).unwrap()).expect("output");
    };
    // read, prepare and compile once
    let started = Instant::now();
    let mut lib = lsim_lib::library();
    let parsed = match parse_library(&text, Some(&lib)) {
        Ok(p) => p,
        Err(e) => {
            write(json!({"stage": "parse", "error": format!("{}", Report(&e))}));
            return;
        }
    };
    lib.types.extend(parsed.types);
    for def in parsed.components.into_values() {
        lib.add(def);
    }
    let Some(model) = lib.components.get(name).cloned() else {
        write(json!({"stage": "parse", "error": format!("no model {name}")}));
        return;
    };
    let popts = lsim_prep::PrepOptions {
        table_cells: match case["options"]["table_cells"].as_str() {
            Some("kinks") => lsim_prep::hold::TableCells::Kinks,
            Some("off") => lsim_prep::hold::TableCells::Off,
            Some("jumps") | None => lsim_prep::hold::TableCells::Jumps,
            Some(other) => panic!("unknown table_cells {other}"),
        },
        ..Default::default()
    };
    let prepared = match lsim_prep::prepare(&lib, &model, &popts) {
        Ok(p) => p,
        Err(d) => {
            let msgs: Vec<String> = d.iter().map(|x| format!("{x}")).collect();
            write(json!({"stage": "prepare", "error": msgs.join("\n")}));
            return;
        }
    };
    let prepare_s = started.elapsed().as_secs_f64();
    let compiled = Instant::now();
    let code = match lsim_codegen::compile(&prepared, &Default::default()) {
        Ok(c) => c,
        Err(e) => {
            write(json!({"stage": "compile", "error": format!("{e}")}));
            return;
        }
    };
    let compile_s = compiled.elapsed().as_secs_f64();
    let mut info = RunInfo::from_prepared(&prepared);
    // dynamic state selection, unless the case asks for the states chosen
    // at the start only
    if case["options"]["dynamic_states"].as_bool() != Some(false) {
        info.choices = lsim_project::model::choices(&lib, &model, &popts, &info);
    }
    let states: Vec<&str> =
        prepared.states.iter().map(|v| prepared.flat.var(*v).name.as_str()).collect();

    let mut runs = vec![];
    for rtol in case["rtols"].as_array().expect("rtols").iter().filter_map(Value::as_f64) {
        let opts = options(&case, rtol);
        let mut walls = vec![];
        let mut first: Option<Result<SimResult, String>> = None;
        for _ in 0..repeat {
            let t = Instant::now();
            let r = lsim_solve::simulate(&code, &info, &opts, grid, &mut []);
            walls.push(t.elapsed().as_secs_f64());
            let failed = r.is_err();
            if first.is_none() {
                first = Some(r.map_err(|e| format!("{e}")));
            }
            if failed {
                break;
            }
        }
        let wall = median(walls);
        runs.push(match first.expect("a run") {
            Ok(r) => {
                let books = r.energy.as_ref().map(|b| {
                    json!({"closure": b.relative_closure, "error_controlled": b.error_controlled})
                });
                json!({
                    "rtol": rtol, "atol": opts.atol, "ok": true, "wall_s": wall,
                    "sim_wall_s": r.wall_seconds,
                    "steps": r.stats.steps, "rhs_evals": r.stats.rhs_evals,
                    "jac_evals": r.stats.jac_evals, "err_test_fails": r.stats.err_test_fails,
                    "nonlin_fails": r.stats.nonlin_fails, "restarts": r.stats.restarts,
                    "events": r.events.len(),
                    "switches": r.events.iter()
                        .filter(|e| matches!(e.kind, lsim_solve::EventKind::States(_))).count(),
                    "event_times": r.events.iter().map(|e| e.t).collect::<Vec<_>>(),
                    "event_labels": r.events.iter().map(|e| e.label.clone()).collect::<Vec<_>>(),
                    "backend": r.backend, "method": r.report.method,
                    "notes": r.report.notes, "warnings": r.report.warnings,
                    "worst_local": r.report.error.worst_local,
                    "worst_local_var": r.report.error.worst_local_var,
                    "books": books,
                    "final": r.names.iter().zip(&r.values)
                        .map(|(n, v)| (n.clone(), json!(v.last().copied())))
                        .collect::<serde_json::Map<_, _>>(),
                    "series": series(&r, &record),
                })
            }
            Err(e) => {
                json!({"rtol": rtol, "atol": opts.atol, "ok": false, "error": e, "wall_s": wall})
            }
        });
    }
    write(json!({
        "stage": "run", "prepare_s": prepare_s, "compile_s": compile_s,
        "states": states, "n_y": info.y_names.len(), "runs": runs,
    }));
}
