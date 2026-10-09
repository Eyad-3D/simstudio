//! The exact-answer problems of `benchmarks/reference/problems/` built from
//! the library's primitives and run with the engine's pipeline: every
//! compared signal and energy term at the checkpoint times, every event
//! time, and every part's energy books.

mod common;

use common::*;
use lsim_ir::component::build::{eq, state};
use lsim_ir::expr::{der, name as n};
use lsim_lib::x::{discrete, lt, time, when};
use lsim_project::reference::load;

const RTOL: f64 = 1e-9;
const TOL: f64 = 1e-6;

fn assert_event(name: &str, got: f64, want: f64) {
    assert!((got - want).abs() < 1e-7 + 1e-7 * want, "{name}: {got} vs exact {want}");
}

#[test]
fn elec_rc_step() {
    let pr = load("elec_rc_step").unwrap();
    let lib = lib();
    let top = model(
        "RC",
        vec![
            part("src", "Electrical.ConstantVoltage", &[("V", pr.p("V"))]),
            part("r", "Electrical.Resistor", &[("R", pr.p("R"))]),
            part("cap", "Electrical.Capacitor", &[("C", pr.p("C"))]),
            part("gnd", "Electrical.Ground", &[]),
            part("vs", "Electrical.VoltageSensor", &[]),
            part("x", "Test.Crossing_V", &[("level", pr.p("V") * pr.p("event_fraction"))]),
        ],
        &[
            ("src.p", "r.p"),
            ("r.n", "cap.p"),
            ("cap.n", "src.n"),
            ("src.n", "gnd.p"),
            ("vs.p", "cap.p"),
            ("vs.n", "cap.n"),
            ("vs.v", "x.u"),
        ],
    );
    let (_built, res) = run(&lib, &top, pr.t_end(), pr.run["output_dt"], RTOL);
    let (t, cp) = (&pr.times, &pr.checkpoints);
    check(&res, "cap.v", t, &cp["v_C"], 400.0, TOL);
    check(&res, "cap.i", t, &cp["i"], 8.0, TOL);
    let (src_ports, ..) = books(&res, "src");
    let (_, r_loss, ..) = books(&res, "r");
    let (.., c_stored, _) = books(&res, "cap");
    for (k, tt) in t.iter().enumerate() {
        let i = res.times.iter().position(|x| (x - tt).abs() < 1e-12).unwrap();
        let s = cp["E_source"][3];
        assert!((-src_ports[i] - cp["E_source"][k]).abs() < TOL * s);
        assert!((r_loss[i] - cp["E_R"][k]).abs() < TOL * s);
        assert!((c_stored[i] - cp["E_C"][k]).abs() < TOL * s);
    }
    assert_event("t_event", last(&res, "x.t_up"), pr.events["t_event"]);
    books_close(&res, 1e-7);
}

#[test]
fn elec_rl_step() {
    let pr = load("elec_rl_step").unwrap();
    let lib = lib();
    let top = model(
        "RL",
        vec![
            part("src", "Electrical.ConstantVoltage", &[("V", pr.p("V"))]),
            part("r", "Electrical.Resistor", &[("R", pr.p("R"))]),
            part("ind", "Electrical.Inductor", &[("L", pr.p("L"))]),
            part("is", "Electrical.CurrentSensor", &[]),
            part("gnd", "Electrical.Ground", &[]),
            part("x", "Test.Crossing_A", &[("level", pr.p("i_event"))]),
        ],
        &[
            ("src.p", "is.p"),
            ("is.n", "r.p"),
            ("r.n", "ind.p"),
            ("ind.n", "src.n"),
            ("src.n", "gnd.p"),
            ("is.i", "x.u"),
        ],
    );
    let (_built, res) = run(&lib, &top, pr.t_end(), pr.run["output_dt"], RTOL);
    let (t, cp) = (&pr.times, &pr.checkpoints);
    check(&res, "ind.i", t, &cp["i"], 0.5, TOL);
    check(&res, "ind.v", t, &cp["v_L"], 12.0, TOL);
    let s = cp["E_source"][3];
    let (src, ..) = books(&res, "src");
    let (_, rl, ..) = books(&res, "r");
    let (.., ls, _) = books(&res, "ind");
    for (k, tt) in t.iter().enumerate() {
        let i = res.times.iter().position(|x| (x - tt).abs() < 1e-12).unwrap();
        assert!((-src[i] - cp["E_source"][k]).abs() < TOL * s);
        assert!((rl[i] - cp["E_R"][k]).abs() < TOL * s);
        assert!((ls[i] - cp["E_L"][k]).abs() < TOL * s);
    }
    assert_event("t_event", last(&res, "x.t_up"), pr.events["t_event"]);
    books_close(&res, 1e-7);
}

fn dc_motor(id: &str, with_l: bool) {
    let pr = load(id).unwrap();
    let lib = lib();
    let (v, r, k, j, b) = (pr.p("V"), pr.p("R"), pr.p("k"), pr.p("J"), pr.p("b"));
    let w_inf = v / (k + r * b / k);
    let mut parts = vec![
        part("src", "Electrical.ConstantVoltage", &[("V", v)]),
        part("r", "Electrical.Resistor", &[("R", r)]),
        part("emf", "Electrical.Emf", &[("k", k)]),
        part("rotor", "Rotational.Inertia", &[("J", j)]),
        part("fric", "Rotational.Damper", &[("d", b)]),
        part("gnd", "Electrical.Ground", &[]),
        part("ws", "Rotational.SpeedSensor", &[]),
        part("x", "Test.Crossing_rad_s", &[("level", pr.p("event_fraction") * w_inf)]),
    ];
    let mut conns = vec![
        ("src.p", "r.p"),
        ("emf.n", "src.n"),
        ("src.n", "gnd.p"),
        ("emf.flange", "rotor.a"),
        ("fric.flange", "rotor.b"),
        ("ws.flange", "rotor.a"),
        ("ws.w", "x.u"),
    ];
    if with_l {
        parts.push(part("ind", "Electrical.Inductor", &[("L", pr.p("L"))]));
        conns.push(("r.n", "ind.p"));
        conns.push(("ind.n", "emf.p"));
    } else {
        conns.push(("r.n", "emf.p"));
    }
    let top = model("DcMotor", parts, &conns);
    let (_built, res) = run(&lib, &top, pr.t_end(), pr.run["output_dt"], RTOL);
    let (t, cp) = (&pr.times, &pr.checkpoints);
    check(&res, "rotor.w", t, &cp["omega"], w_inf, TOL);
    check(&res, "r.i", t, &cp["i"], v / r, TOL);
    let s = cp["E_in"][3];
    let (src, ..) = books(&res, "src");
    let (_, rl, ..) = books(&res, "r");
    let (.., kin, _) = books(&res, "rotor");
    let (_, fl, ..) = books(&res, "fric");
    for (q, tt) in t.iter().enumerate() {
        let i = res.times.iter().position(|x| (x - tt).abs() < 1e-9).unwrap();
        assert!((-src[i] - cp["E_in"][q]).abs() < TOL * s, "E_in at {tt}");
        assert!((rl[i] - cp["E_R"][q]).abs() < TOL * s, "E_R at {tt}");
        assert!((kin[i] - cp["E_kin"][q]).abs() < TOL * s, "E_kin at {tt}");
        assert!((fl[i] - cp["E_friction"][q]).abs() < TOL * s, "E_friction at {tt}");
        if with_l {
            let (.., ls, _) = books(&res, "ind");
            assert!((ls[i] - cp["E_L"][q]).abs() < TOL * s, "E_L at {tt}");
        }
    }
    assert_event("t_event", last(&res, "x.t_up"), pr.events["t_event"]);
    books_close(&res, 1e-7);
}

#[test]
fn motor_dc_spinup() {
    dc_motor("motor_dc_spinup", true);
}

#[test]
fn motor_dc_spinup_l0() {
    dc_motor("motor_dc_spinup_l0", false);
}

#[test]
fn mech_inertia_coastdown() {
    let pr = load("mech_inertia_coastdown").unwrap();
    let lib = lib();
    let w0 = pr.init("omega");
    let mut top = model(
        "Coastdown",
        vec![
            part("rotor", "Rotational.Inertia", &[("J", pr.p("J")), ("w0", w0)]),
            part("visc", "Rotational.Damper", &[("d", pr.p("c"))]),
            part("coulomb", "Rotational.Friction", &[("s0", w0)]),
            part("tc", "Signal.Constant_N_m", &[("k", pr.p("T_c"))]),
        ],
        &[("visc.flange", "rotor.a"), ("coulomb.flange", "rotor.b"), ("tc.y", "coulomb.fc")],
    );
    top.vars.push(state("theta", "rad", 0.0, "the rotor's angle"));
    top.equations.push(eq(der("theta"), n("rotor.w"), "the angle"));
    let (_built, res) = run(&lib, &top, pr.t_end(), pr.run["output_dt"], RTOL);
    let (t, cp) = (&pr.times, &pr.checkpoints);
    check(&res, "rotor.w", t, &cp["omega"], w0, TOL);
    check(&res, "theta", t, &cp["theta"], cp["theta"][3], TOL);
    let s = -cp["E_kin"][3];
    let (_, vl, ..) = books(&res, "visc");
    let (_, cl, ..) = books(&res, "coulomb");
    let (.., kin, _) = books(&res, "rotor");
    for (q, tt) in t.iter().enumerate() {
        let i = res.times.iter().position(|x| (x - tt).abs() < 1e-9).unwrap();
        assert!((vl[i] - cp["E_viscous"][q]).abs() < TOL * s, "E_viscous at {tt}: {}", vl[i]);
        assert!((cl[i] - cp["E_coulomb"][q]).abs() < TOL * s, "E_coulomb at {tt}: {}", cl[i]);
        assert!((kin[i] - cp["E_kin"][q]).abs() < TOL * s, "E_kin at {tt}: {}", kin[i]);
    }
    assert_event("t_stop", ev(&res, "sticks"), pr.events["t_stop"]);
    // it stays stopped
    assert!(last(&res, "rotor.w").abs() < 1e-9);
    books_close(&res, 1e-7);
}

#[test]
fn mech_clutch_lockup() {
    let pr = load("mech_clutch_lockup").unwrap();
    let lib = lib();
    let mut top = model(
        "Clutch",
        vec![
            part("j1", "Rotational.Inertia", &[("J", pr.p("J1")), ("w0", pr.init("omega1"))]),
            part("j2", "Rotational.Inertia", &[("J", pr.p("J2")), ("w0", pr.init("omega2"))]),
            part("drive", "Rotational.ConstantTorque", &[("tau", pr.p("T_drive"))]),
            part("load", "Rotational.ConstantTorque", &[("tau", -pr.p("T_load"))]),
            part("clutch", "Rotational.Clutch", &[("s0", pr.init("omega1") - pr.init("omega2"))]),
            part("cap", "Signal.Constant_N_m", &[("k", pr.p("T_c"))]),
        ],
        &[
            ("drive.flange", "j1.a"),
            ("j1.b", "clutch.a"),
            ("clutch.b", "j2.a"),
            ("load.flange", "j2.b"),
            ("cap.y", "clutch.fc"),
        ],
    );
    top.vars.push(discrete("t_lock", "s", -1.0, "when the slip fell to eps_lock"));
    top.params.push(lsim_lib::x::p("eps_lock", "rad/s", pr.p("eps_lock"), "lock level"));
    top.equations.push(when(
        lt(n("clutch.s"), n("eps_lock")),
        &[("t_lock", time())],
        "the slip falls to eps_lock",
    ));
    let (_built, res) = run(&lib, &top, pr.t_end(), pr.run["output_dt"], RTOL);
    let (t, cp) = (&pr.times, &pr.checkpoints);
    check(&res, "j1.w", t, &cp["omega1"], 250.0, TOL);
    check(&res, "j2.w", t, &cp["omega2"], 250.0, TOL);
    check(&res, "clutch.a.tau", t, &cp["T_clutch"], 100.0, TOL);
    let s = cp["E_clutch"][3];
    let (drive, ..) = books(&res, "drive");
    let (_, cl, ..) = books(&res, "clutch");
    let (.., k1, _) = books(&res, "j1");
    let (.., k2, _) = books(&res, "j2");
    for (q, tt) in t.iter().enumerate() {
        let i = res.times.iter().position(|x| (x - tt).abs() < 1e-9).unwrap();
        assert!((-drive[i] - cp["E_drive"][q]).abs() < TOL * s, "E_drive at {tt}");
        assert!((cl[i] - cp["E_clutch"][q]).abs() < TOL * s, "E_clutch at {tt}");
        assert!((k1[i] + k2[i] - cp["E_kin"][q]).abs() < TOL * s, "E_kin at {tt}");
    }
    assert_event("t_lock", last(&res, "t_lock"), pr.events["t_lock"]);
    books_close(&res, 1e-7);
}

/// The exact answer conserves angular momentum through the shift (an
/// impulse at the gear mesh). Stating that needs `reinit` of the speeds at
/// the shift, which preparation drops for now (DESIGN.md 5.8: `reinit`
/// must reach `PreparedWhen`): without it the restart keeps whichever
/// speed index reduction made a state and moves the other with the new
/// ratio, so the kinetic energy jumps by the wrong amount (the energy
/// books show it as energy lost at the event).
#[test]
#[ignore = "needs reinit at the shift, which preparation drops (DESIGN.md 5.8)"]
fn mech_gear_change() {
    let pr = load("mech_gear_change").unwrap();
    let mut lib = lib();
    lib.add(lsim_lib::rotational::lossy_gear("Test.VariableGear", true));
    let top = model(
        "GearChange",
        vec![
            part("motor", "Rotational.Inertia", &[("J", pr.p("J1"))]),
            part("load", "Rotational.Inertia", &[("J", pr.p("J2"))]),
            part("drive", "Rotational.ConstantTorque", &[("tau", pr.p("T"))]),
            part("gear", "Test.VariableGear", &[("eta", 1.0)]),
            part(
                "select",
                "Signal.Step",
                &[("y0", pr.p("i1")), ("y1", pr.p("i2")), ("t_step", pr.p("t_shift"))],
            ),
        ],
        &[
            ("drive.flange", "motor.a"),
            ("motor.b", "gear.a"),
            ("gear.b", "load.a"),
            ("select.y", "gear.ratio"),
        ],
    );
    let (_built, res) = run(&lib, &top, pr.t_end(), pr.run["output_dt"], RTOL);
    let (t, cp) = (&pr.times, &pr.checkpoints);
    check(&res, "load.w", t, &cp["omega2"], cp["omega2"][3], TOL);
    check(&res, "motor.w", t, &cp["omega1"], cp["omega1"][3], TOL);
    let s = cp["E_drive"][3];
    let (drive, ..) = books(&res, "drive");
    let (.., k1, _) = books(&res, "motor");
    let (.., k2, _) = books(&res, "load");
    for (q, tt) in t.iter().enumerate() {
        let i = res.times.iter().position(|x| (x - tt).abs() < 1e-9).unwrap();
        assert!((-drive[i] - cp["E_drive"][q]).abs() < TOL * s, "E_drive at {tt}");
        assert!((k1[i] + k2[i] - cp["E_kin"][q]).abs() < TOL * s, "E_kin at {tt}");
    }
    // the shift's loss: the kinetic energy that vanishes at the event
    let e = res.energy.as_ref().expect("books");
    assert!(
        (e.event_loss - cp["E_shift"][3]).abs() < TOL * s,
        "E_shift: {} vs exact {}",
        e.event_loss,
        cp["E_shift"][3]
    );
    books_close(&res, 1e-7);
}

#[test]
fn veh_coastdown() {
    let pr = load("veh_coastdown").unwrap();
    let lib = lib();
    let v0 = pr.init("v");
    let mut top = model(
        "Coastdown",
        vec![
            part("car", "Translational.Mass", &[("m", pr.p("m")), ("v0", v0)]),
            part("aero", "Translational.QuadraticDrag", &[("cda", 2.0 * pr.p("C")), ("rho", 1.0)]),
            part("roll", "Translational.ConstantFriction", &[("f_max", pr.p("A")), ("s0", v0)]),
            part("vs", "Translational.SpeedSensor", &[]),
            part("x", "Test.Crossing_m_s", &[("level", pr.p("v_event"))]),
        ],
        &[
            ("aero.flange", "car.a"),
            ("roll.flange", "car.b"),
            ("vs.flange", "car.a"),
            ("vs.v", "x.u"),
        ],
    );
    top.vars.push(state("dist", "m", 0.0, "distance"));
    top.equations.push(eq(der("dist"), n("car.v"), "the distance"));
    let (_built, res) = run(&lib, &top, pr.t_end(), pr.run["output_dt"], RTOL);
    let (t, cp) = (&pr.times, &pr.checkpoints);
    check(&res, "car.v", t, &cp["v"], v0, TOL);
    check(&res, "dist", t, &cp["x"], cp["x"][3], TOL);
    let s = -cp["E_kin"][3];
    let (_, al, ..) = books(&res, "aero");
    let (_, rl, ..) = books(&res, "roll");
    let (.., kin, _) = books(&res, "car");
    for (q, tt) in t.iter().enumerate() {
        let i = res.times.iter().position(|x| (x - tt).abs() < 1e-9).unwrap();
        assert!((al[i] - cp["E_aero"][q]).abs() < TOL * s, "E_aero at {tt}");
        assert!((rl[i] - cp["E_roll"][q]).abs() < TOL * s, "E_roll at {tt}");
        assert!((kin[i] - cp["E_kin"][q]).abs() < TOL * s, "E_kin at {tt}");
    }
    assert_event("t_event", last(&res, "x.t_down"), pr.events["t_event"]);
    assert_event("t_stop", ev(&res, "sticks"), pr.events["t_stop"]);
    assert!(last(&res, "car.v").abs() < 1e-9, "it stays stopped");
    books_close(&res, 1e-7);
}

#[test]
fn veh_constant_power() {
    let pr = load("veh_constant_power").unwrap();
    let lib = lib();
    let mut top = model(
        "ConstantPower",
        vec![
            part("car", "Translational.Mass", &[("m", pr.p("m")), ("v0", pr.init("v"))]),
            part("push", "Test.ConstantPowerForce", &[("P", pr.p("P"))]),
            part("vs", "Translational.SpeedSensor", &[]),
            part("x", "Test.Crossing_m_s", &[("level", pr.p("v_event"))]),
        ],
        &[("push.flange", "car.a"), ("vs.flange", "car.b"), ("vs.v", "x.u")],
    );
    top.vars.push(state("dist", "m", 0.0, "distance"));
    top.equations.push(eq(der("dist"), n("car.v"), "the distance"));
    let (_built, res) = run(&lib, &top, pr.t_end(), pr.run["output_dt"], RTOL);
    let (t, cp) = (&pr.times, &pr.checkpoints);
    check(&res, "car.v", t, &cp["v"], 35.0, TOL);
    check(&res, "dist", t, &cp["x"], cp["x"][3], TOL);
    let s = cp["E_supplied"][3];
    let (push, ..) = books(&res, "push");
    let (.., kin, _) = books(&res, "car");
    for (q, tt) in t.iter().enumerate() {
        let i = res.times.iter().position(|x| (x - tt).abs() < 1e-9).unwrap();
        assert!((-push[i] - cp["E_supplied"][q]).abs() < TOL * s);
        assert!((kin[i] - cp["E_kin"][q]).abs() < TOL * s);
    }
    assert_event("t_event", last(&res, "x.t_up"), pr.events["t_event"]);
    books_close(&res, 1e-7);
}

const K: f64 = 273.15;

#[test]
fn therm_lumped_mass() {
    let pr = load("therm_lumped_mass").unwrap();
    let lib = lib();
    let top = model(
        "LumpedMass",
        vec![
            part("mass", "Thermal.HeatCapacitor", &[("C", pr.p("C")), ("T0", pr.init("T") + K)]),
            part("cond", "Thermal.ThermalConductor", &[("G", pr.p("G"))]),
            part("amb", "Thermal.FixedTemperature", &[("T", pr.p("T_amb") + K)]),
            part("heat", "Thermal.PrescribedHeatFlow", &[]),
            part(
                "p",
                "Signal.Step_W",
                &[("y0", pr.p("P")), ("y1", 0.0), ("t_step", pr.p("t_off"))],
            ),
            part("ts", "Thermal.TemperatureSensor", &[]),
            part("hot", "Test.Crossing_K", &[("level", pr.p("T_hot") + K)]),
            part("cool", "Test.Crossing_K", &[("level", pr.p("T_cool") + K)]),
        ],
        &[
            ("heat.port", "mass.port"),
            ("mass.port", "cond.a"),
            ("cond.b", "amb.port"),
            ("p.y", "heat.Q_flow"),
            ("ts.port", "mass.port"),
            ("ts.T", "hot.u"),
            ("ts.T", "cool.u"),
        ],
    );
    let (_built, res) = run(&lib, &top, pr.t_end(), pr.run["output_dt"], RTOL);
    let (t, cp) = (&pr.times, &pr.checkpoints);
    let shifted: Vec<f64> = cp["T"].iter().map(|x| x + K).collect();
    check(&res, "mass.T", t, &shifted, 75.0, TOL);
    let s = cp["E_heat"][3];
    let (heat, ..) = books(&res, "heat");
    let (amb, ..) = books(&res, "amb");
    let (.., st, _) = books(&res, "mass");
    for (q, tt) in t.iter().enumerate() {
        let i = res.times.iter().position(|x| (x - tt).abs() < 1e-9).unwrap();
        assert!((-heat[i] - cp["E_heat"][q]).abs() < TOL * s);
        assert!((amb[i] - cp["E_ambient"][q]).abs() < TOL * s);
        assert!((st[i] - cp["E_stored"][q]).abs() < TOL * s);
    }
    assert_event("t_hot", last(&res, "hot.t_up"), pr.events["t_hot"]);
    assert_event("t_cool", last(&res, "cool.t_down"), pr.events["t_cool"]);
    books_close(&res, 1e-7);
}

#[test]
fn therm_two_masses() {
    let pr = load("therm_two_masses").unwrap();
    let lib = lib();
    let mut top = model(
        "TwoMasses",
        vec![
            part("cells", "Thermal.HeatCapacitor", &[("C", pr.p("C1")), ("T0", pr.init("T1") + K)]),
            part("plate", "Thermal.HeatCapacitor", &[("C", pr.p("C2")), ("T0", pr.init("T2") + K)]),
            part("g12", "Thermal.ThermalConductor", &[("G", pr.p("G12"))]),
            part("g2a", "Thermal.ThermalConductor", &[("G", pr.p("G2a"))]),
            part("amb", "Thermal.FixedTemperature", &[("T", pr.p("T_amb") + K)]),
            part("heat", "Thermal.FixedHeatFlow", &[("Q_flow", pr.p("P"))]),
            part("ts", "Thermal.TemperatureSensor", &[]),
            part("x", "Test.Crossing_K", &[("level", pr.p("T1_event") + K)]),
        ],
        &[
            ("heat.port", "cells.port"),
            ("cells.port", "g12.a"),
            ("g12.b", "plate.port"),
            ("plate.port", "g2a.a"),
            ("g2a.b", "amb.port"),
            ("ts.port", "cells.port"),
            ("ts.T", "x.u"),
        ],
    );
    top.vars.push(state("e12", "J", 0.0, "heat passed from 1 to 2"));
    top.equations.push(eq(der("e12"), n("g12.Q"), "its integral"));
    let (_built, res) = run(&lib, &top, pr.t_end(), pr.run["output_dt"], RTOL);
    let (t, cp) = (&pr.times, &pr.checkpoints);
    let t1: Vec<f64> = cp["T1"].iter().map(|x| x + K).collect();
    let t2: Vec<f64> = cp["T2"].iter().map(|x| x + K).collect();
    check(&res, "cells.T", t, &t1, 45.0, TOL);
    check(&res, "plate.T", t, &t2, 25.0, TOL);
    let s = cp["E_heat"][3];
    let (heat, ..) = books(&res, "heat");
    let (amb, ..) = books(&res, "amb");
    let (.., s1, _) = books(&res, "cells");
    let (.., s2, _) = books(&res, "plate");
    for (q, tt) in t.iter().enumerate() {
        let i = res.times.iter().position(|x| (x - tt).abs() < 1e-9).unwrap();
        assert!((-heat[i] - cp["E_heat"][q]).abs() < TOL * s);
        assert!((amb[i] - cp["E_ambient"][q]).abs() < TOL * s);
        assert!((s1[i] - cp["E_stored1"][q]).abs() < TOL * s);
        assert!((s2[i] - cp["E_stored2"][q]).abs() < TOL * s);
        assert!((at(&res, "e12", *tt) - cp["E_12"][q]).abs() < TOL * s);
    }
    assert_event("t_event", last(&res, "x.t_up"), pr.events["t_event"]);
    books_close(&res, 1e-7);
}
