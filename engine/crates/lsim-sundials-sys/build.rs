//! Compiles the vendored SUNDIALS 7.1.1 C sources with the `cc` crate: no
//! CMake, no libclang, only the C compiler every Rust target already needs
//! (MSVC on Windows, the Xcode command-line tools on macOS, gcc or clang on
//! Linux). The configuration header (`sundials/include/sundials/
//! sundials_config.h`) and the Rust bindings (`src/bindings.rs`) are
//! committed.

use std::path::{Path, PathBuf};

/// The C files, relative to `sundials/src`.
const SOURCES: &[&str] = &[
    // core
    "sundials/sundials_adaptcontroller.c",
    "sundials/sundials_band.c",
    "sundials/sundials_context.c",
    "sundials/sundials_dense.c",
    "sundials/sundials_direct.c",
    "sundials/sundials_errors.c",
    "sundials/sundials_futils.c",
    "sundials/sundials_hashmap.c",
    "sundials/sundials_iterative.c",
    "sundials/sundials_linearsolver.c",
    "sundials/sundials_logger.c",
    "sundials/sundials_math.c",
    "sundials/sundials_matrix.c",
    "sundials/sundials_memory.c",
    "sundials/sundials_nonlinearsolver.c",
    "sundials/sundials_nvector_senswrapper.c",
    "sundials/sundials_nvector.c",
    "sundials/sundials_profiler.c",
    "sundials/sundials_version.c",
    // vectors, matrices, linear and nonlinear solvers
    "nvector/serial/nvector_serial.c",
    "sunmatrix/dense/sunmatrix_dense.c",
    "sunmatrix/band/sunmatrix_band.c",
    "sunmatrix/sparse/sunmatrix_sparse.c",
    "sunlinsol/dense/sunlinsol_dense.c",
    "sunlinsol/band/sunlinsol_band.c",
    "sunnonlinsol/newton/sunnonlinsol_newton.c",
    "sunnonlinsol/fixedpoint/sunnonlinsol_fixedpoint.c",
    // CVODES (CVODE with quadratures and sensitivities)
    "cvodes/cvodea.c",
    "cvodes/cvodea_io.c",
    "cvodes/cvodes.c",
    "cvodes/cvodes_bandpre.c",
    "cvodes/cvodes_bbdpre.c",
    "cvodes/cvodes_diag.c",
    "cvodes/cvodes_io.c",
    "cvodes/cvodes_ls.c",
    "cvodes/cvodes_nls.c",
    "cvodes/cvodes_nls_sim.c",
    "cvodes/cvodes_nls_stg.c",
    "cvodes/cvodes_nls_stg1.c",
    "cvodes/cvodes_proj.c",
    // IDAS (IDA with quadratures and sensitivities)
    "idas/idas.c",
    "idas/idaa.c",
    "idas/idas_io.c",
    "idas/idas_ic.c",
    "idas/idaa_io.c",
    "idas/idas_ls.c",
    "idas/idas_bbdpre.c",
    "idas/idas_nls.c",
    "idas/idas_nls_sim.c",
    "idas/idas_nls_stg.c",
    // KINSOL
    "kinsol/kinsol.c",
    "kinsol/kinsol_bbdpre.c",
    "kinsol/kinsol_io.c",
    "kinsol/kinsol_ls.c",
];

fn main() {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("sundials");
    let src = root.join("src");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=sundials");

    let mut b = cc::Build::new();
    b.include(root.join("include"));
    for dir in ["sundials", "cvodes", "idas", "kinsol"] {
        b.include(src.join(dir));
    }
    b.define("SUNDIALS_STATIC_DEFINE", None);
    // the same optimisation in every Cargo profile, so results do not
    // depend on whether the engine was built for debugging
    b.opt_level(2);
    b.debug(false);
    b.warnings(false);
    // IEEE arithmetic as written: no fused multiply-adds the source did not
    // ask for, so x86-64 and arm64 builds give the same numbers
    b.flag_if_supported("-ffp-contract=off");
    if b.get_compiler().is_like_msvc() {
        b.flag("/fp:precise");
        b.define("_CRT_SECURE_NO_WARNINGS", None);
    }
    for f in SOURCES {
        let p: &Path = &src.join(f);
        assert!(p.exists(), "missing SUNDIALS source {}", p.display());
        b.file(p);
    }
    // LightSim's own additions (dense output of selected components)
    let own = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("csrc");
    println!("cargo:rerun-if-changed=csrc");
    for f in ["lsim_cvodes_dky.c", "lsim_idas_dky.c"] {
        b.file(own.join(f));
    }
    b.compile("lsim_sundials");
    // where the headers are, for any dependent build script
    println!("cargo:include={}", root.join("include").display());
}
