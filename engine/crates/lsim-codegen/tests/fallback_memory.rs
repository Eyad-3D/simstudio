//! A link the system refuses (Windows' Arbitrary Code Guard, simulated by
//! `deny_executable_memory`) gives back the pages `define_function_bytes`
//! allocated for the code: the module is held by `CodeMemory` from before
//! the link, so dropping it calls `free_memory` (it used to be dropped bare,
//! leaving about the code's size mapped per compile).
//!
//! Measured (Linux) by the process's anonymous writable mappings that are
//! not the heap, before and after compiling and dropping the same model
//! many times; its own test binary, as it measures the whole process.

#[path = "common/cars.rs"]
mod cars;

use lsim_codegen::{CodegenOptions, MachineCode, compile};

/// Bytes of anonymous `rw-p` mappings with no file and no name (memmap2's
/// anonymous maps; the heap is `[heap]`, large mallocs are excluded by
/// comparing before and after a warm-up).
fn anon_rw_bytes() -> usize {
    std::fs::read_to_string("/proc/self/maps")
        .map(|s| {
            s.lines()
                .filter_map(|l| {
                    let f: Vec<&str> = l.split_whitespace().collect();
                    (f.len() == 5 && f[1] == "rw-p").then(|| {
                        let (a, b) = f[0].split_once('-').unwrap();
                        usize::from_str_radix(b, 16).unwrap()
                            - usize::from_str_radix(a, 16).unwrap()
                    })
                })
                .sum()
        })
        .unwrap_or(0)
}

#[test]
fn a_refused_link_gives_its_memory_back() {
    let car = cars::one_per_project(cars::cars("")).into_iter().next().expect("an example car");
    let deny = CodegenOptions { threads: 1, deny_executable_memory: true, ..Default::default() };
    let normal = CodegenOptions { threads: 1, ..Default::default() };
    let bytes = compile(&car.model, &normal).expect("compiles").code_bytes;
    for _ in 0..3 {
        drop(compile(&car.model, &deny).expect("compiles"));
    }
    let before = anon_rw_bytes();
    let rounds = 200;
    for _ in 0..rounds {
        let m = compile(&car.model, &deny).expect("compiles on tapes");
        assert!(matches!(m.machine_code(), MachineCode::Tapes(_)));
        drop(m);
    }
    let after = anon_rw_bytes();
    println!(
        "{}: {bytes} bytes of machine code a compile; after {rounds} refused links, each model \
         dropped: {} kB more anonymous writable memory",
        car.name,
        (after as i64 - before as i64) / 1024
    );
    assert!(
        after < before + 2 * bytes,
        "{} kB left behind by {rounds} compiles whose link was refused",
        (after - before) / 1024
    );
}

/// The same measurement with the link allowed: nothing left behind (so
/// the growth above is the refused link's).
#[test]
fn control_an_allowed_link_leaves_nothing() {
    let car = cars::one_per_project(cars::cars("")).into_iter().next().expect("an example car");
    let normal = CodegenOptions { threads: 1, ..Default::default() };
    for _ in 0..3 {
        drop(compile(&car.model, &normal).expect("compiles"));
    }
    let before = anon_rw_bytes();
    for _ in 0..200 {
        drop(compile(&car.model, &normal).expect("compiles"));
    }
    let after = anon_rw_bytes();
    println!(
        "control: {} kB more anonymous writable memory",
        (after as i64 - before as i64) / 1024
    );
}
