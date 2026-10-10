//! Conditions that read time and continuous variables
//! ([`crate::TimeFunction::Mixed`]: `x > sin(ω time)`, a driver's command
//! from a driving cycle's target against the car's speed, a motor's
//! limits from its maps at that command): no step may hold a sign change
//! root finding cannot see, two inside one step (a pulse), as root finding
//! sees a condition only by its sign at a step's ends. A driving cycle's
//! conditions too: the stops at the cycle's breakpoints keep its target
//! linear within a step, but the state it is compared with may curve.
//!
//! Both checks read the integrator's dense output over the step as the
//! polynomial it interpolates with ([`crate::DenseOutput`]: SUNDIALS'
//! Nordsieck array or divided differences as it holds them; diffsol's
//! interpolant through six Chebyshev points, exact for its order up to a
//! round-off carried as an error), so no pulse can hide between samples.
//! Two checks, the cheap one first:
//!
//! * **A certificate.** The condition keeps its sign over a time window
//!   and a box of the states it reads: its interval enclosure there (the
//!   time terms exactly, a table by its pieces) excludes zero. A step
//!   inside the window over which each of those states stays inside the
//!   box needs no more. Where a state goes over the step is bounded first
//!   by how far it may stray from its polynomial's value at the step's end
//!   (the sizes of the basis functions over the step: a few products per
//!   state, for all the conditions), and, where that is not enough, by its
//!   range (the basis functions' ranges by interval products); both
//!   rounded outwards. The certificate is made again, around the state at
//!   a step's end, when a step leaves it (its window grows while it holds,
//!   and shrinks when the condition is near zero; after a failure the next
//!   attempt waits a few steps, longer each time, to 16).
//! * **The step along the dense output**, where no certificate holds: the
//!   states and iteration variables the condition reads, as that
//!   polynomial (in powers of the time, its coefficients intervals rounded
//!   outwards), enclose the condition over any part of the step, and
//!   [`first_change`] finds its first sign change (the whole step's
//!   enclosure first, which settles most). One root finding did not
//!   report ends the step, as a root does, where the model's own root
//!   function on the dense output is on the new side. The crossing root
//!   finding located at the step's end is not one: it locates a root to
//!   its tolerance (100 ε (|t| + h)), a little after the change found
//!   here, which is then the only change up to the step's end.
//!
//! The condition is evaluated through the chain of the assignments it
//! reads (a shared variable once), not expanded into one expression; the
//! steps of the chain that do not move along a step are enclosed once
//! while the values constant between events keep theirs.
//!
//! A condition whose value moves only where a `noEvent` comparison flips
//! (built of values constant between events, of such comparisons and of
//! functions of these: a motor's "running" flag, `command ≠ 0`) is left to
//! root finding, as `noEvent` asks: no event needs locating where such a
//! comparison flips, and a model's comparisons that are events are modes,
//! constant along a step.

use super::{Loop, same_instant};
use crate::info::{VarSource, table_at};
use crate::interval::{
    Basis, Cx, DensePoly, Found, Grid2, Iv, J2, basis_sizes, enclose, first_change, stray,
    supported,
};
use crate::{DenseOutput, Integrator, SolveError};
use lsim_ir::runtime::ModelFunctions;
use lsim_ir::{Expr, ParamId, VarId};
use std::cell::RefCell;

/// The steps one search may take.
const BUDGET: usize = 4000;

/// What a channel is to a mixed condition.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Slot {
    /// constant between events: its channel's value
    Const,
    /// computed: its step in the chain
    Chain(u32),
    /// an entry of y (its position among the condition's), with a sign
    Leaf(u32, f64),
}

/// A certificate: the condition keeps its sign from `from` until `until`
/// while the states it reads stay in `[lo, hi]` (and the channels constant
/// between events keep the values `consts`).
#[derive(Clone, Debug)]
struct Cert {
    from: f64,
    until: f64,
    lo: Vec<f64>,
    hi: Vec<f64>,
    consts: Vec<f64>,
}

/// A mixed condition.
pub(super) struct Mixed<'a> {
    /// the root functions that are it
    pub(super) roots: Vec<usize>,
    g: &'a Expr,
    chain: &'a [(usize, Expr)],
    /// per channel
    slot: Vec<Slot>,
    /// the entries of y it reads
    y_idx: Vec<usize>,
    /// the channels it reads that are constant between events
    consts: Vec<usize>,
    /// per step of the chain: whether it moves along a step (reads time
    /// or an entry of y); the others' enclosures kept while the channels
    /// constant between events keep their values (their bits)
    moving: Vec<bool>,
    fixed: RefCell<(Vec<u64>, Vec<J2>)>,
    cert: Option<Cert>,
    /// the next certificate's window
    window: f64,
    /// certificates in a row that could not be made, and the steps left
    /// before the next is tried (each failure doubles the wait, to 16
    /// steps: the steps are checked along the dense output meanwhile)
    fails: u32,
    wait: u32,
    warned: bool,
    /// steps a certificate cleared, steps taken along the dense output
    pub(super) certified: u64,
    pub(super) scanned: u64,
}

/// Where a condition's constants come from.
struct Base<'a> {
    params: &'a [f64],
    vars: &'a [f64],
    model: &'a dyn ModelFunctions,
    breaks: &'a [Vec<f64>],
    grids: &'a [Option<Grid2>],
}

/// A mixed condition at a time, its leaves' values given and its chain
/// computed so far.
struct PointEnv<'a> {
    t: f64,
    m: &'a Mixed<'a>,
    ys: &'a [f64],
    chain: &'a [f64],
    base: &'a Base<'a>,
}

impl lsim_ir::eval::Env for PointEnv<'_> {
    fn time(&self) -> f64 {
        self.t
    }
    fn var(&self, v: VarId) -> f64 {
        let k = v.0 as usize;
        match self.m.slot.get(k).copied().unwrap_or(Slot::Const) {
            Slot::Chain(i) => self.chain.get(i as usize).copied().unwrap_or(f64::NAN),
            Slot::Leaf(p, s) => s * self.ys[p as usize],
            Slot::Const => self.base.vars.get(k).copied().unwrap_or(f64::NAN),
        }
    }
    fn der(&self, _: VarId) -> f64 {
        f64::NAN
    }
    fn param(&self, p: ParamId) -> f64 {
        self.base.params[p.0 as usize]
    }
    fn table(&self, k: u32, args: &[f64]) -> f64 {
        table_at(self.base.model, k, args).map_or(f64::NAN, |(v, _)| v)
    }
}

impl<'a> Mixed<'a> {
    /// Whether its enclosures can bound it usefully (what moves along a
    /// step: time, the entries of y it reads and what it computes from
    /// them).
    fn enclosable(&self, breaks: &[Vec<f64>], grids: &[Option<Grid2>]) -> bool {
        let moves = |e: &Expr| {
            e.any(&mut |x| match x {
                Expr::Time => true,
                Expr::Var(v) | Expr::Pre(v) => {
                    self.slot.get(v.0 as usize).is_some_and(|s| *s != Slot::Const)
                }
                _ => false,
            })
        };
        self.chain.iter().all(|(_, e)| supported(e, breaks, grids, &moves))
            && supported(self.g, breaks, grids, &moves)
    }

    /// The entries of y it reads.
    pub(super) fn reads_y(&self) -> &[usize] {
        &self.y_idx
    }

    /// Whether it is the condition `g` over `chain`.
    pub(super) fn same(&self, chain: &[(usize, Expr)], g: &Expr) -> bool {
        self.g == g && self.chain == chain
    }

    /// The condition at `t`, its leaves at `ys` (in `y_idx` order).
    fn point(&self, t: f64, ys: &[f64], base: &Base<'_>, scratch: &mut Vec<f64>) -> f64 {
        scratch.clear();
        for (_, e) in self.chain {
            let v = lsim_ir::eval::eval(e, &PointEnv { t, m: self, ys, chain: scratch, base });
            scratch.push(v);
        }
        lsim_ir::eval::eval(self.g, &PointEnv { t, m: self, ys, chain: scratch, base })
    }

    /// The condition's enclosure over the times `t`, its leaves enclosed
    /// by `leaf` (by position in `y_idx`).
    fn enclose(
        &self,
        t: Iv,
        leaf: &dyn Fn(usize) -> J2,
        base: &Base<'_>,
        scratch: &mut Vec<J2>,
    ) -> J2 {
        // the leaves first (each once), then the chain's steps
        scratch.clear();
        scratch.extend((0..self.y_idx.len()).map(leaf));
        let ny = self.y_idx.len();
        let lookup = |v: usize, at: &[J2]| match self.slot.get(v).copied().unwrap_or(Slot::Const) {
            Slot::Chain(i) => at.get(ny + i as usize).copied(),
            Slot::Leaf(p, s) => {
                let j = at[p as usize];
                Some(if s < 0.0 { j.neg() } else { j })
            }
            Slot::Const => None,
        };
        // the steps that do not move: as long as the constants keep their
        // values
        let mut fixed = self.fixed.borrow_mut();
        let same = fixed.1.len() == self.chain.len()
            && fixed.0.len() == self.consts.len()
            && self.consts.iter().zip(&fixed.0).all(|(k, b)| base.vars[*k].to_bits() == *b);
        if !same {
            fixed.0 = self.consts.iter().map(|k| base.vars[*k].to_bits()).collect();
            let mut at: Vec<J2> = vec![J2::konst(Iv::point(f64::NAN)); ny];
            for ((_, e), moves) in self.chain.iter().zip(&self.moving) {
                let j = if *moves {
                    J2::konst(Iv::point(f64::NAN))
                } else {
                    let l = |v: usize| lookup(v, &at);
                    let cx = Cx {
                        params: base.params,
                        vars: base.vars,
                        model: base.model,
                        breaks: base.breaks,
                        grids: base.grids,
                        leaf: &l,
                    };
                    enclose(e, &cx, t)
                };
                at.push(j);
            }
            fixed.1 = at.split_off(ny);
        }
        for ((_, e), (moves, kept)) in self.chain.iter().zip(self.moving.iter().zip(&fixed.1)) {
            if !moves {
                scratch.push(*kept);
                continue;
            }
            let j = {
                let l = |v: usize| lookup(v, scratch);
                let cx = Cx {
                    params: base.params,
                    vars: base.vars,
                    model: base.model,
                    breaks: base.breaks,
                    grids: base.grids,
                    leaf: &l,
                };
                enclose(e, &cx, t)
            };
            scratch.push(j);
        }
        let l = |v: usize| lookup(v, scratch);
        let cx = Cx {
            params: base.params,
            vars: base.vars,
            model: base.model,
            breaks: base.breaks,
            grids: base.grids,
            leaf: &l,
        };
        enclose(self.g, &cx, t)
    }
}

/// The scratch of the mixed conditions' check along a step, kept between
/// steps: the dense output's polynomial for the states they read, bounds
/// on the size of its basis functions over the step and on where each
/// state strays (by entry of y), the basis itself (set when first
/// needed), and the states at the step's end and their ranges over it (by
/// entry of y, filled when needed).
#[derive(Default)]
pub(super) struct StepScratch {
    dense: DenseOutput,
    step: (f64, f64),
    sizes: Vec<f64>,
    near: Vec<[f64; 2]>,
    basis: Basis,
    exact: bool,
    y1: Vec<f64>,
    span: Vec<[f64; 2]>,
}

impl StepScratch {
    /// The step `[t0, t1]`, its dense output in place for the entries
    /// `idx` of y (of `ny`): the sizes of its basis functions over the
    /// step, and where each entry may stray (the basis itself, the states
    /// at the step's end and their ranges over it when needed).
    fn start(&mut self, (t0, t1): (f64, f64), idx: &[usize], ny: usize) {
        let dense = &self.dense;
        self.step = (t0, t1);
        self.exact = false;
        basis_sizes(dense.origin, &dense.nodes, &dense.scales, (t0, t1), &mut self.sizes);
        self.near.resize(ny, [f64::NAN; 2]);
        let entries = dense.coef.chunks_exact(dense.degree() + 1).zip(&dense.err);
        for (&i, (a, &e)) in idx.iter().zip(entries) {
            let r = stray(a, &self.sizes, e);
            self.near[i] = [a[0] - r, a[0] + r];
        }
        self.y1.resize(ny, f64::NAN);
        self.span.resize(ny, [f64::NAN; 2]);
    }

    /// The basis set to the step.
    fn set_basis(&mut self) {
        if !self.exact {
            let (t0, t1) = self.step;
            self.basis.set(self.dense.origin, &self.dense.nodes, &self.dense.scales, t0, t1);
            self.exact = true;
        }
    }

    /// The ranges over the step of the states `idx` (by entry of y; `ypos`
    /// their entries in the dense output), into `span`.
    fn spans(&mut self, idx: &[usize], ypos: &[usize]) {
        self.set_basis();
        let l = self.dense.degree() + 1;
        for &i in idx {
            let k = ypos[i];
            let r = self.basis.range(&self.dense.coef[k * l..(k + 1) * l]);
            let e = self.dense.err[k];
            self.span[i] =
                if e > 0.0 { [(r.lo - e).next_down(), (r.hi + e).next_up()] } else { [r.lo, r.hi] };
        }
    }

    /// Whether the states `idx` stay inside the box `lo`, `hi` over the
    /// step: by how far each may stray from its polynomial's first
    /// coefficient first (no basis needed), by their ranges when that is
    /// not enough.
    fn inside(&mut self, idx: &[usize], ypos: &[usize], lo: &[f64], hi: &[f64]) -> bool {
        inside_box(idx, &self.near, lo, hi) || {
            self.spans(idx, ypos);
            inside_box(idx, &self.span, lo, hi)
        }
    }
}

/// Whether the ranges `span` of the states `y_idx` (by entry of y) over a
/// step lie inside the box `[lo, hi]` (by position).
fn inside_box(y_idx: &[usize], span: &[[f64; 2]], lo: &[f64], hi: &[f64]) -> bool {
    y_idx.iter().zip(lo.iter().zip(hi)).all(|(&i, (lo, hi))| {
        let [a, b] = span[i];
        a >= *lo && b <= *hi
    })
}

/// What the run loop makes of a mixed condition.
pub(super) enum Kind<'a> {
    /// checked along every step
    Scan(Box<Mixed<'a>>),
    /// its value moves only where a `noEvent` comparison flips: left to
    /// root finding, as `noEvent` asks
    Switched,
    /// a variable it reads is not an entry of y or constant between
    /// events, or what it computes cannot be enclosed
    Unsupported,
}

/// Whether `e` stays constant along a step but where a `noEvent`
/// comparison flips (`noev`: inside a `noEvent`): built of values constant
/// between events, of such comparisons and of functions of these; a
/// variable of the chain by its definition (`memo`: per step of the
/// chain).
fn switched(
    e: &Expr,
    noev: bool,
    slot: &[Slot],
    chain: &[(usize, Expr)],
    memo: &mut [Option<bool>],
) -> bool {
    match e {
        Expr::Time | Expr::Der(_) | Expr::Name(_) => false,
        Expr::Var(v) | Expr::Pre(v) => match slot.get(v.0 as usize).copied() {
            None | Some(Slot::Const) => true,
            Some(Slot::Leaf(..)) => false,
            Some(Slot::Chain(i)) => {
                let i = i as usize;
                if let Some(s) = memo.get(i).copied().flatten() {
                    return s;
                }
                let s = chain.get(i).is_some_and(|(_, d)| switched(d, false, slot, chain, memo));
                if let Some(m) = memo.get_mut(i) {
                    *m = Some(s);
                }
                s
            }
        },
        Expr::NoEvent(a) => switched(a, true, slot, chain, memo),
        // a truth value: it flips where its arguments cross
        Expr::Compare(_, a, b) => {
            noev || (switched(a, noev, slot, chain, memo) && switched(b, noev, slot, chain, memo))
        }
        _ => e.children().into_iter().all(|c| switched(c, noev, slot, chain, memo)),
    }
}

impl<'a> Loop<'a> {
    /// What a mixed condition reads, and whether it is checked along every
    /// step.
    pub(super) fn mixed_of(&self, chain: &'a [(usize, Expr)], g: &'a Expr) -> Kind<'a> {
        match self.mixed_reads(chain, g) {
            None => Kind::Unsupported,
            Some(m) => {
                let mut memo = vec![None; chain.len()];
                if switched(g, false, &m.slot, chain, &mut memo) {
                    Kind::Switched
                } else if m.enclosable(&self.info.table_breaks, &self.tf.grids) {
                    Kind::Scan(Box::new(m))
                } else {
                    Kind::Unsupported
                }
            }
        }
    }

    /// What a mixed condition reads (`None`: a variable it reads is not an
    /// entry of y or constant between events).
    fn mixed_reads(&self, chain: &'a [(usize, Expr)], g: &'a Expr) -> Option<Mixed<'a>> {
        let info = self.info;
        let n = info.var_sources.len().max(self.vars.len());
        let mut slot = vec![Slot::Const; n];
        for (k, (v, _)) in chain.iter().enumerate() {
            *slot.get_mut(*v)? = Slot::Chain(k as u32);
        }
        let mut y_idx: Vec<usize> = vec![];
        let mut consts: Vec<usize> = vec![];
        let mut ok = true;
        let mut visit = |e: &Expr| {
            e.walk(&mut |x| {
                let (Expr::Var(v) | Expr::Pre(v)) = x else { return };
                let k = v.0 as usize;
                if k >= slot.len() {
                    ok = false;
                    return;
                }
                if slot[k] != Slot::Const || consts.contains(&k) {
                    return;
                }
                match info.var_sources.get(k).copied() {
                    Some(VarSource::Y(i) | VarSource::NegY(i)) => {
                        let sign = if matches!(info.var_sources[k], VarSource::NegY(_)) {
                            -1.0
                        } else {
                            1.0
                        };
                        let p = y_idx.iter().position(|j| *j == i).unwrap_or_else(|| {
                            y_idx.push(i);
                            y_idx.len() - 1
                        });
                        slot[k] = Slot::Leaf(p as u32, sign);
                    }
                    Some(
                        VarSource::D(_)
                        | VarSource::NegD(_)
                        | VarSource::Const(_)
                        | VarSource::U(_),
                    ) => consts.push(k),
                    _ => ok = false,
                }
            })
        };
        for (_, e) in chain {
            visit(e);
        }
        visit(g);
        let mut moving: Vec<bool> = Vec::with_capacity(chain.len());
        for (_, e) in chain {
            let m = e.any(&mut |x| match x {
                Expr::Time => true,
                Expr::Var(v) | Expr::Pre(v) => match slot.get(v.0 as usize) {
                    Some(Slot::Leaf(..)) => true,
                    Some(Slot::Chain(i)) => moving.get(*i as usize).copied().unwrap_or(true),
                    _ => false,
                },
                _ => false,
            });
            moving.push(m);
        }
        ok.then_some(Mixed {
            moving,
            fixed: RefCell::new((vec![], vec![])),
            roots: vec![],
            g,
            chain,
            slot,
            y_idx,
            consts,
            cert: None,
            window: 0.0,
            fails: 0,
            wait: 0,
            warned: false,
            certified: 0,
            scanned: 0,
        })
    }

    /// The first sign change of a mixed condition inside the step `(t0,
    /// t1]` that root finding did not report (`reported`: the directions
    /// of the crossings it located at `t1`, per root function; empty for a
    /// step that did not end at one), in a direction one of its root
    /// functions is watched in. Returns its time, and per root function the
    /// direction (+1, -1; 0 for the others).
    pub(super) fn scan_mixed(
        &mut self,
        integ: &mut dyn Integrator,
        t0: f64,
        t1: f64,
        reported: &[i32],
        d: &[f64],
    ) -> Result<Option<(f64, Vec<i32>)>, SolveError> {
        if self.tf.mixed.is_empty() || t1 <= t0 || same_instant(t0, t1) {
            return Ok(None);
        }
        // the dense output's polynomial over the step, for every state the
        // conditions read (scratch kept between steps)
        let mut sc = self.tf.scratch.take().unwrap_or_default();
        let out = self.scan_mixed_with(integ, (t0, t1), reported, d, &mut sc);
        self.tf.scratch = Some(sc);
        out
    }

    /// [`Self::scan_mixed`] with its scratch.
    fn scan_mixed_with(
        &mut self,
        integ: &mut dyn Integrator,
        (t0, t1): (f64, f64),
        reported: &[i32],
        d: &[f64],
        sc: &mut StepScratch,
    ) -> Result<Option<(f64, Vec<i32>)>, SolveError> {
        let n = self.tf.mixed_y.len();
        let dense = &mut sc.dense;
        if !integ.dense_output(t0, &self.tf.mixed_y, dense)?
            || dense.coef.len() < n * (dense.degree() + 1)
            || dense.err.len() < n
        {
            for j in 0..self.tf.mixed.len() {
                self.cannot_check(j, "the integrator gives no polynomial for its dense output");
            }
            return Ok(None);
        }
        sc.start((t0, t1), &self.tf.mixed_y, integ.y().len());
        self.scan_mixed_at(integ, (t0, t1), reported, d, sc)
    }

    /// Says once that mixed condition `j` could not be checked along a
    /// step, and why.
    fn cannot_check(&mut self, j: usize, why: &str) {
        if !std::mem::replace(&mut self.tf.mixed[j].warned, true) {
            let k = self.tf.mixed[j].roots[0];
            let label = self.crossing_label(k);
            self.warnings.push(format!(
                "'{label}': a step could not be checked for a pulse of its condition ({why}); \
                 root finding alone watches it there, and can step over a pulse shorter than a \
                 step (a smaller max_step guards against that)"
            ));
        }
    }

    /// [`Self::scan_mixed`] with the dense output's polynomial over the
    /// step in `sc`.
    fn scan_mixed_at(
        &mut self,
        integ: &mut dyn Integrator,
        (t0, t1): (f64, f64),
        reported: &[i32],
        d: &[f64],
        sc: &mut StepScratch,
    ) -> Result<Option<(f64, Vec<i32>)>, SolveError> {
        let mut best: Option<(f64, usize, bool)> = None;
        let mut jscratch: Vec<J2> = vec![];
        let mut fscratch: Vec<f64> = vec![];
        for j in 0..self.tf.mixed.len() {
            let base = Base {
                params: &self.info.params,
                vars: &self.vars,
                model: self.model,
                breaks: &self.info.table_breaks,
                grids: &self.tf.grids,
            };
            let m = &self.tf.mixed[j];
            // a certificate holds for this step: nothing more to check (the
            // range of each state it reads over the step inside its box)
            let holds = m.cert.as_ref().is_some_and(|c| {
                t0 >= c.from
                    && t1 <= c.until
                    && m.consts
                        .iter()
                        .zip(&c.consts)
                        .all(|(k, v)| base.vars[*k].to_bits() == v.to_bits())
                    && sc.inside(&m.y_idx, &self.tf.ypos, &c.lo, &c.hi)
            });
            if holds {
                self.tf.mixed[j].certified += 1;
                continue;
            }
            // the step along the dense output
            let limit = best.map_or(t1, |b| b.0);
            sc.set_basis();
            let found = self.scan_step(
                j,
                (t0, t1, limit),
                reported,
                (&sc.dense, &mut sc.basis),
                &base,
                &mut jscratch,
                &mut fscratch,
            );
            self.tf.mixed[j].scanned += 1;
            match found {
                Ok(Some((at, rising))) => best = Some((at, j, rising)),
                Ok(None) => {}
                Err(why) => self.cannot_check(j, &why),
            }
            // a certificate for the steps ahead, around the state at t1
            let m = &mut self.tf.mixed[j];
            m.cert = None;
            if m.wait > 0 {
                m.wait -= 1;
            } else {
                let y = integ.y();
                for &i in &m.y_idx {
                    sc.y1[i] = y[i];
                }
                sc.spans(&m.y_idx, &self.tf.ypos);
                self.certify(j, (t0, t1), &sc.y1, &sc.span, &mut jscratch);
            }
        }
        let Some((at, j, rising)) = best else { return Ok(None) };
        let mut dirs = vec![0; self.sides.len()];
        for &k in &self.tf.mixed[j].roots {
            let dk = self.info.root_dirs.get(k).copied().unwrap_or(0);
            if dk == 0 || (dk > 0) == rising {
                dirs[k] = if rising { 1 } else { -1 };
            }
        }
        // where the model's own root function, on the dense output, is on
        // the new side (the event iteration decides the conditions with it)
        let k = dirs.iter().position(|x| *x != 0).unwrap_or(self.tf.mixed[j].roots[0]);
        Ok(self.model_side(integ, k, at, rising, t1, d)?.map(|at| (at, dirs)))
    }

    /// Mixed condition `j` along the step `[t0, t1]` of the dense output
    /// (`dense`, for the states the conditions read): its first sign
    /// change in a watched direction before `limit` that is not the
    /// crossing root finding located at `t1` (`reported`), with its
    /// direction (`Err`: why it could not be checked).
    #[allow(clippy::too_many_arguments)]
    fn scan_step(
        &self,
        j: usize,
        (t0, t1, limit): (f64, f64, f64),
        reported: &[i32],
        (dense, basis): (&DenseOutput, &mut Basis),
        base: &Base<'_>,
        jscratch: &mut Vec<J2>,
        fscratch: &mut Vec<f64>,
    ) -> Result<Option<(f64, bool)>, String> {
        let m = &self.tf.mixed[j];
        let ny = m.y_idx.len();
        // each state it reads: the dense output's polynomial itself
        let polys: Vec<DensePoly> = m
            .y_idx
            .iter()
            .map(|&i| {
                let k = self.tf.ypos[i];
                let form = (dense.origin, &dense.nodes[..], &dense.scales[..]);
                basis.poly(form, dense.coefficients(k), dense.err[k])
            })
            .collect();
        let jcell = std::cell::RefCell::new(std::mem::take(jscratch));
        let fcell = std::cell::RefCell::new(std::mem::take(fscratch));
        let mut ys = vec![0.0; ny];
        let mut p = |t: f64| {
            for (y, poly) in ys.iter_mut().zip(&polys) {
                *y = poly.at(t);
            }
            m.point(t, &ys, base, &mut fcell.borrow_mut())
        };
        let mut enc = |l: f64, r: f64| {
            let t = Iv { lo: l, hi: r };
            let e = m.enclose(t, &|pos| polys[pos].j2(t), base, &mut jcell.borrow_mut());
            (e.v, e.d)
        };
        let watched = |rising: bool| {
            m.roots.iter().any(|k| {
                let d = self.info.root_dirs.get(*k).copied().unwrap_or(0);
                d == 0 || (d > 0) == rising
            })
        };
        // the whole step first, its value alone: most keep their sign over
        // it (no point evaluation needed)
        let v = {
            let t = Iv { lo: t0, hi: limit };
            let leaf = |pos: usize| J2::jumps(polys[pos].value(t));
            m.enclose(t, &leaf, base, &mut jcell.borrow_mut()).v
        };
        let mut from = if v.lo > 0.0 || v.hi < 0.0 { limit } else { t0 };
        let out = loop {
            if from >= limit {
                break Ok(None);
            }
            match first_change(&mut p, &mut enc, from, limit, BUDGET) {
                Found::Change { at, rising } => {
                    if same_instant(at, limit) || at >= limit {
                        break Ok(None);
                    }
                    if watched(rising) {
                        // the crossing root finding located at the step's
                        // end (to its tolerance, a little after the change
                        // found here): the only change up to there
                        let located = m.roots.iter().any(|k| {
                            let r = reported.get(*k).copied().unwrap_or(0);
                            r != 0 && (r > 0) == rising
                        });
                        if located
                            && matches!(
                                first_change(&mut p, &mut enc, at, t1, BUDGET),
                                Found::Nothing
                            )
                        {
                            break Ok(None);
                        }
                        break Ok(Some((at, rising)));
                    }
                    from = at;
                }
                Found::Nothing => break Ok(None),
                Found::Clear(at) => {
                    break Err(format!(
                        "its search along the step makes no headway at t = {at:.6} s"
                    ));
                }
                Found::Undefined => break Err(format!("it is not defined at t = {from:.6} s")),
            }
        };
        *jscratch = jcell.into_inner();
        *fscratch = fcell.into_inner();
        out
    }

    /// A certificate for mixed condition `j` from `t1` on: a time window
    /// and a box of its states around `y1` over which its enclosure
    /// excludes zero, the box's half-width each state's range over the
    /// last step (`span`) for each step the window holds at that pace, and
    /// one more; none when the condition is too near zero.
    fn certify(
        &mut self,
        j: usize,
        (t0, t1): (f64, f64),
        y1: &[f64],
        span: &[[f64; 2]],
        scratch: &mut Vec<J2>,
    ) {
        let h = t1 - t0;
        let horizon = (self.tf.t_end - t1).max(0.0);
        let base = Base {
            params: &self.info.params,
            vars: &self.vars,
            model: self.model,
            breaks: &self.info.table_breaks,
            grids: &self.tf.grids,
        };
        let m = &self.tf.mixed[j];
        let mut w = if m.window > 0.0 { m.window } else { 16.0 * h };
        let mut got = None;
        for attempt in 0..3 {
            // the last attempt: the next step alone
            let w_try = if attempt == 2 { h } else { w.min(horizon).max(h) };
            let steps = w_try / h;
            let (lo, hi): (Vec<f64>, Vec<f64>) = m
                .y_idx
                .iter()
                .map(|&i| {
                    let move_ = (span[i][1] - span[i][0]) * (steps + 1.0);
                    let r = move_ + 1e-9 * y1[i].abs();
                    (y1[i] - r, y1[i] + r)
                })
                .unzip();
            let t = Iv::new(t1, t1 + w_try);
            let e = m.enclose(t, &|p| J2::jumps(Iv::new(lo[p], hi[p])), &base, scratch);
            if e.v.lo > 0.0 || e.v.hi < 0.0 {
                let consts = m.consts.iter().map(|k| base.vars[*k]).collect();
                got = Some((Cert { from: t1, until: t1 + w_try, lo, hi, consts }, w_try));
                break;
            }
            w = w_try / 4.0;
            if w < h {
                break;
            }
        }
        let m = &mut self.tf.mixed[j];
        match got {
            Some((c, w_used)) => {
                m.cert = Some(c);
                m.window = 2.0 * w_used;
                m.fails = 0;
            }
            None => {
                m.cert = None;
                m.window = (w / 4.0).max(h);
                m.fails += 1;
                m.wait = (1u32 << m.fails.min(5)).min(16) - 1;
            }
        }
    }

    /// The first time from `at` on (to `limit`) at which root function `k`
    /// of the model, on the dense output, is on the side a change in
    /// direction `rising` leads to; `None` when it is not before `limit`.
    fn model_side(
        &mut self,
        integ: &mut dyn Integrator,
        k: usize,
        at: f64,
        rising: bool,
        limit: f64,
        d: &[f64],
    ) -> Result<Option<f64>, SolveError> {
        let mut y = integ.y().to_vec();
        let mut g = |lp: &mut Self, t: f64| -> Result<f64, SolveError> {
            integ.interpolate(t, &mut y)?;
            lp.eval_raw(t, &y, d);
            Ok(lp.raw[k])
        };
        // (strictly: an exact zero counts as the side it came from)
        let there = |x: f64| if rising { x > 0.0 } else { x < 0.0 };
        if there(g(self, at)?) {
            return Ok(Some(at));
        }
        // forward, doubling, then bisected to adjacent floats
        let (mut lo, mut step) = (at, at.next_up() - at);
        let mut hi = loop {
            let hi = (lo + step).min(limit);
            if there(g(self, hi)?) {
                break hi;
            }
            if hi >= limit {
                return Ok(None);
            }
            lo = hi;
            step *= 2.0;
        };
        loop {
            let m = lo + 0.5 * (hi - lo);
            if m <= lo || m >= hi {
                return Ok(Some(hi));
            }
            if there(g(self, m)?) {
                hi = m;
            } else {
                lo = m;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsim_ir::expr::{BinaryOp, Builtin, CmpOp};

    /// A state that swings out and back within one step, S-shaped (`c − k
    /// u (u − ½)(u − 1)`, u from 1 to 0 across the step): it has the same
    /// value at the step's ends and middle, so a check of those three
    /// points (the eighth round's certificate) would have kept a box it
    /// leaves. Its range over the step, from the dense output's polynomial
    /// itself, does not fit the box.
    #[test]
    fn an_s_shaped_swing_inside_one_step_leaves_the_certificate() {
        let (t0, h, k, c) = (10.0, 0.5, 2.0, 3.0);
        // the cubic in powers of τ = t − (t0 + h) (a Newton form with nodes
        // 0 and scales 1): u = −τ / h, −k (u³ − 3u²/2 + u/2)
        let (a3, a2, a1) = (k / (h * h * h), 1.5 * k / (h * h), 0.5 * k / h);
        let coef = [c, a1, a2, a3];
        let dense = DenseOutput {
            origin: t0 + h,
            nodes: vec![0.0; 3],
            scales: vec![1.0; 3],
            coef: coef.to_vec(),
            err: vec![0.0],
        };
        let at = |t: f64| dense.at(0, t);
        // the ends and the middle are at c
        for t in [t0, t0 + 0.5 * h, t0 + h] {
            assert!((at(t) - c).abs() < 1e-12, "{t}: {}", at(t));
        }
        // it swings by k √3 / 36 ≈ 0.096 either way
        let swing = k * 3f64.sqrt() / 36.0;
        let peak = at(t0 + h * (0.5 - 3f64.sqrt() / 6.0));
        assert!((peak - c - swing).abs() < 1e-9, "{peak}");
        // a box of half the swing about c holds the three points the old
        // check sampled, with its margins (twice the change, four times
        // the bulge: both zero)
        let (lo, hi) = ([c - 0.5 * swing], [c + 0.5 * swing]);
        assert!([at(t0), at(t0 + 0.5 * h), at(t0 + h)].iter().all(|y| lo[0] <= *y && *y <= hi[0]));
        // the check along the step does not hold in it: not by how far the
        // state may stray from its first coefficient (that bound holds the
        // swing), nor by its range
        let mut sc = StepScratch { dense: dense.clone(), ..Default::default() };
        sc.start((t0, t0 + h), &[0], 1);
        let [nlo, nhi] = sc.near[0];
        assert!(nlo <= c - swing && nhi >= c + swing, "{:?}", sc.near[0]);
        assert!(!sc.inside(&[0], &[0], &lo, &hi));
        assert!(sc.exact);
        // the step's range does not fit it
        let mut basis = Basis::default();
        basis.set(dense.origin, &dense.nodes, &dense.scales, t0, t0 + h);
        let r = basis.range(dense.coefficients(0));
        assert!(r.lo <= c - swing && r.hi >= c + swing, "{r:?}");
        let span = [[r.lo, r.hi]];
        assert!(!inside_box(&[0], &span, &lo, &hi));
        // a box that holds the swing holds the range... once wide enough
        // for the range's overestimate
        assert!(inside_box(&[0], &span, &[r.lo], &[r.hi]));
    }

    /// Only a value that moves where a `noEvent` comparison flips, and
    /// nowhere else, is switched: a continuous function inside `noEvent`
    /// (an `abs` without its event), a branch that moves, or a comparison
    /// that is an event are not.
    #[test]
    fn switched_means_moved_only_by_no_event_comparisons() {
        // var 0: a state; var 1: constant between events; var 2: the chain's
        // step `if noEvent(var 0 > time) then 1 else 0`
        let slot = [Slot::Leaf(0, 1.0), Slot::Const, Slot::Chain(0)];
        let x = || Expr::Var(VarId(0));
        let c = || Expr::Var(VarId(1));
        let cmp = |a: Expr, b: Expr| Expr::Compare(CmpOp::Gt, Box::new(a), Box::new(b));
        let noev = |a: Expr| Expr::NoEvent(Box::new(a));
        let ite = |p: Expr, a: Expr, b: Expr| Expr::If(Box::new(p), Box::new(a), Box::new(b));
        let sub = |a: Expr, b: Expr| Expr::Binary(BinaryOp::Sub, Box::new(a), Box::new(b));
        let chain = vec![(2, ite(noev(cmp(x(), Expr::Time)), Expr::Const(1.0), Expr::Const(0.0)))];
        let is = |e: &Expr| switched(e, false, &slot, &chain, &mut [None]);
        assert!(is(&sub(Expr::Var(VarId(2)), Expr::Const(0.5))));
        assert!(is(&ite(noev(cmp(x(), c())), c(), Expr::Const(2.0))));
        assert!(is(&sub(c(), Expr::Const(1.0))));
        // moves continuously
        assert!(!is(&sub(noev(Expr::Call(Builtin::Abs, vec![x()])), Expr::Time)));
        assert!(!is(&ite(noev(cmp(x(), c())), x(), Expr::Const(0.0))));
        assert!(!is(&sub(Expr::Var(VarId(2)), Expr::Time)));
        // a comparison that is an event
        assert!(!is(&ite(cmp(x(), Expr::Time), Expr::Const(1.0), Expr::Const(0.0))));
    }
}
