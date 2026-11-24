//! The library's Driver held at its limit: its anti-windup against the
//! closed form of `LimPID`'s back-calculation, and the review's two cars
//! whose frozen integral stopped the run as an event storm.

mod common;

use common::*;
use lsim_ir::ComponentDef;
use lsim_ir::math::{cos, exp, sin};
use lsim_lib::x::*;
use lsim_solve::SolverOptions;
use std::path::PathBuf;

/// The app's default gains in SI (0.35 1/(km/h), 0.08 1/(km/h·s)) and
/// `LimPID`'s default Ni.
const KP: f64 = 0.35 * 3.6;
const KI: f64 = 0.08 * 3.6;
const NI: f64 = 0.9;
/// A car that can accelerate at only 0.1 m/s², asked for 10 m/s at once.
const A: f64 = 0.1;
const V: f64 = 10.0;

/// A point car: its acceleration is the pedals' command times `a`. It
/// hands the Driver its speed less the target `vt`, so the Driver's own
/// target (0) less that is the speed error.
fn point_car() -> ComponentDef {
    ComponentDef {
        name: "Test.PointCar".into(),
        doc: "A car whose acceleration is its pedal command times a.".into(),
        ports: vec![
            input("acc", "1", "the accelerator pedal"),
            input("brk", "1", "the brake pedal"),
            output("v_rel", "m/s", "the speed less the target"),
            output("cap", "N.m", "no recuperation"),
        ],
        params: vec![
            p("a", "m/s2", A, "the acceleration at a full command"),
            p("vt", "m/s", V, "the target speed"),
            p("unit_Nm", "N.m", 1.0, "unit carrier"),
        ],
        vars: vec![state("v", "m/s", 0.0, "the speed")],
        equations: vec![
            eq(der("v"), n("a") * (n("acc") - n("brk")), "the pedals accelerate it"),
            eq(n("v_rel"), n("v") - n("vt"), "the speed less the target"),
            eq(n("cap"), c(0.0) * n("unit_Nm"), "no recuperation"),
        ],
        ..Default::default()
    }
}

/// The closed form. Held at full throttle from the start (kp·V > 1), the
/// car's speed is a·t and the error e = V − a·t; back-calculation makes
/// I' = e + (1 − kp·e − ki·I)/(Ni·kp), linear in I: I = c0 + c1·t + (0 −
/// c0)·exp(−λ·t), λ = ki/(Ni·kp). The command leaves its limit at t_x,
/// where kp·e + ki·I = 1; then e'' + a·kp·e' + a·ki·e = 0 from e(t_x) and
/// e'(t_x) = −a, a damped oscillation (its command stays inside ±1).
struct Exact {
    t_x: f64,
}

impl Exact {
    fn lambda() -> f64 {
        KI / (NI * KP)
    }

    /// I in the saturated phase.
    fn i_sat(t: f64) -> f64 {
        let lam = Self::lambda();
        let alpha = V * (1.0 - 1.0 / NI) + 1.0 / (NI * KP);
        let beta = A * (1.0 / NI - 1.0);
        let c1 = beta / lam;
        let c0 = (alpha - c1) / lam;
        c0 + c1 * t - c0 * exp(-lam * t)
    }

    fn new() -> Exact {
        // the command's excess kp·e + ki·I − 1 falls through 0 once:
        // bisection to the last bit
        let g = |t: f64| KP * (V - A * t) + KI * Self::i_sat(t) - 1.0;
        let (mut lo, mut hi) = (0.0, V / A);
        assert!(g(lo) > 0.0 && g(hi) < 0.0);
        for _ in 0..200 {
            let mid = 0.5 * (lo + hi);
            if g(mid) > 0.0 { lo = mid } else { hi = mid }
        }
        Exact { t_x: 0.5 * (lo + hi) }
    }

    /// (speed, integral) at t.
    fn at(&self, t: f64) -> (f64, f64) {
        if t <= self.t_x {
            return (A * t, Self::i_sat(t));
        }
        let sigma = 0.5 * A * KP;
        let omega = (A * KI - sigma * sigma).sqrt();
        let e_x = V - A * self.t_x;
        let b = (sigma * e_x - A) / omega;
        let s = t - self.t_x;
        let (cs, sn, ex) = (cos(omega * s), sin(omega * s), exp(-sigma * s));
        let e = ex * (e_x * cs + b * sn);
        let de = ex * ((-sigma * e_x + omega * b) * cs + (-sigma * b - omega * e_x) * sn);
        let cmd = -de / A;
        (V - e, (cmd - KP * e) / KI)
    }
}

/// A car that cannot keep up: full throttle for 99.6 s, the integral
/// pulled back while it is held there, the command off its limit at the
/// closed form's instant, and the speed and the integral on the closed
/// form all along (the limit is a kink, not an event: `clamp` is min and
/// max). The frozen integral (conditional integration) slid along the
/// limit here, the car catching up at full throttle (a is below ki/kp² =
/// 0.18 m/s²), and the run stopped as an event storm at 92 s.
#[test]
fn a_saturated_driver_follows_limpids_back_calculation() {
    let res = point_car_with(KP, KI, 250.0);
    let exact = Exact::new();
    // e(t_x) = a·(1 − Ni)·Ti once the start has died away (exp(−λ·t_x) =
    // 1e-11): t_x = V/a − (1 − Ni)·kp/ki
    assert!((exact.t_x - 99.5625).abs() < 1e-9, "t_x = {}", exact.t_x);
    // full throttle up to t_x, off its limit after
    let pedal = res.channel("driver.sig_accel_pedal").unwrap();
    for (k, &t) in res.times.iter().enumerate() {
        assert_eq!(pedal[k] == 1.0, t < exact.t_x, "the pedal at {t} s: {}", pedal[k]);
    }
    let mut worst = (0.0f64, 0.0f64);
    for (k, &t) in res.times.iter().enumerate() {
        let (v, i) = exact.at(t);
        let ev = (res.channel("car.v").unwrap()[k] - v).abs() / V;
        let ei = (res.channel("driver.I").unwrap()[k] - i).abs() / 3.3;
        worst = (worst.0.max(ev), worst.1.max(ei));
    }
    println!("off its limit at {} s; worst error: v {:.1e}, I {:.1e}", exact.t_x, worst.0, worst.1);
    assert!(worst.0 < 1e-6 && worst.1 < 1e-6, "{worst:?}");
    // held at its limit, the integral is pulled back below where today's
    // engine froze it (0)
    assert!(at(&res, "driver.I", 10.0) < -0.4);
}

fn point_car_with(kp: f64, ki: f64, t_end: f64) -> lsim_solve::SimResult {
    let mut lib = lib();
    lib.add(point_car());
    let top = model(
        "DriverAtItsLimit",
        vec![
            part(
                "driver",
                "Blocks.Driver",
                &[("driver_kp", kp), ("driver_ki", ki), ("regen_weight_pct", 0.0)],
            ),
            part("car", "Test.PointCar", &[]),
        ],
        &[
            ("car.v_rel", "driver.v_vehicle"),
            ("car.cap", "driver.regen_cap"),
            ("car.cap", "driver.regen_cut"),
            ("driver.sig_accel_pedal", "car.acc"),
            ("driver.sig_brake_pedal", "car.brk"),
        ],
    );
    run(&lib, &top, t_end, 0.5, 1e-8).1
}

/// Without a P Gain `LimPID`'s Ti, and so its tracking time, would be 0:
/// the Driver takes Ti as Ti_min (1 ms), and the integral is held where
/// the command meets its limit (1/ki), as the frozen integral held it,
/// to within the tracking time's share, Ni·Ti_min·e. With neither gain
/// the command is 0, and the integral is the error's (no 0/0).
#[test]
fn a_driver_without_a_p_gain_or_without_gains() {
    let res = point_car_with(0.0, KI, 20.0);
    for t in [5.0, 10.0, 20.0] {
        let e = V - at(&res, "car.v", t);
        let held = 1.0 / KI + NI * 1e-3 * e;
        let got = at(&res, "driver.I", t);
        assert!((got - held).abs() < 1e-6 * held, "I at {t} s: {got}, held at {held}");
        assert_eq!(at(&res, "driver.sig_accel_pedal", t), 1.0);
    }
    let res = point_car_with(0.0, 0.0, 20.0);
    for t in [5.0, 10.0, 20.0] {
        assert_eq!(at(&res, "car.v", t), 0.0, "no command, no motion");
        let got = at(&res, "driver.I", t);
        assert!((got - V * t).abs() < 1e-9 * V * t, "I at {t} s: {got}");
    }
}

/// 40 CFR 1066.425(b): the speed within 2.0 mi/h above the highest and
/// below the lowest point of the schedule within 1.0 s of each instant.
/// The output times outside where neither pedal is fully down: behind at
/// full throttle, the car is driven at its maximum available power, which
/// the rule allows (1066.425(b)(4)(ii), 1066.415(e)(5)); ahead at full
/// brake, it cannot brake as the schedule asks.
fn outside_the_band(r: &lsim_solve::SimResult) -> Vec<f64> {
    let ch = |name: &str| r.channel(name).unwrap_or_else(|| panic!("no channel {name}"));
    let (v, target) = (ch("el_vehicle.v"), ch("el_task.sig_demand"));
    let (acc, brk) = (ch("el_driver.sig_accel_pedal"), ch("el_driver.sig_brake_pedal"));
    let tol = 2.0 * 1.609344 / 3.6;
    let t = &r.times;
    let mut out = vec![];
    let (mut a, mut b) = (0, 0);
    for k in 0..t.len() {
        while t[a] < t[k] - 1.0 - 1e-9 {
            a += 1;
        }
        while b + 1 < t.len() && t[b + 1] <= t[k] + 1.0 + 1e-9 {
            b += 1;
        }
        let window = &target[a..=b];
        let lo = window.iter().copied().fold(f64::INFINITY, f64::min) - tol;
        let hi = window.iter().copied().fold(f64::NEG_INFINITY, f64::max) + tol;
        if (v[k] < lo && acc[k] < 1.0) || (v[k] > hi && brk[k] < 1.0) {
            out.push(t[k]);
        }
    }
    out
}

/// The review's two cars (finding 37): the Battery Electric Car on the
/// WLTC at 6000 kg stopped at 961.83 s, and on its city cycle at 19 270 kg
/// at 2.64 s, each with an event storm of the frozen integral. Both run to
/// their ends, every 0.1 s inside the speed-trace rule's band wherever
/// the Driver has neither pedal fully down, their books closed.
#[test]
fn the_heavy_cars_that_stormed_run() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let project: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("../../../backend/projects/bev-car.json")).unwrap(),
    )
    .unwrap();
    let opts = SolverOptions { rtol: 1e-6, atol: 1e-8, ..Default::default() };
    for (case, mass) in [("case-wltc", 6000.0), ("case-city", 19_270.0)] {
        let mut pj = project.clone();
        let entry =
            pj["cases"].as_array_mut().unwrap().iter_mut().find(|x| x["id"] == case).unwrap();
        entry["parameterOverrides"]["el-vehicle"]["mass_kg"] = serde_json::json!(mass);
        entry["timeStep"] = serde_json::json!(0.1);
        let duration = entry["duration"].as_f64().unwrap();
        let run = lsim_project::golden::run_case(&pj, case, &opts, None)
            .unwrap_or_else(|e| panic!("{case} at {mass} kg: {e}"));
        assert_eq!(run.result.times.last().copied(), Some(duration), "{case} ran to its end");
        let outside = outside_the_band(&run.result);
        assert!(outside.is_empty(), "{case}: outside the band at {outside:?}");
        let books = run.result.energy.as_ref().unwrap().relative_closure;
        assert!(books < 1.5e-9, "{case}: the books close to {books:e}");
    }
}

// ---- a performance test's Driver: full throttle up to the target ----------

/// A point car whose target steps from 0 to `vt` at `t_step` (a Driving
/// Task's profile "0:0; t_step:vt"), accelerating at `a` per pedal.
fn stepped_car() -> ComponentDef {
    ComponentDef {
        name: "Test.SteppedCar".into(),
        doc: "A point car whose target is 0 until t_step.".into(),
        ports: vec![
            input("acc", "1", "the accelerator pedal"),
            input("brk", "1", "the brake pedal"),
            output("v_rel", "m/s", "the speed less the target"),
            output("cap", "N.m", "no recuperation"),
        ],
        params: vec![
            p("a", "m/s2", A, "the acceleration at a full command"),
            p("vt", "m/s", V, "the target speed"),
            p("t_step", "s", 0.0, "when the target steps"),
            p("drag", "1/s", 0.0, "the speed lost per second per m/s"),
            p(
                "off",
                "m/s",
                0.0,
                "a start error that is gone at 0.5 s (a start not solved to the digit)",
            ),
            p("unit_Nm", "N.m", 1.0, "unit carrier"),
        ],
        vars: vec![state("v", "m/s", 0.0, "the speed")],
        equations: vec![
            eq(
                der("v"),
                n("a") * (n("acc") - n("brk")) - n("drag") * n("v"),
                "the pedals accelerate it, drag slows it",
            ),
            eq(
                n("v_rel"),
                n("v") - n("vt") * ite(ge(time(), n("t_step")), c(1.0), c(0.0))
                    + n("off") * ite(lt(time(), c(0.5)), c(1.0), c(0.0)),
                "the speed less the target (0 until t_step), less a start error",
            ),
            eq(n("cap"), c(0.0) * n("unit_Nm"), "no recuperation"),
        ],
        ..Default::default()
    }
}

fn performance_run(a: f64, t_step: f64, drag: f64, t_end: f64) -> lsim_solve::SimResult {
    performance_run_off(a, t_step, drag, t_end, 0.0)
}

fn performance_run_off(
    a: f64,
    t_step: f64,
    drag: f64,
    t_end: f64,
    off: f64,
) -> lsim_solve::SimResult {
    let mut lib = lib();
    lib.add(stepped_car());
    let driver = lsim_lib::blocks::vehicle::driver(lsim_lib::blocks::vehicle::DriverConfig {
        performance: true,
        ..Default::default()
    });
    let name = driver.name.clone();
    lib.add(driver);
    let top = model(
        "PerformanceDriver",
        vec![
            part(
                "driver",
                &name,
                &[("driver_kp", KP), ("driver_ki", KI), ("regen_weight_pct", 0.0)],
            ),
            part(
                "car",
                "Test.SteppedCar",
                &[("a", a), ("t_step", t_step), ("drag", drag), ("off", off)],
            ),
        ],
        &[
            ("car.v_rel", "driver.v_vehicle"),
            ("car.cap", "driver.regen_cap"),
            ("car.cap", "driver.regen_cut"),
            ("driver.sig_accel_pedal", "car.acc"),
            ("driver.sig_brake_pedal", "car.brk"),
        ],
    );
    run(&lib, &top, t_end, 0.05, 1e-8).1
}

/// A performance test holds full throttle from the step in the target
/// until the speed first gets there (an error of 0 at V/a after the step),
/// and then the PI holds it: the integral did not wind up at full throttle,
/// so the command falls at once to the PI's (a car with no drag stays at the
/// target at no throttle), where the ordinary Driver, its integral pulled
/// back to the limit, would overshoot. With the target at 0 until
/// `t_step` (an error of 0, not below it) the run is not yet "reached".
#[test]
fn a_performance_driver_holds_full_throttle_up_to_the_target() {
    let (a, t_step) = (2.0, 3.0);
    let res = performance_run(a, t_step, 0.0, 12.0);
    let t_reach = t_step + V / a;
    let pedal = res.channel("driver.sig_accel_pedal").unwrap();
    let v = res.channel("car.v").unwrap();
    for (k, &t) in res.times.iter().enumerate() {
        if t < t_step - 1e-9 {
            assert_eq!(pedal[k], 0.0, "no throttle before the step, at {t} s");
        } else if t > t_step + 1e-9 && t < t_reach - 1e-9 {
            assert_eq!(pedal[k], 1.0, "full throttle on the way, at {t} s: {}", pedal[k]);
        } else if t > t_reach + 1e-3 {
            assert!(pedal[k].abs() < 1e-3, "the PI's own command after it, at {t} s: {}", pedal[k]);
            assert!((v[k] - V).abs() < 1e-3, "held at the target at {t} s: {}", v[k]);
        }
    }
    let t_hit = res.times.iter().zip(v).find(|(_, v)| **v >= V - 1e-6).map(|(t, _)| *t).unwrap();
    assert!((t_hit - t_reach).abs() < 0.06, "the target is reached at {t_hit} s, not {t_reach} s");
}

/// A target above the speed from the start (0:100 at t = 0) is the same.
#[test]
fn a_performance_driver_with_a_target_from_the_start() {
    let res = performance_run(2.0, 0.0, 0.0, 10.0);
    let pedal = res.channel("driver.sig_accel_pedal").unwrap();
    for (k, &t) in res.times.iter().enumerate() {
        if t > 1e-9 && t < V / 2.0 - 1e-9 {
            assert_eq!(pedal[k], 1.0, "full throttle at {t} s: {}", pedal[k]);
        } else if t > V / 2.0 + 1e-3 {
            assert!(pedal[k].abs() < 1e-3, "the PI's after it, at {t} s: {}", pedal[k]);
        }
    }
}

/// A car with drag that has just got to the target is held there by the PI
/// (the integral did not wind up, so the command starts from the PI's own),
/// and is not let off the throttle and put back on at every crossing of the
/// target: the run ends at the target with a handful of events.
#[test]
fn a_performance_driver_does_not_chatter_where_it_gets_there() {
    let res = performance_run(2.0, 0.0, 0.05, 40.0);
    let v = res.channel("car.v").unwrap();
    assert!((v[v.len() - 1] - V).abs() < 0.02 * V, "held at the target: {}", v[v.len() - 1]);
    assert!(res.events.len() < 20, "{} events", res.events.len());
    let pedal = res.channel("driver.sig_accel_pedal").unwrap();
    for (k, &t) in res.times.iter().enumerate() {
        if t > 1e-9 && t < 4.0 {
            assert_eq!(pedal[k], 1.0, "full throttle on the way at {t} s: {}", pedal[k]);
        }
    }
}

/// A car at rest with the target at 0 has an error of 0 up to the digits the
/// start was solved to (here 5e-9 m/s, gone at 0.5 s): that falling to 0 is
/// not "the speed got to the target", and the Driver must still hold full
/// throttle when the target steps, up to the target (the check found it
/// handing over to the PI as soon as the error fell below 1 m/s).
#[test]
fn a_performance_driver_that_starts_at_its_target_is_not_reached() {
    let (a, t_step) = (2.0, 3.0);
    let res = performance_run_off(a, t_step, 0.0, 12.0, -5e-9);
    let t_reach = t_step + V / a;
    let pedal = res.channel("driver.sig_accel_pedal").unwrap();
    for (k, &t) in res.times.iter().enumerate() {
        if t > t_step + 1e-9 && t < t_reach - 1e-9 {
            assert_eq!(pedal[k], 1.0, "full throttle on the way, at {t} s: {}", pedal[k]);
        }
    }
}
