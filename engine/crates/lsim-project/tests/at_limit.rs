//! A `when` whose threshold equals a limit the model holds a value at is
//! warned about (step 3): the Formula Student car's 75 m acceleration with a
//! counter of the battery's power above a level, beside the Accumulator's
//! 80 kW Output Power Limit. At 80 kW exactly the run stopped with an event
//! storm at rtol 1e-6 and 1e-8 (at 1e-4 it counted 125 firings instead); a
//! little above or below, the count is right.

use lsim_ir::component::build::connect;
use lsim_ir::component::build::{discrete, eq, var};
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

/// A user's part that holds its input at limits written as expressions of
/// its parameters (`-p_max`, `2*p_half`) and counts how often the held
/// values reach them.
fn clip() -> ComponentDef {
    ComponentDef {
        name: "User.Clip".into(),
        doc: "Holds its input within limits and counts how often it reaches them.".into(),
        ports: vec![input("u", "W", "the held signal")],
        params: vec![
            p("p_max", "W", 50_000.0, "the largest power either way"),
            p("p_half", "W", 30_000.0, "half the largest power"),
        ],
        vars: vec![
            var("y1", "W", "u held within -p_max and p_max"),
            var("y2", "W", "u held below twice p_half"),
            discrete("low", "1", 0.0, "how many times y1 fell below -p_max"),
            discrete("high", "1", 0.0, "how many times y2 rose above twice p_half"),
        ],
        equations: vec![
            eq(n("y1"), limit(n("u"), -n("p_max"), n("p_max")), "y1 within the power limits"),
            eq(n("y2"), min(n("u"), c(2.0) * n("p_half")), "y2 below twice p_half"),
            when(lt(n("y1"), -n("p_max")), &[("low", pre("low") + c(1.0))], "y1 below -p_max"),
            when(
                gt(n("y2"), c(2.0) * n("p_half")),
                &[("high", pre("high") + c(1.0))],
                "y2 above twice p_half",
            ),
        ],
        ..Default::default()
    }
}

/// The warnings of fs-electric's 75 m acceleration with the counter at `level`.
fn warnings(level: f64) -> Vec<Diagnostic> {
    with_part(
        watch(),
        "watch",
        vec![Modifier { param: "level".into(), value: ParamValue::Real(Expr::Const(level)) }],
    )
}

/// The warnings of fs-electric's 75 m acceleration with the user's part
/// `def`, named `name` and labelled after it, reading the battery's power.
fn with_part(def: ComponentDef, name: &str, modifiers: Vec<Modifier>) -> Vec<Diagnostic> {
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
    let (def_name, label) = (def.name.clone(), def.name.trim_start_matches("User.").to_string());
    lib.add(def);
    top.components.push(SubDecl {
        name: name.into(),
        def: def_name,
        modifiers,
        label: Some(label),
        ui_id: None,
    });
    top.connections.push(connect("el_battery.sig_power", &format!("{name}.u")));
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

/// A limit written as an expression of a parameter counts by its value: a
/// threshold at `-p_max` beside `limit(u, -p_max, p_max)`, and one at
/// `2*p_half` beside `min(u, 2*p_half)`, are both warned about, each
/// naming the parameter it reads (fix round 1: only a plain parameter
/// counted, so neither was).
#[test]
fn a_threshold_at_a_negated_or_scaled_limit_is_warned_about() {
    let at: Vec<Diagnostic> = with_part(clip(), "clip", vec![])
        .into_iter()
        .filter(|d| d.code == "WHEN-AT-LIMIT")
        .collect();
    assert_eq!(at.len(), 2, "{at:?}");
    for (d, (value, param)) in at.iter().zip([("-50000", "p_max"), ("60000", "p_half")]) {
        for part in ["'Clip'", value, param] {
            assert!(d.message.contains(part), "{part} not in: {}", d.message);
        }
        assert!(d.detail.iter().any(|x| x.contains(param)), "{:?}", d.detail);
    }
}

/// A `when` on time is never at a limit in watts, though its threshold
/// has the limit's number: beside `min(u, 2*p_half)` (60 000 W), `when
/// time >= p_t` (p_t = 60 000 s) and `when time >= 60000` add no warning
/// to the two above (fix round 2: the unit was checked only where both
/// were plain parameters, so both were warned about).
#[test]
fn a_when_on_time_at_a_limits_number_is_not_warned_about() {
    let mut def = clip();
    def.name = "User.ClipT".into();
    def.params.push(p("p_t", "s", 60_000.0, "a late moment"));
    def.vars.push(discrete("late", "1", 0.0, "how many late moments passed"));
    for (thr, label) in [(n("p_t"), "late"), (c(60_000.0), "late, by number")] {
        def.equations.push(when(ge(time(), thr), &[("late", pre("late") + c(1.0))], label));
    }
    let at: Vec<Diagnostic> =
        with_part(def, "clip", vec![]).into_iter().filter(|d| d.code == "WHEN-AT-LIMIT").collect();
    assert_eq!(at.len(), 2, "{at:?}");
    assert!(at.iter().all(|d| !d.detail.iter().any(|x| x.contains("late"))), "{at:?}");
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
