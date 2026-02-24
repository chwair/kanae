# package.ps1 — Build Kanae and assemble a self-contained dist/ folder.
#
# Usage (from repo root):
#   .\scripts\package.ps1
#
# Requires:
#   - Rust toolchain  (cargo)
#   - Qt 6.8.0 installed at 6.8.0\msvc2022_64\  (set via .cargo\config.toml)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$RepoRoot      = Split-Path $PSScriptRoot -Parent
$Dist          = Join-Path $RepoRoot "dist"
$QtBin         = Join-Path $RepoRoot "6.8.0\msvc2022_64\bin"
$WinDeployQt   = Join-Path $QtBin "windeployqt6.exe"
$QmlSourceDir  = Join-Path $RepoRoot "qml"

if (-not (Test-Path $WinDeployQt)) {
    Write-Error "windeployqt6.exe not found at $WinDeployQt"
}

# ── 1. Build ──────────────────────────────────────────────────────────────────

Write-Host "`n==> Building release binary..." -ForegroundColor Cyan
Push-Location $RepoRoot
cargo build --release
Pop-Location

# ── 2. Prepare dist/ ─────────────────────────────────────────────────────────

Write-Host "`n==> Assembling dist/..." -ForegroundColor Cyan
if (Test-Path $Dist) { Remove-Item -Recurse -Force $Dist }
New-Item -ItemType Directory $Dist | Out-Null

Copy-Item "$RepoRoot\target\release\kanae.exe" $Dist

# ── 3. Qt deployment (DLLs + plugins + QML) ───────────────────────────────────
# windeployqt6 scans the binary for Qt imports and runs qmlimportscanner against
# --qmldir to deploy only the QML modules the app actually uses.

Write-Host "`n==> Running windeployqt6..." -ForegroundColor Cyan

# Add Qt's own bin to PATH so windeployqt6 can find Qt DLLs while running.
$env:PATH = "$QtBin;$env:PATH"

& $WinDeployQt `
    --release `
    --no-translations `
    --no-system-d3d-compiler `
    --no-compiler-runtime `
    --qmldir $QmlSourceDir `
    "$Dist\kanae.exe"

if ($LASTEXITCODE -ne 0) {
    Write-Error "windeployqt6 failed with exit code $LASTEXITCODE"
}

# ── 4. Summary ────────────────────────────────────────────────────────────────

$size = (Get-ChildItem $Dist -Recurse -File | Measure-Object Length -Sum).Sum
$mb   = [math]::Round($size / 1MB, 1)
Write-Host "`n==> Done.  dist/ is $mb MB" -ForegroundColor Green
Write-Host "    $Dist" -ForegroundColor Green
