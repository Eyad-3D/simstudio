/* Stand-in for the UCRT header of the same name: the declarations
   CORE-MATH's 13 files use, with the UCRT's types and constants, so
   that clang-cl compiles them as on Windows (run.sh). */
#pragma once
typedef struct _iobuf FILE;
FILE *__acrt_iob_func(unsigned);
#define stderr (__acrt_iob_func(2))
int printf(const char *, ...);
int fprintf(FILE *, const char *, ...);
int puts(const char *);
