//! Compile times and per-call costs of the generated code on the example
//! projects' cases (`backend/projects/*.json`), and where a run's time
//! goes:
//!
//! ```sh
//! cargo run --release -p lsim-codegen --example cars_bench [filter] [--run SECONDS]
//! ```
//!
//! `--run` also simulates each case without sampled blocks for that long
//! and reports the time spent in each model function.

#[path = "../tests/common/cars.rs"]
mod cars;

use lsim_codegen::{CodegenOptions, JitModel, compile};
use lsim_ir::runtime::{
    EvalInput, InitFunctions, Layout, ModelFunctions, SparsityPattern, TableGuard,
};
use lsim_solve::{OutputGrid, RunInfo, SolverOptions};
use std::hint::black_box;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

fn per_call(mut f: impl FnMut()) -> f64 {
    // calibrate to ~10 ms, best of 7
    let mut n = 1usize;
    loop {
        let t = Instant::now();
        for _ in 0..n {
            f();
        }
        if t.elapsed().as_secs_f64() > 0.01 {
            break;
        }
        n *= 2;
    }
    let mut best = f64::INFINITY;
    for _ in 0..7 {
        let t = Instant::now();
        for _ in 0..n {
            f();
        }
        best = best.min(t.elapsed().as_secs_f64() / n as f64);
    }
    best
}

/// A consistent-ish point: the start vector, through the initialisation's
/// `finish` at its guesses when there is one.
fn start_point(j: &JitModel, p: &[f64]) -> (Vec<f64>, Vec<f64>) {
    let l = *j.layout();
    let mut y0 = vec![0.0; l.n_y()];
    let mut d0 = vec![0.0; l.n_d];
    j.start(p, &mut y0, &mut d0);
    if let Some(init) = j.init() {
        let mut w = vec![0.0; init.n_w()];
        init.guess(p, &mut w);
        let mut work = vec![0.0; l.n_work];
        let u = vec![0.0; l.n_u];
        let inp = EvalInput { t: 0.0, y: &w, p, d: &d0, u: &u };
        init.finish(&inp, &mut work, &mut y0);
    }
    (y0, d0)
}

/// Delegates to a model and times each function.
struct Timed<'a> {
    m: &'a JitModel,
    ns: [AtomicU64; 8],
    calls: [AtomicU64; 8],
    /// the last call's point (t, y, d bits) and function, and how often a
    /// call came at the same point as the previous one: [same function,
    /// another function]
    last: std::sync::Mutex<(Vec<u64>, usize)>,
    repeats: [AtomicU64; 2],
    /// [previous][this]: calls at the previous call's point
    pairs: [[AtomicU64; 8]; 8],
}

const NAMES: [&str; 8] = [
    "residual",
    "jvp",
    "jacobian_sparse",
    "jacobian_dense",
    "roots",
    "vars",
    "when+modes",
    "guards",
];

impl Timed<'_> {
    fn note(&self, k: usize, inp: &EvalInput<'_>) {
        let key: Vec<u64> = std::iter::once(inp.t.to_bits())
            .chain(inp.y.iter().map(|x| x.to_bits()))
            .chain(inp.d.iter().map(|x| x.to_bits()))
            .collect();
        let mut last = self.last.lock().unwrap();
        if last.0 == key {
            self.repeats[if last.1 == k { 0 } else { 1 }].fetch_add(1, Ordering::Relaxed);
            self.pairs[last.1][k].fetch_add(1, Ordering::Relaxed);
        }
        *last = (key, k);
    }

    fn time<R>(&self, k: usize, f: impl FnOnce() -> R) -> R {
        let t = Instant::now();
        let r = f();
        self.ns[k].fetch_add(t.elapsed().as_nanos() as u64, Ordering::Relaxed);
        self.calls[k].fetch_add(1, Ordering::Relaxed);
        r
    }
}

impl ModelFunctions for Timed<'_> {
    fn layout(&self) -> &Layout {
        self.m.layout()
    }
    fn residual(&self, inp: &EvalInput<'_>, work: &mut [f64], out: &mut [f64]) {
        self.note(0, inp);
        self.time(0, || self.m.residual(inp, work, out))
    }
    fn jvp(&self, inp: &EvalInput<'_>, v: &[f64], work: &mut [f64], out: &mut [f64]) {
        self.time(1, || self.m.jvp(inp, v, work, out))
    }
    fn roots(&self, inp: &EvalInput<'_>, work: &mut [f64], out: &mut [f64]) {
        self.note(4, inp);
        self.time(4, || self.m.roots(inp, work, out))
    }
    fn vars(&self, inp: &EvalInput<'_>, work: &mut [f64], out: &mut [f64]) {
        self.note(5, inp);
        self.time(5, || self.m.vars(inp, work, out))
    }
    fn when(&self, inp: &EvalInput<'_>, fired: &[f64], work: &mut [f64], d_out: &mut [f64]) {
        self.time(6, || self.m.when(inp, fired, work, d_out))
    }
    fn start(&self, p: &[f64], y0: &mut [f64], d0: &mut [f64]) {
        self.m.start(p, y0, d0)
    }
    fn jacobian_dense(&self, inp: &EvalInput<'_>, work: &mut [f64], out: &mut [f64]) {
        self.time(3, || self.m.jacobian_dense(inp, work, out))
    }
    fn sparsity(&self) -> Option<&SparsityPattern> {
        self.m.sparsity()
    }
    fn jacobian_sparse(&self, inp: &EvalInput<'_>, work: &mut [f64], values: &mut [f64]) {
        self.note(2, inp);
        self.time(2, || ModelFunctions::jacobian_sparse(self.m, inp, work, values))
    }
    fn modes(&self, inp: &EvalInput<'_>, work: &mut [f64], d_out: &mut [f64]) {
        self.time(6, || self.m.modes(inp, work, d_out))
    }
    fn init(&self) -> Option<&dyn InitFunctions> {
        self.m.init()
    }
    fn table_guard_list(&self) -> &[TableGuard] {
        self.m.table_guard_list()
    }
    fn table_guards(&self, inp: &EvalInput<'_>, work: &mut [f64], out: &mut [f64]) {
        self.note(7, inp);
        self.time(7, || self.m.table_guards(inp, work, out))
    }
    fn eval_table(&self, k: u32, args: [f64; 2]) -> Option<(f64, [f64; 2])> {
        self.m.eval_table(k, args)
    }
    fn table_axes(&self, k: u32) -> Option<[Vec<f64>; 2]> {
        self.m.table_axes(k)
    }
    fn condition_kernels(&self) -> Option<&dyn lsim_ir::ConditionKernels> {
        self.m.condition_kernels()
    }
}

/// Counts of what a model's assignments compute (`STATS=1`).
fn stats(m: &lsim_ir::PreparedModel) {
    use lsim_ir::expr::{BinaryOp, Expr};
    use std::collections::BTreeMap;
    let mut c: BTreeMap<String, usize> = BTreeMap::new();
    let mut count = |e: &Expr| {
        e.walk(&mut |x| {
            let k = match x {
                Expr::Binary(BinaryOp::Pow, _, b) => match **b {
                    Expr::Const(n) => format!("pow {n}"),
                    _ => "pow var".into(),
                },
                Expr::Binary(op, ..) => format!("{op:?}"),
                Expr::Call(f, _) => f.name().to_string(),
                Expr::Table { args, .. } => format!("table{}", args.len()),
                Expr::If(..) => "if".into(),
                Expr::Compare(..) => "compare".into(),
                _ => return,
            };
            *c.entry(k).or_default() += 1;
        })
    };
    for a in &m.assignments {
        count(&a.expr);
    }
    for r in &m.residuals {
        count(&r.expr);
    }
    let mut zc = 0;
    for z in &m.zero_crossings {
        z.expr.walk(&mut |_| zc += 1);
    }
    println!("  ops: {c:?}; zero-crossing nodes {zc}");
    let info = RunInfo::from_prepared(m);
    for (k, f) in info.time_functions.iter().enumerate() {
        match f {
            Some(lsim_solve::TimeFunction::Mixed { chain, g }) => {
                let nodes: usize = chain.iter().map(|(_, e)| e.size()).sum::<usize>() + g.size();
                let ifs: usize = chain
                    .iter()
                    .map(|(_, e)| {
                        let mut n = 0;
                        e.walk(&mut |x| n += matches!(x, Expr::If(..)) as usize);
                        n
                    })
                    .sum();
                let tables: usize = chain
                    .iter()
                    .map(|(_, e)| {
                        let mut n = 0;
                        e.walk(&mut |x| n += matches!(x, Expr::Table { .. }) as usize);
                        n
                    })
                    .sum();
                println!(
                    "  mixed condition {k} '{}': chain of {} steps, {nodes} nodes, {ifs} ifs, {tables} tables",
                    m.zero_crossings[k].origin.label.clone().unwrap_or_default(),
                    chain.len()
                );
                if std::env::var_os("SHOW").is_some() {
                    println!("    g = {g}");
                    for (v, e) in chain {
                        println!("    {} = {e}", m.flat.vars[*v].name);
                    }
                }
            }
            Some(lsim_solve::TimeFunction::Pure(_)) => println!("  pure time function {k}"),
            Some(lsim_solve::TimeFunction::Unhandled) => println!("  unhandled {k}"),
            None => {}
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let run_for: Option<f64> =
        args.iter().position(|a| a == "--run").and_then(|i| args.get(i + 1)?.parse().ok());
    let filter = args.first().filter(|a| !a.starts_with("--")).cloned().unwrap_or_default();
    let reps: usize = std::env::var("REPS").ok().and_then(|s| s.parse().ok()).unwrap_or(7);
    let leak = |s: String| -> &'static str { Box::leak(s.into_boxed_str()) };
    let mut opts = CodegenOptions::default();
    if let Ok(o) = std::env::var("OPT") {
        opts.opt_level = leak(o);
    }
    if let Ok(r) = std::env::var("REGALLOC") {
        opts.regalloc = leak(r);
    }
    if let Some(t) = std::env::var("THREADS").ok().and_then(|t| t.parse().ok()) {
        opts.threads = t;
    }
    for car in cars::cars(&filter) {
        let m = &car.model;
        let mut best = f64::INFINITY;
        let mut jit = None;
        for _ in 0..reps {
            let t = Instant::now();
            let j = compile(m, &opts).expect("compiles");
            best = best.min(t.elapsed().as_secs_f64());
            jit = Some(j);
        }
        let j = jit.unwrap();
        let r = &j.report;
        let l = *j.layout();
        println!(
            "== {}: {} states, {} iteration variables, {} assignments ({} nodes), {} discretes, {} roots, {} whens, {} modes, {} tables, {} vars",
            car.name,
            l.n_x,
            l.n_z,
            m.assignments.len(),
            r.nodes,
            l.n_d,
            l.n_roots,
            l.n_whens,
            m.modes.len(),
            m.flat.tables.len(),
            l.n_vars
        );
        println!(
            "  compile {:.2} ms best of {reps} (analysis {:.2}, IR {:.2}, codegen {:.2}; {} functions, {} instructions, {} taped operations, {} kB, {} threads, opt {}, {}); Jacobian {} nnz, {} colours",
            best * 1e3,
            r.analysis_seconds * 1e3,
            r.ir_seconds * 1e3,
            r.codegen_seconds * 1e3,
            r.functions,
            r.instructions,
            r.tape_ops,
            r.code_bytes / 1024,
            r.threads,
            r.opt_level,
            r.regalloc,
            r.jac_nnz,
            r.jac_colours
        );
        if std::env::var_os("STATS").is_some() {
            stats(m);
        }
        let p: Vec<f64> = m.flat.params.iter().map(|q| q.value).collect();
        let (y0, d0) = start_point(&j, &p);
        let u = vec![0.0; l.n_u];
        let inp = EvalInput { t: 1.0, y: &y0, p: &p, d: &d0, u: &u };
        let mut work = vec![0.0; l.n_work];
        let mut out = vec![0.0; l.n_y()];
        let v: Vec<f64> = (0..l.n_y()).map(|i| 1.0 / (1.0 + i as f64)).collect();
        let mut vals = vec![0.0; j.pattern().nnz()];
        let mut vars = vec![0.0; l.n_vars];
        let mut roots = vec![0.0; l.n_roots];
        let res = per_call(|| j.residual(black_box(&inp), &mut work, &mut out));
        let jv = per_call(|| j.jvp(black_box(&inp), &v, &mut work, &mut out));
        let js = per_call(|| j.jacobian_sparse(black_box(&inp), &mut work, &mut vals));
        let va = per_call(|| j.vars(black_box(&inp), &mut work, &mut vars));
        let ro = per_call(|| j.roots(black_box(&inp), &mut work, &mut roots));
        println!(
            "  per call: residual {:.0} ns, jvp {:.0} ns, sparse Jacobian {:.0} ns (= {:.1} residuals), vars {:.0} ns, roots {:.0} ns",
            res * 1e9,
            jv * 1e9,
            js * 1e9,
            js / res,
            va * 1e9,
            ro * 1e9
        );
        if let Some(t_end) = run_for {
            if car.sampled {
                println!("  (run skipped: sampled blocks need their host)");
                continue;
            }
            let timed = Timed {
                m: &j,
                ns: Default::default(),
                calls: Default::default(),
                last: Default::default(),
                repeats: Default::default(),
                pairs: Default::default(),
            };
            let info = RunInfo::from_prepared(m);
            let so = SolverOptions { rtol: 1e-6, atol: 1e-8, ..Default::default() };
            let t = Instant::now();
            let res = lsim_solve::simulate(
                &timed,
                &info,
                &so,
                OutputGrid { t0: 0.0, t_end, dt: 1.0 },
                &mut [],
            );
            let wall = t.elapsed().as_secs_f64();
            match res {
                Err(e) => println!("  run failed: {e}"),
                Ok(r) => {
                    let mut line = format!(
                        "  run {t_end} s: {:.1} ms ({:.0}× real time), {} steps;",
                        wall * 1e3,
                        t_end / wall,
                        r.stats.steps
                    );
                    let mut inside = 0.0;
                    for (k, name) in NAMES.iter().enumerate() {
                        let (ns, n) = (
                            timed.ns[k].load(Ordering::Relaxed),
                            timed.calls[k].load(Ordering::Relaxed),
                        );
                        if n > 0 {
                            inside += ns as f64 * 1e-9;
                            line += &format!(
                                " {} {}× {:.0} ns ({:.1} %),",
                                name,
                                n,
                                ns as f64 / n as f64,
                                100.0 * ns as f64 * 1e-9 / wall
                            );
                        }
                    }
                    line += &format!(" model functions {:.1} %", 100.0 * inside / wall);
                    line += &format!(
                        "; calls at the previous call's point: {} of the same function, {} of another",
                        timed.repeats[0].load(Ordering::Relaxed),
                        timed.repeats[1].load(Ordering::Relaxed)
                    );
                    for (a, row) in timed.pairs.iter().enumerate() {
                        for (b, n) in row.iter().enumerate() {
                            let n = n.load(Ordering::Relaxed);
                            if n > 0 {
                                line += &format!("\n    {} then {}: {n}", NAMES[a], NAMES[b]);
                            }
                        }
                    }
                    println!("{line}");
                }
            }
        }
    }
}
