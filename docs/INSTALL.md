# Instalación Varn

Código manda. Fuentes: `Cargo.toml:112-166`, `rust-toolchain.toml:9`, `crates/varn-cli/src/cli.rs`, `.github/workflows/ci.yml:90-110,194-196`.

## Requisitos

- Toolchain pineada: `channel = "1.97.1"` (`rust-toolchain.toml:9`). Usar rustup, pin se aplica solo.
- Edición 2024 (`Cargo.toml:61`).
- Linker: `cc`/`gcc`/`clang` en Unix; MSVC o `rust-lld` en Windows.

```bash
git clone <repo-url>
cd varn-lang
```

## Perfiles (`Cargo.toml:112-166`)

| Perfil | Flags | Uso |
|---|---|---|
| `quick` | hereda release, `opt-level=3`, `lto=false`, `codegen-units=16` (`Cargo.toml:136-141`) | Iteración diaria (Ley 13 AGENTS.md) |
| `release` | `opt-level=3`, `lto="thin"`, `codegen-units=1`, `panic="abort"` (`Cargo.toml:112-116`) | Puerta validación, CI test-matrix (`ci.yml:91`) |
| `dev-it` | hereda quick, `opt-level=0` (`Cargo.toml:150-152`) | Edición LSP/DAP, nunca matriz ni bench |
| `dist` | hereda release, `lto="fat"`, `strip="symbols"` (`Cargo.toml:157-163`) | Solo CI release (`ci.yml:196`) |

## Build

Iteración:

```bash
cargo build --profile quick --bin vn
```

Puerta / CI:

```bash
cargo build --release --bin vn
```

Dist (solo CI, no local salvo empaquetar):

```bash
cargo build --profile dist --bin vn --target <triple>
```

Binario: `target/<perfil>/vn` (+`.exe` Windows). CI cubre 5 targets (`ci.yml:135-156`): `x86_64-pc-windows-msvc`, `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-apple-darwin`, `aarch64-apple-darwin`.

## Binarios oficiales

Release CI (`ci.yml:126-295`): push a main → build dist por target → smoke (`--version` + `run ./tests/main.vn`) → asset `vn-v$VERSION-$target.{tar.gz,zip}` + `SHA256SUMS.txt` → tag `v$VERSION` (versión de `Cargo.toml:60`, hoy `0.1.0`).

Estado local verificado: tag `v0.1.0` existe en git local; `dist/std.vnb` existe (bundle stdlib build, no asset release). URLs `github.com/.../releases/download/...` no verificadas localmente — componer desde tag publicado, no copiar de memoria.

## PATH

```powershell
Copy-Item target\quick\vn.exe "$env:USERPROFILE\.cargo\bin\vn.exe" -Force
```

```bash
sudo cp target/quick/vn /usr/local/bin/vn
```

## Verificación

```bash
vn doctor
vn run tests/main.vn
```

Matriz puerta = 4 cuadrantes (`ci.yml:93-110`): std `dev-checkout`/`@embedded` × JIT/`VARN_NO_JIT=1`. Falla 1 = falla todo.

## Env runtime

| Variable | Efecto (código) |
|---|---|
| `VARN_STD=@embedded\|dev-checkout\|<path>` | Origen stdlib |
| `VARN_NO_JIT=1` | Fuerza intérprete |
| `VARN_GC_TRACE=1` | Línea por colección menor (`varn-debug/src/flags/phases.rs:67`) |
| `RUST_BACKTRACE=1\|full` | Stack trace pánico Rust |
