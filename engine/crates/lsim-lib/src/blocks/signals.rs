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
use crate::table::{Table1, Table2, UnitCarriers, table_param};
use lsim_ir::{ParamValue, TableData};

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

impl ProfileConfig {
    /// The profile's table without its steps (runtime data: `profile`).
    pub fn values(&self, x_unit: &str) -> Result<Vec<(String, ParamValue)>, String> {
        let (cont, _) = self.table.split_steps();
        Ok(vec![("profile".into(), ParamValue::Table(cont.data(x_unit)?))])
    }
}

fn profile_block(
    id: &str,
    base: &str,
    out_port: &str,
    cfg: &ProfileConfig,
    defaults: &ProfileConfig,
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
    let (_, jumps) = cfg.table.split_steps();
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
    // the steps: jumps just past their abscissae (events)
    let mut value = lsim_ir::expr::table("profile", vec![n("x")]);
    for (xj, j) in &jumps {
        let carrier = uc.of(y_unit).map(|u| c(*j) * u).unwrap_or(c(*j));
        value = value
            + ite(
                gt(n("x"), c(*xj) * uc.of(x_unit).expect("unit")),
                carrier,
                c(0.0) * n("profile_zero"),
            );
    }
    let mut params = vec![];
    if !jumps.is_empty() {
        params.push(p("profile_zero", y_unit, 1.0, "1 in the profile's unit"));
    }
    // the default data: the library's profile (a part sets its own)
    let (base_cont, _) = defaults.table.split_steps();
    params.push(table_param(
        "profile",
        y_unit,
        base_cont.data(x_unit).expect("the default profile"),
        "the profile's points (its steps are separate jumps)",
    ));
    let value = if scale {
        params.push(cp(id, "scale_pct"));
        n("scale_pct") * value
    } else {
        value
    };
    eqs.push(eq(n(out_port), value, "it follows its profile"));
    let mut all = uc.into_params();
    all.extend(params);
    // the structure: distance or time, repeat (its period), the steps
    let mut fp = vec![cfg.distance as u8 as f64, cfg.repeat as u8 as f64];
    if cfg.repeat {
        fp.extend([x1, period]);
    }
    for (xj, j) in &jumps {
        fp.extend([*xj, *j]);
    }
    let plain = !cfg.repeat && jumps.is_empty() && cfg.distance == defaults.distance;
    ComponentDef {
        name: if plain {
            base.to_string()
        } else {
            format!("{base}_{}", crate::signal::fingerprint(fp))
        },
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
    profile_block(
        "signal.driving_task",
        "Blocks.DrivingTask",
        "sig_demand",
        cfg,
        &default_task(),
        true,
    )
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
    profile_block(
        "signal.road_profile",
        "Blocks.RoadProfile",
        "sig_grade",
        cfg,
        &default_road(),
        false,
    )
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

/// control.pid: today's PID exactly (`ControlSlave`), a discrete
/// controller run by its host (`pid_host`, `lsim_project::golden::PidHost`)
/// at today's solver step, or at its Sample Time when that is longer, its
/// output held in between. Its inputs and its gains and limits reach the
/// host as the host's inputs, read at each sample (a live edit of a gain
/// takes effect at the next one).
///
/// (It was a PID in continuous time, its anti-windup a mode: at its limit
/// with the error pulling outwards, the frozen integral let the output
/// back inside and the growing one pushed it out again, so the state slid
/// along the mode's boundary and the run stopped as an event storm, where
/// today's PID, which takes one step at a time, does not.)
pub fn pid() -> ComponentDef {
    let id = "control.pid";
    let mut components = vec![sub("ctl", PID_HOST, &[])];
    let mut connections =
        vec![connect("sig_setpoint_in", "ctl.sp"), connect("sig_feedback_in", "ctl.fb")];
    for (g, unit) in pid_gain_units() {
        let k = crate::signal::constant(&unit);
        components.push(sub(&format!("{g}_value"), &k.name, &[("k", n(g))]));
        connections.push(connect(&format!("{g}_value.y"), &format!("ctl.{g}")));
    }
    connections.push(connect("ctl.out", "sig_out"));
    ComponentDef {
        name: "Blocks.Pid".into(),
        doc: doc(id),
        ports: vec![inp(id, "sig_setpoint_in"), inp(id, "sig_feedback_in"), out(id, "sig_out")],
        params: cps(id, &["kp", "ki", "kd", "out_min", "out_max", "sample_time_s"]),
        components,
        connections,
        ..Default::default()
    }
}

/// The PID's gains and limits, in the order its host reads them after the
/// setpoint and the feedback.
pub const PID_GAINS: [&str; 5] = ["kp", "ki", "kd", "out_min", "out_max"];

/// Each gain's and limit's unit (its catalog parameter's).
pub fn pid_gain_units() -> Vec<(&'static str, String)> {
    PID_GAINS.into_iter().zip(cps("control.pid", &PID_GAINS)).map(|(g, p)| (g, p.unit)).collect()
}

/// The constants that carry the PID's gains and limits to its host.
pub fn pid_gain_constants() -> Vec<ComponentDef> {
    let mut units: Vec<String> = pid_gain_units().into_iter().map(|(_, u)| u).collect();
    units.sort();
    units.dedup();
    units.iter().map(|u| crate::signal::constant(u)).collect()
}

/// The PID's host block's definition.
pub const PID_HOST: &str = "External.Pid";

/// The sampled part of control.pid: its host runs today's PID law.
pub fn pid_host() -> ComponentDef {
    let mut ports = vec![input("sp", "1", "the setpoint"), input("fb", "1", "the feedback")];
    for (k, unit) in pid_gain_units() {
        ports.push(input(k, &unit, "a gain or limit"));
    }
    ports.push(output("out", "1", "the output, held between samples"));
    ComponentDef {
        name: PID_HOST.into(),
        doc: "today's PID law, run by its host at each sample".into(),
        ports,
        params: vec![p("period", "s", 0.01, "its tick spacing (the host's own: today's step)")],
        ..Default::default()
    }
}

/// The sampled part of a Lookup with a Sample Time longer than today's
/// solver step: its host takes the inputs at each sample and holds them.
pub fn hold_host(two: bool) -> ComponentDef {
    let mut ports = vec![input("x_in", "1", "Input X")];
    if two {
        ports.push(input("y_in", "1", "Input Y"));
    }
    ports.push(output("x_out", "1", "Input X as last sampled"));
    if two {
        ports.push(output("y_out", "1", "Input Y as last sampled"));
    }
    ComponentDef {
        name: if two { "External.Hold2" } else { "External.Hold1" }.into(),
        doc: "its inputs, taken at each sample and held in between".into(),
        ports,
        params: vec![p("period", "s", 0.01, "its tick spacing (the host's own)")],
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

impl LookupConfig {
    /// Its table as runtime data (`table`).
    pub fn values(&self) -> Result<Vec<(String, ParamValue)>, String> {
        Ok(vec![(
            "table".into(),
            ParamValue::Table(match self {
                LookupConfig::D1(t) => t.data("1")?,
                LookupConfig::D2(t) => t.grid_data(["1", "1"])?,
            }),
        )])
    }
}

/// signal.lookup: its table at its input(s), in the numbers of what it is
/// wired to.
pub fn lookup(cfg: &LookupConfig) -> ComponentDef {
    let id = "signal.lookup";
    let (ports, value, name, data) = match cfg {
        LookupConfig::D1(_) => (
            vec![inp(id, "sig_x_in"), out(id, "sig_out")],
            lsim_ir::expr::table("table", vec![n("sig_x_in")]),
            "Blocks.Lookup",
            default_lookup(),
        ),
        LookupConfig::D2(_) => (
            vec![inp(id, "sig_x_in"), inp(id, "sig_y_in"), out(id, "sig_out")],
            lsim_ir::expr::table("table", vec![n("sig_x_in"), n("sig_y_in")]),
            "Blocks.Lookup2D",
            {
                let p = &catalog::param(id, "table_2d").expect("table_2d")["default"];
                LookupConfig::D2(
                    Table2::from_json(p, Default::default(), Default::default()).expect("table"),
                )
            },
        ),
    };
    let data = match data.values().expect("the default table").remove(0).1 {
        ParamValue::Table(t) => t,
        _ => unreachable!(),
    };
    ComponentDef {
        name: name.into(),
        doc: doc(id),
        ports,
        params: vec![table_param("table", "1", data, "the table")],
        equations: vec![eq(n("sig_out"), value, "it reads its table")],
        ..Default::default()
    }
}

/// signal.lookup with a Sample Time longer than today's solver step: as
/// today's engine, its table read at its inputs as they were at its last
/// sample (its host, `hold_host`, takes them at the instants today's
/// engine samples at and holds them), so its output holds between
/// samples, and the table's watch and its records are at the point today
/// books (its held point).
pub fn lookup_held(cfg: &LookupConfig) -> ComponentDef {
    let two = matches!(cfg, LookupConfig::D2(_));
    let host = hold_host(two);
    let mut d = lookup(cfg);
    d.name = format!("{}Held", d.name);
    d.connections = vec![connect("sig_x_in", "hold.x_in")];
    let mut at = vec![n("hold.x_out")];
    if two {
        d.connections.push(connect("sig_y_in", "hold.y_in"));
        at.push(n("hold.y_out"));
    }
    d.equations = vec![eq(
        n("sig_out"),
        lsim_ir::expr::table("table", at),
        "it reads its table at its inputs as last sampled",
    )];
    d.components = vec![sub("hold", &host.name, &[])];
    d
}

/// The default lookup (1D).
pub fn default_lookup() -> LookupConfig {
    let p = &catalog::param("signal.lookup", "table_1d").expect("table_1d")["default"];
    LookupConfig::D1(Table1::from_json(p, Default::default()).expect("table"))
}

/// signal.map: its inputs (the axes of its maps, in order) and outputs,
/// each with its SI unit, and each output's map: a table with one axis per
/// input (SI), or its value when the block has no input.
#[derive(Clone, Debug, PartialEq)]
pub struct MapConfig {
    /// (port id, SI unit) of each input, in order
    pub inputs: Vec<(String, String)>,
    /// (port id, SI unit) of each output
    pub outputs: Vec<(String, String)>,
    /// each output's map: [`ParamValue::Table`], or [`ParamValue::Real`]
    /// with no input
    pub maps: Vec<ParamValue>,
}

impl MapConfig {
    /// The parameter that holds output `k`'s map: named after the output
    /// (`loss_map`), so that a message about it says which, and never a
    /// port's name.
    pub fn param(&self, k: usize) -> String {
        let taken = |n: &str| self.inputs.iter().chain(&self.outputs).any(|(id, _)| id == n);
        let mut name = format!("{}_map", self.outputs[k].0);
        while taken(&name) {
            name.push('_');
        }
        name
    }

    /// Its maps as the part's values.
    pub fn values(&self) -> Vec<(String, ParamValue)> {
        (0..self.outputs.len()).map(|k| (self.param(k), self.maps[k].clone())).collect()
    }
}

/// The default Map: one input and one output, its map a line through (0,
/// 0) and (1, 1).
pub fn default_map() -> MapConfig {
    let one = |id: &str| (id.to_string(), "1".to_string());
    let line = TableData {
        interpolation: crate::table::Interpolation::Linear,
        ..TableData::new_1d(vec![0.0, 1.0], vec![0.0, 1.0]).with_units(&["1"])
    };
    MapConfig {
        inputs: vec![one("input_1")],
        outputs: vec![one("output_1")],
        maps: vec![ParamValue::Table(line)],
    }
}

/// signal.map: each output its map read at the inputs, every step (a
/// smooth signal function of them: no state, no sample time). Its ports
/// are its own (`dynamicPorts`), so its definition is named with a
/// fingerprint of them (`Blocks.Map` for [`default_map`]'s).
pub fn map(cfg: &MapConfig) -> ComponentDef {
    let mut ports = vec![];
    for (id, u) in &cfg.inputs {
        ports.push(crate::x::input(id, u, "an input: an axis of the maps"));
    }
    for (id, u) in &cfg.outputs {
        ports.push(crate::x::output(id, u, "an output: its map at the inputs"));
    }
    let at: Vec<Expr> = cfg.inputs.iter().map(|(id, _)| n(id)).collect();
    let mut params = vec![];
    let mut equations = vec![];
    for (k, (id, u)) in cfg.outputs.iter().enumerate() {
        let name = cfg.param(k);
        let (decl, value) = match &cfg.maps[k] {
            ParamValue::Table(t) => (
                table_param(&name, u, t.clone(), &format!("the map of output {id}")),
                lsim_ir::expr::table(&name, at.clone()),
            ),
            other => {
                let v = match other {
                    ParamValue::Real(Expr::Const(v)) => *v,
                    _ => 0.0,
                };
                (p(&name, u, v, &format!("the value of output {id} (no input)")), n(&name))
            }
        };
        params.push(decl);
        equations.push(eq(n(id), value, &format!("output {id}: its map at the inputs")));
    }
    let shape: Vec<f64> = cfg
        .inputs
        .iter()
        .chain(&cfg.outputs)
        .flat_map(|(a, b)| a.bytes().chain(b.bytes()).map(f64::from).chain([-1.0]))
        .chain([cfg.inputs.len() as f64])
        .chain(cfg.maps.iter().map(|m| f64::from(u8::from(matches!(m, ParamValue::Table(_))))))
        .collect();
    let base = default_map();
    let same = cfg.inputs == base.inputs && cfg.outputs == base.outputs;
    ComponentDef {
        name: if same {
            "Blocks.Map".into()
        } else {
            format!("Blocks.Map_{}", crate::signal::fingerprint(shape))
        },
        doc: doc("signal.map"),
        ports,
        params,
        equations,
        ..Default::default()
    }
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
/// Traction Control): a sampled external block (`External.*`): no
/// equations; its outputs are discrete variables the host sets at each
/// tick and holds in between; its inputs are read then; its `period` is
/// its Sample Time, or 10 ms (today's solver step) when that is 0.
pub fn sampled(base: &str, id: &str, ports: &SampledPorts) -> ComponentDef {
    let mut decl = vec![];
    for (pid, u) in &ports.inputs {
        decl.push(input(pid, u, "an input, read at each tick"));
    }
    for (pid, u) in &ports.outputs {
        decl.push(output(pid, u, "an output, held between ticks"));
    }
    let fp: Vec<f64> = ports
        .inputs
        .iter()
        .chain(&ports.outputs)
        .flat_map(|(a, b)| a.bytes().chain(b.bytes()).map(|x| x as f64).chain([-1.0]))
        .collect();
    let mut params =
        vec![p("default_period", "s", 0.01, "the period with no Sample Time: today's solver step")];
    if catalog::param(id, "sample_time_s").is_some() {
        params.push(cp(id, "sample_time_s"));
        params.push(pe(
            "period",
            "s",
            ite(gt(n("sample_time_s"), c(0.0)), n("sample_time_s"), n("default_period")),
            "its tick spacing",
        ));
    } else {
        params.push(pe("period", "s", n("default_period"), "its tick spacing"));
    }
    ComponentDef {
        name: format!("External.{base}_{}", crate::signal::fingerprint(fp)),
        doc: doc(id),
        ports: decl,
        params,
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

/// track.lap in a lap case's model: the curvature and grade at the point
/// of the lap being solved (`kappa_now`, `grade_now`, set by the lap
/// solver for each point, lsim-lap) for the Vehicle (`kappa`, `grade`),
/// and its curvature output; its other outputs read 0 (the lap's channels
/// are the lap solver's).
pub fn race_track_lap() -> ComponentDef {
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
    let mut ports: Vec<PortDecl> = outs.iter().map(|q| out(id, q)).collect();
    ports.push(output("kappa", "1/m", "the curvature here (for the Vehicle)"));
    ports.push(output("grade", "1", "the grade here (for the Vehicle)"));
    let mut equations: Vec<lsim_ir::EquationDecl> = outs
        .iter()
        .map(|q| {
            if *q == "sig_curvature" {
                return eq(n(q), n("kappa_now"), "the curvature here");
            }
            let u = port_si(id, q);
            let unit_expr =
                if u == "1" { n("zero") } else { n("zero") * n(&format!("unit_{}", ident(u))) };
            eq(n(q), unit_expr, "the lap solver's channel")
        })
        .collect();
    equations.push(eq(n("kappa"), n("kappa_now"), "the curvature here"));
    equations.push(eq(n("grade"), n("grade_now"), "the grade here"));
    ComponentDef {
        name: "Blocks.RaceTrack_lap".into(),
        doc: doc(id),
        ports,
        params: vec![
            p("zero", "1", 0.0, "the value its other outputs read"),
            p("kappa_now", "1/m", 0.0, "the curvature at the point being solved (+ left)"),
            p("grade_now", "1", 0.0, "the grade (rise over run) at the point being solved"),
        ],
        equations,
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
    let mut script = sampled("Script", "signal.script", &SampledPorts::default());
    script.name = "External.Script".into();
    let mut fmu = sampled("Fmu", "signal.fmu", &SampledPorts::default());
    fmu.name = "External.Fmu".into();
    let tc_ports = SampledPorts {
        inputs: ["sig_demand_in", "sig_slip_in", "sig_slip2_in", "sig_speed_in"]
            .iter()
            .map(|p| (p.to_string(), "1".to_string()))
            .collect(),
        outputs: vec![("sig_out".into(), "1".into())],
    };
    let mut tc = sampled("TractionControl", "control.traction", &tc_ports);
    tc.name = "External.TractionControl".into();
    let mut tc_params = cps(
        "control.traction",
        &["target_slip", "kp", "ki", "launch_ramp_s", "launch_torque_pct", "min_speed_kmh"],
    );
    tc_params.extend(tc.params);
    tc.params = tc_params;
    let (held1, held2) = (lookup_held(&default_lookup()), {
        let p = &catalog::param("signal.lookup", "table_2d").expect("table_2d")["default"];
        lookup_held(&LookupConfig::D2(
            Table2::from_json(p, Default::default(), Default::default()).expect("table"),
        ))
    });
    let mut out = vec![
        task,
        road,
        constant(),
        pid(),
        pid_host(),
        look,
        held1,
        held2,
        hold_host(false),
        hold_host(true),
        script,
        fmu,
        tc,
        monitor(&[]),
        system(),
        race_track(),
        map(&default_map()),
    ];
    out.extend(pid_gain_constants());
    out
}
