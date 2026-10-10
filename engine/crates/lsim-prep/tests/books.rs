//! The energy books of prepared, compiled models whose declared stored
//! energies and losses read tables. The books evaluate those expressions
//! outside the compiled code: they read the model's own interpolants
//! (`ModelFunctions::eval_table`), and a stored energy's rate goes through
//! a table exactly, by its interpolant's derivatives. The review found
//! the books NaN and the default run failing at t = 0 (the books had no
//! tables).

mod common;

use common::*;
use lsim_ir::component::build::{connect, eq, param, port, state, sub, var};
use lsim_ir::expr::{Expr, c, der, name as n, table};
use lsim_ir::{ComponentDef, EnergyDecl, ParamDecl, ParamValue};
use lsim_prep::{Settings, prepare_with_report};
use lsim_solve::{OutputGrid, RunInfo, SolverOptions};

/// How the inertia declares its books.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Form {
    /// stored ½ J w² · one(w), a table that is 1 everywhere
    Scaled,
    /// stored ½ J id(w)², a table that is the identity (a monotone cubic
    /// through points on a line is that line)
    Identity,
    /// stored ½ J id2(w, s)², a 2-D table that is the identity along its
    /// first axis, `s = time` (a declared energy reads time only through
    /// a variable)
    Identity2D,
    /// stored ½ J w², and a loss 1e-30 × tau w one(w): no loss to speak of
    Loss,
}

fn table_param(name: &str, unit: &str, default: ParamValue) -> ParamDecl {
    ParamDecl {
        name: name.into(),
        unit: unit.into(),
        display_unit: None,
        default,
        min: None,
        max: None,
        structural: false,
        doc: String::new(),
    }
}

/// An inertia J = 2 kg m² driven by 3 N m from rest (every state 0): w =
/// 1.5 t, so at 2 s w = 3 rad/s, and the source supplied 9 J, all of it
/// stored.
fn spin(form: Form) -> (lsim_ir::PreparedModel, usize) {
    let half_j = || c(0.5) * n("J");
    let (stored, loss) = match form {
        Form::Scaled => (half_j() * n("w") * n("w") * table("one", vec![n("w")]), None),
        Form::Identity => {
            let id = || table("id", vec![n("w")]);
            (half_j() * id() * id(), None)
        }
        Form::Identity2D => {
            let id = || table("id2", vec![n("w"), n("s")]);
            (half_j() * id() * id(), None)
        }
        Form::Loss => (
            half_j() * n("w") * n("w"),
            Some(c(1e-30) * n("a.tau") * n("w") * table("one", vec![n("w")])),
        ),
    };
    let mut wheel = ComponentDef {
        name: "Test.TableInertia".into(),
        ports: vec![port("a", "Flange", "")],
        vars: vec![state("w", "rad/s", 0.0, "speed"), var("s", "s", "the time")],
        equations: vec![
            eq(n("w"), n("a.w"), "it turns with its flange"),
            eq(n("J") * der("w"), n("a.tau"), "J dw/dt = tau"),
            eq(n("s"), Expr::Time, "the time"),
        ],
        energy: EnergyDecl { stored: Some(stored), loss },
        ..Default::default()
    };
    wheel.params.push(param("J", "kg.m2", 2.0, "inertia"));
    let one =
        ParamValue::Table1D { x: vec![-1e3, 1e3], y: vec![1.0, 1.0], axis_unit: "rad/s".into() };
    wheel.params.push(table_param("one", "1", one));
    let line = vec![-1e3, -10.0, 0.0, 0.5, 2.0, 1e3];
    let id = ParamValue::Table1D { x: line.clone(), y: line.clone(), axis_unit: "rad/s".into() };
    wheel.params.push(table_param("id", "rad/s", id));
    let x2 = vec![0.0, 1.0, 10.0];
    let id2 = ParamValue::Table2D {
        values: line.iter().flat_map(|v| [*v; 3]).collect(),
        x1: line,
        x2,
        axis_units: ["rad/s".into(), "s".into()],
    };
    wheel.params.push(table_param("id2", "rad/s", id2));
    let mut lib = library();
    lib.add(wheel);
    let top = ComponentDef {
        name: "Test.Spin".into(),
        components: vec![
            sub("drive", "Rotational.ConstantTorque", &[("tau", c(3.0))]),
            sub("wheel", "Test.TableInertia", &[]),
        ],
        connections: vec![connect("drive.flange", "wheel.a")],
        ..Default::default()
    };
    let (m, _) = prepare_with_report(&lib, &top, None, &Settings::default())
        .unwrap_or_else(|d| panic!("{d:#?}"));
    let info = RunInfo::from_prepared(&m);
    let part = info
        .energy
        .as_ref()
        .and_then(|e| e.parts.iter().position(|p| p.path == "wheel"))
        .expect("the wheel's books");
    (m, part)
}

fn check(form: Form) {
    let (m, part) = spin(form);
    let jit = lsim_codegen::compile(&m, &Default::default()).expect("compiles");
    let info = RunInfo::from_prepared(&m);
    // the rate through the table is taken exactly, not by a difference
    let rates = info.stored_rates.as_ref().expect("stored rates");
    assert!(rates.exact[part], "{form:?}: the wheel's rate is exact");
    // with and without the books' error control (the default has it)
    for control in [true, false] {
        let opts = SolverOptions {
            rtol: 1e-8,
            atol: 1e-8,
            energy_error_control: control,
            ..Default::default()
        };
        let grid = OutputGrid { t0: 0.0, t_end: 2.0, dt: 0.5 };
        let run = lsim_solve::simulate(&jit, &info, &opts, grid, &mut [])
            .unwrap_or_else(|e| panic!("{form:?}, error control {control}: {e}"));
        let w = *run.channel("wheel.w").unwrap().last().unwrap();
        let b = run.energy.as_ref().expect("the books");
        let wheel = &b.parts[part];
        println!(
            "{form:?}, error control {control}: w(2) = {w}, supplied {} J, stored {} J \
             (integrated {} J), lost {:e} J, closure {:.1e}",
            b.supplied, wheel.stored_change, wheel.stored_integral, wheel.lost, b.relative_closure
        );
        assert!((w - 3.0).abs() < 1e-9, "{form:?}: w(2) = {w}");
        assert!((wheel.stored_change - 9.0).abs() < 1e-7, "{form:?}: {}", wheel.stored_change);
        // the books close to round-off: the stored energy's rate is the
        // power in at every instant
        assert!(b.relative_closure.abs() < 1e-12, "{form:?}: closure {:.1e}", b.relative_closure);
        if !control {
            // (without their error control the quadratures take the
            // states' steps, 1 s long here: the integrals are coarse, the
            // drift says so)
            continue;
        }
        assert!((b.supplied - 9.0).abs() < 1e-7, "{form:?}: supplied {}", b.supplied);
        assert!((wheel.stored_integral - 9.0).abs() < 1e-7, "{form:?}: {}", wheel.stored_integral);
        if form == Form::Loss {
            // ∫ 1e-30 tau w dt = 1e-30 × 9 J
            assert!((wheel.lost - 9e-30).abs() < 1e-36, "{form:?}: lost {:e}", wheel.lost);
        }
    }
}

/// A stored energy written through a table, 1-D or 2-D, of the speed (and
/// the time): the run goes from rest with the books on, and they close.
#[test]
fn a_stored_energy_through_a_table_closes_its_books() {
    for form in [Form::Scaled, Form::Identity, Form::Identity2D] {
        check(form);
    }
}

/// A declared loss written through a table: the books read it.
#[test]
fn a_loss_through_a_table_closes_its_books() {
    check(Form::Loss);
}

/// The books read the tables the compiled code interpolates: the model's
/// tables at a point, value and derivatives, are those its channels show.
#[test]
fn the_books_read_the_compiled_tables() {
    use lsim_ir::runtime::ModelFunctions;
    let (m, _) = spin(Form::Identity);
    let jit = lsim_codegen::compile(&m, &Default::default()).expect("compiles");
    let k = m.flat.tables.iter().position(|t| t.name.ends_with(".id")).expect("the table") as u32;
    for x in [-20.0, -10.0, 0.25, 1.0, 3.0, 500.0] {
        let (v, d) = jit.eval_table(k, [x, 0.0]).expect("the model gives its tables");
        assert!((v - x).abs() < 1e-12 * x.abs().max(1.0) && (d[0] - 1.0).abs() < 1e-12, "{x}");
        assert_eq!(d[1], 0.0);
        assert_eq!((v, d), jit.table(k as usize).eval([x, 0.0]));
    }
    assert!(jit.eval_table(m.flat.tables.len() as u32, [0.0, 0.0]).is_none(), "no such table");
}
