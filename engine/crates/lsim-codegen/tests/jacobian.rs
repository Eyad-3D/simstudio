//! Derivatives against differences (DESIGN.md §16, WP3's acceptance):
//! every Jacobian-vector product (from the coloured Jacobian, the
//! default, and from its own forward-mode code) and every column of the
//! sparse Jacobian matches central differences of the residual, to
//! within the differences' own error estimate (Richardson extrapolation
//! from three steps); differences outside the sparsity pattern are exact
//! zeros (the pattern holds every dependence); the dense Jacobian is the
//! sparse one's values in place, and the forward-mode code's columns are
//! the coloured Jacobian's.
//!
//! On random smooth models with states and iteration variables, on the
//! example projects along a simulated drive (also with every block
//! implicit), and on their initialisation systems.
//!
//! Where the differences themselves do not converge (a kink, a table's
//! breakpoint or a branch within the steps) an entry is not compared; the
//! tests count those and require them to be rare.

#[path = "common/cars.rs"]
mod cars;
#[path = "common/random.rs"]
mod random;
#[path = "common/synth.rs"]
mod synth;

use lsim_codegen::{CodegenOptions, JitModel, compile};
use lsim_ir::runtime::{EvalInput, ModelFunctions, SparsityPattern};
use lsim_ir::{PreparedModel, Slot, VarId};
use synth::Rng;

/// The first of the three steps, relative to `1 + |y|` (the others are
/// its half and quarter): Richardson's error falls as h⁴, rounding grows
/// as 1/h.
const H: f64 = 1.0 / 8192.0;

/// An entry is compared when its two extrapolations agree to this much of
/// its row's scale (otherwise the differences have not converged).
const CONVERGED: f64 = 1e-7;

/// The name of the Jacobian-vector product from the sparse Jacobian.
const FROM_JACOBIAN: &str = "from the Jacobian";

/// What a check saw.
#[derive(Default)]
struct Tally {
    /// Jacobian entries and directional derivatives compared with the
    /// central differences
    compared: usize,
    /// compared with one side's differences: a kink at the point (the
    /// code takes one side of it)
    sided: usize,
    /// not compared: the differences did not converge (a kink within the
    /// steps)
    rough: usize,
    /// not compared: a direction through a kink at the point
    kinked: usize,
    /// not compared: a value of the function was not finite
    not_finite: usize,
    /// not compared: the function finite, the derivative not (an
    /// intermediate value overflowed)
    nan_derivative: usize,
    /// entries outside the pattern whose differences are exact zeros
    zeros: usize,
    /// the errors of those compared, relative to their row's scale (where
    /// that is well above the differences' rounding)
    errors: Vec<f64>,
}

impl std::fmt::Debug for Tally {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} compared, {} at a kink by one side; not compared: {} rough, {} directions \
             through a kink, {} not finite, {} derivatives not finite; {} exact zeros outside \
             the pattern; relative errors: median {:.1e}, 99 % {:.1e}, largest {:.1e}",
            self.compared,
            self.sided,
            self.rough,
            self.kinked,
            self.not_finite,
            self.nan_derivative,
            self.zeros,
            self.quantile(0.5),
            self.quantile(0.99),
            self.quantile(1.0)
        )
    }
}

impl Tally {
    fn add(&mut self, o: &Tally) {
        self.compared += o.compared;
        self.sided += o.sided;
        self.rough += o.rough;
        self.kinked += o.kinked;
        self.not_finite += o.not_finite;
        self.nan_derivative += o.nan_derivative;
        self.zeros += o.zeros;
        self.errors.extend(&o.errors);
    }

    fn all(&self) -> usize {
        self.compared + self.sided + self.not_compared()
    }

    /// The `q`-quantile of the relative errors.
    fn quantile(&self, q: f64) -> f64 {
        let mut e = self.errors.clone();
        e.sort_by(f64::total_cmp);
        e.get(((e.len() as f64 - 1.0) * q).round() as usize).copied().unwrap_or(0.0)
    }

    fn not_compared(&self) -> usize {
        self.rough + self.kinked + self.not_finite + self.nan_derivative
    }
}

/// Differences of `f` at `y` in direction `dir`, at three steps.
struct Diff {
    /// central differences, Richardson-extrapolated from the first two
    /// steps and from the last two (errors of order h⁴)
    r1: Vec<f64>,
    r2: Vec<f64>,
    /// one-sided differences forward and backward, extrapolated from all
    /// three steps (errors of order h³), and from the last two (h²)
    plus: [Vec<f64>; 2],
    minus: [Vec<f64>; 2],
    /// the largest |f| seen (at least the subject's unit; the caller
    /// raises it to the size of f's terms)
    fmax: Vec<f64>,
    /// f equal bit for bit on both sides at every step
    flat: Vec<bool>,
    /// every value finite
    finite: Vec<bool>,
    /// the smallest step
    h_min: f64,
}

fn differences(
    f: &mut dyn FnMut(&[f64], &mut [f64]),
    y: &[f64],
    dir: &[f64],
    h: f64,
    unit: f64,
) -> Diff {
    let n = y.len();
    let mut f0 = vec![0.0; n];
    f(y, &mut f0);
    // [central, forward, backward] at each step
    let mut d = vec![[vec![0.0; n], vec![0.0; n], vec![0.0; n]]; 3];
    let mut fmax: Vec<f64> = f0.iter().map(|x| x.abs().max(unit)).collect();
    let mut flat = vec![true; n];
    let mut finite: Vec<bool> = f0.iter().map(|x| x.is_finite()).collect();
    let (mut fp, mut fm) = (vec![0.0; n], vec![0.0; n]);
    let mut hk = h;
    for dk in &mut d {
        let yp: Vec<f64> = y.iter().zip(dir).map(|(a, v)| a + hk * v).collect();
        let ym: Vec<f64> = y.iter().zip(dir).map(|(a, v)| a - hk * v).collect();
        f(&yp, &mut fp);
        f(&ym, &mut fm);
        for i in 0..n {
            dk[0][i] = (fp[i] - fm[i]) / (2.0 * hk);
            dk[1][i] = (fp[i] - f0[i]) / hk;
            dk[2][i] = (f0[i] - fm[i]) / hk;
            fmax[i] = fmax[i].max(fp[i].abs()).max(fm[i].abs());
            // (equal values: a zero's sign may follow a branch)
            let same = |a: f64, b: f64| a == b || (a.is_nan() && b.is_nan());
            flat[i] &= same(fp[i], fm[i]) && same(fp[i], f0[i]);
            finite[i] &= fp[i].is_finite() && fm[i].is_finite();
        }
        hk *= 0.5;
    }
    let col = |s: usize, k: usize, i: usize| d[k][s][i];
    let rich2 = |s: usize| -> Vec<f64> {
        (0..n).map(|i| (4.0 * col(s, 2, i) - col(s, 1, i)) / 3.0).collect()
    };
    let side = |s: usize| -> [Vec<f64>; 2] {
        [
            (0..n)
                .map(|i| (col(s, 0, i) - 6.0 * col(s, 1, i) + 8.0 * col(s, 2, i)) / 3.0)
                .collect(),
            (0..n).map(|i| 2.0 * col(s, 2, i) - col(s, 1, i)).collect(),
        ]
    };
    Diff {
        r1: (0..n).map(|i| (4.0 * col(0, 1, i) - col(0, 0, i)) / 3.0).collect(),
        r2: rich2(0),
        plus: side(1),
        minus: side(2),
        fmax,
        flat,
        finite,
        h_min: 2.0 * hk,
    }
}

/// What a derivative is taken along.
#[derive(Clone, Copy, PartialEq)]
enum Along {
    Column,
    Direction,
}

/// Compares `got` with the differences `df` in row `i`, whose terms are
/// of size `scale`.
fn compare(what: &str, i: usize, got: f64, df: &Diff, scale: f64, along: Along, t: &mut Tally) {
    if !df.finite[i] {
        t.not_finite += 1;
        return;
    }
    if !got.is_finite() {
        t.nan_derivative += 1;
        return;
    }
    let (r1, r2) = (df.r1[i], df.r2[i]);
    let (plus, minus) = (df.plus[0][i], df.minus[0][i]);
    let est = (r2 - r1).abs();
    let kink = (plus - minus).abs();
    let size = r2.abs() + scale;
    // rounding in f over the smallest step (centrally, and on one side,
    // whose extrapolation weighs it more), and the steps' own rounding
    let noise = 64.0 * f64::EPSILON * df.fmax[i] / df.h_min + 1e-11 * scale;
    let noise_side = 8.0 * noise;
    let err;
    if est <= CONVERGED * size + noise && kink <= CONVERGED * size + noise_side {
        // smooth: the extrapolation's error estimate, the rounding, and
        // what the two sides still disagree by
        let tol = 4.0 * est + noise + kink;
        err = (got - r2).abs();
        assert!(
            err <= tol,
            "{what}, row {i}: {got:e}, the differences say {r2:e} (and {r1:e}): error {err:e} > {tol:e} \
             (terms of size {scale:e}, |f| up to {:e}, sides {plus:e} and {minus:e})",
            df.fmax[i]
        );
        t.compared += 1;
    } else {
        let e_plus = (plus - df.plus[1][i]).abs();
        let e_minus = (minus - df.minus[1][i]).abs();
        if e_plus.max(e_minus) > CONVERGED * size + noise_side {
            t.rough += 1;
            return;
        }
        if along == Along::Direction {
            // a direction may cross several kinks at once, each of whose
            // sides the code takes on its own
            t.kinked += 1;
            return;
        }
        // a kink at the point: each side smooth, their slopes apart; the
        // code differentiates one side, or takes their mean (as the
        // derivative of |x| at 0 is 0)
        let tol = 4.0 * e_plus.max(e_minus) + noise_side;
        err = [plus, minus, 0.5 * (plus + minus)]
            .iter()
            .map(|s| (got - s).abs())
            .fold(f64::INFINITY, f64::min);
        assert!(
            err <= tol,
            "{what}, row {i}: {got:e} at a kink, whose sides say {plus:e} and {minus:e}: error {err:e} > {tol:e}"
        );
        t.sided += 1;
    }
    // (the relative error where the differences resolve it)
    if size > 1e3 * noise {
        t.errors.push(err / size);
    }
}

/// A vector function of a vector: `f(x, out)`.
type VecFn<'a> = Box<dyn FnMut(&[f64], &mut [f64]) + 'a>;

/// A function of y with its derivatives at one point: the residual of a
/// model or of its initialisation system, at fixed t, p, d and u.
struct Subject<'a> {
    name: String,
    y: Vec<f64>,
    pattern: &'a SparsityPattern,
    /// the residual
    f: VecFn<'a>,
    /// the sparse Jacobian's values at y
    jac: Vec<f64>,
    /// Jacobian-vector products at y (of v), by name
    jvps: Vec<(&'static str, VecFn<'a>)>,
    /// derivatives found not finite before the check (where the function
    /// is)
    nan_derivative: usize,
    /// the size of the function's intermediate values, at least: its
    /// rounding errors are at least this size's (a random model's
    /// `log(1 + x²)` is 0 for a tiny x, its rate 2x is not)
    unit: f64,
}

/// Every column and a few directions of `s` against central differences.
fn check(s: &mut Subject<'_>, r: &mut Rng) -> Tally {
    let mut t = Tally { nan_derivative: s.nan_derivative, ..Default::default() };
    let n = s.y.len();
    let pat = s.pattern;
    assert_eq!(pat.n, n);
    // columns: the differences, then the entries
    let mut cols = Vec::with_capacity(n);
    for j in 0..n {
        let mut e = vec![0.0; n];
        e[j] = 1.0;
        cols.push(differences(&mut s.f, &s.y, &e, H * (1.0 + s.y[j].abs()), s.unit));
    }
    // each row's terms, as the differences see them
    let mut row_scale = vec![0.0; n];
    for (c, yj) in cols.iter().zip(&s.y) {
        for (rs, r2) in row_scale.iter_mut().zip(&c.r2) {
            if r2.is_finite() {
                *rs += r2.abs() * (1.0 + yj.abs());
            }
        }
    }
    // f's rounding: of the size of its terms at least (a residual
    // `a - b` near zero rounds as a and b do)
    for c in &mut cols {
        for (m, r) in c.fmax.iter_mut().zip(&row_scale) {
            *m = m.max(*r);
        }
    }
    for (j, c) in cols.iter().enumerate() {
        let mut in_pattern = vec![None; n];
        for k in pat.col_ptr[j]..pat.col_ptr[j + 1] {
            in_pattern[pat.row_idx[k]] = Some(k);
        }
        for i in 0..n {
            match in_pattern[i] {
                Some(k) => {
                    let what = format!("{}: J[{i}][{j}]", s.name);
                    let scale = row_scale[i] / (1.0 + s.y[j].abs());
                    compare(&what, i, s.jac[k], c, scale, Along::Column, &mut t);
                }
                None if c.flat[i] => t.zeros += 1,
                None => {
                    // a branch's condition reads y_j (which the pattern
                    // leaves out: its derivative is zero but at the
                    // switch), or the pattern misses a dependence: the
                    // differences must not converge to a non-zero slope
                    let what = format!("{}: J[{i}][{j}] (outside the pattern)", s.name);
                    let scale = row_scale[i] / (1.0 + s.y[j].abs());
                    compare(&what, i, 0.0, c, scale, Along::Column, &mut t);
                }
            }
        }
    }
    // directions, some with zeros, scaled like the steps
    for _ in 0..3 {
        let v: Vec<f64> = s
            .y
            .iter()
            .map(|y| {
                if r.below(4) == 0 {
                    0.0
                } else {
                    (1.0 + y.abs()) * r.range(0.5, 1.0) * if r.below(2) == 0 { -1.0 } else { 1.0 }
                }
            })
            .collect();
        let mut df = differences(&mut s.f, &s.y, &v, H, s.unit);
        for (m, r) in df.fmax.iter_mut().zip(&row_scale) {
            *m = m.max(*r);
        }
        // the size of each row's terms, and its product from the sparse
        // values
        let (mut scale, mut jv) = (vec![0.0; n], vec![0.0; n]);
        for (j, (c, vj)) in cols.iter().zip(&v).enumerate() {
            for (sc, r2) in scale.iter_mut().zip(&c.r2) {
                if r2.is_finite() {
                    *sc += (r2 * vj).abs();
                }
            }
            for k in pat.col_ptr[j]..pat.col_ptr[j + 1] {
                jv[pat.row_idx[k]] += s.jac[k] * vj;
            }
        }
        for (name, jvp) in &mut s.jvps {
            let mut out = vec![0.0; n];
            jvp(&v, &mut out);
            for i in 0..n {
                let what = format!("{}: jvp ({name})", s.name);
                compare(&what, i, out[i], &df, scale[i], Along::Direction, &mut t);
            }
            if *name == FROM_JACOBIAN {
                // the sparse values' own product, bit for bit
                assert!(
                    out.iter()
                        .zip(&jv)
                        .all(|(a, b)| a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())),
                    "{}: jvp ({name}) {out:?} but the Jacobian's product is {jv:?}",
                    s.name
                );
            }
        }
    }
    t
}

/// The model's residual, Jacobian and products at `(t, y, d)`; `own` is
/// the same model compiled with its own forward-mode code.
fn model_subject<'a>(
    name: &str,
    jit: &'a JitModel,
    own: &'a JitModel,
    (p, u): (&'a [f64], &'a [f64]),
    (t, y, d): (f64, &[f64], &'a [f64]),
) -> Subject<'a> {
    let l = *jit.layout();
    let nw = l.n_work.max(own.layout().n_work);
    let n = l.n_y();
    let pattern = jit.pattern();
    let y0 = y.to_vec();
    let mut w = vec![0.0; nw];
    let mut jac = vec![0.0; pattern.nnz()];
    jit.jacobian_sparse(&EvalInput { t, y: &y0, p, d, u }, &mut w, &mut jac);

    // the dense Jacobian is the sparse values in place
    let mut dense = vec![f64::NAN; n * n];
    jit.jacobian_dense(&EvalInput { t, y: &y0, p, d, u }, &mut w, &mut dense);
    let mut want = vec![0.0; n * n];
    for j in 0..n {
        for k in pattern.col_ptr[j]..pattern.col_ptr[j + 1] {
            want[j * n + pattern.row_idx[k]] = jac[k];
        }
    }
    assert!(
        dense.iter().zip(&want).all(|(a, b)| a.to_bits() == b.to_bits()),
        "{name}: the dense Jacobian is not the sparse values in place"
    );
    // the forward-mode code's columns are the coloured Jacobian's (where
    // an intermediate value overflows, the forward-mode code multiplies
    // the other columns' infinite rates by their zero seeds, NaN, which
    // the coloured sweep never forms: counted, not compared)
    let mut col = vec![0.0; n];
    let mut nan_derivative = 0;
    for j in 0..n {
        let mut e = vec![0.0; n];
        e[j] = 1.0;
        own.jvp(&EvalInput { t, y: &y0, p, d, u }, &e, &mut w, &mut col);
        for (i, c) in col.iter().enumerate() {
            let want = want[j * n + i];
            if !c.is_finite() && want.is_finite() {
                nan_derivative += 1;
                continue;
            }
            assert!(
                *c == want || (c.is_nan() && want.is_nan()),
                "{name}: column {j} of the forward-mode code, row {i}: {c} vs {want}"
            );
        }
    }

    let (yv, yo) = (y0.clone(), y0.clone());
    let mut wf = vec![0.0; nw];
    let mut wv = vec![0.0; nw];
    let mut wo = vec![0.0; nw];
    Subject {
        name: format!("{name} at t = {t}"),
        y: y0,
        pattern,
        f: Box::new(move |y: &[f64], out: &mut [f64]| {
            jit.residual(&EvalInput { t, y, p, d, u }, &mut wf, out)
        }),
        jac,
        jvps: vec![
            (
                FROM_JACOBIAN,
                Box::new(move |v: &[f64], out: &mut [f64]| {
                    jit.jvp(&EvalInput { t, y: &yv, p, d, u }, v, &mut wv, out)
                }),
            ),
            (
                "its own code",
                Box::new(move |v: &[f64], out: &mut [f64]| {
                    own.jvp(&EvalInput { t, y: &yo, p, d, u }, v, &mut wo, out)
                }),
            ),
        ],
        nan_derivative,
        unit: 0.0,
    }
}

fn compile_both(m: &PreparedModel) -> (JitModel, JitModel) {
    let jit = compile(m, &CodegenOptions::default()).expect("compiles");
    let own =
        compile(m, &CodegenOptions { compile_jvp: true, ..Default::default() }).expect("compiles");
    (jit, own)
}

#[test]
fn random_smooth_models_have_the_derivatives_of_their_differences() {
    let mut r = Rng(0x5eed_1ac0b1);
    let mut total = Tally::default();
    for case in 0..240 {
        let (n_x, n_z) = (1 + r.below(6), r.below(4));
        let m = random::implicit_model(&mut r, n_x, n_z, 8, 0, 4, random::SMOOTH);
        let (jit, own) = compile_both(&m);
        assert_eq!(jit.layout().n_z, n_z);
        let p = synth::params(&m);
        let l = *jit.layout();
        let (mut y0, mut d) = (vec![0.0; l.n_y()], vec![0.0; l.n_d]);
        jit.start(&p, &mut y0, &mut d);
        let u = vec![0.0; l.n_u];
        for _ in 0..3 {
            let t = r.range(0.0, 10.0);
            let y: Vec<f64> = (0..l.n_y()).map(|_| r.range(-2.0, 2.0)).collect();
            let mut s = model_subject(&format!("case {case}"), &jit, &own, (&p, &u), (t, &y, &d));
            // (leaves, constants and parameters of order one)
            s.unit = 1.0;
            total.add(&check(&mut s, &mut r));
        }
    }
    println!("random smooth models: {total:?}");
    assert!(total.compared >= 20_000, "{total:?}");
    // smooth but for the tables' breakpoints (their second derivatives
    // jump) and a rare overflow
    assert!(total.not_compared() <= total.all() / 100, "{total:?}");
}

/// Operating points of a car: along a simulated drive, or (for a model
/// with sampled blocks, which need their host) its start moved about.
fn operating_points(
    car: &cars::Car,
    jit: &JitModel,
    r: &mut Rng,
    every: usize,
) -> Vec<(f64, Vec<f64>, Vec<f64>)> {
    if car.sampled {
        let l = *jit.layout();
        let p = synth::params(&car.model);
        let (mut y0, mut d0) = (vec![0.0; l.n_y()], vec![0.0; l.n_d]);
        jit.start(&p, &mut y0, &mut d0);
        (0..8)
            .map(|i| {
                let y = y0.iter().map(|x| x * r.range(0.8, 1.2) + r.range(-1.0, 1.0)).collect();
                (i as f64 * 7.5, y, d0.clone())
            })
            .collect()
    } else {
        cars::trajectory(&car.model, jit, 120.0).into_iter().step_by(every).collect()
    }
}

/// Operating points of the implicit form `im` of a car, from the drive
/// of its explicit form `ex`: the states, the iteration variables and the
/// discrete values from the explicit model's channels (a derivative from
/// its residual).
fn implicit_points(
    ex: &PreparedModel,
    ex_jit: &JitModel,
    im: &PreparedModel,
    every: usize,
) -> Vec<(f64, Vec<f64>, Vec<f64>)> {
    let l = *ex_jit.layout();
    let p = synth::params(ex);
    let u = vec![0.0; l.n_u];
    let mut w = vec![0.0; l.n_work];
    let (mut vars, mut der) = (vec![0.0; l.n_vars], vec![0.0; l.n_y()]);
    let mut out = vec![];
    for (t, y, d) in cars::trajectory(ex, ex_jit, 120.0).into_iter().step_by(every) {
        let inp = EvalInput { t, y: &y, p: &p, d: &d, u: &u };
        ex_jit.vars(&inp, &mut w, &mut vars);
        ex_jit.residual(&inp, &mut w, &mut der);
        let der_of = |v: VarId| der[ex.states.iter().position(|x| *x == v).expect("a state")];
        let yi = im
            .states
            .iter()
            .map(|v| vars[v.0 as usize])
            .chain(im.algebraics.iter().map(|s| match s {
                Slot::Var(v) => vars[v.0 as usize],
                Slot::Der(v) => der_of(*v),
            }))
            .collect();
        let di = im.discretes.iter().map(|v| vars[v.0 as usize]).collect();
        out.push((t, yi, di));
    }
    out
}

fn example_projects(implicit: bool) -> Tally {
    let mut r = Rng(29);
    let mut total = Tally::default();
    let explicit = cars::one_per_project(cars::cars(""));
    let cars = if implicit {
        let opts = lsim_prep::PrepOptions { force_implicit: true };
        cars::one_per_project(cars::cars_with("", &opts))
    } else {
        cars::one_per_project(cars::cars(""))
    };
    assert!(cars.len() >= 4);
    for (car, ex) in cars.iter().zip(&explicit) {
        assert_eq!(car.name, ex.name);
        let (jit, own) = compile_both(&car.model);
        let points = if implicit && !car.sampled {
            assert!(jit.layout().n_z > jit.layout().n_x, "{}: {:?}", car.name, jit.layout());
            let ex_jit = compile(&ex.model, &CodegenOptions::default()).expect("compiles");
            implicit_points(&ex.model, &ex_jit, &car.model, 20)
        } else {
            operating_points(car, &jit, &mut r, 20)
        };
        let p = synth::params(&car.model);
        let u = vec![0.0; jit.layout().n_u];
        let mut tally = Tally::default();
        for (t, y, d) in points {
            let mut s = model_subject(&car.name, &jit, &own, (&p, &u), (t, &y, &d));
            tally.add(&check(&mut s, &mut r));
        }
        println!("{} ({} unknowns): {tally:?}", car.name, jit.layout().n_y());
        total.add(&tally);
    }
    total
}

#[test]
fn the_example_projects_have_the_derivatives_of_their_differences() {
    let total = example_projects(false);
    println!("example projects: {total:?}");
    assert!(total.compared >= 5_000, "{total:?}");
    // kinks (limits, breakpoints) within the steps
    assert!(total.not_compared() <= total.all() / 20, "{total:?}");
}

#[test]
fn the_example_projects_with_every_block_implicit_have_the_derivatives_of_their_differences() {
    let total = example_projects(true);
    println!("example projects, every block implicit: {total:?}");
    assert!(total.compared >= 5_000, "{total:?}");
    assert!(total.not_compared() <= total.all() / 20, "{total:?}");
}

#[test]
fn initialisation_systems_have_the_derivatives_of_their_differences() {
    let mut r = Rng(31);
    let mut total = Tally::default();
    let mut systems = 0;
    for car in cars::one_per_project(cars::cars("")) {
        let m = &car.model;
        let jit = compile(m, &CodegenOptions::default()).expect("compiles");
        let Some(init) = jit.init() else { continue };
        systems += 1;
        let p = synth::params(m);
        let l = *jit.layout();
        let (mut y0, mut d0) = (vec![0.0; l.n_y()], vec![0.0; l.n_d]);
        jit.start(&p, &mut y0, &mut d0);
        let u = vec![0.0; l.n_u];
        let n = init.n_w();
        let mut w0 = vec![0.0; n];
        init.guess(&p, &mut w0);
        let mut work = vec![0.0; l.n_work];
        let mut tally = Tally::default();
        for k in 0..16 {
            let w: Vec<f64> = if k == 0 {
                w0.clone()
            } else {
                w0.iter().map(|x| x * r.range(0.9, 1.1) + r.range(-0.5, 0.5)).collect()
            };
            let inp = EvalInput { t: 0.0, y: &w, p: &p, d: &d0, u: &u };
            let mut jac = vec![0.0; init.sparsity().nnz()];
            init.jacobian_sparse(&inp, &mut work, &mut jac);
            let (mut wf, mut wv) = (vec![0.0; l.n_work], vec![0.0; l.n_work]);
            let (p, d0, u, wj) = (&p, &d0, &u, w.clone());
            let mut s = Subject {
                name: format!("{} (initialisation)", car.name),
                y: w.clone(),
                pattern: init.sparsity(),
                f: Box::new(move |y: &[f64], out: &mut [f64]| {
                    init.residual(&EvalInput { t: 0.0, y, p, d: d0, u }, &mut wf, out)
                }),
                jac,
                jvps: vec![(
                    "its own code",
                    Box::new(move |v: &[f64], out: &mut [f64]| {
                        init.jvp(&EvalInput { t: 0.0, y: &wj, p, d: d0, u }, v, &mut wv, out)
                    }),
                )],
                nan_derivative: 0,
                unit: 0.0,
            };
            tally.add(&check(&mut s, &mut r));
        }
        println!("{} initialisation ({n} unknowns): {tally:?}", car.name);
        total.add(&tally);
    }
    println!("initialisation systems: {total:?}");
    assert!(systems >= 4, "{systems} initialisation systems");
    assert!(total.compared >= 2_000, "{total:?}");
    assert!(total.not_compared() <= total.all() / 20, "{total:?}");
}
