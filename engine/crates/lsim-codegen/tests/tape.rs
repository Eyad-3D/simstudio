//! A function run from its tape computes bitwise what its machine code
//! computes (the lowering emits the same operations into both), and
//! Jacobian-vector products from the coloured Jacobian agree with their
//! own forward-mode code: on the example projects' initialisation
//! systems, at their start and away from it.

#[path = "common/cars.rs"]
mod cars;

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
