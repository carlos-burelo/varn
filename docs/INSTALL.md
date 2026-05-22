# Instalación de Varn

## Requisitos

- Rust stable (https://rustup.rs)
- Cargo
- Windows, Linux o macOS

## Desde el código fuente

```sh
git clone https://github.com/tu-usuario/Varn
cd Varn
cargo build --bin vn --release
```

El binario queda en `target/release/wr`.

### Agregar al PATH

**Linux/macOS:**
```sh
cp target/release/vn ~/.local/bin/wr
# O agregar target/release/ al PATH en ~/.bashrc / ~/.zshrc
```

**Windows (PowerShell):**
```powershell
Copy-Item target\release\wr.exe "$env:USERPROFILE\.cargo\bin\wr.exe"
```

## Verificar instalación

```sh
vn doctor
vn tests/main.vn
# PASSED: 529 / FAILED: 0
```

## Modo desarrollo

```sh
# Build dev (más rápido de compilar)
cargo build --bin wr

# Ejecutar directamente sin instalar
cargo run --bin vn -- tests/main.vn
```

## Variables de entorno opcionales

```sh
RUST_LOG=debug vn run program.vn    # Logging detallado
RUST_BACKTRACE=1 vn run program.vn  # Backtrace en errores
```
