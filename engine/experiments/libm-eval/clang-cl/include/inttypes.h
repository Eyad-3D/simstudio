/* Stand-in for the UCRT header of the same name: the declarations
   CORE-MATH's 13 files use, with the UCRT's types and constants, so
   that clang-cl compiles them as on Windows (run.sh). */
#pragma once
#include <stdint.h>
#define PRIx64 "llx"
#define PRId64 "lld"
#define PRIu64 "llu"
