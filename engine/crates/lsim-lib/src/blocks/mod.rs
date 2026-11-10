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
        ("signal.script", "External.Script"),
        ("signal.fmu", "External.Fmu"),
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
        ("control.traction", "External.TractionControl"),
        ("control.pid", "Blocks.Pid"),
        ("signal.lookup", "Blocks.Lookup"),
        ("signal.road_profile", "Blocks.RoadProfile"),
        ("track.lap", "Blocks.RaceTrack"),
    ]
}

/// A table read in a block's equations.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TableSite {
    /// the block, by today's component id (`motor.emotor`)
    pub component: &'static str,
    /// its table parameter (`power_loss`)
    pub table: String,
    /// the abscissa each axis is read at, as the equations write it
    /// (`wa`, `noEvent(abs(T))`)
    pub at: Vec<String>,
}

/// Every table read in the blocks' equations (in an equation, a
/// parameter's binding, a start value, the energy books), each once,
/// sorted: the blocks in their default configuration, and those that read
/// tables in every combination of the structural choices their
/// configurations hold. The new path's judge of a table read past its data
/// (`backend/app/solver/new_engine.py`, BRANCH_READS) holds a rule for each
/// read; a configuration field that changes which tables a block reads, or
/// where, is enumerated here.
pub fn table_sites() -> Vec<TableSite> {
    let names = default_names();
    let mut defs: Vec<(&'static str, ComponentDef)> = defaults()
        .into_iter()
        .filter_map(|d| names.iter().find(|(_, n)| *n == d.name).map(|(id, _)| (*id, d)))
        .collect();
    // every combination of n choices
    let all =
        |n: u32| (0..1u32 << n).map(move |b| (0..n).map(|i| b >> i & 1 == 1).collect::<Vec<_>>());
    for f in all(2) {
        let cfg = motor::MotorConfig { windowed: f[0], inverse: f[1], ..Default::default() };
        defs.push(("motor.emotor", motor::emotor(&cfg)));
    }
    for f in all(4) {
        let cfg = engine::EngineConfig {
            throttle_wired: f[0],
            enable_wired: f[1],
            tank: f[2],
            behind_clutch: f[3],
            ..Default::default()
        };
        defs.push(("engine.combustion", engine::engine(&cfg)));
    }
    for tank in [false, true] {
        let cfg = engine::FuelCellConfig { tank, ..Default::default() };
        defs.push(("fuelcell.stack", engine::fuel_cell(&cfg)));
    }
    for f in all(3) {
        let cfg = electric::ClimateConfig {
            heat_pump: f[0],
            enable_wired: f[1],
            served_input: f[2],
            ..Default::default()
        };
        defs.push(("electric.climate", electric::climate(&cfg)));
    }
    let limits = std::iter::once(None).chain(all(5).map(|f| {
        Some(battery::PackLimits { i_dis: f[0], i_ch: f[1], v_min: f[2], v_max: f[3], band: f[4] })
    }));
    for limits in limits {
        for f in all(3) {
            let cfg = battery::BatteryConfig {
                rc: f[0],
                limits,
                limit_enforced: f[1],
                inverse: f[2],
                ..Default::default()
            };
            defs.push(("battery.generic", battery::battery(&cfg)));
        }
    }
    let p = &catalog::param("signal.lookup", "table_2d").expect("table_2d")["default"];
    let t2 = crate::table::Table2::from_json(p, Default::default(), Default::default());
    let d2 = signals::LookupConfig::D2(t2.expect("the default 2D table"));
    defs.push(("signal.lookup", signals::lookup(&d2)));
    for f in all(2) {
        let (distance, repeat) = (f[0], f[1]);
        let task = signals::ProfileConfig { distance, repeat, ..signals::default_task() };
        defs.push(("signal.driving_task", signals::driving_task(&task)));
        let road = signals::ProfileConfig { distance, repeat, ..signals::default_road() };
        defs.push(("signal.road_profile", signals::road_profile(&road)));
    }
    let mut out = std::collections::BTreeSet::new();
    for (id, d) in &defs {
        let id = *id;
        for e in expressions(d) {
            e.walk(&mut |x| {
                if let Expr::Table { args, .. } = x
                    && let Some((Expr::Name(table), at)) = args.split_first()
                {
                    let at = at.iter().map(ToString::to_string).collect();
                    out.insert(TableSite { component: id, table: table.clone(), at });
                }
            });
        }
    }
    out.into_iter().collect()
}

/// Every expression of a definition: its equations' and initial
/// equations', its parameters' bindings, its variables' start values, its
/// energy books, its impulses and engagements, its sub-components'
/// modifiers.
fn expressions(d: &ComponentDef) -> Vec<&Expr> {
    use lsim_ir::{Equation, WhenAction};
    fn real(v: &ParamValue) -> Option<&Expr> {
        match v {
            ParamValue::Real(e) => Some(e),
            _ => None,
        }
    }
    let mut v: Vec<&Expr> = vec![];
    for e in d.equations.iter().chain(&d.initial_equations) {
        match &e.eq {
            Equation::Eq { lhs, rhs } => v.extend([lhs, rhs]),
            Equation::When { condition, actions } => {
                v.push(condition);
                v.extend(actions.iter().map(|a| match a {
                    WhenAction::Assign { value, .. } | WhenAction::Reinit { value, .. } => value,
                }));
            }
            Equation::Assert { condition, .. } => v.push(condition),
        }
    }
    v.extend(d.params.iter().filter_map(|p| real(&p.default)));
    v.extend(d.vars.iter().filter_map(|x| x.start.as_ref()));
    v.extend(d.energy.stored.iter().chain(&d.energy.loss));
    v.extend(d.impulse.iter().flat_map(|i| [&i.keep, &i.active]));
    v.extend(d.engagements.iter().map(|g| &g.changes));
    v.extend(d.components.iter().flat_map(|s| s.modifiers.iter().filter_map(|m| real(&m.value))));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_table_a_block_holds_is_read() {
        // each table parameter of each block has a read the list holds
        // (one read only in a configuration the list leaves out would be
        // missed by the judge's rules), and the reads of a configuration
        // that is not the default are there: the fuel cell's curve at its
        // maximum current, a windowed motor's loss map at no torque, the
        // 2D lookup's second input
        let sites = table_sites();
        let defs = defaults();
        for (id, name) in default_names() {
            let Some(d) = defs.iter().find(|d| d.name == name) else { continue };
            for p in d.params.iter().filter(|p| p.default.table().is_some()) {
                assert!(
                    sites.iter().any(|s| s.component == id && s.table == p.name),
                    "{id}.{}",
                    p.name
                );
            }
        }
        let has = |component: &str, table: &str, at: &[&str]| {
            sites.iter().any(|s| s.component == component && s.table == table && s.at == at)
        };
        assert!(has("fuelcell.stack", "polarization", &["I"]));
        assert!(has("fuelcell.stack", "polarization", &["i_max"]));
        assert!(has("motor.emotor", "power_loss", &["noEvent(min(wa, w_max))", "0 * unit_N_m"]));
        assert!(has("signal.lookup", "table", &["sig_x_in", "sig_y_in"]));
        assert!(sites.windows(2).all(|w| w[0] < w[1]), "sorted, each once");
    }

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
