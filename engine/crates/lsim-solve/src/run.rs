//! The run loop, for any [`Integrator`].
//!
//! * **Steps** run free: the output grid never constrains them; grid
//!   values come from the dense output, and each interval's min, max and
//!   mean from every point visited ([`crate::Recorder`]).
//! * **Zero crossings** (located by the integrator) fire `when` clauses
//!   and flip modes; then **event iteration** re-evaluates every condition
//!   with the new discrete values until nothing changes (bounded: a
//!   condition that keeps flipping at one instant stops the run naming
//!   it); the integrator restarts only if a discrete value changed, or
//!   where the event put a crossing it reported back on the side it came
//!   from (a mode on an iteration variable whose relation, those solved
//!   again, does not hold yet: the integrator meets it again). A
//!   condition on an iteration variable that solving them again puts back
//!   a little short of zero is held on the side it crossed to, so its
//!   `when` fires once ([`Loop::update_sides`]); event iteration evaluates
//!   every condition as held too, so a `when x < 0.5` beside a `when x >
//!   0.5` does not fire at that crossing ([`Loop::hold_conditions`]). The
//!   hold stands only while the integrator's own solution, wherever the
//!   run loop has it (a step's end, an event's point), is still short of
//!   that side within the hold.
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
//!   the event, and an output point there shows the values just after it
//!   (and keeps those just before it: [`crate::SimResult::left_limits`]);
//!   a root the integrator locates within a few ulps of a scheduled time
//!   (inside its root tolerance, it may report the root instead of the
//!   stop time) is that instant, and the time events and crossings due
//!   there join its event.
//! * **Tables read along time** ([`RunInfo::time_tables`]: a driving
//!   cycle's target by time) stop the integrator at each breakpoint
//!   (without a restart): a condition they drive is checked at least
//!   there, even when nothing integrated moves and the steps grow long. A
//!   table whose derivative the model reads (index reduction through it)
//!   stops the integrator where the cell before each breakpoint still
//!   holds and restarts it there: the derivative jumps at the breakpoint,
//!   and no step's history may span the jump.
//! * **Conditions on explicit functions of time**
//!   ([`RunInfo::time_functions`], [`timefn`]): one that reads time,
//!   parameters and discrete values alone is left out of root finding;
//!   its next sign change is found ahead without integrating
//!   ([`crate::interval`]) and reached exactly, as a time crossing's is.
//!   One that reads continuous variables too stays with root finding,
//!   and every step is checked along the integrator's dense output for a
//!   sign change root finding did not see (two inside one step): the step
//!   ends at the first, as at a root.
//! * **Sampled blocks** ([`DiscreteBlock`], DESIGN.md risk R1) tick at
//!   `offset + k·period`. A tick that falls inside a step is evaluated on
//!   the dense output; if its outputs did not change, nothing else happens
//!   (no restart, no shortened step: a block that changes nothing costs one
//!   interpolation of its inputs and its own call), and so it is when the
//!   outputs that changed reach nothing the integrator integrates or
//!   watches ([`RunInfo::dynamic_discretes`]). Otherwise the step is cut
//!   back to the tick: the integrator restarts there with the new values
//!   (or, with [`SolverOptions::light_restarts`], opt-in, goes on with its
//!   history after a slight change at a step's end), and the block's next
//!   tick becomes a stop time until a tick changes nothing again.
//! * **Impulses** ([`impulse`]): at a rigid engagement a part declares (a
//!   gearbox's selected ratio changes), the states jump as an
//!   instantaneous, rigid engagement makes them: the inertias the
//!   engagement ties together meet keeping their momentum (the engaging
//!   part books the kinetic energy that loses), then each coupling a model
//!   declares stiff and unbounded, judged at the state that leaves,
//!   relaxes to its relative velocity before the event. Nothing with
//!   bounded forces passes an impulse in zero time (a tyre, whose force is
//!   at most μ N; a slipping clutch): the integrator follows its relative
//!   velocity after the event with its own law. Nothing but a declared
//!   engagement starts a projection, and what a `reinit` set at the event
//!   stays.
//!   Event iteration then goes on from the moved states, and an
//!   engagement it makes there is projected in turn; a cascade of more
//!   than [`SolverOptions::max_event_iterations`] engagements at one
//!   instant stops the run, naming them.
//! * **Event storms**: more than [`SolverOptions::storm_events`] state
//!   events in [`SolverOptions::storm_window`] of the run stop it, naming
//!   the conditions (and so the parts) that chatter. Sample ticks and time
//!   events are scheduled, and so is every mode change and `when` they
//!   cause at their instant: none of them counts.
//! * **Modes checked after every step**: a mode whose condition left zero
//!   right after a restart (root finding cannot see a crossing that starts
//!   exactly at zero) is caught at the step's end and flipped there.
//! * **Steps that no longer move the time** (shorter than its resolution:
//!   no longer step passes the error test, at a jump the model declares
//!   no event for or where the model turns singular) stop the run after
//!   ten in a row, naming the variable with the largest local error; the
//!   integrators would take them without end.
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

mod impulse;
mod mixed;
mod switch;
mod timefn;

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
    /// an input is a computed channel (with or without its chain)
    reads_vars: bool,
    /// an input reads an iteration variable (an entry of y past the
    /// states, or a channel computed without a chain: any)
    reads_z: bool,
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
    /// for each input evaluated alone: its chain as a tape and the tape's
    /// registers (none where the chain keeps the interpreter)
    tapes: Vec<Option<(crate::tape::ChainTape, Vec<f64>)>>,
    /// where each input is read from
    srcs: Vec<VarSource>,
    /// every input can be read from the dense output, the discrete
    /// values, the inputs or a tape: the clock's ticks inside a step can
    /// be read together ([`Loop::read_ahead`])
    batchable: bool,
    /// the inputs of the clock's next ticks inside the step, read
    /// together: the ticks `batch_k0..` at `batch_ts`, tick after tick
    /// (`batch_in`; `batch_y` the entries `y_idx` of y), valid while the
    /// dense output and the discrete values are those of generation
    /// `batch_gen`
    batch_ts: Vec<f64>,
    batch_y: Vec<f64>,
    batch_in: Vec<f64>,
    batch_k0: u64,
    batch_gen: u64,
    /// per input with a tape: its registers, one lane per tick; and one
    /// value per tick
    lane_regs: Vec<Vec<f64>>,
    lane_out: Vec<f64>,
    /// ticks in a row that changed nothing (how many to read ahead)
    idle_run: usize,
}

/// The most ticks of one clock whose inputs are read together.
const TICK_BATCH: usize = crate::tape::TAPE_LANES;

/// A sampled block's clock: its schedule from `t0` and how its inputs are
/// read in the model `info` describes (of layout `l`).
fn clock_for(
    b: &dyn DiscreteBlock,
    bi: &crate::BlockInfo,
    info: &RunInfo,
    opts: &SolverOptions,
    l: &lsim_ir::runtime::Layout,
    t0: f64,
) -> Clock {
    let period = b.period();
    let offset = b.offset();
    // the first tick at or after the start
    let k0 = if offset >= t0 { 0 } else { ((t0 - offset) / period - 1e-9).ceil().max(0.0) as u64 };
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
            chain(k).map(|c| c.from_y.iter().map(|(_, _, i)| at(*i)).collect()).unwrap_or_default()
        })
        .collect();
    let any_chain = (0..srcs.len()).any(|k| chain(k).is_some());
    let needs_vars =
        srcs.iter().enumerate().any(|(k, s)| *s == VarSource::Computed && chain(k).is_none());
    let needs_y = !y_idx.is_empty();
    let reads_z = needs_vars || y_idx.iter().any(|&i| i >= l.n_x);
    let reads_vars = srcs.contains(&VarSource::Computed);
    let tapes: Vec<Option<(crate::tape::ChainTape, Vec<f64>)>> = (0..srcs.len())
        .map(|k| {
            chain(k)
                .filter(|_| opts.tick_read_ahead && srcs[k] == VarSource::Computed)
                .and_then(|c| crate::tape::ChainTape::new(c, &info.params))
                .map(|t| {
                    let r = t.registers();
                    (t, r)
                })
        })
        .collect();
    Clock {
        reads_z,
        reads_vars,
        y_vals: vec![0.0; y_idx.len()],
        y_idx,
        y_pos,
        chain_pos,
        vals: if any_chain { vec![0.0; l.n_vars] } else { vec![] },
        ders: if any_chain { vec![0.0; l.n_vars] } else { vec![] },
        chain_y: vec![],
        batch_ts: vec![],
        batch_y: vec![],
        batch_in: vec![],
        batch_k0: 0,
        batch_gen: u64::MAX,
        lane_regs: tapes
            .iter()
            .map(|t| t.as_ref().map(|(t, _)| t.lane_registers(TICK_BATCH)).unwrap_or_default())
            .collect(),
        lane_out: vec![0.0; TICK_BATCH],
        idle_run: 0,
        batchable: opts.tick_read_ahead
            && !needs_vars
            && srcs
                .iter()
                .enumerate()
                .all(|(k, s)| *s != VarSource::Computed || tapes[k].is_some()),
        srcs,
        tapes,
        period,
        offset,
        k: k0,
        stop_next: false,
        inputs: vec![0.0; bi.inputs.len()],
        outputs: vec![0.0; bi.outputs.len()],
        needs_vars,
        needs_y,
    }
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
    /// for each root function: how far on the other side of its side it
    /// is still taken as on it (0: not at all; [`Loop::update_sides`])
    hold: Vec<f64>,
    /// some function is held (a hold not 0): kept with `hold`, so that the
    /// step's end looks for holds to release only where there are any
    held: bool,
    /// the model's root functions and table guards, unmapped
    raw: Vec<f64>,
    /// the mode whose crossing (or its falling copy) each root function is
    mode_of_root: Vec<Option<usize>>,
    /// the `when` clause each other root function decides (the first one
    /// on it): an exact zero counts as the side its condition holds on
    when_of_root: Vec<Option<usize>>,
    /// the root functions that are a held table read's cell edges
    /// ([`crate::held`]): positive inside the cell, an exact zero inside
    held_root: Vec<bool>,
    /// the model's root functions an event may hold, in order: neither a
    /// mode's nor a held read's edge ([`Loop::may_hold_root`] but for the
    /// time crossings, which can change)
    hold_roots: Vec<usize>,
    /// where the integrator saw the conditions of the event being iterated
    /// ([`Loop::hold_conditions`]): the point it located it at, the root
    /// functions there (unmapped), the crossings it reported, and the
    /// discrete values before the event
    seen_y: Vec<f64>,
    seen_g: Vec<f64>,
    seen_dirs: Vec<i32>,
    seen_d: Vec<f64>,
    /// the conditions the last release found past their holds
    /// ([`Loop::release_holds`])
    passed: Vec<usize>,
    /// per table guard: since when it is outside its data
    outside_since: Vec<Option<f64>>,
    /// per table guard: time spent outside, and when it first left
    outside_total: Vec<(f64, f64)>,
    /// asserts that already warned
    warned: Vec<bool>,
    /// whether several links shared an engagement's impulse (warned once)
    warned_links: bool,
    warnings: Vec<String>,
    /// the event being handled was scheduled (a sample tick, a time event):
    /// the mode changes and `when`s it causes are no state events
    scheduled: bool,
    /// per root function that depends on time only: the time it next
    /// crosses zero, scheduled exactly (NaN: not ahead)
    t_star: Vec<f64>,
    /// the root functions `t_star` may be set for (the time crossings and
    /// the time functions searched ahead), in order: NaN at every other,
    /// so a step looks at these alone ([`Loop::time_live`])
    t_live: Vec<usize>,
    /// per root function: a time crossing whose time, with the discrete
    /// values now, is this instant (its modes take their values just after
    /// it); refreshed by [`Loop::right_limits`]
    now: Vec<bool>,
    /// the backend leaves the time crossings to the run loop
    use_timed: bool,
    /// the conditions on explicit functions of time ([`timefn`])
    tf: timefn::TimeFns<'a>,
    /// the time tables' breakpoints as times, sorted: those whose
    /// position does not depend on a discrete value, and those that do
    /// (scheduled again when one changes)
    breaks_fixed: Vec<f64>,
    breaks_moving: Vec<f64>,
    /// of those, the stops at kinks ([`crate::info::TimeTable::kink`]),
    /// sorted: the integrator restarts there
    kinks_fixed: Vec<f64>,
    kinks_moving: Vec<f64>,
    /// for each channel the run records (those [`SolverOptions::record`]
    /// names), its index among the channels of the model it is on (after
    /// the run chose its states again, [`switch`], the new model's; `None`:
    /// every channel of the first model, as they are)
    chan: Option<Vec<usize>>,
    /// the recorded channels, read through `chan`
    chan_buf: Vec<f64>,
    /// the point after an event [`after_event`] sampled `vars` at (its
    /// time; the states and discrete values below), while `vars` still
    /// hold those channels ([`Loop::sampled_after`]): `None` once they
    /// change
    after_t: Option<f64>,
    after_y: Vec<f64>,
    after_d: Vec<f64>,
    /// the integrator's internal steps so far (over every integrator the
    /// run went on with)
    steps: u64,
    /// per group of held table reads ([`crate::HeldInfo::group`]): the
    /// step counts at its last crossings (at most
    /// [`crate::held::CROSSINGS_WINDOW`] + 1), and the instant of the last
    crossings: Vec<Vec<u64>>,
    crossed_at: Vec<f64>,
    /// what the run says of itself (the held reads it released): kept
    /// outside the loop, so that a run that fails after a release says so
    notes: &'a std::cell::RefCell<Vec<String>>,
}

/// Whether two times are one instant to the integrators: closer than a
/// few ulps, an interval they cannot step (SUNDIALS refuses one shorter
/// than 2 ulps: "tout too close to t0").
pub(crate) fn same_instant(a: f64, b: f64) -> bool {
    (a - b).abs() <= 16.0 * f64::EPSILON * a.abs().max(b.abs())
}

/// A hold's width ([`Loop::update_sides`]) in what the iteration
/// variables' tolerances allow in its function. The inconsistency it
/// covers measured up to 0.56 of that on conditions on iteration
/// variables (`tests/implicit_conditions.rs`' models over their rates,
/// rtol 1e-4 … 1e-10, 50 holds). An event that moves the condition itself
/// (a `reinit`, a discrete value an iteration variable is solved with) is
/// not held, however little it moves it: with the iteration variables
/// solved for the discrete values before the event, the condition was on
/// the side the integrator saw ([`Loop::keep_unmoved`]).
const HOLD: f64 = 4.0;

/// Steps in a row that leave the time where it was before the run stops
/// (CVODE warns of the first ten, "t + h = t on the next step", and goes
/// on; IDA says nothing).
const STUCK_STEPS: usize = 10;

/// The variable with the largest local error (as a share of its
/// tolerance) in the integrator's last step, in words: an iteration
/// variable with the equations that decide it (those that read it at
/// `(t, y, d)`), and the iteration variables' Jacobian there (`None`: not
/// taken, too many of them). Empty where the integrator gives no estimate.
#[allow(clippy::too_many_arguments)]
fn worst_variable(
    model: &dyn ModelFunctions,
    info: &RunInfo,
    integ: &mut dyn Integrator,
    t: f64,
    y: &[f64],
    d: &[f64],
    u: &[f64],
) -> (String, Option<Vec<Vec<f64>>>) {
    let l = *model.layout();
    let (nx, nz) = (l.n_x, l.n_z);
    let mut jac = None;
    if nz > 0 && nz <= 2000 && y.len() == nx + nz {
        let mut work = vec![0.0; l.n_work];
        let (mut v, mut out) = (vec![0.0; nx + nz], vec![0.0; nx + nz]);
        let inp = EvalInput { t, y, p: &info.params, d, u };
        let mut m = vec![vec![0.0; nz]; nz];
        for j in 0..nz {
            v[nx + j] = 1.0;
            model.jvp(&inp, &v, &mut work, &mut out);
            v[nx + j] = 0.0;
            for i in 0..nz {
                m[i][j] = out[nx + i];
            }
        }
        jac = Some(m);
    }
    let mut local = vec![0.0; nx + nz];
    if !integ.local_error(&mut local) {
        return (String::new(), jac);
    }
    let Some((i, name)) = local
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .and_then(|(i, _)| info.y_names.get(i).map(|v| (i, v.clone())))
    else {
        return (String::new(), jac);
    };
    // an iteration variable: the equations that decide it
    let eqs: Vec<String> = match (&jac, i.checked_sub(nx)) {
        (Some(m), Some(j)) => (0..nz)
            .filter(|&r| m[r][j] != 0.0)
            .take(3)
            .filter_map(|r| info.residual_labels.get(r).cloned())
            .collect(),
        _ => vec![],
    };
    // (the equations' labels name their parts in quotes already)
    let words =
        if eqs.is_empty() { name } else { format!("{name}, decided by {}", eqs.join(" and by ")) };
    (words, jac)
}

/// The error for steps that no longer move the time at `t`, naming the
/// variable with the largest local error (as a share of its tolerance) in
/// the last of them, and the equations that decide it.
#[allow(clippy::too_many_arguments)]
fn no_progress(
    model: &dyn ModelFunctions,
    info: &RunInfo,
    integ: &mut dyn Integrator,
    t: f64,
    y: &[f64],
    d: &[f64],
    u: &[f64],
) -> SolveError {
    let (worst, _) = worst_variable(model, info, integ, t, y, d, u);
    let worst =
        if worst.is_empty() { worst } else { format!(" (the largest local error: {worst})") };
    // a table read at a state, stepped over, at one of its breakpoints:
    // named, with the option that holds it
    let message = match crate::held::at_breakpoint(model, info, t, y, d, u) {
        Some(read) => format!(
            "{} made no progress: no step longer than the time's resolution passes its error \
             test here{worst}. {read}",
            integ.name()
        ),
        None => format!(
            "{} made no progress: no step longer than the time's resolution passes its error \
             test here{worst}. The model is not smooth here (a jump it declares no event for, \
             in a `noEvent` relation or a function that is not smooth), an equation loses its \
             solution (a root that turns back, a source asked for more than it can give), or \
             the model is singular (the states it integrates no longer determine the others: \
             an algebraic loop turns singular)",
            integ.name()
        ),
    };
    SolveError::Integrator { t, message }
}

/// The integrator's own failure `e` (SUNDIALS' flag), told with what the
/// run can see at `(t, y, d)`, the point its last step started from
/// (SUNDIALS' message alone names neither an equation nor a part): where
/// the equations that decide the iteration variables no longer do (their
/// Jacobian, its columns and rows scaled to their largest entries, with a
/// pivot below 1e-8 by full pivoting: rows that depend on each other,
/// parts locked against each other), that equation and variable; else the
/// variable with the largest local error and the equations that decide it,
/// and what can cause it. SUNDIALS' words are kept beside them. A failure
/// of index reduction's choice of states is told by the watch instead
/// ([`crate::states`]).
#[allow(clippy::too_many_arguments)]
fn integrator_failure(
    model: &dyn ModelFunctions,
    info: &RunInfo,
    integ: &mut dyn Integrator,
    t: f64,
    y: &[f64],
    d: &[f64],
    u: &[f64],
    e: SolveError,
) -> SolveError {
    let SolveError::Integrator { t: at, message } = &e else { return e };
    if !message.contains("SUNDIALS flag") {
        return e;
    }
    let nx = model.layout().n_x;
    let (worst, jac) = worst_variable(model, info, integ, t, y, d, u);
    let mut why = None;
    if let Some(mut jac) = jac {
        let finite = jac.iter().all(|r| r.iter().all(|x| x.is_finite()));
        let (p, row, col) = lsim_ir::choice::smallest_pivot(&mut jac);
        let eq = info.residual_labels.get(row).cloned().unwrap_or_else(|| format!("g{row}"));
        let var = info.y_names.get(nx + col).cloned().unwrap_or_else(|| format!("z{col}"));
        if !finite {
            why = Some(format!(
                "the equations that decide the iteration variables are not numbers here: {var} \
                 is decided through a value that is not a number (a division by zero, a root of \
                 a negative number, a table read outside its data), by {eq}"
            ));
        } else if p < 1e-8 {
            why = Some(format!(
                "the equations that decide the iteration variables depend on each other here: \
                 {var} is no longer decided (the smallest pivot is {p:.1e} of its size), by \
                 {eq}: parts locked against each other, or equations that say the same"
            ));
        }
    }
    let why = why.or_else(|| {
        // a table read at a state, stepped over, at one of its breakpoints
        crate::held::at_breakpoint(model, info, t, y, d, u).map(|read| {
            let worst = if worst.is_empty() {
                String::new()
            } else {
                format!(" (the largest local error: {worst})")
            };
            format!("it stopped here{worst}. {read}")
        })
    });
    let why = why.unwrap_or_else(|| {
        let worst =
            if worst.is_empty() { worst } else { format!(" (the largest local error: {worst})") };
        format!(
            "it stopped here{worst}: the model is not smooth here (a jump it declares no event \
             for), an equation loses its solution (a root that turns back, a source asked for \
             more than it can give), or it turns singular"
        )
    });
    SolveError::Integrator {
        t: *at,
        message: format!("{} could not go on: {why} ({message})", integ.name()),
    }
}

/// Maps an exact zero of a root function to the side it counts as, so a
/// function resting at zero after its event does not fire again, and a
/// value on the other side within its hold to that side too (a crossing
/// that re-solving the iteration variables at its event put back a little
/// short of zero: [`Loop::update_sides`]), and holds the masked ones (time
/// events the run loop schedules; their indices) at a constant (the
/// backends call this on every evaluation of the root functions, through
/// [`ZeroSides`]). `holds`: `None` where no function is held (no hold
/// above 0, where a value on the other side never moves: only an exact
/// zero does).
pub(crate) fn apply_zero_sides(
    g: &mut [f64],
    sides: &[f64],
    holds: Option<&[f64]>,
    masked: &[usize],
) {
    match holds {
        Some(holds) => {
            for ((g, s), h) in g.iter_mut().zip(sides).zip(holds) {
                if *s != 0.0 && (*g == 0.0 || (*g * *s < 0.0 && g.abs() <= *h)) {
                    *g = *s * f64::MIN_POSITIVE;
                }
            }
        }
        None => {
            // (an exact zero is rare: a run of values without one is passed
            // over in one look, a vector compare)
            let n = g.len().min(sides.len());
            let mut gs = g[..n].chunks_exact_mut(ZERO_RUN);
            let mut ss = sides[..n].chunks_exact(ZERO_RUN);
            for (g, s) in (&mut gs).zip(&mut ss) {
                if g.iter().fold(false, |zero, x| zero | (*x == 0.0)) {
                    zeros_to_sides(g, s);
                }
            }
            zeros_to_sides(gs.into_remainder(), ss.remainder());
        }
    }
    for &i in masked {
        if let Some(g) = g.get_mut(i) {
            *g = 1.0;
        }
    }
}

/// Whether a function is held: a hold not 0 ([`Loop::held`]; every one
/// looked at, a vector compare).
fn held(hold: &[f64]) -> bool {
    hold.iter().fold(false, |any, h| any | (*h != 0.0))
}

/// The values [`apply_zero_sides`] looks at together for an exact zero.
const ZERO_RUN: usize = 8;

/// Each exact zero of `g` to its side.
fn zeros_to_sides(g: &mut [f64], sides: &[f64]) {
    for (g, s) in g.iter_mut().zip(sides) {
        if *g == 0.0 && *s != 0.0 {
            *g = *s * f64::MIN_POSITIVE;
        }
    }
}

/// What a backend maps its root functions' values with
/// ([`apply_zero_sides`]): the sides and holds the run loop hands it
/// ([`Integrator::set_root_sides`]), and the functions it masks
/// ([`Integrator::set_root_mask`]).
#[derive(Debug, Clone, Default)]
pub(crate) struct ZeroSides {
    side: Vec<f64>,
    hold: Vec<f64>,
    /// a hold is above 0 (else only exact zeros move)
    held: bool,
    /// the masked functions
    masked: Vec<usize>,
}

impl ZeroSides {
    /// For `n` root functions: no side, no hold, none masked.
    pub(crate) fn new(n: usize) -> ZeroSides {
        ZeroSides { side: vec![0.0; n], hold: vec![0.0; n], held: false, masked: vec![] }
    }

    /// The sides and holds from the run loop.
    pub(crate) fn set(&mut self, sides: &[f64], holds: &[f64]) {
        self.side.copy_from_slice(sides);
        self.hold.copy_from_slice(holds);
        // (every one looked at: a vector compare)
        self.held = holds.iter().fold(false, |any, h| any | (*h > 0.0));
    }

    /// The functions masked: those `mask` marks.
    pub(crate) fn mask(&mut self, mask: &[bool]) {
        self.masked = mask.iter().enumerate().filter(|(_, m)| **m).map(|(i, _)| i).collect();
    }

    /// `g`, the root functions' values, mapped.
    pub(crate) fn apply(&self, g: &mut [f64]) {
        apply_zero_sides(g, &self.side, self.held.then_some(self.hold.as_slice()), &self.masked);
    }
}

impl Loop<'_> {
    /// The model's channels at `(t, y, d)`, apart from the ones last
    /// sampled.
    fn vars_at(&self, t: f64, y: &[f64], d: &[f64]) -> Vec<f64> {
        let l = self.model.layout();
        let mut work = vec![0.0; l.n_work];
        let mut out = vec![0.0; l.n_vars];
        let inp = EvalInput { t, y, p: &self.info.params, d, u: self.u };
        self.model.vars(&inp, &mut work, &mut out);
        out
    }

    /// The channels the run records, from the model's channels as last
    /// sampled ([`Loop::chan`]).
    fn recorded(&mut self) -> &[f64] {
        match &self.chan {
            None => &self.vars,
            Some(map) => {
                self.chan_buf.clear();
                self.chan_buf.extend(map.iter().map(|&j| self.vars[j]));
                &self.chan_buf
            }
        }
    }

    /// The channels the run records, from the model's channels `vars`.
    fn recorded_of<'v>(&self, vars: &'v [f64]) -> std::borrow::Cow<'v, [f64]> {
        match &self.chan {
            None => std::borrow::Cow::Borrowed(vars),
            Some(map) => std::borrow::Cow::Owned(map.iter().map(|&j| vars[j]).collect()),
        }
    }

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
    /// in `dirs`), and the holds, for the integrator
    /// ([`Loop::push_sides`]). `before`: the point the integrator located
    /// the event at, and the discrete values before it.
    ///
    /// A **hold**: the integrator locates a crossing on its interpolant of
    /// everything it integrates, the iteration variables too, which agree
    /// with their equations only to its tolerance. Where the event
    /// re-solves the iteration variables from the states (a discrete value
    /// changed: the `when` the crossing fired), a condition on one of them
    /// (`x > 0.5` with `x³ + x = z`) can come out a little short of zero,
    /// back on the side it crossed from; the integrator, restarted there,
    /// sees it cross again a moment later, and the `when` fired twice for
    /// one crossing (or three times). Such a function is held on the side
    /// the integrator saw it on until it reaches that side itself, or goes
    /// back past zero by more than its hold ([`HOLD`] times what the
    /// iteration variables' tolerances allow in it: its change as each is
    /// moved by its tolerance, summed): relation hysteresis, as
    /// OpenModelica holds a relation on its side (`relationhysteresis`,
    /// by 1e-4 rtol of its operands' size, where a condition is evaluated
    /// with its iteration variables solved), here as wide as the
    /// inconsistency it covers. That is looked at wherever the run loop has
    /// the integrator's own solution: at every step's end and at every
    /// event's point, before event iteration reads a condition there
    /// ([`Loop::release_holds`]); one that went back past its hold crossed
    /// back, and its `when` fires there whether or not the integrator,
    /// told it was held, reported it ([`Loop::passed`]). Not where
    /// something else explains the change: the event moved a state (an
    /// engagement), the new discrete values move the function, alone at the
    /// point the integrator had or through the iteration variables (solved
    /// for the discrete values before the event, it was on the side the
    /// integrator saw: [`Loop::keep_unmoved`]), or
    /// the function is a mode's (a mode follows its relation at the event,
    /// and where that sets it back the integrator restarts and sees it
    /// cross again: [`Loop::undone`]). Event iteration, before, evaluates
    /// the conditions held the same way ([`Loop::hold_conditions`]): what
    /// it fires on and what the integrator watches after agree.
    fn update_sides(
        &mut self,
        integ: &mut dyn Integrator,
        t: f64,
        y: &[f64],
        d: &[f64],
        dirs: Option<&[i32]>,
        before: Option<(&[f64], &[f64])>,
    ) {
        self.sides_and_holds(integ, t, y, d, dirs, before);
        self.held = held(&self.hold);
    }

    /// [`Loop::update_sides`] but for [`Loop::held`].
    fn sides_and_holds(
        &mut self,
        integ: &mut dyn Integrator,
        t: f64,
        y: &[f64],
        d: &[f64],
        dirs: Option<&[i32]>,
        before: Option<(&[f64], &[f64])>,
    ) {
        debug_assert_eq!(self.held, held(&self.hold), "Loop::held out of step with the holds");
        if self.raw.is_empty() {
            return;
        }
        let l = *self.model.layout();
        // the side each function was on as the integrator saw it before
        // this instant (with the sides and holds it had), where the event
        // moved no state and re-solved the iteration variables a condition
        // reads
        let seen: Option<Vec<f64>> = before
            .filter(|(y0, _)| {
                l.n_z > 0
                    && self.info.events_read_z
                    && y0[..l.n_x] == y[..l.n_x]
                    && y0[l.n_x..] != y[l.n_x..]
            })
            .map(|(y0, d0)| {
                self.eval_raw(t, y0, d0);
                let mut g = self.raw.clone();
                apply_zero_sides(
                    &mut g,
                    &self.sides,
                    self.held.then_some(self.hold.as_slice()),
                    &[],
                );
                // a `when`'s exact zero: the side its condition takes there
                // ([`Loop::seen_side`], as event iteration takes it)
                for j in 0..self.hold_roots.len() {
                    let c = self.hold_roots[j];
                    if self.raw[c] == 0.0 && self.hold[c] == 0.0 && self.when_of_root[c].is_some() {
                        g[c] = self.seen_side(c, 0.0, 0) * f64::MIN_POSITIVE;
                    }
                }
                g
            });
        // the holds made at earlier events
        let held: Vec<(usize, f64, f64)> = if self.held {
            (0..self.hold.len())
                .filter(|&c| self.hold[c] > 0.0)
                .map(|c| (c, self.sides[c], self.hold[c]))
                .collect()
        } else {
            vec![]
        };
        self.hold.fill(0.0);
        self.eval_raw(t, y, d);
        for c in 0..self.raw.len() {
            let g = self.raw[c];
            self.sides[c] = if g > 0.0 {
                1.0
            } else if g < 0.0 {
                -1.0
            } else if self.held_root[c] {
                // (a held read's argument on its cell's edge: in the cell)
                1.0
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
        // those still short of their side, within their hold (an exact zero
        // too: not on it yet, as a release and the integrator take it; else
        // the side a `when`'s zero is on, `x > 0.5` false there, and the
        // `when` fired again as x went on past it)
        for (c, s, h) in held {
            let g = self.raw[c];
            if g * s <= 0.0 && g.abs() <= h {
                self.sides[c] = s;
                self.hold[c] = h;
            }
        }
        self.hold_cells(t, y, d);
        let (Some(seen), Some((y0, d0))) = (seen, before) else { return };
        // the functions now on the other side of the one the integrator
        // saw them on (crossed to, where it reported a crossing), or on a
        // zero that counts as the other side
        let mut was: Vec<(usize, f64)> = vec![];
        for (c, seen_c) in seen.iter().enumerate() {
            if !self.may_hold_root(c) {
                continue;
            }
            let r = dirs.and_then(|x| x.get(c)).copied().unwrap_or(0);
            let s = if r != 0 { f64::from(r) } else { seen_c.signum() };
            let g = self.raw[c];
            if s != 0.0 && (g * s < 0.0 || (g == 0.0 && self.sides[c] != s)) {
                was.push((c, s));
            }
        }
        if was.is_empty() {
            return;
        }
        let after = self.raw.clone();
        // not the new discrete values' own doing: with them, at the point
        // the integrator had, each is still where it saw it
        self.eval_raw(t, y0, d);
        was.retain(|&(c, s)| self.raw[c] * s >= 0.0);
        if was.is_empty() {
            return;
        }
        let cs: Vec<usize> = was.iter().map(|x| x.0).collect();
        let band = self.band(t, y, d, &after, &cs);
        was.retain(|&(c, _)| after[c].abs() <= HOLD * band[c]);
        if was.is_empty() {
            return;
        }
        // nor the event's change through the iteration variables
        // ([`Loop::keep_unmoved`])
        self.keep_unmoved(integ, t, y0, d0, &mut was);
        for (c, s) in was {
            self.sides[c] = s;
            self.hold[c] = HOLD * band[c];
        }
    }

    /// The edges of the held table reads' cells ([`crate::held`]) at an
    /// event's point: inside their cells (an exact zero too); and an edge
    /// the argument is next to (it just crossed it), where the argument
    /// reads iteration variables, held inside within what their tolerances
    /// allow in it: the integrator makes them consistent again as it
    /// restarts, which can put the argument a little short of the edge,
    /// and a function that starts on the wrong side of zero would not be
    /// seen to cross it when the argument goes back ([`HOLD`], as a
    /// condition on them is held on its side).
    fn hold_cells(&mut self, t: f64, y: &[f64], d: &[f64]) {
        if self.info.held.is_empty() {
            return;
        }
        let l = *self.model.layout();
        let mut near = vec![];
        for h in &self.info.held {
            for ax in &h.axes {
                for (c, edge) in [(ax.below, d[ax.lo]), (ax.above, d[ax.hi])] {
                    if c >= self.raw.len() {
                        continue;
                    }
                    self.sides[c] = 1.0;
                    if l.n_z > 0 && self.raw[c] <= 1e-3 * (1.0 + edge.abs()) {
                        near.push(c);
                    }
                }
            }
        }
        if near.is_empty() {
            return;
        }
        let after = self.raw.clone();
        let band = self.band(t, y, d, &after, &near);
        self.raw = after;
        for &c in &near {
            let h = HOLD * band[c];
            if h > 0.0 && self.raw[c] <= h {
                self.hold[c] = self.hold[c].max(h);
            }
        }
    }

    /// Releases the holds ([`Loop::update_sides`]) of the functions that,
    /// at a point of the integrator's own solution (`t`, `y`: a step's end,
    /// an event's point), reached their held side, or went back past zero
    /// by more than their hold; returns whether one was released. A
    /// condition that went past its hold crossed back from the side the run
    /// took it on: [`Loop::passed`] names those, a crossing there whether or
    /// not the integrator reported it.
    fn release_holds(&mut self, t: f64, y: &[f64], d: &[f64]) -> bool {
        debug_assert_eq!(self.held, held(&self.hold), "Loop::held out of step with the holds");
        self.passed.clear();
        if !self.held {
            return false;
        }
        self.eval_raw(t, y, d);
        let mut released = false;
        for c in 0..self.hold.len() {
            let (g, s, h) = (self.raw[c], self.sides[c], self.hold[c]);
            if h > 0.0 && (g * s > 0.0 || g.abs() > h) {
                if g * s < 0.0 && self.may_hold_root(c) {
                    self.passed.push(c);
                }
                self.hold[c] = 0.0;
                if g != 0.0 {
                    self.sides[c] = g.signum();
                }
                released = true;
            }
        }
        self.held = held(&self.hold);
        released
    }

    /// The crossings [`Loop::passed`] names, in their directions, merged
    /// into those the integrator reported (`dirs`).
    fn with_passed(&self, dirs: Option<&[i32]>) -> Vec<i32> {
        let mut v = dirs.map_or_else(Vec::new, <[i32]>::to_vec);
        v.resize(v.len().max(self.sides.len()), 0);
        for &c in &self.passed {
            if v[c] == 0 {
                v[c] = self.sides[c] as i32;
            }
        }
        v
    }

    /// Whether the sides put a crossing the integrator reported (`dirs`)
    /// back on the side it crossed from.
    fn undone(&self, dirs: &[i32]) -> bool {
        // (a held read's edge the argument crossed is the next cell's then,
        // positive again: nothing undone)
        dirs.iter().enumerate().any(|(c, &r)| {
            r != 0
                && !self.timed(c)
                && !self.held_root.get(c).copied().unwrap_or(false)
                && self.sides.get(c).is_some_and(|s| *s != f64::from(r))
        })
    }

    /// Whether a condition can be held ([`Loop::update_sides`]): the
    /// events re-solve iteration variables a condition reads.
    fn may_hold(&self) -> bool {
        self.model.layout().n_z > 0 && self.info.events_read_z && !self.raw.is_empty()
    }

    /// Whether root function `c` is one an event holds on its side
    /// ([`Loop::update_sides`]): not a mode's (a mode follows its relation
    /// at the event: [`Loop::undone`]), a time crossing's (scheduled
    /// exactly) or a held table read's cell edge ([`Loop::hold_cells`]).
    fn may_hold_root(&self, c: usize) -> bool {
        !(self.mode_of_root.get(c).is_some_and(|m| m.is_some())
            || self.timed(c)
            || self.held_root.get(c).copied().unwrap_or(false))
    }

    /// The side the integrator saw root function `c` on at an event's
    /// point, where its value is `g` (unmapped): the direction it reported
    /// it crossing in (`r`, not 0); else the side a hold keeps it on
    /// ([`Loop::update_sides`]: the other side within the hold, or zero),
    /// or its sign. At an exact zero, the side on which its `when`'s
    /// condition has the value it has at zero (`x > 0.5` is false at zero:
    /// the negative side; `x < 0.5` is false too: the positive side), so
    /// that the side and the condition event iteration evaluates there
    /// agree; else the side zero counts as.
    fn seen_side(&self, c: usize, g: f64, r: i32) -> f64 {
        if r != 0 {
            return f64::from(r);
        }
        let (s, h) = (self.sides[c], self.hold[c]);
        if h > 0.0 && s != 0.0 && g * s <= 0.0 && g.abs() <= h {
            s
        } else if g > 0.0 {
            1.0
        } else if g < 0.0 {
            -1.0
        } else {
            self.zero_side(c)
        }
    }

    /// The side an exact zero of root function `c` is on: the side on
    /// which its `when`'s condition has the value it has at zero; else the
    /// side zero counts as ([`Loop::seen_side`]).
    fn zero_side(&self, c: usize) -> f64 {
        if let Some(k) = self.when_of_root.get(c).copied().flatten() {
            let true_side = if self.info.whens[k].1 == Direction::Falling { -1.0 } else { 1.0 };
            if self.holds(k, 0.0) { true_side } else { -true_side }
        } else {
            self.sides[c]
        }
    }

    /// Of the root functions `cs` that an event left on the other side of
    /// the one the integrator saw each on (that side with it), keeps those
    /// the integrator's own inconsistency put there and drops those the
    /// event moved: at the point the integrator had (`y0`), the iteration
    /// variables solved again for the discrete values before the event
    /// (`d0`), one the event moved is on the side it was seen on (`x³ + x
    /// = 0.5 + 0.125 k`, k set by a time event: x from 0.41 to 0.5, or
    /// across 0.5 by 6e-13), one whose crossing was located a little early
    /// is not (x short of 0.5 before the event too). An exact zero is on
    /// the side its `when`'s condition takes there. Where the iteration
    /// variables cannot be solved there, each is kept.
    fn keep_unmoved(
        &mut self,
        integ: &mut dyn Integrator,
        t: f64,
        y0: &[f64],
        d0: &[f64],
        cs: &mut Vec<(usize, f64)>,
    ) {
        let mut y = y0.to_vec();
        if integ.consistent_z_aside(t, &mut y, d0).is_err() {
            return;
        }
        self.eval_raw(t, &y, d0);
        cs.retain(|&(c, s)| {
            let g = self.raw[c];
            let side = if g > 0.0 {
                1.0
            } else if g < 0.0 {
                -1.0
            } else {
                self.zero_side(c)
            };
            side != s
        });
    }

    /// What the iteration variables' tolerances allow in each root function
    /// `cs` names at `(t, y, d)`, where the functions are `after`: its
    /// change as each iteration variable is moved by its tolerance, summed
    /// ([`HOLD`] times that is a hold's width).
    fn band(&mut self, t: f64, y: &[f64], d: &[f64], after: &[f64], cs: &[usize]) -> Vec<f64> {
        let l = *self.model.layout();
        let mut band = vec![0.0; after.len()];
        let mut yp = y.to_vec();
        for i in l.n_x..l.n_y() {
            let tol = self.opts.rtol * y[i].abs() + self.opts.atol * self.info.y_nominal[i];
            yp[i] = y[i] + tol;
            self.eval_raw(t, &yp, d);
            yp[i] = y[i];
            for &c in cs {
                band[c] += (self.raw[c] - after[c]).abs();
            }
        }
        band
    }

    /// The conditions as event iteration evaluates them (`self.roots`, or
    /// `self.roots_prev` where `prev`, at `(t, y, d)`): as the integrator
    /// sees their root functions, relation hysteresis at events too, so
    /// that what event iteration fires on and what the integrator watches
    /// agree (else a `when x < 0.5` beside a `when x > 0.5` fires at the
    /// rise's crossing too, where x, solved again, comes out a little
    /// short of 0.5). A function a hold keeps on its side
    /// ([`Loop::update_sides`]) is on that side. And at an event (`seen`:
    /// where the integrator located it, [`Loop::seen_y`], and the side it
    /// saw each function on there, [`Loop::seen_side`]), one that solving the
    /// iteration variables again put on the other side of where the
    /// integrator saw it, or on zero, by no more than its hold, and not
    /// by the event's own doing (the new discrete values alone at the point
    /// the integrator had, or through the iteration variables:
    /// [`Loop::keep_unmoved`]), is on the side the integrator saw: the hold
    /// the integrator gets after the event ([`Loop::sides_and_holds`]). A
    /// condition the event moves across (a discrete value an iteration
    /// variable is solved with, a state it moves) is not held: its `when`
    /// fires there.
    fn hold_conditions(
        &mut self,
        integ: &mut dyn Integrator,
        t: f64,
        y: &[f64],
        d: &[f64],
        seen: bool,
        prev: bool,
    ) {
        debug_assert_eq!(self.held, held(&self.hold), "Loop::held out of step with the holds");
        if self.held {
            for c in 0..self.roots.len() {
                let (s, h) = (self.sides[c], self.hold[c]);
                let g = if prev { self.roots_prev[c] } else { self.roots[c] };
                if h > 0.0 && s != 0.0 && g * s <= 0.0 && g.abs() <= h && self.may_hold_root(c) {
                    let g = if prev { &mut self.roots_prev[c] } else { &mut self.roots[c] };
                    *g = s * f64::MIN_POSITIVE;
                }
            }
        }
        if !seen || prev {
            return;
        }
        let l = *self.model.layout();
        if y[..l.n_x] != self.seen_y[..l.n_x] || y[l.n_x..] == self.seen_y[l.n_x..] {
            return;
        }
        let mut cs: Vec<(usize, f64)> = vec![];
        for j in 0..self.hold_roots.len() {
            let c = self.hold_roots[j];
            let (g0, g) = (self.seen_g[c], self.roots[c]);
            let r = self.seen_dirs.get(c).copied().unwrap_or(0);
            // (most: on the side they were seen on, no crossing reported,
            // no hold)
            if r == 0 && self.hold[c] == 0.0 && (g0 * g > 0.0 || (g0 == 0.0 && g == 0.0)) {
                continue;
            }
            let s = self.seen_side(c, g0, r);
            if s != 0.0 && (g * s < 0.0 || (g == 0.0 && g0 != 0.0)) && !self.timed(c) {
                cs.push((c, s));
            }
        }
        if cs.is_empty() {
            return;
        }
        // (with the new discrete values, at the point the integrator had,
        // still where it saw it)
        let y0 = std::mem::take(&mut self.seen_y);
        self.eval_raw(t, &y0, d);
        self.seen_y = y0;
        cs.retain(|&(c, s)| self.raw[c] * s >= 0.0);
        if cs.is_empty() {
            return;
        }
        let after = self.roots.clone();
        let band = self.band(t, y, d, &after, &cs.iter().map(|x| x.0).collect::<Vec<_>>());
        cs.retain(|&(c, _)| after[c].abs() <= HOLD * band[c]);
        if cs.is_empty() {
            return;
        }
        let (y0, d0) = (std::mem::take(&mut self.seen_y), std::mem::take(&mut self.seen_d));
        self.keep_unmoved(integ, t, &y0, &d0, &mut cs);
        (self.seen_y, self.seen_d) = (y0, d0);
        for (c, s) in cs {
            self.roots[c] = s * f64::MIN_POSITIVE;
        }
    }

    /// Hands the sides and holds to the integrator.
    fn push_sides(&self, integ: &mut dyn Integrator) {
        integ.set_root_sides(&self.sides, &self.hold);
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
        let info = self.info;
        if !info.modes.iter().any(|m| self.timed(m.crossing)) {
            return;
        }
        self.sample(t, y, d);
        let env = crate::info::ChannelEnv {
            t,
            vars: &self.vars,
            params: &info.params,
            model: self.model,
        };
        for m in &info.modes {
            // the side its relation takes just after this instant, when its
            // crossing is now
            let after = if !self.timed(m.crossing) {
                None
            } else if let Some(Some(tc)) = info.time_crossings.get(m.crossing) {
                same_instant(lsim_ir::eval::eval(&tc.at, &env), t).then_some(tc.rising)
            } else {
                self.function_now(m.crossing, t)
            };
            self.now[m.crossing] = after.is_some();
            if let Some(up) = after {
                d[m.discrete] = if up { 1.0 } else { 0.0 };
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
        self.use_timed
            && (self.info.time_crossings.get(c).is_some_and(|x| x.is_some()) || self.tf.searched(c))
    }

    /// The root functions [`Loop::t_star`] may be set for, in order: the
    /// time crossings and the time functions searched ahead
    /// ([`timefn::TimeFns::live`]).
    fn time_live(&self) -> Vec<usize> {
        let searched = self.tf.live();
        (0..self.t_star.len())
            .filter(|&k| {
                self.info.time_crossings.get(k).is_some_and(|x| x.is_some())
                    || searched.contains(&k)
            })
            .collect()
    }

    /// The next crossing time of every time crossing, from the parameters
    /// and the discrete values now (the channels just sampled at `t`).
    fn schedule(&mut self, t: f64) {
        if !self.use_timed {
            return;
        }
        for (k, tc) in self.info.time_crossings.iter().enumerate() {
            // (the time functions' entries are [`Loop::schedule_functions`]')
            let Some(tc) = tc else { continue };
            let env = crate::info::ChannelEnv {
                t,
                vars: &self.vars,
                params: &self.info.params,
                model: self.model,
            };
            let at = lsim_ir::eval::eval(&tc.at, &env);
            // (a time within a few ulps of now is this instant: handled
            // here, not scheduled again)
            self.t_star[k] = if at > t && !same_instant(at, t) { at } else { f64::NAN };
        }
    }

    /// The times of the time tables' breakpoints ([`RunInfo::time_tables`]),
    /// with the channels and parameters as they stand: those whose
    /// position reads no discrete value (`moving` false: once, at the
    /// start), or those whose position does (`moving`: again after every
    /// discrete change, so no stale stop is kept where a breakpoint was).
    ///
    /// A kink ([`crate::info::TimeTable::kink`]) is stopped at where the
    /// cell before the breakpoint still holds: some ulps before the
    /// breakpoint's time, more than the rounding of the position (its
    /// terms, and the cell search over the table's extent) can move it. A
    /// position that grows falls short of the breakpoint there (whose own
    /// cell is the one after it); one that falls is still above it (in the
    /// cell it comes from). Another breakpoint stop between the two joins
    /// it.
    fn schedule_breaks(&mut self, t: f64, moving: bool) {
        let mut out = vec![];
        let mut kinks: Vec<(f64, f64)> = vec![];
        for tt in &self.info.time_tables {
            let reads =
                tt.b.any(&mut |x| matches!(x, lsim_ir::Expr::Var(_) | lsim_ir::Expr::Pre(_)));
            if moving != reads {
                continue;
            }
            let env = crate::info::ChannelEnv {
                t,
                vars: &self.vars,
                params: &self.info.params,
                model: self.model,
            };
            let b = lsim_ir::eval::eval(&tt.b, &env);
            let times = tt.at.iter().map(|x| (x - b) / tt.c).filter(|x| x.is_finite());
            if tt.kink {
                let extent = tt.at.iter().fold(0.0f64, |a, p| a.max(p.abs()));
                for (x, tb) in tt.at.iter().zip(times) {
                    let early = 64.0
                        * f64::EPSILON
                        * (tb.abs() + (x.abs() + b.abs() + extent) / tt.c.abs());
                    kinks.push((tb - early, tb));
                }
            } else {
                out.extend(times);
            }
        }
        kinks.sort_by(|a, b| a.0.total_cmp(&b.0));
        out.retain(|&r| {
            // the last kink stopped at or before r: does r fall in it?
            let i = kinks.partition_point(|k| k.0 <= r);
            i == 0 || {
                let (_, tb) = kinks[i - 1];
                !(r <= tb || same_instant(r, tb))
            }
        });
        let mut stops: Vec<f64> = kinks.iter().map(|k| k.0).collect();
        stops.sort_by(f64::total_cmp);
        stops.dedup();
        out.extend(&stops);
        out.sort_by(f64::total_cmp);
        out.dedup();
        if moving {
            self.breaks_moving = out;
            self.kinks_moving = stops;
        } else {
            self.breaks_fixed = out;
            self.kinks_fixed = stops;
        }
    }

    /// Whether `t` is a stop at a kink of a table read along time.
    fn at_kink(&self, t: f64) -> bool {
        [&self.kinks_fixed, &self.kinks_moving].iter().any(|k| {
            let i = k.partition_point(|x| *x < t);
            [i.wrapping_sub(1), i].iter().any(|&j| k.get(j).is_some_and(|x| same_instant(*x, t)))
        })
    }

    /// The next time table breakpoint after `t` (not within a few ulps of
    /// it), or infinity.
    fn next_break(&self, t: f64) -> f64 {
        [&self.breaks_fixed, &self.breaks_moving]
            .iter()
            .filter_map(|b| {
                let k = b.partition_point(|x| *x <= t || same_instant(*x, t));
                b.get(k).copied()
            })
            .fold(f64::INFINITY, f64::min)
    }

    /// The model's asserts on the channels just sampled.
    fn check_asserts(&mut self, t: f64) -> Result<(), SolveError> {
        for (k, a) in self.info.asserts.iter().enumerate() {
            let env = crate::info::ChannelEnv {
                t,
                vars: &self.vars,
                params: &self.info.params,
                model: self.model,
            };
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
        self.after_t = None;
    }

    /// The channels `vars`, where [`after_event`] sampled them at `(t, y,
    /// d)` and nothing changed them since (the point after an event, where
    /// the watch looks again: [`crate::states::Watch::begin`]).
    fn sampled_after(&self, t: f64, y: &[f64], d: &[f64]) -> Option<&[f64]> {
        let same = |a: &[f64], b: &[f64]| {
            a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.to_bits() == y.to_bits())
        };
        (self.after_t.is_some_and(|ta| ta.to_bits() == t.to_bits())
            && same(&self.after_y, y)
            && same(&self.after_d, d))
        .then_some(self.vars.as_slice())
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
            EventKind::States(k) => (usize::MAX, format!("the states chosen again (set {k})")),
            EventKind::Cell(k) => (
                usize::MAX,
                format!(
                    "the table '{}' read in the cell its argument went on in",
                    self.info.held.get(k).map(|h| h.label.as_str()).unwrap_or("a table")
                ),
            ),
        };
        self.events.push(EventRecord { t, when, label: label.clone(), kind });
        // storm detection: state events only (clocks and time events are
        // scheduled, not chattering, and so is what they cause at their
        // instant: a controller switching a mode from one tick to the next)
        // (nor a held table read going on in its next cell: its argument
        // crossing the table's breakpoints, as often as the data has them)
        if self.scheduled
            || matches!(kind, EventKind::Block(_) | EventKind::Time(_) | EventKind::Cell(_))
        {
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
    /// value changed from `d_pre`. `at_event`: `y` is the point the
    /// integrator had at an event (where it located a crossing, a tick in
    /// a step, a step's end), and the conditions are held where it saw
    /// them ([`Loop::hold_conditions`]); not at the start.
    #[allow(clippy::too_many_arguments)]
    fn iterate(
        &mut self,
        integ: &mut dyn Integrator,
        t: f64,
        y: &mut [f64],
        d: &mut Vec<f64>,
        d_pre: &[f64],
        dirs: Option<&[i32]>,
        at_event: bool,
    ) -> Result<bool, SolveError> {
        let info = self.info;
        let n_whens = info.whens.len();
        let mut fired = vec![0.0; n_whens];
        let mut any_fired = false;
        // the holds that no longer stand here, as at a step's end; a
        // condition past its hold crossed (the integrator may not have
        // reported it: it took the condition as held)
        if at_event && self.release_holds(t, y, d_pre) {
            self.push_sides(integ);
        }
        let merged = (at_event && !self.passed.is_empty()).then(|| self.with_passed(dirs));
        let dirs = merged.as_deref().or(dirs);
        // the conditions just before the event, and where the integrator
        // saw them
        self.eval_roots(t, y, d_pre, true);
        let seen = at_event && self.may_hold() && !self.hold_roots.is_empty();
        if seen {
            self.seen_y.clear();
            self.seen_y.extend_from_slice(y);
            self.seen_g.clear();
            self.seen_g.extend_from_slice(&self.roots_prev);
            self.seen_dirs.clear();
            self.seen_dirs.extend_from_slice(dirs.unwrap_or(&[]));
            self.seen_d.clear();
            self.seen_d.extend_from_slice(d_pre);
        }
        self.hold_conditions(integ, t, y, d_pre, false, true);
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
        self.rounds(integ, t, y, d, &mut fired, any_fired, &reported, &mut z_for, seen)?;
        self.solve_z(integ, t, y, d, &mut z_for)?;
        Ok(d.as_slice() != d_pre)
    }

    /// Event iteration's rounds at `t`: applies the `when` clauses in
    /// `fired`, sets every mode from its relation, and re-checks every
    /// condition against its value in the round before (on entry:
    /// `self.roots_prev`), until nothing changes. A clause fires when its
    /// condition goes from not holding to holding (Modelica's edge);
    /// `skip`: clauses the first round does not check (decided already).
    /// The conditions as the integrator sees them, held where it saw them
    /// at the event (`seen`: iterated from where it located the event;
    /// [`Loop::hold_conditions`]).
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
        seen: bool,
    ) -> Result<(), SolveError> {
        let info = self.info;
        let mut iterations = 0;
        // the held table reads moved at this instant, per axis
        // (crate::held::place)
        let mut moved = vec![[false; 2]; info.held.len()];
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
            self.hold_conditions(integ, t, y, d, seen, false);
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
            // the table reads held in their cells whose arguments left them
            // (crate::held): in the cells they are in now
            if !info.held.is_empty() {
                for (k, axes) in crate::held::place(self.model, info, &self.roots, t, d, &moved)? {
                    for (m, a) in moved[k].iter_mut().zip(axes) {
                        *m |= a;
                    }
                    again = true;
                    flipped.push(format!("the table '{}' read in its cells", info.held[k].label));
                    self.record(t, EventKind::Cell(k))?;
                    self.crossed(t, k, d);
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

    /// Held read `k` moved to another cell at `t` (a crossing of its
    /// group's edges, counted once an instant): its group's crossings
    /// recorded, and the group released ([`crate::held::release`]) where
    /// they have come too often ([`crate::held::released_by_crossings`];
    /// with [`SolverOptions::held_release`], on the explicit form, and for
    /// a group that holds a read that jumps, 1-D or 2-D
    /// ([`crate::HeldInfo::jumps`]): one held only for comparison stays
    /// held, and the implicit form, which cannot step over a jump, holds
    /// every read through every crossing).
    fn crossed(&mut self, t: f64, k: usize, d: &mut [f64]) {
        let info = self.info;
        let g = info.held[k].group;
        if g >= self.crossings.len() || self.crossed_at[g] == t {
            return;
        }
        self.crossed_at[g] = t;
        let at = &mut self.crossings[g];
        at.push(self.steps);
        if at.len() > crate::held::CROSSINGS_WINDOW + 1 {
            at.remove(0);
        }
        if self.opts.held_release
            && self.model.layout().n_z == 0
            && info.held.iter().any(|h| h.group == g && h.jumps)
            && crate::held::released_by_crossings(at)
        {
            crate::held::release(info, g, d);
            self.notes.borrow_mut().push(format!(
                "at t = {t:.6} s the table '{}' read at a state had crossed its cells' edges {} \
                 times within {} steps: stepped over from there, no longer held in its cells",
                info.held[k].label,
                at.len(),
                at[at.len() - 1] - at[0]
            ));
        }
    }

    /// With light restarts on ([`SolverOptions::light_restarts`], opt-in),
    /// whether a sample tick's change at `t` (the end of the integrator's
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
        if !self.opts.light_restarts || !only_outputs || h <= 0.0 {
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
        let dx = rms((0..n_x).map(|i| lsim_ir::math::powi((f1[i] - f0[i]) * w(i), 2)).sum(), n_x);
        let dz = if self.opts.suppress_algebraic_error {
            0.0
        } else {
            rms((n_x..n).map(|i| lsim_ir::math::powi((y[i] - y_old[i]) * w(i), 2)).sum(), n - n_x)
        };
        if !(h * dx <= 0.1 && dz <= 0.1) {
            return Ok(false);
        }
        Ok(integ.resume(t))
    }

    /// Event iteration with the rigid engagements: iterates, then at each
    /// rigid engagement that changed moves the states (the impulse
    /// projection, [`Loop::engage`]) and iterates on from the moved states,
    /// as Modelica re-checks every condition after a `reinit` at the same
    /// instant: a condition the jump crosses fires there, a mode it crosses
    /// flips there. Until nothing changes: it ends only when every
    /// engagement the iteration made is projected. A cascade of more than
    /// `max_event_iterations` engagements at one instant (each one's new
    /// speeds firing the next) stops the run with an event storm that names
    /// the engagement and the conditions; a cascade that is meant needs a
    /// higher limit. Returns whether a discrete value changed from `d_pre`,
    /// and where the engagements' lost energy goes.
    #[allow(clippy::too_many_arguments)]
    fn settle(
        &mut self,
        integ: &mut dyn Integrator,
        t: f64,
        y: &mut [f64],
        d: &mut Vec<f64>,
        d_pre: &[f64],
        dirs: Option<&[i32]>,
        before_vars: &[f64],
    ) -> Result<(bool, Option<Engagement>), SolveError> {
        let changed = self.iterate(integ, t, y, d, d_pre, dirs, true)?;
        if !changed || self.info.impulse.is_none() {
            return Ok((changed, None));
        }
        let mut books: Option<Engagement> = None;
        let mut ref_vars = before_vars.to_vec();
        let mut ref_d = d_pre.to_vec();
        let mut engaged = 0usize;
        // the conditions the last engagement's jump fired
        let mut fired_by: Vec<String> = vec![];
        loop {
            // the conditions as they stand before the states move
            self.eval_roots(t, y, d, true);
            self.hold_conditions(integ, t, y, d, false, true);
            let Some(e) = self.engage(integ, t, y, d, &ref_vars, &ref_d)? else { break };
            engaged += 1;
            if engaged > self.opts.max_event_iterations {
                return Err(self.cascade(t, &e, engaged, &fired_by));
            }
            match &mut books {
                Some(b) => b.merge(e),
                None => books = Some(e),
            }
            // what the next engagement, if the iteration makes one, starts
            // from
            let l = *self.model.layout();
            if l.n_z > 0 && self.info.events_read_z {
                integ.consistent_z(t, y, d)?;
            }
            self.sample(t, y, d);
            ref_vars.copy_from_slice(&self.vars);
            ref_d.copy_from_slice(d);
            // iterate on from the moved states
            let mut z_for = d.clone();
            let mut fired = vec![0.0; self.info.whens.len()];
            let seen = self.events.len();
            self.rounds(integ, t, y, d, &mut fired, false, &[], &mut z_for, false)?;
            self.solve_z(integ, t, y, d, &mut z_for)?;
            fired_by.clear();
            for ev in &self.events[seen..] {
                if !fired_by.contains(&ev.label) {
                    fired_by.push(ev.label.clone());
                }
            }
        }
        Ok((true, books))
    }

    /// The event storm of a cascade of rigid engagements at `t` longer than
    /// the event iteration allows: `e` the last engagement, `n` how many
    /// there were, `fired_by` the conditions the one before it fired.
    fn cascade(&self, t: f64, e: &Engagement, n: usize, fired_by: &[String]) -> SolveError {
        let energy = self.info.energy.as_ref();
        let mut names: Vec<String> = vec![];
        for (part, _, link) in &e.losses {
            if *link {
                continue;
            }
            let name = match (part, energy) {
                (Some(p), Some(en)) => en.parts[*p].name.clone(),
                _ => "a rigid engagement".to_string(),
            };
            if !names.contains(&name) {
                names.push(name);
            }
        }
        let max = self.opts.max_event_iterations;
        let why = if fired_by.is_empty() {
            String::new()
        } else {
            format!(", each one's new speeds firing {} again", fired_by.join(", "))
        };
        let mut parts = names.clone();
        parts.extend(fired_by.iter().cloned());
        SolveError::EventStorm {
            t,
            message: format!(
                "{} engaged {n} times at this instant{why}: more than the event iteration \
                 allows (SolverOptions::max_event_iterations = {max}). Check those conditions \
                 against the speeds each engagement leaves; if a cascade this long is meant, \
                 raise the limit.",
                names.join(", ")
            ),
            parts,
        }
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
                    Some(_) if c.tapes[k].is_some() => {
                        let (tape, regs) = c.tapes[k].as_mut().expect("a tape");
                        tape.eval(t, &c.y_vals, &c.chain_pos[k], d, self.u, regs)
                    }
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

    /// The inputs of clock `c`'s next ticks inside the step (at most
    /// `max`, those before `t_new`), as [`Self::read_inputs_selected`]
    /// gives them one by one, the same values: the dense output read at
    /// all of them at once, each tape run across them.
    fn read_ahead(
        &self,
        c: &mut Clock,
        integ: &mut dyn Integrator,
        t_new: f64,
        d: &[f64],
        max: usize,
    ) -> Result<(), SolveError> {
        c.batch_ts.clear();
        let mut k = c.k;
        while c.batch_ts.len() < max {
            let x = c.offset + k as f64 * c.period;
            // (x is finite: periods are checked)
            if x >= t_new {
                break;
            }
            c.batch_ts.push(x);
            k += 1;
        }
        c.batch_k0 = c.k;
        let n = c.batch_ts.len();
        let m = c.y_idx.len();
        if m > 0 {
            c.batch_y.resize(n * m, 0.0);
            integ.interpolate_select_many(&c.batch_ts, &c.y_idx, &mut c.batch_y)?;
        }
        let n_in = c.srcs.len();
        c.batch_in.resize(n * n_in, 0.0);
        for (k, src) in c.srcs.iter().enumerate() {
            // (one value for every tick)
            let same = match *src {
                VarSource::D(i) => Some(d[i]),
                VarSource::NegD(i) => Some(-d[i]),
                VarSource::U(i) => Some(self.u[i]),
                VarSource::Const(v) => Some(v),
                _ => None,
            };
            if let Some(v) = same {
                c.batch_in[k..].iter_mut().step_by(n_in).take(n).for_each(|x| *x = v);
                continue;
            }
            match *src {
                VarSource::Y(_) | VarSource::NegY(_) => {
                    let p = c.y_pos[k].expect("a state input");
                    let neg = matches!(src, VarSource::NegY(_));
                    let col = c.batch_in[k..].iter_mut().step_by(n_in).take(n);
                    for (x, y) in col.zip(&c.batch_y[p * n..][..n]) {
                        *x = if neg { -*y } else { *y };
                    }
                }
                VarSource::D(_) | VarSource::NegD(_) | VarSource::U(_) | VarSource::Const(_) => {}
                VarSource::Computed => {
                    let (tape, _) = c.tapes[k].as_ref().expect("a tape");
                    // (a block's only input: straight into its rows)
                    let out = if n_in == 1 { &mut c.batch_in[..n] } else { &mut c.lane_out[..n] };
                    tape.eval_lanes(
                        &c.batch_ts,
                        &c.batch_y,
                        n,
                        &c.chain_pos[k],
                        d,
                        self.u,
                        TICK_BATCH,
                        &mut c.lane_regs[k],
                        out,
                    );
                    if n_in > 1 {
                        let col = c.batch_in[k..].iter_mut().step_by(n_in).take(n);
                        for (x, v) in col.zip(&c.lane_out) {
                            *x = *v;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Whether the discrete values `now` differ from `then` in one that
    /// moves the iteration variables ([`RunInfo::z_discretes`]).
    fn moves_z(&self, now: &[f64], then: &[f64]) -> bool {
        self.model.layout().n_z > 0
            && now.iter().zip(then).enumerate().any(|(i, (a, b))| {
                a.to_bits() != b.to_bits() && self.info.z_discretes.get(i).copied().unwrap_or(true)
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

/// Every table the run evaluates outside the compiled code (the energy
/// books, the conditions on functions of time, the asserts, the time
/// events' instants and the positions of the tables read along time, the
/// impulse projection; a sampled block's computed input reads no table:
/// it is read from the channels then) is one the model gives
/// ([`ModelFunctions::eval_table`], and for a read of a table's partial
/// derivative, which index reduction writes, [`ModelFunctions::
/// eval_table_order`]): without it they would read NaN (books NaN, a run
/// that fails or a search that stalls), so the run does not start.
fn tables_given(
    model: &dyn ModelFunctions,
    info: &RunInfo,
    opts: &SolverOptions,
    t0: f64,
) -> Result<(), SolveError> {
    use crate::TimeFunction;
    let mut read: Vec<(u32, [u8; 2], &str)> = vec![];
    let mut visit = |e: &lsim_ir::Expr, what: &'static str| {
        e.walk(&mut |x| {
            if let lsim_ir::Expr::Table { table, order, .. } = x
                && !read.iter().any(|(k, o, _)| k == table && o == order)
            {
                read.push((*table, *order, what));
            }
        })
    };
    if opts.energy_books
        && let Some(e) = &info.energy
    {
        for p in &e.parts {
            for x in [Some(&p.power), p.loss.as_ref(), p.stored.as_ref()].into_iter().flatten() {
                visit(x, "the energy books");
            }
        }
        if let Some(r) = &info.stored_rates {
            for (_, x) in &r.chain {
                visit(x, "the energy books' rates");
            }
        }
    }
    for f in info.time_functions.iter().flatten() {
        match f {
            TimeFunction::Pure(g) => visit(g, "a condition on a function of time"),
            TimeFunction::Mixed { chain, g } => {
                for (_, e) in chain {
                    visit(e, "a condition on a function of time");
                }
                visit(g, "a condition on a function of time");
            }
            TimeFunction::Unhandled => {}
        }
    }
    for a in &info.asserts {
        visit(&a.condition, "an assert");
    }
    for c in info.time_crossings.iter().flatten() {
        visit(&c.at, "a time event's instant");
    }
    for tt in &info.time_tables {
        visit(&tt.b, "a table read along time");
    }
    if opts.impulses
        && let Some(imp) = &info.impulse
    {
        // what it evaluates: the engagements, how the computed variables
        // follow from the state, the links' relative velocities and their
        // conditions, and the stored energies it balances (with the books
        // off too)
        let what = "the impulse projection";
        for e in &imp.engagements {
            visit(&e.changes, what);
        }
        for (_, _, e) in &imp.chain {
            visit(e, what);
        }
        for l in &imp.links {
            visit(&l.keep, what);
            visit(&l.active, what);
        }
        if let Some(e) = &info.energy {
            for (k, _) in &imp.parts {
                if let Some(x) = e.parts.get(*k).and_then(|p| p.stored.as_ref()) {
                    visit(x, what);
                }
            }
        }
    }
    for (k, order, what) in read {
        if model.eval_table_order(k, order, [0.0, 0.0]).is_none() {
            let name = info.table_names.get(k as usize).cloned().unwrap_or_else(|| format!("{k}"));
            let (which, method) = if order == [0, 0] {
                (format!("the table '{name}'"), "eval_table")
            } else {
                (
                    format!("the partial derivative {order:?} of the table '{name}'"),
                    "eval_table_order",
                )
            };
            return Err(SolveError::Initialisation {
                t: t0,
                message: format!(
                    "{what} read {which} outside the compiled code, and the model does not give \
                     it (ModelFunctions::{method}): a wrapper around a compiled model must pass it \
                     on"
                ),
            });
        }
    }
    Ok(())
}

/// The tables the run stops at or encloses along time by their
/// breakpoints (the tables read along time, those the conditions on
/// functions of time read) whose values the model gives but not their
/// breakpoints ([`ModelFunctions::table_axes`]): one warning each. The run
/// then uses the breakpoints it was prepared with, wrong if the table's
/// data changed since (a wrapper around a compiled model that does not
/// pass `table_axes` on).
fn axes_missing(model: &dyn ModelFunctions, info: &RunInfo) -> Vec<String> {
    use crate::TimeFunction;
    let mut used: std::collections::BTreeSet<u32> =
        info.time_tables.iter().map(|tt| tt.table).collect();
    let mut add = |e: &lsim_ir::Expr| {
        e.walk(&mut |x| {
            if let lsim_ir::Expr::Table { table, .. } = x {
                used.insert(*table);
            }
        })
    };
    for f in info.time_functions.iter().flatten() {
        match f {
            TimeFunction::Pure(g) => add(g),
            TimeFunction::Mixed { chain, g } => {
                for (_, e) in chain {
                    add(e);
                }
                add(g);
            }
            TimeFunction::Unhandled => {}
        }
    }
    used.into_iter()
        .filter(|&k| model.eval_table(k, [0.0, 0.0]).is_some() && model.table_axes(k).is_none())
        .map(|k| {
            let name = info.table_names.get(k as usize).cloned().unwrap_or_else(|| format!("{k}"));
            format!(
                "the table '{name}': the model gives its values but not its breakpoints \
                 (ModelFunctions::table_axes), so the run stops and searches along time at the \
                 breakpoints it was prepared with, wrong if the table's data changed since (a \
                 wrapper around a compiled model must pass table_axes on)"
            )
        })
        .collect()
}

/// The run loop, for any [`Integrator`]: see the module documentation.
/// Index reduction's choice of states is watched at every step
/// ([`crate::states`]); an integrator that fails where it has degraded is
/// told as its failure, and any other failure of the integrator with what
/// the run can see where its last step started ([`integrator_failure`]).
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
    // the tables' breakpoints as the model interpolates them (said where
    // the model gives a table's values but not its breakpoints)
    let axes_warnings = axes_missing(model, info);
    let given = info.with_model_tables(model);
    let info = given.as_ref().unwrap_or(info);
    // what the run says of itself (the held reads it released), told with
    // its failure too
    let notes = std::cell::RefCell::new(vec![]);
    // the models the run switches to, and the one it is on
    let arena = switch::Arena::new();
    let mut cur = Current { model, info, integ: None };
    let watch = crate::states::Watch::new(info, model, opts.books_kernels);
    let mut watch = if opts.backend == crate::Backend::Sundials {
        watch.switching(info, opts.state_switch)
    } else {
        watch.unswitched(
            "the run is on diffsol, the cross-check, which does not choose the states again",
        )
    };
    let run = run_watched(
        &arena,
        &mut cur,
        opts,
        grid,
        integ,
        u,
        blocks,
        started,
        &mut watch,
        axes_warnings,
        &notes,
    );
    run.map_err(|e| {
        let (model, info) = (cur.model, cur.info);
        let integ: &mut dyn Integrator = match cur.integ.as_deref_mut() {
            Some(i) => i,
            None => integ,
        };
        let e = match watch.blamed(info, &e) {
            Some(named) => named,
            None => {
                // (where the last step started: the integrator's solution
                // is still the last point it reached)
                let (t, d) = watch.from();
                let y = integ.y().to_vec();
                integrator_failure(model, info, integ, t, &y, d, u, e)
            }
        };
        e.with_notes(&notes.borrow())
    })
}

/// The model a run is on and its integrator: the first, given to the run
/// loop, until the run chooses the states again ([`switch`]).
struct Current<'a> {
    model: &'a dyn ModelFunctions,
    info: &'a RunInfo,
    /// the integrator of the model switched to (`None`: the run's own)
    integ: Option<Box<dyn Integrator + 'a>>,
}

/// The channels at a step's end the switch's look took ([`switch`]), for
/// the step's end itself.
#[derive(Default)]
struct Probe {
    /// the step's end has these channels: no tick changed anything since
    valid: bool,
    t: f64,
    y: Vec<f64>,
    vars: Vec<f64>,
    work: Vec<f64>,
}

/// A switch to the states chosen again, ready to be made where the run is
/// ([`ready_switch`]): found at a step's end, or inside the step where the
/// step went past the point (the step then cut back there only once the
/// switch is ready: a refused one leaves the step as it was), or at the
/// point after an event. Made at the loop's top where the run is at its
/// point; not made where a tick inside the step restarted the run before
/// it (the watch looks again from the tick), or an event comes at it (the
/// watch looks again from the point after the event).
struct Ready<'a> {
    /// the set chosen again
    k: usize,
    /// the model to go on with
    next: switch::Next<'a>,
    /// the integrator on its states from the point
    integ: Box<dyn Integrator + 'a>,
    /// the watch on it
    watch: crate::states::Watch,
    /// its states and iteration variables there
    y: Vec<f64>,
    /// the point's time, and the discrete values it was readied with
    t: f64,
    d: Vec<f64>,
    /// the discrete values the run goes on with (the reads released on
    /// the explicit form held again on the implicit one), and those reads'
    /// tables
    d_next: Vec<f64>,
    held_again: Vec<String>,
    /// the step was cut back to the point
    cut: bool,
    /// the set's factor there
    e: f64,
}

/// Starts the integrator on the model `next` (set `k` of the model `from`
/// chosen again) at the point `(t, d)` the run is at on `old` (watched by
/// `watch`, the energy books `books`), and checks that the run can go on
/// with it there ([`crate::states::Watch::holds`]). Where it cannot: the
/// set of `from` to blame, that set's factor where it is not `k`, and why.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn ready_switch<'a>(
    opts: &SolverOptions,
    grid: OutputGrid,
    old: &mut dyn Integrator,
    watch: &crate::states::Watch,
    from: &RunInfo,
    next: switch::Next<'a>,
    k: usize,
    t: f64,
    d: &[f64],
    u: &[f64],
    books: Option<&Ledger<'_>>,
    (cut, e): (bool, f64),
) -> Result<Result<Ready<'a>, (usize, Option<f64>, String)>, SolveError> {
    let refuse = |why: &str| Ok(Err((k, None, why.to_string())));
    // a table read the run released (on the explicit form) would be
    // stepped over on the implicit form, which cannot step over its jump:
    // it is held in its cells again there (that form releases nothing)
    let mut d_next = d.to_vec();
    let mut held_again: Vec<String> = vec![];
    if next.model.layout().n_z > 0 {
        for h in &next.info.held {
            if d_next.get(h.released).is_some_and(|r| *r > 0.5) {
                d_next[h.released] = 0.0;
                for ax in &h.axes {
                    d_next[ax.cell] = -1.0;
                }
                if !held_again.contains(&h.label) {
                    held_again.push(h.label.clone());
                }
            }
        }
        if !held_again.is_empty() {
            crate::held::settle(next.model, next.info, t, &next.y, &mut d_next, u)?;
        }
    }
    let started = crate::new_integrator(
        next.model,
        next.info,
        opts,
        OutputGrid { t0: t, ..grid },
        &next.y,
        d_next.clone(),
        u.to_vec(),
    );
    let mut integ = match started {
        None => return refuse("the integrator cannot be started again on this backend"),
        Some(Err(e)) => return refuse(&format!("the integrator did not start on them ({e})")),
        Some(Ok(i)) => i,
    };
    let mut q = vec![0.0; books.map_or(0, |lg| lg.q.len())];
    if !q.is_empty() {
        old.quadrature(t, &mut q)?;
    }
    if !integ.set_quadratures(&q)? {
        return refuse("the energy integrals cannot be carried over");
    }
    let rebinds = match (books, next.info.energy.as_ref()) {
        (Some(lg), Some(e)) => lg.can_rebind(e),
        (Some(_), None) => false,
        (None, _) => true,
    };
    if !rebinds {
        return refuse(
            "the energy books cannot be read through the model with the states chosen again",
        );
    }
    let mut w = crate::states::Watch::new(next.info, next.model, opts.books_kernels)
        .switching(next.info, opts.state_switch);
    w.adopt(watch, from, next.info, k);
    let y = integ.y().to_vec();
    if let Err(e) = w.holds(next.info, next.model, t, &y, &d_next, u) {
        return Ok(Err(e));
    }
    Ok(Ok(Ready { k, next, integ, watch: w, y, t, d: d.to_vec(), d_next, held_again, cut, e }))
}

#[allow(clippy::too_many_arguments)]
fn run_watched<'a>(
    arena: &'a switch::Arena,
    cur: &mut Current<'a>,
    opts: &'a SolverOptions,
    grid: OutputGrid,
    integ0: &mut dyn Integrator,
    u: &'a [f64],
    blocks: &mut [Box<dyn DiscreteBlock>],
    started: Instant,
    watch: &mut crate::states::Watch,
    axes_warnings: Vec<String>,
    notes: &'a std::cell::RefCell<Vec<String>>,
) -> Result<SimResult, SolveError> {
    // the default floating-point environment (lsim_ir::fenv)
    let _env = lsim_ir::fenv::DefaultFloatEnv::enter();
    let (model0, info0) = (cur.model, cur.info);
    let (mut model, mut info) = (model0, info0);
    let mut l = *model.layout();
    let mut n = l.n_y();
    let times = grid.times()?;
    let t_end = grid.t_end;
    tables_given(model, info, opts, grid.t0)?;
    let span = (t_end - grid.t0).abs();
    // the channels recorded (SolverOptions::record): the first model's,
    // through `Loop::chan`
    if let Some(bad) = opts.record.iter().flatten().find(|&&c| c >= l.n_vars) {
        return Err(SolveError::Initialisation {
            t: grid.t0,
            message: format!(
                "SolverOptions::record names channel {bad}; the model has {} channels",
                l.n_vars
            ),
        });
    }
    let mut rec = Recorder::new(opts.record.as_ref().map_or(l.n_vars, |r| r.len()), &times)?;
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
        hold: vec![0.0; l.n_roots + model.table_guard_list().len()],
        held: false,
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
        held_root: {
            let mut v = vec![false; l.n_roots + model.table_guard_list().len()];
            for ax in info.held.iter().flat_map(|h| &h.axes) {
                for c in [ax.below, ax.above] {
                    if c < v.len() {
                        v[c] = true;
                    }
                }
            }
            v
        },
        hold_roots: vec![],
        seen_y: vec![],
        seen_g: vec![],
        seen_dirs: vec![],
        seen_d: vec![],
        passed: vec![],
        outside_since: vec![None; model.table_guard_list().len()],
        outside_total: vec![(0.0, f64::NAN); model.table_guard_list().len()],
        warned: vec![false; info.asserts.len()],
        warned_links: false,
        warnings: axes_warnings,
        scheduled: false,
        t_star: vec![f64::NAN; info.time_crossings.len().max(info.time_functions.len())],
        t_live: vec![],
        now: vec![false; l.n_roots],
        use_timed: false,
        tf: timefn::TimeFns::new(info.time_crossings.len().max(info.time_functions.len())),
        breaks_fixed: vec![],
        breaks_moving: vec![],
        kinks_fixed: vec![],
        kinks_moving: vec![],
        chan: opts.record.clone(),
        chan_buf: vec![],
        after_t: None,
        after_y: vec![],
        after_d: vec![],
        steps: 0,
        crossings: vec![vec![]; crate::held::groups(info)],
        crossed_at: vec![f64::NAN; crate::held::groups(info)],
        notes,
    };
    for (k, (c, _)) in info.whens.iter().enumerate() {
        if *c < lp.when_of_root.len() && lp.mode_of_root[*c].is_none() {
            lp.when_of_root[*c].get_or_insert(k);
        }
    }
    lp.hold_roots =
        (0..l.n_roots).filter(|&c| lp.mode_of_root[c].is_none() && !lp.held_root[c]).collect();
    let p = &info.params;
    // (the run's own integrator until it chooses its states again)
    let integ: &mut dyn Integrator = &mut *integ0;
    let mut y = vec![0.0; n];
    let mut d = integ.discrete_mut().to_vec();
    let mut ledger = if opts.energy_books {
        info.energy.as_ref().filter(|e| !e.parts.is_empty()).map(|e| Ledger::new(e, model))
    } else {
        None
    };
    if let Some(lg) = ledger.as_mut() {
        lg.reserve(times.len())?;
    }
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
        .map(|(b, bi)| clock_for(b.as_ref(), bi, info, opts, &l, grid.t0))
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
    // (and the time functions it searches)
    {
        let mut mask = lp.time_functions_start(t_end);
        lp.t_live = lp.time_live();
        mask.resize(lp.sides.len(), false);
        for (k, tc) in info.time_crossings.iter().enumerate() {
            mask[k] |= tc.is_some();
        }
        if mask.iter().any(|m| *m) {
            lp.use_timed = integ.set_root_mask(&mask);
        }
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
            lp.iterate(integ, t, &mut y, &mut d, &d_init, None, false)?;
            integ.discrete_mut().copy_from_slice(&d);
            integ.restart(t, &y)?;
            y.copy_from_slice(integ.y());
        }
    }
    lp.sample(t, &y, &d);
    lp.schedule(t);
    let mut scheduled_for = d.clone();
    rec.start(t, lp.recorded());
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
    lp.update_sides(integ, t, &y, &d, None, None);
    lp.push_sides(integ);

    let mut next_time_event = 0;
    while next_time_event < info.time_events.len() && info.time_events[next_time_event] <= t {
        next_time_event += 1;
    }
    let mut local = vec![0.0; n];
    let mut stuck = 0;
    let mut acc = vec![0.0; n];
    let mut ymax: Vec<f64> = y.iter().map(|v| v.abs()).collect();
    let mut yk = vec![0.0; n];

    if !info.time_tables.is_empty() {
        lp.schedule_breaks(t, false);
        lp.schedule_breaks(t, true);
    }
    // the ticks inside a step: the clocks due at an instant, those that
    // changed an output, the discrete values after them
    let mut due_clocks: Vec<usize> = Vec::with_capacity(clocks.len());
    let mut changed_blocks: Vec<usize> = Vec::with_capacity(clocks.len());
    let mut d_tick = d.clone();
    // the dense output's generation: a new one at every step and change
    let mut dense_gen: u64 = 0;
    // dynamic state selection: the switch asked for, the work and the
    // error estimates of the integrators the run went on from, and the
    // switches made
    let mut ready: Option<Ready<'a>> = None;
    let mut stats_left = crate::SolverStats::default();
    let mut errors_left: Vec<(String, f64, f64, f64)> = vec![];
    let mut switch_notes: Vec<String> = vec![];
    let mut switches = 0usize;
    let mut probe = Probe::default();
    while t < t_end {
        // the states chosen again: the run goes on with them from here (a
        // switch readied in a step that a tick then cut short, or whose
        // point a tick's event changed, is not made: the watch looks again
        // from the point after the tick)
        if let Some(r) = ready.take().filter(|r| r.t == t && r.d == d) {
            let old: &mut dyn Integrator = match cur.integ.as_deref_mut() {
                Some(i) => i,
                None => &mut *integ0,
            };
            let Ready {
                k: sw_k, next, integ: mut ni, watch: w2, y: yc, d_next, held_again, ..
            } = r;
            d = d_next;
            for label in held_again {
                lp.notes.borrow_mut().push(format!(
                    "at t = {t:.6} s the table '{label}' read at a state, stepped over before, is \
                     held in its cells again: the model the states chosen again need is of the \
                     implicit form, which cannot step over it"
                ));
            }
            // what the integrator left: its work, its error estimates (by
            // name, over the models the run was on)
            stats_left += old.stats();
            for i in 0..n {
                let name = info.y_names.get(i).cloned().unwrap_or_default();
                let (a, m, f) = (acc[i], ymax[i], opts.atol * info.y_nominal[i]);
                match errors_left.iter_mut().find(|x| x.0 == name) {
                    Some(x) => {
                        x.1 += a;
                        x.2 = x.2.max(m);
                        x.3 = x.3.max(f);
                    }
                    None => errors_left.push((name, a, m, f)),
                }
            }
            // the point before the switch, in the books and the recorded
            // channels
            lp.sample(t, &y, &d);
            let before_vars = lp.vars.clone();
            let before = ledger.as_mut().map(|lg| lg.before_event(t, &before_vars, p));
            let left = lp.recorded_of(&before_vars).into_owned();
            let from_states = switch::states_of(info, l.n_x);
            let row =
                info.state_sets.get(sw_k).and_then(|s| s.rows.first()).cloned().unwrap_or_default();
            // the model the run is on
            model = next.model;
            info = next.info;
            cur.model = model;
            cur.info = info;
            l = *model.layout();
            n = l.n_y();
            lp.model = model;
            lp.info = info;
            lp.work.resize(l.n_work, 0.0);
            lp.vars.resize(l.n_vars, 0.0);
            lp.after_t = None;
            let identity =
                next.map.iter().enumerate().all(|(c, &j)| c == j) && l.n_vars == next.map.len();
            lp.chan = match &opts.record {
                // (the channels recorded, among the first model's)
                Some(record) => Some(record.iter().map(|&c| next.map[c]).collect()),
                None => (!identity).then_some(next.map),
            };
            y = yc;
            local = vec![0.0; n];
            acc = vec![0.0; n];
            ymax = y.iter().map(|v| v.abs()).collect();
            yk = vec![0.0; n];
            if let Some(lg) = ledger.as_mut()
                && let Some(e) = info.energy.as_ref()
            {
                // (the same parts: ready_switch checked it)
                let rebound = lg.rebind(e, model);
                debug_assert!(rebound, "the books' parts were checked");
            }
            // the conditions on functions of time, the tables read
            // along time, the time crossings: set up again
            lp.tf = timefn::TimeFns::new(info.time_crossings.len().max(info.time_functions.len()));
            lp.sample(t, &y, &d);
            let mut mask = lp.time_functions_start(t_end);
            lp.t_live = lp.time_live();
            mask.resize(lp.sides.len(), false);
            for (k, tc) in info.time_crossings.iter().enumerate() {
                mask[k] |= tc.is_some();
            }
            lp.use_timed = mask.iter().any(|m| *m) && ni.set_root_mask(&mask);
            lp.t_star.iter_mut().for_each(|x| *x = f64::NAN);
            lp.schedule(t);
            if !info.time_tables.is_empty() {
                lp.schedule_breaks(t, false);
                lp.schedule_breaks(t, true);
            }
            scheduled_for.copy_from_slice(&d);
            // the sampled blocks read their inputs from it
            for (b, c) in clocks.iter_mut().enumerate() {
                let mut fresh = clock_for(blocks[b].as_ref(), &info.blocks[b], info, opts, &l, t);
                fresh.k = c.k;
                fresh.stop_next = c.stop_next;
                fresh.inputs = std::mem::take(&mut c.inputs);
                fresh.outputs = std::mem::take(&mut c.outputs);
                fresh.idle_run = c.idle_run;
                *c = fresh;
            }
            lp.update_sides(&mut *ni, t, &y, &d, None, None);
            lp.push_sides(&mut *ni);
            // the switch, an event: the stored energy's change
            // (the iteration variables solved again) booked
            let label = format!(
                "the states chosen again for {row}: {} in place of {}",
                next.states.join(", "),
                from_states.join(", "),
            );
            lp.events.push(EventRecord {
                t,
                when: usize::MAX,
                label: label.clone(),
                kind: EventKind::States(sw_k),
            });
            switches += 1;
            if switch_notes.len() < 20 {
                switch_notes.push(format!("at t = {t:.9} s, {label}"));
            }
            *watch = w2;
            cur.integ = Some(ni);
            let integ: &mut dyn Integrator = match cur.integ.as_deref_mut() {
                Some(i) => i,
                None => &mut *integ0,
            };
            // (as after any event: the recorded channels, as the
            // run records them, before and after it)
            lp.sample(t, &y, &d);
            rec.interior(t, lp.recorded());
            if let (Some(lg), Some(b)) = (ledger.as_mut(), before) {
                lg.after_event(t, &lp.vars, p, &b, None);
            }
            if rec.next_grid_time() == Some(t) {
                rec.left_limit(&left, lp.recorded());
                rec.grid_point(t, lp.recorded());
                if let Some(lg) = ledger.as_mut() {
                    integ.quadrature(t, &mut lg.q)?;
                    lg.grid_point(t, &lp.vars, p);
                }
            }
        }
        let integ: &mut dyn Integrator = match cur.integ.as_deref_mut() {
            Some(i) => i,
            None => &mut *integ0,
        };
        probe.valid = false;
        dense_gen += 1;
        // the time crossings' times move only when a discrete value does
        // (a time function's next sign change after one fired too)
        if d != scheduled_for {
            lp.schedule(t);
            if !info.time_tables.is_empty() {
                lp.schedule_breaks(t, true);
            }
            scheduled_for.copy_from_slice(&d);
        }
        // (those that fired, ran out of steps here, or read a value that
        // changed)
        lp.schedule_functions(integ, t, &y)?;
        let mut t_stop = t_end.min(lp.next_break(t)).min(lp.tf.next_stop(t));
        if let Some(&te) = info.time_events.get(next_time_event) {
            t_stop = t_stop.min(te);
        }
        // (a scheduled crossing is set only at those `t_live` names: the
        // time crossings and the functions searched ahead)
        debug_assert!(
            lp.t_star.iter().enumerate().all(|(k, ts)| ts.is_nan() || lp.t_live.contains(&k)),
            "a crossing scheduled outside Loop::t_live"
        );
        for &k in &lp.t_live {
            let ts = lp.t_star[k];
            if ts > t {
                t_stop = t_stop.min(ts);
            }
        }
        for c in &clocks {
            if c.stop_next {
                t_stop = t_stop.min(c.next());
            }
        }
        // the point the step starts from (where a failure of the
        // integrator is told), and the choice of states there, where an
        // event came since it was last checked: the next stretch starts
        // here
        let sampled = lp.sampled_after(t, &y, &d);
        if let Some(k) = watch.begin(info, model, t, &y, &d, u, lp.events.len(), sampled)? {
            // the states to be chosen again here (the point after an
            // event, or the start)
            let (e, _) = watch.standing(info, model, k, t, &lp.vars_at(t, &y, &d));
            let window = watch.threshold(k);
            let made = match switch::choose_at(
                arena,
                (model, info),
                (model0, info0),
                &info0.var_names,
                t,
                &y,
                &d,
                u,
                k,
                window,
            ) {
                Ok(next) => ready_switch(
                    opts,
                    grid,
                    integ,
                    watch,
                    info,
                    next,
                    k,
                    t,
                    &d,
                    u,
                    ledger.as_ref(),
                    (false, e),
                )?,
                Err(why) => Err((k, None, why)),
            };
            match made {
                Ok(r) => {
                    ready = Some(r);
                    continue;
                }
                Err((blame, factor, why)) => {
                    if switch_notes.len() < 20 {
                        switch_notes.push(format!(
                            "at t = {t:.9} s, the states were not chosen again: {why}"
                        ));
                    }
                    watch.refused(blame, factor.unwrap_or(e), &why);
                }
            }
        }
        // a stop time a few ulps away (a tick that falls just before the
        // end, two clocks' ticks that round apart) is this instant: the
        // integrators cannot step so short an interval, and the solution
        // does not move across it
        let same = same_instant(t, t_stop);
        let mut st = if same { Step::Stopped(t_stop) } else { integ.step(t_stop)? };
        if !same {
            lp.steps += 1;
        }
        // steps that leave the time where it was (shorter than its
        // resolution here, which the integrators take for steps all the
        // same): no step longer passes the error test, at a jump the model
        // declares no event for, and the run would go on without end
        if matches!(st, Step::Internal(_)) && st.time() <= t {
            stuck += 1;
            if stuck >= STUCK_STEPS {
                return Err(no_progress(model, info, integ, t, &y, &d, u));
            }
        } else {
            stuck = 0;
        }
        // a mixed condition's sign change inside the step that root
        // finding did not see (two inside one step: a pulse): the step
        // ends there, as at a root; the state there from the dense output
        let mut y_cut: Option<Vec<f64>> = None;
        let reported: &[i32] = if let Step::Root(_, dirs) = &st { dirs } else { &[] };
        let timer = (opts.time_mixed_checks && !same).then(Instant::now);
        let pulse = if same { None } else { lp.scan_mixed(integ, t, st.time(), reported, &d)? };
        if let Some(t0) = timer {
            report.mixed_seconds += t0.elapsed().as_secs_f64();
        }
        if let Some((at, dirs)) = pulse {
            let mut yv = vec![0.0; n];
            integ.interpolate(at, &mut yv)?;
            y_cut = Some(yv);
            st = Step::Root(at, dirs);
            report.pulses_found += 1;
        }
        // dynamic state selection: where inside the step the states should
        // be chosen again (the step is cut back there)
        if !same && watch.may_switch() {
            let t1 = st.time();
            // the channels at the step's end (kept for the step's end below,
            // where nothing changed them since: the same values)
            probe.t = t1;
            probe.y.clear();
            probe.y.extend_from_slice(y_cut.as_deref().unwrap_or(integ.y()));
            probe.vars.resize(l.n_vars, 0.0);
            probe.work.resize(l.n_work, 0.0);
            let inp = EvalInput { t: t1, y: &probe.y, p: &info.params, d: &d, u };
            model.vars(&inp, &mut probe.work, &mut probe.vars);
            probe.valid = true;
            let (y1, v1) = (&probe.y, &probe.vars);
            let l_now = l;
            let mut y_in = vec![];
            let mut work_in = vec![];
            let found = {
                let mut inside = |tau: f64, v: &mut Vec<f64>| {
                    y_in.resize(l_now.n_y(), 0.0);
                    work_in.resize(l_now.n_work, 0.0);
                    v.resize(l_now.n_vars, 0.0);
                    if integ.interpolate(tau, &mut y_in).is_err() {
                        return false;
                    }
                    let inp = EvalInput { t: tau, y: &y_in, p: &info.params, d: &d, u };
                    model.vars(&inp, &mut work_in, v);
                    true
                };
                watch.switch_point(info, model, t1, v1, &d, lp.events.len(), &mut inside)
            };
            if let Some((ts, k)) = found {
                let cut = ts < t1 && !same_instant(ts, t1);
                let ys = if cut {
                    let mut yv = vec![0.0; n];
                    integ.interpolate(ts, &mut yv)?;
                    yv
                } else {
                    y1.clone()
                };
                let (e, _) = watch.standing(info, model, k, ts, &lp.vars_at(ts, &ys, &d));
                let window = watch.threshold(k);
                // (readied before the step is cut back: a switch refused
                // leaves the step as it was, its integrator not restarted
                // from a point of its dense output)
                let made = match switch::choose_at(
                    arena,
                    (model, info),
                    (model0, info0),
                    &info0.var_names,
                    ts,
                    &ys,
                    &d,
                    u,
                    k,
                    window,
                ) {
                    Ok(next) => ready_switch(
                        opts,
                        grid,
                        integ,
                        watch,
                        info,
                        next,
                        k,
                        ts,
                        &d,
                        u,
                        ledger.as_ref(),
                        (cut, e),
                    )?,
                    Err(why) => Err((k, None, why)),
                };
                match made {
                    Ok(r) => {
                        if cut {
                            y_cut = Some(ys);
                            st = Step::Stopped(ts);
                        }
                        ready = Some(r);
                    }
                    Err((blame, factor, why)) => {
                        if switch_notes.len() < 20 {
                            switch_notes.push(format!(
                                "at t = {ts:.9} s, the states were not chosen again: {why}"
                            ));
                        }
                        watch.refused(blame, factor.unwrap_or(e), &why);
                    }
                }
            }
        }
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
        let crossing_due = lp.t_live.iter().any(|&k| due(lp.t_star[k]));
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
                (_, Some((first, tk0))) => {
                    let mut tk = tk0;
                    changed_blocks.clear();
                    let mut have_y = false;
                    // a clock due alone that reads ahead ticks in a row
                    // while nothing else comes before its next tick (no
                    // grid point, no other clock, the step's end): each
                    // tick as below, without going round this loop; one
                    // that changes an output is handled as below
                    let others = clocks
                        .iter()
                        .enumerate()
                        .filter(|(b, _)| *b != first)
                        .map(|(_, c)| c.next())
                        .reduce(f64::min);
                    let lone = |x: f64| others.is_none_or(|o| x < o && !same_instant(x, o));
                    let c = &mut clocks[first];
                    let ran = c.batchable && tk < t_new && c.next() == tk && lone(tk);
                    if ran {
                        due_clocks.clear();
                        due_clocks.push(first);
                        let outs = &info.blocks[first].outputs;
                        let n_in = info.blocks[first].inputs.len();
                        let blk = &mut blocks[first];
                        loop {
                            report.block_ticks += 1;
                            if !(c.batch_gen == dense_gen
                                && c.k >= c.batch_k0
                                && c.k - c.batch_k0 < c.batch_ts.len() as u64)
                            {
                                let max = (c.idle_run + 1).min(TICK_BATCH);
                                if lp.read_ahead(c, integ, t_new, &d, max).is_err() {
                                    lp.read_ahead(c, integ, t_new, &d, 1)?;
                                }
                                c.batch_gen = dense_gen;
                            }
                            for k in 0..outs.len() {
                                c.outputs[k] = d[outs[k]];
                            }
                            let j = (c.k - c.batch_k0) as usize;
                            let row = &c.batch_in[j * n_in..][..n_in];
                            if let Err(m) = blk.tick(tk, row, &mut c.outputs) {
                                return Err(SolveError::Block {
                                    block: blk.name().into(),
                                    t: tk,
                                    message: m,
                                });
                            }
                            c.k += 1;
                            let mut changed = false;
                            for k in 0..outs.len() {
                                changed |= d[outs[k]] != c.outputs[k];
                            }
                            c.stop_next = changed;
                            if changed {
                                c.idle_run = 0;
                                report.block_changes += 1;
                                d_tick.copy_from_slice(&d);
                                for (k, &o) in outs.iter().enumerate() {
                                    d_tick[o] = c.outputs[k];
                                }
                                changed_blocks.push(first);
                                break;
                            }
                            c.idle_run += 1;
                            // (the next tick's time as read ahead: the
                            // same as `next`)
                            let x = c.batch_ts.get(j + 1).copied().unwrap_or_else(|| c.next());
                            if x < t_new && tg.is_none_or(|g| g >= x) && lone(x) {
                                tk = x;
                            } else {
                                break;
                            }
                        }
                    }
                    // every clock due at this instant ticks in it, in order,
                    // each reading its inputs with what the ones before it
                    // set, as at an event's instant: the instant's values
                    // are those after all of them
                    // (buffers kept from tick to tick: an idle tick
                    // allocates nothing)
                    due_clocks.clear();
                    if !ran {
                        due_clocks.extend((0..clocks.len()).filter(|&b| {
                            let x = clocks[b].next();
                            x == tk || same_instant(x, tk)
                        }));
                    }
                    // the discrete values as the blocks before left them:
                    // `d` itself until one changes an output
                    let mut d_dirty = false;
                    // the state with its iteration variables solved again
                    // for what the blocks before set, when a block reads
                    // them (and the discrete values they are solved for)
                    let mut yz: Option<(Vec<f64>, Vec<f64>)> = None;
                    for &b in &due_clocks {
                        report.block_ticks += 1;
                        let c = &mut clocks[b];
                        let dt: &[f64] = if d_dirty { &d_tick } else { &d };
                        // a clock whose ticks inside the step read what
                        // the step and the discrete values give: its next
                        // ticks' inputs read together (the same values),
                        // as many as it lately ticked without a change
                        let ahead = c.batchable && !d_dirty && tk < t_new && c.next() == tk;
                        if ahead
                            && !(c.batch_gen == dense_gen
                                && c.k >= c.batch_k0
                                && c.k - c.batch_k0 < c.batch_ts.len() as u64)
                        {
                            let max = (c.idle_run + 1).min(TICK_BATCH);
                            // (a later tick that cannot be read: this one
                            // alone, which fails where one by one would)
                            if lp.read_ahead(c, integ, t_new, dt, max).is_err() {
                                lp.read_ahead(c, integ, t_new, dt, 1)?;
                            }
                            c.batch_gen = dense_gen;
                        }
                        let mut inputs = if ahead { vec![] } else { std::mem::take(&mut c.inputs) };
                        let z_for = yz.as_ref().map_or(d.as_slice(), |x| x.1.as_slice());
                        // (no block changed anything yet: nothing moved)
                        if !ahead && c.reads_z && d_dirty && lp.moves_z(dt, z_for) {
                            if !have_y {
                                integ.interpolate(tk, &mut yk)?;
                                have_y = true;
                            }
                            let mut yv = yz.take().map_or_else(|| yk.clone(), |x| x.0);
                            integ.consistent_z(tk, &mut yv, dt)?;
                            report.z_solves += 1;
                            yz = Some((yv, dt.to_vec()));
                        }
                        let fresh = c.reads_z && yz.is_some();
                        if ahead {
                            // (read ahead)
                        } else if c.needs_vars || tk == t_new || fresh {
                            // the whole state (and the channels)
                            if !have_y {
                                integ.interpolate(tk, &mut yk)?;
                                have_y = true;
                            }
                            let ys = match &yz {
                                Some((v, _)) if c.reads_z => v.as_slice(),
                                _ => yk.as_slice(),
                            };
                            if c.reads_vars {
                                lp.sample(tk, ys, dt);
                            }
                            lp.read_inputs(b, ys, dt, &mut inputs);
                        } else {
                            // only the entries of y the block reads
                            if c.needs_y {
                                integ.interpolate_select(tk, &c.y_idx, &mut c.y_vals)?;
                            }
                            lp.read_inputs_selected(b, tk, c, dt, &mut inputs);
                        }
                        let outs = &info.blocks[b].outputs;
                        for (k, &o) in outs.iter().enumerate() {
                            c.outputs[k] = dt[o];
                        }
                        let blk = &mut blocks[b];
                        let row: &[f64] = if ahead {
                            let n_in = info.blocks[b].inputs.len();
                            &c.batch_in[(c.k - c.batch_k0) as usize * n_in..][..n_in]
                        } else {
                            &inputs
                        };
                        blk.tick(tk, row, &mut c.outputs).map_err(|m| SolveError::Block {
                            block: blk.name().into(),
                            t: tk,
                            message: m,
                        })?;
                        if !ahead {
                            c.inputs = inputs;
                        }
                        c.k += 1;
                        let changed = outs.iter().enumerate().any(|(k, &o)| dt[o] != c.outputs[k]);
                        c.stop_next = changed;
                        c.idle_run = if changed { 0 } else { c.idle_run + 1 };
                        if changed {
                            report.block_changes += 1;
                            if !d_dirty {
                                d_tick.copy_from_slice(&d);
                                d_dirty = true;
                            }
                            for (k, &o) in outs.iter().enumerate() {
                                d_tick[o] = c.outputs[k];
                            }
                            changed_blocks.push(b);
                        }
                    }
                    if ran {
                        due_clocks.push(first);
                    }
                    if changed_blocks.is_empty() {
                        continue;
                    }
                    // (what the change does to the integrator is read
                    // afresh; the channels at the step's end too)
                    dense_gen += 1;
                    // the outputs changed: an event at the tick; the rest of
                    // the step is dropped
                    if !have_y {
                        integ.interpolate(tk, &mut yk)?;
                    }
                    // (at the step's end, the channels the switch's look
                    // took there, where it is the same point: the same
                    // values)
                    let same_point = probe.valid
                        && probe.t == tk
                        && probe.y.len() == yk.len()
                        && probe.y.iter().zip(&yk).all(|(a, b)| a.to_bits() == b.to_bits());
                    if same_point {
                        std::mem::swap(&mut lp.vars, &mut probe.vars);
                        lp.after_t = None;
                        watch.probed(tk);
                    } else {
                        lp.sample(tk, &yk, &d);
                    }
                    probe.valid = false;
                    rec.interior(tk, lp.recorded());
                    // the choice of states up to the tick, before it
                    // changes anything (the step's dense output holds the
                    // stretch since the last point checked)
                    watch_step(watch, model, info, integ, tk, &lp.vars, &d, u, lp.events.len())?;
                    let before = ledger.as_mut().map(|lg| lg.before_event(tk, &lp.vars, p));
                    let before_vars = lp.vars.clone();
                    let d_pre = d.clone();
                    d.copy_from_slice(&d_tick);
                    for &b in &changed_blocks {
                        lp.record(tk, crate::EventKind::Block(b))?;
                    }
                    let outs: Vec<usize> = changed_blocks
                        .iter()
                        .flat_map(|&b| info.blocks[b].outputs.iter().copied())
                        .collect();
                    // (the point the integrator had, for the holds)
                    let y_at = lp.may_hold().then(|| yk.clone());
                    lp.scheduled = true;
                    let it = lp.settle(integ, tk, &mut yk, &mut d, &d_pre, None, &before_vars);
                    lp.scheduled = false;
                    let (_, imp) = it?;
                    integ.discrete_mut().copy_from_slice(&d);
                    // outputs that reach nothing the integrator integrates
                    // or watches (a value only shown as a channel): the
                    // solution is exactly the one without the tick, so the
                    // step stands (no cut, no restart)
                    let inert = imp.is_none()
                        && d.iter().zip(&d_pre).enumerate().all(|(i, (a, b))| {
                            a == b
                                || (outs.contains(&i)
                                    && !info.dynamic_discretes.get(i).copied().unwrap_or(true))
                        });
                    if inert {
                        report.inert_ticks += 1;
                        for &b in &due_clocks {
                            clocks[b].stop_next = false;
                        }
                        after_event(
                            &mut lp,
                            &mut rec,
                            integ,
                            &mut ledger,
                            before,
                            &before_vars,
                            tk,
                            &yk,
                            &d,
                            None,
                        )?;
                        // (nothing the choice of states reads changed: its
                        // stretch goes on)
                        watch.carry(lp.events.len());
                        if tk == t_new {
                            resumed = Some(yk.clone());
                        }
                        continue;
                    }
                    if let Some(e) = &imp {
                        report.impulses += e.count;
                    }
                    let at = y_at.as_deref().map(|y0| (y0, d_pre.as_slice()));
                    lp.update_sides(integ, tk, &yk, &d, None, at);
                    lp.push_sides(integ);
                    if imp.is_none()
                        && tk == t_new
                        && lp.slight(integ, tk, &mut yk, &d_pre, &d, &outs)?
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
                            &before_vars,
                            tk,
                            &yk,
                            &d,
                            None,
                        )?;
                        // (the choice of states from the point after it)
                        watch.resume(info, model, tk, &lp.vars, &d, lp.events.len())?;
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
                        &before_vars,
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
        match (&resumed, &y_cut) {
            (Some(after), _) => {
                y.copy_from_slice(after);
                lp.sample(t_new, &y, &d);
            }
            (None, cut) => {
                y.copy_from_slice(cut.as_deref().unwrap_or(integ.y()));
                // (the channels the switch's look at the step's end took,
                // where nothing has moved since: the same values)
                if probe.valid && probe.t == t_new {
                    std::mem::swap(&mut lp.vars, &mut probe.vars);
                    lp.after_t = None;
                    watch.probed(t_new);
                } else {
                    lp.sample(t_new, &y, &d);
                }
                rec.interior(t_new, lp.recorded());
            }
        }
        t = t_new;
        lp.check_asserts(t)?;
        // the switch readied in this step: made at the loop's top (an event
        // here: not made, the watch looks again from the point after it). A
        // tick at the point that changed the discrete values (going on
        // without a restart): readied again with them; where it cannot be
        // then, this point is checked as any step's end, and a step cut
        // back to it goes on from it
        let mut back = false;
        if let Some(r) = ready.take()
            && !event_now
            && r.t == t
        {
            if r.d == d {
                ready = Some(r);
                continue;
            }
            match ready_switch(
                opts,
                grid,
                integ,
                watch,
                info,
                r.next,
                r.k,
                t,
                &d,
                u,
                ledger.as_ref(),
                (r.cut, r.e),
            )? {
                Ok(again) => {
                    ready = Some(again);
                    continue;
                }
                Err((blame, factor, why)) => {
                    // (the step was cut back to the point to switch there:
                    // the integrator, past it, restarts at it from the
                    // step's dense output, as after a tick that changes
                    // what it integrates)
                    if switch_notes.len() < 20 {
                        let back = if r.cut {
                            "; the run goes on from the step's dense output there"
                        } else {
                            ""
                        };
                        switch_notes.push(format!(
                            "at t = {t:.9} s, the states were not chosen again with the discrete \
                             values a block's tick there set: {why}{back}"
                        ));
                    }
                    watch.refused(blame, factor.unwrap_or(r.e), &why);
                    back = r.cut;
                }
            }
        }
        watch_step(watch, model, info, integ, t, &lp.vars, &d, u, lp.events.len())?;
        if back {
            integ.restart(t, &y)?;
            y.copy_from_slice(integ.y());
        }
        // the holds of functions that reached their side, or left it; a
        // condition past its hold, which the integrator did not report (a
        // step's end is no root of it), crossed: an event here
        let mut crossed: Option<Vec<i32>> = None;
        if !event_now && lp.release_holds(t, &y, &d) {
            lp.push_sides(integ);
            crossed = (!lp.passed.is_empty()).then(|| lp.with_passed(None));
        }

        // a kink of a table read along time (index reduction differentiated
        // it): the integrator restarts, so that no step's history spans it
        if (stopped || is_root) && lp.at_kink(t) {
            integ.restart(t, &y)?;
            y.copy_from_slice(integ.y());
            report.kink_restarts += 1;
        }

        // a mode its condition no longer agrees with (one that left zero
        // right after a restart, where root finding cannot see it)
        if !event_now && (crossed.is_some() || !info.modes.is_empty() || !info.held.is_empty()) {
            // (the side first: a mode rarely disagrees)
            let stale = crossed.is_some() || {
                lp.eval_roots(t, &y, &d, false);
                info.modes.iter().any(|m| {
                    let g = lp.roots[m.crossing];
                    ((g > 0.0 && d[m.discrete] == 0.0) || (g < 0.0 && d[m.discrete] != 0.0))
                        && !lp.timed(m.crossing)
                }) || info.held.iter().flat_map(|h| &h.axes).any(|ax| {
                    // (an edge held inside, within its hold, is inside)
                    [ax.below, ax.above]
                        .iter()
                        .any(|&c| lp.roots[c] < -lp.hold.get(c).copied().unwrap_or(0.0))
                })
            };
            if stale {
                let before = ledger.as_mut().map(|lg| lg.before_event(t, &lp.vars, p));
                let before_vars = lp.vars.clone();
                let d_pre = d.clone();
                let y_at = lp.may_hold().then(|| y.clone());
                let dirs = crossed.as_deref();
                let (moved, imp) =
                    lp.settle(integ, t, &mut y, &mut d, &d_pre, dirs, &before_vars)?;
                // (a crossing found here restarts the integrator even where
                // it changed nothing: it took the condition as held)
                if moved || dirs.is_some() {
                    integ.discrete_mut().copy_from_slice(&d);
                    if let Some(e) = &imp {
                        report.impulses += e.count;
                    }
                    let at = y_at.as_deref().map(|y0| (y0, d_pre.as_slice()));
                    lp.update_sides(integ, t, &y, &d, dirs, at);
                    lp.push_sides(integ);
                    integ.restart(t, &y)?;
                    y.copy_from_slice(integ.y());
                    after_event(
                        &mut lp,
                        &mut rec,
                        integ,
                        &mut ledger,
                        before,
                        &before_vars,
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
            let y_at = lp.may_hold().then(|| y.clone());
            let mut changed = false;
            {
                // (as inside a step: the iteration variables solved again
                // for what the blocks before set, the channels sampled
                // again, when a block reads them)
                let mut yz: Option<(Vec<f64>, Vec<f64>)> = None;
                let mut resampled = false;
                for (b, c) in clocks.iter_mut().enumerate() {
                    if c.next() == t || same_instant(c.next(), t) {
                        report.block_ticks += 1;
                        let mut inputs = std::mem::take(&mut c.inputs);
                        let z_for = yz.as_ref().map_or(d_pre.as_slice(), |x| x.1.as_slice());
                        if c.reads_z && lp.moves_z(&d, z_for) {
                            let mut yv = yz.take().map_or_else(|| y.clone(), |x| x.0);
                            integ.consistent_z(t, &mut yv, &d)?;
                            report.z_solves += 1;
                            yz = Some((yv, d.clone()));
                        }
                        let ys = match &yz {
                            Some((v, _)) if c.reads_z => v.as_slice(),
                            _ => y.as_slice(),
                        };
                        if c.reads_vars && d != d_pre {
                            lp.sample(t, ys, &d);
                            resampled = true;
                        }
                        lp.read_inputs(b, ys, &d, &mut inputs);
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
                if resampled {
                    lp.vars.copy_from_slice(&before_vars);
                    lp.after_t = None;
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
                        let rising = match info.time_crossings.get(k) {
                            Some(Some(c)) => c.rising,
                            _ => lp.tf.dir[k] > 0,
                        };
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
            let it = lp.settle(integ, t, &mut y, &mut d, &d_pre, dirs, &before_vars);
            lp.scheduled = false;
            let (moved, imp) = it?;
            changed |= moved;
            if crossing_due {
                for k in 0..lp.t_star.len() {
                    if lp.t_star[k] == t || same_instant(lp.t_star[k], t) {
                        lp.t_star[k] = f64::NAN;
                        // (a time function changes sign again later)
                        lp.tf.stale[k] = lp.tf.searched(k);
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
            if changed {
                integ.discrete_mut().copy_from_slice(&d);
            }
            if let Some(e) = &imp {
                report.impulses += e.count;
            }
            let at = y_at.as_deref().map(|y0| (y0, d_pre.as_slice()));
            lp.update_sides(integ, t, &y, &d, dirs, at);
            lp.push_sides(integ);
            // (a step ended at a pulse the run loop found: the integrator,
            // past it, restarts here even when nothing changed; so it does
            // where the event put a crossing it reported back on the side
            // it came from: a mode whose relation, the iteration variables
            // solved again, does not hold yet. The integrator saw it cross
            // and would not see it cross again)
            let undone = is_root && dirs.is_some_and(|ds| lp.undone(ds));
            if changed || y_cut.is_some() || undone {
                integ.restart(t, &y)?;
                y.copy_from_slice(integ.y());
                after_event(
                    &mut lp,
                    &mut rec,
                    integ,
                    &mut ledger,
                    before,
                    &before_vars,
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
    // (the run's own integrator's notes: its start, its method)
    let start_notes = integ0.setup_notes();
    let integ: &mut dyn Integrator = match cur.integ.as_deref_mut() {
        Some(i) => i,
        None => &mut *integ0,
    };
    let (values, min, max, mean, left_limits) = rec.finish();
    let energy = match ledger {
        Some(mut lg) => {
            integ.quadrature(t_end, &mut lg.q)?;
            let ye = integ.y().to_vec();
            lp.sample(t_end, &ye, &d);
            let mut books = lg.finish(t_end, &lp.vars, p);
            books.error_controlled = opts.energy_error_control;
            Some(books)
        }
        None => None,
    };
    let mut accumulated: Vec<(String, f64)> = if errors_left.is_empty() {
        (0..n)
            .map(|i| {
                let scale = ymax[i].max(opts.atol * info.y_nominal[i]).max(f64::MIN_POSITIVE);
                (info.y_names.get(i).cloned().unwrap_or_default(), acc[i] / scale)
            })
            .collect()
    } else {
        // (by name, over the models the run was on)
        for i in 0..n {
            let name = info.y_names.get(i).cloned().unwrap_or_default();
            let (a, m, f) = (acc[i], ymax[i], opts.atol * info.y_nominal[i]);
            match errors_left.iter_mut().find(|x| x.0 == name) {
                Some(x) => {
                    x.1 += a;
                    x.2 = x.2.max(m);
                    x.3 = x.3.max(f);
                }
                None => errors_left.push((name, a, m, f)),
            }
        }
        errors_left
            .into_iter()
            .map(|(name, a, m, f)| (name, a / m.max(f).max(f64::MIN_POSITIVE)))
            .collect()
    };
    accumulated.sort_by(|a, b| b.1.total_cmp(&a.1));
    report.error = ErrorEstimate { accumulated, ..report.error };
    report.backend = integ.name();
    report.method = integ.method();
    report.notes = start_notes;
    if switches > 0 {
        report.notes.push(format!(
            "dynamic state selection: the states were chosen again {switches} times"
        ));
    }
    report.notes.append(&mut switch_notes);
    report.notes.append(&mut lp.notes.borrow_mut());
    report.stats = integ.stats();
    report.stats += stats_left;
    report.events = lp.events.len();
    // time spent outside tables' data (told once: the guards of a table's
    // reads at the same argument, one per read, say the same)
    for (k, g) in model.table_guard_list().iter().enumerate() {
        if let Some(t0) = lp.outside_since[k].take() {
            lp.outside_total[k].0 += t_end - t0;
        }
        let (total, first) = lp.outside_total[k];
        if total > 0.0 || !first.is_nan() {
            let name = info.table_names.get(g.table as usize).cloned().unwrap_or_default();
            let said = format!(
                "the table '{name}' was read outside its data on its {} axis for {total:.6} s, first at t = {first:.6} s",
                if g.axis == 0 { "first" } else { "second" }
            );
            if !report.warnings.contains(&said) {
                report.warnings.push(said);
            }
        }
    }
    for m in &lp.tf.mixed {
        report.mixed_certified += m.certified;
        report.mixed_scanned += m.scanned;
    }
    report.warnings.append(&mut lp.warnings);
    if let Some(e) = &energy {
        report.notes.push(e.summary());
        if !e.error_controlled {
            report.warnings.push(format!(
                "the energy books were integrated without error control (energy_error_control \
                 is off): their integrals ride on the states' steps and can be far off, the drift \
                 {:.1e} of the throughput says how far; the closure compares them with each other \
                 and can be zero all the same",
                e.relative_drift
            ));
        }
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
        names: match &opts.record {
            Some(record) => record.iter().map(|&c| info0.var_names[c].clone()).collect(),
            None => info0.var_names.clone(),
        },
        values,
        min,
        max,
        mean,
        left_limits,
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
    ledger: &mut Option<Ledger<'_>>,
    t: f64,
    yk: &mut [f64],
    d: &[f64],
) -> Result<(), SolveError> {
    integ.interpolate(t, yk)?;
    lp.sample(t, yk, d);
    rec.grid_point(t, lp.recorded());
    if let Some(lg) = ledger.as_mut() {
        integ.quadrature(t, &mut lg.q)?;
        lg.grid_point(t, &lp.vars, &lp.info.params);
    }
    Ok(())
}

/// The watch on index reduction's choice of states at `t`, a point the
/// step just taken reached (its end, or a tick inside it before the tick
/// changes anything): the channels `vars` there and the discrete values
/// `d` the step was taken with, the run having recorded `events` events;
/// the points inside the step from its dense output.
#[allow(clippy::too_many_arguments)]
fn watch_step(
    watch: &mut crate::states::Watch,
    model: &dyn ModelFunctions,
    info: &RunInfo,
    integ: &mut dyn Integrator,
    t: f64,
    vars: &[f64],
    d: &[f64],
    u: &[f64],
    events: usize,
) -> Result<(), SolveError> {
    if !watch.active() {
        return Ok(());
    }
    let l = *model.layout();
    // (allocated only where the step's inside is read)
    let mut y_in = vec![];
    let mut work_in = vec![];
    let mut inside = |tau: f64, v: &mut Vec<f64>| {
        y_in.resize(l.n_y(), 0.0);
        work_in.resize(l.n_work, 0.0);
        v.resize(l.n_vars, 0.0);
        if integ.interpolate(tau, &mut y_in).is_err() {
            return false;
        }
        let inp = EvalInput { t: tau, y: &y_in, p: &info.params, d, u };
        model.vars(&inp, &mut work_in, v);
        true
    };
    watch.end(info, model, t, vars, d, events, &mut inside)
}

/// After a restart at an event: the right limit is recorded (and a grid
/// point exactly at the event takes it, with `left`, the channels just
/// before the event, as its left limit), the stored-energy jump booked.
#[allow(clippy::too_many_arguments)]
fn after_event(
    lp: &mut Loop<'_>,
    rec: &mut Recorder,
    integ: &mut dyn Integrator,
    ledger: &mut Option<Ledger<'_>>,
    before: Option<Vec<f64>>,
    left: &[f64],
    t: f64,
    y: &[f64],
    d: &[f64],
    impulse: Option<&Engagement>,
) -> Result<(), SolveError> {
    lp.sample(t, y, d);
    lp.after_t = Some(t);
    lp.after_y.clear();
    lp.after_y.extend_from_slice(y);
    lp.after_d.clear();
    lp.after_d.extend_from_slice(d);
    rec.interior(t, lp.recorded());
    if let (Some(lg), Some(b)) = (ledger.as_mut(), before) {
        lg.after_event(t, &lp.vars, &lp.info.params, &b, impulse);
    }
    if rec.next_grid_time() == Some(t) {
        let left = lp.recorded_of(left);
        rec.left_limit(&left, lp.recorded());
        rec.grid_point(t, lp.recorded());
        if let Some(lg) = ledger.as_mut() {
            integ.quadrature(t, &mut lg.q)?;
            lg.grid_point(t, &lp.vars, &lp.info.params);
        }
    }
    Ok(())
}
