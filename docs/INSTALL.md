# Guía de Instalación y Configuración de Varn

Este documento proporciona las instrucciones completas para compilar, instalar y configurar el entorno de ejecución de **Varn** en Windows, Linux y macOS.

---

## Tabla de Contenidos

- [Requisitos del Sistema](#requisitos-del-sistema)
- [Compilación desde el Código Fuente](#compilación-desde-el-código-fuente)
  - [Build de Producción (`release`)](#build-de-producción-release)
  - [Build de Desarrollo (`dev`)](#build-de-desarrollo-dev)
- [Configuración de Variables de Entorno (`PATH`)](#configuración-de-variables-de-entorno-path)
  - [Windows (PowerShell)](#windows-powershell)
  - [Linux / macOS (Bash / Zsh)](#linux--macos-bash--zsh)
- [Verificación de la Instalación](#verificación-de-la-instalación)
- [Variables de Entorno Avanzadas](#variables-de-entorno-avanzadas)

---

## Requisitos del Sistema

- **Sistema Operativo**: Windows 10/11 (x86_64), Linux (x86_64), macOS (x86_64 / Apple Silicon).
- **Toolchain de Rust**: Rust Stable (1.75 o superior) instalado a través de [`rustup`](https://rustup.rs).
- **Linker**: `cc` / `gcc` / `clang` en Unix; `MSVC` o `rust-lld.exe` en Windows.

---

## Compilación desde el Código Fuente

Clona el repositorio oficial de Varn:

```bash
git clone https://github.com/carlos-burelo/varn.git
cd varn-lang
```

### Build de Producción (`release`)

Para compilar el binario `vn` optimizado con ThinLTO:

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

Para ejecutar `vn` globalmente desde cualquier directorio, añade el binario compilado a tu `PATH`.

### Windows (PowerShell)

Copia el ejecutable a la carpeta de binarios de Cargo (que usualmente ya está en el PATH):

```powershell
Copy-Item target\release\vn.exe "$env:USERPROFILE\.cargo\bin\vn.exe" -Force
```

### Linux / macOS (Bash / Zsh)

Copia el ejecutable a tu directorio de binarios local:

```bash
cp target/release/vn ~/.local/bin/vn
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
[OK] Varn CLI Binary Version: 0.8.0
[OK] System OS: windows (x86_64)
[OK] StdLib Provider: @embedded
[OK] Environment status: Healthy
```

Verifica la estabilidad ejecutando la suite de integración:

```bash
vn run tests/main.vn
```

---

## Variables de Entorno Avanzadas

Varn admite varias variables de entorno para controlar el comportamiento del runtime y la VM:

| Variable | Valores Posibles | Descripción |
|---|---|---|
| `VARN_STD` | `@embedded`, `dev-checkout`, `/path/to/vnb` | Define la procedencia de la biblioteca estándar. Por defecto usa `dev-checkout` si existe el árbol `std/`, o `@embedded` si se ejecuta el binario empaquetado. |
| `VARN_NO_JIT` | `1`, `0` | Desactiva el compilador JIT x86-64 y fuerza a la VM a interpretar todo el bytecode. |
| `RUST_LOG` | `info`, `debug`, `trace` | Controla el nivel de logs detallados del pipeline de compilación. |
| `RUST_BACKTRACE` | `1`, `full` | Muestra el stack trace completo de Rust en caso de pánico interno. |

---

> [!TIP]
> Para probar la compilación empaquetada de la stdlib (utilizada en distribuibles release):
> ```bash
> VARN_STD=@embedded ./target/release/vn.exe run ./tests/main.vn
> ```
