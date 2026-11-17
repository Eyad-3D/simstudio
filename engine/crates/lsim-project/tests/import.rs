//! The importer on small projects: sub-systems, signal links across them,
//! unit conversions at links into blocks without units, implicit links,
//! unwired inputs, case values and the channel map.

use lsim_project::model::Model;
use lsim_project::{ImportOptions, import, import_case, standard_registry};
use lsim_solve::{OutputGrid, SolverOptions};
use serde_json::json;

#[test]
fn the_registry_maps_all_36_blocks() {
    let reg = standard_registry();
    let ids: Vec<&str> = reg.ids().collect();
    assert_eq!(ids.len(), 36, "{ids:?}");
    let cat = lsim_lib::blocks::catalog::catalog();
    for c in cat["components"].as_array().unwrap() {
        let id = c["id"].as_str().unwrap();
        assert!(reg.get(id).is_some(), "{id} has no mapping");
    }
}

/// A battery feeding a heater-like consumer whose demand comes from a
/// Constant inside a sub-system (in kW, as today's numbers), and a Script
/// block's port reading the battery's SOC in %.
fn project() -> serde_json::Value {
    json!({
        "id": "demo", "name": "Demo",
        "systems": [
            {"id": "sys-root", "name": "Demo", "parentId": null,
             "elements": [
                {"id": "el-bat", "componentDefId": "battery.generic", "label": "Pack",
                 "parameterOverrides": {"capacity_kWh": 10, "initial_soc_pct": 80}},
                {"id": "el-load", "componentDefId": "electric.constant_drive", "label": "Load"},
                {"id": "el-sub", "componentDefId": "container.system", "label": "Controls",
                 "isSubSystem": true, "subSystemId": "sys-ctl"},
                {"id": "el-mon", "componentDefId": "signal.monitor", "label": "Monitor",
                 "dynamicPorts": [{"id": "soc", "name": "SOC", "direction": "input",
                                   "kind": "signal", "unitGroup": "Percent"}]}
             ],
             "connections": [
                {"id": "w1", "sourceElementId": "el-bat", "sourcePortId": "pos",
                 "targetElementId": "el-load", "targetPortId": "pos"}
             ]},
            {"id": "sys-ctl", "name": "Controls", "parentId": "sys-root",
             "elements": [
                {"id": "el-k", "componentDefId": "signal.constant", "label": "Demand",
                 "parameterOverrides": {"value": 2.5}}
             ],
             "connections": []}
        ],
        "dataBusConnections": [
            {"id": "l1", "element1Id": "el-k", "port1Id": "sig_out",
             "element2Id": "el-load", "port2Id": "sig_demand_in"},
            {"id": "l2", "element1Id": "el-bat", "port1Id": "sig_soc",
             "element2Id": "el-mon", "port2Id": "soc"}
        ],
        "cases": [
            {"id": "c1", "name": "Ten minutes", "duration": 600, "timeStep": 60,
             "parameterOverrides": {"el-k": {"value": 5.0}}}
        ]
    })
}

#[test]
fn a_sub_system_converts_and_links_as_today() {
    let reg = standard_registry();
    let (top, rep) = import(&project(), &reg).unwrap_or_else(|e| panic!("{e:?}"));
    // the open negative terminals share an implicit ground
    assert!(top.components.iter().any(|s| s.def == "Electrical.Ground"));
    // a Constant (a number in today's kW) into a power input: converted
    let conv =
        top.components.iter().find(|s| s.def.starts_with("Signal.Convert")).expect("a conversion");
    let k = conv.modifiers.iter().find(|m| m.param == "k").unwrap();
    assert_eq!(k.value, lsim_ir::ParamValue::Real(lsim_ir::expr::c(1000.0)));
    // the channel map names today's channels with their display units
    let soc = &rep.channels["el-bat:sig_soc"];
    assert_eq!(soc.var, "el_bat.sig_soc");
    assert_eq!(soc.unit.scale, 0.01);
    assert_eq!(rep.channel_map["el-mon:soc"], "el_mon.soc");
    let lib = rep.library();
    let model = Model::build(&lib, &top).unwrap_or_else(|e| panic!("{e:?}"));
    let res = model
        .run(&SolverOptions::default(), OutputGrid { t0: 0.0, t_end: 600.0, dt: 60.0 }, &mut [])
        .unwrap();
    // 2.5 kW for 600 s from 10 kWh at about the OCV table's mean voltage
    let p = res.channel("el_load.sig_power").unwrap();
    assert!((p[5] - 2500.0).abs() < 1e-6, "{p:?}");
    let soc_pct = res.channel("el_mon.soc").unwrap();
    let soc = res.channel("el_bat.soc").unwrap();
    assert!((soc_pct[10] - 100.0 * soc[10]).abs() < 1e-9, "the monitor reads %");
    assert!(soc[10] < 0.8 && soc[10] > 0.75, "{}", soc[10]);
    // the case's own value
    let (top2, rep2) =
        import_case(&project(), Some("c1"), &reg, &ImportOptions::default()).unwrap();
    let m2 = Model::build(&rep2.library(), &top2).unwrap();
    let r2 = m2
        .run(&SolverOptions::default(), OutputGrid { t0: 0.0, t_end: 600.0, dt: 60.0 }, &mut [])
        .unwrap();
    assert!((r2.channel("el_load.sig_power").unwrap()[5] - 5000.0).abs() < 1e-6);
    assert_eq!(rep2.run.duration, 600.0);
    assert_eq!(rep2.run.time_step, 60.0);
}

#[test]
fn an_unknown_block_and_a_lap_case_are_refused_in_plain_words() {
    let reg = standard_registry();
    let mut p = project();
    p["systems"][0]["elements"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id": "el-x", "componentDefId": "electric.heater", "label": "Heater"}));
    let e = import(&p, &reg).unwrap_err();
    assert!(e[0].message.contains("'Heater'"), "{}", e[0].message);
    let mut q = project();
    q["cases"][0]["kind"] = json!("lap");
    let e = import_case(&q, Some("c1"), &reg, &ImportOptions::default()).unwrap_err();
    assert_eq!(e[0].code, "LAP-CASE");
}

/// A part's start value (a number the importer set) in the hybrid car
/// started at 80 km/h, with `edit` applied to the project first.
fn hybrid_start(edit: &dyn Fn(&mut serde_json::Value), part: &str, param: &str) -> Option<f64> {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut p: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("../../../backend/projects/hybrid-car.json")).unwrap(),
    )
    .unwrap();
    let veh = &mut p["systems"][0]["elements"][0];
    assert_eq!(veh["id"], "el-vehicle");
    veh["parameterOverrides"]["initial_speed_kmh"] = json!(80.0);
    edit(&mut p);
    let (top, _) = import(&p, &standard_registry()).unwrap_or_else(|e| panic!("{e:?}"));
    let part = top.components.iter().find(|s| s.ui_id.as_deref() == Some(part)).expect(part);
    part.modifiers.iter().find(|m| m.param == param).map(|m| match &m.value {
        lsim_ir::ParamValue::Real(lsim_ir::Expr::Const(v)) => *v,
        v => panic!("{param}: {v:?}"),
    })
}

/// A car that starts at speed, as today's `start_at_vehicle_speed` sets it:
/// a clutch that a block commands has had no say at t = 0, so it starts
/// open, what only it ties to the wheels starts at rest and the clutch
/// starts slipping; a clutch that is unwired, or that a Constant engages,
/// starts closed and both its sides turn together.
#[test]
fn what_only_an_open_clutch_ties_to_the_wheels_starts_at_rest() {
    // 80 km/h on 0.31 m wheels, the final drive and first gear: about 9000 1/min
    let motor = hybrid_start(&|_| (), "el-motor", "w0").unwrap();
    assert!((850.0..1050.0).contains(&motor), "{motor}");
    // the hybrid's Script commands its clutch (engine on flange a)
    assert_eq!(hybrid_start(&|_| (), "el-engine", "w0"), Some(0.0));
    assert_eq!(hybrid_start(&|_| (), "el-clutch", "s0"), Some(-motor));
    // nothing commands it: engaged, the engine turns with the gearbox input
    let unwire = |p: &mut serde_json::Value| {
        let links = p["dataBusConnections"].as_array_mut().unwrap();
        links.retain(|l| l["port2Id"] != "sig_engage_in");
    };
    assert_eq!(hybrid_start(&unwire, "el-engine", "w0"), Some(motor));
    assert_eq!(hybrid_start(&unwire, "el-clutch", "s0"), None);
    // a Constant commands it: closed above 0, open at 0
    let constant = |value: f64| {
        move |p: &mut serde_json::Value| {
            p["systems"][0]["elements"].as_array_mut().unwrap().push(json!({
                "id": "el-k", "componentDefId": "signal.constant", "label": "Engage",
                "parameterOverrides": {"value": value}}));
            for l in p["dataBusConnections"].as_array_mut().unwrap() {
                if l["port2Id"] == "sig_engage_in" {
                    l["element1Id"] = json!("el-k");
                    l["port1Id"] = json!("sig_out");
                }
            }
        }
    };
    assert_eq!(hybrid_start(&constant(1.0), "el-engine", "w0"), Some(motor));
    assert_eq!(hybrid_start(&constant(0.0), "el-engine", "w0"), Some(0.0));
    // a clutch that can carry nothing holds nothing
    let no_torque = |p: &mut serde_json::Value| {
        unwire(p);
        for e in p["systems"][0]["elements"].as_array_mut().unwrap() {
            if e["id"] == "el-clutch" {
                e["parameterOverrides"]["max_torque_Nm"] = json!(0.0);
            }
        }
    };
    assert_eq!(hybrid_start(&no_torque, "el-engine", "w0"), Some(0.0));
}

/// An acceleration test of a car built for cycles: the Driver's Target and
/// Actual Speed inputs stay linked, and at full throttle it reads neither.
#[test]
fn an_acceleration_test_keeps_the_drivers_linked_inputs() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut p: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("../../../backend/projects/bev-car.json")).unwrap(),
    )
    .unwrap();
    p["cases"][0]["kind"] = json!("acceleration");
    let id = p["cases"][0]["id"].as_str().unwrap().to_string();
    let (top, rep) = import_case(&p, Some(&id), &standard_registry(), &ImportOptions::default())
        .unwrap_or_else(|e| panic!("{e:?}"));
    Model::build(&rep.library(), &top).unwrap();
}
