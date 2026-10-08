//! A sparse direct linear solver for SUNDIALS on faer's sparse LU (MIT):
//! the role KLU plays in SUNDIALS' own distribution, which LightSim cannot
//! use (KLU is LGPL).
//!
//! It is a `SUNLinearSolver` of type "direct" working on SUNDIALS' own
//! compressed-sparse-column matrix (`SUNSparseMatrix`): the symbolic
//! analysis (fill-reducing column ordering, elimination structure) runs
//! once per structure and again only if the structure changes; each setup
//! refactorises numerically with partial pivoting; each solve is two
//! triangular solves. Everything runs on the calling thread (`Par::Seq`),
//! so parallel sweeps do not contend.

use faer::dyn_stack::{MemBuffer, MemStack, StackReq};
use faer::sparse::linalg::lu::{LuRef, NumericLu, SymbolicLu, factorize_symbolic_lu};
use faer::sparse::{SparseColMatRef, SymbolicSparseColMatRef};
use faer::{Conj, MatMut, Par};
use lsim_sundials_sys::*;
use std::ffi::c_void;
use std::os::raw::{c_int, c_long};

/// Counts kept by the solver, for the run report.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SparseLuStats {
    /// symbolic analyses (once per structure)
    pub symbolic: u64,
    /// numeric factorisations
    pub numeric: u64,
    /// solves
    pub solves: u64,
}

struct Content {
    col_ptr: Vec<u64>,
    row_idx: Vec<u64>,
    symbolic: Option<SymbolicLu<u64>>,
    numeric: NumericLu<u64, f64>,
    buf: MemBuffer,
    buf_req: StackReq,
    last_flag: sunindextype,
    stats: SparseLuStats,
}

/// # Safety
/// `s` must be a solver made by [`new`].
unsafe fn content<'a>(s: SUNLinearSolver) -> &'a mut Content {
    // SAFETY: `content` is the Box<Content> leaked in `new`.
    unsafe { &mut *((*s).content as *mut Content) }
}

unsafe extern "C" fn gettype(_: SUNLinearSolver) -> SUNLinearSolver_Type {
    SUNLinearSolver_Type_SUNLINEARSOLVER_DIRECT
}

unsafe extern "C" fn getid(_: SUNLinearSolver) -> SUNLinearSolver_ID {
    SUNLinearSolver_ID_SUNLINEARSOLVER_CUSTOM
}

unsafe extern "C" fn initialize(s: SUNLinearSolver) -> SUNErrCode {
    // SAFETY: `s` is ours.
    unsafe { content(s).last_flag = 0 };
    0
}

/// # Safety
/// `a` must be a CSC `SUNSparseMatrix`.
unsafe fn csc<'a>(a: SUNMatrix) -> (usize, &'a [u64], &'a [u64], &'a [f64]) {
    // SAFETY: by the caller's contract; indices are non-negative, so the
    // i64 arrays read as u64.
    unsafe {
        let n = SUNSparseMatrix_Columns(a) as usize;
        let ptr = std::slice::from_raw_parts(SUNSparseMatrix_IndexPointers(a) as *const u64, n + 1);
        let nnz = ptr[n] as usize;
        let rows = std::slice::from_raw_parts(SUNSparseMatrix_IndexValues(a) as *const u64, nnz);
        let vals = std::slice::from_raw_parts(SUNSparseMatrix_Data(a), nnz);
        (n, ptr, rows, vals)
    }
}

unsafe extern "C" fn setup(s: SUNLinearSolver, a: SUNMatrix) -> c_int {
    // SAFETY: `s` is ours, `a` the CSC matrix SUNDIALS formed.
    let (c, (n, ptr, rows, vals)) = unsafe { (content(s), csc(a)) };
    if c.symbolic.is_none() || c.col_ptr != ptr || c.row_idx != rows {
        c.col_ptr = ptr.to_vec();
        c.row_idx = rows.to_vec();
        // SAFETY: SUNDIALS keeps CSC columns sorted and within bounds.
        let sym = unsafe { SymbolicSparseColMatRef::new_unchecked(n, n, ptr, None, rows) };
        match factorize_symbolic_lu(sym, Default::default()) {
            Ok(f) => {
                let req = f
                    .factorize_numeric_lu_scratch::<f64>(Par::Seq, Default::default())
                    .or(f.solve_in_place_scratch::<f64>(1, Par::Seq));
                if c.buf_req != req {
                    c.buf = MemBuffer::new(req);
                    c.buf_req = req;
                }
                c.symbolic = Some(f);
                c.stats.symbolic += 1;
            }
            Err(_) => {
                c.last_flag = -1;
                return SUNLS_PACKAGE_FAIL_REC;
            }
        }
    }
    let sym = c.symbolic.as_ref().expect("analysed above");
    // SAFETY: as above.
    let mat = SparseColMatRef::new(
        unsafe { SymbolicSparseColMatRef::new_unchecked(n, n, ptr, None, rows) },
        vals,
    );
    c.stats.numeric += 1;
    match sym.factorize_numeric_lu(
        &mut c.numeric,
        mat,
        Par::Seq,
        MemStack::new(&mut c.buf),
        Default::default(),
    ) {
        Ok(_) => {
            c.last_flag = 0;
            0
        }
        // a recoverable failure: the integrator retries with a smaller step
        Err(_) => {
            c.last_flag = 1;
            SUNLS_LUFACT_FAIL
        }
    }
}

unsafe extern "C" fn solve(
    s: SUNLinearSolver,
    _a: SUNMatrix,
    x: N_Vector,
    b: N_Vector,
    _tol: sunrealtype,
) -> c_int {
    // SAFETY: `s` is ours; x and b are serial vectors of n values.
    unsafe {
        let c = content(s);
        let Some(sym) = c.symbolic.as_ref() else {
            return SUNLS_PACKAGE_FAIL_REC;
        };
        let n = c.col_ptr.len() - 1;
        N_VScale(1.0, b, x);
        let xs = std::slice::from_raw_parts_mut(N_VGetArrayPointer(x), n);
        LuRef::new_unchecked(sym, &c.numeric).solve_in_place_with_conj(
            Conj::No,
            MatMut::from_column_major_slice_mut(xs, n, 1),
            Par::Seq,
            MemStack::new(&mut c.buf),
        );
        c.stats.solves += 1;
        if xs.iter().all(|v| v.is_finite()) {
            c.last_flag = 0;
            0
        } else {
            // a numerically singular matrix: let the integrator cut the step
            c.last_flag = 2;
            SUNLS_PACKAGE_FAIL_REC
        }
    }
}

unsafe extern "C" fn lastflag(s: SUNLinearSolver) -> sunindextype {
    // SAFETY: `s` is ours.
    unsafe { content(s).last_flag }
}

unsafe extern "C" fn space(_: SUNLinearSolver, lrw: *mut c_long, liw: *mut c_long) -> SUNErrCode {
    // SAFETY: SUNDIALS passes valid pointers.
    unsafe {
        *lrw = 0;
        *liw = 0;
    }
    0
}

unsafe extern "C" fn free(s: SUNLinearSolver) -> SUNErrCode {
    if s.is_null() {
        return 0;
    }
    // SAFETY: frees the content leaked in `new`, then the shell.
    unsafe {
        if !(*s).content.is_null() {
            drop(Box::from_raw((*s).content as *mut Content));
            (*s).content = std::ptr::null_mut();
        }
        SUNLinSolFreeEmpty(s);
    }
    0
}

/// A new faer sparse LU solver in context `ctx`.
///
/// # Safety
/// `ctx` must be a live SUNDIALS context; free the solver with
/// `SUNLinSolFree` before the context.
pub unsafe fn new(ctx: SUNContext) -> SUNLinearSolver {
    // SAFETY: SUNLinSolNewEmpty allocates the shell and its ops table.
    unsafe {
        let s = SUNLinSolNewEmpty(ctx);
        let ops = &mut *(*s).ops;
        ops.gettype = Some(gettype);
        ops.getid = Some(getid);
        ops.initialize = Some(initialize);
        ops.setup = Some(setup);
        ops.solve = Some(solve);
        ops.lastflag = Some(lastflag);
        ops.space = Some(space);
        ops.free = Some(free);
        let c = Box::new(Content {
            col_ptr: vec![],
            row_idx: vec![],
            symbolic: None,
            numeric: NumericLu::new(),
            buf: MemBuffer::new(StackReq::EMPTY),
            buf_req: StackReq::EMPTY,
            last_flag: 0,
            stats: SparseLuStats::default(),
        });
        (*s).content = Box::into_raw(c) as *mut c_void;
        s
    }
}

/// The solver's counters.
///
/// # Safety
/// `s` must be a solver made by [`new`].
pub unsafe fn stats(s: SUNLinearSolver) -> SparseLuStats {
    // SAFETY: by the caller's contract.
    unsafe { content(s).stats }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solves_a_sparse_system_like_a_dense_lu() {
        // SAFETY: SUNDIALS objects created and freed here.
        unsafe {
            let mut ctx: SUNContext = std::ptr::null_mut();
            assert_eq!(SUNContext_Create(SUN_COMM_NULL, &mut ctx), 0);
            let n = 5usize;
            // a non-symmetric matrix needing pivoting: zero diagonal at 0
            let dense = [
                [0.0, 2.0, 0.0, 0.0, 1.0],
                [3.0, 1.0, 0.0, 0.0, 0.0],
                [0.0, 0.0, 4.0, 1.0, 0.0],
                [0.0, 1.0, 0.0, 5.0, 0.0],
                [1.0, 0.0, 2.0, 0.0, 6.0],
            ];
            let a = SUNSparseMatrix(n as i64, n as i64, 25, CSC_MAT, ctx);
            let (ptr, idx, val) = (
                SUNSparseMatrix_IndexPointers(a),
                SUNSparseMatrix_IndexValues(a),
                SUNSparseMatrix_Data(a),
            );
            let mut k = 0;
            for j in 0..n {
                *ptr.add(j) = k as i64;
                for (i, row) in dense.iter().enumerate() {
                    if row[j] != 0.0 {
                        *idx.add(k) = i as i64;
                        *val.add(k) = row[j];
                        k += 1;
                    }
                }
            }
            *ptr.add(n) = k as i64;
            let want = [1.0, -2.0, 3.0, 0.5, -1.0];
            let b = N_VNew_Serial(n as i64, ctx);
            let x = N_VNew_Serial(n as i64, ctx);
            for (i, row) in dense.iter().enumerate() {
                *N_VGetArrayPointer(b).add(i) = row.iter().zip(&want).map(|(a, w)| a * w).sum();
            }
            let ls = new(ctx);
            assert_eq!(SUNLinSolInitialize(ls), 0);
            for _ in 0..2 {
                assert_eq!(SUNLinSolSetup(ls, a), 0);
                assert_eq!(SUNLinSolSolve(ls, a, x, b, 0.0), 0);
                for (i, w) in want.iter().enumerate() {
                    assert!((*N_VGetArrayPointer(x).add(i) - w).abs() < 1e-14);
                }
            }
            let st = stats(ls);
            assert_eq!((st.symbolic, st.numeric, st.solves), (1, 2, 2));
            SUNLinSolFree(ls);
            SUNMatDestroy(a);
            N_VDestroy(b);
            N_VDestroy(x);
            SUNContext_Free(&mut ctx);
        }
    }
}
