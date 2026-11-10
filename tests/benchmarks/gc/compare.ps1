param(
    [string]$Vn = (Join-Path $PSScriptRoot '..\..\..\target\release\vn.exe'),
    [int]$Runs = 3,
    [string[]]$Workloads = @('w1', 'w3', 'w5')
)

$ErrorActionPreference = 'Stop'
$work = Join-Path ([IO.Path]::GetTempPath()) 'varn-gc-compare'
Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force "$work\go", "$work\net" | Out-Null
Copy-Item "$PSScriptRoot\go\*" "$work\go" -Force
Copy-Item "$PSScriptRoot\net\*" "$work\net" -Force

$runtimes = [ordered]@{}
$node = Get-Command node -ErrorAction SilentlyContinue
if ($node) { $runtimes['node'] = { param($w) @($node.Source, "`"$PSScriptRoot\gcbench.js`" $w") } }
if (Get-Command go -ErrorAction SilentlyContinue) {
    Push-Location "$work\go"; go build -o gcbench.exe .; Pop-Location
    $runtimes['go'] = { param($w) @("$work\go\gcbench.exe", $w) }
}
$dotnet = Get-Command dotnet -ErrorAction SilentlyContinue
if ($dotnet) {
    dotnet build "$work\net\gcbench.csproj" -c Release -o "$work\net\out" --nologo -v q | Out-Null
    $runtimes['dotnet'] = { param($w) @($dotnet.Source, "`"$work\net\out\gcbench.dll`" $w") }
}

function Invoke-Measured($exe, $arguments) {
    $psi = New-Object Diagnostics.ProcessStartInfo
    $psi.FileName = $exe
    $psi.Arguments = $arguments
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.UseShellExecute = $false
    $p = [Diagnostics.Process]::Start($psi)
    $peak = 0
    while (-not $p.HasExited) {
        try { $p.Refresh(); if ($p.PeakWorkingSet64 -gt $peak) { $peak = $p.PeakWorkingSet64 } } catch {}
        Start-Sleep -Milliseconds 15
    }
    [pscustomobject]@{ Out = $p.StandardOutput.ReadToEnd().Trim(); PeakMb = [math]::Round($peak / 1MB) }
}

function Get-Median($values) {
    $sorted = @($values | Sort-Object)
    $sorted[[int](($sorted.Count - 1) / 2)]
}

$rows = foreach ($w in $Workloads) {
    foreach ($name in $runtimes.Keys) {
        $samples = 1..$Runs | ForEach-Object {
            $cmd = & $runtimes[$name] $w
            $r = Invoke-Measured $cmd[0] $cmd[1]
            $j = $r.Out | ConvertFrom-Json
            $maxPause = if ($null -ne $j.pause_max_ms) { $j.pause_max_ms } elseif ($null -ne $j.major_max_ms) { [math]::Max($j.major_max_ms, $j.minor_max_ms) } else { $null }
            $gcs = if ($null -ne $j.gcs) { $j.gcs } else { [int]$j.gc0 + [int]$j.gc1 + [int]$j.gc2 }
            [pscustomobject]@{ Wall = $j.wall_ms; Peak = $r.PeakMb; Pause = [double]$j.pause_total_ms; MaxPause = $maxPause; Gcs = $gcs }
        }
        [pscustomobject]@{
            Workload = $w; Runtime = $name
            WallMs = Get-Median $samples.Wall; PeakMb = Get-Median $samples.Peak
            PauseTotalMs = Get-Median $samples.Pause
            MaxPauseMs = if ($null -ne $samples[0].MaxPause) { Get-Median $samples.MaxPause } else { 'n/d' }
            Collections = Get-Median $samples.Gcs
        }
    }
    if (Test-Path $Vn) {
        $samples = 1..$Runs | ForEach-Object {
            $o = & $Vn bench "$PSScriptRoot\$w.vn" --runs 1 --warmup 0 --allow-all -v 2>&1
            $get = { param($k) $m = $o | Select-String -Pattern ('^' + [regex]::Escape($k) + '\s+([\d.]+)') | Select-Object -First 1; if ($m) { [double]$m.Matches[0].Groups[1].Value } else { 0 } }
            $exec = ($o | Select-String '^execute \(warm\)\s+[\d.]+\s+([\d.]+)' | Select-Object -First 1).Matches[0].Groups[1].Value
            [pscustomobject]@{
                Wall = [double]$exec
                Peak = [math]::Round((& $get 'peak memory'))
                Pause = ((& $get 'major pause total ns') + (& $get 'minor pause total ns')) / 1e6
                MaxPause = [math]::Max((& $get 'major pause max ns'), (& $get 'minor pause max ns')) / 1e6
                Gcs = [int]((& $get 'collections') + (& $get 'minor runs'))
            }
        }
        [pscustomobject]@{
            Workload = $w; Runtime = 'varn'
            WallMs = [math]::Round((Get-Median $samples.Wall)); PeakMb = Get-Median $samples.Peak
            PauseTotalMs = [math]::Round((Get-Median $samples.Pause), 1)
            MaxPauseMs = [math]::Round((Get-Median $samples.MaxPause), 1)
            Collections = Get-Median $samples.Gcs
        }
    }
}
$rows | Format-Table -AutoSize
