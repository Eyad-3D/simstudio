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
use lsim_lap::{LapOptions, Parts, Track, solve};
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
    let p = solve(&model, &model.info.params, &parts, &track, &opt).expect("lap");
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
}
