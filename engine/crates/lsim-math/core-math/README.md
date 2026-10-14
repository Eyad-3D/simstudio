# CORE-MATH, vendored

The correctly rounded binary64 functions the engine uses, from the
CORE-MATH project (<https://core-math.gitlabpages.inria.fr/>, MIT licence:
`LICENSE` here and the header of every file), copied unmodified from

    https://gitlab.inria.fr/core-math/core-math
    revision e78d2681bb90363ac945d20008e284c3d1b0315f (13 October 2026)

`binary64/<f>/<f>.c` for sin, cos, tan, asin, acos, atan, atan2, sinh,
cosh, tanh, exp, log and pow, with the headers they include (`tint.h` of
atan2, `dint.h` of log, `pow.h`, `dint.h` and `qint.h` of pow). Nothing
else of the project is needed to build them.

`build.rs` compiles them with `-ffp-contract=off` (no contraction into
fused multiply-adds the source does not write) and `-fno-math-errno`,
once with `-mfma` and once with an exact software fused multiply-add
(`../csrc`), renaming each `cr_<f>`.

## Why this revision

The next one, 37384b5 ("reduce the number of exceptional cases from 61 to
21", 14 October 2026), rounds three hard cases of exp the wrong way
(exp(-0x1.07e05e6e03b83p-2), exp(-0x1.40a990279c2c0p-6),
exp(0x1.aca7ae8da5a7bp+0): one ulp off, each within 1.6e-16 ulp of a
rounding boundary); so does the head of 14 October (d768e65). This
revision gives the correctly rounded result on every one of CORE-MATH's
own hard-to-round cases at the head (8.4 million inputs across the 13
functions), on 2.2 million random inputs and on the special values, in
both builds, checked against MPFR (`engine/experiments/libm-eval`). The
three exp inputs are among the test's references (`../tests`).

## Updating

1. Copy the files above from the new revision.
2. Run `engine/experiments/libm-eval` on the new revision's hard-to-round
   cases (all of them, both builds); every result must be the correctly
   rounded one.
3. Regenerate `../tests/data/hard_cases.txt` with `../tools/hard_cases.py`
   from the new checkout and run the crate's tests.
4. Record the revision here.
