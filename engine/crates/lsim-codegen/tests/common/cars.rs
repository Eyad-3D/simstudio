//! The example projects' time-domain cases (`backend/projects/*.json`),
//! imported with lsim-project and prepared with lsim-prep: the models the
//! code generator's budget is about.
#![allow(dead_code)]

use lsim_ir::PreparedModel;
use lsim_project::{ImportOptions, import_case, standard_registry};
use std::path::PathBuf;

/// One case of an example project, prepared.
pub struct Car {
    /// `project/case`
    pub name: String,
    /// the prepared model (alias start values carried, as `Model::build` does)
    pub model: PreparedModel,
    /// whether it has sampled blocks (Script blocks …)
    pub sampled: bool,
}

/// The repository's example projects, by name.
pub fn projects() -> Vec<(String, serde_json::Value)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../backend/projects");
    let mut v: Vec<(String, serde_json::Value)> = std::fs::read_dir(&dir)
        .expect("the example projects")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .map(|p| {
            let text = std::fs::read_to_string(&p).expect("readable");
            (
                p.file_stem().unwrap().to_string_lossy().to_string(),
                serde_json::from_str(&text).expect("JSON"),
            )
        })
        .collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

/// Every time-domain case of every example project, prepared; `filter`
/// keeps the names containing it.
pub fn cars(filter: &str) -> Vec<Car> {
    let reg = standard_registry();
    let mut out = vec![];
    for (name, project) in projects() {
        for case in project["cases"].as_array().into_iter().flatten() {
            let id = case["id"].as_str().unwrap();
            let tag = format!("{name}/{id}");
            if !tag.contains(filter) {
                continue;
            }
            let Ok((top, rep)) = import_case(&project, Some(id), &reg, &ImportOptions::default())
            else {
                continue; // lap and performance cases are not time simulations here
            };
            let lib = rep.library();
            let mut model = lsim_prep::prepare(&lib, &top, &Default::default())
                .unwrap_or_else(|e| panic!("{tag} prepares: {e:?}"));
            lsim_project::model::carry_alias_starts(&mut model);
            out.push(Car { name: tag, model, sampled: !rep.sampled.is_empty() });
        }
    }
    out
}
