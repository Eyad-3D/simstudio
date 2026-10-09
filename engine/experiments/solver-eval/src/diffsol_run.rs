//! diffsol 0.17.1 (pure Rust): BDF on the ODE form and on the mass-matrix
//! DAE form (M y' = f(y), M = diag(C1, J, 0)), each with the exact
//! Jacobian as a Jacobian-vector product, dense (nalgebra or faer) LU,
//! its own root finding for the brake event, and its dense output.

use crate::problem::*;
use diffsol::{
    FaerLU, FaerMat, NalgebraLU, NalgebraMat, NewtonNonlinearSolver, NoLineSearch, OdeBuilder,
    OdeSolverMethod, OdeSolverStopReason, Vector,
};
use std::cell::Cell;
use std::rc::Rc;

/// diffsol's default smallest step is an absolute 1e-13 s, which the DAE
/// form hits at t = 0 for rtol <= 1e-10 (its consistent initialisation
/// leaves the algebraic variables' derivatives at zero, so the first
/// steps' error estimates fail until h < 1e-13). `HMIN=1e-20` lowers it.
fn set_min_step(o: &mut diffsol::OdeSolverOptions<f64>) {
    if let Ok(h) = std::env::var("HMIN") {
        o.min_timestep = h.parse().expect("HMIN must be a number");
    }
}

#[derive(Default)]
struct Counters {
    s: Cell<f64>,
    rhs: Cell<u64>,
}

macro_rules! bdf_ode {
    ($name:ident, $mat:ty, $ls:ty) => {
        pub fn $name(rtol: f64, atol: [f64; 3]) -> RunStats {
            let c = Rc::new(Counters::default());
            let (c1, c2) = (c.clone(), c.clone());
            let problem = OdeBuilder::<$mat>::new()
                .rtol(rtol)
                .atol([atol[0], atol[1]])
                .rhs_implicit(
                    move |x: &[f64], _p: &[f64], _t: f64, y: &mut [f64]| {
                        c1.rhs.set(c1.rhs.get() + 1);
                        ode_rhs(x, c1.s.get(), y);
                    },
                    |_x: &[f64], _p: &[f64], _t: f64, v: &[f64], y: &mut [f64]| {
                        let a = ode_jac();
                        y[0] = a[0][0] * v[0] + a[0][1] * v[1];
                        y[1] = a[1][0] * v[0] + a[1][1] * v[1];
                    },
                )
                .init(|_p: &[f64], _t: f64, y: &mut [f64]| y.fill(0.0), 2)
                .root(
                    move |x: &[f64], _p: &[f64], _t: f64, y: &mut [f64]| {
                        y[0] = if c2.s.get() == 0.0 { x[1] - W_ON } else { 1.0 };
                    },
                    1,
                )
                .build()
                .expect("build");
            let mut problem = problem;
            set_min_step(&mut problem.ode_options);
            let mut solver = problem.bdf::<$ls>().expect("bdf");
            solver.set_stop_time(T_END).expect("tstop");
            let times = sample_times();
            let mut st = RunStats::default();
            let mut k = 0;
            loop {
                match solver.step().expect("step") {
                    OdeSolverStopReason::InternalTimestep => {
                        let t = solver.state().t;
                        while k < times.len() && times[k] <= t {
                            let y = solver.interpolate(times[k]).expect("interp");
                            let (v1, w) = (y.get_index(0), y.get_index(1));
                            st.samples.push([v1, w, current(v1, w)]);
                            k += 1;
                        }
                    }
                    OdeSolverStopReason::RootFound(tr, _) => {
                        while k < times.len() && times[k] <= tr {
                            let y = solver.interpolate(times[k]).expect("interp");
                            let (v1, w) = (y.get_index(0), y.get_index(1));
                            st.samples.push([v1, w, current(v1, w)]);
                            k += 1;
                        }
                        st.t_event = tr;
                        solver.state_mut_back(tr).expect("back");
                        c.s.set(1.0);
                        // the brake changes w' discontinuously: refresh dy
                        let state = solver.state_mut();
                        let y = [state.y.get_index(0), state.y.get_index(1)];
                        let mut dy = [0.0; 2];
                        ode_rhs(&y, 1.0, &mut dy);
                        state.dy.set_index(0, dy[0]);
                        state.dy.set_index(1, dy[1]);
                    }
                    OdeSolverStopReason::TstopReached => {
                        while k < times.len() {
                            let y = solver.interpolate(times[k]).expect("interp");
                            let (v1, w) = (y.get_index(0), y.get_index(1));
                            st.samples.push([v1, w, current(v1, w)]);
                            k += 1;
                        }
                        break;
                    }
                }
            }
            let s = solver.get_statistics();
            st.steps = s.number_of_steps as u64;
            st.jac_evals = s.number_of_linear_solver_setups as u64;
            st.err_test_fails = s.number_of_error_test_failures as u64;
            st.rhs_evals = c.rhs.get();
            st
        }
    };
}

bdf_ode!(bdf_ode_nalgebra, NalgebraMat<f64>, NalgebraLU<f64>);
bdf_ode!(bdf_ode_faer, FaerMat<f64>, FaerLU<f64>);

macro_rules! bdf_dae {
    ($name:ident, $mat:ty, $ls:ty) => {
        pub fn $name(rtol: f64, atol: [f64; 3]) -> RunStats {
            let c = Rc::new(Counters::default());
            let (c1, c2) = (c.clone(), c.clone());
            let problem = OdeBuilder::<$mat>::new()
                .rtol(rtol)
                .atol(atol)
                .rhs_implicit(
                    move |y: &[f64], _p: &[f64], _t: f64, f: &mut [f64]| {
                        c1.rhs.set(c1.rhs.get() + 1);
                        let s = c1.s.get();
                        f[0] = y[2] - y[0] / R1;
                        f[1] = K * y[2] - B * y[1] - TB * s;
                        f[2] = E - R0 * y[2] - y[0] - K * y[1];
                    },
                    |_y: &[f64], _p: &[f64], _t: f64, v: &[f64], f: &mut [f64]| {
                        f[0] = -v[0] / R1 + v[2];
                        f[1] = -B * v[1] + K * v[2];
                        f[2] = -v[0] - K * v[1] - R0 * v[2];
                    },
                )
                .mass(|v: &[f64], _p: &[f64], _t: f64, beta: f64, y: &mut [f64]| {
                    y[0] = C1 * v[0] + beta * y[0];
                    y[1] = J * v[1] + beta * y[1];
                    y[2] *= beta;
                })
                // the current starts at a wrong guess (0 A): the solver's
                // consistent initialisation must find 8000 A
                .init(|_p: &[f64], _t: f64, y: &mut [f64]| y.fill(0.0), 3)
                .root(
                    move |y: &[f64], _p: &[f64], _t: f64, g: &mut [f64]| {
                        g[0] = if c2.s.get() == 0.0 { y[1] - W_ON } else { 1.0 };
                    },
                    1,
                )
                .build()
                .expect("build");
            let mut problem = problem;
            set_min_step(&mut problem.ode_options);
            let mut solver = problem.bdf::<$ls>().expect("bdf");
            solver.set_stop_time(T_END).expect("tstop");
            let times = sample_times();
            let mut st = RunStats::default();
            let mut k = 0;
            let push = |st: &mut RunStats, y: &<$mat as diffsol::MatrixCommon>::V| {
                st.samples
                    .push([y.get_index(0), y.get_index(1), y.get_index(2)]);
            };
            loop {
                match solver.step().expect("step") {
                    OdeSolverStopReason::InternalTimestep => {
                        let t = solver.state().t;
                        while k < times.len() && times[k] <= t {
                            let y = solver.interpolate(times[k]).expect("interp");
                            push(&mut st, &y);
                            k += 1;
                        }
                    }
                    OdeSolverStopReason::RootFound(tr, _) => {
                        while k < times.len() && times[k] <= tr {
                            let y = solver.interpolate(times[k]).expect("interp");
                            push(&mut st, &y);
                            k += 1;
                        }
                        st.t_event = tr;
                        solver.state_mut_back(tr).expect("back");
                        c.s.set(1.0);
                        let problem = solver.problem();
                        let mut newton = NewtonNonlinearSolver::new(<$ls>::default(), NoLineSearch);
                        solver
                            .state_mut()
                            .set_consistent(problem, &mut newton)
                            .expect("reinit");
                    }
                    OdeSolverStopReason::TstopReached => {
                        while k < times.len() {
                            let y = solver.interpolate(times[k]).expect("interp");
                            push(&mut st, &y);
                            k += 1;
                        }
                        break;
                    }
                }
            }
            let s = solver.get_statistics();
            st.steps = s.number_of_steps as u64;
            st.jac_evals = s.number_of_linear_solver_setups as u64;
            st.err_test_fails = s.number_of_error_test_failures as u64;
            st.rhs_evals = c.rhs.get();
            st
        }
    };
}

bdf_dae!(bdf_dae_nalgebra, NalgebraMat<f64>, NalgebraLU<f64>);
bdf_dae!(bdf_dae_faer, FaerMat<f64>, FaerLU<f64>);
