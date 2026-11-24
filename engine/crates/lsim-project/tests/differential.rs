//! The Differential's three Types against hand answers: a torque T into
//! it, output a held back by a friction of G (a wheel with little grip:
//! it slips at G), output b a free inertia, both outputs 1 kg·m², the
//! differential lossless with ratio 1 and no inertia of its own. With
//! T = 100 N·m and G = 40 N·m:
//!
//! * open: each side gets T/2 = 50 N·m, so a speeds up at 50 − 40 = 10
//!   rad/s² and b at 50;
//! * locked: one shaft, (T − G)/2 = 30 rad/s² for both;
//! * limited-slip, slipping (preload 5 N·m, 20 % under power): to turn
//!   together b would need 30 N·m, 20 N·m less than its open share, more
//!   than the coupling's capacity 5 + ½·0.2·100 = 15 N·m, so it slips and
//!   passes 15 N·m from b (the faster) to a: a gets 65 N·m (25 rad/s²), b
//!   35 (35 rad/s²), and the coupling turns 15 × 10·t W into heat
//!   (75·t² J);
//! * limited-slip, stuck (preload 15 N·m): the capacity 25 N·m holds the
//!   20 N·m, so both turn as locked;
//! * limited-slip off power (T = −100 N·m, 10 % coast): the mirror image,
//!   capacity 5 + ½·0.1·100 = 10 N·m: a gets −60 N·m (−20 rad/s²), b −40.
//!
//! Fast mode's variant (both wheels' grip as inputs, `g_a` and `g_b`):
//! locked, the torque splits as the grip; limited-slip, that split as far
//! as the capacity reaches.

mod common;

use common::*;
use lsim_lib::blocks::driveline::{SplitConfig, SplitKind, split};
use lsim_solve::SimResult;

const RTOL: f64 = 1e-9;

/// The rig with the differential `def` and its parameters; T, G.
fn rig(def: &str, diff: &[(&str, f64)], t: f64, g: f64) -> lsim_ir::ComponentDef {
    let mut d = vec![("ratio", 1.0), ("efficiency_pct", 1.0), ("inertia_kgm2", 0.0)];
    d.extend_from_slice(diff);
    model(
        "DiffRig",
        vec![
            part("drive", "Rotational.ConstantTorque", &[("tau", t)]),
            part("diff", def, &d),
            part("ja", "Rotational.Inertia", &[("J", 1.0)]),
            part("jb", "Rotational.Inertia", &[("J", 1.0)]),
            part("grip", "Rotational.Clutch", &[]),
            part("ground", "Rotational.Fixed", &[]),
            part("g", "Signal.Constant_N_m", &[("k", g)]),
        ],
        &[
            ("drive.flange", "diff.flange_in"),
            ("diff.flange_out_a", "ja.a"),
            ("ja.b", "grip.a"),
            ("grip.b", "ground.flange"),
            ("g.y", "grip.fc"),
            ("diff.flange_out_b", "jb.a"),
        ],
    )
}

fn library() -> lsim_ir::Library {
    let mut lib = lib();
    for kind in [SplitKind::Locked, SplitKind::LimitedSlip] {
        for inverse in [false, true] {
            lib.add(split(SplitConfig { transfer_case: false, kind, inverse }));
        }
    }
    lib
}

/// Runs 2 s and checks each side's speed at 1 s and 2 s against its
/// acceleration × t, the torques out, and that the books close.
fn check_run(top: &lsim_ir::ComponentDef, acc_a: f64, acc_b: f64, torque_a: f64) -> SimResult {
    let (_built, res) = run(&library(), top, 2.0, 0.5, RTOL);
    for t in [1.0, 2.0] {
        let (wa, wb) = (at(&res, "ja.w", t), at(&res, "jb.w", t));
        assert!((wa - acc_a * t).abs() < 1e-6 * acc_a.abs().max(1.0) * t, "ω_a({t}) {wa}");
        assert!((wb - acc_b * t).abs() < 1e-6 * acc_b.abs().max(1.0) * t, "ω_b({t}) {wb}");
    }
    let ta = at(&res, "diff.sig_torque_a", 2.0);
    assert!((ta - torque_a).abs() < 1e-6 * torque_a.abs(), "torque out to a {ta}");
    books_close(&res, 1e-7);
    res
}

#[test]
fn open_each_side_gets_half() {
    check_run(&rig("Blocks.Differential", &[], 100.0, 40.0), 10.0, 50.0, 50.0);
}

#[test]
fn locked_one_shaft() {
    // a carries 30 N·m for its inertia and 40 against its grip
    check_run(&rig("Blocks.DifferentialLocked", &[], 100.0, 40.0), 30.0, 30.0, 70.0);
}

const LSD: &str = "Blocks.DifferentialLimitedSlip";

#[test]
fn limited_slip_slipping_passes_its_capacity_and_books_its_heat() {
    let lsd = [("preload_Nm", 5.0), ("lock_drive_pct", 0.2), ("lock_coast_pct", 0.1)];
    let res = check_run(&rig(LSD, &lsd, 100.0, 40.0), 25.0, 35.0, 65.0);
    // the coupling's heat: 15 N·m × 10·t rad/s → 75·t² J
    let (_, loss, ..) = books(&res, "diff");
    let i = res.times.iter().position(|x| (x - 2.0).abs() < 1e-12).unwrap();
    assert!((loss[i] - 300.0).abs() < 1e-6 * 300.0, "the coupling's heat {}", loss[i]);
    let p = at(&res, "diff.sig_losses", 2.0);
    assert!((p - 300.0).abs() < 1e-6 * 300.0, "its loss now: 15 N·m × 20 rad/s, {p}");
}

#[test]
fn limited_slip_stuck_turns_as_locked() {
    let lsd = [("preload_Nm", 15.0), ("lock_drive_pct", 0.2), ("lock_coast_pct", 0.1)];
    let res = check_run(&rig(LSD, &lsd, 100.0, 40.0), 30.0, 30.0, 70.0);
    assert!(at(&res, "diff.sig_losses", 2.0).abs() < 1e-6, "stuck: no heat");
}

#[test]
fn limited_slip_off_power_uses_the_coast_ramp() {
    let lsd = [("preload_Nm", 5.0), ("lock_drive_pct", 0.2), ("lock_coast_pct", 0.1)];
    check_run(&rig(LSD, &lsd, -100.0, 40.0), -20.0, -40.0, -60.0);
}

/// Fast mode's variant, grip 1 on a and 3 on b (any unit): locked, a gets
/// a quarter of the 100 N·m; limited-slip with a capacity of 15 N·m, a gets
/// 50 − 15 = 35 N·m (it would need 25 less than its open half).
#[test]
fn fast_mode_splits_by_the_grip() {
    for (def, extra, torque_a) in [
        ("Blocks.DifferentialLockedGrip", vec![], 25.0),
        (
            "Blocks.DifferentialLimitedSlipGrip",
            vec![("preload_Nm", 5.0), ("lock_drive_pct", 0.2), ("lock_coast_pct", 0.1)],
            35.0,
        ),
    ] {
        let mut d = vec![("ratio", 1.0), ("efficiency_pct", 1.0), ("inertia_kgm2", 0.0)];
        d.extend(extra);
        let top = model(
            "GripRig",
            vec![
                part("drive", "Rotational.ConstantTorque", &[("tau", 100.0)]),
                part("diff", def, &d),
                part("ja", "Rotational.Inertia", &[("J", 1.0)]),
                part("jb", "Rotational.Inertia", &[("J", 1.0)]),
                part("ga", "Signal.Constant_N", &[("k", 1.0)]),
                part("gb", "Signal.Constant_N", &[("k", 3.0)]),
            ],
            &[
                ("drive.flange", "diff.flange_in"),
                ("diff.flange_out_a", "ja.a"),
                ("diff.flange_out_b", "jb.a"),
                ("ga.y", "diff.g_a"),
                ("gb.y", "diff.g_b"),
            ],
        );
        check_run(&top, torque_a, 100.0 - torque_a, torque_a);
    }
}

/// A torque that grows with time: `k · t` into its flange.
fn rising_torque() -> lsim_ir::ComponentDef {
    use lsim_ir::component::build::{eq, port};
    use lsim_lib::x::{n, p, time};
    lsim_ir::ComponentDef {
        name: "Test.RisingTorque".into(),
        doc: "Drives its flange with k · t.".into(),
        ports: vec![port("flange", "Flange", "the driven shaft")],
        params: vec![p("k", "N.m/s", 0.0, "how fast the torque grows")],
        equations: vec![eq(n("flange.tau"), -(n("k") * time()), "it drives the shaft with k · t")],
        ..Default::default()
    }
}

/// The torque into a limited-slip differential grows as 10·t N·m, its
/// outputs two free inertias, 1 and 3 kg·m², its clutch's preload 3 N·m and
/// 20 % under power. Held together, both turn at T/4 rad/s², so output a
/// needs T/4 of its open half T/2: the clutch passes T/4 from a to b. Its
/// capacity is 3 + ½·0.2·T, so it breaks away where T/4 = 3 + 0.1·T: T =
/// 20 N·m, at t = 2 s. That condition reads time and the inertias' rates
/// (J·dω/dt), and the run's look-ahead follows it along each step: no
/// warning, and the break-away located at 2 s (past the capacity by the
/// friction rule's margin) to the event tolerance.
#[test]
fn limited_slip_breaks_away_where_a_rising_torque_passes_its_capacity() {
    let mut lib = library();
    lib.add(rising_torque());
    let lsd = [
        ("ratio", 1.0),
        ("efficiency_pct", 1.0),
        ("inertia_kgm2", 0.0),
        ("preload_Nm", 3.0),
        ("lock_drive_pct", 0.2),
        ("lock_coast_pct", 0.2),
    ];
    let top = model(
        "RisingRig",
        vec![
            part("drive", "Test.RisingTorque", &[("k", 10.0)]),
            part("diff", LSD, &lsd),
            part("ja", "Rotational.Inertia", &[("J", 1.0)]),
            part("jb", "Rotational.Inertia", &[("J", 3.0)]),
        ],
        &[
            ("drive.flange", "diff.flange_in"),
            ("diff.flange_out_a", "ja.a"),
            ("diff.flange_out_b", "jb.a"),
        ],
    );
    let (_built, res) = run(&lib, &top, 3.0, 0.5, RTOL);
    // (past the capacity by the friction rule's margin: a share BREAKAWAY
    // of it and eps, 1e-9 N·m: T = 20.0000333 N·m)
    let b = lsim_lib::rotational::BREAKAWAY;
    let t_star = (3.0 * (1.0 + b) + 1e-9) / (0.25 - 0.1 * (1.0 + b)) / 10.0;
    let t = ev(&res, "breaks away forwards");
    assert!((t - t_star).abs() < 1e-9, "break-away at {t}, by hand {t_star}");
    assert!(res.report.warnings.is_empty(), "{:?}", res.report.warnings);
    // held until then: both at T/4 = 2.5·t rad/s², ω = 1.25·t²
    let (wa, wb) = (at(&res, "ja.w", 1.5), at(&res, "jb.w", 1.5));
    assert!((wa - 1.25 * 2.25).abs() < 1e-6 && (wb - wa).abs() < 1e-9, "{wa} {wb}");
    books_close(&res, 1e-7);
}
