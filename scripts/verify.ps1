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

.PARAMETER Quick
    Ciclo de iteración: compila con el perfil `quick` (sin LTO, codegen-units=16),
    omite lint y benchmark, y corre solo el cuadrante 1 (dev-checkout + JIT).
    No sustituye a la matriz completa antes de commitear.

.EXAMPLE
    .\scripts\verify.ps1
    .\scripts\verify.ps1 -Fast
    .\scripts\verify.ps1 -Quick
#>

param (
    [switch]$Fast,
    [switch]$SkipLint,
    [switch]$CleanCache,
    [switch]$Quick
)

if ($Quick) {
    $SkipLint = $true
    $Fast = $true
}
$BuildProfile = if ($Quick) { "quick" } else { "release" }

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

# 3. Auditoria de Tamano de Archivos (techo AGENTS §6: 400; >1000 falla)
Write-StepHeader "3/6: Auditoria de Gobernanza de Tamano de Archivos (Anti-God Files)"
$largeFiles = Get-ChildItem -Path "crates" -Recurse -Filter "*.rs" |
    ForEach-Object {
        $lines = (Get-Content $_.FullName | Measure-Object -Line).Lines
        [PSCustomObject]@{
            Path   = $_.FullName.Replace("$RootDir\", "")
            Lines  = $lines
            Status = if ($lines -gt 1000) { "ERROR (>1000 lineas)" } elseif ($lines -gt 400) { "ADVERTENCIA (>400 lineas)" } else { "OK" }
        }
    } | Where-Object { $_.Lines -gt 400 } | Sort-Object Lines -Descending

if ($largeFiles) {
    $largeFiles | Format-Table -AutoSize
    $critical = $largeFiles | Where-Object { $_.Lines -gt 1000 }
    if ($critical) {
        Write-Failure "Se detectaron archivos que superan el limite estricto de 1000 lineas (Regla Anti-God File)."
        $failedSteps += "File Size Governance (>1000 lines)"
    } else {
        Write-WarningMsg "Archivos entre 400 y 1000 lineas detectados. Techo AGENTS §6: 400 por dominio."
    }
} else {
    Write-Success "Todos los archivos de crates cumplen con la gobernanza de tamano (<400 lineas)."
}

# 4. Compilacion en Modo Release
Write-StepHeader "4/6: Compilacion (cargo build --profile $BuildProfile --bin vn)"
cargo build --profile $BuildProfile --bin vn
if ($LASTEXITCODE -ne 0) {
    Write-Failure "Error al compilar el binario 'vn' (perfil $BuildProfile)."
    $failedSteps += "cargo build --profile $BuildProfile"
    exit 1
}
Write-Success "Binario 'vn' compilado exitosamente (perfil $BuildProfile)."

$vnBin = Join-Path $RootDir "target\$BuildProfile\vn.exe"
if (-not (Test-Path $vnBin)) {
    $vnBin = Join-Path $RootDir "target\$BuildProfile\vn"
}

# 4b. Artefactos derivados (tablas de contratos + bundle std para cuadrantes embedded)
Write-StepHeader "4b/6: Artefactos derivados (gen-contract-tables --check)"
& $vnBin gen-contract-tables --check
if ($LASTEXITCODE -ne 0) {
    Write-Failure "Tablas de contratos stale: regenerar con 'vn gen-contract-tables'."
    $failedSteps += "gen-contract-tables --check"
} else {
    Write-Success "Tablas de contratos al dia."
}

$stdBundle = Join-Path $RootDir "dist\std.vnb"
if (-not $Quick) {
    Write-StepHeader "4c/6: Bundle std para cuadrantes @embedded (vn std-bundle)"
    & $vnBin std-bundle --std-dir "std" --out $stdBundle
    if ($LASTEXITCODE -ne 0) {
        Write-Failure "No se pudo compilar el bundle std."
        $failedSteps += "vn std-bundle"
    } else {
        Write-Success "Bundle std listo."
    }
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

if ($Quick) {
    Write-WarningMsg "Saltando cuadrantes 2-4 (-Quick activo): correr la matriz completa antes de commitear."
} else {

# Q2: dev-checkout + No-JIT (Interprete Pure)
$q2 = Run-Quadrant "2/4: [dev-checkout] + [Interprete Pure (VARN_NO_JIT=1)]" @{ "VARN_NO_JIT" = "1" } @("run", "tests/main.vn")
if (-not $q2) { $failedSteps += "Cuadrante 2 (dev-checkout + No-JIT)" }

# Q3: @embedded + JIT
$q3 = Run-Quadrant "3/4: [@embedded std] + [JIT Habilitado]" @{ "VARN_STD" = "@embedded"; "VARN_STD_BUNDLE" = $stdBundle } @("run", "tests/main.vn")
if (-not $q3) { $failedSteps += "Cuadrante 3 (@embedded + JIT)" }

# Q4: @embedded + No-JIT (Interprete Pure)
$q4 = Run-Quadrant "4/4: [@embedded std] + [Interprete Pure (VARN_NO_JIT=1)]" @{ "VARN_STD" = "@embedded"; "VARN_STD_BUNDLE" = $stdBundle; "VARN_NO_JIT" = "1" } @("run", "tests/main.vn")
if (-not $q4) { $failedSteps += "Cuadrante 4 (@embedded + No-JIT)" }
}

# 6. Benchmark de Estabilidad
if (-not $Fast) {
    Write-StepHeader "6/6: Benchmark de Estabilidad (vn bench tests/benchmarks/vn/bench_fib.vn -v)"
    $proc = Start-Process -FilePath $vnBin -ArgumentList @("bench", "tests/benchmarks/vn/bench_fib.vn", "-v") -NoNewWindow -PassThru -Wait
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
