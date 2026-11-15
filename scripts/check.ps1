#!/usr/bin/env pwsh
# Runs every quality gate of the project. CI runs this same script.
#
#   scripts/check.ps1          all gates
#   scripts/check.ps1 -Fast    everything except mutation testing
param([switch]$Fast)

$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot -Parent)

function Step([string]$Name, [scriptblock]$Command) {
    Write-Host "`n== $Name" -ForegroundColor Cyan
    & $Command
    if ($LASTEXITCODE -ne 0) {
        Write-Host "FAILED: $Name" -ForegroundColor Red
        exit 1
    }
}

Step 'format' { cargo fmt --all --check }
Step 'lint' { cargo clippy --workspace --all-targets -- -D warnings }
Step 'ARM64 type check' { cargo check --workspace --all-targets --target aarch64-pc-windows-msvc }
Step 'tests with coverage' { cargo llvm-cov --workspace --lcov --output-path target/lcov.info }
Step 'CRAP at most 10' { cargo crap --workspace --lcov target/lcov.info --threshold 10 --fail-above }
Step 'no duplicated functions' {
    # 0.7 on cargo-dupes' scale: a copy with renamed names, a changed constant
    # and one added line scores 0.72. See AGENTS.md.
    cargo dupes check --threshold 0.7 --exclude-tests --max-exact 0 --max-near 0
}
if (-not $Fast) {
    Step 'mutation testing' { cargo mutants --no-shuffle }
}

Write-Host "`nAll checks passed." -ForegroundColor Green
