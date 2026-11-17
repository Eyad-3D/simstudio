//! The Engine block's rev limiter: a free-revving engine at full throttle
//! reaches the full-load curve's last speed and settles in its limiter's
//! band just above it, firing the share of the time that holds it, its
//! torque and fuel in proportion. Before, fired and not fired flipped at
//! one instant and the run stopped with an event storm at t = 0.853 s.

use lsim_lib::blocks::engine::REV_BAND;
use lsim_project::model::Model;
use lsim_project::{ImportOptions, import_case, standard_registry};
use lsim_solve::{OutputGrid, SolverOptions};
use serde_json::{Value, json};

const RPM: f64 = std::f64::consts::PI / 30.0;

fn el(id: &str, kind: &str, over: Value) -> Value {
    json!({"id": id, "componentDefId": kind, "label": id, "parameterOverrides": over})
}

#[test]
fn a_free_revving_engine_is_held_at_its_rev_limit() {
    let p = json!({"id": "rig", "name": "Rig",
        "systems": [{"id": "sys-root", "name": "Rig", "parentId": null,
            "elements": [el("eng", "engine.combustion", json!({})),
                         el("thr", "signal.constant", json!({"value": 1.0})),
                         el("sh", "mech.shaft", json!({}))],
            "connections": [{"id": "w1", "sourceElementId": "eng", "sourcePortId": "shaft",
                             "targetElementId": "sh", "targetPortId": "flange_a"}]}],
        "dataBusConnections": [{"id": "l1", "element1Id": "thr", "port1Id": "sig_out",
                                "element2Id": "eng", "port2Id": "sig_throttle_in"}],
        "cases": [{"id": "c", "name": "c", "duration": 5, "timeStep": 0.01}]});
    let (top, rep) = import_case(&p, Some("c"), &standard_registry(), &ImportOptions::default())
        .unwrap_or_else(|e| panic!("{e:?}"));
    let model = Model::build(&rep.library(), &top).unwrap_or_else(|e| {
        panic!("{}", e.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n"))
    });
    let res = model
        .run(&SolverOptions::default(), OutputGrid { t0: 0.0, t_end: 5.0, dt: 0.01 }, &mut [])
        .unwrap_or_else(|e| panic!("{e}"));
    let ch = |name: &str| res.channel(name).unwrap_or_else(|| panic!("no {name}")).to_vec();
    let (speed, torque, fuel) =
        (ch("eng.sig_speed"), ch("eng.sig_torque"), ch("eng.sig_fuel_rate"));
    let top = 6000.0 * RPM;
    let hi = speed.iter().cloned().fold(0.0, f64::max);
    assert!(hi > 0.999 * top && hi < 6100.0 * RPM, "highest speed {} 1/min", hi / RPM);
    // over the last second it is held in the limiter's band: no torque (the
    // bare shaft takes none), a share of its full-load fuel
    for k in 400..=500 {
        assert!(
            speed[k] > top && speed[k] < top * (1.0 + REV_BAND),
            "t = {}: {} 1/min",
            k as f64 / 100.0,
            speed[k] / RPM
        );
        assert!(torque[k].abs() < 1e-3, "t = {}: torque {}", k as f64 / 100.0, torque[k]);
        assert!(fuel[k] > 0.0 && fuel[k] < fuel[50], "t = {}: fuel {}", k as f64 / 100.0, fuel[k]);
    }
}
