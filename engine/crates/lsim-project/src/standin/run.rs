//! Stand-in run loop: Stage 1's SUNDIALS backend driven with what work
//! package 4 will provide, done here in the simplest correct way:
//!
//! * **event iteration** with Modelica's `when` semantics: after the fired
//!   clauses' actions, the integrator restarts (consistent iteration
//!   variables), every condition is evaluated again with the new discrete
//!   values, and a clause whose condition became true fires in the same
//!   instant, until nothing changes (a friction element that sticks and
//!   cannot hold slides on the other way at once);
//! * **impulse re-initialisation**: when an event changes a constraint
//!   between states (a gearbox selects another ratio), the speeds jump to
//!   the nearest consistent ones in the kinetic-energy metric — the
//!   inelastic, momentum-conserving engagement of a rigid dog clutch
//!   (Gauss's principle of least constraint). The masses come from the
//!   components' stored energies (½·J·w², ½·m·v², ½·C·v²);
//! * **sampled blocks** (`DiscreteBlock`: Script blocks, FMUs, digital
//!   controllers): the integrator stops exactly at each tick; it restarts
//!   only when an output changed;
//! * a **stop condition** (an acceleration test's finish line), located on
//!   the dense output.

use super::prep::Prepared;
use lsim_codegen::JitModel;
use lsim_ir::VarId;
use lsim_ir::eval::{SliceEnv, eval};
use lsim_ir::expr::Expr;
use lsim_ir::prepared::{Direction, Slot};
use lsim_ir::runtime::{DiscreteBlock, EvalInput, ModelFunctions};
use lsim_prep::symbolic::{diff, simplify};
use lsim_solve::sundials::Sundials;
use lsim_solve::{Integrator, OutputGrid, Recorder, RunInfo, SolverOptions, SolverStats, Step};
use std::time::Instant;

/// A sampled block and where it sits in the model.
pub struct Sampled {
    /// the block
    pub block: Box<dyn DiscreteBlock>,
    /// the variables it reads at each tick
    pub inputs: Vec<VarId>,
    /// the discrete variables it sets
    pub outputs: Vec<VarId>,
}

/// What to run.
#[derive(Clone, Debug)]
pub struct RunSpec {
    /// tolerances and limits
    pub solver: SolverOptions,
    /// the output grid
    pub grid: OutputGrid,
    /// stop when this variable rises to this level (located exactly)
    pub stop: Option<(VarId, f64)>,
    /// most event iterations at one instant
    pub max_event_iterations: usize,
}

impl RunSpec {
    /// A run over `grid` with default tolerances.
    pub fn new(grid: OutputGrid) -> Self {
        RunSpec { solver: SolverOptions::default(), grid, stop: None, max_event_iterations: 50 }
    }
}

/// An event that happened.
#[derive(Clone, Debug)]
pub struct Event {
    /// when, s
    pub t: f64,
    /// what fired
    pub what: String,
}

/// A run's results.
#[derive(Clone, Debug)]
pub struct RunResult {
    /// output times (the last one is the stop time when a stop condition ended the run)
    pub times: Vec<f64>,
    /// every flat variable's name
    pub names: Vec<String>,
    /// `values[var][k]`
    pub values: Vec<Vec<f64>>,
    /// lowest value over the interval ending at `times[k]`
    pub min: Vec<Vec<f64>>,
    /// highest value over the interval
    pub max: Vec<Vec<f64>>,
    /// time-mean over the interval
    pub mean: Vec<Vec<f64>>,
    /// the events
    pub events: Vec<Event>,
    /// solver counters
    pub stats: SolverStats,
    /// which integrator ran
    pub backend: &'static str,
    /// when the stop condition ended the run, if it did
    pub stopped_at: Option<f64>,
    /// wall-clock time of the run, s
    pub wall_seconds: f64,
    /// sampled-block ticks that changed an output (each one restarted the integrator)
    pub restarts_by_blocks: usize,
    /// kinetic energy dissipated by rigid engagements (impulses), J
    pub impulse_loss: f64,
}

impl RunResult {
    /// A channel by flat name.
    pub fn channel(&self, name: &str) -> Option<&[f64]> {
        self.names.iter().position(|n| n == name).map(|i| self.values[i].as_slice())
    }

    /// The last value of a channel.
    pub fn last(&self, name: &str) -> Option<f64> {
        self.channel(name).and_then(|v| v.last().copied())
    }
}

/// Precomputed impulse re-initialisation data.
struct Impulse {
    /// the constraint vars (all constraints), and their y position if kept
    vars: Vec<(VarId, Option<usize>)>,
    /// per constraint: residual and its partial derivatives per var
    rows: Vec<(Expr, Vec<(usize, Expr)>)>,
    /// per var: the second derivative of the stored energy (its mass)
    mass: Vec<Expr>,
}

fn impulse_data(prep: &Prepared) -> Option<Impulse> {
    if prep.constraints.is_empty() {
        return None;
    }
    let mut vars: Vec<(VarId, Option<usize>)> = vec![];
    for c in &prep.constraints {
        for v in &c.vars {
            if !vars.iter().any(|(x, _)| x == v) {
                let pos = prep.model.states.iter().position(|s| s == v);
                vars.push((*v, pos));
            }
        }
    }
    let rows = prep
        .constraints
        .iter()
        .map(|c| {
            let parts = vars
                .iter()
                .enumerate()
                .filter(|(_, (v, _))| c.vars.contains(v))
                .map(|(j, (v, _))| (j, simplify(diff(&c.residual, Slot::Var(*v)))))
                .collect();
            (c.residual.clone(), parts)
        })
        .collect();
    let mass = vars
        .iter()
        .map(|(v, _)| {
            let mut m = Expr::Const(0.0);
            for (_, s) in &prep.stored {
                if s.any(&mut |x| matches!(x, Expr::Var(w) if w == v)) {
                    let d2 = simplify(diff(&diff(s, Slot::Var(*v)), Slot::Var(*v)));
                    m = m + d2;
                }
            }
            simplify(m)
        })
        .collect();
    Some(Impulse { vars, rows, mass })
}

/// Solves a small dense system in place (Gaussian elimination with
/// partial pivoting); `a` is row-major n×n.
fn solve_dense(a: &mut [Vec<f64>], b: &mut [f64]) -> bool {
    let n = b.len();
    for col in 0..n {
        let piv = (col..n).max_by(|&i, &j| a[i][col].abs().total_cmp(&a[j][col].abs()));
        let Some(piv) = piv else { return false };
        if a[piv][col].abs() < 1e-300 {
            return false;
        }
        a.swap(col, piv);
        b.swap(col, piv);
        let (top, rest) = a.split_at_mut(col + 1);
        let pivot_row = &top[col];
        for (off, row) in rest.iter_mut().enumerate() {
            let f = row[col] / pivot_row[col];
            if f != 0.0 {
                for (x, p) in row[col..n].iter_mut().zip(&pivot_row[col..n]) {
                    *x -= f * p;
                }
                b[col + 1 + off] -= f * b[col];
            }
        }
    }
    for r in (0..n).rev() {
        let mut s = b[r];
        for k in r + 1..n {
            s -= a[r][k] * b[k];
        }
        b[r] = s / a[r][r];
    }
    true
}

impl Impulse {
    /// Projects the kept states in `y` onto the constraints with the new
    /// discrete values, in the kinetic metric; `vars` are the values just
    /// before the event with the new discrete values put in. Returns, when
    /// it moved anything, the first violated constraint and each variable's
    /// change of kinetic energy (½·M·(w+² − w−²)).
    fn project(
        &self,
        vars: &[f64],
        params: &[f64],
        t: f64,
        y: &mut [f64],
    ) -> Option<(usize, Vec<(VarId, f64)>)> {
        let ders = vec![f64::NAN; vars.len()];
        let env = SliceEnv { t, vars, ders: &ders, params };
        let r: Vec<f64> = self.rows.iter().map(|(g, _)| eval(g, &env)).collect();
        let scale = self.vars.iter().map(|(v, _)| vars[v.0 as usize].abs()).fold(1.0, f64::max);
        let violated = r.iter().position(|x| x.abs() > 1e-10 * scale)?;
        let m: Vec<f64> = self.mass.iter().map(|e| eval(e, &env)).collect();
        let m_max = m.iter().copied().fold(0.0, f64::max).max(1e-300);
        let minv: Vec<f64> =
            m.iter().map(|&x| 1.0 / if x > 1e-12 * m_max { x } else { 1e-12 * m_max }).collect();
        let nc = self.rows.len();
        let cmat: Vec<Vec<(usize, f64)>> = self
            .rows
            .iter()
            .map(|(_, parts)| parts.iter().map(|(j, e)| (*j, eval(e, &env))).collect())
            .collect();
        let mut a = vec![vec![0.0; nc]; nc];
        for i in 0..nc {
            for k in 0..nc {
                let mut s = 0.0;
                for (j, cij) in &cmat[i] {
                    if let Some((_, ckj)) = cmat[k].iter().find(|(jj, _)| jj == j) {
                        s += cij * minv[*j] * ckj;
                    }
                }
                a[i][k] = s;
            }
        }
        let tr: f64 = (0..nc).map(|i| a[i][i].abs()).sum::<f64>().max(1e-300);
        for (i, row) in a.iter_mut().enumerate() {
            row[i] += 1e-13 * tr;
        }
        let mut lam = r;
        if !solve_dense(&mut a, &mut lam) {
            return None;
        }
        let mut delta = vec![0.0; self.vars.len()];
        for (i, row) in cmat.iter().enumerate() {
            for (j, cij) in row {
                delta[*j] -= minv[*j] * cij * lam[i];
            }
        }
        let mut dke = vec![];
        for (j, (v, pos)) in self.vars.iter().enumerate() {
            if let Some(p) = pos {
                y[*p] += delta[j];
            }
            if m[j] > 1e-12 * m_max {
                let w = vars[v.0 as usize];
                dke.push((*v, 0.5 * m[j] * ((w + delta[j]) * (w + delta[j]) - w * w)));
            }
        }
        Some((violated, dke))
    }
}

struct Loop<'a> {
    model: &'a JitModel,
    prep: &'a Prepared,
    info: RunInfo,
    params: Vec<f64>,
    u: Vec<f64>,
    work: Vec<f64>,
    vars: Vec<f64>,
    impulse: Option<Impulse>,
    cond: Vec<bool>,
    events: Vec<Event>,
    impulse_loss: f64,
}

impl Loop<'_> {
    fn sample(&mut self, t: f64, y: &[f64], d: &[f64]) {
        let inp = EvalInput { t, y, p: &self.params, d, u: &self.u };
        self.model.vars(&inp, &mut self.work, &mut self.vars);
    }

    fn conditions(&mut self, t: f64, y: &[f64], d: &[f64]) -> Vec<bool> {
        let n = self.model.layout().n_roots;
        let mut g = vec![0.0; n];
        let inp = EvalInput { t, y, p: &self.params, d, u: &self.u };
        self.model.roots(&inp, &mut self.work, &mut g);
        self.info
            .whens
            .iter()
            .map(|(k, dir)| match dir {
                Direction::Rising => g[*k] > 0.0,
                Direction::Falling => g[*k] < 0.0,
                Direction::Both => g[*k] != 0.0,
            })
            .collect()
    }

    /// Books an impulsive engagement: each mass's change of kinetic energy
    /// is the work the impulse did on it (its port energy), and what they
    /// lost together is dissipated in the part whose constraint changed.
    fn book_impulse(&mut self, c: usize, dke: &[(VarId, f64)], y: &mut [f64], t: f64) {
        let pos =
            |v: Option<VarId>| v.and_then(|v| self.prep.model.states.iter().position(|s| *s == v));
        let mut total = 0.0;
        for (v, e) in dke {
            total += e;
            let owner = self
                .prep
                .stored
                .iter()
                .find(|(_, s)| s.any(&mut |x| matches!(x, Expr::Var(w) if w == v)))
                .map(|(i, _)| *i);
            if let Some(m) = owner.and_then(|i| self.prep.meters.iter().find(|m| m.instance == i))
                && let Some(p) = pos(m.port_energy)
            {
                y[p] += e;
            }
        }
        let inst = self.prep.constraints[c].instance;
        if let Some(m) = self.prep.meters.iter().find(|m| m.instance == inst) {
            if let Some(p) = pos(m.port_energy) {
                y[p] -= total;
            }
            if let Some(p) = pos(m.loss_energy) {
                y[p] -= total;
            }
        }
        let who = self.prep.model.flat.instance_name(inst);
        self.impulse_loss += -total;
        self.events.push(Event {
            t,
            what: format!("{who}: rigid engagement, {:.6e} J of kinetic energy dissipated", -total),
        });
    }

    /// Runs the event iteration at `t` starting with the clauses `fired`;
    /// returns whether anything changed (the integrator was restarted).
    fn event(
        &mut self,
        integ: &mut Sundials<'_>,
        t: f64,
        mut fired: Vec<f64>,
        max_iter: usize,
        force_restart: bool,
    ) -> Result<bool, String> {
        let mut y = integ.y().to_vec();
        let mut d = integ.discrete_mut().to_vec();
        self.sample(t, &y, &d);
        let before = self.vars.clone();
        // the conditions at the left limit; the clauses that fired were false
        self.cond = self.conditions(t, &y, &d);
        for (k, f) in fired.iter().enumerate() {
            if *f != 0.0 {
                self.cond[k] = false;
            }
        }
        let mut changed = force_restart;
        for iter in 0..=max_iter {
            if iter == max_iter {
                return Err(format!(
                    "events at t = {t} s did not settle after {max_iter} iterations: {}",
                    self.events
                        .iter()
                        .rev()
                        .take(6)
                        .map(|e| e.what.clone())
                        .collect::<Vec<_>>()
                        .join("; ")
                ));
            }
            let mut d_new = d.clone();
            if fired.iter().any(|f| *f != 0.0) {
                let inp = EvalInput { t, y: &y, p: &self.params, d: &d, u: &self.u };
                self.model.when(&inp, &fired, &mut self.work, &mut d_new);
                for (k, f) in fired.iter().enumerate() {
                    if *f != 0.0 {
                        self.events.push(Event { t, what: self.info.when_labels[k].clone() });
                        self.cond[k] = true;
                    }
                }
            }
            let d_changed = d_new != d;
            if !d_changed && !changed {
                self.cond = self.conditions(t, &y, &d);
                break;
            }
            if d_changed || iter == 0 {
                d = d_new;
                integ.discrete_mut().copy_from_slice(&d);
                if let Some(imp) = &self.impulse {
                    let mut probe = before.clone();
                    for (k, v) in self.prep.model.discretes.iter().enumerate() {
                        probe[v.0 as usize] = d[k];
                    }
                    if let Some((c, dke)) = imp.project(&probe, &self.params, t, &mut y) {
                        self.book_impulse(c, &dke, &mut y, t);
                    }
                }
                integ.restart(t, &y).map_err(|e| e.to_string())?;
                y = integ.y().to_vec();
                changed = true;
            }
            let now = self.conditions(t, &y, &d);
            fired = now
                .iter()
                .zip(&self.cond)
                .map(|(a, b)| if *a && !*b { 1.0 } else { 0.0 })
                .collect();
            self.cond = now;
            if fired.iter().all(|f| *f == 0.0) {
                break;
            }
        }
        Ok(changed)
    }
}

/// Runs a prepared, compiled model.
pub fn simulate(
    prep: &Prepared,
    model: &JitModel,
    params: &[f64],
    spec: &RunSpec,
    blocks: &mut [Sampled],
) -> Result<RunResult, String> {
    let started = Instant::now();
    let l = *model.layout();
    let mut info = RunInfo::from_prepared(&prep.model);
    info.params = params.to_vec();
    let grid = spec.grid;
    let mut y0 = vec![0.0; l.n_y()];
    let mut d0 = vec![0.0; l.n_d];
    model.start(params, &mut y0, &mut d0);
    let u = vec![0.0; l.n_u];
    let mut integ = Sundials::new(model, &info, &spec.solver, grid, &y0, d0, u.clone())
        .map_err(|e| e.to_string())?;
    let d_index = |v: VarId| prep.model.discretes.iter().position(|x| *x == v);
    let mut lp = Loop {
        model,
        prep,
        info,
        params: params.to_vec(),
        u,
        work: vec![0.0; l.n_work],
        vars: vec![0.0; l.n_vars],
        impulse: impulse_data(prep),
        cond: vec![],
        events: vec![],
        impulse_loss: 0.0,
    };
    let times = grid.times();
    let mut rec = Recorder::new(l.n_vars, &times);
    let mut t = grid.t0;
    let mut y = integ.y().to_vec();
    let mut d = integ.discrete_mut().to_vec();
    // the sampled blocks' first outputs
    let mut next_tick: Vec<f64> = vec![];
    let mut restarts_by_blocks = 0;
    if !blocks.is_empty() {
        lp.sample(t, &y, &d);
        let mut d_new = d.clone();
        for b in blocks.iter_mut() {
            let ins: Vec<f64> = b.inputs.iter().map(|v| lp.vars[v.0 as usize]).collect();
            let mut outs: Vec<f64> =
                b.outputs.iter().map(|v| d_index(*v).map(|k| d[k]).unwrap_or(0.0)).collect();
            b.block.init(t, &ins, &mut outs).map_err(|e| format!("{}: {e}", b.block.name()))?;
            for (v, o) in b.outputs.iter().zip(&outs) {
                if let Some(k) = d_index(*v) {
                    d_new[k] = *o;
                }
            }
            next_tick.push(
                b.block.offset().max(t) + if b.block.offset() > t { 0.0 } else { b.block.period() },
            );
        }
        if d_new != d {
            integ.discrete_mut().copy_from_slice(&d_new);
            integ.restart(t, &y).map_err(|e| e.to_string())?;
            y = integ.y().to_vec();
            d = d_new;
        }
    }
    lp.cond = lp.conditions(t, &y, &d);
    lp.sample(t, &y, &d);
    rec.start(t, &lp.vars);
    let mut stopped_at = None;
    let mut stop_prev = spec.stop.map(|(v, _)| lp.vars[v.0 as usize]);
    'outer: while t < grid.t_end {
        let tick = next_tick.iter().copied().fold(f64::INFINITY, f64::min);
        let t_stop = grid.t_end.min(tick);
        let st = integ.step(t_stop).map_err(|e| e.to_string())?;
        let t_new = match &st {
            Step::Internal(t) | Step::Stopped(t) | Step::Root(t, _) => *t,
        };
        d = integ.discrete_mut().to_vec();
        // the stop condition, located on the dense output
        if let (Some((v, level)), Some(prev)) = (spec.stop, stop_prev) {
            y.copy_from_slice(integ.y());
            lp.sample(t_new, &y, &d);
            let now = lp.vars[v.0 as usize];
            if prev < level && now >= level {
                let (mut lo, mut hi) = (t, t_new);
                let mut yy = y.clone();
                for _ in 0..200 {
                    let mid = 0.5 * (lo + hi);
                    if mid <= lo || mid >= hi {
                        break;
                    }
                    integ.interpolate(mid, &mut yy).map_err(|e| e.to_string())?;
                    lp.sample(mid, &yy, &d);
                    if lp.vars[v.0 as usize] >= level {
                        hi = mid;
                    } else {
                        lo = mid;
                    }
                }
                // grid points before the stop
                while let Some(tk) = rec.next_grid_time() {
                    if tk >= hi {
                        break;
                    }
                    integ.interpolate(tk, &mut yy).map_err(|e| e.to_string())?;
                    lp.sample(tk, &yy, &d);
                    rec.grid_point(tk, &lp.vars);
                }
                integ.interpolate(hi, &mut yy).map_err(|e| e.to_string())?;
                lp.sample(hi, &yy, &d);
                rec.grid_point(hi, &lp.vars);
                stopped_at = Some(hi);
                break 'outer;
            }
            stop_prev = Some(now);
        }
        while let Some(tk) = rec.next_grid_time() {
            if tk > t_new || (tk == t_new && matches!(st, Step::Root(..))) {
                break;
            }
            if tk == t_new {
                y.copy_from_slice(integ.y());
            } else {
                integ.interpolate(tk, &mut y).map_err(|e| e.to_string())?;
            }
            lp.sample(tk, &y, &d);
            rec.grid_point(tk, &lp.vars);
        }
        y.copy_from_slice(integ.y());
        lp.sample(t_new, &y, &d);
        rec.interior(t_new, &lp.vars);
        t = t_new;
        let mut changed = false;
        if let Step::Root(te, dirs) = &st {
            let fired: Vec<f64> = lp
                .info
                .whens
                .iter()
                .map(|(k, dir)| {
                    let r = dirs[*k];
                    let hit = match dir {
                        Direction::Rising => r > 0,
                        Direction::Falling => r < 0,
                        Direction::Both => r != 0,
                    };
                    if hit { 1.0 } else { 0.0 }
                })
                .collect();
            changed = lp.event(&mut integ, *te, fired, spec.max_event_iterations, false)?;
        }
        // sampled blocks due now
        if !blocks.is_empty() && t >= tick - 1e-12 * tick.abs().max(1.0) {
            y.copy_from_slice(integ.y());
            d = integ.discrete_mut().to_vec();
            lp.sample(t, &y, &d);
            let mut d_new = d.clone();
            for (b, nt) in blocks.iter_mut().zip(next_tick.iter_mut()) {
                if *nt > t + 1e-12 * t.abs().max(1.0) {
                    continue;
                }
                let ins: Vec<f64> = b.inputs.iter().map(|v| lp.vars[v.0 as usize]).collect();
                let mut outs: Vec<f64> =
                    b.outputs.iter().map(|v| d_index(*v).map(|k| d[k]).unwrap_or(0.0)).collect();
                b.block.tick(t, &ins, &mut outs).map_err(|e| format!("{}: {e}", b.block.name()))?;
                for (v, o) in b.outputs.iter().zip(&outs) {
                    if let Some(k) = d_index(*v) {
                        d_new[k] = *o;
                    }
                }
                *nt += b.block.period();
            }
            if d_new != d {
                integ.discrete_mut().copy_from_slice(&d_new);
                restarts_by_blocks += 1;
                let none = vec![0.0; lp.info.whens.len()];
                lp.event(&mut integ, t, none, spec.max_event_iterations, true)?;
                changed = true;
            }
        }
        if changed {
            y.copy_from_slice(integ.y());
            d = integ.discrete_mut().to_vec();
            lp.sample(t, &y, &d);
            rec.interior(t, &lp.vars);
            if rec.next_grid_time() == Some(t) {
                rec.grid_point(t, &lp.vars);
            }
            if let Some((v, _)) = spec.stop {
                stop_prev = Some(lp.vars[v.0 as usize]);
            }
        }
    }
    let (values, min, max, mean) = rec.finish();
    let n_rec = values.first().map(|v| v.len()).unwrap_or(0);
    let mut out_times: Vec<f64> = times.into_iter().take(n_rec).collect();
    if let (Some(ts), Some(last)) = (stopped_at, out_times.last_mut()) {
        *last = ts;
    }
    Ok(RunResult {
        times: out_times,
        names: prep.model.flat.vars.iter().map(|v| v.name.clone()).collect(),
        values,
        min,
        max,
        mean,
        events: lp.events,
        stats: integ.stats(),
        backend: integ.name(),
        stopped_at,
        wall_seconds: started.elapsed().as_secs_f64(),
        restarts_by_blocks,
        impulse_loss: lp.impulse_loss,
    })
}
