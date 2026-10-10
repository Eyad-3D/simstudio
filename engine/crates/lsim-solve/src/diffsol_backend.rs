//! diffsol backend (pure Rust, MIT; feature `diffsol`): BDF with the
//! model's exact Jacobian-vector product, nalgebra dense LU; an ODE for
//! models without iteration variables, the mass-matrix DAE `M y' = [f; g]`
//! with `M = diag(I, 0)` otherwise. It is the independent second
//! integrator of the cross-check suite (DESIGN.md §3.5), driven by the same
//! run loop as SUNDIALS, so everything around the integrator (events,
//! modes, sampled blocks, energy books, recording) is shared.
//!
//! What diffsol does not provide, this module adds: root directions (each
//! crossing is classified from the root functions' signs at the step's
//! start and at the root), consistent iteration variables at the start and
//! after an event (the same Newton and homotopy as SUNDIALS' path,
//! [`crate::init`]), and the energy integrals (three-point Gauss–Legendre
//! on the dense output over each step).
//!
//! diffsol's smallest step is an absolute 1e-13 s by default, which its DAE
//! path hits at tight tolerances (DESIGN.md §3.2); it is lowered to 1e-20.

use crate::energy::Integrand;
use crate::init::{InitSettings, consistent_z};
use crate::jac::JacStructure;
use crate::{
    DenseOutput, Integrator, OutputGrid, RunInfo, SimResult, SolveError, SolverOptions,
    SolverStats, Step, run_loop,
};
use diffsol::{
    Bdf, NalgebraContext, NalgebraLU, NalgebraMat, NalgebraVec, OdeBuilder, OdeEquationsImplicit,
    OdeSolverMethod, OdeSolverProblem, OdeSolverStopReason, Vector,
};
use lsim_ir::runtime::{DiscreteBlock, EvalInput, ModelFunctions};
use std::cell::{Cell, RefCell};
use std::time::Instant;

/// What the closures share with the integrator.
struct Shared {
    d: RefCell<Vec<f64>>,
    zero_side: RefCell<Vec<f64>>,
    root_mask: RefCell<Vec<bool>>,
    work: RefCell<Vec<f64>>,
    rhs: Cell<u64>,
    jvp: Cell<u64>,
}

/// Gauss–Legendre nodes and weights on [0, 1].
const GAUSS: [(f64, f64); 3] = [
    (0.112_701_665_379_258_31, 5.0 / 18.0),
    (0.5, 8.0 / 18.0),
    (0.887_298_334_620_741_7, 5.0 / 18.0),
];

fn to_vec(v: &NalgebraVec<f64>) -> Vec<f64> {
    (0..v.len()).map(|i| v.get_index(i)).collect()
}

/// Runs the model with diffsol: see the module documentation.
#[allow(clippy::too_many_arguments)]
pub fn simulate(
    model: &dyn ModelFunctions,
    info: &RunInfo,
    opts: &SolverOptions,
    grid: OutputGrid,
    y0: &[f64],
    d0: Vec<f64>,
    u: &[f64],
    quad: Option<Integrand>,
    blocks: &mut [Box<dyn DiscreteBlock>],
    started: Instant,
) -> Result<SimResult, SolveError> {
    let l = *model.layout();
    let n = l.n_y();
    let n_x = l.n_x;
    let nr = l.n_roots;
    let ng = model.table_guard_list().len();
    let p = info.params.clone();
    // a consistent start for the iteration variables (diffsol's own
    // initialisation then has nothing left to do)
    let jac = JacStructure::for_model(model, info.pattern.as_ref(), n);
    let mut y_start = y0.to_vec();
    let init = consistent_z(
        model,
        info,
        &jac,
        grid.t0,
        &mut y_start,
        &p,
        &d0,
        u,
        &InitSettings { rtol: opts.rtol, atol: opts.atol, max_iterations: 50 },
    )?;
    let shared = Shared {
        d: RefCell::new(d0),
        zero_side: RefCell::new(vec![0.0; nr + ng]),
        root_mask: RefCell::new(vec![]),
        work: RefCell::new(vec![0.0; l.n_work]),
        rhs: Cell::new(0),
        jvp: Cell::new(0),
    };
    let atol: Vec<f64> = info.y_nominal.iter().map(|nm| opts.atol * nm).collect();
    let rhs = |x: &[f64], _p: &[f64], t: f64, out: &mut [f64]| {
        shared.rhs.set(shared.rhs.get() + 1);
        let d = shared.d.borrow();
        let inp = EvalInput { t, y: x, p: &p, d: &d, u };
        model.residual(&inp, &mut shared.work.borrow_mut(), out);
    };
    let jvp = |x: &[f64], _p: &[f64], t: f64, v: &[f64], out: &mut [f64]| {
        shared.jvp.set(shared.jvp.get() + 1);
        let d = shared.d.borrow();
        let inp = EvalInput { t, y: x, p: &p, d: &d, u };
        model.jvp(&inp, v, &mut shared.work.borrow_mut(), out);
    };
    let y_init = y_start.clone();
    let init_fn = move |_p: &[f64], _t: f64, y: &mut [f64]| y.copy_from_slice(&y_init);
    let root = |x: &[f64], _p: &[f64], t: f64, out: &mut [f64]| {
        if nr + ng == 0 {
            out[0] = 1.0;
            return;
        }
        let d = shared.d.borrow();
        let inp = EvalInput { t, y: x, p: &p, d: &d, u };
        let mut work = shared.work.borrow_mut();
        model.roots(&inp, &mut work, &mut out[..nr]);
        if ng > 0 {
            model.table_guards(&inp, &mut work, &mut out[nr..]);
        }
        crate::run::apply_zero_sides(out, &shared.zero_side.borrow(), &shared.root_mask.borrow());
    };
    let builder = OdeBuilder::<NalgebraMat<f64>>::new()
        .t0(grid.t0)
        .rtol(opts.rtol)
        .atol(atol)
        .rhs_implicit(rhs, jvp)
        .init(init_fn, n)
        .root(root, (nr + ng).max(1));
    let ctx = Ctx {
        model,
        info,
        opts,
        grid,
        u,
        shared: &shared,
        quad,
        jac,
        init_note: format!("initialisation: {}", init.describe()),
    };
    let err = |e: diffsol::DiffsolError| SolveError::Integrator {
        t: grid.t0,
        message: format!("diffsol: {e}"),
    };
    if n_x == n {
        let mut problem = builder.build().map_err(err)?;
        problem.ode_options.min_timestep = 1e-20;
        drive(&problem, ctx, blocks, started, false)
    } else {
        let mass = move |v: &[f64], _p: &[f64], _t: f64, beta: f64, y: &mut [f64]| {
            for i in 0..y.len() {
                y[i] = if i < n_x { v[i] + beta * y[i] } else { beta * y[i] };
            }
        };
        let mut problem = builder.mass(mass).build().map_err(err)?;
        problem.ode_options.min_timestep = 1e-20;
        drive(&problem, ctx, blocks, started, true)
    }
}

struct Ctx<'a> {
    model: &'a dyn ModelFunctions,
    info: &'a RunInfo,
    opts: &'a SolverOptions,
    grid: OutputGrid,
    u: &'a [f64],
    shared: &'a Shared,
    quad: Option<Integrand>,
    jac: JacStructure,
    init_note: String,
}

fn drive<'a, Eqn>(
    problem: &'a OdeSolverProblem<Eqn>,
    ctx: Ctx<'a>,
    blocks: &mut [Box<dyn DiscreteBlock>],
    started: Instant,
    dae: bool,
) -> Result<SimResult, SolveError>
where
    Eqn: OdeEquationsImplicit<
            T = f64,
            V = NalgebraVec<f64>,
            M = NalgebraMat<f64>,
            C = NalgebraContext,
        > + 'a,
{
    let solver = problem.bdf::<NalgebraLU<f64>>().map_err(|e| SolveError::Integrator {
        t: ctx.grid.t0,
        message: format!("diffsol could not start: {e}"),
    })?;
    let n = ctx.model.layout().n_y();
    let n_q = ctx.quad.as_ref().map(|q| q.len()).unwrap_or(0);
    let y = to_vec(solver.state().y);
    let mut integ = Diffsol {
        solver,
        model: ctx.model,
        info: ctx.info,
        opts: ctx.opts,
        u: ctx.u,
        shared: ctx.shared,
        quad: ctx.quad,
        jac: ctx.jac,
        dae,
        n,
        y,
        t: ctx.grid.t0,
        t_y: ctx.grid.t0,
        t_a: ctx.grid.t0,
        q_a: vec![0.0; n_q],
        q_b: vec![0.0; n_q],
        pending_back: None,
        restarts: 0,
        notes: vec![ctx.init_note],
        tmp: NalgebraVec::zeros(n, Default::default()),
        _p: std::marker::PhantomData,
    };
    let u = ctx.u.to_vec();
    run_loop(ctx.model, ctx.info, ctx.opts, ctx.grid, &mut integ, &u, blocks, started)
}

/// The integrator over a diffsol BDF solver.
struct Diffsol<'a, Eqn>
where
    Eqn: OdeEquationsImplicit<
            T = f64,
            V = NalgebraVec<f64>,
            M = NalgebraMat<f64>,
            C = NalgebraContext,
        > + 'a,
{
    solver: Bdf<'a, Eqn, NalgebraLU<f64>>,
    model: &'a dyn ModelFunctions,
    info: &'a RunInfo,
    opts: &'a SolverOptions,
    u: &'a [f64],
    shared: &'a Shared,
    quad: Option<Integrand>,
    jac: JacStructure,
    dae: bool,
    n: usize,
    /// y at `t` (the end of the last step, or a root inside it)
    y: Vec<f64>,
    t: f64,
    /// the time `y` is the state at: `t`, but for a step that ended a few
    /// ulps short of the stop time, reported at the stop time
    t_y: f64,
    /// the last step's start and the integrals there
    t_a: f64,
    q_a: Vec<f64>,
    /// the integrals at `t`
    q_b: Vec<f64>,
    /// the solver is past `t` (a root inside its step): move it back
    /// before stepping on, unless a restart does
    pending_back: Option<f64>,
    restarts: u64,
    notes: Vec<String>,
    tmp: NalgebraVec<f64>,
    _p: std::marker::PhantomData<Eqn>,
}

impl<'a, Eqn> Diffsol<'a, Eqn>
where
    Eqn: OdeEquationsImplicit<
            T = f64,
            V = NalgebraVec<f64>,
            M = NalgebraMat<f64>,
            C = NalgebraContext,
        > + 'a,
{
    fn fail(&self, what: &str, e: impl std::fmt::Display) -> SolveError {
        SolveError::Integrator { t: self.t, message: format!("diffsol {what}: {e}") }
    }

    fn interp(&mut self, t: f64) -> Result<Vec<f64>, SolveError> {
        let ts = self.solver.state().t;
        if t == ts || crate::run::same_instant(t, ts) {
            return Ok(to_vec(self.solver.state().y));
        }
        let r = self.solver.interpolate_inplace(t, &mut self.tmp);
        r.map_err(|e| self.fail("interpolation", e))?;
        Ok(to_vec(&self.tmp))
    }

    fn interp_dy(&mut self, t: f64) -> Result<Vec<f64>, SolveError> {
        let ts = self.solver.state().t;
        if t == ts || crate::run::same_instant(t, ts) {
            return Ok(to_vec(self.solver.state().dy));
        }
        let r = self.solver.interpolate_dy_inplace(t, &mut self.tmp);
        r.map_err(|e| self.fail("interpolation", e))?;
        Ok(to_vec(&self.tmp))
    }

    fn roots_at(&mut self, t: f64, y: &[f64]) -> Vec<f64> {
        let nr = self.model.layout().n_roots;
        let ng = self.model.table_guard_list().len();
        let mut g = vec![0.0; nr + ng];
        let d = self.shared.d.borrow();
        let inp = EvalInput { t, y, p: &self.info.params, d: &d, u: self.u };
        let mut work = self.shared.work.borrow_mut();
        self.model.roots(&inp, &mut work, &mut g[..nr]);
        if ng > 0 {
            self.model.table_guards(&inp, &mut work, &mut g[nr..]);
        }
        drop(work);
        crate::run::apply_zero_sides(
            &mut g,
            &self.shared.zero_side.borrow(),
            &self.shared.root_mask.borrow(),
        );
        g
    }

    /// The integrals at `t` (inside the last step): those at its start
    /// plus a Gauss–Legendre rule on the dense output.
    fn integrate(&mut self, t: f64, out: &mut [f64]) -> Result<(), SolveError> {
        out.copy_from_slice(&self.q_a);
        let Some(mut q) = self.quad.take() else { return Ok(()) };
        let h = t - self.t_a;
        let mut res = Ok(());
        if h > 0.0 {
            let mut f = vec![0.0; out.len()];
            let mut f_x = vec![0.0; self.n];
            for (node, w) in GAUSS {
                let tn = self.t_a + node * h;
                let (y, mut yd) = match (self.interp(tn), self.interp_dy(tn)) {
                    (Ok(y), Ok(yd)) => (y, yd),
                    (Err(e), _) | (_, Err(e)) => {
                        res = Err(e);
                        break;
                    }
                };
                let d = self.shared.d.borrow();
                let inp = EvalInput { t: tn, y: &y, p: &self.info.params, d: &d, u: self.u };
                // x' from the model itself (exact at the point); z' from the
                // interpolant
                self.model.residual(&inp, &mut self.shared.work.borrow_mut(), &mut f_x);
                let n_x = self.model.layout().n_x;
                yd[..n_x].copy_from_slice(&f_x[..n_x]);
                q.eval(self.model, &inp, &yd, &mut self.shared.work.borrow_mut(), &mut f);
                for k in 0..out.len() {
                    out[k] += w * h * f[k];
                }
            }
        }
        self.quad = Some(q);
        res
    }

    /// The integrals at the solver's state (after a step that ended a few
    /// ulps short of a stop time): those at its end.
    fn integrate_to_state(&mut self) -> Result<(), SolveError> {
        let ts = self.solver.state().t;
        let mut q = vec![0.0; self.q_a.len()];
        if ts > self.t_a {
            self.integrate(ts, &mut q)?;
        } else {
            q.copy_from_slice(&self.q_a);
        }
        self.q_b = q;
        Ok(())
    }

    /// Puts the solver at `t` with `y` (iteration variables made
    /// consistent) and restarts it at first order.
    fn reset_to(&mut self, t: f64, y: &[f64]) -> Result<(), SolveError> {
        let mut y = y.to_vec();
        if self.dae {
            let d = self.shared.d.borrow().clone();
            consistent_z(
                self.model,
                self.info,
                &self.jac,
                t,
                &mut y,
                &self.info.params,
                &d,
                self.u,
                &InitSettings { rtol: self.opts.rtol, atol: self.opts.atol, max_iterations: 50 },
            )?;
        }
        let mut f = vec![0.0; self.n];
        {
            let d = self.shared.d.borrow();
            let inp = EvalInput { t, y: &y, p: &self.info.params, d: &d, u: self.u };
            self.model.residual(&inp, &mut self.shared.work.borrow_mut(), &mut f);
        }
        let n_x = self.model.layout().n_x;
        let h_old = self.solver.state().h.abs();
        let s = self.solver.state_mut();
        for i in 0..self.n {
            s.y.set_index(i, y[i]);
            s.dy.set_index(i, if i < n_x { f[i] } else { 0.0 });
        }
        *s.t = t;
        // a first-order restart: start well below the last step
        *s.h = (0.1 * h_old).max(1e-12 * t.abs().max(1.0));
        self.y = y;
        self.t = t;
        self.t_y = t;
        self.t_a = t;
        self.pending_back = None;
        Ok(())
    }
}

impl<'a, Eqn> Integrator for Diffsol<'a, Eqn>
where
    Eqn: OdeEquationsImplicit<
            T = f64,
            V = NalgebraVec<f64>,
            M = NalgebraMat<f64>,
            C = NalgebraContext,
        > + 'a,
{
    fn name(&self) -> &'static str {
        if self.dae { "diffsol BDF (DAE, mass matrix)" } else { "diffsol BDF" }
    }

    fn step(&mut self, t_stop: f64) -> Result<Step, SolveError> {
        if let Some(tb) = self.pending_back.take() {
            // a root that changed nothing: go on from it
            let y = self.y.clone();
            let q = self.q_b.clone();
            self.reset_to(tb, &y)?;
            self.q_a = q;
        }
        let t_state = self.solver.state().t;
        // diffsol may stop a few ulps short of a stop time: that is the
        // stop time
        let near =
            |a: f64, b: f64| (a - b).abs() <= 8.0 * f64::EPSILON * a.abs().max(b.abs()).max(1.0);
        if near(t_stop, t_state) {
            self.y = to_vec(self.solver.state().y);
            self.t = t_stop;
            self.t_y = t_state;
            self.q_b = self.q_a.clone();
            self.integrate_to_state()?;
            return Ok(Step::Stopped(t_stop));
        }
        if t_stop > t_state {
            let r = self.solver.set_stop_time(t_stop);
            r.map_err(|e| self.fail("stop time", e))?;
        }
        // the step's start, for the energy integrals
        if self.t_a != t_state {
            self.q_a = self.q_b.clone();
            self.t_a = t_state;
        }
        let r = self.solver.step();
        let reason = r.map_err(|e| self.fail("step", e))?;
        let t_n = self.solver.state().t;
        match reason {
            OdeSolverStopReason::InternalTimestep | OdeSolverStopReason::TstopReached => {
                self.y = to_vec(self.solver.state().y);
                self.t = t_n;
                self.t_y = t_n;
                let mut q = vec![0.0; self.q_a.len()];
                self.integrate(t_n, &mut q)?;
                self.q_b = q;
                Ok(if matches!(reason, OdeSolverStopReason::TstopReached) || near(t_stop, t_n) {
                    self.t = t_stop;
                    Step::Stopped(t_stop)
                } else {
                    Step::Internal(t_n)
                })
            }
            OdeSolverStopReason::RootFound(t_r, _) => {
                // directions from the signs at the step's start and at the root
                let ya = self.interp(self.t_a)?;
                let ga = self.roots_at(self.t_a, &ya);
                let yr = self.interp(t_r)?;
                let gr = self.roots_at(t_r, &yr);
                let dirs: Vec<i32> = ga
                    .iter()
                    .zip(&gr)
                    .map(|(a, b)| {
                        if *a < 0.0 && *b >= 0.0 {
                            1
                        } else if *a > 0.0 && *b <= 0.0 {
                            -1
                        } else {
                            0
                        }
                    })
                    .collect();
                let mut q = vec![0.0; self.q_a.len()];
                self.integrate(t_r, &mut q)?;
                self.q_b = q;
                self.y = yr;
                self.t = t_r;
                self.t_y = t_r;
                self.pending_back = Some(t_r);
                // table guards (after the model's roots) are watched both ways
                let watched = dirs.iter().enumerate().any(|(k, d)| {
                    let w = self.info.root_dirs.get(k).copied().unwrap_or(0);
                    *d != 0 && (w == 0 || w == *d)
                });
                Ok(if watched { Step::Root(t_r, dirs) } else { Step::Internal(t_r) })
            }
        }
    }

    fn y(&self) -> &[f64] {
        &self.y
    }

    fn interpolate(&mut self, t: f64, out: &mut [f64]) -> Result<(), SolveError> {
        if t == self.t {
            out.copy_from_slice(&self.y);
            return Ok(());
        }
        let y = self.interp(t)?;
        out.copy_from_slice(&y);
        Ok(())
    }

    /// diffsol's BDF interpolates a step with a polynomial of degree its
    /// order, at most 5, but does not expose its difference array: its
    /// values at six Chebyshev points of `[t0, t]` give that polynomial in
    /// Newton form, up to round-off. The error carried is an estimate, not
    /// a proof: 4096 ε of the largest value, generous for the round-off of
    /// diffsol's own interpolation, which this cannot see (diffsol 0.17.1
    /// keeps its differences and order private). The step's end node is
    /// the time the state is at (a step that ended a few ulps short of the
    /// stop time is reported at the stop time).
    fn dense_output(
        &mut self,
        t0: f64,
        idx: &[usize],
        out: &mut DenseOutput,
    ) -> Result<bool, SolveError> {
        let t1 = self.t_y;
        if t0 >= t1 || t0 < self.t_a {
            if t0 >= t1 {
                let ys: Vec<f64> = idx.iter().map(|i| self.y[*i]).collect();
                out.constant(t1, ys.into_iter());
                return Ok(true);
            }
            return Ok(false);
        }
        const K: usize = 6;
        let (c, h) = (0.5 * (t0 + t1), 0.5 * (t1 - t0));
        let ts: [f64; K] = std::array::from_fn(|i| match i {
            0 => t1,
            _ if i == K - 1 => t0,
            _ => c + h * (i as f64 * std::f64::consts::PI / (K - 1) as f64).cos(),
        });
        let mut f = vec![[0.0; K]; idx.len()];
        for (i, t) in ts.iter().enumerate() {
            let y = if i == 0 { self.y.clone() } else { self.interp(*t)? };
            for (fm, k) in f.iter_mut().zip(idx) {
                fm[i] = y[*k];
            }
        }
        out.origin = t1;
        out.nodes = ts[..K - 1].iter().map(|t| t - t1).collect();
        out.scales = vec![1.0; K - 1];
        out.coef.clear();
        out.err.clear();
        for fm in &f {
            // divided differences, in place
            let mut a = *fm;
            for j in 1..K {
                for i in (j..K).rev() {
                    a[i] = (a[i] - a[i - 1]) / (ts[i] - ts[i - j]);
                }
            }
            out.coef.extend_from_slice(&a);
            let big = fm.iter().fold(0.0f64, |m, v| m.max(v.abs()));
            out.err.push(4096.0 * f64::EPSILON * big);
        }
        Ok(true)
    }

    fn discrete_mut(&mut self) -> &mut [f64] {
        // SAFETY: the closures borrow `d` only while the solver evaluates
        // them, inside this integrator's own methods; the returned slice
        // borrows `self` mutably, so none of them can run while it lives.
        unsafe { (*self.shared.d.as_ptr()).as_mut_slice() }
    }

    fn set_root_sides(&mut self, sides: &[f64]) {
        self.shared.zero_side.borrow_mut().copy_from_slice(sides);
    }

    fn set_root_mask(&mut self, mask: &[bool]) -> bool {
        *self.shared.root_mask.borrow_mut() = mask.to_vec();
        true
    }

    fn restart(&mut self, t: f64, y: &[f64]) -> Result<(), SolveError> {
        let mut q = vec![0.0; self.q_a.len()];
        if t == self.t {
            q.copy_from_slice(&self.q_b);
        } else {
            self.integrate(t, &mut q)?;
        }
        self.reset_to(t, y)?;
        self.q_a = q.clone();
        self.q_b = q;
        self.restarts += 1;
        Ok(())
    }

    fn consistent_z(&mut self, t: f64, y: &mut [f64], d: &[f64]) -> Result<(), SolveError> {
        if !self.dae {
            return Ok(());
        }
        consistent_z(
            self.model,
            self.info,
            &self.jac,
            t,
            y,
            &self.info.params,
            d,
            self.u,
            &InitSettings { rtol: self.opts.rtol, atol: self.opts.atol, max_iterations: 50 },
        )
        .map(|_| ())
    }

    fn stats(&self) -> SolverStats {
        let s = self.solver.get_statistics();
        SolverStats {
            steps: s.number_of_steps as u64,
            rhs_evals: self.shared.rhs.get(),
            jac_evals: self.shared.jvp.get(),
            err_test_fails: s.number_of_error_test_failures as u64,
            nonlin_fails: s.number_of_nonlinear_solver_fails as u64,
            restarts: self.restarts,
            lin_setups: s.number_of_linear_solver_setups as u64,
        }
    }

    fn quadrature(&mut self, t: f64, out: &mut [f64]) -> Result<(), SolveError> {
        if t == self.t {
            out.copy_from_slice(&self.q_b);
            Ok(())
        } else {
            self.integrate(t, out)
        }
    }

    fn setup_notes(&self) -> Vec<String> {
        let mut n = self.notes.clone();
        n.push(format!(
            "diffsol 0.17.1 BDF, nalgebra dense LU on {} unknowns, Jacobian from products",
            self.n
        ));
        n
    }
}
