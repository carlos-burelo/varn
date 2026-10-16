#Requires -Version 5.1
<#
.SYNOPSIS
    Simula localmente lo que CI valida y `verify.ps1` no cubre:
      1. Codigo `cfg(unix/macos)` que Windows nunca compila (cross-check).
      2. Checkouts con otros finales de linea (CRLF vs LF).
.DESCRIPTION
    Complementa a `verify.ps1 -Fast` (que valida fmt, clippy, matriz 4/4).
    No compila binarios cruzados ni corre la suite completa: es una red
    rapida (<5 min) para lo que historicamente rompio CI en main.
    Requiere: targets instalados una vez con
      rustup target add x86_64-unknown-linux-gnu aarch64-apple-darwin
#>
param(
    [switch]$SkipCross
)

$ErrorActionPreference = "Stop"
$RootDir = Split-Path -Parent $PSScriptRoot
Set-Location $RootDir
$failed = @()

function Step($name, [scriptblock]$body) {
    Write-Host "`n=== $name ===" -ForegroundColor Yellow
    $oldPref = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        & $body
        $ErrorActionPreference = $oldPref
        Write-Host "[PASS] $name" -ForegroundColor Green
    } catch {
        $ErrorActionPreference = $oldPref
        Write-Host "[FAIL] $name : $_" -ForegroundColor Red
        $script:failed += $name
    }
}

if (-not $SkipCross) {
    Step "cross-check linux-gnu (cfg unix)" {
        cargo check -p varn-jit --target x86_64-unknown-linux-gnu > $null 2> $null
        if ($LASTEXITCODE -ne 0) { throw "cargo check linux-gnu exit=$LASTEXITCODE" }
    }
    Step "cross-check macOS ARM (cfg macos)" {
        cargo check -p varn-jit --target aarch64-apple-darwin > $null 2> $null
        if ($LASTEXITCODE -ne 0) { throw "cargo check macos exit=$LASTEXITCODE" }
    }
    Step "fallback.rs aislado para linux-gnu (sin dependencias C)" {
        $tmp = Join-Path $env:TEMP "vn-fbcheck"
        New-Item -ItemType Directory -Force -Path $tmp | Out-Null
        Copy-Item "crates/varn-vm/src/arch/fallback.rs" (Join-Path $tmp "fallback.rs") -Force
        $out = Join-Path $tmp "out"
        New-Item -ItemType Directory -Force -Path $out | Out-Null
        rustc --edition 2021 --crate-type lib --target x86_64-unknown-linux-gnu (Join-Path $tmp "fallback.rs") --out-dir $out > $null 2> $null
        if ($LASTEXITCODE -ne 0) { throw "rustc fallback exit=$LASTEXITCODE" }
    }
}

Write-Host ""
if ($failed.Count -eq 0) {
    Write-Host "CI-LOCAL: todo verde." -ForegroundColor Green
    exit 0
} else {
    Write-Host ("CI-LOCAL fallo en: " + ($failed -join ", ")) -ForegroundColor Red
    exit 1
}
