//! Causal signal blocks: constants, gains, sums, unit conversions,
//! integrators, limiters, first-order lags, lookup tables, time tables.
//!
//! Signal ports carry units, and a link joins ports of one dimension, so
//! the generic blocks are generated for the units they connect
//! ([`gain`], [`convert`] …); the library holds their dimensionless
//! versions. Table blocks hold their data in a table parameter `table`
//! (runtime data: a part sets its own with a modifier).

use crate::table::{Interpolation, Table1, Table2, table_param};
use crate::x::*;
use lsim_ir::ComponentDef;
use lsim_ir::TableData;

/// A generated block's name: `base`, or `base_<unit>_<unit>…` when any
/// unit is not dimensionless.
pub fn unit_name(base: &str, units: &[&str]) -> String {
    if units.iter().all(|u| u.is_empty() || *u == "1") {
        return base.to_string();
    }
    let parts: Vec<String> = units.iter().map(|u| ident(u)).collect();
    format!("{base}_{}", parts.join("_"))
}

/// A short, stable fingerprint of numbers (FNV-1a over their bits).
pub fn fingerprint(values: impl IntoIterator<Item = f64>) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for v in values {
        for b in v.to_bits().to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    format!("{h:016x}")
}

/// Fingerprint of a 1-D table.
pub fn t1_fingerprint(t: &Table1) -> String {
    fingerprint(t.x.iter().chain(&t.y).copied().chain([t.outside as u8 as f64]))
}

/// Fingerprint of a 2-D table.
pub fn t2_fingerprint(t: &Table2) -> String {
    let mut v: Vec<f64> = t.outer.clone();
    v.push(t.outer_outside as u8 as f64);
    for s in &t.sheets {
        v.push(s.x.len() as f64);
        v.extend(&s.x);
        v.extend(&s.y);
        v.push(s.outside as u8 as f64);
    }
    fingerprint(v)
}

/// y = k (a constant in `unit`).
pub fn constant(unit: &str) -> ComponentDef {
    ComponentDef {
        name: unit_name("Signal.Constant", &[unit]),
        doc: "A constant signal.".into(),
        ports: vec![output("y", unit, "the value")],
        params: vec![p("k", unit, 0.0, "the value")],
        equations: vec![eq(n("y"), n("k"), "it outputs its value")],
        ..Default::default()
    }
}

/// y = k·u.
pub fn gain(u_in: &str, u_out: &str) -> ComponentDef {
    let k_unit = if u_in == u_out { "1".to_string() } else { format!("({u_out})/({u_in})") };
    ComponentDef {
        name: unit_name("Signal.Gain", &[u_in, u_out]),
        doc: "y = k·u.".into(),
        ports: vec![input("u", u_in, "input"), output("y", u_out, "k·u")],
        params: vec![p("k", &k_unit, 1.0, "gain")],
        equations: vec![eq(n("y"), n("k") * n("u"), "y = k·u")],
        ..Default::default()
    }
}

/// y = k·u + b: a signal taken from `u_in` to `u_out` (today's display
/// units ↔ SI, at a link between a typed port and a block that works in
/// display numbers).
pub fn convert(u_in: &str, u_out: &str) -> ComponentDef {
    let k_unit = if u_in == u_out { "1".to_string() } else { format!("({u_out})/({u_in})") };
    ComponentDef {
        name: unit_name("Signal.Convert", &[u_in, u_out]),
        doc: "y = k·u + b: the same quantity in another unit.".into(),
        ports: vec![input("u", u_in, "input"), output("y", u_out, "converted")],
        params: vec![p("k", &k_unit, 1.0, "scale"), p("b", u_out, 0.0, "offset")],
        equations: vec![eq(n("y"), n("k") * n("u") + n("b"), "y = k·u + b")],
        ..Default::default()
    }
}

/// y = Σ k_i·u_i over `count` inputs u1…un.
pub fn add(unit: &str, count: usize) -> ComponentDef {
    let mut ports = vec![];
    let mut params = vec![];
    let mut terms = vec![];
    for i in 1..=count {
        ports.push(input(&format!("u{i}"), unit, "an input"));
        params.push(p(&format!("k{i}"), "1", 1.0, "its gain"));
        terms.push(n(&format!("k{i}")) * n(&format!("u{i}")));
    }
    ports.push(output("y", unit, "the weighted sum"));
    ComponentDef {
        name: format!("{}{count}", unit_name("Signal.Add", &[unit])),
        doc: format!("y = the weighted sum of {count} inputs."),
        ports,
        params,
        equations: vec![eq(n("y"), sum(terms), "y = Σ k_i·u_i")],
        ..Default::default()
    }
}

/// y = u1·u2 (units multiply).
pub fn product(u1: &str, u2: &str, u_out: &str) -> ComponentDef {
    ComponentDef {
        name: unit_name("Signal.Product", &[u1, u2, u_out]),
        doc: "y = u1·u2.".into(),
        ports: vec![
            input("u1", u1, "factor"),
            input("u2", u2, "factor"),
            output("y", u_out, "product"),
        ],
        equations: vec![eq(n("y"), n("u1") * n("u2"), "y = u1·u2")],
        ..Default::default()
    }
}

/// y = k·∫u dt + y0.
pub fn integrator(u_in: &str, u_out: &str) -> ComponentDef {
    let k_unit = format!("({u_out})/(({u_in}).s)");
    let mut x = state("y", u_out, 0.0, "the integral");
    x.start = Some(n("y0"));
    ComponentDef {
        name: unit_name("Signal.Integrator", &[u_in, u_out]),
        doc: "y = y0 + k·∫u dt.".into(),
        ports: vec![input("u", u_in, "input"), output("y_out", u_out, "the integral")],
        params: vec![p("k", &k_unit, 1.0, "gain"), p("y0", u_out, 0.0, "start value")],
        vars: vec![x],
        equations: vec![
            eq(der("y"), n("k") * n("u"), "it integrates its input"),
            eq(n("y_out"), n("y"), "it outputs the integral"),
        ],
        ..Default::default()
    }
}

/// y = clamp(u, lo, hi), a saturation that is part of the controller.
pub fn limiter(unit: &str) -> ComponentDef {
    ComponentDef {
        name: unit_name("Signal.Limiter", &[unit]),
        doc: "y = u held between lo and hi.".into(),
        ports: vec![input("u", unit, "input"), output("y", unit, "limited")],
        params: vec![p("lo", unit, -1.0, "lower limit"), p("hi", unit, 1.0, "upper limit")],
        equations: vec![eq(
            n("y"),
            clamp(n("u"), n("lo"), n("hi")),
            "it holds u within its limits",
        )],
        ..Default::default()
    }
}

/// T·dy/dt + y = k·u.
pub fn first_order(unit: &str) -> ComponentDef {
    let mut x = state("y", unit, 0.0, "output");
    x.start = Some(n("y0"));
    ComponentDef {
        name: unit_name("Signal.FirstOrder", &[unit]),
        doc: "A first-order lag: T·dy/dt + y = k·u.".into(),
        ports: vec![input("u", unit, "input"), output("y_out", unit, "lagged")],
        params: vec![
            p("k", "1", 1.0, "gain"),
            p("T", "s", 1.0, "time constant"),
            p("y0", unit, 0.0, "start value"),
        ],
        vars: vec![x],
        equations: vec![
            eq(n("T") * der("y"), n("k") * n("u") - n("y"), "it lags its input"),
            eq(n("y_out"), n("y"), "its output"),
        ],
        ..Default::default()
    }
}

fn line(x_unit: &str) -> TableData {
    TableData {
        interpolation: Interpolation::Linear,
        axis_units: [x_unit.to_string(), String::new()],
        ..TableData::new_1d(vec![0.0, 1.0], vec![0.0, 1.0])
    }
}

/// y = table(u), a 1-D table (its parameter `table`; linear by default,
/// as today's app reads tables).
pub fn table1d(u_in: &str, u_out: &str) -> ComponentDef {
    ComponentDef {
        name: unit_name("Signal.Table1D", &[u_in, u_out]),
        doc: "y = a 1-D table read at u.".into(),
        ports: vec![input("u", u_in, "abscissa"), output("y", u_out, "the table's value")],
        params: vec![table_param("table", u_out, line(u_in), "the table")],
        equations: vec![eq(
            n("y"),
            lsim_ir::expr::table("table", vec![n("u")]),
            "it reads its table",
        )],
        ..Default::default()
    }
}

/// y = table(u1, u2), a 2-D table (its parameter `table`).
pub fn table2d(u1: &str, u2: &str, u_out: &str) -> ComponentDef {
    let data = TableData {
        interpolation: Interpolation::Linear,
        axis_units: [u1.to_string(), u2.to_string()],
        ..TableData::new_2d(vec![0.0, 1.0], vec![0.0, 1.0], vec![0.0, 0.0, 0.0, 1.0])
    };
    ComponentDef {
        name: unit_name("Signal.Table2D", &[u1, u2, u_out]),
        doc: "y = a 2-D table read at (u1, u2).".into(),
        ports: vec![
            input("u1", u1, "first abscissa"),
            input("u2", u2, "second abscissa"),
            output("y", u_out, "the table's value"),
        ],
        params: vec![table_param("table", u_out, data, "the table")],
        equations: vec![eq(
            n("y"),
            lsim_ir::expr::table("table", vec![n("u1"), n("u2")]),
            "it reads its table",
        )],
        ..Default::default()
    }
}

/// y = table(time), a signal that follows a table over time.
pub fn time_table(u_out: &str) -> ComponentDef {
    ComponentDef {
        name: unit_name("Signal.TimeTable", &[u_out]),
        doc: "y = a table read at the time.".into(),
        ports: vec![output("y", u_out, "the table's value now")],
        params: vec![table_param("table", u_out, line("s"), "the table over time")],
        vars: vec![var("t_now", "s", "the time")],
        equations: vec![
            eq(n("t_now"), time(), "the time"),
            eq(n("y"), lsim_ir::expr::table("table", vec![n("t_now")]), "it reads its table"),
        ],
        ..Default::default()
    }
}

/// A table parameter's value from today's data: a 1-D table (`Table1`) as
/// runtime data with its axis unit.
pub fn t1_value(t: &Table1, x_unit: &str) -> Result<lsim_ir::ParamValue, String> {
    t.data(x_unit).map(lsim_ir::ParamValue::Table)
}

/// A 2-D table parameter's value from today's sheets (resampled onto one
/// grid).
pub fn t2_value(t: &Table2, units: [&str; 2]) -> Result<lsim_ir::ParamValue, String> {
    t.grid_data(units).map(lsim_ir::ParamValue::Table)
}

/// y = y0 before t_step, y1 from then on (an event at t_step).
pub fn step(unit: &str) -> ComponentDef {
    let mut after = discrete("after", "1", 0.0, "1 once stepped");
    after.start = Some(ite(le(n("t_step"), c(0.0)), c(1.0), c(0.0)));
    ComponentDef {
        name: unit_name("Signal.Step", &[unit]),
        doc: "y = y0 before t_step, y1 from then on (an event at t_step).".into(),
        ports: vec![output("y", unit, "the step")],
        params: vec![
            p("y0", unit, 0.0, "the value before"),
            p("y1", unit, 1.0, "the value after"),
            p("t_step", "s", 0.0, "when it steps"),
        ],
        vars: vec![after],
        equations: vec![
            eq(n("y"), n("y0") + (n("y1") - n("y0")) * n("after"), "before or after the step"),
            when(ge(time(), n("t_step")), &[("after", c(1.0))], "it steps at t_step"),
        ],
        ..Default::default()
    }
}

/// The dimensionless signal blocks for the library.
pub fn signal() -> Vec<ComponentDef> {
    let mut v = vec![
        constant("1"),
        gain("1", "1"),
        convert("1", "1"),
        add("1", 2),
        add("1", 3),
        product("1", "1", "1"),
        integrator("1", "1"),
        limiter("1"),
        first_order("1"),
        table1d("1", "1"),
        table2d("1", "1", "1"),
        time_table("1"),
    ];
    v.push(step("1"));
    v
}
