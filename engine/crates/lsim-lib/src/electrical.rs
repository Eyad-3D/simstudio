//! Electrical primitives: ground, R, C, L, sources (constant and signal-
//! driven, voltage and current), power loads, the ideal DC machine.
//!
//! Sign convention: current is positive *into* a pin; a two-pin's `i` flows
//! from `p` to `n` through it and `v = p.v - n.v`, so `v·i` is the power it
//! takes from the circuit.

use crate::x::*;
use lsim_ir::{ComponentDef, EnergyDecl, VarDecl};

/// The two-pin pattern: ports p and n, voltage v = p.v - n.v, current i
/// from p to n through the component.
pub fn two_pin(name: &str, doc: &str, v_decl: VarDecl) -> ComponentDef {
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
    resistor.params.push(lsim_ir::component::build::param("R", "Ohm", 1.0, "resistance"));
    resistor.equations.push(eq(n("v"), n("R") * n("i"), "Ohm's law"));
    resistor.energy.loss = Some(n("v") * n("i"));

    let mut capacitor = two_pin(
        "Electrical.Capacitor",
        "Ideal capacitor; its voltage is a state.",
        state("v", "V", 0.0, "voltage across it"),
    );
    capacitor.params.push(lsim_ir::component::build::param("C", "F", 1.0, "capacitance"));
    capacitor.equations.push(eq(n("C") * der("v"), n("i"), "i = C dv/dt"));
    capacitor.energy.stored = Some(c(0.5) * n("C") * n("v") * n("v"));

    let mut inductor =
        two_pin("Electrical.Inductor", "Ideal inductor; its current is a state.", v());
    inductor.vars[1] = state("i", "A", 0.0, "current from p to n");
    inductor.params.push(lsim_ir::component::build::param("L", "H", 1.0, "inductance"));
    inductor.equations.push(eq(n("L") * der("i"), n("v"), "v = L di/dt"));
    inductor.energy.stored = Some(c(0.5) * n("L") * n("i") * n("i"));

    let mut source = two_pin(
        "Electrical.ConstantVoltage",
        "Ideal constant voltage source: v = V whatever the current.",
        v(),
    );
    source.params.push(lsim_ir::component::build::param("V", "V", 1.0, "source voltage"));
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
        params: vec![lsim_ir::component::build::param(
            "k",
            "N.m/A",
            1.0,
            "torque (and back-EMF) constant",
        )],
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

    let mut sig_v = two_pin(
        "Electrical.SignalVoltage",
        "Ideal voltage source set by a signal: v = v_in whatever the current.",
        v(),
    );
    sig_v.ports.push(input("v_in", "V", "the voltage to hold"));
    sig_v.equations.push(eq(n("v"), n("v_in"), "the source holds the voltage it is given"));

    let mut const_i = two_pin(
        "Electrical.ConstantCurrent",
        "Ideal constant current source: I flows from p to n through it whatever the voltage \
         (so it drives I out of p into the circuit when I < 0).",
        v(),
    );
    const_i.params.push(p("I", "A", 0.0, "current from p to n through the source"));
    const_i.equations.push(eq(n("i"), n("I"), "the source holds its current"));

    let mut sig_i = two_pin(
        "Electrical.SignalCurrent",
        "Ideal current source set by a signal: i = i_in from p to n through it.",
        v(),
    );
    sig_i.ports.push(input("i_in", "A", "the current to hold, from p to n"));
    sig_i.equations.push(eq(n("i"), n("i_in"), "the source holds the current it is given"));

    let mut load = two_pin(
        "Electrical.PowerLoad",
        "Takes the power it is given from the circuit, whatever the voltage: v·i = P_in (an \
         auxiliary load, an inverter seen from its DC side). The power leaves the network: its \
         books count it as used.",
        v(),
    );
    load.ports.push(input("P_in", "W", "the power it takes"));
    load.equations.push(eq(n("v") * n("i"), n("P_in"), "it takes the power it is given"));
    load.energy = EnergyDecl { stored: None, loss: Some(n("v") * n("i")) };

    let mut sig_r =
        two_pin("Electrical.VariableResistor", "A resistor whose resistance is a signal.", v());
    sig_r.ports.push(input("R_in", "Ohm", "its resistance"));
    sig_r.equations.push(eq(n("v"), n("R_in") * n("i"), "Ohm's law"));
    sig_r.energy.loss = Some(n("v") * n("i"));

    let vsensor = ComponentDef {
        name: "Electrical.VoltageSensor".into(),
        doc: "Measures the voltage between its pins; draws no current.".into(),
        ports: vec![
            port("p", "Pin", "positive terminal"),
            port("n", "Pin", "negative terminal"),
            output("v", "V", "p.v - n.v"),
        ],
        equations: vec![
            eq(n("p.i"), c(0.0), "it draws no current"),
            eq(n("n.i"), c(0.0), "it draws no current"),
            eq(n("v"), n("p.v") - n("n.v"), "it measures the voltage"),
        ],
        ..Default::default()
    };
    let isensor = ComponentDef {
        name: "Electrical.CurrentSensor".into(),
        doc: "Measures the current from p to n through it; no voltage across it.".into(),
        ports: vec![
            port("p", "Pin", "current flows in here"),
            port("n", "Pin", "and out here"),
            output("i", "A", "the current from p to n"),
        ],
        equations: vec![
            eq(n("p.v"), n("n.v"), "no voltage across it"),
            eq(c(0.0), n("p.i") + n("n.i"), "the current into p leaves at n"),
            eq(n("i"), n("p.i"), "it measures the current"),
        ],
        ..Default::default()
    };
    vec![
        ground, resistor, capacitor, inductor, source, emf, sig_v, const_i, sig_i, load, sig_r,
        vsensor, isensor,
    ]
}
