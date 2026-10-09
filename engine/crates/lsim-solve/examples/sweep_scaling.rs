//! Parallel sweeps (DESIGN.md §8.4): the 20-stage ladder drive over
//! 1800 s for 16 parameter sets, on 1, 2 and 4 threads; best of several
//! rounds, with the machine's load before and after and each run's CPU
//! time (which shows whether runs slow each other down, independently of
//! how many cores the shared machine had free).
//!
//! `cargo run --release -p lsim-solve --example sweep_scaling`

#[path = "shared/models.rs"]
mod models;

use lsim_solve::{OutputGrid, RunInfo, SolverOptions, sweep};
use std::time::Instant;

fn load() -> String {
    std::fs::read_to_string("/proc/loadavg")
        .unwrap_or_default()
        .split_whitespace()
        .take(3)
        .collect::<Vec<_>>()
        .join(" ")
}

/// CPU time of the whole process, s.
fn cpu() -> f64 {
    let s = std::fs::read_to_string("/proc/self/stat").unwrap_or_default();
    let f: Vec<&str> = s.rsplit(')').next().unwrap_or("").split_whitespace().collect();
    let tick = 100.0;
    (f.get(11).and_then(|x| x.parse::<f64>().ok()).unwrap_or(0.0)
        + f.get(12).and_then(|x| x.parse::<f64>().ok()).unwrap_or(0.0))
        / tick
}

fn main() {
    let lib = models::library();
    let prepared =
        lsim_prep::prepare(&lib, &models::ladder_drive(20), &Default::default()).expect("prepares");
    let jit = lsim_codegen::compile(&prepared, &Default::default()).expect("compiles");
    let info = RunInfo::from_prepared(&prepared);
    let grid = OutputGrid { t0: 0.0, t_end: 1800.0, dt: 1.0 };
    let opts = SolverOptions::default();
    let j = prepared.flat.find_param("rotor.J").unwrap().0 as usize;
    let sets: Vec<Vec<f64>> = (0..16)
        .map(|k| {
            let mut p = info.params.clone();
            p[j] = 2.0 + 0.5 * k as f64;
            p
        })
        .collect();
    let rounds: usize = std::env::var("ROUNDS").ok().and_then(|s| s.parse().ok()).unwrap_or(5);
    println!("{} sets of {} states; load before: {}", sets.len(), prepared.states.len(), load());
    let mut base = 0.0;
    for threads in [1, 2, 4] {
        let mut best = f64::MAX;
        let mut cpu_per_run = 0.0;
        for _ in 0..rounds {
            let c0 = cpu();
            let t0 = Instant::now();
            let r = sweep(&jit, &info, &sets, &opts, grid, threads);
            let wall = t0.elapsed().as_secs_f64();
            assert!(r.iter().all(|x| x.is_ok()));
            if wall < best {
                best = wall;
                cpu_per_run = (cpu() - c0) / sets.len() as f64;
            }
        }
        if threads == 1 {
            base = best;
        }
        println!(
            "{threads} thread(s): best {:.1} ms for {} runs, speed-up {:.2}x, CPU per run {:.2} ms; load now {}",
            best * 1e3,
            sets.len(),
            base / best,
            cpu_per_run * 1e3,
            load()
        );
    }
}
