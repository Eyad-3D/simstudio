//! The example cars in fast mode (DESIGN.md section 10): each drive-cycle
//! case imported as fast mode's inverse model (`ImportOptions::inverse`)
//! runs over its cycle, and what fast mode cannot invert is refused with
//! its reason.

use lsim_engine::{BuildOptions, Engine, Model};
use lsim_fast::{FastOptions, FastResult, Trace};
use lsim_project::{ImportOptions, ImportReport, import_case, standard_registry};
use serde_json::{Value, json};
use std::path::PathBuf;

fn project(name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../../backend/projects/{name}.json"));
    serde_json::from_str(&std::fs::read_to_string(path).expect("the example project"))
        .expect("JSON")
}

/// The example project with one drive-cycle case, `case-CYCLE`, on the
/// bundled cycle `cycle` over its whole length (its first case on a
/// bundled cycle, the cycle swapped).
fn on_cycle(name: &str, cycle: &str) -> Value {
    let mut pj = project(name);
    let csv = std::fs::read_to_string(
        lsim_project::import::bundled_cycles_dir().join(format!("{cycle}.csv")),
    )
    .expect("the bundled cycle");
    let last = csv.lines().rev().find(|l| !l.trim().is_empty()).unwrap();
    let duration: f64 = last.split(',').next().unwrap().trim().parse().unwrap();
    let mut c = pj["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["parameterOverrides"]["el-task"]["cycle"].is_string())
        .expect("a case on a bundled cycle")
        .clone();
    c["id"] = json!(format!("case-{cycle}"));
    c["duration"] = json!(duration);
    c["parameterOverrides"] = json!({"el-task": {"cycle": cycle}});
    c.as_object_mut().unwrap().remove("references");
    pj["cases"] = json!([c]);
    pj
}

fn inverse(name: &str, case: &str) -> Result<(Model, ImportReport), String> {
    inverse_of(&project(name), case)
}

fn inverse_of(pj: &Value, case: &str) -> Result<(Model, ImportReport), String> {
    let opts = ImportOptions { inverse: true, ..Default::default() };
    let (top, rep) = import_case(pj, Some(case), &standard_registry(), &opts)
        .map_err(|d| d.iter().map(|x| x.to_string()).collect::<Vec<_>>().join("\n"))?;
    let pr = rep.inverse.as_ref().expect("the cycle prescribed");
    let m = Engine::new(rep.library())
        .build_inverse(&top, &pr.spec, &BuildOptions::default())
        .map_err(|d| d.iter().map(|x| x.to_string()).collect::<Vec<_>>().join("\n"))?;
    Ok((m, rep))
}

fn fast(m: &Model, rep: &ImportReport) -> FastResult {
    let pr = rep.inverse.as_ref().unwrap();
    let trace = Trace::new(pr.t.clone(), pr.v.clone()).unwrap();
    let o = FastOptions { t_end: Some(rep.run.duration.min(trace.end())), ..Default::default() };
    m.fast(&[trace], &o).unwrap_or_else(|e| panic!("{e}"))
}

/// The battery electric cars' cycles, which they can follow: each runs in
/// fast mode over its whole cycle without a flag, the vehicle's speed the
/// cycle's at every second.
#[test]
fn the_example_cars_drive_their_cycles_in_fast_mode() {
    for (name, case) in [
        ("aero-bev", "case-udds"),
        ("aero-bev", "case-hwfet"),
        ("aero-bev", "case-wltc"),
        ("bev-car", "case-city"),
        ("bev-car", "case-wltc"),
        ("bev-car", "case-wltc-winter"),
    ] {
        let (m, rep) = inverse(name, case).unwrap_or_else(|e| panic!("{name}/{case}: {e}"));
        assert_eq!(m.report.mode, "fast");
        let pr = rep.inverse.as_ref().unwrap();
        assert_eq!(pr.spec.prescribed, ["el_vehicle.v"]);
        assert_eq!(pr.spec.freed, ["el_driver.sig_cmd_in"]);
        let r = fast(&m, &rep);
        assert!(r.flags.is_empty(), "{name}/{case}: {:?}", r.flags);
        let trace = Trace::new(pr.t.clone(), pr.v.clone()).unwrap();
        let v = r.channel("el_vehicle.v").unwrap();
        for (k, t) in r.times.iter().enumerate() {
            let want = trace.on_segment(trace.segment(*t), *t).0;
            assert!((v[k] - want).abs() <= 1e-12 * want.abs().max(1.0), "{name}/{case} at {t} s");
        }
        assert_eq!(*r.times.last().unwrap(), rep.run.duration);
    }
}

/// An acceleration test has no cycle to prescribe, and the hybrid's
/// supervisory controller, a Script block, reads the driver's pedal at its
/// ticks: fast mode refuses both, saying why.
#[test]
fn fast_mode_refuses_what_it_cannot_invert() {
    let e = inverse("fs-electric", "case-accel-75m").err().expect("refused");
    assert!(e.contains("FAST-NO-CYCLE") && e.contains("acceleration test"), "{e}");
    let e = inverse("hybrid-car", "case-udds").err().expect("refused");
    assert!(e.contains("FAST-SAMPLED-CONTROL") && e.contains("Hybrid Control Unit"), "{e}");
}

/// Fast mode against the full dynamic run of the same car on the same
/// cycle, which raises no flag: the charge used agrees within 1 % (DESIGN.md
/// section 10). The UDDS stops the car 17 times; each start from rest sets
/// the rolling resistance's direction, a mode on the acceleration, at the
/// cycle's sample where the car drives off (it stayed off, and the charge
/// used was 48 % short, before fast mode treated a sample as an input
/// event). New York's city cycle stands a third of its time: the inverter
/// of a car standing with nothing to drive draws nothing (fast mode
/// charged its no-load loss there, 1.4 % over the full dynamic run).
#[test]
fn fast_mode_and_the_full_dynamic_run_agree_on_the_charge_used() {
    for (name, case, pj) in [
        ("aero-bev", "case-udds", project("aero-bev")),
        ("bev-car", "case-city", project("bev-car")),
        ("aero-bev", "case-nycc", on_cycle("aero-bev", "nycc")),
        ("bev-car", "case-nycc", on_cycle("bev-car", "nycc")),
    ] {
        let (m, rep) = inverse_of(&pj, case).unwrap();
        let r = fast(&m, &rep);
        let soc = r.channel("el_battery.soc").unwrap();
        let used_fast = soc[0] - soc[soc.len() - 1];
        let (top, frep) =
            import_case(&pj, Some(case), &standard_registry(), &ImportOptions::default()).unwrap();
        let fwd = lsim_project::model::Model::build(&frep.library(), &top).unwrap();
        let grid = lsim_solve::OutputGrid { t0: 0.0, t_end: frep.run.duration, dt: 1.0 };
        let opts = lsim_solve::SolverOptions { rtol: 1e-6, atol: 1e-8, ..Default::default() };
        let run = fwd.run(&opts, grid, &mut []).unwrap();
        let soc = run.channel("el_battery.soc").unwrap();
        let used_full = soc[0] - soc[soc.len() - 1];
        let d = (used_fast - used_full) / used_full;
        println!(
            "{name}/{case}: charge used fast {used_fast:.6}, full dynamic {used_full:.6} ({d:+.2e})"
        );
        assert!(r.flags.is_empty());
        assert!(d.abs() < 0.01, "{name}/{case}: fast {used_fast}, full dynamic {used_full}");
        // every start from rest is an event at the sample it starts at
        let pr = rep.inverse.as_ref().unwrap();
        let starts = (1..pr.t.len() - 1)
            .filter(|&k| pr.v[k] == 0.0 && pr.v[k + 1] > 0.0 && pr.t[k] < rep.run.duration)
            .count();
        let at_samples = r
            .events
            .iter()
            .filter(|e| pr.t.binary_search_by(|t| t.total_cmp(&e.t)).is_ok())
            .count();
        assert!(
            at_samples >= starts,
            "{name}/{case}: {at_samples} events at samples, {starts} starts"
        );
    }
}

/// Every bundled cycle the full dynamic run of the example cars completes
/// runs in fast mode too, without a flag where the car follows it: on
/// these, a prediction across a jump of the acceleration put Newton's
/// method into a region where the torque did not depend on the pedal (a
/// motor command held at 1, friction brakes at their capacity, a supply's
/// cut), and the run stopped on a singular Jacobian, or it found a second
/// root of the equations (a tyre slipping backwards, a battery current
/// with the bus voltage near 0) and flagged limits the car never reached.
#[test]
fn fast_mode_runs_the_cycles_it_stopped_on() {
    for (name, cycle) in [
        ("bev-car", "hwfet"),
        ("bev-car", "nedc"),
        ("bev-car", "wmtc-part2"),
        ("bev-car", "wmtc-part3"),
        ("bev-car", "ftp-75"),
        ("bev-car", "wltc-3-extra-high"),
        ("aero-bev", "la92"),
        ("aero-bev", "us06"),
    ] {
        let case = format!("case-{cycle}");
        let (m, rep) = inverse_of(&on_cycle(name, cycle), &case)
            .unwrap_or_else(|e| panic!("{name}/{cycle}: {e}"));
        let pr = rep.inverse.as_ref().unwrap();
        let trace = Trace::new(pr.t.clone(), pr.v.clone()).unwrap();
        let o =
            FastOptions { t_end: Some(rep.run.duration.min(trace.end())), ..Default::default() };
        let r = m.fast(&[trace], &o).unwrap_or_else(|e| panic!("{name}/{cycle}: {e}"));
        assert!(r.flags.is_empty(), "{name}/{cycle}: {:?}", r.flags);
        // the pedal stays inside full brake … full throttle (it reached
        // 3.7e15 at one point inside a step of the FTP-75)
        let j = r.names.iter().position(|n| n == "el_driver.cmd").unwrap();
        for table in [&r.min, &r.max] {
            let c = r.column(table, j);
            assert!(c.iter().all(|v| v.abs() <= 1.0), "{name}/{cycle}: pedal {c:?}");
        }
    }
}

/// A change of the cycle at round-off level changes fast mode's result at
/// round-off level only: no flag appears, the charge used moves by much
/// less than a millionth (the FTP-75's first 1369 s differ from the UDDS
/// by 61 samples within 8e-4 km/h of its lines, and fast mode flagged the
/// pedal, two tyres' grip and a pedal of 3.7e15 on one and not the other).
#[test]
fn a_round_off_change_of_the_cycle_changes_nothing_else() {
    let pj = on_cycle("bev-car", "ftp-75");
    let (m, rep) = inverse_of(&pj, "case-ftp-75").unwrap();
    let pr = rep.inverse.as_ref().unwrap();
    let o = FastOptions { t_end: Some(rep.run.duration), ..Default::default() };
    let used = |v: Vec<f64>| {
        let r = m.fast(&[Trace::new(pr.t.clone(), v).unwrap()], &o).unwrap();
        assert!(r.flags.is_empty(), "{:?}", r.flags);
        let soc = r.channel("el_battery.soc").unwrap();
        soc[0] - soc[soc.len() - 1]
    };
    let a = used(pr.v.clone());
    let shaken =
        pr.v.iter().enumerate().map(|(k, v)| v * (1.0 + if k % 2 == 0 { 1e-9 } else { -1e-9 }));
    let b = used(shaken.collect());
    assert!((a - b).abs() <= 1e-6 * a, "charge used {a} against {b}");
}

/// A car whose battery runs empty on its cycle could not have driven on:
/// fast mode flags it, from the instant its state of charge reaches the
/// minimum SOC (the battery's own window and the motor's power beyond what
/// it then gives). The BEV on the UDDS from 5 % (its minimum 4 %): before,
/// the run went on with the battery drained below empty and no flag (the
/// motor's flag tolerance was set by its open lower bound, 1e30 W, and the
/// state of charge had none).
#[test]
fn a_battery_that_runs_empty_is_flagged() {
    let mut pj = on_cycle("bev-car", "udds");
    pj["cases"][0]["parameterOverrides"]["el-battery"] = json!({"initial_soc_pct": 5});
    let (m, rep) = inverse_of(&pj, "case-udds").unwrap();
    let r = fast(&m, &rep);
    let soc = r.channel("el_battery.soc").unwrap();
    let k = soc.iter().position(|s| *s < 0.04).expect("the battery runs empty");
    let empty = r.flags.iter().find(|f| f.part == "el_battery" && !f.upper).expect("flagged");
    assert!(
        empty.t_start > r.times[k - 1] && empty.t_start <= r.times[k],
        "flagged from {} s, empty between {} and {} s",
        empty.t_start,
        r.times[k - 1],
        r.times[k]
    );
    assert!(r.flags.iter().any(|f| f.part == "el_motor" && f.upper), "{:?}", r.flags);
}
