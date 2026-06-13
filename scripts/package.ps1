# package.ps1 — Build Kanae and assemble a self-contained dist/ folder.
#
# Usage (from repo root):
#   .\scripts\package.ps1 [-Variant gui|tui|hybrid] [-MakeInstaller]
#
# Variants:
#   gui     — Qt/QML only   (--no-default-features --features gui)
#   tui     — TUI only      (--no-default-features --features tui)
#   hybrid  — both          (default features)
#
# -MakeInstaller  — run makensis to produce an .exe installer (gui/hybrid only)
#
# Requires:
#   - Rust toolchain  (cargo)
#   - Qt 6.8.0 installed at 6.8.0\msvc2022_64\  (set via .cargo\config.toml)
#   - NSIS (makensis) on PATH for -MakeInstaller

param(
    [ValidateSet("gui","tui","hybrid")]
    [string]$Variant = "hybrid",
    [switch]$MakeInstaller
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$RepoRoot    = Split-Path $PSScriptRoot -Parent
$Dist        = Join-Path $RepoRoot "dist"
$QtBin       = Join-Path $RepoRoot "6.8.0\msvc2022_64\bin"
$WinDeployQt = Join-Path $QtBin "windeployqt6.exe"
$QmlSourceDir = Join-Path $RepoRoot "qml"

# ── 1. Build ──────────────────────────────────────────────────────────────────

$CargoFlags = switch ($Variant) {
    "gui"    { "--no-default-features --features gui" }
    "tui"    { "--no-default-features --features tui" }
    "hybrid" { "" }
}

Write-Host "`n==> Building release binary ($Variant)..." -ForegroundColor Cyan
Push-Location $RepoRoot
Invoke-Expression "cargo build --release $CargoFlags"
Pop-Location

# ── 2. Package TUI ────────────────────────────────────────────────────────────

if ($Variant -eq "tui") {
    $Out = Join-Path $RepoRoot "kanae-tui-windows-x64.zip"
    Compress-Archive -Path "$RepoRoot\target\release\kanae.exe" -DestinationPath $Out -Force
    Write-Host "`n==> Done.  $Out" -ForegroundColor Green
    return
}

# ── 3. Prepare dist/ for GUI/Hybrid ──────────────────────────────────────────

if (-not (Test-Path $WinDeployQt)) {
    Write-Error "windeployqt6.exe not found at $WinDeployQt"
}

Write-Host "`n==> Assembling dist/..." -ForegroundColor Cyan
if (Test-Path $Dist) { Remove-Item -Recurse -Force $Dist }
New-Item -ItemType Directory $Dist | Out-Null
Copy-Item "$RepoRoot\target\release\kanae.exe" $Dist

# ── 4. Qt deployment (DLLs + plugins + QML) ──────────────────────────────────

Write-Host "`n==> Running windeployqt6..." -ForegroundColor Cyan
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

# ── 5. NSIS installer (optional) ─────────────────────────────────────────────

if ($MakeInstaller) {
    Write-Host "`n==> Running makensis ($Variant)..." -ForegroundColor Cyan
    Push-Location $RepoRoot
    makensis /DVERSION=dev /DVARIANT=$Variant scripts\installer.nsi
    Pop-Location
    if ($LASTEXITCODE -ne 0) { Write-Error "makensis failed" }
    Write-Host "`n==> Installer created." -ForegroundColor Green
}

# ── 6. Summary ────────────────────────────────────────────────────────────────

$size = (Get-ChildItem $Dist -Recurse -File | Measure-Object Length -Sum).Sum
$mb   = [math]::Round($size / 1MB, 1)
Write-Host "`n==> Done.  dist/ is $mb MB ($Variant build)" -ForegroundColor Green
Write-Host "    $Dist" -ForegroundColor Green

