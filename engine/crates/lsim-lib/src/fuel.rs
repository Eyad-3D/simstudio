//! Fuel primitives: a tank (a mass of fuel with its heating value) and a
//! consumer drawing a mass flow from it.
//!
//! A fuel port's `e` is the fuel's specific energy (its lower heating
//! value) and `m_flow` the mass flow into the component, so `e·m_flow` is
//! the chemical power that flows: the energy books see fuel the way they
//! see electricity.

use crate::x::*;
use lsim_ir::{ComponentDef, EnergyDecl};

/// Fuel primitives.
pub fn fuel() -> Vec<ComponentDef> {
    let tank = ComponentDef {
        name: "Fuel.Tank".into(),
        doc: "A fuel reservoir: its mass is a state; it gives fuel of heating value LHV. \
              It does not stop a draw when empty (a consumer reads its mass to stop)."
            .into(),
        ports: vec![port("fuel", "FuelPort", "the fuel line")],
        params: vec![
            pd("LHV", "J/kg", "MJ/kg", 42.9e6, "lower heating value"),
            p("m0", "kg", 0.0, "the fuel mass at the start"),
        ],
        vars: vec![{
            let mut m = state("m", "kg", 0.0, "fuel mass");
            m.start = Some(n("m0"));
            m
        }],
        equations: vec![
            eq(n("fuel.e"), n("LHV"), "the fuel carries its heating value"),
            eq(der("m"), n("fuel.m_flow"), "its mass changes by what flows in"),
        ],
        energy: EnergyDecl { stored: Some(n("m") * n("LHV")), loss: None },
        ..Default::default()
    };
    let sink = ComponentDef {
        name: "Fuel.Sink".into(),
        doc: "Draws the mass flow it is given from a fuel line and burns it: its chemical \
              power e·m_flow leaves the network."
            .into(),
        ports: vec![port("fuel", "FuelPort", "the fuel line"), input("m_flow", "kg/s", "the draw")],
        equations: vec![eq(n("fuel.m_flow"), n("m_flow"), "it draws what it is given")],
        energy: EnergyDecl { stored: None, loss: Some(n("fuel.e") * n("fuel.m_flow")) },
        ..Default::default()
    };
    vec![tank, sink]
}
