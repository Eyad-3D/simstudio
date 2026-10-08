#!/usr/bin/env bash
# The engine's CI gate: formatting, lints, tests and licences.
#
#   engine/check.sh            # everything
#   CARGO_BUILD_JOBS=4 ...     # more parallel jobs (default 2)
#
# Needs: Rust 1.94+ (rustfmt, clippy), CMake and a C compiler and libclang
# (the vendored SUNDIALS build; work package 4 drops CMake and libclang),
# Python 3 (for the licence check). cargo-deny is used too when installed.
set -euo pipefail
cd "$(dirname "$0")"
jobs="${CARGO_BUILD_JOBS:-2}"

step() { printf '\n== %s\n' "$*"; }

step "rustfmt"
cargo fmt --all --check

step "clippy (warnings are errors)"
cargo clippy --workspace --all-targets --locked -j "$jobs" -- -D warnings

step "tests"
cargo test --workspace --locked -j "$jobs"

step "licences of every third-party crate"
python3 scripts/licences.py

if command -v cargo-deny >/dev/null 2>&1; then
    step "cargo-deny (licences, bans)"
    cargo deny --locked check licenses bans
fi

printf '\nengine checks passed\n'
