//! The tables' interpolants (DESIGN.md §16, WP3's acceptance: "tables C¹
//! and monotone wherever their data are"), on random data: 1-D and 2-D,
//! uniform and uneven axes, monotone, oscillating, flat and plateaued
//! data over several orders of magnitude.
//!
//! * Monotone cubic (Steffen in 1-D, the monotone bicubic in 2-D): the
//!   value and every partial derivative are continuous across every
//!   breakpoint and cell edge, and across the data's edge when outside is
//!   `Linear` or `Error` (C¹); the value meets the data at the
//!   breakpoints; along an axis, in every cell whose data are monotone
//!   that way, the interpolant is monotone and stays within the data
//!   (no overshoot), and its partial derivative has the data's sign. In
//!   1-D that is every interval (Steffen's slopes vanish at an extremum).
//! * Linear (bilinear): continuous, meets the data, monotone the same way.
//!
//! Tolerances are a few roundings of the values and rates involved.

#[path = "common/synth.rs"]
mod synth;

use lsim_codegen::tables::Table;
use lsim_ir::table::{Interpolation, Outside, TableData};
use synth::Rng;

/// A random axis of `n` points: uniform or uneven (widths over three
/// orders of magnitude).
fn axis(r: &mut Rng, n: usize) -> Vec<f64> {
    let mut x = r.range(-50.0, 50.0);
    let uniform = r.below(3) == 0;
    let h = 10f64.powf(r.range(-2.0, 2.0));
    (0..n)
        .map(|_| {
            let v = x;
            x += if uniform { h } else { h * 10f64.powf(r.range(-1.5, 1.5)) };
            v
        })
        .collect()
}

/// Random data of `n` values: rising, falling, oscillating, with flat
/// stretches, of a random scale.
fn values(r: &mut Rng, n: usize) -> Vec<f64> {
    let scale = 10f64.powf(r.range(-6.0, 6.0));
    let kind = r.below(4);
    let mut v = r.range(-1.0, 1.0);
    (0..n)
        .map(|_| {
            let step = r.range(0.0, 1.0) * if r.below(4) == 0 { 0.0 } else { 1.0 };
            v += match kind {
                0 => step,
                1 => -step,
                2 => step * if r.below(2) == 0 { 1.0 } else { -1.0 },
                _ => step * if r.below(5) == 0 { -1.0 } else { 1.0 },
            };
            v * scale
        })
        .collect()
}

/// The sign of a data step: 1, -1, or 0 (flat).
fn sign(d: f64) -> f64 {
    if d > 0.0 {
        1.0
    } else if d < 0.0 {
        -1.0
    } else {
        0.0
    }
}

/// Sizes for tolerances: of the values, of the rates (the steepest
/// secant), and of the rates' rates (a Hermite cubic's second derivative
/// is at most a few secants over the interval's width).
struct Scale {
    v: f64,
    d: f64,
    dd: f64,
}

fn scale_1d(x: &[f64], f: &[f64]) -> Scale {
    let v = f.iter().fold(0.0f64, |m, a| m.max(a.abs()));
    let d = x
        .windows(2)
        .zip(f.windows(2))
        .map(|(x, f)| ((f[1] - f[0]) / (x[1] - x[0])).abs())
        .fold(0.0f64, f64::max);
    let d = d.max(v / (x[x.len() - 1] - x[0]));
    let hmin = x.windows(2).map(|w| w[1] - w[0]).fold(f64::INFINITY, f64::min);
    Scale { v: v.max(f64::MIN_POSITIVE), d, dd: 16.0 * d / hmin }
}

const ROUNDINGS: f64 = 64.0 * f64::EPSILON;

/// `a` and `b` agree to a few roundings of `size` (and the change over
/// the gap `gap` at the rate `rate`).
fn near(a: f64, b: f64, size: f64, gap: f64, rate: f64) -> bool {
    (a - b).abs() <= ROUNDINGS * size + 4.0 * gap * rate
}

#[test]
fn one_dimensional_tables_are_c1_and_monotone_on_every_interval() {
    let mut r = Rng(0x7ab1e);
    let mut checked = (0, 0, 0);
    for case in 0..2000 {
        let n = 2 + r.below(12);
        let x = axis(&mut r, n);
        let f = values(&mut r, n);
        let mut data = TableData::new_1d(x.clone(), f.clone());
        let cubic = case % 4 != 0;
        if !cubic {
            data.interpolation = Interpolation::Linear;
        }
        let outside = [Outside::Clamp, Outside::Linear, Outside::Error][case % 3];
        data.outside = [outside, outside];
        let t = Table::new(&data).expect("a table");
        let s = scale_1d(&x, &f);
        let at = |x: f64| t.eval([x, 0.0]);
        let what = |i: usize| format!("case {case}, {data:?}, at breakpoint {i}");
        // the data, and continuity across every breakpoint
        for i in 0..n {
            let (v, d) = at(x[i]);
            assert!(near(v, f[i], s.v, 0.0, 0.0), "{}: {v} is not the data", what(i));
            let below = x[i].next_down();
            let gap = x[i] - below;
            let (vl, dl) = at(below);
            if i > 0 || outside != Outside::Clamp {
                assert!(near(vl, v, s.v, gap, 4.0 * s.d), "{}: value {vl} vs {v}", what(i));
            }
            let inner = i > 0 && i < n - 1;
            if cubic && (inner || outside != Outside::Clamp) {
                // the rates' own rounding: terms of the size of the
                // steepest secant, times a few
                let ok = near(dl[0], d[0], 8.0 * s.d, gap, s.dd);
                assert!(ok, "{}: rate {} vs {} (not C¹)", what(i), dl[0], d[0]);
                checked.0 += 1;
            }
            if i == n - 1 && outside != Outside::Clamp && cubic {
                let (va, da) = at(x[i].next_up());
                assert!(
                    near(va, v, s.v, gap, 4.0 * s.d) && near(da[0], d[0], 8.0 * s.d, gap, s.dd)
                );
            }
        }
        // every interval: monotone between its data, rates of their sign
        for i in 0..n - 1 {
            let dir = sign(f[i + 1] - f[i]);
            let (lo, hi) = (f[i].min(f[i + 1]), f[i].max(f[i + 1]));
            let mut prev = f[i];
            for k in 0..=64 {
                let xk =
                    if k == 64 { x[i + 1] } else { x[i] + (x[i + 1] - x[i]) * k as f64 / 64.0 };
                let (v, d) = at(xk);
                let w = format!("{} (interval {i}, x = {xk})", what(i));
                assert!(
                    v >= lo - ROUNDINGS * s.v && v <= hi + ROUNDINGS * s.v,
                    "{w}: {v} overshoots"
                );
                assert!((v - prev) * dir >= -ROUNDINGS * s.v, "{w}: {prev} then {v}");
                if dir == 0.0 {
                    assert!((v - f[i]).abs() <= ROUNDINGS * s.v, "{w}: flat data, {v}");
                }
                // (a linear table's rate at a breakpoint may be either
                // interval's)
                if cubic || (k > 0 && k < 64) {
                    let ok = d[0] * dir >= -ROUNDINGS * 8.0 * s.d;
                    assert!(ok, "{w}: rate {} against the data", d[0]);
                }
                prev = v;
                checked.1 += 1;
            }
        }
        // outside: clamped flat, else along the edge's rate
        let (v_hi, d_hi) = at(x[n - 1]);
        let far = x[n - 1] + 3.0 * (x[n - 1] - x[0]);
        let (v, d) = at(far);
        match outside {
            Outside::Clamp => assert!(v == v_hi && d[0] == 0.0, "case {case}: clamped"),
            _ => {
                let want = v_hi + d_hi[0] * (far - x[n - 1]);
                assert!(
                    near(v, want, s.v + (want - v_hi).abs(), 0.0, 0.0),
                    "case {case}: {v} {want}"
                );
                assert!(d[0] == d_hi[0], "case {case}: rate outside {} {}", d[0], d_hi[0]);
            }
        }
        assert!(at(f64::NAN).0.is_nan());
        checked.2 += 1;
    }
    println!("{} tables, {} breakpoints C¹, {} monotone samples", checked.2, checked.0, checked.1);
    assert!(checked.0 > 5000 && checked.1 > 500_000);
}

#[test]
fn two_dimensional_tables_are_c1_and_monotone_where_their_data_are() {
    let mut r = Rng(0x2d7ab1e);
    let mut checked = (0, 0, 0);
    for case in 0..600 {
        let (nx, ny) = (2 + r.below(7), 2 + r.below(7));
        let (x, y) = (axis(&mut r, nx), axis(&mut r, ny));
        // values row by row (`f[i * ny + j]` at `(x_i, y_j)`): monotone
        // along one axis, both, or neither
        let along_x = values(&mut r, nx);
        let along_y = values(&mut r, ny);
        let mix = r.below(4);
        let noise = values(&mut r, nx * ny);
        let f: Vec<f64> = (0..nx * ny)
            .map(|k| {
                let (i, j) = (k / ny, k % ny);
                match mix {
                    0 => along_x[i] + along_y[j],
                    1 => along_x[i] * (1.0 + 0.1 * j as f64),
                    2 => noise[k],
                    _ => along_x[i] + along_y[j] + 0.01 * noise[k],
                }
            })
            .collect();
        let mut data = TableData::new_2d(x.clone(), y.clone(), f.clone());
        let cubic = case % 4 != 0;
        if !cubic {
            data.interpolation = Interpolation::Linear;
        }
        let outside = [Outside::Clamp, Outside::Linear, Outside::Error][case % 3];
        data.outside = [outside, outside];
        let t = Table::new(&data).expect("a table");
        let fv = f.iter().fold(0.0f64, |m, a| m.max(a.abs())).max(f64::MIN_POSITIVE);
        let hmin =
            x.windows(2).chain(y.windows(2)).map(|w| w[1] - w[0]).fold(f64::INFINITY, f64::min);
        let fd = 2.0 * fv / hmin;
        let at = |a: f64, b: f64| t.eval([a, b]);
        let what = format!("case {case}, {data:?}");
        // the data
        for i in 0..nx {
            for j in 0..ny {
                let v = at(x[i], y[j]).0;
                assert!(near(v, f[i * ny + j], fv, 0.0, 0.0), "{what}: ({i}, {j}): {v}");
            }
        }
        // continuity of the value and both rates across every inner grid
        // line (and the edges when outside continues the data), at lines
        // across the other axis
        let cross = |r: &mut Rng, p: &[f64]| -> Vec<f64> {
            let mut v: Vec<f64> = (0..6).map(|_| r.range(p[0], p[p.len() - 1])).collect();
            v.extend_from_slice(p);
            v
        };
        let edges_too = outside != Outside::Clamp;
        for (ax, (pts, other)) in [(&x, &y), (&y, &x)].into_iter().enumerate() {
            let lines = cross(&mut r, other);
            for (i, &p) in pts.iter().enumerate() {
                let inner = i > 0 && i < pts.len() - 1;
                if !(inner || edges_too) || !cubic {
                    continue;
                }
                for &q in &lines {
                    let pt = |p: f64| if ax == 0 { at(p, q) } else { at(q, p) };
                    let (v, d) = pt(p);
                    let (vl, dl) = pt(p.next_down());
                    let gap = p - p.next_down();
                    let w = format!("{what}: across axis {ax} line {i} at {q}");
                    assert!(near(vl, v, fv, gap, 4.0 * fd), "{w}: value {vl} vs {v}");
                    for k in 0..2 {
                        assert!(
                            near(dl[k], d[k], 16.0 * fd, gap, 16.0 * fd / hmin),
                            "{w}: rate {k}: {} vs {} (not C¹)",
                            dl[k],
                            d[k]
                        );
                    }
                    checked.0 += 1;
                }
            }
        }
        // monotone along an axis in each cell whose data are monotone
        // that way: along lines through the cell, within the cell's data
        for i in 0..nx - 1 {
            for j in 0..ny - 1 {
                let c = |a: usize, b: usize| f[(i + a) * ny + (j + b)];
                let cell = [c(0, 0), c(1, 0), c(0, 1), c(1, 1)];
                let (lo, hi) = cell
                    .iter()
                    .fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), v| (l.min(*v), h.max(*v)));
                // the directions along each axis (None: not monotone)
                let dirs: Vec<Option<f64>> = (0..2)
                    .map(|ax| {
                        let (s0, s1) = if ax == 0 {
                            (sign(c(1, 0) - c(0, 0)), sign(c(1, 1) - c(0, 1)))
                        } else {
                            (sign(c(0, 1) - c(0, 0)), sign(c(1, 1) - c(1, 0)))
                        };
                        (s0 * s1 >= 0.0).then(|| sign(s0 + s1))
                    })
                    .collect();
                for ax in 0..2 {
                    let Some(dir) = dirs[ax] else { continue };
                    for l in 0..=8 {
                        let u = l as f64 / 8.0;
                        let mut prev = f64::NAN;
                        for k in 0..=32 {
                            let s = k as f64 / 32.0;
                            let (sx, sy) = if ax == 0 { (s, u) } else { (u, s) };
                            let px =
                                if sx == 1.0 { x[i + 1] } else { x[i] + (x[i + 1] - x[i]) * sx };
                            let py =
                                if sy == 1.0 { y[j + 1] } else { y[j] + (y[j + 1] - y[j]) * sy };
                            let (v, d) = at(px, py);
                            let w =
                                format!("{what}: cell ({i}, {j}) along axis {ax} at ({px}, {py})");
                            if dirs.iter().all(|d| d.is_some()) {
                                // monotone both ways: within the cell's data
                                assert!(
                                    v >= lo - ROUNDINGS * fv && v <= hi + ROUNDINGS * fv,
                                    "{w}: {v} outside the cell's data [{lo}, {hi}]"
                                );
                            }
                            if !prev.is_nan() {
                                assert!(
                                    (v - prev) * dir >= -ROUNDINGS * fv,
                                    "{w}: {prev} then {v}"
                                );
                            }
                            // (a bilinear table's rate on a cell's edge
                            // may be either cell's)
                            if cubic || (k > 0 && k < 32) {
                                let ok = d[ax] * dir >= -ROUNDINGS * 16.0 * fd;
                                assert!(ok, "{w}: rate {}", d[ax]);
                            }
                            prev = v;
                            checked.1 += 1;
                        }
                    }
                }
            }
        }
        assert!(at(f64::NAN, y[0]).0.is_nan() && at(x[0], f64::NAN).0.is_nan());
        checked.2 += 1;
    }
    println!("{} tables, {} C¹ crossings, {} monotone samples", checked.2, checked.0, checked.1);
    assert!(checked.0 > 20_000 && checked.1 > 500_000, "{checked:?}");
}
