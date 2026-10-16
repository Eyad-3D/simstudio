//! Interpolants built from [`TableData`] and evaluated with their
//! derivatives: one implementation for the interpreter, preparation, the
//! generated code (through `lsim-codegen`'s `lsim_tab*` symbols) and the
//! solver (DESIGN.md, *Code generation*).
//!
//! * **1-D, monotone cubic.** Hermite cubics with Steffen's slopes (M.
//!   Steffen, "A simple method for monotonic interpolation in one
//!   dimension", Astron. Astrophys. 239, 1990): the slope at a breakpoint
//!   is the parabola's through its neighbours, limited to twice the
//!   smaller adjacent secant and set to zero at a local extremum of the
//!   data. The result is C¹, monotone on every interval where the data
//!   are, reproduces quadratics away from extrema (third-order accurate),
//!   and each slope depends on its two neighbours only.
//! * **2-D, monotone bicubic.** Bicubic Hermite patches on the grid with
//!   zero twist: the node slopes along each axis start as Steffen's along
//!   that grid line, then are reduced (never increased, never changed in
//!   sign) until every cell whose data are monotone along an axis has a
//!   Bézier control net monotone along that axis, which makes the patch
//!   monotone there (the sufficient condition of Carlson and Fritsch,
//!   "Monotone piecewise bicubic interpolation", SIAM J. Numer. Anal. 22,
//!   1985). The node-based Hermite form keeps the surface C¹ across cells
//!   whatever the slopes.
//! * **Linear** (bilinear for 2-D), as today's app interpolates.
//! * **Outside the data**, per axis: `Clamp` holds the edge value (zero
//!   slope across the edge), `Linear` continues along the slope at the
//!   edge (C¹ across it), `Error` evaluates like `Linear` so the solver's
//!   trial points are harmless, and the run loop stops the run where the
//!   axis leaves its data (watched by the table guards).
//!
//! Evaluation is pure, allocation-free and never panics: breakpoints on a
//! uniform grid are found by one multiplication, others by a branch-free
//! binary search; a NaN argument gives NaN.

use crate::table::{Interpolation, Outside, TableData};
use std::sync::atomic::{AtomicUsize, Ordering};

/// One axis of a built table.
#[derive(Debug)]
struct Axis {
    /// breakpoints, strictly increasing (at least one)
    pts: Vec<f64>,
    /// `(x0, 1/h)` when the breakpoints are uniformly spaced
    uniform: Option<(f64, f64)>,
    /// what happens outside
    outside: Outside,
    /// the app's tolerance: float noise this close to an edge is inside
    tol: f64,
    /// the interval found last (uneven breakpoints): a run reads a table
    /// at nearby arguments call after call, and checking it first spares
    /// most searches. Any value is a valid start (it is checked); threads
    /// sharing the table may overwrite each other's, which costs a search
    /// and changes no result.
    hint: AtomicUsize,
}

impl Clone for Axis {
    fn clone(&self) -> Axis {
        Axis {
            pts: self.pts.clone(),
            uniform: self.uniform,
            outside: self.outside,
            tol: self.tol,
            hint: AtomicUsize::new(self.hint.load(Ordering::Relaxed)),
        }
    }
}

impl Axis {
    fn new(pts: &[f64], outside: Outside) -> Axis {
        let n = pts.len();
        let uniform = (n >= 3)
            .then(|| {
                let h = (pts[n - 1] - pts[0]) / (n - 1) as f64;
                let ok = pts.windows(2).all(|w| ((w[1] - w[0]) - h).abs() <= 1e-12 * h.abs());
                ok.then_some((pts[0], 1.0 / h))
            })
            .flatten();
        let (lo, hi) = (pts[0], pts[n - 1]);
        Axis {
            pts: pts.to_vec(),
            uniform,
            outside,
            tol: 1e-9 * lo.abs().max(hi.abs()),
            hint: AtomicUsize::new(0),
        }
    }

    fn lo(&self) -> f64 {
        self.pts[0]
    }

    fn hi(&self) -> f64 {
        self.pts[self.pts.len() - 1]
    }

    /// The interval holding `x` (clamped to the data): `i` with
    /// `pts[i] <= x < pts[i + 1]`, at most `n - 2`.
    #[inline]
    fn interval(&self, x: f64) -> usize {
        let last = self.pts.len() - 2;
        if let Some((x0, inv_h)) = self.uniform {
            // `as` saturates (and maps NaN to 0)
            let i = ((x - x0) * inv_h) as usize;
            return i.min(last);
        }
        // the interval found last, or the next one: exactly the search's
        // answer (the last start at or below x) when x lies in it
        let p = &self.pts;
        let h = self.hint.load(Ordering::Relaxed).min(last);
        for i in [h, h + 1] {
            if i <= last && x >= p[i] && (i == last || x < p[i + 1]) {
                return i;
            }
        }
        // branch-free lower bound over the interval starts
        let p = &self.pts[..=last];
        let (mut lo, mut n) = (0usize, p.len());
        while n > 1 {
            let half = n / 2;
            if x >= p[lo + half] {
                lo += half;
            }
            n -= half;
        }
        self.hint.store(lo, Ordering::Relaxed);
        lo
    }

    /// Where `x` lies: the point to evaluate the data at, the distance
    /// beyond it, and the factor (0 or 1) for derivatives across the edge.
    #[inline]
    fn place(&self, x: f64) -> (f64, f64, f64) {
        let (lo, hi) = (self.lo(), self.hi());
        if x < lo {
            match self.outside {
                Outside::Clamp => (lo, 0.0, 0.0),
                Outside::Linear | Outside::Error => (lo, x - lo, 1.0),
            }
        } else if x > hi {
            match self.outside {
                Outside::Clamp => (hi, 0.0, 0.0),
                Outside::Linear | Outside::Error => (hi, x - hi, 1.0),
            }
        } else {
            (x, 0.0, 1.0)
        }
    }

    /// Positive inside the data, negative outside (the run loop's guard).
    fn guard(&self, x: f64) -> f64 {
        if self.pts.len() < 2 {
            // a single point is a constant: never outside
            return if x.is_nan() { x } else { 1.0 };
        }
        (x - (self.lo() - self.tol)).min((self.hi() + self.tol) - x)
    }
}

/// A table ready to evaluate.
#[derive(Clone, Debug)]
pub struct Table {
    /// 1 or 2 arguments
    dims: usize,
    /// the axes that vary (a 2-D table with a single point along an axis
    /// is evaluated as 1-D along the other)
    ax: Vec<Axis>,
    /// which argument each of `ax` reads
    arg_of: [usize; 2],
    /// per axis of the declared table (for guards)
    guard_axes: Vec<Axis>,
    /// the value when no axis varies
    constant: f64,
    /// 1-D: four power-basis coefficients per interval (local coordinate
    /// `s = x - x_i`); 2-D: sixteen per cell, `a[4p + q]` of `s^p t^q`
    coef: Vec<f64>,
    /// whether every interpolant's rates stay finite over its cells (the
    /// coefficients and the cells' widths are moderate): then a value
    /// inside the data or held at its edge is exactly its polynomial's
    /// (the terms that carry it outside are exact zeros), which
    /// [`Table::value`] uses
    tame: bool,
    /// the partial derivative this table gives instead of its value
    /// ([`Table::derivative`]; `[0, 0]`: its value)
    order: [u8; 2],
    /// interpolation, for [`Table::continuity`]
    cubic: bool,
}

/// Steffen's slopes for the data `(x, f)`, at least two points.
pub fn steffen_slopes(x: &[f64], f: &[f64]) -> Vec<f64> {
    let n = x.len();
    debug_assert!(n >= 2 && f.len() == n);
    let h: Vec<f64> = x.windows(2).map(|w| w[1] - w[0]).collect();
    let s: Vec<f64> = (0..n - 1).map(|i| (f[i + 1] - f[i]) / h[i]).collect();
    if n == 2 {
        return vec![s[0], s[0]];
    }
    let sign = |v: f64| {
        if v > 0.0 {
            1.0
        } else if v < 0.0 {
            -1.0
        } else {
            0.0
        }
    };
    let mut d = vec![0.0; n];
    for i in 1..n - 1 {
        let p = (s[i - 1] * h[i] + s[i] * h[i - 1]) / (h[i - 1] + h[i]);
        d[i] = (sign(s[i - 1]) + sign(s[i])) * s[i - 1].abs().min(s[i].abs()).min(0.5 * p.abs());
    }
    // the ends: the parabola through the first (last) three points, kept
    // to the end secant's sign and to at most twice it (Steffen, eq. 26-27)
    let end = |s0: f64, s1: f64, h0: f64, h1: f64| {
        let p = s0 * (1.0 + h0 / (h0 + h1)) - s1 * h0 / (h0 + h1);
        if p * s0 <= 0.0 {
            0.0
        } else if p.abs() > 2.0 * s0.abs() {
            2.0 * s0
        } else {
            p
        }
    };
    d[0] = end(s[0], s[1], h[0], h[1]);
    d[n - 1] = end(s[n - 2], s[n - 3], h[n - 2], h[n - 3]);
    d
}

/// Power-basis coefficients of the cubic Hermite on `[0, h]`.
fn hermite(f0: f64, f1: f64, d0: f64, d1: f64, h: f64) -> [f64; 4] {
    let m = (f1 - f0) / h;
    [f0, d0, (3.0 * m - 2.0 * d0 - d1) / h, (d0 + d1 - 2.0 * m) / (h * h)]
}

/// The Hermite basis on `[0, h]` in power form: the polynomials that
/// multiply f0, f1, d0, d1.
fn hermite_basis(h: f64) -> [[f64; 4]; 4] {
    let (h2, h3) = (h * h, h * h * h);
    [
        [1.0, 0.0, -3.0 / h2, 2.0 / h3],
        [0.0, 0.0, 3.0 / h2, -2.0 / h3],
        [0.0, 1.0, -2.0 / h, 1.0 / h2],
        [0.0, 0.0, -1.0 / h, 1.0 / h2],
    ]
}

/// One condition `base + Σ c_k g_k >= 0` on node slopes `g_k`; when it
/// fails, the terms that work against it are scaled down until it holds.
/// Returns whether anything changed.
fn enforce(base: f64, terms: &mut [(f64, &mut f64)]) -> bool {
    let mut total = base;
    let (mut help, mut harm, mut scale) = (base, 0.0, base.abs());
    for (c, g) in terms.iter() {
        let v = *c * **g;
        total += v;
        scale += v.abs();
        if v >= 0.0 {
            help += v;
        } else {
            harm -= v;
        }
    }
    if total >= -1e-13 * scale || harm == 0.0 {
        return false;
    }
    let lambda = (help / harm).clamp(0.0, 1.0);
    for (c, g) in terms.iter_mut() {
        if *c * **g < 0.0 {
            **g *= lambda;
        }
    }
    true
}

/// Reduces the node slopes of a bicubic Hermite surface with zero twist
/// until each cell's Bézier net is monotone along every axis along which
/// the cell's data are monotone.
fn monotone_2d(x: &[f64], y: &[f64], f: &[f64], fx: &mut [f64], fy: &mut [f64]) {
    let (nx, ny) = (x.len(), y.len());
    let k = |i: usize, j: usize| i * ny + j;
    // the signs along which a pair of edge differences is monotone
    let signs = |a: f64, b: f64| -> &'static [f64] {
        if a >= 0.0 && b >= 0.0 && (a > 0.0 || b > 0.0) {
            &[1.0]
        } else if a <= 0.0 && b <= 0.0 && (a < 0.0 || b < 0.0) {
            &[-1.0]
        } else if a == 0.0 && b == 0.0 {
            &[1.0, -1.0]
        } else {
            &[]
        }
    };
    let mut converged = false;
    for _pass in 0..64 {
        let mut changed = false;
        for i in 0..nx - 1 {
            for j in 0..ny - 1 {
                let (hx, hy) = (x[i + 1] - x[i], y[j + 1] - y[j]);
                let (k00, k10, k01, k11) = (k(i, j), k(i + 1, j), k(i, j + 1), k(i + 1, j + 1));
                // along x: rows of the net at y_j (edge), the two inner
                // rows, and at y_{j+1} (edge)
                let (d0, d1) = (f[k10] - f[k00], f[k11] - f[k01]);
                for &s in signs(d0, d1) {
                    let (cx, cy) = (s * hx / 3.0, s * hy / 3.0);
                    let [a, b, c, d] = pick4(fx, [k00, k10, k01, k11]);
                    changed |= enforce(s * d0, &mut [(-cx, a), (-cx, b)]);
                    changed |= enforce(s * d1, &mut [(-cx, c), (-cx, d)]);
                    let [a, b, c, d] = pick4(fx, [k00, k10, k01, k11]);
                    let [e, g, _, _] = pick4(fy, [k10, k00, k01, k11]);
                    changed |= enforce(s * d0, &mut [(-cx, a), (-cx, b), (cy, e), (-cy, g)]);
                    let [e, g, _, _] = pick4(fy, [k11, k01, k00, k10]);
                    changed |= enforce(s * d1, &mut [(-cx, c), (-cx, d), (-cy, e), (cy, g)]);
                }
                // along y, the same with the axes' roles swapped
                let (g0, g1) = (f[k01] - f[k00], f[k11] - f[k10]);
                for &s in signs(g0, g1) {
                    let (cx, cy) = (s * hx / 3.0, s * hy / 3.0);
                    let [a, b, c, d] = pick4(fy, [k00, k01, k10, k11]);
                    changed |= enforce(s * g0, &mut [(-cy, a), (-cy, b)]);
                    changed |= enforce(s * g1, &mut [(-cy, c), (-cy, d)]);
                    let [a, b, c, d] = pick4(fy, [k00, k01, k10, k11]);
                    let [e, g, _, _] = pick4(fx, [k01, k00, k10, k11]);
                    changed |= enforce(s * g0, &mut [(-cy, a), (-cy, b), (cx, e), (-cx, g)]);
                    let [e, g, _, _] = pick4(fx, [k11, k10, k00, k01]);
                    changed |= enforce(s * g1, &mut [(-cy, c), (-cy, d), (-cx, e), (cx, g)]);
                }
            }
        }
        if !changed {
            converged = true;
            break;
        }
    }
    if !converged {
        // a last resort that always satisfies every condition: flat slopes
        // (the patches then blend the corner values monotonically)
        fx.iter_mut().for_each(|v| *v = 0.0);
        fy.iter_mut().for_each(|v| *v = 0.0);
    }
}

/// The `k`-th derivative of the cubic `c[0] + c[1] s + c[2] s² + c[3] s³`
/// at `s` (the first derivative by the same operations as the value's
/// rate).
#[inline]
fn poly_der(c: &[f64], k: u8, s: f64) -> f64 {
    match k {
        0 => c[0] + s * (c[1] + s * (c[2] + s * c[3])),
        1 => c[1] + s * (2.0 * c[2] + s * (3.0 * c[3])),
        2 => 2.0 * c[2] + s * (6.0 * c[3]),
        3 => 6.0 * c[3],
        _ => 0.0,
    }
}

/// The partial derivative of order `(m, n)` of the bicubic
/// `Σ a[4p + q] s^p t^q` at `(s, t)`: rows in `t` first, then `s`, as the
/// value is evaluated.
#[inline]
fn poly_der2(a: &[f64], m: u8, n: u8, s: f64, t: f64) -> f64 {
    if m > 3 || n > 3 {
        return 0.0;
    }
    let mut r = [0.0; 4];
    for (p, rp) in r.iter_mut().enumerate() {
        *rp = poly_der(&a[4 * p..4 * p + 4], n, t);
    }
    poly_der(&r, m, s)
}

/// Four distinct mutable entries of a slice.
fn pick4(v: &mut [f64], idx: [usize; 4]) -> [&mut f64; 4] {
    v.get_disjoint_mut(idx).expect("four distinct nodes")
}

impl Table {
    /// Builds the interpolant. The data must pass [`TableData::check`].
    pub fn new(data: &TableData) -> Result<Table, String> {
        data.check()?;
        let dims = data.dims();
        let guard_axes: Vec<Axis> = if dims == 1 {
            vec![Axis::new(&data.x, data.outside[0])]
        } else {
            vec![Axis::new(&data.x, data.outside[0]), Axis::new(&data.y, data.outside[1])]
        };
        let varying: Vec<usize> = (0..dims).filter(|&a| guard_axes[a].pts.len() >= 2).collect();
        let cubic = data.interpolation == Interpolation::MonotoneCubic;
        let mut t = Table {
            dims,
            ax: varying.iter().map(|&a| guard_axes[a].clone()).collect(),
            arg_of: [varying.first().copied().unwrap_or(0), varying.get(1).copied().unwrap_or(0)],
            guard_axes,
            constant: data.values[0],
            coef: vec![],
            tame: false,
            order: [0, 0],
            cubic,
        };
        match varying.len() {
            0 => {}
            1 => {
                // a 1-D table, or a 2-D one with a single point along an axis
                // (a 2-D table with one point along an axis keeps its values
                // in the order of the other)
                let pts: &[f64] = if dims == 2 && varying[0] == 1 { &data.y } else { &data.x };
                let vals = data.values.clone();
                let d = if cubic {
                    steffen_slopes(pts, &vals)
                } else {
                    let s: Vec<f64> = (0..pts.len() - 1)
                        .map(|i| (vals[i + 1] - vals[i]) / (pts[i + 1] - pts[i]))
                        .collect();
                    // linear: each interval's own secant (d1 unused below)
                    s
                };
                for i in 0..pts.len() - 1 {
                    let h = pts[i + 1] - pts[i];
                    let c = if cubic {
                        hermite(vals[i], vals[i + 1], d[i], d[i + 1], h)
                    } else {
                        [vals[i], d[i], 0.0, 0.0]
                    };
                    t.coef.extend_from_slice(&c);
                }
            }
            _ => {
                let (x, y, f) = (&data.x, &data.y, &data.values);
                let (nx, ny) = (x.len(), y.len());
                let k = |i: usize, j: usize| i * ny + j;
                let (mut fx, mut fy) = (vec![0.0; nx * ny], vec![0.0; nx * ny]);
                if cubic {
                    for j in 0..ny {
                        let line: Vec<f64> = (0..nx).map(|i| f[k(i, j)]).collect();
                        for (i, d) in steffen_slopes(x, &line).into_iter().enumerate() {
                            fx[k(i, j)] = d;
                        }
                    }
                    for i in 0..nx {
                        let line = &f[k(i, 0)..k(i, 0) + ny];
                        for (j, d) in steffen_slopes(y, line).into_iter().enumerate() {
                            fy[k(i, j)] = d;
                        }
                    }
                    monotone_2d(x, y, f, &mut fx, &mut fy);
                }
                for i in 0..nx - 1 {
                    for j in 0..ny - 1 {
                        let (hx, hy) = (x[i + 1] - x[i], y[j + 1] - y[j]);
                        let mut a = [0.0; 16];
                        if cubic {
                            let (bx, by) = (hermite_basis(hx), hermite_basis(hy));
                            // (value, d/dx, d/dy) at the four corners
                            for (ci, cj) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                                let n = k(i + ci, j + cj);
                                let terms = [
                                    (f[n], &bx[ci], &by[cj]),
                                    (fx[n], &bx[2 + ci], &by[cj]),
                                    (fy[n], &bx[ci], &by[2 + cj]),
                                ];
                                for (w, ps, qs) in terms {
                                    for p in 0..4 {
                                        for q in 0..4 {
                                            a[4 * p + q] += w * ps[p] * qs[q];
                                        }
                                    }
                                }
                            }
                        } else {
                            let (f00, f10, f01, f11) =
                                (f[k(i, j)], f[k(i + 1, j)], f[k(i, j + 1)], f[k(i + 1, j + 1)]);
                            a[0] = f00;
                            a[4] = (f10 - f00) / hx;
                            a[1] = (f01 - f00) / hy;
                            a[5] = (f11 - f10 - f01 + f00) / (hx * hy);
                        }
                        t.coef.extend_from_slice(&a);
                    }
                }
            }
        }
        let width = |a: &Axis| a.pts.windows(2).map(|w| w[1] - w[0]).fold(0.0, f64::max);
        t.tame = t.coef.iter().all(|c| c.abs() < 1e100)
            && t.ax.iter().all(|a| width(a) < 1e30 && a.pts.iter().all(|p| p.abs() < 1e100));
        Ok(t)
    }

    /// How many arguments it takes.
    pub fn dims(&self) -> usize {
        self.dims
    }

    /// The value and its partial derivatives (`d[1]` is zero for 1-D); for
    /// a [`Table::derivative`], that derivative and its partial
    /// derivatives.
    #[inline]
    pub fn eval(&self, args: [f64; 2]) -> (f64, [f64; 2]) {
        if self.order != [0, 0] {
            return self.eval_order(args, [0, 0]);
        }
        self.eval0(args)
    }

    /// The value and its partial derivatives.
    #[inline]
    fn eval0(&self, args: [f64; 2]) -> (f64, [f64; 2]) {
        match self.ax.len() {
            0 => (
                if args[..self.dims].iter().any(|a| a.is_nan()) { f64::NAN } else { self.constant },
                [0.0; 2],
            ),
            1 => {
                let a = self.arg_of[0];
                let (v, d) = self.eval1(args[a]);
                let mut g = [0.0; 2];
                g[a] = d;
                // a NaN in the ignored argument still poisons the result
                let other = if self.dims == 2 { args[1 - a] } else { 0.0 };
                (if other.is_nan() { other } else { v }, g)
            }
            _ => {
                let (v, dx, dy) = self.eval2(args[0], args[1]);
                (v, [dx, dy])
            }
        }
    }

    /// The value alone: bitwise `self.eval(args).0` (the same code), without
    /// the rates where they cannot change it: inside the data, or held at
    /// its edge, the terms that carry a value across the edge are exact
    /// zeros, and a non-zero value is the polynomial's.
    #[inline]
    pub fn value(&self, args: [f64; 2]) -> f64 {
        if self.order != [0, 0] {
            return self.partial(args, self.order);
        }
        match self.ax.len() {
            0 => self.eval0(args).0,
            1 => {
                let a = self.arg_of[0];
                let (v, _) = self.eval1_with::<false>(args[a]);
                // a NaN in the ignored argument still poisons the result
                let other = if self.dims == 2 { args[1 - a] } else { 0.0 };
                if other.is_nan() { other } else { v }
            }
            _ => self.eval2_with::<false>(args[0], args[1]).0,
        }
    }

    /// The same interpolant giving its partial derivative of `order`
    /// (along each argument) instead of its value: what the generated code
    /// calls for a derivative read ([`crate::Expr::Table`]'s `order`).
    /// [`Table::eval`] and [`Table::value`] then give that derivative (and
    /// its partial derivatives). The coefficients are the same, built by
    /// the same code from the same data.
    pub fn derivative(&self, order: [u8; 2]) -> Table {
        let mut t = self.clone();
        t.order = [self.order[0].saturating_add(order[0]), self.order[1].saturating_add(order[1])];
        t
    }

    /// The partial derivative of `order` (on top of this table's own, see
    /// [`Table::derivative`]) at `args`, and its partial derivatives: the
    /// interpolant's own, exact up to rounding. Order `[0, 0]` is
    /// [`Table::eval`]; a first derivative is bitwise the rate
    /// [`Table::eval`] gives with the value.
    ///
    /// Where a derivative jumps (the second derivative of a monotone cubic
    /// at its breakpoints, the first of a linear table at its breakpoints,
    /// the first of any table at a clamped edge), it is the derivative of
    /// the cell the argument lies in: at a breakpoint inside the data the
    /// cell to its right (`x_i <= x < x_{i+1}`; on an evenly spaced axis
    /// the cell search is one multiplication, whose rounding may put a
    /// point within an ulp or so of a breakpoint in the cell before it),
    /// at the data's last point
    /// the last cell, so on the data's edges the side inside the data.
    /// Outside the data a clamped axis is constant (every derivative along
    /// it 0), a linear (or `Error`) axis continues along its edge slope
    /// (first derivative that slope, higher ones 0).
    pub fn eval_order(&self, args: [f64; 2], order: [u8; 2]) -> (f64, [f64; 2]) {
        // (orders saturate: past the fourth every derivative is 0 anyway)
        let o = [self.order[0].saturating_add(order[0]), self.order[1].saturating_add(order[1])];
        if o == [0, 0] {
            return self.eval0(args);
        }
        let v = self.partial(args, o);
        let gx = self.partial(args, [o[0].saturating_add(1), o[1]]);
        let gy =
            if self.dims == 2 { self.partial(args, [o[0], o[1].saturating_add(1)]) } else { 0.0 };
        (v, [gx, gy])
    }

    /// The partial derivative of order `o` (not `[0, 0]`) at `args`.
    fn partial(&self, args: [f64; 2], o: [u8; 2]) -> f64 {
        let nan = args[..self.dims].iter().any(|a| a.is_nan());
        if self.dims == 1 && o[1] > 0 {
            return if nan { f64::NAN } else { 0.0 };
        }
        // first derivatives: the rates of the value, the same code (also
        // where an argument is NaN: a rate along an axis with one point is
        // 0 whatever the other argument is)
        if o[0] + o[1] == 1 {
            return self.eval0(args).1[usize::from(o[1] == 1)];
        }
        if nan {
            return f64::NAN;
        }
        match self.ax.len() {
            0 => 0.0,
            1 => {
                let a = self.arg_of[0];
                if self.dims == 2 && o[1 - a] > 0 {
                    // along an axis with a single point: constant
                    return 0.0;
                }
                let ax = &self.ax[0];
                let x = args[a];
                let (xc, _, _) = ax.place(x);
                if !(ax.lo() <= x && x <= ax.hi()) {
                    // (order 2 and more) outside: held or continued linearly
                    return 0.0;
                }
                let i = ax.interval(xc);
                let c = &self.coef[4 * i..4 * i + 4];
                poly_der(c, o[a], xc - ax.pts[i])
            }
            _ => {
                let (axx, axy) = (&self.ax[0], &self.ax[1]);
                let (x, y) = (args[0], args[1]);
                let (xc, dx, ex) = axx.place(x);
                let (yc, dy, ey) = axy.place(y);
                let (i, j) = (axx.interval(xc), axy.interval(yc));
                let ny1 = axy.pts.len() - 1;
                let a = &self.coef[16 * (i * ny1 + j)..16 * (i * ny1 + j) + 16];
                let (s, t) = (xc - axx.pts[i], yc - axy.pts[j]);
                // per axis, the derivative of order k along it is
                // w[0]·(the polynomial's of order k) + w[1]·(of order k + 1)
                let w = |k: u8, d: f64, e: f64, inside: bool| -> [f64; 2] {
                    match k {
                        0 => [1.0, e * d],
                        1 => [e, 0.0],
                        _ => [if inside { 1.0 } else { 0.0 }, 0.0],
                    }
                };
                let wx = w(o[0], dx, ex, axx.lo() <= x && x <= axx.hi());
                let wy = w(o[1], dy, ey, axy.lo() <= y && y <= axy.hi());
                let mut sum = 0.0;
                for (p, wp) in wx.iter().enumerate() {
                    for (q, wq) in wy.iter().enumerate() {
                        let (m, n) = (o[0].saturating_add(p as u8), o[1].saturating_add(q as u8));
                        if *wp * *wq != 0.0 {
                            sum += *wp * *wq * poly_der2(a, m, n, s, t);
                        }
                    }
                }
                sum
            }
        }
    }

    /// How many times the interpolant is continuously differentiable
    /// everywhere, inside and outside its data (`u32::MAX`: it is
    /// constant). A monotone cubic (1-D or bicubic) is C¹ where every axis
    /// that varies continues linearly outside (`Linear`, `Error`), C⁰ when
    /// an axis is clamped (a kink at its edge); a linear table is C⁰ (a
    /// kink at its breakpoints).
    pub fn continuity(&self) -> u32 {
        if self.ax.is_empty() {
            return u32::MAX;
        }
        let clamped = self.ax.iter().any(|a| a.outside == Outside::Clamp);
        if self.cubic && !clamped { 1 } else { 0 }
    }
}

/// [`Table::continuity`] of the interpolant `data` makes, from its rules
/// and point counts alone (nothing is built): what preparation and the run
/// loop judge a table's smoothness by.
pub fn continuity(data: &TableData) -> u32 {
    let axes: [(&[f64], Outside); 2] = [(&data.x, data.outside[0]), (&data.y, data.outside[1])];
    let varying: Vec<Outside> =
        axes[..data.dims()].iter().filter(|(p, _)| p.len() >= 2).map(|(_, o)| *o).collect();
    if varying.is_empty() {
        return u32::MAX;
    }
    let cubic = data.interpolation == Interpolation::MonotoneCubic;
    if cubic && !varying.contains(&Outside::Clamp) { 1 } else { 0 }
}

impl Table {
    /// The partial derivative this table gives instead of its value
    /// (`[0, 0]`: its value).
    pub fn order(&self) -> [u8; 2] {
        self.order
    }

    #[inline]
    fn eval1(&self, x: f64) -> (f64, f64) {
        self.eval1_with::<true>(x)
    }

    /// The value and rate at `x`; without `RATES`, where the rate cannot
    /// change the value (`tame`, inside the data or held at its edge, a
    /// non-zero value), the value alone (the rate then 0).
    #[inline(always)]
    fn eval1_with<const RATES: bool>(&self, x: f64) -> (f64, f64) {
        let ax = &self.ax[0];
        let (xc, dx, e) = ax.place(x);
        let i = ax.interval(xc);
        let c = &self.coef[4 * i..4 * i + 4];
        let s = xc - ax.pts[i];
        let v = c[0] + s * (c[1] + s * (c[2] + s * c[3]));
        if !RATES && self.tame && (e == 0.0 || dx == 0.0) && v != 0.0 {
            // `v + e * d * dx` below is `v + ±0`
            return (v, 0.0);
        }
        let d = c[1] + s * (2.0 * c[2] + s * (3.0 * c[3]));
        (v + e * d * dx, e * d)
    }

    #[inline]
    fn eval2(&self, x: f64, y: f64) -> (f64, f64, f64) {
        self.eval2_with::<true>(x, y)
    }

    /// [`Table::eval1_with`] in 2-D.
    #[inline(always)]
    fn eval2_with<const RATES: bool>(&self, x: f64, y: f64) -> (f64, f64, f64) {
        let (axx, axy) = (&self.ax[0], &self.ax[1]);
        let (xc, dx, ex) = axx.place(x);
        let (yc, dy, ey) = axy.place(y);
        let (i, j) = (axx.interval(xc), axy.interval(yc));
        let ny1 = axy.pts.len() - 1;
        let a = &self.coef[16 * (i * ny1 + j)..16 * (i * ny1 + j) + 16];
        let (s, t) = (xc - axx.pts[i], yc - axy.pts[j]);
        // row polynomials in t
        let mut r = [0.0; 4];
        for (p, rp) in r.iter_mut().enumerate() {
            let q = &a[4 * p..4 * p + 4];
            *rp = q[0] + t * (q[1] + t * (q[2] + t * q[3]));
        }
        let v = r[0] + s * (r[1] + s * (r[2] + s * r[3]));
        if !RATES && self.tame && (ex == 0.0 || dx == 0.0) && (ey == 0.0 || dy == 0.0) && v != 0.0 {
            // every term added to `v` below is `±0`
            return (v, 0.0, 0.0);
        }
        // and their t-derivatives
        let mut rt = [0.0; 4];
        for (p, rp) in rt.iter_mut().enumerate() {
            let q = &a[4 * p..4 * p + 4];
            *rp = q[1] + t * (2.0 * q[2] + t * (3.0 * q[3]));
        }
        let vx = r[1] + s * (2.0 * r[2] + s * (3.0 * r[3]));
        let vy = rt[0] + s * (rt[1] + s * (rt[2] + s * rt[3]));
        let vxy = rt[1] + s * (2.0 * rt[2] + s * (3.0 * rt[3]));
        let val = v + ex * vx * dx + ey * vy * dy + ex * ey * vxy * dx * dy;
        let gx = ex * (vx + ey * vxy * dy);
        let gy = ey * (vy + ex * vxy * dx);
        (val, gx, gy)
    }

    /// The guard of axis `axis`'s argument `a`: positive inside the data
    /// (within float noise of an edge counts as inside), negative outside.
    pub fn guard(&self, axis: usize, a: f64) -> f64 {
        self.guard_axes.get(axis).map_or(f64::NAN, |ax| ax.guard(a))
    }

    /// An axis's breakpoints.
    pub fn points(&self, axis: usize) -> &[f64] {
        &self.guard_axes[axis].pts
    }

    /// The data range of an axis.
    pub fn range(&self, axis: usize) -> (f64, f64) {
        let ax = &self.guard_axes[axis];
        (ax.lo(), ax.hi())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steffen_reproduces_lines_and_parabolas() {
        let x = [0.0, 1.0, 2.5, 3.0, 4.5];
        let line: Vec<f64> = x.iter().map(|v| 2.0 * v - 1.0).collect();
        assert!(steffen_slopes(&x, &line).iter().all(|d| (d - 2.0).abs() < 1e-14));
        // a monotone parabola: the interior slopes are exact
        let par: Vec<f64> = x.iter().map(|v| v * v + v).collect();
        let d = steffen_slopes(&x, &par);
        for i in 1..x.len() - 1 {
            assert!((d[i] - (2.0 * x[i] + 1.0)).abs() < 1e-12, "{i}: {}", d[i]);
        }
    }

    #[test]
    fn evaluates_at_breakpoints_and_outside() {
        let mut data = TableData::new_1d(vec![0.0, 1.0, 3.0], vec![0.0, 1.0, 5.0]);
        data.outside = [Outside::Clamp, Outside::Clamp];
        let t = Table::new(&data).unwrap();
        assert_eq!(t.points(0), &[0.0, 1.0, 3.0]);
        for (x, v) in [(0.0, 0.0), (1.0, 1.0), (3.0, 5.0)] {
            assert!((t.eval([x, 0.0]).0 - v).abs() < 1e-14);
        }
        assert_eq!(t.eval([-1.0, 0.0]), (0.0, [0.0; 2]));
        assert_eq!(t.eval([4.0, 0.0]).0, 5.0);
        data.outside = [Outside::Linear, Outside::Clamp];
        let t = Table::new(&data).unwrap();
        let (v, d) = t.eval([4.0, 0.0]);
        let (v3, d3) = t.eval([3.0, 0.0]);
        assert!((v - (v3 + d3[0])).abs() < 1e-14 && d == d3);
        assert!(t.eval([f64::NAN, 0.0]).0.is_nan());
        assert!(t.guard(0, 1.0) > 0.0 && t.guard(0, 3.5) < 0.0 && t.guard(0, 3.0 + 1e-12) > 0.0);
    }

    /// The interval search started at the last interval found gives the
    /// plain search's interval, whatever was found before: on uneven axes,
    /// along random walks and jumps, at the breakpoints, outside, at NaN.
    #[test]
    fn the_hinted_interval_is_the_searched_one() {
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut rnd = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        let search = |p: &[f64], x: f64| -> usize {
            let last = p.len() - 2;
            (0..=last).rev().find(|&k| x >= p[k]).unwrap_or(0)
        };
        let mut checked = 0;
        for case in 0..200 {
            let n = 2 + case % 40;
            let mut x = -5.0 * rnd();
            let pts: Vec<f64> = (0..n)
                .map(|_| {
                    let v = x;
                    x += 0.01 + 3.0 * rnd() * rnd();
                    v
                })
                .collect();
            let ax = Axis::new(&pts, Outside::Linear);
            assert!(ax.uniform.is_none() || n < 3, "uneven");
            let (lo, hi) = (pts[0], pts[n - 1]);
            let mut at = lo + (hi - lo) * rnd();
            for k in 0..400 {
                at = match k % 7 {
                    0 => lo + (hi - lo) * rnd(),
                    1 => pts[(rnd() * n as f64) as usize % n],
                    2 => f64::NAN,
                    3 => lo - 1.0,
                    _ => (at + (hi - lo) * 0.02 * (rnd() - 0.5)).clamp(lo, hi),
                };
                let want = search(&pts, at);
                assert_eq!(ax.interval(at), want, "case {case}, x = {at}, axis {pts:?}");
                checked += 1;
            }
        }
        assert!(checked >= 80_000);
    }

    /// The value alone is bitwise the value `eval` gives, inside the data,
    /// on its edges, outside it, at NaN and at signed zeros, on random
    /// 1-D and 2-D tables of every interpolation and outside rule, and on
    /// tables whose value is zero somewhere.
    #[test]
    fn the_value_alone_is_bitwise_evals() {
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        let mut rnd = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        let outs = [Outside::Clamp, Outside::Linear, Outside::Error];
        let mut checked = 0;
        for case in 0..300 {
            let axis = |r: &mut dyn FnMut() -> f64, n: usize| -> Vec<f64> {
                let mut x = -3.0 * r();
                (0..n)
                    .map(|_| {
                        let v = x;
                        x += 0.05 + 2.0 * r();
                        v
                    })
                    .collect()
            };
            let nx = 1 + (case % 6);
            let x = axis(&mut rnd, nx);
            let zero = case % 4 == 0;
            let val =
                |r: &mut dyn FnMut() -> f64| if zero && r() < 0.5 { 0.0 } else { 4.0 * r() - 2.0 };
            let mut data = if case % 2 == 0 {
                let y = (0..nx).map(|_| val(&mut rnd)).collect();
                TableData::new_1d(x.clone(), y)
            } else {
                let ny = 1 + (case / 2) % 5;
                let y = axis(&mut rnd, ny);
                let v = (0..nx * ny).map(|_| val(&mut rnd)).collect();
                TableData::new_2d(x.clone(), y, v)
            };
            if case % 3 == 0 {
                data.interpolation = Interpolation::Linear;
            }
            data.outside = [outs[case % 3], outs[(case / 3) % 3]];
            let Ok(t) = Table::new(&data) else { continue };
            let mut probe = |a: f64, b: f64| {
                let (v, _) = t.eval([a, b]);
                let w = t.value([a, b]);
                assert!(
                    v.to_bits() == w.to_bits(),
                    "case {case} at ({a}, {b}): eval {v:e} ({:x}), value {w:e} ({:x})",
                    v.to_bits(),
                    w.to_bits()
                );
                checked += 1;
            };
            let pts_x = data.x.clone();
            let pts_y = if data.y.is_empty() { vec![0.0] } else { data.y.clone() };
            let specials = [f64::NAN, 0.0, -0.0, 1e300, -1e300];
            for &a in pts_x.iter().chain(&specials) {
                for &b in pts_y.iter().chain(&specials) {
                    probe(a, b);
                }
            }
            for _ in 0..200 {
                let a = -5.0 + 15.0 * rnd();
                let b = -5.0 + 15.0 * rnd();
                probe(a, b);
            }
        }
        assert!(checked > 50_000, "{checked}");
    }

    /// Random 1-D and 2-D tables of every interpolation and outside rule.
    fn random_tables(cases: usize) -> Vec<TableData> {
        let mut seed = 0x51f1_5eed_c0ff_ee11u64;
        let mut rnd = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        let outs = [Outside::Clamp, Outside::Linear, Outside::Error];
        let mut out = vec![];
        for case in 0..cases {
            let axis = |r: &mut dyn FnMut() -> f64, n: usize| -> Vec<f64> {
                let mut x = -2.0 * r();
                (0..n)
                    .map(|_| {
                        let v = x;
                        x += 0.2 + 1.5 * r();
                        v
                    })
                    .collect()
            };
            let nx = 2 + case % 5;
            let x = axis(&mut rnd, nx);
            let mut data = if case % 2 == 0 {
                let y = (0..nx).map(|_| 4.0 * rnd() - 2.0).collect();
                TableData::new_1d(x, y)
            } else {
                let ny = 2 + (case / 2) % 4;
                let y = axis(&mut rnd, ny);
                let v = (0..nx * ny).map(|_| 4.0 * rnd() - 2.0).collect();
                TableData::new_2d(x, y, v)
            };
            if case % 3 == 0 {
                data.interpolation = Interpolation::Linear;
            }
            data.outside = [outs[case % 3], outs[(case / 3) % 3]];
            out.push(data);
        }
        // an axis with a single point: constant along it
        out.push(TableData::new_2d(vec![1.0], vec![0.0, 1.0, 2.0], vec![0.0, 1.0, 4.0]));
        out.push(TableData::new_2d(vec![0.0, 1.0, 2.0], vec![1.0], vec![0.0, 1.0, 4.0]));
        out.push(TableData::new_2d(vec![1.0], vec![2.0], vec![3.0]));
        out
    }

    /// Points strictly inside cells, and outside the data, away from every
    /// breakpoint by `gap` at least.
    fn probes(t: &Table, gap: f64) -> Vec<[f64; 2]> {
        let cuts = |pts: &[f64]| -> Vec<f64> {
            let mut v = vec![pts[0] - 0.7, pts[pts.len() - 1] + 0.45];
            for w in pts.windows(2) {
                for f in [0.13, 0.5, 0.91] {
                    v.push(w[0] + f * (w[1] - w[0]));
                }
            }
            v.retain(|x| pts.iter().all(|p| (x - p).abs() > gap));
            v
        };
        let xs = cuts(t.points(0));
        let ys = if t.dims() == 2 { cuts(t.points(1)) } else { vec![0.0] };
        xs.iter().flat_map(|&x| ys.iter().map(move |&y| [x, y])).collect()
    }

    /// Every partial derivative up to the fourth order along each axis
    /// against central differences of the order below, inside the cells
    /// and outside the data: the derivatives are the interpolant's own.
    #[test]
    fn derivatives_of_every_order_match_differences() {
        let mut checked = 0;
        for data in random_tables(120) {
            let t = Table::new(&data).unwrap();
            assert_eq!(continuity(&data), t.continuity(), "{data:?}");
            let h = 1e-6;
            for at in probes(&t, 1e-3) {
                for o0 in 0..4u8 {
                    for o1 in 0..(if t.dims() == 2 { 4u8 } else { 1 }) {
                        let (v, g) = t.eval_order(at, [o0, o1]);
                        for j in 0..t.dims() {
                            let mut a = at;
                            let mut b = at;
                            a[j] += h;
                            b[j] -= h;
                            let fd = (t.eval_order(a, [o0, o1]).0 - t.eval_order(b, [o0, o1]).0)
                                / (2.0 * h);
                            let scale = 1.0 + g[j].abs() + v.abs();
                            assert!(
                                (fd - g[j]).abs() < 2e-6 * scale,
                                "{data:?} at {at:?}, order [{o0}, {o1}], axis {j}: {fd} vs {}",
                                g[j]
                            );
                            checked += 1;
                        }
                    }
                }
            }
        }
        assert!(checked > 20_000, "{checked}");
    }

    /// A first derivative is bitwise the rate the value comes with, and a
    /// derivative table ([`Table::derivative`]) gives what
    /// [`Table::eval_order`] gives, by `eval` and by `value`.
    #[test]
    fn first_derivatives_are_the_values_rates_bitwise() {
        for data in random_tables(90) {
            let t = Table::new(&data).unwrap();
            let mut pts: Vec<[f64; 2]> = probes(&t, 0.0);
            // the breakpoints themselves, and NaN
            for &x in t.points(0) {
                pts.push([x, if t.dims() == 2 { t.points(1)[0] } else { 0.0 }]);
            }
            pts.push([f64::NAN, 0.5]);
            pts.push([0.5, f64::NAN]);
            for at in pts {
                let (_, g) = t.eval(at);
                for (j, o) in [[1u8, 0u8], [0, 1]].into_iter().enumerate().take(t.dims()) {
                    let (d, _) = t.eval_order(at, o);
                    assert_eq!(d.to_bits(), g[j].to_bits(), "{data:?} at {at:?}");
                }
                for o in [[1u8, 0u8], [2, 0], [1, 1], [0, 2], [3, 1]] {
                    if t.dims() == 1 && o[1] > 0 {
                        continue;
                    }
                    let dt = t.derivative(o);
                    let want = t.eval_order(at, o);
                    let got = dt.eval(at);
                    assert_eq!(want.0.to_bits(), got.0.to_bits());
                    assert_eq!(want.1.map(f64::to_bits), got.1.map(f64::to_bits));
                    assert_eq!(dt.value(at).to_bits(), want.0.to_bits());
                    // a derivative of a derivative table adds the orders
                    let dd = dt.eval_order(at, [1, 0]);
                    let direct = t.eval_order(at, [o[0] + 1, o[1]]);
                    assert_eq!(dd.0.to_bits(), direct.0.to_bits());
                }
            }
        }
    }

    /// Where a derivative jumps it is the side of the cell the argument
    /// lies in: at a breakpoint inside the data the cell to its right, at
    /// the last point the last cell; outside, held axes are constant and
    /// linear ones continue along their edge slope.
    #[test]
    fn one_sided_derivatives_take_the_cell_the_argument_lies_in() {
        // a monotone cubic through uneven data: C¹, its second derivative
        // jumps at the breakpoints
        let mut data = TableData::new_1d(vec![0.0, 1.0, 2.5, 3.0], vec![0.0, 1.0, 1.5, 3.0]);
        data.outside = [Outside::Linear, Outside::Linear];
        let t = Table::new(&data).unwrap();
        assert_eq!(t.continuity(), 1);
        let h = 1e-7;
        for (i, &x) in t.points(0).iter().enumerate() {
            let d1 = |x: f64| t.eval_order([x, 0.0], [1, 0]).0;
            let d2 = t.eval_order([x, 0.0], [2, 0]).0;
            // the first derivative is continuous
            assert!((d1(x - h) - d1(x + h)).abs() < 1e-5, "{x}");
            let right = (d1(x + h) - d1(x)) / h;
            let left = (d1(x) - d1(x - h)) / h;
            if i + 1 < t.points(0).len() {
                assert!((d2 - right).abs() < 1e-5, "at {x}: {d2} vs from the right {right}");
            } else {
                assert!((d2 - left).abs() < 1e-5, "at the last point {x}: {d2} vs {left}");
            }
            if i > 0 && i + 1 < t.points(0).len() {
                assert!((right - left).abs() > 1e-3, "the second derivative jumps at {x}");
            }
        }
        // outside, continued linearly: the edge slope, then nothing
        let (v, g) = t.eval_order([4.0, 0.0], [1, 0]);
        assert_eq!(v, t.eval_order([3.0, 0.0], [1, 0]).0);
        assert_eq!(g[0], 0.0);
        assert_eq!(t.eval_order([-1.0, 0.0], [2, 0]).0, 0.0);
        // held: a kink at the edges (continuity 0), the inside's slope on them
        data.outside = [Outside::Clamp, Outside::Clamp];
        let t = Table::new(&data).unwrap();
        assert_eq!(t.continuity(), 0);
        assert!(t.eval_order([0.0, 0.0], [1, 0]).0 > 0.0);
        assert!(t.eval_order([3.0, 0.0], [1, 0]).0 > 0.0);
        assert_eq!(t.eval_order([-1e-12, 0.0], [1, 0]).0, 0.0);
        assert_eq!(t.eval_order([3.0 + 1e-12, 0.0], [1, 0]).0, 0.0);
        // linear: a kink at every breakpoint, the right cell's slope there
        data.interpolation = Interpolation::Linear;
        data.outside = [Outside::Linear, Outside::Linear];
        let t = Table::new(&data).unwrap();
        assert_eq!(t.continuity(), 0);
        assert_eq!(t.eval_order([1.0, 0.0], [1, 0]).0, (1.5 - 1.0) / 1.5);
        assert_eq!(t.eval_order([3.0, 0.0], [1, 0]).0, (3.0 - 1.5) / 0.5);
        assert_eq!(t.eval_order([1.2, 0.0], [2, 0]).0, 0.0);
        // a NaN stays NaN at every order; a constant has no derivatives
        assert!(t.eval_order([f64::NAN, 0.0], [2, 0]).0.is_nan());
        let one = Table::new(&TableData::new_1d(vec![1.0], vec![4.0])).unwrap();
        assert_eq!(one.continuity(), u32::MAX);
        assert_eq!(one.eval_order([3.0, 0.0], [2, 0]), (0.0, [0.0; 2]));
    }

    /// A bicubic surface: its mixed and second derivatives are the
    /// patch's, one-sided across cell edges like the 1-D ones, and a
    /// table of `g(x) + h(y)` with quadratic `g` and `h` is that function
    /// with every derivative.
    #[test]
    fn a_bicubic_of_a_sum_of_quadratics_is_that_sum() {
        let g = |x: f64| 1.0 + 0.5 * x + 0.1 * x * x;
        let h = |y: f64| -2.0 + 0.8 * y + 0.05 * y * y;
        let x = vec![0.0, 0.7, 1.5, 2.0, 3.1];
        let y = vec![-1.0, 0.0, 0.4, 2.0];
        let v: Vec<f64> = x.iter().flat_map(|&a| y.iter().map(move |&b| g(a) + h(b))).collect();
        let mut data = TableData::new_2d(x, y, v);
        data.outside = [Outside::Linear, Outside::Error];
        let t = Table::new(&data).unwrap();
        assert_eq!(t.continuity(), 1);
        for &a in &[0.0, 0.3, 0.7, 1.9, 2.0, 3.1] {
            for &b in &[-1.0, -0.5, 0.0, 0.4, 1.3, 2.0] {
                let close = |u: f64, w: f64| (u - w).abs() < 1e-12 * (1.0 + w.abs());
                assert!(close(t.eval([a, b]).0, g(a) + h(b)));
                assert!(close(t.eval_order([a, b], [1, 0]).0, 0.5 + 0.2 * a));
                assert!(close(t.eval_order([a, b], [0, 1]).0, 0.8 + 0.1 * b));
                assert!(close(t.eval_order([a, b], [2, 0]).0, 0.2));
                assert!(close(t.eval_order([a, b], [0, 2]).0, 0.1));
                assert!(t.eval_order([a, b], [1, 1]).0.abs() < 1e-12);
                assert!(t.eval_order([a, b], [3, 0]).0.abs() < 1e-12);
            }
        }
    }
}
