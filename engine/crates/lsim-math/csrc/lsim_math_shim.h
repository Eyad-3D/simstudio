/* Included first into every CORE-MATH file the engine builds (build.rs).

   CORE-MATH asks for fused multiply-adds with __builtin_fma. The build for
   CPUs with the FMA instruction (-mfma) gets the instruction; the build for
   CPUs without it (LSIM_SOFT_FMA) would get a call to the C runtime's
   fma, so it gets lsim_math_fma_soft instead: exact in software
   (lsim_math_fma.c), the same value as the instruction, and no
   dependence on the C runtime's fma. Both builds therefore compute the
   very same numbers. */
#ifndef LSIM_MATH_SHIM_H
#define LSIM_MATH_SHIM_H

double lsim_math_fma_soft(double x, double y, double z);

#ifdef LSIM_SOFT_FMA
#define __builtin_fma(a, b, c) lsim_math_fma_soft((a), (b), (c))
#endif

#endif
