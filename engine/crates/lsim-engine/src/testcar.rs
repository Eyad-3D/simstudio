//! A battery-electric test car, written by hand in the engine's IR: the
//! stand-in for the example cars until work package 5 maps today's blocks
//! (DESIGN.md: "WP6 starts its stepper on hand-written inverse models").
//!
//! Its values follow the BEV example (`backend/projects/bev-car.json`: a
//! 1927 kg car, 0.3488 m wheels, a 12.8:1 final drive, a 310 N·m / 150 kW
//! motor, a 177 Ah battery) and its structure what a vehicle model needs
//! in both modes:
//!
//! * `TestCar.Body` — mass with the rotating parts lumped in as an
//!   equivalent mass (as the quasi-static method does), rolling resistance
//!   and air drag; its speed is the state fast mode prescribes;
//! * `TestCar.Wheel` — rolling radius and a **grip limit** on the force it
//!   can pass to the road;
//! * `TestCar.Gear` — the final drive, with speed-dependent and drag losses;
//! * `TestCar.Motor` — torque within its full-load curve (310 N·m, then
//!   150 kW) and its **regeneration limit** (80 kW), a loss model, and the
//!   electrical power it draws;
//! * `TestCar.Battery` — OCV(SOC), R0, one RC pair, SOC from charge
//!   counting, its initial SOC a parameter;
//! * `TestCar.Aux` — a constant electrical load;
//! * `TestCar.Brake` — friction brakes with a **capacity limit**;
//! * `TestCar.Driver` — a PI speed controller (full dynamic mode only);
//! * `TestCar.Blend` — splits the driver's torque request between
//!   regeneration (up to a cap) and the friction brakes;
//! * `TestCar.Trace…` — the cycle's speed as a function of time (a sum of
//!   ramps), the driver's reference.
//!
//! Fast mode prescribes `body.v` and frees `blend.tau_req`: the driver and
//! the trace block leave the model and the blend's request becomes the
//! unknown the inverse model solves for.

use lsim_ir::component::build::{connect, eq, param, port, state, sub, var};
use lsim_ir::component::{ComponentDef, EnergyDecl, Library, PortDecl, PortKind, VarDecl};
use lsim_ir::expr::{Builtin, Expr, c, call, der, name as n};
use lsim_ir::{InverseSpec, ParamValue};

fn input(name: &str, unit: &str, doc: &str) -> PortDecl {
    PortDecl { name: name.into(), kind: PortKind::Input { unit: unit.into() }, doc: doc.into() }
}

fn output(name: &str, unit: &str, doc: &str) -> PortDecl {
    PortDecl { name: name.into(), kind: PortKind::Output { unit: unit.into() }, doc: doc.into() }
}

fn guess(mut v: VarDecl, value: f64) -> VarDecl {
    v.start = Some(c(value));
    v
}

fn abs(x: Expr) -> Expr {
    Expr::NoEvent(Box::new(call(Builtin::Abs, vec![x])))
}

fn tanh(x: Expr) -> Expr {
    call(Builtin::Tanh, vec![x])
}

fn min(a: Expr, b: Expr) -> Expr {
    call(Builtin::Min, vec![a, b])
}

fn max(a: Expr, b: Expr) -> Expr {
    call(Builtin::Max, vec![a, b])
}

fn limit(x: Expr, lo: Expr, hi: Expr) -> Expr {
    call(Builtin::Limit, vec![x, lo, hi])
}

/// The test car's component definitions.
pub fn components() -> Vec<ComponentDef> {
    let body = ComponentDef {
        name: "TestCar.Body".into(),
        doc: "Vehicle body: mass (rotating parts lumped in as an equivalent mass), rolling \
              resistance and air drag on a flat road."
            .into(),
        ports: vec![
            port("flange", "TFlange", "where the wheels push it"),
            output("v_out", "m/s", "its speed"),
        ],
        params: vec![
            param("m", "kg", 1927.0, "mass"),
            param("m_rot", "kg", 94.0, "equivalent mass of the rotating parts"),
            param("cr", "1", 0.011, "rolling resistance coefficient"),
            param("cd", "1", 0.27, "drag coefficient"),
            param("area", "m2", 2.31, "frontal area"),
            param("rho", "kg/m3", 1.2, "air density"),
            param("g", "m/s2", 9.80665, "gravity"),
            param("v_eps", "m/s", 0.1, "speed below which rolling resistance fades out"),
        ],
        vars: vec![
            state("v", "m/s", 0.0, "speed"),
            var("a", "m/s2", "acceleration"),
            var("f_roll", "N", "rolling resistance"),
            var("f_drag", "N", "air drag"),
            var("f_road", "N", "road load"),
        ],
        equations: vec![
            eq(n("flange.v"), n("v"), "it moves with its flange"),
            eq(n("a"), der("v"), "its acceleration"),
            eq(
                (n("m") + n("m_rot")) * n("a"),
                n("flange.f") - n("f_road"),
                "Newton's law: the push less the road load",
            ),
            eq(
                n("f_roll"),
                n("cr") * n("m") * n("g") * tanh(n("v") / n("v_eps")),
                "rolling resistance",
            ),
            eq(
                n("f_drag"),
                c(0.5) * n("rho") * n("cd") * n("area") * n("v") * abs(n("v")),
                "air drag",
            ),
            eq(n("f_road"), n("f_roll") + n("f_drag"), "the road load"),
            eq(n("v_out"), n("v"), "it reports its speed"),
        ],
        energy: EnergyDecl {
            stored: Some(c(0.5) * (n("m") + n("m_rot")) * n("v") * n("v")),
            loss: Some(n("f_road") * n("v")),
        },
        ..Default::default()
    };

    let wheel = ComponentDef {
        name: "TestCar.Wheel".into(),
        doc: "The driven wheels: rolling radius, no slip, and the largest force the tyres can \
              pass to the road."
            .into(),
        ports: vec![
            port("axle", "Flange", "the axle"),
            port("road", "TFlange", "the contact with the road (the body)"),
        ],
        params: vec![
            param("r", "m", 0.3488, "rolling radius"),
            param("f_grip", "N", 12000.0, "the largest force the tyres can pass"),
        ],
        vars: vec![var("f_drive", "N", "the force the axle torque asks of the tyres")],
        equations: vec![
            eq(n("road.v"), n("r") * n("axle.w"), "they roll without slip"),
            eq(n("f_drive"), n("axle.tau") / n("r"), "the force the axle torque asks for"),
            eq(
                n("road.f"),
                -limit(n("f_drive"), -n("f_grip"), n("f_grip")),
                "the tyres pass the force within their grip",
            ),
        ],
        energy: EnergyDecl {
            stored: None,
            loss: Some(n("axle.tau") * n("axle.w") + n("road.f") * n("road.v")),
        },
        ..Default::default()
    };

    let gear = ComponentDef {
        name: "TestCar.Gear".into(),
        doc: "Final drive: a fixed ratio with a speed-dependent and a drag loss.".into(),
        ports: vec![port("a", "Flange", "motor side"), port("b", "Flange", "wheel side")],
        params: vec![
            param("ratio", "1", 12.8, "ratio (motor speed / wheel speed)"),
            param("d_loss", "N.m.s/rad", 0.002, "speed-dependent loss"),
            param("tau_drag", "N.m", 0.5, "drag torque"),
            param("w_eps", "rad/s", 1.0, "speed below which the drag fades out"),
        ],
        vars: vec![var("tau_loss", "N.m", "loss torque on the motor side")],
        equations: vec![
            eq(n("a.w"), n("ratio") * n("b.w"), "the motor side turns ratio times faster"),
            eq(
                n("tau_loss"),
                n("d_loss") * n("a.w") + n("tau_drag") * tanh(n("a.w") / n("w_eps")),
                "its loss torque",
            ),
            eq(
                n("ratio") * n("a.tau") + n("b.tau"),
                n("ratio") * n("tau_loss"),
                "torque balance with the loss",
            ),
        ],
        energy: EnergyDecl { stored: None, loss: Some(n("tau_loss") * n("a.w")) },
        ..Default::default()
    };

    let motor = ComponentDef {
        name: "TestCar.Motor".into(),
        doc: "Electric machine: torque on demand within its full-load curve and its \
              regeneration limit, a loss model, and the electrical power it draws."
            .into(),
        ports: vec![
            port("flange", "Flange", "the shaft"),
            port("p", "Pin", "positive terminal"),
            port("n", "Pin", "negative terminal"),
            input("tau_dem", "N.m", "torque demand"),
        ],
        params: vec![
            param("tau_max", "N.m", 310.0, "peak torque"),
            param("p_max", "W", 150e3, "peak power"),
            param("p_regen", "W", 80e3, "peak regeneration power"),
            param("w_eps", "rad/s", 10.0, "speed scale at standstill"),
            param("k0", "W", 200.0, "constant loss when turning"),
            param("kc", "s/(kg.m2)", 0.05, "copper loss per torque squared"),
            param("ki", "W.s2", 0.001, "iron loss per speed squared"),
        ],
        vars: vec![
            var("w", "rad/s", "speed"),
            var("tau_hi", "N.m", "the full-load torque at this speed"),
            var("tau_lo", "N.m", "the regeneration limit at this speed"),
            var("tau", "N.m", "torque"),
            var("p_mech", "W", "mechanical power"),
            var("p_loss", "W", "losses"),
            var("p_elec", "W", "electrical power drawn"),
            guess(var("v", "V", "terminal voltage"), 380.0),
            var("i", "A", "current drawn"),
        ],
        equations: vec![
            eq(n("w"), n("flange.w"), "it turns with its shaft"),
            eq(
                n("tau_hi"),
                min(n("tau_max"), n("p_max") / max(abs(n("w")), n("w_eps"))),
                "its full-load torque: peak torque, then peak power",
            ),
            eq(
                n("tau_lo"),
                -min(n("tau_max"), n("p_regen") / max(abs(n("w")), n("w_eps"))),
                "its regeneration limit",
            ),
            eq(
                n("tau"),
                limit(n("tau_dem"), n("tau_lo"), n("tau_hi")),
                "the torque follows the demand within its limits",
            ),
            eq(n("flange.tau"), -n("tau"), "it drives its shaft"),
            eq(n("p_mech"), n("tau") * n("w"), "mechanical power"),
            eq(
                n("p_loss"),
                n("k0") * tanh(abs(n("w")) / n("w_eps"))
                    + n("kc") * n("tau") * n("tau")
                    + n("ki") * n("w") * n("w"),
                "its losses",
            ),
            eq(n("p_elec"), n("p_mech") + n("p_loss"), "electrical power: mechanical plus losses"),
            eq(n("v"), n("p.v") - n("n.v"), "terminal voltage"),
            eq(c(0.0), n("p.i") + n("n.i"), "the current into p leaves at n"),
            eq(n("i"), n("p.i"), "its current"),
            eq(n("p_elec"), n("v") * n("i"), "it draws its power from its terminals"),
        ],
        energy: EnergyDecl { stored: None, loss: Some(n("p_loss")) },
        ..Default::default()
    };

    let mut soc = state("soc", "1", 0.0, "state of charge");
    soc.start = Some(n("soc0"));
    let battery = ComponentDef {
        name: "TestCar.Battery".into(),
        doc: "Battery: open-circuit voltage linear in SOC, series resistance R0, one RC pair, \
              SOC by charge counting."
            .into(),
        ports: vec![port("p", "Pin", "positive terminal"), port("n", "Pin", "negative terminal")],
        params: vec![
            param("q", "C", 637200.0, "capacity (177 Ah)"),
            param("ocv0", "V", 320.0, "open-circuit voltage when empty"),
            param("ocv1", "V", 80.0, "open-circuit voltage rise from empty to full"),
            param("r0", "Ohm", 0.08, "series resistance"),
            param("r1", "Ohm", 0.03, "RC pair resistance"),
            param("c1", "F", 1000.0, "RC pair capacitance"),
            param("soc0", "1", 0.9, "initial state of charge"),
        ],
        vars: vec![
            soc,
            state("v1", "V", 0.0, "RC pair voltage"),
            var("ocv", "V", "open-circuit voltage"),
            guess(var("i", "A", "discharge current"), 0.0),
            guess(var("v", "V", "terminal voltage"), 380.0),
            var("p_term", "W", "power delivered at the terminals"),
        ],
        equations: vec![
            eq(c(0.0), n("p.i") + n("n.i"), "the current into p leaves at n"),
            eq(n("i"), -n("p.i"), "discharge current leaves at p"),
            eq(n("ocv"), n("ocv0") + n("ocv1") * n("soc"), "open-circuit voltage"),
            eq(n("v"), n("p.v") - n("n.v"), "terminal voltage"),
            eq(n("v"), n("ocv") - n("r0") * n("i") - n("v1"), "OCV less the internal drops"),
            eq(n("c1") * der("v1"), n("i") - n("v1") / n("r1"), "the RC pair"),
            eq(n("q") * der("soc"), -n("i"), "charge counting"),
            eq(n("p_term"), n("v") * n("i"), "terminal power"),
        ],
        ..Default::default()
    };

    let aux = ComponentDef {
        name: "TestCar.Aux".into(),
        doc: "A constant electrical load (control units, pumps, fans).".into(),
        ports: vec![port("p", "Pin", "positive terminal"), port("n", "Pin", "negative terminal")],
        params: vec![param("p_aux", "W", 300.0, "power drawn")],
        vars: vec![guess(var("v", "V", "terminal voltage"), 380.0)],
        equations: vec![
            eq(n("v"), n("p.v") - n("n.v"), "terminal voltage"),
            eq(c(0.0), n("p.i") + n("n.i"), "the current into p leaves at n"),
            eq(n("p_aux"), n("v") * n("p.i"), "it draws its power"),
        ],
        energy: EnergyDecl { stored: None, loss: Some(n("p_aux")) },
        ..Default::default()
    };

    let brake = ComponentDef {
        name: "TestCar.Brake".into(),
        doc: "Friction brakes on the driven axle, within their capacity.".into(),
        ports: vec![port("axle", "Flange", "the axle"), input("tau_dem", "N.m", "torque demand")],
        params: vec![
            param("tau_max", "N.m", 3000.0, "capacity"),
            param("w_eps", "rad/s", 0.5, "speed scale of the reversal at standstill"),
        ],
        vars: vec![var("tau_b", "N.m", "brake torque")],
        equations: vec![
            eq(
                n("tau_b"),
                limit(n("tau_dem"), c(0.0), n("tau_max")),
                "the brake torque follows the demand within the brakes' capacity",
            ),
            eq(
                n("axle.tau"),
                n("tau_b") * tanh((n("axle.w") + n("w_eps")) / n("w_eps")),
                "it opposes forward rotation, and holds at standstill",
            ),
        ],
        energy: EnergyDecl { stored: None, loss: Some(n("axle.tau") * n("axle.w")) },
        ..Default::default()
    };

    let driver = ComponentDef {
        name: "TestCar.Driver".into(),
        doc: "PI speed controller: the torque request (motor side) from the speed error.".into(),
        ports: vec![
            input("v_ref", "m/s", "target speed"),
            input("v", "m/s", "vehicle speed"),
            output("tau_req", "N.m", "torque request, motor side"),
        ],
        params: vec![
            param("kp", "N.m.s/m", 300.0, "proportional gain"),
            param("ki", "N.m/m", 150.0, "integral gain"),
        ],
        vars: vec![state("e_int", "m", 0.0, "integral of the speed error")],
        equations: vec![
            eq(der("e_int"), n("v_ref") - n("v"), "it integrates the error"),
            eq(n("tau_req"), n("kp") * (n("v_ref") - n("v")) + n("ki") * n("e_int"), "PI control"),
        ],
        ..Default::default()
    };

    let blend = ComponentDef {
        name: "TestCar.Blend".into(),
        doc: "Splits a torque request between regeneration (up to a cap) and the friction \
              brakes."
            .into(),
        ports: vec![
            input("tau_req", "N.m", "torque request, motor side"),
            output("motor_dem", "N.m", "motor torque demand"),
            output("brake_dem", "N.m", "friction brake torque demand, wheel side"),
        ],
        params: vec![
            param("regen_cap", "N.m", 120.0, "braking torque left to regeneration"),
            param("ratio", "1", 12.8, "final drive ratio"),
        ],
        equations: vec![
            eq(n("motor_dem"), max(n("tau_req"), -n("regen_cap")), "the motor takes what it can"),
            eq(
                n("brake_dem"),
                n("ratio") * max(c(0.0), -n("tau_req") - n("regen_cap")),
                "the brakes take the rest",
            ),
        ],
        ..Default::default()
    };
    vec![body, wheel, gear, motor, battery, aux, brake, driver, blend]
}

/// A speed trace as a component: `v_ref(t)` piecewise linear through the
/// samples (a sum of ramps, so no events), held after the last sample.
pub fn speed_trace(name: &str, t: &[f64], v: &[f64]) -> ComponentDef {
    assert!(t.len() >= 2 && t.len() == v.len());
    let slope = |k: usize| (v[k + 1] - v[k]) / (t[k + 1] - t[k]);
    let mut terms = vec![];
    let mut prev = 0.0;
    for k in 0..t.len() {
        let s = if k + 1 < t.len() { slope(k) } else { 0.0 };
        let ds = s - prev;
        if ds != 0.0 {
            terms.push(c(ds) * max(Expr::Time - c(t[k]), c(0.0)));
        }
        prev = s;
    }
    // a balanced sum keeps the expression shallow
    fn sum(mut xs: Vec<Expr>) -> Expr {
        match xs.len() {
            0 => c(0.0),
            1 => xs.pop().expect("one"),
            n => {
                let rest = xs.split_off(n / 2);
                sum(xs) + sum(rest)
            }
        }
    }
    ComponentDef {
        name: name.into(),
        doc: "A cycle's speed as a function of time.".into(),
        ports: vec![output("v_ref", "m/s", "target speed")],
        params: vec![param("per_s", "m/s2", 1.0, "unit factor")],
        equations: vec![eq(n("v_ref"), c(v[0]) + n("per_s") * sum(terms), "the cycle's speed")],
        ..Default::default()
    }
}

/// The library: the standard one plus the test car's parts and `trace`.
pub fn library(trace: ComponentDef) -> Library {
    let mut lib = lsim_lib::library();
    for c in components() {
        lib.add(c);
    }
    lib.add(trace);
    lib
}

/// The car with its driver following the trace definition named `trace`.
pub fn car(trace: &str) -> ComponentDef {
    let label = |mut s: lsim_ir::SubDecl, l: &str, id: &str| {
        s.label = Some(l.into());
        s.ui_id = Some(id.into());
        s
    };
    ComponentDef {
        name: "TestCar.Car".into(),
        doc: "A battery-electric test car with its driver.".into(),
        components: vec![
            label(sub("body", "TestCar.Body", &[]), "Vehicle", "el-vehicle"),
            label(sub("wheel", "TestCar.Wheel", &[]), "Wheels", "el-wheel"),
            label(sub("gear", "TestCar.Gear", &[]), "Final Drive", "el-final-drive"),
            label(sub("motor", "TestCar.Motor", &[]), "E-Motor", "el-motor"),
            label(sub("battery", "TestCar.Battery", &[]), "HV Battery", "el-battery"),
            label(sub("aux", "TestCar.Aux", &[]), "Auxiliaries", "el-aux"),
            label(sub("ground", "Electrical.Ground", &[]), "Ground", "el-ground"),
            label(sub("brake", "TestCar.Brake", &[]), "Brakes", "el-brake"),
            label(sub("driver", "TestCar.Driver", &[]), "Driver", "el-driver"),
            label(sub("blend", "TestCar.Blend", &[]), "Brake Blend", "el-blend"),
            label(sub("trace", trace, &[]), "Vehicle Task", "el-task"),
        ],
        connections: vec![
            connect("wheel.road", "body.flange"),
            connect("gear.b", "wheel.axle"),
            connect("brake.axle", "wheel.axle"),
            connect("motor.flange", "gear.a"),
            connect("battery.p", "motor.p"),
            connect("battery.p", "aux.p"),
            connect("battery.n", "motor.n"),
            connect("battery.n", "aux.n"),
            connect("battery.n", "ground.p"),
            connect("trace.v_ref", "driver.v_ref"),
            connect("body.v_out", "driver.v"),
            connect("driver.tau_req", "blend.tau_req"),
            connect("blend.motor_dem", "motor.tau_dem"),
            connect("blend.brake_dem", "brake.tau_dem"),
        ],
        ..Default::default()
    }
}

/// What fast mode prescribes and frees in the test car.
pub fn inverse_spec() -> InverseSpec {
    InverseSpec { prescribed: vec!["body.v".into()], freed: vec!["blend.tau_req".into()] }
}

/// A top-level parameter value: a modifier on one of the car's parts.
pub fn set(top: &mut ComponentDef, part: &str, param: &str, value: f64) {
    let s = top.components.iter_mut().find(|s| s.name == part).expect("a part of the car");
    s.modifiers.retain(|m| m.param != param);
    s.modifiers.push(lsim_ir::Modifier { param: param.into(), value: ParamValue::Real(c(value)) });
}

/// WLTC class 3b (1800 s), from the app's cycle file: times, s, and speeds, m/s.
pub fn wltc() -> (Vec<f64>, Vec<f64>) {
    let text = include_str!("../../../../backend/app/cycles/wltc-3b.csv");
    let mut t = vec![];
    let mut v = vec![];
    for line in text.lines().skip(1) {
        let mut it = line.split(',');
        let (Some(a), Some(b)) = (it.next(), it.next()) else { continue };
        let (Ok(a), Ok(b)) = (a.trim().parse::<f64>(), b.trim().parse::<f64>()) else { continue };
        t.push(a);
        v.push(b / 3.6);
    }
    (t, v)
}

/// A short, hard cycle that runs into every limit of the car: a launch
/// past the motor's power, a stop past its regeneration limit, the brakes'
/// capacity and the tyres' grip, a launch past its peak torque and the
/// grip, and a gentle stop that stays within everything.
pub fn hard_cycle() -> (Vec<f64>, Vec<f64>) {
    let kmh = 1.0 / 3.6;
    let pts = [
        (0.0, 0.0),
        (2.0, 0.0),
        (8.0, 100.0 * kmh),
        (20.0, 100.0 * kmh),
        (24.0, 0.0),
        (30.0, 0.0),
        (31.5, 50.0 * kmh),
        (40.0, 50.0 * kmh),
        (45.0, 0.0),
        (50.0, 0.0),
    ];
    (pts.iter().map(|p| p.0).collect(), pts.iter().map(|p| p.1).collect())
}
