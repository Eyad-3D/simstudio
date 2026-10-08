//! The on-disk cache of prepared models.
//!
//! Key: SHA-256 of the model's inputs — the top component, every library
//! definition it reaches, the connectors, the preparation options, the mode
//! (full dynamic, or fast mode with its [`InverseSpec`]) and the engine
//! version — **without the values of runtime parameters**: a top-level
//! part's numeric parameter value (a modifier) and the top component's own
//! numeric parameter defaults are left out of the key unless the parameter
//! is structural. So a parameter study, or the same car with another
//! battery capacity, shares one entry. Value: the [`PreparedModel`] as JSON.
//!
//! A hit skips everything after flattening: the model is flattened again
//! (cheap, linear in its size) only to read the parameters' values and the
//! start values that follow from them, which [`refresh`] copies into the
//! cached model before it is compiled. Machine code is regenerated
//! (milliseconds).

use lsim_ir::component::{ComponentDef, Library, ParamValue};
use lsim_ir::expr::Expr;
use lsim_ir::flat::FlatSystem;
use lsim_ir::{InverseSpec, PreparedModel};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

/// Bumped when what is cached, or how it is keyed, changes.
const FORMAT: &str = "prepared-v2";

/// The top component with its runtime parameter values left out.
pub fn masked_top(lib: &Library, top: &ComponentDef) -> ComponentDef {
    let runtime = Expr::Name("<runtime value>".into());
    let mut t = top.clone();
    for s in &mut t.components {
        let Some(def) = lib.components.get(&s.def) else { continue };
        for m in &mut s.modifiers {
            let structural =
                def.params.iter().find(|p| p.name == m.param).is_none_or(|p| p.structural);
            if !structural && matches!(m.value, ParamValue::Real(Expr::Const(_))) {
                m.value = ParamValue::Real(runtime.clone());
            }
        }
    }
    for p in &mut t.params {
        if !p.structural && matches!(p.default, ParamValue::Real(Expr::Const(_))) {
            p.default = ParamValue::Real(runtime.clone());
        }
    }
    t
}

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
        masked_top(lib, top),
        used,
        &lib.connectors,
        force_implicit,
        inverse,
    ))
    .expect("the IR serialises");
    Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// Copies the parameter values and start values of a fresh flattening of
/// the same model (same key, other runtime values) into a cached prepared
/// model, by name.
pub fn refresh(m: &mut PreparedModel, fresh: &FlatSystem) {
    let params: HashMap<&str, f64> =
        fresh.params.iter().map(|p| (p.name.as_str(), p.value)).collect();
    for p in &mut m.flat.params {
        if let Some(v) = params.get(p.name.as_str()) {
            p.value = *v;
        }
    }
    let starts: HashMap<&str, Option<f64>> =
        fresh.vars.iter().map(|v| (v.name.as_str(), v.start)).collect();
    for v in &mut m.flat.vars {
        if let Some(s) = starts.get(v.name.as_str()) {
            v.start = *s;
        }
    }
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

    #[test]
    fn runtime_values_are_not_in_the_key() {
        let lib = lsim_lib::library();
        let a = battery_drive(&Params::default());
        let b = battery_drive(&Params { ocv: 450.0, j: 7.0, ..Params::default() });
        assert_eq!(input_key(&lib, &a, false, None), input_key(&lib, &b, false, None));
        // the options, the mode and the structure are
        assert_ne!(input_key(&lib, &a, false, None), input_key(&lib, &a, true, None));
        let spec = InverseSpec { prescribed: vec!["inertia.w".into()], freed: vec![] };
        assert_ne!(input_key(&lib, &a, false, None), input_key(&lib, &a, false, Some(&spec)));
        let mut c = a.clone();
        c.connections.pop();
        assert_ne!(input_key(&lib, &a, false, None), input_key(&lib, &c, false, None));
        // a structural parameter's value is
        let mut lib2 = lib.clone();
        let mut res = lib2.components["Rotational.Inertia"].clone();
        res.params[0].structural = true;
        lib2.add(res);
        assert_ne!(input_key(&lib2, &a, false, None), input_key(&lib2, &b, false, None));
    }
}
