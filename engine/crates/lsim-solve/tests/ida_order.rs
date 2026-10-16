//! IDA's order on a model whose states are exactly polynomial in time,
//! with the energy books under error control: a motor's constant torque
//! spins a load up through the library's final drive (a DAE: the gear's
//! input inertia reads the output's acceleration), so the speeds grow
//! exactly linearly and the books' integrals as t². The steps and the
//! books' error fall with the tolerance as they should, against the exact
//! answer of the mesh law.

mod common;

use lsim_ir::component::build::{connect, sub};
use lsim_ir::expr::c;
use lsim_solve::{OutputGrid, SolverOptions, simulate};

const RATIO: f64 = 12.8;
const ETA: f64 = 0.9;
const T: f64 = 100.0; // the motor's torque, N·m
const J_ROTOR: f64 = 0.045; // the rotor, kg·m²
const J_IN: f64 = 0.01; // the gear's input side
const J_OUT: f64 = 0.02; // its output side
const J_LOAD: f64 = 1500.0 * 0.35 * 0.35; // a car on its wheels
const T_END: f64 = 2.0;

fn spin_up() -> lsim_ir::ComponentDef {
    lsim_ir::ComponentDef {
        name: "Test.FinalDriveSpinUp".into(),
        components: vec![
            sub("drive", "Rotational.ConstantTorque", &[("tau", c(T))]),
            sub("rotor", "Rotational.Inertia", &[("J", c(J_ROTOR))]),
            sub(
                "fd",
                "Blocks.FinalDrive",
                &[
                    ("ratio", c(RATIO)),
                    ("efficiency_pct", c(ETA)),
                    ("inertia_in_kgm2", c(J_IN)),
                    ("inertia_out_kgm2", c(J_OUT)),
                    // the efficiency's blend through zero power: far below
                    // anything the run passes after its first instant
                    ("p_dir", c(1e-9)),
                ],
            ),
            sub("load", "Rotational.Inertia", &[("J", c(J_LOAD))]),
        ],
        connections: vec![
            connect("drive.flange", "rotor.a"),
            connect("rotor.b", "fd.flange_in"),
            connect("fd.flange_out", "load.a"),
        ],
        ..Default::default()
    }
}

/// The mesh law: the output's acceleration and the gear's loss by `t`.
fn exact(t: f64) -> (f64, f64) {
    let (j1, j2) = (J_ROTOR + J_IN, J_OUT + J_LOAD);
    let alpha = RATIO * ETA * T / (j2 + ETA * RATIO * RATIO * j1);
    let tau = T - j1 * RATIO * alpha;
    (alpha, (1.0 - ETA) * tau * RATIO * alpha * t * t / 2.0)
}

#[test]
fn ida_raises_its_order_on_states_exactly_polynomial_in_time() {
    let built = common::build(&common::library(), &spin_up(), false);
    let l = *lsim_ir::runtime::ModelFunctions::layout(&built.jit);
    assert!(l.n_z > 0, "a DAE: it runs on IDA");
    let (alpha, loss) = exact(T_END);
    let grid = OutputGrid { t0: 0.0, t_end: T_END, dt: 0.5 };
    let mut rows = vec![];
    for rtol in [1e-6, 1e-8, 1e-10] {
        let opts = SolverOptions { rtol, atol: rtol, ..Default::default() };
        let run = simulate(&built.jit, &built.info, &opts, grid, &mut []).expect("runs");
        assert!(run.backend.contains("IDA"), "{}", run.backend);
        let w = *run.channel("fd.w_out").unwrap().last().unwrap();
        let e = run.energy.as_ref().expect("books");
        let lost = e.parts.iter().find(|p| p.path == "fd").expect("the gear").lost;
        let (ew, el) = ((w - alpha * T_END).abs() / (alpha * T_END), (lost - loss).abs() / loss);
        println!(
            "rtol {rtol:.0e}: {} steps, {} error test failures; speed {ew:.1e} off, the gear's \
             loss {el:.1e} off, closure {:.1e}",
            run.stats.steps, run.stats.err_test_fails, e.relative_closure
        );
        rows.push((rtol, run.stats.steps, el));
        // (no event, no restart: the order is IDA's own choice)
        assert!(run.events.is_empty() && run.stats.restarts == 0, "{:?}", run.events);
        // the speed exact to round-off; the loss's book, an integral of t
        // that order 2 integrates exactly, to well inside the tolerance
        // (before: 0.26·√rtol off, 2.6e-4, 2.6e-5 and 2.6e-6)
        assert!(ew < 1e-12, "speed {w} against {}", alpha * T_END);
        assert!(el < 1e-3 * rtol, "loss {lost} against {loss}: {el:e}");
        assert!(e.relative_closure < 1e-9, "{}", e.relative_closure);
    }
    // the books' quadratures no longer set the step at order 1 (before:
    // 6 406, 63 715 and 636 288 steps, growing as 1/√rtol); what is left is
    // mostly the start, where the gear's efficiency blends through zero
    // power (p_dir) in the first nanosecond
    for (rtol, steps, _) in &rows {
        assert!(*steps < 200, "rtol {rtol:e}: {steps} steps");
    }
    assert!(rows[2].1 < 3 * rows[0].1, "the steps hardly grow as the tolerance tightens");
}

/// CVODE decides its order on the states' and the quadratures' errors
/// together already: the same spin-up without the gear's inertias (an ODE)
/// takes a few steps at every tolerance, its books exact.
#[test]
fn cvode_keeps_its_order_on_states_exactly_polynomial_in_time() {
    let top = lsim_ir::ComponentDef {
        name: "Test.SpinUp".into(),
        components: vec![
            sub("drive", "Rotational.ConstantTorque", &[("tau", c(T))]),
            sub("rotor", "Rotational.Inertia", &[("J", c(J_ROTOR + J_LOAD))]),
        ],
        connections: vec![connect("drive.flange", "rotor.a")],
        ..Default::default()
    };
    let built = common::build(&common::library(), &top, false);
    let grid = OutputGrid { t0: 0.0, t_end: T_END, dt: 0.5 };
    let j = J_ROTOR + J_LOAD;
    for rtol in [1e-6, 1e-8, 1e-10] {
        let opts = SolverOptions { rtol, atol: rtol, ..Default::default() };
        let run = simulate(&built.jit, &built.info, &opts, grid, &mut []).expect("runs");
        assert!(run.backend.contains("CVODE"), "{}", run.backend);
        let e = run.energy.as_ref().expect("books");
        let stored = e.parts.iter().find(|p| p.path == "rotor").expect("the rotor").stored_integral;
        // ½ J w² with w = T t / J
        let want = 0.5 * T * T * T_END * T_END / j;
        let es = (stored - want).abs() / want;
        println!(
            "rtol {rtol:.0e}: {} steps on {}, the rotor's book {es:.1e} off",
            run.stats.steps, run.backend
        );
        assert!(run.stats.steps < 100 && es < 1e-2 * rtol, "{} steps, {es:e}", run.stats.steps);
    }
}
