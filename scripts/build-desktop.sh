#!/usr/bin/env bash
# Build the LightSim desktop app end to end.
#
#   ./scripts/build-desktop.sh            # installers for the current OS
#   ./scripts/build-desktop.sh --dir      # unpacked app only (fast, for testing)
#
# Runs on Linux and macOS (Apple silicon). Windows users: see
# scripts/build-desktop.ps1. A Mac build made here is unsigned: run it on your
# own Mac only. Published Mac builds are signed and notarised by the release
# workflow (.github/workflows/desktop-build.yml).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PYTHON="${PYTHON:-python3}"

echo "==> 1/4  Building the UI"
cd "$ROOT/frontend"
npm ci || npm install
npm run build

# The new engine (Rust, engine/): the app runs every case on its module,
# lightsim_engine, which the frozen backend must carry. Needs Rust (rustup,
# https://rustup.rs), which installs the release engine/rust-toolchain.toml pins.
echo "==> 2/4  Building the new engine's module"
cd "$ROOT/engine"
rustup toolchain install --no-self-update
"$PYTHON" -m pip install "maturin>=1.7,<2" numpy
rm -rf wheels
"$PYTHON" -m maturin build --release -m crates/lsim-py/Cargo.toml --out wheels
"$PYTHON" -m pip install --force-reinstall --no-deps --no-index --find-links wheels lightsim-engine

echo "==> 3/4  Freezing the backend"
cd "$ROOT/backend"
"$PYTHON" -m pip install -r requirements-build.txt
rm -rf build dist
# LIGHTSIM_FREEZE_ENGINE=1 stops the freeze when the module is not installed
LIGHTSIM_FREEZE_ENGINE=1 "$PYTHON" -m PyInstaller --noconfirm --distpath dist --workpath build lightsim-backend.spec

echo "==> 4/4  Packaging the desktop app"
cd "$ROOT/desktop"
npm install
# Licence check, and THIRD-PARTY-NOTICES.txt for the installer.
"$PYTHON" "$ROOT/scripts/third-party-notices.py"
if [ "${1:-}" = "--dir" ]; then
  npx electron-builder --dir
else
  npx electron-builder
fi

echo
echo "Done. Output is in desktop/release/"
