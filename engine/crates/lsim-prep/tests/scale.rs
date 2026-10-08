//! Work package 2's acceptance: synthetic networks of 10⁵ equations
//! prepare in under a second (the test profile is optimised).

mod common;

use common::library;
use lsim_ir::ComponentDef;
use lsim_ir::component::build::{connect, sub};
use lsim_ir::expr::c;
use lsim_prep::{PrepReport, Settings, prepare_with_report};
use std::time::Instant;

/// An RC ladder of `n` sections driven by a source: series resistors,
/// shunt capacitors to one ground (a connection set of n + 2 ports).
fn rc_ladder(n: usize) -> ComponentDef {
    let mut comps = vec![
        sub("src", "Electrical.ConstantVoltage", &[("V", c(10.0))]),
        sub("gnd", "Electrical.Ground", &[]),
    ];
    let mut conns = vec![connect("src.n", "gnd.p")];
    for k in 0..n {
        comps.push(sub(
            &format!("r{k}"),
            "Electrical.Resistor",
            &[("R", c(1.0 + k as f64 * 1e-3))],
        ));
        comps.push(sub(&format!("c{k}"), "Electrical.Capacitor", &[("C", c(1e-3))]));
        let prev = if k == 0 { "src.p".to_string() } else { format!("r{}.n", k - 1) };
        conns.push(connect(&prev, &format!("r{k}.p")));
        conns.push(connect(&format!("r{k}.n"), &format!("c{k}.p")));
        conns.push(connect(&format!("c{k}.n"), "gnd.p"));
    }
    ComponentDef {
        name: "Scale.RcLadder".into(),
        components: comps,
        connections: conns,
        ..Default::default()
    }
}

/// A torsional chain: `n` inertias joined by spring-dampers.
fn torsional_chain(n: usize) -> ComponentDef {
    let mut comps = vec![sub("torque", "Rotational.ConstantTorque", &[("tau", c(10.0))])];
    let mut conns = vec![connect("torque.flange", "j0.a")];
    for k in 0..n {
        comps.push(sub(&format!("j{k}"), "Rotational.Inertia", &[("J", c(1.0))]));
        if k + 1 < n {
            comps.push(sub(
                &format!("s{k}"),
                "Rotational.SpringDamper",
                &[("k", c(1e4)), ("d", c(1.0))],
            ));
            conns.push(connect(&format!("j{k}.b"), &format!("s{k}.a")));
            conns.push(connect(&format!("s{k}.b"), &format!("j{}.a", k + 1)));
        }
    }
    ComponentDef {
        name: "Scale.Chain".into(),
        components: comps,
        connections: conns,
        ..Default::default()
    }
}

/// A resistive ladder (series and shunt resistors, no capacitors) closed by
/// one capacitor: one algebraic block as large as the ladder.
fn resistive_ladder(n: usize) -> ComponentDef {
    let mut comps = vec![
        sub("src", "Electrical.ConstantVoltage", &[("V", c(10.0))]),
        sub("gnd", "Electrical.Ground", &[]),
        sub("cend", "Electrical.Capacitor", &[("C", c(1e-3))]),
    ];
    let mut conns = vec![connect("src.n", "gnd.p")];
    for k in 0..n {
        comps.push(sub(&format!("rs{k}"), "Electrical.Resistor", &[("R", c(1.0))]));
        comps.push(sub(&format!("rp{k}"), "Electrical.Resistor", &[("R", c(100.0))]));
        let prev = if k == 0 { "src.p".to_string() } else { format!("rs{}.n", k - 1) };
        conns.push(connect(&prev, &format!("rs{k}.p")));
        conns.push(connect(&format!("rs{k}.n"), &format!("rp{k}.p")));
        conns.push(connect(&format!("rp{k}.n"), &format!("gnd{}.p", k)));
        comps.push(sub(&format!("gnd{k}"), "Electrical.Ground", &[]));
    }
    conns.push(connect(&format!("rs{}.n", n - 1), "cend.p"));
    conns.push(connect("cend.n", "gnd.p"));
    ComponentDef {
        name: "Scale.ResistiveLadder".into(),
        components: comps,
        connections: conns,
        ..Default::default()
    }
}

fn timed(top: &ComponentDef) -> (f64, usize, PrepReport, lsim_ir::PreparedModel) {
    let lib = library();
    let mut best = f64::INFINITY;
    let mut out = None;
    for _ in 0..2 {
        let t = Instant::now();
        let (m, r) = prepare_with_report(&lib, top, None, &Settings::default())
            .unwrap_or_else(|d| panic!("{:#?}", &d[..d.len().min(3)]));
        best = best.min(t.elapsed().as_secs_f64());
        out = Some((m, r));
    }
    let (m, r) = out.unwrap();
    (best, m.stats.flat_equations, r, m)
}

fn report(name: &str, secs: f64, eqs: usize, r: &PrepReport, m: &lsim_ir::PreparedModel) {
    let phases: Vec<String> =
        r.seconds.iter().map(|(p, s)| format!("{p} {:.0}", s * 1e3)).collect();
    println!(
        "{name}: {eqs} equations prepared in {:.0} ms ({} states, {} iteration variables, {} assignments; ms: {})",
        secs * 1e3,
        m.states.len(),
        m.algebraics.len(),
        m.assignments.len(),
        phases.join(", ")
    );
}

#[test]
fn an_rc_ladder_of_100k_equations_prepares_in_under_a_second() {
    let (secs, eqs, r, m) = timed(&rc_ladder(8400));
    report("RC ladder", secs, eqs, &r, &m);
    assert!(eqs >= 100_000, "{eqs}");
    assert_eq!(m.states.len(), 8400);
    assert!(m.algebraics.is_empty());
    assert!(secs < 1.0, "{secs} s");
}

#[test]
fn a_torsional_chain_of_100k_equations_prepares_in_under_a_second() {
    let (secs, eqs, r, m) = timed(&torsional_chain(6000));
    report("torsional chain", secs, eqs, &r, &m);
    assert!(eqs >= 100_000, "{eqs}");
    assert!(m.algebraics.is_empty());
    assert!(secs < 1.0, "{secs} s");
}

#[test]
fn a_resistive_ladder_of_100k_equations_prepares_in_under_a_second() {
    let (secs, eqs, r, m) = timed(&resistive_ladder(4200));
    report("resistive ladder", secs, eqs, &r, &m);
    for b in &r.blocks {
        println!(
            "  block of {}: {} torn, linear {}, {} iterated",
            b.size, b.torn, b.linear, b.iteration
        );
    }
    assert!(eqs >= 100_000, "{eqs}");
    assert!(secs < 1.0, "{secs} s");
}
