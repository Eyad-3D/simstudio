//! # lsim-lang: the text format of equation components
//!
//! Users write their own physical components in a small format that is a
//! strict subset of Base Modelica (the flat Modelica of the Modelica
//! Association's MCP-0031), so every component maps one-to-one to it and
//! Base Modelica import is the same parser with more of the grammar:
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
//! * declarations: `parameter Real`, `Real`, `discrete Real`, `input Real`,
//!   `output Real`, each with `unit` (a coherent SI unit, checked),
//!   optional `displayUnit`, `start`, `fixed`, `nominal`, `min`, `max`;
//! * `connector p: Pin` declares a physical port of a library connector
//!   type (an across/through pair); sub-components are `Lib.Name x(p = …)`;
//! * equations: `lhs = rhs "plain-words label"`, `connect(a, b)`,
//!   `when cond then v = expr; end when;`, `assert(cond, "message")`;
//! * expressions: `+ - * / ^`, comparisons, `and or not`,
//!   `if … then … else …`, `der pre noEvent sin cos tan asin acos atan
//!   atan2 sinh cosh tanh exp log sqrt abs sign min max`, and the two engine
//!   built-ins `limit(x, lo, hi)` and table interpolation;
//! * the label string after an equation is what fault messages quote;
//! * energy books go in the vendor annotation `__LightSim_energy(stored =
//!   …, loss = …)`, which Base Modelica tools ignore.
//!
//! Stage 1 provides [`to_text`] (the printer, so the format is fixed by
//! example and by tests); work package 1 adds the parser ([`parse`]) with
//! spans, unit checking at parse time, and Base Modelica import.

use lsim_ir::component::{ComponentDef, Equation, ParamValue, PortKind, VarKind, WhenAction};
use lsim_ir::expr::Expr;
use std::fmt::Write;

/// A position in the source text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    /// 1-based line
    pub line: u32,
    /// 1-based column
    pub col: u32,
}

/// A parse error.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
#[error("line {}, column {}: {message}", span.line, span.col)]
pub struct LangError {
    /// where
    pub span: Span,
    /// what is wrong, in plain words
    pub message: String,
}

/// Parses component definitions (work package 1).
pub fn parse(_text: &str) -> Result<Vec<ComponentDef>, Vec<LangError>> {
    Err(vec![LangError {
        span: Span { line: 1, col: 1 },
        message: "the text format's parser comes with work package 1".into(),
    }])
}

fn quoted(doc: &str) -> String {
    if doc.is_empty() { String::new() } else { format!(" \"{}\"", doc.replace('"', "\\\"")) }
}

fn value(v: &ParamValue) -> String {
    match v {
        ParamValue::Real(e) => e.to_string(),
        ParamValue::Bool(b) => b.to_string(),
        ParamValue::Enum(s) => s.clone(),
        ParamValue::Table1D { .. } => "table(…)".into(),
    }
}

/// Prints a component definition in the text format.
pub fn to_text(def: &ComponentDef) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "model {}{}", def.name, quoted(&def.doc));
    for p in &def.ports {
        let _ = match &p.kind {
            PortKind::Physical { connector } => {
                writeln!(s, "  connector {}: {connector}{};", p.name, quoted(&p.doc))
            }
            PortKind::Input { unit } => {
                writeln!(s, "  input Real {}(unit = \"{unit}\"){};", p.name, quoted(&p.doc))
            }
            PortKind::Output { unit } => {
                writeln!(s, "  output Real {}(unit = \"{unit}\"){};", p.name, quoted(&p.doc))
            }
        };
    }
    for p in &def.params {
        let mut attrs = vec![format!("unit = \"{}\"", p.unit)];
        if let Some(du) = &p.display_unit {
            attrs.push(format!("displayUnit = \"{du}\""));
        }
        if let Some(m) = p.min {
            attrs.push(format!("min = {m}"));
        }
        if let Some(m) = p.max {
            attrs.push(format!("max = {m}"));
        }
        let kw = if p.structural { "structural parameter" } else { "parameter" };
        let _ = writeln!(
            s,
            "  {kw} Real {}({}) = {}{};",
            p.name,
            attrs.join(", "),
            value(&p.default),
            quoted(&p.doc)
        );
    }
    for v in &def.vars {
        let mut attrs = vec![format!("unit = \"{}\"", v.unit)];
        if let Some(st) = &v.start {
            attrs.push(format!("start = {st}"));
        }
        if v.fixed {
            attrs.push("fixed = true".into());
        }
        if let Some(nm) = v.nominal {
            attrs.push(format!("nominal = {nm}"));
        }
        let kw = if v.kind == VarKind::Discrete { "discrete Real" } else { "Real" };
        let _ = writeln!(s, "  {kw} {}({}){};", v.name, attrs.join(", "), quoted(&v.doc));
    }
    for c in &def.components {
        let mods: Vec<String> =
            c.modifiers.iter().map(|m| format!("{} = {}", m.param, value(&m.value))).collect();
        let label = c.label.as_deref().map(quoted).unwrap_or_default();
        let _ = writeln!(s, "  {} {}({}){label};", c.def, c.name, mods.join(", "));
    }
    let has_eq = !def.equations.is_empty()
        || !def.connections.is_empty()
        || def.energy.stored.is_some()
        || def.energy.loss.is_some();
    if has_eq {
        let _ = writeln!(s, "equation");
    }
    for c in &def.connections {
        let _ = writeln!(s, "  connect({}, {});", c.a, c.b);
    }
    for e in &def.equations {
        let label = e.label.as_deref().map(quoted).unwrap_or_default();
        let _ = match &e.eq {
            Equation::Eq { lhs, rhs } => writeln!(s, "  {lhs} = {rhs}{label};"),
            Equation::When { condition, actions } => {
                let mut body = String::new();
                for a in actions {
                    let (WhenAction::Assign { var, value } | WhenAction::Reinit { var, value }) = a;
                    let line = match a {
                        WhenAction::Assign { .. } => format!("    {var} = {value};\n"),
                        WhenAction::Reinit { .. } => format!("    reinit({var}, {value});\n"),
                    };
                    body.push_str(&line);
                }
                writeln!(s, "  when {condition} then{label}\n{body}  end when;")
            }
            Equation::Assert { condition, message, error } => {
                let level = if *error { "" } else { ", AssertionLevel.warning" };
                writeln!(s, "  assert({condition}, \"{message}\"{level});")
            }
        };
    }
    let energy: Vec<String> = [("stored", &def.energy.stored), ("loss", &def.energy.loss)]
        .iter()
        .filter_map(|(k, e)| e.as_ref().map(|e: &Expr| format!("{k} = {e}")))
        .collect();
    if !energy.is_empty() {
        let _ = writeln!(s, "  annotation(__LightSim_energy({}));", energy.join(", "));
    }
    let _ = writeln!(s, "end {};", def.name);
    s
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
        assert!(b.contains("  when flange.w >= w_on then"), "{b}");
        assert!(b.contains("    engaged = 1;\n  end when;"), "{b}");
        let bat = to_text(&lib.components["Battery.OcvR0Rc"]);
        assert!(bat.contains("  Electrical.ConstantVoltage source(V = ocv);"), "{bat}");
        assert!(bat.contains("  connect(r0.n, c1.p);"), "{bat}");
        assert!(parse(&bat).is_err());
    }
}
