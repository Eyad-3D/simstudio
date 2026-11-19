/* Stand-in for the UCRT header of the same name: the declarations
   CORE-MATH's 13 files use, with the UCRT's types and constants, so
   that clang-cl compiles them as on Windows (run.sh). */
#pragma once
int *_errno(void);
#define errno (*_errno())
#define EDOM 33
#define ERANGE 34
