//! The E-Motor at an empty battery: its inverter off, the motor unpowered
//! and dragging, and the car coasting to rest with the accelerator down.

use serde_json::json;
use std::path::PathBuf;

const RPM: f64 = std::f64::consts::PI / 30.0;

/// The motor's drag table (rpm → N·m) at a speed in rad/s, linear between
/// its points and held beyond them, as the block reads it.
fn drag_at(table: &serde_json::Value, w: f64) -> f64 {
    let mut pts: Vec<(f64, f64)> = table
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.parse::<f64>().unwrap() * RPM, v.as_f64().unwrap()))
        .collect();
    pts.sort_by(|a, b| a.0.total_cmp(&b.0));
    let k = pts.partition_point(|p| p.0 <= w);
    if k == 0 {
        return pts[0].1;
    }
    if k == pts.len() {
        return pts[k - 1].1;
    }
    let ((x0, y0), (x1, y1)) = (pts[k - 1], pts[k]);
    y0 + (y1 - y0) * (w - x0) / (x1 - x0)
}

/// The Battery Electric Car on its city cycle from 4.1 % SOC (its minimum
/// is 4 %): the battery is empty after 26 s, at 42 km/h, and the car
/// coasts to rest with the accelerator fully down. While the inverter was
/// off, the share of the command its supply allows was still solved from
/// the supply's window: with no room even for the spin losses, the share
/// whose power fits is a generator torque that covers them, which grows
/// as the rotor slows, and below about 4 rad/s there is none. The run
/// stopped there with IDA's flag -4 (at 124.8 s). The share is not solved
/// while the inverter is off: the motor gives and takes nothing, its
/// torque is its drag, and the car comes to rest.
#[test]
fn a_car_whose_battery_is_empty_coasts_to_rest() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut project: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("../../../backend/projects/bev-car.json")).unwrap(),
    )
    .unwrap();
    let drag = project["systems"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|s| s["elements"].as_array().unwrap())
        .find(|e| e["id"] == "el-motor")
        .unwrap()["parameterOverrides"]["drag_torque"]
        .clone();
    let case = project["cases"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|c| c["id"] == "case-city")
        .unwrap();
    case["parameterOverrides"] = json!({"el-battery": {"initial_soc_pct": 4.1}});
    let opts = lsim_solve::SolverOptions { rtol: 1e-8, atol: 1e-10, ..Default::default() };
    let run = lsim_project::golden::run_case(&project, "case-city", &opts, None)
        .unwrap_or_else(|e| panic!("{e}"));
    let r = &run.result;
    assert_eq!(r.times.last().copied(), Some(600.0), "it ran to its end");
    let ch = |name: &str| r.channel(name).unwrap_or_else(|| panic!("no channel {name}"));
    let (soc, i_bat) = (ch("el_battery.soc"), ch("el_battery.i"));
    let (p_elec, torque, w) = (ch("el_motor.p_elec"), ch("el_motor.T"), ch("el_motor.w"));
    let v = ch("el_vehicle.v");
    // empty early, at speed, and never below its minimum
    let k0 = soc.iter().position(|s| *s <= 0.04 + 1e-9).expect("the battery empties");
    println!("empty at {} s, {:.2} km/h", r.times[k0], 3.6 * v[k0]);
    assert!(r.times[k0] < 60.0 && v[k0] > 10.0, "{} s, {} m/s", r.times[k0], v[k0]);
    assert!(soc.iter().all(|s| *s >= 0.04 - 1e-12));
    let mut dragging = 0;
    for k in k0 + 1..r.times.len() {
        // the inverter off: nothing in or out, the battery idle
        assert_eq!(p_elec[k], 0.0, "the motor's power at {} s", r.times[k]);
        assert!(i_bat[k].abs() < 1e-9, "the battery's current at {} s: {}", r.times[k], i_bat[k]);
        // its torque is its drag against the motion (above the speed
        // below which the drag fades)
        if w[k].abs() > 0.1 {
            let want = -drag_at(&drag, w[k].abs()) * w[k].signum();
            assert!(
                (torque[k] - want).abs() <= 1e-12 * want.abs(),
                "the torque at {} s: {} against the drag {want}",
                r.times[k],
                torque[k]
            );
            dragging += 1;
        }
    }
    assert!(dragging > 10, "{dragging} points dragging");
    // at rest well before the end, and staying there
    let rest = r.times.iter().position(|t| *t >= 500.0).unwrap();
    assert!(v[rest..].iter().all(|x| x.abs() < 1e-9), "{:?}", &v[rest..rest + 3]);
    let books = r.energy.as_ref().unwrap().relative_closure;
    assert!(books < 1e-6, "the books close to {books:e}");
}
