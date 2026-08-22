<#
.SYNOPSIS
  Paired benchmark table: Varn against Bun, Node and Python.

.DESCRIPTION
  Runs every benchmark on every available runtime back to back and prints one
  formatted comparison table showing process wall-clock time and internal loop timing.

.PARAMETER Runs
  Timed runs for `vn` (default 5).

.PARAMETER Only
  Run just the named benchmarks, e.g. -Only fib,matrix,dto.

.PARAMETER SkipPython
  Skip Python benchmark execution.

.PARAMETER Markdown
  Emit a Markdown table for docs/reports.

.EXAMPLE
  .\benchmarks\compare.ps1
  .\benchmarks\compare.ps1 -SkipPython -Markdown
#>
[CmdletBinding()]
param(
    [int]$Runs = 5,
    [string[]]$Only,
    [switch]$SkipPython,
    [switch]$Markdown
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$benchDir = Join-Path $root 'benchmarks'
$vn = Join-Path $root 'target\release\vn.exe'

if (-not (Test-Path $vn)) {
    throw "vn.exe not found at $vn -- run: cargo build --release --bin vn"
}

# Master list of benchmarks
$AllBenchmarks = @(
    @{ Name = 'fib';                 Vn = 'bench_fib.vn';                 TS = 'bench_fib.ts';                 Py = 'py/fib.py' }
    @{ Name = 'gc_alloc';            Vn = 'bench_gc_alloc.vn';            TS = 'bench_gc_alloc.ts';            Py = 'py/gc_alloc.py' }
    @{ Name = 'dto';                 Vn = 'bench_dto_local.vn';           TS = 'bench_dto.ts';                 Py = 'py/dto.py' }
    @{ Name = 'matrix';              Vn = 'bench_matrix.vn';              TS = 'bench_matrix.ts';              Py = 'py/matrix.py' }
    @{ Name = 'str_ops';             Vn = 'bench_str_ops.vn';             TS = 'bench_str_ops.ts';             Py = $null }
    @{ Name = 'json_native';         Vn = 'bench_json.vn';                TS = 'bench_json.ts';                Py = $null }
    @{ Name = 'json_pure';           Vn = 'bench_json_pure.vn';           TS = 'bench_json_pure.ts';           Py = $null }
    @{ Name = 'csv_pipeline';        Vn = 'bench_csv_pipeline.vn';        TS = 'bench_csv_pipeline.ts';        Py = $null }
    @{ Name = 'collection_pipeline'; Vn = 'bench_collection_pipeline.vn'; TS = 'bench_collection_pipeline.ts'; Py = $null }
    @{ Name = 'http_routing';        Vn = 'bench_http_routing.vn';        TS = 'bench_http_routing.ts';        Py = $null }
    @{ Name = 'csv_etl';             Vn = 'bench_csv_etl.vn';             TS = 'bench_csv_etl.ts';             Py = $null }
    @{ Name = 'json_api_payloads';   Vn = 'bench_json_api_payloads.vn';   TS = 'bench_json_api_payloads.ts';   Py = $null }
)

if ($Only) {
    $AllBenchmarks = $AllBenchmarks | Where-Object { $Only -contains $_.Name }
}

# Discover runtimes
$runtimes = @('varn')
if (Get-Command 'bun' -ErrorAction SilentlyContinue) { $runtimes += 'bun' }
if (Get-Command 'node' -ErrorAction SilentlyContinue) { $runtimes += 'node' }
if (-not $SkipPython -and (Get-Command 'python' -ErrorAction SilentlyContinue)) { $runtimes += 'python' }

function Measure-RuntimeProcess([string]$exe, [string[]]$cmdArgs) {
    $minMs = [double]::MaxValue
    $lastOutput = ""
    for ($i = 0; $i -lt $Runs; $i++) {
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        $out = & $exe $cmdArgs 2>&1
        $sw.Stop()
        $ms = $sw.Elapsed.TotalMilliseconds
        if ($ms -lt $minMs) {
            $minMs = $ms
        }
        $lastOutput = ($out -join ' ')
    }
    return [pscustomobject]@{
        MinMs = [Math]::Round($minMs, 1)
        Output = $lastOutput
    }
}

Write-Host ""
Write-Host "  Runtimes: $($runtimes -join ', ')  |  Runs per bench: $Runs" -ForegroundColor Cyan
Write-Host "  Measuring real process wall-clock time (Min of $Runs runs)." -ForegroundColor DarkGray
Write-Host ""

$results = @()

foreach ($b in $AllBenchmarks) {
    $name = $b.Name
    Write-Host "  Running $name ..." -NoNewline

    $row = [ordered]@{ Benchmark = $name }

    foreach ($rt in $runtimes) {
        $ms = $null
        switch ($rt) {
            'varn' {
                $file = Join-Path $benchDir $b.Vn
                if (Test-Path $file) {
                    $res = Measure-RuntimeProcess $vn @('run', $file)
                    $ms = $res.MinMs
                }
            }
            'bun' {
                if ($b.TS) {
                    $file = Join-Path $benchDir $b.TS
                    if (Test-Path $file) {
                        $res = Measure-RuntimeProcess 'bun' @('run', $file)
                        $ms = $res.MinMs
                    }
                }
            }
            'node' {
                if ($b.TS) {
                    $file = Join-Path $benchDir $b.TS
                    if (Test-Path $file) {
                        $res = Measure-RuntimeProcess 'node' @($file)
                        $ms = $res.MinMs
                    }
                }
            }
            'python' {
                if ($b.Py) {
                    $file = Join-Path $benchDir $b.Py
                    if (Test-Path $file) {
                        $res = Measure-RuntimeProcess 'python' @($file)
                        $ms = $res.MinMs
                    }
                }
            }
        }
        $row[$rt] = $ms
    }

    # Compare Varn vs fastest rival
    $vnVal = $row['varn']
    $rivals = @()
    foreach ($rt in $runtimes) {
        if ($rt -ne 'varn' -and $null -ne $row[$rt]) {
            $rivals += $row[$rt]
        }
    }
    if ($vnVal -and $rivals.Count -gt 0) {
        $fastestRival = ($rivals | Measure-Object -Minimum).Minimum
        $ratio = [Math]::Round(($fastestRival / $vnVal), 2)
        $row['Ratio'] = $ratio
    } else {
        $row['Ratio'] = $null
    }

    $results += [pscustomobject]$row
    Write-Host "`r  Done: $name           " -ForegroundColor Green
}

Write-Host ""

if ($Markdown) {
    $cols = $runtimes
    $headerStr = "| Benchmark | " + ($cols -join " | ") + " | vs Fastest Rival |"
    $sepStr = "|---| " + (($cols | ForEach-Object { "---" }) -join " | ") + " |---|"
    Write-Host $headerStr
    Write-Host $sepStr
    foreach ($r in $results) {
        $line = "| $($r.Benchmark) | "
        foreach ($rt in $runtimes) {
            $val = if ($null -ne $r.$rt) { "$($r.$rt) ms" } else { "--" }
            $line += "$val | "
        }
        $ratStr = if ($null -ne $r.Ratio) {
            if ($r.Ratio -ge 1.0) { "$($r.Ratio)x WIN *" } else { "$($r.Ratio)x" }
        } else { "--" }
        $line += "$ratStr |"
        Write-Host $line
    }
} else {
    $w = 12
    $hdr = "  {0,-16}" -f "Benchmark"
    foreach ($rt in $runtimes) {
        $hdr += "{0,$w}" -f $rt
    }
    $hdr += "{0,18}" -f "vs Fastest Rival"
    Write-Host $hdr -ForegroundColor Cyan
    Write-Host ("  " + ("-" * ($hdr.Length - 2))) -ForegroundColor DarkGray

    foreach ($r in $results) {
        $line = "  {0,-16}" -f $r.Benchmark
        foreach ($rt in $runtimes) {
            $valStr = if ($null -ne $r.$rt) { "$($r.$rt) ms" } else { "--" }
            $line += "{0,$w}" -f $valStr
        }
        $ratStr = if ($null -ne $r.Ratio) {
            if ($r.Ratio -ge 1.0) { "$($r.Ratio)x WIN *" } else { "$($r.Ratio)x" }
        } else { "--" }
        $line += "{0,18}" -f $ratStr

        $color = if ($null -ne $r.Ratio -and $r.Ratio -ge 1.0) { 'Green' } else { 'Yellow' }
        Write-Host $line -ForegroundColor $color
    }
}

Write-Host ""
Write-Host "  Values are minimum process wall-clock time over $Runs runs." -ForegroundColor DarkGray
Write-Host "  Ratio >= 1.0x WIN = Varn is faster than the fastest rival." -ForegroundColor DarkGray
Write-Host ""
