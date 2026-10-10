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
//! * **`when` clauses** follow Modelica: a clause fires at each instant its
//!   condition changes from false to true while the model runs, compared
//!   with its value just before that instant (before a sample tick set
//!   its outputs, say). Nothing fires at the start: the initialisation
//!   takes every condition as it is there (Modelica's `pre(c) = c` after
//!   initialisation; sampled blocks' initial outputs are start values
//!   too), so a condition already true at the start fires only once it
//!   has been false. One exactly at its threshold counts as written:
//!   `x >= 0` holds at zero, a strict `x > 0` does not, and fires as x
//!   leaves zero ([`RunInfo::when_strict`]). What must hold from the
//!   start belongs in the start values (the IR has no `initial()`). A
//!   sample tick at the start time is an event after the initialisation
//!   like any other.
//! * **Time events** ([`RunInfo::time_events`]) are reached exactly as stop
//!   times and restart the integrator. So are **time crossings**
//!   ([`RunInfo::time_crossings`]): a zero-crossing function that depends
//!   on time only between events (`when time >= t_shift`, a mode of `if
//!   time > t_on`) is not left to root finding, which lands a few ulps
//!   after it; the run loop computes its time from the parameters and
//!   discrete values, stops there exactly and fires it in its direction.
//!   Its modes take their values just after the instant (`time > t1` is
//!   true from the event at `t1` on) while its time, with the discrete
//!   values the event iteration has set, is still that instant: a `when`
//!   that moves the time on at its own instant (`t_next := t_next + 0.5`)
//!   leaves the relation as it stands there, one that sets it to now
//!   (`t_last := time`) makes `time > t_last` true right after. At every
//!   scheduled event (and at a root) sample ticks due at that instant join
//!   the event, and an output point there shows the values just after it;
//!   a root the integrator locates within a few ulps of a scheduled time
//!   (inside its root tolerance, it may report the root instead of the
//!   stop time) is that instant, and the time events and crossings due
//!   there join its event.
//! * **Sampled blocks** ([`DiscreteBlock`], DESIGN.md risk R1) tick at
//!   `offset + k·period`. A tick that falls inside a step is evaluated on
//!   the dense output; if its outputs did not change, nothing else happens
//!   (no restart, no shortened step: a block that changes nothing costs one
//!   interpolation of its inputs and its own call). If they changed, the
//!   step is cut back to the tick: the integrator restarts there with the
//!   new values, and the block's next tick becomes a stop time until a
//!   tick changes nothing again.
//! * **Impulses**: when an event changes how the states map onto the
//!   velocities the parts' stored energies weigh (a gearbox shifts), the
//!   states jump to the nearest consistent ones in that metric: the
//!   momentum of everything the rigid couplings tie together is kept, as
//!   a perfectly inelastic engagement keeps it, through every coupling
//!   that declares it passes an impulse on (a tyre that grips, keeping its
//!   slip velocity) and through none with bounded forces (a slipping
//!   clutch). The kinetic energy that loses is booked as lost at the
//!   event: a coupling that passed the impulse on books what it
//!   dissipated across the velocity it keeps (a tyre's slip share), the
//!   part whose coupling changed the rest ([`crate::ImpulseInfo`]).
//! * **Event storms**: more than [`SolverOptions::storm_events`] state
//!   events in [`SolverOptions::storm_window`] of the run stop it, naming
//!   the conditions (and so the parts) that chatter. Sample ticks and time
//!   events are scheduled, and so is every mode change and `when` they
//!   cause at their instant: none of them counts.
//! * **Modes checked after every step**: a mode whose condition left zero
//!   right after a restart (root finding cannot see a crossing that starts
//!   exactly at zero) is caught at the step's end and flipped there.
//! * **Energy books** ([`crate::energy`]): quadratures read at every grid
//!   point, stored energy before and after every event, the closure at the
//!   end.

use crate::energy::{Engagement, Ledger};
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
    /// the entries of y the inputs read (when no input needs the
    /// channels), and for each input its position among them
    y_idx: Vec<usize>,
    y_vals: Vec<f64>,
    y_pos: Vec<Option<usize>>,
    /// for each input evaluated alone: the positions in `y_vals` of what
    /// its chain reads from y
    chain_pos: Vec<Vec<usize>>,
    /// scratch for the chains: flat variables' values and derivatives,
    /// and the entries of y one chain reads
    vals: Vec<f64>,
    ders: Vec<f64>,
    chain_y: Vec<f64>,
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
    /// the `when` clause each other root function decides (the first one
    /// on it): an exact zero counts as the side its condition holds on
    when_of_root: Vec<Option<usize>>,
    /// per table guard: since when it is outside its data
    outside_since: Vec<Option<f64>>,
    /// per table guard: time spent outside, and when it first left
    outside_total: Vec<(f64, f64)>,
    /// asserts that already warned
    warned: Vec<bool>,
    warnings: Vec<String>,
    /// the event being handled was scheduled (a sample tick, a time event):
    /// the mode changes and `when`s it causes are no state events
    scheduled: bool,
    /// per root function that depends on time only: the time it next
    /// crosses zero, scheduled exactly (NaN: not ahead)
    t_star: Vec<f64>,
    /// per root function: a time crossing whose time, with the discrete
    /// values now, is this instant (its modes take their values just after
    /// it); refreshed by [`Loop::right_limits`]
    now: Vec<bool>,
    /// the backend leaves the time crossings to the run loop
    use_timed: bool,
}

/// Whether two times are one instant to the integrators: closer than a
/// few ulps, an interval they cannot step (SUNDIALS refuses one shorter
/// than 2 ulps: "tout too close to t0").
pub(crate) fn same_instant(a: f64, b: f64) -> bool {
    (a - b).abs() <= 16.0 * f64::EPSILON * a.abs().max(b.abs())
}

/// Maps an exact zero of a root function to the side it counts as, so a
/// function resting at zero after its event does not fire again, and holds
/// the masked ones (time events the run loop schedules) at a constant (the
/// backends call this on every evaluation of the root functions).
pub(crate) fn apply_zero_sides(g: &mut [f64], sides: &[f64], mask: &[bool]) {
    for (g, s) in g.iter_mut().zip(sides) {
        if *g == 0.0 && *s != 0.0 {
            *g = *s * f64::MIN_POSITIVE;
        }
    }
    for (g, m) in g.iter_mut().zip(mask) {
        if *m {
            *g = 1.0;
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
            } else if let Some(k) = self.when_of_root[c] {
                // its condition at zero: `x >= 0` holds there (the positive
                // side), `x > 0` does not (so x leaving zero upwards fires)
                let true_side = if self.info.whens[k].1 == Direction::Falling { -1.0 } else { 1.0 };
                if self.holds(k, 0.0) { true_side } else { -true_side }
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
        self.right_limits(t, y, d);
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

    /// The modes of time crossings whose time is this instant take their
    /// values just after it (a relation `time > t1` is false at `t1`
    /// exactly, true right after: the event at `t1` is where it becomes
    /// true; `time >= t1` agrees). Whether a crossing's time is now is
    /// decided with the discrete values `d` as they are: a `when` that moves
    /// the time on at its own instant (`t_next := t_next + 0.5`) leaves the
    /// relation false, one that sets it to now (`t_last := time`) makes the
    /// strict relation true right after. Refreshes [`Loop::now`].
    fn right_limits(&mut self, t: f64, y: &[f64], d: &mut [f64]) {
        if !self.use_timed {
            return;
        }
        self.sample(t, y, d);
        for (k, tc) in self.info.time_crossings.iter().enumerate() {
            self.now[k] = tc.as_ref().is_some_and(|tc| {
                let env =
                    crate::info::ChannelEnv { t, vars: &self.vars, params: &self.info.params };
                same_instant(lsim_ir::eval::eval(&tc.at, &env), t)
            });
        }
        for m in &self.info.modes {
            if self.now.get(m.crossing).copied().unwrap_or(false) {
                let rising =
                    self.info.time_crossings[m.crossing].as_ref().is_some_and(|c| c.rising);
                d[m.discrete] = if rising { 1.0 } else { 0.0 };
            }
        }
    }

    /// Whether `when` clause `k`'s condition holds where its crossing is
    /// `g`: `x >= 0` (rising, not strict) holds at zero, `x > 0` (strict)
    /// does not; a falling clause's condition is `x <= 0` or `x < 0`.
    fn holds(&self, k: usize, g: f64) -> bool {
        let strict = self.info.when_strict.get(k).copied().unwrap_or(false);
        match self.info.whens[k].1 {
            Direction::Rising | Direction::Both => {
                if strict {
                    g > 0.0
                } else {
                    g >= 0.0
                }
            }
            Direction::Falling => {
                if strict {
                    g < 0.0
                } else {
                    g <= 0.0
                }
            }
        }
    }

    /// Whether root function `c` is a time crossing the run loop schedules.
    fn timed(&self, c: usize) -> bool {
        self.use_timed && self.info.time_crossings.get(c).is_some_and(|x| x.is_some())
    }

    /// The next crossing time of every time crossing, from the parameters
    /// and the discrete values now (the channels just sampled at `t`).
    fn schedule(&mut self, t: f64) {
        if !self.use_timed {
            return;
        }
        for (k, tc) in self.info.time_crossings.iter().enumerate() {
            self.t_star[k] = match tc {
                Some(tc) => {
                    let env =
                        crate::info::ChannelEnv { t, vars: &self.vars, params: &self.info.params };
                    let at = lsim_ir::eval::eval(&tc.at, &env);
                    // (a time within a few ulps of now is this instant:
                    // handled here, not scheduled again)
                    if at > t && !same_instant(at, t) { at } else { f64::NAN }
                }
                None => f64::NAN,
            };
        }
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
        // scheduled, not chattering, and so is what they cause at their
        // instant: a controller switching a mode from one tick to the next)
        if self.scheduled || matches!(kind, EventKind::Block(_) | EventKind::Time(_)) {
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

    /// Event iteration at `t` (Modelica's): fires the `when` clauses whose
    /// crossings the integrator reported (`dirs`), sets the modes they
    /// flip, then re-evaluates every condition with the new discrete values
    /// until nothing changes. A `when` fires when its condition changes
    /// from false to true at this instant: each condition is compared with
    /// its value before anything changed here, with the discrete values
    /// `d_pre` (before a sample tick set its outputs, say), not with the
    /// values `d` already holds on entry. Whenever a discrete value has
    /// changed, the iteration variables in `y` are solved again (the states
    /// held) before anything reads them: the `when` values, the modes'
    /// relations and the conditions all see the algebraic equations as
    /// they hold for the new discrete values. Returns whether a discrete
    /// value changed from `d_pre`.
    #[allow(clippy::too_many_arguments)]
    fn iterate(
        &mut self,
        integ: &mut dyn Integrator,
        t: f64,
        y: &mut [f64],
        d: &mut Vec<f64>,
        d_pre: &[f64],
        dirs: Option<&[i32]>,
    ) -> Result<bool, SolveError> {
        let info = self.info;
        let n_whens = info.whens.len();
        let mut fired = vec![0.0; n_whens];
        let mut any_fired = false;
        // the conditions just before the event
        self.eval_roots(t, y, d_pre, true);
        // the discrete values y's iteration variables are consistent with
        let mut z_for: Vec<f64> = d_pre.to_vec();
        // the clauses on crossings the integrator reported: they fire (or
        // not) by its direction, and the first round does not check them
        // again
        let mut reported = vec![false; n_whens];
        if let Some(dirs) = dirs {
            for (k, (c, dir)) in info.whens.iter().enumerate() {
                let r = dirs.get(*c).copied().unwrap_or(0);
                reported[k] = r != 0;
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
            // the modes of time crossings at this instant: their values
            // just after it (a `when` of this event that moves the time on
            // undoes it below, in the modes' relations)
            let held = d.clone();
            self.right_limits(t, y, d);
            for (k, m) in info.modes.iter().enumerate() {
                if d[m.discrete] != held[m.discrete] {
                    self.record(t, EventKind::Mode(k))?;
                }
            }
            for (k, m) in info.modes.iter().enumerate() {
                let r = dirs.get(m.crossing).copied().unwrap_or(0);
                if r != 0 && !self.timed(m.crossing) {
                    let want = if r > 0 { 1.0 } else { 0.0 };
                    if d[m.discrete] != want {
                        d[m.discrete] = want;
                        self.record(t, EventKind::Mode(k))?;
                    }
                }
            }
        }
        self.rounds(integ, t, y, d, &mut fired, any_fired, &reported, &mut z_for)?;
        self.solve_z(integ, t, y, d, &mut z_for)?;
        Ok(d.as_slice() != d_pre)
    }

    /// Event iteration's rounds at `t`: applies the `when` clauses in
    /// `fired`, sets every mode from its relation, and re-checks every
    /// condition against its value in the round before (on entry:
    /// `self.roots_prev`), until nothing changes. A clause fires when its
    /// condition goes from not holding to holding (Modelica's edge);
    /// `skip`: clauses the first round does not check (decided already).
    #[allow(clippy::too_many_arguments)]
    fn rounds(
        &mut self,
        integ: &mut dyn Integrator,
        t: f64,
        y: &mut [f64],
        d: &mut Vec<f64>,
        fired: &mut [f64],
        mut any_fired: bool,
        skip: &[bool],
        z_for: &mut Vec<f64>,
    ) -> Result<(), SolveError> {
        let info = self.info;
        let mut iterations = 0;
        loop {
            if any_fired {
                self.solve_z(integ, t, y, d, z_for)?;
                let mut d_new = d.clone();
                let inp = EvalInput { t, y, p: &info.params, d, u: self.u };
                self.model.when(&inp, fired, &mut self.work, &mut d_new);
                *d = d_new;
                fired.fill(0.0);
                any_fired = false;
            }
            // every mode from its relation with the new discrete values
            self.solve_z(integ, t, y, d, z_for)?;
            if self.modes_from_relations(t, y, d)? {
                self.solve_z(integ, t, y, d, z_for)?;
            }
            if self.roots.is_empty() {
                break;
            }
            self.eval_roots(t, y, d, false);
            let mut again = false;
            let mut flipped = vec![];
            for (k, (c, dir)) in info.whens.iter().enumerate() {
                if iterations == 0 && skip.get(k).copied().unwrap_or(false) {
                    continue;
                }
                let (a, b) = (self.roots_prev[*c], self.roots[*c]);
                let hit = match dir {
                    Direction::Rising | Direction::Falling => !self.holds(k, a) && self.holds(k, b),
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
                if self.now.get(m.crossing).copied().unwrap_or(false) {
                    continue;
                }
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
        Ok(())
    }

    /// Whether a sample tick's change at `t` (the end of the integrator's
    /// last step, `y` the point after the event) is so slight that the
    /// integration can go on with its history: only the block's `outputs`
    /// changed (no mode, no `when`), no condition or table guard changed
    /// side, and the jumps the next step's error test will see are within
    /// a tenth of its budget: the derivatives' jump over the planned step,
    /// `h ‖Δx'‖`, and the iteration variables' jump `‖Δz‖` (left out when
    /// the error test leaves them out), weighted as the test weighs them.
    /// Then the integrator resumes. On return `y`'s iteration variables
    /// are consistent with `d`.
    #[allow(clippy::too_many_arguments)]
    fn slight(
        &mut self,
        integ: &mut dyn Integrator,
        t: f64,
        y: &mut [f64],
        d_pre: &[f64],
        d: &[f64],
        outputs: &[usize],
    ) -> Result<bool, SolveError> {
        let only_outputs =
            d.iter().zip(d_pre).enumerate().all(|(i, (a, b))| a == b || outputs.contains(&i));
        let h = integ.planned_step();
        if !only_outputs || h <= 0.0 {
            return Ok(false);
        }
        let l = *self.model.layout();
        let (n_x, n) = (l.n_x, l.n_y());
        let y_old = integ.y().to_vec();
        if l.n_z > 0 && !self.info.events_read_z {
            integ.consistent_z(t, y, d)?;
        }
        // the conditions keep their sides
        self.eval_raw(t, &y_old, d_pre);
        let before = self.raw.clone();
        self.eval_raw(t, y, d);
        if before
            .iter()
            .zip(&self.raw)
            .any(|(a, b)| (*a > 0.0) != (*b > 0.0) || (*a < 0.0) != (*b < 0.0))
        {
            return Ok(false);
        }
        let mut f0 = vec![0.0; n];
        let mut f1 = vec![0.0; n];
        let p = &self.info.params;
        self.model.residual(
            &EvalInput { t, y: &y_old, p, d: d_pre, u: self.u },
            &mut self.work,
            &mut f0,
        );
        self.model.residual(&EvalInput { t, y, p, d, u: self.u }, &mut self.work, &mut f1);
        let w = |i: usize| {
            1.0 / (self.opts.rtol * y[i].abs() + self.opts.atol * self.info.y_nominal[i])
        };
        let rms = |s: f64, k: usize| if k == 0 { 0.0 } else { (s / k as f64).sqrt() };
        let dx = rms((0..n_x).map(|i| ((f1[i] - f0[i]) * w(i)).powi(2)).sum(), n_x);
        let dz = if self.opts.suppress_algebraic_error {
            0.0
        } else {
            rms((n_x..n).map(|i| ((y[i] - y_old[i]) * w(i)).powi(2)).sum(), n - n_x)
        };
        if !(h * dx <= 0.1 && dz <= 0.1) {
            return Ok(false);
        }
        Ok(integ.resume(t))
    }

    /// The impulse projection at a change of rigid couplings (DESIGN.md,
    /// *Events*; [`crate::ImpulseInfo`]): when the discrete values `d`
    /// (after the event at `t`) map the states onto the variables the
    /// stored energies read differently than `d_pre` did, the states move
    /// to the nearest consistent ones in the metric of the stored energies,
    /// `min ½ ΔUᵀ H ΔU` over the states coupled to what changed (ΔU the
    /// change of those variables from `before`, the channels before the
    /// event; H the stored energies' Hessian, the masses and inertias),
    /// with every active link keeping its relative velocity. For masses
    /// and inertias that keeps the momentum of everything the rigid
    /// couplings tie together, as a perfectly inelastic engagement does
    /// (`w_out⁺ = (J_out w_out⁻ + r J_in w_in⁻) / (J_out + r² J_in)` for a
    /// gear of ratio r between two inertias). Returns where the lost
    /// energy goes, or `None` when nothing moved: each link that passed
    /// the impulse on takes what it dissipated across the relative
    /// velocity it keeps (λ·keep for its impulse λ: a gripping tyre's
    /// slip times the impulse through it), the parts whose coupling
    /// changed the rest. `y`'s states change; its iteration variables
    /// are left for the restart to solve.
    #[allow(clippy::too_many_arguments)]
    fn impulse(
        &mut self,
        integ: &mut dyn Integrator,
        t: f64,
        y: &mut [f64],
        d_pre: &[f64],
        d: &[f64],
        before: &[f64],
    ) -> Result<Option<Engagement>, SolveError> {
        let Some(imp) = self.info.impulse.clone() else { return Ok(None) };
        if !self.opts.impulses || !imp.discretes.iter().any(|&k| d[k] != d_pre[k]) {
            return Ok(None);
        }
        let l = *self.model.layout();
        let n_x = l.n_x;
        let reads_z = l.n_z > 0 && imp.vars.iter().any(|v| v.reads_z);
        if reads_z && !self.info.events_read_z {
            integ.consistent_z(t, y, d)?;
        }
        // the variables after the event with the states held
        self.sample(t, y, d);
        let u0 = self.vars.clone();
        let changed: Vec<usize> = (0..imp.vars.len())
            .filter(|&k| {
                let v = imp.vars[k].var;
                let (a, b) = (before[v], u0[v]);
                (a - b).abs() > 1e-9 * a.abs().max(b.abs()) && a.is_finite() && b.is_finite()
            })
            .collect();
        if changed.is_empty() {
            return Ok(None);
        }
        // what the stored energies weigh: each part's Hessian at `before`
        let params = self.info.params.clone();
        fn chan<'a>(t: f64, vars: &'a [f64], params: &'a [f64]) -> crate::info::ChannelEnv<'a> {
            crate::info::ChannelEnv { t, vars, params }
        }
        let energy = self.info.energy.clone().expect("an impulse needs the energy books' parts");
        let mut work = before.to_vec();
        let mut hess: Vec<Vec<f64>> = Vec::with_capacity(imp.parts.len());
        for (p, at) in &imp.parts {
            let e = energy.parts[*p].stored.as_ref().expect("a stored energy");
            let m = at.len();
            let mut h = vec![0.0; m * m];
            let step = |k: usize| before[imp.vars[at[k]].var].abs().max(1.0);
            let value = |work: &mut [f64], moves: &[(usize, f64)]| {
                for &(k, dx) in moves {
                    work[imp.vars[at[k]].var] += dx;
                }
                let v = lsim_ir::eval::eval(e, &chan(t, work, &params));
                for &(k, dx) in moves {
                    work[imp.vars[at[k]].var] -= dx;
                }
                v
            };
            let e0 = value(&mut work, &[]);
            for a in 0..m {
                let ha = step(a);
                h[a * m + a] = (value(&mut work, &[(a, ha)]) - 2.0 * e0
                    + value(&mut work, &[(a, -ha)]))
                    / (ha * ha);
                for b in 0..a {
                    let hb = step(b);
                    let x = (value(&mut work, &[(a, ha), (b, hb)])
                        - value(&mut work, &[(a, ha), (b, -hb)])
                        - value(&mut work, &[(a, -ha), (b, hb)])
                        + value(&mut work, &[(a, -ha), (b, -hb)]))
                        / (4.0 * ha * hb);
                    h[a * m + b] = x;
                    h[b * m + a] = x;
                }
            }
            // round-off of the differences is no coupling (a body's ½ m v²
            // + m g z has no v-z term, nor a z-z one)
            let top = h.iter().fold(0.0f64, |a, x| a.max(x.abs()));
            for x in h.iter_mut() {
                if x.abs() <= 1e-9 * top {
                    *x = 0.0;
                }
            }
            hess.push(h);
        }
        // the links active before the event
        let active: Vec<bool> = imp
            .links
            .iter()
            .map(|lk| lsim_ir::eval::eval(&lk.active, &chan(t, before, &params)) != 0.0)
            .collect();
        let pos: std::collections::HashMap<usize, usize> =
            imp.vars.iter().enumerate().map(|(k, v)| (v.var, k)).collect();
        // what is coupled to the variables that changed: through the states
        // they depend on, the stored energies that weigh them together and
        // the active links
        let mut var_in = vec![false; imp.vars.len()];
        let mut state_in = vec![false; n_x];
        let mut link_in = vec![false; imp.links.len()];
        let mut todo: Vec<usize> = changed.clone();
        for &k in &changed {
            var_in[k] = true;
        }
        while let Some(k) = todo.pop() {
            for &sx in &imp.vars[k].states {
                if !state_in[sx] {
                    state_in[sx] = true;
                    for (j, v) in imp.vars.iter().enumerate() {
                        if !var_in[j] && v.states.contains(&sx) {
                            var_in[j] = true;
                            todo.push(j);
                        }
                    }
                }
            }
            for (pi, (_, at)) in imp.parts.iter().enumerate() {
                let Some(a) = at.iter().position(|&x| x == k) else { continue };
                let m = at.len();
                for (b, &j) in at.iter().enumerate() {
                    if !var_in[j] && hess[pi][a * m + b] != 0.0 {
                        var_in[j] = true;
                        todo.push(j);
                    }
                }
            }
            for (li, lk) in imp.links.iter().enumerate() {
                if active[li] && lk.vars.iter().any(|v| pos.get(v) == Some(&k)) {
                    link_in[li] = true;
                    for v in &lk.vars {
                        let j = pos[v];
                        if !var_in[j] {
                            var_in[j] = true;
                            todo.push(j);
                        }
                    }
                }
            }
        }
        let vs: Vec<usize> = (0..imp.vars.len()).filter(|&k| var_in[k]).collect();
        let ss: Vec<usize> = (0..n_x).filter(|&j| state_in[j]).collect();
        let links: Vec<usize> = (0..imp.links.len()).filter(|&k| link_in[k]).collect();
        if ss.is_empty() {
            return Ok(None);
        }
        let vat: std::collections::HashMap<usize, usize> =
            vs.iter().enumerate().map(|(i, &k)| (k, i)).collect();
        let (nv, ns, nl) = (vs.len(), ss.len(), links.len());
        // B: the variables' sensitivity to the states (central differences,
        // exact for the linear kinematics of rigid couplings)
        let mut b = vec![0.0; nv * ns];
        let mut yp = y.to_vec();
        for (c, &sx) in ss.iter().enumerate() {
            let h = y[sx].abs().max(self.info.y_nominal[sx]);
            let mut sample = |dx: f64, this: &mut Self| -> Result<Vec<f64>, SolveError> {
                yp.copy_from_slice(y);
                yp[sx] += dx;
                if reads_z {
                    integ.consistent_z(t, &mut yp, d)?;
                }
                this.sample(t, &yp, d);
                Ok(this.vars.clone())
            };
            let up = sample(h, self)?;
            let dn = sample(-h, self)?;
            for (r, &k) in vs.iter().enumerate() {
                if imp.vars[k].states.contains(&sx) || imp.vars[k].reads_z {
                    let v = imp.vars[k].var;
                    b[r * ns + c] = (up[v] - dn[v]) / (2.0 * h);
                }
            }
        }
        // H over the coupled variables
        let mut hm = vec![0.0; nv * nv];
        for (pi, (_, at)) in imp.parts.iter().enumerate() {
            let m = at.len();
            for (a, ka) in at.iter().enumerate() {
                let Some(&ra) = vat.get(ka) else { continue };
                for (bb, kb) in at.iter().enumerate() {
                    if let Some(&rb) = vat.get(kb) {
                        hm[ra * nv + rb] += hess[pi][a * m + bb];
                    }
                }
            }
        }
        // the links' gradients (linear in the velocities)
        let mut g = vec![0.0; nl * nv];
        for (li, &lk) in links.iter().enumerate() {
            let link = &imp.links[lk];
            for v in &link.vars {
                let r = vat[&pos[v]];
                let h = before[*v].abs().max(1.0);
                work.copy_from_slice(before);
                work[*v] += h;
                let up = lsim_ir::eval::eval(&link.keep, &chan(t, &work, &params));
                work[*v] -= 2.0 * h;
                let dn = lsim_ir::eval::eval(&link.keep, &chan(t, &work, &params));
                g[li * nv + r] = (up - dn) / (2.0 * h);
            }
        }
        // the change the event made with the states held
        let du: Vec<f64> =
            vs.iter().map(|&k| u0[imp.vars[k].var] - before[imp.vars[k].var]).collect();
        // the engagement: [BᵀHB Aᵀ; A 0] [Δx; λ] = −[BᵀH ΔU; G ΔU], A = G B
        let n = ns + nl;
        let mut hb = vec![0.0; nv * ns];
        for r in 0..nv {
            for c in 0..ns {
                hb[r * ns + c] = (0..nv).map(|q| hm[r * nv + q] * b[q * ns + c]).sum();
            }
        }
        let mut trip = vec![];
        let mut rhs = vec![0.0; n];
        for i in 0..ns {
            for j in 0..ns {
                let x: f64 = (0..nv).map(|r| b[r * ns + i] * hb[r * ns + j]).sum();
                if x != 0.0 {
                    trip.push((i, j, x));
                }
            }
            rhs[i] = -(0..nv).map(|r| hb[r * ns + i] * du[r]).sum::<f64>();
        }
        for li in 0..nl {
            for j in 0..ns {
                let a: f64 = (0..nv).map(|r| g[li * nv + r] * b[r * ns + j]).sum();
                if a != 0.0 {
                    trip.push((ns + li, j, a));
                    trip.push((j, ns + li, a));
                }
            }
            rhs[ns + li] = -(0..nv).map(|r| g[li * nv + r] * du[r]).sum::<f64>();
        }
        let Some(sol) = crate::init::lin_solve(n, &trip, &rhs) else {
            self.warnings.push(format!(
                "at t = {t:.6} s a change of rigid couplings could not keep the momentum (a \
                 singular engagement): the speeds jumped to the new couplings as they stood"
            ));
            return Ok(None);
        };
        for (c, &sx) in ss.iter().enumerate() {
            y[sx] += sol[c];
        }
        // what each link dissipated: its impulse λ times the relative
        // velocity it keeps, at the mean of before and after (the energy's
        // change is ΔUᵀ H Ū; of H ΔU, the links' part is −Gᵀλ)
        let links: Vec<(Option<usize>, f64)> = links
            .iter()
            .enumerate()
            .map(|(li, &lk)| {
                let kept: f64 = (0..nv)
                    .map(|r| {
                        let jump = du[r] + (0..ns).map(|c| b[r * ns + c] * sol[c]).sum::<f64>();
                        g[li * nv + r] * (before[imp.vars[vs[r]].var] + 0.5 * jump)
                    })
                    .sum();
                (imp.links[lk].part, sol[ns + li] * kept)
            })
            .collect();
        // the parts whose coupling changed
        let mut parts: Vec<usize> = changed.iter().filter_map(|&k| imp.vars[k].part).collect();
        parts.sort_unstable();
        parts.dedup();
        Ok(Some(Engagement { changed: parts, links }))
    }

    /// Solves the iteration variables of `y` again (the states held) when
    /// the discrete values changed since they were solved for `z_for`, and
    /// the events read them.
    fn solve_z(
        &mut self,
        integ: &mut dyn Integrator,
        t: f64,
        y: &mut [f64],
        d: &[f64],
        z_for: &mut Vec<f64>,
    ) -> Result<(), SolveError> {
        if self.model.layout().n_z == 0 || z_for.as_slice() == d {
            return Ok(());
        }
        if self.info.events_read_z {
            integ.consistent_z(t, y, d)?;
        }
        z_for.clear();
        z_for.extend_from_slice(d);
        Ok(())
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
            self.right_limits(t, y, d);
            self.eval_roots(t, y, d, false);
            let mut again = false;
            for m in &self.info.modes {
                if self.now.get(m.crossing).copied().unwrap_or(false) {
                    continue;
                }
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

    /// As [`Self::read_inputs`], with the entries of y the inputs read in
    /// `ysel` (input k's at `pos[k]`).
    fn read_inputs_selected(&self, b: usize, t: f64, c: &mut Clock, d: &[f64], out: &mut [f64]) {
        let bi = &self.info.blocks[b];
        for (k, &v) in bi.inputs.iter().enumerate() {
            out[k] = match self.info.var_sources.get(v).copied().unwrap_or(VarSource::Computed) {
                VarSource::Y(_) => c.y_vals[c.y_pos[k].expect("a state input")],
                VarSource::NegY(_) => -c.y_vals[c.y_pos[k].expect("a state input")],
                VarSource::D(i) => d[i],
                VarSource::NegD(i) => -d[i],
                VarSource::U(i) => self.u[i],
                VarSource::Const(x) => x,
                VarSource::Computed => match bi.chains.get(k).and_then(|x| x.as_ref()) {
                    Some(chain) => {
                        c.chain_y.clear();
                        c.chain_y.extend(c.chain_pos[k].iter().map(|p| c.y_vals[*p]));
                        chain.eval(
                            t,
                            &c.chain_y,
                            d,
                            self.u,
                            &self.info.params,
                            &mut c.vals,
                            &mut c.ders,
                        )
                    }
                    None => self.vars[v],
                },
            };
        }
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
        when_of_root: vec![None; l.n_roots + model.table_guard_list().len()],
        outside_since: vec![None; model.table_guard_list().len()],
        outside_total: vec![(0.0, f64::NAN); model.table_guard_list().len()],
        warned: vec![false; info.asserts.len()],
        warnings: vec![],
        scheduled: false,
        t_star: vec![f64::NAN; info.time_crossings.len()],
        now: vec![false; l.n_roots],
        use_timed: false,
    };
    for (k, (c, _)) in info.whens.iter().enumerate() {
        if *c < lp.when_of_root.len() && lp.mode_of_root[*c].is_none() {
            lp.when_of_root[*c].get_or_insert(k);
        }
    }
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
            let mut y_idx: Vec<usize> = vec![];
            let mut at = |i: usize| {
                y_idx.iter().position(|x| *x == i).unwrap_or_else(|| {
                    y_idx.push(i);
                    y_idx.len() - 1
                })
            };
            let chain = |k: usize| bi.chains.get(k).and_then(|c| c.as_ref());
            let y_pos: Vec<Option<usize>> = srcs
                .iter()
                .map(|s| match s {
                    VarSource::Y(i) | VarSource::NegY(i) => Some(at(*i)),
                    _ => None,
                })
                .collect();
            let chain_pos: Vec<Vec<usize>> = (0..srcs.len())
                .map(|k| {
                    chain(k)
                        .map(|c| c.from_y.iter().map(|(_, _, i)| at(*i)).collect())
                        .unwrap_or_default()
                })
                .collect();
            let any_chain = (0..srcs.len()).any(|k| chain(k).is_some());
            let needs_vars = srcs
                .iter()
                .enumerate()
                .any(|(k, s)| *s == VarSource::Computed && chain(k).is_none());
            let needs_y = !y_idx.is_empty();
            Clock {
                y_vals: vec![0.0; y_idx.len()],
                y_idx,
                y_pos,
                chain_pos,
                vals: if any_chain { vec![0.0; l.n_vars] } else { vec![] },
                ders: if any_chain { vec![0.0; l.n_vars] } else { vec![] },
                chain_y: vec![],
                period,
                offset,
                k: k0,
                stop_next: false,
                inputs: vec![0.0; bi.inputs.len()],
                outputs: vec![0.0; bi.outputs.len()],
                needs_vars,
                needs_y,
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
    // time crossings: the integrator leaves them to the run loop, which
    // reaches each exactly as a stop time
    if info.time_crossings.iter().any(Option::is_some) {
        let mut mask = vec![false; lp.sides.len()];
        for (k, tc) in info.time_crossings.iter().enumerate() {
            mask[k] = tc.is_some();
        }
        lp.use_timed = integ.set_root_mask(&mask);
    }
    {
        let d_before = d.clone();
        // a time crossing exactly at the start: its modes start with their
        // values just after it (a condition true from the start)
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
            // the blocks' initial outputs are start values: as in Modelica's
            // initialisation no `when` fires on them (each condition starts
            // as it is), the modes follow them
            let d_init = d.clone();
            lp.iterate(integ, t, &mut y, &mut d, &d_init, None)?;
            integ.discrete_mut().copy_from_slice(&d);
            integ.restart(t, &y)?;
            y.copy_from_slice(integ.y());
        }
    }
    lp.sample(t, &y, &d);
    lp.schedule(t);
    let mut scheduled_for = d.clone();
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
        // the time crossings' times move only when a discrete value does
        if d != scheduled_for {
            lp.schedule(t);
            scheduled_for.copy_from_slice(&d);
        }
        let mut t_stop = t_end;
        if let Some(&te) = info.time_events.get(next_time_event) {
            t_stop = t_stop.min(te);
        }
        for &ts in &lp.t_star {
            if ts > t {
                t_stop = t_stop.min(ts);
            }
        }
        for c in &clocks {
            if c.stop_next {
                t_stop = t_stop.min(c.next());
            }
        }
        // a stop time a few ulps away (a tick that falls just before the
        // end, two clocks' ticks that round apart) is this instant: the
        // integrators cannot step so short an interval, and the solution
        // does not move across it
        let same = same_instant(t, t_stop);
        let st = if same { Step::Stopped(t_stop) } else { integ.step(t_stop)? };
        let t_new = st.time();
        let is_root = matches!(st, Step::Root(..));
        // a scheduled event at the step's end: a time event, a time
        // crossing; ticks and the output point there join its event (the
        // output point takes the value just after it)
        // (a root located within a few ulps of a scheduled time, which the
        // integrator may report instead of the stop time, is that instant:
        // what is scheduled there joins its event)
        let stopped = matches!(st, Step::Stopped(_));
        let due = |ts: f64| ts == t_new || ((stopped || is_root) && same_instant(ts, t_new));
        let at_time_event = info.time_events.get(next_time_event).is_some_and(|&te| due(te));
        let crossing_due = lp.t_star.iter().any(|&ts| due(ts));
        let event_now = is_root || at_time_event || crossing_due;

        // the integrator's error estimate
        if !same && integ.local_error(&mut local) {
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
        // a slight tick change at the step's end that let the integration
        // go on: the point after it (recorded already)
        let mut resumed: Option<Vec<f64>> = None;
        loop {
            let tg = rec.next_grid_time().filter(|&g| g < t_new || (g == t_new && !event_now));
            let mut tick: Option<(usize, f64)> = None;
            for (b, c) in clocks.iter().enumerate() {
                let tk = c.next();
                if (tk < t_new || (tk == t_new && !event_now)) && tick.is_none_or(|(_, x)| tk < x) {
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
                    let mut inputs = std::mem::take(&mut c.inputs);
                    if c.needs_vars || tk == t_new {
                        // the whole state (and the channels)
                        integ.interpolate(tk, &mut yk)?;
                        if c.needs_vars {
                            lp.sample(tk, &yk, &d);
                        }
                        lp.read_inputs(b, &yk, &d, &mut inputs);
                    } else {
                        // only the entries of y the block reads
                        if c.needs_y {
                            integ.interpolate_select(tk, &c.y_idx, &mut c.y_vals)?;
                        }
                        lp.read_inputs_selected(b, tk, c, &d, &mut inputs);
                    }
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
                    if !(c.needs_vars || tk == t_new) {
                        integ.interpolate(tk, &mut yk)?;
                    }
                    let new_outputs = c.outputs.clone();
                    lp.sample(tk, &yk, &d);
                    rec.interior(tk, &lp.vars);
                    let before = ledger.as_mut().map(|lg| lg.before_event(tk, &lp.vars, p));
                    let before_vars = lp.vars.clone();
                    let d_pre = d.clone();
                    for (k, &o) in outs.iter().enumerate() {
                        d[o] = new_outputs[k];
                    }
                    lp.record(tk, crate::EventKind::Block(b))?;
                    lp.scheduled = true;
                    let it = lp.iterate(integ, tk, &mut yk, &mut d, &d_pre, None);
                    lp.scheduled = false;
                    it?;
                    integ.discrete_mut().copy_from_slice(&d);
                    let imp = lp.impulse(integ, tk, &mut yk, &d_pre, &d, &before_vars)?;
                    if imp.is_some() {
                        report.impulses += 1;
                    }
                    lp.update_sides(tk, &yk, &d, None);
                    integ.set_root_sides(&lp.sides);
                    if imp.is_none()
                        && tk == t_new
                        && lp.slight(integ, tk, &mut yk, &d_pre, &d, outs)?
                    {
                        // the integration goes on: the next step's error
                        // test checks what the change does
                        report.light_restarts += 1;
                        after_event(
                            &mut lp,
                            &mut rec,
                            integ,
                            &mut ledger,
                            before,
                            tk,
                            &yk,
                            &d,
                            None,
                        )?;
                        resumed = Some(yk.clone());
                        continue;
                    }
                    integ.restart(tk, &yk)?;
                    y.copy_from_slice(integ.y());
                    after_event(
                        &mut lp,
                        &mut rec,
                        integ,
                        &mut ledger,
                        before,
                        tk,
                        &y,
                        &d,
                        imp.as_ref(),
                    )?;
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
        match &resumed {
            Some(after) => {
                y.copy_from_slice(after);
                lp.sample(t_new, &y, &d);
            }
            None => {
                y.copy_from_slice(integ.y());
                lp.sample(t_new, &y, &d);
                rec.interior(t_new, &lp.vars);
            }
        }
        t = t_new;
        lp.check_asserts(t)?;

        // a mode its condition no longer agrees with (one that left zero
        // right after a restart, where root finding cannot see it)
        if !event_now && !info.modes.is_empty() {
            lp.eval_roots(t, &y, &d, false);
            let stale = info.modes.iter().any(|m| {
                let g = lp.roots[m.crossing];
                !lp.timed(m.crossing)
                    && ((g > 0.0 && d[m.discrete] == 0.0) || (g < 0.0 && d[m.discrete] != 0.0))
            });
            if stale {
                let before = ledger.as_mut().map(|lg| lg.before_event(t, &lp.vars, p));
                let before_vars = lp.vars.clone();
                let d_pre = d.clone();
                if lp.iterate(integ, t, &mut y, &mut d, &d_pre, None)? {
                    integ.discrete_mut().copy_from_slice(&d);
                    let imp = lp.impulse(integ, t, &mut y, &d_pre, &d, &before_vars)?;
                    if imp.is_some() {
                        report.impulses += 1;
                    }
                    lp.update_sides(t, &y, &d, None);
                    integ.set_root_sides(&lp.sides);
                    integ.restart(t, &y)?;
                    y.copy_from_slice(integ.y());
                    after_event(
                        &mut lp,
                        &mut rec,
                        integ,
                        &mut ledger,
                        before,
                        t,
                        &y,
                        &d,
                        imp.as_ref(),
                    )?;
                    continue;
                }
            }
        }

        if event_now {
            let before_vars = lp.vars.clone();
            let before = ledger.as_mut().map(|lg| lg.before_event(t, &before_vars, p));
            // ticks due exactly at the event join it
            let d_pre = d.clone();
            let mut changed = false;
            {
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
            // the time crossings due now cross as scheduled: in their
            // direction, exactly here (with the roots the integrator found
            // at the same instant)
            let timed_dirs: Option<Vec<i32>> = crossing_due.then(|| {
                let mut v = match &st {
                    Step::Root(_, dirs) => dirs.clone(),
                    _ => vec![0; lp.sides.len()],
                };
                v.resize(v.len().max(lp.t_star.len()), 0);
                for (k, &ts) in lp.t_star.iter().enumerate() {
                    if ts == t || same_instant(ts, t) {
                        let rising = info.time_crossings[k].as_ref().is_some_and(|c| c.rising);
                        v[k] = if rising { 1 } else { -1 };
                    }
                }
                v
            });
            let dirs = match (&st, &timed_dirs) {
                (_, Some(v)) => Some(v.as_slice()),
                (Step::Root(_, dirs), _) => Some(dirs.as_slice()),
                _ => None,
            };
            if is_root && let Some(dirs) = dirs {
                lp.guard_crossings(t, dirs)?;
            }
            // a time event or crossing is scheduled; a root (with any ticks
            // at its instant) is a state event
            lp.scheduled = !is_root;
            let it = lp.iterate(integ, t, &mut y, &mut d, &d_pre, dirs);
            lp.scheduled = false;
            changed |= it?;
            if crossing_due {
                for k in 0..lp.t_star.len() {
                    if lp.t_star[k] == t || same_instant(lp.t_star[k], t) {
                        lp.t_star[k] = f64::NAN;
                    }
                }
            }
            if at_time_event {
                while info
                    .time_events
                    .get(next_time_event)
                    .is_some_and(|&te| te <= t || same_instant(te, t))
                {
                    lp.record(t, crate::EventKind::Time(next_time_event))?;
                    next_time_event += 1;
                }
                changed = true;
            }
            let mut imp = None;
            if changed {
                integ.discrete_mut().copy_from_slice(&d);
                imp = lp.impulse(integ, t, &mut y, &d_pre, &d, &before_vars)?;
                if imp.is_some() {
                    report.impulses += 1;
                }
            }
            lp.update_sides(t, &y, &d, dirs);
            integ.set_root_sides(&lp.sides);
            if changed {
                integ.restart(t, &y)?;
                y.copy_from_slice(integ.y());
                after_event(
                    &mut lp,
                    &mut rec,
                    integ,
                    &mut ledger,
                    before,
                    t,
                    &y,
                    &d,
                    imp.as_ref(),
                )?;
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
    impulse: Option<&Engagement>,
) -> Result<(), SolveError> {
    lp.sample(t, y, d);
    rec.interior(t, &lp.vars);
    if let (Some(lg), Some(b)) = (ledger.as_mut(), before) {
        lg.after_event(t, &lp.vars, &lp.info.params, &b, impulse);
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
