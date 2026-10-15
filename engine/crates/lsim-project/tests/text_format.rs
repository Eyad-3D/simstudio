//! Every component the example projects use, in every configuration they
//! use it (the vehicle blocks' variants, the sub-systems and the models the
//! importer builds), prints as text and reads back as the same definition:
//! a user can write anything the library builds. And the gear change of
//! the reference problem `mech_gear_change`, written as text
//! (`lsim-lib/text/gear_change.mo`), runs as the same model built in Rust.

mod common;

use common::*;
use lsim_ir::{ComponentDef, Library};
use lsim_lang::{Report, parse_library, parse_with, to_text};
use lsim_project::model::Model;
use lsim_project::reference::load;
use lsim_project::{ImportOptions, import_case, standard_registry};
use lsim_solve::{OutputGrid, SimResult, SolverOptions};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn projects() -> Vec<(String, serde_json::Value)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../backend/projects");
    let mut v: Vec<(String, serde_json::Value)> = std::fs::read_dir(&dir)
        .expect("the example projects")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .map(|p| {
            let text = std::fs::read_to_string(&p).expect("readable");
            (
                p.file_stem().unwrap().to_string_lossy().to_string(),
                serde_json::from_str(&text).expect("JSON"),
            )
        })
        .collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

#[test]
fn every_component_of_the_example_projects_round_trips() {
    let reg = standard_registry();
    let mut faults = vec![];
    // definition name -> the text it printed as (each checked once)
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    let mut tops = 0;
    for (name, project) in projects() {
        for case in project["cases"].as_array().into_iter().flatten() {
            let id = case["id"].as_str().unwrap();
            if case["kind"] == "lap" {
                continue;
            }
            let (top, rep) = import_case(&project, Some(id), &reg, &ImportOptions::default())
                .unwrap_or_else(|e| panic!("{name}/{id}: {e:?}"));
            let lib = rep.library();
            for def in rep.defs.iter().chain(std::iter::once(&top)) {
                let text = to_text(def);
                if seen.get(&def.name) == Some(&text) {
                    continue;
                }
                match parse_with(&text, &lib) {
                    Ok(back) if back == vec![def.clone()] => {}
                    Ok(back) => faults.push(format!(
                        "{name}/{id}: {} reads back differently:\n{text}\n{:#?}\nvs\n{:#?}",
                        def.name, back, def
                    )),
                    Err(e) => faults.push(format!(
                        "{name}/{id}: {} does not read back: {}\n{text}",
                        def.name,
                        Report(&e)
                    )),
                }
                seen.insert(def.name.clone(), text);
            }
            tops += 1;
        }
    }
    println!("{} definitions of {tops} imported cases round-trip", seen.len());
    assert!(faults.is_empty(), "{}", faults.join("\n\n"));
}

const GEAR_CHANGE: &str = include_str!("../../lsim-lib/text/gear_change.mo");

/// The test library with the text's components, and those components.
fn text_library() -> (Library, Library) {
    let mut lib = lib();
    let text = parse_library(GEAR_CHANGE, Some(&lib)).unwrap_or_else(|e| panic!("{}", Report(&e)));
    for d in text.components.values() {
        lib.add(d.clone());
    }
    (lib, text)
}

/// The gear change built in Rust, as `lsim-project/tests/exact.rs` builds
/// it, with the labels the text gives its parts.
fn gear_change_in_rust(gear: &str) -> ComponentDef {
    let pr = load("mech_gear_change").unwrap();
    let mut top = model(
        "Examples.GearChange",
        vec![
            part("motor", "Rotational.Inertia", &[("J", pr.p("J1"))]),
            part("load", "Rotational.Inertia", &[("J", pr.p("J2"))]),
            part("drive", "Rotational.ConstantTorque", &[("tau", pr.p("T"))]),
            part("gear", gear, &[("eta", 1.0)]),
            part(
                "select",
                "Signal.Step",
                &[("y0", pr.p("i1")), ("y1", pr.p("i2")), ("t_step", pr.p("t_shift"))],
            ),
        ],
        &[
            ("drive.flange", "motor.a"),
            ("motor.b", "gear.a"),
            ("gear.b", "load.a"),
            ("select.y", "gear.ratio"),
        ],
    );
    top.doc = "A motor at 200 N.m drives a 135 kg.m2 load through a gear that shifts from 12:1 \
               to 7:1 at 4 s."
        .into();
    for (s, label) in top.components.iter_mut().zip([
        "Motor rotor",
        "Load",
        "Motor torque",
        "Gearbox",
        "Selected ratio",
    ]) {
        s.label = Some(label.into());
    }
    top
}

fn build(lib: &Library, top: &ComponentDef) -> Model {
    Model::build(lib, top).unwrap_or_else(|e| {
        panic!("{}", e.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n"))
    })
}

fn run_model(m: &Model) -> SimResult {
    let opts = SolverOptions { rtol: 1e-9, atol: 1e-11, ..Default::default() };
    m.run(&opts, OutputGrid { t0: 0.0, t_end: 8.0, dt: 0.01 }, &mut []).expect("runs")
}

/// Every number two runs give, bit for bit: channels, left limits, events,
/// energy books.
fn assert_bitwise(a: &SimResult, b: &SimResult) {
    let bits = |v: &[f64]| v.iter().map(|x| x.to_bits()).collect::<Vec<_>>();
    assert_eq!(a.names, b.names);
    assert_eq!(bits(&a.times), bits(&b.times));
    for (k, (x, y)) in a.values.iter().zip(&b.values).enumerate() {
        assert_eq!(bits(x), bits(y), "channel {}", a.names[k]);
    }
    assert_eq!(a.left_limits.len(), b.left_limits.len());
    for (x, y) in a.left_limits.iter().zip(&b.left_limits) {
        assert_eq!((x.k, &x.channels), (y.k, &y.channels));
        assert_eq!(bits(&x.before), bits(&y.before));
    }
    let ev = |r: &SimResult| {
        r.events.iter().map(|e| (e.t.to_bits(), e.label.clone())).collect::<Vec<_>>()
    };
    assert_eq!(ev(a), ev(b));
    // the books' Debug text holds every number in its shortest exact form
    assert_eq!(format!("{:?}", a.energy), format!("{:?}", b.energy));
}

/// The text's shifting gear is the library's gear, definition for
/// definition, so the gear change written as text prepares to the same
/// model as the one built in Rust and runs to the same bits, lossless (the
/// reference problem) and with an efficiency.
#[test]
fn the_gear_change_in_text_runs_bit_for_bit_as_built_in_rust() {
    let (lib, text) = text_library();
    let rust_gear = lsim_lib::rotational::lossy_gear("Rotational.ShiftingGear", true);
    assert_eq!(text.components["Rotational.ShiftingGear"], rust_gear);
    let from_text = text.components["Examples.GearChange"].clone();
    let mut rust_lib = common::lib();
    rust_lib.add(rust_gear);
    let from_rust = gear_change_in_rust("Rotational.ShiftingGear");
    assert_eq!(from_text, from_rust);
    let (mut a, mut b) = (build(&lib, &from_text), build(&rust_lib, &from_rust));
    assert_eq!(a.prepared, b.prepared, "the same prepared model");
    for eta in [1.0, 0.95] {
        a.set_param("gear.eta", eta).unwrap();
        b.set_param("gear.eta", eta).unwrap();
        let (ra, rb) = (run_model(&a), run_model(&b));
        assert_bitwise(&ra, &rb);
        println!(
            "eta {eta}: text and Rust bit for bit ({} points, {} channels, {} events)",
            ra.times.len(),
            ra.names.len(),
            ra.events.len()
        );
    }
}

/// The momentum `J2 ω2 + i2 J1 ω1` just before and just after the shift.
fn momentum_at_shift(r: &SimResult) -> (f64, f64) {
    let pr = load("mech_gear_change").unwrap();
    let (j1, j2, i2) = (pr.p("J1"), pr.p("J2"), pr.p("i2"));
    let k = r.times.iter().position(|t| *t == pr.p("t_shift")).expect("the shift on the grid");
    let ch = |name: &str| r.names.iter().position(|n| n == name).unwrap() as u32;
    let ll = r.left_limits.iter().find(|l| l.k == k).expect("both sides of the shift recorded");
    let before = |name: &str| {
        let c = ch(name);
        ll.channels.iter().position(|x| *x == c).map(|i| ll.before[i]).expect("it jumps")
    };
    let after = |name: &str| r.values[ch(name) as usize][k];
    (
        j2 * before("load.w") + i2 * j1 * before("motor.w"),
        j2 * after("load.w") + i2 * j1 * after("motor.w"),
    )
}

/// The gear change against the exact answer: the speeds, the momentum the
/// shift keeps, and the kinetic energy it loses, booked to the gearbox.
/// The same shift written out with reinit gives the same speeds to
/// round-off and loses the same energy at the event; a reinit's loss is
/// the event's, not booked to a part (only a declared engagement books it
/// to the part that engages).
#[test]
fn the_gear_change_in_text_keeps_the_momentum_and_books_the_shift_to_the_gearbox() {
    let pr = load("mech_gear_change").unwrap();
    let (cp, times) = (&pr.checkpoints, &pr.times);
    let e_shift = cp["E_shift"][3];
    let (lib, text) = text_library();
    let engaged = run_model(&build(&lib, &text.components["Examples.GearChange"]));
    let reinit = run_model(&build(&lib, &text.components["Examples.GearChangeReinit"]));

    for (what, r) in [("engagement", &engaged), ("reinit", &reinit)] {
        assert_eq!(r.events.len(), 1, "{what}: {:?}", r.events);
        assert_eq!(r.events[0].t, pr.p("t_shift"), "{what}: the shift is at t_shift exactly");
        let worst = |name: &str, key: &str| {
            times
                .iter()
                .zip(&cp[key])
                .map(|(t, want)| ((at(r, name, *t) - want) / cp[key][3]).abs())
                .fold(0.0f64, f64::max)
        };
        let (w2, w1) = (worst("load.w", "omega2"), worst("motor.w", "omega1"));
        let (m_before, m_after) = momentum_at_shift(r);
        let dm = (m_after - m_before).abs() / m_before;
        let e = r.energy.as_ref().expect("books");
        let gear = e.parts.iter().find(|p| p.path == "gear").expect("the gear's books");
        println!(
            "{what}: omega2 {w2:.1e}, omega1 {w1:.1e} of the exact answer; momentum through \
             the shift kept to {dm:.1e}; lost at the shift {:.10} J (exact {e_shift:.10} J, \
             {:.1e}), booked to the gearbox {:.10} J; books close to {:.1e}",
            e.event_loss,
            (e.event_loss - e_shift).abs() / e_shift,
            gear.impulse_lost,
            e.relative_closure
        );
        assert!(w2 < 1e-12 && w1 < 1e-12, "{what}: {w2:e} {w1:e}");
        assert!(dm < 1e-14, "{what}: the momentum changed by {dm:e}");
        assert!((e.event_loss - e_shift).abs() < 1e-12 * e_shift, "{what}: {}", e.event_loss);
        books_close(r, 1e-12);
        if what == "engagement" {
            assert!((gear.impulse_lost - e_shift).abs() < 1e-12 * e_shift, "{}", gear.impulse_lost);
            assert_eq!(e.impulse_loss, gear.impulse_lost, "all of it the gearbox's");
        } else {
            assert_eq!((gear.impulse_lost, e.impulse_loss), (0.0, 0.0));
        }
    }
    // the two forms against each other, on the whole grid
    let mut worst = 0.0f64;
    for name in ["load.w", "motor.w"] {
        let (a, b) = (engaged.channel(name).unwrap(), reinit.channel(name).unwrap());
        let scale = a.iter().fold(0.0f64, |m, x| m.max(x.abs()));
        for (x, y) in a.iter().zip(b) {
            worst = worst.max((x - y).abs() / scale);
        }
    }
    println!("engagement and reinit differ by at most {worst:.1e} of the largest speed");
    assert!(worst < 1e-14, "{worst:e}");
}
