//! The Stage 1 end-to-end spike: a model described in the IR is flattened,
//! alias-eliminated, sorted, compiled to machine code with Cranelift,
//! solved by SUNDIALS with exact event location, and compared with its
//! closed-form answer.

use lsim_engine::spike::{CHANNELS, Exact, Params, battery_drive};
use lsim_engine::{BuildOptions, Engine, Model};
use lsim_solve::{OutputGrid, SimResult, SolverOptions};

const GRID: OutputGrid = OutputGrid { t0: 0.0, t_end: 4.0, dt: 0.01 };

/// Largest error of each compared channel over the grid, relative to the
/// channel's largest exact magnitude.
fn errors(run: &SimResult, exact: &Exact) -> [f64; 3] {
    let mut err = [0.0f64; 3];
    let mut scale = [0.0f64; 3];
    for (j, name) in CHANNELS.iter().enumerate() {
        let ch = run.channel(name).unwrap_or_else(|| panic!("no channel {name}"));
        for (k, t) in run.times.iter().enumerate() {
            let ex = exact.at(*t)[j];
            scale[j] = scale[j].max(ex.abs());
            err[j] = err[j].max((ch[k] - ex).abs());
        }
    }
    [err[0] / scale[0], err[1] / scale[1], err[2] / scale[2]]
}

fn build(force_implicit: bool) -> Model {
    Engine::standard()
        .build(&battery_drive(&Params::default()), &BuildOptions { force_implicit, cache: None })
        .unwrap_or_else(|d| panic!("the spike builds: {d:#?}"))
}

fn tight() -> SolverOptions {
    SolverOptions { rtol: 1e-10, atol: 1e-10, ..Default::default() }
}

#[test]
fn ode_path_matches_the_exact_answer() {
    let model = build(false);
    let r = &model.report;
    // two states (capacitor voltage, speed), everything else explicit
    assert_eq!((r.sizes.0, r.sizes.1), (2, 0), "{r:#?}");
    let run = model.simulate(&tight(), GRID).expect("runs");
    let exact = Exact::new(&Params::default());
    assert_eq!(run.backend, "SUNDIALS CVODE (BDF)");
    assert_eq!(run.events.len(), 1, "{:?}", run.events);
    let dt_event = (run.events[0].t - exact.t_event).abs();
    let e = errors(&run, &exact);
    println!(
        "ODE path: flat {}/{} -> {} aliases, {} explicit; prepare {:.2} ms, compile {:.2} ms ({} B); \
         solve {:.3} ms ({} steps, {} f, {} J); errors v1 {:.1e} w {:.1e} i {:.1e}; event {:.1e} s; {:.0}x real time",
        r.flat.0,
        r.flat.1,
        r.sizes.3,
        r.sizes.2,
        r.prepare_seconds * 1e3,
        r.compile_seconds * 1e3,
        r.code_bytes,
        run.wall_seconds * 1e3,
        run.stats.steps,
        run.stats.rhs_evals,
        run.stats.jac_evals,
        e[0],
        e[1],
        e[2],
        dt_event,
        GRID.t_end / run.wall_seconds
    );
    assert!(dt_event < 1e-8, "event time off by {dt_event:e} s");
    for (j, x) in e.iter().enumerate() {
        assert!(*x < 1e-8, "{} off by {x:e} (relative)", CHANNELS[j]);
    }
    assert!(run.events[0].label.contains("'Overspeed Brake'"), "{}", run.events[0].label);
}

#[test]
fn dae_path_matches_the_exact_answer() {
    // every block kept implicit: the iteration variables go to IDA
    let model = build(true);
    let r = &model.report;
    assert!(r.sizes.1 > 5, "expected iteration variables: {r:#?}");
    let run = model.simulate(&tight(), GRID).expect("runs");
    let exact = Exact::new(&Params::default());
    assert_eq!(run.backend, "SUNDIALS IDA (BDF, DAE)");
    assert_eq!(run.events.len(), 1);
    let dt_event = (run.events[0].t - exact.t_event).abs();
    let e = errors(&run, &exact);
    println!(
        "DAE path: {} states + {} iteration variables; solve {:.3} ms ({} steps); errors v1 {:.1e} w {:.1e} i {:.1e}; event {:.1e} s",
        r.sizes.0,
        r.sizes.1,
        run.wall_seconds * 1e3,
        run.stats.steps,
        e[0],
        e[1],
        e[2],
        dt_event
    );
    assert!(dt_event < 1e-8, "event time off by {dt_event:e} s");
    for (j, x) in e.iter().enumerate() {
        assert!(*x < 1e-8, "{} off by {x:e} (relative)", CHANNELS[j]);
    }
}

#[test]
fn tighter_tolerance_shrinks_the_error() {
    // the one-click accuracy check: 10x tighter twice must gain accuracy
    let model = build(false);
    let exact = Exact::new(&Params::default());
    let mut opts = SolverOptions { rtol: 1e-5, atol: 1e-5, ..Default::default() };
    let mut last = f64::INFINITY;
    for _ in 0..3 {
        let e = errors(&model.simulate(&opts, GRID).unwrap(), &exact);
        let worst = e.iter().cloned().fold(0.0, f64::max);
        println!("rtol {:.0e}: worst relative error {worst:.1e}", opts.rtol);
        assert!(worst < 50.0 * opts.rtol, "error {worst:e} at rtol {:e}", opts.rtol);
        assert!(worst < last / 3.0, "a tighter run must be more accurate");
        last = worst;
        opts = opts.tighter();
    }
}

#[test]
fn parameters_change_without_recompiling() {
    let mut model = build(false);
    let key = model.report.structure_key.clone();
    model.set_param("battery.ocv", 450.0).expect("a parameter of the battery");
    let p = Params { ocv: 450.0, ..Params::default() };
    let run = model.simulate(&tight(), GRID).unwrap();
    let e = errors(&run, &Exact::new(&p));
    assert!(e.iter().all(|x| *x < 1e-8), "{e:?}");
    assert_eq!(model.report.structure_key, key);
    // the bound sub-component parameter followed its parent
    let id = model.prepared.flat.find_param("battery.source.V").unwrap();
    assert_eq!(model.info.params[id.0 as usize], 450.0);
}

#[test]
fn min_max_mean_cover_the_steps_between_outputs() {
    let model = build(false);
    let coarse = OutputGrid { t0: 0.0, t_end: 4.0, dt: 1.0 };
    let run = model.simulate(&tight(), coarse).unwrap();
    let i = run.names.iter().position(|n| n == "motor.i").unwrap();
    // the current starts at 8000 A and falls fast: the first interval's max
    // is the start, its mean is far below it, its min is the end-of-interval
    // value or lower
    assert!((run.max[i][1] - 8000.0).abs() < 1e-6, "{}", run.max[i][1]);
    assert!(run.mean[i][1] < run.max[i][1] && run.mean[i][1] > run.min[i][1]);
    assert!(run.min[i][1] <= run.values[i][1] + 1e-9);
    // the mean of w over [3, 4] s matches the exact average (Simpson's rule
    // on the closed form)
    let w = run.names.iter().position(|n| n == "inertia.w").unwrap();
    let ex = Exact::new(&Params::default());
    let n = 2000;
    let h = 1.0 / n as f64;
    let mut s = 0.0;
    for k in 0..=n {
        let wgt = if k == 0 || k == n {
            1.0
        } else if k % 2 == 1 {
            4.0
        } else {
            2.0
        };
        s += wgt * ex.at(3.0 + k as f64 * h)[1];
    }
    let exact_mean = s * h / 3.0;
    assert!(
        (run.mean[w][4] - exact_mean).abs() < 1e-3 * exact_mean,
        "{} vs {exact_mean}",
        run.mean[w][4]
    );
}

#[test]
fn the_cache_skips_preparation() {
    let dir = std::env::temp_dir().join(format!("lsim-cache-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let engine = Engine::standard();
    let top = battery_drive(&Params::default());
    let opts = BuildOptions { force_implicit: false, cache: Some(dir.clone()) };
    let first = engine.build(&top, &opts).unwrap();
    let second = engine.build(&top, &opts).unwrap();
    assert!(!first.report.cache_hit && second.report.cache_hit);
    assert_eq!(first.report.structure_key, second.report.structure_key);
    let a = first.simulate(&tight(), GRID).unwrap();
    let b = second.simulate(&tight(), GRID).unwrap();
    assert_eq!(a.values, b.values);
    let _ = std::fs::remove_dir_all(&dir);
}
