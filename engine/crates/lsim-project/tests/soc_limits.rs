//! The P2 Hybrid Car's HV Battery at its SOC limits: at its minimum SOC
//! while the engine charges it, and full while it goes on charging. Its
//! window (the power it gives and takes) closed at a jump there, its
//! delivering or taking mode switched by the SOC; the engine's charge and
//! the motors' draw drove the SOC back to the limit from each side, the
//! run loop met the crossing each way every 1e-11 to 1e-10 s, and the run
//! stopped with an event storm (the acceptance's fault runs, 20 November
//! 2026).

use serde_json::{Value, json};
use std::path::PathBuf;

fn hybrid() -> Value {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(
        &std::fs::read_to_string(root.join("../../../backend/projects/hybrid-car.json")).unwrap(),
    )
    .unwrap()
}

/// The UDDS to `duration`, one run (no charge balancing), at the golden
/// comparison's tolerance: its run, ended where it ended.
fn udds(mut project: Value, duration: f64) -> lsim_project::golden::CaseRun {
    let case = project["cases"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|c| c["id"] == "case-udds")
        .unwrap();
    case["chargeBalance"] = json!(false);
    case["duration"] = json!(duration);
    let opts = lsim_solve::SolverOptions { rtol: 1e-6, atol: 1e-8, ..Default::default() };
    lsim_project::golden::run_case(&project, "case-udds", &opts, None)
        .unwrap_or_else(|e| panic!("{e}"))
}

/// Its battery a hundredth of its own (15.6 Wh): empty after about 200 s,
/// then held at its minimum SOC, 10 %, while the engine charges it and the
/// motors draw what it gives (it stopped at 207.38 s). The SOC never goes
/// below the minimum (but for rounding), and it sits there, within the
/// window's taper, until the engine has charged it above.
#[test]
fn a_hybrid_charged_at_its_minimum_soc_runs_on() {
    let mut project = hybrid();
    let bat = project["systems"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .flat_map(|s| s["elements"].as_array_mut().unwrap())
        .find(|e| e["id"] == "el-battery")
        .unwrap();
    assert_eq!(bat["parameterOverrides"]["capacity_kWh"], json!(1.56));
    bat["parameterOverrides"]["capacity_kWh"] = json!(1.56 / 100.0);
    let run = udds(project, 300.0);
    let r = &run.result;
    assert_eq!(r.times.last().copied(), Some(300.0), "it ran to its end");
    let soc = r.channel("el_battery.soc").expect("soc");
    let low = soc.iter().copied().fold(f64::INFINITY, f64::min);
    let k0 = soc.iter().position(|s| *s <= 0.10 + 1e-4).expect("it empties");
    println!("at the minimum from {} s; lowest SOC {low}", r.times[k0]);
    assert!(low >= 0.10 - 1e-9, "below its minimum: {low}");
    let at = soc[k0..].iter().filter(|s| **s <= 0.10 + 1e-4).count();
    println!("{at} output points at the minimum");
    assert!(at >= 10, "it sits at its minimum: {at} points");
    let books = r.energy.as_ref().unwrap().relative_closure;
    assert!(books < 1.5e-9, "the books close to {books:e}");
}

/// Its control unit not told the SOC (the data link from the battery cut,
/// the fault `db-2`): the engine charges the battery until it is full and
/// goes on (on the whole cycle it stopped at 368.68 s, 101 events within
/// 1.36 s). The SOC never goes above 100 % (but for rounding), and it sits
/// there, within the window's taper, while the engine goes on charging.
#[test]
fn a_hybrid_charged_past_full_runs_on() {
    let mut project = hybrid();
    let links = project["dataBusConnections"].as_array_mut().unwrap();
    let n = links.len();
    links.retain(|c| c["id"] != "db-2");
    assert_eq!(links.len(), n - 1, "the link from the battery's SOC");
    let run = udds(project, 1369.0);
    let r = &run.result;
    assert_eq!(r.times.last().copied(), Some(1369.0), "it ran to its end");
    let soc = r.channel("el_battery.soc").expect("soc");
    let high = soc.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let k1 = soc.iter().position(|s| *s >= 1.0 - 1e-4).expect("it fills");
    println!("full from {} s; highest SOC {high}", r.times[k1]);
    assert!(high <= 1.0 + 1e-9, "above full: {high}");
    let at = soc[k1..].iter().filter(|s| **s >= 1.0 - 1e-4).count();
    println!("{at} output points full");
    assert!(at >= 10, "it sits full: {at} points");
    let books = r.energy.as_ref().unwrap().relative_closure;
    assert!(books < 1.5e-9, "the books close to {books:e}");
}
