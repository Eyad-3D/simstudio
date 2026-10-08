//! Tables: 1-D and 2-D interpolation data held as runtime parameters
//! (DESIGN.md, *Expressions* and *Code generation*).
//!
//! An [`crate::Expr::Table`] names a table of the flat system by its index
//! in [`crate::FlatSystem::tables`]. The data are inputs of the compiled
//! model, like parameter values: changing them never recompiles. The code
//! generator builds the interpolant (monotone cubic by default: C¹, no
//! overshoot) and evaluates it with its derivatives.

use crate::flat::InstanceId;
use serde::{Deserialize, Serialize};

/// How a table interpolates between its points.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default, Serialize, Deserialize)]
pub enum Interpolation {
    /// Monotone piecewise cubic: continuous first derivatives (no events
    /// at breakpoints) and no overshoot (monotone wherever the data are).
    /// 2-D tables use bicubic Hermite patches on the grid.
    #[default]
    MonotoneCubic,
    /// Piecewise linear (bilinear for 2-D), as today's app interpolates.
    Linear,
}

/// What a table gives outside its data along one axis: today's
/// `tableOutside` setting.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default, Serialize, Deserialize)]
pub enum Outside {
    /// The axis may not leave its data: the run stops with a message
    /// naming the table. (The value continues along the edge slope, so the
    /// solver's trial points outside are harmless; the run loop watches
    /// the axis with the compiled model's table guards.)
    Error,
    /// Hold the edge value (today's default).
    #[default]
    Clamp,
    /// Continue along the slope at the edge.
    Linear,
}

/// A table's data, SI.
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
pub struct TableData {
    /// the first axis's breakpoints, strictly increasing
    pub x: Vec<f64>,
    /// the second axis's breakpoints, strictly increasing; empty for a
    /// 1-D table
    pub y: Vec<f64>,
    /// the values: `values[i]` at `x[i]` (1-D), or row-major
    /// `values[i * y.len() + j]` at `(x[i], y[j])` (2-D)
    pub values: Vec<f64>,
    /// how it interpolates
    pub interpolation: Interpolation,
    /// outside the data: `[first axis, second axis]`
    pub outside: [Outside; 2],
}

impl TableData {
    /// A 1-D table.
    pub fn new_1d(x: Vec<f64>, values: Vec<f64>) -> Self {
        TableData { x, values, ..Default::default() }
    }

    /// A 2-D table on the grid `x × y`, values row-major (`x` outer).
    pub fn new_2d(x: Vec<f64>, y: Vec<f64>, values: Vec<f64>) -> Self {
        TableData { x, y, values, ..Default::default() }
    }

    /// 1 or 2: how many arguments the table takes.
    pub fn dims(&self) -> usize {
        if self.y.is_empty() { 1 } else { 2 }
    }

    /// Whether the data are well formed: at least one point per axis,
    /// strictly increasing finite breakpoints, finite values, one value
    /// per grid point. The message says what is wrong.
    pub fn check(&self) -> Result<(), String> {
        fn axis(a: &[f64], what: &str) -> Result<(), String> {
            if a.is_empty() {
                return Err(format!("the {what} axis has no points"));
            }
            if let Some(v) = a.iter().find(|v| !v.is_finite()) {
                return Err(format!("the {what} axis has a point that is not a number ({v})"));
            }
            if let Some(w) = a.windows(2).find(|w| w[1] <= w[0]) {
                return Err(format!(
                    "the {what} axis is not strictly increasing ({} then {})",
                    w[0], w[1]
                ));
            }
            Ok(())
        }
        axis(&self.x, "first")?;
        let n = if self.y.is_empty() {
            self.x.len()
        } else {
            axis(&self.y, "second")?;
            self.x.len() * self.y.len()
        };
        if self.values.len() != n {
            return Err(format!("{} values for {n} points", self.values.len()));
        }
        if let Some(v) = self.values.iter().find(|v| !v.is_finite()) {
            return Err(format!("a value is not a number ({v})"));
        }
        Ok(())
    }
}

/// A table of the flat system.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct FlatTable {
    /// full dotted name (`motor.loss_map`)
    pub name: String,
    /// the instance it belongs to
    pub instance: InstanceId,
    /// its data
    pub data: TableData,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_the_data() {
        assert!(TableData::new_1d(vec![0.0, 1.0], vec![1.0, 2.0]).check().is_ok());
        let e = TableData::new_1d(vec![0.0, 0.0], vec![1.0, 2.0]).check().unwrap_err();
        assert!(e.contains("strictly increasing"), "{e}");
        let t = TableData::new_2d(vec![0.0, 1.0], vec![0.0, 1.0, 2.0], vec![0.0; 6]);
        assert_eq!(t.dims(), 2);
        assert!(t.check().is_ok());
        let e = TableData::new_2d(vec![0.0, 1.0], vec![0.0], vec![0.0; 3]).check().unwrap_err();
        assert!(e.contains("3 values for 2 points"), "{e}");
    }
}
