//! What the run loop needs to know about a model besides its compiled
//! functions: names, scales, events, modes, sampled blocks, the Jacobian's
//! structure and the energy books. [`RunInfo::from_prepared`] gathers it
//! from a prepared model; tests and other front ends may fill it by hand.

use lsim_ir::eval::Env;
use lsim_ir::prepared::{AliasTarget, Direction, PreparedModel};
use lsim_ir::runtime::{ModelFunctions, SparsityPattern};
use lsim_ir::{Expr, ParamId, Slot, VarId};
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

/// A mode: a discrete Boolean held between events, set from the sign of
/// one zero-crossing function (`d[discrete] = 1` while `roots[crossing] >
/// 0`, else 0). The equations read the held value, so the integrator never
/// sees the discontinuity; at each crossing the run loop flips it and
/// re-evaluates every mode and `when` condition until nothing changes
/// (event iteration). The function may itself depend on the mode (a
/// stick/slip friction whose crossing is the speed while slipping and the
/// torque margin while stuck).
#[derive(Clone, Debug, PartialEq)]
pub struct ModeInfo {
    /// index into the zero-crossing functions
    pub crossing: usize,
    /// index into the discrete variables
    pub discrete: usize,
    /// what it is, naming the part (`'Clutch': locked`)
    pub label: String,
}

/// A zero-crossing function that depends on time only, between events:
/// `rate · (t − at)` with a constant rate and `at` an expression of
/// parameters and discrete variables (a `when time >= t_shift`, a mode of
/// `if time > t_on`). The run loop schedules it as a time event at `at`,
/// reached exactly as a stop time, instead of locating it by root finding
/// (which lands a few ulps after it, so an output at exactly `at` would
/// show the value before the event).
#[derive(Clone, Debug, PartialEq)]
pub struct TimeCrossing {
    /// when it crosses zero (flat scope: parameters, discrete variables)
    pub at: Expr,
    /// it rises through zero as time passes (its rate is positive)
    pub rising: bool,
}

/// A zero-crossing function that reads time beyond `c · time + b`
/// ([`RunInfo::time_functions`]): `sin(2π time / T) > 0.95`, a pulse
/// narrower than the steps the integrator takes while nothing it
/// integrates moves. Root finding sees a condition only by its sign at
/// step ends and would step over it.
#[derive(Clone, Debug, PartialEq)]
pub enum TimeFunction {
    /// It reads time, parameters and discrete values only (the computed
    /// variables that read time replaced by their definitions): the run
    /// loop finds its sign changes ahead without integrating and reaches
    /// each exactly, as a time crossing.
    Pure(Expr),
    /// It reads continuous variables too: these are its terms in time
    /// alone (with parameters and discrete values). The run loop stops at
    /// their extrema, so that between two stops each is monotone and a
    /// pulse they make is not inside one step.
    Mixed(Vec<Expr>),
    /// It reads time in a way the run loop cannot search ahead (the run
    /// warns, and leaves it to root finding).
    Unhandled,
}

/// What the impulse projection at a rigid engagement needs to know about a
/// model (DESIGN.md, *Events*): the rigid engagements, the couplings that
/// pass an impulse on, the parts' stored energies, how every variable they
/// read follows from the states (the model's assignments, which the
/// projection interprets with exact derivatives) and the states `reinit`
/// restarts.
///
/// When an engagement's `changes` takes a new value at an event (a
/// gearbox's selected ratio), the speeds its rigid coupling ties together
/// jump as an instantaneous, rigid engagement makes them, in two stages.
/// First the rigidly coupled inertias meet: their momentum is kept, and
/// the kinetic energy that loses is the engaging part's. Then each
/// coupling a model declares stiff and unbounded ([`ImpulseLink`]) whose
/// `active` holds at the state the first stage left relaxes to the
/// relative velocity it had before the event, passing momentum on: the
/// kinetic energy that loses is that coupling's (the event's, when several
/// share it). A coupling with bounded forces (a tyre, within or at its
/// grip; a slipping clutch) passes nothing in zero time: the integrator
/// follows it after the event. Nothing else starts a projection (a stored energy that
/// merely depends on a discrete value does not), and what a `reinit` set at
/// the event stays where it put it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImpulseInfo {
    /// per energy part with a stored energy: (its index in
    /// [`EnergyInfo::parts`], the flat variables its stored energy reads)
    pub parts: Vec<(usize, Vec<usize>)>,
    /// the rigid engagements
    pub engagements: Vec<EngagementInfo>,
    /// couplings that keep a relative velocity through an impulse
    pub links: Vec<ImpulseLink>,
    /// how each computed variable the stored energies, the links and the
    /// engagements read follows from y, d and u: (flat variable, whether it
    /// is the variable's derivative, its expression), in evaluation order
    pub chain: Vec<(usize, bool, Expr)>,
    /// per variable a `reinit` restarts: (its continuous part's entry of y,
    /// its jump's entry of d)
    pub restarts: Vec<(usize, usize)>,
    /// what each flat variable depends on (derived)
    deps: HashMap<usize, Deps>,
    /// per link: the flat variables `keep` reads (derived)
    link_vars: Vec<Vec<usize>>,
    /// every flat variable the stored energies and links read (derived)
    vars: Vec<usize>,
    /// per state: the variables of `vars` that depend on it (derived)
    by_state: HashMap<usize, Vec<usize>>,
    /// the chain step that computes each variable, and what each step
    /// reads (derived)
    step_of: HashMap<(usize, bool), usize>,
    step_reads: Vec<Vec<(usize, bool)>>,
    /// the discrete values the engagements depend on, and whether one
    /// depends on an iteration variable (derived)
    eng_discretes: BTreeSet<usize>,
    eng_reads_z: bool,
}

/// What a flat variable depends on, through the assignments.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Deps {
    /// states (entries of y)
    pub states: BTreeSet<usize>,
    /// discrete values (entries of d)
    pub discretes: BTreeSet<usize>,
    /// an iteration variable
    pub z: bool,
}

/// A rigid engagement a part makes ([`lsim_ir::EngagementDecl`], in flat
/// scope).
#[derive(Clone, Debug, PartialEq)]
pub struct EngagementInfo {
    /// what takes a new value at an engagement (a gear's selected ratio)
    pub changes: Expr,
    /// the energy part (index in [`EnergyInfo::parts`]) of the instance
    /// that declares it: the kinetic energy its engagement loses is booked
    /// to it
    pub part: Option<usize>,
}

/// A coupling a model declares stiff and unbounded (in the rigid limit):
/// at an engagement it passes the impulse on, its relative velocity `keep`
/// relaxing back to its value before the event, while `active` holds at
/// the state the rigid engagement leaves (or comes to hold as the other
/// links relax). A coupling with bounded forces (a tyre, a slipping
/// clutch) declares none. In flat scope.
#[derive(Clone, Debug, PartialEq)]
pub struct ImpulseLink {
    /// the relative velocity kept
    pub keep: Expr,
    /// while this holds (a truth value)
    pub active: Expr,
    /// the energy part (index in [`EnergyInfo::parts`]) of the instance
    /// that declares it: the kinetic energy its relaxation loses is booked
    /// to it
    pub part: Option<usize>,
}

impl ImpulseInfo {
    /// The projection's structure from its parts: the stored energies (and
    /// the variables they read), the engagements, the links, how the
    /// computed variables follow from y, d and u (`chain`, in evaluation
    /// order), the restarted states, where each variable comes from
    /// (`sources`) and the number of states.
    pub fn new(
        parts: Vec<(usize, Vec<usize>)>,
        engagements: Vec<EngagementInfo>,
        links: Vec<ImpulseLink>,
        chain: Vec<(usize, bool, Expr)>,
        restarts: Vec<(usize, usize)>,
        sources: &[VarSource],
        n_x: usize,
    ) -> ImpulseInfo {
        let leaf = |v: usize| -> Option<Deps> {
            let mut d = Deps::default();
            match sources.get(v).copied()? {
                VarSource::Y(i) | VarSource::NegY(i) => {
                    if i < n_x {
                        d.states.insert(i);
                    } else {
                        d.z = true;
                    }
                }
                VarSource::D(i) | VarSource::NegD(i) => {
                    d.discretes.insert(i);
                }
                VarSource::U(_) | VarSource::Const(_) => {}
                VarSource::Computed => return None,
            }
            Some(d)
        };
        let mut deps: HashMap<(usize, bool), Deps> = HashMap::new();
        let mut step_of = HashMap::new();
        let mut step_reads = vec![];
        for (k, (v, der, e)) in chain.iter().enumerate() {
            let mut d = Deps::default();
            let mut reads = vec![];
            e.walk(&mut |x| {
                let key = match x {
                    Expr::Var(w) => (w.0 as usize, false),
                    Expr::Der(w) => (w.0 as usize, true),
                    _ => return,
                };
                reads.push(key);
                let from = deps.get(&key).cloned().or_else(|| {
                    if key.1 {
                        // a derivative the chain does not compute: an
                        // iteration variable
                        Some(Deps { z: true, ..Default::default() })
                    } else {
                        leaf(key.0)
                    }
                });
                if let Some(f) = from {
                    d.states.extend(f.states);
                    d.discretes.extend(f.discretes);
                    d.z |= f.z;
                }
            });
            deps.insert((*v, *der), d);
            step_of.insert((*v, *der), k);
            step_reads.push(reads);
        }
        let read = |e: &Expr| {
            let mut out = BTreeSet::new();
            e.walk(&mut |x| {
                if let Expr::Var(v) = x {
                    out.insert(v.0 as usize);
                }
            });
            out
        };
        let link_vars: Vec<Vec<usize>> =
            links.iter().map(|l| read(&l.keep).into_iter().collect()).collect();
        let mut all: BTreeSet<usize> = BTreeSet::new();
        for (_, at) in &parts {
            all.extend(at.iter().copied());
        }
        for lv in &link_vars {
            all.extend(lv.iter().copied());
        }
        for en in &engagements {
            all.extend(read(&en.changes));
        }
        let mut var_deps: HashMap<usize, Deps> = HashMap::new();
        for &v in &all {
            let d = deps.get(&(v, false)).cloned().or_else(|| leaf(v)).unwrap_or_default();
            var_deps.insert(v, d);
        }
        for ((v, der), d) in &deps {
            if !der {
                var_deps.entry(*v).or_insert_with(|| d.clone());
            }
        }
        let vars: Vec<usize> = {
            let mut s: BTreeSet<usize> = BTreeSet::new();
            for (_, at) in &parts {
                s.extend(at.iter().copied());
            }
            for lv in &link_vars {
                s.extend(lv.iter().copied());
            }
            s.into_iter().collect()
        };
        let mut by_state: HashMap<usize, Vec<usize>> = HashMap::new();
        for &u in &vars {
            for &s in &var_deps[&u].states {
                by_state.entry(s).or_default().push(u);
            }
        }
        let (mut eng_discretes, mut eng_reads_z) = (BTreeSet::new(), false);
        for en in &engagements {
            for v in read(&en.changes) {
                if let Some(d) = var_deps.get(&v) {
                    eng_discretes.extend(d.discretes.iter().copied());
                    eng_reads_z |= d.z;
                }
            }
        }
        ImpulseInfo {
            parts,
            engagements,
            links,
            chain,
            restarts,
            deps: var_deps,
            link_vars,
            vars,
            by_state,
            step_of,
            step_reads,
            eng_discretes,
            eng_reads_z,
        }
    }

    /// Whether an engagement can have changed between the discrete values
    /// `before` and `d` (one it depends on changed).
    pub(crate) fn may_engage(&self, d: &[f64], before: &[f64]) -> bool {
        !self.engagements.is_empty()
            && (self.eng_reads_z || self.eng_discretes.iter().any(|&k| d[k] != before[k]))
    }

    /// What flat variable `v` depends on.
    pub(crate) fn deps(&self, v: usize) -> Deps {
        self.deps.get(&v).cloned().unwrap_or_default()
    }

    /// The discrete values an expression depends on.
    pub(crate) fn expr_discretes(&self, e: &Expr) -> BTreeSet<usize> {
        let mut out = BTreeSet::new();
        e.walk(&mut |x| {
            if let Expr::Var(v) = x
                && let Some(d) = self.deps.get(&(v.0 as usize))
            {
                out.extend(d.discretes.iter().copied());
            }
        });
        out
    }

    /// Every flat variable the stored energies and the links read.
    pub(crate) fn vars(&self) -> &[usize] {
        &self.vars
    }

    /// The flat variables link `l`'s `keep` reads.
    pub(crate) fn link_vars(&self, l: usize) -> &[usize] {
        &self.link_vars[l]
    }

    /// Whether a variable of the energies or links depends on an
    /// iteration variable.
    pub(crate) fn reads_z(&self) -> bool {
        self.vars.iter().any(|v| self.deps.get(v).is_some_and(|d| d.z))
    }

    /// What `start` is rigidly coupled to: through the states the
    /// variables depend on (and every variable that depends on those
    /// states), and through the links in `links` (all their variables
    /// move together). The variables and the states.
    pub(crate) fn coupled(
        &self,
        start: &[usize],
        links: &[usize],
    ) -> (BTreeSet<usize>, BTreeSet<usize>) {
        let mut vars: BTreeSet<usize> = BTreeSet::new();
        let mut states: BTreeSet<usize> = BTreeSet::new();
        let mut todo: Vec<usize> = start.to_vec();
        while let Some(u) = todo.pop() {
            if !vars.insert(u) {
                continue;
            }
            if let Some(d) = self.deps.get(&u) {
                for &s in &d.states {
                    if states.insert(s) {
                        todo.extend(self.by_state.get(&s).into_iter().flatten().copied());
                    }
                }
            }
            for &lk in links {
                if self.link_vars[lk].contains(&u) {
                    todo.extend(self.link_vars[lk].iter().copied());
                }
            }
        }
        (vars, states)
    }

    /// The energy parts whose stored energy reads one of `vars`.
    pub(crate) fn parts_touching(&self, vars: &BTreeSet<usize>) -> Vec<usize> {
        (0..self.parts.len())
            .filter(|&k| self.parts[k].1.iter().any(|v| vars.contains(v)))
            .collect()
    }

    /// The chain's steps that compute `vars` (with what they read), in
    /// evaluation order.
    pub(crate) fn steps_for(&self, vars: &BTreeSet<usize>) -> Vec<usize> {
        let mut need: BTreeSet<usize> = BTreeSet::new();
        let mut todo: Vec<(usize, bool)> = vars.iter().map(|v| (*v, false)).collect();
        while let Some(key) = todo.pop() {
            if let Some(&k) = self.step_of.get(&key)
                && need.insert(k)
            {
                todo.extend(self.step_reads[k].iter().copied());
            }
        }
        need.into_iter().collect()
    }
}

/// Where a sampled block (a [`lsim_ir::DiscreteBlock`]) reads and writes.
#[derive(Clone, Debug, PartialEq)]
pub struct BlockInfo {
    /// the block's name in messages
    pub name: String,
    /// the flat variables it reads at each tick (indices into the channels)
    pub inputs: Vec<usize>,
    /// the discrete variables it sets (indices into `d`)
    pub outputs: Vec<usize>,
    /// its period, s, as prepared (the host's [`lsim_ir::DiscreteBlock::period`]
    /// is the one used)
    pub period: f64,
    /// for each input the model computes: the assignments it depends on,
    /// so a tick evaluates that input alone instead of every channel
    /// (`None`: read it from the channels)
    pub chains: Vec<Option<InputChain>>,
}

/// A computed channel evaluated on its own: the assignments it depends on,
/// in evaluation order, interpreted.
#[derive(Clone, Debug, PartialEq)]
pub struct InputChain {
    /// the flat variable that is the input (an alias resolved) and its sign
    pub result: (usize, f64),
    /// (target flat variable, whether it is the variable's derivative, its
    /// expression) in evaluation order
    pub steps: Vec<(usize, bool, Expr)>,
    /// flat variables read from y: (variable, derivative?, entry of y)
    pub from_y: Vec<(usize, bool, usize)>,
    /// flat variables read from the discrete values: (variable, entry of d)
    pub from_d: Vec<(usize, usize)>,
    /// flat variables read from the inputs: (variable, entry of u)
    pub from_u: Vec<(usize, usize)>,
}

/// How a channel's value can be had without evaluating the whole model:
/// what a sampled block's tick reads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VarSource {
    /// `y[i]` (a state or an iteration variable)
    Y(usize),
    /// `-y[i]`
    NegY(usize),
    /// `d[i]`
    D(usize),
    /// `-d[i]`
    NegD(usize),
    /// `u[i]`
    U(usize),
    /// a constant
    Const(f64),
    /// computed by the model's assignments (needs [`lsim_ir::ModelFunctions::vars`])
    Computed,
}

/// One part's energy books, in flat scope (every reference is a flat
/// variable, a parameter or time).
#[derive(Clone, Debug, PartialEq)]
pub struct EnergyPart {
    /// the instance's path (`battery.r0`)
    pub path: String,
    /// how a person names it (`'HV Battery'.r0`, or its label)
    pub name: String,
    /// the power into it through all its ports, W
    pub power: Expr,
    /// the power it turns into heat, W
    pub loss: Option<Expr>,
    /// the energy it stores, J
    pub stored: Option<Expr>,
}

/// A condition that must hold while the model runs (a component's
/// `assert`), in flat scope.
#[derive(Clone, Debug, PartialEq)]
pub struct AssertInfo {
    /// what must hold (a truth value: non-zero is true)
    pub condition: Expr,
    /// what to tell the user, naming the part
    pub message: String,
    /// true: stop the run; false: warn once and go on
    pub error: bool,
}

/// The energy books of every primitive part with physical ports.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EnergyInfo {
    /// one entry per primitive part
    pub parts: Vec<EnergyPart>,
}

/// What the run loop needs to know about a model besides its functions.
#[derive(Clone, Debug)]
pub struct RunInfo {
    /// each flat variable's name (the channel names)
    pub var_names: Vec<String>,
    /// nominal magnitude of each entry of y (absolute tolerance scale)
    pub y_nominal: Vec<f64>,
    /// for each `when` clause: its zero crossing and direction
    pub whens: Vec<(usize, Direction)>,
    /// the direction each zero crossing is watched in: +1, -1 or 0 (both)
    pub root_dirs: Vec<i32>,
    /// for each `when` clause: what it is, in words
    pub when_labels: Vec<String>,
    /// for each `when` clause: its condition is strict (`x > 0`: an exact
    /// zero does not hold; empty: none is)
    pub when_strict: Vec<bool>,
    /// parameter values, SI
    pub params: Vec<f64>,
    /// each entry of y by name (states, then iteration variables)
    pub y_names: Vec<String>,
    /// for each residual g: the equation it is, naming the part
    pub residual_labels: Vec<String>,
    /// the modes
    pub modes: Vec<ModeInfo>,
    /// times at which the integrator must stop and restart exactly
    /// (breakpoints of prescribed inputs), increasing
    pub time_events: Vec<f64>,
    /// the sampled blocks, in the order the host passes them to `simulate`
    pub blocks: Vec<BlockInfo>,
    /// the structure of `∂[x'; g]/∂y`; `None`: treat it as dense
    pub pattern: Option<SparsityPattern>,
    /// how each channel can be had cheaply (for sampled blocks' inputs)
    pub var_sources: Vec<VarSource>,
    /// the energy books; `None`: no books are kept
    pub energy: Option<Arc<EnergyInfo>>,
    /// the conditions checked at every accepted step
    pub asserts: Vec<AssertInfo>,
    /// each table's name (index: the table's number in the flat system),
    /// for the table guards' messages
    pub table_names: Vec<String>,
    /// for each residual of the initialisation system: the equation it is
    pub init_labels: Vec<String>,
    /// what the impulse projection at a rigid engagement needs; `None`: no
    /// projection (a model that declares no rigid engagement)
    pub impulse: Option<Arc<ImpulseInfo>>,
    /// for each zero-crossing function that depends on time only between
    /// events: when it crosses (empty, or `None` for the others: located
    /// by root finding)
    pub time_crossings: Vec<Option<TimeCrossing>>,
    /// whether a zero-crossing function, a mode's relation or a `when`'s
    /// assigned value reads an iteration variable (through the
    /// assignments): event iteration then solves the iteration variables
    /// again whenever a discrete value changes, before it re-checks the
    /// conditions
    pub events_read_z: bool,
    /// per discrete value: whether it reaches what the integrator
    /// integrates or watches (a state's derivative, a residual of the
    /// iteration variables, an energy integrand, a zero-crossing function,
    /// a table's argument) through the assignments. A sample tick that
    /// changes only values that do not leaves the solution exactly as it
    /// is: the step stands, no restart. Empty: every discrete value counts
    /// as reaching them
    pub dynamic_discretes: Vec<bool>,
    /// per discrete value: whether it reaches the residuals that determine
    /// the iteration variables (through the assignments): a sampled block
    /// that reads an iteration variable at an instant where another block
    /// changed one of these reads it solved again. Empty: every discrete
    /// value counts as reaching them
    pub z_discretes: Vec<bool>,
    /// how the energy books take each stored energy's rate exactly (the
    /// assignments to differentiate along the solution); `None`: from the
    /// variables' sources alone ([`StoredRates::from_sources`])
    pub stored_rates: Option<Arc<StoredRates>>,
    /// the tables read along time (a driving cycle's speed by time): the
    /// integrator stops at each of their breakpoints
    pub time_tables: Vec<TimeTable>,
    /// for each zero-crossing function that reads time and is not a time
    /// crossing: how the run loop keeps a pulse in time from being stepped
    /// over (empty, or `None` for the others)
    pub time_functions: Vec<Option<TimeFunction>>,
    /// per table: the breakpoints of a 1-D table (empty: 2-D, or not
    /// known), for the enclosures of the time functions that read it
    pub table_breaks: Vec<Vec<f64>>,
}

/// A table read at a position that moves with time alone, `c · time + b`
/// (`b` of parameters and discrete values): its breakpoints are stop
/// times, so no step spans one. A condition such a table drives (a
/// controller's command from a driving cycle's target) is checked at least
/// at every breakpoint even when nothing the integrator integrates moves
/// (a car at rest), and the table's kinks fall on step ends.
#[derive(Clone, Debug, PartialEq)]
pub struct TimeTable {
    /// the breakpoints along that axis
    pub at: Vec<f64>,
    /// `c`, per second
    pub c: f64,
    /// `b`
    pub b: Expr,
}

/// How the energy books take each stored energy's rate, `dE/dt` along the
/// solution, exactly: by forward-mode differentiation in the direction
/// `(1, y')` through the assignments that compute the variables the
/// stored energies read (the time's rate is one; a table's derivatives
/// are those of the model's interpolant, [`ModelFunctions::eval_table`]).
/// A stored energy whose variables reach a derivative or a previous value
/// through them is left to a finite difference.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StoredRates {
    /// (variable, its expression), in evaluation order
    pub chain: Vec<(usize, Expr)>,
    /// per energy part ([`EnergyInfo::parts`]): whether its stored
    /// energy's rate is taken exactly
    pub exact: Vec<bool>,
}

impl StoredRates {
    /// Without the model's assignments: exact for the stored energies that
    /// read only states, discrete values, inputs and constants.
    pub fn from_sources(energy: &EnergyInfo, sources: &[VarSource]) -> StoredRates {
        let exact = energy
            .parts
            .iter()
            .map(|p| {
                p.stored.as_ref().is_some_and(|e| {
                    let mut ok = !reaches_unknown(e);
                    e.walk(&mut |x| {
                        if let Expr::Var(v) = x
                            && sources.get(v.0 as usize).is_none_or(|s| *s == VarSource::Computed)
                        {
                            ok = false;
                        }
                    });
                    ok
                })
            })
            .collect();
        StoredRates { chain: vec![], exact }
    }
}

/// Whether `e` reads what a rate along the solution cannot take exactly
/// here: a derivative, a previous value, a name.
fn reaches_unknown(e: &Expr) -> bool {
    let mut bad = false;
    e.walk(&mut |x| {
        if matches!(x, Expr::Der(_) | Expr::Pre(_) | Expr::Name(_)) {
            bad = true;
        }
    });
    bad
}

/// The assignments the stored energies' rates need (see [`StoredRates`]).
fn stored_rates(m: &PreparedModel, energy: &EnergyInfo, sources: &[VarSource]) -> StoredRates {
    let assigned: HashMap<Slot, usize> =
        m.assignments.iter().enumerate().map(|(k, a)| (a.target, k)).collect();
    let alias: HashMap<VarId, AliasTarget> = m.aliases.iter().map(|a| (a.var, a.target)).collect();
    let mut steps: BTreeSet<usize> = BTreeSet::new();
    let mut alias_steps: Vec<(usize, Expr)> = vec![];
    let mut exact = vec![];
    for p in &energy.parts {
        let Some(e) = &p.stored else {
            exact.push(false);
            continue;
        };
        let mut ok = !reaches_unknown(e);
        let (mut mine, mut my_aliases) = (BTreeSet::new(), vec![]);
        let mut seen: BTreeSet<VarId> = BTreeSet::new();
        let mut todo: Vec<VarId> = vec![];
        e.walk(&mut |x| {
            if let Expr::Var(v) = x {
                todo.push(*v);
            }
        });
        while ok && let Some(v) = todo.pop() {
            if !seen.insert(v) {
                continue;
            }
            if let Some(&k) = assigned.get(&Slot::Var(v)) {
                let a = &m.assignments[k].expr;
                if reaches_unknown(a) {
                    ok = false;
                }
                mine.insert(k);
                a.walk(&mut |x| {
                    if let Expr::Var(r) = x {
                        todo.push(*r);
                    }
                });
            } else if sources.get(v.0 as usize) == Some(&VarSource::Computed) {
                match alias.get(&v) {
                    Some(AliasTarget::Var { var, negated }) => {
                        let t = if *negated { -Expr::Var(*var) } else { Expr::Var(*var) };
                        my_aliases.push((v.0 as usize, t));
                        todo.push(*var);
                    }
                    _ => ok = false,
                }
            }
        }
        if ok {
            steps.extend(mine);
            for a in my_aliases {
                if !alias_steps.iter().any(|(v, _)| *v == a.0) {
                    alias_steps.push(a);
                }
            }
        }
        exact.push(ok);
    }
    let mut chain: Vec<(usize, Expr)> = steps
        .into_iter()
        .map(|k| {
            let a = &m.assignments[k];
            let (Slot::Var(w) | Slot::Der(w)) = a.target;
            (w.0 as usize, a.expr.clone())
        })
        .collect();
    // aliases after what they copy (an alias of an alias after its target)
    alias_steps.reverse();
    chain.extend(alias_steps);
    StoredRates { chain, exact }
}

impl RunInfo {
    /// The minimal run information for a model of `n_y` unknowns whose
    /// functions are written by hand (tests, test doubles): no names beyond
    /// numbers, unit nominal values, no events, no books.
    pub fn bare(n_y: usize, n_vars: usize, params: Vec<f64>) -> RunInfo {
        RunInfo {
            var_names: (0..n_vars).map(|i| format!("v{i}")).collect(),
            y_nominal: vec![1.0; n_y],
            whens: vec![],
            root_dirs: vec![],
            when_labels: vec![],
            when_strict: vec![],
            params,
            y_names: (0..n_y).map(|i| format!("y{i}")).collect(),
            residual_labels: vec![],
            modes: vec![],
            time_events: vec![],
            blocks: vec![],
            pattern: None,
            var_sources: vec![VarSource::Computed; n_vars],
            energy: None,
            asserts: vec![],
            table_names: vec![],
            init_labels: vec![],
            impulse: None,
            time_crossings: vec![],
            events_read_z: true,
            dynamic_discretes: vec![],
            z_discretes: vec![],
            stored_rates: None,
            time_tables: vec![],
            time_functions: vec![],
            table_breaks: vec![],
        }
    }

    /// Gathers the run information from a prepared model.
    pub fn from_prepared(m: &PreparedModel) -> RunInfo {
        let flat = &m.flat;
        let nominal = |v: VarId| flat.var(v).nominal.abs().max(1e-30);
        let mut y_nominal: Vec<f64> = m.states.iter().map(|v| nominal(*v)).collect();
        let mut y_names: Vec<String> = m.states.iter().map(|v| flat.var(*v).name.clone()).collect();
        for s in &m.algebraics {
            let (Slot::Var(v) | Slot::Der(v)) = *s;
            y_nominal.push(nominal(v));
            y_names.push(match s {
                Slot::Var(_) => flat.var(v).name.clone(),
                Slot::Der(_) => format!("der({})", flat.var(v).name),
            });
        }
        let mut root_dirs = vec![0; m.zero_crossings.len()];
        for w in &m.whens {
            root_dirs[w.crossing] = match w.direction {
                Direction::Rising => 1,
                Direction::Falling => -1,
                Direction::Both => 0,
            };
        }
        let labelled = |o: &lsim_ir::Origin| {
            let who = flat.instance_name(o.instance);
            match &o.label {
                Some(l) => format!("{who}: {l}"),
                None => who,
            }
        };
        let d_index: HashMap<VarId, usize> =
            m.discretes.iter().enumerate().map(|(k, v)| (*v, k)).collect();
        let sources = var_sources(m);
        let energy = energy_info(m);
        let dynamic_discretes = dynamic_discretes(m, &energy);
        let z_discretes = z_discretes(m);
        let impulse = impulse_info(m, &energy, &sources).map(Arc::new);
        let stored_rates = Some(Arc::new(stored_rates(m, &energy, &sources)));
        let time_crossings: Vec<Option<TimeCrossing>> = {
            let discrete: std::collections::HashSet<VarId> = m.discretes.iter().copied().collect();
            m.zero_crossings.iter().map(|z| time_crossing(&z.expr, &discrete)).collect()
        };
        let blocks = m
            .external
            .iter()
            .map(|b| BlockInfo {
                name: flat.instance_name(b.instance),
                inputs: b.inputs.iter().map(|v| v.0 as usize).collect(),
                outputs: b.outputs.iter().filter_map(|v| d_index.get(v).copied()).collect(),
                period: b.period,
                chains: b
                    .inputs
                    .iter()
                    .map(|v| match sources[v.0 as usize] {
                        VarSource::Computed => input_chain(m, *v),
                        _ => None,
                    })
                    .collect(),
            })
            .collect();
        RunInfo {
            var_names: flat.vars.iter().map(|v| v.name.clone()).collect(),
            y_nominal,
            whens: m.whens.iter().map(|w| (w.crossing, w.direction)).collect(),
            root_dirs,
            when_labels: m.whens.iter().map(|w| labelled(&w.origin)).collect(),
            when_strict: m.whens.iter().map(|w| w.strict).collect(),
            params: flat.params.iter().map(|p| p.value).collect(),
            y_names,
            residual_labels: m.residuals.iter().map(|r| labelled(&r.origin)).collect(),
            modes: m
                .modes
                .iter()
                .filter_map(|md| {
                    Some(ModeInfo {
                        crossing: md.crossing,
                        discrete: *d_index.get(&md.var)?,
                        label: labelled(&md.origin),
                    })
                })
                .collect(),
            time_events: vec![],
            blocks,
            pattern: Some(
                if m.jac_pattern.n > 0 && m.jac_pattern.col_ptr.len() == m.jac_pattern.n + 1 {
                    m.jac_pattern.clone()
                } else {
                    structural_pattern(m)
                },
            ),
            energy: Some(Arc::new(energy)),
            asserts: flat
                .asserts
                .iter()
                .map(|a| AssertInfo {
                    condition: a.condition.clone(),
                    message: format!("{}: {}", labelled(&a.origin), a.message),
                    error: a.error,
                })
                .collect(),
            table_names: flat.tables.iter().map(|t| t.name.clone()).collect(),
            init_labels: m.init.residuals.iter().map(|r| labelled(&r.origin)).collect(),
            impulse,
            time_functions: time_functions(m, &sources, &time_crossings),
            table_breaks: flat
                .tables
                .iter()
                .map(|t| if t.data.dims() == 1 { t.data.x.clone() } else { vec![] })
                .collect(),
            var_sources: sources,
            time_crossings,
            events_read_z: events_read_z(m),
            dynamic_discretes,
            z_discretes,
            stored_rates,
            time_tables: time_tables(m),
        }
    }

    /// The same run with other parameter values (a sweep's set). Bound
    /// parameters are not re-evaluated here: the caller passes a complete,
    /// consistent vector.
    pub fn with_params(&self, params: &[f64]) -> RunInfo {
        RunInfo { params: params.to_vec(), ..self.clone() }
    }
}

/// The impulse projection's structure ([`ImpulseInfo`]); `None` when the
/// model declares no rigid engagement.
fn impulse_info(
    m: &PreparedModel,
    energy: &EnergyInfo,
    sources: &[VarSource],
) -> Option<ImpulseInfo> {
    if m.flat.engagements.is_empty() {
        return None;
    }
    let part_of = |i: lsim_ir::InstanceId| {
        let path = &m.flat.instance(i).path;
        energy.parts.iter().position(|p| &p.path == path)
    };
    let read = |e: &Expr, out: &mut BTreeSet<usize>| {
        e.walk(&mut |x| {
            if let Expr::Var(v) = x {
                out.insert(v.0 as usize);
            }
        })
    };
    let mut parts = vec![];
    let mut wanted: BTreeSet<usize> = BTreeSet::new();
    for (k, p) in energy.parts.iter().enumerate() {
        let Some(e) = &p.stored else { continue };
        let mut at = BTreeSet::new();
        read(e, &mut at);
        wanted.extend(at.iter().copied());
        parts.push((k, at.into_iter().collect()));
    }
    let links: Vec<ImpulseLink> = m
        .flat
        .impulse
        .iter()
        .map(|l| {
            read(&l.keep, &mut wanted);
            ImpulseLink {
                keep: l.keep.clone(),
                active: l.active.clone(),
                part: part_of(l.origin.instance),
            }
        })
        .collect();
    let engagements: Vec<EngagementInfo> = m
        .flat
        .engagements
        .iter()
        .map(|en| {
            read(&en.changes, &mut wanted);
            EngagementInfo { changes: en.changes.clone(), part: part_of(en.origin.instance) }
        })
        .collect();
    // the assignments these variables need, in evaluation order; aliases
    // of computed variables after them
    let assigned: HashMap<Slot, usize> =
        m.assignments.iter().enumerate().map(|(k, a)| (a.target, k)).collect();
    let alias: HashMap<VarId, AliasTarget> = m.aliases.iter().map(|a| (a.var, a.target)).collect();
    let mut need: BTreeSet<usize> = BTreeSet::new();
    let mut alias_steps: Vec<(usize, bool, Expr)> = vec![];
    let mut seen: BTreeSet<Slot> = BTreeSet::new();
    let mut todo: Vec<Slot> = wanted.iter().map(|v| Slot::Var(VarId(*v as u32))).collect();
    while let Some(slot) = todo.pop() {
        if !seen.insert(slot) {
            continue;
        }
        if let Some(&k) = assigned.get(&slot) {
            need.insert(k);
            m.assignments[k].expr.walk(&mut |x| match x {
                Expr::Var(r) => todo.push(Slot::Var(*r)),
                Expr::Der(r) => todo.push(Slot::Der(*r)),
                _ => {}
            });
        } else if let Slot::Var(v) = slot
            && sources.get(v.0 as usize) == Some(&VarSource::Computed)
            && let Some(AliasTarget::Var { var, negated }) = alias.get(&v)
        {
            let e = if *negated { -Expr::Var(*var) } else { Expr::Var(*var) };
            alias_steps.push((v.0 as usize, false, e));
            todo.push(Slot::Var(*var));
        }
    }
    let mut chain: Vec<(usize, bool, Expr)> = need
        .into_iter()
        .map(|k| {
            let a = &m.assignments[k];
            let (Slot::Var(w) | Slot::Der(w)) = a.target;
            (w.0 as usize, matches!(a.target, Slot::Der(_)), a.expr.clone())
        })
        .collect();
    chain.extend(alias_steps.into_iter().rev());
    let y_of: HashMap<VarId, usize> = m.states.iter().enumerate().map(|(i, v)| (*v, i)).collect();
    let d_of: HashMap<VarId, usize> =
        m.discretes.iter().enumerate().map(|(k, v)| (*v, k)).collect();
    let restarts = m
        .flat
        .restarts
        .iter()
        .filter_map(|r| Some((*y_of.get(&r.continuous)?, *d_of.get(&r.jump)?)))
        .collect();
    Some(ImpulseInfo::new(parts, engagements, links, chain, restarts, sources, m.states.len()))
}

/// The tables read at a position affine in time ([`TimeTable`]), through
/// the assignments that compute it.
fn time_tables(m: &PreparedModel) -> Vec<TimeTable> {
    let discrete: std::collections::HashSet<VarId> = m.discretes.iter().copied().collect();
    let assigned: HashMap<VarId, &Expr> = m
        .assignments
        .iter()
        .filter_map(|a| match a.target {
            Slot::Var(v) => Some((v, &a.expr)),
            Slot::Der(_) => None,
        })
        .collect();
    // a computed variable that moves with time alone, as `c · time + b`
    // (memoised; None: it reads a state or an iteration variable)
    fn affine_var(
        v: VarId,
        assigned: &HashMap<VarId, &Expr>,
        discrete: &std::collections::HashSet<VarId>,
        memo: &mut HashMap<VarId, Option<Expr>>,
        depth: usize,
    ) -> Option<Expr> {
        if let Some(m) = memo.get(&v) {
            return m.clone();
        }
        let def = *assigned.get(&v)?;
        if depth > 64 {
            return None;
        }
        let r = resolve(def, assigned, discrete, memo, depth + 1).and_then(|e| {
            let (c, b) = time_affine(&e, discrete)?;
            Some(Expr::Const(c) * Expr::Time + b)
        });
        memo.insert(v, r.clone());
        r
    }
    // `e` with its computed variables replaced by their forms in time
    fn resolve(
        e: &Expr,
        assigned: &HashMap<VarId, &Expr>,
        discrete: &std::collections::HashSet<VarId>,
        memo: &mut HashMap<VarId, Option<Expr>>,
        depth: usize,
    ) -> Option<Expr> {
        let mut ok = true;
        let out = e.clone().rewrite(&mut |x| match x {
            Expr::Var(v) if assigned.contains_key(&v) => {
                affine_var(v, assigned, discrete, memo, depth).unwrap_or_else(|| {
                    ok = false;
                    Expr::Var(v)
                })
            }
            other => other,
        });
        ok.then_some(out)
    }
    let mut memo: HashMap<VarId, Option<Expr>> = HashMap::new();
    let mut seen: BTreeSet<(u32, usize, String)> = BTreeSet::new();
    let mut out = vec![];
    let mut visit = |e: &Expr| {
        e.walk(&mut |x| {
            let Expr::Table { table, args } = x else { return };
            for (axis, arg) in args.iter().enumerate().take(2) {
                let Some(pos) = resolve(arg, &assigned, &discrete, &mut memo, 0) else { continue };
                let Some((c, b)) = time_affine(&pos, &discrete) else { continue };
                if c == 0.0 || !c.is_finite() {
                    continue;
                }
                let Some(t) = m.flat.tables.get(*table as usize) else { continue };
                let at = if axis == 0 { t.data.x.clone() } else { t.data.y.clone() };
                if at.is_empty() || !seen.insert((*table, axis, format!("{c} {b}"))) {
                    continue;
                }
                out.push(TimeTable { at, c, b });
            }
        })
    };
    for a in &m.assignments {
        visit(&a.expr);
    }
    for r in &m.residuals {
        visit(&r.expr);
    }
    for z in &m.zero_crossings {
        visit(&z.expr);
    }
    out
}

/// How each zero-crossing function that reads time, and is not a time
/// crossing, is kept from being stepped over ([`TimeFunction`]): the
/// computed variables that read time are replaced by their definitions
/// (through the assignments and aliases); what is left reads time,
/// parameters and constant values (discrete values, and computed values
/// of them), or continuous variables too.
fn time_functions(
    m: &PreparedModel,
    sources: &[VarSource],
    crossings: &[Option<TimeCrossing>],
) -> Vec<Option<TimeFunction>> {
    let assigned: HashMap<VarId, &Expr> = m
        .assignments
        .iter()
        .filter_map(|a| match a.target {
            Slot::Var(v) => Some((v, &a.expr)),
            Slot::Der(_) => None,
        })
        .collect();
    let alias: HashMap<VarId, AliasTarget> = m.aliases.iter().map(|a| (a.var, a.target)).collect();
    let mut r = Resolver { assigned, alias, sources, memo: HashMap::new() };
    // (an expression grown past this many nodes is not searched)
    const MAX_NODES: usize = 20_000;
    m.zero_crossings
        .iter()
        .enumerate()
        .map(|(k, z)| {
            if crossings.get(k).is_some_and(|c| c.is_some()) {
                return None;
            }
            let (time, _) = r.reads(&z.expr, 0);
            if !time {
                return None;
            }
            let Some(g) = r.resolve(&z.expr, 0) else {
                return Some(TimeFunction::Unhandled);
            };
            let mut nodes = 0;
            g.walk(&mut |_| nodes += 1);
            if nodes > MAX_NODES {
                return Some(TimeFunction::Unhandled);
            }
            let (_, cont) = r.reads(&g, 0);
            if !cont {
                return Some(TimeFunction::Pure(g));
            }
            let discrete: std::collections::HashSet<VarId> = m.discretes.iter().copied().collect();
            let mut terms = vec![];
            r.terms(&g, &discrete, &mut terms);
            (!terms.is_empty()).then_some(TimeFunction::Mixed(terms))
        })
        .collect()
}

/// Resolves the computed variables that read time ([`time_functions`]).
struct Resolver<'a> {
    assigned: HashMap<VarId, &'a Expr>,
    alias: HashMap<VarId, AliasTarget>,
    sources: &'a [VarSource],
    /// per variable: (reads time, reads continuous variables)
    memo: HashMap<VarId, (bool, bool)>,
}

impl Resolver<'_> {
    /// Whether variable `v` reads time, and continuous variables (states,
    /// iteration variables, inputs, anything not known).
    fn var(&mut self, v: VarId, depth: usize) -> (bool, bool) {
        if let Some(c) = self.memo.get(&v) {
            return *c;
        }
        let out = match self.sources.get(v.0 as usize) {
            Some(VarSource::D(_) | VarSource::NegD(_) | VarSource::Const(_)) => (false, false),
            Some(VarSource::Computed) if depth < 64 => {
                if let Some(e) = self.assigned.get(&v).copied() {
                    self.reads(e, depth + 1)
                } else {
                    match self.alias.get(&v).copied() {
                        Some(AliasTarget::Var { var, .. }) => self.var(var, depth + 1),
                        Some(AliasTarget::Const(_)) => (false, false),
                        None => (false, true),
                    }
                }
            }
            _ => (false, true),
        };
        self.memo.insert(v, out);
        out
    }

    /// Whether `e` reads time, and continuous variables.
    fn reads(&mut self, e: &Expr, depth: usize) -> (bool, bool) {
        let mut vars = vec![];
        let (mut time, mut cont) = (false, false);
        e.walk(&mut |x| match x {
            Expr::Time => time = true,
            Expr::Der(_) | Expr::Name(_) => cont = true,
            Expr::Var(v) | Expr::Pre(v) => vars.push(*v),
            _ => {}
        });
        for v in vars {
            let (t, c) = self.var(v, depth);
            time |= t;
            cont |= c;
        }
        (time, cont)
    }

    /// `e` with the computed variables that read time replaced by their
    /// definitions (`None`: too deep).
    fn resolve(&mut self, e: &Expr, depth: usize) -> Option<Expr> {
        if depth > 64 {
            return None;
        }
        let mut ok = true;
        let out = e.clone().rewrite(&mut |x| match x {
            Expr::Var(v) if self.var(v, depth).0 => {
                let def = match self.assigned.get(&v).copied() {
                    Some(def) => self.resolve(def, depth + 1),
                    None => match self.alias.get(&v).copied() {
                        Some(AliasTarget::Var { var, negated }) => {
                            let t = self.resolve(&Expr::Var(var), depth + 1);
                            if negated { t.map(|t| -t) } else { t }
                        }
                        _ => None,
                    },
                };
                def.unwrap_or_else(|| {
                    ok = false;
                    Expr::Var(v)
                })
            }
            other => other,
        });
        ok.then_some(out)
    }

    /// The largest parts of `e` that read time and no continuous variable,
    /// leaving out those monotone between the stops the run loop makes
    /// already: affine in time, or a table read at a position affine in
    /// time (monotone between its breakpoints, [`TimeTable`]).
    fn terms(
        &mut self,
        e: &Expr,
        discrete: &std::collections::HashSet<VarId>,
        out: &mut Vec<Expr>,
    ) {
        let (time, cont) = self.reads(e, 0);
        if !time {
            return;
        }
        if !cont {
            if !monotone_between_stops(e, discrete) && !out.contains(e) {
                out.push(e.clone());
            }
            return;
        }
        for ch in e.children() {
            self.terms(ch, discrete, out);
        }
    }
}

/// Whether `e`, a function of time, is monotone between the stops the run
/// loop makes anyway: affine in time, or a table read at a position
/// affine in time (monotone between its breakpoints, [`TimeTable`]),
/// scaled and shifted by values constant in time.
fn monotone_between_stops(e: &Expr, discrete: &std::collections::HashSet<VarId>) -> bool {
    use lsim_ir::expr::BinaryOp::*;
    let affine = |x: &Expr| time_affine(x, discrete).is_some();
    let timeless = |x: &Expr| time_affine(x, discrete).is_some_and(|(c, _)| c == 0.0);
    let m = |x: &Expr| monotone_between_stops(x, discrete);
    match e {
        _ if affine(e) => true,
        Expr::Table { args, .. } => args.iter().all(affine),
        Expr::Neg(a) | Expr::NoEvent(a) => m(a),
        Expr::Binary(Add | Sub | Mul, a, b) => (timeless(a) && m(b)) || (timeless(b) && m(a)),
        Expr::Binary(Div, a, b) => timeless(b) && m(a),
        _ => false,
    }
}

/// The crossing time of `f` when it is `c · time + b` with a constant
/// `c ≠ 0` and `b` free of time and of continuous variables: `at = −b / c`
/// (written so that `time − t1` gives `t1` exactly).
pub fn time_crossing(
    f: &Expr,
    discrete: &std::collections::HashSet<VarId>,
) -> Option<TimeCrossing> {
    let (c, b) = time_affine(f, discrete)?;
    if c == 0.0 || !c.is_finite() {
        return None;
    }
    let at = if c == 1.0 {
        neg(b)
    } else if c == -1.0 {
        b
    } else {
        Expr::Binary(lsim_ir::expr::BinaryOp::Div, Box::new(neg(b)), Box::new(Expr::Const(c)))
    };
    Some(TimeCrossing { at, rising: c > 0.0 })
}

fn neg(e: Expr) -> Expr {
    match e {
        Expr::Neg(a) => *a,
        Expr::Const(v) => Expr::Const(-v),
        e => Expr::Neg(Box::new(e)),
    }
}

/// `e` as `c · time + b` (c a constant, b free of time and of continuous
/// variables), if it is one.
fn time_affine(e: &Expr, discrete: &std::collections::HashSet<VarId>) -> Option<(f64, Expr)> {
    use lsim_ir::expr::BinaryOp::*;
    let timeless = |e: &Expr| {
        !e.any(&mut |x| match x {
            Expr::Time | Expr::Der(_) | Expr::Name(_) => true,
            Expr::Var(v) | Expr::Pre(v) => !discrete.contains(v),
            _ => false,
        })
    };
    if timeless(e) {
        return Some((0.0, e.clone()));
    }
    let add = |a: Expr, b: Expr, minus: bool| -> Expr {
        match (a, b) {
            (a, Expr::Const(0.0)) => a,
            (Expr::Const(0.0), b) => {
                if minus {
                    neg(b)
                } else {
                    b
                }
            }
            (a, b) => Expr::Binary(if minus { Sub } else { Add }, Box::new(a), Box::new(b)),
        }
    };
    match e {
        Expr::Time => Some((1.0, Expr::Const(0.0))),
        Expr::Neg(a) => {
            let (c, b) = time_affine(a, discrete)?;
            Some((-c, neg(b)))
        }
        Expr::Binary(op @ (Add | Sub), a, b) => {
            let (ca, ea) = time_affine(a, discrete)?;
            let (cb, eb) = time_affine(b, discrete)?;
            let minus = *op == Sub;
            Some((if minus { ca - cb } else { ca + cb }, add(ea, eb, minus)))
        }
        Expr::Binary(Mul, a, b) => {
            let (ca, ea) = time_affine(a, discrete)?;
            let (cb, eb) = time_affine(b, discrete)?;
            match (ca == 0.0, cb == 0.0, &ea, &eb) {
                (true, false, Expr::Const(k), _) => {
                    Some((k * cb, Expr::Binary(Mul, Box::new(Expr::Const(*k)), Box::new(eb))))
                }
                (false, true, _, Expr::Const(k)) => {
                    Some((ca * k, Expr::Binary(Mul, Box::new(ea), Box::new(Expr::Const(*k)))))
                }
                _ => None,
            }
        }
        Expr::Binary(Div, a, b) => {
            let (ca, ea) = time_affine(a, discrete)?;
            match time_affine(b, discrete)? {
                (cb, Expr::Const(k)) if cb == 0.0 && k != 0.0 => {
                    Some((ca / k, Expr::Binary(Div, Box::new(ea), Box::new(Expr::Const(k)))))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// Per discrete value: whether it reaches a state's derivative, a residual,
/// an energy integrand, a zero-crossing function or a table's argument
/// through the assignments ([`RunInfo::dynamic_discretes`]).
fn dynamic_discretes(m: &PreparedModel, energy: &EnergyInfo) -> Vec<bool> {
    let mut deps: HashMap<Slot, BTreeSet<usize>> = HashMap::new();
    for (k, v) in m.discretes.iter().enumerate() {
        deps.insert(Slot::Var(*v), [k].into());
    }
    let read = |e: &Expr, deps: &HashMap<Slot, BTreeSet<usize>>| {
        let mut out = BTreeSet::new();
        e.walk(&mut |x| {
            let s = match x {
                Expr::Var(v) | Expr::Pre(v) => Slot::Var(*v),
                Expr::Der(v) => Slot::Der(*v),
                _ => return,
            };
            if let Some(d) = deps.get(&s) {
                out.extend(d.iter().copied());
            }
        });
        out
    };
    for a in &m.assignments {
        let s = read(&a.expr, &deps);
        deps.insert(a.target, s);
    }
    // an alias reads what its target reads
    for a in &m.aliases {
        if let AliasTarget::Var { var, .. } = a.target
            && let Some(s) = deps.get(&Slot::Var(var)).cloned()
        {
            deps.insert(Slot::Var(a.var), s);
        }
    }
    let mut dynamic = vec![false; m.discretes.len()];
    let mut mark = |s: &BTreeSet<usize>| {
        for k in s {
            dynamic[*k] = true;
        }
    };
    for v in &m.states {
        if let Some(s) = deps.get(&Slot::Der(*v)) {
            mark(s);
        }
    }
    for r in &m.residuals {
        mark(&read(&r.expr, &deps));
    }
    for z in &m.zero_crossings {
        mark(&read(&z.expr, &deps));
    }
    // (a table's guard watches its arguments)
    for a in &m.assignments {
        if a.expr.any(&mut |x| matches!(x, Expr::Table { .. })) {
            mark(&read(&a.expr, &deps));
        }
    }
    for p in &energy.parts {
        for e in [Some(&p.power), p.loss.as_ref(), p.stored.as_ref()].into_iter().flatten() {
            mark(&read(e, &deps));
        }
    }
    dynamic
}

/// Per discrete value: whether it reaches a residual of the iteration
/// variables through the assignments ([`RunInfo::z_discretes`]).
fn z_discretes(m: &PreparedModel) -> Vec<bool> {
    let mut deps: HashMap<Slot, BTreeSet<usize>> = HashMap::new();
    for (k, v) in m.discretes.iter().enumerate() {
        deps.insert(Slot::Var(*v), [k].into());
    }
    let read = |e: &Expr, deps: &HashMap<Slot, BTreeSet<usize>>| {
        let mut out = BTreeSet::new();
        e.walk(&mut |x| {
            let s = match x {
                Expr::Var(v) | Expr::Pre(v) => Slot::Var(*v),
                Expr::Der(v) => Slot::Der(*v),
                _ => return,
            };
            if let Some(d) = deps.get(&s) {
                out.extend(d.iter().copied());
            }
        });
        out
    };
    for a in &m.assignments {
        let s = read(&a.expr, &deps);
        deps.insert(a.target, s);
    }
    for a in &m.aliases {
        if let AliasTarget::Var { var, .. } = a.target
            && let Some(s) = deps.get(&Slot::Var(var)).cloned()
        {
            deps.insert(Slot::Var(a.var), s);
        }
    }
    let mut out = vec![false; m.discretes.len()];
    for r in &m.residuals {
        for k in read(&r.expr, &deps) {
            out[k] = true;
        }
    }
    out
}

/// Whether an event's condition or assigned value reads an iteration
/// variable, through the assignments ([`RunInfo::events_read_z`]).
fn events_read_z(m: &PreparedModel) -> bool {
    if m.algebraics.is_empty() {
        return false;
    }
    let mut reads: HashMap<Slot, bool> = m.algebraics.iter().map(|s| (*s, true)).collect();
    let depends = |e: &Expr, reads: &HashMap<Slot, bool>| {
        e.any(&mut |x| match x {
            Expr::Var(v) | Expr::Pre(v) => reads.get(&Slot::Var(*v)).copied().unwrap_or(false),
            Expr::Der(v) => reads.get(&Slot::Der(*v)).copied().unwrap_or(false),
            _ => false,
        })
    };
    for a in &m.assignments {
        let r = depends(&a.expr, &reads);
        reads.insert(a.target, r);
    }
    // an alias reads what its target reads
    for a in &m.aliases {
        if let AliasTarget::Var { var, .. } = a.target
            && reads.get(&Slot::Var(var)).copied().unwrap_or(false)
        {
            reads.insert(Slot::Var(a.var), true);
        }
    }
    let reads_expr = |e: &Expr| depends(e, &reads);
    m.zero_crossings.iter().any(|z| reads_expr(&z.expr))
        || m.modes.iter().any(|md| reads_expr(&md.relation))
        || m.whens.iter().any(|w| w.assign.iter().any(|(_, e)| reads_expr(e)))
}

/// The structure of `∂[x'; g]/∂y` from the prepared model: which entries
/// of y each state derivative and each residual depends on, through the
/// explicit assignments. Structural (an entry is kept even where its value
/// happens to be zero), so it holds at every point and in every mode.
pub fn structural_pattern(m: &PreparedModel) -> SparsityPattern {
    let n_x = m.states.len();
    let n = n_x + m.algebraics.len();
    let mut y_index: HashMap<Slot, usize> = HashMap::new();
    for (i, v) in m.states.iter().enumerate() {
        y_index.insert(Slot::Var(*v), i);
    }
    for (k, s) in m.algebraics.iter().enumerate() {
        y_index.insert(*s, n_x + k);
    }
    let mut deps: HashMap<Slot, BTreeSet<usize>> = HashMap::new();
    fn expr_deps(
        e: &Expr,
        y_index: &HashMap<Slot, usize>,
        deps: &HashMap<Slot, BTreeSet<usize>>,
        out: &mut BTreeSet<usize>,
    ) {
        e.walk(&mut |x| {
            let s = match x {
                Expr::Var(v) | Expr::Pre(v) => Slot::Var(*v),
                Expr::Der(v) => Slot::Der(*v),
                _ => return,
            };
            if let Some(&i) = y_index.get(&s) {
                out.insert(i);
            } else if let Some(d) = deps.get(&s) {
                out.extend(d.iter().copied());
            }
        });
    }
    for a in &m.assignments {
        let mut set = BTreeSet::new();
        expr_deps(&a.expr, &y_index, &deps, &mut set);
        deps.insert(a.target, set);
    }
    // rows: x'_i, then g_k
    let mut rows: Vec<BTreeSet<usize>> = Vec::with_capacity(n);
    for v in &m.states {
        let s = Slot::Der(*v);
        let mut set = BTreeSet::new();
        if let Some(&i) = y_index.get(&s) {
            set.insert(i);
        } else if let Some(d) = deps.get(&s) {
            set.extend(d.iter().copied());
        }
        rows.push(set);
    }
    for r in &m.residuals {
        let mut set = BTreeSet::new();
        expr_deps(&r.expr, &y_index, &deps, &mut set);
        rows.push(set);
    }
    pattern_from_rows(n, &rows)
}

/// A column-compressed pattern from each row's column set.
pub fn pattern_from_rows(n: usize, rows: &[BTreeSet<usize>]) -> SparsityPattern {
    let mut cols: Vec<Vec<usize>> = vec![vec![]; n];
    for (i, r) in rows.iter().enumerate() {
        for &j in r {
            cols[j].push(i);
        }
    }
    let mut col_ptr = vec![0];
    let mut row_idx = vec![];
    for c in cols {
        row_idx.extend(c);
        col_ptr.push(row_idx.len());
    }
    SparsityPattern { n, col_ptr, row_idx }
}

fn var_sources(m: &PreparedModel) -> Vec<VarSource> {
    let flat = &m.flat;
    let n_x = m.states.len();
    let mut direct: HashMap<VarId, VarSource> = HashMap::new();
    for (i, v) in m.states.iter().enumerate() {
        direct.insert(*v, VarSource::Y(i));
    }
    for (k, s) in m.algebraics.iter().enumerate() {
        if let Slot::Var(v) = s {
            direct.insert(*v, VarSource::Y(n_x + k));
        }
    }
    for (k, v) in m.discretes.iter().enumerate() {
        direct.insert(*v, VarSource::D(k));
    }
    for (k, v) in m.inputs.iter().enumerate() {
        direct.insert(*v, VarSource::U(k));
    }
    let alias: HashMap<VarId, AliasTarget> = m.aliases.iter().map(|a| (a.var, a.target)).collect();
    (0..flat.vars.len())
        .map(|i| {
            let mut v = VarId(i as u32);
            let mut neg = false;
            for _ in 0..64 {
                if let Some(s) = direct.get(&v) {
                    return match (*s, neg) {
                        (VarSource::Y(k), true) => VarSource::NegY(k),
                        (VarSource::D(k), true) => VarSource::NegD(k),
                        (VarSource::U(_), true) => VarSource::Computed,
                        (s, _) => s,
                    };
                }
                match alias.get(&v) {
                    Some(AliasTarget::Const(c)) => {
                        return VarSource::Const(if neg { -c } else { *c });
                    }
                    Some(AliasTarget::Var { var, negated }) => {
                        v = *var;
                        neg ^= negated;
                    }
                    None => return VarSource::Computed,
                }
            }
            VarSource::Computed
        })
        .collect()
}

/// The chain of assignments that computes `v` alone (`None` when it reads
/// something the interpreter cannot evaluate, such as a table).
pub fn input_chain(m: &PreparedModel, v: VarId) -> Option<InputChain> {
    let n_x = m.states.len();
    let alias: HashMap<VarId, AliasTarget> = m.aliases.iter().map(|a| (a.var, a.target)).collect();
    let (mut var, mut sign) = (v, 1.0);
    for _ in 0..64 {
        match alias.get(&var) {
            Some(AliasTarget::Var { var: w, negated }) => {
                var = *w;
                if *negated {
                    sign = -sign;
                }
            }
            Some(AliasTarget::Const(_)) => return None,
            None => break,
        }
    }
    let mut y_index: HashMap<Slot, usize> = HashMap::new();
    for (i, s) in m.states.iter().enumerate() {
        y_index.insert(Slot::Var(*s), i);
    }
    for (k, s) in m.algebraics.iter().enumerate() {
        y_index.insert(*s, n_x + k);
    }
    let d_index: HashMap<VarId, usize> =
        m.discretes.iter().enumerate().map(|(k, v)| (*v, k)).collect();
    let u_index: HashMap<VarId, usize> =
        m.inputs.iter().enumerate().map(|(k, v)| (*v, k)).collect();
    let assigned: HashMap<Slot, usize> =
        m.assignments.iter().enumerate().map(|(k, a)| (a.target, k)).collect();
    let mut chain = InputChain {
        result: (var.0 as usize, sign),
        steps: vec![],
        from_y: vec![],
        from_d: vec![],
        from_u: vec![],
    };
    let mut needed: BTreeSet<usize> = BTreeSet::new();
    let mut seen: BTreeSet<Slot> = BTreeSet::new();
    let mut todo = vec![Slot::Var(var)];
    while let Some(slot) = todo.pop() {
        if !seen.insert(slot) {
            continue;
        }
        let (Slot::Var(w) | Slot::Der(w)) = slot;
        if let Some(&i) = y_index.get(&slot) {
            chain.from_y.push((w.0 as usize, matches!(slot, Slot::Der(_)), i));
        } else if let (Slot::Var(_), Some(&k)) = (slot, d_index.get(&w)) {
            chain.from_d.push((w.0 as usize, k));
        } else if let (Slot::Var(_), Some(&k)) = (slot, u_index.get(&w)) {
            chain.from_u.push((w.0 as usize, k));
        } else if let Some(&k) = assigned.get(&slot) {
            needed.insert(k);
            let mut bad = false;
            m.assignments[k].expr.walk(&mut |x| match x {
                Expr::Var(r) | Expr::Pre(r) => todo.push(Slot::Var(*r)),
                Expr::Der(r) => todo.push(Slot::Der(*r)),
                Expr::Table { .. } | Expr::Name(_) => bad = true,
                _ => {}
            });
            if bad {
                return None;
            }
        } else {
            return None;
        }
    }
    for k in needed {
        let a = &m.assignments[k];
        let (Slot::Var(w) | Slot::Der(w)) = a.target;
        chain.steps.push((w.0 as usize, matches!(a.target, Slot::Der(_)), a.expr.clone()));
    }
    Some(chain)
}

impl InputChain {
    /// The input's value at `t`: `yv[k]` holds the entry of y read by
    /// `from_y[k]`; `vals` and `ders` are scratch of one value per flat
    /// variable.
    #[allow(clippy::too_many_arguments)]
    pub fn eval(
        &self,
        t: f64,
        yv: &[f64],
        d: &[f64],
        u: &[f64],
        params: &[f64],
        vals: &mut [f64],
        ders: &mut [f64],
    ) -> f64 {
        for ((var, der, _), y) in self.from_y.iter().zip(yv) {
            if *der {
                ders[*var] = *y;
            } else {
                vals[*var] = *y;
            }
        }
        for (var, i) in &self.from_d {
            vals[*var] = d[*i];
        }
        for (var, i) in &self.from_u {
            vals[*var] = u[*i];
        }
        for (var, der, e) in &self.steps {
            let x = lsim_ir::eval::eval(e, &ChainEnv { t, vals, ders, params });
            if *der {
                ders[*var] = x;
            } else {
                vals[*var] = x;
            }
        }
        self.result.1 * vals[self.result.0]
    }
}

/// Evaluates [`InputChain`]s: values of the flat variables (and their
/// derivatives) a chain reads or computes.
pub(crate) struct ChainEnv<'a> {
    pub t: f64,
    pub vals: &'a [f64],
    pub ders: &'a [f64],
    pub params: &'a [f64],
}

impl Env for ChainEnv<'_> {
    fn time(&self) -> f64 {
        self.t
    }
    fn var(&self, v: VarId) -> f64 {
        self.vals[v.0 as usize]
    }
    fn der(&self, v: VarId) -> f64 {
        self.ders[v.0 as usize]
    }
    fn param(&self, p: ParamId) -> f64 {
        self.params[p.0 as usize]
    }
}

fn energy_info(m: &PreparedModel) -> EnergyInfo {
    let flat = &m.flat;
    let mut order: Vec<lsim_ir::InstanceId> = vec![];
    let mut power: HashMap<lsim_ir::InstanceId, Expr> = HashMap::new();
    for pp in &flat.port_powers {
        match power.remove(&pp.instance) {
            Some(e) => {
                power.insert(pp.instance, e + pp.power.clone());
            }
            None => {
                order.push(pp.instance);
                power.insert(pp.instance, pp.power.clone());
            }
        }
    }
    let decl: HashMap<lsim_ir::InstanceId, &lsim_ir::InstanceEnergy> =
        flat.energy.iter().map(|e| (e.instance, e)).collect();
    EnergyInfo {
        parts: order
            .into_iter()
            .map(|id| {
                let e = decl.get(&id);
                EnergyPart {
                    path: flat.instance(id).path.clone(),
                    name: part_name(flat, id),
                    power: power[&id].clone(),
                    loss: e.and_then(|e| e.loss.clone()),
                    stored: e.and_then(|e| e.stored.clone()),
                }
            })
            .collect(),
    }
}

/// `'Label'` for a diagram part, `'Label' (path.inside)` for a part inside
/// one, else `'path'`.
fn part_name(flat: &lsim_ir::FlatSystem, id: lsim_ir::InstanceId) -> String {
    let inst = flat.instance(id);
    if inst.label.is_some() {
        return flat.instance_name(id);
    }
    let top = flat.top_part(id);
    if top != id && flat.instance(top).label.is_some() {
        let tp = &flat.instance(top).path;
        let rest = inst.path.strip_prefix(tp).unwrap_or(&inst.path).trim_start_matches('.');
        return format!("{} ({rest})", flat.instance_name(top));
    }
    flat.instance_name(id)
}

/// Evaluates flat-scope expressions from the channel values, with the
/// model's tables as its compiled code interpolates them.
pub(crate) struct ChannelEnv<'a> {
    pub t: f64,
    pub vars: &'a [f64],
    pub params: &'a [f64],
    pub model: &'a dyn ModelFunctions,
}

impl Env for ChannelEnv<'_> {
    fn time(&self) -> f64 {
        self.t
    }
    fn var(&self, v: VarId) -> f64 {
        self.vars[v.0 as usize]
    }
    fn der(&self, _: VarId) -> f64 {
        f64::NAN
    }
    fn param(&self, p: ParamId) -> f64 {
        self.params[p.0 as usize]
    }
    fn table(&self, k: u32, args: &[f64]) -> f64 {
        table_at(self.model, k, args).map_or(f64::NAN, |(v, _)| v)
    }
}

/// Table `k` of `model` at `args` (one or two of them): its value and
/// partial derivatives, `None` when the model does not give it.
pub(crate) fn table_at(
    model: &dyn ModelFunctions,
    k: u32,
    args: &[f64],
) -> Option<(f64, [f64; 2])> {
    match args {
        [x] => model.eval_table(k, [*x, 0.0]),
        [x, y] => model.eval_table(k, [*x, *y]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsim_ir::expr::BinaryOp;

    fn bin(op: BinaryOp, a: Expr, b: Expr) -> Expr {
        Expr::Binary(op, Box::new(a), Box::new(b))
    }

    #[test]
    fn a_crossing_affine_in_time_becomes_a_time_event() {
        let discrete: std::collections::HashSet<VarId> = [VarId(1)].into_iter().collect();
        let p = || Expr::Param(ParamId(0));
        let env = lsim_ir::eval::SliceEnv { t: 0.0, vars: &[0.0, 2.5], ders: &[], params: &[4.0] };
        let at = |f: Expr| {
            time_crossing(&f, &discrete).map(|c| (lsim_ir::eval::eval(&c.at, &env), c.rising))
        };
        // time - t1: exactly t1, rising
        assert_eq!(at(bin(BinaryOp::Sub, Expr::Time, p())), Some((4.0, true)));
        // t1 - time: falling
        assert_eq!(at(bin(BinaryOp::Sub, p(), Expr::Time)), Some((4.0, false)));
        // 2 time - t1, and time - (a discrete + t1)
        assert_eq!(
            at(bin(BinaryOp::Sub, bin(BinaryOp::Mul, Expr::Const(2.0), Expr::Time), p())),
            Some((2.0, true))
        );
        assert_eq!(
            at(bin(BinaryOp::Sub, Expr::Time, bin(BinaryOp::Add, Expr::Var(VarId(1)), p()))),
            Some((6.5, true))
        );
        // a continuous variable, or time inside a function: root finding
        assert_eq!(at(bin(BinaryOp::Sub, Expr::Time, Expr::Var(VarId(0)))), None);
        assert_eq!(at(Expr::Call(lsim_ir::Builtin::Sin, vec![Expr::Time])), None);
        // no time at all
        assert_eq!(at(p()), None);
    }
}
