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
# -Version        — version string stamped into artifact names (default: git describe)
#
# Requires:
#   - Rust toolchain  (cargo)
#   - Qt 6 — located via the QMAKE env var, qmake6/qmake on PATH, or a
#     repo-local 6.8.0\msvc2022_64\ install (in that order)
#   - NSIS (makensis) on PATH for -MakeInstaller

param(
    [ValidateSet("gui","tui","hybrid")]
    [string]$Variant = "hybrid",
    [switch]$MakeInstaller,
    [string]$Version = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$RepoRoot     = Split-Path $PSScriptRoot -Parent
$Dist         = Join-Path $RepoRoot "dist"
$QmlSourceDir = Join-Path $RepoRoot "qml"

if (-not $Version) {
    $Version = (git -C $RepoRoot describe --tags --always 2>$null)
    if (-not $Version) { $Version = "dev" }
}

# ── 0. Locate Qt ─────────────────────────────────────────────────────────────

$Qmake = $null
if ($env:QMAKE -and (Test-Path $env:QMAKE)) {
    $Qmake = $env:QMAKE
} else {
    $cmd = Get-Command qmake6, qmake -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($cmd) { $Qmake = $cmd.Source }
}
if (-not $Qmake) {
    $local = Join-Path $RepoRoot "6.8.0\msvc2022_64\bin\qmake.exe"
    if (Test-Path $local) { $Qmake = $local }
}
if (-not $Qmake -and $Variant -ne "tui") {
    Write-Error "Qt not found. Set QMAKE, put qmake6 on PATH, or install to 6.8.0\msvc2022_64\."
}

if ($Qmake) {
    $QtBin = Split-Path $Qmake -Parent
    $env:QMAKE = $Qmake   # let cxx-qt-build find the same Qt
    $WinDeployQt = @("windeployqt6.exe", "windeployqt.exe") |
        ForEach-Object { Join-Path $QtBin $_ } |
        Where-Object { Test-Path $_ } |
        Select-Object -First 1
}

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
    $Out = Join-Path $RepoRoot "kanae-tui-windows-x64-$Version.zip"
    Compress-Archive -Path "$RepoRoot\target\release\kanae.exe" -DestinationPath $Out -Force
    Write-Host "`n==> Done.  $Out" -ForegroundColor Green
    return
}

# ── 3. Prepare dist/ for GUI/Hybrid ──────────────────────────────────────────

if (-not $WinDeployQt) {
    Write-Error "windeployqt6.exe not found next to $Qmake"
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
    makensis /DVERSION=$Version /DVARIANT=$Variant scripts\installer.nsi
    Pop-Location
    if ($LASTEXITCODE -ne 0) { Write-Error "makensis failed" }
    Write-Host "`n==> Installer created." -ForegroundColor Green
}

# ── 6. Summary ────────────────────────────────────────────────────────────────

$size = (Get-ChildItem $Dist -Recurse -File | Measure-Object Length -Sum).Sum
$mb   = [math]::Round($size / 1MB, 1)
Write-Host "`n==> Done.  dist/ is $mb MB ($Variant build)" -ForegroundColor Green
Write-Host "    $Dist" -ForegroundColor Green

