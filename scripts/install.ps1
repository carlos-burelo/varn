param (
    [string]$Dest
)

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot

if (-not $Dest) {
    $varnHome = if ($env:VARN_HOME) { $env:VARN_HOME } else { Join-Path $HOME ".vn" }
    $Dest = Join-Path $varnHome "bin"
}
New-Item -ItemType Directory -Force $Dest | Out-Null

Push-Location $repoRoot
try {
    & cargo build --release -p varn-cli -p varn-shadow
    if ($LASTEXITCODE -ne 0) { throw "cargo build falló (exit $LASTEXITCODE)" }
} finally {
    Pop-Location
}

$stamp = Get-Date -Format "yyyyMMddHHmmssfff"
foreach ($bin in @("vn.exe", "vn-shadow.exe")) {
    $installed = Join-Path $Dest $bin
    Get-ChildItem -Path $Dest -Filter "$bin.old-*" -ErrorAction SilentlyContinue |
        ForEach-Object { Remove-Item -Force $_.FullName -ErrorAction SilentlyContinue }
    if (Test-Path $installed) {
        Rename-Item -Path $installed -NewName "$bin.old-$stamp"
    }
    Copy-Item -Path (Join-Path $repoRoot "target\release\$bin") -Destination $installed
    Write-Host "instalado: $installed" -ForegroundColor Green
}
