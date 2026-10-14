//! `LSIM_MATH_NO_FMA=1` makes the library run its build for CPUs without
//! FMA (to check that a run gives the same bytes both ways), whatever the
//! CPU has. (Its own test binary: the build is chosen once, at the first
//! call.)

#[test]
fn the_environment_can_ask_for_the_build_without_fma() {
    // SAFETY: the only test of this binary, before any other thread runs.
    unsafe { std::env::set_var("LSIM_MATH_NO_FMA", "1") };
    let r = lsim_math::exp(1.0);
    let expected = if cfg!(target_arch = "x86_64") { "soft" } else { "native" };
    assert_eq!(lsim_math::build().name, expected);
    assert_eq!(r, std::f64::consts::E);
}
