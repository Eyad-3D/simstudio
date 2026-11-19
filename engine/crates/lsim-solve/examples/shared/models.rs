//! A mid-size model for the solver's measurements: a sinusoidal supply
//! feeds an RC ladder (`N` stages), a DC machine and a rotor with viscous
//! loss; stiff (the ladder's fast poles), driven all the time (so the
//! integrator's natural steps stay moderate, as in a drive cycle), built
//! from the Stage 1 library plus a sinusoidal source written here.

use lsim_ir::component::build::{connect, eq, param, port, sub, var};
use lsim_ir::expr::{c, call, name as n};
use lsim_ir::{Builtin, ComponentDef, Library};

/// v = v0 + a sin(2π f t)
pub fn sine_source() -> ComponentDef {
    ComponentDef {
        name: "Test.SineVoltage".into(),
        doc: "A sinusoidal voltage source.".into(),
        ports: vec![port("p", "Pin", ""), port("n", "Pin", "")],
        params: vec![
            param("v0", "V", 300.0, ""),
            param("a", "V", 100.0, ""),
            param("f", "1/s", 0.05, ""),
        ],
        vars: vec![var("v", "V", ""), var("i", "A", "")],
        equations: vec![
            eq(n("v"), n("p.v") - n("n.v"), "the voltage across it"),
            eq(c(0.0), n("p.i") + n("n.i"), "the current passes through"),
            eq(n("i"), n("p.i"), "its current"),
            eq(
                n("v"),
                n("v0")
                    + n("a")
                        * call(
                            Builtin::Sin,
                            vec![c(2.0 * std::f64::consts::PI) * n("f") * lsim_ir::Expr::Time],
                        ),
                "it holds its voltage",
            ),
        ],
        ..Default::default()
    }
}

pub fn library() -> Library {
    let mut lib = lsim_lib::library();
    lib.add(sine_source());
    lib
}

/// The ladder drive with `stages` RC stages.
pub fn ladder_drive(stages: usize) -> ComponentDef {
    let mut components = vec![
        sub("src", "Test.SineVoltage", &[]),
        sub("gnd", "Electrical.Ground", &[]),
        sub("motor", "Electrical.Emf", &[("k", c(1.0))]),
        sub("rotor", "Rotational.Inertia", &[("J", c(5.0))]),
        sub("loss", "Rotational.Damper", &[("d", c(0.5))]),
    ];
    let mut connections = vec![
        connect("src.n", "gnd.p"),
        connect("motor.n", "gnd.p"),
        connect("motor.flange", "rotor.a"),
        connect("rotor.b", "loss.flange"),
    ];
    let mut prev = "src.p".to_string();
    for k in 0..stages {
        components.push(sub(&format!("r{k}"), "Electrical.Resistor", &[("R", c(0.01))]));
        components.push(sub(&format!("c{k}"), "Electrical.Capacitor", &[("C", c(0.05))]));
        connections.push(connect(&prev, &format!("r{k}.p")));
        connections.push(connect(&format!("r{k}.n"), &format!("c{k}.p")));
        connections.push(connect(&format!("c{k}.n"), "gnd.p"));
        prev = format!("r{k}.n");
    }
    components.push(sub("rm", "Electrical.Resistor", &[("R", c(0.1))]));
    connections.push(connect(&prev, "rm.p"));
    connections.push(connect("rm.n", "motor.p"));
    ComponentDef { name: "Test.LadderDrive".into(), components, connections, ..Default::default() }
}
