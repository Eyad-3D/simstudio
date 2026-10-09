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

use lsim_ir::VarKind;
use lsim_ir::expr::{BinaryOp, Expr};
use lsim_ir::flat::{FlatSystem, VarId};
use lsim_ir::prepared::{AliasEntry, AliasTarget};

/// The most terms an alias equation's linear form is read for: an alias
/// has two, and longer sums (a node's current balance) are no aliases.
const MAX_TERMS: usize = 8;

/// A few terms, on the stack.
struct Terms {
    t: [(VarId, f64); MAX_TERMS],
    n: usize,
}

impl Terms {
    fn push(&mut self, x: (VarId, f64)) -> bool {
        if self.n == MAX_TERMS {
            return false;
        }
        self.t[self.n] = x;
        self.n += 1;
        true
    }
}

/// A linear form: Σ coef·var + constant (terms unmerged).
fn linear(e: &Expr, scale: f64, terms: &mut Terms, k: &mut f64) -> bool {
    match e {
        Expr::Const(v) => {
            *k += scale * v;
            true
        }
        Expr::Var(v) => terms.push((*v, scale)),
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
    /// 0 = 0: the equation says nothing (a closed network's last balance,
    /// once its other balances made its currents aliases of each other)
    Identity,
}

fn classify(lhs: &Expr, rhs: &Expr) -> Option<Found> {
    let mut terms = Terms { t: [(VarId(0), 0.0); MAX_TERMS], n: 0 };
    let mut k = 0.0;
    if !(linear(lhs, 1.0, &mut terms, &mut k) && linear(rhs, -1.0, &mut terms, &mut k)) {
        return None;
    }
    let ts = &mut terms.t[..terms.n];
    ts.sort_by_key(|(v, _)| *v);
    let mut merged = Terms { t: [(VarId(0), 0.0); MAX_TERMS], n: 0 };
    for &(v, c) in ts.iter() {
        if merged.n > 0 && merged.t[merged.n - 1].0 == v {
            merged.t[merged.n - 1].1 += c;
        } else {
            merged.push((v, c));
        }
    }
    let mut t = [(VarId(0), 0.0); MAX_TERMS];
    let mut n = 0;
    for &(v, c) in &merged.t[..merged.n] {
        if c != 0.0 {
            t[n] = (v, c);
            n += 1;
        }
    }
    let t = &t[..n];
    match t {
        [(a, ca), (b, cb)] if k == 0.0 && ca.abs() == cb.abs() => {
            Some(Found::Pair(*a, *b, -cb / ca))
        }
        [(a, ca)] => Some(Found::Const(*a, -k / ca)),
        [] if k == 0.0 => Some(Found::Identity),
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

/// Removes aliases from `flat.equations` (and substitutes them in `whens`
/// and the energy expressions); returns the alias table.
pub fn eliminate(flat: &mut FlatSystem) -> Vec<AliasEntry> {
    let known = vec![false; flat.vars.len()];
    eliminate_with(flat, &known)
}

#[allow(clippy::needless_range_loop)] // several arrays indexed in step
/// [`eliminate`] with some variables `known` (an inverse model's
/// prescribed inputs): a known variable is kept in preference to any other
/// (a state equal to it is eliminated in its favour), two known variables
/// are never merged and a known variable never becomes a constant.
pub fn eliminate_with(flat: &mut FlatSystem, known: &[bool]) -> Vec<AliasEntry> {
    let n = flat.vars.len();
    let mut is_state = vec![false; n];
    for e in &flat.equations {
        for side in [&e.lhs, &e.rhs] {
            crate::walk::visit(side, &mut |x| {
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
            let found = classify(&e.lhs, &e.rhs);
            if let Some(Found::Identity) = found {
                // dropped: it removes no unknown, and keeping it would hide
                // the unknown it fails to decide (a circuit's missing
                // ground) behind a numerically singular block
                continue;
            }
            match found {
                Some(Found::Pair(a, b, s))
                    if !discrete[a.0 as usize] && !discrete[b.0 as usize] =>
                {
                    let (ra, sa) = cls.find(a.0 as usize);
                    let (rb, sb) = cls.find(b.0 as usize);
                    // a = s·b, a = sa·ra, b = sb·rb  →  ra = s·sa·sb·rb
                    let rel = s * sa * sb;
                    let (a_known, b_known) = (known[ra], known[rb]);
                    let (a_state, b_state) = (is_state[ra], is_state[rb]);
                    let (a_val, b_val) = (cls.value[ra].is_some(), cls.value[rb].is_some());
                    let merge = ra != rb
                        && !(a_known && b_known)
                        && !(a_known && b_val)
                        && !(b_known && a_val)
                        && (a_known || b_known || !(a_state && b_state))
                        && !(a_state && b_val)
                        && !(b_state && a_val);
                    if merge {
                        // keep the known input, else the constant, else the
                        // state, else the older variable
                        let keep_a = a_known
                            || (!b_known
                                && (a_val || (!b_val && (a_state || (!b_state && ra < rb)))));
                        let (keep_root, drop_root) = if keep_a { (ra, rb) } else { (rb, ra) };
                        cls.parent[drop_root] = keep_root;
                        cls.sign[drop_root] = rel; // ±1 is its own inverse
                        eliminated[drop_root] = true;
                        used = true;
                    }
                }
                Some(Found::Const(a, value)) if !discrete[a.0 as usize] => {
                    let (ra, sa) = cls.find(a.0 as usize);
                    if !is_state[ra] && !known[ra] && cls.value[ra].is_none() {
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
        // only what reads a replaced variable is rewritten
        let touched = |e: &Expr| {
            crate::walk::any(
                e,
                &mut |x| matches!(x, Expr::Var(v) | Expr::Der(v) if repl[v.0 as usize].is_some()),
            )
        };
        let sub = |mut e: Expr| {
            if !touched(&e) {
                return e;
            }
            crate::walk::mutate(&mut e, &mut |x| match *x {
                Expr::Var(v) => {
                    if let Some(r) = &repl[v.0 as usize] {
                        *x = r.clone();
                    }
                }
                Expr::Der(v) => match &repl[v.0 as usize] {
                    Some(Expr::Var(w)) => *x = Expr::Der(*w),
                    Some(Expr::Neg(w)) => {
                        if let Expr::Var(w) = &**w {
                            *x = -Expr::Der(*w);
                        }
                    }
                    _ => {}
                },
                _ => {}
            });
            crate::symbolic::simplify_mut(&mut e);
            e
        };
        for e in flat.equations.iter_mut().chain(flat.initial_equations.iter_mut()) {
            e.lhs = sub(std::mem::replace(&mut e.lhs, Expr::Const(0.0)));
            e.rhs = sub(std::mem::replace(&mut e.rhs, Expr::Const(0.0)));
        }
        for a in &mut flat.asserts {
            a.condition = sub(std::mem::replace(&mut a.condition, Expr::Const(0.0)));
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

/// A variable kept by alias elimination with no start value of its own
/// takes one from the variables eliminated in its favour, with their sign:
/// a fixed one first, else the first in the model's order. It is a guess
/// for the kept variable (preparation's start solve and the run's
/// initialisation start from it); a fixed start of an eliminated variable
/// stays a condition of the initialisation system on the kept one. So a
/// battery's terminal voltage guess reaches the bus voltage its node's port
/// carries, where `v·i = P` from 0 V would give no current (a singular
/// start). `start` holds the start expressions per variable.
pub fn carry_starts(flat: &mut FlatSystem, start: &mut [Option<Expr>], aliases: &[AliasEntry]) {
    let has_start = |flat: &FlatSystem, start: &[Option<Expr>], v: VarId| {
        start.get(v.0 as usize).is_some_and(|s| s.is_some()) || flat.var(v).start.is_some()
    };
    // per kept variable: (fixed, the alias, its sign)
    let mut best: std::collections::BTreeMap<u32, (bool, VarId, bool)> = Default::default();
    for a in aliases {
        let AliasTarget::Var { var, negated } = a.target else { continue };
        if has_start(flat, start, var) || !has_start(flat, start, a.var) {
            continue;
        }
        let fixed = flat.var(a.var).fixed;
        let better = match best.get(&var.0) {
            None => true,
            Some((f, w, _)) => (fixed && !f) || (fixed == *f && a.var < *w),
        };
        if better {
            best.insert(var.0, (fixed, a.var, negated));
        }
    }
    for (kept, (_, from, negated)) in best {
        let kept = kept as usize;
        let i = from.0 as usize;
        let expr = start
            .get(i)
            .cloned()
            .flatten()
            .or_else(|| flat.vars[i].start.map(Expr::Const))
            .expect("a start");
        let value = flat.vars[i].start;
        if kept < start.len() {
            start[kept] = Some(if negated { -expr } else { expr });
        }
        flat.vars[kept].start = value.map(|v| if negated { -v } else { v });
    }
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
