//! A `when` whose threshold equals a limit the model holds a value at is
//! warned about (step 3): the Formula Student car's 75 m acceleration with a
//! counter of the battery's power above a level, beside the Accumulator's
//! 80 kW Output Power Limit. At 80 kW exactly every build stopped with an
//! event storm; a little above or below, the count is right.

use lsim_ir::component::build::connect;
use lsim_ir::component::build::discrete;
use lsim_ir::expr::Expr;
use lsim_ir::{ComponentDef, Diagnostic, Modifier, ParamValue, SubDecl};
use lsim_lib::x::*;
use lsim_project::{ImportOptions, import_case, standard_registry};
use std::path::PathBuf;

/// The user's part: counts how often u rises above `level`.
fn watch() -> ComponentDef {
    ComponentDef {
        name: "User.Watch".into(),
        doc: "Counts how often its input rises above a level.".into(),
        ports: vec![input("u", "W", "the watched signal")],
        params: vec![p("level", "W", 0.0, "the level")],
        vars: vec![discrete("hits", "1", 0.0, "how many times u rose above the level")],
        equations: vec![when(
            gt(n("u"), n("level")),
            &[("hits", pre("hits") + c(1.0))],
            "the watched signal rises above the level",
        )],
        ..Default::default()
    }
}

/// The warnings of fs-electric's 75 m acceleration with the counter at `level`.
fn warnings(level: f64) -> Vec<Diagnostic> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../backend/projects");
    let project: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("fs-electric.json")).unwrap())
            .unwrap();
    let (mut top, rep) = import_case(
        &project,
        Some("case-accel-75m"),
        &standard_registry(),
        &ImportOptions::default(),
    )
    .expect("the case imports");
    let mut lib = rep.library();
    lib.add(watch());
    top.components.push(SubDecl {
        name: "watch".into(),
        def: "User.Watch".into(),
        modifiers: vec![Modifier {
            param: "level".into(),
            value: ParamValue::Real(Expr::Const(level)),
        }],
        label: Some("Watch".into()),
        ui_id: None,
    });
    top.connections.push(connect("el_battery.sig_power", "watch.u"));
    let m = lsim_prep::prepare(&lib, &top, &Default::default()).expect("the model prepares");
    m.warnings
}

#[test]
fn a_threshold_at_the_accumulators_output_power_limit_is_warned_about() {
    let at: Vec<Diagnostic> =
        warnings(80_000.0).into_iter().filter(|d| d.code == "WHEN-AT-LIMIT").collect();
    assert_eq!(at.len(), 1, "{at:?}");
    let d = &at[0];
    assert_eq!(d.severity, lsim_ir::Severity::Warning);
    for part in ["'Watch'", "'Accumulator'", "Output Power Limit", "80000 W"] {
        assert!(d.message.contains(part), "{part} not in: {}", d.message);
    }
    assert!(
        d.parts.contains(&"watch".to_string()) && d.parts.contains(&"el_battery".to_string()),
        "{:?}",
        d.parts
    );
    for level in [79_999.0, 80_001.0, 70_000.0] {
        let w: Vec<Diagnostic> =
            warnings(level).into_iter().filter(|d| d.code == "WHEN-AT-LIMIT").collect();
        assert!(w.is_empty(), "{level}: {w:?}");
    }
}

/// No example project's case, as it comes, is warned about: the library's
/// own `when`s (a vehicle's break-away at 0.5 m/s beside a wheel's 0.5 m/s
/// slip floor, a `max` in a denominator) are not thresholds at a held value.
#[test]
fn no_example_case_has_a_threshold_at_a_limit() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../backend/projects");
    let reg = standard_registry();
    let mut seen = 0;
    let mut bad = vec![];
    for entry in std::fs::read_dir(&dir).expect("the example projects") {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|x| x != "json") {
            continue;
        }
        let project: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        for case in project["cases"].as_array().into_iter().flatten() {
            let id = case["id"].as_str().unwrap();
            let Ok((top, rep)) = import_case(&project, Some(id), &reg, &ImportOptions::default())
            else {
                continue;
            };
            let Ok(m) = lsim_prep::prepare(&rep.library(), &top, &Default::default()) else {
                continue;
            };
            seen += 1;
            for d in m.warnings.iter().filter(|d| d.code == "WHEN-AT-LIMIT") {
                bad.push(format!("{} {id}: {}", path.display(), d.message));
            }
        }
    }
    assert!(seen >= 10, "only {seen} cases prepared");
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}
