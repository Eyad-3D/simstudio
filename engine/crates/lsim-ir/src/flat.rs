//! The flat system: every variable, parameter and equation of a model with
//! the hierarchy flattened away, each one remembering where it came from
//! (its [`Origin`]) so a fault can be told in terms of the parts the user
//! placed.

use crate::component::VarKind;
use crate::expr::Expr;
use crate::units::Unit;
use serde::{Deserialize, Serialize};

/// Index of a flat variable.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub struct VarId(pub u32);

/// Index of a parameter.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub struct ParamId(pub u32);

/// Index of a component instance.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub struct InstanceId(pub u32);

/// A component instance in the flattened tree.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Instance {
    /// dotted path from the top (`battery.R0`); empty for the top
    pub path: String,
    /// its definition's name
    pub def: String,
    /// the enclosing instance
    pub parent: Option<InstanceId>,
    /// the name people see (a diagram part's label)
    pub label: Option<String>,
    /// the app's element id
    pub ui_id: Option<String>,
}

/// What a variable is to its component.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum VarRole {
    /// a declared variable
    Local,
    /// the across quantity of a physical port
    Across {
        /// the port's name
        port: String,
    },
    /// the through quantity of a physical port
    Through {
        /// the port's name
        port: String,
    },
    /// a signal input
    Input,
    /// a signal output
    Output,
}

/// A flat variable.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct FlatVar {
    /// full dotted name (`battery.R0.p.v`)
    pub name: String,
    /// its unit (values are SI inside the engine)
    pub unit: Unit,
    /// its unit as written, for display
    pub unit_text: String,
    /// continuous or discrete
    pub kind: VarKind,
    /// start value, SI
    pub start: Option<f64>,
    /// whether the start value must hold
    pub fixed: bool,
    /// typical magnitude, SI (1 when not given)
    pub nominal: f64,
    /// the instance it belongs to
    pub instance: InstanceId,
    /// what it is to that instance
    pub role: VarRole,
}

/// A parameter of the flat system.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct FlatParam {
    /// full dotted name
    pub name: String,
    /// its unit
    pub unit: Unit,
    /// its value, SI
    pub value: f64,
    /// when bound to other parameters: that expression (flat scope), which
    /// the run re-evaluates whenever a parameter changes
    pub binding: Option<Expr>,
    /// whether it is structural (part of the cache key)
    pub structural: bool,
    /// the instance it belongs to
    pub instance: InstanceId,
}

/// Which rule made an equation.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub enum OriginKind {
    /// the n-th equation of the instance's definition
    Component {
        /// its index in `ComponentDef::equations`
        index: usize,
    },
    /// the across quantities of a connection set are equal
    ConnectionAcross {
        /// the ports of the set, `instance.port`
        ports: Vec<String>,
    },
    /// the through quantities of a connection set sum to zero
    ConnectionThrough {
        /// the ports of the set
        ports: Vec<String>,
    },
    /// a port with nothing connected carries no flow
    Unconnected {
        /// the port
        port: String,
    },
    /// a signal link: input = output
    SignalLink {
        /// the input
        input: String,
        /// the output
        output: String,
    },
}

/// Where an equation came from.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Origin {
    /// the instance whose definition holds it (for connections: the
    /// instance whose `connect` made it)
    pub instance: InstanceId,
    /// which rule
    pub kind: OriginKind,
    /// the equation's plain-words label, if it has one
    pub label: Option<String>,
}

/// A flat equation: lhs = rhs, in flat scope.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct FlatEquation {
    /// left side
    pub lhs: Expr,
    /// right side
    pub rhs: Expr,
    /// where it came from
    pub origin: Origin,
}

/// A flat `when` clause.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct FlatWhen {
    /// its condition
    pub condition: Expr,
    /// discrete variable := value, in order
    pub assign: Vec<(VarId, Expr)>,
    /// state := value (restarts)
    pub reinit: Vec<(VarId, Expr)>,
    /// where it came from
    pub origin: Origin,
}

/// The power flowing into an instance through one of its ports.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct PortPower {
    /// the instance
    pub instance: InstanceId,
    /// the port
    pub port: String,
    /// the power, W (flat scope)
    pub power: Expr,
}

/// What an instance stores and loses (its definition's [`crate::EnergyDecl`],
/// in flat scope).
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct InstanceEnergy {
    /// the instance
    pub instance: InstanceId,
    /// energy stored, J
    pub stored: Option<Expr>,
    /// power lost to heat, W
    pub loss: Option<Expr>,
}

/// The flat system.
#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
pub struct FlatSystem {
    /// component instances; index 0 is the top
    pub instances: Vec<Instance>,
    /// variables
    pub vars: Vec<FlatVar>,
    /// parameters
    pub params: Vec<FlatParam>,
    /// equations
    pub equations: Vec<FlatEquation>,
    /// `when` clauses
    pub whens: Vec<FlatWhen>,
    /// equations that hold at the start only
    pub initial_equations: Vec<FlatEquation>,
    /// port powers of primitive instances, for the energy books
    pub port_powers: Vec<PortPower>,
    /// stored energy and losses of the instances that declare them
    pub energy: Vec<InstanceEnergy>,
}

impl FlatSystem {
    /// The variable's record.
    pub fn var(&self, v: VarId) -> &FlatVar {
        &self.vars[v.0 as usize]
    }

    /// The instance's record.
    pub fn instance(&self, i: InstanceId) -> &Instance {
        &self.instances[i.0 as usize]
    }

    /// How a person would name the instance: its label, else its path.
    pub fn instance_name(&self, i: InstanceId) -> String {
        let inst = self.instance(i);
        match (&inst.label, inst.path.is_empty()) {
            (Some(l), _) => format!("'{l}'"),
            (None, true) => "the model".to_string(),
            (None, false) => format!("'{}'", inst.path),
        }
    }

    /// The outermost instance below the top that contains `i`: the part on
    /// the user's diagram (`battery` for `battery.R0`).
    pub fn top_part(&self, mut i: InstanceId) -> InstanceId {
        while let Some(p) = self.instance(i).parent {
            if p.0 == 0 {
                return i;
            }
            i = p;
        }
        i
    }

    /// The variable's id by full name.
    pub fn find_var(&self, name: &str) -> Option<VarId> {
        self.vars.iter().position(|v| v.name == name).map(|i| VarId(i as u32))
    }

    /// The parameter's id by full name.
    pub fn find_param(&self, name: &str) -> Option<ParamId> {
        self.params.iter().position(|p| p.name == name).map(|i| ParamId(i as u32))
    }
}
