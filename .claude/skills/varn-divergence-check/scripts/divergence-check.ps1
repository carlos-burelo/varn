<#
.SYNOPSIS
    Run a .vn file under every execution tier and report output divergence.

.DESCRIPTION
    A Varn program must produce byte-identical output no matter which tier runs
    it. This script executes each file across the live axes and diffs the output
    against the baseline (JIT + dev-checkout std):

      * JIT              vs  interpreter        (VARN_NO_JIT=1)
      * dev-checkout std vs  embedded bundle    (VARN_STD=@embedded, with -Embedded)

    It also reports SSA lowering fallbacks: `OptError::Unsupported` surfaces as
    `Unsupported("...")` in the error text of `vn debug -p bytecode`, and marks a
    construct varn-compiler could not lower.

    HISTORY: this replaces the old `scripts/diffcheck.ps1`, which compared
    `VN_OPT=1` against a legacy compiler path. `VN_OPT` is no longer read
    anywhere (only `VN_OPT_TRACE` survives, in varn-compiler/src/lib.rs), and
    the legacy path is gone — varn-compiler is the only lowering pipeline, so
    that comparison could no longer differ.

.PARAMETER Path
    File, directory or glob of .vn files. Default: tests/main.vn.

.PARAMETER Bin
    Path to the vn binary. Auto-detected (release, then debug) if omitted.

.PARAMETER Embedded
    Also cross every run against the embedded stdlib bundle (VARN_STD=@embedded),
    giving the full 4-way matrix. Purges the compile cache between provenances.

.PARAMETER ShowOutput
    Print the full stdout of both sides when a divergence is found.

.EXAMPLE
    pwsh -File .claude/skills/varn-divergence-check/scripts/divergence-check.ps1
    pwsh -File .claude/skills/varn-divergence-check/scripts/divergence-check.ps1 -Embedded
    pwsh -File .claude/skills/varn-divergence-check/scripts/divergence-check.ps1 -Path tests/*.vn
#>
[CmdletBinding()]
param(
    [string]$Path = "tests/main.vn",
    [string]$Bin,
    [switch]$Embedded,
    [switch]$ShowOutput
)

$ErrorActionPreference = "Stop"
# vn exits non-zero on files with diagnostics; don't let that abort the sweep
# (PS 7.4+ promotes native non-zero to a terminating error under Stop).
$PSNativeCommandUseErrorActionPreference = $false
try { [Console]::OutputEncoding = [System.Text.Encoding]::UTF8 } catch { }

# The script lives under .claude/skills/<skill>/scripts/, so the repo root is not
# a fixed number of levels up. Walk parents until the workspace manifest appears;
# fall back to the current directory when invoked from outside the tree.
function Resolve-RepoRoot {
    $dir = $PSScriptRoot
    while ($dir) {
        if (Test-Path (Join-Path $dir "Cargo.toml")) { return $dir }
        $parent = Split-Path -Parent $dir
        if ($parent -eq $dir) { break }
        $dir = $parent
    }
    return (Get-Location).Path
}
$repoRoot = Resolve-RepoRoot

function Resolve-VnBin {
    param([string]$Explicit)
    if ($Explicit) {
        if (-not (Test-Path $Explicit)) { throw "vn binary not found: $Explicit" }
        return (Resolve-Path $Explicit).Path
    }
    foreach ($c in @("target/release/vn.exe", "target/debug/vn.exe")) {
        $p = Join-Path $repoRoot $c
        if (Test-Path $p) { return $p }
    }
    throw "vn binary not found. Build it first: cargo build --release -p varn-cli"
}

function Get-VnFiles {
    param([string]$Pattern)
    $p = if ([System.IO.Path]::IsPathRooted($Pattern)) { $Pattern } else { Join-Path $repoRoot $Pattern }
    if (Test-Path $p -PathType Container) { $p = Join-Path $p "*.vn" }
    $files = Get-ChildItem -Path $p -File -ErrorAction SilentlyContinue | Where-Object { $_.Extension -eq ".vn" }
    if (-not $files) { throw "no .vn files matched: $Pattern" }
    return $files
}

# Run `vn` with a scoped environment overlay and capture stdout+stderr as one
# string, so a panic or diagnostic counts as part of the observable output.
function Invoke-Vn {
    param([string[]]$VnArgs, [hashtable]$Env = @{})
    $saved = @{}
    foreach ($k in $Env.Keys) {
        $saved[$k] = [Environment]::GetEnvironmentVariable($k)
        [Environment]::SetEnvironmentVariable($k, $Env[$k])
    }
    try {
        return (& $bin @VnArgs 2>&1 | Out-String).Trim()
    }
    finally {
        foreach ($k in $saved.Keys) { [Environment]::SetEnvironmentVariable($k, $saved[$k]) }
    }
}

# Strip compiler progress diagnostics from an output before comparing it.
#
# `[varn-compiler] compiled module: N fn(s) + top-level` is emitted once per
# module the pipeline actually COMPILES, so the count depends on what the
# bytecode cache already holds — which differs between the first run of a
# batch and the rest, and across the `vn cache clean` this script performs at
# provenance switches. That made `tests/main.vn` report DIFFER on all three
# non-baseline tiers while the program output was byte-identical: a false
# positive severe enough to make the check unusable on the one file the
# validation protocol cares most about.
#
# Only build chatter is removed. Program output, panics and diagnostics all
# survive, so a real divergence still shows up.
function Remove-BuildNoise {
    param([string]$Text)
    if (-not $Text) { return $Text }
    return (($Text -split "`r?`n" | Where-Object { $_ -notmatch '^\[varn-compiler\]' }) -join "`n").Trim()
}

$bin = Resolve-VnBin -Explicit $Bin
$files = Get-VnFiles -Pattern $Path

# `Unsupported("<reason>")` inside the lowering error text.
$unsupportedRe = [regex]'Unsupported\("(.+?)"\)'

$diverged = 0

Write-Host ""
Write-Host "  Varn tier divergence check" -ForegroundColor Cyan
Write-Host "  binary : $bin" -ForegroundColor DarkGray
Write-Host "  files  : $($files.Count)" -ForegroundColor DarkGray
Write-Host "  matrix : jit, nojit$(if ($Embedded) { ', embedded, embedded+nojit' })" -ForegroundColor DarkGray
Write-Host ""

foreach ($f in $files) {
    # SSA lowering fallbacks (informational; not a divergence by itself).
    $trace = Invoke-Vn -VnArgs @("debug", "-p", "bytecode", $f.FullName) -Env @{ VN_OPT_TRACE = "1" }
    $fallbacks = ($unsupportedRe.Matches($trace) | ForEach-Object { $_.Groups[1].Value } | Sort-Object -Unique) -join '; '

    $runs = [ordered]@{
        "jit"   = @{ Env = @{}; Purge = $false }
        "nojit" = @{ Env = @{ VARN_NO_JIT = "1" }; Purge = $false }
    }
    if ($Embedded) {
        # Provenance switch invalidates the compile cache (see CLAUDE.md <validation>).
        $runs["embedded"] = @{ Env = @{ VARN_STD = "@embedded" }; Purge = $true }
        $runs["embedded+nojit"] = @{ Env = @{ VARN_STD = "@embedded"; VARN_NO_JIT = "1" }; Purge = $false }
    }

    $outputs = [ordered]@{}
    foreach ($name in $runs.Keys) {
        if ($runs[$name].Purge) { Invoke-Vn -VnArgs @("cache", "clean") -Env $runs[$name].Env | Out-Null }
        $outputs[$name] = Remove-BuildNoise (Invoke-Vn -VnArgs @("run", $f.FullName) -Env $runs[$name].Env)
    }
    if ($Embedded) { Invoke-Vn -VnArgs @("cache", "clean") | Out-Null }

    $baselineName = "jit"
    $baseline = $outputs[$baselineName]
    $bad = @($outputs.Keys | Where-Object { $_ -ne $baselineName -and $outputs[$_] -ne $baseline })

    $status = if ($bad.Count -eq 0) { "MATCH" } else { "DIFFER" }
    $color = if ($bad.Count -eq 0) { "Green" } else { "Red" }
    $detail = if ($bad.Count -eq 0) { "" } else { " ({0})" -f ($bad -join ', ') }
    $fbn = if ($fallbacks) { $fallbacks } else { "(none)" }

    Write-Host ("  {0,-32} " -f $f.Name) -NoNewline
    Write-Host ("{0}{1}" -f $status, $detail) -NoNewline -ForegroundColor $color
    Write-Host ("  fallback: {0}" -f $fbn) -ForegroundColor DarkGray

    if ($bad.Count -gt 0) {
        $diverged++
        if ($ShowOutput) {
            Write-Host "    --- $baselineName ---" -ForegroundColor DarkGray
            $baseline -split "`r?`n" | ForEach-Object { Write-Host "    $_" }
            foreach ($name in $bad) {
                Write-Host "    --- $name ---" -ForegroundColor DarkGray
                $outputs[$name] -split "`r?`n" | ForEach-Object { Write-Host "    $_" }
            }
        }
    }
}

Write-Host ""
Write-Host ("  diverging files : {0} / {1}" -f $diverged, $files.Count) -ForegroundColor $(if ($diverged) { "Red" } else { "Green" })
Write-Host ""

# Exit code = diverging file count (0 = clean). Clamp to byte range for CI gates.
exit ([math]::Min($diverged, 255))
