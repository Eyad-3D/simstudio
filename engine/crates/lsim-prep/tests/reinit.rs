//! `reinit`: states restarted at events, against exact answers.
//!
//! Preparation rewrites `reinit(x, v)` as a jump `x.jump := v -
//! x.continuous` (a discrete assignment, which the run loop applies after
//! an event like any other), with `x = x.continuous + x.jump`.

mod common;

use common::*;
use lsim_ir::component::build::{connect, discrete, eq, param, port, state, sub};
use lsim_ir::expr::{Builtin, CmpOp, Expr, c, call, cmp, der, name as n};
use lsim_ir::{ComponentDef, Equation, EquationDecl, WhenAction};
use lsim_prep::{Settings, prepare_with_report};
use lsim_solve::{OutputGrid, RunInfo, SolverOptions};

fn when(condition: Expr, actions: Vec<WhenAction>, label: &str) -> EquationDecl {
    EquationDecl { eq: Equation::When { condition, actions }, label: Some(label.into()) }
}

fn reinit(var: &str, value: Expr) -> WhenAction {
    WhenAction::Reinit { var: var.into(), value }
}

fn pre(var: &str) -> Expr {
    call(Builtin::Pre, vec![n(var)])
}

fn assign(var: &str, value: Expr) -> WhenAction {
    WhenAction::Assign { var: var.into(), value }
}

// benchmarks/reference/problems/mech_gear_change.toml
const J1: f64 = 0.05;
const J2: f64 = 135.0;
const T: f64 = 200.0;
const I1: f64 = 12.0;
const I2: f64 = 7.0;
const T_SHIFT: f64 = 4.0;

/// A gear whose ratio steps from `i1` to `i2` at `t_shift`, engaging like
/// an ideal dog clutch: the impulse of the engagement keeps
/// `J2 ω2 + i2 J1 ω1`, so the load's speed restarts at
/// `(J2 + i1 i2 J1) ω2 / (J2 + i2² J1)` (the shafts' inertias are its
/// parameters, as a shift model needs them).
fn shifting_gear() -> ComponentDef {
    ComponentDef {
        name: "Test.ShiftingGear".into(),
        ports: vec![port("a", "Flange", "motor side"), port("b", "Flange", "load side")],
        params: vec![
            param("i1", "1", I1, "first gear"),
            param("i2", "1", I2, "second gear"),
            param("t_shift", "s", T_SHIFT, "when it shifts"),
            param("J1", "kg.m2", J1, "the motor side's inertia"),
            param("J2", "kg.m2", J2, "the load side's inertia"),
        ],
        vars: vec![discrete("i", "1", I1, "the engaged ratio a.w / b.w")],
        equations: vec![
            eq(n("a.w"), n("i") * n("b.w"), "a turns i times as fast as b"),
            eq(c(0.0), n("i") * n("a.tau") + n("b.tau"), "the power through it is kept"),
            when(
                cmp(CmpOp::Gt, Expr::Time, n("t_shift")),
                vec![
                    assign("i", n("i2")),
                    reinit(
                        "b.w",
                        // the ratio before the shift: pre(i) (a when
                        // clause reads its own new values otherwise)
                        (n("J2") + pre("i") * n("i2") * n("J1"))
                            / (n("J2") + n("i2") * n("i2") * n("J1"))
                            * n("b.w"),
                    ),
                ],
                "it shifts, keeping the angular momentum",
            ),
        ],
        ..Default::default()
    }
}

fn gear_change() -> ComponentDef {
    ComponentDef {
        name: "Test.GearChange".into(),
        components: vec![
            sub("torque", "Rotational.ConstantTorque", &[("tau", c(T))]),
            sub("rotor", "Rotational.Inertia", &[("J", c(J1))]),
            sub("gear", "Test.ShiftingGear", &[]),
            sub("load", "Rotational.Inertia", &[("J", c(J2))]),
        ],
        connections: vec![
            connect("torque.flange", "rotor.a"),
            connect("rotor.b", "gear.a"),
            connect("gear.b", "load.a"),
        ],
        ..Default::default()
    }
}

/// The exact answer: the load's speed (right limit at the shift).
fn omega2(t: f64) -> f64 {
    let before = |t: f64| T * I1 * t / (J2 + I1 * I1 * J1);
    if t < T_SHIFT {
        before(t)
    } else {
        let after = (J2 + I1 * I2 * J1) * before(T_SHIFT) / (J2 + I2 * I2 * J1);
        after + T * I2 * (t - T_SHIFT) / (J2 + I2 * I2 * J1)
    }
}

fn omega1(t: f64) -> f64 {
    (if t < T_SHIFT { I1 } else { I2 }) * omega2(t)
}

#[test]
fn a_gear_change_keeps_the_angular_momentum_and_loses_the_exact_energy() {
    let mut lib = library();
    lib.add(shifting_gear());
    let (m, report) = prepare_with_report(&lib, &gear_change(), None, &Settings::default())
        .unwrap_or_else(|d| panic!("{d:#?}"));
    println!("{report:#?}");
    let names: Vec<&str> = m.states.iter().map(|v| m.flat.var(*v).name.as_str()).collect();
    assert_eq!(names, ["load.w.continuous"], "the restarted speed's continuous part is the state");
    assert_eq!(m.whens.len(), 1);
    assert_eq!(m.whens[0].assign.len(), 2, "the new ratio and the speed's jump");

    let jit = lsim_codegen::compile(&m, &Default::default()).expect("compiles");
    let info = RunInfo::from_prepared(&m);
    let opts = SolverOptions { rtol: 1e-10, atol: 1e-10, energy_books: true, ..Default::default() };
    let run = lsim_solve::simulate(
        &jit,
        &info,
        &opts,
        OutputGrid { t0: 0.0, t_end: 8.0, dt: 0.01 },
        &mut [],
    )
    .expect("runs");

    // the reference problem's checkpoints, to the last digits
    let checkpoints = [2.0, 4.0, 6.0, 8.0];
    let w2_ref = [33.755274261603375, 68.37008624540108, 88.74113026140691, 109.11217427741272];
    let w1_ref = [405.0632911392405, 478.5906037178076, 621.1879118298484, 763.7852199418892];
    let e_kin_ref = [81012.65822784811, 321252.8609404162, 541208.5640499474, 818203.1904042948];
    let (w1, w2) = (run.channel("rotor.w").unwrap(), run.channel("load.w").unwrap());
    // the shift is located by root finding: within rounding of t_shift
    assert_eq!(run.events.len(), 1, "{:?}", run.events);
    let t_event = run.events[0].t;
    println!("the shift at t = {t_event} (t_shift + {:.1e})", t_event - T_SHIFT);
    assert!((t_event - T_SHIFT).abs() < 1e-12, "{t_event}");
    let mut worst_rel = 0.0f64;
    for (j, &tc) in checkpoints.iter().enumerate() {
        let k = run.times.iter().position(|&t| (t - tc).abs() < 1e-9).expect("a grid point");
        let e_kin = 0.5 * J1 * w1[k] * w1[k] + 0.5 * J2 * w2[k] * w2[k];
        // the closed form agrees with the reference's digits
        assert!((omega2(tc) - w2_ref[j]).abs() < 1e-12 * w2_ref[j]);
        assert!((omega1(tc) - w1_ref[j]).abs() < 1e-12 * w1_ref[j]);
        // the reference takes the right limit at the shift; a grid point
        // the located event falls just after shows the left limit
        let (w2_want, w1_want, e_want) = if run.times[k] < t_event && tc == T_SHIFT {
            let w2m = T * I1 * T_SHIFT / (J2 + I1 * I1 * J1);
            let w1m = I1 * w2m;
            (w2m, w1m, 0.5 * J1 * w1m * w1m + 0.5 * J2 * w2m * w2m)
        } else {
            (w2_ref[j], w1_ref[j], e_kin_ref[j])
        };
        println!(
            "t = {tc}: omega2 {} (exact {w2_want}), omega1 {} (exact {w1_want}), E_kin {e_kin} \
             (exact {e_want})",
            w2[k], w1[k]
        );
        for (got, want) in [(w2[k], w2_want), (w1[k], w1_want), (e_kin, e_want)] {
            worst_rel = worst_rel.max((got - want).abs() / want);
        }
    }
    println!("worst relative error at the checkpoints: {worst_rel:.1e}");
    assert!(worst_rel < 1e-8, "{worst_rel:e}");
    // and everywhere on the grid (away from the shift instant itself)
    let mut worst_grid = 0.0f64;
    for (k, &t) in run.times.iter().enumerate() {
        if (t - T_SHIFT).abs() > 1e-9 {
            // (the closed forms switch at t_shift itself)
            worst_grid = worst_grid.max((w2[k] - omega2(t)).abs() / omega2(8.0));
            worst_grid = worst_grid.max((w1[k] - omega1(t)).abs() / omega1(8.0));
        }
    }
    println!("worst error on the grid: {worst_grid:.1e}");
    assert!(worst_grid < 1e-8, "{worst_grid:e}");

    // the energy books: the shift loses exactly E_shift
    let e_shift = 2797.7719709762023;
    if let Some(books) = &run.energy {
        println!("{}", books.summary());
        println!("event loss {} (exact {e_shift})", books.event_loss);
        assert!((books.event_loss - e_shift).abs() < 1e-6 * e_shift, "{}", books.event_loss);
        assert!(books.relative_closure < 1e-8, "{}", books.relative_closure);
    } else {
        // the kinetic energy just before and just after the shift
        let w2m = T * I1 * T_SHIFT / (J2 + I1 * I1 * J1);
        let lost = 0.5 * (J2 + I1 * I1 * J1) * w2m * w2m
            - 0.5 * (J2 + I2 * I2 * J1) * omega2(T_SHIFT) * omega2(T_SHIFT);
        assert!((lost - e_shift).abs() < 1e-9 * e_shift);
    }
}

/// A ball dropped from 1 m bounces with restitution e: each impact
/// restarts its speed at -e times the speed it hit with. Exact: the n-th
/// impact at t1 (1 + 2e + … + 2e^(n-1)), with t1 = √(2 h0 / g).
#[test]
fn a_bouncing_ball_restarts_its_speed_at_each_impact() {
    let (h0, g, e) = (1.0, 9.81, 0.8);
    let ball = ComponentDef {
        name: "Test.Ball".into(),
        params: vec![param("g", "m/s2", g, ""), param("e", "1", e, "restitution")],
        vars: vec![state("h", "m", h0, "height"), state("v", "m/s", 0.0, "speed")],
        equations: vec![
            eq(der("h"), n("v"), "it moves"),
            eq(der("v"), -n("g"), "it falls"),
            when(
                cmp(CmpOp::Lt, n("h"), c(0.0)),
                vec![reinit("v", -(n("e") * n("v")))],
                "it bounces",
            ),
        ],
        ..Default::default()
    };
    let mut lib = library();
    lib.add(ball);
    let top = ComponentDef {
        name: "Test.Drop".into(),
        components: vec![sub("ball", "Test.Ball", &[])],
        ..Default::default()
    };
    let (m, _) = prepare_with_report(&lib, &top, None, &Settings::default())
        .unwrap_or_else(|d| panic!("{d:#?}"));
    let run = simulate(&m, 2.5, 0.01, 1e-10);
    let t1 = (2.0 * h0 / g).sqrt();
    let exact_h = |t: f64| {
        // find the flight t is in
        let (mut start, mut v0) = (0.0, 0.0);
        let mut h_start = h0;
        let mut flight = t1;
        loop {
            if t < start + flight {
                let s = t - start;
                return h_start + v0 * s - 0.5 * g * s * s;
            }
            let v_hit = if start == 0.0 { -g * t1 } else { -v0 };
            start += flight;
            v0 = -e * v_hit;
            h_start = 0.0;
            flight = 2.0 * v0 / g;
        }
    };
    let err = worst(&run, "ball.h", exact_h);
    let bounces = run.events.len();
    println!("worst height error {err:.1e} over {bounces} events");
    assert!(bounces >= 3, "{bounces}");
    assert!(err < 1e-7, "{err:e}");
}

/// In fast mode the load's speed is prescribed: the shift's restart has no
/// effect on the motion (it is told as information), and the torque the
/// motor needs follows from the prescribed speed in either gear.
#[test]
fn fast_mode_prescribes_the_motion_through_a_gear_change() {
    use lsim_ir::InverseSpec;
    let actuator = ComponentDef {
        name: "Test.TorqueActuator".into(),
        ports: vec![port("flange", "Flange", ""), signal("tau", false, "N.m")],
        equations: vec![eq(n("flange.tau"), -n("tau"), "it drives with the torque it is told")],
        ..Default::default()
    };
    let command = ComponentDef {
        name: "Test.TorqueCommand".into(),
        ports: vec![signal("y", true, "N.m")],
        params: vec![param("k", "N.m", T, "")],
        equations: vec![eq(n("y"), n("k"), "a constant command")],
        ..Default::default()
    };
    let mut lib = library();
    lib.add(shifting_gear());
    lib.add(actuator);
    lib.add(command);
    let mut top = gear_change();
    top.components[0] = sub("torque", "Test.TorqueActuator", &[]);
    top.components.push(sub("command", "Test.TorqueCommand", &[]));
    top.connections.push(connect("command.y", "torque.tau"));
    // the forward model still runs the shift
    prepare_with_report(&lib, &top, None, &Settings::default())
        .unwrap_or_else(|d| panic!("{d:#?}"));
    let spec = InverseSpec { prescribed: vec!["load.w".into()], freed: vec!["torque.tau".into()] };
    let (m, report) = prepare_with_report(&lib, &top, Some(&spec), &Settings::default())
        .unwrap_or_else(|d| panic!("{d:#?}"));
    println!("{:?}", m.warnings);
    assert_eq!(report.removed, ["command"]);
    assert!(m.warnings.iter().any(|d| d.code == "REINIT-PRESCRIBED"), "{:#?}", m.warnings);
    assert_eq!(m.input_names(), ["load.w", "der(load.w)"]);
    // T = (J2 + i² J1) a / i in either gear, at w2 = 50, a = 2 (first gear)
    let vals = {
        use lsim_ir::eval::{SliceEnv, eval};
        let p: Vec<f64> = m.flat.params.iter().map(|q| q.value).collect();
        let mut vars = vec![f64::NAN; m.flat.vars.len()];
        let ders = vec![f64::NAN; m.flat.vars.len()];
        for v in m.states.iter().chain(&m.discretes) {
            vars[v.0 as usize] = m.flat.var(*v).start.unwrap_or(0.0);
        }
        for (v, x) in m.inputs.iter().zip([50.0, 2.0]) {
            vars[v.0 as usize] = x;
        }
        assert!(m.algebraics.is_empty(), "explicit");
        let mut ders = ders;
        for a in &m.assignments {
            let x = eval(&a.expr, &SliceEnv { t: 0.0, vars: &vars, ders: &ders, params: &p });
            match a.target {
                lsim_ir::Slot::Var(v) => vars[v.0 as usize] = x,
                lsim_ir::Slot::Der(v) => ders[v.0 as usize] = x,
            }
        }
        for a in &m.aliases {
            vars[a.var.0 as usize] = match a.target {
                lsim_ir::AliasTarget::Const(c) => c,
                lsim_ir::AliasTarget::Var { var, negated } => {
                    let x = vars[var.0 as usize];
                    if negated { -x } else { x }
                }
            };
        }
        vars
    };
    let tau = vals[m.flat.find_var("torque.tau").unwrap().0 as usize];
    let want = (J2 + I1 * I1 * J1) * 2.0 / I1;
    println!("torque {tau} (exact {want})");
    assert!((tau - want).abs() < 1e-12 * want, "{tau}");
}
