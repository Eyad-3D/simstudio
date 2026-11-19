//! The golden comparison's one figure outside its band, run again at
//! tighter tolerances (the Battery Electric Car's city cycle, three times:
//! under a second). The recuperated energy on the city cycle stays
//! just outside its band however tight the tolerance: the intended
//! difference the golden README describes (today's engine puts a gear's
//! efficiency on its sources' torque), not an integration error. The band
//! and today's 1 ms value come from `golden/results.json`.

use lsim_solve::SolverOptions;
use std::path::PathBuf;

#[test]
fn the_bev_city_recuperation_converges_just_outside_its_band() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let project: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("../../../backend/projects/bev-car.json")).unwrap(),
    )
    .unwrap();
    let results: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("golden/results.json")).unwrap())
            .unwrap();
    let case = results
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["case"] == "case-city")
        .expect("the city case");
    let row = case["figures"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["what"].as_str().unwrap_or("").contains("energy recuperated"))
        .expect("the recuperation figure");
    let (today, band, recorded) = (
        row["today_1ms"].as_f64().unwrap(),
        row["band"].as_f64().unwrap(),
        row["new"].as_f64().unwrap(),
    );
    println!(
        "results.json: today 1 ms {today:.9} kWh, band {band:.6e}, new {recorded:.9} ({:.4} x band)",
        (recorded - today) / band
    );
    let mut off = vec![];
    // the golden comparison's tolerances (atol = rtol / 100), then tighter
    for rtol in [1e-6, 1e-8, 1e-9] {
        let opts = SolverOptions { rtol, atol: rtol * 1e-2, ..Default::default() };
        let run = lsim_project::golden::run_case(&project, "case-city", &opts, None).unwrap();
        let f = run
            .figures
            .iter()
            .find(|f| f.label.contains("energy recuperated"))
            .expect("the figure");
        let x = (f.value - today) / band;
        println!(
            "rtol {rtol:.0e}: {:.9} {} ({x:+.4} x band); closure {:.1e}; {} steps",
            f.value,
            f.unit,
            run.result.energy.as_ref().map(|e| e.relative_closure).unwrap_or(f64::NAN),
            run.result.stats.steps
        );
        off.push(x);
    }
    // converged: the two tightest agree to a thousandth of the band, and
    // the figure is outside it (the intended difference, not the
    // integration's error)
    assert!((off[1] - off[2]).abs() < 1e-3, "{off:?}");
    assert!(off[2].abs() > 1.0, "{off:?}");
    // the golden comparison's own run is the one recorded
    assert!((off[0] - (recorded - today) / band).abs() < 1e-3, "{off:?}");
}
