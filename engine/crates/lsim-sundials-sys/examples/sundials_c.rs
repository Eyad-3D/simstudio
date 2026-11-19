//! The engine's SUNDIALS as a static C library (`libsundials_c.a`): the
//! vendored, patched C code and the mathematical functions it calls
//! (lsim-math), for C programs that use SUNDIALS' C interface, such as
//! SUNDIALS' own examples (`benchmarks/hard/hard.py sundials`).

pub use lsim_math;
pub use lsim_sundials_sys;
