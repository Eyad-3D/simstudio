# Draft: report to CORE-MATH (not sent)

A report of the exp regression for CORE-MATH's authors
(core-math@inria.fr, or an issue at
<https://gitlab.inria.fr/core-math/core-math/-/issues>), written in
October 2026 and not sent: whether and when to send it is the owner's
decision. It names no one and nothing of the engine but that it vendors
the functions.

---

**Subject: binary64 exp: three hard cases misrounded since 37384b5**

Hello,

Since commit 37384b5 ("[exp] reduce the number of exceptional cases from
61 to 21", 14 October 2026), `cr_exp` in `src/binary64/exp/exp.c` returns
the double next to the correctly rounded result for three inputs, in
rounding to nearest; the head of the same day (d768e65, "[exp] factor
common code") does too, and the previous commit (e78d268) rounds all
three correctly:

| x | correctly rounded exp(x) | 37384b5 and d768e65 | exact value |
|---|---|---|---|
| -0x1.07e05e6e03b83p-2 | 0x1.8bb0d03b36f1cp-1 | 0x1.8bb0d03b36f1dp-1 | 1.3e-16 ulp below the midpoint |
| -0x1.40a990279c2c0p-6 | 0x1.f613a44b93cdcp-1 | 0x1.f613a44b93cddp-1 | 1.5e-16 ulp below the midpoint |
| 0x1.aca7ae8da5a7bp+0 | 0x1.557d4acd7e557p+2 | 0x1.557d4acd7e556p+2 | 2.8e-17 ulp above the midpoint |

(The midpoint between the correctly rounded result and the one returned;
the exact values from MPFR at 400 bits and mpmath, which agree.)

Reproduced with gcc 13.3 (`-O2 -ffp-contract=off -fno-math-errno`, with
and without `-mfma`) and clang 18 (`-mfma`) on x86-64 Linux, compiling
`exp.c` alone and calling `cr_exp` from a small program:

```c
#include <stdio.h>
double cr_exp (double);
int main (void) {
  double x[3] = {-0x1.07e05e6e03b83p-2, -0x1.40a990279c2c0p-6, 0x1.aca7ae8da5a7bp+0};
  for (int i = 0; i < 3; i++) printf ("exp(%a) = %a\n", x[i], cr_exp (x[i]));
}
```

The three inputs are among the hard-to-round cases of
`src/binary64/exp/exp.wc`. Neither revision lists them in `db`;
37384b5 rounds its tables of 2^(i/2^6) to nearest, so perhaps they now
need an entry there. We found them running every input of the `.wc`
files of the 13 binary64 functions we use against MPFR: the other
1 129 429 inputs of `exp.wc` (at d768e65) are correctly rounded at
37384b5 and d768e65, and all 1 129 432 at e78d268.

Thank you for CORE-MATH.
