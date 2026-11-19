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

    /// The table with a constant extension, as wide as its span, past its
    /// first point when that point is at or below `lo` and past its last
    /// when that is at or above `hi`: `lo` and `hi` are the least and
    /// greatest values its argument takes (a speed magnitude's 0, an SOC
    /// held to 0-1), so it never reads the extension, and the run loop's
    /// watch of the table's edge is never resting exactly on it. An end the
    /// argument can pass is not padded: a read past the data there is
    /// watched (counted, or the run stopped on an Error axis) as today's
    /// engine does. A single point is left as it is ([`Table1::data`]).
    pub fn padded(&self, lo: Option<f64>, hi: Option<f64>) -> Table1 {
        if self.x.len() < 2 {
            return self.clone();
        }
        let (x0, xn) = (self.x[0], self.x[self.x.len() - 1]);
        let d = (xn - x0).abs().max(1.0);
        let mut t = self.clone();
        if lo.is_some_and(|b| x0 <= b) {
            t.x.insert(0, x0 - d);
            t.y.insert(0, self.y[0]);
        }
        if hi.is_some_and(|b| xn >= b) {
            t.x.push(xn + d);
            t.y.push(self.y[self.y.len() - 1]);
        }
        t
    }

    /// The table as runtime data: linear, today's outside rule, the axis
    /// in `x_unit`. The abscissae must increase strictly (take a profile's
    /// steps out first with [`Table1::split_steps`]). A single point is a
    /// constant, which today's engine never reads outside its data: it is
    /// held (Clamp) and spread 1 either side, so the run loop's watch of
    /// its edge does not rest on it (the backend does not count what it
    /// sees past them).
    pub fn data(&self, x_unit: &str) -> Result<TableData, String> {
        let (x, y, outside) = match (self.x.as_slice(), self.y.as_slice()) {
            ([x0], [y0]) => (vec![x0 - 1.0, *x0, x0 + 1.0], vec![*y0; 3], Outside::Clamp),
            _ => (self.x.clone(), self.y.clone(), self.outside),
        };
        let d = TableData {
            interpolation: Interpolation::Linear,
            outside: [outside, Outside::Clamp],
            axis_units: [x_unit.to_string(), String::new()],
            ..TableData::new_1d(x, y)
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

/// The refusal of a 2-D table whose sheets' inner ranges differ.
const UNEVEN: &str = "its sheets do not all cover the same range of its second axis";
const WHY: &str = ": today's engine counts a read outside the narrowest as outside the data, \
                   which the new engine does not watch; give every sheet the same first and last \
                   point";

impl Table2 {
    /// From today's JSON `{outer: {inner: value}}`. Sheets whose inner
    /// ranges differ are refused ([`Table2::grid_data`]).
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
        let t = Table2 {
            outer: sheets.iter().map(|s| s.0).collect(),
            sheets: sheets.into_iter().map(|s| s.1).collect(),
            outer_outside: outer,
        };
        // (checked here, on the data as given: grid_data sees them scaled
        // to SI and padded, which a message should not show)
        if let (Some((lo, hi)), Some((a, b))) = (t.inner_range(), t.inner_span())
            && (lo, hi) != (a, b)
        {
            return Err(format!("{UNEVEN} ({lo} to {hi} on every sheet, {a} to {b} on some){WHY}"));
        }
        Ok(t)
    }

    /// The lowest and highest inner point of any sheet.
    fn inner_span(&self) -> Option<(f64, f64)> {
        let xs = self.sheets.iter().flat_map(|s| s.x.iter().copied());
        xs.fold(None, |r, x| Some(r.map_or((x, x), |(a, b): (f64, f64)| (a.min(x), b.max(x)))))
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

    /// The table padded (see [`Table1::padded`]) on its outer axis
    /// (`outer`: the least and greatest values its argument takes) and on
    /// every sheet's inner axis (`inner`, the same for its argument).
    pub fn padded(
        &self,
        outer: (Option<f64>, Option<f64>),
        inner: (Option<f64>, Option<f64>),
    ) -> Table2 {
        let mut t = self.clone();
        for s in &mut t.sheets {
            *s = s.padded(inner.0, inner.1);
        }
        if self.outer.len() > 1 {
            let (x0, xn) = (self.outer[0], self.outer[self.outer.len() - 1]);
            let d = (xn - x0).abs().max(1.0);
            if outer.0.is_some_and(|b| x0 <= b) {
                t.outer.insert(0, x0 - d);
                let s0 = t.sheets[0].clone();
                t.sheets.insert(0, s0);
            }
            if outer.1.is_some_and(|b| xn >= b) {
                t.outer.push(xn + d);
                let sn = t.sheets.last().expect("a sheet").clone();
                t.sheets.push(sn);
            }
        }
        t
    }

    /// The table as runtime data on one rectangular grid: the outer
    /// points × the union of the sheets' inner points, each sheet read
    /// there by its own interpolation and outside rule — exact for linear
    /// interpolation. `units`: the outer and inner axes' units.
    ///
    /// The run loop watches the grid's edges, so they are today's: a
    /// single outer point, or sheets with no second point, are constants
    /// along that axis, never outside their data today (held, Clamp, and
    /// spread 1 either side, as [`Table1::data`] does); sheets whose inner
    /// ranges differ are refused, as today's engine counts a read outside
    /// the narrowest ([`Table2::inner_range`]) and the grid's edges are
    /// the widest.
    pub fn grid_data(&self, units: [&str; 2]) -> Result<TableData, String> {
        let mut inner: Vec<f64> = self.sheets.iter().flat_map(|s| s.x.iter().copied()).collect();
        inner.sort_by(f64::total_cmp);
        inner.dedup();
        let mut inner_outside = self.sheets.first().map(|s| s.outside).unwrap_or_default();
        if let (Some(&a), Some(&b)) = (inner.first(), inner.last()) {
            match self.inner_range() {
                None => {
                    inner.insert(0, a - 1.0);
                    inner.push(b + 1.0);
                    inner_outside = Outside::Clamp;
                }
                // (from_json refused such data with its own numbers; these
                // are scaled and padded, so none is shown)
                Some((lo, hi)) if (lo, hi) != (a, b) => return Err(format!("{UNEVEN}{WHY}")),
                Some(_) => {}
            }
        }
        let (mut outer, mut sheets, mut outer_outside) =
            (self.outer.clone(), self.sheets.iter().collect::<Vec<_>>(), self.outer_outside);
        if let ([o], [s]) = (self.outer.as_slice(), self.sheets.as_slice()) {
            (outer, sheets, outer_outside) =
                (vec![o - 1.0, *o, o + 1.0], vec![s; 3], Outside::Clamp);
        }
        let mut values = Vec::with_capacity(outer.len() * inner.len());
        for s in &sheets {
            for &xi in &inner {
                values.push(s.eval(xi));
            }
        }
        let d = TableData {
            interpolation: Interpolation::Linear,
            outside: [outer_outside, inner_outside],
            axis_units: [units[0].to_string(), units[1].to_string()],
            ..TableData::new_2d(outer, inner, values)
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
    fn only_an_end_the_argument_cannot_pass_is_padded() {
        // a speed magnitude (never below 0) on data from 0, and from 1000
        let from0 = Table1::from_points(vec![(0.0, 1.0), (500.0, 2.0)], Outside::Error);
        assert_eq!(from0.padded(Some(0.0), None).x, vec![-500.0, 0.0, 500.0]);
        let from1000 = Table1::from_points(vec![(1000.0, 1.0), (5000.0, 2.0)], Outside::Error);
        assert_eq!(from1000.padded(Some(0.0), None).x, vec![1000.0, 5000.0], "read below 1000");
        // an SOC held to 0-1 on data for 90-100 %: only its top is padded
        let ocv = Table1::from_points(vec![(0.9, 368.0), (1.0, 376.0)], Outside::Error);
        let x = ocv.padded(Some(0.0), Some(1.0)).x;
        assert_eq!((x.len(), x[0], x[1]), (3, 0.9, 1.0));
        // a 2-D map's outer and inner axes alike
        let sheet = |x0: f64| Table1::from_points(vec![(x0, 1.0), (300.0, 9.0)], Outside::Clamp);
        let map = Table2 {
            outer: vec![1000.0, 16000.0],
            sheets: vec![sheet(0.0), sheet(0.0)],
            outer_outside: Outside::Error,
        };
        let p = map.padded((Some(0.0), None), (Some(0.0), None));
        assert_eq!(p.outer, vec![1000.0, 16000.0]);
        assert_eq!(p.sheets[0].x, vec![-300.0, 0.0, 300.0]);
        let d = p.grid_data(["rad/s", "N.m"]).unwrap();
        assert_eq!((d.x[0], d.y[0], d.outside[0]), (1000.0, -300.0, Outside::Error));
    }

    #[test]
    fn a_single_point_is_a_constant_never_outside() {
        let t = Table1::from_points(vec![(0.0, 0.3)], Outside::Error);
        let d = t.padded(Some(0.0), None).data("rad/s").unwrap();
        assert_eq!(
            (d.x, d.values, d.outside[0]),
            (vec![-1.0, 0.0, 1.0], vec![0.3; 3], Outside::Clamp)
        );
        // a 2-D table with one sheet, and with sheets of one point each
        let one = |x: f64, y: f64| Table1::from_points(vec![(x, y)], Outside::Error);
        let t =
            Table2 { outer: vec![5.0], sheets: vec![one(1.0, 2.0)], outer_outside: Outside::Error };
        let d = t.grid_data(["1", "1"]).unwrap();
        assert_eq!(
            (d.x, d.y.clone(), d.outside),
            (vec![4.0, 5.0, 6.0], vec![0.0, 1.0, 2.0], [Outside::Clamp; 2])
        );
        assert_eq!(d.values, vec![2.0; 9]);
        let t = Table2 {
            outer: vec![0.0, 10.0],
            sheets: vec![one(1.0, 2.0), one(3.0, 4.0)],
            outer_outside: Outside::Error,
        };
        let d = t.grid_data(["1", "1"]).unwrap();
        assert_eq!((d.y, d.outside), (vec![0.0, 1.0, 3.0, 4.0], [Outside::Error, Outside::Clamp]));
        assert_eq!(d.values, vec![2.0, 2.0, 2.0, 2.0, 4.0, 4.0, 4.0, 4.0]);
    }

    #[test]
    fn sheets_of_different_inner_ranges_are_refused() {
        // today counts a read outside the narrowest: the grid's edge is the widest
        let json = serde_json::json!({"0": {"0": 1.0, "300": 2.0}, "100": {"50": 1.0, "300": 2.0}});
        let e = Table2::from_json(&json, Outside::Clamp, Outside::Clamp).unwrap_err();
        assert!(e.contains("(50 to 300 on every sheet, 0 to 300 on some)"), "{e}");
        // said in the data's own numbers, not scaled to SI and padded below
        // 0 as the grid sees them (before: "-300 to 300", in rad/s)
        let json =
            serde_json::json!({"1000": {"0": 1.0, "300": 2.0}, "2000": {"10": 1.0, "300": 2.0}});
        let e = Table2::from_json(&json, Outside::Clamp, Outside::Clamp).unwrap_err();
        assert!(e.contains("(10 to 300 on every sheet, 0 to 300 on some)"), "{e}");
        let even =
            serde_json::json!({"1000": {"0": 1.0, "300": 2.0}, "2000": {"0": 1.0, "300": 2.0}});
        let mut t = Table2::from_json(&even, Outside::Clamp, Outside::Clamp).unwrap();
        t.sheets[1] = Table1::from_points(vec![(10.0, 1.0), (300.0, 2.0)], Outside::Clamp);
        let e =
            t.scaled(0.1, 2.0, 1.0).padded((None, None), (Some(0.0), None)).grid_data(["1", "1"]);
        let e = e.unwrap_err();
        assert!(
            e.starts_with("its sheets do not all cover the same range of its second axis:"),
            "{e}"
        );
        assert!(!e.contains("-600") && !e.contains("600 on"), "{e}");
        // a sheet of one point inside the others' range is no wider
        let json = serde_json::json!({"0": {"0": 1.0, "300": 2.0}, "100": {"50": 1.0}});
        assert!(
            Table2::from_json(&json, Outside::Clamp, Outside::Clamp)
                .unwrap()
                .grid_data(["1", "1"])
                .is_ok()
        );
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
