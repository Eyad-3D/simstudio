//! Driveline blocks: mechanical node, shaft, final drive, gearbox,
//! differential and transfer case, clutch, brake, propeller.
//!
//! Today's engine lumps each rigid section into one inertia and applies a
//! gear's efficiency to the torque its *sources* send through it; here
//! every part has its own inertia on its own shaft, rigid couplings are
//! constraints (index reduction makes them exact), and a gear's efficiency
//! acts on the torque it actually carries — the sources' torque less what
//! the inertias before it take to speed up. In steady driving the two are
//! the same; while accelerating, today books a little extra gear loss for
//! the torque that only spins up a rotor (an intended difference).
//!
//! * The clutch and the brakes are Coulomb friction with stiction: they
//!   lock and break away at events located exactly. Today's clutch is a
//!   smoothed band (0.5 rad/s) that slips a little under any load, and
//!   today's brake holds a shaft by a one-step rule at |ω| ≤ 0.5 rad/s.
//! * A gear change is rigid and instantaneous, as today, but conserves
//!   momentum: the shafts' speeds jump to the nearest consistent ones in
//!   the kinetic-energy metric and the energy that costs is booked as the
//!   gearbox's loss (today keeps one shaft's speed and creates or destroys
//!   kinetic energy the books do not see).

use super::*;
use crate::rotational::driveline_speed;
use crate::table::Table1;
use lsim_ir::EnergyDecl;

/// `ite(tau·w >= 0, eta, 1/eta)`: the loss follows the power.
fn eta_dir(tau: Expr, w: Expr, eta: Expr) -> Expr {
    // η^s with s the power's direction: +1 (η, the loss comes off what
    // passes on) or −1 (1/η, flowing back), blended within `p_dir` of no
    // power so the torque it passes on stays continuous through a power
    // reversal at rest (no event; the loss τ·w·(1 − η^s) is never negative)
    exp(lsim_ir::expr::call(lsim_ir::Builtin::Log, vec![eta])
        * noev(clamp(tau * w / n("p_dir"), c(-1.0), c(1.0))))
}

/// The Mechanical Node's flanges in the library; a part may list its own.
pub const NODE_FLANGES: [&str; 4] = ["f1", "f2", "f3", "f4"];

/// mech.node: its flanges (`fs`: the library's four, or the part's own, at
/// least two) on one rigid shaft.
pub fn mech_node(fs: &[String]) -> ComponentDef {
    let id = "mech.node";
    let first = format!("{}.w", fs[0]);
    let mut eqs: Vec<lsim_ir::EquationDecl> =
        fs[1..].iter().map(|f| eq(n(&format!("{f}.w")), n(&first), "one rigid shaft")).collect();
    eqs.push(eq(c(0.0), sum(fs.iter().map(|f| n(&format!("{f}.tau")))), "the torques sum to zero"));
    eqs.push(eq(n("sig_speed"), noev(abs(n(&first))), "its speed"));
    let mut ports: Vec<PortDecl> = fs.iter().map(|f| phys(id, f, "Flange")).collect();
    ports.push(out(id, "sig_speed"));
    ComponentDef {
        name: super::electric::junction_name("Blocks.MechNode", fs, &NODE_FLANGES),
        doc: doc(id),
        ports,
        equations: eqs,
        ..Default::default()
    }
}

/// mech.shaft: an inertia on flange a's side and a coupling to flange b
/// with an efficiency in the direction the power flows.
pub fn shaft() -> ComponentDef {
    let id = "mech.shaft";
    let mut params = cps(id, &["efficiency_pct", "inertia_kgm2"]);
    params.push(p("w0", "rad/s", 0.0, "its speed at the start"));
    params.push(pe("eta", "1", max(n("efficiency_pct"), c(1e-3)), "efficiency"));
    params.push(p(
        "p_dir",
        "W",
        1.0,
        "within this power of none, its loss blends between the directions",
    ));
    let w = driveline_speed("w", "w0", "its speed");
    ComponentDef {
        name: "Blocks.Shaft".into(),
        doc: doc(id),
        ports: vec![
            phys(id, "flange_a", "Flange"),
            phys(id, "flange_b", "Flange"),
            out(id, "sig_power"),
            out(id, "sig_losses"),
        ],
        params,
        vars: vec![
            w,
            var("tau_x", "N.m", "the torque it passes from a towards b"),
            guess("eta_d", "1", n("eta"), "torque factor"),
        ],
        equations: vec![
            eq(n("w"), n("flange_a.w"), "flange a turns with it"),
            eq(n("w"), n("flange_b.w"), "flange b turns with it (rigid)"),
            eq(
                n("tau_x"),
                n("flange_a.tau") - n("inertia_kgm2") * der("w"),
                "what is left after its inertia",
            ),
            eq(n("eta_d"), eta_dir(n("tau_x"), n("w"), n("eta")), "the loss follows the power"),
            eq(
                n("flange_b.tau"),
                -(n("eta_d") * n("tau_x")),
                "it passes on its efficiency's share",
            ),
            eq(n("sig_power"), n("tau_x") * n("w"), "transmitted power"),
            eq(n("sig_losses"), n("tau_x") * n("w") * (c(1.0) - n("eta_d")), "its loss"),
        ],
        energy: EnergyDecl {
            stored: Some(c(0.5) * n("inertia_kgm2") * n("w") * n("w")),
            loss: Some(n("tau_x") * n("w") * (c(1.0) - n("eta_d"))),
        },
        ..Default::default()
    }
}

/// mech.final_drive: input inertia, a fixed-ratio gear with its
/// efficiency, output inertia; one speed (the output's).
pub fn final_drive() -> ComponentDef {
    let id = "mech.final_drive";
    let mut params = cps(id, &["ratio", "inertia_in_kgm2", "inertia_out_kgm2", "efficiency_pct"]);
    params.push(p("w0", "rad/s", 0.0, "its output speed at the start"));
    params.push(pe("eta", "1", max(n("efficiency_pct"), c(1e-3)), "efficiency"));
    params.push(p(
        "p_dir",
        "W",
        1.0,
        "within this power of none, its loss blends between the directions",
    ));
    params.push(pe(
        "i",
        "1",
        ite(gt(abs(n("ratio")), c(0.0)), n("ratio"), c(1.0)),
        "the ratio (0 counts as 1)",
    ));
    let w = driveline_speed("w_out", "w0", "output speed");
    let w_in = n("i") * n("w_out");
    ComponentDef {
        name: "Blocks.FinalDrive".into(),
        doc: doc(id),
        ports: vec![
            phys(id, "flange_in", "Flange"),
            phys(id, "flange_out", "Flange"),
            out(id, "sig_power"),
            out(id, "sig_speed_out"),
            out(id, "sig_losses"),
        ],
        params,
        vars: vec![
            w,
            var("tau_x", "N.m", "the torque the gear takes at its input"),
            guess("eta_d", "1", n("eta"), "torque factor"),
        ],
        equations: vec![
            eq(n("w_out"), n("flange_out.w"), "the output turns with it"),
            eq(n("flange_in.w"), w_in.clone(), "the input turns ratio times as fast"),
            eq(
                n("tau_x"),
                n("flange_in.tau") - n("inertia_in_kgm2") * n("i") * der("w_out"),
                "what the input side passes to the gear",
            ),
            eq(
                n("eta_d"),
                eta_dir(n("tau_x"), w_in.clone(), n("eta")),
                "the loss follows the power",
            ),
            eq(
                n("inertia_out_kgm2") * der("w_out"),
                n("flange_out.tau") + n("i") * n("eta_d") * n("tau_x"),
                "the output side: the gear's torque × ratio, less its loss",
            ),
            eq(n("sig_power"), n("tau_x") * w_in.clone(), "transmitted power"),
            eq(n("sig_speed_out"), noev(abs(n("w_out"))), "output speed"),
            eq(n("sig_losses"), n("tau_x") * w_in.clone() * (c(1.0) - n("eta_d")), "its loss"),
        ],
        energy: EnergyDecl {
            stored: Some(
                c(0.5) * n("inertia_in_kgm2") * w_in.clone() * w_in.clone()
                    + c(0.5) * n("inertia_out_kgm2") * n("w_out") * n("w_out"),
            ),
            loss: Some(n("tau_x") * w_in * (c(1.0) - n("eta_d"))),
        },
        ..Default::default()
    }
}

/// The gearbox's configuration: its ratios by gear number and whether the
/// gear is selected by a signal.
#[derive(Clone, Debug, PartialEq)]
pub struct GearboxConfig {
    /// ratio by gear number
    pub ratios: Table1,
    /// the Gear Select input is wired
    pub select_wired: bool,
}

impl Default for GearboxConfig {
    fn default() -> Self {
        let p = &catalog::param("mech.gearbox", "ratios").expect("ratios")["default"];
        GearboxConfig {
            ratios: Table1::from_json(p, Default::default()).expect("ratios"),
            select_wired: false,
        }
    }
}

/// The nearest defined gear's value (`values`) for a gear number `x`:
/// steps at the midpoints between the defined gears (today: the nearest
/// gear wins, the lower one on a tie).
fn nearest(gears: &[f64], values: &[f64], x: Expr) -> Expr {
    let mut e = c(values.first().copied().unwrap_or(1.0));
    for k in 1..gears.len() {
        let mid = 0.5 * (gears[k - 1] + gears[k]);
        let dv = values[k] - values[k - 1];
        if dv != 0.0 {
            e = e + c(dv) * ite(gt(x.clone(), c(mid)), c(1.0), c(0.0));
        }
    }
    e
}

/// mech.gearbox: input inertia, the selected gear's ratio with the
/// efficiency, output inertia; two speeds tied by the ratio (a rigid
/// constraint that changes at a shift).
pub fn gearbox(cfg: &GearboxConfig) -> ComponentDef {
    let id = "mech.gearbox";
    let mut params =
        cps(id, &["default_gear", "efficiency_pct", "inertia_in_kgm2", "inertia_out_kgm2"]);
    params.push(p("w0", "rad/s", 0.0, "its output speed at the start"));
    params.push(pe("eta", "1", max(n("efficiency_pct"), c(1e-3)), "efficiency"));
    params.push(p(
        "p_dir",
        "W",
        1.0,
        "within this power of none, its loss blends between the directions",
    ));
    let mut ports = vec![phys(id, "flange_in", "Flange"), phys(id, "flange_out", "Flange")];
    if cfg.select_wired {
        ports.push(inp(id, "sig_gear_in"));
    }
    for q in ["sig_gear", "sig_speed_out", "sig_power", "sig_losses"] {
        ports.push(out(id, q));
    }
    ports.push(output("ratio_now", "1", "the selected gear's ratio (for the driver's blending)"));
    let sel = if cfg.select_wired { n("sig_gear_in") } else { n("default_gear") };
    let ratios: Vec<f64> = cfg.ratios.y.iter().map(|r| if *r != 0.0 { *r } else { 1.0 }).collect();
    let w_out = driveline_speed("w_out", "w0", "output speed");
    let mut w_in = driveline_speed("w_in", "w0", "input speed");
    // a wired selector's first value is not known before the run: the
    // start guess uses the default gear and the initial solve corrects it
    w_in.start = Some(n("w0") * nearest(&cfg.ratios.x, &ratios, n("default_gear")));
    let mut fp = vec![cfg.select_wired as u8 as f64];
    fp.extend(&cfg.ratios.x);
    fp.extend(&cfg.ratios.y);
    ComponentDef {
        name: variant("Blocks.Gearbox", *cfg == GearboxConfig::default(), fp),
        doc: doc(id),
        ports,
        params,
        vars: vec![
            w_in,
            w_out,
            guess(
                "ratio",
                "1",
                c(ratios.first().copied().unwrap_or(1.0)),
                "the selected gear's ratio",
            ),
            var("gear", "1", "the selected gear"),
            var("tau_x", "N.m", "the torque the gear takes at its input"),
            guess("eta_d", "1", n("eta"), "torque factor"),
        ],
        equations: vec![
            eq(
                n("gear"),
                nearest(&cfg.ratios.x, &cfg.ratios.x, sel.clone()),
                "the nearest defined gear",
            ),
            eq(n("ratio"), nearest(&cfg.ratios.x, &ratios, sel), "its ratio"),
            eq(n("w_in"), n("flange_in.w"), "the input shaft"),
            eq(n("w_out"), n("flange_out.w"), "the output shaft"),
            eq(n("w_in"), n("ratio") * n("w_out"), "the input turns ratio times as fast (rigid)"),
            eq(
                n("inertia_in_kgm2") * der("w_in"),
                n("flange_in.tau") - n("tau_x"),
                "the input side's inertia",
            ),
            eq(n("eta_d"), eta_dir(n("tau_x"), n("w_in"), n("eta")), "the loss follows the power"),
            eq(
                n("inertia_out_kgm2") * der("w_out"),
                n("flange_out.tau") + n("ratio") * n("eta_d") * n("tau_x"),
                "the output side: the gear's torque × ratio, less its loss",
            ),
            eq(n("sig_gear"), n("gear"), "active gear"),
            eq(n("ratio_now"), n("ratio"), "the selected ratio"),
            eq(n("sig_speed_out"), noev(abs(n("w_out"))), "output speed"),
            eq(n("sig_power"), n("tau_x") * n("w_in"), "transmitted power"),
            eq(n("sig_losses"), n("tau_x") * n("w_in") * (c(1.0) - n("eta_d")), "its loss"),
        ],
        energy: EnergyDecl {
            stored: Some(
                c(0.5) * n("inertia_in_kgm2") * n("w_in") * n("w_in")
                    + c(0.5) * n("inertia_out_kgm2") * n("w_out") * n("w_out"),
            ),
            loss: Some(n("tau_x") * n("w_in") * (c(1.0) - n("eta_d"))),
        },
        // a change of the selected ratio is a rigid engagement: the run
        // keeps the momentum of what the gears tie together
        engagements: vec![lsim_ir::EngagementDecl { changes: n("ratio") }],
        ..Default::default()
    }
}

/// How a differential's two outputs are tied to each other.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SplitKind {
    /// open: the outputs' speeds are free, the torque splits in fixed shares
    #[default]
    Open,
    /// locked: both outputs at one speed
    Locked,
    /// limited-slip: open, with a friction coupling between the outputs
    LimitedSlip,
}

/// A differential's or transfer case's configuration.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct SplitConfig {
    /// a transfer case (torque split from its parameter; else halves)
    pub transfer_case: bool,
    /// how its outputs are tied (a transfer case: open or locked)
    pub kind: SplitKind,
    /// fast mode's inverse model (a locked or limited-slip one only): the
    /// wheels turn at the vehicle's speed, so the outputs' speeds cannot
    /// say how the torque splits; the inputs `g_a` and `g_b` (each output's
    /// wheel's grip) do instead
    pub inverse: bool,
}

/// mech.differential and mech.transfer_case: an input carrier with its
/// inertia, splitting torque to two outputs. Open: the outputs' speeds are
/// free and the input turns ratio × their weighted mean ((1−f)·a + f·b),
/// the torque splits (1−f) : f; locked: both outputs turn together and the
/// torque follows the grip. The efficiency acts on the power through it.
///
/// Limited-slip (a clutch-pack type, as Salisbury's): open, with a Coulomb
/// friction with stiction between the two outputs (the clutch's, from
/// rotational::friction_mode_eqs; with its outputs at one speed at the
/// start it starts stuck, and breaks away just after it if it cannot hold).
/// Its torque `tau_c`, taken from output a and given to output b, is at
/// most the locking capacity
/// `preload + ½ · lock · |i·η_d·τ_x|` (the torque the gears pass on),
/// `lock` the drive ramp's share while that torque drives forwards and the
/// coast ramp's while it drives backwards: stuck, the outputs turn together and the coupling
/// holds whatever keeps them so; slipping, it takes exactly its capacity
/// from the faster output to the slower one, and its loss is `tau_c · s`,
/// s = ω_a − ω_b. The two outputs' torques then differ by 2·tau_c: by the
/// preload twice with no torque through it (the preload is the torque that
/// turns one output with the other held and the input free) and by `lock`
/// × the torque through it without preload (the locking share the ramps give,
/// a torque bias ratio of (1 + lock) / (1 − lock)).
///
/// In fast mode's inverse model the wheels turn at the vehicle's speed and
/// the outputs with them, so the outputs never slip there and a stuck
/// coupling's torque would be undetermined; the split instead follows
/// the wheels' grip (inputs `g_a`, `g_b`), as it does at a locked axle's
/// limit: locked, the outputs' torques in proportion to their wheels' grip
/// (both reach it at once: the axle holds the sum of the two); limited-
/// slip, that split as far as the coupling's capacity reaches (the low-grip
/// wheel's torque plus the locking torque, at most the sum). With equal
/// grip that is the open split.
pub fn split(cfg: SplitConfig) -> ComponentDef {
    let id = if cfg.transfer_case { "mech.transfer_case" } else { "mech.differential" };
    let lsd = cfg.kind == SplitKind::LimitedSlip;
    let inverse = cfg.inverse && cfg.kind != SplitKind::Open;
    let mut params = cps(id, &["ratio", "efficiency_pct", "inertia_kgm2"]);
    if cfg.transfer_case {
        params.push(cp(id, "torque_split_a_pct"));
        params.push(pe(
            "f",
            "1",
            c(1.0) - clamp(n("torque_split_a_pct"), c(0.0), c(1.0)),
            "output B's share",
        ));
    } else {
        params.push(p("f", "1", 0.5, "output B's share"));
    }
    if lsd {
        params.extend(cps(id, &["preload_Nm", "lock_drive_pct", "lock_coast_pct"]));
    }
    params.push(p("w0", "rad/s", 0.0, "its input speed at the start"));
    params.push(pe("eta", "1", max(n("efficiency_pct"), c(1e-3)), "efficiency"));
    params.push(p(
        "p_dir",
        "W",
        1.0,
        "within this power of none, its loss blends between the directions",
    ));
    params.push(pe(
        "i",
        "1",
        ite(gt(abs(n("ratio")), c(0.0)), n("ratio"), c(1.0)),
        "the ratio (0 counts as 1)",
    ));
    let w = driveline_speed("w", "w0", "the carrier's speed (the input's)");
    let mut vars = vec![
        w,
        var("tau_x", "N.m", "torque through the gears"),
        guess("eta_d", "1", n("eta"), "torque factor"),
    ];
    let mut ports = vec![
        phys(id, "flange_in", "Flange"),
        phys(id, "flange_out_a", "Flange"),
        phys(id, "flange_out_b", "Flange"),
    ];
    let mut eqs = vec![
        eq(n("w"), n("flange_in.w"), "the carrier turns with the input"),
        eq(
            n("tau_x"),
            n("flange_in.tau") - n("inertia_kgm2") * der("w"),
            "what the carrier passes to the gears",
        ),
        eq(n("eta_d"), eta_dir(n("tau_x"), n("w"), n("eta")), "the loss follows the power"),
    ];
    // the torque the gears pass to the two outputs together
    let t_out = || n("i") * n("eta_d") * n("tau_x");
    let open_speed = || {
        eq(
            n("w"),
            n("i") * ((c(1.0) - n("f")) * n("flange_out_a.w") + n("f") * n("flange_out_b.w")),
            "open: the input turns ratio × the outputs' weighted mean",
        )
    };
    // the outputs' torques: the open shares (written as before: an open
    // one's model text is unchanged), less (a) and plus (b) what the
    // coupling passes from a to b
    let shares = |coupled: bool| {
        let a = -((c(1.0) - n("f")) * n("i") * n("eta_d") * n("tau_x"));
        let b = -(n("f") * n("i") * n("eta_d") * n("tau_x"));
        if coupled {
            [
                eq(
                    n("flange_out_a.tau"),
                    a + n("tau_c"),
                    "side a's share, less what the coupling passes to b",
                ),
                eq(
                    n("flange_out_b.tau"),
                    b - n("tau_c"),
                    "side b's share, and what the coupling passes to it",
                ),
            ]
        } else {
            [
                eq(n("flange_out_a.tau"), a, "side a's share"),
                eq(n("flange_out_b.tau"), b, "side b's share"),
            ]
        }
    };
    if lsd {
        vars.push(var("cap", "N.m", "the coupling's locking capacity now"));
        // drive ramp while the torque through the gears drives forwards,
        // coast ramp while it drives backwards (a kink at no torque: no event)
        let t_in = || n("i") * n("eta_d") * n("tau_x");
        eqs.push(eq(
            n("cap"),
            max(n("preload_Nm"), c(0.0))
                + c(0.5)
                    * (clamp(n("lock_drive_pct"), c(0.0), c(1.0)) * noev(max(t_in(), c(0.0)))
                        + clamp(n("lock_coast_pct"), c(0.0), c(1.0)) * noev(max(-t_in(), c(0.0)))),
            "preload + ½ × the ramp's locking share × the torque the gears pass on",
        ));
    }
    if inverse {
        ports.push(input("g_a", "N", "output a's wheel's grip (fast mode)"));
        ports.push(input("g_b", "N", "output b's wheel's grip (fast mode)"));
        params.push(p("unit_N", "N", 1.0, "unit carrier"));
        vars.push(var("tau_c", "N.m", "the torque the coupling passes from output a to b"));
        // output a's share of the torque at the wheels' grip (the open
        // share with no grip on either)
        let g = || n("g_a") + n("g_b");
        let share_a = noev(ite(
            gt(g(), c(1e-9) * n("unit_N")),
            n("g_a") / max(g(), c(1e-9) * n("unit_N")),
            c(1.0) - n("f"),
        ));
        let target = (c(1.0) - n("f") - share_a) * t_out();
        eqs.push(open_speed());
        eqs.extend(shares(true));
        eqs.push(if lsd {
            eq(
                n("tau_c"),
                noev(clamp(target, -n("cap"), n("cap"))),
                "fast mode: the grip's split, as far as the coupling's capacity reaches",
            )
        } else {
            eq(n("tau_c"), target, "fast mode, locked: the torque splits as the wheels' grip")
        });
    } else if cfg.kind == SplitKind::Locked {
        eqs.extend([
            eq(n("flange_out_a.w"), n("flange_out_b.w"), "locked: both outputs turn together"),
            eq(n("w"), n("i") * n("flange_out_a.w"), "the input turns ratio times as fast"),
            eq(
                n("flange_out_a.tau") + n("flange_out_b.tau"),
                -t_out(),
                "the outputs share the torque as their grip takes it",
            ),
        ]);
    } else if lsd {
        params.extend([
            p("s0", "rad/s", 0.0, "the outputs' speed difference at the start"),
            p(
                "eps",
                "N.m",
                1e-9,
                "how far the holding torque must exceed the capacity to break away",
            ),
            p("unit_m", "kg.m2", 1.0, "unit carrier"),
            crate::rotational::s_small("rad/s"),
            crate::rotational::s_jump("rad/s"),
            p("t_start", "s", 1e-9, "just after the start: when it checks a stuck start"),
        ]);
        let mut s = driveline_speed("s_c", "s0", "the outputs' speed difference, a − b");
        s.fixed = false;
        vars.extend([
            s,
            var("acc_c", "rad/s2", "its rate"),
            var("tau_c", "N.m", "the torque the coupling passes from output a to b"),
            var("brk_start", "1", "1 past the start while stuck past its capacity"),
        ]);
        // with its outputs together at the start it starts stuck. The shared
        // rule starts a friction sliding forwards (dir from s0 ≥ 0), which
        // here passed the whole capacity from one output to the other until
        // it stuck (on the FS launch for 1.8 ns, setting the rear wheels'
        // speeds apart: 5 more events); with no direction (passing nothing)
        // the outputs' slip wanders at round-off across 0, an event at each
        // crossing. Its break-away at the start, which a condition already
        // true at t = 0 never fires, is the event just after it (below)
        let mut modes = crate::rotational::friction_mode_vars("s0");
        for v in modes.iter_mut().filter(|v| v.name == "stuck") {
            v.start = Some(ite(and(ge(n("s0"), c(0.0)), le(n("s0"), c(0.0))), c(1.0), c(0.0)));
        }
        vars.extend(modes);
        eqs.extend([
            eq(n("s_c"), n("flange_out_a.w") - n("flange_out_b.w"), "the coupling's slip"),
            eq(n("acc_c"), der("s_c"), "the slip's rate"),
            open_speed(),
        ]);
        eqs.extend(shares(true));
        eqs.extend(crate::rotational::friction_mode_eqs("s_c", "acc_c", "tau_c", "cap", "unit_m"));
        // started stuck with its holding torque already past its capacity, it
        // breaks away just after the start, the way that torque pushes
        let brk = |k: &str| gt(n(k), c(0.5));
        eqs.push(eq(
            n("brk_start"),
            ite(gt(time(), n("t_start")), c(1.0), c(0.0)) * (n("brk_fwd") + n("brk_back")),
            "past the start, stuck with its holding torque past its capacity",
        ));
        eqs.push(when(
            gt(n("brk_start"), c(0.5)),
            &[
                ("stuck", ite(brk("brk_fwd"), c(0.0), ite(brk("brk_back"), c(0.0), pre("stuck")))),
                ("dir", ite(brk("brk_fwd"), c(1.0), ite(brk("brk_back"), c(-1.0), pre("dir")))),
                ("armed", ite(brk("brk_fwd"), c(0.0), ite(brk("brk_back"), c(0.0), pre("armed")))),
            ],
            "started stuck past its capacity, it breaks away just after the start",
        ));
    } else {
        eqs.push(open_speed());
        eqs.extend(shares(false));
    }
    let coupled = lsd || inverse;
    let slip = || n("flange_out_a.w") - n("flange_out_b.w");
    let loss = || {
        let gears = n("tau_x") * n("w") * (c(1.0) - n("eta_d"));
        if coupled { gears + n("tau_c") * slip() } else { gears }
    };
    eqs.extend([
        eq(n("sig_torque_a"), -n("flange_out_a.tau"), "torque out to side a"),
        eq(n("sig_torque_b"), -n("flange_out_b.tau"), "torque out to side b"),
        eq(n("sig_speed_in"), noev(abs(n("w"))), "input speed"),
        eq(n("sig_power"), n("tau_x") * n("w"), "input power"),
        eq(
            n("sig_losses"),
            loss(),
            if coupled { "its loss: the gears' and the coupling's" } else { "its loss" },
        ),
    ]);
    let base = if cfg.transfer_case { "Blocks.TransferCase" } else { "Blocks.Differential" };
    for q in ["sig_torque_a", "sig_torque_b", "sig_speed_in", "sig_power", "sig_losses"] {
        ports.push(out(id, q));
    }
    let kind = match cfg.kind {
        SplitKind::Open => "",
        SplitKind::Locked => "Locked",
        SplitKind::LimitedSlip => "LimitedSlip",
    };
    ComponentDef {
        name: format!("{base}{kind}{}", if inverse { "Grip" } else { "" }),
        doc: doc(id),
        ports,
        params,
        vars,
        equations: eqs,
        energy: EnergyDecl {
            stored: Some(c(0.5) * n("inertia_kgm2") * n("w") * n("w")),
            loss: Some(loss()),
        },
        ..Default::default()
    }
}

/// The clutch's configuration.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct ClutchConfig {
    /// the Engagement input is wired (else fully engaged)
    pub engage_wired: bool,
}

/// mech.clutch: a dry clutch, Coulomb friction with stiction between its
/// flanges, capacity engagement × maximum torque.
pub fn clutch(cfg: ClutchConfig) -> ComponentDef {
    let id = "mech.clutch";
    let engage = if cfg.engage_wired { clamp(n("sig_engage_in"), c(0.0), c(1.0)) } else { c(1.0) };
    let mut d = crate::rotational::friction_core(
        "",
        "",
        "Flange",
        true,
        engage * max(n("max_torque_Nm"), c(0.0)),
    );
    d.name =
        variant("Blocks.Clutch", cfg == ClutchConfig::default(), [cfg.engage_wired as u8 as f64]);
    d.doc = doc(id);
    d.ports = vec![phys(id, "flange_a", "Flange"), phys(id, "flange_b", "Flange")];
    if cfg.engage_wired {
        d.ports.push(inp(id, "sig_engage_in"));
    }
    for q in ["sig_torque", "sig_slip_speed", "sig_losses"] {
        d.ports.push(out(id, q));
    }
    rename_ports(&mut d, &[("a", "flange_a"), ("b", "flange_b")]);
    d.params.insert(0, cp(id, "max_torque_Nm"));
    d.equations.extend([
        eq(n("sig_torque"), n("flange_a.tau"), "transmitted torque"),
        eq(n("sig_slip_speed"), n("s"), "slip speed, a − b"),
        eq(n("sig_losses"), n("flange_a.tau") * n("s"), "slip loss"),
    ]);
    d
}

/// The brake's configuration.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrakeConfig {
    /// the Brake Command input is wired (else it never brakes)
    pub demand_wired: bool,
    /// fast mode's inverse model, its speed prescribed forwards: its
    /// torque acts against forward rotation (a wheel's slip turns it back
    /// a little at the last moment of a stop; braking must not then drive
    /// it, which no command could follow), and it follows a command beyond
    /// 1 (the driver's pedal beyond full brake, which fast mode flags)
    pub prescribed: bool,
}

impl Default for BrakeConfig {
    fn default() -> Self {
        BrakeConfig { demand_wired: true, prescribed: false }
    }
}

/// mech.brake: its disc's inertia and a friction torque against the
/// rotation, capacity command × maximum torque. Within `w_hold` of
/// standstill the torque is in proportion to the speed (a stiff damper:
/// holding a shaft at rest it lets it creep at most at `w_hold`), beyond it
/// it is the full capacity: a brake on each wheel of an axle, coupled
/// through a differential to a motor and through stiff tyres to the
/// vehicle, then holds without the stick-slip events that, with today's
/// run loop, chatter at every stop (today's brake: torque against the
/// rotation, holding below 0.5 rad/s by a one-step rule).
pub fn brake(cfg: BrakeConfig) -> ComponentDef {
    let id = "mech.brake";
    // fast mode: beyond a full command (where the driver's pedal is beyond
    // full brake, which fast mode flags) its torque keeps following it
    let cmd = match (cfg.demand_wired, cfg.prescribed) {
        (true, false) => clamp(n("sig_demand_in"), c(0.0), c(1.0)),
        (true, true) => max(n("sig_demand_in"), c(0.0)),
        (false, _) => c(0.0),
    };
    let mut ports = vec![phys(id, "flange", "Flange")];
    if cfg.demand_wired {
        ports.push(inp(id, "sig_demand_in"));
    }
    ports.extend([out(id, "sig_torque"), out(id, "sig_power")]);
    let mut params = cps(id, &["max_torque_Nm", "inertia_kgm2"]);
    params.extend([
        p("s0", "rad/s", 0.0, "the disc's speed at the start"),
        p("w_hold", "rad/s", 1e-3, "within this speed of rest its torque grows with the speed"),
    ]);
    let s = driveline_speed("s", "s0", "the disc's speed");
    let vars = vec![
        s,
        var("acc", "rad/s2", "its acceleration"),
        var("f_c", "N.m", "the brake's torque capacity now"),
        var("tau_b", "N.m", "the torque the brake takes"),
    ];
    let mut eqs = vec![
        eq(n("s"), n("flange.w"), "the disc turns with the shaft"),
        eq(n("acc"), der("s"), "its acceleration"),
        eq(n("inertia_kgm2") * n("acc"), n("flange.tau") - n("tau_b"), "the disc's inertia"),
        eq(n("f_c"), cmd * max(n("max_torque_Nm"), c(0.0)), "command × maximum torque"),
    ];
    eqs.push(if cfg.prescribed {
        eq(n("tau_b"), n("f_c"), "its torque against forward rotation: its capacity")
    } else {
        eq(
            n("tau_b"),
            n("f_c") * noev(clamp(n("s") / n("w_hold"), c(-1.0), c(1.0))),
            "its torque against the rotation: its capacity, in proportion to the speed near rest",
        )
    });
    eqs.extend([
        eq(n("sig_torque"), n("f_c"), "brake torque (its capacity now)"),
        eq(n("sig_power"), n("tau_b") * n("s"), "braking power"),
    ]);
    ComponentDef {
        name: variant(
            "Blocks.Brake",
            cfg == BrakeConfig::default(),
            [cfg.demand_wired as u8 as f64].into_iter().chain(cfg.prescribed.then_some(1.0)),
        ),
        doc: doc(id),
        ports,
        params,
        vars,
        equations: eqs,
        energy: EnergyDecl {
            stored: Some(c(0.5) * n("inertia_kgm2") * n("s") * n("s")),
            loss: Some(n("tau_b") * n("s")),
        },
        ..Default::default()
    }
}

/// propulsion.propeller: its inertia and a load torque growing with the
/// square of speed, against the motion.
pub fn propeller() -> ComponentDef {
    let id = "propulsion.propeller";
    let mut params = cps(id, &["torque_ref_Nm", "ref_speed_rpm", "inertia_kgm2"]);
    params.push(p("w0", "rad/s", 0.0, "its speed at the start"));
    params.push(pe(
        "w_ref",
        "rad/s",
        max(n("ref_speed_rpm"), c(std::f64::consts::PI / 30.0) * n("unit_rad_s")),
        "reference speed, at least 1 1/min",
    ));
    params.insert(0, p("unit_rad_s", "rad/s", 1.0, "unit carrier"));
    let w = driveline_speed("w", "w0", "its speed");
    ComponentDef {
        name: "Blocks.Propeller".into(),
        doc: doc(id),
        ports: vec![phys(id, "shaft", "Flange"), out(id, "sig_speed"), out(id, "sig_shaft_power")],
        params,
        vars: vec![w, var("t_load", "N.m", "the load torque, against the motion")],
        equations: vec![
            eq(n("w"), n("shaft.w"), "it turns with the shaft"),
            eq(
                n("t_load"),
                max(n("torque_ref_Nm"), c(0.0)) * n("w") * noev(abs(n("w")))
                    / (n("w_ref") * n("w_ref")),
                "torque ∝ speed²",
            ),
            eq(n("inertia_kgm2") * der("w"), n("shaft.tau") - n("t_load"), "its inertia"),
            eq(n("sig_speed"), noev(abs(n("w"))), "its speed"),
            eq(n("sig_shaft_power"), noev(abs(n("t_load") * n("w"))), "shaft power"),
        ],
        energy: EnergyDecl {
            stored: Some(c(0.5) * n("inertia_kgm2") * n("w") * n("w")),
            loss: Some(n("t_load") * n("w")),
        },
        ..Default::default()
    }
}

/// Renames a definition's ports (and every name that starts with them).
fn rename_ports(d: &mut ComponentDef, map: &[(&str, &str)]) {
    let fix = |s: &str| -> String {
        for (a, b) in map {
            if let Some(rest) = s.strip_prefix(&format!("{a}.")) {
                return format!("{b}.{rest}");
            }
        }
        s.to_string()
    };
    let rw = |e: Expr| {
        e.rewrite(&mut |x| match x {
            Expr::Name(s) => Expr::Name(fix(&s)),
            other => other,
        })
    };
    for e in d.equations.iter_mut() {
        match &mut e.eq {
            lsim_ir::Equation::Eq { lhs, rhs } => {
                *lhs = rw(std::mem::replace(lhs, c(0.0)));
                *rhs = rw(std::mem::replace(rhs, c(0.0)));
            }
            lsim_ir::Equation::When { condition, actions } => {
                *condition = rw(std::mem::replace(condition, c(0.0)));
                for a in actions {
                    let (lsim_ir::WhenAction::Assign { value, .. }
                    | lsim_ir::WhenAction::Reinit { value, .. }) = a;
                    *value = rw(std::mem::replace(value, c(0.0)));
                }
            }
            lsim_ir::Equation::Assert { condition, .. } => {
                *condition = rw(std::mem::replace(condition, c(0.0)));
            }
        }
    }
    if let Some(l) = d.energy.loss.take() {
        d.energy.loss = Some(rw(l));
    }
    if let Some(s) = d.energy.stored.take() {
        d.energy.stored = Some(rw(s));
    }
}

/// The driveline blocks in their default configuration.
pub fn defaults() -> Vec<ComponentDef> {
    vec![
        mech_node(&NODE_FLANGES.map(String::from)),
        shaft(),
        final_drive(),
        gearbox(&GearboxConfig::default()),
        split(SplitConfig::default()),
        split(SplitConfig { transfer_case: true, ..Default::default() }),
        clutch(ClutchConfig::default()),
        brake(BrakeConfig::default()),
        propeller(),
    ]
}
