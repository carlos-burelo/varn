<#
.SYNOPSIS
    Checks ejecutables del comportamiento externo de `vn`.

.DESCRIPTION
    Cada check imprime PASS <nombre> o FAIL <nombre>. Termina con ALL CHECKS PASSED o exit code 1.
    Los temporales viven en target/tmp/smoke.
#>

param (
    [string]$Profile = "quick"
)

$ErrorActionPreference = "Stop"
$RootDir = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $RootDir

$vn = Join-Path $RootDir "target\$Profile\vn.exe"
if (-not (Test-Path $vn)) { $vn = Join-Path $RootDir "target\$Profile\vn" }

$work = Join-Path $RootDir "target\tmp\smoke"
if (Test-Path $work) { Remove-Item $work -Recurse -Force }
New-Item -ItemType Directory -Force $work | Out-Null
$env:VARN_CACHE_DIR = Join-Path $work "cache"

$failed = @()

function Vn {
    $out = & $vn @args 2>&1 | Out-String
    [pscustomobject]@{ Code = $LASTEXITCODE; Out = $out }
}

function Check($name, [scriptblock]$body) {
    try {
        $ok = & $body
    } catch {
        $ok = $false
    }
    if ($ok) {
        Write-Host "PASS $name"
    } else {
        Write-Host "FAIL $name"
        $script:failed += $name
    }
}

function Write-Vn($name, $text) {
    $path = Join-Path $work $name
    New-Item -ItemType Directory -Force (Split-Path $path) | Out-Null
    Set-Content $path $text -Encoding utf8
    $path
}

Check "run-prints" {
    $f = Write-Vn "hello.vn" 'print("smoke-ok");'
    $r = Vn run $f
    $r.Code -eq 0 -and $r.Out -match "smoke-ok"
}

Check "check-reports-error-with-code-3" {
    $f = Write-Vn "bad.vn" 'let x: int = "text";'
    $r = Vn check $f
    $r.Code -eq 3 -and $r.Out -match "could not compile"
}

Check "eval-runs" {
    $r = Vn eval 'print(1 + 1);'
    $r.Code -eq 0 -and $r.Out -match "2"
}

Check "eval-syntax-error-renders-with-code-3" {
    $r = Vn eval 'let ='
    $r.Code -eq 3 -and $r.Out -match '\(eval\)' -and $r.Out -match 'could not compile'
}

Check "fmt-check-passes-on-formatted" {
    $f = Write-Vn "fmt\ok.vn" 'print("a");'
    & $vn fmt $f *> $null
    $r = Vn fmt --check $f
    $r.Code -eq 0
}

Check "test-fails-when-a-suite-fails" {
    Write-Vn "suite\pass.vn" 'print("p");' | Out-Null
    Write-Vn "suite\fail.vn" 'throw "boom";' | Out-Null
    $r = Vn test (Join-Path $work "suite")
    $r.Code -ne 0
}

if ($failed.Count -gt 0) {
    Write-Host "FAILED: $($failed -join ', ')"
    exit 1
}
Write-Host "ALL CHECKS PASSED"
