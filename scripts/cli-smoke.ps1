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
        Write-Host "  $($_.Exception.Message)"
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

Check "init-writes-a-manifest-pkg-reads" {
    $dir = Join-Path $work "initproj"
    $init = Vn init $dir
    Push-Location $dir
    try { $tree = Vn pkg tree } finally { Pop-Location }
    $init.Code -eq 0 -and $tree.Code -eq 0
}

Check "manifest-unknown-key-is-named" {
    $dir = Join-Path $work "badmanifest"
    New-Item -ItemType Directory -Force $dir | Out-Null
    Set-Content (Join-Path $dir "varn.toml") 'nmae = "x"' -Encoding utf8
    $f = Write-Vn "badmanifest\hello.vn" 'print("hi");'
    $r = Vn run $f
    $r.Code -ne 0 -and $r.Out -match "nmae" -and $r.Out -match "varn.toml"
}

Check "cache-hit-on-second-run" {
    $f = Write-Vn "cache\hello.vn" 'print("cached");'
    $first = Vn run -v $f
    $second = Vn run -v $f
    $first.Out -match "compile cache miss" -and $second.Out -match "compile cache hit" -and $second.Code -eq 0
}

Check "cache-concurrent-writers" {
    $f = Write-Vn "concurrent\hello.vn" 'print("same");'
    $runs = 1..8 | ForEach-Object -ThrottleLimit 8 -Parallel {
        $out = & $using:vn run $using:f 2>&1 | Out-String
        [pscustomobject]@{ Code = $LASTEXITCODE; Out = $out.Trim() }
    }
    ($runs | Where-Object { $_.Code -ne 0 -or $_.Out -ne "same" }).Count -eq 0
}

Check "eval-honors-permissions" {
    $code = 'import { readText } from "std:fs"; print(readText("Cargo.toml").length > 0)'
    $denied = Vn eval --allow-net $code
    $granted = Vn eval --allow-read $code
    $denied.Code -ne 0 -and $denied.Out -match "PERMISSION" -and $granted.Code -eq 0 -and $granted.Out -match "true"
}

if (Test-Path (Join-Path (Split-Path $vn) "varn_rt.lib")) {
    Check "build-native-links-and-runs" {
        $f = Write-Vn "native\hello.vn" 'print("native-ok");'
        $out = Join-Path $work "native\hello.exe"
        $built = Vn build $f --target native -o $out
        $ran = & $out 2>&1 | Out-String
        $built.Code -eq 0 -and $ran.Trim() -eq "native-ok"
    }
} else {
    Write-Host "SKIP build-native-links-and-runs (varn_rt.lib not built)"
}

Check "exe-runs-elsewhere" {
    $f = Write-Vn "exe\hello.vn" 'print("standalone-ok");'
    $exe = Join-Path $work "exe\hello.exe"
    $built = Vn build $f --target exe -o $exe
    Push-Location (Join-Path $work "cache")
    try { $ran = & $exe 2>&1 | Out-String } finally { Pop-Location }
    $built.Code -eq 0 -and $ran.Trim() -eq "standalone-ok"
}

Check "exe-corrupt-trailer-is-an-error" {
    $exe = Join-Path $work "exe\hello.exe"
    $bad = Join-Path $work "exe\corrupt.exe"
    $bytes = [IO.File]::ReadAllBytes($exe)
    $payload = [BitConverter]::ToUInt64($bytes, $bytes.Length - 12)
    $start = [int]($bytes.Length - 12 - $payload)
    $bytes[$start + 40] = $bytes[$start + 40] -bxor 0xFF
    [IO.File]::WriteAllBytes($bad, $bytes)
    $out = & $bad 2>&1 | Out-String
    $LASTEXITCODE -ne 0 -and $out -match "corrupt"
}

function Write-Tasks($dir, $toml) {
    New-Item -ItemType Directory -Force (Join-Path $work $dir) | Out-Null
    Set-Content (Join-Path (Join-Path $work $dir) "varn.toml") $toml -Encoding utf8
    Join-Path $work $dir
}

Check "task-runs-in-order-from-the-project-root" {
    $dir = Write-Tasks "tasks-order" @'
[tasks]
hello = ["eval 'print(1)'", "eval 'print(2)'"]
'@
    New-Item -ItemType Directory -Force (Join-Path $dir "sub") | Out-Null
    Push-Location (Join-Path $dir "sub")
    try { $r = Vn task hello } finally { Pop-Location }
    $lines = @($r.Out -split "`r?`n" | Where-Object { $_ -match '^\d$' })
    $r.Code -eq 0 -and $lines.Count -eq 2 -and $lines[0] -eq "1" -and $lines[1] -eq "2"
}

Check "task-stops-on-failure" {
    $dir = Write-Tasks "tasks-stop" @'
[tasks]
broken = ["eval 'print(\"first-ran\")'", "eval 'let ='", "eval 'print(\"third-ran\")'"]
'@
    Push-Location $dir
    try { $r = Vn task broken } finally { Pop-Location }
    $r.Code -ne 0 -and $r.Out -match "first-ran" -and $r.Out -notmatch "third-ran"
}

Check "task-cycle-detected" {
    $dir = Write-Tasks "tasks-cycle" @'
[tasks]
a = ["task b"]
b = ["task a"]
'@
    Push-Location $dir
    try { $r = Vn task a } finally { Pop-Location }
    $r.Code -ne 0 -and $r.Out -match "task cycle"
}

Check "debug-all-phases-on-a-generic-program" {
    $r = Vn debug (Join-Path $RootDir "crates/varn-pipeline/tests/fixtures/debug/generics.vn") -p all
    $r.Code -eq 0 -and $r.Out -notmatch "panicked"
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
