<#
.SYNOPSIS
    Script de pre-verificación local de Varn para validar cambios antes de commit/push.

.DESCRIPTION
    Ejecuta el conjunto completo de validaciones requeridas por el proyecto:
    1. Formateo de código (cargo fmt)
    2. Linter (cargo clippy)
    3. Auditoría de gobernanza de tamaño de archivos
    4. Compilación del binario de producción (cargo build --release --bin vn)
    5. Matriz obligatoria de 4 cuadrantes sobre tests/main.vn
    6. Benchmark de estabilidad (opcional con -Fast)

.PARAMETER Fast
    Omite el paso de benchmark para iteración rápida.

.PARAMETER SkipLint
    Omite cargo fmt y cargo clippy.

.PARAMETER CleanCache
    Limpia la caché de Varn (`vn cache clean`) antes de cada cuadrante.

.EXAMPLE
    .\scripts\verify.ps1
    .\scripts\verify.ps1 -Fast
#>

param (
    [switch]$Fast,
    [switch]$SkipLint,
    [switch]$CleanCache
)

$ErrorActionPreference = "Stop"
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RootDir = Split-Path -Parent $ScriptDir
Set-Location $RootDir

function Write-StepHeader($title) {
    Write-Host ""
    Write-Host ("=" * 70) -ForegroundColor Cyan
    Write-Host "  >> $title" -ForegroundColor Cyan
    Write-Host ("=" * 70) -ForegroundColor Cyan
}

function Write-Success($msg) {
    Write-Host " [PASS] $msg" -ForegroundColor Green
}

function Write-Failure($msg) {
    Write-Host " [FAIL] $msg" -ForegroundColor Red
}

function Write-WarningMsg($msg) {
    Write-Host " [WARN] $msg" -ForegroundColor Yellow
}

$startTime = [System.Diagnostics.Stopwatch]::StartNew()
$failedSteps = @()

# 1. Formateo
if (-not $SkipLint) {
    Write-StepHeader "1/6: Verificacion de Formateo (cargo fmt --check)"
    cargo fmt --all -- --check
    if ($LASTEXITCODE -ne 0) {
        Write-WarningMsg "El codigo tiene diferencias de formateo con cargo fmt. Puedes ejecutar 'cargo fmt --all' para estandarizar."
    } else {
        Write-Success "Formato de codigo correcto."
    }

    # 2. Linter Clippy
    Write-StepHeader "2/6: Analisis Estatico con Clippy (cargo clippy)"
    cargo clippy --workspace --all-targets
    if ($LASTEXITCODE -ne 0) {
        Write-Failure "Clippy encontro errores de compilacion."
        $failedSteps += "cargo clippy"
    } else {
        Write-Success "Analisis de Clippy completado exitosamente."
    }
} else {
    Write-WarningMsg "Saltando pasos de formateo y clippy (-SkipLint activo)."
}

# 3. Auditoria de Tamano de Archivos
Write-StepHeader "3/6: Auditoria de Gobernanza de Tamano de Archivos (Anti-God Files)"
$largeFiles = Get-ChildItem -Path "crates" -Recurse -Filter "*.rs" |
    ForEach-Object {
        $lines = (Get-Content $_.FullName | Measure-Object -Line).Lines
        [PSCustomObject]@{
            Path   = $_.FullName.Replace("$RootDir\", "")
            Lines  = $lines
            Status = if ($lines -gt 1000) { "ERROR (>1000 lineas)" } elseif ($lines -gt 700) { "ADVERTENCIA (>700 lineas)" } else { "OK" }
        }
    } | Where-Object { $_.Lines -gt 700 } | Sort-Object Lines -Descending

if ($largeFiles) {
    $largeFiles | Format-Table -AutoSize
    $critical = $largeFiles | Where-Object { $_.Lines -gt 1000 }
    if ($critical) {
        Write-Failure "Se detectaron archivos que superan el limite estricto de 1000 lineas (Regla Anti-God File)."
        $failedSteps += "File Size Governance (>1000 lines)"
    } else {
        Write-WarningMsg "Archivos entre 700 y 1000 lineas detectados. Se recomienda evaluar modularizacion."
    }
} else {
    Write-Success "Todos los archivos de crates cumplen con la gobernanza de tamano (<700 lineas)."
}

# 4. Compilacion en Modo Release
Write-StepHeader "4/6: Compilacion de Produccion (cargo build --release --bin vn)"
cargo build --release --bin vn
if ($LASTEXITCODE -ne 0) {
    Write-Failure "Error al compilar el binario de produccion 'vn'."
    $failedSteps += "cargo build --release"
    exit 1
}
Write-Success "Binario 'vn' compilado exitosamente."

$vnBin = Join-Path $RootDir "target\release\vn.exe"
if (-not (Test-Path $vnBin)) {
    $vnBin = Join-Path $RootDir "target\release\vn"
}

# Helper para ejecutar un cuadrante
function Run-Quadrant($name, $envVars, $argsList) {
    Write-Host "`n--- Ejecutando Cuadrante: $name ---" -ForegroundColor Yellow
    
    # Guardar estado anterior de variables
    $prevVars = @{}
    foreach ($k in $envVars.Keys) {
        $prevVars[$k] = [System.Environment]::GetEnvironmentVariable($k, "Process")
        [System.Environment]::SetEnvironmentVariable($k, $envVars[$k], "Process")
    }

    if ($CleanCache) {
        & $vnBin cache clean | Out-Null
    }

    $proc = Start-Process -FilePath $vnBin -ArgumentList $argsList -NoNewWindow -PassThru -Wait
    $code = $proc.ExitCode

    # Restaurar variables de entorno
    foreach ($k in $envVars.Keys) {
        [System.Environment]::SetEnvironmentVariable($k, $prevVars[$k], "Process")
    }

    if ($code -ne 0) {
        Write-Failure "Cuadrante fallido: $name (Exit Code: $code)"
        return $false
    } else {
        Write-Success "Cuadrante superado: $name"
        return $true
    }
}

# 5. Matriz de Validacion de 4 Cuadrantes
Write-StepHeader "5/6: Matriz Obligatoria de 4 Cuadrantes (tests/main.vn)"

# Q1: dev-checkout + JIT
$q1 = Run-Quadrant "1/4: [dev-checkout] + [JIT Habilitado]" @{} @("run", "tests/main.vn")
if (-not $q1) { $failedSteps += "Cuadrante 1 (dev-checkout + JIT)" }

# Q2: dev-checkout + No-JIT (Interprete Pure)
$q2 = Run-Quadrant "2/4: [dev-checkout] + [Interprete Pure (VARN_NO_JIT=1)]" @{ "VARN_NO_JIT" = "1" } @("run", "tests/main.vn")
if (-not $q2) { $failedSteps += "Cuadrante 2 (dev-checkout + No-JIT)" }

# Q3: @embedded + JIT
$q3 = Run-Quadrant "3/4: [@embedded std] + [JIT Habilitado]" @{ "VARN_STD" = "@embedded" } @("run", "tests/main.vn")
if (-not $q3) { $failedSteps += "Cuadrante 3 (@embedded + JIT)" }

# Q4: @embedded + No-JIT (Interprete Pure)
$q4 = Run-Quadrant "4/4: [@embedded std] + [Interprete Pure (VARN_NO_JIT=1)]" @{ "VARN_STD" = "@embedded"; "VARN_NO_JIT" = "1" } @("run", "tests/main.vn")
if (-not $q4) { $failedSteps += "Cuadrante 4 (@embedded + No-JIT)" }

# 6. Benchmark de Estabilidad
if (-not $Fast) {
    Write-StepHeader "6/6: Benchmark de Estabilidad (vn bench benchmarks/bench_fib.vn -v)"
    $proc = Start-Process -FilePath $vnBin -ArgumentList @("bench", "benchmarks/bench_fib.vn", "-v") -NoNewWindow -PassThru -Wait
    if ($proc.ExitCode -ne 0) {
        Write-Failure "El benchmark de estabilidad reporto errores."
        $failedSteps += "Benchmark de Estabilidad"
    } else {
        Write-Success "Benchmark de estabilidad completado satisfactoriamente."
    }
} else {
    Write-WarningMsg "Saltando paso de benchmark (-Fast activo)."
}

$startTime.Stop()
$elapsed = [Math]::Round($startTime.Elapsed.TotalSeconds, 2)

# Resumen Final
Write-Host ""
Write-Host ("=" * 70) -ForegroundColor Cyan
Write-Host "  RESUMEN DE PRE-VERIFICACION LOCAL (Tiempo total: ${elapsed}s)" -ForegroundColor Cyan
Write-Host ("=" * 70) -ForegroundColor Cyan

if ($failedSteps.Count -eq 0) {
    Write-Host "`n [TODO CORRECTO] Todas las verificaciones y la matriz de 4 cuadrantes pasaron exitosamente." -ForegroundColor Green
    Write-Host " El codigo esta listo para produccion y es seguro hacer commit / push.`n" -ForegroundColor Green
    exit 0
} else {
    Write-Host "`n [FALLOS DETECTADOS] Los siguientes pasos fallaron:" -ForegroundColor Red
    foreach ($step in $failedSteps) {
        Write-Host "  - $step" -ForegroundColor Red
    }
    Write-Host "`n Por favor corrige los errores anteriores antes de enviar a produccion.`n" -ForegroundColor Red
    exit 1
}
