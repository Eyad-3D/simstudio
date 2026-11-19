//! Thermal primitives: heat capacity, conductance, convection, fixed and
//! prescribed temperatures and heat flows.
//!
//! `Q` is the heat flow *into* the component through the port; a port's
//! power is the heat flow itself.

use crate::x::*;
use lsim_ir::{ComponentDef, EnergyDecl};

/// Thermal primitives.
pub fn thermal() -> Vec<ComponentDef> {
    let capacitor = ComponentDef {
        name: "Thermal.HeatCapacitor".into(),
        doc: "A lumped thermal mass; its temperature is a state.".into(),
        ports: vec![port("port", "HeatPort", "its surface")],
        params: vec![
            p("C", "J/K", 1.0, "heat capacity (mass × specific heat)"),
            pd("T0", "K", "degC", 293.15, "the temperature at the start"),
        ],
        vars: vec![{
            let mut t = state("T", "K", 293.15, "temperature");
            t.start = Some(n("T0"));
            t.nominal = Some(300.0);
            t
        }],
        equations: vec![
            eq(n("T"), n("port.T"), "its surface is at its temperature"),
            eq(n("C") * der("T"), n("port.Q"), "C dT/dt is the heat flowing in"),
        ],
        energy: EnergyDecl { stored: Some(n("C") * n("T")), loss: None },
        ..Default::default()
    };
    let conductor = ComponentDef {
        name: "Thermal.ThermalConductor".into(),
        doc: "Heat conduction between two ports: Q = G·(a.T - b.T) from a to b.".into(),
        ports: vec![port("a", "HeatPort", "one side"), port("b", "HeatPort", "the other side")],
        params: vec![p("G", "W/K", 1.0, "thermal conductance")],
        vars: vec![var("Q", "W", "heat flow from a to b")],
        equations: vec![
            eq(n("Q"), n("G") * (n("a.T") - n("b.T")), "heat flows down the temperature"),
            eq(n("a.Q"), n("Q"), "it takes Q from a"),
            eq(n("b.Q"), -n("Q"), "and gives it to b"),
        ],
        ..Default::default()
    };
    let convection = ComponentDef {
        name: "Thermal.Convection".into(),
        doc: "Convective heat transfer with a conductance given as a signal: \
              Q = Gc·(solid.T - fluid.T)."
            .into(),
        ports: vec![
            port("solid", "HeatPort", "the surface"),
            port("fluid", "HeatPort", "the fluid"),
            input("Gc", "W/K", "convective conductance (h·A)"),
        ],
        vars: vec![var("Q", "W", "heat flow from the solid to the fluid")],
        equations: vec![
            eq(n("Q"), n("Gc") * (n("solid.T") - n("fluid.T")), "heat flows to the fluid"),
            eq(n("solid.Q"), n("Q"), "it takes Q from the solid"),
            eq(n("fluid.Q"), -n("Q"), "and gives it to the fluid"),
        ],
        ..Default::default()
    };
    let fixed_t = ComponentDef {
        name: "Thermal.FixedTemperature".into(),
        doc: "A boundary at a fixed temperature (ambient air, a coolant held at temperature)."
            .into(),
        ports: vec![port("port", "HeatPort", "held at T")],
        params: vec![pd("T", "K", "degC", 293.15, "its temperature")],
        equations: vec![eq(n("port.T"), n("T"), "it holds its temperature")],
        ..Default::default()
    };
    let pres_t = ComponentDef {
        name: "Thermal.PrescribedTemperature".into(),
        doc: "A boundary at the temperature it is given.".into(),
        ports: vec![port("port", "HeatPort", "held at T"), input("T", "K", "the temperature")],
        equations: vec![eq(n("port.T"), n("T"), "it holds the temperature it is given")],
        ..Default::default()
    };
    let fixed_q = ComponentDef {
        name: "Thermal.FixedHeatFlow".into(),
        doc: "Puts a fixed heat flow into what it is connected to.".into(),
        ports: vec![port("port", "HeatPort", "where the heat goes")],
        params: vec![p("Q_flow", "W", 0.0, "the heat flow it puts in")],
        equations: vec![eq(n("port.Q"), -n("Q_flow"), "it puts Q_flow in")],
        ..Default::default()
    };
    let pres_q = ComponentDef {
        name: "Thermal.PrescribedHeatFlow".into(),
        doc: "Puts the heat flow it is given into what it is connected to (a loss turned \
              into heat)."
            .into(),
        ports: vec![
            port("port", "HeatPort", "where the heat goes"),
            input("Q_flow", "W", "the heat flow"),
        ],
        equations: vec![eq(n("port.Q"), -n("Q_flow"), "it puts Q_flow in")],
        ..Default::default()
    };
    let sensor = ComponentDef {
        name: "Thermal.TemperatureSensor".into(),
        doc: "Measures a port's temperature; takes no heat.".into(),
        ports: vec![
            port("port", "HeatPort", "the measured point"),
            output("T", "K", "its temperature"),
        ],
        equations: vec![
            eq(n("port.Q"), c(0.0), "it takes no heat"),
            eq(n("T"), n("port.T"), "it reads the temperature"),
        ],
        ..Default::default()
    };
    vec![capacitor, conductor, convection, fixed_t, pres_t, fixed_q, pres_q, sensor]
}
