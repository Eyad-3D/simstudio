//! The prepared-model cache's code identity (`src/cache.rs`).
//!
//! A cache hit must be the model preparation would give. The key holds
//! the model's inputs, but what preparation makes of them is set by the
//! code that prepares: a change there (a field added to a prepared set, a
//! choice made otherwise, a new warning) leaves the inputs the same. The
//! key's format string, bumped by hand, missed such a change once (the
//! second review of dynamic state selection, F1: an entry written before
//! `StateSet` had the fields that tell a run which set is which read with
//! them empty, and the run stopped). So the key holds this digest too: of
//! the sources that turn inputs into a prepared model, lsim-prep (the
//! preparation), lsim-ir (the types it writes and the cache reads) and
//! lsim-math (the mathematics both compute with), and of the workspace's
//! `Cargo.lock` (the versions of what they depend on). An engine built
//! from other preparation code computes other keys, whether or not anyone
//! bumped the format.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// The crates whose code shapes a prepared model.
const CRATES: &[&str] = &["lsim-ir", "lsim-math", "lsim-prep"];

/// What of each crate is code (its tests, tools and notes are not).
const PARTS: &[&str] =
    &["Cargo.toml", "build.rs", "c_flags.rs", "src", "csrc", "include", "core-math"];

fn main() {
    let here = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let crates = here.parent().expect("the crates' folder");
    let root = crates.parent().expect("the workspace");
    let mut files = vec![root.join("Cargo.lock")];
    for c in CRATES {
        for p in PARTS {
            let path = crates.join(c).join(p);
            if path.exists() {
                walk(&path, &mut files);
            }
        }
    }
    let mut named: Vec<(String, PathBuf)> = files
        .into_iter()
        .map(|f| {
            let rel = f.strip_prefix(root).expect("in the workspace");
            let name: Vec<String> =
                rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
            (name.join("/"), f)
        })
        .collect();
    named.sort();
    let mut h = Sha256::new();
    for (name, f) in &named {
        println!("cargo:rerun-if-changed={}", f.display());
        let bytes = std::fs::read(f).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
        // the code as committed: a checkout with CRLF line ends is the
        // same code
        let mut text = Vec::with_capacity(bytes.len());
        for (i, &b) in bytes.iter().enumerate() {
            if !(b == b'\r' && bytes.get(i + 1) == Some(&b'\n')) {
                text.push(b);
            }
        }
        h.update(name.as_bytes());
        h.update([0]);
        h.update((text.len() as u64).to_le_bytes());
        h.update(&text);
    }
    // a file added anywhere in a crate, also at a code path that did not
    // exist at the last run (a new `build.rs` or `include/`), reruns this
    // script: Cargo watches each crate's folder as a whole. So does an
    // edit of its tests, benches or examples, which rebuilds this crate
    // for a digest that comes out the same (it is over `PARTS` alone): the
    // price of seeing a new code path, as Cargo watches a folder only
    // recursively and reruns on every build a path that does not exist
    for c in CRATES {
        println!("cargo:rerun-if-changed={}", crates.join(c).display());
    }
    let digest: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
    println!("cargo:rustc-env=LSIM_PREPARATION_DIGEST={digest}");
}

/// Every file under `path` (itself, if a file), hidden ones (editors'
/// swap files, `.DS_Store`) left out.
fn walk(path: &Path, out: &mut Vec<PathBuf>) {
    let hidden = path.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.'));
    if hidden {
        return;
    }
    if path.is_dir() {
        let entries = std::fs::read_dir(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        for e in entries {
            walk(&e.unwrap_or_else(|e| panic!("{}: {e}", path.display())).path(), out);
        }
    } else {
        out.push(path.to_path_buf());
    }
}
