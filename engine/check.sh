#!/usr/bin/env bash
# The engine's CI gate: formatting, lints, tests and licences.
#
#   engine/check.sh            # everything
#   CARGO_BUILD_JOBS=4 ...     # more parallel jobs (default 2)
#
# Needs: Rust 1.94+ (rustfmt, clippy), a C compiler (the vendored SUNDIALS,
# built by lsim-sundials-sys with the cc crate: no CMake, no libclang; on
# Windows MSVC, and clang-cl for lsim-math's CORE-MATH: Visual Studio's
# "C++ Clang tools for Windows" or LLVM),
# Python 3 (for the licence check) and Python 3.11, the version the app
# ships, with the backend's requirements for the Script parity test
# (`LSIM_PYTHON`, else python3).
# cargo-deny is used too when installed.
set -euo pipefail
cd "$(dirname "$0")"
jobs="${CARGO_BUILD_JOBS:-2}"

step() { printf '\n== %s\n' "$*"; }

# the native Script runner reproduces CPython 3.11, the app's Python, message
# for message; another version words some of them otherwise (3.12: "max()
# iterable argument is empty"), and the parity test would report those
step "the Script parity test's Python"
"${LSIM_PYTHON:-python3}" -c 'import sys; v = sys.version_info[:2]; sys.exit(0 if v == (3, 11) else
    f"Python {v[0]}.{v[1]}: the Script parity test compares with CPython 3.11, the version the app ships; set LSIM_PYTHON to a Python 3.11 with the backend requirements")'

step "rustfmt"
cargo fmt --all --check

step "clippy (warnings are errors)"
cargo clippy --workspace --all-targets --locked -j "$jobs" -- -D warnings

step "tests"
cargo test --workspace --locked -j "$jobs"

step "licences of every third-party crate"
python3 scripts/licences.py

step "the outside judge's own scoring (engine/judge)"
python3 judge/test_judge.py

step "the hard-problem harness's transcriber (benchmarks/hard)"
python3 ../benchmarks/hard/test_fortran.py

if command -v cargo-deny >/dev/null 2>&1; then
    step "cargo-deny (licences, bans)"
    cargo deny --locked check licenses bans
fi

printf '\nengine checks passed\n'
