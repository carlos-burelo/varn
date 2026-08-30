<#
.SYNOPSIS
  Runtime comparison that reports what the measurements actually support.

.DESCRIPTION
  A benchmark harness has two jobs: measure honestly, and refuse to claim more
  than the measurements support. This one is built around the ways the previous
  version got both wrong.

  WHAT IT MEASURES

  Process wall-clock is two costs summed - the time a runtime needs to exist,
  and the time it spends on the program. Reporting only the sum lets one hide
  behind the other. Varn starts in ~9 ms against Bun's ~44 ms, so a ~35 ms
  advantage used to be added to every row, enough to decide most of them. Each
  runtime is calibrated once on an empty program through the same launcher and
  file extension the benchmarks use, and every row reports startup and work
  separately. The verdict compares work, because that is what a language change
  moves.

  WHY MEDIAN, NOT MINIMUM

  The minimum is the luckiest run, and on a hybrid CPU that mostly means "the
  runs that landed on a P-core". It hides variance completely, which is how a
  +-13% machine produces confident-looking tables. Median is reported with the
  full [min, max] range beside it so the spread is never invisible.

  WHY IT INTERLEAVES

  Running every repetition of one runtime before starting the next makes the
  result depend on when each ran: thermal drift or a background process
  penalises whoever occupied the bad window. Repetitions are therefore
  round-robined across runtimes.

  WHY IT CHECKS OUTPUT

  A benchmark that computes the WRONG answer quickly used to score as a win.
  That is not hypothetical: a JIT bug in this repo returned heap slot numbers
  where strings belonged, and every affected program still "finished fast".
  Program output is captured and compared across runtimes; a mismatch
  invalidates the row rather than decorating it.

  WHEN IT REFUSES TO CALL A WINNER

  If the [min, max] work ranges of the two runtimes overlap, the difference is
  not resolved by this many runs on this machine, and the row says so instead
  of printing a ratio that looks decisive.

.PARAMETER Runs
  Repetitions per benchmark per runtime (default 7). Two are discarded as
  warmup, so at least three are required.

.PARAMETER Only
  Run just the named benchmarks, e.g. -Only fib,matrix,dto.

.PARAMETER Baseline
  Path to a second vn.exe, measured as its own runtime named `varn-base`.
  This is the A/B for "did my change help?" - same interleaving, same output
  check, same refusal to call unresolved differences.

.PARAMETER SkipPython
  Skip Python benchmark execution.

.PARAMETER Compact
  One line per benchmark instead of the bar chart.

.PARAMETER Markdown
  Emit a Markdown table for docs/reports.

.EXAMPLE
  .\tests\benchmarks\compare.ps1
  .\tests\benchmarks\compare.ps1 -Only fib,matrix -Runs 15
  .\tests\benchmarks\compare.ps1 -Baseline C:\tmp\vn-before.exe -SkipPython
#>
[CmdletBinding()]
param(
    [int]$Runs = 7,
    [string[]]$Only,
    [string]$Baseline,
    [switch]$SkipPython,
    [switch]$Compact,
    [switch]$Markdown
)

$ErrorActionPreference = 'Continue'
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$benchDir = $PSScriptRoot
$vn = Join-Path $root 'target\release\vn.exe'

if (-not (Test-Path $vn)) {
    throw "vn.exe not found at $vn -- run: cargo build --release --bin vn"
}
if ($Baseline -and -not (Test-Path $Baseline)) {
    throw "baseline binary not found: $Baseline"
}
# Two runs are discarded as warmup, so fewer than three leaves no samples.
if ($Runs -lt 3) { throw "-Runs must be at least 3 (two are warmup)" }

$WarmupRuns = 2

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
    # `pwsh -File script.ps1 -Only fib,matrix` binds the whole list as one
    # string, while -Command and dot-sourcing bind a real array. Split on
    # commas so both invocations behave the same.
    $Only = @($Only | ForEach-Object { $_ -split ',' } | ForEach-Object { $_.Trim() } | Where-Object { $_ })
    $AllBenchmarks = @($AllBenchmarks | Where-Object { $Only -contains $_.Name })
    if (-not $AllBenchmarks) { throw "no benchmark matched: $($Only -join ', ')" }
}

$runtimes = @('varn')
if ($Baseline) { $runtimes += 'varn-base' }
if (Get-Command 'bun' -ErrorAction SilentlyContinue) { $runtimes += 'bun' }
if (Get-Command 'node' -ErrorAction SilentlyContinue) { $runtimes += 'node' }
if (-not $SkipPython -and (Get-Command 'python' -ErrorAction SilentlyContinue)) { $runtimes += 'python' }

# How each runtime is launched, and what an empty program looks like for it.
# The startup probe must go through the SAME launcher and extension as the
# benchmarks, or it calibrates a path the benchmarks never take.
$Invoke = @{
    varn        = { param($f) & $vn 'run' $f }
    'varn-base' = { param($f) & $Baseline 'run' $f }
    bun         = { param($f) & 'bun' 'run' $f }
    node        = { param($f) & 'node' $f }
    python      = { param($f) & 'python' $f }
}
$EmptyProgram = @{
    varn        = @{ Ext = '.vn'; Body = 'print(1)' }
    'varn-base' = @{ Ext = '.vn'; Body = 'print(1)' }
    bun         = @{ Ext = '.ts'; Body = 'console.log(1)' }
    node        = @{ Ext = '.ts'; Body = 'console.log(1)' }
    python      = @{ Ext = '.py'; Body = 'print(1)' }
}

function Get-BenchFile($b, [string]$rt) {
    switch ($rt) {
        'varn' { $b.Vn }
        'varn-base' { $b.Vn }
        'python' { $b.Py }
        default { $b.TS }
    }
}

function Get-Median([double[]]$xs) {
    if (-not $xs -or $xs.Count -eq 0) { return $null }
    $s = @($xs | Sort-Object)
    $n = $s.Count
    if ($n % 2) { return $s[[int](($n - 1) / 2)] }
    return (($s[$n / 2 - 1] + $s[$n / 2]) / 2.0)
}

# The comparable part of a program's output.
#
# Several benchmarks time themselves internally and print the result, which of
# course differs on every run and between runtimes. That is instrumentation,
# not the answer, so lines mentioning elapsed time are dropped and what remains
# is reduced to the integers it contains — the values the benchmark actually
# computed. Labels and formatting differ freely between the .vn and .ts ports;
# the computed numbers must not.
function Get-ResultSignature([string]$raw) {
    $keep = @()
    foreach ($line in ($raw -split "`r?`n")) {
        # A `\bms\b` alternative misses `junk_ms=37`: `_` is a word character,
        # so there is no boundary before `ms`. Match the timing-token shapes
        # explicitly rather than loosening to a bare `ms`, which would swallow
        # any result line containing those two letters.
        if ($line -match '(?i)elapsed|took|\btime\b|(^|[^a-z])ms\b|_ms\b|\bms\s*[=:]') { continue }
        $keep += $line
    }
    $text = $keep -join ' '
    # Integers only: a float in surviving output is almost always a duration or
    # a formatted timing, and comparing those across runtimes is meaningless.
    $nums = [regex]::Matches($text, '(?<![\d.])-?\d+(?![\d.])') | ForEach-Object { $_.Value }
    return ($nums -join ',')
}

# One timed execution. Output is captured, not discarded: it is the only way to
# tell a fast right answer from a fast wrong one.
# One bytecode-cache directory per runtime, created fresh for this session so
# no earlier run's artifacts leak into these numbers.
$CacheRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("varn-bench-cache-" + [Guid]::NewGuid().ToString('N').Substring(0, 8))
$CacheDirs = @{}
foreach ($rt in $runtimes) {
    $dir = Join-Path $CacheRoot ($rt -replace '[^a-zA-Z0-9]', '_')
    New-Item -ItemType Directory -Path $dir -Force | Out-Null
    $CacheDirs[$rt] = $dir
}

function Invoke-Once([string]$rt, [string]$file) {
    # Each runtime gets its own bytecode cache. Varn keys a cache entry by the
    # producing binary, so `varn` and `varn-base` no longer read each other's
    # bytecode - but sharing one directory still makes them compete for the
    # same retained generations, and a measurement harness should not depend
    # on the runtime's retention policy to stay honest. A directory apiece
    # makes the isolation explicit and survives any future change to that
    # policy. Non-Varn runtimes ignore the variable.
    $prev = $env:VARN_CACHE_DIR
    $env:VARN_CACHE_DIR = $CacheDirs[$rt]
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $out = & $Invoke[$rt] $file 2>&1
    $sw.Stop()
    if ($null -eq $prev) { Remove-Item Env:\VARN_CACHE_DIR -ErrorAction SilentlyContinue }
    else { $env:VARN_CACHE_DIR = $prev }
    $raw = ($out | Out-String).Trim()
    return [pscustomobject]@{
        Ms        = $sw.Elapsed.TotalMilliseconds
        Signature = Get-ResultSignature $raw
        Output    = ($raw -replace '\s+', ' ')
    }
}

# ---------------------------------------------------------------- provenance

$cpu = try { (Get-CimInstance Win32_Processor | Select-Object -First 1).Name.Trim() } catch { 'unknown' }
Write-Host ""
Write-Host "  Runtimes: $($runtimes -join ', ')" -ForegroundColor Cyan
Write-Host "  $Runs runs per benchmark per runtime ($WarmupRuns discarded as warmup), interleaved." -ForegroundColor DarkGray
Write-Host "  Host: $cpu" -ForegroundColor DarkGray
if ($Baseline) { Write-Host "  varn-base: $Baseline" -ForegroundColor DarkGray }
Write-Host ""

# ---------------------------------------------------------------- calibration

Write-Host "  Calibrating startup on an empty program ..." -NoNewline -ForegroundColor DarkGray
$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("varn-bench-" + [Guid]::NewGuid().ToString('N').Substring(0, 8))
New-Item -ItemType Directory -Path $tmp -Force | Out-Null
$startup = @{}
$startupRange = @{}
try {
    $probes = @{}
    foreach ($rt in $runtimes) {
        $spec = $EmptyProgram[$rt]
        # Distinct file per runtime: a shared path would also share its
        # file-cache state and any transpile cache keyed on it.
        $probe = Join-Path $tmp ("startup_$($rt -replace '[^a-zA-Z0-9]', '_')" + $spec.Ext)
        Set-Content -Path $probe -Value $spec.Body -Encoding utf8
        $probes[$rt] = $probe
    }
    $samples = @{}
    foreach ($rt in $runtimes) { $samples[$rt] = @() }
    for ($i = 0; $i -lt $Runs; $i++) {
        foreach ($rt in $runtimes) {
            $r = Invoke-Once $rt $probes[$rt]
            if ($i -ge $WarmupRuns) { $samples[$rt] += $r.Ms }
        }
    }
    foreach ($rt in $runtimes) {
        $startup[$rt] = [Math]::Round((Get-Median $samples[$rt]), 1)
        $startupRange[$rt] = @{
            Min = [Math]::Round((($samples[$rt] | Measure-Object -Minimum).Minimum), 1)
            Max = [Math]::Round((($samples[$rt] | Measure-Object -Maximum).Maximum), 1)
        }
    }
}
finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
Write-Host ("`r  startup (median [min-max]):" + (' ' * 30)) -ForegroundColor DarkGray
foreach ($rt in $runtimes) {
    Write-Host ("    {0,-11} {1,7} ms  [{2} - {3}]" -f $rt, $startup[$rt], $startupRange[$rt].Min, $startupRange[$rt].Max) -ForegroundColor DarkGray
}
Write-Host ""

# ------------------------------------------------------------------- measure

$results = @()
foreach ($b in $AllBenchmarks) {
    Write-Host ("`r  Running {0} ...{1}" -f $b.Name, (' ' * 34)) -NoNewline -ForegroundColor DarkGray

    $files = @{}
    foreach ($rt in $runtimes) {
        $rel = Get-BenchFile $b $rt
        if ($rel) {
            $f = Join-Path $benchDir $rel
            if (Test-Path $f) { $files[$rt] = $f }
        }
    }
    $present = @($runtimes | Where-Object { $files.ContainsKey($_) })

    $samples = @{}
    $outputs = @{}
    $sigs = @{}
    foreach ($rt in $present) { $samples[$rt] = @() }
    # Round-robin: repetition i of every runtime happens before repetition i+1
    # of any of them, so slow drift lands on all of them alike.
    for ($i = 0; $i -lt $Runs; $i++) {
        foreach ($rt in $present) {
            $r = Invoke-Once $rt $files[$rt]
            if ($i -ge $WarmupRuns) { $samples[$rt] += $r.Ms }
            $outputs[$rt] = $r.Output
            $sigs[$rt] = $r.Signature
        }
    }

    $row = [ordered]@{ Benchmark = $b.Name }
    foreach ($rt in $runtimes) {
        if (-not $files.ContainsKey($rt)) {
            $row["${rt}_total"] = $null; $row["${rt}_work"] = $null
            $row["${rt}_lo"] = $null; $row["${rt}_hi"] = $null
            continue
        }
        $med = Get-Median $samples[$rt]
        $lo = ($samples[$rt] | Measure-Object -Minimum).Minimum
        $hi = ($samples[$rt] | Measure-Object -Maximum).Maximum
        $row["${rt}_total"] = [Math]::Round($med, 1)
        # Work floors at zero: a benchmark finishing in about the time the
        # runtime needs to start yields noise, not a negative duration.
        $row["${rt}_work"] = [Math]::Round([Math]::Max(0.0, $med - $startup[$rt]), 1)
        $row["${rt}_lo"] = [Math]::Round([Math]::Max(0.0, $lo - $startup[$rt]), 1)
        $row["${rt}_hi"] = [Math]::Round([Math]::Max(0.0, $hi - $startup[$rt]), 1)
    }

    # Output agreement. Runtimes are only comparable if they computed the same
    # thing; a mismatch means the row measures two different programs.
    $seen = @($present | ForEach-Object { $sigs[$_] } | Where-Object { $_ } | Select-Object -Unique)
    $row['OutputOk'] = ($seen.Count -le 1)
    $row['OutputsByRt'] = $outputs

    # Verdict on work, against the fastest runtime that is not a varn build.
    $rivals = @($present | Where-Object { $_ -notlike 'varn*' })
    $best = $null; $bestRt = $null
    foreach ($rt in $rivals) {
        $w = $row["${rt}_work"]
        if ($null -ne $w -and ($null -eq $best -or $w -lt $best)) { $best = $w; $bestRt = $rt }
    }
    $row['RivalWork'] = $best
    $row['RivalName'] = $bestRt
    $vw = $row['varn_work']
    $row['WorkRatio'] = if ($null -ne $vw -and $vw -gt 0 -and $null -ne $best) { [Math]::Round($best / $vw, 2) } else { $null }
    # Separated only when the [min,max] work ranges do not overlap. Otherwise
    # this many runs on this machine do not resolve the difference.
    $row['Resolved'] = if ($null -ne $best -and $null -ne $vw -and $bestRt) {
        ($row['varn_hi'] -lt $row["${bestRt}_lo"]) -or ($row["${bestRt}_hi"] -lt $row['varn_lo'])
    }
    else { $false }

    $row['BaseRatio'] = $null
    $row['BaseResolved'] = $false
    if ($Baseline -and $null -ne $row['varn-base_work'] -and $null -ne $vw -and $vw -gt 0) {
        $row['BaseRatio'] = [Math]::Round($row['varn-base_work'] / $vw, 2)
        $row['BaseResolved'] = ($row['varn_hi'] -lt $row['varn-base_lo']) -or ($row['varn-base_hi'] -lt $row['varn_lo'])
    }

    $results += [pscustomobject]$row
}
Write-Host ("`r" + (' ' * 64) + "`r") -NoNewline

# -------------------------------------------------------------------- output

function Write-Verdict($r) {
    if (-not $r.OutputOk) {
        Write-Host "    -> RUNTIMES DISAGREE ON OUTPUT - this row compares different programs" -ForegroundColor Red
        foreach ($rt in $r.OutputsByRt.Keys) {
            $o = [string]$r.OutputsByRt[$rt]
            if ($o.Length -gt 60) { $o = $o.Substring(0, 60) + '...' }
            Write-Host ("       {0,-11} {1}" -f $rt, $o) -ForegroundColor DarkRed
        }
        return
    }
    if ($null -ne $r.WorkRatio) {
        if (-not $r.Resolved) {
            Write-Host ("    -> work: varn {0} ms vs {1} {2} ms - ranges overlap, not resolved by {3} runs" -f `
                    $r.varn_work, $r.RivalName, $r.RivalWork, $Runs) -ForegroundColor DarkGray
        }
        elseif ($r.WorkRatio -ge 1.0) {
            Write-Host ("    -> work: varn {0}x faster than {1}" -f $r.WorkRatio, $r.RivalName) -ForegroundColor Green
        }
        else {
            Write-Host ("    -> work: varn {0}x SLOWER than {1}" -f [Math]::Round(1 / $r.WorkRatio, 2), $r.RivalName) -ForegroundColor Red
        }
    }
    if ($null -ne $r.BaseRatio) {
        $bv = $r.BaseRatio
        $txt = if ($bv -ge 1.0) { "{0}x faster than varn-base" -f $bv } else { "{0}x slower than varn-base" -f [Math]::Round(1 / $bv, 2) }
        if (-not $r.BaseResolved) { $txt += " (ranges overlap - not resolved)" }
        Write-Host ("    -> vs baseline: varn {0}" -f $txt) -ForegroundColor $(if ($r.BaseResolved) { 'Cyan' } else { 'DarkGray' })
    }
}

function Get-VerdictText($r) {
    if (-not $r.OutputOk) { return "OUTPUT MISMATCH" }
    if ($null -eq $r.WorkRatio) { return "--" }
    if (-not $r.Resolved) { return "not resolved" }
    if ($r.WorkRatio -ge 1) { return ("{0}x faster" -f $r.WorkRatio) }
    return ("{0}x slower" -f [Math]::Round(1 / $r.WorkRatio, 2))
}

if ($Markdown) {
    $hdrCells = ($runtimes | ForEach-Object { "$_ work" }) -join " | "
    $sepCells = ($runtimes | ForEach-Object { "---" }) -join "|"
    Write-Host "| Benchmark | $hdrCells | verdict (work) |"
    Write-Host "|---|$sepCells|---|"
    foreach ($r in $results) {
        $cells = ($runtimes | ForEach-Object {
                if ($null -ne $r."${_}_work") { "$($r."${_}_work") ms" } else { "--" }
            }) -join " | "
        Write-Host "| $($r.Benchmark) | $cells | $(Get-VerdictText $r) |"
    }
}
elseif ($Compact) {
    $hdr = "  {0,-20}" -f "Benchmark"
    foreach ($rt in $runtimes) { $hdr += "{0,15}" -f "$rt work" }
    $hdr += "  verdict"
    Write-Host $hdr -ForegroundColor Cyan
    Write-Host ("  " + ("-" * ($hdr.Length - 2))) -ForegroundColor DarkGray
    foreach ($r in $results) {
        $line = "  {0,-20}" -f $r.Benchmark
        foreach ($rt in $runtimes) {
            $line += "{0,15}" -f $(if ($null -ne $r."${rt}_work") { "$($r."${rt}_work") ms" } else { "--" })
        }
        $color =
        if (-not $r.OutputOk) { 'Red' }
        elseif (-not $r.Resolved) { 'DarkGray' }
        elseif ($r.WorkRatio -ge 1) { 'Green' }
        else { 'Yellow' }
        Write-Host ($line + "  " + (Get-VerdictText $r)) -ForegroundColor $color
    }
}
else {
    $width = 40
    foreach ($r in $results) {
        $totals = @($runtimes | Where-Object { $null -ne $r."${_}_total" } | ForEach-Object { $r."${_}_total" })
        if (-not $totals.Count) { continue }
        $scale = ($totals | Measure-Object -Maximum).Maximum

        Write-Host ""
        $tag = if ($r.OutputOk) { "" } else { "   [OUTPUT MISMATCH]" }
        Write-Host ("  " + $r.Benchmark + $tag) -ForegroundColor $(if ($r.OutputOk) { 'Cyan' } else { 'Red' })
        foreach ($rt in $runtimes) {
            $total = $r."${rt}_total"
            if ($null -eq $total) {
                Write-Host ("    {0,-11} --" -f $rt) -ForegroundColor DarkGray
                continue
            }
            $work = $r."${rt}_work"
            $s = [int][Math]::Round(($startup[$rt] / $scale) * $width)
            $w = [int][Math]::Round(($work / $scale) * $width)
            if ($work -gt 0 -and $w -lt 1) { $w = 1 }
            $isVarn = ($rt -eq 'varn')
            Write-Host ("    {0,-11} " -f $rt) -NoNewline -ForegroundColor $(if ($isVarn) { 'White' } else { 'Gray' })
            Write-Host ('.' * $s) -NoNewline -ForegroundColor DarkGray
            Write-Host ('#' * $w) -NoNewline -ForegroundColor $(if ($isVarn) { 'Cyan' } else { 'DarkYellow' })
            $pad = $width - $s - $w
            if ($pad -gt 0) { Write-Host (' ' * $pad) -NoNewline }
            $flag = if ($work -le 0) { ' (at or below startup - unresolvable)' } else { '' }
            Write-Host ("  {0,6} + {1,7} = {2,7} ms   work [{3} - {4}]{5}" -f `
                    $startup[$rt], $work, $total, $r."${rt}_lo", $r."${rt}_hi", $flag) -ForegroundColor DarkGray
        }
        Write-Verdict $r
    }
}

$unresolved = @($results | Where-Object { $_.OutputOk -and $null -ne $_.WorkRatio -and -not $_.Resolved }).Count
$mismatched = @($results | Where-Object { -not $_.OutputOk }).Count

Write-Host ""
Write-Host "  Median of $($Runs - $WarmupRuns) timed runs (plus $WarmupRuns warmup), interleaved across runtimes." -ForegroundColor DarkGray
Write-Host "  Bars: " -NoNewline -ForegroundColor DarkGray
Write-Host "...startup" -NoNewline -ForegroundColor DarkGray
Write-Host " ###work" -NoNewline -ForegroundColor Cyan
Write-Host "   scaled to the slowest runtime in each benchmark." -ForegroundColor DarkGray
Write-Host "  A verdict is printed only when the two [min-max] work ranges do not overlap." -ForegroundColor DarkGray
if ($unresolved) { Write-Host "  $unresolved benchmark(s) unresolved at this run count - re-run with -Runs 15 on an idle machine." -ForegroundColor DarkYellow }
if ($mismatched) { Write-Host "  $mismatched benchmark(s) produced different output across runtimes - those rows are not comparisons." -ForegroundColor Red }
Write-Host ""
