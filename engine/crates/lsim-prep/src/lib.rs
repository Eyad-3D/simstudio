//! # lsim-prep: model preparation
//!
//! From a component tree to the [`PreparedModel`] the code generator
//! compiles (DESIGN.md, *Preparation pipeline*):
//!
//! 1. [`flatten`] — instances, parameters, port variables, connection sets;
//! 2. [`units_check`] — every equation balances its dimensions;
//! 3. [`alias`] — `a = ±b` and `a = constant` removed;
//! 4. [`structure`] — matching, block-lower-triangular order, explicit
//!    solving of affine single equations, iteration variables for the rest,
//!    zero crossings for `when` clauses; structural faults as plain-language
//!    diagnostics;
//! 5. [`key`] — the structure key for the compiled-code cache.
//!
//! Stage 1 is a real but minimal version of each step. Work package 2
//! (DESIGN.md) owns this crate and adds Hopcroft–Karp matching, tearing,
//! Pantelides index reduction with dummy derivatives, if-expression events,
//! the inverse-model (fast mode) preparation and a stronger simplifier.

pub mod alias;
pub mod flatten;
pub mod key;
pub mod structure;
pub mod symbolic;
pub mod units_check;

pub use structure::CausalOptions;

use lsim_ir::{ComponentDef, Diagnostic, Library, PreparedModel};

/// Options for [`prepare`].
#[derive(Clone, Debug, Default)]
pub struct PrepOptions {
    /// see [`CausalOptions::force_implicit`]
    pub force_implicit: bool,
}

/// Flattens, checks, simplifies and sorts `top` into a prepared model.
pub fn prepare(
    lib: &Library,
    top: &ComponentDef,
    opts: &PrepOptions,
) -> Result<PreparedModel, Vec<Diagnostic>> {
    let mut flat = flatten::flatten(lib, top)?;
    let unit_faults = units_check::check(&flat, lib, top);
    if !unit_faults.is_empty() {
        return Err(unit_faults);
    }
    let (flat_vars, flat_equations) = (flat.vars.len(), flat.equations.len());
    let aliases = alias::eliminate(&mut flat);
    let mut model = structure::causalize(
        flat,
        aliases,
        &CausalOptions { force_implicit: opts.force_implicit },
        lib,
    )?;
    model.stats.flat_vars = flat_vars;
    model.stats.flat_equations = flat_equations;
    Ok(model)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsim_ir::component::build::{connect, sub};
    use lsim_ir::expr::c;

    fn parallel_sources() -> ComponentDef {
        let mut v1 = sub("v1", "Electrical.ConstantVoltage", &[("V", c(10.0))]);
        v1.label = Some("Bench supply".into());
        let mut v2 = sub("v2", "Electrical.ConstantVoltage", &[("V", c(12.0))]);
        v2.label = Some("Charger".into());
        ComponentDef {
            name: "Parallel".into(),
            components: vec![v1, v2, sub("gnd", "Electrical.Ground", &[])],
            connections: vec![
                connect("v1.p", "v2.p"),
                connect("v1.n", "v2.n"),
                connect("v1.n", "gnd.p"),
            ],
            ..Default::default()
        }
    }

    #[test]
    fn two_sources_in_parallel_are_named() {
        let err = prepare(&lsim_lib::library(), &parallel_sources(), &PrepOptions::default())
            .unwrap_err();
        let over = err.iter().find(|d| d.code == "STRUCT-OVER").expect("over-determined");
        assert!(over.message.contains("'Bench supply' and 'Charger'"), "{}", over.message);
        assert!(over.hint.as_ref().unwrap().contains("parallel"));
        let under = err.iter().find(|d| d.code == "STRUCT-UNDER").expect("under-determined");
        assert!(under.message.contains("Nothing determines"), "{}", under.message);
        assert_eq!(over.parts, vec!["v1".to_string(), "v2".to_string()]);
    }

    #[test]
    fn a_unit_slip_is_named() {
        let mut lib = lsim_lib::library();
        let mut bad = lib.components["Electrical.Resistor"].clone();
        bad.name = "Electrical.BadResistor".into();
        // v = R / i: ohm/ampere is not volt
        bad.equations[3] = lsim_ir::component::build::eq(
            lsim_ir::expr::name("v"),
            lsim_ir::expr::name("R") / lsim_ir::expr::name("i"),
            "Ohm's law, mistyped",
        );
        lib.add(bad);
        let mut r = sub("r", "Electrical.BadResistor", &[("R", c(2.0))]);
        r.label = Some("Heater".into());
        let top = ComponentDef {
            name: "T".into(),
            components: vec![
                r,
                sub("src", "Electrical.ConstantVoltage", &[("V", c(1.0))]),
                sub("gnd", "Electrical.Ground", &[]),
            ],
            connections: vec![
                connect("src.p", "r.p"),
                connect("r.n", "src.n"),
                connect("src.n", "gnd.p"),
            ],
            ..Default::default()
        };
        let err = prepare(&lib, &top, &PrepOptions::default()).unwrap_err();
        assert_eq!(err[0].code, "UNIT-MISMATCH");
        assert!(err[0].message.contains("'Heater' (Electrical.BadResistor)"), "{}", err[0].message);
        assert!(err[0].message.contains("“v = R / i”"), "{}", err[0].message);
        assert!(err[0].message.contains("left side is in V"), "{}", err[0].message);
    }
}
