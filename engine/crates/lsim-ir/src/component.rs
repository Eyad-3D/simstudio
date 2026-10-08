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
    /// one of the declared options (structural)
    Enum(String),
    /// a 1-D table: abscissae (in `axis_unit`) and values (in the unit)
    Table1D {
        /// the abscissae, increasing
        x: Vec<f64>,
        /// the values
        y: Vec<f64>,
        /// the abscissae's unit text
        axis_unit: String,
    },
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
}

/// A set of connector and component definitions.
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
pub struct Library {
    /// connector types by name
    pub connectors: BTreeMap<String, ConnectorDef>,
    /// components by name
    pub components: BTreeMap<String, ComponentDef>,
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
