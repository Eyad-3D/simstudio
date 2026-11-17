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

/// Today's fuel-cell range extender without its driveline: a fuel cell on
/// its own bus, a DC-DC converter delivering `setpoint_kw` into the bus of
/// a battery at `soc_pct` (setpoint mode), and a 10 kW Power Consumer on
/// that bus.
fn range_extender(setpoint_kw: f64, soc_pct: f64) -> serde_json::Value {
    let wire = |id: &str, a: &str, pa: &str, b: &str, pb: &str| {
        json!({"id": id, "sourceElementId": a, "sourcePortId": pa,
               "targetElementId": b, "targetPortId": pb})
    };
    json!({
        "id": "rex", "name": "Range extender",
        "systems": [{"id": "sys-root", "name": "Range extender", "parentId": null,
            "elements": [
                {"id": "el-fc", "componentDefId": "fuelcell.stack", "label": "Fuel Cell"},
                {"id": "el-h2", "componentDefId": "fuel.h2_tank", "label": "H2 Tank"},
                {"id": "el-fcbus", "componentDefId": "electric.node", "label": "FC Bus"},
                {"id": "el-dc", "componentDefId": "controller.dcdc", "label": "DC-DC",
                 "parameterOverrides": {"power_setpoint_kW": setpoint_kw}},
                {"id": "el-bat", "componentDefId": "battery.generic", "label": "Battery",
                 "parameterOverrides": {"initial_soc_pct": soc_pct}},
                {"id": "el-hvbus", "componentDefId": "electric.node", "label": "HV Bus"},
                {"id": "el-load", "componentDefId": "electric.constant_drive", "label": "Load",
                 "parameterOverrides": {"power_kW": 10.0}}
            ],
            "connections": [
                wire("w1", "el-fc", "pos", "el-fcbus", "t1"),
                wire("w2", "el-fcbus", "t2", "el-dc", "a_pos"),
                wire("w3", "el-dc", "b_pos", "el-hvbus", "t1"),
                wire("w4", "el-bat", "pos", "el-hvbus", "t2"),
                wire("w5", "el-load", "pos", "el-hvbus", "t3")
            ]}],
        "dataBusConnections": [],
        "cases": [{"id": "c1", "name": "A minute", "duration": 60, "timeStep": 1}]
    })
}

fn linked(top: &lsim_ir::ComponentDef, a: &str, b: &str) -> bool {
    top.connections.iter().any(|c| (c.a == a && c.b == b) || (c.a == b && c.b == a))
}

/// The bus manager linked to `end`.
fn manager_at(top: &lsim_ir::ComponentDef, end: &str) -> String {
    let other = top
        .connections
        .iter()
        .find_map(|c| match () {
            _ if c.a == end => Some(c.b.clone()),
            _ if c.b == end => Some(c.a.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{end} is linked to nothing"));
    other.split('.').next().unwrap().to_string()
}

/// A DC-DC converter feeding a battery's bus delivers its power setpoint
/// (today's setpoint mode): on its input bus it is one more load, served
/// first with the consumers and cut back with them; the battery bus's
/// window counts what it brings in; it throttles back to what the battery
/// bus takes (its loads plus the battery's charge window).
#[test]
fn a_dc_dc_feeding_a_battery_bus_delivers_its_setpoint() {
    let reg = standard_registry();
    let (top, rep) = import(&range_extender(25.0, 60.0), &reg).unwrap_or_else(|e| panic!("{e:?}"));
    // the input bus counts it as a load and tells it the share it is served
    let fc_bus = manager_at(&top, "el_dc.p_dem");
    assert!(linked(&top, "el_fc.p_deliver", &format!("{fc_bus}.p_deliver")));
    assert!(linked(&top, &format!("{fc_bus}.served"), "el_dc.served"));
    // the battery bus's window knows the inflow; what feeds its absorb
    let hv_bus = manager_at(&top, "el_dc.p_set");
    assert_ne!(fc_bus, hv_bus);
    assert!(linked(&top, "el_dc.p_set", &format!("{hv_bus}.p_in1")));
    assert!(linked(&top, &format!("{hv_bus}.absorb1"), "el_dc.absorb"));
    assert!(linked(&top, "el_bat.p_absorb", &format!("{hv_bus}.p_absorb")));
    assert!(linked(&top, "el_load.sig_power", &format!("{hv_bus}.p_use1")));
    assert!(rep.warnings.iter().all(|w| w.code != "DCDC-WINDOW"), "{:?}", rep.warnings);
    let run = |sp: f64, soc: f64| {
        let (top, rep) = import(&range_extender(sp, soc), &reg).unwrap();
        let model = Model::build(&rep.library(), &top).unwrap_or_else(|e| panic!("{e:?}"));
        model
            .run(&SolverOptions::default(), OutputGrid { t0: 0.0, t_end: 60.0, dt: 1.0 }, &mut [])
            .unwrap_or_else(|e| panic!("{e:?}"))
    };
    let at = |r: &lsim_solve::SimResult, ch: &str| r.channel(ch).unwrap()[30];
    // 25 kW: the load takes 10, the battery the rest; the fuel cell gives 25/0.97
    let r = run(25.0, 60.0);
    assert!((at(&r, "el_dc.sig_power_out") - 25e3).abs() < 1e-3);
    assert!((at(&r, "el_dc.sig_power_in") - 25e3 / 0.97).abs() < 1e-3);
    assert!((at(&r, "el_bat.sig_power") + 15e3).abs() < 1e-3);
    assert!((at(&r, "el_fc.sig_power") - 25e3 / 0.97).abs() < 1e-3);
    // 120 kW is more than the fuel cell's 100 kW gives: cut back to it
    let r = run(120.0, 60.0);
    assert!((at(&r, "el_fc.sig_power") - 100e3).abs() < 1e-3, "{}", at(&r, "el_fc.sig_power"));
    assert!((at(&r, "el_dc.sig_power_out") - 97e3).abs() < 1e-3);
    // a battery full: it delivers only what the load uses
    let r = run(25.0, 100.0);
    assert!((at(&r, "el_dc.sig_power_out") - 10e3).abs() < 1e-3);
    assert!(at(&r, "el_bat.sig_power").abs() < 1e-3);
}

/// Two converters at their setpoints (20 and 10 kW) from a voltage source
/// into a full battery's bus: they share what its 10 kW load takes in
/// proportion to their setpoints, as today's bus balance shares it; a
/// voltage source serves them in full.
#[test]
fn two_dc_dcs_share_what_their_battery_bus_takes() {
    let mut p = range_extender(20.0, 100.0);
    let els = p["systems"][0]["elements"].as_array_mut().unwrap();
    els.retain(|e| e["id"] != "el-fc" && e["id"] != "el-h2");
    els.push(
        json!({"id": "el-vs", "componentDefId": "electric.voltage_source", "label": "Supply"}),
    );
    els.push(json!({"id": "el-dc2", "componentDefId": "controller.dcdc", "label": "DC-DC 2",
                    "parameterOverrides": {"power_setpoint_kW": 10.0}}));
    let wires = p["systems"][0]["connections"].as_array_mut().unwrap();
    wires[0]["sourceElementId"] = json!("el-vs");
    for (id, a, pa, b, pb) in
        [("w6", "el-fcbus", "t3", "el-dc2", "a_pos"), ("w7", "el-dc2", "b_pos", "el-hvbus", "t4")]
    {
        wires.push(json!({"id": id, "sourceElementId": a, "sourcePortId": pa,
                          "targetElementId": b, "targetPortId": pb}));
    }
    let reg = standard_registry();
    let (top, rep) = import(&p, &reg).unwrap_or_else(|e| panic!("{e:?}"));
    let model = Model::build(&rep.library(), &top).unwrap_or_else(|e| panic!("{e:?}"));
    let r = model
        .run(&SolverOptions::default(), OutputGrid { t0: 0.0, t_end: 60.0, dt: 1.0 }, &mut [])
        .unwrap_or_else(|e| panic!("{e:?}"));
    let at = |ch: &str| r.channel(ch).unwrap()[30];
    assert!((at("el_dc.sig_power_out") - 20e3 / 3.0).abs() < 1e-3, "{}", at("el_dc.sig_power_out"));
    assert!((at("el_dc2.sig_power_out") - 10e3 / 3.0).abs() < 1e-3);
    assert!(at("el_bat.sig_power").abs() < 1e-3);
}

/// A converter at its setpoint whose input is left open, or that another
/// converter feeds (a chain), has no source of its own to cut its setpoint
/// back: refused, saying what to connect.
#[test]
fn a_dc_dc_at_its_setpoint_with_no_source_of_its_own_is_refused() {
    let reg = standard_registry();
    let refused = |p: &serde_json::Value| {
        let e = import(p, &reg).unwrap_err();
        assert!(e[0].message.contains("'DC-DC'"), "{}", e[0].message);
        assert!(e[0].message.contains("connect its input"), "{}", e[0].message);
    };
    let mut open = range_extender(25.0, 60.0);
    open["systems"][0]["connections"].as_array_mut().unwrap().retain(|w| w["id"] != "w2");
    refused(&open);
    // the fuel cell behind a converter holding 350 V on the setpoint converter's input bus
    let mut chain = range_extender(25.0, 60.0);
    chain["systems"][0]["elements"].as_array_mut().unwrap().extend([
        json!({"id": "el-dc0", "componentDefId": "controller.dcdc", "label": "Boost"}),
        json!({"id": "el-mid", "componentDefId": "electric.node", "label": "Mid Bus"}),
    ]);
    let wires = chain["systems"][0]["connections"].as_array_mut().unwrap();
    wires[1]["sourceElementId"] = json!("el-mid");
    for (id, a, pa, b, pb) in
        [("w8", "el-fcbus", "t3", "el-dc0", "a_pos"), ("w9", "el-dc0", "b_pos", "el-mid", "t1")]
    {
        wires.push(json!({"id": id, "sourceElementId": a, "sourcePortId": pa,
                          "targetElementId": b, "targetPortId": pb}));
    }
    refused(&chain);
}
