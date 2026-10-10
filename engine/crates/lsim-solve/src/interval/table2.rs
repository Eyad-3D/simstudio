//! A 2-D table over a box of its arguments: its value, partial derivatives
//! and second partial derivatives, for the enclosures of conditions that
//! read one where both of its arguments may move (a motor's loss map by
//! speed and torque).
//!
//! The model's interpolant is a polynomial on each region of the grid: a
//! cell inside the data is a bicubic patch (bilinear for linear
//! interpolation, a bicubic Hermite patch for the monotone cubic), and past
//! the data along an axis the table holds its edge or continues along the
//! edge slope, which is of degree 1 along that axis. Each region's
//! polynomial is fitted, on first use, to the model's own interpolant at a
//! tensor grid of points inside the region (4 per axis of degree 3 at
//! Chebyshev points, 2 per axis of degree 1), which gives it exactly up to
//! round-off; one more point checks the fit, and four times its error
//! there, with the round-off of the coefficients, widens every enclosure
//! (as the dense output's polynomials do: Markov's inequality bounds the
//! derivatives' share). A box over several regions takes the hull of each
//! region's enclosure; the first derivatives may jump across a region's
//! edge (bilinear cells), so such a box has no second derivatives.

use super::{ALL, Iv, ZERO};
use lsim_ir::runtime::ModelFunctions;
use std::cell::OnceCell;

/// The regions one enclosure may visit (over more, it gives up: the box is
/// too wide to say anything useful).
const MAX_REGIONS: usize = 1024;

/// A region's polynomial in the local coordinates `u = (x − o₀) / h₀`,
/// `w = (y − o₁) / h₁`: `Σ c[p (d₁ + 1) + q] u^p w^q`.
#[derive(Clone, Debug)]
struct Patch {
    o: [f64; 2],
    h: [f64; 2],
    deg: [usize; 2],
    c: Vec<f64>,
    /// a bound on its distance from the interpolant over the region's
    /// nodes (beyond them, along an axis of degree 1, it grows with |u|)
    err: f64,
    /// the degrees of that distance, a polynomial (the fitted ones: the
    /// degree may drop to 1 for a bilinear cell)
    fit_deg: [usize; 2],
}

/// A 2-D table's grid and its regions' polynomials (fitted on first use).
#[derive(Debug)]
pub(crate) struct Grid2 {
    k: u32,
    ax: [Vec<f64>; 2],
    patches: Vec<OnceCell<Option<Patch>>>,
}

/// The value, its partial derivatives along x and y, and the second
/// partials xx, xy, yy over a box.
pub(crate) type Partials = [Iv; 6];

/// Where region `r` of an axis starts, its scale, its degree, the points a
/// fit samples and the point that checks it (in its local coordinate):
/// region 0 lies before the data, `n` after them.
fn region(x: &[f64], r: usize) -> (f64, f64, usize, Vec<f64>, f64) {
    let n = x.len();
    if r == 0 {
        (x[0], x[1] - x[0], 1, vec![-0.75, -0.25], -1.5)
    } else if r == n {
        (x[n - 1], x[n - 1] - x[n - 2], 1, vec![0.25, 0.75], 1.5)
    } else {
        let cheb = (0..4)
            .map(|k| 0.5 * (1.0 - ((2 * k + 1) as f64 * std::f64::consts::PI / 8.0).cos()))
            .collect();
        (x[r - 1], x[r] - x[r - 1], 3, cheb, 0.37)
    }
}

/// The inverse of the Vandermonde matrix at `u` (`m[p][k]`: the
/// coefficient of `u^p` from the value at `u[k]`), by Gauss–Jordan with
/// partial pivoting.
fn inverse_vandermonde(u: &[f64]) -> Vec<Vec<f64>> {
    let n = u.len();
    let mut m: Vec<Vec<f64>> = (0..n)
        .map(|i| {
            let mut row: Vec<f64> = (0..n).map(|k| u[i].powi(k as i32)).collect();
            row.extend((0..n).map(|k| if k == i { 1.0 } else { 0.0 }));
            row
        })
        .collect();
    for col in 0..n {
        let piv =
            (col..n).max_by(|a, b| m[*a][col].abs().total_cmp(&m[*b][col].abs())).unwrap_or(col);
        m.swap(col, piv);
        let p = m[col][col];
        for x in m[col].iter_mut() {
            *x /= p;
        }
        let pivot_row = m[col].clone();
        for (r, row) in m.iter_mut().enumerate() {
            if r != col {
                let f = row[col];
                if f != 0.0 {
                    for (x, pv) in row.iter_mut().zip(&pivot_row) {
                        *x -= f * pv;
                    }
                }
            }
        }
    }
    // [V | I] became [I | V⁻¹]
    (0..n).map(|p| (0..n).map(|k| m[p][n + k]).collect()).collect()
}

/// The polynomial `Σ a[p] u^p` and its first and second derivatives over
/// `u` (interval Horner).
fn horner3(a: &[Iv], u: Iv) -> [Iv; 3] {
    let n = a.len();
    let mut v = ZERO;
    let mut d = ZERO;
    let mut dd = ZERO;
    for p in (0..n).rev() {
        v = v.mul(u).add(a[p]);
        if p >= 1 {
            d = d.mul(u).add(a[p].scale(p as f64));
        }
        if p >= 2 {
            dd = dd.mul(u).add(a[p].scale((p * (p - 1)) as f64));
        }
    }
    [v, d, dd]
}

/// Markov's bounds on a polynomial of degree `d` on an interval of unit
/// length: its first and second derivatives at most these times its
/// largest magnitude there.
fn markov(d: usize) -> (f64, f64) {
    let n2 = (d * d) as f64;
    (2.0 * n2, 4.0 * n2 * (n2 - 1.0) / 3.0)
}

impl Patch {
    fn fit(model: &dyn ModelFunctions, k: u32, ax: &[Vec<f64>; 2], r: [usize; 2]) -> Option<Patch> {
        let (o0, h0, d0, u, uc) = region(&ax[0], r[0]);
        let (o1, h1, d1, w, wc) = region(&ax[1], r[1]);
        let at = |u: f64, w: f64| -> Option<f64> {
            let v = model.eval_table(k, [o0 + h0 * u, o1 + h1 * w])?.0;
            v.is_finite().then_some(v)
        };
        let mut f = vec![vec![0.0; w.len()]; u.len()];
        let mut fmax = 0.0f64;
        for (i, ui) in u.iter().enumerate() {
            for (j, wj) in w.iter().enumerate() {
                f[i][j] = at(*ui, *wj)?;
                fmax = fmax.max(f[i][j].abs());
            }
        }
        let (iu, iw) = (inverse_vandermonde(&u), inverse_vandermonde(&w));
        // c = Vu⁻¹ f Vw⁻ᵀ
        let mut c = vec![0.0; (d0 + 1) * (d1 + 1)];
        for p in 0..=d0 {
            for q in 0..=d1 {
                let mut s = 0.0;
                for (i, fi) in f.iter().enumerate() {
                    for (j, fij) in fi.iter().enumerate() {
                        s += iu[p][i] * fij * iw[q][j];
                    }
                }
                c[p * (d1 + 1) + q] = s;
            }
        }
        // round-off: the solve's (the inverses' row sums bound how the
        // values' errors reach the coefficients) and the evaluations'
        let row_sum = |m: &[Vec<f64>]| {
            m.iter().map(|r| r.iter().map(|x| x.abs()).sum::<f64>()).fold(0.0, f64::max)
        };
        let terms = ((d0 + 1) * (d1 + 1)) as f64;
        let cond = row_sum(&iu) * row_sum(&iw);
        let coef: f64 = c.iter().map(|x| x.abs()).sum();
        let mut err = 16.0 * f64::EPSILON * (terms * terms * cond * fmax + coef + fmax);
        // a bilinear cell: the powers above 1 are round-off; dropped (their
        // size joins the error), it costs a quarter of the work
        let (mut e0, mut e1) = (d0, d1);
        let high = |p: usize, q: usize| p > e0.min(1) || q > e1.min(1);
        let dropped: f64 = (0..=d0)
            .flat_map(|p| (0..=d1).map(move |q| (p, q)))
            .filter(|(p, q)| high(*p, *q))
            .map(|(p, q)| c[p * (d1 + 1) + q].abs())
            .sum();
        if dropped <= 1e-12 * (coef + fmax) {
            err += dropped;
            (e0, e1) = (e0.min(1), e1.min(1));
        }
        let c = (0..=e0)
            .flat_map(|p| (0..=e1).map(move |q| (p, q)))
            .map(|(p, q)| c[p * (d1 + 1) + q])
            .collect();
        let mut patch =
            Patch { o: [o0, o1], h: [h0, h1], deg: [e0, e1], c, err, fit_deg: [d0, d1] };
        let miss = (patch.at(uc, wc) - at(uc, wc)?).abs();
        patch.err += 4.0 * miss;
        patch.err.is_finite().then_some(patch)
    }

    /// The value at local coordinates `(u, w)`.
    fn at(&self, u: f64, w: f64) -> f64 {
        let d1 = self.deg[1];
        (0..=self.deg[0]).rev().fold(0.0, |acc, p| {
            let row = &self.c[p * (d1 + 1)..(p + 1) * (d1 + 1)];
            acc * u + row.iter().rev().fold(0.0, |r, a| r * w + a)
        })
    }

    /// Its enclosures over the box `x × y` (inside its region).
    fn enclose(&self, x: Iv, y: Iv) -> Partials {
        let local = |v: Iv, a: usize| {
            Iv::wide((v.lo - self.o[a]) / self.h[a], (v.hi - self.o[a]) / self.h[a], 2)
        };
        let (u, w) = (local(x, 0), local(y, 1));
        let [v, vu, vw, vuu, vuw, vww] =
            if self.deg == [1, 1] { self.bilinear(u, w) } else { self.horner(u, w) };
        // the fit's error: along an axis of degree 1 past the data it grows
        // with the distance
        let grow = |t: Iv, a: usize| if self.fit_deg[a] == 1 { 1.0 + 2.0 * t.mag() } else { 1.0 };
        let e = self.err * grow(u, 0) * grow(w, 1);
        let (m0, mm0) = markov(self.fit_deg[0]);
        let (m1, mm1) = markov(self.fit_deg[1]);
        let widen = |t: Iv, by: f64| Iv::new(t.lo - by, t.hi + by);
        let (h0, h1) = (Iv::point(self.h[0]), Iv::point(self.h[1]));
        let (r0, r1) = (h0.recip(), h1.recip());
        [
            widen(v, e),
            widen(vu, m0 * e).mul(r0),
            widen(vw, m1 * e).mul(r1),
            widen(vuu, mm0 * e).mul(r0.sqr()),
            widen(vuw, m0 * m1 * e).mul(r0.mul(r1)),
            widen(vww, mm1 * e).mul(r1.sqr()),
        ]
    }

    /// A bilinear patch over `u × w` in local coordinates: its value at
    /// the corners (a bilinear function's extremes over a box), its
    /// derivatives linear in the other coordinate.
    fn bilinear(&self, u: Iv, w: Iv) -> Partials {
        let (c00, c01, c10, c11) = (self.c[0], self.c[1], self.c[2], self.c[3]);
        let f = |u: f64, w: f64| c00 + c01 * w + u * (c10 + c11 * w);
        let corners = [f(u.lo, w.lo), f(u.lo, w.hi), f(u.hi, w.lo), f(u.hi, w.hi)];
        let lo = corners.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = corners.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        // round-off of each evaluation
        let (um, wm) = (u.mag().max(1.0), w.mag().max(1.0));
        let rd = |x: f64| 4.0 * f64::EPSILON * x;
        let ev = rd(c00.abs() + c01.abs() * wm + um * (c10.abs() + c11.abs() * wm));
        let line = |a: f64, b: f64, t: Iv| {
            let (p, q) = (a + b * t.lo, a + b * t.hi);
            let r = rd(a.abs() + b.abs() * t.mag().max(1.0));
            Iv::new(p.min(q) - r, p.max(q) + r)
        };
        [
            Iv::new(lo - ev, hi + ev),
            line(c10, c11, w),
            line(c01, c11, u),
            ZERO,
            Iv::point(c11),
            ZERO,
        ]
    }

    /// The patch over `u × w` in local coordinates by interval Horner, its
    /// value also by the mean value form about the box's middle (the
    /// tighter of the two).
    fn horner(&self, u: Iv, w: Iv) -> Partials {
        let [v, vu, vw, vuu, vuw, vww] = self.horner_raw(u, w);
        let mid = |t: Iv| 0.5 * (t.lo + t.hi);
        let (mu, mw) = (mid(u), mid(w));
        let at = self.horner_raw(Iv::point(mu), Iv::point(mw))[0];
        let mv = at.add(vu.mul(Iv::wide(u.lo - mu, u.hi - mu, 1))).add(vw.mul(Iv::wide(
            w.lo - mw,
            w.hi - mw,
            1,
        )));
        let v = Iv::new(v.lo.max(mv.lo), v.hi.min(mv.hi));
        [v, vu, vw, vuu, vuw, vww]
    }

    fn horner_raw(&self, u: Iv, w: Iv) -> Partials {
        let (n0, n1) = (self.deg[0] + 1, self.deg[1] + 1);
        // per power of u: the row polynomial in w and its derivatives
        let mut rows = [[ZERO; 4]; 3];
        for (p, coef) in self.c.chunks(n1).enumerate().take(n0) {
            let mut a = [ZERO; 4];
            for (q, c) in coef.iter().enumerate() {
                a[q] = Iv::point(*c);
            }
            let r = horner3(&a[..n1], w);
            for (row, ri) in rows.iter_mut().zip(r) {
                row[p] = ri;
            }
        }
        let [v, vu, vuu] = horner3(&rows[0][..n0], u);
        let [vw, vuw, _] = horner3(&rows[1][..n0], u);
        let [vww, _, _] = horner3(&rows[2][..n0], u);
        [v, vu, vw, vuu, vuw, vww]
    }
}

/// The regions of an axis with points `x` that `[a, b]` meets.
fn regions(x: &[f64], a: f64, b: f64) -> std::ops::RangeInclusive<usize> {
    x.partition_point(|p| *p < a)..=x.partition_point(|p| *p <= b)
}

impl Grid2 {
    /// Table `k` on the grid `x × y` (`None`: an axis of fewer than two
    /// points, or not increasing).
    pub(crate) fn new(k: u32, x: &[f64], y: &[f64]) -> Option<Grid2> {
        let ok = |a: &[f64]| a.len() >= 2 && a.windows(2).all(|w| w[0] < w[1]);
        if !ok(x) || !ok(y) {
            return None;
        }
        let n = (x.len() + 1) * (y.len() + 1);
        Some(Grid2 {
            k,
            ax: [x.to_vec(), y.to_vec()],
            patches: (0..n).map(|_| OnceCell::new()).collect(),
        })
    }

    /// The table's enclosures over the box `x × y` (`None`: not bounded).
    pub(crate) fn enclose(&self, model: &dyn ModelFunctions, x: Iv, y: Iv) -> Option<Partials> {
        let finite = |t: Iv| t.lo.is_finite() && t.hi.is_finite();
        if !finite(x) || !finite(y) {
            return None;
        }
        let (rx, ry) = (regions(&self.ax[0], x.lo, x.hi), regions(&self.ax[1], y.lo, y.hi));
        let count = (rx.end() - rx.start() + 1) * (ry.end() - ry.start() + 1);
        if count > MAX_REGIONS {
            return None;
        }
        let ny = self.ax[1].len() + 1;
        let mut out: Option<Partials> = None;
        for i in rx.clone() {
            for j in ry.clone() {
                let patch = self.patches[i * ny + j]
                    .get_or_init(|| Patch::fit(model, self.k, &self.ax, [i, j]))
                    .as_ref()?;
                // the part of the box in this region
                let cut = |t: Iv, a: &[f64], r: usize| {
                    let lo = if r == 0 { t.lo } else { t.lo.max(a[r - 1]) };
                    let hi = if r == a.len() { t.hi } else { t.hi.min(a[r]) };
                    Iv::new(lo, hi.max(lo))
                };
                let e = patch.enclose(cut(x, &self.ax[0], i), cut(y, &self.ax[1], j));
                out = Some(match out {
                    None => e,
                    Some(o) => std::array::from_fn(|q| o[q].hull(e[q])),
                });
            }
        }
        let mut out = out?;
        if rx.start() != rx.end() || ry.start() != ry.end() {
            // across a region's edge the first derivatives may jump
            for s in &mut out[3..] {
                *s = ALL;
            }
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsim_ir::runtime::{EvalInput, Layout};
    use lsim_ir::table::{Interpolation, Outside, TableData};

    /// A model whose only table is `t`, interpolated by the code
    /// generator's own runtime.
    struct One(lsim_codegen::tables::Table, Layout);
    impl ModelFunctions for One {
        fn layout(&self) -> &Layout {
            &self.1
        }
        fn residual(&self, _: &EvalInput<'_>, _: &mut [f64], _: &mut [f64]) {}
        fn jvp(&self, _: &EvalInput<'_>, _: &[f64], _: &mut [f64], _: &mut [f64]) {}
        fn roots(&self, _: &EvalInput<'_>, _: &mut [f64], _: &mut [f64]) {}
        fn vars(&self, _: &EvalInput<'_>, _: &mut [f64], _: &mut [f64]) {}
        fn when(&self, _: &EvalInput<'_>, _: &[f64], _: &mut [f64], _: &mut [f64]) {}
        fn start(&self, _: &[f64], _: &mut [f64], _: &mut [f64]) {}
        fn eval_table(&self, k: u32, args: [f64; 2]) -> Option<(f64, [f64; 2])> {
            (k == 0).then(|| self.0.eval(args))
        }
    }

    fn model(data: &TableData) -> One {
        let layout = Layout {
            n_x: 0,
            n_z: 0,
            n_p: 0,
            n_d: 0,
            n_u: 0,
            n_roots: 0,
            n_whens: 0,
            n_vars: 0,
            n_work: 0,
        };
        One(lsim_codegen::tables::Table::new(data).expect("table"), layout)
    }

    /// Every value and partial derivative sampled in a box lies in its
    /// enclosure, on both interpolations and every outside rule, for boxes
    /// inside a cell, across cells and past the data; the second partials
    /// too, by differences of the derivatives, where the box keeps to one
    /// region.
    #[test]
    fn a_2d_table_is_enclosed_over_a_box() {
        let x: Vec<f64> = vec![0.0, 1.0, 2.5, 3.0, 5.0];
        let y: Vec<f64> = vec![-1.0, 0.0, 0.5, 2.0];
        let values: Vec<f64> = x
            .iter()
            .flat_map(|a| y.iter().map(move |b| (a * 1.3).sin() * 4.0 + a * b * b - 2.0 * b))
            .collect();
        let boxes = [
            ((0.2, 0.7), (0.1, 0.4)),
            ((1.0, 1.0), (0.5, 0.5)),
            ((0.6, 2.8), (-0.5, 1.2)),
            ((-2.0, 0.5), (1.5, 3.0)),
            ((4.0, 7.0), (-3.0, -0.2)),
            ((2.6, 2.9), (2.5, 2.6)),
            ((-1.0, 6.0), (-2.0, 3.0)),
        ];
        for interpolation in [Interpolation::Linear, Interpolation::MonotoneCubic] {
            for outside in [Outside::Clamp, Outside::Linear] {
                let data = TableData {
                    interpolation,
                    outside: [outside, outside],
                    ..TableData::new_2d(x.clone(), y.clone(), values.clone())
                };
                let m = model(&data);
                let g = Grid2::new(0, &x, &y).expect("grid");
                for ((xa, xb), (ya, yb)) in boxes {
                    let what = format!("{interpolation:?} {outside:?} [{xa}, {xb}] × [{ya}, {yb}]");
                    let e = g.enclose(&m, Iv::new(xa, xb), Iv::new(ya, yb)).expect(&what);
                    let one_region =
                        regions(&x, xa, xb).count() == 1 && regions(&y, ya, yb).count() == 1;
                    let mut spread = (f64::INFINITY, f64::NEG_INFINITY);
                    for i in 0..=40 {
                        for j in 0..=40 {
                            let a = xa + (xb - xa) * i as f64 / 40.0;
                            let b = ya + (yb - ya) * j as f64 / 40.0;
                            let (v, d) = m.0.eval([a, b]);
                            let holds = |t: Iv, v: f64| t.lo <= v && v <= t.hi;
                            spread = (spread.0.min(v), spread.1.max(v));
                            assert!(
                                holds(e[0], v),
                                "{what}: value {v} at ({a}, {b}) not in {:?}",
                                e[0]
                            );
                            assert!(
                                holds(e[1], d[0]),
                                "{what}: d/dx {} at ({a}, {b}) not in {:?}",
                                d[0],
                                e[1]
                            );
                            assert!(
                                holds(e[2], d[1]),
                                "{what}: d/dy {} at ({a}, {b}) not in {:?}",
                                d[1],
                                e[2]
                            );
                            if one_region && xa < xb && ya < yb && i < 40 && j < 40 {
                                let dh = 1e-6;
                                let (a2, b2) = ((a + dh).min(xb), (b + dh).min(yb));
                                let (_, dx) = m.0.eval([a2, b]);
                                let (_, dy) = m.0.eval([a, b2]);
                                let fd = |p: f64, q: f64, h: f64| (p - q) / h;
                                let tol = |t: Iv| 1e-4 * (1.0 + t.mag());
                                let near = |t: Iv, v: f64| t.lo - tol(t) <= v && v <= t.hi + tol(t);
                                if a2 > a {
                                    let s = fd(dx[0], d[0], a2 - a);
                                    assert!(near(e[3], s), "{what}: d²/dx² {s} not in {:?}", e[3]);
                                    let s = fd(dx[1], d[1], a2 - a);
                                    assert!(near(e[4], s), "{what}: d²/dxdy {s} not in {:?}", e[4]);
                                }
                                if b2 > b {
                                    let s = fd(dy[1], d[1], b2 - b);
                                    assert!(near(e[5], s), "{what}: d²/dy² {s} not in {:?}", e[5]);
                                }
                            }
                        }
                    }
                    // and tight, for a box inside one region: a bilinear
                    // cell to its values' spread (its extremes at the box's
                    // corners, sampled) and the fit's error bound, a cubic
                    // one to a few times it (interval Horner)
                    if one_region {
                        let (w, s) = (e[0].hi - e[0].lo, spread.1 - spread.0);
                        let most = match interpolation {
                            Interpolation::Linear => s + 1e-6,
                            Interpolation::MonotoneCubic => 4.0 * s + 1e-6,
                        };
                        assert!(w <= most, "{what}: {:?} for a spread {s}", e[0]);
                    }
                }
            }
        }
    }
}
