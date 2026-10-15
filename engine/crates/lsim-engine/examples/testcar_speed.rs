//! Speed of the test car on the WLTC in both modes (release build):
//!
//! ```sh
//! cargo run --release -p lsim-engine --example testcar_speed
//! ```
//!
//! Prints the best of several runs of fast mode (with and without the
//! per-interval statistics) and of the full dynamic run, as multiples of
//! real time, with what each run did.

use lsim_engine::testcar::{self, car, inverse_spec, library, speed_trace};
use lsim_engine::{BuildOptions, Engine};
use lsim_fast::{FastOptions, Trace};
use lsim_solve::{OutputGrid, SolverOptions};

fn main() {
    let (t, v) = testcar::wltc();
    let engine = Engine::new(library(speed_trace("TestCar.Trace", &t, &v)));
    let top = car("TestCar.Trace");
    let inv = engine.build_inverse(&top, &inverse_spec(), &BuildOptions::default()).unwrap();
    // let the background machine code finish before timing
    let _ = inv.jit.wait_machine_code();
    let traces = [Trace::new(t.clone(), v.clone()).unwrap()];
    let duration = t[t.len() - 1] - t[0];
    for stats in [true, false] {
        let o = FastOptions { interval_stats: stats, ..Default::default() };
        let mut best = f64::INFINITY;
        let mut last = None;
        for _ in 0..50 {
            let r = inv.fast(&traces, &o).unwrap();
            best = best.min(r.wall_seconds);
            last = Some(r);
        }
        let r = last.unwrap();
        let rep = &r.report;
        println!(
            "fast (interval statistics {stats}): best {:.3} ms = {:.3e} x real time; per step \
             {:.2} residuals, {:.2} Newton iterations, {:.2} channel evaluations; flags {}; \
             largest scaled error {:.3e} at {} s",
            best * 1e3,
            duration / best,
            rep.residual_evals as f64 / rep.steps as f64,
            rep.newton_iterations as f64 / rep.steps as f64,
            rep.vars_evals as f64 / rep.steps as f64,
            r.flags.len(),
            rep.max_error,
            rep.max_error_at,
        );
    }
    let fwd = engine.build(&top, &BuildOptions::default()).unwrap();
    let _ = fwd.jit.wait_machine_code();
    let mut best = f64::INFINITY;
    for _ in 0..5 {
        let run = fwd
            .simulate(
                &SolverOptions { rtol: 1e-6, atol: 1e-6, ..Default::default() },
                OutputGrid { t0: 0.0, t_end: duration, dt: 1.0 },
            )
            .unwrap();
        best = best.min(run.wall_seconds);
        if best == run.wall_seconds {
            println!("full dynamic: {:?}", run.stats);
        }
    }
    println!("full dynamic: best {:.1} ms = {:.0} x real time", best * 1e3, duration / best);
}
