//! Parameter values at run time: re-evaluating bindings and start values
//! after a parameter changes, without preparing again.
//!
//! Flattening lays the parameters out in binding order (each after every
//! parameter its binding reads), so one pass in order brings every bound
//! parameter up to date.

use lsim_ir::eval::{Env, eval};
use lsim_ir::expr::Expr;
use lsim_ir::flat::{FlatSystem, ParamId, VarId};

struct ParamsOnly<'a>(&'a [f64]);

impl Env for ParamsOnly<'_> {
    fn time(&self) -> f64 {
        f64::NAN
    }
    fn var(&self, _: VarId) -> f64 {
        f64::NAN
    }
    fn der(&self, _: VarId) -> f64 {
        f64::NAN
    }
    fn param(&self, p: ParamId) -> f64 {
        self.0[p.0 as usize]
    }
}

/// Re-evaluates every bound parameter of `flat` in `values` (one value per
/// parameter, the free ones already set).
pub fn rebind(flat: &FlatSystem, values: &mut [f64]) {
    for (i, p) in flat.params.iter().enumerate() {
        if let Some(b) = &p.binding {
            values[i] = eval(b, &ParamsOnly(values));
        }
    }
}

/// An expression of the parameters (a start value, a guess, a guard) at
/// the given parameter values.
pub fn value(e: &Expr, values: &[f64]) -> f64 {
    eval(e, &ParamsOnly(values))
}

/// Whether the bindings are in order: each reads only earlier parameters.
pub fn in_binding_order(flat: &FlatSystem) -> bool {
    flat.params.iter().enumerate().all(|(i, p)| {
        p.binding
            .as_ref()
            .is_none_or(|b| !b.any(&mut |x| matches!(x, Expr::Param(q) if q.0 as usize >= i)))
    })
}
