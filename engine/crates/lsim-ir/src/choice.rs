//! How a choice of states stands at a point (DESIGN.md §6, step 5): the
//! measure preparation and the run both judge index reduction's choice by
//! ([`crate::StateSet`]; the run's watch is `lsim_solve::states`).
//!
//! With `M` a set's constraints differentiated by its candidates (rows ×
//! columns) and `S` the columns the choice decides (one per row), `B =
//! M_S⁻¹ M` holds how much each decided candidate moves per unit of each
//! candidate that stays a state while the constraints hold (`|dx/dy|` for a
//! pendulum whose rod decides x from y). Its largest entry outside `S` is
//! the factor by which exchanging that pair would enlarge the chosen
//! block's determinant (as in a basis exchange): 1 or less where no
//! exchange helps. It depends neither on how the constraints are written
//! (any row operation) nor on the order the choice took them in. Columns
//! the choice left out (kept above all: `stateSelect = always`, a state a
//! `reinit` restarts) are measured apart: an exchange with one of them is
//! a choice the model forbids. How far the constraints are from depending
//! on each other, for any choice, is measured on `M` with each column and
//! then each row scaled to its largest entry, so the candidates' units (a
//! speed beside a ratio) do not count.
//!
//! The exchange factor is in the candidates' own units, as the choice by
//! pivoting is: two shafts tied by a gear of ratio 150 move 150 times as
//! much as each other, which is a scale and no degeneracy. So a set whose
//! entries move only at events (parameters, tables, discrete values) is
//! judged only by whether its chosen block is singular; the factor judges
//! the sets whose entries move with the solution, where it changes by
//! orders of magnitude only as the motion approaches a point where the
//! choice decides nothing (a pendulum's x decided from y at the bottom of
//! its swing).

/// The run's limit (`lsim_solve::states`): where a decided candidate moves
/// more than `1 / DEGRADED` times as much as a state (ten times beyond the
/// factor 10 the choice by pivoting allows at the start), or the
/// constraints' smallest pivot falls below it, the choice has degraded and
/// the run stops. Preparation takes a choice that `stateSelect` asks for
/// only well inside it (`lsim_prep::index::WIDE`).
pub const DEGRADED: f64 = 1e-2;

/// A set of index reduction's choice to keep as it is where a run chooses
/// its states again ([`ChoicePoint::keep`]: every set but the one that
/// degraded): which set ([`crate::StateSet::eqs`] and
/// [`crate::StateSet::nodes`], the same in every preparation of the model)
/// and its choice.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct KeepSet {
    /// the set's equations, in index reduction's own numbering
    pub eqs: Vec<usize>,
    /// its candidates, in index reduction's own numbering
    pub nodes: Vec<usize>,
    /// per row, the column chosen for it
    pub pivots: Vec<usize>,
    /// the columns left out of the choice
    pub masked: Vec<bool>,
}

/// A point of a run to choose the states at (dynamic state selection,
/// DESIGN.md §6 step 5): index reduction's dummy derivatives are chosen by
/// the same pivoting as at the start, `stateSelect` leading it, on the
/// constraints' derivatives evaluated here, except in the sets it keeps.
/// The start is not solved (the run's point is consistent).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChoicePoint {
    /// the time
    pub t: f64,
    /// the values by name: every channel, each state's derivative as
    /// `der(<state>)`, and the discrete values (one missing reads 0)
    pub values: Vec<(String, f64)>,
    /// the parameter values, in the flat system's order (empty: the
    /// model's own)
    pub params: Vec<f64>,
    /// derivatives (`der(x)`) to keep as variables of their own whatever
    /// the choice (the channels of the model the run switches from: a
    /// state's derivative is then also a variable, set from it), so that
    /// every channel the run records is in the model it switches to
    pub channels: Vec<String>,
    /// the pivoting's window for the sets chosen again: a preferred column
    /// is taken within this factor of a row's largest entry (the switch
    /// threshold of the set that degraded, so the choice the run leaves,
    /// just past it, is not taken again; 0: the start's, 10)
    pub window: f64,
    /// the sets kept as they are where they are not chosen again (the
    /// choices the run has)
    pub keep: Vec<KeepSet>,
    /// the equations of the set that degraded ([`crate::StateSet::eqs`]):
    /// it is chosen again with the sets of its equations at index
    /// reduction's other levels (those its rows are differentiated from or
    /// to, whose candidates its choice decides, or which decide its own);
    /// every other set keeps its choice (`keep`). Empty: every set is
    /// chosen again
    pub again: Vec<usize>,
}

/// How a choice stands at a point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PivotState {
    /// the largest factor by which taking a candidate that is a state in
    /// place of a decided one would enlarge the chosen block's determinant
    /// (1 or less: no exchange helps; infinite where the block is
    /// singular, NaN where an entry is not a number); the columns left out
    /// of the choice not counted
    pub exchange: f64,
    /// that decided candidate (a column)
    pub chosen: usize,
    /// that state candidate (a column)
    pub best: usize,
    /// the same factor for an exchange with a column left out of the choice
    /// (0: none is)
    pub exchange_kept: f64,
    /// the decided candidate of that exchange
    pub chosen_kept: usize,
    /// the column left out of it
    pub kept: usize,
    /// how far the constraints are from depending on each other, for any
    /// choice: the smallest pivot of `M` by full pivoting, its columns and
    /// then its rows each scaled to their largest entry (1 at best; 0:
    /// dependent)
    pub independence: f64,
    /// the row where it is smallest
    pub dep_row: usize,
    /// the sign of the chosen block's determinant (0: singular)
    pub sign: f64,
}

impl PivotState {
    /// The larger of the two exchange factors.
    pub fn worst_exchange(&self) -> f64 {
        if self.exchange.is_nan() { f64::NAN } else { self.exchange.max(self.exchange_kept) }
    }
}

/// Scratch for [`pivot_state`].
#[derive(Default, Clone, Debug)]
pub struct Scratch {
    a: Vec<Vec<f64>>,
    raw: Vec<Vec<f64>>,
    scales: Vec<f64>,
    perm: Vec<usize>,
    b: Vec<f64>,
    tmp: Vec<f64>,
}

/// The LU factors of a square matrix with partial pivoting, in place: the
/// row order and the determinant's sign (0: singular).
fn lu(a: &mut [Vec<f64>], perm: &mut Vec<usize>) -> f64 {
    let n = a.len();
    perm.clear();
    perm.extend(0..n);
    let mut sign = 1.0;
    for k in 0..n {
        let p = (k..n).max_by(|&i, &j| a[i][k].abs().total_cmp(&a[j][k].abs())).unwrap_or(k);
        if p != k {
            a.swap(p, k);
            perm.swap(p, k);
            sign = -sign;
        }
        let d = a[k][k];
        if d == 0.0 {
            return 0.0;
        }
        sign *= d.signum();
        let (top, rest) = a.split_at_mut(k + 1);
        for row in rest.iter_mut() {
            let f = row[k] / d;
            row[k] = f;
            if f != 0.0 {
                for (x, q) in row[k + 1..].iter_mut().zip(&top[k][k + 1..]) {
                    *x -= f * q;
                }
            }
        }
    }
    sign
}

/// Solves `LU x = b[perm]` (the factors of [`lu`]); `b` comes back as `x`.
fn lu_solve(a: &[Vec<f64>], perm: &[usize], b: &mut Vec<f64>, tmp: &mut Vec<f64>) {
    let n = a.len();
    tmp.clear();
    tmp.extend(perm.iter().map(|&i| b[i]));
    for i in 0..n {
        let s: f64 = (0..i).map(|j| a[i][j] * tmp[j]).sum();
        tmp[i] -= s;
    }
    for i in (0..n).rev() {
        let s: f64 = (i + 1..n).map(|j| a[i][j] * tmp[j]).sum();
        tmp[i] = (tmp[i] - s) / a[i][i];
    }
    b.clear();
    b.extend_from_slice(tmp);
}

/// How far the rows of `m` are from depending on each other, whatever the
/// columns' units ([`PivotState::independence`]): the smallest pivot of
/// Gaussian elimination with full pivoting once its columns and rows are
/// scaled to their largest entries (1 at best, 0: dependent), its row and
/// its column; `m` (the entries as they are) is changed.
pub fn smallest_pivot(m: &mut [Vec<f64>]) -> (f64, usize, usize) {
    let rows = m.len();
    let cols = m.first().map_or(0, Vec::len);
    // the columns, then the rows, each scaled to its largest entry, three
    // times over (the scales settle: a gearbox row whose ratio's column
    // carries a 150 rad/s shaft speed beside a row that sets the ratio)
    for _ in 0..3 {
        for c in 0..cols {
            let s = m.iter().fold(0.0f64, |a, r| a.max(r[c].abs()));
            if s > 0.0 {
                for r in m.iter_mut() {
                    r[c] /= s;
                }
            }
        }
        for r in m.iter_mut() {
            let s = r.iter().fold(0.0f64, |a, x| a.max(x.abs()));
            if s > 0.0 {
                for x in r.iter_mut() {
                    *x /= s;
                }
            }
        }
    }
    let mut used_c = vec![false; cols];
    let mut used_r = vec![false; rows];
    let (mut smallest, mut at, mut at_c) = (1.0f64, 0, 0);
    for _ in 0..rows {
        let mut best = (0.0f64, usize::MAX, usize::MAX);
        for r in (0..rows).filter(|&r| !used_r[r]) {
            for c in (0..cols).filter(|&c| !used_c[c]) {
                if m[r][c].abs() > best.0 {
                    best = (m[r][c].abs(), r, c);
                }
            }
        }
        let (p, r, c) = best;
        if r == usize::MAX {
            let r0 = (0..rows).find(|&r| !used_r[r]).unwrap_or(0);
            let c0 = (0..cols).find(|&c| !used_c[c]).unwrap_or(0);
            return (0.0, r0, c0);
        }
        if p < smallest {
            smallest = p;
            at = r;
            at_c = c;
        }
        used_r[r] = true;
        used_c[c] = true;
        let pivot_row = m[r].clone();
        for (i, row) in m.iter_mut().enumerate() {
            if used_r[i] {
                continue;
            }
            let f = row[c] / pivot_row[c];
            if f != 0.0 {
                for (x, q) in row.iter_mut().zip(&pivot_row) {
                    *x -= f * q;
                }
            }
        }
    }
    (smallest, at, at_c)
}

/// How the choice `pivots` (per row, the column it decides) stands on the
/// entries `m` (rows × columns; changed), `masked` the columns left out of
/// it.
pub fn pivot_state(
    pivots: &[usize],
    masked: &[bool],
    m: &mut [Vec<f64>],
    s: &mut Scratch,
) -> PivotState {
    pivot_state_with(pivots, masked, m, s, true, None)
}

/// [`pivot_state`], with the independence measured only when asked
/// (`independence`; else it reads 1), and, when `reach` is given, for each
/// row `r` the largest entry of column `r` of the chosen block's inverse
/// (the entries as they are, unscaled): how far a change of that row's
/// entries can move what the block decides.
pub fn pivot_state_with(
    pivots: &[usize],
    masked: &[bool],
    m: &mut [Vec<f64>],
    s: &mut Scratch,
    independence_too: bool,
    reach: Option<&mut Vec<f64>>,
) -> PivotState {
    let rows = m.len();
    let cols = m.first().map_or(0, Vec::len);
    let first = pivots.first().copied().unwrap_or(0);
    let mut out = PivotState {
        exchange: 0.0,
        chosen: first,
        best: first,
        exchange_kept: 0.0,
        chosen_kept: first,
        kept: first,
        independence: 1.0,
        dep_row: 0,
        sign: 1.0,
    };
    if independence_too {
        s.raw.resize(rows, vec![]);
        for (a, b) in s.raw.iter_mut().zip(m.iter()) {
            a.clone_from(b);
        }
    }
    // each row scaled to its largest entry (which changes neither B nor
    // the choice)
    s.scales.clear();
    for row in m.iter_mut() {
        let scale = row.iter().fold(0.0f64, |a, x| a.max(x.abs()));
        if !scale.is_finite() || row.iter().any(|x| x.is_nan()) {
            out.exchange = f64::NAN;
            out.sign = f64::NAN;
            return out;
        }
        s.scales.push(if scale > 0.0 { scale } else { 1.0 });
        if scale > 0.0 {
            for x in row.iter_mut() {
                *x /= scale;
            }
        }
    }

    // the chosen block, and B on the columns outside it
    s.a.resize(rows, vec![]);
    for (r, row) in m.iter().enumerate() {
        s.a[r].clear();
        s.a[r].extend(pivots.iter().map(|&c| row[c]));
    }
    out.sign = lu(&mut s.a, &mut s.perm);
    if out.sign == 0.0 {
        out.exchange = f64::INFINITY;
        if let Some(reach) = reach {
            reach.clear();
            reach.resize(rows, f64::INFINITY);
        }
    } else {
        if let Some(reach) = reach {
            // column r of the unscaled block's inverse: the scaled one's,
            // over row r's scale
            reach.clear();
            for r in 0..rows {
                let sc = s.scales[r];
                s.b.clear();
                s.b.resize(rows, 0.0);
                s.b[r] = 1.0;
                lu_solve(&s.a, &s.perm, &mut s.b, &mut s.tmp);
                reach.push(s.b.iter().fold(0.0f64, |a, x| a.max(x.abs())) / sc);
            }
        }
        for c in (0..cols).filter(|c| !pivots.contains(c)) {
            s.b.clear();
            s.b.extend(m.iter().map(|row| row[c]));
            lu_solve(&s.a, &s.perm, &mut s.b, &mut s.tmp);
            let kept = masked.get(c).copied().unwrap_or(false);
            for (i, &x) in s.b.iter().enumerate() {
                let x = if x.is_nan() { f64::INFINITY } else { x.abs() };
                if kept && x > out.exchange_kept {
                    out.exchange_kept = x;
                    out.chosen_kept = pivots[i];
                    out.kept = c;
                } else if !kept && x > out.exchange {
                    out.exchange = x;
                    out.chosen = pivots[i];
                    out.best = c;
                }
            }
        }
    }
    if independence_too {
        let (ind, at, _) = smallest_pivot(&mut s.raw);
        out.independence = ind;
        out.dep_row = at;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(pivots: &[usize], masked: &[bool], mut m: Vec<Vec<f64>>) -> PivotState {
        pivot_state(pivots, masked, &mut m, &mut Scratch::default())
    }

    #[test]
    fn the_exchange_factor_is_how_much_a_decided_variable_moves_per_state() {
        // the rod's constraint x² + y² = L², by x and y: [2x 2y]; the choice
        // decides x (column 0): dx/dy = -y/x
        let at = |x: f64, y: f64| state(&[0], &[false; 2], vec![vec![2.0 * x, 2.0 * y]]);
        assert!((at(0.9, -0.1).exchange - 0.1 / 0.9).abs() < 1e-15);
        let low = at(0.005, -1.0);
        assert!((low.exchange - 200.0).abs() < 1e-9 && low.best == 1 && low.chosen == 0);
        assert_eq!(low.sign, 1.0);
        assert_eq!(at(-0.005, -1.0).sign, -1.0);
        let zero = at(0.0, -1.0);
        assert!(zero.exchange.is_infinite() && zero.sign == 0.0);
        // a single row is never dependent, whatever its entries' sizes
        assert_eq!(low.independence, 1.0);
        // with the trivial rows der(x) = vx, der(y) = vy beside it, in any
        // order: the same factor
        let (x, y) = (0.005, -1.0);
        let rows = vec![
            vec![0.0, 0.0, 2.0 * x, 2.0 * y],
            vec![-1.0, 0.0, 1.0, 0.0],
            vec![0.0, -1.0, 0.0, 1.0],
        ];
        let ps = state(&[2, 0, 3], &[false; 4], rows.clone());
        assert!((ps.exchange - 200.0).abs() < 1e-9, "{ps:?}");
        let swapped = vec![rows[1].clone(), rows[2].clone(), rows[0].clone()];
        let ps2 = state(&[0, 3, 2], &[false; 4], swapped);
        assert!((ps2.exchange - ps.exchange).abs() < 1e-9);
    }

    #[test]
    fn the_order_the_choice_took_its_pivots_in_does_not_matter() {
        let m = vec![vec![1.0, 0.02, 0.3], vec![0.5, 1.0, 0.2]];
        let a = state(&[0, 1], &[false; 3], m.clone());
        let b = state(&[1, 0], &[false; 3], m);
        assert!((a.exchange - b.exchange).abs() < 1e-12);
        assert_eq!(a.sign, -b.sign);
    }

    #[test]
    fn rows_that_depend_on_each_other_are_told_whatever_the_units() {
        let ps = state(&[0, 1], &[false; 3], vec![vec![1.0, 1.0, 0.5], vec![1.0, 1.0 + 1e-5, 0.5]]);
        assert!(ps.independence < 1e-4, "{ps:?}");
        // the hybrid car's driveline at 84 s: a gearbox w_in − r·w_out −
        // w_out·r' (the ratio's rate in a column of 150 rad/s), the row
        // that sets the ratio, a clutch, a final drive, a differential and
        // the connections: no dependence
        let m = vec![
            vec![-1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            vec![0.0, 0.0, 1.0, -1.15, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -152.65],
            vec![0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
            vec![0.0, 0.0, 0.0, 1.0, -3.38, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, -0.5, -0.5, 0.0],
            vec![0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            vec![0.0, 0.0, 0.0, 0.0, 1.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            vec![0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 1.0, 0.0, 0.0],
            vec![0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 1.0, 0.0],
        ];
        let ps = state(&[6, 3, 11, 4, 10, 2, 5, 9, 1], &[false; 12], m);
        assert!(ps.independence >= 0.5 && ps.exchange < 2.0, "{ps:?}");
    }

    #[test]
    fn an_exchange_with_a_column_left_out_is_measured_apart() {
        // column 0 kept above all (stateSelect = always)
        let ps = state(&[1], &[true, false, false], vec![vec![100.0, 0.5, 1.0]]);
        assert_eq!((ps.exchange, ps.best), (2.0, 2));
        assert_eq!((ps.exchange_kept, ps.kept, ps.chosen_kept), (200.0, 0, 1));
        assert_eq!(ps.worst_exchange(), 200.0);
    }
}
