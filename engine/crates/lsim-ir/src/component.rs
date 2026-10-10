//! Component definitions: what a library part, a user's equation component
//! or a whole model is before flattening.
//!
//! A [`ComponentDef`] declares ports, parameters and variables, may contain
//! sub-components wired by [`Connect`]s, and adds its own equations. A
//! composite (the battery made of a source, R0 and an RC pair, or a whole
//! vehicle) and a primitive (a resistor) are the same type; a project's
//! diagram becomes one top-level `ComponentDef` (lsim-project).
//!
//! Physical ports carry an across quantity (equal at a connection: voltage,
//! speed, velocity, temperature) and a through quantity (summing to zero at
//! a connection: current, torque, force, heat flow) — the owner's
//! effort/flow pairs. Through quantities are positive *into* the component.
//! Signal ports are causal: an output drives any number of inputs.
//!
//! Numbers in the IR are SI: declared units must be coherent SI units
//! (scale 1, no offset), as Base Modelica's `unit` attribute is in
//! practice; `km/h`, `kW`, `rev/min`, `%` or `°C` are display units only,
//! converted where values enter or leave the engine (project import,
//! results).

use crate::expr::Expr;
pub use crate::table::{Interpolation, Outside, TableData};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// How a connector's port power is formed, for the energy books.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum PowerRule {
    /// power into the component = across × through (electrical,
    /// mechanical: v·i, w·tau, v·f)
    AcrossTimesThrough,
    /// the through quantity is itself a power (thermal: heat flow, W)
    ThroughIsPower,
}

/// One quantity of a connector.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct QuantityDecl {
    /// its name on the port (`v`, `i`, `w`, `tau`, `T`, `Q`)
    pub name: String,
    /// its unit text (`V`, `A`, `rad/s`, `N.m`, `K`, `W`)
    pub unit: String,
}

/// A physical connector type: an across/through pair.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct ConnectorDef {
    /// `Pin`, `Flange`, `TFlange`, `HeatPort` …
    pub name: String,
    /// equal at a connection
    pub across: QuantityDecl,
    /// sums to zero at a connection; positive into the component
    pub through: QuantityDecl,
    /// how its power is formed
    pub power: PowerRule,
    /// what it is
    pub doc: String,
}

/// What a port is.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub enum PortKind {
    /// a physical port of the named connector type
    Physical {
        /// the [`ConnectorDef`]'s name
        connector: String,
    },
    /// a causal signal input
    Input {
        /// its unit text
        unit: String,
    },
    /// a causal signal output
    Output {
        /// its unit text
        unit: String,
    },
}

/// A port.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct PortDecl {
    /// its name; a physical port `p` brings the variables `p.<across>` and
    /// `p.<through>`, a signal port `u` the variable `u`
    pub name: String,
    /// what it is
    pub kind: PortKind,
    /// what it is for
    pub doc: String,
}

/// A parameter's value.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub enum ParamValue {
    /// a number in the parameter's unit, or an expression of other
    /// parameters of the same scope (a binding, re-evaluated at run time)
    Real(Expr),
    /// true or false (structural: it may change the equations)
    Bool(bool),
    /// one of the options of an enumeration type (structural), by its
    /// qualified name: `"Mode.Manual"` is the option `Manual` of the type
    /// `Mode` (see [`EnumType`]); the type is the text before the last dot
    Enum(String),
    /// a 1-D table: abscissae (in `axis_unit`) and values (in the unit),
    /// interpolated by the default rules ([`TableData`])
    Table1D {
        /// the abscissae, increasing
        x: Vec<f64>,
        /// the values
        y: Vec<f64>,
        /// the abscissae's unit text
        axis_unit: String,
    },
    /// a 2-D table: the values at the grid points `(x1[i], x2[j])`, in the
    /// unit, interpolated by the default rules ([`TableData`])
    Table2D {
        /// the first axis' points, increasing
        x1: Vec<f64>,
        /// the second axis' points, increasing
        x2: Vec<f64>,
        /// row-major: `values[i * x2.len() + j]` is at `(x1[i], x2[j])`
        values: Vec<f64>,
        /// the two axes' unit texts
        axis_units: [String; 2],
    },
    /// a table of one or two axes with its own interpolation and
    /// outside-the-data rules
    Table(TableData),
}

impl ParamValue {
    /// The table this value holds, with its rules (the defaults for
    /// [`ParamValue::Table1D`] and [`ParamValue::Table2D`]); `None` when it
    /// is not a table.
    pub fn table(&self) -> Option<TableData> {
        match self {
            ParamValue::Table1D { x, y, axis_unit } => Some(TableData {
                axis_units: [axis_unit.clone(), String::new()],
                ..TableData::new_1d(x.clone(), y.clone())
            }),
            ParamValue::Table2D { x1, x2, values, axis_units } => Some(TableData {
                axis_units: axis_units.clone(),
                ..TableData::new_2d(x1.clone(), x2.clone(), values.clone())
            }),
            ParamValue::Table(t) => Some(t.clone()),
            _ => None,
        }
    }
}

/// One option of an enumeration type.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct EnumLiteral {
    /// its name (`Manual`)
    pub name: String,
    /// what it means
    pub doc: String,
}

/// An enumeration type (Modelica's `type Mode = enumeration(Manual, Auto)`):
/// a parameter of this type takes one of its options. Its value is
/// [`ParamValue::Enum`] with the option's qualified name (`"Mode.Auto"`);
/// in equations the option `Mode.Auto` stands for its position, counting
/// from 1 (Modelica's `Integer(Mode.Auto)`), so `mode == Mode.Auto`
/// compares numbers.
///
/// A component declares the types it uses in [`ComponentDef::types`];
/// types shared by several components go in [`Library::types`]. A name is
/// looked up in the component that declares the parameter first, then in
/// the library.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct EnumType {
    /// its name (`Mode`, or a qualified library name `Gearbox.Mode`)
    pub name: String,
    /// its options, in order
    pub literals: Vec<EnumLiteral>,
    /// what it is
    pub doc: String,
}

impl EnumType {
    /// The position of the option `literal` (its bare name, `Auto`),
    /// counting from 1, as Modelica's `Integer()` gives it.
    pub fn ordinal(&self, literal: &str) -> Option<usize> {
        self.literals.iter().position(|l| l.name == literal).map(|i| i + 1)
    }
}

/// Splits an option's qualified name (`"Gearbox.Mode.Auto"`) into its type
/// (`"Gearbox.Mode"`) and the option (`"Auto"`).
pub fn split_enum_value(qualified: &str) -> Option<(&str, &str)> {
    qualified.rsplit_once('.')
}

/// A parameter.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct ParamDecl {
    /// its name
    pub name: String,
    /// its unit text: a coherent SI unit (`V`, `N.m`, `kg.m2`, `1`); the
    /// value is in it
    pub unit: String,
    /// the unit it is shown and typed in (`kW`, `km/h`, `rev/min`, `%`)
    pub display_unit: Option<String>,
    /// its default value
    pub default: ParamValue,
    /// the lowest allowed value, if any
    pub min: Option<f64>,
    /// the highest allowed value, if any
    pub max: Option<f64>,
    /// true when changing it may change the equations (the old library's
    /// `variability: fixed`); a structural parameter is part of the cache
    /// key, every other one is a runtime input of the compiled model
    pub structural: bool,
    /// what it is
    pub doc: String,
}

/// Continuous or discrete (piecewise constant, changed only at events).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum VarKind {
    /// solved continuously
    Continuous,
    /// changed only by `when` actions
    Discrete,
}

/// A variable.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct VarDecl {
    /// its name
    pub name: String,
    /// its unit text: a coherent SI unit
    pub unit: String,
    /// the unit it is shown in
    pub display_unit: Option<String>,
    /// continuous or discrete
    pub kind: VarKind,
    /// its start value (a state's initial value, an iteration's guess); an
    /// expression of parameters
    pub start: Option<Expr>,
    /// true: the start value is an initial condition that must hold;
    /// false: only a guess
    pub fixed: bool,
    /// its typical magnitude, for error control and scaling
    pub nominal: Option<f64>,
    /// what it is
    pub doc: String,
}

/// A parameter value given to a sub-component.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Modifier {
    /// the sub-component's parameter
    pub param: String,
    /// its value: an expression of the enclosing scope's parameters
    pub value: ParamValue,
}

/// A sub-component.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct SubDecl {
    /// its instance name (a valid identifier)
    pub name: String,
    /// the [`ComponentDef`]'s name
    pub def: String,
    /// its parameter values
    pub modifiers: Vec<Modifier>,
    /// the name people see (a diagram part's label), for messages
    pub label: Option<String>,
    /// the app's element id, when it came from a project
    pub ui_id: Option<String>,
}

/// A connection between two ports: `"R0.n"` (a sub-component's port) or
/// `"p"` (this component's own port).
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Connect {
    /// one end
    pub a: String,
    /// the other end
    pub b: String,
}

/// An action of a `when` clause.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub enum WhenAction {
    /// discrete variable := value
    Assign {
        /// the discrete variable
        var: String,
        /// its new value
        value: Expr,
    },
    /// restart a state from a new value
    Reinit {
        /// the state
        var: String,
        /// its new value
        value: Expr,
    },
}

/// An equation.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub enum Equation {
    /// lhs = rhs
    Eq {
        /// left side
        lhs: Expr,
        /// right side
        rhs: Expr,
    },
    /// when the condition becomes true, do the actions
    When {
        /// a truth-valued expression; its relations become zero crossings
        condition: Expr,
        /// what happens
        actions: Vec<WhenAction>,
    },
    /// the condition must hold; otherwise warn or stop with the message
    Assert {
        /// what must hold
        condition: Expr,
        /// what to tell the user
        message: String,
        /// true: stop the run; false: warn
        error: bool,
    },
}

/// An equation with the words used when it is part of a fault.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct EquationDecl {
    /// the equation
    pub eq: Equation,
    /// what it states, in plain words (`"Ohm's law"`, `"the speed of both
    /// flanges is the shaft's speed"`)
    pub label: Option<String>,
}

/// What a component stores and loses, for its energy books.
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
pub struct EnergyDecl {
    /// energy stored, J (½·C·v², ½·J·w², SOC·capacity·OCV …)
    pub stored: Option<Expr>,
    /// power lost to heat, W (i²·R, b·w², brake torque × speed …)
    pub loss: Option<Expr>,
}

/// A relative velocity a component keeps through an impulse while a
/// condition holds: it passes a rigid, instantaneous engagement elsewhere
/// in the model on rigidly, as if it were a rigid coupling for that
/// instant. A tyre that grips keeps its slip velocity (`keep = w*r - v`
/// while it is not at its grip limit), so a gear shift's impulse reaches
/// the vehicle; a part that carries only bounded forces (a slipping
/// clutch, a tyre at its grip limit) declares none and passes no impulse.
/// The text format writes it as `annotation(__LightSim_impulse(keep = …,
/// active = …))`.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct ImpulseDecl {
    /// the relative velocity kept (an expression of the component's
    /// variables, linear in its velocities)
    pub keep: Expr,
    /// while this holds (a truth value)
    pub active: Expr,
}

/// A rigid engagement a component makes: when `changes` takes a new value
/// at an event (a gear's selected ratio at a shift), the speeds the
/// component's rigid coupling ties together jump as an instantaneous,
/// rigid engagement makes them. The run loop then keeps the momentum of
/// everything the coupling ties together (an impulse projection, passed on
/// through the couplings that declare [`ImpulseDecl`]s) and books the
/// kinetic energy the engagement loses to this component. Nothing else
/// starts a projection: a stored energy that merely depends on a discrete
/// value (a converter's sampled duty ratio) does not, nor does a `reinit`,
/// whose restarted states the projection leaves where the `reinit` put
/// them. The text format writes it as
/// `annotation(__LightSim_engagement(changes = …))`.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct EngagementDecl {
    /// what takes a new value at an engagement (the selected ratio): an
    /// expression of the component's variables and parameters
    pub changes: Expr,
}

/// A component definition.
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
pub struct ComponentDef {
    /// its library name, e.g. `Electrical.Resistor`
    pub name: String,
    /// what it is
    pub doc: String,
    /// its ports
    pub ports: Vec<PortDecl>,
    /// its parameters
    pub params: Vec<ParamDecl>,
    /// its own variables (port variables are implied by the ports)
    pub vars: Vec<VarDecl>,
    /// its sub-components
    pub components: Vec<SubDecl>,
    /// connections among its own ports and its sub-components' ports
    pub connections: Vec<Connect>,
    /// its equations
    pub equations: Vec<EquationDecl>,
    /// equations that hold at the start only
    pub initial_equations: Vec<EquationDecl>,
    /// its energy books
    pub energy: EnergyDecl,
    /// the enumeration types it declares for its own parameters
    #[serde(default)]
    pub types: Vec<EnumType>,
    /// the relative velocities it keeps through an impulse
    #[serde(default)]
    pub impulse: Vec<ImpulseDecl>,
    /// the rigid engagements it makes
    #[serde(default)]
    pub engagements: Vec<EngagementDecl>,
}

/// A set of connector and component definitions.
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
pub struct Library {
    /// connector types by name
    pub connectors: BTreeMap<String, ConnectorDef>,
    /// components by name
    pub components: BTreeMap<String, ComponentDef>,
    /// enumeration types shared by its components, by name
    #[serde(default)]
    pub types: BTreeMap<String, EnumType>,
}

impl Library {
    /// Adds (or replaces) a connector type.
    pub fn add_connector(&mut self, c: ConnectorDef) {
        self.connectors.insert(c.name.clone(), c);
    }

    /// Adds (or replaces) a component.
    pub fn add(&mut self, c: ComponentDef) {
        self.components.insert(c.name.clone(), c);
    }

    /// Adds (or replaces) a shared enumeration type.
    pub fn add_type(&mut self, t: EnumType) {
        self.types.insert(t.name.clone(), t);
    }

    /// The enumeration type `name` as `scope` sees it: one `scope`
    /// declares, else the library's.
    pub fn enum_type<'a>(&'a self, scope: &'a ComponentDef, name: &str) -> Option<&'a EnumType> {
        scope.types.iter().find(|t| t.name == name).or_else(|| self.types.get(name))
    }

    /// The number an enumeration option (`"Mode.Auto"`) stands for, as
    /// `scope` sees its type: its position counting from 1.
    pub fn enum_ordinal(&self, scope: &ComponentDef, qualified: &str) -> Option<usize> {
        let (ty, lit) = split_enum_value(qualified)?;
        self.enum_type(scope, ty)?.ordinal(lit)
    }
}

/// Small builders that keep library definitions readable.
pub mod build {
    use super::*;

    /// A physical port.
    pub fn port(name: &str, connector: &str, doc: &str) -> PortDecl {
        PortDecl {
            name: name.into(),
            kind: PortKind::Physical { connector: connector.into() },
            doc: doc.into(),
        }
    }

    /// A real parameter with a default.
    pub fn param(name: &str, unit: &str, default: f64, doc: &str) -> ParamDecl {
        ParamDecl {
            name: name.into(),
            unit: unit.into(),
            display_unit: None,
            default: ParamValue::Real(Expr::Const(default)),
            min: None,
            max: None,
            structural: false,
            doc: doc.into(),
        }
    }

    /// A continuous variable.
    pub fn var(name: &str, unit: &str, doc: &str) -> VarDecl {
        VarDecl {
            name: name.into(),
            unit: unit.into(),
            display_unit: None,
            kind: VarKind::Continuous,
            start: None,
            fixed: false,
            nominal: None,
            doc: doc.into(),
        }
    }

    /// A state with a fixed initial value.
    pub fn state(name: &str, unit: &str, start: f64, doc: &str) -> VarDecl {
        VarDecl { start: Some(Expr::Const(start)), fixed: true, ..var(name, unit, doc) }
    }

    /// A discrete variable with a start value.
    pub fn discrete(name: &str, unit: &str, start: f64, doc: &str) -> VarDecl {
        VarDecl {
            kind: VarKind::Discrete,
            start: Some(Expr::Const(start)),
            fixed: true,
            ..var(name, unit, doc)
        }
    }

    /// lhs = rhs, with a plain-words label.
    pub fn eq(lhs: Expr, rhs: Expr, label: &str) -> EquationDecl {
        EquationDecl { eq: Equation::Eq { lhs, rhs }, label: Some(label.into()) }
    }

    /// A sub-component with real parameter values.
    pub fn sub(name: &str, def: &str, mods: &[(&str, Expr)]) -> SubDecl {
        SubDecl {
            name: name.into(),
            def: def.into(),
            modifiers: mods
                .iter()
                .map(|(p, v)| Modifier { param: (*p).into(), value: ParamValue::Real(v.clone()) })
                .collect(),
            label: None,
            ui_id: None,
        }
    }

    /// connect(a, b)
    pub fn connect(a: &str, b: &str) -> Connect {
        Connect { a: a.into(), b: b.into() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enumeration_options_count_from_one() {
        let mode = EnumType {
            name: "Mode".into(),
            literals: vec![
                EnumLiteral { name: "Manual".into(), doc: String::new() },
                EnumLiteral { name: "Auto".into(), doc: String::new() },
            ],
            doc: String::new(),
        };
        let mut lib = Library::default();
        let mut shared = mode.clone();
        shared.name = "Gearbox.Mode".into();
        shared.literals.reverse();
        lib.add_type(shared);
        let scope = ComponentDef { types: vec![mode], ..Default::default() };
        assert_eq!(lib.enum_ordinal(&scope, "Mode.Auto"), Some(2));
        assert_eq!(lib.enum_ordinal(&scope, "Gearbox.Mode.Auto"), Some(1));
        assert_eq!(lib.enum_ordinal(&scope, "Mode.Sport"), None);
        assert_eq!(split_enum_value("Gearbox.Mode.Auto"), Some(("Gearbox.Mode", "Auto")));
    }

    #[test]
    fn tables_normalise_and_check_their_shape() {
        let t1 =
            ParamValue::Table1D { x: vec![0.0, 1.0], y: vec![3.0, 4.0], axis_unit: "1".into() };
        let d = t1.table().unwrap();
        assert_eq!(d.dims(), 1);
        assert_eq!(d.interpolation, Interpolation::MonotoneCubic);
        assert_eq!(d.outside, [Outside::Clamp; 2]);
        assert_eq!(d.axis_units[0], "1");
        assert!(d.check().is_ok());
        let t2 = ParamValue::Table2D {
            x1: vec![0.0, 1.0],
            x2: vec![0.0, 10.0, 20.0],
            values: vec![1.0; 6],
            axis_units: ["rad/s".into(), "N.m".into()],
        };
        assert!(t2.table().unwrap().check().is_ok());
        let mut bad = t2.table().unwrap();
        bad.values.pop();
        assert_eq!(bad.check().unwrap_err(), "the table has 6 grid points but 5 values");
        bad.values.push(1.0);
        bad.y = vec![0.0, 20.0, 10.0];
        assert!(bad.check().unwrap_err().contains("increase strictly, but 20 is followed by 10"));
        assert_eq!(ParamValue::Bool(true).table(), None);
    }
}
