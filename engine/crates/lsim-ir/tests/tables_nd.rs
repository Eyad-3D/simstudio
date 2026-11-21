//! Tables of three and four axes against their definition, computed here
//! and not by the engine's routine (docs/spec/tables.md): linear tables
//! (the app's, a Map block's) are the weighted sum of their cell's corners,
//! straight lines along each axis, past an axis's data held at its edge
//! (`Clamp`) or continued along its edge cell (`Linear`, and `Error`, which
//! the run loop watches); monotone cubic ones reproduce a sum of quadratics
//! along the axes, are C¹ across their cells, and monotone where their data
//! are. Inside the grid, on its breakpoints, on each edge, past each edge
//! under each rule, with an axis of one breakpoint, and each partial
//! derivative against a central difference.

use lsim_ir::interp::Table;
use lsim_ir::{Interpolation, Outside, TableData};

/// A small deterministic generator (xorshift).
struct Rnd(u64);
impl Rnd {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// Uneven breakpoints: `n` increasing from about `start`.
fn axis(r: &mut Rnd, n: usize, start: f64) -> Vec<f64> {
    let mut x = start;
    (0..n)
        .map(|_| {
            let v = x;
            x += 0.3 + 2.0 * r.next();
            v
        })
        .collect()
}

/// The value of a linear table at `at` by its definition: per axis, the
/// cell and the weight of its upper corner (beyond the data: the edge cell,
/// its weight held to 0 or 1 when clamped, else continued); the corners'
/// values weighted by the products of their axes' weights.
fn multilinear(axes: &[Vec<f64>], rules: &[Outside], values: &[f64], at: &[f64]) -> f64 {
    let n = axes.len();
    let mut cells = vec![];
    for k in 0..n {
        let p = &axes[k];
        if p.len() == 1 {
            cells.push((0usize, 0.0, 0usize));
            continue;
        }
        let x = at[k];
        let i = (0..p.len() - 1).rev().find(|&i| p[i] <= x).unwrap_or(0);
        let mut w = (x - p[i]) / (p[i + 1] - p[i]);
        if rules[k] == Outside::Clamp {
            w = w.clamp(0.0, 1.0);
        }
        cells.push((i, w, 1));
    }
    let stride = |k: usize| axes[k + 1..].iter().map(Vec::len).product::<usize>();
    let mut total = 0.0;
    for corner in 0..1usize << n {
        let (mut weight, mut index) = (1.0, 0);
        for (k, &(i, w, up_ok)) in cells.iter().enumerate() {
            let up = (corner >> k & 1).min(up_ok);
            if corner >> k & 1 == 1 && up_ok == 0 {
                weight = 0.0;
            }
            weight *= if up == 1 { w } else { 1.0 - w };
            index += (i + up) * stride(k);
        }
        if weight != 0.0 {
            total += weight * values[index];
        }
    }
    total
}

/// Every point of the grid of `axes`, row-major.
fn grid_points(axes: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let mut out = vec![vec![]];
    for a in axes {
        out = out
            .into_iter()
            .flat_map(|p: Vec<f64>| a.iter().map(move |&x| [&p[..], &[x]].concat()))
            .collect();
    }
    out
}

/// Points to read at: inside each cell, on every breakpoint, on each edge
/// and past each edge, combined across the axes (a sample of them).
fn probes(axes: &[Vec<f64>], r: &mut Rnd) -> Vec<Vec<f64>> {
    let per_axis: Vec<Vec<f64>> = axes
        .iter()
        .map(|p| {
            let (lo, hi) = (p[0], p[p.len() - 1]);
            let mut v = p.clone();
            v.extend([lo - 1.3, lo - 1e-3, hi + 1e-3, hi + 2.1]);
            for w in p.windows(2) {
                v.push(w[0] + (0.1 + 0.8 * r.next()) * (w[1] - w[0]));
            }
            v
        })
        .collect();
    let mut out = vec![];
    for _ in 0..3000 {
        out.push(
            per_axis.iter().map(|v| v[(r.next() * v.len() as f64) as usize % v.len()]).collect(),
        );
    }
    out
}

fn linear_table(axes: &[Vec<f64>], rules: &[Outside], r: &mut Rnd) -> (TableData, Table) {
    let n: usize = axes.iter().map(Vec::len).product();
    let values: Vec<f64> = (0..n).map(|_| 10.0 * r.next() - 5.0).collect();
    let data = TableData {
        interpolation: Interpolation::Linear,
        ..TableData::new(axes.to_vec(), values).with_outside(rules)
    };
    let t = Table::new(&data).expect("a valid table");
    (data, t)
}

#[test]
fn linear_tables_of_three_and_four_axes_are_their_definition() {
    let mut r = Rnd(0x1234_5678_9abc_def1);
    let rules = [Outside::Clamp, Outside::Linear, Outside::Error];
    let mut checked = 0;
    for dims in [3usize, 4] {
        for case in 0..12 {
            let axes: Vec<Vec<f64>> = (0..dims)
                .map(|k| {
                    let s = -2.0 * r.next();
                    axis(&mut r, 2 + (case + k) % 3, s)
                })
                .collect();
            let rule: Vec<Outside> = (0..dims).map(|k| rules[(case + k) % 3]).collect();
            let (data, t) = linear_table(&axes, &rule, &mut r);
            assert_eq!(t.dims(), dims);
            // on its breakpoints: the values themselves
            for (k, p) in grid_points(&axes).iter().enumerate() {
                assert!(
                    (t.value(p) - data.values[k]).abs() <= 1e-12 * (1.0 + data.values[k].abs())
                );
            }
            for at in probes(&axes, &mut r) {
                let want = multilinear(&axes, &rule, &data.values, &at);
                let (v, g) = t.eval(&at);
                assert!(
                    (v - want).abs() <= 1e-11 * (1.0 + want.abs()),
                    "{dims} axes, {rule:?} at {at:?}: {v} vs {want}"
                );
                // the value alone is the value with its rates, to the bit
                assert_eq!(t.value(&at).to_bits(), v.to_bits());
                // each rate against a central difference away from breakpoints
                for k in 0..dims {
                    let h = 1e-6;
                    let near = axes[k].iter().any(|p| (at[k] - p).abs() < 1e-4);
                    if near {
                        continue;
                    }
                    let (mut a, mut b) = (at.clone(), at.clone());
                    a[k] += h;
                    b[k] -= h;
                    let fd = (multilinear(&axes, &rule, &data.values, &a)
                        - multilinear(&axes, &rule, &data.values, &b))
                        / (2.0 * h);
                    assert!(
                        (g[k] - fd).abs() < 1e-6 * (1.0 + fd.abs()),
                        "{dims} axes, axis {k} at {at:?}: {} vs {fd}",
                        g[k]
                    );
                    // and the derivative read (index reduction's) is the rate
                    let mut o = vec![0u8; dims];
                    o[k] = 1;
                    assert_eq!(t.eval_order(&at, &o).0.to_bits(), g[k].to_bits());
                    checked += 1;
                }
                // outside the data a clamped axis has no rate
                for k in 0..dims {
                    let (lo, hi) = (axes[k][0], axes[k][axes[k].len() - 1]);
                    if rule[k] == Outside::Clamp && (at[k] < lo || at[k] > hi) {
                        assert_eq!(g[k], 0.0);
                    }
                }
            }
        }
    }
    assert!(checked > 50_000, "{checked}");
}

#[test]
fn an_axis_of_one_breakpoint_is_constant_and_never_outside() {
    let mut r = Rnd(0x0dd_ba11);
    let axes = vec![axis(&mut r, 3, 0.0), vec![7.0], axis(&mut r, 4, -1.0)];
    let rule = [Outside::Linear, Outside::Error, Outside::Clamp];
    let (data, t) = linear_table(&axes, &rule, &mut r);
    for at in probes(&axes, &mut r) {
        let want = multilinear(&axes, &rule, &data.values, &at);
        let (v, g) = t.eval(&at);
        assert!((v - want).abs() <= 1e-11 * (1.0 + want.abs()));
        assert_eq!(g[1], 0.0);
        // its guard: inside wherever it is read
        assert!(t.guard(1, at[1] + 1e6) > 0.0 && t.guard(1, -1e9) > 0.0);
    }
    // the same bits as the table without that axis (one model: the axes
    // that vary are interpolated, by the arithmetic of their number)
    let two = TableData {
        interpolation: Interpolation::Linear,
        ..TableData::new(vec![axes[0].clone(), axes[2].clone()], data.values.clone())
            .with_outside(&[rule[0], rule[2]])
    };
    let t2 = Table::new(&two).unwrap();
    for at in probes(&axes, &mut r).into_iter().take(500) {
        let (v3, g3) = t.eval(&at);
        let (v2, g2) = t2.eval(&[at[0], at[2]]);
        assert_eq!(v3.to_bits(), v2.to_bits());
        assert_eq!((g3[0].to_bits(), g3[2].to_bits()), (g2[0].to_bits(), g2[1].to_bits()));
    }
    // a NaN in it still poisons the value
    assert!(t.value(&[axes[0][1], f64::NAN, axes[2][1]]).is_nan());
}

/// A sum of quadratics along the axes, each monotone over the data.
fn quad(k: usize, x: f64) -> (f64, f64) {
    let (a, b) = [(0.5, 0.05), (-0.8, -0.04), (0.3, 0.02), (1.1, 0.03)][k];
    (a * x + b * x * x, a + 2.0 * b * x)
}

#[test]
fn monotone_cubic_tables_of_three_and_four_axes() {
    let mut r = Rnd(0xfeed_beef_1357_9bdf);
    for dims in [3usize, 4] {
        // breakpoints where each quadratic is monotone (a + 2 b x keeps its sign)
        let axes: Vec<Vec<f64>> = (0..dims).map(|_| axis(&mut r, 4, 0.0)).collect();
        let f = |p: &[f64]| (0..dims).map(|k| quad(k, p[k]).0).sum::<f64>();
        let values: Vec<f64> = grid_points(&axes).iter().map(|p| f(p)).collect();
        let rule: Vec<Outside> =
            (0..dims).map(|k| [Outside::Linear, Outside::Error][k % 2]).collect();
        let data = TableData::new(axes.clone(), values.clone()).with_outside(&rule);
        assert_eq!(data.interpolation, Interpolation::MonotoneCubic);
        let t = Table::new(&data).unwrap();
        assert_eq!(t.continuity(), 1);
        for at in probes(&axes, &mut r) {
            let (v, g) = t.eval(&at);
            // inside: the sum itself (Steffen's slopes are a quadratic's own,
            // and its net is monotone: nothing is reduced); outside: its
            // edge's tangent along each axis that left the data
            let mut want = 0.0;
            for k in 0..dims {
                let (lo, hi) = (axes[k][0], axes[k][3]);
                let e = at[k].clamp(lo, hi);
                let (q, dq) = quad(k, e);
                want += q + dq * (at[k] - e);
                let rate = dq;
                assert!(
                    (g[k] - rate).abs() < 1e-9 * (1.0 + rate.abs()),
                    "{dims}: axis {k} at {at:?}"
                );
            }
            assert!(
                (v - want).abs() < 1e-9 * (1.0 + want.abs()),
                "{dims} at {at:?}: {v} vs {want}"
            );
            assert_eq!(t.value(&at).to_bits(), v.to_bits());
        }
        // second derivatives inside: the quadratics' (2 b), the mixed ones 0
        let mid: Vec<f64> = axes.iter().map(|p| 0.5 * (p[1] + p[2])).collect();
        for k in 0..dims {
            let mut o = vec![0u8; dims];
            o[k] = 2;
            let want = 2.0 * [0.05, -0.04, 0.02, 0.03][k];
            assert!((t.eval_order(&mid, &o).0 - want).abs() < 1e-9);
            o[k] = 1;
            o[(k + 1) % dims] = 1;
            assert!(t.eval_order(&mid, &o).0.abs() < 1e-9);
        }
    }
}

#[test]
fn monotone_cubic_tables_are_c1_across_cells_and_monotone_where_their_data_are() {
    let mut r = Rnd(0x5eed_0fc0_debb);
    for dims in [3usize, 4] {
        for case in 0..6 {
            let axes: Vec<Vec<f64>> =
                (0..dims).map(|k| axis(&mut r, 3 + (case + k) % 2, 0.0)).collect();
            let pts = grid_points(&axes);
            // increasing along every axis, steep in places (the slopes need reducing)
            let values: Vec<f64> = pts
                .iter()
                .map(|p| {
                    p.iter()
                        .enumerate()
                        .map(|(k, x)| {
                            let y = x * (1.0 + k as f64);
                            y * y * y * 0.1 + if *x > 2.0 { 5.0 } else { 0.0 }
                        })
                        .sum()
                })
                .collect();
            let data =
                TableData::new(axes.clone(), values).with_outside(&vec![Outside::Linear; dims]);
            let t = Table::new(&data).unwrap();
            for at in probes(&axes, &mut r).into_iter().take(800) {
                let inside: Vec<f64> =
                    at.iter().zip(&axes).map(|(x, p)| x.clamp(p[0], p[p.len() - 1])).collect();
                for k in 0..dims {
                    // monotone along k: a step up along it never lowers the value
                    let mut up = inside.clone();
                    up[k] += 0.05;
                    if up[k] <= axes[k][axes[k].len() - 1] {
                        assert!(
                            t.value(&up) >= t.value(&inside) - 1e-12,
                            "{dims}: axis {k} at {inside:?}"
                        );
                    }
                    // C¹ across a breakpoint inside: the rates either side agree
                    let b = axes[k][1];
                    let (mut lo, mut hi) = (inside.clone(), inside.clone());
                    lo[k] = b - 1e-9;
                    hi[k] = b + 1e-9;
                    let (_, gl) = t.eval(&lo);
                    let (_, gh) = t.eval(&hi);
                    for j in 0..dims {
                        assert!(
                            (gl[j] - gh[j]).abs() < 1e-6 * (1.0 + gl[j].abs()),
                            "{dims}: across axis {k}, rate {j}"
                        );
                    }
                }
            }
        }
    }
}
