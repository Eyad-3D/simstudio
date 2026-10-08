//! What the code generator works out before emitting any code: where each
//! value lives, which assignments each function needs, the structural
//! dependence of every value on `y` (the Jacobian's sparsity), its column
//! colouring, and the table reads the run loop watches.

use crate::CodegenError;
use lsim_ir::expr::{BinaryOp, Builtin, Expr};
use lsim_ir::prepared::{PreparedModel, Slot};
use lsim_ir::runtime::SparsityPattern;
use lsim_ir::{Outside, TableGuard, VarId};
use std::collections::HashMap;

/// Where a referenced value comes from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Src {
    /// `y[i]` (an unknown of the system: it has a tangent)
    Y(usize),
    /// the k-th assignment of the system
    Work(usize),
    /// `d[k]`
    D(usize),
    /// `u[k]`
    U(usize),
    /// a fixed number (a start value the initialisation leaves alone)
    Const(f64),
}

/// An output row of a system's residual: `[x'; g]` for the model, the
/// initialisation's residuals for the initialisation.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Row<'m> {
    /// the value of a slot (a state's derivative)
    Slot(Slot),
    /// a residual expression
    Expr(&'m Expr),
}

/// Model-wide facts every system shares.
pub(crate) struct Ctx<'m> {
    pub model: &'m PreparedModel,
    pub d_index: HashMap<u32, usize>,
    pub u_index: HashMap<u32, usize>,
    /// each table's number of arguments
    pub table_dims: Vec<usize>,
}

impl<'m> Ctx<'m> {
    pub(crate) fn new(model: &'m PreparedModel) -> Result<Self, CodegenError> {
        let d_index = model.discretes.iter().enumerate().map(|(k, v)| (v.0, k)).collect();
        let u_index = model.inputs.iter().enumerate().map(|(k, v)| (v.0, k)).collect();
        let table_dims = model.flat.tables.iter().map(|t| t.data.dims()).collect();
        Ok(Ctx { model, d_index, u_index, table_dims })
    }
}

/// A system of the model: unknowns `y`, explicit assignments, output rows.
pub(crate) struct System<'m> {
    /// "the model" or "the initialisation", for messages
    pub what: &'static str,
    pub y_index: HashMap<Slot, usize>,
    pub n_y: usize,
    pub exprs: Vec<&'m Expr>,
    pub targets: Vec<Slot>,
    pub target_index: HashMap<Slot, usize>,
    pub rows: Vec<Row<'m>>,
    /// whether a variable that is neither unknown nor computed holds its
    /// start value (the initialisation) or is an error (the model)
    pub start_fallback: bool,
    /// per assignment: the earlier assignments it reads
    pub refs: Vec<Vec<usize>>,
    /// per assignment: the entries of y it depends on (sorted)
    pub ydeps: Vec<Vec<u32>>,
    /// per assignment: whether its code calls out (math library, tables)
    pub calls: Vec<bool>,
    /// per assignment: its size in expression nodes
    pub size: Vec<usize>,
    /// where each flat variable's value comes from (by index)
    var_src: Vec<Option<Src>>,
    /// where each flat variable's derivative comes from
    der_src: Vec<Option<Src>>,
}

impl<'m> System<'m> {
    pub(crate) fn new(
        cx: &Ctx<'m>,
        what: &'static str,
        unknowns: &[Slot],
        assigns: &'m [lsim_ir::Assignment],
        rows: Vec<Row<'m>>,
        start_fallback: bool,
    ) -> Result<Self, CodegenError> {
        let y_index: HashMap<Slot, usize> =
            unknowns.iter().enumerate().map(|(i, s)| (*s, i)).collect();
        let mut sys = System {
            what,
            n_y: unknowns.len(),
            y_index,
            exprs: assigns.iter().map(|a| &a.expr).collect(),
            targets: assigns.iter().map(|a| a.target).collect(),
            target_index: HashMap::with_capacity(assigns.len()),
            rows,
            start_fallback,
            refs: Vec::with_capacity(assigns.len()),
            ydeps: Vec::with_capacity(assigns.len()),
            calls: Vec::with_capacity(assigns.len()),
            size: Vec::with_capacity(assigns.len()),
            var_src: vec![],
            der_src: vec![],
        };
        for (k, a) in assigns.iter().enumerate() {
            if sys.target_index.insert(a.target, k).is_some() || sys.y_index.contains_key(&a.target)
            {
                return Err(CodegenError::Unsupported(format!(
                    "{} computes {:?} twice",
                    what, a.target
                )));
            }
        }
        let n_vars = cx.model.flat.vars.len();
        sys.var_src = vec![None; n_vars];
        sys.der_src = vec![None; n_vars];
        for (s, &i) in &sys.y_index {
            match s {
                Slot::Var(v) => sys.var_src.get_mut(v.0 as usize).map(|x| *x = Some(Src::Y(i))),
                Slot::Der(v) => sys.der_src.get_mut(v.0 as usize).map(|x| *x = Some(Src::Y(i))),
            };
        }
        for (s, &k) in &sys.target_index {
            match s {
                Slot::Var(v) => sys.var_src.get_mut(v.0 as usize).map(|x| *x = Some(Src::Work(k))),
                Slot::Der(v) => sys.der_src.get_mut(v.0 as usize).map(|x| *x = Some(Src::Work(k))),
            };
        }
        // discrete variables and inputs are read as such wherever they are
        for (&v, &k) in &cx.u_index {
            if let Some(x) = sys.var_src.get_mut(v as usize) {
                *x = Some(Src::U(k));
            }
        }
        for (&v, &k) in &cx.d_index {
            if let Some(x) = sys.var_src.get_mut(v as usize) {
                *x = Some(Src::D(k));
            }
        }
        for a in assigns {
            check_expr(cx, &a.expr)?;
            let mut refs = vec![];
            sys.collect_refs(cx, &a.expr, &mut refs)?;
            refs.sort_unstable();
            refs.dedup();
            let deps = sys.deps(cx, &a.expr)?;
            sys.refs.push(refs);
            sys.ydeps.push(deps);
            sys.calls.push(has_call(&a.expr));
            sys.size.push(a.expr.size());
        }
        for r in &sys.rows {
            match r {
                Row::Slot(s) => {
                    sys.slot_src(*s)?;
                }
                Row::Expr(e) => check_expr(cx, e)?,
            }
        }
        Ok(sys)
    }

    /// Where a slot's value comes from, if it is an unknown or computed.
    fn slot_src(&self, s: Slot) -> Result<Src, CodegenError> {
        if let Some(&i) = self.y_index.get(&s) {
            return Ok(Src::Y(i));
        }
        if let Some(&k) = self.target_index.get(&s) {
            return Ok(Src::Work(k));
        }
        Err(CodegenError::Unsupported(format!(
            "{s:?} is used in {} but neither solved for nor computed (a preparation bug)",
            self.what
        )))
    }

    /// Where a reference reads from. `der`: the derivative of `v`; `pre`:
    /// the value before the event (discrete variables).
    #[inline]
    pub(crate) fn resolve(&self, cx: &Ctx<'_>, v: VarId, der: bool) -> Result<Src, CodegenError> {
        let table = if der { &self.der_src } else { &self.var_src };
        if let Some(Some(s)) = table.get(v.0 as usize) {
            return Ok(*s);
        }
        if self.start_fallback && !der {
            let fv = cx.model.flat.vars.get(v.0 as usize);
            return Ok(Src::Const(fv.and_then(|f| f.start).unwrap_or(0.0)));
        }
        let slot = if der { Slot::Der(v) } else { Slot::Var(v) };
        self.slot_src(slot)
    }

    /// The source of an output row's slot.
    pub(crate) fn row_src(&self, s: Slot) -> Result<Src, CodegenError> {
        self.slot_src(s)
    }

    fn collect_refs(
        &self,
        cx: &Ctx<'_>,
        e: &Expr,
        out: &mut Vec<usize>,
    ) -> Result<(), CodegenError> {
        let k_now = self.refs.len();
        let mut err = None;
        e.walk(&mut |x| {
            let r = match x {
                Expr::Var(v) | Expr::Pre(v) => self.resolve(cx, *v, false),
                Expr::Der(v) => self.resolve(cx, *v, true),
                _ => return,
            };
            match r {
                Ok(Src::Work(k)) => {
                    if k >= k_now && err.is_none() {
                        err = Some(CodegenError::Unsupported(format!(
                            "{} uses {:?} before it is computed (an ordering bug)",
                            self.what, self.targets[k]
                        )));
                    }
                    out.push(k)
                }
                Ok(_) => {}
                Err(e) => {
                    if err.is_none() {
                        err = Some(e)
                    }
                }
            }
        });
        match err {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// The assignments `e` reads.
    pub(crate) fn refs_of(&self, cx: &Ctx<'_>, e: &Expr) -> Result<Vec<usize>, CodegenError> {
        let mut out = vec![];
        let mut err = None;
        e.walk(&mut |x| {
            let r = match x {
                Expr::Var(v) | Expr::Pre(v) => self.resolve(cx, *v, false),
                Expr::Der(v) => self.resolve(cx, *v, true),
                _ => return,
            };
            match r {
                Ok(Src::Work(k)) => out.push(k),
                Ok(_) => {}
                Err(e) => {
                    if err.is_none() {
                        err = Some(e)
                    }
                }
            }
        });
        if let Some(e) = err {
            return Err(e);
        }
        out.sort_unstable();
        out.dedup();
        Ok(out)
    }

    /// The entries of `y` that `e` depends on structurally: exactly the
    /// directions the generated tangent code can make non-zero.
    pub(crate) fn deps(&self, cx: &Ctx<'_>, e: &Expr) -> Result<Vec<u32>, CodegenError> {
        Ok(match e {
            Expr::Const(_) | Expr::Time | Expr::Param(_) | Expr::Name(_) => vec![],
            Expr::Var(v) | Expr::Pre(v) | Expr::Der(v) => {
                match self.resolve(cx, *v, matches!(e, Expr::Der(_)))? {
                    Src::Y(i) => vec![i as u32],
                    Src::Work(k) => self.ydeps.get(k).cloned().unwrap_or_default(),
                    _ => vec![],
                }
            }
            Expr::Neg(a) | Expr::NoEvent(a) => self.deps(cx, a)?,
            Expr::Binary(op, a, b) => {
                if *op == BinaryOp::Pow && matches!(**b, Expr::Const(_)) {
                    self.deps(cx, a)?
                } else {
                    union(&self.deps(cx, a)?, &self.deps(cx, b)?)
                }
            }
            Expr::Call(f, args) => match f {
                Builtin::Sign | Builtin::Der | Builtin::Pre => vec![],
                Builtin::Atan2 | Builtin::Min | Builtin::Max | Builtin::Limit => {
                    let mut acc = vec![];
                    for a in args {
                        acc = union(&acc, &self.deps(cx, a)?);
                    }
                    acc
                }
                _ => match args.first() {
                    Some(a) => self.deps(cx, a)?,
                    None => vec![],
                },
            },
            Expr::Compare(..) | Expr::And(..) | Expr::Or(..) | Expr::Not(_) => vec![],
            Expr::If(_, a, b) => union(&self.deps(cx, a)?, &self.deps(cx, b)?),
            Expr::Table { args, .. } => {
                let mut acc = vec![];
                for a in args {
                    acc = union(&acc, &self.deps(cx, a)?);
                }
                acc
            }
        })
    }

    /// The structural dependence of each output row on `y`.
    pub(crate) fn row_deps(&self, cx: &Ctx<'_>) -> Result<Vec<Vec<u32>>, CodegenError> {
        self.rows
            .iter()
            .map(|r| match r {
                Row::Slot(s) => Ok(match self.row_src(*s)? {
                    Src::Y(i) => vec![i as u32],
                    Src::Work(k) => self.ydeps[k].clone(),
                    _ => vec![],
                }),
                Row::Expr(e) => self.deps(cx, e),
            })
            .collect()
    }

    /// Every assignment needed to evaluate expressions reading `roots`
    /// (assignment indices), in evaluation order.
    pub(crate) fn closure(&self, roots: impl IntoIterator<Item = usize>) -> Vec<usize> {
        let n = self.exprs.len();
        let mut need = vec![false; n];
        for r in roots {
            need[r] = true;
        }
        for k in (0..n).rev() {
            if need[k] {
                for &j in &self.refs[k] {
                    need[j] = true;
                }
            }
        }
        (0..n).filter(|&k| need[k]).collect()
    }
}

fn union(a: &[u32], b: &[u32]) -> Vec<u32> {
    if a.is_empty() {
        return b.to_vec();
    }
    if b.is_empty() {
        return a.to_vec();
    }
    let mut out = Vec::with_capacity(a.len() + b.len());
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Less => {
                out.push(a[i]);
                i += 1;
            }
            std::cmp::Ordering::Greater => {
                out.push(b[j]);
                j += 1;
            }
            std::cmp::Ordering::Equal => {
                out.push(a[i]);
                i += 1;
                j += 1;
            }
        }
    }
    out.extend_from_slice(&a[i..]);
    out.extend_from_slice(&b[j..]);
    out
}

/// Whether the integer power `n` is computed inline (by multiplication).
pub(crate) fn inline_power(n: f64) -> bool {
    n == 0.0 || n == 1.0 || n == 2.0 || n == -1.0 || n == 0.5 || {
        n.fract() == 0.0 && (3.0..=16.0).contains(&n.abs())
    }
}

/// Whether evaluating `e` calls out of the generated code.
pub(crate) fn has_call(e: &Expr) -> bool {
    e.any(&mut |x| match x {
        Expr::Call(f, _) => !matches!(
            f,
            Builtin::Sqrt
                | Builtin::Abs
                | Builtin::Sign
                | Builtin::Min
                | Builtin::Max
                | Builtin::Limit
                | Builtin::Der
                | Builtin::Pre
        ),
        Expr::Binary(BinaryOp::Pow, _, b) => match **b {
            Expr::Const(n) => !inline_power(n),
            _ => true,
        },
        Expr::Table { .. } => true,
        _ => false,
    })
}

/// Rejects what the generated code cannot evaluate, with a message.
fn check_expr(cx: &Ctx<'_>, e: &Expr) -> Result<(), CodegenError> {
    let mut err = None;
    e.walk(&mut |x| {
        if err.is_some() {
            return;
        }
        match x {
            Expr::Name(n) => {
                err = Some(CodegenError::Unsupported(format!("unresolved name '{n}'")));
            }
            Expr::Param(p) if p.0 as usize >= cx.model.flat.params.len() => {
                err = Some(CodegenError::Unsupported(format!(
                    "parameter {} is used but the model has {}",
                    p.0,
                    cx.model.flat.params.len()
                )));
            }
            Expr::Call(Builtin::Der | Builtin::Pre, _) => {
                err = Some(CodegenError::Unsupported(
                    "der/pre in component scope (unflattened)".into(),
                ));
            }
            Expr::Call(f, args) if args.len() != f.arity() => {
                err = Some(CodegenError::Unsupported(format!(
                    "{} takes {} arguments, given {}",
                    f.name(),
                    f.arity(),
                    args.len()
                )));
            }
            Expr::Table { table, args } => match cx.table_dims.get(*table as usize) {
                None => {
                    err = Some(CodegenError::Unsupported(format!(
                        "table {table} is used but the model has {} tables",
                        cx.table_dims.len()
                    )));
                }
                Some(&d) if d != args.len() => {
                    let name = &cx.model.flat.tables[*table as usize].name;
                    err = Some(CodegenError::Unsupported(format!(
                        "table '{name}' has {d} axes but is read with {} arguments",
                        args.len()
                    )));
                }
                _ => {}
            },
            _ => {}
        }
    });
    match err {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// The column-compressed pattern of rows depending on columns.
pub(crate) fn pattern_from_rows(n: usize, rows: &[Vec<u32>]) -> SparsityPattern {
    let mut count = vec![0usize; n];
    for r in rows {
        for &j in r {
            count[j as usize] += 1;
        }
    }
    let mut col_ptr = vec![0usize; n + 1];
    for j in 0..n {
        col_ptr[j + 1] = col_ptr[j] + count[j];
    }
    let mut fill = col_ptr.clone();
    let mut row_idx = vec![0usize; col_ptr[n]];
    for (i, r) in rows.iter().enumerate() {
        for &j in r {
            row_idx[fill[j as usize]] = i;
            fill[j as usize] += 1;
        }
    }
    SparsityPattern { n, col_ptr, row_idx }
}

/// Whether `outer` contains every entry of `inner` (same size, both with
/// increasing rows per column).
pub(crate) fn contains(outer: &SparsityPattern, inner: &SparsityPattern) -> bool {
    if outer.n != inner.n || outer.col_ptr.len() != outer.n + 1 {
        return false;
    }
    (0..inner.n).all(|j| {
        let o = outer.col(j);
        inner.col(j).iter().all(|r| o.binary_search(r).is_ok())
    })
}

/// A column colouring: columns sharing a row get different colours, so
/// one forward-mode sweep per colour recovers every entry. Greedy in
/// several orders (largest degree first, natural, reverse); the fewest
/// colours wins. Returns the colour of each column and the count.
pub(crate) fn colour(p: &SparsityPattern) -> (Vec<u32>, usize) {
    let n = p.n;
    if n == 0 {
        return (vec![], 0);
    }
    // rows as lists of columns
    let n_rows = p.row_idx.iter().copied().max().map_or(0, |m| m + 1);
    let mut rows: Vec<Vec<u32>> = vec![vec![]; n_rows];
    for j in 0..n {
        for &i in p.col(j) {
            rows[i].push(j as u32);
        }
    }
    // degree in the column-intersection graph (bounded work: with a very
    // dense row every column conflicts with every other anyway)
    let work: usize = rows.iter().map(|r| r.len() * r.len()).sum();
    let mut degree = vec![0usize; n];
    if work <= 50_000_000 {
        let mut seen = vec![usize::MAX; n];
        for (j, deg) in degree.iter_mut().enumerate() {
            for &i in p.col(j) {
                for &k in &rows[i] {
                    if k as usize != j && seen[k as usize] != j {
                        seen[k as usize] = j;
                        *deg += 1;
                    }
                }
            }
        }
    } else {
        for (j, deg) in degree.iter_mut().enumerate() {
            *deg = p.col(j).iter().map(|&i| rows[i].len()).sum();
        }
    }
    let mut lf: Vec<usize> = (0..n).collect();
    lf.sort_by_key(|&j| std::cmp::Reverse(degree[j]));
    let orders = [lf, (0..n).collect(), (0..n).rev().collect()];
    let mut best: Option<(Vec<u32>, usize)> = None;
    for order in orders {
        let mut col = vec![u32::MAX; n];
        let mut mark: Vec<usize> = vec![usize::MAX; n + 1];
        let mut n_col = 0usize;
        for (step, &j) in order.iter().enumerate() {
            for &i in p.col(j) {
                for &k in &rows[i] {
                    let c = col[k as usize];
                    if c != u32::MAX {
                        mark[c as usize] = step;
                    }
                }
            }
            let c = (0..).find(|&c| mark[c] != step).expect("a free colour");
            col[j] = c as u32;
            n_col = n_col.max(c + 1);
        }
        if best.as_ref().is_none_or(|b| n_col < b.1) {
            best = Some((col, n_col));
        }
    }
    best.expect("one ordering")
}

/// Where each `(row, colour)` lands in the column-compressed values.
pub(crate) fn csc_positions(p: &SparsityPattern, col_of: &[u32]) -> HashMap<(usize, u32), usize> {
    let mut pos = HashMap::with_capacity(p.nnz());
    for (j, &c) in col_of.iter().enumerate().take(p.n) {
        for k in p.col_ptr[j]..p.col_ptr[j + 1] {
            pos.insert((p.row_idx[k], c), k);
        }
    }
    pos
}

/// A table read whose axes the run loop watches.
#[derive(Clone, Debug)]
pub(crate) struct TableSite<'m> {
    pub table: u32,
    pub args: &'m [Expr],
}

/// Every table read in the model's assignments and residuals, with the
/// guard list (one entry per axis of each read).
pub(crate) fn table_sites<'m>(model: &'m PreparedModel) -> (Vec<TableSite<'m>>, Vec<TableGuard>) {
    let mut sites = vec![];
    let exprs =
        model.assignments.iter().map(|a| &a.expr).chain(model.residuals.iter().map(|r| &r.expr));
    for e in exprs {
        collect_tables(e, &mut sites);
    }
    let mut guards = vec![];
    for s in &sites {
        let data = &model.flat.tables[s.table as usize].data;
        for axis in 0..s.args.len() {
            let outside = data.outside.get(axis).copied().unwrap_or(Outside::Clamp);
            guards.push(TableGuard { table: s.table, axis: axis as u8, outside });
        }
    }
    (sites, guards)
}

fn collect_tables<'m>(e: &'m Expr, out: &mut Vec<TableSite<'m>>) {
    if let Expr::Table { table, args } = e {
        out.push(TableSite { table: *table, args });
    }
    for c in e.children() {
        collect_tables(c, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_a_tridiagonal_pattern_with_three() {
        let n = 50;
        let rows: Vec<Vec<u32>> = (0..n)
            .map(|i: u32| {
                let mut r = vec![i];
                if i > 0 {
                    r.insert(0, i - 1);
                }
                if i + 1 < n {
                    r.push(i + 1);
                }
                r
            })
            .collect();
        let p = pattern_from_rows(n as usize, &rows);
        let (c, k) = colour(&p);
        assert_eq!(k, 3);
        let pos = csc_positions(&p, &c);
        assert_eq!(pos.len(), p.nnz(), "every (row, colour) is unique");
        assert!(contains(&p, &p));
        assert!(contains(&SparsityPattern::dense(n as usize), &p));
        assert!(!contains(&p, &SparsityPattern::dense(n as usize)));
    }
}
