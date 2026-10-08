//! Shared helpers for the integration tests: building small models from
//! library parts, running them with the stand-ins, reading channels at
//! checkpoint times and checking energy books.

#![allow(dead_code)]

use lsim_ir::component::build::{connect, discrete, eq, port};
use lsim_ir::{ComponentDef, Library, Modifier, ParamValue, SubDecl};
use lsim_lib::x::*;
use lsim_project::standin::{Built, Options, RunResult, RunSpec};
use lsim_solve::{OutputGrid, SolverOptions};

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

/// Prepares, compiles and runs `top` with energy meters, at `rtol`.
pub fn run(
    lib: &Library,
    top: &ComponentDef,
    t_end: f64,
    dt: f64,
    rtol: f64,
) -> (Built, RunResult) {
    let built = Built::new(lib, top, &Options { energy_meters: true, ..Default::default() })
        .unwrap_or_else(|e| {
            panic!("{}", e.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n"))
        });
    let mut spec = RunSpec::new(OutputGrid { t0: 0.0, t_end, dt });
    spec.solver = SolverOptions { rtol, atol: rtol * 1e-2, ..Default::default() };
    let res = built.run(&spec, &mut []).unwrap_or_else(|e| panic!("run failed: {e}"));
    (built, res)
}

/// A channel's value at time t (the grid holds t).
pub fn at(res: &RunResult, name: &str, t: f64) -> f64 {
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
pub fn check(res: &RunResult, name: &str, times: &[f64], exact: &[f64], scale: f64, tol: f64) {
    for (t, want) in times.iter().zip(exact) {
        let got = at(res, name, *t);
        let err = (got - want).abs() / scale;
        assert!(
            err < tol,
            "{name} at t = {t}: {got} vs exact {want} (error {err:e} of scale {scale})"
        );
    }
}

/// The energy meters of instance `path`: (port energy, loss, stored now −
/// stored at start, throughput) at each output time.
pub fn books(res: &RunResult, path: &str) -> (Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>) {
    let get = |what: &str| {
        res.channel(&format!("{path}.__energy_{what}"))
            .map(|v| v.to_vec())
            .unwrap_or_else(|| vec![0.0; res.times.len()])
    };
    let stored = get("stored");
    let s0 = stored.first().copied().unwrap_or(0.0);
    (get("ports"), get("loss"), stored.iter().map(|s| s - s0).collect(), get("throughput"))
}

/// Every part that declares books (a loss or a stored energy) closes them:
/// ports − loss − Δstored ≤ tol × its throughput, at every output time.
/// Parts without books are sources, boundaries or lossless couplings; all
/// port energies together sum to zero (connections conserve power).
pub fn books_close(built: &Built, res: &RunResult, tol: f64) {
    let n = res.times.len();
    let mut total = vec![0.0; n];
    let mut scale = vec![1.0f64; n];
    for m in &built.prep.meters {
        let path = &built.prep.model.flat.instance(m.instance).path;
        let (ports, loss, stored, thru) = books(res, path);
        for k in 0..n {
            total[k] += ports[k];
            scale[k] = scale[k].max(thru[k].abs());
        }
        if m.loss_energy.is_none() && m.stored.is_none() {
            continue;
        }
        for k in 0..n {
            let gap = ports[k] - loss[k] - stored[k];
            let s = thru[k].abs().max(stored[k].abs()).max(1.0);
            assert!(
                gap.abs() <= tol * s,
                "the books of {path} do not close at t = {}: ports {} − loss {} − Δstored {} = {gap:e} (throughput {})",
                res.times[k],
                ports[k],
                loss[k],
                stored[k],
                thru[k]
            );
        }
    }
    for k in 0..n {
        assert!(
            total[k].abs() <= tol * scale[k],
            "port energies do not sum to zero at t = {}: {:e} (largest throughput {})",
            res.times[k],
            total[k],
            scale[k]
        );
    }
}
