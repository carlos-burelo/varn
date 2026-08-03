<#
.SYNOPSIS
  Paired benchmark table: Varn against Bun, Node and Python.

.DESCRIPTION
  Runs every benchmark on every available runtime back to back and prints one
  table. Two rules from benchmarks/README.md are enforced here rather than left
  to the reader:

    1. Output before time. A runtime whose checksum differs from the others is
       not doing the same work, so its number is garbage. Mismatches are shown
       as FAIL and excluded from the ratios.
    2. Paired in one window. The machine drifts (thermals, background load), so
       absolute milliseconds are not comparable across sessions -- ratios are.
       All runtimes for one benchmark run adjacently to share conditions.

  Varn's number is the MIN of `vn bench` execute; the other runtimes' harnesses
  report their own best-of-N. Min against min is the honest pairing.

.PARAMETER Runs
  Timed runs for `vn bench` (default 10). The JS/Python harnesses carry their
  own counts.

.PARAMETER Only
  Run just the named benchmarks, e.g. -Only fib,matrix.

.PARAMETER SkipPython
  Skip Python. It is ~10-40x slower here and dominates wall time.

.PARAMETER Markdown
  Emit a Markdown table for pasting into docs.

.PARAMETER NoJit
  Run Varn with VARN_NO_JIT=1, to size what the JIT is buying.

.EXAMPLE
  .\benchmarks\compare.ps1
  .\benchmarks\compare.ps1 -SkipPython -Markdown
  .\benchmarks\compare.ps1 -Only matrix,array_ops
#>
[CmdletBinding()]
param(
    [int]$Runs = 10,
    [string[]]$Only,
    [switch]$SkipPython,
    [switch]$Markdown,
    [switch]$NoJit
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$benchDir = Join-Path $root 'benchmarks'
$vn = Join-Path $root 'target\release\vn.exe'

# Checksum extraction is per-benchmark on purpose. The `.vn` files print their
# results in whatever shape reads best, while the JS/Python harnesses print one
# combined value; these rules reconcile the two. A generic "sum all numbers"
# rule would silently fold gc_alloc's timing lines into its checksum.
$Benchmarks = @(
    @{ Name = 'fib';       Sum = 1 }
    @{ Name = 'math';      Sum = 1 }
    @{ Name = 'matrix';    Sum = 2 }
    @{ Name = 'array_ops'; Pattern = 'Array get sum:\s*(-?[\d.]+)' }
    @{ Name = 'dto';       Sum = 2 }
    @{ Name = 'gc_alloc';  Pattern = 'check=(-?[\d.]+)/(-?[\d.]+)/(-?[\d.]+)' }
)

if ($Only) { $Benchmarks = $Benchmarks | Where-Object { $Only -contains $_.Name } }
if (-not $Benchmarks) { throw "No benchmarks matched -Only" }

function Remove-Ansi([string]$s) { $s -replace "`e\[[0-9;]*m", '' }

function Get-Checksum($spec, [string]$output) {
    if ($spec.Pattern) {
        $m = [regex]::Match($output, $spec.Pattern)
        if (-not $m.Success) { return $null }
        $t = 0.0
        for ($i = 1; $i -lt $m.Groups.Count; $i++) { $t += [double]$m.Groups[$i].Value }
        return $t
    }
    # Sum the first N standalone numbers the program printed.
    $nums = [regex]::Matches($output, '(?m)^\s*(-?\d+(?:\.\d+)?)\s*$')
    if ($nums.Count -lt $spec.Sum) { return $null }
    $t = 0.0
    for ($i = 0; $i -lt $spec.Sum; $i++) { $t += [double]$nums[$i].Groups[1].Value }
    return $t
}

# Two checksums agree when they are the same number. Compared with a relative
# tolerance because the float benchmarks (math, dto) legitimately differ in the
# last bits across runtimes.
function Test-SameChecksum($a, $b) {
    if ($null -eq $a -or $null -eq $b) { return $false }
    if ($a -eq $b) { return $true }
    $scale = [Math]::Max([Math]::Abs($a), [Math]::Abs($b))
    if ($scale -eq 0) { return $true }
    return ([Math]::Abs($a - $b) / $scale) -lt 1e-9
}

function Invoke-Runtime([string]$exe, [string[]]$argv) {
    $out = & $exe @argv 2>&1
    $stdout = @(); $stderr = @()
    foreach ($line in $out) {
        if ($line -is [System.Management.Automation.ErrorRecord]) { $stderr += "$line" }
        else { $stdout += "$line" }
    }
    [pscustomobject]@{ Out = ($stdout -join "`n"); Err = ($stderr -join "`n") }
}

# --- runtime discovery -----------------------------------------------------
$runtimes = @()
if (Test-Path $vn) { $runtimes += 'varn' } else { throw "vn.exe not found at $vn -- run: cargo build --release --bin vn" }
foreach ($r in 'bun', 'node') {
    if (Get-Command $r -ErrorAction SilentlyContinue) { $runtimes += $r }
}
if (-not $SkipPython -and (Get-Command 'python' -ErrorAction SilentlyContinue)) { $runtimes += 'python' }

# A stale binary is the quietest way to measure the wrong thing.
$newestSrc = Get-ChildItem (Join-Path $root 'crates') -Recurse -Filter *.rs |
    Sort-Object LastWriteTime -Descending | Select-Object -First 1
if ($newestSrc -and $newestSrc.LastWriteTime -gt (Get-Item $vn).LastWriteTime) {
    Write-Host "  WARNING: vn.exe is older than $($newestSrc.Name) -- rebuild before trusting these numbers." -ForegroundColor Yellow
}

$label = if ($NoJit) { 'varn (no-jit)' } else { 'varn' }
Write-Host ""
Write-Host "  Runtimes: $($runtimes -join ', ')   vn runs: $Runs" -ForegroundColor Cyan
Write-Host "  Paired in one window; ratios are the comparable figure, not absolute ms." -ForegroundColor DarkGray
Write-Host ""

$rows = @()
foreach ($spec in $Benchmarks) {
    $name = $spec.Name
    Write-Host "  $name ..." -NoNewline

    $row = [ordered]@{ Benchmark = $name }
    $checks = @{}
    $cv = $null

    foreach ($rt in $runtimes) {
        switch ($rt) {
            'varn' {
                $file = Join-Path $benchDir "bench_$name.vn"
                if (-not (Test-Path $file)) { $row['varn'] = $null; continue }
                $old = $env:VARN_NO_JIT
                if ($NoJit) { $env:VARN_NO_JIT = '1' }
                # Correctness first: a plain run, for the checksum.
                $r = Invoke-Runtime $vn @('run', $file)
                $checks['varn'] = Get-Checksum $spec (Remove-Ansi $r.Out)
                # Then the timing run.
                $b = Invoke-Runtime $vn @('bench', $file, '--runs', "$Runs")
                $env:VARN_NO_JIT = $old
                $txt = Remove-Ansi ($b.Out + "`n" + $b.Err)
                # The phase table is drawn with U+2502, not ASCII '|', and the
                # headline carries a second "execute" line with no columns at
                # all -- so match the row shape, then split it into fields:
                #   execute | min | p50 | mean | max | CV% | share%
                $row['varn'] = $null
                $line = $txt -split "`n" | Where-Object { $_ -match '^\s*execute\s*│' } | Select-Object -First 1
                if ($line) {
                    $f = $line -split '│' | ForEach-Object { $_.Trim() }
                    if ($f.Count -ge 6 -and $f[1] -match '^([\d.]+)\s*(ms|s|µs|us)$') {
                        $v = [double]$Matches[1]
                        switch ($Matches[2]) { 's' { $v *= 1000 } 'µs' { $v /= 1000 } 'us' { $v /= 1000 } }
                        $row['varn'] = $v
                    }
                    if ($f[5] -match '^([\d.]+)%$') { $cv = [double]$Matches[1] }
                }
            }
            default {
                $script = if ($rt -eq 'python') { Join-Path $benchDir "py\$name.py" } else { Join-Path $benchDir "js\$name.js" }
                if (-not (Test-Path $script)) { $row[$rt] = $null; continue }
                $r = Invoke-Runtime $rt @($script)
                $checks[$rt] = if ($r.Err -match '(-?[\d.]+)') { [double]$Matches[1] } else { $null }
                $row[$rt] = if ($r.Out -match '([\d.]+)') { [double]$Matches[1] } else { $null }
            }
        }
    }

    # Checksum agreement, against Varn as the reference.
    $bad = @()
    foreach ($k in $checks.Keys) {
        if ($k -eq 'varn') { continue }
        if (-not (Test-SameChecksum $checks['varn'] $checks[$k])) { $bad += $k }
    }
    $row['_bad'] = $bad
    $row['_cv'] = $cv
    $rows += [pscustomobject]$row
    Write-Host "`r  $name    " -NoNewline
    Write-Host ""
}

# --- table -----------------------------------------------------------------
$cols = @($runtimes)
$fmt = { param($v) if ($null -eq $v) { '--' } elseif ($v -ge 1000) { '{0:N0}' -f $v } else { '{0:N2}' -f $v } }

Write-Host ""
if ($Markdown) {
    $head = "| bench | " + (($cols | ForEach-Object { $_ -eq 'varn' ? $label : $_ }) -join ' | ') + " | vs best |"
    $sep = "|" + ("---|" * ($cols.Count + 2))
    Write-Host $head
    Write-Host $sep
}
else {
    $w = 12
    $hdr = "  {0,-11}" -f 'bench'
    foreach ($c in $cols) { $hdr += "{0,$w}" -f ($c -eq 'varn' ? $label : $c) }
    $hdr += "{0,12}" -f 'vs best'
    Write-Host $hdr -ForegroundColor Cyan
    Write-Host ("  " + ("-" * ($hdr.Length - 2))) -ForegroundColor DarkGray
}

foreach ($r in $rows) {
    $vnVal = $r.varn
    # Fastest rival, ignoring any runtime whose checksum disagreed.
    $rivals = @()
    foreach ($c in $cols) {
        if ($c -eq 'varn') { continue }
        if ($r.'_bad' -contains $c) { continue }
        if ($null -ne $r.$c) { $rivals += $r.$c }
    }
    $ratio = if ($vnVal -and $rivals.Count) { $vnVal / ($rivals | Measure-Object -Minimum).Minimum } else { $null }

    $cells = @()
    foreach ($c in $cols) {
        $v = $r.$c
        $s = & $fmt $v
        if ($r.'_bad' -contains $c) { $s = "FAIL" }
        $cells += $s
    }
    $rs = if ($null -eq $ratio) { '--' } elseif ($ratio -lt 1) { ('{0:N2}x WIN' -f $ratio) } else { ('{0:N2}x' -f $ratio) }

    if ($Markdown) {
        Write-Host ("| {0} | {1} | {2} |" -f $r.Benchmark, ($cells -join ' | '), $rs)
    }
    else {
        $line = "  {0,-11}" -f $r.Benchmark
        foreach ($s in $cells) { $line += "{0,12}" -f $s }
        $line += "{0,12}" -f $rs
        $color = if ($null -ne $ratio -and $ratio -lt 1) { 'Green' } else { 'Gray' }
        Write-Host $line -ForegroundColor $color
        if ($null -ne $r.'_cv' -and $r.'_cv' -gt 10) {
            Write-Host ("      noisy: vn CV {0}% -- above the harness' own 10% gate, treat as indicative" -f $r.'_cv') -ForegroundColor Yellow
        }
    }
}

Write-Host ""
Write-Host "  ms = best of N (Varn: min of `vn bench` execute). 'vs best' = Varn / fastest rival." -ForegroundColor DarkGray
Write-Host "  FAIL = checksum disagreed with Varn; that runtime is excluded from the ratio." -ForegroundColor DarkGray
Write-Host ""
