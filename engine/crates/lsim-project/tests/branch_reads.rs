//! A table read in a branch of its part's equations that the part does not
//! use at that moment (an E-Motor's drag while it is powered, an Engine's
//! fuel map while it is not fired) does not stop a run on an axis set to
//! Error: today's engine reads a table only where it uses it. Where the
//! branch is taken, the stop stands.

use lsim_project::golden::run_case;
use lsim_solve::SolverOptions;
use serde_json::{Value, json};
use std::path::PathBuf;

fn example(name: &str) -> Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../../backend/projects/{name}.json"));
    serde_json::from_str(&std::fs::read_to_string(p).expect("the example")).expect("JSON")
}

/// The part `el` of `p`, its table `key` changed by `cut` and its axes'
/// out-of-range settings `outside`.
fn cut_table(p: &mut Value, el: &str, key: &str, outside: Value, cut: impl Fn(&Value) -> Value) {
    for s in p["systems"].as_array_mut().unwrap() {
        for e in s["elements"].as_array_mut().unwrap() {
            if e["id"] == el {
                let t = cut(&e["parameterOverrides"][key]);
                e["parameterOverrides"][key] = t;
                e["tableOutside"] = json!({ key: outside });
            }
        }
    }
}

fn keys(t: &Value) -> Vec<(f64, String)> {
    let mut k: Vec<(f64, String)> =
        t.as_object().unwrap().keys().map(|k| (k.parse().unwrap(), k.clone())).collect();
    k.sort_by(|a, b| a.0.total_cmp(&b.0));
    k
}

fn run(p: &Value, case: &str) -> Result<f64, String> {
    let opts = SolverOptions { rtol: 1e-6, atol: 1e-8, ..Default::default() };
    run_case(p, case, &opts, None).map(|r| *r.result.times.last().unwrap())
}

#[test]
fn a_motor_s_drag_table_set_to_error_stops_no_run_while_the_motor_is_powered() {
    // the BEV's drag table cut to its lower half (0 to 4000 1/min) on Error:
    // the motor turns faster while it drives (powered: its drag not used),
    // and today's engine finishes the city case; the new engine stopped
    // (RUN-TABLE) the first time the powered motor passed 4000 1/min
    let mut p = example("bev-car");
    cut_table(&mut p, "el-motor", "drag_torque", json!(["error"]), |t| {
        let k = keys(t);
        let half = k.len() / 2;
        json!(
            k[..k.len() - half]
                .iter()
                .map(|(_, s)| (s.clone(), t[s].clone()))
                .collect::<serde_json::Map<_, _>>()
        )
    });
    assert_eq!(run(&p, "case-city"), Ok(600.0));
}

#[test]
fn an_engine_s_fuel_map_set_to_error_stops_no_run_while_the_engine_is_not_fired() {
    // the hybrid's fuel map without its 0 N·m torque points, on Error: the
    // engine at rest is not fired (its fuel map not used), and today's
    // engine finishes the mixed cycle; the new engine stopped at t = 0
    // (its read at 0 N·m, the idle governor's branch)
    let mut p = example("hybrid-car");
    cut_table(&mut p, "el-engine", "fuel_map", json!(["error", "error"]), |t| {
        let lowest = keys(&t.as_object().unwrap().values().next().unwrap().clone())[0].1.clone();
        let mut out = t.clone();
        for row in out.as_object_mut().unwrap().values_mut() {
            row.as_object_mut().unwrap().remove(&lowest);
        }
        out
    });
    let end = run(&p, "case-mixed").unwrap_or_else(|e| panic!("{e}"));
    assert!(end > 0.0, "{end}");
}

mod common;

/// x = t; y = if x > `on_at` then f(x) else 0, f's data on 0..1 with its
/// axis on Error.
fn branch_model() -> (lsim_ir::Library, lsim_ir::ComponentDef) {
    use lsim_ir::table::{Outside, TableData};
    use lsim_lib::x::*;
    let mut data = TableData::new_1d(vec![0.0, 1.0], vec![0.0, 1.0]);
    data.outside = [Outside::Error, Outside::Error];
    data.axis_units = ["1".into(), "1".into()];
    let def = lsim_ir::ComponentDef {
        name: "Test.Branch".into(),
        doc: "a table read in one branch".into(),
        params: vec![
            p("on_at", "1", 0.0, "where the branch is taken from"),
            lsim_lib::table::table_param("f", "1", data, "the table"),
        ],
        vars: vec![state("x", "1", 0.0, "the abscissa"), var("y", "1", "the branch's value")],
        equations: vec![
            eq(der("x"), c(1.0), "x = t"),
            eq(
                n("y"),
                ite(gt(n("x"), n("on_at")), lsim_ir::expr::table("f", vec![n("x")]), c(0.0)),
                "the table, from on_at on",
            ),
        ],
        ..Default::default()
    };
    let mut lib = common::lib();
    lib.add(def);
    (lib, common::model("M", vec![common::part("b", "Test.Branch", &[])], &[]))
}

fn branch_run(on_at: f64, t_end: f64) -> Result<f64, String> {
    use lsim_ir::{Modifier, ParamValue};
    let (lib, mut top) = branch_model();
    top.components[0].modifiers = vec![Modifier {
        param: "on_at".into(),
        value: ParamValue::Real(lsim_ir::expr::Expr::Const(on_at)),
    }];
    let built = lsim_project::model::Model::build(&lib, &top).unwrap_or_else(|e| {
        panic!("{}", e.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n"))
    });
    let opts = SolverOptions { rtol: 1e-8, atol: 1e-10, ..Default::default() };
    built
        .run(&opts, lsim_solve::OutputGrid { t0: 0.0, t_end, dt: 0.5 }, &mut [])
        .map(|r| *r.times.last().unwrap())
        .map_err(|e| e.to_string())
}

#[test]
fn an_error_axis_read_past_its_data_stops_the_run_only_where_its_branch_is_taken() {
    // x leaves the data at t = 1, in the branch not taken: no stop; the
    // branch is taken at t = 2, with x past the data: the run stops there
    // (before: it stopped at t = 1)
    let e = branch_run(2.0, 5.0).unwrap_err();
    assert!(e.contains("t = 2") && e.contains("outside its data"), "{e}");
    // never taken before the end: the run finishes
    assert_eq!(branch_run(10.0, 5.0), Ok(5.0));
    // taken from the start: it stops where x leaves the data
    let e = branch_run(-1.0, 5.0).unwrap_err();
    assert!(e.contains("t = 1") && e.contains("outside its data"), "{e}");
}
