//! Prescribed trajectories: piecewise-linear samples.
//!
//! A trace is a function of time, not a list of samples: where a sample
//! lies on the line through its neighbours (to within a few units of
//! round-off) the trace's slope does not change there, and the sample is
//! no breakpoint. Fast mode splits its steps, and restarts, only at the
//! breakpoints ([`Trace::breakpoints`]), and reads each piece between two
//! breakpoints as the line through its ends ([`Trace::on_piece`]), so
//! adding samples on a trace's lines changes nothing beyond round-off.

use crate::FastError;

/// A prescribed trajectory: piecewise-linear samples (one per prescribed
/// variable of the [`crate::InverseSpec`], in its order). Between samples
/// the value is linear, so its derivative (the acceleration, for a speed
/// trace) is constant on each segment: the constant acceleration the
/// quasi-static (backward-facing) method assumes over each step.
#[derive(Clone, Debug, PartialEq)]
pub struct Trace {
    /// times, s, increasing
    pub t: Vec<f64>,
    /// values, SI
    pub value: Vec<f64>,
}

impl Trace {
    /// A trace from samples; times must increase strictly, both lists have
    /// the same length (at least two) and every number is finite.
    pub fn new(t: Vec<f64>, value: Vec<f64>) -> Result<Trace, FastError> {
        let tr = Trace { t, value };
        tr.check()?;
        Ok(tr)
    }

    /// Checks the samples (see [`Trace::new`]).
    pub fn check(&self) -> Result<(), FastError> {
        if self.t.len() != self.value.len() {
            return Err(FastError::Trace(format!(
                "a trace has {} times but {} values",
                self.t.len(),
                self.value.len()
            )));
        }
        if self.t.len() < 2 {
            return Err(FastError::Trace("a trace needs at least two samples".into()));
        }
        if self.t.iter().chain(&self.value).any(|x| !x.is_finite()) {
            return Err(FastError::Trace("a trace holds a value that is not a number".into()));
        }
        if let Some(w) = self.t.windows(2).find(|w| w[1] <= w[0]) {
            return Err(FastError::Trace(format!(
                "a trace's times must increase: {} s is followed by {} s",
                w[0], w[1]
            )));
        }
        Ok(())
    }

    /// The first time.
    pub fn start(&self) -> f64 {
        self.t[0]
    }

    /// The last time.
    pub fn end(&self) -> f64 {
        self.t[self.t.len() - 1]
    }

    /// The segment `[t_k, t_{k+1}]` that holds `t` (the last one for
    /// `t` at or after the end, the first one before the start).
    pub fn segment(&self, t: f64) -> usize {
        let n = self.t.len();
        match self.t.binary_search_by(|x| x.total_cmp(&t)) {
            Ok(k) => k.min(n - 2),
            Err(0) => 0,
            Err(k) => (k - 1).min(n - 2),
        }
    }

    /// Value and slope at `t` on segment `k` (extrapolated linearly outside it).
    #[inline]
    pub fn on_segment(&self, k: usize, t: f64) -> (f64, f64) {
        let (t0, t1) = (self.t[k], self.t[k + 1]);
        let (v0, v1) = (self.value[k], self.value[k + 1]);
        let s = (v1 - v0) / (t1 - t0);
        (v0 + s * (t - t0), s)
    }

    /// The samples where the slope changes (the breakpoints), with the
    /// first and the last: the ends of the trace's pieces, increasing. A
    /// sample is not a breakpoint when it lies on the line from the
    /// piece's start to the next sample within the round-off of the
    /// numbers involved (four units in the last place of the largest
    /// value, and of the largest time times the slope).
    pub fn breakpoints(&self) -> Vec<usize> {
        let n = self.t.len();
        let mut bp = vec![0];
        for k in 1..n.saturating_sub(1) {
            let a = *bp.last().expect("the first sample");
            let (ta, va) = (self.t[a], self.value[a]);
            let (tk, vk) = (self.t[k], self.value[k]);
            let (tn, vn) = (self.t[k + 1], self.value[k + 1]);
            let s = (vn - va) / (tn - ta);
            let line = va + s * (tk - ta);
            let scale = va.abs().max(vk.abs()).max(vn.abs()) + s.abs() * ta.abs().max(tn.abs());
            if (vk - line).abs() > 4.0 * f64::EPSILON * scale {
                bp.push(k);
            }
        }
        if n > 1 {
            bp.push(n - 1);
        }
        bp
    }

    /// The piece (between `bp[p]` and `bp[p + 1]`, from
    /// [`Trace::breakpoints`]) that holds `t`: the later one at a
    /// breakpoint, the last one at or after the end, the first one
    /// before the start.
    pub fn piece(&self, bp: &[usize], t: f64) -> usize {
        let pieces = bp.len().saturating_sub(1).max(1);
        let k = bp.partition_point(|&i| self.t[i] <= t);
        k.saturating_sub(1).min(pieces - 1)
    }

    /// Value and slope at `t` on piece `p` of the breakpoints `bp`: the
    /// line through the piece's ends (extrapolated outside it).
    #[inline]
    pub fn on_piece(&self, bp: &[usize], p: usize, t: f64) -> (f64, f64) {
        let (a, b) = (bp[p], bp[p + 1]);
        let (t0, t1) = (self.t[a], self.t[b]);
        let (v0, v1) = (self.value[a], self.value[b]);
        let s = (v1 - v0) / (t1 - t0);
        (v0 + s * (t - t0), s)
    }

    /// Value and slope at `t`, as fast mode reads them (the later piece at
    /// a breakpoint).
    pub fn at(&self, t: f64) -> (f64, f64) {
        let bp = self.breakpoints();
        self.on_piece(&bp, self.piece(&bp, t), t)
    }

    /// A trace sampled every `dt` from a trace given at other times (for
    /// a cycle stored with gaps): the same piecewise-linear function only
    /// when every original time is on the new grid.
    pub fn resampled(&self, dt: f64) -> Trace {
        let (a, b) = (self.start(), self.end());
        let n = ((b - a) / dt - 1e-9).ceil().max(1.0) as usize;
        let t: Vec<f64> = (0..=n).map(|k| (a + k as f64 * dt).min(b)).collect();
        let value = t
            .iter()
            .map(|&x| {
                let k = self.segment(x);
                self.on_segment(k, x).0
            })
            .collect();
        Trace { t, value }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_segments_and_slopes() {
        let tr = Trace::new(vec![0.0, 2.0, 3.0], vec![0.0, 4.0, 1.0]).unwrap();
        assert_eq!(tr.segment(-1.0), 0);
        assert_eq!(tr.segment(0.0), 0);
        assert_eq!(tr.segment(1.9), 0);
        assert_eq!(tr.segment(2.0), 1);
        assert_eq!(tr.segment(3.0), 1);
        assert_eq!(tr.segment(9.0), 1);
        assert_eq!(tr.on_segment(0, 1.0), (2.0, 2.0));
        assert_eq!(tr.on_segment(1, 2.5), (2.5, -3.0));
        let r = tr.resampled(0.5);
        assert_eq!(r.t.len(), 7);
        assert_eq!(r.value[3], 3.0);
    }

    #[test]
    fn breakpoints_are_where_the_slope_changes() {
        // 0 → 4 by 2 m/s², held, then down: samples on the lines are none
        let tr = Trace::new(
            vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0],
            vec![0.0, 2.0, 4.0, 4.0, 4.0, 2.0, 0.0],
        )
        .unwrap();
        let bp = tr.breakpoints();
        assert_eq!(bp, [0, 2, 4, 6]);
        assert_eq!(tr.piece(&bp, -1.0), 0);
        assert_eq!(tr.piece(&bp, 1.5), 0);
        assert_eq!(tr.piece(&bp, 2.0), 1);
        assert_eq!(tr.piece(&bp, 4.5), 2);
        assert_eq!(tr.piece(&bp, 9.0), 2);
        assert_eq!(tr.on_piece(&bp, 0, 1.5), (3.0, 2.0));
        assert_eq!(tr.at(5.5), (1.0, -2.0));
        // a sample put on a line by arithmetic (not exactly on it) is none
        let mid = 0.1 + (0.7 - 0.1) * (1.0 / 3.0);
        let tr = Trace::new(vec![0.0, 1.0, 3.0], vec![0.1, mid, 0.7]).unwrap();
        assert_eq!(tr.breakpoints(), [0, 2]);
        // a real change of slope, however small against the speeds, is one
        let tr = Trace::new(vec![0.0, 1.0, 2.0], vec![30.0, 30.0, 30.0 + 1e-9]).unwrap();
        assert_eq!(tr.breakpoints(), [0, 1, 2]);
    }

    #[test]
    fn rejects_bad_samples() {
        assert!(Trace::new(vec![0.0, 0.0], vec![1.0, 2.0]).is_err());
        assert!(Trace::new(vec![0.0], vec![1.0]).is_err());
        assert!(Trace::new(vec![0.0, 1.0], vec![1.0]).is_err());
        assert!(Trace::new(vec![0.0, 1.0], vec![1.0, f64::NAN]).is_err());
    }
}
