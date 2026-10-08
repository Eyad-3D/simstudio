//! Lookup tables, read the way today's engine reads them
//! (`backend/app/solver/maps.py`): piecewise linear between points; past
//! the data an axis holds its edge value (Clamp), extends its edge
//! segment's slope (Linear) or stops the run (Error); a 2-D table is
//! linear along its inner axis on the two outer sheets that bracket the
//! point, then linear between them. Each sheet may have its own inner
//! points.
//!
//! **Stand-in.** The IR's runtime tables (`ParamValue::Table1D/2D`,
//! `Expr::Table`, monotone cubic or linear, values as runtime data) are
//! work packages 1 and 3. Until they land, a table is expanded here into
//! an expression of the abscissae — a sum of clamped segments, with no
//! relations, so it needs no events — and its data are baked into the
//! block that uses it (a table change re-prepares that block). The
//! expansion is exact: [`Table1::eval`] and [`Table2::eval`] are today's
//! `interp1`/`interp2`, and the tests check the expressions against them.
//! Swapping the expansion for `Expr::Table` later changes only
//! [`Table1::expr`] and [`Table2::expr`] and the [`UnitCarriers`].

use crate::x::{c, ident, max, min, n};
use lsim_ir::ParamDecl;
use lsim_ir::component::build::param;
use lsim_ir::expr::Expr;
use serde_json::Value;

/// What a table does past its data on one axis (today's `tableOutside`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Outside {
    /// hold the edge value (the default)
    #[default]
    Clamp,
    /// extend the edge segment's slope
    Linear,
    /// the run stops there (expanded as Clamp plus an assertion)
    Error,
}

impl Outside {
    /// From today's setting text (`clamp`, `linear`, `error`).
    pub fn parse(text: &str) -> Outside {
        match text {
            "linear" => Outside::Linear,
            "error" => Outside::Error,
            _ => Outside::Clamp,
        }
    }
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

    /// The table as an expression of `x` (a dimensionless number: wrap it
    /// with [`UnitCarriers`] for units). `x` is repeated in the result, so
    /// pass a variable, not a large expression.
    pub fn expr(&self, x: Expr) -> Expr {
        let (xs, ys, k) = (&self.x, &self.y, self.x.len());
        if k == 0 {
            return c(0.0);
        }
        if k == 1 {
            return c(ys[0]);
        }
        let linear = self.outside == Outside::Linear;
        let mut terms: Vec<Expr> = vec![];
        // the first and last segments that have a length (the linear
        // extension uses them)
        let first = (1..k).find(|&i| xs[i] != xs[i - 1]);
        let last = (1..k).rev().find(|&i| xs[i] != xs[i - 1]);
        for i in 1..k {
            let (xa, xb, dy) = (xs[i - 1], xs[i], ys[i] - ys[i - 1]);
            if dy == 0.0 {
                continue;
            }
            if xa == xb {
                // a step: the value jumps just past xa
                terms.push(c(dy) * crate::x::ite(crate::x::gt(x.clone(), c(xa)), c(1.0), c(0.0)));
                continue;
            }
            let s = dy / (xb - xa);
            let lo_open = linear && Some(i) == first;
            let hi_open = linear && Some(i) == last;
            let seg = match (lo_open, hi_open) {
                (true, true) => x.clone(),
                (true, false) => min(x.clone(), c(xb)),
                (false, true) => max(x.clone(), c(xa)),
                (false, false) => min(max(x.clone(), c(xa)), c(xb)),
            };
            terms.push(c(s) * (seg - c(xa)));
        }
        let mut e = c(ys[0]);
        for t in terms {
            e = e + t;
        }
        e
    }

    /// ∫ from the first abscissa to `x` of the table (piecewise quadratic):
    /// a stored energy from a table of potential (∫ OCV dQ).
    pub fn integral_expr(&self, x: Expr) -> Expr {
        let (xs, ys, k) = (&self.x, &self.y, self.x.len());
        if k == 0 {
            return c(0.0);
        }
        let x0 = xs[0];
        let mut e = c(ys[0]) * (x.clone() - c(x0));
        if k == 1 {
            return e;
        }
        let linear = self.outside == Outside::Linear;
        let first = (1..k).find(|&i| xs[i] != xs[i - 1]);
        let last = (1..k).rev().find(|&i| xs[i] != xs[i - 1]);
        for i in 1..k {
            let (xa, xb, dy) = (xs[i - 1], xs[i], ys[i] - ys[i - 1]);
            if dy == 0.0 {
                continue;
            }
            if xa == xb {
                // a step of dy at xa: adds dy · max(0, x − xa)
                e = e + c(dy) * max(x.clone() - c(xa), c(0.0));
                continue;
            }
            let (s, h) = (dy / (xb - xa), xb - xa);
            let lo_open = linear && Some(i) == first;
            let hi_open = linear && Some(i) == last;
            let u = match (lo_open, hi_open) {
                (true, true) => x.clone() - c(xa),
                (true, false) => min(x.clone(), c(xb)) - c(xa),
                (false, true) => max(x.clone(), c(xa)) - c(xa),
                (false, false) => min(max(x.clone(), c(xa)), c(xb)) - c(xa),
            };
            let mut g = c(0.5) * u.clone() * u;
            if !hi_open {
                g = g + c(h) * max(x.clone() - c(xb), c(0.0));
            }
            e = e + c(s) * g;
        }
        e
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

    /// The hat function of sheet k along the outer axis.
    fn hat(&self, k: usize) -> Table1 {
        Table1 {
            x: self.outer.clone(),
            y: (0..self.outer.len()).map(|j| if j == k { 1.0 } else { 0.0 }).collect(),
            outside: self.outer_outside,
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

    /// The table as an expression of (outer, inner): Σ hat_k(outer) ·
    /// sheet_k(inner). Pass variables.
    pub fn expr(&self, xo: Expr, xi: Expr) -> Expr {
        let k = self.outer.len();
        if k == 0 {
            return c(0.0);
        }
        if k == 1 {
            return self.sheets[0].expr(xi);
        }
        let mut e: Option<Expr> = None;
        for j in 0..k {
            let term = self.hat(j).expr(xo.clone()) * self.sheets[j].expr(xi.clone());
            e = Some(match e {
                None => term,
                Some(acc) => acc + term,
            });
        }
        e.unwrap_or(c(0.0))
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

/// Parameters of value 1 that carry a table's units through the unit
/// check (an expanded table is a plain number expression): the table is
/// read at `x / [x unit]` and its value is `[y unit] · f(…)`. Stand-in
/// until runtime tables carry their own units.
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
            self.params.push(param(
                &name,
                unit,
                1.0,
                "unit carrier of value 1 for a table expansion (stand-in until runtime tables)",
            ));
        }
        Some(n(&name))
    }

    /// A 1-D table read at `x` (in `x_unit`), its value in `y_unit`.
    pub fn t1(&mut self, t: &Table1, x: Expr, x_unit: &str, y_unit: &str) -> Expr {
        let xi = match self.of(x_unit) {
            Some(u) => x / u,
            None => x,
        };
        let e = t.expr(xi);
        match self.of(y_unit) {
            Some(u) => u * e,
            None => e,
        }
    }

    /// A 2-D table read at (`xo`, `xi`).
    pub fn t2(
        &mut self,
        t: &Table2,
        xo: Expr,
        xo_unit: &str,
        xi: Expr,
        xi_unit: &str,
        y_unit: &str,
    ) -> Expr {
        let a = match self.of(xo_unit) {
            Some(u) => xo / u,
            None => xo,
        };
        let b = match self.of(xi_unit) {
            Some(u) => xi / u,
            None => xi,
        };
        let e = t.expr(a, b);
        match self.of(y_unit) {
            Some(u) => u * e,
            None => e,
        }
    }

    /// The carriers to add to the component's parameters.
    pub fn into_params(self) -> Vec<ParamDecl> {
        self.params
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsim_ir::eval::{Env, eval};
    use lsim_ir::flat::{ParamId, VarId};

    struct X(f64, f64);
    impl Env for X {
        fn time(&self) -> f64 {
            0.0
        }
        fn var(&self, v: VarId) -> f64 {
            if v.0 == 0 { self.0 } else { self.1 }
        }
        fn der(&self, _: VarId) -> f64 {
            0.0
        }
        fn param(&self, _: ParamId) -> f64 {
            1.0
        }
    }

    fn at(e: &Expr, x: f64, y: f64) -> f64 {
        eval(e, &X(x, y))
    }

    #[test]
    fn expansion_matches_todays_interpolation() {
        for outside in [Outside::Clamp, Outside::Linear] {
            let t = Table1::from_points(
                vec![(0.0, 300.0), (10.0, 318.0), (20.0, 330.0), (40.0, 342.0), (100.0, 376.0)],
                outside,
            );
            let e = t.expr(Expr::Var(VarId(0)));
            for k in -40..=160 {
                let x = k as f64 * 0.9 - 3.0;
                assert!((at(&e, x, 0.0) - t.eval(x)).abs() < 1e-9, "{outside:?} at {x}");
            }
            // the integral against the trapezoid rule on the pieces
            let ie = t.integral_expr(Expr::Var(VarId(0)));
            for x in [-5.0, 0.0, 3.0, 10.0, 33.0, 100.0, 120.0] {
                let want = t.integral(0.0, x);
                assert!((at(&ie, x, 0.0) - want).abs() < 1e-7, "{outside:?} ∫ to {x}");
            }
        }
        // a step (a repeated abscissa): value before, jump just after
        let s = Table1::from_profile("0:0; 10:5; 10:8; 20:8");
        let e = s.expr(Expr::Var(VarId(0)));
        for x in [5.0, 10.0, 10.000001, 15.0, 25.0] {
            assert!((at(&e, x, 0.0) - s.eval(x)).abs() < 1e-9, "step at {x}");
        }
        assert_eq!(s.eval(10.0), 5.0);
    }

    #[test]
    fn two_dimensional_expansion_matches() {
        let json = serde_json::json!({
            "0": {"0": 0.1, "100": 1.6, "200": 3.2, "350": 8.5},
            "3000": {"0": 0.35, "100": 2.0, "200": 4.2, "350": 10.4},
            "6000": {"0": 0.7, "120": 2.8, "350": 12.8}
        });
        for (o, i) in [(Outside::Clamp, Outside::Clamp), (Outside::Linear, Outside::Linear)] {
            let t = Table2::from_json(&json, o, i).unwrap();
            let e = t.expr(Expr::Var(VarId(0)), Expr::Var(VarId(1)));
            for xo in [-500.0, 0.0, 1500.0, 3000.0, 4500.0, 6000.0, 7000.0] {
                for xi in [-10.0, 0.0, 50.0, 120.0, 300.0, 400.0] {
                    assert!(
                        (at(&e, xo, xi) - t.eval(xo, xi)).abs() < 1e-9,
                        "{o:?} at ({xo}, {xi}): {} vs {}",
                        at(&e, xo, xi),
                        t.eval(xo, xi)
                    );
                }
            }
        }
    }
}
