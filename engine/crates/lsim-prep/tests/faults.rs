//! Work package 2's acceptance: every fault of the structural-diagnostics
//! catalogue (DESIGN.md, *Error messages*) gives its code and names the
//! parts, each with a model that has it.

mod common;

use common::{library, signal};
use lsim_ir::component::build::{connect, eq, param, port, state, sub, var};
use lsim_ir::expr::{CmpOp, Expr, c, cmp, der, name as n};
use lsim_ir::{
    ComponentDef, Diagnostic, Equation, EquationDecl, Library, ParamValue, Severity, SubDecl,
    WhenAction,
};
use lsim_prep::{PrepOptions, prepare};

fn labelled(mut s: SubDecl, label: &str) -> SubDecl {
    s.label = Some(label.into());
    s
}

fn model(comps: Vec<SubDecl>, conns: &[(&str, &str)]) -> ComponentDef {
    ComponentDef {
        name: "Faults.Model".into(),
        components: comps,
        connections: conns.iter().map(|(a, b)| connect(a, b)).collect(),
        ..Default::default()
    }
}

/// The diagnostics of preparing `top` (errors, or a prepared model's
/// warnings).
fn diagnostics_with(lib: &Library, top: &ComponentDef) -> Vec<Diagnostic> {
    match prepare(lib, top, &PrepOptions::default()) {
        Ok(m) => m.warnings,
        Err(d) => d,
    }
}

/// Prepares `top`, finds the diagnostic `code`, checks its parts and
/// severity, prints it and returns it.
fn expect_in(
    lib: &Library,
    top: &ComponentDef,
    code: &str,
    parts: &[&str],
    sev: Severity,
) -> Diagnostic {
    let diags = diagnostics_with(lib, top);
    let d = diags
        .iter()
        .find(|d| d.code == code)
        .unwrap_or_else(|| panic!("no {code} among {diags:#?}"))
        .clone();
    println!("{d}\n    parts {:?}", d.parts);
    assert_eq!(d.parts, parts, "{code}: parts");
    assert_eq!(d.severity, sev, "{code}: severity");
    assert!(!d.message.is_empty() && d.hint.as_ref().is_none_or(|h| !h.is_empty()));
    d
}

fn expect(top: &ComponentDef, code: &str, parts: &[&str]) -> Diagnostic {
    expect_in(&library(), top, code, parts, Severity::Error)
}

fn src(name: &str, v: f64) -> SubDecl {
    sub(name, "Electrical.ConstantVoltage", &[("V", c(v))])
}

fn res(name: &str, r: f64) -> SubDecl {
    sub(name, "Electrical.Resistor", &[("R", c(r))])
}

fn gnd() -> SubDecl {
    sub("gnd", "Electrical.Ground", &[])
}

#[test]
fn two_batteries_wired_in_a_loop_with_no_resistance() {
    let top = model(
        vec![labelled(src("a", 400.0), "Battery A"), labelled(src("b", 390.0), "Battery B"), gnd()],
        &[("a.p", "b.p"), ("a.n", "b.n"), ("a.n", "gnd.p")],
    );
    let d = expect(&top, "ELEC-SOURCE-LOOP", &["a", "b"]);
    assert!(d.message.contains("'Battery A' and 'Battery B'"), "{}", d.message);
}

#[test]
fn a_loop_of_sources_with_zero_resistances_cannot_be_solved() {
    // structurally fine (each source has its resistor), but both are 0 Ω
    let top = model(
        vec![
            labelled(src("a", 400.0), "Battery A"),
            labelled(src("b", 390.0), "Battery B"),
            labelled(res("ra", 0.0), "Cable A"),
            labelled(res("rb", 0.0), "Cable B"),
            gnd(),
        ],
        &[("a.p", "ra.p"), ("ra.n", "rb.n"), ("rb.p", "b.p"), ("a.n", "b.n"), ("a.n", "gnd.p")],
    );
    // the equations left are the cables', which name the zero resistances
    expect(&top, "SINGULAR-LOOP", &["ra", "rb"]);
}

#[test]
fn a_circuit_with_no_ground() {
    let top = model(vec![src("s", 12.0), res("r", 4.0)], &[("s.p", "r.p"), ("r.n", "s.n")]);
    expect(&top, "ELEC-NO-GROUND", &["r", "s"]);
}

#[test]
fn a_circuit_with_no_ground_and_parallel_branches() {
    // the node balances are not aliases here: the structure is balanced
    // and only the numbers show that the potentials float
    let top = model(
        vec![src("s", 12.0), res("r1", 4.0), res("r2", 6.0)],
        &[("s.p", "r1.p"), ("s.p", "r2.p"), ("r1.n", "s.n"), ("r2.n", "s.n")],
    );
    let d = expect(&top, "ELEC-NO-GROUND", &["r1", "r2", "s"]);
    assert!(d.message.contains("no ground"), "{}", d.message);
}

#[test]
fn two_current_sources_in_series() {
    let i = |k: &str, a: f64| sub(k, "Electrical.ConstantCurrent", &[("I", c(a))]);
    let top = model(
        vec![i("i1", 1.0), i("i2", 2.0), gnd()],
        &[("i1.n", "i2.p"), ("i2.n", "i1.p"), ("i1.p", "gnd.p")],
    );
    expect(&top, "ELEC-CURRENT-SOURCES", &["i1", "i2"]);
}

#[test]
fn a_floating_thermal_network() {
    let g = |k: &str| sub(k, "Thermal.Conductor", &[("G", c(10.0))]);
    let top = model(vec![g("g1"), g("g2")], &[("g1.b", "g2.a"), ("g2.b", "g1.a")]);
    expect(&top, "THERM-FLOATING", &["g1", "g2"]);
}

#[test]
fn two_fixed_temperatures_on_one_node() {
    let t = |k: &str, v: f64| sub(k, "Thermal.FixedTemperature", &[("T", c(v))]);
    let top = model(
        vec![t("t1", 300.0), t("t2", 310.0), sub("g", "Thermal.Conductor", &[])],
        &[("t1.port", "t2.port"), ("t1.port", "g.a"), ("g.b", "t2.port")],
    );
    expect(&top, "THERM-TEMP-CONFLICT", &["t1", "t2"]);
}

#[test]
fn two_speed_sources_on_one_rigid_shaft() {
    let w = |k: &str, v: f64| sub(k, "Rotational.ConstantSpeed", &[("w", c(v))]);
    let top = model(
        vec![
            labelled(w("m1", 100.0), "Front motor"),
            labelled(w("m2", 120.0), "Rear motor"),
            sub("shaft", "Rotational.Inertia", &[]),
        ],
        &[("m1.flange", "shaft.a"), ("m2.flange", "shaft.b")],
    );
    let d = expect(&top, "MECH-SPEED-CONFLICT", &["m1", "m2"]);
    assert!(d.message.contains("'Front motor' and 'Rear motor'"), "{}", d.message);
}

#[test]
fn a_shaft_with_nothing_to_drive_or_hold_it() {
    // a spring-damper between two dampers' flanges is held; between two
    // gears' free ends nothing gives the shaft a speed
    let top = model(
        vec![
            sub("sd", "Rotational.SpringDamper", &[("k", c(100.0)), ("d", c(1.0))]),
            sub("g1", "Rotational.IdealGear", &[("ratio", c(2.0))]),
            sub("g2", "Rotational.IdealGear", &[("ratio", c(3.0))]),
        ],
        &[("sd.a", "g1.b"), ("sd.b", "g2.a")],
    );
    expect(&top, "MECH-FLOATING", &["g1", "g2", "sd"]);
}

#[test]
fn a_part_not_connected_at_all() {
    let top = model(
        vec![src("s", 12.0), res("r", 4.0), labelled(res("spare", 1.0), "Spare resistor"), gnd()],
        &[("s.p", "r.p"), ("r.n", "s.n"), ("s.n", "gnd.p")],
    );
    let d = expect(&top, "PART-UNCONNECTED", &["spare"]);
    assert!(d.message.contains("'Spare resistor' is not connected"), "{}", d.message);
}

#[test]
fn a_gearbox_with_no_ratio_input() {
    let top = model(
        vec![
            sub("t", "Rotational.ConstantTorque", &[("tau", c(10.0))]),
            sub("j1", "Rotational.Inertia", &[]),
            labelled(sub("gb", "Rotational.Gearbox", &[]), "Gearbox"),
            sub("j2", "Rotational.Inertia", &[]),
        ],
        &[("t.flange", "j1.a"), ("j1.b", "gb.a"), ("gb.b", "j2.a")],
    );
    let d = expect(&top, "GEAR-NO-RATIO", &["gb"]);
    assert!(d.message.contains("'Gearbox' (Rotational.Gearbox) has no gear"), "{}", d.message);
}

#[test]
fn a_signal_input_left_open() {
    let top = model(vec![labelled(sub("k", "Signal.Gain", &[]), "Gain")], &[]);
    expect(&top, "SIGNAL-UNCONNECTED", &["k"]);
}

#[test]
fn a_signal_driven_twice() {
    let top = model(
        vec![
            sub("c1", "Signal.Constant", &[]),
            sub("c2", "Signal.Constant", &[]),
            sub("k", "Signal.Gain", &[]),
        ],
        &[("c1.y", "k.u"), ("c2.y", "k.u")],
    );
    expect(&top, "SIGNAL-SOURCES", &["c1", "c2", "k"]);
}

#[test]
fn an_algebraic_loop_through_controllers() {
    let top = model(
        vec![
            sub("r", "Signal.Constant", &[]),
            labelled(sub("sum", "Signal.Add", &[]), "Error"),
            labelled(sub("k", "Signal.Gain", &[("k", c(2.0))]), "Controller"),
            sub("neg", "Signal.Gain", &[("k", c(-1.0))]),
        ],
        &[("r.y", "sum.u1"), ("sum.y", "k.u"), ("k.y", "neg.u"), ("neg.y", "sum.u2")],
    );
    expect_in(&library(), &top, "CAUSAL-LOOP", &["k", "neg", "sum"], Severity::Warning);
}

/// A component with its own variables and equations, for the faults that
/// need equations of a particular shape.
fn custom(name: &str, vars: Vec<lsim_ir::VarDecl>, eqs: Vec<EquationDecl>) -> ComponentDef {
    ComponentDef { name: name.into(), vars, equations: eqs, ..Default::default() }
}

fn with(lib_extra: Vec<ComponentDef>) -> Library {
    let mut lib = library();
    for d in lib_extra {
        lib.add(d);
    }
    lib
}

#[test]
fn an_equation_too_many_and_a_variable_too_few() {
    let over = custom(
        "Faults.Over",
        vec![var("x", "1", "")],
        vec![eq(n("x"), c(1.0), "x is one"), eq(n("x"), c(2.0), "x is two")],
    );
    let under = custom(
        "Faults.Under",
        vec![var("y", "1", ""), var("z", "1", "")],
        vec![eq(n("y"), n("z"), "y follows z")],
    );
    let lib = with(vec![over, under]);
    let top = model(vec![labelled(sub("o", "Faults.Over", &[]), "Twice")], &[]);
    expect_in(&lib, &top, "STRUCT-OVER", &["o"], Severity::Error);
    let top = model(vec![labelled(sub("u", "Faults.Under", &[]), "Loose")], &[]);
    expect_in(&lib, &top, "STRUCT-UNDER", &["u"], Severity::Error);
}

#[test]
fn initial_equations_that_contradict_the_model() {
    let mut top = model(
        vec![src("s", 12.0), res("r", 4.0), gnd()],
        &[("s.p", "r.p"), ("r.n", "s.n"), ("s.n", "gnd.p")],
    );
    top.initial_equations.push(eq(n("r.i"), c(1.0), "the current starts at 1 A"));
    expect(&top, "INIT-OVER", &["r", "s"]);
}

#[test]
fn parameters_given_by_each_other() {
    let mut d = custom(
        "Faults.Cycle",
        vec![state("x", "1", 0.0, "")],
        vec![eq(der("x"), n("a") / n("tau"), "x grows")],
    );
    d.params = vec![param("a", "1", 1.0, ""), param("b", "1", 1.0, ""), param("tau", "s", 1.0, "")];
    d.params[0].default = ParamValue::Real(n("b"));
    d.params[1].default = ParamValue::Real(n("a") * c(2.0));
    let lib = with(vec![d]);
    let top = model(vec![sub("p", "Faults.Cycle", &[])], &[]);
    expect_in(&lib, &top, "PARAM-CYCLE", &["p"], Severity::Error);
}

#[test]
fn a_sampled_block_without_a_period() {
    let ext = ComponentDef {
        name: "External.Script".into(),
        ports: vec![signal("u", false, "1"), signal("y", true, "1")],
        ..Default::default()
    };
    let lib = with(vec![ext]);
    let top = model(
        vec![sub("c", "Signal.Constant", &[]), labelled(sub("hcu", "External.Script", &[]), "HCU")],
        &[("c.y", "hcu.u")],
    );
    expect_in(&lib, &top, "EXTERNAL-PERIOD", &["hcu"], Severity::Error);
}

#[test]
fn sampled_blocks_feeding_each_other() {
    let ext = ComponentDef {
        name: "External.Script".into(),
        ports: vec![signal("u", false, "1"), signal("y", true, "1")],
        params: vec![param("period", "s", 0.01, "")],
        ..Default::default()
    };
    let lib = with(vec![ext]);
    let top = model(
        vec![sub("a", "External.Script", &[]), sub("b", "External.Script", &[])],
        &[("a.y", "b.u"), ("b.y", "a.u")],
    );
    expect_in(&lib, &top, "EXTERNAL-LOOP", &["a", "b"], Severity::Warning);
}

#[test]
fn a_when_condition_of_two_comparisons() {
    let mut d = custom(
        "Faults.When",
        vec![state("x", "1", 0.0, ""), lsim_ir::component::build::discrete("hit", "1", 0.0, "")],
        vec![
            eq(der("x"), n("rate"), "x grows"),
            EquationDecl {
                eq: Equation::When {
                    condition: Expr::And(
                        Box::new(cmp(CmpOp::Gt, n("x"), c(1.0))),
                        Box::new(cmp(CmpOp::Lt, n("x"), c(2.0))),
                    ),
                    actions: vec![WhenAction::Assign { var: "hit".into(), value: c(1.0) }],
                },
                label: Some("it notes the window".into()),
            },
        ],
    );
    d.params = vec![param("rate", "1/s", 1.0, "")];
    let lib = with(vec![d]);
    let top = model(vec![sub("w", "Faults.When", &[])], &[]);
    expect_in(&lib, &top, "WHEN-CONDITION", &["w"], Severity::Error);
}

#[test]
fn an_event_that_sets_a_continuous_variable() {
    let mut d = custom(
        "Faults.WhenCont",
        vec![state("x", "1", 0.0, ""), var("y", "1", "")],
        vec![
            eq(der("x"), n("rate"), "x grows"),
            eq(n("y"), n("x"), "y follows"),
            EquationDecl {
                eq: Equation::When {
                    condition: cmp(CmpOp::Gt, n("x"), c(1.0)),
                    actions: vec![WhenAction::Assign { var: "y".into(), value: c(0.0) }],
                },
                label: None,
            },
        ],
    );
    d.params = vec![param("rate", "1/s", 1.0, "")];
    let lib = with(vec![d]);
    let top = model(vec![sub("w", "Faults.WhenCont", &[])], &[]);
    expect_in(&lib, &top, "WHEN-CONTINUOUS", &["w"], Severity::Error);
}

#[test]
fn a_rate_of_something_that_does_not_change_continuously() {
    let mut top = model(
        vec![src("s", 12.0), res("r", 4.0), gnd()],
        &[("s.p", "r.p"), ("r.n", "s.n"), ("s.n", "gnd.p")],
    );
    top.initial_equations.push(eq(der("r.i"), c(0.0), "the current starts steady"));
    // r.i is the source's current too: both parts are named
    let d = expect(&top, "DER-NOT-STATE", &["r", "s"]);
    assert!(d.message.contains("i of 'r'"), "{}", d.message);
}

#[test]
fn a_division_by_a_quantity_that_starts_at_zero() {
    // a constant-power load on a capacitor that starts empty: i = P / v
    let top = model(
        vec![
            sub("cap", "Electrical.Capacitor", &[("C", c(1.0))]),
            labelled(sub("load", "Electrical.PowerLoad", &[("P", c(10.0))]), "Heater"),
            gnd(),
        ],
        &[("cap.p", "load.p"), ("cap.n", "load.n"), ("cap.n", "gnd.p")],
    );
    expect_in(&library(), &top, "PIVOT-ZERO-AT-START", &["load"], Severity::Warning);
}

#[test]
fn a_constraint_through_a_table_cannot_be_differentiated() {
    let mut tv = ComponentDef {
        name: "Faults.TableVoltage".into(),
        ports: vec![port("p", "Pin", ""), port("n", "Pin", "")],
        vars: vec![var("v", "V", ""), var("i", "A", "")],
        equations: vec![
            eq(n("v"), n("p.v") - n("n.v"), ""),
            eq(c(0.0), n("p.i") + n("n.i"), ""),
            eq(n("i"), n("p.i"), ""),
            eq(n("v"), lsim_ir::expr::table("profile", vec![Expr::Time]), "it follows its profile"),
        ],
        ..Default::default()
    };
    tv.params = vec![lsim_ir::ParamDecl {
        name: "profile".into(),
        unit: "V".into(),
        display_unit: None,
        default: ParamValue::Table1D {
            x: vec![0.0, 1.0],
            y: vec![0.0, 10.0],
            axis_unit: "s".into(),
        },
        min: None,
        max: None,
        structural: false,
        doc: String::new(),
    }];
    let lib = with(vec![tv]);
    let top = model(
        vec![
            labelled(sub("src", "Faults.TableVoltage", &[]), "Profile source"),
            sub("cap", "Electrical.Capacitor", &[("C", c(1e-3))]),
            res("r", 10.0),
            sub("c2", "Electrical.Capacitor", &[("C", c(1e-3))]),
            gnd(),
        ],
        &[
            ("src.p", "cap.p"),
            ("src.n", "cap.n"),
            ("src.p", "r.p"),
            ("r.n", "c2.p"),
            ("c2.n", "src.n"),
            ("src.n", "gnd.p"),
        ],
    );
    expect_in(&lib, &top, "INDEX-DIFFERENTIATE", &["src"], Severity::Error);
}

#[test]
fn constraints_that_contradict_each_other() {
    // x follows the clock, and is also held still: with each variable's
    // derivatives counted as the variable (the check before index
    // reduction) there are two equations for x, so the fault is found
    // before differentiation could go on forever (INDEX-TOO-HIGH stays a
    // safety net: Pantelides ends on any system that passes this check)
    let d = custom(
        "Faults.Contradiction",
        vec![state("x", "s", 0.0, "")],
        vec![eq(n("x"), Expr::Time, "x follows the clock"), eq(der("x"), c(0.0), "x is held")],
    );
    let lib = with(vec![d]);
    let top = model(vec![labelled(sub("k", "Faults.Contradiction", &[]), "Clock")], &[]);
    expect_in(&lib, &top, "STRUCT-OVER", &["k"], Severity::Error);
}

#[test]
fn a_constraint_that_is_singular_at_the_start() {
    // a point held at the origin by x² + y² = 0: the constraint's gradient
    // vanishes there, so no choice of states works
    use common::guess;
    let d = ComponentDef {
        name: "Faults.Point".into(),
        params: vec![param("m", "kg", 1.0, ""), param("g", "m/s2", 9.81, "")],
        vars: vec![
            state("x", "m", 0.0, ""),
            guess("y", "m", c(0.0)),
            state("vx", "m/s", 0.0, ""),
            guess("vy", "m/s", c(0.0)),
            guess("F", "N/m", c(0.0)),
        ],
        equations: vec![
            eq(der("x"), n("vx"), ""),
            eq(der("y"), n("vy"), ""),
            eq(n("m") * der("vx"), -(n("x") * n("F")), ""),
            eq(n("m") * der("vy"), -(n("y") * n("F")) - n("m") * n("g"), ""),
            eq(n("x") * n("x") + n("y") * n("y"), c(0.0), "it stays at the origin"),
        ],
        ..Default::default()
    };
    let lib = with(vec![d]);
    let top = model(vec![labelled(sub("pt", "Faults.Point", &[]), "Point")], &[]);
    expect_in(&lib, &top, "STATE-SELECT-SINGULAR", &["pt"], Severity::Error);
}

#[test]
fn a_restart_of_something_that_is_not_a_state() {
    let mut d = custom(
        "Faults.Restart",
        vec![state("x", "1", 0.0, ""), var("y", "1", "")],
        vec![
            eq(der("x"), n("rate"), "x grows"),
            eq(n("y"), c(2.0) * n("x"), "y is twice x"),
            EquationDecl {
                eq: Equation::When {
                    condition: cmp(CmpOp::Gt, n("x"), c(1.0)),
                    actions: vec![WhenAction::Reinit { var: "y".into(), value: c(0.0) }],
                },
                label: Some("y restarts".into()),
            },
        ],
    );
    d.params = vec![param("rate", "1/s", 1.0, "")];
    let lib = with(vec![d]);
    let top = model(vec![labelled(sub("r", "Faults.Restart", &[]), "Counter")], &[]);
    let d = expect_in(&lib, &top, "REINIT-NOT-STATE", &["r"], Severity::Error);
    assert!(d.message.contains("y of 'Counter'"), "{}", d.message);
}

#[test]
fn restarts_of_two_rigidly_coupled_speeds() {
    // two inertias on one shaft have one state between them: restarting
    // both speeds cannot keep both
    let kick = custom(
        "Faults.Kick",
        vec![],
        vec![EquationDecl {
            eq: Equation::When {
                condition: cmp(CmpOp::Gt, Expr::Time, c(1.0)),
                actions: vec![
                    WhenAction::Reinit { var: "a.w".into(), value: c(1.0) },
                    WhenAction::Reinit { var: "b.w".into(), value: c(2.0) },
                ],
            },
            label: Some("both speeds restart".into()),
        }],
    );
    let mut kick = kick;
    kick.components = vec![
        sub("a", "Rotational.Inertia", &[("J", c(1.0))]),
        sub("b", "Rotational.Inertia", &[("J", c(2.0))]),
    ];
    kick.connections = vec![connect("a.b", "b.a")];
    let lib = with(vec![kick]);
    let top = model(vec![labelled(sub("k", "Faults.Kick", &[]), "Shaft")], &[]);
    expect_in(&lib, &top, "REINIT-NOT-STATE", &["k"], Severity::Error);
}
