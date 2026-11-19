//! The Driver's map: a car's case on the new engine at every mass and on
//! every driving cycle asked for, each run's outcome, the solver's work
//! and its driven trace written as one JSON line
//! (`engine/judge/driver.py` scores the traces by 40 CFR 1066.425).
//!
//! ```text
//! cargo run --release -p lsim-project --example driver_map -- \
//!     PROJECT CASE OUT_JSONL --masses 1000,2000 --cycles wltc-3b,own [--dt 0.1]
//! ```
//!
//! PROJECT is a project file (`backend/projects/bev-car.json`), CASE one
//! of its cases. A cycle is a bundled cycle's id (`backend/app/cycles`),
//! run for its whole length, or `own`: the case's own driving task. The
//! car's mass is set on its vehicle body; the run is the golden
//! comparison's (rtol 1e-6, atol 1e-8, IDA, energy books), its outputs
//! every `--dt` s (0.1 by default; 1 s on cycles longer than 10 000 s).
//! The trace: the vehicle's speed, the driving task's target (km/h, to
//! 1e-4) and the driver's pedals (to 1e-9) at each output time, from `t0`
//! every `dt` s, `n` points; with `--vars a,b` also the model variables
//! whose names hold those texts (SI, the first such), and the events
//! counted by label (with `--window t0,t1` also those in that time, each
//! with its instant). A line is appended per run, so a map can be run in
//! parts. CPU time is the thread's own (Linux).

use lsim_project::golden;
use lsim_solve::SolverOptions;
use serde_json::{Value, json};
use std::io::Write as _;
use std::path::PathBuf;

/// The calling thread's CPU time, s (Linux: `/proc/thread-self/schedstat`).
fn cpu_seconds() -> f64 {
    std::fs::read_to_string("/proc/thread-self/schedstat")
        .ok()
        .and_then(|s| s.split_whitespace().next().and_then(|x| x.parse::<f64>().ok()))
        .map_or(f64::NAN, |ns| ns * 1e-9)
}

fn arg_list(args: &[String], flag: &str) -> Vec<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .map(|s| s.split(',').map(str::to_string).collect())
        .unwrap_or_default()
}

/// The ids of the case's parts of a kind (`vehicle.body`).
fn element_of(project: &Value, def: &str) -> Option<String> {
    let mut found = None;
    fn walk(v: &Value, def: &str, found: &mut Option<String>) {
        match v {
            Value::Object(m) => {
                if m.get("componentDefId").and_then(Value::as_str) == Some(def) && found.is_none() {
                    *found = m.get("id").and_then(Value::as_str).map(str::to_string);
                }
                m.values().for_each(|x| walk(x, def, found));
            }
            Value::Array(a) => a.iter().for_each(|x| walk(x, def, found)),
            _ => {}
        }
    }
    walk(&project["systems"], def, &mut found);
    found
}

/// A bundled cycle's length, s (its CSV's last time).
fn cycle_length(id: &str) -> Option<f64> {
    let path = lsim_project::import::bundled_cycles_dir().join(format!("{id}.csv"));
    let text = std::fs::read_to_string(path).ok()?;
    text.lines().filter_map(|l| l.split(',').next()?.trim().parse::<f64>().ok()).next_back()
}

/// A channel in its display unit at each output time, rounded to `digits`
/// decimals (the traces are long).
fn channel(run: &golden::CaseRun, key: &str, digits: i32) -> Value {
    let Some(c) = run.report.channels.get(key) else { return Value::Null };
    let Some(v) = run.result.channel(&c.var) else { return Value::Null };
    let scale = lsim_ir::math::powi(10.0, digits);
    json!(v.iter().map(|x| (c.unit.from_si(*x) * scale).round() / scale).collect::<Vec<f64>>())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        eprintln!(
            "usage: driver_map PROJECT CASE OUT_JSONL --masses m1,m2 --cycles c1,c2 [--dt s]"
        );
        std::process::exit(2);
    }
    let project: Value = serde_json::from_str(&std::fs::read_to_string(&args[0]).expect("project"))
        .expect("project JSON");
    let case_id = &args[1];
    let out = PathBuf::from(&args[2]);
    let masses: Vec<f64> =
        arg_list(&args, "--masses").iter().map(|m| m.parse().expect("a mass, kg")).collect();
    let cycles = arg_list(&args, "--cycles");
    let dt: f64 = arg_list(&args, "--dt").first().map_or(0.1, |s| s.parse().expect("dt"));
    let extra = arg_list(&args, "--vars");
    let window: Vec<f64> =
        arg_list(&args, "--window").iter().map(|x| x.parse().expect("a time, s")).collect();
    let vehicle = element_of(&project, "vehicle.body").expect("a vehicle body");
    let task = element_of(&project, "signal.driving_task").expect("a driving task");
    let driver = element_of(&project, "driver.driver").expect("a driver");
    let python = std::env::var("LSIM_PYTHON").ok().map(|p| golden::PythonHost {
        python: PathBuf::from(p),
        backend: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../backend"),
    });
    let solver = SolverOptions { rtol: 1e-6, atol: 1e-8, ..Default::default() };
    let mut file =
        std::fs::OpenOptions::new().create(true).append(true).open(&out).expect("output file");
    for cycle in &cycles {
        for &mass in &masses {
            let mut pj = project.clone();
            let cases = pj["cases"].as_array_mut().expect("cases");
            let case = cases.iter_mut().find(|c| c["id"] == **case_id).expect("the case");
            let over = case
                .as_object_mut()
                .expect("case")
                .entry("parameterOverrides")
                .or_insert_with(|| json!({}));
            over.as_object_mut()
                .expect("overrides")
                .entry(vehicle.clone())
                .or_insert_with(|| json!({}))["mass_kg"] = json!(mass);
            let mut length = case["duration"].as_f64().unwrap_or(0.0);
            if cycle != "own" {
                length = cycle_length(cycle).expect("a bundled cycle");
                case["parameterOverrides"]
                    .as_object_mut()
                    .expect("overrides")
                    .entry(task.clone())
                    .or_insert_with(|| json!({}))["cycle"] = json!(cycle);
                case["duration"] = json!(length);
            }
            let step = if length > 10_000.0 { dt.max(1.0) } else { dt };
            case["timeStep"] = json!(step);
            let cpu0 = cpu_seconds();
            let run = golden::run_case(&pj, case_id, &solver, python.as_ref());
            let cpu = cpu_seconds() - cpu0;
            let line = match run {
                Ok(run) => {
                    let r = &run.result;
                    let mut labels = serde_json::Map::new();
                    for e in &r.events {
                        let n = labels.entry(e.label.clone()).or_insert(json!(0));
                        *n = json!(n.as_u64().unwrap_or(0) + 1);
                    }
                    // a variable by its model name, or by a part of it
                    let vars = extra
                        .iter()
                        .filter_map(|v| {
                            let name =
                                r.names.iter().find(|x| *x == v || x.contains(v.as_str()))?;
                            Some((name.clone(), json!(r.channel(name)?)))
                        })
                        .collect::<serde_json::Map<_, _>>();
                    json!({
                        "cycle": cycle, "mass_kg": mass, "ok": true, "t_end": length,
                        "dt": step, "run_s": run.run_seconds, "cpu_s": cpu,
                        "steps": r.stats.steps, "events": r.events.len(),
                        "err_test_fails": r.stats.err_test_fails,
                        "nonlin_fails": r.stats.nonlin_fails, "restarts": r.stats.restarts,
                        "block_changes": r.report.block_changes,
                        "inert_ticks": r.report.inert_ticks,
                        "pulses_found": r.report.pulses_found,
                        "mixed_certified": r.report.mixed_certified,
                        "mixed_scanned": r.report.mixed_scanned,
                        "books_closure": r.energy.as_ref().map(|e| e.relative_closure),
                        "warnings": r.report.warnings,
                        "figures": run.figures.iter()
                            .map(|f| (f.key.clone(), json!(f.value)))
                            .filter(|(k, _)| !k.is_empty())
                            .collect::<serde_json::Map<_, _>>(),
                        "t0": r.times.first(), "n": r.times.len(),
                        "speed_kmh": channel(&run, &format!("{vehicle}:sig_speed"), 4),
                        "target_kmh": channel(&run, &format!("{task}:sig_demand"), 4),
                        "accel_pedal": channel(&run, &format!("{driver}:sig_accel_pedal"), 9),
                        "brake_pedal": channel(&run, &format!("{driver}:sig_brake_pedal"), 9),
                        "vars": vars, "event_labels": labels,
                        "events_in_window": r.events.iter()
                            .filter(|e| window.len() == 2 && e.t >= window[0] && e.t <= window[1])
                            .map(|e| json!([e.t, e.label]))
                            .collect::<Vec<_>>(),
                    })
                }
                Err(e) => json!({
                    "cycle": cycle, "mass_kg": mass, "ok": false, "t_end": length, "dt": step,
                    "cpu_s": cpu, "error": e,
                }),
            };
            writeln!(file, "{}", serde_json::to_string(&line).expect("JSON")).expect("write");
            eprintln!(
                "{cycle} {mass} kg: {} ({cpu:.2} s CPU)",
                if line["ok"] == true { "ran".to_string() } else { line["error"].to_string() }
            );
        }
    }
}
