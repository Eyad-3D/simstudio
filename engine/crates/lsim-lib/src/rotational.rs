//! Rotational primitives: inertia, damper, fixed frame, torque and speed
//! sources, ideal and lossy gears, Coulomb friction with stiction (brakes,
//! clutches), spring-damper, quadratic load, sensors.
//!
//! Sign convention: `tau` is the torque *into* the component through the
//! flange; a part that brakes a shaft turning at `w > 0` takes `tau > 0`.

use crate::x::*;
use lsim_ir::expr::Expr;
use lsim_ir::{CmpOp, ComponentDef, EnergyDecl, Equation, EquationDecl, WhenAction};

/// The friction mode logic shared by every Coulomb friction with stiction
/// (rotational and translational, to the frame or between two flanges).
///
/// The path-parameter formulation of Otter, Elmqvist and Mattsson (1993),
/// as Modelica's friction elements use it: the structure never changes;
/// one equation reads `0 = if stuck then a·[unit] else f − dir·f_c`, so
/// while stuck the relative acceleration `a` is zero and the friction
/// force `f` is whatever holds the parts together, and while sliding `f`
/// is the friction force against the motion. Mode changes are events:
///
/// * sliding → stuck when the relative speed `s` crosses zero in the
///   direction it was sliding (the conditions are on `s` alone, so a mode
///   change never re-triggers them);
/// * stuck → sliding when the holding force exceeds the friction force
///   (by `eps`, so a free part with no force on it stays put): it slides
///   the way that force pushes.
///
/// `s` is the relative speed, `a` its rate, `f` the force (torque) through
/// the element, `fc` the friction force's magnitude, `unit` a parameter of
/// value 1 that gives the acceleration the force's units.
fn friction_modes(s: &str, a: &str, f: &str, fc: &str, unit: &str) -> Vec<EquationDecl> {
    let stuck = || n("stuck");
    vec![
        eq(
            c(0.0),
            ite(gt(stuck(), c(0.5)), n(a) * n(unit), n(f) - n("dir") * n(fc)),
            "stuck, it holds (no relative acceleration); sliding, it takes the friction force \
             against the motion",
        ),
        EquationDecl {
            eq: Equation::When {
                condition: lsim_ir::expr::cmp(CmpOp::Lt, n(s), c(0.0)),
                actions: vec![WhenAction::Assign {
                    var: "stuck".into(),
                    value: ite(
                        and(gt(pre("dir"), c(0.0)), lt(pre("stuck"), c(0.5))),
                        c(1.0),
                        pre("stuck"),
                    ),
                }],
            },
            label: Some("it sticks when, sliding forwards, it comes to rest".into()),
        },
        EquationDecl {
            eq: Equation::When {
                condition: lsim_ir::expr::cmp(CmpOp::Gt, n(s), c(0.0)),
                actions: vec![WhenAction::Assign {
                    var: "stuck".into(),
                    value: ite(
                        and(lt(pre("dir"), c(0.0)), lt(pre("stuck"), c(0.5))),
                        c(1.0),
                        pre("stuck"),
                    ),
                }],
            },
            label: Some("it sticks when, sliding backwards, it comes to rest".into()),
        },
        when(
            gt(stuck() * (n(f) - n(fc) - n("eps")) + (stuck() - c(1.0)), c(0.0)),
            &[("stuck", c(0.0)), ("dir", c(1.0))],
            "it breaks away forwards when the holding force exceeds the friction force",
        ),
        when(
            gt(stuck() * (-n(f) - n(fc) - n("eps")) + (stuck() - c(1.0)), c(0.0)),
            &[("stuck", c(0.0)), ("dir", c(-1.0))],
            "it breaks away backwards when the holding force exceeds the friction force",
        ),
    ]
}

/// The discrete mode variables of a friction element, starting stuck when
/// its relative speed starts at zero.
fn friction_vars(s0: &str) -> Vec<lsim_ir::VarDecl> {
    let mut stuck = discrete("stuck", "1", 0.0, "1 while it holds, 0 while it slides");
    stuck.start = Some(ite(and(ge(n(s0), c(0.0)), le(n(s0), c(0.0))), c(1.0), c(0.0)));
    let mut dir = discrete("dir", "1", 1.0, "the direction it slides in: +1 or -1");
    dir.start = Some(ite(ge(n(s0), c(0.0)), c(1.0), c(-1.0)));
    vec![stuck, dir]
}

/// A Coulomb friction with stiction, to the fixed frame or between two
/// flanges, of the connector `conn` (`Flange` or `TFlange`).
pub fn friction(
    name: &str,
    doc: &str,
    conn: &str,
    two_sided: bool,
    with_input: bool,
) -> ComponentDef {
    let (across, through, au, fu, accu, unit_u) = match conn {
        "Flange" => ("w", "tau", "rad/s", "N.m", "rad/s2", "kg.m2"),
        _ => ("v", "f", "m/s", "N", "m/s2", "kg"),
    };
    let side = |p: &str, q: &str| format!("{p}.{q}");
    let mut ports = if two_sided {
        vec![
            port("a", conn, "side a (the friction force acts against its motion relative to b)"),
            port("b", conn, "side b"),
        ]
    } else {
        vec![port("flange", conn, "the part it acts on (against the fixed frame)")]
    };
    let mut params = vec![
        p("s0", au, 0.0, "the relative speed at the start (it starts stuck at 0)"),
        p(
            "eps",
            fu,
            1e-9,
            "how far the holding force must exceed the friction force to break away",
        ),
        p(
            "unit_m",
            unit_u,
            1.0,
            "unit carrier (value 1) giving the acceleration the force's units",
        ),
    ];
    if with_input {
        ports.push(input("fc", fu, "the friction force's magnitude (negative counts as 0)"));
    } else {
        params.push(p("f_max", fu, 0.0, "the friction force's magnitude"));
    }
    let fc_src = if with_input { n("fc") } else { n("f_max") };
    let mut vars = vec![
        state("s", au, 0.0, "relative speed (a - b, or of the part)"),
        var("acc", accu, "relative acceleration"),
        var("f_c", fu, "friction force magnitude, at least 0"),
    ];
    vars[0].start = Some(n("s0"));
    vars.extend(friction_vars("s0"));
    let (f_into, speed_eq) = if two_sided {
        (
            side("a", through),
            eq(n("s"), n(&side("a", across)) - n(&side("b", across)), "its slip speed is a - b"),
        )
    } else {
        (side("flange", through), eq(n("s"), n(&side("flange", across)), "it moves with the part"))
    };
    let mut equations = vec![
        speed_eq,
        eq(n("acc"), der("s"), "the slip speed's rate"),
        eq(n("f_c"), max(fc_src, c(0.0)), "the friction force's magnitude"),
    ];
    if two_sided {
        equations.push(eq(
            c(0.0),
            n(&side("a", through)) + n(&side("b", through)),
            "what it takes from a it gives to b",
        ));
    }
    equations.extend(friction_modes("s", "acc", &f_into, "f_c", "unit_m"));
    let loss = n(&f_into) * n("s");
    ComponentDef {
        name: name.into(),
        doc: doc.into(),
        ports,
        params,
        vars,
        equations,
        energy: EnergyDecl { stored: None, loss: Some(loss) },
        ..Default::default()
    }
}

/// Rotational primitives.
pub fn rotational() -> Vec<ComponentDef> {
    let inertia = ComponentDef {
        name: "Rotational.Inertia".into(),
        doc: "Rigid rotating mass; its speed is a state.".into(),
        ports: vec![port("a", "Flange", "one side"), port("b", "Flange", "the other side")],
        params: vec![
            p("J", "kg.m2", 1.0, "moment of inertia"),
            p("w0", "rad/s", 0.0, "the speed at the start"),
        ],
        vars: vec![{
            let mut w = state("w", "rad/s", 0.0, "speed");
            w.start = Some(n("w0"));
            w
        }],
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
        params: vec![p("d", "N.m.s/rad", 0.0, "damping")],
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
            p("tau_max", "N.m", 0.0, "torque once engaged"),
            p("w_on", "rad/s", 0.0, "the speed at which it engages"),
        ],
        vars: vec![discrete("engaged", "1", 0.0, "1 once engaged")],
        equations: vec![
            eq(n("flange.tau"), n("tau_max") * n("engaged"), "it takes its torque once engaged"),
            EquationDecl {
                eq: Equation::When {
                    condition: lsim_ir::expr::cmp(CmpOp::Ge, n("flange.w"), n("w_on")),
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
    let fixed = ComponentDef {
        name: "Rotational.Fixed".into(),
        doc: "The fixed frame: holds its flange still.".into(),
        ports: vec![port("flange", "Flange", "held at rest")],
        equations: vec![eq(n("flange.w"), c(0.0), "it does not turn")],
        ..Default::default()
    };
    let torque = ComponentDef {
        name: "Rotational.Torque".into(),
        doc: "Drives its flange with the torque it is given (reacting on the fixed frame).".into(),
        ports: vec![
            port("flange", "Flange", "the driven shaft"),
            input("tau", "N.m", "the torque"),
        ],
        equations: vec![eq(n("flange.tau"), -n("tau"), "it drives the shaft with tau")],
        ..Default::default()
    };
    let const_torque = ComponentDef {
        name: "Rotational.ConstantTorque".into(),
        doc: "Drives its flange with a constant torque.".into(),
        ports: vec![port("flange", "Flange", "the driven shaft")],
        params: vec![p("tau", "N.m", 0.0, "the torque")],
        equations: vec![eq(n("flange.tau"), -n("tau"), "it drives the shaft with tau")],
        ..Default::default()
    };
    let speed = ComponentDef {
        name: "Rotational.Speed".into(),
        doc: "Turns its flange at the speed it is given, with whatever torque that takes.".into(),
        ports: vec![port("flange", "Flange", "the driven shaft"), input("w", "rad/s", "the speed")],
        equations: vec![eq(n("flange.w"), n("w"), "the shaft turns at w")],
        ..Default::default()
    };
    let ideal_gear = ComponentDef {
        name: "Rotational.IdealGear".into(),
        doc: "Ideal gear: a turns ratio times as fast as b; lossless.".into(),
        ports: vec![port("a", "Flange", "input side"), port("b", "Flange", "output side")],
        params: vec![p("ratio", "1", 1.0, "a's speed / b's speed")],
        equations: vec![
            eq(n("a.w"), n("ratio") * n("b.w"), "a turns ratio times as fast as b"),
            eq(n("b.tau"), -(n("ratio") * n("a.tau")), "the torque is multiplied by the ratio"),
        ],
        ..Default::default()
    };
    let lossy = lossy_gear("Rotational.LossyGear", false);
    let rfric = friction(
        "Rotational.Friction",
        "Coulomb friction with stiction against the fixed frame (a brake): it holds the shaft \
         at rest until the torque on it exceeds the friction torque fc, and takes fc against \
         the motion while it turns. Mode changes are events located exactly (stick when the \
         speed reaches zero, break away when the holding torque exceeds fc).",
        "Flange",
        false,
        true,
    );
    let clutch = friction(
        "Rotational.Clutch",
        "Dry clutch: Coulomb friction with stiction between two shafts. Locked, the two turn \
         together and it carries whatever torque that takes, up to fc; slipping, it carries fc \
         against the slip. Lock-up and break-away are events.",
        "Flange",
        true,
        true,
    );
    let sensor = ComponentDef {
        name: "Rotational.SpeedSensor".into(),
        doc: "Measures its flange's speed; takes no torque.".into(),
        ports: vec![port("flange", "Flange", "the shaft"), output("w", "rad/s", "its speed")],
        equations: vec![
            eq(n("flange.tau"), c(0.0), "it takes no torque"),
            eq(n("w"), n("flange.w"), "it reads the speed"),
        ],
        ..Default::default()
    };
    let tsensor = ComponentDef {
        name: "Rotational.TorqueSensor".into(),
        doc: "Measures the torque passed from a to b; rigid.".into(),
        ports: vec![
            port("a", "Flange", "torque enters here"),
            port("b", "Flange", "and leaves here"),
            output("tau", "N.m", "the torque from a to b"),
        ],
        equations: vec![
            eq(n("a.w"), n("b.w"), "rigid"),
            eq(c(0.0), n("a.tau") + n("b.tau"), "it passes the torque on"),
            eq(n("tau"), n("a.tau"), "it reads the torque"),
        ],
        ..Default::default()
    };
    let spring = ComponentDef {
        name: "Rotational.SpringDamper".into(),
        doc: "Torsional spring and damper in parallel between two flanges (a compliant shaft); \
              its twist is a state."
            .into(),
        ports: vec![port("a", "Flange", "one end"), port("b", "Flange", "the other end")],
        params: vec![
            p("c", "N.m/rad", 1e5, "torsional stiffness"),
            p("d", "N.m.s/rad", 0.0, "torsional damping"),
            p("phi0", "rad", 0.0, "the twist at the start"),
        ],
        vars: vec![
            {
                let mut s = state("phi", "rad", 0.0, "twist, b relative to a");
                s.start = Some(n("phi0"));
                s
            },
            var("tau", "N.m", "the torque it carries from a to b"),
        ],
        equations: vec![
            eq(der("phi"), n("b.w") - n("a.w"), "it twists with the speed difference"),
            eq(
                n("tau"),
                n("c") * n("phi") + n("d") * (n("b.w") - n("a.w")),
                "the spring and damper torques",
            ),
            eq(n("b.tau"), n("tau"), "it holds b back"),
            eq(n("a.tau"), -n("tau"), "and pulls a along"),
        ],
        energy: EnergyDecl {
            stored: Some(c(0.5) * n("c") * n("phi") * n("phi")),
            loss: Some(n("d") * (n("b.w") - n("a.w")) * (n("b.w") - n("a.w"))),
        },
        ..Default::default()
    };
    let quad = ComponentDef {
        name: "Rotational.QuadraticLoad".into(),
        doc: "A load torque growing with the square of speed (fan, propeller): \
              tau = tau_ref·(w/w_ref)², against the motion."
            .into(),
        ports: vec![port("flange", "Flange", "the shaft")],
        params: vec![
            p("tau_ref", "N.m", 0.0, "the torque at the reference speed"),
            p("w_ref", "rad/s", 1.0, "the reference speed"),
        ],
        equations: vec![eq(
            n("flange.tau"),
            n("tau_ref") * n("flange.w") * abs(n("flange.w")) / (n("w_ref") * n("w_ref")),
            "the load grows with the square of speed",
        )],
        energy: EnergyDecl { stored: None, loss: Some(n("flange.tau") * n("flange.w")) },
        ..Default::default()
    };
    vec![
        inertia,
        damper,
        brake,
        fixed,
        torque,
        const_torque,
        speed,
        ideal_gear,
        lossy,
        rfric,
        clutch,
        sensor,
        tsensor,
        spring,
        quad,
    ]
}

/// A gear with an efficiency on the power that flows through it, in the
/// direction it flows (as today's gears: driving, b gets eta·P; flowing
/// back, a gets eta of what b gives). With `ratio_input`, the ratio is a
/// signal (a gearbox's selected gear).
pub fn lossy_gear(name: &str, ratio_input: bool) -> ComponentDef {
    let ratio = n("ratio");
    let fwd = ge(n("a.tau") * n("a.w"), c(0.0));
    let mut ports = vec![port("a", "Flange", "input side"), port("b", "Flange", "output side")];
    let mut params = vec![p("eta", "1", 1.0, "efficiency, in the direction the power flows")];
    if ratio_input {
        ports.push(input("ratio", "1", "a's speed / b's speed"));
    } else {
        params.insert(0, p("ratio", "1", 1.0, "a's speed / b's speed"));
    }
    ComponentDef {
        name: name.into(),
        doc: "Gear with a ratio and an efficiency on the power through it, in the direction it \
              flows: a turns ratio times as fast as b; driving (power into a), b gets eta of it; \
              flowing back, a gets eta of what b gives."
            .into(),
        ports,
        params,
        vars: vec![{
            let mut v = var("eta_dir", "1", "the torque factor for the present power direction");
            v.start = Some(n("eta"));
            v
        }],
        equations: vec![
            eq(n("a.w"), ratio.clone() * n("b.w"), "a turns ratio times as fast as b"),
            eq(n("eta_dir"), ite(fwd, n("eta"), c(1.0) / n("eta")), "the loss follows the power"),
            eq(
                n("b.tau"),
                -(ratio * n("eta_dir") * n("a.tau")),
                "the torque is multiplied by the ratio, less the loss",
            ),
        ],
        energy: EnergyDecl {
            stored: None,
            loss: Some(n("a.tau") * n("a.w") + n("b.tau") * n("b.w")),
        },
        ..Default::default()
    }
}

/// Builds `Expr` sums of a port's through variables (for tests).
pub fn through_sum(ports: &[&str], q: &str) -> Expr {
    sum(ports.iter().map(|p| n(&format!("{p}.{q}"))))
}
