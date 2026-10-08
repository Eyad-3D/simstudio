//! The exact-answer suite (DESIGN.md, *Testing strategy*): the reference
//! problems of `benchmarks/reference/problems` that the Stage 1 library
//! can express, built from its components, solved on every backend, and
//! compared with the problems' checkpoints (the exact answer at four times
//! and the exact event times) — first against the suite's own tolerances
//! (`benchmarks/targets.toml`), then for convergence: at rtol 1e-6, 1e-8
//! and 1e-10 every signal within 100·rtol of its scale, every event within
//! 10·rtol of its exact time, the energy books closing to 1e-6.

mod common;

use common::{Problem, build, library, targets};
use lsim_ir::ComponentDef;
use lsim_ir::component::build::{connect, sub};
use lsim_ir::expr::c;
use lsim_solve::{Backend, OutputGrid, SimResult, SolverOptions, simulate};

/// How a problem's quantity is read from a run.
#[derive(Clone, Copy)]
enum Read {
    /// a channel's value
    Channel(&'static str),
    /// energy delivered by a part (minus its energy in)
    Delivered(&'static str),
    /// energy lost in a part
    Lost(&'static str),
    /// change of a part's stored energy
    Stored(&'static str),
    /// zero (a term the model has no part for)
    Zero,
}

struct Case {
    problem: Problem,
    top: ComponentDef,
    reads: Vec<(&'static str, Read)>,
}

fn level(p: &Problem, name: &str) -> f64 {
    p.compare.iter().find(|c| c.name == name).and_then(|c| c.level).expect("an event level")
}

fn named(mut s: lsim_ir::SubDecl, label: &str) -> lsim_ir::SubDecl {
    s.label = Some(label.into());
    s
}

fn rc_step() -> Case {
    let p = Problem::load("elec_rc_step");
    assert_eq!(p.initial["v_C"], 0.0, "the library capacitor starts at 0 V");
    let top = ComponentDef {
        name: "Ref.RcStep".into(),
        components: vec![
            named(sub("src", "Electrical.ConstantVoltage", &[("V", c(p.p("V")))]), "Source"),
            named(sub("r", "Electrical.Resistor", &[("R", c(p.p("R")))]), "Precharge resistor"),
            named(sub("c", "Electrical.Capacitor", &[("C", c(p.p("C")))]), "DC link"),
            named(
                sub("sensor", "Test.VoltageLevel", &[("level", c(level(&p, "t_event")))]),
                "Precharge done",
            ),
            sub("gnd", "Electrical.Ground", &[]),
        ],
        connections: vec![
            connect("src.p", "r.p"),
            connect("r.n", "c.p"),
            connect("c.n", "src.n"),
            connect("src.n", "gnd.p"),
            connect("sensor.p", "c.p"),
            connect("sensor.n", "c.n"),
        ],
        ..Default::default()
    };
    Case {
        problem: p,
        top,
        reads: vec![
            ("v_C", Read::Channel("c.v")),
            ("i", Read::Channel("r.i")),
            ("E_source", Read::Delivered("src")),
            ("E_R", Read::Lost("r")),
            ("E_C", Read::Stored("c")),
        ],
    }
}

fn rl_step() -> Case {
    let p = Problem::load("elec_rl_step");
    assert_eq!(p.initial["i"], 0.0, "the library inductor starts at 0 A");
    let top = ComponentDef {
        name: "Ref.RlStep".into(),
        components: vec![
            named(sub("src", "Electrical.ConstantVoltage", &[("V", c(p.p("V")))]), "Supply"),
            named(sub("r", "Electrical.Resistor", &[("R", c(p.p("R")))]), "Coil resistance"),
            named(sub("l", "Electrical.Inductor", &[("L", c(p.p("L")))]), "Coil"),
            named(
                sub("sensor", "Test.CurrentLevel", &[("level", c(level(&p, "t_event")))]),
                "Pull-in",
            ),
            sub("gnd", "Electrical.Ground", &[]),
        ],
        connections: vec![
            connect("src.p", "sensor.p"),
            connect("sensor.n", "r.p"),
            connect("r.n", "l.p"),
            connect("l.n", "src.n"),
            connect("src.n", "gnd.p"),
        ],
        ..Default::default()
    };
    Case {
        problem: p,
        top,
        reads: vec![
            ("i", Read::Channel("l.i")),
            ("v_L", Read::Channel("l.v")),
            ("E_source", Read::Delivered("src")),
            ("E_R", Read::Lost("r")),
            ("E_L", Read::Stored("l")),
        ],
    }
}

fn motor(id: &str, with_l: bool) -> Case {
    let p = Problem::load(id);
    assert_eq!(p.p("T_load"), 0.0);
    assert_eq!((p.initial["omega"], p.initial["i"]), (0.0, 0.0));
    let mut components = vec![
        named(sub("src", "Electrical.ConstantVoltage", &[("V", c(p.p("V")))]), "Supply"),
        named(sub("r", "Electrical.Resistor", &[("R", c(p.p("R")))]), "Armature resistance"),
        named(sub("emf", "Electrical.Emf", &[("k", c(p.p("k")))]), "Motor"),
        named(sub("inertia", "Rotational.Inertia", &[("J", c(p.p("J")))]), "Rotor"),
        named(sub("damper", "Rotational.Damper", &[("d", c(p.p("b")))]), "Bearing friction"),
        named(sub("sensor", "Test.SpeedLevel", &[("level", c(level(&p, "t_event")))]), "Speed"),
        sub("gnd", "Electrical.Ground", &[]),
    ];
    let mut connections = vec![
        connect("emf.n", "src.n"),
        connect("src.n", "gnd.p"),
        connect("src.p", "r.p"),
        connect("emf.flange", "inertia.a"),
        connect("inertia.b", "damper.flange"),
        connect("inertia.b", "sensor.flange"),
    ];
    let mut reads = vec![
        ("omega", Read::Channel("inertia.w")),
        ("i", Read::Channel("emf.i")),
        ("E_in", Read::Delivered("src")),
        ("E_R", Read::Lost("r")),
        ("E_kin", Read::Stored("inertia")),
        ("E_friction", Read::Lost("damper")),
        ("E_load", Read::Zero),
    ];
    if with_l {
        components.push(named(
            sub("l", "Electrical.Inductor", &[("L", c(p.p("L")))]),
            "Armature inductance",
        ));
        connections.push(connect("r.n", "l.p"));
        connections.push(connect("l.n", "emf.p"));
        reads.push(("E_L", Read::Stored("l")));
    } else {
        connections.push(connect("r.n", "emf.p"));
    }
    Case {
        problem: p,
        top: ComponentDef {
            name: format!("Ref.{id}"),
            components,
            connections,
            ..Default::default()
        },
        reads,
    }
}

fn cases() -> Vec<Case> {
    vec![rc_step(), rl_step(), motor("motor_dc_spinup", true), motor("motor_dc_spinup_l0", false)]
}

/// A quantity's values at the checkpoint times.
fn read(run: &SimResult, p: &Problem, r: Read) -> Vec<f64> {
    let idx: Vec<usize> = p
        .times
        .iter()
        .map(|tc| {
            run.times
                .iter()
                .position(|t| (t - tc).abs() <= 1e-9 * tc.abs().max(1.0))
                .unwrap_or_else(|| panic!("{}: no output at t = {tc}", p.id))
        })
        .collect();
    let books = || run.energy.as_ref().expect("energy books");
    let part = |path: &str| {
        books()
            .parts
            .iter()
            .find(|b| b.path == path)
            .unwrap_or_else(|| panic!("{}: no books for {path}", p.id))
    };
    match r {
        Read::Channel(ch) => {
            let v = run.channel(ch).unwrap_or_else(|| panic!("no channel {ch}"));
            idx.iter().map(|&k| v[k]).collect()
        }
        Read::Delivered(path) => idx.iter().map(|&k| -part(path).energy_in_t[k]).collect(),
        Read::Lost(path) => idx.iter().map(|&k| part(path).lost_t[k]).collect(),
        Read::Stored(path) => {
            let b = part(path);
            idx.iter().map(|&k| b.stored_t[k] - b.stored_t[0]).collect()
        }
        Read::Zero => vec![0.0; idx.len()],
    }
}

/// Errors of one run: (worst signal error / scale, event error s, worst
/// energy error / energy scale, the books' closure).
struct Errors {
    signal: f64,
    signal_name: String,
    event: f64,
    event_exact: f64,
    energy: f64,
    energy_name: String,
    closure: f64,
}

fn errors(case: &Case, run: &SimResult) -> Errors {
    let p = &case.problem;
    let mut e = Errors {
        signal: 0.0,
        signal_name: String::new(),
        event: f64::NAN,
        event_exact: f64::NAN,
        energy: 0.0,
        energy_name: String::new(),
        closure: run.energy.as_ref().map(|b| b.relative_closure).unwrap_or(f64::NAN),
    };
    // the energy scale: the largest exact term at the end
    let energy_scale = p
        .compare
        .iter()
        .filter(|c| c.kind == "energy")
        .filter_map(|c| p.exact.get(&c.name).and_then(|v| v.last()))
        .fold(0.0f64, |a, b| a.max(b.abs()));
    for cmp in &p.compare {
        match cmp.kind.as_str() {
            "signal" | "energy" => {
                let r = case
                    .reads
                    .iter()
                    .find(|(n, _)| *n == cmp.name)
                    .unwrap_or_else(|| panic!("{}: no reading for {}", p.id, cmp.name))
                    .1;
                let got = read(run, p, r);
                let want = &p.exact[&cmp.name];
                if cmp.kind == "signal" {
                    let scale = cmp
                        .scale
                        .unwrap_or_else(|| want.iter().fold(0.0f64, |a, b| a.max(b.abs())));
                    for (g, w) in got.iter().zip(want) {
                        let err = (g - w).abs() / scale;
                        if err > e.signal {
                            e.signal = err;
                            e.signal_name = cmp.name.clone();
                        }
                    }
                } else {
                    // energy terms at every checkpoint (the suite checks the end)
                    for (g, w) in got.iter().zip(want) {
                        let err = (g - w).abs() / energy_scale;
                        if err > e.energy {
                            e.energy = err;
                            e.energy_name = cmp.name.clone();
                        }
                    }
                }
            }
            "event" => {
                let exact = p.events[&cmp.name];
                let t = run
                    .events
                    .iter()
                    .find(|ev| ev.label.contains("reaches its level"))
                    .unwrap_or_else(|| {
                        panic!("{}: the event was not found: {:?}", p.id, run.events)
                    })
                    .t;
                e.event = (t - exact).abs();
                e.event_exact = exact;
            }
            other => panic!("unknown comparison kind {other}"),
        }
    }
    e
}

fn run(case: &Case, built: &common::Built, backend: Backend, rtol: f64) -> SimResult {
    let p = &case.problem;
    let opts = SolverOptions { rtol, atol: rtol * 1e-2, backend, ..Default::default() };
    let grid = OutputGrid { t0: 0.0, t_end: p.t_end, dt: p.output_dt };
    simulate(&built.jit, &built.info, &opts, grid, &mut [])
        .unwrap_or_else(|e| panic!("{} on {backend:?} at rtol {rtol:e}: {e}", p.id))
}

fn backends() -> Vec<Backend> {
    let mut b = vec![Backend::Sundials];
    if cfg!(feature = "diffsol") {
        b.push(Backend::Diffsol);
    }
    b
}

#[test]
fn the_reference_problems_pass_the_suite_tolerances() {
    let tg = targets();
    let lib = library();
    for case in cases() {
        let p = &case.problem;
        for force_implicit in [false, true] {
            let built = build(&lib, &case.top, force_implicit);
            for backend in backends() {
                let r = run(&case, &built, backend, 1e-8);
                let e = errors(&case, &r);
                let path = if force_implicit { "DAE" } else { "ODE" };
                println!(
                    "{:<22} {path} {:<24} signal {:.1e} ({}), event {:.1e} s, energy {:.1e}, closure {:.1e}, {} steps",
                    p.id,
                    r.backend,
                    e.signal,
                    e.signal_name,
                    e.event,
                    e.energy,
                    e.closure,
                    r.stats.steps
                );
                assert!(e.signal <= tg.signal_rtol, "{}: signal error {:e}", p.id, e.signal);
                assert!(
                    e.event <= tg.event_atol_s + tg.event_rtol * e.event_exact,
                    "{}: event off by {:e} s",
                    p.id,
                    e.event
                );
                assert!(e.energy <= tg.energy_rtol, "{}: energy error {:e}", p.id, e.energy);
                assert!(e.closure <= tg.closure_rtol, "{}: closure {:e}", p.id, e.closure);
            }
        }
    }
}

#[test]
fn errors_shrink_with_the_tolerance_and_events_stay_within_ten_rtol() {
    let lib = library();
    for case in cases() {
        let p = &case.problem;
        let built = build(&lib, &case.top, false);
        for backend in backends() {
            for rtol in [1e-6, 1e-8, 1e-10] {
                let r = run(&case, &built, backend, rtol);
                let e = errors(&case, &r);
                println!(
                    "{:<22} {:<24} rtol {rtol:.0e}: signal {:.1e} ({}), event {:.1e} s ({:.1} rtol), energy {:.1e} ({}), closure {:.1e}, {} steps, {:.2} ms",
                    p.id,
                    r.backend,
                    e.signal,
                    e.signal_name,
                    e.event,
                    e.event / (rtol * e.event_exact),
                    e.energy,
                    e.energy_name,
                    e.closure,
                    r.stats.steps,
                    r.wall_seconds * 1e3
                );
                assert!(
                    e.signal <= 100.0 * rtol,
                    "{}: signal error {:e} at rtol {rtol:e}",
                    p.id,
                    e.signal
                );
                assert!(
                    e.event <= 10.0 * rtol * e.event_exact,
                    "{}: event off by {:e} s at rtol {rtol:e} (exact {} s)",
                    p.id,
                    e.event,
                    e.event_exact
                );
                assert!(
                    e.energy <= 100.0 * rtol,
                    "{}: energy error {:e} at rtol {rtol:e}",
                    p.id,
                    e.energy
                );
                assert!(e.closure <= 1e-6, "{}: closure {:e} at rtol {rtol:e}", p.id, e.closure);
            }
        }
    }
}

#[test]
fn the_backends_agree_within_tolerance() {
    if !cfg!(feature = "diffsol") {
        return;
    }
    let lib = library();
    for case in cases() {
        let p = &case.problem;
        for force_implicit in [false, true] {
            let built = build(&lib, &case.top, force_implicit);
            for rtol in [1e-6, 1e-9] {
                let a = run(&case, &built, Backend::Sundials, rtol);
                let b = run(&case, &built, Backend::Diffsol, rtol);
                let mut worst = (0.0f64, String::new());
                for (i, name) in a.names.iter().enumerate() {
                    let scale = a.values[i].iter().fold(0.0f64, |m, x| m.max(x.abs()));
                    if scale == 0.0 {
                        continue;
                    }
                    for k in 0..a.times.len() {
                        let d = (a.values[i][k] - b.values[i][k]).abs() / scale;
                        if d > worst.0 {
                            worst = (d, name.clone());
                        }
                    }
                }
                let ev = (a.events[0].t - b.events[0].t).abs();
                println!(
                    "{:<22} {} rtol {rtol:.0e}: backends differ by {:.1e} ({}), events by {ev:.1e} s",
                    p.id,
                    if force_implicit { "DAE" } else { "ODE" },
                    worst.0,
                    worst.1
                );
                assert!(worst.0 <= 100.0 * rtol, "{}: {} differs by {:e}", p.id, worst.1, worst.0);
                assert!(ev <= 10.0 * rtol * a.events[0].t);
            }
        }
    }
}
