//! The example cars in fast mode (DESIGN.md section 10): each drive-cycle
//! case imported as fast mode's inverse model (`ImportOptions::inverse`)
//! runs over its cycle, and what fast mode cannot invert is refused with
//! its reason.

use lsim_engine::{BuildOptions, Engine, Model};
use lsim_fast::{FastOptions, FastResult, Trace};
use lsim_project::{ImportOptions, ImportReport, import_case, standard_registry};
use serde_json::Value;
use std::path::PathBuf;

fn project(name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../../backend/projects/{name}.json"));
    serde_json::from_str(&std::fs::read_to_string(path).expect("the example project"))
        .expect("JSON")
}

fn inverse(name: &str, case: &str) -> Result<(Model, ImportReport), String> {
    let opts = ImportOptions { inverse: true, ..Default::default() };
    let (top, rep) = import_case(&project(name), Some(case), &standard_registry(), &opts)
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
