//! The connector types: across/through pairs and how their power is formed.

use lsim_ir::{ConnectorDef, PowerRule, QuantityDecl};

fn q(name: &str, unit: &str) -> QuantityDecl {
    QuantityDecl { name: name.into(), unit: unit.into() }
}

/// The connector types.
pub fn connectors() -> Vec<ConnectorDef> {
    vec![
        ConnectorDef {
            name: "Pin".into(),
            across: q("v", "V"),
            through: q("i", "A"),
            power: PowerRule::AcrossTimesThrough,
            doc: "Electrical terminal: potential v and current i into the component.".into(),
        },
        ConnectorDef {
            name: "Flange".into(),
            across: q("w", "rad/s"),
            through: q("tau", "N.m"),
            power: PowerRule::AcrossTimesThrough,
            doc: "Rotational flange: speed w and torque tau into the component (speed-based, \
                  so long runs carry no growing angle)."
                .into(),
        },
        ConnectorDef {
            name: "TFlange".into(),
            across: q("v", "m/s"),
            through: q("f", "N"),
            power: PowerRule::AcrossTimesThrough,
            doc: "Translational flange: velocity v and force f into the component.".into(),
        },
        ConnectorDef {
            name: "HeatPort".into(),
            across: q("T", "K"),
            through: q("Q", "W"),
            power: PowerRule::ThroughIsPower,
            doc: "Thermal port: temperature T and heat flow Q into the component.".into(),
        },
        ConnectorDef {
            name: "FuelPort".into(),
            across: q("e", "J/kg"),
            through: q("m_flow", "kg/s"),
            power: PowerRule::AcrossTimesThrough,
            doc: "Fuel line: the fuel's specific (heating-value) energy e and its mass flow \
                  m_flow into the component; their product is the chemical power that flows \
                  (tanks, engines, fuel cells)."
                .into(),
        },
    ]
}
