//! The numbers behind the machine-code caching decision (DESIGN.md,
//! *Code generation and the cache*): for each example project, what a
//! build costs today on a cache hit (the prepared model read back from
//! JSON, then compiled) and what caching the machine code too could save
//! (Cranelift and the IR it reads; a hit would still read the code, place
//! it in executable memory and resolve its calls, measured here as the
//! compiler's own link step and a file read of the same size).
//!
//! ```sh
//! cargo run --release -p lsim-codegen --example cache_bench
//! ```

#[path = "../tests/common/cars.rs"]
mod cars;
#[path = "../tests/common/synth.rs"]
mod synth;

use lsim_codegen::{CodegenOptions, CompileReport, compile};
use lsim_ir::PreparedModel;
use lsim_project::{ImportOptions, import_case, standard_registry};
use std::time::Instant;

/// The best of `n` runs of `f`, s, and its last result.
fn best<T>(n: usize, mut f: impl FnMut() -> T) -> (f64, T) {
    let mut t_best = f64::INFINITY;
    let mut last = None;
    for _ in 0..n {
        let t = Instant::now();
        let r = f();
        t_best = t_best.min(t.elapsed().as_secs_f64());
        last = Some(r);
    }
    (t_best, last.expect("ran"))
}

fn report(name: &str, m: &PreparedModel, prepare: Option<f64>) {
    let json = serde_json::to_vec(m).expect("serialises");
    let (load, back) = best(5, || serde_json::from_slice::<PreparedModel>(&json).expect("parses"));
    assert_eq!(back.assignments.len(), m.assignments.len());
    let compiled = |threads: usize| -> (f64, CompileReport) {
        let o = CodegenOptions { threads, tiered_above: usize::MAX, ..Default::default() };
        let (t, j) = best(5, || compile(m, &o).expect("compiles"));
        (t, j.report)
    };
    let (t4, r4) = compiled(0);
    let (t1, r1) = compiled(1);
    // a file of the machine code's size, written and read back
    let path = std::env::temp_dir().join(format!("lsim-cache-bench-{}.bin", std::process::id()));
    std::fs::write(&path, vec![0x90u8; r1.code_bytes]).expect("writable");
    let (read, bytes) = best(5, || std::fs::read(&path).expect("readable"));
    let _ = std::fs::remove_file(&path);
    assert_eq!(bytes.len(), r1.code_bytes);
    // what a hit would still do: the analysis (layout, plans, tapes and
    // kernels could be cached too: their size is of the code's order),
    // the read and the link
    let hit = r1.analysis_seconds + read + r1.link_seconds;
    let ms = |s: f64| s * 1e3;
    println!("== {name}: {} nodes, {} kB of machine code", r1.nodes, r1.code_bytes / 1024);
    if let Some(p) = prepare {
        println!("  preparing (what the prepared-model cache saves): {:.2} ms", ms(p));
    }
    println!(
        "  a hit today: the prepared model from {} kB of JSON {:.2} ms, then compiling {:.2} ms on {} threads, {:.2} ms on one",
        json.len() / 1024,
        ms(load),
        ms(t4),
        r4.threads,
        ms(t1)
    );
    println!(
        "    of which (one thread) analysis {:.2} ms, IR and tapes {:.2} ms, Cranelift {:.2} ms, link {:.2} ms",
        ms(r1.analysis_seconds),
        ms(r1.ir_seconds),
        ms(r1.codegen_seconds - r1.link_seconds),
        ms(r1.link_seconds)
    );
    println!(
        "  a machine-code hit instead: analysis {:.2} + read {:.3} + link {:.2} = {:.2} ms: saves {:.2} ms on {} threads, {:.2} ms on one",
        ms(r1.analysis_seconds),
        ms(read),
        ms(r1.link_seconds),
        ms(hit),
        ms(t4 - hit),
        r4.threads,
        ms(t1 - hit)
    );
}

fn main() {
    let reg = standard_registry();
    for (name, project) in cars::projects() {
        // one case per project (a project's cases share its model)
        let Some(case) = project["cases"].as_array().into_iter().flatten().find(|c| {
            import_case(&project, c["id"].as_str(), &reg, &ImportOptions::default()).is_ok()
        }) else {
            continue;
        };
        let id = case["id"].as_str().expect("an id");
        let (top, rep) =
            import_case(&project, Some(id), &reg, &ImportOptions::default()).expect("imports");
        let lib = rep.library();
        let (prep, m) =
            best(3, || lsim_prep::prepare(&lib, &top, &Default::default()).expect("prepares"));
        report(&format!("{name}/{id}"), &m, Some(prep));
    }
    report("network(1430), 10 000 equations", &synth::network(1430, 7), None);
}
