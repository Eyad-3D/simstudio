//! Work package 2's acceptance: tearing leaves at most the expected
//! tearing variables on a benchmark set of algebraic loops. For each loop
//! the heuristic's count is compared with the fewest possible, found by
//! exhaustive search over the block's unknowns.

mod common;

use common::library;
use lsim_ir::ComponentDef;
use lsim_ir::component::build::{connect, sub};
use lsim_ir::expr::c;
use lsim_prep::{BlockSummary, Settings, prepare_with_report};

fn blocks(top: &ComponentDef) -> (Vec<BlockSummary>, lsim_ir::PreparedModel) {
    let settings = Settings { tearing_minimum: true, ..Settings::default() };
    let (m, r) =
        prepare_with_report(&library(), top, None, &settings).unwrap_or_else(|d| panic!("{d:#?}"));
    (r.blocks.into_iter().filter(|b| b.size > 1).collect(), m)
}

fn model(name: &str, comps: Vec<lsim_ir::SubDecl>, conns: &[(&str, &str)]) -> ComponentDef {
    ComponentDef {
        name: name.into(),
        components: comps,
        connections: conns.iter().map(|(a, b)| connect(a, b)).collect(),
        ..Default::default()
    }
}

fn r(name: &str, ohm: f64) -> lsim_ir::SubDecl {
    sub(name, "Electrical.Resistor", &[("R", c(ohm))])
}

/// Checks the one algebraic block of `top`: its size, the tearing
/// variables the heuristic left (at most `expected`), the exhaustive
/// minimum, linearity and how many iteration variables remain.
fn check(name: &str, top: &ComponentDef, expected: usize, linear: bool, iterated: usize) {
    let (bs, m) = blocks(top);
    let b = bs.iter().max_by_key(|b| b.size).unwrap_or_else(|| panic!("{name}: no loop"));
    println!(
        "{name:<32} block of {:>3}: torn {} (fewest possible {:?}), linear {}, {} iterated; model: \
         {} states, {} iteration variables",
        b.size,
        b.torn,
        b.minimal,
        b.linear,
        b.iteration,
        m.states.len(),
        m.algebraics.len()
    );
    assert!(b.torn <= expected, "{name}: {} tearing variables, expected {expected}", b.torn);
    assert_eq!(b.minimal, Some(b.torn), "{name}: the heuristic is not minimal");
    assert_eq!(b.linear, linear, "{name}: linear");
    assert_eq!(b.iteration, iterated, "{name}: iteration variables");
}

#[test]
fn a_wheatstone_bridge() {
    let top = model(
        "Bench.Bridge",
        vec![
            sub("src", "Electrical.ConstantVoltage", &[("V", c(10.0))]),
            sub("gnd", "Electrical.Ground", &[]),
            r("r1", 1.0),
            r("r2", 2.0),
            r("r3", 3.0),
            r("r4", 4.0),
            r("r5", 5.0),
        ],
        &[
            ("src.p", "r1.p"),
            ("src.p", "r3.p"),
            ("r1.n", "r2.p"),
            ("r3.n", "r4.p"),
            ("r1.n", "r5.p"),
            ("r3.n", "r5.n"),
            ("r2.n", "src.n"),
            ("r4.n", "src.n"),
            ("src.n", "gnd.p"),
        ],
    );
    // one tearing variable, and the loop is solved symbolically for it
    check("Wheatstone bridge", &top, 1, true, 0);
    // the bridge current against the node equations solved by hand
    let (_, m) = blocks(&top);
    let vals = common::explicit_values(&m);
    let (v, r1, r2, r3, r4, r5) = (10.0, 1.0, 2.0, 3.0, 4.0, 5.0);
    // KCL at a and b: [1/r1 + 1/r2 + 1/r5, -1/r5; -1/r5, 1/r3 + 1/r4 + 1/r5] [va vb] = [v/r1, v/r3]
    let (a11, a12, a22) =
        (1.0 / r1 + 1.0 / r2 + 1.0 / r5, -1.0 / r5, 1.0 / r3 + 1.0 / r4 + 1.0 / r5);
    let det = a11 * a22 - a12 * a12;
    let va = (v / r1 * a22 - a12 * v / r3) / det;
    let vb = (a11 * v / r3 - a12 * v / r1) / det;
    let i5 = common::value_of(&m, &vals, "r5.i");
    assert!((i5 - (va - vb) / r5).abs() < 1e-14, "{i5} vs {}", (va - vb) / r5);
}

#[test]
fn a_resistive_ladder() {
    let mut comps = vec![
        sub("src", "Electrical.ConstantVoltage", &[("V", c(10.0))]),
        sub("gnd", "Electrical.Ground", &[]),
    ];
    let mut conns = vec![("src.n".to_string(), "gnd.p".to_string())];
    for k in 0..10 {
        comps.push(r(&format!("rs{k}"), 1.0));
        comps.push(r(&format!("rp{k}"), 10.0));
        let prev = if k == 0 { "src.p".to_string() } else { format!("rs{}.n", k - 1) };
        conns.push((prev, format!("rs{k}.p")));
        conns.push((format!("rs{k}.n"), format!("rp{k}.p")));
        conns.push((format!("rp{k}.n"), "gnd.p".into()));
    }
    let conns: Vec<(&str, &str)> = conns.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    let top = model("Bench.Ladder", comps, &conns);
    // one tearing variable; substituting the whole ladder into its residual
    // would grow without bound (each section reads two before it), so the
    // tearing variable stays an iteration variable
    check("resistive ladder, 10 sections", &top, 1, true, 1);
}

#[test]
fn a_rigid_gear_train() {
    let top = model(
        "Bench.GearTrain",
        vec![
            sub("torque", "Rotational.ConstantTorque", &[("tau", c(10.0))]),
            sub("j1", "Rotational.Inertia", &[("J", c(0.1))]),
            sub("g1", "Rotational.IdealGear", &[("ratio", c(3.0))]),
            sub("j2", "Rotational.Inertia", &[("J", c(0.5))]),
            sub("g2", "Rotational.IdealGear", &[("ratio", c(4.0))]),
            sub("j3", "Rotational.Inertia", &[("J", c(20.0))]),
            sub("loss", "Rotational.Damper", &[("d", c(0.3))]),
        ],
        &[
            ("torque.flange", "j1.a"),
            ("j1.b", "g1.a"),
            ("g1.b", "j2.a"),
            ("j2.b", "g2.a"),
            ("g2.b", "j3.a"),
            ("j3.b", "loss.flange"),
        ],
    );
    // index reduction leaves one state; the accelerations form one linear
    // loop, solved symbolically
    check("rigid gear train, 3 inertias", &top, 1, true, 0);
}

#[test]
fn a_diode_circuit() {
    let top = model(
        "Bench.Diode",
        vec![
            sub("src", "Electrical.ConstantVoltage", &[("V", c(5.0))]),
            sub("gnd", "Electrical.Ground", &[]),
            r("r", 100.0),
            sub("d", "Electrical.Diode", &[]),
        ],
        &[("src.p", "r.p"), ("r.n", "d.p"), ("d.n", "src.n"), ("src.n", "gnd.p")],
    );
    check("diode and resistor", &top, 1, false, 1);
}

#[test]
fn a_constant_power_load_on_a_battery() {
    let top = model(
        "Bench.PowerLoad",
        vec![
            sub("bat", "Battery.OcvR0Rc", &[]),
            sub("gnd", "Electrical.Ground", &[]),
            sub("load", "Electrical.PowerLoad", &[("P", c(20e3))]),
        ],
        &[("bat.p", "load.p"), ("load.n", "bat.n"), ("bat.n", "gnd.p")],
    );
    check("constant-power load on a battery", &top, 1, false, 1);
}

#[test]
fn a_feedback_loop_through_controllers() {
    let top = model(
        "Bench.SignalLoop",
        vec![
            sub("r", "Signal.Constant", &[("k", c(1.0))]),
            sub("sum", "Signal.Add", &[]),
            sub("k", "Signal.Gain", &[("k", c(4.0))]),
            sub("neg", "Signal.Gain", &[("k", c(-1.0))]),
        ],
        &[("r.y", "sum.u1"), ("sum.y", "k.u"), ("k.y", "neg.u"), ("neg.y", "sum.u2")],
    );
    check("feedback loop through gains", &top, 1, true, 0);
    let (_, m) = blocks(&top);
    // y = 4 (1 - y): y = 0.8
    let vals = common::explicit_values(&m);
    assert!((common::value_of(&m, &vals, "k.y") - 0.8).abs() < 1e-15);
    let w = m.warnings.iter().find(|d| d.code == "CAUSAL-LOOP").expect("warned");
    assert_eq!(w.parts, vec!["k", "neg", "sum"]);
}

#[test]
fn a_resistor_mesh() {
    // a 3 × 3 grid of nodes joined by resistors, driven across a diagonal
    let mut comps = vec![
        sub("src", "Electrical.ConstantVoltage", &[("V", c(1.0))]),
        sub("gnd", "Electrical.Ground", &[]),
    ];
    let mut conns: Vec<(String, String)> = vec![("src.n".into(), "gnd.p".into())];
    let node = |i: usize, j: usize| format!("n{i}{j}");
    let mut k = 0;
    let mut first: std::collections::BTreeMap<String, String> = Default::default();
    let mut join = |a: String, port: String, conns: &mut Vec<(String, String)>| match first.get(&a)
    {
        Some(p) => conns.push((p.clone(), port)),
        None => {
            first.insert(a, port);
        }
    };
    for i in 0..3 {
        for j in 0..3 {
            for (di, dj) in [(0, 1), (1, 0)] {
                let (i2, j2) = (i + di, j + dj);
                if i2 > 2 || j2 > 2 {
                    continue;
                }
                let name = format!("r{k}");
                k += 1;
                comps.push(r(&name, 1.0 + k as f64));
                join(node(i, j), format!("{name}.p"), &mut conns);
                join(node(i2, j2), format!("{name}.n"), &mut conns);
            }
        }
    }
    join(node(0, 0), "src.p".into(), &mut conns);
    join(node(2, 2), "src.n".into(), &mut conns);
    let conns: Vec<(&str, &str)> = conns.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    let top = model("Bench.Mesh", comps, &conns);
    // twelve resistors and the source on nine nodes: three tearing variables
    // suffice (fewer than the four independent loops)
    check("resistor mesh, 3 x 3 nodes", &top, 3, true, 3);
}

#[test]
fn a_pendulums_accelerations() {
    let top = ComponentDef {
        name: "Bench.Pendulum".into(),
        components: vec![sub("p", "Mechanics.CartesianPendulum", &[])],
        ..Default::default()
    };
    check("Cartesian pendulum, after index reduction", &top, 1, true, 0);
}
