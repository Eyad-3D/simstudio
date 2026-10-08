//! The ready-made blocks of today's library (`components.json`),
//! re-created as acausal components.
//!
//! * **Same names**: a block's ports and parameters keep today's ids
//!   (`pos`, `shaft`, `sig_soc`, `capacity_kWh` …), so wires, data-bus
//!   links, overrides and channels map one to one.
//! * **SI inside**: every parameter and port is held in SI; today's display
//!   unit is the parameter's `display_unit` (`capacity_kWh` holds J and
//!   shows kWh). The importer converts the app's numbers on the way in.
//! * **Defaults are today's**: read from the embedded `components.json`
//!   ([`catalog`]).
//! * **Structure**: choices that change the equations (an enum such as
//!   Road Load From, a tick box such as Locked, a table's data until the
//!   IR has runtime tables, which optional inputs are wired) are a
//!   block's *configuration*: its generator builds a definition for them,
//!   named with a fingerprint when it differs from the default. Plain
//!   numbers stay parameters (runtime inputs; today's `variability: fixed`
//!   ones are marked structural).
//!
//! Each block documents where it follows today's engine exactly and where
//! it is more exact (events instead of a 10 ms step, exact load transfer,
//! stick/slip friction instead of a smoothed band).

pub mod battery;
pub mod catalog;
pub mod driveline;
pub mod electric;
pub mod engine;
pub mod motor;
pub mod signals;
pub mod vehicle;

use crate::x::*;
use catalog::{AppUnit, app_unit};
use lsim_ir::expr::Expr;
use lsim_ir::{ComponentDef, ParamDecl, ParamValue, PortDecl};

/// The text a display unit is written in for the unit parser, if it
/// parses (`1/min` is revolutions per minute, `g` standard gravity).
pub fn display_text(app: &str) -> Option<String> {
    let t = match app {
        "" | "-" | "kg/kg" => return None,
        "1/min" => "rev/min".to_string(),
        "g" => "gn".to_string(),
        "°C" => "degC".to_string(),
        "N/(km/h)²" => "N.h2/km2".to_string(),
        "1/(km/h·s)" => "h/(km.s)".to_string(),
        "1/(km/h)" => "h/km".to_string(),
        "N/(km/h)" => "N.h/km".to_string(),
        other => other.to_string(),
    };
    lsim_ir::units::parse_unit(&t).ok().map(|_| t)
}

/// A catalog parameter of block `id` as an IR parameter: today's key, SI
/// unit, today's unit as the display unit, today's default.
pub fn cp(id: &str, key: &str) -> ParamDecl {
    let entry = catalog::param(id, key).unwrap_or_else(|| panic!("{id} has no parameter {key}"));
    let unit_text = entry["unit"].as_str().unwrap_or("");
    let au = app_unit(unit_text);
    ParamDecl {
        name: key.into(),
        unit: au.si.into(),
        display_unit: display_text(unit_text),
        default: ParamValue::Real(Expr::Const(catalog::default_si(id, key))),
        min: None,
        max: None,
        structural: entry["variability"] == "fixed",
        doc: entry["label"].as_str().unwrap_or(key).to_string(),
    }
}

/// Several catalog parameters.
pub fn cps(id: &str, keys: &[&str]) -> Vec<ParamDecl> {
    keys.iter().map(|k| cp(id, k)).collect()
}

/// A port's SI unit text from its unit group.
pub fn port_si(id: &str, port: &str) -> &'static str {
    catalog::port_unit(id, port).si
}

/// A port's app unit.
pub fn port_app(id: &str, port: &str) -> AppUnit {
    catalog::port_unit(id, port)
}

/// The catalog port's name (for docs).
fn port_name(id: &str, port: &str) -> String {
    catalog::block(id)
        .and_then(|b| b["ports"].as_array())
        .and_then(|ps| ps.iter().find(|p| p["id"] == port))
        .and_then(|p| p["name"].as_str())
        .unwrap_or(port)
        .to_string()
}

/// A signal output of block `id`, in SI.
pub fn out(id: &str, port: &str) -> PortDecl {
    output(port, port_si(id, port), &port_name(id, port))
}

/// A signal input of block `id`, in SI.
pub fn inp(id: &str, port: &str) -> PortDecl {
    input(port, port_si(id, port), &port_name(id, port))
}

/// A physical port of block `id`.
pub fn phys(id: &str, port: &str, connector: &str) -> PortDecl {
    lsim_ir::component::build::port(port, connector, &port_name(id, port))
}

/// The block's description from the catalog.
pub fn doc(id: &str) -> String {
    catalog::block(id).and_then(|b| b["description"].as_str()).unwrap_or("").to_string()
}

/// A definition's name: `base` for the default configuration, else
/// `base_<fingerprint of the configuration>`.
pub fn variant(base: &str, is_default: bool, config: impl IntoIterator<Item = f64>) -> String {
    if is_default {
        base.to_string()
    } else {
        format!("{base}_{}", crate::signal::fingerprint(config))
    }
}

/// A variable with a start guess (an iteration variable's first value).
pub fn guess(name: &str, unit: &str, start: Expr, doc: &str) -> lsim_ir::VarDecl {
    let mut v = var(name, unit, doc);
    v.start = Some(start);
    v
}

/// Every block in its default configuration.
pub fn defaults() -> Vec<ComponentDef> {
    let mut v = vec![];
    v.extend(electric::defaults());
    v.extend(battery::defaults());
    v.extend(motor::defaults());
    v.extend(vehicle::defaults());
    v.extend(driveline::defaults());
    v.extend(engine::defaults());
    v.extend(signals::defaults());
    v
}

/// The block definition names by today's component id, in their default
/// configuration.
pub fn default_names() -> Vec<(&'static str, &'static str)> {
    vec![
        ("vehicle.body", "Blocks.VehicleBody"),
        ("driver.driver", "Blocks.Driver"),
        ("battery.generic", "Blocks.Battery"),
        ("motor.emotor", "Blocks.EMotor"),
        ("controller.dcdc", "Blocks.DcDc"),
        ("electric.voltage_source", "Blocks.VoltageSource"),
        ("electric.node", "Blocks.ElectricNode"),
        ("electric.constant_drive", "Blocks.PowerConsumer"),
        ("electric.climate", "Blocks.ClimateControl"),
        ("boundary.ground", "Blocks.Ground"),
        ("mech.shaft", "Blocks.Shaft"),
        ("mech.final_drive", "Blocks.FinalDrive"),
        ("mech.differential", "Blocks.Differential"),
        ("mech.node", "Blocks.MechNode"),
        ("propulsion.wheel", "Blocks.Wheel"),
        ("mech.brake", "Blocks.Brake"),
        ("propulsion.propeller", "Blocks.Propeller"),
        ("signal.driving_task", "Blocks.DrivingTask"),
        ("signal.constant", "Blocks.Constant"),
        ("signal.script", "Blocks.Script"),
        ("signal.fmu", "Blocks.Fmu"),
        ("signal.monitor", "Blocks.Monitor"),
        ("boundary.ambient", "Blocks.Ambient"),
        ("container.system", "Blocks.System"),
        ("engine.combustion", "Blocks.CombustionEngine"),
        ("fuel.tank", "Blocks.FuelTank"),
        ("mech.gearbox", "Blocks.Gearbox"),
        ("mech.clutch", "Blocks.Clutch"),
        ("mech.transfer_case", "Blocks.TransferCase"),
        ("fuelcell.stack", "Blocks.FuelCell"),
        ("fuel.h2_tank", "Blocks.H2Tank"),
        ("control.traction", "Blocks.TractionControl"),
        ("control.pid", "Blocks.Pid"),
        ("signal.lookup", "Blocks.Lookup"),
        ("signal.road_profile", "Blocks.RoadProfile"),
        ("track.lap", "Blocks.RaceTrack"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_block_has_a_default_definition() {
        let defs: Vec<String> = defaults().into_iter().map(|d| d.name).collect();
        let ids: Vec<&str> = catalog::catalog()["components"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids.len(), 36);
        for id in ids {
            let (_, name) = default_names().into_iter().find(|(i, _)| *i == id).expect(id);
            assert!(defs.iter().any(|d| d == name), "{id}: {name} missing");
        }
    }
}
