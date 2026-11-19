//! A case the vendored C code cannot round with certainty is counted, and
//! the first kept, instead of ending the process (engine patch 1,
//! `core-math/README.md`). No input is known to reach it, so the test
//! calls the function the C code calls. (Its own test binary: the count
//! is the process's.)

use lsim_math as m;

#[test]
fn an_unexpected_case_is_counted_and_the_first_kept() {
    assert_eq!(m::unexpected_cases(), (0, None));
    m::lsim_math_unexpected_case(1, 0.75, -3.5);
    m::lsim_math_unexpected_case(2, 1.0, 2.0);
    let (n, first) = m::unexpected_cases();
    assert_eq!(n, 2);
    assert_eq!(first, Some(m::UnexpectedCase { function: "pow", args: [0.75, -3.5] }));
    // and the library still answers
    assert_eq!(m::pow(2.0, 10.0), 1024.0);
}
