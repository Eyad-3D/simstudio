//! Today's lookup tables (`backend/app/solver/maps.py`), read from the
//! app's JSON and profile text, and made into the IR's runtime tables
//! ([`TableData`], read in equations with `lsim_ir::expr::table`).
//!
//! Today's engine interpolates linearly between points; past the data an
//! axis holds its edge value (Clamp), extends its edge segment's slope
//! (Linear) or stops the run (Error); a 2-D table is linear along its inner
//! axis on the two outer sheets that bracket the point, then linear
//! between them, and each sheet may have its own inner points. So the
//! tables made here interpolate linearly ([`Interpolation::Linear`]) with
//! today's outside rule per axis, and a 2-D table's sheets are resampled
//! onto the union of their inner points ([`Table2::grid_data`]), which is
//! exact for linear interpolation. [`Table1::eval`] and [`Table2::eval`]
//! are today's `interp1`/`interp2`, for start guesses and tests.
//!
//! A profile (a Driving Task's or Road Profile's points) may repeat an
//! abscissa: a step. A runtime table's breakpoints increase strictly, so
//! [`Table1::split_steps`] takes the steps out as jumps that the block adds
//! as `if` terms (events), leaving a continuous table.

use crate::x::{ident, n};
use lsim_ir::component::build::param;
use lsim_ir::expr::Expr;
pub use lsim_ir::table::{Interpolation, Outside};
use lsim_ir::{ParamDecl, ParamValue, TableData};
use serde_json::Value;

/// Today's outside-the-data setting from its text (`clamp`, `linear`,
/// `error`).
pub fn outside(text: &str) -> Outside {
    match text {
        "linear" => Outside::Linear,
        "error" => Outside::Error,
        _ => Outside::Clamp,
    }
}

/// A table parameter holding `data` (values in `unit`).
pub fn table_param(name: &str, unit: &str, data: TableData, doc: &str) -> ParamDecl {
    ParamDecl { default: ParamValue::Table(data), ..param(name, unit, 0.0, doc) }
}

/// A 1-D table: points (x, y) with x increasing; a repeated x is a step
/// (the value jumps just after it, as today's profiles do).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Table1 {
    /// abscissae, non-decreasing
    pub x: Vec<f64>,
    /// values
    pub y: Vec<f64>,
    /// past the data
    pub outside: Outside,
}

/// A 2-D table: sheets along the outer axis, each a 1-D table along the
/// inner axis.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Table2 {
    /// the outer axis, increasing
    pub outer: Vec<f64>,
    /// one inner table per outer point (its `outside` is the inner axis')
    pub sheets: Vec<Table1>,
    /// past the outer axis' data
    pub outer_outside: Outside,
}

fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(x) => x.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

impl Table1 {
    /// From points in any order (sorted by x, stably).
    pub fn from_points(mut pts: Vec<(f64, f64)>, outside: Outside) -> Table1 {
        pts.sort_by(|a, b| a.0.total_cmp(&b.0));
        Table1 {
            x: pts.iter().map(|p| p.0).collect(),
            y: pts.iter().map(|p| p.1).collect(),
            outside,
        }
    }

    /// From today's JSON `{x: y}` (keys are numeric text). Duplicate keys
    /// cannot occur in JSON; an empty or non-numeric table is an error.
    pub fn from_json(v: &Value, outside: Outside) -> Result<Table1, String> {
        let obj = v.as_object().ok_or("a 1-D table must be an {x: value} object")?;
        if obj.is_empty() {
            return Err("a 1-D table needs at least one point".into());
        }
        let mut pts = vec![];
        for (k, val) in obj {
            let x: f64 =
                k.trim().parse().map_err(|_| format!("table key '{k}' is not a number"))?;
            let y = num(val).ok_or_else(|| format!("table value at {k} is not a number"))?;
            pts.push((x, y));
        }
        Ok(Table1::from_points(pts, outside))
    }

    /// A profile string `x:v; x:v; …` (`,` also separates a pair), as the
    /// Driving Task and Road Profile take it; entries that are not pairs
    /// of finite numbers are skipped, as today.
    pub fn from_profile(text: &str) -> Table1 {
        let mut pts = vec![];
        for chunk in text.replace('\n', ";").split(';') {
            let chunk = chunk.trim();
            if chunk.is_empty() {
                continue;
            }
            let sep = if chunk.contains(':') { ':' } else { ',' };
            if let Some((a, b)) = chunk.split_once(sep)
                && let (Ok(x), Ok(y)) = (a.trim().parse::<f64>(), b.trim().parse::<f64>())
                && x.is_finite()
                && y.is_finite()
            {
                pts.push((x, y));
            }
        }
        Table1::from_points(pts, Outside::Clamp)
    }

    /// The table with every abscissa × kx and every value × ky (unit
    /// conversion to SI).
    pub fn scaled(&self, kx: f64, ky: f64) -> Table1 {
        Table1 {
            x: self.x.iter().map(|v| v * kx).collect(),
            y: self.y.iter().map(|v| v * ky).collect(),
            outside: self.outside,
        }
    }

    /// Its value at x: today's `interp1` exactly.
    pub fn eval(&self, x: f64) -> f64 {
        let (xs, ys, k) = (&self.x, &self.y, self.x.len());
        if k == 0 {
            return 0.0;
        }
        let linear = self.outside == Outside::Linear;
        if x <= xs[0] {
            if linear && k > 1 && x < xs[0] && xs[1] != xs[0] {
                return ys[0] + (ys[1] - ys[0]) * (x - xs[0]) / (xs[1] - xs[0]);
            }
            return ys[0];
        }
        if x >= xs[k - 1] || x.is_nan() {
            if linear && k > 1 && x > xs[k - 1] && xs[k - 1] != xs[k - 2] {
                return ys[k - 1]
                    + (ys[k - 1] - ys[k - 2]) * (x - xs[k - 1]) / (xs[k - 1] - xs[k - 2]);
            }
            return ys[k - 1];
        }
        // the first point at or beyond x ends the segment that holds it
        let lo = xs[1..k - 1].partition_point(|&v| v < x) + 1;
        let (xa, ya, xb, yb) = (xs[lo - 1], ys[lo - 1], xs[lo], ys[lo]);
        if xb == xa {
            return yb;
        }
        ya + (yb - ya) * (x - xa) / (xb - xa)
    }

    /// The lowest and highest abscissa.
    pub fn range(&self) -> (f64, f64) {
        (self.x.first().copied().unwrap_or(0.0), self.x.last().copied().unwrap_or(0.0))
    }

    /// The table as runtime data: linear, today's outside rule, the axis
    /// in `x_unit`. The abscissae must increase strictly (take a profile's
    /// steps out first with [`Table1::split_steps`]).
    pub fn data(&self, x_unit: &str) -> Result<TableData, String> {
        let d = TableData {
            interpolation: Interpolation::Linear,
            outside: [self.outside, Outside::Clamp],
            axis_units: [x_unit.to_string(), String::new()],
            ..TableData::new_1d(self.x.clone(), self.y.clone())
        };
        d.check()?;
        Ok(d)
    }

    /// The table without its steps, and the steps: `(x, jump)` where the
    /// value jumps by `jump` just past `x`. The table plus the jumps is
    /// this table (today's `interp1` of a repeated abscissa).
    pub fn split_steps(&self) -> (Table1, Vec<(f64, f64)>) {
        let mut x: Vec<f64> = vec![];
        let mut y: Vec<f64> = vec![];
        let mut jumps: Vec<(f64, f64)> = vec![];
        let mut offset = 0.0;
        for (&xi, &yi) in self.x.iter().zip(&self.y) {
            if let Some(&last) = x.last()
                && xi == last
            {
                let before = *y.last().expect("a point") + offset;
                let j = yi - before;
                if j != 0.0 {
                    jumps.push((xi, j));
                    offset += j;
                }
                continue;
            }
            x.push(xi);
            y.push(yi - offset);
        }
        (Table1 { x, y, outside: self.outside }, jumps)
    }

    /// ∫ of the table from `a` to `b` (reference, by the segment rule).
    pub fn integral(&self, a: f64, b: f64) -> f64 {
        // Simpson on each linear piece is exact; split at the breakpoints
        let (lo, hi, sgn) = if a <= b { (a, b, 1.0) } else { (b, a, -1.0) };
        let mut cuts: Vec<f64> = vec![lo];
        cuts.extend(self.x.iter().copied().filter(|&v| v > lo && v < hi));
        cuts.push(hi);
        let mut total = 0.0;
        for w in cuts.windows(2) {
            let (p, q) = (w[0], w[1]);
            if q <= p {
                continue;
            }
            // values just inside the piece (steps sit at its ends)
            let eps = 1e-12 * (q - p);
            let (fp, fq) = (self.eval(p + eps), self.eval(q - eps));
            total += 0.5 * (fp + fq) * (q - p);
        }
        sgn * total
    }
}

impl Table2 {
    /// From today's JSON `{outer: {inner: value}}`.
    pub fn from_json(v: &Value, outer: Outside, inner: Outside) -> Result<Table2, String> {
        let obj = v.as_object().ok_or("a 2-D table must be an {x: {y: value}} object")?;
        if obj.is_empty() {
            return Err("a 2-D table needs at least one sheet".into());
        }
        let mut sheets: Vec<(f64, Table1)> = vec![];
        for (k, row) in obj {
            let x: f64 =
                k.trim().parse().map_err(|_| format!("table key '{k}' is not a number"))?;
            sheets.push((x, Table1::from_json(row, inner)?));
        }
        sheets.sort_by(|a, b| a.0.total_cmp(&b.0));
        Ok(Table2 {
            outer: sheets.iter().map(|s| s.0).collect(),
            sheets: sheets.into_iter().map(|s| s.1).collect(),
            outer_outside: outer,
        })
    }

    /// Every outer abscissa × ko, inner × ki, value × kv.
    pub fn scaled(&self, ko: f64, ki: f64, kv: f64) -> Table2 {
        Table2 {
            outer: self.outer.iter().map(|v| v * ko).collect(),
            sheets: self.sheets.iter().map(|s| s.scaled(ki, kv)).collect(),
            outer_outside: self.outer_outside,
        }
    }

    /// Its value: today's `interp2` exactly.
    pub fn eval(&self, xo: f64, xi: f64) -> f64 {
        let k = self.outer.len();
        if k == 0 {
            return 0.0;
        }
        let lin = self.outer_outside == Outside::Linear;
        let lo = if !(self.outer[0] < xo && xo < self.outer[k - 1]) {
            let edge = if xo <= self.outer[0] { 0 } else { k - 1 };
            if !lin || k < 2 || xo == self.outer[edge] || xo.is_nan() {
                return self.sheets[edge].eval(xi);
            }
            if edge == 0 { 1 } else { k - 1 }
        } else {
            self.outer[1..k - 1].partition_point(|&v| v < xo) + 1
        };
        let (xa, xb) = (self.outer[lo - 1], self.outer[lo]);
        let (ya, yb) = (self.sheets[lo - 1].eval(xi), self.sheets[lo].eval(xi));
        if xb == xa {
            return yb;
        }
        ya + (yb - ya) * (xo - xa) / (xb - xa)
    }

    /// The table as runtime data on one rectangular grid: the outer
    /// points × the union of the sheets' inner points, each sheet read
    /// there by its own interpolation and outside rule — exact for linear
    /// interpolation. `units`: the outer and inner axes' units.
    pub fn grid_data(&self, units: [&str; 2]) -> Result<TableData, String> {
        let mut inner: Vec<f64> = self.sheets.iter().flat_map(|s| s.x.iter().copied()).collect();
        inner.sort_by(f64::total_cmp);
        inner.dedup();
        let mut values = Vec::with_capacity(self.outer.len() * inner.len());
        for s in &self.sheets {
            for &xi in &inner {
                values.push(s.eval(xi));
            }
        }
        let inner_outside = self.sheets.first().map(|s| s.outside).unwrap_or_default();
        let d = TableData {
            interpolation: Interpolation::Linear,
            outside: [self.outer_outside, inner_outside],
            axis_units: [units[0].to_string(), units[1].to_string()],
            ..TableData::new_2d(self.outer.clone(), inner, values)
        };
        d.check()?;
        Ok(d)
    }

    /// The inner axis' range common to every sheet (its narrowest), as
    /// today's `inner_range`.
    pub fn inner_range(&self) -> Option<(f64, f64)> {
        let rows: Vec<&Table1> = self.sheets.iter().filter(|s| s.x.len() > 1).collect();
        if rows.is_empty() {
            return None;
        }
        Some((
            rows.iter().map(|s| s.x[0]).fold(f64::NEG_INFINITY, f64::max),
            rows.iter().map(|s| *s.x.last().unwrap()).fold(f64::INFINITY, f64::min),
        ))
    }
}

/// Parameters of value 1 that give a bare number a unit (`0 W` is written
/// `0 * unit_W`), one per unit, named `unit_<unit>`.
#[derive(Default, Debug, Clone)]
pub struct UnitCarriers {
    params: Vec<ParamDecl>,
}

impl UnitCarriers {
    /// The carrier of `unit` (`None` for a dimensionless unit).
    pub fn of(&mut self, unit: &str) -> Option<Expr> {
        if unit.is_empty() || unit == "1" {
            return None;
        }
        let name = format!("unit_{}", ident(unit));
        if !self.params.iter().any(|p| p.name == name) {
            self.params.push(param(&name, unit, 1.0, "unit carrier: 1 in this unit"));
        }
        Some(n(&name))
    }

    /// The carriers to add to the component's parameters.
    pub fn into_params(self) -> Vec<ParamDecl> {
        self.params
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Linear interpolation on a grid axis with an outside rule (what the
    /// runtime table does with `Interpolation::Linear`).
    fn lin(xs: &[f64], ys: &[f64], x: f64, o: Outside) -> f64 {
        Table1 { x: xs.to_vec(), y: ys.to_vec(), outside: o }.eval(x)
    }

    #[test]
    fn steps_come_out_as_jumps() {
        let s = Table1::from_profile("0:0; 10:5; 10:8; 20:8; 30:2; 30:0");
        let (cont, jumps) = s.split_steps();
        assert_eq!(jumps, vec![(10.0, 3.0), (30.0, -2.0)]);
        assert!(cont.x.windows(2).all(|w| w[1] > w[0]));
        // (at a step that ends the data, today gives the value after it at that
        // very point: a single instant, left out)
        for x in [-1.0, 5.0, 10.0, 10.000001, 15.0, 25.0, 29.9, 30.5, 40.0] {
            let with: f64 =
                cont.eval(x) + jumps.iter().filter(|j| x > j.0).map(|j| j.1).sum::<f64>();
            assert!((with - s.eval(x)).abs() < 1e-12, "at {x}: {with} vs {}", s.eval(x));
        }
        assert_eq!(s.eval(10.0), 5.0);
        assert!(cont.data("s").is_ok());
        assert!(s.data("s").is_err(), "a step is not a runtime table");
    }

    #[test]
    fn sheets_on_one_grid_read_as_today() {
        let json = serde_json::json!({
            "0": {"0": 0.1, "100": 1.6, "200": 3.2, "350": 8.5},
            "3000": {"0": 0.35, "100": 2.0, "200": 4.2, "350": 10.4},
            "6000": {"0": 0.7, "120": 2.8, "350": 12.8}
        });
        for (o, i) in [(Outside::Clamp, Outside::Clamp), (Outside::Linear, Outside::Linear)] {
            let t = Table2::from_json(&json, o, i).unwrap();
            let d = t.grid_data(["rad/s", "N.m"]).unwrap();
            assert_eq!(d.y, vec![0.0, 100.0, 120.0, 200.0, 350.0]);
            assert_eq!(d.interpolation, Interpolation::Linear);
            let ny = d.y.len();
            for xo in [-500.0, 0.0, 1500.0, 3000.0, 4500.0, 6000.0, 7000.0] {
                for xi in [-10.0, 0.0, 50.0, 110.0, 120.0, 300.0, 400.0] {
                    // bilinear on the grid: inner first on every row, then outer
                    let rows: Vec<f64> = (0..d.x.len())
                        .map(|r| lin(&d.y, &d.values[r * ny..(r + 1) * ny], xi, d.outside[1]))
                        .collect();
                    let v = lin(&d.x, &rows, xo, d.outside[0]);
                    assert!(
                        (v - t.eval(xo, xi)).abs() < 1e-9,
                        "{o:?} at ({xo}, {xi}): {v} vs {}",
                        t.eval(xo, xi)
                    );
                }
            }
        }
    }
}
