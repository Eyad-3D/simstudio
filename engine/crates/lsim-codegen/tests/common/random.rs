//! Random flat expressions and random prepared models made of them, for
//! the differential tests of the generated code against the reference
//! interpreters (`lsim_ir::eval`, `lsim_ir::interval::enclose`).
#![allow(dead_code)]

use super::synth::{Builder, Rng};
use lsim_ir::expr::{BinaryOp, Builtin, CmpOp, Expr};
use lsim_ir::flat::VarId;
use lsim_ir::prepared::PreparedModel;
use lsim_ir::table::{Interpolation, Outside, TableData};

/// What an expression may read.
pub struct Leaves {
    pub vars: Vec<VarId>,
    pub params: Vec<Expr>,
    pub tables: Vec<(u32, usize)>,
    pub time: bool,
}

/// Which operations an expression may use.
#[derive(Clone, Copy)]
pub struct Ops {
    /// library calls (`sin`, `exp`, `pow` …); otherwise arithmetic only
    pub library: bool,
    /// constant powers (`x^3`, `x^0.5` …)
    pub powers: bool,
    /// tables
    pub tables: bool,
}

pub const ALL_OPS: Ops = Ops { library: true, powers: true, tables: true };

fn c(x: f64) -> Expr {
    Expr::Const(x)
}

/// A random constant: small integers, ordinary numbers, extremes and
/// signed zeros.
pub fn constant(r: &mut Rng) -> f64 {
    match r.below(12) {
        0 => 0.0,
        1 => -0.0,
        2 => 1.0,
        3 => -1.0,
        4 => (r.below(9) as f64) - 4.0,
        5 => r.range(-1e3, 1e3),
        6 => r.range(-1e-3, 1e-3),
        7 => [0.5, 2.0, 0.1, 3.0, 1e-9, 1e9][r.below(6)],
        _ => r.range(-3.0, 3.0),
    }
}

/// A random expression of at most `depth` levels.
pub fn expr(r: &mut Rng, depth: usize, l: &Leaves, ops: Ops) -> Expr {
    if depth == 0 || r.below(10) < 2 {
        return leaf(r, l);
    }
    let sub = |r: &mut Rng| expr(r, depth - 1, l, ops);
    let pick = r.below(30);
    match pick {
        0..=3 => Expr::bin(BinaryOp::Add, sub(r), sub(r)),
        4..=6 => Expr::bin(BinaryOp::Sub, sub(r), sub(r)),
        7..=9 => Expr::bin(BinaryOp::Mul, sub(r), sub(r)),
        10..=11 => Expr::bin(BinaryOp::Div, sub(r), sub(r)),
        12 => Expr::Neg(Box::new(sub(r))),
        13 if ops.powers => {
            let n = [2.0, 3.0, -1.0, 0.5, 4.0, 5.0, -2.0, 7.0, 16.0, -3.0, 2.5, 1.0, 0.0, 17.0]
                [r.below(14)];
            Expr::bin(BinaryOp::Pow, sub(r), c(n))
        }
        14 if ops.library => Expr::bin(BinaryOp::Pow, sub(r), sub(r)),
        15..=18 if ops.library => {
            let f = [
                Builtin::Sin,
                Builtin::Cos,
                Builtin::Tan,
                Builtin::Asin,
                Builtin::Acos,
                Builtin::Atan,
                Builtin::Sinh,
                Builtin::Cosh,
                Builtin::Tanh,
                Builtin::Exp,
                Builtin::Log,
            ][r.below(11)];
            Expr::Call(f, vec![sub(r)])
        }
        19 if ops.library => Expr::Call(Builtin::Atan2, vec![sub(r), sub(r)]),
        20 => Expr::Call([Builtin::Sqrt, Builtin::Abs, Builtin::Sign][r.below(3)], vec![sub(r)]),
        21 => Expr::Call([Builtin::Min, Builtin::Max][r.below(2)], vec![sub(r), sub(r)]),
        22 => Expr::Call(Builtin::Limit, vec![sub(r), sub(r), sub(r)]),
        23..=24 => {
            Expr::If(Box::new(cond(r, depth - 1, l, ops)), Box::new(sub(r)), Box::new(sub(r)))
        }
        25 => Expr::NoEvent(Box::new(sub(r))),
        26 if ops.tables && !l.tables.is_empty() => {
            let (k, dims) = l.tables[r.below(l.tables.len())];
            Expr::Table { table: k, args: (0..dims).map(|_| sub(r)).collect() }
        }
        27 => cond(r, depth - 1, l, ops),
        _ => Expr::bin(BinaryOp::Add, sub(r), leaf(r, l)),
    }
}

/// A random truth: comparisons and their combinations.
pub fn cond(r: &mut Rng, depth: usize, l: &Leaves, ops: Ops) -> Expr {
    let sub = |r: &mut Rng| expr(r, depth.saturating_sub(1), l, ops);
    let cmp = |r: &mut Rng| {
        let op = [CmpOp::Lt, CmpOp::Le, CmpOp::Gt, CmpOp::Ge][r.below(4)];
        // often a variable against something (the interval enclosures cut
        // a branch's variables by such comparisons)
        let a = if r.below(2) == 0 { leaf(r, l) } else { sub(r) };
        let b = if r.below(3) == 0 { leaf(r, l) } else { sub(r) };
        Expr::Compare(op, Box::new(a), Box::new(b))
    };
    match r.below(8) {
        0 => Expr::And(Box::new(cmp(r)), Box::new(cmp(r))),
        1 => Expr::Or(Box::new(cmp(r)), Box::new(cmp(r))),
        2 => Expr::Not(Box::new(cmp(r))),
        3 => Expr::NoEvent(Box::new(cmp(r))),
        _ => cmp(r),
    }
}

/// A random leaf.
pub fn leaf(r: &mut Rng, l: &Leaves) -> Expr {
    match r.below(10) {
        0..=4 if !l.vars.is_empty() => Expr::Var(l.vars[r.below(l.vars.len())]),
        5..=6 if !l.params.is_empty() => l.params[r.below(l.params.len())].clone(),
        7 if l.time => Expr::Time,
        _ => c(constant(r)),
    }
}

/// A random table (1-D or 2-D, cubic or linear, every outside rule).
pub fn table(r: &mut Rng, dims: usize) -> TableData {
    let axis = |r: &mut Rng, n: usize| -> Vec<f64> {
        let mut x = r.range(-3.0, 0.0);
        (0..n)
            .map(|_| {
                let v = x;
                x += r.range(0.05, 1.5);
                v
            })
            .collect()
    };
    let nx = 2 + r.below(6);
    let x = axis(r, nx);
    let mut data = if dims == 1 {
        let y: Vec<f64> = (0..nx).map(|_| r.range(-2.0, 2.0)).collect();
        TableData::new_1d(x, y)
    } else {
        let ny = 2 + r.below(5);
        let y = axis(r, ny);
        let v: Vec<f64> = (0..nx * ny).map(|_| r.range(-2.0, 2.0)).collect();
        TableData::new_2d(x, y, v)
    };
    if r.below(3) == 0 {
        data.interpolation = Interpolation::Linear;
    }
    let out = [Outside::Clamp, Outside::Linear, Outside::Error];
    data.outside = [out[r.below(3)], out[r.below(3)]];
    data
}

/// A random prepared model: `n_states` states, `n_params` parameters,
/// discrete variables, tables, then `n_assign` assignments (each a random
/// expression of what comes before it), the states' derivatives, and
/// `n_cross` zero crossings (each reading the assignments, the states
/// and time).
pub fn model(
    r: &mut Rng,
    n_states: usize,
    n_assign: usize,
    n_cross: usize,
    depth: usize,
    ops: Ops,
) -> PreparedModel {
    let mut b = Builder::new();
    let mut l = Leaves { vars: vec![], params: vec![], tables: vec![], time: true };
    let states: Vec<VarId> =
        (0..n_states).map(|i| b.state(&format!("x{i}"), r.range(-2.0, 2.0))).collect();
    l.vars.extend(&states);
    for i in 0..3 {
        l.params.push(b.param(&format!("p{i}"), r.range(-2.0, 2.0)));
    }
    for i in 0..2 {
        l.vars.push(b.discrete(&format!("d{i}"), r.range(-2.0, 2.0)));
    }
    if ops.tables {
        for i in 0..3 {
            let dims = 1 + (i % 2);
            let k = b.table(&format!("t{i}"), table(r, dims));
            l.tables.push((k, dims));
        }
    }
    for i in 0..n_assign {
        let e = expr(r, depth, &l, ops);
        let v = b.var(&format!("a{i}"));
        b.set(v, e);
        l.vars.push(v);
    }
    for &x in &states {
        let e = expr(r, depth, &l, ops);
        b.der(x, e);
    }
    for _ in 0..n_cross {
        let e = expr(r, depth, &l, ops);
        // (it reads time and a state, so the run loop checks it along steps)
        let e = Expr::bin(
            BinaryOp::Add,
            e,
            Expr::bin(BinaryOp::Mul, Expr::Time, Expr::Var(states[r.below(states.len())])),
        );
        b.crossing(e);
    }
    b.finish()
}
