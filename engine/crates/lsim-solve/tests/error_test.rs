//! Where IDA's error test cannot pass. A gear's output acceleration (an
//! iteration variable index reduction made, `der(fd.w_out)`) is the
//! derivative of what IDA integrates: where the torque driving it starts
//! a ramp (a kink in a table read along time, a stop of the run without a
//! restart), its slope jumps, and a step from the long history of the
//! steady run before fails the error test ten times (before: "IDA failed
//! (SUNDIALS flag -3)" at t = 1 s, h = 3.7e-6 s, at rtol 1e-8 and 1e-10;
//! the Battery Electric Car's city cycle at 1e-9 the same way at 120 s). A
//! fresh start there crosses it: the run restarts the integrator once at
//! that point and goes on, to the exact answer. Where the torque jumps
//! without an event, no step longer than the time's resolution passes:
//! the run stops there at once and says so (before, IDA went on taking
//! steps too short to move the time, without end).

mod common;

use lsim_ir::component::build::{connect, eq, param, port, sub};
use lsim_ir::expr::{CmpOp, Expr, c, cmp, name as n};
use lsim_ir::{ComponentDef, Interpolation, Outside, ParamDecl, ParamValue, TableData};
use lsim_solve::{OutputGrid, SolveError, SolverOptions, simulate};

const RATIO: f64 = 12.8;
const ETA: f64 = 0.9;
const TAU: f64 = 100.0; // the torque the ramp ends at, N·m
const J_ROTOR: f64 = 0.045;
const J_IN: f64 = 0.01;
const J_OUT: f64 = 0.02;
const J_LOAD: f64 = 1500.0 * 0.35 * 0.35;
const W0: f64 = 40.0; // the load's speed at the start, rad/s

/// A torque on its flange: `profile` over time (`jump`: `TAU` from 1 s on,
/// a jump in a `noEvent` comparison, no event).
fn source(jump: bool) -> ComponentDef {
    let profile = TableData {
        interpolation: Interpolation::Linear,
        outside: [Outside::Clamp, Outside::Clamp],
        axis_units: ["s".into(), String::new()],
        ..TableData::new_1d(vec![0.0, 1.0, 2.0], vec![0.0, 0.0, TAU])
    };
    let tau = if jump {
        Expr::If(
            Box::new(Expr::NoEvent(Box::new(cmp(CmpOp::Gt, Expr::Time, c(1.0))))),
            Box::new(c(TAU)),
            Box::new(c(0.0)),
        )
    } else {
        lsim_ir::expr::table("profile", vec![Expr::Time])
    };
    ComponentDef {
        name: "Test.TorqueProfile".into(),
        ports: vec![port("flange", "Flange", "")],
        params: vec![ParamDecl {
            default: ParamValue::Table(profile),
            ..param("profile", "N.m", 0.0, "")
        }],
        equations: vec![eq(n("flange.tau"), -tau, "it drives its flange")],
        ..Default::default()
    }
}

/// The torque through a rotor and the library's final drive onto a load
/// turning at `W0`.
fn drive() -> ComponentDef {
    ComponentDef {
        name: "Test.DriveUp".into(),
        components: vec![
            sub("drive", "Test.TorqueProfile", &[]),
            sub("rotor", "Rotational.Inertia", &[("J", c(J_ROTOR)), ("w0", c(W0 * RATIO))]),
            sub(
                "fd",
                "Blocks.FinalDrive",
                &[
                    ("ratio", c(RATIO)),
                    ("efficiency_pct", c(ETA)),
                    ("inertia_in_kgm2", c(J_IN)),
                    ("inertia_out_kgm2", c(J_OUT)),
                    ("p_dir", c(1e-9)),
                ],
            ),
            sub("load", "Rotational.Inertia", &[("J", c(J_LOAD)), ("w0", c(W0))]),
        ],
        connections: vec![
            connect("drive.flange", "rotor.a"),
            connect("rotor.b", "fd.flange_in"),
            connect("fd.flange_out", "load.a"),
        ],
        ..Default::default()
    }
}

fn library(jump: bool) -> lsim_ir::Library {
    let mut lib = common::library();
    lib.add(source(jump));
    lib
}

/// The load's speed by `t` (the mesh law, as `tests/ida_order.rs`): its
/// acceleration in proportion to the torque, the ramp's integral.
fn exact(t: f64) -> f64 {
    let (j1, j2) = (J_ROTOR + J_IN, J_OUT + J_LOAD);
    let k = RATIO * ETA / (j2 + ETA * RATIO * RATIO * j1);
    let integral = match t {
        t if t <= 1.0 => 0.0,
        t if t <= 2.0 => 0.5 * TAU * (t - 1.0) * (t - 1.0),
        t => 0.5 * TAU + TAU * (t - 2.0),
    };
    W0 + k * integral
}

#[test]
fn ida_restarts_where_its_history_cannot_cross_a_kink() {
    let built = common::build(&library(false), &drive(), false);
    let l = *lsim_ir::runtime::ModelFunctions::layout(&built.jit);
    assert!(l.n_z > 0, "a DAE: it runs on IDA");
    let grid = OutputGrid { t0: 0.0, t_end: 3.0, dt: 0.5 };
    for rtol in [1e-8, 1e-10] {
        let opts = SolverOptions { rtol, atol: rtol * 1e-2, ..Default::default() };
        let run = simulate(&built.jit, &built.info, &opts, grid, &mut []).expect("runs");
        assert!(run.backend.contains("IDA"), "{}", run.backend);
        let w = run.channel("load.w").unwrap();
        let worst = run
            .times
            .iter()
            .zip(w)
            .map(|(t, w)| (w - exact(*t)).abs() / exact(*t))
            .fold(0.0, f64::max);
        let notes: Vec<&String> =
            run.report.notes.iter().filter(|n| n.contains("error test failed")).collect();
        println!(
            "rtol {rtol:.0e}: {} steps, {} error test failures, the speed {worst:.1e} off; {notes:?}",
            run.stats.steps, run.stats.err_test_fails
        );
        assert!(worst < 1e-2 * rtol, "{worst:e}");
        assert_eq!(notes.len(), 1, "the restart is noted: {:?}", run.report.notes);
    }
}

#[test]
fn a_jump_no_step_passes_stops_the_run_where_it_is() {
    let built = common::build(&library(true), &drive(), false);
    let grid = OutputGrid { t0: 0.0, t_end: 3.0, dt: 0.5 };
    let opts = SolverOptions { rtol: 1e-8, atol: 1e-10, ..Default::default() };
    let started = std::time::Instant::now();
    let err = simulate(&built.jit, &built.info, &opts, grid, &mut []).map(|_| ()).unwrap_err();
    println!("{err} ({:.2} s)", started.elapsed().as_secs_f64());
    let SolveError::Integrator { t, message } = &err else { panic!("{err}") };
    assert!((t - 1.0).abs() < 1e-9, "{err}");
    assert!(message.contains("no progress") && message.contains("fd."), "{err}");
}
