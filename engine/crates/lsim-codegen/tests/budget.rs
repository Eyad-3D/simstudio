//! The code generator's time budget (DESIGN.md §16, WP3's acceptance):
//! the example projects compile in under 50 ms, and a model of 10 000
//! equations is ready to run in under 100 ms (a model that large is
//! tiered: it runs on its tapes, which compute bitwise what its machine
//! code computes, while the machine code compiles on other threads).
//!
//! Times are the best of several compilations, so that a busy machine
//! (tests run in parallel) does not fail them; the budgets hold with a
//! margin of two or more on the development machine (4 cores).

#[path = "common/cars.rs"]
mod cars;
#[path = "common/synth.rs"]
mod synth;

use lsim_codegen::{CodegenOptions, JitModel, compile};
use lsim_ir::PreparedModel;
use std::time::Instant;

/// The best of `n` compilations, s, and the last model.
fn best_of(n: usize, m: &PreparedModel, opts: &CodegenOptions) -> (f64, JitModel) {
    let mut best = f64::INFINITY;
    let mut last = None;
    for _ in 0..n {
        let t = Instant::now();
        let j = compile(m, opts).expect("compiles");
        best = best.min(t.elapsed().as_secs_f64());
        // (a tiered model's machine code first, so that the next
        // compilation has the machine to itself)
        if let Some(r) = j.wait_machine_code() {
            r.expect("machine code");
        }
        last = Some(j);
    }
    (best, last.expect("compiled"))
}

#[test]
fn the_example_projects_compile_in_under_50_ms() {
    for car in cars::one_per_project(cars::cars("")) {
        let (t, j) = best_of(5, &car.model, &CodegenOptions::default());
        println!("{}: {:.1} ms ({} nodes)", car.name, t * 1e3, j.report.nodes);
        assert!(!j.report.tiered, "{}: tiered", car.name);
        assert!(t < 0.050, "{}: {:.1} ms", car.name, t * 1e3);
    }
}

#[test]
fn ten_thousand_equations_are_ready_in_under_100_ms() {
    // 1 430 nodes: their derivatives and the assignments of the flows
    // between them, their sources and channels, 10 000 equations or so
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
    assert!(t < 0.100, "ready in {:.1} ms", t * 1e3);
}
