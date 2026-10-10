//! The golden comparison (DESIGN.md 12.3): every time-domain case of every
//! example project runs on the new engine and is compared, figure by figure
//! and channel by channel, with today's engine.
//!
//! Today's engine integrates with semi-implicit Euler steps of at most
//! 10 ms; its reference runs (`golden/reference.py`) are made at that step
//! and at a tenth of it. Each figure's band is measured from them: the
//! larger of twice the difference between the two and a floor (0.1 % for
//! energies, 0.5 % for times and everything else); the new engine must fall
//! within the band of today's fine-step result. A channel is compared by
//! the root-mean-square of its difference to today's fine run, against a
//! band of twice the RMS difference between today's two runs and a floor of
//! 0.5 % of the channel's RMS, over every output point but the first (today
//! records it before its first solver step, when the signals its blocks
//! publish still read 0).
//!
//! Sampled blocks run in hosts here: a Script block runs today's own script
//! runner in a Python process (the stand-in for work package 6's sandbox),
//! the Traction Control block today's rule in Rust.

use crate::import::{CaseKind, ImportOptions, ImportReport, SampledKind, SampledSpec};
use crate::model::Model;
use lsim_ir::runtime::DiscreteBlock;
use lsim_solve::{OutputGrid, SimResult, SolverOptions};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

/// Where today's Python engine is, for Script blocks.
#[derive(Clone, Debug)]
pub struct PythonHost {
    /// the Python interpreter
    pub python: PathBuf,
    /// the repository's `backend` folder (today's engine)
    pub backend: PathBuf,
}

const BRIDGE: &str = r#"
import json, sys
sys.path.insert(0, '.')
from app.solver.scripting import compile_script, run_script
first = json.loads(sys.stdin.readline())
fn = compile_script(first['code'], first['label'])
state = {}
params = first['params']
sys.stdout.write('ready\n'); sys.stdout.flush()
for line in sys.stdin:
    msg = json.loads(line)
    try:
        res = run_script(fn, first['label'], msg['t'], msg['dt'], msg['inputs'], state, params)
        sys.stdout.write(json.dumps(res) + '\n')
    except Exception as e:
        sys.stdout.write(json.dumps({'__error__': str(e)}) + '\n')
    sys.stdout.flush()
"#;

/// A Script block run by today's own script runner in a Python process:
/// `step(t, dt, inputs, state, params)` every period, its inputs and
/// outputs in today's display numbers (the importer's conversions see to
/// that), outputs it does not return holding their last value.
pub struct ScriptHost {
    name: String,
    period: f64,
    inputs: Vec<String>,
    outputs: Vec<String>,
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl ScriptHost {
    /// Starts the Python process for `spec`.
    pub fn start(spec: &SampledSpec, host: &PythonHost) -> Result<ScriptHost, String> {
        let SampledKind::Script { code } = &spec.kind else {
            return Err("not a Script block".into());
        };
        let mut child = Command::new(&host.python)
            .arg("-c")
            .arg(BRIDGE)
            .current_dir(&host.backend)
            .env("LIGHTSIM_SCRIPT_TRUST", "off")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| format!("starting {}: {e}", host.python.display()))?;
        let mut stdin = child.stdin.take().ok_or("no stdin")?;
        let mut stdout = BufReader::new(child.stdout.take().ok_or("no stdout")?);
        let first = json!({"code": code, "label": spec.label, "params": Value::Object(spec.params.clone())});
        writeln!(stdin, "{first}").map_err(|e| e.to_string())?;
        stdin.flush().map_err(|e| e.to_string())?;
        let mut line = String::new();
        stdout.read_line(&mut line).map_err(|e| e.to_string())?;
        if line.trim() != "ready" {
            return Err(format!("Script '{}' did not start: {line}", spec.label));
        }
        Ok(ScriptHost {
            name: spec.label.clone(),
            period: spec.period,
            inputs: spec.inputs.clone(),
            outputs: spec.outputs.clone(),
            child,
            stdin,
            stdout,
        })
    }

    fn call(&mut self, t: f64, inputs: &[f64], outputs: &mut [f64]) -> Result<(), String> {
        let ins: Map<String, Value> =
            self.inputs.iter().zip(inputs).map(|(k, v)| (k.clone(), json!(v))).collect();
        let msg = json!({"t": t, "dt": self.period, "inputs": ins});
        writeln!(self.stdin, "{msg}").map_err(|e| e.to_string())?;
        self.stdin.flush().map_err(|e| e.to_string())?;
        let mut line = String::new();
        self.stdout.read_line(&mut line).map_err(|e| e.to_string())?;
        let v: Value = serde_json::from_str(&line).map_err(|e| format!("{e}: {line}"))?;
        if let Some(e) = v.get("__error__") {
            return Err(e.as_str().unwrap_or("").to_string());
        }
        for (k, o) in self.outputs.iter().zip(outputs.iter_mut()) {
            if let Some(x) = v.get(k).and_then(Value::as_f64) {
                *o = x;
            }
        }
        Ok(())
    }
}

impl Drop for ScriptHost {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl DiscreteBlock for ScriptHost {
    fn name(&self) -> &str {
        &self.name
    }
    fn period(&self) -> f64 {
        self.period
    }
    // today runs a Script at t = 0 (the start) and then every period
    fn offset(&self) -> f64 {
        self.period
    }
    fn init(&mut self, t0: f64, inputs: &[f64], outputs: &mut [f64]) -> Result<(), String> {
        self.call(t0, inputs, outputs)
    }
    fn tick(&mut self, t: f64, inputs: &[f64], outputs: &mut [f64]) -> Result<(), String> {
        self.call(t, inputs, outputs)
    }
}

/// The Traction Control block: today's `traction_control` exactly, in
/// today's display numbers (a slip, km/h); unwired slip and speed inputs
/// are absent, as today.
pub struct TractionHost {
    name: String,
    period: f64,
    p: Map<String, Value>,
    wired: Vec<bool>,
    integral: f64,
    launch_t: Option<f64>,
}

impl TractionHost {
    /// From its spec.
    pub fn new(spec: &SampledSpec) -> Self {
        TractionHost {
            name: spec.label.clone(),
            period: spec.period,
            p: spec.params.clone(),
            wired: spec.wired.clone(),
            integral: 1.0,
            launch_t: None,
        }
    }

    fn step(&mut self, t: f64, inputs: &[f64]) -> f64 {
        use lsim_lib::blocks::catalog::get;
        let target = get(&self.p, "target_slip", 0.1);
        let kp = get(&self.p, "kp", 0.5);
        let ki = get(&self.p, "ki", 10.0);
        let ramp = get(&self.p, "launch_ramp_s", 0.3).max(0.0);
        let start = (get(&self.p, "launch_torque_pct", 60.0) / 100.0).clamp(0.0, 1.0);
        let v_min = get(&self.p, "min_speed_kmh", 5.0).max(0.0);
        let input = |k: usize| self.wired.get(k).copied().unwrap_or(false).then(|| inputs[k]);
        let demand = inputs[0];
        let (slip, slip2, speed) = (input(1), input(2), input(3));
        if demand <= 0.0 {
            self.integral = 1.0;
            self.launch_t = None;
            return demand;
        }
        let slow = speed.is_some_and(|s| s.abs() < v_min);
        if slow && self.launch_t.is_none() {
            self.launch_t = Some(t);
        }
        let mut limit: f64 = 1.0;
        if let Some(t0) = self.launch_t
            && ramp > 0.0
        {
            limit = (start + (1.0 - start) * (t - t0) / ramp).min(1.0);
        }
        let slips: Vec<f64> = [slip, slip2].into_iter().flatten().collect();
        if !slips.is_empty() && !slow {
            let err = target - slips.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            self.integral = (self.integral + ki * err * self.period).clamp(0.0, 1.0);
            limit = limit.min((self.integral + kp * err).clamp(0.0, 1.0));
        } else if slow {
            self.integral = limit;
        }
        if limit >= 1.0 && !slow {
            self.launch_t = None;
        }
        demand.min(limit)
    }
}

impl DiscreteBlock for TractionHost {
    fn name(&self) -> &str {
        &self.name
    }
    fn period(&self) -> f64 {
        self.period
    }
    fn offset(&self) -> f64 {
        self.period
    }
    fn init(&mut self, t0: f64, inputs: &[f64], outputs: &mut [f64]) -> Result<(), String> {
        outputs[0] = self.step(t0, inputs);
        Ok(())
    }
    fn tick(&mut self, t: f64, inputs: &[f64], outputs: &mut [f64]) -> Result<(), String> {
        outputs[0] = self.step(t, inputs);
        Ok(())
    }
}

/// The hosts of a model's sampled blocks, in the run loop's order.
pub fn hosts(
    model: &Model,
    rep: &ImportReport,
    python: Option<&PythonHost>,
) -> Result<Vec<Box<dyn DiscreteBlock>>, String> {
    let mut out: Vec<Box<dyn DiscreteBlock>> = vec![];
    for b in &model.info.blocks {
        let spec = rep
            .sampled
            .iter()
            .find(|s| {
                s.instance == b.name
                    || s.label == b.name
                    || b.name.contains(&format!("'{}'", s.label))
            })
            .ok_or_else(|| format!("no host for the sampled block {}", b.name))?;
        match &spec.kind {
            SampledKind::Script { .. } => {
                let py = python.ok_or("Script blocks need today's Python engine")?;
                out.push(Box::new(ScriptHost::start(spec, py)?));
            }
            SampledKind::TractionControl => out.push(Box::new(TractionHost::new(spec))),
            SampledKind::Fmu { .. } => {
                return Err(format!("FMU '{}': co-simulation is work package 6's", spec.label));
            }
        }
    }
    Ok(out)
}

/// A summary figure.
#[derive(Clone, Debug, PartialEq)]
pub struct Figure {
    /// today's key (`el-battery.final_soc_pct`), or "" for rows without one
    pub key: String,
    /// today's label
    pub label: String,
    /// the value
    pub value: f64,
    /// its unit
    pub unit: String,
}

/// One case run on the new engine.
pub struct CaseRun {
    /// the import's report
    pub report: ImportReport,
    /// the results
    pub result: SimResult,
    /// the summary figures, by today's definitions
    pub figures: Vec<Figure>,
    /// wall-clock time of the build, s
    pub build_seconds: f64,
    /// wall-clock time of the run(s), s
    pub run_seconds: f64,
}

/// Imports, builds and runs a case; an acceleration test runs again to the
/// moment it crosses the finish, where today's run ends.
pub fn run_case(
    project: &Value,
    case_id: &str,
    solver: &SolverOptions,
    python: Option<&PythonHost>,
) -> Result<CaseRun, String> {
    let reg = crate::import::standard_registry();
    let (top, rep) =
        crate::import::import_case(project, Some(case_id), &reg, &ImportOptions::default())
            .map_err(|e| e.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("; "))?;
    let t0 = std::time::Instant::now();
    let lib = rep.library();
    let model = Model::build(&lib, &top)
        .map_err(|e| e.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("; "))?;
    let build_seconds = t0.elapsed().as_secs_f64();
    let run = &rep.run;
    let t1 = std::time::Instant::now();
    let go = |t_end: f64| -> Result<SimResult, String> {
        let mut blocks = hosts(&model, &rep, python)?;
        let grid = OutputGrid { t0: 0.0, t_end, dt: run.output_dt() };
        model.run(solver, grid, &mut blocks).map_err(|e| e.to_string())
    };
    let mut result = go(run.duration)?;
    if let Some(tm) = &run.timing {
        let t_fin = result.channel(&tm.t_finish).and_then(|c| c.last().copied()).unwrap_or(-1.0);
        if t_fin > 0.0 && t_fin < run.duration {
            result = go(t_fin)?;
        }
    }
    let run_seconds = t1.elapsed().as_secs_f64();
    let figures = figures(project, &rep, &model, &result);
    Ok(CaseRun { report: rep, result, figures, build_seconds, run_seconds })
}

/// What the battery figures need of a battery as the case runs it.
#[derive(Clone)]
struct BatteryFacts {
    /// its charge capacity, A·h
    q_ah: f64,
    /// its minimum SOC, 0-1
    min_soc: f64,
    /// its OCV table, by SOC 0-1
    ocv: lsim_lib::table::Table1,
}

impl BatteryFacts {
    fn of(model: &Model, inst: &str, params: &Map<String, Value>) -> Option<BatteryFacts> {
        let cfg = lsim_lib::blocks::battery::BatteryConfig::from_params(
            params,
            lsim_lib::blocks::catalog::axis_outside("battery.generic", "ocv_table", 0),
        )
        .ok()?;
        Some(BatteryFacts {
            q_ah: model.param(&format!("{inst}.Q"))? / 3600.0,
            min_soc: model.param(&format!("{inst}.min_soc_pct"))?,
            ocv: cfg.ocv,
        })
    }

    /// The OCV's mean between two SOCs (today's `ocv_mean`).
    fn ocv_mean(&self, lo: f64, hi: f64) -> f64 {
        if hi > lo { self.ocv.integral(lo, hi) / (hi - lo) } else { self.ocv.eval(lo) }
    }
}

/// A channel's largest (or smallest) value over the run, from the
/// output intervals' extremes.
fn extreme(res: &SimResult, name: &str, largest: bool) -> Option<f64> {
    let i = res.names.iter().position(|n| n == name)?;
    let pick = |a: f64, b: f64| if largest { a.max(b) } else { a.min(b) };
    let first = res.values[i].first().copied()?;
    let ext = if largest { &res.max[i] } else { &res.min[i] };
    Some(ext.iter().skip(1).copied().fold(first, pick))
}

/// The largest moving average of a channel over `window` s (today's
/// `MovingAverage`: no power before the start, the window's mean of the
/// output intervals' means, read at the output times; 0 s: the largest
/// value).
fn peak_moving_average(res: &SimResult, name: &str, window: f64) -> Option<f64> {
    if window <= 0.0 {
        return extreme(res, name, true);
    }
    let i = res.names.iter().position(|n| n == name)?;
    let t0 = res.times.first().copied()?;
    // ∫ from the start to each output time
    let mut cum = vec![0.0; res.times.len()];
    for k in 1..res.times.len() {
        cum[k] = cum[k - 1] + res.mean[i][k] * (res.times[k] - res.times[k - 1]);
    }
    let at = |t: f64| -> f64 {
        if t <= t0 {
            return 0.0;
        }
        let k = res.times.partition_point(|x| *x < t).min(res.times.len() - 1);
        let (ta, tb) = (res.times[k - 1], res.times[k]);
        cum[k - 1] + res.mean[i][k] * (t - ta).min(tb - ta)
    };
    let mut best = f64::NEG_INFINITY;
    for (k, &t) in res.times.iter().enumerate().skip(1) {
        best = best.max((cum[k] - at(t - window)) / window);
    }
    Some(best)
}

fn last(res: &SimResult, name: &str) -> Option<f64> {
    res.channel(name).and_then(|c| c.last().copied())
}

/// ∫ of a channel over the run, from each output interval's time-mean.
fn integral(res: &SimResult, name: &str, f: impl Fn(f64) -> f64) -> Option<f64> {
    let i = res.names.iter().position(|n| n == name)?;
    let mut s = 0.0;
    for k in 1..res.times.len() {
        s += f(res.mean[i][k]) * (res.times[k] - res.times[k - 1]);
    }
    Some(s)
}

fn cycle_phases(cycle: &str) -> Vec<(String, f64, f64)> {
    let path = crate::import::bundled_cycles_dir().join("cycles.json");
    let Ok(text) = std::fs::read_to_string(path) else { return vec![] };
    let Ok(v) = serde_json::from_str::<Value>(&text) else { return vec![] };
    let entry = if v["cycles"].is_object() { &v["cycles"][cycle] } else { &v[cycle] };
    entry["phases"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            let a = p.as_array()?;
            Some((a.first()?.as_str()?.to_string(), a.get(1)?.as_f64()?, a.get(2)?.as_f64()?))
        })
        .collect()
}

/// The summary figures by today's definitions (`core.py`, `labfig.py`,
/// `verdict.py`), from the new engine's channels.
pub fn figures(project: &Value, rep: &ImportReport, model: &Model, res: &SimResult) -> Vec<Figure> {
    let mut out = vec![];
    let mut add = |key: String, label: String, value: f64, unit: &str| {
        out.push(Figure { key, label, value, unit: unit.to_string() });
    };
    let elements: Vec<(String, String, String, Map<String, Value>)> = project["systems"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|s| s["elements"].as_array().cloned().unwrap_or_default())
        .map(|e| {
            (
                e["id"].as_str().unwrap_or("").to_string(),
                e["componentDefId"].as_str().unwrap_or("").to_string(),
                e["label"].as_str().unwrap_or("").to_string(),
                e["parameterOverrides"].as_object().cloned().unwrap_or_default(),
            )
        })
        .collect();
    let inst = |id: &str| rep.instances.get(id).cloned().unwrap_or_default();
    type El = (String, String, String, Map<String, Value>);
    fn of_kind<'a>(els: &'a [El], kind: &'a str) -> impl Iterator<Item = &'a El> + 'a {
        els.iter().filter(move |e| e.1 == kind)
    }
    let of = |kind: &'static str| of_kind(&elements, kind);
    let kwh = 1.0 / 3.6e6;
    let mut net_j = 0.0;
    let mut batteries: Vec<BatteryFacts> = vec![];
    let mut checked: Vec<String> = vec![];
    // a part's parameters as the case runs it: the catalog's defaults, its
    // own values and the case's
    let case_overrides = project["cases"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|c| c["id"].as_str().is_some() && c["id"].as_str() == rep.run.case_id.as_deref())
        .map(|c| c["parameterOverrides"].clone())
        .unwrap_or(Value::Null);
    let params_of = |id: &str| -> Map<String, Value> {
        let Some((_, kind, _, own)) = elements.iter().find(|e| e.0 == id) else {
            return Map::new();
        };
        let mut own = own.clone();
        if let Some(o) = case_overrides[id].as_object() {
            own.extend(o.clone());
        }
        lsim_lib::blocks::catalog::merged(kind, &own)
    };
    for (id, _, label, _) in of("battery.generic") {
        let i = inst(id);
        let g = |v: &str| last(res, &format!("{i}.{v}")).unwrap_or(0.0);
        add(format!("{id}.final_soc_pct"), format!("{label} — final SOC"), g("soc") * 100.0, "%");
        add(
            format!("{id}.energy_delivered_kwh"),
            format!("{label} — energy delivered"),
            g("e_delivered") * kwh,
            "kWh",
        );
        add(
            format!("{id}.energy_recuperated_kwh"),
            format!("{label} — energy recuperated"),
            g("e_recuperated") * kwh,
            "kWh",
        );
        add(
            format!("{id}.internal_losses_kwh"),
            format!("{label} — internal losses"),
            g("e_losses") * kwh,
            "kWh",
        );
        net_j += g("e_delivered") - g("e_recuperated");
        let bf = BatteryFacts::of(model, &i, &params_of(id));
        if let Some(bf) = &bf {
            let p = params_of(id);
            let limit_kw = lsim_lib::blocks::catalog::get(&p, "output_power_limit_kW", 0.0);
            let v_class = lsim_lib::blocks::catalog::get(&p, "voltage_class_V", 0.0);
            if limit_kw > 0.0 || v_class > 0.0 {
                checked.push(id.clone());
                let power = format!("{i}.sig_power");
                let volt = format!("{i}.sig_voltage");
                if limit_kw > 0.0 {
                    let window = lsim_lib::blocks::catalog::get(&p, "power_limit_window_s", 0.0);
                    add(
                        format!("{id}.peak_terminal_power_kw"),
                        format!("{label} — peak terminal power"),
                        extreme(res, &power, true).unwrap_or(0.0) / 1000.0,
                        "kW",
                    );
                    add(
                        format!("{id}.peak_terminal_power_averaged_kw"),
                        format!("{label} — peak terminal power, averaged"),
                        peak_moving_average(res, &power, window).unwrap_or(0.0) / 1000.0,
                        "kW",
                    );
                    add(
                        format!("{id}.time_at_power_limit_s"),
                        format!("{label} — time held at the output power limit"),
                        g("t_held"),
                        "s",
                    );
                }
                if v_class > 0.0 {
                    let v_peak = extreme(res, &volt, true).unwrap_or(0.0);
                    add(
                        format!("{id}.max_pack_voltage_v"),
                        format!("{label} — maximum pack voltage"),
                        bf.ocv.eval(1.0).max(v_peak),
                        "V",
                    );
                }
                add(
                    format!("{id}.min_pack_voltage_v"),
                    format!("{label} — minimum pack voltage"),
                    extreme(res, &volt, false).unwrap_or(0.0),
                    "V",
                );
                let (soc, floor) = (g("soc").clamp(0.0, 1.0), bf.min_soc.clamp(0.0, 1.0));
                let left = if soc > floor {
                    bf.q_ah * (soc - floor) * bf.ocv_mean(floor, soc) / 1000.0
                } else {
                    0.0
                };
                add(
                    format!("{id}.usable_energy_left_kwh"),
                    format!("{label} — usable energy left"),
                    left,
                    "kWh",
                );
            }
            batteries.push(bf.clone());
        }
    }
    for (id, _, label, _) in of("motor.emotor") {
        let i = inst(id);
        let tl = last(res, &format!("{i}.t_limited")).unwrap_or(0.0);
        if tl > 0.0 {
            add(
                format!("{id}.time_limited_by_supply_s"),
                format!("{label} — time limited by supply"),
                tl,
                "s",
            );
        }
        let lost = last(res, &format!("{i}.e_regen_lost")).unwrap_or(0.0);
        if lost > 0.0 {
            add(
                format!("{id}.regen_not_recovered_kwh"),
                format!("{label} — regeneration not recovered"),
                lost * kwh,
                "kWh",
            );
        }
    }
    let mut fuel_kg = 0.0;
    for (id, _, label, _) in of("engine.combustion") {
        let f = last(res, &format!("{}.fuel_used", inst(id))).unwrap_or(0.0);
        fuel_kg += f;
        add(format!("{id}.fuel_used_kg"), format!("{label} — fuel used"), f, "kg");
    }
    for (id, _, label, _) in of("fuelcell.stack") {
        let e = integral(res, &format!("{}.sig_power", inst(id)), |x| x).unwrap_or(0.0);
        add(
            format!("{id}.energy_supplied_kwh"),
            format!("{label} — energy supplied"),
            e * kwh,
            "kWh",
        );
    }
    for (id, _, label, _) in of("electric.climate") {
        let i = inst(id);
        let e = integral(res, &format!("{i}.sig_power"), |x| x).unwrap_or(0.0);
        add(format!("{id}.energy_used_kwh"), format!("{label} — energy used"), e * kwh, "kWh");
        let heat = integral(res, &format!("{i}.sig_heat"), |x| x.max(0.0)).unwrap_or(0.0);
        if heat > 0.0 {
            add(
                format!("{id}.heating_delivered_kwh"),
                format!("{label} — heating delivered"),
                heat * kwh,
                "kWh",
            );
        }
        let cool = integral(res, &format!("{i}.sig_heat"), |x| (-x).max(0.0)).unwrap_or(0.0);
        if cool > 0.0 {
            add(
                format!("{id}.cooling_delivered_kwh"),
                format!("{label} — cooling delivered"),
                cool * kwh,
                "kWh",
            );
        }
    }
    for (id, _, label, _) in of("electric.voltage_source") {
        let e = integral(res, &format!("{}.sig_power", inst(id)), |x| x).unwrap_or(0.0);
        add(
            format!("{id}.energy_supplied_kwh"),
            format!("{label} — energy supplied"),
            e * kwh,
            "kWh",
        );
    }
    let veh = of("vehicle.body").next().map(|e| inst(&e.0));
    if let Some(v) = &veh {
        let dist = last(res, &format!("{v}.dist")).unwrap_or(0.0);
        let km = dist / 1000.0;
        add("distance_km".into(), "Distance driven".into(), km, "km");
        let net_wh = net_j / 3600.0;
        if dist > 100.0 && net_wh > 0.0 {
            add(
                "consumption_kwh_per_100km".into(),
                "Consumption".into(),
                net_wh / 10.0 / km,
                "kWh/100km",
            );
        }
        let tank = of("fuel.tank").next();
        let tank_p = |k: &str, d: f64| {
            tank.map(|t| lsim_lib::blocks::catalog::merged("fuel.tank", &t.3))
                .map(|m| lsim_lib::blocks::catalog::get(&m, k, d))
                .unwrap_or(d)
        };
        let density = tank_p("density_kg_per_l", 0.745).max(1e-3);
        if dist > 100.0 && fuel_kg > 0.0 {
            let co2 = tank_p("co2_kg_per_kg", 3.17).max(0.0);
            add(
                "fuel_consumption_l_per_100km".into(),
                "Fuel consumption".into(),
                fuel_kg / density * 100.0 / km,
                "l/100km",
            );
            add("co2_g_per_km".into(), "CO₂ emissions".into(), fuel_kg * co2 * 1000.0 / km, "g/km");
        }
        // the lab rows
        let electric = net_wh > 0.0 && fuel_kg <= 0.0 && of("fuelcell.stack").next().is_none();
        if km > 0.1 && electric {
            let dc = net_wh / 10.0 / km;
            let effs: Vec<f64> = of("battery.generic")
                .map(|b| {
                    let m = lsim_lib::blocks::catalog::merged("battery.generic", &b.3);
                    lsim_lib::blocks::catalog::get(&m, "charger_efficiency_pct", 86.0)
                })
                .collect();
            let eff =
                (effs.iter().sum::<f64>() / effs.len().max(1) as f64).clamp(1.0, 100.0) / 100.0;
            let ac = dc / eff;
            add(
                "consumption_ac_kwh_per_100km".into(),
                "Consumption at the socket (AC)".into(),
                ac,
                "kWh/100km",
            );
            add(
                "mpge_ac".into(),
                "Fuel-economy equivalent (MPGe, AC)".into(),
                33.705 / (ac / 100.0 * 1.609344),
                "MPGe",
            );
            let usable_wh: f64 = batteries
                .iter()
                .filter(|b| b.min_soc < 1.0)
                .map(|b| b.q_ah * (1.0 - b.min_soc.max(0.0)) * b.ocv_mean(b.min_soc, 1.0))
                .sum();
            if usable_wh > 0.0 {
                add(
                    "range_km".into(),
                    "Range at this consumption".into(),
                    usable_wh / (dc * 10.0),
                    "km",
                );
            }
        }
        if km > 0.1 && fuel_kg > 0.0 && of("battery.generic").next().is_some() {
            let lhv = tank_p("lhv_MJ_per_kg", 42.9) * 1e6;
            let share = -100.0 * net_wh * 3600.0 / (fuel_kg * lhv);
            add(
                "battery_energy_change_pct_of_fuel".into(),
                "Battery energy change, share of fuel energy".into(),
                share,
                "%",
            );
            let work_wh: f64 = of("engine.combustion")
                .map(|e| last(res, &format!("{}.work_fired", inst(&e.0))).unwrap_or(0.0) / 3600.0)
                .sum();
            if work_wh > 0.0 {
                let corrected = fuel_kg * (1.0 + net_wh / work_wh);
                add(
                    "fuel_consumption_corrected_l_per_100km".into(),
                    "Fuel consumption, charge-corrected".into(),
                    corrected / density * 100.0 / km,
                    "l/100km",
                );
            }
        }
        // per phase, at the phase ends of a bundled cycle
        if rep.run.kind == CaseKind::Cycle
            && let Some(task) = of("signal.driving_task").next()
        {
            let cycle = rep_cycle(project, &rep.run, &task.0);
            let phases = cycle.map(|c| cycle_phases(&c)).unwrap_or_default();
            let at = |name: &str, t: f64| -> f64 {
                let Some(ch) = res.channel(name) else { return 0.0 };
                match res.times.iter().position(|x| *x >= t - 1e-9) {
                    Some(k) => ch[k],
                    None => *ch.last().unwrap_or(&0.0),
                }
            };
            let net_at = |t: f64| -> f64 {
                of("battery.generic")
                    .map(|b| {
                        let i = inst(&b.0);
                        at(&format!("{i}.e_delivered"), t) - at(&format!("{i}.e_recuperated"), t)
                    })
                    .sum::<f64>()
                    / 3600.0
            };
            let fuel_at = |t: f64| -> f64 {
                of("engine.combustion").map(|e| at(&format!("{}.fuel_used", inst(&e.0)), t)).sum()
            };
            let end = res.times.last().copied().unwrap_or(0.0);
            for (k, (name, a, b)) in phases.iter().enumerate() {
                if *b > end + 1e-9 {
                    break;
                }
                let d_km = (at(&format!("{v}.dist"), *b) - at(&format!("{v}.dist"), *a)) / 1000.0;
                if d_km <= 0.0 {
                    continue;
                }
                let k = k + 1;
                add(
                    format!("phase{k}_distance_km"),
                    format!("Phase {name} — distance"),
                    d_km,
                    "km",
                );
                if electric || (net_wh > 0.0 && fuel_kg <= 0.0) {
                    add(
                        format!("phase{k}_consumption_kwh_per_100km"),
                        format!("Phase {name} — consumption"),
                        (net_at(*b) - net_at(*a)) / 10.0 / d_km,
                        "kWh/100km",
                    );
                }
                if fuel_kg > 0.0 {
                    add(
                        format!("phase{k}_fuel_consumption_l_per_100km"),
                        format!("Phase {name} — fuel consumption"),
                        (fuel_at(*b) - fuel_at(*a)) / density * 100.0 / d_km,
                        "l/100km",
                    );
                }
            }
        }
    }
    if let Some(tm) = &rep.run.timing {
        let (ts, tf) =
            (last(res, &tm.t_start).unwrap_or(-1.0), last(res, &tm.t_finish).unwrap_or(-1.0));
        let d = tm.finish - tm.start_line;
        if ts >= 0.0 && tf >= 0.0 {
            add("accel_time_s".into(), format!("Time to {d} m"), tf - ts, "s");
            if let Some(v) = &veh {
                let sp = last(res, &format!("{v}.v")).unwrap_or(0.0) * 3.6;
                add("accel_end_speed_kmh".into(), format!("Speed at {d} m"), sp, "km/h");
            }
        }
        let reference_time = project["cases"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|c| c["id"].as_str() == rep.run.case_id.as_deref())
            .and_then(|c| c["referenceTime"].as_f64())
            .unwrap_or(0.0);
        if ts >= 0.0 && tf >= 0.0 && reference_time > 0.0 {
            add(
                "accel_gap_to_reference_s".into(),
                "Gap to reference time".into(),
                tf - ts - reference_time,
                "s",
            );
        }
        let t100 = last(res, &tm.t_100).unwrap_or(-1.0);
        if t100 > 0.0 {
            add("time_to_100_kmh_s".into(), "Time to 100 km/h".into(), t100, "s");
        }
        let run_s =
            res.times.last().copied().unwrap_or(0.0) - res.times.first().copied().unwrap_or(0.0);
        for (id, _, label, _) in of("battery.generic") {
            let i = inst(id);
            if !checked.contains(id)
                || lsim_lib::blocks::catalog::get(&params_of(id), "output_power_limit_kW", 0.0)
                    <= 0.0
            {
                add(
                    format!("{id}.peak_terminal_power_kw"),
                    format!("{label} — peak terminal power"),
                    extreme(res, &format!("{i}.sig_power"), true).unwrap_or(0.0) / 1000.0,
                    "kW",
                );
            }
            if run_s > 0.0 {
                let g = |v: &str| last(res, &format!("{i}.{v}")).unwrap_or(0.0);
                add(
                    format!("{id}.mean_terminal_power_kw"),
                    format!("{label} — mean terminal power"),
                    (g("e_delivered") - g("e_recuperated")) / run_s / 1000.0,
                    "kW",
                );
            }
        }
        if run_s > 0.0 {
            // the share of the run a wheel spent at its grip limit: today
            // counts a step when any driven wheel is there; the wheels'
            // interval means are joined by their largest
            let wheels: Vec<String> =
                of("propulsion.wheel").map(|w| format!("{}.at_grip", inst(&w.0))).collect();
            let idx: Vec<usize> =
                wheels.iter().filter_map(|w| res.names.iter().position(|n| n == w)).collect();
            if !idx.is_empty() {
                let mut s = 0.0;
                for k in 1..res.times.len() {
                    let m = idx.iter().map(|&j| res.mean[j][k]).fold(0.0, f64::max);
                    s += m * (res.times[k] - res.times[k - 1]);
                }
                add(
                    "grip_limit_time_pct".into(),
                    "Time at the tyres' grip limit".into(),
                    100.0 * s / run_s,
                    "%",
                );
            }
        }
    }
    add(
        "simulated_duration_s".into(),
        "Simulated duration".into(),
        res.times.last().copied().unwrap_or(0.0),
        "s",
    );
    out
}

/// The bundled cycle a Driving Task drives in this case, if any.
fn rep_cycle(project: &Value, run: &crate::import::RunSettings, task: &str) -> Option<String> {
    let case = project["cases"]
        .as_array()?
        .iter()
        .find(|c| c["id"].as_str().is_some() && c["id"].as_str() == run.case_id.as_deref())?;
    let from_case = case["parameterOverrides"][task]["cycle"].as_str();
    let el = project["systems"]
        .as_array()?
        .iter()
        .flat_map(|s| s["elements"].as_array().cloned().unwrap_or_default())
        .find(|e| e["id"] == task)?;
    let own = el["parameterOverrides"]["cycle"].as_str();
    from_case.or(own).filter(|c| !c.is_empty() && !c.starts_with("own:")).map(String::from)
}

/// How a figure or channel compares.
#[derive(Clone, Debug)]
pub struct Row {
    /// what (a figure's label, or a channel's name)
    pub what: String,
    /// today's key, for figures
    pub key: String,
    /// its unit
    pub unit: String,
    /// today, at 10 ms
    pub normal: f64,
    /// today, at 1 ms
    pub fine: f64,
    /// the band around today's fine value
    pub band: f64,
    /// the new engine
    pub new: f64,
    /// new − fine (for a channel: the RMS of the difference)
    pub diff: f64,
    /// inside the band
    pub inside: bool,
}

fn is_time(unit: &str) -> bool {
    unit == "s"
}

/// The figures' rows: today's figures that the new engine computes, with
/// their measured bands.
pub fn compare_figures(reference: &Value, new: &[Figure]) -> Vec<Row> {
    let summary = |run: &str| -> Vec<(String, String, f64, String)> {
        reference[run]["summary"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|s| {
                (
                    s["key"].as_str().unwrap_or("").to_string(),
                    s["label"].as_str().unwrap_or("").to_string(),
                    s["value"].as_f64().unwrap_or(f64::NAN),
                    s["unit"].as_str().unwrap_or("").to_string(),
                )
            })
            .collect()
    };
    let (normal, fine) = (summary("normal"), summary("fine"));
    let mut rows = vec![];
    for (key, label, f, unit) in &fine {
        let find = |list: &[(String, String, f64, String)]| {
            list.iter()
                .find(|(k, l, _, _)| if key.is_empty() { l == label } else { k == key })
                .map(|x| x.2)
        };
        let Some(n) = find(&normal) else { continue };
        let Some(fig) =
            new.iter().find(|x| if key.is_empty() { x.label == *label } else { x.key == *key })
        else {
            continue;
        };
        let floor = if is_time(unit) {
            0.005
        } else if unit == "kWh" || unit == "J" {
            0.001
        } else {
            0.005
        };
        let band = (2.0 * (n - f).abs()).max(floor * f.abs()).max(1e-9);
        let diff = fig.value - f;
        rows.push(Row {
            what: label.clone(),
            key: key.clone(),
            unit: unit.clone(),
            normal: n,
            fine: *f,
            band,
            new: fig.value,
            diff,
            inside: diff.abs() <= band,
        });
    }
    rows
}

fn rms(v: impl Iterator<Item = f64>) -> f64 {
    let (mut s, mut n) = (0.0, 0usize);
    for x in v {
        s += x * x;
        n += 1;
    }
    if n == 0 { 0.0 } else { (s / n as f64).sqrt() }
}

/// The channels' rows: every channel of today's run that the new engine
/// records, compared on today's output grid. At an output time that falls
/// on an event the new engine records both sides; today's engine records
/// that point before its step, so it is compared with the new engine's
/// left limit (the value just before the event).
pub fn compare_channels(reference: &Value, run: &CaseRun) -> Vec<Row> {
    let mut rows = vec![];
    let times: Vec<f64> = reference["fine"]["times"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_f64)
        .collect();
    let res = &run.result;
    let Some(fine) = reference["fine"]["channels"].as_object() else { return rows };
    for (name, ch) in fine {
        let Some(c) = run.report.channels.get(name) else { continue };
        let Some(i) = res.names.iter().position(|x| *x == c.var) else { continue };
        let mut newv = res.values[i].clone();
        for l in &res.left_limits {
            if let (Some(x), Some(left)) = (newv.get_mut(l.k), l.get(i)) {
                *x = left;
            }
        }
        let vals = |v: &Value| -> Vec<f64> {
            v["values"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|x| x.as_f64().unwrap_or(f64::NAN))
                .collect()
        };
        let f = vals(ch);
        let n = vals(&reference["normal"]["channels"][name]);
        let m = f.len().min(n.len()).min(newv.len()).min(times.len());
        // the new run's grid is today's (same output times); the first
        // point is left out: today records it before its first solver step,
        // when the values its blocks publish are not set yet (they read 0)
        let pairs = (1..m).filter(|&k| f[k].is_finite() && n[k].is_finite());
        let idx: Vec<usize> = pairs.collect();
        if idx.is_empty() {
            continue;
        }
        let new_disp: Vec<f64> = newv.iter().map(|v| c.unit.from_si(*v)).collect();
        let today_err = rms(idx.iter().map(|&k| n[k] - f[k]));
        let new_err = rms(idx.iter().map(|&k| new_disp[k] - f[k]));
        let scale = rms(idx.iter().map(|&k| f[k]));
        let band = (2.0 * today_err).max(0.005 * scale).max(1e-9);
        rows.push(Row {
            what: name.clone(),
            key: String::new(),
            unit: ch["unit"].as_str().unwrap_or("").to_string(),
            normal: rms(idx.iter().map(|&k| n[k])),
            fine: scale,
            band,
            new: rms(idx.iter().map(|&k| new_disp[k])),
            diff: new_err,
            inside: new_err <= band,
        });
    }
    rows
}

/// Reads a reference file.
pub fn read_reference(path: &Path) -> Result<Value, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// The reference files in a folder, by (project, case).
pub fn references(dir: &Path) -> BTreeMap<(String, String), PathBuf> {
    let mut out = BTreeMap::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        let Some(stem) = p.file_stem().and_then(|s| s.to_str()) else { continue };
        if p.extension().is_none_or(|x| x != "json") {
            continue;
        }
        if let Some((a, b)) = stem.split_once("__") {
            out.insert((a.to_string(), b.to_string()), p.clone());
        }
    }
    out
}
