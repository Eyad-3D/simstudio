//! Prescribed trajectories: piecewise-linear samples.

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
    fn rejects_bad_samples() {
        assert!(Trace::new(vec![0.0, 0.0], vec![1.0, 2.0]).is_err());
        assert!(Trace::new(vec![0.0], vec![1.0]).is_err());
        assert!(Trace::new(vec![0.0, 1.0], vec![1.0]).is_err());
        assert!(Trace::new(vec![0.0, 1.0], vec![1.0, f64::NAN]).is_err());
    }
}
