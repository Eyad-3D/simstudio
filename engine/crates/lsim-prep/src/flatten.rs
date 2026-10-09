//! Flattening: a component tree becomes one flat system.
//!
//! Every instance's parameters, port variables and own variables get flat
//! records named by their dotted path; names in equations are resolved;
//! `connect`s become connection sets whose across quantities are equal and
//! whose through quantities sum to zero (Modelica's rule: ports seen from
//! inside a composite enter the sum negated); a physical port with nothing
//! connected carries no flow; a signal input takes its one driving output.
//!
//! Parameters stay symbolic in the equations (runtime inputs); their values
//! are evaluated here, in SI, and a parameter bound to others keeps that
//! binding so the run can re-evaluate it. Parameters are laid out in
//! binding order — a parent's before its children's, and within a
//! component each after the ones its default refers to — so one pass in
//! order re-evaluates every binding; a default that refers back to itself
//! through others is an error. Start values stay expressions of the
//! parameters too ([`Extras::start`]), for the initialisation system.
//!
//! A component whose definition's name begins with `External.` is a
//! sampled external block (a Script block, an FMU for co-simulation, a
//! digital controller: DESIGN.md, *Causal blocks*): it has no equations,
//! its signal outputs are discrete variables the host sets at each tick,
//! its signal inputs are read at each tick, and its parameter `period` is
//! the tick spacing in seconds.

use crate::walk::FxMap;
use lsim_ir::component::{
    ComponentDef, Equation, Library, ParamValue, PortKind, TableData, WhenAction, split_enum_value,
};
use lsim_ir::eval::{Env, eval};
use lsim_ir::expr::{Builtin, Expr};
use lsim_ir::flat::*;
use lsim_ir::units::{Unit, parse_unit};
use lsim_ir::{Diagnostic, PowerRule, VarKind};

#[derive(Clone, Copy, Debug)]
enum Sym {
    Var(VarId),
    Param(ParamId),
}

/// A definition's own names, shared by every instance of it: each
/// variable by the order the instance creates its variables in, each
/// parameter by its declaration index. (Building a map of names per
/// instance cost most of flattening's allocations.)
#[derive(Default)]
struct Template {
    syms: FxMap<String, Local>,
}

#[derive(Clone, Copy, Debug)]
enum Local {
    /// the instance's n-th variable
    Var(u32),
    /// the k-th declared parameter
    Param(u32),
}

/// An instance's names: its definition's template, where its variables
/// start, its parameters, and its sub-components.
struct Scope {
    template: usize,
    var_base: u32,
    /// how many of its variables exist yet (names of later ones are not
    /// in scope yet, as when they were added one by one)
    n_vars: u32,
    /// by declaration index (`NO_PARAM` until created)
    params: Vec<ParamId>,
    subs: FxMap<String, InstanceId>,
}

const NO_PARAM: ParamId = ParamId(u32::MAX);

/// A port as an end of connections: `outside` is a composite's own port
/// seen from inside it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct Node {
    inst: InstanceId,
    port: usize,
    outside: bool,
}

#[derive(Clone, Copy, Debug)]
enum PortVars {
    Physical { across: VarId, through: VarId },
    Signal { var: VarId, output: bool },
}

/// What flattening knows beyond the flat system.
#[derive(Clone, Debug, Default)]
pub struct Extras {
    /// per parameter: its declared lowest and highest value
    pub param_range: Vec<(Option<f64>, Option<f64>)>,
    /// per variable: its start value as an expression of the parameters
    pub start: Vec<Option<Expr>>,
    /// the sampled external blocks, in the order they were found
    pub external: Vec<lsim_ir::ExternalBlock>,
}

struct Flattener<'a> {
    lib: &'a Library,
    /// unit texts already parsed (SI ones only)
    units: FxMap<String, Unit>,
    flat: FlatSystem,
    extras: Extras,
    scopes: Vec<Scope>,
    defs: Vec<&'a ComponentDef>,
    ports: FxMap<(InstanceId, usize), PortVars>,
    nodes: FxMap<Node, usize>,
    node_list: Vec<Node>,
    parent: Vec<usize>,
    node_scope: Vec<InstanceId>,
    diags: Vec<Diagnostic>,
    /// table parameters: their index in `flat.tables`
    table_of: FxMap<ParamId, u32>,
    /// each definition's names (by the definition's address)
    templates: Vec<Template>,
    template_of: FxMap<usize, usize>,
    /// parameter creation orders, by definition and the set of parameters
    /// given (when no cycle was found)
    orders: FxMap<(usize, u64), std::rc::Rc<[usize]>>,
}

struct ParamEnv<'a>(&'a FlatSystem);

impl Env for ParamEnv<'_> {
    fn time(&self) -> f64 {
        f64::NAN
    }
    fn var(&self, _: VarId) -> f64 {
        f64::NAN
    }
    fn der(&self, _: VarId) -> f64 {
        f64::NAN
    }
    fn param(&self, p: ParamId) -> f64 {
        self.0.params[p.0 as usize].value
    }
}

/// The sum of `terms` as a balanced tree, so a connection set of thousands
/// of ports does not make an expression thousands of levels deep.
fn balanced_sum(mut terms: Vec<Expr>) -> Expr {
    while terms.len() > 1 {
        let mut next = Vec::with_capacity(terms.len().div_ceil(2));
        let mut it = terms.into_iter();
        while let Some(a) = it.next() {
            next.push(match it.next() {
                Some(b) => a + b,
                None => a,
            });
        }
        terms = next;
    }
    terms.pop().expect("a set has members")
}

fn join(path: &str, name: &str) -> String {
    if path.is_empty() {
        return name.to_string();
    }
    let mut s = String::with_capacity(path.len() + 1 + name.len());
    s.push_str(path);
    s.push('.');
    s.push_str(name);
    s
}

/// `path.a.b` in one allocation.
fn join2(path: &str, a: &str, b: &str) -> String {
    let mut s = String::with_capacity(path.len() + a.len() + b.len() + 2);
    if !path.is_empty() {
        s.push_str(path);
        s.push('.');
    }
    s.push_str(a);
    s.push('.');
    s.push_str(b);
    s
}

/// Flattens `top` (a model or any component) against `lib`.
pub fn flatten(lib: &Library, top: &ComponentDef) -> Result<FlatSystem, Vec<Diagnostic>> {
    flatten_full(lib, top).map(|(f, _)| f)
}

/// [`flatten`], with what the later steps need besides the flat system.
pub fn flatten_full(
    lib: &Library,
    top: &ComponentDef,
) -> Result<(FlatSystem, Extras), Vec<Diagnostic>> {
    let mut f = Flattener {
        lib,
        units: FxMap::default(),
        flat: FlatSystem::default(),
        extras: Extras::default(),
        scopes: vec![],
        defs: vec![],
        ports: FxMap::default(),
        nodes: FxMap::default(),
        node_list: vec![],
        parent: vec![],
        node_scope: vec![],
        diags: vec![],
        table_of: FxMap::default(),
        templates: vec![],
        template_of: FxMap::default(),
        orders: FxMap::default(),
    };
    f.instantiate(top, String::new(), None, None, None, &FxMap::default());
    f.connection_equations();
    if f.diags.is_empty() { Ok((f.flat, f.extras)) } else { Err(f.diags) }
}

/// A parameter value handed down by a modifier: SI value and binding,
/// or a table.
#[derive(Clone)]
struct Given {
    value: f64,
    binding: Option<Expr>,
    structural: bool,
    table: Option<TableSource>,
}

/// Where a table parameter's data come from.
#[derive(Clone)]
enum TableSource {
    /// data given here
    Data(TableData),
    /// the same table as an enclosing parameter (`ocv = ocv_cell`)
    Shared(u32),
}

impl<'a> Flattener<'a> {
    fn describe(&self, inst: InstanceId) -> String {
        let def = &self.flat.instance(inst).def;
        format!("{} ({def})", self.flat.instance_name(inst))
    }

    fn si_unit(&mut self, inst: InstanceId, what: &dyn Fn() -> String, text: &str) -> Unit {
        if let Some(u) = self.units.get(text) {
            return *u;
        }
        match parse_unit(text) {
            Ok(u) if u.scale == 1.0 && u.offset == 0.0 => {
                self.units.insert(text.to_string(), u);
                u
            }
            Ok(u) => {
                let who = self.describe(inst);
                let what = what();
                self.diags.push(
                    Diagnostic::error(
                        "UNIT-NOT-SI",
                        format!(
                            "In {who}, {what} is declared in '{text}', which is not an SI unit."
                        ),
                    )
                    .with_hint(
                        "Declare it in the SI unit and give the other one as its display unit.",
                    ),
                );
                Unit::new(u.dim, 1.0)
            }
            Err(e) => {
                let who = self.describe(inst);
                let what = what();
                self.diags
                    .push(Diagnostic::error("UNIT-SYNTAX", format!("In {who}, {what}: {e}.")));
                Unit::ONE
            }
        }
    }

    fn lookup(&self, inst: InstanceId, name: &str) -> Option<Sym> {
        let scope = &self.scopes[inst.0 as usize];
        match self.templates[scope.template].syms.get(name) {
            Some(Local::Var(n)) if *n < scope.n_vars => {
                return Some(Sym::Var(VarId(scope.var_base + n)));
            }
            Some(Local::Param(k)) if scope.params[*k as usize] != NO_PARAM => {
                return Some(Sym::Param(scope.params[*k as usize]));
            }
            _ => {}
        }
        let (head, rest) = name.split_once('.')?;
        let sub = scope.subs.get(head)?;
        self.lookup(*sub, rest)
    }

    /// The number an enumeration option stands for in `inst`'s scope.
    fn enum_option(&self, inst: InstanceId, qualified: &str) -> Option<f64> {
        let def = self.defs[inst.0 as usize];
        self.lib.enum_ordinal(def, qualified).map(|k| k as f64)
    }

    /// The tree of [`Flattener::resolve`], rebuilt from references (a name
    /// is looked up, never cloned): `der(x)` and `pre(x)` of a variable
    /// become `Der`/`Pre`, a table read by its parameter its flat table.
    fn resolve_in(
        &self,
        inst: InstanceId,
        e: &Expr,
        missing: &mut Vec<String>,
        not_tables: &mut Vec<String>,
    ) -> Expr {
        let b = Box::new;
        let mut r = |x: &Expr| self.resolve_in(inst, x, missing, not_tables);
        match e {
            Expr::Name(n) => match self.lookup(inst, n) {
                Some(Sym::Var(v)) => Expr::Var(v),
                Some(Sym::Param(p)) => Expr::Param(p),
                None => match self.enum_option(inst, n) {
                    Some(k) => Expr::Const(k),
                    None => {
                        missing.push(n.clone());
                        Expr::Const(f64::NAN)
                    }
                },
            },
            Expr::Neg(a) => Expr::Neg(b(r(a))),
            Expr::Not(a) => Expr::Not(b(r(a))),
            Expr::NoEvent(a) => Expr::NoEvent(b(r(a))),
            Expr::Binary(op, x, y) => Expr::Binary(*op, b(r(x)), b(r(y))),
            Expr::Compare(op, x, y) => Expr::Compare(*op, b(r(x)), b(r(y))),
            Expr::And(x, y) => Expr::And(b(r(x)), b(r(y))),
            Expr::Or(x, y) => Expr::Or(b(r(x)), b(r(y))),
            Expr::If(c, x, y) => Expr::If(b(r(c)), b(r(x)), b(r(y))),
            Expr::Call(g @ (Builtin::Der | Builtin::Pre), args) if args.len() == 1 => {
                match (g, r(&args[0])) {
                    (Builtin::Der, Expr::Var(v)) => Expr::Der(v),
                    (Builtin::Pre, Expr::Var(v)) => Expr::Pre(v),
                    (_, a) => Expr::Call(*g, vec![a]),
                }
            }
            Expr::Call(g, args) => Expr::Call(*g, args.iter().map(&mut r).collect()),
            Expr::Table { table, args } => {
                let args: Vec<Expr> = args.iter().map(&mut r).collect();
                // component scope names the table first; flat scope indexes it
                match args.first() {
                    Some(Expr::Param(p)) => match self.table_of.get(p) {
                        Some(&k) => Expr::Table { table: k, args: args[1..].to_vec() },
                        None => {
                            not_tables.push(self.flat.params[p.0 as usize].name.clone());
                            Expr::Const(f64::NAN)
                        }
                    },
                    _ => Expr::Table { table: *table, args },
                }
            }
            leaf => leaf.clone(),
        }
    }

    /// Resolves names in `e` (component scope of `inst`) to flat references.
    fn resolve(&mut self, inst: InstanceId, e: &Expr, what: &dyn Fn() -> String) -> Expr {
        let mut missing: Vec<String> = vec![];
        let mut not_tables: Vec<String> = vec![];
        let out = self.resolve_in(inst, e, &mut missing, &mut not_tables);
        for n in missing {
            let who = self.describe(inst);
            let what = what();
            self.diags.push(Diagnostic::error(
                "UNKNOWN-NAME",
                format!(
                    "In {who}, {what} uses '{n}', which is not one of its variables, ports or \
                     parameters."
                ),
            ));
        }
        for n in not_tables {
            let who = self.describe(inst);
            let what = what();
            self.diags.push(Diagnostic::error(
                "NOT-A-TABLE",
                format!("In {who}, {what} reads '{n}' as a table, but it is a number."),
            ));
        }
        let mut as_number = None;
        if !self.table_of.is_empty() {
            crate::walk::visit(&out, &mut |x| {
                if let Expr::Param(p) = x
                    && self.table_of.contains_key(p)
                {
                    as_number.get_or_insert(*p);
                }
            });
        }
        if let Some(p) = as_number {
            let who = self.describe(inst);
            let what = what();
            let name = &self.flat.params[p.0 as usize].name;
            self.diags.push(
                Diagnostic::error(
                    "TABLE-AS-NUMBER",
                    format!("In {who}, {what} uses the table '{name}' as a number."),
                )
                .with_hint("Read a table at a point, as ocv(soc)."),
            );
        }
        out
    }

    fn new_var(&mut self, rec: FlatVar) -> VarId {
        self.flat.vars.push(rec);
        self.extras.start.push(None);
        VarId(self.flat.vars.len() as u32 - 1)
    }

    /// Puts an instance's newly created variable in its scope under `name`
    /// (built only for the definition's first instance).
    fn own_var(&mut self, id: InstanceId, fresh: bool, name: impl FnOnce() -> String) {
        let scope = &mut self.scopes[id.0 as usize];
        let n = scope.n_vars;
        scope.n_vars += 1;
        if fresh {
            let t = scope.template;
            self.templates[t].syms.insert(name(), Local::Var(n));
        }
    }

    /// The order to create a component's parameters in: each after the
    /// parameters of the same component its default refers to.
    fn param_order(
        &mut self,
        id: InstanceId,
        def: &ComponentDef,
        given: &FxMap<String, Given>,
    ) -> std::rc::Rc<[usize]> {
        let n = def.params.len();
        let key = (n <= 64).then(|| {
            let mask = def
                .params
                .iter()
                .enumerate()
                .filter(|(_, p)| given.contains_key(&p.name))
                .fold(0u64, |m, (k, _)| m | (1 << k));
            (def as *const ComponentDef as usize, mask)
        });
        if let Some(o) = key.and_then(|k| self.orders.get(&k)) {
            return o.clone();
        }
        let before = self.diags.len();
        let order: std::rc::Rc<[usize]> = self.param_order_of(id, def, given).into();
        if let Some(k) = key
            && self.diags.len() == before
        {
            self.orders.insert(k, order.clone());
        }
        order
    }

    fn param_order_of(
        &mut self,
        id: InstanceId,
        def: &ComponentDef,
        given: &FxMap<String, Given>,
    ) -> Vec<usize> {
        let n = def.params.len();
        let index: FxMap<&str, usize> =
            def.params.iter().enumerate().map(|(k, p)| (p.name.as_str(), k)).collect();
        let mut deps: Vec<Vec<usize>> = vec![vec![]; n];
        for (k, p) in def.params.iter().enumerate() {
            if given.contains_key(&p.name) {
                continue;
            }
            if let ParamValue::Real(e) = &p.default {
                e.walk(&mut |x| {
                    if let Expr::Name(nm) = x
                        && let Some(&j) = index.get(nm.as_str())
                        && j != k
                    {
                        deps[k].push(j);
                    }
                    if let Expr::Name(nm) = x
                        && nm == &p.name
                    {
                        deps[k].push(k);
                    }
                });
            }
        }
        let mut done = vec![false; n];
        let mut order = Vec::with_capacity(n);
        while order.len() < n {
            let next = (0..n).find(|&k| !done[k] && deps[k].iter().all(|&j| done[j] && j != k));
            match next {
                Some(k) => {
                    done[k] = true;
                    order.push(k);
                }
                None => {
                    let stuck: Vec<String> = (0..n)
                        .filter(|&k| !done[k])
                        .map(|k| format!("'{}'", def.params[k].name))
                        .collect();
                    let who = self.describe(id);
                    let mut d = Diagnostic::error(
                        "PARAM-CYCLE",
                        format!(
                            "In {who}, the parameters {} are each given by the others: none of \
                             them has a value to start from.",
                            stuck.join(", ")
                        ),
                    )
                    .with_hint("Give one of them a number.");
                    d.parts.push(self.flat.instance(id).path.clone());
                    self.diags.push(d);
                    order.extend((0..n).filter(|&k| !done[k]));
                    break;
                }
            }
        }
        order
    }

    fn instantiate(
        &mut self,
        def: &'a ComponentDef,
        path: String,
        parent: Option<InstanceId>,
        label: Option<String>,
        ui_id: Option<String>,
        given: &FxMap<String, Given>,
    ) -> InstanceId {
        let id = InstanceId(self.flat.instances.len() as u32);
        self.flat.instances.push(Instance {
            path: path.clone(),
            def: def.name.clone(),
            parent,
            label,
            ui_id,
        });
        let def_key = def as *const ComponentDef as usize;
        let (template, fresh) = match self.template_of.get(&def_key) {
            Some(&t) => (t, false),
            None => {
                self.templates.push(Template::default());
                self.template_of.insert(def_key, self.templates.len() - 1);
                (self.templates.len() - 1, true)
            }
        };
        self.scopes.push(Scope {
            template,
            var_base: self.flat.vars.len() as u32,
            n_vars: 0,
            params: vec![NO_PARAM; def.params.len()],
            subs: FxMap::default(),
        });
        self.defs.push(def);

        // parameters, in binding order
        let order = self.param_order(id, def, given);
        for &k in order.iter() {
            let p = &def.params[k];
            let unit = self.si_unit(id, &|| format!("parameter '{}'", p.name), &p.unit);
            let is_table = p.default.table().is_some();
            let g = match given.get(&p.name) {
                Some(g) => {
                    let mut g = g.clone();
                    g.structural |= p.structural;
                    if is_table != g.table.is_some() {
                        let who = self.describe(id);
                        let want = if is_table { "a table" } else { "a number" };
                        self.diags.push(Diagnostic::error(
                            "PARAM-KIND",
                            format!("{who} is given a value for '{}' that is not {want}.", p.name),
                        ));
                    }
                    g
                }
                None => self.param_value(
                    id,
                    def,
                    &p.default,
                    &|| format!("the default of '{}'", p.name),
                    p.structural,
                ),
            };
            let name = join(&path, &p.name);
            let pid = ParamId(self.flat.params.len() as u32);
            let mut value = g.value;
            if let Some(src) = g.table {
                let k = match src {
                    TableSource::Shared(k) => k,
                    TableSource::Data(data) => {
                        if let Err(why) = data.check() {
                            let who = self.describe(id);
                            self.diags.push(Diagnostic::error(
                                "TABLE-DATA",
                                format!("In {who}, the table '{}' is not valid: {why}.", p.name),
                            ));
                        }
                        self.flat.tables.push(FlatTable {
                            name: name.clone(),
                            instance: id,
                            param: Some(pid),
                            unit,
                            data,
                        });
                        self.flat.tables.len() as u32 - 1
                    }
                };
                self.table_of.insert(pid, k);
                value = k as f64;
            }
            self.flat.params.push(FlatParam {
                name,
                unit,
                value,
                binding: g.binding,
                structural: g.structural,
                instance: id,
            });
            self.extras.param_range.push((p.min, p.max));
            self.scopes[id.0 as usize].params[k] = pid;
            if fresh {
                self.templates[template].syms.insert(p.name.clone(), Local::Param(k as u32));
            }
        }
        for m in given.keys() {
            if !def.params.iter().any(|p| &p.name == m) {
                let who = self.describe(id);
                self.diags.push(Diagnostic::error(
                    "UNKNOWN-PARAMETER",
                    format!("{who} has no parameter '{m}'."),
                ));
            }
        }

        // ports
        let external = def.name.starts_with("External.");
        for (k, port) in def.ports.iter().enumerate() {
            match &port.kind {
                PortKind::Physical { connector } => {
                    let Some(cdef) = self.lib.connectors.get(connector) else {
                        let who = self.describe(id);
                        self.diags.push(Diagnostic::error(
                            "UNKNOWN-CONNECTOR",
                            format!(
                                "In {who}, port '{}' is of an unknown type '{connector}'.",
                                port.name
                            ),
                        ));
                        continue;
                    };
                    let mut ids = [VarId(0); 2];
                    for (j, (q, role)) in [
                        (&cdef.across, VarRole::Across { port: port.name.clone() }),
                        (&cdef.through, VarRole::Through { port: port.name.clone() }),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        let unit = self.si_unit(id, &|| format!("port '{}'", port.name), &q.unit);
                        ids[j] = self.new_var(FlatVar {
                            name: join2(&path, &port.name, &q.name),
                            unit,
                            unit_text: q.unit.clone(),
                            kind: VarKind::Continuous,
                            start: None,
                            fixed: false,
                            nominal: 1.0,
                            instance: id,
                            role,
                        });
                        self.own_var(id, fresh, || join(&port.name, &q.name));
                    }
                    self.ports
                        .insert((id, k), PortVars::Physical { across: ids[0], through: ids[1] });
                    if def.components.is_empty() {
                        let power = match cdef.power {
                            PowerRule::AcrossTimesThrough => Expr::Var(ids[0]) * Expr::Var(ids[1]),
                            PowerRule::ThroughIsPower => Expr::Var(ids[1]),
                        };
                        self.flat.port_powers.push(PortPower {
                            instance: id,
                            port: port.name.clone(),
                            power,
                        });
                    }
                }
                PortKind::Input { unit } | PortKind::Output { unit } => {
                    let output = matches!(port.kind, PortKind::Output { .. });
                    let u = self.si_unit(id, &|| format!("port '{}'", port.name), unit);
                    let held = output && external;
                    let v = self.new_var(FlatVar {
                        name: join(&path, &port.name),
                        unit: u,
                        unit_text: unit.clone(),
                        kind: if held { VarKind::Discrete } else { VarKind::Continuous },
                        start: held.then_some(0.0),
                        fixed: false,
                        nominal: 1.0,
                        instance: id,
                        role: if output { VarRole::Output } else { VarRole::Input },
                    });
                    self.own_var(id, fresh, || port.name.clone());
                    self.ports.insert((id, k), PortVars::Signal { var: v, output });
                }
            }
        }

        // own variables
        for v in &def.vars {
            let unit = self.si_unit(id, &|| format!("variable '{}'", v.name), &v.unit);
            let start_expr = v
                .start
                .as_ref()
                .map(|s| self.resolve(id, s, &|| format!("the start value of '{}'", v.name)));
            let start = start_expr.as_ref().map(|r| eval(r, &ParamEnv(&self.flat)));
            let vid = self.new_var(FlatVar {
                name: join(&path, &v.name),
                unit,
                unit_text: v.unit.clone(),
                kind: v.kind,
                start,
                fixed: v.fixed,
                nominal: v.nominal.unwrap_or(1.0),
                instance: id,
                role: VarRole::Local,
            });
            self.extras.start[vid.0 as usize] = start_expr;
            self.own_var(id, fresh, || v.name.clone());
        }

        if external {
            self.external_block(id, def);
        }

        // sub-components
        for s in &def.components {
            let Some(sdef) = self.lib.components.get(&s.def) else {
                let who = self.describe(id);
                self.diags.push(Diagnostic::error(
                    "UNKNOWN-COMPONENT",
                    format!("{who} uses '{}', which is not in the library.", s.def),
                ));
                continue;
            };
            let mut sub_given = FxMap::default();
            for m in &s.modifiers {
                let g = self.param_value(
                    id,
                    sdef,
                    &m.value,
                    &|| format!("the value given to {}.{}", s.name, m.param),
                    false,
                );
                sub_given.insert(m.param.clone(), g);
            }
            let child = self.instantiate(
                sdef,
                join(&path, &s.name),
                Some(id),
                s.label.clone(),
                s.ui_id.clone(),
                &sub_given,
            );
            self.scopes[id.0 as usize].subs.insert(s.name.clone(), child);
        }

        // equations
        for (index, e) in def.equations.iter().enumerate() {
            let origin = Origin {
                instance: id,
                kind: OriginKind::Component { index },
                label: e.label.clone(),
            };
            let what = || format!("equation {}", index + 1);
            match &e.eq {
                Equation::Eq { lhs, rhs } => {
                    let lhs = self.resolve(id, lhs, &what);
                    let rhs = self.resolve(id, rhs, &what);
                    self.flat.equations.push(FlatEquation { lhs, rhs, origin });
                }
                Equation::When { condition, actions } => {
                    let condition = self.resolve(id, condition, &what);
                    let mut assign = vec![];
                    let mut reinit = vec![];
                    for a in actions {
                        let (var, value, list) = match a {
                            WhenAction::Assign { var, value } => (var, value, &mut assign),
                            WhenAction::Reinit { var, value } => (var, value, &mut reinit),
                        };
                        let value = self.resolve(id, value, &what);
                        match self.lookup(id, var) {
                            Some(Sym::Var(v)) => list.push((v, value)),
                            _ => {
                                let who = self.describe(id);
                                self.diags.push(Diagnostic::error(
                                    "UNKNOWN-NAME",
                                    format!("In {who}, {} assigns '{var}', which is not one of its variables.", what()),
                                ));
                            }
                        }
                    }
                    self.flat.whens.push(FlatWhen { condition, assign, reinit, origin });
                }
                Equation::Assert { condition, message, error } => {
                    let condition = self.resolve(id, condition, &what);
                    self.flat.asserts.push(FlatAssert {
                        condition,
                        message: message.clone(),
                        error: *error,
                        origin,
                    });
                }
            }
        }
        for (index, e) in def.initial_equations.iter().enumerate() {
            if let Equation::Eq { lhs, rhs } = &e.eq {
                let what = || format!("initial equation {}", index + 1);
                let lhs = self.resolve(id, lhs, &what);
                let rhs = self.resolve(id, rhs, &what);
                let origin = Origin {
                    instance: id,
                    kind: OriginKind::Component { index },
                    label: e.label.clone(),
                };
                self.flat.initial_equations.push(FlatEquation { lhs, rhs, origin });
            }
        }

        // energy books
        if def.energy.stored.is_some() || def.energy.loss.is_some() {
            let stored = def
                .energy
                .stored
                .as_ref()
                .map(|x| self.resolve(id, x, &|| "its stored energy".into()));
            let loss = def.energy.loss.as_ref().map(|x| self.resolve(id, x, &|| "its loss".into()));
            self.flat.energy.push(InstanceEnergy { instance: id, stored, loss });
        }

        // connections
        for cn in &def.connections {
            let a = self.port_node(id, &cn.a);
            let b = self.port_node(id, &cn.b);
            if let (Some(a), Some(b)) = (a, b)
                && self.compatible(id, a, b, &cn.a, &cn.b)
            {
                let ia = self.node_index(a, id);
                let ib = self.node_index(b, id);
                self.union(ia, ib);
            }
        }
        id
    }

    fn external_block(&mut self, id: InstanceId, def: &ComponentDef) {
        let mut inputs = vec![];
        let mut outputs = vec![];
        for (k, port) in def.ports.iter().enumerate() {
            match self.ports.get(&(id, k)) {
                Some(PortVars::Signal { var, output: false }) => inputs.push(*var),
                Some(PortVars::Signal { var, output: true }) => outputs.push(*var),
                _ => {
                    let who = self.describe(id);
                    self.diags.push(Diagnostic::error(
                        "EXTERNAL-PORT",
                        format!(
                            "{who} is a sampled block, so its port '{}' must be a signal.",
                            port.name
                        ),
                    ));
                }
            }
        }
        let period = match self.lookup(id, "period") {
            Some(Sym::Param(p)) => self.flat.params[p.0 as usize].value,
            _ => f64::NAN,
        };
        if period.is_nan() || period <= 0.0 {
            let who = self.describe(id);
            let mut d = Diagnostic::error(
                "EXTERNAL-PERIOD",
                format!("{who} is a sampled block but has no positive parameter 'period'."),
            )
            .with_hint("Give it a sample period in seconds.");
            d.parts.push(self.flat.instance(id).path.clone());
            self.diags.push(d);
        }
        if !def.equations.is_empty() {
            let who = self.describe(id);
            self.diags.push(Diagnostic::error(
                "EXTERNAL-EQUATIONS",
                format!("{who} is a sampled block: its outputs come from the host, not equations."),
            ));
        }
        self.extras.external.push(lsim_ir::ExternalBlock { instance: id, inputs, outputs, period });
    }

    /// A parameter value: `scope` is where its expression's names live,
    /// `owner` the definition that declares the parameter (its
    /// enumeration types give an option its number).
    fn param_value(
        &mut self,
        scope: InstanceId,
        owner: &ComponentDef,
        v: &ParamValue,
        what: &dyn Fn() -> String,
        structural: bool,
    ) -> Given {
        let num = |value: f64, binding: Option<Expr>, structural: bool| Given {
            value,
            binding,
            structural,
            table: None,
        };
        match v {
            ParamValue::Real(e) => {
                // a table parameter of the enclosing scope, handed down
                if let Expr::Name(n) = e
                    && let Some(Sym::Param(p)) = self.lookup(scope, n)
                    && let Some(&k) = self.table_of.get(&p)
                {
                    return Given {
                        table: Some(TableSource::Shared(k)),
                        ..num(k as f64, None, false)
                    };
                }
                let r = self.resolve(scope, e, what);
                let value = eval(&r, &ParamEnv(&self.flat));
                let bound = r.any(&mut |x| matches!(x, Expr::Param(_)));
                num(value, bound.then_some(r), structural)
            }
            ParamValue::Bool(b) => num(if *b { 1.0 } else { 0.0 }, None, true),
            ParamValue::Enum(q) => match self.lib.enum_ordinal(owner, q) {
                Some(k) => num(k as f64, None, true),
                None => {
                    let who = self.describe(scope);
                    let options = split_enum_value(q)
                        .and_then(|(t, _)| self.lib.enum_type(owner, t))
                        .map(|t| {
                            let names: Vec<&str> =
                                t.literals.iter().map(|l| l.name.as_str()).collect();
                            format!(" (its options: {})", names.join(", "))
                        })
                        .unwrap_or_else(|| " (its type is not known)".into());
                    self.diags.push(Diagnostic::error(
                        "ENUM-OPTION",
                        format!(
                            "In {who}, {} is '{q}', which is not an option of its type{options}.",
                            what()
                        ),
                    ));
                    num(f64::NAN, None, true)
                }
            },
            other => {
                let data = other.table().expect("the remaining values are tables");
                Given { table: Some(TableSource::Data(data)), ..num(f64::NAN, None, false) }
            }
        }
    }

    fn port_node(&mut self, scope: InstanceId, text: &str) -> Option<Node> {
        let def = self.defs[scope.0 as usize];
        if let Some(k) = def.ports.iter().position(|p| p.name == text) {
            return Some(Node { inst: scope, port: k, outside: true });
        }
        if let Some((head, rest)) = text.split_once('.')
            && let Some(&sub) = self.scopes[scope.0 as usize].subs.get(head)
        {
            let sdef = self.defs[sub.0 as usize];
            if let Some(k) = sdef.ports.iter().position(|p| p.name == rest) {
                return Some(Node { inst: sub, port: k, outside: false });
            }
        }
        let who = self.describe(scope);
        self.diags.push(Diagnostic::error(
            "UNKNOWN-PORT",
            format!("In {who}, a connection names '{text}', which is not a port."),
        ));
        None
    }

    fn compatible(&mut self, scope: InstanceId, a: Node, b: Node, ta: &str, tb: &str) -> bool {
        let ka = &self.defs[a.inst.0 as usize].ports[a.port].kind;
        let kb = &self.defs[b.inst.0 as usize].ports[b.port].kind;
        let ok = match (ka, kb) {
            (PortKind::Physical { connector: x }, PortKind::Physical { connector: y }) => x == y,
            (PortKind::Physical { .. }, _) | (_, PortKind::Physical { .. }) => false,
            (x, y) => {
                let unit = |k: &PortKind| match k {
                    PortKind::Input { unit } | PortKind::Output { unit } => {
                        parse_unit(unit).map(|u| u.dim).ok()
                    }
                    _ => None,
                };
                unit(x) == unit(y)
            }
        };
        if !ok {
            let who = self.describe(scope);
            self.diags.push(Diagnostic::error(
                "CONNECT-MISMATCH",
                format!("In {who}, '{ta}' and '{tb}' cannot be connected: they carry different quantities."),
            ));
        }
        ok
    }

    fn node_index(&mut self, n: Node, scope: InstanceId) -> usize {
        if let Some(&i) = self.nodes.get(&n) {
            return i;
        }
        let i = self.node_list.len();
        self.nodes.insert(n, i);
        self.node_list.push(n);
        self.parent.push(i);
        self.node_scope.push(scope);
        i
    }

    fn find(&mut self, mut i: usize) -> usize {
        while self.parent[i] != i {
            self.parent[i] = self.parent[self.parent[i]];
            i = self.parent[i];
        }
        i
    }

    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.parent[rb] = ra;
        }
    }

    fn node_name(&self, n: Node) -> String {
        let port = &self.defs[n.inst.0 as usize].ports[n.port].name;
        join(&self.flat.instance(n.inst).path, port)
    }

    fn connection_equations(&mut self) {
        let mut sets: FxMap<usize, Vec<usize>> = FxMap::default();
        for i in 0..self.node_list.len() {
            let r = self.find(i);
            sets.entry(r).or_default().push(i);
        }
        let mut roots: Vec<usize> = sets.keys().copied().collect();
        roots.sort_unstable();
        for r in roots {
            let members: Vec<Node> = sets[&r].iter().map(|&i| self.node_list[i]).collect();
            let scope = self.node_scope[sets[&r][0]];
            let names: Vec<String> = members.iter().map(|&n| self.node_name(n)).collect();
            let vars: Vec<PortVars> =
                members.iter().filter_map(|n| self.ports.get(&(n.inst, n.port)).copied()).collect();
            if vars.len() != members.len() {
                continue; // a port that failed to instantiate; already reported
            }
            match vars[0] {
                PortVars::Physical { .. } => {
                    let mut terms: Vec<Expr> = Vec::with_capacity(members.len());
                    let first_across = match vars[0] {
                        PortVars::Physical { across, .. } => across,
                        _ => unreachable!(),
                    };
                    for (k, (n, pv)) in members.iter().zip(&vars).enumerate() {
                        let PortVars::Physical { across, through } = *pv else { continue };
                        if k > 0 {
                            // each across equation names the two ports it joins
                            let ports = vec![names[0].clone(), names[k].clone()];
                            self.flat.equations.push(FlatEquation {
                                lhs: Expr::Var(first_across),
                                rhs: Expr::Var(across),
                                origin: Origin {
                                    instance: scope,
                                    kind: OriginKind::ConnectionAcross { ports },
                                    label: None,
                                },
                            });
                        }
                        terms.push(if n.outside {
                            -Expr::Var(through)
                        } else {
                            Expr::Var(through)
                        });
                    }
                    self.flat.equations.push(FlatEquation {
                        lhs: Expr::Const(0.0),
                        rhs: balanced_sum(terms),
                        origin: Origin {
                            instance: scope,
                            kind: OriginKind::ConnectionThrough { ports: names.clone() },
                            label: None,
                        },
                    });
                }
                PortVars::Signal { .. } => {
                    let mut sources = vec![];
                    let mut sinks = vec![];
                    for (n, pv) in members.iter().zip(&vars) {
                        let PortVars::Signal { var, output } = *pv else { continue };
                        // an output seen from outside, or a composite's input
                        // seen from inside, drives the set
                        if output != n.outside {
                            sources.push((var, self.node_name(*n)));
                        } else {
                            sinks.push((var, self.node_name(*n)));
                        }
                    }
                    if sources.len() != 1 {
                        let who = self.describe(scope);
                        let mut d = Diagnostic::error(
                            "SIGNAL-SOURCES",
                            format!(
                                "In {who}, the signal link joining {} has {} outputs driving it; it needs exactly one.",
                                names.join(", "),
                                sources.len()
                            ),
                        );
                        let mut parts: Vec<String> = members
                            .iter()
                            .filter(|n| n.inst.0 != 0)
                            .map(|n| self.flat.instance(self.flat.top_part(n.inst)).path.clone())
                            .collect();
                        parts.sort();
                        parts.dedup();
                        d.parts = parts;
                        self.diags.push(d);
                        continue;
                    }
                    let (src, src_name) = sources[0].clone();
                    for (sink, sink_name) in sinks {
                        self.flat.equations.push(FlatEquation {
                            lhs: Expr::Var(sink),
                            rhs: Expr::Var(src),
                            origin: Origin {
                                instance: scope,
                                kind: OriginKind::SignalLink {
                                    input: sink_name,
                                    output: src_name.clone(),
                                },
                                label: None,
                            },
                        });
                    }
                }
            }
        }

        // ports with nothing connected
        let mut keys: Vec<(InstanceId, usize)> = self.ports.keys().copied().collect();
        keys.sort_unstable();
        for (inst, k) in keys {
            let pv = self.ports[&(inst, k)];
            let has_parent = self.flat.instance(inst).parent.is_some();
            let composite = !self.defs[inst.0 as usize].components.is_empty();
            let inside_free =
                has_parent && !self.nodes.contains_key(&Node { inst, port: k, outside: false });
            let outside_free =
                composite && !self.nodes.contains_key(&Node { inst, port: k, outside: true });
            if !inside_free && !outside_free {
                continue;
            }
            let name = self.node_name(Node { inst, port: k, outside: false });
            match pv {
                PortVars::Physical { through, .. } => {
                    for _ in 0..(inside_free as usize + outside_free as usize) {
                        self.flat.equations.push(FlatEquation {
                            lhs: Expr::Var(through),
                            rhs: Expr::Const(0.0),
                            origin: Origin {
                                instance: inst,
                                kind: OriginKind::Unconnected { port: name.clone() },
                                label: None,
                            },
                        });
                    }
                }
                PortVars::Signal { output: false, .. } if inside_free => {
                    let who = self.describe(inst);
                    let port = self.defs[inst.0 as usize].ports[k].name.clone();
                    let lower = port.to_lowercase();
                    let mut d = if lower.contains("gear") || lower.contains("ratio") {
                        Diagnostic::error(
                            "GEAR-NO-RATIO",
                            format!(
                                "{who} has no gear selected: its input '{port}' is not connected, \
                                 so nothing says which ratio it runs in."
                            ),
                        )
                        .with_hint("Link its gear input to a gear selection or a Constant block.")
                    } else {
                        Diagnostic::error(
                            "SIGNAL-UNCONNECTED",
                            format!("The input '{port}' of {who} is not connected."),
                        )
                        .with_hint("Link it to a signal output, or to a Constant block.")
                    };
                    d.parts.push(self.flat.instance(self.flat.top_part(inst)).path.clone());
                    d.detail.push(name.clone());
                    self.diags.push(d);
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsim_ir::component::SubDecl;
    use lsim_ir::component::build::{connect, sub};
    use lsim_ir::expr::c;

    #[test]
    fn flattens_a_divider() {
        let lib = lsim_lib::library();
        let top = ComponentDef {
            name: "Divider".into(),
            components: vec![
                sub("src", "Electrical.ConstantVoltage", &[("V", c(10.0))]),
                sub("r1", "Electrical.Resistor", &[("R", c(1.0))]),
                sub("r2", "Electrical.Resistor", &[("R", c(3.0))]),
                sub("gnd", "Electrical.Ground", &[]),
            ],
            connections: vec![
                connect("src.p", "r1.p"),
                connect("r1.n", "r2.p"),
                connect("r2.n", "src.n"),
                connect("src.n", "gnd.p"),
            ],
            ..Default::default()
        };
        let flat = flatten(&lib, &top).expect("flattens");
        // 3 two-pins × 6 vars + ground 2 = 20 variables
        assert_eq!(flat.vars.len(), 20);
        // own equations 3·4 + 1 = 13; the sets {src.p, r1.p}, {r1.n, r2.p} and
        // {r2.n, src.n, gnd.p} give 1 + 1 + 2 across and 3 through equations
        assert_eq!(flat.equations.len(), 13 + 4 + 3);
        assert_eq!(flat.vars.len(), flat.equations.len());
        assert_eq!(flat.params.iter().find(|p| p.name == "r2.R").unwrap().value, 3.0);
    }

    #[test]
    fn flattens_tables_enumerations_and_asserts() {
        use lsim_ir::component::build::{eq, param, var};
        use lsim_ir::component::{EnumLiteral, EnumType, EquationDecl, Modifier, ParamDecl};
        use lsim_ir::expr::{CmpOp, cmp, name as n, table};
        let mut lib = Library::default();
        let mode = EnumType {
            name: "Mode".into(),
            literals: ["Off", "Eco", "Sport"]
                .iter()
                .map(|l| EnumLiteral { name: (*l).into(), doc: String::new() })
                .collect(),
            doc: String::new(),
        };
        let ocv = ParamDecl {
            default: ParamValue::Table1D {
                x: vec![0.0, 1.0],
                y: vec![3.0, 4.0],
                axis_unit: "1".into(),
            },
            ..param("ocv", "V", 0.0, "")
        };
        let m = ParamDecl {
            default: ParamValue::Enum("Mode.Eco".into()),
            unit: String::new(),
            ..param("mode", "", 0.0, "")
        };
        let cell = ComponentDef {
            name: "Cell".into(),
            types: vec![mode],
            params: vec![ocv, m],
            vars: vec![var("v", "V", ""), var("soc", "1", ""), var("k", "1", "")],
            equations: vec![
                eq(n("v"), table("ocv", vec![n("soc")]), "the OCV"),
                eq(n("soc"), c(0.5), ""),
                eq(n("k"), n("Mode.Sport") * n("mode"), ""),
                EquationDecl {
                    eq: Equation::Assert {
                        condition: cmp(CmpOp::Le, n("soc"), c(1.0)),
                        message: "full".into(),
                        error: false,
                    },
                    label: None,
                },
            ],
            ..Default::default()
        };
        lib.add(cell);
        let pack_table = ParamDecl {
            default: ParamValue::Table2D {
                x1: vec![0.0, 1.0],
                x2: vec![0.0, 1.0],
                values: vec![3.0, 3.1, 4.0, 4.1],
                axis_units: ["1".into(), "1".into()],
            },
            ..param("ocv_pack", "V", 0.0, "")
        };
        let top = ComponentDef {
            name: "Pack".into(),
            params: vec![pack_table],
            components: vec![
                SubDecl {
                    modifiers: vec![Modifier {
                        param: "mode".into(),
                        value: ParamValue::Enum("Mode.Off".into()),
                    }],
                    ..sub("a", "Cell", &[])
                },
                SubDecl {
                    modifiers: vec![Modifier {
                        param: "ocv".into(),
                        value: ParamValue::Table1D {
                            x: vec![0.0, 0.5, 1.0],
                            y: vec![3.0, 3.5, 4.2],
                            axis_unit: "1".into(),
                        },
                    }],
                    ..sub("b", "Cell", &[])
                },
            ],
            ..Default::default()
        };
        let flat = flatten(&lib, &top).unwrap_or_else(|d| panic!("{d:#?}"));
        // the pack's own table, then each cell's
        let names: Vec<&str> = flat.tables.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["ocv_pack", "a.ocv", "b.ocv"]);
        assert_eq!(flat.tables[2].data.x, vec![0.0, 0.5, 1.0]);
        let a_v = flat.find_var("a.v").unwrap();
        let read = flat.equations.iter().find(|e| e.lhs == Expr::Var(a_v)).unwrap();
        assert!(
            matches!(&read.rhs, Expr::Table { table: 1, args } if args.len() == 1),
            "{:?}",
            read.rhs
        );
        // enumeration options are numbers: Off = 1, Eco = 2, Sport = 3
        assert_eq!(flat.params[flat.find_param("a.mode").unwrap().0 as usize].value, 1.0);
        assert_eq!(flat.params[flat.find_param("b.mode").unwrap().0 as usize].value, 2.0);
        assert!(flat.params[flat.find_param("b.mode").unwrap().0 as usize].structural);
        let a_k = flat.find_var("a.k").unwrap();
        let k = flat.equations.iter().find(|e| e.lhs == Expr::Var(a_k)).unwrap();
        assert!(matches!(&k.rhs, Expr::Binary(_, x, _) if **x == Expr::Const(3.0)), "{:?}", k.rhs);
        assert_eq!(flat.asserts.len(), 2);
        assert_eq!(flat.asserts[0].message, "full");
        assert!(!flat.asserts[0].error);

        // a wrong option and a table used as a number are named
        let mut bad = top.clone();
        bad.components[0].modifiers[0].value = ParamValue::Enum("Mode.Turbo".into());
        bad.equations.push(eq(n("ocv_pack"), c(1.0), ""));
        let err = flatten(&lib, &bad).unwrap_err();
        let codes: Vec<&str> = err.iter().map(|d| d.code.as_str()).collect();
        assert_eq!(codes, ["ENUM-OPTION", "TABLE-AS-NUMBER"], "{err:#?}");
        assert!(err[0].message.contains("its options: Off, Eco, Sport"), "{}", err[0].message);
    }

    #[test]
    fn reports_unknown_names_with_the_part() {
        let mut lib = lsim_lib::library();
        let mut bad = lib.components["Electrical.Resistor"].clone();
        bad.name = "Bad".into();
        bad.equations.push(lsim_ir::component::build::eq(
            lsim_ir::expr::name("v"),
            lsim_ir::expr::name("Rx"),
            "typo",
        ));
        lib.add(bad);
        let mut s = sub("r", "Bad", &[]);
        s.label = Some("Heater".into());
        let top = ComponentDef { name: "T".into(), components: vec![s], ..Default::default() };
        let err = flatten(&lib, &top).unwrap_err();
        assert_eq!(err[0].code, "UNKNOWN-NAME");
        assert!(err[0].message.contains("'Heater' (Bad)"), "{}", err[0].message);
        assert!(err[0].message.contains("'Rx'"));
    }
}
