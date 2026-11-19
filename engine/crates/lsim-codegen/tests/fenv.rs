//! The engine computes in the default floating-point environment even
//! when the calling thread's is another (flush-to-zero and
//! denormals-are-zero, as a host library may set them): compiling (on the
//! calling thread, the compiler's threads and a tiered model's background
//! thread) and running a model enter the default for their duration
//! (`lsim_ir::fenv`) and give the thread its own back after.
#![cfg(target_arch = "x86_64")]

#[path = "common/synth.rs"]
mod synth;

use lsim_codegen::{CodegenOptions, JitModel, compile};
use lsim_ir::PreparedModel;
use lsim_ir::fenv::{DEFAULT_MXCSR, is_default, mxcsr, set_mxcsr};
use lsim_ir::runtime::{EvalInput, ModelFunctions};
use lsim_ir::table::TableData;
use lsim_solve::{OutputGrid, RunInfo, SolverOptions};
use synth::{Builder, c, tab, v};

/// Flush-to-zero and denormals-are-zero, as a host library may set them.
const FTZ_DAZ: u32 = DEFAULT_MXCSR | (1 << 15) | (1 << 6);

/// A model whose values are subnormal numbers scaled up: a parameter, a
/// table's data (its coefficients are computed when it is compiled) and
/// a state rising at their rate.
fn tiny_model(extra: usize) -> PreparedModel {
    let mut b = Builder::new();
    let x = b.state("x", 0.0);
    let a = b.param("a", 3e-310);
    let t = b.table(
        "tiny",
        TableData::new_1d(vec![0.0, 1.0, 2.0, 4.0], vec![0.0, 2e-310, 5e-310, 6e-310]),
    );
    let rate = b.let_("rate", (a + tab(t, vec![Expr::Time])) * c(1e300));
    b.der(x, rate.clone() * c(1e-300) * v(x) + rate);
    // more of the same, to make a model large enough to tier
    for i in 0..extra {
        let _ =
            b.let_(&format!("more{i}"), tab(t, vec![Expr::Time * c(0.5 + i as f64)]) * c(1e300));
    }
    b.finish()
}

use lsim_ir::Expr;

/// Every channel at a few times, in the default environment.
fn channels(j: &JitModel, m: &PreparedModel) -> Vec<u64> {
    assert!(is_default());
    let l = *j.layout();
    let p = synth::params(m);
    let (mut w, mut out) = (vec![0.0; l.n_work], vec![0.0; l.n_vars]);
    let mut bits = vec![];
    for t in [0.25, 1.5, 3.0] {
        let y = vec![1.0; l.n_y()];
        let inp = EvalInput { t, y: &y, p: &p, d: &[], u: &[] };
        j.vars(&inp, &mut w, &mut out);
        bits.extend(out.iter().map(|x| x.to_bits()));
    }
    bits
}

#[test]
fn compiling_and_running_use_the_default_floating_point_environment() {
    let m = tiny_model(0);
    let info = RunInfo::from_prepared(&m);
    let so = SolverOptions::default();
    let grid = OutputGrid { t0: 0.0, t_end: 3.0, dt: 0.5 };
    // the reference: compiled and run in the default environment
    assert!(is_default());
    let reference = compile(&m, &CodegenOptions::default()).expect("compiles");
    let want = channels(&reference, &m);
    let run = lsim_solve::simulate(&reference, &info, &so, grid, &mut []).expect("runs");
    assert!(run.values.iter().flatten().any(|x| *x != 0.0));

    // SAFETY: the test's own thread, restored at the end
    unsafe { set_mxcsr(FTZ_DAZ) };
    // the hazard: the generated code called directly under the host's
    // environment reads the subnormal parameter and data as zeros
    {
        let l = *reference.layout();
        let p = synth::params(&m);
        let (mut w, mut out) = (vec![0.0; l.n_work], vec![0.0; l.n_vars]);
        let y = vec![1.0; l.n_y()];
        reference.vars(&EvalInput { t: 1.5, y: &y, p: &p, d: &[], u: &[] }, &mut w, &mut out);
        assert!(out.iter().all(|x| *x == 0.0 || *x == 1.0), "{out:?}");
    }
    // compiled here (one thread, four, tiered with a background thread)
    // and run here: as in the default environment, and the thread's own
    // environment back after each
    let mut compiled = vec![];
    for opts in [
        CodegenOptions { threads: 1, ..Default::default() },
        CodegenOptions { threads: 4, chunk_nodes: 10, ..Default::default() },
        CodegenOptions { tiered_above: 0, ..Default::default() },
    ] {
        let j = compile(&m, &opts).expect("compiles");
        assert_eq!(mxcsr(), FTZ_DAZ);
        if let Some(r) = j.wait_machine_code() {
            r.expect("machine code");
        }
        compiled.push(j);
    }
    let here = lsim_solve::simulate(&reference, &info, &so, grid, &mut []).expect("runs");
    assert_eq!(mxcsr(), FTZ_DAZ);
    assert!(here.report.notes.iter().any(|n| n.contains("floating-point environment")));
    // SAFETY: back to the default
    unsafe { set_mxcsr(DEFAULT_MXCSR) };

    assert_eq!(here.times, run.times);
    for (a, b) in here.values.iter().zip(&run.values) {
        assert!(a.iter().zip(b).all(|(x, y)| x.to_bits() == y.to_bits()), "{a:?} vs {b:?}");
    }
    for j in &compiled {
        assert_eq!(channels(j, &m), want);
    }
}
