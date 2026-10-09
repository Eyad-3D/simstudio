//! Compile times and per-call costs of the generated code on synthetic
//! models of the example cars' size and on large networks:
//!
//! ```sh
//! cargo run --release -p lsim-codegen --example codegen_bench [filter]
//! ```

#[path = "../tests/common/synth.rs"]
mod synth;

use lsim_codegen::{CodegenOptions, JitModel, compile};
use lsim_ir::PreparedModel;
use lsim_ir::runtime::{EvalInput, ModelFunctions};
use std::hint::black_box;
use std::time::Instant;

/// This thread's CPU time, s (Linux; NaN elsewhere): compile times
/// measured on a loaded machine without the time spent waiting for a CPU.
fn cpu_time() -> f64 {
    std::fs::read_to_string("/proc/thread-self/schedstat")
        .ok()
        .and_then(|s| s.split_whitespace().next().and_then(|v| v.parse::<f64>().ok()))
        .map_or(f64::NAN, |ns| ns * 1e-9)
}

fn best_compile(m: &PreparedModel, o: &CodegenOptions, reps: usize) -> (f64, f64, JitModel) {
    let (mut best, mut best_cpu) = (f64::INFINITY, f64::INFINITY);
    let mut keep = None;
    for _ in 0..reps {
        let (t, c) = (Instant::now(), cpu_time());
        let j = compile(m, o).expect("compiles");
        best = best.min(t.elapsed().as_secs_f64());
        best_cpu = best_cpu.min(cpu_time() - c);
        keep = Some(j);
    }
    (best, best_cpu, keep.unwrap())
}

fn per_call(mut f: impl FnMut()) -> f64 {
    // calibrate to ~20 ms, best of 5
    let mut n = 1usize;
    loop {
        let t = Instant::now();
        for _ in 0..n {
            f();
        }
        if t.elapsed().as_secs_f64() > 0.02 {
            break;
        }
        n *= 2;
    }
    let mut best = f64::INFINITY;
    for _ in 0..5 {
        let t = Instant::now();
        for _ in 0..n {
            f();
        }
        best = best.min(t.elapsed().as_secs_f64() / n as f64);
    }
    best
}

fn calls(name: &str, m: &PreparedModel, j: &JitModel) {
    let l = *j.layout();
    let p = synth::params(m);
    let (d, u) = (vec![0.0; l.n_d], vec![0.0; l.n_u]);
    let mut y0 = vec![0.0; l.n_y()];
    let mut d0 = vec![0.0; l.n_d];
    j.start(&p, &mut y0, &mut d0);
    let _ = d;
    let mut work = vec![0.0; l.n_work];
    let inp = EvalInput { t: 12.3, y: &y0, p: &p, d: &d0, u: &u };
    let mut out = vec![0.0; l.n_y()];
    let v: Vec<f64> = (0..l.n_y()).map(|i| 1.0 / (1.0 + i as f64)).collect();
    let mut vals = vec![0.0; j.pattern().nnz()];
    let mut dense = vec![0.0; l.n_y() * l.n_y()];
    let mut vars = vec![0.0; l.n_vars];
    let r = per_call(|| j.residual(black_box(&inp), &mut work, &mut out));
    let jv = per_call(|| j.jvp(black_box(&inp), &v, &mut work, &mut out));
    let js = per_call(|| j.jacobian_sparse(black_box(&inp), &mut work, &mut vals));
    let jd = if l.n_y() <= 400 {
        per_call(|| j.jacobian_dense(black_box(&inp), &mut work, &mut dense))
    } else {
        f64::NAN
    };
    let va = per_call(|| j.vars(black_box(&inp), &mut work, &mut vars));
    println!(
        "{name}: residual {:.1} ns, jvp {:.1} ns, sparse Jacobian {:.1} ns ({} nnz, {} colours; = {:.1} residuals), dense Jacobian {:.1} ns, vars {:.1} ns",
        r * 1e9,
        jv * 1e9,
        js * 1e9,
        j.report.jac_nnz,
        j.report.jac_colours,
        js / r,
        jd * 1e9,
        va * 1e9
    );
}

fn main() {
    let filter = std::env::args().nth(1).unwrap_or_default();
    let models: Vec<(&str, PreparedModel)> = vec![
        ("vehicle(1)", synth::vehicle(1)),
        ("vehicle(2)", synth::vehicle(2)),
        ("vehicle(4)", synth::vehicle(4)),
        ("network(400)", synth::network(400, 7)),
        ("network(2000)", synth::network(2000, 7)),
        ("network(4000)", synth::network(4000, 7)),
    ];
    let variants: Vec<(&str, CodegenOptions)> = vec![
        ("default", CodegenOptions::default()),
        ("auto, 1 thread", CodegenOptions { threads: 1, ..Default::default() }),
        (
            "speed+backtracking",
            CodegenOptions { opt_level: "speed", regalloc: "backtracking", ..Default::default() },
        ),
        (
            "none+backtracking",
            CodegenOptions { opt_level: "none", regalloc: "backtracking", ..Default::default() },
        ),
        (
            "none+single_pass",
            CodegenOptions { opt_level: "none", regalloc: "single_pass", ..Default::default() },
        ),
        (
            "speed, 1 thread",
            CodegenOptions {
                opt_level: "speed",
                regalloc: "backtracking",
                threads: 1,
                ..Default::default()
            },
        ),
        (
            "speed, chunk 2000, 1 thread",
            CodegenOptions {
                opt_level: "speed",
                regalloc: "backtracking",
                threads: 1,
                chunk_nodes: 2000,
                ..Default::default()
            },
        ),
        (
            "speed, chunk 500, 1 thread",
            CodegenOptions {
                opt_level: "speed",
                regalloc: "backtracking",
                threads: 1,
                chunk_nodes: 500,
                ..Default::default()
            },
        ),
        (
            "none, chunk 500, 1 thread",
            CodegenOptions {
                opt_level: "none",
                regalloc: "backtracking",
                threads: 1,
                chunk_nodes: 500,
                ..Default::default()
            },
        ),
    ];
    let only: Option<String> = std::env::var("VARIANT").ok();
    let variants: Vec<(&str, CodegenOptions)> = variants
        .into_iter()
        .filter(|(n, _)| only.as_ref().is_none_or(|o| n.contains(o.as_str())))
        .collect();
    for (name, m) in &models {
        if !name.contains(&filter) {
            continue;
        }
        let n_a = m.assignments.len();
        println!(
            "== {name}: {} assignments, {} states, {} iteration variables, {} tables",
            n_a,
            m.states.len(),
            m.algebraics.len(),
            m.flat.tables.len()
        );
        for (vn, o) in &variants {
            let (t, cpu, j) = best_compile(m, o, 3);
            let r = &j.report;
            println!(
                "  {vn:20} compile {:.2} ms, CPU {:.2} ms (analysis {:.2}, IR {:.2}, codegen {:.2}; {} functions, {} kB, {} threads, opt {}, {})",
                t * 1e3,
                cpu * 1e3,
                r.analysis_seconds * 1e3,
                r.ir_seconds * 1e3,
                r.codegen_seconds * 1e3,
                r.functions,
                r.code_bytes / 1024,
                r.threads,
                r.opt_level,
                r.regalloc
            );
            calls(&format!("    {vn}"), m, &j);
        }
    }
}
