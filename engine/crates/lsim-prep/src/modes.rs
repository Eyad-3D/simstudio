//! Modes of `if` relations (DESIGN.md, *Events and modes*).
//!
//! A relation in an equation (`if w > 0 then … else …`, the sign test of
//! `abs` and `sign`) outside `noEvent` would make the right-hand side
//! jump inside a step. Each becomes a *mode*: a discrete variable that
//! holds the relation's value between events, read by the equation in its
//! place, with the relation's zero-crossing function (positive where the
//! relation holds: [`crossing`]) to locate where it changes. Equal relations share one mode. Relations of
//! parameters and discrete variables only change at events anyway and are
//! left as they are; so is everything under `noEvent`.

use lsim_ir::component::VarKind;
use lsim_ir::expr::{Builtin, CmpOp, Expr};
use lsim_ir::flat::{FlatSystem, FlatVar, Origin, VarId, VarRole};
use lsim_ir::units::Unit;
use std::collections::HashMap;

/// A mode, in flat scope.
#[derive(Clone, Debug)]
pub struct FlatMode {
    /// its discrete variable (appended to the flat system)
    pub var: VarId,
    /// the relation
    pub relation: Expr,
    /// where it first appeared
    pub origin: Origin,
}

struct Ctx<'a> {
    flat: &'a mut FlatSystem,
    discrete: Vec<bool>,
    modes: Vec<FlatMode>,
    by_text: HashMap<String, VarId>,
    count: HashMap<u32, usize>,
}

impl Ctx<'_> {
    fn continuous(&self, e: &Expr) -> bool {
        e.any(&mut |x| match x {
            Expr::Var(v) => !self.discrete.get(v.0 as usize).copied().unwrap_or(false),
            Expr::Der(_) | Expr::Time => true,
            _ => false,
        })
    }

    fn mode(&mut self, relation: Expr, origin: &Origin) -> Expr {
        let key = relation.to_string();
        if let Some(v) = self.by_text.get(&key) {
            return Expr::Var(*v);
        }
        let inst = origin.instance;
        let k = self.count.entry(inst.0).or_insert(0);
        *k += 1;
        let path = &self.flat.instance(inst).path;
        let name = if path.is_empty() { format!("mode[{k}]") } else { format!("{path}.mode[{k}]") };
        self.flat.vars.push(FlatVar {
            name,
            unit: Unit::ONE,
            unit_text: "1".into(),
            kind: VarKind::Discrete,
            start: Some(0.0),
            fixed: false,
            nominal: 1.0,
            instance: inst,
            role: VarRole::Local,
        });
        let v = VarId(self.flat.vars.len() as u32 - 1);
        self.discrete.push(true);
        self.modes.push(FlatMode { var: v, relation, origin: origin.clone() });
        self.by_text.insert(key, v);
        Expr::Var(v)
    }

    fn rewrite(&mut self, e: Expr, origin: &Origin) -> Expr {
        let b = Box::new;
        match e {
            Expr::NoEvent(_) => e,
            Expr::Compare(op, x, y) => {
                let x = self.rewrite(*x, origin);
                let y = self.rewrite(*y, origin);
                let rel = Expr::Compare(op, b(x), b(y));
                if self.continuous(&rel) { self.mode(rel, origin) } else { rel }
            }
            Expr::Call(Builtin::Abs, args) if args.len() == 1 => {
                let x = self.rewrite(args.into_iter().next().expect("one argument"), origin);
                if !self.continuous(&x) {
                    return Expr::Call(Builtin::Abs, vec![x]);
                }
                let m =
                    self.mode(Expr::Compare(CmpOp::Ge, b(x.clone()), b(Expr::Const(0.0))), origin);
                Expr::If(b(m), b(x.clone()), b(Expr::Neg(b(x))))
            }
            Expr::Call(Builtin::Sign, args) if args.len() == 1 => {
                let x = self.rewrite(args.into_iter().next().expect("one argument"), origin);
                if !self.continuous(&x) {
                    return Expr::Call(Builtin::Sign, vec![x]);
                }
                let pos =
                    self.mode(Expr::Compare(CmpOp::Gt, b(x.clone()), b(Expr::Const(0.0))), origin);
                let neg = self.mode(Expr::Compare(CmpOp::Lt, b(x), b(Expr::Const(0.0))), origin);
                Expr::If(
                    b(pos),
                    b(Expr::Const(1.0)),
                    b(Expr::If(b(neg), b(Expr::Const(-1.0)), b(Expr::Const(0.0)))),
                )
            }
            Expr::Neg(a) => Expr::Neg(b(self.rewrite(*a, origin))),
            Expr::Not(a) => Expr::Not(b(self.rewrite(*a, origin))),
            Expr::Binary(op, x, y) => {
                Expr::Binary(op, b(self.rewrite(*x, origin)), b(self.rewrite(*y, origin)))
            }
            Expr::And(x, y) => Expr::And(b(self.rewrite(*x, origin)), b(self.rewrite(*y, origin))),
            Expr::Or(x, y) => Expr::Or(b(self.rewrite(*x, origin)), b(self.rewrite(*y, origin))),
            Expr::If(c, x, y) => Expr::If(
                b(self.rewrite(*c, origin)),
                b(self.rewrite(*x, origin)),
                b(self.rewrite(*y, origin)),
            ),
            Expr::Call(f, args) => {
                Expr::Call(f, args.into_iter().map(|a| self.rewrite(a, origin)).collect())
            }
            Expr::Table { table, args } => Expr::Table {
                table,
                args: args.into_iter().map(|a| self.rewrite(a, origin)).collect(),
            },
            leaf => leaf,
        }
    }
}

/// Replaces the event relations of `flat.equations` by mode variables
/// (appended to `flat.vars`); returns the modes.
pub fn extract(flat: &mut FlatSystem) -> Vec<FlatMode> {
    let discrete = flat.vars.iter().map(|v| v.kind == VarKind::Discrete).collect();
    let mut eqs = std::mem::take(&mut flat.equations);
    let mut ctx =
        Ctx { flat, discrete, modes: vec![], by_text: HashMap::new(), count: HashMap::new() };
    for e in &mut eqs {
        let origin = e.origin.clone();
        e.lhs = ctx.rewrite(std::mem::replace(&mut e.lhs, Expr::Const(0.0)), &origin);
        e.rhs = ctx.rewrite(std::mem::replace(&mut e.rhs, Expr::Const(0.0)), &origin);
    }
    let modes = ctx.modes;
    flat.equations = eqs;
    modes
}

/// A relation's zero-crossing function, positive where the relation
/// holds and negative where it does not: `lhs - rhs` for `>` and `>=`,
/// `rhs - lhs` for `<` and `<=` (the shared contract of
/// [`lsim_ir::Mode::crossing`]). So rising through zero makes the relation
/// true and falling makes it false, whatever its operator.
pub fn crossing(relation: &Expr) -> Expr {
    let simplify = crate::symbolic::simplify;
    match relation {
        Expr::Compare(CmpOp::Gt | CmpOp::Ge, a, b) => simplify((**a).clone() - (**b).clone()),
        Expr::Compare(CmpOp::Lt | CmpOp::Le, a, b) => simplify((**b).clone() - (**a).clone()),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsim_ir::expr::{c, cmp};

    #[test]
    fn a_crossing_is_positive_where_its_relation_holds() {
        let w = Expr::Var(VarId(0));
        for (op, at_two) in
            [(CmpOp::Gt, 1.0), (CmpOp::Ge, 1.0), (CmpOp::Lt, -1.0), (CmpOp::Le, -1.0)]
        {
            let f = crossing(&cmp(op, w.clone(), c(1.0)));
            let env = lsim_ir::eval::SliceEnv { t: 0.0, vars: &[2.0], ders: &[0.0], params: &[] };
            // w = 2: `w > 1` holds, `w < 1` does not
            assert_eq!(lsim_ir::eval::eval(&f, &env).signum(), at_two, "{op:?}: {f}");
        }
    }
}
