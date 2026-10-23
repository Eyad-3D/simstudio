//! The on-disk cache of prepared models.
//!
//! Key: SHA-256 of the model's inputs — the top component with its
//! parameter values, every library definition it reaches, the connectors,
//! the library's shared enumeration types (an option's value is its place
//! in its type), the preparation options, the mode (full dynamic, or fast mode with its
//! [`InverseSpec`]) — and of what prepares them: the key's format, the
//! engine version and a digest of the preparation's code (`build.rs`:
//! lsim-prep, lsim-ir, lsim-math and the dependency versions), so an
//! engine built from other preparation code never reads this one's
//! entries. Value: the [`PreparedModel`] as JSON. A hit is the model
//! preparation would give, so it skips flattening and structural
//! analysis; machine code is regenerated (milliseconds).
//!
//! Parameter values are in the key because preparation makes choices that
//! depend on them: index reduction picks the states by pivoting at the
//! start point (a pendulum released at 0.4 rad keeps `x`, one released
//! horizontally must keep `y`), and the start checks warn at the start
//! values. A model prepared at other values is not the model. A study at
//! other values sets them on one built model (`Model::params_with`), which
//! checks what the preparation divided by and that the states it chose
//! work at the start; the prepared model carries the choice
//! ([`lsim_ir::StateSet`]), so a run from the cache watches it as any
//! other run does.

use lsim_ir::component::{ComponentDef, Library};
use lsim_ir::{InverseSpec, PreparedModel};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Bumped when what is cached, or how it is keyed, changes. v5: the
/// digest of the preparation's code joined the key (an engine whose sets
/// lacked the fields dynamic state selection reads wrote v4 entries that
/// this engine's v4 key hit, read with the fields empty: the second
/// review's F1). v6: the table reads held in their cells
/// (`PreparedModel::held`, no default: an entry without it is prepared
/// again).
const FORMAT: &str = "prepared-v6";

/// The digest of the code that prepares a model (`build.rs`).
const PREPARATION: &str = env!("LSIM_PREPARATION_DIGEST");

/// The cache key of a model's inputs; `inverse` is fast mode's
/// specification (`None` for full dynamic).
pub fn input_key(
    lib: &Library,
    top: &ComponentDef,
    force_implicit: bool,
    inverse: Option<&InverseSpec>,
) -> String {
    let mut used: BTreeMap<&str, &ComponentDef> = BTreeMap::new();
    let mut todo: Vec<&ComponentDef> = vec![top];
    while let Some(d) = todo.pop() {
        for s in &d.components {
            if let Some(def) = lib.components.get(&s.def)
                && used.insert(&def.name, def).is_none()
            {
                todo.push(def);
            }
        }
    }
    let text = serde_json::to_string(&(
        FORMAT,
        env!("CARGO_PKG_VERSION"),
        PREPARATION,
        top,
        used,
        &lib.connectors,
        &lib.types,
        force_implicit,
        inverse,
    ))
    .expect("the IR serialises");
    Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// A directory of prepared models.
pub struct DiskCache {
    dir: PathBuf,
}

impl DiskCache {
    /// A cache in `dir` (created when first written).
    pub fn new(dir: impl AsRef<Path>) -> Self {
        DiskCache { dir: dir.as_ref().to_path_buf() }
    }

    fn path(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{key}.prepared.json"))
    }

    /// The prepared model stored under `key`, if any and readable.
    pub fn get(&self, key: &str) -> Option<PreparedModel> {
        let text = std::fs::read_to_string(self.path(key)).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// Stores a prepared model; a failure only loses the cache entry.
    pub fn put(&self, key: &str, m: &PreparedModel) {
        let Ok(text) = serde_json::to_string(m) else { return };
        if std::fs::create_dir_all(&self.dir).is_err() {
            return;
        }
        let tmp = self.dir.join(format!("{key}.{}.tmp", std::process::id()));
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, self.path(key));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spike::{Params, battery_drive};

    /// The cache stores the model as JSON: a number must come back with
    /// the same bits, or a cached model is not the model (serde_json's
    /// default parser is off by one unit in the last place for some
    /// numbers; the workspace turns on its `float_roundtrip`). The
    /// Python module's cache test runs a cached car and finds it bitwise
    /// the car built without the cache.
    #[test]
    fn numbers_come_back_from_the_cache_with_the_same_bits() {
        for x in [0.37065600000000004f64, 1.0 / 3.0, 0.1 + 0.2, 5e-324, 1.7976931348623157e308] {
            let back: f64 = serde_json::from_str(&serde_json::to_string(&x).unwrap()).unwrap();
            assert_eq!(back.to_bits(), x.to_bits(), "{x:e}");
        }
    }

    #[test]
    fn every_input_is_in_the_key() {
        let lib = lsim_lib::library();
        let a = battery_drive(&Params::default());
        let b = battery_drive(&Params { ocv: 450.0, j: 7.0, ..Params::default() });
        assert_eq!(input_key(&lib, &a, false, None), input_key(&lib, &a.clone(), false, None));
        // parameter values, the options, the mode and the structure
        assert_ne!(input_key(&lib, &a, false, None), input_key(&lib, &b, false, None));
        assert_ne!(input_key(&lib, &a, false, None), input_key(&lib, &a, true, None));
        let spec = InverseSpec { prescribed: vec!["inertia.w".into()], freed: vec![] };
        assert_ne!(input_key(&lib, &a, false, None), input_key(&lib, &a, false, Some(&spec)));
        let mut c = a.clone();
        c.connections.pop();
        assert_ne!(input_key(&lib, &a, false, None), input_key(&lib, &c, false, None));
        // the library's shared enumeration types (before, not in the key)
        let mut typed = lib.clone();
        typed.add_type(lsim_ir::EnumType {
            name: "Speed".into(),
            literals: vec![],
            doc: String::new(),
        });
        assert_ne!(input_key(&lib, &a, false, None), input_key(&typed, &a, false, None));
    }
}
