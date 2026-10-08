//! Signal blocks: driving task, road profile, constant, PID, lookup, and
//! the sampled ones (Script, FMU, Traction Control), monitor, sub-system
//! container and race track.
//!
//! Blocks whose ports have no unit (Constant, PID, Lookup, Script, FMU,
//! Traction Control) work, as today, in the numbers of whatever they are
//! wired to, in the app's display units: the importer converts at each
//! link between a port with a unit and one without (a vehicle speed in m/s
//! reaches a Script as km/h, as today).

use super::*;
use crate::table::{Table1, Table2, UnitCarriers};

/// A profile block's configuration (Driving Task, Road Profile).
#[derive(Clone, Debug, PartialEq)]
pub struct ProfileConfig {
    /// the points: x (s or m) → value (SI: m/s, or grade as a fraction)
    pub table: Table1,
    /// x is the distance driven (an input), else the time
    pub distance: bool,
    /// it starts again after its last point
    pub repeat: bool,
}

fn profile_block(
    id: &str,
    base: &str,
    out_port: &str,
    cfg: &ProfileConfig,
    scale: bool,
) -> ComponentDef {
    let mut uc = UnitCarriers::default();
    let x_unit = if cfg.distance { "m" } else { "s" };
    let y_unit = port_si(id, out_port);
    let mut ports = vec![];
    if cfg.distance {
        ports.push(input("x_in", "m", "the distance the vehicle has driven"));
    }
    ports.push(out(id, out_port));
    let x = if cfg.distance { n("x_in") } else { time() };
    let (x0, x1) = cfg.table.range();
    let period = x1 - x0;
    let mut vars = vec![var("x", x_unit, "where it reads its profile")];
    let mut eqs = vec![];
    if cfg.repeat && period > 0.0 {
        vars.push(discrete("offset", x_unit, 0.0, "how many whole passes are behind"));
        eqs.push(eq(n("x"), x.clone() - n("offset"), "the position within this pass"));
        eqs.push(when(
            ge(x - n("offset"), c(x1) * uc.of(x_unit).expect("unit")),
            &[("offset", pre("offset") + c(period) * uc.of(x_unit).expect("unit"))],
            "after its last point it starts again from its first",
        ));
    } else {
        eqs.push(eq(n("x"), x, "where it reads its profile"));
    }
    let value = uc.t1(&cfg.table, n("x"), x_unit, y_unit);
    let mut params = vec![];
    let value = if scale {
        params.push(cp(id, "scale_pct"));
        n("scale_pct") * value
    } else {
        value
    };
    eqs.push(eq(n(out_port), value, "it follows its profile"));
    let mut all = uc.into_params();
    all.extend(params);
    let mut fp = vec![cfg.distance as u8 as f64, cfg.repeat as u8 as f64];
    fp.extend(&cfg.table.x);
    fp.extend(&cfg.table.y);
    ComponentDef {
        name: format!("{base}_{}", crate::signal::fingerprint(fp)),
        doc: doc(id),
        ports,
        params: all,
        vars,
        equations: eqs,
        ..Default::default()
    }
}

/// signal.driving_task: the target speed against time or distance.
pub fn driving_task(cfg: &ProfileConfig) -> ComponentDef {
    profile_block("signal.driving_task", "Blocks.DrivingTask", "sig_demand", cfg, true)
}

/// The Driving Task's default profile.
pub fn default_task() -> ProfileConfig {
    let text = catalog::param("signal.driving_task", "profile").expect("profile")["default"]
        .as_str()
        .unwrap_or("");
    ProfileConfig {
        table: Table1::from_profile(text).scaled(1.0, 1.0 / 3.6),
        distance: false,
        repeat: false,
    }
}

/// signal.road_profile: the road's grade against distance or time.
pub fn road_profile(cfg: &ProfileConfig) -> ComponentDef {
    profile_block("signal.road_profile", "Blocks.RoadProfile", "sig_grade", cfg, false)
}

/// The Road Profile's default.
pub fn default_road() -> ProfileConfig {
    let text = catalog::param("signal.road_profile", "profile").expect("profile")["default"]
        .as_str()
        .unwrap_or("");
    ProfileConfig {
        table: Table1::from_profile(text).scaled(1.0, 0.01),
        distance: true,
        repeat: false,
    }
}

/// signal.constant: its value, in the numbers of what it feeds.
pub fn constant() -> ComponentDef {
    let id = "signal.constant";
    ComponentDef {
        name: "Blocks.Constant".into(),
        doc: doc(id),
        ports: vec![out(id, "sig_out")],
        params: cps(id, &["value"]),
        equations: vec![eq(n("sig_out"), n("value"), "its value")],
        ..Default::default()
    }
}

/// control.pid in continuous time: today's PID with its anti-windup (the
/// integral grows only while the output is inside its limits or the error
/// pulls it back); the derivative of the error through a first-order
/// filter with the controller's period as its time constant (today's
/// difference over one 10 ms step, or over its Sample Time).
pub fn pid() -> ComponentDef {
    let id = "control.pid";
    let mut params = cps(id, &["kp", "ki", "kd", "out_min", "out_max", "sample_time_s"]);
    params.push(pe(
        "t_f",
        "s",
        max(n("sample_time_s"), c(0.01) * n("unit_s")),
        "the derivative's filter time",
    ));
    params.insert(0, p("unit_s", "s", 1.0, "unit carrier"));
    ComponentDef {
        name: "Blocks.Pid".into(),
        doc: doc(id),
        ports: vec![inp(id, "sig_setpoint_in"), inp(id, "sig_feedback_in"), out(id, "sig_out")],
        params,
        vars: vec![
            state("I", "s", 0.0, "the error's integral"),
            state("xd", "1", 0.0, "the error, lagged by the derivative's filter"),
            var("err", "1", "setpoint − feedback"),
            var("deriv", "1/s", "the error's rate"),
            var("u", "1", "the output before its limits"),
        ],
        equations: vec![
            eq(n("err"), n("sig_setpoint_in") - n("sig_feedback_in"), "the error"),
            eq(n("t_f") * der("xd"), n("err") - n("xd"), "the derivative's filter"),
            eq(n("deriv"), (n("err") - n("xd")) / n("t_f"), "the error's rate"),
            eq(n("u"), n("kp") * n("err") + n("ki") * n("I") + n("kd") * n("deriv"), "P + I + D"),
            eq(
                der("I"),
                ite(
                    or(
                        and(ge(n("u"), n("out_min")), le(n("u"), n("out_max"))),
                        lt(n("err") * n("u"), c(0.0)),
                    ),
                    n("err"),
                    c(0.0),
                ),
                "the integral grows inside the limits or when the error pulls back",
            ),
            eq(n("sig_out"), clamp(n("u"), n("out_min"), n("out_max")), "held to its limits"),
        ],
        ..Default::default()
    }
}

/// A lookup block's configuration.
#[derive(Clone, Debug, PartialEq)]
pub enum LookupConfig {
    /// 1D: the table read at Input X
    D1(Table1),
    /// 2D: the table read at Input X and Input Y
    D2(Table2),
}

/// signal.lookup: its table at its input(s).
pub fn lookup(cfg: &LookupConfig) -> ComponentDef {
    let id = "signal.lookup";
    let mut uc = UnitCarriers::default();
    let (ports, value, fp) = match cfg {
        LookupConfig::D1(t) => (
            vec![inp(id, "sig_x_in"), out(id, "sig_out")],
            uc.t1(t, n("sig_x_in"), "1", "1"),
            crate::signal::t1_fingerprint(t),
        ),
        LookupConfig::D2(t) => (
            vec![inp(id, "sig_x_in"), inp(id, "sig_y_in"), out(id, "sig_out")],
            uc.t2(t, n("sig_x_in"), "1", n("sig_y_in"), "1", "1"),
            crate::signal::t2_fingerprint(t),
        ),
    };
    ComponentDef {
        name: format!("Blocks.Lookup_{fp}"),
        doc: doc(id),
        ports,
        params: uc.into_params(),
        equations: vec![eq(n("sig_out"), value, "it reads its table")],
        ..Default::default()
    }
}

/// The default lookup (1D).
pub fn default_lookup() -> LookupConfig {
    let p = &catalog::param("signal.lookup", "table_1d").expect("table_1d")["default"];
    LookupConfig::D1(Table1::from_json(p, Default::default()).expect("table"))
}

/// A sampled block's ports: its inputs and outputs with their SI units.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct SampledPorts {
    /// (port id, SI unit)
    pub inputs: Vec<(String, String)>,
    /// (port id, SI unit)
    pub outputs: Vec<(String, String)>,
}

/// A block run outside the equations on its own clock (Script, FMU,
/// Traction Control): its outputs are discrete variables `d_<port>` the
/// host sets at each tick and holds in between; its inputs are read then.
pub fn sampled(base: &str, id: &str, ports: &SampledPorts) -> ComponentDef {
    let mut decl = vec![];
    let mut vars = vec![];
    let mut eqs = vec![];
    for (pid, u) in &ports.inputs {
        decl.push(input(pid, u, "an input"));
    }
    for (pid, u) in &ports.outputs {
        decl.push(output(pid, u, "an output, held between ticks"));
        vars.push(discrete(&format!("d_{pid}"), u, 0.0, "the held output"));
        eqs.push(eq(n(pid), n(&format!("d_{pid}")), "it holds what its last tick gave"));
    }
    let fp: Vec<f64> = ports
        .inputs
        .iter()
        .chain(&ports.outputs)
        .flat_map(|(a, b)| a.bytes().chain(b.bytes()).map(|x| x as f64).chain([-1.0]))
        .collect();
    let mut params = vec![];
    if catalog::param(id, "sample_time_s").is_some() {
        params.push(cp(id, "sample_time_s"));
    }
    ComponentDef {
        name: format!("{base}_{}", crate::signal::fingerprint(fp)),
        doc: doc(id),
        ports: decl,
        params,
        vars,
        equations: eqs,
        ..Default::default()
    }
}

/// signal.monitor: display only; its inputs are read as channels.
pub fn monitor(inputs: &[(String, String)]) -> ComponentDef {
    let fp: Vec<f64> = inputs
        .iter()
        .flat_map(|(a, b)| a.bytes().chain(b.bytes()).map(|x| x as f64).chain([-1.0]))
        .collect();
    ComponentDef {
        name: if inputs.is_empty() {
            "Blocks.Monitor".into()
        } else {
            format!("Blocks.Monitor_{}", crate::signal::fingerprint(fp))
        },
        doc: doc("signal.monitor"),
        ports: inputs.iter().map(|(p, u)| input(p, u, "a monitored signal")).collect(),
        ..Default::default()
    }
}

/// container.system: no physics (its children are imported in its place).
pub fn system() -> ComponentDef {
    ComponentDef {
        name: "Blocks.System".into(),
        doc: doc("container.system"),
        ..Default::default()
    }
}

/// track.lap: lap cases run in today's lap-mode solver (quasi-steady, not
/// a time simulation); in a time-domain case the track has no physics and
/// its outputs read 0, as today's unrecorded ones do.
pub fn race_track() -> ComponentDef {
    let id = "track.lap";
    let outs = [
        "sig_lap_distance",
        "sig_lap",
        "sig_curvature",
        "sig_long_accel",
        "sig_lat_accel",
        "sig_limit",
        "sig_x",
        "sig_y",
        "sig_elevation",
    ];
    ComponentDef {
        name: "Blocks.RaceTrack".into(),
        doc: doc(id),
        ports: outs.iter().map(|q| out(id, q)).collect(),
        params: vec![p("zero", "1", 0.0, "the value its outputs read outside lap cases")],
        equations: outs
            .iter()
            .map(|q| {
                let u = port_si(id, q);
                let unit_expr =
                    if u == "1" { n("zero") } else { n("zero") * n(&format!("unit_{}", ident(u))) };
                eq(n(q), unit_expr, "no lap case: 0")
            })
            .collect(),
        ..Default::default()
    }
    .with_carriers(&outs.iter().map(|q| port_si(id, q)).collect::<Vec<_>>())
}

trait Carriers {
    fn with_carriers(self, units: &[&str]) -> Self;
}

impl Carriers for ComponentDef {
    fn with_carriers(mut self, units: &[&str]) -> Self {
        let mut uc = UnitCarriers::default();
        for u in units {
            uc.of(u);
        }
        let mut ps = uc.into_params();
        ps.extend(self.params);
        self.params = ps;
        self
    }
}

/// The signal blocks in their default configuration.
pub fn defaults() -> Vec<ComponentDef> {
    let mut task = driving_task(&default_task());
    task.name = "Blocks.DrivingTask".into();
    let mut road = road_profile(&default_road());
    road.name = "Blocks.RoadProfile".into();
    let mut look = lookup(&default_lookup());
    look.name = "Blocks.Lookup".into();
    let mut script = sampled("Blocks.Script", "signal.script", &SampledPorts::default());
    script.name = "Blocks.Script".into();
    let mut fmu = sampled("Blocks.Fmu", "signal.fmu", &SampledPorts::default());
    fmu.name = "Blocks.Fmu".into();
    let tc_ports = SampledPorts {
        inputs: ["sig_demand_in", "sig_slip_in", "sig_slip2_in", "sig_speed_in"]
            .iter()
            .map(|p| (p.to_string(), "1".to_string()))
            .collect(),
        outputs: vec![("sig_out".into(), "1".into())],
    };
    let mut tc = sampled("Blocks.TractionControl", "control.traction", &tc_ports);
    tc.name = "Blocks.TractionControl".into();
    tc.params = cps(
        "control.traction",
        &[
            "target_slip",
            "kp",
            "ki",
            "launch_ramp_s",
            "launch_torque_pct",
            "min_speed_kmh",
            "sample_time_s",
        ],
    );
    vec![task, road, constant(), pid(), look, script, fmu, tc, monitor(&[]), system(), race_track()]
}
