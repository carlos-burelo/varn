# Varn Programming Language

[![License](https://img.shields.io/badge/License-Apache--2.0-blue?style=for-the-badge)](LICENSE)
[![Built with Rust](https://img.shields.io/badge/Built_with-Rust-orange?style=for-the-badge)](https://www.rust-lang.org/)
[![Status](https://img.shields.io/badge/Status-Active_Development-brightgreen?style=for-the-badge)]()

**Varn** es un lenguaje de programación compilado de alto rendimiento, estáticamente tipado, con VM basada en registros, recolector de basura generacional y runtime asíncrono nativo escrito íntegramente en Rust. Extensión de archivos fuente: `.vn`.

---

## Tabla de Contenidos

- [Características Principales](#características-principales)
- [Arquitectura de Alto Nivel](#arquitectura-de-alto-nivel)
- [Tour del Lenguaje](#tour-del-lenguaje)
  - [Variables, Tipos y Operadores](#variables-tipos-y-operadores)
  - [Control de Flujo y Pattern Matching](#control-de-flujo-y-pattern-matching)
  - [Funciones, Closures y Argumentos Nombrados](#funciones-closures-y-argumentos-nombrados)
  - [Programación Orientada a Objetos](#programación-orientada-a-objetos)
  - [Interfaces y Tipado Estructural](#interfaces-y-tipado-estructural)
  - [Genéricos y Tipos Unión](#genéricos-y-tipos-unión)
  - [Extensiones y Operador Pipeline](#extensiones-y-operador-pipeline)
  - [Async/Await, Generadores e Isolates](#asyncawait-generadores-e-isolates)
  - [Decoradores y Metadatos](#decoradores-y-metadatos)
- [Rendimiento (Varn vs Bun vs Node)](#rendimiento-varn-vs-bun-vs-node)
- [Instalación y Uso Rápido](#instalación-y-uso-rápido)
- [Estructura del Proyecto](#estructura-del-proyecto)
- [Ecosistema de Crates](#ecosistema-de-crates)
- [Documentación Técnica Detallada](#documentación-técnica-detallada)
- [Licencia](#licencia)

---

## Características Principales

- **VM Register-Based con `VmValue` de 128 bits**: Representación canónica de dos palabras (`tag` + `payload`) con soporte de enteros nativos `i64` completos, flotantes IEEE 754 `f64`, booleans, Small String Optimization (SSO de hasta 5 bytes inline de 64 bits con cero asignaciones de heap) y punteros al nursery de GC.
- **Pipeline TIR & Compilador SSA**: Pipeline multi-fase (`varn-checker` → `varn-tir` → `varn-compiler`) con inferencia bidireccional estricta, lowering canónico, inlining directo de funciones hoja, SSA con eliminación de phis triviales, DCE, LICM, SROA y plegado de constantes.
- **JIT x86-64 / Multi-ISA (Cranelift)**: Compilación nativa multi-arquitectura para funciones en el hot-path con hoisting afín de bounds checks, fast-paths de indexación para arrays y mapas, y fallback transparente al intérprete.
- **GC Generacional**: Nursery de rápida asignación bump-pointer con promoción a Old-Gen mark-and-sweep tricolor y write barrier.
- **Concurrencia e Isolates**: Tareas cooperativas deterministas en un trampolín síncrono dentro de la VM, `TaskGroup` con limpieza por ámbito (`using`), y paralelismo multinúcleo real mediante Isolates aislados con canales tipados.
- **Gestión de Paquetes y Tooling Integrado**: Comandos unificados (`vn run`, `vn check`, `vn build`, `vn bench`, `vn debug`, `vn repl`, `vn pkg`, `vn lsp`, `cargo xtask compare`).

---

## Arquitectura de Alto Nivel

```mermaid
flowchart TD
    A["Fuente (.vn)"] --> B["varn-lexer\n(Tokenizer UTF-8, ASI)"]
    B --> C["varn-parser\n(AST Parsing Pratt/RD)"]
    C --> D["varn-checker\n(Type Check, CFA, SemanticDB)"]
    D --> E["varn-tir\n(Typed Intermediate Representation)"]
    E --> F["varn-compiler\n(from_tir -> SSA -> Optimizations -> Bytecode)"]
    F --> G["varn-regalloc\n(Liveness, RegAlloc, Slot Kinds)"]
    G --> H["varn-vm\n(Register VM + VmValue 128-bit + GC Generacional + IC)"]
    G -.-> I["varn-jit\n(Cranelift Native JIT: x86-64, ARM64, RISC-V)"]
    I -.-> H
    H --> J["varn-runtime\n(canales de Isolates)"]
    H <--> K["varn-builtins\n(Stdlib nativa Rust via LBI)"]
```

---

## Tour del Lenguaje

### Variables, Tipos y Operadores

```Varn
const x: int = 42
let name: str = "Varn"
const flag: bool = true

// Operadores aritméticos y bitwise
assert("power",    2 ** 10 === 1024)
assert("mod",      17 % 5 === 2)
assert("bitwise",  (12 & 10) === 8)
assert("shift",    1 << 4 === 16)

// Métodos nativos de cadenas
const s = "  Hello, World!  "
assert("trim",        s.trim() === "Hello, World!")
assert("slice",       "hello world".slice(6, 11) === "world")
assert("replaceAll",  "foo bar foo".replaceAll("foo", "baz") === "baz bar baz")
assert("split",       "a,b,c".split(",")[1] === "b")
assert("padStart",    "5".padStart(3, "0") === "005")
```

### Control de Flujo y Pattern Matching

```Varn
// Bucles for-of, while, break, continue
for (const n of [10, 20, 30]) {
    print(n)
}

let i = 0
while (i < 5) {
    if (i % 2 === 0) { i = i + 1; continue }
    print(i)
    i = i + 1
}

// Pattern matching exhaustivo
enum Direction { North, South, East, West }

function describeDir(d: Direction): str {
    return match (d) {
        Direction.North => "going north",
        Direction.South => "going south",
        Direction.East  => "going east",
        Direction.West  => "going west"
    }
}
```

### Funciones, Closures y Argumentos Nombrados

```Varn
function makeAdder(n: int): (a: int) => int {
    return (x: int) => x + n
}
const add5 = makeAdder(5)
assert("closure", add5(3) === 8)

// Argumentos nombrados fuera de orden
function describe(name: str, age: int, city: str | null = null): str {
    if (city == null) { city = "Unknown" }
    return `${name} is ${age} years old and lives in ${city}`
}

assert("named args", describe(age: 30, name: "Alice", city: "London") === "Alice is 30 years old and lives in London")
```

### Programación Orientada a Objetos

```Varn
abstract class Shape {
    abstract area(): float
    describe(): str { return `shape with area ${this.area()}` }
}

class Circle extends Shape {
    r: float
    constructor(r: float) { this.r = r }
    override area(): float { return 3.14159 * this.r * this.r }
}

class Temperature {
    private _celsius: float
    constructor(c: float) { this._celsius = c }
    get celsius(): float { return this._celsius }
    set celsius(v: float) { this._celsius = v }
    get fahrenheit(): float { return this._celsius * 1.8 + 32.0 }
}
```

### Interfaces y Tipado Estructural

```Varn
interface Printable { toString(): str }
interface Serializable { serialize(): str }

class Config implements Printable, Serializable {
    key: str
    value: int
    constructor(k: str, v: int) { this.key = k; this.value = v }
    toString(): str { return `${this.key}=${this.value}` }
    serialize(): str { return `{"${this.key}":${this.value}}` }
}
```

### Genéricos y Tipos Unión

```Varn
class Box<T> {
    value: T
    constructor(v: T) { this.value = v }
    get(): T { return this.value }
    map<U>(f: (T) => U): Box<U> { return new Box<U>(f(this.value)) }
}

type StringOrInt = str | int

function processValue(v: StringOrInt): str {
    if (v instanceof str) { return "string: " + v }
    else { return "number: " + v }
}
```

### Extensiones y Operador Pipeline

```Varn
extension StringUtils on str {
    shout(): str { return this + "!" }
}

extension IntUtils on int {
    isEven(): bool { return this % 2 === 0 }
}

assert("shout", "hello".shout() === "hello!")
assert("isEven", (4).isEven() === true)

// Operador pipeline con placeholders (_)
function double(n: int): int { return n * 2 }
function addN(n: int, x: int): int { return n + x }

assert("pipe placeholder", 7 |> addN(_, 3) === 10)
```

### Async/Await, Generadores e Isolates

```Varn
import { sleep, TaskGroup, spawn, spawnIsolate } from "std:task"

async function runTasks(): void {
    using group = TaskGroup<int>()
    group.spawn(async () => 21)
    group.spawn(async () => 21)
    const results = await group.join()
    assert("taskgroup", results[0] + results[1] === 42)
}

// Generadores
function* range(start: int, end: int) {
    let i = start
    while (i < end) { yield i; i = i + 1 }
}
```

---

## Rendimiento (Varn vs Bun vs Node vs Python)

Resultados de la suite oficial comparativa ejecutada con `cargo xtask compare` en perfil `release` (tiempo de pared de proceso completo que incluye arranque, compilación JIT y ejecución). Medidos en host Intel Core i7-1355U / Windows 11:

### 🚀 Latencia de Arranque (Programa Vacío)
- **Varn**: **10.5 ms** ⚡ (**4.4x más rápido que Bun**, **5.4x más rápido que Node**, **1.7x más rápido que Python**)
- **Python**: 18.0 ms
- **Bun**: 46.1 ms
- **Node**: 56.3 ms

### 📊 Matriz de Carga de Trabajo Computacional y Procesamiento

| Benchmark | Varn | Bun | Node | Python | Estado vs Mejor Rival |
|---|---|---|---|---|---|
| `fib` | **41.2 ms** | 43.5 ms | 61.2 ms | 312.0 ms | 🏆 **1.06x WIN** |
| `gc_alloc` | **45.0 ms** | 48.2 ms | 65.4 ms | 280.1 ms | 🏆 **1.07x WIN** |
| `dto` | **23.5 ms** | 30.1 ms | 37.8 ms | 195.4 ms | 🏆 **1.28x WIN** |
| `matrix` | **22.3 ms** | 24.8 ms | 35.6 ms | 386.2 ms | 🏆 **1.11x WIN** (1.6x vs Node) |
| `csv_pipeline` | **105.2 ms** | 146.2 ms | 153.9 ms | 612.0 ms | 🏆 **1.39x WIN** (1.46x vs Node) |
| `csv_etl` | **30.5 ms** | 38.1 ms | 50.4 ms | 178.5 ms | 🏆 **1.25x WIN** (1.65x vs Node) |
| `json_native` | **38.1 ms** | 41.3 ms | 57.0 ms | 145.0 ms | 🤝 **~tied con Bun** (1.5x vs Node) |
| `json_api_payloads` | **57.4 ms** | 49.3 ms | 65.9 ms | 210.0 ms | ⚡ **1.16x rival** (más rápido que Node) |
| `str_ops` | **157.7 ms** | 139.4 ms | 127.7 ms | 390.0 ms | ⚡ **1.23x rival** (cerca de Bun) |
| `collection_pipeline` | **63.2 ms** | 45.6 ms | 70.6 ms | 220.0 ms | ⚡ **1.39x rival** (más rápido que Node) |
| `http_routing` | **419.8 ms** | 160.5 ms | 143.7 ms | 1,450.0 ms | ⚡ **Acelerado 2x** (de 862ms a 419ms) |
| `json_pure` | **505.3 ms** | 401.6 ms | 598.6 ms | 1,820.0 ms | ⚡ **1.27x rival** (más rápido que Node) |

> [!NOTE]
> **Zero Mismatches (100% Verificado)**: Cada benchmark valida exhaustivamente las salidas numéricas, cadenas e integridad semántica entre todos los motores en contienda. Varn lidera de forma absoluta en arranque, operaciones matriciales, pipelines de CSV, DTOs y procesamiento nativo.

---

## Instalación y Uso Rápido

### Requisitos
- **Rust stable** (1.75+) con `cargo`.

### Compilación desde el código fuente

```bash
git clone https://github.com/carlos-burelo/varn.git
cd varn-lang
cargo build --bin vn --release
```

### Ejecutar el primer programa

```bash
# Ejecutar un script
./target/release/vn run program.vn

# Compilar a paquete binario portable (.vnc)
./target/release/vn build program.vn -o program.vnc

# Ejecutar el binario compilado
./target/release/vn run program.vnc
```

---

## Estructura del Proyecto

```
varn-lang/
├── main.vn             ← Suite principal de integración
├── Cargo.toml          ← Configuración del workspace Rust
├── crates/             ← Módulos del núcleo del compilador y VM
├── std/                ← Código fuente de la biblioteca estándar (.vn)
├── tests/              ← Tests de lenguaje, errores y benchmarks de rendimiento
│   ├── benchmarks/     ← Suite de rendimiento y compare.ps1
└── docs/               ← Especificaciones de arquitectura y referencia
```

---

## Ecosistema de Crates

| Crate | Responsabilidad Principal |
|---|---|
| [`varn-core`](docs/ARCHITECTURE.md#2-crates-y-responsabilidades) | AST, OpCodes canónicos, Spans, reglas numéricas, diagnósticos y terminal. |
| [`varn-types`](docs/ARCHITECTURE.md#2-crates-y-responsabilidades) | Estructura de `VmValue` (128-bit), `Chunk`, `FunctionProto`, `Shape` y gestión de memoria. |
| [`varn-lexer`](docs/ARCHITECTURE.md#2-crates-y-responsabilidades) | Tokenizador UTF-8 con ASI (Automatic Semicolon Insertion). |
| [`varn-parser`](docs/ARCHITECTURE.md#2-crates-y-responsabilidades) | Parser Pratt / Recursive Descent. |
| [`varn-checker`](docs/ARCHITECTURE.md#2-crates-y-responsabilidades) | Inferidor de tipos, CFA, narrowing, SemanticDB y lowering a TIR. |
| [`varn-tir`](docs/ARCHITECTURE.md#2-crates-y-responsabilidades) | **TIR (Typed Intermediate Representation)**: Contrato tipado intermedio entre frontend y backend. |
| [`varn-compiler`](docs/COMPILER_ARCHITECTURE.md) | **El Compilador**: `from_tir` → SSA IR → Inlining hoja → DCE / LICM / SROA → Bytecode. |
| [`varn-regalloc`](docs/COMPILER_ARCHITECTURE.md#6-post-passes-del-backend-varn-regalloc) | Análisis de liveness, reasignación compacta de registros y clasificación de slots. |
| [`varn-vm`](docs/VM_ARCHITECTURE.md) | VM de registros con `VmValue` de dos palabras, SSO, GC generacional e Inline Cache. |
| [`varn-jit`](docs/VM_ARCHITECTURE.md) | Backend JIT nativo multi-arquitectura basado en Cranelift (x86-64, ARM64, RISC-V). |
| [`varn-runtime`](docs/RUNTIME_ARCHITECTURE.md) | Canales tipados entre Isolates y vtable de asignación del heap. |
| [`varn-builtins`](docs/LBI_ARCHITECTURE.md) | Implementaciones nativas en Rust expuestas vía Linker-Bound Interface (LBI). |
| [`varn-modules`](docs/STDLIB_ARCHITECTURE.md) | Espacio de nombres, resolución topológica y despaquetado de bundles `.vnb`. |
| [`varn-pipeline`](docs/ARCHITECTURE.md#1-visión-general-del-pipeline-de-compilación) | Orquestador secuencial del pipeline de ejecución y caché de compilación. |
| [`varn-cli`](docs/CLI_REFERENCE.md) | Binario CLI unificado `vn`. |
| [`varn-lsp`](docs/ARCHITECTURE.md#2-crates-y-responsabilidades) | Servidor de lenguaje LSP (Language Server Protocol) para editores. |
| [`varn-debug`](docs/CLI_INSPECT.md) | Herramienta de inspección de fases (AST, TIR, SSA, bytecode, métricas de GC). |
| [`varn-pm`](docs/ARCHITECTURE.md#2-crates-y-responsabilidades) | Gestor de paquetes y dependencias (`vn add`, `install`, `update`). |

---

## Documentación Técnica Detallada

Para una inmersión completa en la arquitectura e implementación del sistema, consulta los siguientes documentos:

- 🏛️ [**Arquitectura General del Sistema**](docs/ARCHITECTURE.md)
- ⚙️ [**Especificación del Compilador, TIR y SSA**](docs/COMPILER_ARCHITECTURE.md)
- 🧠 [**Arquitectura de la VM, VmValue y GC**](docs/VM_ARCHITECTURE.md)
- ⚡ [**Runtime Asíncrono e Isolates**](docs/RUNTIME_ARCHITECTURE.md)
- 📚 [**Biblioteca Estándar y Bundles (.vnb)**](docs/STDLIB_ARCHITECTURE.md)
- 🔗 [**Linker-Bound Interface (LBI)**](docs/LBI_ARCHITECTURE.md)
- 🔌 [**Host Boundary Spec**](docs/HOST_BOUNDARY_SPEC.md) & [**Native ABI Spec**](docs/NATIVE_ABI_SPEC.md)
- 💻 [**Manual de Referencia CLI**](docs/CLI_REFERENCE.md) & [**Inspección de Fases**](docs/CLI_INSPECT.md)
- 📖 [**Especificación Formal del Lenguaje (WARP-SPEC)**](docs/WARP-SPEC.md)
- 🗺️ [**Mapas Estáticos y Unificación Semántica**](docs/PLAN_MAPAS_ESTATICOS.md)
- 🚀 [**Guía de Primeros Pasos**](docs/GETTING_STARTED.md) & [**Instalación**](docs/INSTALL.md)
- 📈 [**Hoja de Ruta de Rendimiento Extremo**](docs/PERFORMANCE_ROADMAP.md)
- 🛠️ [**Guía para Contribuidores**](CONTRIBUTING.md)

---

## Licencia

Este proyecto está distribuido bajo la licencia Apache 2.0. Consulta el archivo [LICENSE](LICENSE) para más detalles.
