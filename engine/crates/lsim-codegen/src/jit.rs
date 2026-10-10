//! The Cranelift side: the runtime symbols generated code calls, the
//! module, and compilation of many functions on several threads.

use crate::CodegenError;
use crate::tables::{lsim_tab_guard, lsim_tab1, lsim_tab1d, lsim_tab2, lsim_tab2d};
use cranelift_codegen::control::ControlPlane;
use cranelift_codegen::ir::types::{F64, I64};
use cranelift_codegen::ir::{AbiParam, Function, Signature, Type};
use cranelift_codegen::isa::{OwnedTargetIsa, TargetIsa};
use cranelift_codegen::{Context, settings, settings::Configurable};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{
    FuncId, Linkage, Module, ModuleReloc, ModuleRelocTarget, default_libcall_names,
};
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

macro_rules! unary_fns {
    ($($name:ident => $f:ident),* $(,)?) => {
        $(extern "C" fn $name(x: f64) -> f64 { x.$f() })*
        fn unary() -> Vec<(&'static str, *const u8)> {
            vec![$((stringify!($name), $name as *const u8)),*]
        }
    };
}

unary_fns!(
    lsim_exp => exp, lsim_log => ln, lsim_sin => sin, lsim_cos => cos, lsim_tan => tan,
    lsim_asin => asin, lsim_acos => acos, lsim_atan => atan, lsim_sinh => sinh,
    lsim_cosh => cosh, lsim_tanh => tanh,
);

extern "C" fn lsim_pow(x: f64, y: f64) -> f64 {
    x.powf(y)
}

extern "C" fn lsim_atan2(y: f64, x: f64) -> f64 {
    y.atan2(x)
}

/// An imported function: its id in the module and its signature.
#[derive(Clone, Debug)]
pub(crate) struct Import {
    pub id: FuncId,
    pub sig: Signature,
}

/// The names generated code may call, with their addresses and argument
/// types (`true`: pointer or integer, `false`: f64); every one returns
/// an f64.
fn runtime_symbols() -> Vec<(&'static str, *const u8, Vec<bool>)> {
    let mut v: Vec<(&'static str, *const u8, Vec<bool>)> =
        unary().into_iter().map(|(n, p)| (n, p, vec![false])).collect();
    v.push(("lsim_pow", lsim_pow as *const u8, vec![false, false]));
    v.push(("lsim_atan2", lsim_atan2 as *const u8, vec![false, false]));
    v.push(("lsim_tab1", lsim_tab1 as *const u8, vec![true, false]));
    v.push(("lsim_tab1d", lsim_tab1d as *const u8, vec![true, false, true]));
    v.push(("lsim_tab2", lsim_tab2 as *const u8, vec![true, false, false]));
    v.push(("lsim_tab2d", lsim_tab2d as *const u8, vec![true, false, false, true]));
    v.push(("lsim_tab_guard", lsim_tab_guard as *const u8, vec![true, true, false]));
    v
}

/// The names of the math functions generated code calls.
pub const MATH_SYMBOLS: &[&str] = &[
    "lsim_exp",
    "lsim_log",
    "lsim_sin",
    "lsim_cos",
    "lsim_tan",
    "lsim_asin",
    "lsim_acos",
    "lsim_atan",
    "lsim_sinh",
    "lsim_cosh",
    "lsim_tanh",
    "lsim_pow",
    "lsim_atan2",
];

/// The host's ISA with the chosen settings.
pub(crate) fn isa(opt_level: &str, regalloc: &str) -> Result<OwnedTargetIsa, CodegenError> {
    isa_for(None, opt_level, regalloc)
}

/// The ISA of `target` (a triple such as `x86_64-pc-windows-msvc`; the
/// host when `None`) with the chosen settings and the host's CPU
/// features: another target's only for tests of the code generator's
/// portability (its code cannot run here).
pub(crate) fn isa_for(
    target: Option<&str>,
    opt_level: &str,
    regalloc: &str,
) -> Result<OwnedTargetIsa, CodegenError> {
    let be = |e: &dyn std::fmt::Display| CodegenError::Backend(e.to_string());
    let mut fb = settings::builder();
    fb.set("opt_level", opt_level).map_err(|e| be(&e))?;
    fb.set("regalloc_algorithm", regalloc).map_err(|e| be(&e))?;
    // nothing unwinds through generated code (it calls only functions
    // that cannot panic)
    fb.set("unwind_info", "false").map_err(|e| be(&e))?;
    // a frame larger than a page touches its pages in order: Windows
    // commits a thread's stack one guard page at a time, and a frame that
    // skips the guard page faults (elsewhere the probes are harmless)
    fb.set("enable_probestack", "true").map_err(|e| be(&e))?;
    fb.set("probestack_strategy", "inline").map_err(|e| be(&e))?;
    fb.set("enable_verifier", if cfg!(debug_assertions) { "true" } else { "false" })
        .map_err(|e| be(&e))?;
    let builder = match target {
        None => cranelift_native::builder().map_err(|e| be(&e))?,
        Some(t) => {
            let mut b = cranelift_codegen::isa::lookup_by_name(t).map_err(|e| be(&e))?;
            cranelift_native::infer_native_flags(&mut b).map_err(|e| be(&e))?;
            b
        }
    };
    let isa = builder.finish(settings::Flags::new(fb)).map_err(|e| be(&e))?;
    if isa.pointer_type() != I64 {
        return Err(CodegenError::Unsupported("targets with 64-bit pointers only".into()));
    }
    Ok(isa)
}

/// A signature in the target's own calling convention, the one its C
/// compilers use and Rust's `extern "C"` means there (System V on Linux
/// and macOS, the Windows x64 convention on Windows): the runtime
/// symbols are `extern "C"` Rust functions and generated functions are
/// called as `extern "C"` ones. Every signature the code generator makes
/// comes from here; [`check_conventions`] holds every compiled function
/// to it.
pub(crate) fn signature(isa: &dyn TargetIsa, params: &[Type], returns: &[Type]) -> Signature {
    let mut sig = Signature::new(isa.default_call_conv());
    sig.params.extend(params.iter().map(|t| AbiParam::new(*t)));
    sig.returns.extend(returns.iter().map(|t| AbiParam::new(*t)));
    sig
}

/// Refuses a function whose own signature or any signature it calls
/// through is not in the target's default calling convention (one built
/// with a fixed convention, which would mismatch the Rust side on some
/// platform).
pub(crate) fn check_conventions(isa: &dyn TargetIsa, f: &Function) -> Result<(), CodegenError> {
    let want = isa.default_call_conv();
    let sigs = std::iter::once(&f.signature).chain(f.dfg.signatures.values());
    match sigs.map(|s| s.call_conv).find(|c| *c != want) {
        None => Ok(()),
        Some(c) => Err(CodegenError::Backend(format!(
            "a signature in the {c} calling convention, the target's is {want}"
        ))),
    }
}

/// A JIT module whose code links against the runtime symbols, and their
/// declarations.
pub(crate) fn module(
    isa: &OwnedTargetIsa,
) -> Result<(JITModule, HashMap<&'static str, Import>), CodegenError> {
    let mut jb = JITBuilder::with_isa(isa.clone(), default_libcall_names());
    let syms = runtime_symbols();
    for (name, ptr, _) in &syms {
        jb.symbol(*name, *ptr);
    }
    let mut m = JITModule::new(jb);
    let mut decls = HashMap::new();
    for (name, _, args) in syms {
        let params: Vec<Type> = args.iter().map(|&p| if p { I64 } else { F64 }).collect();
        let sig = signature(&**isa, &params, &[F64]);
        let id = m
            .declare_function(name, Linkage::Import, &sig)
            .map_err(|e| CodegenError::Backend(e.to_string()))?;
        decls.insert(name, Import { id, sig });
    }
    Ok((m, decls))
}

/// The signature of every generated function: one pointer, to a
/// [`crate::CallCtx`].
pub(crate) fn eval_signature(isa: &dyn TargetIsa) -> Signature {
    signature(isa, &[I64], &[])
}

/// A function's machine code, ready to define in the module.
pub(crate) struct Compiled {
    pub id: FuncId,
    /// Cranelift IR instructions
    pub insts: usize,
    pub bytes: Vec<u8>,
    pub align: u64,
    pub relocs: Vec<ModuleReloc>,
}

/// Compiles one function.
fn compile_one(
    isa: &OwnedTargetIsa,
    id: FuncId,
    func: Function,
    trace: bool,
) -> Result<Compiled, CodegenError> {
    let started = std::time::Instant::now();
    let insts = func.dfg.num_insts();
    if let Some(dir) = std::env::var_os("LSIM_CODEGEN_DUMP") {
        let path = std::path::Path::new(&dir).join(format!("f{}.clif", id.as_u32()));
        let _ = std::fs::write(path, func.display().to_string());
    }
    let mut ctx = Context::for_function(func);
    ctx.compile(&**isa, &mut ControlPlane::default())
        .map_err(|e| CodegenError::Backend(format!("{:?}", e.inner)))?;
    let code = ctx.compiled_code().expect("just compiled");
    let align = code.buffer.alignment as u64;
    let bytes = code.code_buffer().to_vec();
    let relocs: Vec<ModuleReloc> = code
        .buffer
        .relocs()
        .iter()
        .map(|r| ModuleReloc::from_mach_reloc(r, &ctx.func, id))
        .collect();
    // calls go to the runtime symbols only: never to a library function
    // of Cranelift's own choosing, which the JIT would look up in the
    // platform's C runtime
    if let Some(r) = relocs.iter().find(|r| !matches!(r.name, ModuleRelocTarget::User { .. })) {
        return Err(CodegenError::Backend(format!("a call to {:?}", r.name)));
    }
    if trace {
        eprintln!(
            "lsim-codegen: function {} ({insts} instructions, {} bytes) in {:.2} ms",
            id.as_u32(),
            bytes.len(),
            started.elapsed().as_secs_f64() * 1e3
        );
    }
    Ok(Compiled { id, insts, bytes, align, relocs })
}

/// Builds (with `build(plan, chunk, id)`) and compiles every function,
/// largest first, on up to `threads` threads. Also returns the time spent
/// building IR and in Cranelift, summed over threads.
pub(crate) fn build_and_compile<F>(
    isa: &OwnedTargetIsa,
    mut jobs: Vec<(usize, usize, FuncId, usize)>,
    threads: usize,
    build: F,
) -> Result<(Vec<Compiled>, f64, f64), CodegenError>
where
    F: Fn(usize, usize, FuncId) -> Result<Function, CodegenError> + Sync,
{
    let trace = std::env::var_os("LSIM_CODEGEN_TRACE").is_some();
    jobs.sort_by_key(|j| j.3);
    let ir_ns = AtomicU64::new(0);
    let cl_ns = AtomicU64::new(0);
    let one = |job: (usize, usize, FuncId, usize)| -> Result<Compiled, CodegenError> {
        let t0 = std::time::Instant::now();
        let f = build(job.0, job.1, job.2)?;
        check_conventions(&**isa, &f)?;
        let t1 = std::time::Instant::now();
        let r = compile_one(isa, job.2, f, trace);
        ir_ns.fetch_add((t1 - t0).as_nanos() as u64, Ordering::Relaxed);
        cl_ns.fetch_add(t1.elapsed().as_nanos() as u64, Ordering::Relaxed);
        r
    };
    let threads = threads.max(1).min(jobs.len().max(1));
    let done: Result<Vec<Compiled>, CodegenError> = if threads == 1 {
        jobs.into_iter().rev().map(one).collect()
    } else {
        let queue = Mutex::new(jobs);
        let done = Mutex::new(Vec::new());
        std::thread::scope(|s| {
            for _ in 0..threads {
                s.spawn(|| {
                    loop {
                        let job = queue.lock().expect("queue").pop();
                        let Some(job) = job else { break };
                        let r = one(job);
                        done.lock().expect("results").push(r);
                    }
                });
            }
        });
        done.into_inner().expect("results").into_iter().collect()
    };
    let secs = |a: &AtomicU64| a.load(Ordering::Relaxed) as f64 * 1e-9;
    Ok((done?, secs(&ir_ns), secs(&cl_ns)))
}

/// How many threads to compile on.
pub(crate) fn default_threads() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get()).min(4)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cranelift_codegen::ir::{ExtFuncData, ExternalName, UserExternalName, UserFuncName};
    use cranelift_codegen::isa::CallConv;

    /// A function in convention `own` that calls through `callee`.
    fn function(own: CallConv, callee: CallConv) -> Function {
        let mut f = Function::with_name_signature(UserFuncName::user(0, 0), Signature::new(own));
        let sig = f.import_signature(Signature::new(callee));
        let user = f.declare_imported_user_function(UserExternalName { namespace: 0, index: 1 });
        f.import_function(ExtFuncData {
            name: ExternalName::user(user),
            signature: sig,
            colocated: false,
            patchable: false,
        });
        f
    }

    /// The guard holds every compiled function to the target's default
    /// convention: a signature built with a fixed one (the other
    /// platform's) is refused, in the function itself or in a call.
    #[test]
    fn a_fixed_calling_convention_is_refused() {
        for (triple, own, other) in [
            ("x86_64-pc-windows-msvc", CallConv::WindowsFastcall, CallConv::SystemV),
            ("x86_64-unknown-linux-gnu", CallConv::SystemV, CallConv::WindowsFastcall),
        ] {
            let isa = isa_for(Some(triple), "speed", "backtracking").expect("an x86-64 ISA");
            assert_eq!(isa.default_call_conv(), own);
            assert!(check_conventions(&*isa, &function(own, own)).is_ok());
            assert!(check_conventions(&*isa, &function(other, own)).is_err());
            assert!(check_conventions(&*isa, &function(own, other)).is_err());
            // and what the code generator makes is the target's
            assert_eq!(eval_signature(&*isa).call_conv, own);
            let (_, decls) = module(&isa).expect("a module");
            assert!(decls.values().all(|d| d.sig.call_conv == own));
        }
    }
}
