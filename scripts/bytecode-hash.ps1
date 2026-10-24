<#
.SYNOPSIS
    Hashea el bytecode que `vn build` produce para tests/ y examples/ y lo compara con una base.

.DESCRIPTION
    Ley 4: mismo input, mismo bytecode. -Record guarda la base en target/tmp/baseline/bytecode.tsv;
    -Compare regenera los hashes y reporta IDENTICAL o cada diferencia. Un archivo que no compila
    se registra como ERROR:<hash de su salida de diagnóstico>, de modo que un cambio de render también
    se detecta.

.PARAMETER Record
    Guarda la base.

.PARAMETER Compare
    Compara contra la base.

.PARAMETER Profile
    Perfil del binario vn (por defecto quick).
#>

param (
    [switch]$Record,
    [switch]$Compare,
    [string]$Profile = "quick"
)

$ErrorActionPreference = "Stop"
$RootDir = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $RootDir

if ($Record -eq $Compare) {
    Write-Host "usage: bytecode-hash.ps1 -Record | -Compare"
    exit 2
}

$vn = Join-Path $RootDir "target\$Profile\vn.exe"
if (-not (Test-Path $vn)) { $vn = Join-Path $RootDir "target\$Profile\vn" }

$baseDir = Join-Path $RootDir "target\tmp\baseline"
$outDir = Join-Path $RootDir "target\tmp\bytecode"
$baseFile = Join-Path $baseDir "bytecode.tsv"
New-Item -ItemType Directory -Force $baseDir, $outDir | Out-Null
Get-ChildItem $outDir -File | Remove-Item -Force

$files = @(Get-ChildItem tests, examples -Recurse -Filter *.vn -File |
    ForEach-Object { $_.FullName.Substring($RootDir.Length + 1).Replace('\', '/') } |
    Sort-Object)

$lines = $files | ForEach-Object -ThrottleLimit 8 -Parallel {
    $rel = $_
    $out = Join-Path $using:outDir ($rel.Replace('/', '__') + ".vnc")
    $text = & $using:vn build $rel -o $out 2>&1 | Out-String
    if ($LASTEXITCODE -eq 0 -and (Test-Path $out)) {
        "$rel`t$((Get-FileHash $out -Algorithm SHA256).Hash)"
    } else {
        $bytes = [Text.Encoding]::UTF8.GetBytes($text)
        $hash = [BitConverter]::ToString([Security.Cryptography.SHA256]::HashData($bytes)).Replace('-', '')
        "$rel`tERROR:$hash"
    }
} | Sort-Object

if ($Record) {
    $lines | Set-Content $baseFile -Encoding utf8
    $errors = @($lines | Where-Object { $_ -like "*`tERROR:*" }).Count
    Write-Host "RECORDED $($lines.Count) files ($errors ERROR) -> $baseFile"
    exit 0
}

if (-not (Test-Path $baseFile)) {
    Write-Host "no baseline: run -Record first"
    exit 2
}

$base = @(Get-Content $baseFile)
$diff = Compare-Object $base $lines
if (-not $diff) {
    Write-Host "IDENTICAL ($($lines.Count) files)"
    exit 0
}
$diff | ForEach-Object { Write-Host "$($_.SideIndicator) $($_.InputObject)" }
Write-Host "DIFFERENT"
exit 1
