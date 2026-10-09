//! The printer: IR definitions as text the parser reads back to the same
//! IR (`parse(to_text(d)) == d`), in Modelica's syntax.

use crate::lexer::plain_ident;
use lsim_ir::component::*;
use lsim_ir::expr::{BinaryOp, CmpOp, Expr};
use std::fmt::Write;

/// A number as the parser reads it back exactly: the shortest digits
/// that round-trip, with an exponent for very small or large values.
pub(crate) fn num(v: f64) -> String {
    if v.is_infinite() {
        return if v > 0.0 { "1e999".into() } else { "-1e999".into() };
    }
    let a = v.abs();
    if a != 0.0 && !(1e-5..1e16).contains(&a) { format!("{v:e}") } else { format!("{v}") }
}

/// A name part, quoted when it is not a plain identifier.
fn part(p: &str) -> String {
    if plain_ident(p) {
        p.to_string()
    } else {
        format!("'{}'", p.replace('\\', "\\\\").replace('\'', "\\'"))
    }
}

/// A declared name: one identifier (quoted when needed, as Base Modelica
/// names with dots are).
pub(crate) fn decl_name(n: &str) -> String {
    part(n)
}

/// A reference `a.b.c`, each part quoted when needed.
pub(crate) fn ref_name(n: &str) -> String {
    if n.split('.').any(str::is_empty) {
        return part(n);
    }
    n.split('.').map(part).collect::<Vec<_>>().join(".")
}

/// Text in quotes, escaped.
pub(crate) fn string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

fn equality(e: &Expr) -> Option<(&'static str, &Expr, &Expr)> {
    match e {
        Expr::And(x, y) => match (&**x, &**y) {
            (Expr::Compare(CmpOp::Ge, a, b), Expr::Compare(CmpOp::Le, c, d))
                if a == c && b == d =>
            {
                Some(("==", a, b))
            }
            _ => None,
        },
        Expr::Or(x, y) => match (&**x, &**y) {
            (Expr::Compare(CmpOp::Lt, a, b), Expr::Compare(CmpOp::Gt, c, d))
                if a == c && b == d =>
            {
                Some(("<>", a, b))
            }
            _ => None,
        },
        _ => None,
    }
}

/// Binding strength in Modelica's grammar (higher binds tighter).
fn prec(e: &Expr) -> u8 {
    if equality(e).is_some() {
        return 4;
    }
    match e {
        Expr::If(..) => 0,
        Expr::Or(..) => 1,
        Expr::And(..) => 2,
        Expr::Not(..) => 3,
        Expr::Compare(..) => 4,
        Expr::Binary(BinaryOp::Add | BinaryOp::Sub, ..) | Expr::Neg(_) => 5,
        Expr::Const(v) if v.is_sign_negative() => 5,
        Expr::Binary(BinaryOp::Mul | BinaryOp::Div, ..) => 6,
        Expr::Binary(BinaryOp::Pow, ..) => 7,
        _ => 8,
    }
}

fn args(out: &mut String, list: &[Expr]) {
    for (i, a) in list.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        expr(out, a, 0);
    }
}

/// Writes `e`, in parentheses when it binds looser than `need`.
fn expr(out: &mut String, e: &Expr, need: u8) {
    if prec(e) < need {
        out.push('(');
        expr(out, e, 0);
        out.push(')');
        return;
    }
    if let Some((op, a, b)) = equality(e) {
        expr(out, a, 5);
        let _ = write!(out, " {op} ");
        expr(out, b, 5);
        return;
    }
    match e {
        Expr::Const(v) => out.push_str(&num(*v)),
        Expr::Time => out.push_str("time"),
        Expr::Name(n) => out.push_str(&ref_name(n)),
        Expr::Var(v) => {
            let _ = write!(out, "v[{}]", v.0);
        }
        Expr::Param(p) => {
            let _ = write!(out, "p[{}]", p.0);
        }
        Expr::Der(v) => {
            let _ = write!(out, "der(v[{}])", v.0);
        }
        Expr::Pre(v) => {
            let _ = write!(out, "pre(v[{}])", v.0);
        }
        Expr::Neg(a) => {
            out.push('-');
            if matches!(**a, Expr::Const(c) if !c.is_sign_negative()) {
                // `-(2)`: a minus applied to a number, not the number -2
                out.push('(');
                expr(out, a, 0);
                out.push(')');
            } else {
                expr(out, a, 6);
            }
        }
        Expr::Binary(op, a, b) => {
            let (s, l, r) = match op {
                BinaryOp::Add => ("+", 5, 6),
                BinaryOp::Sub => ("-", 5, 6),
                BinaryOp::Mul => ("*", 6, 7),
                BinaryOp::Div => ("/", 6, 7),
                BinaryOp::Pow => ("^", 8, 8),
            };
            expr(out, a, l);
            let _ = write!(out, " {s} ");
            expr(out, b, r);
        }
        Expr::Call(f, list) => {
            out.push_str(f.name());
            out.push('(');
            args(out, list);
            out.push(')');
        }
        Expr::Compare(op, a, b) => {
            let s = match op {
                CmpOp::Lt => "<",
                CmpOp::Le => "<=",
                CmpOp::Gt => ">",
                CmpOp::Ge => ">=",
            };
            expr(out, a, 5);
            let _ = write!(out, " {s} ");
            expr(out, b, 5);
        }
        Expr::And(a, b) => {
            expr(out, a, 2);
            out.push_str(" and ");
            expr(out, b, 3);
        }
        Expr::Or(a, b) => {
            expr(out, a, 1);
            out.push_str(" or ");
            expr(out, b, 2);
        }
        Expr::Not(a) => {
            out.push_str("not ");
            expr(out, a, 4);
        }
        Expr::If(c, a, b) => {
            out.push_str("if ");
            expr(out, c, 1);
            out.push_str(" then ");
            expr(out, a, 1);
            let mut rest = &**b;
            while let Expr::If(c2, a2, b2) = rest {
                out.push_str(" elseif ");
                expr(out, c2, 1);
                out.push_str(" then ");
                expr(out, a2, 1);
                rest = b2;
            }
            out.push_str(" else ");
            expr(out, rest, 1);
        }
        Expr::NoEvent(a) => {
            out.push_str("noEvent(");
            expr(out, a, 0);
            out.push(')');
        }
        Expr::Table { table, args: list } => match list.split_first() {
            Some((Expr::Name(n), rest)) => {
                out.push_str(&ref_name(n));
                out.push('(');
                args(out, rest);
                out.push(')');
            }
            _ => {
                let _ = write!(out, "table{table}(");
                args(out, list);
                out.push(')');
            }
        },
    }
}

/// An expression in the text format.
pub(crate) fn expr_text(e: &Expr) -> String {
    let mut s = String::new();
    expr(&mut s, e, 0);
    s
}

/// An equation's left side (no if-expression without parentheses).
fn lhs_text(e: &Expr) -> String {
    let mut s = String::new();
    expr(&mut s, e, 1);
    s
}

fn list(v: &[f64]) -> String {
    format!("{{{}}}", v.iter().map(|x| num(*x)).collect::<Vec<_>>().join(", "))
}

fn table_text(t: &TableData, rules: bool) -> String {
    let mut parts = vec![];
    if t.dims() == 1 {
        parts.push(format!("x = {}", list(&t.x)));
        parts.push(format!("y = {}", list(&t.values)));
        parts.push(format!("xUnit = {}", string(&t.axis_units[0])));
    } else {
        let rows: Vec<String> = t
            .values
            .chunks(t.y.len())
            .map(|r| r.iter().map(|x| num(*x)).collect::<Vec<_>>().join(", "))
            .collect();
        parts.push(format!("x1 = {}", list(&t.x)));
        parts.push(format!("x2 = {}", list(&t.y)));
        parts.push(format!("values = [{}]", rows.join("; ")));
        parts.push(format!("x1Unit = {}", string(&t.axis_units[0])));
        parts.push(format!("x2Unit = {}", string(&t.axis_units[1])));
    }
    if rules {
        let i = match t.interpolation {
            Interpolation::MonotoneCubic => "monotoneCubic",
            Interpolation::Linear => "linear",
        };
        parts.push(format!("interpolation = {i}"));
        let o: Vec<&str> = t.outside[..t.dims()]
            .iter()
            .map(|o| match o {
                Outside::Clamp => "clamp",
                Outside::Linear => "linear",
                Outside::Error => "error",
            })
            .collect();
        parts.push(format!("outside = {{{}}}", o.join(", ")));
    }
    format!("table({})", parts.join(", "))
}

/// A parameter value as written after `=`.
pub(crate) fn value_text(v: &ParamValue) -> String {
    match v {
        ParamValue::Real(e) => expr_text(e),
        ParamValue::Bool(b) => b.to_string(),
        ParamValue::Enum(q) => ref_name(q),
        ParamValue::Table(t) => table_text(t, true),
        other => table_text(&other.table().expect("a table"), false),
    }
}

fn doc(d: &str) -> String {
    if d.is_empty() { String::new() } else { format!(" {}", string(d)) }
}

fn label(l: &Option<String>) -> String {
    l.as_deref().map(|s| format!(" {}", string(s))).unwrap_or_default()
}

fn attrs(a: &[String]) -> String {
    if a.is_empty() { String::new() } else { format!("({})", a.join(", ")) }
}

/// An enumeration type's declaration (without indentation or newline).
pub fn enum_type_to_text(t: &EnumType) -> String {
    let lits: Vec<String> =
        t.literals.iter().map(|l| format!("{}{}", decl_name(&l.name), doc(&l.doc))).collect();
    format!("type {} = enumeration({}){};", ref_name(&t.name), lits.join(", "), doc(&t.doc))
}

/// A connector type in the text format.
pub fn connector_to_text(c: &ConnectorDef) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "connector {}{}", ref_name(&c.name), doc(&c.doc));
    let _ = writeln!(s, "  Real {}(unit = {});", decl_name(&c.across.name), string(&c.across.unit));
    let _ = writeln!(
        s,
        "  flow Real {}(unit = {});",
        decl_name(&c.through.name),
        string(&c.through.unit)
    );
    if c.power == PowerRule::ThroughIsPower {
        let _ = writeln!(s, "  annotation(__LightSim_power = \"through\");");
    }
    let _ = writeln!(s, "end {};", ref_name(&c.name));
    s
}

fn equation(s: &mut String, e: &EquationDecl, indent: &str) {
    let lbl = label(&e.label);
    match &e.eq {
        Equation::Eq { lhs, rhs } => {
            let _ = writeln!(s, "{indent}{} = {}{lbl};", lhs_text(lhs), expr_text(rhs));
        }
        Equation::When { condition, actions } => {
            let _ = writeln!(s, "{indent}when {} then", expr_text(condition));
            for a in actions {
                let _ = match a {
                    WhenAction::Assign { var, value } => {
                        writeln!(s, "{indent}  {} = {};", ref_name(var), expr_text(value))
                    }
                    WhenAction::Reinit { var, value } => {
                        writeln!(s, "{indent}  reinit({}, {});", ref_name(var), expr_text(value))
                    }
                };
            }
            let _ = writeln!(s, "{indent}end when{lbl};");
        }
        Equation::Assert { condition, message, error } => {
            let level = if *error { "" } else { ", AssertionLevel.warning" };
            let _ = writeln!(
                s,
                "{indent}assert({}, {}{level}){lbl};",
                expr_text(condition),
                string(message)
            );
        }
    }
}

/// Prints a component definition in the text format.
pub fn to_text(def: &ComponentDef) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "model {}{}", ref_name(&def.name), doc(&def.doc));
    for t in &def.types {
        let _ = writeln!(s, "  {}", enum_type_to_text(t));
    }
    for p in &def.ports {
        let _ = match &p.kind {
            PortKind::Physical { connector } => {
                writeln!(
                    s,
                    "  connector {}: {}{};",
                    decl_name(&p.name),
                    ref_name(connector),
                    doc(&p.doc)
                )
            }
            PortKind::Input { unit } | PortKind::Output { unit } => {
                let dir = if matches!(p.kind, PortKind::Input { .. }) { "input" } else { "output" };
                let a =
                    if unit.is_empty() { vec![] } else { vec![format!("unit = {}", string(unit))] };
                writeln!(s, "  {dir} Real {}{}{};", decl_name(&p.name), attrs(&a), doc(&p.doc))
            }
        };
    }
    for p in &def.params {
        let mut a = vec![];
        if !p.unit.is_empty() {
            a.push(format!("unit = {}", string(&p.unit)));
        }
        if let Some(du) = &p.display_unit {
            a.push(format!("displayUnit = {}", string(du)));
        }
        if let Some(m) = p.min {
            a.push(format!("min = {}", num(m)));
        }
        if let Some(m) = p.max {
            a.push(format!("max = {}", num(m)));
        }
        let ty = match &p.default {
            ParamValue::Bool(_) => "Boolean".to_string(),
            ParamValue::Enum(q) => {
                ref_name(split_enum_value(q).map(|(t, _)| t).unwrap_or(q.as_str()))
            }
            _ => "Real".to_string(),
        };
        let kw = if p.structural { "structural parameter" } else { "parameter" };
        let _ = writeln!(
            s,
            "  {kw} {ty} {}{} = {}{};",
            decl_name(&p.name),
            attrs(&a),
            value_text(&p.default),
            doc(&p.doc)
        );
    }
    for v in &def.vars {
        let mut a = vec![];
        if !v.unit.is_empty() {
            a.push(format!("unit = {}", string(&v.unit)));
        }
        if let Some(du) = &v.display_unit {
            a.push(format!("displayUnit = {}", string(du)));
        }
        if let Some(st) = &v.start {
            a.push(format!("start = {}", expr_text(st)));
        }
        if v.fixed {
            a.push("fixed = true".into());
        }
        if let Some(nm) = v.nominal {
            a.push(format!("nominal = {}", num(nm)));
        }
        let kw = if v.kind == VarKind::Discrete { "discrete Real" } else { "Real" };
        let _ = writeln!(s, "  {kw} {}{}{};", decl_name(&v.name), attrs(&a), doc(&v.doc));
    }
    for c in &def.components {
        let mods: Vec<String> = c
            .modifiers
            .iter()
            .map(|m| format!("{} = {}", decl_name(&m.param), value_text(&m.value)))
            .collect();
        let id = c
            .ui_id
            .as_deref()
            .map(|i| format!(" annotation(__LightSim(id = {}))", string(i)))
            .unwrap_or_default();
        let _ = writeln!(
            s,
            "  {} {}{}{}{id};",
            ref_name(&c.def),
            decl_name(&c.name),
            attrs(&mods),
            label(&c.label)
        );
    }
    if !def.equations.is_empty() || !def.connections.is_empty() {
        let _ = writeln!(s, "equation");
    }
    for c in &def.connections {
        let _ = writeln!(s, "  connect({}, {});", ref_name(&c.a), ref_name(&c.b));
    }
    for e in &def.equations {
        equation(&mut s, e, "  ");
    }
    if !def.initial_equations.is_empty() {
        let _ = writeln!(s, "initial equation");
        for e in &def.initial_equations {
            equation(&mut s, e, "  ");
        }
    }
    let energy: Vec<String> = [("stored", &def.energy.stored), ("loss", &def.energy.loss)]
        .iter()
        .filter_map(|(k, e)| e.as_ref().map(|e| format!("{k} = {}", expr_text(e))))
        .collect();
    if !energy.is_empty() {
        let _ = writeln!(s, "  annotation(__LightSim_energy({}));", energy.join(", "));
    }
    for im in &def.impulse {
        let _ = writeln!(
            s,
            "  annotation(__LightSim_impulse(keep = {}, active = {}));",
            expr_text(&im.keep),
            expr_text(&im.active)
        );
    }
    let _ = writeln!(s, "end {};", ref_name(&def.name));
    s
}

/// Prints a whole library: its enumeration types, connectors and
/// components, which [`crate::parse_library`] reads back.
pub fn library_to_text(lib: &Library) -> String {
    let mut s = String::new();
    for t in lib.types.values() {
        let _ = writeln!(s, "{}\n", enum_type_to_text(t));
    }
    for c in lib.connectors.values() {
        let _ = writeln!(s, "{}", connector_to_text(c));
    }
    for c in lib.components.values() {
        let _ = writeln!(s, "{}", to_text(c));
    }
    s
}
