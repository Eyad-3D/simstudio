//! Translational primitives: mass, fixed frame, force sources, dampers,
//! Coulomb friction with stiction, quadratic drag, the ideal rolling wheel.
//!
//! `f` is the force *into* the component through the flange.

use crate::rotational::friction;
use crate::x::*;
use lsim_ir::{ComponentDef, EnergyDecl};

/// Translational primitives.
pub fn translational() -> Vec<ComponentDef> {
    let mass = ComponentDef {
        name: "Translational.Mass".into(),
        doc: "A rigid mass; its velocity is a state.".into(),
        ports: vec![port("a", "TFlange", "one side"), port("b", "TFlange", "the other side")],
        params: vec![p("m", "kg", 1.0, "mass"), p("v0", "m/s", 0.0, "the velocity at the start")],
        vars: vec![{
            let mut v = state("v", "m/s", 0.0, "velocity");
            v.start = Some(n("v0"));
            v
        }],
        equations: vec![
            eq(n("v"), n("a.v"), "side a moves with it"),
            eq(n("v"), n("b.v"), "side b moves with it"),
            eq(n("m") * der("v"), n("a.f") + n("b.f"), "m dv/dt is the net force"),
        ],
        energy: EnergyDecl { stored: Some(c(0.5) * n("m") * n("v") * n("v")), loss: None },
        ..Default::default()
    };
    let fixed = ComponentDef {
        name: "Translational.Fixed".into(),
        doc: "The fixed frame: holds its flange still.".into(),
        ports: vec![port("flange", "TFlange", "held at rest")],
        equations: vec![eq(n("flange.v"), c(0.0), "it does not move")],
        ..Default::default()
    };
    let force = ComponentDef {
        name: "Translational.Force".into(),
        doc: "Pushes its flange with the force it is given (reacting on the fixed frame).".into(),
        ports: vec![port("flange", "TFlange", "the pushed part"), input("f", "N", "the force")],
        equations: vec![eq(n("flange.f"), -n("f"), "it pushes with f")],
        ..Default::default()
    };
    let const_force = ComponentDef {
        name: "Translational.ConstantForce".into(),
        doc: "Pushes its flange with a constant force.".into(),
        ports: vec![port("flange", "TFlange", "the pushed part")],
        params: vec![p("f", "N", 0.0, "the force")],
        equations: vec![eq(n("flange.f"), -n("f"), "it pushes with f")],
        ..Default::default()
    };
    let damper = ComponentDef {
        name: "Translational.Damper".into(),
        doc: "Viscous loss to the fixed frame: force d·v against the velocity.".into(),
        ports: vec![port("flange", "TFlange", "the part")],
        params: vec![p("d", "N.s/m", 0.0, "damping")],
        equations: vec![eq(n("flange.f"), n("d") * n("flange.v"), "it takes force d·v")],
        energy: EnergyDecl { stored: None, loss: Some(n("d") * n("flange.v") * n("flange.v")) },
        ..Default::default()
    };
    let drag = ComponentDef {
        name: "Translational.QuadraticDrag".into(),
        doc: "Air drag to the fixed frame: ½·rho·CdA·v·|v| against the velocity.".into(),
        ports: vec![port("flange", "TFlange", "the part")],
        params: vec![
            p("cda", "m2", 0.0, "drag coefficient × frontal area"),
            p("rho", "kg/m3", 1.204, "air density"),
        ],
        equations: vec![eq(
            n("flange.f"),
            c(0.5) * n("rho") * n("cda") * n("flange.v") * abs(n("flange.v")),
            "air drag grows with the square of speed",
        )],
        energy: EnergyDecl { stored: None, loss: Some(n("flange.f") * n("flange.v")) },
        ..Default::default()
    };
    let fric = friction(
        "Translational.Friction",
        "Coulomb friction with stiction against the fixed frame: it holds the part at rest \
         until the force on it exceeds the friction force fc, and takes fc against the motion \
         while it moves (a vehicle's rolling resistance, a brake). Mode changes are events.",
        "TFlange",
        false,
        true,
    );
    let cfric = friction(
        "Translational.ConstantFriction",
        "Coulomb friction with stiction against the fixed frame, of constant magnitude f_max.",
        "TFlange",
        false,
        false,
    );
    let rolling = ComponentDef {
        name: "Translational.IdealRollingWheel".into(),
        doc: "A wheel that rolls without slip: v = radius·w, and the force on the ground is \
              the torque over the radius; lossless."
            .into(),
        ports: vec![
            port("flange", "Flange", "the axle"),
            port("road", "TFlange", "the contact with what it drives"),
        ],
        params: vec![p("radius", "m", 0.3, "rolling radius")],
        equations: vec![
            eq(n("road.v"), n("radius") * n("flange.w"), "it rolls without slip"),
            eq(n("flange.tau"), -(n("radius") * n("road.f")), "torque = radius × force"),
        ],
        ..Default::default()
    };
    let sensor = ComponentDef {
        name: "Translational.SpeedSensor".into(),
        doc: "Measures its flange's velocity; takes no force.".into(),
        ports: vec![port("flange", "TFlange", "the part"), output("v", "m/s", "its velocity")],
        equations: vec![
            eq(n("flange.f"), c(0.0), "it takes no force"),
            eq(n("v"), n("flange.v"), "it reads the velocity"),
        ],
        ..Default::default()
    };
    vec![mass, fixed, force, const_force, damper, drag, fric, cfric, rolling, sensor]
}
