//! Base Modelica import: the flat subset of Modelica that MCP-0031
//! defines, as a tool's flattener writes it, read into one component
//! definition.
//!
//! ```text
//! //! base 0.1.0
//! package 'RC'
//!   type 'Voltage' = Real(unit = "V");
//!   model 'RC' "a capacitor charged through a resistor"
//!     parameter Real 'R'(unit = "Ohm") = 50;
//!     parameter Real 'C'(unit = "F") = 1e-3;
//!     parameter 'Voltage' 'V' = 400;
//!     'Voltage' 'vC'(start = 0, fixed = true);
//!   equation
//!     'C' * der('vC') = ('V' - 'vC') / 'R';
//!   end 'RC';
//! end 'RC';
//! ```
//!
//! What is read:
//!
//! * a `package` holding types, records, functions, constants and one
//!   `model` (or those definitions without the package);
//! * `type` aliases of Real, Integer and Boolean with their attributes
//!   (`unit`, `displayUnit`, `min`, `max`, `start`, `fixed`, `nominal`),
//!   and enumeration types;
//! * records of scalars: a record variable becomes one variable per field
//!   (`'r'.'a'` is the variable `r.a`); record values come from
//!   modifiers, a record constructor `'R'(1, 2)` or another record;
//!   equations between records hold field by field;
//! * functions of scalars, inlined: inputs (with defaults), the first
//!   output, protected locals, an algorithm of assignments and `if`
//!   statements;
//! * parameters (`Evaluate = true` makes one structural), constants
//!   (replaced by their values), variables (Real, Integer, Boolean, an
//!   enumeration type; a variable assigned in a `when` is discrete),
//!   inputs and outputs (signal ports), bindings (equations), `min` and
//!   `max` of variables (warning asserts);
//! * equations: `=`, `if` equations, `when`/`elsewhen` with `reinit`,
//!   `assert`, the `initial equation` section; `der`, `pre`, `noEvent`,
//!   the scalar built-ins and the forms the expression lowering lists
//!   (`==`, `edge`, `smooth`, `homotopy` …).
//!
//! Units are checked as in the text format, each unbalanced equation
//! quoted; a declaration without a unit gets the one its equations imply
//! where they determine it (unit inference), else none (dimensionless).
//!
//! Not read (each with a plain message where it appears): arrays, `for`
//! loops, clocked equations, external functions, functions of records,
//! `initial()`, `sample`, `delay`, rounding functions; asserts inside
//! functions are not carried into the model.

use crate::ast::{self, Args, ClassDef, ClassKind, Element, ExprKind, ModArg, Short, StmtKind};
use crate::dims::{self, D, NameDim, Resolve};
use crate::exprs::{self, LowerCx};
use crate::lexer::SourceMap;
use crate::lower::{self, CompResolve, Known, Lowered};
use crate::{LangError, Span};
use lsim_ir::component::*;
use lsim_ir::expr::{BinaryOp, Builtin, Expr};
use lsim_ir::units::{Dim, describe, parse_unit};
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Imports a Base Modelica model (MCP-0031's flat subset) as one
/// component definition; its parameters, variables and equations are the
/// model's, with records expanded, functions inlined and constants
/// replaced by their values.
pub fn import(text: &str) -> Result<ComponentDef, Vec<LangError>> {
    let (classes, map) = crate::syntax(text)?;
    Importer::new(&map).run(&classes)
}

/// A scalar type: Real, Integer, Boolean, an alias of one, or an
/// enumeration.
#[derive(Clone, Debug)]
enum Scalar {
    Real,
    Integer,
    Boolean,
    Enum(String),
}

#[derive(Clone, Debug)]
struct TypeDef {
    base: Scalar,
    mods: Vec<ModArg>,
}

#[derive(Clone, Debug)]
struct RecordDef {
    /// field name, its type, its declaration (attributes, binding)
    fields: Vec<(String, Vec<String>, Element)>,
}

#[derive(Clone, Debug)]
struct FunctionDef {
    name: String,
    inputs: Vec<(String, Option<ast::Expr>)>,
    outputs: Vec<String>,
    locals: Vec<(String, Option<ast::Expr>)>,
    body: Vec<ast::Stmt>,
}

struct Importer<'a> {
    map: &'a SourceMap<'a>,
    errs: Vec<LangError>,
    types: BTreeMap<String, TypeDef>,
    enums: BTreeMap<String, EnumType>,
    records: BTreeMap<String, RecordDef>,
    functions: BTreeMap<String, FunctionDef>,
    /// constants: their values (lowered)
    consts: HashMap<String, Expr>,
    /// record variables: name → record type (for whole-record uses)
    record_vars: BTreeMap<String, String>,
}

/// Lowering context of the model's expressions.
struct ModelCx<'a, 'b> {
    imp: &'b Importer<'a>,
    depth: Cell<usize>,
}

impl LowerCx for ModelCx<'_, '_> {
    fn reference(&self, parts: &[String], span: Span) -> Result<Expr, LangError> {
        let n = parts.join(".");
        if let Some(v) = self.imp.consts.get(&n) {
            return Ok(v.clone());
        }
        if let Some(r) = self.imp.record_vars.get(&n) {
            return Err(LangError::new(
                "RECORD",
                span,
                format!("'{n}' is a record ({r}): use its fields, as {n}.field"),
            ));
        }
        Ok(Expr::Name(n))
    }

    fn call(&self, name: &[String], args: &Args, span: Span) -> Option<Result<Expr, LangError>> {
        let n = name.join(".");
        if let Some(f) = self.imp.functions.get(&n) {
            return Some(self.imp.inline(f, args, span, self, &self.depth));
        }
        if self.imp.records.contains_key(&n) {
            return Some(Err(LangError::new(
                "RECORD",
                span,
                format!(
                    "the record constructor '{n}(…)' can only give a record variable its value"
                ),
            )));
        }
        None
    }
}

/// Lowering context inside a function being inlined.
struct FnCx<'a, 'b> {
    imp: &'b Importer<'a>,
    f: &'b FunctionDef,
    env: &'b HashMap<String, Expr>,
    depth: &'b Cell<usize>,
}

impl LowerCx for FnCx<'_, '_> {
    fn reference(&self, parts: &[String], span: Span) -> Result<Expr, LangError> {
        let n = parts.join(".");
        if let Some(v) = self.env.get(&n) {
            return Ok(v.clone());
        }
        if let Some(v) = self.imp.consts.get(&n) {
            return Ok(v.clone());
        }
        let mut known = self
            .f
            .inputs
            .iter()
            .map(|x| &x.0)
            .chain(&self.f.outputs)
            .chain(self.f.locals.iter().map(|x| &x.0));
        let why = if known.any(|k| k == &n) {
            format!("'{n}' is used in the function '{}' before it has a value", self.f.name)
        } else {
            format!(
                "'{n}' is not an input, output or local of the function '{}' (a function reads \
                 only its inputs and constants)",
                self.f.name
            )
        };
        Err(LangError::new("FUNCTION", span, why))
    }

    fn call(&self, name: &[String], args: &Args, span: Span) -> Option<Result<Expr, LangError>> {
        let f = self.imp.functions.get(&name.join("."))?;
        Some(self.imp.inline(f, args, span, self, self.depth))
    }
}

/// Unit text for a dimension: a named unit when there is one.
fn unit_text(d: Dim) -> String {
    let named = describe(d);
    if !named.contains(' ') && parse_unit(&named).map(|u| u.dim) == Ok(d) {
        return named;
    }
    if d.is_none() { "1".into() } else { d.to_string() }
}

impl<'a> Importer<'a> {
    fn new(map: &'a SourceMap<'a>) -> Self {
        Importer {
            map,
            errs: vec![],
            types: BTreeMap::new(),
            enums: BTreeMap::new(),
            records: BTreeMap::new(),
            functions: BTreeMap::new(),
            consts: HashMap::new(),
            record_vars: BTreeMap::new(),
        }
    }

    fn err(&mut self, code: &'static str, span: Span, msg: String) {
        self.errs.push(LangError::new(code, span, msg));
    }

    fn run(mut self, classes: &[ClassDef]) -> Result<ComponentDef, Vec<LangError>> {
        // the package, or the definitions at the top
        let items: Vec<&ClassDef> = match classes {
            [p] if p.kind == ClassKind::Package => {
                for el in &p.elements {
                    self.global_constant(el);
                }
                p.classes.iter().collect()
            }
            _ => classes.iter().collect(),
        };
        let mut models = vec![];
        for c in &items {
            match (c.kind, &c.short) {
                (ClassKind::Type, Some(Short::Enumeration(lits))) => {
                    let t = EnumType {
                        name: c.name.clone(),
                        literals: lits
                            .iter()
                            .map(|(n, d, _)| EnumLiteral {
                                name: n.clone(),
                                doc: d.clone().unwrap_or_default(),
                            })
                            .collect(),
                        doc: c.doc.clone().unwrap_or_default(),
                    };
                    self.enums.insert(c.name.clone(), t);
                }
                (ClassKind::Type, Some(Short::Alias { base, base_span, mods })) => {
                    let b = base.join(".");
                    let Some(scalar) = self.scalar_of(&b) else {
                        self.err(
                            "TYPE",
                            *base_span,
                            format!(
                                "the type '{}' stands for '{b}', which is not Real, Integer, \
                                 Boolean or a type defined here",
                                c.name
                            ),
                        );
                        continue;
                    };
                    // an alias of an alias inherits its attributes
                    let mut all = self.types.get(&b).map(|t| t.mods.clone()).unwrap_or_default();
                    all.extend(mods.iter().cloned());
                    self.types.insert(c.name.clone(), TypeDef { base: scalar, mods: all });
                }
                (ClassKind::Record, None) => self.record(c),
                (ClassKind::Function, None) => self.function(c),
                (ClassKind::Model | ClassKind::Block | ClassKind::Class, None) => models.push(*c),
                (ClassKind::Package, _) => self.err(
                    "UNSUPPORTED",
                    c.name_span,
                    "a package inside the package is not part of Base Modelica's flat form".into(),
                ),
                (kind, _) => self.err(
                    "UNSUPPORTED",
                    c.name_span,
                    format!("a {} is not read by the Base Modelica import", kind.word()),
                ),
            }
        }
        let model = match models.as_slice() {
            [m] => *m,
            [] => {
                let span = classes.first().map(|c| c.name_span).unwrap_or_default();
                self.err("NO-MODEL", span, "the text defines no model to import".into());
                return Err(self.errs);
            }
            [_, second, ..] => {
                self.err(
                    "MODELS",
                    second.name_span,
                    "Base Modelica holds one model; the text defines more than one".into(),
                );
                return Err(self.errs);
            }
        };
        let def = self.model(model);
        if self.errs.is_empty() {
            Ok(def)
        } else {
            self.errs.sort_by_key(|e| (e.span.start, e.span.end));
            self.errs.dedup();
            Err(self.errs)
        }
    }

    fn scalar_of(&self, type_name: &str) -> Option<Scalar> {
        match type_name {
            "Real" => Some(Scalar::Real),
            "Integer" => Some(Scalar::Integer),
            "Boolean" => Some(Scalar::Boolean),
            other => {
                if self.enums.contains_key(other) {
                    return Some(Scalar::Enum(other.to_string()));
                }
                self.types.get(other).map(|t| t.base.clone())
            }
        }
    }

    fn global_constant(&mut self, el: &Element) {
        if !el.prefixes.constant {
            self.err(
                "UNSUPPORTED",
                el.span,
                "a package holds only constants besides its definitions".into(),
            );
            return;
        }
        let value = {
            let cx = ModelCx { imp: self, depth: Cell::new(0) };
            match &el.binding {
                Some(b) => exprs::lower(b, &cx),
                None => Err(LangError::new(
                    "CONSTANT",
                    el.name_span,
                    format!("the constant '{}' has no value", el.name),
                )),
            }
        };
        match value {
            Ok(v) => {
                self.consts.insert(el.name.clone(), v);
            }
            Err(e) => self.errs.push(e),
        }
    }

    fn record(&mut self, c: &ClassDef) {
        let mut fields = vec![];
        for el in &c.elements {
            let ty = el.type_name.join(".");
            if self.scalar_of(&ty).is_none() {
                self.err(
                    "RECORD",
                    el.type_span,
                    format!(
                        "the record '{}' holds '{ty}': records of scalars only (Real, Integer, \
                         Boolean, their types and enumerations)",
                        c.name
                    ),
                );
                continue;
            }
            fields.push((el.name.clone(), el.type_name.clone(), el.clone()));
        }
        if !c.equations.is_empty() {
            self.err(
                "RECORD",
                c.name_span,
                format!("the record '{}' cannot hold equations", c.name),
            );
        }
        self.records.insert(c.name.clone(), RecordDef { fields });
    }

    fn function(&mut self, c: &ClassDef) {
        let mut f = FunctionDef {
            name: c.name.clone(),
            inputs: vec![],
            outputs: vec![],
            locals: vec![],
            body: c.algorithm.clone(),
        };
        let mut output_bindings = vec![];
        for el in &c.elements {
            let ty = el.type_name.join(".");
            if self.scalar_of(&ty).is_none() {
                self.err(
                    "FUNCTION",
                    el.type_span,
                    format!("the function '{}' uses '{ty}': functions of scalars only", c.name),
                );
                continue;
            }
            if el.prefixes.input {
                f.inputs.push((el.name.clone(), el.binding.clone()));
            } else if el.prefixes.output {
                f.outputs.push(el.name.clone());
                if let Some(b) = &el.binding {
                    output_bindings.push(ast::Stmt {
                        kind: StmtKind::Assign(
                            ast::Expr {
                                kind: ExprKind::Ref(vec![el.name.clone()]),
                                span: el.name_span,
                            },
                            b.clone(),
                        ),
                        span: el.span,
                    });
                }
            } else {
                f.locals.push((el.name.clone(), el.binding.clone()));
            }
        }
        output_bindings.append(&mut f.body);
        f.body = output_bindings;
        if f.outputs.is_empty() {
            self.err("FUNCTION", c.name_span, format!("the function '{}' has no output", c.name));
        }
        if !c.equations.is_empty() {
            self.err(
                "FUNCTION",
                c.name_span,
                format!(
                    "the function '{}' has equations; a function computes with an algorithm",
                    c.name
                ),
            );
        }
        self.functions.insert(c.name.clone(), f);
    }

    /// Inlines a call of `f`: its algorithm executed symbolically.
    fn inline(
        &self,
        f: &FunctionDef,
        args: &Args,
        span: Span,
        outer: &dyn LowerCx,
        depth: &Cell<usize>,
    ) -> Result<Expr, LangError> {
        if depth.get() > 32 {
            return Err(LangError::new(
                "FUNCTION",
                span,
                format!(
                    "the function '{}' calls itself (directly or not): recursion is not supported",
                    f.name
                ),
            ));
        }
        if args.positional.len() > f.inputs.len() {
            return Err(LangError::new(
                "CALL-ARGS",
                span,
                format!(
                    "'{}' takes {} inputs, but is given {}",
                    f.name,
                    f.inputs.len(),
                    args.positional.len()
                ),
            ));
        }
        let mut given: HashMap<String, Expr> = HashMap::new();
        for (k, a) in args.positional.iter().enumerate() {
            given.insert(f.inputs[k].0.clone(), exprs::lower(a, outer)?);
        }
        for (n, s, a) in &args.named {
            if !f.inputs.iter().any(|(i, _)| i == n) {
                return Err(LangError::new(
                    "CALL-ARGS",
                    *s,
                    format!("'{}' has no input '{n}'", f.name),
                ));
            }
            given.insert(n.clone(), exprs::lower(a, outer)?);
        }
        depth.set(depth.get() + 1);
        let result = (|| {
            let mut env: HashMap<String, Expr> = HashMap::new();
            for (n, default) in &f.inputs {
                let v = match (given.remove(n), default) {
                    (Some(v), _) => v,
                    (None, Some(d)) => exprs::lower(d, &FnCx { imp: self, f, env: &env, depth })?,
                    (None, None) => {
                        return Err(LangError::new(
                            "CALL-ARGS",
                            span,
                            format!("the call of '{}' gives no value to its input '{n}'", f.name),
                        ));
                    }
                };
                env.insert(n.clone(), v);
            }
            for (n, b) in &f.locals {
                if let Some(b) = b {
                    let v = exprs::lower(b, &FnCx { imp: self, f, env: &env, depth })?;
                    env.insert(n.clone(), v);
                }
            }
            self.run_body(f, &f.body, &mut env, depth)?;
            let out = &f.outputs[0];
            let v = env.remove(out).ok_or_else(|| {
                LangError::new(
                    "FUNCTION",
                    span,
                    format!("the function '{}' never gives its output '{out}' a value", f.name),
                )
            })?;
            if v.size() > 200_000 {
                return Err(LangError::new(
                    "FUNCTION",
                    span,
                    format!("inlining '{}' gives an expression too large to compile well", f.name),
                ));
            }
            Ok(v)
        })();
        depth.set(depth.get() - 1);
        result
    }

    fn run_body(
        &self,
        f: &FunctionDef,
        body: &[ast::Stmt],
        env: &mut HashMap<String, Expr>,
        depth: &Cell<usize>,
    ) -> Result<(), LangError> {
        let partial = |name: &str, span: Span| {
            LangError::new(
                "FUNCTION",
                span,
                format!(
                    "'{name}' gets a value in some branches of this 'if' only, and has none before it"
                ),
            )
        };
        for (k, st) in body.iter().enumerate() {
            match &st.kind {
                StmtKind::Assign(target, value) => {
                    let ExprKind::Ref(parts) = &target.strip().kind else {
                        return Err(LangError::new(
                            "FUNCTION",
                            target.span,
                            "only a single variable can be assigned here".into(),
                        ));
                    };
                    let n = parts.join(".");
                    let known = f.outputs.contains(&n) || f.locals.iter().any(|(l, _)| l == &n);
                    if !known {
                        return Err(LangError::new(
                            "FUNCTION",
                            target.span,
                            format!(
                                "'{n}' is not an output or local of the function '{}' (inputs \
                                 cannot be assigned)",
                                f.name
                            ),
                        ));
                    }
                    let v = exprs::lower(value, &FnCx { imp: self, f, env, depth })?;
                    env.insert(n, v);
                }
                StmtKind::If(branches, otherwise) => {
                    let mut conds = vec![];
                    let mut envs = vec![];
                    for (c, b) in branches {
                        conds.push(exprs::lower(c, &FnCx { imp: self, f, env, depth })?);
                        let mut e = env.clone();
                        self.run_body(f, b, &mut e, depth)?;
                        envs.push(e);
                    }
                    let mut e = env.clone();
                    self.run_body(f, otherwise, &mut e, depth)?;
                    envs.push(e);
                    let mut assigned: BTreeSet<String> = BTreeSet::new();
                    for e in &envs {
                        for (name, v) in e {
                            if env.get(name) != Some(v) {
                                assigned.insert(name.clone());
                            }
                        }
                    }
                    for name in assigned {
                        let last = envs.last().and_then(|e| e.get(&name));
                        let mut acc = last.cloned().ok_or_else(|| partial(&name, st.span))?;
                        for (c, e) in conds.iter().zip(&envs).rev() {
                            let v = e.get(&name).cloned().ok_or_else(|| partial(&name, st.span))?;
                            acc = Expr::If(Box::new(c.clone()), Box::new(v), Box::new(acc));
                        }
                        env.insert(name, acc);
                    }
                }
                StmtKind::Return => {
                    if k + 1 != body.len() {
                        return Err(LangError::new(
                            "FUNCTION",
                            st.span,
                            "'return' is supported as the last statement only".into(),
                        ));
                    }
                }
                StmtKind::Call(c) => {
                    let ExprKind::Call(n, _) = &c.kind else { unreachable!("parser") };
                    if n.join(".") != "assert" {
                        return Err(LangError::new(
                            "FUNCTION",
                            st.span,
                            format!("'{}(…)' cannot stand alone in a function here", n.join(".")),
                        ));
                    }
                    // a function's own checks are not carried into the model
                }
            }
        }
        Ok(())
    }

    /// The attributes of a declaration: its type's, then its own.
    fn attrs(&self, el: &Element) -> Vec<ModArg> {
        let mut all =
            self.types.get(&el.type_name.join(".")).map(|t| t.mods.clone()).unwrap_or_default();
        all.extend(el.mods.iter().cloned());
        all
    }

    fn model(&mut self, m: &ClassDef) -> ComponentDef {
        let mut def = ComponentDef {
            name: m.name.clone(),
            doc: m.doc.clone().unwrap_or_default(),
            types: self.enums.values().cloned().collect(),
            ..Default::default()
        };
        for c in &m.classes {
            self.err(
                "UNSUPPORTED",
                c.name_span,
                "definitions belong in the package, not inside the model".into(),
            );
        }
        // what `when` clauses assign is discrete
        let mut assigned_in_when = BTreeSet::new();
        for e in &m.equations {
            when_targets(e, false, &mut assigned_in_when);
        }
        // records expand into one declaration per field
        let mut decls: Vec<Element> = vec![];
        for el in &m.elements {
            let ty = el.type_name.join(".");
            if let Some(r) = self.records.get(&ty).cloned() {
                self.record_vars.insert(el.name.clone(), ty.clone());
                decls.extend(self.expand_record(el, &ty, &r));
            } else {
                decls.push(el.clone());
            }
        }
        for el in &decls {
            if el.prefixes.constant {
                self.global_constant(el);
            }
        }
        let mut eq_spans: Vec<Span> = vec![];
        let mut decl_spans: HashMap<String, Span> = HashMap::new();
        let mut declared_units: BTreeSet<String> = BTreeSet::new();
        let mut range_asserts = vec![];
        for el in &decls {
            if el.prefixes.constant {
                continue;
            }
            if el.port {
                self.err("UNSUPPORTED", el.span, "ports are not part of Base Modelica".into());
                continue;
            }
            let ty = el.type_name.join(".");
            let Some(scalar) = self.scalar_of(&ty) else {
                self.err(
                    "TYPE",
                    el.type_span,
                    format!(
                        "'{ty}' is not a type this model can use (Real, Integer, Boolean, or a \
                         type, enumeration or record defined in the package)"
                    ),
                );
                continue;
            };
            decl_spans.insert(el.name.clone(), el.span);
            let attrs = self.attrs(el);
            let mut unit = String::new();
            let mut display = None;
            let (mut min, mut max, mut nominal) = (None, None, None);
            let mut start = None;
            let mut fixed = None;
            for a in &attrs {
                let name = a.name.join(".");
                let res: Result<(), LangError> = match name.as_str() {
                    "unit" => str_attr(a).map(|s| {
                        unit = s;
                        declared_units.insert(el.name.clone());
                    }),
                    "displayUnit" => str_attr(a).map(|s| display = Some(s)),
                    "min" => num_attr(a).map(|v| min = Some(v)),
                    "max" => num_attr(a).map(|v| max = Some(v)),
                    "nominal" => num_attr(a).map(|v| nominal = Some(v)),
                    "start" => {
                        start = a.value.clone();
                        Ok(())
                    }
                    "fixed" => match a.value.as_ref().map(|v| &v.strip().kind) {
                        Some(ExprKind::Bool(b)) => {
                            fixed = Some(*b);
                            Ok(())
                        }
                        _ => Err(LangError::new(
                            "ATTRIBUTE",
                            a.span,
                            "fixed is true or false".into(),
                        )),
                    },
                    "quantity" | "stateSelect" | "unbounded" => Ok(()),
                    other => Err(LangError::new(
                        "ATTRIBUTE",
                        a.span,
                        format!("'{other}' is not an attribute LightSim reads"),
                    )),
                };
                if let Err(e) = res {
                    self.errs.push(e);
                }
            }
            if !unit.is_empty() {
                lower::check_unit(&unit, el.span, &format!("'{}'", el.name), &mut self.errs);
            }
            let doc = el.doc.clone().unwrap_or_default();
            let evaluate = el.annotation.iter().any(|a| {
                a.name.join(".") == "Evaluate"
                    && matches!(a.value.as_ref().map(|v| &v.kind), Some(ExprKind::Bool(true)))
            });
            if el.prefixes.parameter {
                if fixed == Some(false) {
                    self.err(
                        "UNSUPPORTED",
                        el.span,
                        format!(
                            "'{}' is a parameter computed at initialisation (fixed = false), \
                             which is not supported",
                            el.name
                        ),
                    );
                    continue;
                }
                let Some(value) = el.binding.as_ref().or(start.as_ref()) else {
                    self.err(
                        "PARAM-VALUE",
                        el.name_span,
                        format!("the parameter '{}' has no value", el.name),
                    );
                    continue;
                };
                let default = {
                    let cx = ModelCx { imp: self, depth: Cell::new(0) };
                    match &scalar {
                        Scalar::Boolean => match exprs::lower(value, &cx) {
                            Ok(Expr::Const(v)) => Ok(ParamValue::Bool(v != 0.0)),
                            other => other.map(ParamValue::Real),
                        },
                        Scalar::Enum(t) => match &value.strip().kind {
                            ExprKind::Ref(parts)
                                if parts.len() >= 2 && parts[..parts.len() - 1].join(".") == *t =>
                            {
                                Ok(ParamValue::Enum(parts.join(".")))
                            }
                            _ => exprs::lower(value, &cx).map(ParamValue::Real),
                        },
                        _ => exprs::lower(value, &cx).map(ParamValue::Real),
                    }
                };
                match default {
                    Ok(default) => def.params.push(ParamDecl {
                        name: el.name.clone(),
                        unit,
                        display_unit: display,
                        default,
                        min,
                        max,
                        structural: evaluate,
                        doc,
                    }),
                    Err(e) => self.errs.push(e),
                }
                continue;
            }
            if el.prefixes.input {
                def.ports.push(PortDecl {
                    name: el.name.clone(),
                    kind: PortKind::Input { unit },
                    doc,
                });
                continue;
            }
            let lowered_start = {
                let cx = ModelCx { imp: self, depth: Cell::new(0) };
                start.as_ref().map(|s| exprs::lower(s, &cx))
            };
            let start_expr = match lowered_start {
                Some(Ok(e)) => Some(e),
                Some(Err(e)) => {
                    self.errs.push(e);
                    None
                }
                None => None,
            };
            if el.prefixes.output {
                def.ports.push(PortDecl {
                    name: el.name.clone(),
                    kind: PortKind::Output { unit },
                    doc,
                });
            } else {
                let discrete = el.prefixes.discrete || assigned_in_when.contains(&el.name);
                def.vars.push(VarDecl {
                    name: el.name.clone(),
                    unit,
                    display_unit: display,
                    kind: if discrete { VarKind::Discrete } else { VarKind::Continuous },
                    start: start_expr,
                    fixed: fixed.unwrap_or(false),
                    nominal,
                    doc,
                });
            }
            if let Some(b) = &el.binding {
                let rhs = {
                    let cx = ModelCx { imp: self, depth: Cell::new(0) };
                    exprs::lower(b, &cx)
                };
                match rhs {
                    Ok(rhs) => {
                        def.equations.push(EquationDecl {
                            eq: Equation::Eq { lhs: Expr::Name(el.name.clone()), rhs },
                            label: None,
                        });
                        eq_spans.push(el.span);
                    }
                    Err(e) => self.errs.push(e),
                }
            }
            for (bound, op, word) in
                [(min, lsim_ir::CmpOp::Ge, "minimum"), (max, lsim_ir::CmpOp::Le, "maximum")]
            {
                if let Some(b) = bound {
                    range_asserts.push((
                        EquationDecl {
                            eq: Equation::Assert {
                                condition: lsim_ir::expr::cmp(
                                    op,
                                    Expr::Name(el.name.clone()),
                                    Expr::Const(b),
                                ),
                                message: format!("'{}' is past its {word} {b}", el.name),
                                error: false,
                            },
                            label: Some(format!("the {word} of '{}'", el.name)),
                        },
                        el.span,
                    ));
                }
            }
        }
        // equations
        let mut init_spans = vec![];
        for (list, initial) in [(&m.equations, false), (&m.initial_equations, true)] {
            for eq in list {
                match self.equation(eq) {
                    Ok(eqs) => {
                        for e in eqs {
                            if initial {
                                def.initial_equations.push(e);
                                init_spans.push(eq.span);
                            } else {
                                def.equations.push(e);
                                eq_spans.push(eq.span);
                            }
                        }
                    }
                    Err(e) => self.errs.push(e),
                }
            }
        }
        for (a, s) in range_asserts {
            def.equations.push(a);
            eq_spans.push(s);
        }
        // energy books, if a LightSim tool wrote them
        for a in &m.annotation {
            if a.name.join(".") != "__LightSim_energy" {
                continue;
            }
            for x in &a.mods {
                let Some(v) = &x.value else { continue };
                let e = {
                    let cx = ModelCx { imp: self, depth: Cell::new(0) };
                    exprs::lower(v, &cx)
                };
                match (x.name.join(".").as_str(), e) {
                    ("stored", Ok(e)) => def.energy.stored = Some(e),
                    ("loss", Ok(e)) => def.energy.loss = Some(e),
                    (_, Err(e)) => self.errs.push(e),
                    (other, _) => {
                        self.err("ENERGY", x.span, format!("__LightSim_energy has no '{other}'"))
                    }
                }
            }
        }
        if !self.errs.is_empty() {
            return def;
        }
        infer_units(&mut def, &declared_units);
        self.check(m, &def, &eq_spans, &init_spans, &decl_spans);
        def
    }

    /// One declaration per field of a record variable.
    fn expand_record(&mut self, el: &Element, ty: &str, r: &RecordDef) -> Vec<Element> {
        // values: from a constructor, another record, or modifiers
        let mut values: HashMap<String, ast::Expr> = HashMap::new();
        if let Some(b) = &el.binding {
            match &b.strip().kind {
                ExprKind::Call(n, args) if n.join(".") == ty => {
                    for (k, a) in args.positional.iter().enumerate() {
                        match r.fields.get(k) {
                            Some((f, _, _)) => {
                                values.insert(f.clone(), a.clone());
                            }
                            None => self.err(
                                "RECORD",
                                a.span,
                                format!("the record '{ty}' has {} fields", r.fields.len()),
                            ),
                        }
                    }
                    for (n, s, a) in &args.named {
                        if r.fields.iter().any(|(f, _, _)| f == n) {
                            values.insert(n.clone(), a.clone());
                        } else {
                            self.err("RECORD", *s, format!("the record '{ty}' has no field '{n}'"));
                        }
                    }
                }
                ExprKind::Ref(parts)
                    if self.record_vars.get(&parts.join(".")).is_some_and(|t| t == ty) =>
                {
                    for (f, _, _) in &r.fields {
                        let mut p = parts.clone();
                        p.push(f.clone());
                        values
                            .insert(f.clone(), ast::Expr { kind: ExprKind::Ref(p), span: b.span });
                    }
                }
                _ => self.err(
                    "RECORD",
                    b.span,
                    format!(
                        "the record '{}' takes its value from {ty}(…) or another {ty}",
                        el.name
                    ),
                ),
            }
        }
        let mut field_mods: HashMap<String, Vec<ModArg>> = HashMap::new();
        for m in &el.mods {
            let Some(first) = m.name.first() else { continue };
            if !r.fields.iter().any(|(f, _, _)| f == first) {
                self.err("RECORD", m.span, format!("the record '{ty}' has no field '{first}'"));
                continue;
            }
            let mut inner = m.clone();
            inner.name.remove(0);
            if inner.name.is_empty() {
                // `r(a = 1)` or `r(a(start = 1))`
                field_mods.entry(first.clone()).or_default().extend(inner.mods.clone());
                if let Some(v) = &inner.value {
                    values.insert(first.clone(), v.clone());
                }
            } else {
                // `r(a.start = 1)`
                field_mods.entry(first.clone()).or_default().push(inner);
            }
        }
        r.fields
            .iter()
            .map(|(f, fty, decl)| {
                let mut mods = decl.mods.clone();
                mods.extend(field_mods.remove(f).unwrap_or_default());
                Element {
                    span: el.span,
                    prefixes: el.prefixes.clone(),
                    port: false,
                    protected: false,
                    type_name: fty.clone(),
                    type_span: el.type_span,
                    name: format!("{}.{f}", el.name),
                    name_span: el.name_span,
                    mods,
                    binding: values.remove(f).or_else(|| decl.binding.clone()),
                    doc: decl.doc.clone().or_else(|| el.doc.clone()),
                    annotation: el.annotation.clone(),
                }
            })
            .collect()
    }

    /// Lowers one equation; equations between records hold per field.
    fn equation(&self, eq: &ast::Equation) -> Result<Vec<EquationDecl>, LangError> {
        let cx = ModelCx { imp: self, depth: Cell::new(0) };
        if let ast::EqKind::Simple(l, r) = &eq.kind
            && let Some(fields) = self.record_sides(l, r)
        {
            let mut out = vec![];
            for (fl, fr) in fields {
                let lhs = exprs::lower(&fl, &cx)?;
                let rhs = exprs::lower(&fr, &cx)?;
                out.push(EquationDecl { eq: Equation::Eq { lhs, rhs }, label: eq.doc.clone() });
            }
            return Ok(out);
        }
        if let ast::EqKind::Connect(..) = eq.kind {
            return Err(LangError::new(
                "UNSUPPORTED",
                eq.span,
                "connect(…) is not part of Base Modelica (a flat model holds its connection \
                 equations)"
                    .into(),
            ));
        }
        match lower::lower_equation(eq, &cx)? {
            Lowered::Equations(list) => Ok(list),
            Lowered::Connect(_) => unreachable!("rejected above"),
        }
    }

    /// `r1 = r2` or `r1 = R(…)` as pairs of field expressions.
    fn record_sides(&self, l: &ast::Expr, r: &ast::Expr) -> Option<Vec<(ast::Expr, ast::Expr)>> {
        let fields_of = |e: &ast::Expr| -> Option<(String, Vec<ast::Expr>)> {
            match &e.strip().kind {
                ExprKind::Ref(p) => {
                    let ty = self.record_vars.get(&p.join("."))?;
                    let rec = self.records.get(ty)?;
                    let fields = rec
                        .fields
                        .iter()
                        .map(|(f, _, _)| {
                            let mut q = p.clone();
                            q.push(f.clone());
                            ast::Expr { kind: ExprKind::Ref(q), span: e.span }
                        })
                        .collect();
                    Some((ty.clone(), fields))
                }
                ExprKind::Call(n, args)
                    if self.records.contains_key(&n.join(".")) && args.named.is_empty() =>
                {
                    Some((n.join("."), args.positional.clone()))
                }
                _ => None,
            }
        };
        let (tl, fl) = fields_of(l)?;
        let (tr, fr) = fields_of(r)?;
        (tl == tr && fl.len() == fr.len()).then(|| fl.into_iter().zip(fr).collect())
    }

    /// Names, units and events, as the text format checks them.
    fn check(
        &mut self,
        m: &ClassDef,
        def: &ComponentDef,
        eq_spans: &[Span],
        init_spans: &[Span],
        decl_spans: &HashMap<String, Span>,
    ) {
        let known = Known {
            lib: None,
            connectors: BTreeMap::new(),
            types: BTreeMap::new(),
            components: BTreeMap::new(),
        };
        let r = Strict(CompResolve { def, known: &known });
        let who = format!("'{}'", def.name);
        let mut names = vec![];
        for e in m.equations.iter().chain(&m.initial_equations) {
            collect_refs(e, &mut names);
        }
        for (n, s) in names {
            if self.consts.contains_key(&n) || self.record_vars.contains_key(&n) {
                continue;
            }
            if r.name(&n) == NameDim::Missing {
                self.err(
                    "UNKNOWN-NAME",
                    s,
                    format!("'{n}' is not a variable, parameter or constant of {who}"),
                );
            }
        }
        // parameters' values and start values in their declared units
        let values = def
            .params
            .iter()
            .filter_map(|p| match &p.default {
                ParamValue::Real(e) => Some((&p.name, &p.unit, e, "the value of the parameter")),
                _ => None,
            })
            .chain(def.vars.iter().filter_map(|v| {
                v.start.as_ref().map(|e| (&v.name, &v.unit, e, "the start value of"))
            }));
        for (name, unit, e, what) in values {
            if let Ok(u) = parse_unit(unit)
                && let Err(why) = dims::expect(e, u.dim, &r)
            {
                let at = decl_spans.get(name).copied().unwrap_or(m.name_span);
                self.errs.push(LangError::new(
                    "UNIT-MISMATCH",
                    at,
                    format!("In {who}, {what} '{name}' does not have its unit ({unit}): {why}"),
                ));
            }
        }
        for (list, spans, what) in [
            (&def.equations, eq_spans, "equation"),
            (&def.initial_equations, init_spans, "initial equation"),
        ] {
            for (e, at) in list.iter().zip(spans) {
                lower::check_equation(&e.eq, *at, what, &who, def, &r, self.map, &mut self.errs);
            }
        }
    }
}

/// In a Base Modelica model every name is declared: an unknown one is an
/// error, not a library's.
struct Strict<'a>(CompResolve<'a>);

impl Resolve for Strict<'_> {
    fn name(&self, n: &str) -> NameDim {
        match self.0.name(n) {
            NameDim::Value(D::Unknown) => NameDim::Missing,
            other => other,
        }
    }
}

fn str_attr(a: &ModArg) -> Result<String, LangError> {
    match a.value.as_ref().map(|v| &v.strip().kind) {
        Some(ExprKind::Str(s)) => Ok(s.clone()),
        _ => Err(LangError::new(
            "ATTRIBUTE",
            a.span,
            format!("'{}' is text in quotes", a.name.join(".")),
        )),
    }
}

fn num_attr(a: &ModArg) -> Result<f64, LangError> {
    a.value.as_ref().and_then(lower::number).ok_or_else(|| {
        LangError::new(
            "ATTRIBUTE",
            a.span,
            format!("'{}' is a plain number here", a.name.join(".")),
        )
    })
}

fn when_targets(e: &ast::Equation, inside: bool, out: &mut BTreeSet<String>) {
    match &e.kind {
        ast::EqKind::When(branches) => {
            for (_, body) in branches {
                for b in body {
                    when_targets(b, true, out);
                }
            }
        }
        ast::EqKind::Simple(l, _) if inside => {
            if let ExprKind::Ref(p) = &l.strip().kind {
                out.insert(p.join("."));
            }
        }
        ast::EqKind::If(branches, other) => {
            for (_, body) in branches {
                body.iter().for_each(|b| when_targets(b, inside, out));
            }
            other.iter().for_each(|b| when_targets(b, inside, out));
        }
        _ => {}
    }
}

fn collect_refs(e: &ast::Equation, out: &mut Vec<(String, Span)>) {
    fn walk(x: &ast::Expr, out: &mut Vec<(String, Span)>) {
        match &x.kind {
            ExprKind::Ref(p) if !(p.len() == 1 && p[0] == "time") => {
                out.push((p.join("."), x.span))
            }
            ExprKind::Call(n, args) => {
                // an assert's message and level are not names of the model
                let keep = if n.join(".") == "assert" { 1 } else { usize::MAX };
                for (k, a) in args.positional.iter().enumerate() {
                    if k < keep {
                        walk(a, out);
                    }
                }
                for (_, _, a) in &args.named {
                    walk(a, out);
                }
            }
            ExprKind::Neg(a) | ExprKind::Not(a) | ExprKind::Paren(a) => walk(a, out),
            ExprKind::Bin(_, a, b) => {
                walk(a, out);
                walk(b, out);
            }
            ExprKind::If(br, o) => {
                for (c, v) in br {
                    walk(c, out);
                    walk(v, out);
                }
                walk(o, out);
            }
            _ => {}
        }
    }
    match &e.kind {
        ast::EqKind::Simple(a, b) | ast::EqKind::Connect(a, b) => {
            walk(a, out);
            walk(b, out);
        }
        ast::EqKind::Call(c) => walk(c, out),
        ast::EqKind::If(br, o) => {
            for (c, body) in br {
                walk(c, out);
                body.iter().for_each(|b| collect_refs(b, out));
            }
            o.iter().for_each(|b| collect_refs(b, out));
        }
        ast::EqKind::When(br) => {
            for (c, body) in br {
                walk(c, out);
                body.iter().for_each(|b| collect_refs(b, out));
            }
        }
    }
}

/// Dimensions while inferring: the known ones by name.
struct Inferred<'a>(&'a HashMap<String, Dim>);

impl Resolve for Inferred<'_> {
    fn name(&self, n: &str) -> NameDim {
        match self.0.get(n) {
            Some(d) => NameDim::Value(D::Known(*d)),
            None => NameDim::Value(D::Unknown),
        }
    }
}

fn known_dim(e: &Expr, dims: &HashMap<String, Dim>) -> Option<Dim> {
    match dims::dim(e, &Inferred(dims)) {
        Ok(D::Known(d)) => Some(d),
        _ => None,
    }
}

/// Pushes the dimension `want` down to the undeclared names `e`
/// determines.
fn push(e: &Expr, want: Dim, dims: &mut HashMap<String, Dim>, open: &mut BTreeSet<String>) {
    match e {
        Expr::Name(n) if open.contains(n) => {
            open.remove(n);
            dims.insert(n.clone(), want);
        }
        Expr::Neg(a) | Expr::NoEvent(a) => push(a, want, dims, open),
        Expr::Binary(BinaryOp::Add | BinaryOp::Sub, a, b) | Expr::If(_, a, b) => {
            push(a, want, dims, open);
            push(b, want, dims, open);
        }
        Expr::Binary(BinaryOp::Mul, a, b) => {
            if let Some(x) = known_dim(a, dims) {
                push(b, want / x, dims, open);
            } else if matches!(**a, Expr::Const(_)) {
                push(b, want, dims, open);
            }
            if let Some(y) = known_dim(b, dims) {
                push(a, want / y, dims, open);
            } else if matches!(**b, Expr::Const(_)) {
                push(a, want, dims, open);
            }
        }
        Expr::Binary(BinaryOp::Div, a, b) => {
            if let Some(y) = known_dim(b, dims) {
                push(a, want * y, dims, open);
            } else if matches!(**b, Expr::Const(_)) {
                push(a, want, dims, open);
            }
            if let Some(x) = known_dim(a, dims) {
                push(b, x / want, dims, open);
            }
        }
        Expr::Call(
            Builtin::Min | Builtin::Max | Builtin::Limit | Builtin::Abs | Builtin::Pre,
            args,
        ) => {
            for a in args {
                push(a, want, dims, open);
            }
        }
        Expr::Call(Builtin::Der, args) => {
            if let Some(a) = args.first() {
                push(a, want * Dim::TIME, dims, open);
            }
        }
        Expr::Call(Builtin::Sqrt, args) => {
            if let Some(a) = args.first() {
                push(a, want * want, dims, open);
            }
        }
        _ => {}
    }
}

/// Gives declarations without a unit the unit their equations imply,
/// where they determine it: `v = R * i` with `v` in V and `R` in Ohm
/// gives `i` A. Names nothing determines keep no unit (dimensionless).
fn infer_units(def: &mut ComponentDef, declared: &BTreeSet<String>) {
    let mut dims: HashMap<String, Dim> = HashMap::new();
    let mut open: BTreeSet<String> = BTreeSet::new();
    let mut note = |name: &str, unit: &str| {
        if declared.contains(name) {
            if let Ok(u) = parse_unit(unit) {
                dims.insert(name.to_string(), u.dim);
            }
        } else {
            open.insert(name.to_string());
        }
    };
    let mut fixed_dimensionless = vec![];
    for p in &def.params {
        if matches!(p.default, ParamValue::Real(_)) {
            note(&p.name, &p.unit);
        } else {
            fixed_dimensionless.push(p.name.clone());
        }
    }
    for v in &def.vars {
        note(&v.name, &v.unit);
    }
    for p in &def.ports {
        if let PortKind::Input { unit } | PortKind::Output { unit } = &p.kind {
            note(&p.name, unit);
        }
    }
    for n in fixed_dimensionless {
        dims.insert(n, Dim::NONE);
    }
    if open.is_empty() {
        return;
    }
    // the pairs of things that must have the same dimension
    let mut pairs: Vec<(Expr, Expr)> = vec![];
    let mut compare = |x: &Expr| {
        if let Expr::Compare(_, a, b) = x {
            pairs.push(((**a).clone(), (**b).clone()));
        }
    };
    for e in def.equations.iter().chain(&def.initial_equations) {
        match &e.eq {
            Equation::Eq { lhs, rhs } => {
                lhs.walk(&mut compare);
                rhs.walk(&mut compare);
            }
            Equation::When { condition, .. } | Equation::Assert { condition, .. } => {
                condition.walk(&mut compare)
            }
        }
    }
    for e in def.equations.iter().chain(&def.initial_equations) {
        match &e.eq {
            Equation::Eq { lhs, rhs } => pairs.push((lhs.clone(), rhs.clone())),
            Equation::When { actions, .. } => {
                for a in actions {
                    let (WhenAction::Assign { var, value } | WhenAction::Reinit { var, value }) = a;
                    pairs.push((Expr::Name(var.clone()), value.clone()));
                }
            }
            Equation::Assert { .. } => {}
        }
    }
    for p in &def.params {
        if let ParamValue::Real(e) = &p.default {
            pairs.push((Expr::Name(p.name.clone()), e.clone()));
        }
    }
    for v in &def.vars {
        if let Some(s) = &v.start {
            pairs.push((Expr::Name(v.name.clone()), s.clone()));
        }
    }
    for _ in 0..64 {
        let before = open.len();
        for (a, b) in &pairs {
            if let Some(d) = known_dim(a, &dims) {
                push(b, d, &mut dims, &mut open);
            }
            if let Some(d) = known_dim(b, &dims) {
                push(a, d, &mut dims, &mut open);
            }
        }
        if open.is_empty() || open.len() == before {
            break;
        }
    }
    let unit_of = |n: &str| dims.get(n).filter(|_| !declared.contains(n)).map(|d| unit_text(*d));
    for p in &mut def.params {
        if matches!(p.default, ParamValue::Real(_))
            && let Some(u) = unit_of(&p.name)
        {
            p.unit = u;
        }
    }
    for v in &mut def.vars {
        if let Some(u) = unit_of(&v.name) {
            v.unit = u;
        }
    }
    for p in &mut def.ports {
        if let Some(u) = unit_of(&p.name)
            && let PortKind::Input { unit } | PortKind::Output { unit } = &mut p.kind
        {
            *unit = u;
        }
    }
}
