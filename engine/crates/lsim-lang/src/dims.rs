//! Dimensional analysis of component-scope expressions, at parse time.
//!
//! The rules are those of the preparation step's check (lsim-prep), so a
//! model that passes here passes there: a bare number takes the dimension
//! its context needs in sums and comparisons and is dimensionless as a
//! factor; transcendental functions need dimensionless arguments; `der`
//! divides by seconds. One more state exists here: a name whose unit the
//! parser cannot know (a port of a connector type or a part from a
//! library it was not given) is *unknown* and constrains nothing.

use lsim_ir::expr::{BinaryOp, Builtin, Expr};
use lsim_ir::units::{Dim, describe};

#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum D {
    /// a known dimension
    Known(Dim),
    /// a bare number: takes what its context needs
    Any,
    /// cannot be known here
    Unknown,
}

/// What a name is, as far as units go.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum NameDim {
    /// a value with this dimension (or unknown)
    Value(D),
    /// a table parameter: its values' dimension and its axes' dimensions
    Table(D, Vec<D>),
    /// not a name of this scope
    Missing,
}

pub(crate) trait Resolve {
    fn name(&self, n: &str) -> NameDim;
}

pub(crate) struct Mismatch(pub String);

pub(crate) fn words(d: D) -> String {
    match d {
        D::Known(x) => describe(x),
        D::Any => "a plain number".into(),
        D::Unknown => "an unknown unit".into(),
    }
}

fn unify(a: D, b: D, what: &str) -> Result<D, Mismatch> {
    match (a, b) {
        (D::Any, x) | (x, D::Any) => Ok(x),
        (D::Unknown, x) | (x, D::Unknown) => Ok(x),
        (D::Known(x), D::Known(y)) if x == y => Ok(a),
        (D::Known(x), D::Known(y)) => {
            Err(Mismatch(format!("{what} mixes {} and {}", describe(x), describe(y))))
        }
    }
}

fn dimensionless(a: D, f: &str) -> Result<(), Mismatch> {
    match a {
        D::Known(d) if !d.is_none() => {
            Err(Mismatch(format!("{f} needs a dimensionless argument, not {}", describe(d))))
        }
        _ => Ok(()),
    }
}

fn const_value(e: &Expr) -> Option<f64> {
    match e {
        Expr::Const(v) => Some(*v),
        Expr::Neg(a) => const_value(a).map(|v| -v),
        _ => None,
    }
}

/// The dimension of `e`.
pub(crate) fn dim(e: &Expr, r: &dyn Resolve) -> Result<D, Mismatch> {
    Ok(match e {
        Expr::Const(_) => D::Any,
        Expr::Time => D::Known(Dim::TIME),
        Expr::Name(n) => match r.name(n) {
            NameDim::Value(d) => d,
            NameDim::Table(..) => {
                return Err(Mismatch(format!("'{n}' is a table: read it at a point, as {n}(x)")));
            }
            NameDim::Missing => D::Unknown,
        },
        Expr::Var(_) | Expr::Param(_) | Expr::Der(_) | Expr::Pre(_) => D::Unknown,
        Expr::Neg(a) | Expr::NoEvent(a) => dim(a, r)?,
        Expr::Binary(op, a, b) => {
            let (da, db) = (dim(a, r)?, dim(b, r)?);
            match op {
                BinaryOp::Add | BinaryOp::Sub => unify(da, db, "a sum")?,
                BinaryOp::Mul => match (da, db) {
                    (D::Unknown, _) | (_, D::Unknown) => D::Unknown,
                    (D::Known(x), D::Known(y)) => D::Known(x * y),
                    (D::Known(x), D::Any) | (D::Any, D::Known(x)) => D::Known(x),
                    (D::Any, D::Any) => D::Any,
                },
                BinaryOp::Div => match (da, db) {
                    (D::Unknown, _) | (_, D::Unknown) => D::Unknown,
                    (D::Known(x), D::Known(y)) => D::Known(x / y),
                    (D::Known(x), D::Any) => D::Known(x),
                    (D::Any, D::Known(y)) => D::Known(Dim::NONE / y),
                    (D::Any, D::Any) => D::Any,
                },
                BinaryOp::Pow => {
                    dimensionless(db, "the exponent of ^")?;
                    match da {
                        D::Any | D::Unknown => da,
                        D::Known(x) if x.is_none() => da,
                        D::Known(x) => match const_value(b) {
                            Some(n) if n.fract() == 0.0 && n.abs() < 64.0 => {
                                D::Known(x.powi(n as i8))
                            }
                            Some(n) if (2.0 * n).fract() == 0.0 && n.abs() < 32.0 => {
                                match x.root(2) {
                                    Some(rt) => D::Known(rt.powi((2.0 * n) as i8)),
                                    None => {
                                        return Err(Mismatch(format!(
                                            "{} has no square root",
                                            describe(x)
                                        )));
                                    }
                                }
                            }
                            _ => {
                                return Err(Mismatch(format!(
                                    "{} is raised to a power that is not a whole or half number",
                                    describe(x)
                                )));
                            }
                        },
                    }
                }
            }
        }
        Expr::Call(f, args) => {
            let ds: Vec<D> = args.iter().map(|a| dim(a, r)).collect::<Result<_, _>>()?;
            let first = ds.first().copied().unwrap_or(D::Unknown);
            match f {
                Builtin::Sqrt => match first {
                    D::Known(x) => D::Known(
                        x.root(2).ok_or_else(|| Mismatch(format!("sqrt() of {}", describe(x))))?,
                    ),
                    other => other,
                },
                Builtin::Abs | Builtin::Pre => first,
                Builtin::Der => match first {
                    D::Known(d) => D::Known(d / Dim::TIME),
                    other => other,
                },
                Builtin::Sign => D::Known(Dim::NONE),
                Builtin::Min | Builtin::Max | Builtin::Limit => {
                    let what = format!("{}()", f.name());
                    ds.iter().skip(1).try_fold(first, |acc, d| unify(acc, *d, &what))?
                }
                Builtin::Atan2 => {
                    unify(first, ds.get(1).copied().unwrap_or(D::Unknown), "atan2()")?;
                    D::Known(Dim::NONE)
                }
                _ => {
                    dimensionless(first, &format!("{}()", f.name()))?;
                    D::Known(Dim::NONE)
                }
            }
        }
        Expr::Compare(_, a, b) => {
            unify(dim(a, r)?, dim(b, r)?, "a comparison")?;
            D::Known(Dim::NONE)
        }
        Expr::And(a, b) | Expr::Or(a, b) => {
            dim(a, r)?;
            dim(b, r)?;
            D::Known(Dim::NONE)
        }
        Expr::Not(a) => {
            dim(a, r)?;
            D::Known(Dim::NONE)
        }
        Expr::If(c, a, b) => {
            dim(c, r)?;
            unify(dim(a, r)?, dim(b, r)?, "an if-expression's two branches")?
        }
        Expr::Table { args, .. } => {
            let Some((Expr::Name(t), at)) = args.split_first() else { return Ok(D::Unknown) };
            match r.name(t) {
                NameDim::Table(values, axes) => {
                    if at.len() != axes.len() {
                        return Err(Mismatch(format!(
                            "the table '{t}' has {} axes but is read at {} values",
                            axes.len(),
                            at.len()
                        )));
                    }
                    for (k, (a, ax)) in at.iter().zip(&axes).enumerate() {
                        let da = dim(a, r)?;
                        if let (D::Known(x), D::Known(y)) = (da, *ax)
                            && x != y
                        {
                            return Err(Mismatch(format!(
                                "the table '{t}' is read at {} on its axis {}, which is in {}",
                                describe(x),
                                k + 1,
                                describe(y)
                            )));
                        }
                    }
                    values
                }
                NameDim::Value(_) => {
                    return Err(Mismatch(format!("'{t}' is not a table")));
                }
                NameDim::Missing => D::Unknown,
            }
        }
    })
}

/// `0 = if c then a else b` holds as `0 = a` while `c` and `0 = b`
/// otherwise (an `if` equation whose branches state different
/// quantities): each branch must balance on its own.
fn residual_branches(e: &Expr, r: &dyn Resolve) -> Result<(), String> {
    match e {
        Expr::If(c, a, b) => {
            dim(c, r).map_err(|m| m.0)?;
            residual_branches(a, r)?;
            residual_branches(b, r)
        }
        other => dim(other, r).map(|_| ()).map_err(|m| m.0),
    }
}

/// Checks that `lhs` and `rhs` have the same dimension; the reason in
/// words when not.
pub(crate) fn balance(lhs: &Expr, rhs: &Expr, r: &dyn Resolve) -> Result<(), String> {
    match (lhs, rhs) {
        (Expr::Const(z), e @ Expr::If(..)) | (e @ Expr::If(..), Expr::Const(z)) if *z == 0.0 => {
            return residual_branches(e, r);
        }
        _ => {}
    }
    let l = dim(lhs, r).map_err(|m| m.0)?;
    let rr = dim(rhs, r).map_err(|m| m.0)?;
    unify(l, rr, "the equation")
        .map(|_| ())
        .map_err(|_| format!("the left side is in {}, the right side in {}", words(l), words(rr)))
}

/// Checks that `e` has the dimension `want` (a bare number or unknown
/// passes); the reason in words when not.
pub(crate) fn expect(e: &Expr, want: Dim, r: &dyn Resolve) -> Result<(), String> {
    let d = dim(e, r).map_err(|m| m.0)?;
    match d {
        D::Known(x) if x != want => {
            Err(format!("it is in {}, not {}", describe(x), describe(want)))
        }
        _ => Ok(()),
    }
}
