# Instalación Varn

Código manda. Fuentes: `Cargo.toml`, `rust-toolchain.toml`, `crates/varn-cli/src/cli.rs`, `.github/workflows/ci.yml`.

## Requisitos

- Toolchain pineada: `channel = "1.97.1"` (`rust-toolchain.toml`). Usar rustup, pin se aplica solo.
- Edición 2024 (`Cargo.toml`).
- Linker: `cc`/`gcc`/`clang` en Unix; MSVC o `rust-lld` en Windows.

```bash
git clone https://github.com/carlos-burelo/varn
cd varn
```

## Perfiles (`Cargo.toml`)

| Perfil | Flags | Uso |
|---|---|---|
| `quick` | hereda release, `opt-level=3`, `lto=false`, `codegen-units=16` (`Cargo.toml`) | Iteración diaria (Ley 13 AGENTS.md) |
| `release` | `opt-level=3`, `lto="thin"`, `codegen-units=1`, `panic="abort"` (`Cargo.toml`) | Puerta validación, CI test-matrix (`ci.yml`) |
| `dev-it` | hereda quick, `opt-level=0` (`Cargo.toml`) | Edición LSP/DAP, nunca matriz ni bench |
| `dist` | hereda release, `lto="fat"`, `strip="symbols"` (`Cargo.toml`) | Solo CI release (`ci.yml`) |

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

Binario: `target/<perfil>/vn` (+`.exe` Windows). CI cubre 5 targets (`ci.yml`): `x86_64-pc-windows-msvc`, `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-apple-darwin`, `aarch64-apple-darwin`.

## Binarios oficiales

Release CI (`ci.yml`): push a main → build dist por target → smoke (`--version` + `run ./tests/main.vn`) → asset `vn-v$VERSION-$target.{tar.gz,zip}` + `SHA256SUMS.txt` → tag `v$VERSION` (versión de `Cargo.toml`, hoy `0.1.0`).

`dist/` está en `.gitignore`: `dist/std.vnb` es la imagen de la std que construye `vn std-bundle` (CI la usa en los cuadrantes `@embedded`), no un asset de release.

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

Matriz puerta = 4 cuadrantes (`ci.yml`): std `dev-checkout`/`@embedded` × JIT/`VARN_NO_JIT=1`. Falla 1 = falla todo. En local, `scripts/verify.ps1 -Fast` añade el cuadrante 5 (`VARN_JIT_CACHE=verify`).

## Env runtime

| Variable | Efecto (código) |
|---|---|
| `VARN_STD=@embedded\|<path>` | Origen stdlib. Orden: clave `std` del manifiesto del proyecto → `VARN_STD` → `std/` en un ancestro del binario → imagen embebida |
| `VARN_NO_JIT=1` | Fuerza intérprete |
| `VARN_GC_TRACE=1` | Línea por colección menor |
| `RUST_BACKTRACE=1\|full` | Stack trace pánico Rust |

Lista completa de variables: [`CLI_REFERENCE.md`](CLI_REFERENCE.md#variables-de-entorno).
