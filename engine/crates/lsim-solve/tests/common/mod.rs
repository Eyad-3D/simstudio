//! Shared helpers for the solver's tests: building models from component
//! definitions (lsim-prep and lsim-codegen, as the engine does), the
//! reference problems of `benchmarks/reference/problems` (their
//! checkpoints: the exact answer at four times and the event times), and
//! a few test-only equation components (level sensors) that turn a
//! problem's threshold into a `when` clause the solver locates.

#![allow(dead_code)]

use lsim_codegen::{CodegenOptions, JitModel};
use lsim_ir::component::build::{discrete, eq, param, port};
use lsim_ir::expr::{c, cmp, name as n};
use lsim_ir::{CmpOp, ComponentDef, Equation, EquationDecl, Library, PreparedModel, WhenAction};
use lsim_solve::RunInfo;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// A built model.
pub struct Built {
    pub prepared: PreparedModel,
    pub jit: JitModel,
    pub info: RunInfo,
}

/// Prepares and compiles `top` against `lib`.
pub fn build(lib: &Library, top: &ComponentDef, force_implicit: bool) -> Built {
    let prepared = lsim_prep::prepare(lib, top, &lsim_prep::PrepOptions { force_implicit })
        .unwrap_or_else(|d| panic!("{} prepares: {d:#?}", top.name));
    let jit = lsim_codegen::compile(&prepared, &CodegenOptions::default()).expect("compiles");
    let info = RunInfo::from_prepared(&prepared);
    Built { prepared, jit, info }
}

/// The Stage 1 library plus the test-only sensors.
pub fn library() -> Library {
    let mut lib = lsim_lib::library();
    for s in sensors() {
        lib.add(s);
    }
    lib
}

fn when(cond: lsim_ir::Expr, var: &str, label: &str) -> EquationDecl {
    EquationDecl {
        eq: Equation::When {
            condition: cond,
            actions: vec![WhenAction::Assign { var: var.into(), value: c(1.0) }],
        },
        label: Some(label.into()),
    }
}

/// Sensors that take no power and latch a `when` clause at a level: the
/// reference problems' threshold events, located by the solver's root
/// finding.
pub fn sensors() -> Vec<ComponentDef> {
    vec![
        ComponentDef {
            name: "Test.CurrentLevel".into(),
            doc: "Ammeter in series: latches when the current from p to n reaches `level`.".into(),
            ports: vec![port("p", "Pin", ""), port("n", "Pin", "")],
            params: vec![param("level", "A", 0.0, "")],
            vars: vec![discrete("reached", "1", 0.0, "")],
            equations: vec![
                eq(n("p.v"), n("n.v"), "no voltage across it"),
                eq(c(0.0), n("p.i") + n("n.i"), "the current passes through"),
                when(
                    cmp(CmpOp::Ge, n("p.i"), n("level")),
                    "reached",
                    "the current reaches its level",
                ),
            ],
            ..Default::default()
        },
        ComponentDef {
            name: "Test.VoltageLevel".into(),
            doc: "Voltmeter: latches when p.v - n.v reaches `level`.".into(),
            ports: vec![port("p", "Pin", ""), port("n", "Pin", "")],
            params: vec![param("level", "V", 0.0, "")],
            vars: vec![discrete("reached", "1", 0.0, "")],
            equations: vec![
                eq(n("p.i"), c(0.0), "no current into p"),
                eq(n("n.i"), c(0.0), "no current into n"),
                when(
                    cmp(CmpOp::Ge, n("p.v") - n("n.v"), n("level")),
                    "reached",
                    "the voltage reaches its level",
                ),
            ],
            ..Default::default()
        },
        ComponentDef {
            name: "Test.SpeedLevel".into(),
            doc: "Tachometer: latches when the flange speed reaches `level`.".into(),
            ports: vec![port("flange", "Flange", "")],
            params: vec![param("level", "rad/s", 0.0, "")],
            vars: vec![discrete("reached", "1", 0.0, "")],
            equations: vec![
                eq(n("flange.tau"), c(0.0), "no torque"),
                when(
                    cmp(CmpOp::Ge, n("flange.w"), n("level")),
                    "reached",
                    "the speed reaches its level",
                ),
            ],
            ..Default::default()
        },
    ]
}

/// One comparison of a problem.
#[derive(Clone, Debug)]
pub struct Compare {
    pub name: String,
    pub kind: String,
    pub signal: Option<String>,
    pub level: Option<f64>,
    pub rtol: Option<f64>,
    pub atol: Option<f64>,
    pub scale: Option<f64>,
}

/// A reference problem's data and checkpoints.
#[derive(Clone, Debug)]
pub struct Problem {
    pub id: String,
    pub parameters: BTreeMap<String, f64>,
    pub initial: BTreeMap<String, f64>,
    pub t_end: f64,
    pub output_dt: f64,
    pub compare: Vec<Compare>,
    pub sources: Vec<String>,
    pub sinks: Vec<String>,
    /// checkpoint times
    pub times: Vec<f64>,
    /// exact values at the checkpoint times, by quantity
    pub exact: BTreeMap<String, Vec<f64>>,
    /// exact event times
    pub events: BTreeMap<String, f64>,
}

/// The repository's `benchmarks/` folder.
pub fn benchmarks_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../benchmarks")
}

fn num(v: &toml::Value) -> f64 {
    match v {
        toml::Value::Float(f) => *f,
        toml::Value::Integer(i) => *i as f64,
        other => panic!("not a number: {other:?}"),
    }
}

impl Problem {
    /// Reads `benchmarks/reference/problems/<id>.toml`.
    pub fn load(id: &str) -> Problem {
        let path = benchmarks_dir().join("reference/problems").join(format!("{id}.toml"));
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
        let t: toml::Table = toml::from_str(&text).expect("valid TOML");
        let values = |key: &str| -> BTreeMap<String, f64> {
            t.get(key)
                .and_then(|v| v.as_table())
                .map(|tab| {
                    tab.iter()
                        .map(|(k, v)| {
                            let x = v.as_table().and_then(|e| e.get("value")).unwrap_or(v);
                            (k.clone(), num(x))
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
        let run = t["run"].as_table().expect("[run]");
        let compare = t
            .get("compare")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .map(|c| {
                        let c = c.as_table().expect("[[compare]]");
                        let s = |k: &str| c.get(k).and_then(|v| v.as_str()).map(String::from);
                        let f = |k: &str| c.get(k).map(num);
                        Compare {
                            name: s("name").expect("name"),
                            kind: s("kind").expect("kind"),
                            signal: s("signal"),
                            level: f("level"),
                            rtol: f("rtol"),
                            atol: f("atol"),
                            scale: f("scale"),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        let list = |k: &str| -> Vec<String> {
            t.get("energy")
                .and_then(|e| e.get(k))
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                .unwrap_or_default()
        };
        let cp = t["checkpoints"].as_table().expect("[checkpoints]");
        let times: Vec<f64> = cp["times"].as_array().unwrap().iter().map(num).collect();
        let mut exact = BTreeMap::new();
        let mut events = BTreeMap::new();
        for (k, v) in cp {
            match v {
                toml::Value::Array(a) if k != "times" => {
                    exact.insert(k.clone(), a.iter().map(num).collect());
                }
                toml::Value::Table(ev) if k == "events" => {
                    for (e, x) in ev {
                        events.insert(e.clone(), num(x));
                    }
                }
                _ => {}
            }
        }
        Problem {
            id: id.into(),
            parameters: values("parameters"),
            initial: values("initial"),
            t_end: num(&run["t_end"]),
            output_dt: num(&run["output_dt"]),
            compare,
            sources: list("sources"),
            sinks: list("sinks"),
            times,
            exact,
            events,
        }
    }

    /// A parameter's value.
    pub fn p(&self, name: &str) -> f64 {
        *self.parameters.get(name).unwrap_or_else(|| panic!("{}: no parameter {name}", self.id))
    }
}

/// The default tolerances of `benchmarks/targets.toml` [accuracy].
#[derive(Clone, Copy, Debug)]
pub struct Targets {
    pub signal_rtol: f64,
    pub event_atol_s: f64,
    pub event_rtol: f64,
    pub energy_rtol: f64,
    pub closure_rtol: f64,
}

pub fn targets() -> Targets {
    let text = std::fs::read_to_string(benchmarks_dir().join("targets.toml")).expect("targets");
    let t: toml::Table = toml::from_str(&text).expect("valid TOML");
    let a = t["accuracy"].as_table().expect("[accuracy]");
    Targets {
        signal_rtol: num(&a["signal_rtol"]),
        event_atol_s: num(&a["event_atol_s"]),
        event_rtol: num(&a["event_rtol"]),
        energy_rtol: num(&a["energy_rtol"]),
        closure_rtol: num(&a["closure_rtol"]),
    }
}
