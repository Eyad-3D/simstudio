//! The golden comparison's runner: every reference run of today's engine in
//! a folder (made by `golden/reference.py`) against the new engine.
//!
//! ```text
//! cargo run --release -p lsim-project --example golden -- REF_DIR OUT_DIR [project[/case] ...]
//! ```
//!
//! `LSIM_CONDITION_KERNELS=0` interprets every condition the run loop
//! checks along its steps instead of using the model's compiled kernels.
//!
//! Script blocks run today's own script runner: set `LSIM_PYTHON` to the
//! Python that runs today's engine (the repository's `backend` folder is
//! found from this crate). Writes `OUT_DIR/golden.json` and
//! `OUT_DIR/golden.md`.

use lsim_project::golden::{self, PythonHost, Row};
use lsim_solve::SolverOptions;
use serde_json::{Value, json};
use std::fmt::Write as _;
use std::path::PathBuf;

fn fmt(v: f64) -> String {
    if v == 0.0 {
        "0".into()
    } else if v.abs() >= 1e4 || v.abs() < 1e-3 {
        format!("{v:.4e}")
    } else {
        format!("{v:.6}")
    }
}

fn row_json(r: &Row) -> Value {
    json!({"what": r.what, "key": r.key, "unit": r.unit, "today_10ms": r.normal, "today_1ms": r.fine,
           "band": r.band, "new": r.new, "diff": r.diff, "inside": r.inside})
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("usage: golden REF_DIR OUT_DIR [project[/case] ...]");
        std::process::exit(2);
    }
    let (refs, out) = (PathBuf::from(&args[0]), PathBuf::from(&args[1]));
    let only: Vec<&String> = args[2..].iter().collect();
    std::fs::create_dir_all(&out).expect("output folder");
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let python = std::env::var("LSIM_PYTHON")
        .ok()
        .map(|p| PythonHost { python: PathBuf::from(p), backend: repo.join("backend") });
    let rtol: f64 = std::env::var("LSIM_RTOL").ok().and_then(|s| s.parse().ok()).unwrap_or(1e-6);
    // `LSIM_CONDITION_KERNELS=0`: every condition interpreted (the
    // kernels' acceptance compares the two)
    let kernels = std::env::var("LSIM_CONDITION_KERNELS").map_or(true, |v| v != "0");
    let solver =
        SolverOptions { rtol, atol: rtol * 1e-2, condition_kernels: kernels, ..Default::default() };
    let mut all = vec![];
    let mut md = String::new();
    for ((project, case), path) in golden::references(&refs) {
        let tag = format!("{project}/{case}");
        if !only.is_empty() && !only.iter().any(|o| **o == project || **o == tag) {
            continue;
        }
        let reference = golden::read_reference(&path).expect("reference");
        let pj: Value = serde_json::from_str(
            &std::fs::read_to_string(repo.join(format!("backend/projects/{project}.json")))
                .expect("project"),
        )
        .expect("JSON");
        eprintln!("{tag} …");
        let run = match golden::run_case(&pj, &case, &solver, python.as_ref()) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("{tag}: {e}");
                let _ = writeln!(md, "## {tag}\n\nThe new engine did not run it: {e}\n");
                all.push(json!({"project": project, "case": case, "error": e}));
                continue;
            }
        };
        // the new run's channels under today's names, in today's units
        let mut chans_new = serde_json::Map::new();
        for (name, c) in &run.report.channels {
            if let Some(v) = run.result.channel(&c.var) {
                chans_new.insert(
                    name.clone(),
                    json!(v.iter().map(|x| c.unit.from_si(*x)).collect::<Vec<_>>()),
                );
            }
        }
        std::fs::write(
            out.join(format!("{project}__{case}.new.json")),
            serde_json::to_string(&json!({"times": run.result.times, "channels": chans_new,
                "figures": run.figures.iter().map(|f| json!({"key": f.key, "label": f.label, "value": f.value, "unit": f.unit})).collect::<Vec<_>>()}))
            .unwrap(),
        )
        .unwrap();
        let figs = golden::compare_figures(&reference, &run.figures);
        let chans = golden::compare_channels(&reference, &run);
        let books = run.result.energy.as_ref();
        // today's gear shifts: the kinetic energy a shift loses (its tyres'
        // slip relaxed at once), less the impulse through the tyres times
        // their slip before the shift (booked to the tyres), under the
        // gearboxes' "gear shifts" term, kWh
        let today_shifts: f64 = reference["fine"]["part_energy"]
            .as_array()
            .map(|parts| {
                parts.iter().filter_map(|p| p["terms"]["gear shifts"].as_f64()).sum::<f64>()
            })
            .unwrap_or(0.0)
            + 0.0; // (an empty sum is −0)
        let _ = writeln!(md, "## {tag} ({})\n", reference["name"].as_str().unwrap_or(""));
        let _ = writeln!(
            md,
            "New engine: build {:.2} s, run {:.2} s ({} steps, {} events); today: {:.1} s at 10 ms, {:.1} s at 1 ms. {}\n",
            run.build_seconds,
            run.run_seconds,
            run.result.stats.steps,
            run.result.events.len(),
            reference["normal"]["wall_seconds"].as_f64().unwrap_or(0.0),
            reference["fine"]["wall_seconds"].as_f64().unwrap_or(0.0),
            books
                .map(|b| {
                    // the tyres pass no impulse: what their slip loses
                    // after a shift is their own slip loss, over time
                    let links = if b.impulse_link_loss != 0.0 {
                        format!(
                            ", {:.6} kWh in couplings that passed the impulse on",
                            b.impulse_link_loss / 3.6e6
                        )
                    } else {
                        String::new()
                    };
                    let shifts = match run.result.report.impulses {
                        0 => String::new(),
                        n => format!(
                            " ({n} gear shifts: {:.6} kWh as the gears engaged{links}; today's gearbox term {today_shifts:.6} kWh)",
                            (b.impulse_loss - b.impulse_link_loss) / 3.6e6,
                        ),
                    };
                    format!(
                        "Energy books: closure {:.1e} of the throughput, {:.4} kWh lost at events{shifts}.",
                        b.relative_closure,
                        b.event_loss / 3.6e6,
                    )
                })
                .unwrap_or_default()
        );
        let _ = writeln!(
            md,
            "| figure | unit | today 10 ms | today 1 ms | band | new | new − 1 ms | |"
        );
        let _ = writeln!(md, "|---|---|---:|---:|---:|---:|---:|---|");
        for r in &figs {
            let _ = writeln!(
                md,
                "| {} | {} | {} | {} | ±{} | {} | {} | {} |",
                r.what,
                r.unit,
                fmt(r.normal),
                fmt(r.fine),
                fmt(r.band),
                fmt(r.new),
                fmt(r.diff),
                if r.inside { "inside" } else { "**outside**" }
            );
        }
        let inside = chans.iter().filter(|r| r.inside).count();
        let _ = writeln!(
            md,
            "\nChannels: {inside} of {} inside their bands (RMS of the difference to today's 1 ms run).\n",
            chans.len()
        );
        let mut outside: Vec<&Row> = chans.iter().filter(|r| !r.inside).collect();
        outside.sort_by(|a, b| (b.diff / b.band).total_cmp(&(a.diff / a.band)));
        if !outside.is_empty() {
            let _ = writeln!(
                md,
                "| channel outside | unit | RMS today 1 ms | band | RMS new − 1 ms | × band |"
            );
            let _ = writeln!(md, "|---|---|---:|---:|---:|---:|");
            for r in outside {
                let _ = writeln!(
                    md,
                    "| {} | {} | {} | {} | {} | {:.1} |",
                    r.what,
                    r.unit,
                    fmt(r.fine),
                    fmt(r.band),
                    fmt(r.diff),
                    r.diff / r.band
                );
            }
            let _ = writeln!(md);
        }
        all.push(json!({
            "project": project, "case": case,
            "build_seconds": run.build_seconds, "run_seconds": run.run_seconds,
            "steps": run.result.stats.steps, "events": run.result.events.len(),
            "energy_closure": books.map(|b| b.relative_closure),
            "energy_event_loss_kwh": books.map(|b| b.event_loss / 3.6e6),
            "energy_shift_loss_kwh": books.map(|b| b.impulse_loss / 3.6e6),
            "energy_shift_gear_kwh": books.map(|b| (b.impulse_loss - b.impulse_link_loss) / 3.6e6),
            "energy_shift_tyre_kwh": books.map(|b| b.impulse_link_loss / 3.6e6),
            "today_shift_gear_kwh": today_shifts,
            "gear_shifts": run.result.report.impulses,
            "light_restarts": run.result.report.light_restarts,
            "inert_ticks": run.result.report.inert_ticks,
            "block_changes": run.result.report.block_changes,
            "figures": figs.iter().map(row_json).collect::<Vec<_>>(),
            "channels": chans.iter().map(row_json).collect::<Vec<_>>(),
        }));
        std::fs::write(out.join("golden.json"), serde_json::to_string_pretty(&all).unwrap())
            .unwrap();
        std::fs::write(out.join("golden.md"), &md).unwrap();
    }
    std::fs::write(out.join("golden.json"), serde_json::to_string_pretty(&all).unwrap()).unwrap();
    std::fs::write(out.join("golden.md"), &md).unwrap();
}
