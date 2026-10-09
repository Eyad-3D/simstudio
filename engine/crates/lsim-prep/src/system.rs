//! The equation system as structural analysis sees it.
//!
//! Every variable and each of its time derivatives is a *node*: `x` is
//! node (x, 0), `der(x)` node (x, 1), and index reduction adds (x, 2) and
//! so on, or the derivatives of variables that had none. Equations are
//! residuals (`lhs - rhs`) whose references are nodes, written
//! `Expr::Var(VarId(node))`; `Expr::Pre(VarId(node))` is the value before
//! an event of a discrete node. Once the states are chosen, every node is
//! mapped back to a slot of the prepared model ([`Sys::slots`]).

use crate::symbolic::{simplify, time_derivative};
use lsim_ir::expr::Expr;
use lsim_ir::flat::{FlatSystem, FlatVar, VarId, VarRole};
use lsim_ir::prepared::Slot;
use lsim_ir::units::Unit;
use lsim_ir::{ParamId, VarKind};

/// What a node is to the solver.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NodeKind {
    /// to be computed
    Unknown,
    /// a discrete variable: known between events, derivative zero
    Discrete,
    /// set from outside (an inverse model's prescribed trajectory and its
    /// derivatives)
    Input,
}

/// A variable or one of its time derivatives.
#[derive(Clone, Debug)]
pub struct Node {
    /// the flat variable it is, or is a derivative of
    pub var: VarId,
    /// how many times differentiated
    pub order: u32,
    /// unknown, discrete or input
    pub kind: NodeKind,
    /// the node of its derivative, once there is one
    pub deriv: Option<usize>,
    /// the node it is the derivative of
    pub integral: Option<usize>,
}

/// An equation of the system: `res = 0`.
#[derive(Clone, Debug)]
pub struct SysEq {
    /// the residual, over nodes
    pub res: Expr,
    /// the flat equation it is, or is a derivative of (its origin is that
    /// equation's)
    pub src: usize,
    /// the equation this one is the time derivative of
    pub diff_of: Option<usize>,
    /// this equation's time derivative, once made
    pub derived: Option<usize>,
    /// the nodes it refers to, increasing
    pub inc: Vec<usize>,
}

/// The system.
#[derive(Clone, Debug, Default)]
pub struct Sys {
    /// the nodes
    pub nodes: Vec<Node>,
    /// flat variable → its node (None: eliminated as an alias)
    pub base: Vec<Option<usize>>,
    /// the equations
    pub eqs: Vec<SysEq>,
}

/// The nodes an expression refers to (increasing, no repeats).
pub fn nodes_of(e: &Expr) -> Vec<usize> {
    let mut out = vec![];
    crate::walk::visit(e, &mut |x| {
        if let Expr::Var(v) | Expr::Pre(v) = x {
            out.push(v.0 as usize);
        }
    });
    out.sort_unstable();
    out.dedup();
    out
}

/// A node reference.
pub fn node(n: usize) -> Expr {
    Expr::Var(VarId(n as u32))
}

impl Sys {
    /// A system over the flat variables that are not eliminated; discrete
    /// variables become discrete nodes, those marked in `input` inputs.
    pub fn new(flat: &FlatSystem, eliminated: &[bool], input: &[bool]) -> Sys {
        let mut sys = Sys { base: vec![None; flat.vars.len()], ..Default::default() };
        for (i, v) in flat.vars.iter().enumerate() {
            if eliminated[i] {
                continue;
            }
            let kind = if v.kind == VarKind::Discrete {
                NodeKind::Discrete
            } else if input[i] {
                NodeKind::Input
            } else {
                NodeKind::Unknown
            };
            sys.base[i] = Some(sys.nodes.len());
            sys.nodes.push(Node {
                var: VarId(i as u32),
                order: 0,
                kind,
                deriv: None,
                integral: None,
            });
        }
        sys
    }

    /// The derivative node of `n`, made when missing.
    pub fn deriv_node(&mut self, n: usize) -> usize {
        if let Some(d) = self.nodes[n].deriv {
            return d;
        }
        let d = self.nodes.len();
        let parent = &self.nodes[n];
        self.nodes.push(Node {
            var: parent.var,
            order: parent.order + 1,
            kind: parent.kind,
            deriv: None,
            integral: Some(n),
        });
        self.nodes[n].deriv = Some(d);
        d
    }

    /// A flat-scope expression over nodes: `Var(v)` → node (v, 0),
    /// `Der(v)` → node (v, 1), `Pre(v)` → `Pre` of node (v, 0).
    pub fn from_flat(&mut self, e: &Expr) -> Expr {
        crate::walk::map_up(e, &mut |x| match x {
            Expr::Var(v) => match self.base[v.0 as usize] {
                Some(n) => node(n),
                None => Expr::Var(v),
            },
            Expr::Pre(v) => match self.base[v.0 as usize] {
                Some(n) => Expr::Pre(VarId(n as u32)),
                None => Expr::Pre(v),
            },
            Expr::Der(v) => match self.base[v.0 as usize] {
                Some(n) if self.nodes[n].kind == NodeKind::Discrete => Expr::Const(0.0),
                Some(n) => node(self.deriv_node(n)),
                None => Expr::Der(v),
            },
            other => other,
        })
    }

    /// [`Sys::from_flat`] on an owned expression, changing it in place.
    pub fn from_flat_owned(&mut self, mut e: Expr) -> Expr {
        crate::walk::mutate(&mut e, &mut |x| match *x {
            Expr::Var(v) => {
                if let Some(n) = self.base[v.0 as usize] {
                    *x = node(n);
                }
            }
            Expr::Pre(v) => {
                if let Some(n) = self.base[v.0 as usize] {
                    *x = Expr::Pre(VarId(n as u32));
                }
            }
            Expr::Der(v) => match self.base[v.0 as usize] {
                Some(n) if self.nodes[n].kind == NodeKind::Discrete => *x = Expr::Const(0.0),
                Some(n) => *x = node(self.deriv_node(n)),
                None => {}
            },
            _ => {}
        });
        e
    }

    /// [`Sys::from_flat`] without making nodes: `Err(v)` when it reads the
    /// derivative of a variable `v` that has none in the system.
    pub fn from_flat_fixed(&self, e: &Expr) -> Result<Expr, VarId> {
        let mut bad = None;
        let out = crate::walk::map_up(e, &mut |x| match x {
            Expr::Var(v) => match self.base[v.0 as usize] {
                Some(n) => node(n),
                None => Expr::Var(v),
            },
            Expr::Pre(v) => match self.base[v.0 as usize] {
                Some(n) => Expr::Pre(VarId(n as u32)),
                None => Expr::Pre(v),
            },
            Expr::Der(v) => match self.base[v.0 as usize] {
                Some(n) if self.nodes[n].kind == NodeKind::Discrete => Expr::Const(0.0),
                Some(n) => match self.nodes[n].deriv {
                    Some(d) => node(d),
                    None => {
                        bad = Some(v);
                        Expr::Const(0.0)
                    }
                },
                None => {
                    bad = Some(v);
                    Expr::Const(0.0)
                }
            },
            other => other,
        });
        match bad {
            Some(v) => Err(v),
            None => Ok(out),
        }
    }

    /// Adds `res = 0`.
    pub fn push(&mut self, res: Expr, src: usize, diff_of: Option<usize>) -> usize {
        let inc = nodes_of(&res);
        self.eqs.push(SysEq { res, src, diff_of, derived: None, inc });
        self.eqs.len() - 1
    }

    /// The time derivative of an expression over nodes; makes the
    /// derivative nodes it needs.
    pub fn time_derivative(&mut self, e: &Expr) -> Result<Expr, String> {
        let mut dvar = |v: VarId| {
            let n = v.0 as usize;
            if self.nodes[n].kind == NodeKind::Discrete {
                Expr::Const(0.0)
            } else {
                node(self.deriv_node(n))
            }
        };
        time_derivative(e, &mut dvar).map(simplify)
    }

    /// Differentiates equation `e` in time (once); returns the new
    /// equation, or the one made before.
    pub fn differentiate(&mut self, e: usize) -> Result<usize, String> {
        if let Some(d) = self.eqs[e].derived {
            return Ok(d);
        }
        let res = self.eqs[e].res.clone();
        let d = self.time_derivative(&res)?;
        let src = self.eqs[e].src;
        let k = self.push(d, src, Some(e));
        self.eqs[e].derived = Some(k);
        Ok(k)
    }

    /// Whether a node is an unknown with no derivative in the system.
    pub fn highest_unknown(&self, n: usize) -> bool {
        let nd = &self.nodes[n];
        nd.kind == NodeKind::Unknown && nd.deriv.is_none()
    }

    /// How many times an equation was differentiated to make it.
    pub fn eq_level(&self, mut e: usize) -> u32 {
        let mut k = 0;
        while let Some(p) = self.eqs[e].diff_of {
            e = p;
            k += 1;
        }
        k
    }

    /// The node's name as people read it: `inertia.w`, `der(inertia.w)`,
    /// `der(der(pendulum.x))`.
    pub fn name(&self, flat: &FlatSystem, n: usize) -> String {
        let nd = &self.nodes[n];
        let mut s = flat.var(nd.var).name.clone();
        for _ in 0..nd.order {
            s = format!("der({s})");
        }
        s
    }

    /// A node-space expression with names, for messages.
    pub fn pretty(&self, flat: &FlatSystem, e: &Expr) -> String {
        let named = crate::walk::map_up(e, &mut |x| match x {
            Expr::Var(v) if (v.0 as usize) < self.nodes.len() => {
                Expr::Name(self.name(flat, v.0 as usize))
            }
            Expr::Param(p) if (p.0 as usize) < flat.params.len() => {
                Expr::Name(flat.params[p.0 as usize].name.clone())
            }
            other => other,
        });
        named.to_string()
    }
}

/// How the nodes map onto the prepared model, once the states are chosen.
pub struct SlotMap {
    /// each node's slot (discrete and input nodes: `Var` of their flat
    /// variable)
    pub slot: Vec<Slot>,
    /// the states' flat variables, in order
    pub states: Vec<VarId>,
    /// for each state whose derivative is itself a state: `der(x) := y`
    pub chained: Vec<(VarId, VarId)>,
}

impl Sys {
    /// Maps nodes to slots: a state node is a flat variable (made for a
    /// derivative order ≥ 1), the derivative of a state is that state's
    /// `Der` slot unless it is a state itself, and any other derivative
    /// node becomes a flat variable named `der(…)` (dummy derivatives and
    /// the derivatives of prescribed inputs). New variables are appended to
    /// `flat.vars`.
    pub fn slots(&self, flat: &mut FlatSystem, is_state: &[bool]) -> SlotMap {
        let n = self.nodes.len();
        let mut var_of: Vec<Option<VarId>> = vec![None; n];
        let mut slot = vec![Slot::Var(VarId(0)); n];
        // nodes in order of increasing derivative order, so integrals first
        let mut order: Vec<usize> = (0..n).collect();
        order.sort_by_key(|&i| (self.nodes[i].order, i));
        for &i in &order {
            let nd = &self.nodes[i];
            if nd.order == 0 {
                var_of[i] = Some(nd.var);
                slot[i] = Slot::Var(nd.var);
                continue;
            }
            let p = nd.integral.expect("a derivative node has an integral");
            let own_var = is_state[i] || !is_state[p] || nd.kind != NodeKind::Unknown;
            if own_var {
                let base = flat.var(nd.var).clone();
                let mut dim = base.unit.dim;
                for _ in 0..nd.order {
                    dim = dim / lsim_ir::Dim::TIME;
                }
                let rec = FlatVar {
                    name: self.name(flat, i),
                    unit: Unit::new(dim, 1.0),
                    unit_text: dim.to_string(),
                    kind: VarKind::Continuous,
                    start: None,
                    fixed: false,
                    nominal: base.nominal,
                    instance: base.instance,
                    role: VarRole::Local,
                };
                flat.vars.push(rec);
                let v = VarId(flat.vars.len() as u32 - 1);
                var_of[i] = Some(v);
                slot[i] = Slot::Var(v);
            } else {
                let pv = var_of[p].expect("a state is a variable");
                slot[i] = Slot::Der(pv);
            }
        }
        let mut states = vec![];
        let mut chained = vec![];
        for i in 0..n {
            if !is_state[i] {
                continue;
            }
            let v = var_of[i].expect("a state is a variable");
            states.push(v);
            let d = self.nodes[i].deriv.expect("a state has a derivative");
            if is_state[d] {
                chained.push((v, var_of[d].expect("a state is a variable")));
            }
        }
        SlotMap { slot, states, chained }
    }
}

impl SlotMap {
    /// [`SlotMap::to_flat`] on an owned expression, changing it in place.
    pub fn to_flat_owned(&self, mut e: Expr) -> Expr {
        crate::walk::mutate(&mut e, &mut |x| match *x {
            Expr::Var(v) => {
                *x = match self.slot[v.0 as usize] {
                    Slot::Var(w) => Expr::Var(w),
                    Slot::Der(w) => Expr::Der(w),
                }
            }
            Expr::Pre(v) => {
                *x = match self.slot[v.0 as usize] {
                    Slot::Var(w) | Slot::Der(w) => Expr::Pre(w),
                }
            }
            _ => {}
        });
        e
    }

    /// An expression over nodes in flat scope.
    pub fn to_flat(&self, e: &Expr) -> Expr {
        crate::walk::map_up(e, &mut |x| match x {
            Expr::Var(v) => match self.slot[v.0 as usize] {
                Slot::Var(w) => Expr::Var(w),
                Slot::Der(w) => Expr::Der(w),
            },
            Expr::Pre(v) => match self.slot[v.0 as usize] {
                Slot::Var(w) | Slot::Der(w) => Expr::Pre(w),
            },
            other => other,
        })
    }
}

/// Values of nodes and parameters, for the reference interpreter.
pub struct NodeEnv<'a> {
    /// time
    pub t: f64,
    /// a value per node
    pub vals: &'a [f64],
    /// parameter values
    pub params: &'a [f64],
}

impl lsim_ir::eval::Env for NodeEnv<'_> {
    fn time(&self) -> f64 {
        self.t
    }
    fn var(&self, v: VarId) -> f64 {
        self.vals.get(v.0 as usize).copied().unwrap_or(f64::NAN)
    }
    fn der(&self, _: VarId) -> f64 {
        f64::NAN
    }
    fn param(&self, p: ParamId) -> f64 {
        self.params[p.0 as usize]
    }
}
