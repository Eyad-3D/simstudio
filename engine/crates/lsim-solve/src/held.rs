//! Table reads held in their cells during every step ([`crate::HeldInfo`],
//! `lsim_prep::hold`; DESIGN.md §8.2, *Tables read at a state*): Park and
//! Barton's discontinuity locking. The equations read each such table as
//! its cell's polynomial in powers of `(a − m)`, with discrete coefficients;
//! here the run puts each read in the cell its argument lies in (at the
//! start, and at every event where the argument left its cell, which the
//! cell's edges, `a − lo` and `hi − a`, tell by falling below zero), and
//! sets the coefficients from the table the model interpolates.
//!
//! An axis with points `p₀ < … < p₍ₙ₋₁₎` has the cells 0 (below `p₀`), `i`
//! for `p₍ᵢ₋₁₎ ≤ a < pᵢ` (`i` = 1 … n − 1; the last one closed at
//! `p₍ₙ₋₁₎`, as the table takes the data's last point in its last cell)
//! and `n` (above `p₍ₙ₋₁₎`). A cell's polynomial is written at a point
//! strictly inside it (the middle; outside the data, one cell's width from
//! the edge), where the table's derivatives are those of that cell's
//! polynomial: its Taylor coefficients there are the cell's polynomial,
//! exact to rounding, inside the cell and continued beyond it.

use crate::SolveError;
use crate::info::RunInfo;
use lsim_ir::{EvalInput, ModelFunctions};

/// An open edge of a cell (`lsim_prep::hold::OPEN`).
const OPEN: f64 = 1e300;

/// The cell `a` lies in, among the points `p` (at least two); `merged`:
/// the cells outside the data are the edge cells' own (the table goes on
/// with their polynomials there: no kink at the data's edges).
pub(crate) fn cell_of(p: &[f64], a: f64, merged: bool) -> usize {
    let n = p.len();
    let c = if a < p[0] {
        0
    } else if a >= p[n - 1] {
        if a == p[n - 1] { n - 1 } else { n }
    } else {
        p.partition_point(|&x| x <= a)
    };
    if merged { c.clamp(1, n - 1) } else { c }
}

/// Cell `c`'s edges (`merged` as for [`cell_of`]).
fn edges(p: &[f64], c: usize, merged: bool) -> (f64, f64) {
    let n = p.len();
    let lo = if c == 0 || (merged && c == 1) { -OPEN } else { p[c - 1] };
    let hi = if c >= n || (merged && c == n - 1) { OPEN } else { p[c] };
    (lo, hi)
}

/// Whether held read `h`'s table goes on with its edge cells' polynomials
/// outside its data along `axis` (as the model interpolates it).
fn merged(model: &dyn ModelFunctions, table: u32, axis: usize) -> bool {
    model.table_interp(table).is_some_and(|t| t.continues_cells(axis))
}

/// The point of cell `c` its polynomial is written at.
fn anchor(p: &[f64], c: usize) -> f64 {
    let n = p.len();
    if c == 0 {
        p[0] - (p[1] - p[0])
    } else if c >= n {
        p[n - 1] + (p[n - 1] - p[n - 2])
    } else {
        0.5 * p[c - 1] + 0.5 * p[c]
    }
}

fn factorial(k: u8) -> f64 {
    (1..=u32::from(k)).map(f64::from).product()
}

/// Puts read `k` in the cells `cells` (one per argument): their numbers,
/// points, edges and the polynomial's coefficients, in `d`.
fn set(
    model: &dyn ModelFunctions,
    info: &RunInfo,
    k: usize,
    cells: &[usize],
    t: f64,
    d: &mut [f64],
) -> Result<(), SolveError> {
    let h = &info.held[k];
    let mut at = [0.0; 2];
    let mut width = [0.0; 2];
    for (j, ax) in h.axes.iter().enumerate() {
        let p = info.table_points(h.table, j);
        let (lo, hi) = edges(p, cells[j], merged(model, h.table, j));
        at[j] = anchor(p, cells[j]);
        width[j] = if lo > -OPEN && hi < OPEN { hi - lo } else { (p[1] - p[0]).abs() };
        d[ax.cell] = cells[j] as f64;
        d[ax.anchor] = at[j];
        d[ax.lo] = lo;
        d[ax.hi] = hi;
    }
    let dims = h.axes.len();
    let read = |o: [u8; 2], x: [f64; 2]| -> Result<f64, SolveError> {
        crate::info::table_at(model, h.table, o, &x[..dims]).map(|(v, _)| v).ok_or_else(|| {
            SolveError::Integrator {
                t,
                message: format!(
                    "the model does not give the table '{}', which a run holds in its cells \
                     (ModelFunctions::eval_table_order)",
                    h.label
                ),
            }
        })
    };
    let w = usize::from(h.degree[1]) + 1;
    for i in 0..=h.degree[0] {
        for j in 0..=h.degree[1] {
            let o = [h.order[0].saturating_add(i), h.order[1].saturating_add(j)];
            let c = read(o, at)? / (factorial(i) * factorial(j));
            d[h.coef[usize::from(i) * w + usize::from(j)]] = c;
        }
    }
    // the polynomial is the table in the cell: nothing of a higher degree
    // there (a table given cubic data after the model was built for
    // linear ones would be read wrong: such a model must be built again)
    for j in 0..dims {
        let mut o = h.order;
        o[j] = o[j].saturating_add(h.degree[j] + 1);
        let mut x = at;
        for frac in [0.0, 0.25] {
            x[j] = at[j] + frac * width[j];
            let v = read(o, x)?;
            if v != 0.0 {
                return Err(SolveError::Integrator {
                    t,
                    message: format!(
                        "the table '{}' interpolates its data with a higher degree than the model \
                         was built for (it was linear when the model was built): build the model \
                         again with these data",
                        h.label
                    ),
                });
            }
        }
    }
    Ok(())
}

/// Puts every held read whose argument left its cell (its edge function in
/// `g`, the model's zero crossings at the point with the discrete values
/// `d`, below zero), or whose cell is not set yet, in the cell the argument
/// lies in; not the reads `frozen` marks (moved already at this instant:
/// an argument the iteration variables decide can come out a little short
/// of the edge it crossed once they are solved again in the new cell, and
/// the read does not go back for that). Returns the reads moved.
pub(crate) fn place(
    model: &dyn ModelFunctions,
    info: &RunInfo,
    g: &[f64],
    t: f64,
    d: &mut [f64],
    frozen: &[bool],
) -> Result<Vec<usize>, SolveError> {
    let mut moved = vec![];
    for (k, h) in info.held.iter().enumerate() {
        if frozen.get(k).copied().unwrap_or(false) {
            continue;
        }
        let mut cells = [0usize; 2];
        let mut change = false;
        for (j, ax) in h.axes.iter().enumerate() {
            let (gb, ga) = (g[ax.below], g[ax.above]);
            let now = d[ax.cell];
            // (unset: negative, or not a number)
            let unset = now.is_nan() || now < 0.0;
            let p = info.table_points(h.table, j);
            if p.len() < 2 {
                return Err(SolveError::Integrator {
                    t,
                    message: format!(
                        "the run does not know the breakpoints of the table '{}', which it holds \
                         in its cells",
                        h.label
                    ),
                });
            }
            cells[j] = now as usize;
            if unset || gb < 0.0 || ga < 0.0 {
                // the argument, from its edge functions (a finite edge)
                let (lo, hi) = (d[ax.lo], d[ax.hi]);
                let a = if lo > -OPEN { lo + gb } else { hi - ga };
                if a.is_nan() {
                    continue;
                }
                let c = cell_of(p, a, merged(model, h.table, j));
                if unset || c != cells[j] {
                    cells[j] = c;
                    change = true;
                }
            }
        }
        if change {
            set(model, info, k, &cells[..h.axes.len()], t, d)?;
            moved.push(k);
        }
    }
    Ok(moved)
}

/// The held reads put in the cells their arguments start in, at `(t, y)`
/// with the discrete values `d` (before the integrator starts): placed
/// until nothing moves, as a read's argument may itself read a held table.
pub(crate) fn settle(
    model: &dyn ModelFunctions,
    info: &RunInfo,
    t: f64,
    y: &[f64],
    d: &mut [f64],
    u: &[f64],
) -> Result<(), SolveError> {
    if info.held.is_empty() {
        return Ok(());
    }
    let l = *model.layout();
    let mut work = vec![0.0; l.n_work];
    let mut g = vec![0.0; l.n_roots];
    // (a cell's number unset: the edges at 0, so that the edge functions
    // are the arguments themselves)
    for h in &info.held {
        for ax in &h.axes {
            if d[ax.cell].is_nan() || d[ax.cell] < 0.0 {
                d[ax.lo] = 0.0;
                d[ax.hi] = 0.0;
            }
        }
    }
    for _ in 0..=2 * info.held.len() + 2 {
        let inp = EvalInput { t, y, p: &info.params, d, u };
        model.roots(&inp, &mut work, &mut g);
        if place(model, info, &g, t, d, &[])?.is_empty() {
            return Ok(());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells_are_the_tables_own() {
        let p = [0.0, 1.0, 3.0];
        assert_eq!(cell_of(&p, -0.5, false), 0);
        assert_eq!(cell_of(&p, 0.0, false), 1);
        assert_eq!(cell_of(&p, 0.999, false), 1);
        assert_eq!(cell_of(&p, 1.0, false), 2);
        assert_eq!(cell_of(&p, 3.0, false), 2, "the data's last point is in the last cell");
        assert_eq!(cell_of(&p, 3.0 + 1e-15, false), 3);
        for c in 0..=3 {
            let (lo, hi) = edges(&p, c, false);
            let m = anchor(&p, c);
            assert!(lo < m && m < hi, "cell {c}: {lo} < {m} < {hi}");
            assert_eq!(cell_of(&p, m, false), c);
        }
        // continued outside: the edge cells reach out
        assert_eq!(cell_of(&p, -0.5, true), 1);
        assert_eq!(cell_of(&p, 4.0, true), 2);
        assert_eq!(edges(&p, 1, true), (-OPEN, 1.0));
        assert_eq!(edges(&p, 2, true), (1.0, OPEN));
    }
}
