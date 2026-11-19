/* Included first into every C file of the SUNDIALS build (build.rs).

   SUNDIALS calls the C library's mathematical functions where it chooses
   steps and orders (pow in CVODE's and IDA's step-size and order
   selection, in IDA's convergence rate, in the tolerances it derives from
   the unit roundoff; exp in KINSOL). The C runtime's results differ
   between platforms and CPUs in the last place, and a different step
   changes a run's result. Every such call goes to the engine's own library
   instead (lsim-math: correctly rounded, the same bits everywhere).

   sqrt, fabs, ceil, floor, fmax, fmin and the like are exact in IEEE
   arithmetic and stay the C runtime's. Any other function of <math.h> is
   mapped to a name nothing defines, so that a new call of one fails to
   build instead of quietly calling the C runtime. */
#ifndef LSIM_SUNDIALS_MATH_H
#define LSIM_SUNDIALS_MATH_H

#include <math.h>
#include "lsim_math.h"

#define sin(x) lsim_math_sin(x)
#define cos(x) lsim_math_cos(x)
#define tan(x) lsim_math_tan(x)
#define asin(x) lsim_math_asin(x)
#define acos(x) lsim_math_acos(x)
#define atan(x) lsim_math_atan(x)
#define atan2(y, x) lsim_math_atan2((y), (x))
#define sinh(x) lsim_math_sinh(x)
#define cosh(x) lsim_math_cosh(x)
#define tanh(x) lsim_math_tanh(x)
#define exp(x) lsim_math_exp(x)
#define log(x) lsim_math_log(x)
#define pow(x, y) lsim_math_pow((x), (y))
#define fma(x, y, z) lsim_math_fma((x), (y), (z))

/* not in the engine's library: a call does not build */
#define asinh(x) lsim_math_has_no_asinh(x)
#define acosh(x) lsim_math_has_no_acosh(x)
#define atanh(x) lsim_math_has_no_atanh(x)
#define exp2(x) lsim_math_has_no_exp2(x)
#define expm1(x) lsim_math_has_no_expm1(x)
#define log2(x) lsim_math_has_no_log2(x)
#define log10(x) lsim_math_has_no_log10(x)
#define log1p(x) lsim_math_has_no_log1p(x)
#define cbrt(x) lsim_math_has_no_cbrt(x)
#define hypot(x, y) lsim_math_has_no_hypot((x), (y))
#define erf(x) lsim_math_has_no_erf(x)
#define erfc(x) lsim_math_has_no_erfc(x)
#define tgamma(x) lsim_math_has_no_tgamma(x)
#define lgamma(x) lsim_math_has_no_lgamma(x)
#define powf(x, y) lsim_math_has_no_powf((x), (y))
#define expf(x) lsim_math_has_no_expf(x)
#define logf(x) lsim_math_has_no_logf(x)
#define powl(x, y) lsim_math_has_no_powl((x), (y))
#define expl(x) lsim_math_has_no_expl(x)
#define logl(x) lsim_math_has_no_logl(x)

#endif
