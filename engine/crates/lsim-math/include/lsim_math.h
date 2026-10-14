/* The engine's mathematical library, for C code built with the engine
   (lsim-sundials-sys routes SUNDIALS' calls here): the functions of the
   lsim-math crate, correctly rounded (CORE-MATH), the same on every
   platform and CPU. Defined in Rust (lsim-math's src/lib.rs). */
#ifndef LSIM_MATH_H
#define LSIM_MATH_H

double lsim_math_sin(double x);
double lsim_math_cos(double x);
double lsim_math_tan(double x);
double lsim_math_asin(double x);
double lsim_math_acos(double x);
double lsim_math_atan(double x);
double lsim_math_atan2(double y, double x);
double lsim_math_sinh(double x);
double lsim_math_cosh(double x);
double lsim_math_tanh(double x);
double lsim_math_exp(double x);
double lsim_math_log(double x);
double lsim_math_pow(double x, double y);
double lsim_math_fma(double x, double y, double z);

#endif
