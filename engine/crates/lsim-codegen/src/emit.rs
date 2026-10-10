//! Which functions a model compiles to, what each computes, and the
//! emission of their bodies: assignments split into chunks (one machine
//! function each, chained through `work`) and segments (values reused from
//! registers), then each kind's outputs.

use crate::CodegenError;
use crate::analysis::{Ctx, Row, Src, System, TableSite};
use crate::backend::{Clif, Emit};
use crate::jit::Import;
use crate::lower::{D, Exact, Lw, LwSetup, TanLayout, TanMode};
use crate::tape::{Recorder, Tape};
use cranelift_codegen::ir::{Function, InstBuilder, Signature, UserFuncName};
use cranelift_codegen::isa::TargetFrontendConfig;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::FuncId;
use lsim_ir::prepared::{AliasTarget, Slot};
use lsim_ir::{Expr, SparsityPattern, VarId};
use std::collections::HashMap;
use std::ops::Range;

/// The functions of a compiled model.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Kind {
    Residual,
    /// a large model's residual outputs, after its chunks
    ResidualOut,
    Jvp,
    Jac,
    Roots,
    Vars,
    /// a large model's assignments the residual does not need (for the
    /// channels)
    VarsRest,
    When,
    Modes,
    Guards,
    InitResidual,
    InitJvp,
    InitJac,
    InitFinish,
}

impl Kind {
    pub(crate) fn init(self) -> bool {
        matches!(self, Kind::InitResidual | Kind::InitJvp | Kind::InitJac | Kind::InitFinish)
    }

    /// Whether its arithmetic is the interpreter's to the bit (no
    /// shortcuts): the functions the run loop compares with the
    /// interpreter.
    pub(crate) fn exact(self) -> Exact {
        match self {
            Kind::Roots | Kind::When | Kind::Modes | Kind::Guards => Exact::Interpreter,
            _ => Exact::Fast,
        }
    }

    /// Its machine functions' name.
    pub(crate) fn fname(self) -> String {
        self.name().to_string()
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Kind::Residual => "residual",
            Kind::ResidualOut => "residual_out",
            Kind::Jvp => "jvp",
            Kind::Jac => "jacobian",
            Kind::Roots => "roots",
            Kind::Vars => "vars",
            Kind::VarsRest => "vars_rest",
            Kind::When => "when",
            Kind::Modes => "modes",
            Kind::Guards => "table_guards",
            Kind::InitResidual => "init_residual",
            Kind::InitJvp => "init_jvp",
            Kind::InitJac => "init_jacobian",
            Kind::InitFinish => "init_finish",
        }
    }
}

/// A system's Jacobian structure and colouring.
pub(crate) struct Coloured {
    pub pattern: SparsityPattern,
    pub colour: Vec<u32>,
    pub n_colours: usize,
    pub pos: HashMap<(usize, u32), usize>,
}

/// Everything the emission of every function reads.
pub(crate) struct Env<'m> {
    pub cx: Ctx<'m>,
    pub main: System<'m>,
    pub init: Option<System<'m>>,
    pub main_jac: Coloured,
    pub init_jac: Option<Coloured>,
    pub sites: Vec<TableSite<'m>>,
    pub decls: HashMap<&'static str, Import>,
    pub fma: bool,
    pub alias: HashMap<u32, AliasTarget>,
    /// the model's y layout (states, then iteration variables)
    pub main_slots: Vec<Slot>,
}

impl<'m> Env<'m> {
    pub(crate) fn sys(&self, kind: Kind) -> &System<'m> {
        if kind.init() { self.init.as_ref().expect("an initialisation system") } else { &self.main }
    }

    pub(crate) fn coloured(&self, kind: Kind) -> &Coloured {
        if kind.init() {
            self.init_jac.as_ref().expect("an initialisation system")
        } else {
            &self.main_jac
        }
    }

    pub(crate) fn mode(&self, kind: Kind) -> TanMode<'_> {
        match kind {
            Kind::Jvp | Kind::InitJvp => TanMode::Jvp,
            Kind::Jac | Kind::InitJac => TanMode::Colours(&self.coloured(kind).colour),
            _ => TanMode::None,
        }
    }

    /// Whether the kind has anything to compute.
    fn has_outputs(&self, kind: Kind) -> bool {
        let m = self.cx.model;
        match kind {
            Kind::Residual | Kind::ResidualOut | Kind::Jvp | Kind::Jac => {
                !self.main.rows.is_empty()
            }
            Kind::VarsRest => !m.flat.vars.is_empty(),
            Kind::Roots => !m.zero_crossings.is_empty(),
            Kind::Vars => !m.flat.vars.is_empty(),
            Kind::When => m.whens.iter().any(|w| !w.assign.is_empty()),
            Kind::Modes => !m.modes.is_empty(),
            Kind::Guards => !self.sites.is_empty(),
            Kind::InitResidual | Kind::InitJvp | Kind::InitJac => {
                self.init.as_ref().is_some_and(|s| !s.rows.is_empty())
            }
            Kind::InitFinish => self.init.is_some() && !self.main_slots.is_empty(),
        }
    }

    /// The assignments the kind's outputs read directly.
    fn tail_refs(&self, kind: Kind) -> Result<Vec<usize>, CodegenError> {
        let m = self.cx.model;
        let sys = self.sys(kind);
        let cx = &self.cx;
        let mut out = vec![];
        let push_src = |s: Src, out: &mut Vec<usize>| {
            if let Src::Work(k) = s {
                out.push(k)
            }
        };
        match kind {
            Kind::Residual
            | Kind::ResidualOut
            | Kind::Jvp
            | Kind::Jac
            | Kind::InitResidual
            | Kind::InitJvp
            | Kind::InitJac => {
                for r in &sys.rows {
                    match r {
                        Row::Slot(s) => push_src(sys.row_src(*s)?, &mut out),
                        Row::Expr(e) => out.extend(sys.refs_of(cx, e)?),
                    }
                }
            }
            Kind::Roots => {
                for z in &m.zero_crossings {
                    out.extend(sys.refs_of(cx, &z.expr)?);
                }
            }
            Kind::Vars | Kind::VarsRest => {
                for i in 0..m.flat.vars.len() {
                    let v = match self.alias.get(&(i as u32)) {
                        Some(AliasTarget::Const(_)) => continue,
                        Some(AliasTarget::Var { var, .. }) => *var,
                        None => VarId(i as u32),
                    };
                    push_src(sys.resolve(cx, v, false)?, &mut out);
                }
            }
            Kind::When => {
                for w in &m.whens {
                    for (_, e) in &w.assign {
                        out.extend(sys.refs_of(cx, e)?);
                    }
                }
            }
            Kind::Modes => {
                for md in &m.modes {
                    out.extend(sys.refs_of(cx, &md.relation)?);
                }
            }
            Kind::Guards => {
                for s in &self.sites {
                    for a in s.args {
                        out.extend(sys.refs_of(cx, a)?);
                    }
                }
            }
            Kind::InitFinish => {
                for s in &self.main_slots {
                    if let Some(&k) = sys.target_index.get(s) {
                        out.push(k);
                    }
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        Ok(out)
    }
}

/// How a kind's function is laid out.
pub(crate) struct Plan {
    pub kind: Kind,
    /// the assignments it computes, in order
    pub list: Vec<usize>,
    /// positions of `list` per machine function
    pub chunks: Vec<Range<usize>>,
    /// segment of each position, and of the outputs (at `list.len()`)
    pub seg: Vec<u32>,
    /// per assignment of the system: whether it is written to `work`
    pub keep: Vec<bool>,
    /// whether the kind's outputs follow the last chunk's assignments
    pub outputs: bool,
    /// the expression nodes it compiles (a size measure)
    pub nodes: usize,
}

/// Sizes that shape the code.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Shape {
    pub chunk_nodes: usize,
    pub segment_nodes: usize,
}

/// Splits `list` into chunks and segments; returns them and, per
/// position (and the outputs, at the end), the segment.
fn layout(sys: &System<'_>, list: &[usize], shape: Shape) -> (Vec<Range<usize>>, Vec<u32>) {
    let n = list.len();
    let mut chunks = vec![];
    let mut seg = vec![0u32; n + 1];
    let (mut start, mut acc_chunk, mut acc_seg, mut s) = (0usize, 0usize, 0usize, 0u32);
    for q in 0..n {
        let k = list[q];
        if q > start && acc_chunk + sys.size[k] > shape.chunk_nodes {
            chunks.push(start..q);
            start = q;
            acc_chunk = 0;
            s += 1;
            acc_seg = 0;
        } else if q > 0 && (sys.calls[list[q - 1]] || acc_seg > shape.segment_nodes) {
            s += 1;
            acc_seg = 0;
        }
        seg[q] = s;
        acc_chunk += sys.size[k];
        acc_seg += sys.size[k];
    }
    chunks.push(start..n);
    if n > 0 && (sys.calls[list[n - 1]] || acc_seg > shape.segment_nodes) {
        s += 1;
    }
    seg[n] = s;
    (chunks, seg)
}

/// The values that must go through `work`: those read in another segment.
fn kept(sys: &System<'_>, list: &[usize], seg: &[u32], tail: &[usize]) -> Vec<bool> {
    let n = list.len();
    let mut pos = vec![usize::MAX; sys.exprs.len()];
    for (q, &k) in list.iter().enumerate() {
        pos[k] = q;
    }
    let mut keep = vec![false; sys.exprs.len()];
    for (q, &k) in list.iter().enumerate() {
        for &r in &sys.refs[k] {
            if seg[pos[r]] != seg[q] {
                keep[r] = true;
            }
        }
    }
    for &r in tail {
        if seg[pos[r]] != seg[n] {
            keep[r] = true;
        }
    }
    keep
}

/// Plans a kind computing its own assignments, or `None` when it
/// computes nothing.
pub(crate) fn plan_kind(
    env: &Env<'_>,
    kind: Kind,
    shape: Shape,
) -> Result<Option<Plan>, CodegenError> {
    if !env.has_outputs(kind) {
        return Ok(None);
    }
    let sys = env.sys(kind);
    let tail = env.tail_refs(kind)?;
    let list = if kind == Kind::Vars {
        (0..sys.exprs.len()).collect()
    } else {
        sys.closure(tail.iter().copied())
    };
    let (chunks, seg) = layout(sys, &list, shape);
    let keep = kept(sys, &list, &seg, &tail);
    let nodes = list.iter().map(|&k| sys.size[k]).sum();
    Ok(Some(Plan { kind, list, chunks, seg, keep, outputs: true, nodes }))
}

/// Every function of the model. A small model gets one self-contained
/// function per kind (each computing only what its outputs need: fastest
/// calls). A large one shares work to compile less: the residual's
/// assignments keep every value in `work`, and the channels add only the
/// assignments the residual does not need. Jacobian-vector products come
/// from the coloured Jacobian unless `compile_jvp`.
pub(crate) fn plans(
    env: &Env<'_>,
    shape: Shape,
    large: bool,
    compile_jvp: bool,
) -> Result<Vec<Plan>, CodegenError> {
    let mut out = vec![];
    let mut push = |p: Option<Plan>| {
        if let Some(p) = p {
            out.push(p)
        }
    };
    if !large {
        for kind in [Kind::Residual, Kind::Jac, Kind::Vars] {
            push(plan_kind(env, kind, shape)?);
        }
    } else {
        let sys = &env.main;
        let n_a = sys.exprs.len();
        let tail = env.tail_refs(Kind::Residual)?;
        let list = sys.closure(tail.iter().copied());
        let mut in_r = vec![false; n_a];
        for &k in &list {
            in_r[k] = true;
        }
        let all = vec![true; n_a];
        if env.has_outputs(Kind::Residual) {
            let (chunks, seg) = layout(sys, &list, shape);
            let nodes = list.iter().map(|&k| sys.size[k]).sum();
            push(Some(Plan {
                kind: Kind::Residual,
                list,
                chunks,
                seg,
                keep: all.clone(),
                outputs: false,
                nodes,
            }));
            push(Some(Plan {
                kind: Kind::ResidualOut,
                list: vec![],
                chunks: std::iter::once(0..0).collect(),
                seg: vec![0],
                keep: all.clone(),
                outputs: true,
                nodes: 0,
            }));
        }
        let rest: Vec<usize> = (0..n_a).filter(|&k| !in_r[k]).collect();
        if !rest.is_empty() {
            let (chunks, seg) = layout(sys, &rest, shape);
            let nodes = rest.iter().map(|&k| sys.size[k]).sum();
            push(Some(Plan {
                kind: Kind::VarsRest,
                list: rest,
                chunks,
                seg,
                keep: all,
                outputs: false,
                nodes,
            }));
        }
        push(plan_kind(env, Kind::Jac, shape)?);
    }
    if compile_jvp {
        push(plan_kind(env, Kind::Jvp, shape)?);
    }
    for kind in [
        Kind::Roots,
        Kind::When,
        Kind::Modes,
        Kind::Guards,
        Kind::InitResidual,
        Kind::InitJvp,
        Kind::InitJac,
        Kind::InitFinish,
    ] {
        push(plan_kind(env, kind, shape)?);
    }
    Ok(out)
}

/// How much of `work` a plan's code uses.
pub(crate) fn work_need(env: &Env<'_>, plan: &Plan, tan: &TanLayout) -> usize {
    if !plan.keep.iter().any(|&k| k) {
        return 0;
    }
    match env.mode(plan.kind) {
        TanMode::None => env.sys(plan.kind).exprs.len(),
        _ => tan.end,
    }
}

/// What a plan's lowering is set up with.
fn setup<'a>(env: &'a Env<'a>, plan: &'a Plan, tan: &'a TanLayout) -> LwSetup<'a> {
    LwSetup {
        cx: &env.cx,
        sys: env.sys(plan.kind),
        mode: env.mode(plan.kind),
        exact: plan.kind.exact(),
        fma: env.fma,
        keep: &plan.keep,
        tan,
    }
}

/// Lowers positions `range` of a plan's list (and its outputs when
/// `outputs`) into `lw`.
fn lower_range<E: Emit>(
    lw: &mut Lw<'_, E>,
    env: &Env<'_>,
    plan: &Plan,
    range: std::ops::Range<usize>,
    outputs_too: bool,
) -> Result<(), CodegenError> {
    let sys = env.sys(plan.kind);
    for q in range.clone() {
        if q > range.start && plan.seg[q] != plan.seg[q - 1] {
            lw.new_segment();
        }
        let k = plan.list[q];
        let d = lw.lower(sys.exprs[k])?;
        lw.define(k, d);
    }
    if outputs_too {
        let n = plan.list.len();
        if n > 0 && plan.seg[n] != plan.seg[n - 1] {
            lw.new_segment();
        }
        outputs(lw, env, plan.kind)?;
    }
    Ok(())
}

/// Builds the IR of one chunk of a plan.
pub(crate) fn build_chunk(
    env: &Env<'_>,
    plan: &Plan,
    tan: &TanLayout,
    c: usize,
    id: FuncId,
    sig: &Signature,
    fc: TargetFrontendConfig,
) -> Result<Function, CodegenError> {
    let range = plan.chunks[c].clone();
    let mut fctx = FunctionBuilderContext::new();
    let mut func = Function::with_name_signature(UserFuncName::user(0, id.as_u32()), sig.clone());
    {
        let mut b = FunctionBuilder::new(&mut func, &mut fctx);
        let block = b.create_block();
        b.append_block_params_for_function_params(block);
        b.switch_to_block(block);
        let ctx = b.block_params(block)[0];
        {
            let e = Clif::new(&mut b, ctx, &env.decls);
            let mut lw = Lw::new(e, setup(env, plan, tan));
            let last = plan.outputs && c + 1 == plan.chunks.len();
            lower_range(&mut lw, env, plan, range, last)?;
        }
        b.ins().return_(&[]);
        b.seal_all_blocks();
        b.finalize(fc);
    }
    Ok(func)
}

/// Records a whole plan (every chunk, then its outputs) on a tape.
pub(crate) fn build_tape(
    env: &Env<'_>,
    plan: &Plan,
    tan: &TanLayout,
) -> Result<Tape, CodegenError> {
    let mut lw = Lw::new(Recorder::default(), setup(env, plan, tan));
    lower_range(&mut lw, env, plan, 0..plan.list.len(), plan.outputs)?;
    Ok(lw.e.finish())
}

/// The value of an output row.
fn row<E: Emit>(
    lw: &mut Lw<'_, E>,
    sys: &System<'_>,
    r: &Row<'_>,
) -> Result<D<E::V>, CodegenError> {
    match r {
        Row::Slot(s) => {
            let src = sys.row_src(*s)?;
            lw.src(src, false)
        }
        Row::Expr(e) => lw.lower(e),
    }
}

/// The order in which the `when` clauses' assignments are applied: each
/// after every assignment of a discrete variable it reads directly (not
/// through `pre`), so that it reads that variable's new value; otherwise
/// in the order written (an assignment reading its own target, or a cycle
/// among them, reads the values assigned so far). Items are (clause,
/// position in the clause).
pub(crate) fn when_order(m: &lsim_ir::PreparedModel) -> Vec<(usize, usize)> {
    let items: Vec<(usize, usize)> = m
        .whens
        .iter()
        .enumerate()
        .flat_map(|(w, c)| (0..c.assign.len()).map(move |k| (w, k)))
        .collect();
    let n = items.len();
    let target = |i: usize| m.whens[items[i].0].assign[items[i].1].0;
    // per item: the items that must come first
    let mut before: Vec<Vec<usize>> = vec![vec![]; n];
    for i in 0..n {
        let expr = &m.whens[items[i].0].assign[items[i].1].1;
        let mut reads = vec![];
        expr.walk(&mut |x| {
            if let Expr::Var(v) = x {
                reads.push(*v);
            }
        });
        for j in 0..n {
            if j != i && target(j) != target(i) && reads.contains(&target(j)) {
                before[i].push(j);
            }
        }
    }
    // Kahn's algorithm, the earliest written first; what is left of a
    // cycle in the order written
    let mut done = vec![false; n];
    let mut order = Vec::with_capacity(n);
    while order.len() < n {
        let next = (0..n).find(|&i| !done[i] && before[i].iter().all(|&j| done[j]));
        let i = next.unwrap_or_else(|| (0..n).find(|&i| !done[i]).expect("one left"));
        done[i] = true;
        order.push(items[i]);
    }
    order
}

/// Emits the kind's outputs.
fn outputs<E: Emit>(lw: &mut Lw<'_, E>, env: &Env<'_>, kind: Kind) -> Result<(), CodegenError> {
    let m = env.cx.model;
    let sys = env.sys(kind);
    match kind {
        Kind::Residual | Kind::ResidualOut | Kind::InitResidual => {
            for (i, r) in sys.rows.iter().enumerate() {
                let d = row(lw, sys, r)?;
                lw.store_out(i, d.v);
            }
        }
        Kind::Jvp | Kind::InitJvp => {
            for (i, r) in sys.rows.iter().enumerate() {
                let d = row(lw, sys, r)?;
                let v = match d.t.first() {
                    Some(&(_, x)) => lw.tv(x),
                    None => lw.cst(0.0),
                };
                lw.store_out(i, v);
            }
        }
        Kind::Jac | Kind::InitJac => {
            let col = env.coloured(kind);
            let mut written = vec![false; col.pattern.nnz()];
            for (i, r) in sys.rows.iter().enumerate() {
                let d = row(lw, sys, r)?;
                for &(dir, x) in &d.t {
                    let Some(&k) = col.pos.get(&(i, dir)) else {
                        return Err(CodegenError::Backend(format!(
                            "internal: a derivative of row {i} lies outside the Jacobian's pattern"
                        )));
                    };
                    let v = lw.tv(x);
                    lw.store_out(k, v);
                    written[k] = true;
                }
            }
            if written.iter().any(|w| !w) {
                let zero = lw.cst(0.0);
                for (k, w) in written.iter().enumerate() {
                    if !w {
                        lw.store_out(k, zero);
                    }
                }
            }
        }
        Kind::Roots => {
            for (k, z) in m.zero_crossings.iter().enumerate() {
                let d = lw.lower(&z.expr)?;
                lw.store_out(k, d.v);
            }
        }
        Kind::VarsRest => {}
        Kind::Vars => {
            for i in 0..m.flat.vars.len() {
                let v = match env.alias.get(&(i as u32)) {
                    Some(AliasTarget::Const(c)) => lw.cst(*c),
                    Some(AliasTarget::Var { var, negated }) => {
                        let s = sys.resolve(&env.cx, *var, false)?;
                        let x = lw.src(s, false)?.v;
                        if *negated { lw.e.neg(x) } else { x }
                    }
                    None => {
                        let s = sys.resolve(&env.cx, VarId(i as u32), false)?;
                        lw.src(s, false)?.v
                    }
                };
                lw.store_out(i, v);
            }
        }
        Kind::When => {
            for (w, k) in when_order(m) {
                let (var, expr) = &m.whens[w].assign[k];
                let Some(&idx) = env.cx.d_index.get(&var.0) else {
                    return Err(CodegenError::Unsupported(format!(
                        "a when clause assigns {:?}, which is not a discrete variable",
                        m.flat.var(*var).name
                    )));
                };
                let f = lw.load_v(w);
                let cond = lw.is_true(f);
                // a discrete variable it reads directly: its new value (as
                // assigned so far, in `when_order`); `pre`: the value
                // before the event; the assignments it reads: before the
                // event
                lw.when_new = true;
                let new = lw.lower(expr)?.v;
                let old = lw.src(Src::D(idx), false)?.v;
                lw.when_new = false;
                let v = lw.e.select(cond, new, old);
                lw.store_out(idx, v);
            }
        }
        Kind::Modes => {
            for md in &m.modes {
                let Some(&idx) = env.cx.d_index.get(&md.var.0) else {
                    return Err(CodegenError::Unsupported(format!(
                        "mode variable {:?} is not a discrete variable",
                        m.flat.var(md.var).name
                    )));
                };
                let d = lw.lower(&md.relation)?;
                let c = lw.is_true(d.v);
                let v = lw.as_number(c);
                lw.store_out(idx, v);
            }
        }
        Kind::Guards => {
            let mut g = 0;
            for s in &env.sites {
                for (axis, a) in s.args.iter().enumerate() {
                    let x = lw.lower(a)?.v;
                    let v = lw.e.table_guard(s.table, axis as u8, x);
                    lw.store_out(g, v);
                    g += 1;
                }
            }
        }
        Kind::InitFinish => {
            for (i, slot) in env.main_slots.iter().enumerate() {
                let v = if let Some(&w) = sys.y_index.get(slot) {
                    lw.src(Src::Y(w), false)?.v
                } else if let Some(&k) = sys.target_index.get(slot) {
                    lw.src(Src::Work(k), false)?.v
                } else {
                    let start = match slot {
                        Slot::Var(v) => m.flat.var(*v).start.unwrap_or(0.0),
                        Slot::Der(_) => 0.0,
                    };
                    lw.cst(start)
                };
                lw.store_out(i, v);
            }
        }
    }
    Ok(())
}
