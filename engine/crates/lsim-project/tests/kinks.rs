//! Tables read at a state, held in their cells (DESIGN.md §8.2, *Tables
//! read at a state*; `lsim_prep::hold`): where a model reads a table at a
//! position that moves with its states and the read kinks or jumps at the
//! table's breakpoints, every step integrates the cell's own polynomial and
//! each crossing of a breakpoint is an event. Measured before against exact
//! answers (`engine/judge`, the `kink-…` cases): stepping over a jump read
//! through index reduction ended up to 178 rtol off where the same function
//! with its breakpoints located is within 3, and many kinks of a value read
//! cost up to 5 times the located run's error; held, each run is the
//! located one.

use lsim_ir::interp::Table;
use lsim_ir::{Interpolation, Outside, PreparedModel, TableData};
use lsim_lang::{Report, parse_library};
use lsim_solve::{Method, OutputGrid, RunInfo, SimResult, SolverOptions};

const RTOLS: [f64; 7] = [1e-4, 1e-5, 1e-6, 1e-7, 1e-8, 1e-9, 1e-10];

fn prepare(text: &str, name: &str, hold: bool) -> PreparedModel {
    let mut lib = lsim_lib::library();
    let parsed = parse_library(text, Some(&lib)).unwrap_or_else(|e| panic!("{}", Report(&e)));
    lib.types.extend(parsed.types);
    for def in parsed.components.into_values() {
        lib.add(def);
    }
    let model = lib.components.get(name).cloned().expect("the model");
    let settings = lsim_prep::Settings { hold_table_cells: hold, ..Default::default() };
    lsim_prep::prepare_with_report(&lib, &model, None, &settings)
        .unwrap_or_else(|d| panic!("{d:#?}"))
        .0
}

fn run(m: &PreparedModel, rtol: f64, t_end: f64, dt: f64, method: Method) -> SimResult {
    let code = lsim_codegen::compile(m, &Default::default()).expect("compiles");
    let info = RunInfo::from_prepared(m);
    let opts = SolverOptions { rtol, atol: 1e-2 * rtol, method, ..Default::default() };
    lsim_solve::simulate(&code, &info, &opts, OutputGrid { t0: 0.0, t_end, dt }, &mut [])
        .unwrap_or_else(|e| panic!("{e}"))
}

/// The largest error of `channel` against `exact` over the grid, relative
/// to its largest value.
fn error(r: &SimResult, channel: &str, exact: &[f64]) -> f64 {
    let ch = r.channel(channel).unwrap_or_else(|| panic!("no channel {channel}"));
    let scale = exact.iter().fold(0.0f64, |a, x| a.max(x.abs()));
    ch.iter().zip(exact).fold(0.0f64, |a, (g, w)| a.max((g - w).abs())) / scale
}

fn cells(r: &SimResult) -> usize {
    r.events.iter().filter(|e| matches!(e.kind, lsim_solve::EventKind::Cell(_))).count()
}

fn table_text(xs: &[f64], ys: &[f64], unit: &str, interp: &str, outside: &str) -> String {
    let list = |v: &[f64]| v.iter().map(|x| format!("{x:?}")).collect::<Vec<_>>().join(", ");
    format!(
        "table(x = {{{}}}, y = {{{}}}, xUnit = \"{unit}\", interpolation = {interp}, outside = {{{outside}}})",
        list(xs),
        list(ys)
    )
}

/// The gear of the outside judge's `kink-gear-…` cases: a rotor driven by
/// `tau0` (reversed at `t_rev`) turns a load through a gear whose ratio is
/// a table of the load's speed, `w1 = r(w2) w2`: index reduction
/// differentiates it and reads `r'`, which jumps at a linear table's
/// breakpoints and at a held table's edges.
fn gear(table: &str, reverse: Option<f64>) -> String {
    let tau = if reverse.is_some() { "if time < t_rev then tau0 else -tau0" } else { "tau0" };
    format!(
        r#"model Test.KinkGear "a gear whose ratio is a table of the load's speed"
  parameter Real J1(unit = "kg.m2") = 0.05;
  parameter Real J2(unit = "kg.m2") = 2;
  parameter Real tau0(unit = "N.m") = 20;
  parameter Real t_rev(unit = "s") = {t_rev:?};
  parameter Real r = {table};
  Real w1(unit = "rad/s", start = 0);
  Real w2(unit = "rad/s", start = 0, fixed = true);
  Real t1(unit = "N.m");
  Real t2(unit = "N.m");
  Real tau(unit = "N.m");
equation
  tau = {tau};
  J1 * der(w1) = tau - t1;
  J2 * der(w2) = t2;
  w1 = r(w2) * w2;
  t2 = r(w2) * t1;
end Test.KinkGear;
"#,
        t_rev = reverse.unwrap_or(1e9)
    )
}

/// The gear's load speed in closed form: its first integral
/// `Φ(w2) = J1 r(w2) w2 + J2 ∫₀^w2 dw / r(w)` is `∫₀^t tau`, `Φ` rising
/// with `w2` (the integral by 16-point Gauss–Legendre on each cell, where
/// `1 / r` is smooth), inverted by bisection.
fn gear_exact(data: &TableData, reverse: Option<f64>, times: &[f64]) -> Vec<f64> {
    let (j1, j2, tau0) = (0.05, 2.0, 20.0);
    let t = Table::new(data).expect("a table");
    let r = |w: f64| t.value([w, 0.0]);
    // Gauss–Legendre nodes and weights on [-1, 1], 16 points
    let gl: [(f64, f64); 8] = [
        (0.0950125098376374, 0.1894506104550685),
        (0.2816035507792589, 0.1826034150449236),
        (0.4580167776572274, 0.1691565193950025),
        (0.6178762444026438, 0.1495959888165767),
        (0.755404408355003, 0.1246289712555339),
        (0.8656312023878318, 0.0951585116824928),
        (0.9445750230732326, 0.0622535239386479),
        (0.9894009349916499, 0.0271524594117541),
    ];
    let quad = |a: f64, b: f64| -> f64 {
        let (m, h) = (0.5 * (a + b), 0.5 * (b - a));
        gl.iter().map(|&(x, w)| w * (1.0 / r(m - h * x) + 1.0 / r(m + h * x))).sum::<f64>() * h
    };
    let edges: Vec<f64> = data.x.clone();
    let integral = |w: f64| -> f64 {
        let mut acc = 0.0;
        let mut lo = 0.0;
        for &e in edges.iter().filter(|&&e| e > 0.0) {
            if w <= e {
                break;
            }
            acc += quad(lo, e);
            lo = e;
        }
        acc + quad(lo, w)
    };
    let phi = |w: f64| j1 * r(w) * w + j2 * integral(w);
    times
        .iter()
        .map(|&tt| {
            let f = match reverse {
                Some(tr) if tt > tr => tau0 * (2.0 * tr - tt),
                _ => tau0 * tt,
            };
            let (mut lo, mut hi) = (0.0, 200.0);
            for _ in 0..200 {
                let mid = 0.5 * (lo + hi);
                if phi(mid) < f { lo = mid } else { hi = mid }
            }
            0.5 * (lo + hi)
        })
        .collect()
}

/// The ratio a linear table of the load's speed: `r'` jumps at each of its
/// four inner breakpoints. Stepped over (CVODE's Adams, as the run chose),
/// the load's speed was 9.7, 9.9, 23, 25, 28, 167 and 178 rtol off at rtol
/// 1e-4 … 1e-10; held, 1.1, 1.0, 1.2, 1.8, 2.8, 1.2 and 1.7 rtol, the
/// located run's to the bit (the judge's `kink-gear-linear-located`).
#[test]
fn a_gear_ratio_read_at_the_load_speed_is_held_in_its_cells() {
    let (ws, rs) = ([0.0, 10.0, 25.0, 40.0, 60.0, 80.0], [2.0, 2.5, 2.8, 3.5, 3.6, 4.0]);
    let text = gear(&table_text(&ws, &rs, "rad/s", "linear", "linear"), None);
    let mut data = TableData::new_1d(ws.to_vec(), rs.to_vec());
    data.interpolation = Interpolation::Linear;
    data.outside = [Outside::Linear, Outside::Linear];
    let m = prepare(&text, "Test.KinkGear", true);
    // the ratio's value and its derivative, both read at the load's speed
    assert_eq!(m.held.len(), 2, "{:?}", m.held);
    let times: Vec<f64> = (0..=300).map(|k| k as f64 * 0.01).collect();
    let exact = gear_exact(&data, None, &times);
    assert!(exact.last().unwrap() > &60.0, "it crosses four breakpoints");
    let mut qs = vec![];
    for rtol in RTOLS {
        let r = run(&m, rtol, 3.0, 0.01, Method::Auto);
        let q = error(&r, "w2", &exact) / rtol;
        qs.push(q);
        // each crossing an event of both reads
        assert_eq!(cells(&r), 8, "rtol {rtol:e}");
    }
    println!("error / rtol, held: {qs:.2?}");
    // the measured bound (2.8), with a margin
    assert!(qs.iter().all(|q| *q < 8.0), "{qs:?}");
    // stepped over (for comparison only): not proportional
    let off = prepare(&text, "Test.KinkGear", false);
    assert!(off.held.is_empty());
    let q = error(&run(&off, 1e-10, 3.0, 0.01, Method::Auto), "w2", &exact) / 1e-10;
    println!("error / rtol at rtol 1e-10, stepped over: {q:.1}");
    assert!(q > 50.0, "{q}");
}

/// The ratio a monotone cubic held at its edges (`clamp`): `r'` jumps where
/// the load's speed leaves the data at 40 rad/s and where it comes back,
/// the torque reversed at 2.5 s. Stepped over: 6.7, 17, 14, 4.3, 12, 71 and
/// 131 rtol; held, at most 3.7 (as located).
#[test]
fn a_ratio_held_at_its_edges_is_held_in_its_cells_across_them() {
    let (ws, rs) = ([0.0, 10.0, 25.0, 40.0], [2.0, 2.5, 2.8, 3.5]);
    let text = gear(&table_text(&ws, &rs, "rad/s", "monotoneCubic", "clamp"), Some(2.5));
    let mut data = TableData::new_1d(ws.to_vec(), rs.to_vec());
    data.outside = [Outside::Clamp, Outside::Clamp];
    let m = prepare(&text, "Test.KinkGear", true);
    assert_eq!(m.held.len(), 2, "{:?}", m.held);
    let times: Vec<f64> = (0..=400).map(|k| k as f64 * 0.01).collect();
    let exact = gear_exact(&data, Some(2.5), &times);
    let top = exact.iter().fold(0.0f64, |a, x| a.max(*x));
    assert!(top > 50.0 && *exact.last().unwrap() < 25.0, "it leaves the data and comes back");
    let mut qs = vec![];
    for rtol in RTOLS {
        let r = run(&m, rtol, 4.0, 0.01, Method::Auto);
        qs.push(error(&r, "w2", &exact) / rtol);
    }
    println!("error / rtol, held: {qs:.2?}");
    assert!(qs.iter().all(|q| *q < 10.0), "{qs:?}");
}

/// A linear 1-D table's pieces as `if` branches on its breakpoints (a
/// linear table continued linearly outside: its edge cells continued).
fn if_chain(xs: &[f64], ys: &[f64], arg: &str) -> String {
    let n = xs.len();
    let piece = |i: usize| {
        let s = (ys[i + 1] - ys[i]) / (xs[i + 1] - xs[i]);
        format!("({:?} + ({arg} - ({:?})) * ({s:?}))", ys[i], xs[i])
    };
    let mut out = String::new();
    for i in 0..n - 2 {
        out += &format!("if {arg} < {:?} then {} else", xs[i + 1], piece(i));
    }
    out += &format!(" {}", piece(n - 2));
    format!("({out})")
}

/// A table of two states, `x'' = T(x, y)` with `y'' = -y`, linear in each
/// between its points and continued linearly (the judge's `kink-2d`):
/// stepped over, its error was 1.1, 2.3, 2.4, 1.9, 2.9, 3.0 and 5.0 times
/// that of the same function written as `if` branches on its breakpoints
/// (each crossing an event) at rtol 1e-4 … 1e-10; held, the same run.
#[test]
fn a_table_of_two_states_integrates_as_with_its_breakpoints_located() {
    let (xs, fx) = ([-1.0, -0.4, 0.2, 0.8, 1.4], [2.2, 1.0, -0.8, -2.0, -4.4]);
    let (ys, gy) = ([-0.6, -0.2, 0.3, 0.7], [0.4, 0.0, 0.25, -0.15]);
    let list = |v: &[f64]| v.iter().map(|x| format!("{x:?}")).collect::<Vec<_>>().join(", ");
    let values: Vec<String> =
        fx.iter().map(|a| list(&gy.iter().map(|b| a + b).collect::<Vec<_>>())).collect();
    let model = |name: &str, force: &str| {
        format!(
            r#"model Test.{name} "a mass driven by a table of two states"
  parameter Real k(unit = "1/s2") = 1;
  parameter Real T = table(x1 = {{{}}}, x2 = {{{}}}, values = [{}], x1Unit = "1", x2Unit = "1", interpolation = linear, outside = {{linear, linear}});
  Real x(start = 1.0, fixed = true);
  Real v(unit = "1/s", start = 0.0, fixed = true);
  Real y(start = 0.9, fixed = true);
  Real w(unit = "1/s", start = 0, fixed = true);
equation
  der(x) = v;
  der(v) = k * {force};
  der(y) = w;
  der(w) = -k * y;
end Test.{name};
"#,
            list(&xs),
            list(&ys),
            values.join("; ")
        )
    };
    let table = prepare(&model("Table", "T(x, y)"), "Test.Table", true);
    assert_eq!(table.held.len(), 1);
    let located = format!("({} + {})", if_chain(&xs, &fx, "x"), if_chain(&ys, &gy, "y"));
    let located = prepare(&model("Located", &located), "Test.Located", true);
    assert!(located.held.is_empty() && !located.modes.is_empty());
    let mut ratios = vec![];
    for rtol in RTOLS {
        let a = run(&table, rtol, 30.0, 0.02, Method::Auto);
        let b = run(&located, rtol, 30.0, 0.02, Method::Auto);
        let worst = |r: &SimResult, s: &SimResult| {
            let (x, z) = (r.channel("x").unwrap(), s.channel("x").unwrap());
            x.iter().zip(z).fold(0.0f64, |m, (p, q)| m.max((p - q).abs()))
        };
        // both against a run at a far tighter tolerance of the located model
        let fine = run(&located, 1e-12, 30.0, 0.02, Method::Auto);
        let (ea, eb) = (worst(&a, &fine), worst(&b, &fine));
        ratios.push(ea / eb);
        assert!(cells(&a) > 50, "it crosses its cells' edges: {}", cells(&a));
    }
    println!("error, held / located: {ratios:.3?}");
    assert!(ratios.iter().all(|q| (0.8..1.25).contains(q)), "{ratios:?}");
}

/// What is held: a read at a state that kinks or jumps there. Not a
/// monotone cubic's value continued linearly (C¹), nor a read along time
/// (the run stops at its breakpoints), nor at a parameter.
#[test]
fn reads_are_held_where_they_kink_at_a_state() {
    let text = |table: &str, arg: &str| {
        format!(
            r#"model Test.Spring "a spring whose force is a table"
  parameter Real k(unit = "1/s2") = 1;
  parameter Real p = 0.3;
  parameter Real F = {table};
  Real x(start = 0.5, fixed = true);
  Real v(unit = "1/s", start = 0, fixed = true);
  Real f;
equation
  der(x) = v;
  f = F({arg});
  der(v) = -k * x + k * 0.1 * f;
end Test.Spring;
"#
        )
    };
    let xs = [-1.0, 0.0, 0.4, 1.0];
    let ys = [1.0, 0.0, -0.3, -1.2];
    let held =
        |table: String, arg: &str| prepare(&text(&table, arg), "Test.Spring", true).held.len();
    assert_eq!(held(table_text(&xs, &ys, "1", "linear", "linear"), "x"), 1, "a linear table");
    assert_eq!(held(table_text(&xs, &ys, "1", "monotoneCubic", "clamp"), "x"), 1, "held edges");
    assert_eq!(held(table_text(&xs, &ys, "1", "monotoneCubic", "linear"), "x"), 0, "C¹");
    assert_eq!(held(table_text(&xs, &ys, "s", "linear", "linear"), "time"), 0, "along time");
    assert_eq!(held(table_text(&xs, &ys, "1", "linear", "linear"), "p"), 0, "a parameter");
    // held, the run is the table's: within rounding of it at every point
    let m = prepare(&text(&table_text(&xs, &ys, "1", "linear", "clamp"), "x"), "Test.Spring", true);
    let r = run(&m, 1e-8, 20.0, 0.01, Method::Auto);
    let mut data = TableData::new_1d(xs.to_vec(), ys.to_vec());
    data.interpolation = Interpolation::Linear;
    data.outside = [Outside::Clamp, Outside::Clamp];
    let t = Table::new(&data).unwrap();
    let (x, f) = (r.channel("x").unwrap(), r.channel("f").unwrap());
    let worst = x.iter().zip(f).fold(0.0f64, |m, (a, b)| m.max((t.value([*a, 0.0]) - b).abs()));
    assert!(worst < 1e-14, "{worst:e}");
    assert!(cells(&r) > 10, "{}", cells(&r));
}

/// A held read's polynomial has its table's degree when the model was
/// built: given data that interpolate with a higher one after (a linear
/// table's data swapped for a monotone cubic's), the run stops and says
/// to build the model again, instead of reading the table wrong.
#[test]
fn a_held_table_given_cubic_data_after_the_build_is_refused() {
    let (ws, rs) = ([0.0, 10.0, 25.0, 40.0, 60.0, 80.0], [2.0, 2.5, 2.8, 3.5, 3.6, 4.0]);
    let text = gear(&table_text(&ws, &rs, "rad/s", "linear", "linear"), None);
    let m = prepare(&text, "Test.KinkGear", true);
    let code = lsim_codegen::compile(&m, &Default::default()).expect("compiles");
    let mut data = TableData::new_1d(ws.to_vec(), rs.to_vec());
    data.outside = [Outside::Linear, Outside::Linear];
    let swapped = code.with_tables(&[data]).expect("the same shape");
    let info = RunInfo::from_prepared(&m);
    let opts = SolverOptions { rtol: 1e-6, atol: 1e-8, ..Default::default() };
    let e = lsim_solve::simulate(
        &swapped,
        &info,
        &opts,
        OutputGrid { t0: 0.0, t_end: 3.0, dt: 0.01 },
        &mut [],
    )
    .expect_err("refused");
    assert!(e.to_string().contains("build the model again"), "{e}");
}
