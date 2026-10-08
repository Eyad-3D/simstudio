//! Scaling check: a torsional chain of N inertias joined by stiff
//! spring-dampers (a driveline), driven by a constant torque at one end and
//! a viscous load at the other. n = 2N - 1 states, stiff and oscillatory
//! (sqrt(k/J) = 1000 rad/s, damping 500 1/s), no events. Every backend
//! solves it to t = 10 s; their answers are compared with each other at a
//! tight tolerance (no closed form is used here).

pub const JI: f64 = 0.1;
pub const KS: f64 = 1.0e5;
pub const CD: f64 = 50.0;
pub const BL: f64 = 1.0;
pub const TQ: f64 = 100.0;
pub const T_END: f64 = 10.0;

/// y = [w_1..w_N, th_1..th_{N-1}]
pub fn rhs(n: usize, y: &[f64], f: &mut [f64], forcing: bool) {
    let w = &y[..n];
    let th = &y[n..];
    for k in 0..n {
        let mut t = 0.0;
        if k == 0 && forcing {
            t += TQ;
        }
        if k > 0 {
            t += KS * th[k - 1] + CD * (w[k - 1] - w[k]);
        }
        if k + 1 < n {
            t -= KS * th[k] + CD * (w[k] - w[k + 1]);
        }
        if k + 1 == n {
            t -= BL * w[k];
        }
        f[k] = t / JI;
    }
    for k in 0..n - 1 {
        f[n + k] = w[k] - w[k + 1];
    }
}

pub fn jac_dense(n: usize) -> Vec<Vec<f64>> {
    let m = 2 * n - 1;
    let mut a = vec![vec![0.0; m]; m];
    let mut e = vec![0.0; m];
    let mut f = vec![0.0; m];
    for j in 0..m {
        e.fill(0.0);
        e[j] = 1.0;
        rhs(n, &e, &mut f, false);
        for i in 0..m {
            a[i][j] = f[i];
        }
    }
    a
}

#[cfg(feature = "sundials")]
pub mod sun {
    use super::*;
    use std::ffi::c_void;
    use std::os::raw::{c_int, c_long};
    use std::ptr;
    use sundials_sys::*;

    struct U {
        n: usize,
        jac: Vec<Vec<f64>>,
    }

    unsafe extern "C" fn f(_t: f64, y: N_Vector, yd: N_Vector, d: *mut c_void) -> c_int {
        let u = &*(d as *const U);
        let m = 2 * u.n - 1;
        let ys = std::slice::from_raw_parts(N_VGetArrayPointer(y), m);
        let fs = std::slice::from_raw_parts_mut(N_VGetArrayPointer(yd), m);
        rhs(u.n, ys, fs, true);
        0
    }

    unsafe extern "C" fn jac(
        _t: f64,
        _y: N_Vector,
        _fy: N_Vector,
        jm: SUNMatrix,
        d: *mut c_void,
        _a: N_Vector,
        _b: N_Vector,
        _c: N_Vector,
    ) -> c_int {
        let u = &*(d as *const U);
        let m = 2 * u.n - 1;
        for j in 0..m {
            let col = SUNDenseMatrix_Column(jm, j as sunindextype);
            for i in 0..m {
                *col.add(i) = u.jac[i][j];
            }
        }
        0
    }

    /// interleaved index of w_k and th_k: z = [w1, th1, w2, th2, ..., wN]
    fn perm(n: usize) -> Vec<usize> {
        // perm[i] = position in z of y-index i
        let mut p = vec![0; 2 * n - 1];
        for k in 0..n {
            p[k] = 2 * k;
        }
        for k in 0..n - 1 {
            p[n + k] = 2 * k + 1;
        }
        p
    }

    struct UB {
        n: usize,
        p: Vec<usize>,
        y: Vec<f64>,
        f: Vec<f64>,
        jac: Vec<Vec<f64>>,
    }

    unsafe extern "C" fn fb(_t: f64, z: N_Vector, zd: N_Vector, d: *mut c_void) -> c_int {
        let u = &mut *(d as *mut UB);
        let m = 2 * u.n - 1;
        let zs = std::slice::from_raw_parts(N_VGetArrayPointer(z), m);
        let fs = std::slice::from_raw_parts_mut(N_VGetArrayPointer(zd), m);
        for i in 0..m {
            u.y[i] = zs[u.p[i]];
        }
        rhs(u.n, &u.y, &mut u.f, true);
        for i in 0..m {
            fs[u.p[i]] = u.f[i];
        }
        0
    }

    unsafe extern "C" fn jacb(
        _t: f64,
        _y: N_Vector,
        _fy: N_Vector,
        jm: SUNMatrix,
        d: *mut c_void,
        _a: N_Vector,
        _b: N_Vector,
        _c: N_Vector,
    ) -> c_int {
        let u = &*(d as *const UB);
        let m = 2 * u.n - 1;
        for j in 0..m {
            let col = SUNBandMatrix_Column(jm, u.p[j] as sunindextype);
            for i in 0..m {
                let v = u.jac[i][j];
                if v != 0.0 {
                    // SM_COLUMN_ELEMENT_B(col, i, j) = col[i - j]
                    let off = u.p[i] as isize - u.p[j] as isize;
                    *col.offset(off) = v;
                }
            }
        }
        0
    }

    /// CVODE with SUNDIALS' band LU (bandwidth 2 in the interleaved order):
    /// what CVODE costs with a structure-exploiting linear solver.
    pub fn cvode_band(n: usize, rtol: f64) -> (Vec<f64>, u64, u64) {
        let m = 2 * n - 1;
        unsafe {
            let u = Box::into_raw(Box::new(UB {
                n,
                p: perm(n),
                y: vec![0.0; m],
                f: vec![0.0; m],
                jac: jac_dense(n),
            }));
            let mut ctx: SUNContext = ptr::null_mut();
            SUNContext_Create(SUN_COMM_NULL, &mut ctx);
            let y = N_VNew_Serial(m as sunindextype, ctx);
            std::slice::from_raw_parts_mut(N_VGetArrayPointer(y), m).fill(0.0);
            let mut mem = CVodeCreate(CV_BDF, ctx);
            CVodeInit(mem, Some(fb), 0.0, y);
            CVodeSStolerances(mem, rtol, rtol);
            CVodeSetUserData(mem, u as *mut c_void);
            let a = SUNBandMatrix(m as sunindextype, 2, 2, ctx);
            let ls = SUNLinSol_Band(y, a, ctx);
            CVodeSetLinearSolver(mem, ls, a);
            CVodeSetJacFn(mem, Some(jacb));
            CVodeSetMaxNumSteps(mem, 10_000_000);
            let mut t = 0.0;
            let flag = CVode(mem, T_END, y, &mut t, CV_NORMAL);
            assert!(flag >= 0);
            let zs = std::slice::from_raw_parts(N_VGetArrayPointer(y), m);
            let p = perm(n);
            let out: Vec<f64> = (0..m).map(|i| zs[p[i]]).collect();
            let (mut ns, mut nj): (c_long, c_long) = (0, 0);
            CVodeGetNumSteps(mem, &mut ns);
            CVodeGetNumLinSolvSetups(mem, &mut nj);
            CVodeFree(&mut mem);
            SUNLinSolFree(ls);
            SUNMatDestroy(a);
            N_VDestroy(y);
            SUNContext_Free(&mut ctx);
            drop(Box::from_raw(u));
            (out, ns as u64, nj as u64)
        }
    }

    /// returns (y(T_END), steps, jac setups)
    pub fn cvode(n: usize, rtol: f64) -> (Vec<f64>, u64, u64) {
        let m = 2 * n - 1;
        unsafe {
            let u = Box::into_raw(Box::new(U {
                n,
                jac: jac_dense(n),
            }));
            let mut ctx: SUNContext = ptr::null_mut();
            SUNContext_Create(SUN_COMM_NULL, &mut ctx);
            let y = N_VNew_Serial(m as sunindextype, ctx);
            std::slice::from_raw_parts_mut(N_VGetArrayPointer(y), m).fill(0.0);
            let mut mem = CVodeCreate(CV_BDF, ctx);
            CVodeInit(mem, Some(f), 0.0, y);
            CVodeSStolerances(mem, rtol, rtol);
            CVodeSetUserData(mem, u as *mut c_void);
            let a = SUNDenseMatrix(m as sunindextype, m as sunindextype, ctx);
            let ls = SUNLinSol_Dense(y, a, ctx);
            CVodeSetLinearSolver(mem, ls, a);
            CVodeSetJacFn(mem, Some(jac));
            CVodeSetMaxNumSteps(mem, 10_000_000);
            let mut t = 0.0;
            let flag = CVode(mem, T_END, y, &mut t, CV_NORMAL);
            assert!(flag >= 0);
            let out = std::slice::from_raw_parts(N_VGetArrayPointer(y), m).to_vec();
            let (mut ns, mut nj): (c_long, c_long) = (0, 0);
            CVodeGetNumSteps(mem, &mut ns);
            CVodeGetNumLinSolvSetups(mem, &mut nj);
            CVodeFree(&mut mem);
            SUNLinSolFree(ls);
            SUNMatDestroy(a);
            N_VDestroy(y);
            SUNContext_Free(&mut ctx);
            drop(Box::from_raw(u));
            (out, ns as u64, nj as u64)
        }
    }
}

#[cfg(feature = "diffsol")]
pub mod dsol {
    use super::*;
    use diffsol::{
        FaerSparseLU, FaerSparseMat, NalgebraLU, NalgebraMat, OdeBuilder, OdeSolverMethod,
        OdeSolverStopReason, Vector,
    };

    macro_rules! run {
        ($name:ident, $mat:ty, $ls:ty) => {
            pub fn $name(n: usize, rtol: f64) -> (Vec<f64>, u64, u64) {
                let m = 2 * n - 1;
                let mut problem = OdeBuilder::<$mat>::new()
                    .rtol(rtol)
                    .atol([rtol])
                    .rhs_implicit(
                        move |y: &[f64], _p: &[f64], _t: f64, f: &mut [f64]| rhs(n, y, f, true),
                        move |_y: &[f64], _p: &[f64], _t: f64, v: &[f64], f: &mut [f64]| {
                            rhs(n, v, f, false)
                        },
                    )
                    .init(move |_p: &[f64], _t: f64, y: &mut [f64]| y.fill(0.0), m)
                    .build()
                    .expect("build");
                problem.ode_options.min_timestep = 1e-20;
                let mut s = problem.bdf::<$ls>().expect("bdf");
                s.set_stop_time(T_END).expect("tstop");
                while !matches!(s.step().expect("step"), OdeSolverStopReason::TstopReached) {}
                let st = s.state();
                let out: Vec<f64> = (0..m).map(|i| st.y.get_index(i)).collect();
                let stats = s.get_statistics();
                (
                    out,
                    stats.number_of_steps as u64,
                    stats.number_of_linear_solver_setups as u64,
                )
            }
        };
    }
    run!(bdf_nalgebra, NalgebraMat<f64>, NalgebraLU<f64>);
    run!(bdf_faer_sparse, FaerSparseMat<f64>, FaerSparseLU<f64>);
}
