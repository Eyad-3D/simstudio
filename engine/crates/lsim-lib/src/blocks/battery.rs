//! battery.generic: the equivalent-circuit battery pack.
//!
//! OCV(SOC) table + R0 + an optional RC pair; the SOC counts charge
//! (A·h) with a coulombic efficiency on charging; today's limits as the
//! power window its bus manager hands the motors (`p_deliver`,
//! `p_absorb`): the maximum-power point, the Output Power Limit less its
//! margin, the Max Charge Power, the minimum SOC and 100 %, and with pack
//! limits the current and voltage limits and the SOC derating band.
//!
//! Exactly today's equations (`battery.py`, `domains.py`), in continuous
//! time: the RC pair's implicit-Euler update is the ODE it discretises; the
//! "within the step" SOC limits (a current that would cross the minimum
//! SOC within one 10 ms step) become their limit, a window that closes
//! at the minimum SOC and at 100 %.
//!
//! The OCV table is runtime data (the parameter `ocv_table`, linear as
//! today); the definition depends only on the structural choices (an RC
//! pair, which pack limits are set, whether the power limit holds).
//!
//! Energy books: stored = the chemical energy ∫OCV·dQ (a state, so the
//! OCV table can change without a rebuild) + ½·C1·v1²; loss = R0·i² +
//! v1²/R1 + the charge not stored, (1−η)·OCV·|i| while charging.

use super::*;
use crate::table::{Table1, UnitCarriers, table_param};
use lsim_ir::EnergyDecl;
use lsim_ir::ParamValue;

const ID: &str = "battery.generic";
/// "infinite" power or current for a limit that is not set
const BIG: f64 = 1e30;

/// Pack-level limits (Pack values with a current or voltage limit or a
/// derating band; today's `CellPack` in PACK mode).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PackLimits {
    /// a discharge current limit is set
    pub i_dis: bool,
    /// a charge current limit is set
    pub i_ch: bool,
    /// a minimum terminal voltage is set
    pub v_min: bool,
    /// a maximum terminal voltage is set
    pub v_max: bool,
    /// a derating band is set
    pub band: bool,
}

/// The battery's configuration.
#[derive(Clone, Debug, PartialEq)]
pub struct BatteryConfig {
    /// open-circuit voltage, V, by SOC as a fraction 0-1 (runtime data:
    /// [`BatteryConfig::values`])
    pub ocv: Table1,
    /// an RC pair (R1 and its time constant both above 0)
    pub rc: bool,
    /// pack limits, if any
    pub limits: Option<PackLimits>,
    /// the Output Power Limit holds the power (else it is only checked)
    pub limit_enforced: bool,
}

impl Default for BatteryConfig {
    fn default() -> Self {
        let p = &catalog::param(ID, "ocv_table").expect("ocv_table")["default"];
        BatteryConfig {
            ocv: Table1::from_json(p, catalog::axis_outside(ID, "ocv_table", 0))
                .expect("default table")
                .scaled(0.01, 1.0),
            rc: false,
            limits: None,
            limit_enforced: true,
        }
    }
}

impl BatteryConfig {
    /// From today's merged parameters (Pack values).
    pub fn from_params(
        p: &serde_json::Map<String, serde_json::Value>,
        outside: crate::table::Outside,
    ) -> Result<Self, String> {
        use catalog::{flag, get};
        let ocv = Table1::from_json(p.get("ocv_table").ok_or("no ocv_table")?, outside)?
            .scaled(0.01, 1.0);
        let limits = [
            get(p, "max_discharge_current_A", 0.0) > 0.0,
            get(p, "max_charge_current_A", 0.0) > 0.0,
            get(p, "min_voltage_V", 0.0) > 0.0,
            get(p, "max_voltage_V", 0.0) > 0.0,
            get(p, "soc_derate_band_pct", 0.0) > 0.0,
        ];
        Ok(BatteryConfig {
            ocv,
            rc: get(p, "rc_resistance_ohm", 0.0) > 0.0 && get(p, "rc_time_constant_s", 0.0) > 0.0,
            limits: limits.iter().any(|x| *x).then_some(PackLimits {
                i_dis: limits[0],
                i_ch: limits[1],
                v_min: limits[2],
                v_max: limits[3],
                band: limits[4],
            }),
            limit_enforced: flag(p, "power_limit_enforced", true),
        })
    }
}

/// The OCV table's mean over 0-100 % SOC (today's `ocv_mean`).
pub fn ocv_mean(t: &Table1) -> f64 {
    t.integral(0.0, 1.0)
}

impl BatteryConfig {
    /// The runtime data a part of this configuration takes: its OCV table
    /// and the table's mean.
    pub fn values(&self) -> Result<Vec<(String, ParamValue)>, String> {
        Ok(vec![
            // read at the SOC held to 0-1: padded past both ends
            ("ocv_table".into(), ParamValue::Table(self.ocv.padded(true, true).data("1")?)),
            ("ocv_mean".into(), ParamValue::Real(c(ocv_mean(&self.ocv)))),
        ])
    }

    /// The structural choices, for the definition's name.
    fn is_default_structure(&self) -> bool {
        let d = BatteryConfig::default();
        self.rc == d.rc && self.limits == d.limits && self.limit_enforced == d.limit_enforced
    }
}

/// The battery block.
pub fn battery(cfg: &BatteryConfig) -> ComponentDef {
    let id = ID;
    let mut uc = UnitCarriers::default();
    let mut params = cps(
        id,
        &[
            "capacity_kWh",
            "capacity_Ah",
            "coulombic_efficiency_pct",
            "internal_resistance_ohm",
            "rc_resistance_ohm",
            "rc_time_constant_s",
            "max_charge_power_kW",
            "initial_soc_pct",
            "min_soc_pct",
            "output_power_limit_kW",
            "power_limit_margin_pct",
            "max_discharge_current_A",
            "max_charge_current_A",
            "min_voltage_V",
            "max_voltage_V",
            "soc_derate_band_pct",
        ],
    );
    let defaults = BatteryConfig::default();
    params.push(table_param(
        "ocv_table",
        "V",
        defaults.ocv.padded(true, true).data("1").expect("the default OCV table"),
        "open-circuit voltage by SOC (0-1)",
    ));
    params.push(p("ocv_mean", "V", ocv_mean(&defaults.ocv), "the OCV table's mean over 0-100 %"));
    params.extend([
        pe(
            "Q",
            "C",
            ite(
                gt(n("capacity_Ah"), c(0.0)),
                n("capacity_Ah"),
                max(n("capacity_kWh"), c(3600.0)) / max(n("ocv_mean"), c(1e-6) * n("unit_V")),
            ),
            "charge capacity: Charge Capacity, else Usable Capacity at the OCV table's mean voltage",
        ),
        pe("r0", "Ohm", max(n("internal_resistance_ohm"), c(1e-6)), "R0, at least 1 µΩ"),
        pe(
            "eta_c",
            "1",
            min(max(n("coulombic_efficiency_pct"), c(1e-3)), c(1.0)),
            "the share of the charging current stored",
        ),
        pe(
            "p_cap",
            "W",
            if cfg.limit_enforced {
                ite(
                    gt(n("output_power_limit_kW"), c(0.0)),
                    max(
                        c(0.0),
                        n("output_power_limit_kW")
                            * (c(1.0) - min(max(n("power_limit_margin_pct"), c(0.0)), c(1.0))),
                    ),
                    c(BIG) * n("unit_W"),
                )
            } else {
                c(BIG) * n("unit_W")
            },
            "the terminal power it is held to (Output Power Limit less its margin)",
        ),
    ]);
    uc.of("V");
    uc.of("W");
    let mut ports = vec![phys(id, "pos", "Pin"), phys(id, "neg", "Pin")];
    for p in [
        "sig_soc",
        "sig_voltage",
        "sig_current",
        "sig_power",
        "sig_losses",
        "sig_i_dis_limit",
        "sig_i_ch_limit",
    ] {
        ports.push(out(id, p));
    }
    ports.push(output("p_deliver", "W", "the most terminal power it gives now (its window)"));
    ports.push(output("p_absorb", "W", "the most terminal power it takes now (its window)"));
    let mut soc = state("soc", "1", 0.9, "state of charge, a fraction");
    soc.start = Some(n("initial_soc_pct"));
    soc.nominal = Some(1.0);
    let mut vars = vec![
        soc,
        guess("v", "V", n("ocv_mean"), "terminal voltage"),
        var("i", "A", "current, discharging positive"),
        var("ocv", "V", "open-circuit voltage"),
        var("soc_c", "1", "the SOC the table is read at (0-1)"),
        var("a_volt", "V", "the source voltage behind R0"),
        var("i_mpp", "A", "the maximum-power-point current"),
        var("i_dis_lim", "A", "the discharge current limit"),
        var("i_ch_lim", "A", "the charge current limit"),
        state("e_delivered", "J", 0.0, "energy delivered at the terminals"),
        state("e_recuperated", "J", 0.0, "energy taken back at the terminals"),
        state("e_losses", "J", 0.0, "energy lost inside"),
        state("e_chem", "J", 0.0, "chemical energy stored since the start, ∫OCV·dQ"),
    ];
    for v in vars.iter_mut().skip(9) {
        v.nominal = Some(1.0);
    }
    let v1 = if cfg.rc { n("v1") } else { c(0.0) * n("unit_V") };
    // charging or discharging switches are continuous at i = 0 (both sides
    // give 0): no event for them
    let mut eqs = vec![
        eq(n("v"), n("pos.v") - n("neg.v"), "its terminal voltage"),
        eq(c(0.0), n("pos.i") + n("neg.i"), "the current into pos leaves at neg"),
        eq(n("i"), -n("pos.i"), "discharging, current leaves at pos"),
        eq(n("soc_c"), clamp(n("soc"), c(0.0), c(1.0)), "the SOC read in the table"),
        eq(
            n("ocv"),
            lsim_ir::expr::table("ocv_table", vec![n("soc_c")]),
            "the OCV table at that SOC",
        ),
        eq(n("a_volt"), n("ocv") - v1.clone(), "the voltage behind R0"),
        eq(n("v"), n("a_volt") - n("r0") * n("i"), "R0's drop"),
        eq(
            der("soc"),
            -(ite(noev(ge(n("i"), c(0.0))), n("i"), n("eta_c") * n("i")) / n("Q")),
            "the SOC counts the charge, only a share of it stored while charging",
        ),
        eq(n("sig_soc"), n("soc"), "SOC"),
        eq(n("sig_voltage"), n("v"), "terminal voltage"),
        eq(n("sig_current"), n("i"), "current"),
        eq(n("sig_power"), n("v") * n("i"), "discharge power"),
        eq(
            n("sig_losses"),
            ite(noev(ge(n("i"), c(0.0))), c(1.0), n("eta_c")) * n("ocv") * n("i") - n("v") * n("i"),
            "what the cells give up less what reaches the terminals",
        ),
        eq(
            n("i_mpp"),
            max(n("a_volt"), c(0.0)) / (c(2.0) * n("r0")),
            "the maximum-power-point current",
        ),
        eq(der("e_delivered"), max(n("sig_power"), c(0.0)), "energy delivered"),
        eq(der("e_recuperated"), max(-n("sig_power"), c(0.0)), "energy recuperated"),
        eq(der("e_losses"), n("sig_losses"), "internal losses"),
        eq(
            der("e_chem"),
            -(n("ocv") * ite(noev(ge(n("i"), c(0.0))), n("i"), n("eta_c") * n("i"))),
            "the chemical energy follows the charge stored",
        ),
    ];
    let mut loss = n("r0") * n("i") * n("i")
        + ite(
            noev(lt(n("i"), c(0.0))),
            (c(1.0) - n("eta_c")) * n("ocv") * -n("i"),
            c(0.0) * n("unit_W"),
        );
    let mut stored = n("e_chem");
    if cfg.rc {
        params.push(pe("r1", "Ohm", max(n("rc_resistance_ohm"), c(1e-9)), "R1"));
        params.push(pe(
            "tau1",
            "s",
            max(n("rc_time_constant_s"), c(1e-9)),
            "the RC pair's time constant",
        ));
        vars.push(state("v1", "V", 0.0, "the RC pair's voltage"));
        eqs.push(eq(
            n("tau1") * der("v1"),
            n("r1") * n("i") - n("v1"),
            "the RC pair charges with the current (time constant R1·C1)",
        ));
        loss = loss + n("v1") * n("v1") / n("r1");
        stored = stored + c(0.5) * (n("tau1") / n("r1")) * n("v1") * n("v1");
    }
    // limits: current and voltage limits, the derating band
    let lim = cfg.limits.unwrap_or_default();
    let big_a = || c(BIG) * n("unit_A");
    uc.of("A");
    let mut dis = if lim.i_dis { n("max_discharge_current_A") } else { big_a() };
    let mut ch = if lim.i_ch { n("max_charge_current_A") } else { big_a() };
    if lim.v_min {
        dis = min(dis, max(c(0.0), (n("a_volt") - n("min_voltage_V")) / n("r0")));
    }
    if lim.v_max {
        ch = min(ch, max(c(0.0), (n("max_voltage_V") - n("a_volt")) / n("r0")));
    }
    if lim.band {
        let k_dis = clamp((n("soc") - n("min_soc_pct")) / n("soc_derate_band_pct"), c(0.0), c(1.0));
        let k_ch = clamp((c(1.0) - n("soc")) / n("soc_derate_band_pct"), c(0.0), c(1.0));
        let any_dis = lim.i_dis || lim.v_min;
        let any_ch = lim.i_ch || lim.v_max;
        let free_ch = c(2.0) * n("max_charge_power_kW")
            / (n("a_volt")
                + sqrt(n("a_volt") * n("a_volt") + c(4.0) * n("r0") * n("max_charge_power_kW")));
        dis = if any_dis { dis * k_dis } else { n("i_mpp") * k_dis };
        ch = if any_ch { ch * k_ch } else { free_ch * k_ch };
    }
    eqs.push(eq(n("i_dis_lim"), dis, "the discharge current limit"));
    eqs.push(eq(n("i_ch_lim"), ch, "the charge current limit"));
    eqs.push(eq(
        n("sig_i_dis_limit"),
        if lim.i_dis || lim.v_min || lim.band { n("i_dis_lim") } else { c(0.0) * n("unit_A") },
        "the discharge current limit (0 without one)",
    ));
    eqs.push(eq(
        n("sig_i_ch_limit"),
        if lim.i_ch || lim.v_max || lim.band { n("i_ch_lim") } else { c(0.0) * n("unit_A") },
        "the charge current limit (0 without one)",
    ));
    let i_d = min(n("i_mpp"), n("i_dis_lim"));
    eqs.push(eq(
        n("p_deliver"),
        ite(
            gt(n("soc"), n("min_soc_pct")),
            min(i_d.clone() * (n("a_volt") - n("r0") * i_d), n("p_cap")),
            c(0.0) * n("unit_W"),
        ),
        "it delivers up to its maximum-power point, its limits and its Output Power Limit, \
         down to its minimum SOC",
    ));
    let i_c = n("i_ch_lim");
    eqs.push(eq(
        n("p_absorb"),
        ite(
            lt(n("soc"), c(1.0)),
            min(n("max_charge_power_kW"), i_c.clone() * (n("a_volt") + n("r0") * i_c)),
            c(0.0) * n("unit_W"),
        ),
        "it takes up to its Max Charge Power and charge limits, until full",
    ));
    let mut all = uc.into_params();
    all.extend(params);
    let params = all;
    let mut fp = vec![cfg.rc as u8 as f64, cfg.limit_enforced as u8 as f64];
    if let Some(l) = cfg.limits {
        fp.extend([
            2.0,
            l.i_dis as u8 as f64,
            l.i_ch as u8 as f64,
            l.v_min as u8 as f64,
            l.v_max as u8 as f64,
            l.band as u8 as f64,
        ]);
    }
    ComponentDef {
        name: variant("Blocks.Battery", cfg.is_default_structure(), fp),
        doc: doc(id),
        ports,
        params,
        vars,
        equations: eqs,
        energy: EnergyDecl { stored: Some(stored), loss: Some(loss) },
        ..Default::default()
    }
}

/// The battery in its default configuration.
pub fn defaults() -> Vec<ComponentDef> {
    vec![battery(&BatteryConfig::default())]
}
