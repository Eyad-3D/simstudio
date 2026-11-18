//! The PID and Lookup blocks as today's engine runs them (its
//! `ControlSlave`): a PID is a discrete controller, run at today's solver
//! step (10 ms) or at its Sample Time when that is longer, and a PID or
//! Lookup with a Sample Time longer than the step holds its output between
//! samples, taken at the first step at or after each multiple of it.

use lsim_project::golden::{CaseRun, run_case};
use lsim_solve::SolverOptions;
use serde_json::{Value, json};
use std::path::PathBuf;

fn bev() -> Value {
    let p =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../backend/projects/bev-car.json");
    serde_json::from_str(&std::fs::read_to_string(p).expect("the example")).expect("JSON")
}

fn el(id: &str, kind: &str, params: Value) -> Value {
    json!({"id": id, "componentDefId": kind, "label": id, "position": {"x": 0, "y": 0},
           "parameterOverrides": params})
}

fn wire(k: usize, a: &str, ap: &str, b: &str, bp: &str) -> Value {
    json!({"id": format!("db-t{k}"), "element1Id": a, "port1Id": ap, "element2Id": b, "port2Id": bp})
}

/// The BEV with `blocks` added (wired by `wires`), its case `case` cut to
/// `duration` s with an output step of `dt` s.
fn with(blocks: Vec<Value>, wires: Vec<Value>, case: &str, duration: f64, dt: f64) -> Value {
    let mut p = bev();
    let els = p["systems"][0]["elements"].as_array_mut().unwrap();
    els.extend(blocks);
    p["dataBusConnections"].as_array_mut().unwrap().extend(wires);
    for c in p["cases"].as_array_mut().unwrap() {
        if c["id"] == case {
            c["duration"] = json!(duration);
            c["timeStep"] = json!(dt);
            c["realtimeFactor"] = json!(0);
        }
    }
    p
}

fn run(p: &Value, case: &str) -> CaseRun {
    let opts = SolverOptions { rtol: 1e-6, atol: 1e-8, ..Default::default() };
    run_case(p, case, &opts, None).unwrap_or_else(|e| panic!("{e}"))
}

fn series(r: &CaseRun, name: &str) -> Vec<(f64, f64)> {
    let res = &r.result;
    let k = res.names.iter().position(|n| n == name).unwrap_or_else(|| panic!("no channel {name}"));
    res.times.iter().copied().zip(res.values[k].iter().copied()).collect()
}

/// The sweep's PID (benchmarks/back_to_back/sweep.py): a setpoint of 10
/// against the car's speed in km/h, P 0.01, I 0.002, limits ±10.
fn pid(sample: f64) -> (Vec<Value>, Vec<Value>) {
    (
        vec![
            el("x-setpoint", "signal.constant", json!({"value": 10.0})),
            el(
                "x-pid",
                "control.pid",
                json!({"sample_time_s": sample, "kp": 0.01, "ki": 0.002,
                                               "kd": 0.0, "out_min": -10.0, "out_max": 10.0}),
            ),
        ],
        vec![
            wire(1, "x-setpoint", "sig_out", "x-pid", "sig_setpoint_in"),
            wire(2, "el-vehicle", "sig_speed", "x-pid", "sig_feedback_in"),
        ],
    )
}

#[test]
fn a_pid_held_at_its_limit_runs_to_the_end() {
    // the city case: the car above the setpoint for long stretches drives
    // the integral to the lower limit (from about 300 s); the continuous
    // PID's frozen integral slid along that limit and stopped the run with
    // an event storm at 309 s
    let (b, w) = pid(0.0);
    let p = with(b, w, "case-city-live", 600.0, 1.0);
    let r = run(&p, "case-city-live");
    let out = series(&r, "x_pid.sig_out");
    assert_eq!(out.last().unwrap().0, 600.0);
    assert!(out.iter().any(|(_, v)| *v == -10.0), "it reaches its limit");
}

/// The car's speed in km/h (what a PID or Lookup wired to it reads).
fn kmh(r: &CaseRun) -> Vec<(f64, f64)> {
    series(r, "el_vehicle.sig_speed").into_iter().map(|(t, v)| (t, v * 3.6)).collect()
}

#[test]
fn a_pid_with_a_sample_time_runs_today_s_law_at_its_samples_and_holds_in_between() {
    // 5 s: samples at 0, 5, 10 … (whole outputs of a 1 s grid), its time
    // step 5 s; the output between them is the last sample's
    let (b, w) = pid(5.0);
    let p = with(b, w, "case-city", 120.0, 1.0);
    let r = run(&p, "case-city");
    let (out, v) = (series(&r, "x_pid.sig_out"), kmh(&r));
    let (mut integral, dt) = (0.0, 5.0);
    let mut want = 0.0;
    for (k, ((t, o), (_, fb))) in out.iter().zip(&v).enumerate() {
        // a point shows the output held up to it: a sample at a point is
        // taken just after it (today records the point before its blocks
        // run there), so it shows from the next point on
        if k == 0 || (k > 1 && k % 5 == 1) {
            let (_, fb) = v[if k == 0 { 0 } else { k - 1 }];
            // today's ControlSlave, P 0.01, I 0.002, D 0, limits ±10
            let err = 10.0 - fb;
            let unsat = 0.01 * err + 0.002 * (integral + err * dt);
            if (-10.0..=10.0).contains(&unsat) || err * unsat < 0.0 {
                integral += err * dt;
            }
            want = (0.01 * err + 0.002 * integral).clamp(-10.0, 10.0);
        }
        let _ = fb;
        assert!((o - want).abs() <= 1e-6 * want.abs().max(1.0), "at t = {t}: {o} against {want}");
    }
}

#[test]
fn a_lookup_with_a_sample_time_reads_its_table_at_its_last_sample() {
    // a table that gives back its input, sampled every 0.5 s: between
    // samples the output holds the speed at the last one
    let blocks = vec![el(
        "x-look",
        "signal.lookup",
        json!({"sample_time_s": 0.5, "table_1d": {"0": 0, "200": 200}}),
    )];
    let wires = vec![wire(1, "el-vehicle", "sig_speed", "x-look", "sig_x_in")];
    let p = with(blocks, wires, "case-city", 60.0, 0.1);
    let r = run(&p, "case-city");
    let (out, v) = (series(&r, "x_look.sig_out"), kmh(&r));
    let at = |t: f64| v.iter().find(|(x, _)| (x - t).abs() < 1e-9).map(|p| p.1).unwrap();
    let mut moved = 0;
    for (t, o) in &out {
        // the last sample before the point (a point shows the output held
        // up to it), or the first at t = 0
        let s = if *t == 0.0 { 0.0 } else { ((t / 0.5 - 1e-6).ceil() - 1.0) * 0.5 };
        let want = at(s);
        assert!(
            (o - want).abs() <= 1e-6 * want.abs().max(1.0),
            "at t = {t}: {o} against {want} (at {s})"
        );
        moved += usize::from((at(*t) - want).abs() > 1e-6);
    }
    assert!(moved > 100, "the speed moves between samples ({moved} points)");
}
