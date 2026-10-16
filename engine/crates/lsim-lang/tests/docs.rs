//! The examples of the text format's documentation (`engine/docs/
//! text-format.md`) work as written: each one parses, checked against the
//! library (also with Windows line ends), and prints back; a component the
//! page presents as the library's is the library's; every model to try
//! (`Examples.…`) prepares, runs and gives the numbers the page states,
//! and together they use every component and type the page defines. The
//! page's Rust example is compiled and run as a doctest (`src/lib.rs`).

use lsim_ir::{ComponentDef, Library, PortKind};
use lsim_lang::{Report, basemodelica, parse_library, to_text};
use lsim_solve::{OutputGrid, RunInfo, SimResult, SolverOptions};
use std::collections::BTreeSet;

/// What the page says a model gives: (model, run to, output step,
/// [(channel, at time, value, relative tolerance)]).
type Expect = (&'static str, f64, f64, &'static [(&'static str, f64, f64, f64)]);

const EXPECT: &[Expect] = &[
    (
        "Examples.ResistorCircuit",
        1.0,
        0.1,
        &[("r.i", 0.0, 3.0, 1e-12), ("r.i", 1.0, 1.103638323514327, 1e-8)],
    ),
    ("Examples.SpinUp", 2.0, 0.5, &[("rotor.w", 2.0, 8.0, 1e-9)]),
    ("Examples.CellOnLoad", 1.0, 0.1, &[("load.i", 1.0, 40.0, 1e-9)]),
    (
        "Examples.Selection",
        1.0,
        0.5,
        &[("selector.ratio", 1.0, 2.8, 1e-15), ("sum.y_out", 1.0, 2.8, 1e-9)],
    ),
    (
        "Examples.BrakeTest",
        8.0,
        0.5,
        &[("rotor.w", 5.5, 275.0, 1e-9), ("rotor.w", 8.0, 200.0, 1e-7)],
    ),
    (
        "Examples.DiodeCircuit",
        1.0,
        0.5,
        // 5 V / (1000 + 1e-5) ohm, falling with RC = (1000 + 1e-5) * 1e-3 s
        &[("r.i", 0.0, 0.00499999995, 1e-9), ("r.i", 1.0, 0.0018393972058572115, 1e-7)],
    ),
    (
        "Examples.CellDischarge",
        10.0,
        1.0,
        &[("load.i", 0.0, 2.0, 1e-9), ("cell.soc", 10.0, 0.9 - 20.0 / 180000.0, 1e-7)],
    ),
    (
        "Examples.GearChange",
        8.0,
        0.01,
        &[
            ("load.w", 4.0, 68.37008624540108, 1e-9),
            ("load.w", 8.0, 109.11217427741272, 1e-9),
            ("motor.w", 8.0, 763.7852199418892, 1e-9),
        ],
    ),
    (
        "Examples.GearChangeReinit",
        8.0,
        0.01,
        &[("load.w", 4.0, 68.37008624540108, 1e-9), ("load.w", 8.0, 109.11217427741272, 1e-9)],
    ),
    (
        "Examples.CouplingEngages",
        2.0,
        0.5,
        &[
            ("flywheel.w", 0.5, 100.0, 1e-9),
            ("flywheel.w", 2.0, 25.0, 1e-6),
            ("load.w", 2.0, 25.0, 1e-6),
        ],
    ),
    ("Examples.ModeHeater", 10.0, 1.0, &[("mass.T", 10.0, 298.15, 1e-9)]),
    ("RC", 0.2, 0.05, &[("vC", 0.1, 345.8658867053549, 1e-6)]),
    ("Examples.Divider", 1.0, 0.5, &[("r1.i", 0.0, 2.0, 1e-15), ("r2.v", 1.0, 4.0, 1e-15)]),
];

/// The page, with Windows line ends made Unix ones.
fn page() -> String {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/text-format.md");
    std::fs::read_to_string(path).expect("the documentation is there").replace("\r\n", "\n")
}

/// The page's code blocks of one kind (```modelica, ```basemodelica).
fn blocks(doc: &str, kind: &str) -> Vec<String> {
    let fence = format!("```{kind}\n");
    let mut out = vec![];
    let mut rest = doc;
    while let Some(start) = rest.find(&fence) {
        let body = &rest[start + fence.len()..];
        let end = body.find("```").expect("a closed block");
        out.push(body[..end].to_string());
        rest = &body[end + 3..];
    }
    out
}

fn run(lib: &Library, def: &ComponentDef, t_end: f64, dt: f64) -> SimResult {
    let m = lsim_prep::prepare(lib, def, &Default::default())
        .unwrap_or_else(|d| panic!("{}: {d:#?}", def.name));
    let jit = lsim_codegen::compile(&m, &Default::default()).expect("compiles");
    let info = RunInfo::from_prepared(&m);
    let opts = SolverOptions { rtol: 1e-10, atol: 1e-10, ..Default::default() };
    lsim_solve::simulate(&jit, &info, &opts, OutputGrid { t0: 0.0, t_end, dt }, &mut [])
        .unwrap_or_else(|e| panic!("{}: {e}", def.name))
}

/// Checks a run against what the page says.
fn check(name: &str, r: &SimResult, wants: &[(&str, f64, f64, f64)]) {
    for &(channel, t, want, tol) in wants {
        let ch = r.channel(channel).unwrap_or_else(|| panic!("{name}: no channel {channel}"));
        let k = r.times.iter().position(|x| (x - t).abs() < 1e-9).expect("on the grid");
        let got = ch[k];
        println!("{name}: {channel}({t}) = {got} (the page: {want})");
        assert!(
            (got - want).abs() <= tol * want.abs(),
            "{name}: {channel}({t}) = {got}, not {want}"
        );
    }
}

/// Every component and type a definition reaches through its parts.
fn reach(lib: &Library, def: &ComponentDef, seen: &mut BTreeSet<String>) {
    if !seen.insert(def.name.clone()) {
        return;
    }
    for p in &def.params {
        if let lsim_ir::ParamValue::Enum(v) = &p.default {
            seen.insert(v.rsplit_once('.').map(|(t, _)| t.to_string()).unwrap_or_default());
        }
    }
    for s in &def.components {
        if let Some(sd) = lib.components.get(&s.def) {
            reach(lib, sd, seen);
        }
    }
}

#[test]
fn every_example_of_the_documentation_parses_runs_and_gives_what_the_page_says() {
    let doc = page();
    let library = lsim_lib::library();
    let mut lib = library.clone();
    let mut defined = BTreeSet::new();
    let mut models = vec![];
    let modelica = blocks(&doc, "modelica");
    for text in &modelica {
        let parsed =
            parse_library(text, Some(&lib)).unwrap_or_else(|e| panic!("{}\n{text}", Report(&e)));
        // a model written on Windows has CRLF line ends: it reads the same
        let crlf = parse_library(&text.replace('\n', "\r\n"), Some(&lib))
            .unwrap_or_else(|e| panic!("with CRLF line ends: {}\n{text}", Report(&e)));
        assert_eq!(crlf, parsed, "with CRLF line ends");
        for c in parsed.connectors.values() {
            // the page shows the library's connectors as they are
            assert_eq!(Some(c), library.connectors.get(&c.name), "{}", c.name);
        }
        for (k, t) in &parsed.types {
            defined.insert(k.clone());
            lib.types.insert(k.clone(), t.clone());
        }
        for def in parsed.components.values() {
            lib.add(def.clone());
        }
        for def in parsed.components.values() {
            let again = parse_library(&to_text(def), Some(&lib)).expect("prints back");
            assert_eq!(&again.components[&def.name], def);
            if let Some(l) = library.components.get(&def.name) {
                assert_eq!(def, l, "the page shows the library's {} as it is", def.name);
            }
            defined.insert(def.name.clone());
            if def.ports.is_empty() {
                assert!(
                    def.name.starts_with("Examples."),
                    "{}: a model to try is Examples.…",
                    def.name
                );
                models.push(def.name.clone());
            }
        }
    }
    // every model to try runs and gives what the page says
    let mut used = BTreeSet::new();
    for name in &models {
        let (_, t_end, dt, wants) = EXPECT
            .iter()
            .find(|e| e.0 == name)
            .unwrap_or_else(|| panic!("{name}: say in EXPECT what it gives"));
        let r = run(&lib, &lib.components[name], *t_end, *dt);
        check(name, &r, wants);
        reach(&lib, &lib.components[name], &mut used);
    }
    // the gear change: the load's speed just before the shift, and the
    // shift's loss is the gearbox's
    let r = run(&lib, &lib.components["Examples.GearChange"], 8.0, 0.01);
    let k = r.times.iter().position(|t| *t == 4.0).expect("the shift on the grid");
    let ch = r.names.iter().position(|n| n == "load.w").unwrap() as u32;
    let ll = r.left_limits.iter().find(|l| l.k == k).expect("both sides of the shift");
    let before = ll.before[ll.channels.iter().position(|c| *c == ch).expect("it jumps")];
    assert!((before - 67.51054852320675).abs() < 1e-9 * 67.5, "{before}");
    let books = r.energy.expect("books");
    let gear = books.parts.iter().find(|p| p.path == "gear").expect("the gear's books");
    assert!((gear.impulse_lost - 2797.7719709762023).abs() < 1e-9 * 2797.77, "{gear:?}");
    // and the coupling turns 3750 J into heat
    let r = run(&lib, &lib.components["Examples.CouplingEngages"], 2.0, 0.5);
    // nothing jumps at 1 s: the flywheel's speed there is 100 on both sides
    let k = r.times.iter().position(|t| *t == 1.0).unwrap();
    assert_eq!(r.channel("flywheel.w").unwrap()[k], 100.0);
    let ch = r.names.iter().position(|n| n == "flywheel.w").unwrap() as u32;
    assert!(r.left_limits.iter().all(|l| l.k != k || !l.channels.contains(&ch)));
    let books = r.energy.expect("books");
    let c = books.parts.iter().find(|p| p.path == "coupling").expect("the coupling's books");
    assert!((c.lost - 3750.0).abs() < 1e-6 * 3750.0, "{c:?}");
    let unused: Vec<&String> = defined.difference(&used).collect();
    assert!(unused.is_empty(), "no model on the page uses {unused:?}");

    // the Base Modelica example imports and runs
    let bm = blocks(&doc, "basemodelica");
    assert_eq!(bm.len(), 1);
    for text in &bm {
        let def = basemodelica::import(text).unwrap_or_else(|e| panic!("{}", Report(&e)));
        assert!(def.ports.iter().all(|p| !matches!(p.kind, PortKind::Physical { .. })));
        let (_, t_end, dt, wants) = EXPECT.iter().find(|e| e.0 == def.name).expect("in EXPECT");
        check(&def.name, &run(&lib, &def, *t_end, *dt), wants);
        models.push(def.name.clone());
    }
    assert_eq!(models.len(), EXPECT.len(), "every model of EXPECT is on the page");
    println!("{} examples parse; {} models run as the page says", modelica.len() + 1, models.len());
}
