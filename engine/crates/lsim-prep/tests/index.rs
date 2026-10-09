//! Work package 2's acceptance: index-2 and index-3 models prepare (by
//! Pantelides' algorithm and dummy derivatives) and simulate to their
//! exact answers.

mod common;

use common::*;
use lsim_ir::ComponentDef;
use lsim_ir::component::build::{connect, sub};
use lsim_ir::expr::c;
use lsim_prep::{PrepOptions, Settings, prepare, prepare_with_report};

fn labelled(mut s: lsim_ir::SubDecl, label: &str) -> lsim_ir::SubDecl {
    s.label = Some(label.into());
    s
}

/// A motor's rotor driven at constant torque, through a 12:1 gear, spins a
/// vehicle-sized inertia: the two inertias are rigidly coupled (index 2).
/// `benchmarks/reference/problems/mech_gear_change.toml` before its shift.
fn gear_drive() -> ComponentDef {
    ComponentDef {
        name: "Test.GearDrive".into(),
        components: vec![
            labelled(
                sub("torque", "Rotational.ConstantTorque", &[("tau", c(200.0))]),
                "Motor torque",
            ),
            labelled(sub("rotor", "Rotational.Inertia", &[("J", c(0.05))]), "Rotor"),
            labelled(sub("gear", "Rotational.IdealGear", &[("ratio", c(12.0))]), "First gear"),
            labelled(sub("load", "Rotational.Inertia", &[("J", c(135.0))]), "Vehicle"),
        ],
        connections: vec![
            connect("torque.flange", "rotor.a"),
            connect("rotor.b", "gear.a"),
            connect("gear.b", "load.a"),
        ],
        ..Default::default()
    }
}

#[test]
fn inertias_coupled_through_a_gear_follow_the_exact_answer() {
    let (m, report) = prepare_with_report(&library(), &gear_drive(), None, &Settings::default())
        .unwrap_or_else(|d| panic!("{d:#?}"));
    println!("{report:#?}");
    assert!(report.differentiated >= 1, "index reduction differentiated the coupling");
    assert_eq!(m.states.len(), 1, "one state for one rigid body: {:?}", report.states);
    assert!(m.algebraics.is_empty(), "the rigid block is solved explicitly");
    let run = simulate(&m, 4.0, 0.01, 1e-10);
    // omega2 = T i t / (J2 + i^2 J1)
    let w2 = |t: f64| 200.0 * 12.0 * t / (135.0 + 144.0 * 0.05);
    let e_load = worst(&run, "load.w", w2);
    let e_rotor = worst(&run, "rotor.w", |t| 12.0 * w2(t));
    println!("errors: load {e_load:.1e}, rotor {e_rotor:.1e}");
    assert!(e_load < 1e-9 && e_rotor < 1e-9);
    // the reference problem's checkpoint at t = 2 s
    let k = run.times.iter().position(|&t| (t - 2.0).abs() < 1e-12).unwrap();
    let got = run.channel("load.w").unwrap()[k];
    assert!((got - 33.755274261603375).abs() < 1e-8 * 33.76, "{got}");
}

/// Two inertias connected directly, the second braked by a viscous loss:
/// w(t) = (T/d)(1 - exp(-d t / (J1 + J2))).
#[test]
fn inertias_connected_directly_share_one_state() {
    let top = ComponentDef {
        name: "Test.TwoInertias".into(),
        components: vec![
            sub("torque", "Rotational.ConstantTorque", &[("tau", c(10.0))]),
            sub("a", "Rotational.Inertia", &[("J", c(2.0))]),
            sub("b", "Rotational.Inertia", &[("J", c(3.0))]),
            sub("loss", "Rotational.Damper", &[("d", c(0.5))]),
        ],
        connections: vec![
            connect("torque.flange", "a.a"),
            connect("a.b", "b.a"),
            connect("b.b", "loss.flange"),
        ],
        ..Default::default()
    };
    let m = prepare(&library(), &top, &PrepOptions::default()).unwrap_or_else(|d| panic!("{d:#?}"));
    assert_eq!(m.states.len(), 1);
    assert!(m.algebraics.is_empty());
    let run = simulate(&m, 10.0, 0.05, 1e-10);
    let w = |t: f64| 10.0 / 0.5 * (1.0 - (-0.5 * t / 5.0).exp());
    let (ea, eb) = (worst(&run, "a.w", w), worst(&run, "b.w", w));
    println!("errors: a {ea:.1e}, b {eb:.1e}");
    assert!(ea < 1e-9 && eb < 1e-9);
}

/// A capacitor directly across a sine voltage source, with a resistor in
/// parallel and a series RC branch beside them: the first capacitor's
/// voltage is the source's (index 2) and its current C dV/dt; the branch's
/// capacitor charges as the closed form says. (The branch also gives the
/// model a state: the Stage 1 integrators need at least one.)
#[test]
fn a_capacitor_across_a_source_takes_c_dv_dt() {
    let (v0, f, cap, r) = (10.0, 50.0, 1e-3, 10.0);
    let (r2, c2) = (2.0, 1e-3);
    let top = ComponentDef {
        name: "Test.CapacitorAcrossSource".into(),
        components: vec![
            sub("src", "Electrical.SineVoltage", &[("V0", c(v0)), ("f", c(f))]),
            labelled(sub("cap", "Electrical.Capacitor", &[("C", c(cap))]), "DC link"),
            sub("res", "Electrical.Resistor", &[("R", c(r))]),
            sub("r2", "Electrical.Resistor", &[("R", c(r2))]),
            sub("c2", "Electrical.Capacitor", &[("C", c(c2))]),
            sub("gnd", "Electrical.Ground", &[]),
        ],
        connections: vec![
            connect("src.p", "cap.p"),
            connect("src.p", "res.p"),
            connect("src.p", "r2.p"),
            connect("r2.n", "c2.p"),
            connect("src.n", "cap.n"),
            connect("src.n", "res.n"),
            connect("src.n", "c2.n"),
            connect("src.n", "gnd.p"),
        ],
        ..Default::default()
    };
    let (m, report) = prepare_with_report(&library(), &top, None, &Settings::default())
        .unwrap_or_else(|d| panic!("{d:#?}"));
    println!("{report:#?}");
    assert!(report.differentiated >= 1);
    assert_eq!(report.states, vec!["c2.v".to_string()], "the link capacitor is no state");
    assert!(m.algebraics.is_empty());
    assert!(m.warnings.is_empty(), "{:#?}", m.warnings);
    let w = 2.0 * std::f64::consts::PI * f;
    let run = simulate(&m, 0.05, 1e-4, 1e-10);
    let ev = worst(&run, "cap.v", |t| v0 * (w * t).sin());
    let ei = worst(&run, "cap.i", |t| cap * v0 * w * (w * t).cos());
    let tau = r2 * c2;
    let wt = w * tau;
    let v2 = |t: f64| {
        v0 / (1.0 + wt * wt) * ((w * t).sin() - wt * (w * t).cos() + wt * (-t / tau).exp())
    };
    let e2 = worst(&run, "c2.v", v2);
    println!("errors: v {ev:.1e}, i {ei:.1e}, branch {e2:.1e}");
    assert!(ev < 1e-12 && ei < 1e-12, "the index-2 part is exact up to rounding");
    assert!(e2 < 1e-8);
}

/// The same with a constant source: the capacitor's fixed start of 0 V
/// cannot hold, and preparation says so instead of failing.
#[test]
fn a_start_value_index_reduction_overrides_is_reported() {
    let top = ComponentDef {
        name: "Test.CapacitorOnBattery".into(),
        components: vec![
            labelled(sub("src", "Electrical.ConstantVoltage", &[("V", c(12.0))]), "Battery"),
            labelled(sub("cap", "Electrical.Capacitor", &[("C", c(1e-3))]), "DC link"),
            sub("r2", "Electrical.Resistor", &[("R", c(2.0))]),
            sub("c2", "Electrical.Capacitor", &[("C", c(1e-3))]),
            sub("gnd", "Electrical.Ground", &[]),
        ],
        connections: vec![
            connect("src.p", "cap.p"),
            connect("src.p", "r2.p"),
            connect("r2.n", "c2.p"),
            connect("src.n", "cap.n"),
            connect("src.n", "c2.n"),
            connect("src.n", "gnd.p"),
        ],
        ..Default::default()
    };
    let m = prepare(&library(), &top, &PrepOptions::default()).unwrap_or_else(|d| panic!("{d:#?}"));
    let w = m.warnings.iter().find(|d| d.code == "INIT-START-IGNORED").expect("warned");
    println!("{w}");
    assert_eq!(w.parts, vec!["cap".to_string()]);
    assert!(w.message.contains("'DC link'") && w.message.contains("12"), "{}", w.message);
    let run = simulate(&m, 0.02, 0.001, 1e-10);
    assert!(run.channel("cap.v").unwrap().iter().all(|&v| (v - 12.0).abs() < 1e-12));
    assert!(run.channel("cap.i").unwrap().iter().all(|&i| i.abs() < 1e-12));
    let e2 = worst(&run, "c2.v", |t| 12.0 * (1.0 - (-t / 2e-3).exp()));
    assert!(e2 < 1e-8, "{e2:e}");
}

/// A pendulum in Cartesian coordinates (index 3): against the exact
/// solution by Jacobi's elliptic functions, the rod's length held exactly.
#[test]
fn a_cartesian_pendulum_swings_exactly() {
    let theta0 = 0.5;
    let top = ComponentDef {
        name: "Test.Pendulum".into(),
        components: vec![labelled(
            sub("p", "Mechanics.CartesianPendulum", &[("theta0", c(theta0)), ("L", c(1.5))]),
            "Pendulum",
        )],
        ..Default::default()
    };
    let (m, report) = prepare_with_report(&library(), &top, None, &Settings::default())
        .unwrap_or_else(|d| panic!("{d:#?}"));
    println!("{report:#?}");
    assert_eq!(report.states, vec!["p.x".to_string(), "p.vx".to_string()]);
    assert!(report.differentiated >= 3, "the constraint differentiated twice and more");
    let run = simulate(&m, 3.0, 0.01, 1e-10);
    let (l, g) = (1.5, 9.81);
    let ex = worst(&run, "p.x", |t| l * pendulum_angle(theta0, g, l, t).sin());
    let ey = worst(&run, "p.y", |t| -l * pendulum_angle(theta0, g, l, t).cos());
    let (x, y) = (run.channel("p.x").unwrap(), run.channel("p.y").unwrap());
    let drift = x
        .iter()
        .zip(y)
        .map(|(x, y)| (x * x + y * y).sqrt() - l)
        .fold(0.0f64, |a, d| a.max(d.abs()));
    println!("errors: x {ex:.1e}, y {ey:.1e}; length drift {drift:.1e}");
    assert!(ex < 1e-7 && ey < 1e-7);
    // the rod's length is an equation of the reduced model: it holds to the
    // solver's own tolerance at every time, with no drift growing
    assert!(drift < 1e-9);
}
