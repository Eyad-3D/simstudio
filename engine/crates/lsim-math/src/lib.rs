//! # lsim-math: the engine's one mathematical library
//!
//! Every transcendental function the engine evaluates goes through this
//! crate: the interpreter (`lsim_ir::eval`), the generated code (the
//! runtime symbols it calls) and its tapes, the condition kernels, the
//! interval enclosures, the solver layer and, through `lsim-sundials-sys`,
//! SUNDIALS' own C code. The functions are CORE-MATH's (MIT; vendored in
//! `core-math/`, its revision and how it was checked in its README):
//! **correctly rounded**, each result the exact value rounded to the
//! nearest double (ties to even), and so the same on every platform,
//! compiler and CPU, whatever the C runtime (glibc's and the MSVC UCRT's
//! differ in the last place, and both pick FMA variants by CPU).
//!
//! Two builds of the same C sources are linked on x86-64: one for CPUs with
//! the FMA instruction, and one for every CPU, whose fused multiply-adds
//! are exact in software (`csrc/lsim_math_fma.c`). A fused multiply-add is
//! exact either way, so the two compute the very same numbers at every
//! step; the first call picks the build the CPU can run (the instruction
//! where it has FMA and AVX). `LSIM_MATH_NO_FMA=1` in the environment
//! forces the software build, to check that a run gives the same bytes
//! both ways.
//!
//! What correct rounding guarantees, and what the interval code builds on
//! (`lsim_ir::interval`): the result `r` of `f(x)` is the double nearest to
//! the exact `f(x)`, so the exact value lies between `r`'s neighbours,
//! `next_down(r) ≤ f(x) ≤ next_up(r)`, at overflow and underflow too (an
//! infinite `r` means `|f(x)|` at least halfway past the largest double, a
//! zero `r` at most half the smallest subnormal). One ulp of widening on
//! each side encloses it: [`LIBRARY_ULPS`].
//!
//! Not here, on purpose: `sqrt`, which IEEE 754 rounds correctly and Rust's
//! `f64::sqrt` computes with the instruction on every target, and the
//! operations that are exact by definition (`abs`, `floor`, `min`, `max`,
//! …). [`fma`] and [`powi`] are here although not transcendental: Rust's
//! `mul_add` calls the C runtime's `fma` on x86-64, and `powi`'s precision
//! is left unspecified.

use std::ffi::c_int;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};

/// The widening, in ulps on each side, that encloses the exact value of
/// any function of this crate given its result: 1, as every result is
/// correctly rounded (the crate's documentation has the argument).
pub const LIBRARY_ULPS: u32 = 1;

type F1 = unsafe extern "C" fn(f64) -> f64;
type F2 = unsafe extern "C" fn(f64, f64) -> f64;

/// One build of the C sources: its functions.
#[derive(Clone, Copy)]
pub struct Build {
    /// its name (`"fma"`, `"soft"`, or `"native"` off x86-64)
    pub name: &'static str,
    sin: F1,
    cos: F1,
    tan: F1,
    asin: F1,
    acos: F1,
    atan: F1,
    atan2: F2,
    sinh: F1,
    cosh: F1,
    tanh: F1,
    exp: F1,
    log: F1,
    pow: F2,
}

macro_rules! build {
    ($name:literal, $($f:ident => $c:ident),* $(,)?) => {
        Build { name: $name, $($f: ffi::$c),* }
    };
}

// The C functions (build.rs renames CORE-MATH's `cr_<f>` to `<build>_<f>`).
mod ffi {
    #[cfg(target_arch = "x86_64")]
    unsafe extern "C" {
        pub fn lsim_cm_fma_sin(x: f64) -> f64;
        pub fn lsim_cm_fma_cos(x: f64) -> f64;
        pub fn lsim_cm_fma_tan(x: f64) -> f64;
        pub fn lsim_cm_fma_asin(x: f64) -> f64;
        pub fn lsim_cm_fma_acos(x: f64) -> f64;
        pub fn lsim_cm_fma_atan(x: f64) -> f64;
        pub fn lsim_cm_fma_atan2(y: f64, x: f64) -> f64;
        pub fn lsim_cm_fma_sinh(x: f64) -> f64;
        pub fn lsim_cm_fma_cosh(x: f64) -> f64;
        pub fn lsim_cm_fma_tanh(x: f64) -> f64;
        pub fn lsim_cm_fma_exp(x: f64) -> f64;
        pub fn lsim_cm_fma_log(x: f64) -> f64;
        pub fn lsim_cm_fma_pow(x: f64, y: f64) -> f64;
        pub fn lsim_cm_soft_sin(x: f64) -> f64;
        pub fn lsim_cm_soft_cos(x: f64) -> f64;
        pub fn lsim_cm_soft_tan(x: f64) -> f64;
        pub fn lsim_cm_soft_asin(x: f64) -> f64;
        pub fn lsim_cm_soft_acos(x: f64) -> f64;
        pub fn lsim_cm_soft_atan(x: f64) -> f64;
        pub fn lsim_cm_soft_atan2(y: f64, x: f64) -> f64;
        pub fn lsim_cm_soft_sinh(x: f64) -> f64;
        pub fn lsim_cm_soft_cosh(x: f64) -> f64;
        pub fn lsim_cm_soft_tanh(x: f64) -> f64;
        pub fn lsim_cm_soft_exp(x: f64) -> f64;
        pub fn lsim_cm_soft_log(x: f64) -> f64;
        pub fn lsim_cm_soft_pow(x: f64, y: f64) -> f64;
        /// the exact fused multiply-add in software (csrc/lsim_math_fma.c)
        pub fn lsim_math_fma_soft(x: f64, y: f64, z: f64) -> f64;
    }
    #[cfg(not(target_arch = "x86_64"))]
    unsafe extern "C" {
        pub fn lsim_cm_native_sin(x: f64) -> f64;
        pub fn lsim_cm_native_cos(x: f64) -> f64;
        pub fn lsim_cm_native_tan(x: f64) -> f64;
        pub fn lsim_cm_native_asin(x: f64) -> f64;
        pub fn lsim_cm_native_acos(x: f64) -> f64;
        pub fn lsim_cm_native_atan(x: f64) -> f64;
        pub fn lsim_cm_native_atan2(y: f64, x: f64) -> f64;
        pub fn lsim_cm_native_sinh(x: f64) -> f64;
        pub fn lsim_cm_native_cosh(x: f64) -> f64;
        pub fn lsim_cm_native_tanh(x: f64) -> f64;
        pub fn lsim_cm_native_exp(x: f64) -> f64;
        pub fn lsim_cm_native_log(x: f64) -> f64;
        pub fn lsim_cm_native_pow(x: f64, y: f64) -> f64;
    }
}

/// The build for CPUs with the FMA instruction.
#[cfg(target_arch = "x86_64")]
static FMA: Build = build!("fma",
    sin => lsim_cm_fma_sin, cos => lsim_cm_fma_cos, tan => lsim_cm_fma_tan,
    asin => lsim_cm_fma_asin, acos => lsim_cm_fma_acos, atan => lsim_cm_fma_atan,
    atan2 => lsim_cm_fma_atan2, sinh => lsim_cm_fma_sinh, cosh => lsim_cm_fma_cosh,
    tanh => lsim_cm_fma_tanh, exp => lsim_cm_fma_exp, log => lsim_cm_fma_log,
    pow => lsim_cm_fma_pow);

/// The build for every x86-64 CPU (fused multiply-adds in software).
#[cfg(target_arch = "x86_64")]
static SOFT: Build = build!("soft",
    sin => lsim_cm_soft_sin, cos => lsim_cm_soft_cos, tan => lsim_cm_soft_tan,
    asin => lsim_cm_soft_asin, acos => lsim_cm_soft_acos, atan => lsim_cm_soft_atan,
    atan2 => lsim_cm_soft_atan2, sinh => lsim_cm_soft_sinh, cosh => lsim_cm_soft_cosh,
    tanh => lsim_cm_soft_tanh, exp => lsim_cm_soft_exp, log => lsim_cm_soft_log,
    pow => lsim_cm_soft_pow);

/// The one build off x86-64 (the target's own fused multiply-add).
#[cfg(not(target_arch = "x86_64"))]
static NATIVE: Build = build!("native",
    sin => lsim_cm_native_sin, cos => lsim_cm_native_cos, tan => lsim_cm_native_tan,
    asin => lsim_cm_native_asin, acos => lsim_cm_native_acos, atan => lsim_cm_native_atan,
    atan2 => lsim_cm_native_atan2, sinh => lsim_cm_native_sinh, cosh => lsim_cm_native_cosh,
    tanh => lsim_cm_native_tanh, exp => lsim_cm_native_exp, log => lsim_cm_native_log,
    pow => lsim_cm_native_pow);

/// Whether the CPU runs the FMA build (FMA and AVX, with the operating
/// system saving the AVX state; `std` checks that).
#[cfg(target_arch = "x86_64")]
fn cpu_has_fma() -> bool {
    std::arch::is_x86_feature_detected!("fma") && std::arch::is_x86_feature_detected!("avx")
}

/// The build in use: 0 not chosen yet, 1 FMA, 2 software.
static CHOSEN: AtomicU8 = AtomicU8::new(0);

#[cfg(target_arch = "x86_64")]
#[cold]
fn choose() -> &'static Build {
    let forced_soft = std::env::var_os("LSIM_MATH_NO_FMA").is_some_and(|v| v != "0");
    let b = if cpu_has_fma() && !forced_soft { &FMA } else { &SOFT };
    CHOSEN.store(if std::ptr::eq(b, &FMA) { 1 } else { 2 }, Ordering::Relaxed);
    b
}

/// The build every function of the crate calls.
#[inline]
pub fn build() -> &'static Build {
    #[cfg(target_arch = "x86_64")]
    {
        match CHOSEN.load(Ordering::Relaxed) {
            1 => &FMA,
            2 => &SOFT,
            _ => choose(),
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        let _ = &CHOSEN;
        &NATIVE
    }
}

/// Every build this CPU can run (for tests: they must agree bit for bit).
pub fn builds() -> Vec<&'static Build> {
    #[cfg(target_arch = "x86_64")]
    {
        if cpu_has_fma() { vec![&FMA, &SOFT] } else { vec![&SOFT] }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        vec![&NATIVE]
    }
}

// SAFETY (every call below): the C functions take and return doubles by
// value, touch no memory of the caller and are thread-safe (CORE-MATH's
// tables are constant); the FMA build is only called on a CPU that has
// the instructions it uses (`choose`, `builds`).
impl Build {
    /// sine
    #[inline]
    pub fn sin(&self, x: f64) -> f64 {
        unsafe { (self.sin)(x) }
    }
    /// cosine
    #[inline]
    pub fn cos(&self, x: f64) -> f64 {
        unsafe { (self.cos)(x) }
    }
    /// tangent
    #[inline]
    pub fn tan(&self, x: f64) -> f64 {
        unsafe { (self.tan)(x) }
    }
    /// arcsine
    #[inline]
    pub fn asin(&self, x: f64) -> f64 {
        unsafe { (self.asin)(x) }
    }
    /// arccosine
    #[inline]
    pub fn acos(&self, x: f64) -> f64 {
        unsafe { (self.acos)(x) }
    }
    /// arctangent
    #[inline]
    pub fn atan(&self, x: f64) -> f64 {
        unsafe { (self.atan)(x) }
    }
    /// the angle of the point (x, y): `atan2(y, x)`
    #[inline]
    pub fn atan2(&self, y: f64, x: f64) -> f64 {
        unsafe { (self.atan2)(y, x) }
    }
    /// hyperbolic sine
    #[inline]
    pub fn sinh(&self, x: f64) -> f64 {
        unsafe { (self.sinh)(x) }
    }
    /// hyperbolic cosine
    #[inline]
    pub fn cosh(&self, x: f64) -> f64 {
        unsafe { (self.cosh)(x) }
    }
    /// hyperbolic tangent
    #[inline]
    pub fn tanh(&self, x: f64) -> f64 {
        unsafe { (self.tanh)(x) }
    }
    /// e^x
    #[inline]
    pub fn exp(&self, x: f64) -> f64 {
        unsafe { (self.exp)(x) }
    }
    /// the natural logarithm
    #[inline]
    pub fn ln(&self, x: f64) -> f64 {
        unsafe { (self.log)(x) }
    }
    /// x^y (C's `pow`, IEEE 754's special cases)
    #[inline]
    pub fn pow(&self, x: f64, y: f64) -> f64 {
        unsafe { (self.pow)(x, y) }
    }
}

/// sine, correctly rounded
#[inline]
pub fn sin(x: f64) -> f64 {
    build().sin(x)
}

/// cosine, correctly rounded
#[inline]
pub fn cos(x: f64) -> f64 {
    build().cos(x)
}

/// tangent, correctly rounded
#[inline]
pub fn tan(x: f64) -> f64 {
    build().tan(x)
}

/// arcsine, correctly rounded
#[inline]
pub fn asin(x: f64) -> f64 {
    build().asin(x)
}

/// arccosine, correctly rounded
#[inline]
pub fn acos(x: f64) -> f64 {
    build().acos(x)
}

/// arctangent, correctly rounded
#[inline]
pub fn atan(x: f64) -> f64 {
    build().atan(x)
}

/// the angle of the point (x, y), `atan2(y, x)`, correctly rounded
#[inline]
pub fn atan2(y: f64, x: f64) -> f64 {
    build().atan2(y, x)
}

/// hyperbolic sine, correctly rounded
#[inline]
pub fn sinh(x: f64) -> f64 {
    build().sinh(x)
}

/// hyperbolic cosine, correctly rounded
#[inline]
pub fn cosh(x: f64) -> f64 {
    build().cosh(x)
}

/// hyperbolic tangent, correctly rounded
#[inline]
pub fn tanh(x: f64) -> f64 {
    build().tanh(x)
}

/// e^x, correctly rounded
#[inline]
pub fn exp(x: f64) -> f64 {
    build().exp(x)
}

/// the natural logarithm, correctly rounded
#[inline]
pub fn ln(x: f64) -> f64 {
    build().ln(x)
}

/// x^y, correctly rounded (C's `pow`: `pow(x, ±0) = 1` and `pow(1, y) = 1`
/// for every x and y but a signalling NaN, `pow(x, y)` NaN for a negative
/// x and a y that is not an integer, …)
#[inline]
pub fn pow(x: f64, y: f64) -> f64 {
    build().pow(x, y)
}

/// The square root: IEEE 754's, correctly rounded on every platform (the
/// instruction). Here so that code which takes its functions from this
/// crate finds it with the others.
#[inline]
pub fn sqrt(x: f64) -> f64 {
    x.sqrt()
}

/// `a × b + c` rounded once: the CPU's instruction where it has one, else
/// the exact software one the CORE-MATH build without FMA uses (never the
/// C runtime's `fma`, which Rust's `mul_add` calls on x86-64). The same
/// value either way.
#[inline]
pub fn fma(a: f64, b: f64, c: f64) -> f64 {
    #[cfg(target_arch = "x86_64")]
    {
        if std::arch::is_x86_feature_detected!("fma") {
            // SAFETY: the CPU has the instruction (checked just now).
            return unsafe { fma_x86(a, b, c) };
        }
        fma_soft(a, b, c)
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        // (the base instruction set's own: AArch64's `fmadd`)
        a.mul_add(b, c)
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "fma")]
fn fma_x86(a: f64, b: f64, c: f64) -> f64 {
    use std::arch::x86_64::{_mm_cvtsd_f64, _mm_fmadd_sd, _mm_set_sd};
    _mm_cvtsd_f64(_mm_fmadd_sd(_mm_set_sd(a), _mm_set_sd(b), _mm_set_sd(c)))
}

/// The exact software fused multiply-add (musl's), whatever the CPU.
#[cfg(target_arch = "x86_64")]
#[inline]
pub fn fma_soft(a: f64, b: f64, c: f64) -> f64 {
    // SAFETY: plain C arithmetic on doubles passed by value.
    unsafe { ffi::lsim_math_fma_soft(a, b, c) }
}

/// A result CORE-MATH could not prove correctly rounded: its rounding test
/// failed even in its last, most accurate phase. CORE-MATH's authors know
/// of no such input (the hard-to-round cases are known); upstream, `pow`
/// and `atan2` print the input and end the process there. The engine's
/// copy (engine patch 1, `core-math/README.md`) returns the rounding of
/// the best approximation it has instead, and reports the case here.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UnexpectedCase {
    /// the function: `"pow"` or `"atan2"`
    pub function: &'static str,
    /// its arguments, in order (`atan2(y, x)`: y, then x)
    pub args: [f64; 2],
}

static UNEXPECTED: AtomicU64 = AtomicU64::new(0);
static FIRST_UNEXPECTED: OnceLock<UnexpectedCase> = OnceLock::new();

/// How many results since the process started were not proven correctly
/// rounded ([`UnexpectedCase`]), and the first of them. `(0, None)` but
/// for an input no one knows of.
pub fn unexpected_cases() -> (u64, Option<UnexpectedCase>) {
    (UNEXPECTED.load(Ordering::Relaxed), FIRST_UNEXPECTED.get().copied())
}

/// Where the vendored C code reports an [`UnexpectedCase`] (engine patch
/// 1): `function` 1 is `pow`, 2 is `atan2`, `x` and `y` its arguments in
/// C's order.
#[unsafe(no_mangle)]
pub extern "C" fn lsim_math_unexpected_case(function: c_int, x: f64, y: f64) {
    let function = match function {
        1 => "pow",
        2 => "atan2",
        _ => "unknown",
    };
    FIRST_UNEXPECTED.get_or_init(|| UnexpectedCase { function, args: [x, y] });
    UNEXPECTED.fetch_add(1, Ordering::Relaxed);
}

/// `x^n` by repeated squaring, as LLVM's `powi` (compiler-rt's
/// `__powidf2`): `n − 1` products at most for `n > 0` (relative error at
/// most `(|n| − 1) u` for `|n| ≥ 1`, `u = ε / 2`, before a reciprocal for a
/// negative `n`), the same steps on every platform.
#[inline]
pub fn powi(x: f64, n: i32) -> f64 {
    let mut base = x;
    let mut k = n.unsigned_abs();
    let mut r = 1.0;
    loop {
        if k & 1 != 0 {
            r *= base;
        }
        k >>= 1;
        if k == 0 {
            break;
        }
        base *= base;
    }
    if n < 0 { 1.0 / r } else { r }
}

// The same functions for C code built with the engine (`include/lsim_math.h`;
// lsim-sundials-sys routes SUNDIALS' calls to them).

/// [`sin`] for C
#[unsafe(no_mangle)]
pub extern "C" fn lsim_math_sin(x: f64) -> f64 {
    sin(x)
}
/// [`cos`] for C
#[unsafe(no_mangle)]
pub extern "C" fn lsim_math_cos(x: f64) -> f64 {
    cos(x)
}
/// [`tan`] for C
#[unsafe(no_mangle)]
pub extern "C" fn lsim_math_tan(x: f64) -> f64 {
    tan(x)
}
/// [`asin`] for C
#[unsafe(no_mangle)]
pub extern "C" fn lsim_math_asin(x: f64) -> f64 {
    asin(x)
}
/// [`acos`] for C
#[unsafe(no_mangle)]
pub extern "C" fn lsim_math_acos(x: f64) -> f64 {
    acos(x)
}
/// [`atan`] for C
#[unsafe(no_mangle)]
pub extern "C" fn lsim_math_atan(x: f64) -> f64 {
    atan(x)
}
/// [`atan2`] for C
#[unsafe(no_mangle)]
pub extern "C" fn lsim_math_atan2(y: f64, x: f64) -> f64 {
    atan2(y, x)
}
/// [`sinh`] for C
#[unsafe(no_mangle)]
pub extern "C" fn lsim_math_sinh(x: f64) -> f64 {
    sinh(x)
}
/// [`cosh`] for C
#[unsafe(no_mangle)]
pub extern "C" fn lsim_math_cosh(x: f64) -> f64 {
    cosh(x)
}
/// [`tanh`] for C
#[unsafe(no_mangle)]
pub extern "C" fn lsim_math_tanh(x: f64) -> f64 {
    tanh(x)
}
/// [`exp`] for C
#[unsafe(no_mangle)]
pub extern "C" fn lsim_math_exp(x: f64) -> f64 {
    exp(x)
}
/// [`ln`] for C (C's `log`)
#[unsafe(no_mangle)]
pub extern "C" fn lsim_math_log(x: f64) -> f64 {
    ln(x)
}
/// [`pow`] for C
#[unsafe(no_mangle)]
pub extern "C" fn lsim_math_pow(x: f64, y: f64) -> f64 {
    pow(x, y)
}
/// [`fma`] for C
#[unsafe(no_mangle)]
pub extern "C" fn lsim_math_fma(x: f64, y: f64, z: f64) -> f64 {
    fma(x, y, z)
}
