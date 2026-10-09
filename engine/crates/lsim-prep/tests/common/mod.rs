//! What the acceptance tests share: components beyond the Stage 1 library
//! (work package 5 builds the real ones), exact answers, and a harness that
//! compiles a prepared model and simulates it with SUNDIALS.

#![allow(dead_code)] // each test file uses part of it

use lsim_ir::component::build::{eq, param, port, state, var};
use lsim_ir::expr::{Builtin, Expr, c, call, der, name as n};
use lsim_ir::{ComponentDef, Library, PortDecl, PortKind, PreparedModel, VarDecl};
use lsim_solve::{OutputGrid, RunInfo, SimResult, SolverOptions};

/// A signal port.
pub fn signal(name: &str, output: bool, unit: &str) -> PortDecl {
    PortDecl {
        name: name.into(),
        kind: if output {
            PortKind::Output { unit: unit.into() }
        } else {
            PortKind::Input { unit: unit.into() }
        },
        doc: String::new(),
    }
}

/// A variable with a start value that is only a guess.
pub fn guess(name: &str, unit: &str, start: Expr) -> VarDecl {
    VarDecl { start: Some(start), ..var(name, unit, "") }
}

/// A state whose fixed start is an expression of the parameters.
pub fn state_expr(name: &str, unit: &str, start: Expr) -> VarDecl {
    VarDecl { start: Some(start), fixed: true, ..var(name, unit, "") }
}

fn two_pin(name: &str) -> ComponentDef {
    ComponentDef {
        name: name.into(),
        ports: vec![port("p", "Pin", ""), port("n", "Pin", "")],
        vars: vec![var("v", "V", ""), var("i", "A", "")],
        equations: vec![
            eq(n("v"), n("p.v") - n("n.v"), "the voltage across it is p.v - n.v"),
            eq(c(0.0), n("p.i") + n("n.i"), "the current into p leaves at n"),
            eq(n("i"), n("p.i"), "its current is the current into p"),
        ],
        ..Default::default()
    }
}

/// The components the tests add to the Stage 1 library.
pub fn extra() -> Vec<ComponentDef> {
    let torque = ComponentDef {
        name: "Rotational.ConstantTorque".into(),
        ports: vec![port("flange", "Flange", "")],
        params: vec![param("tau", "N.m", 0.0, "the torque it drives with")],
        equations: vec![eq(n("flange.tau"), -n("tau"), "it drives the flange with tau")],
        ..Default::default()
    };
    let gear = ComponentDef {
        name: "Rotational.IdealGear".into(),
        doc: "Lossless gear: a turns ratio times as fast as b.".into(),
        ports: vec![port("a", "Flange", "input"), port("b", "Flange", "output")],
        params: vec![param("ratio", "1", 1.0, "a.w / b.w")],
        equations: vec![
            eq(n("a.w"), n("ratio") * n("b.w"), "a turns ratio times as fast as b"),
            eq(c(0.0), n("ratio") * n("a.tau") + n("b.tau"), "the power through it is kept"),
        ],
        ..Default::default()
    };
    let speed_source = ComponentDef {
        name: "Rotational.ConstantSpeed".into(),
        ports: vec![port("flange", "Flange", "")],
        params: vec![param("w", "rad/s", 0.0, "the speed it holds")],
        equations: vec![eq(n("flange.w"), n("w"), "it holds the flange's speed")],
        ..Default::default()
    };
    let spring_damper = ComponentDef {
        name: "Rotational.SpringDamper".into(),
        ports: vec![port("a", "Flange", ""), port("b", "Flange", "")],
        params: vec![param("k", "N.m/rad", 1.0, ""), param("d", "N.m.s/rad", 0.0, "")],
        vars: vec![state("phi", "rad", 0.0, "twist"), var("tau", "N.m", "")],
        equations: vec![
            eq(der("phi"), n("a.w") - n("b.w"), "it twists with the speed difference"),
            eq(
                n("tau"),
                n("k") * n("phi") + n("d") * (n("a.w") - n("b.w")),
                "spring and damper torque",
            ),
            eq(n("a.tau"), n("tau"), "a takes the torque"),
            eq(n("b.tau"), -n("tau"), "b gives it back"),
        ],
        ..Default::default()
    };
    let mut sine = two_pin("Electrical.SineVoltage");
    sine.params = vec![param("V0", "V", 1.0, "amplitude"), param("f", "1/s", 1.0, "frequency")];
    sine.equations.push(eq(
        n("v"),
        n("V0") * call(Builtin::Sin, vec![c(2.0 * std::f64::consts::PI) * n("f") * Expr::Time]),
        "the source holds a sine voltage",
    ));
    let mut diode = two_pin("Electrical.Diode");
    diode.doc = "Shockley diode: i = Is (exp(v / Vt) - 1).".into();
    diode.params = vec![param("Is", "A", 1e-9, ""), param("Vt", "V", 0.025, "")];
    diode.equations.push(eq(
        n("i"),
        n("Is") * (call(Builtin::Exp, vec![n("v") / n("Vt")]) - c(1.0)),
        "the diode's law",
    ));
    let mut power_load = two_pin("Electrical.PowerLoad");
    power_load.doc = "A load that draws a constant power: v i = P.".into();
    power_load.params = vec![param("P", "W", 1.0, "")];
    power_load.vars[1] = guess("i", "A", c(1.0));
    power_load.equations.push(eq(n("v") * n("i"), n("P"), "it draws its power"));
    let mut current = two_pin("Electrical.ConstantCurrent");
    current.params = vec![param("I", "A", 1.0, "")];
    current.equations.push(eq(n("i"), n("I"), "the source holds its current"));
    let mut conductor = ComponentDef {
        name: "Thermal.Conductor".into(),
        ports: vec![port("a", "HeatPort", ""), port("b", "HeatPort", "")],
        params: vec![param("G", "W/K", 1.0, "")],
        equations: vec![
            eq(n("a.Q"), n("G") * (n("a.T") - n("b.T")), "heat flows down the temperature"),
            eq(c(0.0), n("a.Q") + n("b.Q"), "what enters at a leaves at b"),
        ],
        ..Default::default()
    };
    conductor.doc = "Thermal conductance.".into();
    let fixed_temp = ComponentDef {
        name: "Thermal.FixedTemperature".into(),
        ports: vec![port("port", "HeatPort", "")],
        params: vec![param("T", "K", 293.15, "")],
        equations: vec![eq(n("port.T"), n("T"), "it holds its temperature")],
        ..Default::default()
    };
    let gain = ComponentDef {
        name: "Signal.Gain".into(),
        ports: vec![signal("u", false, "1"), signal("y", true, "1")],
        params: vec![param("k", "1", 1.0, "")],
        equations: vec![eq(n("y"), n("k") * n("u"), "y = k u")],
        ..Default::default()
    };
    let sum = ComponentDef {
        name: "Signal.Add".into(),
        ports: vec![signal("u1", false, "1"), signal("u2", false, "1"), signal("y", true, "1")],
        equations: vec![eq(n("y"), n("u1") + n("u2"), "y = u1 + u2")],
        ..Default::default()
    };
    let constant = ComponentDef {
        name: "Signal.Constant".into(),
        ports: vec![signal("y", true, "1")],
        params: vec![param("k", "1", 1.0, "")],
        equations: vec![eq(n("y"), n("k"), "y = k")],
        ..Default::default()
    };
    let gearbox = ComponentDef {
        name: "Rotational.Gearbox".into(),
        ports: vec![port("a", "Flange", ""), port("b", "Flange", ""), signal("ratio", false, "1")],
        equations: vec![
            eq(n("a.w"), n("ratio") * n("b.w"), "a turns ratio times as fast as b"),
            eq(c(0.0), n("ratio") * n("a.tau") + n("b.tau"), "the power through it is kept"),
        ],
        ..Default::default()
    };
    vec![
        diode,
        power_load,
        torque,
        gear,
        speed_source,
        spring_damper,
        sine,
        current,
        conductor,
        fixed_temp,
        gain,
        sum,
        constant,
        gearbox,
        pendulum(),
    ]
}

/// A pendulum in Cartesian coordinates: index 3.
pub fn pendulum() -> ComponentDef {
    ComponentDef {
        name: "Mechanics.CartesianPendulum".into(),
        doc: "A point mass on a rigid massless rod, in x and y: the rod's length is a \
              constraint on the positions (index 3)."
            .into(),
        params: vec![
            param("m", "kg", 1.0, "mass"),
            param("L", "m", 1.0, "rod length"),
            param("g", "m/s2", 9.81, "gravity"),
            param("theta0", "1", 0.5, "start angle from the vertical"),
        ],
        vars: vec![
            state_expr("x", "m", n("L") * call(Builtin::Sin, vec![n("theta0")])),
            guess("y", "m", -(n("L") * call(Builtin::Cos, vec![n("theta0")]))),
            state("vx", "m/s", 0.0, ""),
            guess("vy", "m/s", c(0.0)),
            guess("F", "N", n("m") * n("g")),
        ],
        equations: vec![
            eq(der("x"), n("vx"), "x changes with vx"),
            eq(der("y"), n("vy"), "y changes with vy"),
            eq(n("m") * der("vx"), -(n("x") * n("F") / n("L")), "the rod pulls along x"),
            eq(
                n("m") * der("vy"),
                -(n("y") * n("F") / n("L")) - n("m") * n("g"),
                "the rod and gravity pull along y",
            ),
            eq(n("x") * n("x") + n("y") * n("y"), n("L") * n("L"), "the rod keeps its length"),
        ],
        ..Default::default()
    }
}

/// The Stage 1 library with the test components.
pub fn library() -> Library {
    let mut lib = lsim_lib::library();
    for d in extra() {
        lib.add(d);
    }
    lib
}

/// Compiles and simulates a prepared model.
pub fn simulate(m: &PreparedModel, t_end: f64, dt: f64, rtol: f64) -> SimResult {
    let jit = lsim_codegen::compile(m, &Default::default()).expect("compiles");
    let info = RunInfo::from_prepared(m);
    let opts = SolverOptions { rtol, atol: rtol, ..Default::default() };
    lsim_solve::simulate(&jit, &info, &opts, OutputGrid { t0: 0.0, t_end, dt }, &mut [])
        .expect("runs")
}

/// The complete elliptic integral of the first kind, K(m) with m = k²,
/// by the arithmetic-geometric mean.
pub fn ellip_k(m: f64) -> f64 {
    let (mut a, mut b) = (1.0f64, (1.0 - m).sqrt());
    for _ in 0..60 {
        let (an, bn) = (0.5 * (a + b), (a * b).sqrt());
        a = an;
        b = bn;
        if (a - b).abs() < 1e-17 * a {
            break;
        }
    }
    std::f64::consts::PI / (2.0 * a)
}

/// The Jacobi elliptic function sn(u | m), by the descending Landen
/// transformation (Abramowitz and Stegun 16.4).
pub fn ellip_sn(u: f64, m: f64) -> f64 {
    let mut a = vec![1.0f64];
    let mut cs = vec![m.sqrt()];
    let mut b = (1.0 - m).sqrt();
    while cs.last().copied().unwrap_or(0.0).abs() > 1e-17 && a.len() < 60 {
        let (al, bl) = (*a.last().unwrap(), b);
        a.push(0.5 * (al + bl));
        cs.push(0.5 * (al - bl));
        b = (al * bl).sqrt();
    }
    let nn = a.len() - 1;
    let mut phi = (1u64 << nn) as f64 * a[nn] * u;
    for k in (1..=nn).rev() {
        phi = 0.5 * (phi + (cs[k] / a[k] * phi.sin()).asin());
    }
    phi.sin()
}

/// The exact angle of a pendulum released from rest at `theta0`.
pub fn pendulum_angle(theta0: f64, g: f64, l: f64, t: f64) -> f64 {
    let k = (0.5 * theta0).sin();
    let m = k * k;
    let w0 = (g / l).sqrt();
    2.0 * (k * ellip_sn(ellip_k(m) - w0 * t, m)).asin()
}

/// The largest difference between a channel and an exact function of
/// time, relative to the exact function's largest magnitude.
pub fn worst(run: &SimResult, channel: &str, exact: impl Fn(f64) -> f64) -> f64 {
    let ch = run.channel(channel).unwrap_or_else(|| panic!("no channel {channel}"));
    let mut scale = 0.0f64;
    let mut err = 0.0f64;
    for (k, &t) in run.times.iter().enumerate() {
        let x = exact(t);
        scale = scale.max(x.abs());
        err = err.max((ch[k] - x).abs());
    }
    err / scale.max(1e-300)
}

/// Every variable's value at t = 0 by the reference interpreter, for a
/// model whose unknowns are all explicit (states take their start values).
pub fn explicit_values(m: &PreparedModel) -> Vec<f64> {
    use lsim_ir::eval::{SliceEnv, eval};
    use lsim_ir::{AliasTarget, Slot};
    assert!(m.algebraics.is_empty(), "explicit models only");
    let n = m.flat.vars.len();
    let p: Vec<f64> = m.flat.params.iter().map(|q| q.value).collect();
    let mut vars = vec![f64::NAN; n];
    let mut ders = vec![f64::NAN; n];
    for v in m.states.iter().chain(&m.discretes) {
        vars[v.0 as usize] = m.flat.var(*v).start.unwrap_or(0.0);
    }
    for a in &m.assignments {
        let x = eval(&a.expr, &SliceEnv { t: 0.0, vars: &vars, ders: &ders, params: &p });
        match a.target {
            Slot::Var(v) => vars[v.0 as usize] = x,
            Slot::Der(v) => ders[v.0 as usize] = x,
        }
    }
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

/// A variable's value by name in [`explicit_values`].
pub fn value_of(m: &PreparedModel, vals: &[f64], name: &str) -> f64 {
    vals[m.flat.find_var(name).unwrap_or_else(|| panic!("no variable {name}")).0 as usize]
}
