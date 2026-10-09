//! # lsim-lib: the component library in the engine's IR
//!
//! Connectors and the physical primitives every model is built from, then
//! composites made of them. Stage 1 holds what the end-to-end spike needs
//! (electrical and rotational primitives, an OCV + R0 + RC battery, a brake
//! that engages at a speed); work package 5 (DESIGN.md) grows it into the
//! full physical library and re-creates the 36 ready-made vehicle blocks of
//! `backend/app/library/components.json` on top of it.
//!
//! Sign convention: a port's through quantity (current, torque, force, heat
//! flow) is positive *into* the component.

use lsim_ir::component::build::{connect, discrete, eq, param, port, state, sub, var};
use lsim_ir::expr::{c, cmp, der, name as n};
use lsim_ir::{
    CmpOp, ComponentDef, ConnectorDef, EnergyDecl, Equation, EquationDecl, Library, PowerRule,
    QuantityDecl, VarDecl, WhenAction,
};

/// The connector types.
pub fn connectors() -> Vec<ConnectorDef> {
    let q = |name: &str, unit: &str| QuantityDecl { name: name.into(), unit: unit.into() };
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
    ]
}

/// The two-pin pattern: ports p and n, voltage v = p.v - n.v, current i
/// from p to n through the component.
fn two_pin(name: &str, doc: &str, v_decl: VarDecl) -> ComponentDef {
    ComponentDef {
        name: name.into(),
        doc: doc.into(),
        ports: vec![
            port("p", "Pin", "positive terminal (current flows in here)"),
            port("n", "Pin", "negative terminal"),
        ],
        vars: vec![v_decl, var("i", "A", "current from p to n")],
        equations: vec![
            eq(n("v"), n("p.v") - n("n.v"), "the voltage across it is p.v - n.v"),
            eq(c(0.0), n("p.i") + n("n.i"), "the current into p leaves at n"),
            eq(n("i"), n("p.i"), "its current is the current into p"),
        ],
        ..Default::default()
    }
}

/// Electrical primitives.
pub fn electrical() -> Vec<ComponentDef> {
    let v = || var("v", "V", "voltage across it");
    let ground = ComponentDef {
        name: "Electrical.Ground".into(),
        doc: "Zero potential.".into(),
        ports: vec![port("p", "Pin", "the ground terminal")],
        equations: vec![eq(n("p.v"), c(0.0), "ground is at 0 V")],
        ..Default::default()
    };

    let mut resistor = two_pin("Electrical.Resistor", "Ideal linear resistor.", v());
    resistor.params.push(param("R", "Ohm", 1.0, "resistance"));
    resistor.equations.push(eq(n("v"), n("R") * n("i"), "Ohm's law"));
    resistor.energy.loss = Some(n("v") * n("i"));

    let mut capacitor = two_pin(
        "Electrical.Capacitor",
        "Ideal capacitor; its voltage is a state.",
        state("v", "V", 0.0, "voltage across it"),
    );
    capacitor.params.push(param("C", "F", 1.0, "capacitance"));
    capacitor.equations.push(eq(n("C") * der("v"), n("i"), "i = C dv/dt"));
    capacitor.energy.stored = Some(c(0.5) * n("C") * n("v") * n("v"));

    let mut inductor =
        two_pin("Electrical.Inductor", "Ideal inductor; its current is a state.", v());
    inductor.vars[1] = state("i", "A", 0.0, "current from p to n");
    inductor.params.push(param("L", "H", 1.0, "inductance"));
    inductor.equations.push(eq(n("L") * der("i"), n("v"), "v = L di/dt"));
    inductor.energy.stored = Some(c(0.5) * n("L") * n("i") * n("i"));

    let mut source = two_pin(
        "Electrical.ConstantVoltage",
        "Ideal constant voltage source: v = V whatever the current.",
        v(),
    );
    source.params.push(param("V", "V", 1.0, "source voltage"));
    source.equations.push(eq(n("v"), n("V"), "the source holds its voltage"));

    let emf = ComponentDef {
        name: "Electrical.Emf".into(),
        doc: "Ideal DC machine (electromotive force): v = k·w and the flange torque is k·i; \
              lossless, either direction."
            .into(),
        ports: vec![
            port("p", "Pin", "positive terminal"),
            port("n", "Pin", "negative terminal"),
            port("flange", "Flange", "the shaft"),
        ],
        params: vec![param("k", "N.m/A", 1.0, "torque (and back-EMF) constant")],
        vars: vec![
            var("v", "V", "terminal voltage"),
            var("i", "A", "current from p to n"),
            var("w", "rad/s", "shaft speed"),
        ],
        equations: vec![
            eq(n("v"), n("p.v") - n("n.v"), "the voltage across it is p.v - n.v"),
            eq(c(0.0), n("p.i") + n("n.i"), "the current into p leaves at n"),
            eq(n("i"), n("p.i"), "its current is the current into p"),
            eq(n("w"), n("flange.w"), "the shaft turns with the flange"),
            eq(n("v"), n("k") * n("w"), "the back-EMF is k times the speed"),
            eq(
                n("flange.tau"),
                -(n("k") * n("i")),
                "it drives the flange with k times the current",
            ),
        ],
        ..Default::default()
    };
    vec![ground, resistor, capacitor, inductor, source, emf]
}

/// Rotational primitives.
pub fn rotational() -> Vec<ComponentDef> {
    let inertia = ComponentDef {
        name: "Rotational.Inertia".into(),
        doc: "Rigid rotating mass; its speed is a state.".into(),
        ports: vec![port("a", "Flange", "one side"), port("b", "Flange", "the other side")],
        params: vec![param("J", "kg.m2", 1.0, "moment of inertia")],
        vars: vec![state("w", "rad/s", 0.0, "speed")],
        equations: vec![
            eq(n("w"), n("a.w"), "side a turns with it"),
            eq(n("w"), n("b.w"), "side b turns with it"),
            eq(n("J") * der("w"), n("a.tau") + n("b.tau"), "J dw/dt is the net torque"),
        ],
        energy: EnergyDecl { stored: Some(c(0.5) * n("J") * n("w") * n("w")), loss: None },
        ..Default::default()
    };
    let damper = ComponentDef {
        name: "Rotational.Damper".into(),
        doc: "Viscous loss to the fixed frame: torque d·w against the speed.".into(),
        ports: vec![port("flange", "Flange", "the shaft")],
        params: vec![param("d", "N.m.s/rad", 0.0, "damping")],
        equations: vec![eq(n("flange.tau"), n("d") * n("flange.w"), "it takes torque d·w")],
        energy: EnergyDecl { stored: None, loss: Some(n("d") * n("flange.w") * n("flange.w")) },
        ..Default::default()
    };
    let brake = ComponentDef {
        name: "Rotational.ThresholdBrake".into(),
        doc: "A brake that clamps on, for good, the moment the speed first reaches w_on; then \
              it takes a constant torque tau_max (the spike's state event)."
            .into(),
        ports: vec![port("flange", "Flange", "the shaft")],
        params: vec![
            param("tau_max", "N.m", 0.0, "torque once engaged"),
            param("w_on", "rad/s", 0.0, "the speed at which it engages"),
        ],
        vars: vec![discrete("engaged", "1", 0.0, "1 once engaged")],
        equations: vec![
            eq(n("flange.tau"), n("tau_max") * n("engaged"), "it takes its torque once engaged"),
            EquationDecl {
                eq: Equation::When {
                    condition: cmp(CmpOp::Ge, n("flange.w"), n("w_on")),
                    actions: vec![WhenAction::Assign { var: "engaged".into(), value: c(1.0) }],
                },
                label: Some("it engages when the speed reaches w_on".into()),
            },
        ],
        energy: EnergyDecl {
            stored: None,
            loss: Some(n("tau_max") * n("engaged") * n("flange.w")),
        },
        ..Default::default()
    };
    vec![inertia, damper, brake]
}

/// Composites built from the primitives.
pub fn composites() -> Vec<ComponentDef> {
    let battery = ComponentDef {
        name: "Battery.OcvR0Rc".into(),
        doc: "Equivalent-circuit battery: constant open-circuit voltage, series resistance R0 \
              and one RC pair (R1 || C1)."
            .into(),
        ports: vec![port("p", "Pin", "positive terminal"), port("n", "Pin", "negative terminal")],
        params: vec![
            param("ocv", "V", 400.0, "open-circuit voltage"),
            param("r0", "Ohm", 0.05, "series resistance"),
            param("r1", "Ohm", 0.01, "RC pair resistance"),
            param("c1", "F", 0.01, "RC pair capacitance"),
        ],
        components: vec![
            sub("source", "Electrical.ConstantVoltage", &[("V", n("ocv"))]),
            sub("r0", "Electrical.Resistor", &[("R", n("r0"))]),
            sub("r1", "Electrical.Resistor", &[("R", n("r1"))]),
            sub("c1", "Electrical.Capacitor", &[("C", n("c1"))]),
        ],
        connections: vec![
            connect("n", "source.n"),
            connect("source.p", "r0.p"),
            connect("r0.n", "r1.p"),
            connect("r0.n", "c1.p"),
            connect("r1.n", "c1.n"),
            connect("r1.n", "p"),
        ],
        ..Default::default()
    };
    vec![battery]
}

/// The whole Stage 1 library.
pub fn library() -> Library {
    let mut lib = Library::default();
    for c in connectors() {
        lib.add_connector(c);
    }
    for c in electrical().into_iter().chain(rotational()).chain(composites()) {
        lib.add(c);
    }
    lib
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_is_consistent() {
        let lib = library();
        assert_eq!(lib.connectors.len(), 4);
        for def in lib.components.values() {
            for p in &def.ports {
                if let lsim_ir::PortKind::Physical { connector } = &p.kind {
                    assert!(lib.connectors.contains_key(connector), "{}: {connector}", def.name);
                }
            }
            for s in &def.components {
                assert!(lib.components.contains_key(&s.def), "{}: {}", def.name, s.def);
            }
            for prm in &def.params {
                let u = lsim_ir::units::parse_unit(&prm.unit).expect("parameter unit");
                assert_eq!((u.scale, u.offset), (1.0, 0.0), "{}.{} is not SI", def.name, prm.name);
            }
        }
    }
}
