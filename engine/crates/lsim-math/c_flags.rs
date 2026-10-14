// The C compiler's options: only those the build script chooses.
//
// Included (`include!`) by the build scripts of lsim-math (CORE-MATH,
// whose correct rounding depends on its options) and lsim-sundials-sys
// (SUNDIALS, whose results the golden comparison pins). Not a module of
// either crate.
//
// The cc crate adds the environment's C flags (`CFLAGS`, `TARGET_CFLAGS`,
// `CFLAGS_<target>`…) after the build's own, so that they win:
// `CFLAGS=-ffast-math` overrode `-ffp-contract=off` and misrounded 2 270
// of lsim-math's 4 672 references in each build, the build succeeding
// silently (review of 80c8ab0). Hence two guards, before and after the
// build is configured.

/// Removes from this build script's environment, before the cc crate takes
/// its snapshot of it, every variable through which options the build
/// does not choose would reach the compiler: the cc crate's C flags
/// (`CFLAGS` with its `HOST_`, `TARGET_` and target-suffixed forms) and
/// `CRATE_CC_NO_DEFAULTS` (which drops the optimisation level and the
/// target's options), `CL` and `_CL_` (which cl and clang-cl read
/// themselves) and `CCC_OVERRIDE_OPTIONS` (clang). Each one set is named in
/// a warning: flags meant for other C code no longer reach this code.
/// Call it first, from `main`.
fn ignore_environment_c_flags(what: &str) {
    let target = std::env::var("TARGET").unwrap_or_default();
    let target_u = target.replace(['-', '.'], "_");
    let names = [
        "CFLAGS".to_string(),
        "HOST_CFLAGS".to_string(),
        "TARGET_CFLAGS".to_string(),
        format!("CFLAGS_{target}"),
        format!("CFLAGS_{target_u}"),
        "CRATE_CC_NO_DEFAULTS".to_string(),
        "CL".to_string(),
        "_CL_".to_string(),
        "CCC_OVERRIDE_OPTIONS".to_string(),
    ];
    for name in &names {
        println!("cargo:rerun-if-env-changed={name}");
        if let Some(value) = std::env::var_os(name) {
            if !value.is_empty() {
                println!(
                    "cargo:warning={what} is compiled with the options its build script chooses \
                     only: {name}={} is ignored for it",
                    value.to_string_lossy()
                );
            }
            // SAFETY: the build script is single-threaded until it builds,
            // and nothing else reads the environment meanwhile.
            unsafe { std::env::remove_var(name) };
        }
    }
}

/// Stops the build if the compiler would get an option that changes
/// floating-point results (what the environment can still pass: `CC` may
/// carry options, as in `CC="gcc -ffast-math"`).
fn refuse_unsafe_fp_options(what: &str, compiler: &cc::Tool) {
    // (prefixes; for clang-cl also behind `/clang:`; the gcc and clang
    // options that relax IEEE semantics, contract, flush subnormals or
    // compute in the x87's extended precision, and MSVC's)
    const UNSAFE: &[&str] = &[
        "-ffast-math",
        "-Ofast",
        "-funsafe-math-optimizations",
        "-fassociative-math",
        "-freciprocal-math",
        "-ffinite-math-only",
        "-fno-honor-infinities",
        "-fno-honor-nans",
        "-fno-signed-zeros",
        "-fapprox-func",
        "-ffp-model=fast",
        "-ffp-model=aggressive",
        "-ffp-contract=fast",
        "-ffp-contract=on",
        "-fdenormal-fp-math=preserve-sign",
        "-fdenormal-fp-math=positive-zero",
        "-mfpmath=387",
        "-mfpmath=both",
        "-mno-sse2",
        "-mrecip",
        "-mdaz-ftz",
        "-menable-unsafe-fp-math",
        "/fp:fast",
        "-fp:fast",
        "/fp:contract",
        "-fp:contract",
    ];
    // (the command line as the cc crate will run it: with what CC carries,
    // which `Tool::args` leaves out)
    let command = compiler.to_command();
    for arg in command.get_args() {
        let arg = arg.to_string_lossy();
        let bare = arg.strip_prefix("/clang:").unwrap_or(&arg);
        if UNSAFE.iter().any(|u| bare.starts_with(u)) {
            panic!(
                "{what}: the C compiler {} would get {arg}, which changes floating-point \
                 results; remove it (from CC or the compiler's configuration)",
                compiler.path().display()
            );
        }
    }
}
