//! # lsim-lang: the text format of components, and Base Modelica import
//!
//! Users write their own physical components in a small format that
//! follows Modelica's syntax (Base Modelica, the flat Modelica of the
//! Modelica Association's MCP-0031, is the reference), so every component
//! maps one-to-one to it and Base Modelica import is the same parser with
//! more of the grammar:
//!
//! ```text
//! model Electrical.Resistor "Ideal linear resistor."
//!   connector p: Pin "positive terminal (current flows in here)";
//!   connector n: Pin "negative terminal";
//!   parameter Real R(unit = "Ohm") = 1 "resistance";
//!   Real v(unit = "V") "voltage across it";
//!   Real i(unit = "A") "current from p to n";
//! equation
//!   v = p.v - n.v "the voltage across it is p.v - n.v";
//!   0 = p.i + n.i "the current into p leaves at n";
//!   i = p.i "its current is the current into p";
//!   v = R * i "Ohm's law";
//!   annotation(__LightSim_energy(loss = v * i));
//! end Electrical.Resistor;
//! ```
//!
//! * declarations: `parameter Real`, `structural parameter`, `parameter
//!   Boolean`, a parameter of an enumeration type, a table parameter
//!   (`= table(x = {…}, y = {…}, xUnit = "…")`), `Real`, `discrete Real`,
//!   `input Real`, `output Real`; units are checked (coherent SI, with
//!   `displayUnit` for what people read);
//! * `connector p: Pin` declares a physical port of a connector type (an
//!   across/through pair); sub-components are `Lib.Name x(p = …)`;
//! * equations: `lhs = rhs "plain-words label"`, `connect(a, b)`,
//!   `when cond then v = expr; end when;`, `if` equations,
//!   `assert(cond, "message")`, an `initial equation` section;
//! * expressions: `+ - * / ^`, comparisons with `==` and `<>`, `and or
//!   not`, `if … then … elseif … else …`, `der pre noEvent sin cos tan asin
//!   acos atan atan2 sinh cosh tanh exp log sqrt abs sign min max`, the
//!   engine built-in `limit(x, lo, hi)` and table reads `ocv(soc)`;
//! * energy books go in the vendor annotation `__LightSim_energy(stored =
//!   …, loss = …)`, which Modelica tools ignore.
//!
//! [`parse`] reads definitions and checks everything the text itself
//! declares: names, units (a unit error quotes the equation as written),
//! values of each kind of parameter; [`parse_with`] also checks the parts,
//! ports and connector types a library provides. [`to_text`] prints a
//! definition back so that `parse(to_text(d)) == d`. [`basemodelica`]
//! imports Base Modelica models. Every error has a [`Span`] and a plain
//! message. The format's user documentation is `engine/docs/text-format.md`.

pub mod basemodelica;

mod ast;
mod dims;
mod exprs;
mod lexer;
mod lower;
mod parser;
mod print;

use lsim_ir::component::{ComponentDef, Library};
use lsim_ir::diag::Diagnostic;
use std::fmt;

pub use print::{connector_to_text, enum_type_to_text, library_to_text, to_text};

/// A place in the source text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Span {
    /// 1-based line of the start
    pub line: u32,
    /// 1-based column (in characters) of the start
    pub col: u32,
    /// 1-based line of the end
    pub end_line: u32,
    /// 1-based column of the end (just after the last character)
    pub end_col: u32,
    /// byte offset of the start
    pub start: usize,
    /// byte offset of the end
    pub end: usize,
}

/// An error in a text, with where it is and what is wrong in plain words.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
#[error("line {}, column {}: {message}", span.line, span.col)]
pub struct LangError {
    /// a stable code (`SYNTAX`, `UNIT-MISMATCH`, `UNKNOWN-NAME` …)
    pub code: &'static str,
    /// where
    pub span: Span,
    /// what is wrong, in plain words
    pub message: String,
}

impl LangError {
    pub(crate) fn new(code: &'static str, span: Span, message: String) -> Self {
        LangError { code, span, message }
    }

    /// The error as an engine diagnostic (the place goes in the detail).
    pub fn to_diagnostic(&self) -> Diagnostic {
        let mut d = Diagnostic::error(self.code, self.message.clone());
        d.detail.push(format!(
            "line {}, column {} to line {}, column {}",
            self.span.line, self.span.col, self.span.end_line, self.span.end_col
        ));
        d
    }

    /// The error with the line of `text` it points at, and a caret.
    pub fn render(&self, text: &str) -> String {
        let line = text.lines().nth(self.span.line.saturating_sub(1) as usize).unwrap_or("");
        let width = if self.span.end_line == self.span.line {
            (self.span.end_col.saturating_sub(self.span.col)).max(1) as usize
        } else {
            1
        };
        let pad = " ".repeat(self.span.col.saturating_sub(1) as usize);
        format!("{self}\n  {line}\n  {pad}{}", "^".repeat(width))
    }
}

/// All the errors of a text, one per line, for showing to a person.
pub struct Report<'a>(pub &'a [LangError]);

impl fmt::Display for Report<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for e in self.0 {
            writeln!(f, "{e}")?;
        }
        Ok(())
    }
}

fn syntax(text: &str) -> Result<(Vec<ast::ClassDef>, lexer::SourceMap<'_>), Vec<LangError>> {
    let map = lexer::SourceMap::new(text);
    let toks = lexer::lex(text, &map).map_err(|e| vec![e])?;
    let classes = parser::Parser::new(toks, &map).file().map_err(|e| vec![e])?;
    Ok((classes, map))
}

fn parse_lowered(text: &str, lib: Option<&Library>) -> Result<lower::Parsed, Vec<LangError>> {
    let (classes, map) = syntax(text)?;
    lower::lower_text(&classes, &map, lib)
}

/// Parses component definitions. Names, units and values are checked
/// against what the text declares; parts, ports and connector types it
/// takes from a library are taken on trust (see [`parse_with`]).
pub fn parse(text: &str) -> Result<Vec<ComponentDef>, Vec<LangError>> {
    parse_lowered(text, None).map(|p| p.components)
}

/// Parses component definitions, checking them against `lib` as well:
/// every part, connector type and enumeration type must be defined in the
/// text or in the library, and the parts' parameters and ports are
/// checked (names, units, kinds).
pub fn parse_with(text: &str, lib: &Library) -> Result<Vec<ComponentDef>, Vec<LangError>> {
    parse_lowered(text, Some(lib)).map(|p| p.components)
}

/// Parses a text of connector types, enumeration types and components
/// into a library (checked against `base` when given, as [`parse_with`]
/// does); [`library_to_text`] prints one.
pub fn parse_library(text: &str, base: Option<&Library>) -> Result<Library, Vec<LangError>> {
    let p = parse_lowered(text, base)?;
    let mut lib = Library::default();
    for c in p.connectors {
        lib.add_connector(c);
    }
    for t in p.types {
        lib.add_type(t);
    }
    for c in p.components {
        lib.add(c);
    }
    Ok(lib)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prints_the_resistor_and_the_brake() {
        let lib = lsim_lib::library();
        let r = to_text(&lib.components["Electrical.Resistor"]);
        assert!(r.starts_with("model Electrical.Resistor \"Ideal linear resistor.\"\n"), "{r}");
        assert!(r.contains("  parameter Real R(unit = \"Ohm\") = 1 \"resistance\";\n"), "{r}");
        assert!(r.contains("  v = R * i \"Ohm's law\";\n"), "{r}");
        assert!(r.contains("annotation(__LightSim_energy(loss = v * i));"), "{r}");
        let b = to_text(&lib.components["Rotational.ThresholdBrake"]);
        assert!(
            b.contains("  discrete Real engaged(unit = \"1\", start = 0, fixed = true)"),
            "{b}"
        );
        assert!(b.contains("  when flange.w >= w_on then\n"), "{b}");
        assert!(b.contains("    engaged = 1;\n  end when \"it engages"), "{b}");
        let bat = to_text(&lib.components["Battery.OcvR0Rc"]);
        assert!(bat.contains("  Electrical.ConstantVoltage source(V = ocv);"), "{bat}");
        assert!(bat.contains("  connect(r0.n, c1.p);"), "{bat}");
    }
}
