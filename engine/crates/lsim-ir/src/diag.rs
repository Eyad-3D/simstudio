//! Diagnostics: what goes wrong, told in terms of the parts on the user's
//! diagram (DESIGN.md, *Error messages*).

use serde::{Deserialize, Serialize};
use std::fmt;

/// How bad it is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Severity {
    /// the model cannot run
    Error,
    /// it runs, but something is off
    Warning,
    /// for information
    Info,
}

/// One diagnostic.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Diagnostic {
    /// how bad it is
    pub severity: Severity,
    /// a stable code (`STRUCT-OVER`, `UNIT-MISMATCH` …) for tests and help
    /// links; the message may change, the code does not
    pub code: String,
    /// the plain-language message, naming the parts by their labels
    pub message: String,
    /// the parts involved: instance paths (the app highlights them)
    pub parts: Vec<String>,
    /// what to do about it
    pub hint: Option<String>,
    /// the technical detail (equations, variables), for the "details" fold
    pub detail: Vec<String>,
}

impl Diagnostic {
    /// An error with a code and a message.
    pub fn error(code: &str, message: impl Into<String>) -> Self {
        Diagnostic {
            severity: Severity::Error,
            code: code.into(),
            message: message.into(),
            parts: vec![],
            hint: None,
            detail: vec![],
        }
    }

    /// Adds a hint.
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)?;
        if let Some(h) = &self.hint {
            write!(f, " {h}")?;
        }
        Ok(())
    }
}
