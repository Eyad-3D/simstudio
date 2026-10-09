//! Modes (DESIGN.md, *Events and modes*): relations of `if`, `abs` and
//! `sign` outside `noEvent` become discrete modes held between events, and
//! the models run to their exact answers across the switches.

mod common;

use common::*;
use lsim_ir::ComponentDef;
use lsim_ir::component::build::{connect, eq, param, port, state, sub, var};
use lsim_ir::expr::{Builtin, CmpOp, Expr, c, call, cmp, der, if_, name as n};
use lsim_prep::{Settings, prepare_with_report};

/// An RC circuit fed by a source that switches off at t_off: the source's
/// `if` is a mode, and the capacitor charges, then discharges, exactly.
#[test]
fn an_rc_circuit_follows_a_source_that_switches_off() {
    let (v0, r, cap, t_off) = (10.0, 10.0, 0.01, 0.5);
    let step = ComponentDef {
        name: "Test.StepVoltage".into(),
        ports: vec![port("p", "Pin", ""), port("n", "Pin", "")],
        params: vec![param("V", "V", v0, ""), param("t_off", "s", t_off, "")],
        vars: vec![var("v", "V", ""), var("i", "A", "")],
        equations: vec![
            eq(n("v"), n("p.v") - n("n.v"), "the voltage across it"),
            eq(c(0.0), n("p.i") + n("n.i"), "the current into p leaves at n"),
            eq(n("i"), n("p.i"), "its current"),
            eq(
                n("v"),
                if_(cmp(CmpOp::Lt, Expr::Time, n("t_off")), n("V"), c(0.0)),
                "it is on until t_off",
            ),
        ],
        ..Default::default()
    };
    let mut lib = library();
    lib.add(step);
    let top = ComponentDef {
        name: "Test.SwitchedRc".into(),
        components: vec![
            sub("src", "Test.StepVoltage", &[]),
            sub("r", "Electrical.Resistor", &[("R", c(r))]),
            sub("cap", "Electrical.Capacitor", &[("C", c(cap))]),
            sub("gnd", "Electrical.Ground", &[]),
        ],
        connections: vec![
            connect("src.p", "r.p"),
            connect("r.n", "cap.p"),
            connect("cap.n", "src.n"),
            connect("src.n", "gnd.p"),
        ],
        ..Default::default()
    };
    let (m, _) = prepare_with_report(&lib, &top, None, &Settings::default())
        .unwrap_or_else(|d| panic!("{d:#?}"));
    assert_eq!(m.modes.len(), 1, "the source's switch is a mode");
    let run = simulate(&m, 1.5, 0.01, 1e-10);
    let tau = r * cap;
    let charged = v0 * (1.0 - (-t_off / tau).exp());
    let exact = |t: f64| {
        if t < t_off { v0 * (1.0 - (-t / tau).exp()) } else { charged * (-(t - t_off) / tau).exp() }
    };
    let ch = run.channel("cap.v").unwrap();
    let mut err = 0.0f64;
    for (k, &t) in run.times.iter().enumerate() {
        if (t - t_off).abs() > 1e-9 {
            err = err.max((ch[k] - exact(t)).abs() / v0);
        }
    }
    println!("worst error {err:.1e}, events {:?}", run.events);
    assert!(err < 1e-8, "{err:e}");
    assert!(!run.events.is_empty() && run.events.iter().all(|e| e.t == run.events[0].t));
    assert!((run.events[0].t - t_off).abs() < 1e-12, "{}", run.events[0].t);
}

/// A flywheel driven by a constant torque against quadratic drag
/// `c w |w|`, starting backwards: the `abs` is a mode that flips as the
/// speed passes zero. Exact: w = k tan(s (t - t1)) before, k tanh(s (t -
/// t1)) after, k = √(T/c), s = √(T c)/J.
#[test]
fn quadratic_drag_flips_its_mode_as_the_speed_passes_zero() {
    let (j, torque, drag, w0) = (2.0, 4.0, 0.5, -3.0);
    let flywheel = ComponentDef {
        name: "Test.Flywheel".into(),
        params: vec![
            param("J", "kg.m2", j, ""),
            param("T", "N.m", torque, ""),
            param("c", "N.m.s2", drag, "quadratic drag"),
        ],
        vars: vec![state("w", "rad/s", w0, "speed")],
        equations: vec![eq(
            n("J") * der("w"),
            n("T") - n("c") * n("w") * call(Builtin::Abs, vec![n("w")]),
            "torque against quadratic drag",
        )],
        ..Default::default()
    };
    let mut lib = library();
    lib.add(flywheel);
    let top = ComponentDef {
        name: "Test.Drag".into(),
        components: vec![sub("fw", "Test.Flywheel", &[])],
        ..Default::default()
    };
    let (m, _) = prepare_with_report(&lib, &top, None, &Settings::default())
        .unwrap_or_else(|d| panic!("{d:#?}"));
    assert_eq!(m.modes.len(), 1, "abs is a mode");
    let run = simulate(&m, 5.0, 0.01, 1e-11);
    let k = (torque / drag).sqrt();
    let s = (torque * drag).sqrt() / j;
    let t1 = (-w0 / k).atan() / s;
    let exact = |t: f64| {
        if t < t1 { k * (s * (t - t1)).tan() } else { k * (s * (t - t1)).tanh() }
    };
    let err = worst(&run, "fw.w", exact);
    println!("worst error {err:.1e}; the speed passes zero at {t1}, events {:?}", run.events);
    assert!(err < 1e-8, "{err:e}");
    // one instant (the mode's when clause and its flip are both recorded)
    assert!(!run.events.is_empty() && run.events.iter().all(|e| e.t == run.events[0].t));
    assert!((run.events[0].t - t1).abs() < 1e-8, "{} vs {t1}", run.events[0].t);
}
