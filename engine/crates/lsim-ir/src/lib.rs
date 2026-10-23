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
//! | [`choice`] | how index reduction's choice of states stands at a point | – | lsim-prep, lsim-solve |
//! | [`eval`] | a reference interpreter for expressions | – | tests, constant folding |
//! | [`math`] | the one mathematical library (lsim-math: correctly rounded, the same bits everywhere) | – | everyone who evaluates |
//! | [`table`] | 1-D and 2-D table data and rules (runtime parameters) | lsim-lang, lsim-lib, lsim-project, lsim-prep (flatten) | lsim-prep, lsim-codegen |
//!
//! Changes to these types after Stage 1 are additive, or agreed between the
//! owners of the crates that use them.

pub mod choice;
pub mod component;
pub mod diag;
pub mod eval;
pub mod expr;
pub mod fenv;
pub mod flat;
pub mod interp;
pub mod interval;
pub mod prepared;
pub mod runtime;
pub mod table;
pub mod units;

/// The engine's one mathematical library (the `lsim-math` crate): every
/// transcendental function the engine evaluates goes through it.
pub use lsim_math as math;

pub use component::{
    ComponentDef, Connect, ConnectorDef, EnergyDecl, EngagementDecl, EnumLiteral, EnumType,
    Equation, EquationDecl, ImpulseDecl, Library, Modifier, ParamDecl, ParamValue, PortDecl,
    PortKind, PowerRule, QuantityDecl, StateSelect, SubDecl, VarDecl, VarKind, WhenAction,
};
pub use diag::{Diagnostic, Severity};
pub use expr::{BinaryOp, Builtin, CmpOp, Expr};
pub use flat::{
    FlatAssert, FlatEngagement, FlatEquation, FlatImpulse, FlatParam, FlatRestart, FlatSystem,
    FlatVar, FlatWhen, Instance, InstanceEnergy, InstanceId, Origin, OriginKind, ParamId,
    PortPower, VarId, VarRole,
};
pub use prepared::{
    AliasEntry, AliasTarget, Assignment, Direction, ExternalBlock, HeldAxis, HeldRead, InitSystem,
    InverseSpec, LimitSite, Mode, ParamGuard, PrepStats, PreparedModel, PreparedWhen, Residual,
    Slot, StateSet, ZeroCrossing,
};
pub use runtime::{
    BooksCover, BooksKernel, BooksPart, BooksSpec, ConditionKernels, DiscreteBlock, Enclosure,
    EvalInput, InitFunctions, Layout, ModelFunctions, RateSource, SparsityPattern, TableGuard,
};
pub use table::{FlatTable, Interpolation, Outside, TableData};
pub use units::{Dim, Unit, UnitError};
