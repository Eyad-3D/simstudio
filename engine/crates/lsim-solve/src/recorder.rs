//! Sampling the solution onto the output grid.
//!
//! Values at grid times come from the integrator's dense output, so the
//! grid never constrains the steps. Each interval's min, max and time-mean
//! use every point the integrator visited inside it (its internal steps,
//! both sides of any event) plus the interval's ends; the mean is the
//! trapezoidal integral over those points divided by the interval length.
//! A grid time that falls exactly on an event takes the values just after
//! it, and the values just before it (its left limit) are kept beside it.
//!
//! Over the whole run each channel's square and magnitude are integrated
//! the same way, as integrals of the line through every point visited
//! (∫ x² dt and ∫ |x| dt, a crossing of zero inside a piece split where
//! the line crosses): what a duty's RMS and a flow's two directions are
//! read from, at every step the integrator took and not at sampled ones.
//! A trace ([`crate::SolverOptions::trace`]) samples a few channels at
//! times of its own from the same dense output, besides the grid and
//! without changing what the grid records. A window
//! ([`crate::SolverOptions::windows`]) keeps a channel's highest moving
//! average over a span of time, of the same line through every point
//! visited, read at each of them (a battery's power over its check's
//! averaging window, as an energy meter's log has it: nothing before the
//! start).

use std::collections::VecDeque;

/// Collects channels on the output grid.
pub struct Recorder {
    times: Vec<f64>,
    next: usize,
    values: Vec<Vec<f64>>,
    min: Vec<Vec<f64>>,
    max: Vec<Vec<f64>>,
    mean: Vec<Vec<f64>>,
    // the running interval
    lo: Vec<f64>,
    hi: Vec<f64>,
    integral: Vec<f64>,
    last_t: f64,
    last: Vec<f64>,
    interval_start: f64,
    /// grid points at an event: (index, the values just before it)
    left: Vec<crate::LeftLimit>,
    /// over the run: ∫ x² dt and ∫ |x| dt of the line through the points
    square: Vec<f64>,
    magnitude: Vec<f64>,
    trace: Option<Traced>,
    /// the next trace time to record
    trace_next: usize,
    windows: Vec<Window>,
}

/// A channel's highest moving average over a window of time, of the line
/// through every point the integrator visited, read at each of them, with
/// no value before the run's start (it counts as 0 there); a window of 0
/// s: the channel's highest value ([`crate::SolverOptions::windows`]).
#[derive(Clone, Debug, PartialEq)]
pub struct WindowPeak {
    /// the channel (index of the flat variables)
    pub channel: usize,
    /// the window, s
    pub window: f64,
    /// the highest average (−∞ before any point)
    pub peak: f64,
    /// when the window it was over ended, s
    pub t: f64,
}

/// A [`WindowPeak`] being kept: the points in its window and the one just
/// before it, each with the line's integral from the start up to it.
struct Window {
    peak: WindowPeak,
    pts: VecDeque<(f64, f64, f64)>,
}

impl Window {
    fn add(&mut self, t: f64, x: f64) {
        let w = self.peak.window;
        let cum = self.pts.back().map_or(0.0, |&(ta, xa, ca)| ca + 0.5 * (xa + x) * (t - ta));
        self.pts.push_back((t, x, cum));
        let avg = if w > 0.0 {
            let lo = t - w;
            while self.pts.len() > 1 && self.pts[1].0 <= lo {
                self.pts.pop_front();
            }
            // the integral up to the window's start: 0 before the first
            // point (then the start's, whose integral is 0), else along the
            // line from the point before it
            let (ta, xa, ca) = self.pts[0];
            let before = if lo <= ta {
                ca
            } else {
                let (tb, xb, _) = self.pts[1];
                let x_lo = xa + (xb - xa) * (lo - ta) / (tb - ta);
                ca + 0.5 * (xa + x_lo) * (lo - ta)
            };
            (cum - before) / w
        } else {
            if self.pts.len() > 1 {
                self.pts.pop_front(); // (only the last point is needed)
            }
            x
        };
        if avg > self.peak.peak {
            self.peak.peak = avg;
            self.peak.t = t;
        }
    }
}

/// Channels sampled at times of their own from the dense output (a run's
/// trace, [`crate::SolverOptions::trace`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Traced {
    /// the times, s (those the run reached)
    pub times: Vec<f64>,
    /// the recorded channels sampled (their indices)
    pub channels: Vec<usize>,
    /// `values[j][k]`: channel `channels[j]` at `times[k]`
    pub values: Vec<Vec<f64>>,
}

/// What a [`Recorder`] collected.
pub struct Recorded {
    /// the output times recorded: the grid's, or up to where the run
    /// stopped and that time ([`Recorder::end_at`])
    pub times: Vec<f64>,
    /// per channel, the value at each grid time
    pub values: Vec<Vec<f64>>,
    /// per channel, the lowest value over each interval
    pub min: Vec<Vec<f64>>,
    /// per channel, the highest value over each interval
    pub max: Vec<Vec<f64>>,
    /// per channel, the time-mean over each interval
    pub mean: Vec<Vec<f64>>,
    /// the grid points that fall on an event, with their left limits
    pub left: Vec<crate::LeftLimit>,
    /// per channel, ∫ x² dt over the run
    pub square: Vec<f64>,
    /// per channel, ∫ |x| dt over the run
    pub magnitude: Vec<f64>,
    /// the trace, when one was asked for
    pub trace: Option<Traced>,
    /// the windows asked for, in their order
    pub windows: Vec<WindowPeak>,
}

impl Recorder {
    /// A recorder for `n` channels on `times`: an error, not an abort,
    /// when they do not fit in memory.
    pub fn new(n: usize, times: &[f64]) -> Result<Self, crate::SolveError> {
        let what = || format!("{n} channels at {} output times", times.len());
        // (each channel's own allocation of the grid's length: `vec![v;
        // n]` clones `v`, and a clone keeps none of its spare capacity, so
        // the channels grew by reallocation, run after run)
        let col = || -> Result<Vec<Vec<f64>>, crate::SolveError> {
            let mut out = Vec::new();
            crate::try_room(&mut out, n, what)?;
            for _ in 0..n {
                let mut c = Vec::new();
                crate::try_room(&mut c, times.len(), what)?;
                out.push(c);
            }
            Ok(out)
        };
        let mut own = Vec::new();
        crate::try_room(&mut own, times.len(), what)?;
        own.extend_from_slice(times);
        Ok(Recorder {
            times: own,
            next: 0,
            values: col()?,
            min: col()?,
            max: col()?,
            mean: col()?,
            lo: vec![f64::INFINITY; n],
            hi: vec![f64::NEG_INFINITY; n],
            integral: vec![0.0; n],
            last_t: 0.0,
            last: vec![0.0; n],
            interval_start: 0.0,
            left: vec![],
            square: vec![0.0; n],
            magnitude: vec![0.0; n],
            trace: None,
            trace_next: 0,
            windows: vec![],
        })
    }

    /// The same, keeping also each `(channel, window)`'s highest moving
    /// average ([`WindowPeak`]).
    pub fn with_windows(mut self, windows: &[(usize, f64)]) -> Self {
        self.windows = windows
            .iter()
            .map(|&(channel, window)| Window {
                peak: WindowPeak { channel, window, peak: f64::NEG_INFINITY, t: 0.0 },
                pts: VecDeque::new(),
            })
            .collect();
        self
    }

    /// The same, sampling also the channels `channels` at `times`
    /// (increasing): an error, not an abort, when they do not fit.
    pub fn with_trace(
        mut self,
        times: Vec<f64>,
        channels: Vec<usize>,
    ) -> Result<Self, crate::SolveError> {
        let what = || format!("a trace of {} channels at {} times", channels.len(), times.len());
        let mut values = Vec::new();
        crate::try_room(&mut values, channels.len(), what)?;
        for _ in &channels {
            let mut c = Vec::new();
            crate::try_room(&mut c, times.len(), what)?;
            values.push(c);
        }
        self.trace = Some(Traced { times, channels, values });
        Ok(self)
    }

    /// The next time still to record: a grid time, or a trace's.
    pub fn next_grid_time(&self) -> Option<f64> {
        let out = self.times.get(self.next).copied();
        let tr = self.trace.as_ref().and_then(|tr| tr.times.get(self.trace_next).copied());
        match (out, tr) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// The next grid time still to record (not a trace's).
    pub fn next_output_time(&self) -> Option<f64> {
        self.times.get(self.next).copied()
    }

    /// The output points recorded so far: their times, and each channel's
    /// values at them (`values[c][k]`).
    pub fn so_far(&self) -> (&[f64], &[Vec<f64>]) {
        (&self.times[..self.next], &self.values)
    }

    /// Ends the record at `t`, where the run stopped before the grid's
    /// end: the grid times not reached are dropped and `t` is the last
    /// output point, with the channels `v` (the trace's last point too).
    /// False when the last point recorded is at `t` already.
    pub fn end_at(&mut self, t: f64, v: &[f64]) -> bool {
        self.times.truncate(self.next);
        if let Some(tr) = self.trace.as_mut() {
            tr.times.truncate(self.trace_next);
            if tr.times.last() != Some(&t) {
                tr.times.push(t);
            }
        }
        if self.times.last() == Some(&t) {
            return false;
        }
        self.times.push(t);
        self.grid_point(t, v)
    }

    /// The trace's point at `t`, when its next time is `t`.
    fn trace_point(&mut self, t: f64, v: &[f64]) {
        let Some(tr) = self.trace.as_mut() else { return };
        if tr.times.get(self.trace_next) != Some(&t) {
            return;
        }
        for (col, &c) in tr.values.iter_mut().zip(&tr.channels) {
            col.push(v[c]);
        }
        self.trace_next += 1;
    }

    /// The first point (the grid's first time).
    pub fn start(&mut self, t: f64, v: &[f64]) {
        self.trace_point(t, v);
        for (i, x) in v.iter().enumerate() {
            self.values[i].push(*x);
            self.min[i].push(*x);
            self.max[i].push(*x);
            self.mean[i].push(*x);
        }
        for w in &mut self.windows {
            w.add(t, v[w.peak.channel]);
        }
        self.next = 1;
        self.reset(t, v);
    }

    fn reset(&mut self, t: f64, v: &[f64]) {
        self.lo.copy_from_slice(v);
        self.hi.copy_from_slice(v);
        self.integral.iter_mut().for_each(|x| *x = 0.0);
        self.last.copy_from_slice(v);
        self.last_t = t;
        self.interval_start = t;
    }

    /// A point inside the running interval.
    pub fn interior(&mut self, t: f64, v: &[f64]) {
        let dt = t - self.last_t;
        for (i, x) in v.iter().enumerate() {
            let (a, b) = (self.last[i], *x);
            self.integral[i] += 0.5 * (a + b) * dt;
            self.lo[i] = self.lo[i].min(*x);
            self.hi[i] = self.hi[i].max(*x);
            // the line from a to b: its square, and its magnitude, which
            // crosses zero inside the piece where a and b differ in sign
            self.square[i] += (a * a + a * b + b * b) / 3.0 * dt;
            let m = if (a < 0.0) == (b < 0.0) || a == 0.0 || b == 0.0 {
                0.5 * (a.abs() + b.abs())
            } else {
                0.5 * (a * a + b * b) / (a.abs() + b.abs())
            };
            self.magnitude[i] += m * dt;
        }
        for w in &mut self.windows {
            w.add(t, v[w.peak.channel]);
        }
        self.last.copy_from_slice(v);
        self.last_t = t;
    }

    /// The point at the next time to record ([`Self::next_grid_time`]):
    /// the trace's point there, and, when it is the next grid time, the
    /// point that ends the running interval. Whether it was a grid time.
    pub fn grid_point(&mut self, t: f64, v: &[f64]) -> bool {
        self.trace_point(t, v);
        if self.times.get(self.next) != Some(&t) {
            return false;
        }
        self.interior(t, v);
        let len = t - self.interval_start;
        for (i, x) in v.iter().enumerate() {
            self.values[i].push(*x);
            self.min[i].push(self.lo[i]);
            self.max[i].push(self.hi[i]);
            self.mean[i].push(if len > 0.0 { self.integral[i] / len } else { *x });
        }
        self.next += 1;
        self.reset(t, v);
        true
    }

    /// An event at `t`: when the next grid point falls on it, `left` are
    /// the values just before it, `right` those after it (the grid
    /// point's); the channels it changed are kept.
    pub fn left_limit(&mut self, t: f64, left: &[f64], right: &[f64]) {
        if self.times.get(self.next) == Some(&t) {
            self.left.push(crate::LeftLimit::new(self.next, left, right));
        }
    }

    /// What was collected.
    pub fn finish(self) -> Recorded {
        let reached = self.trace_next;
        let trace = self.trace.map(|mut tr| {
            tr.times.truncate(reached);
            tr
        });
        Recorded {
            times: self.times,
            values: self.values,
            min: self.min,
            max: self.max,
            mean: self.mean,
            left: self.left,
            square: self.square,
            magnitude: self.magnitude,
            trace,
            windows: self.windows.into_iter().map(|w| w.peak).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mean_is_the_trapezoid_integral() {
        let mut r = Recorder::new(1, &[0.0, 1.0]).unwrap();
        r.start(0.0, &[0.0]);
        r.interior(0.5, &[1.0]);
        r.grid_point(1.0, &[0.0]);
        let got = r.finish();
        assert_eq!(got.values[0], vec![0.0, 0.0]);
        assert_eq!(got.max[0][1], 1.0);
        assert_eq!(got.min[0][1], 0.0);
        assert!((got.mean[0][1] - 0.5).abs() < 1e-15);
    }

    /// The run's integrals are those of the line through every point
    /// visited: a sine sampled finely gives its square's and its
    /// magnitude's integrals, a held value jumping at an event (both sides
    /// at one time) is exact, and a piece that crosses zero has its
    /// magnitude split where its line crosses.
    #[test]
    fn the_run_integrals_are_the_lines_through_the_points() {
        let n = 20_000;
        let mut r = Recorder::new(1, &[0.0, 1.0]).unwrap();
        r.start(0.0, &[0.0]);
        for k in 1..n {
            let t = k as f64 / n as f64;
            r.interior(t, &[lsim_ir::math::sin(2.0 * std::f64::consts::PI * t)]);
        }
        r.grid_point(1.0, &[0.0]);
        let got = r.finish();
        // over a period ∫ sin² is 1/2 and ∫ |sin| is 2/π; the lines' error is O(h²)
        assert!((got.square[0] - 0.5).abs() < 1e-7, "{}", got.square[0]);
        assert!((got.magnitude[0] - 2.0 / std::f64::consts::PI).abs() < 1e-7);

        // 3 for 0.5 s, then -1 (an event at 0.5 s: both sides recorded)
        let mut r = Recorder::new(1, &[0.0, 1.0]).unwrap();
        r.start(0.0, &[3.0]);
        r.interior(0.5, &[3.0]);
        r.interior(0.5, &[-1.0]);
        r.grid_point(1.0, &[-1.0]);
        let got = r.finish();
        assert_eq!(got.square[0], 0.5 * 9.0 + 0.5 * 1.0);
        assert_eq!(got.magnitude[0], 0.5 * 3.0 + 0.5 * 1.0);
        assert_eq!(got.mean[0][1], 0.5 * 3.0 - 0.5);

        // a line from 1 to -3 over 1 s crosses zero at 0.25 s
        let mut r = Recorder::new(1, &[0.0, 1.0]).unwrap();
        r.start(0.0, &[1.0]);
        r.grid_point(1.0, &[-3.0]);
        let got = r.finish();
        assert!((got.magnitude[0] - (0.5 * 0.25 + 1.5 * 0.75)).abs() < 1e-15);
        assert!((got.square[0] - 7.0 / 3.0).abs() < 1e-15);
    }

    /// A trace samples its channels at its own times, between the grid's
    /// and on them, and changes nothing the grid records.
    #[test]
    fn a_trace_samples_its_channels_without_changing_the_grid() {
        let run = |trace: bool| {
            let mut r = Recorder::new(2, &[0.0, 1.0, 2.0]).unwrap();
            if trace {
                r = r.with_trace(vec![0.0, 0.5, 1.0, 1.5, 2.0], vec![1]).unwrap();
            }
            r.start(0.0, &[0.0, 10.0]);
            let mut last = 0.0;
            while let Some(t) = r.next_grid_time() {
                // a step's end before each point, as the run loop has one
                if t - 0.25 > last {
                    r.interior(t - 0.25, &[t - 0.25, 10.0 + t - 0.25]);
                }
                last = t;
                let out = r.grid_point(t, &[t, 10.0 + t]);
                assert_eq!(out, t.fract() == 0.0, "{t}");
            }
            r.finish()
        };
        let (plain, traced) = (run(false), run(true));
        let tr = traced.trace.clone().unwrap();
        assert_eq!(tr.times, vec![0.0, 0.5, 1.0, 1.5, 2.0]);
        assert_eq!(tr.values, vec![vec![10.0, 10.5, 11.0, 11.5, 12.0]]);
        assert!(plain.trace.is_none());
        assert_eq!(plain.values, traced.values);
        assert_eq!(plain.max, vec![vec![0.0, 1.0, 2.0], vec![10.0, 11.0, 12.0]]);
    }

    /// A window's highest moving average is that of the line through the
    /// points visited, read at each, with nothing before the start: a held
    /// value reaches itself once the window is full, a pulse's area over a
    /// window that holds it all, a window starting between two points reads
    /// the line there; a window of 0 s is the highest value and its time.
    #[test]
    fn a_window_keeps_the_highest_moving_average_of_the_line() {
        let run = |pts: &[(f64, f64)], windows: &[(usize, f64)]| {
            let mut r =
                Recorder::new(1, &[pts[0].0, pts[pts.len() - 1].0]).unwrap().with_windows(windows);
            r.start(pts[0].0, &[pts[0].1]);
            for &(t, x) in &pts[1..pts.len() - 1] {
                r.interior(t, &[x]);
            }
            let &(t, x) = pts.last().unwrap();
            r.grid_point(t, &[x]);
            r.finish().windows
        };
        let held: Vec<(f64, f64)> = (0..=40).map(|k| (k as f64 * 0.25, 2.0)).collect();
        let got = run(&held, &[(0, 1.0)]);
        assert_eq!((got[0].peak, got[0].t), (2.0, 1.0)); // (0.5 at 0.25 s: nothing before the start)
        // a triangle of area 5 between 5 and 6 s
        let pulse = [(0.0, 0.0), (5.0, 0.0), (5.5, 10.0), (6.0, 0.0), (6.5, 0.0), (9.0, 0.0)];
        let got = run(&pulse, &[(0, 1.0), (0, 0.0), (0, 4.0)]);
        assert_eq!((got[0].peak, got[0].t), (5.0, 6.0));
        assert_eq!((got[1].peak, got[1].t), (10.0, 5.5));
        assert_eq!((got[2].peak, got[2].t), (5.0 / 4.0, 6.0));
        // the window at 3 s starts at 1.5 s, on the line from (1, 0) to (2, 4)
        let step = [(0.0, 0.0), (1.0, 0.0), (2.0, 4.0), (3.0, 4.0)];
        let got = run(&step, &[(0, 1.5)]);
        assert!((got[0].peak - (0.5 * (2.0 + 4.0) * 0.5 + 4.0) / 1.5).abs() < 1e-15);
        assert_eq!(got[0].t, 3.0);
        // both sides of an event at one time: a jump, the line holds it
        let jump = [(0.0, 1.0), (1.0, 1.0), (1.0, 3.0), (2.0, 3.0)];
        let got = run(&jump, &[(0, 1.0), (0, 0.0)]);
        assert_eq!((got[0].peak, got[0].t), (3.0, 2.0));
        assert_eq!((got[1].peak, got[1].t), (3.0, 1.0));
    }

    /// Channels that do not fit in memory are an error the caller gets, not
    /// an abort of the process.
    #[test]
    fn channels_that_do_not_fit_are_an_error() {
        let e = Recorder::new(1 << 40, &[0.0, 1.0]).err().expect("no room");
        assert!(e.to_string().contains("do not fit in memory"), "{e}");
    }
}
