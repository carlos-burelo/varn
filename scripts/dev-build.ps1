<#
.SYNOPSIS
    Reconstruye el binario `vn` sin fallar por el archivo bloqueado en Windows.

.DESCRIPTION
    En Windows, si el LSP está corriendo (spawneado por la extensión de VS Code
    desde `target/debug/vn.exe` o `target/release/vn.exe`), `cargo build` falla
    con "Acceso denegado" al intentar reemplazar el .exe: el linker no puede
    borrar un archivo con un handle abierto.

    Este script evita la carrera por completo con el truco de Windows de
    renombrar el ejecutable bloqueado antes de compilar (el lock es sobre los
    DATOS del archivo, no sobre su nombre — renombrar siempre funciona aunque
    el proceso siga corriendo). El proceso viejo sigue vivo con la imagen ya
    cargada en memoria; sólo hace falta que VS Code lo reinicie
    (`Varn: Restart Language Server`) para que tome el binario nuevo.

    No mata procesos por su cuenta: eso interrumpe una sesión de VS Code activa
    sin avisar. Si el rename falla igual (el `mv` en sí bloqueado, no sólo el
    contenido — raro pero posible), el script lo informa y sugiere cerrar
    VS Code, en vez de reintentar a ciegas.

.PARAMETER Release
    Compila en modo release (`--release`) en vez de debug.

.PARAMETER Both
    Compila ambos perfiles (debug y release).

.EXAMPLE
    .\scripts\dev-build.ps1
    .\scripts\dev-build.ps1 -Release
    .\scripts\dev-build.ps1 -Both
#>

param (
    [switch]$Release,
    [switch]$Both
)

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $repoRoot

function Invoke-Profile {
    param([string]$Profile)

    $dir = if ($Profile -eq "release") { "release" } else { "debug" }
    $exePath = Join-Path $repoRoot "target\$dir\vn.exe"

    if (Test-Path $exePath) {
        $stalePath = "$exePath.stale"
        if (Test-Path $stalePath) {
            Remove-Item -Force $stalePath -ErrorAction SilentlyContinue
        }
        try {
            Rename-Item -Path $exePath -NewName "vn.exe.stale" -ErrorAction Stop
            Write-Host "[$dir] binario previo renombrado (lock evitado)." -ForegroundColor DarkGray
        } catch {
            Write-Host "[$dir] no se pudo renombrar $exePath — sigue igual de bloqueado que un 'cargo build' normal." -ForegroundColor Yellow
            Write-Host "         Cierra VS Code (o mata vn.exe manualmente) y reintenta." -ForegroundColor Yellow
            throw
        }
    }

    $cargoArgs = @("build", "--bin", "vn")
    if ($Profile -eq "release") { $cargoArgs += "--release" }

    Write-Host "Compilando vn ($dir)..." -ForegroundColor Cyan
    & cargo @cargoArgs
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build falló para el perfil $dir (exit $LASTEXITCODE)"
    }

    $stalePath = "$exePath.stale"
    if (Test-Path $stalePath) {
        Remove-Item -Force $stalePath -ErrorAction SilentlyContinue
    }

    Write-Host "[$dir] listo: $exePath" -ForegroundColor Green
}

if ($Both) {
    Invoke-Profile -Profile "debug"
    Invoke-Profile -Profile "release"
} elseif ($Release) {
    Invoke-Profile -Profile "release"
} else {
    Invoke-Profile -Profile "debug"
}

Write-Host ""
Write-Host "Si VS Code tenía el LSP abierto: corre 'Varn: Restart Language Server' para que tome el binario nuevo." -ForegroundColor Cyan
