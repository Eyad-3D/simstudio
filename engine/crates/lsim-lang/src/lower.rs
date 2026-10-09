//! The text format: from the syntax tree to IR definitions, with every
//! check that can be made at parse time — names, units (each equation
//! quoted as written), values of each kind of parameter, connections.

use crate::ast::{self, ClassDef, ClassKind, Element, EqKind, ExprKind, ModArg, Short};
use crate::dims::{self, D, NameDim, Resolve};
use crate::exprs::{self, LowerCx};
use crate::lexer::SourceMap;
use crate::{LangError, Span};
use lsim_ir::component::*;
use lsim_ir::expr::{BinaryOp, Expr};
use lsim_ir::units::{Dim, parse_unit};
use std::collections::BTreeMap;

/// What the text and the library (if given) define.
pub(crate) struct Known<'a> {
    pub lib: Option<&'a Library>,
    pub connectors: BTreeMap<String, ConnectorDef>,
    pub types: BTreeMap<String, EnumType>,
    pub components: BTreeMap<String, ComponentDef>,
}

impl Known<'_> {
    pub fn connector(&self, n: &str) -> Option<&ConnectorDef> {
        self.connectors.get(n).or_else(|| self.lib.and_then(|l| l.connectors.get(n)))
    }

    pub fn component(&self, n: &str) -> Option<&ComponentDef> {
        self.components.get(n).or_else(|| self.lib.and_then(|l| l.components.get(n)))
    }

    pub fn enum_type<'b>(&'b self, scope: &'b ComponentDef, n: &str) -> Option<&'b EnumType> {
        scope
            .types
            .iter()
            .find(|t| t.name == n)
            .or_else(|| self.types.get(n))
            .or_else(|| self.lib.and_then(|l| l.types.get(n)))
    }

    /// With a library every type must be known; without one, names of
    /// types and parts the text does not define are taken on trust.
    pub fn strict(&self) -> bool {
        self.lib.is_some()
    }
}

fn unit_dim(text: &str) -> D {
    parse_unit(text).map(|u| D::Known(u.dim)).unwrap_or(D::Unknown)
}

/// Resolves the names of one component's scope for the unit check.
pub(crate) struct CompResolve<'a> {
    pub def: &'a ComponentDef,
    pub known: &'a Known<'a>,
}

impl CompResolve<'_> {
    fn resolve_in(&self, def: &ComponentDef, n: &str, depth: usize) -> NameDim {
        if let Some(p) = def.params.iter().find(|p| p.name == n) {
            return match &p.default {
                ParamValue::Real(_) => NameDim::Value(unit_dim(&p.unit)),
                ParamValue::Bool(_) | ParamValue::Enum(_) => NameDim::Value(D::Known(Dim::NONE)),
                other => {
                    let axes = other
                        .table()
                        .map(|t| t.axis_units[..t.dims()].iter().map(|u| unit_dim(u)).collect())
                        .unwrap_or_default();
                    NameDim::Table(unit_dim(&p.unit), axes)
                }
            };
        }
        if let Some(v) = def.vars.iter().find(|v| v.name == n) {
            return NameDim::Value(unit_dim(&v.unit));
        }
        if let Some(port) = def.ports.iter().find(|p| p.name == n) {
            return match &port.kind {
                PortKind::Input { unit } | PortKind::Output { unit } => {
                    NameDim::Value(unit_dim(unit))
                }
                PortKind::Physical { .. } => NameDim::Missing,
            };
        }
        if let Some((head, rest)) = n.split_once('.') {
            if let Some(port) = def.ports.iter().find(|p| p.name == head) {
                let PortKind::Physical { connector } = &port.kind else { return NameDim::Missing };
                return match self.known.connector(connector) {
                    Some(c) if c.across.name == rest => NameDim::Value(unit_dim(&c.across.unit)),
                    Some(c) if c.through.name == rest => NameDim::Value(unit_dim(&c.through.unit)),
                    Some(_) => NameDim::Missing,
                    None => NameDim::Value(D::Unknown),
                };
            }
            if let Some(s) = def.components.iter().find(|s| s.name == head) {
                return match self.known.component(&s.def) {
                    Some(sd) if depth < 32 => self.resolve_in(sd, rest, depth + 1),
                    Some(_) => NameDim::Value(D::Unknown),
                    None => NameDim::Value(D::Unknown),
                };
            }
            if let Some((ty, lit)) = split_enum_value(n)
                && let Some(t) = self.known.enum_type(def, ty)
            {
                return if t.ordinal(lit).is_some() {
                    NameDim::Value(D::Known(Dim::NONE))
                } else {
                    NameDim::Missing
                };
            }
            if !self.known.strict() {
                return NameDim::Value(D::Unknown);
            }
        }
        NameDim::Missing
    }

    /// Why `n` is not a name of this scope, in words.
    fn why_missing(&self, n: &str) -> String {
        let def = self.def;
        let who = format!("'{}'", def.name);
        if def.ports.iter().any(|p| p.name == n && matches!(p.kind, PortKind::Physical { .. })) {
            let port = def.ports.iter().find(|p| p.name == n).expect("found");
            let PortKind::Physical { connector } = &port.kind else { unreachable!() };
            return match self.known.connector(connector) {
                Some(c) => format!(
                    "'{n}' is a port: write its quantities, {n}.{} and {n}.{}",
                    c.across.name, c.through.name
                ),
                None => format!("'{n}' is a port: write one of its quantities, as {n}.v"),
            };
        }
        if let Some((head, rest)) = n.split_once('.') {
            if let Some(port) = def.ports.iter().find(|p| p.name == head)
                && let PortKind::Physical { connector } = &port.kind
                && let Some(c) = self.known.connector(connector)
            {
                return format!(
                    "the connector {} of '{head}' has no quantity '{rest}' (it has {} and {})",
                    c.name, c.across.name, c.through.name
                );
            }
            if let Some(s) = def.components.iter().find(|s| s.name == head) {
                return format!("'{head}' ({}) has no variable, parameter or port '{rest}'", s.def);
            }
            if let Some((ty, lit)) = split_enum_value(n)
                && let Some(t) = self.known.enum_type(def, ty)
            {
                let opts: Vec<&str> = t.literals.iter().map(|l| l.name.as_str()).collect();
                return format!("'{lit}' is not an option of {ty} ({})", opts.join(", "));
            }
        }
        format!("'{n}' is not a variable, parameter or port of {who}")
    }
}

impl Resolve for CompResolve<'_> {
    fn name(&self, n: &str) -> NameDim {
        self.resolve_in(self.def, n, 0)
    }
}

/// Lowering context for one component's expressions.
struct TextCx<'a> {
    def: &'a ComponentDef,
}

impl LowerCx for TextCx<'_> {
    fn reference(&self, parts: &[String], _span: Span) -> Result<Expr, LangError> {
        Ok(Expr::Name(parts.join(".")))
    }

    fn call(
        &self,
        name: &[String],
        args: &ast::Args,
        span: Span,
    ) -> Option<Result<Expr, LangError>> {
        if name.len() != 1 {
            return None;
        }
        let p = self.def.params.iter().find(|p| p.name == name[0])?;
        let table = p.default.table()?;
        Some((|| {
            if let Some((n, s, _)) = args.named.first() {
                return Err(LangError::new(
                    "CALL-ARGS",
                    *s,
                    format!("the table '{}' takes no named argument '{n}'", p.name),
                ));
            }
            if args.positional.len() != table.dims() {
                let (axes, at) = if table.dims() == 1 {
                    ("one axis", "one value")
                } else {
                    ("two axes", "two values")
                };
                return Err(LangError::new(
                    "CALL-ARGS",
                    span,
                    format!(
                        "the table '{}' has {axes}, so it is read at {at}, not {}",
                        p.name,
                        args.positional.len()
                    ),
                ));
            }
            let at =
                args.positional.iter().map(|a| exprs::lower(a, self)).collect::<Result<_, _>>()?;
            Ok(lsim_ir::expr::table(&p.name, at))
        })())
    }
}

/// Names referred to in an expression, with their spans.
fn refs(e: &ast::Expr, out: &mut Vec<(String, Span)>) {
    match &e.kind {
        ExprKind::Ref(p) if !(p.len() == 1 && p[0] == "time") => out.push((p.join("."), e.span)),
        ExprKind::Ref(_) | ExprKind::Num(_) | ExprKind::Str(_) | ExprKind::Bool(_) => {}
        ExprKind::Call(_, args) => {
            for a in &args.positional {
                refs(a, out);
            }
            for (_, _, a) in &args.named {
                refs(a, out);
            }
        }
        ExprKind::Neg(a) | ExprKind::Not(a) | ExprKind::Paren(a) => refs(a, out),
        ExprKind::Bin(_, a, b) => {
            refs(a, out);
            refs(b, out);
        }
        ExprKind::If(br, other) => {
            for (c, v) in br {
                refs(c, out);
                refs(v, out);
            }
            refs(other, out);
        }
        ExprKind::Array(items) => items.iter().for_each(|a| refs(a, out)),
        ExprKind::Matrix(rows) => rows.iter().flatten().for_each(|a| refs(a, out)),
    }
}

/// Reads a string-valued modifier.
fn string_value(m: &ModArg, what: &str) -> Result<String, LangError> {
    match m.value.as_ref().map(|v| &v.strip().kind) {
        Some(ExprKind::Str(s)) => Ok(s.clone()),
        _ => Err(LangError::new(
            "ATTRIBUTE",
            m.span,
            format!("{what} is given as text in quotes, as {} = \"V\"", m.name.join(".")),
        )),
    }
}

fn bool_value(m: &ModArg) -> Result<bool, LangError> {
    match m.value.as_ref().map(|v| &v.strip().kind) {
        Some(ExprKind::Bool(b)) => Ok(*b),
        _ => Err(LangError::new(
            "ATTRIBUTE",
            m.span,
            format!("'{}' is true or false", m.name.join(".")),
        )),
    }
}

/// A plain number, possibly negative.
pub(crate) fn number(e: &ast::Expr) -> Option<f64> {
    match &e.strip().kind {
        ExprKind::Num(v) => Some(*v),
        ExprKind::Neg(a) => number(a).map(|v| -v),
        _ => None,
    }
}

fn number_value(m: &ModArg) -> Result<f64, LangError> {
    m.value.as_ref().and_then(number).ok_or_else(|| {
        LangError::new(
            "ATTRIBUTE",
            m.span,
            format!("'{}' is a plain number here", m.name.join(".")),
        )
    })
}

fn numbers(e: &ast::Expr, what: &str) -> Result<Vec<f64>, LangError> {
    let ExprKind::Array(items) = &e.strip().kind else {
        return Err(LangError::new(
            "TABLE",
            e.span,
            format!("{what} is a list of numbers in braces, as {{0, 0.5, 1}}"),
        ));
    };
    items
        .iter()
        .map(|i| {
            number(i).ok_or_else(|| {
                LangError::new("TABLE", i.span, format!("{what} holds plain numbers only"))
            })
        })
        .collect()
}

fn outside_word(e: &ast::Expr) -> Option<Outside> {
    match &e.strip().kind {
        ExprKind::Ref(p) if p.len() == 1 => match p[0].as_str() {
            "clamp" => Some(Outside::Clamp),
            "linear" => Some(Outside::Linear),
            "error" => Some(Outside::Error),
            _ => None,
        },
        _ => None,
    }
}

/// `table(x = {…}, y = {…}, xUnit = "…")`,
/// `table(x1 = {…}, x2 = {…}, values = […], x1Unit = "…", x2Unit = "…")`,
/// with optional `interpolation = linear|monotoneCubic` and `outside =
/// {clamp|linear|error, …}`.
pub(crate) fn table_value(e: &ast::Expr) -> Result<ParamValue, LangError> {
    let ExprKind::Call(name, args) = &e.strip().kind else { unreachable!("checked by the caller") };
    debug_assert_eq!(name.join("."), "table");
    if let Some(p) = args.positional.first() {
        return Err(LangError::new(
            "TABLE",
            p.span,
            "a table's parts are named: table(x = {…}, y = {…}, xUnit = \"…\")".into(),
        ));
    }
    let mut got: BTreeMap<&str, (&ast::Expr, Span)> = BTreeMap::new();
    for (k, s, v) in &args.named {
        const KNOWN: [&str; 10] = [
            "x",
            "y",
            "xUnit",
            "x1",
            "x2",
            "values",
            "x1Unit",
            "x2Unit",
            "interpolation",
            "outside",
        ];
        if !KNOWN.contains(&k.as_str()) {
            return Err(LangError::new(
                "TABLE",
                *s,
                format!(
                    "a table has no part '{k}': a 1-D table has x, y and xUnit; a 2-D table x1, \
                     x2, values, x1Unit and x2Unit; both may give interpolation and outside"
                ),
            ));
        }
        if got.insert(k.as_str(), (v, *s)).is_some() {
            return Err(LangError::new("TABLE", *s, format!("the table gives '{k}' twice")));
        }
    }
    let unit = |k: &str| -> Result<String, LangError> {
        match got.get(k) {
            None => Ok(String::new()),
            Some((v, s)) => match &v.strip().kind {
                ExprKind::Str(u) => Ok(u.clone()),
                _ => Err(LangError::new("TABLE", *s, format!("{k} is a unit in quotes, as \"1\""))),
            },
        }
    };
    let need = |k: &str| -> Result<&ast::Expr, LangError> {
        got.get(k).map(|(v, _)| *v).ok_or_else(|| {
            LangError::new("TABLE", e.span, format!("the table is missing its '{k}'"))
        })
    };
    let two_d = got.contains_key("x1") || got.contains_key("values");
    let (axes, values) = if two_d {
        for k in ["x", "y", "xUnit"] {
            if let Some((_, s)) = got.get(k) {
                return Err(LangError::new(
                    "TABLE",
                    *s,
                    format!("'{k}' belongs to a 1-D table; a 2-D table has x1, x2 and values"),
                ));
            }
        }
        let x1 = numbers(need("x1")?, "x1")?;
        let x2 = numbers(need("x2")?, "x2")?;
        let v = need("values")?;
        let ExprKind::Matrix(rows) = &v.strip().kind else {
            return Err(LangError::new(
                "TABLE",
                v.span,
                "a 2-D table's values are a matrix, one row per x1 point: [1, 2; 3, 4]".into(),
            ));
        };
        if rows.len() != x1.len() {
            return Err(LangError::new(
                "TABLE",
                v.span,
                format!("the values have {} rows but x1 has {} points", rows.len(), x1.len()),
            ));
        }
        let mut values = vec![];
        for (i, row) in rows.iter().enumerate() {
            if row.len() != x2.len() {
                return Err(LangError::new(
                    "TABLE",
                    v.span,
                    format!(
                        "row {} of the values has {} numbers but x2 has {} points",
                        i + 1,
                        row.len(),
                        x2.len()
                    ),
                ));
            }
            for item in row {
                values.push(number(item).ok_or_else(|| {
                    LangError::new("TABLE", item.span, "the values are plain numbers".into())
                })?);
            }
        }
        (vec![(x1, unit("x1Unit")?), (x2, unit("x2Unit")?)], values)
    } else {
        let x = numbers(need("x")?, "x")?;
        let y = numbers(need("y")?, "y")?;
        (vec![(x, unit("xUnit")?)], y)
    };
    let rules = got.contains_key("interpolation") || got.contains_key("outside");
    let interpolation = match got.get("interpolation") {
        None => Interpolation::default(),
        Some((v, s)) => match &v.strip().kind {
            ExprKind::Ref(p) if p.len() == 1 && p[0] == "linear" => Interpolation::Linear,
            ExprKind::Ref(p) if p.len() == 1 && p[0] == "monotoneCubic" => {
                Interpolation::MonotoneCubic
            }
            _ => {
                return Err(LangError::new(
                    "TABLE",
                    *s,
                    "interpolation is monotoneCubic or linear".into(),
                ));
            }
        },
    };
    let outside = match got.get("outside") {
        None => vec![Outside::default(); axes.len()],
        Some((v, s)) => {
            let words: Vec<&ast::Expr> = match &v.strip().kind {
                ExprKind::Array(items) => items.iter().collect(),
                _ => vec![*v],
            };
            let mut out = vec![];
            for w in &words {
                out.push(outside_word(w).ok_or_else(|| {
                    LangError::new("TABLE", w.span, "outside is clamp, linear or error".into())
                })?);
            }
            if out.len() == 1 && axes.len() == 2 {
                out.push(out[0]);
            }
            if out.len() != axes.len() {
                return Err(LangError::new(
                    "TABLE",
                    *s,
                    format!("outside gives {} rules for {} axes", out.len(), axes.len()),
                ));
            }
            out
        }
    };
    let mut axes = axes.into_iter();
    let (x, x_unit) = axes.next().expect("one axis at least");
    let (y, y_unit) = axes.next().unwrap_or_default();
    let data = TableData {
        x,
        y,
        values,
        interpolation,
        outside: [outside[0], outside.get(1).copied().unwrap_or_default()],
        axis_units: [x_unit, y_unit],
    };
    data.check().map_err(|why| {
        LangError::new("TABLE", e.span, format!("this table is not valid: {why}"))
    })?;
    if rules {
        return Ok(ParamValue::Table(data));
    }
    let [x_unit, y_unit] = data.axis_units;
    Ok(if data.y.is_empty() {
        ParamValue::Table1D { x: data.x, y: data.values, axis_unit: x_unit }
    } else {
        ParamValue::Table2D {
            x1: data.x,
            x2: data.y,
            values: data.values,
            axis_units: [x_unit, y_unit],
        }
    })
}

fn is_table_call(e: &ast::Expr) -> bool {
    matches!(&e.strip().kind, ExprKind::Call(n, _) if n.len() == 1 && n[0] == "table")
}

/// Checks a declared unit: known, and coherent SI (`what` names it).
pub(crate) fn check_unit(text: &str, span: Span, what: &str, errs: &mut Vec<LangError>) {
    match parse_unit(text) {
        Ok(u) if u.scale == 1.0 && u.offset == 0.0 => {}
        Ok(u) => {
            let si = lsim_ir::units::describe(u.dim);
            errs.push(LangError::new(
                "UNIT-NOT-SI",
                span,
                format!(
                    "{what} is declared in '{text}', which is not an SI unit: numbers in the \
                     engine are SI; declare unit = \"{si}\" and displayUnit = \"{text}\""
                ),
            ));
        }
        Err(e) => errs.push(LangError::new("UNIT-SYNTAX", span, format!("{what}: {e}"))),
    }
}

fn check_display_unit(
    unit: &str,
    display: &str,
    span: Span,
    what: &str,
    errs: &mut Vec<LangError>,
) {
    match (parse_unit(unit), parse_unit(display)) {
        (Ok(u), Ok(d)) if u.dim != d.dim => errs.push(LangError::new(
            "UNIT-DISPLAY",
            span,
            format!(
                "{what} is in '{unit}' but shown in '{display}', which measures something else"
            ),
        )),
        (_, Err(e)) => errs.push(LangError::new("UNIT-SYNTAX", span, format!("{what}: {e}"))),
        _ => {}
    }
}

/// Spans of a component's parts, for the checks after lowering.
#[derive(Default)]
struct DeclSpans {
    params: Vec<(Span, Option<Span>)>,
    vars: Vec<(Span, Option<Span>)>,
    subs: Vec<(Span, Vec<Span>)>,
    names: BTreeMap<String, Span>,
}

const ATTRS_PARAM: &str = "a parameter takes unit, displayUnit, min and max";

/// Lowers the declarations of a model (no equations yet).
fn lower_decls(
    cls: &ClassDef,
    errs: &mut Vec<LangError>,
) -> (ComponentDef, DeclSpans, Vec<Element>) {
    let mut def = ComponentDef {
        name: cls.name.clone(),
        doc: cls.doc.clone().unwrap_or_default(),
        ..Default::default()
    };
    let mut spans = DeclSpans::default();
    // bindings of variables and outputs: equations to add, in order
    let mut bindings = vec![];
    for c in &cls.classes {
        match (&c.kind, &c.short) {
            (ClassKind::Type, Some(Short::Enumeration(lits))) => {
                if spans.names.insert(c.name.clone(), c.name_span).is_some() {
                    errs.push(LangError::new(
                        "DUPLICATE",
                        c.name_span,
                        format!("'{}' is declared twice in '{}'", c.name, cls.name),
                    ));
                }
                def.types.push(enum_type(c, lits, errs));
            }
            _ => errs.push(LangError::new(
                "UNSUPPORTED",
                c.name_span,
                format!(
                    "a {} cannot be defined inside '{}': only enumeration types \
                     (type Mode = enumeration(…)) can",
                    c.kind.word(),
                    cls.name
                ),
            )),
        }
    }
    let mut kinds: BTreeMap<String, bool> = BTreeMap::new();
    for el in &cls.elements {
        let ty = el.type_name.join(".");
        let is_param = el.prefixes.parameter;
        let is_sub = !el.port
            && !el.prefixes.parameter
            && !el.prefixes.input
            && !el.prefixes.output
            && !matches!(ty.as_str(), "Real" | "Integer" | "Boolean" | "String");
        if let Some(prev) = spans.names.insert(el.name.clone(), el.name_span) {
            // a parameter and a part of the same name: reported once below
            let prev_param = kinds.get(&el.name).copied().unwrap_or(false);
            let clash = (prev_param && is_sub)
                || (is_param && def.components.iter().any(|s| s.name == el.name));
            if !clash {
                errs.push(LangError::new(
                    "DUPLICATE",
                    el.name_span,
                    format!(
                        "'{}' is declared twice in '{}' (first on line {})",
                        el.name, cls.name, prev.line
                    ),
                ));
                continue;
            }
        }
        kinds.insert(el.name.clone(), is_param);
        if el.protected {
            errs.push(LangError::new(
                "UNSUPPORTED",
                el.span,
                "'protected' declarations are supported in functions only".into(),
            ));
        }
        let doc = el.doc.clone().unwrap_or_default();
        let ty = el.type_name.join(".");
        let p = &el.prefixes;
        if el.port {
            def.ports.push(PortDecl {
                name: el.name.clone(),
                kind: PortKind::Physical { connector: ty },
                doc,
            });
            continue;
        }
        if p.flow {
            errs.push(LangError::new(
                "UNSUPPORTED",
                el.span,
                "'flow' belongs in a connector definition; a component declares a physical port \
                 as 'connector p: Pin'"
                    .into(),
            ));
            continue;
        }
        if p.constant {
            errs.push(LangError::new(
                "UNSUPPORTED",
                el.span,
                format!(
                    "'{}': constants are not supported in components; declare a parameter",
                    el.name
                ),
            ));
            continue;
        }
        let scalar = matches!(ty.as_str(), "Real" | "Integer" | "Boolean");
        if p.input || p.output {
            if !scalar {
                errs.push(LangError::new(
                    "PORT-TYPE",
                    el.type_span,
                    format!("the signal port '{}' is a Real (found '{ty}')", el.name),
                ));
                continue;
            }
            let mut unit = String::new();
            for m in &el.mods {
                match m.name.join(".").as_str() {
                    "unit" => match string_value(m, "the unit") {
                        Ok(u) => {
                            check_unit(&u, m.span, &format!("the port '{}'", el.name), errs);
                            unit = u;
                        }
                        Err(e) => errs.push(e),
                    },
                    other => errs.push(LangError::new(
                        "ATTRIBUTE",
                        m.span,
                        format!("a signal port takes only a unit, not '{other}'"),
                    )),
                }
            }
            if let Some(b) = &el.binding {
                if p.input {
                    errs.push(LangError::new(
                        "BINDING",
                        b.span,
                        format!(
                            "the input '{}' gets its value from a link; it cannot be given one",
                            el.name
                        ),
                    ));
                } else {
                    bindings.push(el.clone());
                }
            }
            let kind = if p.input { PortKind::Input { unit } } else { PortKind::Output { unit } };
            def.ports.push(PortDecl { name: el.name.clone(), kind, doc });
            continue;
        }
        if p.parameter {
            let mut decl = ParamDecl {
                name: el.name.clone(),
                unit: String::new(),
                display_unit: None,
                default: ParamValue::Real(Expr::Const(0.0)),
                min: None,
                max: None,
                structural: p.structural,
                doc,
            };
            let mut unit_span = None;
            for m in &el.mods {
                let r = match m.name.join(".").as_str() {
                    "unit" => string_value(m, "the unit").map(|u| {
                        unit_span = Some(m.span);
                        decl.unit = u;
                    }),
                    "displayUnit" => string_value(m, "the display unit").map(|u| {
                        decl.display_unit = Some(u);
                    }),
                    "min" => number_value(m).map(|v| decl.min = Some(v)),
                    "max" => number_value(m).map(|v| decl.max = Some(v)),
                    "quantity" => Ok(()),
                    other => Err(LangError::new(
                        "ATTRIBUTE",
                        m.span,
                        format!("{ATTRS_PARAM}, not '{other}'"),
                    )),
                };
                if let Err(e) = r {
                    errs.push(e);
                }
            }
            if el.annotation.iter().any(|a| {
                a.name.join(".") == "Evaluate"
                    && matches!(a.value.as_ref().map(|v| &v.kind), Some(ExprKind::Bool(true)))
            }) {
                decl.structural = true;
            }
            let Some(value) = &el.binding else {
                errs.push(LangError::new(
                    "PARAM-VALUE",
                    el.name_span,
                    format!(
                        "the parameter '{}' needs a value, as 'parameter Real {}(unit = \"…\") = 1'",
                        el.name, el.name
                    ),
                ));
                continue;
            };
            let default = match ty.as_str() {
                "Real" | "Integer" if is_table_call(value) => table_value(value),
                "Real" | "Integer" => Ok(ParamValue::Real(Expr::Const(0.0))), // set below
                "Boolean" => match &value.strip().kind {
                    ExprKind::Bool(b) => Ok(ParamValue::Bool(*b)),
                    _ => Err(LangError::new(
                        "PARAM-VALUE",
                        value.span,
                        format!("the Boolean parameter '{}' is true or false", el.name),
                    )),
                },
                "String" => Err(LangError::new(
                    "PARAM-TYPE",
                    el.type_span,
                    "text parameters are not supported".into(),
                )),
                _ => match &value.strip().kind {
                    ExprKind::Ref(parts) if parts.len() >= 2 => {
                        let q = parts.join(".");
                        let (vt, _) = split_enum_value(&q).expect("two parts at least");
                        if vt != ty {
                            Err(LangError::new(
                                "PARAM-VALUE",
                                value.span,
                                format!("'{q}' is not an option of {ty}"),
                            ))
                        } else {
                            Ok(ParamValue::Enum(q))
                        }
                    }
                    _ => Err(LangError::new(
                        "PARAM-VALUE",
                        value.span,
                        format!(
                            "the parameter '{}' of type {ty} takes one of its options, as {ty}.Option",
                            el.name
                        ),
                    )),
                },
            };
            match default {
                Ok(ParamValue::Real(_)) => match exprs::lower(value, &TextCx { def: &def }) {
                    Ok(e) => decl.default = ParamValue::Real(e),
                    Err(e) => errs.push(e),
                },
                Ok(v) => decl.default = v,
                Err(e) => errs.push(e),
            }
            spans.params.push((el.span, unit_span));
            def.params.push(decl);
            continue;
        }
        if scalar {
            let mut decl = VarDecl {
                name: el.name.clone(),
                unit: String::new(),
                display_unit: None,
                kind: if p.discrete { VarKind::Discrete } else { VarKind::Continuous },
                start: None,
                fixed: false,
                nominal: None,
                doc,
            };
            let mut unit_span = None;
            for m in &el.mods {
                let r = match m.name.join(".").as_str() {
                    "unit" => string_value(m, "the unit").map(|u| {
                        unit_span = Some(m.span);
                        decl.unit = u;
                    }),
                    "displayUnit" => string_value(m, "the display unit").map(|u| {
                        decl.display_unit = Some(u);
                    }),
                    "start" => match &m.value {
                        Some(v) => {
                            exprs::lower(v, &TextCx { def: &def }).map(|e| decl.start = Some(e))
                        }
                        None => Ok(()),
                    },
                    "fixed" => bool_value(m).map(|b| decl.fixed = b),
                    "nominal" => number_value(m).map(|v| decl.nominal = Some(v)),
                    "quantity" | "stateSelect" => Ok(()),
                    "min" | "max" => Err(LangError::new(
                        "ATTRIBUTE",
                        m.span,
                        "a variable does not keep min and max here: state the range with \
                         assert(…) in the equations"
                            .into(),
                    )),
                    other => Err(LangError::new(
                        "ATTRIBUTE",
                        m.span,
                        format!(
                            "a variable takes unit, displayUnit, start, fixed and nominal, not \
                             '{other}'"
                        ),
                    )),
                };
                if let Err(e) = r {
                    errs.push(e);
                }
            }
            if el.binding.is_some() {
                bindings.push(el.clone());
            }
            spans.vars.push((el.span, unit_span));
            def.vars.push(decl);
            continue;
        }
        if ty == "String" {
            errs.push(LangError::new(
                "UNSUPPORTED",
                el.type_span,
                "text variables are not supported".into(),
            ));
            continue;
        }
        // a sub-component
        if let Some(b) = &el.binding {
            errs.push(LangError::new(
                "BINDING",
                b.span,
                format!(
                    "the part '{}' cannot be given a value; give its parameters in parentheses",
                    el.name
                ),
            ));
        }
        let mut sub = SubDecl {
            name: el.name.clone(),
            def: ty,
            modifiers: vec![],
            label: el.doc.clone(),
            ui_id: None,
        };
        let mut mod_spans = vec![];
        for m in &el.mods {
            if m.name.len() != 1 || !m.mods.is_empty() {
                errs.push(LangError::new(
                    "MODIFIER",
                    m.span,
                    format!(
                        "give '{}' a parameter value as name = value; a part's parts cannot be \
                         changed from here",
                        el.name
                    ),
                ));
                continue;
            }
            let Some(v) = &m.value else {
                errs.push(LangError::new(
                    "MODIFIER",
                    m.span,
                    format!("the parameter '{}' of '{}' needs a value", m.name[0], el.name),
                ));
                continue;
            };
            let value = if is_table_call(v) {
                table_value(v)
            } else {
                match &v.strip().kind {
                    ExprKind::Bool(b) => Ok(ParamValue::Bool(*b)),
                    ExprKind::Ref(parts) if parts.len() >= 2 => {
                        Ok(ParamValue::Enum(parts.join(".")))
                    }
                    _ => exprs::lower(v, &TextCx { def: &def }).map(ParamValue::Real),
                }
            };
            match value {
                Ok(value) => {
                    if sub.modifiers.iter().any(|x| x.param == m.name[0]) {
                        errs.push(LangError::new(
                            "DUPLICATE",
                            m.span,
                            format!("'{}' is given twice to '{}'", m.name[0], el.name),
                        ));
                        continue;
                    }
                    sub.modifiers.push(Modifier { param: m.name[0].clone(), value });
                    mod_spans.push(m.span);
                }
                Err(e) => errs.push(e),
            }
        }
        for a in &el.annotation {
            if a.name.join(".") == "__LightSim" {
                for inner in &a.mods {
                    if inner.name.join(".") == "id" {
                        match string_value(inner, "the part's id") {
                            Ok(id) => sub.ui_id = Some(id),
                            Err(e) => errs.push(e),
                        }
                    }
                }
            }
        }
        spans.subs.push((el.span, mod_spans));
        def.components.push(sub);
    }
    // the clash Modelica forbids (DESIGN.md 5.4)
    for s in &def.components {
        if let Some(k) = def.params.iter().position(|p| p.name == s.name) {
            errs.push(LangError::new(
                "NAME-CLASH",
                spans.params[k].0,
                format!(
                    "'{}' names both a parameter and a part of '{}': give the parameter another \
                     name (for example '{}_value') and use that name in the part's parameters",
                    s.name, def.name, s.name
                ),
            ));
        }
    }
    (def, spans, bindings)
}

fn enum_type(
    c: &ClassDef,
    lits: &[(String, Option<String>, Span)],
    errs: &mut Vec<LangError>,
) -> EnumType {
    let mut out =
        EnumType { name: c.name.clone(), literals: vec![], doc: c.doc.clone().unwrap_or_default() };
    for (n, d, s) in lits {
        if out.literals.iter().any(|l| &l.name == n) {
            errs.push(LangError::new(
                "DUPLICATE",
                *s,
                format!("the option '{n}' appears twice in '{}'", c.name),
            ));
            continue;
        }
        out.literals.push(EnumLiteral { name: n.clone(), doc: d.clone().unwrap_or_default() });
    }
    out
}

fn lower_connector(c: &ClassDef, errs: &mut Vec<LangError>) -> Option<ConnectorDef> {
    let mut across = None;
    let mut through = None;
    for el in &c.elements {
        if el.type_name.join(".") != "Real" || el.prefixes.parameter || el.port {
            errs.push(LangError::new(
                "CONNECTOR",
                el.span,
                format!(
                    "a connector holds two Real quantities, one of them 'flow' (found '{}')",
                    el.name
                ),
            ));
            continue;
        }
        let mut unit = String::new();
        for m in &el.mods {
            match (m.name.join(".").as_str(), string_value(m, "the unit")) {
                ("unit", Ok(u)) => {
                    check_unit(&u, m.span, &format!("the quantity '{}'", el.name), errs);
                    unit = u;
                }
                ("unit", Err(e)) => errs.push(e),
                (other, _) => errs.push(LangError::new(
                    "ATTRIBUTE",
                    m.span,
                    format!("a connector's quantity takes only a unit, not '{other}'"),
                )),
            }
        }
        let q = QuantityDecl { name: el.name.clone(), unit };
        let slot = if el.prefixes.flow { &mut through } else { &mut across };
        if slot.is_some() {
            errs.push(LangError::new(
                "CONNECTOR",
                el.span,
                format!(
                    "the connector '{}' already has its {} quantity: a connector has one across \
                     quantity and one 'flow' quantity",
                    c.name,
                    if el.prefixes.flow { "flow" } else { "across" }
                ),
            ));
            continue;
        }
        *slot = Some(q);
    }
    let (Some(across), Some(through)) = (across, through) else {
        errs.push(LangError::new(
            "CONNECTOR",
            c.name_span,
            format!(
                "the connector '{}' needs one across quantity and one 'flow' quantity, as \
                 'Real v(unit = \"V\"); flow Real i(unit = \"A\");'",
                c.name
            ),
        ));
        return None;
    };
    let mut power = PowerRule::AcrossTimesThrough;
    for a in &c.annotation {
        if a.name.join(".") == "__LightSim_power" {
            match a.value.as_ref().map(|v| &v.kind) {
                Some(ExprKind::Str(s)) if s == "through" => power = PowerRule::ThroughIsPower,
                Some(ExprKind::Str(s)) if s == "across*through" => {}
                _ => errs.push(LangError::new(
                    "CONNECTOR",
                    a.span,
                    "__LightSim_power is \"across*through\" or \"through\"".into(),
                )),
            }
        }
    }
    Some(ConnectorDef {
        name: c.name.clone(),
        across,
        through,
        power,
        doc: c.doc.clone().unwrap_or_default(),
    })
}

/// Lowers an equation list (text format) into `out`, with each equation's
/// span in `spans`.
fn lower_equations(
    eqs: &[ast::Equation],
    def: &mut ComponentDef,
    initial: bool,
    errs: &mut Vec<LangError>,
    spans: &mut Vec<Span>,
    connects: &mut Vec<(Span, Span, Span)>,
) {
    for eq in eqs {
        let lowered = lower_equation(eq, &TextCx { def });
        match lowered {
            Ok(Lowered::Connect(c)) if !initial => {
                if let EqKind::Connect(a, b) = &eq.kind {
                    connects.push((eq.span, a.span, b.span));
                }
                def.connections.push(c);
            }
            Ok(Lowered::Connect(_)) => errs.push(LangError::new(
                "INITIAL",
                eq.span,
                "connect(…) belongs in the equation section, not in 'initial equation'".into(),
            )),
            Ok(Lowered::Equations(list)) => {
                for e in list {
                    spans.push(eq.span);
                    if initial {
                        def.initial_equations.push(e);
                    } else {
                        def.equations.push(e);
                    }
                }
            }
            Err(e) => errs.push(e),
        }
    }
}

pub(crate) enum Lowered {
    Connect(Connect),
    Equations(Vec<EquationDecl>),
}

/// Lowers one equation: a connect, or one or more equations (an `if`
/// equation gives one per branch equation; `when … elsewhen …` one per
/// branch, the first branch last so that it wins when two fire at once).
pub(crate) fn lower_equation(eq: &ast::Equation, cx: &dyn LowerCx) -> Result<Lowered, LangError> {
    let label = eq.doc.clone();
    Ok(match &eq.kind {
        EqKind::Simple(l, r) => {
            let (lhs, rhs) = (exprs::lower(l, cx)?, exprs::lower(r, cx)?);
            Lowered::Equations(vec![EquationDecl { eq: Equation::Eq { lhs, rhs }, label }])
        }
        EqKind::Connect(a, b) => {
            if eq.doc.is_some() {
                return Err(LangError::new(
                    "CONNECT",
                    eq.span,
                    "connect(…) takes no description; write a comment (// …) instead".into(),
                ));
            }
            let name = |e: &ast::Expr| match &e.kind {
                ExprKind::Ref(p) => p.join("."),
                _ => String::new(),
            };
            Lowered::Connect(Connect { a: name(a), b: name(b) })
        }
        EqKind::Call(call) => {
            let ExprKind::Call(name, args) = &call.kind else { unreachable!("parser") };
            match name.join(".").as_str() {
                "assert" => Lowered::Equations(vec![EquationDecl {
                    eq: lower_assert(args, call.span, cx)?,
                    label,
                }]),
                "reinit" => {
                    return Err(LangError::new(
                        "REINIT",
                        call.span,
                        "reinit(…) belongs inside a when-equation".into(),
                    ));
                }
                other => {
                    return Err(LangError::new(
                        "SYNTAX",
                        call.span,
                        format!(
                            "'{other}(…)' cannot stand alone as an equation; an equation is \
                             written 'left = right'"
                        ),
                    ));
                }
            }
        }
        EqKind::If(branches, otherwise) => {
            Lowered::Equations(lower_if(branches, otherwise, label, eq.span, cx)?)
        }
        EqKind::When(branches) => {
            let mut out = vec![];
            for (cond, body) in branches.iter().rev() {
                let condition = when_condition(cond, cx)?;
                let mut actions = vec![];
                for b in body {
                    actions.push(when_action(b, cx)?);
                }
                out.push(EquationDecl {
                    eq: Equation::When { condition, actions },
                    label: label.clone(),
                });
            }
            Lowered::Equations(out)
        }
    })
}

fn when_condition(cond: &ast::Expr, cx: &dyn LowerCx) -> Result<Expr, LangError> {
    if let ExprKind::Array(items) = &cond.strip().kind {
        let mut out: Option<Expr> = None;
        for i in items {
            let c = exprs::lower(i, cx)?;
            out = Some(match out {
                None => c,
                Some(o) => Expr::Or(Box::new(o), Box::new(c)),
            });
        }
        return out.ok_or_else(|| {
            LangError::new("WHEN", cond.span, "the list of conditions is empty".into())
        });
    }
    exprs::lower(cond, cx)
}

fn when_action(b: &ast::Equation, cx: &dyn LowerCx) -> Result<WhenAction, LangError> {
    match &b.kind {
        EqKind::Simple(l, r) => {
            let target = match &l.strip().kind {
                ExprKind::Ref(p) => exprs::lower(l, cx).and_then(|e| match e {
                    Expr::Name(n) => Ok(n),
                    _ => Err(LangError::new(
                        "WHEN",
                        l.span,
                        format!("'{}' cannot be assigned at an event", p.join(".")),
                    )),
                })?,
                _ => {
                    return Err(LangError::new(
                        "WHEN",
                        l.span,
                        "inside 'when', the left side is the variable that changes: 'v = new value'".into(),
                    ));
                }
            };
            Ok(WhenAction::Assign { var: target, value: exprs::lower(r, cx)? })
        }
        EqKind::Call(call) => {
            let ExprKind::Call(name, args) = &call.kind else { unreachable!("parser") };
            if name.join(".") != "reinit" {
                return Err(LangError::new(
                    "WHEN",
                    call.span,
                    format!(
                        "inside 'when' only assignments 'v = value' and reinit(x, value) are \
                         supported (found '{}(…)')",
                        name.join(".")
                    ),
                ));
            }
            if args.positional.len() != 2 || !args.named.is_empty() {
                return Err(LangError::new(
                    "CALL-ARGS",
                    call.span,
                    "reinit(…) takes the state and its new value".into(),
                ));
            }
            let var = match exprs::lower(&args.positional[0], cx)? {
                Expr::Name(n) => n,
                _ => {
                    return Err(LangError::new(
                        "WHEN",
                        args.positional[0].span,
                        "reinit(…) restarts a state: its first argument is the state's name".into(),
                    ));
                }
            };
            Ok(WhenAction::Reinit { var, value: exprs::lower(&args.positional[1], cx)? })
        }
        _ => Err(LangError::new(
            "WHEN",
            b.span,
            "inside 'when' only assignments 'v = value' and reinit(x, value) are supported".into(),
        )),
    }
}

/// assert(condition, "message" [, AssertionLevel.warning|error])
pub(crate) fn lower_assert(
    args: &ast::Args,
    span: Span,
    cx: &dyn LowerCx,
) -> Result<Equation, LangError> {
    let mut cond = args.positional.first();
    let mut msg = args.positional.get(1);
    let mut level = args.positional.get(2);
    for (k, s, v) in &args.named {
        match k.as_str() {
            "condition" => cond = Some(v),
            "message" => msg = Some(v),
            "level" => level = Some(v),
            other => {
                return Err(LangError::new(
                    "CALL-ARGS",
                    *s,
                    format!("assert() has no argument '{other}'"),
                ));
            }
        }
    }
    let (Some(cond), Some(msg)) = (cond, msg) else {
        return Err(LangError::new(
            "CALL-ARGS",
            span,
            "assert(…) takes a condition and a message: assert(x >= 0, \"x must not be negative\")"
                .into(),
        ));
    };
    let message = message_text(msg)?;
    let error = match level.map(|l| &l.strip().kind) {
        None => true,
        Some(ExprKind::Ref(p)) if p.last().map(String::as_str) == Some("warning") => false,
        Some(ExprKind::Ref(p)) if p.last().map(String::as_str) == Some("error") => true,
        Some(_) => {
            return Err(LangError::new(
                "CALL-ARGS",
                level.map(|l| l.span).unwrap_or(span),
                "the level of an assert is AssertionLevel.error or AssertionLevel.warning".into(),
            ));
        }
    };
    Ok(Equation::Assert { condition: exprs::lower(cond, cx)?, message, error })
}

/// The text of an assert message: a string, or strings joined with '+'
/// (values in String(…) are written as their expression's text).
fn message_text(e: &ast::Expr) -> Result<String, LangError> {
    match &e.strip().kind {
        ExprKind::Str(s) => Ok(s.clone()),
        ExprKind::Bin(ast::BinOp::Add, a, b) => Ok(message_text(a)? + &message_text(b)?),
        _ => Err(LangError::new(
            "CALL-ARGS",
            e.span,
            "the message of an assert is text in quotes".into(),
        )),
    }
}

/// An `if` equation as equations with if-expressions: each branch must
/// have the same number of equations; the k-th equations of the branches
/// become one equation (`v = if c then a else b` when every branch sets
/// the same left side, else `0 = if c then l1 - r1 else l2 - r2`).
fn lower_if(
    branches: &[(ast::Expr, Vec<ast::Equation>)],
    otherwise: &[ast::Equation],
    label: Option<String>,
    span: Span,
    cx: &dyn LowerCx,
) -> Result<Vec<EquationDecl>, LangError> {
    let mut conds = vec![];
    let mut bodies: Vec<Vec<EquationDecl>> = vec![];
    for (c, body) in branches {
        conds.push(exprs::lower(c, cx)?);
        bodies.push(flat_body(body, cx)?);
    }
    bodies.push(flat_body(otherwise, cx)?);
    let n = bodies[0].len();
    if let Some(k) = bodies.iter().position(|b| b.len() != n) {
        let which = if k + 1 == bodies.len() {
            "the 'else' part".to_string()
        } else {
            format!("branch {}", k + 1)
        };
        return Err(LangError::new(
            "IF-EQUATION",
            span,
            format!(
                "every branch of an 'if' equation must have as many equations as the first ({n}); \
                 {which} has {}",
                bodies[k].len()
            ),
        ));
    }
    let mut out = vec![];
    for k in 0..n {
        let sides: Vec<(&Expr, &Expr)> = bodies
            .iter()
            .map(|b| match &b[k].eq {
                Equation::Eq { lhs, rhs } => Ok((lhs, rhs)),
                _ => Err(LangError::new(
                    "IF-EQUATION",
                    span,
                    "an 'if' equation holds equations 'a = b' only (no when or assert)".into(),
                )),
            })
            .collect::<Result<_, _>>()?;
        let same_lhs = sides.iter().all(|(l, _)| *l == sides[0].0);
        let (lhs, mut acc) = if same_lhs {
            (sides[0].0.clone(), sides.last().expect("else").1.clone())
        } else {
            let (l, r) = sides.last().expect("else");
            (
                Expr::Const(0.0),
                Expr::Binary(BinaryOp::Sub, Box::new((*l).clone()), Box::new((*r).clone())),
            )
        };
        for (j, c) in conds.iter().enumerate().rev() {
            let (l, r) = sides[j];
            let v = if same_lhs {
                r.clone()
            } else {
                Expr::Binary(BinaryOp::Sub, Box::new(l.clone()), Box::new(r.clone()))
            };
            acc = Expr::If(Box::new(c.clone()), Box::new(v), Box::new(acc));
        }
        let lbl = bodies.iter().find_map(|b| b[k].label.clone()).or(label.clone());
        out.push(EquationDecl { eq: Equation::Eq { lhs, rhs: acc }, label: lbl });
    }
    Ok(out)
}

fn flat_body(body: &[ast::Equation], cx: &dyn LowerCx) -> Result<Vec<EquationDecl>, LangError> {
    let mut out = vec![];
    for e in body {
        match lower_equation(e, cx)? {
            Lowered::Equations(list) => out.extend(list),
            Lowered::Connect(_) => {
                return Err(LangError::new(
                    "IF-EQUATION",
                    e.span,
                    "connect(…) cannot be conditional".into(),
                ));
            }
        }
    }
    Ok(out)
}

fn energy(def: &mut ComponentDef, cls: &ClassDef, errs: &mut Vec<LangError>) -> Option<Span> {
    let mut span = None;
    for a in &cls.annotation {
        if a.name.join(".") != "__LightSim_energy" {
            continue;
        }
        span = Some(a.span);
        for m in &a.mods {
            let Some(v) = &m.value else { continue };
            let e = match exprs::lower(v, &TextCx { def }) {
                Ok(e) => e,
                Err(e) => {
                    errs.push(e);
                    continue;
                }
            };
            match m.name.join(".").as_str() {
                "stored" => def.energy.stored = Some(e),
                "loss" => def.energy.loss = Some(e),
                other => errs.push(LangError::new(
                    "ENERGY",
                    m.span,
                    format!("__LightSim_energy takes stored and loss, not '{other}'"),
                )),
            }
        }
    }
    span
}

/// The first name in `e` that is neither a parameter of `def` nor an
/// enumeration option: what a parameter's value or a start value may not
/// use.
fn non_parameter(e: &Expr, def: &ComponentDef, known: &Known) -> Option<String> {
    let mut bad = None;
    e.walk(&mut |x| {
        if let Expr::Name(n) = x
            && !def.params.iter().any(|q| &q.name == n)
        {
            let option = split_enum_value(n).is_some_and(|(t, l)| {
                known.enum_type(def, t).map_or(!known.strict(), |t| t.ordinal(l).is_some())
            });
            if !option {
                bad.get_or_insert(n.clone());
            }
        }
    });
    bad
}

/// Every check of a lowered component: names, units, values, connections.
/// Where a component's equations are in the text.
struct Placed<'a> {
    eqs: &'a [Span],
    init: &'a [Span],
    connects: &'a [(Span, Span, Span)],
    energy: Option<Span>,
}

fn check_component(
    cls: &ClassDef,
    def: &ComponentDef,
    spans: &DeclSpans,
    placed: &Placed,
    known: &Known,
    map: &SourceMap,
    errs: &mut Vec<LangError>,
) {
    let (eq_spans, init_spans, energy_span) = (placed.eqs, placed.init, placed.energy);
    let r = CompResolve { def, known };
    let who = format!("'{}'", def.name);
    // names in equations, as written
    let mut names = vec![];
    for eq in cls.equations.iter().chain(&cls.initial_equations) {
        eq_refs(eq, &mut names);
    }
    for a in &cls.annotation {
        for m in &a.mods {
            if let Some(v) = &m.value
                && a.name.join(".") == "__LightSim_energy"
            {
                refs(v, &mut names);
            }
        }
    }
    for (n, s) in &names {
        if r.name(n) == NameDim::Missing {
            errs.push(LangError::new("UNKNOWN-NAME", *s, r.why_missing(n)));
        }
    }
    // declared units
    for (k, p) in def.params.iter().enumerate() {
        let (decl, unit_span) = spans.params[k];
        let at = unit_span.unwrap_or(decl);
        if matches!(p.default, ParamValue::Real(_)) || p.default.table().is_some() {
            check_unit(&p.unit, at, &format!("the parameter '{}'", p.name), errs);
        }
        if let Some(du) = &p.display_unit {
            check_display_unit(&p.unit, du, decl, &format!("the parameter '{}'", p.name), errs);
        }
        if let Some(t) = p.default.table() {
            for (j, unit) in t.axis_units[..t.dims()].iter().enumerate() {
                check_unit(unit, decl, &format!("axis {} of the table '{}'", j + 1, p.name), errs);
            }
        }
        match &p.default {
            ParamValue::Real(e) => {
                if let Some(n) = non_parameter(e, def, known) {
                    errs.push(LangError::new(
                        "PARAM-VALUE",
                        decl,
                        format!(
                            "the value of the parameter '{}' uses '{n}', which is not a \
                             parameter of {who}",
                            p.name
                        ),
                    ));
                } else if let Ok(u) = parse_unit(&p.unit)
                    && let Err(why) = dims::expect(e, u.dim, &r)
                {
                    errs.push(LangError::new(
                        "UNIT-MISMATCH",
                        decl,
                        format!(
                            "In {who}, the value of the parameter '{}' does not have its unit \
                             ({}): {why}",
                            p.name, p.unit
                        ),
                    ));
                }
            }
            ParamValue::Enum(q) => {
                let (ty, lit) = split_enum_value(q).expect("checked when lowered");
                match known.enum_type(def, ty) {
                    Some(t) if t.ordinal(lit).is_none() => {
                        let opts: Vec<&str> = t.literals.iter().map(|l| l.name.as_str()).collect();
                        errs.push(LangError::new(
                            "PARAM-VALUE",
                            decl,
                            format!("'{lit}' is not an option of {ty} ({})", opts.join(", ")),
                        ));
                    }
                    None if known.strict() => errs.push(LangError::new(
                        "PARAM-TYPE",
                        decl,
                        format!("'{ty}' is not a type LightSim knows (an enumeration type is declared as type {ty} = enumeration(…))"),
                    )),
                    _ => {}
                }
            }
            _ => {}
        }
    }
    for (k, v) in def.vars.iter().enumerate() {
        let (decl, unit_span) = spans.vars[k];
        check_unit(&v.unit, unit_span.unwrap_or(decl), &format!("the variable '{}'", v.name), errs);
        if let Some(du) = &v.display_unit {
            check_display_unit(&v.unit, du, decl, &format!("the variable '{}'", v.name), errs);
        }
        if let Some(st) = &v.start {
            if let Some(n) = non_parameter(st, def, known) {
                errs.push(LangError::new(
                    "START-VALUE",
                    decl,
                    format!(
                        "the start value of '{}' uses '{n}', which is not a parameter of {who}",
                        v.name
                    ),
                ));
            } else if let Ok(u) = parse_unit(&v.unit)
                && let Err(why) = dims::expect(st, u.dim, &r)
            {
                errs.push(LangError::new(
                    "UNIT-MISMATCH",
                    decl,
                    format!(
                        "In {who}, the start value of '{}' does not have its unit ({}): {why}",
                        v.name, v.unit
                    ),
                ));
            }
        }
    }
    // the parts and their parameters
    for (k, s) in def.components.iter().enumerate() {
        let (decl, mod_spans) = &spans.subs[k];
        let Some(sd) = known.component(&s.def) else {
            if known.strict() {
                errs.push(LangError::new(
                    "UNKNOWN-COMPONENT",
                    *decl,
                    format!(
                        "'{}' is not a component LightSim knows (the part '{}')",
                        s.def, s.name
                    ),
                ));
            }
            continue;
        };
        for (j, m) in s.modifiers.iter().enumerate() {
            let at = mod_spans[j];
            let Some(p) = sd.params.iter().find(|p| p.name == m.param) else {
                let names: Vec<&str> = sd.params.iter().map(|p| p.name.as_str()).collect();
                errs.push(LangError::new(
                    "UNKNOWN-PARAMETER",
                    at,
                    format!(
                        "{} has no parameter '{}' (its parameters: {})",
                        s.def,
                        m.param,
                        if names.is_empty() { "none".to_string() } else { names.join(", ") }
                    ),
                ));
                continue;
            };
            let what = format!("the value given to {}.{}", s.name, m.param);
            match (&p.default, &m.value) {
                (ParamValue::Real(_), ParamValue::Real(e)) => {
                    if let Some(n) = non_parameter(e, def, known) {
                        errs.push(LangError::new(
                            "PARAM-VALUE",
                            at,
                            format!("{what} uses '{n}', which is not a parameter of {who}"),
                        ));
                    } else if let Ok(u) = parse_unit(&p.unit)
                        && let Err(why) = dims::expect(e, u.dim, &r)
                    {
                        errs.push(LangError::new(
                            "UNIT-MISMATCH",
                            at,
                            format!(
                                "In {who}, {what} does not have the parameter's unit ({}): {why}",
                                p.unit
                            ),
                        ));
                    }
                }
                (ParamValue::Bool(_), ParamValue::Bool(_)) => {}
                (ParamValue::Enum(d), ParamValue::Enum(v)) => {
                    let (dt, _) = split_enum_value(d).expect("qualified");
                    let (vt, lit) = split_enum_value(v).expect("qualified");
                    let t = known.enum_type(sd, dt);
                    if vt != dt || t.is_some_and(|t| t.ordinal(lit).is_none()) {
                        errs.push(LangError::new(
                            "PARAM-VALUE",
                            at,
                            format!(
                                "'{v}' is not an option of {dt}, the type of {}.{}",
                                s.name, m.param
                            ),
                        ));
                    }
                }
                (d, ParamValue::Real(Expr::Name(n))) if d.table().is_some() => {
                    if !def.params.iter().any(|q| &q.name == n && q.default.table().is_some()) {
                        errs.push(LangError::new(
                            "PARAM-VALUE",
                            at,
                            format!("{what} must be a table, as table(x = {{…}}, y = {{…}}), or a table parameter of {who}"),
                        ));
                    }
                }
                (d, v) if d.table().is_some() && v.table().is_some() => {
                    let (dt, vt) = (d.table().expect("table"), v.table().expect("table"));
                    if dt.dims() != vt.dims() {
                        errs.push(LangError::new(
                            "PARAM-VALUE",
                            at,
                            format!(
                                "{what} is a table of {} axes; {}.{} has {}",
                                vt.dims(),
                                s.def,
                                m.param,
                                dt.dims()
                            ),
                        ));
                    }
                }
                (d, _) => {
                    let kind = match d {
                        ParamValue::Real(_) => "a number or an expression of parameters",
                        ParamValue::Bool(_) => "true or false",
                        ParamValue::Enum(_) => "one of its type's options",
                        _ => "a table",
                    };
                    errs.push(LangError::new("PARAM-VALUE", at, format!("{what} must be {kind}")));
                }
            }
        }
    }
    // connections
    for (cn, s) in def.connections.iter().zip(placed.connects.iter().copied()) {
        let mut types = vec![];
        for (end, at) in [(&cn.a, s.1), (&cn.b, s.2)] {
            match port_of(def, end, known) {
                Ok(t) => types.push(t),
                Err(why) => {
                    errs.push(LangError::new("UNKNOWN-PORT", at, why));
                    types.push(None);
                }
            }
        }
        if let (Some(Some(a)), Some(Some(b))) = (types.first(), types.get(1)) {
            let same = match (a, b) {
                (PortKind::Physical { connector: x }, PortKind::Physical { connector: y }) => {
                    x == y
                }
                (PortKind::Physical { .. }, _) | (_, PortKind::Physical { .. }) => false,
                (
                    PortKind::Input { unit: x } | PortKind::Output { unit: x },
                    PortKind::Input { unit: y } | PortKind::Output { unit: y },
                ) => unit_dim(x) == unit_dim(y),
            };
            if !same {
                errs.push(LangError::new(
                    "CONNECT-MISMATCH",
                    s.0,
                    format!(
                        "'{}' and '{}' cannot be connected: they carry different quantities ({} \
                         and {})",
                        cn.a,
                        cn.b,
                        port_words(a),
                        port_words(b)
                    ),
                ));
            }
        }
    }
    // equations' units, each quoted as written
    for (list, spans_of, what) in [
        (&def.equations, eq_spans, "equation"),
        (&def.initial_equations, init_spans, "initial equation"),
    ] {
        for (e, at) in list.iter().zip(spans_of) {
            check_equation(&e.eq, *at, what, &who, def, &r, map, errs);
        }
    }
    if let Some(at) = energy_span {
        let joule = parse_unit("J").expect("J").dim;
        let watt = parse_unit("W").expect("W").dim;
        for (e, want, what) in
            [(&def.energy.stored, joule, "stored energy"), (&def.energy.loss, watt, "loss")]
        {
            if let Some(e) = e
                && let Err(why) = dims::expect(e, want, &r)
            {
                errs.push(LangError::new(
                    "UNIT-MISMATCH",
                    at,
                    format!(
                        "In {who}, the {what} “{}” is not in {}: {why}",
                        crate::print::expr_text(e),
                        if want == joule { "J" } else { "W" }
                    ),
                ));
            }
        }
    }
}

fn port_words(k: &PortKind) -> String {
    match k {
        PortKind::Physical { connector } => connector.clone(),
        PortKind::Input { unit } | PortKind::Output { unit } => format!("a signal in '{unit}'"),
    }
}

/// The kind of the port `end` names, `None` when it cannot be known here.
fn port_of(def: &ComponentDef, end: &str, known: &Known) -> Result<Option<PortKind>, String> {
    if let Some(p) = def.ports.iter().find(|p| p.name == end) {
        return Ok(Some(p.kind.clone()));
    }
    if let Some((head, rest)) = end.split_once('.')
        && let Some(s) = def.components.iter().find(|s| s.name == head)
    {
        return match known.component(&s.def) {
            Some(sd) => match sd.ports.iter().find(|p| p.name == rest) {
                Some(p) => Ok(Some(p.kind.clone())),
                None => {
                    let names: Vec<&str> = sd.ports.iter().map(|p| p.name.as_str()).collect();
                    Err(format!(
                        "'{head}' ({}) has no port '{rest}' (its ports: {})",
                        s.def,
                        if names.is_empty() { "none".into() } else { names.join(", ") }
                    ))
                }
            },
            None => Ok(None),
        };
    }
    Err(format!(
        "connect(…) names '{end}', which is not a port of '{}' or of one of its parts",
        def.name
    ))
}

fn eq_refs(eq: &ast::Equation, out: &mut Vec<(String, Span)>) {
    match &eq.kind {
        EqKind::Simple(a, b) => {
            refs(a, out);
            refs(b, out);
        }
        EqKind::Connect(..) => {}
        EqKind::Call(c) => {
            if let ExprKind::Call(n, args) = &c.kind {
                let skip_level = n.join(".") == "assert";
                for (k, a) in args.positional.iter().enumerate() {
                    if !(skip_level && k >= 1) {
                        refs(a, out);
                    }
                }
            }
        }
        EqKind::If(br, other) => {
            for (c, body) in br {
                refs(c, out);
                body.iter().for_each(|e| eq_refs(e, out));
            }
            other.iter().for_each(|e| eq_refs(e, out));
        }
        EqKind::When(br) => {
            for (c, body) in br {
                refs(c, out);
                body.iter().for_each(|e| eq_refs(e, out));
            }
        }
    }
}

/// Checks one equation's units (and a when's targets); the message
/// quotes the equation as written.
#[allow(clippy::too_many_arguments)]
pub(crate) fn check_equation(
    eq: &Equation,
    at: Span,
    what: &str,
    who: &str,
    def: &ComponentDef,
    r: &dyn Resolve,
    map: &SourceMap,
    errs: &mut Vec<LangError>,
) {
    let quote = || map.quote(at);
    match eq {
        Equation::Eq { lhs, rhs } => {
            if let Err(why) = dims::balance(lhs, rhs, r) {
                errs.push(LangError::new(
                    "UNIT-MISMATCH",
                    at,
                    format!(
                        "In {who}, the {what} “{}” does not balance its units: {why}.",
                        quote()
                    ),
                ));
            }
        }
        Equation::When { condition, actions } => {
            if let Err(m) = dims::dim(condition, r) {
                errs.push(LangError::new(
                    "UNIT-MISMATCH",
                    at,
                    format!(
                        "In {who}, the condition of “{}” does not balance its units: {}.",
                        quote(),
                        m.0
                    ),
                ));
            }
            for a in actions {
                let (WhenAction::Assign { var, value } | WhenAction::Reinit { var, value }) = a;
                let target = def.vars.iter().find(|v| &v.name == var);
                match (a, target) {
                    (_, None) => {
                        let kind = if def.params.iter().any(|p| &p.name == var) {
                            "a parameter"
                        } else if def.ports.iter().any(|p| &p.name == var) {
                            "a port"
                        } else {
                            "not one of its variables"
                        };
                        errs.push(LangError::new(
                            "WHEN",
                            at,
                            format!(
                                "In {who}, “{}” changes '{var}' at an event, but '{var}' is {kind}: \
                                 an event changes a discrete variable (declared 'discrete Real') \
                                 or restarts a state with reinit",
                                quote()
                            ),
                        ));
                        continue;
                    }
                    (WhenAction::Assign { .. }, Some(v)) if v.kind != VarKind::Discrete => {
                        errs.push(LangError::new(
                            "WHEN",
                            at,
                            format!(
                                "In {who}, “{}” assigns '{var}' at an event, but '{var}' is not \
                                 discrete: declare it 'discrete Real', or restart a state with \
                                 reinit({var}, …)",
                                quote()
                            ),
                        ));
                    }
                    _ => {}
                }
                if let Err(why) = dims::balance(&Expr::Name(var.clone()), value, r) {
                    errs.push(LangError::new(
                        "UNIT-MISMATCH",
                        at,
                        format!(
                            "In {who}, the event “{}” gives '{var}' a value in other units: {why}.",
                            quote()
                        ),
                    ));
                }
            }
        }
        Equation::Assert { condition, .. } => {
            if let Err(m) = dims::dim(condition, r) {
                errs.push(LangError::new(
                    "UNIT-MISMATCH",
                    at,
                    format!(
                        "In {who}, the condition of “{}” does not balance its units: {}.",
                        quote(),
                        m.0
                    ),
                ));
            }
        }
    }
}

/// What a text defines, in the order it defines it.
#[derive(Default)]
pub(crate) struct Parsed {
    pub connectors: Vec<ConnectorDef>,
    pub types: Vec<EnumType>,
    pub components: Vec<ComponentDef>,
}

/// Lowers a whole text: connectors, enumeration types and components.
pub(crate) fn lower_text(
    classes: &[ClassDef],
    map: &SourceMap,
    lib: Option<&Library>,
) -> Result<Parsed, Vec<LangError>> {
    let mut errs = vec![];
    let mut known = Known {
        lib,
        connectors: BTreeMap::new(),
        types: BTreeMap::new(),
        components: BTreeMap::new(),
    };
    let mut seen: BTreeMap<String, Span> = BTreeMap::new();
    let mut models = vec![];
    let mut out = Parsed::default();
    for c in classes {
        if let Some(prev) = seen.insert(c.name.clone(), c.name_span) {
            errs.push(LangError::new(
                "DUPLICATE",
                c.name_span,
                format!("'{}' is defined twice (first on line {})", c.name, prev.line),
            ));
            continue;
        }
        match (&c.kind, &c.short) {
            (ClassKind::Type, Some(Short::Enumeration(lits))) => {
                let t = enum_type(c, lits, &mut errs);
                known.types.insert(t.name.clone(), t.clone());
                out.types.push(t);
            }
            (ClassKind::Connector, None) => {
                if let Some(cd) = lower_connector(c, &mut errs) {
                    known.connectors.insert(cd.name.clone(), cd.clone());
                    out.connectors.push(cd);
                }
            }
            (ClassKind::Model | ClassKind::Block | ClassKind::Class, None) => models.push(c),
            (ClassKind::Type, Some(Short::Alias { .. })) => errs.push(LangError::new(
                "UNSUPPORTED",
                c.name_span,
                "type aliases are not supported here: give each declaration its unit".into(),
            )),
            (kind, _) => errs.push(LangError::new(
                "UNSUPPORTED",
                c.name_span,
                format!(
                    "a {} cannot be defined in the text format (it holds models, connectors and \
                     enumeration types; records, functions and packages are read by the Base \
                     Modelica import)",
                    kind.word()
                ),
            )),
        }
    }
    // declarations first, so parts may come from anywhere in the text
    let mut lowered = vec![];
    for c in &models {
        let (def, spans, bindings) = lower_decls(c, &mut errs);
        known.components.insert(def.name.clone(), def.clone());
        lowered.push((c, def, spans, bindings));
    }
    for (c, mut def, spans, bindings) in lowered {
        let mut eq_spans = vec![];
        let mut init_spans = vec![];
        // declarations with a value: equations first, in order
        for el in &bindings {
            let Some(b) = &el.binding else { continue };
            match exprs::lower(b, &TextCx { def: &def }) {
                Ok(rhs) => {
                    def.equations.push(EquationDecl {
                        eq: Equation::Eq { lhs: Expr::Name(el.name.clone()), rhs },
                        label: None,
                    });
                    eq_spans.push(el.span);
                }
                Err(e) => errs.push(e),
            }
        }
        let mut connects = vec![];
        lower_equations(&c.equations, &mut def, false, &mut errs, &mut eq_spans, &mut connects);
        lower_equations(
            &c.initial_equations,
            &mut def,
            true,
            &mut errs,
            &mut init_spans,
            &mut connects,
        );
        let energy_span = energy(&mut def, c, &mut errs);
        for a in &c.annotation {
            let n = a.name.join(".");
            if n.starts_with("__LightSim") && n != "__LightSim_energy" {
                errs.push(LangError::new(
                    "ANNOTATION",
                    a.span,
                    format!(
                        "'{n}' is not a LightSim annotation of a model (there is __LightSim_energy)"
                    ),
                ));
            }
        }
        check_component(
            c,
            &def,
            &spans,
            &Placed { eqs: &eq_spans, init: &init_spans, connects: &connects, energy: energy_span },
            &known,
            map,
            &mut errs,
        );
        out.components.push(def);
    }
    if errs.is_empty() {
        Ok(out)
    } else {
        errs.sort_by_key(|e| (e.span.start, e.span.end));
        errs.dedup();
        Err(errs)
    }
}
