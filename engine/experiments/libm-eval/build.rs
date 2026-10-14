//! Builds CORE-MATH's 13 functions three ways: for the x86-64 baseline
//! with the C library's fma (`coremath`), with `-mfma` (`coremath_fma`, as
//! the engine's FMA build) and for the baseline with the exact software
//! fma (`coremath_soft`, as the engine's other build). The sources are the
//! engine's vendored ones unless `CORE_MATH` names another
//! `core-math/src/binary64` (a checkout at another revision). On Windows
//! the compiler is clang-cl, as in the engine's build (MSVC's cl lacks
//! `__int128`), its GCC-style options behind `/clang:`.

const FUNCS: &[&str] = &[
    "sin", "cos", "tan", "asin", "acos", "atan", "atan2", "sinh", "cosh", "tanh", "exp", "log",
    "pow",
];

fn main() {
    let here = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let math = here.join("../../crates/lsim-math");
    let cm = std::env::var("CORE_MATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| math.join("core-math/binary64"));
    let csrc = math.join("csrc");
    for (tag, fma, soft) in [("cmA", false, false), ("cmB", true, false), ("cmC", false, true)] {
        let mut b = cc::Build::new();
        b.prefer_clang_cl_over_msvc(true);
        let compiler = b.get_compiler();
        let clang_cl = compiler.is_like_clang_cl();
        assert!(!compiler.is_like_msvc() || clang_cl, "CORE-MATH needs clang-cl on Windows");
        let opt = |o: &str| if clang_cl { format!("/clang:{o}") } else { o.to_string() };
        b.opt_level(2).warnings(false).flag(opt("-ffp-contract=off")).flag(opt("-fno-math-errno"));
        if fma {
            b.flag(opt("-mfma"));
        }
        if soft {
            b.define("LSIM_SOFT_FMA", None);
            let shim = csrc.join("lsim_math_shim.h");
            if clang_cl {
                b.flag(format!("/FI{}", shim.display()));
            } else {
                b.flag("-include").flag(shim.display().to_string());
            }
            b.file(csrc.join("lsim_math_fma.c"));
        }
        for f in FUNCS {
            b.file(cm.join(f).join(format!("{f}.c")));
            b.include(cm.join(f));
            b.define(&format!("cr_{f}"), Some(format!("{tag}_{f}").as_str()));
        }
        let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join(tag);
        std::fs::create_dir_all(&out).unwrap();
        b.out_dir(out);
        b.compile(tag);
    }
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=CORE_MATH");
}
