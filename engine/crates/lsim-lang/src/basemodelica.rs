//! Base Modelica import (work in progress).

use crate::LangError;
use lsim_ir::component::ComponentDef;

/// Imports a Base Modelica model.
pub fn import(_text: &str) -> Result<ComponentDef, Vec<LangError>> {
    Err(vec![])
}
