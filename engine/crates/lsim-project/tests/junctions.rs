//! Junctions with as many terminals as a part lists (`terminals`): an
//! Electric Node with seven terminals gives the same currents and powers as
//! the same loads on two chained five-terminal nodes, and a Mechanical Node
//! with six flanges the same speeds and torques as two chained four-flange
//! nodes. With the library's own terminals a node is the definition it
//! always was.

mod common;

use common::{model, part, run};
use lsim_lib::blocks::{driveline, electric};
use lsim_project::model::Model;
use lsim_project::{ImportOptions, import_case, standard_registry};
use lsim_solve::{OutputGrid, SimResult, SolverOptions};
use serde_json::{Value, json};

fn wire(id: &str, a: &str, pa: &str, b: &str, pb: &str) -> Value {
    json!({"id": id, "sourceElementId": a, "sourcePortId": pa, "targetElementId": b, "targetPortId": pb})
}

/// A 10 kWh battery feeding six loads (1 to 3.5 kW) through one node with
/// seven terminals (the battery and four loads on t1 to t5, two more loads
/// on t6 and t7), or through two chained five-terminal nodes (the second on
/// the first's t5).
fn bus(seven: bool) -> Value {
    let mut els = vec![
        json!({"id": "el-bat", "componentDefId": "battery.generic", "label": "Pack",
               "parameterOverrides": {"capacity_kWh": 10, "initial_soc_pct": 80}}),
        json!({"id": "el-gnd", "componentDefId": "boundary.ground", "label": "Ground"}),
    ];
    let mut wires = vec![
        wire("w-gnd", "el-bat", "neg", "el-gnd", "t1"),
        wire("w-bat", "el-bat", "pos", "el-n", "t1"),
    ];
    for k in 1..=6 {
        let id = format!("el-l{k}");
        els.push(json!({"id": id, "componentDefId": "electric.constant_drive", "label": format!("Load {k}"),
                         "parameterOverrides": {"power_kW": 0.5 + 0.5 * k as f64}}));
        wires.push(wire(&format!("w-r{k}"), &id, "neg", "el-gnd", "t2"));
        let (node, t) = match (seven, k) {
            (true, _) => ("el-n", k + 1),
            (false, 1..=3) => ("el-n", k + 1),
            (false, _) => ("el-m", k - 2),
        };
        wires.push(wire(&format!("w-l{k}"), node, &format!("t{t}"), &id, "pos"));
    }
    if seven {
        els.push(json!({"id": "el-n", "componentDefId": "electric.node", "label": "Bus",
                        "terminals": ["t1", "t2", "t3", "t4", "t5", "t6", "t7"]}));
    } else {
        els.push(json!({"id": "el-n", "componentDefId": "electric.node", "label": "Bus"}));
        els.push(json!({"id": "el-m", "componentDefId": "electric.node", "label": "Bus 2"}));
        wires.push(wire("w-chain", "el-n", "t5", "el-m", "t1"));
    }
    json!({
        "id": "bus", "name": "Bus",
        "systems": [{"id": "sys-root", "name": "Bus", "parentId": null,
                     "elements": els, "connections": wires}],
        "dataBusConnections": [],
        "cases": [{"id": "c1", "name": "Ten minutes", "duration": 600, "timeStep": 60}]
    })
}

/// The element `id` of the project.
fn element<'a>(p: &'a mut Value, id: &str) -> &'a mut Value {
    p["systems"][0]["elements"].as_array_mut().unwrap().iter_mut().find(|e| e["id"] == id).unwrap()
}

fn run_project(p: &Value) -> SimResult {
    let reg = standard_registry();
    let (top, rep) = import_case(p, Some("c1"), &reg, &ImportOptions::default())
        .unwrap_or_else(|e| panic!("{e:?}"));
    let model = Model::build(&rep.library(), &top).unwrap_or_else(|e| panic!("{e:?}"));
    let opts = SolverOptions { rtol: 1e-8, atol: 1e-10, ..Default::default() };
    model.run(&opts, OutputGrid { t0: 0.0, t_end: 600.0, dt: 60.0 }, &mut []).unwrap()
}

/// Every channel of `a` whose name starts with one of `prefixes` is in `b`
/// with the same values, to `tol` of its largest magnitude (the two runs'
/// equations differ, so their steps do: the same to the solver's
/// tolerance); how many, and the largest difference found, as a share.
fn same_channels(a: &SimResult, b: &SimResult, prefixes: &[&str], tol: f64) -> (usize, f64) {
    let (mut n, mut worst) = (0, 0.0_f64);
    for (name, va) in a.names.iter().zip(&a.values) {
        if !prefixes.iter().any(|p| name.starts_with(p)) {
            continue;
        }
        let vb = b.channel(name).unwrap_or_else(|| panic!("{name} only in one run"));
        let scale = va.iter().fold(1e-3_f64, |m, x| m.max(x.abs()));
        for (k, (x, y)) in va.iter().zip(vb).enumerate() {
            worst = worst.max((x - y).abs() / scale);
            assert!((x - y).abs() <= tol * scale, "{name} at point {k}: {x} vs {y}");
        }
        n += 1;
    }
    (n, worst)
}

#[test]
fn seven_terminals_carry_what_two_chained_nodes_carry() {
    let one = run_project(&bus(true));
    let two = run_project(&bus(false));
    let parts = ["el_bat.", "el_l1.", "el_l2.", "el_l3.", "el_l4.", "el_l5.", "el_l6.", "el_gnd."];
    let (n, worst) = same_channels(&one, &two, &parts, 1e-7);
    println!("electric: {n} channels, largest difference {worst:.1e} of the channel's size");
    assert!(n > 20, "{n} channels compared");
    // the loads take their power, the battery gives it all
    let p: f64 = (1..=6).map(|k| one.channel(&format!("el_l{k}.sig_power")).unwrap()[5]).sum();
    assert!((p - 13_500.0).abs() < 1e-6, "{p}");
    // the node's Throughput Power is its source's, through the terminal the
    // battery is wired to (t1), as the first of the two chained nodes'
    let thr = one.channel("el_n.sig_power").unwrap();
    let thr2 = two.channel("el_n.sig_power").unwrap();
    for (x, y) in thr.iter().zip(thr2) {
        assert!((x - y).abs() <= 1e-7 * 13_500.0, "{x} vs {y}");
    }
    assert!((thr[5] - 13_500.0).abs() < 1.0, "{}", thr[5]);
}

#[test]
fn a_node_with_the_librarys_terminals_is_the_definition_it_always_was() {
    let lib5 = electric::NODE_TERMINALS.map(String::from);
    assert_eq!(electric::electric_node(&lib5, None).name, "Blocks.ElectricNode");
    assert_eq!(electric::electric_node(&lib5, Some(0)).name, "Blocks.ElectricNode_src1");
    let lib4 = driveline::NODE_FLANGES.map(String::from);
    assert_eq!(driveline::mech_node(&lib4).name, "Blocks.MechNode");
    // a part's own list makes a definition of its own
    let own: Vec<String> = ["t1", "t2", "t4", "t6"].map(String::from).to_vec();
    let d = electric::electric_node(&own, Some(1));
    assert_eq!(d.name, "Blocks.ElectricNode_t1_t2_t4_t6_src2");
    assert_eq!(
        d.ports.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        ["t1", "t2", "t4", "t6", "sig_power"]
    );
    // listing the library's terminals is the library's definition
    let mut p = bus(false);
    element(&mut p, "el-n")["terminals"] = json!(["t1", "t2", "t3", "t4", "t5"]);
    let reg = standard_registry();
    let (top, _) = import_case(&p, Some("c1"), &reg, &ImportOptions::default()).unwrap();
    assert!(
        top.components.iter().all(|s| !s.def.contains("ElectricNode_t")),
        "{:?}",
        top.components.iter().map(|s| &s.def).collect::<Vec<_>>()
    );
}

#[test]
fn a_node_needs_at_least_two_terminals_each_named_once() {
    // (a wire to a terminal a node does not list is refused before a run,
    // by the data checks, as a wire to any port a part does not have)
    let reg = standard_registry();
    for bad in [json!(["t1"]), json!(["t1", "t1"])] {
        let mut q = bus(true);
        element(&mut q, "el-n")["terminals"] = bad;
        let e = import_case(&q, Some("c1"), &reg, &ImportOptions::default()).unwrap_err();
        assert!(e.iter().any(|d| d.message.contains("at least two terminals")), "{e:?}");
    }
}

/// A constant torque driving an inertia and five dampers through one node
/// of six flanges (four wired, two more loads), or through two chained
/// four-flange nodes.
fn shaft(six: bool) -> SimResult {
    let mut lib = common::lib();
    let fs = |n: usize| (1..=n).map(|k| format!("f{k}")).collect::<Vec<_>>();
    lib.add(driveline::mech_node(&fs(6)));
    let mut parts = vec![
        part("drive", "Rotational.ConstantTorque", &[("tau", 20.0)]),
        part("mass", "Rotational.Inertia", &[("J", 2.0)]),
        part("d0", "Rotational.Damper", &[("d", 0.1)]),
    ];
    for k in 1..=4 {
        parts.push(part(&format!("d{k}"), "Rotational.Damper", &[("d", 0.1 * k as f64)]));
    }
    let mut conns = vec![("mass.b", "d0.flange")];
    let top = if six {
        parts.push(part("n", "Blocks.MechNode_f1_f2_f3_f4_f5_f6", &[]));
        conns.extend([
            ("drive.flange", "n.f1"),
            ("mass.a", "n.f2"),
            ("d1.flange", "n.f3"),
            ("d2.flange", "n.f4"),
            ("d3.flange", "n.f5"),
            ("d4.flange", "n.f6"),
        ]);
        model("Six", parts, &conns)
    } else {
        parts.push(part("n", "Blocks.MechNode", &[]));
        parts.push(part("m", "Blocks.MechNode", &[]));
        conns.extend([
            ("drive.flange", "n.f1"),
            ("mass.a", "n.f2"),
            ("d1.flange", "n.f3"),
            ("n.f4", "m.f1"),
            ("d2.flange", "m.f2"),
            ("d3.flange", "m.f3"),
            ("d4.flange", "m.f4"),
        ]);
        model("Chained", parts, &conns)
    };
    run(&lib, &top, 40.0, 1.0, 1e-9).1
}

#[test]
fn six_flanges_turn_as_two_chained_nodes_turn() {
    let one = shaft(true);
    let two = shaft(false);
    let (n, worst) =
        same_channels(&one, &two, &["drive.", "mass.", "d0.", "d1.", "d2.", "d3.", "d4."], 1e-7);
    println!("mechanical: {n} channels, largest difference {worst:.1e} of the channel's size");
    assert!(n >= 10, "{n} channels compared");
    // and as the exact answer: w = tau/D (1 − e^(−D t / J)), D = 1.1
    let w = one.channel("mass.w").unwrap();
    for (t, x) in one.times.iter().zip(w) {
        let exact = 20.0 / 1.1 * (1.0 - lsim_ir::math::exp(-1.1 * t / 2.0));
        assert!((x - exact).abs() < 1e-6 * 20.0 / 1.1, "t = {t}: {x} vs {exact}");
    }
    // the node's Node Speed is the shaft's
    let s = one.channel("n.sig_speed").unwrap();
    assert!((s[40] - w[40]).abs() < 1e-9 * w[40], "{} vs {}", s[40], w[40]);
}
