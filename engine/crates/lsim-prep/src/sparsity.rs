//! The Jacobian's structure (DESIGN.md, *Preparation pipeline*, step 10):
//! which entries of `∂[x'; g]/∂y` can be non-zero, following each
//! assignment back to the states and iteration variables it reads. Both
//! branches of an `if` count, so the pattern holds in every mode.

use lsim_ir::expr::Expr;
use lsim_ir::prepared::{PreparedModel, Slot};
use lsim_ir::runtime::SparsityPattern;

const NO: u32 = u32::MAX;

struct Deps {
    /// per flat variable: index into `sets` of its assignment, for `Var`
    var: Vec<u32>,
    /// the same for `Der`
    der: Vec<u32>,
    /// per flat variable: its column in y as `Var`
    y_var: Vec<u32>,
    /// the same as `Der`
    y_der: Vec<u32>,
    sets: Vec<Vec<u32>>,
    mark: Vec<u32>,
    stamp: u32,
}

impl Deps {
    fn collect(&mut self, e: &Expr) -> Vec<u32> {
        self.stamp += 1;
        let stamp = self.stamp;
        let mut out = vec![];
        let add = |c: u32, mark: &mut Vec<u32>, out: &mut Vec<u32>| {
            if mark[c as usize] != stamp {
                mark[c as usize] = stamp;
                out.push(c);
            }
        };
        e.walk(&mut |x| {
            let (y, a) = match x {
                Expr::Var(v) => (self.y_var[v.0 as usize], self.var[v.0 as usize]),
                Expr::Der(v) => (self.y_der[v.0 as usize], self.der[v.0 as usize]),
                _ => return,
            };
            if y != NO {
                add(y, &mut self.mark, &mut out);
            } else if a != NO {
                for &c in &self.sets[a as usize] {
                    add(c, &mut self.mark, &mut out);
                }
            }
        });
        out
    }
}

/// The sparsity pattern of the model's Jacobian.
pub fn pattern(m: &PreparedModel) -> SparsityPattern {
    let n_v = m.flat.vars.len();
    let n_x = m.states.len();
    let n = n_x + m.algebraics.len();
    let mut d = Deps {
        var: vec![NO; n_v],
        der: vec![NO; n_v],
        y_var: vec![NO; n_v],
        y_der: vec![NO; n_v],
        sets: vec![],
        mark: vec![0; n],
        stamp: 0,
    };
    for (i, x) in m.states.iter().enumerate() {
        d.y_var[x.0 as usize] = i as u32;
    }
    for (k, s) in m.algebraics.iter().enumerate() {
        match s {
            Slot::Var(v) => d.y_var[v.0 as usize] = (n_x + k) as u32,
            Slot::Der(v) => d.y_der[v.0 as usize] = (n_x + k) as u32,
        }
    }
    for a in &m.assignments {
        let set = d.collect(&a.expr);
        let id = d.sets.len() as u32;
        d.sets.push(set);
        match a.target {
            Slot::Var(v) => d.var[v.0 as usize] = id,
            Slot::Der(v) => d.der[v.0 as usize] = id,
        }
    }
    let mut entries: Vec<(u32, u32)> = vec![];
    for (i, x) in m.states.iter().enumerate() {
        let row = d.collect(&Expr::Der(*x));
        entries.extend(row.into_iter().map(|c| (c, i as u32)));
    }
    for (k, r) in m.residuals.iter().enumerate() {
        let row = d.collect(&r.expr);
        entries.extend(row.into_iter().map(|c| (c, (n_x + k) as u32)));
    }
    entries.sort_unstable();
    let mut col_ptr = vec![0usize; n + 1];
    for &(c, _) in &entries {
        col_ptr[c as usize + 1] += 1;
    }
    for j in 0..n {
        col_ptr[j + 1] += col_ptr[j];
    }
    SparsityPattern { n, col_ptr, row_idx: entries.into_iter().map(|(_, r)| r as usize).collect() }
}
