//! Today's project JSON → one top-level [`ComponentDef`] (DESIGN.md,
//! *Project JSON → IR*).
//!
//! [`facts::read`] reduces the project to what the importer needs (merged
//! parameters with the case's values, wires and signal links across
//! sub-systems, buses, wheels, the drivelines' ratios); each part then
//! becomes a sub-component through its [`BlockMapping`] in the
//! [`Registry`] ([`standard_registry`] holds the 36 of today's library):
//! the block definition its options and wiring ask for, its parameters in
//! SI (generic: every catalogue number the definition declares, converted
//! from today's unit), and the runtime data the mapping works out (tables,
//! scale factors, start speeds).
//!
//! The importer then adds what today's engine does implicitly:
//!
//! * the vehicle: every wheel pushes the body (`road`), reads its weight
//!   and its axle's load transfer, adds its rolling resistance; the body
//!   reads the air density from the Ambient (or standard air);
//! * the driver: the vehicle's speed, the motors' generator torque at the
//!   wheels (for blending recuperation and friction brakes) and the
//!   friction brakes' capacity;
//! * electrical: a ground for every negative terminal left open and for
//!   every circuit without one (today's implicit return); a bus manager
//!   for each bus whose battery or fuel cell hands out a power window;
//! * fuel: engines and fuel cells draw from the tank, or from an
//!   inexhaustible supply without one;
//! * profiles read against distance get the distance driven;
//! * a signal link between ports of different display units carries a
//!   conversion: today's blocks pass numbers in display units, and blocks
//!   without units (Constant, PID, Lookup, Script, FMU, Traction Control,
//!   Monitor) work in those numbers, so the link converts exactly as
//!   today's numbers read.
//!
//! The [`ImportReport`] carries the channel map (today's `element:port`
//! names → flat variables, with their display units), the generated
//! definitions, the sampled blocks the host must run, and the case's run
//! settings.

pub mod facts;

use crate::ident;
use facts::{Cycle, Element, Facts};
use lsim_ir::component::build::connect;
use lsim_ir::component::{Modifier, ParamValue, SubDecl};
use lsim_ir::expr::Expr;
use lsim_ir::{ComponentDef, Diagnostic, Library, PortKind};
use lsim_lib::blocks::catalog::{self, AppUnit};
use lsim_lib::blocks::{battery, driveline, electric, engine, motor, signals, vehicle};
use lsim_lib::table::{Table1, Table2};
use lsim_lib::x::{c, n};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

/// What a sampled block's host runs at each tick.
#[derive(Clone, Debug, PartialEq)]
pub enum SampledKind {
    /// a Script block: today's Python `step(t, dt, inputs, state, params)`
    Script {
        /// its code
        code: String,
    },
    /// an FMU for co-simulation
    Fmu {
        /// its file
        path: String,
    },
    /// the Traction Control block (today's `traction_control`)
    TractionControl,
}

/// A part as its mapping makes it.
#[derive(Clone, Debug)]
pub struct Mapped {
    /// its definition: a block in the configuration its options and wiring
    /// ask for
    pub def: ComponentDef,
    /// today's port id → the definition's port, where they differ
    pub ports: Vec<(String, String)>,
    /// values the mapping works out (tables, scale factors), as modifiers
    pub values: Vec<(String, ParamValue)>,
    /// for a sampled block: what its host runs
    pub sampled: Option<SampledKind>,
}

impl Mapped {
    fn of(def: ComponentDef) -> Mapped {
        Mapped { def, ports: vec![], values: vec![], sampled: None }
    }
    fn with(mut self, values: Vec<(String, ParamValue)>) -> Mapped {
        self.values.extend(values);
        self
    }
}

/// How one of today's block types becomes a part.
pub trait BlockMapping: Send + Sync {
    /// The part for `el`, knowing the whole project's `facts`.
    fn map(&self, el: &Element, facts: &Facts) -> Result<Mapped, String>;
}

impl<F> BlockMapping for F
where
    F: Fn(&Element, &Facts) -> Result<Mapped, String> + Send + Sync,
{
    fn map(&self, el: &Element, facts: &Facts) -> Result<Mapped, String> {
        self(el, facts)
    }
}

/// The mappings by `componentDefId`.
#[derive(Default)]
pub struct Registry {
    map: BTreeMap<String, Box<dyn BlockMapping>>,
}

impl Registry {
    /// Adds (or replaces) a mapping.
    pub fn add(&mut self, component_def_id: &str, m: Box<dyn BlockMapping>) {
        self.map.insert(component_def_id.to_string(), m);
    }

    /// A mapping.
    pub fn get(&self, component_def_id: &str) -> Option<&dyn BlockMapping> {
        self.map.get(component_def_id).map(|b| b.as_ref())
    }

    /// The block types it maps.
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.map.keys().map(String::as_str)
    }
}

/// A recorded channel: its flat variable and the display unit today's
/// results give it in (`value_display = unit.from_si(value)`).
#[derive(Clone, Debug, PartialEq)]
pub struct Channel {
    /// the flat variable
    pub var: String,
    /// today's display unit
    pub unit: AppUnit,
}

/// A sampled block the host must run.
#[derive(Clone, Debug)]
pub struct SampledSpec {
    /// the element's id
    pub element: String,
    /// its label
    pub label: String,
    /// its instance name in the model
    pub instance: String,
    /// what runs
    pub kind: SampledKind,
    /// its input ports, in the order the run loop hands them over
    pub inputs: Vec<String>,
    /// its output ports, in order
    pub outputs: Vec<String>,
    /// for each input, whether the project wires it (today an unwired
    /// input reads as absent)
    pub wired: Vec<bool>,
    /// its parameters as today has them (display units)
    pub params: Map<String, Value>,
    /// its tick spacing, s
    pub period: f64,
}

/// A case's kind (today's `kind`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CaseKind {
    /// follow the Driving Task
    #[default]
    Cycle,
    /// full throttle up to the target, then hold it
    Performance,
    /// full throttle over a distance
    Acceleration,
    /// a lap of the Race Track (today's lap solver)
    Lap,
}

/// How the case runs.
#[derive(Clone, Debug, Default)]
pub struct RunSettings {
    /// the case's id
    pub case_id: Option<String>,
    /// its name
    pub name: String,
    /// its kind
    pub kind: CaseKind,
    /// how long, s
    pub duration: f64,
    /// how often results are stored, s
    pub time_step: f64,
    /// store every Nth point
    pub output_every: usize,
    /// pacing (0: as fast as possible)
    pub realtime_factor: f64,
    /// an acceleration test: the start line and the finish (distance
    /// driven, m), and the flat variables that latch the times they are
    /// crossed
    pub timing: Option<Timing>,
}

/// An acceleration test's timing.
#[derive(Clone, Debug)]
pub struct Timing {
    /// the start line, m
    pub start_line: f64,
    /// the finish, m from the start of the run
    pub finish: f64,
    /// the variable holding the time the start line was crossed
    pub t_start: String,
    /// the variable holding the time the finish was crossed (−1 until then)
    pub t_finish: String,
    /// the variable holding the time 100 km/h was reached (−1 until then)
    pub t_100: String,
}

impl RunSettings {
    /// The output spacing, s.
    pub fn output_dt(&self) -> f64 {
        self.time_step * self.output_every.max(1) as f64
    }
}

/// What the import found besides the model.
#[derive(Debug, Default)]
pub struct ImportReport {
    /// today's channel name (`element:port`) → flat variable name
    pub channel_map: BTreeMap<String, String>,
    /// today's channel name → its variable and display unit
    pub channels: BTreeMap<String, Channel>,
    /// warnings (errors abort the import)
    pub warnings: Vec<Diagnostic>,
    /// the definitions the model uses that the library does not hold
    pub defs: Vec<ComponentDef>,
    /// the sampled blocks, in the model's order
    pub sampled: Vec<SampledSpec>,
    /// the case's settings
    pub run: RunSettings,
    /// element id → instance name
    pub instances: BTreeMap<String, String>,
}

impl ImportReport {
    /// The library the model is prepared against: the standard library
    /// and the generated definitions.
    pub fn library(&self) -> Library {
        let mut lib = lsim_lib::library();
        for d in &self.defs {
            lib.add(d.clone());
        }
        lib
    }
}

/// Where drive cycles come from.
#[derive(Clone, Debug, Default)]
pub struct ImportOptions {
    /// the folder of today's bundled cycles (`<id>.csv`: `t_s`,
    /// `speed_kmh`[, `grade_pct`]); `None`: the repository's
    /// `backend/app/cycles`
    pub cycles_dir: Option<PathBuf>,
}

/// The repository's folder of bundled drive cycles.
pub fn bundled_cycles_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../backend/app/cycles")
}

/// A bundled cycle's CSV as profile text.
fn read_csv_cycle(text: &str) -> Option<Cycle> {
    let mut lines = text.lines();
    let header: Vec<String> = lines.next()?.split(',').map(|h| h.trim().to_lowercase()).collect();
    let col = |name: &str| header.iter().position(|h| h == name);
    let (t, v, g) = (col("t_s")?, col("speed_kmh"), col("grade_pct"));
    let mut speed = vec![];
    let mut grade = vec![];
    for line in lines {
        let cells: Vec<&str> = line.split(',').map(str::trim).collect();
        let Some(x) = cells.get(t).and_then(|s| s.parse::<f64>().ok()) else { continue };
        if let Some(y) = v.and_then(|i| cells.get(i)).and_then(|s| s.parse::<f64>().ok()) {
            speed.push(format!("{x}:{y}"));
        }
        if let Some(y) = g.and_then(|i| cells.get(i)).and_then(|s| s.parse::<f64>().ok()) {
            grade.push(format!("{x}:{y}"));
        }
    }
    let join = |v: Vec<String>| (!v.is_empty()).then(|| v.join("; "));
    Some(Cycle { speed: join(speed), grade: join(grade), distance: false })
}

/// A project's own cycle (`cycles[]`, `own:…`) as profile text.
fn own_cycle(c: &Value) -> Cycle {
    let x: Vec<f64> = c["x"].as_array().into_iter().flatten().filter_map(Value::as_f64).collect();
    let col = |key: &str| -> Option<String> {
        let v = c[key].as_array()?;
        let pts: Vec<String> =
            x.iter().zip(v).filter_map(|(x, y)| y.as_f64().map(|y| format!("{x}:{y}"))).collect();
        (!pts.is_empty()).then(|| pts.join("; "))
    };
    Cycle { speed: col("speed"), grade: col("grade"), distance: c["axis"] == "distance" }
}

/// Imports a project with no case (the parts' own values, a 600 s run).
pub fn import(
    project: &Value,
    registry: &Registry,
) -> Result<(ComponentDef, ImportReport), Vec<Diagnostic>> {
    import_case(project, None, registry, &ImportOptions::default())
}

fn err(code: &str, text: impl Into<String>) -> Diagnostic {
    Diagnostic::error(code, text.into())
}

fn warning(code: &str, text: impl Into<String>) -> Diagnostic {
    Diagnostic { severity: lsim_ir::Severity::Warning, ..Diagnostic::error(code, text.into()) }
}

/// The standard library (built once).
fn standard_library() -> &'static Library {
    static LIB: std::sync::OnceLock<Library> = std::sync::OnceLock::new();
    LIB.get_or_init(lsim_lib::library)
}

/// A definition of the standard library.
fn lib_def(name: &str) -> ComponentDef {
    standard_library().components[name].clone()
}

/// The kinds whose ports carry no unit: they work in the numbers of what
/// they are wired to (today's display units).
const UNTYPED: [&str; 7] = [
    "signal.constant",
    "control.pid",
    "signal.lookup",
    "signal.script",
    "signal.fmu",
    "control.traction",
    "signal.monitor",
];

/// Imports a project for one of its cases (`None`: no case).
pub fn import_case(
    project: &Value,
    case_id: Option<&str>,
    registry: &Registry,
    opts: &ImportOptions,
) -> Result<(ComponentDef, ImportReport), Vec<Diagnostic>> {
    let case = match case_id {
        None => None,
        Some(id) => Some(
            project["cases"].as_array().into_iter().flatten().find(|c| c["id"] == id).ok_or_else(
                || vec![err("PROJECT-NO-CASE", format!("The project has no case '{id}'."))],
            )?,
        ),
    };
    let run = run_settings(case);
    match run.kind {
        CaseKind::Lap => {
            return Err(vec![err(
                "LAP-CASE",
                "Lap cases run in today's lap solver (quasi-steady, not a time simulation).",
            )]);
        }
        CaseKind::Performance => {
            return Err(vec![err(
                "NOT-YET",
                "Performance cases (full throttle up to the target, then hold it) are not imported yet.",
            )]);
        }
        _ => {}
    }
    let dir = opts.cycles_dir.clone().unwrap_or_else(bundled_cycles_dir);
    let own: Vec<Value> = project["cycles"].as_array().cloned().unwrap_or_default();
    let cycles = move |id: &str| -> Option<Cycle> {
        if let Some(c) = own.iter().find(|c| c["id"] == id) {
            return Some(own_cycle(c));
        }
        let safe = id.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c));
        if !safe {
            return None;
        }
        read_csv_cycle(&std::fs::read_to_string(dir.join(format!("{id}.csv"))).ok()?)
    };
    let mut facts = facts::read(project, case, &cycles)
        .map_err(|e| e.into_iter().map(|m| err("PROJECT", m)).collect::<Vec<_>>())?;
    facts.full_throttle = run.kind == CaseKind::Acceleration;
    let mut b = Builder::new(project, &facts, run);
    b.parts(registry);
    if b.errors.is_empty() {
        b.wires();
        b.links();
        b.vehicle();
        b.driver();
        b.electrical();
        b.fuel();
        b.profiles();
        b.timing();
        b.unwired_inputs();
        b.channels();
    }
    if !b.errors.is_empty() {
        return Err(b.errors);
    }
    let Builder { top, report, .. } = b;
    Ok((top, report))
}

fn run_settings(case: Option<&Value>) -> RunSettings {
    let Some(c) = case else {
        return RunSettings {
            duration: 600.0,
            time_step: 1.0,
            output_every: 1,
            ..Default::default()
        };
    };
    let num = |k: &str, d: f64| c[k].as_f64().unwrap_or(d);
    RunSettings {
        case_id: c["id"].as_str().map(String::from),
        name: c["name"].as_str().unwrap_or("").to_string(),
        kind: match c["kind"].as_str().unwrap_or("cycle") {
            "performance" => CaseKind::Performance,
            "acceleration" => CaseKind::Acceleration,
            "lap" => CaseKind::Lap,
            _ => CaseKind::Cycle,
        },
        duration: num("duration", 600.0),
        time_step: num("timeStep", 1.0),
        output_every: c["outputEvery"].as_u64().unwrap_or(1).max(1) as usize,
        realtime_factor: num("realtimeFactor", 0.0),
        timing: None,
    }
}

/// The importer's working state.
struct Builder<'a> {
    project: &'a Value,
    facts: &'a Facts,
    top: ComponentDef,
    report: ImportReport,
    errors: Vec<Diagnostic>,
    /// element id → its mapping's port renames
    ports: BTreeMap<String, Vec<(String, String)>>,
    /// element id → its definition
    defs: BTreeMap<String, ComponentDef>,
    /// generated definitions by name
    generated: BTreeMap<String, ComponentDef>,
    /// how many helper parts of each kind so far
    counter: BTreeMap<String, usize>,
}

impl<'a> Builder<'a> {
    fn new(project: &'a Value, facts: &'a Facts, run: RunSettings) -> Self {
        let top = ComponentDef {
            name: project["id"].as_str().map(ident).unwrap_or_else(|| "project".into()),
            doc: project["name"].as_str().unwrap_or("").to_string(),
            ..Default::default()
        };
        Builder {
            project,
            facts,
            top,
            report: ImportReport { run, ..Default::default() },
            errors: vec![],
            ports: BTreeMap::new(),
            defs: BTreeMap::new(),
            generated: BTreeMap::new(),
            counter: BTreeMap::new(),
        }
    }

    /// Uses `def`: adds it to the generated definitions unless the library
    /// holds it as it is.
    fn use_def(&mut self, def: &ComponentDef) {
        if standard_library().components.get(&def.name) == Some(def) {
            return;
        }
        match self.generated.get(&def.name) {
            Some(d) if d == def => {}
            Some(_) => self.errors.push(err(
                "IMPORT-INTERNAL",
                format!("two different definitions share the name '{}'", def.name),
            )),
            None => {
                self.generated.insert(def.name.clone(), def.clone());
                self.report.defs.push(def.clone());
            }
        }
    }

    /// Adds a helper part (not one of the project's): `prefix_<k>`.
    fn helper(&mut self, prefix: &str, def: ComponentDef, values: &[(&str, f64)]) -> String {
        self.use_def(&def);
        let k = self.counter.entry(prefix.to_string()).or_insert(0);
        *k += 1;
        let name = format!("{prefix}_{k}");
        self.top.components.push(SubDecl {
            name: name.clone(),
            def: def.name.clone(),
            modifiers: values
                .iter()
                .map(|(p, v)| Modifier { param: (*p).into(), value: ParamValue::Real(c(*v)) })
                .collect(),
            label: None,
            ui_id: None,
        });
        name
    }

    fn inst(&self, el: &str) -> String {
        ident(el)
    }

    /// The definition's port for today's port of an element.
    fn port(&self, el: &str, app_port: &str) -> String {
        self.ports
            .get(el)
            .and_then(|r| r.iter().find(|(a, _)| a == app_port))
            .map(|(_, d)| d.clone())
            .unwrap_or_else(|| app_port.to_string())
    }

    fn has_port(&self, el: &str, port: &str) -> bool {
        self.defs.get(el).is_some_and(|d| d.ports.iter().any(|p| p.name == port))
    }

    /// `instance.port` of an element's port (today's id).
    fn end(&self, el: &str, app_port: &str) -> String {
        format!("{}.{}", self.inst(el), self.port(el, app_port))
    }

    fn link(&mut self, a: String, b: String) {
        self.top.connections.push(connect(&a, &b));
    }

    fn parts(&mut self, registry: &Registry) {
        let facts = self.facts;
        for el in &facts.elements {
            let Some(m) = registry.get(&el.kind) else {
                self.errors.push(err(
                    "BLOCK-NOT-MAPPED",
                    format!(
                        "'{}' is a {}, which the new engine does not model yet.",
                        el.label, el.kind
                    ),
                ));
                continue;
            };
            let mapped = match m.map(el, facts) {
                Ok(x) => x,
                Err(e) => {
                    self.errors.push(err("BLOCK", format!("'{}' ({}): {e}", el.label, el.kind)));
                    continue;
                }
            };
            let def = mapped.def;
            self.use_def(&def);
            // today's numbers, in SI, for every catalogue parameter the
            // definition declares
            let mut modifiers = vec![];
            let given: BTreeSet<&str> = mapped.values.iter().map(|(k, _)| k.as_str()).collect();
            for p in &def.params {
                if given.contains(p.name.as_str()) || catalog::param(&el.kind, &p.name).is_none() {
                    continue;
                }
                if matches!(p.default, ParamValue::Real(_))
                    && let Some(v) = el.params.get(&p.name).and_then(catalog::num)
                {
                    let si = catalog::param_unit(&el.kind, &p.name).to_si(v);
                    modifiers
                        .push(Modifier { param: p.name.clone(), value: ParamValue::Real(c(si)) });
                }
            }
            for (k, v) in mapped.values {
                modifiers.push(Modifier { param: k, value: v });
            }
            let name = self.inst(&el.id);
            if let Some(kind) = mapped.sampled {
                let (inputs, outputs) = signal_ports(&def);
                let period = match el.params.get("sample_time_s").and_then(catalog::num) {
                    Some(t) if t > 0.0 => t,
                    _ => 0.01,
                };
                let wired = inputs.iter().map(|p| facts.is_wired(&el.id, p)).collect();
                self.report.sampled.push(SampledSpec {
                    element: el.id.clone(),
                    label: el.label.clone(),
                    instance: name.clone(),
                    kind,
                    inputs,
                    outputs,
                    wired,
                    params: el.params.clone(),
                    period,
                });
            }
            self.top.components.push(SubDecl {
                name: name.clone(),
                def: def.name.clone(),
                modifiers,
                label: Some(el.label.clone()),
                ui_id: Some(el.id.clone()),
            });
            self.report.instances.insert(el.id.clone(), name);
            self.ports.insert(el.id.clone(), mapped.ports);
            self.defs.insert(el.id.clone(), def);
        }
    }

    fn wires(&mut self) {
        for (a, b) in self.facts.wires.clone() {
            for (e, p) in [&a, &b] {
                let dp = self.port(e, p);
                if !self.has_port(e, &dp) {
                    let label = &self.facts.el(e).label;
                    self.errors.push(err(
                        "PORT-NOT-MAPPED",
                        format!(
                            "Port '{p}' of '{label}' is wired, but its block has no such port."
                        ),
                    ));
                }
            }
            let (ea, eb) = (self.end(&a.0, &a.1), self.end(&b.0, &b.1));
            self.link(ea, eb);
        }
    }

    /// A port's signal unit in its definition.
    fn signal_unit(&self, el: &str, port: &str) -> Option<String> {
        let d = self.defs.get(el)?;
        d.ports.iter().find(|p| p.name == port).and_then(|p| match &p.kind {
            PortKind::Input { unit } | PortKind::Output { unit } => Some(unit.clone()),
            _ => None,
        })
    }

    /// The numbers today's port carries: its display unit, or plain numbers
    /// for a block without units.
    fn display(&self, el: &str, app_port: &str) -> AppUnit {
        let e = self.facts.el(el);
        if UNTYPED.contains(&e.kind.as_str()) {
            return AppUnit { si: "1", scale: 1.0, offset: 0.0 };
        }
        e.port_unit(app_port)
    }

    fn links(&mut self) {
        for (from, to) in self.facts.links.clone() {
            let (pf, pt) = (self.port(&from.0, &from.1), self.port(&to.0, &to.1));
            let (Some(uf), Some(ut)) =
                (self.signal_unit(&from.0, &pf), self.signal_unit(&to.0, &pt))
            else {
                for (e, p, dp) in [(&from.0, &from.1, &pf), (&to.0, &to.1, &pt)] {
                    if self.signal_unit(e, dp).is_none() {
                        let label = self.facts.el(e).label.clone();
                        self.errors.push(err(
                            "PORT-NOT-MAPPED",
                            format!("Port '{p}' of '{label}' is linked, but its block has no such signal port."),
                        ));
                    }
                }
                continue;
            };
            let (ds, dt) = (self.display(&from.0, &from.1), self.display(&to.0, &to.1));
            let (a, b) = (self.end(&from.0, &from.1), self.end(&to.0, &to.1));
            if uf == ut && ds.scale == dt.scale && ds.offset == dt.offset {
                self.link(a, b);
                continue;
            }
            // y = k·u + b: the source's SI value → its display number →
            // read as the target's display number → the target's SI value
            let k = dt.scale / ds.scale;
            let off = dt.offset - dt.scale * ds.offset / ds.scale;
            let conv = lsim_lib::signal::convert(&uf, &ut);
            let h = self.helper("convert", conv, &[("k", k), ("b", off)]);
            self.link(a, format!("{h}.u"));
            self.link(format!("{h}.y"), b);
        }
    }

    /// A constant signal into `target`.
    fn constant_into(&mut self, target: String, unit: &str, value: f64) {
        let h = self.helper("constant", lsim_lib::signal::constant(unit), &[("k", value)]);
        self.link(format!("{h}.y"), target);
    }

    fn vehicle(&mut self) {
        let facts = self.facts;
        let veh = facts.vehicle.clone();
        // the air: the Ambient's density, else standard air
        if let Some(v) = &veh {
            let rho = format!("{}.rho", self.inst(v));
            match &facts.ambient {
                Some(a) => self.link(format!("{}.rho", self.inst(a)), rho),
                None => self.constant_into(rho, "kg/m3", vehicle::AIR_DENSITY),
            }
        }
        for cl in facts.of_kind("electric.climate") {
            let t = format!("{}.T_amb", self.inst(&cl.id));
            match &facts.ambient {
                Some(a) => self.link(format!("{}.T_amb", self.inst(a)), t),
                None => self.constant_into(t, "K", 293.15),
            }
        }
        let Some(v) = veh else { return };
        let vi = self.inst(&v);
        let wheels: Vec<String> = facts.of_kind("propulsion.wheel").map(|w| w.id.clone()).collect();
        for w in &wheels {
            if !self.has_port(w, "road") {
                continue;
            }
            let wi = self.inst(w);
            self.link(format!("{vi}.road"), format!("{wi}.road"));
            self.link(format!("{vi}.w_n"), format!("{wi}.w_n"));
            let front = facts.wheels.get(w).is_none_or(|s| s.front);
            let df = if front { "df_front" } else { "df_rear" };
            self.link(format!("{vi}.{df}"), format!("{wi}.df"));
        }
        if self.has_port(&v, "f_rr") {
            let on: Vec<String> =
                wheels.iter().filter(|w| self.has_port(w, "road")).cloned().collect();
            if on.is_empty() {
                self.constant_into(format!("{vi}.f_rr"), "N", 0.0);
            } else {
                let add = lsim_lib::signal::add("N", on.len());
                let h = self.helper("sum_rr", add, &[]);
                for (k, w) in on.iter().enumerate() {
                    self.link(format!("{}.f_rr", self.inst(w)), format!("{h}.u{}", k + 1));
                }
                self.link(format!("{h}.y"), format!("{vi}.f_rr"));
            }
        }
    }

    fn driver(&mut self) {
        let facts = self.facts;
        let Some(d) = facts.driver.clone() else { return };
        if facts.full_throttle {
            return;
        }
        let di = self.inst(&d);
        match &facts.vehicle {
            Some(v) => self.link(format!("{}.sig_speed", self.inst(v)), format!("{di}.v_vehicle")),
            None => self.constant_into(format!("{di}.v_vehicle"), "m/s", 0.0),
        }
        // the motors' generator torque at the wheels: each motor's limit ×
        // its speed per wheel speed (gearboxes in their current gear) ÷ the
        // efficiency of the way down (today's t_motor_cap)
        let mut terms: Vec<(String, f64, Vec<String>)> = vec![];
        for m in facts.of_kind("motor.emotor") {
            let Some(k) = facts.kin_of(&m.id, "shaft") else { continue };
            let mut gbs = k.gearboxes.clone();
            gbs.sort();
            gbs.dedup();
            let fixed: f64 =
                gbs.iter().map(|g| facts::default_ratio(facts.el(g))).product::<f64>().max(1e-12);
            terms.push((m.id.clone(), k.speed.abs() / fixed / k.eff.max(1e-3), gbs));
        }
        if terms.is_empty() {
            self.constant_into(format!("{di}.regen_cap"), "N.m", 0.0);
        } else {
            let all_gbs: Vec<String> = {
                let mut v: Vec<String> = terms.iter().flat_map(|t| t.2.clone()).collect();
                v.sort();
                v.dedup();
                v
            };
            let def = regen_cap(&terms, &all_gbs);
            let values: Vec<(String, f64)> =
                terms.iter().enumerate().map(|(i, t)| (format!("k{}", i + 1), t.1)).collect();
            let vals: Vec<(&str, f64)> = values.iter().map(|(a, b)| (a.as_str(), *b)).collect();
            let h = self.helper("regen_cap", def, &vals);
            for (i, t) in terms.iter().enumerate() {
                self.link(format!("{}.t_regen", self.inst(&t.0)), format!("{h}.t{}", i + 1));
            }
            for (j, g) in all_gbs.iter().enumerate() {
                self.link(format!("{}.ratio_now", self.inst(g)), format!("{h}.r{}", j + 1));
            }
            self.link(format!("{h}.y"), format!("{di}.regen_cap"));
        }
        // the friction brakes' torque at the wheels at a full command
        let fr_cap: f64 = facts
            .of_kind("mech.brake")
            .map(|b| {
                let m = facts.kin_of(&b.id, "flange").map(|k| k.speed.abs()).unwrap_or(1.0);
                b.num("max_torque_Nm", 0.0).max(0.0) * m
            })
            .sum();
        if let Some(s) = self.top.components.iter_mut().find(|s| s.name == di) {
            s.modifiers
                .push(Modifier { param: "fr_cap".into(), value: ParamValue::Real(c(fr_cap)) });
        }
    }

    fn electrical(&mut self) {
        let facts = self.facts;
        // every electrical port, and which are wired
        let mut ports: Vec<(String, String)> = vec![];
        for el in &facts.elements {
            let Some(d) = self.defs.get(&el.id) else { continue };
            for p in &d.ports {
                if matches!(&p.kind, PortKind::Physical { connector } if connector == "Pin") {
                    ports.push((el.id.clone(), p.name.clone()));
                }
            }
        }
        // a ground for every negative terminal left open (today's implicit
        // return)
        let mut grounds: Vec<String> = vec![];
        for (e, p) in &ports {
            let kind = facts.el(e).kind.as_str();
            if kind == "boundary.ground" || facts.is_wired(e, p) {
                continue;
            }
            if p == "neg" || p.ends_with("_neg") {
                let g = self.helper("ground", lib_def("Electrical.Ground"), &[]);
                self.link(format!("{}.{p}", self.inst(e)), format!("{g}.p"));
                grounds.push(e.clone());
            }
        }
        // a circuit with no ground: one at its source's negative terminal
        let mut parent: BTreeMap<String, String> = BTreeMap::new();
        fn find(p: &mut BTreeMap<String, String>, x: &str) -> String {
            let mut r = x.to_string();
            while let Some(q) = p.get(&r).cloned() {
                if q == r {
                    break;
                }
                r = q;
            }
            r
        }
        let union = |p: &mut BTreeMap<String, String>, a: &str, b: &str| {
            let (ra, rb) = (find(p, a), find(p, b));
            if ra != rb {
                p.insert(ra, rb);
            }
        };
        let elec: BTreeSet<String> = ports.iter().map(|(e, _)| e.clone()).collect();
        for e in &elec {
            parent.entry(e.clone()).or_insert(e.clone());
        }
        for (a, b) in &facts.wires {
            if elec.contains(&a.0) && elec.contains(&b.0) {
                union(&mut parent, &a.0, &b.0);
            }
        }
        let mut grounded: BTreeSet<String> = BTreeSet::new();
        for e in &elec {
            if facts.el(e).kind == "boundary.ground" || grounds.contains(e) {
                grounded.insert(find(&mut parent, e));
            }
        }
        for e in &elec {
            let r = find(&mut parent, e);
            if grounded.contains(&r) {
                continue;
            }
            let neg = ["neg", "a_neg", "b_neg"].into_iter().find(|p| self.has_port(e, p));
            if let Some(p) = neg {
                let g = self.helper("ground", lib_def("Electrical.Ground"), &[]);
                self.link(format!("{}.{p}", self.inst(e)), format!("{g}.p"));
                grounded.insert(r);
            }
        }
        // a bus manager on every bus whose source hands out a window
        for bus in facts.buses.iter().filter(|b| b.windowed()) {
            let (src, kind) = bus.source.clone().expect("a windowed bus has a source");
            let consumers: Vec<String> =
                bus.consumers.iter().filter(|c| self.has_port(c, "served")).cloned().collect();
            let motors: Vec<String> =
                bus.motors.iter().filter(|m| self.has_port(m, "p_hi")).cloned().collect();
            if consumers.is_empty() && motors.is_empty() {
                continue;
            }
            let h = self.helper("bus", electric::bus_manager(motors.len(), consumers.len()), &[]);
            let si = self.inst(&src);
            self.link(format!("{si}.p_deliver"), format!("{h}.p_deliver"));
            if kind == "battery.generic" {
                self.link(format!("{si}.p_absorb"), format!("{h}.p_absorb"));
            } else {
                self.constant_into(format!("{h}.p_absorb"), "W", 0.0);
            }
            for (k, cid) in consumers.iter().enumerate() {
                let ci = self.inst(cid);
                self.link(format!("{ci}.p_dem"), format!("{h}.p_dem{}", k + 1));
                self.link(format!("{h}.served"), format!("{ci}.served"));
            }
            for (k, mid) in motors.iter().enumerate() {
                let mi = self.inst(mid);
                self.link(format!("{mi}.p_request"), format!("{h}.p_req{}", k + 1));
                self.link(format!("{h}.p_hi{}", k + 1), format!("{mi}.p_hi"));
                self.link(format!("{h}.p_lo{}", k + 1), format!("{mi}.p_lo"));
            }
        }
        if facts.of_kind("controller.dcdc").next().is_some() {
            self.report.warnings.push(warning(
                "DCDC-WINDOW",
                "Buses behind a DC-DC converter have no bus manager yet: their motors are not \
                 held to the supply's window.",
            ));
        }
    }

    fn fuel(&mut self) {
        let facts = self.facts;
        for (kind, port, mass, tank) in [
            ("engine.combustion", "fuel", "fuel_mass", facts.fuel_tank.clone()),
            ("fuelcell.stack", "h2", "h2_mass", facts.h2_tank.clone()),
        ] {
            for e in facts.of_kind(kind) {
                let ei = self.inst(&e.id);
                match &tank {
                    Some(t) => {
                        let ti = self.inst(t);
                        self.link(format!("{ei}.{port}"), format!("{ti}.fuel"));
                        self.link(format!("{ti}.sig_mass"), format!("{ei}.{mass}"));
                    }
                    None => {
                        let lhv = if kind == "fuelcell.stack" {
                            engine::H2_LHV
                        } else {
                            engine::FUEL_LHV
                        };
                        let h = self.helper("supply", engine::supply(), &[("LHV", lhv)]);
                        self.link(format!("{ei}.{port}"), format!("{h}.fuel"));
                    }
                }
            }
        }
    }

    fn profiles(&mut self) {
        let facts = self.facts;
        for e in facts.of_kind("signal.driving_task").chain(facts.of_kind("signal.road_profile")) {
            if !self.has_port(&e.id, "x_in") || facts.is_wired(&e.id, "sig_distance_in") {
                continue;
            }
            let x = format!("{}.x_in", self.inst(&e.id));
            match &facts.vehicle {
                Some(v) => self.link(format!("{}.sig_distance", self.inst(v)), x),
                None => self.constant_into(x, "m", 0.0),
            }
        }
    }

    fn timing(&mut self) {
        let Some(case) = self.report.run.case_id.clone() else { return };
        if self.report.run.kind != CaseKind::Acceleration {
            return;
        }
        let c = self.project["cases"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|x| x["id"] == case.as_str())
            .cloned()
            .unwrap_or_default();
        let start = c["startLine"].as_f64().unwrap_or(0.0).max(0.0);
        let Some(dist) = c["endDistance"].as_f64().filter(|d| *d > 0.0) else { return };
        let Some(v) = self.facts.vehicle.clone() else { return };
        let h = self.helper(
            "timing",
            finish_line(),
            &[("start_line", start), ("finish", start + dist)],
        );
        self.link(format!("{}.sig_distance", self.inst(&v)), format!("{h}.d"));
        self.link(format!("{}.sig_speed", self.inst(&v)), format!("{h}.v"));
        self.report.run.timing = Some(Timing {
            start_line: start,
            finish: start + dist,
            t_start: format!("{h}.t_start"),
            t_finish: format!("{h}.t_finish"),
            t_100: format!("{h}.t_100"),
        });
    }

    /// Every signal input nothing drives reads 0 in its unit (today an
    /// unwired input reads as absent, and its block takes 0).
    fn unwired_inputs(&mut self) {
        let driven: BTreeSet<String> =
            self.top.connections.iter().flat_map(|c| [c.a.clone(), c.b.clone()]).collect();
        let mut todo: Vec<(String, String)> = vec![];
        for s in &self.top.components {
            let def = self
                .generated
                .get(&s.def)
                .cloned()
                .or_else(|| standard_library().components.get(&s.def).cloned());
            let Some(def) = def else { continue };
            for p in &def.ports {
                if let PortKind::Input { unit } = &p.kind {
                    let end = format!("{}.{}", s.name, p.name);
                    if !driven.contains(&end) {
                        todo.push((end, unit.clone()));
                    }
                }
            }
        }
        for (end, unit) in todo {
            self.constant_into(end, &unit, 0.0);
        }
    }

    fn channels(&mut self) {
        let facts = self.facts;
        for el in &facts.elements {
            let Some(def) = self.defs.get(&el.id) else { continue };
            let mut ids: Vec<String> = catalog::block(&el.kind)
                .and_then(|b| b["ports"].as_array())
                .into_iter()
                .flatten()
                .filter(|p| p["kind"] == "signal")
                .filter_map(|p| p["id"].as_str().map(String::from))
                .collect();
            ids.extend(el.dynamic_ports.iter().filter_map(|p| p["id"].as_str().map(String::from)));
            for app in ids {
                let dp = self.port(&el.id, &app);
                if !def
                    .ports
                    .iter()
                    .any(|p| p.name == dp && !matches!(p.kind, PortKind::Physical { .. }))
                {
                    continue;
                }
                let key = format!("{}:{app}", el.id);
                let var = format!("{}.{dp}", self.inst(&el.id));
                let unit = self.display(&el.id, &app);
                self.report.channel_map.insert(key.clone(), var.clone());
                self.report.channels.insert(key, Channel { var, unit });
            }
        }
    }
}

/// A sampled definition's signal ports: (inputs, outputs) in order.
fn signal_ports(def: &ComponentDef) -> (Vec<String>, Vec<String>) {
    let mut i = vec![];
    let mut o = vec![];
    for p in &def.ports {
        match p.kind {
            PortKind::Input { .. } => i.push(p.name.clone()),
            PortKind::Output { .. } => o.push(p.name.clone()),
            _ => {}
        }
    }
    (i, o)
}

/// The motors' generator torque at the wheels, for the driver's blending:
/// y = Σ k_m · t_m · Π (current ratio of each gearbox below motor m).
fn regen_cap(terms: &[(String, f64, Vec<String>)], gearboxes: &[String]) -> ComponentDef {
    use lsim_lib::x::{input, output, p};
    let mut ports = vec![];
    let mut params = vec![];
    let mut sum: Option<Expr> = None;
    let mut fp: Vec<f64> = vec![terms.len() as f64, gearboxes.len() as f64];
    for (i, t) in terms.iter().enumerate() {
        ports.push(input(&format!("t{}", i + 1), "N.m", "a motor's generator torque limit"));
        params.push(p(
            &format!("k{}", i + 1),
            "1",
            1.0,
            "its speed per wheel speed over its way's efficiency",
        ));
        let mut e = n(&format!("k{}", i + 1)) * n(&format!("t{}", i + 1));
        for g in &t.2 {
            let j = gearboxes.iter().position(|x| x == g).expect("listed") + 1;
            e = e * n(&format!("r{j}"));
            fp.extend([i as f64, j as f64]);
        }
        sum = Some(match sum {
            None => e,
            Some(s) => s + e,
        });
    }
    for j in 1..=gearboxes.len() {
        ports.push(input(&format!("r{j}"), "1", "a gearbox's current ratio"));
    }
    ports.push(output("y", "N.m", "the motors' generator torque at the wheels"));
    ComponentDef {
        name: format!("Import.RegenCap_{}", lsim_lib::signal::fingerprint(fp)),
        doc: "The motors' generator torque at the wheels (the driver's blending).".into(),
        ports,
        params,
        equations: vec![lsim_ir::component::build::eq(
            n("y"),
            sum.unwrap_or(c(0.0)),
            "Σ k·t·ratios",
        )],
        ..Default::default()
    }
}

/// An acceleration test's timing: the times the distance driven passes the
/// start line and the finish (−1 until then).
fn finish_line() -> ComponentDef {
    use lsim_lib::x::{discrete, ge, input, p, time, when};
    ComponentDef {
        name: "Import.FinishLine".into(),
        doc: "An acceleration test's timing: when the start line and the finish are passed.".into(),
        ports: vec![input("d", "m", "the distance driven"), input("v", "m/s", "the speed")],
        params: vec![
            p("start_line", "m", 0.0, "the start line"),
            p("finish", "m", 75.0, "the finish, from the start of the run"),
            p("v100", "m/s", 100.0 / 3.6, "100 km/h"),
        ],
        vars: vec![
            discrete("t_start", "s", -1.0, "when the start line was passed"),
            discrete("t_finish", "s", -1.0, "when the finish was passed"),
            discrete("t_100", "s", -1.0, "when 100 km/h was reached"),
        ],
        equations: vec![
            when(ge(n("d"), n("start_line")), &[("t_start", time())], "it passes the start line"),
            when(ge(n("d"), n("finish")), &[("t_finish", time())], "it passes the finish"),
            when(ge(n("v"), n("v100")), &[("t_100", time())], "it reaches 100 km/h"),
        ],
        ..Default::default()
    }
}

// ---- the mappings ------------------------------------------------------

fn wired(el: &Element, f: &Facts, port: &str) -> bool {
    f.is_wired(&el.id, port)
}

fn table1(el: &Element, key: &str) -> Result<Table1, String> {
    Table1::from_json(el.params.get(key).ok_or(format!("no table '{key}'"))?, el.outside(key, 0))
}

fn table2(el: &Element, key: &str) -> Result<Table2, String> {
    Table2::from_json(
        el.params.get(key).ok_or(format!("no table '{key}'"))?,
        el.outside(key, 0),
        el.outside(key, 1),
    )
}

fn w0(el: &Element, f: &Facts, port: &str, param: &str) -> Vec<(String, ParamValue)> {
    f.kin_of(&el.id, port)
        .map(|k| vec![(param.to_string(), ParamValue::Real(c(k.w0)))])
        .unwrap_or_default()
}

fn text<'a>(el: &'a Element, key: &str, default: &'a str) -> &'a str {
    catalog::text(&el.params, key, default)
}

/// Today's 36 block types.
pub fn standard_registry() -> Registry {
    let mut r = Registry::default();
    macro_rules! add {
        ($id:expr, $f:expr) => {
            r.add($id, Box::new($f));
        };
    }
    add!("boundary.ground", |_: &Element, _: &Facts| Ok(Mapped::of(electric::ground())));
    add!("electric.node", |el: &Element, f: &Facts| {
        let term =
            f.buses.iter().flat_map(|b| &b.nodes).find(|(n, _)| *n == el.id).and_then(|x| x.1);
        Ok(Mapped::of(electric::electric_node(term)))
    });
    add!("electric.voltage_source", |_: &Element, _: &Facts| Ok(Mapped::of(
        electric::voltage_source()
    )));
    add!("electric.constant_drive", |el: &Element, f: &Facts| {
        Ok(Mapped::of(electric::power_consumer(electric::LoadConfig {
            demand_wired: wired(el, f, "sig_demand_in"),
            served_input: f.windowed(&el.id),
        })))
    });
    add!("electric.climate", |el: &Element, f: &Facts| {
        let cfg = electric::ClimateConfig {
            demand: table1(el, "demand_table")?.scaled(1.0, 1e3),
            heat_pump: text(el, "heat_source", "PTC heater") == "Heat pump",
            enable_wired: wired(el, f, "sig_on_in"),
            served_input: f.windowed(&el.id),
        };
        Ok(Mapped::of(electric::climate(&cfg)).with(cfg.values()?))
    });
    add!("controller.dcdc", |el: &Element, f: &Facts| {
        // a battery on its output bus: it delivers its setpoint
        let setpoint_mode = f.bus_of(&el.id, "b_pos").is_some_and(|b| {
            b.members.iter().any(|(e, p)| p == "pos" && f.el(e).kind == "battery.generic")
        });
        if setpoint_mode {
            return Err(
                "a DC-DC converter feeding a battery bus (setpoint mode) is not imported yet"
                    .into(),
            );
        }
        Ok(Mapped::of(electric::dcdc(electric::DcDcConfig {
            setpoint_mode,
            setpoint_wired: wired(el, f, "sig_setpoint_in"),
        })))
    });
    add!("battery.generic", |el: &Element, _: &Facts| {
        if text(el, "pack_model", "Pack values") != "Pack values" {
            return Err("a battery defined by its cells is not imported yet".into());
        }
        let cfg = battery::BatteryConfig::from_params(&el.params, el.outside("ocv_table", 0))?;
        Ok(Mapped::of(battery::battery(&cfg)).with(cfg.values()?))
    });
    add!("motor.emotor", |el: &Element, f: &Facts| {
        let cfg = motor::MotorConfig::from_params(
            &el.params,
            &|k, a| el.outside(k, a),
            f.windowed(&el.id),
        )?;
        Ok(Mapped::of(motor::emotor(&cfg)).with(cfg.values()?).with(w0(el, f, "shaft", "w0")))
    });
    add!("vehicle.body", |el: &Element, f: &Facts| {
        let front = f.wheels.values().any(|w| w.front);
        let rear = f.wheels.values().any(|w| !w.front);
        let cfg = vehicle::BodyConfig {
            abc: text(el, "road_load_mode", "") == "Coefficients A/B/C",
            two_axles: front && rear,
            single_front: !rear,
            grade_wired: wired(el, f, "sig_grade_in"),
        };
        let share =
            |fr: bool| f.wheels.values().filter(|w| w.front == fr).map(|w| w.share).sum::<f64>();
        Ok(Mapped::of(vehicle::body(cfg)).with(vec![
            ("share_front".into(), ParamValue::Real(c(share(true)))),
            ("share_rear".into(), ParamValue::Real(c(share(false)))),
        ]))
    });
    add!("propulsion.wheel", |el: &Element, f: &Facts| {
        let on_vehicle = f.vehicle.is_some() && f.wheels.contains_key(&el.id);
        let mut values = w0(el, f, "shaft", "w0");
        if let (true, Some(s), Some(v)) = (on_vehicle, f.wheels.get(&el.id), &f.vehicle) {
            let mass = f.el(v).num("mass_kg", 1800.0);
            values.extend([
                ("share".into(), ParamValue::Real(c(s.share))),
                ("axle_part".into(), ParamValue::Real(c(s.axle_part))),
                ("fz_static".into(), ParamValue::Real(c(s.share * mass * vehicle::GRAVITY))),
            ]);
        }
        Ok(Mapped::of(vehicle::wheel(vehicle::WheelConfig { on_vehicle })).with(values))
    });
    add!("driver.driver", |el: &Element, f: &Facts| {
        Ok(Mapped::of(vehicle::driver(vehicle::DriverConfig {
            full_throttle: f.full_throttle,
            speed_wired: wired(el, f, "sig_speed_in"),
            target_wired: wired(el, f, "sig_target_in"),
        })))
    });
    add!("boundary.ambient", |_: &Element, _: &Facts| Ok(Mapped::of(vehicle::ambient())));
    add!("mech.node", |_: &Element, _: &Facts| Ok(Mapped::of(driveline::mech_node())));
    add!("mech.shaft", |el: &Element, f: &Facts| {
        Ok(Mapped::of(driveline::shaft()).with(w0(el, f, "flange_a", "w0")))
    });
    add!("mech.final_drive", |el: &Element, f: &Facts| {
        let mut m = Mapped::of(driveline::final_drive()).with(w0(el, f, "flange_out", "w0"));
        if f.lossless_axle {
            m.values.push(("efficiency_pct".into(), ParamValue::Real(c(1.0))));
        }
        Ok(m)
    });
    add!("mech.gearbox", |el: &Element, f: &Facts| {
        let cfg = driveline::GearboxConfig {
            ratios: table1(el, "ratios")?,
            select_wired: wired(el, f, "sig_gear_in"),
        };
        Ok(Mapped::of(driveline::gearbox(&cfg)).with(w0(el, f, "flange_out", "w0")))
    });
    for (id, tc) in [("mech.differential", false), ("mech.transfer_case", true)] {
        add!(id, move |el: &Element, f: &Facts| {
            let cfg = driveline::SplitConfig {
                transfer_case: tc,
                locked: catalog::flag(&el.params, "locked", false),
            };
            let mut m = Mapped::of(driveline::split(cfg)).with(w0(el, f, "flange_in", "w0"));
            if f.lossless_axle {
                m.values.push(("efficiency_pct".into(), ParamValue::Real(c(1.0))));
            }
            Ok(m)
        });
    }
    add!("mech.clutch", |el: &Element, f: &Facts| {
        Ok(Mapped::of(driveline::clutch(driveline::ClutchConfig {
            engage_wired: wired(el, f, "sig_engage_in"),
        })))
    });
    add!("mech.brake", |el: &Element, f: &Facts| {
        Ok(Mapped::of(driveline::brake(driveline::BrakeConfig {
            demand_wired: wired(el, f, "sig_demand_in"),
        }))
        .with(w0(el, f, "flange", "s0")))
    });
    add!("propulsion.propeller", |el: &Element, f: &Facts| {
        Ok(Mapped::of(driveline::propeller()).with(w0(el, f, "shaft", "w0")))
    });
    add!("engine.combustion", |el: &Element, f: &Facts| {
        let cfg = engine::EngineConfig::from_params(
            &el.params,
            &|k, a| el.outside(k, a),
            wired(el, f, "sig_throttle_in"),
            wired(el, f, "sig_on_in"),
            f.fuel_tank.is_some(),
        )?;
        Ok(Mapped::of(engine::engine(&cfg)).with(cfg.values()?).with(w0(el, f, "shaft", "w0")))
    });
    add!("fuel.tank", |_: &Element, _: &Facts| Ok(Mapped::of(engine::tank(false))));
    add!("fuel.h2_tank", |_: &Element, _: &Facts| Ok(Mapped::of(engine::tank(true))));
    add!("fuelcell.stack", |el: &Element, f: &Facts| {
        let cfg = engine::FuelCellConfig {
            polarization: table1(el, "polarization")?,
            tank: f.h2_tank.is_some(),
        };
        Ok(Mapped::of(engine::fuel_cell(&cfg)).with(cfg.values()?))
    });
    for (id, task) in [("signal.driving_task", true), ("signal.road_profile", false)] {
        add!(id, move |el: &Element, _: &Facts| {
            let text_p = text(el, "profile", "");
            let distance = text(el, "mode", if task { "time" } else { "distance" }) == "distance";
            let t = Table1::from_profile(text_p);
            if t.x.is_empty() {
                return Err("its profile has no points".into());
            }
            let cfg = signals::ProfileConfig {
                table: if task { t.scaled(1.0, 1.0 / 3.6) } else { t.scaled(1.0, 0.01) },
                distance,
                repeat: catalog::flag(&el.params, "repeat", false),
            };
            let def = if task { signals::driving_task(&cfg) } else { signals::road_profile(&cfg) };
            let mut m = Mapped::of(def).with(cfg.values(if distance { "m" } else { "s" })?);
            m.ports.push(("sig_distance_in".into(), "x_in".into()));
            Ok(m)
        });
    }
    add!("signal.constant", |_: &Element, _: &Facts| Ok(Mapped::of(signals::constant())));
    add!("control.pid", |_: &Element, _: &Facts| Ok(Mapped::of(signals::pid())));
    add!("signal.lookup", |el: &Element, _: &Facts| {
        let cfg = if text(el, "mode", "1D") == "2D" {
            signals::LookupConfig::D2(table2(el, "table_2d")?)
        } else {
            signals::LookupConfig::D1(table1(el, "table_1d")?)
        };
        Ok(Mapped::of(signals::lookup(&cfg)).with(cfg.values()?))
    });
    for id in ["signal.script", "signal.fmu"] {
        add!(id, move |el: &Element, _: &Facts| {
            let mut ports = signals::SampledPorts::default();
            for p in &el.dynamic_ports {
                let pid = p["id"].as_str().unwrap_or("").to_string();
                match p["direction"].as_str() {
                    Some("input") => ports.inputs.push((pid, "1".into())),
                    Some("output") => ports.outputs.push((pid, "1".into())),
                    _ => {}
                }
            }
            let (base, kind) = if id == "signal.script" {
                ("Script", SampledKind::Script { code: text(el, "code", "").to_string() })
            } else {
                ("Fmu", SampledKind::Fmu { path: text(el, "fmu_path", "").to_string() })
            };
            let mut m = Mapped::of(signals::sampled(base, id, &ports));
            m.sampled = Some(kind);
            Ok(m)
        });
    }
    add!("control.traction", |_: &Element, _: &Facts| {
        let def = lsim_lib::blocks::signals::defaults()
            .into_iter()
            .find(|d| d.name == "External.TractionControl")
            .expect("the traction control block");
        let mut m = Mapped::of(def);
        m.sampled = Some(SampledKind::TractionControl);
        Ok(m)
    });
    add!("signal.monitor", |el: &Element, f: &Facts| {
        let inputs: Vec<(String, String)> = el
            .dynamic_ports
            .iter()
            .filter(|p| p["direction"] == "input")
            .filter_map(|p| p["id"].as_str())
            .filter(|p| wired(el, f, p))
            .map(|p| (p.to_string(), "1".to_string()))
            .collect();
        Ok(Mapped::of(signals::monitor(&inputs)))
    });
    add!("track.lap", |_: &Element, _: &Facts| Ok(Mapped::of(signals::race_track())));
    // a sub-system's container has no physics: its parts are imported in
    // its place (facts::read skips it)
    add!("container.system", |_: &Element, _: &Facts| Ok(Mapped::of(signals::system())));
    r
}
