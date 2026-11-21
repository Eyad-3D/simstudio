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
//! SOC, or reach 100 %, within one 10 ms step) are a current that would
//! take the charge left above the minimum SOC (or up to full) in 10 ms
//! ([`TAU_SOC`]): a window that closes continuously as the SOC reaches its
//! limit, with no switch there.
//!
//! The OCV table is runtime data (the parameter `ocv_table`, linear as
//! today); the definition depends only on the structural choices (an RC
//! pair, which pack limits are set, whether the power limit holds).
//!
//! Energy books: stored = the chemical energy ∫OCV·dQ (a state, so the
//! OCV table can change without a rebuild) + ½·C1·v1²; loss = R0·i² +
//! v1²/R1 + the charge not stored, (1−η)·OCV·|i| while charging.
//!
//! *Defined By: Cells* ([`CellsConfig`], today's `CellPack` in CELLS mode)
//! builds the pack from a cell and an Ns × Np layout: Np × the cell's
//! charge, Ns × its open-circuit voltage, and R0 a variable: the cell's DC
//! resistance × its factor for the pulse length and SOC × its factor for
//! the cell temperature (the first Ambient's, 20 °C without one), per
//! series element ÷ Np, plus each element's interconnect and the
//! contactors. The pulse length is the time since the current last changed
//! direction (beyond ±1 µA, as today): a clean event at each reversal sets
//! its start (`t0`, `dir`), so the pulse is `time − t0`, an explicit
//! function of time; the peak current's duration running out is then a
//! time the run reaches exactly (an event), with no switch back until the
//! current reverses. An optional weak series element has its own SOC
//! (a share of the capacity) and a multiple of the resistance (its factor
//! read at its own SOC): it sags the pack, its cell voltage limits hold the
//! current, and it stops the string at the minimum SOC or full by the same
//! continuous rule as the pack's ([`TAU_SOC`]). The cells' current and
//! voltage limits and the derating band are today's, in continuous time
//! (the RC pair met as it is now, as for pack limits); the band reads the
//! lower of the pack's and the weak element's SOC for discharging, the
//! higher for charging (it empties and fills first). Outputs: the lowest
//! and highest cell voltage, and the state of power for 2, 10 and 30 s
//! pulses.

use super::*;
use crate::table::{Outside, Table1, Table2, UnitCarriers, table_param};
use lsim_ir::EnergyDecl;
use lsim_ir::ParamValue;

const ID: &str = "battery.generic";
/// "infinite" power or current for a limit that is not set
const BIG: f64 = 1e30;
/// The SOC limits' time constant, s: the window lets through at most the
/// current that would take the charge left above the minimum SOC (or up
/// to full) in this time, so it closes continuously as the SOC reaches the
/// limit. Today's engine's rule with its 10 ms step: the current that
/// reaches the limit within one step. A window that closed at a jump there
/// switched at the SOC: a hybrid whose engine charges it while its motors
/// draw drove the SOC back to the limit from each side, and the run
/// stopped with an event storm (DESIGN.md 8.2).
const TAU_SOC: f64 = 0.01;

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

/// A pack built from cells (*Defined By: Cells*; today's `CellPack` in
/// CELLS mode): the runtime data and the structural choices.
#[derive(Clone, Debug, PartialEq)]
pub struct CellsConfig {
    /// one cell's open-circuit voltage, V, by SOC 0-1
    pub ocv: Table1,
    /// the cell resistance's factor by pulse length (s, outer) and SOC
    /// (0-1, inner)
    pub r_factor: Table2,
    /// the cell resistance's factor by cell temperature (K)
    pub t_factor: Table1,
    /// cells in series (today's `max(1, round(Series Cells))`)
    pub ns: f64,
    /// cells in parallel
    pub np: f64,
    /// a weak series element: its capacity share or resistance multiple is
    /// not 1
    pub weak: bool,
    /// a continuous discharge current is set (above 0)
    pub i_dis: bool,
    /// a peak discharge current is set
    pub i_dis_peak: bool,
    /// a continuous charge current is set
    pub i_ch: bool,
    /// a peak charge current is set
    pub i_ch_peak: bool,
    /// a minimum cell voltage is set
    pub v_min: bool,
    /// a maximum cell voltage is set
    pub v_max: bool,
}

/// The cells' layout from today's values: (Ns, Np), each `max(1,
/// round(x))` with Python's rounding (half to even).
pub fn layout(p: &serde_json::Map<String, serde_json::Value>) -> (f64, f64) {
    let whole = |k: &str, d: f64| catalog::get(p, k, d).round_ties_even().max(1.0);
    (whole("series_cells", 96.0), whole("parallel_cells", 30.0))
}

/// The pack's mass estimate, kg: Ns × Np × the cell's mass × the packaging
/// factor (at least 1), as today's.
pub fn pack_mass_kg(p: &serde_json::Map<String, serde_json::Value>) -> f64 {
    let (ns, np) = layout(p);
    ns * np
        * catalog::get(p, "cell_mass_kg", 0.07).max(0.0)
        * catalog::get(p, "packaging_factor", 1.4).max(1.0)
}

/// Far past a Clamp axis' data, in its unit: the cells' resistance factors
/// are padded out to here (see [`CellsConfig::values`]).
const FAR: f64 = 1e9;

/// A Clamp axis padded far past both ends with its edge values: today's
/// engine books no read of a cell's resistance factors past their data
/// (it holds them there, "as VECTO does"; only an Error axis stops the
/// run), so the run is given data that reaches wherever it reads them and
/// records nothing past it. Another rule is left as it is (Linear: the
/// slope continues and the read is recorded; Error: the run stops).
fn held_far(t: &Table1) -> Table1 {
    if t.outside != Outside::Clamp || t.x.len() < 2 {
        return t.clone();
    }
    let mut t = t.clone();
    let (x0, y0) = (t.x[0], t.y[0]);
    let (xn, yn) = (t.x[t.x.len() - 1], t.y[t.y.len() - 1]);
    t.x.insert(0, x0 - FAR);
    t.y.insert(0, y0);
    t.x.push(xn + FAR);
    t.y.push(yn);
    t
}

impl CellsConfig {
    /// From today's merged parameters (Defined By: Cells); `outside`: an
    /// axis' outside rule by table and axis.
    pub fn from_params(
        p: &serde_json::Map<String, serde_json::Value>,
        outside: &dyn Fn(&str, usize) -> Outside,
    ) -> Result<Self, String> {
        use catalog::get;
        let ocv = Table1::from_json(
            p.get("cell_ocv_table").ok_or("no cell_ocv_table")?,
            outside("cell_ocv_table", 0),
        )?
        .scaled(0.01, 1.0);
        let r_factor = Table2::from_json(
            p.get("cell_resistance_factor").ok_or("no cell_resistance_factor")?,
            outside("cell_resistance_factor", 0),
            outside("cell_resistance_factor", 1),
        )?
        .scaled(1.0, 0.01, 1.0);
        let t_factor = Table1::from_json(
            p.get("cell_temperature_factor").ok_or("no cell_temperature_factor")?,
            outside("cell_temperature_factor", 0),
        )?
        .mapped(|x| x + 273.15, |y| y);
        let (ns, np) = layout(p);
        let set = |k: &str, d: f64| get(p, k, d) > 0.0;
        Ok(CellsConfig {
            ocv,
            r_factor,
            t_factor,
            ns,
            np,
            // (today's clamps first: a share of at least 0.1 % and at most
            // 1, a multiple of at least 0.5)
            weak: (get(p, "weak_cell_capacity_pct", 100.0) / 100.0).clamp(1e-3, 1.0) != 1.0
                || (get(p, "weak_cell_resistance_pct", 100.0) / 100.0).max(0.5) != 1.0,
            i_dis: set("cell_max_discharge_A", 15.0),
            i_dis_peak: set("cell_peak_discharge_A", 30.0),
            i_ch: set("cell_max_charge_A", 5.0),
            i_ch_peak: set("cell_peak_charge_A", 10.0),
            v_min: set("cell_min_voltage_V", 2.5),
            v_max: set("cell_max_voltage_V", 4.2),
        })
    }

    /// The pack's open-circuit voltage table: the cell's × Ns.
    pub fn pack_ocv(&self) -> Table1 {
        self.ocv.scaled(1.0, self.ns)
    }

    /// Its runtime data: the cell's tables and the layout. The cell's OCV
    /// is read at SOCs held to 0-1 (padded as the pack's is), the resistance
    /// factor's SOC axis likewise; their Clamp axes of pulse length and
    /// temperature reach far past their data ([`held_far`]).
    pub fn values(&self) -> Result<Vec<(String, ParamValue)>, String> {
        let mut rf = self.r_factor.clone();
        for s in &mut rf.sheets {
            *s = s.padded(Some(0.0), Some(1.0));
        }
        if rf.outer_outside == Outside::Clamp && rf.outer.len() > 1 {
            let (first, last) = (rf.sheets[0].clone(), rf.sheets[rf.sheets.len() - 1].clone());
            let (x0, xn) = (rf.outer[0], rf.outer[rf.outer.len() - 1]);
            rf.outer.insert(0, x0 - FAR);
            rf.sheets.insert(0, first);
            rf.outer.push(xn + FAR);
            rf.sheets.push(last);
        }
        Ok(vec![
            (
                "cell_ocv_table".into(),
                ParamValue::Table(self.ocv.padded(Some(0.0), Some(1.0)).data("1")?),
            ),
            ("cell_resistance_factor".into(), ParamValue::Table(rf.grid_data(["s", "1"])?)),
            (
                "cell_temperature_factor".into(),
                ParamValue::Table(held_far(&self.t_factor).data("K")?),
            ),
            ("ns".into(), ParamValue::Real(c(self.ns))),
            ("np".into(), ParamValue::Real(c(self.np))),
        ])
    }
}

impl Default for CellsConfig {
    fn default() -> Self {
        let p = catalog::merged(ID, &serde_json::Map::new());
        CellsConfig::from_params(&p, &|k, a| catalog::axis_outside(ID, k, a))
            .expect("the default cells")
    }
}

/// The battery's configuration.
#[derive(Clone, Debug, PartialEq)]
pub struct BatteryConfig {
    /// open-circuit voltage, V, by SOC as a fraction 0-1 (runtime data:
    /// [`BatteryConfig::values`]); a pack built from cells: the cell's ×
    /// Ns
    pub ocv: Table1,
    /// an RC pair (R1 and its time constant both above 0)
    pub rc: bool,
    /// pack limits, if any (Pack values)
    pub limits: Option<PackLimits>,
    /// the Output Power Limit holds the power (else it is only checked)
    pub limit_enforced: bool,
    /// fast mode's inverse model: its state of charge is flagged below the
    /// minimum SOC and above full (a car whose motors no window limits
    /// would drive on, its battery empty, without a word)
    pub inverse: bool,
    /// built from cells (Defined By: Cells), else Pack values
    pub cells: Option<CellsConfig>,
    /// a derating band is set on a pack built from cells (Pack values keep
    /// theirs in `limits`)
    pub cells_band: bool,
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
            inverse: false,
            cells: None,
            cells_band: false,
        }
    }
}

impl BatteryConfig {
    /// From today's merged parameters; `outside`: an axis' outside rule by
    /// table and axis (the part's own or the library's).
    pub fn from_params(
        p: &serde_json::Map<String, serde_json::Value>,
        outside: &dyn Fn(&str, usize) -> Outside,
    ) -> Result<Self, String> {
        use catalog::{flag, get, text};
        let rc = get(p, "rc_resistance_ohm", 0.0) > 0.0 && get(p, "rc_time_constant_s", 0.0) > 0.0;
        let limit_enforced = flag(p, "power_limit_enforced", true);
        let band = get(p, "soc_derate_band_pct", 0.0) > 0.0;
        if text(p, "pack_model", "Pack values") == "Cells" {
            let cells = CellsConfig::from_params(p, outside)?;
            return Ok(BatteryConfig {
                ocv: cells.pack_ocv(),
                rc,
                limits: None,
                limit_enforced,
                inverse: false,
                cells: Some(cells),
                cells_band: band,
            });
        }
        let ocv =
            Table1::from_json(p.get("ocv_table").ok_or("no ocv_table")?, outside("ocv_table", 0))?
                .scaled(0.01, 1.0);
        let limits = [
            get(p, "max_discharge_current_A", 0.0) > 0.0,
            get(p, "max_charge_current_A", 0.0) > 0.0,
            get(p, "min_voltage_V", 0.0) > 0.0,
            get(p, "max_voltage_V", 0.0) > 0.0,
            band,
        ];
        Ok(BatteryConfig {
            ocv,
            rc,
            limits: limits.iter().any(|x| *x).then_some(PackLimits {
                i_dis: limits[0],
                i_ch: limits[1],
                v_min: limits[2],
                v_max: limits[3],
                band: limits[4],
            }),
            limit_enforced,
            inverse: false,
            cells: None,
            cells_band: false,
        })
    }
}

/// The OCV table's mean over 0-100 % SOC (today's `ocv_mean`).
pub fn ocv_mean(t: &Table1) -> f64 {
    t.integral(0.0, 1.0)
}

impl BatteryConfig {
    /// The runtime data a part of this configuration takes: its OCV table
    /// and the table's mean; built from cells, the cell's tables and the
    /// layout instead of the pack's table.
    pub fn values(&self) -> Result<Vec<(String, ParamValue)>, String> {
        if let Some(cells) = &self.cells {
            let mut v = cells.values()?;
            v.push(("ocv_mean".into(), ParamValue::Real(c(ocv_mean(&self.ocv)))));
            return Ok(v);
        }
        Ok(vec![
            // read at the SOC held to 0-1: padded past an end its data
            // reach (one short of 0 or 100 % is watched there)
            (
                "ocv_table".into(),
                ParamValue::Table(self.ocv.padded(Some(0.0), Some(1.0)).data("1")?),
            ),
            ("ocv_mean".into(), ParamValue::Real(c(ocv_mean(&self.ocv)))),
        ])
    }

    /// The structural choices, for the definition's name.
    fn is_default_structure(&self) -> bool {
        let d = BatteryConfig::default();
        self.rc == d.rc
            && self.limits == d.limits
            && self.limit_enforced == d.limit_enforced
            && self.inverse == d.inverse
            && self.cells.is_none()
    }
}

/// The outputs of a pack built from cells (today's channels for it): the
/// lowest and highest cell voltage, and the state of power, discharging
/// and charging, for each of [`SOP_PULSES`].
const CELL_OUTPUTS: [&str; 8] = [
    "sig_v_cell_min",
    "sig_v_cell_max",
    "sig_p_dis_2s",
    "sig_p_dis_10s",
    "sig_p_dis_30s",
    "sig_p_ch_2s",
    "sig_p_ch_10s",
    "sig_p_ch_30s",
];
/// The state-of-power pulse lengths, s (today's `SOP_PULSES`).
const SOP_PULSES: [f64; 3] = [2.0, 10.0, 30.0];
/// A current smaller than this, A, does not start a new pulse (today's
/// `SIGN_A`).
const SIGN_A: f64 = 1e-6;
/// When the first pulse's direction is looked at, s: a `when` sees edges,
/// and a current that flows from the start has none (today's engine sets
/// it after its first step).
const START_LOOK: f64 = 1e-3;

impl CellsConfig {
    /// A discharge current or voltage limit is set.
    pub fn any_dis(&self) -> bool {
        self.i_dis || self.i_dis_peak || self.v_min
    }

    /// A charge current or voltage limit is set.
    pub fn any_ch(&self) -> bool {
        self.i_ch || self.i_ch_peak || self.v_max
    }

    /// Its discharge limit can end during a run: only the peak current is
    /// set, and past the peak's duration there is none (as today's).
    pub fn dis_lapses(&self) -> bool {
        self.i_dis_peak && !self.i_dis && !self.v_min
    }

    /// Its charge limit can end during a run ([`Self::dis_lapses`]).
    pub fn ch_lapses(&self) -> bool {
        self.i_ch_peak && !self.i_ch && !self.v_max
    }
}

/// The parameters of a pack built from cells: the cell's (today's clamps
/// as parameter expressions), its tables and the layout.
fn cells_params(uc: &mut UnitCarriers) -> Vec<ParamDecl> {
    let d = CellsConfig::default();
    let mut v = cps(
        ID,
        &[
            "cell_capacity_Ah",
            "cell_min_voltage_V",
            "cell_max_voltage_V",
            "cell_resistance_ohm",
            "cell_max_discharge_A",
            "cell_peak_discharge_A",
            "cell_max_charge_A",
            "cell_peak_charge_A",
            "cell_peak_duration_s",
            "interconnect_resistance_ohm",
            "contactor_resistance_ohm",
            "weak_cell_capacity_pct",
            "weak_cell_resistance_pct",
        ],
    );
    let data = d.values().expect("the default cells' data");
    for (name, unit, doc) in [
        ("cell_ocv_table", "V", "a cell's open-circuit voltage by SOC (0-1)"),
        ("cell_resistance_factor", "1", "the cell resistance's factor by pulse length and SOC"),
        ("cell_temperature_factor", "1", "the cell resistance's factor by cell temperature"),
    ] {
        let Some((_, ParamValue::Table(t))) = data.iter().find(|(k, _)| k == name) else {
            unreachable!("the default cells give {name}")
        };
        v.push(table_param(name, unit, t.clone(), doc));
    }
    let (uq, uohm, us) =
        (uc.of("C").expect("C"), uc.of("Ohm").expect("Ohm"), uc.of("s").expect("s"));
    v.extend([
        p("ns", "1", d.ns, "cells in series"),
        p("np", "1", d.np, "cells in parallel"),
        pe(
            "Q",
            "C",
            n("np") * max(n("cell_capacity_Ah"), c(3.6e-3) * uq),
            "charge capacity: Np cells' (each at least 1 µA·h)",
        ),
        pe(
            "dcr",
            "Ohm",
            max(n("cell_resistance_ohm"), c(1e-9) * uohm.clone()),
            "a cell's DC resistance, at least 1 nΩ",
        ),
        pe(
            "r_ic",
            "Ohm",
            max(n("interconnect_resistance_ohm"), c(0.0) * uohm.clone()),
            "per series element",
        ),
        pe("r_ct", "Ohm", max(n("contactor_resistance_ohm"), c(0.0) * uohm), "the contactors'"),
        pe(
            "weak_cap",
            "1",
            clamp(n("weak_cell_capacity_pct"), c(1e-3), c(1.0)),
            "the weak element's share of the capacity",
        ),
        pe(
            "weak_res",
            "1",
            max(n("weak_cell_resistance_pct"), c(0.5)),
            "the weak element's multiple of the resistance",
        ),
        pe(
            "peak_s",
            "s",
            max(n("cell_peak_duration_s"), c(0.0) * us),
            "how long a pulse may draw the peak currents",
        ),
    ]);
    v
}

/// when `cond`: a pulse starts in direction `dir` (+1, −1, or 0: none).
fn pulse_starts(cond: Expr, dir: Expr, label: &str) -> lsim_ir::EquationDecl {
    when(cond, &[("dir", dir), ("t0", time())], label)
}

/// A pack built from cells: its pulse, its cells' resistance and voltage,
/// the weak element, R0, the cells' current and voltage limits (returned:
/// discharge, charge, before the derating band), the band's factors, the
/// state of power and the cell voltages.
fn cells_part(
    cc: &CellsConfig,
    rc: bool,
    band: bool,
    vars: &mut Vec<lsim_ir::VarDecl>,
    eqs: &mut Vec<lsim_ir::EquationDecl>,
    uc: &mut UnitCarriers,
) -> (Expr, Expr) {
    let mut u = |s: &str| uc.of(s).expect("a unit");
    let (ua, us, uv, uohm) = (u("A"), u("s"), u("V"), u("Ohm"));
    let zero_a = || c(0.0) * ua.clone();
    let big_a = || c(BIG) * ua.clone();
    let v1 = if rc { n("v1") } else { c(0.0) * uv.clone() };
    let rf = |pulse: Expr, soc: &str| {
        lsim_ir::expr::table("cell_resistance_factor", vec![pulse, n(soc)])
    };
    // the pulse going on: since the current last changed direction
    vars.push(discrete("t0", "s", 0.0, "when the pulse going on started"));
    vars.push(discrete(
        "dir",
        "1",
        0.0,
        "the pulse's direction: +1 discharging, -1 charging, 0 none",
    ));
    let pulse = time() - n("t0");
    let sign_a = c(SIGN_A) * ua.clone();
    eqs.push(pulse_starts(
        ge(time(), c(START_LOOK) * us.clone()),
        ite(gt(n("i"), sign_a.clone()), c(1.0), ite(lt(n("i"), -sign_a.clone()), c(-1.0), c(0.0))),
        "the first pulse's direction, looked at once just after the start",
    ));
    // (each on the current alone, its direction looked at in the action:
    // a condition that crossed at an instant stays crossed there, so one that
    // reads `dir` would come back crossed when the other `when` sets it, and
    // the two would fire in turn where a reversal's new resistance turns the
    // current back: a cold pack at its charge limit against a heater's load)
    eqs.push(when(
        gt(n("i"), sign_a.clone()),
        &[("dir", c(1.0)), ("t0", ite(lt(pre("dir"), c(0.5)), time(), pre("t0")))],
        "a discharge pulse starts where the current turns to discharging",
    ));
    eqs.push(when(
        lt(n("i"), -sign_a),
        &[("dir", c(-1.0)), ("t0", ite(gt(pre("dir"), c(-0.5)), time(), pre("t0")))],
        "a charge pulse starts where the current turns to charging",
    ));
    // the cells' resistance and voltage
    vars.push(var("f_t", "1", "the cells' temperature factor"));
    eqs.push(eq(
        n("f_t"),
        lsim_ir::expr::table("cell_temperature_factor", vec![n("T_cell")]),
        "the resistance's factor at the cells' temperature",
    ));
    // (read at `time - t0`, a position that moves with time alone: the run
    // stops at its breakpoints, so no step spans a kink of the resistance)
    vars.push(var("r_pulse", "Ohm", "a cell's resistance in the pulse going on"));
    eqs.push(eq(
        n("r_pulse"),
        n("dcr") * rf(pulse.clone(), "soc_c") * n("f_t"),
        "the DC resistance × its factors for the pulse going on and the temperature",
    ));
    vars.push(var("r_cell0", "Ohm", "a cell's resistance in a pulse starting now"));
    eqs.push(eq(
        n("r_cell0"),
        n("dcr") * rf(c(0.0) * us.clone(), "soc_c") * n("f_t"),
        "the DC resistance × its factors for a pulse starting now and the temperature",
    ));
    vars.push(var("r_cell", "Ohm", "a cell's resistance now"));
    eqs.push(eq(
        n("r_cell"),
        ite(noev(gt(n("dir") * n("dir"), c(0.5))), n("r_pulse"), n("r_cell0")),
        "in the pulse going on (none yet: one starting now)",
    ));
    vars.push(var("v_cell", "V", "a cell's open-circuit voltage"));
    eqs.push(eq(
        n("v_cell"),
        lsim_ir::expr::table("cell_ocv_table", vec![n("soc_c")]),
        "the cell's OCV at the SOC",
    ));
    // (cell, its resistance multiple, the SOC its resistance factor is read
    // at, its resistance now and in a pulse starting now): the normal cells,
    // and the weak element
    let mut kinds = vec![(n("v_cell"), None, "soc_c", "r_cell", "r_cell0")];
    if cc.weak {
        let mut sw = state("soc_w", "1", 0.9, "the weak element's state of charge");
        sw.start = Some(n("initial_soc_pct"));
        sw.nominal = Some(1.0);
        vars.push(sw);
        vars.push(var("soc_wc", "1", "the weak element's SOC its OCV is read at (0-1)"));
        vars.push(var("v_weak", "V", "the weak element's open-circuit voltage per cell"));
        eqs.push(eq(
            der("soc_w"),
            -(ite(noev(ge(n("i"), c(0.0))), n("i"), n("eta_c") * n("i"))
                / (n("Q") * n("weak_cap"))),
            "its smaller capacity moves further with the same current",
        ));
        eqs.push(eq(n("soc_wc"), clamp(n("soc_w"), c(0.0), c(1.0)), "held to 0-1"));
        eqs.push(eq(
            n("v_weak"),
            lsim_ir::expr::table("cell_ocv_table", vec![n("soc_wc")]),
            "the cell's OCV at the weak element's SOC",
        ));
        // its resistance: the normal cell's, its factor read at its own SOC
        // (before its multiple)
        vars.push(var("r_pulse_w", "Ohm", "the weak cell's resistance in the pulse going on"));
        eqs.push(eq(
            n("r_pulse_w"),
            n("dcr") * rf(pulse.clone(), "soc_wc") * n("f_t"),
            "the DC resistance × its factors for the pulse going on at its SOC and the temperature",
        ));
        vars.push(var("r_cell0_w", "Ohm", "the weak cell's resistance in a pulse starting now"));
        eqs.push(eq(
            n("r_cell0_w"),
            n("dcr") * rf(c(0.0) * us.clone(), "soc_wc") * n("f_t"),
            "the DC resistance × its factors for a pulse starting now at its SOC and the temperature",
        ));
        vars.push(var("r_cell_w", "Ohm", "the weak cell's resistance now (before its multiple)"));
        eqs.push(eq(
            n("r_cell_w"),
            ite(noev(gt(n("dir") * n("dir"), c(0.5))), n("r_pulse_w"), n("r_cell0_w")),
            "in the pulse going on (none yet: one starting now)",
        ));
        kinds.push((n("v_weak"), Some(n("weak_res")), "soc_wc", "r_cell_w", "r_cell0_w"));
    }
    // R0 from a normal cell's resistance and the weak cell's (before its
    // multiple; none without a weak element)
    let weak = cc.weak;
    let pack_r = |r_cell: Expr, r_weak: Option<Expr>| {
        let series = match r_weak {
            Some(rw) if weak => (n("ns") - c(1.0)) * r_cell + n("weak_res") * rw,
            _ => n("ns") * r_cell,
        };
        series / n("np") + n("ns") * n("r_ic") + n("r_ct")
    };
    vars.push(var("r0", "Ohm", "R0 now"));
    eqs.push(eq(
        n("r0"),
        pack_r(n("r_cell"), cc.weak.then(|| n("r_cell_w"))),
        "Ns series elements of Np cells (the weak one × its multiple, at its SOC), their \
         interconnects and the contactors",
    ));
    // the cells' current limits: continuous, or the peak for a pulse up to
    // its duration (a pulse the other way: one starting now); 0 = none
    let cur = |cont: bool, peak: bool, k_cont: &str, k_peak: &str, over: Expr| match (cont, peak) {
        (true, true) => ite(over, n(k_cont), n(k_peak)) * n("np"),
        (false, true) => ite(over, big_a(), n(k_peak) * n("np")),
        (true, false) => n(k_cont) * n("np"),
        (false, false) => big_a(),
    };
    vars.push(var("i_dis_cur", "A", "the cells' discharge current limit"));
    vars.push(var("i_ch_cur", "A", "the cells' charge current limit"));
    eqs.push(eq(
        n("i_dis_cur"),
        cur(
            cc.i_dis,
            cc.i_dis_peak,
            "cell_max_discharge_A",
            "cell_peak_discharge_A",
            and(gt(n("dir"), c(0.5)), gt(pulse.clone(), n("peak_s"))),
        ),
        "Np × the continuous discharge current, or the peak's up to its duration",
    ));
    eqs.push(eq(
        n("i_ch_cur"),
        cur(
            cc.i_ch,
            cc.i_ch_peak,
            "cell_max_charge_A",
            "cell_peak_charge_A",
            and(lt(n("dir"), c(-0.5)), gt(pulse, n("peak_s"))),
        ),
        "Np × the continuous charge current, or the peak's up to its duration",
    ));
    // the cells' voltage limits, behind each cell's resistance in the pulse
    // that way and its share of the RC pair's voltage, for the normal cells
    // and the weak element
    let r_dis = |now: &str, start: &str| ite(noev(gt(n("dir"), c(0.5))), n(now), n(start));
    let r_ch = |now: &str, start: &str| ite(noev(lt(n("dir"), c(-0.5))), n(now), n(start));
    let v_rc_cell = v1.clone() / n("ns");
    let times = |r: Expr, k: &Option<Expr>| match k {
        Some(k) => r * k.clone(),
        None => r,
    };
    let (mut dis, mut ch) = (n("i_dis_cur"), n("i_ch_cur"));
    for (v_ocv, k, _, now, start) in &kinds {
        let v_src = v_ocv.clone() - v_rc_cell.clone();
        if cc.v_min {
            dis = min(
                dis,
                max(
                    zero_a(),
                    (v_src.clone() - n("cell_min_voltage_V")) * n("np")
                        / times(r_dis(now, start), k),
                ),
            );
        }
        if cc.v_max {
            ch = min(
                ch,
                max(
                    zero_a(),
                    (n("cell_max_voltage_V") - v_src) * n("np") / times(r_ch(now, start), k),
                ),
            );
        }
    }
    // the derating band's factors (the weak element's SOC, if lower, for
    // discharging; if higher, for charging: it empties and fills first)
    if band {
        let soc_d = if cc.weak { min(n("soc"), n("soc_w")) } else { n("soc") };
        let soc_ch = if cc.weak { max(n("soc"), n("soc_w")) } else { n("soc") };
        vars.push(var("k_dis", "1", "the derating band's discharge factor"));
        vars.push(var("k_ch", "1", "the derating band's charge factor"));
        eqs.push(eq(
            n("k_dis"),
            clamp((soc_d - n("min_soc_pct")) / n("soc_derate_band_pct"), c(0.0), c(1.0)),
            "1 above the band, falling to 0 at the minimum SOC",
        ));
        eqs.push(eq(
            n("k_ch"),
            clamp((c(1.0) - soc_ch) / n("soc_derate_band_pct"), c(0.0), c(1.0)),
            "1 below the band, falling to 0 at full",
        ));
    }
    // the state of power: for a pulse of each length from now, its current
    // limits at that pulse's resistance with the RC pair charged over it,
    // no more than the maximum-power point (today's `sop`)
    for d in SOP_PULSES {
        let ds = c(d) * us.clone();
        let (v_keep, r_rc) = if rc {
            let decay = exp(-(ds.clone() / n("tau1")));
            (n("v1") * decay.clone(), n("r1") * (c(1.0) - decay))
        } else {
            (c(0.0) * uv.clone(), c(0.0) * uohm.clone())
        };
        let rc_d = |soc: &str| n("dcr") * rf(ds.clone(), soc) * n("f_t");
        let r_tot = pack_r(rc_d("soc_c"), cc.weak.then(|| rc_d("soc_wc"))) + r_rc.clone();
        let a_d = n("ocv") - v_keep.clone();
        let i_mpp = max(a_d.clone(), c(0.0) * uv.clone()) / (c(2.0) * r_tot.clone());
        let within = le(ds.clone(), n("peak_s"));
        let cur_d = |cont: bool, peak: bool, k_cont: &str, k_peak: &str| match (cont, peak) {
            (true, true) => ite(within.clone(), n(k_peak), n(k_cont)) * n("np"),
            (false, true) => ite(within.clone(), n(k_peak) * n("np"), big_a()),
            (true, false) => n(k_cont) * n("np"),
            (false, false) => big_a(),
        };
        let mut dis_d =
            cur_d(cc.i_dis, cc.i_dis_peak, "cell_max_discharge_A", "cell_peak_discharge_A");
        let mut ch_d = cur_d(cc.i_ch, cc.i_ch_peak, "cell_max_charge_A", "cell_peak_charge_A");
        let r_rc_cell = r_rc.clone() * n("np") / n("ns");
        for (v_ocv, k, soc, _, _) in &kinds {
            let v_src = v_ocv.clone() - v_keep.clone() / n("ns");
            let r_e = times(rc_d(soc), k) + r_rc_cell.clone();
            if cc.v_min {
                dis_d = min(
                    dis_d,
                    max(
                        zero_a(),
                        (v_src.clone() - n("cell_min_voltage_V")) * n("np") / r_e.clone(),
                    ),
                );
            }
            if cc.v_max {
                ch_d = min(ch_d, max(zero_a(), (n("cell_max_voltage_V") - v_src) * n("np") / r_e));
            }
        }
        // (no limit, or the peak's only for a pulse past its duration: the
        // band lowers the maximum-power point instead, and no charge power
        // is given, as today's; looked at before the band's factor, which
        // would make the "none" look like a limit)
        let none = |i: &Expr| noev(ge(i.clone(), c(0.5 * BIG) * ua.clone()));
        let ch_none = none(&ch_d);
        if band {
            dis_d = if cc.dis_lapses() {
                ite(none(&dis_d), i_mpp.clone(), dis_d.clone()) * n("k_dis")
            } else if cc.any_dis() {
                dis_d * n("k_dis")
            } else {
                i_mpp.clone() * n("k_dis")
            };
            ch_d = ch_d * n("k_ch");
        }
        let i_dis = min(dis_d, i_mpp);
        let i_ch = if cc.any_ch() { ite(ch_none, zero_a(), ch_d) } else { zero_a() };
        let k = d as u32;
        eqs.push(eq(
            n(&format!("sig_p_dis_{k}s")),
            i_dis.clone() * (a_d.clone() - i_dis * r_tot.clone()),
            "the most power it can give over a pulse of this length from now",
        ));
        eqs.push(eq(
            n(&format!("sig_p_ch_{k}s")),
            i_ch.clone() * (a_d + i_ch * r_tot),
            "the most power it can take over a pulse of this length from now",
        ));
    }
    // the lowest and highest cell voltage
    let i_cell = n("i") / n("np");
    let v_of = |v_ocv: &Expr, k: &Option<Expr>, now: &str| {
        v_ocv.clone() - v_rc_cell.clone() - i_cell.clone() * times(n(now), k)
    };
    let vs: Vec<Expr> = kinds.iter().map(|(v, k, _, now, _)| v_of(v, k, now)).collect();
    let (lo, hi) = match vs.as_slice() {
        [a, b] => (min(a.clone(), b.clone()), max(a.clone(), b.clone())),
        [a] => (a.clone(), a.clone()),
        _ => unreachable!("the normal cells and at most one weak element"),
    };
    eqs.push(eq(n("sig_v_cell_min"), lo, "the lowest cell voltage now"));
    eqs.push(eq(n("sig_v_cell_max"), hi, "the highest cell voltage now"));
    (dis, ch)
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
    let cells = cfg.cells.as_ref();
    if cells.is_none() {
        params.push(table_param(
            "ocv_table",
            "V",
            defaults.ocv.padded(Some(0.0), Some(1.0)).data("1").expect("the default OCV table"),
            "open-circuit voltage by SOC (0-1)",
        ));
    }
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
        p("tau_soc", "s", TAU_SOC, "the SOC limits' time constant (a model constant)"),
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
        pe(
            "p_check",
            "W",
            if cfg.limit_enforced {
                n("p_cap") * c(1.0 - 1e-6)
            } else {
                ite(
                    gt(n("output_power_limit_kW"), c(0.0)),
                    n("output_power_limit_kW"),
                    c(BIG) * n("unit_W"),
                )
            },
            "above this terminal power it counts as held at its Output Power Limit (only \
             checked: as over it), as today's check counts it",
        ),
    ]);
    if cells.is_some() {
        // its charge from the cells, its R0 a variable (cells_part)
        params.retain(|p| p.name != "Q" && p.name != "r0");
        params.extend(cells_params(&mut uc));
    }
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
    if cells.is_some() {
        ports.push(input("T_cell", "K", "the cells' temperature (the first Ambient's)"));
        for p in CELL_OUTPUTS {
            ports.push(out(id, p));
        }
    }
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
        state(
            "t_held",
            "s",
            0.0,
            "time held at its Output Power Limit (over it, where the limit is only checked)",
        ),
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
        match cells {
            None => eq(
                n("ocv"),
                lsim_ir::expr::table("ocv_table", vec![n("soc_c")]),
                "the OCV table at that SOC",
            ),
            Some(cc) => eq(
                n("ocv"),
                if cc.weak {
                    n("ns") * n("v_cell") + (n("v_weak") - n("v_cell"))
                } else {
                    n("ns") * n("v_cell")
                },
                "Ns cells' open-circuit voltage (the weak element's in place of one)",
            ),
        },
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
        eq(
            der("t_held"),
            ite(gt(n("sig_power"), n("p_check")), c(1.0), c(0.0)),
            "the time it is held at its Output Power Limit",
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
    // a current limit that is none (BIG)
    let none = |i: &Expr| noev(ge(i.clone(), c(0.5 * BIG) * n("unit_A")));
    let (mut dis, mut ch, any_dis, any_ch, band) = match cells {
        None => {
            let mut dis = if lim.i_dis { n("max_discharge_current_A") } else { big_a() };
            let mut ch = if lim.i_ch { n("max_charge_current_A") } else { big_a() };
            if lim.v_min {
                dis = min(dis, max(c(0.0), (n("a_volt") - n("min_voltage_V")) / n("r0")));
            }
            if lim.v_max {
                ch = min(ch, max(c(0.0), (n("max_voltage_V") - n("a_volt")) / n("r0")));
            }
            (dis, ch, lim.i_dis || lim.v_min, lim.i_ch || lim.v_max, lim.band)
        }
        Some(cc) => {
            let (dis, ch) = cells_part(cc, cfg.rc, cfg.cells_band, &mut vars, &mut eqs, &mut uc);
            (dis, ch, cc.any_dis(), cc.any_ch(), cfg.cells_band)
        }
    };
    if band {
        let (k_dis, k_ch) = match cells {
            None => (
                clamp((n("soc") - n("min_soc_pct")) / n("soc_derate_band_pct"), c(0.0), c(1.0)),
                clamp((c(1.0) - n("soc")) / n("soc_derate_band_pct"), c(0.0), c(1.0)),
            ),
            Some(_) => (n("k_dis"), n("k_ch")),
        };
        let free_ch = c(2.0) * n("max_charge_power_kW")
            / (n("a_volt")
                + sqrt(n("a_volt") * n("a_volt") + c(4.0) * n("r0") * n("max_charge_power_kW")));
        // (built from cells with only a peak current set: past its duration
        // there is no limit, and the band lowers what the pack would give or
        // take without one, as with none set)
        dis = match cells {
            Some(cc) if cc.dis_lapses() => ite(none(&dis), n("i_mpp"), dis.clone()) * k_dis,
            _ if any_dis => dis * k_dis,
            _ => n("i_mpp") * k_dis,
        };
        ch = match cells {
            Some(cc) if cc.ch_lapses() => ite(none(&ch), free_ch, ch.clone()) * k_ch,
            _ if any_ch => ch * k_ch,
            _ => free_ch * k_ch,
        };
    }
    eqs.push(eq(n("i_dis_lim"), dis, "the discharge current limit"));
    eqs.push(eq(n("i_ch_lim"), ch, "the charge current limit"));
    // (0 without one: none set, or a peak's that has ended with no band)
    let shown = |lim: &str, any: bool, lapses: bool| {
        if lapses && !band {
            ite(none(&n(lim)), c(0.0) * n("unit_A"), n(lim))
        } else if any || band {
            n(lim)
        } else {
            c(0.0) * n("unit_A")
        }
    };
    eqs.push(eq(
        n("sig_i_dis_limit"),
        shown("i_dis_lim", any_dis, cells.is_some_and(|cc| cc.dis_lapses())),
        "the discharge current limit (0 without one)",
    ));
    eqs.push(eq(
        n("sig_i_ch_limit"),
        shown("i_ch_lim", any_ch, cells.is_some_and(|cc| cc.ch_lapses())),
        "the charge current limit (0 without one)",
    ));
    // the SOC limits: at most the current that would take the charge left
    // above the minimum SOC (or up to full) in TAU_SOC, today's "within the
    // step" rule in continuous time ([`TAU_SOC`]); a weak element stops the
    // string where it gets there (the same current flows through every
    // series element)
    // (variables of their own: the app says which limit held the motors)
    vars.push(var("i_floor", "A", "the discharge current its minimum SOC allows"));
    vars.push(var("i_full", "A", "the charge current its full charge allows"));
    let mut floor =
        max(c(0.0) * n("unit_A"), (n("soc") - n("min_soc_pct")) * n("Q") / n("tau_soc"));
    let mut full =
        max(c(0.0) * n("unit_A"), (c(1.0) - n("soc")) * n("Q") / (n("tau_soc") * n("eta_c")));
    if cells.is_some_and(|cc| cc.weak) {
        let q_w = n("Q") * n("weak_cap");
        floor = min(
            floor,
            max(c(0.0) * n("unit_A"), (n("soc_w") - n("min_soc_pct")) * q_w.clone() / n("tau_soc")),
        );
        full = min(
            full,
            max(c(0.0) * n("unit_A"), (c(1.0) - n("soc_w")) * q_w / (n("tau_soc") * n("eta_c"))),
        );
    }
    eqs.push(eq(
        n("i_floor"),
        floor,
        "it gives at most what takes the charge above its minimum SOC in tau_soc",
    ));
    eqs.push(eq(n("i_full"), full, "it takes at most what fills it in tau_soc"));
    let (i_floor, i_full) = (n("i_floor"), n("i_full"));
    let i_d = min(min(n("i_mpp"), n("i_dis_lim")), i_floor);
    eqs.push(eq(
        n("p_deliver"),
        min(i_d.clone() * (n("a_volt") - n("r0") * i_d), n("p_cap")),
        "it delivers up to its maximum-power point, its limits and its Output Power Limit, \
         less as it nears its minimum SOC",
    ));
    let i_c = min(n("i_ch_lim"), i_full);
    eqs.push(eq(
        n("p_absorb"),
        min(n("max_charge_power_kW"), i_c.clone() * (n("a_volt") + n("r0") * i_c)),
        "it takes up to its Max Charge Power and charge limits, less as it nears full",
    ));

    if cfg.inverse {
        vars.push(var("soc_in", "1", "its state of charge, within its minimum SOC … full"));
        eqs.push(eq(
            n("soc_in"),
            limit(n("soc"), n("min_soc_pct"), c(1.0)),
            "its state of charge, from its minimum SOC to full (fast mode flags it beyond: \
             the car could not have driven on)",
        ));
    }
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
    if cfg.inverse {
        fp.push(1.0);
    }
    if let Some(cc) = cells {
        fp.extend([
            3.0,
            cc.weak as u8 as f64,
            cc.i_dis as u8 as f64,
            cc.i_dis_peak as u8 as f64,
            cc.i_ch as u8 as f64,
            cc.i_ch_peak as u8 as f64,
            cc.v_min as u8 as f64,
            cc.v_max as u8 as f64,
            cfg.cells_band as u8 as f64,
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
