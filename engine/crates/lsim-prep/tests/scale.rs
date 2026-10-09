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

/// This thread's CPU time, s (Linux), or `None`.
fn cpu_seconds() -> Option<f64> {
    let s = std::fs::read_to_string("/proc/thread-self/schedstat").ok()?;
    let ns: f64 = s.split_whitespace().next()?.parse().ok()?;
    Some(ns * 1e-9)
}

/// What a timed preparation gave.
struct Timed {
    /// best CPU time of three runs, s (wall time where CPU time cannot be read)
    cpu: f64,
    /// best wall time, s
    wall: f64,
    report: PrepReport,
    model: lsim_ir::PreparedModel,
}

/// Prepares `top` three times.
fn timed(top: &ComponentDef) -> Timed {
    let lib = library();
    let (mut cpu, mut wall) = (f64::INFINITY, f64::INFINITY);
    let mut out = None;
    for _ in 0..3 {
        let (t, c) = (Instant::now(), cpu_seconds());
        let (m, r) = prepare_with_report(&lib, top, None, &Settings::default())
            .unwrap_or_else(|d| panic!("{:#?}", &d[..d.len().min(3)]));
        let w = t.elapsed().as_secs_f64();
        let c = match (c, cpu_seconds()) {
            (Some(a), Some(b)) => b - a,
            _ => w,
        };
        cpu = cpu.min(c);
        wall = wall.min(w);
        out = Some((m, r));
    }
    let (model, report) = out.unwrap();
    Timed { cpu, wall, report, model }
}

fn print(name: &str, t: &Timed) {
    let phases: Vec<String> =
        t.report.seconds.iter().map(|(p, s)| format!("{p} {:.0}", s * 1e3)).collect();
    let m = &t.model;
    println!(
        "{name}: {} equations prepared in {:.0} ms CPU ({:.0} ms wall); {} states, {} iteration \
         variables, {} assignments; last run by step, ms: {}",
        m.stats.flat_equations,
        t.cpu * 1e3,
        t.wall * 1e3,
        m.states.len(),
        m.algebraics.len(),
        m.assignments.len(),
        phases.join(", ")
    );
}

#[test]
fn an_rc_ladder_of_100k_equations_prepares_in_under_a_second() {
    let t = timed(&rc_ladder(8400));
    print("RC ladder", &t);
    assert!(t.model.stats.flat_equations >= 100_000);
    assert_eq!(t.model.states.len(), 8400);
    assert!(t.model.algebraics.is_empty());
    assert!(t.cpu < 1.0, "{} s", t.cpu);
}

#[test]
fn a_torsional_chain_of_100k_equations_prepares_in_under_a_second() {
    let t = timed(&torsional_chain(9100));
    print("torsional chain", &t);
    assert!(t.model.stats.flat_equations >= 100_000);
    assert!(t.model.algebraics.is_empty());
    assert!(t.cpu < 1.0, "{} s", t.cpu);
}

#[test]
fn a_resistive_ladder_of_100k_equations_prepares_in_under_a_second() {
    let t = timed(&resistive_ladder(7200));
    print("resistive ladder", &t);
    for b in &t.report.blocks {
        println!(
            "  block of {}: {} torn, linear {}, {} iterated",
            b.size, b.torn, b.linear, b.iteration
        );
    }
    assert!(t.model.stats.flat_equations >= 100_000);
    // one block as large as the ladder, torn to one variable
    let big = t.report.blocks.iter().max_by_key(|b| b.size).unwrap();
    assert!(
        big.size > 20_000 && big.torn == 1 && big.linear,
        "{} {} {}",
        big.size,
        big.torn,
        big.linear
    );
    assert!(t.cpu < 1.0, "{} s", t.cpu);
}
