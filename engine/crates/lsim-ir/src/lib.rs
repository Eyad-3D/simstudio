//! # lsim-ir: the engine's shared types
//!
//! Every crate of the engine talks through the types defined here, so the
//! work packages can be built in parallel against a fixed contract
//! (DESIGN.md, *Interfaces*):
//!
//! | module | what it holds | produced by | consumed by |
//! |---|---|---|---|
//! | [`units`] | SI dimensions, unit strings, conversion | – | everyone |
//! | [`expr`] | the expression tree of equations | lsim-lang, lsim-lib | lsim-prep, lsim-codegen |
//! | [`component`] | connectors, ports, parameters, variables, equations, composition | lsim-lang, lsim-lib, lsim-project | lsim-prep |
//! | [`flat`] | the flattened system with every equation's origin | lsim-prep (flatten) | lsim-prep (analysis), diagnostics |
//! | [`prepared`] | the causalised, sorted model | lsim-prep | lsim-codegen, lsim-fast |
//! | [`runtime`] | the compiled model's functions, as the solver sees them | lsim-codegen | lsim-solve, lsim-fast |
//! | [`diag`] | plain-language diagnostics | everyone | the app |
//! | [`eval`] | a reference interpreter for expressions | – | tests, constant folding |
//!
//! Changes to these types after Stage 1 are additive, or agreed between the
//! owners of the crates that use them.

pub mod component;
pub mod diag;
pub mod eval;
pub mod expr;
pub mod flat;
pub mod prepared;
pub mod runtime;
pub mod units;

pub use component::{
    ComponentDef, Connect, ConnectorDef, EnergyDecl, Equation, EquationDecl, Library, Modifier,
    ParamDecl, ParamValue, PortDecl, PortKind, PowerRule, QuantityDecl, SubDecl, VarDecl, VarKind,
    WhenAction,
};
pub use diag::{Diagnostic, Severity};
pub use expr::{BinaryOp, Builtin, CmpOp, Expr};
pub use flat::{
    FlatEquation, FlatParam, FlatSystem, FlatVar, FlatWhen, Instance, InstanceEnergy, InstanceId,
    Origin, OriginKind, ParamId, PortPower, VarId, VarRole,
};
pub use prepared::{
    AliasEntry, AliasTarget, Assignment, Direction, ExternalBlock, InverseSpec, Mode, PrepStats,
    PreparedModel, PreparedWhen, Residual, Slot, ZeroCrossing,
};
pub use runtime::{DiscreteBlock, EvalInput, Layout, ModelFunctions, SparsityPattern};
pub use units::{Dim, Unit, UnitError};
