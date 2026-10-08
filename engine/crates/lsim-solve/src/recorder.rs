//! Sampling the solution onto the output grid.
//!
//! Values at grid times come from the integrator's dense output, so the
//! grid never constrains the steps. Each interval's min, max and time-mean
//! use every point the integrator visited inside it (its internal steps,
//! both sides of any event) plus the interval's ends; the mean is the
//! trapezoidal integral over those points divided by the interval length.

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
}

impl Recorder {
    /// A recorder for `n` channels on `times`.
    pub fn new(n: usize, times: &[f64]) -> Self {
        let col = || vec![Vec::with_capacity(times.len()); n];
        Recorder {
            times: times.to_vec(),
            next: 0,
            values: col(),
            min: col(),
            max: col(),
            mean: col(),
            lo: vec![f64::INFINITY; n],
            hi: vec![f64::NEG_INFINITY; n],
            integral: vec![0.0; n],
            last_t: 0.0,
            last: vec![0.0; n],
            interval_start: 0.0,
        }
    }

    /// The next grid time still to record.
    pub fn next_grid_time(&self) -> Option<f64> {
        self.times.get(self.next).copied()
    }

    /// The first point (the grid's first time).
    pub fn start(&mut self, t: f64, v: &[f64]) {
        for (i, x) in v.iter().enumerate() {
            self.values[i].push(*x);
            self.min[i].push(*x);
            self.max[i].push(*x);
            self.mean[i].push(*x);
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
            self.integral[i] += 0.5 * (self.last[i] + x) * dt;
            self.lo[i] = self.lo[i].min(*x);
            self.hi[i] = self.hi[i].max(*x);
        }
        self.last.copy_from_slice(v);
        self.last_t = t;
    }

    /// The point that ends the running interval (the next grid time).
    pub fn grid_point(&mut self, t: f64, v: &[f64]) {
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
    }

    /// Values, min, max and mean per channel.
    #[allow(clippy::type_complexity)]
    pub fn finish(self) -> (Vec<Vec<f64>>, Vec<Vec<f64>>, Vec<Vec<f64>>, Vec<Vec<f64>>) {
        (self.values, self.min, self.max, self.mean)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mean_is_the_trapezoid_integral() {
        let mut r = Recorder::new(1, &[0.0, 1.0]);
        r.start(0.0, &[0.0]);
        r.interior(0.5, &[1.0]);
        r.grid_point(1.0, &[0.0]);
        let (v, lo, hi, mean) = r.finish();
        assert_eq!(v[0], vec![0.0, 0.0]);
        assert_eq!(hi[0][1], 1.0);
        assert_eq!(lo[0][1], 0.0);
        assert!((mean[0][1] - 0.5).abs() < 1e-15);
    }
}
