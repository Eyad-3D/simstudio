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
