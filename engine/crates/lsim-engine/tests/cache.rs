//! The prepared-model cache: a hit is the model built without it, and a
//! build at other parameter values is prepared again, since preparation
//! chooses by them (index reduction picks a pendulum's states by pivoting
//! at its start). Parameters set on a built model, from the cache or not,
//! are checked against that choice: values at whose start it does not
//! work are refused, naming the constraint.

use lsim_engine::lsim_ir::component::VarDecl;
use lsim_engine::lsim_ir::component::build::{eq, param, state, sub, var};
use lsim_engine::lsim_ir::expr::{Builtin, Expr, c, call, der, name as n};
use lsim_engine::lsim_ir::{ComponentDef, Library};
use lsim_engine::lsim_solve::{EventKind, OutputGrid, SolverOptions};
use lsim_engine::{BuildOptions, Engine, Model};

/// A pendulum on a rigid rod of length `L0`, released at rest at `theta0`
/// from the vertical: index 3, its rod's force `F` unknown.
fn pendulum() -> ComponentDef {
    let guess =
        |name: &str, unit: &str, start: Expr| VarDecl { start: Some(start), ..var(name, unit, "") };
    ComponentDef {
        name: "Test.Pendulum".into(),
        params: vec![
            param("m", "kg", 1.0, "mass"),
            param("g", "m/s2", 9.81, "gravity"),
            param("theta0", "1", 0.4, "start angle"),
            param("L0", "m", 1.0, "length"),
        ],
        vars: vec![
            VarDecl {
                start: Some(n("L0") * call(Builtin::Sin, vec![n("theta0")])),
                fixed: true,
                ..var("x", "m", "")
            },
            guess("y", "m", -(n("L0") * call(Builtin::Cos, vec![n("theta0")]))),
            state("vx", "m/s", 0.0, ""),
            guess("vy", "m/s", c(0.0)),
            guess("F", "N", n("m") * n("g")),
        ],
        equations: vec![
            eq(der("x"), n("vx"), "x"),
            eq(der("y"), n("vy"), "y"),
            eq(n("m") * der("vx"), -(n("x") * n("F") / n("L0")), "fx"),
            eq(n("m") * der("vy"), -(n("y") * n("F") / n("L0")) - n("m") * n("g"), "fy"),
            eq(n("x") * n("x") + n("y") * n("y"), n("L0") * n("L0"), "rod"),
        ],
        ..Default::default()
    }
}

fn top(theta0: f64) -> ComponentDef {
    ComponentDef {
        name: "Test.Top".into(),
        components: vec![sub("p", "Test.Pendulum", &[("theta0", c(theta0)), ("L0", c(1.0))])],
        ..Default::default()
    }
}

fn engine() -> Engine {
    let mut lib: Library = lsim_lib::library();
    lib.add(pendulum());
    Engine::new(lib)
}

/// `p.x` at 0.5, 1, 1.5 and 2 s, and the whole run's values.
fn swing(m: &Model) -> (Vec<f64>, Vec<Vec<f64>>) {
    let opts = SolverOptions { rtol: 1e-8, atol: 1e-8, ..Default::default() };
    let run = m.simulate(&opts, OutputGrid { t0: 0.0, t_end: 2.0, dt: 0.01 }).expect("runs");
    let ix = run.names.iter().position(|x| x == "p.x").expect("p.x");
    (
        vec![run.values[ix][50], run.values[ix][100], run.values[ix][150], run.values[ix][200]],
        run.values,
    )
}

#[test]
fn a_build_at_other_values_is_prepared_again_and_chooses_its_own_states() {
    let dir = std::env::temp_dir().join(format!("lsim-cache-values-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let engine = engine();
    let cached = BuildOptions { cache: Some(dir.clone()), ..Default::default() };

    let first = engine.build(&top(0.4), &cached).expect("builds");
    let again = engine.build(&top(0.4), &cached).expect("builds");
    assert!(!first.report.cache_hit && again.report.cache_hit);
    assert_eq!(swing(&first).1, swing(&again).1, "a hit is the model");

    // released horizontally, x cannot be a state (the rod's constraint
    // does not fix y from it there): preparation must choose again
    let level = engine.build(&top(std::f64::consts::FRAC_PI_2), &cached).expect("builds");
    let plain =
        engine.build(&top(std::f64::consts::FRAC_PI_2), &BuildOptions::default()).expect("builds");
    assert!(!level.report.cache_hit, "other values are not a hit");
    assert_eq!(level.info.y_names, plain.info.y_names);
    assert_ne!(level.info.y_names, first.info.y_names, "the states are chosen at the values");
    // y and vy, chosen there, no longer decide x at the bottom of the
    // swing (a quarter period, 0.592 s): both builds choose the states
    // again there, and again as the rod comes level, and go on, the same
    // run (before dynamic state selection both stopped at the bottom,
    // saying so; before the choice was watched, they passed it 7e-6 m off
    // at rtol 1e-8, 700 rtol)
    let opts = SolverOptions { rtol: 1e-8, atol: 1e-8, ..Default::default() };
    let grid = OutputGrid { t0: 0.0, t_end: 2.0, dt: 0.01 };
    let a = level.simulate(&opts, grid).expect("runs");
    let b = plain.simulate(&opts, grid).expect("runs");
    assert_eq!(a.values, b.values, "the cached build's run is the plain build's");
    let switches = a.events.iter().filter(|e| matches!(e.kind, EventKind::States(_))).count();
    assert!(switches >= 2, "{switches} switches");
    // and right: the closed form (Jacobi's elliptic functions, mpmath at
    // 34 digits, as the outside judge's `pendulum_reference`) at 0.5, 1,
    // 1.5 and 2 s, within 100 rtol
    let ix = a.names.iter().position(|v| v == "p.x").expect("p.x");
    for (k, want) in
        [(50, 0.3910487916), (100, -0.9862917511), (150, -0.8841323603), (200, 0.7935661953)]
    {
        let x = a.values[ix][k];
        assert!((x - want).abs() < 1e-6, "x({}) = {x}, the closed form {want}", k as f64 / 100.0);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Built at 0.4 rad the pendulum keeps x and vx as its states. Set to
/// release level (π/2, x = L), the rod's constraint no longer depends on
/// y there: the choice cannot start, and the change is refused, naming the
/// rod, as a broken divisor is (before, it was taken, and the run failed
/// on IDA's flag -4, or, from the cache, returned a pendulum that never
/// falls, the merge review's finding). Values the choice suits are taken,
/// from the cache as without it; a sweep refuses the set alone; a run
/// handed such values directly stops at its start, saying why.
#[test]
fn parameters_at_which_the_chosen_states_cannot_start_are_refused() {
    let dir = std::env::temp_dir().join(format!("lsim-cache-states-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let engine = engine();
    let cached = BuildOptions { cache: Some(dir.clone()), ..Default::default() };
    let first = engine.build(&top(0.4), &cached).expect("builds");
    let hit = engine.build(&top(0.4), &cached).expect("builds");
    assert!(hit.report.cache_hit);
    assert!(!first.prepared.state_sets.is_empty());
    assert_eq!(hit.prepared.state_sets, first.prepared.state_sets, "the hit carries the choice");
    let level = std::f64::consts::FRAC_PI_2;
    for mut m in [first, hit] {
        assert_eq!(m.info.y_names[..2], ["p.x", "p.vx"]);
        let why = m.set_param("p.theta0", level).unwrap_err();
        assert!(
            why.contains("'p' (rod) decides p.y") && why.contains("build the model again"),
            "{why}"
        );
        assert_eq!(m.param("p.theta0"), Some(0.4), "a refused value is not set");
        // at 1.5 rad y moves 14 times as much as x at the start: taken
        m.set_param("p.theta0", 1.5).expect("the choice works there");
        let opts = SolverOptions { rtol: 1e-8, atol: 1e-8, ..Default::default() };
        m.simulate(&opts, OutputGrid { t0: 0.0, t_end: 2.0, dt: 0.01 }).expect("runs");
        let sets: Vec<Vec<(String, f64)>> =
            [0.4, level, 1.0].iter().map(|&v| vec![("p.theta0".to_string(), v)]).collect();
        let runs = m.sweep(&sets, &opts, OutputGrid { t0: 0.0, t_end: 1.0, dt: 0.1 }, 2);
        assert!(runs[0].is_ok() && runs[2].is_ok());
        let e = runs[1].as_ref().map(|_| ()).unwrap_err().to_string();
        assert!(e.contains("decides p.y"), "{e}");
        // past the check: the run itself stops at its start
        let p = {
            let mut p = m.info.params.clone();
            let k = m.param_names().iter().position(|n| *n == "p.theta0").expect("theta0");
            p[k] = level;
            p
        };
        let err = m.simulate_with(&p, &opts, OutputGrid { t0: 0.0, t_end: 1.0, dt: 0.1 }, &mut []);
        let e = err.map(|_| ()).unwrap_err().to_string();
        assert!(e.contains("at the start") && e.contains("decides p.y"), "{e}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Two pendulums in one model, released at `a` and `b` rad: two sets of
/// states, each chosen again where its own pivots degrade.
fn two(a: f64, b: f64) -> ComponentDef {
    ComponentDef {
        name: "Test.Two".into(),
        components: vec![
            sub("p", "Test.Pendulum", &[("theta0", c(a)), ("L0", c(1.0))]),
            sub("q", "Test.Pendulum", &[("theta0", c(b)), ("L0", c(1.0))]),
        ],
        ..Default::default()
    }
}

/// The key the engine computed before dynamic state selection
/// (`cache.rs` at the main branch's 3aaea7e): SHA-256 of the format
/// `prepared-v4`, the engine's version and the inputs. Nothing else in
/// it changed on the branch that added [`StateSet`]'s `kept`, `eqs`,
/// `nodes` and `roots`, so an entry written then hit under it.
///
/// [`StateSet`]: lsim_engine::lsim_ir::StateSet
fn key_before(lib: &Library, top: &ComponentDef) -> String {
    use sha2::{Digest, Sha256};
    let mut used: std::collections::BTreeMap<&str, &ComponentDef> = Default::default();
    let mut todo = vec![top];
    while let Some(d) = todo.pop() {
        for s in &d.components {
            if let Some(def) = lib.components.get(&s.def)
                && used.insert(&def.name, def).is_none()
            {
                todo.push(def);
            }
        }
    }
    let inverse: Option<&lsim_engine::lsim_ir::InverseSpec> = None;
    let text = serde_json::to_string(&(
        "prepared-v4",
        env!("CARGO_PKG_VERSION"),
        top,
        used,
        &lib.connectors,
        false,
        inverse,
    ))
    .expect("the IR serialises");
    Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// A run's values and its switches, or why it stopped.
fn swings(m: &Model) -> Result<(Vec<Vec<f64>>, usize), String> {
    let opts = SolverOptions { rtol: 1e-6, atol: 1e-8, ..Default::default() };
    let run = m
        .simulate(&opts, OutputGrid { t0: 0.0, t_end: 10.0, dt: 0.01 })
        .map_err(|e| e.to_string())?;
    let switches = run.events.iter().filter(|e| matches!(e.kind, EventKind::States(_))).count();
    Ok((run.values, switches))
}

/// The entry a fresh build wrote, as an engine that did not know the
/// fields dynamic state selection added to each set wrote it.
fn without_the_sets_new_fields(text: &str) -> String {
    let mut v: serde_json::Value = serde_json::from_str(text).expect("JSON");
    let sets = v["state_sets"].as_array_mut().expect("the model's sets");
    // two per pendulum: its rod at index reduction's two levels
    assert_eq!(sets.len(), 4);
    for s in sets {
        for f in ["kept", "eqs", "nodes", "roots"] {
            s.as_object_mut().expect("a set").remove(f).expect("the field");
        }
    }
    serde_json::to_string(&v).expect("JSON")
}

/// A cache hit must be the model preparation gives (cache.rs). An entry
/// an engine before dynamic state selection wrote has none of the
/// fields that tell a run which set is which; read with them empty, the
/// run chose both pendulums' states again where only one had degraded,
/// and stopped at 0.2135 s, the first review's finding B1 back (the
/// second review's F1: the key's format was not bumped, and the fields
/// read as empty by default). Such an entry is not used, under the key
/// it was written under or under this engine's: the build prepares the
/// model again, and runs as a fresh build does.
#[test]
fn an_entry_an_older_engine_wrote_is_not_used() {
    let dir = std::env::temp_dir().join(format!("lsim-cache-older-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let engine = engine();
    let top = two(1.50, 1.80);
    let cached = BuildOptions { cache: Some(dir.clone()), ..Default::default() };
    let fresh = engine.build(&top, &cached).expect("builds");
    assert!(!fresh.report.cache_hit);
    let (want, switches) = swings(&fresh).expect("runs to the end");
    assert!(switches >= 30, "{switches} switches");
    let entry = |key: &str| dir.join(format!("{key}.prepared.json"));
    let written = std::fs::read_to_string(entry(&fresh.report.cache_key)).expect("the entry");
    let older = without_the_sets_new_fields(&written);

    // where the older engine left it
    std::fs::remove_file(entry(&fresh.report.cache_key)).expect("removed");
    std::fs::write(entry(&key_before(engine.library(), &top)), &older).expect("written");
    let m = engine.build(&top, &cached).expect("builds");
    assert!(!m.report.cache_hit, "an older engine's entry is not this engine's");
    assert_eq!(swings(&m), Ok((want.clone(), switches)), "the run is a fresh build's");

    // and under this engine's own key: an entry without the fields is
    // not read (they have no default), and is written again whole
    std::fs::write(entry(&fresh.report.cache_key), &older).expect("written");
    let m = engine.build(&top, &cached).expect("builds");
    assert!(!m.report.cache_hit, "an entry without the sets' fields is not used");
    assert_eq!(swings(&m), Ok((want.clone(), switches)));
    let hit = engine.build(&top, &cached).expect("builds");
    assert!(hit.report.cache_hit, "written again whole");
    assert_eq!(hit.prepared, fresh.prepared);
    assert_eq!(swings(&hit), Ok((want, switches)));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A part whose parameter `mode` takes an option of the library's shared
/// enumeration `Speed` (its value: the option's position, from 1), and a
/// variable that reads it.
fn moded() -> ComponentDef {
    use lsim_engine::lsim_ir::{ParamDecl, ParamValue};
    ComponentDef {
        name: "Test.Moded".into(),
        params: vec![ParamDecl {
            default: ParamValue::Enum("Speed.Slow".into()),
            unit: String::new(),
            ..param("mode", "", 0.0, "how fast")
        }],
        vars: vec![var("y", "1", "")],
        equations: vec![eq(n("y"), n("mode"), "y reads the mode")],
        ..Default::default()
    }
}

/// The library with the shared enumeration `Speed`'s options in `order`.
fn speeds(order: [&str; 2]) -> Library {
    use lsim_engine::lsim_ir::{EnumLiteral, EnumType};
    let mut lib: Library = lsim_lib::library();
    lib.add(moded());
    lib.add_type(EnumType {
        name: "Speed".into(),
        literals: order
            .iter()
            .map(|l| EnumLiteral { name: (*l).into(), doc: String::new() })
            .collect(),
        doc: String::new(),
    });
    lib
}

/// The library's shared enumeration types are inputs of preparation (an
/// option's value is its position in its type): a library whose type
/// lists its options in another order is not a hit. Before, the key held
/// the components and connectors the model reaches but not the shared
/// types, and the second build was a hit with the first library's value.
#[test]
fn a_library_with_other_enumeration_types_is_prepared_again() {
    use lsim_engine::lsim_ir::{Modifier, ParamValue};
    let dir = std::env::temp_dir().join(format!("lsim-cache-types-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut part = sub("m", "Test.Moded", &[]);
    part.modifiers
        .push(Modifier { param: "mode".into(), value: ParamValue::Enum("Speed.Fast".into()) });
    let top =
        ComponentDef { name: "Test.Top".into(), components: vec![part], ..Default::default() };
    let cached = BuildOptions { cache: Some(dir.clone()), ..Default::default() };
    let first = Engine::new(speeds(["Slow", "Fast"])).build(&top, &cached).expect("builds");
    assert_eq!(first.param("m.mode"), Some(2.0));
    let other = Engine::new(speeds(["Fast", "Slow"])).build(&top, &cached).expect("builds");
    assert!(!other.report.cache_hit, "another library's types are not a hit");
    assert_eq!(other.param("m.mode"), Some(1.0), "the option's place in this library's type");
    let again = Engine::new(speeds(["Slow", "Fast"])).build(&top, &cached).expect("builds");
    assert!(again.report.cache_hit && again.param("m.mode") == Some(2.0));
    let _ = std::fs::remove_dir_all(&dir);
}
