//! motor.emotor: the electric machine with its inverter, from maps.
//!
//! Today's model (`domains.py`: `motor_command`, `motor_torque`), in
//! continuous time:
//!
//! * the traction command (−1…1) asks for that share of the full-load
//!   torque at the supply voltage and speed (as a generator, the Q4 share
//!   of it); driving, the torque falls to zero over the last 2 % below the
//!   maximum speed;
//! * powered (a command other than exactly 0, a live supply above 1 V, at
//!   or below the maximum speed), its electrical power is torque × speed
//!   plus the loss map at that speed and torque; unpowered, it draws
//!   nothing and its drag torque brakes the shaft;
//! * with a supply window from its bus (`p_lo`, `p_hi`: what the source can
//!   give and take after the other loads), the torque is cut to the largest
//!   share of the command whose electrical power fits — today's bisection,
//!   here solved exactly — and when not even the spin losses fit, the
//!   inverter is off.
//!
//! The rotor's inertia is the block's own (scaled with the Torque Scale);
//! the maps carry today's scale factors (MOD-47). The maps are runtime
//! tables (linear, as today; [`MotorConfig::values`]), so the definition
//! depends only on whether a supply window limits it. Energy books:
//! stored ½·J·w², loss = electrical power − shaft power.

use super::*;
use crate::table::{Outside, Table1, Table2, UnitCarriers, table_param};
use lsim_ir::EnergyDecl;
use lsim_ir::ParamValue;

const ID: &str = "motor.emotor";
const RPM: f64 = std::f64::consts::PI / 30.0;
/// the share of the maximum speed over which the drive torque falls to 0
const SPEED_LIMIT_BAND: f64 = 0.02;

/// The motor's configuration: its maps (SI, scaled) and whether a supply
/// window limits it.
#[derive(Clone, Debug, PartialEq)]
pub struct MotorConfig {
    /// full-load torque, N·m, by supply voltage (V, outer) and speed (rad/s)
    pub full_load: Table2,
    /// motor + inverter loss, W, by speed (rad/s, outer) and torque (N·m)
    pub loss: Table2,
    /// drag torque while unpowered, N·m, by speed (rad/s)
    pub drag: Table1,
    /// Torque, Speed and Voltage Scale (today's MOD-47 factors)
    pub scales: [f64; 3],
    /// its bus hands it a supply window
    pub windowed: bool,
}

fn scale_pct(p: &serde_json::Map<String, serde_json::Value>, key: &str) -> f64 {
    let v = catalog::get(p, key, 100.0);
    if v > 0.0 { (v / 100.0).max(0.01) } else { 1.0 }
}

impl MotorConfig {
    /// From today's merged parameters and the element's per-table axis
    /// settings (`tableOutside`).
    pub fn from_params(
        p: &serde_json::Map<String, serde_json::Value>,
        outside: &dyn Fn(&str, usize) -> Outside,
        windowed: bool,
    ) -> Result<Self, String> {
        let (kt, kn, kv) = (
            scale_pct(p, "torque_scale_pct"),
            scale_pct(p, "speed_scale_pct"),
            scale_pct(p, "voltage_scale_pct"),
        );
        let fl = Table2::from_json(
            p.get("full_load_torque").ok_or("no full_load_torque")?,
            outside("full_load_torque", 0),
            outside("full_load_torque", 1),
        )?;
        let loss = Table2::from_json(
            p.get("power_loss").ok_or("no power_loss")?,
            outside("power_loss", 0),
            outside("power_loss", 1),
        )?;
        let drag = Table1::from_json(
            p.get("drag_torque").ok_or("no drag_torque")?,
            outside("drag_torque", 0),
        )?;
        Ok(MotorConfig {
            // voltage → (speed → torque): voltage × kV, speed × kn, torque × kT/kn
            full_load: fl.scaled(kv, kn * RPM, kt / kn),
            // speed → (torque → loss): speed × kn, torque × kT/kn, loss × kT
            loss: loss.scaled(kn * RPM, kt / kn, kt * 1e3),
            drag: drag.scaled(kn * RPM, kt / kn),
            scales: [kt, kn, kv],
            windowed,
        })
    }
}

impl MotorConfig {
    /// The runtime data a part of this configuration takes: its maps (SI,
    /// scaled), the Torque Scale and the full-load map's last speed.
    pub fn values(&self) -> Result<Vec<(String, ParamValue)>, String> {
        let last = self.full_load.inner_range().map(|r| r.1).unwrap_or(1e30);
        Ok(vec![
            // speeds and torque magnitudes are never below 0: the maps are
            // padded there, so the run loop's watch of their edges never
            // rests on an edge at standstill
            (
                "full_load_torque".into(),
                ParamValue::Table(
                    self.full_load
                        .padded((false, false), (true, false))
                        .grid_data(["V", "rad/s"])?,
                ),
            ),
            (
                "power_loss".into(),
                ParamValue::Table(
                    self.loss.padded((true, false), (true, false)).grid_data(["rad/s", "N.m"])?,
                ),
            ),
            ("drag_torque".into(), ParamValue::Table(self.drag.padded(true, false).data("rad/s")?)),
            ("kt".into(), ParamValue::Real(c(self.scales[0]))),
            ("kn".into(), ParamValue::Real(c(self.scales[1]))),
            ("w_fl_last".into(), ParamValue::Real(c(last))),
        ])
    }
}

impl Default for MotorConfig {
    fn default() -> Self {
        let p = catalog::merged(ID, &serde_json::Map::new());
        MotorConfig::from_params(&p, &|key, axis| catalog::axis_outside(ID, key, axis), false)
            .expect("default maps")
    }
}

/// The E-Motor block.
pub fn emotor(cfg: &MotorConfig) -> ComponentDef {
    let id = ID;
    let mut uc = UnitCarriers::default();
    let defaults = MotorConfig::default();
    let mut params = cps(id, &["max_speed_rpm", "inertia_kgm2", "q4_torque_scale_pct"]);
    for (name, value) in defaults.values().expect("the default maps") {
        let (unit, doc) = match name.as_str() {
            "full_load_torque" => ("N.m", "full-load torque by supply voltage and speed"),
            "power_loss" => ("W", "motor and inverter loss by speed and torque"),
            "drag_torque" => ("N.m", "drag torque while unpowered, by speed"),
            "kt" => ("1", "Torque Scale (its inertia and maps grow with it)"),
            "kn" => ("1", "Speed Scale (its maximum speed grows with it)"),
            _ => ("rad/s", "the full-load map's last speed"),
        };
        params.push(match value {
            ParamValue::Table(t) => table_param(&name, unit, t, doc),
            ParamValue::Real(Expr::Const(v)) => p(&name, unit, v, doc),
            _ => unreachable!("tables and numbers"),
        });
    }
    params.push(p("w0", "rad/s", 0.0, "the rotor's speed at the start"));
    params.push(p("w_slow", "rad/s", 0.1, "below this speed the drag fades with the speed"));
    params.push(pe(
        "w_max",
        "rad/s",
        ite(gt(n("max_speed_rpm"), c(0.0)), n("max_speed_rpm") * n("kn"), n("w_fl_last")),
        "the maximum speed: Maximum Speed, else the full-load curve's last speed",
    ));
    params.push(pe(
        "J",
        "kg.m2",
        n("inertia_kgm2") * n("kt"),
        "the rotor's inertia, as the Torque Scale makes it",
    ));
    uc.of("rad/s");
    uc.of("V");
    uc.of("N.m");
    uc.of("W");
    let mut ports = vec![
        phys(id, "pos", "Pin"),
        phys(id, "neg", "Pin"),
        phys(id, "shaft", "Flange"),
        inp(id, "sig_demand_in"),
    ];
    if cfg.windowed {
        ports.push(input("p_hi", "W", "the most electrical power its supply gives it now"));
        ports.push(input(
            "p_lo",
            "W",
            "the most electrical power (negative) its supply takes back now",
        ));
    }
    for p in ["sig_speed", "sig_torque", "sig_mech_power", "sig_elec_power", "sig_losses"] {
        ports.push(out(id, p));
    }
    ports.push(output(
        "t_regen",
        "N.m",
        "its generator torque limit now (for the driver's blending)",
    ));
    ports.push(output(
        "p_request",
        "W",
        "the electrical power its command asks for (for the bus manager)",
    ));
    let mut w = state("w", "rad/s", 0.0, "rotor speed");
    w.start = Some(n("w0"));
    let mut vars = vec![
        w,
        guess("v", "V", c(400.0) * n("unit_V"), "supply voltage"),
        var("i", "A", "current it takes in at pos"),
        var("wa", "rad/s", "speed magnitude"),
        var("dem", "1", "the command, held to −1…1"),
        var("t_fl", "N.m", "full-load torque at this voltage and speed"),
        var("t_cmd", "N.m", "the torque the command asks for"),
        var("p_req", "W", "the electrical power that torque takes"),
        var("alive", "1", "1 while the inverter may run"),
        var("on", "1", "1 while the inverter runs"),
        var("T", "N.m", "shaft torque it produces"),
        guess("p_elec", "W", c(0.0) * n("unit_W"), "electrical power"),
        state("t_limited", "s", 0.0, "time the supply held its torque below the command"),
        state(
            "e_regen_lost",
            "J",
            0.0,
            "recuperation its command asked for that the supply could not take",
        ),
    ];
    for v in vars.iter_mut().skip(12) {
        v.nominal = Some(1.0);
    }
    let uc2 = uc;
    let t_fl_expr = lsim_ir::expr::table("full_load_torque", vec![n("v"), n("wa")]);
    let loss_at = |torque: Expr| lsim_ir::expr::table("power_loss", vec![n("wa"), torque]);
    let wmax = n("w_max");
    let band = c(SPEED_LIMIT_BAND);
    // the continuous switches (equal on both sides of their boundary) need
    // no event: their relations are under noEvent
    let t_drive = ite(
        noev(gt(n("wa"), wmax.clone() * (c(1.0) - band.clone()))),
        n("t_fl") * max(c(0.0), wmax.clone() - n("wa")) / (band * wmax.clone()),
        n("t_fl"),
    );
    let mut eqs = vec![
        eq(n("v"), n("pos.v") - n("neg.v"), "its supply voltage"),
        eq(c(0.0), n("pos.i") + n("neg.i"), "the current into pos leaves at neg"),
        eq(n("i"), n("pos.i"), "the current it takes"),
        eq(n("w"), n("shaft.w"), "the rotor turns with the shaft"),
        eq(n("J") * der("w"), n("T") + n("shaft.tau"), "the rotor's inertia"),
        eq(n("wa"), noev(abs(n("w"))), "speed magnitude (its kink at 0 needs no event)"),
        eq(n("dem"), clamp(n("sig_demand_in"), c(-1.0), c(1.0)), "the command, held to −1…1"),
        eq(n("t_fl"), t_fl_expr, "the full-load map at this voltage and speed"),
        eq(
            n("t_cmd"),
            ite(
                noev(gt(n("dem"), c(0.0))),
                n("dem") * t_drive,
                n("dem") * n("t_fl") * n("q4_torque_scale_pct"),
            ),
            "the command's share of the full-load torque (as a generator, its Q4 share)",
        ),
        eq(
            n("p_req"),
            n("t_cmd") * n("w") + loss_at(noev(abs(n("t_cmd")))),
            "shaft power plus the loss map",
        ),
        eq(
            n("alive"),
            ite(
                and(
                    // a command of exactly 0 is an instant (no event)
                    and(gt(n("v"), c(1.0) * n("unit_V")), noev(gt(abs(n("dem")), c(0.0)))),
                    le(n("wa"), wmax.clone() * c(1.0 + 1e-9)),
                ),
                c(1.0),
                c(0.0),
            ),
            "the inverter runs for a command other than 0, a live supply, up to the maximum speed",
        ),
    ];
    // unpowered, the drag torque acts against the motion; below 0.1 rad/s
    // (about 1 rpm) it fades in proportion to the speed, so a rotor coming
    // to rest settles at 0 instead of chattering about it (a bare
    // −sign(w)·drag switches every time the speed crosses 0; a Coulomb
    // friction with stiction would hold the driveline together with its
    // stuck brakes, which is statically indeterminate)
    let unpowered = -(lsim_ir::expr::table("drag_torque", vec![n("wa")])
        * noev(n("w") / max(noev(abs(n("w"))), n("w_slow"))));

    if cfg.windowed {
        vars.push(guess("frac", "1", c(1.0), "the share of the command its supply allows"));
        let l0 = loss_at(c(0.0) * n("unit_N_m"));
        eqs.push(eq(
            n("on"),
            ite(
                and(gt(n("alive"), c(0.5)), and(le(l0.clone(), n("p_hi")), ge(l0, n("p_lo")))),
                c(1.0),
                c(0.0),
            ),
            "with no room even for its spin losses, the inverter is off",
        ));
        let p_of = n("frac") * n("t_cmd") * n("w") + loss_at(noev(abs(n("frac") * n("t_cmd"))));
        eqs.push(eq(
            c(0.0),
            ite(
                noev(gt(n("p_req"), n("p_hi"))),
                p_of.clone() - n("p_hi"),
                ite(
                    noev(lt(n("p_req"), n("p_lo"))),
                    p_of - n("p_lo"),
                    (n("frac") - c(1.0)) * n("unit_W"),
                ),
            ),
            "the torque is cut to the largest share of the command whose power fits its supply",
        ));
        eqs.push(eq(
            n("T"),
            ite(gt(n("on"), c(0.5)), n("frac") * n("t_cmd"), unpowered),
            "powered, the allowed torque; unpowered, its drag against the motion",
        ));
        eqs.push(eq(
            der("t_limited"),
            ite(
                and(
                    gt(n("alive"), c(0.5)),
                    noev(or(gt(n("p_req"), n("p_hi")), lt(n("p_req"), n("p_lo")))),
                ),
                c(1.0),
                c(0.0),
            ),
            "time held below its command",
        ));
        eqs.push(eq(
            der("e_regen_lost"),
            ite(
                and(
                    gt(n("alive"), c(0.5)),
                    noev(lt(n("p_req"), min(n("p_lo"), c(0.0) * n("unit_W")))),
                ),
                n("p_elec") - n("p_req"),
                c(0.0) * n("unit_W"),
            ),
            "recuperation the supply could not take",
        ));
    } else {
        eqs.push(eq(n("on"), n("alive"), "it runs whenever it may"));
        eqs.push(eq(
            n("T"),
            ite(gt(n("on"), c(0.5)), n("t_cmd"), unpowered),
            "powered, the command's torque; unpowered, its drag against the motion",
        ));
        eqs.push(eq(der("t_limited"), c(0.0), "never held back"));
        eqs.push(eq(der("e_regen_lost"), c(0.0) * n("unit_W"), "never held back"));
    }
    eqs.extend([
        eq(
            n("p_elec"),
            ite(
                gt(n("on"), c(0.5)),
                n("T") * n("w") + loss_at(noev(abs(n("T")))),
                c(0.0) * n("unit_W"),
            ),
            "powered: shaft power plus the loss map; unpowered: nothing",
        ),
        eq(n("v") * n("i"), n("p_elec"), "it takes that power from its supply"),
        eq(n("sig_speed"), n("wa"), "shaft speed"),
        eq(n("sig_torque"), n("T"), "shaft torque"),
        eq(n("sig_mech_power"), n("T") * n("w"), "mechanical power"),
        eq(n("sig_elec_power"), n("p_elec"), "electrical power"),
        eq(n("sig_losses"), n("p_elec") - n("T") * n("w"), "losses"),
        eq(
            n("t_regen"),
            ite(
                and(gt(n("v"), c(1.0) * n("unit_V")), le(n("wa"), wmax * c(1.0 + 1e-9))),
                n("t_fl") * n("q4_torque_scale_pct"),
                c(0.0) * n("unit_N_m"),
            ),
            "its generator torque limit",
        ),
        eq(n("p_request"), n("alive") * n("p_req"), "the power its command asks for"),
    ]);
    let mut all = uc2.into_params();
    all.extend(params);
    ComponentDef {
        name: variant("Blocks.EMotor", !cfg.windowed, [cfg.windowed as u8 as f64]),
        doc: doc(id),
        ports,
        params: all,
        vars,
        equations: eqs,
        energy: EnergyDecl {
            stored: Some(c(0.5) * n("J") * n("w") * n("w")),
            loss: Some(n("v") * n("i") - n("T") * n("w")),
        },
        ..Default::default()
    }
}

/// The E-Motor in its default configuration.
pub fn defaults() -> Vec<ComponentDef> {
    vec![emotor(&MotorConfig::default())]
}
