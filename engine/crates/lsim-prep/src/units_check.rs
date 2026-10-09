//! Dimensional checking of flat equations.
//!
//! Each side's dimension is inferred from the variables' and parameters'
//! units; a bare number takes whatever dimension its context needs in sums
//! and comparisons, and is dimensionless as a factor. Transcendental
//! functions need dimensionless arguments. A mismatch is reported against
//! the component and the equation as the user wrote it.

use lsim_ir::Diagnostic;
use lsim_ir::component::{Equation, Library};
use lsim_ir::expr::{BinaryOp, Builtin, Expr};
use lsim_ir::flat::{FlatSystem, OriginKind};
use lsim_ir::units::{Dim, describe};

#[derive(Clone, Copy, PartialEq, Debug)]
enum D {
    Known(Dim),
    /// a bare number
    Any,
}

struct Mismatch(String);

fn unify(a: D, b: D, what: &str) -> Result<D, Mismatch> {
    match (a, b) {
        (D::Any, x) | (x, D::Any) => Ok(x),
        (D::Known(x), D::Known(y)) if x == y => Ok(a),
        (D::Known(x), D::Known(y)) => {
            Err(Mismatch(format!("{what} mixes {} and {}", describe(x), describe(y))))
        }
    }
}

fn dimensionless(a: D, f: &str) -> Result<(), Mismatch> {
    match a {
        D::Any => Ok(()),
        D::Known(d) if d.is_none() => Ok(()),
        D::Known(d) => {
            Err(Mismatch(format!("{f}() needs a dimensionless argument, not {}", describe(d))))
        }
    }
}

fn dim(e: &Expr, flat: &FlatSystem) -> Result<D, Mismatch> {
    let var = |v: &lsim_ir::VarId| D::Known(flat.vars[v.0 as usize].unit.dim);
    Ok(match e {
        Expr::Const(_) => D::Any,
        Expr::Time => D::Known(Dim::TIME),
        Expr::Name(_) => D::Any,
        Expr::Table { table, args } => match flat.tables.get(*table as usize) {
            Some(t) => {
                for (k, (a, unit)) in args.iter().zip(&t.data.axis_units).enumerate() {
                    if let (D::Known(x), Ok(u)) = (dim(a, flat)?, lsim_ir::units::parse_unit(unit))
                        && x != u.dim
                    {
                        return Err(Mismatch(format!(
                            "the table '{}' is read at {} on its axis {}, which is in {}",
                            t.name,
                            describe(x),
                            k + 1,
                            describe(u.dim)
                        )));
                    }
                }
                D::Known(t.unit.dim)
            }
            None => D::Any,
        },
        Expr::Var(v) | Expr::Pre(v) => var(v),
        Expr::Param(p) => D::Known(flat.params[p.0 as usize].unit.dim),
        Expr::Der(v) => match var(v) {
            D::Known(d) => D::Known(d / Dim::TIME),
            D::Any => D::Any,
        },
        Expr::Neg(a) | Expr::NoEvent(a) => dim(a, flat)?,
        Expr::Binary(op, a, b) => {
            let (da, db) = (dim(a, flat)?, dim(b, flat)?);
            match op {
                BinaryOp::Add | BinaryOp::Sub => unify(da, db, "a sum")?,
                BinaryOp::Mul => match (da, db) {
                    (D::Known(x), D::Known(y)) => D::Known(x * y),
                    (D::Known(x), D::Any) | (D::Any, D::Known(x)) => D::Known(x),
                    (D::Any, D::Any) => D::Any,
                },
                BinaryOp::Div => match (da, db) {
                    (D::Known(x), D::Known(y)) => D::Known(x / y),
                    (D::Known(x), D::Any) => D::Known(x),
                    (D::Any, D::Known(y)) => D::Known(Dim::NONE / y),
                    (D::Any, D::Any) => D::Any,
                },
                BinaryOp::Pow => {
                    dimensionless(db, "the exponent of ^")?;
                    match (da, b.as_ref()) {
                        (D::Any, _) => D::Any,
                        (D::Known(x), _) if x.is_none() => da,
                        (D::Known(x), Expr::Const(n)) if n.fract() == 0.0 => {
                            D::Known(x.powi(*n as i8))
                        }
                        (D::Known(x), Expr::Const(n)) if (2.0 * n).fract() == 0.0 => {
                            match x.root(2) {
                                Some(r) => D::Known(r.powi((2.0 * n) as i8)),
                                None => {
                                    return Err(Mismatch(format!(
                                        "{} has no square root",
                                        describe(x)
                                    )));
                                }
                            }
                        }
                        (D::Known(x), _) => {
                            return Err(Mismatch(format!(
                                "{} is raised to a power that is not a whole or half number",
                                describe(x)
                            )));
                        }
                    }
                }
            }
        }
        Expr::Call(f, args) => {
            let ds: Vec<D> = args.iter().map(|a| dim(a, flat)).collect::<Result<_, _>>()?;
            match f {
                Builtin::Sqrt => match ds[0] {
                    D::Any => D::Any,
                    D::Known(x) => D::Known(
                        x.root(2).ok_or_else(|| Mismatch(format!("sqrt() of {}", describe(x))))?,
                    ),
                },
                Builtin::Abs => ds[0],
                Builtin::Sign => D::Known(Dim::NONE),
                Builtin::Min | Builtin::Max | Builtin::Limit => {
                    ds.iter().skip(1).try_fold(ds[0], |acc, d| unify(acc, *d, f.name()))?
                }
                Builtin::Atan2 => {
                    unify(ds[0], ds[1], "atan2()")?;
                    D::Known(Dim::NONE)
                }
                Builtin::Der | Builtin::Pre => ds[0],
                _ => {
                    dimensionless(ds[0], f.name())?;
                    D::Known(Dim::NONE)
                }
            }
        }
        Expr::Compare(_, a, b) => {
            unify(dim(a, flat)?, dim(b, flat)?, "a comparison")?;
            D::Known(Dim::NONE)
        }
        Expr::And(a, b) | Expr::Or(a, b) => {
            dim(a, flat)?;
            dim(b, flat)?;
            D::Known(Dim::NONE)
        }
        Expr::Not(a) => {
            dim(a, flat)?;
            D::Known(Dim::NONE)
        }
        Expr::If(c, a, b) => {
            dim(c, flat)?;
            unify(dim(a, flat)?, dim(b, flat)?, "an if-expression's two branches")?
        }
    })
}

/// `0 = if c then a else b` holds as `0 = a` while `c` and `0 = b`
/// otherwise (an `if` equation whose branches state different
/// quantities): each branch must balance on its own.
fn residual_branches(e: &Expr, flat: &FlatSystem) -> Result<(), Mismatch> {
    match e {
        Expr::If(c, a, b) => {
            dim(c, flat)?;
            residual_branches(a, flat)?;
            residual_branches(b, flat)
        }
        other => dim(other, flat).map(|_| ()),
    }
}

/// Checks every equation and `when` action; returns one diagnostic per
/// unbalanced equation.
pub fn check(flat: &FlatSystem, lib: &Library, top: &lsim_ir::ComponentDef) -> Vec<Diagnostic> {
    let mut out = vec![];
    let text_of = |origin: &lsim_ir::Origin| -> String {
        let inst = flat.instance(origin.instance);
        if let OriginKind::Component { index } = origin.kind
            && let Some(def) =
                if inst.def == top.name { Some(top) } else { lib.components.get(&inst.def) }
            && let Some(Equation::Eq { lhs, rhs }) = def.equations.get(index).map(|e| &e.eq)
        {
            return format!("“{lhs} = {rhs}”");
        }
        match &origin.kind {
            OriginKind::Component { index } => format!("number {}", index + 1),
            other => format!("{other:?}"),
        }
    };
    for e in &flat.equations {
        let residual_if = match (&e.lhs, &e.rhs) {
            (Expr::Const(z), x @ Expr::If(..)) | (x @ Expr::If(..), Expr::Const(z))
                if *z == 0.0 =>
            {
                Some(x)
            }
            _ => None,
        };
        if let Some(x) = residual_if {
            if let Err(Mismatch(why)) = residual_branches(x, flat) {
                let who = flat.instance_name(e.origin.instance);
                let def = &flat.instance(e.origin.instance).def;
                let mut d = Diagnostic::error(
                    "UNIT-MISMATCH",
                    format!(
                        "In {who} ({def}), the equation {} does not balance its units: {why}.",
                        text_of(&e.origin)
                    ),
                );
                d.parts.push(flat.instance(e.origin.instance).path.clone());
                out.push(d);
            }
            continue;
        }
        let r = dim(&e.lhs, flat).and_then(|l| {
            let r = dim(&e.rhs, flat)?;
            unify(l, r, "the equation").map_err(|_| {
                let side = |d: D| match d {
                    D::Known(x) => describe(x),
                    D::Any => "a plain number".into(),
                };
                Mismatch(format!("the left side is in {}, the right side in {}", side(l), side(r)))
            })
        });
        if let Err(Mismatch(why)) = r {
            let who = flat.instance_name(e.origin.instance);
            let def = &flat.instance(e.origin.instance).def;
            let mut d = Diagnostic::error(
                "UNIT-MISMATCH",
                format!(
                    "In {who} ({def}), the equation {} does not balance its units: {why}.",
                    text_of(&e.origin)
                ),
            );
            d.parts.push(flat.instance(e.origin.instance).path.clone());
            out.push(d);
        }
    }
    for w in &flat.whens {
        for (v, value) in &w.assign {
            let target = D::Known(flat.vars[v.0 as usize].unit.dim);
            if let Err(Mismatch(why)) =
                dim(value, flat).and_then(|d| unify(target, d, "the assignment"))
            {
                let who = flat.instance_name(w.origin.instance);
                out.push(Diagnostic::error(
                    "UNIT-MISMATCH",
                    format!(
                        "In {who}, an event assigns '{}' a value whose units differ: {why}.",
                        flat.vars[v.0 as usize].name
                    ),
                ));
            }
        }
    }
    out
}
