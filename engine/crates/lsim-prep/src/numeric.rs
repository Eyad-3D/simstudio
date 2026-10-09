//! A small numerical solver for sorted systems at preparation time, on the
//! reference interpreter: block by block, explicit assignments evaluated,
//! torn blocks solved by damped Newton with a finite-difference Jacobian.
//!
//! Preparation uses it for the initial point with the parameter values at
//! hand: to choose dummy derivatives with a pivoting check at the start,
//! to check start values that index reduction makes redundant, to warn
//! about pivots that are zero at the start, and to give the modes their
//! first values. The run solves the initialisation system again with its
//! own parameter values (work package 4); this solver is not on that path.

use crate::causal::Sorted;
use crate::system::NodeEnv;
use lsim_ir::eval::eval;
use lsim_ir::expr::Expr;

/// Why a block could not be solved.
#[derive(Clone, Debug)]
pub struct NotSolved {
    /// the block's index in [`Sorted::blocks`]
    pub block: usize,
    /// the equation with the largest residual left
    pub worst_eq: usize,
    /// that residual
    pub worst: f64,
    /// the Jacobian was singular (as opposed to Newton not converging)
    pub singular: bool,
}

/// The largest torn block solved here (iteration variables).
const MAX_ITERATION: usize = 400;

/// Solves `sorted` (residuals in `eqs`) for the node values in `vals`,
/// which holds the known values (discrete, inputs) and first guesses for
/// the iteration variables on entry.
pub fn solve(sorted: &Sorted, vals: &mut [f64], params: &[f64], t: f64) -> Result<(), NotSolved> {
    for (b, block) in sorted.blocks.iter().enumerate() {
        let assign = &sorted.assignments[block.assign.0..block.assign.1];
        let iter = &sorted.iteration[block.iter.0..block.iter.1];
        let resid = &sorted.residuals[block.iter.0..block.iter.1];
        if iter.is_empty() {
            evaluate(assign, vals, params, t);
            continue;
        }
        if iter.len() > MAX_ITERATION {
            return Err(NotSolved {
                block: b,
                worst_eq: resid[0].1,
                worst: f64::NAN,
                singular: false,
            });
        }
        newton(assign, iter, resid, vals, params, t).map_err(|(worst_eq, worst, singular)| {
            NotSolved { block: b, worst_eq, worst, singular }
        })?;
    }
    Ok(())
}

fn evaluate(assign: &[(usize, Expr, usize)], vals: &mut [f64], params: &[f64], t: f64) {
    for (n, e, _) in assign {
        let v = eval(e, &NodeEnv { t, vals, params });
        vals[*n] = v;
    }
}

fn residuals(
    assign: &[(usize, Expr, usize)],
    resid: &[(Expr, usize)],
    vals: &mut [f64],
    params: &[f64],
    t: f64,
    out: &mut [f64],
) {
    evaluate(assign, vals, params, t);
    for (k, (e, _)) in resid.iter().enumerate() {
        out[k] = eval(e, &NodeEnv { t, vals, params });
    }
}

fn norm(x: &[f64]) -> f64 {
    x.iter().fold(0.0f64, |a, v| if v.is_nan() { f64::NAN } else { a.max(v.abs()) })
}

fn newton(
    assign: &[(usize, Expr, usize)],
    iter: &[usize],
    resid: &[(Expr, usize)],
    vals: &mut [f64],
    params: &[f64],
    t: f64,
) -> Result<(), (usize, f64, bool)> {
    let n = iter.len();
    for &x in iter {
        if !vals[x].is_finite() {
            vals[x] = 0.0;
        }
    }
    let mut f = vec![0.0; n];
    let mut f2 = vec![0.0; n];
    let mut jac = vec![0.0; n * n];
    residuals(assign, resid, vals, params, t, &mut f);
    let f_start = norm(&f);
    let worst = |f: &[f64], singular: bool| {
        let k = (0..f.len()).max_by(|&a, &b| f[a].abs().total_cmp(&f[b].abs())).unwrap_or(0);
        (resid[k].1, f[k], singular)
    };
    for _ in 0..100 {
        let fn0 = norm(&f);
        if fn0.is_nan() {
            return Err(worst(&f, false));
        }
        if fn0 == 0.0 {
            return Ok(());
        }
        // Jacobian by forward differences
        for j in 0..n {
            let x = vals[iter[j]];
            let h = 1e-7 * x.abs().max(1e-3);
            vals[iter[j]] = x + h;
            residuals(assign, resid, vals, params, t, &mut f2);
            vals[iter[j]] = x;
            for i in 0..n {
                jac[i * n + j] = (f2[i] - f[i]) / h;
            }
        }
        let mut dx: Vec<f64> = f.iter().map(|v| -v).collect();
        if !lu_solve(&mut jac.clone(), &mut dx, n) {
            return Err(worst(&f, true));
        }
        // damped step: halve until the residual falls
        let x0: Vec<f64> = iter.iter().map(|&i| vals[i]).collect();
        let mut lambda = 1.0;
        loop {
            for j in 0..n {
                vals[iter[j]] = x0[j] + lambda * dx[j];
            }
            residuals(assign, resid, vals, params, t, &mut f2);
            let fn1 = norm(&f2);
            if fn1 < (1.0 - 1e-4 * lambda) * fn0 || lambda < 1e-4 {
                break;
            }
            lambda *= 0.5;
        }
        f.copy_from_slice(&f2);
        let xs = norm(&x0).max(1.0);
        if norm(&dx) * lambda <= 1e-13 * xs {
            break;
        }
    }
    residuals(assign, resid, vals, params, t, &mut f);
    let fin = norm(&f);
    if fin.is_finite() && fin <= 1e-8 * (1.0 + f_start) { Ok(()) } else { Err(worst(&f, false)) }
}

/// Solves `a x = b` in place (row-major `a`, partial pivoting); false
/// when singular.
pub fn lu_solve(a: &mut [f64], b: &mut [f64], n: usize) -> bool {
    for k in 0..n {
        let p = (k..n).max_by(|&i, &j| a[i * n + k].abs().total_cmp(&a[j * n + k].abs()));
        let p = p.expect("rows remain");
        let piv = a[p * n + k];
        if piv == 0.0 || !piv.is_finite() {
            return false;
        }
        if p != k {
            for j in 0..n {
                a.swap(p * n + j, k * n + j);
            }
            b.swap(p, k);
        }
        for i in k + 1..n {
            let f = a[i * n + k] / piv;
            if f != 0.0 {
                for j in k..n {
                    a[i * n + j] -= f * a[k * n + j];
                }
                b[i] -= f * b[k];
            }
        }
    }
    for k in (0..n).rev() {
        let mut s = b[k];
        for j in k + 1..n {
            s -= a[k * n + j] * b[j];
        }
        b[k] = s / a[k * n + k];
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lu_solves_with_pivoting() {
        let mut a = vec![0.0, 2.0, 1.0, 1.0];
        let mut b = vec![4.0, 3.0];
        assert!(lu_solve(&mut a, &mut b, 2));
        assert!((b[0] - 1.0).abs() < 1e-15 && (b[1] - 2.0).abs() < 1e-15, "{b:?}");
    }
}
