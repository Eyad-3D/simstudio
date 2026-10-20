//! Input events (FMI 3.0: an input with a discontinuity): where a trace's
//! slope jumps, or a sampled block's tick changes its outputs, a condition
//! that reads them jumps between two steps. Fast mode fires the `when`
//! clauses whose crossings changed sign there, sets the modes, and checks
//! every mode against its crossing after each step. Exact answers on
//! hand-written models (test doubles of the compiled inverse model), and
//! the metamorphic relation that samples on a trace's lines change nothing.

use lsim_fast::{BoundBlock, FastOptions, FastProblem, FastResult, ModeInfo, Trace, WhenInfo, run};
use lsim_ir::prepared::Direction;
use lsim_ir::runtime::{DiscreteBlock, EvalInput, Layout, ModelFunctions};
use proptest::prelude::*;

type Fun = Box<dyn Fn(&EvalInput<'_>, &mut [f64]) + Send + Sync>;

/// A model of states with modes, given by closures: `x' = f(t, x, d, u)`
/// (no iteration variables), its roots, its channels, and its modes, each
/// held by the two `when` clauses preparation gives a mode (rising on its
/// crossing `2k` sets it, falling on the copy `2k + 1` clears it; DESIGN.md
/// 5.6) and set from the sign of its crossing by `modes`.
struct Moded {
    layout: Layout,
    f: Fun,
    roots: Fun,
    vars: Fun,
    x0: Vec<f64>,
    d0: Vec<f64>,
    /// for each mode, its crossing (the one at `2k`)
    modes: usize,
}

impl Moded {
    #[allow(clippy::too_many_arguments)]
    fn new(
        n_x: usize,
        n_u: usize,
        modes: usize,
        n_vars: usize,
        f: Fun,
        roots: Fun,
        vars: Fun,
    ) -> Self {
        Moded {
            layout: Layout {
                n_x,
                n_u,
                n_d: modes,
                n_roots: 2 * modes,
                n_whens: 2 * modes,
                n_vars,
                n_work: 1,
                ..Default::default()
            },
            f,
            roots,
            vars,
            x0: vec![0.0; n_x],
            d0: vec![0.0; modes],
            modes,
        }
    }

    fn whens(&self) -> Vec<WhenInfo> {
        (0..self.modes)
            .flat_map(|k| {
                [
                    WhenInfo {
                        crossing: 2 * k,
                        direction: Direction::Rising,
                        label: format!("on {k}"),
                    },
                    WhenInfo {
                        crossing: 2 * k + 1,
                        direction: Direction::Falling,
                        label: format!("off {k}"),
                    },
                ]
            })
            .collect()
    }

    fn mode_infos(&self) -> Vec<ModeInfo> {
        (0..self.modes)
            .map(|k| ModeInfo { crossing: 2 * k, discrete: k, label: format!("mode {k}") })
            .collect()
    }
}

impl ModelFunctions for Moded {
    fn layout(&self) -> &Layout {
        &self.layout
    }
    fn residual(&self, inp: &EvalInput<'_>, _w: &mut [f64], out: &mut [f64]) {
        (self.f)(inp, out)
    }
    fn jvp(&self, inp: &EvalInput<'_>, v: &[f64], _w: &mut [f64], out: &mut [f64]) {
        // x' depends on x linearly in these models: by differences
        let n = inp.y.len();
        let mut yp = inp.y.to_vec();
        for i in 0..n {
            yp[i] += v[i];
        }
        let mut f0 = vec![0.0; n];
        let mut f1 = vec![0.0; n];
        (self.f)(inp, &mut f0);
        (self.f)(&EvalInput { y: &yp, ..*inp }, &mut f1);
        for i in 0..n {
            out[i] = f1[i] - f0[i];
        }
    }
    fn roots(&self, inp: &EvalInput<'_>, _w: &mut [f64], out: &mut [f64]) {
        (self.roots)(inp, out)
    }
    fn vars(&self, inp: &EvalInput<'_>, _w: &mut [f64], out: &mut [f64]) {
        (self.vars)(inp, out)
    }
    fn when(&self, _inp: &EvalInput<'_>, fired: &[f64], _w: &mut [f64], d_out: &mut [f64]) {
        for k in 0..self.modes {
            if fired[2 * k] != 0.0 {
                d_out[k] = 1.0;
            }
            if fired[2 * k + 1] != 0.0 {
                d_out[k] = 0.0;
            }
        }
    }
    fn modes(&self, inp: &EvalInput<'_>, _w: &mut [f64], d_out: &mut [f64]) {
        let mut r = vec![0.0; 2 * self.modes];
        (self.roots)(inp, &mut r);
        for k in 0..self.modes {
            d_out[k] = if r[2 * k] > 0.0 { 1.0 } else { 0.0 };
        }
    }
    fn start(&self, _p: &[f64], y0: &mut [f64], d0: &mut [f64]) {
        y0.copy_from_slice(&self.x0);
        d0.copy_from_slice(&self.d0);
    }
}

fn fast(m: &Moded, traces: &[Trace], o: &FastOptions, blocks: &mut [BoundBlock]) -> FastResult {
    let whens = m.whens();
    let modes = m.mode_infos();
    let mut p = FastProblem::new(m, &[], traces, o);
    p.whens = &whens;
    p.modes = &modes;
    run(&p, blocks).unwrap_or_else(|e| panic!("{e}"))
}

/// The reviewer's probe: x' = d with the mode d = [der(v) > 0], the speed
/// 0 → 1 → 0 m/s at 0, 2, 4 s. The acceleration jumps from +0.5 to -0.5
/// m/s² at the sample at 2 s, between two steps: the mode must clear there,
/// so x(4) = 2 s (it stayed set, and x(4) was 4, before).
#[test]
fn a_mode_on_the_acceleration_clears_where_the_trace_bends() {
    let m = Moded::new(
        1,
        2,
        1,
        3,
        Box::new(|i, out| out[0] = i.d[0]),
        Box::new(|i, out| {
            out[0] = i.u[1];
            out[1] = i.u[1];
        }),
        Box::new(|i, out| {
            out[0] = i.y[0];
            out[1] = i.d[0];
            out[2] = i.u[1];
        }),
    );
    let traces = [Trace::new(vec![0.0, 2.0, 4.0], vec![0.0, 1.0, 0.0]).unwrap()];
    let r = fast(&m, &traces, &FastOptions::default(), &mut []);
    let (x, d) = (r.channel("v[0]").unwrap(), r.channel("v[1]").unwrap());
    assert_eq!(d, [1.0, 1.0, 1.0, 0.0, 0.0], "the mode at 0 … 4 s (left limits)");
    assert!((x[4] - 2.0).abs() < 1e-12, "x(4) = {}", x[4]);
    assert_eq!(r.events.len(), 1, "{:?}", r.events);
    assert_eq!((r.events[0].t, r.events[0].label.as_str()), (2.0, "off 0"));
}

/// A mode whose crossing is exactly 0 where the trace bends and leaves 0
/// within the next step (d = [v > 0], a car that stands and drives off at
/// 2 s): no crossing test sees a sign change from 0, so the check after
/// the step sets the mode at its start and redoes the step: x(4) = 2.
#[test]
fn a_mode_leaving_zero_at_a_bend_is_set_where_it_leaves() {
    let m = Moded::new(
        1,
        2,
        1,
        2,
        Box::new(|i, out| out[0] = i.d[0]),
        Box::new(|i, out| {
            out[0] = i.u[0];
            out[1] = i.u[0];
        }),
        Box::new(|i, out| {
            out[0] = i.y[0];
            out[1] = i.d[0];
        }),
    );
    let traces = [Trace::new(vec![0.0, 2.0, 4.0], vec![0.0, 0.0, 2.0]).unwrap()];
    let r = fast(&m, &traces, &FastOptions::default(), &mut []);
    let x = r.channel("v[0]").unwrap();
    assert!((x[4] - 2.0).abs() < 1e-12, "x(4) = {}", x[4]);
    assert_eq!(r.events.iter().map(|e| (e.t, e.when)).collect::<Vec<_>>(), [(2.0, 0)]);
}

/// A `when` clause on the acceleration fires at every sample where the
/// trace turns from rising to falling, at the sample: a count of the
/// decelerations of 0 → 1 → 0 → 1 → 0 m/s is 2, at 1 s and 3 s.
#[test]
fn a_when_on_the_acceleration_fires_at_the_samples() {
    // the mode stands for any `when`: it counts its rising edges in x
    let m = Moded::new(
        1,
        2,
        1,
        2,
        Box::new(|_, out| out[0] = 0.0),
        Box::new(|i, out| {
            out[0] = -i.u[1];
            out[1] = -i.u[1];
        }),
        Box::new(|i, out| {
            out[0] = i.d[0];
            out[1] = i.u[1];
        }),
    );
    let traces =
        [Trace::new(vec![0.0, 1.0, 2.0, 3.0, 4.0], vec![0.0, 1.0, 0.0, 1.0, 0.0]).unwrap()];
    let r = fast(&m, &traces, &FastOptions::default(), &mut []);
    let on: Vec<f64> = r.events.iter().filter(|e| e.when == 0).map(|e| e.t).collect();
    let off: Vec<f64> = r.events.iter().filter(|e| e.when == 1).map(|e| e.t).collect();
    assert_eq!(on, [1.0, 3.0], "{:?}", r.events);
    assert_eq!(off, [2.0], "{:?}", r.events);
}

/// Switches its output from 0 to 1 at its tick at or after `at`.
struct Switch {
    at: f64,
}

impl DiscreteBlock for Switch {
    fn name(&self) -> &str {
        "switch"
    }
    fn period(&self) -> f64 {
        0.5
    }
    fn init(&mut self, _t0: f64, _inputs: &[f64], outputs: &mut [f64]) -> Result<(), String> {
        outputs[0] = 0.0;
        Ok(())
    }
    fn tick(&mut self, t: f64, _inputs: &[f64], outputs: &mut [f64]) -> Result<(), String> {
        outputs[0] = if t >= self.at - 1e-12 { 1.0 } else { 0.0 };
        Ok(())
    }
}

/// A block's tick that changes its output is an event: the mode
/// d = [o > 0.5] on the block's output o, which switches at its tick at
/// 1.5 s, sets there, so x' = d gives x(3) = 1.5 (it stayed clear before).
#[test]
fn a_ticks_new_output_is_an_event() {
    // d[0] the mode, d[1] the block's output
    let mut m = Moded::new(
        1,
        2,
        1,
        2,
        Box::new(|i, out| out[0] = i.d[0]),
        Box::new(|i, out| {
            out[0] = i.d[1] - 0.5;
            out[1] = i.d[1] - 0.5;
        }),
        Box::new(|i, out| {
            out[0] = i.y[0];
            out[1] = i.d[1];
        }),
    );
    m.layout.n_d = 2;
    m.d0 = vec![0.0, 0.0];
    let traces = [Trace::new(vec![0.0, 3.0], vec![0.0, 0.0]).unwrap()];
    let o = FastOptions { step: 0.5, ..Default::default() };
    let mut blocks =
        [BoundBlock { block: Box::new(Switch { at: 1.5 }), inputs: vec![], outputs: vec![1] }];
    let r = fast(&m, &traces, &o, &mut blocks);
    let x = r.channel("v[0]").unwrap();
    assert!((x[x.len() - 1] - 1.5).abs() < 1e-12, "x(3) = {}", x[x.len() - 1]);
    assert_eq!(r.events.iter().map(|e| (e.t, e.when)).collect::<Vec<_>>(), [(1.5, 0)]);
}

/// The model of the metamorphic test: a mode on the acceleration (x0
/// counts the time it holds), a mode on the speed above 1 m/s (crossed
/// inside steps: located events), and the distance driven at double rate
/// above it.
fn two_modes() -> Moded {
    Moded::new(
        2,
        2,
        2,
        4,
        Box::new(|i, out| {
            out[0] = i.d[0];
            out[1] = i.u[0] * (1.0 + i.d[1]);
        }),
        Box::new(|i, out| {
            out[0] = i.u[1];
            out[1] = i.u[1];
            out[2] = i.u[0] - 1.0;
            out[3] = i.u[0] - 1.0;
        }),
        Box::new(|i, out| {
            out[0] = i.y[0];
            out[1] = i.y[1];
            out[2] = i.d[0];
            out[3] = i.d[1];
        }),
    )
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, ..ProptestConfig::default() })]

    /// Samples added on a trace's lines (each at a point of a segment, its
    /// value interpolated) are not breakpoints: the run is the same, every
    /// value, extreme, mean and event, to round-off.
    #[test]
    fn samples_on_the_lines_change_nothing(
        v0 in 0.0f64..3.0,
        segs in prop::collection::vec((0.3f64..2.5, -1.5f64..1.5, any::<bool>()), 2..8),
        cuts in prop::collection::vec((0usize..8, 0.05f64..0.95), 1..12),
    ) {
        let mut t = vec![0.0];
        let mut v = vec![v0];
        for &(dt, slope, flat) in &segs {
            let s = if flat { 0.0 } else { slope };
            let last = *v.last().unwrap();
            t.push(t[t.len() - 1] + dt);
            v.push(last + s * dt);
        }
        let mut t2 = t.clone();
        let mut v2 = v.clone();
        for &(k, f) in &cuts {
            let k = k % (t.len() - 1);
            let (ta, tb, va, vb) = (t[k], t[k + 1], v[k], v[k + 1]);
            t2.push(ta + (tb - ta) * f);
            v2.push(va + (vb - va) * f);
        }
        let mut pts: Vec<(f64, f64)> = t2.into_iter().zip(v2).collect();
        pts.sort_by(|a, b| a.0.total_cmp(&b.0));
        pts.dedup_by(|b, a| b.0 == a.0);
        let (t2, v2): (Vec<f64>, Vec<f64>) = pts.into_iter().unzip();
        let m = two_modes();
        let o = FastOptions::default();
        let a = fast(&m, &[Trace::new(t, v).unwrap()], &o, &mut []);
        let b = fast(&m, &[Trace::new(t2, v2).unwrap()], &o, &mut []);
        prop_assert_eq!(&a.times, &b.times);
        for (x, y) in [(&a.values, &b.values), (&a.min, &b.min), (&a.max, &b.max), (&a.mean, &b.mean)] {
            for (p, q) in x.iter().zip(y.iter()) {
                prop_assert!((p - q).abs() <= 1e-12 * (1.0 + p.abs()), "{} against {}", p, q);
            }
        }
        let ev = |r: &FastResult| r.events.iter().map(|e| (e.when, e.t)).collect::<Vec<_>>();
        let (ea, eb) = (ev(&a), ev(&b));
        prop_assert_eq!(ea.len(), eb.len(), "{:?} against {:?}", ea, eb);
        for ((wa, ta), (wb, tb)) in ea.iter().zip(&eb) {
            prop_assert_eq!(wa, wb);
            prop_assert!((ta - tb).abs() <= 1e-12 * (1.0 + ta.abs()));
        }
    }
}
