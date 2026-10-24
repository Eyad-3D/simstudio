//! Fast mode on a project's drive-cycle cases, against the full dynamic
//! run of the same case: the outside judge's runner for fast mode
//! (`engine/judge/judge.py fast`).
//!
//! ```text
//! cargo run --release -p lsim-project --example fast -- [--forward] [--replay DT] PROJECT/CASE ...
//! ```
//!
//! `PROJECT` is an example project's name (`backend/projects/NAME.json`) or
//! a path to a project file. Each case is imported as fast mode's inverse
//! model (`ImportOptions::inverse`: the vehicle's speed follows the cycle
//! its Driver is given) and run over that cycle at 1 s steps; with
//! `--forward` the case also runs in full dynamic mode (rtol 1e-6, the
//! golden comparison's setting), with `--replay DT` fast mode runs again
//! on the speed that run drove, sampled every `DT` s (which takes the
//! driver's tracking out of the comparison), and with `--perturb REL` it
//! runs again on the cycle with every speed off by `REL` of itself, up or
//! down by turns (a change at round-off level, which must change nothing
//! but round-off: no flag, no event). Script blocks run on
//! lsim-script's native runner where their code is within its subset, else
//! on today's Python runner (`LSIM_PYTHON`, as for the golden comparison).
//!
//! Prints one JSON object per case: the battery's charge used and energies
//! (terminal energy out and back, internal losses: J), the distance (m),
//! the energy at the wall per km as EPA's reference suite counts it
//! (terminal energy net of recuperation, plus the battery's internal
//! losses, over a charger efficiency of 0.86: Wh/km), fast mode's steps,
//! events, flags and run time, and the same of the other runs.

use lsim_engine::{BuildOptions, Engine};
use lsim_fast::{BoundBlock, FastOptions, FastResult, Trace};
use lsim_project::golden::{self, PythonHost, ScriptRunner};
use lsim_project::{ImportOptions, import_case, standard_registry};
use lsim_solve::{OutputGrid, SimResult, SolverOptions};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::Instant;

/// EPA's reference suite's charger efficiency (backend/validation/suite.json).
const CHARGER: f64 = 0.86;

const BATTERY: [&str; 4] = [
    "el_battery.e_delivered",
    "el_battery.e_recuperated",
    "el_battery.e_losses",
    "el_vehicle.dist",
];

/// The energy figures of a run, from its last values.
fn figures(last: &dyn Fn(&str) -> Option<f64>, soc: (f64, f64)) -> Value {
    let get = |n: &str| last(n).unwrap_or(f64::NAN);
    let (out, back, loss, dist) =
        (get(BATTERY[0]), get(BATTERY[1]), get(BATTERY[2]), get(BATTERY[3]));
    json!({
        "charge_used": soc.0 - soc.1,
        "e_delivered_J": out,
        "e_recuperated_J": back,
        "e_losses_J": loss,
        "distance_m": dist,
        "wall_wh_per_km": (out - back + loss) / 3600.0 / CHARGER / (dist / 1000.0),
    })
}

fn fast_figures(r: &FastResult, wall: f64, samples: &[f64]) -> Value {
    let last = |n: &str| r.channel(n).and_then(|c| c.last().copied());
    let soc = r.channel("el_battery.soc").unwrap_or_default();
    let mut v = figures(
        &last,
        (soc.first().copied().unwrap_or(f64::NAN), soc.last().copied().unwrap_or(f64::NAN)),
    );
    // the events at a trace's samples (input events) and the others
    let at_sample =
        r.events.iter().filter(|e| samples.binary_search_by(|t| t.total_cmp(&e.t)).is_ok()).count();
    v["steps"] = json!(r.report.steps);
    v["events"] = json!(r.events.len());
    v["events_at_samples"] = json!(at_sample);
    v["flags"] = json!(
        r.flags
            .iter()
            .map(|f| format!("{:.1}..{:.1} {}", f.t_start, f.t_end, f.label))
            .collect::<Vec<_>>()
    );
    v["newton_restarts"] = json!(r.report.newton_restarts);
    v["newton_iterations"] = json!(r.report.newton_iterations);
    v["residual_evals"] = json!(r.report.residual_evals);
    v["step_redos"] = json!(r.report.step_redos);
    v["max_error"] = json!(r.report.max_error);
    v["run_s"] = json!(wall);
    v
}

fn forward_figures(r: &SimResult, wall: f64) -> Value {
    let last = |n: &str| r.channel(n).and_then(|c| c.last().copied());
    let soc = r.channel("el_battery.soc").unwrap_or(&[]);
    let mut v = figures(
        &last,
        (soc.first().copied().unwrap_or(f64::NAN), soc.last().copied().unwrap_or(f64::NAN)),
    );
    v["run_s"] = json!(wall);
    v
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let forward = args.iter().any(|a| a == "--forward");
    args.retain(|a| a != "--forward");
    let mut replay: Option<f64> = None;
    if let Some(i) = args.iter().position(|a| a == "--replay") {
        replay = Some(args[i + 1].parse().expect("--replay DT"));
        args.drain(i..i + 2);
    }
    let mut perturb: Option<f64> = None;
    if let Some(i) = args.iter().position(|a| a == "--perturb") {
        perturb = Some(args[i + 1].parse().expect("--perturb REL"));
        args.drain(i..i + 2);
    }
    if args.is_empty() {
        eprintln!("usage: fast [--forward] [--replay DT] [--perturb REL] PROJECT/CASE ...");
        std::process::exit(2);
    }
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let python = std::env::var("LSIM_PYTHON")
        .ok()
        .map(|p| PythonHost { python: PathBuf::from(p), backend: repo.join("backend") });
    let reg = standard_registry();
    let solver = SolverOptions { rtol: 1e-6, atol: 1e-8, ..Default::default() };
    for tag in &args {
        let (project, case) = tag.rsplit_once('/').expect("PROJECT/CASE");
        let path = if project.ends_with(".json") {
            PathBuf::from(project)
        } else {
            repo.join(format!("backend/projects/{project}.json"))
        };
        let pj: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("project")).expect("JSON");
        let mut out = json!({"project": project, "case": case});
        let fail = |mut out: Value, what: &str, e: String| {
            out[what] = json!({"error": e});
            println!("{out}");
        };
        let opts = ImportOptions { inverse: true, ..Default::default() };
        let (top, rep) = match import_case(&pj, Some(case), &reg, &opts) {
            Ok(x) => x,
            Err(d) => {
                fail(out, "fast", d.iter().map(|x| x.to_string()).collect::<Vec<_>>().join("; "));
                continue;
            }
        };
        let pr = rep.inverse.clone().expect("an inverse import prescribes the cycle");
        let trace = Trace::new(pr.t.clone(), pr.v.clone()).expect("the cycle");
        let engine = Engine::new(rep.library());
        let inv = match engine.build_inverse(&top, &pr.spec, &BuildOptions::default()) {
            Ok(m) => m,
            Err(d) => {
                fail(out, "fast", d.iter().map(|x| x.to_string()).collect::<Vec<_>>().join("; "));
                continue;
            }
        };
        let o =
            FastOptions { t_end: Some(rep.run.duration.min(trace.end())), ..Default::default() };
        // the channels the figures read (`figures`: the battery's, the
        // distance, its state of charge), by index among `names`
        let read = |names: &[String]| -> Vec<usize> {
            BATTERY
                .iter()
                .chain(["el_battery.soc"].iter())
                .filter_map(|w| names.iter().position(|n| n == w))
                .collect()
        };
        let run_fast = |trace: &Trace,
                        o: &FastOptions,
                        record: Option<&[usize]>|
         -> Result<(FastResult, f64), String> {
            let hosts = golden::hosts_for(&inv.info, &rep, python.as_ref(), ScriptRunner::Native)?;
            let mut blocks: Vec<BoundBlock> = hosts
                .blocks
                .into_iter()
                .zip(&inv.info.blocks)
                .map(|(block, b)| BoundBlock {
                    block,
                    inputs: b.inputs.clone(),
                    outputs: b.outputs.clone(),
                })
                .collect();
            let t0 = Instant::now();
            let r = inv
                .fast_with(&inv.info.params, std::slice::from_ref(trace), o, record, &mut blocks)
                .map_err(|e| e.to_string())?;
            Ok((r, t0.elapsed().as_secs_f64()))
        };
        match run_fast(&trace, &o, None) {
            Ok((r, wall)) => out["fast"] = fast_figures(&r, wall, &trace.t),
            Err(e) => out["fast"] = json!({"error": e}),
        }
        if let Some(rel) = perturb {
            let v: Vec<f64> =
                pr.v.iter()
                    .enumerate()
                    .map(|(k, v)| v * (1.0 + if k % 2 == 0 { rel } else { -rel }))
                    .collect();
            let shaken = Trace::new(pr.t.clone(), v).expect("the cycle, perturbed");
            match run_fast(&shaken, &o, None) {
                Ok((r, wall)) => out["perturbed"] = fast_figures(&r, wall, &shaken.t),
                Err(e) => out["perturbed"] = json!({"error": e}),
            }
        }
        if forward || replay.is_some() {
            let fwd = import_case(&pj, Some(case), &reg, &ImportOptions::default())
                .map_err(|d| d.iter().map(|x| x.to_string()).collect::<Vec<_>>().join("; "))
                .and_then(|(ftop, frep)| {
                    let m =
                        lsim_project::model::Model::build(&frep.library(), &ftop).map_err(|d| {
                            d.iter().map(|x| x.to_string()).collect::<Vec<_>>().join("; ")
                        })?;
                    Ok((m, frep))
                });
            match fwd {
                Ok((m, frep)) => {
                    let run =
                        |dt: f64, record: Option<Vec<usize>>| -> Result<(SimResult, f64), String> {
                            let mut h = golden::hosts_for(
                                &m.info,
                                &frep,
                                python.as_ref(),
                                ScriptRunner::Native,
                            )?;
                            let t0 = Instant::now();
                            let grid = OutputGrid { t0: 0.0, t_end: frep.run.duration, dt };
                            let opts = SolverOptions { record, ..solver.clone() };
                            let r = m.run(&opts, grid, &mut h.blocks).map_err(|e| e.to_string())?;
                            Ok((r, t0.elapsed().as_secs_f64()))
                        };
                    if forward {
                        match run(frep.run.output_dt(), None) {
                            Ok((r, wall)) => out["forward"] = forward_figures(&r, wall),
                            Err(e) => out["forward"] = json!({"error": e}),
                        }
                    }
                    if let Some(dt) = replay {
                        // (the channels the replay reads alone: every channel
                        // every 0.1 s over the long-haul cycle's 23 h did
                        // not fit in memory; the run itself is the same)
                        let mut keep = read(&m.info.var_names);
                        keep.extend(
                            m.info.var_names.iter().position(|n| *n == pr.spec.prescribed[0]),
                        );
                        match run(dt, Some(keep)) {
                            Ok((r, wall)) => {
                                out["forward_replayed"] = forward_figures(&r, wall);
                                // the speed it drove (its round-off below 0
                                // at rest held at 0: the cycles go forwards)
                                let v: Vec<f64> = r
                                    .channel(&pr.spec.prescribed[0])
                                    .unwrap_or(&[])
                                    .iter()
                                    .map(|v| v.max(0.0))
                                    .collect();
                                let driven =
                                    Trace::new(r.times.clone(), v).expect("the driven speed");
                                let o2 = FastOptions { step: dt, ..o.clone() };
                                let keep = read(&inv.info.var_names);
                                match run_fast(&driven, &o2, Some(&keep)) {
                                    Ok((rr, wall)) => {
                                        out["replay"] = fast_figures(&rr, wall, &driven.t)
                                    }
                                    Err(e) => out["replay"] = json!({"error": e}),
                                }
                            }
                            Err(e) => out["forward_replayed"] = json!({"error": e}),
                        }
                    }
                }
                Err(e) => out["forward"] = json!({"error": e}),
            }
        }
        println!("{out}");
    }
}
