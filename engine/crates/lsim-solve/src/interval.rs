//! The search for the next sign change of a function of time
//! ([`first_change`]), and the integrator's dense output as interval
//! polynomials over a step ([`Basis`], [`DensePoly`]) (DESIGN.md §8.2,
//! *Conditions on explicit functions of time*). The enclosures themselves
//! are `lsim_ir::interval`'s.
//!
//! Root finding sees a condition only by its sign at the ends of the
//! integrator's steps: a pulse narrower than a step (`sin(2π time / T) >
//! 0.95` while nothing integrated moves and the steps grow long) is stepped
//! over. A function of time alone between events (with parameters and
//! discrete values) needs no integration to find where it changes sign:
//! `J2` encloses the function, its rate and its second rate over a time
//! interval, rigorously, and [`first_change`] walks forward in time.
//! It skips an interval whose enclosure keeps the sign, or where the
//! function is monotone with the same sign at both ends (a comparison
//! whose sides' difference is strictly monotone over the interval flips
//! once at most, one way: a step of known sign, monotone too); otherwise it
//! advances by what the bound on the rate allows (a function of value g
//! and rate at most L cannot reach zero within |g| / L), shrinking the
//! interval as it nears a zero, and bisects to adjacent floats once a sign
//! change is bracketed. A grazing touch without a sign change is passed.

#[cfg(test)]
use lsim_ir::expr::{BinaryOp, CmpOp, Expr};
pub(crate) use lsim_ir::interval::{ALL, Cx, Grid2, Iv, J2, ONE, ZERO, enclose, supported};
#[cfg(test)]
use lsim_ir::runtime::ModelFunctions;
#[cfg(test)]
use std::f64::consts::PI;

/// Bounds on the size over the step `[t0, t1]` of each function of the
/// Newton basis `x`, `s` about `origin` ([`Basis`]), into `out` (`|β_j| ≤
/// out[j]`): the product of each factor's size at the step's ends, rounded
/// up. Cheaper than [`Basis::set`], and looser.
pub(crate) fn basis_sizes(
    origin: f64,
    x: &[f64],
    s: &[f64],
    (t0, t1): (f64, f64),
    out: &mut Vec<f64>,
) {
    let q = x.len().min(s.len());
    out.resize(q + 1, 1.0);
    out[0] = 1.0;
    // τ at the ends, each within half an ulp of the exact value
    let (a, b) = (t0 - origin, t1 - origin);
    let et = 2.0 * f64::EPSILON * a.abs().max(b.abs());
    // |τ − x| is largest at an end; three roundings in each factor and one
    // in each product, 4 j + 1 half-ulps for β_j: the products start from
    // a factor above 1 that covers them (NaN spreads: the bound then fails
    // every check)
    let mut p = 1.0 + 4.0 * (q as f64 + 2.0) * f64::EPSILON;
    for (o, (xi, si)) in out[1..].iter_mut().zip(x.iter().zip(s)) {
        p *= ((a - xi).abs().max((b - xi).abs()) + et) / si.abs();
        *o = p;
    }
}

/// A bound on how far `Σ a_j β_j` strays from `a_0` over the step (`sizes`
/// from [`basis_sizes`]), `err` beyond it, rounded up so that `a_0 ∓` it in
/// floating point lies outside every value (infinite when the sizes are
/// not those of this polynomial's basis).
pub(crate) fn stray(a: &[f64], sizes: &[f64], err: f64) -> f64 {
    if a.is_empty() || a.len() > sizes.len() {
        return f64::INFINITY;
    }
    let mut r = 0.0f64;
    for (aj, sj) in a[1..].iter().zip(&sizes[1..]) {
        r += aj.abs() * sj;
    }
    let n = a.len() as f64;
    (r + err) * (1.0 + 2.0 * (n + 2.0) * f64::EPSILON)
        + 2.0 * f64::EPSILON * a[0].abs()
        + f64::MIN_POSITIVE
}

/// The product of `[a, b]` and `[c, d]`, all four finite (`a ≤ b`, `c ≤
/// d`), each end the product of two of them, rounded to nearest.
fn mul_ends(a: f64, b: f64, c: f64, d: f64) -> (f64, f64) {
    if a >= 0.0 {
        if c >= 0.0 {
            (a * c, b * d)
        } else if d <= 0.0 {
            (b * c, a * d)
        } else {
            (b * c, b * d)
        }
    } else if b <= 0.0 {
        if c >= 0.0 {
            (a * d, b * c)
        } else if d <= 0.0 {
            (b * d, a * c)
        } else {
            (a * d, a * c)
        }
    } else if c >= 0.0 {
        (a * d, b * d)
    } else if d <= 0.0 {
        (b * c, a * c)
    } else {
        let (p, q, r, s) = (a * d, b * c, a * c, b * d);
        (if p <= q { p } else { q }, if r >= s { r } else { s })
    }
}

/// The Newton basis of a dense output over a step, `β_j(τ) = Π_{i<j} (τ −
/// x_i) / s_i`, `τ = t − origin`: each function's range over the step
/// (rounded outwards), and its coefficients in powers of τ (made when a
/// step is searched). Kept between steps (no allocation).
#[derive(Clone, Debug, Default)]
pub(crate) struct Basis {
    /// the step
    t0: f64,
    t1: f64,
    /// per function, its range over the step as a middle and a radius,
    /// and a bound on its magnitude there (`[mid, rad, mag]`; `β_0 = 1`)
    ranges: Vec<[f64; 3]>,
    /// per function, its coefficients in powers of τ (empty until asked)
    mono: Vec<Vec<Iv>>,
}

impl Basis {
    /// The basis `x`, `s` about `origin` over the step `[t0, t1]`.
    pub(crate) fn set(&mut self, origin: f64, x: &[f64], s: &[f64], t0: f64, t1: f64) {
        (self.t0, self.t1) = (t0.min(t1), t0.max(t1));
        self.mono.clear();
        self.ranges.clear();
        self.ranges.push([1.0, 0.0, 1.0]);
        // τ over the step, each end within half an ulp of the exact value
        let (a, b) = (self.t0 - origin, self.t1 - origin);
        let reach = a.abs().max(b.abs());
        let et = f64::EPSILON * reach;
        let (mut lo, mut hi) = (1.0f64, 1.0f64);
        let up = 1.0 + 2.0 * f64::EPSILON;
        for (xi, si) in x.iter().zip(s) {
            // the factor (τ − x) / s over the step: two roundings in each end,
            // the first's error absolute (the second may cancel exactly)
            let (p, q) = ((a - et - xi) / si, (b + et - xi) / si);
            let (flo, fhi) = if p <= q { (p, q) } else { (q, p) };
            let ef = 4.0 * f64::EPSILON * ((reach + xi.abs()) / si.abs())
                + 2.0 * f64::EPSILON * flo.abs().max(fhi.abs());
            let (flo, fhi) = (flo - ef, fhi + ef);
            // (a step or node that is not finite: no bound)
            if !(flo >= -f64::MAX && fhi <= f64::MAX) {
                self.unbounded(x.len().min(s.len()));
                return;
            }
            // its product with the range so far, each end within half an ulp
            let (nlo, nhi) = mul_ends(lo, hi, flo, fhi);
            let ep = f64::EPSILON * nlo.abs().max(nhi.abs());
            (lo, hi) = (nlo - ep, nhi + ep);
            // (an overflow: no bound)
            if !(lo >= -f64::MAX && hi <= f64::MAX) {
                self.unbounded(x.len().min(s.len()));
                return;
            }
            // as a middle and a radius (rounded up: the two hold the range)
            let mid = 0.5 * (lo + hi);
            let rad = (hi - mid).max(mid - lo) * up + f64::MIN_POSITIVE;
            self.ranges.push([mid, rad, (mid.abs() + rad) * up]);
        }
    }

    /// The functions from the last one set up to `β_q` unbounded.
    fn unbounded(&mut self, q: usize) {
        self.ranges.resize(q + 1, [0.0, f64::INFINITY, f64::INFINITY]);
    }

    /// The range over the step of `Σ a_j β_j` (sums of products in
    /// floating point, their round-off bounded).
    pub(crate) fn range(&self, a: &[f64]) -> Iv {
        let Some((&a0, rest)) = a.split_first() else {
            return ZERO;
        };
        if a.len() > self.ranges.len() {
            // (not the basis this polynomial is in)
            return ALL;
        }
        // the middle, the radius and the size (β_0 = 1)
        let (mut c, mut r, mut m) = (a0, 0.0f64, a0.abs());
        for (aj, [mid, rad, mag]) in rest.iter().zip(&self.ranges[1..]) {
            c += aj * mid;
            r += aj.abs() * rad;
            m += aj.abs() * mag;
        }
        let e = 2.0 * (a.len() as f64 + 1.0) * f64::EPSILON * m + f64::MIN_POSITIVE;
        Iv::new(c - r - e, c + r + e)
    }

    /// The polynomial `Σ a_j β_j` of the Newton form `origin`, `x`, `s`
    /// (the one the basis was [`Basis::set`] to: an entry of the dense
    /// output), within `err` of the integrator's interpolant over the step.
    pub(crate) fn poly(
        &mut self,
        (origin, x, s): (f64, &[f64], &[f64]),
        a: &[f64],
        err: f64,
    ) -> DensePoly {
        let q = x.len();
        if self.mono.len() != q + 1 {
            // β_j = β_{j−1} (τ − x_{j−1}) / s_{j−1}, in powers of τ
            self.mono.clear();
            self.mono.push(vec![ONE]);
            for j in 1..=q {
                let (xj, rs) = (Iv::point(x[j - 1]), Iv::point(s[j - 1]).recip());
                let prev = &self.mono[j - 1];
                let next: Vec<Iv> = (0..=j)
                    .map(|k| {
                        let up = if k >= 1 { prev[k - 1] } else { ZERO };
                        let down = if k < j { prev[k].mul(xj) } else { ZERO };
                        up.sub(down).mul(rs)
                    })
                    .collect();
                self.mono.push(next);
            }
        }
        let mut c = vec![ZERO; q + 1];
        for (aj, bj) in a.iter().zip(&self.mono) {
            for (ck, bk) in c.iter_mut().zip(bj) {
                *ck = ck.add(bk.scale(*aj));
            }
        }
        let dc: Vec<Iv> = (1..=q).map(|k| c[k].scale(k as f64)).collect();
        let ddc: Vec<Iv> = (2..=q).map(|k| c[k].scale((k * (k - 1)) as f64)).collect();
        // the floating-point evaluation's round-off over the step: a few ε
        // of each term's size
        let size: f64 = a.iter().zip(&self.ranges).map(|(aj, bj)| aj.abs() * bj[2]).sum();
        let round = 4.0 * (q as f64 + 2.0) * f64::EPSILON * size;
        // Markov's inequality on the step for the rates' share of `err`
        let len = self.t1 - self.t0;
        let n2 = (q * q) as f64;
        let err = if err > 0.0 && len > 0.0 {
            [err, 2.0 * n2 / len * err, 4.0 * n2 * (n2 - 1.0) / (3.0 * len * len) * err]
        } else {
            [err, 0.0, 0.0]
        };
        DensePoly { origin, x: x.to_vec(), s: s.to_vec(), a: a.to_vec(), c, dc, ddc, err, round }
    }
}

/// A variable along a step: an entry of the integrator's dense output over
/// the step ([`Basis::poly`]), its Newton form expanded into powers of τ
/// with interval coefficients rounded outwards, so that its enclosures hold
/// that polynomial itself. `err` bounds its distance from the integrator's
/// interpolant (0 when it is that interpolant, as SUNDIALS holds it), the
/// rates' share by Markov's inequality over the step; `round` bounds the
/// round-off of evaluating it in floating point ([`DensePoly::at`]), by
/// which the value's enclosure is widened so that the point values lie
/// inside it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DensePoly {
    origin: f64,
    x: Vec<f64>,
    s: Vec<f64>,
    a: Vec<f64>,
    c: Vec<Iv>,
    dc: Vec<Iv>,
    ddc: Vec<Iv>,
    err: [f64; 3],
    round: f64,
}

impl DensePoly {
    /// The value at `t` in floating point (the nested Newton form, as the
    /// integrators evaluate it).
    pub(crate) fn at(&self, t: f64) -> f64 {
        let tau = t - self.origin;
        let q = self.x.len();
        let mut acc = self.a[q];
        for j in (0..q).rev() {
            acc = self.a[j] + (tau - self.x[j]) / self.s[j] * acc;
        }
        acc
    }

    fn horner(c: &[Iv], tau: Iv) -> Iv {
        c.iter().rev().fold(ZERO, |p, a| p.mul(tau).add(*a))
    }

    /// The value over `t` alone (as [`DensePoly::j2`] gives it).
    pub(crate) fn value(&self, t: Iv) -> Iv {
        let tau = Iv::wide(t.lo - self.origin, t.hi - self.origin, 1);
        let v = Self::horner(&self.c, tau);
        let e = self.err[0] + self.round;
        Iv::new(v.lo - e, v.hi + e)
    }

    /// The value, rate and second rate over `t` (interval Horner).
    pub(crate) fn j2(&self, t: Iv) -> J2 {
        let tau = Iv::wide(t.lo - self.origin, t.hi - self.origin, 1);
        let widen = |x: Iv, e: f64| if e > 0.0 { Iv::new(x.lo - e, x.hi + e) } else { x };
        J2 {
            v: widen(Self::horner(&self.c, tau), self.err[0] + self.round),
            d: widen(Self::horner(&self.dc, tau), self.err[1]),
            dd: widen(Self::horner(&self.ddc, tau), self.err[2]),
        }
    }
}

/// What [`first_change`] found after the start.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Found {
    /// the first time the sign differs from the start's (the next float
    /// before it has the start's sign): rising when it goes up
    Change { at: f64, rising: bool },
    /// no change up to this time; the search ran out of steps there
    Clear(f64),
    /// no change before the end
    Nothing,
    /// the function is not defined (NaN) at the start
    Undefined,
}

fn sign(x: f64) -> i8 {
    if x > 0.0 {
        1
    } else if x < 0.0 {
        -1
    } else {
        0
    }
}

/// The first time after the instant `a` (not within a few ulps of it,
/// [`crate::run::same_instant`]) and up to `b` at which `p` has another
/// sign than just after `a` and keeps it (an exact zero that `p` leaves
/// with the sign it had is a touch, passed). `enc(l, r)` encloses p and
/// its rate over `[l, r]`. At most `budget` steps.
pub(crate) fn first_change(
    p: &mut dyn FnMut(f64) -> f64,
    enc: &mut dyn FnMut(f64, f64) -> (Iv, Iv),
    a: f64,
    b: f64,
    budget: usize,
) -> Found {
    // the first time that is not this instant (near zero, on the scale
    // of the horizon: no stop a few hundred ulps of the end time after 0)
    let scale = b.abs();
    let after = |a: f64| {
        let mut t = a + 17.0 * f64::EPSILON * a.abs().max(scale);
        while crate::run::same_instant(a, t) || t <= a {
            t = t.next_up();
        }
        t
    };
    let mut t = after(a);
    if t > b {
        return Found::Nothing;
    }
    let pt = p(t);
    if pt.is_nan() {
        return Found::Undefined;
    }
    let s0 = sign(pt);
    // p(t) when known
    let mut g = Some(pt);
    // the window ahead of t
    let mut w = b - t;
    let mut steps = 0;
    loop {
        if t >= b {
            return Found::Nothing;
        }
        steps += 1;
        if steps > budget {
            return Found::Clear(t);
        }
        // a few ulps: the least window (the integrators' instant)
        let least = 32.0 * f64::EPSILON * t.abs().max(scale).max(f64::MIN_POSITIVE);
        w = w.max(least);
        let minimal = w <= least;
        let r = (t + w).min(b);
        let (v, d) = enc(t, r);
        let keeps = match s0 {
            1 => v.lo > 0.0,
            -1 => v.hi < 0.0,
            _ => v == ZERO,
        };
        if keeps {
            t = r;
            g = None;
            w *= 2.0;
            continue;
        }
        // a sign change bracketed in (lo, hi]: down to adjacent floats
        let bracket = |p: &mut dyn FnMut(f64) -> f64, mut lo: f64, mut hi: f64| {
            loop {
                let m = lo + 0.5 * (hi - lo);
                if m <= lo || m >= hi {
                    break;
                }
                if sign(p(m)) == s0 {
                    lo = m;
                } else {
                    hi = m;
                }
            }
            let ph = p(hi);
            // an exact zero: the sign after it decides
            let s1 = if ph == 0.0 { sign(p(after(hi))) } else { sign(ph) };
            if ph == 0.0 && s1 == s0 && s0 != 0 { Err(after(hi)) } else { Ok((hi, s1 > s0)) }
        };
        let pr = p(r);
        // (monotone in the wide sense: a step of known sign counts)
        let monotone = s0 != 0 && (d.lo >= 0.0 || d.hi <= 0.0);
        let found = if monotone && sign(pr) == s0 {
            t = r;
            g = Some(pr);
            w *= 2.0;
            continue;
        } else if monotone || (minimal && sign(pr) != s0) {
            Some(bracket(p, t, r))
        } else if minimal {
            // at round-off: none inside
            t = r;
            g = Some(pr);
            continue;
        } else {
            // a safe advance: |p| cannot fall to zero within |p(t)| / L
            let gt = *g.get_or_insert_with(|| p(t));
            let l = d.mag();
            let step = if s0 != 0 && l.is_finite() { gt.abs() / l } else { 0.0 };
            if step >= r - t {
                t = r;
                g = Some(pr);
                w *= 2.0;
                continue;
            }
            let span = r - t;
            let mut out = None;
            let advance = step > least;
            if advance {
                let t1 = t + step * (1.0 - 1e-12);
                let p1 = p(t1);
                if sign(p1) != s0 {
                    out = Some(bracket(p, t, t1));
                } else {
                    t = t1;
                    g = Some(p1);
                }
            }
            // a bound loose over the window (the advance small beside it):
            // a narrower one
            w = if !advance || step < span / 8.0 { 0.5 * span } else { (r - t).max(least) };
            out
        };
        match found {
            Some(Ok((at, rising))) => return Found::Change { at, rising },
            Some(Err(on)) => {
                // a touch: on from just after it
                t = on;
                g = None;
                w = least;
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsim_ir::expr::Builtin;
    use lsim_ir::runtime::{EvalInput, Layout};

    struct NoModel(Layout);
    impl ModelFunctions for NoModel {
        fn layout(&self) -> &Layout {
            &self.0
        }
        fn residual(&self, _: &EvalInput<'_>, _: &mut [f64], _: &mut [f64]) {}
        fn jvp(&self, _: &EvalInput<'_>, _: &[f64], _: &mut [f64], _: &mut [f64]) {}
        fn roots(&self, _: &EvalInput<'_>, _: &mut [f64], _: &mut [f64]) {}
        fn vars(&self, _: &EvalInput<'_>, _: &mut [f64], _: &mut [f64]) {}
        fn when(&self, _: &EvalInput<'_>, _: &[f64], _: &mut [f64], _: &mut [f64]) {}
        fn start(&self, _: &[f64], _: &mut [f64], _: &mut [f64]) {}
    }

    fn no_model() -> NoModel {
        NoModel(Layout {
            n_x: 0,
            n_z: 0,
            n_p: 0,
            n_d: 0,
            n_u: 0,
            n_roots: 0,
            n_whens: 0,
            n_vars: 0,
            n_work: 0,
        })
    }

    fn bin(op: BinaryOp, a: Expr, b: Expr) -> Expr {
        Expr::Binary(op, Box::new(a), Box::new(b))
    }

    fn call(f: Builtin, a: Vec<Expr>) -> Expr {
        Expr::Call(f, a)
    }

    fn point(e: &Expr, t: f64) -> f64 {
        struct At(f64);
        impl lsim_ir::eval::Env for At {
            fn time(&self) -> f64 {
                self.0
            }
            fn var(&self, _: lsim_ir::VarId) -> f64 {
                f64::NAN
            }
            fn der(&self, _: lsim_ir::VarId) -> f64 {
                f64::NAN
            }
            fn param(&self, _: lsim_ir::ParamId) -> f64 {
                f64::NAN
            }
        }
        lsim_ir::eval::eval(e, &At(t))
    }

    /// A variable along a step, as the dense output's Newton form: its
    /// enclosures hold its value (as evaluated in floating point too) and
    /// rates everywhere inside, an error bound widens them, and the basis
    /// ranges bound its range.
    #[test]
    fn a_dense_output_leaf_holds_its_values_and_rates() {
        // a cubic in Newton form (IDA's kind of nodes and scales), about 3
        let (origin, x, s, a) = (3.0, [0.0, -0.3, -0.7], [0.3, 0.4, 0.5], [1.0, -2.0, 0.5, 3.0]);
        // its powers of τ, by hand: b1 = τ/0.3, b2 = b1 (τ + 0.3)/0.4,
        // b3 = b2 (τ + 0.7)/0.5
        let poly = |tau: f64| -> [f64; 3] {
            let b1 = [0.0, 1.0 / 0.3];
            let b2 = [0.0, 0.3 * b1[1] / 0.4, b1[1] / 0.4];
            let b3 = [0.0, 0.7 * b2[1] / 0.5, (0.7 * b2[2] + b2[1]) / 0.5, b2[2] / 0.5];
            let c = [
                a[0],
                a[1] * b1[1] + a[2] * b2[1] + a[3] * b3[1],
                a[2] * b2[2] + a[3] * b3[2],
                a[3] * b3[3],
            ];
            let v = c[0] + tau * (c[1] + tau * (c[2] + tau * c[3]));
            let d = c[1] + tau * (2.0 * c[2] + tau * 3.0 * c[3]);
            let dd = 2.0 * c[2] + 6.0 * c[3] * tau;
            [v, d, dd]
        };
        let mut basis = Basis::default();
        basis.set(origin, &x, &s, 2.3, 3.0);
        let p = basis.poly((origin, &x, &s), &a, 0.0);
        for (lo, hi) in [(2.3, 3.0), (2.9, 3.0), (2.5, 2.5), (2.3, 2.4)] {
            let j = p.j2(Iv::new(lo, hi));
            let v = p.value(Iv::new(lo, hi));
            for k in 0..=100 {
                let t = lo + (hi - lo) * k as f64 / 100.0;
                let [pv, pd, pdd] = poly(t - origin);
                let slack = |x: f64| 1e-13 * (1.0 + x.abs());
                let near = |i: Iv, x: f64| i.lo - slack(x) <= x && x <= i.hi + slack(x);
                assert!((p.at(t) - pv).abs() < 1e-12, "at {t}: {} {pv}", p.at(t));
                assert!(j.v.lo <= p.at(t) && p.at(t) <= j.v.hi, "[{lo}, {hi}] at {t}: {:?}", j.v);
                assert!(v.lo <= p.at(t) && p.at(t) <= v.hi);
                assert!(near(j.v, pv) && near(j.d, pd) && near(j.dd, pdd), "[{lo}, {hi}] at {t}");
            }
            // tight: a few ulps beyond the sampled spread
            assert!(j.v.hi - j.v.lo <= 2.0 * (hi - lo) * 50.0 + 1e-12);
        }
        // an error bound widens the value, and the rates by Markov's factor
        let wide = basis.poly((origin, &x, &s), &a, 1e-3).j2(Iv::new(3.0, 3.0));
        assert!(wide.v.hi - wide.v.lo >= 2e-3 && wide.d.hi - wide.d.lo >= 2.0 * 18.0 / 0.7 * 1e-3);
        // the basis ranges and the range of the sum hold every value, and so
        // does the bound on how far it strays from its first coefficient
        let r = basis.range(&a);
        let mut sizes = vec![];
        basis_sizes(origin, &x, &s, (2.3, 3.0), &mut sizes);
        let w = stray(&a, &sizes, 0.0);
        for k in 0..=100 {
            let t = 2.3 + 0.7 * k as f64 / 100.0;
            assert!(r.lo <= p.at(t) && p.at(t) <= r.hi, "{t}: {r:?}");
            assert!(a[0] - w <= p.at(t) && p.at(t) <= a[0] + w, "{t}: {w}");
        }
        // (looser than the range)
        assert!(2.0 * w >= r.hi - r.lo);
    }

    /// A sine pulse: every sign change found in order, exactly (the float
    /// before has the other sign), and nothing after the last.
    #[test]
    fn every_sign_change_of_a_pulse_is_found_in_order() {
        let period = 10.0;
        let e = bin(
            BinaryOp::Sub,
            call(
                Builtin::Sin,
                vec![bin(BinaryOp::Mul, Expr::Const(2.0 * PI / period), Expr::Time)],
            ),
            Expr::Const(0.95),
        );
        let m = no_model();
        let cx = Cx { params: &[], vars: &[], model: &m, breaks: &[], grids: &[], leaf: &|_| None };
        let mut p = |t: f64| point(&e, t);
        let mut enc = |l: f64, r: f64| {
            let j = enclose(&e, &cx, Iv::new(l, r));
            (j.v, j.d)
        };
        let half = (PI - 2.0 * 0.95f64.asin()) / (2.0 * PI) * period;
        let mut t = 0.0;
        let mut found = vec![];
        loop {
            match first_change(&mut p, &mut enc, t, 100.0, 1000) {
                Found::Change { at, rising } => {
                    found.push((at, rising));
                    t = at;
                }
                Found::Nothing => break,
                other => panic!("{other:?} after {t}"),
            }
        }
        assert_eq!(found.len(), 20, "{found:?}");
        for (k, (at, rising)) in found.iter().enumerate() {
            assert_eq!(*rising, k % 2 == 0);
            let centre = period / 4.0 + period * (k / 2) as f64;
            let want = if *rising { centre - half / 2.0 } else { centre + half / 2.0 };
            assert!((at - want).abs() < 1e-12 * period, "{k}: {at} vs {want}");
            let (before, here) = (p(at.next_down()), p(*at));
            assert!(before.signum() != here.signum() || here == 0.0, "{k}: {before} {here}");
        }
    }

    /// A pulse far narrower than the search's first window, a touch that
    /// does not change the sign, and a jump.
    #[test]
    fn a_narrow_pulse_and_a_jump_are_found_and_a_touch_is_passed() {
        let m = no_model();
        let cx = Cx { params: &[], vars: &[], model: &m, breaks: &[], grids: &[], leaf: &|_| None };
        let run = |e: &Expr, a: f64, b: f64| {
            let mut p = |t: f64| point(e, t);
            let mut enc = |l: f64, r: f64| {
                let j = enclose(e, &cx, Iv::new(l, r));
                (j.v, j.d)
            };
            first_change(&mut p, &mut enc, a, b, 2000)
        };
        // exp(-((t - 500) / 1e-3)²) - 0.5: zeros at 500 ∓ 1e-3 √ln 2
        let z = bin(
            BinaryOp::Div,
            bin(BinaryOp::Sub, Expr::Time, Expr::Const(500.0)),
            Expr::Const(1e-3),
        );
        let gauss = bin(
            BinaryOp::Sub,
            call(Builtin::Exp, vec![Expr::Neg(Box::new(bin(BinaryOp::Mul, z.clone(), z)))]),
            Expr::Const(0.5),
        );
        let w = 1e-3 * 2f64.ln().sqrt();
        match run(&gauss, 0.0, 1000.0) {
            Found::Change { at, rising: true } => assert!((at - (500.0 - w)).abs() < 1e-12, "{at}"),
            other => panic!("{other:?}"),
        }
        match run(&gauss, 500.0, 1000.0) {
            Found::Change { at, rising: false } => {
                assert!((at - (500.0 + w)).abs() < 1e-12, "{at}")
            }
            other => panic!("{other:?}"),
        }
        // (t - 3)² touches zero at 3 without changing sign
        let touch =
            bin(BinaryOp::Pow, bin(BinaryOp::Sub, Expr::Time, Expr::Const(3.0)), Expr::Const(2.0));
        assert_eq!(run(&touch, 0.0, 10.0), Found::Nothing);
        // a jump at 4
        let jump = Expr::If(
            Box::new(Expr::Compare(CmpOp::Gt, Box::new(Expr::Time), Box::new(Expr::Const(4.0)))),
            Box::new(Expr::Const(1.0)),
            Box::new(Expr::Const(-1.0)),
        );
        match run(&jump, 0.0, 10.0) {
            Found::Change { at, rising: true } => assert_eq!(at, 4f64.next_up()),
            other => panic!("{other:?}"),
        }
    }
}

#[cfg(test)]
mod table2_tests {
    use lsim_ir::interval::{Grid2, Iv, regions};
    use lsim_ir::runtime::ModelFunctions;
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
        fn table_axes(&self, k: u32) -> Option<[Vec<f64>; 2]> {
            let second = if self.0.dims() == 2 { self.0.points(1).to_vec() } else { vec![] };
            (k == 0).then(|| [self.0.points(0).to_vec(), second])
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
