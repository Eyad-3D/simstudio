#!/usr/bin/env bash
# Build the SimStudio desktop app end to end.
#
#   ./scripts/build-desktop.sh            # installers for the current OS
#   ./scripts/build-desktop.sh --dir      # unpacked app only (fast, for testing)
#
# Runs on Linux and macOS. Windows users: see scripts/build-desktop.ps1.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PYTHON="${PYTHON:-python3}"

echo "==> 1/3  Building the UI"
cd "$ROOT/frontend"
npm ci || npm install
npm run build

echo "==> 2/3  Freezing the backend"
cd "$ROOT/backend"
"$PYTHON" -m pip install -r requirements-build.txt
rm -rf build dist
"$PYTHON" -m PyInstaller --noconfirm --distpath dist --workpath build simstudio-backend.spec

echo "==> 3/3  Packaging the desktop app"
cd "$ROOT/desktop"
npm install
if [ "${1:-}" = "--dir" ]; then
  npx electron-builder --dir
else
  npx electron-builder
fi

echo
echo "Done. Output is in desktop/release/"
