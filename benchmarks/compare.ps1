<#
.SYNOPSIS
  Paired benchmark comparison: Varn against Bun, Node and Python, split by phase.

.DESCRIPTION
  Process wall-clock time is two things added together — the time a runtime
  needs to exist at all, and the time it spends on the program. Reporting only
  the sum lets one hide behind the other, and for short benchmarks the sum is
  mostly startup.

  Varn starts in roughly half the time Bun does, so that difference is added to
  every row. A benchmark can show Varn "winning" on total while the rival does
  the actual work several times faster.

  So each runtime is calibrated once on an empty program, and every result is
  reported as `startup + work`. The verdict column compares WORK, because that
  is what a language change can move; startup is reported next to it because it
  is a real advantage, just a different one.

.PARAMETER Runs
  Timed runs per benchmark, per runtime (default 5). The minimum is kept:
  wall-clock noise is one-sided, so the fastest run is the closest to the
  machine's actual capability.

.PARAMETER Only
  Run just the named benchmarks, e.g. -Only fib,matrix,dto.

.PARAMETER SkipPython
  Skip Python benchmark execution.

.PARAMETER Compact
  Print the one-line-per-benchmark table instead of the bar chart.

.PARAMETER Markdown
  Emit a Markdown table for docs/reports.

.EXAMPLE
  .\benchmarks\compare.ps1
  .\benchmarks\compare.ps1 -Only fib,matrix
  .\benchmarks\compare.ps1 -SkipPython -Compact
#>
[CmdletBinding()]
param(
    [int]$Runs = 5,
    [string[]]$Only,
    [switch]$SkipPython,
    [switch]$Compact,
    [switch]$Markdown
)

$ErrorActionPreference = 'Continue'
$root = Split-Path -Parent $PSScriptRoot
$benchDir = Join-Path $root 'benchmarks'
$vn = Join-Path $root 'target\release\vn.exe'

if (-not (Test-Path $vn)) {
    throw "vn.exe not found at $vn -- run: cargo build --release --bin vn"
}

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
    # `pwsh -File script.ps1 -Only fib,matrix` hands the whole thing over as a
    # single string, while `-Command` and dot-sourcing bind a real array. Split
    # on commas so both invocations behave the same.
    $Only = @($Only | ForEach-Object { $_ -split ',' } | ForEach-Object { $_.Trim() } | Where-Object { $_ })
    $AllBenchmarks = @($AllBenchmarks | Where-Object { $Only -contains $_.Name })
    if (-not $AllBenchmarks) { throw "no benchmark matched: $($Only -join ', ')" }
}

$runtimes = @('varn')
if (Get-Command 'bun' -ErrorAction SilentlyContinue) { $runtimes += 'bun' }
if (Get-Command 'node' -ErrorAction SilentlyContinue) { $runtimes += 'node' }
if (-not $SkipPython -and (Get-Command 'python' -ErrorAction SilentlyContinue)) { $runtimes += 'python' }

# How each runtime is invoked, and what an empty program looks like for it.
# The startup probe MUST go through the same pipeline as the benchmarks — same
# extension, same launcher — or it measures a path the benchmarks never take.
$Invoke = @{
    varn   = { param($f) & $vn 'run' $f }
    bun    = { param($f) & 'bun' 'run' $f }
    node   = { param($f) & 'node' $f }
    python = { param($f) & 'python' $f }
}
$EmptyProgram = @{
    varn   = @{ Ext = '.vn'; Body = 'print(1)' }
    bun    = @{ Ext = '.ts'; Body = 'console.log(1)' }
    node   = @{ Ext = '.ts'; Body = 'console.log(1)' }
    python = @{ Ext = '.py'; Body = 'print(1)' }
}

function Measure-Min([string]$rt, [string]$file) {
    $minMs = [double]::MaxValue
    for ($i = 0; $i -lt $Runs; $i++) {
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        & $Invoke[$rt] $file | Out-Null
        $sw.Stop()
        if ($sw.Elapsed.TotalMilliseconds -lt $minMs) { $minMs = $sw.Elapsed.TotalMilliseconds }
    }
    return [Math]::Round($minMs, 1)
}

# ---------------------------------------------------------------- calibration

Write-Host ""
Write-Host "  Runtimes: $($runtimes -join ', ')  |  Runs per bench: $Runs" -ForegroundColor Cyan
Write-Host "  Calibrating startup on an empty program ..." -ForegroundColor DarkGray

$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("varn-bench-" + [Guid]::NewGuid().ToString('N').Substring(0, 8))
New-Item -ItemType Directory -Path $tmp -Force | Out-Null
$startup = @{}
try {
    foreach ($rt in $runtimes) {
        $spec = $EmptyProgram[$rt]
        $probe = Join-Path $tmp ("startup" + $spec.Ext)
        Set-Content -Path $probe -Value $spec.Body -Encoding utf8
        $startup[$rt] = Measure-Min $rt $probe
    }
}
finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}

$startupLine = ($runtimes | ForEach-Object { "{0} {1} ms" -f $_, $startup[$_] }) -join '   '
Write-Host "  startup: $startupLine" -ForegroundColor DarkGray
Write-Host ""

# ------------------------------------------------------------------- measure

$results = @()
foreach ($b in $AllBenchmarks) {
    Write-Host ("`r  Running {0} ...{1}" -f $b.Name, (' ' * 30)) -NoNewline -ForegroundColor DarkGray

    $row = [ordered]@{ Benchmark = $b.Name }
    foreach ($rt in $runtimes) {
        $rel = switch ($rt) {
            'varn'   { $b.Vn }
            'python' { $b.Py }
            default  { $b.TS }
        }
        $total = $null
        if ($rel) {
            $file = Join-Path $benchDir $rel
            if (Test-Path $file) { $total = Measure-Min $rt $file }
        }
        $row["${rt}_total"] = $total
        # Work can measure slightly below zero on a benchmark that finishes in
        # about the time the runtime needs to start; that is noise, not a
        # negative duration, so it floors at zero and the row is flagged.
        $row["${rt}_work"] = if ($null -ne $total) { [Math]::Round([Math]::Max(0.0, $total - $startup[$rt]), 1) } else { $null }
    }

    $rivalWork = @()
    foreach ($rt in $runtimes) {
        if ($rt -ne 'varn' -and $null -ne $row["${rt}_work"]) { $rivalWork += $row["${rt}_work"] }
    }
    $vnWork = $row['varn_work']
    $row['RivalWork'] = if ($rivalWork.Count) { ($rivalWork | Measure-Object -Minimum).Minimum } else { $null }
    # Ratio on WORK: >1 means Varn does the work faster. Guarded against a zero
    # denominator, which a sub-startup benchmark can produce.
    $row['WorkRatio'] = if ($null -ne $vnWork -and $vnWork -gt 0 -and $null -ne $row['RivalWork']) {
        [Math]::Round($row['RivalWork'] / $vnWork, 2)
    } else { $null }
    $row['TotalRatio'] = if ($null -ne $row['varn_total']) {
        $rt2 = @($runtimes | Where-Object { $_ -ne 'varn' -and $null -ne $row["${_}_total"] } | ForEach-Object { $row["${_}_total"] })
        if ($rt2.Count) { [Math]::Round((($rt2 | Measure-Object -Minimum).Minimum) / $row['varn_total'], 2) } else { $null }
    } else { $null }

    $results += [pscustomobject]$row
}
# Wipe the progress line: a bare CR leaves the longest previous name behind.
Write-Host ("`r" + (' ' * 60) + "`r") -NoNewline

# -------------------------------------------------------------------- output

function Get-Bar([double]$startupMs, [double]$workMs, [double]$scale, [int]$width) {
    if ($scale -le 0) { return '' }
    $s = [int][Math]::Round(($startupMs / $scale) * $width)
    $w = [int][Math]::Round(($workMs / $scale) * $width)
    if ($workMs -gt 0 -and $w -lt 1) { $w = 1 }
    return @{ Startup = ('░' * $s); Work = ('█' * $w) }
}

if ($Markdown) {
    $hdrCells = ($runtimes | ForEach-Object { "$_ work" }) -join " | "
    $sepCells = ($runtimes | ForEach-Object { "---" }) -join "|"
    Write-Host "| Benchmark | $hdrCells | Varn vs rival (work) |"
    Write-Host "|---|$sepCells|---|"
    foreach ($r in $results) {
        $cells = ($runtimes | ForEach-Object {
            if ($null -ne $r."${_}_work") { "$($r."${_}_work") ms" } else { "--" }
        }) -join " | "
        $verdict = if ($null -ne $r.WorkRatio) { "$($r.WorkRatio)x" } else { "--" }
        Write-Host "| $($r.Benchmark) | $cells | $verdict |"
    }
}
elseif ($Compact) {
    $hdr = "  {0,-20}" -f "Benchmark"
    foreach ($rt in $runtimes) { $hdr += "{0,14}" -f "$rt work" }
    $hdr += "{0,14}" -f "work ratio"
    Write-Host $hdr -ForegroundColor Cyan
    Write-Host ("  " + ("-" * ($hdr.Length - 2))) -ForegroundColor DarkGray
    foreach ($r in $results) {
        $line = "  {0,-20}" -f $r.Benchmark
        foreach ($rt in $runtimes) {
            $line += "{0,14}" -f $(if ($null -ne $r."${rt}_work") { "$($r."${rt}_work") ms" } else { "--" })
        }
        $line += "{0,14}" -f $(if ($null -ne $r.WorkRatio) { "$($r.WorkRatio)x" } else { "--" })
        $color = if ($null -ne $r.WorkRatio -and $r.WorkRatio -ge 1.0) { 'Green' } else { 'Yellow' }
        Write-Host $line -ForegroundColor $color
    }
}
else {
    $width = 44
    foreach ($r in $results) {
        $totals = @($runtimes | Where-Object { $null -ne $r."${_}_total" } | ForEach-Object { $r."${_}_total" })
        if (-not $totals.Count) { continue }
        $scale = ($totals | Measure-Object -Maximum).Maximum

        Write-Host ""
        Write-Host ("  " + $r.Benchmark) -ForegroundColor Cyan
        foreach ($rt in $runtimes) {
            $total = $r."${rt}_total"
            if ($null -eq $total) {
                Write-Host ("    {0,-7} --" -f $rt) -ForegroundColor DarkGray
                continue
            }
            $work = $r."${rt}_work"
            $bar = Get-Bar $startup[$rt] $work $scale $width
            $isVarn = ($rt -eq 'varn')
            Write-Host ("    {0,-7} " -f $rt) -NoNewline -ForegroundColor $(if ($isVarn) { 'White' } else { 'Gray' })
            Write-Host $bar.Startup -NoNewline -ForegroundColor DarkGray
            Write-Host $bar.Work -NoNewline -ForegroundColor $(if ($isVarn) { 'Cyan' } else { 'DarkYellow' })
            $pad = $width - $bar.Startup.Length - $bar.Work.Length
            if ($pad -gt 0) { Write-Host (' ' * $pad) -NoNewline }
            $flag = if ($work -le 0) { ' (below startup)' } else { '' }
            Write-Host ("  {0,6} + {1,7} = {2,7} ms{3}" -f $startup[$rt], $work, $total, $flag) -ForegroundColor DarkGray
        }

        if ($null -ne $r.WorkRatio) {
            $v = $r.WorkRatio
            if ($v -ge 1.0) {
                Write-Host ("    -> work: varn {0}x faster than the fastest rival" -f $v) -ForegroundColor Green
            } else {
                $inv = if ($v -gt 0) { [Math]::Round(1 / $v, 2) } else { 0 }
                Write-Host ("    -> work: varn {0}x SLOWER than the fastest rival" -f $inv) -ForegroundColor Red
            }
            if ($null -ne $r.TotalRatio) {
                $tv = $r.TotalRatio
                # The row worth calling out: total says one thing, work says the
                # opposite, and the gap between them is startup.
                if (($tv -ge 1.0) -ne ($v -ge 1.0)) {
                    Write-Host ("       total says {0}x -- that difference is startup, not execution" -f $tv) -ForegroundColor DarkYellow
                }
            }
        }
    }
}

Write-Host ""
Write-Host "  Minimum of $Runs runs. Bars are process wall-clock, scaled to the slowest runtime per benchmark." -ForegroundColor DarkGray
Write-Host "  " -NoNewline
Write-Host "  startup" -NoNewline -ForegroundColor DarkGray
Write-Host "   " -NoNewline
Write-Host "  work" -NoNewline -ForegroundColor Cyan
Write-Host "   (startup calibrated once per runtime on an empty program)" -ForegroundColor DarkGray
Write-Host "  Verdict compares WORK. Startup is a real advantage, but a different one." -ForegroundColor DarkGray
Write-Host ""
