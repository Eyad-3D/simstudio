//! SUNDIALS backend: CVODE for models without iteration variables, IDA for
//! index-1 DAEs, both with the model's exact Jacobian (dense in Stage 1;
//! work package 4 plugs in a sparse LU written in Rust, since SUNDIALS'
//! own sparse solver KLU is LGPL and not allowed).

use crate::{
    Integrator, Method, OutputGrid, RunInfo, SolveError, SolverOptions, SolverStats, Step,
};
use lsim_ir::runtime::{EvalInput, Layout, ModelFunctions};
use std::ffi::c_void;
use std::marker::PhantomData;
use std::os::raw::{c_int, c_long};
use std::ptr;
use sundials_sys::*;

/// What the C callbacks reach through their user-data pointer.
struct Problem {
    /// lifetime-erased; valid while the owning [`Sundials`] lives
    model: *const (dyn ModelFunctions + 'static),
    layout: Layout,
    p: Vec<f64>,
    d: Vec<f64>,
    u: Vec<f64>,
    work: Vec<f64>,
    out: Vec<f64>,
    jac: Vec<f64>,
}

impl Problem {
    #[allow(clippy::type_complexity)]
    fn parts(
        &mut self,
    ) -> (&dyn ModelFunctions, &[f64], &[f64], &[f64], &mut Vec<f64>, &mut Vec<f64>, &mut Vec<f64>)
    {
        // SAFETY: `model` outlives the integrator that owns this problem
        // (`Sundials<'m>` borrows it for 'm).
        let m = unsafe { &*self.model };
        (m, &self.p, &self.d, &self.u, &mut self.work, &mut self.out, &mut self.jac)
    }
}

/// # Safety
/// `v` must be a serial N_Vector of at least `n` values.
unsafe fn slice<'a>(v: N_Vector, n: usize) -> &'a mut [f64] {
    // SAFETY: by the caller's contract.
    unsafe { std::slice::from_raw_parts_mut(N_VGetArrayPointer(v), n) }
}

/// # Safety
/// `ud` must be the `Problem` installed as user data.
unsafe fn problem<'a>(ud: *mut c_void) -> &'a mut Problem {
    // SAFETY: by the caller's contract.
    unsafe { &mut *(ud as *mut Problem) }
}

unsafe extern "C" fn cv_rhs(t: f64, y: N_Vector, ydot: N_Vector, ud: *mut c_void) -> c_int {
    // SAFETY: SUNDIALS passes our vectors and user data back.
    unsafe {
        let pr = problem(ud);
        let n = pr.layout.n_y();
        let (m, p, d, u, work, _, _) = pr.parts();
        let inp = EvalInput { t, y: slice(y, n), p, d, u };
        m.residual(&inp, work, slice(ydot, n));
    }
    0
}

#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn cv_jac(
    t: f64,
    y: N_Vector,
    _fy: N_Vector,
    jm: SUNMatrix,
    ud: *mut c_void,
    _t1: N_Vector,
    _t2: N_Vector,
    _t3: N_Vector,
) -> c_int {
    // SAFETY: as in `cv_rhs`; `jm` is the n×n dense matrix we created.
    unsafe {
        let pr = problem(ud);
        let n = pr.layout.n_y();
        let (m, p, d, u, work, _, jac) = pr.parts();
        let inp = EvalInput { t, y: slice(y, n), p, d, u };
        m.jacobian_dense(&inp, work, jac);
        for j in 0..n {
            let col = SUNDenseMatrix_Column(jm, j as sunindextype);
            std::ptr::copy_nonoverlapping(jac.as_ptr().add(j * n), col, n);
        }
    }
    0
}

unsafe extern "C" fn cv_root(t: f64, y: N_Vector, g: *mut f64, ud: *mut c_void) -> c_int {
    // SAFETY: as in `cv_rhs`; `g` has one value per root function.
    unsafe {
        let pr = problem(ud);
        let n = pr.layout.n_y();
        let nr = pr.layout.n_roots;
        let (m, p, d, u, work, _, _) = pr.parts();
        let inp = EvalInput { t, y: slice(y, n), p, d, u };
        m.roots(&inp, work, std::slice::from_raw_parts_mut(g, nr));
    }
    0
}

unsafe extern "C" fn ida_res(
    t: f64,
    yy: N_Vector,
    yp: N_Vector,
    rr: N_Vector,
    ud: *mut c_void,
) -> c_int {
    // SAFETY: as in `cv_rhs`.
    unsafe {
        let pr = problem(ud);
        let n = pr.layout.n_y();
        let nx = pr.layout.n_x;
        let (m, p, d, u, work, out, _) = pr.parts();
        let inp = EvalInput { t, y: slice(yy, n), p, d, u };
        m.residual(&inp, work, out);
        let (ypv, r) = (slice(yp, n), slice(rr, n));
        for i in 0..n {
            r[i] = if i < nx { ypv[i] - out[i] } else { out[i] };
        }
    }
    0
}

#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn ida_jac(
    t: f64,
    cj: f64,
    yy: N_Vector,
    _yp: N_Vector,
    _rr: N_Vector,
    jm: SUNMatrix,
    ud: *mut c_void,
    _t1: N_Vector,
    _t2: N_Vector,
    _t3: N_Vector,
) -> c_int {
    // SAFETY: as in `cv_jac`.
    unsafe {
        let pr = problem(ud);
        let n = pr.layout.n_y();
        let nx = pr.layout.n_x;
        let (m, p, d, u, work, _, jac) = pr.parts();
        let inp = EvalInput { t, y: slice(yy, n), p, d, u };
        m.jacobian_dense(&inp, work, jac);
        for j in 0..n {
            let col =
                std::slice::from_raw_parts_mut(SUNDenseMatrix_Column(jm, j as sunindextype), n);
            for i in 0..n {
                let a = jac[j * n + i];
                col[i] = if i < nx { -a + if i == j { cj } else { 0.0 } } else { a };
            }
        }
    }
    0
}

unsafe extern "C" fn ida_root(
    t: f64,
    yy: N_Vector,
    _yp: N_Vector,
    g: *mut f64,
    ud: *mut c_void,
) -> c_int {
    // SAFETY: as in `cv_root`.
    unsafe { cv_root(t, yy, g, ud) }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Cvode,
    Ida,
}

/// CVODE or IDA, chosen by whether the model has iteration variables.
pub struct Sundials<'m> {
    kind: Kind,
    mem: *mut c_void,
    ctx: SUNContext,
    y: N_Vector,
    yp: N_Vector,
    tmp: N_Vector,
    id: N_Vector,
    atol: N_Vector,
    a: SUNMatrix,
    ls: SUNLinearSolver,
    prob: Box<Problem>,
    n: usize,
    n_roots: usize,
    root_dirs: Vec<c_int>,
    t_last: f64,
    t_end: f64,
    done: SolverStats,
    _model: PhantomData<&'m dyn ModelFunctions>,
}

fn check(flag: c_int, what: &str, t: f64) -> Result<c_int, SolveError> {
    if flag < 0 {
        Err(SolveError::Integrator { t, message: format!("{what} failed (SUNDIALS flag {flag})") })
    } else {
        Ok(flag)
    }
}

impl<'m> Sundials<'m> {
    /// Sets up the integrator at the grid's start with `y0` (iteration
    /// variables are made consistent) and the discrete values `d0`.
    pub fn new(
        model: &'m dyn ModelFunctions,
        info: &RunInfo,
        opts: &SolverOptions,
        grid: OutputGrid,
        y0: &[f64],
        d0: Vec<f64>,
        u: Vec<f64>,
    ) -> Result<Self, SolveError> {
        let layout = *model.layout();
        let n = layout.n_y();
        let kind = if layout.n_z == 0 { Kind::Cvode } else { Kind::Ida };
        // SAFETY: erases the borrow's lifetime; `Sundials<'m>` keeps 'm alive
        // for as long as the callbacks can run (they run only inside its
        // methods), so the pointer never dangles.
        let model_ptr: *const (dyn ModelFunctions + 'static) = unsafe {
            std::mem::transmute::<&'m dyn ModelFunctions, &'static dyn ModelFunctions>(model)
        };
        let prob = Box::new(Problem {
            model: model_ptr,
            layout,
            p: info.params.clone(),
            d: d0,
            u,
            work: vec![0.0; layout.n_work],
            out: vec![0.0; n],
            jac: vec![0.0; n * n],
        });
        let t0 = grid.t0;
        // SAFETY: plain SUNDIALS set-up; every object is freed in `Drop`.
        unsafe {
            let mut ctx: SUNContext = ptr::null_mut();
            check(SUNContext_Create(SUN_COMM_NULL, &mut ctx), "SUNContext_Create", t0)?;
            let len = n.max(1) as sunindextype;
            let y = N_VNew_Serial(len, ctx);
            let yp = N_VNew_Serial(len, ctx);
            let tmp = N_VNew_Serial(len, ctx);
            let id = N_VNew_Serial(len, ctx);
            let atol = N_VNew_Serial(len, ctx);
            slice(y, n).copy_from_slice(y0);
            slice(yp, n).fill(0.0);
            for i in 0..n {
                slice(atol, n)[i] = opts.atol * info.y_nominal[i];
                slice(id, n)[i] = if i < layout.n_x { 1.0 } else { 0.0 };
            }
            let a = SUNDenseMatrix(len, len, ctx);
            let ls = SUNLinSol_Dense(y, a, ctx);
            let mut s = Sundials {
                kind,
                mem: ptr::null_mut(),
                ctx,
                y,
                yp,
                tmp,
                id,
                atol,
                a,
                ls,
                prob,
                n,
                n_roots: layout.n_roots,
                root_dirs: info.root_dirs.iter().map(|&x| x as c_int).collect(),
                t_last: t0,
                t_end: grid.t_end,
                done: SolverStats::default(),
                _model: PhantomData,
            };
            let ud = &mut *s.prob as *mut Problem as *mut c_void;
            match kind {
                Kind::Cvode => {
                    let lmm = if opts.method == Method::Adams { CV_ADAMS } else { CV_BDF };
                    s.mem = CVodeCreate(lmm, ctx);
                    check(CVodeInit(s.mem, Some(cv_rhs), t0, y), "CVodeInit", t0)?;
                    check(CVodeSVtolerances(s.mem, opts.rtol, atol), "CVodeSVtolerances", t0)?;
                    check(CVodeSetUserData(s.mem, ud), "CVodeSetUserData", t0)?;
                    check(CVodeSetLinearSolver(s.mem, ls, a), "CVodeSetLinearSolver", t0)?;
                    check(CVodeSetJacFn(s.mem, Some(cv_jac)), "CVodeSetJacFn", t0)?;
                    check(
                        CVodeSetMaxNumSteps(s.mem, opts.max_steps as c_long),
                        "CVodeSetMaxNumSteps",
                        t0,
                    )?;
                    if opts.max_step > 0.0 {
                        check(CVodeSetMaxStep(s.mem, opts.max_step), "CVodeSetMaxStep", t0)?;
                    }
                }
                Kind::Ida => {
                    s.mem = IDACreate(ctx);
                    check(IDAInit(s.mem, Some(ida_res), t0, y, yp), "IDAInit", t0)?;
                    check(IDASVtolerances(s.mem, opts.rtol, atol), "IDASVtolerances", t0)?;
                    check(IDASetUserData(s.mem, ud), "IDASetUserData", t0)?;
                    check(IDASetLinearSolver(s.mem, ls, a), "IDASetLinearSolver", t0)?;
                    check(IDASetJacFn(s.mem, Some(ida_jac)), "IDASetJacFn", t0)?;
                    check(IDASetId(s.mem, id), "IDASetId", t0)?;
                    check(
                        IDASetMaxNumSteps(s.mem, opts.max_steps as c_long),
                        "IDASetMaxNumSteps",
                        t0,
                    )?;
                    if opts.max_step > 0.0 {
                        check(IDASetMaxStep(s.mem, opts.max_step), "IDASetMaxStep", t0)?;
                    }
                }
            }
            s.init_roots()?;
            if kind == Kind::Ida {
                s.consistent(t0)?;
            }
            Ok(s)
        }
    }

    fn init_roots(&mut self) -> Result<(), SolveError> {
        if self.n_roots == 0 {
            return Ok(());
        }
        let t = self.t_last;
        // SAFETY: `mem` is live; the directions array has one entry per root.
        unsafe {
            match self.kind {
                Kind::Cvode => {
                    check(
                        CVodeRootInit(self.mem, self.n_roots as c_int, Some(cv_root)),
                        "CVodeRootInit",
                        t,
                    )?;
                    check(
                        CVodeSetRootDirection(self.mem, self.root_dirs.as_mut_ptr()),
                        "CVodeSetRootDirection",
                        t,
                    )?;
                }
                Kind::Ida => {
                    check(
                        IDARootInit(self.mem, self.n_roots as c_int, Some(ida_root)),
                        "IDARootInit",
                        t,
                    )?;
                    check(
                        IDASetRootDirection(self.mem, self.root_dirs.as_mut_ptr()),
                        "IDASetRootDirection",
                        t,
                    )?;
                }
            }
        }
        Ok(())
    }

    /// IDA: makes z and x' consistent with x at t.
    fn consistent(&mut self, t: f64) -> Result<(), SolveError> {
        let h = (1e-3 * (self.t_end - t).abs()).max(1e-9);
        // SAFETY: `mem` is a live IDA memory; y and yp are its vectors.
        unsafe {
            check(
                IDACalcIC(self.mem, IDA_YA_YDP_INIT, t + h),
                "the consistent initialisation (IDACalcIC)",
                t,
            )?;
            check(IDAGetConsistentIC(self.mem, self.y, self.yp), "IDAGetConsistentIC", t)?;
        }
        Ok(())
    }

    fn counters(&self) -> SolverStats {
        let mut s = SolverStats::default();
        let mut x: c_long = 0;
        // SAFETY: `mem` is live; these only read counters.
        unsafe {
            match self.kind {
                Kind::Cvode => {
                    CVodeGetNumSteps(self.mem, &mut x);
                    s.steps = x as u64;
                    CVodeGetNumRhsEvals(self.mem, &mut x);
                    s.rhs_evals = x as u64;
                    CVodeGetNumJacEvals(self.mem, &mut x);
                    s.jac_evals = x as u64;
                    CVodeGetNumErrTestFails(self.mem, &mut x);
                    s.err_test_fails = x as u64;
                    CVodeGetNumNonlinSolvConvFails(self.mem, &mut x);
                    s.nonlin_fails = x as u64;
                }
                Kind::Ida => {
                    IDAGetNumSteps(self.mem, &mut x);
                    s.steps = x as u64;
                    IDAGetNumResEvals(self.mem, &mut x);
                    s.rhs_evals = x as u64;
                    IDAGetNumJacEvals(self.mem, &mut x);
                    s.jac_evals = x as u64;
                    IDAGetNumErrTestFails(self.mem, &mut x);
                    s.err_test_fails = x as u64;
                    IDAGetNumNonlinSolvConvFails(self.mem, &mut x);
                    s.nonlin_fails = x as u64;
                }
            }
        }
        s
    }
}

impl Integrator for Sundials<'_> {
    fn name(&self) -> &'static str {
        match self.kind {
            Kind::Cvode => "SUNDIALS CVODE (BDF)",
            Kind::Ida => "SUNDIALS IDA (BDF, DAE)",
        }
    }

    fn step(&mut self, t_stop: f64) -> Result<Step, SolveError> {
        let mut t = self.t_last;
        // SAFETY: `mem` is live; y/yp are its vectors.
        let flag = unsafe {
            match self.kind {
                Kind::Cvode => {
                    check(CVodeSetStopTime(self.mem, t_stop), "CVodeSetStopTime", t)?;
                    check(
                        CVode(self.mem, t_stop, self.y, &mut t, CV_ONE_STEP),
                        "CVODE",
                        self.t_last,
                    )?
                }
                Kind::Ida => {
                    check(IDASetStopTime(self.mem, t_stop), "IDASetStopTime", t)?;
                    check(
                        IDASolve(self.mem, t_stop, &mut t, self.y, self.yp, IDA_ONE_STEP),
                        "IDA",
                        self.t_last,
                    )?
                }
            }
        };
        self.t_last = t;
        Ok(match (self.kind, flag) {
            (Kind::Cvode, CV_ROOT_RETURN) | (Kind::Ida, IDA_ROOT_RETURN) => {
                let mut info = vec![0 as c_int; self.n_roots];
                // SAFETY: `info` has one entry per root.
                unsafe {
                    match self.kind {
                        Kind::Cvode => CVodeGetRootInfo(self.mem, info.as_mut_ptr()),
                        Kind::Ida => IDAGetRootInfo(self.mem, info.as_mut_ptr()),
                    };
                }
                Step::Root(t, info)
            }
            (Kind::Cvode, CV_TSTOP_RETURN) | (Kind::Ida, IDA_TSTOP_RETURN) => Step::Stopped(t),
            _ => Step::Internal(t),
        })
    }

    fn y(&self) -> &[f64] {
        // SAFETY: y is our serial vector of n values.
        unsafe { slice(self.y, self.n) }
    }

    fn interpolate(&mut self, t: f64, out: &mut [f64]) -> Result<(), SolveError> {
        // SAFETY: `mem` is live; tmp is a vector of n values.
        unsafe {
            match self.kind {
                Kind::Cvode => check(CVodeGetDky(self.mem, t, 0, self.tmp), "CVodeGetDky", t)?,
                Kind::Ida => check(IDAGetDky(self.mem, t, 0, self.tmp), "IDAGetDky", t)?,
            };
            out.copy_from_slice(slice(self.tmp, self.n));
        }
        Ok(())
    }

    fn discrete_mut(&mut self) -> &mut [f64] {
        &mut self.prob.d
    }

    fn restart(&mut self, t: f64, y: &[f64]) -> Result<(), SolveError> {
        self.done += self.counters();
        self.done.restarts += 1;
        self.t_last = t;
        // SAFETY: `mem` is live; y/yp are its vectors.
        unsafe {
            slice(self.y, self.n).copy_from_slice(y);
            match self.kind {
                Kind::Cvode => {
                    check(CVodeReInit(self.mem, t, self.y), "CVodeReInit", t)?;
                }
                Kind::Ida => {
                    check(IDAReInit(self.mem, t, self.y, self.yp), "IDAReInit", t)?;
                }
            }
        }
        self.init_roots()?;
        if self.kind == Kind::Ida {
            self.consistent(t)?;
        }
        Ok(())
    }

    fn stats(&self) -> SolverStats {
        let mut s = self.done;
        s += self.counters();
        s
    }
}

impl Drop for Sundials<'_> {
    fn drop(&mut self) {
        // SAFETY: frees what `new` created, once.
        unsafe {
            match self.kind {
                Kind::Cvode => CVodeFree(&mut self.mem),
                Kind::Ida => IDAFree(&mut self.mem),
            }
            SUNLinSolFree(self.ls);
            SUNMatDestroy(self.a);
            for v in [self.y, self.yp, self.tmp, self.id, self.atol] {
                N_VDestroy(v);
            }
            SUNContext_Free(&mut self.ctx);
        }
    }
}
