//! SUNDIALS backend: CVODES for models without iteration variables, IDAS
//! for index-1 DAEs (built in-tree by `lsim-sundials-sys`), with the
//! model's exact Jacobian evaluated by coloured Jacobian-vector products
//! into a dense, band or sparse matrix; the sparse LU is faer's
//! ([`crate::faer_ls`]), since SUNDIALS' own (KLU) is LGPL.
//!
//! * Method: BDF with Newton by default; with [`Method::Auto`] a
//!   power-iteration estimate of the Jacobian's spectral radius, against
//!   the run's length at the start and against the step actually taken at
//!   each restart, selects Adams with fixed-point iteration for plainly
//!   non-stiff models; convergence trouble switches back to BDF.
//! * Energy books: integrated as CVODES/IDAS quadratures next to the
//!   states (optionally under error control).
//! * Initialisation: IDA's iteration variables by damped Newton, then a
//!   homotopy ([`crate::init`]), then `IDACalcIC` for the derivatives.
//! * Errors: SUNDIALS' own messages are captured from the context's error
//!   handler and passed on in [`SolveError`].

use crate::energy::Integrand;
use crate::init::{InitSettings, consistent_z};
use crate::jac::JacStructure;
use crate::{
    Integrator, LinearSolver, Method, OutputGrid, RunInfo, SolveError, SolverOptions, SolverStats,
    Step,
};
use lsim_ir::runtime::{EvalInput, Layout, ModelFunctions};
use lsim_sundials_sys::*;
use std::ffi::{CStr, c_void};
use std::marker::PhantomData;
use std::os::raw::{c_char, c_int, c_long};
use std::ptr;

/// The linear solver in use.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Lin {
    Dense,
    Band,
    Sparse,
}

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
    seed: Vec<f64>,
    jout: Vec<f64>,
    jac: JacStructure,
    jvals: Vec<f64>,
    lin: Lin,
    quad: Option<Integrand>,
    /// for each root function: the side an exact zero counts as (+1, -1;
    /// 0: none), so a function resting at zero after its event does not
    /// fire again
    zero_side: Vec<f64>,
    /// table guards watched after the model's own root functions
    n_guards: usize,
}

impl Problem {
    fn model<'a>(&self) -> &'a dyn ModelFunctions {
        // SAFETY: `model` outlives the integrator that owns this problem
        // (`Sundials<'m>` borrows it for 'm, and every use is inside its
        // methods or SUNDIALS calls they make).
        unsafe { &*self.model }
    }

    /// Fills `jm` with `f(i, j, ∂[x';g]_i/∂y_j)` over the structure.
    ///
    /// # Safety
    /// `jm` must be the matrix created for this problem's linear solver.
    unsafe fn fill(
        &mut self,
        t: f64,
        y: &[f64],
        jm: SUNMatrix,
        f: impl Fn(usize, usize, f64) -> f64,
    ) {
        let m = self.model();
        let inp = EvalInput { t, y, p: &self.p, d: &self.d, u: &self.u };
        self.jac.eval(m, &inp, &mut self.work, &mut self.seed, &mut self.jout, &mut self.jvals);
        let s = &self.jac;
        let n = s.n;
        // SAFETY: the matrix has the type and size `lin` says.
        unsafe {
            match self.lin {
                Lin::Dense => {
                    let data = std::slice::from_raw_parts_mut(SUNDenseMatrix_Data(jm), n * n);
                    data.fill(0.0);
                    for j in 0..n {
                        for k in s.col_ptr[j]..s.col_ptr[j + 1] {
                            let i = s.row_idx[k];
                            data[j * n + i] = f(i, j, self.jvals[k]);
                        }
                    }
                }
                Lin::Band => {
                    SUNMatZero(jm);
                    for j in 0..n {
                        let col = SUNBandMatrix_Column(jm, j as sunindextype);
                        for k in s.col_ptr[j]..s.col_ptr[j + 1] {
                            let i = s.row_idx[k];
                            *col.offset(i as isize - j as isize) = f(i, j, self.jvals[k]);
                        }
                    }
                }
                Lin::Sparse => {
                    let ptr = SUNSparseMatrix_IndexPointers(jm);
                    let idx = SUNSparseMatrix_IndexValues(jm);
                    let val = SUNSparseMatrix_Data(jm);
                    for j in 0..=n {
                        *ptr.add(j) = s.col_ptr[j] as sunindextype;
                    }
                    for j in 0..n {
                        for k in s.col_ptr[j]..s.col_ptr[j + 1] {
                            let i = s.row_idx[k];
                            *idx.add(k) = i as sunindextype;
                            *val.add(k) = f(i, j, self.jvals[k]);
                        }
                    }
                }
            }
        }
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

fn finite(x: &[f64]) -> c_int {
    // a non-finite value is a recoverable failure: the step is cut
    if x.iter().all(|v| v.is_finite()) { 0 } else { 1 }
}

unsafe extern "C" fn cv_rhs(t: f64, y: N_Vector, ydot: N_Vector, ud: *mut c_void) -> c_int {
    // SAFETY: SUNDIALS passes our vectors and user data back.
    unsafe {
        let pr = problem(ud);
        let n = pr.layout.n_y();
        let out = slice(ydot, n);
        let m = pr.model();
        let inp = EvalInput { t, y: slice(y, n), p: &pr.p, d: &pr.d, u: &pr.u };
        m.residual(&inp, &mut pr.work, out);
        finite(out)
    }
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
    // SAFETY: as in `cv_rhs`; `jm` is the matrix we created.
    unsafe {
        let pr = problem(ud);
        let n = pr.layout.n_y();
        pr.fill(t, slice(y, n), jm, |_, _, v| v);
    }
    0
}

unsafe extern "C" fn cv_root(t: f64, y: N_Vector, g: *mut f64, ud: *mut c_void) -> c_int {
    // SAFETY: as in `cv_rhs`; `g` has one value per root function.
    unsafe {
        let pr = problem(ud);
        let n = pr.layout.n_y();
        let nr = pr.layout.n_roots;
        let m = pr.model();
        let inp = EvalInput { t, y: slice(y, n), p: &pr.p, d: &pr.d, u: &pr.u };
        let g = std::slice::from_raw_parts_mut(g, nr + pr.n_guards);
        m.roots(&inp, &mut pr.work, &mut g[..nr]);
        if pr.n_guards > 0 {
            m.table_guards(&inp, &mut pr.work, &mut g[nr..]);
        }
        crate::run::apply_zero_sides(g, &pr.zero_side);
    }
    0
}

unsafe extern "C" fn cv_quad(t: f64, y: N_Vector, yq: N_Vector, ud: *mut c_void) -> c_int {
    // SAFETY: as in `cv_rhs`; `yq` has one value per integral.
    unsafe {
        let pr = problem(ud);
        let n = pr.layout.n_y();
        let m = pr.model();
        let Some(q) = pr.quad.as_mut() else { return 0 };
        let out = slice(yq, q.len());
        let inp = EvalInput { t, y: slice(y, n), p: &pr.p, d: &pr.d, u: &pr.u };
        // y' = f(t, y): an ODE has no iteration variables
        m.residual(&inp, &mut pr.work, &mut pr.out);
        q.eval(m, &inp, &pr.out, &mut pr.work, out);
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
        let m = pr.model();
        let inp = EvalInput { t, y: slice(yy, n), p: &pr.p, d: &pr.d, u: &pr.u };
        m.residual(&inp, &mut pr.work, &mut pr.out);
        let (ypv, r) = (slice(yp, n), slice(rr, n));
        for i in 0..n {
            r[i] = if i < nx { ypv[i] - pr.out[i] } else { pr.out[i] };
        }
        finite(r)
    }
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
        pr.fill(
            t,
            slice(yy, n),
            jm,
            |i, j, a| {
                if i < nx { -a + if i == j { cj } else { 0.0 } } else { a }
            },
        );
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

unsafe extern "C" fn ida_quad(
    t: f64,
    yy: N_Vector,
    yp: N_Vector,
    yq: N_Vector,
    ud: *mut c_void,
) -> c_int {
    // SAFETY: as in `cv_quad`; IDA passes y' with y.
    unsafe {
        let pr = problem(ud);
        let n = pr.layout.n_y();
        let m = pr.model();
        let Some(q) = pr.quad.as_mut() else { return 0 };
        let out = slice(yq, q.len());
        let inp = EvalInput { t, y: slice(yy, n), p: &pr.p, d: &pr.d, u: &pr.u };
        q.eval(m, &inp, slice(yp, n), &mut pr.work, out);
    }
    0
}

/// Collects SUNDIALS' error messages.
#[derive(Default)]
struct ErrSink {
    last: String,
}

unsafe extern "C" fn on_error(
    _line: c_int,
    func: *const c_char,
    _file: *const c_char,
    msg: *const c_char,
    code: SUNErrCode,
    ud: *mut c_void,
    _ctx: SUNContext,
) {
    // SAFETY: `ud` is our ErrSink; the strings are SUNDIALS' own.
    unsafe {
        let sink = &mut *(ud as *mut ErrSink);
        let s = |p: *const c_char| {
            if p.is_null() { String::new() } else { CStr::from_ptr(p).to_string_lossy().into() }
        };
        sink.last = format!("{} (in {}, code {code})", s(msg).trim(), s(func));
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Cvode,
    Ida,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Lmm {
    Bdf,
    Adams,
}

/// CVODES or IDAS, chosen by whether the model has iteration variables.
pub struct Sundials<'m> {
    kind: Kind,
    lmm: Lmm,
    auto: bool,
    mem: *mut c_void,
    ctx: SUNContext,
    y: N_Vector,
    yp: N_Vector,
    tmp: N_Vector,
    id: N_Vector,
    atol: N_Vector,
    ele: N_Vector,
    ewt: N_Vector,
    yq: N_Vector,
    tq: N_Vector,
    n_q: usize,
    a: SUNMatrix,
    ls: SUNLinearSolver,
    nls: SUNNonlinearSolver,
    prob: Box<Problem>,
    err: Box<ErrSink>,
    n: usize,
    n_roots: usize,
    root_dirs: Vec<c_int>,
    t_last: f64,
    t_end: f64,
    opts: SolverOptions,
    info: RunInfo,
    done: SolverStats,
    notes: Vec<String>,
    methods: Vec<(f64, Lmm)>,
    fresh: bool,
    _model: PhantomData<&'m dyn ModelFunctions>,
}

fn flag_error(flag: c_int, what: &str, t: f64, sink: &ErrSink) -> SolveError {
    let detail = if sink.last.is_empty() { String::new() } else { format!(": {}", sink.last) };
    SolveError::Integrator { t, message: format!("{what} failed (SUNDIALS flag {flag}){detail}") }
}

impl<'m> Sundials<'m> {
    fn check(&self, flag: c_int, what: &str, t: f64) -> Result<c_int, SolveError> {
        if flag < 0 { Err(flag_error(flag, what, t, &self.err)) } else { Ok(flag) }
    }

    /// Sets up the integrator at the grid's start with `y0` (iteration
    /// variables are made consistent), the discrete values `d0`, the inputs
    /// `u` and the energy integrand `quad`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        model: &'m dyn ModelFunctions,
        info: &RunInfo,
        opts: &SolverOptions,
        grid: OutputGrid,
        y0: &[f64],
        d0: Vec<f64>,
        u: Vec<f64>,
        quad: Option<Integrand>,
    ) -> Result<Self, SolveError> {
        let layout = *model.layout();
        let n = layout.n_y();
        let kind = if layout.n_z == 0 { Kind::Cvode } else { Kind::Ida };
        let t0 = grid.t0;
        // SAFETY: erases the borrow's lifetime; `Sundials<'m>` keeps 'm alive
        // for as long as the callbacks can run (they run only inside its
        // methods), so the pointer never dangles.
        let model_ptr: *const (dyn ModelFunctions + 'static) = unsafe {
            std::mem::transmute::<&'m dyn ModelFunctions, &'static dyn ModelFunctions>(model)
        };
        // the Jacobian's structure, checked against the model at the start
        let mut jac = JacStructure::for_model(model, info.pattern.as_ref(), n);
        let mut notes = vec![];
        if (info.pattern.is_some() || jac.uses_model_jacobian()) && n <= 3000 && n > 0 {
            let inp = EvalInput { t: t0, y: y0, p: &info.params, d: &d0, u: &u };
            let mut work = vec![0.0; layout.n_work];
            let miss = jac.missing(model, &inp, &mut work);
            if !miss.is_empty() {
                notes.push(format!(
                    "the Jacobian structure missed {} entries at the start; added",
                    miss.len()
                ));
                jac.extend(&miss);
            }
        }
        let lin = match (opts.linear_solver, n) {
            (LinearSolver::Dense, _) => Lin::Dense,
            (LinearSolver::Band, _) => Lin::Band,
            (LinearSolver::Sparse, _) => Lin::Sparse,
            (LinearSolver::Auto, n) if n <= 40 => Lin::Dense,
            (LinearSolver::Auto, _) if jac.ml + jac.mu <= 20 => Lin::Band,
            (LinearSolver::Auto, _) => Lin::Sparse,
        };
        let how = if jac.uses_model_jacobian() {
            "the model's own sparse Jacobian".to_string()
        } else {
            format!("{} coloured Jacobian-vector products", jac.colours.len())
        };
        notes.push(match lin {
            Lin::Dense => format!("dense LU on {n} unknowns; Jacobian from {how}"),
            Lin::Band => format!(
                "band LU on {n} unknowns (widths {} below, {} above); Jacobian from {how}",
                jac.ml, jac.mu
            ),
            Lin::Sparse => format!(
                "sparse LU (faer) on {n} unknowns, {} non-zeros; Jacobian from {how}",
                jac.nnz()
            ),
        });
        let n_q = quad.as_ref().map(|q| q.len()).unwrap_or(0);
        let prob = Box::new(Problem {
            model: model_ptr,
            layout,
            p: info.params.clone(),
            d: d0,
            u,
            work: vec![0.0; layout.n_work],
            out: vec![0.0; n],
            seed: vec![0.0; n],
            jout: vec![0.0; n],
            jvals: vec![0.0; jac.nnz()],
            jac,
            lin,
            quad,
            zero_side: vec![0.0; layout.n_roots + model.table_guard_list().len()],
            n_guards: model.table_guard_list().len(),
        });
        // SAFETY: plain SUNDIALS set-up; every object is freed in `Drop`.
        unsafe {
            let mut ctx: SUNContext = ptr::null_mut();
            if SUNContext_Create(SUN_COMM_NULL, &mut ctx) < 0 {
                return Err(SolveError::Integrator { t: t0, message: "SUNContext_Create".into() });
            }
            let mut err = Box::new(ErrSink::default());
            SUNContext_ClearErrHandlers(ctx);
            SUNContext_PushErrHandler(
                ctx,
                Some(on_error),
                &mut *err as *mut ErrSink as *mut c_void,
            );
            let len = n.max(1) as sunindextype;
            let vec = || N_VNew_Serial(len, ctx);
            let (y, yp, tmp, id, atol, ele, ewt) =
                (vec(), vec(), vec(), vec(), vec(), vec(), vec());
            let (yq, tq) = if n_q > 0 {
                let a = N_VNew_Serial(n_q as sunindextype, ctx);
                slice(a, n_q).fill(0.0);
                (a, N_VNew_Serial(n_q as sunindextype, ctx))
            } else {
                (ptr::null_mut(), ptr::null_mut())
            };
            slice(y, n).copy_from_slice(y0);
            slice(yp, n).fill(0.0);
            for i in 0..n {
                slice(atol, n)[i] = opts.atol * info.y_nominal[i];
                slice(id, n)[i] = if i < layout.n_x { 1.0 } else { 0.0 };
            }
            let jac = &prob.jac;
            let a = match lin {
                Lin::Dense => SUNDenseMatrix(len, len, ctx),
                Lin::Band => {
                    SUNBandMatrix(len, jac.mu as sunindextype, jac.ml as sunindextype, ctx)
                }
                Lin::Sparse => {
                    SUNSparseMatrix(len, len, jac.nnz().max(1) as sunindextype, CSC_MAT, ctx)
                }
            };
            let ls = match lin {
                Lin::Dense => SUNLinSol_Dense(y, a, ctx),
                Lin::Band => SUNLinSol_Band(y, a, ctx),
                Lin::Sparse => crate::faer_ls::new(ctx),
            };
            let mut s = Sundials {
                kind,
                lmm: Lmm::Bdf,
                auto: opts.method == Method::Auto && kind == Kind::Cvode,
                mem: ptr::null_mut(),
                ctx,
                y,
                yp,
                tmp,
                id,
                atol,
                ele,
                ewt,
                yq,
                tq,
                n_q,
                a,
                ls,
                nls: ptr::null_mut(),
                prob,
                err,
                n,
                n_roots: layout.n_roots + model.table_guard_list().len(),
                root_dirs: info
                    .root_dirs
                    .iter()
                    .map(|&x| x as c_int)
                    .chain(std::iter::repeat_n(0, model.table_guard_list().len()))
                    .collect(),
                t_last: t0,
                t_end: grid.t_end,
                opts: opts.clone(),
                info: info.clone(),
                done: SolverStats::default(),
                notes,
                methods: vec![],
                fresh: true,
                _model: PhantomData,
            };
            match kind {
                Kind::Cvode => {
                    let lmm = match opts.method {
                        Method::Bdf => Lmm::Bdf,
                        Method::Adams => Lmm::Adams,
                        Method::Auto => {
                            let rho = s.spectral_radius(t0);
                            let span = grid.t_end - t0;
                            let non_stiff = rho * span <= 100.0;
                            s.notes.push(format!(
                                "method: {} (spectral radius of the Jacobian {rho:.3e} 1/s over a {span} s run: {})",
                                if non_stiff { "Adams" } else { "BDF" },
                                if non_stiff { "non-stiff" } else { "stiff" }
                            ));
                            if non_stiff { Lmm::Adams } else { Lmm::Bdf }
                        }
                    };
                    s.create_cvode(lmm, t0)?;
                }
                Kind::Ida => {
                    s.mem = IDACreate(ctx);
                    let ud = &mut *s.prob as *mut Problem as *mut c_void;
                    s.check(IDAInit(s.mem, Some(ida_res), t0, y, yp), "IDAInit", t0)?;
                    s.check(IDASVtolerances(s.mem, opts.rtol, atol), "IDASVtolerances", t0)?;
                    s.check(IDASetUserData(s.mem, ud), "IDASetUserData", t0)?;
                    s.check(IDASetLinearSolver(s.mem, ls, a), "IDASetLinearSolver", t0)?;
                    s.check(IDASetJacFn(s.mem, Some(ida_jac)), "IDASetJacFn", t0)?;
                    s.check(IDASetId(s.mem, id), "IDASetId", t0)?;
                    s.check(
                        IDASetMaxNumSteps(s.mem, opts.max_steps as c_long),
                        "IDASetMaxNumSteps",
                        t0,
                    )?;
                    if opts.max_step > 0.0 {
                        s.check(IDASetMaxStep(s.mem, opts.max_step), "IDASetMaxStep", t0)?;
                    }
                    if opts.suppress_algebraic_error {
                        s.check(IDASetSuppressAlg(s.mem, 1), "IDASetSuppressAlg", t0)?;
                    }
                    if n_q > 0 {
                        s.check(IDAQuadInit(s.mem, Some(ida_quad), yq), "IDAQuadInit", t0)?;
                        s.quad_tolerances(t0)?;
                    }
                    s.init_roots()?;
                    s.methods.push((t0, Lmm::Bdf));
                    let what = s.consistent(t0)?;
                    s.notes.push(format!("initialisation: {what}"));
                }
            }
            Ok(s)
        }
    }

    fn quad_tolerances(&mut self, t: f64) -> Result<(), SolveError> {
        let on = self.opts.energy_error_control;
        // SAFETY: `mem` is live with quadratures initialised.
        unsafe {
            match self.kind {
                Kind::Cvode => {
                    self.check(CVodeSetQuadErrCon(self.mem, on as c_int), "CVodeSetQuadErrCon", t)?;
                    if on {
                        self.check(
                            CVodeQuadSStolerances(self.mem, self.opts.rtol, self.opts.atol),
                            "CVodeQuadSStolerances",
                            t,
                        )?;
                    }
                }
                Kind::Ida => {
                    self.check(IDASetQuadErrCon(self.mem, on as c_int), "IDASetQuadErrCon", t)?;
                    if on {
                        self.check(
                            IDAQuadSStolerances(self.mem, self.opts.rtol, self.opts.atol),
                            "IDAQuadSStolerances",
                            t,
                        )?;
                    }
                }
            }
        }
        Ok(())
    }

    /// (Re)creates the CVODE memory with method `lmm` at `t` from `self.y`
    /// (and the integrals in `self.yq`).
    fn create_cvode(&mut self, lmm: Lmm, t: f64) -> Result<(), SolveError> {
        let opts = self.opts.clone();
        // SAFETY: plain SUNDIALS set-up on our live objects.
        unsafe {
            if !self.mem.is_null() {
                self.done += self.counters();
                CVodeFree(&mut self.mem);
            }
            self.mem = CVodeCreate(if lmm == Lmm::Adams { CV_ADAMS } else { CV_BDF }, self.ctx);
            let ud = &mut *self.prob as *mut Problem as *mut c_void;
            self.check(CVodeInit(self.mem, Some(cv_rhs), t, self.y), "CVodeInit", t)?;
            self.check(CVodeSVtolerances(self.mem, opts.rtol, self.atol), "CVodeSVtolerances", t)?;
            self.check(CVodeSetUserData(self.mem, ud), "CVodeSetUserData", t)?;
            match lmm {
                Lmm::Bdf => {
                    self.check(
                        CVodeSetLinearSolver(self.mem, self.ls, self.a),
                        "CVodeSetLinearSolver",
                        t,
                    )?;
                    self.check(CVodeSetJacFn(self.mem, Some(cv_jac)), "CVodeSetJacFn", t)?;
                }
                Lmm::Adams => {
                    if self.nls.is_null() {
                        self.nls = SUNNonlinSol_FixedPoint(self.y, 0, self.ctx);
                    }
                    self.check(
                        CVodeSetNonlinearSolver(self.mem, self.nls),
                        "CVodeSetNonlinearSolver",
                        t,
                    )?;
                }
            }
            self.check(
                CVodeSetMaxNumSteps(self.mem, opts.max_steps as c_long),
                "CVodeSetMaxNumSteps",
                t,
            )?;
            if opts.max_step > 0.0 {
                self.check(CVodeSetMaxStep(self.mem, opts.max_step), "CVodeSetMaxStep", t)?;
            }
            if self.n_q > 0 {
                self.check(CVodeQuadInit(self.mem, Some(cv_quad), self.yq), "CVodeQuadInit", t)?;
                self.quad_tolerances(t)?;
            }
        }
        self.lmm = lmm;
        self.methods.push((t, lmm));
        self.fresh = true;
        self.init_roots()
    }

    fn init_roots(&mut self) -> Result<(), SolveError> {
        if self.n_roots == 0 {
            return Ok(());
        }
        let t = self.t_last;
        let dirs = self.root_dirs.as_mut_ptr();
        // SAFETY: `mem` is live; the directions array has one entry per root.
        unsafe {
            match self.kind {
                Kind::Cvode => {
                    self.check(
                        CVodeRootInit(self.mem, self.n_roots as c_int, Some(cv_root)),
                        "CVodeRootInit",
                        t,
                    )?;
                    self.check(CVodeSetRootDirection(self.mem, dirs), "CVodeSetRootDirection", t)?;
                }
                Kind::Ida => {
                    self.check(
                        IDARootInit(self.mem, self.n_roots as c_int, Some(ida_root)),
                        "IDARootInit",
                        t,
                    )?;
                    self.check(IDASetRootDirection(self.mem, dirs), "IDASetRootDirection", t)?;
                }
            }
        }
        Ok(())
    }

    /// An estimate of the largest magnitude of the Jacobian's eigenvalues
    /// (power iteration with Jacobian-vector products), 1/s.
    fn spectral_radius(&mut self, t: f64) -> f64 {
        let n = self.n;
        if n == 0 {
            return 0.0;
        }
        let pr = &mut *self.prob;
        let m = pr.model();
        // SAFETY: y is our serial vector of n values.
        let y = unsafe { slice(self.y, n) }.to_vec();
        let inp = EvalInput { t, y: &y, p: &pr.p, d: &pr.d, u: &pr.u };
        let mut v: Vec<f64> = (0..n).map(|i| 1.0 + 0.37 * ((i as f64) * 1.7).sin()).collect();
        let norm = |v: &[f64]| v.iter().map(|x| x * x).sum::<f64>().sqrt();
        let nv = norm(&v);
        v.iter_mut().for_each(|x| *x /= nv);
        let mut w = vec![0.0; n];
        let mut rho = 0.0f64;
        for k in 0..40 {
            m.jvp(&inp, &v, &mut pr.work, &mut w);
            let nw = norm(&w);
            if !nw.is_finite() || nw == 0.0 {
                break;
            }
            // complex pairs make the ratio oscillate: keep the largest of
            // the last iterations
            rho = if k >= 25 { rho.max(nw) } else { nw };
            for i in 0..n {
                v[i] = w[i] / nw;
            }
        }
        rho
    }

    /// IDA: makes z consistent (Newton, then homotopy) and x' with it.
    fn consistent(&mut self, t: f64) -> Result<String, SolveError> {
        let n = self.n;
        let pr = &mut *self.prob;
        // SAFETY: y is our serial vector of n values.
        let y = unsafe { slice(self.y, n) };
        let outcome = consistent_z(
            pr.model(),
            &self.info,
            &pr.jac,
            t,
            y,
            &pr.p,
            &pr.d,
            &pr.u,
            &InitSettings { rtol: self.opts.rtol, atol: self.opts.atol, max_iterations: 50 },
        )?;
        // x' from the model at the consistent point
        let m = pr.model();
        let inp = EvalInput { t, y, p: &pr.p, d: &pr.d, u: &pr.u };
        m.residual(&inp, &mut pr.work, &mut pr.out);
        let nx = pr.layout.n_x;
        // SAFETY: yp is our serial vector of n values.
        let yp = unsafe { slice(self.yp, n) };
        for (i, v) in yp.iter_mut().enumerate() {
            *v = if i < nx { pr.out[i] } else { 0.0 };
        }
        let h = (1e-3 * (self.t_end - t).abs()).max(1e-9);
        // SAFETY: `mem` is a live IDA memory; y and yp are its vectors.
        unsafe {
            self.check(IDAReInit(self.mem, t, self.y, self.yp), "IDAReInit", t)?;
            if self.n_q > 0 {
                self.check(IDAQuadReInit(self.mem, self.yq), "IDAQuadReInit", t)?;
            }
            self.check(
                IDACalcIC(self.mem, IDA_YA_YDP_INIT, t + h),
                "the consistent initialisation (IDACalcIC)",
                t,
            )?;
            self.check(IDAGetConsistentIC(self.mem, self.y, self.yp), "IDAGetConsistentIC", t)?;
        }
        self.fresh = true;
        Ok(outcome.describe())
    }

    fn counters(&self) -> SolverStats {
        let mut s = SolverStats::default();
        let mut x: c_long = 0;
        if self.mem.is_null() {
            return s;
        }
        // SAFETY: `mem` is live; these only read counters.
        unsafe {
            match self.kind {
                Kind::Cvode => {
                    CVodeGetNumSteps(self.mem, &mut x);
                    s.steps = x as u64;
                    CVodeGetNumRhsEvals(self.mem, &mut x);
                    s.rhs_evals = x as u64;
                    if self.lmm == Lmm::Bdf {
                        CVodeGetNumJacEvals(self.mem, &mut x);
                        s.jac_evals = x as u64;
                        CVodeGetNumLinSolvSetups(self.mem, &mut x);
                        s.lin_setups = x as u64;
                    }
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
                    IDAGetNumLinSolvSetups(self.mem, &mut x);
                    s.lin_setups = x as u64;
                    IDAGetNumErrTestFails(self.mem, &mut x);
                    s.err_test_fails = x as u64;
                    IDAGetNumNonlinSolvConvFails(self.mem, &mut x);
                    s.nonlin_fails = x as u64;
                }
            }
        }
        s
    }

    /// The last step's start (the dense output is valid on [start, t_last]).
    fn last_step_start(&self) -> f64 {
        if self.fresh {
            return self.t_last;
        }
        let mut h = 0.0;
        // SAFETY: `mem` is live.
        unsafe {
            match self.kind {
                Kind::Cvode => CVodeGetLastStep(self.mem, &mut h),
                Kind::Ida => IDAGetLastStep(self.mem, &mut h),
            };
        }
        self.t_last - h
    }

    /// Adams in trouble: back to BDF from the current point.
    fn adams_struggles(&self) -> bool {
        let c = self.counters();
        c.nonlin_fails >= 10 && c.nonlin_fails * 10 >= c.steps
    }
}

impl Integrator for Sundials<'_> {
    fn name(&self) -> &'static str {
        match (self.kind, self.lmm) {
            (Kind::Cvode, Lmm::Bdf) => "SUNDIALS CVODE (BDF)",
            (Kind::Cvode, Lmm::Adams) => "SUNDIALS CVODE (Adams)",
            (Kind::Ida, _) => "SUNDIALS IDA (BDF, DAE)",
        }
    }

    fn step(&mut self, t_stop: f64) -> Result<Step, SolveError> {
        let mut t = self.t_last;
        // SAFETY: `mem` is live; y/yp are its vectors.
        let flag = unsafe {
            match self.kind {
                Kind::Cvode => {
                    self.check(CVodeSetStopTime(self.mem, t_stop), "CVodeSetStopTime", t)?;
                    CVode(self.mem, t_stop, self.y, &mut t, CV_ONE_STEP)
                }
                Kind::Ida => {
                    self.check(IDASetStopTime(self.mem, t_stop), "IDASetStopTime", t)?;
                    IDASolve(self.mem, t_stop, &mut t, self.y, self.yp, IDA_ONE_STEP)
                }
            }
        };
        if flag < 0 {
            // Adams that cannot converge: the model is stiff here after all
            if self.kind == Kind::Cvode && self.lmm == Lmm::Adams && self.auto {
                self.notes.push(format!(
                    "switched from Adams to BDF at t = {:.6} s (the fixed-point iteration failed)",
                    self.t_last
                ));
                self.create_cvode(Lmm::Bdf, self.t_last)?;
                return self.step(t_stop);
            }
            let what = match self.kind {
                Kind::Cvode => "CVODE",
                Kind::Ida => "IDA",
            };
            return Err(flag_error(flag, what, self.t_last, &self.err));
        }
        self.t_last = t;
        self.fresh = false;
        if self.kind == Kind::Cvode && self.lmm == Lmm::Adams && self.auto && self.adams_struggles()
        {
            self.notes.push(format!(
                "switched from Adams to BDF at t = {t:.6} s (repeated convergence failures)"
            ));
            // the quadratures carry over: read them at t first
            if self.n_q > 0 {
                let mut q = vec![0.0; self.n_q];
                self.quadrature(t, &mut q)?;
                // SAFETY: yq holds n_q values.
                unsafe { slice(self.yq, self.n_q).copy_from_slice(&q) };
            }
            self.create_cvode(Lmm::Bdf, t)?;
            return Ok(Step::Internal(t));
        }
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
        if t == self.t_last || self.fresh {
            out.copy_from_slice(self.y());
            return Ok(());
        }
        let t = t.clamp(self.last_step_start(), self.t_last);
        // SAFETY: `mem` is live; tmp is a vector of n values.
        unsafe {
            let flag = match self.kind {
                Kind::Cvode => CVodeGetDky(self.mem, t, 0, self.tmp),
                Kind::Ida => IDAGetDky(self.mem, t, 0, self.tmp),
            };
            self.check(flag, "the dense output", t)?;
            out.copy_from_slice(slice(self.tmp, self.n));
        }
        Ok(())
    }

    fn discrete_mut(&mut self) -> &mut [f64] {
        &mut self.prob.d
    }

    fn set_root_sides(&mut self, sides: &[f64]) {
        self.prob.zero_side.copy_from_slice(sides);
    }

    fn restart(&mut self, t: f64, y: &[f64]) -> Result<(), SolveError> {
        // the integrals at t before the memory is reset
        if self.n_q > 0 {
            let mut q = vec![0.0; self.n_q];
            self.quadrature(t, &mut q)?;
            // SAFETY: yq is our vector of n_q values.
            unsafe { slice(self.yq, self.n_q).copy_from_slice(&q) };
        }
        let h_last = (self.t_last - self.last_step_start()).abs();
        self.done.restarts += 1;
        // SAFETY: y is our serial vector of n values.
        unsafe { slice(self.y, self.n).copy_from_slice(y) };
        match self.kind {
            Kind::Cvode => {
                // re-check the method where the dynamics may have changed
                let mut lmm = self.lmm;
                if self.auto {
                    let rho = self.spectral_radius(t);
                    let want = if self.lmm == Lmm::Bdf && h_last > 0.0 && rho * h_last < 0.2 {
                        Lmm::Adams
                    } else if self.lmm == Lmm::Adams && rho * h_last > 1.5 {
                        Lmm::Bdf
                    } else {
                        self.lmm
                    };
                    if want != self.lmm {
                        self.notes.push(format!(
                            "switched to {} at t = {t:.6} s (spectral radius {rho:.3e} 1/s, last step {h_last:.3e} s)",
                            if want == Lmm::Adams { "Adams" } else { "BDF" }
                        ));
                        lmm = want;
                    }
                }
                self.t_last = t;
                if lmm != self.lmm {
                    // a new memory (the old one's counters are banked there)
                    self.create_cvode(lmm, t)?;
                } else {
                    self.done += self.counters();
                    // SAFETY: `mem` is live; y is its vector.
                    unsafe {
                        self.check(CVodeReInit(self.mem, t, self.y), "CVodeReInit", t)?;
                        if self.n_q > 0 {
                            self.check(CVodeQuadReInit(self.mem, self.yq), "CVodeQuadReInit", t)?;
                        }
                    }
                    self.fresh = true;
                    self.init_roots()?;
                }
            }
            Kind::Ida => {
                self.done += self.counters();
                self.t_last = t;
                self.init_roots()?;
                self.consistent(t)?;
            }
        }
        Ok(())
    }

    fn stats(&self) -> SolverStats {
        let mut s = self.done;
        s += self.counters();
        s
    }

    fn quadrature(&mut self, t: f64, out: &mut [f64]) -> Result<(), SolveError> {
        if self.n_q == 0 {
            return Ok(());
        }
        if self.fresh {
            // right after a (re)start, before any step: the values set then
            // SAFETY: yq holds n_q values.
            out.copy_from_slice(unsafe { slice(self.yq, self.n_q) });
            return Ok(());
        }
        // SAFETY: `mem` is live with quadratures; tq is a vector of n_q.
        unsafe {
            let flag = if t == self.t_last {
                let mut tret = 0.0;
                match self.kind {
                    Kind::Cvode => CVodeGetQuad(self.mem, &mut tret, self.tq),
                    Kind::Ida => IDAGetQuad(self.mem, &mut tret, self.tq),
                }
            } else {
                let t = t.clamp(self.last_step_start(), self.t_last);
                match self.kind {
                    Kind::Cvode => CVodeGetQuadDky(self.mem, t, 0, self.tq),
                    Kind::Ida => IDAGetQuadDky(self.mem, t, 0, self.tq),
                }
            };
            self.check(flag, "the energy integrals", t)?;
            out.copy_from_slice(slice(self.tq, self.n_q));
        }
        Ok(())
    }

    fn local_error(&mut self, out: &mut [f64]) -> bool {
        if self.fresh {
            return false;
        }
        // SAFETY: `mem` is live; ele and ewt are vectors of n values.
        unsafe {
            let (a, b) = match self.kind {
                Kind::Cvode => (
                    CVodeGetEstLocalErrors(self.mem, self.ele),
                    CVodeGetErrWeights(self.mem, self.ewt),
                ),
                Kind::Ida => {
                    (IDAGetEstLocalErrors(self.mem, self.ele), IDAGetErrWeights(self.mem, self.ewt))
                }
            };
            if a < 0 || b < 0 {
                return false;
            }
            let (e, w) = (slice(self.ele, self.n), slice(self.ewt, self.n));
            for i in 0..self.n {
                out[i] = (e[i] * w[i]).abs();
            }
        }
        true
    }

    fn method(&self) -> String {
        let name = |l: Lmm| if l == Lmm::Adams { "Adams" } else { "BDF" };
        let mut parts: Vec<String> = vec![];
        for (k, (t, l)) in self.methods.iter().enumerate() {
            if k == 0 {
                parts.push(name(*l).into());
            } else if self.methods[k - 1].1 != *l {
                parts.push(format!("{} from t = {t:.6} s", name(*l)));
            }
        }
        parts.join(", then ")
    }

    fn setup_notes(&self) -> Vec<String> {
        let mut n = self.notes.clone();
        if self.prob.lin == Lin::Sparse {
            // SAFETY: `ls` is our faer solver.
            let st = unsafe { crate::faer_ls::stats(self.ls) };
            n.push(format!(
                "sparse LU: {} symbolic analyses, {} factorisations, {} solves",
                st.symbolic, st.numeric, st.solves
            ));
        }
        n
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
            if !self.nls.is_null() {
                SUNNonlinSolFree(self.nls);
            }
            SUNMatDestroy(self.a);
            for v in [self.y, self.yp, self.tmp, self.id, self.atol, self.ele, self.ewt] {
                N_VDestroy(v);
            }
            for v in [self.yq, self.tq] {
                if !v.is_null() {
                    N_VDestroy(v);
                }
            }
            SUNContext_Free(&mut self.ctx);
        }
    }
}
