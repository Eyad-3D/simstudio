//! The preparation pipeline (DESIGN.md, *Preparation pipeline*): from a
//! component tree to the prepared model, for the forward model and, with
//! an [`InverseSpec`], for fast mode's inverse model.

#![allow(clippy::needless_range_loop)] // parallel arrays indexed in step

use crate::causal::{self, Ctx, ParamInfo, Sorted};
use crate::diagnose::{
    self, Quantity, Structure, equation_words, info, local_name, parts_of, pretty, warning,
};
use crate::flatten::{self, Extras};
use crate::graph::{Bipartite, NONE, hopcroft_karp};
use crate::index::{self, IndexFault};
use crate::init::{self, InitBuild, RowKind};
use crate::system::{NodeEnv, NodeKind, Sys};
use crate::{alias, external, inverse, key, modes, numeric, reinit, sparsity, units_check};
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
    /// find the fewest tearing variables of each small block by exhaustive
    /// search, for the report ([`BlockSummary::minimal`]): a yardstick for
    /// the tearing heuristic, not needed to run
    pub tearing_minimum: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { force_implicit: false, numeric_start: true, tearing_minimum: false }
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
    /// the fewest tearing variables possible, by exhaustive search (when
    /// [`Settings::tearing_minimum`] asked for it and the block is small)
    pub minimal: Option<usize>,
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
                crate::walk::visit(side, &mut |x| {
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

fn index_diag(flat: &FlatSystem, sys: &Sys, f: IndexFault) -> Diagnostic {
    let origin_of = |k: usize| flat.equations[sys.eqs[k].src].origin.clone();
    match f {
        IndexFault::TooHigh { eq } => {
            let origin = origin_of(eq);
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
        IndexFault::Differentiate { eq, why } => {
            let origin = origin_of(eq);
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
        IndexFault::Singular { eqs, .. } => {
            let origins: Vec<Origin> = eqs.iter().map(|&k| origin_of(k)).collect();
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
    alias::carry_starts(&mut flat, &mut extras.start, &aliases);
    clock.lap("aliases");
    let restarted = reinit::apply(&mut flat, &mut extras.start, &aliases, &input, &mut warnings)?;
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
    for (k, e) in flat.equations.iter().enumerate() {
        let r = crate::symbolic::simplify(e.lhs.clone() - e.rhs.clone());
        let r = sys.from_flat_owned(r);
        sys.push(r, k, None);
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
                // the variables equal to it, as the model may have named
                // any of them
                let same: Vec<VarId> = std::iter::once(v)
                    .chain(aliases.iter().filter_map(|a| match a.target {
                        AliasTarget::Var { var, .. } if var == v => Some(a.var),
                        _ => None,
                    }))
                    .collect();
                let mut d = Diagnostic::error(
                    "DER-NOT-STATE",
                    format!(
                        "{} reads the rate of change of {} in {what}, but nothing in the model \
                         makes it change continuously.",
                        flat.instance_name(origin.instance),
                        if same.len() == 1 {
                            local_name(&flat, v)
                        } else {
                            format!(
                                "{} (the same as {})",
                                local_name(&flat, v),
                                same[1..]
                                    .iter()
                                    .take(4)
                                    .map(|&x| local_name(&flat, x))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            )
                        }
                    ),
                );
                // the part that reads it, and the parts the variable is of
                d.parts = parts_of(
                    &flat,
                    std::iter::once(origin.instance)
                        .chain(same.iter().map(|&x| flat.var(x).instance)),
                );
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
    let differentiated =
        index::pantelides(&mut sys).map_err(|f| vec![index_diag(&flat, &sys, f)])?;
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
        crate::walk::map_up(e, &mut |x| match x {
            Expr::Var(v) | Expr::Pre(v) if relation_of.contains_key(&(v.0 as usize)) => {
                relation_of[&(v.0 as usize)].clone()
            }
            other => other,
        })
    };
    let model_res: Option<Vec<Expr>> = if relation_of.is_empty() {
        None
    } else {
        Some(sys.eqs.iter().map(|e| subst_modes(&e.res)).collect())
    };
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
    // the start is the model's own sorting after the states' start values
    // when nothing else differs: no index reduction, no modes, no initial
    // equations, every start value kept and on a state
    let reuse = differentiated == 0
        && node_modes.is_empty()
        && initial_eqs.is_empty()
        && ib.dropped.is_empty()
        && ib.constant_conflicts.is_empty()
        && ib.starts.iter().all(|&(_, n, _)| sys.nodes[n].deriv.is_some());
    let ctx = Ctx {
        params: &params,
        kinds: &kinds,
        is_mode: &is_mode,
        force_implicit: settings.force_implicit,
    };
    let refs: Vec<&Expr> = sys.eqs.iter().map(|e| &e.res).collect();
    let incs: Vec<&[usize]> = sys.eqs.iter().map(|e| e.inc.as_slice()).collect();
    let sort_model = |is_state: &[bool]| -> Result<(Vec<usize>, Sorted), Vec<Diagnostic>> {
        let unknowns: Vec<usize> =
            (0..nn).filter(|&n| kinds[n] == NodeKind::Unknown && !is_state[n]).collect();
        let sorted = causal::sort(&refs, &incs, &unknowns, &ctx).map_err(|u| {
            let s = Structure {
                flat: &flat,
                lib,
                graph: &u.graph,
                matching: &u.matching,
                origins: sys.eqs.iter().map(|e| &flat.equations[e.src].origin).collect(),
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
        Ok((unknowns, sorted))
    };
    let trivial_states: Vec<bool> =
        sys.nodes.iter().map(|n| n.kind == NodeKind::Unknown && n.deriv.is_some()).collect();
    // needed for the state choice, the modes' first values, initial
    // equations and start values the model overrides; for small models
    // always (their pivots are checked at the start too)
    let needed = settings.numeric_start
        && (differentiated > 0
            || !node_modes.is_empty()
            || !initial_eqs.is_empty()
            || !ib.dropped.is_empty()
            || !ib.constant_conflicts.is_empty()
            || ib.kinds.iter().any(|k| matches!(k, RowKind::Assumed(_)))
            || sys.eqs.len() <= 20_000);
    let mut model_sorted = None;
    // with the model's sorting reused, no iteration variable and only
    // constant start values, the start values hold as they are: the
    // initialisation system is empty
    let mut trivial_init = false;
    let init_sorted = if reuse {
        let (_, sorted) = sort_model(&trivial_states)?;
        clock.lap("sort");
        let constant = |e: &Expr| matches!(e, Expr::Const(_));
        trivial_init = sorted.iteration.is_empty()
            && ib.starts.iter().all(|(_, _, v)| constant(v))
            && extras.start.iter().enumerate().all(|(i, s)| {
                flat.vars[i].kind != VarKind::Discrete || s.as_ref().is_none_or(constant)
            });
        let composed =
            if trivial_init && !needed { Sorted::default() } else { compose_init(&ib, &sorted) };
        model_sorted = Some(sorted);
        composed
    } else {
        let no_modes = vec![false; nn];
        let init_ctx =
            Ctx { params: &params, kinds: &kinds, is_mode: &no_modes, force_implicit: false };
        let init_refs = ib.rows(&sys);
        let init_inc = ib.incs(&sys);
        causal::sort(&init_refs, &init_inc, &ib.unknowns, &init_ctx).map_err(|_| {
            vec![Diagnostic::error(
                "INIT-SINGULAR",
                "The start of the run is not determined: the initialisation system has no \
                 matching (an engine fault; please report the model).",
            )]
        })?
    };
    clock.lap("init");

    // the start at preparation
    let mut vals = start_guesses(&sys, &flat);
    let solved = if needed {
        numeric::solve(&init_sorted, &mut vals, &pvals, 0.0).map_err(Some)
    } else {
        Err(None)
    };
    report.start_solved = solved.is_ok();
    if let Err(Some(ns)) = &solved {
        let b = &init_sorted.blocks[ns.block];
        let parts = parts_of(&flat, b.eqs.iter().map(|&k| ib.origin(&sys, &flat, k).instance));
        let names: Vec<String> =
            parts.iter().map(|p| format!("'{}'", diagnose::label_of(&flat, p))).collect();
        if b.linear && ns.singular {
            // what the singular block leaves undecided: the potentials of a
            // circuit all moving together is a circuit with no ground
            let rows: Vec<&Expr> = b.eqs.iter().map(|&k| ib.row(&sys, k)).collect();
            let mut probe = vals.clone();
            let free = numeric::null_direction(&rows, &b.nodes, &mut probe, &pvals, 0.0);
            let floating = free.and_then(|x| {
                let moving: Vec<usize> = (0..x.len()).filter(|&j| x[j].abs() > 1e-6).collect();
                let together = moving.iter().all(|&j| (x[j] - x[moving[0]]).abs() < 1e-6);
                let qs: BTreeSet<Quantity> = moving
                    .iter()
                    .map(|&j| {
                        let node = &sys.nodes[b.nodes[j]];
                        if node.order == 0 {
                            diagnose::quantity(&flat, lib, node.var)
                        } else {
                            Quantity::Other
                        }
                    })
                    .collect();
                let across = qs.len() == 1
                    && (qs.contains(&Quantity::Voltage)
                        || qs.contains(&Quantity::Temperature)
                        || qs.contains(&Quantity::Speed));
                if together && across {
                    diagnose::floating(&qs, &diagnose::join_names(&names))
                } else {
                    None
                }
            });
            if let Some(mut d) = floating {
                d.parts = parts;
                d.detail = b
                    .eqs
                    .iter()
                    .map(|&k| equation_words(&flat, ib.origin(&sys, &flat, k)))
                    .collect();
                return Err(vec![d]);
            }
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
            .with_hint(
                "Check the parameters of these parts for zeros: a resistance, an inertia or a \
                 ratio of 0.",
            );
            d.parts = parts;
            d.detail =
                b.eqs.iter().map(|&k| equation_words(&flat, ib.origin(&sys, &flat, k))).collect();
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
            equation_words(&flat, ib.origin(&sys, &flat, ns.worst_eq)),
            ns.worst
        ));
        warnings.push(d);
    }
    clock.lap("start");

    // dummy derivatives
    // how strongly to keep each node a state: a restarted state's
    // continuous part above all (reinit needs it), then a fixed start
    let mut keep_node: Vec<u8> = sys
        .nodes
        .iter()
        .map(|n| u8::from(n.order == 0 && flat.var(n.var).fixed && flat.var(n.var).start.is_some()))
        .collect();
    for r in &restarted {
        if let Some(nd) = sys.base[r.continuous.0 as usize] {
            keep_node[nd] = 2;
        }
    }
    let (is_state, dummy) = if differentiated > 0 {
        let point = if solved.is_ok() { vals.clone() } else { generic_values(&sys, &flat) };
        let sel = index::dummy_derivatives(&sys, &point, &pvals, &keep_node)
            .or_else(|f| {
                if solved.is_ok() {
                    Err(f)
                } else {
                    index::dummy_derivatives(&sys, &generic_values(&sys, &flat), &pvals, &keep_node)
                }
            })
            .map_err(|f| vec![index_diag(&flat, &sys, f)])?;
        (sel.is_state, sel.dummy)
    } else {
        (trivial_states, vec![false; nn])
    };
    report.dummy_derivatives = (0..nn).filter(|&n| dummy[n]).map(|n| sys.name(&flat, n)).collect();
    // a restarted state must stay one (in fast mode the prescribed motion
    // decides it, and the restart moves only its continuous part)
    let lost: Vec<&reinit::Restarted> = restarted
        .iter()
        .filter(|r| !sys.base[r.continuous.0 as usize].is_some_and(|nd| is_state[nd]))
        .collect();
    if spec.is_some() {
        warnings.extend(lost.iter().map(|r| reinit::prescribed(&flat, r.var, &r.origin)));
    } else if !lost.is_empty() {
        return Err(lost
            .iter()
            .map(|r| {
                reinit::not_state(
                    &flat,
                    r.var,
                    &r.origin,
                    "it is rigidly tied to other states, and index reduction had to make it \
                     follow from them",
                )
            })
            .collect());
    }
    clock.lap("states");

    // the model: sorted, torn, solved
    let sorted = match model_sorted {
        Some(s) => s,
        None => {
            let (_, s) = sort_model(&is_state)?;
            clock.lap("sort");
            s
        }
    };
    report.blocks = summaries(&sorted, &sys, &flat, |k| &flat.equations[sys.eqs[k].src].origin);
    if settings.tearing_minimum {
        let big = sorted.blocks.iter().filter(|b| b.eqs.len() > 1 || b.iter.1 > b.iter.0);
        for (summary, b) in report.blocks.iter_mut().zip(big) {
            if b.nodes.len() <= 80 {
                summary.minimal = causal::minimal_tearing(&refs, &incs, b, &ctx, b.torn, 5_000_000);
            }
        }
    }
    report.init_blocks = summaries(&init_sorted, &sys, &flat, |k| ib.origin(&sys, &flat, k));
    warnings.extend(causal_loops(&sorted, &sys, &flat, lib));

    // checks at the start
    if solved.is_ok() {
        let env = NodeEnv { t: 0.0, vals: &vals, params: &pvals };
        for (a, k) in &sorted.variable_pivots {
            let v = eval(a, &env);
            if v == 0.0 || !v.is_finite() {
                let o = &flat.equations[sys.eqs[*k].src].origin;
                let mut d = warning(
                    "PIVOT-ZERO-AT-START",
                    format!(
                        "At the start, solving {} divides by {}, which is {} there: the run \
                         would start with an infinite or undefined value.",
                        equation_words(&flat, o),
                        sys.pretty(&flat, a),
                        if v == 0.0 { "0".to_string() } else { v.to_string() }
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
    let origin_of = |k: usize, flat: &FlatSystem| flat.equations[sys.eqs[k].src].origin.clone();
    let largest = sorted.blocks.iter().map(|b| b.eqs.len()).max().unwrap_or(0);
    let n_blocks = sorted.blocks.len();
    let Sorted {
        assignments: sorted_assignments,
        iteration: sorted_iteration,
        residuals: sorted_residuals,
        guards: sorted_guards,
        ..
    } = sorted;
    let mut assignments: Vec<Assignment> = map
        .chained
        .iter()
        .map(|&(x, y)| Assignment {
            target: Slot::Der(x),
            expr: Expr::Var(y),
            origin: init::start_origin(&flat, x),
        })
        .collect();
    assignments.reserve(sorted_assignments.len());
    for (n, e, k) in sorted_assignments {
        assignments.push(Assignment {
            target: map.slot[n],
            expr: map.to_flat_owned(e),
            origin: origin_of(k, &flat),
        });
    }
    let algebraics: Vec<Slot> = sorted_iteration.iter().map(|&n| map.slot[n]).collect();
    let residuals: Vec<Residual> = sorted_residuals
        .into_iter()
        .map(|(e, k)| Residual { expr: map.to_flat_owned(e), origin: origin_of(k, &flat) })
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
        // positive where the relation holds: rising sets 1, falling 0
        let f = modes::crossing(&relation);
        let k = zero_crossings.len();
        for (dir, value) in [(Direction::Rising, 1.0), (Direction::Falling, 0.0)] {
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
    for (a, k) in sorted_guards {
        if seen_guard.insert(a.to_string()) {
            guards.push(ParamGuard { expr: map.to_flat_owned(a), origin: origin_of(k, &flat) });
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
    let discrete_starts: Vec<Expr> = discretes
        .iter()
        .map(|v| match mode_rel_flat.get(v) {
            Some(r) => r.clone(),
            None => extras.start[v.0 as usize]
                .clone()
                .unwrap_or(Expr::Const(flat.var(*v).start.unwrap_or(0.0))),
        })
        .collect();
    let init_iteration = init_sorted.iteration.clone();
    let init_system = if trivial_init {
        InitSystem::default()
    } else {
        InitSystem {
            unknowns: init_iteration.iter().map(|&n| map.slot[n]).collect(),
            guesses: init_iteration.iter().map(|&n| guess_of(n)).collect(),
            assignments: init_sorted
                .assignments
                .into_iter()
                .map(|(n, e, k)| Assignment {
                    target: map.slot[n],
                    expr: map.to_flat_owned(e),
                    origin: ib.origin(&sys, &flat, k).clone(),
                })
                .collect(),
            residuals: init_sorted
                .residuals
                .into_iter()
                .map(|(e, k)| Residual {
                    expr: map.to_flat_owned(e),
                    origin: ib.origin(&sys, &flat, k).clone(),
                })
                .collect(),
            discrete_starts,
        }
    };

    // the start values the Stage 1 code generator bakes in
    if solved.is_ok() {
        for (k, &n) in state_nodes.iter().enumerate() {
            flat.vars[map.states[k].0 as usize].start = Some(vals[n]);
        }
        for &n in &sorted_iteration {
            if let Slot::Var(v) = map.slot[n] {
                flat.vars[v.0 as usize].start = Some(vals[n]);
            }
        }
        let env = NodeEnv { t: 0.0, vals: &vals, params: &pvals };
        for m in &node_modes {
            flat.vars[m.var.0 as usize].start = Some(eval(&m.relation, &env));
        }
    }

    let stats = PrepStats {
        flat_vars,
        flat_equations,
        aliases: aliases.len(),
        blocks: n_blocks,
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

/// The initialisation system as the model's own sorting after the states'
/// start values (each `node := start`).
fn compose_init(ib: &InitBuild, sorted: &Sorted) -> Sorted {
    let mut out = Sorted::default();
    for (row, n, value) in &ib.starts {
        let a = out.assignments.len();
        out.assignments.push((*n, value.clone(), *row));
        out.blocks.push(causal::BlockInfo {
            eqs: vec![*row],
            nodes: vec![*n],
            assign: (a, a + 1),
            ..Default::default()
        });
    }
    let shift = out.assignments.len();
    out.assignments.extend(sorted.assignments.iter().cloned());
    out.iteration = sorted.iteration.clone();
    out.residuals = sorted.residuals.clone();
    out.blocks.extend(sorted.blocks.iter().map(|b| causal::BlockInfo {
        assign: (b.assign.0 + shift, b.assign.1 + shift),
        ..b.clone()
    }));
    out
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
        origins: sys.eqs.iter().map(|e| &flat.equations[e.src].origin).collect(),
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
            minimal: None,
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
            let o = &flat.equations[sys.eqs[k].src].origin;
            matches!(o.kind, OriginKind::SignalLink { .. })
                || (o.instance.0 != 0 && !diagnose::is_physical(flat, lib, o.instance))
        });
        if !causal {
            continue;
        }
        let parts =
            parts_of(flat, b.eqs.iter().map(|&k| flat.equations[sys.eqs[k].src].origin.instance));
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
