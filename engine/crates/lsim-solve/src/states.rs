//! The watch on index reduction's choice of states (DESIGN.md §6, step 5).
//!
//! Where constraints tie variables that appear differentiated together (a
//! pendulum's rod, a mechanism's joints), preparation chose once, at the
//! start, which of them the integrator integrates: for each differentiated
//! constraint, a candidate it decides (a dummy derivative), by Gaussian
//! elimination with a pivot within a factor 10 of the largest entry left,
//! heeding the model's `stateSelect` ([`lsim_ir::StateSet`]). The
//! candidates it did not take stay states. Along the motion the
//! constraints' derivatives move. Where the variables they decide come to
//! move far faster than the states they are decided from, the choice
//! decides them only through a nearly singular equation, and a little
//! further on not at all (a pendulum whose x is decided from y at the
//! bottom of its swing): the integrator then fails without saying why, or
//! passes the point on the wrong branch and returns a wrong answer without
//! a word. Where the model gives its other choices ([`RunInfo::choices`]),
//! the run chooses the states again before that ([`Watch::switching`],
//! [`crate::choice`]: dynamic state selection); without them, or where no
//! other choice works there, the run stops there instead, naming the
//! constraint and the parts.
//!
//! The measure ([`lsim_ir::choice`]): with `M` the constraints'
//! derivatives by the candidates and `S` the chosen columns, `B = M_S⁻¹ M`
//! holds how much each decided candidate moves per unit of each state
//! candidate while the constraints hold (`|dx/dy|` for the pendulum). Its
//! largest entry outside `S` is the factor by which exchanging that pair
//! would enlarge the chosen block's determinant (the choice's volume, as in
//! a basis exchange): 1 or less where no exchange helps. It depends neither
//! on how the constraints are written nor on the order the choice took
//! them in. A run stops
//!
//! * where that factor exceeds `1 /` [`DEGRADED`]: a decided variable
//!   moves a hundred times as much as a state (ten times beyond the factor
//!   10 the choice by pivoting allows at the start, twice the 50 a choice
//!   the model's marks ask for may start at: `lsim_prep::index::MARGIN`;
//!   so a run that starts at the edge of its choice's window does not stop
//!   at the first wobble), and carries a hundred times its error;
//! * where the chosen block's determinant changed sign between two points
//!   checked with no event between them (two step ends; the point after an
//!   event and the next step's end): it passed through zero inside the
//!   step, which the step ends alone may not show;
//! * at the start, where the constraints themselves no longer hold apart
//!   (the smallest pivot of `M`, its columns and rows scaled to their
//!   largest entries, below [`DEGRADED`]): no choice decides them, the
//!   parts are in a singular position (a mechanism's bars in line);
//! * where the integrator fails at a point whose factor is beyond the
//!   choice's own window ([`OUTSIDE`]), or whose constraints are that close
//!   to depending on each other: told as the choice's failure, with the
//!   integrator's words.
//!
//! Where a step's end shows the choice failing, the step's own dense output
//! (the integrator's interpolant, no event since the last point checked:
//! an event ends the stretch, the point after it starts the next, [`Watch`])
//! shows where: the run stops at the first point inside the step where the
//! factor passes the limit or the determinant its zero, found by
//! bisection, so a run stops at the same point whatever its tolerance and
//! however long its steps.
//!
//! The factor is in the candidates' own units ([`lsim_ir::choice`]): a set
//! whose entries move only at events (parameters, tables, discrete values:
//! a gear ratio) is judged only by whether its chosen block is singular, at
//! the start of each run (a parameter changed after preparation, a cached
//! model) and after each event that changes what it reads. Sets of
//! constants are never checked.
//!
//! The entries that move are evaluated at every step's end, compiled where
//! the model compiles them (the energy books' kernel, which compiles any
//! expression of the channels, bitwise the interpreter:
//! [`ModelFunctions::books_kernel`]), interpreted otherwise. A full check
//! (the exchange factors, the chosen block's sign) is made only where the
//! entries' change since the last one could have changed the verdict (a
//! bound, [`Watch`]).

use crate::SolveError;
use crate::info::{ChannelEnv, RunInfo, SetReads, StateSetInfo};
use lsim_ir::Expr;
use lsim_ir::runtime::{BooksKernel, BooksPart, BooksSpec, EvalInput, ModelFunctions};
use std::sync::Arc;

pub use lsim_ir::choice::{DEGRADED, PivotState, Scratch};

/// The choice's own window: it took no pivot below this share of the
/// largest entry. An integrator that fails where a decided candidate moves
/// more than `1 / OUTSIDE` times as much as a state is told as the
/// choice's failure.
pub const OUTSIDE: f64 = 0.1;

/// The factor a choice the model's `stateSelect` marks lead may reach
/// before the run chooses again (dynamic state selection): the margin
/// within which preparation takes a marked choice (`lsim_prep::index::WIDE`,
/// a decided candidate moving 50 times as much as a state, half the run's
/// limit), so a mark leads the choice wherever it is taken at all.
pub const MARKED: f64 = 0.5 / DEGRADED;

/// How the choice of `set` stands on its entries `m` (rows × columns;
/// changed): [`lsim_ir::choice::pivot_state`].
pub fn pivot_state(set: &StateSetInfo, m: &mut [Vec<f64>], s: &mut Scratch) -> PivotState {
    lsim_ir::choice::pivot_state(&set.pivots, &set.masked, m, s)
}

/// The entries of `set` at the channels `vars`, into `m`.
fn entries(
    set: &StateSetInfo,
    t: f64,
    vars: &[f64],
    params: &[f64],
    model: &dyn ModelFunctions,
    m: &mut Vec<Vec<f64>>,
) {
    let env = ChannelEnv { t, vars, params, model };
    m.resize(set.entries.len(), vec![]);
    for (row, out) in set.entries.iter().zip(m.iter_mut()) {
        out.clear();
        out.extend(row.iter().map(|e| lsim_ir::eval::eval(e, &env)));
    }
}

/// When a set's choice is told as failing.
enum How {
    /// at the start of the run
    Start,
    /// at a point of the run
    Run,
    /// its block's determinant passed through zero after this time (where
    /// exactly, the step's output could not show)
    Crossed(f64),
    /// the integrator failed there, saying this
    Failed(String),
}

/// The constraint a decided candidate belongs to: the first row that
/// reads it.
fn row_of(m: &[Vec<f64>], col: usize) -> usize {
    (0..m.len()).find(|&r| m[r].get(col).is_some_and(|x| *x != 0.0)).unwrap_or(0)
}

/// What the run tells where a set's choice fails (`m`: its entries there;
/// `scale_only`: the set is judged only by whether its block is singular;
/// `again`: why the run did not choose the states again, in words that
/// follow "and": [`Watch::again`]).
#[allow(clippy::too_many_arguments)]
fn failing(
    info: &RunInfo,
    k: usize,
    ps: &PivotState,
    m: &[Vec<f64>],
    t: f64,
    how: How,
    scale_only: bool,
    again: &str,
) -> SolveError {
    let set = &info.state_sets[k];
    let last = set.rows.len().saturating_sub(1);
    let keep = "Prepare the model at a start whose states suit the whole motion, mark the \
                variable to keep as a state with stateSelect = StateSelect.prefer (or always), \
                or describe the part in coordinates that stay regular through it (an angle)";
    let rebuild = "The states were chosen at the values the model was prepared at: build the \
                   model again at these values";
    let times = |e: f64| if e >= 100.0 { format!("{e:.0}") } else { format!("{e:.1}") };
    // the exchange that would serve: with a state the choice may take, or
    // with one it keeps above all
    let kept_worse = ps.exchange_kept > ps.exchange && !ps.exchange.is_nan();
    let (chosen, other) = if kept_worse {
        (&set.candidates[ps.chosen_kept], &set.candidates[ps.kept])
    } else {
        (&set.candidates[ps.chosen], &set.candidates[ps.best])
    };
    let row = &set.rows[row_of(m, if kept_worse { ps.chosen_kept } else { ps.chosen }).min(last)];
    let e = ps.worst_exchange();
    let moves = if e.is_finite() {
        format!("{chosen} moves {} times as much as {other} does", times(e))
    } else {
        format!("{chosen} does not follow from {other} at all")
    };
    let message = if e.is_nan() {
        format!(
            "the states chosen when the model was prepared cannot be judged here: {row} decides \
             {chosen} from the states, and depends on it by a value that is not a number"
        )
    } else if scale_only {
        let when = match &how {
            How::Start => "at the start, ",
            _ => "after the event here, ",
        };
        format!(
            "{when}the states chosen when the model was prepared do not work: {row} decides \
             {chosen} from the states, but with the values it reads now (parameters, tables, \
             discrete values) it does not depend on it at all. {rebuild}"
        )
    } else if ps.independence < DEGRADED && !matches!(how, How::Crossed(_)) {
        format!(
            "{} no longer decides anything here, for any choice of states: what is left of it \
             after the other constraints is {:.1e} of its size. The parts are in a singular \
             position (bars in line, a crank at a dead point), where the equations do not \
             decide the motion; a compliance in one joint (a stiff spring) carries the motion \
             through it",
            set.rows[ps.dep_row.min(last)],
            ps.independence
        )
    } else if kept_worse {
        let when = match &how {
            How::Start => "at the start",
            _ => "here",
        };
        let failed = match &how {
            How::Failed(why) => format!(", and the integrator stopped ({why})"),
            _ => String::new(),
        };
        format!(
            "the states chosen when the model was prepared no longer work: {row} decides \
             {chosen} from the states, but {when} {moves}{failed}. Only taking {chosen} as a \
             state in place of {other} would serve, and {other} is marked stateSelect = always \
             (or is a state a reinit restarts). Mark it prefer instead, or describe the part \
             in coordinates that stay regular through it (an angle)"
        )
    } else {
        let swap = format!(
            "The run would have to take {chosen} as a state in place of {other}, and {again}"
        );
        match &how {
            How::Start => format!(
                "at the start, the states chosen when the model was prepared do not work: {row} \
                 decides {chosen} from the states, but there {moves}. {rebuild}, or {}",
                lower_first(keep)
            ),
            How::Crossed(t0) => format!(
                "between t = {t0} and {t} s, the states chosen when the model was prepared \
                 stopped working: {row} decides {chosen} from the states, and somewhere there it \
                 did not depend on {chosen} at all (its determinant passed through zero). \
                 {swap}. {keep}"
            ),
            How::Run => format!(
                "the states chosen when the model was prepared no longer work: {row} decides \
                 {chosen} from the states, but here {moves}. {swap}. {keep}"
            ),
            How::Failed(why) => format!(
                "the states chosen when the model was prepared are failing here: {row} decides \
                 {chosen} from the states, but {moves}, and the integrator stopped ({why}). \
                 {swap}. {keep}"
            ),
        }
    };
    SolveError::StateSelection {
        t,
        message: if message.ends_with('.') { message } else { format!("{message}.") },
        parts: set.parts.clone(),
    }
}

fn lower_first(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_lowercase().chain(c).collect()).unwrap_or_default()
}

/// Whether a set's choice fails at the start, by what it reads.
fn fails(reads: SetReads, ps: &PivotState) -> bool {
    match reads {
        SetReads::Variables => {
            let e = ps.worst_exchange();
            e.is_nan() || e * DEGRADED > 1.0 || ps.independence < DEGRADED
        }
        // a parameter's scale is no degenerate motion: only a chosen block
        // that is exactly singular
        SetReads::Parameters => singular(ps),
        SetReads::Constants | SetReads::Unreadable => false,
    }
}

/// Whether a chosen block is singular, or not a number.
fn singular(ps: &PivotState) -> bool {
    ps.exchange.is_nan() || ps.sign == 0.0
}

/// Checks the choice of states at a run's start `(t, y, d)`: the sets
/// that read variables or parameters. Fails, naming the constraint and the
/// parts, where it has degraded or is singular (a model prepared at other
/// values: a parameter set later, a cached model).
pub fn check_start(
    model: &dyn ModelFunctions,
    info: &RunInfo,
    t: f64,
    y: &[f64],
    d: &[f64],
    u: &[f64],
) -> Result<(), SolveError> {
    if !info
        .state_sets
        .iter()
        .any(|s| matches!(s.reads, SetReads::Variables | SetReads::Parameters))
    {
        return Ok(());
    }
    let l = *model.layout();
    let mut work = vec![0.0; l.n_work];
    let mut vars = vec![0.0; l.n_vars];
    model.vars(&EvalInput { t, y, p: &info.params, d, u }, &mut work, &mut vars);
    let (mut m, mut raw, mut s) = (vec![], vec![], Scratch::default());
    // (the positions' sets, the last ones, first: their words are plainer)
    for (k, set) in info.state_sets.iter().enumerate().rev() {
        if !matches!(set.reads, SetReads::Variables | SetReads::Parameters) {
            continue;
        }
        let scale_only = set.reads == SetReads::Parameters || !moving_set(set, info);
        entries(set, t, &vars, &info.params, model, &mut m);
        raw.clone_from(&m);
        let ps = pivot_state(set, &mut m, &mut s);
        let reads = if scale_only { SetReads::Parameters } else { set.reads };
        if fails(reads, &ps) {
            return Err(failing(info, k, &ps, &raw, t, How::Start, scale_only, ""));
        }
    }
    Ok(())
}

/// Whether an entry moves with the solution between events: it reads a
/// variable that is not held between them (a state, an iteration
/// variable, one computed from them), or time.
fn moves(e: &Expr, info: &RunInfo) -> bool {
    use crate::info::VarSource;
    e.any(&mut |x| match x {
        Expr::Time => true,
        Expr::Var(v) => !matches!(
            info.var_sources.get(v.0 as usize),
            Some(VarSource::D(_) | VarSource::NegD(_) | VarSource::Const(_) | VarSource::U(_))
        ),
        _ => false,
    })
}

/// Whether any of a set's entries moves with the solution.
fn moving_set(set: &StateSetInfo, info: &RunInfo) -> bool {
    set.entries.iter().flatten().any(|e| moves(e, info))
}

/// The channels at a time inside the step just taken, from its dense
/// output (false where it cannot give them).
type Inside<'a> = dyn FnMut(f64, &mut Vec<f64>) -> bool + 'a;

/// One watched set in the run loop.
struct Watched {
    /// the set
    k: usize,
    /// whether its entries move with the solution (else it is judged only
    /// by whether its block is singular, after the events that change what
    /// it reads)
    moving: bool,
    /// the entries that move
    cells: Vec<Cell>,
    /// the discrete values the other entries read (indices into `d`), and
    /// their values when those entries were last read
    dvars: Vec<usize>,
    dvals: Vec<f64>,
    /// every entry at the last full check (the moving ones, at the last
    /// point checked, are [`Cell::now`]'s)
    m: Vec<Vec<f64>>,
    /// per row, at the last full check: the largest entry of that column
    /// of the chosen block's inverse ([`lsim_ir::choice::pivot_state_with`])
    reach: Vec<f64>,
    /// the larger exchange factor at the last full check
    e0: f64,
    /// the chosen block's sign at the last full check
    sign: f64,
    /// no event since the last point checked, whose sign is `sign`
    continuous: bool,
    /// the last step end checked (in full or by the bound)
    t_prev: f64,
    /// whether the entries that do not move are current
    fresh: bool,
    /// the factor (an exchange with a state the choice may take) past
    /// which the run chooses the states again (infinite: it does not)
    switch: f64,
    /// the switch threshold asked for ([`Watch::switching`]): `switch` is
    /// it times the factor where the entries that do not move were last
    /// read, where that is above one (their scale: gears' ratios between
    /// shafts move a decided speed several times as much as a state
    /// without any degeneracy), so a choice is made again where it has
    /// degraded from where it started, by the motion
    base: f64,
    /// the bound [`Watch::switch_point`] found at the point it last
    /// looked at, from the full check it kept there where it made one
    /// (`None`: it found none), for the check at that point
    probed: Option<f64>,
    /// why the run last could not choose its states again for this set
    /// ([`Watch::refused`])
    refusal: Option<String>,
}

/// The bound of [`Watch::check`] on set `w`'s factors with its moving
/// entries at `vals`: from those at its last full check, the change
/// carried through the block's inverse there, `(e0 + η) / (1 − g)`
/// (infinite where that change may make the block singular, `g ≥ 1`).
fn bound(w: &Watched, vals: &[f64]) -> f64 {
    let (mut g, mut eta) = (0.0f64, 0.0f64);
    for cell in &w.cells {
        let dm = (vals[cell.k] - cell.base).abs() * cell.reach;
        if cell.in_s {
            g += dm;
        } else {
            eta += dm;
        }
    }
    if g < 1.0 { (w.e0 + eta) / (1.0 - g) } else { f64::INFINITY }
}

/// An entry that moves with the solution.
struct Cell {
    /// its row and column
    r: usize,
    c: usize,
    /// which of the watch's distinct expressions it is
    k: usize,
    /// its column is one the choice decides
    in_s: bool,
    /// its value at the last full check
    base: f64,
    /// the largest entry of its row's column of the chosen block's inverse
    /// there
    reach: f64,
}

/// The moving entries compiled ([`ModelFunctions::books_kernel`]) and the
/// scratch they run in.
struct Compiled {
    kernel: Arc<dyn BooksKernel>,
    /// per expression: the kernel computes it
    covered: Vec<bool>,
    work: Vec<f64>,
    out: Vec<f64>,
    /// the rates the kernel would read for stored energies (none here)
    ydot: Vec<f64>,
}

/// The run loop's watch on the sets whose entries move.
///
/// At each step's end it reads the entries that move with the solution. A
/// full check (the exchange factors and the chosen block's sign) is made
/// where they may have changed the choice's standing: with `G` the
/// block's inverse at the last full check times the entries' change in
/// the block since, while `‖G‖∞ < 1` and the bound `(E₀ + η) / (1 −
/// ‖G‖∞)` on the exchange factors (`E₀` the last full check's, `η` the
/// change in the other columns carried through the inverse) stays below
/// `1 / DEGRADED`, no exchange factor can have reached it and the block's
/// determinant cannot have changed sign (`det(I + G) > 0` while `‖G‖ <
/// 1`), and the check is skipped (both norms taken from above: the
/// largest entry of each column of the inverse times the changes). A car's driveline, its gear ratio's rate
/// read with the shaft's speed, is checked in full where the speed has
/// moved enough, a small share of its steps. The entries that do not move
/// are read again where an event changed a discrete value they read.
///
/// An event ends the stretch the signs compare within, and the point after
/// it starts the next: the run loop gives the point before it (a step's
/// end, or a tick inside a step: [`Watch::end`]) and the point after it
/// ([`Watch::resume`]; [`Watch::begin`] where the integrator restarts
/// there), each checked as a step's end is, so the step after an event is
/// watched as any other: its sign compared with the point after the
/// event, its dense output searched from there. A tick that changes
/// nothing the integrator integrates (the step stands) changes nothing a
/// set reads, and the stretch goes on across it ([`Watch::carry`]). An
/// event the watch was not told of (the run recorded one since the last
/// point checked) still ends the stretch, unchecked. A changed discrete
/// value that a set's fixed entries read makes them be read again. Only
/// those values are compared at each step, not every discrete value of
/// the model.
///
/// It also keeps where each step started (its time, and the discrete
/// values, copied where an event has changed them), so that an integrator
/// that fails anywhere in the run is told with what the run can see there
/// (`run::integrator_failure`), never by SUNDIALS' flag alone.
pub(crate) struct Watch {
    sets: Vec<Watched>,
    /// the distinct expressions of the moving entries of every set (the
    /// levels of one constraint, its rows repeated, share them)
    exprs: Vec<Expr>,
    compiled: Option<Compiled>,
    s: Scratch,
    /// scratch rows
    tmp: Vec<Vec<f64>>,
    vals: Vec<f64>,
    /// the moving entries where [`Watch::switch_point`] looks, at time
    /// `probe_t`; `probe_ok`: the run checks that point next with the same
    /// channels ([`Watch::probed`]), so they are not evaluated again
    vals_probe: Vec<f64>,
    probe_t: f64,
    probe_ok: bool,
    /// scratch of [`Watch::switch_point`]'s checks
    reach_probe: Vec<f64>,
    /// the last point checked: its time, and how many events the run had
    /// recorded there
    at_t: f64,
    at_events: usize,
    /// scratch for the channels at the point after an event
    at_work: Vec<f64>,
    at_vars: Vec<f64>,
    /// where the last step started: its time, the discrete values, and how
    /// many events the run had recorded when they were copied
    from_t: f64,
    from_d: Vec<f64>,
    from_events: usize,
    /// why the run never chooses its states again (`None`: it does, where
    /// [`Watch::switching`] set the sets' thresholds)
    never: Option<String>,
}

impl Watch {
    /// The watch for `info`'s sets (the positions' first: their words are
    /// plainer), their moving entries compiled by `model` when `compiled`.
    pub(crate) fn new(info: &RunInfo, model: &dyn ModelFunctions, compiled: bool) -> Watch {
        use crate::info::VarSource;
        let mut exprs: Vec<Expr> = vec![];
        let sets: Vec<Watched> = info
            .state_sets
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, s)| s.reads == SetReads::Variables)
            .map(|(k, set)| {
                let cols = set.candidates.len();
                let mut cells = vec![];
                let mut dvars = vec![];
                for (r, row) in set.entries.iter().enumerate() {
                    for (c, e) in row.iter().enumerate() {
                        if moves(e, info) {
                            let k = exprs.iter().position(|x| x == e).unwrap_or_else(|| {
                                exprs.push(e.clone());
                                exprs.len() - 1
                            });
                            cells.push(Cell {
                                r,
                                c,
                                k,
                                in_s: set.pivots.contains(&c),
                                base: f64::NAN,
                                reach: f64::INFINITY,
                            });
                        } else {
                            e.walk(&mut |x| {
                                if let Expr::Var(v) = x
                                    && let Some(VarSource::D(i) | VarSource::NegD(i)) =
                                        info.var_sources.get(v.0 as usize)
                                    && !dvars.contains(i)
                                {
                                    dvars.push(*i);
                                }
                            });
                        }
                    }
                }
                Watched {
                    k,
                    moving: !cells.is_empty(),
                    cells,
                    dvals: vec![f64::NAN; dvars.len()],
                    dvars,
                    m: vec![vec![0.0; cols]; set.entries.len()],
                    reach: vec![],
                    e0: f64::INFINITY,
                    sign: f64::NAN,
                    continuous: false,
                    t_prev: f64::NAN,
                    fresh: false,
                    switch: f64::INFINITY,
                    base: f64::INFINITY,
                    probed: None,
                    refusal: None,
                }
            })
            .collect();
        let compiled = (compiled && !exprs.is_empty()).then(|| compile(&exprs, model)).flatten();
        Watch {
            sets,
            exprs,
            compiled,
            s: Scratch::default(),
            tmp: vec![],
            vals: vec![],
            vals_probe: vec![],
            probe_t: f64::NAN,
            probe_ok: false,
            reach_probe: vec![],
            at_t: f64::NAN,
            at_events: 0,
            at_work: vec![],
            at_vars: vec![],
            from_t: f64::NAN,
            from_d: vec![],
            from_events: usize::MAX,
            never: Some(
                "it keeps the states chosen at the start (the run was not set to choose them \
                 again)"
                    .into(),
            ),
        }
    }

    /// The watch choosing the states again where a set's factor passes
    /// `switch` (`MARKED`, if larger, for a set whose choice keeps a column
    /// the model marks to be kept: `stateSelect` leads it) times its factor
    /// where the entries that do not
    /// move were last read (the start, an event that changed what they
    /// read), where that is above one, where the run can
    /// ([`RunInfo::choices`]): [`Watch::switch_point`].
    pub(crate) fn switching(mut self, info: &RunInfo, switch: f64) -> Watch {
        if info.choices.is_none() {
            self.never = Some(
                "it keeps the states chosen at the start (the model was given no other choice \
                 of states to switch to)"
                    .into(),
            );
            return self;
        }
        if switch.is_nan() || switch <= 0.0 {
            self.never = Some(
                "it keeps the states chosen at the start (dynamic state selection is off: \
                 SolverOptions::state_switch is 0)"
                    .into(),
            );
            return self;
        }
        self.never = None;
        for w in &mut self.sets {
            if w.moving {
                let set = &info.state_sets[w.k];
                // (a choice that keeps a column the model marks to be kept
                // is the marks' choice: they lead it)
                let honoured = set
                    .kept
                    .iter()
                    .enumerate()
                    .any(|(c, &k)| k && !set.pivots.contains(&c) && !set.masked[c])
                    || set.kept.iter().zip(&set.masked).any(|(&k, &m)| k && m);
                w.base = if honoured { switch.max(MARKED) } else { switch };
                w.switch = w.base;
            }
        }
        self
    }

    /// The watch of a run that never chooses its states again, `why` (in
    /// words that follow "it keeps the states chosen at the start"):
    /// a backend that cannot be started again on other states.
    pub(crate) fn unswitched(mut self, why: &str) -> Watch {
        self.never = Some(format!("it keeps the states chosen at the start ({why})"));
        for w in &mut self.sets {
            w.switch = f64::INFINITY;
            w.base = f64::INFINITY;
        }
        self
    }

    /// Why the run did not choose the states of set `k` again, in words
    /// that follow "and": it never does (why), or the last time it tried
    /// it could not (why).
    fn again(&self, k: usize) -> String {
        if let Some(why) = &self.never {
            return why.clone();
        }
        match self.sets.iter().find(|w| w.k == k).and_then(|w| w.refusal.as_deref()) {
            Some(why) => format!("choosing the states again did not work here: {why}"),
            None => "the run did not come to choose the states again before this point".into(),
        }
    }

    /// Set `k`'s switch threshold (infinite: it is not switched).
    pub(crate) fn threshold(&self, k: usize) -> f64 {
        self.sets.iter().find(|w| w.k == k).map_or(f64::INFINITY, |w| w.switch)
    }

    /// Whether the watch may ask for the states to be chosen again.
    pub(crate) fn may_switch(&self) -> bool {
        self.sets.iter().any(|w| w.switch.is_finite())
    }

    /// Whether there is anything to watch.
    pub(crate) fn active(&self) -> bool {
        !self.sets.is_empty()
    }

    /// Where the states should be chosen again inside the step just taken
    /// to `t` (`vars` the channels there, `d` the discrete values, the run
    /// having recorded `events` events): the first point after the last
    /// point checked where a set's factor (an exchange with a state the
    /// choice may take) passes its switch threshold, or its block's
    /// determinant changes sign, found by bisection on the step's dense
    /// output (`inside`; the step's end where it cannot be had), and the
    /// set (its index in [`RunInfo::state_sets`]). `None`: no set needs it.
    /// What the watch keeps is not changed: the points before the one
    /// found are checked as they come ([`Watch::end`]).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn switch_point(
        &mut self,
        info: &RunInfo,
        model: &dyn ModelFunctions,
        t: f64,
        vars: &[f64],
        d: &[f64],
        events: usize,
        inside: &mut dyn FnMut(f64, &mut Vec<f64>) -> bool,
    ) -> Option<(f64, usize)> {
        self.probe_ok = false;
        self.probe_t = f64::NAN;
        if !self.may_switch() || events != self.at_events {
            return None;
        }
        let mut vals = std::mem::take(&mut self.vals_probe);
        moving_values(&self.exprs, self.compiled.as_mut(), info, model, t, vars, &mut vals);
        self.probe_t = t;
        let mut found: Option<(f64, usize)> = None;
        for j in 0..self.sets.len() {
            self.sets[j].probed = None;
            let w = &self.sets[j];
            if !w.moving
                || !w.switch.is_finite()
                || !w.fresh
                || !w.continuous
                || w.t_prev >= t
                || w.t_prev.is_nan()
            {
                continue;
            }
            // (the discrete values the fixed entries read: as last read)
            if w.dvars.iter().zip(&w.dvals).any(|(&i, &v)| d[i].to_bits() != v.to_bits()) {
                continue;
            }
            // the bound of [`Watch::check`], against the switch threshold
            let b = bound(w, &vals);
            if b < w.switch {
                self.sets[j].probed = Some(b);
                continue;
            }
            self.tmp.clone_from(&w.m);
            for cell in &w.cells {
                self.tmp[cell.r][cell.c] = vals[cell.k];
            }
            let set = &info.state_sets[w.k];
            let mut reach = std::mem::take(&mut self.reach_probe);
            let ps = lsim_ir::choice::pivot_state_with(
                &set.pivots,
                &set.masked,
                &mut self.tmp,
                &mut self.s,
                false,
                Some(&mut reach),
            );
            let crossed = w.sign * ps.sign < 0.0;
            if !(ps.exchange > w.switch || ps.exchange.is_nan() || crossed) {
                // (nothing to choose again: the check here, as the step's
                // end's would make it, is kept for the bound's next look)
                let e = ps.worst_exchange();
                if e * DEGRADED <= 1.0 {
                    let w = &mut self.sets[j];
                    for cell in &mut w.cells {
                        w.m[cell.r][cell.c] = vals[cell.k];
                        cell.base = vals[cell.k];
                        cell.reach = reach[cell.r];
                    }
                    std::mem::swap(&mut w.reach, &mut reach);
                    w.e0 = e;
                    w.sign = ps.sign;
                    // (the bound from it, there: its factor)
                    w.probed = Some(e);
                }
                self.reach_probe = reach;
                continue;
            }
            self.reach_probe = reach;
            let w = &self.sets[j];
            let (t0, sign0, limit) = (w.t_prev, w.sign, w.switch);
            let k = w.k;
            // at the step's end, where the integrator's corrector holds the
            // solution (the constraints met, the error tested), unless the
            // step went past the point: its block's determinant changed sign
            // in it, or its factor there is beyond the choice's own window
            // (the decided variables there carry more than ten times a
            // state's error); then where the factor first passes the
            // threshold inside it, on its dense output. (Cut there every
            // time, the run goes on from interpolated iteration variables,
            // and the outside judge's pendulums come out further off:
            // DESIGN.md §15.)
            let at = if !crossed && ps.exchange * OUTSIDE <= 1.0 {
                t
            } else {
                self.locate(j, info, model, t0, t, sign0, inside, Some(limit))
                    .map_or(t, |(at, _)| at)
            };
            if found.is_none_or(|(f, _)| at < f) {
                found = Some((at, k));
            }
        }
        self.vals_probe = vals;
        found
    }

    /// The point [`Watch::switch_point`] last looked at, `t`, is checked
    /// next with the same channels: the moving entries it evaluated there
    /// are those.
    pub(crate) fn probed(&mut self, t: f64) {
        self.probe_ok = self.probe_t == t;
    }

    /// A switch the run could not make for set `k` (no better choice
    /// there), `why`: the set is not switched again before its factor has
    /// grown past twice `factor` (the run's limit still stops it).
    pub(crate) fn refused(&mut self, k: usize, factor: f64, why: &str) {
        for w in &mut self.sets {
            if w.k == k {
                let next = (2.0 * factor).max(2.0 * w.switch);
                w.switch = if next * DEGRADED < 1.0 { next } else { f64::INFINITY };
                w.refusal = Some(why.to_string());
            }
        }
    }

    /// Whether the choice of a model the run switches to holds at `(t, y,
    /// d)`: each set that moves regular there, its constraints apart, its
    /// factor inside its switch threshold and the run's limit. Why not,
    /// where it does not.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn holds(
        &mut self,
        info: &RunInfo,
        model: &dyn ModelFunctions,
        t: f64,
        y: &[f64],
        d: &[f64],
        u: &[f64],
    ) -> Result<(), String> {
        let l = *model.layout();
        let mut work = vec![0.0; l.n_work];
        let mut vars = vec![0.0; l.n_vars];
        model.vars(&EvalInput { t, y, p: &info.params, d, u }, &mut work, &mut vars);
        let ks: Vec<(usize, f64)> =
            self.sets.iter().filter(|w| w.moving).map(|w| (w.k, w.switch)).collect();
        for (k, switch) in ks {
            let (e, apart) = self.standing(info, model, k, t, &vars);
            let set = &info.state_sets[k];
            if !apart {
                return Err(format!(
                    "{} does not decide the variables for any choice of states there",
                    set.rows.first().map_or("a constraint", String::as_str)
                ));
            }
            if e.is_nan() || e >= switch || e * DEGRADED > 1.0 {
                return Err(format!(
                    "the states chosen again there would have a variable that {} decides move \
                     {e:.1} times as much as a state",
                    set.rows.first().map_or("a constraint", String::as_str)
                ));
            }
        }
        Ok(())
    }

    /// The largest factor of set `k` (an exchange with a state the choice
    /// may take) and whether its constraints hold apart, at the channels
    /// `vars` (`t`, the discrete values those were read with being current).
    pub(crate) fn standing(
        &mut self,
        info: &RunInfo,
        model: &dyn ModelFunctions,
        k: usize,
        t: f64,
        vars: &[f64],
    ) -> (f64, bool) {
        let set = &info.state_sets[k];
        let mut m = vec![];
        entries(set, t, vars, &info.params, model, &mut m);
        let ps = pivot_state(set, &mut m, &mut self.s);
        (ps.exchange, ps.independence >= DEGRADED && !singular(&ps))
    }

    /// The point `(t, d)` the run loop is at, the run having recorded
    /// `events` events; `stretch`: whether it is in the stretch of the last
    /// point checked (a step's end, or a point before an event inside it),
    /// else it starts a new one (the point after an event). An event the
    /// watch was not told of since the last point checked ends the stretch
    /// too, and a discrete value a set's fixed entries read that changed
    /// makes them be read again.
    fn arrive(&mut self, t: f64, d: &[f64], events: usize, stretch: bool) {
        let event = !stretch || events != self.at_events;
        self.at_t = t;
        self.at_events = events;
        for w in &mut self.sets {
            if event {
                w.continuous = false;
            }
            if w.fresh && w.dvars.iter().zip(&w.dvals).any(|(&i, &v)| d[i].to_bits() != v.to_bits())
            {
                w.fresh = false;
                w.continuous = false;
            }
        }
    }

    /// Before a step from `(t, y, d)` (the inputs `u`), the run having
    /// recorded `events` events. Where an event came since the last point
    /// checked (or the run starts here), this point, after it, starts the
    /// next stretch ([`Watch::resume`]): fails where the choice fails
    /// here, and gives the set whose states should be chosen again here
    /// (its factor past its switch threshold: [`Watch::switching`]).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn begin(
        &mut self,
        info: &RunInfo,
        model: &dyn ModelFunctions,
        t: f64,
        y: &[f64],
        d: &[f64],
        u: &[f64],
        events: usize,
    ) -> Result<Option<usize>, SolveError> {
        self.from_t = t;
        if events != self.from_events || self.from_d.len() != d.len() {
            self.from_d.clear();
            self.from_d.extend_from_slice(d);
            self.from_events = events;
        }
        if !self.active() || (events == self.at_events && t == self.at_t) {
            return Ok(None);
        }
        let l = *model.layout();
        let mut vars = std::mem::take(&mut self.at_vars);
        self.at_work.resize(l.n_work, 0.0);
        vars.resize(l.n_vars, 0.0);
        model.vars(&EvalInput { t, y, p: &info.params, d, u }, &mut self.at_work, &mut vars);
        let checked = self.resume(info, model, t, &vars, d, events);
        let switch = match checked {
            Ok(()) if self.may_switch() => self.wants_switch(info, model, t, &vars),
            _ => None,
        };
        self.at_vars = vars;
        checked.map(|()| switch)
    }

    /// The set (its index in [`RunInfo::state_sets`]) whose factor at the
    /// channels `vars` is past its switch threshold, if any.
    fn wants_switch(
        &mut self,
        info: &RunInfo,
        model: &dyn ModelFunctions,
        t: f64,
        vars: &[f64],
    ) -> Option<usize> {
        // (the moving entries here are those the check here just
        // evaluated: a set whose bound keeps it under its threshold needs
        // no look in full)
        let ks: Vec<(usize, f64)> = self
            .sets
            .iter()
            .filter(|w| {
                let b = bound(w, &self.vals);
                w.switch.is_finite() && (b >= w.switch || b.is_nan())
            })
            .map(|w| (w.k, w.switch))
            .collect();
        ks.into_iter().find_map(|(k, switch)| {
            let (e, _) = self.standing(info, model, k, t, vars);
            (e.is_nan() || e > switch).then_some(k)
        })
    }

    /// The point `(t, vars, d)` right after an event (the run having
    /// recorded `events` events), or where the run starts: the next
    /// stretch starts here. Each set is checked as at a step's end, its
    /// sign here being what the next step's end is compared with; fails
    /// where the choice fails here.
    pub(crate) fn resume(
        &mut self,
        info: &RunInfo,
        model: &dyn ModelFunctions,
        t: f64,
        vars: &[f64],
        d: &[f64],
        events: usize,
    ) -> Result<(), SolveError> {
        if !self.active() {
            return Ok(());
        }
        self.arrive(t, d, events, false);
        self.check(info, model, t, vars, d, None)
    }

    /// An event at the last point checked, the run now having recorded
    /// `events` events, that changes nothing the choice reads (a tick whose
    /// outputs reach nothing the integrator integrates or watches: the
    /// step stands): the stretch goes on across it.
    pub(crate) fn carry(&mut self, events: usize) {
        self.at_events = events;
    }

    /// Where the last step started: its time and discrete values.
    pub(crate) fn from(&self) -> (f64, &[f64]) {
        (self.from_t, &self.from_d)
    }

    /// At a step's end `t` (or a point inside the step, before an event
    /// there), the channels `vars` just sampled there and the discrete
    /// values `d` the step was taken with, the run having recorded `events`
    /// events; `inside(τ, vars)` gives the channels at a time `τ` inside
    /// the step just taken from its dense output (false where it cannot).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn end(
        &mut self,
        info: &RunInfo,
        model: &dyn ModelFunctions,
        t: f64,
        vars: &[f64],
        d: &[f64],
        events: usize,
        inside: &mut dyn FnMut(f64, &mut Vec<f64>) -> bool,
    ) -> Result<(), SolveError> {
        if !self.active() {
            return Ok(());
        }
        self.arrive(t, d, events, true);
        self.check(info, model, t, vars, d, Some(inside))
    }

    /// Checks each set at `(t, vars, d)` ([`Watch::end`], [`Watch::resume`]):
    /// a sign compared with the last point checked where it is in that
    /// point's stretch, and `inside` searched where the choice fails.
    fn check(
        &mut self,
        info: &RunInfo,
        model: &dyn ModelFunctions,
        t: f64,
        vars: &[f64],
        d: &[f64],
        mut inside: Option<&mut Inside<'_>>,
    ) -> Result<(), SolveError> {
        // every moving entry, each distinct one once (where the switch's
        // look evaluated them at these channels already: those)
        let probed =
            self.probe_ok && self.probe_t == t && self.vals_probe.len() == self.exprs.len();
        if probed {
            std::mem::swap(&mut self.vals, &mut self.vals_probe);
        } else {
            moving_values(
                &self.exprs,
                self.compiled.as_mut(),
                info,
                model,
                t,
                vars,
                &mut self.vals,
            );
        }
        self.probe_ok = false;
        for j in 0..self.sets.len() {
            let w = &mut self.sets[j];
            let set = &info.state_sets[w.k];
            let reread = !w.fresh;
            if w.fresh {
                if !w.moving {
                    // nothing it reads has changed
                    w.t_prev = t;
                    continue;
                }
                // the moving entries' change since the last full check,
                // carried through the block's inverse then: in the block
                // (g ≥ ‖G‖∞) and in the other columns (η, summed over them:
                // at least the largest column's); where the switch's look
                // here found it, that
                let b = match w.probed {
                    Some(b) if probed => b,
                    _ => bound(w, &self.vals),
                };
                if b * DEGRADED < 1.0 {
                    // (the sign is the last full check's here too)
                    w.continuous = true;
                    w.t_prev = t;
                    continue;
                }
            } else {
                entries(set, t, vars, &info.params, model, &mut w.m);
                for (v, &i) in w.dvals.iter_mut().zip(&w.dvars) {
                    *v = d[i];
                }
                w.fresh = true;
            }
            for cell in &w.cells {
                w.m[cell.r][cell.c] = self.vals[cell.k];
            }
            // a full check
            self.tmp.clone_from(&w.m);
            let ps = lsim_ir::choice::pivot_state_with(
                &set.pivots,
                &set.masked,
                &mut self.tmp,
                &mut self.s,
                false,
                Some(&mut w.reach),
            );
            for cell in &mut w.cells {
                cell.base = w.m[cell.r][cell.c];
                cell.reach = w.reach[cell.r];
            }
            let e = ps.worst_exchange();
            // (the switch threshold from the scale the entries read now)
            if reread && w.base.is_finite() && w.moving {
                w.switch = if ps.exchange > 1.0 { w.base * ps.exchange } else { w.base };
            }
            let (sign0, continuous, t0) = (w.sign, w.continuous, w.t_prev);
            let crossed = continuous && sign0 * ps.sign < 0.0;
            w.e0 = e;
            w.sign = ps.sign;
            w.continuous = true;
            w.t_prev = t;
            let bad =
                if w.moving { e.is_nan() || e * DEGRADED > 1.0 || crossed } else { singular(&ps) };
            if !bad {
                continue;
            }
            let (k, moving) = (w.k, w.moving);
            // where inside the step it went wrong (no event since the last
            // point checked: the step's dense output holds it)
            let found = match inside.as_deref_mut() {
                Some(inside) if moving && continuous && t0 < t => {
                    self.locate(j, info, model, t0, t, sign0, inside, None)
                }
                _ => None,
            };
            let (at, m, how) = match found {
                Some((at, m)) => (at, m, How::Run),
                None => {
                    let how =
                        if crossed && e * DEGRADED <= 1.0 { How::Crossed(t0) } else { How::Run };
                    (t, self.sets[j].m.clone(), how)
                }
            };
            // (in full, for the words)
            self.tmp.clone_from(&m);
            let full = pivot_state(set, &mut self.tmp, &mut self.s);
            let how = match how {
                // a sign that changed with every factor bounded: the
                // constraints depend on each other there, or the bisection
                // could not show where (told as a crossing)
                How::Run
                    if full.worst_exchange() * DEGRADED <= 1.0
                        && full.independence >= DEGRADED
                        && !singular(&full) =>
                {
                    How::Crossed(t0)
                }
                other => other,
            };
            let again = self.again(k);
            return Err(failing(info, k, &full, &m, at, how, !moving, &again));
        }
        Ok(())
    }

    /// The first point in `(t0, t1]` where set `j`'s choice fails (its
    /// factor past the limit, or its block's sign other than `sign0`), by
    /// bisection on the step's dense output (`inside`), and the set's
    /// entries there; `None` where the dense output is not to be had.
    /// With `switch`, the first point where its factor (an exchange with a
    /// state the choice may take) passes `switch` instead of the limit.
    #[allow(clippy::too_many_arguments)]
    fn locate(
        &mut self,
        j: usize,
        info: &RunInfo,
        model: &dyn ModelFunctions,
        t0: f64,
        t1: f64,
        sign0: f64,
        inside: &mut dyn FnMut(f64, &mut Vec<f64>) -> bool,
        switch: Option<f64>,
    ) -> Option<(f64, Vec<Vec<f64>>)> {
        let set = &info.state_sets[self.sets[j].k];
        let (mut lo, mut hi) = (t0, t1);
        let mut m_hi = self.sets[j].m.clone();
        let mut m = m_hi.clone();
        let (mut vars, mut vals) = (vec![], vec![]);
        for _ in 0..64 {
            let mid = 0.5 * (lo + hi);
            if mid <= lo || mid >= hi {
                break;
            }
            if !inside(mid, &mut vars) {
                return None;
            }
            moving_values(&self.exprs, self.compiled.as_mut(), info, model, mid, &vars, &mut vals);
            // (the other entries do not move inside the step)
            for cell in &self.sets[j].cells {
                m[cell.r][cell.c] = vals[cell.k];
            }
            self.tmp.clone_from(&m);
            let ps = pivot_state(set, &mut self.tmp, &mut self.s);
            let past = match switch {
                Some(limit) => ps.exchange.is_nan() || ps.exchange > limit,
                None => {
                    let e = ps.worst_exchange();
                    e.is_nan() || e * DEGRADED > 1.0
                }
            };
            if past || sign0 * ps.sign <= 0.0 {
                hi = mid;
                m_hi.clone_from(&m);
            } else {
                lo = mid;
            }
        }
        Some((hi, m_hi))
    }

    /// An integrator's failure `e` at the last point checked, told as the
    /// choice of states' failure where the choice there was beyond its own
    /// window, or the constraints near dependent, with the integrator's
    /// words; `None` where the choice is not to blame.
    pub(crate) fn blamed(&mut self, info: &RunInfo, e: &SolveError) -> Option<SolveError> {
        let SolveError::Integrator { t, message } = e else { return None };
        let mut worst: Option<(usize, PivotState, f64)> = None;
        for (j, w) in self.sets.iter_mut().enumerate() {
            if !w.fresh || !w.moving {
                continue;
            }
            // (the entries at the last point checked)
            for cell in &w.cells {
                w.m[cell.r][cell.c] = self.vals[cell.k];
            }
            let set = &info.state_sets[w.k];
            self.tmp.clone_from(&w.m);
            let ps = pivot_state(set, &mut self.tmp, &mut self.s);
            let near = (ps.worst_exchange() * OUTSIDE).max(OUTSIDE / ps.independence);
            if worst.as_ref().is_none_or(|(_, _, n)| near > *n) {
                worst = Some((j, ps, near));
            }
        }
        match worst {
            Some((j, ps, near)) if near > 1.0 || near.is_nan() => {
                let w = &self.sets[j];
                let again = self.again(w.k);
                Some(failing(info, w.k, &ps, &w.m, *t, How::Failed(message.clone()), false, &again))
            }
            _ => None,
        }
    }
}

/// The moving entries `exprs` compiled by `model`'s energy books' kernel
/// (each one a part's power in), or `None` where it compiles none.
fn compile(exprs: &[Expr], model: &dyn ModelFunctions) -> Option<Compiled> {
    let n = exprs.len();
    let spec = BooksSpec {
        parts: exprs
            .iter()
            .enumerate()
            .map(|(k, e)| BooksPart {
                power: e.clone(),
                loss: None,
                stored: None,
                exact: false,
                power_at: k,
                abs_at: n + k,
                loss_at: None,
                stored_at: None,
            })
            .collect(),
        chain: vec![],
        rates: vec![],
        n_out: 2 * n,
    };
    let kernel = model.books_kernel(&spec)?;
    let covered: Vec<bool> = (0..n).map(|k| kernel.cover(k).power).collect();
    if !covered.contains(&true) {
        return None;
    }
    Some(Compiled {
        work: vec![0.0; kernel.scratch()],
        out: vec![0.0; 2 * n],
        ydot: vec![0.0; model.layout().n_y()],
        covered,
        kernel,
    })
}

/// The moving entries `exprs` at time `t` and the channels `vars`, into
/// `out`: compiled where they are, interpreted otherwise (bitwise the
/// same).
fn moving_values(
    exprs: &[Expr],
    compiled: Option<&mut Compiled>,
    info: &RunInfo,
    model: &dyn ModelFunctions,
    t: f64,
    vars: &[f64],
    out: &mut Vec<f64>,
) {
    let env = ChannelEnv { t, vars, params: &info.params, model };
    out.clear();
    match compiled {
        Some(c) => {
            c.kernel.eval(t, vars, &c.ydot, &info.params, &mut c.work, &mut c.out);
            out.extend(
                exprs.iter().enumerate().map(|(k, e)| {
                    if c.covered[k] { c.out[k] } else { lsim_ir::eval::eval(e, &env) }
                }),
            );
        }
        None => out.extend(exprs.iter().map(|e| lsim_ir::eval::eval(e, &env))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsim_ir::runtime::Layout;

    fn set(pivots: Vec<usize>, cols: usize) -> StateSetInfo {
        StateSetInfo {
            entries: vec![vec![Expr::Const(0.0); cols]; pivots.len()],
            pivots,
            masked: vec![false; cols],
            reads: SetReads::Variables,
            rows: vec!["'pendulum' (the rod keeps its length)".into(); 3],
            parts: vec!["pendulum".into()],
            candidates: (0..cols).map(|k| format!("c{k}")).collect(),
            kept: vec![false; cols],
        }
    }

    fn state(s: &StateSetInfo, mut m: Vec<Vec<f64>>) -> PivotState {
        pivot_state(s, &mut m, &mut Scratch::default())
    }

    #[test]
    fn a_choice_fails_by_what_its_entries_read() {
        // the rod's [2x 2y] deciding x: x a 200th of y
        let low = state(&set(vec![0], 2), vec![vec![0.01, -2.0]]);
        assert!(fails(SetReads::Variables, &low));
        // a parameter's scale is no failure (a gear ratio of 1e-3); zero is
        let ps = state(&set(vec![1], 2), vec![vec![1.0, -1e-3]]);
        assert!(!fails(SetReads::Parameters, &ps) && fails(SetReads::Variables, &ps));
        let ps = state(&set(vec![1], 2), vec![vec![1.0, 0.0]]);
        assert!(fails(SetReads::Parameters, &ps));
        // rows that depend on each other: every choice fails
        let ps = state(&set(vec![0, 1], 3), vec![vec![1.0, 1.0, 0.5], vec![1.0, 1.0 + 1e-5, 0.5]]);
        assert!(fails(SetReads::Variables, &ps));
        // a column kept above all that the choice can no longer do without
        let mut s = set(vec![1], 3);
        s.masked = vec![true, false, false];
        assert!(!fails(SetReads::Variables, &state(&s, vec![vec![10.0, 0.5, 1.0]])));
        assert!(fails(SetReads::Variables, &state(&s, vec![vec![100.0, 0.5, 1.0]])));
    }

    struct NoModel(Layout);
    impl ModelFunctions for NoModel {
        fn layout(&self) -> &Layout {
            &self.0
        }
        fn residual(&self, _: &EvalInput<'_>, _: &mut [f64], _: &mut [f64]) {}
        fn jvp(&self, _: &EvalInput<'_>, _: &[f64], _: &mut [f64], _: &mut [f64]) {}
        fn roots(&self, _: &EvalInput<'_>, _: &mut [f64], _: &mut [f64]) {}
        fn vars(&self, _: &EvalInput<'_>, _: &mut [f64], _: &mut [f64]) {}
        fn when(&self, _: &EvalInput<'_>, _: &[f64], _: &mut [f64], _: &mut [f64]) {}
        fn start(&self, _: &[f64], _: &mut [f64], _: &mut [f64]) {}
    }

    fn no_model() -> NoModel {
        NoModel(Layout {
            n_x: 0,
            n_z: 0,
            n_p: 0,
            n_d: 0,
            n_u: 0,
            n_roots: 0,
            n_whens: 0,
            n_vars: 2,
            n_work: 0,
        })
    }

    /// The rod's [2x 2y], deciding x; channels x, y.
    fn rod() -> RunInfo {
        let two = |v: u32| Expr::Const(2.0) * Expr::Var(lsim_ir::VarId(v));
        let mut info = RunInfo::bare(0, 2, vec![]);
        info.state_sets = vec![StateSetInfo {
            entries: vec![vec![two(0), two(1)]],
            candidates: vec!["x".into(), "y".into()],
            ..set(vec![0], 2)
        }];
        info
    }

    #[test]
    fn a_determinant_that_changes_sign_between_step_ends_stops_the_run() {
        let (model, info) = (no_model(), rod());
        let mut none = |_: f64, _: &mut Vec<f64>| false;
        let mut w = Watch::new(&info, &model, false);
        assert!(w.active());
        w.end(&info, &model, 0.10, &[0.3, -0.95], &[], 0, &mut none).expect("regular");
        // a step that passes the bottom: x changes sign, |x| never small at
        // a step's end; with no dense output, told between the step ends
        let e = w.end(&info, &model, 0.12, &[-0.3, -0.95], &[], 0, &mut none).unwrap_err();
        let SolveError::StateSelection { t, message, .. } = &e else { panic!("{e}") };
        assert_eq!(*t, 0.12);
        assert!(message.contains("between t = 0.1 and 0.12 s"), "{e}");
        // an event between them that the watch was not told of (the run
        // recorded one): no compare
        let mut w = Watch::new(&info, &model, false);
        w.end(&info, &model, 0.10, &[0.3, -0.95], &[0.0], 0, &mut none).expect("regular");
        w.end(&info, &model, 0.12, &[-0.3, -0.95], &[1.0], 1, &mut none).expect("a new stretch");
        // a pivot a hundredth of the largest: stops, the integrator's
        // failure beside it told as the choice's
        let e = w.end(&info, &model, 0.13, &[0.009, -1.0], &[1.0], 1, &mut none).unwrap_err();
        assert!(e.to_string().contains("x moves 111 times as much as y"), "{e}");
        let mut w = Watch::new(&info, &model, false);
        w.end(&info, &model, 0.10, &[0.05, -1.0], &[], 0, &mut none)
            .expect("still within a hundred");
        let flag = SolveError::Integrator { t: 0.1, message: "IDA failed (flag -4)".into() };
        let told = w.blamed(&info, &flag).expect("the choice's failure").to_string();
        assert!(told.contains("failing here") && told.contains("flag -4"), "{told}");
    }

    #[test]
    fn the_step_after_an_event_is_watched_from_the_point_after_it() {
        let (model, info) = (no_model(), rod());
        let mut none = |_: f64, _: &mut Vec<f64>| false;
        // the point before an event, the point after it, and a step from
        // there that passes the bottom: compared with the point after it
        let mut w = Watch::new(&info, &model, false);
        w.end(&info, &model, 0.10, &[0.3, -0.95], &[0.0], 0, &mut none).expect("regular");
        w.resume(&info, &model, 0.10, &[0.3, -0.95], &[1.0], 1).expect("regular after it");
        let e = w.end(&info, &model, 0.12, &[-0.3, -0.95], &[1.0], 1, &mut none).unwrap_err();
        assert!(e.to_string().contains("between t = 0.1 and 0.12 s"), "{e}");
        // the step's dense output searched from the point after the event
        let line = |tau: f64| [0.3 - 6.0 * (tau - 0.1), -0.95];
        let mut along = |tau: f64, v: &mut Vec<f64>| {
            v.clear();
            v.extend_from_slice(&line(tau));
            true
        };
        let mut w = Watch::new(&info, &model, false);
        w.end(&info, &model, 0.05, &[0.5, -0.87], &[0.0], 0, &mut along).expect("regular");
        w.resume(&info, &model, 0.1, &line(0.1), &[1.0], 1).expect("regular after it");
        let e = w.end(&info, &model, 0.2, &line(0.2), &[1.0], 1, &mut along).unwrap_err();
        let SolveError::StateSelection { t, .. } = &e else { panic!("{e}") };
        let want = 0.1 + (0.3 - 0.0095) / 6.0;
        assert!((t - want).abs() < 1e-12, "stops at {t}, not {want}: {e}");
        // an event that changes nothing the choice reads (a tick whose step
        // stands): the stretch goes on across it
        let mut w = Watch::new(&info, &model, false);
        w.end(&info, &model, 0.10, &[0.3, -0.95], &[0.0], 0, &mut none).expect("regular");
        w.carry(1);
        let e = w.end(&info, &model, 0.12, &[-0.3, -0.95], &[1.0], 1, &mut none).unwrap_err();
        assert!(e.to_string().contains("between t = 0.1 and 0.12 s"), "{e}");
        // the point after an event where the choice no longer works
        let mut w = Watch::new(&info, &model, false);
        w.end(&info, &model, 0.10, &[0.3, -0.95], &[0.0], 0, &mut none).expect("regular");
        let e = w.resume(&info, &model, 0.10, &[0.009, -1.0], &[1.0], 1).unwrap_err();
        assert!(e.to_string().contains("x moves 111 times as much as y"), "{e}");
    }

    #[test]
    fn the_step_s_dense_output_shows_where_the_choice_fails() {
        // x = 0.3 − 6 (τ − 0.1) along the step from 0.1 to 0.2 (y = −0.95):
        // |x| = |y| / 100 at τ = 0.1 + (0.3 − 0.0095) / 6, before x's zero
        let (model, info) = (no_model(), rod());
        let line = |tau: f64| [0.3 - 6.0 * (tau - 0.1), -0.95];
        let mut along = |tau: f64, v: &mut Vec<f64>| {
            v.clear();
            v.extend_from_slice(&line(tau));
            true
        };
        let mut w = Watch::new(&info, &model, false);
        w.end(&info, &model, 0.1, &line(0.1), &[], 0, &mut along).expect("regular");
        let e = w.end(&info, &model, 0.2, &line(0.2), &[], 0, &mut along).unwrap_err();
        let SolveError::StateSelection { t, message, .. } = &e else { panic!("{e}") };
        let want = 0.1 + (0.3 - 0.0095) / 6.0;
        assert!((t - want).abs() < 1e-12, "stops at {t}, not {want}: {e}");
        assert!(message.contains("x moves 100 times as much as y"), "{e}");
        // the same from a step end that already shows it: the same point
        let mut w = Watch::new(&info, &model, false);
        w.end(&info, &model, 0.1, &line(0.1), &[], 0, &mut along).expect("regular");
        let e = w.end(&info, &model, 0.15, &line(0.15), &[], 0, &mut along).unwrap_err();
        let SolveError::StateSelection { t: t2, .. } = &e else { panic!("{e}") };
        assert!((t2 - want).abs() < 1e-12, "{e}");
    }

    #[test]
    fn a_set_whose_entries_move_only_at_events_is_judged_by_its_singularity() {
        // a gear w_in = r w_out with r a discrete value (d[0]): [1, −r],
        // deciding w_out; a ratio of 1/500 is a scale, a ratio of zero is
        // singular
        let model = NoModel(Layout { n_d: 1, ..no_model().0 });
        let mut info = RunInfo::bare(0, 2, vec![]);
        info.var_sources = vec![crate::info::VarSource::D(0), crate::info::VarSource::Computed];
        let r = Expr::Var(lsim_ir::VarId(0));
        info.state_sets = vec![StateSetInfo {
            entries: vec![vec![Expr::Const(1.0), -r]],
            candidates: vec!["w_in".into(), "w_out".into()],
            ..set(vec![1], 2)
        }];
        let mut none = |_: f64, _: &mut Vec<f64>| false;
        let mut w = Watch::new(&info, &model, false);
        w.end(&info, &model, 0.1, &[2.0, 0.0], &[2.0], 0, &mut none).expect("regular");
        w.end(&info, &model, 0.2, &[0.002, 0.0], &[0.002], 1, &mut none).expect("a scale");
        let e = w.end(&info, &model, 0.3, &[0.0, 0.0], &[0.0], 2, &mut none).unwrap_err();
        assert!(e.to_string().contains("after the event here"), "{e}");
    }
}
