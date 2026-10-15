//! The compiled energy books against the interpreter (DESIGN.md §11):
//! lsim-solve's `energy::Integrand` with the model's compiled kernel
//! (`ModelFunctions::books_kernel`) computes bitwise what it computes
//! interpreted, on its tape and as machine code, on the books of every
//! example project at the operating points of a drive and on random books
//! of random models (random powers, losses and stored energies, through
//! random chains of assignments, at random points and rates, NaNs and
//! infinities included).

#[path = "common/cars.rs"]
mod cars;
#[path = "common/random.rs"]
mod random;
#[path = "common/synth.rs"]
mod synth;

use lsim_codegen::{CodegenOptions, CompiledBooks, JitModel, compile};
use lsim_ir::flat::{Instance, InstanceEnergy, InstanceId, PortPower};
use lsim_ir::runtime::{BooksKernel, BooksSpec, EvalInput, Layout, ModelFunctions};
use lsim_ir::{Expr, PreparedModel};
use lsim_solve::RunInfo;
use lsim_solve::energy::Integrand;
use std::sync::Arc;
use synth::Rng;

/// Equal bit for bit (NaNs alike).
fn same(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
}

/// A compiled model whose books kernel is a given one (its tape alone,
/// say); everything else the model's.
struct With<'a> {
    m: &'a JitModel,
    books: Option<Arc<CompiledBooks>>,
}

impl ModelFunctions for With<'_> {
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
        self.m.start(p, y0, d0)
    }
    fn eval_table(&self, k: u32, args: [f64; 2]) -> Option<(f64, [f64; 2])> {
        self.m.eval_table(k, args)
    }
    fn books_kernel(&self, _: &BooksSpec) -> Option<Arc<dyn BooksKernel>> {
        self.books.clone().map(|b| b as Arc<dyn BooksKernel>)
    }
}

/// One model's books: interpreted, on the kernel's tape, as machine code.
struct Books<'a> {
    jit: &'a JitModel,
    interpreted: Integrand,
    taped: Integrand,
    machine: Integrand,
    /// parts, of them those whose power, loss and stored energy's rate
    /// are compiled, and those whose stored energy's rate is exact
    counts: [usize; 5],
}

impl<'a> Books<'a> {
    fn new(m: &PreparedModel, jit: &'a JitModel) -> Option<Self> {
        let info = RunInfo::from_prepared(m);
        let energy = info.energy.clone().filter(|e| !e.parts.is_empty())?;
        let l = *jit.layout();
        let interpreted = Integrand::new(&energy, &info, &l);
        // the kernel the run gets (machine code once it is in) and its tape
        let spec = Integrand::new(&energy, &info, &l).spec();
        let kernel = jit.compile_books(&spec)?;
        kernel.wait_machine_code().expect("machine code compiled").expect("machine code runs");
        assert!(kernel.machine_code_ready());
        let tape = Arc::new(kernel.tape_only());
        let taped =
            Integrand::new(&energy, &info, &l).compiled_by(&With { m: jit, books: Some(tape) });
        let machine = Integrand::new(&energy, &info, &l)
            .compiled_by(&With { m: jit, books: Some(kernel.clone()) });
        let mut counts = [spec.parts.len(), 0, 0, 0, 0];
        for j in 0..spec.parts.len() {
            let c = kernel.cover(j);
            counts[1] += c.power as usize;
            counts[2] += c.loss as usize;
            counts[3] += c.stored as usize;
            counts[4] += (spec.parts[j].exact && spec.parts[j].stored.is_some()) as usize;
        }
        Some(Books { jit, interpreted, taped, machine, counts })
    }

    /// Checks the books at (`t`, `y`, `d`) with the rates `ydot`.
    fn check(&mut self, t: f64, y: &[f64], d: &[f64], ydot: &[f64], p: &[f64]) -> bool {
        let l = *self.jit.layout();
        let u = vec![0.0; l.n_u];
        let inp = EvalInput { t, y, p, d, u: &u };
        let n = self.interpreted.len();
        let mut work = vec![0.0; l.n_work];
        let mut want = vec![f64::NAN; n];
        self.interpreted.eval(self.jit, &inp, ydot, &mut work, &mut want);
        for (what, q) in [("tape", &mut self.taped), ("machine code", &mut self.machine)] {
            let mut got = vec![f64::NAN; n];
            q.eval(self.jit, &inp, ydot, &mut work, &mut got);
            for k in 0..n {
                if !same(got[k], want[k]) {
                    eprintln!(
                        "integrand {k} at t = {t} ({what}): {:e} ({:#x}), interpreted {:e} ({:#x})",
                        got[k],
                        got[k].to_bits(),
                        want[k],
                        want[k].to_bits()
                    );
                    return false;
                }
            }
        }
        true
    }
}

/// The rates of y at a point: the model's own x' (as the run gives them),
/// random rates for the iteration variables.
fn rates(jit: &JitModel, t: f64, y: &[f64], d: &[f64], p: &[f64], r: &mut Rng) -> Vec<f64> {
    let l = *jit.layout();
    let u = vec![0.0; l.n_u];
    let mut out = vec![0.0; l.n_y()];
    let mut work = vec![0.0; l.n_work];
    jit.residual(&EvalInput { t, y, p, d, u: &u }, &mut work, &mut out);
    for z in &mut out[l.n_x..] {
        *z = r.range(-3.0, 3.0);
    }
    out
}

#[test]
fn books_are_bitwise_the_interpreters_on_the_example_projects() {
    let mut r = Rng(31);
    let mut checked = 0;
    for car in cars::one_per_project(cars::cars("")) {
        let jit = compile(&car.model, &CodegenOptions::default()).expect("compiles");
        let Some(mut b) = Books::new(&car.model, &jit) else { continue };
        let [parts, power, loss, stored, exact] = b.counts;
        println!(
            "{}: {parts} parts, compiled: {power} powers, {loss} losses, {stored} of {exact} exact rates",
            car.name
        );
        // the books of the example cars compile whole
        assert_eq!((power, stored), (parts, exact), "{}", car.name);
        let p: Vec<f64> = car.model.flat.params.iter().map(|q| q.value).collect();
        let points = if car.sampled {
            let l = *jit.layout();
            let (mut y0, mut d0) = (vec![0.0; l.n_y()], vec![0.0; l.n_d]);
            jit.start(&p, &mut y0, &mut d0);
            (0..200)
                .map(|i| {
                    let y =
                        y0.iter().map(|x| x * r.range(0.5, 1.5) + r.range(-10.0, 10.0)).collect();
                    (i as f64 * 0.5, y, d0.clone())
                })
                .collect()
        } else {
            cars::trajectory(&car.model, &jit, 120.0)
        };
        for (t, y, d) in &points {
            let ydot = rates(&jit, *t, y, d, &p, &mut r);
            assert!(b.check(*t, y, d, &ydot, &p), "{}", car.name);
            checked += 1;
        }
    }
    assert!(checked > 500, "{checked} checks");
    println!("{checked} points of the example projects' books");
}

/// Adds `n` parts to `m`, each with a random power through one or two
/// ports and, mostly, a random loss and stored energy, reading what the
/// model's expressions read (its states, iteration variables, discrete
/// variables, assignments, parameters, tables, time); now and then a
/// derivative or a previous value (which the kernel declines).
fn with_books(m: &mut PreparedModel, n: usize, r: &mut Rng) {
    let mut l = random::Leaves { vars: vec![], params: vec![], tables: vec![], time: true };
    l.vars.extend(m.states.iter().copied());
    for s in &m.algebraics {
        if let lsim_ir::Slot::Var(v) = s {
            l.vars.push(*v);
        }
    }
    l.vars.extend(m.discretes.iter().copied());
    for a in &m.assignments {
        if let lsim_ir::Slot::Var(v) = a.target {
            l.vars.push(v);
        }
    }
    for k in 0..m.flat.params.len() {
        l.params.push(Expr::Param(lsim_ir::ParamId(k as u32)));
    }
    for (k, t) in m.flat.tables.iter().enumerate() {
        l.tables.push((k as u32, t.data.dims()));
    }
    let x0 = m.states[0];
    let e = |r: &mut Rng| {
        let x = random::expr(r, 4, &l, random::ALL_OPS);
        match r.below(40) {
            0 => Expr::bin(lsim_ir::BinaryOp::Add, x, Expr::Der(x0)),
            1 => Expr::bin(lsim_ir::BinaryOp::Mul, x, Expr::Pre(x0)),
            _ => x,
        }
    };
    for i in 0..n {
        let id = InstanceId(m.flat.instances.len() as u32);
        m.flat.instances.push(Instance {
            path: format!("part{i}"),
            def: "Part".into(),
            parent: None,
            label: None,
            ui_id: None,
        });
        for port in 0..1 + r.below(2) {
            let power = e(r);
            m.flat.port_powers.push(PortPower { instance: id, port: format!("p{port}"), power });
        }
        if r.below(5) > 0 {
            let stored = (r.below(5) > 0).then(|| e(r));
            let loss = (r.below(4) > 0).then(|| e(r));
            m.flat.energy.push(InstanceEnergy { instance: id, stored, loss });
        }
    }
}

/// A random point: ordinary values mostly, now and then a zero, a signed
/// zero, a large value, an infinity or a NaN.
fn value(r: &mut Rng) -> f64 {
    match r.below(30) {
        0 => 0.0,
        1 => -0.0,
        2 => f64::INFINITY,
        3 => f64::NAN,
        4 => r.range(-1e6, 1e6),
        5 => r.range(-1e-6, 1e-6),
        _ => r.range(-3.0, 3.0),
    }
}

#[test]
fn books_are_bitwise_the_interpreters_on_random_books() {
    let mut r = Rng(2027);
    let (mut checked, mut counts) = (0, [0; 4]);
    for _ in 0..80 {
        let mut m = random::implicit_model(&mut r, 3, 2, 10, 1, 4, random::ALL_OPS);
        with_books(&mut m, 6, &mut r);
        let jit = compile(&m, &CodegenOptions::default()).expect("compiles");
        let Some(mut b) = Books::new(&m, &jit) else { continue };
        for (c, k) in counts.iter_mut().zip(&b.counts[..4]) {
            *c += *k;
        }
        let l = *jit.layout();
        let p: Vec<f64> = m.flat.params.iter().map(|q| q.value).collect();
        let d0: Vec<f64> = m.discretes.iter().map(|v| m.flat.var(*v).start.unwrap()).collect();
        for _ in 0..40 {
            let y: Vec<f64> = (0..l.n_y()).map(|_| value(&mut r)).collect();
            let ydot: Vec<f64> = (0..l.n_y()).map(|_| value(&mut r)).collect();
            let t = [0.0, -0.0, r.range(-2.0, 5.0)][r.below(3)];
            assert!(b.check(t, &y, &d0, &ydot, &p));
            checked += 1;
        }
    }
    let [parts, power, loss, stored] = counts;
    println!(
        "{checked} random points; {parts} parts, compiled {power} powers, {loss} losses, {stored} rates"
    );
    assert!(checked > 2000, "{checked} checks");
    // most compile (a few read a derivative or a previous value)
    assert!(power * 10 > parts * 8 && stored > parts / 4, "{counts:?}");
}

/// Directed cases on a random model's variables: constant rates (time's,
/// its negation, `t - t`), signed zeros, powers at zero, `atan2` with one
/// rate constant, `min`/`max`/`limit` ties and NaNs as values and as
/// rates, a 2-D table read with one argument, and exact stored energies
/// the kernel declines (a table read with three arguments, `sin` with an
/// extra argument: their rates interpreted beside the compiled ones), at
/// points and rates full of zeros of both signs, infinities and NaNs.
#[test]
fn books_are_bitwise_the_interpreters_on_edge_cases() {
    use lsim_ir::{BinaryOp as B, Builtin as F, CmpOp};
    let mut r = Rng(77);
    let mut m = random::implicit_model(&mut r, 2, 1, 4, 1, 3, random::ALL_OPS);
    m.flat.port_powers.clear();
    m.flat.energy.clear();
    let v = |id: lsim_ir::VarId| Expr::Var(id);
    let (x, y) = (v(m.states[0]), v(m.states[1]));
    let z = match m.algebraics[0] {
        lsim_ir::Slot::Var(id) => v(id),
        lsim_ir::Slot::Der(id) => v(id),
    };
    let a = match m.assignments[2].target {
        lsim_ir::Slot::Var(id) => v(id),
        lsim_ir::Slot::Der(id) => v(id),
    };
    let p = Expr::Param(lsim_ir::ParamId(0));
    let t = Expr::Time;
    let c = Expr::Const;
    let bin = Expr::bin;
    let call = |f: F, args: Vec<Expr>| Expr::Call(f, args);
    let neg = |e: Expr| Expr::Neg(Box::new(e));
    let tab = |k: u32, args: Vec<Expr>| Expr::Table { table: k, args };
    // (the random model's tables: 1-D, 2-D, 1-D)
    let cases: Vec<Expr> = vec![
        bin(B::Mul, neg(t.clone()), x.clone()),
        bin(B::Div, x.clone(), neg(p.clone())),
        bin(B::Add, bin(B::Sub, t.clone(), t.clone()), x.clone()),
        bin(B::Mul, bin(B::Sub, t.clone(), t.clone()), x.clone()),
        bin(B::Pow, x.clone(), c(0.0)),
        bin(B::Pow, bin(B::Mul, c(0.0), x.clone()), y.clone()),
        bin(B::Pow, x.clone(), z.clone()),
        bin(B::Pow, x.clone(), t.clone()),
        call(F::Atan2, vec![x.clone(), bin(B::Mul, c(0.0), t.clone())]),
        call(F::Atan2, vec![bin(B::Mul, x.clone(), c(-0.0)), z.clone()]),
        call(F::Min, vec![x.clone(), z.clone()]),
        call(F::Max, vec![x.clone(), c(-0.0)]),
        call(F::Max, vec![neg(x.clone()), bin(B::Mul, c(0.0), y.clone())]),
        call(F::Limit, vec![x.clone(), c(-0.0), c(0.0)]),
        call(F::Limit, vec![x.clone(), z.clone(), a.clone()]),
        tab(0, vec![x.clone()]),
        tab(1, vec![x.clone()]),
        tab(1, vec![x.clone(), t.clone()]),
        bin(B::Mul, a.clone(), a.clone()),
        call(F::Sqrt, vec![x.clone()]),
        bin(B::Mul, call(F::Sign, vec![x.clone()]), x.clone()),
        Expr::If(
            Box::new(Expr::Compare(CmpOp::Gt, Box::new(x.clone()), Box::new(c(0.0)))),
            Box::new(bin(B::Mul, x.clone(), x.clone())),
            Box::new(neg(z.clone())),
        ),
        bin(B::Mul, call(F::Cos, vec![neg(x.clone())]), neg(y.clone())),
        // declined, exact: interpreted beside the kernel
        tab(0, vec![x.clone(), y.clone(), z.clone()]),
        // (an extra argument, which the rules do not read)
        call(F::Sin, vec![x.clone(), a.clone()]),
    ];
    for (i, e) in cases.iter().enumerate() {
        let id = InstanceId(m.flat.instances.len() as u32);
        m.flat.instances.push(Instance {
            path: format!("edge{i}"),
            def: "Part".into(),
            parent: None,
            label: None,
            ui_id: None,
        });
        // (the declined ones only as stored energies: `eval` stops at a
        // missing argument)
        let ok = cases.len() - 2;
        let power = cases[(i + 1) % ok].clone();
        m.flat.port_powers.push(PortPower { instance: id, port: "p".into(), power });
        let loss = Some(cases[(i + 2) % ok].clone());
        m.flat.energy.push(InstanceEnergy { instance: id, stored: Some(e.clone()), loss });
    }
    let jit = compile(&m, &CodegenOptions::default()).expect("compiles");
    let mut b = Books::new(&m, &jit).expect("books");
    let [parts, _, _, stored, _] = b.counts;
    assert!(stored >= parts - 2 && stored < parts, "{:?}", b.counts);
    let l = *jit.layout();
    let prm: Vec<f64> = m.flat.params.iter().map(|q| q.value).collect();
    let d0: Vec<f64> = m.discretes.iter().map(|w| m.flat.var(*w).start.unwrap()).collect();
    let special = [0.0, -0.0, 1.0, -1.0, f64::INFINITY, f64::NEG_INFINITY, f64::NAN, 2.5];
    for k in 0..4000 {
        let pick = |r: &mut Rng| {
            if r.below(2) == 0 { special[r.below(special.len())] } else { r.range(-3.0, 3.0) }
        };
        let yv: Vec<f64> = (0..l.n_y()).map(|_| pick(&mut r)).collect();
        let ydot: Vec<f64> = (0..l.n_y()).map(|_| pick(&mut r)).collect();
        let t = [0.0, -0.0, 1.0, r.range(-2.0, 5.0)][k % 4];
        assert!(b.check(t, &yv, &d0, &ydot, &prm));
    }
}

/// The model keeps one kernel per spec, compared to the bit: specs that
/// differ only in a zero's sign get their own, and a spec with a NaN
/// constant finds its kernel again.
#[test]
fn compiled_books_are_kept_per_spec_to_the_bit() {
    let mut r = Rng(5);
    let mut m = random::implicit_model(&mut r, 2, 0, 3, 1, 2, random::ALL_OPS);
    with_books(&mut m, 2, &mut r);
    let jit = compile(&m, &CodegenOptions::default()).expect("compiles");
    let info = RunInfo::from_prepared(&m);
    let energy = info.energy.clone().expect("books");
    let mut spec = Integrand::new(&energy, &info, jit.layout()).spec();
    let x = Expr::Var(m.states[0]);
    let with = |spec: &mut BooksSpec, k: f64| {
        spec.parts[0].power = Expr::bin(lsim_ir::BinaryOp::Div, x.clone(), Expr::Const(k));
        spec.clone()
    };
    let (plus, minus, nan) =
        (with(&mut spec, 0.0), with(&mut spec, -0.0), with(&mut spec, f64::NAN));
    let kp = jit.compile_books(&plus).expect("compiles");
    let km = jit.compile_books(&minus).expect("compiles");
    assert!(!Arc::ptr_eq(&kp, &km));
    assert!(Arc::ptr_eq(&kp, &jit.compile_books(&plus).unwrap()));
    let kn = jit.compile_books(&nan).expect("compiles");
    assert!(Arc::ptr_eq(&kn, &jit.compile_books(&nan).unwrap()));
}
