//! Prepared models built directly (test doubles of lsim-prep's output) for
//! the code generator's tests and benchmarks: a vehicle-like model of the
//! example cars' size and make-up, large thermal networks, and a builder.
#![allow(dead_code)]

use lsim_ir::component::VarKind;
use lsim_ir::expr::{BinaryOp, Builtin, CmpOp, Expr};
use lsim_ir::flat::*;
use lsim_ir::prepared::*;
use lsim_ir::table::{FlatTable, Outside, TableData};
use lsim_ir::units::parse_unit;

/// Builds a prepared model by hand.
pub struct Builder {
    pub m: PreparedModel,
}

pub fn origin() -> Origin {
    Origin { instance: InstanceId(0), kind: OriginKind::Component { index: 0 }, label: None }
}

impl Default for Builder {
    fn default() -> Self {
        Self::new()
    }
}

impl Builder {
    pub fn new() -> Builder {
        let mut flat = FlatSystem::default();
        flat.instances.push(Instance {
            path: String::new(),
            def: "Synthetic".into(),
            parent: None,
            label: None,
            ui_id: None,
        });
        Builder {
            m: PreparedModel {
                flat,
                states: vec![],
                algebraics: vec![],
                discretes: vec![],
                inputs: vec![],
                external: vec![],
                assignments: vec![],
                residuals: vec![],
                aliases: vec![],
                zero_crossings: vec![],
                whens: vec![],
                structure_key: String::new(),
                stats: Default::default(),
                jac_pattern: Default::default(),
                init: Default::default(),
                modes: vec![],
            },
        }
    }

    fn new_var(&mut self, name: &str, kind: VarKind, start: f64) -> VarId {
        let id = VarId(self.m.flat.vars.len() as u32);
        self.m.flat.vars.push(FlatVar {
            name: name.into(),
            unit: parse_unit("1").unwrap(),
            unit_text: "1".into(),
            kind,
            start: Some(start),
            fixed: false,
            nominal: 1.0,
            instance: InstanceId(0),
            role: VarRole::Local,
        });
        id
    }

    /// A variable computed by an assignment (call `set` next).
    pub fn var(&mut self, name: &str) -> VarId {
        self.new_var(name, VarKind::Continuous, 0.0)
    }

    pub fn state(&mut self, name: &str, start: f64) -> VarId {
        let v = self.new_var(name, VarKind::Continuous, start);
        self.m.states.push(v);
        v
    }

    /// An iteration variable (z): an unknown with a residual.
    pub fn algebraic(&mut self, name: &str, start: f64) -> VarId {
        let v = self.new_var(name, VarKind::Continuous, start);
        self.m.algebraics.push(Slot::Var(v));
        v
    }

    pub fn discrete(&mut self, name: &str, start: f64) -> VarId {
        let v = self.new_var(name, VarKind::Discrete, start);
        self.m.discretes.push(v);
        v
    }

    pub fn input(&mut self, name: &str) -> VarId {
        let v = self.new_var(name, VarKind::Continuous, 0.0);
        self.m.inputs.push(v);
        v
    }

    pub fn param(&mut self, name: &str, value: f64) -> Expr {
        let id = ParamId(self.m.flat.params.len() as u32);
        self.m.flat.params.push(FlatParam {
            name: name.into(),
            unit: parse_unit("1").unwrap(),
            value,
            binding: None,
            structural: false,
            instance: InstanceId(0),
        });
        Expr::Param(id)
    }

    pub fn table(&mut self, name: &str, data: TableData) -> u32 {
        let k = self.m.flat.tables.len() as u32;
        self.m.flat.tables.push(FlatTable { name: name.into(), instance: InstanceId(0), data });
        k
    }

    /// v := e
    pub fn set(&mut self, v: VarId, e: Expr) -> Expr {
        self.m.assignments.push(Assignment { target: Slot::Var(v), expr: e, origin: origin() });
        Expr::Var(v)
    }

    /// A new variable `name` := e.
    pub fn let_(&mut self, name: &str, e: Expr) -> Expr {
        let v = self.var(name);
        self.set(v, e)
    }

    /// der(x) := e
    pub fn der(&mut self, x: VarId, e: Expr) {
        self.m.assignments.push(Assignment { target: Slot::Der(x), expr: e, origin: origin() });
    }

    pub fn residual(&mut self, e: Expr) {
        self.m.residuals.push(Residual { expr: e, origin: origin() });
    }

    pub fn crossing(&mut self, e: Expr) -> usize {
        self.m.zero_crossings.push(ZeroCrossing { expr: e, origin: origin() });
        self.m.zero_crossings.len() - 1
    }

    /// A mode: discrete Boolean `name` holding `relation`, with its crossing.
    pub fn mode(&mut self, name: &str, relation: Expr, crossing: Expr, start: f64) -> Expr {
        let d = self.discrete(name, start);
        let c = self.crossing(crossing);
        self.m.modes.push(Mode { var: d, relation, crossing: c, origin: origin() });
        Expr::Var(d)
    }

    pub fn finish(self) -> PreparedModel {
        self.m
    }
}

pub fn v(x: VarId) -> Expr {
    Expr::Var(x)
}
pub fn c(x: f64) -> Expr {
    Expr::Const(x)
}
pub fn f1(b: Builtin, a: Expr) -> Expr {
    Expr::Call(b, vec![a])
}
pub fn f2(b: Builtin, a: Expr, x: Expr) -> Expr {
    Expr::Call(b, vec![a, x])
}
pub fn pow(a: Expr, b: Expr) -> Expr {
    Expr::bin(BinaryOp::Pow, a, b)
}
pub fn lim(x: Expr, lo: Expr, hi: Expr) -> Expr {
    Expr::Call(Builtin::Limit, vec![x, lo, hi])
}
pub fn gt(a: Expr, b: Expr) -> Expr {
    Expr::Compare(CmpOp::Gt, Box::new(a), Box::new(b))
}
pub fn lt(a: Expr, b: Expr) -> Expr {
    Expr::Compare(CmpOp::Lt, Box::new(a), Box::new(b))
}
pub fn if_(c: Expr, a: Expr, b: Expr) -> Expr {
    Expr::If(Box::new(c), Box::new(a), Box::new(b))
}
pub fn noev(a: Expr) -> Expr {
    Expr::NoEvent(Box::new(a))
}
pub fn tab(k: u32, args: Vec<Expr>) -> Expr {
    Expr::Table { table: k, args }
}

/// A smooth, monotone-ish 1-D curve sampled on `n` points over [a, b].
fn curve(n: usize, a: f64, b: f64, f: impl Fn(f64) -> f64) -> TableData {
    let x: Vec<f64> = (0..n).map(|i| a + (b - a) * (i as f64 / (n - 1) as f64).powf(1.2)).collect();
    let y = x.iter().map(|&t| f(t)).collect();
    TableData::new_1d(x, y)
}

fn grid(
    nx: usize,
    ny: usize,
    ax: (f64, f64),
    ay: (f64, f64),
    f: impl Fn(f64, f64) -> f64,
) -> TableData {
    let x: Vec<f64> = (0..nx).map(|i| ax.0 + (ax.1 - ax.0) * i as f64 / (nx - 1) as f64).collect();
    let y: Vec<f64> = (0..ny).map(|j| ay.0 + (ay.1 - ay.0) * j as f64 / (ny - 1) as f64).collect();
    let mut vals = vec![];
    for &a in &x {
        for &b in &y {
            vals.push(f(a, b));
        }
    }
    TableData::new_2d(x, y, vals)
}

/// One electric drive unit: a battery-fed motor with its maps, a
/// driveline to two wheels with tyres and brakes, and their temperatures.
/// Returns the wheels' traction force.
fn drive_unit(b: &mut Builder, n: usize, cmd: &Expr, v_body: &Expr, amb: &Expr) -> Expr {
    let s = |x: &str| format!("u{n}.{x}");
    // battery: SOC and RC states, OCV table, current from power (a loop
    // kept implicit: the current is an iteration variable)
    let soc = b.state(&s("soc"), 0.8);
    let vrc = b.state(&s("v_rc"), 0.0);
    let tbat = b.state(&s("T_bat"), 298.0);
    let ocv_t =
        b.table(&s("ocv"), curve(12, 0.0, 1.0, |q| 3.0 + 1.2 * q - 0.4 * (1.0 - q).powi(3)));
    let r0_t = b.table(
        &s("r0"),
        grid(5, 6, (0.0, 1.0), (253.0, 333.0), |q, t| {
            0.002 * (1.0 + 0.5 * (1.0 - q)) * (1.0 + 3.0 * (-(t - 253.0) / 30.0).exp())
        }),
    );
    let cells = b.param(&s("cells"), 96.0);
    let cap = b.param(&s("capacity"), 60.0 * 3600.0);
    let r1 = b.param(&s("r1"), 0.01);
    let c1 = b.param(&s("c1"), 2000.0);
    let i = b.algebraic(&s("i"), 10.0);
    let ocv = b.let_(&s("ocv_v"), cells.clone() * tab(ocv_t, vec![v(soc)]));
    let r0 = b.let_(&s("r0_v"), cells.clone() * tab(r0_t, vec![v(soc), v(tbat)]));
    let vt = b.let_(&s("v_term"), ocv.clone() - r0.clone() * v(i) - v(vrc));
    b.der(soc, -v(i) / cap.clone());
    b.der(vrc, (v(i) - v(vrc) / r1.clone()) / c1.clone());
    let loss_b = b.let_(&s("loss_bat"), r0 * v(i) * v(i) + v(vrc) * v(vrc) / r1);
    // motor: speed from the wheels, torque limited by its full-load map at
    // the terminal voltage, losses from a 2-D map
    let ratio = b.param(&s("ratio"), 9.0);
    let r_w = b.param(&s("r_wheel"), 0.33);
    let w_l = b.state(&s("w_left"), 1.0);
    let w_r = b.state(&s("w_right"), 1.0);
    let w_m = b.let_(&s("w_motor"), ratio.clone() * (v(w_l) + v(w_r)) * c(0.5));
    let fl_t = b.table(
        &s("full_load"),
        grid(3, 20, (250.0, 400.0), (0.0, 1500.0), |u, w| {
            (300.0 * u / 350.0) * (1.0 / (1.0 + (w / 600.0).powi(2))).sqrt()
        }),
    );
    let loss_t = b.table(
        &s("loss_map"),
        grid(12, 9, (0.0, 1500.0), (0.0, 350.0), |w, t| {
            200.0 + 0.02 * w * w * 0.01 + 0.03 * t * t + 0.5 * w
        }),
    );
    let drag_t = b.table(&s("drag"), curve(6, 0.0, 1500.0, |w| 0.5 + 0.002 * w));
    let tmax =
        b.let_(&s("t_max"), tab(fl_t, vec![vt.clone(), noev(f1(Builtin::Abs, w_m.clone()))]));
    let t_dem = b.let_(&s("t_demand"), cmd.clone() * tmax.clone());
    let t_m = b.let_(
        &s("t_motor"),
        lim(t_dem, -tmax.clone(), tmax) - tab(drag_t, vec![noev(f1(Builtin::Abs, w_m.clone()))]),
    );
    let p_mech = b.let_(&s("p_mech"), t_m.clone() * w_m.clone());
    let p_loss = b.let_(
        &s("p_loss_motor"),
        tab(loss_t, vec![noev(f1(Builtin::Abs, w_m.clone())), noev(f1(Builtin::Abs, t_m.clone()))]),
    );
    let p_el = b.let_(&s("p_el"), p_mech.clone() + p_loss.clone());
    // the battery delivers the electrical power: v_term i = p_el
    b.residual(vt.clone() * v(i) - p_el.clone());
    // regeneration limit as a mode (held between events)
    let regen = b.mode(&s("regen"), lt(p_el.clone(), c(0.0)), p_el.clone(), 0.0);
    let p_aux = b.let_(&s("p_aux"), if_(regen, c(0.0), c(300.0)));
    let _ = b.let_(&s("p_total"), p_el + p_aux);
    // thermal: motor and battery temperatures
    let tm = b.state(&s("T_motor"), 298.0);
    let h = b.param(&s("h"), 15.0);
    let cm = b.param(&s("C_motor"), 20_000.0);
    let cb = b.param(&s("C_bat"), 200_000.0);
    let sigma_a = b.param(&s("rad"), 5.67e-8 * 0.3);
    let q_rad = b.let_(&s("q_rad"), sigma_a * (pow(v(tm), c(4.0)) - pow(amb.clone(), c(4.0))));
    b.der(tm, (p_loss - h.clone() * (v(tm) - amb.clone()) - q_rad) / cm);
    b.der(tbat, (loss_b - h * (v(tbat) - amb.clone())) / cb);
    // the driveline to two wheels: tyre slip forces (Pacejka-like),
    // brakes as smooth friction, wheel inertias
    let jw = b.param(&s("J_wheel"), 1.2);
    let fz = b.param(&s("Fz"), 4000.0);
    let (bb, cc, dd) = (b.param(&s("B"), 10.0), b.param(&s("C"), 1.9), b.param(&s("D"), 1.0));
    let t_brake_max = b.param(&s("T_brake"), 1500.0);
    let brake = b.let_(&s("brake_cmd"), noev(f2(Builtin::Max, -cmd.clone(), c(0.0))));
    let mut force = c(0.0);
    for (side, w) in [("l", w_l), ("r", w_r)] {
        let vw = b.let_(&s(&format!("v_{side}")), v(w) * r_w.clone());
        let slip = b.let_(
            &s(&format!("slip_{side}")),
            (vw.clone() - v_body.clone())
                / noev(f2(Builtin::Max, f1(Builtin::Abs, v_body.clone()), c(1.0))),
        );
        let fx = b.let_(
            &s(&format!("Fx_{side}")),
            fz.clone()
                * dd.clone()
                * f1(Builtin::Sin, cc.clone() * f1(Builtin::Atan, bb.clone() * slip)),
        );
        let tb = b.let_(
            &s(&format!("T_brake_{side}")),
            brake.clone() * t_brake_max.clone() * f1(Builtin::Tanh, v(w) / c(0.5)),
        );
        let t_axle = t_m.clone() * ratio.clone() * c(0.5);
        b.der(w, (t_axle - tb - fx.clone() * r_w.clone()) / jw.clone());
        let _ = b.let_(&s(&format!("p_slip_{side}")), fx.clone() * (vw - v_body.clone()));
        force = force + fx;
    }
    b.let_(&s("force"), force)
}

/// A vehicle-like model: `units` drive units (one or two per car: front
/// and rear), a body with road loads and grade, a PI driver following a
/// drive cycle table (uniform 1 s grid, like WLTC's 1801 points), climate
/// and auxiliaries, modes and a `when` clause, and monitor channels.
pub fn vehicle(units: usize) -> PreparedModel {
    let mut b = Builder::new();
    let cycle = b.table(
        "cycle.speed",
        TableData::new_1d(
            (0..1801).map(|k| k as f64).collect(),
            (0..1801)
                .map(|k| {
                    let t = k as f64;
                    (14.0 * (t / 120.0).sin().powi(2) + 6.0 * (t / 37.0).sin().abs()).max(0.0)
                })
                .collect(),
        ),
    );
    let grade_t = b.table("road.grade", curve(40, 0.0, 30_000.0, |s| 0.03 * (s / 2000.0).sin()));
    let mut climate =
        curve(10, -20.0, 40.0, |t| 3000.0 - 120.0 * (t + 20.0) + 2.0 * (t - 10.0).powi(2));
    climate.outside = [Outside::Linear, Outside::Clamp];
    let climate_t = b.table("climate.demand", climate);
    let vb = b.state("body.v", 0.0);
    let xb = b.state("body.x", 0.0);
    let e_int = b.state("driver.e_int", 0.0);
    let gear = b.discrete("gearbox.gear", 1.0);
    let amb = b.param("ambient.T", 293.0);
    let mass = b.param("body.mass", 1800.0 + 300.0 * units as f64);
    let cda = b.param("body.CdA", 0.62);
    let rho = b.param("ambient.rho", 1.2);
    let crr = b.param("body.crr", 0.009);
    let kp = b.param("driver.kp", 0.4);
    let ki = b.param("driver.ki", 0.05);
    let v_ref = b.let_("driver.v_ref", tab(cycle, vec![Expr::Time]));
    let err = b.let_("driver.error", v_ref.clone() - v(vb));
    let u = b.let_("driver.u", kp * err.clone() + ki * v(e_int));
    let cmd = b.let_("driver.cmd", lim(u.clone(), c(-1.0), c(1.0)));
    let saturated = b.mode(
        "driver.saturated",
        gt(f1(Builtin::Abs, u.clone()), c(1.0)),
        f1(Builtin::Abs, u) - c(1.0),
        0.0,
    );
    b.der(e_int, if_(saturated, c(0.0), err));
    let mut traction = c(0.0);
    for n in 0..units {
        let f = drive_unit(&mut b, n, &cmd, &v(vb), &amb);
        traction = traction + f;
    }
    let grade = b.let_("road.grade_v", tab(grade_t, vec![v(xb)]));
    let f_aero = b.let_("body.F_aero", c(0.5) * rho * cda * v(vb) * f1(Builtin::Abs, v(vb)));
    let f_roll =
        b.let_("body.F_roll", crr * mass.clone() * c(9.80665) * f1(Builtin::Tanh, v(vb) / c(0.1)));
    let f_grade = b.let_(
        "body.F_grade",
        mass.clone() * c(9.80665) * f1(Builtin::Sin, f1(Builtin::Atan, grade)),
    );
    b.der(vb, (traction - f_aero.clone() - f_roll.clone() - f_grade) / mass);
    b.der(xb, v(vb));
    let p_clim = b.let_("climate.p", tab(climate_t, vec![amb.clone() - c(273.15)]));
    // monitor channels (output only: residual code never computes them)
    let _ = b.let_("monitor.p_aero", f_aero * v(vb));
    let _ = b.let_("monitor.p_roll", f_roll * v(vb));
    let _ = b.let_("monitor.kmh", v(vb) * c(3.6));
    let _ = b.let_("monitor.p_clim_kw", p_clim / c(1000.0));
    // a gear shift as a when clause
    let zc = b.crossing(v(vb) - c(15.0));
    b.m.whens.push(PreparedWhen {
        crossing: zc,
        direction: Direction::Rising,
        assign: vec![(gear, Expr::Pre(gear) + c(1.0))],
        origin: origin(),
    });
    b.finish()
}

/// A thermal network of `n` nodes (states), each linked to a few others
/// by nonlinear conductances, with radiation, heat sources from tables and
/// output channels: about `5 n` assignments, `n` states.
pub fn network(n: usize, seed: u64) -> PreparedModel {
    let mut rng = Rng(seed);
    let mut b = Builder::new();
    let src_t = b.table("source", curve(20, 0.0, 1000.0, |t| 100.0 + 50.0 * (t / 100.0).sin()));
    let g0 = b.param("g0", 0.8);
    let alpha = b.param("alpha", 0.004);
    let cap = b.param("C", 500.0);
    let eps = b.param("eps", 5.67e-8 * 0.2);
    let amb = b.param("T_amb", 290.0);
    let nodes: Vec<VarId> =
        (0..n).map(|i| b.state(&format!("T{i}"), 290.0 + (i % 17) as f64)).collect();
    let mut inflow: Vec<Expr> = vec![c(0.0); n];
    for i in 0..n {
        // links to 2 neighbours: one local, one random
        for j in [(i + 1) % n, rng.below(n)] {
            if j == i {
                continue;
            }
            let dt = v(nodes[i]) - v(nodes[j]);
            let g = g0.clone() * (c(1.0) + alpha.clone() * (v(nodes[i]) + v(nodes[j])) * c(0.5));
            let q = b.let_(&format!("q{i}_{j}"), g * dt);
            inflow[j] = inflow[j].clone() + q.clone();
            inflow[i] = inflow[i].clone() - q;
        }
    }
    for i in 0..n {
        let rad = b.let_(
            &format!("rad{i}"),
            eps.clone() * (pow(v(nodes[i]), c(4.0)) - pow(amb.clone(), c(4.0))),
        );
        let src = b.let_(
            &format!("src{i}"),
            if i % 10 == 0 { tab(src_t, vec![Expr::Time]) } else { c(0.0) },
        );
        let net = b.let_(&format!("net{i}"), std::mem::replace(&mut inflow[i], c(0.0)) - rad + src);
        b.der(nodes[i], net / cap.clone());
        let _ = b.let_(&format!("out{i}"), v(nodes[i]) - c(273.15));
    }
    b.finish()
}

/// A small deterministic generator (xorshift).
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        let mut x = self.0.max(1);
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    pub fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    pub fn range(&mut self, a: f64, b: f64) -> f64 {
        a + (b - a) * self.unit()
    }
}

/// The parameter values of a model.
pub fn params(m: &PreparedModel) -> Vec<f64> {
    m.flat.params.iter().map(|p| p.value).collect()
}
