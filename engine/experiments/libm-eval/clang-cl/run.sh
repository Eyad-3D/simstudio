#!/usr/bin/env bash
# Runs CORE-MATH, compiled as the engine compiles it on Windows, on every
# reference of libm-eval, here on Linux.
#
#   clang-cl/run.sh REFS [SRC]
#
# REFS: the folder gen_refs.py writes (its special, rand and wc folders are
# read); SRC: a binary64 folder, by default the engine's vendored one.
#
# Each file is compiled by clang-cl with the command line the cc crate
# gives it in lsim-math's build.rs on Windows (captured from a cross build:
# -MD -O2 -Brepro --target=x86_64-pc-windows-msvc …), the FMA build and
# the software-fma build, against stand-ins for the UCRT's headers
# (include/), to LLVM IR. That IR has clang-cl's semantics: 32-bit long,
# _MSC_VER and the branches CORE-MATH takes for it, MS volatile. It is
# then retargeted to x86-64 Linux (the data layouts differ only in symbol
# mangling) and compiled to run here, the floating-point environment's
# calls going to shims with the UCRT's constants (harness.c). What this
# does not cover is Windows' own code generation and C runtime (the
# calling convention, floor, round, fmin, fmax: exact operations), which
# Windows CI runs.
#
# Needs clang-cl (CLANG_CL, by default clang-cl), clang and ar.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
refs=$1
src=${2:-$here/../../../crates/lsim-math/core-math/binary64}
csrc=$here/../../../crates/lsim-math/csrc
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT
export INCLUDE=$here/include
F="sin cos tan asin acos atan atan2 sinh cosh tanh exp log pow"
for tag in winF winS; do
  mkdir -p "$out/$tag"
  extra=(); defs=(); incs=(); files=()
  [ $tag = winF ] && extra+=(/clang:-mfma)
  [ $tag = winS ] && extra+=(-DLSIM_SOFT_FMA)
  for f in $F; do defs+=("-Dcr_$f=${tag}_$f"); incs+=(-I "$src/$f"); files+=("$src/$f/$f.c"); done
  [ $tag = winS ] && files+=("$csrc/lsim_math_fma.c")
  for file in "${files[@]}"; do
    b=$(basename "$file" .c)
    # (clang-cl writes the IR for /clang:-S to the current folder)
    (cd "$out/$tag" && "${CLANG_CL:-clang-cl}" -nologo -MD -O2 -Brepro \
      --target=x86_64-pc-windows-msvc "${incs[@]}" -W0 /clang:-ffp-contract=off \
      /clang:-fno-math-errno "${extra[@]}" "/FI$csrc/lsim_math_shim.h" "${defs[@]}" \
      /clang:-S /clang:-emit-llvm -c -- "$file")
    sed -i -e 's/^target triple = .*/target triple = "x86_64-unknown-linux-gnu"/' \
      -e 's/^target datalayout = "e-m:w-/target datalayout = "e-m:e-/' \
      -e '/^!llvm.linker.options = /d' \
      -e 's/@\(fegetround\|feraiseexcept\|fetestexcept\|feclearexcept\|feholdexcept\|feupdateenv\|fegetexceptflag\|fesetexceptflag\)\b/@ucrt_\1/g' \
      "$out/$tag/$b.ll"
    clang -c -O2 -fPIC -Wno-override-module "$out/$tag/$b.ll" -o "$out/$tag/$b.o"
  done
  ar rcs "$out/lib$tag.a" "$out/$tag"/*.o
done
clang -O2 "$here/harness.c" -o "$out/harness" "$out/libwinF.a" "$out/libwinS.a" -lm
"$out/harness" "$refs/special" "$refs/rand" "$refs/wc"
