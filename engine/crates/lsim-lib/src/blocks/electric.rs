//! Electrical blocks: ground, electric node, voltage source, power
//! consumer, climate control, DC-DC converter.
//!
//! Today's engine balances power on each bus's positive rail and ignores
//! the return path; here the circuit is a circuit: every part sits between
//! its `pos` and `neg` pins, the importer connects any unwired negative
//! terminal to an implicit ground (today's implicit return), and loads take
//! power as `v·i`. With one source per bus, as today requires, the powers
//! are today's.

use super::*;
use crate::table::{Table1, UnitCarriers, table_param};
use lsim_ir::EnergyDecl;
use lsim_ir::ParamValue;

const CLIMATE: &str = "electric.climate";

/// The two-pin equations of a block between `pos` and `neg`: `v` across
/// it, `i` the current it takes in at `pos`.
fn two_pin_eqs() -> Vec<lsim_ir::EquationDecl> {
    vec![
        eq(n("v"), n("pos.v") - n("neg.v"), "its voltage is pos − neg"),
        eq(c(0.0), n("pos.i") + n("neg.i"), "the current into pos leaves at neg"),
        eq(n("i"), n("pos.i"), "the current it takes in at pos"),
    ]
}

fn two_pin_vars() -> Vec<lsim_ir::VarDecl> {
    vec![
        guess("v", "V", c(400.0), "its voltage, pos − neg"),
        var("i", "A", "the current it takes in at pos"),
    ]
}

/// boundary.ground: three terminals, all at 0 V.
pub fn ground() -> ComponentDef {
    let id = "boundary.ground";
    ComponentDef {
        name: "Blocks.Ground".into(),
        doc: doc(id),
        ports: vec![phys(id, "t1", "Pin"), phys(id, "t2", "Pin"), phys(id, "t3", "Pin")],
        equations: ["t1", "t2", "t3"]
            .iter()
            .map(|t| eq(n(&format!("{t}.v")), c(0.0), "ground is at 0 V"))
            .collect(),
        ..Default::default()
    }
}

/// The Electric Node's terminals in the library; a part may list its own.
pub const NODE_TERMINALS: [&str; 5] = ["t1", "t2", "t3", "t4", "t5"];

/// A junction's definition name: `base` with the library's terminals, else
/// `base` followed by the part's own terminals (`Blocks.ElectricNode_t1_t2_t6`).
pub fn junction_name(base: &str, ts: &[String], library: &[&str]) -> String {
    if ts.iter().map(String::as_str).eq(library.iter().copied()) {
        base.to_string()
    } else {
        format!("{base}_{}", ts.join("_"))
    }
}

/// electric.node: its terminals (`ts`: the library's five, or the part's
/// own, at least two) at one voltage, currents summing to zero.
/// Its Throughput Power is the power its bus's source feeds in through
/// terminal `ts[source]` (today's bus load, negative while the bus feeds the
/// source back), or, when no terminal leads straight to a source, the
/// power coming in through all terminals that bring it.
pub fn electric_node(ts: &[String], source: Option<usize>) -> ComponentDef {
    let id = "electric.node";
    let first = format!("{}.v", ts[0]);
    let mut equations: Vec<lsim_ir::EquationDecl> = ts[1..]
        .iter()
        .map(|t| eq(n(&format!("{t}.v")), n(&first), "every terminal is at one voltage"))
        .collect();
    equations.push(eq(
        c(0.0),
        sum(ts.iter().map(|t| n(&format!("{t}.i")))),
        "the currents sum to zero",
    ));
    let power = match source {
        Some(k) => n(&format!("{}.v", ts[k])) * n(&format!("{}.i", ts[k])),
        None => sum(ts.iter().map(|t| max(n(&format!("{t}.v")) * n(&format!("{t}.i")), c(0.0)))),
    };
    equations.push(eq(n("sig_power"), power, "the power its source feeds in"));
    let mut ports: Vec<PortDecl> = ts.iter().map(|t| phys(id, t, "Pin")).collect();
    ports.push(out(id, "sig_power"));
    let base = junction_name("Blocks.ElectricNode", ts, &NODE_TERMINALS);
    ComponentDef {
        name: match source {
            None => base,
            Some(k) => format!("{base}_src{}", k + 1),
        },
        doc: doc(id),
        ports,
        equations,
        ..Default::default()
    }
}

/// electric.voltage_source: an ideal source; Supplied Power is what it
/// gives (negative when it takes power back).
pub fn voltage_source() -> ComponentDef {
    let id = "electric.voltage_source";
    let mut eqs = two_pin_eqs();
    eqs.push(eq(n("v"), n("voltage_V"), "it holds its voltage"));
    eqs.push(eq(n("sig_voltage"), n("v"), "its voltage"));
    eqs.push(eq(n("sig_power"), -(n("v") * n("i")), "the power it supplies"));
    ComponentDef {
        name: "Blocks.VoltageSource".into(),
        doc: doc(id),
        ports: vec![
            phys(id, "pos", "Pin"),
            phys(id, "neg", "Pin"),
            out(id, "sig_voltage"),
            out(id, "sig_power"),
        ],
        params: cps(id, &["voltage_V"]),
        vars: two_pin_vars(),
        equations: eqs,
        ..Default::default()
    }
}

/// What feeds a load's served share (its source cutting it back).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LoadConfig {
    /// the Power demand input is wired (else the parameter sets the power)
    pub demand_wired: bool,
    /// a bus manager gives it the share of its demand its source serves
    pub served_input: bool,
}

/// electric.constant_drive: draws its power (the demand input, else
/// Constant Power Draw; negative counts as 0), cut back to what its source
/// serves.
pub fn power_consumer(cfg: LoadConfig) -> ComponentDef {
    let id = "electric.constant_drive";
    let mut ports = vec![phys(id, "pos", "Pin"), phys(id, "neg", "Pin")];
    if cfg.demand_wired {
        ports.push(inp(id, "sig_demand_in"));
    }
    if cfg.served_input {
        ports.push(input("served", "1", "the share of its demand its source serves (0-1)"));
    }
    ports.push(out(id, "sig_power"));
    if cfg.served_input {
        ports.push(output("p_dem", "W", "the power it asks for (for the bus manager)"));
    }
    let demand = if cfg.demand_wired { n("sig_demand_in") } else { n("power_kW") };
    let share = if cfg.served_input { n("served") } else { c(1.0) };
    let mut eqs = two_pin_eqs();
    eqs.push(eq(n("p_demand"), max(demand, c(0.0)), "it asks for its demand, never less than 0"));
    if cfg.served_input {
        eqs.push(eq(n("p_dem"), n("p_demand"), "what it asks of its bus"));
    }
    eqs.push(eq(n("sig_power"), n("p_demand") * share, "it draws what its source serves"));
    eqs.push(eq(n("v") * n("i"), n("sig_power"), "it takes that power from the circuit"));
    let mut vars = two_pin_vars();
    vars.push(var("p_demand", "W", "the power it asks for"));
    ComponentDef {
        name: variant(
            "Blocks.PowerConsumer",
            cfg == LoadConfig::default(),
            [cfg.demand_wired as u8 as f64, cfg.served_input as u8 as f64],
        ),
        doc: doc(id),
        ports,
        params: cps(id, &["power_kW"]),
        vars,
        equations: eqs,
        energy: EnergyDecl { stored: None, loss: Some(n("v") * n("i")) },
        ..Default::default()
    }
}

/// The climate control's configuration.
#[derive(Clone, Debug, PartialEq)]
pub struct ClimateConfig {
    /// heat (+) or cooling (−) the cabin needs, W, by outside temperature, °C
    pub demand: Table1,
    /// Heat Source is the heat pump (else the PTC heater)
    pub heat_pump: bool,
    /// the Enable input is wired (else always on)
    pub enable_wired: bool,
    /// a bus manager gives it the share of its demand its source serves
    pub served_input: bool,
}

impl Default for ClimateConfig {
    fn default() -> Self {
        let p = &catalog::param(CLIMATE, "demand_table").expect("demand_table")["default"];
        ClimateConfig {
            demand: Table1::from_json(p, catalog::axis_outside(CLIMATE, "demand_table", 0))
                .expect("default table")
                .scaled(1.0, 1e3),
            heat_pump: false,
            enable_wired: false,
            served_input: false,
        }
    }
}

impl ClimateConfig {
    /// The runtime data a part of this configuration takes: its demand
    /// table (heat by outside temperature).
    pub fn values(&self) -> Result<Vec<(String, ParamValue)>, String> {
        Ok(vec![
            ("demand_table".into(), ParamValue::Table(self.demand.data("K")?)),
            (
                "heat_source".into(),
                ParamValue::Enum(
                    if self.heat_pump { "HeatSource.HeatPump" } else { "HeatSource.PTCHeater" }
                        .into(),
                ),
            ),
        ])
    }
}

/// A heat pump's COP: `share` × the Carnot COP between the exchangers at
/// `t_hot` and `t_cold` (K), at least 1 heating and 0.5 cooling (today's
/// `carnot_share_cop`).
fn carnot(t_hot: Expr, t_cold: Expr, heating: bool) -> Expr {
    let lift = max(t_hot.clone() - t_cold.clone(), c(1.0));
    let ideal = if heating { t_hot } else { t_cold } / lift;
    max(c(if heating { 1.0 } else { 0.5 }), n("cop_carnot_share") * ideal)
}

/// electric.climate: heating or air-conditioning as an electrical load
/// from the outside temperature (today's `climate.py`, exactly): the heat
/// the cabin needs from the demand table; a PTC heater at COP 1, a heat
/// pump (above its minimum outside temperature) or the air-conditioning
/// at a share of the Carnot COP between exchangers 15 K past the air they
/// serve; the blower's power on top while it heats or cools.
pub fn climate(cfg: &ClimateConfig) -> ComponentDef {
    let id = CLIMATE;
    let mut uc = UnitCarriers::default();
    let mut ports = vec![phys(id, "pos", "Pin"), phys(id, "neg", "Pin")];
    if cfg.enable_wired {
        ports.push(inp(id, "sig_on_in"));
    }
    ports.push(input("T_amb", "K", "the outside air temperature (from the Ambient)"));
    if cfg.served_input {
        ports.push(input("served", "1", "the share of its demand its source serves (0-1)"));
    }
    ports.extend([out(id, "sig_power"), out(id, "sig_heat"), out(id, "sig_cop")]);
    if cfg.served_input {
        ports.push(output("p_dem", "W", "the power it asks for (for the bus manager)"));
    }
    let on = cfg.enable_wired.then(|| ge(n("sig_on_in"), c(0.5)));
    let approach = 15.0;
    let t_set = n("cabin_setpoint_C");
    let t_out = n("T_amb");
    // the heat pump (above its minimum outside temperature), else the
    // PTC heater at COP 1
    let cop_heat = ite(
        and(gt(n("heat_source"), c(1.5)), ge(t_out.clone(), n("heat_pump_min_C"))),
        carnot(t_set.clone() + c(approach), t_out.clone() - c(approach), true),
        c(1.0),
    );
    let cop_cool = carnot(t_out.clone() + c(approach), t_set - c(approach), false);
    let share = if cfg.served_input { n("served") } else { c(1.0) };
    let mut eqs = two_pin_eqs();
    eqs.push(eq(
        n("t_out_c"),
        n("T_amb") - c(273.15) * n("unit_K"),
        "the outside temperature, from 0 °C",
    ));
    eqs.push(eq(
        n("q"),
        match on {
            Some(on) => ite(
                on,
                lsim_ir::expr::table("demand_table", vec![n("t_out_c")]),
                c(0.0) * n("unit_W"),
            ),
            None => lsim_ir::expr::table("demand_table", vec![n("t_out_c")]),
        },
        "the heat the cabin needs (cooling negative), while enabled",
    ));
    eqs.push(eq(
        n("sig_cop"),
        ite(gt(n("q"), c(0.0)), cop_heat, ite(lt(n("q"), c(0.0)), cop_cool, c(0.0))),
        "its COP: heater, heat pump or air-conditioning; 0 when off",
    ));
    eqs.push(eq(
        n("p_asked"),
        ite(
            gt(noev(abs(n("q"))), c(0.0)),
            noev(abs(n("q"))) / n("sig_cop") + n("fan_power_kW"),
            c(0.0),
        ),
        "the electrical power that heat asks for, with the blower",
    ));
    eqs.push(eq(n("sig_power"), n("p_asked") * share.clone(), "it draws what its source serves"));
    if cfg.served_input {
        eqs.push(eq(n("p_dem"), n("p_asked"), "what it asks of its bus"));
    }
    eqs.push(eq(n("sig_heat"), n("q") * share, "cut back, it heats or cools that much less"));
    eqs.push(eq(n("v") * n("i"), n("sig_power"), "it takes that power from the circuit"));
    let mut params =
        cps(id, &["cop_carnot_share", "heat_pump_min_C", "cabin_setpoint_C", "fan_power_kW"]);
    params.push(lsim_ir::ParamDecl {
        default: ParamValue::Enum("HeatSource.PTCHeater".into()),
        structural: true,
        ..p("heat_source", "1", 0.0, "Heat Source: the PTC heater or the heat pump")
    });
    params.push(table_param(
        "demand_table",
        "W",
        ClimateConfig::default().demand.data("K").expect("the default table"),
        "heat the cabin needs (cooling negative) by outside temperature, from 0 °C",
    ));
    let mut vars = two_pin_vars();
    vars.extend([
        var("t_out_c", "K", "the outside temperature above 0 °C"),
        var("q", "W", "heat asked for (cooling negative)"),
        var("p_asked", "W", "the electrical power it asks for"),
    ]);
    uc.of("K");
    uc.of("W");
    let mut all = uc.into_params();
    all.extend(params);
    let params = all;
    let cfg_print = [cfg.enable_wired as u8 as f64, cfg.served_input as u8 as f64];
    let d = ClimateConfig::default();
    let is_default = (cfg.enable_wired, cfg.served_input) == (d.enable_wired, d.served_input);
    ComponentDef {
        name: variant("Blocks.ClimateControl", is_default, cfg_print),
        doc: doc(id),
        ports,
        params,
        vars,
        equations: eqs,
        energy: EnergyDecl { stored: None, loss: Some(n("v") * n("i")) },
        types: vec![lsim_ir::EnumType {
            name: "HeatSource".into(),
            literals: vec![
                lsim_ir::EnumLiteral { name: "PTCHeater".into(), doc: "PTC heater".into() },
                lsim_ir::EnumLiteral { name: "HeatPump".into(), doc: "Heat pump".into() },
            ],
            doc: "what heats the cabin".into(),
        }],
        ..Default::default()
    }
}

/// A bus's source-limit handshake (today's `update_source_limits` and
/// `allocate_motor_power`) for a bus with a battery or fuel cell of its
/// own: the consumers are served first, all cut back by one share when
/// the source cannot carry them; the motors share what is left, in
/// proportion to what they ask for, and feed back at most what the source
/// takes plus what the consumers use. Its inputs: the source's window
/// (`p_deliver`, `p_absorb`), each consumer's demand `p_dem<k>`, each
/// motor's request `p_req<m>`; its outputs: the consumers' share `served`
/// and each motor's window `p_hi<m>`, `p_lo<m>` (its share where they are
/// cut back, else its request and the room left).
pub fn bus_manager(motors: usize, consumers: usize) -> ComponentDef {
    bus_manager_fed(motors, consumers, 0, 0)
}

/// [`bus_manager`] for a battery's bus that `inflows` DC-DC converters at
/// their power setpoint feed (today's setpoint mode): each one's setpoint,
/// as far as its own supply serves it (`p_in<j>`), adds to what the
/// battery gives (today's `inflow`); and with what the bus's `uses` loads
/// use now (`p_use<u>`: motors, consumers, converters drawing from it),
/// each converter is told its share of what the bus takes in (`absorb<j>`:
/// the battery's charge window plus those loads, split in proportion to
/// the setpoints, as today's bus balance splits it).
pub fn bus_manager_fed(
    motors: usize,
    consumers: usize,
    inflows: usize,
    uses: usize,
) -> ComponentDef {
    let zero = || c(0.0) * n("unit_W");
    let mut ports = vec![
        input("p_deliver", "W", "the most its source gives now"),
        input("p_absorb", "W", "the most its source takes now"),
    ];
    for k in 1..=consumers {
        ports.push(input(&format!("p_dem{k}"), "W", "a consumer's demand"));
    }
    for m in 1..=motors {
        ports.push(input(&format!("p_req{m}"), "W", "a motor's request"));
    }
    for j in 1..=inflows {
        ports.push(input(
            &format!("p_in{j}"),
            "W",
            "a converter's setpoint, as its supply serves it",
        ));
    }
    for u in 1..=uses {
        ports.push(input(&format!("p_use{u}"), "W", "the power a load on the bus uses now"));
    }
    ports.push(output("served", "1", "the share of their demand the consumers get"));
    for m in 1..=motors {
        ports.push(output(&format!("p_hi{m}"), "W", "the most this motor may draw now"));
        ports.push(output(
            &format!("p_lo{m}"),
            "W",
            "the most (negative) this motor may feed back now",
        ));
    }
    for j in 1..=inflows {
        ports.push(output(&format!("absorb{j}"), "W", "its share of what the bus takes in"));
    }
    // what the source gives, with the converters' inflow
    let supply = if inflows == 0 {
        n("p_deliver")
    } else {
        n("p_deliver") + sum((1..=inflows).map(|j| n(&format!("p_in{j}"))))
    };
    let fixed = sum((1..=consumers).map(|k| n(&format!("p_dem{k}"))).chain([zero()]));
    let req = |m: usize| n(&format!("p_req{m}"));
    let pos = sum((1..=motors).map(|m| max(req(m), zero())).chain([zero()]));
    let neg = sum((1..=motors).map(|m| min(req(m), zero())).chain([zero()]));
    let tol = |x: Expr| c(1e-9) * max(n("unit_W"), abs(x));
    // every switch here is continuous in its effect (a window equal to the
    // request at its boundary), so none needs an event
    let mut eqs = vec![
        eq(n("fixed"), fixed, "the consumers' demand"),
        eq(n("deliver"), max(supply.clone(), zero()), "what the source gives"),
        eq(
            n("served"),
            ite(noev(gt(n("fixed"), n("deliver"))), n("deliver") / n("fixed"), c(1.0)),
            "the consumers are served first, cut back when the source cannot carry them",
        ),
        eq(n("hi"), supply - n("fixed") * n("served"), "room left for the motors"),
        eq(n("lo"), -(n("p_absorb") + n("fixed") * n("served")), "what the motors may feed back"),
        eq(n("pos"), pos, "what the motors drawing ask for"),
        eq(n("neg"), neg, "what the motors recuperating ask to feed back"),
        eq(
            n("cut_hi"),
            ite(noev(gt(n("pos") + n("neg"), n("hi") + tol(n("hi")))), c(1.0), c(0.0)),
            "the motors ask for more than the room",
        ),
        eq(
            n("cut_lo"),
            ite(
                and(
                    noev(lt(n("cut_hi"), c(0.5))),
                    and(
                        noev(lt(n("pos") + n("neg"), n("lo") - tol(n("lo")))),
                        noev(lt(n("neg"), zero())),
                    ),
                ),
                c(1.0),
                c(0.0),
            ),
            "they feed back more than the source takes",
        ),
        eq(
            n("k_hi"),
            max(c(0.0), (n("hi") - n("neg")) / max(n("pos"), c(1e-9) * n("unit_W"))),
            "the share of their requests the drawing motors get",
        ),
        eq(
            n("k_lo"),
            min(c(1.0), max(c(0.0), (n("lo") - n("pos")) / min(n("neg"), c(-1e-9) * n("unit_W")))),
            "the share of their feedback the recuperating motors get",
        ),
    ];
    // each motor's window is continuous in its request: cut back, its share
    // of the room (of what the source takes); else its request and all the
    // room left (the room alone for a motor that asks for nothing). (It
    // was open, 1e30 W, for a request on the other side of zero: at an
    // empty battery a request that was the solver's noise about zero
    // switched a motor's window between open and nothing by its sign.)
    let total = || n("pos") + n("neg");
    for m in 1..=motors {
        eqs.push(eq(
            n(&format!("p_hi{m}")),
            ite(
                noev(gt(n("cut_hi"), c(0.5))),
                n("k_hi") * max(req(m), zero()),
                max(req(m), zero()) + max(n("hi") - total(), zero()),
            ),
            "its share of the room",
        ));
        eqs.push(eq(
            n(&format!("p_lo{m}")),
            ite(
                noev(gt(n("cut_lo"), c(0.5))),
                n("k_lo") * min(req(m), zero()),
                min(req(m), zero()) - max(total() - n("lo"), zero()),
            ),
            "its share of what the source takes",
        ));
    }
    let mut vars = vec![];
    if inflows > 0 {
        let inflow = sum((1..=inflows).map(|j| n(&format!("p_in{j}"))));
        let takes =
            n("p_absorb") + sum((1..=uses).map(|u| n(&format!("p_use{u}"))).chain([zero()]));
        eqs.push(eq(n("inflow"), inflow, "the converters' setpoints"));
        eqs.push(eq(
            n("takes"),
            max(takes, zero()),
            "what the bus takes in: the battery's charge window plus its loads",
        ));
        for j in 1..=inflows {
            eqs.push(eq(
                n(&format!("absorb{j}")),
                n("takes") * n(&format!("p_in{j}")) / max(n("inflow"), c(1e-9) * n("unit_W")),
                "its share of it, in proportion to its setpoint",
            ));
        }
        vars.push(var("inflow", "W", "the converters' setpoints, as their supplies serve them"));
        vars.push(var("takes", "W", "what the bus takes in"));
    }
    ComponentDef {
        name: if inflows == 0 {
            format!("Blocks.BusManager_{motors}_{consumers}")
        } else {
            format!("Blocks.BusManager_{motors}_{consumers}_{inflows}_{uses}")
        },
        doc: "A bus's source-limit handshake: consumers first, then the motors share the \
              source's window (today's update_source_limits and allocate_motor_power)."
            .into(),
        ports,
        params: vec![p("unit_W", "W", 1.0, "unit carrier")],
        vars: vec![
            var("fixed", "W", "the consumers' demand"),
            var("deliver", "W", "what the source gives"),
            var("hi", "W", "room for the motors"),
            var("lo", "W", "what the motors may feed back (negative)"),
            var("pos", "W", "requests of the motors drawing"),
            var("neg", "W", "requests of the motors recuperating"),
            var("cut_hi", "1", "1 while the drawing motors are cut"),
            var("cut_lo", "1", "1 while the recuperating motors are cut"),
            var("k_hi", "1", "the drawing motors' share"),
            var("k_lo", "1", "the recuperating motors' share"),
        ]
        .into_iter()
        .chain(vars)
        .collect(),
        equations: eqs,
        ..Default::default()
    }
}

/// [`bus_manager`] for the bus a DC-DC converter holding its output
/// voltage feeds: it also says what its loads ask of the converter, which
/// asks it of its own input bus (`ask_fixed`: its consumers' demand;
/// `ask_motors`: its motors' requests to draw). Its source window is what
/// the converter's input bus grants the converter, times its efficiency;
/// it takes nothing back (the converter passes power one way).
pub fn bus_manager_asking(motors: usize, consumers: usize) -> ComponentDef {
    let mut d = bus_manager_fed(motors, consumers, 0, 0);
    d.name = format!("Blocks.BusManager_{motors}_{consumers}_asking");
    d.ports.push(output("ask_fixed", "W", "its consumers' demand, asked of its converter"));
    d.ports.push(output("ask_motors", "W", "its motors' requests to draw, asked of its converter"));
    d.equations.push(eq(n("ask_fixed"), n("fixed"), "its consumers' demand"));
    d.equations.push(eq(n("ask_motors"), n("pos"), "its motors' requests to draw"));
    d
}

/// An open window, W.
pub const BIG_W: f64 = 1e30;

/// How a DC-DC converter runs.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DcDcConfig {
    /// its output bus has a source of its own (a battery): it delivers its
    /// power setpoint; else it holds its output voltage
    pub setpoint_mode: bool,
    /// the Power Setpoint input is wired (setpoint mode)
    pub setpoint_wired: bool,
}

/// controller.dcdc: an averaged converter. Holding its output voltage, it
/// draws P_out/η at its input for what the output bus takes, inside its
/// input bus's window as today's source tree has it: on its input bus it
/// asks for its output bus's consumers' demand over η as a consumer
/// (`p_dem`, served with the consumers there: `served`) and for its
/// output bus's motors' requests over η as a motor (`p_request`, given
/// its window `p_hi`), and what it is granted, times η, is its output
/// bus's window (`p_deliver`). It passes power one way: a surplus on its
/// output bus (only rounding, once the window holds) is its loss, not
/// power at its input. At a power setpoint, it feeds that power into the
/// output bus (the bus's battery balances the rest) and draws P/η; on its
/// input bus it is a load of P/η (`p_dem`) cut back with the consumers
/// there (`served`), and what that leaves of its setpoint (`p_set`) is
/// what its output bus's window counts on. Power flows from a to b; the
/// motors behind it are allowed to feed back only what that bus's own
/// loads use (the bus manager's window), as today. A Power Setpoint
/// linked while it holds its voltage is a port it does not read.
pub fn dcdc(cfg: DcDcConfig) -> ComponentDef {
    let id = "controller.dcdc";
    let mut ports = vec![
        phys(id, "a_pos", "Pin"),
        phys(id, "a_neg", "Pin"),
        phys(id, "b_pos", "Pin"),
        phys(id, "b_neg", "Pin"),
    ];
    if cfg.setpoint_wired {
        ports.push(inp(id, "sig_setpoint_in"));
    }
    ports.push(input("served", "1", "the share of its demand its input bus serves (0-1)"));
    if cfg.setpoint_mode {
        ports.push(input(
            "absorb",
            "W",
            "its share of what the output bus can take in (its battery's charge window plus its loads)",
        ));
    } else {
        ports.push(input(
            "p_hi",
            "W",
            "the most its output bus's motors may draw through it, at its input",
        ));
        ports.push(input("ask_fixed", "W", "its output bus's consumers' demand"));
        ports.push(input("ask_motors", "W", "its output bus's motors' requests to draw"));
    }
    ports.extend([out(id, "sig_power_in"), out(id, "sig_power_out"), out(id, "sig_losses")]);
    if cfg.setpoint_mode {
        ports.push(output("p_dem", "W", "what its setpoint asks of its input bus"));
        ports.push(output("p_set", "W", "its setpoint, as far as its input bus serves it"));
    } else {
        ports.push(output("p_dem", "W", "what its output bus's consumers ask of its input bus"));
        ports.push(output("p_request", "W", "what its output bus's motors ask of its input bus"));
        ports.push(output("p_deliver", "W", "the most its output bus may take from it now"));
    }
    let mut eqs = vec![
        eq(n("va"), n("a_pos.v") - n("a_neg.v"), "input voltage"),
        eq(n("vb"), n("b_pos.v") - n("b_neg.v"), "output voltage"),
        eq(c(0.0), n("a_pos.i") + n("a_neg.i"), "input current balance"),
        eq(c(0.0), n("b_pos.i") + n("b_neg.i"), "output current balance"),
        eq(n("sig_power_in"), n("va") * n("a_pos.i"), "the power it draws at its input"),
        eq(n("sig_power_out"), -(n("vb") * n("b_pos.i")), "the power it gives at its output"),
        if cfg.setpoint_mode {
            eq(
                n("sig_power_in"),
                ite(
                    ge(n("sig_power_out"), c(0.0)),
                    n("sig_power_out") / n("efficiency_pct"),
                    n("sig_power_out") * n("efficiency_pct"),
                ),
                "it draws the output power over its efficiency",
            )
        } else {
            eq(
                n("sig_power_in"),
                max(n("sig_power_out"), c(0.0)) / n("efficiency_pct"),
                "it draws the output power over its efficiency, one way: a surplus on its \
                 output bus is its loss",
            )
        },
        eq(n("sig_losses"), n("sig_power_in") - n("sig_power_out"), "its loss"),
    ];
    if cfg.setpoint_mode {
        let sp = if cfg.setpoint_wired { n("sig_setpoint_in") } else { n("power_setpoint_kW") };
        eqs.push(eq(
            n("p_dem"),
            max(sp.clone(), c(0.0)) / n("efficiency_pct"),
            "what it asks of its input",
        ));
        eqs.push(eq(
            n("p_set"),
            max(sp, c(0.0)) * n("served"),
            "its setpoint, as its input serves it",
        ));
        eqs.push(eq(
            n("sig_power_out"),
            min(n("p_set"), max(n("absorb"), c(0.0))),
            "it delivers its setpoint, as far as its output bus takes it",
        ));
    } else {
        eqs.push(eq(n("vb"), n("output_voltage_V"), "it holds its output voltage"));
        eqs.push(eq(
            n("p_dem"),
            n("ask_fixed") / n("efficiency_pct"),
            "its output bus's consumers' demand, at its input",
        ));
        eqs.push(eq(
            n("p_request"),
            n("ask_motors") / n("efficiency_pct"),
            "its output bus's motors' requests, at its input",
        ));
        eqs.push(eq(
            n("p_deliver"),
            n("efficiency_pct") * (n("p_dem") * n("served") + n("p_hi")),
            "what its input bus grants it, at its output",
        ));
    }
    ComponentDef {
        name: variant(
            "Blocks.DcDc",
            cfg == DcDcConfig::default(),
            [cfg.setpoint_mode as u8 as f64, cfg.setpoint_wired as u8 as f64],
        ),
        doc: doc(id),
        ports,
        params: cps(id, &["efficiency_pct", "output_voltage_V", "power_setpoint_kW"]),
        vars: vec![
            guess("va", "V", c(400.0), "input voltage"),
            guess("vb", "V", c(350.0), "output voltage"),
        ],
        equations: eqs,
        energy: EnergyDecl { stored: None, loss: Some(n("sig_losses")) },
        ..Default::default()
    }
}

/// The electrical blocks in their default configuration.
pub fn defaults() -> Vec<ComponentDef> {
    vec![
        ground(),
        electric_node(&NODE_TERMINALS.map(String::from), None),
        voltage_source(),
        power_consumer(LoadConfig::default()),
        climate(&ClimateConfig::default()),
        dcdc(DcDcConfig::default()),
    ]
}
