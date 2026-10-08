//! Times the Stage 1 spike: `cargo run --release -p lsim-engine --example spike`.
//!
//! Prints how long preparation and compilation take, what one call of the
//! generated code costs, and — at three tolerances, best of repeated runs —
//! the solve time, the work, the real-time factor and the error against
//! the exact answer, for the ODE path (CVODE) and the DAE path (IDA).

use lsim_engine::spike::{CHANNELS, Exact, Params, battery_drive};
use lsim_engine::{BuildOptions, Engine};
use lsim_ir::runtime::{EvalInput, ModelFunctions};
use lsim_solve::{OutputGrid, SolverOptions};
use std::time::Instant;

fn main() {
    let p = Params::default();
    let exact = Exact::new(&p);
    let grid = OutputGrid { t0: 0.0, t_end: 4.0, dt: 0.01 };
    let engine = Engine::standard();
    for force_implicit in [false, true] {
        // build several times: the first includes one-off set-up
        let mut best_prep = f64::MAX;
        let mut best_jit = f64::MAX;
        let mut model = None;
        for _ in 0..20 {
            let m = engine
                .build(&battery_drive(&p), &BuildOptions { force_implicit, cache: None })
                .expect("builds");
            best_prep = best_prep.min(m.report.prepare_seconds);
            best_jit = best_jit.min(m.report.compile_seconds);
            model = Some(m);
        }
        let model = model.unwrap();
        let r = &model.report;
        println!(
            "\n{} path: flat {} vars / {} equations; {} aliases removed; {} states, {} iteration variables, {} explicit assignments",
            if force_implicit { "DAE (all blocks implicit)" } else { "ODE" },
            r.flat.0,
            r.flat.1,
            r.sizes.3,
            r.sizes.0,
            r.sizes.1,
            r.sizes.2
        );
        println!(
            "  prepare {:.3} ms, Cranelift compile {:.3} ms ({} bytes of machine code), structure key {}…",
            best_prep * 1e3,
            best_jit * 1e3,
            r.code_bytes,
            &r.structure_key[..12]
        );

        // one call of the generated code
        let l = *model.jit.layout();
        let mut y = vec![0.0; l.n_y()];
        let mut d = vec![0.0; l.n_d];
        model.jit.start(&model.info.params, &mut y, &mut d);
        let u = vec![0.0; l.n_u];
        let mut work = vec![0.0; l.n_work];
        let mut out = vec![0.0; l.n_y()];
        let v = vec![1.0; l.n_y()];
        let inp = EvalInput { t: 0.0, y: &y, p: &model.info.params, d: &d, u: &u };
        let n = 2_000_000;
        let t0 = Instant::now();
        for _ in 0..n {
            model.jit.residual(std::hint::black_box(&inp), &mut work, &mut out);
        }
        let res_ns = t0.elapsed().as_secs_f64() / n as f64 * 1e9;
        let t0 = Instant::now();
        for _ in 0..n {
            model.jit.jvp(std::hint::black_box(&inp), &v, &mut work, &mut out);
        }
        let jvp_ns = t0.elapsed().as_secs_f64() / n as f64 * 1e9;
        println!(
            "  generated code: residual {res_ns:.1} ns per call, Jacobian-vector product {jvp_ns:.1} ns per call"
        );

        println!(
            "  | rtol | solve (best of 20) | x real time | steps | f evals | Jac evals | worst error (rel.) | event time error |"
        );
        println!("  |---|---|---|---|---|---|---|---|");
        for rtol in [1e-6, 1e-8, 1e-10] {
            let opts = SolverOptions { rtol, atol: rtol, ..Default::default() };
            let mut best = f64::MAX;
            let mut last = None;
            for _ in 0..20 {
                let run = model.simulate(&opts, grid).expect("runs");
                best = best.min(run.wall_seconds);
                last = Some(run);
            }
            let run = last.unwrap();
            let mut worst = 0.0f64;
            for (j, name) in CHANNELS.iter().enumerate() {
                let ch = run.channel(name).unwrap();
                let scale = run.times.iter().map(|t| exact.at(*t)[j].abs()).fold(0.0, f64::max);
                for (k, t) in run.times.iter().enumerate() {
                    worst = worst.max((ch[k] - exact.at(*t)[j]).abs() / scale);
                }
            }
            println!(
                "  | {rtol:.0e} | {:.3} ms | {:.0} | {} | {} | {} | {worst:.1e} | {:.1e} s |",
                best * 1e3,
                grid.t_end / best,
                run.stats.steps,
                run.stats.rhs_evals,
                run.stats.jac_evals,
                (run.events[0].t - exact.t_event).abs()
            );
        }
    }
}
