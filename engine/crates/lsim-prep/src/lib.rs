//! # lsim-prep: model preparation
//!
//! From a component tree to the [`PreparedModel`] the code generator
//! compiles (DESIGN.md, *Preparation pipeline*), in [`pipeline`]:
//!
//! 1. [`flatten`] — instances, parameters (in binding order), port
//!    variables, connection sets, start values as expressions of the
//!    parameters, sampled external blocks;
//! 2. [`units_check`] — every equation balances its dimensions;
//! 3. [`inverse`] — fast mode only: the prescribed trajectory becomes an
//!    input, the driver is removed, limits pass through;
//! 4. [`alias`] — `a = ±b` and `a = constant` removed;
//! 5. [`modes`] — relations of `if`-expressions, `abs` and `sign` become
//!    held modes with zero crossings;
//! 6. [`system`] — every variable and derivative a node, every equation a
//!    residual; a structurally singular model is told in plain words
//!    ([`diagnose`]);
//! 7. [`index`] — Pantelides' algorithm and dummy derivatives;
//! 8. [`init`] — the initialisation system, solved at preparation
//!    ([`numeric`]) for the pivoting checks and the states' choice;
//! 9. [`causal`] — Hopcroft–Karp matching ([`graph`]), block-lower-
//!    triangular order, tearing, linear blocks and explicit solving with
//!    pivot checks;
//! 10. [`external`] — the tick order of sampled blocks; [`sparsity`] —
//!     the Jacobian's pattern; [`key`] — the structure key.
//!
//! [`prepare`] makes the forward model, [`prepare_inverse`] fast mode's
//! inverse model, [`prepare_init`] the initialisation system alone (it is
//! also part of every prepared model, `PreparedModel::init`), and
//! [`prepare_with_report`] tells what preparation found.

pub mod alias;
pub mod causal;
pub mod diagnose;
pub mod external;
pub mod flatten;
pub mod graph;
pub mod index;
pub mod init;
pub mod inverse;
pub mod key;
pub mod modes;
pub mod numeric;
pub mod params;
pub mod pipeline;
pub mod sparsity;
pub mod structure;
pub mod symbolic;
pub mod system;
pub mod units_check;

pub use pipeline::{BlockSummary, PrepReport, Settings};
pub use structure::CausalOptions;

use lsim_ir::{ComponentDef, Diagnostic, InitSystem, InverseSpec, Library, PreparedModel};

/// Options for [`prepare`].
#[derive(Clone, Debug, Default)]
pub struct PrepOptions {
    /// see [`CausalOptions::force_implicit`]
    pub force_implicit: bool,
}

impl PrepOptions {
    fn settings(&self) -> Settings {
        Settings { force_implicit: self.force_implicit, ..Settings::default() }
    }
}

/// Flattens, checks, simplifies and sorts `top` into a prepared model.
pub fn prepare(
    lib: &Library,
    top: &ComponentDef,
    opts: &PrepOptions,
) -> Result<PreparedModel, Vec<Diagnostic>> {
    pipeline::run(lib, top, None, &opts.settings()).map(|(m, _)| m)
}

/// Prepares fast mode's inverse model: the same equations with the
/// variables `spec.prescribed` (and their derivatives) known inputs and the
/// signal inputs `spec.freed` computed from them (DESIGN.md, *Fast mode*).
pub fn prepare_inverse(
    lib: &Library,
    top: &ComponentDef,
    spec: &InverseSpec,
    opts: &PrepOptions,
) -> Result<PreparedModel, Vec<Diagnostic>> {
    pipeline::run(lib, top, Some(spec), &opts.settings()).map(|(m, _)| m)
}

/// The initialisation system of `top` (also `PreparedModel::init` of
/// [`prepare`]'s result).
pub fn prepare_init(
    lib: &Library,
    top: &ComponentDef,
    opts: &PrepOptions,
) -> Result<InitSystem, Vec<Diagnostic>> {
    prepare(lib, top, opts).map(|m| m.init)
}

/// [`prepare`] or (with `spec`) [`prepare_inverse`] with explicit
/// settings, and the report of what preparation found.
pub fn prepare_with_report(
    lib: &Library,
    top: &ComponentDef,
    spec: Option<&InverseSpec>,
    settings: &Settings,
) -> Result<(PreparedModel, PrepReport), Vec<Diagnostic>> {
    pipeline::run(lib, top, spec, settings)
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
        for d in &err {
            println!("{d}");
        }
        let fault = err.iter().find(|d| d.code == "ELEC-SOURCE-LOOP").expect("recognised");
        assert!(fault.message.contains("'Bench supply' and 'Charger'"), "{}", fault.message);
        assert!(fault.hint.as_ref().unwrap().contains("parallel"));
        assert_eq!(fault.parts, vec!["v1".to_string(), "v2".to_string()]);
        // the loop current left undetermined is the same fault, told once
        assert_eq!(err.len(), 1, "{err:#?}");
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
        println!("{}", err[0]);
        assert_eq!(err[0].code, "UNIT-MISMATCH");
        assert!(err[0].message.contains("'Heater' (Electrical.BadResistor)"), "{}", err[0].message);
        assert!(err[0].message.contains("“v = R / i”"), "{}", err[0].message);
        assert!(err[0].message.contains("left side is in V"), "{}", err[0].message);
    }
}
