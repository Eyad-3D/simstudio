//! The holds suite's runner (`judge.py holds`, `engine/judge/holds.py`):
//! every model of a cases file, read, prepared and compiled once, run at
//! each of its runs (parameters, tolerance, backend, ticks) on threads, one
//! JSON line per run. It is not part of the workspace: `judge.py holds
//! build` copies it into a checkout's `lsim-project/examples` for one
//! build and takes it out again, so that the same runner builds at every
//! commit the suite measures (the APIs it uses are those of 36c8a0d8 too).
//!
//! ```text
//! holds_runner CASES_JSON OUT_JSONL [THREADS]
//! ```
//!
//! A build: `{"id", "text", "model", "force_implicit", "t_end", "record":
//! [var], "grid": [var], "tick_vars": [var], "runs": [run]}`; a run:
//! `{"params": {name: value}, "rtol", "backend": "sundials" | "diffsol",
//! "ticks": [t], "max_step", "t_end"}` (atol = 1e-2 rtol; output every
//! 0.1 s). A line: `{"b", "r", "ok", "error", "fired": [[t, label]] (the
//! `when`s), "final": {var: value}, "grid": {var: [values]}, "backend",
//! "steps"}`.

use lsim_lang::{Report, parse_library};
use lsim_solve::{Backend, BlockInfo, EventKind, OutputGrid, RunInfo, SolverOptions, VarSource};
use serde_json::{Value, json};
use std::io::Write;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

/// One tick at `.0` that sets its output to 1.
struct Tick(f64);

impl lsim_ir::runtime::DiscreteBlock for Tick {
    fn name(&self) -> &str {
        "'Tick'"
    }
    fn period(&self) -> f64 {
        1e3
    }
    fn offset(&self) -> f64 {
        self.0
    }
    fn init(&mut self, _t: f64, _i: &[f64], _o: &mut [f64]) -> Result<(), String> {
        Ok(())
    }
    fn tick(&mut self, _t: f64, _i: &[f64], o: &mut [f64]) -> Result<(), String> {
        o[0] = 1.0;
        Ok(())
    }
}

fn strs(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
        .unwrap_or_default()
}

/// The build's model, compiled, or why not.
fn compile(
    base: &lsim_ir::Library,
    b: &Value,
) -> Result<(lsim_codegen::JitModel, RunInfo, Vec<String>), String> {
    let mut lib = base.clone();
    let parsed = parse_library(b["text"].as_str().ok_or("no text")?, Some(&lib))
        .map_err(|e| format!("parse: {}", Report(&e)))?;
    lib.types.extend(parsed.types);
    for def in parsed.components.into_values() {
        lib.add(def);
    }
    let name = b["model"].as_str().ok_or("no model")?;
    let model = lib.components.get(name).cloned().ok_or(format!("no model {name}"))?;
    let popts = lsim_prep::PrepOptions {
        force_implicit: b["force_implicit"].as_bool().unwrap_or(false),
        ..Default::default()
    };
    let prepared = lsim_prep::prepare(&lib, &model, &popts).map_err(|d| {
        format!("prepare: {}", d.iter().map(|x| format!("{x}")).collect::<Vec<_>>().join("; "))
    })?;
    let code = lsim_codegen::compile(&prepared, &Default::default())
        .map_err(|e| format!("compile: {e}"))?;
    let mut info = RunInfo::from_prepared(&prepared);
    let ticks = strs(&b["tick_vars"]);
    for v in &ticks {
        let p = info.var_names.iter().position(|x| x == v).ok_or(format!("no tick var {v}"))?;
        let VarSource::D(k) = info.var_sources[p] else { return Err(format!("{v} not discrete")) };
        info.blocks.push(BlockInfo {
            name: "'Tick'".into(),
            inputs: vec![],
            outputs: vec![k],
            period: 1e3,
            chains: vec![],
        });
    }
    let params = prepared.flat.params.iter().map(|p| p.name.clone()).collect();
    Ok((code, info, params))
}

fn run_build(base: &lsim_ir::Library, b: &Value) -> Vec<String> {
    let id = &b["id"];
    let runs = b["runs"].as_array().cloned().unwrap_or_default();
    let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| compile(base, b)))
        .unwrap_or_else(|_| Err("build panicked".into()));
    let (code, info, params) = match built {
        Ok(x) => x,
        Err(e) => {
            return (0..runs.len())
                .map(|r| json!({"b": id, "r": r, "ok": false, "error": e}).to_string())
                .collect();
        }
    };
    let record = strs(&b["record"]);
    let gridded = strs(&b["grid"]);
    let mut lines = vec![];
    for (ri, run) in runs.iter().enumerate() {
        let fail = |e: String| json!({"b": id, "r": ri, "ok": false, "error": e}).to_string();
        let mut inf = info.clone();
        let mut missing = vec![];
        if let Some(ps) = run["params"].as_object() {
            for (name, v) in ps {
                match params.iter().position(|p| p == name) {
                    Some(i) => inf.params[i] = v.as_f64().unwrap_or(f64::NAN),
                    None => missing.push(name.clone()),
                }
            }
        }
        if !missing.is_empty() {
            lines.push(fail(format!("no parameters {missing:?}")));
            continue;
        }
        let rtol = run["rtol"].as_f64().unwrap_or(1e-6);
        let mut opts = SolverOptions { rtol, atol: 1e-2 * rtol, ..Default::default() };
        if run["backend"].as_str() == Some("diffsol") {
            opts.backend = Backend::Diffsol;
        }
        if let Some(h) = run["max_step"].as_f64() {
            opts.max_step = h;
        }
        let t_end = run["t_end"].as_f64().or(b["t_end"].as_f64()).unwrap_or(2.0);
        let grid = OutputGrid { t0: 0.0, t_end, dt: 0.1 };
        let mut blocks: Vec<Box<dyn lsim_ir::runtime::DiscreteBlock>> = run["ticks"]
            .as_array()
            .map(|a| a.iter().filter_map(Value::as_f64).map(|t| Box::new(Tick(t)) as _).collect())
            .unwrap_or_default();
        if blocks.len() != inf.blocks.len() {
            lines.push(fail(format!("{} ticks for {} tick vars", blocks.len(), inf.blocks.len())));
            continue;
        }
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            lsim_solve::simulate(&code, &inf, &opts, grid, &mut blocks)
        }));
        let r = match res {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => {
                lines.push(fail(format!("{e}")));
                continue;
            }
            Err(_) => {
                lines.push(fail("panic".into()));
                continue;
            }
        };
        let fired: Vec<Value> = r
            .events
            .iter()
            .filter(|e| matches!(e.kind, EventKind::When(_)))
            .map(|e| json!([e.t, e.label]))
            .collect();
        let col = |v: &str| r.names.iter().position(|x| x == v);
        let fin: serde_json::Map<String, Value> = record
            .iter()
            .filter_map(|v| col(v).map(|c| (v.clone(), json!(r.values[c].last().copied()))))
            .collect();
        let grd: serde_json::Map<String, Value> = gridded
            .iter()
            .filter_map(|v| col(v).map(|c| (v.clone(), json!(r.values[c]))))
            .collect();
        lines.push(
            json!({"b": id, "r": ri, "ok": true, "fired": fired, "final": fin, "grid": grd,
                   "backend": r.backend, "steps": r.stats.steps})
            .to_string(),
        );
    }
    lines
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("usage: holds_runner CASES_JSON OUT_JSONL [THREADS]");
        std::process::exit(2);
    }
    let cases: Value =
        serde_json::from_str(&std::fs::read_to_string(&args[0]).expect("cases")).expect("JSON");
    let builds = cases["builds"].as_array().expect("builds");
    let threads: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(12);
    let out = Mutex::new(std::io::BufWriter::new(std::fs::File::create(&args[1]).expect("out")));
    let next = AtomicUsize::new(0);
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                let base = lsim_lib::library();
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(b) = builds.get(i) else { break };
                    let lines = run_build(&base, b);
                    let mut o = out.lock().unwrap();
                    for l in lines {
                        writeln!(o, "{l}").expect("write");
                    }
                }
            });
        }
    });
    out.into_inner().unwrap().flush().expect("flush");
}
