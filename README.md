# Varn Programming Language

<div align="center">

[![Latest Release](https://img.shields.io/github/v/release/carlos-burelo/varn?style=for-the-badge&logo=github&color=blue)](https://github.com/carlos-burelo/varn/releases/latest)
[![CI/CD Pipeline](https://img.shields.io/github/actions/workflow/status/carlos-burelo/varn/ci.yml?branch=main&style=for-the-badge&logo=githubactions&logoColor=white)](https://github.com/carlos-burelo/varn/actions/workflows/ci.yml)
[![Platforms](https://img.shields.io/badge/Platforms-Linux%20%7C%20macOS%20%7C%20Windows-brightgreen?style=for-the-badge&logo=linux&logoColor=white)](https://github.com/carlos-burelo/varn/releases/latest)
[![Built with Rust](https://img.shields.io/badge/Built_with-Rust_1.75+-orange?style=for-the-badge&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/License-Apache--2.0-blue?style=for-the-badge)](LICENSE)

<p align="center">
  <b>Lenguaje de programación de alto rendimiento, estáticamente tipado, con arquitectura VM basada en registros, motor JIT Cranelift nativo, recolector de basura generacional y runtime asíncrono basado en Isolates.</b>
</p>

[Descargas](#-descargas-y-distribución-binaria) •
[Inicio Rápido](#-inicio-rápido) •
[Tour del Lenguaje](#-tour-del-lenguaje) •
[Arquitectura](#-arquitectura-del-sistema) •
[Benchmarks](#-rendimiento-comparativo) •
[Documentación](#-documentación-técnica)

</div>

---

## ⚡ Aspectos Destacados

* **Tipado Estático Estricto y Bidireccional**: Inferencia de tipos sin sobrecarga en runtime, análisis de flujo de control (CFA), exhaustividad garantizada y cero coerción implícita de tipos.
* **VM de Registros con `VmValue` de 128 bits**: Representación canónica de dos palabras (`tag` + `payload`) con enteros `int` nativos de 64 bits, flotantes `float` IEEE 754, `bool`, `char` y *Small String Optimization* (SSO de hasta 5 bytes inline sin tocar el heap).
* **Compilación Nativa JIT Multi-ISA**: Generación de código máquina nativo para hot paths mediante backend Cranelift con fast-paths optimizados, hoisting de comprobaciones de límites y fallback transparente al intérprete.
* **Pipeline TIR & SSA Optimizado**: Lowering canónico a *Typed Intermediate Representation* (TIR), construcción SSA, eliminación de bloques inalcanzables (DCE), movimiento de código invariante de bucles (LICM) y reemplazo escalar de agregados (SROA).
* **GC Generacional**: Nursery de bump-allocation ultrarrápido junto con un Old-Generation mark-and-sweep tricolor con write barriers de bajo coste.
* **Concurrencia Determinista y Paralelismo Real**: Tareas cooperativas síncronas en micro-trampolín, `TaskGroup` con gestión de recursos por ámbito (`using`), e **Isolates** independientes en hilos OS comunicados por canales tipados de paso de mensajes sin memoria compartida.
* **Tooling de Primera Clase**: CLI unificado (`vn run`, `vn check`, `vn build`, `vn bench`, `vn debug -p`, `vn repl`, `vn pkg`, `vn lsp`).

---

## 📦 Descargas y Distribución Binaria

Los binarios precompilados de producción están disponibles para las principales arquitecturas y sistemas operativos en cada lanzamiento oficial:

| Plataforma / Arquitectura | Target Triple | Paquete Oficial (v0.1.0) |
|---|---|---|
| **Linux x86_64** (glibc) | `x86_64-unknown-linux-gnu` | [`vn-v0.1.0-x86_64-unknown-linux-gnu.tar.gz`](https://github.com/carlos-burelo/varn/releases/download/v0.1.0/vn-v0.1.0-x86_64-unknown-linux-gnu.tar.gz) |
| **Linux ARM64** (glibc) | `aarch64-unknown-linux-gnu` | [`vn-v0.1.0-aarch64-unknown-linux-gnu.tar.gz`](https://github.com/carlos-burelo/varn/releases/download/v0.1.0/vn-v0.1.0-aarch64-unknown-linux-gnu.tar.gz) |
| **macOS Apple Silicon** | `aarch64-apple-darwin` | [`vn-v0.1.0-aarch64-apple-darwin.tar.gz`](https://github.com/carlos-burelo/varn/releases/download/v0.1.0/vn-v0.1.0-aarch64-apple-darwin.tar.gz) |
| **macOS Intel** | `x86_64-apple-darwin` | [`vn-v0.1.0-x86_64-apple-darwin.tar.gz`](https://github.com/carlos-burelo/varn/releases/download/v0.1.0/vn-v0.1.0-x86_64-apple-darwin.tar.gz) |
| **Windows x86_64** | `x86_64-pc-windows-msvc` | [`vn-v0.1.0-x86_64-pc-windows-msvc.zip`](https://github.com/carlos-burelo/varn/releases/download/v0.1.0/vn-v0.1.0-x86_64-pc-windows-msvc.zip) |

Para verificar la integridad criptográfica de las descargas:
- **Checksums**: [`SHA256SUMS.txt`](https://github.com/carlos-burelo/varn/releases/download/v0.1.0/SHA256SUMS.txt)

---

## 🚀 Inicio Rápido

### Instalación de Binarios Precompilados

#### Linux & macOS
```bash
# Descargar y extraer (ejemplo para Linux x86_64)
curl -LO https://github.com/carlos-burelo/varn/releases/download/v0.1.0/vn-v0.1.0-x86_64-unknown-linux-gnu.tar.gz
tar -xzf vn-v0.1.0-x86_64-unknown-linux-gnu.tar.gz

# Mover a tu PATH local
sudo mv vn /usr/local/bin/
vn --version
```

#### Windows (PowerShell)
```powershell
Invoke-WebRequest -Uri "https://github.com/carlos-burelo/varn/releases/download/v0.1.0/vn-v0.1.0-x86_64-pc-windows-msvc.zip" -OutFile "vn.zip"
Expand-Archive -Path "vn.zip" -DestinationPath "$HOME\bin"
$env:Path += ";$HOME\bin"
vn --version
```

### Compilación desde el Código Fuente

Requiere **Rust 1.75+** con `cargo`:

```bash
git clone https://github.com/carlos-burelo/varn.git
cd varn-lang
cargo build --release --bin vn
./target/release/vn --version
```

### Primer Programa (`hola.vn`)

```Varn
// hola.vn
function saludar(nombre: str): str {
    return `¡Bienvenido a Varn, ${nombre}!`
}

print(saludar("Mundo"))
```

Ejecuta directamente:
```bash
vn run hola.vn
```

Compila a bytecode optimizado (`.vnc`):
```bash
vn build hola.vn -o hola.vnc
vn run hola.vnc
```

Inspecciona el pipeline completo (AST → TIR → Bytecode → VM):
```bash
vn debug -p hola.vn
```

---

## 🏛️ Arquitectura del Sistema

```mermaid
flowchart TD
    A["Código Fuente (.vn)"] --> B["varn-lexer\n(Tokenizador UTF-8 + ASI)"]
    B --> C["varn-parser\n(Parser Pratt / Descendente Recursivo)"]
    C --> D["varn-checker\n(Tipado Estático, CFA, SemanticDB)"]
    D --> E["varn-tir\n(Typed Intermediate Representation)"]
    E --> F["varn-compiler\n(from_tir → SSA IR → Opts: DCE, LICM, SROA)"]
    F --> G["varn-regalloc\n(Liveness Analysis & Linear Scan)"]
    G --> H["varn-vm\n(Register VM + VmValue 128-bit + GC Generacional)"]
    G -.-> I["varn-jit\n(Cranelift Native Machine Code: x86-64, ARM64)"]
    I -.-> H
    H --> J["varn-runtime\n(Isolates en hilos OS + Canales Tipados)"]
    H <--> K["varn-builtins\n(Stdlib Nativa Rust via LBI)"]
```

---

## 💻 Tour del Lenguaje

### Variables y Tipos Primitivos Canónicos

Varn implementa nombres de tipos canónicos únicos: `int`, `float`, `str`, `bool`, `char`.

```Varn
const puerto: int = 8080
let host: str = "127.0.0.1"
const activo: bool = true
const factor: float = 3.14159

// Operaciones y aserciones
assert("potencia", 2 ** 10 === 1024)
assert("bit-shift", 1 << 4 === 16)

// Métodos intrínsecos de strings
const s = "  Varn Language  "
assert("trim", s.trim() === "Varn Language")
assert("slice", s.trim().slice(0, 4) === "Varn")
```

### Pattern Matching Exhaustivo

```Varn
enum Estado { Pendiente, Procesando, Completado, Error }

function describir(estado: Estado): str {
    return match (estado) {
        Estado.Pendiente   => "En cola de espera",
        Estado.Procesando  => "Procesando tarea",
        Estado.Completado  => "Finalizado con éxito",
        Estado.Error       => "Fallo en la ejecución"
    }
}
```

### Funciones, Closures y Argumentos Nombrados

```Varn
function crearSumador(base: int): (n: int) => int {
    return (x: int) => base + x
}

const sumar10 = crearSumador(10)
assert("closure", sumar10(5) === 15)

// Argumentos nombrados fuera de orden
function configurarServidor(host: str, puerto: int, ssl: bool = false): str {
    const proto = ssl ? "https" : "http"
    return `${proto}://${host}:${puerto}`
}

assert("named args", configurarServidor(puerto: 443, host: "varn.dev", ssl: true) === "https://varn.dev:443")
```

### Clases, Interfaces y Polimorfismo

```Varn
interface Dibujable {
    area(): float
}

abstract class Figura implements Dibujable {
    abstract area(): float
    etiqueta(): str {
        return `Área calculada: ${this.area()}`
    }
}

class Circulo extends Figura {
    radio: float
    constructor(radio: float) {
        this.radio = radio
    }
    override area(): float {
        return 3.1415926535 * this.radio * this.radio
    }
}
```

### Extensiones y Operador Pipeline

```Varn
extension EnteroUtil on int {
    esPar(): bool { return this % 2 === 0 }
    alCuadrado(): int { return this * this }
}

assert("extension", (4).esPar() === true)

// Pipeline chaining con placeholder (_)
function duplicar(n: int): int { return n * 2 }
function restar(a: int, b: int): int { return a - b }

const resultado = 5 |> duplicar(_) |> restar(_, 3)
assert("pipeline", resultado === 7)
```

### Concurrencia con `TaskGroup` e `Isolates`

```Varn
import { sleep, TaskGroup, spawnIsolate } from "std:task"

// Tareas cooperativas asíncronas
async function calcular(): void {
    using grupo = TaskGroup<int>()
    grupo.spawn(async () => 20)
    grupo.spawn(async () => 22)
    const partes = await grupo.join()
    assert("concurrencia", partes[0] + partes[1] === 42)
}

// Paralelismo multinúcleo real (Isolate con heap aislado)
function tareaPesada(): void {
    const worker = spawnIsolate("./worker.vn")
    worker.send("procesar_lote")
}
```

---

## 📊 Rendimiento Comparativo

Resultados de la suite comparativa oficial (`cargo xtask compare`) ejecutada en perfil `release` con mediciones de tiempo de proceso completo (tiempo de pared de arranque, compilación JIT y ejecución). Evaluado en host Intel Core i7-1355U / Windows 11:

### 🚀 Latencia de Arranque (Cold Start)
- **Varn**: **10.5 ms** ⚡ (**4.4x más rápido que Bun**, **5.4x más rápido que Node.js**, **1.7x más rápido que Python**)
- **Python**: 18.0 ms
- **Bun**: 46.1 ms
- **Node.js**: 56.3 ms

### 📈 Matriz de Cargas de Trabajo

| Carga de Trabajo | Varn | Bun | Node.js | Python | Ventaja de Varn |
|---|---|---|---|---|---|
| `fib` (Recursión hot) | **41.2 ms** | 43.5 ms | 61.2 ms | 312.0 ms | 🏆 **1.06x vs Bun** (1.48x vs Node) |
| `gc_alloc` (GC Trashing) | **45.0 ms** | 48.2 ms | 65.4 ms | 280.1 ms | 🏆 **1.07x vs Bun** (1.45x vs Node) |
| `matrix` (Álgebra matricial) | **22.3 ms** | 24.8 ms | 35.6 ms | 386.2 ms | 🏆 **1.11x vs Bun** (1.60x vs Node) |
| `csv_etl` (Extracción y parseo) | **30.5 ms** | 38.1 ms | 50.4 ms | 178.5 ms | 🏆 **1.25x vs Bun** (1.65x vs Node) |
| `dto` (Instanciación tipada) | **23.5 ms** | 30.1 ms | 37.8 ms | 195.4 ms | 🏆 **1.28x vs Bun** (1.60x vs Node) |
| `csv_pipeline` (Transformación) | **105.2 ms** | 146.2 ms | 153.9 ms | 612.0 ms | 🏆 **1.39x vs Bun** (1.46x vs Node) |
| `json_native` (Parseo/Stringify) | **38.1 ms** | 41.3 ms | 57.0 ms | 145.0 ms | 🤝 **~empate con Bun** (1.50x vs Node) |
| `json_api_payloads` | **57.4 ms** | 49.3 ms | 65.9 ms | 210.0 ms | ⚡ Más rápido que Node.js |
| `collection_pipeline` | **63.2 ms** | 45.6 ms | 70.6 ms | 220.0 ms | ⚡ Más rápido que Node.js |

> [!NOTE]
> **Integridad Verificada (Zero Mismatches)**: Todos los benchmarks validan formalmente la equivalencia semántica de las salidas numéricas y estructuras de datos generadas frente a los demás motores.

---

## 🛠️ Ecosistema de Crates

El compilador y runtime de Varn están diseñados de forma modular siguiendo dominios estrictos:

| Crate | Descripción |
|---|---|
| [`varn-core`](docs/ARCHITECTURE.md) | Definiciones canónicas de AST, OpCodes, Spans, diagnósticos y constantes centrales. |
| [`varn-types`](docs/ARCHITECTURE.md) | Representación de `VmValue` (128-bit), `Chunk`, `FunctionProto`, `Shape` y memoria. |
| [`varn-lexer`](docs/ARCHITECTURE.md) | Tokenizador UTF-8 determinista con soporte de ASI (*Automatic Semicolon Insertion*). |
| [`varn-parser`](docs/ARCHITECTURE.md) | Analizador sintáctico Pratt y descendente recursivo. |
| [`varn-checker`](docs/ARCHITECTURE.md) | Verificador estático, inferencia bidireccional, CFA y generación de TIR. |
| [`varn-tir`](docs/ARCHITECTURE.md) | Representación Intermedia Tipada (*Typed Intermediate Representation*). |
| [`varn-compiler`](docs/COMPILER_ARCHITECTURE.md) | Generador SSA, inlining de hojas, pasadas de optimización y emisión de bytecode. |
| [`varn-regalloc`](docs/COMPILER_ARCHITECTURE.md) | Asignación lineal de registros y liveness analysis. |
| [`varn-vm`](docs/VM_ARCHITECTURE.md) | Máquina virtual basada en registros, recolector de basura generacional e inline caches. |
| [`varn-jit`](docs/VM_ARCHITECTURE.md) | Motor JIT nativo multi-arquitectura basado en Cranelift. |
| [`varn-runtime`](docs/RUNTIME_ARCHITECTURE.md) | Gestión de hilos de SO, runtime asíncrono y canales entre Isolates. |
| [`varn-builtins`](docs/LBI_ARCHITECTURE.md) | Biblioteca estándar nativa Rust vinculada mediante Linker-Bound Interface (LBI). |
| [`varn-modules`](docs/STDLIB_ARCHITECTURE.md) | Cargador de módulos, resolución de dependencias y bundles `.vnb`. |
| [`varn-pipeline`](docs/ARCHITECTURE.md) | Orquestador de fases de compilación y almacenamiento en caché. |
| [`varn-cli`](docs/CLI_REFERENCE.md) | Interfaz de línea de comandos unificada `vn`. |
| [`varn-lsp`](docs/ARCHITECTURE.md) | Implementación del Language Server Protocol para soporte en IDEs y editores. |
| [`varn-debug`](docs/CLI_INSPECT.md) | Pipeline inspector para diagnóstico integral (`vn debug -p`). |

---

## 📚 Documentación Técnica

* 🏛️ [**Arquitectura General del Sistema**](docs/ARCHITECTURE.md)
* ⚙️ [**Compilador, TIR y Optimizaciones SSA**](docs/COMPILER_ARCHITECTURE.md)
* 🧠 [**Arquitectura de la VM, VmValue y GC**](docs/VM_ARCHITECTURE.md)
* ⚡ [**Runtime Asíncrono e Isolates**](docs/RUNTIME_ARCHITECTURE.md)
* 📚 [**Biblioteca Estándar y Bundles (.vnb)**](docs/STDLIB_ARCHITECTURE.md)
* 🔗 [**Linker-Bound Interface (LBI)**](docs/LBI_ARCHITECTURE.md)
* 💻 [**Manual de Referencia CLI**](docs/CLI_REFERENCE.md)
* 🔍 [**Inspección del Pipeline con `vn debug`**](docs/CLI_INSPECT.md)
* 📦 [**Guía de Instalación Detallada**](docs/INSTALL.md)
* 🤝 [**Guía para Contribuidores**](CONTRIBUTING.md)

---

## 📄 Licencia

Distribuido bajo la Licencia **Apache 2.0**. Consulta el archivo [LICENSE](LICENSE) para más información.
