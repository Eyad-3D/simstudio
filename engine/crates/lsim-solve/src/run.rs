//! The run loop, for any [`Integrator`].
//!
//! * **Steps** run free: the output grid never constrains them; grid
//!   values come from the dense output, and each interval's min, max and
//!   mean from every point visited ([`crate::Recorder`]).
//! * **Zero crossings** (located by the integrator) fire `when` clauses
//!   and flip modes; then **event iteration** re-evaluates every condition
//!   with the new discrete values until nothing changes (bounded: a
//!   condition that keeps flipping at one instant stops the run naming
//!   it); the integrator restarts only if a discrete value changed.
//! * **Time events** ([`RunInfo::time_events`]) are reached exactly as stop
//!   times and restart the integrator.
//! * **Sampled blocks** ([`DiscreteBlock`], DESIGN.md risk R1) tick at
//!   `offset + k·period`. A tick that falls inside a step is evaluated on
//!   the dense output; if its outputs did not change, nothing else happens
//!   (no restart, no shortened step: a block that changes nothing costs one
//!   interpolation of its inputs and its own call). If they changed, the
//!   step is cut back to the tick: the integrator restarts there with the
//!   new values, and the block's next tick becomes a stop time until a
//!   tick changes nothing again.
//! * **Event storms**: more than [`SolverOptions::storm_events`] state
//!   events in [`SolverOptions::storm_window`] of the run stop it, naming
//!   the conditions (and so the parts) that chatter.
//! * **Modes checked after every step**: a mode whose condition left zero
//!   right after a restart (root finding cannot see a crossing that starts
//!   exactly at zero) is caught at the step's end and flipped there.
//! * **Energy books** ([`crate::energy`]): quadratures read at every grid
//!   point, stored energy before and after every event, the closure at the
//!   end.

use crate::energy::Ledger;
use crate::info::VarSource;
use crate::{
    ErrorEstimate, EventKind, EventRecord, Integrator, OutputGrid, Recorder, RunInfo, SimResult,
    SolveError, SolverOptions, SolverReport, Step,
};
use lsim_ir::prepared::Direction;
use lsim_ir::runtime::{DiscreteBlock, EvalInput, ModelFunctions};
use std::collections::VecDeque;
use std::time::Instant;

/// A sampled block's schedule and buffers.
struct Clock {
    period: f64,
    offset: f64,
    k: u64,
    stop_next: bool,
    inputs: Vec<f64>,
    outputs: Vec<f64>,
    needs_vars: bool,
    needs_y: bool,
}

impl Clock {
    fn next(&self) -> f64 {
        self.offset + self.k as f64 * self.period
    }
}

/// Everything the loop's helpers share.
struct Loop<'a> {
    model: &'a dyn ModelFunctions,
    info: &'a RunInfo,
    opts: &'a SolverOptions,
    u: &'a [f64],
    work: Vec<f64>,
    vars: Vec<f64>,
    roots: Vec<f64>,
    roots_prev: Vec<f64>,
    events: Vec<EventRecord>,
    recent: VecDeque<(f64, String)>,
    window: f64,
    /// for each root function (the model's, then the table guards): the
    /// side an exact zero counts as
    sides: Vec<f64>,
    /// the model's root functions and table guards, unmapped
    raw: Vec<f64>,
    /// the mode whose crossing (or its falling copy) each root function is
    mode_of_root: Vec<Option<usize>>,
    /// per table guard: since when it is outside its data
    outside_since: Vec<Option<f64>>,
    /// per table guard: time spent outside, and when it first left
    outside_total: Vec<(f64, f64)>,
    /// asserts that already warned
    warned: Vec<bool>,
    warnings: Vec<String>,
}

/// Maps an exact zero of a root function to the side it counts as, so a
/// function resting at zero after its event does not fire again (the
/// backends call this on every evaluation of the root functions).
pub(crate) fn apply_zero_sides(g: &mut [f64], sides: &[f64]) {
    for (g, s) in g.iter_mut().zip(sides) {
        if *g == 0.0 && *s != 0.0 {
            *g = *s * f64::MIN_POSITIVE;
        }
    }
}

impl Loop<'_> {
    /// The model's root functions and table guards at (t, y, d), unmapped.
    fn eval_raw(&mut self, t: f64, y: &[f64], d: &[f64]) {
        let nr = self.model.layout().n_roots;
        let inp = EvalInput { t, y, p: &self.info.params, d, u: self.u };
        self.model.roots(&inp, &mut self.work, &mut self.raw[..nr]);
        if self.raw.len() > nr {
            self.model.table_guards(&inp, &mut self.work, &mut self.raw[nr..]);
        }
    }

    /// The sides of exact zeros after an event at `t` (crossings reported
    /// in `dirs`), handed to the integrator.
    fn update_sides(&mut self, t: f64, y: &[f64], d: &[f64], dirs: Option<&[i32]>) {
        if self.raw.is_empty() {
            return;
        }
        self.eval_raw(t, y, d);
        for c in 0..self.raw.len() {
            let g = self.raw[c];
            self.sides[c] = if g > 0.0 {
                1.0
            } else if g < 0.0 {
                -1.0
            } else if let Some(k) = self.mode_of_root[c] {
                if d[self.info.modes[k].discrete] != 0.0 { 1.0 } else { -1.0 }
            } else if let Some(r) = dirs.and_then(|x| x.get(c)).filter(|r| **r != 0) {
                *r as f64
            } else {
                self.sides[c]
            };
        }
    }

    /// Sets every mode from its relation (`ModelFunctions::modes`, which
    /// decides the value exactly at zero), recording the flips.
    fn modes_from_relations(
        &mut self,
        t: f64,
        y: &[f64],
        d: &mut [f64],
    ) -> Result<bool, SolveError> {
        if self.info.modes.is_empty() {
            return Ok(false);
        }
        let before = d.to_vec();
        let inp = EvalInput { t, y, p: &self.info.params, d: &before, u: self.u };
        self.model.modes(&inp, &mut self.work, d);
        let mut changed = false;
        for k in 0..self.info.modes.len() {
            let i = self.info.modes[k].discrete;
            if d[i] != before[i] {
                changed = true;
                self.record(t, EventKind::Mode(k))?;
            }
        }
        Ok(changed)
    }

    /// The model's asserts on the channels just sampled.
    fn check_asserts(&mut self, t: f64) -> Result<(), SolveError> {
        for (k, a) in self.info.asserts.iter().enumerate() {
            let env = crate::info::ChannelEnv { t, vars: &self.vars, params: &self.info.params };
            let v = lsim_ir::eval::eval(&a.condition, &env);
            if v == 0.0 {
                if a.error {
                    return Err(SolveError::Assert { t, message: a.message.clone() });
                }
                if !self.warned[k] {
                    self.warned[k] = true;
                    self.warnings.push(format!("at t = {t:.6} s: {}", a.message));
                }
            }
        }
        Ok(())
    }

    /// Table guards that crossed at `t`: leaving the data on an `Error`
    /// axis stops the run; the others book the time outside.
    fn guard_crossings(&mut self, t: f64, dirs: &[i32]) -> Result<(), SolveError> {
        let nr = self.model.layout().n_roots;
        let guards = self.model.table_guard_list();
        for (k, g) in guards.iter().enumerate() {
            match dirs.get(nr + k).copied().unwrap_or(0) {
                -1 => self.leave_table(k, t, g)?,
                1 => {
                    if let Some(t0) = self.outside_since[k].take() {
                        self.outside_total[k].0 += t - t0;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn leave_table(
        &mut self,
        k: usize,
        t: f64,
        g: &lsim_ir::runtime::TableGuard,
    ) -> Result<(), SolveError> {
        let name = self
            .info
            .table_names
            .get(g.table as usize)
            .cloned()
            .unwrap_or_else(|| format!("table {}", g.table));
        if g.outside == lsim_ir::table::Outside::Error {
            return Err(SolveError::TableOutside {
                t,
                message: format!(
                    "the table '{name}' was read outside its data on its {} axis, which allows no                      values outside (outside = Error): widen the table or keep the operating point inside it",
                    if g.axis == 0 { "first" } else { "second" }
                ),
            });
        }
        if self.outside_since[k].is_none() {
            self.outside_since[k] = Some(t);
            if self.outside_total[k].1.is_nan() {
                self.outside_total[k].1 = t;
            }
        }
        Ok(())
    }
    fn sample(&mut self, t: f64, y: &[f64], d: &[f64]) {
        let inp = EvalInput { t, y, p: &self.info.params, d, u: self.u };
        self.model.vars(&inp, &mut self.work, &mut self.vars);
    }

    fn eval_roots(&mut self, t: f64, y: &[f64], d: &[f64], prev: bool) {
        let inp = EvalInput { t, y, p: &self.info.params, d, u: self.u };
        let out = if prev { &mut self.roots_prev } else { &mut self.roots };
        self.model.roots(&inp, &mut self.work, out);
    }

    fn record(&mut self, t: f64, kind: EventKind) -> Result<(), SolveError> {
        let (when, label) = match kind {
            EventKind::When(k) => (k, self.info.when_labels.get(k).cloned().unwrap_or_default()),
            EventKind::Mode(k) => (usize::MAX, self.info.modes[k].label.clone()),
            EventKind::Block(k) => (
                usize::MAX,
                format!(
                    "{}: its output changed",
                    self.info.blocks.get(k).map(|b| b.name.as_str()).unwrap_or("a sampled block")
                ),
            ),
            EventKind::Time(k) => (usize::MAX, format!("time event {k}")),
        };
        self.events.push(EventRecord { t, when, label: label.clone(), kind });
        // storm detection: state events only (clocks and time events are
        // scheduled, not chattering)
        if matches!(kind, EventKind::Block(_) | EventKind::Time(_)) {
            return Ok(());
        }
        self.recent.push_back((t, label));
        while let Some((t0, _)) = self.recent.front() {
            if t - t0 > self.window {
                self.recent.pop_front();
            } else {
                break;
            }
        }
        if self.recent.len() > self.opts.storm_events {
            let mut counts: Vec<(String, usize)> = vec![];
            for (_, l) in &self.recent {
                match counts.iter_mut().find(|(x, _)| x == l) {
                    Some(c) => c.1 += 1,
                    None => counts.push((l.clone(), 1)),
                }
            }
            counts.sort_by(|a, b| b.1.cmp(&a.1));
            let span = t - self.recent.front().map(|x| x.0).unwrap_or(t);
            let list: Vec<String> = counts.iter().map(|(l, c)| format!("{l} ({c}×)")).collect();
            return Err(SolveError::EventStorm {
                t,
                message: format!(
                    "{} events within {span:.3e} s, from {}. A condition is chattering: give the \
                     part's switching a force- or torque-based condition, or check its parameters.",
                    self.recent.len(),
                    list.join(", ")
                ),
                parts: counts.into_iter().map(|(l, _)| l).collect(),
            });
        }
        Ok(())
    }

    /// Event iteration at `t`: fires the `when` clauses whose crossings the
    /// integrator reported (`dirs`), sets the modes they flip, then
    /// re-evaluates every condition with the new discrete values until
    /// nothing changes. Returns whether a discrete value changed.
    fn iterate(
        &mut self,
        t: f64,
        y: &[f64],
        d: &mut Vec<f64>,
        dirs: Option<&[i32]>,
    ) -> Result<bool, SolveError> {
        let info = self.info;
        let n_whens = info.whens.len();
        let start = d.clone();
        let mut fired = vec![0.0; n_whens];
        let mut any_fired = false;
        self.eval_roots(t, y, d, true);
        if let Some(dirs) = dirs {
            for (k, (c, dir)) in info.whens.iter().enumerate() {
                let r = dirs.get(*c).copied().unwrap_or(0);
                let hit = match dir {
                    Direction::Rising => r > 0,
                    Direction::Falling => r < 0,
                    Direction::Both => r != 0,
                };
                if hit {
                    fired[k] = 1.0;
                    any_fired = true;
                    self.record(t, EventKind::When(k))?;
                }
            }
            for (k, m) in info.modes.iter().enumerate() {
                let r = dirs.get(m.crossing).copied().unwrap_or(0);
                if r != 0 {
                    let want = if r > 0 { 1.0 } else { 0.0 };
                    if d[m.discrete] != want {
                        d[m.discrete] = want;
                        self.record(t, EventKind::Mode(k))?;
                    }
                }
            }
        }
        let mut iterations = 0;
        loop {
            if any_fired {
                let mut d_new = d.clone();
                let inp = EvalInput { t, y, p: &info.params, d, u: self.u };
                self.model.when(&inp, &fired, &mut self.work, &mut d_new);
                *d = d_new;
                fired.fill(0.0);
                any_fired = false;
            }
            // every mode from its relation with the new discrete values
            self.modes_from_relations(t, y, d)?;
            if self.roots.is_empty() {
                break;
            }
            self.eval_roots(t, y, d, false);
            let mut again = false;
            let mut flipped = vec![];
            for (k, (c, dir)) in info.whens.iter().enumerate() {
                let (a, b) = (self.roots_prev[*c], self.roots[*c]);
                let hit = match dir {
                    Direction::Rising => a < 0.0 && b >= 0.0,
                    Direction::Falling => a > 0.0 && b <= 0.0,
                    Direction::Both => (a < 0.0 && b >= 0.0) || (a > 0.0 && b <= 0.0),
                };
                if hit {
                    fired[k] = 1.0;
                    any_fired = true;
                    again = true;
                    flipped.push(info.when_labels[k].clone());
                    self.record(t, EventKind::When(k))?;
                }
            }
            for (k, m) in info.modes.iter().enumerate() {
                let g = self.roots[m.crossing];
                let want = if g > 0.0 {
                    1.0
                } else if g < 0.0 {
                    0.0
                } else {
                    d[m.discrete]
                };
                if d[m.discrete] != want {
                    d[m.discrete] = want;
                    again = true;
                    flipped.push(m.label.clone());
                    self.record(t, EventKind::Mode(k))?;
                }
            }
            if !again {
                break;
            }
            iterations += 1;
            if iterations > self.opts.max_event_iterations {
                return Err(SolveError::EventStorm {
                    t,
                    message: format!(
                        "the event iteration did not settle after {iterations} rounds: {} keep \
                         changing each other at this instant",
                        flipped.join(", ")
                    ),
                    parts: flipped,
                });
            }
            std::mem::swap(&mut self.roots_prev, &mut self.roots);
        }
        Ok(*d != start)
    }

    /// Sets every mode from its condition at the start (no `when` fires at
    /// the start).
    fn settle_modes(&mut self, t: f64, y: &[f64], d: &mut [f64]) -> Result<(), SolveError> {
        if self.info.modes.is_empty() {
            return Ok(());
        }
        for _ in 0..=self.opts.max_event_iterations {
            let before = d.to_vec();
            let inp = EvalInput { t, y, p: &self.info.params, d: &before, u: self.u };
            self.model.modes(&inp, &mut self.work, d);
            self.eval_roots(t, y, d, false);
            let mut again = false;
            for m in &self.info.modes {
                let g = self.roots[m.crossing];
                let want = if g > 0.0 {
                    1.0
                } else if g < 0.0 {
                    0.0
                } else {
                    d[m.discrete]
                };
                if d[m.discrete] != want {
                    d[m.discrete] = want;
                    again = true;
                }
            }
            if !again && d == before.as_slice() {
                return Ok(());
            }
        }
        Err(SolveError::Initialisation {
            t,
            message: "the modes do not settle at the start".into(),
        })
    }

    fn read_inputs(&self, b: usize, y: &[f64], d: &[f64], out: &mut [f64]) {
        for (k, &v) in self.info.blocks[b].inputs.iter().enumerate() {
            out[k] = match self.info.var_sources.get(v).copied().unwrap_or(VarSource::Computed) {
                VarSource::Y(i) => y[i],
                VarSource::NegY(i) => -y[i],
                VarSource::D(i) => d[i],
                VarSource::NegD(i) => -d[i],
                VarSource::U(i) => self.u[i],
                VarSource::Const(c) => c,
                VarSource::Computed => self.vars[v],
            };
        }
    }
}

/// The run loop, for any [`Integrator`]: see the module documentation.
#[allow(clippy::too_many_arguments)]
pub fn run_loop(
    model: &dyn ModelFunctions,
    info: &RunInfo,
    opts: &SolverOptions,
    grid: OutputGrid,
    integ: &mut dyn Integrator,
    u: &[f64],
    blocks: &mut [Box<dyn DiscreteBlock>],
    started: Instant,
) -> Result<SimResult, SolveError> {
    let l = *model.layout();
    let n = l.n_y();
    let times = grid.times();
    let t_end = grid.t_end;
    let span = (t_end - grid.t0).abs();
    let mut rec = Recorder::new(l.n_vars, &times);
    let mut lp = Loop {
        model,
        info,
        opts,
        u,
        work: vec![0.0; l.n_work],
        vars: vec![0.0; l.n_vars],
        roots: vec![0.0; l.n_roots],
        roots_prev: vec![0.0; l.n_roots],
        events: vec![],
        recent: VecDeque::new(),
        window: (opts.storm_window * span).max(1e-6),
        sides: vec![0.0; l.n_roots + model.table_guard_list().len()],
        raw: vec![0.0; l.n_roots + model.table_guard_list().len()],
        mode_of_root: {
            let mut m = vec![None; l.n_roots + model.table_guard_list().len()];
            for (k, md) in info.modes.iter().enumerate() {
                if md.crossing < l.n_roots {
                    m[md.crossing] = Some(k);
                }
                // preparation's falling copy of a mode's crossing
                let copy = md.crossing + 1;
                if copy < l.n_roots
                    && m[copy].is_none()
                    && info.whens.iter().any(|(c, dir)| *c == copy && *dir == Direction::Falling)
                    && !info.modes.iter().any(|o| o.crossing == copy)
                {
                    m[copy] = Some(k);
                }
            }
            m
        },
        outside_since: vec![None; model.table_guard_list().len()],
        outside_total: vec![(0.0, f64::NAN); model.table_guard_list().len()],
        warned: vec![false; info.asserts.len()],
        warnings: vec![],
    };
    let p = &info.params;
    let mut y = vec![0.0; n];
    let mut d = integ.discrete_mut().to_vec();
    let mut ledger = if opts.energy_books {
        info.energy.as_ref().filter(|e| !e.parts.is_empty()).map(|e| Ledger::new(e, &l))
    } else {
        None
    };
    let mut report = SolverReport { rtol: opts.rtol, atol: opts.atol, ..Default::default() };

    // sampled blocks: their clocks
    if blocks.len() != info.blocks.len() {
        return Err(SolveError::Block {
            block: format!("({} given)", blocks.len()),
            t: grid.t0,
            message: format!("the model has {} sampled blocks", info.blocks.len()),
        });
    }
    let mut clocks: Vec<Clock> = blocks
        .iter()
        .zip(&info.blocks)
        .map(|(b, bi)| {
            let period = b.period();
            let offset = b.offset();
            // the first tick at or after the start
            let k0 = if offset >= grid.t0 {
                0
            } else {
                ((grid.t0 - offset) / period - 1e-9).ceil().max(0.0) as u64
            };
            let srcs: Vec<VarSource> = bi
                .inputs
                .iter()
                .map(|&v| info.var_sources.get(v).copied().unwrap_or(VarSource::Computed))
                .collect();
            Clock {
                period,
                offset,
                k: k0,
                stop_next: false,
                inputs: vec![0.0; bi.inputs.len()],
                outputs: vec![0.0; bi.outputs.len()],
                needs_vars: srcs.contains(&VarSource::Computed),
                needs_y: srcs.iter().any(|s| matches!(s, VarSource::Y(_) | VarSource::NegY(_))),
            }
        })
        .collect();
    for c in &clocks {
        if c.period.is_nan() || c.period <= 0.0 || !c.period.is_finite() {
            return Err(SolveError::Block {
                block: "a sampled block".into(),
                t: grid.t0,
                message: format!("its period must be positive, not {}", c.period),
            });
        }
    }

    // the start: modes from their conditions, blocks' initial outputs
    y.copy_from_slice(integ.y());
    let mut t = grid.t0;
    {
        let d_before = d.clone();
        lp.settle_modes(t, &y, &mut d)?;
        lp.sample(t, &y, &d);
        for (b, blk) in blocks.iter_mut().enumerate() {
            let c = &mut clocks[b];
            let mut inputs = std::mem::take(&mut c.inputs);
            lp.read_inputs(b, &y, &d, &mut inputs);
            for (k, &o) in info.blocks[b].outputs.iter().enumerate() {
                c.outputs[k] = d[o];
            }
            blk.init(t, &inputs, &mut c.outputs).map_err(|m| SolveError::Block {
                block: blk.name().into(),
                t,
                message: m,
            })?;
            for (k, &o) in info.blocks[b].outputs.iter().enumerate() {
                d[o] = c.outputs[k];
            }
            c.inputs = inputs;
        }
        if d != d_before {
            lp.iterate(t, &y, &mut d, None)?;
            integ.discrete_mut().copy_from_slice(&d);
            integ.restart(t, &y)?;
            y.copy_from_slice(integ.y());
        }
    }
    lp.sample(t, &y, &d);
    rec.start(t, &lp.vars);
    if let Some(lg) = ledger.as_mut() {
        lg.start(t, &lp.vars, p);
    }
    lp.check_asserts(t)?;
    // table guards already outside at the start
    let guards = model.table_guard_list();
    if !guards.is_empty() {
        lp.eval_raw(t, &y, &d);
        for (k, g) in guards.iter().enumerate() {
            if lp.raw[l.n_roots + k] < 0.0 {
                lp.leave_table(k, t, g)?;
            }
        }
    }
    lp.update_sides(t, &y, &d, None);
    integ.set_root_sides(&lp.sides);

    let mut next_time_event = 0;
    while next_time_event < info.time_events.len() && info.time_events[next_time_event] <= t {
        next_time_event += 1;
    }
    let mut local = vec![0.0; n];
    let mut acc = vec![0.0; n];
    let mut ymax: Vec<f64> = y.iter().map(|v| v.abs()).collect();
    let mut yk = vec![0.0; n];

    while t < t_end {
        let mut t_stop = t_end;
        if let Some(&te) = info.time_events.get(next_time_event) {
            t_stop = t_stop.min(te);
        }
        for c in &clocks {
            if c.stop_next {
                t_stop = t_stop.min(c.next());
            }
        }
        let st = integ.step(t_stop)?;
        let t_new = st.time();
        let is_root = matches!(st, Step::Root(..));

        // the integrator's error estimate
        if integ.local_error(&mut local) {
            let yv = integ.y();
            for i in 0..n {
                ymax[i] = ymax[i].max(yv[i].abs());
                if local[i] > report.error.worst_local {
                    report.error.worst_local = local[i];
                    report.error.worst_local_var = info.y_names.get(i).cloned().unwrap_or_default();
                }
                let tol = opts.rtol * yv[i].abs() + opts.atol * info.y_nominal[i];
                acc[i] += local[i] * tol;
            }
        }

        // grid points and ticks inside the step, in time order
        let mut cut = false;
        loop {
            let tg = rec.next_grid_time().filter(|&g| g < t_new || (g == t_new && !is_root));
            let mut tick: Option<(usize, f64)> = None;
            for (b, c) in clocks.iter().enumerate() {
                let tk = c.next();
                if (tk < t_new || (tk == t_new && !is_root)) && tick.is_none_or(|(_, x)| tk < x) {
                    tick = Some((b, tk));
                }
            }
            match (tg, tick) {
                (None, None) => break,
                (Some(g), Some((_, tk))) if g < tk => {
                    grid_point(&mut lp, &mut rec, integ, &mut ledger, g, &mut yk, &d)?;
                }
                (Some(g), None) => {
                    grid_point(&mut lp, &mut rec, integ, &mut ledger, g, &mut yk, &d)?;
                }
                (_, Some((b, tk))) => {
                    report.block_ticks += 1;
                    let c = &mut clocks[b];
                    if c.needs_y || c.needs_vars || tk == t_new {
                        integ.interpolate(tk, &mut yk)?;
                    }
                    if c.needs_vars {
                        lp.sample(tk, &yk, &d);
                    }
                    let mut inputs = std::mem::take(&mut c.inputs);
                    lp.read_inputs(b, &yk, &d, &mut inputs);
                    let outs = &info.blocks[b].outputs;
                    for (k, &o) in outs.iter().enumerate() {
                        c.outputs[k] = d[o];
                    }
                    let blk = &mut blocks[b];
                    blk.tick(tk, &inputs, &mut c.outputs).map_err(|m| SolveError::Block {
                        block: blk.name().into(),
                        t: tk,
                        message: m,
                    })?;
                    c.inputs = inputs;
                    c.k += 1;
                    let changed = outs.iter().enumerate().any(|(k, &o)| d[o] != c.outputs[k]);
                    c.stop_next = changed;
                    if !changed {
                        continue;
                    }
                    // the outputs changed: an event at the tick; the rest of
                    // the step is dropped
                    report.block_changes += 1;
                    if !(c.needs_y || c.needs_vars || tk == t_new) {
                        integ.interpolate(tk, &mut yk)?;
                    }
                    let new_outputs = c.outputs.clone();
                    lp.sample(tk, &yk, &d);
                    rec.interior(tk, &lp.vars);
                    let before = ledger.as_mut().map(|lg| lg.before_event(tk, &lp.vars, p));
                    for (k, &o) in outs.iter().enumerate() {
                        d[o] = new_outputs[k];
                    }
                    lp.record(tk, crate::EventKind::Block(b))?;
                    lp.iterate(tk, &yk, &mut d, None)?;
                    integ.discrete_mut().copy_from_slice(&d);
                    lp.update_sides(tk, &yk, &d, None);
                    integ.set_root_sides(&lp.sides);
                    integ.restart(tk, &yk)?;
                    y.copy_from_slice(integ.y());
                    after_event(&mut lp, &mut rec, integ, &mut ledger, before, tk, &y, &d)?;
                    t = tk;
                    cut = true;
                    break;
                }
            }
        }
        if cut {
            continue;
        }

        // the step's end point, for min/max/mean, and the asserts
        y.copy_from_slice(integ.y());
        lp.sample(t_new, &y, &d);
        rec.interior(t_new, &lp.vars);
        t = t_new;
        lp.check_asserts(t)?;

        // a mode its condition no longer agrees with (one that left zero
        // right after a restart, where root finding cannot see it)
        if !is_root && !info.modes.is_empty() {
            lp.eval_roots(t, &y, &d, false);
            let stale = info.modes.iter().any(|m| {
                let g = lp.roots[m.crossing];
                (g > 0.0 && d[m.discrete] == 0.0) || (g < 0.0 && d[m.discrete] != 0.0)
            });
            if stale {
                let before = ledger.as_mut().map(|lg| lg.before_event(t, &lp.vars, p));
                if lp.iterate(t, &y, &mut d, None)? {
                    integ.discrete_mut().copy_from_slice(&d);
                    lp.update_sides(t, &y, &d, None);
                    integ.set_root_sides(&lp.sides);
                    integ.restart(t, &y)?;
                    y.copy_from_slice(integ.y());
                    after_event(&mut lp, &mut rec, integ, &mut ledger, before, t, &y, &d)?;
                    continue;
                }
            }
        }

        let at_time_event = matches!(st, Step::Stopped(_))
            && info.time_events.get(next_time_event).is_some_and(|&te| te == t_new);
        if is_root || at_time_event {
            let before_vars = lp.vars.clone();
            let before = ledger.as_mut().map(|lg| lg.before_event(t, &before_vars, p));
            // ticks due exactly at a root join its event
            let mut changed = false;
            if is_root {
                for (b, c) in clocks.iter_mut().enumerate() {
                    if c.next() == t {
                        report.block_ticks += 1;
                        let mut inputs = std::mem::take(&mut c.inputs);
                        lp.read_inputs(b, &y, &d, &mut inputs);
                        let outs = &info.blocks[b].outputs;
                        for (k, &o) in outs.iter().enumerate() {
                            c.outputs[k] = d[o];
                        }
                        let blk = &mut blocks[b];
                        blk.tick(t, &inputs, &mut c.outputs).map_err(|m| SolveError::Block {
                            block: blk.name().into(),
                            t,
                            message: m,
                        })?;
                        c.inputs = inputs;
                        c.k += 1;
                        let ch = outs.iter().enumerate().any(|(k, &o)| d[o] != c.outputs[k]);
                        c.stop_next = ch;
                        if ch {
                            report.block_changes += 1;
                            for (k, &o) in outs.iter().enumerate() {
                                d[o] = c.outputs[k];
                            }
                            lp.record(t, crate::EventKind::Block(b))?;
                            changed = true;
                        }
                    }
                }
            }
            let dirs = match &st {
                Step::Root(_, dirs) => Some(dirs.as_slice()),
                _ => None,
            };
            if let Some(dirs) = dirs {
                lp.guard_crossings(t, dirs)?;
            }
            changed |= lp.iterate(t, &y, &mut d, dirs)?;
            if at_time_event {
                while info.time_events.get(next_time_event).is_some_and(|&te| te <= t) {
                    lp.record(t, crate::EventKind::Time(next_time_event))?;
                    next_time_event += 1;
                }
                changed = true;
            }
            lp.update_sides(t, &y, &d, dirs);
            integ.set_root_sides(&lp.sides);
            if changed {
                integ.discrete_mut().copy_from_slice(&d);
                integ.restart(t, &y)?;
                y.copy_from_slice(integ.y());
                after_event(&mut lp, &mut rec, integ, &mut ledger, before, t, &y, &d)?;
            } else if rec.next_grid_time() == Some(t) {
                grid_point(&mut lp, &mut rec, integ, &mut ledger, t, &mut yk, &d)?;
            }
        }
    }

    // the end
    let (values, min, max, mean) = rec.finish();
    let energy = match ledger {
        Some(mut lg) => {
            integ.quadrature(t_end, &mut lg.q)?;
            let ye = integ.y().to_vec();
            lp.sample(t_end, &ye, &d);
            Some(lg.finish(t_end, &lp.vars, p))
        }
        None => None,
    };
    let mut accumulated: Vec<(String, f64)> = (0..n)
        .map(|i| {
            let scale = ymax[i].max(opts.atol * info.y_nominal[i]).max(f64::MIN_POSITIVE);
            (info.y_names.get(i).cloned().unwrap_or_default(), acc[i] / scale)
        })
        .collect();
    accumulated.sort_by(|a, b| b.1.total_cmp(&a.1));
    report.error = ErrorEstimate { accumulated, ..report.error };
    report.backend = integ.name();
    report.method = integ.method();
    report.notes = integ.setup_notes();
    report.stats = integ.stats();
    report.events = lp.events.len();
    // time spent outside tables' data
    for (k, g) in model.table_guard_list().iter().enumerate() {
        if let Some(t0) = lp.outside_since[k].take() {
            lp.outside_total[k].0 += t_end - t0;
        }
        let (total, first) = lp.outside_total[k];
        if total > 0.0 || !first.is_nan() {
            let name = info.table_names.get(g.table as usize).cloned().unwrap_or_default();
            report.warnings.push(format!(
                "the table '{name}' was read outside its data on its {} axis for {total:.6} s, first at t = {first:.6} s",
                if g.axis == 0 { "first" } else { "second" }
            ));
        }
    }
    report.warnings.append(&mut lp.warnings);
    if let Some(e) = &energy {
        report.notes.push(e.summary());
        if e.relative_closure > 1e-6 {
            report.warnings.push(format!(
                "the energy books close to {:.1e} of the throughput, above 1e-6: {}",
                e.relative_closure,
                worst_parts_words(e)
            ));
        }
        if e.relative_drift > (100.0 * opts.rtol).max(1e-6) {
            report.warnings.push(format!(
                "the stored energy drifted {:.1e} of the throughput from its books: the \
                 solution is less accurate than the tolerance suggests; run the 10× tighter check",
                e.relative_drift
            ));
        }
        if opts.energy_tolerance > 0.0 && e.relative_closure > opts.energy_tolerance {
            return Err(SolveError::EnergyBooks {
                message: format!(
                    "the closure is {:.3e} J, {:.2e} of the throughput (allowed {:.1e}); {}",
                    e.closure,
                    e.relative_closure,
                    opts.energy_tolerance,
                    worst_parts_words(e)
                ),
            });
        }
    }
    Ok(SimResult {
        times,
        names: info.var_names.clone(),
        values,
        min,
        max,
        mean,
        events: lp.events,
        stats: report.stats,
        backend: integ.name(),
        options: opts.clone(),
        wall_seconds: started.elapsed().as_secs_f64(),
        energy,
        report,
    })
}

fn worst_parts_words(e: &crate::EnergyBooks) -> String {
    let w: Vec<String> = e
        .worst_parts(3)
        .iter()
        .filter(|p| p.closure != 0.0)
        .map(|p| format!("{} {:+.3e} J", p.name, p.closure))
        .collect();
    if w.is_empty() {
        "no single part stands out".into()
    } else {
        format!("the parts whose books close worst: {}", w.join(", "))
    }
}

/// A grid point inside the last step, from the dense output.
fn grid_point(
    lp: &mut Loop<'_>,
    rec: &mut Recorder,
    integ: &mut dyn Integrator,
    ledger: &mut Option<Ledger>,
    t: f64,
    yk: &mut [f64],
    d: &[f64],
) -> Result<(), SolveError> {
    integ.interpolate(t, yk)?;
    lp.sample(t, yk, d);
    rec.grid_point(t, &lp.vars);
    if let Some(lg) = ledger.as_mut() {
        integ.quadrature(t, &mut lg.q)?;
        lg.grid_point(t, &lp.vars, &lp.info.params);
    }
    Ok(())
}

/// After a restart at an event: the right limit is recorded (and a grid
/// point exactly at the event takes it), the stored-energy jump booked.
#[allow(clippy::too_many_arguments)]
fn after_event(
    lp: &mut Loop<'_>,
    rec: &mut Recorder,
    integ: &mut dyn Integrator,
    ledger: &mut Option<Ledger>,
    before: Option<Vec<f64>>,
    t: f64,
    y: &[f64],
    d: &[f64],
) -> Result<(), SolveError> {
    lp.sample(t, y, d);
    rec.interior(t, &lp.vars);
    if let (Some(lg), Some(b)) = (ledger.as_mut(), before) {
        lg.after_event(t, &lp.vars, &lp.info.params, &b);
    }
    if rec.next_grid_time() == Some(t) {
        rec.grid_point(t, &lp.vars);
        if let Some(lg) = ledger.as_mut() {
            integ.quadrature(t, &mut lg.q)?;
            lg.grid_point(t, &lp.vars, &lp.info.params);
        }
    }
    Ok(())
}
