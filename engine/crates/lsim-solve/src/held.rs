//! Table reads held in their cells during every step ([`crate::HeldInfo`],
//! `lsim_prep::hold`; DESIGN.md §8.2, *Tables read at a state*): Park and
//! Barton's discontinuity locking. The equations read each such table as
//! its cell's own function with discrete coefficients; here the run puts
//! each read in the cell its argument lies in (at the start, and at every
//! event where the argument left its cell, which the cell's edges, `a − lo`
//! and `hi − a`, tell by falling below zero), and sets the coefficients
//! from the table the model interpolates.
//!
//! Held at every breakpoint ([`lsim_ir::HeldCells::Points`]), an axis
//! with points `p₀ < … < p₍ₙ₋₁₎` has the cells 0 (below `p₀`), `i` for
//! `p₍ᵢ₋₁₎ ≤ a < pᵢ` (`i` = 1 … n − 1; the last one closed at `p₍ₙ₋₁₎`, as
//! the table takes the data's last point in its last cell) and `n` (above
//! `p₍ₙ₋₁₎`). A cell's polynomial is written at a point strictly inside it
//! (the middle; outside the data, one cell's width from the edge), where
//! the table's derivatives are those of that cell's polynomial: its Taylor
//! coefficients there are the cell's polynomial, exact to rounding, inside
//! the cell and continued beyond it. Held at the data's edges only
//! ([`lsim_ir::HeldCells::Edges`]), the cells are 0 (below `p₀`), 1
//! (`p₀ ≤ a ≤ p₍ₙ₋₁₎`) and 2 (above): inside, the table is read where the
//! argument is, held between the edges, and continued beyond them by the
//! first and the last data cells' polynomials; outside, the table is read
//! at the edge, with the difference to its held value outside added.
//!
//! A group of held reads (one table's at the same arguments) is released
//! where holding it costs more than it saves (`SolverOptions::held_release`,
//! DESIGN.md §8.2): at the start, where its jumps are within the tolerance
//! ([`continuous_to`]), and during the run, where its crossings come too
//! often ([`released_by_crossings`]). Released, it reads the table itself
//! (stepped over) for the rest of the run, its edges open.

use crate::SolveError;
use crate::info::{HeldInfo, RunInfo};
use lsim_ir::{EvalInput, ModelFunctions};

/// An open edge of a cell (`lsim_prep::hold::OPEN`).
const OPEN: f64 = 1e300;

/// The cell `a` lies in, among the points `p` (at least two), held at
/// every point; `merged`: the cells outside the data are the edge cells'
/// own (the table goes on with their polynomials there: no kink at the
/// data's edges).
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

/// The cell `a` lies in, held at the data's edges only: 0 below, 1 inside
/// (the edges included), 2 above.
fn cell_between(p: &[f64], a: f64) -> usize {
    if a < p[0] {
        0
    } else if a > p[p.len() - 1] {
        2
    } else {
        1
    }
}

/// [`cell_between`]'s cell `c`'s edges.
fn edges_between(p: &[f64], c: usize) -> (f64, f64) {
    let (first, last) = (p[0], p[p.len() - 1]);
    match c {
        0 => (-OPEN, first),
        1 => (first, last),
        _ => (last, OPEN),
    }
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

/// The message of a held table whose data no longer suit the model.
fn rebuild(t: f64, h: &HeldInfo, what: &str) -> SolveError {
    SolveError::Integrator {
        t,
        message: format!(
            "the table '{}' {what} than when the model was built, which holds it in its cells: \
             build the model again with these data",
            h.label
        ),
    }
}

/// Puts read `k` in the cells `cells` (one per argument): their numbers,
/// points, edges and the polynomials' coefficients, in `d`.
fn set(
    model: &dyn ModelFunctions,
    info: &RunInfo,
    k: usize,
    cells: &[usize],
    t: f64,
    d: &mut [f64],
) -> Result<(), SolveError> {
    let h = &info.held[k];
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
    // the polynomial at `at` along every axis is the table in the cell
    // (`width` wide there): nothing of a higher degree (a table given cubic
    // data after the model was built for linear ones would be read wrong:
    // such a model must be built again)
    let degree_holds = |at: [f64; 2], width: [f64; 2]| -> Result<(), SolveError> {
        for j in 0..dims {
            let mut o = h.order;
            o[j] = o[j].saturating_add(h.degree[j] + 1);
            let mut x = at;
            for frac in [0.0, 0.25] {
                x[j] = at[j] + frac * width[j];
                if read(o, x)? != 0.0 {
                    return Err(rebuild(t, h, "interpolates its data with a higher degree"));
                }
            }
        }
        Ok(())
    };
    let Some(e) = h.edges else {
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
        let w = usize::from(h.degree[1]) + 1;
        for i in 0..=h.degree[0] {
            for j in 0..=h.degree[1] {
                let o = [h.order[0].saturating_add(i), h.order[1].saturating_add(j)];
                let c = read(o, at)? / (factorial(i) * factorial(j));
                d[h.coef[usize::from(i) * w + usize::from(j)]] = c;
            }
        }
        return degree_holds(at, width);
    };
    // held at the data's edges only (one argument): between them the table
    // itself, which must be smooth enough there for the read
    let ax = &h.axes[0];
    let p = info.table_points(h.table, 0);
    let n = p.len();
    if let Some(tb) = model.table_interp(h.table)
        && u32::from(h.order[0]) > tb.smoothness(0).inside
    {
        return Err(rebuild(t, h, "is less smooth between its points"));
    }
    let c = cells[0];
    let (lo, hi) = edges_between(p, c);
    d[ax.cell] = c as f64;
    d[ax.lo] = lo;
    d[ax.hi] = hi;
    let w = usize::from(h.degree[0]) + 1;
    if c == 1 {
        let (first, last) = (anchor(p, 1), anchor(p, n - 1));
        d[e.from] = p[0];
        d[e.to] = p[n - 1];
        d[e.offset] = 0.0;
        d[ax.anchor] = first;
        d[e.anchor_end] = last;
        for i in 0..=h.degree[0] {
            let o = [h.order[0].saturating_add(i), 0];
            let f = factorial(i);
            d[h.coef[usize::from(i)]] = read(o, [first, 0.0])? / f;
            d[h.coef[w + usize::from(i)]] = read(o, [last, 0.0])? / f;
        }
        degree_holds([first, 0.0], [p[1] - p[0], 0.0])?;
        return degree_holds([last, 0.0], [p[n - 1] - p[n - 2], 0.0]);
    }
    // outside: the table read at the edge, plus its held value there less
    // that (the table must be constant there: held at its edges)
    let edge = if c == 0 { p[0] } else { p[n - 1] };
    let out = anchor(p, if c == 0 { 0 } else { n });
    d[e.from] = edge;
    d[e.to] = edge;
    d[e.offset] = read(h.order, [out, 0.0])? - read(h.order, [edge, 0.0])?;
    d[ax.anchor] = out;
    d[e.anchor_end] = out;
    for &v in &h.coef {
        d[v] = 0.0;
    }
    let mut o = h.order;
    o[0] = o[0].saturating_add(1);
    let side = if c == 0 { -1.0 } else { 1.0 };
    for x in [out, out + side * 0.25 * (p[1] - p[0]).abs()] {
        if read(o, [x, 0.0])? != 0.0 {
            return Err(rebuild(t, h, "is not held at its edges, as it was"));
        }
    }
    Ok(())
}

/// Puts every held read whose argument left its cell along an axis (its
/// edge function in `g`, the model's zero crossings at the point with the
/// discrete values `d`, below zero), or whose cell is not set yet, in the
/// cell the argument lies in; not along the axes `frozen` marks (moved
/// already at this instant: an argument the iteration variables decide can
/// come out a little short of the edge it crossed once they are solved
/// again in the new cell, and the read does not go back for that). Returns
/// the reads moved and along which axes.
pub(crate) fn place(
    model: &dyn ModelFunctions,
    info: &RunInfo,
    g: &[f64],
    t: f64,
    d: &mut [f64],
    frozen: &[[bool; 2]],
) -> Result<Vec<(usize, [bool; 2])>, SolveError> {
    let mut moved = vec![];
    for (k, h) in info.held.iter().enumerate() {
        // (released: the table itself, its edges open)
        if d[h.released] > 0.5 {
            continue;
        }
        let mut cells = [0usize; 2];
        let mut change = [false; 2];
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
            let frozen = frozen.get(k).is_some_and(|f| f[j]);
            if unset || (!frozen && (gb < 0.0 || ga < 0.0)) {
                // the argument, from its edge functions (a finite edge)
                let (lo, hi) = (d[ax.lo], d[ax.hi]);
                let a = if lo > -OPEN { lo + gb } else { hi - ga };
                if a.is_nan() {
                    continue;
                }
                let c = match h.edges {
                    None => cell_of(p, a, merged(model, h.table, j)),
                    Some(_) => cell_between(p, a),
                };
                if unset || c != cells[j] {
                    cells[j] = c;
                    change[j] = true;
                }
            }
        }
        if change.iter().any(|c| *c) {
            set(model, info, k, &cells[..h.axes.len()], t, d)?;
            moved.push((k, change));
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
        if d[h.released] > 0.5 {
            continue;
        }
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

/// The number of groups of held reads (the reads of one table at the same
/// arguments: [`HeldInfo::group`]).
pub(crate) fn groups(info: &RunInfo) -> usize {
    info.held.iter().map(|h| h.group + 1).max().unwrap_or(0)
}

/// Releases group `g` in `d`: each of its reads reads the table itself from
/// here on, stepped over by the integrator, its edges open (their
/// functions never cross zero again).
pub(crate) fn release(info: &RunInfo, g: usize, d: &mut [f64]) {
    for h in info.held.iter().filter(|h| h.group == g) {
        d[h.released] = 1.0;
        for ax in &h.axes {
            d[ax.lo] = -OPEN;
            d[ax.hi] = OPEN;
        }
    }
}

/// The steps between crossings below which a held read's crossings come
/// too often ([`released_by_crossings`]): after a restart CVODE runs at
/// order 1 and raises its order only after `q + 1` steps at order `q`, so
/// its 6th step is the first that can be at order 3.
pub(crate) const RAMP_STEPS: u64 = 6;

/// How many intervals between a group's crossings the run looks back over.
pub(crate) const CROSSINGS_WINDOW: usize = 8;

/// Whether a group whose crossings came after the integrator's step counts
/// `at` (oldest first, the last one now) has come to cross too often: its
/// last [`CROSSINGS_WINDOW`] intervals between crossings took fewer than
/// [`RAMP_STEPS`] steps each on average, so that the integrator, which
/// restarts at every crossing, was kept in the low orders of its restarts
/// (DESIGN.md §8.2). Then stepping over the breakpoints costs less than
/// holding the read.
pub(crate) fn released_by_crossings(at: &[u64]) -> bool {
    let n = at.len();
    n > CROSSINGS_WINDOW
        && at[n - 1] - at[n - 1 - CROSSINGS_WINDOW] < RAMP_STEPS * CROSSINGS_WINDOW as u64
}

/// Whether group `g`'s reads that jump are continuous to the tolerance
/// `rtol` in the table the model interpolates: at every breakpoint they are
/// held at, the read's two sides differ by no more than `rtol` times the
/// largest of their magnitudes over those points (the right-hand side then
/// jumps there by about the tolerance or less, which stepping over costs
/// no more than a restart). False for a group with no read that jumps
/// (held only for comparison) and for a 2-D read.
pub(crate) fn continuous_to(
    model: &dyn ModelFunctions,
    info: &RunInfo,
    g: usize,
    rtol: f64,
    t: f64,
) -> Result<bool, SolveError> {
    let mut any = false;
    let mut jump: f64 = 0.0;
    let mut size: f64 = 0.0;
    for h in info.held.iter().filter(|h| h.group == g && h.jumps) {
        if h.axes.len() != 1 {
            return Ok(false);
        }
        any = true;
        let p = info.table_points(h.table, 0);
        let n = p.len();
        if n < 2 {
            return Ok(false);
        }
        let read = |o: u8, x: f64| -> Result<f64, SolveError> {
            crate::info::table_at(model, h.table, [o, 0], &[x]).map(|(v, _)| v).ok_or_else(|| {
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
        let o = h.order[0];
        // the two sides at each breakpoint it is held at
        let mut sides: Vec<(f64, f64)> = vec![];
        match h.edges {
            None => {
                // cell c's polynomial (its Taylor coefficients at its
                // anchor) at x
                let poly = |c: usize, x: f64| -> Result<f64, SolveError> {
                    let m = anchor(p, c);
                    let mut v = 0.0;
                    for i in (0..=h.degree[0]).rev() {
                        v = v * (x - m) + read(o.saturating_add(i), m)? / factorial(i);
                    }
                    Ok(v)
                };
                let merged = merged(model, h.table, 0);
                for (c, &b) in p.iter().enumerate() {
                    // b parts cell c from cell c + 1
                    if merged && (c == 0 || c == n - 1) {
                        continue;
                    }
                    sides.push((poly(c, b)?, poly(c + 1, b)?));
                }
            }
            Some(_) => {
                let w0 = (p[1] - p[0]).abs();
                let w1 = (p[n - 1] - p[n - 2]).abs();
                sides.push((read(o, p[0] - w0)?, read(o, p[0])?));
                sides.push((read(o, p[n - 1])?, read(o, p[n - 1] + w1)?));
            }
        }
        for (a, b) in sides {
            jump = jump.max((a - b).abs());
            size = size.max(a.abs()).max(b.abs());
        }
    }
    Ok(any && jump <= rtol * size)
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
        // held at the edges only: below, inside (both edges in), above
        assert_eq!(cell_between(&p, -1e-15), 0);
        assert_eq!(cell_between(&p, 0.0), 1);
        assert_eq!(cell_between(&p, 1.0), 1);
        assert_eq!(cell_between(&p, 3.0), 1);
        assert_eq!(cell_between(&p, 3.0 + 1e-15), 2);
        assert_eq!(edges_between(&p, 0), (-OPEN, 0.0));
        assert_eq!(edges_between(&p, 1), (0.0, 3.0));
        assert_eq!(edges_between(&p, 2), (3.0, OPEN));
    }
}
