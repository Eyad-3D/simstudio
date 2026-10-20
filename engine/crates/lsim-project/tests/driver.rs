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
