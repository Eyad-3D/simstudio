/* -----------------------------------------------------------------
 * SUNDIALS export macros for LightSim's in-tree build: a static library
 * on every platform, so nothing is exported or imported (what CMake's
 * GenerateExportHeader writes for SUNDIALS_STATIC_DEFINE).
 * SPDX-License-Identifier: BSD-3-Clause
 * -----------------------------------------------------------------*/

#ifndef SUNDIALS_EXPORT_H
#define SUNDIALS_EXPORT_H

#define SUNDIALS_EXPORT
#define SUNDIALS_NO_EXPORT

#ifndef SUNDIALS_DEPRECATED
#if defined(_MSC_VER) && !defined(__clang__)
#define SUNDIALS_DEPRECATED __declspec(deprecated)
#else
#define SUNDIALS_DEPRECATED __attribute__((__deprecated__))
#endif
#endif

#ifndef SUNDIALS_DEPRECATED_EXPORT
#define SUNDIALS_DEPRECATED_EXPORT SUNDIALS_EXPORT SUNDIALS_DEPRECATED
#endif

#ifndef SUNDIALS_DEPRECATED_NO_EXPORT
#define SUNDIALS_DEPRECATED_NO_EXPORT SUNDIALS_NO_EXPORT SUNDIALS_DEPRECATED
#endif

#endif /* SUNDIALS_EXPORT_H */
