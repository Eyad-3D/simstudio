//! The Stage 1 end-to-end spike: a battery (OCV + R0 + one RC pair) drives
//! a DC machine spinning an inertia with viscous loss; a brake clamps on
//! when the speed reaches 300 rad/s (a state event). Built from library
//! components with hierarchy (the battery is a composite), it exercises
//! flattening, connection sets, alias elimination, sorting, explicit
//! solving, Cranelift code generation, the exact Jacobian, event location
//! and re-initialisation. The same circuit, reduced by hand to a linear
//! ODE, has a closed-form solution ([`Exact`]) — the yardstick.
//!
//! It is stiff: the RC pair's pole is at about -12 000 1/s, the mechanics'
//! at about -0.84 1/s.

use lsim_ir::ComponentDef;
use lsim_ir::component::build::{connect, sub};
use lsim_ir::expr::c;

/// The circuit's values.
#[derive(Clone, Copy, Debug)]
pub struct Params {
    /// open-circuit voltage, V
    pub ocv: f64,
    /// series resistance, Ohm
    pub r0: f64,
    /// RC pair resistance, Ohm
    pub r1: f64,
    /// RC pair capacitance, F
    pub c1: f64,
    /// machine constant, N.m/A
    pub k: f64,
    /// inertia, kg.m2
    pub j: f64,
    /// viscous loss, N.m.s/rad
    pub b: f64,
    /// brake torque once engaged, N.m
    pub tb: f64,
    /// the speed the brake engages at, rad/s
    pub w_on: f64,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            ocv: 400.0,
            r0: 0.05,
            r1: 0.01,
            c1: 0.01,
            k: 1.0,
            j: 20.0,
            b: 0.05,
            tb: 2000.0,
            w_on: 300.0,
        }
    }
}

/// The model, as a project's diagram would make it.
pub fn battery_drive(p: &Params) -> ComponentDef {
    let mut battery = sub(
        "battery",
        "Battery.OcvR0Rc",
        &[("ocv", c(p.ocv)), ("R0", c(p.r0)), ("R1", c(p.r1)), ("C1", c(p.c1))],
    );
    battery.label = Some("HV Battery".into());
    let mut motor = sub("motor", "Electrical.Emf", &[("k", c(p.k))]);
    motor.label = Some("Drive Motor".into());
    let mut inertia = sub("inertia", "Rotational.Inertia", &[("J", c(p.j))]);
    inertia.label = Some("Rotor and Load".into());
    let mut loss = sub("loss", "Rotational.Damper", &[("d", c(p.b))]);
    loss.label = Some("Windage".into());
    let mut brake =
        sub("brake", "Rotational.ThresholdBrake", &[("tau_max", c(p.tb)), ("w_on", c(p.w_on))]);
    brake.label = Some("Overspeed Brake".into());
    ComponentDef {
        name: "Spike.BatteryDrive".into(),
        doc: "Stage 1 spike: battery, DC machine, inertia, loss and an overspeed brake.".into(),
        components: vec![
            battery,
            motor,
            inertia,
            loss,
            brake,
            sub("ground", "Electrical.Ground", &[]),
        ],
        connections: vec![
            connect("battery.p", "motor.p"),
            connect("motor.n", "battery.n"),
            connect("battery.n", "ground.p"),
            connect("motor.flange", "inertia.a"),
            connect("inertia.b", "loss.flange"),
            connect("inertia.b", "brake.flange"),
        ],
        ..Default::default()
    }
}

/// The channels compared with the exact answer: RC voltage, speed, current.
pub const CHANNELS: [&str; 3] = ["battery.c1.v", "inertia.w", "motor.i"];

/// Closed-form solution of x' = A x + c on one side of the event.
#[derive(Clone, Copy, Debug)]
pub struct Segment {
    t0: f64,
    x0: [f64; 2],
    /// the equilibrium
    pub xeq: [f64; 2],
    /// the fast pole, 1/s
    pub l1: f64,
    /// the slow pole, 1/s
    pub l2: f64,
    a: [[f64; 2]; 2],
}

impl Segment {
    fn new(p: &Params, t0: f64, x0: [f64; 2], s: f64) -> Self {
        let g = 1.0 / p.r0;
        let a = [
            [-(g + 1.0 / p.r1) / p.c1, -g * p.k / p.c1],
            [-p.k * g / p.j, -(p.k * p.k * g + p.b) / p.j],
        ];
        let c = [p.ocv * g / p.c1, (p.k * p.ocv * g - p.tb * s) / p.j];
        let det = a[0][0] * a[1][1] - a[0][1] * a[1][0];
        let xeq =
            [-(a[1][1] * c[0] - a[0][1] * c[1]) / det, -(-a[1][0] * c[0] + a[0][0] * c[1]) / det];
        let tr = a[0][0] + a[1][1];
        let disc = (0.25 * tr * tr - det).sqrt();
        let l1 = 0.5 * tr - disc;
        let l2 = det / l1;
        Segment { t0, x0, xeq, l1, l2, a }
    }

    /// (v1, w) at t, via exp(At) = p(t) I + q(t) A.
    pub fn at(&self, t: f64) -> [f64; 2] {
        let tau = t - self.t0;
        let d = [self.x0[0] - self.xeq[0], self.x0[1] - self.xeq[1]];
        let (e1, e2) = ((self.l1 * tau).exp(), (self.l2 * tau).exp());
        let q = (e1 - e2) / (self.l1 - self.l2);
        let pp = (self.l1 * e2 - self.l2 * e1) / (self.l1 - self.l2);
        let a = self.a;
        [
            self.xeq[0] + pp * d[0] + q * (a[0][0] * d[0] + a[0][1] * d[1]),
            self.xeq[1] + pp * d[1] + q * (a[1][0] * d[0] + a[1][1] * d[1]),
        ]
    }

    fn dw(&self, t: f64) -> f64 {
        let x = self.at(t);
        let a = self.a;
        a[1][0] * (x[0] - self.xeq[0]) + a[1][1] * (x[1] - self.xeq[1])
    }
}

/// The exact answer: two segments split at the brake event.
#[derive(Clone, Copy, Debug)]
pub struct Exact {
    p: Params,
    /// before the brake engages
    pub before: Segment,
    /// after
    pub after: Segment,
    /// when it engages, s
    pub t_event: f64,
}

impl Exact {
    /// Solves the circuit in closed form.
    pub fn new(p: &Params) -> Self {
        let before = Segment::new(p, 0.0, [0.0, 0.0], 0.0);
        let mut t = -(1.0 - p.w_on / before.xeq[1]).ln() / (-before.l2);
        for _ in 0..60 {
            let step = (before.at(t)[1] - p.w_on) / before.dw(t);
            t -= step;
            if step.abs() < 1e-16 * t.abs() {
                break;
            }
        }
        let xe = before.at(t);
        let after = Segment::new(p, t, [xe[0], p.w_on], 1.0);
        Exact { p: *p, before, after, t_event: t }
    }

    /// (v1, w, i) at t; at the event time, the value after it.
    pub fn at(&self, t: f64) -> [f64; 3] {
        let x = if t < self.t_event { self.before.at(t) } else { self.after.at(t) };
        [x[0], x[1], (self.p.ocv - x[0] - self.p.k * x[1]) / self.p.r0]
    }
}
