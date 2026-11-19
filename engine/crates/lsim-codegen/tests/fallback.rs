//! Where the system refuses to make code executable (Windows' Arbitrary
//! Code Guard, some security policies), a model runs on its tapes, which
//! compute bit for bit what its machine code computes: compiling still
//! succeeds, and says so. Simulated with the memory provider's refusal
//! (`CodegenOptions::deny_executable_memory`), for a model compiled at
//! once and for a tiered one (whose background compilation then fails and
//! leaves it on its tapes).

#[path = "common/cars.rs"]
mod cars;
#[path = "common/random.rs"]
mod random;
#[path = "common/synth.rs"]
mod synth;

use lsim_codegen::{CodegenOptions, JitModel, MachineCode, compile};
use lsim_ir::PreparedModel;
use lsim_ir::runtime::{EvalInput, ModelFunctions};

fn same(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| x.to_bits() == y.to_bits() || (x.is_nan() && y.is_nan()))
}

/// One of a model's functions, called with a work buffer and its output.
type Call<'a> = dyn Fn(&JitModel, &mut [f64], &mut [f64]) + 'a;

/// Every function of `a` and `b` at a few points, bit for bit.
fn agree(name: &str, m: &PreparedModel, a: &JitModel, b: &JitModel) -> usize {
    let l = *a.layout();
    // (tapes keep their registers in `work` too)
    assert_eq!(
        lsim_ir::runtime::Layout { n_work: 0, ..l },
        lsim_ir::runtime::Layout { n_work: 0, ..*b.layout() }
    );
    let p: Vec<f64> = m.flat.params.iter().map(|q| q.value).collect();
    let (mut y0, mut d0) = (vec![0.0; l.n_y()], vec![0.0; l.n_d]);
    a.start(&p, &mut y0, &mut d0);
    let u = vec![0.25; l.n_u];
    let (mut wa, mut wb) = (vec![f64::NAN; l.n_work], vec![f64::NAN; b.layout().n_work]);
    let mut checked = 0;
    for k in 0..5 {
        let y: Vec<f64> = y0
            .iter()
            .enumerate()
            .map(|(i, x)| x * (1.0 + 0.1 * k as f64) + 0.01 * i as f64)
            .collect();
        let inp = EvalInput { t: 0.5 * k as f64, y: &y, p: &p, d: &d0, u: &u };
        let mut both = |n: usize, f: &Call<'_>| {
            let (mut x, mut z) = (vec![0.0; n], vec![0.0; n]);
            f(a, &mut wa, &mut x);
            f(b, &mut wb, &mut z);
            assert!(same(&x, &z), "{name}: {x:?} vs {z:?}");
            checked += n;
        };
        both(l.n_y(), &|j, w, o| j.residual(&inp, w, o));
        both(a.pattern().nnz(), &|j, w, o| j.jacobian_sparse(&inp, w, o));
        both(l.n_roots, &|j, w, o| j.roots(&inp, w, o));
        both(l.n_vars, &|j, w, o| j.vars(&inp, w, o));
        both(a.table_guard_list().len(), &|j, w, o| j.table_guards(&inp, w, o));
        let fired = vec![1.0; l.n_whens];
        both(l.n_d, &|j, w, o| {
            o.copy_from_slice(&d0);
            j.when(&inp, &fired, w, o)
        });
    }
    checked
}

#[test]
fn without_executable_memory_a_model_runs_on_its_tapes() {
    let mut models: Vec<(String, PreparedModel)> =
        cars::one_per_project(cars::cars("")).into_iter().map(|c| (c.name, c.model)).collect();
    let mut r = synth::Rng(61);
    for k in 0..6 {
        models.push((format!("random {k}"), random::model(&mut r, 3, 20, 4, 4, random::ALL_OPS)));
    }
    let mut checked = 0;
    for (name, m) in &models {
        let machine = compile(m, &CodegenOptions::default()).expect("compiles");
        assert_eq!(machine.machine_code(), MachineCode::Ready, "{name}");
        let denied = CodegenOptions { deny_executable_memory: true, ..Default::default() };
        let taped = compile(m, &denied).expect("compiles on tapes");
        let why = taped.report.on_tapes.clone().expect("on tapes");
        assert!(why.contains("executable"), "{why}");
        assert_eq!(taped.machine_code(), MachineCode::Tapes(why), "{name}");
        assert!(!taped.machine_code_ready() && taped.report.tape_ops > 0, "{name}");
        checked += agree(name, m, &machine, &taped);
    }
    assert!(checked > 10_000, "{checked}");
}

#[test]
fn a_tiered_model_without_executable_memory_stays_on_its_tapes() {
    let m = synth::network(200, 5);
    let opts = CodegenOptions { tiered_above: 0, ..Default::default() };
    let machine = compile(&m, &opts).expect("compiles");
    machine.wait_machine_code().expect("tiered").expect("machine code");
    let denied = CodegenOptions { deny_executable_memory: true, ..opts };
    let taped = compile(&m, &denied).expect("compiles");
    assert!(taped.report.tiered && taped.report.on_tapes.is_none());
    let why = taped.wait_machine_code().expect("tiered").expect_err("no executable memory");
    assert!(why.contains("executable"), "{why}");
    assert_eq!(taped.machine_code(), MachineCode::Tapes(why));
    assert!(agree("network(200)", &m, &machine, &taped) > 1_000);
}
