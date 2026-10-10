//! The large-model settings (no Cranelift optimisation, the single-pass
//! register allocator, functions cut into chunks, the channels gathered
//! from the residual's assignments) compute bitwise what the default
//! settings compute, on the random models `fuzz.rs` checks against the
//! interpreter with the default settings only: every function, at random
//! points (from the review of WP3).

#[path = "common/random.rs"]
mod random;
#[path = "common/synth.rs"]
mod synth;

use lsim_codegen::{CodegenOptions, JitModel, compile};
use lsim_ir::runtime::{EvalInput, ModelFunctions};
use synth::Rng;

/// One of a model's functions, called with a work buffer and its output.
type Call<'a> = dyn Fn(&JitModel, &mut [f64], &mut [f64]) + 'a;

fn same(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| x.to_bits() == y.to_bits() || (x.is_nan() && y.is_nan()))
}

#[test]
fn large_model_settings_compute_what_the_default_computes() {
    let mut r = Rng(2027);
    let large = CodegenOptions { auto_fast_above: 0, chunk_nodes: 300, ..Default::default() };
    let (mut models, mut checks) = (0, 0);
    for model_no in 0..100 {
        let m = random::implicit_model(&mut r, 3, 2, 100, 8, 5, random::ALL_OPS);
        let a: JitModel = compile(&m, &CodegenOptions::default()).expect("compiles");
        let b: JitModel = compile(&m, &large).expect("compiles");
        let l = *a.layout();
        assert_eq!(l.n_y(), b.layout().n_y());
        let p: Vec<f64> = m.flat.params.iter().map(|q| q.value).collect();
        let (mut wa, mut wb) = (vec![0.0; l.n_work], vec![0.0; b.layout().n_work]);
        for _ in 0..5 {
            let t = r.range(-3.0, 3.0);
            let y: Vec<f64> = (0..l.n_y()).map(|_| random::constant(&mut r)).collect();
            let d: Vec<f64> = (0..l.n_d).map(|_| random::constant(&mut r)).collect();
            let u = vec![0.0; l.n_u];
            let inp = EvalInput { t, y: &y, p: &p, d: &d, u: &u };
            let mut both = |what: &str, n: usize, f: &Call<'_>| {
                let (mut x, mut z) = (vec![0.0; n], vec![0.0; n]);
                f(&a, &mut wa, &mut x);
                f(&b, &mut wb, &mut z);
                assert!(same(&x, &z), "model {model_no}, {what} at t = {t}:\n{x:?}\n{z:?}");
                checks += n;
            };
            both("residual", l.n_y(), &|j, w, o| j.residual(&inp, w, o));
            both("roots", l.n_roots, &|j, w, o| j.roots(&inp, w, o));
            both("vars", l.n_vars, &|j, w, o| j.vars(&inp, w, o));
            both("jacobian", a.pattern().nnz(), &|j, w, o| j.jacobian_sparse(&inp, w, o));
        }
        models += 1;
    }
    println!("{models} random models, {checks} values compared");
}
