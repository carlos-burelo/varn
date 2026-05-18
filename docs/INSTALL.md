# Instalación de Varn

## Requisitos

- Rust stable (https://rustup.rs)
- Cargo
- Windows, Linux o macOS

## Desde el código fuente

```sh
git clone https://github.com/tu-usuario/Varn
cd Varn
cargo build --bin wr --release
```

El binario queda en `target/release/wr`.

### Agregar al PATH

**Linux/macOS:**
```sh
cp target/release/wr ~/.local/bin/wr
# O agregar target/release/ al PATH en ~/.bashrc / ~/.zshrc
```

**Windows (PowerShell):**
```powershell
Copy-Item target\release\wr.exe "$env:USERPROFILE\.cargo\bin\wr.exe"
```

## Verificar instalación

```sh
wr doctor
wr tests/main.wr
# PASSED: 534 / FAILED: 0
```

## Modo desarrollo

```sh
# Build dev (más rápido de compilar)
cargo build --bin wr

# Ejecutar directamente sin instalar
cargo run --bin wr -- tests/main.wr
```

## Variables de entorno opcionales

```sh
RUST_LOG=debug wr run program.wr    # Logging detallado
RUST_BACKTRACE=1 wr run program.wr  # Backtrace en errores
```
