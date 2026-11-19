//! SUNDIALS' own C code takes its mathematical functions from the engine's
//! library (lsim-math), not from the C runtime: a step size or an order
//! chosen from a `pow` that differs in the last place between platforms
//! changes a run's result.
//!
//! `build.rs` includes `csrc/lsim_sundials_math.h` first into every C file
//! it compiles; that header maps each function of `<math.h>` that is not
//! exact to lsim-math's, or to a name nothing defines (a new call does not
//! build). These tests check that the build does so, that every function
//! the compiled sources call is either mapped or exact, and that SUNDIALS'
//! `pow` gives lsim-math's correctly rounded result where glibc's does
//! not.

use std::path::{Path, PathBuf};

fn here() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// `<math.h>`'s functions that are exact in IEEE arithmetic (or are
/// classifications), which may stay the C runtime's.
const EXACT: &[&str] = &[
    "sqrt",
    "fabs",
    "ceil",
    "floor",
    "trunc",
    "round",
    "nearbyint",
    "rint",
    "fmax",
    "fmin",
    "fmod",
    "remainder",
    "copysign",
    "ldexp",
    "scalbn",
    "frexp",
    "modf",
    "nextafter",
    "isnan",
    "isinf",
    "isfinite",
    "signbit",
    "fpclassify",
];

/// The rest of `<math.h>` (double, float and long double).
const INEXACT: &[&str] = &[
    "sin", "cos", "tan", "asin", "acos", "atan", "atan2", "sinh", "cosh", "tanh", "asinh", "acosh",
    "atanh", "exp", "exp2", "expm1", "log", "log2", "log10", "log1p", "pow", "cbrt", "hypot",
    "fma", "erf", "erfc", "tgamma", "lgamma",
];

/// The C files the build compiles, from `build.rs`.
fn compiled_sources() -> Vec<PathBuf> {
    let build = std::fs::read_to_string(here().join("build.rs")).expect("build.rs");
    let mut out = vec![];
    for line in build.lines() {
        let t = line.trim();
        if let Some(name) = t.strip_prefix('"').and_then(|s| s.strip_suffix("\","))
            && name.ends_with(".c")
        {
            out.push(here().join("sundials/src").join(name));
        }
        // LightSim's own files
        if t.starts_with("for f in [\"lsim_") {
            for name in t.split('"').filter(|s| s.ends_with(".c")) {
                out.push(here().join("csrc").join(name));
            }
        }
    }
    assert!(out.len() > 50, "{out:?}");
    out
}

fn headers(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).expect("a folder") {
        let p = e.expect("an entry").path();
        if p.is_dir() {
            headers(&p, out);
        } else if p.extension().is_some_and(|x| x == "h") {
            out.push(p);
        }
    }
}

/// C text without its comments.
fn code(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    loop {
        let (b, l) = (rest.find("/*"), rest.find("//"));
        match (b, l) {
            (Some(b), l) if l.is_none_or(|l| b < l) => {
                out.push_str(&rest[..b]);
                rest = rest[b + 2..].find("*/").map_or("", |e| &rest[b + 2 + e + 2..]);
            }
            (_, Some(l)) => {
                out.push_str(&rest[..l]);
                rest = rest[l..].find('\n').map_or("", |e| &rest[l + e..]);
            }
            _ => {
                out.push_str(rest);
                return out;
            }
        }
    }
}

/// The functions of `<math.h>` that `text` calls (a name followed by `(`
/// that is not part of a longer name or a member).
fn calls(text: &str) -> Vec<String> {
    let mut found = vec![];
    let b = text.as_bytes();
    for name in EXACT.iter().chain(INEXACT) {
        for variant in [name.to_string(), format!("{name}f"), format!("{name}l")] {
            let mut from = 0;
            while let Some(i) = text[from..].find(&variant) {
                let s = from + i;
                let e = s + variant.len();
                from = e;
                let before = if s == 0 { b' ' } else { b[s - 1] };
                if before.is_ascii_alphanumeric()
                    || before == b'_'
                    || before == b'.'
                    || before == b'>'
                {
                    continue;
                }
                let after = text[e..].trim_start();
                if after.starts_with('(') && !found.contains(&variant) {
                    found.push(variant.clone());
                }
            }
        }
    }
    found
}

/// The names `csrc/lsim_sundials_math.h` maps to the engine's library, and
/// those it maps to a name nothing defines.
fn shim() -> (Vec<String>, Vec<String>) {
    let text = std::fs::read_to_string(here().join("csrc/lsim_sundials_math.h")).expect("shim");
    let (mut to_lib, mut refused) = (vec![], vec![]);
    for line in text.lines() {
        let Some(def) = line.strip_prefix("#define ") else { continue };
        let Some((name, rest)) = def.split_once('(') else { continue };
        if rest.contains("lsim_math_has_no_") {
            refused.push(name.to_string());
        } else if rest.contains(&format!("lsim_math_{name}(")) {
            to_lib.push(name.to_string());
        }
    }
    (to_lib, refused)
}

#[test]
fn the_build_includes_the_maths_header_first_in_every_file() {
    let build = std::fs::read_to_string(here().join("build.rs")).expect("build.rs");
    assert!(build.contains("lsim_sundials_math.h"), "build.rs does not name the header");
    assert!(build.contains("\"-include\"") && build.contains("/FI"), "no forced include");
    assert!(build.contains("DEP_LSIM_MATH_INCLUDE"), "lsim-math's header not on the path");
}

#[test]
fn every_inexact_function_sundials_calls_is_the_engines() {
    let (to_lib, refused) = shim();
    // every inexact function of <math.h> is mapped one way or the other
    for name in INEXACT {
        assert!(
            to_lib.iter().any(|n| n == name) || refused.iter().any(|n| n == name),
            "{name} is not mapped by lsim_sundials_math.h"
        );
    }
    let mut files = compiled_sources();
    headers(&here().join("sundials"), &mut files);
    headers(&here().join("csrc"), &mut files);
    let mut wrong = vec![];
    let mut seen = vec![];
    for f in files.iter().filter(|f| !f.ends_with("lsim_sundials_math.h")) {
        let text = code(&std::fs::read_to_string(f).expect("readable"));
        for c in calls(&text) {
            // (the float and long double variants: `ceilf`, `ceill`)
            let base = c.strip_suffix(['f', 'l']).unwrap_or(&c);
            let exact = EXACT.contains(&c.as_str()) || EXACT.contains(&base);
            if !exact && !to_lib.contains(&c) && !refused.contains(&c) {
                wrong.push(format!("{}: {c}", f.display()));
            }
            if !seen.contains(&c) {
                seen.push(c);
            }
        }
    }
    assert!(wrong.is_empty(), "calls the C runtime's maths: {wrong:?}");
    // what SUNDIALS 7.1.1 calls: pow (SUNRpowerR) and exp (SUNRexp) inexact
    for c in ["pow", "exp", "sqrt"] {
        assert!(seen.iter().any(|s| s == c), "{c} not found: {seen:?}");
    }
}

#[test]
fn sundials_pow_rounds_correctly_where_glibc_does_not() {
    // (x, y, pow(x, y) correctly rounded): hard-to-round cases from
    // CORE-MATH's list where glibc 2.39's pow is one ulp off
    let cases: [(u64, u64, u64); 6] = [
        (0x3fe619e4bd9e9323, 0x3fd4d31ec6d44040, 0x3fec5e8c40cb428a),
        (0x3fe585e1192393de, 0x3fc274cefe288080, 0x3fee38a93eb53e1e),
        (0x3fe726cd8f8aa920, 0x3fdf921e84d44040, 0x3feb47185aa1cf90),
        (0x3fe5ac6d6dc2a1b2, 0x3fde467e38d44040, 0x3fea9d065f15fab6),
        (0x3fe022b42ddd8b2b, 0x3fec4e3e8c46f468, 0x3fe1767bf0f6aed2),
        (0x3fe30b3e414e3d3b, 0x3fea6c0a38da8066, 0x3fe4d8d99ac41402),
    ];
    for (x, y, r) in cases {
        let (x, y) = (f64::from_bits(x), f64::from_bits(y));
        // SAFETY: a pure function of two doubles
        let v = unsafe { lsim_sundials_sys::SUNRpowerR(x, y) };
        assert_eq!(v.to_bits(), r, "SUNRpowerR({x:e}, {y:e})");
        assert_eq!(v.to_bits(), lsim_math::pow(x, y).to_bits());
    }
}
