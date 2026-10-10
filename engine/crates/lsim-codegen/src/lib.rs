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
mod kernel;
mod lower;
pub mod tables;
mod tape;

pub use jit::MATH_SYMBOLS;

use analysis::{Ctx, Row, System, colour, contains, csc_positions, pattern_from_rows};
use cranelift_jit::JITModule;
use cranelift_module::{FuncId, Linkage, Module};
use emit::{Coloured, Env, Kind, Plan, Shape};
use lower::TanLayout;
use lsim_ir::fenv::DefaultFloatEnv;
use lsim_ir::prepared::{AliasTarget, PreparedModel, Slot};
use lsim_ir::runtime::{
    ConditionKernels, Enclosure, EvalInput, InitFunctions, Layout, ModelFunctions, SparsityPattern,
    TableGuard,
};
use lsim_ir::table::TableData;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
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
    /// Kernels of the conditions that read time and the integrator's
    /// variables ([`lsim_ir::ConditionKernels`]; `true`, the default).
    pub kernels: bool,
    /// Models above this many expression nodes are tiered: `compile`
    /// returns them running on tapes (built in a fraction of the time
    /// Cranelift takes) and compiles their machine code on another
    /// thread, to which each function switches once it is in. A tape
    /// computes bitwise what its machine code computes, so results do not
    /// depend on when the switch happens. `usize::MAX`: never.
    pub tiered_above: usize,
    /// For tests: the memory provider refuses to make code executable,
    /// as Windows' Arbitrary Code Guard and some security policies do
    /// (the model then runs on its tapes: [`MachineCode::Tapes`]).
    #[doc(hidden)]
    pub deny_executable_memory: bool,
    /// For tests: use the CPU's fused multiply-add where it has one (the
    /// default); `false` lowers as for a CPU without, which computes the
    /// same bits.
    #[doc(hidden)]
    pub fused_multiply_add: bool,
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
            kernels: true,
            tiered_above: 20_000,
            deny_executable_memory: false,
            fused_multiply_add: true,
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
    /// the calling convention of the generated functions and of their
    /// calls (the target's default: `system_v`, `windows_fastcall` …)
    pub call_conv: String,
    /// the model was returned on its tapes, its machine code compiling on
    /// another thread ([`CodegenOptions::tiered_above`],
    /// [`JitModel::wait_machine_code`]); the counts of machine functions,
    /// instructions and bytes above are then zero
    pub tiered: bool,
    /// of the codegen time: placing the machine code in executable memory
    /// and resolving its calls, s (what loading cached machine code would
    /// cost too)
    pub link_seconds: f64,
    /// Why the model runs on its tapes for good, if it does: the system
    /// refused to make its machine code executable (the tapes compute the
    /// same, more slowly; [`JitModel::machine_code`])
    pub on_tapes: Option<String>,
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
    /// these, one after the other (a large model's chained tapes)
    Seq(Vec<Code>),
    /// a tape until its machine code, compiled on another thread, is in:
    /// bitwise the same function either way
    Tiered(Arc<TieredCode>),
}

/// A function that starts on its tape and switches to machine code.
struct TieredCode {
    tape: Code,
    machine: OnceLock<Code>,
}

/// A tiered model's machine code, compiled on another thread.
#[derive(Default)]
struct Upgrade {
    /// the memory the machine code lives in, set before any function
    /// switches to it
    memory: OnceLock<Arc<CodeMemory>>,
    /// the compilation's outcome, once it is over
    done: Mutex<Option<Result<CompileReport, String>>>,
    over: Condvar,
}

impl Upgrade {
    fn finish(&self, r: Result<CompileReport, String>) {
        *self.done.lock().unwrap_or_else(|e| e.into_inner()) = Some(r);
        self.over.notify_all();
    }
}

/// Stops a tiered model's background compilation when the last holder of
/// the model goes away (its clones share this).
struct CancelOnDrop(Arc<AtomicBool>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// Background compilations running, and those stopped because their
/// model was dropped (for tests).
static BACKGROUND_RUNNING: AtomicUsize = AtomicUsize::new(0);
static BACKGROUND_CANCELLED: AtomicUsize = AtomicUsize::new(0);

/// `(running, cancelled so far)`: tiered models' background compilations
/// in this process (for tests).
#[doc(hidden)]
pub fn background_compiles() -> (usize, usize) {
    (BACKGROUND_RUNNING.load(Ordering::SeqCst), BACKGROUND_CANCELLED.load(Ordering::SeqCst))
}

/// How a model's functions run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MachineCode {
    /// as machine code
    Ready,
    /// on tapes, while their machine code compiles on another thread (a
    /// tiered model: [`CodegenOptions::tiered_above`])
    Compiling,
    /// on tapes for good, which compute bit for bit what the machine code
    /// would, more slowly: why (the system refused to make code
    /// executable, or the background compilation failed)
    Tapes(String),
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
            Code::Seq(codes) => {
                for c in codes {
                    c.call(inp, v, work, out, tables);
                }
            }
            Code::Tiered(t) => match t.machine.get() {
                Some(m) => m.call(inp, v, work, out, tables),
                None => t.tape.call(inp, v, work, out, tables),
            },
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

/// Owns the JIT memory; the code in it is immutable once finalised, and
/// given back when this is dropped (cranelift-jit's memory provider leaks
/// it otherwise).
///
/// Pointers to the functions in it live only next to an `Arc` of it: in
/// the `JitModel` that holds it as `_memory`, or, for a tiered model, in
/// its `TieredCode`s, whose holders hold the `Upgrade` whose `memory` is set
/// before any of them switches to machine code.
struct CodeMemory(Option<JITModule>);

impl CodeMemory {
    fn module(&self) -> &JITModule {
        self.0.as_ref().expect("the module lives until the memory is dropped")
    }
}

impl Drop for CodeMemory {
    fn drop(&mut self) {
        if let Some(m) = self.0.take() {
            // SAFETY: the last holder of the `Arc` is going away, and with
            // it every pointer into this memory (above): no function in it
            // runs or is called again. A link the system refused drops it
            // before any pointer into it was taken.
            unsafe { m.free_memory() }
        }
    }
}

// SAFETY: after `finalize_definitions` the module's code pages are read-and-
// execute only and nothing mutates them; the module is only dropped (which
// frees them) when the last holder of this `Arc` goes away.
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
    /// the interval kernels of the conditions
    kernels: Option<Arc<kernel::Kernels>>,
    /// per zero crossing: its point kernel
    points: Vec<Option<Code>>,
    /// the scratch values the point kernels need
    point_work: usize,
    /// the tables' breakpoints and 2-D grids, as the run loop builds them
    table_tools: Arc<kernel::Tools>,
    /// time spent compiling, s
    pub compile_seconds: f64,
    /// bytes of machine code (all functions)
    pub code_bytes: usize,
    /// details of the compilation
    pub report: CompileReport,
    _memory: Arc<CodeMemory>,
    /// a tiered model's machine code to come
    upgrade: Option<Arc<Upgrade>>,
    /// stops its compilation when the last clone goes away
    _cancel: Option<Arc<CancelOnDrop>>,
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

    /// The functions a tiered model switches to machine code, in a fixed
    /// order.
    fn slots_mut(&mut self) -> Vec<&mut Code> {
        let vars = match &mut self.vars {
            VarsCode::Own(c) => c,
            VarsCode::Gather { code, .. } => code,
        };
        let mut v = vec![
            &mut self.residual,
            &mut self.jac,
            &mut self.roots,
            vars,
            &mut self.when,
            &mut self.modes,
            &mut self.guards,
        ];
        if let Some(j) = &mut self.jvp {
            v.push(j);
        }
        v
    }

    /// Whether the model's functions run as machine code
    /// ([`JitModel::machine_code`] is [`MachineCode::Ready`]).
    pub fn machine_code_ready(&self) -> bool {
        self.machine_code() == MachineCode::Ready
    }

    /// How the model's functions run now: as machine code, on tapes while
    /// it compiles (tiered), or on tapes for good, and why.
    pub fn machine_code(&self) -> MachineCode {
        if let Some(why) = &self.report.on_tapes {
            return MachineCode::Tapes(why.clone());
        }
        let Some(u) = &self.upgrade else {
            return MachineCode::Ready;
        };
        match &*u.done.lock().unwrap_or_else(|e| e.into_inner()) {
            None => MachineCode::Compiling,
            Some(Ok(_)) => MachineCode::Ready,
            Some(Err(why)) => MachineCode::Tapes(why.clone()),
        }
    }

    /// A tiered model as it runs before its machine code is in: on its
    /// tapes alone (for tests and benchmarks of the tiers); `None` for a
    /// model compiled at once.
    #[doc(hidden)]
    pub fn tapes_only(&self) -> Option<JitModel> {
        self.upgrade.as_ref()?;
        let mut m = self.clone();
        for c in m.slots_mut() {
            if let Code::Tiered(t) = c {
                *c = t.tape.clone();
            }
        }
        m.upgrade = None;
        Some(m)
    }

    /// Waits for a tiered model's machine code: its compilation's report
    /// (`seconds` counted from the start of [`compile`]), or why the model
    /// stays on its tapes; `None` for a model compiled at once.
    pub fn wait_machine_code(&self) -> Option<Result<CompileReport, String>> {
        let u = self.upgrade.as_ref()?;
        let mut done = u.done.lock().unwrap_or_else(|e| e.into_inner());
        while done.is_none() {
            done = u.over.wait(done).unwrap_or_else(|e| e.into_inner());
        }
        done.clone()
    }

    /// The same machine code with other table data (tables are runtime
    /// parameters: no recompilation). Each table must keep its number of
    /// axes.
    pub fn with_tables(&self, data: &[TableData]) -> Result<JitModel, CodegenError> {
        // (the tables' coefficients in the default floating-point
        // environment, as `compile` builds them)
        let _env = DefaultFloatEnv::enter();
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
        m.table_tools = Arc::new(kernel::table_tools(&store));
        m.kernels = self.kernels.as_ref().map(|k| Arc::new(k.fresh()));
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

    fn condition_kernels(&self) -> Option<&dyn ConditionKernels> {
        self.kernels.as_ref().is_some_and(|k| k.any()).then_some(self as &dyn ConditionKernels)
    }
}

impl ConditionKernels for JitModel {
    fn covers(&self, k: usize) -> bool {
        self.kernels.as_ref().is_some_and(|ks| ks.covers(k))
            && self.points.get(k).is_some_and(|p| p.is_some())
    }

    fn scratch(&self) -> (usize, usize) {
        (self.point_work.max(1), self.kernels.as_ref().map_or(0, |k| k.scratch()))
    }

    fn point(&self, k: usize, inp: &EvalInput<'_>, work: &mut [f64]) -> f64 {
        let l = &self.layout;
        assert!(
            inp.y.len() == l.n_y()
                && inp.p.len() == l.n_p
                && inp.d.len() == l.n_d
                && inp.u.len() == l.n_u,
            "input vectors do not match the compiled model's layout"
        );
        assert!(work.len() >= self.point_work, "the point kernels' scratch is too short");
        let Some(Some(code)) = self.points.get(k) else {
            return f64::NAN;
        };
        let mut out = [f64::NAN];
        code.call(inp, &[], work, &mut out, &self.tables);
        out[0]
    }

    fn enclose(
        &self,
        k: usize,
        t: [f64; 2],
        y: &[Enclosure],
        d: &[f64],
        p: &[f64],
        u: &[f64],
        work: &mut [Enclosure],
    ) -> Enclosure {
        let l = &self.layout;
        assert!(
            y.len() == l.n_y() && p.len() == l.n_p && d.len() == l.n_d && u.len() == l.n_u,
            "input vectors do not match the compiled model's layout"
        );
        let Some(ks) = &self.kernels else {
            return Enclosure {
                v: [f64::NEG_INFINITY, f64::INFINITY],
                d: [f64::NEG_INFINITY, f64::INFINITY],
                dd: [f64::NEG_INFINITY, f64::INFINITY],
            };
        };
        let (breaks, grids) = &*self.table_tools;
        let tools = kernel::TableTools { model: self, breaks, grids };
        ks.enclose(k, t, y, d, p, u, work, &tools)
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
///
/// Where the system refuses to make the code executable (Windows'
/// Arbitrary Code Guard, some security policies), the model runs on its
/// tapes instead, which compute the same: [`CompileReport::on_tapes`]
/// says why.
pub fn compile(model: &PreparedModel, opts: &CodegenOptions) -> Result<JitModel, CodegenError> {
    // the default floating-point environment (lsim_ir::fenv): tables'
    // coefficients and constants computed here must not depend on the
    // calling thread's
    let _env = DefaultFloatEnv::enter();
    match build(model, opts, None, How::Auto)? {
        Built::Model(m) => Ok(*m),
        Built::NoExecutableMemory(why) => match build(model, opts, None, How::Tapes)? {
            Built::Model(mut m) => {
                m.report.on_tapes = Some(why);
                Ok(*m)
            }
            _ => unreachable!("tapes need no executable memory"),
        },
        Built::Foreign(_) => unreachable!("the host's code is a model"),
    }
}

/// Compiles `model` for another target, named by its triple (such as
/// `x86_64-pc-windows-msvc`), as far as machine code, which cannot run
/// here, and reports on it: for tests that the code generator is right
/// for a target the test machine is not (its calling convention above
/// all).
#[doc(hidden)]
pub fn compile_for_target(
    model: &PreparedModel,
    opts: &CodegenOptions,
    triple: &str,
) -> Result<CompileReport, CodegenError> {
    let _env = DefaultFloatEnv::enter();
    match build(model, opts, Some(triple), How::Auto)? {
        Built::Foreign(r) => Ok(r),
        _ => unreachable!("another target's code cannot run"),
    }
}

/// What [`build`] made: a model to run, another target's code's report,
/// or nothing because the system refused executable memory (why).
enum Built {
    Model(Box<JitModel>),
    Foreign(CompileReport),
    NoExecutableMemory(String),
}

/// How [`build`] compiles a model for this machine.
#[derive(Clone, Copy)]
enum How<'c> {
    /// machine code, tiered above [`CodegenOptions::tiered_above`]
    Auto,
    /// every function on its tape (no executable memory)
    Tapes,
    /// a tiered model's machine code, on its background thread: stopped
    /// when `cancel` is set
    Background(&'c AtomicBool),
}

fn build(
    model: &PreparedModel,
    opts: &CodegenOptions,
    target: Option<&str>,
    how: How<'_>,
) -> Result<Built, CodegenError> {
    let cancel = match how {
        How::Background(c) => Some(c),
        _ => None,
    };
    let stopped = || cancel.is_some_and(|c| c.load(Ordering::Relaxed));
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
    let isa = jit::isa_for(target, opt_level, regalloc)?;
    let fma = isa.has_native_fma() && opts.fused_multiply_add;
    let (mut module, decls) = jit::module(&isa, opts.deny_executable_memory)?;
    let alias: HashMap<u32, AliasTarget> =
        model.aliases.iter().map(|a| (a.var.0, a.target)).collect();
    let env = Env { cx, main, init, main_jac, init_jac, sites, decls, fma, alias, main_slots };
    let shape = Shape { chunk_nodes: opts.chunk_nodes.max(1), segment_nodes: opts.segment_nodes };
    let plans: Vec<Plan> = emit::plans(&env, shape, large, opts.compile_jvp)?;
    if stopped() {
        return Err(jit::cancelled());
    }
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
    let tiered = matches!(how, How::Auto) && target.is_none() && nodes > opts.tiered_above;
    let all_tapes = tiered || matches!(how, How::Tapes);
    let taped = |kind: Kind| all_tapes || (opts.tape_init && kind.init());
    let sig = jit::eval_signature(&*isa);
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
    // the conditions' kernels: interval programs and taped point kernels
    let (kernels, points, point_work) = if opts.kernels {
        let ks = kernel::candidates(model, &env.main, &env.cx);
        if trace {
            eprintln!("lsim-codegen: kernels for the zero crossings {ks:?}");
        }
        let built = kernel::Kernels::build(model, &env.main, &env.cx, &ks);
        let mut points: Vec<Option<Code>> = vec![None; model.zero_crossings.len()];
        let mut need = 0;
        for &k in &ks {
            if !built.covers(k as usize) {
                continue;
            }
            let Some(plan) = emit::plan_kind(&env, Kind::Point(k), shape)? else { continue };
            let t = emit::build_tape(&env, &plan, &tan_none)?;
            tape_ops += t.ops.len();
            let regs_at = emit::work_need(&env, &plan, &tan_none);
            need = need.max(regs_at + t.regs);
            points[k as usize] = Some(Code::Tape { tape: Arc::new(t), regs_at });
        }
        (Some(Arc::new(built)), points, need)
    } else {
        (None, vec![None; model.zero_crossings.len()], 0)
    };
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
        jit::build_and_compile(&isa, jobs, threads, cancel, |plan, c, id| {
            let p = &plans[plan];
            emit::build_chunk(&env, p, tan_of(p.kind), c, id, &sig, fc)
        })?;
    let ir_done = Instant::now();
    let code_bytes: usize = compiled.iter().map(|c| c.bytes.len()).sum();
    let instructions: usize = compiled.iter().map(|c| c.insts).sum();
    let report = |done: Instant, link_seconds: f64| CompileReport {
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
        jac_nnz: env.main_jac.pattern.nnz(),
        jac_colours: env.main_jac.n_colours,
        call_conv: isa.default_call_conv().to_string(),
        tiered,
        link_seconds,
        on_tapes: None,
    };
    if target.is_some() {
        return Ok(Built::Foreign(report(Instant::now(), 0.0)));
    }
    if stopped() {
        return Err(jit::cancelled());
    }
    let link_started = Instant::now();
    // from here on the memory is given back however this returns, a refused
    // link included (no pointer into it is taken before it succeeds)
    let mut memory = CodeMemory(Some(module));
    if !compiled.is_empty() {
        let module = memory.0.as_mut().expect("the module lives until the memory is dropped");
        // a system that refuses executable memory (allocating it, or
        // making it executable) leaves the model to its tapes
        for c in &compiled {
            if let Err(e) = module.define_function_bytes(c.id, c.align, &c.bytes, &c.relocs) {
                return Ok(Built::NoExecutableMemory(no_executable_memory(&e)));
            }
        }
        if let Err(e) = module.finalize_definitions() {
            return Ok(Built::NoExecutableMemory(no_executable_memory(&e)));
        }
    }
    let link_seconds = link_started.elapsed().as_secs_f64();
    let memory = Arc::new(memory);
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
                    let ptr = memory.module().get_finalized_function(*id);
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
            (a, b) => Code::Seq(vec![a, b]),
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
    let report = report(Instant::now(), link_seconds);
    let mut jm = JitModel {
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
        table_tools: Arc::new(kernel::table_tools(&store)),
        tables: store,
        init,
        kernels,
        points,
        point_work,
        compile_seconds: report.seconds,
        code_bytes,
        report,
        _memory: memory,
        upgrade: None,
        _cancel: None,
    };
    if tiered {
        upgrade(&mut jm, model, opts, started);
    }
    Ok(Built::Model(Box::new(jm)))
}

/// Puts a model returned on its tapes on the way to machine code: each of
/// its functions becomes tiered, and another thread compiles the model as
/// a whole (the same analysis, plans and work layout) and switches each to
/// its machine code.
fn upgrade(jm: &mut JitModel, model: &PreparedModel, opts: &CodegenOptions, started: Instant) {
    let up = Arc::new(Upgrade::default());
    let slots: Vec<Arc<TieredCode>> = jm
        .slots_mut()
        .into_iter()
        .map(|c| {
            let t = Arc::new(TieredCode { tape: std::mem::take(c), machine: OnceLock::new() });
            *c = Code::Tiered(t.clone());
            t
        })
        .collect();
    jm.upgrade = Some(up.clone());
    let cancel = Arc::new(AtomicBool::new(false));
    jm._cancel = Some(Arc::new(CancelOnDrop(cancel.clone())));
    let (layout, jac_scratch) = (jm.layout, jm.jac_scratch);
    let model = model.clone();
    let opts = CodegenOptions { tiered_above: usize::MAX, ..opts.clone() };
    let up2 = up.clone();
    BACKGROUND_RUNNING.fetch_add(1, Ordering::SeqCst);
    let spawned = std::thread::Builder::new().name("lsim-codegen".into()).spawn(move || {
        let _env = DefaultFloatEnv::enter();
        let r = match build(&model, &opts, None, How::Background(&cancel)) {
            Ok(Built::Model(mut m)) => {
                let fits = m.layout.n_work <= layout.n_work && m.jac_scratch <= jac_scratch;
                let (memory, mut report) = (m._memory.clone(), m.report.clone());
                let codes = m.slots_mut();
                if codes.len() != slots.len() || !fits {
                    Err("internal: the machine code's layout differs from the tapes'".into())
                } else {
                    let _ = up2.memory.set(memory);
                    for (s, c) in slots.iter().zip(codes) {
                        let _ = s.machine.set(std::mem::take(c));
                    }
                    report.seconds = started.elapsed().as_secs_f64();
                    Ok(report)
                }
            }
            Ok(Built::NoExecutableMemory(why)) => Err(why),
            Ok(Built::Foreign(_)) => Err("internal: foreign code".into()),
            Err(e) => Err(e.to_string()),
        };
        if cancel.load(Ordering::Relaxed) {
            BACKGROUND_CANCELLED.fetch_add(1, Ordering::SeqCst);
        } else if let Err(e) = &r
            && std::env::var_os("LSIM_CODEGEN_TRACE").is_some()
        {
            eprintln!("lsim-codegen: the model stays on its tapes: {e}");
        }
        up2.finish(r);
        BACKGROUND_RUNNING.fetch_sub(1, Ordering::SeqCst);
    });
    if let Err(e) = spawned {
        BACKGROUND_RUNNING.fetch_sub(1, Ordering::SeqCst);
        up.finish(Err(format!("no thread to compile on: {e}")));
    }
}

/// Why the model runs on its tapes: the system refused executable memory.
fn no_executable_memory(e: &cranelift_module::ModuleError) -> String {
    format!(
        "the system refused to make the machine code executable ({e}): the model runs on its tapes, which compute the same"
    )
}

/// The ISA the code is generated for: whether it has fused multiply-add
/// (exact integer powers use it).
pub fn native_fma() -> bool {
    jit::isa("speed", "backtracking").map(|i| i.has_native_fma()).unwrap_or(false)
}
