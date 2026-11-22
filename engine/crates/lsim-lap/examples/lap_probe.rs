//! A lap's profile from the command line, for tuning and checking the lap
//! solver (not a test):
//!
//! ```text
//! cargo run --release -p lsim-lap --example lap_probe -- PROJECT.json CASE TRACK.json [V_START]
//! ```
//!
//! `TRACK.json` holds `kappa`, `sin_t` (one value a point), `ds` and
//! `closed` (backend `lapsim.load_track`'s). Prints the lap time, the time
//! held by each code and cause, the point solver's work and the run time.

use lsim_engine::{BuildOptions, Engine};
use lsim_lap::{LapOptions, Parts, State, Track, drive, solve};
use lsim_project::{ImportOptions, import_case, standard_registry};
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::Instant;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let pj: Value = serde_json::from_str(&std::fs::read_to_string(&a[0]).unwrap()).unwrap();
    let tj: Value = serde_json::from_str(&std::fs::read_to_string(&a[2]).unwrap()).unwrap();
    let v_start: f64 = a.get(3).map(|s| s.parse().unwrap()).unwrap_or(0.0);
    let nums = |k: &str| -> Vec<f64> {
        tj[k].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect()
    };
    let track = Track {
        kappa: nums("kappa"),
        sin_t: nums("sin_t"),
        ds: tj["ds"].as_f64().unwrap(),
        closed: tj["closed"].as_bool().unwrap(),
    };
    let opts = ImportOptions { inverse: true, lap: true, ..Default::default() };
    let (top, rep) = import_case(&pj, Some(&a[1]), &standard_registry(), &opts).expect("import");
    let pr = rep.inverse.clone().unwrap();
    let lp = rep.lap.clone().unwrap();
    let model =
        Engine::new(rep.library()).build_inverse(&top, &pr.spec, &BuildOptions::default()).unwrap();
    let parts = Parts {
        track: lp.track,
        driver: lp.driver,
        wheels: lp.wheels,
        motors: lp.motors,
        batteries: lp.batteries,
    };
    let opt = LapOptions { v_start, more: true, ..Default::default() };
    let t0 = Instant::now();
    let p = solve(&model, &model.info.params, &parts, &track, &opt, None).expect("lap");
    let wall = t0.elapsed().as_secs_f64();
    let mut held: BTreeMap<String, f64> = BTreeMap::new();
    for i in 0..p.code.len() {
        *held.entry(format!("{:?}/{:?}", p.code[i], p.cause[i])).or_default() +=
            p.t[i + 1] - p.t[i];
    }
    println!(
        "lap {:.4} s, v_cap {:.4} m/s, {:.3} s to solve, {:?}",
        p.t[p.t.len() - 1],
        p.v_cap,
        wall,
        p.stats
    );
    for (k, s) in held {
        println!("  {k:<24} {s:8.3} s");
    }
    // the energy pass along it, laps of it on (each from the last one's state)
    let laps: usize = std::env::var("LAPS").ok().and_then(|s| s.parse().ok()).unwrap_or(1);
    let (q, _) = model.books_quantities();
    let mut state: Option<State> = None;
    let mut v0 = v_start;
    for k in 0..laps {
        let opt = LapOptions { v_start: v0, more: k + 1 < laps, ..Default::default() };
        let t1 = Instant::now();
        let mut prof =
            solve(&model, &model.info.params, &parts, &track, &opt, state.as_ref()).expect("lap");
        // another speed profile on the same points (PROFILE: a JSON file of
        // `t` and `v`), driven in its place
        if let Ok(f) = std::env::var("PROFILE") {
            let pj: Value = serde_json::from_str(&std::fs::read_to_string(f).unwrap()).unwrap();
            let get = |k: &str| -> Vec<f64> {
                pj[k].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect()
            };
            prof.t = get("t");
            prof.v = get("v");
            assert_eq!(prof.t.len(), track.kappa.len());
        }
        let t_prof = t1.elapsed().as_secs_f64();
        let t2 = Instant::now();
        let r = match drive(&model, &model.info.params, &track, &prof, state.as_ref(), &q) {
            Ok(r) => r,
            Err(e) => {
                println!("lap {k}: {e}");
                // the energy pass up to CUT s, the battery around its limit
                let cut: f64 =
                    std::env::var("CUT").ok().and_then(|s| s.parse().ok()).unwrap_or(0.2);
                let n = prof.t.iter().position(|t| *t > cut).unwrap_or(prof.t.len()).max(2);
                let mut pc = prof.clone();
                pc.t.truncate(n);
                pc.v.truncate(n);
                let tc = Track {
                    kappa: track.kappa[..n].to_vec(),
                    sin_t: track.sin_t[..n].to_vec(),
                    ..track.clone()
                };
                let r = drive(&model, &model.info.params, &tc, &pc, state.as_ref(), &q)
                    .expect("cut pass");
                let names: Vec<&String> = r
                    .names
                    .iter()
                    .filter(|n| {
                        n.contains("battery")
                            && (n.ends_with("sig_power")
                                || n.contains("p_check")
                                || n.contains("p_cap")
                                || n.contains("p_deliver"))
                    })
                    .collect();
                if let Ok(path) = std::env::var("DUMP") {
                    let pat = std::env::var("LISTPAT").unwrap_or_default();
                    let mut m = serde_json::Map::new();
                    m.insert("t".into(), serde_json::json!(r.times));
                    for nm in r.names.iter().filter(|n| pat.split(',').any(|p| n.contains(p))) {
                        m.insert(nm.clone(), serde_json::json!(r.channel(nm).unwrap()));
                    }
                    std::fs::write(path, serde_json::Value::Object(m).to_string()).unwrap();
                }
                if let Ok(pat) = std::env::var("LISTPAT") {
                    for nm in r.names.iter().filter(|n| pat.split(',').any(|p| n.contains(p))) {
                        let c = r.channel(nm).unwrap();
                        println!("  {nm}: {:?}", &c[c.len().saturating_sub(3)..]);
                    }
                }
                for nm in &names {
                    let c = r.channel(nm).unwrap();
                    println!("  {nm}: {:?}", &c[c.len().saturating_sub(6)..]);
                }
                println!(
                    "  codes {:?} causes {:?} v {:?}",
                    &prof.code[..n.min(prof.code.len())],
                    &prof.cause[..n.min(prof.cause.len())],
                    &prof.v[..n]
                );
                return;
            }
        };
        let t_drive = t2.elapsed().as_secs_f64();
        let b = model.books(&r);
        if std::env::var("ENERGIES").is_ok() {
            // each energy state's change over the lap, J
            for (j, nm) in r.names.iter().enumerate() {
                let leaf = nm.rsplit('.').next().unwrap_or("");
                if leaf.starts_with("e_") || leaf.starts_with("E_") {
                    let c = r.column(&r.values, j);
                    println!("    {nm:<40} {:12.1} J", c[c.len() - 1] - c[0]);
                }
            }
        }
        let e_out = r.channel("el_battery.e_delivered").map(|c| c[c.len() - 1]).unwrap_or(f64::NAN);
        let soc = r.channel("el_battery.soc").map(|c| c[c.len() - 1]).unwrap_or(f64::NAN);
        println!(
            "lap {k}: {:.4} s, profile {t_prof:.3} s, energy pass {t_drive:.3} s ({} points, {} steps, {} events, {} flags); \
             battery out {:.4} kWh (cumulative), SOC {:.5}; books supplied {:.1} J lost {:.1} J stored {:.1} J closure {:.3e} of throughput {:.4e}",
            prof.t[prof.t.len() - 1],
            r.times.len(),
            r.report.steps,
            r.events.len(),
            r.flags.len(),
            e_out / 3.6e6,
            soc,
            b.supplied,
            b.lost,
            b.stored_change,
            b.closure.abs() / b.throughput,
            b.throughput
        );
        let mut by: BTreeMap<String, (usize, f64, f64)> = BTreeMap::new();
        for f in &r.flags {
            let e =
                by.entry(format!("{} {}", f.part, if f.upper { "+" } else { "-" })).or_default();
            e.0 += 1;
            e.1 += f.t_end - f.t_start;
            e.2 = e.2.max(f.worst_excess);
        }
        let mut big: Vec<_> =
            r.flags.iter().filter(|f| f.worst_excess > 50.0 && f.part.contains("wheel")).collect();
        big.sort_by(|a, b| b.worst_excess.total_cmp(&a.worst_excess));
        for f in big.iter().take(8) {
            let i = prof.t.iter().position(|t| *t >= f.t_start).unwrap_or(0);
            println!(
                "    big {:.3}..{:.3} s (point {i}, v {:.2}, kappa {:.4}, code {:?}) {} {} {:.1} {}",
                f.t_start,
                f.t_end,
                prof.v[i],
                track.kappa[i],
                prof.code.get(i),
                f.part,
                if f.upper { "+" } else { "-" },
                f.worst_excess,
                f.unit
            );
        }
        for (k, (n, s, w)) in by {
            println!("    flags {k:<20} {n:>4} stretches, {s:7.3} s, worst {w:.4}");
        }
        v0 = prof.v[prof.v.len() - 1];
        state = Some(State { y: r.end_y, d: r.end_d });
    }
}
