//! The E-Motor at an empty battery: its supply short of even its spin
//! losses, its shaft feeding them, and the car coasting to rest with the
//! accelerator down;
//! and at its full-load torque, the last torque point of its loss map.

use serde_json::json;
use std::path::PathBuf;

/// The Battery Electric Car on its city cycle from 4.1 % SOC (its minimum
/// is 4 %): the battery is empty after 26 s, at 42 km/h, and the car
/// coasts to rest with the accelerator fully down. While the inverter was
/// off, the share of the command its supply allows was still solved from
/// the supply's window: with no room even for the spin losses, the share
/// whose power fits is a generator torque that covers them, which grows
/// as the rotor slows, and below about 4 rad/s there is none. The run
/// stopped there with IDA's flag -4 (at 124.8 s). Since 20 November 2026 a
/// short supply does not switch the inverter off (off on a window that did
/// not cover its spin losses, it switched on and off without end as a
/// battery's window tapered through them): short of even its spin losses,
/// the motor takes its whole window, the share is not solved, and its shaft
/// feeds the rest of its spin losses, a torque against the motion. The
/// battery stays at its minimum, and the car comes to rest.
#[test]
fn a_car_whose_battery_is_empty_coasts_to_rest() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut project: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("../../../backend/projects/bev-car.json")).unwrap(),
    )
    .unwrap();
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
    let soc = ch("el_battery.soc");
    let (p_elec, p_hi) = (ch("el_motor.p_elec"), ch("el_motor.p_hi"));
    let (torque, w) = (ch("el_motor.T"), ch("el_motor.w"));
    let v = ch("el_vehicle.v");
    // empty early, at speed, and never below its minimum
    let k0 = soc.iter().position(|s| *s <= 0.04 + 1e-9).expect("the battery empties");
    println!("empty at {} s, {:.2} km/h", r.times[k0], 3.6 * v[k0]);
    assert!(r.times[k0] < 60.0 && v[k0] > 10.0, "{} s, {} m/s", r.times[k0], v[k0]);
    assert!(soc.iter().all(|s| *s >= 0.04 - 1e-12));
    let mut fed = 0;
    for k in k0 + 1..r.times.len() {
        // short: it takes its whole window, no more
        assert!(p_elec[k] == p_hi[k] && p_elec[k] >= 0.0, "the motor's power at {} s", r.times[k]);
        // its shaft feeds the rest of its spin losses: a torque against
        // the motion (above the speed below which it fades)
        if w[k].abs() > 0.1 {
            assert!(torque[k] * w[k] < 0.0, "the torque at {} s: {}", r.times[k], torque[k]);
            fed += 1;
        }
    }
    assert!(fed > 10, "{fed} points feeding its losses from the shaft");
    // at rest well before the end, and staying there
    let rest = r.times.iter().position(|t| *t >= 500.0).unwrap();
    assert!(v[rest..].iter().all(|x| x.abs() < 1e-9), "{:?}", &v[rest..rest + 3]);
    let books = r.energy.as_ref().unwrap().relative_closure;
    assert!(books < 1e-6, "the books close to {books:e}");
}

/// The Battery Electric Car with its final drive's ratio a hundredth of its
/// own (0.128 for 12.8): its motor at full load cannot move it, and it
/// stands with the accelerator down, the motor at its full-load torque,
/// 310 N·m, which is the last torque point of its loss map (an axis that
/// allows no values outside). The motor's torque was the share of the
/// command its supply allows times the command's torque; where the supply
/// does not cut it the share's equation is "the share is 1", which the
/// integrator meets only to its tolerance: at 22.76 s the share was
/// 1.000000001 and the torque 310.00000031 N·m, and the loss map, read at
/// that torque, was read past its data and the run stopped. A read past its
/// edge by no more than what the iteration variables' tolerances allow in
/// it is on the edge (the run loop's table guard holds it there): the car
/// stands to the end at full load, its torque the command's to the
/// integrator's tolerance.
#[test]
fn a_motor_at_its_full_load_reads_its_loss_map_inside_its_data() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut project: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("../../../backend/projects/bev-car.json")).unwrap(),
    )
    .unwrap();
    let fd = project["systems"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .flat_map(|s| s["elements"].as_array_mut().unwrap())
        .find(|e| e["id"] == "el-final-drive")
        .unwrap();
    assert_eq!(fd["parameterOverrides"]["ratio"], json!(12.8));
    fd["parameterOverrides"]["ratio"] = json!(12.8 / 100.0);
    let opts = lsim_solve::SolverOptions { rtol: 1e-6, atol: 1e-8, ..Default::default() };
    let run = lsim_project::golden::run_case(&project, "case-city", &opts, None)
        .unwrap_or_else(|e| panic!("{e}"));
    let r = &run.result;
    assert_eq!(r.times.last().copied(), Some(600.0), "it ran to its end");
    let ch = |name: &str| r.channel(name).unwrap_or_else(|| panic!("no channel {name}"));
    let (torque, t_cmd, on) = (ch("el_motor.T"), ch("el_motor.t_cmd"), ch("el_motor.on"));
    let mut full = 0;
    for k in 0..r.times.len() {
        if on[k] == 1.0 {
            assert!(
                (torque[k] - t_cmd[k]).abs() <= 1e-6 * t_cmd[k].abs(),
                "at {} s the torque {} against the command's {}",
                r.times[k],
                torque[k],
                t_cmd[k]
            );
            full += usize::from(t_cmd[k] == 310.0);
        }
    }
    assert!(full > 10, "{full} points at full load");
    let dist = ch("el_vehicle.sig_distance");
    println!("distance {} m", dist.last().unwrap());
    assert!(dist.iter().all(|s| s.abs() < 1.0), "it does not drive");
}
