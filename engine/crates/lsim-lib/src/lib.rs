//! # lsim-lib: the component library in the engine's IR
//!
//! Connectors and the physical primitives every model is built from
//! (electrical, rotational, translational, thermal, fuel, signal), the
//! composites made of them, and the 36 ready-made blocks of today's
//! library (`backend/app/library/components.json`) with today's parameters,
//! ports and recorded channels ([`blocks`]).
//!
//! Sign convention: a port's through quantity (current, torque, force, heat
//! flow, fuel mass flow) is positive *into* the component.
//!
//! Energy books: every primitive declares what it stores and loses
//! ([`lsim_ir::EnergyDecl`]); its port powers are formed by its
//! connectors, so `Σ port power = loss + d(stored)/dt` holds for each part
//! and, connection by connection, for any model built from them. Tests in
//! `lsim-project` check this by simulation for every component.
//!
//! Stand-ins (until work packages 1–4 deliver them): tables are expanded
//! into expressions ([`table`]); relations in equations are meant to become
//! modes (work package 2) and `when` conditions are single comparisons.

pub mod blocks;
pub mod connectors;
pub mod electrical;
pub mod fuel;
pub mod rotational;
pub mod signal;
pub mod table;
pub mod thermal;
pub mod translational;
pub mod x;

pub use connectors::connectors;
pub use electrical::electrical;
pub use fuel::fuel;
pub use rotational::rotational;
pub use signal::signal;
pub use thermal::thermal;
pub use translational::translational;

use lsim_ir::{ComponentDef, Library};
use x::*;

/// Composites built from the primitives.
pub fn composites() -> Vec<ComponentDef> {
    let battery = ComponentDef {
        name: "Battery.OcvR0Rc".into(),
        doc: "Equivalent-circuit battery: constant open-circuit voltage, series resistance R0 \
              and one RC pair (R1 || C1)."
            .into(),
        ports: vec![port("p", "Pin", "positive terminal"), port("n", "Pin", "negative terminal")],
        params: vec![
            p("ocv", "V", 400.0, "open-circuit voltage"),
            p("r0", "Ohm", 0.05, "series resistance"),
            p("r1", "Ohm", 0.01, "RC pair resistance"),
            p("c1", "F", 0.01, "RC pair capacitance"),
        ],
        components: vec![
            sub("source", "Electrical.ConstantVoltage", &[("V", n("ocv"))]),
            sub("r0", "Electrical.Resistor", &[("R", n("r0"))]),
            sub("r1", "Electrical.Resistor", &[("R", n("r1"))]),
            sub("c1", "Electrical.Capacitor", &[("C", n("c1"))]),
        ],
        connections: vec![
            connect("n", "source.n"),
            connect("source.p", "r0.p"),
            connect("r0.n", "r1.p"),
            connect("r0.n", "c1.p"),
            connect("r1.n", "c1.n"),
            connect("r1.n", "p"),
        ],
        ..Default::default()
    };
    vec![battery]
}

/// Every primitive and composite, without the vehicle blocks.
pub fn primitives() -> Vec<ComponentDef> {
    let mut v = electrical();
    v.extend(rotational());
    v.extend(translational());
    v.extend(thermal());
    v.extend(fuel());
    v.extend(signal());
    v.extend(composites());
    v
}

/// The whole library: connectors, primitives, composites and the vehicle
/// blocks in their default configuration.
pub fn library() -> Library {
    let mut lib = Library::default();
    for c in connectors() {
        lib.add_connector(c);
    }
    for c in primitives().into_iter().chain(blocks::defaults()) {
        lib.add(c);
    }
    lib
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_is_consistent() {
        let lib = library();
        assert_eq!(lib.connectors.len(), 5);
        for def in lib.components.values() {
            for p in &def.ports {
                if let lsim_ir::PortKind::Physical { connector } = &p.kind {
                    assert!(lib.connectors.contains_key(connector), "{}: {connector}", def.name);
                }
            }
            for s in &def.components {
                assert!(lib.components.contains_key(&s.def), "{}: {}", def.name, s.def);
            }
            for prm in &def.params {
                let u = lsim_ir::units::parse_unit(&prm.unit).expect("parameter unit");
                assert_eq!((u.scale, u.offset), (1.0, 0.0), "{}.{} is not SI", def.name, prm.name);
                if let Some(d) = &prm.display_unit {
                    lsim_ir::units::parse_unit(d).unwrap_or_else(|e| {
                        panic!("{}.{} display unit {d}: {e}", def.name, prm.name)
                    });
                }
            }
            for v in &def.vars {
                let u = lsim_ir::units::parse_unit(&v.unit).expect("variable unit");
                assert_eq!((u.scale, u.offset), (1.0, 0.0), "{}.{} is not SI", def.name, v.name);
            }
        }
    }
}
