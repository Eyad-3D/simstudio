//! # lsim-codegen: prepared model → machine code
//!
//! Each [`PreparedModel`] is compiled with Cranelift into five functions,
//! all straight-line code (an `if` becomes a `select`):
//!
//! | function | computes |
//! |---|---|
//! | `residual` | `[x'; g]`: the state derivatives, then the residuals |
//! | `jvp` | `(∂[x'; g]/∂y)·v` in forward mode (dual numbers): the exact Jacobian times a vector |
//! | `roots` | the zero-crossing functions |
//! | `vars` | every flat variable, aliases included (the recorded channels) |
//! | `when` | the discrete variables after the fired `when` clauses |
//!
//! Parameters, discrete variables and inputs are read from memory at every
//! call, so changing a parameter never recompiles. Transcendental functions
//! call the Rust standard library through registered symbols.
//!
//! Work package 3 (DESIGN.md) owns this crate and adds: tables (monotone
//! cubic, 2-D), sparse coloured Jacobians, splitting very large models into
//! several functions, if-expression modes, and the compile-time budget.

mod lower;

use cranelift_codegen::ir::{AbiParam, Signature, UserFuncName, types};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module, default_libcall_names};
use lsim_ir::runtime::{EvalInput, Layout, ModelFunctions};
use lsim_ir::{PreparedModel, Slot, VarKind};
use std::sync::Arc;
use std::time::Instant;

pub use lower::MATH_SYMBOLS;

/// Why compilation failed.
#[derive(Debug, thiserror::Error)]
pub enum CodegenError {
    /// The model uses something the code generator does not handle yet.
    #[error("not supported by the code generator yet: {0}")]
    Unsupported(String),
    /// Cranelift refused the generated code (an engine bug).
    #[error("code generation failed: {0}")]
    Backend(String),
}

/// Options for [`compile`].
#[derive(Clone, Debug)]
pub struct CodegenOptions {
    /// Cranelift's `opt_level`: `"speed"` or `"none"`.
    pub opt_level: &'static str,
}

impl Default for CodegenOptions {
    fn default() -> Self {
        CodegenOptions { opt_level: "speed" }
    }
}

type EvalFn =
    unsafe extern "C" fn(f64, *const f64, *const f64, *const f64, *const f64, *mut f64, *mut f64);
type JvpFn = unsafe extern "C" fn(
    f64,
    *const f64,
    *const f64,
    *const f64,
    *const f64,
    *const f64,
    *mut f64,
    *mut f64,
);

/// Owns the JIT memory; the code in it is immutable once finalised.
struct CodeMemory(#[allow(dead_code)] JITModule);

// SAFETY: after `finalize_definitions` the module's code pages are read-and-
// execute only and nothing mutates them; the module is only dropped (which
// frees them) when the last `JitModel` holding this `Arc` goes away.
unsafe impl Send for CodeMemory {}
// SAFETY: as above: shared references never mutate the module.
unsafe impl Sync for CodeMemory {}

/// Where each kind of value lives, shared by the lowering of every function.
#[derive(Clone, Debug)]
pub(crate) struct Places {
    pub n_x: usize,
    /// slot → position in y (states and iteration variables)
    pub y_index: std::collections::HashMap<Slot, usize>,
    /// var index → position in d
    pub d_index: std::collections::HashMap<u32, usize>,
    /// var index → position in u
    pub u_index: std::collections::HashMap<u32, usize>,
}

/// A compiled model.
pub struct JitModel {
    layout: Layout,
    residual: EvalFn,
    jvp: JvpFn,
    roots: EvalFn,
    vars: EvalFn,
    when: JvpFn,
    y0: Vec<f64>,
    d0: Vec<f64>,
    /// time spent compiling, s
    pub compile_seconds: f64,
    /// bytes of machine code (all five functions)
    pub code_bytes: usize,
    _memory: Arc<CodeMemory>,
}

impl ModelFunctions for JitModel {
    fn layout(&self) -> &Layout {
        &self.layout
    }

    fn residual(&self, inp: &EvalInput<'_>, work: &mut [f64], out: &mut [f64]) {
        self.check(inp, out.len(), self.layout.n_y());
        // SAFETY: the slices have the lengths the code was generated for
        // (checked above); the code only reads and writes inside them.
        unsafe {
            (self.residual)(
                inp.t,
                inp.y.as_ptr(),
                inp.p.as_ptr(),
                inp.d.as_ptr(),
                inp.u.as_ptr(),
                work.as_mut_ptr(),
                out.as_mut_ptr(),
            )
        }
    }

    fn jvp(&self, inp: &EvalInput<'_>, v: &[f64], work: &mut [f64], out: &mut [f64]) {
        self.check(inp, out.len(), self.layout.n_y());
        assert_eq!(v.len(), self.layout.n_y());
        // SAFETY: as in `residual`; `v` has n_y values.
        unsafe {
            (self.jvp)(
                inp.t,
                inp.y.as_ptr(),
                inp.p.as_ptr(),
                inp.d.as_ptr(),
                inp.u.as_ptr(),
                v.as_ptr(),
                work.as_mut_ptr(),
                out.as_mut_ptr(),
            )
        }
    }

    fn roots(&self, inp: &EvalInput<'_>, work: &mut [f64], out: &mut [f64]) {
        self.check(inp, out.len(), self.layout.n_roots);
        // SAFETY: as in `residual`.
        unsafe {
            (self.roots)(
                inp.t,
                inp.y.as_ptr(),
                inp.p.as_ptr(),
                inp.d.as_ptr(),
                inp.u.as_ptr(),
                work.as_mut_ptr(),
                out.as_mut_ptr(),
            )
        }
    }

    fn vars(&self, inp: &EvalInput<'_>, work: &mut [f64], out: &mut [f64]) {
        self.check(inp, out.len(), self.layout.n_vars);
        // SAFETY: as in `residual`.
        unsafe {
            (self.vars)(
                inp.t,
                inp.y.as_ptr(),
                inp.p.as_ptr(),
                inp.d.as_ptr(),
                inp.u.as_ptr(),
                work.as_mut_ptr(),
                out.as_mut_ptr(),
            )
        }
    }

    fn when(&self, inp: &EvalInput<'_>, fired: &[f64], work: &mut [f64], d_out: &mut [f64]) {
        self.check(inp, d_out.len(), self.layout.n_d);
        assert_eq!(fired.len(), self.layout.n_whens);
        // SAFETY: as in `residual`; `fired` has one value per when-clause.
        unsafe {
            (self.when)(
                inp.t,
                inp.y.as_ptr(),
                inp.p.as_ptr(),
                inp.d.as_ptr(),
                inp.u.as_ptr(),
                fired.as_ptr(),
                work.as_mut_ptr(),
                d_out.as_mut_ptr(),
            )
        }
    }

    fn start(&self, _p: &[f64], y0: &mut [f64], d0: &mut [f64]) {
        y0.copy_from_slice(&self.y0);
        d0.copy_from_slice(&self.d0);
    }
}

impl JitModel {
    fn check(&self, inp: &EvalInput<'_>, out: usize, want: usize) {
        let l = &self.layout;
        assert!(
            inp.y.len() == l.n_y()
                && inp.p.len() == l.n_p
                && inp.d.len() == l.n_d
                && inp.u.len() == l.n_u,
            "input vectors do not match the compiled model's layout"
        );
        assert_eq!(out, want, "output vector has the wrong length");
    }
}

fn eval_signature(m: &JITModule, extra_ptr: bool) -> Signature {
    let ptr = m.target_config().pointer_type();
    let mut sig = m.make_signature();
    sig.params.push(AbiParam::new(types::F64));
    let n_ptr = if extra_ptr { 7 } else { 6 };
    for _ in 0..n_ptr {
        sig.params.push(AbiParam::new(ptr));
    }
    sig
}

/// Which function body to generate.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Kind {
    Residual,
    Jvp,
    Roots,
    Vars,
    When,
}

/// Compiles `model` to machine code.
pub fn compile(model: &PreparedModel, opts: &CodegenOptions) -> Result<JitModel, CodegenError> {
    let started = Instant::now();
    let flat = &model.flat;
    let n_x = model.states.len();
    let mut places = Places {
        n_x,
        y_index: Default::default(),
        d_index: Default::default(),
        u_index: Default::default(),
    };
    for (i, v) in model.states.iter().enumerate() {
        places.y_index.insert(Slot::Var(*v), i);
    }
    for (k, s) in model.algebraics.iter().enumerate() {
        places.y_index.insert(*s, n_x + k);
    }
    for (k, v) in model.discretes.iter().enumerate() {
        places.d_index.insert(v.0, k);
    }
    for (k, v) in model.inputs.iter().enumerate() {
        places.u_index.insert(v.0, k);
    }
    let layout = Layout {
        n_x,
        n_z: model.algebraics.len(),
        n_p: flat.params.len(),
        n_d: model.discretes.len(),
        n_u: model.inputs.len(),
        n_roots: model.zero_crossings.len(),
        n_whens: model.whens.len(),
        n_vars: flat.vars.len(),
        n_work: model.assignments.len().max(1),
    };

    let mut flags = vec![("opt_level", opts.opt_level)];
    flags.push(("enable_verifier", if cfg!(debug_assertions) { "true" } else { "false" }));
    let mut flag_builder = settings::builder();
    for (k, v) in &flags {
        flag_builder.set(k, v).map_err(|e| CodegenError::Backend(e.to_string()))?;
    }
    let isa = cranelift_native::builder()
        .map_err(|e| CodegenError::Backend(e.to_string()))?
        .finish(settings::Flags::new(flag_builder))
        .map_err(|e| CodegenError::Backend(e.to_string()))?;
    let mut jb = JITBuilder::with_isa(isa, default_libcall_names());
    for (name, ptr) in lower::math_symbols() {
        jb.symbol(name, ptr);
    }
    let mut module = JITModule::new(jb);
    let math = lower::declare_math(&mut module)?;

    let kinds = [Kind::Residual, Kind::Jvp, Kind::Roots, Kind::Vars, Kind::When];
    let mut ids: Vec<FuncId> = vec![];
    let mut ctx = module.make_context();
    let mut fctx = FunctionBuilderContext::new();
    let mut code_bytes = 0;
    let tc = module.target_config();
    for (n, kind) in kinds.iter().enumerate() {
        let sig = eval_signature(&module, matches!(kind, Kind::Jvp | Kind::When));
        let name = format!("{kind:?}").to_lowercase();
        let id = module
            .declare_function(&name, Linkage::Local, &sig)
            .map_err(|e| CodegenError::Backend(e.to_string()))?;
        ctx.func.signature = sig;
        ctx.func.name = UserFuncName::user(0, n as u32);
        {
            let mut b = FunctionBuilder::new(&mut ctx.func, &mut fctx);
            let refs = lower::import_math(&mut module, &mut b, &math);
            lower::body(
                &mut b,
                *kind,
                model,
                &places,
                &refs,
                module.target_config().pointer_type(),
            )?;
            b.seal_all_blocks();
            b.finalize(tc);
        }
        module
            .define_function(id, &mut ctx)
            .map_err(|e| CodegenError::Backend(format!("{e:?}")))?;
        code_bytes += ctx.compiled_code().map(|c| c.code_buffer().len()).unwrap_or(0);
        module.clear_context(&mut ctx);
        ids.push(id);
    }
    module.finalize_definitions().map_err(|e| CodegenError::Backend(e.to_string()))?;
    let ptr = |i: usize| module.get_finalized_function(ids[i]);
    // SAFETY: each pointer is a function just compiled with exactly the
    // signature it is transmuted to (`eval_signature`).
    let (residual, jvp, roots, vars, when) = unsafe {
        (
            std::mem::transmute::<*const u8, EvalFn>(ptr(0)),
            std::mem::transmute::<*const u8, JvpFn>(ptr(1)),
            std::mem::transmute::<*const u8, EvalFn>(ptr(2)),
            std::mem::transmute::<*const u8, EvalFn>(ptr(3)),
            std::mem::transmute::<*const u8, JvpFn>(ptr(4)),
        )
    };

    let mut y0 = vec![0.0; layout.n_y()];
    for (i, v) in model.states.iter().enumerate() {
        y0[i] = flat.var(*v).start.unwrap_or(0.0);
    }
    for (k, s) in model.algebraics.iter().enumerate() {
        if let Slot::Var(v) = s {
            y0[n_x + k] = flat.var(*v).start.unwrap_or(0.0);
        }
    }
    let d0 = model
        .discretes
        .iter()
        .map(|v| {
            debug_assert_eq!(flat.var(*v).kind, VarKind::Discrete);
            flat.var(*v).start.unwrap_or(0.0)
        })
        .collect();

    Ok(JitModel {
        layout,
        residual,
        jvp,
        roots,
        vars,
        when,
        y0,
        d0,
        compile_seconds: started.elapsed().as_secs_f64(),
        code_bytes,
        _memory: Arc::new(CodeMemory(module)),
    })
}
