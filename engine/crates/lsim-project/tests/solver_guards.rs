//! Two guards of the run loop that step 3's hard problems showed missing
//! (`benchmarks/hard`): a state the equations keep at or above zero that a
//! run takes clearly below it is told (Robertson's reaction at a loose
//! absolute tolerance ended at y1 = -5.7e7 and said nothing), and the limit
//! of steps ends a run (the integrators' own limit counts the steps of one
//! call, and the run calls them a step at a time, so a run needing millions
//! of steps went on for hours).

use lsim_ir::component::build::param;
use lsim_ir::runtime::DiscreteBlock;
use lsim_ir::{ComponentDef, PortDecl, PortKind};
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

model RobertsonBalance "Robertson with the third species from the balance (SUNDIALS' idaRoberts)"
  parameter Real s1(unit = "s") = 1;
  Real y1(start = 1, fixed = true);
  Real y2(start = 0, fixed = true);
  Real y3(start = 0);
equation
  s1 * der(y1) = -0.04 * y1 + 1e4 * y2 * y3;
  s1 * der(y2) = 0.04 * y1 - 1e4 * y2 * y3 - 3e7 * y2 * y2;
  y1 + y2 + y3 = 1;
end RobertsonBalance;

model Fed "x's rate is the balance a = 1 - z, which goes below zero (z = t) before x does"
  parameter Real s1(unit = "s") = 1;
  Real x(start = 1, fixed = true);
  Real z(start = 0, fixed = true);
  Real a;
equation
  a = 1 - z;
  s1 * der(x) = a;
  s1 * der(z) = 1;
end Fed;

model Amplified "x's rate is 1e8 times the balance m = a - b, which goes slightly below zero (b = 1e-6 t) and takes x rightly below zero"
  parameter Real s1(unit = "s") = 1;
  Real x(start = 0, fixed = true);
  Real a(start = 0, fixed = true);
  Real b(start = 0, fixed = true);
  Real m;
equation
  m = a - b;
  s1 * der(x) = 1e8 * m;
  s1 * der(a) = 0;
  s1 * der(b) = 1e-6;
end Amplified;

model Falling "a state its equations take below zero"
  Real x(start = 1, fixed = true);
equation
  der(x) = -1;
end Falling;

model Pulled "a's rate is b, which is not negative where it starts at or above zero, but starts below"
  parameter Real s1(unit = "s") = 1;
  Real a(start = 1, fixed = true);
  Real b(start = -1, fixed = true);
equation
  s1 * der(a) = b;
  s1 * der(b) = 0;
end Pulled;

model Redone "u goes below zero at 0.08 (the proof is made then, with the balance m a member), m = 1 - z leaves at 1, x and q (rate m) go below zero rightly at 2.7"
  parameter Real s1(unit = "s") = 1;
  Real u(start = 1, fixed = true);
  Real v(start = 0, fixed = true);
  Real x(start = 1, fixed = true);
  Real q(start = 1, fixed = true);
  Real z(start = 0, fixed = true);
  Real m;
equation
  m = 1 - z;
  s1 * der(u) = 20 * v;
  s1 * der(v) = -20 * u;
  s1 * der(x) = m;
  s1 * der(q) = m;
  s1 * der(z) = 1;
end Redone;

model RobertsonClock "Robertson's reaction beside a clock of its own: q counts the time sin(w time) > c, c falling slowly, a condition of time and a state whose pulses CVODE's long steps span (the run cuts each step back to the pulse's edge)"
  parameter Real s1(unit = "s") = 1;
  parameter Real w(unit = "1/s") = 1.0471975511965976e-8 "2 pi over 6e8 s";
  Real y1(start = 1, fixed = true);
  Real y2(start = 0, fixed = true);
  Real y3(start = 0, fixed = true);
  Real c(start = 1, fixed = true);
  Real q(start = 0, fixed = true);
equation
  s1 * der(y1) = -0.04 * y1 + 1e4 * y2 * y3;
  s1 * der(y2) = 0.04 * y1 - 1e4 * y2 * y3 - 3e7 * y2 * y2;
  s1 * der(y3) = 3e7 * y2 * y2;
  s1 * der(c) = -1e-11;
  s1 * der(q) = if sin(w * time) > c then 1 else 0;
end RobertsonClock;

model RobertsonTick "Robertson's reaction beside a clock: a sampled block whose output sets q's rate and changes only at every few ticks, so the integrator steps past the ticks that change nothing and the run cuts a step back to the tick that does"
  parameter Real s1(unit = "s") = 1;
  External.Toggler tg;
  Real y1(start = 1, fixed = true);
  Real y2(start = 0, fixed = true);
  Real y3(start = 0, fixed = true);
  Real q(start = 0, fixed = true);
equation
  s1 * der(y1) = -0.04 * y1 + 1e4 * y2 * y3;
  s1 * der(y2) = 0.04 * y1 - 1e4 * y2 * y3 - 3e7 * y2 * y2;
  s1 * der(y3) = 3e7 * y2 * y2;
  s1 * der(q) = tg.y;
end RobertsonTick;

model Oscillator
  parameter Real s1(unit = "s") = 1;
  Real x(start = 1, fixed = true);
  Real v(start = 0, fixed = true);
equation
  s1 * der(x) = v;
  s1 * der(v) = -x;
end Oscillator;
"#;

/// A sampled block that holds 0 or 1 and flips it at every `every`th tick.
struct Toggler {
    period: f64,
    every: u64,
    k: u64,
}

impl DiscreteBlock for Toggler {
    fn name(&self) -> &str {
        "tg"
    }
    fn period(&self) -> f64 {
        self.period
    }
    fn init(&mut self, _t: f64, _i: &[f64], o: &mut [f64]) -> Result<(), String> {
        self.k = 0;
        o[0] = 0.0;
        Ok(())
    }
    fn tick(&mut self, _t: f64, _i: &[f64], o: &mut [f64]) -> Result<(), String> {
        self.k += 1;
        o[0] = ((self.k / self.every) % 2) as f64;
        Ok(())
    }
}

fn run(name: &str, opts: &SolverOptions, t_end: f64) -> Result<SimResult, SolveError> {
    run_src(MODELS, name, opts, t_end, None)
}

/// RobertsonClock with its pulses `period` s apart.
fn run_clock(period: f64, opts: &SolverOptions) -> Result<SimResult, SolveError> {
    let w = 2.0 * std::f64::consts::PI / period;
    let src = MODELS.replace("1.0471975511965976e-8", &format!("{w:e}"));
    run_src(&src, "RobertsonClock", opts, 1e11, None)
}

/// `tick`: the period and the `every` of the model's [`Toggler`].
fn run_src(
    src: &str,
    name: &str,
    opts: &SolverOptions,
    t_end: f64,
    tick: Option<(f64, u64)>,
) -> Result<SimResult, SolveError> {
    let mut lib = lsim_lib::library();
    lib.add(ComponentDef {
        name: "External.Toggler".into(),
        ports: vec![PortDecl {
            name: "y".into(),
            kind: PortKind::Output { unit: "1".into() },
            doc: String::new(),
        }],
        params: vec![param("period", "s", tick.map_or(1.0, |t| t.0), "")],
        ..Default::default()
    });
    let parsed = parse_library(src, Some(&lib)).unwrap_or_else(|e| panic!("{}", Report(&e)));
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
    let mut blocks: Vec<Box<dyn DiscreteBlock>> = match tick {
        Some((period, every)) => vec![Box::new(Toggler { period, every, k: 0 })],
        None => vec![],
    };
    lsim_solve::simulate(&code, &info, opts, grid, &mut blocks)
}

fn tol(t: f64) -> SolverOptions {
    SolverOptions { rtol: t, atol: t, ..Default::default() }
}

fn below(r: &SimResult) -> Vec<&String> {
    r.report.warnings.iter().filter(|w| w.contains("went below zero")).collect()
}

/// The lowest value of `name` along the run, as a warning writes it.
fn own_low(r: &SimResult, name: &str) -> String {
    let i = r.names.iter().position(|n| n == name).unwrap();
    let low = r.min[i].iter().copied().fold(f64::INFINITY, f64::min);
    format!("down to {low:.3e}")
}

#[test]
fn a_state_kept_at_or_above_zero_taken_below_it_is_told() {
    // CVODE (upstream too) at rtol = atol = 1e-4: y1 ends near -5e7
    let r = run("Robertson", &tol(1e-4), 1e11).unwrap();
    let y1 = r.names.iter().position(|n| n == "y1").unwrap();
    assert!(r.values[y1].last().unwrap() < &-1.0, "the blow-up this guards: {:?}", r.values[y1]);
    let w = below(&r);
    // (one warning, naming both)
    assert!(w.len() == 1 && w[0].starts_with("y1, y2 went below zero"), "{:?}", r.report.warnings);
    // (the value it gives, for the first of them to go below, is the lowest
    // the run has of it)
    assert!(w[0].contains(&own_low(&r, "y2")), "{w:?} against {}", own_low(&r, "y2"));
    // within the tolerance at 1e-8 (y1 = 2.08e-8 at 1e11), and nothing said
    let r = run("Robertson", &tol(1e-8), 1e11).unwrap();
    assert!((r.values[y1].last().unwrap() - 2.0833e-8).abs() < 1e-8, "{:?}", r.values[y1]);
    assert!(below(&r).is_empty(), "{:?}", r.report.warnings);
    // a state its equations do take below zero is not told
    let r = run("Falling", &tol(1e-6), 3.0).unwrap();
    assert!(below(&r).is_empty(), "{:?}", r.report.warnings);
    // nor one whose rate reads a state that starts below zero (a = 1 - t)
    let r = run("Pulled", &tol(1e-6), 3.0).unwrap();
    assert!(below(&r).is_empty(), "{:?}", r.report.warnings);
}

#[test]
fn a_balance_of_the_states_joins_the_proof_while_it_stays_at_or_above_zero() {
    // Robertson with y3 = 1 - y1 - y2 (no interval over the states keeps
    // it at or above zero): the same blow-up on the default path, told
    for t in [1e-4, 1e-6] {
        let r = run("RobertsonBalance", &tol(t), 1e11).unwrap();
        let y1 = r.names.iter().position(|n| n == "y1").unwrap();
        assert!(r.values[y1].last().unwrap() < &-1.0, "{t}: {:?}", r.values[y1]);
        let w = below(&r);
        assert!(w.len() == 1 && w[0].starts_with("y1"), "{t}: {:?}", r.report.warnings);
    }
    // a balance that goes below zero first (a = 1 - z, z = t) leaves the
    // proof: x = 1 + t - t²/2 then goes below zero rightly, not told
    let r = run("Fed", &tol(1e-6), 4.0).unwrap();
    let x = r.names.iter().position(|n| n == "x").unwrap();
    assert!(r.values[x].last().unwrap() < &-0.5, "{:?}", r.values[x]);
    assert!(below(&r).is_empty(), "{:?}", r.report.warnings);
    // nor one that is slightly below zero, well within its error weights,
    // when a rate multiplies it many times (m = -1e-6 t, x = -50 t²: the
    // ring modulator's y2 in miniature)
    let r = run("Amplified", &tol(1e-6), 1.0).unwrap();
    let x = r.names.iter().position(|n| n == "x").unwrap();
    assert!(r.values[x].last().unwrap() < &-10.0, "{:?}", r.values[x]);
    assert!(below(&r).is_empty(), "{:?}", r.report.warnings);
}

#[test]
fn the_proof_is_made_again_when_a_member_has_left_since() {
    // u (an oscillation, which keeps the steps short) goes below zero first,
    // so the proof is made while the balance m is still a member, and then
    // holds x and q (whose rate is m) at or above zero. m leaves at t = 1; x
    // and q then go below zero rightly, and the proof made with m would tell
    // them falsely: it must be made again without m.
    let r = run("Redone", &tol(1e-6), 4.0).unwrap();
    for n in ["x", "q"] {
        let i = r.names.iter().position(|x| x == n).unwrap();
        assert!(r.values[i].last().unwrap() < &-0.5, "{n}: {:?}", r.values[i]);
    }
    assert!(below(&r).is_empty(), "{:?}", r.report.warnings);
}

#[test]
fn a_step_cut_back_by_an_event_is_judged_at_the_cut_not_at_its_end() {
    // Robertson at 1e-5 stays within its tolerance (y1 never lower than
    // -1.2e-5 against a floor of -2e-5), and the clock's pulses make the
    // run cut steps back. CVODE's step that ended at 1.3344e9 s was cut
    // back to a pulse's edge, where y1 = -2.4e-6; the watch read the state
    // at the step's end instead (-4.9e-5, below the floor) and told, at the
    // cut's time, of a value the run never had.
    let r = run("RobertsonClock", &tol(1e-5), 1e11).unwrap();
    assert!(r.report.pulses_found > 100, "the cuts this guards: {}", r.report.pulses_found);
    let y1 = r.names.iter().position(|n| n == "y1").unwrap();
    let low = r.min[y1].iter().copied().fold(f64::INFINITY, f64::min);
    assert!(low > -2e-5, "y1 stays above its floor along the run: {low:e}");
    assert!(below(&r).is_empty(), "{:?}", r.report.warnings);
}

#[test]
fn the_low_a_cut_step_is_told_with_is_the_runs_own() {
    // RobertsonClock with a pulse every 1.5e9 s at 3e-6: y1 does fall below
    // its floor (-6e-6) on this run, to -6.15e-6 at 2.13e9 s, so the warning
    // is right, and what it gives must be the lowest y1 the run has. The
    // watch once gave -1.6e-5, the end of a step cut back to a pulse, at
    // 1.83e9 s. (As the test above: it depends on CVODE's exact steps; the
    // knob is the pulses' period, 1.5e9 s, with the tolerance.)
    let r = run_clock(1.5e9, &tol(3e-6)).unwrap();
    assert!(r.report.pulses_found > 10, "the cuts this guards: {}", r.report.pulses_found);
    let w = below(&r);
    assert!(w.len() == 1 && w[0].starts_with("y1 went below zero"), "{:?}", r.report.warnings);
    assert!(w[0].contains(&own_low(&r, "y1")), "{w:?} against {}", own_low(&r, "y1"));
}

#[test]
fn a_step_a_tick_cut_back_is_judged_at_the_tick_not_at_its_end() {
    // The block ticks every 75 000 000 s and changes its output (q's rate)
    // at every 8th tick: the integrator steps past the ticks that change
    // nothing, and the run cuts a step back to the tick that does. Robertson
    // at 1e-6 stays within its tolerance (y1 never lower than +2e-8; its
    // floor is -2e-6). The step that ended at 6.35e8 s was cut back to the
    // tick at 5.25e8 s, where y1 = +1.7e-6; the watch read the state at the
    // step's end instead (-8.4e-6, below the floor) and told, at the tick's
    // time, of a value the run never had.
    // (It depends on CVODE's exact steps: if another platform moves them,
    // this passes without reaching the case. The knob is the block's
    // period, 7.5e7 s, with `every` = 8 and the tolerance beside it: re-tune
    // them until a run of the unfixed watch warns here.)
    let r = run_src(MODELS, "RobertsonTick", &tol(1e-6), 1e11, Some((7.5e7, 8))).unwrap();
    assert!(r.report.block_changes > 100, "the cuts this guards: {}", r.report.block_changes);
    let y1 = r.names.iter().position(|n| n == "y1").unwrap();
    let low = r.min[y1].iter().copied().fold(f64::INFINITY, f64::min);
    assert!(low > -2e-6, "y1 stays above its floor along the run: {low:e}");
    assert!(below(&r).is_empty(), "{:?}", r.report.warnings);
}

#[test]
fn a_warning_at_a_tick_cut_is_told_at_the_tick() {
    // The same model with the ticks 3e8 s apart, at 1e-6: Robertson blows up
    // here (y1 to -4.75e7), so the warning is right; the first step that
    // ended below the floor was cut back to the tick at 6e8 s, and the
    // warning gives that tick as its time. (The watch once gave the end of
    // the step, 6.35e8 s, where the run never was.) The knob, if CVODE's
    // steps move: the period, 3e8 s, with `every` = 3.
    let r = run_src(MODELS, "RobertsonTick", &tol(1e-6), 1e11, Some((3e8, 3))).unwrap();
    let w = below(&r);
    assert!(w.len() == 1 && w[0].starts_with("y1, y2 went below zero"), "{:?}", r.report.warnings);
    assert!(w[0].contains("first y1 at t = 600000000.000000 s"), "{w:?}");
    assert!(w[0].contains(&own_low(&r, "y1")), "{w:?} against {}", own_low(&r, "y1"));
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
