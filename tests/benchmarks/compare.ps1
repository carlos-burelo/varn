<#
.SYNOPSIS
  Runtime comparison harness wrapper delegating to native Rust `cargo xtask compare`.

.DESCRIPTION
  Forwarding wrapper for the native Rust benchmark comparison harness.
  You can also invoke it directly via:
    cargo xtask compare [OPTIONS]
    cargo xtask bench [OPTIONS]

.PARAMETER Runs
  Repetitions per benchmark per runtime (default 7). Two are discarded as warmup.

.PARAMETER Only
  Run just the named benchmarks, e.g. -Only fib,matrix,dto.

.PARAMETER Baseline
  Path to a second vn.exe, measured as its own runtime named `varn-base`.

.PARAMETER SkipPython
  Skip Python benchmark execution.

.PARAMETER Compact
  One line per benchmark instead of the bar chart.

.PARAMETER Markdown
  Emit a Markdown table for docs/reports.

.PARAMETER Json
  Emit JSON output with detailed metrics.

.PARAMETER Detailed
  Include extended statistics (mean, stddev, P95).

.EXAMPLE
  .\tests\benchmarks\compare.ps1
  .\tests\benchmarks\compare.ps1 -Only fib,matrix -Runs 15
  .\tests\benchmarks\compare.ps1 -Compact
#>
[CmdletBinding()]
param(
    [int]$Runs = 7,
    [string[]]$Only,
    [string]$Baseline,
    [switch]$SkipPython,
    [switch]$Compact,
    [switch]$Markdown,
    [switch]$Json,
    [switch]$Detailed
)

$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$cargoArgs = @('xtask', 'compare')

if ($Runs -ne 7) { $cargoArgs += @('--runs', $Runs.ToString()) }
if ($Only) {
    $onlyStr = ($Only | ForEach-Object { $_ -split ',' } | ForEach-Object { $_.Trim() } | Where-Object { $_ }) -join ','
    $cargoArgs += @('--only', $onlyStr)
}
if ($Baseline) { $cargoArgs += @('--baseline', $Baseline) }
if ($SkipPython) { $cargoArgs += '--skip-python' }
if ($Compact) { $cargoArgs += '--compact' }
if ($Markdown) { $cargoArgs += '--markdown' }
if ($Json) { $cargoArgs += '--json' }
if ($Detailed) { $cargoArgs += '--detailed' }

Push-Location $root
try {
    & cargo @cargoArgs
}
finally {
    Pop-Location
}
