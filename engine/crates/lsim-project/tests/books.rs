//! Every block's energy books close: each part's energy in through its
//! ports less its losses less its stored energy's change is zero up to the
//! integration's accuracy, in rigs that exercise the blocks the example
//! projects do not (a DC-DC converter, a heat pump, a gearbox, a propeller,
//! an engine and its tank, a clutch, a transfer case, a fuel cell and its
//! hydrogen tank, a voltage source) and in the examples themselves.

use lsim_project::model::Model;
use lsim_project::{ImportOptions, import_case, standard_registry};
use lsim_solve::{OutputGrid, SimResult, SolverOptions};
use serde_json::{Value, json};

const TOL: f64 = 1e-6;

fn run(project: &Value, case: &str, t_end: f64) -> SimResult {
    let (top, rep) =
        import_case(project, Some(case), &standard_registry(), &ImportOptions::default())
            .unwrap_or_else(|e| panic!("{e:?}"));
    let model = Model::build(&rep.library(), &top).unwrap_or_else(|e| {
        panic!("{}", e.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n"))
    });
    let opts = SolverOptions { rtol: 1e-8, atol: 1e-10, ..Default::default() };
    model
        .run(&opts, OutputGrid { t0: 0.0, t_end, dt: 1.0 }, &mut [])
        .unwrap_or_else(|e| panic!("{e}"))
}

fn check_books(res: &SimResult, what: &str) -> Vec<String> {
    let e = res.energy.as_ref().expect("books");
    let mut checked = vec![];
    for p in e.parts.iter().filter(|p| p.declared) {
        let s = p.throughput.max(p.stored_change.abs()).max(1.0);
        assert!(
            p.closure.abs() <= TOL * s,
            "{what}: the books of {} ({}) do not close: in {:.6e} − lost {:.6e} − stored {:.6e} = {:.3e} (throughput {:.3e})",
            p.path,
            p.name,
            p.energy_in,
            p.lost,
            p.stored_integral,
            p.closure,
            p.throughput
        );
        checked.push(p.path.clone());
    }
    assert!(e.relative_closure <= TOL, "{what}: {}", e.summary());
    checked
}

fn el(id: &str, kind: &str, over: Value) -> Value {
    json!({"id": id, "componentDefId": kind, "label": id, "parameterOverrides": over})
}

fn wire(a: &str, pa: &str, b: &str, pb: &str) -> Value {
    json!({"id": format!("{a}-{pa}-{b}-{pb}"), "sourceElementId": a, "sourcePortId": pa,
           "targetElementId": b, "targetPortId": pb})
}

fn link(a: &str, pa: &str, b: &str, pb: &str) -> Value {
    json!({"id": format!("{a}-{pa}-{b}-{pb}"), "element1Id": a, "port1Id": pa,
           "element2Id": b, "port2Id": pb})
}

fn project(elements: Vec<Value>, wires: Vec<Value>, links: Vec<Value>) -> Value {
    json!({"id": "rig", "name": "Rig",
           "systems": [{"id": "sys-root", "name": "Rig", "parentId": null,
                        "elements": elements, "connections": wires}],
           "dataBusConnections": links,
           "cases": [{"id": "c", "name": "c", "duration": 60, "timeStep": 1}]})
}

/// A bench supply through a DC-DC converter to a bus with a load, a heat
/// pump and a motor that turns a propeller through a shaft and a gearbox.
#[test]
fn an_electric_rig_closes_its_books() {
    let p = project(
        vec![
            el("src", "electric.voltage_source", json!({"voltage_V": 400})),
            el("dcdc", "controller.dcdc", json!({"output_voltage_V": 350, "efficiency_pct": 96})),
            el("bus", "electric.node", json!({})),
            el("load", "electric.constant_drive", json!({"power_kW": 0.8})),
            el("hp", "electric.climate", json!({"heat_source": "Heat pump"})),
            el("amb", "boundary.ambient", json!({"temperature_C": -5})),
            el("mot", "motor.emotor", json!({})),
            el("cmd", "signal.constant", json!({"value": 0.6})),
            el("shaft", "mech.shaft", json!({"efficiency_pct": 99})),
            el("gb", "mech.gearbox", json!({"default_gear": 2})),
            el("prop", "propulsion.propeller", json!({})),
        ],
        vec![
            wire("src", "pos", "dcdc", "a_pos"),
            wire("dcdc", "b_pos", "bus", "t1"),
            wire("bus", "t2", "load", "pos"),
            wire("bus", "t3", "hp", "pos"),
            wire("bus", "t4", "mot", "pos"),
            wire("mot", "shaft", "shaft", "flange_a"),
            wire("shaft", "flange_b", "gb", "flange_in"),
            wire("gb", "flange_out", "prop", "shaft"),
        ],
        vec![link("cmd", "sig_out", "mot", "sig_demand_in")],
    );
    let res = run(&p, "c", 60.0);
    let parts = check_books(&res, "electric rig");
    for want in ["dcdc", "load", "hp", "mot", "shaft", "gb", "prop"] {
        assert!(parts.iter().any(|x| x == want), "{want} has no books: {parts:?}");
    }
    // it turned the propeller and heated
    assert!(res.channel("prop.sig_speed").unwrap()[60] > 10.0);
    assert!(res.channel("hp.sig_heat").unwrap()[60] > 0.0);
}

/// An engine fed by its tank, through a clutch and a transfer case to two
/// propellers; a fuel cell fed by its hydrogen tank, with a load.
#[test]
fn a_fuel_rig_closes_its_books() {
    let p = project(
        vec![
            el("tank", "fuel.tank", json!({})),
            el("eng", "engine.combustion", json!({})),
            el("thr", "signal.constant", json!({"value": 0.5})),
            el("cl", "mech.clutch", json!({"max_torque_Nm": 400})),
            el("tc", "mech.transfer_case", json!({"torque_split_a_pct": 40})),
            el("p1", "propulsion.propeller", json!({})),
            el("p2", "propulsion.propeller", json!({})),
            el("h2", "fuel.h2_tank", json!({})),
            el("fc", "fuelcell.stack", json!({})),
            el("load", "electric.constant_drive", json!({"power_kW": 20})),
        ],
        vec![
            wire("eng", "shaft", "cl", "flange_a"),
            wire("cl", "flange_b", "tc", "flange_in"),
            wire("tc", "flange_out_a", "p1", "shaft"),
            wire("tc", "flange_out_b", "p2", "shaft"),
            wire("fc", "pos", "load", "pos"),
        ],
        vec![link("thr", "sig_out", "eng", "sig_throttle_in")],
    );
    let res = run(&p, "c", 60.0);
    let parts = check_books(&res, "fuel rig");
    for want in ["tank", "eng", "cl", "tc", "p1", "p2", "h2", "fc", "load"] {
        assert!(parts.iter().any(|x| x == want), "{want} has no books: {parts:?}");
    }
    assert!(res.channel("eng.fuel_used").unwrap()[60] > 0.0);
    assert!(res.channel("h2.m").unwrap()[60] < res.channel("h2.m").unwrap()[0]);
}

/// The example cars' parts over the first two minutes of their cycles.
#[test]
fn the_example_cars_close_their_books() {
    let dir =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../backend/projects");
    for (name, case) in [
        ("bev-car", "case-wltc-winter"),
        ("aero-bev", "case-udds"),
        ("fs-electric", "case-accel-75m"),
    ] {
        let p: Value = serde_json::from_str(
            &std::fs::read_to_string(dir.join(format!("{name}.json"))).unwrap(),
        )
        .unwrap();
        let res = run(&p, case, if name == "fs-electric" { 4.0 } else { 120.0 });
        let parts = check_books(&res, name);
        assert!(parts.len() >= 10, "{name}: {parts:?}");
    }
}
