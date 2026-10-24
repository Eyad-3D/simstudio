//! Work package 2's acceptance for index reduction through tables: index-2
//! and index-3 models whose constraints read 1-D and 2-D tables prepare (the
//! constraint differentiated through the interpolant, its derivatives
//! exact) and simulate to answers computed here independently of the
//! engine: in closed form, or from this file's own monotone cubic
//! (Steffen's slopes, Hermite cubics) by quadrature or a fine fixed-step
//! integration. A table that is not smooth enough for how often its
//! constraint must be differentiated is refused, with the reason.

mod common;

use common::*;
use lsim_ir::component::build::{connect, eq, param, port, state, sub, var};
use lsim_ir::expr::{Builtin, Expr, c, call, der, name as n};
use lsim_ir::{
    ComponentDef, Interpolation, Library, Modifier, Outside, ParamDecl, ParamValue, Severity,
    SubDecl, TableData,
};
use lsim_prep::{PrepReport, Settings, prepare_with_report};
use lsim_solve::SimResult;

// ---------------------------------------------------------------------
// an independent monotone cubic: Steffen (1990), Hermite basis
// ---------------------------------------------------------------------

/// A monotone cubic interpolant built here from the published method,
/// evaluated in the Hermite basis (the engine uses a power basis).
struct Cubic {
    x: Vec<f64>,
    f: Vec<f64>,
    d: Vec<f64>,
}

impl Cubic {
    fn steffen(x: &[f64], f: &[f64]) -> Cubic {
        let n = x.len();
        let h: Vec<f64> = (0..n - 1).map(|i| x[i + 1] - x[i]).collect();
        let s: Vec<f64> = (0..n - 1).map(|i| (f[i + 1] - f[i]) / h[i]).collect();
        let sgn = |v: f64| {
            if v > 0.0 {
                1.0
            } else if v < 0.0 {
                -1.0
            } else {
                0.0
            }
        };
        let mut d = vec![0.0; n];
        for i in 1..n - 1 {
            // Steffen eq. 11-13: the parabola's slope, limited
            let p = (s[i - 1] * h[i] + s[i] * h[i - 1]) / (h[i - 1] + h[i]);
            d[i] = (sgn(s[i - 1]) + sgn(s[i])) * s[i - 1].abs().min(s[i].abs()).min(0.5 * p.abs());
        }
        // eq. 26-27 at both ends
        let end = |s0: f64, s1: f64, h0: f64, h1: f64| {
            let p = s0 * (1.0 + h0 / (h0 + h1)) - s1 * h0 / (h0 + h1);
            if p * s0 <= 0.0 {
                0.0
            } else if p.abs() > 2.0 * s0.abs() {
                2.0 * s0
            } else {
                p
            }
        };
        d[0] = end(s[0], s[1], h[0], h[1]);
        d[n - 1] = end(s[n - 2], s[n - 3], h[n - 2], h[n - 3]);
        Cubic { x: x.to_vec(), f: f.to_vec(), d }
    }

    /// Piecewise linear through the data (its slopes unused).
    fn linear(x: &[f64], f: &[f64]) -> Cubic {
        Cubic { x: x.to_vec(), f: f.to_vec(), d: vec![] }
    }

    /// The cell holding `t` (the right one at a breakpoint).
    fn cell(&self, t: f64) -> usize {
        let n = self.x.len();
        (0..n - 1).rev().find(|&i| t >= self.x[i]).unwrap_or(0)
    }

    /// Value, first and second derivative at `t` inside the data.
    fn at(&self, t: f64) -> [f64; 3] {
        let i = self.cell(t);
        let h = self.x[i + 1] - self.x[i];
        let (f0, f1) = (self.f[i], self.f[i + 1]);
        if self.d.is_empty() {
            let m = (f1 - f0) / h;
            return [f0 + m * (t - self.x[i]), m, 0.0];
        }
        let (d0, d1) = (self.d[i] * h, self.d[i + 1] * h);
        let u = (t - self.x[i]) / h;
        let (u2, u3) = (u * u, u * u * u);
        let v = (2.0 * u3 - 3.0 * u2 + 1.0) * f0
            + (u3 - 2.0 * u2 + u) * d0
            + (-2.0 * u3 + 3.0 * u2) * f1
            + (u3 - u2) * d1;
        let du = (6.0 * u2 - 6.0 * u) * f0
            + (3.0 * u2 - 4.0 * u + 1.0) * d0
            + (-6.0 * u2 + 6.0 * u) * f1
            + (3.0 * u2 - 2.0 * u) * d1;
        let ddu = (12.0 * u - 6.0) * f0
            + (6.0 * u - 4.0) * d0
            + (-12.0 * u + 6.0) * f1
            + (6.0 * u - 2.0) * d1;
        [v, du / h, ddu / (h * h)]
    }
}

/// ∫_a^b f by 20-point Gauss–Legendre on each piece between `cuts`.
fn integrate(f: &dyn Fn(f64) -> f64, a: f64, b: f64, cuts: &[f64]) -> f64 {
    // nodes and weights on [-1, 1] by Newton on the Legendre polynomial
    let n = 20;
    let mut nodes = vec![];
    for k in 0..n {
        let mut x = lsim_ir::math::cos(std::f64::consts::PI * (k as f64 + 0.75) / (n as f64 + 0.5));
        for _ in 0..100 {
            let (mut p0, mut p1) = (1.0, x);
            for j in 2..=n {
                let p2 = ((2 * j - 1) as f64 * x * p1 - (j - 1) as f64 * p0) / j as f64;
                p0 = p1;
                p1 = p2;
            }
            let dp = n as f64 * (x * p1 - p0) / (x * x - 1.0);
            let dx = p1 / dp;
            x -= dx;
            if dx.abs() < 1e-16 {
                let w = 2.0 / ((1.0 - x * x) * dp * dp);
                nodes.push((x, w));
                break;
            }
        }
    }
    let mut pts: Vec<f64> = vec![a];
    pts.extend(cuts.iter().copied().filter(|&c| c > a && c < b));
    pts.push(b);
    let mut sum = 0.0;
    for w in pts.windows(2) {
        let (m, r) = (0.5 * (w[0] + w[1]), 0.5 * (w[1] - w[0]));
        sum += nodes.iter().map(|&(x, wt)| wt * f(m + r * x)).sum::<f64>() * r;
    }
    sum
}

/// A classical Runge–Kutta integration of `y' = f(t, y)` from `t0` to each
/// of `ts`, with steps of at most `h` that end on every one of `cuts` (the
/// points where the right-hand side is only piecewise smooth).
fn rk4(
    f: &dyn Fn(f64, &[f64]) -> Vec<f64>,
    y0: &[f64],
    ts: &[f64],
    cuts: &[f64],
    h: f64,
) -> Vec<Vec<f64>> {
    let mut y = y0.to_vec();
    let mut t = 0.0;
    let mut out = vec![];
    for &target in ts {
        while t < target {
            let next_cut = cuts.iter().copied().filter(|&c| c > t).fold(f64::INFINITY, f64::min);
            let stop = target.min(next_cut);
            let steps = ((stop - t) / h).ceil().max(1.0);
            let dt = (stop - t) / steps;
            for _ in 0..steps as usize {
                let add = |y: &[f64], k: &[f64], s: f64| -> Vec<f64> {
                    y.iter().zip(k).map(|(a, b)| a + s * b).collect()
                };
                // each stage reads the side of a cut its step lies on
                let tm = t + 0.5 * dt;
                let k1 = f(t + 1e-12 * dt, &y);
                let k2 = f(tm, &add(&y, &k1, 0.5 * dt));
                let k3 = f(tm, &add(&y, &k2, 0.5 * dt));
                let k4 = f(t + dt - 1e-12 * dt, &add(&y, &k3, dt));
                for j in 0..y.len() {
                    y[j] += dt / 6.0 * (k1[j] + 2.0 * k2[j] + 2.0 * k3[j] + k4[j]);
                }
                t += dt;
            }
            t = stop;
        }
        out.push(y.clone());
    }
    out
}

// ---------------------------------------------------------------------
// the models
// ---------------------------------------------------------------------

fn table_param(name: &str, unit: &str, data: TableData) -> ParamDecl {
    ParamDecl { default: ParamValue::Table(data), ..param(name, unit, 0.0, "") }
}

fn with_table(mut s: SubDecl, name: &str, data: TableData) -> SubDecl {
    s.modifiers.push(Modifier { param: name.into(), value: ParamValue::Table(data) });
    s
}

fn data_1d(x: &[f64], y: &[f64], interp: Interpolation, outside: Outside, axis: &str) -> TableData {
    TableData {
        interpolation: interp,
        outside: [outside, outside],
        axis_units: [axis.into(), String::new()],
        ..TableData::new_1d(x.to_vec(), y.to_vec())
    }
}

/// A pendulum (a point mass on a massless rod, in x and y) whose rod
/// length follows its table `length` over time, or (`two_d`) its 2-D table
/// read at (a·time, b·time + c): the length is a constraint on the
/// positions that reads a table (index 3).
fn varying_pendulum(two_d: bool) -> ComponentDef {
    let mut params = vec![
        param("m", "kg", 1.0, "mass"),
        param("g", "m/s2", 9.81, "gravity"),
        param("theta0", "1", 0.4, "start angle from the vertical"),
        param("L0", "m", 1.0, "the length at the start (the table's)"),
    ];
    let len = if two_d {
        params.push(param("a", "1", 1.0, ""));
        params.push(param("b", "1", 1.0, ""));
        params.push(param("c0", "s", 0.0, ""));
        params.push(table_param(
            "length",
            "m",
            TableData {
                axis_units: ["s".into(), "s".into()],
                ..TableData::new_2d(vec![0.0, 1.0], vec![0.0, 1.0], vec![1.0; 4])
            },
        ));
        lsim_ir::expr::table("length", vec![n("a") * Expr::Time, n("b") * Expr::Time + n("c0")])
    } else {
        params.push(table_param(
            "length",
            "m",
            data_1d(&[0.0, 1.0], &[1.0, 1.0], Interpolation::MonotoneCubic, Outside::Linear, "s"),
        ));
        lsim_ir::expr::table("length", vec![Expr::Time])
    };
    ComponentDef {
        name: if two_d { "Mechanics.VaryingPendulum2D" } else { "Mechanics.VaryingPendulum" }
            .into(),
        params,
        vars: vec![
            state_expr("x", "m", n("L0") * call(Builtin::Sin, vec![n("theta0")])),
            guess("y", "m", -(n("L0") * call(Builtin::Cos, vec![n("theta0")]))),
            state("vx", "m/s", 0.0, ""),
            guess("vy", "m/s", c(0.0)),
            guess("F", "N", n("m") * n("g")),
            var("L", "m", "the rod's length now"),
        ],
        equations: vec![
            eq(n("L"), len, "the rod's length follows its table"),
            eq(der("x"), n("vx"), "x changes with vx"),
            eq(der("y"), n("vy"), "y changes with vy"),
            eq(n("m") * der("vx"), -(n("x") * n("F") / n("L")), "the rod pulls along x"),
            eq(
                n("m") * der("vy"),
                -(n("y") * n("F") / n("L")) - n("m") * n("g"),
                "the rod and gravity pull along y",
            ),
            eq(n("x") * n("x") + n("y") * n("y"), n("L") * n("L"), "the rod has its length"),
        ],
        ..Default::default()
    }
}

/// A gear whose ratio reads a 2-D table at (the output speed, time):
/// a.w = ratio · b.w, the power kept.
fn speed_map_gear() -> ComponentDef {
    ComponentDef {
        name: "Rotational.SpeedMapGear".into(),
        ports: vec![port("a", "Flange", ""), port("b", "Flange", "")],
        params: vec![table_param(
            "ratio_map",
            "1",
            TableData {
                axis_units: ["rad/s".into(), "s".into()],
                ..TableData::new_2d(vec![0.0, 1.0], vec![0.0, 1.0], vec![1.0; 4])
            },
        )],
        vars: vec![var("ratio", "1", "")],
        equations: vec![
            eq(n("ratio"), lsim_ir::expr::table("ratio_map", vec![n("b.w"), Expr::Time]), ""),
            eq(n("a.w"), n("ratio") * n("b.w"), "a turns ratio times as fast as b"),
            eq(c(0.0), n("ratio") * n("a.tau") + n("b.tau"), "the power through it is kept"),
        ],
        ..Default::default()
    }
}

/// A gear whose ratio is a table of its output speed: a.w = ratio · b.w.
fn speed_ratio_gear() -> ComponentDef {
    ComponentDef {
        name: "Rotational.SpeedRatioGear".into(),
        ports: vec![port("a", "Flange", ""), port("b", "Flange", "")],
        params: vec![table_param(
            "ratio_curve",
            "1",
            data_1d(&[0.0, 1.0], &[1.0, 1.0], Interpolation::Linear, Outside::Linear, "rad/s"),
        )],
        vars: vec![var("ratio", "1", "")],
        equations: vec![
            eq(n("ratio"), lsim_ir::expr::table("ratio_curve", vec![n("b.w")]), ""),
            eq(n("a.w"), n("ratio") * n("b.w"), "a turns ratio times as fast as b"),
            eq(c(0.0), n("ratio") * n("a.tau") + n("b.tau"), "the power through it is kept"),
        ],
        ..Default::default()
    }
}

/// A mass held to its table's position over time through a force that
/// follows its command with a lag: the position must be differentiated
/// three times to find the command (index 4).
fn lagged_position() -> ComponentDef {
    ComponentDef {
        name: "Mechanics.LaggedPosition".into(),
        params: vec![
            param("m", "kg", 2.0, ""),
            param("T", "s", 0.1, "the force's lag"),
            table_param(
                "path",
                "m",
                data_1d(
                    &[0.0, 0.5, 1.0, 2.0],
                    &[0.0, 0.2, 0.3, 1.0],
                    Interpolation::MonotoneCubic,
                    Outside::Linear,
                    "s",
                ),
            ),
        ],
        vars: vec![
            state("x", "m", 0.0, ""),
            state("v", "m/s", 0.0, ""),
            state("f", "N", 0.0, ""),
            var("u", "N", "the force commanded"),
        ],
        equations: vec![
            eq(der("x"), n("v"), ""),
            eq(n("m") * der("v"), n("f"), ""),
            eq(n("T") * der("f"), n("u") - n("f"), "the force lags its command"),
            eq(n("x"), lsim_ir::expr::table("path", vec![Expr::Time]), "it follows its path"),
        ],
        ..Default::default()
    }
}

/// A voltage source that follows its table over time.
fn table_voltage() -> ComponentDef {
    ComponentDef {
        name: "Electrical.TableVoltage".into(),
        ports: vec![port("p", "Pin", ""), port("n", "Pin", "")],
        params: vec![table_param(
            "profile",
            "V",
            data_1d(&[0.0, 1.0], &[0.0, 1.0], Interpolation::MonotoneCubic, Outside::Clamp, "s"),
        )],
        vars: vec![var("v", "V", ""), var("i", "A", "")],
        equations: vec![
            eq(n("v"), n("p.v") - n("n.v"), ""),
            eq(c(0.0), n("p.i") + n("n.i"), ""),
            eq(n("i"), n("p.i"), ""),
            eq(n("v"), lsim_ir::expr::table("profile", vec![Expr::Time]), "it follows its profile"),
        ],
        ..Default::default()
    }
}

/// A ratio schedule read at a position that falls with time, `c0 - time`.
fn falling_schedule() -> ComponentDef {
    ComponentDef {
        name: "Signal.FallingSchedule".into(),
        ports: vec![signal("y", true, "1")],
        params: vec![
            param("c0", "s", 3.0, ""),
            table_param(
                "table",
                "1",
                data_1d(&[0.0, 1.0], &[1.0, 1.0], Interpolation::Linear, Outside::Clamp, "s"),
            ),
        ],
        equations: vec![eq(
            n("y"),
            lsim_ir::expr::table("table", vec![n("c0") - Expr::Time]),
            "it reads its table backwards in time",
        )],
        ..Default::default()
    }
}

fn lib() -> Library {
    let mut l = library();
    l.add(falling_schedule());
    l.add(table_voltage());
    l.add(varying_pendulum(false));
    l.add(varying_pendulum(true));
    l.add(speed_map_gear());
    l.add(speed_ratio_gear());
    l.add(lagged_position());
    l
}

fn time_table_def() -> String {
    lsim_lib::signal::time_table("1").name
}

/// A rotor driven at constant torque `tau` spins a load through a gear
/// whose ratio follows a table over time (the library's time-table signal
/// block): the two inertias are rigidly coupled through the table (index
/// 2). `J1`, `J2`: the rotor's and the load's inertia.
fn cvt(schedule: TableData, tau: f64, j1: f64, j2: f64) -> ComponentDef {
    ComponentDef {
        name: "Test.TableCvt".into(),
        components: vec![
            sub("torque", "Rotational.ConstantTorque", &[("tau", c(tau))]),
            sub("rotor", "Rotational.Inertia", &[("J", c(j1))]),
            sub("gear", "Rotational.Gearbox", &[]),
            sub("load", "Rotational.Inertia", &[("J", c(j2))]),
            with_table(sub("schedule", &time_table_def(), &[]), "table", schedule),
        ],
        connections: vec![
            connect("torque.flange", "rotor.a"),
            connect("rotor.b", "gear.a"),
            connect("gear.b", "load.a"),
            connect("schedule.y", "gear.ratio"),
        ],
        ..Default::default()
    }
}

/// The exact load speed of [`cvt`] from rest: d/dt (√M ω₂) = τ r / √M with
/// M = J1 r² + J2, so ω₂(t) = τ ∫₀ᵗ r/√M ds / √M(t).
fn cvt_exact(r: &dyn Fn(f64) -> f64, cuts: &[f64], tau: f64, j1: f64, j2: f64, t: f64) -> f64 {
    let m = |s: f64| (j1 * r(s) * r(s) + j2).sqrt();
    tau * integrate(&|s| r(s) / m(s), 0.0, t, cuts) / m(t)
}

fn prepare(top: &ComponentDef) -> (lsim_ir::PreparedModel, PrepReport) {
    prepare_with_report(&lib(), top, None, &Settings::default())
        .unwrap_or_else(|d| panic!("{d:#?}"))
}

fn err_of(run: &SimResult, channel: &str, exact: &[f64]) -> f64 {
    let ch = run.channel(channel).unwrap_or_else(|| panic!("no channel {channel}"));
    let scale = exact.iter().fold(0.0f64, |a, x| a.max(x.abs()));
    ch.iter().zip(exact).fold(0.0f64, |a, (g, w)| a.max((g - w).abs())) / scale
}

// ---------------------------------------------------------------------
// index 2
// ---------------------------------------------------------------------

/// A capacitor directly across a source that follows a table over time
/// (the model the engine used to refuse): its voltage is the table's and
/// its current C times the table's slope, exactly.
#[test]
fn a_capacitor_across_a_table_source_takes_c_times_its_slope() {
    let ts = [0.0, 0.01, 0.025, 0.03, 0.05];
    let vs = [0.0, 8.0, 11.0, 12.0, 6.0];
    let data = data_1d(&ts, &vs, Interpolation::MonotoneCubic, Outside::Clamp, "s");
    let cap = 1e-3;
    let top = ComponentDef {
        name: "Test.CapacitorOnProfile".into(),
        components: vec![
            with_table(sub("src", "Electrical.TableVoltage", &[]), "profile", data),
            sub("cap", "Electrical.Capacitor", &[("C", c(cap))]),
            sub("r2", "Electrical.Resistor", &[("R", c(2.0))]),
            sub("c2", "Electrical.Capacitor", &[("C", c(1e-3))]),
            sub("gnd", "Electrical.Ground", &[]),
        ],
        connections: vec![
            connect("src.p", "cap.p"),
            connect("src.p", "r2.p"),
            connect("r2.n", "c2.p"),
            connect("src.n", "cap.n"),
            connect("src.n", "c2.n"),
            connect("src.n", "gnd.p"),
        ],
        ..Default::default()
    };
    let (m, report) = prepare(&top);
    assert!(report.differentiated >= 1);
    assert_eq!(report.states, vec!["c2.v".to_string()]);
    let run = simulate(&m, 0.05, 1e-3, 1e-10);
    let cub = Cubic::steffen(&ts, &vs);
    let v: Vec<f64> = run.times.iter().map(|&t| cub.at(t)[0]).collect();
    let i: Vec<f64> = run.times.iter().map(|&t| cap * cub.at(t)[1]).collect();
    let (ev, ei) = (err_of(&run, "cap.v", &v), err_of(&run, "cap.i", &i));
    println!("errors: v {ev:.1e}, i {ei:.1e}");
    assert!(ev < 1e-12 && ei < 1e-12, "exact up to rounding");
}

/// A ratio rising along a line, sampled at uneven times: the monotone
/// cubic through them is that line, and ω₂(t) has a closed form,
/// τ/(k J1) (1 − √(M(0)/M(t))).
#[test]
fn a_gear_whose_ratio_follows_a_table_matches_the_closed_form() {
    let (tau, j1, j2) = (20.0, 0.05, 2.0);
    let (r0, k) = (2.0, 1.5);
    let ts = [0.0, 0.3, 1.1, 1.7, 2.5, 3.0];
    let rs: Vec<f64> = ts.iter().map(|t| r0 + k * t).collect();
    let data = data_1d(&ts, &rs, Interpolation::MonotoneCubic, Outside::Clamp, "s");
    let (m, report) = prepare(&cvt(data, tau, j1, j2));
    println!("{report:#?}");
    assert!(report.differentiated >= 1, "the coupling is differentiated through the table");
    assert_eq!(m.states.len(), 1, "one state for the rigid pair: {:?}", report.states);
    assert!(m.warnings.is_empty(), "{:#?}", m.warnings);
    let run = simulate(&m, 3.0, 0.01, 1e-10);
    let mm = |t: f64| j1 * (r0 + k * t) * (r0 + k * t) + j2;
    let w2 = |t: f64| tau / (k * j1) * (1.0 - (mm(0.0) / mm(t)).sqrt());
    let e2 = worst(&run, "load.w", w2);
    let e1 = worst(&run, "rotor.w", |t| (r0 + k * t) * w2(t));
    println!("errors: load {e2:.1e}, rotor {e1:.1e}");
    assert!(e2 < 1e-9 && e1 < 1e-9);
}

/// A ratio through curved data, interpolated linearly: its slope jumps at
/// every breakpoint, which index 2 allows (the coupling is differentiated
/// once; the rotor's acceleration jumps there, the speeds do not). Per
/// piece r is affine and the integral has a closed form.
#[test]
fn a_linear_table_can_be_differentiated_once() {
    let (tau, j1, j2) = (20.0, 0.05, 2.0);
    let ts = [0.0, 0.4, 0.9, 1.7, 2.2, 3.0];
    let rs = [3.0, 2.6, 2.0, 1.5, 1.3, 1.2];
    let data = data_1d(&ts, &rs, Interpolation::Linear, Outside::Clamp, "s");
    let (m, report) = prepare(&cvt(data, tau, j1, j2));
    assert!(report.differentiated >= 1);
    // the run stops just before each breakpoint and restarts there
    let info = lsim_solve::RunInfo::from_prepared(&m);
    assert!(info.time_tables.iter().all(|tt| tt.kink), "{:?}", info.time_tables);
    let run = simulate(&m, 3.0, 0.01, 1e-10);
    let lin = Cubic::linear(&ts, &rs);
    let mm = |r: f64| (j1 * r * r + j2).sqrt();
    let w2 = |t: f64| {
        // Σ over the pieces before t of [√M]/(k J1)
        let mut acc = 0.0;
        for i in 0..ts.len() - 1 {
            if t <= ts[i] {
                break;
            }
            let b = t.min(ts[i + 1]);
            let k = (rs[i + 1] - rs[i]) / (ts[i + 1] - ts[i]);
            acc += (mm(lin.at(b)[0]) - mm(rs[i])) / (k * j1);
        }
        tau * acc / mm(lin.at(t)[0])
    };
    let e2 = worst(&run, "load.w", w2);
    let e1 = worst(&run, "rotor.w", |t| lin.at(t)[0] * w2(t));
    println!("errors: load {e2:.1e}, rotor {e1:.1e}");
    assert!(e2 < 1e-9 && e1 < 1e-9);
}

/// The same with the monotone cubic through curved data: against this
/// file's own Steffen interpolant and Gauss–Legendre quadrature.
#[test]
fn a_gear_through_a_monotone_cubic_matches_quadrature() {
    let (tau, j1, j2) = (20.0, 0.05, 2.0);
    let ts = [0.0, 0.4, 0.9, 1.7, 2.2, 3.0];
    let rs = [3.0, 2.6, 2.0, 1.5, 1.3, 1.2];
    let data = data_1d(&ts, &rs, Interpolation::MonotoneCubic, Outside::Clamp, "s");
    let (m, _) = prepare(&cvt(data, tau, j1, j2));
    let run = simulate(&m, 3.0, 0.01, 1e-10);
    let cub = Cubic::steffen(&ts, &rs);
    let r = |t: f64| cub.at(t)[0];
    let w2: Vec<f64> = run.times.iter().map(|&t| cvt_exact(&r, &ts, tau, j1, j2, t)).collect();
    let w1: Vec<f64> = run.times.iter().zip(&w2).map(|(&t, w)| r(t) * w).collect();
    let (e2, e1) = (err_of(&run, "load.w", &w2), err_of(&run, "rotor.w", &w1));
    println!("errors: load {e2:.1e}, rotor {e1:.1e}");
    assert!(e2 < 1e-9 && e1 < 1e-9);
}

/// A gear whose ratio is a 2-D table of the output speed and time (a
/// bicubic through g(w) + h(t), quadratics, which it reproduces exactly):
/// the coupling differentiates the table along both axes. Against a fine
/// Runge–Kutta integration of
/// ω₂' (J1 (r + ω₂ ∂r/∂w) + J2/r) = τ − J1 ω₂ ∂r/∂t.
#[test]
fn a_gear_through_a_2d_map_matches_a_fine_integration() {
    let (tau, j1, j2) = (5.0, 0.05, 2.0);
    let g = |w: f64| [2.0 + 0.01 * w + 0.0004 * w * w, 0.01 + 0.0008 * w];
    let h = |t: f64| [-0.3 * t + 0.02 * t * t, -0.3 + 0.04 * t];
    let ws = [0.0, 5.0, 12.0, 20.0, 40.0];
    let ts = [0.0, 0.5, 1.2, 2.0, 3.0];
    let vals: Vec<f64> =
        ws.iter().flat_map(|&w| ts.iter().map(move |&t| g(w)[0] + h(t)[0])).collect();
    let mut data = TableData::new_2d(ws.to_vec(), ts.to_vec(), vals);
    data.axis_units = ["rad/s".into(), "s".into()];
    data.outside = [Outside::Linear, Outside::Linear];
    // the premise: the engine's bicubic is g + h
    let interp = lsim_ir::interp::Table::new(&data).unwrap();
    for (w, t) in [(1.0, 0.1), (7.0, 1.0), (33.0, 2.9)] {
        assert!((interp.eval([w, t]).0 - g(w)[0] - h(t)[0]).abs() < 1e-13);
    }
    let top = ComponentDef {
        name: "Test.MapCvt".into(),
        components: vec![
            sub("torque", "Rotational.ConstantTorque", &[("tau", c(tau))]),
            sub("rotor", "Rotational.Inertia", &[("J", c(j1))]),
            with_table(sub("gear", "Rotational.SpeedMapGear", &[]), "ratio_map", data),
            sub("load", "Rotational.Inertia", &[("J", c(j2))]),
        ],
        connections: vec![
            connect("torque.flange", "rotor.a"),
            connect("rotor.b", "gear.a"),
            connect("gear.b", "load.a"),
        ],
        ..Default::default()
    };
    let (m, report) = prepare(&top);
    println!("{report:#?}");
    assert!(report.differentiated >= 1);
    let run = simulate(&m, 3.0, 0.01, 1e-10);
    let f = |t: f64, y: &[f64]| {
        let w = y[0];
        let (r, rw, rt) = (g(w)[0] + h(t)[0], g(w)[1], h(t)[1]);
        vec![(tau - j1 * w * rt) / (j1 * (r + w * rw) + j2 / r)]
    };
    let refs = rk4(&f, &[0.0], &run.times, &[], 1e-4);
    let w2: Vec<f64> = refs.iter().map(|y| y[0]).collect();
    let w1: Vec<f64> = run.times.iter().zip(&w2).map(|(&t, &w)| (g(w)[0] + h(t)[0]) * w).collect();
    let (e2, e1) = (err_of(&run, "load.w", &w2), err_of(&run, "rotor.w", &w1));
    println!("errors: load {e2:.1e}, rotor {e1:.1e}");
    assert!(e2 < 1e-9 && e1 < 1e-9);
}

// ---------------------------------------------------------------------
// index 3
// ---------------------------------------------------------------------

/// The angle of a pendulum whose length is `len(t)` ([L, L', L'']):
/// L θ'' + 2 L' θ' + g sin θ = 0, from θ0 with x' = 0.
fn pendulum_reference(
    len: &dyn Fn(f64) -> [f64; 3],
    theta0: f64,
    times: &[f64],
    cuts: &[f64],
) -> Vec<Vec<f64>> {
    let g = 9.81;
    let f = |t: f64, y: &[f64]| {
        let [l, dl, _] = len(t);
        vec![y[1], -(2.0 * dl * y[1] + g * lsim_ir::math::sin(y[0])) / l]
    };
    let [l0, dl0, _] = len(0.0);
    // x' = L' sin θ + L cos θ θ' = 0
    let w0 = -dl0 * lsim_ir::math::sin(theta0) / (l0 * lsim_ir::math::cos(theta0));
    rk4(&f, &[theta0, w0], times, cuts, 2e-4)
}

fn check_pendulum(run: &SimResult, len: &dyn Fn(f64) -> [f64; 3], refs: &[Vec<f64>]) {
    let x: Vec<f64> =
        run.times.iter().zip(refs).map(|(&t, y)| len(t)[0] * lsim_ir::math::sin(y[0])).collect();
    let y: Vec<f64> =
        run.times.iter().zip(refs).map(|(&t, y)| -len(t)[0] * lsim_ir::math::cos(y[0])).collect();
    let (ex, ey) = (err_of(run, "p.x", &x), err_of(run, "p.y", &y));
    let (xs, ys) = (run.channel("p.x").unwrap(), run.channel("p.y").unwrap());
    let drift = run
        .times
        .iter()
        .enumerate()
        .map(|(k, &t)| ((xs[k] * xs[k] + ys[k] * ys[k]).sqrt() - len(t)[0]).abs())
        .fold(0.0f64, f64::max);
    println!("errors: x {ex:.1e}, y {ey:.1e}; length held to {drift:.1e}");
    assert!(ex < 3e-8 && ey < 3e-8, "x {ex:e}, y {ey:e}");
    assert!(drift < 1e-9, "the rod's length is an equation of the reduced model");
}

/// A pendulum whose rod is let out and pulled in along a monotone cubic
/// over time (its second derivative jumps at every breakpoint: the rod's
/// pull jumps there, the motion stays smooth): against a Runge–Kutta
/// integration in the angle with this file's own interpolant, stepping to
/// each breakpoint.
#[test]
fn a_pendulum_whose_length_follows_a_table_swings_as_the_reference() {
    let ts = [0.0, 0.5, 1.2, 1.6, 2.4, 3.0, 3.5];
    let ls = [1.0, 1.1, 1.4, 1.5, 1.45, 1.2, 1.1];
    let data = data_1d(&ts, &ls, Interpolation::MonotoneCubic, Outside::Linear, "s");
    let top = ComponentDef {
        name: "Test.ReelPendulum".into(),
        components: vec![with_table(
            sub("p", "Mechanics.VaryingPendulum", &[("theta0", c(0.4)), ("L0", c(ls[0]))]),
            "length",
            data,
        )],
        ..Default::default()
    };
    let (m, report) = prepare(&top);
    println!("{report:#?}");
    assert!(report.differentiated >= 3, "the constraint differentiated twice and more");
    assert_eq!(report.states.len(), 2, "{:?}", report.states);
    let run = simulate(&m, 3.5, 0.01, 1e-10);
    let cub = Cubic::steffen(&ts, &ls);
    let len = |t: f64| cub.at(t);
    let refs = pendulum_reference(&len, 0.4, &run.times, &ts);
    check_pendulum(&run, &len, &refs);
    // the rod's pull jumps where the length's second derivative does: at
    // a breakpoint the run records the cell after it (the integrator stops
    // there, the table being read along time)
    let f = run.channel("p.F").unwrap();
    let k = run.times.iter().position(|&t| (t - 1.2).abs() < 1e-12).unwrap();
    let left = f[k - 1] + (f[k - 1] - f[k - 2]);
    let right = f[k + 1] - (f[k + 2] - f[k + 1]);
    println!("pull at 1.2 s: {} (left ~{left}, right ~{right})", f[k]);
    assert!((f[k] - right).abs() < (f[k] - left).abs(), "the side after the breakpoint");
}

/// The same with the length a 2-D table read at (a t, b t + c): its
/// derivatives along both axes, mixed ones included. The bicubic through
/// g(x) + h(y) (quadratics) is that sum, so L, L', L'' are known exactly.
#[test]
fn a_pendulum_whose_length_follows_a_2d_table_swings_as_the_reference() {
    let (a, b, c0) = (0.8, 0.5, 0.2);
    let g = |x: f64| [0.6 + 0.2 * x - 0.03 * x * x, 0.2 - 0.06 * x, -0.06];
    let h = |y: f64| [0.4 + 0.1 * y + 0.04 * y * y, 0.1 + 0.08 * y, 0.08];
    let xs = [0.0, 0.7, 1.5, 2.2, 3.0];
    let ys = [0.0, 0.5, 1.0, 1.8, 2.5];
    let vals: Vec<f64> =
        xs.iter().flat_map(|&x| ys.iter().map(move |&y| g(x)[0] + h(y)[0])).collect();
    let mut data = TableData::new_2d(xs.to_vec(), ys.to_vec(), vals);
    data.axis_units = ["s".into(), "s".into()];
    data.outside = [Outside::Error, Outside::Linear];
    let interp = lsim_ir::interp::Table::new(&data).unwrap();
    for (x, y) in [(0.1, 0.1), (1.0, 1.3), (2.9, 2.4)] {
        assert!((interp.eval([x, y]).0 - g(x)[0] - h(y)[0]).abs() < 1e-13);
    }
    let len = |t: f64| {
        let (gx, hy) = (g(a * t), h(b * t + c0));
        [gx[0] + hy[0], a * gx[1] + b * hy[1], a * a * gx[2] + b * b * hy[2]]
    };
    let top = ComponentDef {
        name: "Test.MapPendulum".into(),
        components: vec![with_table(
            sub(
                "p",
                "Mechanics.VaryingPendulum2D",
                &[
                    ("theta0", c(0.4)),
                    ("L0", c(len(0.0)[0])),
                    ("a", c(a)),
                    ("b", c(b)),
                    ("c0", c(c0)),
                ],
            ),
            "length",
            data,
        )],
        ..Default::default()
    };
    let (m, report) = prepare(&top);
    println!("{report:#?}");
    assert!(report.differentiated >= 3);
    let run = simulate(&m, 3.0, 0.01, 1e-10);
    let refs = pendulum_reference(&len, 0.4, &run.times, &[]);
    check_pendulum(&run, &len, &refs);
}

// ---------------------------------------------------------------------
// tables not smooth enough, and the library's blocks
// ---------------------------------------------------------------------

fn fault(top: &ComponentDef) -> lsim_ir::Diagnostic {
    let diags = match prepare_with_report(&lib(), top, None, &Settings::default()) {
        Ok(_) => panic!("prepared"),
        Err(d) => d,
    };
    let d = diags
        .iter()
        .find(|d| d.code == "INDEX-TABLE-NOT-SMOOTH")
        .unwrap_or_else(|| panic!("{diags:#?}"))
        .clone();
    println!("{d}");
    assert_eq!(d.severity, Severity::Error);
    assert!(d.hint.as_ref().is_some_and(|h| !h.is_empty()));
    d
}

fn reel(data: TableData) -> ComponentDef {
    ComponentDef {
        name: "Test.ReelPendulum".into(),
        components: vec![with_table(
            sub("p", "Mechanics.VaryingPendulum", &[("L0", c(1.0))]),
            "length",
            data,
        )],
        ..Default::default()
    }
}

/// A pendulum's length read linearly: the length's slope jumps at every
/// breakpoint, and the rod would have to jerk the mass (index 3
/// differentiates the length twice).
#[test]
fn a_linear_table_differentiated_twice_is_refused() {
    let data =
        data_1d(&[0.0, 1.0, 2.0], &[1.0, 1.2, 1.1], Interpolation::Linear, Outside::Linear, "s");
    let d = fault(&reel(data));
    assert_eq!(d.parts, vec!["p".to_string()]);
    assert!(d.message.contains("'p.length'") && d.message.contains("twice"), "{}", d.message);
    assert!(d.message.contains("kink at every breakpoint"), "{}", d.message);
    assert!(d.hint.unwrap().contains("monotone cubic"));
}

/// The same through a monotone cubic held at its edge values: a kink at
/// each edge of the data.
#[test]
fn a_held_table_differentiated_twice_is_refused() {
    let data = data_1d(
        &[0.0, 1.0, 2.0],
        &[1.0, 1.2, 1.1],
        Interpolation::MonotoneCubic,
        Outside::Clamp,
        "s",
    );
    let d = fault(&reel(data));
    assert!(d.message.contains("kink at each edge"), "{}", d.message);
    assert!(d.hint.unwrap().contains("continue linearly"));
}

/// A cubic differentiated three times: its second derivative jumps.
#[test]
fn a_cubic_differentiated_three_times_is_refused() {
    let top = ComponentDef {
        name: "Test.Lagged".into(),
        components: vec![sub("pos", "Mechanics.LaggedPosition", &[])],
        ..Default::default()
    };
    let d = fault(&top);
    assert!(
        d.message.contains("3 times") && d.message.contains("second derivative"),
        "{}",
        d.message
    );
}

/// The library's own blocks reading tables in a constraint: the time
/// table and the 1-D table blocks give a gear's ratio (the ratio of a
/// line read through an identity table: r(t) = r0 + k t), and the 2-D
/// table block reads a map at two time tables.
#[test]
fn the_librarys_table_blocks_in_a_constraint() {
    let (tau, j1, j2) = (20.0, 0.05, 2.0);
    let (r0, k) = (2.0, 1.5);
    let ts = [0.0, 1.0, 2.0, 3.0];
    let line = |f: &dyn Fn(f64) -> f64| -> Vec<f64> { ts.iter().map(|&t| f(t)).collect() };
    // time → u = t (a time table), u → r0 + k u (a 1-D table)
    let clock = data_1d(&ts, &line(&|t| t), Interpolation::MonotoneCubic, Outside::Linear, "s");
    let ratio =
        data_1d(&ts, &line(&|u| r0 + k * u), Interpolation::MonotoneCubic, Outside::Linear, "");
    let t1 = lsim_lib::signal::table1d("1", "1").name;
    let t2 = lsim_lib::signal::table2d("1", "1", "1").name;
    let top = ComponentDef {
        name: "Test.BlockCvt".into(),
        components: vec![
            sub("torque", "Rotational.ConstantTorque", &[("tau", c(tau))]),
            sub("rotor", "Rotational.Inertia", &[("J", c(j1))]),
            sub("gear", "Rotational.Gearbox", &[]),
            sub("load", "Rotational.Inertia", &[("J", c(j2))]),
            with_table(sub("clock", &time_table_def(), &[]), "table", clock.clone()),
            with_table(sub("lookup", &t1, &[]), "table", ratio),
        ],
        connections: vec![
            connect("torque.flange", "rotor.a"),
            connect("rotor.b", "gear.a"),
            connect("gear.b", "load.a"),
            connect("clock.y", "lookup.u"),
            connect("lookup.y", "gear.ratio"),
        ],
        ..Default::default()
    };
    let (m, report) = prepare(&top);
    assert!(report.differentiated >= 1);
    let run = simulate(&m, 3.0, 0.01, 1e-10);
    let mm = |t: f64| j1 * (r0 + k * t) * (r0 + k * t) + j2;
    let w2 = |t: f64| tau / (k * j1) * (1.0 - (mm(0.0) / mm(t)).sqrt());
    let e = worst(&run, "load.w", w2);
    println!("1-D blocks: error {e:.1e}");
    assert!(e < 1e-9);

    // a 2-D map r = r0 + k (u + v) / 2, read at u = t and v = t
    let grid = [0.0, 1.0, 2.0, 3.0];
    let vals: Vec<f64> =
        grid.iter().flat_map(|&u| grid.iter().map(move |&v| r0 + 0.5 * k * (u + v))).collect();
    let mut map = TableData::new_2d(grid.to_vec(), grid.to_vec(), vals);
    map.outside = [Outside::Linear, Outside::Linear];
    let mut top2 = top.clone();
    top2.components.retain(|s| s.name != "lookup");
    top2.components.push(with_table(sub("clock2", &time_table_def(), &[]), "table", clock));
    top2.components.push(with_table(sub("map", &t2, &[]), "table", map));
    top2.connections.retain(|c| !c.a.starts_with("lookup") && !c.b.starts_with("lookup"));
    top2.connections.push(connect("clock.y", "map.u1"));
    top2.connections.push(connect("clock2.y", "map.u2"));
    top2.connections.push(connect("map.y", "gear.ratio"));
    let (m, _) = prepare(&top2);
    let run = simulate(&m, 3.0, 0.01, 1e-10);
    let e = worst(&run, "load.w", w2);
    println!("2-D block: error {e:.1e}");
    assert!(e < 1e-9);

    // the vehicle library's lookup blocks (signal.lookup, 1-D and 2-D) in
    // the same places
    let mut lib2 = lib();
    let lookup2 = lsim_lib::blocks::signals::lookup(&lsim_lib::blocks::signals::LookupConfig::D2(
        Default::default(),
    ));
    let lookup2_name = lookup2.name.clone();
    lib2.add(lookup2);
    let swap = |top: &ComponentDef, from: &str, to: &str, ports: &[(&str, &str)]| {
        let mut t = top.clone();
        for s in &mut t.components {
            if s.def == from {
                s.def = to.into();
            }
        }
        for c in &mut t.connections {
            for (a, b) in ports {
                for end in [&mut c.a, &mut c.b] {
                    if end == a {
                        *end = (*b).into();
                    }
                }
            }
        }
        t
    };
    let vehicle1 = swap(
        &top,
        &t1,
        "Blocks.Lookup",
        &[("lookup.u", "lookup.sig_x_in"), ("lookup.y", "lookup.sig_out")],
    );
    let vehicle2 = swap(
        &top2,
        &t2,
        &lookup2_name,
        &[("map.u1", "map.sig_x_in"), ("map.u2", "map.sig_y_in"), ("map.y", "map.sig_out")],
    );
    for (what, t) in [("vehicle 1-D lookup", vehicle1), ("vehicle 2-D lookup", vehicle2)] {
        let (m, report) = prepare_with_report(&lib2, &t, None, &Settings::default())
            .unwrap_or_else(|d| panic!("{what}: {d:#?}"));
        assert!(report.differentiated >= 1);
        let run = simulate(&m, 3.0, 0.01, 1e-10);
        let e = worst(&run, "load.w", w2);
        println!("{what}: error {e:.1e}");
        assert!(e < 1e-9);
    }
}

/// A gear whose ratio is a linear table of the load's speed: its slope
/// jumps where the speed crosses a breakpoint, at times the run loop does
/// not know ahead; the integrator steps over these kinks of the rotor's
/// acceleration with its error control. Against the closed form of
/// t(ω₂) = (J1 r(ω₂) ω₂ + J2 ∫₀^ω₂ dw / r(w)) / τ (r affine per piece),
/// inverted by bisection.
#[test]
fn a_linear_table_read_at_a_state_is_stepped_over_accurately() {
    let (tau, j1, j2) = (20.0, 0.05, 2.0);
    let ws = [0.0, 10.0, 25.0, 40.0, 60.0, 80.0];
    let rs = [2.0, 2.5, 2.8, 3.5, 3.6, 4.0];
    let data = data_1d(&ws, &rs, Interpolation::Linear, Outside::Linear, "rad/s");
    let top = ComponentDef {
        name: "Test.SpeedCvt".into(),
        components: vec![
            sub("torque", "Rotational.ConstantTorque", &[("tau", c(tau))]),
            sub("rotor", "Rotational.Inertia", &[("J", c(j1))]),
            with_table(sub("gear", "Rotational.SpeedRatioGear", &[]), "ratio_curve", data),
            sub("load", "Rotational.Inertia", &[("J", c(j2))]),
        ],
        connections: vec![
            connect("torque.flange", "rotor.a"),
            connect("rotor.b", "gear.a"),
            connect("gear.b", "load.a"),
        ],
        ..Default::default()
    };
    let (m, report) = prepare(&top);
    assert!(report.differentiated >= 1);
    let run = simulate(&m, 3.0, 0.01, 1e-10);
    let lin = Cubic::linear(&ws, &rs);
    let t_of = |w: f64| {
        let mut inv = 0.0;
        for i in 0..ws.len() - 1 {
            if w <= ws[i] {
                break;
            }
            let b = w.min(ws[i + 1]);
            let k = (rs[i + 1] - rs[i]) / (ws[i + 1] - ws[i]);
            inv += lsim_ir::math::ln(lin.at(b)[0] / rs[i]) / k;
        }
        (j1 * lin.at(w)[0] * w + j2 * inv) / tau
    };
    let w_of = |t: f64| {
        let (mut lo, mut hi) = (0.0, 80.0);
        for _ in 0..200 {
            let mid = 0.5 * (lo + hi);
            if t_of(mid) < t { lo = mid } else { hi = mid }
        }
        0.5 * (lo + hi)
    };
    let w2: Vec<f64> = run.times.iter().map(|&t| w_of(t)).collect();
    assert!(w2.last().unwrap() > &40.0, "it crosses breakpoints: {:?}", w2.last());
    let e2 = err_of(&run, "load.w", &w2);
    println!("errors: load {e2:.1e}");
    assert!(e2 < 1e-8, "{e2:e}");
}

/// The linear schedule read backwards in time (its position falls) on an
/// evenly spaced axis: the run stops before each breakpoint in the cell
/// the position comes from, and restarts there. Per piece the ratio is
/// affine in time and the load's speed has a closed form.
#[test]
fn a_linear_table_read_backwards_in_time_matches_the_closed_form() {
    let (tau, j1, j2) = (20.0, 0.05, 2.0);
    let xs = [0.0, 0.5, 1.0, 1.5, 2.0, 2.5, 3.0];
    let ys = [1.2, 1.3, 1.5, 2.0, 2.6, 2.8, 3.0];
    let data = data_1d(&xs, &ys, Interpolation::Linear, Outside::Clamp, "s");
    let mut top = cvt(data.clone(), tau, j1, j2);
    top.components.retain(|s| s.name != "schedule");
    top.components.push(with_table(sub("schedule", "Signal.FallingSchedule", &[]), "table", data));
    let (m, report) = prepare(&top);
    assert!(report.differentiated >= 1);
    let info = lsim_solve::RunInfo::from_prepared(&m);
    assert!(info.time_tables.iter().all(|tt| tt.kink && tt.c < 0.0), "{:?}", info.time_tables);
    let run = simulate(&m, 3.0, 0.01, 1e-10);
    // in time order: r(t) = table(3 - t)
    let ts: Vec<f64> = xs.iter().rev().map(|x| 3.0 - x).collect();
    let rs: Vec<f64> = ys.iter().rev().copied().collect();
    let lin = Cubic::linear(&ts, &rs);
    let mm = |r: f64| (j1 * r * r + j2).sqrt();
    let w2 = |t: f64| {
        let mut acc = 0.0;
        for i in 0..ts.len() - 1 {
            if t <= ts[i] {
                break;
            }
            let b = t.min(ts[i + 1]);
            let k = (rs[i + 1] - rs[i]) / (ts[i + 1] - ts[i]);
            acc += (mm(lin.at(b)[0]) - mm(rs[i])) / (k * j1);
        }
        tau * acc / mm(lin.at(t)[0])
    };
    let e2 = worst(&run, "load.w", w2);
    println!("errors: load {e2:.1e}");
    assert!(e2 < 1e-9, "{e2:e}");
}
