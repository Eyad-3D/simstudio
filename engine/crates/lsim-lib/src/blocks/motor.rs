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
//!   here solved exactly — and when not even the spin losses fit, it takes
//!   its whole window and its shaft feeds the rest of its spin losses (a
//!   torque against the motion, fading below 0.1 rad/s as the drag does and
//!   at most its generator torque limit; below that the losses its window
//!   cannot pay are not paid). A short supply does not switch the inverter
//!   off: off on such a window, it switched on and off without end as a
//!   battery's window tapered through its spin losses.
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
/// the command above which (either way) the inverter runs near standstill
/// (below `w_slow`): a car standing at its target from the start holds a
/// Driver's command of round-off (its integral picks up about 1e-11 in the
/// start's first microseconds: 3.8e-12), which ran the inverter at its
/// standstill loss for as long as the car stood there (the UDDS's first 20 s,
/// the WLTC's 11 s), where at every later
/// stop the command is exactly 0 and the inverter off. Below this, a command
/// counts as none. Moving, a command passes it in an instant and the
/// inverter is not held off for it: the bound keeps every zero crossing of
/// a moving car's command as it was (at any speed it added events to every
/// battery cycle: 17 -> 52 on the aero BEV's HWFET, 7 with the bound).
pub const CMD_ON: f64 = 1e-9;
/// how far below its maximum speed an E-Motor driven above it runs again:
/// above the maximum its inverter is off and its drag brakes the rotor; on
/// again at the maximum itself, the taper's torque there is 0, so switching
/// on takes away the drag, and a light driveline (the rotor and wheels, held
/// to the car by their tyres' slip) was lifted straight back over it: the
/// inverter switched without end (an event storm). Back on this far below,
/// the taper gives 5 % of the full-load torque, and the motor settles inside
/// its band.
pub const SPEED_BACK_ON: f64 = 1e-3;

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
    /// fast mode's inverse model, which solves for the command: the
    /// torque follows the command everywhere, so the command is always
    /// determined (a torque that did not depend on it would leave it free).
    /// Its inverter runs at a command of 0 too (an instant in a forward
    /// run), but it draws nothing while its shaft stands and its command
    /// is 0 to round-off (a car standing with nothing to drive, whose
    /// forward run holds its brake there: a command of 0, the inverter
    /// off); the command is not held to −1…1 (it leaves that range only
    /// where the driver's pedal is beyond its own limits, which fast mode
    /// flags); and a supply window does not cut the torque: beyond what
    /// the supply gives, fast mode flags the electrical power, and of
    /// recuperation beyond what it takes the supply takes what it can and
    /// the rest is lost (as the friction brakes take it in a forward run,
    /// where the driver brakes harder)
    pub inverse: bool,
    /// a windowed motor a Driver blends its recuperation for with friction
    /// brakes, which take what its supply cuts: it says the share of its
    /// command its supply allows (`k_supply`), cuts its recuperation 1e-5
    /// inside its window, and its cut is neither time held back nor
    /// recuperation lost (its `blended` parameter, 1)
    pub blended: bool,
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
            inverse: false,
            blended: false,
        })
    }
}

impl MotorConfig {
    /// The runtime data a part of this configuration takes: its maps (SI,
    /// scaled), the Torque Scale and the full-load map's last speed.
    pub fn values(&self) -> Result<Vec<(String, ParamValue)>, String> {
        let last = self.full_load.inner_range().map(|r| r.1).unwrap_or(1e30);
        Ok(vec![
            // speeds and torque magnitudes are never below 0: a map whose
            // data start at 0 is padded there, so the run loop's watch of
            // its edges never rests on an edge at standstill (one whose data
            // start higher is watched there, as today's engine reads it)
            (
                "full_load_torque".into(),
                ParamValue::Table(
                    self.full_load
                        .padded((None, None), (Some(0.0), None))
                        .grid_data(["V", "rad/s"])?,
                ),
            ),
            (
                "power_loss".into(),
                ParamValue::Table(
                    self.loss
                        .padded((Some(0.0), None), (Some(0.0), None))
                        .grid_data(["rad/s", "N.m"])?,
                ),
            ),
            (
                "drag_torque".into(),
                ParamValue::Table(self.drag.padded(Some(0.0), None).data("rad/s")?),
            ),
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
    if cfg.windowed && !cfg.inverse && cfg.blended {
        params.push(p(
            "blended",
            "1",
            1.0,
            "1 where a Driver blends its recuperation with friction brakes, which take what \
             its supply cuts (that is then not recuperation lost)",
        ));
    }
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
        if !cfg.inverse && cfg.blended {
            ports.push(output(
                "k_supply",
                "1",
                "the share of its command its supply allows (1: all of it)",
            ));
        }
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
    let w = crate::rotational::driveline_speed("w", "w0", "rotor speed");
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
    let wmax = n("w_max");
    // the maps are read at the speed held to the maximum speed: above it the
    // inverter is off and their values are not used, so a table that ends
    // there is never read past its data (and a stop on its axis is not hit)
    let w_map = noev(min(n("wa"), wmax.clone()));
    let t_fl_expr = lsim_ir::expr::table("full_load_torque", vec![n("v"), w_map.clone()]);
    let loss_at = |torque: Expr| lsim_ir::expr::table("power_loss", vec![w_map.clone(), torque]);
    let band = c(SPEED_LIMIT_BAND);
    // the continuous switches (equal on both sides of their boundary) need
    // no event: their relations are under noEvent
    let t_drive = ite(
        noev(gt(n("wa"), wmax.clone() * (c(1.0) - band.clone()))),
        n("t_fl") * max(c(0.0), wmax.clone() - n("wa")) / (band * wmax.clone()),
        n("t_fl"),
    );
    // above its maximum speed: from where it passes it upwards (or starts
    // above it) until it is SPEED_BACK_ON below it
    vars.push(lsim_ir::VarDecl {
        start: Some(ite(gt(abs(n("w0")), wmax.clone() * c(1.0 + 1e-9)), c(1.0), c(0.0))),
        ..lsim_ir::component::build::discrete(
            "over",
            "1",
            0.0,
            "1 from above its maximum speed until it is back 0.1 % below it",
        )
    });
    let mut eqs = vec![
        when(
            gt(n("wa"), wmax.clone() * c(1.0 + 1e-9)),
            &[("over", c(1.0))],
            "above its maximum speed: the inverter is off",
        ),
        when(
            lt(n("wa"), wmax.clone() * c(1.0 - SPEED_BACK_ON)),
            &[("over", c(0.0))],
            "back 0.1 % below its maximum speed: it may run again",
        ),
        eq(n("v"), n("pos.v") - n("neg.v"), "its supply voltage"),
        eq(c(0.0), n("pos.i") + n("neg.i"), "the current into pos leaves at neg"),
        eq(n("i"), n("pos.i"), "the current it takes"),
        eq(n("w"), n("shaft.w"), "the rotor turns with the shaft"),
        eq(n("J") * der("w"), n("T") + n("shaft.tau"), "the rotor's inertia"),
        eq(n("wa"), noev(abs(n("w"))), "speed magnitude (its kink at 0 needs no event)"),
        if cfg.inverse {
            eq(n("dem"), n("sig_demand_in"), "the command (fast mode: as the driver gives it)")
        } else {
            eq(n("dem"), clamp(n("sig_demand_in"), c(-1.0), c(1.0)), "the command, held to −1…1")
        },
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
                    // a command of 0 is an instant (no event), and near
                    // standstill one within CMD_ON of 0 is round-off; in
                    // fast mode the inverter runs at 0 too
                    if cfg.inverse {
                        gt(n("v"), c(1.0) * n("unit_V"))
                    } else {
                        and(
                            gt(n("v"), c(1.0) * n("unit_V")),
                            noev(gt(
                                abs(n("dem")),
                                ite(lt(n("wa"), n("w_slow")), c(CMD_ON), c(0.0)),
                            )),
                        )
                    },
                    lt(n("over"), c(0.5)),
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

    // fast mode: idle while the shaft stands and the command is 0 to
    // round-off (the solved command is then 0 or a few ulps from it)
    let idle = noev(and(le(n("wa"), c(0.0) * n("unit_rad_s")), le(abs(n("dem")), c(1e-9))));
    let elec = |p: Expr| {
        if cfg.inverse { ite(idle.clone(), c(0.0) * n("unit_W"), p) } else { p }
    };
    // (the windowed motor's short supply, for its electrical power below)
    let mut short_of: Option<Expr> = None;
    if cfg.windowed && cfg.inverse {
        eqs.push(eq(n("on"), n("alive"), "it runs whenever it may"));
        eqs.push(eq(
            n("T"),
            ite(gt(n("on"), c(0.5)), n("t_cmd"), unpowered.clone()),
            "powered, the command's torque; unpowered, its drag against the motion",
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
            "time beyond its supply's window",
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
    } else if cfg.windowed {
        vars.push(guess("frac", "1", c(1.0), "the share of the command its supply allows"));
        // its spin losses (the loss map at zero torque), one variable: the
        // code computes every branch of an `if` and does not share lookups
        vars.push(var("l0", "W", "its spin losses: its loss map at zero torque"));
        eqs.push(eq(n("l0"), loss_at(c(0.0) * n("unit_N_m")), "its spin losses at its speed"));
        let l0 = n("l0");
        // a short supply cuts its torque, down to its shaft feeding its spin
        // losses (below), and does not switch the inverter off: off on a
        // window that did not cover its spin losses, it switched on and off
        // without end as a battery's window tapered through them; and a
        // switch near standstill, where the shaft cannot feed them, did too
        // in a hybrid whose engine keeps the shaft turning. Near standstill
        // the losses its window cannot pay are not paid (the feeding torque
        // fades below w_slow, as the drag does)
        eqs.push(eq(
            n("on"),
            ite(gt(n("alive"), c(0.5)), c(1.0), c(0.0)),
            "it runs whenever it may (a short supply cuts its torque)",
        ));
        // short: its window does not cover even its spin losses
        let short = || noev(and(gt(n("p_req"), n("p_hi")), lt(n("p_hi"), l0.clone())));
        let p_of = n("frac") * n("t_cmd") * n("w") + loss_at(noev(abs(n("frac") * n("t_cmd"))));
        // what it feeds back is cut to: its window; a blended motor's, by
        // 1e-5 of it inside (more than the solver's tolerance on the share),
        // so that its supply takes no more than its window allows (exactly
        // its window where it is not blended: blended is 0)
        let lo_edge = if cfg.blended {
            n("p_lo") + n("blended") * c(1e-5) * noev(max(abs(n("p_lo")), c(1.0) * n("unit_W")))
        } else {
            n("p_lo")
        };
        // off, the share is not used and is held at 1 (its equation need
        // have no root then). Short, it blends the command's torque into the
        // torque that feeds the spin losses from the shaft: 1 where the
        // command just fits its window (the torque the command's, as uncut),
        // 0 where the window reaches the spin losses (as the cut leaves it),
        // continuous between (a request at its window hovers by the solver's
        // noise; its window is below the spin losses throughout)
        let blend = min(c(1.0), max(c(0.0), (l0.clone() - n("p_req")) / (l0.clone() - n("p_hi"))));
        eqs.push(eq(
            c(0.0),
            ite(
                and(gt(n("on"), c(0.5)), short()),
                (n("frac") - blend) * n("unit_W"),
                ite(
                    and(gt(n("on"), c(0.5)), noev(gt(n("p_req"), n("p_hi")))),
                    p_of.clone() - n("p_hi"),
                    ite(
                        and(gt(n("on"), c(0.5)), noev(lt(n("p_req"), lo_edge.clone()))),
                        p_of - lo_edge,
                        (n("frac") - c(1.0)) * n("unit_W"),
                    ),
                ),
            ),
            "the torque is cut to the largest share of the command whose power fits its supply \
             (off, the whole command: it is not used)",
        ));
        // the torque that feeds the spin losses its supply does not cover:
        // against the motion, (l0 - p_hi)/|w|, fading below w_slow as the
        // drag does, at most its generator torque limit
        let t_feed = -(min(
            (l0.clone() - n("p_hi")) / max(n("wa"), n("w_slow")),
            n("t_fl") * n("q4_torque_scale_pct"),
        ) * noev(n("w") / max(noev(abs(n("w"))), n("w_slow"))));
        eqs.push(eq(
            n("T"),
            ite(
                gt(n("on"), c(0.5)),
                ite(
                    short(),
                    n("frac") * n("t_cmd") + (c(1.0) - n("frac")) * t_feed,
                    n("frac") * n("t_cmd"),
                ),
                unpowered,
            ),
            "powered, the allowed torque (short of even its spin losses, the shaft feeds \
             them); unpowered, its drag against the motion",
        ));
        short_of = Some(short());
        // (a blended motor's recuperation its supply cuts the friction
        // brakes take: neither time held back nor recuperation lost)
        let (below, lost) = if cfg.blended {
            eqs.push(eq(n("k_supply"), n("frac"), "the share of its command its supply allows"));
            let own = || noev(lt(n("blended"), c(0.5)));
            (
                noev(or(gt(n("p_req"), n("p_hi")), and(own(), lt(n("p_req"), n("p_lo"))))),
                and(own(), noev(lt(n("p_req"), min(n("p_lo"), c(0.0) * n("unit_W"))))),
            )
        } else {
            (
                noev(or(gt(n("p_req"), n("p_hi")), lt(n("p_req"), n("p_lo")))),
                noev(lt(n("p_req"), min(n("p_lo"), c(0.0) * n("unit_W")))),
            )
        };
        eqs.push(eq(
            der("t_limited"),
            ite(and(gt(n("alive"), c(0.5)), below), c(1.0), c(0.0)),
            "time held below its command",
        ));
        eqs.push(eq(
            der("e_regen_lost"),
            ite(
                and(gt(n("alive"), c(0.5)), lost),
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
    let p_shaft_loss = n("T") * n("w") + loss_at(noev(abs(n("T"))));
    eqs.push(if cfg.windowed && cfg.inverse {
        eq(
            n("p_elec"),
            ite(
                gt(n("on"), c(0.5)),
                // flagged above p_hi only: its lower bound the value itself
                // where below (an open bound, −1e30 W, would set the flags'
                // tolerance, relative to the bounds too, at 1e21 W)
                elec(max(
                    limit(p_shaft_loss.clone(), min(p_shaft_loss, n("p_hi")), n("p_hi")),
                    n("p_lo"),
                )),
                c(0.0) * n("unit_W"),
            ),
            "its electrical power: shaft power plus the loss map, up to what its supply \
             gives (fast mode flags it beyond), of recuperation what its supply takes; \
             unpowered or idle, nothing",
        )
    } else {
        // short, it takes its whole window
        let powered = match short_of {
            Some(s) => ite(s, n("p_hi"), elec(p_shaft_loss)),
            None => elec(p_shaft_loss),
        };
        eq(
            n("p_elec"),
            ite(gt(n("on"), c(0.5)), powered, c(0.0) * n("unit_W")),
            if cfg.inverse {
                "powered: shaft power plus the loss map; unpowered or idle: nothing"
            } else {
                "powered: shaft power plus the loss map; unpowered: nothing"
            },
        )
    });
    eqs.extend([
        if cfg.inverse {
            // fast mode: v·i = p would let a power of 0 stand at v = 0 with any
            // current (the supply shorted), a root a solve can land on and the
            // inverter, off below 1 V, then holds; this form has i = 0 there
            eq(
                n("i"),
                n("p_elec") / max(n("v"), c(1.0) * n("unit_V")),
                "it takes that power from its supply",
            )
        } else {
            eq(n("v") * n("i"), n("p_elec"), "it takes that power from its supply")
        },
        eq(n("sig_speed"), n("wa"), "shaft speed"),
        eq(n("sig_torque"), n("T"), "shaft torque"),
        eq(n("sig_mech_power"), n("T") * n("w"), "mechanical power"),
        eq(n("sig_elec_power"), n("p_elec"), "electrical power"),
        eq(n("sig_losses"), n("p_elec") - n("T") * n("w"), "losses"),
        eq(
            n("t_regen"),
            ite(
                and(gt(n("v"), c(1.0) * n("unit_V")), lt(n("over"), c(0.5))),
                n("t_fl") * n("q4_torque_scale_pct"),
                c(0.0) * n("unit_N_m"),
            ),
            "its generator torque limit",
        ),
        eq(n("p_request"), n("alive") * elec(n("p_req")), "the power its command asks for"),
    ]);
    let mut all = uc2.into_params();
    all.extend(params);
    ComponentDef {
        name: variant(
            "Blocks.EMotor",
            !cfg.windowed && !cfg.inverse,
            [cfg.windowed as u8 as f64]
                .into_iter()
                .chain(cfg.inverse.then_some(1.0))
                .chain(cfg.blended.then_some(2.0)),
        ),
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
