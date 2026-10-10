//! The table runtime: interpolants built from [`TableData`] and evaluated,
//! with their derivatives, by the generated code through the `lsim_tab*`
//! symbols (DESIGN.md, *Code generation*).
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

use lsim_ir::table::{Interpolation, Outside, TableData};

/// One axis of a built table.
#[derive(Clone, Debug)]
struct Axis {
    /// breakpoints, strictly increasing (at least one)
    pts: Vec<f64>,
    /// `(x0, 1/h)` when the breakpoints are uniformly spaced
    uniform: Option<(f64, f64)>,
    /// what happens outside
    outside: Outside,
    /// the app's tolerance: float noise this close to an edge is inside
    tol: f64,
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
        Axis { pts: pts.to_vec(), uniform, outside, tol: 1e-9 * lo.abs().max(hi.abs()) }
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
        Ok(t)
    }

    /// How many arguments it takes.
    pub fn dims(&self) -> usize {
        self.dims
    }

    /// The value and its partial derivatives (`d[1]` is zero for 1-D).
    #[inline]
    pub fn eval(&self, args: [f64; 2]) -> (f64, [f64; 2]) {
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

    #[inline]
    fn eval1(&self, x: f64) -> (f64, f64) {
        let ax = &self.ax[0];
        let (xc, dx, e) = ax.place(x);
        let i = ax.interval(xc);
        let c = &self.coef[4 * i..4 * i + 4];
        let s = xc - ax.pts[i];
        let v = c[0] + s * (c[1] + s * (c[2] + s * c[3]));
        let d = c[1] + s * (2.0 * c[2] + s * (3.0 * c[3]));
        (v + e * d * dx, e * d)
    }

    #[inline]
    fn eval2(&self, x: f64, y: f64) -> (f64, f64, f64) {
        let (axx, axy) = (&self.ax[0], &self.ax[1]);
        let (xc, dx, ex) = axx.place(x);
        let (yc, dy, ey) = axy.place(y);
        let (i, j) = (axx.interval(xc), axy.interval(yc));
        let ny1 = axy.pts.len() - 1;
        let a = &self.coef[16 * (i * ny1 + j)..16 * (i * ny1 + j) + 16];
        let (s, t) = (xc - axx.pts[i], yc - axy.pts[j]);
        // row polynomials in t and their t-derivatives
        let mut r = [0.0; 4];
        let mut rt = [0.0; 4];
        for p in 0..4 {
            let q = &a[4 * p..4 * p + 4];
            r[p] = q[0] + t * (q[1] + t * (q[2] + t * q[3]));
            rt[p] = q[1] + t * (2.0 * q[2] + t * (3.0 * q[3]));
        }
        let v = r[0] + s * (r[1] + s * (r[2] + s * r[3]));
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

/// The value of 1-D table `t` at `x`.
///
/// # Safety
/// `t` must point to a live [`Table`].
pub(crate) unsafe extern "C" fn lsim_tab1(t: *const Table, x: f64) -> f64 {
    // SAFETY: by the caller's contract (the generated code passes the
    // table store's pointers, which live as long as the compiled model).
    let t = unsafe { &*t };
    t.eval([x, 0.0]).0
}

/// The value of 1-D table `t` at `x`; writes the derivative to `d[0]`.
///
/// # Safety
/// `t` must point to a live [`Table`], `d` to one writable value.
pub(crate) unsafe extern "C" fn lsim_tab1d(t: *const Table, x: f64, d: *mut f64) -> f64 {
    // SAFETY: as in `lsim_tab1`; `d` is the generated code's scratch.
    unsafe {
        let (v, g) = (*t).eval([x, 0.0]);
        *d = g[0];
        v
    }
}

/// The value of 2-D table `t` at `(x, y)`.
///
/// # Safety
/// `t` must point to a live [`Table`].
pub(crate) unsafe extern "C" fn lsim_tab2(t: *const Table, x: f64, y: f64) -> f64 {
    // SAFETY: as in `lsim_tab1`.
    unsafe { (*t).eval([x, y]).0 }
}

/// The value of 2-D table `t` at `(x, y)`; writes the partial derivatives
/// to `d[0]` and `d[1]`.
///
/// # Safety
/// `t` must point to a live [`Table`], `d` to two writable values.
pub(crate) unsafe extern "C" fn lsim_tab2d(t: *const Table, x: f64, y: f64, d: *mut f64) -> f64 {
    // SAFETY: as in `lsim_tab1d`.
    unsafe {
        let (v, g) = (*t).eval([x, y]);
        *d = g[0];
        *d.add(1) = g[1];
        v
    }
}

/// Table `t`'s guard of axis `axis` at argument `a`.
///
/// # Safety
/// `t` must point to a live [`Table`].
pub(crate) unsafe extern "C" fn lsim_tab_guard(t: *const Table, axis: i64, a: f64) -> f64 {
    // SAFETY: as in `lsim_tab1`.
    unsafe { (*t).guard(axis as usize, a) }
}

/// The tables of a compiled model: built interpolants and the pointer
/// array the generated code indexes.
pub(crate) struct TableStore {
    tables: Vec<Table>,
    ptrs: Vec<*const Table>,
}

// SAFETY: the pointers point into `tables`, owned by the store and never
// mutated or resized after construction; sharing them across threads only
// reads immutable data.
unsafe impl Send for TableStore {}
// SAFETY: as above.
unsafe impl Sync for TableStore {}

impl TableStore {
    pub(crate) fn new(data: &[&TableData], names: &[&str]) -> Result<TableStore, String> {
        let mut tables = Vec::with_capacity(data.len());
        for (d, name) in data.iter().zip(names) {
            tables.push(Table::new(d).map_err(|e| format!("table '{name}': {e}"))?);
        }
        // (the vector is never resized after this, so the pointers stay valid)
        let ptrs = tables.iter().map(|t| t as *const Table).collect();
        Ok(TableStore { tables, ptrs })
    }

    pub(crate) fn ptrs(&self) -> *const *const Table {
        self.ptrs.as_ptr()
    }

    pub(crate) fn get(&self, k: usize) -> &Table {
        &self.tables[k]
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
}
