//! Start values through alias elimination: a variable kept in place of
//! others takes their start value (a guess) when it has none of its own.

mod common;

use common::*;
use lsim_ir::component::build::{connect, eq, param, port, sub, var};
use lsim_ir::expr::{c, name as n};
use lsim_ir::prepared::AliasTarget;
use lsim_ir::{ComponentDef, Diagnostic};
use lsim_prep::{Settings, prepare_with_report};

/// A load that draws a constant power P: v·i = P. Its voltage's start is a
/// guess (at 0 V the equation gives no current: a singular start).
fn power_load() -> ComponentDef {
    ComponentDef {
        name: "Test.PowerLoad".into(),
        ports: vec![port("p", "Pin", ""), port("n", "Pin", "")],
        params: vec![param("P", "W", 120.0, "the power it draws")],
        vars: vec![guess("v", "V", c(12.0)), var("i", "A", "")],
        equations: vec![
            eq(n("v"), n("p.v") - n("n.v"), "the voltage across it"),
            eq(c(0.0), n("p.i") + n("n.i"), "the current passes through"),
            eq(n("i"), n("p.i"), "its current"),
            eq(n("v") * n("i"), n("P"), "it draws the power P"),
        ],
        ..Default::default()
    }
}

/// A 12 V source behind 0.1 Ω feeding the load: v (12 - v) / 0.1 = 120,
/// v = 6 ± √24; the guess of 12 V leads to 6 + √24.
fn supply() -> ComponentDef {
    ComponentDef {
        name: "Test.Supply".into(),
        components: vec![
            sub("src", "Electrical.ConstantVoltage", &[("V", c(12.0))]),
            sub("r", "Electrical.Resistor", &[("R", c(0.1))]),
            sub("load", "Test.PowerLoad", &[]),
            sub("gnd", "Electrical.Ground", &[]),
            // something that moves, so the run has a state
            sub("drive", "Rotational.ConstantTorque", &[("tau", c(1.0))]),
            sub("rotor", "Rotational.Inertia", &[("J", c(1.0))]),
        ],
        connections: vec![
            connect("src.p", "r.p"),
            connect("r.n", "load.p"),
            connect("load.n", "src.n"),
            connect("src.n", "gnd.p"),
            connect("drive.flange", "rotor.a"),
        ],
        ..Default::default()
    }
}

#[test]
fn a_kept_variable_takes_the_start_of_an_alias_eliminated_in_its_favour() {
    let mut lib = library();
    lib.add(power_load());
    let (m, _) = prepare_with_report(&lib, &supply(), None, &Settings::default())
        .unwrap_or_else(|d| panic!("{d:#?}"));
    let v = m.flat.find_var("load.v").unwrap();
    // the load's voltage is an alias of the node's: the node's variable
    // carries its guess (with the alias's sign)
    let (rep, negated) = match m.aliases.iter().find(|a| a.var == v).map(|a| a.target) {
        Some(AliasTarget::Var { var, negated }) => (var, negated),
        other => panic!("load.v is not an alias of a variable: {other:?}"),
    };
    let s = m.flat.var(rep).start.expect("the kept variable has a start");
    assert_eq!(if negated { -s } else { s }, 12.0, "{}: the load's guess", m.flat.var(rep).name);
    let v_exact = 6.0 + 24f64.sqrt();
    let unsolved: Vec<&Diagnostic> =
        m.warnings.iter().filter(|w| w.code == "START-NOT-SOLVED").collect();
    assert!(unsolved.is_empty(), "the start solves at preparation: {unsolved:#?}");
    let run = simulate(&m, 1.0, 0.5, 1e-9);
    let load_v = run.channel("load.v").unwrap();
    assert!((load_v[0] - v_exact).abs() < 1e-9, "load.v = {}", load_v[0]);
    assert!((load_v.last().unwrap() - v_exact).abs() < 1e-9);
}

/// A load that draws a constant power P: v·i = P, with a guess for v.
fn power_load_named(name: &str, guess_v: f64) -> ComponentDef {
    ComponentDef {
        name: name.into(),
        ports: vec![port("p", "Pin", ""), port("n", "Pin", "")],
        params: vec![param("P", "W", 60.0, "the power it draws")],
        vars: vec![common::guess("v", "V", c(guess_v)), var("i", "A", "")],
        equations: vec![
            eq(n("v"), n("p.v") - n("n.v"), "the voltage across it"),
            eq(c(0.0), n("p.i") + n("n.i"), "the current passes through"),
            eq(n("i"), n("p.i"), "its current"),
            eq(n("v") * n("i"), n("P"), "it draws the power P"),
        ],
        ..Default::default()
    }
}

/// Two constant-power loads in parallel on a 12 V source behind 0.1 Ω:
/// v (12 - v) / 0.1 = 120 has two roots, 6 ± √24. One load guesses 12 V
/// (the high root, the one that runs), the other 1 V. Both guesses reach
/// the one node voltage alias elimination keeps. Which one the run starts
/// from must not depend on the order the parts are listed in (the review
/// found it chosen by model order, silently): the guess farthest from
/// zero wins, and preparation warns that the guesses disagree.
#[test]
fn conflicting_alias_guesses_do_not_depend_on_the_model_order() {
    let mut lib = library();
    lib.add(power_load_named("Test.LoadHigh", 12.0));
    lib.add(power_load_named("Test.LoadLow", 1.0));
    let top = |first: &str, second: &str| ComponentDef {
        name: "Test.Supply".into(),
        components: vec![
            sub("src", "Electrical.ConstantVoltage", &[("V", c(12.0))]),
            sub("r", "Electrical.Resistor", &[("R", c(0.1))]),
            sub("a", first, &[]),
            sub("b", second, &[]),
            sub("gnd", "Electrical.Ground", &[]),
            sub("drive", "Rotational.ConstantTorque", &[("tau", c(1.0))]),
            sub("rotor", "Rotational.Inertia", &[("J", c(1.0))]),
        ],
        connections: vec![
            connect("src.p", "r.p"),
            connect("r.n", "a.p"),
            connect("r.n", "b.p"),
            connect("a.n", "src.n"),
            connect("b.n", "src.n"),
            connect("src.n", "gnd.p"),
            connect("drive.flange", "rotor.a"),
        ],
        ..Default::default()
    };
    let high = 6.0 + 24f64.sqrt();
    let mut got = vec![];
    for (first, second) in [("Test.LoadHigh", "Test.LoadLow"), ("Test.LoadLow", "Test.LoadHigh")] {
        let (m, _) = prepare_with_report(&lib, &top(first, second), None, &Settings::default())
            .unwrap_or_else(|d| panic!("{d:#?}"));
        let run = simulate(&m, 1.0, 0.5, 1e-9);
        let v = run.channel("a.v").unwrap()[0];
        let conflict: Vec<&Diagnostic> =
            m.warnings.iter().filter(|w| w.code == "START-ALIAS-CONFLICT").collect();
        println!(
            "{first} listed first: node voltage {v:.6} V (high root {high:.6}); {:?}",
            conflict.iter().map(|w| &w.message).collect::<Vec<_>>()
        );
        assert_eq!(conflict.len(), 1, "the guesses' disagreement is told");
        assert!(conflict[0].message.contains("12 V") && conflict[0].message.contains("1 V"));
        got.push(v);
    }
    assert!(
        (got[0] - got[1]).abs() < 1e-6,
        "the operating point depends on the order the parts are listed in: {got:?}"
    );
    assert!((got[0] - high).abs() < 1e-6, "the guess farthest from zero: the high root");
}
