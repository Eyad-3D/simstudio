//! Tables read at a state (DESIGN.md §8.2, *Tables read at a state*;
//! `lsim_prep::hold`). A table read at a position that moves with the
//! states is held in its cell during every step where the model's form
//! needs it (`TableCells::Auto`, the default): on the explicit form
//! (CVODES) a linear table's slope, which stepped over is not proportional
//! to the tolerance; on the implicit form (IDAS) every read that jumps,
//! which stepped over stops the run at the first breakpoint; each crossing
//! of a breakpoint an event. On the explicit form the run releases a held
//! read whose jumps are within the tolerance, or whose crossings come too
//! often (`SolverOptions::held_release`); on the implicit form none.
//! Holding costs a restart at each crossing, and where crossings are many
//! it loses to stepping over where stepping over can go on. Every case
//! against an exact answer (closed forms per cell, a first integral
//! integrated per cell and inverted, an energy that is conserved) or a run
//! at a far tighter tolerance.

use lsim_ir::component::{ComponentDef, Library};
use lsim_ir::interp::Table;
use lsim_ir::math;
use lsim_ir::{HeldCells, Interpolation, Outside, PreparedModel, TableData};
use lsim_lang::{Report, parse_library};
use lsim_prep::hold::TableCells;
use lsim_solve::{EventKind, OutputGrid, RunInfo, SimResult, SolverOptions};

const RTOLS: [f64; 7] = [1e-4, 1e-5, 1e-6, 1e-7, 1e-8, 1e-9, 1e-10];

/// The model `name` of `text` against the library, and it prepared with the
/// tables read at a state held as `cells` asks.
fn build(text: &str, name: &str, cells: TableCells) -> (Library, ComponentDef, PreparedModel) {
    let mut lib = lsim_lib::library();
    let parsed = parse_library(text, Some(&lib)).unwrap_or_else(|e| panic!("{}", Report(&e)));
    lib.types.extend(parsed.types);
    for def in parsed.components.into_values() {
        lib.add(def);
    }
    let top = lib.components.get(name).cloned().expect("the model");
    let opts = lsim_prep::PrepOptions { table_cells: cells, ..Default::default() };
    let m = lsim_prep::prepare(&lib, &top, &opts).unwrap_or_else(|d| panic!("{d:#?}"));
    (lib, top, m)
}

fn prepare(text: &str, name: &str, cells: TableCells) -> PreparedModel {
    build(text, name, cells).2
}

fn options(rtol: f64) -> SolverOptions {
    SolverOptions { rtol, atol: 1e-2 * rtol, ..Default::default() }
}

fn run_with(m: &PreparedModel, opts: &SolverOptions, t_end: f64, dt: f64) -> SimResult {
    let code = lsim_codegen::compile(m, &Default::default()).expect("compiles");
    let info = RunInfo::from_prepared(m);
    lsim_solve::simulate(&code, &info, opts, OutputGrid { t0: 0.0, t_end, dt }, &mut [])
        .unwrap_or_else(|e| panic!("{e}"))
}

fn run(m: &PreparedModel, rtol: f64, t_end: f64, dt: f64) -> SimResult {
    run_with(m, &options(rtol), t_end, dt)
}

/// The largest error of `channel` against `exact` over the grid, relative
/// to its largest exact value.
fn error(r: &SimResult, channel: &str, exact: &[f64]) -> f64 {
    let ch = r.channel(channel).unwrap_or_else(|| panic!("no channel {channel}"));
    assert_eq!(ch.len(), exact.len(), "{channel}: the grid");
    let scale = exact.iter().fold(0.0f64, |a, x| a.max(x.abs()));
    ch.iter().zip(exact).fold(0.0f64, |a, (g, w)| a.max((g - w).abs())) / scale
}

/// Whether every channel of `b` is the same, to the bit, in `a` (which may
/// have more: the discrete variables of the reads it holds).
fn same_channels(a: &SimResult, b: &SimResult) -> bool {
    b.names.iter().all(|n| a.channel(n) == b.channel(n))
}

fn cells(r: &SimResult) -> usize {
    r.events.iter().filter(|e| matches!(e.kind, EventKind::Cell(_))).count()
}

fn grid(t_end: f64, dt: f64) -> Vec<f64> {
    (0..=(t_end / dt).round() as usize).map(|k| k as f64 * dt).collect()
}

fn table_text(xs: &[f64], ys: &[f64], unit: &str, interp: &str, outside: &str) -> String {
    let list = |v: &[f64]| v.iter().map(|x| format!("{x:?}")).collect::<Vec<_>>().join(", ");
    format!(
        "table(x = {{{}}}, y = {{{}}}, xUnit = \"{unit}\", interpolation = {interp}, outside = {{{outside}}})",
        list(xs),
        list(ys)
    )
}

fn data_1d(xs: &[f64], ys: &[f64], interp: Interpolation, outside: Outside) -> TableData {
    let mut d = TableData::new_1d(xs.to_vec(), ys.to_vec());
    d.interpolation = interp;
    d.outside = [outside, outside];
    d
}

/// Gauss–Legendre nodes and weights on [-1, 1], 16 points (each pair ±x).
const GL: [(f64, f64); 8] = [
    (0.0950125098376374, 0.1894506104550685),
    (0.2816035507792589, 0.1826034150449236),
    (0.4580167776572274, 0.1691565193950025),
    (0.6178762444026438, 0.1495959888165767),
    (0.755404408355003, 0.1246289712555339),
    (0.8656312023878318, 0.0951585116824928),
    (0.9445750230732326, 0.0622535239386479),
    (0.9894009349916499, 0.0271524594117541),
];

/// `∫_a^b f` split at the breakpoints `edges` between `a` and `b`, by
/// 16-point Gauss–Legendre on each piece (where `f` is smooth).
fn integral(f: &dyn Fn(f64) -> f64, edges: &[f64], a: f64, b: f64) -> f64 {
    let (lo, hi, sign) = if a <= b { (a, b, 1.0) } else { (b, a, -1.0) };
    let mut cuts = vec![lo];
    cuts.extend(edges.iter().copied().filter(|&e| e > lo && e < hi));
    cuts.push(hi);
    let piece = |a: f64, b: f64| {
        let (m, h) = (0.5 * (a + b), 0.5 * (b - a));
        GL.iter().map(|&(x, w)| w * (f(m - h * x) + f(m + h * x))).sum::<f64>() * h
    };
    sign * cuts.windows(2).map(|w| piece(w[0], w[1])).sum::<f64>()
}

/// A gear (the outside judge's `kink-gear-…`): a rotor of inertia `j1`
/// driven by `tau0` (reversed at `t_rev`) turns a load of inertia `j2`
/// through a gear whose ratio is a table of the load's speed, `w1 = r(w2)
/// w2`: index reduction differentiates it and reads `r'`.
struct Gear {
    j1: f64,
    j2: f64,
    tau0: f64,
    t_rev: Option<f64>,
    /// the load's speed at the start
    w0: f64,
    /// the rotor's speed the state (`stateSelect = prefer`), the load's
    /// computed from it
    rotor_state: bool,
    /// the gear's torques written implicitly, `t2 + ε t2³ = r t1 + ε (r
    /// t1)³` (the same solution, `t2 = r t1`, but `t2` an iteration
    /// variable): the model is of the implicit form, on IDAS
    implicit: bool,
}

const GEAR: Gear = Gear {
    j1: 0.05,
    j2: 2.0,
    tau0: 20.0,
    t_rev: None,
    w0: 0.0,
    rotor_state: false,
    implicit: false,
};

fn gear(table: &str, g: &Gear) -> String {
    gear_with(table, g, "r(w2)")
}

/// [`gear`] with the ratio written `ratio` in its equations (`r(w2)`, or
/// the same function as `if` branches on its breakpoints).
fn gear_with(table: &str, g: &Gear, ratio: &str) -> String {
    let tau = if g.t_rev.is_some() { "if time < t_rev then tau0 else -tau0" } else { "tau0" };
    let (torques, tu) = if g.implicit {
        (
            format!("t2 + 1e-6 * t2^3 / tu^2 = {ratio} * t1 + 1e-6 * ({ratio} * t1)^3 / tu^2"),
            "\n  parameter Real tu(unit = \"N.m\") = 1;\n  parameter Real wu(unit = \"rad/s\") = 1;",
        )
    } else {
        (format!("t2 = {ratio} * t1"), "")
    };
    let (w1, w2) = if g.rotor_state {
        (
            "start = 0, fixed = true, stateSelect = StateSelect.prefer".to_string(),
            "start = 0".into(),
        )
    } else {
        ("start = 0".to_string(), format!("start = {:?}, fixed = true", g.w0))
    };
    format!(
        r#"model Test.KinkGear "a gear whose ratio is a table of the load's speed"
  parameter Real J1(unit = "kg.m2") = {j1:?};
  parameter Real J2(unit = "kg.m2") = {j2:?};
  parameter Real tau0(unit = "N.m") = {tau0:?};
  parameter Real t_rev(unit = "s") = {t_rev:?};{tu}
  parameter Real r = {table};
  Real w1(unit = "rad/s", {w1});
  Real w2(unit = "rad/s", {w2});
  Real t1(unit = "N.m");
  Real t2(unit = "N.m");
  Real tau(unit = "N.m");
equation
  tau = {tau};
  J1 * der(w1) = tau - t1;
  J2 * der(w2) = t2;
  w1 = {ratio} * w2;
  {torques};
end Test.KinkGear;
"#,
        j1 = g.j1,
        j2 = g.j2,
        tau0 = g.tau0,
        t_rev = g.t_rev.unwrap_or(1e9),
    )
}

/// The gear's load speed in closed form: its first integral
/// `Φ(w2) = J1 r(w2) w2 + J2 ∫₀^w2 dw / r(w)` grows by `∫ tau dt`, `Φ`
/// rising with `w2` (the integral by Gauss–Legendre on each cell, where
/// `1 / r` is smooth), inverted by bisection between `lo` and `hi`.
fn gear_exact(data: &TableData, g: &Gear, (lo, hi): (f64, f64), times: &[f64]) -> Vec<f64> {
    let t = Table::new(data).expect("a table");
    let r = |w: f64| t.value([w, 0.0]);
    let inv = |w: f64| 1.0 / r(w);
    let phi = |w: f64| g.j1 * r(w) * w + g.j2 * integral(&inv, &data.x, 0.0, w);
    let start = phi(g.w0);
    times
        .iter()
        .map(|&tt| {
            let push = match g.t_rev {
                Some(tr) if tt > tr => g.tau0 * (2.0 * tr - tt),
                _ => g.tau0 * tt,
            };
            let (mut a, mut b) = (lo, hi);
            for _ in 0..200 {
                let mid = 0.5 * (a + b);
                if phi(mid) < start + push { a = mid } else { b = mid }
            }
            0.5 * (a + b)
        })
        .collect()
}

/// The ratio a linear table of the load's speed: `r'` jumps at each of its
/// four inner breakpoints. Held (`TableCells::Jumps`), the load's speed is
/// 0.83 … 2.0 rtol off
/// at rtol 1e-4 … 1e-10 here; stepped over (`TableCells::Off`), 132 rtol
/// at 1e-10. In the outside judge (its own measure): stepped over
/// (`kink-gear-linear-over`, CVODE's Adams, as the run chose) 9.7, 9.9, 23,
/// 25, 28, 167 and 178 rtol, the error per tolerance growing as it
/// tightens; held (`kink-gear-linear`, the default now) 1.1, 1.0, 1.2,
/// 1.8, 2.8, 1.2 and 1.7, as the same function written as `if`
/// branches on its breakpoints (`kink-gear-linear-located`: the same steps,
/// the states the same to about 1e-12, not to the bit).
#[test]
fn a_gear_ratio_read_at_the_load_speed_is_held_in_its_cells() {
    let (ws, rs) = ([0.0, 10.0, 25.0, 40.0, 60.0, 80.0], [2.0, 2.5, 2.8, 3.5, 3.6, 4.0]);
    let text = gear(&table_text(&ws, &rs, "rad/s", "linear", "linear"), &GEAR);
    let data = data_1d(&ws, &rs, Interpolation::Linear, Outside::Linear);
    let m = prepare(&text, "Test.KinkGear", TableCells::Jumps);
    // the ratio's derivative jumps; its value, read at the same speed, is
    // held with it (their edges are crossed together)
    assert_eq!(m.held.len(), 2, "{:?}", m.held);
    assert!(m.held.iter().all(|h| h.cells == HeldCells::Points));
    let times = grid(3.0, 0.01);
    let exact = gear_exact(&data, &GEAR, (-30.0, 300.0), &times);
    assert!(exact.last().unwrap() > &60.0, "it crosses four breakpoints");
    let mut qs = vec![];
    for rtol in RTOLS {
        let r = run(&m, rtol, 3.0, 0.01);
        qs.push(error(&r, "w2", &exact) / rtol);
        // each crossing an event of both reads
        assert_eq!(cells(&r), 8, "rtol {rtol:e}");
    }
    println!("error / rtol, held: {qs:.2?}");
    // measured at most 2.0, with a margin
    assert!(qs.iter().all(|q| *q < 8.0), "{qs:?}");
    // stepped over (`TableCells::Off`, the default before): not proportional
    let off = prepare(&text, "Test.KinkGear", TableCells::Off);
    assert!(off.held.is_empty());
    let q = error(&run(&off, 1e-10, 3.0, 0.01), "w2", &exact) / 1e-10;
    println!("error / rtol at rtol 1e-10, stepped over: {q:.1}");
    assert!(q > 50.0, "{q}");
}

/// The default (`TableCells::Auto`) holds the reads the model's form needs
/// held (DESIGN.md §8.2, rule v4). On the explicit form (CVODES), a linear
/// table's slope at its points: the gear above is the `Jumps` run to the
/// bit, its two reads held with the releases. A monotone cubic held at its
/// edges is not (its slope jumps at its data's edges only; holding that
/// was measured no better than stepping over on CVODE): the stepped-over
/// run to the bit. (The gear's states can be chosen again during a run, and
/// a choice of the implicit form holds the cubic's edges: every choice holds
/// them, the explicit form releasing them from the start.)
#[test]
fn the_default_holds_what_the_models_form_needs() {
    assert_eq!(lsim_prep::PrepOptions::default().table_cells, TableCells::Auto);
    let (ws, rs) = ([0.0, 10.0, 25.0, 40.0, 60.0, 80.0], [2.0, 2.5, 2.8, 3.5, 3.6, 4.0]);
    let text = gear(&table_text(&ws, &rs, "rad/s", "linear", "linear"), &GEAR);
    let def = prepare(&text, "Test.KinkGear", TableCells::default());
    assert!(def.algebraics.is_empty(), "the explicit form");
    assert_eq!(def.held.len(), 2, "{:?}", def.held);
    let jumps = prepare(&text, "Test.KinkGear", TableCells::Jumps);
    for rtol in [1e-4, 1e-8] {
        let (a, b) = (run(&def, rtol, 3.0, 0.01), run(&jumps, rtol, 3.0, 0.01));
        assert_eq!(cells(&a), 8);
        assert_eq!(a.values, b.values, "rtol {rtol:e}");
    }
    let g = Gear { t_rev: Some(2.5), ..GEAR };
    let (ws, rs) = ([0.0, 10.0, 25.0, 40.0], [2.0, 2.5, 2.8, 3.5]);
    let text = gear(&table_text(&ws, &rs, "rad/s", "monotoneCubic", "clamp"), &g);
    let def = prepare(&text, "Test.KinkGear", TableCells::default());
    assert!(!def.held.is_empty() && def.held.iter().all(|h| !h.explicit), "{:?}", def.held);
    let off = prepare(&text, "Test.KinkGear", TableCells::Off);
    for rtol in [1e-4, 1e-8] {
        let (a, b) = (run(&def, rtol, 4.0, 0.01), run(&off, rtol, 4.0, 0.01));
        assert_eq!(cells(&a), 0);
        assert!(same_channels(&a, &b), "rtol {rtol:e}");
    }
}

/// The gear with its torques written implicitly (`t2` an iteration
/// variable: the implicit form, on IDAS), its ratio a linear table of the
/// load's speed. Stepped over (`TableCells::Off`, 1d41c7d7's default), the
/// run stops at the first breakpoint (w2 = 10 rad/s, t = 0.5088 s) at every
/// tolerance: IDAS cannot step over the jump of `r'` in what it solves; the
/// run names the table, the read and the option that holds it. The
/// default holds the read (no release on this form) and is 0.92 … 4.95
/// rtol off at rtol 1e-4 … 1e-10 (in the judge, `kink-gear-linear-imp`),
/// as the same function written as `if` branches on its breakpoints. The
/// same with every block kept implicit (`force_implicit`).
#[test]
fn the_implicit_form_holds_a_read_whose_slope_jumps() {
    let (ws, rs) = ([0.0, 10.0, 25.0, 40.0, 60.0, 80.0], [2.0, 2.5, 2.8, 3.5, 3.6, 4.0]);
    let g = Gear { implicit: true, ..GEAR };
    let table = table_text(&ws, &rs, "rad/s", "linear", "linear");
    let text = gear(&table, &g);
    let data = data_1d(&ws, &rs, Interpolation::Linear, Outside::Linear);
    let times = grid(3.0, 0.01);
    let exact = gear_exact(&data, &g, (-30.0, 300.0), &times);
    let def = prepare(&text, "Test.KinkGear", TableCells::default());
    assert!(!def.algebraics.is_empty(), "the implicit form");
    assert_eq!(def.held.len(), 2, "{:?}", def.held);
    let located = if_chain(&ws, &rs, "(w2 / wu)");
    let located = prepare(&gear_with(&table, &g, &located), "Test.KinkGear", TableCells::Off);
    assert!(located.held.is_empty() && !located.modes.is_empty());
    let off = prepare(&text, "Test.KinkGear", TableCells::Off);
    let (mut qs, mut ql) = (vec![], vec![]);
    for rtol in RTOLS {
        let r = run(&def, rtol, 3.0, 0.01);
        assert!(r.backend.contains("IDA"), "{}", r.backend);
        assert_eq!(cells(&r), 8, "rtol {rtol:e}");
        qs.push(error(&r, "w2", &exact) / rtol);
        ql.push(error(&run(&located, rtol, 3.0, 0.01), "w2", &exact) / rtol);
        let code = lsim_codegen::compile(&off, &Default::default()).expect("compiles");
        let info = RunInfo::from_prepared(&off);
        let grid = OutputGrid { t0: 0.0, t_end: 3.0, dt: 0.01 };
        let e = lsim_solve::simulate(&code, &info, &options(rtol), grid, &mut [])
            .err()
            .unwrap_or_else(|| panic!("rtol {rtol:e}: stepped over, the run went through"));
        let said = e.to_string();
        assert!(said.contains("t = 0.5087"), "rtol {rtol:e}: {said}");
        for words in
            ["the table 'r'", "its first derivative", "breakpoint 10", "table_cells = \"auto\""]
        {
            assert!(said.contains(words), "rtol {rtol:e}: {said}");
        }
    }
    println!("implicit form, error / rtol held {qs:.2?}, located {ql:.2?}");
    // measured 0.92 … 4.95, the located run the same, with a margin
    assert!(qs.iter().all(|q| *q < 20.0), "{qs:?}");
    assert!(qs.iter().zip(&ql).all(|(h, l)| (h / l - 1.0).abs() < 0.25), "{qs:?} {ql:?}");
    // every block implicit (the plain gear): held by default too
    let (lib, top, _) = build(&gear(&table, &GEAR), "Test.KinkGear", TableCells::default());
    let opts = lsim_prep::PrepOptions { force_implicit: true, ..Default::default() };
    let all = lsim_prep::prepare(&lib, &top, &opts).unwrap_or_else(|d| panic!("{d:#?}"));
    assert!(!all.algebraics.is_empty() && all.held.len() == 2, "{:?}", all.held);
    let mut qa = vec![];
    for rtol in [1e-4, 1e-7, 1e-10] {
        let r = run(&all, rtol, 3.0, 0.01);
        assert_eq!(cells(&r), 8, "rtol {rtol:e}");
        qa.push(error(&r, "w2", &exact) / rtol);
    }
    println!("every block implicit, error / rtol held {qa:.2?}");
    // measured 1.41, 1.68, 2.75
    assert!(qa.iter().all(|q| *q < 20.0), "{qa:?}");
}

/// The judge's dense gear (`kink-dense-gear-41`: the ratio linear through
/// `2 + w/40 + 0.25 sin(w/8)` at 41 points on [0, 80] rad/s, 32 crossings
/// in 3 s) with its torques written implicitly (the implicit form). Held
/// with the releases as they were (1d41c7d7's `Jumps`), the run let go of
/// the read where its crossings came often (at rtol 1e-4 and 1e-5) and
/// IDAS then stopped at the next breakpoint ("made no progress"). On the
/// implicit form the run releases nothing: the default holds the read
/// through every crossing, the run with the releases switched off to the
/// bit, 9.85 and 11.7 rtol off at rtol 1e-4 and 1e-5 (the first integral,
/// per cell). Stepped over, it stops at a breakpoint.
#[test]
fn the_implicit_form_holds_a_dense_table_through_every_crossing() {
    let ws: Vec<f64> = (0..41).map(|k| 80.0 * k as f64 / 40.0).collect();
    let rs: Vec<f64> = ws.iter().map(|w| 2.0 + w / 40.0 + 0.25 * math::sin(w / 8.0)).collect();
    let g = Gear { implicit: true, ..GEAR };
    let text = gear(&table_text(&ws, &rs, "rad/s", "linear", "linear"), &g);
    let data = data_1d(&ws, &rs, Interpolation::Linear, Outside::Linear);
    let times = grid(3.0, 0.01);
    let exact = gear_exact(&data, &g, (-30.0, 300.0), &times);
    let def = prepare(&text, "Test.KinkGear", TableCells::default());
    assert!(!def.algebraics.is_empty() && def.held.len() == 2, "{:?}", def.held);
    let mut qs = vec![];
    for rtol in [1e-4, 1e-5] {
        let r = run(&def, rtol, 3.0, 0.01);
        assert!(
            !r.report.notes.iter().any(|n| n.contains("no longer held")),
            "rtol {rtol:e}: {:?}",
            r.report.notes
        );
        assert_eq!(cells(&r), 64, "rtol {rtol:e}: every crossing, both reads");
        let always =
            run_with(&def, &SolverOptions { held_release: false, ..options(rtol) }, 3.0, 0.01);
        assert_eq!(r.values, always.values, "rtol {rtol:e}");
        qs.push(error(&r, "w2", &exact) / rtol);
    }
    println!("dense gear, implicit form: error / rtol held {qs:.2?}");
    // measured 9.85 and 11.7, with a margin
    assert!(qs.iter().all(|q| *q < 40.0), "{qs:?}");
    let off = prepare(&text, "Test.KinkGear", TableCells::Off);
    let code = lsim_codegen::compile(&off, &Default::default()).expect("compiles");
    let e = lsim_solve::simulate(
        &code,
        &RunInfo::from_prepared(&off),
        &options(1e-4),
        OutputGrid { t0: 0.0, t_end: 3.0, dt: 0.01 },
        &mut [],
    )
    .expect_err("stepped over on the implicit form");
    assert!(e.to_string().contains("table_cells = \"auto\""), "{e}");
}

/// A monotone cubic held at its edges (`clamp`), the gear with its torques
/// written implicitly (the judge's `kink-gear-clamp` on IDAS): the load's
/// speed leaves the data at 40 rad/s (t = 1.84 s), where `r'` jumps to 0.
/// Stepped over, the run stops there (or gives no answer within 120 s) at
/// every tolerance, naming the table; the default holds it at its edges:
/// 0.53 … 5.2 rtol off at rtol 1e-4 … 1e-10 (the first integral; in the
/// judge's measure, both speeds, 0.78 … 5.2, the same function as `if`
/// branches 0.29 … 4.6).
#[test]
fn the_implicit_form_holds_a_clamped_tables_edges() {
    let (ws, rs) = ([0.0, 10.0, 25.0, 40.0], [2.0, 2.5, 2.8, 3.5]);
    let g = Gear { t_rev: Some(2.5), implicit: true, ..GEAR };
    let text = gear(&table_text(&ws, &rs, "rad/s", "monotoneCubic", "clamp"), &g);
    let data = data_1d(&ws, &rs, Interpolation::MonotoneCubic, Outside::Clamp);
    let def = prepare(&text, "Test.KinkGear", TableCells::default());
    assert!(!def.algebraics.is_empty(), "the implicit form");
    assert_eq!(def.held.len(), 2, "{:?}", def.held);
    assert!(def.held.iter().all(|h| matches!(h.cells, HeldCells::Edges(_))), "{:?}", def.held);
    let times = grid(4.0, 0.01);
    let exact = gear_exact(&data, &g, (-30.0, 300.0), &times);
    let mut qs = vec![];
    for rtol in RTOLS {
        let r = run(&def, rtol, 4.0, 0.01);
        assert_eq!(cells(&r), 4, "rtol {rtol:e}: out at 40 rad/s and back, both reads");
        qs.push(error(&r, "w2", &exact) / rtol);
    }
    println!("clamped cubic, implicit form: error / rtol held at the edges {qs:.2?}");
    // measured 0.53 … 5.2, with a margin
    assert!(qs.iter().all(|q| *q < 20.0), "{qs:?}");
    let off = prepare(&text, "Test.KinkGear", TableCells::Off);
    let code = lsim_codegen::compile(&off, &Default::default()).expect("compiles");
    let e = lsim_solve::simulate(
        &code,
        &RunInfo::from_prepared(&off),
        &options(1e-6),
        OutputGrid { t0: 0.0, t_end: 4.0, dt: 0.01 },
        &mut [],
    )
    .expect_err("stepped over on the implicit form");
    let said = e.to_string();
    for words in ["t = 1.84", "the table 'r'", "breakpoint 40", "table_cells = \"auto\""] {
        assert!(said.contains(words), "{said}");
    }
}

/// A gear whose ratio is a table of two states, bilinear (`f(w2) + g(c)` on
/// its grid, `c` a state rising at 1/s), its torques written implicitly:
/// index reduction reads both of its slopes, which jump at the grid's lines.
/// Stepped over, the run stops at the first (w2 = 10 rad/s, t = 0.486 s)
/// or gives no answer within 120 s; the default holds the 2-D read at
/// every breakpoint of both arguments: 1.6, 3.9 and 3.0 rtol off at rtol
/// 1e-4, 1e-7 and 1e-10 against the `if` branches' run at 1e-12, as that
/// function's own runs (in the judge's measure, DOP853 per pair of cells,
/// 0.87 … 4.96 at 1e-4 … 1e-10).
#[test]
fn the_implicit_form_holds_a_two_dimensional_read_that_jumps() {
    let (ws, fw) = ([0.0, 10.0, 25.0, 40.0, 60.0, 80.0], [2.0, 2.5, 2.8, 3.5, 3.6, 4.0]);
    let (cs, gc) = ([-1.0, 0.7, 1.6, 2.4, 4.0], [0.0, 0.2, -0.1, 0.25, 0.1]);
    let list = |v: &[f64]| v.iter().map(|x| format!("{x:?}")).collect::<Vec<_>>().join(", ");
    let values: Vec<String> =
        fw.iter().map(|a| list(&gc.iter().map(|b| a + b).collect::<Vec<_>>())).collect();
    let table = format!(
        "table(x1 = {{{}}}, x2 = {{{}}}, values = [{}], x1Unit = \"rad/s\", x2Unit = \"1\", interpolation = linear, outside = {{linear, linear}})",
        list(&ws),
        list(&cs),
        values.join("; ")
    );
    let model = |ratio: &str| {
        format!(
            r#"model Test.Gear2D "a gear whose ratio is a table of the load's speed and of a state c"
  parameter Real J1(unit = "kg.m2") = 0.05;
  parameter Real J2(unit = "kg.m2") = 2;
  parameter Real tau0(unit = "N.m") = 20;
  parameter Real cu(unit = "1/s") = 1;
  parameter Real tu(unit = "N.m") = 1;
  parameter Real wu(unit = "rad/s") = 1;
  parameter Real r = {table};
  Real w1(unit = "rad/s", start = 0);
  Real w2(unit = "rad/s", start = 0, fixed = true);
  Real c(start = 0, fixed = true, stateSelect = StateSelect.always);
  Real t1(unit = "N.m");
  Real t2(unit = "N.m");
equation
  der(c) = cu;
  J1 * der(w1) = tau0 - t1;
  J2 * der(w2) = t2;
  w1 = {ratio} * w2;
  t2 + 1e-6 * t2^3 / tu^2 = {ratio} * t1 + 1e-6 * ({ratio} * t1)^3 / tu^2;
end Test.Gear2D;
"#
        )
    };
    let def = prepare(&model("r(w2, c)"), "Test.Gear2D", TableCells::default());
    assert!(!def.algebraics.is_empty(), "the implicit form");
    assert!(!def.held.is_empty() && def.held.iter().all(|h| h.axes.len() == 2), "{:?}", def.held);
    let located = format!("({} + {})", if_chain(&ws, &fw, "(w2 / wu)"), if_chain(&cs, &gc, "c"));
    let located = prepare(&model(&located), "Test.Gear2D", TableCells::Off);
    assert!(located.held.is_empty());
    // the reference: the located run at a far tighter tolerance
    let fine = run(&located, 1e-12, 3.0, 0.01);
    let fw2 = fine.channel("w2").unwrap().to_vec();
    let (mut qs, mut ql) = (vec![], vec![]);
    for rtol in [1e-4, 1e-7, 1e-10] {
        let r = run(&def, rtol, 3.0, 0.01);
        assert!(cells(&r) > 4, "rtol {rtol:e}: {}", cells(&r));
        qs.push(error(&r, "w2", &fw2) / rtol);
        ql.push(error(&run(&located, rtol, 3.0, 0.01), "w2", &fw2) / rtol);
    }
    println!("2-D read, implicit form: error / rtol held {qs:.2?}, located {ql:.2?}");
    // measured 1.6, 3.9 and 3.0, the located run the same, with a margin
    assert!(qs.iter().all(|q| *q < 20.0), "{qs:?}");
    assert!(qs.iter().zip(&ql).all(|(h, l)| (h / l - 1.0).abs() < 0.25), "{qs:?} {ql:?}");
    let off = prepare(&model("r(w2, c)"), "Test.Gear2D", TableCells::Off);
    let code = lsim_codegen::compile(&off, &Default::default()).expect("compiles");
    let e = lsim_solve::simulate(
        &code,
        &RunInfo::from_prepared(&off),
        &options(1e-4),
        OutputGrid { t0: 0.0, t_end: 3.0, dt: 0.01 },
        &mut [],
    )
    .expect_err("stepped over on the implicit form");
    assert!(e.to_string().contains("the table 'r'"), "{e}");
}

/// A bead on a wire whose cubic's second derivative really jumps (`y =
/// r(x)`, `r` the default table of `x²/2 + sin(3x)/4` at 11 points, `s` a
/// table of its slope there; rule v3's held-out case), every block kept
/// implicit (the implicit form): index reduction reads `r''`. Stepped over,
/// IDAS gives no answer within 120 s at any tolerance (r4 work data); the
/// default holds the reads at every point (50 crossings, each an event of
/// both reads): 13.8, 20.7 and 12.3 rtol off at rtol 1e-4, 1e-6 and 1e-8
/// against the same model on the explicit form held at rtol 1e-12 (7.3 …
/// 21.9 at 1e-4 … 1e-10 against DOP853 per cell).
#[test]
fn the_implicit_form_holds_a_cubics_second_derivative() {
    let xs: Vec<f64> = (0..11).map(|k| -2.0 + 0.4 * k as f64).collect();
    let rs: Vec<f64> = xs.iter().map(|x| 0.5 * x * x + 0.25 * math::sin(3.0 * x)).collect();
    let ss: Vec<f64> = xs.iter().map(|x| x + 0.75 * math::cos(3.0 * x)).collect();
    let list = |v: &[f64]| v.iter().map(|x| format!("{x:?}")).collect::<Vec<_>>().join(", ");
    let tab = |v: &[f64]| {
        format!(
            "table(x = {{{}}}, y = {{{}}}, xUnit = \"m\", outside = {{linear}})",
            list(&xs),
            list(v)
        )
    };
    let text = format!(
        r#"model Test.Bead "a bead on a wire y = r(x) under gravity, r the default table"
  parameter Real m(unit = "kg") = 1;
  parameter Real g(unit = "m/s2") = 9.81;
  parameter Real r(unit = "m") = {} "the wire's height";
  parameter Real s = {} "its slope";
  Real x(unit = "m", start = 1.5, fixed = true);
  Real vx(unit = "m/s", start = 0, fixed = true);
  Real y(unit = "m");
  Real vy(unit = "m/s");
  Real F(unit = "N") "the wire's force along y";
equation
  der(x) = vx;
  der(y) = vy;
  m * der(vx) = -s(x) * F;
  m * der(vy) = F - m * g;
  y = r(x) "on the wire";
end Test.Bead;
"#,
        tab(&rs),
        tab(&ss)
    );
    let (lib, top, explicit) = build(&text, "Test.Bead", TableCells::Kinks);
    assert!(explicit.algebraics.is_empty() && !explicit.held.is_empty());
    let fine =
        run_with(&explicit, &SolverOptions { held_release: false, ..options(1e-12) }, 10.0, 0.01);
    let (fx, fv) = (fine.channel("x").unwrap().to_vec(), fine.channel("vx").unwrap().to_vec());
    let opts = lsim_prep::PrepOptions { force_implicit: true, ..Default::default() };
    let def = lsim_prep::prepare(&lib, &top, &opts).unwrap_or_else(|d| panic!("{d:#?}"));
    assert!(!def.algebraics.is_empty() && !def.held.is_empty(), "{:?}", def.held);
    let mut qs = vec![];
    for rtol in [1e-4, 1e-6, 1e-8] {
        let r = run(&def, rtol, 10.0, 0.01);
        assert!(r.backend.contains("IDA"), "{}", r.backend);
        assert!(cells(&r) > 50, "rtol {rtol:e}: {}", cells(&r));
        qs.push(error(&r, "x", &fx).max(error(&r, "vx", &fv)) / rtol);
    }
    println!("bead (r'' jumps), implicit form: error / rtol held {qs:.2?}");
    // measured 13.8, 20.7 and 12.3, with a margin
    assert!(qs.iter().all(|q| *q < 60.0), "{qs:?}");
}

/// The judge's dense gear on the explicit form (`kink-dense-gear-41`), its
/// read held with the releases: at rtol 1e-4 the run lets go of it after
/// its first crossings (t = 0.87 s), and at t = 2.36 s dynamic state
/// selection takes the rotor's speed as the state, whose model is of the
/// implicit form. There the released read would be stepped over, which
/// IDAS cannot do across a jump: it is held in its cells again (before, it
/// went on released). The load's speed is 9.0 rtol off its first integral
/// (in the judge's measure, both speeds, 11.5).
#[test]
fn a_released_read_is_held_again_on_the_implicit_form() {
    let ws: Vec<f64> = (0..41).map(|k| 80.0 * k as f64 / 40.0).collect();
    let rs: Vec<f64> = ws.iter().map(|w| 2.0 + w / 40.0 + 0.25 * math::sin(w / 8.0)).collect();
    let text = gear(&table_text(&ws, &rs, "rad/s", "linear", "linear"), &GEAR);
    let data = data_1d(&ws, &rs, Interpolation::Linear, Outside::Linear);
    let (lib, top, m) = build(&text, "Test.KinkGear", TableCells::default());
    assert!(m.algebraics.is_empty() && m.held.len() == 2, "{:?}", m.held);
    let code = lsim_codegen::compile(&m, &Default::default()).expect("compiles");
    let mut info = RunInfo::from_prepared(&m);
    info.choices = lsim_project::model::choices(&lib, &top, &Default::default(), &info);
    let out = OutputGrid { t0: 0.0, t_end: 3.0, dt: 0.01 };
    let r = lsim_solve::simulate(&code, &info, &options(1e-4), out, &mut [])
        .unwrap_or_else(|e| panic!("{e}"));
    let said = |w: &str| r.report.notes.iter().any(|n| n.contains(w));
    assert!(said("no longer held"), "{:?}", r.report.notes);
    assert!(said("held in its cells again"), "{:?}", r.report.notes);
    assert!(r.backend.contains("IDA"), "{}", r.backend);
    let switched =
        r.events.iter().find(|e| matches!(e.kind, EventKind::States(_))).expect("a switch").t;
    let after = r.events.iter().filter(|e| matches!(e.kind, EventKind::Cell(_)) && e.t > switched);
    assert!(after.count() > 0, "held again: its crossings are events");
    let times = grid(3.0, 0.01);
    let exact = gear_exact(&data, &GEAR, (-30.0, 300.0), &times);
    let q = error(&r, "w2", &exact) / 1e-4;
    println!("released, then held again on the implicit form: error / rtol {q:.2}");
    // measured 9.0, with a margin
    assert!(q < 50.0, "{q}");
}

/// A run that fails after it released a held read says so: the release's
/// note, which a run that goes through reports, comes with its failure too
/// (here an `assert` that stops the run after the release). Released at
/// the start (a linear table of collinear points: its jumps are within the
/// tolerance), and during the run (the judge's `kink-osc-41`, on the
/// explicit form, its crossings too frequent at rtol 1e-4: released at t =
/// 0.226 s).
#[test]
fn a_run_that_fails_after_a_release_says_so() {
    let ws = [0.0, 10.0, 25.0, 40.0, 60.0, 80.0];
    let rs: Vec<f64> = ws.iter().map(|w| 2.0 + w / 50.0).collect();
    let text = gear(&table_text(&ws, &rs, "rad/s", "linear", "linear"), &GEAR)
        .replace("  tau = ", "  assert(time < 1.0, \"the test stops here\");\n  tau = ");
    let m = prepare(&text, "Test.KinkGear", TableCells::Jumps);
    assert_eq!(m.held.len(), 2);
    let code = lsim_codegen::compile(&m, &Default::default()).expect("compiles");
    let grid = OutputGrid { t0: 0.0, t_end: 3.0, dt: 0.01 };
    let e = lsim_solve::simulate(&code, &RunInfo::from_prepared(&m), &options(1e-6), grid, &mut [])
        .expect_err("the assert stops it");
    let said = e.to_string();
    assert!(said.contains("the test stops here"), "{said}");
    assert!(
        said.contains("earlier in the run") && said.contains("continuous at its points"),
        "{said}"
    );
    let ps: Vec<f64> = (0..41).map(|k| -30.0 + 60.0 * k as f64 / 40.0).collect();
    let vs: Vec<f64> = ps.iter().map(|p| 2.5 + 0.02 * p + 0.2 * math::sin(p / 4.0)).collect();
    let text = gear_spring(&table_text(&ps, &vs, "rad/s", "linear", "linear"), false).replace(
        "  der(phi) = w2;",
        "  der(phi) = w2;\n  assert(time < 0.5, \"the test stops here\");",
    );
    let m = prepare(&text, "Test.GearSpring", TableCells::Jumps);
    let code = lsim_codegen::compile(&m, &Default::default()).expect("compiles");
    let grid = OutputGrid { t0: 0.0, t_end: 20.0, dt: 0.01 };
    let e = lsim_solve::simulate(&code, &RunInfo::from_prepared(&m), &options(1e-4), grid, &mut [])
        .expect_err("the assert stops it");
    let said = e.to_string();
    assert!(said.contains("the test stops here"), "{said}");
    assert!(
        said.contains("earlier in the run") && said.contains("no longer held in its cells"),
        "{said}"
    );
}

/// The ratio a monotone cubic held at its edges (`clamp`): `r'` jumps where
/// the load's speed leaves the data at 40 rad/s and where it comes back,
/// the torque reversed at 2.5 s, and only kinks at the points between
/// (`r''` jumps there). Held at its edges only: a crossing out and one
/// back, an event of each of its two reads, where every point was an event
/// before. Here held 0.88 … 8.4 rtol at rtol 1e-4 … 1e-10, stepped over 90
/// at 1e-10; in the judge (`kink-gear-clamp`) held 1.1 … 12.5, stepped over
/// 6.7, 17, 14, 4.3, 12, 71 and 131, held at every point 0.7 … 3.7.
#[test]
fn a_ratio_held_at_its_edges_is_held_in_its_cells_at_the_edges_only() {
    let (ws, rs) = ([0.0, 10.0, 25.0, 40.0], [2.0, 2.5, 2.8, 3.5]);
    let g = Gear { t_rev: Some(2.5), ..GEAR };
    let text = gear(&table_text(&ws, &rs, "rad/s", "monotoneCubic", "clamp"), &g);
    let data = data_1d(&ws, &rs, Interpolation::MonotoneCubic, Outside::Clamp);
    let m = prepare(&text, "Test.KinkGear", TableCells::Jumps);
    assert_eq!(m.held.len(), 2, "{:?}", m.held);
    assert!(m.held.iter().all(|h| matches!(h.cells, HeldCells::Edges(_))), "{:?}", m.held);
    let times = grid(4.0, 0.01);
    let exact = gear_exact(&data, &g, (-30.0, 300.0), &times);
    let top = exact.iter().fold(0.0f64, |a, x| a.max(*x));
    assert!(top > 50.0 && *exact.last().unwrap() < 25.0, "it leaves the data and comes back");
    let mut qs = vec![];
    for rtol in RTOLS {
        let r = run(&m, rtol, 4.0, 0.01);
        qs.push(error(&r, "w2", &exact) / rtol);
        assert_eq!(cells(&r), 4, "rtol {rtol:e}: out at 40 rad/s and back, both reads");
    }
    println!("error / rtol, held at the edges: {qs:.2?}");
    // measured at most 8.4 (at 1e-9: the kinks between the points, stepped
    // over), with a margin
    assert!(qs.iter().all(|q| *q < 40.0), "{qs:?}");
    let off = prepare(&text, "Test.KinkGear", TableCells::Off);
    let q = error(&run(&off, 1e-10, 4.0, 0.01), "w2", &exact) / 1e-10;
    println!("error / rtol at rtol 1e-10, stepped over: {q:.1}");
    assert!(q > 50.0, "{q}");
}

/// A held read whose argument starts exactly on a breakpoint, moving up
/// into the cell above it or pushed down into the cell below: one event at
/// the start at most, and the run as accurate as from inside a cell.
#[test]
fn a_held_read_starting_on_a_breakpoint() {
    let (ws, rs) = ([0.0, 10.0, 25.0, 40.0, 60.0, 80.0], [2.0, 2.5, 2.8, 3.5, 3.6, 4.0]);
    let data = data_1d(&ws, &rs, Interpolation::Linear, Outside::Linear);
    for tau0 in [20.0, -20.0] {
        let g = Gear { tau0, w0: 25.0, ..GEAR };
        let text = gear(&table_text(&ws, &rs, "rad/s", "linear", "linear"), &g);
        let m = prepare(&text, "Test.KinkGear", TableCells::Jumps);
        let times = grid(1.0, 0.01);
        let exact = gear_exact(&data, &g, (-30.0, 300.0), &times);
        let (first, last) = (exact[1], *exact.last().unwrap());
        assert!(if tau0 > 0.0 { first > 25.0 && last > 40.0 } else { first < 25.0 && last < 10.0 });
        // the breakpoints crossed, each an event of both reads: moving up,
        // those above the start (the read starts in the cell above it, as
        // the table takes it); pushed down, the start's own (crossed at
        // once) and those below
        let crossed = ws
            .iter()
            .filter(|&&p| if tau0 > 0.0 { p > 25.0 && p < last } else { p <= 25.0 && p > last })
            .count();
        let mut qs = vec![];
        for rtol in [1e-4, 1e-7, 1e-10] {
            let r = run(&m, rtol, 1.0, 0.01);
            qs.push(error(&r, "w2", &exact) / rtol);
            assert_eq!(cells(&r), 2 * crossed, "torque {tau0}, rtol {rtol:e}: {:?}", r.events);
        }
        println!("torque {tau0}: error / rtol {qs:.2?}");
        assert!(qs.iter().all(|q| *q < 8.0), "torque {tau0}: {qs:?}");
    }
}

/// A gear whose ratio falls with the load's speed, the rotor's speed the
/// preferred state: the pivot `r + r' w2` of `w1 = r(w2) w2` falls through
/// zero at w2 = 18.3 rad/s and dynamic state selection switches to the
/// load's speed, the ratio's reads held in their cells across the switch
/// (the two models hold the same reads in the same variables).
#[test]
fn the_states_chosen_again_with_held_reads() {
    let (ws, rs) = ([0.0, 10.0, 20.0, 30.0, 40.0], [2.0, 1.6, 1.0, 0.6, 0.5]);
    let g = Gear { tau0: 8.0, rotor_state: true, ..GEAR };
    let text = gear(&table_text(&ws, &rs, "rad/s", "linear", "linear"), &g);
    let data = data_1d(&ws, &rs, Interpolation::Linear, Outside::Linear);
    let (lib, top, m) = build(&text, "Test.KinkGear", TableCells::Jumps);
    assert_eq!(m.held.len(), 2);
    let code = lsim_codegen::compile(&m, &Default::default()).expect("compiles");
    let mut info = RunInfo::from_prepared(&m);
    // (prepared again as it was: holding the same reads)
    let opts = lsim_prep::PrepOptions { table_cells: TableCells::Jumps, ..Default::default() };
    info.choices = lsim_project::model::choices(&lib, &top, &opts, &info);
    assert!(info.choices.is_some(), "the choice of states can move");
    let times = grid(6.0, 0.01);
    let exact = gear_exact(&data, &g, (-20.0, 80.0), &times);
    assert!(*exact.last().unwrap() > 20.0, "past two breakpoints and the pivot's zero");
    let mut qs = vec![];
    // (at rtol 1e-8 the run is 684 rtol off, held or with the breakpoints
    // written as `if` branches alike, as it was before the reads were held:
    // an open item of dynamic state selection, STATUS.md)
    for rtol in [1e-4, 1e-6, 1e-10] {
        let r = lsim_solve::simulate(
            &code,
            &info,
            &options(rtol),
            OutputGrid { t0: 0.0, t_end: 6.0, dt: 0.01 },
            &mut [],
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let switches = r.events.iter().filter(|e| matches!(e.kind, EventKind::States(_))).count();
        assert_eq!(switches, 1, "rtol {rtol:e}: {:?}", r.events);
        assert_eq!(cells(&r), 4, "rtol {rtol:e}: 10 and 20 rad/s, both reads");
        qs.push(error(&r, "w2", &exact) / rtol);
    }
    println!("error / rtol across the switch: {qs:.2?}");
    // measured 3.9, 1.8, 2.9
    assert!(qs.iter().all(|q| *q < 12.0), "{qs:?}");
}

/// The model `name` of `text` built with its choices of states (dynamic
/// state selection: a run can switch them), the tables read at a state held
/// as `cells` asks.
fn choosing(text: &str, name: &str, cells: TableCells) -> lsim_project::model::Model {
    let (lib, top, _) = build(text, name, cells);
    let opts = lsim_prep::PrepOptions { table_cells: cells, ..Default::default() };
    let m = lsim_project::model::Model::build_with(&lib, &top, &opts)
        .unwrap_or_else(|d| panic!("{d:#?}"));
    assert!(m.info.choices.is_some(), "the choice of states can move");
    m
}

/// A run of `m` with its choices of states, and its switches' times.
fn run_choosing(
    m: &lsim_project::model::Model,
    opts: &SolverOptions,
    t_end: f64,
) -> Result<(SimResult, Vec<f64>), String> {
    let r =
        m.run(opts, OutputGrid { t0: 0.0, t_end, dt: 0.01 }, &mut []).map_err(|e| e.to_string())?;
    let at =
        r.events.iter().filter(|e| matches!(e.kind, EventKind::States(_))).map(|e| e.t).collect();
    Ok((r, at))
}

/// A gear whose ratio is a table of two states, `r(w2, c) = f(w2) + g(c)`
/// bilinear on its grid (`c` a state rising at 1/s, `stateSelect =
/// always`), falling with the load's speed, the rotor's speed the preferred
/// state; `prefer`: else the load's speed is the state.
fn switch_gear_2d(prefer: bool) -> String {
    let (ws, fs) = ([-10.0, 0.0, 10.0, 20.0, 30.0, 40.0], [2.2, 2.0, 1.6, 1.0, 0.6, 0.5]);
    let (cs, gs) = ([-1.0, 1.0, 2.5, 8.0], [0.0, 0.05, -0.05, 0.1]);
    let list = |v: &[f64]| v.iter().map(|x| format!("{x:?}")).collect::<Vec<_>>().join(", ");
    let values: Vec<String> =
        fs.iter().map(|a| list(&gs.iter().map(|b| a + b).collect::<Vec<_>>())).collect();
    let (w1, w2) = if prefer {
        ("start = 0, fixed = true, stateSelect = StateSelect.prefer", "start = 0")
    } else {
        ("start = 0", "start = 0, fixed = true")
    };
    format!(
        r#"model Test.SwitchGear2D "a gear whose ratio is a table of the load's speed and of a state c"
  parameter Real J1(unit = "kg.m2") = 0.05;
  parameter Real J2(unit = "kg.m2") = 2;
  parameter Real tau0(unit = "N.m") = 8;
  parameter Real cu(unit = "1/s") = 1;
  parameter Real r = table(x1 = {{{}}}, x2 = {{{}}}, values = [{}], x1Unit = "rad/s", x2Unit = "1", interpolation = linear, outside = {{linear, linear}});
  Real w1(unit = "rad/s", {w1});
  Real w2(unit = "rad/s", {w2});
  Real c(start = 0, fixed = true, stateSelect = StateSelect.always);
  Real t1(unit = "N.m");
  Real t2(unit = "N.m");
equation
  der(c) = cu;
  J1 * der(w1) = tau0 - t1;
  J2 * der(w2) = t2;
  w1 = r(w2, c) * w2;
  t2 = r(w2, c) * t1;
end Test.SwitchGear2D;
"#,
        list(&ws),
        list(&cs),
        values.join("; ")
    )
}

/// A bead on the wire `y = r(x)` (`r` the default table of `x²/2 +
/// sin(3x)/4` at 11 points, continued linearly; `s` a table of its slope),
/// from x = 1.5 at rest; `prefer`: its height and its vertical speed the
/// preferred states (`x` then decided from `y` by Newton: the implicit
/// form). Index reduction reads `r''`.
fn switch_bead(prefer: bool) -> String {
    let xs: Vec<f64> = (0..11).map(|k| -2.0 + 0.4 * k as f64).collect();
    let rs: Vec<f64> = xs.iter().map(|x| 0.5 * x * x + 0.25 * math::sin(3.0 * x)).collect();
    let ss: Vec<f64> = xs.iter().map(|x| x + 0.75 * math::cos(3.0 * x)).collect();
    let list = |v: &[f64]| v.iter().map(|x| format!("{x:?}")).collect::<Vec<_>>().join(", ");
    let tab = |v: &[f64]| {
        format!(
            "table(x = {{{}}}, y = {{{}}}, xUnit = \"m\", outside = {{linear}})",
            list(&xs),
            list(v)
        )
    };
    let sel = if prefer { ", stateSelect = StateSelect.prefer" } else { "" };
    format!(
        r#"model Test.Bead "a bead on a wire y = r(x) under gravity, r the default table"
  parameter Real m(unit = "kg") = 1;
  parameter Real g(unit = "m/s2") = 9.81;
  parameter Real r(unit = "m") = {} "the wire's height";
  parameter Real s = {} "its slope";
  Real x(unit = "m", start = 1.5, fixed = true);
  Real vx(unit = "m/s", start = 0, fixed = true);
  Real y(unit = "m"{sel});
  Real vy(unit = "m/s"{sel});
  Real F(unit = "N") "the wire's force along y";
equation
  der(x) = vx;
  der(y) = vy;
  m * der(vx) = -s(x) * F;
  m * der(vy) = F - m * g;
  y = r(x) "on the wire";
end Test.Bead;
"#,
        tab(&rs),
        tab(&ss)
    )
}

/// A change of the states chosen from the implicit form (IDAS) to the
/// explicit one (CVODES), the model reading a table at a state whose read
/// jumps there: a monotone cubic held at its edges (`r'` jumps at the
/// data's edges), a 2-D read whose slope jumps, a cubic's second
/// derivative. The implicit form holds these by default and the explicit
/// one steps over them; before, the model with the states chosen again held
/// other reads, the switch was refused ("other events, discrete values or
/// inputs") and the run stopped where its states failed (t = 3.137, 2.993,
/// 0.867 s), where 1d41c7d7's run (`TableCells::Off`) goes through. Every
/// choice of states of such a model now holds what either form holds, and
/// the run releases what only the implicit form holds while it is on the
/// explicit one: the switch goes through, as 1d41c7d7's. The gear held at
/// its edges never reaches them: its run is 1d41c7d7's to the bit. In the
/// judge's measure (`r5/b1`, against Radau per cell, DOP853 per cell and the
/// first integral): the gear 1.63, 3.56, 358 at rtol 1e-4, 1e-7, 1e-10; the
/// 2-D gear 3.1, 4.71, 137 (stepped over 3.43, 5.0, 128); the bead, whose
/// `r''` both forms hold, 64.5, 132, 335 (stepped over 222, 197, 208; held
/// on the implicit form only, 102, 150, 420).
#[test]
fn a_change_of_states_onto_the_explicit_form_keeps_the_held_reads() {
    // the gear, its ratio a monotone cubic held at its edges, falling
    let (ws, rs) = ([-10.0, 0.0, 10.0, 20.0, 30.0, 40.0], [2.2, 2.0, 1.6, 1.0, 0.6, 0.5]);
    let g = Gear { tau0: 8.0, rotor_state: true, ..GEAR };
    let text = gear(&table_text(&ws, &rs, "rad/s", "monotoneCubic", "clamp"), &g);
    let data = data_1d(&ws, &rs, Interpolation::MonotoneCubic, Outside::Clamp);
    let auto = choosing(&text, "Test.KinkGear", TableCells::Auto);
    let off = choosing(&text, "Test.KinkGear", TableCells::Off);
    assert!(!auto.prepared.algebraics.is_empty(), "the implicit form");
    assert!(!auto.prepared.held.is_empty() && auto.prepared.held.iter().all(|h| !h.explicit));
    let times = grid(6.0, 0.01);
    let exact = gear_exact(&data, &g, (-20.0, 80.0), &times);
    let mut qs = vec![];
    for rtol in [1e-4, 1e-7] {
        let (r, at) = run_choosing(&auto, &options(rtol), 6.0).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(at.len(), 1, "rtol {rtol:e}: {:?}", r.events);
        assert!(r.backend.contains("CVODE"), "{}", r.backend);
        assert!(r.report.notes.iter().any(|n| n.contains("stepped over from here")));
        let (o, _) = run_choosing(&off, &options(rtol), 6.0).unwrap_or_else(|e| panic!("{e}"));
        for ch in ["w1", "w2"] {
            assert_eq!(r.channel(ch), o.channel(ch), "rtol {rtol:e}: 1d41c7d7's run, bitwise");
        }
        qs.push(error(&r, "w2", &exact) / rtol);
    }
    println!("held at its edges, onto the explicit form: error / rtol {qs:.2?}");
    // measured 1.63 and 3.56 (the judge's measure, both speeds, the same)
    assert!(qs.iter().all(|q| *q < 15.0), "{qs:?}");

    // the 2-D gear, against its run with the load's speed the state (no
    // switch), held at every crossing, at rtol 1e-12
    let fine = prepare(&switch_gear_2d(false), "Test.SwitchGear2D", TableCells::Kinks);
    let fine = run_with(&fine, &SolverOptions { held_release: false, ..options(1e-12) }, 6.0, 0.01);
    let (f1, f2) = (fine.channel("w1").unwrap().to_vec(), fine.channel("w2").unwrap().to_vec());
    let auto = choosing(&switch_gear_2d(true), "Test.SwitchGear2D", TableCells::Auto);
    assert!(!auto.prepared.algebraics.is_empty(), "the implicit form");
    assert!(!auto.prepared.held.is_empty() && auto.prepared.held.iter().all(|h| !h.explicit));
    let mut qs = vec![];
    for rtol in [1e-4, 1e-7] {
        let (r, at) = run_choosing(&auto, &options(rtol), 6.0).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(at.len(), 1, "rtol {rtol:e}: {:?}", r.events);
        assert!(r.backend.contains("CVODE"), "{}", r.backend);
        let before =
            r.events.iter().filter(|e| matches!(e.kind, EventKind::Cell(_)) && e.t < at[0]);
        assert!(before.count() >= 3, "held on the implicit form: c at 1 s, w2 at 10, c at 2.5 s");
        qs.push(error(&r, "w1", &f1).max(error(&r, "w2", &f2)) / rtol);
    }
    println!("2-D read, onto the explicit form: error / rtol {qs:.2?}");
    // measured 3.10 and 4.71 (the judge's, against Radau per cell, the same)
    assert!(qs.iter().all(|q| *q < 20.0), "{qs:?}");

    // the bead, against the same wire with x the state (no switch), held at
    // every crossing, at rtol 1e-12
    let fine = prepare(&switch_bead(false), "Test.Bead", TableCells::Kinks);
    let fine = run_with(&fine, &SolverOptions { held_release: false, ..options(1e-12) }, 3.0, 0.01);
    let (fx, fv) = (fine.channel("x").unwrap().to_vec(), fine.channel("vx").unwrap().to_vec());
    let auto = choosing(&switch_bead(true), "Test.Bead", TableCells::Auto);
    assert!(!auto.prepared.algebraics.is_empty(), "the implicit form");
    let mut qs = vec![];
    for rtol in [1e-4, 1e-7] {
        let (r, at) = run_choosing(&auto, &options(rtol), 3.0).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(at.len(), 1, "rtol {rtol:e}: {:?}", r.events);
        assert!(r.backend.contains("CVODE"), "{}", r.backend);
        // (x where the switch is: the wire's lowest point, r' = 0 at x = -0.35)
        let k = (at[0] / 0.01) as usize;
        assert!((fx[k] + 0.35).abs() < 0.1, "at t = {}: x = {}", at[0], fx[k]);
        qs.push(error(&r, "x", &fx).max(error(&r, "vx", &fv)) / rtol);
    }
    println!("the bead, onto the explicit form: error / rtol {qs:.2?}");
    // measured 64.5 and 132 (the judge's, against DOP853 per cell, the same)
    assert!(qs.iter().all(|q| *q < 400.0), "{qs:?}");
}

/// The other way: a gear on the explicit form (the load's speed the state),
/// its ratio a monotone cubic held at its edges, dense (41 points on [0,
/// 80] rad/s), which the explicit form steps over; where the choice's
/// exchange factor passes 3 (t = 2.33 s), dynamic state selection takes the
/// rotor's speed, of the implicit form, which holds the read: held in its
/// cells again there, its crossing out of the data at 80 rad/s an event.
/// Before, the switch was refused (the other choice held other reads) and
/// the run went on with its states (2.58, 66.7 and 54 rtol at 1e-4, 1e-7,
/// 1e-10 in the judge's measure); now 3.2, 9.1, 22, as 1d41c7d7's run,
/// which switched and stepped over the edge on IDAS.
#[test]
fn a_change_of_states_onto_the_implicit_form_holds_what_the_explicit_stepped_over() {
    let ws: Vec<f64> = (0..41).map(|k| 80.0 * k as f64 / 40.0).collect();
    let rs: Vec<f64> = ws.iter().map(|w| 2.0 + w / 40.0 + 0.25 * math::sin(w / 8.0)).collect();
    let text = gear(&table_text(&ws, &rs, "rad/s", "monotoneCubic", "clamp"), &GEAR);
    let data = data_1d(&ws, &rs, Interpolation::MonotoneCubic, Outside::Clamp);
    let auto = choosing(&text, "Test.KinkGear", TableCells::Auto);
    assert!(auto.prepared.algebraics.is_empty(), "the explicit form");
    assert!(!auto.prepared.held.is_empty() && auto.prepared.held.iter().all(|h| !h.explicit));
    let times = grid(4.0, 0.01);
    let exact = gear_exact(&data, &GEAR, (-30.0, 300.0), &times);
    assert!(*exact.last().unwrap() > 80.0, "it leaves the data");
    let mut qs = vec![];
    for rtol in [1e-4, 1e-7] {
        let (r, at) = run_choosing(&auto, &options(rtol), 4.0).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(at.len(), 1, "rtol {rtol:e}: {:?}", r.events);
        assert!(r.backend.contains("IDA"), "{}", r.backend);
        assert!(r.report.notes.iter().any(|n| n.contains("held in its cells again")));
        let after = r.events.iter().filter(|e| matches!(e.kind, EventKind::Cell(_)) && e.t > at[0]);
        assert_eq!(after.count(), 2, "out at 80 rad/s, both reads");
        qs.push(error(&r, "w2", &exact) / rtol);
    }
    println!("held again on the implicit form: error / rtol {qs:.2?}");
    // measured 3.24 and 9.14 (the judge's measure, both speeds, the same)
    assert!(qs.iter().all(|q| *q < 30.0), "{qs:?}");
}

/// The spring `x'' = F(x)` with `F` a linear table of `x` continued
/// linearly outside, every slope negative, in closed form: per cell a
/// harmonic motion about its centre, the crossing of the cell's next edge
/// from its phase. Returns `x` and `v` on `times`, and the crossings.
fn spring_exact(
    xs: &[f64],
    fs: &[f64],
    (x0, v0): (f64, f64),
    times: &[f64],
) -> (Vec<f64>, Vec<f64>, usize) {
    use std::f64::consts::TAU;
    let n = xs.len();
    // cell c: between xs[c] and xs[c + 1], the edge cells reaching out
    let mut c = xs.partition_point(|&p| p <= x0).clamp(1, n - 1) - 1;
    let (mut t0, mut x, mut v) = (0.0, x0, v0);
    let (mut out_x, mut out_v, mut crossings) = (vec![], vec![], 0);
    let mut k = 0;
    while k < times.len() {
        let s = (fs[c + 1] - fs[c]) / (xs[c + 1] - xs[c]);
        assert!(s < 0.0, "a restoring force");
        let w = (-s).sqrt();
        let centre = xs[c] - fs[c] / s;
        let (a, b) = (x - centre, v / w);
        let r = (a * a + b * b).sqrt();
        // x − centre = r cos θ, v = −r w sin θ
        let th0 = math::atan2(-b, a);
        let mut exit: Option<(f64, usize, f64)> = None;
        let edges =
            [(c > 0).then(|| (xs[c], c.wrapping_sub(1))), (c + 2 < n).then(|| (xs[c + 1], c + 1))];
        for (e, next) in edges.into_iter().flatten() {
            let q = (e - centre) / r;
            if q.abs() >= 1.0 {
                continue;
            }
            for th in [math::acos(q), -math::acos(q)] {
                let mut d = (th - th0).rem_euclid(TAU);
                // (the edge just crossed: its next visit)
                if d < 1e-9 {
                    d += TAU;
                }
                if exit.is_none_or(|(dd, _, _)| d < dd) {
                    exit = Some((d, next, e));
                }
            }
        }
        let t_exit = exit.map_or(f64::INFINITY, |(d, _, _)| t0 + d / w);
        while k < times.len() && times[k] < t_exit {
            let th = th0 + w * (times[k] - t0);
            out_x.push(centre + r * math::cos(th));
            out_v.push(-r * w * math::sin(th));
            k += 1;
        }
        let Some((d, next, e)) = exit else { break };
        let th = th0 + d;
        (t0, x, v, c) = (t_exit, e, -r * w * math::sin(th), next);
        if t_exit <= times[times.len() - 1] {
            crossings += 1;
        }
    }
    (out_x, out_v, crossings)
}

/// The spring of the review's density sweep: `F = −x − 2x³` sampled at
/// `n` points on [−1, 1] as a linear table, from x = 0.9023 at rest.
fn dense_spring(n: usize) -> (Vec<f64>, Vec<f64>, String) {
    let xs: Vec<f64> = (0..n).map(|k| -1.0 + 2.0 * k as f64 / (n - 1) as f64).collect();
    let fs: Vec<f64> = xs.iter().map(|x| -x - 2.0 * x * x * x).collect();
    let text = format!(
        r#"model Test.Spring "a unit mass on a spring whose force is a table of its position"
  parameter Real k(unit = "1/s2") = 1;
  parameter Real F = {};
  Real x(start = 0.9023, fixed = true);
  Real v(unit = "1/s", start = 0, fixed = true);
equation
  der(x) = v;
  der(v) = k * F(x);
end Test.Spring;
"#,
        table_text(&xs, &fs, "1", "linear", "linear")
    );
    (xs, fs, text)
}

/// A table of many points read at a state: a linear table's value only
/// kinks at its points, and is stepped over. Held, each of its 855
/// crossings in 20 s would restart the integrator, about 3.6 rtol each:
/// 2 900 … 3 100 rtol off at rtol 1e-4 … 1e-6 where stepped over it is
/// 39 … 72 (the review's `d101`).
#[test]
fn a_dense_table_read_at_a_state_is_stepped_over() {
    let (xs, fs, text) = dense_spring(101);
    let times = grid(20.0, 0.02);
    let (ex, ev, crossings) = spring_exact(&xs, &fs, (0.9023, 0.0), &times);
    assert_eq!(crossings, 855);
    let q = |r: &SimResult, rtol: f64| error(r, "x", &ex).max(error(r, "v", &ev)) / rtol;
    let m = prepare(&text, "Test.Spring", TableCells::Jumps);
    assert!(m.held.is_empty(), "a value read only kinks: {:?}", m.held);
    let kinks = prepare(&text, "Test.Spring", TableCells::Kinks);
    assert_eq!(kinks.held.len(), 1);
    let (mut over, mut held) = (vec![], vec![]);
    for rtol in [1e-4, 1e-6, 1e-8] {
        let r = run(&m, rtol, 20.0, 0.02);
        assert_eq!(cells(&r), 0);
        over.push(q(&r, rtol));
        let h = run(&kinks, rtol, 20.0, 0.02);
        assert!(cells(&h) > 800, "{}", cells(&h));
        held.push(q(&h, rtol));
    }
    println!("error / rtol, stepped over {over:.1?}, held {held:.1?}");
    // measured 39, 154, 802 stepped over (with a margin); held 2 939, 3 107,
    // 2 216
    for (q, bound) in over.iter().zip([120.0, 470.0, 2500.0]) {
        assert!(*q < bound, "{over:?}");
    }
    assert!(over[..2].iter().zip(&held).all(|(o, h)| 10.0 * o < *h), "{over:?} {held:?}");
}

/// `v' = T(v)` from 0, `T` linear through 3 − 0.002 v² at 301 points on
/// [0, 40] (the judge's `kink-pass-301`): `v` rises through 290 of them once
/// and tends to 38.7. Its value only kinks there: stepped over. Exact per
/// cell, `v = v∞ + (v₀ − v∞) e^{b t}` where `T = a + b v`. Held (for
/// comparison), each crossing a restart: 10 … 26 rtol off at rtol 1e-4 …
/// 1e-8 with 600 … 1 900 steps, where stepped over it is 0.6 … 10 with 37
/// … 450.
#[test]
fn a_single_pass_through_a_dense_table_is_stepped_over() {
    let ps: Vec<f64> = (0..=300).map(|k| 40.0 * k as f64 / 300.0).collect();
    let ts: Vec<f64> = ps.iter().map(|p| 3.0 - 0.002 * p * p).collect();
    let text = format!(
        r#"model Test.Pass "a quantity whose rate is a linear table of itself"
  parameter Real k(unit = "1/s") = 1;
  parameter Real T = {};
  Real v(start = 0, fixed = true);
equation
  der(v) = k * T(v);
end Test.Pass;
"#,
        table_text(&ps, &ts, "1", "linear", "linear")
    );
    // exact: per cell an exponential, the next point reached in closed form
    let times = grid(60.0, 0.05);
    let (mut t0, mut v0, mut c, mut crossings) = (0.0f64, 0.0f64, 0usize, 0usize);
    let mut exact = vec![];
    for &t in &times {
        loop {
            let b = (ts[c + 1] - ts[c]) / (ps[c + 1] - ps[c]);
            let v_inf = ps[c] - ts[c] / b;
            let q = (ps[c + 1] - v_inf) / (v0 - v_inf);
            let reach = if q > 0.0 { math::ln(q) / b } else { f64::INFINITY };
            if t - t0 < reach || c + 2 == ps.len() {
                exact.push(v_inf + (v0 - v_inf) * math::exp(b * (t - t0)));
                break;
            }
            (t0, v0, c) = (t0 + reach, ps[c + 1], c + 1);
            crossings += 1;
        }
    }
    assert_eq!(crossings, 290);
    let m = prepare(&text, "Test.Pass", TableCells::Jumps);
    assert!(m.held.is_empty(), "{:?}", m.held);
    let kinks = prepare(&text, "Test.Pass", TableCells::Kinks);
    let (mut over, mut held) = (vec![], vec![]);
    for rtol in [1e-4, 1e-6, 1e-8] {
        let r = run(&m, rtol, 60.0, 0.05);
        assert_eq!(cells(&r), 0);
        over.push(error(&r, "v", &exact) / rtol);
        let h = run(&kinks, rtol, 60.0, 0.05);
        assert_eq!(cells(&h), 290);
        held.push(error(&h, "v", &exact) / rtol);
    }
    println!("error / rtol, stepped over {over:.2?}, held {held:.2?}");
    // measured 0.58, 2.6, 10.4 stepped over, with a margin; held 10.1, 26.4,
    // 23.2
    for (q, bound) in over.iter().zip([5.0, 15.0, 40.0]) {
        assert!(*q < bound, "{over:?}");
    }
}

/// `v' = T(v)`, `T` a table of `v` as the text format writes one by default
/// (a monotone cubic, held at its edges): `v` rises through every point
/// once and leaves the data at its last. Its value is continuously
/// differentiable between its points and kinks at its edges: stepped over,
/// no event, where every point of a default table was an event before.
/// Against `t(v) = ∫ dv / T(v)` (Gauss–Legendre per cell), inverted.
#[test]
fn the_default_table_read_at_a_state_is_not_held() {
    let ps: Vec<f64> = (0..=40).map(|k| k as f64 * 0.5).collect();
    let ts: Vec<f64> = ps.iter().map(|p| 2.0 + math::sin(p / 3.0)).collect();
    let list = |v: &[f64]| v.iter().map(|x| format!("{x:?}")).collect::<Vec<_>>().join(", ");
    let text = format!(
        r#"model Test.Pass "a speed whose rate is a table of itself"
  parameter Real T(unit = "1/s") = table(x = {{{}}}, y = {{{}}});
  Real v(start = 0, fixed = true);
equation
  der(v) = T(v);
end Test.Pass;
"#,
        list(&ps),
        list(&ts)
    );
    let data = data_1d(&ps, &ts, Interpolation::MonotoneCubic, Outside::Clamp);
    let table = Table::new(&data).unwrap();
    let rate = |v: f64| table.value([v, 0.0]);
    let inv = |v: f64| 1.0 / rate(v);
    let last = *ps.last().unwrap();
    let t_last = integral(&inv, &ps, 0.0, last);
    let times = grid(14.0, 0.01);
    let exact: Vec<f64> = times
        .iter()
        .map(|&t| {
            if t >= t_last {
                // held at its last value beyond the data
                return last + rate(last) * (t - t_last);
            }
            let (mut a, mut b) = (0.0, last);
            for _ in 0..200 {
                let mid = 0.5 * (a + b);
                if integral(&inv, &ps, 0.0, mid) < t { a = mid } else { b = mid }
            }
            0.5 * (a + b)
        })
        .collect();
    assert!(t_last < 12.0 && *exact.last().unwrap() > last + 4.0, "it leaves the data");
    let m = prepare(&text, "Test.Pass", TableCells::Jumps);
    assert!(m.held.is_empty(), "{:?}", m.held);
    let kinks = prepare(&text, "Test.Pass", TableCells::Kinks);
    assert!(matches!(kinks.held[..], [ref h] if matches!(h.cells, HeldCells::Edges(_))));
    let (mut over, mut held) = (vec![], vec![]);
    for rtol in RTOLS {
        let r = run(&m, rtol, 14.0, 0.01);
        assert_eq!(cells(&r), 0);
        over.push(error(&r, "v", &exact) / rtol);
        let h = run(&kinks, rtol, 14.0, 0.01);
        assert_eq!(cells(&h), 1, "out at its last point only");
        held.push(error(&h, "v", &exact) / rtol);
    }
    println!("error / rtol, stepped over {over:.2?}, held at its edges {held:.2?}");
    // measured 2.0 … 37 either way (its points, where the rate's second
    // derivative jumps, stepped over), with a margin
    assert!(over.iter().chain(&held).all(|q| *q < 110.0), "{over:?} {held:?}");
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
/// between its points and continued linearly (the judge's `kink-2d`): its
/// value kinks across its grid lines and is stepped over, 0.68 … 4.7 times
/// the error of the same function written as `if` branches on its
/// breakpoints (each crossing an event) at rtol 1e-4 … 1e-10 (in the
/// judge's measure 1.1 … 5.0); held (for comparison only), the same run as
/// that.
#[test]
fn a_table_of_two_states_is_stepped_over() {
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
    let table = prepare(&model("Table", "T(x, y)"), "Test.Table", TableCells::Jumps);
    assert!(table.held.is_empty());
    let kinks = prepare(&model("Table", "T(x, y)"), "Test.Table", TableCells::Kinks);
    assert_eq!(kinks.held.len(), 1);
    let located = format!("({} + {})", if_chain(&xs, &fx, "x"), if_chain(&ys, &gy, "y"));
    let located = prepare(&model("Located", &located), "Test.Located", TableCells::Jumps);
    assert!(located.held.is_empty() && !located.modes.is_empty());
    let worst = |r: &SimResult, s: &SimResult| {
        let (x, z) = (r.channel("x").unwrap(), s.channel("x").unwrap());
        x.iter().zip(z).fold(0.0f64, |m, (p, q)| m.max((p - q).abs()))
    };
    // the reference: the located model at a far tighter tolerance
    let fine = run(&located, 1e-12, 30.0, 0.02);
    let (mut over, mut held) = (vec![], vec![]);
    for rtol in RTOLS {
        let b = run(&located, rtol, 30.0, 0.02);
        let eb = worst(&b, &fine);
        over.push(worst(&run(&table, rtol, 30.0, 0.02), &fine) / eb);
        let h = run(&kinks, rtol, 30.0, 0.02);
        assert!(cells(&h) > 50, "it crosses its cells' edges: {}", cells(&h));
        held.push(worst(&h, &fine) / eb);
    }
    println!("error / the located run's: stepped over {over:.2?}, held {held:.3?}");
    assert!(held.iter().all(|q| (0.8..1.25).contains(q)), "{held:?}");
    assert!(over.iter().all(|q| *q < 15.0), "{over:?}");
}

/// What is held: a read at a state that jumps there, and the reads of its
/// table at the same arguments. Not a read that only kinks (a linear
/// table's value, a monotone cubic's held at its edges), nor a read along
/// time (the run stops at its breakpoints), nor at a parameter; held for
/// comparison, a kinking read is its table's within rounding.
#[test]
fn reads_are_held_where_they_jump_at_a_state() {
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
    let held = |table: String, arg: &str, cells: TableCells| {
        prepare(&text(&table, arg), "Test.Spring", cells).held.len()
    };
    let j = TableCells::Jumps;
    for (interp, outside) in [("linear", "linear"), ("linear", "clamp"), ("monotoneCubic", "clamp")]
    {
        let t = table_text(&xs, &ys, "1", interp, outside);
        assert_eq!(held(t.clone(), "x", j), 0, "{interp}, {outside}: a value only kinks");
        assert_eq!(held(t, "x", TableCells::Kinks), 1, "{interp}, {outside}: held for comparison");
    }
    let cubic = table_text(&xs, &ys, "1", "monotoneCubic", "linear");
    assert_eq!(held(cubic, "x", TableCells::Kinks), 0, "C¹ everywhere");
    let lin = table_text(&xs, &ys, "s", "linear", "linear");
    assert_eq!(held(lin, "time", TableCells::Kinks), 0, "along time");
    let lin = table_text(&xs, &ys, "1", "linear", "linear");
    assert_eq!(held(lin, "p", TableCells::Kinks), 0, "a parameter");
    // held, the run is the table's: within rounding of it at every point
    let data = |interp, outside| data_1d(&xs, &ys, interp, outside);
    for (t, d) in [
        (table_text(&xs, &ys, "1", "linear", "clamp"), data(Interpolation::Linear, Outside::Clamp)),
        (
            table_text(&xs, &ys, "1", "monotoneCubic", "clamp"),
            data(Interpolation::MonotoneCubic, Outside::Clamp),
        ),
    ] {
        let m = prepare(&text(&t, "x"), "Test.Spring", TableCells::Kinks);
        let r = run(&m, 1e-8, 20.0, 0.01);
        let table = Table::new(&d).unwrap();
        let (x, f) = (r.channel("x").unwrap(), r.channel("f").unwrap());
        let worst =
            x.iter().zip(f).fold(0.0f64, |m, (a, b)| m.max((table.value([*a, 0.0]) - b).abs()));
        assert!(worst < 1e-14, "{t}: {worst:e}");
        // (the linear table's points crossed; the cubic, held at its edges
        // only, never left)
        let crossed = if t.contains("linear") { cells(&r) > 10 } else { cells(&r) == 0 };
        assert!(crossed, "{t}: {}", cells(&r));
    }
}

/// A held read's cells are its table's when the model was built: given
/// data after that interpolate with a higher degree (a linear table's data
/// swapped for a monotone cubic's), or that jump between the points of a
/// table held at its edges only (a cubic's swapped for linear ones), the
/// run stops and says to build the model again, instead of reading the
/// table wrong.
#[test]
fn a_held_table_given_other_data_after_the_build_is_refused() {
    let (ws, rs) = ([0.0, 10.0, 25.0, 40.0, 60.0, 80.0], [2.0, 2.5, 2.8, 3.5, 3.6, 4.0]);
    let g = Gear { t_rev: Some(2.5), ..GEAR };
    for (interp, outside, swapped) in [
        ("linear", "linear", data_1d(&ws, &rs, Interpolation::MonotoneCubic, Outside::Linear)),
        ("monotoneCubic", "clamp", data_1d(&ws, &rs, Interpolation::Linear, Outside::Clamp)),
    ] {
        let text = gear(&table_text(&ws, &rs, "rad/s", interp, outside), &g);
        let m = prepare(&text, "Test.KinkGear", TableCells::Jumps);
        assert_eq!(m.held.len(), 2, "{interp}");
        let code = lsim_codegen::compile(&m, &Default::default()).expect("compiles");
        let swapped = code.with_tables(&[swapped]).expect("the same shape");
        let info = RunInfo::from_prepared(&m);
        let e = lsim_solve::simulate(
            &swapped,
            &info,
            &options(1e-6),
            OutputGrid { t0: 0.0, t_end: 3.0, dt: 0.01 },
            &mut [],
        )
        .expect_err("refused");
        assert!(e.to_string().contains("build the model again"), "{interp}: {e}");
    }
}

/// Held reads on the engine's cross-check backend (diffsol's BDF). The
/// gear's ratio: an event of each read at each of the four breakpoints, as
/// on CVODE. A dense table held for comparison: at rtol 1e-10 diffsol's
/// root finder lost a crossing and the run panicked
/// (`InterpolationTimeAfterCurrentTime`): diffsol tells a sign change by the
/// product of two values, and a condition held on its side
/// (`f64::MIN_POSITIVE`) times an edge function at the rounding level of
/// its crossing (1e-16) rounds to −0. The backend floors the values it
/// gives diffsol, and a panic inside a step is the run's named failure.
/// That run is not proportional on diffsol, held or with its breakpoints
/// located alike (the A3 review: 105, 5 171 and 5 150 rtol at rtol 1e-4,
/// 1e-7 and 1e-10, where CVODE's held run stays at 2 200 … 3 100): an open
/// item of the cross-check backend (STATUS.md), pinned here as measured.
#[test]
fn held_reads_on_the_cross_check_backend() {
    let diffsol =
        |rtol: f64| SolverOptions { backend: lsim_solve::Backend::Diffsol, ..options(rtol) };
    let (ws, rs) = ([0.0, 10.0, 25.0, 40.0, 60.0, 80.0], [2.0, 2.5, 2.8, 3.5, 3.6, 4.0]);
    let text = gear(&table_text(&ws, &rs, "rad/s", "linear", "linear"), &GEAR);
    let data = data_1d(&ws, &rs, Interpolation::Linear, Outside::Linear);
    let m = prepare(&text, "Test.KinkGear", TableCells::Jumps);
    let times = grid(3.0, 0.01);
    let exact = gear_exact(&data, &GEAR, (-30.0, 300.0), &times);
    let mut qs = vec![];
    for rtol in [1e-4, 1e-7, 1e-10] {
        let r = run_with(&m, &diffsol(rtol), 3.0, 0.01);
        assert!(r.backend.contains("diffsol"), "{}", r.backend);
        assert_eq!(cells(&r), 8, "rtol {rtol:e}");
        qs.push(error(&r, "w2", &exact) / rtol);
    }
    println!("diffsol, the gear held: error / rtol {qs:.2?}");
    assert!(qs.iter().all(|q| *q < 10.0), "{qs:?}");
    let (xs, fs, text) = dense_spring(101);
    let times = grid(20.0, 0.02);
    let (ex, ev, crossings) = spring_exact(&xs, &fs, (0.9023, 0.0), &times);
    let kinks = prepare(&text, "Test.Spring", TableCells::Kinks);
    let mut qs = vec![];
    for rtol in [1e-4, 1e-10] {
        let r = run_with(&kinks, &diffsol(rtol), 20.0, 0.02);
        assert_eq!(cells(&r), crossings, "rtol {rtol:e}: every crossing an event, to the end");
        qs.push(error(&r, "x", &ex).max(error(&r, "v", &ev)) / rtol);
    }
    println!("diffsol, a dense table held: error / rtol {qs:.1?}");
    // measured 105 and 5 150, with a margin
    assert!(qs[0] < 320.0 && qs[1] < 1.6e4, "{qs:?}");
}

/// The bead of the A3 review: a bead on a wire `y = r(x)` under gravity, `r`
/// the default table (Steffen's monotone cubic, continued linearly) of
/// `x²/2` at 41 points, `s(x) = x` a second table for the force's
/// direction; from `x = 1.5` at rest for 10 s. Index reduction reads `r'`
/// and `r''`. The explicit form holds a cubic's second derivative by
/// default (rule v4, from the wavy bead below), each of the 204 crossings a
/// restart; this wire's `r''` does not really jump (Steffen's cubic of a
/// parabola is the parabola, its `r''` continuous to rounding at its
/// points; it jumps to 0 at the data's edges, which the bead does not reach,
/// so the release at the start keeps it), and here holding is the price:
/// 131 (released at its crossings' pace, at rtol 1e-4), 502 and 528 rtol
/// off at 1e-4, 1e-6 and 1e-8 where stepped over (`TableCells::Off`, the
/// default before) it is 88, 12.4 and 187 (the judge's `kink-bead-41`, all
/// seven: 131 … 737 against 10.5 … 187). Against the run at rtol 1e-12 and
/// the exact energy `½ m (vx² + vy²) + m g y`.
#[test]
fn a_cubics_second_derivative_read_at_a_state_is_held() {
    let xs: Vec<f64> = (0..41).map(|k| -2.0 + 4.0 * k as f64 / 40.0).collect();
    let rs: Vec<f64> = xs.iter().map(|x| 0.5 * x * x).collect();
    let text = bead(&xs, &rs, &xs);
    let def = prepare(&text, "Test.Bead", TableCells::default());
    assert!(def.algebraics.is_empty(), "the explicit form");
    assert!(!def.held.is_empty() && def.held.iter().all(|h| h.explicit), "{:?}", def.held);
    let off = prepare(&text, "Test.Bead", TableCells::Off);
    assert!(off.held.is_empty());
    let fine = run(&off, 1e-12, 10.0, 0.01);
    let (fx, fv) = (fine.channel("x").unwrap().to_vec(), fine.channel("vx").unwrap().to_vec());
    let q = |r: &SimResult, rtol: f64| error(r, "x", &fx).max(error(r, "vx", &fv)) / rtol;
    let energy = |r: &SimResult| {
        let (vx, vy, y) =
            (r.channel("vx").unwrap(), r.channel("vy").unwrap(), r.channel("y").unwrap());
        let e: Vec<f64> =
            (0..y.len()).map(|k| 0.5 * (vx[k] * vx[k] + vy[k] * vy[k]) + 9.81 * y[k]).collect();
        e.iter().fold(0.0f64, |a, v| a.max((v - e[0]).abs())) / e[0].abs()
    };
    let (mut over, mut held, mut drift) = (vec![], vec![], vec![]);
    for rtol in [1e-4, 1e-6, 1e-8] {
        let a = run(&off, rtol, 10.0, 0.01);
        assert_eq!(cells(&a), 0);
        over.push(q(&a, rtol));
        let h = run(&def, rtol, 10.0, 0.01);
        if rtol > 1e-5 {
            // (its crossings too frequent for the integrator there: let go of)
            assert!(h.report.notes.iter().any(|n| n.contains("no longer held")));
        } else {
            assert!(cells(&h) > 600, "{}", cells(&h));
        }
        held.push(q(&h, rtol));
        drift.push((energy(&a) / rtol, energy(&h) / rtol));
    }
    println!(
        "bead: error / rtol stepped over {over:.1?}, held {held:.1?}; energy drift / rtol {drift:.1?}"
    );
    // measured 88, 12.4, 187 stepped over (the review's against DOP853, to
    // the digit); held 131, 502, 528, the energy too (drift 34, 18, 73
    // stepped over), with a margin
    for (q, bound) in over.iter().zip([300.0, 40.0, 600.0]) {
        assert!(*q < bound, "{over:?}");
    }
    for (q, bound) in held.iter().zip([400.0, 1500.0, 1600.0]) {
        assert!(*q < bound, "{held:?}");
    }
}

/// The bead's model: a wire `y = r(x)`, `r` the default table of `rs` at
/// `xs` (Steffen's cubic, continued linearly), `s` a table of `ss` there.
fn bead(xs: &[f64], rs: &[f64], ss: &[f64]) -> String {
    let list = |v: &[f64]| v.iter().map(|x| format!("{x:?}")).collect::<Vec<_>>().join(", ");
    let tab = |v: &[f64]| {
        format!(
            "table(x = {{{}}}, y = {{{}}}, xUnit = \"m\", outside = {{linear}})",
            list(xs),
            list(v)
        )
    };
    format!(
        r#"model Test.Bead "a bead on a wire y = r(x) under gravity, r the default table"
  parameter Real m(unit = "kg") = 1;
  parameter Real g(unit = "m/s2") = 9.81;
  parameter Real r(unit = "m") = {} "the wire's height";
  parameter Real s = {} "its slope";
  Real x(unit = "m", start = 1.5, fixed = true);
  Real vx(unit = "m/s", start = 0, fixed = true);
  Real y(unit = "m");
  Real vy(unit = "m/s");
  Real F(unit = "N") "the wire's force along y";
equation
  der(x) = vx;
  der(y) = vy;
  m * der(vx) = -s(x) * F;
  m * der(vy) = F - m * g;
  y = r(x) "on the wire";
end Test.Bead;
"#,
        tab(rs),
        tab(ss)
    )
}

/// A bead whose wire's cubic has a second derivative that really jumps
/// (`x²/2 + sin(3x)/4` at 11 points, the judge's `kink-bead-sin-11`; rule
/// v3's held-out bead) on its own form, the explicit one (CVODES). Stepped
/// over (`TableCells::Off`, the default before), the run is 320, 198, 548,
/// 3 800, 12 000, 5 980 and 81 400 rtol off at rtol 1e-4 … 1e-10 (the error
/// per tolerance growing as it tightens); held (the default, rule v4's
/// clause (S): 172 times as far off at 1e-7, where held is within 100), 105,
/// 35, 105, 22, 190, 362 and 114 (the judge's measure, against DOP853 per
/// cell; 50 crossings in 10 s, an event of each of the three reads).
/// Stepped over, the error moves with the data's last bits: with the points
/// written `-2 + 0.4 k`, 248 and 1 051 at 1e-4 and 1e-7 (held 105 and 22).
#[test]
fn a_cubics_second_derivative_that_jumps_is_held_on_the_explicit_form() {
    // (the judge's points, to the bit)
    let xs: Vec<f64> = (0..11).map(|k| -2.0 + 4.0 * k as f64 / 10.0).collect();
    let rs: Vec<f64> = xs.iter().map(|x| 0.5 * x * x + 0.25 * math::sin(3.0 * x)).collect();
    let ss: Vec<f64> = xs.iter().map(|x| x + 0.75 * math::cos(3.0 * x)).collect();
    let text = bead(&xs, &rs, &ss);
    let def = prepare(&text, "Test.Bead", TableCells::default());
    assert!(def.algebraics.is_empty(), "the explicit form");
    assert!(!def.held.is_empty() && def.held.iter().all(|h| h.explicit), "{:?}", def.held);
    let off = prepare(&text, "Test.Bead", TableCells::Off);
    let fine = run_with(&def, &SolverOptions { held_release: false, ..options(1e-12) }, 10.0, 0.01);
    let (fx, fv) = (fine.channel("x").unwrap().to_vec(), fine.channel("vx").unwrap().to_vec());
    let q = |r: &SimResult, rtol: f64| error(r, "x", &fx).max(error(r, "vx", &fv)) / rtol;
    let (mut held, mut over) = (vec![], vec![]);
    for rtol in [1e-4, 1e-7] {
        let h = run(&def, rtol, 10.0, 0.01);
        assert!(h.backend.contains("CVODE"), "{}", h.backend);
        assert!(cells(&h) > 140, "rtol {rtol:e}: {}", cells(&h));
        held.push(q(&h, rtol));
        over.push(q(&run(&off, rtol, 10.0, 0.01), rtol));
    }
    println!("wavy bead, explicit form: error / rtol held {held:.1?}, stepped over {over:.1?}");
    // measured held 105 and 22.1, stepped over 320 and 3 800, with a margin
    assert!(held[0] < 300.0 && held[1] < 60.0, "{held:?}");
    assert!(over[1] > 100.0 * held[1], "{over:?} {held:?}");
}

/// A gear whose ratio is a linear table of collinear points (`r = 2 + w /
/// 50`): its slope does not jump at them. Held (`TableCells::Jumps`), the
/// run releases it at the start (its jumps are within the tolerance) and it
/// is the stepped-over run to the bit, where every point was a restart
/// before.
#[test]
fn a_table_whose_slope_does_not_jump_is_released_at_the_start() {
    let ws = [0.0, 10.0, 25.0, 40.0, 60.0, 80.0];
    let rs: Vec<f64> = ws.iter().map(|w| 2.0 + w / 50.0).collect();
    let text = gear(&table_text(&ws, &rs, "rad/s", "linear", "linear"), &GEAR);
    let m = prepare(&text, "Test.KinkGear", TableCells::Jumps);
    assert_eq!(m.held.len(), 2, "{:?}", m.held);
    let off = prepare(&text, "Test.KinkGear", TableCells::Off);
    for rtol in [1e-4, 1e-8] {
        let (a, b) = (run(&m, rtol, 3.0, 0.01), run(&off, rtol, 3.0, 0.01));
        assert_eq!(cells(&a), 0, "rtol {rtol:e}");
        // (the held model has its cells' discrete variables besides)
        for name in &b.names {
            assert_eq!(a.channel(name), b.channel(name), "rtol {rtol:e}: {name}");
        }
        assert!(
            a.report.notes.iter().any(|n| n.contains("continuous at its points")),
            "{:?}",
            a.report.notes
        );
        // not released: every point a restart
        let held = SolverOptions { held_release: false, ..options(rtol) };
        assert_eq!(cells(&run_with(&m, &held, 3.0, 0.01)), 8);
    }
}

/// The gear on a torsion spring, no torque driving it (the judge's
/// `kink-osc-…`), its ratio `table` of the load's speed, from 28 rad/s;
/// `implicit`: its torques written implicitly, as [`Gear::implicit`].
fn gear_spring(table: &str, implicit: bool) -> String {
    let (torques, tu) = if implicit {
        (
            "t2 + 1e-6 * t2^3 / tu^2 = r(w2) * t1 + 1e-6 * (r(w2) * t1)^3 / tu^2",
            "\n  parameter Real tu(unit = \"N.m\") = 1;",
        )
    } else {
        ("t2 = r(w2) * t1", "")
    };
    format!(
        r#"model Test.GearSpring "a gear whose ratio is a table of the load's speed, the load on a torsion spring"
  parameter Real J1(unit = "kg.m2") = 0.05;
  parameter Real J2(unit = "kg.m2") = 2;
  parameter Real k(unit = "N.m/rad") = 50;{tu}
  parameter Real r = {table};
  Real w1(unit = "rad/s", start = 0);
  Real w2(unit = "rad/s", start = 28, fixed = true);
  Real phi(unit = "rad", start = 0, fixed = true);
  Real t1(unit = "N.m");
  Real t2(unit = "N.m");
equation
  J1 * der(w1) = -t1;
  J2 * der(w2) = t2 - k * phi;
  der(phi) = w2;
  w1 = r(w2) * w2;
  {torques};
end Test.GearSpring;
"#
    )
}

/// The same on the implicit form (the A3 check's probe: `kink-osc-41` with
/// its torques written implicitly). Held with the releases as they were,
/// the run let go of the read at rtol 1e-4 and IDAS stopped at the next
/// breakpoint (t = 0.251 s); stepped over it gave no answer within 120 s.
/// The default holds it through every crossing (1 127 crossings, each an
/// event of both reads): the run with the releases off to the bit, 862
/// rtol off (the judge, DOP853 per cell): the restarts' cost, as on the
/// explicit form (`kink-osc-41-always`, 517).
#[test]
fn the_implicit_form_holds_a_table_crossed_back_and_forth() {
    let ps: Vec<f64> = (0..41).map(|k| -30.0 + 60.0 * k as f64 / 40.0).collect();
    let vs: Vec<f64> = ps.iter().map(|p| 2.5 + 0.02 * p + 0.2 * math::sin(p / 4.0)).collect();
    let text = gear_spring(&table_text(&ps, &vs, "rad/s", "linear", "linear"), true);
    let def = prepare(&text, "Test.GearSpring", TableCells::default());
    assert!(!def.algebraics.is_empty() && def.held.len() == 2, "{:?}", def.held);
    let r = run(&def, 1e-4, 20.0, 0.01);
    assert!(r.backend.contains("IDA"), "{}", r.backend);
    assert!(!r.report.notes.iter().any(|n| n.contains("no longer held")), "{:?}", r.report.notes);
    assert!(cells(&r) > 2200, "{}", cells(&r));
    let always =
        run_with(&def, &SolverOptions { held_release: false, ..options(1e-4) }, 20.0, 0.01);
    assert_eq!(r.values, always.values);
}

/// The limit of holding (the A3 review's `o041`): the gear on a torsion
/// spring, its ratio a linear table of 41 points on [−30, 30] rad/s
/// through `2.5 + w/50 + 0.2 sin(w/4)`, the load's speed swept back and
/// forth through it from 28 rad/s (1 127 crossings in 20 s). Held through
/// every crossing, each a restart costing about 0.45 rtol: 517 rtol off at
/// rtol 1e-4 where stepped over it is 55 (against DOP853 per cell, the
/// judge's `kink-osc-41`); held with the releases (`TableCells::Jumps`),
/// the run lets go of it after its first crossings there (they come every
/// few steps) and is 53. At 1e-6 the crossings are far enough apart that
/// it stays held (489 against 819). Against the held run at rtol 1e-11 and
/// the exact energy `½ J1 w1² + ½ J2 w2² + ½ k φ²`.
#[test]
fn a_dense_table_crossed_back_and_forth() {
    let ps: Vec<f64> = (0..41).map(|k| -30.0 + 60.0 * k as f64 / 40.0).collect();
    let vs: Vec<f64> = ps.iter().map(|p| 2.5 + 0.02 * p + 0.2 * math::sin(p / 4.0)).collect();
    let text = gear_spring(&table_text(&ps, &vs, "rad/s", "linear", "linear"), false);
    let jumps = prepare(&text, "Test.GearSpring", TableCells::Jumps);
    assert_eq!(jumps.held.len(), 2);
    let off = prepare(&text, "Test.GearSpring", TableCells::Off);
    let always = |rtol: f64| SolverOptions { held_release: false, ..options(rtol) };
    let fine = run_with(&jumps, &always(1e-11), 20.0, 0.01);
    let (fp, fw) = (fine.channel("phi").unwrap().to_vec(), fine.channel("w2").unwrap().to_vec());
    let q = |r: &SimResult, rtol: f64| error(r, "phi", &fp).max(error(r, "w2", &fw)) / rtol;
    let energy = |r: &SimResult| {
        let (w1, w2, ph) =
            (r.channel("w1").unwrap(), r.channel("w2").unwrap(), r.channel("phi").unwrap());
        let e: Vec<f64> = (0..w2.len())
            .map(|k| {
                0.5 * 0.05 * w1[k] * w1[k] + 0.5 * 2.0 * w2[k] * w2[k] + 0.5 * 50.0 * ph[k] * ph[k]
            })
            .collect();
        e.iter().fold(0.0f64, |a, v| a.max((v - e[0]).abs())) / e[0]
    };
    let mut seen = vec![];
    for rtol in [1e-4, 1e-6] {
        let o = run(&off, rtol, 20.0, 0.01);
        let a = run_with(&jumps, &always(rtol), 20.0, 0.01);
        let j = run(&jumps, rtol, 20.0, 0.01);
        let released = j.report.notes.iter().any(|n| n.contains("no longer held"));
        let (qo, qa, qj) = (q(&o, rtol), q(&a, rtol), q(&j, rtol));
        println!(
            "rtol {rtol:e}: error / rtol stepped over {qo:.1}, held always {qa:.1} ({} events), held \
             with the releases {qj:.1} ({} events, released {released}); energy drift / rtol {:.1} \
             {:.1} {:.1}",
            cells(&a),
            cells(&j),
            energy(&o) / rtol,
            energy(&a) / rtol,
            energy(&j) / rtol
        );
        // (two reads held: two events a crossing)
        assert!(cells(&a) > 2200, "{}", cells(&a));
        seen.push((qo, qa, qj, released, cells(&j)));
        if rtol == 1e-6 {
            // the crossings far enough apart: held through every one
            assert!(!released);
            for name in &a.names {
                assert_eq!(a.channel(name), j.channel(name), "{name}");
            }
        }
    }
    // measured: at 1e-4 stepped over 55, held always 517 (the restarts'
    // cost, the limit), held with the releases 53 (let go after 9
    // crossings); at 1e-6 819, 489, 489
    let (qo, qa, qj, released, events) = seen[0];
    assert!(qo < 165.0 && qa > 5.0 * qo, "{seen:?}");
    assert!(released && events < 40 && qj < 1.5 * qo, "{seen:?}");
    let (qo, qa, _, _, _) = seen[1];
    assert!(qa < qo && qa < 1500.0, "{seen:?}");
}
