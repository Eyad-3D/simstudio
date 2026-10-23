//! Dynamic state selection in the run loop ([`crate::choice`], DESIGN.md
//! §6 step 5): the models a run switches to, kept for the run's length,
//! and what carries the run over from one to the next.
//!
//! Where the watch ([`crate::states`]) finds a set's factor past its
//! switch threshold (inside a step: the step is cut back there, on its
//! dense output), the run asks the model's [`crate::choice::StateChoices`]
//! for the model with the states chosen again at that point and checks
//! that it can go on with it: the same events, discrete values, inputs,
//! table guards and energy parts (the same model, sorted otherwise), every
//! channel the run records there by name, every one of its states and
//! iteration variables given by the point, its own choice regular there
//! and inside its own switch threshold. Then the integrator restarts on the
//! new states from the solution at the point (its iteration variables
//! solved again with the new states held, as at any event), the energy
//! integrals carried over, the stored energy's change booked as an event's
//! (the books close as before), and the run goes on. Otherwise it goes on
//! with the states it has, and the watch stops it where they degrade past
//! the run's limit, naming the constraint, as without dynamic selection.

use crate::choice::{ChoicePoint, Variant};
use crate::{BlockInfo, RunInfo};
use lsim_ir::runtime::{EvalInput, ModelFunctions};
use std::cell::OnceCell;
use std::sync::Arc;

/// The most models one run switches between (each distinct choice once).
pub(super) const MAX_MODELS: usize = 32;

/// A model a run switches to, with the run's own information on it (its
/// parameters, time events, sampled blocks and choices).
pub(super) struct RunModel {
    pub variant: Arc<Variant>,
    pub info: RunInfo,
}

/// The models a run switches to: each kept, in place, for the run's
/// length (the run loop's helpers borrow them as they borrow the first).
pub(super) struct Arena {
    slots: Vec<OnceCell<RunModel>>,
}

impl Arena {
    pub fn new() -> Arena {
        Arena { slots: (0..MAX_MODELS).map(|_| OnceCell::new()).collect() }
    }

    /// The model kept for the choice `states`, if any.
    pub fn find(&self, states: &[String]) -> Option<&RunModel> {
        self.slots.iter().filter_map(|s| s.get()).find(|m| m.variant.states == states)
    }

    /// Keeps `m`; `None` when the run has switched between too many models.
    pub fn keep(&self, m: RunModel) -> Option<&RunModel> {
        let slot = self.slots.iter().find(|s| s.get().is_none())?;
        let _ = slot.set(m);
        slot.get()
    }
}

/// The point to choose at: every channel of the model the run is on (its
/// names `names`, values `vars`), each state's derivative as `der(<state>)`
/// (`xdot`), the parameters, and the derivatives among the channels the
/// run records (`recorded`), which the model switched to keeps.
pub(super) fn point(
    t: f64,
    info: &RunInfo,
    vars: &[f64],
    y: &[f64],
    xdot: &[f64],
    recorded: &[String],
    window: f64,
) -> ChoicePoint {
    let mut values: Vec<(String, f64)> =
        info.var_names.iter().cloned().zip(vars.iter().copied()).collect();
    for (name, &v) in info.y_names.iter().zip(xdot) {
        let der = format!("der({name})");
        if !info.var_names.contains(&der) {
            values.push((der, v));
        }
    }
    // (an iteration variable that is a state's derivative is no channel)
    for (name, &v) in info.y_names.iter().zip(y).skip(xdot.len()) {
        if !values.iter().any(|(n, _)| n == name) {
            values.push((name.clone(), v));
        }
    }
    ChoicePoint {
        t,
        values,
        params: info.params.clone(),
        channels: recorded.iter().filter(|n| n.starts_with("der(")).cloned().collect(),
        window,
    }
}

/// Whether the run can go on with `to` where it is on `from`: the same
/// events, discrete values, inputs, parameters, table guards, sampled
/// blocks, modes and energy parts. Why not, where it cannot.
pub(super) fn compatible(
    from: (&dyn ModelFunctions, &RunInfo),
    to: (&dyn ModelFunctions, &RunInfo),
) -> Result<(), String> {
    let (a, b) = (from.0.layout(), to.0.layout());
    if (a.n_roots, a.n_whens, a.n_d, a.n_u, a.n_p, a.n_x)
        != (b.n_roots, b.n_whens, b.n_d, b.n_u, b.n_p, b.n_x)
    {
        return Err("the model with the states chosen again has other events, discrete values \
                    or inputs"
            .into());
    }
    if from.0.table_guard_list().len() != to.0.table_guard_list().len() {
        return Err("the model with the states chosen again watches other tables".into());
    }
    // the same tables, to the bit, as each model interpolates them (a model
    // given other table data after it was built keeps the detection: each
    // model with the states chosen again is compiled from the data the
    // model was prepared with)
    for k in 0..from.1.table_names.len() as u32 {
        match (from.0.table_interp(k), to.0.table_interp(k)) {
            (Some(a), Some(b)) if a.same(b) => {}
            (Some(_), Some(_)) => {
                return Err("the model's tables were changed after it was built (the models \
                            with the states chosen again read the data it was prepared with)"
                    .into());
            }
            _ => {
                return Err("the model does not give its tables (ModelFunctions::table_interp), \
                            so the run cannot tell that the model with the states chosen again \
                            reads the same"
                    .into());
            }
        }
    }
    let (fi, ti) = (from.1, to.1);
    if fi.modes.len() != ti.modes.len()
        || fi.whens != ti.whens
        || fi.asserts.len() != ti.asserts.len()
        || fi.time_crossings.len() != ti.time_crossings.len()
        || fi.time_functions.len() != ti.time_functions.len()
    {
        return Err("the model with the states chosen again has other conditions".into());
    }
    let parts = |i: &RunInfo| -> Vec<(String, bool, bool)> {
        i.energy.as_ref().map_or(vec![], |e| {
            e.parts.iter().map(|p| (p.path.clone(), p.loss.is_some(), p.stored.is_some())).collect()
        })
    };
    if parts(fi) != parts(ti) {
        return Err("the model with the states chosen again books other parts".into());
    }
    Ok(())
}

/// The run's own information on the model `v` switched to, from the one
/// it is on (`now`): its parameters, time events and choices, and the
/// sampled blocks with their inputs found by name (a block the model
/// itself declares keeps the model's way of reading them).
pub(super) fn run_info(v: &Variant, now: &RunInfo) -> Result<RunInfo, String> {
    let mut info = v.info.clone();
    if info.params.len() != now.params.len() {
        return Err("the model with the states chosen again has other parameters".into());
    }
    info.params.clone_from(&now.params);
    info.time_events.clone_from(&now.time_events);
    info.choices.clone_from(&now.choices);
    let mut blocks = vec![];
    for b in &now.blocks {
        let inputs: Option<Vec<usize>> = b
            .inputs
            .iter()
            .map(|&i| {
                let name = now.var_names.get(i)?;
                info.var_names.iter().position(|n| n == name)
            })
            .collect();
        let Some(inputs) = inputs else {
            return Err(format!(
                "the sampled block {} reads a channel the model with the states chosen again \
                 does not have",
                b.name
            ));
        };
        let own = v.info.blocks.iter().find(|o| o.name == b.name && o.inputs == inputs);
        blocks.push(BlockInfo {
            name: b.name.clone(),
            chains: own.map_or_else(|| vec![None; inputs.len()], |o| o.chains.clone()),
            inputs,
            outputs: b.outputs.clone(),
            period: b.period,
        });
    }
    info.blocks = blocks;
    // the energy books the run keeps: the model's own (its parts read in
    // its own channels), or those the run was given, read by name
    let own = |e: &Option<Arc<crate::EnergyInfo>>| -> Vec<(String, String)> {
        e.as_ref()
            .map_or(vec![], |e| e.parts.iter().map(|p| (p.path.clone(), p.name.clone())).collect())
    };
    if own(&info.energy) != own(&now.energy) {
        info.energy = match &now.energy {
            None => None,
            Some(e) => {
                let to = &info.var_names;
                let read = |x: &lsim_ir::Expr| remap(x, &now.var_names, to);
                let parts: Option<Vec<crate::EnergyPart>> = e
                    .parts
                    .iter()
                    .map(|p| {
                        Some(crate::EnergyPart {
                            path: p.path.clone(),
                            name: p.name.clone(),
                            power: read(&p.power)?,
                            loss: match &p.loss {
                                Some(l) => Some(read(l)?),
                                None => None,
                            },
                            stored: match &p.stored {
                                Some(s) => Some(read(s)?),
                                None => None,
                            },
                        })
                    })
                    .collect();
                let parts = parts.ok_or_else(|| {
                    "the energy books read a channel the model with the states chosen again \
                     does not have"
                        .to_string()
                })?;
                Some(Arc::new(crate::EnergyInfo { parts }))
            }
        };
        info.stored_rates = None;
        info.impulse = None;
    }
    let tabled = info.with_model_tables(&*v.model);
    Ok(tabled.unwrap_or(info))
}

/// `e` over the channels `from` rewritten over the channels `to`, by name;
/// `None` where one it reads is not there.
fn remap(e: &lsim_ir::Expr, from: &[String], to: &[String]) -> Option<lsim_ir::Expr> {
    use lsim_ir::{Expr, VarId};
    let mut ok = true;
    let mut find = |v: VarId| -> VarId {
        let hit = from.get(v.0 as usize).and_then(|n| to.iter().position(|m| m == n));
        match hit {
            Some(i) => VarId(i as u32),
            None => {
                ok = false;
                v
            }
        }
    };
    let out = e.clone().rewrite(&mut |x| match x {
        Expr::Var(v) => Expr::Var(find(v)),
        Expr::Der(v) => Expr::Der(find(v)),
        Expr::Pre(v) => Expr::Pre(find(v)),
        other => other,
    });
    ok.then_some(out)
}

/// For each channel the run records (`names`), its index among the
/// channels of the model `info` describes; `None` where one is missing.
pub(super) fn channel_map(names: &[String], info: &RunInfo) -> Option<Vec<usize>> {
    names.iter().map(|n| info.var_names.iter().position(|m| m == n)).collect()
}

/// The new model's states and iteration variables (`info.y_names`) from
/// the point's values; `None` where one is not there or not a number.
pub(super) fn y_from(info: &RunInfo, at: &ChoicePoint) -> Option<Vec<f64>> {
    info.y_names
        .iter()
        .map(|n| at.values.iter().find(|(m, _)| m == n).map(|&(_, v)| v).filter(|v| v.is_finite()))
        .collect()
}

/// The derivatives of the states of `model` at `(t, y, d)`: the first
/// `n_x` entries of its residual.
pub(super) fn state_rates(
    model: &dyn ModelFunctions,
    info: &RunInfo,
    t: f64,
    y: &[f64],
    d: &[f64],
    u: &[f64],
) -> Vec<f64> {
    let l = *model.layout();
    let mut work = vec![0.0; l.n_work];
    let mut out = vec![0.0; l.n_y()];
    model.residual(&EvalInput { t, y, p: &info.params, d, u }, &mut work, &mut out);
    out.truncate(l.n_x);
    out
}

/// The model a run goes on with, chosen at a point.
pub(super) struct Next<'a> {
    pub model: &'a dyn ModelFunctions,
    pub info: &'a RunInfo,
    /// for each channel the run records, its index among the new model's
    pub map: Vec<usize>,
    /// the new model's states and iteration variables at the point
    pub y: Vec<f64>,
    /// its states, by name
    pub states: Vec<String>,
}

/// The states, by name, of the model `info` describes (of `n_x` states).
pub(super) fn states_of(info: &RunInfo, n_x: usize) -> Vec<String> {
    info.y_names.iter().take(n_x).cloned().collect()
}

/// The model to go on with from `(t, y, d)` on `now` (the run's first
/// model `first`, the channels it records `recorded`): the states chosen
/// again there, by the model's choices ([`RunInfo::choices`]); why not,
/// where the run cannot go on with another choice there.
#[allow(clippy::too_many_arguments)]
pub(super) fn choose_at<'a>(
    arena: &'a Arena,
    now: (&'a dyn ModelFunctions, &'a RunInfo),
    first: (&'a dyn ModelFunctions, &'a RunInfo),
    recorded: &[String],
    t: f64,
    y: &[f64],
    d: &[f64],
    u: &[f64],
    window: f64,
) -> Result<Next<'a>, String> {
    let Some(choices) = &now.1.choices else {
        return Err("the model has no other choice of states".into());
    };
    let l = *now.0.layout();
    let mut work = vec![0.0; l.n_work];
    let mut vars = vec![0.0; l.n_vars];
    now.0.vars(&EvalInput { t, y, p: &now.1.params, d, u }, &mut work, &mut vars);
    let xdot = state_rates(now.0, now.1, t, y, d, u);
    let at = point(t, now.1, &vars, y, &xdot, recorded, window);
    let v = choices.0.choose(&at)?;
    if v.states == states_of(now.1, l.n_x) {
        return Err("choosing the states again there gives the same states".into());
    }
    let (model, info): (&'a dyn ModelFunctions, &'a RunInfo) =
        if v.states == states_of(first.1, first.0.layout().n_x) {
            first
        } else if let Some(m) = arena.find(&v.states) {
            (&*m.variant.model, &m.info)
        } else {
            let info = run_info(&v, now.1)?;
            let m = arena.keep(RunModel { variant: v.clone(), info }).ok_or_else(|| {
                format!("the run has switched between {MAX_MODELS} choices of states already")
            })?;
            (&*m.variant.model, &m.info)
        };
    compatible(now, (model, info))?;
    let map = channel_map(recorded, info).ok_or_else(|| {
        "the model with the states chosen again does not have every channel the run records"
            .to_string()
    })?;
    let y = y_from(info, &at).ok_or_else(|| {
        "the point does not give every state of the model with the states chosen again".to_string()
    })?;
    Ok(Next { model, info, map, y, states: v.states.clone() })
}
