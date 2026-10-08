//! The hand-written test car in both modes.

use lsim_engine::testcar::{self, car, hard_cycle, inverse_spec, library, speed_trace};
use lsim_engine::{BuildOptions, Engine};
use lsim_fast::{FastOptions, Trace};
use lsim_solve::{OutputGrid, SolverOptions};

#[test]
fn explore_hard_cycle() {
    let (t, v) = hard_cycle();
    let engine = Engine::new(library(speed_trace("TestCar.Trace", &t, &v)));
    let top = car("TestCar.Trace");
    let inv =
        engine.build_inverse(&top, &inverse_spec(), &BuildOptions::default()).unwrap_or_else(|d| {
            panic!("{}", d.iter().map(|x| x.to_string()).collect::<Vec<_>>().join("\n"))
        });
    println!("inverse: {:?}", inv.report);
    for a in &inv.prepared.assignments {
        let _ = a;
    }
    println!(
        "states {:?}",
        inv.prepared
            .states
            .iter()
            .map(|v| inv.prepared.flat.var(*v).name.clone())
            .collect::<Vec<_>>()
    );
    println!("z {:?}", inv.prepared.algebraics);
    for s in &inv.limits {
        println!("site {s:?}");
    }
    let traces = [Trace::new(t.clone(), v.clone()).unwrap()];
    let r = inv.fast(&traces, &FastOptions::default()).unwrap();
    println!("fast: {:?} wall {:.3} ms", r.report, r.wall_seconds * 1e3);
    for f in &r.flags {
        println!(
            "FLAG {:.3}..{:.3} {} worst {:.1} {}",
            f.t_start, f.t_end, f.label, f.worst_excess, f.unit
        );
    }
    let fwd = engine.build(&top, &BuildOptions::default()).unwrap_or_else(|d| panic!("{d:#?}"));
    println!("forward: {:?}", fwd.report);
    let run = fwd
        .simulate(
            &SolverOptions { rtol: 1e-6, atol: 1e-6, ..Default::default() },
            OutputGrid { t0: 0.0, t_end: 50.0, dt: 0.05 },
        )
        .unwrap();
    println!("forward run: {:?} wall {:.3} ms {}", run.stats, run.wall_seconds * 1e3, run.backend);
    for f in fwd.limit_hits(&run, 1e-9) {
        println!("HIT  {:.3}..{:.3} {} worst {:.1}", f.t_start, f.t_end, f.label, f.worst_excess);
    }
    let _ = testcar::wltc();
}

#[test]
fn explore_wltc_speed() {
    let (t, v) = testcar::wltc();
    let engine = Engine::new(library(speed_trace("TestCar.Trace", &t, &v)));
    let top = car("TestCar.Trace");
    let inv = engine.build_inverse(&top, &inverse_spec(), &BuildOptions::default()).unwrap();
    let traces = [Trace::new(t.clone(), v.clone()).unwrap()];
    for stats in [true, false] {
        let o = FastOptions { interval_stats: stats, ..Default::default() };
        let mut best = f64::INFINITY;
        let mut rep = None;
        for _ in 0..20 {
            let r = inv.fast(&traces, &o).unwrap();
            best = best.min(r.wall_seconds);
            rep = Some((r.report.clone(), r.flags.len()));
        }
        let (rep, nflags) = rep.unwrap();
        println!(
            "WLTC fast stats={stats}: best {:.3} ms = {:.0}x real time; {:?}; flags {nflags}; per step: {:.1} residuals, {:.1} newton, {:.2} vars",
            best * 1e3,
            1800.0 / best,
            rep,
            rep.residual_evals as f64 / rep.steps as f64,
            rep.newton_iterations as f64 / rep.steps as f64,
            rep.vars_evals as f64 / rep.steps as f64
        );
    }
    // the cost of one residual call
    use lsim_ir::runtime::{EvalInput, ModelFunctions};
    let l = *inv.jit.layout();
    let mut y = vec![0.0; l.n_y()];
    let mut d = vec![0.0; l.n_d];
    inv.jit.start(&inv.info.params, &mut y, &mut d);
    let u = vec![10.0, 0.5];
    let mut work = vec![0.0; l.n_work];
    let mut out = vec![0.0; l.n_y()];
    let mut vars = vec![0.0; l.n_vars];
    let n = 200_000;
    let t0 = std::time::Instant::now();
    for k in 0..n {
        let inp = EvalInput { t: k as f64 * 1e-6, y: &y, p: &inv.info.params, d: &d, u: &u };
        inv.jit.residual(&inp, &mut work, &mut out);
    }
    let res_ns = t0.elapsed().as_secs_f64() / n as f64 * 1e9;
    let t0 = std::time::Instant::now();
    for k in 0..n {
        let inp = EvalInput { t: k as f64 * 1e-6, y: &y, p: &inv.info.params, d: &d, u: &u };
        inv.jit.vars(&inp, &mut work, &mut vars);
    }
    let vars_ns = t0.elapsed().as_secs_f64() / n as f64 * 1e9;
    println!("residual {res_ns:.1} ns, vars {vars_ns:.1} ns ({} vars)", l.n_vars);
    let fwd = engine.build(&top, &BuildOptions::default()).unwrap();
    let mut best = f64::INFINITY;
    for _ in 0..3 {
        let run = fwd
            .simulate(
                &SolverOptions { rtol: 1e-6, atol: 1e-6, ..Default::default() },
                OutputGrid { t0: 0.0, t_end: 1800.0, dt: 1.0 },
            )
            .unwrap();
        best = best.min(run.wall_seconds);
        println!(
            "WLTC full: {:.1} ms = {:.0}x real time {:?}",
            run.wall_seconds * 1e3,
            1800.0 / run.wall_seconds,
            run.stats
        );
    }
    let _ = best;
}

#[test]
fn explore_eval_costs() {
    use lsim_ir::runtime::{EvalInput, ModelFunctions};
    let (t, v) = testcar::wltc();
    let engine = Engine::new(library(speed_trace("TestCar.Trace", &t, &v)));
    let inv = engine
        .build_inverse(&car("TestCar.Trace"), &inverse_spec(), &BuildOptions::default())
        .unwrap();
    let l = *inv.jit.layout();
    // a realistic point: z from the guess order printed earlier
    let y = vec![0.8, 3.0, 100.0, 0.0, 0.0, -1280.0, 102.0, 0.8, -50.0, 49.0, 375.0];
    assert_eq!(y.len(), l.n_y());
    let d = vec![0.0; l.n_d];
    let u = vec![20.0, 0.5];
    let mut work = vec![0.0; l.n_work];
    let mut out = vec![0.0; l.n_y()];
    let mut vars = vec![0.0; l.n_vars];
    let mut vv = vec![0.0; l.n_y()];
    vv[5] = 1.0;
    let n = 100_000;
    let time = |f: &mut dyn FnMut(usize)| {
        let t0 = std::time::Instant::now();
        for k in 0..n {
            f(k);
        }
        t0.elapsed().as_secs_f64() / n as f64 * 1e9
    };
    let p = inv.info.params.clone();
    let r = time(&mut |k| {
        let mut yy = y.clone();
        yy[0] += k as f64 * 1e-9;
        inv.jit.residual(&EvalInput { t: 1.0, y: &yy, p: &p, d: &d, u: &u }, &mut work, &mut out)
    });
    let base = time(&mut |k| {
        let mut yy = y.clone();
        yy[0] += k as f64 * 1e-9;
        std::hint::black_box(&yy);
    });
    let mut work2 = vec![0.0; l.n_work];
    let va = time(&mut |k| {
        let mut yy = y.clone();
        yy[0] += k as f64 * 1e-9;
        inv.jit.vars(&EvalInput { t: 1.0, y: &yy, p: &p, d: &d, u: &u }, &mut work2, &mut vars)
    });
    let mut work3 = vec![0.0; l.n_work];
    let j = time(&mut |k| {
        let mut yy = y.clone();
        yy[0] += k as f64 * 1e-9;
        inv.jit.jvp(&EvalInput { t: 1.0, y: &yy, p: &p, d: &d, u: &u }, &vv, &mut work3, &mut out)
    });
    println!(
        "realistic point: residual {:.1} ns, vars {:.1} ns, jvp {:.1} ns (loop overhead {base:.1} ns)",
        r - base,
        va - base,
        j - base
    );
}

#[test]
fn explore_profile_target() {
    let (t, v) = testcar::wltc();
    let engine = Engine::new(library(speed_trace("TestCar.Trace", &t, &v)));
    let inv = engine
        .build_inverse(&car("TestCar.Trace"), &inverse_spec(), &BuildOptions::default())
        .unwrap();
    let traces = [Trace::new(t.clone(), v.clone()).unwrap()];
    for _ in 0..3 {
        let r = inv.fast(&traces, &FastOptions::default()).unwrap();
        println!("fast {:.2} ms", r.wall_seconds * 1e3);
    }
}

#[test]
fn explore_refresh_age() {
    let (t, v) = testcar::wltc();
    let engine = Engine::new(library(speed_trace("TestCar.Trace", &t, &v)));
    let inv = engine
        .build_inverse(&car("TestCar.Trace"), &inverse_spec(), &BuildOptions::default())
        .unwrap();
    let traces = [Trace::new(t.clone(), v.clone()).unwrap()];
    for age in [0usize, 1, 2, 5, 10, 20, 50] {
        for tol in [1e-8, 1e-10] {
            let o = FastOptions { max_jacobian_age: age, newton_tol: tol, ..Default::default() };
            let r = inv.fast(&traces, &o).unwrap();
            let rep = &r.report;
            let evals = rep.residual_evals as f64
                + rep.jacobian_evals as f64 * 11.0 * 1.5
                + rep.gz_refreshes as f64 * 9.0 * 1.5;
            println!(
                "age {age:3} tol {tol:.0e}: per step {:.2} residuals {:.2} newton {:.3} jac {:.3} gz; eval-equivalents {:.1}/step; soc end {:.9}",
                rep.residual_evals as f64 / 1800.0,
                rep.newton_iterations as f64 / 1800.0,
                rep.jacobian_evals as f64 / 1800.0,
                rep.gz_refreshes as f64 / 1800.0,
                evals / 1800.0,
                r.channel("battery.soc").unwrap().last().unwrap()
            );
        }
    }
}
