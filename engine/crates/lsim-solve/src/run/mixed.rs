//! Conditions that read time and continuous variables
//! ([`crate::TimeFunction::Mixed`]: `x > sin(ω time)`, a driver's command
//! from a driving cycle's target against the car's speed, a motor's
//! limits from its maps at that command): no step may hold a sign change
//! root finding cannot see, two inside one step (a pulse), as root finding
//! sees a condition only by its sign at a step's ends. A driving cycle's
//! conditions too: the stops at the cycle's breakpoints keep its target
//! linear within a step, but the state it is compared with may curve.
//!
//! Two checks, the cheap one first:
//!
//! * **A certificate.** The condition keeps its sign over a time window
//!   and a box of the states it reads: its interval enclosure there (the
//!   time terms exactly, a table by its pieces) excludes zero. A step
//!   inside the window whose ends and middle lie inside the box, by twice
//!   the step's change and four times its bulge (the middle's distance
//!   from the chord) on either side, needs no more: a few comparisons per
//!   state, and one sample of the dense output per step for all the
//!   conditions. The certificate is made again, around the state at a
//!   step's end, when a step leaves it (its window grows while it holds,
//!   and shrinks when the condition is near zero; after a failure the next
//!   attempt waits a few steps, longer each time, to 16).
//! * **The step along the dense output**, where no certificate holds: the
//!   states and iteration variables the condition reads, as the
//!   polynomials of degree 7 through the integrator's dense output at
//!   Chebyshev points (a BDF dense output exactly; the fit checked at one
//!   more point, its error carried into the enclosures), enclose the
//!   condition over any part of the step, and [`first_change`] finds its
//!   first sign change (the whole step's enclosure first, which settles
//!   most). One root finding did not report ends the step, as a root does,
//!   where the model's own root function on the dense output is on the
//!   new side. The crossing root finding located at the step's end is not
//!   one: it locates a root to its tolerance (100 ε (|t| + h)), a little
//!   after the change found here, which is then the only change up to the
//!   step's end.
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
use crate::interval::{Cx, Found, Grid2, Iv, J2, Poly, enclose, first_change, supported};
use crate::{Integrator, SolveError};
use lsim_ir::runtime::ModelFunctions;
use lsim_ir::{Expr, ParamId, VarId};
use std::cell::RefCell;
use std::sync::OnceLock;

/// The steps one search may take.
const BUDGET: usize = 4000;

/// The points a step is sampled at along its dense output: Chebyshev
/// (Lobatto) points of `[-1, 1]`, the ends included.
const NODES: usize = 8;

fn nodes() -> [f64; NODES] {
    std::array::from_fn(|j| -(j as f64 * std::f64::consts::PI / (NODES - 1) as f64).cos())
}

/// The inverse of the Vandermonde matrix at [`nodes`]: the coefficients of
/// the polynomial of degree 7 through values there (a dense output of
/// order up to 7 exactly).
fn inverse_vandermonde() -> &'static [[f64; NODES]; NODES] {
    static INV: OnceLock<[[f64; NODES]; NODES]> = OnceLock::new();
    INV.get_or_init(|| {
        let u = nodes();
        // [V | I] → [I | V⁻¹], partial pivoting
        let mut m = [[0.0; 2 * NODES]; NODES];
        for (i, row) in m.iter_mut().enumerate() {
            for (k, x) in row.iter_mut().take(NODES).enumerate() {
                *x = u[i].powi(k as i32);
            }
            row[NODES + i] = 1.0;
        }
        for col in 0..NODES {
            let piv = (col..NODES)
                .max_by(|a, b| m[*a][col].abs().total_cmp(&m[*b][col].abs()))
                .unwrap_or(col);
            m.swap(col, piv);
            let p = m[col][col];
            for x in m[col].iter_mut() {
                *x /= p;
            }
            for r in 0..NODES {
                if r != col {
                    let f = m[r][col];
                    if f != 0.0 {
                        let pivot_row = m[col];
                        for (x, pv) in m[r].iter_mut().zip(pivot_row) {
                            *x -= f * pv;
                        }
                    }
                }
            }
        }
        std::array::from_fn(|i| std::array::from_fn(|k| m[i][NODES + k]))
    })
}

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

/// How far a state may stray from its values at a step's ends `a`, `b`
/// (`m`: at its middle) within the step: twice its change, and four times
/// its bulge (the middle's distance from the chord; a quadratic's
/// greatest).
fn reach(a: f64, m: f64, b: f64) -> f64 {
    2.0 * (b - a).abs() + 4.0 * (m - 0.5 * (a + b)).abs()
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
    /// functions is watched in (`y0`: the state at `t0`; the integrator's
    /// at `t1`). Returns its time, and per root function the direction
    /// (+1, -1; 0 for the others).
    pub(super) fn scan_mixed(
        &mut self,
        integ: &mut dyn Integrator,
        t0: f64,
        t1: f64,
        reported: &[i32],
        d: &[f64],
        y0: &[f64],
    ) -> Result<Option<(f64, Vec<i32>)>, SolveError> {
        if self.tf.mixed.is_empty() || t1 <= t0 || same_instant(t0, t1) {
            return Ok(None);
        }
        // the states the conditions read at the step's end and at its
        // middle (scratch kept between steps)
        let mut y1 = std::mem::take(&mut self.tf.y1);
        let y = integ.y();
        y1.resize(y.len(), f64::NAN);
        for &i in &self.tf.mixed_y {
            y1[i] = y[i];
        }
        let mut ym = std::mem::take(&mut self.tf.ym);
        ym.resize(y1.len(), f64::NAN);
        let mut mid = std::mem::take(&mut self.tf.mid);
        mid.resize(self.tf.mixed_y.len(), 0.0);
        integ.interpolate_select(0.5 * (t0 + t1), &self.tf.mixed_y, &mut mid)?;
        // where each may stray within the step: its values at the ends and
        // the middle, widened by twice its change and four times its bulge
        let mut span = std::mem::take(&mut self.tf.span);
        span.resize(y1.len(), [f64::NAN; 2]);
        for (&i, &v) in self.tf.mixed_y.iter().zip(&mid) {
            ym[i] = v;
            let (a, b) = (y0[i], y1[i]);
            let e = reach(a, v, b);
            span[i] = [a.min(v).min(b) - e, a.max(v).max(b) + e];
        }
        let out = self.scan_mixed_at(integ, t0, t1, reported, d, y0, &y1, &ym, &span);
        self.tf.y1 = y1;
        self.tf.ym = ym;
        self.tf.mid = mid;
        self.tf.span = span;
        out
    }

    /// [`Self::scan_mixed`] with the states the conditions read at the
    /// step's end in `y1`, at its middle in `ym`, and where they may stray
    /// within it in `span`.
    #[allow(clippy::too_many_arguments)]
    fn scan_mixed_at(
        &mut self,
        integ: &mut dyn Integrator,
        t0: f64,
        t1: f64,
        reported: &[i32],
        d: &[f64],
        y0: &[f64],
        y1: &[f64],
        ym: &[f64],
        span: &[[f64; 2]],
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
            // step's ends and middle inside its box, by twice the step's
            // change and four times its bulge on either side)
            let holds = m.cert.as_ref().is_some_and(|c| {
                t0 >= c.from
                    && t1 <= c.until
                    && m.consts
                        .iter()
                        .zip(&c.consts)
                        .all(|(k, v)| base.vars[*k].to_bits() == v.to_bits())
                    && m.y_idx.iter().zip(c.lo.iter().zip(&c.hi)).all(|(&i, (lo, hi))| {
                        let [a, b] = span[i];
                        a >= *lo && b <= *hi
                    })
            });
            if holds {
                self.tf.mixed[j].certified += 1;
                continue;
            }
            // the step along the dense output
            let limit = best.map_or(t1, |b| b.0);
            let found = self.scan_step(
                integ,
                j,
                (t0, t1, limit),
                reported,
                &base,
                &mut jscratch,
                &mut fscratch,
            )?;
            self.tf.mixed[j].scanned += 1;
            match found {
                Ok(Some((at, rising))) => best = Some((at, j, rising)),
                Ok(None) => {}
                Err(why) => {
                    if !std::mem::replace(&mut self.tf.mixed[j].warned, true) {
                        let k = self.tf.mixed[j].roots[0];
                        let label = self.crossing_label(k);
                        self.warnings.push(format!(
                            "'{label}': a step could not be checked for a pulse of its condition \
                             ({why}); root finding alone watches it there, and can step over a \
                             pulse shorter than a step (a smaller max_step guards against that)"
                        ));
                    }
                }
            }
            // a certificate for the steps ahead, around the state at t1
            let m = &mut self.tf.mixed[j];
            m.cert = None;
            if m.wait > 0 {
                m.wait -= 1;
            } else {
                self.certify(j, t0, t1, y0, ym, y1, &mut jscratch);
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

    /// Mixed condition `j` along the step `[t0, t1]` of the dense output:
    /// its first sign change in a watched direction before `limit` that is
    /// not the crossing root finding located at `t1` (`reported`), with
    /// its direction (`Err`: why it could not be checked).
    #[allow(clippy::too_many_arguments)]
    fn scan_step(
        &self,
        integ: &mut dyn Integrator,
        j: usize,
        (t0, t1, limit): (f64, f64, f64),
        reported: &[i32],
        base: &Base<'_>,
        jscratch: &mut Vec<J2>,
        fscratch: &mut Vec<f64>,
    ) -> Result<Result<Option<(f64, bool)>, String>, SolveError> {
        let m = &self.tf.mixed[j];
        let (c, s) = (0.5 * (t0 + t1), 0.5 * (t1 - t0));
        let u = nodes();
        let inv = inverse_vandermonde();
        let ny = m.y_idx.len();
        // the entries of y it reads at the nodes, and at one more
        let mut f = vec![[0.0; NODES]; ny];
        let mut buf = vec![0.0; ny];
        for (q, uq) in u.iter().enumerate() {
            let tau = match q {
                0 => t0,
                _ if q == NODES - 1 => t1,
                _ => c + s * uq,
            };
            integ.interpolate_select(tau, &m.y_idx, &mut buf)?;
            for (fi, b) in f.iter_mut().zip(&buf) {
                fi[q] = *b;
            }
        }
        let check = c + 0.37 * s;
        integ.interpolate_select(check, &m.y_idx, &mut buf)?;
        let polys: Vec<Poly> = f
            .iter()
            .zip(&buf)
            .map(|(fi, at_check)| {
                let a: Vec<f64> =
                    (0..NODES).map(|k| (0..NODES).map(|q| inv[k][q] * fi[q]).sum()).collect();
                let mut p = Poly { c, s, a, err: 0.0 };
                let scale: f64 = p.a.iter().map(|x| x.abs()).sum::<f64>()
                    + fi.iter().fold(0.0f64, |x, y| x.max(y.abs()));
                p.err = 256.0 * f64::EPSILON * scale + 4.0 * (p.at(check) - at_check).abs();
                p
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
        Ok(out)
    }

    /// A certificate for mixed condition `j` from `t1` on: a time window
    /// and a box of its states around `y1` over which its enclosure
    /// excludes zero, the box's half-width the last step's change and
    /// twice its bulge for each step the window holds at that pace, and
    /// one more; none when the condition is too near zero.
    #[allow(clippy::too_many_arguments)]
    fn certify(
        &mut self,
        j: usize,
        t0: f64,
        t1: f64,
        y0: &[f64],
        ym: &[f64],
        y1: &[f64],
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
                    let move_ = 0.5 * reach(y0[i], ym[i], y1[i]) * (steps + 1.0);
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
