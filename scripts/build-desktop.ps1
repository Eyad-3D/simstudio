# Build the LightSim desktop app end to end on Windows.
#
#   .\scripts\build-desktop.ps1           # builds the .exe installer
#   .\scripts\build-desktop.ps1 -Dir      # unpacked app only (fast, for testing)
param([switch]$Dir)
$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
$python = if ($env:PYTHON) { $env:PYTHON } else { "python" }

Write-Host "==> 1/4  Building the UI"
Push-Location "$root\frontend"
npm install
npm run build
Pop-Location

# The new engine (Rust, engine/): the app runs every case on its module,
# lightsim_engine, which the frozen backend must carry. Needs Rust (rustup,
# https://rustup.rs), which installs the release engine/rust-toolchain.toml pins.
Write-Host "==> 2/4  Building the new engine's module"
Push-Location "$root\engine"
rustup toolchain install --no-self-update
if ($LASTEXITCODE -ne 0) { Pop-Location; exit $LASTEXITCODE }
& $python -m pip install "maturin>=1.7,<2" numpy
Remove-Item -Recurse -Force wheels -ErrorAction SilentlyContinue
& $python -m maturin build --release -m crates/lsim-py/Cargo.toml --out wheels
if ($LASTEXITCODE -ne 0) { Pop-Location; exit $LASTEXITCODE }
& $python -m pip install --force-reinstall --no-deps --no-index --find-links wheels lightsim-engine
if ($LASTEXITCODE -ne 0) { Pop-Location; exit $LASTEXITCODE }
Pop-Location

# LIGHTSIM_FREEZE_ENGINE=1 stops the freeze when the module is not installed
Write-Host "==> 3/4  Freezing the backend"
Push-Location "$root\backend"
& $python -m pip install -r requirements-build.txt
Remove-Item -Recurse -Force build, dist -ErrorAction SilentlyContinue
$env:LIGHTSIM_FREEZE_ENGINE = "1"
& $python -m PyInstaller --noconfirm --distpath dist --workpath build lightsim-backend.spec
$freezeCode = $LASTEXITCODE
Remove-Item Env:LIGHTSIM_FREEZE_ENGINE
if ($freezeCode -ne 0) { Pop-Location; exit $freezeCode }
Pop-Location

Write-Host "==> 4/4  Packaging the desktop app"
Push-Location "$root\desktop"
npm install
# Licence check, and THIRD-PARTY-NOTICES.txt for the installer.
& $python "$root\scripts\third-party-notices.py"
if ($LASTEXITCODE -ne 0) { Pop-Location; exit $LASTEXITCODE }
if ($Dir) { npx electron-builder --dir } else { npx electron-builder --win }
Pop-Location

Write-Host ""
Write-Host "Done. Output is in desktop\release\"
