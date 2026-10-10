//! The example projects' time-domain cases (`backend/projects/*.json`),
//! imported with lsim-project and prepared with lsim-prep: the models the
//! code generator's budget is about.
#![allow(dead_code)]

use lsim_codegen::JitModel;
use lsim_ir::{PreparedModel, Slot, VarId};
use lsim_prep::PrepOptions;
use lsim_project::{ImportOptions, import_case, standard_registry};
use lsim_solve::{OutputGrid, RunInfo, SolverOptions};
use std::path::PathBuf;

/// One case of an example project, prepared.
pub struct Car {
    /// `project/case`
    pub name: String,
    /// the prepared model (as `lsim_project::model::Model::build` prepares it)
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
    cars_with(filter, &PrepOptions::default())
}

/// [`cars`], prepared with `opts` (every block implicit, say).
pub fn cars_with(filter: &str, opts: &PrepOptions) -> Vec<Car> {
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
            let model = lsim_prep::prepare(&lib, &top, opts)
                .unwrap_or_else(|e| panic!("{tag} prepares: {e:?}"));
            out.push(Car { name: tag, model, sampled: !rep.sampled.is_empty() });
        }
    }
    out
}

/// One case per project (a project's cases share its model).
pub fn one_per_project(cars: Vec<Car>) -> Vec<Car> {
    let mut seen = std::collections::HashSet::new();
    cars.into_iter()
        .filter(|c| seen.insert(c.name.split('/').next().unwrap().to_string()))
        .collect()
}

/// t, y and d at every output point (every half second) of a simulation
/// of `m` over `t_end` seconds (an iteration variable that is a
/// derivative, which the results do not record, takes its state's value
/// there: a point near the drive rather than on it).
pub fn trajectory(m: &PreparedModel, jit: &JitModel, t_end: f64) -> Vec<(f64, Vec<f64>, Vec<f64>)> {
    let info = RunInfo::from_prepared(m);
    let so = SolverOptions { rtol: 1e-6, atol: 1e-8, ..Default::default() };
    let res =
        lsim_solve::simulate(jit, &info, &so, OutputGrid { t0: 0.0, t_end, dt: 0.5 }, &mut [])
            .expect("runs");
    let slots: Vec<VarId> = m
        .states
        .iter()
        .copied()
        .chain(m.algebraics.iter().map(|s| match s {
            Slot::Var(v) => *v,
            Slot::Der(v) => *v,
        }))
        .collect();
    (0..res.times.len())
        .map(|k| {
            let y = slots.iter().map(|v| res.values[v.0 as usize][k]).collect();
            let d = m.discretes.iter().map(|v| res.values[v.0 as usize][k]).collect();
            (res.times[k], y, d)
        })
        .collect()
}
