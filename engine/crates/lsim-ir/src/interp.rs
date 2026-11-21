//! Interpolants built from [`TableData`] and evaluated with their
//! derivatives: one implementation for the interpreter, preparation, the
//! generated code (through `lsim-codegen`'s `lsim_tab*` symbols) and the
//! solver (DESIGN.md, *Code generation*). A table has any number of axes:
//! those with two points or more are interpolated, an axis of one point is
//! constant.
//!
//! * **1-D, monotone cubic.** Hermite cubics with Steffen's slopes (M.
//!   Steffen, "A simple method for monotonic interpolation in one
//!   dimension", Astron. Astrophys. 239, 1990): the slope at a breakpoint
//!   is the parabola's through its neighbours, limited to twice the
//!   smaller adjacent secant and set to zero at a local extremum of the
//!   data. The result is C¹, monotone on every interval where the data
//!   are, reproduces quadratics away from extrema (third-order accurate),
//!   and each slope depends on its two neighbours only.
//! * **More axes, monotone cubic.** Tensor-product Hermite patches on the
//!   grid with zero cross derivatives (zero twist): the node slopes along
//!   each axis start as Steffen's along that grid line, then are reduced
//!   (never increased, never changed in sign) until every cell whose data
//!   are monotone along an axis has a Bézier control net monotone along
//!   that axis, which makes the patch monotone there (the sufficient
//!   condition of Carlson and Fritsch, "Monotone piecewise bicubic
//!   interpolation", SIAM J. Numer. Anal. 22, 1985, on every row of the
//!   net along that axis, for any number of axes). The node-based Hermite
//!   form keeps the surface C¹ across cells whatever the slopes.
//! * **Linear** (bilinear for two axes, multilinear for more), as today's
//!   app interpolates.
//! * **Outside the data**, per axis: `Clamp` holds the edge value (zero
//!   slope across the edge), `Linear` continues along the slope at the
//!   edge (C¹ across it), `Error` evaluates like `Linear` so the solver's
//!   trial points are harmless, and the run loop stops the run where the
//!   axis leaves its data (watched by the table guards).
//!
//! **One model, two forms of arithmetic.** A table that varies along one
//! or two axes keeps each cell's polynomial in power-basis coefficients
//! (4 per interval, 16 per cell), the arithmetic every run has been made
//! with, so that one and two axes give the same bits they always have. A
//! table that varies along three or more is evaluated from its nodes'
//! values and slopes, cell by cell, as the tensor product of the same 1-D
//! rules (4^N coefficients per cell would not fit a fine grid of many
//! axes); for two axes that form is the same function as the bicubic
//! coefficients'.
//!
//! Evaluation is pure, allocation-free for one and two axes and never
//! panics: breakpoints on a uniform grid are found by one multiplication,
//! others by a branch-free binary search; a NaN argument gives NaN.

use crate::table::{Interpolation, Outside, TableData};
use std::sync::atomic::{AtomicUsize, Ordering};

/// A table's partial derivatives, one per argument: inline for up to four
/// arguments (no allocation where a run reads tables outside the compiled
/// code, step after step), on the heap for more. It reads as a slice.
#[derive(Clone, Debug)]
pub enum Rates {
    /// up to four: the values, and how many there are
    Inline([f64; 4], usize),
    /// more
    Heap(Vec<f64>),
}

impl Rates {
    /// `n` zeros.
    pub fn zeros(n: usize) -> Rates {
        if n <= 4 { Rates::Inline([0.0; 4], n) } else { Rates::Heap(vec![0.0; n]) }
    }
}

impl std::ops::Deref for Rates {
    type Target = [f64];
    fn deref(&self) -> &[f64] {
        match self {
            Rates::Inline(a, n) => &a[..*n],
            Rates::Heap(v) => v,
        }
    }
}

impl std::ops::DerefMut for Rates {
    fn deref_mut(&mut self) -> &mut [f64] {
        match self {
            Rates::Inline(a, n) => &mut a[..*n],
            Rates::Heap(v) => v,
        }
    }
}

impl PartialEq for Rates {
    fn eq(&self, other: &Rates) -> bool {
        **self == **other
    }
}

impl PartialEq<Vec<f64>> for Rates {
    fn eq(&self, other: &Vec<f64>) -> bool {
        **self == other[..]
    }
}

impl From<Vec<f64>> for Rates {
    fn from(v: Vec<f64>) -> Rates {
        let mut r = Rates::zeros(v.len());
        r.copy_from_slice(&v);
        r
    }
}

/// Runs `f` with `n` orders, zero, on the stack for up to 16.
#[inline]
fn with_orders<R>(n: usize, f: impl FnOnce(&mut [u8]) -> R) -> R {
    if n <= 16 {
        let mut a = [0u8; 16];
        f(&mut a[..n])
    } else {
        f(&mut vec![0u8; n])
    }
}

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
    /// how many arguments it takes (its number of axes)
    dims: usize,
    /// the axes that vary (an axis with a single point is constant, and
    /// the table is evaluated along the others)
    ax: Vec<Axis>,
    /// which argument each of `ax` reads
    arg_of: Vec<usize>,
    /// the arguments no axis of `ax` reads (a NaN there still poisons the
    /// result)
    ignored: Vec<usize>,
    /// per axis of the declared table (for guards)
    guard_axes: Vec<Axis>,
    /// the value when no axis varies
    constant: f64,
    /// one varying axis: four power-basis coefficients per interval (local
    /// coordinate `s = x - x_i`); two: sixteen per cell, `a[4p + q]` of
    /// `s^p t^q`
    coef: Vec<f64>,
    /// three or more varying axes: the values at the grid's nodes,
    /// row-major over `ax`
    nodes: Vec<f64>,
    /// three or more, monotone cubic: the node slopes along each of `ax`,
    /// that along axis `k` at node `n` at `k * nodes.len() + n`
    slopes: Vec<f64>,
    /// three or more: the nodes' stride along each of `ax`
    strides: Vec<usize>,
    /// whether every interpolant's rates stay finite over its cells (the
    /// coefficients and the cells' widths are moderate): then a value
    /// inside the data or held at its edge is exactly its polynomial's
    /// (the terms that carry it outside are exact zeros), which
    /// [`Table::value`] uses (one and two varying axes)
    tame: bool,
    /// the partial derivative this table gives instead of its value
    /// ([`Table::derivative`]; all 0: its value), one order per argument
    order: Vec<u8>,
    /// interpolation, for [`Table::continuity`]
    cubic: bool,
    /// whether `order` is not every 0 (a derivative table)
    deriv: bool,
    /// the arguments the first two of `ax` read (one or two varying axes:
    /// the hot path reads them without going through `arg_of`)
    first: [usize; 2],
    /// whether some argument is read by no varying axis
    any_ignored: bool,
}

/// Runs `f` with a zeroed scratch of `n` values, on the stack for up to 16.
#[inline]
fn with_scratch<R>(n: usize, f: impl FnOnce(&mut [f64]) -> R) -> R {
    if n <= 16 {
        let mut a = [0.0; 16];
        f(&mut a[..n])
    } else {
        f(&mut vec![0.0; n])
    }
}

/// One condition `base + Σ c_k g[i_k] >= 0` on node slopes `g`; when it
/// fails, the terms that work against it are scaled down until it holds
/// ([`enforce`] over indexes). Returns whether anything changed.
fn enforce_at(base: f64, terms: &[(f64, usize)], g: &mut [f64]) -> bool {
    let mut total = base;
    let (mut help, mut harm, mut scale) = (base, 0.0, base.abs());
    for &(c, i) in terms {
        let v = c * g[i];
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
    for &(c, i) in terms {
        if c * g[i] < 0.0 {
            g[i] *= lambda;
        }
    }
    true
}

/// Reduces the node slopes `g` (along axis `k` at node `n`: `g[k * nn +
/// n]`) of a tensor-product Hermite form with zero cross derivatives over
/// the grid of `ax` until each cell's Bézier net is monotone along every
/// axis along which the cell's data are monotone: [`monotone_2d`]'s
/// conditions for any number of axes.
///
/// Along axis `k` of a cell, the net's rows are its points with the same
/// index (0 to 3) along every other axis; an index 0 or 3 along axis `j`
/// is a corner's own, 1 the lower corner's moved by `h_j/3` of its slope
/// along `j`, 2 the upper's moved back by as much. The difference across
/// the middle of a row from corner `c` to `c + e_k` is then
/// `f(c + e_k) - f(c) - h_k/3 (g_k(c) + g_k(c + e_k)) ± Σ_j h_j/3
/// (g_j(c + e_k) - g_j(c))` over the axes `j` the row has index 1 (+) or 2
/// (−) along; the differences at its ends are `h_k/3` times a slope along
/// `k`, of the data's sign there by Steffen's rule.
fn monotone_nd(ax: &[Axis], strides: &[usize], f: &[f64], g: &mut [f64]) {
    let v = ax.len();
    let nn = f.len();
    let cells: usize = ax.iter().map(|a| a.pts.len() - 1).product();
    let mut terms: Vec<(f64, usize)> = Vec::with_capacity(2 + 2 * v);
    let mut converged = false;
    for _pass in 0..64 {
        let mut changed = false;
        let mut cell = vec![0usize; v];
        for _ in 0..cells {
            let base: usize = cell.iter().zip(strides).map(|(i, s)| i * s).sum();
            let h: Vec<f64> = (0..v).map(|k| ax[k].pts[cell[k] + 1] - ax[k].pts[cell[k]]).collect();
            for k in 0..v {
                let others: Vec<usize> = (0..v).filter(|&j| j != k).collect();
                // the signs along which the cell's edges along k are monotone
                let (mut pos, mut neg) = (true, true);
                for mask in 0..1usize << others.len() {
                    let mut c = base;
                    for (b, &j) in others.iter().enumerate() {
                        if mask >> b & 1 == 1 {
                            c += strides[j];
                        }
                    }
                    let d = f[c + strides[k]] - f[c];
                    pos &= d >= 0.0;
                    neg &= d <= 0.0;
                }
                let signs: &[f64] = match (pos, neg) {
                    (true, true) => &[1.0, -1.0],
                    (true, false) => &[1.0],
                    (false, true) => &[-1.0],
                    (false, false) => &[],
                };
                for &s in signs {
                    let ck = s * h[k] / 3.0;
                    for row in 0..1usize << (2 * others.len()) {
                        let mut c = base;
                        for (b, &j) in others.iter().enumerate() {
                            if (row >> (2 * b)) & 3 >= 2 {
                                c += strides[j];
                            }
                        }
                        let c1 = c + strides[k];
                        terms.clear();
                        terms.push((-ck, k * nn + c));
                        terms.push((-ck, k * nn + c1));
                        for (b, &j) in others.iter().enumerate() {
                            let cj = s * h[j] / 3.0;
                            match (row >> (2 * b)) & 3 {
                                1 => {
                                    terms.push((cj, j * nn + c1));
                                    terms.push((-cj, j * nn + c));
                                }
                                2 => {
                                    terms.push((-cj, j * nn + c1));
                                    terms.push((cj, j * nn + c));
                                }
                                _ => {}
                            }
                        }
                        changed |= enforce_at(s * (f[c1] - f[c]), &terms, g);
                    }
                }
            }
            for k in (0..v).rev() {
                cell[k] += 1;
                if cell[k] < ax[k].pts.len() - 1 {
                    break;
                }
                cell[k] = 0;
            }
        }
        if !changed {
            converged = true;
            break;
        }
    }
    if !converged {
        // a last resort that always satisfies every condition: flat slopes
        g.iter_mut().for_each(|x| *x = 0.0);
    }
}

/// The 1-D bases of a cell of width `h` at `s` (`0 <= s <= h` inside),
/// differentiated `m` times: the polynomials that multiply its lower and
/// upper node's value, and (cubic) its lower and upper node's slope.
#[inline]
fn basis(cubic: bool, m: u8, s: f64, h: f64) -> [f64; 4] {
    let t = s / h;
    if !cubic {
        return match m {
            0 => [1.0 - t, t, 0.0, 0.0],
            1 => [-1.0 / h, 1.0 / h, 0.0, 0.0],
            _ => [0.0; 4],
        };
    }
    match m {
        0 => {
            let up = t * t * (3.0 - 2.0 * t);
            [1.0 - up, up, s * (1.0 - t) * (1.0 - t), s * t * (t - 1.0)]
        }
        1 => {
            let a = 6.0 * t * (t - 1.0) / h;
            [a, -a, (1.0 - t) * (1.0 - 3.0 * t), t * (3.0 * t - 2.0)]
        }
        2 => [
            (12.0 * t - 6.0) / (h * h),
            (6.0 - 12.0 * t) / (h * h),
            (6.0 * t - 4.0) / h,
            (6.0 * t - 2.0) / h,
        ],
        3 => [12.0 / (h * h * h), -12.0 / (h * h * h), 6.0 / (h * h), 6.0 / (h * h)],
        _ => [0.0; 4],
    }
}

/// Where three or more varying axes read their arguments: per axis of
/// `ax`, the point the data are read at, the distance beyond it, the
/// factor across the edge ([`Axis::place`]), the cell and its width.
struct Place {
    /// the cell's lowest node
    base: usize,
    /// per axis: (s, h, d, e, inside)
    at: Vec<(f64, f64, f64, f64, bool)>,
}

/// One cell's partials ([`Table::cell_partial`]) and the scratch they need.
struct CellJet<'a> {
    t: &'a Table,
    b: Vec<[f64; 4]>,
    pre: Vec<f64>,
    suf: Vec<f64>,
}

impl CellJet<'_> {
    /// The partial derivative of order `m` (per axis of `ax`) of the
    /// polynomial of the cell at `p`: Σ over its corners `c` of `f(c) Π_k
    /// φ_k + Σ_k g_k(c) ψ_k Π_(j≠k) φ_j`, each basis differentiated as `m`
    /// says.
    fn partial(&mut self, p: &Place, m: &[u8]) -> f64 {
        let t = self.t;
        let v = t.ax.len();
        let nn = t.nodes.len();
        for ((b, &(s, h, ..)), &mk) in self.b.iter_mut().zip(&p.at).zip(m) {
            *b = basis(t.cubic, mk, s, h);
        }
        let mut sum = 0.0;
        for corner in 0..1usize << v {
            let mut node = p.base;
            for k in 0..v {
                if corner >> k & 1 == 1 {
                    node += t.strides[k];
                }
            }
            self.pre[0] = 1.0;
            for k in 0..v {
                self.pre[k + 1] = self.pre[k] * self.b[k][corner >> k & 1];
            }
            let mut term = t.nodes[node] * self.pre[v];
            if t.cubic {
                self.suf[v] = 1.0;
                for k in (0..v).rev() {
                    self.suf[k] = self.suf[k + 1] * self.b[k][corner >> k & 1];
                }
                for k in 0..v {
                    let psi = self.b[k][2 + (corner >> k & 1)];
                    term += t.slopes[k * nn + node] * psi * self.pre[k] * self.suf[k + 1];
                }
            }
            sum += term;
        }
        sum
    }
}

impl Table {
    /// Builds the interpolant. The data must pass [`TableData::check`].
    pub fn new(data: &TableData) -> Result<Table, String> {
        data.check()?;
        let dims = data.dims();
        let guard_axes: Vec<Axis> =
            data.axes.iter().map(|a| Axis::new(&a.points, a.outside)).collect();
        let varying: Vec<usize> = (0..dims).filter(|&a| guard_axes[a].pts.len() >= 2).collect();
        let cubic = data.interpolation == Interpolation::MonotoneCubic;
        let mut t = Table {
            dims,
            ax: varying.iter().map(|&a| guard_axes[a].clone()).collect(),
            ignored: (0..dims).filter(|a| !varying.contains(a)).collect(),
            arg_of: varying.clone(),
            guard_axes,
            constant: data.values[0],
            coef: vec![],
            nodes: vec![],
            slopes: vec![],
            strides: vec![],
            tame: false,
            order: vec![0; dims],
            cubic,
            deriv: false,
            first: [varying.first().copied().unwrap_or(0), varying.get(1).copied().unwrap_or(0)],
            any_ignored: varying.len() < dims,
        };
        // (the values are row-major over the varying axes too: an axis of
        // one point has stride 0 in effect)
        match varying.len() {
            0 => {}
            1 => {
                // one axis, or more with a single point along the others
                // (the values then in the order of the one that varies)
                let pts: &[f64] = data.points(varying[0]);
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
            2 => {
                let (x, y, f) = (data.points(varying[0]), data.points(varying[1]), &data.values);
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
            _ => t.build_nodes(&data.values),
        }
        let width = |a: &Axis| a.pts.windows(2).map(|w| w[1] - w[0]).fold(0.0, f64::max);
        t.tame = t.coef.iter().all(|c| c.abs() < 1e100)
            && t.ax.iter().all(|a| width(a) < 1e30 && a.pts.iter().all(|p| p.abs() < 1e100));
        Ok(t)
    }

    /// Three or more varying axes: the nodes, their strides and (cubic)
    /// their slopes.
    fn build_nodes(&mut self, values: &[f64]) {
        let v = self.ax.len();
        let mut strides = vec![1usize; v];
        for k in (0..v - 1).rev() {
            strides[k] = strides[k + 1] * self.ax[k + 1].pts.len();
        }
        self.nodes = values.to_vec();
        self.strides = strides;
        if !self.cubic {
            return;
        }
        let nn = values.len();
        let mut g = vec![0.0; v * nn];
        for k in 0..v {
            // every grid line along axis k, from its node of index 0 there
            let (sk, nk) = (self.strides[k], self.ax[k].pts.len());
            let mut line = vec![0.0; nk];
            for start in (0..nn).filter(|n| (n / sk) % nk == 0) {
                for (i, l) in line.iter_mut().enumerate() {
                    *l = values[start + i * sk];
                }
                for (i, d) in steffen_slopes(&self.ax[k].pts, &line).into_iter().enumerate() {
                    g[k * nn + start + i * sk] = d;
                }
            }
        }
        monotone_nd(&self.ax, &self.strides, values, &mut g);
        self.slopes = g;
    }

    /// How many arguments it takes.
    pub fn dims(&self) -> usize {
        self.dims
    }

    /// Whether `other` is the same interpolant, to the bit: the same
    /// breakpoints, rule outside the data on each axis, coefficients (and
    /// so interpolation and values) and derivative. Two tables that agree
    /// at their breakpoints can still differ between and beyond them (a
    /// linear table held at its edges and the same one continued along its
    /// slope).
    pub fn same(&self, other: &Table) -> bool {
        let bits = |a: &[f64], b: &[f64]| {
            a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.to_bits() == y.to_bits())
        };
        let axes = |a: &[Axis], b: &[Axis]| {
            a.len() == b.len()
                && a.iter().zip(b).all(|(p, q)| {
                    bits(&p.pts, &q.pts)
                        && p.outside == q.outside
                        && p.tol.to_bits() == q.tol.to_bits()
                        && p.uniform.map(|(x, h)| (x.to_bits(), h.to_bits()))
                            == q.uniform.map(|(x, h)| (x.to_bits(), h.to_bits()))
                })
        };
        self.dims == other.dims
            && self.arg_of == other.arg_of
            && self.order == other.order
            && self.cubic == other.cubic
            && self.tame == other.tame
            && self.constant.to_bits() == other.constant.to_bits()
            && bits(&self.coef, &other.coef)
            && bits(&self.nodes, &other.nodes)
            && bits(&self.slopes, &other.slopes)
            && axes(&self.ax, &other.ax)
            && axes(&self.guard_axes, &other.guard_axes)
    }

    /// Whether this table gives a derivative instead of its value.
    #[inline]
    fn derived(&self) -> bool {
        self.deriv
    }

    /// A read with fewer arguments than the table has axes (a 2-D table
    /// read with one, as the code generator reads it with its 1-D symbol),
    /// or rates with less room: `f` on the arguments with 0 for each one
    /// missing (NaN for the first, when there is none at all), and the
    /// rates of as many arguments as `grad` holds.
    #[cold]
    fn short(
        &self,
        args: &[f64],
        grad: &mut [f64],
        f: impl FnOnce(&[f64], &mut [f64]) -> f64,
    ) -> f64 {
        let n = self.dims;
        let mut at = vec![0.0; n];
        at[..args.len().min(n)].copy_from_slice(&args[..args.len().min(n)]);
        if args.is_empty() {
            at[0] = f64::NAN;
        }
        let mut g = vec![0.0; n];
        let v = f(&at, &mut g);
        let k = grad.len().min(n);
        grad[..k].copy_from_slice(&g[..k]);
        v
    }

    /// The first NaN among the arguments no varying axis reads.
    #[inline]
    fn ignored_nan(&self, args: &[f64]) -> Option<f64> {
        if !self.any_ignored {
            return None;
        }
        self.ignored.iter().map(|&k| args[k]).find(|a| a.is_nan())
    }

    /// The value and its partial derivatives, one per argument (`grad`
    /// holds [`Table::dims`] values); for a [`Table::derivative`], that
    /// derivative and its partial derivatives.
    #[inline]
    pub fn eval_into(&self, args: &[f64], grad: &mut [f64]) -> f64 {
        if args.len() < self.dims || grad.len() < self.dims {
            return self.short(args, grad, |a, g| self.eval_into(a, g));
        }
        if self.derived() {
            return self.eval_order_into(args, &[], grad);
        }
        self.eval0(args, grad)
    }

    /// [`Table::eval_into`], the partial derivatives as [`Rates`].
    pub fn eval(&self, args: &[f64]) -> (f64, Rates) {
        let mut g = Rates::zeros(self.dims);
        let v = self.eval_into(args, &mut g);
        (v, g)
    }

    /// The value and its partial derivatives.
    #[inline]
    fn eval0(&self, args: &[f64], grad: &mut [f64]) -> f64 {
        grad.iter_mut().for_each(|g| *g = 0.0);
        match self.ax.len() {
            0 => {
                if args[..self.dims].iter().any(|a| a.is_nan()) {
                    f64::NAN
                } else {
                    self.constant
                }
            }
            1 => {
                let a = self.first[0];
                let (v, d) = self.eval1(args[a]);
                grad[a] = d;
                // a NaN in an ignored argument still poisons the result
                self.ignored_nan(args).unwrap_or(v)
            }
            2 => {
                let [a, b] = self.first;
                let (v, dx, dy) = self.eval2(args[a], args[b]);
                grad[a] = dx;
                grad[b] = dy;
                self.ignored_nan(args).unwrap_or(v)
            }
            _ => {
                let v = self.eval_nd(args, Some(grad));
                self.ignored_nan(args).unwrap_or(v)
            }
        }
    }

    /// The value alone: bitwise `self.eval(args).0` (the same code), without
    /// the rates where they cannot change it: inside the data, or held at
    /// its edge, the terms that carry a value across the edge are exact
    /// zeros, and a non-zero value is the polynomial's.
    #[inline]
    pub fn value(&self, args: &[f64]) -> f64 {
        if args.len() < self.dims {
            return self.short(args, &mut [], |a, _| self.value(a));
        }
        if self.derived() {
            return self.partial(args, &self.order);
        }
        match self.ax.len() {
            0 => with_scratch(self.dims, |g| self.eval0(args, g)),
            1 => {
                let a = self.first[0];
                let (v, _) = self.eval1_with::<false>(args[a]);
                // a NaN in an ignored argument still poisons the result
                self.ignored_nan(args).unwrap_or(v)
            }
            2 => {
                let [a, b] = self.first;
                let v = self.eval2_with::<false>(args[a], args[b]).0;
                self.ignored_nan(args).unwrap_or(v)
            }
            _ => {
                let v = self.eval_nd(args, None);
                self.ignored_nan(args).unwrap_or(v)
            }
        }
    }

    /// The same interpolant giving its partial derivative of `order`
    /// (along each argument; missing entries 0) instead of its value: what
    /// the generated code calls for a derivative read
    /// ([`crate::Expr::Table`]'s `order`). [`Table::eval`] and
    /// [`Table::value`] then give that derivative (and its partial
    /// derivatives). The coefficients are the same, built by the same code
    /// from the same data.
    pub fn derivative(&self, order: &[u8]) -> Table {
        let mut t = self.clone();
        for (k, o) in t.order.iter_mut().enumerate() {
            *o = o.saturating_add(order.get(k).copied().unwrap_or(0));
        }
        t.deriv = t.order.iter().any(|&o| o != 0);
        t
    }

    /// The order of this table's own derivative plus `order` (missing
    /// entries 0), one per argument, into `out`; false when `order` asks
    /// for a derivative along an argument the table does not have.
    fn total_order(&self, order: &[u8], out: &mut [u8]) -> bool {
        if order.iter().skip(self.dims).any(|&o| o != 0) {
            return false;
        }
        for (k, o) in out.iter_mut().enumerate() {
            *o = self.order[k].saturating_add(order.get(k).copied().unwrap_or(0));
        }
        true
    }

    /// The partial derivative of `order` (on top of this table's own, see
    /// [`Table::derivative`]) at `args`, and its partial derivatives: the
    /// interpolant's own, exact up to rounding. Order 0 along every
    /// argument is [`Table::eval`]; a first derivative is bitwise the rate
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
    pub fn eval_order(&self, args: &[f64], order: &[u8]) -> (f64, Rates) {
        let mut g = Rates::zeros(self.dims);
        let v = self.eval_order_into(args, order, &mut g);
        (v, g)
    }

    /// [`Table::eval_order`], the partial derivatives into `grad`
    /// ([`Table::dims`] values).
    pub fn eval_order_into(&self, args: &[f64], order: &[u8], grad: &mut [f64]) -> f64 {
        if args.len() < self.dims || grad.len() < self.dims {
            return self.short(args, grad, |a, g| self.eval_order_into(a, order, g));
        }
        // (orders saturate: past the fourth every derivative is 0 anyway)
        with_orders(self.dims, |o| {
            if !self.total_order(order, o) {
                let nan = args[..self.dims].iter().any(|a| a.is_nan());
                grad.iter_mut().for_each(|g| *g = if nan { f64::NAN } else { 0.0 });
                return if nan { f64::NAN } else { 0.0 };
            }
            if o.iter().all(|&x| x == 0) {
                return self.eval0(args, grad);
            }
            let v = self.partial(args, o);
            for k in 0..self.dims {
                let own = o[k];
                o[k] = own.saturating_add(1);
                grad[k] = self.partial(args, o);
                o[k] = own;
            }
            v
        })
    }

    /// The partial derivative of order `o` (one per argument, not all 0)
    /// at `args`.
    fn partial(&self, args: &[f64], o: &[u8]) -> f64 {
        let nan = args[..self.dims].iter().any(|a| a.is_nan());
        // first derivatives: the rates of the value, the same code (also
        // where an argument is NaN: a rate along an axis with one point is
        // 0 whatever the other arguments are)
        if o.iter().map(|&x| u32::from(x)).sum::<u32>() == 1 {
            let k = o.iter().position(|&x| x == 1).expect("one");
            return with_scratch(self.dims, |g| {
                self.eval0(args, g);
                g[k]
            });
        }
        if nan {
            return f64::NAN;
        }
        // along an axis with a single point: constant
        if self.ignored.iter().any(|&k| o[k] > 0) {
            return 0.0;
        }
        match self.ax.len() {
            0 => 0.0,
            1 => {
                let a = self.arg_of[0];
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
            2 => {
                let (ka, kb) = (self.arg_of[0], self.arg_of[1]);
                let o = [o[ka], o[kb]];
                let (axx, axy) = (&self.ax[0], &self.ax[1]);
                let (x, y) = (args[ka], args[kb]);
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
            _ => {
                // as for two axes: per axis, the derivative of order k is
                // w[0]·(the polynomial's of order k) + w[1]·(of order k + 1)
                let p = self.place_nd(args);
                let v = self.ax.len();
                let ow: Vec<u8> = self.arg_of.iter().map(|&a| o[a]).collect();
                let w: Vec<[f64; 2]> = (0..v)
                    .map(|k| {
                        let (_, _, d, e, inside) = p.at[k];
                        match ow[k] {
                            0 => [1.0, e * d],
                            1 => [e, 0.0],
                            _ => [if inside { 1.0 } else { 0.0 }, 0.0],
                        }
                    })
                    .collect();
                let mut jet = self.jet();
                let mut m = ow.clone();
                let mut sum = 0.0;
                for pick in 0..1usize << v {
                    let mut wt = 1.0;
                    for k in 0..v {
                        let b = pick >> k & 1;
                        wt *= w[k][b];
                        m[k] = ow[k].saturating_add(b as u8);
                    }
                    if wt != 0.0 {
                        sum += wt * jet.partial(&p, &m);
                    }
                }
                sum
            }
        }
    }

    /// Three or more varying axes: where `args` lie.
    fn place_nd(&self, args: &[f64]) -> Place {
        let mut base = 0;
        let at = self
            .ax
            .iter()
            .enumerate()
            .map(|(k, ax)| {
                let x = args[self.arg_of[k]];
                let (xc, d, e) = ax.place(x);
                let i = ax.interval(xc);
                base += i * self.strides[k];
                (xc - ax.pts[i], ax.pts[i + 1] - ax.pts[i], d, e, ax.lo() <= x && x <= ax.hi())
            })
            .collect();
        Place { base, at }
    }

    fn jet(&self) -> CellJet<'_> {
        let v = self.ax.len();
        CellJet { t: self, b: vec![[0.0; 4]; v], pre: vec![0.0; v + 1], suf: vec![0.0; v + 1] }
    }

    /// Three or more varying axes: the value at `args` and, when asked, its
    /// partial derivatives into `grad` (one per argument; those of the
    /// arguments no axis reads are left as they are). The cell's
    /// polynomial `P` at the point read, continued past the data along the
    /// axes `O` that continue there (`e = 1`, `d ≠ 0`) as the sum over the
    /// subsets `S` of `O` of `Π_(k∈S) d_k ∂_S P` (two axes: `v + vx dx + vy
    /// dy + vxy dx dy`), and its rate along axis `k` `e_k` times the same
    /// sum over the subsets of `O` without `k` of `∂_(S∪k) P`.
    #[inline(never)]
    fn eval_nd(&self, args: &[f64], grad: Option<&mut [f64]>) -> f64 {
        // ponytail: three scratch vectors per call; a caller-owned scratch
        // if a run of a many-axis table shows them in a profile
        let p = self.place_nd(args);
        let v = self.ax.len();
        let out: Vec<usize> = (0..v).filter(|&k| p.at[k].3 != 0.0 && p.at[k].2 != 0.0).collect();
        let mut jet = self.jet();
        let mut m = vec![0u8; v];
        // Σ over the subsets S of `out` (and `with`, when given) of
        // Π_(k∈S) d_k ∂_(S∪with) P
        let mut continued = |m: &mut [u8], with: Option<usize>| -> f64 {
            let mut sum = 0.0;
            for pick in 0..1usize << out.len() {
                if with.is_some_and(|w| {
                    out.iter().enumerate().any(|(b, &k)| k == w && pick >> b & 1 == 1)
                }) {
                    continue;
                }
                let mut wt = 1.0;
                m.iter_mut().for_each(|x| *x = 0);
                for (b, &k) in out.iter().enumerate() {
                    if pick >> b & 1 == 1 {
                        m[k] = 1;
                        wt *= p.at[k].2;
                    }
                }
                if let Some(w) = with {
                    m[w] = 1;
                }
                sum += wt * jet.partial(&p, m);
            }
            sum
        };
        let val = continued(&mut m, None);
        if let Some(grad) = grad {
            for k in 0..v {
                let e = p.at[k].3;
                grad[self.arg_of[k]] = if e == 0.0 { 0.0 } else { e * continued(&mut m, Some(k)) };
            }
        }
        val
    }

    /// How many times the interpolant is continuously differentiable
    /// everywhere, inside and outside its data (`u32::MAX`: it is
    /// constant). A monotone cubic (of any number of axes) is C¹ where
    /// every axis that varies continues linearly outside (`Linear`,
    /// `Error`), C⁰ when an axis is clamped (a kink at its edge); a linear
    /// table is C⁰ (a kink at its breakpoints).
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
    let varying: Vec<Outside> =
        data.axes.iter().filter(|a| a.points.len() >= 2).map(|a| a.outside).collect();
    if varying.is_empty() {
        return u32::MAX;
    }
    let cubic = data.interpolation == Interpolation::MonotoneCubic;
    if cubic && !varying.contains(&Outside::Clamp) { 1 } else { 0 }
}

impl Table {
    /// The partial derivative this table gives instead of its value (all
    /// 0: its value), one order per argument.
    pub fn order(&self) -> &[u8] {
        &self.order
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

    /// Whether, outside its data along `axis`, the table is its edge
    /// cells' own polynomials continued: a linear table continued along its
    /// edge slope (`Linear`, and `Error`, whose interpolant goes on so),
    /// with no kink at the data's edges along that axis.
    pub fn continues_cells(&self, axis: usize) -> bool {
        !self.cubic && self.guard_axes.get(axis).is_some_and(|ax| ax.outside != Outside::Clamp)
    }

    /// How smooth the table is along `axis` ([`axis_smoothness`]).
    pub fn smoothness(&self, axis: usize) -> AxisSmoothness {
        self.guard_axes.get(axis).map_or(AxisSmoothness::SMOOTH, |ax| {
            axis_smoothness(self.cubic, ax.pts.len(), ax.outside)
        })
    }
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

/// How smooth a table is along one axis, as its number of continuous
/// derivatives there (DESIGN.md §8.2, *Tables read at a state*): at its
/// breakpoints inside its data, and at its data's two edges. `u32::MAX`
/// where it has no breakpoint (no point inside the data: two points; the
/// edges of a linear table continued along its edge cells; an axis of one
/// point).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AxisSmoothness {
    /// at the points inside the data: 0 for a linear table, 1 for a
    /// monotone cubic
    pub inside: u32,
    /// at the data's edges: 0 held there (`Clamp`), else 1 for a monotone
    /// cubic (continued along its edge slope, its second derivative 0
    /// outside) and none for a linear table (its edge cells continued)
    pub edges: u32,
}

impl AxisSmoothness {
    /// No breakpoint at all.
    pub const SMOOTH: AxisSmoothness = AxisSmoothness { inside: u32::MAX, edges: u32::MAX };
}

/// [`AxisSmoothness`] of an axis of `points` breakpoints, interpolated
/// cubically or not, with the rule `outside` beyond its data.
pub fn axis_smoothness(cubic: bool, points: usize, outside: Outside) -> AxisSmoothness {
    if points < 2 {
        return AxisSmoothness::SMOOTH;
    }
    let inside = if points == 2 {
        u32::MAX
    } else if cubic {
        1
    } else {
        0
    };
    let edges = match outside {
        Outside::Clamp => 0,
        Outside::Linear | Outside::Error if cubic => 1,
        Outside::Linear | Outside::Error => u32::MAX,
    };
    AxisSmoothness { inside, edges }
}

/// [`axis_smoothness`] of axis `axis` of the interpolant `data` makes,
/// from its rules and point counts alone (an axis it does not have: no
/// breakpoint).
pub fn data_smoothness(data: &TableData, axis: usize) -> AxisSmoothness {
    let cubic = data.interpolation == Interpolation::MonotoneCubic;
    data.axes
        .get(axis)
        .map_or(AxisSmoothness::SMOOTH, |a| axis_smoothness(cubic, a.points.len(), a.outside))
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
        data = data.with_outside(&[Outside::Clamp, Outside::Clamp]);
        let t = Table::new(&data).unwrap();
        assert_eq!(t.points(0), &[0.0, 1.0, 3.0]);
        for (x, v) in [(0.0, 0.0), (1.0, 1.0), (3.0, 5.0)] {
            assert!((t.eval(&[x]).0 - v).abs() < 1e-14);
        }
        assert_eq!(t.eval(&[-1.0]), (0.0, vec![0.0].into()));
        assert_eq!(t.eval(&[4.0]).0, 5.0);
        data = data.with_outside(&[Outside::Linear, Outside::Clamp]);
        let t = Table::new(&data).unwrap();
        let (v, d) = t.eval(&[4.0]);
        let (v3, d3) = t.eval(&[3.0]);
        assert!((v - (v3 + d3[0])).abs() < 1e-14 && d == d3);
        assert!(t.eval(&[f64::NAN]).0.is_nan());
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
            data = data.with_outside(&[outs[case % 3], outs[(case / 3) % 3]]);
            let Ok(t) = Table::new(&data) else { continue };
            let mut probe = |a: f64, b: f64| {
                let args = [a, b];
                let at = &args[..t.dims()];
                let (v, _) = t.eval(at);
                let w = t.value(at);
                assert!(
                    v.to_bits() == w.to_bits(),
                    "case {case} at ({a}, {b}): eval {v:e} ({:x}), value {w:e} ({:x})",
                    v.to_bits(),
                    w.to_bits()
                );
                checked += 1;
            };
            let pts_x = data.points(0).to_vec();
            let pts_y = if data.dims() == 1 { vec![0.0] } else { data.points(1).to_vec() };
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
            data = data.with_outside(&[outs[case % 3], outs[(case / 3) % 3]]);
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
                        let n = t.dims();
                        let o = [o0, o1];
                        let (v, g) = t.eval_order(&at[..n], &o[..n]);
                        for j in 0..n {
                            let mut a = at;
                            let mut b = at;
                            a[j] += h;
                            b[j] -= h;
                            let fd = (t.eval_order(&a[..n], &o[..n]).0
                                - t.eval_order(&b[..n], &o[..n]).0)
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
            let n = t.dims();
            let bits = |v: &[f64]| v.iter().map(|x| x.to_bits()).collect::<Vec<_>>();
            for at in pts {
                let at = &at[..n];
                let (_, g) = t.eval(at);
                for (j, o) in [[1u8, 0u8], [0, 1]].into_iter().enumerate().take(n) {
                    let (d, _) = t.eval_order(at, &o[..n]);
                    assert_eq!(d.to_bits(), g[j].to_bits(), "{data:?} at {at:?}");
                }
                for o in [[1u8, 0u8], [2, 0], [1, 1], [0, 2], [3, 1]] {
                    if n == 1 && o[1] > 0 {
                        continue;
                    }
                    let dt = t.derivative(&o[..n]);
                    let want = t.eval_order(at, &o[..n]);
                    let got = dt.eval(at);
                    assert_eq!(want.0.to_bits(), got.0.to_bits());
                    assert_eq!(bits(&want.1), bits(&got.1));
                    assert_eq!(dt.value(at).to_bits(), want.0.to_bits());
                    // a derivative of a derivative table adds the orders
                    let dd = dt.eval_order(at, &[1]);
                    let direct = t.eval_order(at, &[o[0] + 1, o[1]][..n]);
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
        data = data.with_outside(&[Outside::Linear, Outside::Linear]);
        let t = Table::new(&data).unwrap();
        assert_eq!(t.continuity(), 1);
        let h = 1e-7;
        for (i, &x) in t.points(0).iter().enumerate() {
            let d1 = |x: f64| t.eval_order(&[x], &[1]).0;
            let d2 = t.eval_order(&[x], &[2]).0;
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
        let (v, g) = t.eval_order(&[4.0], &[1]);
        assert_eq!(v, t.eval_order(&[3.0], &[1]).0);
        assert_eq!(g[0], 0.0);
        assert_eq!(t.eval_order(&[-1.0], &[2]).0, 0.0);
        // held: a kink at the edges (continuity 0), the inside's slope on them
        data = data.with_outside(&[Outside::Clamp, Outside::Clamp]);
        let t = Table::new(&data).unwrap();
        assert_eq!(t.continuity(), 0);
        assert!(t.eval_order(&[0.0], &[1]).0 > 0.0);
        assert!(t.eval_order(&[3.0], &[1]).0 > 0.0);
        assert_eq!(t.eval_order(&[-1e-12], &[1]).0, 0.0);
        assert_eq!(t.eval_order(&[3.0 + 1e-12], &[1]).0, 0.0);
        // linear: a kink at every breakpoint, the right cell's slope there
        data.interpolation = Interpolation::Linear;
        data = data.with_outside(&[Outside::Linear, Outside::Linear]);
        let t = Table::new(&data).unwrap();
        assert_eq!(t.continuity(), 0);
        assert_eq!(t.eval_order(&[1.0], &[1]).0, (1.5 - 1.0) / 1.5);
        assert_eq!(t.eval_order(&[3.0], &[1]).0, (3.0 - 1.5) / 0.5);
        assert_eq!(t.eval_order(&[1.2], &[2]).0, 0.0);
        // a NaN stays NaN at every order; a constant has no derivatives
        assert!(t.eval_order(&[f64::NAN], &[2]).0.is_nan());
        let one = Table::new(&TableData::new_1d(vec![1.0], vec![4.0])).unwrap();
        assert_eq!(one.continuity(), u32::MAX);
        assert_eq!(one.eval_order(&[3.0], &[2]), (0.0, vec![0.0].into()));
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
        data = data.with_outside(&[Outside::Linear, Outside::Error]);
        let t = Table::new(&data).unwrap();
        assert_eq!(t.continuity(), 1);
        for &a in &[0.0, 0.3, 0.7, 1.9, 2.0, 3.1] {
            for &b in &[-1.0, -0.5, 0.0, 0.4, 1.3, 2.0] {
                let close = |u: f64, w: f64| (u - w).abs() < 1e-12 * (1.0 + w.abs());
                assert!(close(t.eval(&[a, b]).0, g(a) + h(b)));
                assert!(close(t.eval_order(&[a, b], &[1, 0]).0, 0.5 + 0.2 * a));
                assert!(close(t.eval_order(&[a, b], &[0, 1]).0, 0.8 + 0.1 * b));
                assert!(close(t.eval_order(&[a, b], &[2, 0]).0, 0.2));
                assert!(close(t.eval_order(&[a, b], &[0, 2]).0, 0.1));
                assert!(t.eval_order(&[a, b], &[1, 1]).0.abs() < 1e-12);
                assert!(t.eval_order(&[a, b], &[3, 0]).0.abs() < 1e-12);
            }
        }
    }
    #[test]
    fn the_same_interpolant_is_told_from_one_that_agrees_only_at_its_breakpoints() {
        let mut d = TableData::new_1d(vec![0.0, 5.0, 7.0], vec![0.0, 1.0, 1.5]);
        d.interpolation = Interpolation::Linear;
        d = d.with_outside(&[Outside::Clamp, Outside::Clamp]);
        let t = Table::new(&d).unwrap();
        assert!(t.same(&t.clone()) && t.same(&Table::new(&d).unwrap()));
        let beyond = d.clone().with_outside(&[Outside::Linear]);
        let mut cubic = d.clone();
        cubic.interpolation = Interpolation::MonotoneCubic;
        let mut value = d.clone();
        value.values[2] = 1.5000000000000002;
        for other in [beyond, cubic, value] {
            let o = Table::new(&other).unwrap();
            // (at the breakpoints the first two agree)
            assert!(!t.same(&o) && !o.same(&t), "{other:?}");
        }
        // a derivative of the table is another interpolant
        let dt = t.derivative(&[1]);
        assert!(!t.same(&dt) && dt.same(&t.derivative(&[1])));
    }
}
