# ============================================================
# build.ps1 — one-command local build for rJSON (Windows / PowerShell)
# Member 3 owns this file.
#
# Usage:  .\build.ps1
#
# Requirements: Rust toolchain managed by rustup.
#   The channel is read from rJSON\rust-toolchain.toml automatically.
#
# What this does:
#   1. Compiles the rJSON Rust crate (both cdylib and rlib).
#   2. Runs the Rust-side port tests.
#
# What this deliberately does NOT do:
#   - Link or run tests\original\ directly (those C tests are compiled against
#     rjson.dll via the self-contained Docker build or tests\adapter\).
#   - Touch anything under rJSON\src\ or rJSON\tests\original*.
#   - Fetch or modify \cJSON (intentionally gitignored, read-only).
# ============================================================

$ErrorActionPreference = 'Stop'

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Definition
$RjsonDir  = Join-Path $ScriptDir 'rJSON'

if (-not (Test-Path $RjsonDir -PathType Container)) {
    Write-Error "ERROR: rJSON\ directory not found at $ScriptDir"
    exit 1
}

Write-Host "=== rJSON build ===" -ForegroundColor Cyan
Write-Host "Crate: $RjsonDir"

Push-Location $RjsonDir
try {
    # rustup writes informational lines to stderr during channel sync; lower the
    # preference temporarily so PS Stop mode doesn't treat those as terminating errors.
    $saved = $ErrorActionPreference
    $ErrorActionPreference = 'SilentlyContinue'
    $toolchainRaw = rustup show active-toolchain 2>&1
    $ErrorActionPreference = $saved
    $toolchainRaw = ($toolchainRaw | Where-Object { $_ -notmatch '^info:' }) -join ''
    if ($toolchainRaw) { $toolchain = $toolchainRaw } else { $toolchain = '(rustup not found — install from https://rustup.rs)' }
    Write-Host "Toolchain: $toolchain"
    Write-Host ""

    Write-Host "--- cargo build ---" -ForegroundColor Yellow
    # Use cmd /c so cargo's stderr flows to the console without PS treating it as an error
    cmd /c 'cargo build 2>&1'
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed (exit $LASTEXITCODE)" }
    Write-Host ""

    Write-Host "--- cargo test ---" -ForegroundColor Yellow
    cmd /c 'cargo test 2>&1'
    if ($LASTEXITCODE -ne 0) { throw "cargo test failed (exit $LASTEXITCODE)" }
    Write-Host ""

    Write-Host "=== Done. Build artifact: rJSON\target\debug\rjson.dll ===" -ForegroundColor Green
}
finally {
    Pop-Location
}
