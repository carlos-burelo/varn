# Guía de Instalación y Configuración de Varn

Este documento proporciona las instrucciones completas para instalar binarios oficiales precompilados o compilar y configurar **Varn** en Windows, Linux y macOS.

---

## Tabla de Contenidos

- [Instalación Rápida con Binarios Oficiales](#instalación-rápida-con-binarios-oficiales)
- [Compilación desde el Código Fuente](#compilación-desde-el-código-fuente)
  - [Requisitos del Sistema](#requisitos-del-sistema)
  - [Build de Producción (`release`)](#build-de-producción-release)
  - [Build de Desarrollo (`dev`)](#build-de-desarrollo-dev)
- [Configuración de Variables de Entorno (`PATH`)](#configuración-de-variables-de-entorno-path)
  - [Windows (PowerShell)](#windows-powershell)
  - [Linux / macOS (Bash / Zsh)](#linux--macos-bash--zsh)
- [Verificación de la Instalación](#verificación-de-la-instalación)
- [Variables de Entorno del Runtime](#variables-de-entorno-del-runtime)

---

## Instalación Rápida con Binarios Oficiales

Descarga el paquete correspondiente a tu arquitectura y sistema operativo desde la [sección de Releases](https://github.com/carlos-burelo/varn/releases/latest):

| Plataforma / Arquitectura | Target Triple | Enlace de Descarga (v0.1.0) |
|---|---|---|
| **Linux x86_64** (glibc) | `x86_64-unknown-linux-gnu` | [`vn-v0.1.0-x86_64-unknown-linux-gnu.tar.gz`](https://github.com/carlos-burelo/varn/releases/download/v0.1.0/vn-v0.1.0-x86_64-unknown-linux-gnu.tar.gz) |
| **Linux ARM64** (glibc) | `aarch64-unknown-linux-gnu` | [`vn-v0.1.0-aarch64-unknown-linux-gnu.tar.gz`](https://github.com/carlos-burelo/varn/releases/download/v0.1.0/vn-v0.1.0-aarch64-unknown-linux-gnu.tar.gz) |
| **macOS Apple Silicon** | `aarch64-apple-darwin` | [`vn-v0.1.0-aarch64-apple-darwin.tar.gz`](https://github.com/carlos-burelo/varn/releases/download/v0.1.0/vn-v0.1.0-aarch64-apple-darwin.tar.gz) |
| **macOS Intel** | `x86_64-apple-darwin` | [`vn-v0.1.0-x86_64-apple-darwin.tar.gz`](https://github.com/carlos-burelo/varn/releases/download/v0.1.0/vn-v0.1.0-x86_64-apple-darwin.tar.gz) |
| **Windows x86_64** | `x86_64-pc-windows-msvc` | [`vn-v0.1.0-x86_64-pc-windows-msvc.zip`](https://github.com/carlos-burelo/varn/releases/download/v0.1.0/vn-v0.1.0-x86_64-pc-windows-msvc.zip) |

Checksums criptográficos: [`SHA256SUMS.txt`](https://github.com/carlos-burelo/varn/releases/download/v0.1.0/SHA256SUMS.txt)

### Linux & macOS

```bash
curl -LO https://github.com/carlos-burelo/varn/releases/download/v0.1.0/vn-v0.1.0-x86_64-unknown-linux-gnu.tar.gz
tar -xzf vn-v0.1.0-x86_64-unknown-linux-gnu.tar.gz
sudo mv vn /usr/local/bin/
vn --version
```

### Windows (PowerShell)

```powershell
Invoke-WebRequest -Uri "https://github.com/carlos-burelo/varn/releases/download/v0.1.0/vn-v0.1.0-x86_64-pc-windows-msvc.zip" -OutFile "vn.zip"
Expand-Archive -Path "vn.zip" -DestinationPath "$HOME\bin"
$env:Path += ";$HOME\bin"
vn --version
```

---

## Compilación desde el Código Fuente

### Requisitos del Sistema

- **Sistema Operativo**: Windows 10/11 (x86_64), Linux (x86_64 / AArch64), macOS (Apple Silicon / x86_64).
- **Toolchain de Rust**: Rust Stable (1.75 o superior) instalado a través de [`rustup`](https://rustup.rs).
- **Linker**: `cc` / `gcc` / `clang` en Unix; `MSVC` o `rust-lld.exe` en Windows.

```bash
git clone https://github.com/carlos-burelo/varn.git
cd varn-lang
```

### Build de Producción (`release`)

Para compilar el binario `vn` optimizado con ThinLTO y Cranelift:

```bash
cargo build --release --bin vn
```

El ejecutable resultante se ubicará en `target/release/vn` (o `target/release/vn.exe` en Windows).

### Build de Desarrollo (`dev`)

Para iteraciones rápidas durante el desarrollo del compilador o la VM:

```bash
cargo build --bin vn
```

---

## Configuración de Variables de Entorno (`PATH`)

### Windows (PowerShell)

Copia el ejecutable a la carpeta de binarios de Cargo (que usualmente ya está en el PATH):

```powershell
Copy-Item target\release\vn.exe "$env:USERPROFILE\.cargo\bin\vn.exe" -Force
```

### Linux / macOS (Bash / Zsh)

Copia el ejecutable a tu directorio de binarios local:

```bash
sudo cp target/release/vn /usr/local/bin/vn
```

O añade el directorio target directamente a tu `~/.bashrc` o `~/.zshrc`:

```bash
export PATH="$HOME/varn-lang/target/release:$PATH"
```

---

## Verificación de la Instalación

Ejecuta la herramienta de diagnóstico integrada:

```bash
vn doctor
```

Salida esperada:
```
[OK] Varn CLI Binary Version: 0.1.0
[OK] System OS: linux (x86_64)
[OK] StdLib Provider: @embedded
[OK] Environment status: Healthy
```

Verifica la estabilidad ejecutando la suite de integración:

```bash
vn run tests/main.vn
```

---

## Variables de Entorno del Runtime

Varn admite variables de entorno para controlar el comportamiento del runtime y la VM:

| Variable | Valores Posibles | Descripción |
|---|---|---|
| `VARN_STD` | `@embedded`, `dev-checkout`, `/path/to/vnb` | Define la procedencia de la biblioteca estándar. Por defecto usa `dev-checkout` si existe el árbol `std/`, o `@embedded` si se ejecuta el binario empaquetado. |
| `VARN_NO_JIT` | `1`, `0` | Desactiva el compilador JIT Cranelift y fuerza a la VM a interpretar todo el bytecode. |
| `VARN_CLIF_OPT` | `none`, `speed`, `speed_and_size` | Nivel de optimización del backend Cranelift (por defecto: `speed`). |
| `RUST_LOG` | `info`, `debug`, `trace` | Controla el nivel de logs detallados del pipeline de compilación. |
| `RUST_BACKTRACE` | `1`, `full` | Muestra el stack trace completo de Rust en caso de pánico interno. |
