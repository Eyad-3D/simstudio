//! Limits: what `limit(x, lo, hi)` does in each mode, and the flags.
//!
//! Every `limit(x, lo, hi)` in a model (a motor's torque within its
//! full-load curve, a battery's power, a brake's capacity, a tyre's grip)
//! is rewritten, before preparation, so the three values it compares are
//! variables of the model and therefore channels of every run:
//!
//! ```text
//! tau = limit(tau_dem, -tau_max, tau_max)
//!   becomes
//! __lim0_x = tau_dem;  __lim0_lo = -tau_max;  __lim0_hi = tau_max;
//! tau = limit(__lim0_x, __lim0_lo, __lim0_hi)      (full dynamic: clamps)
//! tau = __lim0_x                                   (fast mode: passes through)
//! ```
//!
//! In fast mode the value passes through, so the inverse model computes
//! what it would *take* to follow the trace, and the [`FlagTracker`] records
//! each stretch of time it was outside its band as a [`LimitFlag`]. In a
//! full dynamic run the same three channels say when the limit was hit
//! (the demand outside the band while the limit clamps), which is what the
//! fast-mode flags are checked against.
//!
//! The rewrite works on component definitions, so it is independent of how
//! a model is prepared afterwards; the new variables' names start with
//! `__lim` and the result layer hides them.

use crate::LimitFlag;
use lsim_ir::component::{ComponentDef, Equation, Library, PortKind, VarDecl, build::eq};
use lsim_ir::expr::{BinaryOp, Builtin, Expr};
use lsim_ir::flat::{FlatSystem, VarId};
use lsim_ir::units::{Dim, parse_unit};
use std::collections::HashMap;

/// What `limit(x, lo, hi)` does in the prepared model.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LimitMode {
    /// clamp `x` into `[lo, hi]` (full dynamic simulation)
    Clamp,
    /// pass `x` through and flag (fast mode)
    PassThrough,
}

/// A limit in a component definition.
#[derive(Clone, Debug, PartialEq)]
pub struct DefLimit {
    /// the definition's name
    pub def: String,
    /// its number within the definition
    pub k: usize,
    /// what the limited equation says (its label, or "equation n")
    pub label: String,
}

/// A library and top component with every limit rewritten.
#[derive(Clone, Debug)]
pub struct Instrumented {
    /// the library, rewritten
    pub lib: Library,
    /// the top component, rewritten
    pub top: ComponentDef,
    /// every limit found
    pub limits: Vec<DefLimit>,
}

/// A limit of a flattened model: the variables (indices into the model's
/// variables, which is also the order of `ModelFunctions::vars`) holding
/// its value and band.
#[derive(Clone, Debug, PartialEq)]
pub struct LimitSite {
    /// what it is, naming the part: `'E-Motor': the torque follows the demand within its limits`
    pub label: String,
    /// the part on the user's diagram (an instance path), for highlighting
    pub part: String,
    /// the limited value (the demand)
    pub demand: usize,
    /// the lower bound
    pub lower: usize,
    /// the upper bound
    pub upper: usize,
    /// the limited value's unit
    pub unit: String,
}

const PREFIX: &str = "__lim";

fn var_names(k: usize) -> [String; 3] {
    [format!("{PREFIX}{k}_x"), format!("{PREFIX}{k}_lo"), format!("{PREFIX}{k}_hi")]
}

/// Rewrites every `limit` of `top` and of every definition in `lib`.
pub fn instrument(lib: &Library, top: &ComponentDef, mode: LimitMode) -> Instrumented {
    let mut limits = vec![];
    let mut out = lib.clone();
    for def in out.components.values_mut() {
        rewrite_def(lib, def, mode, &mut limits);
    }
    let mut top = top.clone();
    rewrite_def(lib, &mut top, mode, &mut limits);
    Instrumented { lib: out, top, limits }
}

fn has_limit(e: &Expr) -> bool {
    e.any(&mut |x| matches!(x, Expr::Call(Builtin::Limit, _)))
}

fn rewrite_def(lib: &Library, def: &mut ComponentDef, mode: LimitMode, out: &mut Vec<DefLimit>) {
    if !def.equations.iter().any(|e| match &e.eq {
        Equation::Eq { lhs, rhs } => has_limit(lhs) || has_limit(rhs),
        _ => false,
    }) {
        return;
    }
    let original = def.clone();
    let mut k = 0usize;
    let mut new_vars: Vec<VarDecl> = vec![];
    let mut new_eqs = vec![];
    for (i, decl) in def.equations.iter_mut().enumerate() {
        let Equation::Eq { lhs, rhs } = &mut decl.eq else { continue };
        let label = decl.label.clone().unwrap_or_else(|| format!("equation {}", i + 1));
        let side_dim = expr_dim(lib, &original, lhs).or_else(|| expr_dim(lib, &original, rhs));
        for side in [lhs, rhs] {
            if !has_limit(side) {
                continue;
            }
            let e = std::mem::replace(side, Expr::Const(0.0));
            *side = e.rewrite(&mut |x| match x {
                Expr::Call(Builtin::Limit, args) if args.len() == 3 => {
                    let [xn, lon, hin] = var_names(k);
                    let unit = limit_unit(lib, &original, &args, side_dim);
                    for (name, what) in
                        [(&xn, "value"), (&lon, "lower bound"), (&hin, "upper bound")]
                    {
                        new_vars.push(VarDecl {
                            name: name.clone(),
                            unit: unit.clone(),
                            display_unit: None,
                            kind: lsim_ir::VarKind::Continuous,
                            start: None,
                            fixed: false,
                            nominal: None,
                            doc: format!("the {what} of a limit in {label}"),
                        });
                    }
                    let mut args = args.into_iter();
                    let (xe, loe, hie) = (args.next(), args.next(), args.next());
                    for (name, value, what) in [
                        (&xn, xe, "the value it limits"),
                        (&lon, loe, "its lower bound"),
                        (&hin, hie, "its upper bound"),
                    ] {
                        new_eqs.push(eq(
                            Expr::Name(name.clone()),
                            value.expect("limit has three arguments"),
                            &format!("{label} ({what})"),
                        ));
                    }
                    out.push(DefLimit { def: def_name_of(&original), k, label: label.clone() });
                    k += 1;
                    match mode {
                        LimitMode::PassThrough => Expr::Name(xn),
                        LimitMode::Clamp => Expr::Call(
                            Builtin::Limit,
                            vec![Expr::Name(xn), Expr::Name(lon), Expr::Name(hin)],
                        ),
                    }
                }
                other => other,
            });
        }
    }
    def.vars.extend(new_vars);
    def.equations.extend(new_eqs);
}

fn def_name_of(def: &ComponentDef) -> String {
    def.name.clone()
}

/// The unit text for a limit's three variables.
fn limit_unit(lib: &Library, def: &ComponentDef, args: &[Expr], fallback: Option<Dim>) -> String {
    // a plain name: its own unit text, as written
    for a in args {
        if let Expr::Name(n) = a
            && let Some(text) = name_unit_text(lib, def, n)
        {
            return text;
        }
    }
    let dim = args.iter().find_map(|a| expr_dim(lib, def, a)).or(fallback);
    dim.map(unit_text).unwrap_or_else(|| "1".into())
}

/// A readable unit text for a dimension (a named SI unit where there is
/// one, else its base units).
pub fn unit_text(d: Dim) -> String {
    const NAMED: [&str; 16] =
        ["1", "m", "kg", "s", "A", "K", "N", "N.m", "W", "V", "Ohm", "F", "H", "J", "m/s", "m/s2"];
    for n in NAMED {
        if parse_unit(n).map(|u| u.dim) == Ok(d) {
            return n.into();
        }
    }
    d.to_string()
}

fn name_unit_text(lib: &Library, def: &ComponentDef, name: &str) -> Option<String> {
    if let Some(v) = def.vars.iter().find(|v| v.name == name) {
        return Some(v.unit.clone());
    }
    if let Some(p) = def.params.iter().find(|p| p.name == name) {
        return Some(p.unit.clone());
    }
    for port in &def.ports {
        match &port.kind {
            PortKind::Input { unit } | PortKind::Output { unit } if port.name == name => {
                return Some(unit.clone());
            }
            PortKind::Physical { connector } => {
                if let Some(q) =
                    name.strip_prefix(port.name.as_str()).and_then(|r| r.strip_prefix('.'))
                    && let Some(c) = lib.connectors.get(connector)
                {
                    if c.across.name == q {
                        return Some(c.across.unit.clone());
                    }
                    if c.through.name == q {
                        return Some(c.through.unit.clone());
                    }
                }
            }
            _ => {}
        }
    }
    let (head, rest) = name.split_once('.')?;
    let sub = def.components.iter().find(|s| s.name == head)?;
    let sdef = lib.components.get(&sub.def)?;
    name_unit_text(lib, sdef, rest)
}

/// The dimension of a component-scope expression; `None` when it adopts
/// its context's (a bare number) or cannot be told.
pub fn expr_dim(lib: &Library, def: &ComponentDef, e: &Expr) -> Option<Dim> {
    let d = |x: &Expr| expr_dim(lib, def, x);
    match e {
        Expr::Const(_) | Expr::Table { .. } => None,
        Expr::Time => Some(Dim::TIME),
        Expr::Name(n) => {
            name_unit_text(lib, def, n).and_then(|t| parse_unit(&t).ok()).map(|u| u.dim)
        }
        Expr::Var(_) | Expr::Param(_) | Expr::Der(_) | Expr::Pre(_) => None,
        Expr::Neg(a) | Expr::NoEvent(a) => d(a),
        Expr::Binary(op, a, b) => {
            let (da, db) = (d(a), d(b));
            match op {
                BinaryOp::Add | BinaryOp::Sub => da.or(db),
                BinaryOp::Mul => match (da, db) {
                    (None, None) => None,
                    (x, y) => Some(x.unwrap_or(Dim::NONE) * y.unwrap_or(Dim::NONE)),
                },
                BinaryOp::Div => match (da, db) {
                    (None, None) => None,
                    (x, y) => Some(x.unwrap_or(Dim::NONE) / y.unwrap_or(Dim::NONE)),
                },
                BinaryOp::Pow => match (da, &**b) {
                    (Some(x), Expr::Const(n)) if n.fract() == 0.0 && n.abs() < 64.0 => {
                        Some(x.powi(*n as i8))
                    }
                    (Some(x), Expr::Const(n)) if *n == 0.5 => x.root(2),
                    (Some(_), _) => None,
                    (None, _) => None,
                },
            }
        }
        Expr::Call(f, args) => match f {
            Builtin::Der => args.first().and_then(d).map(|x| x / Dim::TIME),
            Builtin::Pre | Builtin::Abs | Builtin::Min | Builtin::Max | Builtin::Limit => {
                args.iter().find_map(d)
            }
            Builtin::Sqrt => args.first().and_then(d).and_then(|x| x.root(2)),
            _ => Some(Dim::NONE),
        },
        Expr::If(_, a, b) => d(a).or_else(|| d(b)),
        Expr::Compare(..) | Expr::And(..) | Expr::Or(..) | Expr::Not(..) => Some(Dim::NONE),
    }
}

/// The limits of a flattened (or prepared) model, from the variables the
/// rewrite added.
pub fn sites(flat: &FlatSystem, limits: &[DefLimit]) -> Vec<LimitSite> {
    let by_name: HashMap<&str, VarId> =
        flat.vars.iter().enumerate().map(|(i, v)| (v.name.as_str(), VarId(i as u32))).collect();
    let mut out = vec![];
    for (i, v) in flat.vars.iter().enumerate() {
        let inst = flat.instance(v.instance);
        let local =
            v.name.strip_prefix(inst.path.as_str()).unwrap_or(&v.name).trim_start_matches('.');
        let Some(k) = local
            .strip_prefix(PREFIX)
            .and_then(|r| r.strip_suffix("_x"))
            .and_then(|r| r.parse::<usize>().ok())
        else {
            continue;
        };
        let Some(dl) = limits.iter().find(|d| d.def == inst.def && d.k == k) else { continue };
        let [_, lo, hi] = var_names(k);
        let full = |n: &str| {
            if inst.path.is_empty() { n.to_string() } else { format!("{}.{n}", inst.path) }
        };
        let (Some(lo), Some(hi)) =
            (by_name.get(full(&lo).as_str()), by_name.get(full(&hi).as_str()))
        else {
            continue;
        };
        out.push(LimitSite {
            label: format!("{}: {}", flat.instance_name(v.instance), dl.label),
            part: flat.instance(flat.top_part(v.instance)).path.clone(),
            demand: i,
            lower: lo.0 as usize,
            upper: hi.0 as usize,
            unit: v.unit_text.clone(),
        });
    }
    out
}

/// Whether a channel name is one of the rewrite's helper variables.
pub fn is_helper(name: &str) -> bool {
    name.rsplit('.').next().is_some_and(|last| last.starts_with(PREFIX))
}

#[derive(Clone, Copy, Debug, Default)]
struct Side {
    /// open stretch: its start and worst excess so far
    open: Option<(f64, f64)>,
    /// the excess at the previous point
    last: f64,
}

/// Turns a sequence of evaluated points into flags: each stretch of time a
/// limit's value is outside its band (beyond a relative tolerance), with
/// its start and end located by linear interpolation of the excess between
/// the points either side.
pub struct FlagTracker<'a> {
    sites: &'a [LimitSite],
    rtol: f64,
    sides: Vec<[Side; 2]>,
    last_t: f64,
    started: bool,
    flags: Vec<LimitFlag>,
}

fn crossing(t0: f64, e0: f64, t1: f64, e1: f64) -> f64 {
    if t1 <= t0 || !(e0.is_finite() && e1.is_finite()) || e1 == e0 {
        return t1;
    }
    (t0 + (t1 - t0) * (-e0) / (e1 - e0)).clamp(t0, t1)
}

impl<'a> FlagTracker<'a> {
    /// A tracker for `sites`; values outside a band by less than `rtol`
    /// of the band's (or the value's) magnitude do not count.
    pub fn new(sites: &'a [LimitSite], rtol: f64) -> Self {
        FlagTracker {
            sites,
            rtol,
            sides: vec![[Side::default(); 2]; sites.len()],
            last_t: f64::NEG_INFINITY,
            started: false,
            flags: vec![],
        }
    }

    /// One evaluated point; points come in time order (two at the same
    /// time are the two sides of a jump).
    pub fn point(&mut self, t: f64, vars: &[f64]) {
        for (s, site) in self.sites.iter().enumerate() {
            let (x, lo, hi) = (vars[site.demand], vars[site.lower], vars[site.upper]);
            if x.is_nan() {
                continue;
            }
            let scale =
                [x, lo, hi].iter().filter(|v| v.is_finite()).fold(0.0f64, |m, v| m.max(v.abs()));
            let tol = self.rtol * scale;
            for (k, e) in [x - hi, lo - x].into_iter().enumerate() {
                let side = &mut self.sides[s][k];
                let outside = e > tol;
                match (outside, side.open) {
                    (true, None) => {
                        let t0 =
                            if self.started { crossing(self.last_t, side.last, t, e) } else { t };
                        side.open = Some((t0, e));
                    }
                    (true, Some((t0, w))) => side.open = Some((t0, w.max(e))),
                    (false, Some((t0, w))) => {
                        let t1 = crossing(self.last_t, side.last, t, e);
                        self.flags.push(flag(site, k == 0, t0, t1, w));
                        side.open = None;
                    }
                    (false, None) => {}
                }
                side.last = e;
            }
        }
        self.last_t = t;
        self.started = true;
    }

    /// The flags, every open one closed at `t_end`, in order of start.
    pub fn finish(mut self, t_end: f64) -> Vec<LimitFlag> {
        for (s, site) in self.sites.iter().enumerate() {
            for k in 0..2 {
                if let Some((t0, w)) = self.sides[s][k].open {
                    self.flags.push(flag(site, k == 0, t0, t_end, w));
                }
            }
        }
        self.flags.sort_by(|a, b| a.t_start.total_cmp(&b.t_start));
        self.flags
    }
}

fn flag(site: &LimitSite, upper: bool, t0: f64, t1: f64, worst: f64) -> LimitFlag {
    LimitFlag {
        t_start: t0,
        t_end: t1,
        label: format!(
            "{} — {}",
            site.label,
            if upper { "above its upper limit" } else { "below its lower limit" }
        ),
        part: site.part.clone(),
        upper,
        worst_excess: worst,
        unit: site.unit.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsim_ir::component::build::{param, port, var};
    use lsim_ir::expr::{c, call, name as n};

    fn limited() -> (Library, ComponentDef) {
        let mut lib = Library::default();
        lib.add_connector(lsim_ir::ConnectorDef {
            name: "Flange".into(),
            across: lsim_ir::QuantityDecl { name: "w".into(), unit: "rad/s".into() },
            through: lsim_ir::QuantityDecl { name: "tau".into(), unit: "N.m".into() },
            power: lsim_ir::PowerRule::AcrossTimesThrough,
            doc: String::new(),
        });
        let mut m = ComponentDef {
            name: "M".into(),
            ports: vec![port("flange", "Flange", "")],
            params: vec![param("tmax", "N.m", 1.0, ""), param("pmax", "W", 1.0, "")],
            vars: vec![var("tau", "N.m", "")],
            ..Default::default()
        };
        m.ports.push(lsim_ir::PortDecl {
            name: "dem".into(),
            kind: PortKind::Input { unit: "N.m".into() },
            doc: String::new(),
        });
        m.equations.push(eq(
            n("tau"),
            call(Builtin::Limit, vec![n("dem"), -n("tmax"), n("pmax") / n("flange.w")]),
            "the torque follows the demand within its limits",
        ));
        m.equations.push(eq(n("flange.tau"), -n("tau"), "it drives the flange"));
        lib.add(m);
        let top = ComponentDef { name: "T".into(), ..Default::default() };
        (lib, top)
    }

    #[test]
    fn rewrites_limits_with_units() {
        let (lib, top) = limited();
        let ins = instrument(&lib, &top, LimitMode::PassThrough);
        let m = &ins.lib.components["M"];
        assert_eq!(ins.limits.len(), 1);
        assert_eq!(ins.limits[0].label, "the torque follows the demand within its limits");
        let names: Vec<_> = m.vars.iter().map(|v| (v.name.as_str(), v.unit.as_str())).collect();
        assert!(names.contains(&("__lim0_x", "N.m")), "{names:?}");
        assert!(names.contains(&("__lim0_hi", "N.m")), "{names:?}");
        match &m.equations[0].eq {
            Equation::Eq { rhs, .. } => assert_eq!(rhs, &n("__lim0_x")),
            _ => panic!(),
        }
        assert_eq!(m.equations.len(), 5);
        let clamp = instrument(&lib, &top, LimitMode::Clamp);
        match &clamp.lib.components["M"].equations[0].eq {
            Equation::Eq { rhs, .. } => {
                assert_eq!(rhs.to_string(), "limit(__lim0_x, __lim0_lo, __lim0_hi)")
            }
            _ => panic!(),
        }
        // the power over speed has the unit of a torque
        let d = expr_dim(&lib, &lib.components["M"], &(n("pmax") / n("flange.w"))).unwrap();
        assert_eq!(d, parse_unit("N.m").unwrap().dim);
        assert_eq!(expr_dim(&lib, &lib.components["M"], &c(2.0)), None);
        assert!(is_helper("motor.__lim0_x"));
        assert!(!is_helper("motor.tau"));
    }

    #[test]
    fn tracks_stretches_outside_the_band() {
        let site = LimitSite {
            label: "'Motor': torque".into(),
            part: "motor".into(),
            demand: 0,
            lower: 1,
            upper: 2,
            unit: "N.m".into(),
        };
        let sites = [site];
        let mut tr = FlagTracker::new(&sites, 1e-9);
        // x = t - 1 on [0, 4], band [-0.5, 2]: above from t = 3, below until t = 0.5
        for k in 0..=8 {
            let t = k as f64 * 0.5;
            tr.point(t, &[t - 1.0, -0.5, 2.0]);
        }
        let flags = tr.finish(4.0);
        assert_eq!(flags.len(), 2, "{flags:?}");
        assert!(!flags[0].upper && flags[0].t_start == 0.0);
        assert!((flags[0].t_end - 0.5).abs() < 1e-12, "{flags:?}");
        assert!(flags[1].upper && (flags[1].t_start - 3.0).abs() < 1e-12);
        assert_eq!(flags[1].t_end, 4.0);
        assert!((flags[1].worst_excess - 1.0).abs() < 1e-12);
        assert!(flags[1].label.ends_with("above its upper limit"));
    }
}
