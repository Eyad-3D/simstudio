/* Stand-in for the UCRT header of the same name: the declarations
   CORE-MATH's 13 files use, with the UCRT's types and constants, so
   that clang-cl compiles them as on Windows (run.sh). */
#pragma once
typedef unsigned long fexcept_t;
typedef struct fenv_t { unsigned long _Fe_ctl, _Fe_stat; } fenv_t;
#define FE_INEXACT 0x01
#define FE_UNDERFLOW 0x02
#define FE_OVERFLOW 0x04
#define FE_DIVBYZERO 0x08
#define FE_INVALID 0x10
#define FE_ALL_EXCEPT 0x1f
#define FE_TONEAREST 0x0000
#define FE_UPWARD 0x0100
#define FE_DOWNWARD 0x0200
#define FE_TOWARDZERO 0x0300
int fegetround(void);
int feraiseexcept(int);
int fetestexcept(int);
int feclearexcept(int);
int feholdexcept(fenv_t *);
int feupdateenv(const fenv_t *);
int fegetexceptflag(fexcept_t *, int);
int fesetexceptflag(const fexcept_t *, int);
