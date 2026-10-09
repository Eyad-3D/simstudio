//! # lsim-project: today's project files in the new engine
//!
//! A project's diagram (`systems[].elements`, `connections`,
//! `dataBusConnections`, `docs/spec/project.md`) becomes one top-level
//! [`ComponentDef`](lsim_ir::ComponentDef) ([`import`], [`import_case`]):
//!
//! * each part (`componentDefId`) becomes a sub-component through its
//!   [`BlockMapping`] in the [`Registry`] ([`standard_registry`]: today's
//!   36 blocks): the block definition its options and wiring ask for, its
//!   parameters in SI, and the runtime data the mapping works out (tables,
//!   scale factors, start speeds);
//! * a wire becomes a `connect` of two physical ports, a signal link a
//!   `connect` of an output to an input (with a conversion where today's
//!   display units differ), across sub-systems;
//! * what today's engine does implicitly becomes parts and connections:
//!   the vehicle's wheels and air, the driver's blending inputs, grounds,
//!   bus managers, fuel lines, unwired inputs;
//! * a case's `parameterOverrides` are applied as the parts' values.
//!
//! The [`ImportReport`]'s channel map lets the app, the Python package and
//! the golden comparisons read new results under today's names, so runs of
//! both engines line up channel by channel. [`model`] builds and runs a
//! model on the engine's pipeline; [`golden`] compares the example
//! projects' cases with today's engine.

pub mod golden;
pub mod import;
pub mod model;
pub mod reference;

use lsim_ir::units::parse_unit;

pub use import::{
    BlockMapping, Channel, ImportOptions, ImportReport, Mapped, Registry, RunSettings, SampledSpec,
    import, import_case, standard_registry,
};

/// The app's display unit text as the engine's unit parser reads it. Two
/// of today's units differ in meaning from their SI reading: rotational
/// speed `1/min` is revolutions per minute, and acceleration `g` is
/// standard gravity.
pub fn app_unit(text: &str) -> String {
    match text {
        "1/min" => "rev/min".into(),
        "g" => "gn".into(),
        other => other.into(),
    }
}

/// A part's id as an identifier: letters, digits and `_` (the id itself is
/// kept as the instance's `ui_id`).
pub fn ident(id: &str) -> String {
    let mut s: String =
        id.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    if s.chars().next().is_none_or(|c| c.is_ascii_digit()) {
        s.insert(0, '_');
    }
    s
}

/// A number in an app unit, converted to SI.
pub fn to_si(value: f64, app_unit_text: &str) -> Result<f64, String> {
    parse_unit(&app_unit(app_unit_text)).map(|u| u.to_si(value)).map_err(|e| e.to_string())
}
