//! Parallel sweeps (DESIGN.md §8.4): the 20-stage ladder drive over
//! 1800 s for 16 parameter sets, on 1, 2 and 4 threads; best of several
//! rounds, with the machine's load before and after and each run's CPU
//! time (which shows whether runs slow each other down, independently of
//! how many cores the shared machine had free).
//!
//! For the best round of each thread count it also prints the cores the
//! sweep had on average (its CPU time over its wall time), how many cores
//! other processes kept busy meanwhile (from /proc/stat), and the speed-up
//! the runs' own CPU times allow on that many free cores (threads × CPU
//! per run on one thread / CPU per run on these threads).
//!
//! `cargo run --release -p lsim-solve --example sweep_scaling`; `STAGES`
//! (20), `SETS` (16) and `ROUNDS` (5) change the ladder, the sets and the
//! rounds.

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

/// The whole process's (user, system) CPU time, s, and minor page faults.
fn usage() -> (f64, f64, f64) {
    let s = std::fs::read_to_string("/proc/self/stat").unwrap_or_default();
    let f: Vec<&str> = s.rsplit(')').next().unwrap_or("").split_whitespace().collect();
    let at = |k: usize| f.get(k).and_then(|x| x.parse::<f64>().ok()).unwrap_or(0.0);
    let tick = 100.0;
    (at(11) / tick, at(12) / tick, at(7))
}

/// CPU time of the whole process, s.
fn cpu() -> f64 {
    let (u, s, _) = usage();
    u + s
}

/// The machine's busy and total CPU time so far, in clock ticks (all
/// cores: the first line of /proc/stat).
fn machine() -> (f64, f64) {
    let s = std::fs::read_to_string("/proc/stat").unwrap_or_default();
    let f: Vec<f64> = s
        .lines()
        .next()
        .unwrap_or("")
        .split_whitespace()
        .skip(1)
        .filter_map(|x| x.parse().ok())
        .collect();
    let total: f64 = f.iter().take(8).sum();
    let idle = f.get(3).copied().unwrap_or(0.0) + f.get(4).copied().unwrap_or(0.0);
    (total - idle, total)
}

fn main() {
    let lib = models::library();
    let prepared = lsim_prep::prepare(
        &lib,
        &models::ladder_drive(
            std::env::var("STAGES").ok().and_then(|s| s.parse().ok()).unwrap_or(20),
        ),
        &Default::default(),
    )
    .expect("prepares");
    let jit = lsim_codegen::compile(&prepared, &Default::default()).expect("compiles");
    let info = RunInfo::from_prepared(&prepared);
    let grid = OutputGrid { t0: 0.0, t_end: 1800.0, dt: 1.0 };
    let opts = SolverOptions::default();
    let j = prepared.flat.find_param("rotor.J").unwrap().0 as usize;
    let n_sets: usize = std::env::var("SETS").ok().and_then(|s| s.parse().ok()).unwrap_or(16);
    let sets: Vec<Vec<f64>> = (0..n_sets)
        .map(|k| {
            let mut p = info.params.clone();
            p[j] = 2.0 + 0.5 * (k % 16) as f64;
            p
        })
        .collect();
    let rounds: usize = std::env::var("ROUNDS").ok().and_then(|s| s.parse().ok()).unwrap_or(5);
    println!("{} sets of {} states; load before: {}", sets.len(), prepared.states.len(), load());
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) as f64;
    let mut base = 0.0;
    let mut base_cpu = 0.0;
    for threads in [1, 2, 4] {
        let mut best = f64::MAX;
        let mut cpu_per_run = 0.0;
        let mut others = 0.0;
        let mut sys_faults = (0.0, 0.0);
        for _ in 0..rounds {
            let (_, s0, f0) = usage();
            let (b0, t0m) = machine();
            let c0 = cpu();
            let t0 = Instant::now();
            let r = sweep(&jit, &info, &sets, &opts, grid, threads);
            let wall = t0.elapsed().as_secs_f64();
            let c = cpu() - c0;
            let (b1, t1m) = machine();
            let (_, s1, f1) = usage();
            assert!(r.iter().all(|x| x.is_ok()));
            if wall < best {
                best = wall;
                cpu_per_run = c / sets.len() as f64;
                // busy cores, all processes, minus this one's
                let busy = if t1m > t0m { cores * (b1 - b0) / (t1m - t0m) } else { 0.0 };
                others = (busy - c / wall).max(0.0);
                sys_faults = ((s1 - s0) / sets.len() as f64, (f1 - f0) / sets.len() as f64);
            }
        }
        if threads == 1 {
            base = best;
            base_cpu = cpu_per_run;
        }
        println!(
            "{threads} thread(s): best {:.1} ms for {} runs, speed-up {:.2}x, CPU per run {:.2} ms \
             ({:.2} ms system, {:.0} page faults), {:.2} cores used, other processes {:.2} cores; free-core speed-up {:.2}x; load now {}",
            best * 1e3,
            sets.len(),
            base / best,
            cpu_per_run * 1e3,
            sys_faults.0 * 1e3,
            sys_faults.1,
            cpu_per_run * sets.len() as f64 / best,
            others,
            threads as f64 * base_cpu / cpu_per_run,
            load()
        );
    }
}
