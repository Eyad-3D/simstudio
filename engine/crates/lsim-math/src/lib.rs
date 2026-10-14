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
//! Correct rounding depends on the C compiler's options: the build script
//! gives it its own only, ignoring the environment's C flags and refusing
//! an option that relaxes floating-point semantics (`c_flags.rs`), and the
//! first call checks the build it picks on 27 hard cases
//! ([`Build::self_check`], about 15 µs), stopping the process with the
//! case if one is misrounded.
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

/// The build in use: 0 not chosen yet, 1 FMA, 2 software, 3 native.
static CHOSEN: AtomicU8 = AtomicU8::new(0);

/// Picks the build the CPU runs, and checks it ([`Build::self_check`]) once,
/// before any result of it is used: a build that misrounds one of the
/// check's cases stops the process here with the case, rather than give
/// results that are not the ones the engine promises.
#[cold]
fn choose() -> &'static Build {
    #[cfg(target_arch = "x86_64")]
    let (b, code) = {
        let forced_soft = std::env::var_os("LSIM_MATH_NO_FMA").is_some_and(|v| v != "0");
        if cpu_has_fma() && !forced_soft { (&FMA, 1) } else { (&SOFT, 2) }
    };
    #[cfg(not(target_arch = "x86_64"))]
    let (b, code) = (&NATIVE, 3);
    if let Err(wrong) = b.self_check() {
        panic!(
            "lsim-math: its {} build is not correctly rounded ({wrong}): the C compiler, or an \
             option it was given, has broken CORE-MATH (see lsim-math's build.rs)",
            b.name
        );
    }
    CHOSEN.store(code, Ordering::Relaxed);
    b
}

/// The build every function of the crate calls: the first call picks it
/// for the CPU and checks it.
#[inline]
pub fn build() -> &'static Build {
    match CHOSEN.load(Ordering::Relaxed) {
        #[cfg(target_arch = "x86_64")]
        1 => &FMA,
        #[cfg(target_arch = "x86_64")]
        2 => &SOFT,
        #[cfg(not(target_arch = "x86_64"))]
        3 => &NATIVE,
        _ => choose(),
    }
}

/// The self-check's cases: function (in [`Build::self_check`]'s order),
/// arguments and correctly rounded result, as the bits of doubles. From the
/// references (`tests/data/hard_cases.txt`), chosen (October 2026) so that
/// a build compiled with any of the options found to break correct
/// rounding (gcc 13: `-ffast-math` or `-Ofast`, 2 270 of the 4 672
/// references misrounded per build; `-funsafe-math-optimizations` or
/// `-fassociative-math`, 2 132; `-mfpmath=387`, 948; `-ffinite-math-only`,
/// 292; `-fno-signed-zeros`, 10) misrounds at least one case of every
/// function the option affects; each line names the options it catches.
/// (`-ffp-contract=fast`, `-freciprocal-math`, `-O0`, `-march=native` and
/// `-frounding-math` misround none; clang refuses `-ffast-math` and
/// `-mfpmath=387` on these files.)
const SELF_CHECK: [(u8, u64, u64, u64); 27] = [
    (0, 0x409cc61bc4de51ea, 0, 0x3fe0d485e5b84f48), // sin: fast, unsafe
    (0, 0x3ee755f84bfd16d7, 0, 0x3ee755f84bfb0559), // sin: fast, x87
    (1, 0x7fb4f396825e0333, 0, 0x3fef7742de035532), // cos: fast, unsafe, x87
    (1, 0x7ff0000000000000, 0, 0xfff8000000000000), // cos(inf) = NaN: finite
    (2, 0x7fdfff4f59503fb1, 0, 0xbff85fdbc178c0f9), // tan: fast, unsafe, x87
    (2, 0x7ff0000000000000, 0, 0xfff8000000000000), // tan(inf) = NaN: fast, finite
    (3, 0x3ea8b54424b0ce7e, 0, 0x3ea8b54424b0d0f3), // asin: fast, unsafe, x87
    (3, 0x7ff0000000000000, 0, 0xfff8000000000000), // asin(inf) = NaN: finite
    (4, 0x3f12854fe4bf0470, 0, 0x3ff921b13f049913), // acos: fast, unsafe, x87
    (4, 0x7ff0000000000000, 0, 0xfff8000000000000), // acos(inf) = NaN: finite
    (5, 0x3ef10ac13c976e99, 0, 0x3ef10ac13c90fcc3), // atan: fast, unsafe, x87
    (5, 0x3ee1349e530388f9, 0, 0x3ee1349e5301e087), // atan: fast, unsafe, x87
    (6, 0x432a468c20cecd8a, 0x432bfc92fae0bf70, 0x3fe81fc0098faaf5), // atan2: fast, unsafe, x87
    (6, 0x4330f6c31bdf2e99, 0x433834a41566edbe, 0x3fe38f9a2f9a4fd1), // atan2: fast, unsafe, x87
    (7, 0x3ff2675de54f6be7, 0, 0x3ff6bd2fd478f227), // sinh: fast, unsafe, x87
    (7, 0x4033890b6d79b0fa, 0, 0x41a22ba49081b564), // sinh: fast, unsafe, x87
    (8, 0x3f95ce51df2631ce, 0, 0x3ff000edc2280bdb), // cosh: fast, unsafe, x87
    (8, 0x7fefffffffffffff, 0, 0x7ff0000000000000), // cosh(max) = inf: finite
    (9, 0x3f6000b47bdb383f, 0, 0x3f6000b32658e41f), // tanh: fast, unsafe, x87
    (9, 0x3fd1b573a7958914, 0, 0x3fd14530c725d89f), // tanh: fast, unsafe, x87
    (10, 0x3e3ab6c3269b2ef1, 0, 0x3ff0000001ab6c33), // exp: fast, unsafe, x87
    (10, 0xbe94df889d46109d, 0, 0x3fefffff5903bcc9), // exp: fast, unsafe, x87
    (11, 0x3f5490a2a84f1862, 0, 0xc01ab8cd569d0422), // log: fast, unsafe, x87
    (11, 0xfff0000000000000, 0, 0xfff8000000000000), // log(-inf) = NaN: fast, finite
    (12, 0x431c4269c893fd34, 0x3fd4000000000000, 0x40ed79ca618b9632), // pow: fast, unsafe, x87
    (12, 0x0587423b7c05b2be, 0x3ff1800000000000, 0x000f48f729360780), // pow: fast, finite
    (12, 0x8000000000000000, 0x3ff0000000000000, 0x8000000000000000), // pow(-0, 1) = -0: nsz
];

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
    /// Checks the build on a few hard cases per function, each against its
    /// correctly rounded result: `Err` names the first it misrounds. The
    /// library runs it once, on the build it picks, before using it
    /// ([`build`]); cheap (27 calls), it catches a build whose C compiler
    /// or options broke correct rounding, which the build script cannot
    /// rule out entirely (it refuses the options known to, and ignores the
    /// environment's C flags).
    pub fn self_check(&self) -> Result<(), String> {
        const NAMES: [&str; 13] = [
            "sin", "cos", "tan", "asin", "acos", "atan", "atan2", "sinh", "cosh", "tanh", "exp",
            "log", "pow",
        ];
        for (f, x, y, r) in SELF_CHECK {
            let (x, y, r) = (f64::from_bits(x), f64::from_bits(y), f64::from_bits(r));
            let v = match f {
                0 => self.sin(x),
                1 => self.cos(x),
                2 => self.tan(x),
                3 => self.asin(x),
                4 => self.acos(x),
                5 => self.atan(x),
                6 => self.atan2(x, y),
                7 => self.sinh(x),
                8 => self.cosh(x),
                9 => self.tanh(x),
                10 => self.exp(x),
                11 => self.ln(x),
                _ => self.pow(x, y),
            };
            let right = if r.is_nan() { v.is_nan() } else { v.to_bits() == r.to_bits() };
            if !right {
                let name = NAMES[f as usize];
                let args =
                    if matches!(f, 6 | 12) { format!("{x:e}, {y:e}") } else { format!("{x:e}") };
                return Err(format!(
                    "{name}({args}) = {v:e} ({:#018x}), not {r:e} ({:#018x})",
                    v.to_bits(),
                    r.to_bits()
                ));
            }
        }
        Ok(())
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    unsafe extern "C" fn quarter1(_: f64) -> f64 {
        0.25
    }
    unsafe extern "C" fn quarter2(_: f64, _: f64) -> f64 {
        0.25
    }

    /// The self-check passes on every build this CPU runs, and fails on a
    /// build where any one function is wrong: every function has cases.
    #[test]
    fn the_self_check_has_cases_for_every_function() {
        for b in builds() {
            assert_eq!(b.self_check(), Ok(()), "{} build", b.name);
        }
        let good = *build();
        let names = [
            "sin", "cos", "tan", "asin", "acos", "atan", "atan2", "sinh", "cosh", "tanh", "exp",
            "log", "pow",
        ];
        for (k, name) in names.iter().enumerate() {
            let mut b = good;
            match k {
                0 => b.sin = quarter1,
                1 => b.cos = quarter1,
                2 => b.tan = quarter1,
                3 => b.asin = quarter1,
                4 => b.acos = quarter1,
                5 => b.atan = quarter1,
                6 => b.atan2 = quarter2,
                7 => b.sinh = quarter1,
                8 => b.cosh = quarter1,
                9 => b.tanh = quarter1,
                10 => b.exp = quarter1,
                11 => b.log = quarter1,
                _ => b.pow = quarter2,
            }
            let e = b.self_check().expect_err(name);
            assert!(e.starts_with(&format!("{name}(")), "{e}");
        }
        // and it is cheap
        let t = std::time::Instant::now();
        for b in builds() {
            b.self_check().unwrap();
        }
        println!("self-check of every build: {:?}", t.elapsed());
    }
}
