/* Stand-in for the UCRT header of the same name: the declarations
   CORE-MATH's 13 files use, with the UCRT's types and constants, so
   that clang-cl compiles them as on Windows (run.sh). */
#pragma once
#define _HUGE_ENUF 1e+300
#define INFINITY ((float)(_HUGE_ENUF * _HUGE_ENUF))
#define NAN (-(float)(INFINITY * 0.0F))
double fma(double, double, double);
double sqrt(double);
double fabs(double);
