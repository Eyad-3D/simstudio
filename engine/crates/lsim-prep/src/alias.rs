//! Alias elimination.
//!
//! Connections and component boilerplate produce many equations of the
//! forms `a = b`, `a = -b`, `a + b = 0` and `a = constant`. Each removes one
//! variable and one equation: the variable is replaced everywhere by the
//! one it equals (with sign) and is still recorded under its own name.
//! States are kept in preference to other variables; two states are never
//! merged (that equation is a constraint for index reduction) and a state
//! is never replaced by a constant. Repeated until nothing changes, since
//! substituting constants creates new aliases (`v = p.v - n.v` with
//! `n.v = 0`).

use crate::symbolic::simplify;
use lsim_ir::VarKind;
use lsim_ir::expr::{BinaryOp, Expr};
use lsim_ir::flat::{FlatSystem, VarId};
use lsim_ir::prepared::{AliasEntry, AliasTarget};
use std::collections::HashMap;

/// A linear form: Σ coef·var + constant.
fn linear(e: &Expr, scale: f64, terms: &mut HashMap<VarId, f64>, k: &mut f64) -> bool {
    match e {
        Expr::Const(v) => {
            *k += scale * v;
            true
        }
        Expr::Var(v) => {
            *terms.entry(*v).or_insert(0.0) += scale;
            true
        }
        Expr::Neg(a) => linear(a, -scale, terms, k),
        Expr::Binary(BinaryOp::Add, a, b) => {
            linear(a, scale, terms, k) && linear(b, scale, terms, k)
        }
        Expr::Binary(BinaryOp::Sub, a, b) => {
            linear(a, scale, terms, k) && linear(b, -scale, terms, k)
        }
        Expr::Binary(BinaryOp::Mul, a, b) => match (&**a, &**b) {
            (Expr::Const(c), x) | (x, Expr::Const(c)) => linear(x, scale * c, terms, k),
            _ => false,
        },
        _ => false,
    }
}

enum Found {
    /// a = sign · b
    Pair(VarId, VarId, f64),
    /// a = value
    Const(VarId, f64),
}

fn classify(lhs: &Expr, rhs: &Expr) -> Option<Found> {
    let mut terms = HashMap::new();
    let mut k = 0.0;
    if !(linear(lhs, 1.0, &mut terms, &mut k) && linear(rhs, -1.0, &mut terms, &mut k)) {
        return None;
    }
    terms.retain(|_, c| *c != 0.0);
    let mut t: Vec<(VarId, f64)> = terms.into_iter().collect();
    t.sort_by_key(|(v, _)| *v);
    match t.as_slice() {
        [(a, ca), (b, cb)] if k == 0.0 && ca.abs() == cb.abs() => {
            Some(Found::Pair(*a, *b, -cb / ca))
        }
        [(a, ca)] => Some(Found::Const(*a, -k / ca)),
        _ => None,
    }
}

/// Union-find with signs: var = sign · root, or var = constant.
struct Classes {
    parent: Vec<usize>,
    sign: Vec<f64>,
    value: Vec<Option<f64>>,
}

impl Classes {
    fn find(&mut self, i: usize) -> (usize, f64) {
        let mut s = 1.0;
        let mut r = i;
        while self.parent[r] != r {
            s *= self.sign[r];
            r = self.parent[r];
        }
        // compress
        let mut j = i;
        let mut sj = s;
        while self.parent[j] != j {
            let next = self.parent[j];
            let snext = sj * self.sign[j];
            self.parent[j] = r;
            self.sign[j] = sj;
            j = next;
            sj = snext;
        }
        (r, s)
    }
}

#[allow(clippy::needless_range_loop)] // several arrays indexed in step
/// Removes aliases from `flat.equations` (and substitutes them in `whens`
/// and the energy expressions); returns the alias table.
pub fn eliminate(flat: &mut FlatSystem) -> Vec<AliasEntry> {
    let n = flat.vars.len();
    let mut is_state = vec![false; n];
    for e in &flat.equations {
        for side in [&e.lhs, &e.rhs] {
            side.walk(&mut |x| {
                if let Expr::Der(v) = x {
                    is_state[v.0 as usize] = true;
                }
            });
        }
    }
    let discrete: Vec<bool> = flat.vars.iter().map(|v| v.kind == VarKind::Discrete).collect();
    let mut cls = Classes { parent: (0..n).collect(), sign: vec![1.0; n], value: vec![None; n] };
    let mut eliminated = vec![false; n];

    loop {
        let mut changed = false;
        let mut keep = Vec::with_capacity(flat.equations.len());
        for e in std::mem::take(&mut flat.equations) {
            let mut used = false;
            match classify(&e.lhs, &e.rhs) {
                Some(Found::Pair(a, b, s))
                    if !discrete[a.0 as usize] && !discrete[b.0 as usize] =>
                {
                    let (ra, sa) = cls.find(a.0 as usize);
                    let (rb, sb) = cls.find(b.0 as usize);
                    // a = s·b, a = sa·ra, b = sb·rb  →  ra = s·sa·sb·rb
                    let rel = s * sa * sb;
                    let (a_state, b_state) = (is_state[ra], is_state[rb]);
                    let (a_val, b_val) = (cls.value[ra].is_some(), cls.value[rb].is_some());
                    let merge = ra != rb
                        && !(a_state && b_state)
                        && !(a_val && b_val)
                        && !(a_state && b_val)
                        && !(b_state && a_val);
                    if merge {
                        // keep the constant, else the state, else the older variable
                        let keep_a = a_val || (!b_val && (a_state || (!b_state && ra < rb)));
                        let (keep_root, drop_root) = if keep_a { (ra, rb) } else { (rb, ra) };
                        cls.parent[drop_root] = keep_root;
                        cls.sign[drop_root] = rel; // ±1 is its own inverse
                        eliminated[drop_root] = true;
                        used = true;
                    }
                }
                Some(Found::Const(a, value)) if !discrete[a.0 as usize] => {
                    let (ra, sa) = cls.find(a.0 as usize);
                    if !is_state[ra] && cls.value[ra].is_none() {
                        cls.value[ra] = Some(value * sa);
                        eliminated[ra] = true;
                        used = true;
                    }
                }
                _ => {}
            }
            if used {
                changed = true;
            } else {
                keep.push(e);
            }
        }
        flat.equations = keep;
        if !changed {
            break;
        }
        // substitute the classes found so far
        let mut repl: Vec<Option<Expr>> = vec![None; n];
        for i in 0..n {
            let (r, s) = cls.find(i);
            let target = match cls.value[r] {
                Some(v) => Some(Expr::Const(s * v)),
                None if r != i => Some(if s < 0.0 {
                    -Expr::Var(VarId(r as u32))
                } else {
                    Expr::Var(VarId(r as u32))
                }),
                None => None,
            };
            repl[i] = target;
        }
        let sub = |e: Expr| {
            simplify(e.rewrite(&mut |x| match x {
                Expr::Var(v) => repl[v.0 as usize].clone().unwrap_or(Expr::Var(v)),
                Expr::Der(v) => match &repl[v.0 as usize] {
                    Some(Expr::Var(w)) => Expr::Der(*w),
                    Some(Expr::Neg(w)) => match &**w {
                        Expr::Var(w) => -Expr::Der(*w),
                        _ => Expr::Der(v),
                    },
                    _ => Expr::Der(v),
                },
                other => other,
            }))
        };
        for e in &mut flat.equations {
            e.lhs = sub(std::mem::replace(&mut e.lhs, Expr::Const(0.0)));
            e.rhs = sub(std::mem::replace(&mut e.rhs, Expr::Const(0.0)));
        }
        for w in &mut flat.whens {
            w.condition = sub(std::mem::replace(&mut w.condition, Expr::Const(0.0)));
            for (_, v) in w.assign.iter_mut().chain(w.reinit.iter_mut()) {
                *v = sub(std::mem::replace(v, Expr::Const(0.0)));
            }
        }
        for en in &mut flat.energy {
            for x in [&mut en.stored, &mut en.loss].into_iter().flatten() {
                *x = sub(std::mem::replace(x, Expr::Const(0.0)));
            }
        }
        for pp in &mut flat.port_powers {
            pp.power = sub(std::mem::replace(&mut pp.power, Expr::Const(0.0)));
        }
    }

    let mut table = vec![];
    for i in 0..n {
        if !eliminated[i] && cls.parent[i] == i {
            continue;
        }
        let (r, s) = cls.find(i);
        let target = match cls.value[r] {
            Some(v) => AliasTarget::Const(s * v),
            None => AliasTarget::Var { var: VarId(r as u32), negated: s < 0.0 },
        };
        if r == i && cls.value[r].is_none() {
            continue;
        }
        table.push(AliasEntry { var: VarId(i as u32), target });
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flatten::flatten;
    use lsim_ir::ComponentDef;
    use lsim_ir::component::build::{connect, sub};
    use lsim_ir::expr::c;

    #[test]
    fn a_divider_keeps_few_unknowns() {
        let lib = lsim_lib::library();
        let top = ComponentDef {
            name: "Divider".into(),
            components: vec![
                sub("src", "Electrical.ConstantVoltage", &[("V", c(10.0))]),
                sub("r1", "Electrical.Resistor", &[("R", c(1.0))]),
                sub("r2", "Electrical.Resistor", &[("R", c(3.0))]),
                sub("gnd", "Electrical.Ground", &[]),
            ],
            connections: vec![
                connect("src.p", "r1.p"),
                connect("r1.n", "r2.p"),
                connect("r2.n", "src.n"),
                connect("src.n", "gnd.p"),
            ],
            ..Default::default()
        };
        let mut flat = flatten(&lib, &top).unwrap();
        let before = flat.equations.len();
        let table = eliminate(&mut flat);
        let after = flat.equations.len();
        assert_eq!(before - after, table.len());
        // what remains: one current, two node voltages' worth of equations
        assert!(after <= 5, "{after} equations left: {:#?}", flat.equations);
        // ground's potential is the constant 0
        let g = flat.find_var("gnd.p.v").unwrap();
        assert!(table.iter().any(|a| a.var == g && a.target == AliasTarget::Const(0.0)));
    }
}
