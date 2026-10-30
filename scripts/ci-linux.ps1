<#
.SYNOPSIS
    Reproduce en un Linux limpio (Docker) lo que el CI ejecuta en ubuntu-latest.

.DESCRIPTION
    Con el mismo Rust que fija rust-toolchain.toml:
      1. cargo clippy --workspace --all-targets          (job "Code Quality & Linting")
      2. cargo build --profile quick --bin vn
      3. cargo test --profile quick -p varn-cli --test error_corpus
      4. los 4 cuadrantes de tests/main.vn (dev-checkout/@embedded x JIT/No-JIT)
    Lo que solo se rompe en Linux (cfg, rutas, FFI SysV, sockets, lints) no aparece en
    verify.ps1. Correr este script antes de hacer push cuando el cambio toque codigo
    dependiente de plataforma.

    Requiere Docker Desktop en ejecucion. Los volumenes con nombre cachean target/ y el
    registro de cargo, asi que solo la primera ejecucion compila todo.
#>
$ErrorActionPreference = "Stop"
$repo = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path

docker info *> $null
if ($LASTEXITCODE -ne 0) {
    Write-Host "Docker no esta en ejecucion: arranca Docker Desktop y vuelve a intentarlo." -ForegroundColor Red
    exit 2
}

$toolchain = (Select-String -Path (Join-Path $repo "rust-toolchain.toml") -Pattern 'channel\s*=\s*"([^"]+)"').Matches[0].Groups[1].Value

$steps = @'
set -e
apt-get update -qq >/dev/null && apt-get install -y -qq fonts-dejavu-core fontconfig >/dev/null && fc-cache -f
cargo clippy --workspace --all-targets
cargo build --profile quick --bin vn
cargo test --profile quick -p varn-cli --test error_corpus
BIN=/target/quick/vn
$BIN std-bundle --std-dir std --out /tmp/std.vnb
echo "== Quadrant 1 (dev-checkout + JIT)";    timeout 900 $BIN run ./tests/main.vn
echo "== Quadrant 2 (dev-checkout + No-JIT)"; VARN_NO_JIT=1 timeout 900 $BIN run ./tests/main.vn
echo "== Quadrant 3 (@embedded + JIT)";       VARN_STD=@embedded VARN_STD_BUNDLE=/tmp/std.vnb timeout 900 $BIN run ./tests/main.vn
echo "== Quadrant 4 (@embedded + No-JIT)";    VARN_STD=@embedded VARN_STD_BUNDLE=/tmp/std.vnb VARN_NO_JIT=1 timeout 900 $BIN run ./tests/main.vn
echo CI_LINUX_OK
'@ -replace "`r`n", "`n"

# El script viaja como archivo: pasarlo como argumento de `docker run` en Windows
# pierde las comillas dobles y bash recibe un script truncado que "pasa" sin ejecutar nada.
$tmp = Join-Path $repo "target/tmp"
New-Item -ItemType Directory -Force $tmp | Out-Null
[System.IO.File]::WriteAllText((Join-Path $tmp "ci-linux.sh"), $steps)

docker run --rm `
    -v "${repo}:/work" `
    -v varn-ci-target:/target `
    -v varn-ci-cargo:/usr/local/cargo/registry `
    -e CARGO_TARGET_DIR=/target `
    -e VARN_STD=/work/std `
    -w /work "rust:$toolchain" bash /work/target/tmp/ci-linux.sh
exit $LASTEXITCODE
