<#
.SYNOPSIS
    Scan .vn files for every value the LSP resolves to `dynamic`.

.DESCRIPTION
    Runs `vn debug -p lsp:hovers` over each file and collects the bindings
    (const / let / var / param / prop / member access) whose resolved type
    contains `dynamic`. Useful to hunt inference holes: a value that should
    have a precise type but fell back to `dynamic`.

    Each finding is tagged:
      [inferred] - the type came from inference (no `: dynamic` written in source)
      [explicit] - the user annotated `: dynamic` at the declaration site

    Inference bugs live in the [inferred] set. Explicit annotations are shown
    for completeness but can be hidden with -InferredOnly.

    NOTE: the LSP debug dashboard writes to STDERR, so this script captures it.

.PARAMETER Path
    Glob or directory of .vn files. Default: tests/*.vn (relative to repo root).

.PARAMETER Bin
    Path to the vn binary. Auto-detected (release, then debug) if omitted.

.PARAMETER ShowAll
    List every occurrence instead of collapsing repeats of the same binding.

.PARAMETER InferredOnly
    Hide explicit `: dynamic` annotations; show only inferred dynamics.

.PARAMETER Json
    Emit findings as JSON instead of the human-readable report.

.EXAMPLE
    pwsh -File .claude/skills/varn-dynamic-audit/scripts/find-dynamic-inference.ps1
    pwsh -File .claude/skills/varn-dynamic-audit/scripts/find-dynamic-inference.ps1 -InferredOnly
    pwsh -File .claude/skills/varn-dynamic-audit/scripts/find-dynamic-inference.ps1 -Path tests/47-isolates-multithread.vn -ShowAll
#>
[CmdletBinding()]
param(
    [string]$Path = "tests/*.vn",
    [string]$Bin,
    [switch]$ShowAll,
    [switch]$InferredOnly,
    [switch]$Json
)

$ErrorActionPreference = "Stop"
# vn exits non-zero on files with diagnostics; don't let that abort the scan
# (PS 7.4+ promotes native non-zero to a terminating error under Stop).
$PSNativeCommandUseErrorActionPreference = $false
# vn prints UTF-8 (the hover `→` etc.). Decode native output as UTF-8 so it
# isn't mangled by the console's OEM codepage (would break the line regex).
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

$bin = Resolve-VnBin -Explicit $Bin
$files = Get-VnFiles -Pattern $Path

# Hover line:   ( 5:11) data            -> const data: dynamic
# Capture position + the whole tail; the `->`/`→` separator is not matched so a
# mis-decoded arrow glyph can't break parsing.
$hoverRe = [regex]'^\s*\(\s*(\d+):\s*(\d+)\)\s+(.*)$'
# Binding kinds whose own type we care about (skip class/method/interface/fn
# sigs). Name stops before the `:` so object-literal types keep their colons.
$bindRe  = [regex]'(?:^|\s)(const|let|var|prop|\(param\)|\(property\))\s+([^\s:]+):\s+(.*)$'
$dynRe   = [regex]'\bdynamic\b'

$findings = New-Object System.Collections.Generic.List[object]

foreach ($f in $files) {
    # The LSP debug dashboard goes to STDERR. Redirect it to a temp file and
    # read raw text: on Windows PowerShell 5.1 a `2>&1` merge wraps each stderr
    # line in an ErrorRecord, which Out-String reformats and breaks the regex.
    $errFile = [System.IO.Path]::GetTempFileName()
    try { & $bin debug -p lsp:hovers $f.FullName 2> $errFile > $null } catch { }
    $raw = Get-Content -LiteralPath $errFile -Raw -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $errFile -Force -ErrorAction SilentlyContinue
    if (-not $raw) { continue }
    $clean = [regex]::Replace($raw, "\x1b\[[0-9;]*m", "")
    $srcLines = Get-Content -LiteralPath $f.FullName

    foreach ($line in ($clean -split "`r?`n")) {
        $m = $hoverRe.Match($line)
        if (-not $m.Success) { continue }
        $bm = $bindRe.Match($m.Groups[3].Value)
        if (-not $bm.Success) { continue }

        $ln   = [int]$m.Groups[1].Value
        $col  = [int]$m.Groups[2].Value
        $kind = $bm.Groups[1].Value
        $name = $bm.Groups[2].Value.Trim()
        $type = $bm.Groups[3].Value.Trim()
        if (-not $dynRe.IsMatch($type)) { continue }

        # Explicit if the source declaration line annotates `<name>: ...dynamic`.
        $explicit = $false
        if ($ln -ge 1 -and $ln -le $srcLines.Count) {
            $src = $srcLines[$ln - 1]
            $annRe = "(?<![A-Za-z0-9_.])" + [regex]::Escape($name) + "\s*:\s*[^=,)]*?\bdynamic\b"
            if ([regex]::IsMatch($src, $annRe)) { $explicit = $true }
        }

        $findings.Add([pscustomobject]@{
            File     = $f.Name
            FullPath = $f.FullName
            Line     = $ln
            Col      = $col
            Kind     = $kind
            Name     = $name
            Type     = $type
            Explicit = $explicit
        })
    }
}

# Explicit-ness is a property of the declaration, not of each hover. A usage
# site (e.g. `res1` in an `assert`) has no annotation on its line and would
# look "inferred". Promote the flag: if ANY occurrence of a binding is
# explicit, the whole binding is explicit.
$findings | Group-Object File, Kind, Name, Type | ForEach-Object {
    $anyExplicit = ($_.Group | Where-Object { $_.Explicit }).Count -gt 0
    foreach ($g in $_.Group) { $g.Explicit = $anyExplicit }
}

if ($InferredOnly) { $findings = $findings | Where-Object { -not $_.Explicit } }

# Collapse repeated uses of the same binding unless -ShowAll.
if (-not $ShowAll) {
    $findings = $findings |
        Group-Object File, Kind, Name, Type |
        ForEach-Object {
            $first = $_.Group | Sort-Object Line, Col | Select-Object -First 1
            $first | Add-Member -NotePropertyName Uses -NotePropertyValue $_.Count -PassThru
        }
} else {
    $findings | ForEach-Object { $_ | Add-Member -NotePropertyName Uses -NotePropertyValue 1 -Force }
}

$findings = @($findings | Sort-Object File, Line, Col)

if ($Json) {
    $findings | ConvertTo-Json -Depth 4
    exit ([math]::Min($findings.Count, 255))
}

$inf = @($findings | Where-Object { -not $_.Explicit })
$exp = @($findings | Where-Object { $_.Explicit })
$byFile = $findings | Group-Object File

Write-Host ""
Write-Host "  LSP dynamic-inference scan" -ForegroundColor Cyan
Write-Host "  binary : $bin" -ForegroundColor DarkGray
Write-Host "  files  : $($files.Count) scanned, $($byFile.Count) with dynamic" -ForegroundColor DarkGray
Write-Host ""

foreach ($g in ($byFile | Sort-Object Name)) {
    Write-Host "  $($g.Name)" -ForegroundColor White
    foreach ($it in ($g.Group | Sort-Object Line, Col)) {
        $loc = "{0}:{1}" -f $it.Line, $it.Col
        $tag = if ($it.Explicit) { "[explicit]" } else { "[inferred]" }
        $tagColor = if ($it.Explicit) { "DarkGray" } else { "Yellow" }
        $uses = if ($it.Uses -gt 1) { " x$($it.Uses)" } else { "" }
        Write-Host ("    {0,-8} " -f $loc) -NoNewline -ForegroundColor DarkGray
        Write-Host ("{0,-12} " -f $tag) -NoNewline -ForegroundColor $tagColor
        Write-Host ("{0} {1}: {2}{3}" -f $it.Kind, $it.Name, $it.Type, $uses)
    }
    Write-Host ""
}

Write-Host "  ── summary ─────────────────────────────" -ForegroundColor Cyan
Write-Host ("  inferred dynamic : {0}" -f $inf.Count) -ForegroundColor Yellow
Write-Host ("  explicit dynamic : {0}" -f $exp.Count) -ForegroundColor DarkGray
Write-Host ("  files affected   : {0}" -f $byFile.Count)
Write-Host ""

# Exit code = inferred count (0 = clean). Clamp to byte range for CI gates.
exit ([math]::Min($inf.Count, 255))
