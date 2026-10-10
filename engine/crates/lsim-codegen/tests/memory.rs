//! A dropped `JitModel` gives its machine code back (from the review of
//! WP3). cranelift-jit 0.134's `SystemMemoryProvider` leaks every
//! allocation on drop unless `JITModule::free_memory` is called; the
//! compiled model's `CodeMemory` calls it when its last holder goes away.
//! Without that, each compile left its code mapped for the rest of the
//! process (200 compiles of a car: 22.4 MB of executable memory), so an
//! app that recompiles on every edit, or a design study over structurally
//! different variants, grew without bound.
//!
//! Measured (Linux) by the process's anonymous executable mappings, before
//! and after compiling and dropping the same model many times: a model
//! compiled at once, and a tiered one whose machine code was compiled on
//! another thread.

#[path = "common/cars.rs"]
mod cars;
#[path = "common/synth.rs"]
mod synth;

use lsim_codegen::{CodegenOptions, compile};
use std::sync::Mutex;

/// The tests measure the whole process's mappings: one at a time.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// The bytes of anonymous executable memory mapped in the process (the
/// JIT's code pages: `r-xp` with no file behind them).
fn exec_anon_bytes() -> usize {
    std::fs::read_to_string("/proc/self/maps")
        .map(|s| {
            s.lines()
                .filter_map(|l| {
                    let f: Vec<&str> = l.split_whitespace().collect();
                    (f.len() == 5 && f[1].contains('x')).then(|| {
                        let (a, b) = f[0].split_once('-').unwrap();
                        usize::from_str_radix(b, 16).unwrap()
                            - usize::from_str_radix(a, 16).unwrap()
                    })
                })
                .sum()
        })
        .unwrap_or(0)
}

/// The process's resident set, kB.
fn rss_kb() -> usize {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmRSS:"))
                .and_then(|l| l.split_whitespace().nth(1)?.parse().ok())
        })
        .unwrap_or(0)
}

#[test]
fn a_dropped_model_gives_its_machine_code_back() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let car = cars::one_per_project(cars::cars("")).into_iter().next().expect("an example car");
    let opts = CodegenOptions { threads: 1, ..Default::default() };
    // warm up (allocator, lazily initialised state)
    for _ in 0..3 {
        drop(compile(&car.model, &opts).expect("compiles"));
    }
    let (maps0, rss0) = (exec_anon_bytes(), rss_kb());
    let rounds = 200;
    let mut bytes = 0;
    for _ in 0..rounds {
        let m = compile(&car.model, &opts).expect("compiles");
        bytes = m.code_bytes;
        drop(m);
    }
    let (maps1, rss1) = (exec_anon_bytes(), rss_kb());
    println!(
        "{}: {} bytes of machine code a compile; after {rounds} compiles, each dropped: \
         {} kB more anonymous executable memory, {} kB more resident",
        car.name,
        bytes,
        (maps1 as i64 - maps0 as i64) / 1024,
        rss1 as i64 - rss0 as i64
    );
    assert!(
        maps1 < maps0 + 2 * bytes,
        "{} kB of executable memory left behind by {rounds} dropped models",
        (maps1 - maps0) / 1024
    );
}

#[test]
fn a_dropped_tiered_model_gives_its_machine_code_back() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let m = synth::network(300, 3);
    let opts = CodegenOptions { tiered_above: 0, threads: 1, ..Default::default() };
    let run = || {
        let j = compile(&m, &opts).expect("compiles");
        let r = j.wait_machine_code().expect("tiered").expect("machine code");
        drop(j);
        r.code_bytes
    };
    for _ in 0..2 {
        run();
    }
    let maps0 = exec_anon_bytes();
    let rounds = 20;
    let mut bytes = 0;
    for _ in 0..rounds {
        bytes = run();
    }
    let maps1 = exec_anon_bytes();
    println!(
        "network(300), tiered: {bytes} bytes of machine code a compile; after {rounds} compiles, \
         each dropped: {} kB more anonymous executable memory",
        (maps1 as i64 - maps0 as i64) / 1024
    );
    assert!(
        maps1 < maps0 + 2 * bytes,
        "{} kB of executable memory left behind by {rounds} dropped tiered models",
        (maps1 - maps0) / 1024
    );
}
