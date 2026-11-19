//! The floating-point environment the engine computes in: rounding to
//! nearest, subnormal numbers kept (no flush-to-zero, no
//! denormals-are-zero), every exception masked: x86-64's default control
//! register (MXCSR `0x1F80`), which Rust's code generation assumes too.
//!
//! The environment belongs to the thread, and a library loaded in the
//! host process (a Python extension compiled with fast-math, an audio or
//! graphics library) may have set flush-to-zero or denormals-are-zero on
//! the thread that calls the engine. Then subnormal values become zeros:
//! the interval enclosures' outward rounding no longer holds near them,
//! tables built on one thread differ from the same tables built on
//! another, and results stop being reproducible. The engine's entry points
//! (running a model, compiling one, the compiler's own threads) therefore
//! enter [`DefaultFloatEnv`] for their duration: it sets the default
//! environment where the thread's differs and restores the thread's own
//! when it ends, so the host keeps whatever it chose.
//!
//! On targets other than x86-64 it does nothing (AArch64's FPCR is the
//! next to cover, should the engine run there).

/// MXCSR's default: every exception masked, round to nearest, neither
/// flush-to-zero (bit 15) nor denormals-are-zero (bit 6).
pub const DEFAULT_MXCSR: u32 = 0x1F80;

/// MXCSR's control bits (exception masks, rounding, FTZ, DAZ); bits 0-5
/// are the sticky exception flags, which change nothing.
const CONTROL: u32 = 0xFFC0;

/// The calling thread's MXCSR (x86-64; [`DEFAULT_MXCSR`] elsewhere).
pub fn mxcsr() -> u32 {
    #[cfg(target_arch = "x86_64")]
    {
        let mut x: u32 = 0;
        // SAFETY: STMXCSR stores the 32-bit register to the address given,
        // a live u32 of ours.
        unsafe {
            std::arch::asm!("stmxcsr [{}]", in(reg) &mut x, options(nostack, preserves_flags));
        }
        x
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        DEFAULT_MXCSR
    }
}

/// Sets the calling thread's MXCSR (x86-64; nothing elsewhere).
///
/// # Safety
/// Code that runs under anything but the default environment may compute
/// other values than Rust's code generation assumed: only to restore a
/// value read with [`mxcsr`], or to set the default.
pub unsafe fn set_mxcsr(x: u32) {
    #[cfg(target_arch = "x86_64")]
    // SAFETY: LDMXCSR loads the register from a live u32 of ours; the
    // caller answers for the value.
    unsafe {
        std::arch::asm!("ldmxcsr [{}]", in(reg) &x, options(nostack, preserves_flags, readonly));
    }
    #[cfg(not(target_arch = "x86_64"))]
    let _ = x;
}

/// Whether the calling thread computes in the default environment.
pub fn is_default() -> bool {
    mxcsr() & CONTROL == DEFAULT_MXCSR & CONTROL
}

/// The default environment on this thread while it lives; the thread's
/// own again when it is dropped (it must be dropped on the thread that
/// made it, as a guard of a scope is).
#[must_use = "the default environment lasts while the guard lives"]
pub struct DefaultFloatEnv {
    /// the thread's own MXCSR, if it was not the default
    saved: Option<u32>,
    /// a guard is the thread's: not Send
    _thread: std::marker::PhantomData<*const ()>,
}

impl DefaultFloatEnv {
    /// Sets the default environment on the calling thread if it is not
    /// already; [`DefaultFloatEnv::changed`] tells whether it was.
    pub fn enter() -> DefaultFloatEnv {
        let saved = (!is_default()).then(mxcsr);
        if saved.is_some() {
            // SAFETY: the default environment (see `set_mxcsr`)
            unsafe { set_mxcsr(DEFAULT_MXCSR) }
        }
        DefaultFloatEnv { saved, _thread: std::marker::PhantomData }
    }

    /// The thread's own MXCSR, which this guard replaced by the default,
    /// if it did.
    pub fn changed(&self) -> Option<u32> {
        self.saved
    }
}

impl Drop for DefaultFloatEnv {
    fn drop(&mut self) {
        if let Some(x) = self.saved {
            // SAFETY: the thread's own value, read when the guard was
            // made (see `set_mxcsr`)
            unsafe { set_mxcsr(x) }
        }
    }
}

#[cfg(all(test, target_arch = "x86_64"))]
mod tests {
    use super::*;
    use std::hint::black_box;

    const FTZ: u32 = 1 << 15;
    const DAZ: u32 = 1 << 6;

    /// Under flush-to-zero and denormals-are-zero subnormals vanish; in
    /// the guard they do not, and after it the thread's own environment
    /// is back.
    #[test]
    fn the_guard_sets_the_default_and_gives_the_thread_its_own_back() {
        let tiny = black_box(3e-310);
        assert!(is_default());
        assert!(black_box(tiny) * black_box(0.5) > 0.0);
        let own = DEFAULT_MXCSR | FTZ | DAZ;
        // SAFETY: a test of exactly this; the default is restored below
        unsafe { set_mxcsr(own) };
        assert!(!is_default());
        // denormals-are-zero: the subnormal reads as zero
        assert_eq!(black_box(tiny) * black_box(2.0), 0.0);
        {
            let g = DefaultFloatEnv::enter();
            assert_eq!(g.changed(), Some(own));
            assert!(is_default());
            assert_eq!(black_box(tiny) * black_box(2.0), 6e-310);
            // a guard in a guard changes nothing
            let inner = DefaultFloatEnv::enter();
            assert_eq!(inner.changed(), None);
        }
        assert_eq!(mxcsr() & CONTROL, own & CONTROL);
        assert_eq!(black_box(1e-308) * black_box(1e-10), 0.0);
        // SAFETY: back to the default
        unsafe { set_mxcsr(DEFAULT_MXCSR) };
        assert!(is_default());
    }
}
