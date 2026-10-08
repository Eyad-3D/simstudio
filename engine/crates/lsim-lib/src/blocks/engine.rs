//! Combustion engine, fuel tank, hydrogen tank, fuel cell stack.
//!
//! Fuel flows through fuel ports (`e`, the heating value, × mass flow), so
//! an engine's or a stack's books see the chemical power it burns: stored
//! fuel energy in the tank, fuel power in, shaft or electrical power out,
//! the rest lost. Without a tank the importer gives the engine an
//! inexhaustible supply at the default heating value (today's "running on
//! infinite fuel").

use super::*;
use crate::table::{Outside, Table1, Table2, UnitCarriers};
use lsim_ir::EnergyDecl;

const ENG: &str = "engine.combustion";
const RPM: f64 = std::f64::consts::PI / 30.0;
/// hydrogen's lower heating value, J/kg (today's H2_LHV_J_PER_KG)
pub const H2_LHV: f64 = 119.96e6;
/// petrol's lower heating value, J/kg (today's FUEL_LHV_MJ)
pub const FUEL_LHV: f64 = 42.9e6;

/// The engine's configuration.
#[derive(Clone, Debug, PartialEq)]
pub struct EngineConfig {
    /// full-load torque, N·m, by speed, rad/s (scaled)
    pub full_load: Table1,
    /// drag torque while not fired, N·m, by speed, rad/s (scaled)
    pub drag: Table1,
    /// fuel flow, kg/s, by speed (rad/s, outer) and torque (N·m)
    pub fuel_map: Table2,
    /// Engine Scale (today's MOD-47 factor)
    pub scale: f64,
    /// the Throttle input is wired (else 0)
    pub throttle_wired: bool,
    /// the Enable input is wired (else always on)
    pub enable_wired: bool,
    /// a Fuel Tank feeds it (it stops when the tank is empty)
    pub tank: bool,
}

impl EngineConfig {
    /// From today's merged parameters and the per-table axis settings.
    pub fn from_params(
        p: &serde_json::Map<String, serde_json::Value>,
        outside: &dyn Fn(&str, usize) -> Outside,
        throttle_wired: bool,
        enable_wired: bool,
        tank: bool,
    ) -> Result<Self, String> {
        let k = {
            let v = catalog::get(p, "engine_scale_pct", 100.0);
            if v > 0.0 { (v / 100.0).max(0.01) } else { 1.0 }
        };
        let fl = Table1::from_json(
            p.get("full_load_torque").ok_or("no full_load_torque")?,
            outside("full_load_torque", 0),
        )?;
        let drag = Table1::from_json(
            p.get("drag_torque").ok_or("no drag_torque")?,
            outside("drag_torque", 0),
        )?;
        let fuel = Table2::from_json(
            p.get("fuel_map").ok_or("no fuel_map")?,
            outside("fuel_map", 0),
            outside("fuel_map", 1),
        )?;
        Ok(EngineConfig {
            full_load: fl.scaled(RPM, k),
            drag: drag.scaled(RPM, k),
            fuel_map: fuel.scaled(RPM, k, k / 3600.0),
            scale: k,
            throttle_wired,
            enable_wired,
            tank,
        })
    }
}

impl Default for EngineConfig {
    fn default() -> Self {
        let p = catalog::merged(ENG, &serde_json::Map::new());
        EngineConfig::from_params(
            &p,
            &|key, axis| catalog::axis_outside(ENG, key, axis),
            true,
            false,
            true,
        )
        .expect("default maps")
    }
}

/// engine.combustion: today's rules exactly (`engine_torque`): fired, it
/// gives throttle × full-load torque (the idle governor adds torque below
/// idle), burning the fuel map at that torque; at zero throttle below the
/// re-entry speed the governor holds idle, trimming the fuel down to the
/// drag torque along a Willans line; not fired (switched off, out of fuel,
/// in overrun fuel cut-off, above the rev limit) it drags and burns
/// nothing. Below the full-load curve's first speed it uses that point
/// (the start-up rule).
pub fn engine(cfg: &EngineConfig) -> ComponentDef {
    let id = ENG;
    let mut uc = UnitCarriers::default();
    let n0 = cfg.full_load.x.first().copied().unwrap_or(0.0);
    let n_top = cfg.full_load.x.last().copied().unwrap_or(0.0);
    uc.of("rad/s");
    uc.of("N.m");
    uc.of("kg/s");
    let mut params = cps(id, &["idle_speed_rpm", "fuel_cut_reentry_rpm", "inertia_kgm2"]);
    params.push(p("w0", "rad/s", 0.0, "its speed at the start"));
    params.push(pe(
        "J",
        "kg.m2",
        n("inertia_kgm2") * c(cfg.scale),
        "inertia, as the Engine Scale makes it",
    ));
    params.push(pe(
        "idle",
        "rad/s",
        max(n("idle_speed_rpm"), c(RPM) * n("unit_rad_s")),
        "idle speed, at least 1 1/min",
    ));
    let mut ports = vec![
        phys(id, "shaft", "Flange"),
        lsim_ir::component::build::port("fuel", "FuelPort", "its fuel line (the Fuel Tank)"),
    ];
    if cfg.throttle_wired {
        ports.push(inp(id, "sig_throttle_in"));
    }
    if cfg.enable_wired {
        ports.push(inp(id, "sig_on_in"));
    }
    if cfg.tank {
        ports.push(input("fuel_mass", "kg", "the fuel left in its tank"));
    }
    for q in
        ["sig_speed", "sig_torque", "sig_fuel_rate", "sig_power", "sig_fuel_power", "sig_losses"]
    {
        ports.push(out(id, q));
    }
    let mut w = state("w", "rad/s", 0.0, "speed");
    w.start = Some(n("w0"));
    let mut vars = vec![
        w,
        var("wa", "rad/s", "speed magnitude"),
        var("thr", "1", "throttle, 0-1"),
        var("on", "1", "1 while switched on and fuelled"),
        var("n_map", "rad/s", "the speed the maps are read at"),
        var("gov", "1", "the idle governor's demand"),
        var("t_full", "N.m", "full-load torque"),
        var("t_drag", "N.m", "drag torque"),
        var("t_b1", "N.m", "torque, fired with throttle"),
        var("t_b2", "N.m", "torque at idle"),
        var("fired_thr", "1", "fired with throttle"),
        var("fired_idle", "1", "fired by the idle governor"),
        var("T", "N.m", "shaft torque"),
        var("fuel_rate", "kg/s", "fuel flow"),
        state("fuel_used", "kg", 0.0, "fuel used"),
        state("work_fired", "J", 0.0, "work given while fired"),
    ];
    for v in vars.iter_mut().skip(14) {
        v.nominal = Some(1.0);
    }
    let thr = if cfg.throttle_wired { clamp(n("sig_throttle_in"), c(0.0), c(1.0)) } else { c(0.0) };
    let mut on = if cfg.enable_wired { ge(n("sig_on_in"), c(0.5)) } else { ge(c(1.0), c(0.0)) };
    if cfg.tank {
        on = and(on, gt(n("fuel_mass"), c(0.0)));
    }
    let below_top = le(n("wa"), (c(n_top) + c(1e-9 * RPM)) * n("unit_rad_s"));
    let fm = |uc: &mut UnitCarriers, t: Expr| {
        uc.t2(&cfg.fuel_map, n("n_map"), "rad/s", t, "N.m", "kg/s")
    };
    let fuel_thr = fm(&mut uc, n("t_b1"));
    let fuel_b2 = fm(&mut uc, n("t_b2"));
    let fuel_0 = fm(&mut uc, c(0.0) * n("unit_N_m"));
    let t_full = uc.t1(&cfg.full_load, n("n_map"), "rad/s", "N.m");
    let t_drag = uc.t1(&cfg.drag, n("wa"), "rad/s", "N.m");
    let eqs = vec![
        eq(n("w"), n("shaft.w"), "it turns with the shaft"),
        eq(n("J") * der("w"), n("T") + n("shaft.tau"), "its inertia"),
        eq(n("wa"), abs(n("w")), "speed magnitude"),
        eq(n("thr"), thr, "throttle"),
        eq(n("on"), ite(on, c(1.0), c(0.0)), "switched on and fuelled"),
        eq(
            n("n_map"),
            clamp(n("wa"), c(n0) * n("unit_rad_s"), c(n_top) * n("unit_rad_s")),
            "the maps are read between the full-load curve's first and last speed",
        ),
        eq(n("gov"), (n("idle") - n("wa")) / (c(0.25) * n("idle")), "the idle governor's demand"),
        eq(n("t_full"), t_full, "full-load torque"),
        eq(n("t_drag"), t_drag, "drag torque"),
        eq(
            n("t_b1"),
            max(n("thr"), min(c(1.0), n("gov"))) * n("t_full"),
            "fired: throttle (or the governor) × full load",
        ),
        eq(
            n("t_b2"),
            max(-n("t_drag"), min(n("t_full"), n("gov") * n("t_full"))),
            "at zero throttle the governor holds idle",
        ),
        eq(
            n("fired_thr"),
            ite(
                and(gt(n("on"), c(0.5)), and(below_top.clone(), gt(n("thr"), c(0.0)))),
                c(1.0),
                c(0.0),
            ),
            "fired with throttle",
        ),
        eq(
            n("fired_idle"),
            ite(
                and(
                    gt(n("on"), c(0.5)),
                    and(
                        below_top,
                        and(le(n("thr"), c(0.0)), le(n("wa"), n("fuel_cut_reentry_rpm"))),
                    ),
                ),
                c(1.0),
                c(0.0),
            ),
            "fired by the governor below the re-entry speed (above it: fuel cut)",
        ),
        eq(
            n("T"),
            ite(
                gt(n("fired_thr"), c(0.5)),
                n("t_b1"),
                ite(gt(n("fired_idle"), c(0.5)), n("t_b2"), -(sign(n("w")) * n("t_drag"))),
            ),
            "fired, its torque; not fired, its drag against the motion",
        ),
        eq(
            n("fuel_rate"),
            ite(
                gt(n("fired_thr"), c(0.5)),
                fuel_thr,
                ite(
                    gt(n("fired_idle"), c(0.5)),
                    ite(
                        ge(n("t_b2"), c(0.0)),
                        fuel_b2,
                        fuel_0 * (c(1.0) + n("t_b2") / max(n("t_drag"), c(1e-12) * n("unit_N_m"))),
                    ),
                    c(0.0) * n("unit_kg_s"),
                ),
            ),
            "the fuel map at its torque; trimmed along a Willans line below zero torque; none unfired",
        ),
        eq(n("fuel.m_flow"), n("fuel_rate"), "it draws its fuel from the tank"),
        eq(der("fuel_used"), n("fuel_rate"), "fuel used"),
        eq(
            der("work_fired"),
            ite(
                gt(n("fuel_rate"), c(0.0)),
                max(n("T") * n("w"), c(0.0) * n("unit_N_m") * n("unit_rad_s")),
                c(0.0) * n("unit_N_m") * n("unit_rad_s"),
            ),
            "the work its fuel gave",
        ),
        eq(n("sig_speed"), n("wa"), "engine speed"),
        eq(n("sig_torque"), n("T"), "engine torque"),
        eq(n("sig_fuel_rate"), n("fuel_rate"), "fuel rate"),
        eq(n("sig_power"), n("T") * n("w"), "mechanical power"),
        eq(n("sig_fuel_power"), n("fuel_rate") * n("fuel.e"), "fuel power"),
        eq(n("sig_losses"), n("fuel_rate") * n("fuel.e") - n("T") * n("w"), "losses"),
    ];
    let mut all = uc.into_params();
    all.extend(params);
    let mut fp = vec![
        cfg.scale,
        cfg.throttle_wired as u8 as f64,
        cfg.enable_wired as u8 as f64,
        cfg.tank as u8 as f64,
    ];
    for t in [&cfg.full_load, &cfg.drag] {
        fp.extend(&t.x);
        fp.extend(&t.y);
    }
    fp.extend(&cfg.fuel_map.outer);
    for s in &cfg.fuel_map.sheets {
        fp.push(s.x.len() as f64);
        fp.extend(&s.x);
        fp.extend(&s.y);
    }
    ComponentDef {
        name: variant("Blocks.CombustionEngine", *cfg == EngineConfig::default(), fp),
        doc: doc(id),
        ports,
        params: all,
        vars,
        equations: eqs,
        energy: EnergyDecl {
            stored: Some(c(0.5) * n("J") * n("w") * n("w")),
            loss: Some(n("fuel.e") * n("fuel.m_flow") - n("T") * n("w")),
        },
        ..Default::default()
    }
}

/// fuel.tank and fuel.h2_tank: a mass of fuel with its heating value.
pub fn tank(hydrogen: bool) -> ComponentDef {
    let id = if hydrogen { "fuel.h2_tank" } else { "fuel.tank" };
    let mut params = cps(id, &["capacity_kg", "initial_fill_pct"]);
    if hydrogen {
        params.push(p("lhv_MJ_per_kg", "J/kg", H2_LHV, "hydrogen's lower heating value"));
    } else {
        params.extend(cps(id, &["density_kg_per_l", "co2_kg_per_kg", "lhv_MJ_per_kg"]));
    }
    params.push(pe(
        "cap",
        "kg",
        max(n("capacity_kg"), c(1e-3) * n("unit_kg")),
        "capacity, at least 1 g",
    ));
    params.insert(0, p("unit_kg", "kg", 1.0, "unit carrier"));
    let mut m = state("m", "kg", 0.0, "fuel mass");
    m.start = Some(n("cap") * clamp(n("initial_fill_pct"), c(0.0), c(1.0)));
    m.nominal = Some(1.0);
    ComponentDef {
        name: if hydrogen { "Blocks.H2Tank".into() } else { "Blocks.FuelTank".into() },
        doc: doc(id),
        ports: vec![
            lsim_ir::component::build::port(
                "fuel",
                "FuelPort",
                "where its engines or fuel cells draw",
            ),
            out(id, "sig_level"),
            out(id, "sig_mass"),
        ],
        params,
        vars: vec![m],
        equations: vec![
            eq(n("fuel.e"), n("lhv_MJ_per_kg"), "its fuel's heating value"),
            eq(der("m"), n("fuel.m_flow"), "its mass falls by what is drawn"),
            eq(n("sig_level"), n("m") / n("cap"), "fill level"),
            eq(n("sig_mass"), n("m"), "fuel mass"),
        ],
        energy: EnergyDecl { stored: Some(n("m") * n("lhv_MJ_per_kg")), loss: None },
        ..Default::default()
    }
}

/// An inexhaustible fuel supply (an engine or stack without a tank).
pub fn supply() -> ComponentDef {
    ComponentDef {
        name: "Blocks.FuelSupply".into(),
        doc: "An inexhaustible fuel supply at a heating value (an engine or a fuel cell \
              without a tank: today's \"running on infinite fuel\")."
            .into(),
        ports: vec![lsim_ir::component::build::port("fuel", "FuelPort", "the fuel line")],
        params: vec![p("LHV", "J/kg", FUEL_LHV, "the fuel's heating value")],
        equations: vec![eq(n("fuel.e"), n("LHV"), "its heating value")],
        ..Default::default()
    }
}

/// The fuel cell's configuration.
#[derive(Clone, Debug, PartialEq)]
pub struct FuelCellConfig {
    /// stack voltage, V, by current, A
    pub polarization: Table1,
    /// a Hydrogen Tank feeds it (it stops when empty)
    pub tank: bool,
}

impl Default for FuelCellConfig {
    fn default() -> Self {
        let p = &catalog::param("fuelcell.stack", "polarization").expect("polarization")["default"];
        FuelCellConfig {
            polarization: Table1::from_json(
                p,
                catalog::axis_outside("fuelcell.stack", "polarization", 0),
            )
            .expect("default table"),
            tank: true,
        }
    }
}

/// fuelcell.stack: a voltage source following its polarization curve at
/// the current its bus takes (today finds that current by bisection on
/// V(I)·I = the bus load: the circuit does it here), up to its maximum
/// current (the bus manager holds its motors to it); hydrogen use in
/// proportion to the electrical energy.
pub fn fuel_cell(cfg: &FuelCellConfig) -> ComponentDef {
    let id = "fuelcell.stack";
    let mut uc = UnitCarriers::default();
    let pol = uc.t1(&cfg.polarization, n("I"), "A", "V");
    let pol_max = uc.t1(&cfg.polarization, n("i_max"), "A", "V");
    let mut params = cps(id, &["max_current_A", "h2_per_kwh_g"]);
    params.push(pe(
        "i_max",
        "A",
        max(n("max_current_A"), c(1.0) * n("unit_A")),
        "maximum current, at least 1 A",
    ));
    let mut ports = vec![
        phys(id, "pos", "Pin"),
        phys(id, "neg", "Pin"),
        lsim_ir::component::build::port("h2", "FuelPort", "its hydrogen line (the Hydrogen Tank)"),
    ];
    if cfg.tank {
        ports.push(input("h2_mass", "kg", "the hydrogen left in its tank"));
    }
    for q in ["sig_voltage", "sig_current", "sig_power", "sig_h2_rate", "sig_losses"] {
        ports.push(out(id, q));
    }
    ports.push(output("p_deliver", "W", "the most power it gives now (its window)"));
    let alive = if cfg.tank { gt(n("h2_mass"), c(0.0)) } else { ge(c(1.0), c(0.0)) };
    uc.of("A");
    let mut all = uc.into_params();
    all.extend(params);
    ComponentDef {
        name: variant(
            "Blocks.FuelCell",
            *cfg == FuelCellConfig::default(),
            cfg.polarization
                .x
                .iter()
                .chain(&cfg.polarization.y)
                .copied()
                .chain([cfg.tank as u8 as f64]),
        ),
        doc: doc(id),
        ports,
        params: all,
        vars: vec![
            guess("v", "V", c(cfg.polarization.eval(0.0)) * n("unit_V"), "stack voltage"),
            var("I", "A", "stack current"),
            var("P", "W", "electrical power"),
        ],
        equations: vec![
            eq(n("v"), n("pos.v") - n("neg.v"), "stack voltage"),
            eq(c(0.0), n("pos.i") + n("neg.i"), "the current out of pos returns at neg"),
            eq(n("I"), -n("pos.i"), "the current it gives"),
            eq(n("v"), pol, "the polarization curve at that current"),
            eq(n("P"), n("v") * n("I"), "electrical power"),
            eq(
                n("h2.m_flow"),
                max(n("P"), c(0.0) * n("unit_V") * n("unit_A")) * n("h2_per_kwh_g"),
                "hydrogen in proportion to the energy it gives",
            ),
            eq(n("sig_voltage"), n("v"), "stack voltage"),
            eq(n("sig_current"), n("I"), "stack current"),
            eq(n("sig_power"), n("P"), "electrical power"),
            eq(n("sig_h2_rate"), n("h2.m_flow"), "hydrogen consumption"),
            eq(n("sig_losses"), n("h2.e") * n("h2.m_flow") - n("P"), "losses"),
            eq(
                n("p_deliver"),
                ite(alive, pol_max * n("i_max"), c(0.0) * n("unit_V") * n("unit_A")),
                "its most power: the curve at its maximum current",
            ),
        ],
        energy: EnergyDecl { stored: None, loss: Some(n("h2.e") * n("h2.m_flow") - n("P")) },
        ..Default::default()
    }
}

/// The engine group in its default configuration.
pub fn defaults() -> Vec<ComponentDef> {
    vec![
        engine(&EngineConfig::default()),
        tank(false),
        tank(true),
        supply(),
        fuel_cell(&FuelCellConfig::default()),
    ]
}
