//! The hand-written test car in both modes: fast mode's flags against the
//! full dynamic run's limit hits, and the two modes' answers on a cycle the
//! car can follow.

use lsim_engine::testcar::{self, car, hard_cycle, inverse_spec, library, speed_trace};
use lsim_engine::{BuildOptions, Engine, Model};
use lsim_fast::{FastOptions, LimitFlag, Trace};
use lsim_solve::{OutputGrid, SimResult, SolverOptions};

fn models(t: &[f64], v: &[f64]) -> (Model, Model) {
    let engine = Engine::new(library(speed_trace("TestCar.Trace", t, v)));
    let top = car("TestCar.Trace");
    let show = |d: Vec<lsim_ir::Diagnostic>| {
        d.iter().map(|x| x.to_string()).collect::<Vec<_>>().join("\n")
    };
    let inv = engine
        .build_inverse(&top, &inverse_spec(), &BuildOptions::default())
        .unwrap_or_else(|d| panic!("{}", show(d)));
    let fwd =
        engine.build(&top, &BuildOptions::default()).unwrap_or_else(|d| panic!("{}", show(d)));
    (inv, fwd)
}

fn forward(fwd: &Model, t_end: f64, dt: f64) -> SimResult {
    fwd.simulate(
        &SolverOptions { rtol: 1e-6, atol: 1e-6, ..Default::default() },
        OutputGrid { t0: 0.0, t_end, dt },
    )
    .unwrap()
}

fn overlaps(a: &LimitFlag, b: &LimitFlag) -> bool {
    a.t_start <= b.t_end && b.t_start <= a.t_end
}

fn show(flags: &[LimitFlag]) -> String {
    flags
        .iter()
        .map(|f| format!("{:.2}..{:.2} {}", f.t_start, f.t_end, f.label))
        .collect::<Vec<_>>()
        .join("\n")
}

/// On a cycle past every limit of the car, each limit the full dynamic run
/// hits is flagged by fast mode at the same time, on the same side; and
/// each flag fast mode raises falls where the full dynamic run hits some
/// limit (a limit it clamps first can keep the ones after it from being
/// reached: the motor's torque clamped, the tyres' grip is not exceeded).
#[test]
fn fast_modes_flags_coincide_with_the_forward_runs_limit_hits() {
    let (t, v) = hard_cycle();
    let (inv, fwd) = models(&t, &v);
    assert_eq!(inv.report.limits, 3, "the grip, the motor's torque and the brakes' capacity");
    assert_eq!(fwd.report.limits, 0, "a forward model clamps");
    let traces = [Trace::new(t.clone(), v.clone()).unwrap()];
    let fast = inv.fast(&traces, &FastOptions::default()).unwrap();
    let run = forward(&fwd, 50.0, 0.05);
    let hits = fwd.limit_hits(&inv, &run, 1e-9).unwrap();
    let why = format!("flags:\n{}\nhits:\n{}", show(&fast.flags), show(&hits));
    assert!(!hits.is_empty() && !fast.flags.is_empty(), "{why}");
    for h in &hits {
        assert!(
            fast.flags.iter().any(|f| f.label == h.label && overlaps(f, h)),
            "a hit without its flag: {} ({:.2}..{:.2})\n{why}",
            h.label,
            h.t_start,
            h.t_end
        );
    }
    for f in &fast.flags {
        assert!(
            hits.iter().any(|h| overlaps(f, h)),
            "a flag while the forward run hit nothing: {}\n{why}",
            f.label
        );
    }
    // the gentle stop at the end stays within every limit
    assert!(fast.flags.iter().all(|f| f.t_end <= 40.0), "{why}");
    assert!(hits.iter().all(|f| f.t_start <= 40.0), "{why}");
}

/// On the WLTC, which the car follows within every limit, neither mode
/// flags anything and the battery's state of charge at the end agrees
/// within the quasi-static method's own error (the forward driver follows
/// the trace closely but not exactly).
#[test]
fn both_modes_agree_on_a_cycle_the_car_can_follow() {
    let (t, v) = testcar::wltc();
    let (inv, fwd) = models(&t, &v);
    let traces = [Trace::new(t.clone(), v.clone()).unwrap()];
    let fast = inv.fast(&traces, &FastOptions::default()).unwrap();
    assert!(fast.flags.is_empty(), "{}", show(&fast.flags));
    let run = forward(&fwd, 1800.0, 1.0);
    assert!(fwd.limit_hits(&inv, &run, 1e-9).unwrap().is_empty());
    let soc_fast = *fast.channel("battery.soc").unwrap().last().unwrap();
    let soc_full = *run.channel("battery.soc").unwrap().last().unwrap();
    let used_fast = 0.9 - soc_fast;
    let used_full = 0.9 - soc_full;
    assert!(used_full > 0.03, "the cycle uses charge: {used_full}");
    assert!(
        (used_fast - used_full).abs() < 0.01 * used_full,
        "charge used: fast {used_fast}, full dynamic {used_full}"
    );
}

/// A parameter change reaches both modes without rebuilding, and a sweep
/// gives each set's own answer.
#[test]
fn parameters_change_without_rebuilding() {
    let (t, v) = hard_cycle();
    let (mut inv, fwd) = models(&t, &v);
    let traces = [Trace::new(t.clone(), v.clone()).unwrap()];
    let soc = |m: &Model| {
        *m.fast(&traces, &FastOptions::default())
            .unwrap()
            .channel("battery.soc")
            .unwrap()
            .last()
            .unwrap()
    };
    let before = soc(&inv);
    let name = "battery.soc0";
    let soc0 = inv.param(name).expect("the battery's initial state of charge");
    inv.set_param(name, soc0 - 0.1).unwrap();
    let after = soc(&inv);
    assert!(((before - after) - 0.1).abs() < 2e-3, "{before} {after}");
    let sets = vec![vec![(name.to_string(), 0.5)], vec![(name.to_string(), 0.7)]];
    let runs = fwd.sweep(
        &sets,
        &SolverOptions::default(),
        OutputGrid { t0: 0.0, t_end: 10.0, dt: 1.0 },
        2,
    );
    let first = |r: &SimResult| r.channel("battery.soc").unwrap()[0];
    assert_eq!(first(runs[0].as_ref().unwrap()), 0.5);
    assert_eq!(first(runs[1].as_ref().unwrap()), 0.7);
    assert!(fwd.params_with(&[("no.such".into(), 1.0)]).is_err());
}

/// A car with a sampled block (a Script part here) runs it only with its
/// host: fast mode without one is refused, as a full dynamic run is,
/// instead of holding the block's outputs at their start values.
#[test]
fn fast_mode_without_the_hosts_of_the_cars_sampled_blocks_is_refused() {
    let (t, v) = testcar::wltc();
    let mut lib = library(speed_trace("TestCar.Trace", &t, &v));
    for d in lsim_lib::blocks::signals::defaults() {
        if d.name == "External.Script" {
            lib.add(d);
        }
    }
    let engine = Engine::new(lib);
    let mut top = car("TestCar.Trace");
    top.components.push(lsim_ir::component::build::sub("controller", "External.Script", &[]));
    let inv = engine.build_inverse(&top, &inverse_spec(), &BuildOptions::default()).unwrap();
    assert_eq!(inv.info.blocks.len(), 1);
    let traces = [Trace::new(t.clone(), v.clone()).unwrap()];
    let e = inv.fast(&traces, &FastOptions::default()).unwrap_err().to_string();
    assert!(e.contains("1 sampled block") && e.contains("0 given"), "{e}");
    let sets = vec![vec![]];
    let swept = inv.fast_sweep(&sets, &traces, &FastOptions::default(), None, 1);
    assert!(swept[0].as_ref().is_err_and(|e| e.to_string().contains("sampled block")));
}

/// Fast mode's inverse model is given no choice of states to switch to
/// (dynamic state selection is the full dynamic run's: the inverse model
/// follows its prescribed motion), and the run loop never runs it: a full
/// dynamic run of it is refused, naming the mode; fast mode steps it with
/// its own Newton solver, which names the variables a singular Jacobian
/// leaves free (DESIGN.md §10).
#[test]
fn fast_mode_s_inverse_model_has_no_choice_of_states_and_is_not_run_by_the_run_loop() {
    let (t, v) = hard_cycle();
    let (inv, _) = models(&t, &v);
    assert!(inv.info.choices.is_none());
    let opts = SolverOptions::default();
    let e = inv.simulate(&opts, OutputGrid { t0: 0.0, t_end: 1.0, dt: 0.1 }).unwrap_err();
    assert!(e.to_string().contains("fast mode's inverse model"), "{e}");
}
