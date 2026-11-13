//! Two guards of the run loop that step 3's hard problems showed missing
//! (`benchmarks/hard`): a state the equations keep at or above zero that a
//! run takes clearly below it is told (Robertson's reaction at a loose
//! absolute tolerance ended at y1 = -5.7e7 and said nothing), and the limit
//! of steps ends a run (the integrators' own limit counts the steps of one
//! call, and the run calls them a step at a time, so a run needing millions
//! of steps went on for hours).

use lsim_lang::{Report, parse_library};
use lsim_solve::{OutputGrid, RunInfo, SimResult, SolveError, SolverOptions};

const MODELS: &str = r#"
model Robertson "Robertson's reaction (the IVP Test Set's rober)"
  parameter Real s1(unit = "s") = 1 "the time unit: the variables are dimensionless";
  Real y1(start = 1, fixed = true);
  Real y2(start = 0, fixed = true);
  Real y3(start = 0, fixed = true);
equation
  s1 * der(y1) = -0.04 * y1 + 1e4 * y2 * y3;
  s1 * der(y2) = 0.04 * y1 - 1e4 * y2 * y3 - 3e7 * y2 * y2;
  s1 * der(y3) = 3e7 * y2 * y2;
end Robertson;

model Falling "a state its equations take below zero"
  Real x(start = 1, fixed = true);
equation
  der(x) = -1;
end Falling;

model Oscillator
  parameter Real s1(unit = "s") = 1;
  Real x(start = 1, fixed = true);
  Real v(start = 0, fixed = true);
equation
  s1 * der(x) = v;
  s1 * der(v) = -x;
end Oscillator;
"#;

fn run(name: &str, opts: &SolverOptions, t_end: f64) -> Result<SimResult, SolveError> {
    let mut lib = lsim_lib::library();
    let parsed = parse_library(MODELS, Some(&lib)).unwrap_or_else(|e| panic!("{}", Report(&e)));
    lib.types.extend(parsed.types);
    for def in parsed.components.into_values() {
        lib.add(def);
    }
    let top = lib.components.get(name).cloned().expect("the model");
    let m =
        lsim_prep::prepare(&lib, &top, &Default::default()).unwrap_or_else(|d| panic!("{d:#?}"));
    let code = lsim_codegen::compile(&m, &Default::default()).expect("compiles");
    let info = RunInfo::from_prepared(&m);
    let grid = OutputGrid { t0: 0.0, t_end, dt: t_end / 10.0 };
    lsim_solve::simulate(&code, &info, opts, grid, &mut [])
}

fn tol(t: f64) -> SolverOptions {
    SolverOptions { rtol: t, atol: t, ..Default::default() }
}

fn below(r: &SimResult) -> Vec<&String> {
    r.report.warnings.iter().filter(|w| w.contains("went below zero")).collect()
}

#[test]
fn a_state_kept_at_or_above_zero_taken_below_it_is_told() {
    // CVODE (upstream too) at rtol = atol = 1e-4: y1 ends near -5e7
    let r = run("Robertson", &tol(1e-4), 1e11).unwrap();
    let y1 = r.names.iter().position(|n| n == "y1").unwrap();
    assert!(r.values[y1].last().unwrap() < &-1.0, "the blow-up this guards: {:?}", r.values[y1]);
    let w = below(&r);
    assert!(w.iter().any(|w| w.starts_with("y1 went below zero")), "{:?}", r.report.warnings);
    // within the tolerance at 1e-8 (y1 = 2.08e-8 at 1e11), and nothing said
    let r = run("Robertson", &tol(1e-8), 1e11).unwrap();
    assert!((r.values[y1].last().unwrap() - 2.0833e-8).abs() < 1e-8, "{:?}", r.values[y1]);
    assert!(below(&r).is_empty(), "{:?}", r.report.warnings);
    // a state its equations do take below zero is not told
    let r = run("Falling", &tol(1e-6), 3.0).unwrap();
    assert!(below(&r).is_empty(), "{:?}", r.report.warnings);
}

#[test]
fn the_limit_of_steps_ends_a_run() {
    let opts = SolverOptions { max_steps: 50, ..tol(1e-8) };
    match run("Oscillator", &opts, 100.0) {
        Err(SolveError::Integrator { message, .. }) => {
            assert!(message.contains("took 50 steps"), "{message}")
        }
        other => panic!("{:?}", other.map(|r| r.stats)),
    }
    // the same run with room for its steps ends
    assert!(run("Oscillator", &tol(1e-8), 100.0).is_ok());
}
