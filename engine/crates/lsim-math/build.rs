//! Compiles the vendored CORE-MATH functions (`core-math/`, MIT) with the
//! `cc` crate, twice on x86-64:
//!
//! * `lsim_cm_fma`, for CPUs with the FMA instruction (`-mfma`, which also
//!   assumes AVX): CORE-MATH's fused multiply-adds are the instruction;
//! * `lsim_cm_soft`, for every x86-64 CPU: the same sources with each fused
//!   multiply-add an exact software one (`csrc/lsim_math_fma.c`, musl's).
//!
//! Both compute the same numbers (a fused multiply-add is exact either
//! way); `src/lib.rs` picks one at run time. Contraction of `a * b + c`
//! into a fused multiply-add is off (`-ffp-contract=off`), so each
//! operation is the one the source writes in both builds. Each function
//! `cr_<f>` is renamed `<build>_<f>`.
//!
//! The C compiler is the platform's for GNU-like targets (gcc or clang);
//! for MSVC targets it is clang-cl, from Visual Studio's "C++ Clang tools
//! for Windows" or from LLVM (CORE-MATH uses `__int128` and GCC builtins,
//! which MSVC's own `cl` lacks).
//!
//! It gets the options chosen here and no others: the environment's C
//! flags are ignored (with a warning), and an option that changes
//! floating-point results, wherever it comes from, stops the build
//! (`c_flags.rs`). `src/lib.rs` checks the build it picks once more, on hard
//! cases, at the first call.

use std::path::{Path, PathBuf};

include!("c_flags.rs");

/// CORE-MATH's functions the engine uses (`core-math/binary64/<f>/<f>.c`).
const FUNCTIONS: &[&str] = &[
    "sin", "cos", "tan", "asin", "acos", "atan", "atan2", "sinh", "cosh", "tanh", "exp", "log",
    "pow",
];

fn main() {
    ignore_environment_c_flags("lsim-math (CORE-MATH)");
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    for p in ["build.rs", "c_flags.rs", "core-math", "csrc", "include"] {
        println!("cargo:rerun-if-changed={p}");
    }
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    if arch == "x86_64" {
        build(&root, "lsim_cm_fma", Fma::Instruction);
        build(&root, "lsim_cm_soft", Fma::Software);
    } else {
        // elsewhere (AArch64) the fused multiply-add is in the base
        // instruction set: one build
        build(&root, "lsim_cm_native", Fma::Native);
    }
    // the C declarations, for dependent build scripts (DEP_LSIM_MATH_INCLUDE)
    println!("cargo:include={}", root.join("include").display());
}

#[derive(Clone, Copy, PartialEq)]
enum Fma {
    /// `-mfma`: the instruction
    Instruction,
    /// the x86-64 baseline, an exact software fused multiply-add
    Software,
    /// the target's own (it has the instruction)
    Native,
}

fn build(root: &Path, name: &str, fma: Fma) {
    let mut b = cc::Build::new();
    b.prefer_clang_cl_over_msvc(true);
    let compiler = b.get_compiler();
    let clang_cl = compiler.is_like_clang_cl();
    if compiler.is_like_msvc() && !clang_cl {
        panic!(
            "lsim-math builds CORE-MATH, which needs clang-cl on Windows (MSVC's cl lacks \
             __int128 and the GCC builtins it uses), but the C compiler found is {}. Install \
             the \"C++ Clang tools for Windows\" component of Visual Studio (or LLVM), or set \
             CC_x86_64_pc_windows_msvc to clang-cl.",
            compiler.path().display()
        );
    }
    // a driver option: clang-cl takes GCC-style options behind `/clang:`
    let opt = |o: &str| if clang_cl { format!("/clang:{o}") } else { o.to_string() };
    // the same optimisation in every Cargo profile
    b.opt_level(2);
    b.debug(false);
    b.warnings(false);
    b.flag(opt("-ffp-contract=off"));
    b.flag(opt("-fno-math-errno"));
    if fma == Fma::Instruction {
        b.flag(opt("-mfma"));
    }
    let shim = root.join("csrc").join("lsim_math_shim.h");
    if clang_cl {
        b.flag(format!("/FI{}", shim.display()));
    } else {
        b.flag("-include").flag(shim.display().to_string());
    }
    if fma == Fma::Software {
        b.define("LSIM_SOFT_FMA", None);
        b.file(root.join("csrc").join("lsim_math_fma.c"));
    }
    let src = root.join("core-math").join("binary64");
    for f in FUNCTIONS {
        let dir = src.join(f);
        let file = dir.join(format!("{f}.c"));
        assert!(file.exists(), "missing CORE-MATH source {}", file.display());
        b.file(file);
        b.include(&dir);
        b.define(&format!("cr_{f}"), Some(format!("{name}_{f}").as_str()));
    }
    refuse_unsafe_fp_options("lsim-math (CORE-MATH)", &b.get_compiler());
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join(name);
    std::fs::create_dir_all(&out).expect("the build's object folder");
    b.out_dir(out);
    b.compile(name);
}
