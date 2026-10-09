//! Every example project imports and builds for each of its time-domain
//! cases, and runs its first seconds.

use lsim_project::model::Model;
use lsim_project::{ImportOptions, import_case, standard_registry};
use lsim_solve::{OutputGrid, SolverOptions};
use std::path::PathBuf;

fn projects() -> Vec<(String, serde_json::Value)> {
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

#[test]
fn every_example_imports_builds_and_starts() {
    let reg = standard_registry();
    let mut faults = vec![];
    let mut built = 0;
    for (name, project) in projects() {
        for case in project["cases"].as_array().into_iter().flatten() {
            let id = case["id"].as_str().unwrap();
            if case["kind"] == "lap" {
                // lap cases stay in today's lap solver
                let e =
                    import_case(&project, Some(id), &reg, &ImportOptions::default()).unwrap_err();
                assert_eq!(e[0].code, "LAP-CASE");
                continue;
            }
            let (top, rep) = match import_case(&project, Some(id), &reg, &ImportOptions::default())
            {
                Ok(x) => x,
                Err(e) => {
                    faults.push(format!("{name}/{id}: import: {e:?}"));
                    continue;
                }
            };
            let lib = rep.library();
            let model = match Model::build(&lib, &top) {
                Ok(m) => m,
                Err(e) => {
                    faults.push(format!(
                        "{name}/{id}: build: {}",
                        e.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n  ")
                    ));
                    continue;
                }
            };
            built += 1;
            let p = &model.prepared;
            println!(
                "{name}/{id}: {} parts, {} states, {} iteration variables, {} modes, {} whens, {} sampled",
                top.components.len(),
                p.states.len(),
                p.algebraics.len(),
                p.modes.len(),
                p.whens.len(),
                rep.sampled.len()
            );
            if !rep.sampled.is_empty() {
                continue; // runs with its sampled blocks' host (the golden harness)
            }
            let opts = SolverOptions { rtol: 1e-6, atol: 1e-8, ..Default::default() };
            if let Err(e) = model.run(&opts, OutputGrid { t0: 0.0, t_end: 5.0, dt: 1.0 }, &mut []) {
                faults.push(format!("{name}/{id}: run: {e}"));
            }
        }
    }
    assert!(faults.is_empty(), "{}", faults.join("\n"));
    assert!(built >= 13, "{built} cases built");
}
