<#
.SYNOPSIS
    Reproduce en un Linux limpio (Docker) los pasos del CI que dependen de la plataforma.

.DESCRIPTION
    Ejecuta con el mismo Rust que fija rust-toolchain.toml:
      1. cargo clippy --workspace --all-targets   (lo que corre "Code Quality & Linting")
      2. cargo test --profile quick -p varn-cli --test error_corpus
    El CI corre en ubuntu-latest y Windows: lo que solo se rompe en Linux (cfg, rutas,
    imports sin usar, lints) no aparece en verify.ps1. Correr este script antes de hacer
    push cuando el cambio toque codigo dependiente de plataforma.

    Requiere Docker Desktop en ejecucion. Usa volumenes con nombre para cachear
    target/ y el registro de cargo entre ejecuciones.
#>
$ErrorActionPreference = "Stop"
$repo = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path

docker info *> $null
if ($LASTEXITCODE -ne 0) {
    Write-Host "Docker no esta en ejecucion: arranca Docker Desktop y vuelve a intentarlo." -ForegroundColor Red
    exit 2
}

$toolchain = (Select-String -Path (Join-Path $repo "rust-toolchain.toml") -Pattern 'channel\s*=\s*"([^"]+)"').Matches[0].Groups[1].Value
$script = "cargo clippy --workspace --all-targets && cargo test --profile quick -p varn-cli --test error_corpus"

docker run --rm `
    -v "${repo}:/work" `
    -v varn-ci-target:/target `
    -v varn-ci-cargo:/usr/local/cargo/registry `
    -e CARGO_TARGET_DIR=/target `
    -w /work "rust:$toolchain" bash -c $script
exit $LASTEXITCODE
