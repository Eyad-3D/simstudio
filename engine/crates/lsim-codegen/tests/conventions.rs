//! Calling conventions: generated code and the Rust functions on either
//! side of it (the runtime symbols it calls, the function pointers the
//! model calls it through) must use the target's own convention, the one
//! Rust's `extern "C"` means there: System V on Linux and macOS on x86-64,
//! the Windows x64 convention on Windows. A signature built with a fixed
//! convention would work on one platform and corrupt arguments on the
//! other.
//!
//! * Every compiled function is held to the target's default convention
//!   (`jit::check_conventions`): compiling the example projects and random
//!   models here, and compiling them for the other x86-64 platform as far
//!   as machine code (which cannot run here), must succeed and report that
//!   platform's convention.
//! * The sources must not name a convention: no `CallConv::` outside the
//!   one place signatures are made, every `extern` function `extern "C"`.

#[path = "common/cars.rs"]
mod cars;
#[path = "common/random.rs"]
mod random;
#[path = "common/synth.rs"]
mod synth;

use lsim_codegen::{CodegenOptions, compile, compile_for_target};
use synth::Rng;

/// The host's default convention, as Cranelift names it.
fn host_convention() -> &'static str {
    if cfg!(all(target_arch = "aarch64", target_vendor = "apple")) {
        "apple_aarch64"
    } else if cfg!(windows) {
        "windows_fastcall"
    } else {
        "system_v"
    }
}

/// The other x86-64 platform: Windows from elsewhere, Linux from Windows.
fn other_target() -> (&'static str, &'static str) {
    if cfg!(windows) {
        ("x86_64-unknown-linux-gnu", "system_v")
    } else {
        ("x86_64-pc-windows-msvc", "windows_fastcall")
    }
}

/// Settings that take every path of the code generator: the default,
/// the large-model one (no optimisation, the single-pass allocator, the
/// residual split from the channels, chunks), and Jacobian-vector
/// products and initialisation compiled rather than taped.
fn settings() -> Vec<CodegenOptions> {
    vec![
        CodegenOptions::default(),
        CodegenOptions { auto_fast_above: 0, chunk_nodes: 400, ..Default::default() },
        CodegenOptions { compile_jvp: true, tape_init: false, ..Default::default() },
    ]
}

#[test]
fn generated_code_uses_the_targets_own_calling_convention() {
    let (triple, theirs) = other_target();
    let mut models: Vec<(String, lsim_ir::PreparedModel)> =
        cars::one_per_project(cars::cars("")).into_iter().map(|c| (c.name, c.model)).collect();
    let mut r = Rng(41);
    for k in 0..20 {
        let m = random::implicit_model(&mut r, 3, 2, 12, 3, 4, random::ALL_OPS);
        models.push((format!("random model {k}"), m));
    }
    let mut foreign = 0;
    for (name, m) in &models {
        for opts in settings() {
            let here = compile(m, &opts).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(here.report.call_conv, host_convention(), "{name}");
            if cfg!(target_arch = "x86_64") {
                let there = compile_for_target(m, &opts, triple)
                    .unwrap_or_else(|e| panic!("{name} for {triple}: {e}"));
                assert_eq!(there.call_conv, theirs, "{name}");
                assert_eq!(there.functions, here.report.functions, "{name}");
                assert!(there.functions == 0 || there.code_bytes > 0, "{name}");
                foreign += there.functions;
            }
        }
    }
    if cfg!(target_arch = "x86_64") {
        assert!(foreign > 100, "{foreign} functions compiled for {triple}");
    }
}

/// The lines of a source file before its tests.
fn code_lines(path: &std::path::Path) -> Vec<(usize, String)> {
    let text = std::fs::read_to_string(path).expect("readable");
    text.lines()
        .take_while(|l| !l.trim_start().starts_with("#[cfg(test)]"))
        .enumerate()
        .map(|(i, l)| (i + 1, l.to_string()))
        .collect()
}

#[test]
fn the_sources_never_name_a_calling_convention() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = vec![];
    let mut stack = vec![dir];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).expect("the sources") {
            let p = e.expect("an entry").path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                files.push(p);
            }
        }
    }
    assert!(files.len() >= 8, "{files:?}");
    let mut externs = 0;
    for f in &files {
        for (n, line) in code_lines(f) {
            let at = format!("{}:{n}: {line}", f.display());
            let code = line.split("//").next().unwrap_or("");
            assert!(!code.contains("CallConv::"), "a fixed calling convention at {at}");
            assert!(
                !code.contains("Signature::new(") || code.contains("isa.default_call_conv()"),
                "a signature not made by jit::signature at {at}"
            );
            assert!(!code.contains("make_signature"), "a module-made signature at {at}");
            if let Some(i) = code.find("extern \"") {
                assert!(
                    code[i..].starts_with("extern \"C\""),
                    "an extern other than \"C\" at {at}"
                );
                externs += 1;
            }
        }
    }
    // the runtime symbols and the generated functions' pointer type
    assert!(externs >= 8, "{externs} extern \"C\" items found");
}
