param(
    [string]$Vn = (Join-Path $PSScriptRoot '..\..\target\release\vn.exe'),
    [int]$Runs = 5,
    [int]$Warmup = 2,
    [string[]]$Only = @(),
    [string[]]$Runtimes = @('varn', 'bun', 'node')
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

$exe = @{ varn = $vn }
foreach ($name in @('bun', 'node', 'python')) {
    if ($Runtimes -contains $name) {
        $found = Resolve-Executable $name
        if ($found) { $exe[$name] = $found } else { Write-Warning "$name is not a runnable executable; skipped" }
    }
}

$cases = @(Get-ChildItem (Join-Path $root 'vn') -Filter 'bench_*.vn' | ForEach-Object {
        [pscustomobject]@{ Name = $_.BaseName -replace '^bench_', ''; Varn = $_.FullName }
    })
if ($Only.Count -gt 0) { $cases = @($cases | Where-Object { $Only -contains $_.Name }) }
if ($cases.Count -eq 0) { throw "no benchmarks selected (wanted: $($Only -join ', '))" }

$extension = @{ bun = 'js'; node = 'js'; python = 'py' }
$rows = foreach ($case in $cases) {
    $row = [ordered]@{ Workload = $case.Name }
    foreach ($name in $exe.Keys) {
        if ($name -eq 'varn') {
            $file = $case.Varn
            $argv = @('run', "`"$file`"")
        } else {
            $file = Join-Path (Join-Path $root $extension[$name]) "$($case.Name).$($extension[$name])"
            if (-not (Test-Path $file)) { continue }
            $argv = @("`"$file`"")
        }
        $samples = @()
        for ($i = 0; $i -lt ($Warmup + $Runs); $i = $i + 1) {
            $psi = New-Object Diagnostics.ProcessStartInfo
            $psi.FileName = $exe[$name]
            $psi.Arguments = $argv -join ' '
            $psi.UseShellExecute = $false
            $psi.RedirectStandardOutput = $true
            $psi.RedirectStandardError = $true
            $clock = [Diagnostics.Stopwatch]::StartNew()
            $p = [Diagnostics.Process]::Start($psi)
            $null = $p.StandardOutput.ReadToEndAsync()
            $null = $p.StandardError.ReadToEndAsync()
            $p.WaitForExit()
            $clock.Stop()
            if ($p.ExitCode -ne 0) { throw "$name failed on $($case.Name) with exit code $($p.ExitCode)" }
            if ($i -ge $Warmup) { $samples += $clock.Elapsed.TotalMilliseconds }
        }
        $sorted = @($samples | Sort-Object)
        $row[$name] = [math]::Round($sorted[[int](($sorted.Count - 1) / 2)], 1)
    }
    [pscustomobject]$row
}

$names = @($exe.Keys)
$header = 'Workload'.PadRight(24) + (($names | ForEach-Object { "$_ ms".PadLeft(10) }) -join '')
Write-Host $header
foreach ($row in $rows) {
    $line = "$($row.Workload)".PadRight(24)
    foreach ($name in $names) { $line += "$($row.$name)".PadLeft(10) }
    Write-Host $line
}
