//! No engine code calls a mathematical function but through this crate.
//!
//! The platform's `sin`, `exp`, `powf`… (Rust's `f64` methods call the C
//! runtime's: glibc here, the MSVC UCRT on Windows) differ between
//! platforms and CPUs in the last place, and are not all within the ulp
//! the interval code relies on; `lsim_math`'s are correctly rounded and the
//! same everywhere. Two checks keep it so:
//!
//! * clippy's `disallowed-methods` (`engine/clippy.toml`, which `check.sh`
//!   and CI run with warnings as errors) rejects every `f64` and `f32`
//!   method below wherever it is called, by type: in the sources, the
//!   tests and the examples. This test checks the list is whole, and that
//!   nothing outside this crate allows them.
//! * This test reads the sources (each crate's `src`, `examples`, `benches`
//!   and build script; not `#[cfg(test)]` items, which clippy covers): no
//!   `.sin(`, `f64::exp`, `libm::` or C declaration of a libm function.
//!
//! `powi` is among them: Rust's `f64::powi` multiplies by repeated
//! squaring on Linux but calls the C runtime's `pow` on Windows (Windows
//! CI, October 2026: 0.628681636656856^32 differs), so `lsim_math::powi`
//! does the squaring itself, the same steps everywhere.
//!
//! Exceptions, on purpose: `sqrt`, which IEEE 754 rounds correctly and
//! Rust computes with the instruction on every target, and the exact
//! operations (`abs`, `floor`, `min`…).

use std::path::{Path, PathBuf};

/// The `f64` (and `f32`) methods that call the platform's mathematical
/// library: every one must come from `lsim_math` instead.
const BANNED: &[&str] = &[
    "sin", "cos", "tan", "asin", "acos", "atan", "atan2", "sin_cos", "sinh", "cosh", "tanh",
    "asinh", "acosh", "atanh", "exp", "exp2", "exp_m1", "ln", "log", "log2", "log10", "ln_1p",
    "powf", "powi", "cbrt", "hypot", "mul_add", "gamma", "ln_gamma",
];

/// The C library's names for them (a Rust `extern` block must not declare
/// one).
const C_NAMES: &[&str] = &[
    "sin", "cos", "tan", "asin", "acos", "atan", "atan2", "sinh", "cosh", "tanh", "asinh", "acosh",
    "atanh", "exp", "exp2", "expm1", "log", "log2", "log10", "log1p", "pow", "cbrt", "hypot",
    "fma", "erf", "erfc", "tgamma", "lgamma",
];

fn engine() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every Rust file of the engine's crates but this one, outside `tests`.
fn sources() -> Vec<PathBuf> {
    let mut files = vec![];
    let crates = engine().join("crates");
    for c in std::fs::read_dir(&crates).expect("the crates") {
        let c = c.expect("an entry").path();
        if c.file_name().is_some_and(|n| n == "lsim-math") || !c.join("Cargo.toml").exists() {
            continue;
        }
        let build = c.join("build.rs");
        if build.exists() {
            files.push(build);
        }
        let mut stack: Vec<PathBuf> = ["src", "examples", "benches"]
            .iter()
            .map(|d| c.join(d))
            .filter(|d| d.is_dir())
            .collect();
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).expect("a folder") {
                let p = e.expect("an entry").path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    files.push(p);
                }
            }
        }
    }
    files.sort();
    files
}

/// The lines of a file that are code: comments cut off, `#[cfg(test)]`
/// items left out (an item from the attribute to the closing brace at its
/// indentation, or its one line).
fn code_lines(path: &Path) -> Vec<(usize, String)> {
    let text = std::fs::read_to_string(path).expect("readable");
    let lines: Vec<&str> = text.lines().collect();
    let mut out = vec![];
    let mut i = 0;
    while i < lines.len() {
        let l = lines[i];
        if l.trim() == "#[cfg(test)]" {
            let indent = l.len() - l.trim_start().len();
            let item = lines.get(i + 1).copied().unwrap_or("");
            i += 1;
            if item.trim_end().ends_with('{') {
                let close = format!("{}}}", &l[..indent]);
                while i < lines.len() && lines[i].trim_end() != close {
                    i += 1;
                }
            }
            i += 1;
            continue;
        }
        out.push((i + 1, l.split("//").next().unwrap_or("").to_string()));
        i += 1;
    }
    out
}

#[test]
fn engine_code_calls_no_platform_maths() {
    let files = sources();
    assert!(files.len() > 60, "{} source files", files.len());
    let mut found = vec![];
    for f in &files {
        let mut in_extern = false;
        for (n, code) in code_lines(f) {
            let at = |what: &str| format!("{}:{n}: {what}: {}", f.display(), code.trim());
            for name in BANNED {
                if code.contains(&format!(".{name}(")) {
                    found.push(at(&format!("`.{name}(`")));
                }
                for ty in ["f64", "f32"] {
                    let p = format!("{ty}::{name}");
                    if let Some(i) = code.find(&p) {
                        let after = code[i + p.len()..].chars().next();
                        if !after.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                            found.push(at(&format!("`{p}`")));
                        }
                    }
                }
            }
            if code.contains("libm::") || code.contains("intrinsics::") {
                found.push(at("another mathematical library"));
            }
            if code.contains("extern \"C\" {") || code.contains("extern {") {
                in_extern = true;
            } else if in_extern && code.trim() == "}" {
                in_extern = false;
            } else if in_extern {
                for c in C_NAMES {
                    if code.contains(&format!("fn {c}(")) {
                        found.push(at(&format!("the C library's `{c}`")));
                    }
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "mathematical functions called outside lsim-math (use lsim_ir::math or lsim_math):\n{}",
        found.join("\n")
    );
}

#[test]
fn clippy_rejects_the_platforms_maths_everywhere_else() {
    let toml = std::fs::read_to_string(engine().join("clippy.toml")).expect("engine/clippy.toml");
    for name in BANNED {
        for ty in ["f64", "f32"] {
            let entry = format!("\"{ty}::{name}\"");
            assert!(toml.contains(&entry), "clippy.toml does not disallow {entry}");
        }
    }
    // only this crate may allow them (the base instruction set's fused
    // multiply-add off x86-64)
    let mut allows = vec![];
    let crates = engine().join("crates");
    let mut stack = vec![crates];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).expect("a folder") {
            let p = e.expect("an entry").path();
            if p.is_dir() {
                if p.file_name().is_some_and(|n| n != "target" && n != "lsim-math") {
                    stack.push(p);
                }
            } else if p.extension().is_some_and(|x| x == "rs") {
                let text = std::fs::read_to_string(&p).expect("readable");
                if text.contains("disallowed_methods") {
                    allows.push(p.display().to_string());
                }
            }
        }
    }
    assert!(allows.is_empty(), "clippy::disallowed_methods allowed in {allows:?}");
}
