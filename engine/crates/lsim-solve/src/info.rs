//! What the run loop needs to know about a model besides its compiled
//! functions: names, scales, events, modes, sampled blocks, the Jacobian's
//! structure and the energy books. [`RunInfo::from_prepared`] gathers it
//! from a prepared model; tests and other front ends may fill it by hand.

use lsim_ir::eval::Env;
use lsim_ir::prepared::{AliasTarget, Direction, PreparedModel};
use lsim_ir::runtime::SparsityPattern;
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
            var_sources: sources,
            energy: Some(Arc::new(energy_info(m))),
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
        }
    }

    /// The same run with other parameter values (a sweep's set). Bound
    /// parameters are not re-evaluated here: the caller passes a complete,
    /// consistent vector.
    pub fn with_params(&self, params: &[f64]) -> RunInfo {
        RunInfo { params: params.to_vec(), ..self.clone() }
    }
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

/// Evaluates flat-scope expressions from the channel values.
pub(crate) struct ChannelEnv<'a> {
    pub t: f64,
    pub vars: &'a [f64],
    pub params: &'a [f64],
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
}
