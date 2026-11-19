//! Work package 2's acceptance: the inverse model of fast mode prepares for
//! stand-ins of the example cars (a BEV, a Formula Student electric car
//! with a motor per axle, a P2 hybrid) with only the drive cycle known,
//! and its explicit solution gives the closed-form commands; each
//! `INVERSE-*` fault gives its code and parts.

#![allow(clippy::needless_range_loop)] // the Newton helper indexes several arrays in step

mod common;

use common::{library, signal};
use lsim_ir::component::build::{connect, discrete, eq, param, port, state, sub, var};
use lsim_ir::eval::{SliceEnv, eval};
use lsim_ir::expr::{Builtin, Expr, c, call, der, name as n};
use lsim_ir::{
    AliasTarget, ComponentDef, Diagnostic, EnergyDecl, InverseSpec, Library, PreparedModel,
    Severity, Slot, SubDecl,
};
use lsim_prep::{PrepOptions, Settings, prepare, prepare_with_report};

// the stand-ins' values
const M: f64 = 1500.0;
const CRR: f64 = 0.01;
const CDA: f64 = 0.6;
const RHO: f64 = 1.2;
const G: f64 = 9.81;
const R_WHEEL: f64 = 0.3;
const J_WHEEL: f64 = 1.2;
const J_ROTOR: f64 = 0.05;
const RATIO: f64 = 9.0;
const V0: f64 = 400.0;
const R0: f64 = 0.05;
const K_T: f64 = 0.8;
const R_M: f64 = 0.02;

fn components() -> Vec<ComponentDef> {
    let body = ComponentDef {
        name: "Vehicle.Body".into(),
        doc: "The car's mass on the road, with rolling and air resistance.".into(),
        ports: vec![port("p", "TFlange", "where the wheels push"), signal("speed", true, "m/s")],
        params: vec![
            param("m", "kg", M, "mass"),
            param("crr", "1", CRR, "rolling resistance coefficient"),
            param("cda", "m2", CDA, "drag area"),
            param("rho", "kg/m3", RHO, "air density"),
            param("g", "m/s2", G, "gravity"),
        ],
        vars: vec![state("v", "m/s", 0.0, "speed"), var("f_res", "N", "resistance")],
        equations: vec![
            eq(n("v"), n("p.v"), "it moves with the wheels' contact"),
            eq(n("m") * der("v"), n("p.f") - n("f_res"), "m dv/dt is the net force"),
            eq(
                n("f_res"),
                n("crr") * n("m") * n("g") + c(0.5) * n("rho") * n("cda") * n("v") * n("v"),
                "rolling and air resistance",
            ),
            eq(n("speed"), n("v"), "its speedometer"),
        ],
        energy: EnergyDecl {
            stored: Some(c(0.5) * n("m") * n("v") * n("v")),
            loss: Some(n("f_res") * n("v")),
        },
        ..Default::default()
    };
    let wheel = ComponentDef {
        name: "Vehicle.Wheel".into(),
        ports: vec![port("hub", "Flange", ""), port("road", "TFlange", "")],
        params: vec![param("r", "m", R_WHEEL, "rolling radius")],
        equations: vec![
            eq(n("road.v"), n("r") * n("hub.w"), "it rolls without slip"),
            eq(c(0.0), n("hub.tau") + n("r") * n("road.f"), "the power through it is kept"),
        ],
        ..Default::default()
    };
    let cycle = ComponentDef {
        name: "Vehicle.Cycle".into(),
        doc: "A drive cycle: the target speed over time.".into(),
        ports: vec![signal("y", true, "m/s")],
        params: vec![
            param("v0", "m/s", 15.0, ""),
            param("dv", "m/s", 5.0, ""),
            param("om", "1/s", 0.3, ""),
        ],
        equations: vec![eq(
            n("y"),
            n("v0") + n("dv") * call(Builtin::Sin, vec![n("om") * Expr::Time]),
            "the target speed",
        )],
        ..Default::default()
    };
    let driver = ComponentDef {
        name: "Vehicle.Driver".into(),
        doc: "A PI driver: a torque command from the speed error.".into(),
        ports: vec![
            signal("target", false, "m/s"),
            signal("speed", false, "m/s"),
            signal("cmd", true, "N.m"),
        ],
        params: vec![param("kp", "N.s", 200.0, ""), param("ki", "N", 50.0, "")],
        vars: vec![state("e_int", "m", 0.0, "integrated error")],
        equations: vec![
            eq(der("e_int"), n("target") - n("speed"), "it integrates the error"),
            eq(n("cmd"), n("kp") * (n("target") - n("speed")) + n("ki") * n("e_int"), "PI"),
        ],
        ..Default::default()
    };
    // a driver that also brakes: its brake output feeds a brake
    let mut driver2 = driver.clone();
    driver2.name = "Vehicle.BrakingDriver".into();
    driver2.ports.push(signal("brake", true, "N.m"));
    driver2.equations.push(eq(n("brake"), c(0.0) * n("cmd"), "it never brakes here"));
    let motor = ComponentDef {
        name: "Vehicle.Motor".into(),
        doc: "A traction motor and inverter: the torque it is told (within its limit), drawing \
              the mechanical power plus its copper loss."
            .into(),
        ports: vec![
            port("flange", "Flange", "its shaft"),
            port("p", "Pin", ""),
            port("n", "Pin", ""),
            signal("tau_cmd", false, "N.m"),
        ],
        params: vec![
            param("tau_max", "N.m", 300.0, ""),
            param("k_t", "N.m/A", K_T, "torque constant"),
            param("r_m", "Ohm", R_M, "copper resistance"),
        ],
        vars: vec![
            var("tau", "N.m", ""),
            var("w", "rad/s", ""),
            var("p_elec", "W", ""),
            var("v", "V", ""),
            var("i", "A", ""),
        ],
        equations: vec![
            eq(n("w"), n("flange.w"), "its shaft"),
            eq(
                n("tau"),
                call(Builtin::Limit, vec![n("tau_cmd"), -n("tau_max"), n("tau_max")]),
                "it gives the torque it is told, within its limit",
            ),
            eq(n("flange.tau"), -n("tau"), "it drives its shaft"),
            eq(
                n("p_elec"),
                n("tau") * n("w") + n("r_m") * (n("tau") / n("k_t")) * (n("tau") / n("k_t")),
                "it draws the mechanical power plus its copper loss",
            ),
            eq(n("v"), n("p.v") - n("n.v"), "its voltage"),
            eq(c(0.0), n("p.i") + n("n.i"), "its current"),
            eq(n("i"), n("p.i"), "its current is the current into p"),
            eq(n("v") * n("i"), n("p_elec"), "the battery delivers its power"),
        ],
        ..Default::default()
    };
    let split = ComponentDef {
        name: "Vehicle.Split".into(),
        doc: "Splits a torque demand between two actuators.".into(),
        ports: vec![
            signal("u", false, "N.m"),
            signal("y1", true, "N.m"),
            signal("y2", true, "N.m"),
        ],
        params: vec![param("k", "1", 0.5, "the first actuator's share")],
        equations: vec![
            eq(n("y1"), n("k") * n("u"), "the first share"),
            eq(n("y2"), (c(1.0) - n("k")) * n("u"), "the rest"),
        ],
        ..Default::default()
    };
    let engine = ComponentDef {
        name: "Vehicle.Engine".into(),
        ports: vec![port("flange", "Flange", "crankshaft"), signal("cmd", false, "N.m")],
        params: vec![param("eff", "1", 0.35, "efficiency")],
        vars: vec![var("fuel_power", "W", "")],
        equations: vec![
            eq(n("flange.tau"), -n("cmd"), "it gives the torque it is told"),
            eq(n("fuel_power"), n("cmd") * n("flange.w") / n("eff"), "the fuel it burns"),
        ],
        ..Default::default()
    };
    let clutch = ComponentDef {
        name: "Vehicle.ClosedClutch".into(),
        ports: vec![port("a", "Flange", ""), port("b", "Flange", "")],
        equations: vec![
            eq(n("a.w"), n("b.w"), "it is engaged"),
            eq(c(0.0), n("a.tau") + n("b.tau"), "it passes the torque"),
        ],
        ..Default::default()
    };
    let brake = ComponentDef {
        name: "Vehicle.Brake".into(),
        ports: vec![port("flange", "Flange", ""), signal("cmd", false, "N.m")],
        equations: vec![eq(n("flange.tau"), n("cmd"), "it takes the torque it is told")],
        ..Default::default()
    };
    let gear_select = ComponentDef {
        name: "Vehicle.GearSelect".into(),
        vars: vec![discrete("gear", "1", 1.0, "the gear engaged")],
        ..Default::default()
    };
    vec![body, wheel, cycle, driver, driver2, motor, split, engine, clutch, brake, gear_select]
}

fn vehicle_library() -> Library {
    let mut lib = library();
    for d in components() {
        lib.add(d);
    }
    lib
}

fn inertia(name: &str, j: f64) -> SubDecl {
    sub(name, "Rotational.Inertia", &[("J", c(j))])
}

fn gear(name: &str, ratio: f64) -> SubDecl {
    sub(name, "Rotational.IdealGear", &[("ratio", c(ratio))])
}

/// The battery: a source behind its internal resistance, grounded.
fn battery() -> (Vec<SubDecl>, Vec<(&'static str, &'static str)>) {
    (
        vec![
            sub("cell", "Electrical.ConstantVoltage", &[("V", c(V0))]),
            sub("r0", "Electrical.Resistor", &[("R", c(R0))]),
            sub("gnd", "Electrical.Ground", &[]),
        ],
        vec![("cell.p", "r0.p"), ("cell.n", "gnd.p")],
    )
}

fn car(name: &str, mut parts: Vec<SubDecl>, links: &[(&str, &str)]) -> ComponentDef {
    let (bat, bat_links) = battery();
    parts.extend(bat);
    parts.push(sub("body", "Vehicle.Body", &[]));
    parts.push(sub("cycle", "Vehicle.Cycle", &[]));
    let mut conns: Vec<_> = links.iter().map(|(a, b)| connect(a, b)).collect();
    conns.extend(bat_links.iter().map(|(a, b)| connect(a, b)));
    conns.push(connect("cycle.y", "driver.target"));
    conns.push(connect("body.speed", "driver.speed"));
    ComponentDef { name: name.into(), components: parts, connections: conns, ..Default::default() }
}

/// Battery-electric: motor, rotor, one gear, the wheels, the body.
fn bev() -> ComponentDef {
    car(
        "Cars.Bev",
        vec![
            sub("driver", "Vehicle.Driver", &[]),
            sub("motor", "Vehicle.Motor", &[]),
            inertia("rotor", J_ROTOR),
            gear("gear", RATIO),
            inertia("wheels", J_WHEEL),
            sub("wheel", "Vehicle.Wheel", &[]),
        ],
        &[
            ("driver.cmd", "motor.tau_cmd"),
            ("motor.flange", "rotor.a"),
            ("rotor.b", "gear.a"),
            ("gear.b", "wheels.a"),
            ("wheels.b", "wheel.hub"),
            ("wheel.road", "body.p"),
            ("r0.n", "motor.p"),
            ("motor.n", "gnd.p"),
        ],
    )
}

const FS_K: f64 = 0.3;
const FS_I_FRONT: f64 = 11.0;
const FS_I_REAR: f64 = 13.0;

/// Formula Student electric: a motor per axle, each with its own gear, the
/// driver's torque split between them.
fn fs() -> ComponentDef {
    car(
        "Cars.Fs",
        vec![
            sub("driver", "Vehicle.Driver", &[]),
            sub("split", "Vehicle.Split", &[("k", c(FS_K))]),
            sub("front", "Vehicle.Motor", &[]),
            sub("rear", "Vehicle.Motor", &[]),
            inertia("front_rotor", J_ROTOR),
            inertia("rear_rotor", J_ROTOR),
            gear("front_gear", FS_I_FRONT),
            gear("rear_gear", FS_I_REAR),
            inertia("front_wheels", 0.5 * J_WHEEL),
            inertia("rear_wheels", 0.5 * J_WHEEL),
            sub("front_wheel", "Vehicle.Wheel", &[]),
            sub("rear_wheel", "Vehicle.Wheel", &[]),
        ],
        &[
            ("driver.cmd", "split.u"),
            ("split.y1", "front.tau_cmd"),
            ("split.y2", "rear.tau_cmd"),
            ("front.flange", "front_rotor.a"),
            ("front_rotor.b", "front_gear.a"),
            ("front_gear.b", "front_wheels.a"),
            ("front_wheels.b", "front_wheel.hub"),
            ("front_wheel.road", "body.p"),
            ("rear.flange", "rear_rotor.a"),
            ("rear_rotor.b", "rear_gear.a"),
            ("rear_gear.b", "rear_wheels.a"),
            ("rear_wheels.b", "rear_wheel.hub"),
            ("rear_wheel.road", "body.p"),
            ("r0.n", "front.p"),
            ("r0.n", "rear.p"),
            ("front.n", "gnd.p"),
            ("rear.n", "gnd.p"),
        ],
    )
}

const P2_K: f64 = 0.6;
const J_ENGINE: f64 = 0.15;
const P2_RATIO: f64 = 7.0;

/// A P2 hybrid: engine, clutch (closed), motor on the gearbox input, the
/// demand split between engine and motor.
fn p2(driver: &str) -> ComponentDef {
    car(
        "Cars.P2",
        vec![
            sub("driver", driver, &[]),
            sub("strategy", "Vehicle.Split", &[("k", c(P2_K))]),
            sub("engine", "Vehicle.Engine", &[]),
            inertia("crank", J_ENGINE),
            sub("clutch", "Vehicle.ClosedClutch", &[]),
            sub("motor", "Vehicle.Motor", &[]),
            inertia("rotor", J_ROTOR),
            gear("gearbox", P2_RATIO),
            inertia("wheels", J_WHEEL),
            sub("wheel", "Vehicle.Wheel", &[]),
        ],
        &[
            ("driver.cmd", "strategy.u"),
            ("strategy.y1", "engine.cmd"),
            ("strategy.y2", "motor.tau_cmd"),
            ("engine.flange", "crank.a"),
            ("crank.b", "clutch.a"),
            ("clutch.b", "rotor.a"),
            ("motor.flange", "rotor.b"),
            ("rotor.b", "gearbox.a"),
            ("gearbox.b", "wheels.a"),
            ("wheels.b", "wheel.hub"),
            ("wheel.road", "body.p"),
            ("r0.n", "motor.p"),
            ("motor.n", "gnd.p"),
        ],
    )
}

fn spec(prescribed: &[&str], freed: &[&str]) -> InverseSpec {
    InverseSpec {
        prescribed: prescribed.iter().map(|s| s.to_string()).collect(),
        freed: freed.iter().map(|s| s.to_string()).collect(),
    }
}

/// Every variable of an inverse model at time `t` for the inputs `u`
/// (in `m.inputs` order): the assignments evaluated in order, the
/// iteration variables (if any) by Newton on the residuals.
fn solve(m: &PreparedModel, u: &[f64], t: f64) -> Vec<f64> {
    let nv = m.flat.vars.len();
    let p: Vec<f64> = m.flat.params.iter().map(|q| q.value).collect();
    let mut vars = vec![f64::NAN; nv];
    let mut ders = vec![f64::NAN; nv];
    for v in m.states.iter().chain(&m.discretes) {
        vars[v.0 as usize] = m.flat.var(*v).start.unwrap_or(0.0);
    }
    for (v, x) in m.inputs.iter().zip(u) {
        vars[v.0 as usize] = *x;
    }
    let set = |vars: &mut [f64], ders: &mut [f64], s: Slot, x: f64| match s {
        Slot::Var(v) => vars[v.0 as usize] = x,
        Slot::Der(v) => ders[v.0 as usize] = x,
    };
    for s in &m.algebraics {
        let guess = match s {
            Slot::Var(v) => m.flat.var(*v).start.unwrap_or(1.0),
            Slot::Der(_) => 0.0,
        };
        set(&mut vars, &mut ders, *s, guess);
    }
    let pass = |vars: &mut Vec<f64>, ders: &mut Vec<f64>| -> Vec<f64> {
        for a in &m.assignments {
            let x = eval(&a.expr, &SliceEnv { t, vars, ders, params: &p });
            set(vars, ders, a.target, x);
        }
        m.residuals.iter().map(|r| eval(&r.expr, &SliceEnv { t, vars, ders, params: &p })).collect()
    };
    let get = |vars: &[f64], ders: &[f64], s: Slot| match s {
        Slot::Var(v) => vars[v.0 as usize],
        Slot::Der(v) => ders[v.0 as usize],
    };
    let nz = m.algebraics.len();
    for _ in 0..50 {
        let f = pass(&mut vars, &mut ders);
        let norm = f.iter().fold(0.0f64, |a, x| a.max(x.abs()));
        if norm < 1e-11 {
            break;
        }
        let mut jac = vec![0.0; nz * nz];
        for j in 0..nz {
            let x = get(&vars, &ders, m.algebraics[j]);
            let h = 1e-7 * x.abs().max(1.0);
            set(&mut vars, &mut ders, m.algebraics[j], x + h);
            let f2 = pass(&mut vars, &mut ders);
            set(&mut vars, &mut ders, m.algebraics[j], x);
            for i in 0..nz {
                jac[i * nz + j] = (f2[i] - f[i]) / h;
            }
        }
        let mut dx: Vec<f64> = f.iter().map(|v| -v).collect();
        assert!(lsim_prep::numeric::lu_solve(&mut jac, &mut dx, nz), "singular");
        for j in 0..nz {
            let x = get(&vars, &ders, m.algebraics[j]);
            set(&mut vars, &mut ders, m.algebraics[j], x + dx[j]);
        }
    }
    let f = pass(&mut vars, &mut ders);
    assert!(f.iter().all(|x| x.abs() < 1e-8), "residuals {f:?}");
    for a in &m.aliases {
        vars[a.var.0 as usize] = match a.target {
            AliasTarget::Const(c) => c,
            AliasTarget::Var { var, negated } => {
                let x = vars[var.0 as usize];
                if negated { -x } else { x }
            }
        };
    }
    vars
}

fn value(m: &PreparedModel, vals: &[f64], name: &str) -> f64 {
    vals[m.flat.find_var(name).unwrap_or_else(|| panic!("no variable {name}")).0 as usize]
}

fn f_res(v: f64) -> f64 {
    CRR * M * G + 0.5 * RHO * CDA * v * v
}

/// The battery current that delivers `p` (W): V0 i - R0 i² = p.
fn battery_current(p: f64) -> f64 {
    (V0 - (V0 * V0 - 4.0 * R0 * p).sqrt()) / (2.0 * R0)
}

fn copper(tau: f64) -> f64 {
    R_M * (tau / K_T) * (tau / K_T)
}

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1e-300)
}

/// The points the inverse models are checked at: (v, a).
const POINTS: [(f64, f64); 4] = [(15.0, 1.2), (3.0, 2.5), (30.0, -0.8), (22.0, 0.0)];

fn prepare_inverse_checked(top: &ComponentDef, freed: &[&str]) -> PreparedModel {
    let lib = vehicle_library();
    // the forward model is sound
    prepare(&lib, top, &PrepOptions::default()).unwrap_or_else(|d| panic!("forward: {d:#?}"));
    let (m, report) =
        prepare_with_report(&lib, top, Some(&spec(&["body.v"], freed)), &Settings::default())
            .unwrap_or_else(|d| panic!("{d:#?}"));
    println!("{report:#?}");
    assert_eq!(report.removed, ["driver"], "the driver is removed");
    assert_eq!(m.input_names(), ["body.v", "der(body.v)"], "only the drive cycle is known");
    let states: Vec<&str> = m.states.iter().map(|v| m.flat.var(*v).name.as_str()).collect();
    assert!(states.is_empty(), "every speed follows the prescribed one: {states:?}");
    m
}

#[test]
fn a_bev_runs_backwards_from_its_drive_cycle() {
    let m = prepare_inverse_checked(&bev(), &["motor.tau_cmd"]);
    assert_eq!(m.limits.len(), 1, "the motor's torque limit is passed through and listed");
    let mut worst = 0.0f64;
    for (v, a) in POINTS {
        let vals = solve(&m, &[v, a], 0.0);
        let tau = J_ROTOR * RATIO * a / R_WHEEL
            + (J_WHEEL * a / R_WHEEL + R_WHEEL * (M * a + f_res(v))) / RATIO;
        let w = RATIO * v / R_WHEEL;
        let i = battery_current(tau * w + copper(tau));
        let got = (value(&m, &vals, "motor.tau_cmd"), value(&m, &vals, "motor.i"));
        println!("v {v}, a {a}: torque {} (exact {tau}), current {} (exact {i})", got.0, got.1);
        worst = worst.max(rel(got.0, tau)).max(rel(got.1, i));
        assert!(rel(value(&m, &vals, "driver.cmd"), tau) < 1e-12, "the link stays");
    }
    println!("worst relative error {worst:.1e}");
    assert!(worst < 1e-10, "{worst:e}");
}

#[test]
fn a_formula_student_car_with_a_motor_per_axle_runs_backwards() {
    let m = prepare_inverse_checked(&fs(), &["split.u"]);
    assert_eq!(m.limits.len(), 2, "both motors' limits are listed");
    let mut worst = 0.0f64;
    for (v, a) in POINTS {
        let vals = solve(&m, &[v, a], 0.0);
        // m a + F = Σ (u_j i_j - (J_rotor i_j² + J_wheels_j) a / r) / r
        let ks = [(FS_K, FS_I_FRONT), (1.0 - FS_K, FS_I_REAR)];
        let inertia: f64 =
            ks.iter().map(|&(_, i)| (J_ROTOR * i * i + 0.5 * J_WHEEL) * a / R_WHEEL).sum();
        let gain: f64 = ks.iter().map(|&(k, i)| k * i).sum();
        let u = (R_WHEEL * (M * a + f_res(v)) + inertia) / gain;
        let (tf, tr) = (FS_K * u, (1.0 - FS_K) * u);
        let p = tf * (FS_I_FRONT * v / R_WHEEL)
            + copper(tf)
            + tr * (FS_I_REAR * v / R_WHEEL)
            + copper(tr);
        let i = battery_current(p);
        let got = (
            value(&m, &vals, "split.u"),
            value(&m, &vals, "front.tau"),
            value(&m, &vals, "rear.tau"),
            value(&m, &vals, "r0.i"),
        );
        println!("v {v}, a {a}: demand {} (exact {u}), battery {} (exact {i})", got.0, got.3);
        for (x, want) in [(got.0, u), (got.1, tf), (got.2, tr), (got.3, i)] {
            worst = worst.max(rel(x, want));
        }
    }
    println!("worst relative error {worst:.1e}");
    assert!(worst < 1e-10, "{worst:e}");
}

#[test]
fn a_p2_hybrid_runs_backwards_and_splits_the_demand() {
    let m = prepare_inverse_checked(&p2("Vehicle.Driver"), &["strategy.u"]);
    let mut worst = 0.0f64;
    for (v, a) in POINTS {
        let vals = solve(&m, &[v, a], 0.0);
        let w = P2_RATIO * v / R_WHEEL;
        let u = (J_ENGINE + J_ROTOR) * P2_RATIO * a / R_WHEEL
            + (J_WHEEL * a / R_WHEEL + R_WHEEL * (M * a + f_res(v))) / P2_RATIO;
        let fuel = P2_K * u * w / 0.35;
        let tm = (1.0 - P2_K) * u;
        let i = battery_current(tm * w + copper(tm));
        let got = (
            value(&m, &vals, "strategy.u"),
            value(&m, &vals, "engine.fuel_power"),
            value(&m, &vals, "motor.i"),
        );
        println!(
            "v {v}, a {a}: demand {} (exact {u}), fuel {} (exact {fuel}), current {} (exact {i})",
            got.0, got.1, got.2
        );
        for (x, want) in [(got.0, u), (got.1, fuel), (got.2, i)] {
            worst = worst.max(rel(x, want));
        }
    }
    println!("worst relative error {worst:.1e}");
    assert!(worst < 1e-10, "{worst:e}");
}

// ---- the faults of fast mode

fn inverse_diagnostics(top: &ComponentDef, s: &InverseSpec) -> Vec<Diagnostic> {
    match prepare_with_report(&vehicle_library(), top, Some(s), &Settings::default()) {
        Ok((m, _)) => m.warnings,
        Err(d) => d,
    }
}

fn expect(top: &ComponentDef, s: &InverseSpec, code: &str, parts: &[&str]) -> Diagnostic {
    let diags = inverse_diagnostics(top, s);
    let d = diags
        .iter()
        .find(|d| d.code == code)
        .unwrap_or_else(|| panic!("no {code} among {diags:#?}"))
        .clone();
    println!("{d}\n    parts {:?}", d.parts);
    assert_eq!(d.parts, parts, "{code}: parts");
    assert_eq!(d.severity, Severity::Error);
    d
}

#[test]
fn freeing_two_commands_for_one_prescribed_speed() {
    // the engine's and the motor's commands both free: the speed decides
    // their sum only
    let d = expect(
        &p2("Vehicle.Driver"),
        &spec(&["body.v"], &["engine.cmd", "motor.tau_cmd"]),
        "INVERSE-UNDER",
        // the commands' parts and everything between them on the shaft
        &["crank", "engine", "motor", "r0", "rotor", "strategy"],
    );
    assert!(d.message.contains("fast mode"), "{}", d.message);
}

#[test]
fn prescribing_two_speeds_of_one_rigid_driveline() {
    expect(
        &bev(),
        &spec(&["body.v", "rotor.w"], &["motor.tau_cmd"]),
        "INVERSE-OVER",
        &["gear", "wheel"],
    );
}

#[test]
fn a_driver_output_something_else_still_needs() {
    // the driver's brake output feeds a brake that stays in the model
    let mut top = bev();
    top.components[0] = sub("driver", "Vehicle.BrakingDriver", &[]);
    top.components.push(sub("brake", "Vehicle.Brake", &[]));
    top.connections.push(connect("driver.brake", "brake.cmd"));
    top.connections.push(connect("brake.flange", "wheels.b"));
    expect(
        &top,
        &spec(&["body.v"], &["motor.tau_cmd"]),
        "INVERSE-DRIVER-USED",
        &["brake", "driver"],
    );
}

#[test]
fn names_fast_mode_cannot_use() {
    expect(&bev(), &spec(&["body.speedo"], &["motor.tau_cmd"]), "INVERSE-UNKNOWN-NAME", &[]);
    expect(&bev(), &spec(&["body.v"], &["body.v"]), "INVERSE-NOT-INPUT", &["body"]);
    let mut top = bev();
    top.components.push(sub("box", "Vehicle.GearSelect", &[]));
    expect(&top, &spec(&["box.gear"], &["motor.tau_cmd"]), "INVERSE-PRESCRIBED", &["box"]);
}
