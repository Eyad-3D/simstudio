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
                        strict: false,
                        label: format!("on {k}"),
                    },
                    WhenInfo {
                        crossing: 2 * k + 1,
                        direction: Direction::Falling,
                        strict: false,
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

/// A mode set where its crossing leaves zero changes, at that instant, the
/// crossing of a mode on it, which must follow there (event mode: the
/// modes settle and iterate): d0 = [v > 0], d1 = [d0 − 0.5 > 0], x' = d1,
/// the car standing until 2 s and then driving off, so d0 and d1 hold from
/// 2 s and x(4) = 2 (OpenModelica: 1.9999999996; d1 followed one step
/// late, and x(4) was 1, before).
#[test]
fn a_mode_set_where_its_crossing_leaves_zero_settles_the_modes_on_it() {
    let m = Moded::new(
        1,
        2,
        2,
        3,
        Box::new(|i, out| out[0] = i.d[1]),
        Box::new(|i, out| {
            out[0] = i.u[0];
            out[1] = i.u[0];
            out[2] = i.d[0] - 0.5;
            out[3] = i.d[0] - 0.5;
        }),
        Box::new(|i, out| {
            out[0] = i.y[0];
            out[1] = i.d[0];
            out[2] = i.d[1];
        }),
    );
    let traces = [Trace::new(vec![0.0, 2.0, 4.0], vec![0.0, 0.0, 2.0]).unwrap()];
    let r = fast(&m, &traces, &FastOptions::default(), &mut []);
    let (x, d1) = (r.channel("v[0]").unwrap(), r.channel("v[2]").unwrap());
    assert!((x[4] - 2.0).abs() < 1e-12, "x(4) = {}", x[4]);
    assert_eq!(d1, [0.0, 0.0, 0.0, 1.0, 1.0], "d1 at 0 … 4 s (left limits)");
    assert_eq!(r.events.iter().map(|e| (e.t, e.when)).collect::<Vec<_>>(), [(2.0, 0), (2.0, 2)]);
}

/// `when der(v) > 0 then n = pre(n) + 1`, not a mode: a counter of a
/// trace's starts from rest. Its condition is strict: false where der(v)
/// is 0, so it fires where der(v) leaves 0 upwards (the starts at 2 s and
/// 8 s) and not where it reaches 0 from below (the stop at 6 s), as in
/// Modelica and the forward run loop (OpenModelica: n = 2, at 2 and 8 s;
/// fast mode counted 1, at 6 s, before).
#[test]
fn a_strict_when_fires_where_its_crossing_leaves_zero() {
    struct Counter(Layout);
    impl ModelFunctions for Counter {
        fn layout(&self) -> &Layout {
            &self.0
        }
        fn residual(&self, _inp: &EvalInput<'_>, _w: &mut [f64], out: &mut [f64]) {
            out[0] = 0.0;
        }
        fn jvp(&self, _inp: &EvalInput<'_>, _v: &[f64], _w: &mut [f64], out: &mut [f64]) {
            out[0] = 0.0;
        }
        fn roots(&self, inp: &EvalInput<'_>, _w: &mut [f64], out: &mut [f64]) {
            out[0] = inp.u[1];
        }
        fn vars(&self, inp: &EvalInput<'_>, _w: &mut [f64], out: &mut [f64]) {
            out[0] = inp.d[0];
        }
        fn when(&self, inp: &EvalInput<'_>, fired: &[f64], _w: &mut [f64], d_out: &mut [f64]) {
            if fired[0] != 0.0 {
                d_out[0] = inp.d[0] + 1.0;
            }
        }
        fn start(&self, _p: &[f64], y0: &mut [f64], d0: &mut [f64]) {
            y0[0] = 0.0;
            d0[0] = 0.0;
        }
    }
    let m = Counter(Layout {
        n_x: 1,
        n_u: 2,
        n_d: 1,
        n_roots: 1,
        n_whens: 1,
        n_vars: 1,
        n_work: 1,
        ..Default::default()
    });
    let whens = [WhenInfo {
        crossing: 0,
        direction: Direction::Rising,
        strict: true,
        label: "starts".into(),
    }];
    let traces = [Trace::new(
        vec![0.0, 2.0, 4.0, 6.0, 8.0, 10.0, 12.0],
        vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0],
    )
    .unwrap()];
    let o = FastOptions::default();
    let mut p = FastProblem::new(&m, &[], &traces, &o);
    p.whens = &whens;
    let r = run(&p, &mut []).unwrap();
    assert_eq!(r.events.iter().map(|e| e.t).collect::<Vec<_>>(), [2.0, 8.0]);
    assert_eq!(*r.channel("v[0]").unwrap().last().unwrap(), 2.0);
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

/// The model of the starts test: the mode d0 = [v > 0], the mode on it
/// d1 = [d0 > 0.5], x0' = d0, x1' = d1, and `when der(v) > 0 then n =
/// pre(n) + 1` (strict, not a mode) counting the trace's starts; channels
/// x0, x1, n.
struct Starts(Layout);

impl Starts {
    fn new() -> Self {
        Starts(Layout {
            n_x: 2,
            n_u: 2,
            n_d: 3,
            n_roots: 5,
            n_whens: 5,
            n_vars: 3,
            n_work: 1,
            ..Default::default()
        })
    }
}

impl ModelFunctions for Starts {
    fn layout(&self) -> &Layout {
        &self.0
    }
    fn residual(&self, inp: &EvalInput<'_>, _w: &mut [f64], out: &mut [f64]) {
        out[0] = inp.d[0];
        out[1] = inp.d[1];
    }
    fn jvp(&self, _inp: &EvalInput<'_>, _v: &[f64], _w: &mut [f64], out: &mut [f64]) {
        out.iter_mut().for_each(|o| *o = 0.0);
    }
    fn roots(&self, inp: &EvalInput<'_>, _w: &mut [f64], out: &mut [f64]) {
        out[0] = inp.u[0];
        out[1] = inp.u[0];
        out[2] = inp.d[0] - 0.5;
        out[3] = inp.d[0] - 0.5;
        out[4] = inp.u[1];
    }
    fn vars(&self, inp: &EvalInput<'_>, _w: &mut [f64], out: &mut [f64]) {
        out[0] = inp.y[0];
        out[1] = inp.y[1];
        out[2] = inp.d[2];
    }
    fn when(&self, inp: &EvalInput<'_>, fired: &[f64], _w: &mut [f64], d_out: &mut [f64]) {
        for (k, (i, v)) in [(0, 1.0), (0, 0.0), (1, 1.0), (1, 0.0)].into_iter().enumerate() {
            if fired[k] != 0.0 {
                d_out[i] = v;
            }
        }
        if fired[4] != 0.0 {
            d_out[2] = inp.d[2] + 1.0;
        }
    }
    fn modes(&self, inp: &EvalInput<'_>, _w: &mut [f64], d_out: &mut [f64]) {
        d_out[0] = if inp.u[0] > 0.0 { 1.0 } else { 0.0 };
        d_out[1] = if inp.d[0] - 0.5 > 0.0 { 1.0 } else { 0.0 };
    }
    fn start(&self, _p: &[f64], y0: &mut [f64], d0: &mut [f64]) {
        y0.iter_mut().for_each(|v| *v = 0.0);
        d0.iter_mut().for_each(|v| *v = 0.0);
    }
}

fn starts(traces: &[Trace]) -> FastResult {
    let m = Starts::new();
    let w = |crossing, direction, strict, label: &str| WhenInfo {
        crossing,
        direction,
        strict,
        label: label.into(),
    };
    let whens = [
        w(0, Direction::Rising, false, "on 0"),
        w(1, Direction::Falling, false, "off 0"),
        w(2, Direction::Rising, false, "on 1"),
        w(3, Direction::Falling, false, "off 1"),
        w(4, Direction::Rising, true, "starts"),
    ];
    let modes = [
        ModeInfo { crossing: 0, discrete: 0, label: "moving".into() },
        ModeInfo { crossing: 2, discrete: 1, label: "moving, again".into() },
    ];
    let o = FastOptions::default();
    let mut p = FastProblem::new(&m, &[], traces, &o);
    p.whens = &whens;
    p.modes = &modes;
    run(&p, &mut []).unwrap_or_else(|e| panic!("{e}"))
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, ..ProptestConfig::default() })]

    /// A trace that stands at exactly 0 m/s, starts, stops and starts
    /// again, at random: the time it moves, counted by the mode and by the
    /// mode on that mode, is exactly the time of its pieces that are not
    /// both ends at rest, at every output; the strict `when` counts each
    /// sample where its slope turns from not rising to rising; and samples
    /// added on its lines change none of it. (The mode on the mode lagged
    /// a step behind at every start from rest, and the count saw the stops
    /// instead of the starts, before.)
    #[test]
    fn standing_starting_and_a_mode_on_a_mode_are_exact(
        knots in prop::collection::vec((0.3f64..2.5, prop_oneof![Just(0.0), 0.2f64..3.0]), 2..10),
        v0 in prop_oneof![Just(0.0), 0.2f64..3.0],
        cuts in prop::collection::vec((0usize..10, 0.05f64..0.95), 0..8),
    ) {
        let mut t = vec![0.0];
        let mut v = vec![v0];
        for &(dt, vk) in &knots {
            t.push(t[t.len() - 1] + dt);
            v.push(vk);
        }
        // the exact answers: moving time up to each instant, and the starts
        let moving = |at: f64| -> f64 {
            (0..t.len() - 1)
                .filter(|&k| !(v[k] == 0.0 && v[k + 1] == 0.0))
                .map(|k| (at.min(t[k + 1]) - t[k]).max(0.0))
                .sum()
        };
        let slope = |k: usize| (v[k + 1] - v[k]) / (t[k + 1] - t[k]);
        let n_starts = (1..t.len() - 1).filter(|&k| slope(k - 1) <= 0.0 && slope(k) > 0.0).count();
        let mut t2 = t.clone();
        let mut v2 = v.clone();
        for &(k, f) in &cuts {
            let k = k % (t.len() - 1);
            t2.push(t[k] + (t[k + 1] - t[k]) * f);
            v2.push(v[k] + (v[k + 1] - v[k]) * f);
        }
        let mut pts: Vec<(f64, f64)> = t2.into_iter().zip(v2).collect();
        pts.sort_by(|a, b| a.0.total_cmp(&b.0));
        pts.dedup_by(|b, a| b.0 == a.0);
        let (t2, v2): (Vec<f64>, Vec<f64>) = pts.into_iter().unzip();
        for (tt, vv) in [(t.clone(), v.clone()), (t2, v2)] {
            let r = starts(&[Trace::new(tt, vv).unwrap()]);
            let (x0, x1, n) =
                (r.channel("v[0]").unwrap(), r.channel("v[1]").unwrap(), r.channel("v[2]").unwrap());
            for (k, at) in r.times.iter().enumerate() {
                prop_assert!((x0[k] - moving(*at)).abs() <= 1e-9, "x0({}) = {} not {}", at, x0[k], moving(*at));
                prop_assert!((x1[k] - x0[k]).abs() <= 1e-12, "x1({}) = {} not {}", at, x1[k], x0[k]);
            }
            prop_assert_eq!(*n.last().unwrap(), n_starts as f64, "{:?}", r.events);
        }
    }
}
