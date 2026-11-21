//! Units whose exponents leave -128..=127 (step 3 fuzzing: they panicked)
//! are refused plainly when a model is prepared: a product, a quotient, a
//! whole or half power, or the derivative of a variable in the equations.

use lsim_ir::component::build::{eq, param, state, var};
use lsim_ir::expr::{BinaryOp, Expr, c, der, name as n};
use lsim_ir::{ComponentDef, Library, ParamDecl, ParamValue, TableData};
use lsim_prep::{PrepOptions, prepare};

/// `x = rhs` with parameters `a` and `b` in `m100`, `m` in `m-128`, `q` in
/// `m4`, and a table `t` in `s100` over an axis in `s-100`.
fn model(rhs: Expr) -> ComponentDef {
    ComponentDef {
        name: "Test.Overflow".into(),
        params: vec![
            param("a", "m100", 1.0, ""),
            param("b", "m100", 1.0, ""),
            param("m", "m-128", 1.0, ""),
            param("q", "m4", 1.0, ""),
            ParamDecl {
                default: ParamValue::Table(
                    TableData::new_1d(vec![0.0, 1.0], vec![0.0, 1.0]).with_units(&["s-100"]),
                ),
                ..param("t", "s100", 0.0, "")
            },
        ],
        vars: vec![var("x", "m", "")],
        equations: vec![eq(n("x"), rhs, "x is given")],
        ..Default::default()
    }
}

fn pow(a: Expr, e: f64) -> Expr {
    Expr::bin(BinaryOp::Pow, a, c(e))
}

/// The message of the refusal of `top`, which must be one.
fn refusal(top: &ComponentDef) -> String {
    let d = prepare(&Library::default(), top, &PrepOptions::default()).expect_err("a refusal");
    d.iter().map(|d| d.message.clone()).collect::<Vec<_>>().join("\n")
}

#[test]
fn a_unit_that_overflows_is_refused_not_a_panic() {
    for (what, rhs) in [
        ("a product", n("a") * n("b")),
        ("a quotient", n("a") / n("m")),
        ("a number over a unit", c(1.0) / n("m")),
        (
            "the slope of a table",
            Expr::Table { table: 0, args: vec![n("t"), c(0.5)], order: vec![1, 0] },
        ),
        ("a whole power", pow(n("q"), 63.0)),
        ("a half power", pow(n("q"), 63.5)),
        ("a power that is too large", pow(n("a"), 200.0)),
    ] {
        let why = refusal(&model(rhs));
        assert!(why.contains("too large"), "{what}: {why}");
    }
}

#[test]
fn the_derivative_of_a_unit_at_the_end_of_the_range_is_refused() {
    let top = ComponentDef {
        name: "Test.Overflow".into(),
        vars: vec![state("y", "s-128", 0.0, "")],
        equations: vec![eq(der("y"), c(0.0), "y does not change")],
        ..Default::default()
    };
    let why = refusal(&top);
    assert!(why.contains("too large"), "{why}");
}
