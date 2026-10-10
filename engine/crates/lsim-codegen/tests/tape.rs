//! A function run from its tape computes bitwise what its machine code
//! computes (the lowering emits the same operations into both): the
//! example projects' initialisation systems, and every function of a
//! tiered model, which runs on its tapes until its machine code is in.
//! Jacobian-vector products from the coloured Jacobian agree with their
//! own forward-mode code.

#[path = "common/cars.rs"]
mod cars;
#[path = "common/random.rs"]
mod random;
#[path = "common/synth.rs"]
mod synth;

use lsim_codegen::{CodegenOptions, JitModel, compile};
use lsim_ir::runtime::{EvalInput, ModelFunctions};

/// Equal bit for bit (NaNs alike).
fn same(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| x.to_bits() == y.to_bits() || (x.is_nan() && y.is_nan()))
}

/// One case per example project.
fn one_per_project() -> Vec<cars::Car> {
    let mut seen = std::collections::HashSet::new();
    cars::cars("")
        .into_iter()
        .filter(|c| seen.insert(c.name.split('/').next().unwrap().to_string()))
        .collect()
}

/// Points around `w0`: itself and two scaled, shifted copies.
fn points(w0: &[f64]) -> Vec<Vec<f64>> {
    let mut out = vec![w0.to_vec()];
    for k in 1..=2 {
        out.push(
            w0.iter()
                .enumerate()
                .map(|(i, w)| w * (1.0 + 0.013 * k as f64) + 0.07 * ((i + k) as f64).sin())
                .collect(),
        );
    }
    out
}

#[test]
fn initialisation_tapes_are_bitwise_their_machine_code() {
    let mut checked = 0;
    for car in one_per_project() {
        let m = &car.model;
        let taped = compile(m, &CodegenOptions::default()).expect("compiles");
        let machine =
            compile(m, &CodegenOptions { tape_init: false, kernels: false, ..Default::default() })
                .expect("compiles");
        assert!(taped.report.tape_ops > 0, "{}: nothing taped", car.name);
        assert_eq!(machine.report.tape_ops, 0);
        let (Some(it), Some(im)) = (taped.init(), machine.init()) else {
            assert!(taped.init().is_none() && machine.init().is_none());
            continue;
        };
        let p: Vec<f64> = m.flat.params.iter().map(|q| q.value).collect();
        let l = *taped.layout();
        let (mut y0, mut d0) = (vec![0.0; l.n_y()], vec![0.0; l.n_d]);
        taped.start(&p, &mut y0, &mut d0);
        let u = vec![0.0; l.n_u];
        let n = it.n_w();
        let mut w0 = vec![0.0; n];
        it.guess(&p, &mut w0);
        let nw = l.n_work.max(machine.layout().n_work);
        let (mut wt, mut wm) = (vec![f64::NAN; nw], vec![f64::NAN; nw]);
        for w in points(&w0) {
            let inp = EvalInput { t: 0.0, y: &w, p: &p, d: &d0, u: &u };
            let (mut a, mut b) = (vec![0.0; n], vec![0.0; n]);
            it.residual(&inp, &mut wt, &mut a);
            im.residual(&inp, &mut wm, &mut b);
            assert!(same(&a, &b), "{}: residual {a:?} {b:?}", car.name);
            for j in 0..n.min(8) {
                let v: Vec<f64> =
                    (0..n).map(|i| if i == j { 1.0 } else { 0.25 / (1.0 + i as f64) }).collect();
                it.jvp(&inp, &v, &mut wt, &mut a);
                im.jvp(&inp, &v, &mut wm, &mut b);
                assert!(same(&a, &b), "{}: jvp", car.name);
            }
            let nnz = it.sparsity().nnz();
            let (mut ja, mut jb) = (vec![0.0; nnz], vec![0.0; nnz]);
            it.jacobian_sparse(&inp, &mut wt, &mut ja);
            im.jacobian_sparse(&inp, &mut wm, &mut jb);
            assert!(same(&ja, &jb), "{}: Jacobian", car.name);
            let (mut ya, mut yb) = (vec![0.0; l.n_y()], vec![0.0; l.n_y()]);
            it.finish(&inp, &mut wt, &mut ya);
            im.finish(&inp, &mut wm, &mut yb);
            assert!(same(&ya, &yb), "{}: finish", car.name);
            checked += 1;
        }
    }
    assert!(checked >= 6, "{checked} points checked");
}

/// The model's Jacobian-vector products, from the coloured Jacobian (the
/// default), are its own forward-mode products up to the order of the
/// sums.
#[test]
fn jacobian_vector_products_from_the_jacobian_match_their_own_code() {
    for car in one_per_project() {
        let m = &car.model;
        let via: JitModel = compile(m, &CodegenOptions::default()).expect("compiles");
        let own = compile(m, &CodegenOptions { compile_jvp: true, ..Default::default() })
            .expect("compiles");
        let l = *via.layout();
        let p: Vec<f64> = m.flat.params.iter().map(|q| q.value).collect();
        let (mut y0, mut d0) = (vec![0.0; l.n_y()], vec![0.0; l.n_d]);
        via.start(&p, &mut y0, &mut d0);
        let u = vec![0.0; l.n_u];
        let n = l.n_y();
        let nw = l.n_work.max(own.layout().n_work);
        let mut w = vec![0.0; nw];
        let pat = via.pattern().clone();
        let mut vals = vec![0.0; pat.nnz()];
        for y in points(&y0) {
            let inp = EvalInput { t: 3.0, y: &y, p: &p, d: &d0, u: &u };
            via.jacobian_sparse(&inp, &mut w, &mut vals);
            for j in 0..n {
                let v: Vec<f64> =
                    (0..n).map(|i| if i == j { 1.0 } else { 0.1 * (i as f64).cos() }).collect();
                let (mut a, mut b) = (vec![0.0; n], vec![0.0; n]);
                via.jvp(&inp, &v, &mut w, &mut a);
                own.jvp(&inp, &v, &mut w, &mut b);
                // the size of each row's terms
                let mut scale = vec![0.0; n];
                for (c, vc) in v.iter().enumerate() {
                    for k in pat.col_ptr[c]..pat.col_ptr[c + 1] {
                        scale[pat.row_idx[k]] += (vals[k] * vc).abs();
                    }
                }
                for i in 0..n {
                    let tol = 1e-12 * scale[i];
                    assert!(
                        (a[i] - b[i]).abs() <= tol,
                        "{}: row {i}: {} vs {} (terms of size {})",
                        car.name,
                        a[i],
                        b[i],
                        scale[i]
                    );
                }
            }
        }
    }
}

/// One of a model's functions, called with a work buffer and its output.
type Call<'a> = dyn FnMut(&JitModel, &mut [f64], &mut [f64]) + 'a;

/// Every function of a tiered model ([`CodegenOptions::tiered_above`]) on
/// its tapes, as the model runs before its machine code is in, against the
/// same model's machine code, bit for bit: the example projects, random
/// models with modes and `when` clauses, and large synthetic models whose
/// functions are chunked and chained (the residual and the channels from
/// one primal code).
#[test]
fn tiered_models_compute_the_same_on_tapes_and_machine_code() {
    let mut models: Vec<(String, lsim_ir::PreparedModel)> =
        one_per_project().into_iter().map(|c| (c.name, c.model)).collect();
    let mut r = synth::Rng(53);
    for k in 0..12 {
        let m = random::implicit_model(&mut r, 3, 2, 14, 4, 4, random::ALL_OPS);
        models.push((format!("random model {k}"), m));
    }
    models.push(("vehicle(2)".into(), synth::vehicle(2)));
    models.push(("network(300)".into(), synth::network(300, 3)));
    let mut checked = 0;
    for (name, m) in &models {
        for compile_jvp in [false, true] {
            let opts = CodegenOptions {
                tiered_above: 0,
                compile_jvp,
                chunk_nodes: 1_500,
                ..Default::default()
            };
            let jit = compile(m, &opts).expect("compiles");
            assert!(jit.report.tiered, "{name}");
            let tapes = jit.tapes_only().expect("tiered");
            let report = jit.wait_machine_code().expect("tiered").expect("machine code");
            assert!(report.functions > 0 && jit.machine_code_ready(), "{name}");
            assert!(tapes.tapes_only().is_none() && !report.tiered);
            let l = *jit.layout();
            assert_eq!(l, *tapes.layout());
            let p: Vec<f64> = m.flat.params.iter().map(|q| q.value).collect();
            let (mut y0, mut d0) = (vec![0.0; l.n_y()], vec![0.0; l.n_d]);
            jit.start(&p, &mut y0, &mut d0);
            let u: Vec<f64> = (0..l.n_u).map(|i| 0.5 + i as f64).collect();
            let (mut wa, mut wb) = (vec![f64::NAN; l.n_work], vec![f64::NAN; l.n_work]);
            for (k, y) in points(&y0).into_iter().enumerate() {
                let t = 0.75 * k as f64;
                let inp = EvalInput { t, y: &y, p: &p, d: &d0, u: &u };
                let mut both = |n: usize, f: &mut Call<'_>| {
                    let (mut a, mut b) = (vec![0.0; n], vec![0.0; n]);
                    f(&tapes, &mut wa, &mut a);
                    f(&jit, &mut wb, &mut b);
                    assert!(same(&a, &b), "{name}: {a:?} vs {b:?}");
                };
                both(l.n_y(), &mut |j, w, o| j.residual(&inp, w, o));
                both(jit.pattern().nnz(), &mut |j, w, o| j.jacobian_sparse(&inp, w, o));
                let v: Vec<f64> = (0..l.n_y()).map(|i| 1.0 / (1.0 + i as f64)).collect();
                both(l.n_y(), &mut |j, w, o| j.jvp(&inp, &v, w, o));
                both(l.n_roots, &mut |j, w, o| j.roots(&inp, w, o));
                both(l.n_vars, &mut |j, w, o| j.vars(&inp, w, o));
                both(jit.table_guard_list().len(), &mut |j, w, o| j.table_guards(&inp, w, o));
                let fired: Vec<f64> = (0..l.n_whens).map(|i| ((i + k) % 2) as f64).collect();
                both(l.n_d, &mut |j, w, o| {
                    o.copy_from_slice(&d0);
                    j.when(&inp, &fired, w, o)
                });
                both(l.n_d, &mut |j, w, o| {
                    o.copy_from_slice(&d0);
                    j.modes(&inp, w, o)
                });
                checked += 1;
            }
        }
    }
    assert!(checked >= 100, "{checked} points");
}
