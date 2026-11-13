param(
    [string]$Vn = (Join-Path $PSScriptRoot '..\..\target\release\vn.exe'),
    [int]$Runs = 5,
    [int]$Warmup = 2,
    [string[]]$Only = @(),
    [string[]]$Runtimes = @('vn', 'bun', 'node', 'python'),
    [switch]$List
)

$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot
$vn = (Resolve-Path $Vn).Path
$Only = @($Only | ForEach-Object { $_ -split ',' } | ForEach-Object { $_.Trim() } | Where-Object { $_ })
$Runtimes = @($Runtimes | ForEach-Object { $_ -split ',' } | ForEach-Object { $_.Trim() } | Where-Object { $_ })

function Resolve-Executable($name) {
    $cmd = Get-Command $name -ErrorAction SilentlyContinue
    if (-not $cmd) { return $null }
    $src = $cmd.Source
    if ($src -match '\.exe$') {
        try { & $src --version *> $null; if ($LASTEXITCODE -eq 0) { return $src } } catch {}
        return $null
    }
    $candidate = Join-Path (Split-Path $src) "node_modules\$name\bin\$name.exe"
    if (Test-Path $candidate) { return $candidate }
    return $null
}

function Measure-Median($exePath, $argvList) {
    $samples = @()
    for ($i = 0; $i -lt ($Warmup + $Runs); $i = $i + 1) {
        $psi = New-Object Diagnostics.ProcessStartInfo
        $psi.FileName = $exePath
        $psi.Arguments = $argvList -join ' '
        $psi.UseShellExecute = $false
        $psi.RedirectStandardOutput = $true
        $psi.RedirectStandardError = $true
        $clock = [Diagnostics.Stopwatch]::StartNew()
        $p = [Diagnostics.Process]::Start($psi)
        try { $p.PriorityClass = [Diagnostics.ProcessPriorityClass]::High } catch {}
        $null = $p.StandardOutput.ReadToEndAsync()
        $null = $p.StandardError.ReadToEndAsync()
        $p.WaitForExit()
        $clock.Stop()
        if ($p.ExitCode -ne 0) { throw "$exePath $($argvList -join ' ') exited with $($p.ExitCode)" }
        if ($i -ge $Warmup) { $samples += $clock.Elapsed.TotalMilliseconds }
    }
    $sorted = @($samples | Sort-Object)
    [math]::Round($sorted[[int](($sorted.Count - 1) / 2)], 1)
}

$exe = @{ vn = $vn }
foreach ($name in @('bun', 'node', 'python')) {
    if ($Runtimes -contains $name) {
        $found = Resolve-Executable $name
        if ($found) { $exe[$name] = $found } else { Write-Warning "$name is not a runnable executable; skipped" }
    }
}

$extension = @{ bun = 'js'; node = 'js'; python = 'py' }
$startup = @{}
foreach ($name in $exe.Keys) {
    $ext = if ($name -eq 'vn') { 'vn' } else { $extension[$name] }
    $file = Join-Path (Join-Path $root 'startup') "empty.$ext"
    if (-not (Test-Path $file)) { continue }
    if ($name -eq 'vn') {
        $startup[$name] = Measure-Median $exe[$name] @('run', "`"$file`"")
    } else {
        $startup[$name] = Measure-Median $exe[$name] @("`"$file`"")
    }
}

$cases = @(Get-ChildItem (Join-Path $root 'vn') -Filter 'bench_*.vn' | ForEach-Object {
        [pscustomobject]@{ Name = $_.BaseName -replace '^bench_', ''; Varn = $_.FullName }
    })
if ($Only.Count -gt 0) { $cases = @($cases | Where-Object { $Only -contains $_.Name }) }
if ($cases.Count -eq 0) { throw "no benchmarks selected (wanted: $($Only -join ', '))" }
if ($List) { $cases | ForEach-Object { $_.Name }; exit 0 }

$rows = foreach ($case in $cases) {
    $row = [ordered]@{ Workload = $case.Name }
    foreach ($name in $exe.Keys) {
        if ($name -eq 'vn') {
            $argv = @('run', "`"$($case.Varn)`"")
        } else {
            $ext = $extension[$name]
            $file = Join-Path (Join-Path $root $ext) "$($case.Name).$ext"
            if (-not (Test-Path $file)) { continue }
            $argv = @("`"$file`"")
        }
        $row[$name] = Measure-Median $exe[$name] $argv
    }
    [pscustomobject]$row
}

$names = @($exe.Keys)
function Print-Table($title, $selector) {
    Write-Host ""
    Write-Host $title
    $header = 'Workload'.PadRight(24) + (($names | ForEach-Object { "$_".PadLeft(10) }) -join '')
    Write-Host $header
    foreach ($row in $rows) {
        $line = "$($row.Workload)".PadRight(24)
        foreach ($name in $names) {
            $value = $row.$name
            if ($null -eq $value) { $line += '-'.PadLeft(10); continue }
            $line += ("{0:N1}" -f (& $selector $value $name)).PadLeft(10)
        }
        Write-Host $line
    }
}

Print-Table 'Wall per process (ms, median)' { param($value, $name) $value }
Print-Table 'Steady state (wall minus startup, ms)' {
    param($value, $name)
    [math]::Max(0, $value - $startup[$name])
}
