//! The preparation pipeline (DESIGN.md, *Preparation pipeline*): from a
//! component tree to the prepared model, for the forward model and, with
//! an [`InverseSpec`], for fast mode's inverse model.

#![allow(clippy::needless_range_loop)] // parallel arrays indexed in step

use crate::causal::{self, Ctx, ParamInfo, Sorted};
use crate::diagnose::{
    self, Structure, equation_words, info, local_name, parts_of, pretty, warning,
};
use crate::flatten::{self, Extras};
use crate::graph::{Bipartite, NONE, hopcroft_karp};
use crate::index::{self, IndexFault};
use crate::init::{self, InitBuild, RowKind};
use crate::system::{NodeEnv, NodeKind, Sys};
use crate::{alias, external, inverse, key, modes, numeric, sparsity, units_check};
use lsim_ir::component::{ComponentDef, Library};
use lsim_ir::eval::eval;
use lsim_ir::expr::{CmpOp, Expr};
use lsim_ir::flat::{FlatSystem, InstanceId, Origin, OriginKind, VarId};
use lsim_ir::prepared::*;
use lsim_ir::{Diagnostic, VarKind};
use std::collections::{BTreeSet, HashMap};
use std::time::Instant;

/// How to prepare, beyond [`crate::PrepOptions`].
#[derive(Clone, Debug)]
pub struct Settings {
    /// keep every block implicit (iteration variables and residuals)
    pub force_implicit: bool,
    /// solve the start at preparation (for the pivoting checks, the state
    /// choice and the start values the Stage 1 code generator bakes in)
    pub numeric_start: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { force_implicit: false, numeric_start: true }
    }
}

/// One block of a sorted system, as the report tells it.
#[derive(Clone, Debug)]
pub struct BlockSummary {
    /// equations in it
    pub size: usize,
    /// tearing variables tearing chose
    pub torn: usize,
    /// linear in its tearing variables
    pub linear: bool,
    /// iteration variables it keeps (0 when solved explicitly)
    pub iteration: usize,
    /// its unknowns' names
    pub unknowns: Vec<String>,
    /// the diagram parts it involves
    pub parts: Vec<String>,
}

/// What preparation found, for the run report and the tests.
#[derive(Clone, Debug, Default)]
pub struct PrepReport {
    /// equations index reduction differentiated
    pub differentiated: usize,
    /// the dummy derivatives chosen
    pub dummy_derivatives: Vec<String>,
    /// the states of the prepared model
    pub states: Vec<String>,
    /// the model's blocks with more than one equation, or kept implicit
    pub blocks: Vec<BlockSummary>,
    /// the same for the initialisation system
    pub init_blocks: Vec<BlockSummary>,
    /// whether the start was solved at preparation
    pub start_solved: bool,
    /// inverse models: the parts removed (the drivers)
    pub removed: Vec<String>,
    /// seconds per step
    pub seconds: Vec<(&'static str, f64)>,
}

struct Clock(Instant, Vec<(&'static str, f64)>);

impl Clock {
    fn lap(&mut self, what: &'static str) {
        let now = Instant::now();
        self.1.push((what, (now - self.0).as_secs_f64()));
        self.0 = now;
    }
}

/// A `when` clause over nodes.
struct NodeWhen {
    crossing: Expr,
    direction: Direction,
    assign: Vec<(VarId, Expr)>,
    origin: Origin,
}

/// A mode over nodes.
struct NodeMode {
    node: usize,
    var: VarId,
    relation: Expr,
    origin: Origin,
}

/// The top-level parts that have physical ports but none of them
/// connected.
fn isolated_parts(flat: &FlatSystem, lib: &Library) -> Vec<InstanceId> {
    let mut connected = vec![false; flat.instances.len()];
    let mut has_port = vec![false; flat.instances.len()];
    for v in &flat.vars {
        if matches!(v.role, lsim_ir::VarRole::Across { .. }) {
            has_port[v.instance.0 as usize] = true;
        }
    }
    for e in &flat.equations {
        if matches!(
            e.origin.kind,
            OriginKind::ConnectionAcross { .. } | OriginKind::ConnectionThrough { .. }
        ) {
            for side in [&e.lhs, &e.rhs] {
                side.walk(&mut |x| {
                    if let Expr::Var(v) = x {
                        connected[flat.var(*v).instance.0 as usize] = true;
                    }
                });
            }
        }
    }
    let _ = lib;
    (1..flat.instances.len())
        .map(|i| InstanceId(i as u32))
        .filter(|&i| flat.instance(i).parent == Some(InstanceId(0)))
        .filter(|&i| has_port[i.0 as usize] && !connected[i.0 as usize])
        .collect()
}

fn index_diag(flat: &FlatSystem, f: IndexFault) -> Diagnostic {
    match f {
        IndexFault::TooHigh { origin } => {
            let mut d = Diagnostic::error(
                "INDEX-TOO-HIGH",
                format!(
                    "Solving the model would need {} differentiated in time more often than any \
                     physical model needs: its constraints contradict each other.",
                    equation_words(flat, &origin)
                ),
            )
            .with_hint("Look for parts that fix the same motion or voltage in different ways.");
            d.parts = parts_of(flat, std::iter::once(origin.instance));
            d
        }
        IndexFault::Differentiate { origin, why } => {
            let mut d = Diagnostic::error(
                "INDEX-DIFFERENTIATE",
                format!(
                    "Solving the model needs {} differentiated in time, but {why}.",
                    equation_words(flat, &origin)
                ),
            )
            .with_hint(
                "The parts are rigidly tied together through it; a compliance (a spring, a \
                 resistance) between them avoids the differentiation.",
            );
            d.parts = parts_of(flat, std::iter::once(origin.instance));
            d
        }
        IndexFault::Singular { origins, .. } => {
            let parts = parts_of(flat, origins.iter().map(|o| o.instance));
            let names: Vec<String> =
                parts.iter().map(|p| format!("'{}'", diagnose::label_of(flat, p))).collect();
            let mut d = Diagnostic::error(
                "STATE-SELECT-SINGULAR",
                format!(
                    "At the start, the constraints of {} are singular for every fixed choice of \
                     states: the run would have to switch which variables are states, which the \
                     engine does not do.",
                    diagnose::join_names(&names)
                ),
            )
            .with_hint("Start the model from a slightly different position.");
            d.parts = parts;
            d.detail = origins.iter().map(|o| equation_words(flat, o)).collect();
            d
        }
    }
}

/// Prepares `top`, forward or (with `spec`) as fast mode's inverse model.
pub fn run(
    lib: &Library,
    top: &ComponentDef,
    spec: Option<&InverseSpec>,
    settings: &Settings,
) -> Result<(PreparedModel, PrepReport), Vec<Diagnostic>> {
    let mut clock = Clock(Instant::now(), vec![]);
    let (mut flat, mut extras) = flatten::flatten_full(lib, top)?;
    clock.lap("flatten");
    let faults = units_check::check(&flat, lib, top);
    if !faults.is_empty() {
        return Err(faults);
    }
    clock.lap("units");
    let (flat_vars, flat_equations) = (flat.vars.len(), flat.equations.len());
    let isolated = isolated_parts(&flat, lib);
    let mut report = PrepReport::default();
    let mut warnings: Vec<Diagnostic> = vec![];
    let (mut input, prescribed) = match spec {
        Some(spec) => {
            let s = inverse::apply(&mut flat, &mut extras, spec)?;
            report.removed = s.removed;
            (s.input, s.prescribed)
        }
        None => (vec![false; flat.vars.len()], vec![]),
    };
    let aliases = alias::eliminate_with(&mut flat, &input);
    clock.lap("aliases");
    let limits_flat = if spec.is_some() { inverse::pass_limits(&mut flat) } else { vec![] };
    let flat_modes = modes::extract(&mut flat);
    let nv = flat.vars.len();
    input.resize(nv, false);
    extras.start.resize(nv, None);
    let mut eliminated = vec![false; nv];
    for a in &aliases {
        eliminated[a.var.0 as usize] = true;
    }

    // the system over nodes
    let mut sys = Sys::new(&flat, &eliminated, &input);
    for e in &flat.equations {
        let r = crate::symbolic::simplify(e.lhs.clone() - e.rhs.clone());
        let r = sys.from_flat(&r);
        sys.push(r, e.origin.clone(), None);
    }
    for v in &prescribed {
        if let Some(n) = sys.base[v.0 as usize] {
            sys.deriv_node(n);
        }
    }
    let mut diags = vec![];
    let fixed_expr =
        |sys: &Sys, e: &Expr, what: &str, origin: &Origin, diags: &mut Vec<Diagnostic>| {
            sys.from_flat_fixed(e).unwrap_or_else(|v| {
                let mut d = Diagnostic::error(
                    "DER-NOT-STATE",
                    format!(
                        "{} reads der({}) in {what}, but nothing in the model makes {} change \
                     continuously.",
                        flat.instance_name(origin.instance),
                        flat.var(v).name,
                        local_name(&flat, v)
                    ),
                );
                d.parts = parts_of(&flat, std::iter::once(origin.instance));
                diags.push(d);
                Expr::Const(0.0)
            })
        };
    let node_modes: Vec<NodeMode> = flat_modes
        .iter()
        .map(|m| NodeMode {
            node: sys.base[m.var.0 as usize].expect("a mode variable has a node"),
            var: m.var,
            relation: fixed_expr(&sys, &m.relation, "a condition", &m.origin, &mut diags),
            origin: m.origin.clone(),
        })
        .collect();
    let mut node_whens = vec![];
    for w in &flat.whens {
        let (cond, flip) = match &w.condition {
            Expr::Not(inner) => (&**inner, true),
            c => (c, false),
        };
        let Expr::Compare(op, a, b) = cond else {
            let mut d = Diagnostic::error(
                "WHEN-CONDITION",
                format!(
                    "{}: a when-condition must be one comparison (such as w >= w_on); this one \
                     is {}.",
                    flat.instance_name(w.origin.instance),
                    pretty(&flat, &w.condition)
                ),
            )
            .with_hint("Split it into several when-clauses, one per comparison.");
            d.parts = parts_of(&flat, std::iter::once(w.origin.instance));
            diags.push(d);
            continue;
        };
        let rising = matches!(op, CmpOp::Gt | CmpOp::Ge) != flip;
        let f = crate::symbolic::simplify((**a).clone() - (**b).clone());
        for (v, _) in &w.assign {
            if flat.var(*v).kind != VarKind::Discrete {
                let mut d = Diagnostic::error(
                    "WHEN-CONTINUOUS",
                    format!(
                        "{}: an event assigns {}, which is not a discrete variable.",
                        flat.instance_name(w.origin.instance),
                        local_name(&flat, *v)
                    ),
                );
                d.parts = parts_of(&flat, std::iter::once(w.origin.instance));
                diags.push(d);
            }
        }
        if !w.reinit.is_empty() {
            let mut d = Diagnostic::error(
                "NOT-YET",
                format!(
                    "{}: reinit in a when-clause is not supported yet.",
                    flat.instance_name(w.origin.instance)
                ),
            );
            d.parts = parts_of(&flat, std::iter::once(w.origin.instance));
            diags.push(d);
        }
        node_whens.push(NodeWhen {
            crossing: fixed_expr(&sys, &f, "an event condition", &w.origin, &mut diags),
            direction: if rising { Direction::Rising } else { Direction::Falling },
            assign: w
                .assign
                .iter()
                .map(|(v, x)| (*v, fixed_expr(&sys, x, "an event", &w.origin, &mut diags)))
                .collect(),
            origin: w.origin.clone(),
        });
    }
    let node_limits: Vec<(Expr, Expr, Expr, Origin)> = limits_flat
        .iter()
        .map(|l| {
            let f = |e: &Expr, diags: &mut Vec<Diagnostic>| {
                fixed_expr(&sys, e, "a limit", &l.origin, diags)
            };
            (f(&l.value, &mut diags), f(&l.lo, &mut diags), f(&l.hi, &mut diags), l.origin.clone())
        })
        .collect();
    let initial_eqs: Vec<(Expr, Origin)> = flat
        .initial_equations
        .iter()
        .map(|e| {
            let r = crate::symbolic::simplify(e.lhs.clone() - e.rhs.clone());
            (fixed_expr(&sys, &r, "an initial equation", &e.origin, &mut diags), e.origin.clone())
        })
        .collect();
    if !diags.is_empty() {
        return Err(diags);
    }
    clock.lap("system");

    // structurally singular? (derivatives merged with their variables)
    precheck(&sys, &flat, lib, &isolated, &prescribed)?;
    clock.lap("matching");

    // index reduction
    let differentiated = index::pantelides(&mut sys).map_err(|f| vec![index_diag(&flat, f)])?;
    report.differentiated = differentiated;
    clock.lap("index");

    let nn = sys.nodes.len();
    let kinds: Vec<NodeKind> = sys.nodes.iter().map(|n| n.kind).collect();
    let mut is_mode = vec![false; nn];
    for m in &node_modes {
        is_mode[m.node] = true;
    }
    let params: Vec<ParamInfo> = flat
        .params
        .iter()
        .zip(&extras.param_range)
        .map(|(p, r)| ParamInfo { value: p.value, min: r.0, max: r.1 })
        .collect();
    let pvals: Vec<f64> = flat.params.iter().map(|p| p.value).collect();

    // the initialisation system: modes stand for their relations
    let relation_of: HashMap<usize, Expr> =
        node_modes.iter().map(|m| (m.node, m.relation.clone())).collect();
    let subst_modes = |e: &Expr| -> Expr {
        if relation_of.is_empty() {
            return e.clone();
        }
        e.clone().rewrite(&mut |x| match x {
            Expr::Var(v) | Expr::Pre(v) if relation_of.contains_key(&(v.0 as usize)) => {
                relation_of[&(v.0 as usize)].clone()
            }
            other => other,
        })
    };
    let model_res: Vec<Expr> = sys.eqs.iter().map(|e| subst_modes(&e.res)).collect();
    let initial: Vec<(Expr, Origin)> =
        initial_eqs.iter().map(|(e, o)| (subst_modes(e), o.clone())).collect();
    let ib = init::build(&sys, &flat, &extras, &aliases, model_res, initial).map_err(|f| {
        let parts = parts_of(&flat, f.origins.iter().map(|o| o.instance));
        let names: Vec<String> =
            parts.iter().map(|p| format!("'{}'", diagnose::label_of(&flat, p))).collect();
        let mut d = Diagnostic::error(
            "INIT-OVER",
            format!(
                "The initial equations of {} contradict the model's equations: {}.",
                diagnose::join_names(&names),
                f.origins.iter().map(|o| equation_words(&flat, o)).collect::<Vec<_>>().join("; ")
            ),
        )
        .with_hint("Remove initial equations that fix what the model already decides.");
        d.parts = parts;
        vec![d]
    })?;
    let no_modes = vec![false; nn];
    let init_ctx =
        Ctx { params: &params, kinds: &kinds, is_mode: &no_modes, force_implicit: false };
    let init_refs: Vec<&Expr> = ib.eqs.iter().collect();
    let init_sorted = causal::sort(&init_refs, &ib.unknowns, &init_ctx).map_err(|_| {
        vec![Diagnostic::error(
            "INIT-SINGULAR",
            "The start of the run is not determined: the initialisation system has no matching \
             (an engine fault; please report the model).",
        )]
    })?;
    clock.lap("init");

    // the start at preparation
    let mut vals = start_guesses(&sys, &flat);
    let solved = if settings.numeric_start {
        numeric::solve(&init_sorted, &mut vals, &pvals, 0.0).map_err(Some)
    } else {
        Err(None)
    };
    report.start_solved = solved.is_ok();
    if let Err(Some(ns)) = &solved {
        let b = &init_sorted.blocks[ns.block];
        let parts = parts_of(&flat, b.eqs.iter().map(|&k| ib.origins[k].instance));
        let names: Vec<String> =
            parts.iter().map(|p| format!("'{}'", diagnose::label_of(&flat, p))).collect();
        if b.linear && ns.singular {
            let mut d = Diagnostic::error(
                "SINGULAR-LOOP",
                format!(
                    "{} form a loop the equations cannot solve with the values given: {} \
                     cannot be worked out (for example ideal sources wired in a loop whose \
                     resistances are all zero).",
                    diagnose::join_names(&names),
                    b.nodes.iter().map(|&n| sys.name(&flat, n)).collect::<Vec<_>>().join(", ")
                ),
            )
            .with_hint("Check the parameters of these parts for zeros: a resistance, an inertia or a ratio of 0.");
            d.parts = parts;
            d.detail = b.eqs.iter().map(|&k| equation_words(&flat, &ib.origins[k])).collect();
            return Err(vec![d]);
        }
        let mut d = info(
            "START-NOT-SOLVED",
            format!(
                "The start could not be solved at preparation (in {}); the run will solve it \
                 again with its own values.",
                diagnose::join_names(&names)
            ),
        );
        d.parts = parts;
        d.detail.push(format!(
            "{} = {:e}",
            equation_words(&flat, &ib.origins[ns.worst_eq]),
            ns.worst
        ));
        warnings.push(d);
    }
    clock.lap("start");

    // dummy derivatives
    let fixed_node: Vec<bool> = sys
        .nodes
        .iter()
        .map(|n| n.order == 0 && flat.var(n.var).fixed && flat.var(n.var).start.is_some())
        .collect();
    let (is_state, dummy) = if differentiated > 0 {
        let point = if solved.is_ok() { vals.clone() } else { generic_values(&sys, &flat) };
        let sel = index::dummy_derivatives(&sys, &point, &pvals, &fixed_node)
            .or_else(|f| {
                if solved.is_ok() {
                    Err(f)
                } else {
                    index::dummy_derivatives(
                        &sys,
                        &generic_values(&sys, &flat),
                        &pvals,
                        &fixed_node,
                    )
                }
            })
            .map_err(|f| vec![index_diag(&flat, f)])?;
        (sel.is_state, sel.dummy)
    } else {
        (
            sys.nodes.iter().map(|n| n.kind == NodeKind::Unknown && n.deriv.is_some()).collect(),
            vec![false; nn],
        )
    };
    report.dummy_derivatives = (0..nn).filter(|&n| dummy[n]).map(|n| sys.name(&flat, n)).collect();
    clock.lap("states");

    // the model: sorted, torn, solved
    let unknowns: Vec<usize> =
        (0..nn).filter(|&n| kinds[n] == NodeKind::Unknown && !is_state[n]).collect();
    let ctx = Ctx {
        params: &params,
        kinds: &kinds,
        is_mode: &is_mode,
        force_implicit: settings.force_implicit,
    };
    let refs: Vec<&Expr> = sys.eqs.iter().map(|e| &e.res).collect();
    let sorted = causal::sort(&refs, &unknowns, &ctx).map_err(|u| {
        let origins: Vec<&Origin> = sys.eqs.iter().map(|e| &e.origin).collect();
        let s = Structure {
            flat: &flat,
            lib,
            graph: &u.graph,
            matching: &u.matching,
            origins,
            texts: sys.eqs.iter().map(|e| e.res.to_string()).collect(),
            vars: unknowns.iter().map(|&n| sys.nodes[n].var).collect(),
            isolated: &isolated,
            prescribed: &prescribed,
            row_has_input: sys
                .eqs
                .iter()
                .map(|e| e.inc.iter().any(|&n| kinds[n] == NodeKind::Input))
                .collect(),
        };
        diagnose::singular(&s)
    })?;
    clock.lap("sort");
    report.blocks = summaries(&sorted, &sys, &flat, |k| &sys.eqs[k].origin);
    report.init_blocks = summaries(&init_sorted, &sys, &flat, |k| &ib.origins[k]);
    warnings.extend(causal_loops(&sorted, &sys, &flat, lib));

    // checks at the start
    if solved.is_ok() {
        let env = NodeEnv { t: 0.0, vals: &vals, params: &pvals };
        for (a, k) in &sorted.variable_pivots {
            let v = eval(a, &env);
            if v == 0.0 || !v.is_finite() {
                let o = &sys.eqs[*k].origin;
                let mut d = warning(
                    "PIVOT-ZERO-AT-START",
                    format!(
                        "At the start, solving {} divides by {}, which is {v} there: the run \
                         would start with an infinite or undefined value.",
                        equation_words(&flat, o),
                        a
                    ),
                )
                .with_hint("Start it from a value where the quantity is not zero.");
                d.parts = parts_of(&flat, std::iter::once(o.instance));
                warnings.push(d);
            }
        }
    }
    start_checks(&flat, &ib, &vals, &pvals, solved.is_ok(), &mut warnings);

    // nodes → slots
    let state_nodes: Vec<usize> = (0..nn).filter(|&n| is_state[n]).collect();
    let map = sys.slots(&mut flat, &is_state);
    extras.start.resize(flat.vars.len(), None);
    report.states = map.states.iter().map(|v| flat.var(*v).name.clone()).collect();
    let origin_of = |k: usize| sys.eqs[k].origin.clone();
    let mut assignments: Vec<Assignment> = map
        .chained
        .iter()
        .map(|&(x, y)| Assignment {
            target: Slot::Der(x),
            expr: Expr::Var(y),
            origin: init::start_origin(&flat, x),
        })
        .collect();
    assignments.extend(sorted.assignments.iter().map(|(n, e, k)| Assignment {
        target: map.slot[*n],
        expr: map.to_flat(e),
        origin: origin_of(*k),
    }));
    let algebraics: Vec<Slot> = sorted.iteration.iter().map(|&n| map.slot[n]).collect();
    let residuals: Vec<Residual> = sorted
        .residuals
        .iter()
        .map(|(e, k)| Residual { expr: map.to_flat(e), origin: origin_of(*k) })
        .collect();
    let discretes: Vec<VarId> = (0..nv)
        .filter(|&i| !eliminated[i] && flat.vars[i].kind == VarKind::Discrete)
        .map(|i| VarId(i as u32))
        .collect();
    let mut inputs = vec![];
    for v in &prescribed {
        let mut n = sys.base[v.0 as usize];
        while let Some(k) = n {
            match map.slot[k] {
                Slot::Var(w) => inputs.push(w),
                Slot::Der(_) => unreachable!("inputs are variables"),
            }
            n = sys.nodes[k].deriv;
        }
    }
    let mut zero_crossings = vec![];
    let mut whens = vec![];
    for w in &node_whens {
        zero_crossings
            .push(ZeroCrossing { expr: map.to_flat(&w.crossing), origin: w.origin.clone() });
        whens.push(PreparedWhen {
            crossing: zero_crossings.len() - 1,
            direction: w.direction,
            assign: w.assign.iter().map(|(v, x)| (*v, map.to_flat(x))).collect(),
            origin: w.origin.clone(),
        });
    }
    let mut prepared_modes = vec![];
    for m in &node_modes {
        let relation = map.to_flat(&m.relation);
        let f = modes::crossing(&relation);
        let up = modes::value_when_rising(&relation);
        let k = zero_crossings.len();
        for (dir, value) in [(Direction::Rising, up), (Direction::Falling, 1.0 - up)] {
            zero_crossings.push(ZeroCrossing { expr: f.clone(), origin: m.origin.clone() });
            whens.push(PreparedWhen {
                crossing: zero_crossings.len() - 1,
                direction: dir,
                assign: vec![(m.var, Expr::Const(value))],
                origin: m.origin.clone(),
            });
        }
        prepared_modes.push(Mode { var: m.var, relation, crossing: k, origin: m.origin.clone() });
    }
    let limits: Vec<LimitSite> = node_limits
        .iter()
        .map(|(v, lo, hi, o)| LimitSite {
            value: map.to_flat(v),
            lo: map.to_flat(lo),
            hi: map.to_flat(hi),
            origin: o.clone(),
        })
        .collect();
    let mut guards: Vec<ParamGuard> = vec![];
    let mut seen_guard = BTreeSet::new();
    for (a, k) in &sorted.guards {
        if seen_guard.insert(a.to_string()) {
            guards.push(ParamGuard { expr: map.to_flat(a), origin: origin_of(*k) });
        }
    }

    // the initialisation system, in slots
    let guess_of = |n: usize| -> Expr {
        let nd = &sys.nodes[n];
        if nd.order == 0 {
            extras.start[nd.var.0 as usize]
                .clone()
                .or_else(|| flat.var(nd.var).start.map(Expr::Const))
                .unwrap_or(Expr::Const(0.0))
        } else {
            Expr::Const(0.0)
        }
    };
    let mode_rel_flat: HashMap<VarId, Expr> =
        prepared_modes.iter().map(|m| (m.var, m.relation.clone())).collect();
    let init_system = InitSystem {
        unknowns: init_sorted.iteration.iter().map(|&n| map.slot[n]).collect(),
        guesses: init_sorted.iteration.iter().map(|&n| guess_of(n)).collect(),
        assignments: init_sorted
            .assignments
            .iter()
            .map(|(n, e, k)| Assignment {
                target: map.slot[*n],
                expr: map.to_flat(e),
                origin: ib.origins[*k].clone(),
            })
            .collect(),
        residuals: init_sorted
            .residuals
            .iter()
            .map(|(e, k)| Residual { expr: map.to_flat(e), origin: ib.origins[*k].clone() })
            .collect(),
        discrete_starts: discretes
            .iter()
            .map(|v| match mode_rel_flat.get(v) {
                Some(r) => r.clone(),
                None => extras.start[v.0 as usize]
                    .clone()
                    .unwrap_or(Expr::Const(flat.var(*v).start.unwrap_or(0.0))),
            })
            .collect(),
    };

    // the start values the Stage 1 code generator bakes in
    if solved.is_ok() {
        for (k, &n) in state_nodes.iter().enumerate() {
            flat.vars[map.states[k].0 as usize].start = Some(vals[n]);
        }
        for &n in &sorted.iteration {
            if let Slot::Var(v) = map.slot[n] {
                flat.vars[v.0 as usize].start = Some(vals[n]);
            }
        }
        let env = NodeEnv { t: 0.0, vals: &vals, params: &pvals };
        for m in &node_modes {
            flat.vars[m.var.0 as usize].start = Some(eval(&m.relation, &env));
        }
    }

    let largest = sorted.blocks.iter().map(|b| b.eqs.len()).max().unwrap_or(0);
    let stats = PrepStats {
        flat_vars,
        flat_equations,
        aliases: aliases.len(),
        blocks: sorted.blocks.len(),
        largest_block: largest,
        explicit: assignments.len(),
    };
    let mut model = PreparedModel {
        flat,
        states: map.states.clone(),
        algebraics,
        discretes,
        inputs,
        external: vec![],
        assignments,
        residuals,
        aliases,
        zero_crossings,
        whens,
        structure_key: String::new(),
        stats,
        jac_pattern: Default::default(),
        modes: prepared_modes,
        init: init_system,
        limits,
        guards,
        warnings: vec![],
    };
    let (blocks, ext_diags) = external::order(&model, extras.external);
    model.external = blocks;
    warnings.extend(ext_diags);
    model.warnings = warnings;
    model.jac_pattern = sparsity::pattern(&model);
    model.structure_key = key::structure_key(&model);
    clock.lap("assemble");
    report.seconds = clock.1;
    Ok((model, report))
}

/// First values of every node: start values, inputs at their starts,
/// derivatives zero.
fn start_guesses(sys: &Sys, flat: &FlatSystem) -> Vec<f64> {
    sys.nodes
        .iter()
        .map(|n| if n.order == 0 { flat.var(n.var).start.unwrap_or(0.0) } else { 0.0 })
        .collect()
}

/// Values where structure and numbers agree almost surely: start values,
/// else a spread of values away from zero.
fn generic_values(sys: &Sys, flat: &FlatSystem) -> Vec<f64> {
    sys.nodes
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let s = if n.order == 0 { flat.var(n.var).start.unwrap_or(0.0) } else { 0.0 };
            if s != 0.0 {
                s
            } else {
                // a deterministic value in [0.5, 1.5)
                let h = (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 11;
                0.5 + (h as f64) / ((1u64 << 53) as f64)
            }
        })
        .collect()
}

/// Structural singularity of the system with each variable's derivatives
/// merged into it.
fn precheck(
    sys: &Sys,
    flat: &FlatSystem,
    lib: &Library,
    isolated: &[InstanceId],
    prescribed: &[VarId],
) -> Result<(), Vec<Diagnostic>> {
    let nn = sys.nodes.len();
    let mut col = vec![NONE; nn];
    let mut vars = vec![];
    for n in 0..nn {
        if sys.nodes[n].order == 0 && sys.nodes[n].kind == NodeKind::Unknown {
            col[n] = vars.len();
            vars.push(sys.nodes[n].var);
        }
    }
    let base_of = |mut n: usize| {
        while let Some(p) = sys.nodes[n].integral {
            n = p;
        }
        n
    };
    let rows: Vec<Vec<usize>> = sys
        .eqs
        .iter()
        .map(|e| {
            let mut r: Vec<usize> = e
                .inc
                .iter()
                .filter(|&&n| sys.nodes[n].kind == NodeKind::Unknown)
                .map(|&n| col[base_of(n)])
                .filter(|&c| c != NONE)
                .collect();
            r.sort_unstable();
            r.dedup();
            r
        })
        .collect();
    let graph = Bipartite::from_rows(vars.len(), &rows);
    let matching = hopcroft_karp(&graph);
    if matching.is_perfect() {
        return Ok(());
    }
    let s = Structure {
        flat,
        lib,
        graph: &graph,
        matching: &matching,
        origins: sys.eqs.iter().map(|e| &e.origin).collect(),
        texts: flat
            .equations
            .iter()
            .map(|e| format!("{} = {}", pretty(flat, &e.lhs), pretty(flat, &e.rhs)))
            .collect(),
        vars,
        isolated,
        prescribed,
        row_has_input: sys
            .eqs
            .iter()
            .map(|e| e.inc.iter().any(|&n| sys.nodes[n].kind == NodeKind::Input))
            .collect(),
    };
    Err(diagnose::singular(&s))
}

fn summaries<'a>(
    sorted: &Sorted,
    sys: &Sys,
    flat: &FlatSystem,
    origin: impl Fn(usize) -> &'a Origin,
) -> Vec<BlockSummary> {
    sorted
        .blocks
        .iter()
        .filter(|b| b.eqs.len() > 1 || b.iter.1 > b.iter.0)
        .map(|b| BlockSummary {
            size: b.eqs.len(),
            torn: b.torn,
            linear: b.linear,
            iteration: b.iter.1 - b.iter.0,
            unknowns: b.nodes.iter().map(|&n| sys.name(flat, n)).collect(),
            parts: parts_of(flat, b.eqs.iter().map(|&k| origin(k).instance)),
        })
        .collect()
}

/// Loops made only of controllers and signal links.
fn causal_loops(sorted: &Sorted, sys: &Sys, flat: &FlatSystem, lib: &Library) -> Vec<Diagnostic> {
    let mut out = vec![];
    for b in &sorted.blocks {
        if b.eqs.len() < 2 {
            continue;
        }
        let causal = b.eqs.iter().all(|&k| {
            let o = &sys.eqs[k].origin;
            matches!(o.kind, OriginKind::SignalLink { .. })
                || (o.instance.0 != 0 && !diagnose::is_physical(flat, lib, o.instance))
        });
        if !causal {
            continue;
        }
        let parts = parts_of(flat, b.eqs.iter().map(|&k| sys.eqs[k].origin.instance));
        let names: Vec<String> =
            parts.iter().map(|p| format!("'{}'", diagnose::label_of(flat, p))).collect();
        let mut d = warning(
            "CAUSAL-LOOP",
            format!(
                "The signals of {} form an algebraic loop: each output feeds straight back into \
                 its own input with no state or delay between them, so the loop is solved anew \
                 at every step{}.",
                diagnose::join_names(&names),
                if b.iter.1 > b.iter.0 { " by iteration" } else { "" }
            ),
        )
        .with_hint(
            "Put a filter, an integrator or a delay into the loop, or check that it is meant.",
        );
        d.parts = parts;
        d.detail = b.nodes.iter().map(|&n| sys.name(flat, n)).collect();
        out.push(d);
    }
    out
}

/// Fixed start values that could not be used, checked against the start.
fn start_checks(
    flat: &FlatSystem,
    ib: &InitBuild,
    vals: &[f64],
    pvals: &[f64],
    solved: bool,
    warnings: &mut Vec<Diagnostic>,
) {
    let env = NodeEnv { t: 0.0, vals, params: pvals };
    for (v, n, start) in &ib.dropped {
        let s = eval(start, &env);
        let part = parts_of(flat, std::iter::once(flat.var(*v).instance));
        let unit = &flat.var(*v).unit_text;
        if !solved {
            continue;
        }
        let actual = vals[*n];
        if (s - actual).abs() > 1e-9 * (1.0 + actual.abs().max(s.abs())) {
            let mut d = warning(
                "INIT-START-IGNORED",
                format!(
                    "The start value {s} {unit} of {} cannot hold: the parts it is tied to \
                     decide it, and it starts at {actual} {unit}.",
                    local_name(flat, *v)
                ),
            )
            .with_hint("Give a start value only to one of the quantities that are tied together.");
            d.parts = part;
            warnings.push(d);
        }
    }
    for (v, s, c) in &ib.constant_conflicts {
        let mut d = warning(
            "INIT-START-IGNORED",
            format!(
                "The start value {s} of {} cannot hold: it is always {c}.",
                local_name(flat, *v)
            ),
        );
        d.parts = parts_of(flat, std::iter::once(flat.var(*v).instance));
        warnings.push(d);
    }
    let assumed: Vec<&VarId> = ib
        .kinds
        .iter()
        .filter_map(|k| if let RowKind::Assumed(v) = k { Some(v) } else { None })
        .collect();
    if !assumed.is_empty() {
        let names: Vec<String> = assumed.iter().map(|v| local_name(flat, **v)).collect();
        let mut d = info(
            "INIT-START-ASSUMED",
            format!(
                "The start is not fully given: {} start{} from {} start value{} (or zero).",
                names.join(", "),
                if names.len() == 1 { "s" } else { "" },
                if names.len() == 1 { "its" } else { "their" },
                if names.len() == 1 { "" } else { "s" }
            ),
        );
        d.parts = parts_of(flat, assumed.iter().map(|v| flat.var(**v).instance));
        warnings.push(d);
    }
}

/// A variable's start as an expression of the parameters (for callers).
pub fn start_expr(extras: &Extras, flat: &FlatSystem, v: VarId) -> Option<Expr> {
    extras.start.get(v.0 as usize).cloned().flatten().or_else(|| flat.var(v).start.map(Expr::Const))
}
