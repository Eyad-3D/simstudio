# CORE-MATH, vendored

The correctly rounded binary64 functions the engine uses, from the
CORE-MATH project (<https://core-math.gitlabpages.inria.fr/>, MIT licence:
`LICENSE` here and the header of every file), copied from

    https://gitlab.inria.fr/core-math/core-math
    revision e78d2681bb90363ac945d20008e284c3d1b0315f (13 October 2026)

`src/binary64/<f>/<f>.c` for sin, cos, tan, asin, acos, atan, atan2, sinh,
cosh, tanh, exp, log and pow, with the headers they include (`tint.h` of
atan2, `dint.h` of log, `pow.h`, `dint.h` and `qint.h` of pow), into
`binary64/`, then patched (below). Nothing else of the project is needed
to build them.

## The engine's two patches

The copy is upstream's with two patches, recorded together as one diff,
`lsim.patch` (every changed line is in it; `patch -p1 < lsim.patch` in a
folder holding upstream's `binary64/` reproduces the copy):

1. **No exit.** Where its last, most accurate phase still cannot prove
   the rounding (a case CORE-MATH's authors know of no input for),
   upstream's `pow` (`pow.c`) and `atan2` (`tint.h`) print the input to
   stdout and call `exit(1)`, which would end the desktop app or the
   Python process the engine runs in. The copy calls
   `lsim_math_unexpected_case` instead (`../src/lib.rs`: counted, the
   first kept, `lsim_math::unexpected_cases`) and returns the rounding of
   the approximation it has, as if the test had passed. No reference
   reaches it (`../tests/references.rs` checks the count stays zero).
2. **No `long`.** `long` is 64 bits on Linux and 32 on Windows: upstream's
   `~0ul>>12` in cosh, sinh and tanh is a 20-bit mask on Windows instead
   of a 52-bit one, `1l << (ex + 1073)` in cos and tan shifts a 32-bit
   value by up to 51, `long dn` in asin truncates a 64-bit difference, and
   `dtoi` in `pow.h`, `2030l` in atan, the masks `(1<<26)-1l` and
   `MASK53` of atan2 are `long` too. Each becomes the 64-bit type it is on
   Linux (`ull`, `ll`, `int64_t`), so that Windows compiles the code Linux
   does. Before the patch, compiled with clang-cl's semantics, cosh, sinh
   and tanh did run other code on Windows (their tests for a hard case
   compared 20 bits instead of 52); no result differed on any of the 9.6
   million references below, before the patch or after.

`SHA256SUMS` holds the SHA-256 of every vendored file as patched
(`sha256sum -c SHA256SUMS` here checks them; the root `.gitattributes`
keeps them byte for byte in a Windows checkout). `../tests/vendored.rs`
checks the same sums, and that no file calls `exit` or `abort` or uses
`long`, an `l` or `ul` integer suffix or a builtin on `long`, so that a
new revision copied without the patches fails.

## How it is built

`../build.rs` compiles the files with `-ffp-contract=off` (no contraction
into fused multiply-adds the source does not write) and
`-fno-math-errno`, once with `-mfma` and once with an exact software fused
multiply-add (`../csrc`), renaming each `cr_<f>`; on Windows with
clang-cl (MSVC's cl lacks `__int128`). There upstream takes its
`_MSC_VER` branches: `roundeven` (in acos, asin, exp, sin and pow) is the
`vroundsd` instruction in the FMA build and a rounding in software in the
other, where Linux has `__builtin_roundeven`: the same values.

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
`upstream-report-draft.md` reports the regression to CORE-MATH's authors:
written, not sent.

With the patches, the same 9.6 million references: every result correctly
rounded on Linux (gcc 13: both builds, the C runtime's `fma` build and
the engine's entry points) and compiled as on Windows
(`engine/experiments/libm-eval/clang-cl/run.sh`: clang-cl 18's semantics,
both builds, 19.2 million results), October 2026. Windows CI runs the
crate's tests on every change.

## Updating

1. Copy the files above from the new revision into `binary64/`, apply
   `lsim.patch` (where a hunk no longer applies, make its change by hand
   and write the diff again: `diff -ruN a/binary64 b/binary64` with
   upstream's files in `a`), and rewrite `SHA256SUMS` (`find binary64
   -type f | LC_ALL=C sort | xargs sha256sum > SHA256SUMS`).
2. Run `engine/experiments/libm-eval` on the new revision's hard-to-round
   cases (all of them, every build) on Linux: every result must be the
   correctly rounded one.
3. The same compiled as on Windows: `clang-cl/run.sh refs` in
   `engine/experiments/libm-eval` on Linux, and on Windows itself `cargo
   run --release -- check refs/wc` there (and `refs/rand`,
   `refs/special`), which builds CORE-MATH with clang-cl as the engine
   does (copy the `refs` folder over).
4. Regenerate `../tests/data/hard_cases.txt` with `../tools/hard_cases.py`
   from the new checkout and run the crate's tests, here and on Windows
   CI.
5. Record the revision here, and what the patches had to change.
