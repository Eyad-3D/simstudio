//! A compiled model with a run's own start values.
//!
//! [`Started`] wraps a model and forwards **every** method of
//! [`ModelFunctions`] to it, changing only [`ModelFunctions::start`] when it
//! is given start values of its own (fast mode's consistent start at the
//! first point of the traces). A method left at the trait's default would
//! not fail to compile but would quietly change the run (DESIGN.md §5.8):
//! modes would stop switching, table guards and the compiled initialisation
//! would vanish, a run whose books or conditions read a table would not
//! start, a model given other table data would be run on the prepared
//! breakpoints. The tests below call every method on the wrapper and on
//! the model it wraps and compare, and check that the impl names every
//! method the trait declares.

use std::sync::Arc;

use lsim_ir::runtime::{
    BooksKernel, BooksSpec, ConditionKernels, EvalInput, InitFunctions, Layout, ModelFunctions,
    SparsityPattern, TableGuard,
};

/// A model with optional start values of its own.
pub struct Started<'a> {
    m: &'a dyn ModelFunctions,
    start: Option<(Vec<f64>, Vec<f64>)>,
}

impl<'a> Started<'a> {
    /// `m` as it is (its own start values).
    pub fn new(m: &'a dyn ModelFunctions) -> Self {
        Started { m, start: None }
    }

    /// `m` started at `y0` and `d0` (lengths `n_y` and `n_d`).
    pub fn with_start(m: &'a dyn ModelFunctions, y0: Vec<f64>, d0: Vec<f64>) -> Self {
        let l = m.layout();
        assert!(y0.len() == l.n_y() && d0.len() == l.n_d, "start values of the wrong length");
        Started { m, start: Some((y0, d0)) }
    }

    /// The model it wraps.
    pub fn inner(&self) -> &'a dyn ModelFunctions {
        self.m
    }
}

impl ModelFunctions for Started<'_> {
    fn layout(&self) -> &Layout {
        self.m.layout()
    }
    fn residual(&self, inp: &EvalInput<'_>, work: &mut [f64], out: &mut [f64]) {
        self.m.residual(inp, work, out)
    }
    fn jvp(&self, inp: &EvalInput<'_>, v: &[f64], work: &mut [f64], out: &mut [f64]) {
        self.m.jvp(inp, v, work, out)
    }
    fn roots(&self, inp: &EvalInput<'_>, work: &mut [f64], out: &mut [f64]) {
        self.m.roots(inp, work, out)
    }
    fn vars(&self, inp: &EvalInput<'_>, work: &mut [f64], out: &mut [f64]) {
        self.m.vars(inp, work, out)
    }
    fn when(&self, inp: &EvalInput<'_>, fired: &[f64], work: &mut [f64], d_out: &mut [f64]) {
        self.m.when(inp, fired, work, d_out)
    }
    fn start(&self, p: &[f64], y0: &mut [f64], d0: &mut [f64]) {
        match &self.start {
            Some((y, d)) => {
                y0.copy_from_slice(y);
                d0.copy_from_slice(d);
            }
            None => self.m.start(p, y0, d0),
        }
    }
    fn jacobian_dense(&self, inp: &EvalInput<'_>, work: &mut [f64], out: &mut [f64]) {
        self.m.jacobian_dense(inp, work, out)
    }
    fn sparsity(&self) -> Option<&SparsityPattern> {
        self.m.sparsity()
    }
    fn jacobian_sparse(&self, inp: &EvalInput<'_>, work: &mut [f64], values: &mut [f64]) {
        self.m.jacobian_sparse(inp, work, values)
    }
    fn modes(&self, inp: &EvalInput<'_>, work: &mut [f64], d_out: &mut [f64]) {
        self.m.modes(inp, work, d_out)
    }
    fn init(&self) -> Option<&dyn InitFunctions> {
        self.m.init()
    }
    fn table_guard_list(&self) -> &[TableGuard] {
        self.m.table_guard_list()
    }
    fn table_guards(&self, inp: &EvalInput<'_>, work: &mut [f64], out: &mut [f64]) {
        self.m.table_guards(inp, work, out)
    }
    fn eval_table(&self, k: u32, args: [f64; 2]) -> Option<(f64, [f64; 2])> {
        self.m.eval_table(k, args)
    }
    fn eval_table_order(&self, k: u32, order: [u8; 2], args: [f64; 2]) -> Option<(f64, [f64; 2])> {
        self.m.eval_table_order(k, order, args)
    }
    fn table_axes(&self, k: u32) -> Option<[Vec<f64>; 2]> {
        self.m.table_axes(k)
    }
    fn condition_kernels(&self) -> Option<&dyn ConditionKernels> {
        self.m.condition_kernels()
    }
    fn books_kernel(&self, spec: &BooksSpec) -> Option<Arc<dyn BooksKernel>> {
        self.m.books_kernel(spec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsim_ir::runtime::BooksCover;
    use lsim_ir::table::Outside;

    /// A model whose every method gives something no default gives, so a
    /// method the wrapper leaves at its default shows.
    struct Probe {
        layout: Layout,
        pattern: SparsityPattern,
        guards: Vec<TableGuard>,
    }

    struct ProbeInit;

    impl InitFunctions for ProbeInit {
        fn n_w(&self) -> usize {
            1
        }
        fn guess(&self, _p: &[f64], w0: &mut [f64]) {
            w0[0] = 41.0;
        }
        fn residual(&self, _: &EvalInput<'_>, _: &mut [f64], out: &mut [f64]) {
            out[0] = 42.0;
        }
        fn jvp(&self, _: &EvalInput<'_>, _: &[f64], _: &mut [f64], out: &mut [f64]) {
            out[0] = 43.0;
        }
        fn sparsity(&self) -> &SparsityPattern {
            unreachable!("not asked")
        }
        fn jacobian_sparse(&self, _: &EvalInput<'_>, _: &mut [f64], values: &mut [f64]) {
            values[0] = 44.0;
        }
        fn finish(&self, _: &EvalInput<'_>, _: &mut [f64], y0: &mut [f64]) {
            y0[0] = 45.0;
        }
    }

    static PROBE_INIT: ProbeInit = ProbeInit;

    struct ProbeKernels;

    impl ConditionKernels for ProbeKernels {
        fn covers(&self, k: usize) -> bool {
            k == 1
        }
        fn scratch(&self) -> (usize, usize) {
            (7, 8)
        }
        fn point(&self, k: usize, inp: &EvalInput<'_>, _: &mut [f64]) -> f64 {
            100.0 + k as f64 + inp.t
        }
        fn enclose(
            &self,
            _k: usize,
            t: [f64; 2],
            _y: &[lsim_ir::runtime::Enclosure],
            _d: &[f64],
            _p: &[f64],
            _u: &[f64],
            _work: &mut [lsim_ir::runtime::Enclosure],
        ) -> lsim_ir::runtime::Enclosure {
            lsim_ir::runtime::Enclosure { v: t, d: [1.0, 2.0], dd: [3.0, 4.0] }
        }
    }

    static PROBE_KERNELS: ProbeKernels = ProbeKernels;

    fn fill(out: &mut [f64], base: f64, inp: &EvalInput<'_>) {
        for (i, o) in out.iter_mut().enumerate() {
            *o = base + i as f64 + inp.t + inp.y.iter().sum::<f64>();
        }
    }

    impl ModelFunctions for Probe {
        fn layout(&self) -> &Layout {
            &self.layout
        }
        fn residual(&self, inp: &EvalInput<'_>, _: &mut [f64], out: &mut [f64]) {
            fill(out, 1.0, inp)
        }
        fn jvp(&self, inp: &EvalInput<'_>, v: &[f64], _: &mut [f64], out: &mut [f64]) {
            fill(out, 2.0 + v[0], inp)
        }
        fn roots(&self, inp: &EvalInput<'_>, _: &mut [f64], out: &mut [f64]) {
            fill(out, 3.0, inp)
        }
        fn vars(&self, inp: &EvalInput<'_>, _: &mut [f64], out: &mut [f64]) {
            fill(out, 4.0, inp)
        }
        fn when(&self, inp: &EvalInput<'_>, fired: &[f64], _: &mut [f64], d_out: &mut [f64]) {
            fill(d_out, 5.0 + fired[0], inp)
        }
        fn start(&self, p: &[f64], y0: &mut [f64], d0: &mut [f64]) {
            y0.iter_mut().for_each(|v| *v = 6.0 + p[0]);
            d0.iter_mut().for_each(|v| *v = 7.0 + p[0]);
        }
        fn jacobian_dense(&self, inp: &EvalInput<'_>, _: &mut [f64], out: &mut [f64]) {
            fill(out, 8.0, inp)
        }
        fn sparsity(&self) -> Option<&SparsityPattern> {
            Some(&self.pattern)
        }
        fn jacobian_sparse(&self, inp: &EvalInput<'_>, _: &mut [f64], values: &mut [f64]) {
            fill(values, 9.0, inp)
        }
        fn modes(&self, inp: &EvalInput<'_>, _: &mut [f64], d_out: &mut [f64]) {
            fill(d_out, 10.0, inp)
        }
        fn init(&self) -> Option<&dyn InitFunctions> {
            Some(&PROBE_INIT)
        }
        fn table_guard_list(&self) -> &[TableGuard] {
            &self.guards
        }
        fn table_guards(&self, inp: &EvalInput<'_>, _: &mut [f64], out: &mut [f64]) {
            fill(out, 11.0, inp)
        }
        fn eval_table(&self, k: u32, args: [f64; 2]) -> Option<(f64, [f64; 2])> {
            Some((12.0 + k as f64, args))
        }
        fn eval_table_order(
            &self,
            k: u32,
            order: [u8; 2],
            args: [f64; 2],
        ) -> Option<(f64, [f64; 2])> {
            Some((15.0 + k as f64 + order[0] as f64 + 2.0 * order[1] as f64, args))
        }
        fn table_axes(&self, k: u32) -> Option<[Vec<f64>; 2]> {
            Some([vec![13.0, k as f64], vec![14.0]])
        }
        fn condition_kernels(&self) -> Option<&dyn ConditionKernels> {
            Some(&PROBE_KERNELS)
        }
        fn books_kernel(&self, spec: &BooksSpec) -> Option<Arc<dyn BooksKernel>> {
            Some(Arc::new(ProbeBooks(spec.n_out)))
        }
    }

    /// A books kernel that says which spec it was made for.
    struct ProbeBooks(usize);

    impl BooksKernel for ProbeBooks {
        fn cover(&self, j: usize) -> BooksCover {
            BooksCover { power: j == 0, loss: true, stored: false }
        }
        fn scratch(&self) -> usize {
            16 + self.0
        }
        fn eval(&self, t: f64, _: &[f64], _: &[f64], _: &[f64], _: &mut [f64], out: &mut [f64]) {
            out.fill(17.0 + t);
        }
    }

    fn probe() -> Probe {
        Probe {
            layout: Layout {
                n_x: 1,
                n_z: 1,
                n_p: 1,
                n_d: 2,
                n_u: 0,
                n_roots: 2,
                n_whens: 1,
                n_vars: 3,
                n_work: 1,
            },
            pattern: SparsityPattern { n: 2, col_ptr: vec![0, 1, 2], row_idx: vec![0, 1] },
            guards: vec![TableGuard { table: 3, axis: 1, outside: Outside::Error }],
        }
    }

    /// Every method's output on `a` and on `b`, side by side.
    fn outputs(m: &dyn ModelFunctions) -> Vec<(String, Vec<f64>)> {
        let l = *m.layout();
        let (y, p, d) = (vec![0.25, 0.5], vec![2.0], vec![1.0, 0.0]);
        let inp = EvalInput { t: 1.5, y: &y, p: &p, d: &d, u: &[] };
        let mut work = vec![0.0; l.n_work];
        let mut o = vec![];
        let mut put = |name: &str, v: Vec<f64>| o.push((name.to_string(), v));
        let ny = l.n_y();
        let mut b = vec![0.0; ny];
        m.residual(&inp, &mut work, &mut b);
        put("residual", b);
        let mut b = vec![0.0; ny];
        m.jvp(&inp, &[1.0, 0.0], &mut work, &mut b);
        put("jvp", b);
        let mut b = vec![0.0; l.n_roots];
        m.roots(&inp, &mut work, &mut b);
        put("roots", b);
        let mut b = vec![0.0; l.n_vars];
        m.vars(&inp, &mut work, &mut b);
        put("vars", b);
        let mut b = vec![0.0; l.n_d];
        m.when(&inp, &[1.0], &mut work, &mut b);
        put("when", b);
        let (mut y0, mut d0) = (vec![0.0; ny], vec![0.0; l.n_d]);
        m.start(&p, &mut y0, &mut d0);
        put("start", [y0, d0].concat());
        let mut b = vec![0.0; ny * ny];
        m.jacobian_dense(&inp, &mut work, &mut b);
        put("jacobian_dense", b);
        let sp = m.sparsity().map(|s| {
            [vec![s.n as f64], s.col_ptr.iter().chain(&s.row_idx).map(|&v| v as f64).collect()]
                .concat()
        });
        put("sparsity", sp.unwrap_or_default());
        let mut b = vec![0.0; 2];
        m.jacobian_sparse(&inp, &mut work, &mut b);
        put("jacobian_sparse", b);
        let mut b = vec![0.0; l.n_d];
        m.modes(&inp, &mut work, &mut b);
        put("modes", b);
        let init = m.init().map(|f| {
            let mut v = vec![f.n_w() as f64];
            let mut w = vec![0.0];
            f.guess(&p, &mut w);
            v.extend(&w);
            let mut r = vec![0.0];
            f.residual(&inp, &mut work, &mut r);
            v.extend(&r);
            f.jvp(&inp, &[1.0], &mut work, &mut r);
            v.extend(&r);
            f.jacobian_sparse(&inp, &mut work, &mut r);
            v.extend(&r);
            let mut y0 = vec![0.0; ny];
            f.finish(&inp, &mut work, &mut y0);
            v.extend(&y0);
            v
        });
        put("init", init.unwrap_or_default());
        let gl: Vec<f64> = m
            .table_guard_list()
            .iter()
            .flat_map(|g| [g.table as f64, g.axis as f64, g.outside as u8 as f64])
            .collect();
        put("table_guard_list", gl);
        let mut b = vec![0.0; m.table_guard_list().len()];
        m.table_guards(&inp, &mut work, &mut b);
        put("table_guards", b);
        let t = m.eval_table(2, [0.5, 0.75]).map(|(v, d)| vec![v, d[0], d[1]]);
        put("eval_table", t.unwrap_or_default());
        let a = m.table_axes(2).map(|[a, b]| [a, b].concat());
        put("table_axes", a.unwrap_or_default());
        let k = m.condition_kernels().map(|k| {
            let (s1, s2) = k.scratch();
            let e = k.enclose(1, [0.0, 1.0], &[], &[], &[], &[], &mut []);
            vec![
                k.covers(1) as u8 as f64,
                k.covers(0) as u8 as f64,
                s1 as f64,
                s2 as f64,
                k.point(1, &inp, &mut work),
                e.v[0],
                e.v[1],
                e.d[0],
                e.dd[1],
            ]
        });
        put("condition_kernels", k.unwrap_or_default());
        let t = m.eval_table_order(2, [1, 0], [0.5, 0.75]).map(|(v, d)| vec![v, d[0], d[1]]);
        put("eval_table_order", t.unwrap_or_default());
        let spec = BooksSpec { parts: vec![], chain: vec![], rates: vec![], n_out: 2 };
        let b = m.books_kernel(&spec).map(|k| {
            let c = k.cover(0);
            let mut out = vec![0.0; 2];
            k.eval(1.5, &[], &[], &[], &mut [], &mut out);
            [
                vec![
                    c.power as u8 as f64,
                    c.loss as u8 as f64,
                    c.stored as u8 as f64,
                    k.scratch() as f64,
                ],
                out,
            ]
            .concat()
        });
        put("books_kernel", b.unwrap_or_default());
        o
    }

    #[test]
    fn the_wrapper_forwards_every_method() {
        let m = probe();
        let inner = outputs(&m);
        let wrapped = outputs(&Started::new(&m));
        assert_eq!(inner.len(), wrapped.len());
        for ((name, a), (_, b)) in inner.iter().zip(&wrapped) {
            assert!(!a.is_empty(), "the probe gives nothing for {name}");
            assert_eq!(a, b, "Started does not forward {name}");
        }
        // with start values of its own, only `start` differs
        let s = Started::with_start(&m, vec![-1.0, -2.0], vec![-3.0, -4.0]);
        for ((name, a), (_, b)) in inner.iter().zip(&outputs(&s)) {
            if name == "start" {
                assert_eq!(b, &[-1.0, -2.0, -3.0, -4.0]);
            } else {
                assert_eq!(a, b, "Started with a start does not forward {name}");
            }
        }
    }

    /// The names of the methods a trait declares, from its source.
    fn trait_methods(src: &str, header: &str) -> Vec<String> {
        let start = src.find(header).expect("the trait is declared");
        let body = &src[start..];
        let mut depth = 0i32;
        let mut end = body.len();
        for (i, ch) in body.char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = i;
                        break;
                    }
                }
                _ => {}
            }
        }
        let body = &body[..end];
        // methods at the trait's own level: `    fn name(` with four spaces
        body.lines()
            .filter_map(|l| l.strip_prefix("    fn "))
            .filter_map(|l| l.split('(').next())
            .map(|n| n.trim().to_string())
            .collect()
    }

    #[test]
    fn the_wrapper_names_every_method_of_the_trait() {
        let trait_src = include_str!("../../lsim-ir/src/runtime.rs");
        let methods = trait_methods(trait_src, "pub trait ModelFunctions");
        assert!(methods.len() >= 17, "found only {methods:?}");
        let own = include_str!("started.rs");
        let imp = &own[own.find("impl ModelFunctions for Started").expect("the impl")..];
        let imp = &imp[..imp.find("\n}\n").expect("its end")];
        for m in &methods {
            assert!(
                imp.contains(&format!("fn {m}(")) && imp.contains(&format!("self.m.{m}(")),
                "Started does not forward ModelFunctions::{m}: add it (DESIGN.md §5.8)"
            );
        }
        // and the behavioural test above covers each of them
        let test = &own[own.find("fn outputs(").expect("the test")..];
        for m in &methods {
            if m != "layout" {
                assert!(test.contains(&format!(".{m}(")), "the test does not call {m}");
            }
        }
    }
}
