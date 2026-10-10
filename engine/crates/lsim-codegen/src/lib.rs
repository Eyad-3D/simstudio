//! # lsim-codegen: prepared model → machine code
//!
//! Each [`PreparedModel`] is compiled with Cranelift into straight-line
//! machine code (an `if` becomes a `select`; there are no branches):
//!
//! | function | computes |
//! |---|---|
//! | `residual` | `[x'; g]`: the state derivatives, then the residuals |
//! | `jacobian_sparse` | every structural non-zero of `∂[x'; g]/∂y`, column-compressed, in one forward sweep over the colours of a column colouring (dual numbers: exact) |
//! | `jvp` | `(∂[x'; g]/∂y)·v`, from the coloured Jacobian (exact) |
//! | `roots` | the zero-crossing functions |
//! | `vars` | every flat variable, aliases included (the recorded channels) |
//! | `when` | the discrete variables after the fired `when` clauses |
//! | `modes` | the `if` relations' Booleans, re-evaluated at events |
//! | `table_guards` | how far inside its data each table read is (for the run loop's outside-the-data handling) |
//! | initialisation | the initialisation problem's residuals, Jacobian and the start vector it gives ([`lsim_ir::InitFunctions`]) |
//!
//! The functions a run calls only a handful of times (the
//! initialisation's) are not compiled: the same lowering records them on
//! a tape that a tight loop interprets (`tape.rs`), bitwise what their
//! machine code would compute, at no compile time.
//!
//! Parameters, discrete variables, inputs and table data are read from
//! memory at every call, so changing them never recompiles. Transcendental
//! functions call the Rust standard library (the reference interpreter's
//! own functions); tables call the [`tables`] runtime (monotone cubic,
//! C¹). The functions the run loop compares with the interpreter (zero
//! crossings, modes, `when` clauses, table guards) are bitwise
//! `lsim_ir::eval`; the residual, the Jacobian and the channels may
//! multiply a constant integer power out (within one rounding of the
//! exact power).
//!
//! Large models compile in bounded time: each function only computes the
//! assignments its outputs need, values pass between distant assignments
//! through the caller's `work` buffer (so register allocation stays
//! linear), very large functions are split into chunks, and the chunks
//! compile on several threads (DESIGN.md, *Code generation*).

mod analysis;
mod backend;
mod emit;
mod jit;
mod lower;
pub mod tables;
mod tape;

pub use jit::MATH_SYMBOLS;

use analysis::{Ctx, Row, System, colour, contains, csc_positions, pattern_from_rows};
use cranelift_jit::JITModule;
use cranelift_module::{FuncId, Linkage, Module};
use emit::{Coloured, Env, Kind, Plan, Shape};
use lower::TanLayout;
use lsim_ir::prepared::{AliasTarget, PreparedModel, Slot};
use lsim_ir::runtime::{
    EvalInput, InitFunctions, Layout, ModelFunctions, SparsityPattern, TableGuard,
};
use lsim_ir::table::TableData;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tables::{Table, TableStore};
use tape::Tape;

/// Why compilation failed.
#[derive(Debug, thiserror::Error)]
pub enum CodegenError {
    /// The model uses something the code generator does not handle yet.
    #[error("not supported by the code generator yet: {0}")]
    Unsupported(String),
    /// Cranelift refused the generated code (an engine bug).
    #[error("code generation failed: {0}")]
    Backend(String),
    /// A table's data cannot be interpolated.
    #[error("{0}")]
    Table(String),
}

/// Options for [`compile`].
#[derive(Clone, Debug)]
pub struct CodegenOptions {
    /// Cranelift's optimisation level: `"speed"`, `"speed_and_size"`,
    /// `"none"`, or `"auto"` (the default): `"speed"` for models up to
    /// [`CodegenOptions::auto_fast_above`] expression nodes, `"none"`
    /// above, where compile time would grow past the budget for a few per
    /// cent of run time (measured: DESIGN.md, *Code generation*).
    pub opt_level: &'static str,
    /// Under `"auto"`: the model size (expression nodes in all
    /// assignments) above which compilation favours compile time.
    pub auto_fast_above: usize,
    /// The register allocator: `"backtracking"`, `"single_pass"` or
    /// `"auto"` (backtracking up to [`CodegenOptions::auto_fast_above`],
    /// single pass above).
    pub regalloc: &'static str,
    /// The largest machine function, in expression nodes: bigger ones are
    /// split into chunks chained through `work`.
    pub chunk_nodes: usize,
    /// How many expression nodes of consecutive assignments share
    /// registers before values go through `work`.
    pub segment_nodes: usize,
    /// Threads to compile on (0: as many as the machine has, at most 4).
    pub threads: usize,
    /// The initialisation's functions run on a tape (`true`, the default:
    /// a run calls them a handful of times) rather than as machine code.
    pub tape_init: bool,
    /// Jacobian-vector products get their own machine code (`false`, the
    /// default: they come from the coloured Jacobian, exact, and a run
    /// calls them rarely).
    pub compile_jvp: bool,
}

impl Default for CodegenOptions {
    fn default() -> Self {
        CodegenOptions {
            opt_level: "auto",
            auto_fast_above: 5_000,
            regalloc: "auto",
            chunk_nodes: 12_000,
            segment_nodes: 48,
            threads: 0,
            tape_init: true,
            compile_jvp: false,
        }
    }
}

/// What a compilation did, for the build report.
#[derive(Clone, Debug, Default)]
pub struct CompileReport {
    /// total time, s
    pub seconds: f64,
    /// analysis (dependencies, sparsity, colouring, plans), s
    pub analysis_seconds: f64,
    /// building Cranelift IR and tapes, s (summed over threads)
    pub ir_seconds: f64,
    /// Cranelift's compilation and linking, s (summed over threads)
    pub codegen_seconds: f64,
    /// machine functions
    pub functions: usize,
    /// operations recorded on tapes
    pub tape_ops: usize,
    /// Cranelift IR instructions compiled
    pub instructions: usize,
    /// bytes of machine code
    pub code_bytes: usize,
    /// the optimisation level used
    pub opt_level: &'static str,
    /// the register allocator used
    pub regalloc: &'static str,
    /// threads used
    pub threads: usize,
    /// expression nodes in the model's assignments
    pub nodes: usize,
    /// structural non-zeros of the Jacobian
    pub jac_nnz: usize,
    /// colours of its column colouring (forward directions per sweep)
    pub jac_colours: usize,
}

/// What a generated function reads and writes: its one argument points to
/// this (the generated code loads the fields at fixed offsets:
/// `backend.rs`, `Base`).
#[repr(C)]
struct CallCtx {
    t: f64,
    y: *const f64,
    p: *const f64,
    d: *const f64,
    u: *const f64,
    v: *const f64,
    work: *mut f64,
    out: *mut f64,
    tabs: *const *const Table,
}

type RawFn = unsafe extern "C" fn(*const CallCtx);

/// One function of the model: machine code (a machine function per
/// chunk), a tape, or nothing to compute.
#[derive(Clone, Default)]
enum Code {
    #[default]
    Empty,
    Machine(Vec<RawFn>),
    /// its registers in `work` from `regs_at` on
    Tape {
        tape: Arc<Tape>,
        regs_at: usize,
    },
}

impl Code {
    /// Runs it. `work` and `out` must have the lengths the code was made
    /// for (checked by the callers), `v` the vector it reads (`fired`, a
    /// direction) or nothing.
    fn call(
        &self,
        inp: &EvalInput<'_>,
        v: &[f64],
        work: &mut [f64],
        out: &mut [f64],
        tables: &TableStore,
    ) {
        match self {
            Code::Empty => {}
            Code::Machine(fns) => {
                let ctx = CallCtx {
                    t: inp.t,
                    y: inp.y.as_ptr(),
                    p: inp.p.as_ptr(),
                    d: inp.d.as_ptr(),
                    u: inp.u.as_ptr(),
                    v: v.as_ptr(),
                    work: work.as_mut_ptr(),
                    out: out.as_mut_ptr(),
                    tabs: tables.ptrs(),
                };
                for f in fns {
                    // SAFETY: the code reads and writes the arrays within
                    // the lengths it was generated for, which the callers
                    // check against the layout before calling.
                    unsafe { f(&ctx) }
                }
            }
            Code::Tape { tape, regs_at } => {
                let (w, regs) = work.split_at_mut(*regs_at);
                let a = tape::Arrays {
                    t: inp.t,
                    y: inp.y,
                    p: inp.p,
                    d: inp.d,
                    u: inp.u,
                    v,
                    work: w,
                    out,
                };
                tape.run(a, tables, regs);
            }
        }
    }
}

/// Where a channel's value is read after the primal code ran.
#[derive(Clone, Copy, Debug)]
enum VarSrc {
    Y(u32, bool),
    Work(u32, bool),
    D(u32, bool),
    U(u32, bool),
    Const(f64),
}

/// How the channels are computed.
#[derive(Clone)]
enum VarsCode {
    /// their own code (small models)
    Own(Code),
    /// the residual's assignments and the rest, then gathered (large models)
    Gather { code: Code, map: Vec<VarSrc> },
}

/// Where each flat variable's value is found once every assignment ran.
fn vars_map(env: &Env<'_>) -> Result<Vec<VarSrc>, CodegenError> {
    let m = env.cx.model;
    (0..m.flat.vars.len())
        .map(|i| {
            let (var, neg) = match env.alias.get(&(i as u32)) {
                Some(AliasTarget::Const(c)) => return Ok(VarSrc::Const(*c)),
                Some(AliasTarget::Var { var, negated }) => (*var, *negated),
                None => (lsim_ir::VarId(i as u32), false),
            };
            Ok(match env.main.resolve(&env.cx, var, false)? {
                analysis::Src::Y(k) => VarSrc::Y(k as u32, neg),
                analysis::Src::Work(k) => VarSrc::Work(k as u32, neg),
                analysis::Src::D(k) => VarSrc::D(k as u32, neg),
                analysis::Src::U(k) => VarSrc::U(k as u32, neg),
                analysis::Src::Const(c) => VarSrc::Const(if neg { -c } else { c }),
            })
        })
        .collect()
}

/// Owns the JIT memory; the code in it is immutable once finalised.
struct CodeMemory(#[allow(dead_code)] JITModule);

// SAFETY: after `finalize_definitions` the module's code pages are read-and-
// execute only and nothing mutates them; the module is only dropped (which
// frees them) when the last `JitModel` holding this `Arc` goes away.
unsafe impl Send for CodeMemory {}
// SAFETY: as above: shared references never mutate the module.
unsafe impl Sync for CodeMemory {}

/// A compiled model. Cloning is cheap (the machine code is shared).
#[derive(Clone)]
pub struct JitModel {
    layout: Layout,
    residual: Code,
    /// own Jacobian-vector products (`None`: from the coloured Jacobian)
    jvp: Option<Code>,
    jac: Code,
    roots: Code,
    vars: VarsCode,
    when: Code,
    modes: Code,
    guards: Code,
    pattern: SparsityPattern,
    /// where `jacobian_dense` and `jvp` keep the compressed values in
    /// `work`
    jac_scratch: usize,
    y0: Vec<f64>,
    d0: Vec<f64>,
    guard_list: Vec<TableGuard>,
    table_dims: Vec<usize>,
    tables: Arc<TableStore>,
    init: Option<JitInit>,
    /// time spent compiling, s
    pub compile_seconds: f64,
    /// bytes of machine code (all functions)
    pub code_bytes: usize,
    /// details of the compilation
    pub report: CompileReport,
    _memory: Arc<CodeMemory>,
}

/// The compiled initialisation problem.
#[derive(Clone)]
struct JitInit {
    n_w: usize,
    w0: Vec<f64>,
    layout: Layout,
    residual: Code,
    jvp: Code,
    jac: Code,
    finish: Code,
    pattern: SparsityPattern,
    tables: Arc<TableStore>,
}

impl JitModel {
    #[inline]
    fn check(&self, inp: &EvalInput<'_>, work: &[f64], out: usize, want: usize) {
        let l = &self.layout;
        assert!(
            inp.y.len() == l.n_y()
                && inp.p.len() == l.n_p
                && inp.d.len() == l.n_d
                && inp.u.len() == l.n_u,
            "input vectors do not match the compiled model's layout"
        );
        assert!(work.len() >= l.n_work, "the work buffer is shorter than Layout::n_work");
        assert_eq!(out, want, "output vector has the wrong length");
    }

    /// The Jacobian's non-zeros, column-compressed in
    /// [`JitModel::pattern`]'s order: one forward sweep with a direction
    /// per colour, exact.
    pub fn jacobian_sparse(&self, inp: &EvalInput<'_>, work: &mut [f64], values: &mut [f64]) {
        self.check(inp, work, values.len(), self.pattern.nnz());
        self.jac.call(inp, &[], &mut work[..self.jac_scratch], values, &self.tables);
    }

    /// Evaluates the Jacobian's values into the end of `work` (checked
    /// by the caller) and returns them.
    fn jac_into_work<'w>(&self, inp: &EvalInput<'_>, work: &'w mut [f64]) -> &'w [f64] {
        let nnz = self.pattern.nnz();
        let base = self.jac_scratch;
        let (w, vals) = work.split_at_mut(base);
        self.jac.call(inp, &[], w, &mut vals[..nnz], &self.tables);
        &vals[..nnz]
    }

    /// The structure [`JitModel::jacobian_sparse`] fills.
    pub fn pattern(&self) -> &SparsityPattern {
        &self.pattern
    }

    /// The same machine code with other table data (tables are runtime
    /// parameters: no recompilation). Each table must keep its number of
    /// axes.
    pub fn with_tables(&self, data: &[TableData]) -> Result<JitModel, CodegenError> {
        if data.len() != self.table_dims.len() {
            return Err(CodegenError::Table(format!(
                "{} tables given, the model has {}",
                data.len(),
                self.table_dims.len()
            )));
        }
        for (k, (d, &dims)) in data.iter().zip(&self.table_dims).enumerate() {
            if d.dims() != dims {
                return Err(CodegenError::Table(format!(
                    "table {k} had {dims} axes, the new data have {}",
                    d.dims()
                )));
            }
        }
        let names: Vec<String> = (0..data.len()).map(|k| format!("{k}")).collect();
        let refs: Vec<&TableData> = data.iter().collect();
        let names: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
        let store = Arc::new(TableStore::new(&refs, &names).map_err(CodegenError::Table)?);
        let mut m = self.clone();
        m.tables = store.clone();
        if let Some(i) = &mut m.init {
            i.tables = store;
        }
        Ok(m)
    }

    /// A table as the generated code evaluates it.
    pub fn table(&self, k: usize) -> &Table {
        self.tables.get(k)
    }
}

impl ModelFunctions for JitModel {
    fn layout(&self) -> &Layout {
        &self.layout
    }

    fn residual(&self, inp: &EvalInput<'_>, work: &mut [f64], out: &mut [f64]) {
        self.check(inp, work, out.len(), self.layout.n_y());
        self.residual.call(inp, &[], work, out, &self.tables);
    }

    fn jvp(&self, inp: &EvalInput<'_>, v: &[f64], work: &mut [f64], out: &mut [f64]) {
        self.check(inp, work, out.len(), self.layout.n_y());
        assert_eq!(v.len(), self.layout.n_y());
        match &self.jvp {
            Some(code) => code.call(inp, v, work, out, &self.tables),
            None => {
                let vals = self.jac_into_work(inp, work);
                out.fill(0.0);
                let p = &self.pattern;
                for (j, &vj) in v.iter().enumerate() {
                    if vj != 0.0 {
                        for k in p.col_ptr[j]..p.col_ptr[j + 1] {
                            out[p.row_idx[k]] += vals[k] * vj;
                        }
                    }
                }
            }
        }
    }

    fn roots(&self, inp: &EvalInput<'_>, work: &mut [f64], out: &mut [f64]) {
        self.check(inp, work, out.len(), self.layout.n_roots);
        self.roots.call(inp, &[], work, out, &self.tables);
    }

    fn vars(&self, inp: &EvalInput<'_>, work: &mut [f64], out: &mut [f64]) {
        self.check(inp, work, out.len(), self.layout.n_vars);
        match &self.vars {
            VarsCode::Own(code) => code.call(inp, &[], work, out, &self.tables),
            VarsCode::Gather { code, map } => {
                code.call(inp, &[], work, &mut [], &self.tables);
                let sign = |x: f64, neg: bool| if neg { -x } else { x };
                for (o, s) in out.iter_mut().zip(map) {
                    *o = match *s {
                        VarSrc::Y(k, n) => sign(inp.y[k as usize], n),
                        VarSrc::Work(k, n) => sign(work[k as usize], n),
                        VarSrc::D(k, n) => sign(inp.d[k as usize], n),
                        VarSrc::U(k, n) => sign(inp.u[k as usize], n),
                        VarSrc::Const(c) => c,
                    };
                }
            }
        }
    }

    fn when(&self, inp: &EvalInput<'_>, fired: &[f64], work: &mut [f64], d_out: &mut [f64]) {
        self.check(inp, work, d_out.len(), self.layout.n_d);
        assert_eq!(fired.len(), self.layout.n_whens);
        self.when.call(inp, fired, work, d_out, &self.tables);
    }

    fn start(&self, _p: &[f64], y0: &mut [f64], d0: &mut [f64]) {
        y0.copy_from_slice(&self.y0);
        d0.copy_from_slice(&self.d0);
    }

    fn jacobian_dense(&self, inp: &EvalInput<'_>, work: &mut [f64], out: &mut [f64]) {
        let n = self.layout.n_y();
        self.check(inp, work, out.len(), n * n);
        let vals = self.jac_into_work(inp, work);
        out.fill(0.0);
        let p = &self.pattern;
        for j in 0..n {
            for k in p.col_ptr[j]..p.col_ptr[j + 1] {
                out[j * n + p.row_idx[k]] = vals[k];
            }
        }
    }

    fn sparsity(&self) -> Option<&SparsityPattern> {
        Some(&self.pattern)
    }

    fn jacobian_sparse(&self, inp: &EvalInput<'_>, work: &mut [f64], values: &mut [f64]) {
        JitModel::jacobian_sparse(self, inp, work, values)
    }

    fn modes(&self, inp: &EvalInput<'_>, work: &mut [f64], d_out: &mut [f64]) {
        self.check(inp, work, d_out.len(), self.layout.n_d);
        self.modes.call(inp, &[], work, d_out, &self.tables);
    }

    fn init(&self) -> Option<&dyn InitFunctions> {
        self.init.as_ref().map(|i| i as &dyn InitFunctions)
    }

    fn table_guard_list(&self) -> &[TableGuard] {
        &self.guard_list
    }

    fn table_guards(&self, inp: &EvalInput<'_>, work: &mut [f64], out: &mut [f64]) {
        self.check(inp, work, out.len(), self.guard_list.len());
        self.guards.call(inp, &[], work, out, &self.tables);
    }

    fn eval_table(&self, k: u32, args: [f64; 2]) -> Option<(f64, [f64; 2])> {
        let k = k as usize;
        (k < self.table_dims.len()).then(|| self.tables.get(k).eval(args))
    }

    fn table_axes(&self, k: u32) -> Option<[Vec<f64>; 2]> {
        let k = k as usize;
        (k < self.table_dims.len()).then(|| {
            let t = self.tables.get(k);
            let second = if t.dims() == 2 { t.points(1).to_vec() } else { vec![] };
            [t.points(0).to_vec(), second]
        })
    }
}

impl JitInit {
    #[inline]
    fn check(&self, inp: &EvalInput<'_>, work: &[f64], out: usize, want: usize) {
        let l = &self.layout;
        assert!(
            inp.y.len() == self.n_w
                && inp.p.len() == l.n_p
                && inp.d.len() == l.n_d
                && inp.u.len() == l.n_u,
            "input vectors do not match the initialisation's layout"
        );
        assert!(work.len() >= l.n_work, "the work buffer is shorter than Layout::n_work");
        assert_eq!(out, want, "output vector has the wrong length");
    }
}

impl InitFunctions for JitInit {
    fn n_w(&self) -> usize {
        self.n_w
    }

    fn guess(&self, _p: &[f64], w0: &mut [f64]) {
        w0.copy_from_slice(&self.w0);
    }

    fn residual(&self, inp: &EvalInput<'_>, work: &mut [f64], out: &mut [f64]) {
        self.check(inp, work, out.len(), self.n_w);
        self.residual.call(inp, &[], work, out, &self.tables);
    }

    fn jvp(&self, inp: &EvalInput<'_>, v: &[f64], work: &mut [f64], out: &mut [f64]) {
        self.check(inp, work, out.len(), self.n_w);
        assert_eq!(v.len(), self.n_w);
        self.jvp.call(inp, v, work, out, &self.tables);
    }

    fn sparsity(&self) -> &SparsityPattern {
        &self.pattern
    }

    fn jacobian_sparse(&self, inp: &EvalInput<'_>, work: &mut [f64], values: &mut [f64]) {
        self.check(inp, work, values.len(), self.pattern.nnz());
        self.jac.call(inp, &[], work, values, &self.tables);
    }

    fn finish(&self, inp: &EvalInput<'_>, work: &mut [f64], y0: &mut [f64]) {
        self.check(inp, work, y0.len(), self.layout.n_y());
        self.finish.call(inp, &[], work, y0, &self.tables);
    }
}

fn coloured(pattern: SparsityPattern) -> Coloured {
    let (colour, n_colours) = colour(&pattern);
    let pos = csc_positions(&pattern, &colour);
    Coloured { pattern, colour, n_colours, pos }
}

/// Compiles `model` to machine code.
pub fn compile(model: &PreparedModel, opts: &CodegenOptions) -> Result<JitModel, CodegenError> {
    let started = Instant::now();
    let flat = &model.flat;
    let n_x = model.states.len();

    // tables (runtime data)
    let names: Vec<&str> = flat.tables.iter().map(|t| t.name.as_str()).collect();
    let data: Vec<&TableData> = flat.tables.iter().map(|t| &t.data).collect();
    let store = Arc::new(TableStore::new(&data, &names).map_err(CodegenError::Table)?);

    // the systems and their Jacobian structure
    let cx = Ctx::new(model)?;
    let mut main_slots: Vec<Slot> = model.states.iter().map(|v| Slot::Var(*v)).collect();
    main_slots.extend(model.algebraics.iter().copied());
    let n_y = main_slots.len();
    let mut rows: Vec<Row<'_>> = model.states.iter().map(|x| Row::Slot(Slot::Der(*x))).collect();
    rows.extend(model.residuals.iter().map(|r| Row::Expr(&r.expr)));
    let main = System::new(&cx, "the model", &main_slots, &model.assignments, rows, false)?;
    let own = pattern_from_rows(n_y, &main.row_deps(&cx)?);
    let given = &model.jac_pattern;
    let pattern = if given.n > 0 && given.col_ptr.len() == given.n + 1 {
        if !contains(given, &own) {
            return Err(CodegenError::Unsupported(
                "the prepared model's Jacobian pattern lacks entries its equations have".into(),
            ));
        }
        given.clone()
    } else {
        own
    };
    let main_jac = coloured(pattern);
    let (init, init_jac) = if model.init.is_empty() {
        (None, None)
    } else {
        let rows = model.init.residuals.iter().map(|r| Row::Expr(&r.expr)).collect();
        let sys = System::new(
            &cx,
            "the initialisation",
            &model.init.unknowns,
            &model.init.assignments,
            rows,
            true,
        )?;
        if sys.rows.len() != sys.n_y {
            return Err(CodegenError::Unsupported(format!(
                "the initialisation has {} unknowns and {} residuals",
                sys.n_y,
                sys.rows.len()
            )));
        }
        let p = pattern_from_rows(sys.n_y, &sys.row_deps(&cx)?);
        (Some(sys), Some(coloured(p)))
    };
    for md in &model.modes {
        if md.crossing >= model.zero_crossings.len() {
            return Err(CodegenError::Unsupported(format!(
                "a mode refers to zero crossing {}, the model has {}",
                md.crossing,
                model.zero_crossings.len()
            )));
        }
    }
    let (sites, guard_list) = analysis::table_sites(model);
    let nodes: usize =
        main.size.iter().sum::<usize>() + init.as_ref().map_or(0, |s| s.size.iter().sum::<usize>());

    // the code generator's settings for this model
    let large = nodes > opts.auto_fast_above;
    let opt_level: &'static str = match opts.opt_level {
        "auto" => {
            if large {
                "none"
            } else {
                "speed"
            }
        }
        o => o,
    };
    let regalloc: &'static str = match opts.regalloc {
        "auto" => {
            if large {
                "single_pass"
            } else {
                "backtracking"
            }
        }
        r => r,
    };
    let isa = jit::isa(opt_level, regalloc)?;
    let fma = isa.has_native_fma();
    let (mut module, decls) = jit::module(&isa)?;
    let alias: HashMap<u32, AliasTarget> =
        model.aliases.iter().map(|a| (a.var.0, a.target)).collect();
    let env = Env { cx, main, init, main_jac, init_jac, sites, decls, fma, alias, main_slots };
    let shape = Shape { chunk_nodes: opts.chunk_nodes.max(1), segment_nodes: opts.segment_nodes };
    let plans: Vec<Plan> = emit::plans(&env, shape, large, opts.compile_jvp)?;
    // where kept tangents live, per system and tangent mode
    let tan_none = TanLayout { slots: vec![], end: 0 };
    let tan_main_jvp = TanLayout::new(&env.main, env.mode(Kind::Jvp));
    let tan_main_col = TanLayout::new(&env.main, env.mode(Kind::Jac));
    let (tan_init_jvp, tan_init_col) = match &env.init {
        Some(sys) => (
            TanLayout::new(sys, env.mode(Kind::InitJvp)),
            TanLayout::new(sys, env.mode(Kind::InitJac)),
        ),
        None => (TanLayout { slots: vec![], end: 0 }, TanLayout { slots: vec![], end: 0 }),
    };
    let tan_of = |kind: Kind| -> &TanLayout {
        match kind {
            Kind::Jvp => &tan_main_jvp,
            Kind::Jac => &tan_main_col,
            Kind::InitJvp => &tan_init_jvp,
            Kind::InitJac => &tan_init_col,
            _ => &tan_none,
        }
    };
    let taped = |kind: Kind| opts.tape_init && kind.init();
    let sig = jit::eval_signature(&module);
    let trace = std::env::var_os("LSIM_CODEGEN_TRACE").is_some();
    let mut ids: Vec<Vec<FuncId>> = vec![];
    let mut jobs: Vec<(usize, usize, FuncId, usize)> = vec![];
    for (i, p) in plans.iter().enumerate() {
        let sys = env.sys(p.kind);
        let mut v = vec![];
        if !taped(p.kind) {
            for (c, r) in p.chunks.iter().enumerate() {
                let id = module
                    .declare_function(&format!("{}_{c}", p.kind.fname()), Linkage::Local, &sig)
                    .map_err(|e| CodegenError::Backend(e.to_string()))?;
                v.push(id);
                let size: usize = p.list[r.clone()].iter().map(|&k| sys.size[k]).sum();
                jobs.push((i, c, id, size));
            }
        }
        if trace {
            eprintln!(
                "lsim-codegen: {} = {} {:?}: {} assignments, {} nodes, {} kept",
                p.kind.fname(),
                if taped(p.kind) { "tape" } else { "functions" },
                v.iter().map(|i| i.as_u32()).collect::<Vec<_>>(),
                p.list.len(),
                p.nodes,
                p.keep.iter().filter(|k| **k).count()
            );
        }
        ids.push(v);
    }
    let analysis_done = Instant::now();

    // the tapes
    let mut tapes: HashMap<Kind, Arc<Tape>> = HashMap::new();
    let mut tape_ops = 0;
    for p in plans.iter().filter(|p| taped(p.kind)) {
        let t = emit::build_tape(&env, p, tan_of(p.kind))?;
        tape_ops += t.ops.len();
        tapes.insert(p.kind, Arc::new(t));
    }
    let tape_seconds = analysis_done.elapsed().as_secs_f64();

    // Cranelift IR and machine code, chunk by chunk, on several threads
    // for big models
    let threads = match opts.threads {
        0 => {
            if nodes < 1_000 {
                1
            } else {
                jit::default_threads()
            }
        }
        t => t,
    };
    let fc = isa.frontend_config();
    let n_functions = jobs.len();
    let (compiled, ir_seconds, cl_seconds) =
        jit::build_and_compile(&isa, jobs, threads, |plan, c, id| {
            let p = &plans[plan];
            emit::build_chunk(&env, p, tan_of(p.kind), c, id, &sig, fc)
        })?;
    let ir_done = Instant::now();
    let mut code_bytes = 0;
    let mut instructions = 0;
    for c in &compiled {
        code_bytes += c.bytes.len();
        instructions += c.insts;
        module
            .define_function_bytes(c.id, c.align, &c.bytes, &c.relocs)
            .map_err(|e| CodegenError::Backend(e.to_string()))?;
    }
    module.finalize_definitions().map_err(|e| CodegenError::Backend(e.to_string()))?;
    // where each plan's code keeps its values in `work`, and how much it
    // needs (a tape's registers after them)
    let need_of = |p: &Plan| -> usize {
        let base = emit::work_need(&env, p, tan_of(p.kind));
        match tapes.get(&p.kind) {
            Some(t) => base + t.regs,
            None => base,
        }
    };
    let code = |kind: Kind| -> Code {
        let Some(i) = plans.iter().position(|p| p.kind == kind) else {
            return Code::Empty;
        };
        if let Some(t) = tapes.get(&kind) {
            let regs_at = emit::work_need(&env, &plans[i], tan_of(kind));
            return Code::Tape { tape: t.clone(), regs_at };
        }
        Code::Machine(
            ids[i]
                .iter()
                .map(|id| {
                    let ptr = module.get_finalized_function(*id);
                    // SAFETY: each pointer is a function just compiled with
                    // exactly this signature (`eval_signature`).
                    unsafe { std::mem::transmute::<*const u8, RawFn>(ptr) }
                })
                .collect(),
        )
    };
    let need = plans.iter().map(need_of).max().unwrap_or(0);
    let chain = |a: Code, b: Code| -> Code {
        match (a, b) {
            (Code::Machine(mut x), Code::Machine(y)) => {
                x.extend(y);
                Code::Machine(x)
            }
            (Code::Empty, b) => b,
            (a, Code::Empty) => a,
            _ => unreachable!("a large model's functions are machine code"),
        }
    };
    let (residual, vars) = if large {
        let residual = chain(code(Kind::Residual), code(Kind::ResidualOut));
        let primal = chain(code(Kind::Residual), code(Kind::VarsRest));
        let map = vars_map(&env)?;
        (residual, VarsCode::Gather { code: primal, map })
    } else {
        (code(Kind::Residual), VarsCode::Own(code(Kind::Vars)))
    };
    let jvp = opts.compile_jvp.then(|| code(Kind::Jvp));

    // sizes and start values
    let core_work = need;
    let nnz = env.main_jac.pattern.nnz();
    let n_work = (core_work + nnz).max(1);
    let layout = Layout {
        n_x,
        n_z: model.algebraics.len(),
        n_p: flat.params.len(),
        n_d: model.discretes.len(),
        n_u: model.inputs.len(),
        n_roots: model.zero_crossings.len(),
        n_whens: model.whens.len(),
        n_vars: flat.vars.len(),
        n_work,
    };
    let start_of = |s: &Slot| match s {
        Slot::Var(v) => flat.var(*v).start.unwrap_or(0.0),
        Slot::Der(_) => 0.0,
    };
    let y0: Vec<f64> = env.main_slots.iter().map(start_of).collect();
    let d0 = model.discretes.iter().map(|v| flat.var(*v).start.unwrap_or(0.0)).collect();
    let init = match (&env.init, &env.init_jac) {
        (Some(sys), Some(col)) => Some(JitInit {
            n_w: sys.n_y,
            w0: model.init.unknowns.iter().map(start_of).collect(),
            layout,
            residual: code(Kind::InitResidual),
            jvp: code(Kind::InitJvp),
            jac: code(Kind::InitJac),
            finish: code(Kind::InitFinish),
            pattern: col.pattern.clone(),
            tables: store.clone(),
        }),
        _ => None,
    };
    let done = Instant::now();
    let report = CompileReport {
        seconds: (done - started).as_secs_f64(),
        analysis_seconds: (analysis_done - started).as_secs_f64(),
        ir_seconds: ir_seconds + tape_seconds,
        codegen_seconds: cl_seconds + (done - ir_done).as_secs_f64(),
        functions: n_functions,
        tape_ops,
        instructions,
        code_bytes,
        opt_level,
        regalloc,
        threads,
        nodes,
        jac_nnz: nnz,
        jac_colours: env.main_jac.n_colours,
    };
    Ok(JitModel {
        layout,
        residual,
        jvp,
        jac: code(Kind::Jac),
        roots: code(Kind::Roots),
        vars,
        when: code(Kind::When),
        modes: code(Kind::Modes),
        guards: code(Kind::Guards),
        pattern: env.main_jac.pattern.clone(),
        jac_scratch: core_work,
        y0,
        d0,
        guard_list,
        table_dims: env.cx.table_dims.clone(),
        tables: store,
        init,
        compile_seconds: report.seconds,
        code_bytes,
        report,
        _memory: Arc::new(CodeMemory(module)),
    })
}

/// The ISA the code is generated for: whether it has fused multiply-add
/// (exact integer powers use it).
pub fn native_fma() -> bool {
    jit::isa("speed", "backtracking").map(|i| i.has_native_fma()).unwrap_or(false)
}
