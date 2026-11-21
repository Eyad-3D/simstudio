//! Tables: interpolation data of any number of axes, held as runtime
//! parameters (DESIGN.md, *Expressions* and *Code generation*).
//!
//! A table enters a component as a table parameter
//! ([`crate::ParamValue::Table`]) and is read in its equations with
//! [`crate::Expr::Table`] (component scope: the parameter's name first,
//! then one abscissa per axis). Flattening gives every table parameter one
//! [`FlatTable`] in [`crate::FlatSystem::tables`] (a part handed its
//! parent's table shares it), and a flat [`crate::Expr::Table`] names the
//! table by its index there. The data are inputs of the compiled model,
//! like parameter values: changing them never recompiles. The code
//! generator builds the interpolant (monotone cubic by default: C¹, no
//! overshoot) and evaluates it with its derivatives.
//!
//! One model serves every number of axes: breakpoints per axis, the values
//! row-major with the first axis outermost, an outside rule and a unit per
//! axis.

use crate::flat::{InstanceId, ParamId};
use crate::units::Unit;
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

/// One axis of a table: its breakpoints, what it gives past them, and
/// their unit.
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
pub struct TableAxis {
    /// the breakpoints, strictly increasing (one: the table is constant
    /// along this axis, never outside its data)
    pub points: Vec<f64>,
    /// past the data
    #[serde(default)]
    pub outside: Outside,
    /// the breakpoints' unit: a coherent SI unit text as declared units
    /// are (empty: dimensionless)
    #[serde(default)]
    pub unit: String,
    /// what it is, for messages (a Map block's input: `Speed`; empty:
    /// unnamed, told by its place)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
}

impl TableAxis {
    /// An axis of `points`, held at its edges, dimensionless.
    pub fn new(points: Vec<f64>) -> TableAxis {
        TableAxis { points, ..Default::default() }
    }
}

/// A table's data, SI, with its rules: one or more axes, and a value at
/// every point of their grid.
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
pub struct TableData {
    /// its axes, in the order its reads give their arguments
    pub axes: Vec<TableAxis>,
    /// the values, in the table parameter's unit, row-major with the first
    /// axis outermost: with `n_k` points on axis `k`, the value at the
    /// points' indexes `(i_0, …, i_(N-1))` is element
    /// `((i_0 · n_1 + i_1) · n_2 + …) · n_(N-1) + i_(N-1)` (the last axis
    /// changes fastest)
    pub values: Vec<f64>,
    /// how it interpolates
    pub interpolation: Interpolation,
}

impl TableData {
    /// A table on the grid of `axes`, values row-major (the first axis
    /// outermost), monotone cubic, every axis held at its edges and
    /// dimensionless.
    pub fn new(axes: Vec<Vec<f64>>, values: Vec<f64>) -> Self {
        TableData {
            axes: axes.into_iter().map(TableAxis::new).collect(),
            values,
            ..Default::default()
        }
    }

    /// A 1-D table.
    pub fn new_1d(x: Vec<f64>, values: Vec<f64>) -> Self {
        TableData::new(vec![x], values)
    }

    /// A 2-D table on the grid `x × y`, values row-major (`x` outer).
    pub fn new_2d(x: Vec<f64>, y: Vec<f64>, values: Vec<f64>) -> Self {
        TableData::new(vec![x, y], values)
    }

    /// How many arguments the table takes: its number of axes.
    pub fn dims(&self) -> usize {
        self.axes.len()
    }

    /// Axis `k`'s breakpoints.
    pub fn points(&self, k: usize) -> &[f64] {
        &self.axes[k].points
    }

    /// Axis `k`'s outside rule.
    pub fn outside(&self, k: usize) -> Outside {
        self.axes[k].outside
    }

    /// Sets every axis's outside rule, in order.
    pub fn with_outside(mut self, outside: &[Outside]) -> Self {
        for (a, o) in self.axes.iter_mut().zip(outside) {
            a.outside = *o;
        }
        self
    }

    /// Sets every axis's unit, in order.
    pub fn with_units(mut self, units: &[&str]) -> Self {
        for (a, u) in self.axes.iter_mut().zip(units) {
            a.unit = u.to_string();
        }
        self
    }

    /// How many points its grid has (the values it needs).
    pub fn grid_points(&self) -> usize {
        self.axes.iter().map(|a| a.points.len()).product()
    }

    /// Whether the data are well formed: at least one axis, at least one
    /// point per axis, strictly increasing finite breakpoints, finite
    /// values, one value per grid point. The message says what is wrong,
    /// in plain words.
    pub fn check(&self) -> Result<(), String> {
        if self.axes.is_empty() {
            return Err("the table has no axis".into());
        }
        let count = self.axes.len();
        for (k, ax) in self.axes.iter().enumerate() {
            let which = axis_name(k, count);
            let a = &ax.points;
            if a.is_empty() {
                return Err(format!("{which} has no points"));
            }
            if let Some(v) = a.iter().find(|v| !v.is_finite()) {
                return Err(format!("{which} has a point that is not a number ({v})"));
            }
            if let Some(w) = a.windows(2).find(|w| w[1] <= w[0]) {
                return Err(format!(
                    "the points of {which} must increase strictly, but {} is followed by {}",
                    w[0], w[1]
                ));
            }
        }
        let n = self.grid_points();
        if self.values.len() != n {
            return Err(format!("the table has {n} grid points but {} values", self.values.len()));
        }
        if let Some(v) = self.values.iter().find(|v| !v.is_finite()) {
            return Err(format!("a value is not a number ({v})"));
        }
        Ok(())
    }
}

/// How a message names axis `k` of a table of `count` axes: "its axis"
/// for a table of one, "the first axis", "the second axis" … otherwise.
pub fn axis_name(k: usize, count: usize) -> String {
    if count == 1 { "its axis".into() } else { format!("the {} axis", ordinal(k)) }
}

/// `first`, `second` … for index `k` (from 0); `11th` … past the tenth.
pub fn ordinal(k: usize) -> String {
    const ORD: [&str; 10] = [
        "first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth", "ninth",
        "tenth",
    ];
    match ORD.get(k) {
        Some(w) => (*w).to_string(),
        None => format!("{}th", k + 1),
    }
}

/// A table of the flat system: what a flat [`crate::Expr::Table`] indexes.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct FlatTable {
    /// full dotted name of its table parameter (`motor.loss_map`)
    pub name: String,
    /// the instance it belongs to
    pub instance: InstanceId,
    /// its table parameter's record, when it came from one (the
    /// parameter's value there is this table's index)
    #[serde(default)]
    pub param: Option<ParamId>,
    /// the unit of its values
    #[serde(default)]
    pub unit: Unit,
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
        assert_eq!(e, "the points of its axis must increase strictly, but 0 is followed by 0");
        let t = TableData::new_2d(vec![0.0, 1.0], vec![0.0, 1.0, 2.0], vec![0.0; 6]);
        assert_eq!(t.dims(), 2);
        assert!(t.check().is_ok());
        let e = TableData::new_2d(vec![0.0, 1.0], vec![0.0], vec![0.0; 3]).check().unwrap_err();
        assert_eq!(e, "the table has 2 grid points but 3 values");
        let e = TableData::new_1d(vec![], vec![]).check().unwrap_err();
        assert_eq!(e, "its axis has no points");
        // any number of axes: the third axis named as such, the grid counted
        let t = TableData::new(vec![vec![0.0, 1.0], vec![0.0], vec![1.0, 2.0, 3.0]], vec![0.0; 6]);
        assert_eq!((t.dims(), t.grid_points()), (3, 6));
        assert!(t.check().is_ok());
        let e = TableData::new(vec![vec![0.0], vec![1.0], vec![2.0, 2.0]], vec![0.0; 2])
            .check()
            .unwrap_err();
        assert_eq!(
            e,
            "the points of the third axis must increase strictly, but 2 is followed by 2"
        );
    }
}
