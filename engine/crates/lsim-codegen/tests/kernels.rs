//! The compiled condition kernels against the interpreters (DESIGN.md
//! §5.8): on every condition of the example projects that the run loop
//! checks along its steps (at the operating points of a simulated drive
//! cycle) and on random models' conditions, at sampled points, intervals
//! and boxes, `point` is bitwise `lsim_ir::eval` of the condition's chain
//! and the compiled `roots` output bitwise `point`; `enclose` is bitwise
//! `lsim_ir::interval::enclose`, and its enclosure holds the point
//! values.

#[path = "common/cars.rs"]
mod cars;
#[path = "common/random.rs"]
mod random;
#[path = "common/synth.rs"]
mod synth;

use lsim_codegen::{CodegenOptions, JitModel, compile};
use lsim_ir::interval::{self, Cx, Grid2, Iv, J2};
use lsim_ir::runtime::{Enclosure, EvalInput, ModelFunctions};
use lsim_ir::{Expr, ParamId, PreparedModel, VarId};
use lsim_solve::{RunInfo, TimeFunction, VarSource};
use synth::Rng;

/// Equal bit for bit (NaNs alike).
fn same(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
}

fn same_enc(a: &Enclosure, b: &J2) -> bool {
    same(a.v[0], b.v.lo)
        && same(a.v[1], b.v.hi)
        && same(a.d[0], b.d.lo)
        && same(a.d[1], b.d.hi)
        && same(a.dd[0], b.dd.lo)
        && same(a.dd[1], b.dd.hi)
}

fn enc(j: J2) -> Enclosure {
    Enclosure { v: [j.v.lo, j.v.hi], d: [j.d.lo, j.d.hi], dd: [j.dd.lo, j.dd.hi] }
}

/// The interpreter's point value of a mixed condition (the run loop's
/// `Mixed::point`): its chain by `lsim_ir::eval`, the leaves from y (with
/// their signs), the variables constant between events from their
/// channels.
struct PointEnv<'a> {
    t: f64,
    chain: &'a std::collections::HashMap<usize, f64>,
    y: &'a [f64],
    channels: &'a [f64],
    sources: &'a [VarSource],
    params: &'a [f64],
    model: &'a dyn ModelFunctions,
}

impl lsim_ir::eval::Env for PointEnv<'_> {
    fn time(&self) -> f64 {
        self.t
    }
    fn var(&self, v: VarId) -> f64 {
        let k = v.0 as usize;
        if let Some(x) = self.chain.get(&k) {
            return *x;
        }
        // (the run loop's `s * ys[p]`, the sign ±1, to the bit)
        #[allow(clippy::neg_multiply, clippy::identity_op)]
        match self.sources.get(k) {
            Some(VarSource::Y(i)) => 1.0 * self.y[*i],
            Some(VarSource::NegY(i)) => -1.0 * self.y[*i],
            _ => self.channels.get(k).copied().unwrap_or(f64::NAN),
        }
    }
    fn der(&self, _: VarId) -> f64 {
        f64::NAN
    }
    fn param(&self, p: ParamId) -> f64 {
        self.params[p.0 as usize]
    }
    fn table(&self, k: u32, args: &[f64]) -> f64 {
        interval::table_at(self.model, k, args).map_or(f64::NAN, |(v, _)| v)
    }
}

/// The channels of the variables constant between events.
fn channels(sources: &[VarSource], d: &[f64], u: &[f64]) -> Vec<f64> {
    sources
        .iter()
        .map(|s| match *s {
            VarSource::D(i) => d[i],
            VarSource::NegD(i) => -d[i],
            VarSource::U(i) => u[i],
            VarSource::Const(c) => c,
            _ => f64::NAN,
        })
        .collect()
}

/// The tables' breakpoints and grids as the run loop builds them from a
/// model's tables.
fn tools(m: &JitModel, n: usize) -> (Vec<Vec<f64>>, Vec<Option<Grid2>>) {
    let mut breaks = vec![];
    let mut grids = vec![];
    for k in 0..n {
        let [x, y] = m.table_axes(k as u32).expect("the model gives its tables");
        if y.is_empty() {
            breaks.push(x);
            grids.push(None);
        } else {
            breaks.push(vec![]);
            grids.push(Grid2::new(k as u32, &x, &y));
        }
    }
    (breaks, grids)
}

/// A mixed condition: its zero crossing, its chain, the condition.
type Condition = (usize, Vec<(usize, Expr)>, Expr);

/// One model's conditions, checked at `samples` (y, d) operating points.
struct Checker<'a> {
    m: &'a PreparedModel,
    jit: JitModel,
    info: RunInfo,
    breaks: Vec<Vec<f64>>,
    grids: Vec<Option<Grid2>>,
}

impl<'a> Checker<'a> {
    fn new(m: &'a PreparedModel) -> Self {
        let jit = compile(m, &CodegenOptions::default()).expect("compiles");
        let info = RunInfo::from_prepared(m);
        let (breaks, grids) = tools(&jit, m.flat.tables.len());
        Checker { m, jit, info, breaks, grids }
    }

    /// The mixed conditions: (zero crossing, chain, condition).
    fn mixed(&self) -> Vec<Condition> {
        self.info
            .time_functions
            .iter()
            .enumerate()
            .filter_map(|(k, f)| match f {
                Some(TimeFunction::Mixed { chain, g }) => Some((k, chain.clone(), g.clone())),
                _ => None,
            })
            .collect()
    }

    /// Checks condition `k` at (`y`, `d`) and over `t` with the leaves'
    /// enclosures `leaf` (by entry of y). Returns false when it found a
    /// difference (and says what).
    #[allow(clippy::too_many_arguments)]
    fn check(
        &self,
        k: usize,
        chain: &[(usize, Expr)],
        g: &Expr,
        y: &[f64],
        d: &[f64],
        t: [f64; 2],
        leaf: &[J2],
    ) -> bool {
        let kern = self.jit.condition_kernels().expect("the model has kernels");
        assert!(kern.covers(k), "condition {k} is not covered");
        let p: Vec<f64> = self.m.flat.params.iter().map(|q| q.value).collect();
        let u = vec![0.0; self.m.inputs.len()];
        let sources = &self.info.var_sources;
        let ch = channels(sources, d, &u);
        // the point: the interpreter, the kernel, the compiled roots
        let mut vals = std::collections::HashMap::new();
        for (v, e) in chain {
            let env = PointEnv {
                t: t[0],
                chain: &vals,
                y,
                channels: &ch,
                sources,
                params: &p,
                model: &self.jit,
            };
            let x = lsim_ir::eval::eval(e, &env);
            vals.insert(*v, x);
        }
        let env = PointEnv {
            t: t[0],
            chain: &vals,
            y,
            channels: &ch,
            sources,
            params: &p,
            model: &self.jit,
        };
        let want = lsim_ir::eval::eval(g, &env);
        let inp = EvalInput { t: t[0], y, p: &p, d, u: &u };
        let (nw, ne) = kern.scratch();
        let mut work = vec![f64::NAN; nw];
        let got = kern.point(k, &inp, &mut work);
        let mut roots = vec![0.0; self.jit.layout().n_roots];
        let mut rw = vec![0.0; self.jit.layout().n_work];
        self.jit.roots(&inp, &mut rw, &mut roots);
        if !same(got, want) || !same(roots[k], want) {
            eprintln!(
                "condition {k} at t = {}: point {got:e}, roots {:e}, interpreter {want:e}",
                t[0], roots[k]
            );
            return false;
        }
        // the enclosure: the interpreter (the run loop's `Mixed::enclose`),
        // the kernel
        let ti = Iv { lo: t[0], hi: t[1] };
        let mut steps: std::collections::HashMap<usize, J2> = Default::default();
        let lookup = |v: usize, steps: &std::collections::HashMap<usize, J2>| -> Option<J2> {
            if let Some(j) = steps.get(&v) {
                return Some(*j);
            }
            match sources.get(v) {
                Some(VarSource::Y(i)) => Some(leaf[*i]),
                Some(VarSource::NegY(i)) => Some(leaf[*i].neg()),
                _ => None,
            }
        };
        for (v, e) in chain {
            let l = |w: usize| lookup(w, &steps);
            let cx = Cx {
                params: &p,
                vars: &ch,
                model: &self.jit,
                breaks: &self.breaks,
                grids: &self.grids,
                leaf: &l,
            };
            let j = interval::enclose(e, &cx, ti);
            steps.insert(*v, j);
        }
        let l = |w: usize| lookup(w, &steps);
        let cx = Cx {
            params: &p,
            vars: &ch,
            model: &self.jit,
            breaks: &self.breaks,
            grids: &self.grids,
            leaf: &l,
        };
        let want = interval::enclose(g, &cx, ti);
        // the enclosure holds the point value (the leaves hold y, the
        // interval holds t[0])
        // (where every step is defined: the interval rules enclose real
        // functions, and a NaN a comparison absorbs has no enclosure)
        let x = got;
        let defined = vals.values().all(|v| v.is_finite());
        if defined && x.is_finite() && !(want.v.lo <= x && x <= want.v.hi) {
            eprintln!("condition {k}: the point value {x:e} is outside {:?}", want.v);
            eprintln!("  g = {g}");
            for (v, e) in chain {
                eprintln!("  v[{v}] = {e}: point {:e}, enclosure {:?}", vals[v], steps[v]);
            }
            eprintln!("  y = {y:?}, t = {t:?}");
            for (i, l) in leaf.iter().enumerate() {
                eprintln!("  leaf {i}: {l:?}");
            }
            return false;
        }
        let yenc: Vec<Enclosure> = leaf.iter().map(|j| enc(*j)).collect();
        let mut ework = vec![enc(J2::all()); ne];
        // twice: the second from the steps kept in the scratch
        for pass in 0..2 {
            let got = kern.enclose(k, t, &yenc, d, &p, &u, &mut ework);
            if !same_enc(&got, &want) {
                eprintln!(
                    "condition {k} over [{}, {}] (pass {pass}): kernel {got:?}, interpreter {want:?}",
                    t[0], t[1]
                );
                return false;
            }
        }
        true
    }
}

/// A leaf's enclosure around `x`: a point with rates, an interval with
/// rates, or a box (rates unknown).
fn leaf_around(r: &mut Rng, x: f64) -> J2 {
    let scale = x.abs().max(1.0);
    let w = [0.0, 1e-9, 1e-4, 1e-2, 0.3][r.below(5)] * scale;
    let lo = x - r.unit() * w;
    let v = Iv::new(lo, lo + w);
    match r.below(3) {
        0 => J2::jumps(v),
        _ => {
            let d0 = r.range(-2.0, 2.0) * scale;
            let dw = r.unit() * scale;
            let dd0 = r.range(-5.0, 5.0) * scale;
            J2 { v, d: Iv::new(d0, d0 + dw), dd: Iv::new(dd0, dd0 + r.unit() * scale) }
        }
    }
}

#[test]
fn kernels_are_bitwise_the_interpreters_on_the_example_projects() {
    let mut r = Rng(17);
    let mut checked = 0;
    let mut seen = std::collections::HashSet::new();
    for car in cars::cars("") {
        // one case per project (the cases of a project share its model)
        if !seen.insert(car.name.split('/').next().unwrap().to_string()) {
            continue;
        }
        let c = Checker::new(&car.model);
        let mixed = c.mixed();
        if mixed.is_empty() {
            continue;
        }
        // the operating points of a drive (a model with sampled blocks,
        // which need their host: its start values, moved about)
        let points = if car.sampled {
            let l = *c.jit.layout();
            let p: Vec<f64> = car.model.flat.params.iter().map(|q| q.value).collect();
            let (mut y0, mut d0) = (vec![0.0; l.n_y()], vec![0.0; l.n_d]);
            c.jit.start(&p, &mut y0, &mut d0);
            (0..200)
                .map(|i| {
                    let y =
                        y0.iter().map(|x| x * r.range(0.5, 1.5) + r.range(-10.0, 10.0)).collect();
                    (i as f64 * 0.5, y, d0.clone())
                })
                .collect()
        } else {
            cars::trajectory(&car.model, &c.jit, 120.0)
        };
        for (t, y, d) in &points {
            for (k, chain, g) in &mixed {
                for _ in 0..4 {
                    let h = [0.0, 1e-6, 1e-3, 0.1, 1.0, 10.0][r.below(6)];
                    let leaf: Vec<J2> = y.iter().map(|x| leaf_around(&mut r, *x)).collect();
                    assert!(c.check(*k, chain, g, y, d, [*t, t + h], &leaf), "{}", car.name);
                    checked += 1;
                }
            }
        }
    }
    assert!(checked > 1000, "{checked} checks");
    println!("{checked} condition checks on the example projects");
}

#[test]
fn kernels_are_bitwise_the_interpreters_on_random_conditions() {
    let mut r = Rng(4242);
    let mut checked = 0;
    for _ in 0..60 {
        let m = random::model(&mut r, 3, 12, 4, 4, random::ALL_OPS);
        let c = Checker::new(&m);
        let n_y = m.states.len();
        let d0: Vec<f64> = m.discretes.iter().map(|v| m.flat.var(*v).start.unwrap()).collect();
        for (k, chain, g) in c.mixed() {
            for _ in 0..40 {
                let y: Vec<f64> = (0..n_y).map(|_| r.range(-3.0, 3.0)).collect();
                let t0 = r.range(-2.0, 5.0);
                let h = [0.0, 1e-6, 1e-2, 0.5, 3.0][r.below(5)];
                let leaf: Vec<J2> = y.iter().map(|x| leaf_around(&mut r, *x)).collect();
                assert!(c.check(k, &chain, &g, &y, &d0, [t0, t0 + h], &leaf));
                checked += 1;
            }
        }
    }
    assert!(checked > 2000, "{checked} checks");
    println!("{checked} random condition checks");
}
