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
        newton(assign, iter, resid, vals, params, t, block.linear).map_err(
            |(worst_eq, worst, singular)| NotSolved { block: b, worst_eq, worst, singular },
        )?;
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
    linear: bool,
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
    for it in 0..100 {
        let fn0 = norm(&f);
        if fn0.is_nan() {
            return Err(worst(&f, false));
        }
        // a linear block's Jacobian is checked even when its residuals
        // already vanish: a singular one (a circuit with no ground) leaves
        // the block's unknowns undecided whatever their values
        let check = linear && it == 0;
        if fn0 == 0.0 && !check {
            return Ok(());
        }
        // Jacobian by forward differences; a linear block's residuals are
        // affine in its unknowns, so a large step is exact and keeps the
        // rounding small enough to tell a singular Jacobian
        for j in 0..n {
            let x = vals[iter[j]];
            let h = if linear { x.abs().max(1.0) } else { 1e-7 * x.abs().max(1e-3) };
            vals[iter[j]] = x + h;
            residuals(assign, resid, vals, params, t, &mut f2);
            vals[iter[j]] = x;
            for i in 0..n {
                jac[i * n + j] = (f2[i] - f[i]) / h;
            }
        }
        if check && rank(&mut jac.clone(), n, n, 1e-11).0 < n {
            return Err(worst(&f, true));
        }
        if fn0 == 0.0 {
            return Ok(());
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

/// A direction along which the affine residuals `rows` do not change (a
/// null vector of their Jacobian in the unknowns `nodes`), when that
/// Jacobian is singular; scaled so its largest entry is 1. For a linear
/// block it says what the equations leave undecided: all the potentials of
/// a circuit moving together is a circuit with no ground.
pub fn null_direction(
    rows: &[&Expr],
    nodes: &[usize],
    vals: &mut [f64],
    params: &[f64],
    t: f64,
) -> Option<Vec<f64>> {
    let (m, n) = (rows.len(), nodes.len());
    if n == 0 || n > MAX_ITERATION {
        return None;
    }
    for &x in nodes {
        if !vals[x].is_finite() {
            vals[x] = 0.0;
        }
    }
    let f0: Vec<f64> = rows.iter().map(|e| eval(e, &NodeEnv { t, vals, params })).collect();
    let mut a = vec![0.0; m * n];
    for j in 0..n {
        let x = vals[nodes[j]];
        // affine residuals: a large step is exact
        let h = x.abs().max(1.0);
        vals[nodes[j]] = x + h;
        for i in 0..m {
            a[i * n + j] = (eval(rows[i], &NodeEnv { t, vals, params }) - f0[i]) / h;
        }
        vals[nodes[j]] = x;
    }
    if a.iter().any(|v| !v.is_finite()) {
        return None;
    }
    let (rank, col) = rank(&mut a, m, n, 1e-11);
    if rank == n {
        return None;
    }
    // the first free unknown set to 1, the other free ones to 0
    let mut y = vec![0.0; n];
    y[rank] = 1.0;
    for k in (0..rank).rev() {
        let s: f64 = (k + 1..n).map(|j| a[k * n + j] * y[j]).sum();
        y[k] = -s / a[k * n + k];
    }
    let mut x = vec![0.0; n];
    for j in 0..n {
        x[col[j]] = y[j];
    }
    let big = x.iter().fold(0.0f64, |s, v| s.max(v.abs()));
    Some(x.iter().map(|v| v / big).collect())
}

/// Gaussian elimination with complete pivoting on the row-scaled `m × n`
/// matrix `a` (row-major, overwritten by its upper triangular factor in
/// permuted columns): the rank (pivots above `tol`, relative to each row's
/// largest entry) and the column permutation. A non-finite matrix has
/// rank 0.
fn rank(a: &mut [f64], m: usize, n: usize, tol: f64) -> (usize, Vec<usize>) {
    let col: Vec<usize> = (0..n).collect();
    if a.iter().any(|v| !v.is_finite()) {
        return (0, col);
    }
    let mut col = col;
    for i in 0..m {
        let big = a[i * n..(i + 1) * n].iter().fold(0.0f64, |s, v| s.max(v.abs()));
        if big > 0.0 {
            a[i * n..(i + 1) * n].iter_mut().for_each(|v| *v /= big);
        }
    }
    let mut r = 0;
    while r < m.min(n) {
        let k = r;
        let (mut pi, mut pj, mut best) = (k, k, 0.0);
        for i in k..m {
            for j in k..n {
                if a[i * n + j].abs() > best {
                    (pi, pj, best) = (i, j, a[i * n + j].abs());
                }
            }
        }
        if best <= tol {
            break;
        }
        for j in 0..n {
            a.swap(pi * n + j, k * n + j);
        }
        for i in 0..m {
            a.swap(i * n + pj, i * n + k);
        }
        col.swap(pj, k);
        for i in k + 1..m {
            let f = a[i * n + k] / a[k * n + k];
            if f != 0.0 {
                for j in k..n {
                    a[i * n + j] -= f * a[k * n + j];
                }
            }
        }
        r += 1;
    }
    (r, col)
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
    fn the_null_direction_of_a_floating_pair() {
        // v1 - v2 = 12 twice: the pair can move together
        use lsim_ir::flat::VarId;
        let d = Expr::Var(VarId(0)) - Expr::Var(VarId(1)) - Expr::Const(12.0);
        let mut vals = vec![0.0, 0.0];
        let x = null_direction(&[&d, &d], &[0, 1], &mut vals, &[], 0.0).unwrap();
        assert!((x[0] - x[1]).abs() < 1e-9 && (x[0].abs() - 1.0).abs() < 1e-12, "{x:?}");
        let e = Expr::Var(VarId(0)) + Expr::Var(VarId(1));
        assert!(null_direction(&[&d, &e], &[0, 1], &mut vals, &[], 0.0).is_none());
    }

    #[test]
    fn lu_solves_with_pivoting() {
        let mut a = vec![0.0, 2.0, 1.0, 1.0];
        let mut b = vec![4.0, 3.0];
        assert!(lu_solve(&mut a, &mut b, 2));
        assert!((b[0] - 1.0).abs() < 1e-15 && (b[1] - 2.0).abs() < 1e-15, "{b:?}");
    }
}
