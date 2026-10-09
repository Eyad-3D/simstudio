//! Shared helpers for the integration tests: building small models from
//! library parts, running them with the engine's pipeline (preparation,
//! code generation, the solver), reading channels at checkpoint times and
//! checking the energy books.

#![allow(dead_code)]

use lsim_ir::component::build::{connect, discrete, eq, port};
use lsim_ir::{ComponentDef, Library, Modifier, ParamValue, SubDecl};
use lsim_lib::x::*;
use lsim_project::model::Model;
use lsim_solve::{OutputGrid, SimResult, SolverOptions};

/// A sub-component with numeric parameters.
pub fn part(name: &str, def: &str, mods: &[(&str, f64)]) -> SubDecl {
    SubDecl {
        name: name.into(),
        def: def.into(),
        modifiers: mods
            .iter()
            .map(|(p, v)| Modifier {
                param: (*p).into(),
                value: ParamValue::Real(lsim_ir::expr::Expr::Const(*v)),
            })
            .collect(),
        label: None,
        ui_id: None,
    }
}

/// A model of parts and connections.
pub fn model(name: &str, parts: Vec<SubDecl>, conns: &[(&str, &str)]) -> ComponentDef {
    ComponentDef {
        name: name.into(),
        components: parts,
        connections: conns.iter().map(|(a, b)| connect(a, b)).collect(),
        ..Default::default()
    }
}

/// Records the first time its input rises above and falls below `level`.
pub fn crossing(unit: &str) -> ComponentDef {
    let first = |v: &str| ite(lt(pre(v), c(0.0)), time(), pre(v));
    ComponentDef {
        name: format!("Test.Crossing_{}", lsim_lib::x::ident(unit)),
        doc: "Records the first crossings of its input through a level.".into(),
        ports: vec![input("u", unit, "the watched signal")],
        params: vec![p("level", unit, 0.0, "the level")],
        vars: vec![
            discrete("t_up", "s", -1.0, "first time u rose above level"),
            discrete("t_down", "s", -1.0, "first time u fell below level"),
        ],
        equations: vec![
            when(gt(n("u"), n("level")), &[("t_up", first("t_up"))], "u rises above the level"),
            when(lt(n("u"), n("level")), &[("t_down", first("t_down"))], "u falls below the level"),
        ],
        ..Default::default()
    }
}

/// A library with the extra generated parts tests use.
pub fn lib() -> Library {
    let mut lib = lsim_lib::library();
    for u in ["N.m", "N", "W", "V", "A", "rad/s", "m/s", "K"] {
        lib.add(lsim_lib::signal::constant(u));
        lib.add(lsim_lib::signal::step(u));
        lib.add(crossing(u));
    }
    lib.add(ComponentDef {
        name: "Test.ConstantPowerForce".into(),
        doc: "Pushes with a constant power: f = P / v.".into(),
        ports: vec![port("flange", "TFlange", "the pushed mass")],
        params: vec![p("P", "W", 0.0, "the power")],
        equations: vec![eq(n("flange.f"), -(n("P") / n("flange.v")), "f = P / v")],
        ..Default::default()
    });
    lib
}

/// Prepares, compiles and runs `top` at `rtol` (energy books kept).
pub fn run(
    lib: &Library,
    top: &ComponentDef,
    t_end: f64,
    dt: f64,
    rtol: f64,
) -> (Model, SimResult) {
    let built = Model::build(lib, top).unwrap_or_else(|e| {
        panic!("{}", e.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n"))
    });
    let opts = SolverOptions { rtol, atol: rtol * 1e-2, ..Default::default() };
    let res = built
        .run(&opts, OutputGrid { t0: 0.0, t_end, dt }, &mut [])
        .unwrap_or_else(|e| panic!("run failed: {e}"));
    (built, res)
}

/// A channel's last value.
pub fn last(res: &SimResult, name: &str) -> f64 {
    *res.channel(name).unwrap_or_else(|| panic!("no channel {name}")).last().expect("a point")
}

/// The time of the first event whose label contains `needle`.
pub fn ev(res: &SimResult, needle: &str) -> f64 {
    res.events
        .iter()
        .find(|e| e.label.contains(needle))
        .unwrap_or_else(|| panic!("no event '{needle}' in {:?}", res.events))
        .t
}

/// A channel's value at time t (the grid holds t).
pub fn at(res: &SimResult, name: &str, t: f64) -> f64 {
    let ch = res.channel(name).unwrap_or_else(|| panic!("no channel {name}"));
    let k = res
        .times
        .iter()
        .position(|x| (x - t).abs() < 1e-9 * t.abs().max(1.0))
        .unwrap_or_else(|| panic!("{t} is not on the output grid"));
    ch[k]
}

/// Checks `name` against the exact values at the checkpoint times, to
/// `tol` of `scale`.
pub fn check(res: &SimResult, name: &str, times: &[f64], exact: &[f64], scale: f64, tol: f64) {
    for (t, want) in times.iter().zip(exact) {
        let got = at(res, name, *t);
        let err = (got - want).abs() / scale;
        assert!(
            err < tol,
            "{name} at t = {t}: {got} vs exact {want} (error {err:e} of scale {scale})"
        );
    }
}

/// A part's books on the output grid: (energy in through its ports, energy
/// lost, stored energy now − at the start, its throughput over the run).
pub fn books(res: &SimResult, path: &str) -> (Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>) {
    let e = res.energy.as_ref().expect("energy books kept");
    let n = res.times.len();
    let Some(p) = e.parts.iter().find(|p| p.path == path) else {
        return (vec![0.0; n], vec![0.0; n], vec![0.0; n], vec![0.0; n]);
    };
    let s0 = p.stored_t.first().copied().unwrap_or(0.0);
    let pad = |v: &[f64]| if v.is_empty() { vec![0.0; n] } else { v.to_vec() };
    (
        pad(&p.energy_in_t),
        pad(&p.lost_t),
        pad(&p.stored_t).iter().map(|s| s - s0).collect(),
        vec![p.throughput; n],
    )
}

/// Every part that declares books (a loss or a stored energy) closes them
/// to `tol` of its throughput (energy in − lost − stored change, with any
/// change at events counted), and the whole model's books close to `tol`
/// of the throughput.
pub fn books_close(res: &SimResult, tol: f64) {
    let e = res.energy.as_ref().expect("energy books kept");
    for p in e.parts.iter().filter(|p| p.declared) {
        let s = p.throughput.max(p.stored_change.abs()).max(1.0);
        assert!(
            p.closure.abs() <= tol * s,
            "the books of {} do not close: in {} − lost {} − stored {} = {:e} (throughput {})",
            p.path,
            p.energy_in,
            p.lost,
            p.stored_integral,
            p.closure,
            p.throughput
        );
    }
    assert!(e.relative_closure <= tol, "{}", e.summary());
}
