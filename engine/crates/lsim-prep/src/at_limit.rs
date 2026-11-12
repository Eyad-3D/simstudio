//! A `when` whose threshold is exactly a limit the model holds a value at
//! (step 3 of the roadmap). Once the model holds the value at its limit,
//! the value sits on the threshold, and the condition can change on the
//! solver's rounding alone: on the Formula Student car a counter of the
//! battery's power above 80 kW, beside its Accumulator's 80 kW Output
//! Power Limit, stopped every build with an event storm. The limits are
//! the parameters an equation holds a value at: an argument of a `min`,
//! `max` or `limit` that is the value of one side of the equation (or of a
//! branch of its `if`); a `when` threshold equal to one (to 1e-9 of it, in
//! the same unit) is warned about, `WHEN-AT-LIMIT`, naming both parts and
//! the limit.

use crate::diagnose::warning;
use crate::params;
use lsim_ir::Diagnostic;
use lsim_ir::component::Library;
use lsim_ir::expr::{Builtin, Expr};
use lsim_ir::flat::{FlatSystem, ParamId};

/// How close a threshold must be to a limit to count as equal, relative.
const SAME: f64 = 1e-9;

/// Whether `e` reads only parameters and numbers.
fn fixed(e: &Expr) -> bool {
    !e.any(&mut |x| {
        matches!(
            x,
            Expr::Var(_)
                | Expr::Der(_)
                | Expr::Pre(_)
                | Expr::Time
                | Expr::Name(_)
                | Expr::Table { .. }
        )
    })
}

/// The parameters a side of an equation holds its value at: the arguments
/// of a `min`, `max` or `limit` that is the side's whole value, or a
/// branch's of an `if` (`p = if c then min(a, p_max) else 0`), not one deep
/// inside arithmetic (a `max(|v|, v_eps)` in a slip's denominator holds no
/// value at v_eps).
fn held_at(e: &Expr, f: &mut impl FnMut(ParamId)) {
    match e {
        Expr::Call(Builtin::Min | Builtin::Max | Builtin::Limit, args) => {
            for a in args {
                if let Expr::Param(p) = a {
                    f(*p);
                } else {
                    held_at(a, f);
                }
            }
        }
        Expr::If(_, a, b) => {
            held_at(a, f);
            held_at(b, f);
        }
        Expr::NoEvent(a) => held_at(a, f),
        _ => {}
    }
}

/// The `WHEN-AT-LIMIT` warnings of a flat model.
pub fn thresholds_at_limits(flat: &FlatSystem, lib: &Library) -> Vec<Diagnostic> {
    let values: Vec<f64> = flat.params.iter().map(|p| p.value).collect();
    // the limits: parameters a min, max or limit bounds a value with (not
    // 0, a sign's split, and not the 1e30 that stands for "no limit")
    let mut limits: Vec<ParamId> = vec![];
    for e in &flat.equations {
        for side in [&e.lhs, &e.rhs] {
            held_at(side, &mut |p| {
                let v = values[p.0 as usize];
                if v != 0.0 && v.abs() < 1e29 && !limits.contains(&p) {
                    limits.push(p);
                }
            });
        }
    }
    let mut out = vec![];
    if limits.is_empty() {
        return out;
    }
    for w in &flat.whens {
        let mut found: Vec<(f64, ParamId)> = vec![];
        w.condition.walk(&mut |x| {
            let Expr::Compare(_, a, b) = x else { return };
            let thr = match (fixed(a), fixed(b)) {
                (true, false) => a.as_ref(),
                (false, true) => b.as_ref(),
                _ => return,
            };
            let v = params::value(thr, &values);
            for &p in &limits {
                let l = values[p.0 as usize];
                let same_unit = match thr {
                    Expr::Param(q) => {
                        flat.params[q.0 as usize].unit == flat.params[p.0 as usize].unit
                    }
                    _ => true,
                };
                if same_unit && (v - l).abs() <= SAME * l.abs() && !found.iter().any(|f| f.1 == p) {
                    found.push((v, p));
                }
            }
        });
        for (v, p) in found {
            let fp = &flat.params[p.0 as usize];
            let local = fp.name.rsplit('.').next().unwrap_or(&fp.name);
            let decl = lib
                .components
                .get(&flat.instance(fp.instance).def)
                .and_then(|d| d.params.iter().find(|q| q.name == local));
            let what = decl.map_or(local.to_string(), |d| format!("{local}: {}", d.doc));
            let unit = decl.map_or("", |d| d.unit.as_str());
            let unit = if unit == "1" { String::new() } else { format!(" {unit}") };
            let (by, at) = (flat.top_part(fp.instance), flat.top_part(w.origin.instance));
            let mut d = warning(
                "WHEN-AT-LIMIT",
                format!(
                    "The `when` in {} compares a value with {v}{unit}, exactly a limit {} \
                     holds a value at ({what}). Held at that limit, the value sits on the \
                     threshold: the condition can change on rounding alone, the `when` fire \
                     many times, or the run stop with an event storm.",
                    flat.instance_name(at),
                    flat.instance_name(by)
                ),
            )
            .with_hint(format!(
                "Set the threshold a little above or below {v}{unit}, so that the condition \
                 changes only where the value really crosses it."
            ));
            d.parts = [at, by]
                .iter()
                .filter(|i| i.0 != 0)
                .map(|i| flat.instance(*i).path.clone())
                .collect();
            d.parts.dedup();
            if let Some(l) = &w.origin.label {
                d.detail.push(format!("the `when`: {l}"));
            }
            d.detail.push(format!("the limit: {} = {v}{unit}", fp.name));
            out.push(d);
        }
    }
    out
}
