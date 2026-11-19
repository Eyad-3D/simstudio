//! SUNDIALS 7.1.1 (vendored in sundials-sys 0.6.2, built by CMake from the
//! crate's own sources): CVODE on the ODE form, IDA on the DAE form. Both
//! get the exact dense Jacobian and find the brake event with their own
//! root finding; results at the sample times come from their dense output.

use crate::problem::*;
use std::ffi::c_void;
use std::os::raw::{c_int, c_long};
use std::ptr;
use sundials_sys::*;

struct User {
    s: f64,
}

unsafe fn slice<'a>(v: N_Vector, n: usize) -> &'a mut [f64] {
    std::slice::from_raw_parts_mut(N_VGetArrayPointer(v), n)
}

unsafe fn user<'a>(d: *mut c_void) -> &'a User {
    &*(d as *const User)
}

// ---------- CVODE (ODE in v1, w) ----------

unsafe extern "C" fn cv_rhs(_t: f64, y: N_Vector, ydot: N_Vector, d: *mut c_void) -> c_int {
    let x = slice(y, 2);
    let dx = slice(ydot, 2);
    ode_rhs(x, user(d).s, dx);
    0
}

unsafe extern "C" fn cv_jac(
    _t: f64,
    _y: N_Vector,
    _fy: N_Vector,
    jm: SUNMatrix,
    _d: *mut c_void,
    _t1: N_Vector,
    _t2: N_Vector,
    _t3: N_Vector,
) -> c_int {
    let a = ode_jac();
    for j in 0..2 {
        let col = SUNDenseMatrix_Column(jm, j as sunindextype);
        for i in 0..2 {
            *col.add(i) = a[i][j];
        }
    }
    0
}

unsafe extern "C" fn cv_root(_t: f64, y: N_Vector, g: *mut f64, _d: *mut c_void) -> c_int {
    let x = slice(y, 2);
    *g = x[1] - W_ON;
    0
}

fn check(flag: c_int, what: &str) {
    assert!(flag >= 0, "{what} failed with flag {flag}");
}

pub fn cvode(rtol: f64, atol: [f64; 3]) -> RunStats {
    unsafe {
        let mut ctx: SUNContext = ptr::null_mut();
        check(
            SUNContext_Create(SUN_COMM_NULL, &mut ctx),
            "SUNContext_Create",
        );
        let y = N_VNew_Serial(2, ctx);
        let av = N_VNew_Serial(2, ctx);
        slice(y, 2).copy_from_slice(&[0.0, 0.0]);
        slice(av, 2).copy_from_slice(&[atol[0], atol[1]]);
        let mut mem = CVodeCreate(CV_BDF, ctx);
        let u: *mut User = Box::into_raw(Box::new(User { s: 0.0 }));
        check(CVodeInit(mem, Some(cv_rhs), 0.0, y), "CVodeInit");
        check(CVodeSVtolerances(mem, rtol, av), "CVodeSVtolerances");
        check(CVodeSetUserData(mem, u as *mut c_void), "user data");
        let a = SUNDenseMatrix(2, 2, ctx);
        let ls = SUNLinSol_Dense(y, a, ctx);
        check(CVodeSetLinearSolver(mem, ls, a), "CVodeSetLinearSolver");
        check(CVodeSetJacFn(mem, Some(cv_jac)), "CVodeSetJacFn");
        check(CVodeSetMaxNumSteps(mem, 1_000_000), "max steps");
        check(CVodeRootInit(mem, 1, Some(cv_root)), "CVodeRootInit");
        check(CVodeSetStopTime(mem, T_END), "stop time");

        let mut st = RunStats::default();
        let times = sample_times();
        let mut k = 0;
        let mut t = 0.0;
        let mut tot = Totals::default();
        while k < times.len() {
            let flag = CVode(mem, times[k], y, &mut t, CV_NORMAL);
            check(flag, "CVode");
            if flag == CV_ROOT_RETURN {
                st.t_event = t;
                tot.add_cv(mem);
                (*u).s = 1.0;
                check(CVodeReInit(mem, t, y), "CVodeReInit");
                check(CVodeRootInit(mem, 0, None), "CVodeRootInit off");
                check(CVodeSetStopTime(mem, T_END), "stop time");
                continue;
            }
            let x = slice(y, 2);
            st.samples.push([x[0], x[1], current(x[0], x[1])]);
            k += 1;
        }
        tot.add_cv(mem);
        st.steps = tot.steps;
        st.rhs_evals = tot.rhs;
        st.jac_evals = tot.jac;
        st.err_test_fails = tot.etf;

        CVodeFree(&mut mem);
        SUNLinSolFree(ls);
        SUNMatDestroy(a);
        N_VDestroy(y);
        N_VDestroy(av);
        SUNContext_Free(&mut ctx);
        drop(Box::from_raw(u));
        st
    }
}

#[derive(Default)]
struct Totals {
    steps: u64,
    rhs: u64,
    jac: u64,
    etf: u64,
}

impl Totals {
    unsafe fn add_cv(&mut self, mem: *mut c_void) {
        let mut n: c_long = 0;
        CVodeGetNumSteps(mem, &mut n);
        self.steps += n as u64;
        CVodeGetNumRhsEvals(mem, &mut n);
        self.rhs += n as u64;
        CVodeGetNumJacEvals(mem, &mut n);
        self.jac += n as u64;
        CVodeGetNumErrTestFails(mem, &mut n);
        self.etf += n as u64;
    }
    unsafe fn add_ida(&mut self, mem: *mut c_void) {
        let mut n: c_long = 0;
        IDAGetNumSteps(mem, &mut n);
        self.steps += n as u64;
        IDAGetNumResEvals(mem, &mut n);
        self.rhs += n as u64;
        IDAGetNumJacEvals(mem, &mut n);
        self.jac += n as u64;
        IDAGetNumErrTestFails(mem, &mut n);
        self.etf += n as u64;
    }
}

// ---------- IDA (DAE in v1, w, i) ----------

unsafe extern "C" fn ida_res(
    _t: f64,
    yy: N_Vector,
    yp: N_Vector,
    rr: N_Vector,
    d: *mut c_void,
) -> c_int {
    let y = slice(yy, 3);
    let p = slice(yp, 3);
    let r = slice(rr, 3);
    let s = user(d).s;
    r[0] = C1 * p[0] - (y[2] - y[0] / R1);
    r[1] = J * p[1] - (K * y[2] - B * y[1] - TB * s);
    r[2] = E - R0 * y[2] - y[0] - K * y[1];
    0
}

#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn ida_jac(
    _t: f64,
    cj: f64,
    _yy: N_Vector,
    _yp: N_Vector,
    _rr: N_Vector,
    jm: SUNMatrix,
    _d: *mut c_void,
    _t1: N_Vector,
    _t2: N_Vector,
    _t3: N_Vector,
) -> c_int {
    let m = [
        [1.0 / R1 + cj * C1, 0.0, -1.0],
        [0.0, B + cj * J, -K],
        [-1.0, -K, -R0],
    ];
    for j in 0..3 {
        let col = SUNDenseMatrix_Column(jm, j as sunindextype);
        for i in 0..3 {
            *col.add(i) = m[i][j];
        }
    }
    0
}

unsafe extern "C" fn ida_root(
    _t: f64,
    yy: N_Vector,
    _yp: N_Vector,
    g: *mut f64,
    _d: *mut c_void,
) -> c_int {
    *g = slice(yy, 3)[1] - W_ON;
    0
}

pub fn ida(rtol: f64, atol: [f64; 3]) -> RunStats {
    unsafe {
        let mut ctx: SUNContext = ptr::null_mut();
        check(
            SUNContext_Create(SUN_COMM_NULL, &mut ctx),
            "SUNContext_Create",
        );
        let yy = N_VNew_Serial(3, ctx);
        let yp = N_VNew_Serial(3, ctx);
        let id = N_VNew_Serial(3, ctx);
        let av = N_VNew_Serial(3, ctx);
        // the current starts at a wrong guess (0 A): IDACalcIC must find 8000 A
        slice(yy, 3).copy_from_slice(&[0.0, 0.0, 0.0]);
        slice(yp, 3).copy_from_slice(&[0.0, 0.0, 0.0]);
        slice(id, 3).copy_from_slice(&[1.0, 1.0, 0.0]);
        slice(av, 3).copy_from_slice(&atol);
        let mut mem = IDACreate(ctx);
        let u: *mut User = Box::into_raw(Box::new(User { s: 0.0 }));
        check(IDAInit(mem, Some(ida_res), 0.0, yy, yp), "IDAInit");
        check(IDASVtolerances(mem, rtol, av), "IDASVtolerances");
        check(IDASetUserData(mem, u as *mut c_void), "user data");
        let a = SUNDenseMatrix(3, 3, ctx);
        let ls = SUNLinSol_Dense(yy, a, ctx);
        check(IDASetLinearSolver(mem, ls, a), "IDASetLinearSolver");
        check(IDASetJacFn(mem, Some(ida_jac)), "IDASetJacFn");
        check(IDASetId(mem, id), "IDASetId");
        check(IDASetMaxNumSteps(mem, 1_000_000), "max steps");
        check(IDARootInit(mem, 1, Some(ida_root)), "IDARootInit");
        check(IDASetStopTime(mem, T_END), "stop time");
        check(IDACalcIC(mem, IDA_YA_YDP_INIT, 0.01), "IDACalcIC");
        check(IDAGetConsistentIC(mem, yy, yp), "IDAGetConsistentIC");

        let mut st = RunStats::default();
        let times = sample_times();
        let mut k = 0;
        let mut t = 0.0;
        let mut tot = Totals::default();
        while k < times.len() {
            let flag = IDASolve(mem, times[k], &mut t, yy, yp, IDA_NORMAL);
            check(flag, "IDASolve");
            if flag == IDA_ROOT_RETURN {
                st.t_event = t;
                tot.add_ida(mem);
                (*u).s = 1.0;
                check(IDAReInit(mem, t, yy, yp), "IDAReInit");
                check(IDARootInit(mem, 0, None), "IDARootInit off");
                check(IDASetStopTime(mem, T_END), "stop time");
                check(
                    IDACalcIC(mem, IDA_YA_YDP_INIT, t + 0.01),
                    "IDACalcIC after event",
                );
                continue;
            }
            let y = slice(yy, 3);
            st.samples.push([y[0], y[1], y[2]]);
            k += 1;
        }
        tot.add_ida(mem);
        st.steps = tot.steps;
        st.rhs_evals = tot.rhs;
        st.jac_evals = tot.jac;
        st.err_test_fails = tot.etf;

        IDAFree(&mut mem);
        SUNLinSolFree(ls);
        SUNMatDestroy(a);
        for v in [yy, yp, id, av] {
            N_VDestroy(v);
        }
        SUNContext_Free(&mut ctx);
        drop(Box::from_raw(u));
        st
    }
}
