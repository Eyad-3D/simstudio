//! The code generator's time budget (DESIGN.md §16, WP3's acceptance):
//! the example projects compile in under 50 ms, and a model of 10 000
//! equations is ready to run in under 100 ms (a model that large is
//! tiered: it runs on its tapes, which compute bitwise what its machine
//! code computes, while the machine code compiles on other threads).
//!
//! Times are on the calling thread's own clock where the platform gives
//! one (Linux): CPU time, the time a compilation would take on an idle
//! machine, which tests running beside it do not change; elsewhere wall
//! time. Each is the best of several compilations, on one thread (a
//! tiered model's machine code is awaited before the next one): in a
//! release build the budgets hold with a margin of about 1.5 (cars) and
//! two (10 000 equations). With debug assertions on (the test profile of
//! `engine/check.sh`: Cranelift's verifier and its debug checks run, the
//! code is optimised less) compilation takes about 1.6 times as long, and
//! the budgets are doubled; `cargo test --release` checks them as stated.

#[path = "common/cars.rs"]
mod cars;
#[path = "common/synth.rs"]
mod synth;

use lsim_codegen::{CodegenOptions, JitModel, compile};
use lsim_ir::PreparedModel;
use std::time::Instant;

/// This thread's CPU time, s, where the platform tells it.
fn thread_cpu() -> Option<f64> {
    let s = std::fs::read_to_string("/proc/thread-self/schedstat").ok()?;
    let ns: f64 = s.split_whitespace().next()?.parse().ok()?;
    Some(ns * 1e-9)
}

/// The time `f` takes: on this thread's CPU clock, or the wall clock.
fn timed<T>(f: impl FnOnce() -> T) -> (f64, T) {
    let (wall, cpu) = (Instant::now(), thread_cpu());
    let r = f();
    let t = match (cpu, thread_cpu()) {
        (Some(a), Some(b)) => b - a,
        _ => wall.elapsed().as_secs_f64(),
    };
    (t, r)
}

/// The best of `n` compilations on one thread, s, and the last model.
fn best_of(n: usize, m: &PreparedModel, opts: &CodegenOptions) -> (f64, JitModel) {
    let opts = CodegenOptions { threads: 1, ..opts.clone() };
    let mut best = f64::INFINITY;
    let mut last = None;
    for _ in 0..n {
        let (t, j) = timed(|| compile(m, &opts).expect("compiles"));
        best = best.min(t);
        if let Some(r) = j.wait_machine_code() {
            r.expect("machine code");
        }
        last = Some(j);
    }
    (best, last.expect("compiled"))
}

/// The budgets' factor: 1 in a release build (above).
const SLACK: f64 = if cfg!(debug_assertions) { 2.0 } else { 1.0 };

/// One test, so that its two parts do not run beside each other.
#[test]
fn the_compile_time_budgets_hold() {
    // the example projects: under 50 ms
    for car in cars::one_per_project(cars::cars("")) {
        let (t, j) = best_of(5, &car.model, &CodegenOptions::default());
        println!("{}: {:.1} ms ({} nodes)", car.name, t * 1e3, j.report.nodes);
        assert!(!j.report.tiered, "{}: tiered", car.name);
        assert!(t < 0.050 * SLACK, "{}: {:.1} ms", car.name, t * 1e3);
    }
    // 1 430 nodes: their derivatives and the assignments of the flows
    // between them, their sources and channels, 10 000 equations or so:
    // ready in under 100 ms
    let m = synth::network(1430, 7);
    let n = m.assignments.len() + m.residuals.len();
    assert!((10_000..10_020).contains(&n), "{n} equations");
    let (t, j) = best_of(5, &m, &CodegenOptions::default());
    let r = j.wait_machine_code().expect("tiered").expect("machine code");
    println!(
        "10 000 equations ({} nodes): ready in {:.1} ms, machine code in {:.1} ms",
        j.report.nodes,
        t * 1e3,
        r.seconds * 1e3
    );
    assert!(j.report.tiered);
    assert!(t < 0.100 * SLACK, "ready in {:.1} ms", t * 1e3);
}
