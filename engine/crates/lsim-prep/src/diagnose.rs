//! Structural diagnostics, told in terms of the parts on the user's
//! diagram (DESIGN.md, *Error messages*).
//!
//! A structurally singular system is split by the Dulmage–Mendelsohn
//! decomposition: alternating paths from the unmatched equations give the
//! over-determined part, from the unmatched unknowns the under-determined
//! part. Each part's equations and unknowns lead, through their origins,
//! to the parts the user placed. Then the **catalogue** recognises the
//! common faults and tells them with a code of their own and a tailored
//! hint:
//!
//! | code | the fault |
//! |---|---|
//! | `ELEC-SOURCE-LOOP` | ideal voltage sources in parallel, or wired in a loop with no resistance |
//! | `ELEC-NO-GROUND` | a circuit with no ground: its voltages have no reference |
//! | `ELEC-CURRENT-SOURCES` | ideal current sources in series, or one with nowhere to send its current |
//! | `THERM-FLOATING` | a thermal network with no heat capacity and no fixed temperature |
//! | `THERM-TEMP-CONFLICT` | two fixed temperatures on one thermal node |
//! | `MECH-SPEED-CONFLICT` | two speed sources on one rigid shaft (or one translating body) |
//! | `MECH-FLOATING` | a shaft with nothing to drive or hold it |
//! | `PART-UNCONNECTED` | a part not connected at all |
//! | `GEAR-NO-RATIO` | a gearbox whose gear input is left open (flattening) |
//! | `SIGNAL-UNCONNECTED` | a signal input left open (flattening) |
//! | `SIGNAL-SOURCES` | a signal driven by no output, or by two (flattening) |
//! | `CAUSAL-LOOP` | an algebraic loop through controllers with no state or delay to break it |
//! | `SINGULAR-LOOP` | a loop the equations cannot solve with the parameters given (a loop of sources whose resistances are all zero) |
//! | `INIT-OVER` | initial equations that contradict the model or each other |
//! | `INIT-START-IGNORED` | a fixed start value the model cannot meet (warning) |
//! | `STATE-SELECT-SINGULAR` | no fixed choice of states works at the start (index reduction) |
//! | `REINIT-NOT-STATE` | `reinit` of something that is not a state, or cannot stay one |
//! | `INVERSE-OVER`, `INVERSE-UNDER` | fast mode's prescription over- or under-determines the model |
//! | `STRUCT-OVER`, `STRUCT-UNDER` | anything else structurally singular |

use crate::graph::{Bipartite, Matching, over_part, under_part};
use lsim_ir::component::{Library, PortKind};
use lsim_ir::flat::{FlatSystem, InstanceId, Origin, OriginKind, VarId, VarRole};
use lsim_ir::{Diagnostic, Severity};
use std::collections::BTreeSet;

/// The physical quantity a variable is.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Quantity {
    /// electrical potential
    Voltage,
    /// electrical current
    Current,
    /// rotational or translational speed
    Speed,
    /// torque or force
    Torque,
    /// temperature
    Temperature,
    /// heat flow
    Heat,
    /// anything else
    Other,
}

/// What a variable is, physically, from its port's connector.
pub fn quantity(flat: &FlatSystem, lib: &Library, v: VarId) -> Quantity {
    let var = flat.var(v);
    let (port, across) = match &var.role {
        VarRole::Across { port } => (port, true),
        VarRole::Through { port } => (port, false),
        // an internal variable (an inertia's own speed, which alias
        // elimination keeps for the shaft): its unit says what it is
        _ => return by_dimension(var.unit.dim),
    };
    let def = &flat.instance(var.instance).def;
    let connector =
        lib.components.get(def).and_then(|d| d.ports.iter().find(|p| &p.name == port)).and_then(
            |p| match &p.kind {
                PortKind::Physical { connector } => Some(connector.as_str()),
                _ => None,
            },
        );
    match (connector, across) {
        (Some("Pin"), true) => Quantity::Voltage,
        (Some("Pin"), false) => Quantity::Current,
        (Some("Flange" | "TFlange"), true) => Quantity::Speed,
        (Some("Flange" | "TFlange"), false) => Quantity::Torque,
        (Some("HeatPort"), true) => Quantity::Temperature,
        (Some("HeatPort"), false) => Quantity::Heat,
        _ => Quantity::Other,
    }
}

/// What a quantity is from its dimension alone.
fn by_dimension(d: lsim_ir::units::Dim) -> Quantity {
    match d.0 {
        [2, 1, -3, -1, 0, 0, 0] => Quantity::Voltage,
        [0, 0, 0, 1, 0, 0, 0] => Quantity::Current,
        [0, 0, -1, 0, 0, 0, 0] | [1, 0, -1, 0, 0, 0, 0] => Quantity::Speed,
        [2, 1, -2, 0, 0, 0, 0] | [1, 1, -2, 0, 0, 0, 0] => Quantity::Torque,
        [0, 0, 0, 0, 1, 0, 0] => Quantity::Temperature,
        [2, 1, -3, 0, 0, 0, 0] => Quantity::Heat,
        _ => Quantity::Other,
    }
}

/// `p.v of 'R1'`.
pub fn local_name(flat: &FlatSystem, v: VarId) -> String {
    let var = flat.var(v);
    let path = &flat.instance(var.instance).path;
    let local = var.name.strip_prefix(path.as_str()).unwrap_or(&var.name).trim_start_matches('.');
    format!("{local} of {}", flat.instance_name(var.instance))
}

/// An equation in words: `'R1' (Ohm's law)`, `the connection of a.p, b.n`.
pub fn equation_words(flat: &FlatSystem, o: &Origin) -> String {
    match (&o.kind, &o.label) {
        (OriginKind::Component { .. }, Some(l)) => {
            format!("{} ({l})", flat.instance_name(o.instance))
        }
        (OriginKind::Component { index }, None) => {
            format!("{} (equation {})", flat.instance_name(o.instance), index + 1)
        }
        (OriginKind::ConnectionAcross { ports } | OriginKind::ConnectionThrough { ports }, _) => {
            format!("the connection of {}", ports.join(", "))
        }
        (OriginKind::Unconnected { port }, _) => format!("the free port {port}"),
        (OriginKind::SignalLink { input, output }, _) => format!("the link {output} → {input}"),
    }
}

/// The diagram parts (top-level instances) behind some instances, sorted.
pub fn parts_of(flat: &FlatSystem, insts: impl Iterator<Item = InstanceId>) -> Vec<String> {
    let mut set = BTreeSet::new();
    for i in insts {
        if i.0 != 0 {
            set.insert(flat.instance(flat.top_part(i)).path.clone());
        }
    }
    set.into_iter().collect()
}

/// A part's label (or its path).
pub fn label_of(flat: &FlatSystem, path: &str) -> String {
    flat.instances
        .iter()
        .find(|i| i.path == path)
        .and_then(|i| i.label.clone())
        .unwrap_or_else(|| path.to_string())
}

/// `1 equation`, `3 unknowns`.
pub fn count(n: usize, what: &str) -> String {
    if n == 1 { format!("1 {what}") } else { format!("{n} {what}s") }
}

/// `'A'`, `'A' and 'B'`, `'A', 'B' and 'C'`.
pub fn join_names(names: &[String]) -> String {
    match names {
        [] => "The model's equations".into(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

fn quoted(flat: &FlatSystem, parts: &[String]) -> Vec<String> {
    parts.iter().map(|p| format!("'{}'", label_of(flat, p))).collect()
}

/// Whether an instance's definition has physical ports (a physical part,
/// not a controller).
pub fn is_physical(flat: &FlatSystem, lib: &Library, i: InstanceId) -> bool {
    let def = &flat.instance(i).def;
    lib.components.get(def).is_some_and(|d| {
        d.ports.iter().any(|p| matches!(p.kind, PortKind::Physical { .. }))
            || !d.components.is_empty()
    })
}

/// What a structural check works on.
pub struct Structure<'a> {
    /// the flat system
    pub flat: &'a FlatSystem,
    /// the library (for connector types)
    pub lib: &'a Library,
    /// equations × unknown columns
    pub graph: &'a Bipartite,
    /// a maximum matching
    pub matching: &'a Matching,
    /// each row's origin
    pub origins: Vec<&'a Origin>,
    /// each row's text, for the details
    pub texts: Vec<String>,
    /// each column's variable
    pub vars: Vec<VarId>,
    /// the top-level parts with no physical port connected
    pub isolated: &'a [InstanceId],
    /// an inverse model's prescribed variables (empty otherwise)
    pub prescribed: &'a [VarId],
    /// per row: it reads a prescribed input
    pub row_has_input: Vec<bool>,
}

/// The diagnostics of a structurally singular system.
pub fn singular(s: &Structure<'_>) -> Vec<Diagnostic> {
    let flat = s.flat;
    let mut out = vec![];
    let inverse = !s.prescribed.is_empty();
    let unmatched_rows = s.matching.row.contains(&crate::graph::NONE);
    let unmatched_cols = s.matching.col.contains(&crate::graph::NONE);
    let mut over_parts: Vec<String> = vec![];
    if unmatched_rows {
        let (rows, cols) = over_part(s.graph, s.matching);
        let parts = parts_of(flat, rows.iter().map(|&r| s.origins[r].instance));
        over_parts = parts.clone();
        let names = quoted(flat, &parts);
        let words: Vec<String> = rows.iter().map(|&r| equation_words(flat, s.origins[r])).collect();
        let what: Vec<String> = cols.iter().map(|&c| local_name(flat, s.vars[c])).collect();
        let qs: BTreeSet<_> = cols.iter().map(|&c| quantity(flat, s.lib, s.vars[c])).collect();
        // the parts whose own equations (not connections) take part
        let setters: BTreeSet<String> = rows
            .iter()
            .filter(|&&r| matches!(s.origins[r].kind, OriginKind::Component { .. }))
            .filter(|&&r| s.origins[r].instance.0 != 0)
            .map(|&r| flat.instance(flat.top_part(s.origins[r].instance)).path.clone())
            .collect();
        let setter_names = quoted(flat, &setters.iter().cloned().collect::<Vec<_>>());
        let only = |q: Quantity| qs.len() == 1 && qs.contains(&q);
        let generic = if cols.is_empty() {
            format!(
                "{} set the same quantity more than once: {} ({}) with nothing left to decide.",
                join_names(&names),
                count(rows.len(), "equation"),
                words.join("; "),
            )
        } else {
            format!(
                "{} set the same quantity more than once: {} ({}) for {} ({}).",
                join_names(&names),
                count(rows.len(), "equation"),
                words.join("; "),
                count(cols.len(), "unknown"),
                what.join(", ")
            )
        };
        let prescribed_involved = inverse && rows.iter().any(|&r| s.row_has_input[r]);
        let mut d = if inverse && prescribed_involved {
            Diagnostic::error(
                "INVERSE-OVER",
                format!(
                    "In fast mode the prescribed speed already decides what {} set: {} ({}).",
                    join_names(&names),
                    count(rows.len(), "equation"),
                    words.join("; ")
                ),
            )
            .with_hint(
                "Prescribe only one speed of a rigid driveline, and free only the commands the \
                 motion decides.",
            )
        } else if setters.len() >= 2 && only(Quantity::Voltage) {
            Diagnostic::error(
                "ELEC-SOURCE-LOOP",
                format!(
                    "{} are ideal voltage sources connected in parallel (or in a loop with no \
                     resistance between them): they fight over one voltage, and nothing limits \
                     the current around the loop.",
                    join_names(&setter_names)
                ),
            )
            .with_hint(
                "Two ideal voltage sources connected in parallel fight over one voltage: put a \
                 resistance between them, or remove one.",
            )
        } else if setters.len() >= 2 && only(Quantity::Current) {
            Diagnostic::error(
                "ELEC-CURRENT-SOURCES",
                format!(
                    "{} are ideal current sources in series (or a current source with nowhere \
                     else for its current to go): they fight over one current.",
                    join_names(&setter_names)
                ),
            )
            .with_hint(
                "Give the current another path (a resistance in parallel), or remove one source.",
            )
        } else if setters.len() >= 2 && only(Quantity::Speed) {
            Diagnostic::error(
                "MECH-SPEED-CONFLICT",
                format!(
                    "{} both set the speed of the same rigid shaft: only one of them can.",
                    join_names(&setter_names)
                ),
            )
            .with_hint(
                "Put a compliance (spring and damper) or a clutch between them, or prescribe the \
                 speed in one place only.",
            )
        } else if setters.len() >= 2 && only(Quantity::Temperature) {
            Diagnostic::error(
                "THERM-TEMP-CONFLICT",
                format!(
                    "{} both fix the temperature of the same thermal node.",
                    join_names(&setter_names)
                ),
            )
            .with_hint("Connect them through a thermal conductance, or keep one fixed temperature.")
        } else {
            Diagnostic::error("STRUCT-OVER", generic.clone()).with_hint(
                if setters.len() >= 2 && setter_names.iter().any(|n| n.contains("ource")) {
                    "Two ideal voltage sources connected in parallel fight over one voltage: put a \
                     resistance between them, or remove one."
                } else {
                    "Look for two parts that each fix the same voltage, speed or temperature and \
                     are connected directly to each other."
                },
            )
        };
        if d.code != "STRUCT-OVER" {
            d.detail.push(generic);
        }
        d.parts = parts;
        d.detail.extend(rows.iter().map(|&r| s.texts[r].clone()));
        out.push(d);
    }
    if unmatched_cols {
        let gt = s.graph.transpose();
        let (rows, cols) = under_part(s.graph, &gt, s.matching);
        let vars: Vec<VarId> = cols.iter().map(|&c| s.vars[c]).collect();
        let mut parts = parts_of(flat, vars.iter().map(|&v| flat.var(v).instance));
        // the parts whose equations take part as well: a floating network
        // is named by all its parts, not only those holding its unknowns
        let wide = parts_of(
            flat,
            vars.iter()
                .map(|&v| flat.var(v).instance)
                .chain(rows.iter().map(|&r| s.origins[r].instance)),
        );
        let names = quoted(flat, &parts);
        let what: Vec<String> = vars.iter().map(|&v| local_name(flat, v)).collect();
        let qs: BTreeSet<_> = vars.iter().map(|&v| quantity(flat, s.lib, v)).collect();
        let only = |allowed: &[Quantity]| qs.iter().all(|q| allowed.contains(q));
        let generic = format!(
            "Nothing determines {}: {} with {} between them, in {}.",
            what.join(", "),
            count(cols.len(), "unknown"),
            if rows.is_empty() { "no equation".to_string() } else { count(rows.len(), "equation") },
            join_names(&names)
        );
        let isolated: Vec<String> = s
            .isolated
            .iter()
            .map(|i| flat.instance(*i).path.clone())
            .filter(|p| parts.contains(p))
            .collect();
        let explained_by_loop = !over_parts.is_empty()
            && parts.iter().all(|p| over_parts.contains(p))
            && only(&[Quantity::Current]);
        let d = if explained_by_loop {
            None
        } else if inverse {
            Some(
                Diagnostic::error(
                    "INVERSE-UNDER",
                    format!(
                        "In fast mode nothing works out {} from the prescribed motion, in {}.",
                        what.join(", "),
                        join_names(&names)
                    ),
                )
                .with_hint(
                    "Free only as many commands as the motion decides: one per prescribed speed \
                     (a braking and a driving command at once cannot both follow from the speed).",
                ),
            )
        } else if !isolated.is_empty() {
            Some(
                Diagnostic::error(
                    "PART-UNCONNECTED",
                    format!(
                        "{} {} not connected to anything, so nothing determines {}.",
                        join_names(&quoted(flat, &isolated)),
                        if isolated.len() == 1 { "is" } else { "are" },
                        what.join(", ")
                    ),
                )
                .with_hint("Wire it into the model, or delete it."),
            )
        } else if let Some(d) = floating(&qs, &join_names(&quoted(flat, &wide))) {
            parts = wide;
            Some(d)
        } else {
            Some(Diagnostic::error("STRUCT-UNDER", generic.clone()).with_hint(
                "Look for a part that is not connected, a circuit with no Ground, or a shaft with \
                 nothing to drive or hold it.",
            ))
        };
        if let Some(mut d) = d {
            if d.code != "STRUCT-UNDER" {
                d.detail.push(generic);
            }
            d.parts = parts;
            d.detail.extend(rows.iter().map(|&r| s.texts[r].clone()));
            out.push(d);
        }
    }
    out
}

/// The diagnostic of a network whose across quantity has no reference,
/// when the quantities `qs` left undecided are those of one domain (its
/// across quantity among them): `ELEC-NO-GROUND`, `THERM-FLOATING` or
/// `MECH-FLOATING`. `names` names the network's parts.
pub fn floating(qs: &BTreeSet<Quantity>, names: &str) -> Option<Diagnostic> {
    use Quantity::*;
    let within = |allowed: &[Quantity]| qs.iter().all(|q| allowed.contains(q));
    if qs.contains(&Voltage) && within(&[Voltage, Current, Other]) {
        Some(
            Diagnostic::error(
                "ELEC-NO-GROUND",
                format!(
                    "The circuit of {names} has no ground: nothing fixes its voltages, only the \
                     differences between them."
                ),
            )
            .with_hint("Connect a Ground block to one node of the circuit."),
        )
    } else if qs.contains(&Temperature) && within(&[Temperature, Heat, Other]) {
        Some(
            Diagnostic::error(
                "THERM-FLOATING",
                format!(
                    "The thermal network of {names} has no heat capacity and no fixed \
                     temperature: nothing decides its temperatures."
                ),
            )
            .with_hint("Give one part a heat capacity, or connect the network to the ambient."),
        )
    } else if qs.contains(&Speed) && within(&[Speed, Torque, Other]) {
        Some(
            Diagnostic::error(
                "MECH-FLOATING",
                format!(
                    "Nothing drives or holds the shaft of {names}: it has no inertia, no speed \
                     source and no fixed point, so its speed is not decided."
                ),
            )
            .with_hint("Give the shaft an inertia, or fix it to the frame."),
        )
    } else {
        None
    }
}

/// An expression with names instead of indices, for the details.
pub fn pretty(flat: &FlatSystem, e: &lsim_ir::Expr) -> String {
    use lsim_ir::Expr;
    let named = e.clone().rewrite(&mut |x| match x {
        Expr::Var(v) if (v.0 as usize) < flat.vars.len() => Expr::Name(flat.var(v).name.clone()),
        Expr::Der(v) if (v.0 as usize) < flat.vars.len() => {
            Expr::Call(lsim_ir::Builtin::Der, vec![Expr::Name(flat.var(v).name.clone())])
        }
        Expr::Pre(v) if (v.0 as usize) < flat.vars.len() => {
            Expr::Call(lsim_ir::Builtin::Pre, vec![Expr::Name(flat.var(v).name.clone())])
        }
        Expr::Param(p) if (p.0 as usize) < flat.params.len() => {
            Expr::Name(flat.params[p.0 as usize].name.clone())
        }
        other => other,
    });
    named.to_string()
}

/// A warning.
pub fn warning(code: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic { severity: Severity::Warning, ..Diagnostic::error(code, message) }
}

/// Information.
pub fn info(code: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic { severity: Severity::Info, ..Diagnostic::error(code, message) }
}
