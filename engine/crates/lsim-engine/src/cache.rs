//! The on-disk cache of prepared models.
//!
//! Key: SHA-256 of the model's inputs — the top component, every library
//! definition it reaches, the connectors, the preparation options and the
//! engine version. Value: the [`PreparedModel`] as JSON. A hit skips
//! flattening and structural analysis; the machine code is regenerated
//! (milliseconds, measured by the spike). Stage 1 keys on every input,
//! runtime parameter values included; work package 6 drops those from the
//! key and re-applies them on a hit, so a parameter sweep shares one entry.

use lsim_ir::{ComponentDef, Library, PreparedModel};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The cache key of a model's inputs.
pub fn input_key(lib: &Library, top: &ComponentDef, force_implicit: bool) -> String {
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
        env!("CARGO_PKG_VERSION"),
        top,
        used,
        &lib.connectors,
        force_implicit,
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
