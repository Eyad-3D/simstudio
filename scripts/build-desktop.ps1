# Build the LightSim desktop app end to end on Windows.
#
#   .\scripts\build-desktop.ps1           # builds the .exe installer
#   .\scripts\build-desktop.ps1 -Dir      # unpacked app only (fast, for testing)
param([switch]$Dir)
$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
$python = if ($env:PYTHON) { $env:PYTHON } else { "python" }

Write-Host "==> 1/3  Building the UI"
Push-Location "$root\frontend"
npm install
npm run build
Pop-Location

Write-Host "==> 2/3  Freezing the backend"
Push-Location "$root\backend"
& $python -m pip install -r requirements-build.txt
Remove-Item -Recurse -Force build, dist -ErrorAction SilentlyContinue
& $python -m PyInstaller --noconfirm --distpath dist --workpath build lightsim-backend.spec
Pop-Location

Write-Host "==> 3/3  Packaging the desktop app"
Push-Location "$root\desktop"
npm install
# Licence check, and THIRD-PARTY-NOTICES.txt for the installer.
& $python "$root\scripts\third-party-notices.py"
if ($LASTEXITCODE -ne 0) { Pop-Location; exit $LASTEXITCODE }
if ($Dir) { npx electron-builder --dir } else { npx electron-builder --win }
Pop-Location

Write-Host ""
Write-Host "Done. Output is in desktop\release\"
