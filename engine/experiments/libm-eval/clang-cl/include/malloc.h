/* Stand-in for the UCRT header of the same name, which clang's
   <x86intrin.h> includes on Windows (mm_malloc.h), so that clang-cl
   compiles CORE-MATH's files as on Windows (run.sh). */
#pragma once
#include <stddef.h>
void *_aligned_malloc(size_t, size_t);
void _aligned_free(void *);
void *malloc(size_t);
void free(void *);
