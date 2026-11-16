//! Extreme parameter values: an example project's case with some of its
//! parameters set to NaN, ±infinity, 0, ±1e±300, the smallest and largest
//! numbers or any number, at run time (`params_with` on the model built
//! once), or in the project's parts before the build (finite values: JSON
//! holds no NaN); then a short run.
#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use lsim_engine::{BuildOptions, Engine, Model};
use lsim_fuzz::{case_ids, examples, plain, project_case, run_case};
use lsim_project::import::{ImportOptions, ImportReport};
use lsim_project::{import_case, standard_registry};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Arbitrary, Debug, Clone, Copy)]
enum Extreme {
    Nan,
    Inf,
    NegInf,
    Zero,
    NegZero,
    Huge,
    NegHuge,
    Tiny,
    NegTiny,
    Denormal,
    Max,
    MinPositive,
    Any(f64),
}

impl Extreme {
    fn value(self) -> f64 {
        match self {
            Extreme::Nan => f64::NAN,
            Extreme::Inf => f64::INFINITY,
            Extreme::NegInf => f64::NEG_INFINITY,
            Extreme::Zero => 0.0,
            Extreme::NegZero => -0.0,
            Extreme::Huge => 1e300,
            Extreme::NegHuge => -1e300,
            Extreme::Tiny => 1e-300,
            Extreme::NegTiny => -1e-300,
            Extreme::Denormal => 5e-324,
            Extreme::Max => f64::MAX,
            Extreme::MinPositive => f64::MIN_POSITIVE,
            Extreme::Any(x) => x,
        }
    }
}

#[derive(Arbitrary, Debug)]
struct Input {
    case: u8,
    /// the values put in the project's parts before the build
    before_build: bool,
    changes: Vec<(u16, Extreme)>,
}

/// Every (project, case) of the examples.
fn cases() -> Vec<(usize, String)> {
    examples()
        .iter()
        .enumerate()
        .flat_map(|(i, (_, p))| case_ids(p).into_iter().map(move |c| (i, c)))
        .collect()
}

type Built = Option<Rc<(Model, ImportReport)>>;

thread_local! {
    static BUILT: RefCell<HashMap<usize, Built>> = RefCell::new(HashMap::new());
}

/// A case built once; `None` for one the new engine refuses (a lap case).
fn built(k: usize, project: &Value, case: &str) -> Built {
    BUILT.with(|b| {
        b.borrow_mut()
            .entry(k)
            .or_insert_with(|| {
                let reg = standard_registry();
                let (top, rep) =
                    import_case(project, Some(case), &reg, &ImportOptions::default()).ok()?;
                let model = Engine::new(rep.library()).build(&top, &BuildOptions::default());
                Some(Rc::new((model.expect("an example that imports builds"), rep)))
            })
            .clone()
    })
}

/// The numbers among the project's parts' own values: (system, element,
/// key).
fn numbers(p: &Value) -> Vec<(usize, usize, String)> {
    let mut out = vec![];
    for (s, sys) in p["systems"].as_array().into_iter().flatten().enumerate() {
        for (e, el) in sys["elements"].as_array().into_iter().flatten().enumerate() {
            for (k, v) in el["parameterOverrides"].as_object().into_iter().flatten() {
                if v.is_number() {
                    out.push((s, e, k.clone()));
                }
            }
        }
    }
    out
}

fuzz_target!(|input: Input| {
    let all = cases();
    let k = input.case as usize % all.len();
    let (pi, case) = &all[k];
    let project = &examples()[*pi].1;
    let changes = &input.changes[..input.changes.len().min(8)];
    if input.before_build {
        let mut p = project.clone();
        let at = numbers(&p);
        for &(i, x) in changes {
            let (s, e, key) = &at[i as usize % at.len()];
            if let Some(n) = serde_json::Number::from_f64(x.value()) {
                p["systems"][*s]["elements"][*e]["parameterOverrides"][key] = Value::Number(n);
            }
        }
        project_case(&p, Some(case));
    } else {
        let Some(b) = built(k, project, case) else { return };
        let (model, rep) = (&b.0, &b.1);
        let names = model.param_names();
        let set: Vec<(String, f64)> = changes
            .iter()
            .map(|&(i, x)| (names[i as usize % names.len()].to_string(), x.value()))
            .collect();
        match model.params_with(&set) {
            Err(m) => plain(&m),
            Ok(params) => run_case(model, rep, &params),
        }
    }
});
