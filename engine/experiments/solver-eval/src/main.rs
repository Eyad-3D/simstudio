//! Runs every backend at four tolerances and prints one Markdown table:
//! the largest error against the exact answer (relative to each variable's
//! largest magnitude), the event-time error, the work counters and the wall
//! time per solve (median of repeated solves, after a warm-up).
//!
//! `cargo run --release` (both backends) or `--no-default-features
//! --features diffsol` (pure Rust only).
//!
//! Environment knobs: `RTOLS=1e-8,1e-10` picks the tolerances, `ONLY=IDA`
//! the backends whose name contains the text, `HMIN=1e-20` lowers diffsol's
//! smallest step (see diffsol_run.rs), and `CHAIN=1` runs the scaling check
//! on a torsional chain of 9, 49 and 199 states instead (chain.rs).

#![allow(clippy::needless_range_loop)]

mod chain;
#[cfg(feature = "diffsol")]
mod diffsol_run;
mod problem;
#[cfg(feature = "sundials")]
mod sundials;

use problem::*;
use std::time::Instant;

type Runner = fn(f64, [f64; 3]) -> RunStats;

fn time_it(f: Runner, rtol: f64, atol: [f64; 3]) -> (RunStats, f64) {
    let st = f(rtol, atol);
    // repeat for about 0.3 s, at least 5 times
    let mut times = Vec::new();
    let start = Instant::now();
    while times.len() < 5 || (start.elapsed().as_secs_f64() < 0.3 && times.len() < 2000) {
        let t0 = Instant::now();
        let s = f(rtol, atol);
        times.push(t0.elapsed().as_secs_f64());
        std::hint::black_box(s);
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    (st, times[times.len() / 2])
}

fn main() {
    if std::env::var("CHAIN").is_ok() {
        return chain_main();
    }
    let exact = Exact::new();
    println!(
        "Exact brake event at t = {:.15} s; w_eq before = {:.6} rad/s, after = {:.6} rad/s; poles {:.1} and {:.4} 1/s",
        exact.t_event, exact.before.xeq[1], exact.after.xeq[1], exact.before.l1, exact.before.l2
    );
    #[allow(unused_mut)]
    let mut runners: Vec<(&str, Runner)> = Vec::new();
    #[cfg(feature = "sundials")]
    {
        runners.push(("SUNDIALS CVODE (BDF, ODE)", sundials::cvode));
        runners.push(("SUNDIALS IDA (BDF, DAE)", sundials::ida));
    }
    #[cfg(feature = "diffsol")]
    {
        runners.push((
            "diffsol BDF, ODE, nalgebra LU",
            diffsol_run::bdf_ode_nalgebra,
        ));
        runners.push(("diffsol BDF, ODE, faer LU", diffsol_run::bdf_ode_faer));
        runners.push((
            "diffsol BDF, DAE (mass), nalgebra LU",
            diffsol_run::bdf_dae_nalgebra,
        ));
        runners.push((
            "diffsol BDF, DAE (mass), faer LU",
            diffsol_run::bdf_dae_faer,
        ));
    }
    println!();
    println!("| backend | rtol | max err v1 | max err w | max err i | event t err (s) | steps | f evals | Jac/LU setups | err-test fails | time/solve (us) |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|");
    let rtols: Vec<f64> = std::env::var("RTOLS")
        .ok()
        .map(|v| v.split(',').map(|x| x.parse().unwrap()).collect())
        .unwrap_or(vec![1e-4, 1e-6, 1e-8, 1e-10]);
    for rtol in rtols {
        let atol = [rtol * 10.0, rtol * 100.0, rtol * 1000.0];
        for (name, f) in &runners {
            if let Ok(only) = std::env::var("ONLY") {
                if !name.contains(only.as_str()) {
                    continue;
                }
            }
            let run = std::panic::catch_unwind(|| time_it(*f, rtol, atol));
            let Ok((st, secs)) = run else {
                println!(
                    "| {name} | {rtol:.0e} | FAILED (see the message above) | | | | | | | | |"
                );
                continue;
            };
            assert_eq!(st.samples.len(), sample_times().len(), "{name}");
            let e = max_rel_error(&exact, &st);
            println!(
                "| {name} | {rtol:.0e} | {:.1e} | {:.1e} | {:.1e} | {:.1e} | {} | {} | {} | {} | {:.0} |",
                e[0],
                e[1],
                e[2],
                (st.t_event - exact.t_event).abs(),
                st.steps,
                st.rhs_evals,
                st.jac_evals,
                st.err_test_fails,
                secs * 1e6
            );
        }
    }
}

type ChainRunner = fn(usize, f64) -> (Vec<f64>, u64, u64);

/// The scaling table: best-of-N wall time per solve of the torsional chain.
fn chain_main() {
    #[allow(unused_mut)]
    let mut runners: Vec<(&str, ChainRunner)> = Vec::new();
    #[cfg(feature = "sundials")]
    {
        runners.push(("SUNDIALS CVODE, dense LU", chain::sun::cvode));
        runners.push(("SUNDIALS CVODE, band LU", chain::sun::cvode_band));
    }
    #[cfg(feature = "diffsol")]
    {
        runners.push(("diffsol BDF, nalgebra dense LU", chain::dsol::bdf_nalgebra));
        runners.push(("diffsol BDF, faer sparse LU", chain::dsol::bdf_faer_sparse));
    }
    println!("| backend | states | rtol | max diff vs reference | steps | LU setups | best time/solve (ms) |");
    println!("|---|---|---|---|---|---|---|");
    for n in [5usize, 25, 100] {
        let (yref, _, _) = runners[0].1(n, 1e-12);
        let scale = yref.iter().fold(0.0f64, |a, b| a.max(b.abs()));
        for rtol in [1e-6, 1e-8] {
            for (name, f) in &runners {
                let (y, steps, lu) = f(n, rtol);
                let d = y
                    .iter()
                    .zip(&yref)
                    .fold(0.0f64, |a, (p, q)| a.max((p - q).abs()))
                    / scale;
                let mut best = f64::MAX;
                let start = Instant::now();
                let mut reps = 0;
                while reps < 3 || (start.elapsed().as_secs_f64() < 1.0 && reps < 200) {
                    let t0 = Instant::now();
                    std::hint::black_box(f(n, rtol));
                    best = best.min(t0.elapsed().as_secs_f64());
                    reps += 1;
                }
                println!(
                    "| {name} | {} | {rtol:.0e} | {d:.1e} | {steps} | {lu} | {:.2} |",
                    2 * n - 1,
                    best * 1e3
                );
            }
        }
    }
}
