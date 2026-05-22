# Arquitectura Interna de Varn

Descripción técnica de la implementación del lenguaje Varn, escrito completamente en Rust.

## 1. Pipeline de Compilación y Ejecución

```text
Código Fuente (.vn)
      │
      ▼
[ varn-lexer ]    → Tokens
      │
      ▼
[ varn-parser ]   → AST
      │
      ▼
[ varn-checker ]  → TypedAST + SemanticDB (inferencia, CFA, narrowing)
      │
      ▼
[ varn-compiler ] → FunctionProto / Bytecode (Chunks)
      │
      ▼
[ varn-vm ]       → VM register-based con NaN-Boxing
      │
      ▼
[ varn-runtime ]  → Scheduler asíncrono (Tokio LocalSet), I/O, timers
```

---

## 2. Crates y Responsabilidades

### Base
- **`varn-core`**: AST, OpCode, ModuleId, Span, constantes. Sin dependencias internas.
- **`varn-types`**: VmValue, Chunk, FunctionProto, ClassObj, Closure, ResourceStore. Tipos compartidos por VM y builtins.
- **`varn-diagnostics`**: Reportes de errores con spans, subrayados, integración LSP.

### Frontend
- **`varn-lexer`**: Tokenizer. UTF-8, escape sequences, ASI (Automatic Semicolon Insertion).
- **`varn-parser`**: Parser Recursive-Descent + Pratt para precedencia de operadores. Incluye `|>`, ternarios, named arguments.
- **`varn-checker`**: Type checker multi-fase. Hoisting, inferencia, CFA, narrowing, resolución de módulos.

### Backend
- **`varn-compiler`**: Lowering de AST a bytecode. Slots estáticos, upvalues, constant pool, back-patching.
- **`varn-vm`**: VM register-based con NaN-boxing, Inline Cache, fast-path calls, upvalues open/closed.
- **`varn-runtime`**: Tokio LocalSet scheduler. Async/await, generators, timers, I/O host.

### Stdlib y Host
- **`varn-builtins`**: Implementaciones nativas (Rust) de stdlib. Usa LBI (Linker-Bound Interface) para autodescubrimiento. Ver [LBI_ARCHITECTURE.md](LBI_ARCHITECTURE.md).
- **`varn-op-macros`**: Proc macros `#[varn_module]`, `#[varn_fn]`, `#[varn_class]`, etc. para registrar ops nativas.
- **`varn-modules`**: Registro canónico de módulos, resolución topológica, caché de bytecode.
- **`varn-base`**: Utilidades compartidas.

### Herramientas
- **`varn-cli`**: Binario `wr`. Orquesta el pipeline completo. Comandos: `run`, `check`, `build`, `bench`, `disasm`, `inspect`, `repl`, `add`, `install`, etc.
- **`varn-lsp`**: Language Server Protocol (tower-lsp + tokio). Consulta SemanticDB del checker. Hover, completions, go-to-definition.
- **`varn-pm`**: Package manager. Resolución semver sobre git tags, caché global `~/.vn/cache/`, SHA256 integrity.
- **`varn-debug`**: Profiling, disassembly, inspección.

---

## 3. Type Checker (`varn-checker`)

Opera en múltiples fases:

1. **Hoisting y Binding**: Registra firmas de funciones, clases e interfaces antes de validar cuerpos. Permite referencias hacia adelante y recursión.
2. **Normalización de Tipos**: `(A | B) | (A | C)` → `A | B | C`. Resuelve aliases para evitar ciclos.
3. **Control Flow Analysis (CFA) y Narrowing**: Si `x instanceof str` en una rama, el tipo de `x` se estrecha a `str` dentro de esa rama.
4. **SemanticDB**: Base de datos relacional con tipo de cada subexpresión, ID de símbolo y Span exacto.

El LSP consulta la SemanticDB directamente — no re-evalúa lógica.

---

## 4. VM Register-Based y NaN-Boxing

`varn-vm` usa una VM basada en registros (no stack-based) con NaN-Boxing.

### NaN-Boxing
Todos los valores caben en 64 bits aprovechando el espacio de Quiet NaN del IEEE 754:

| Tipo       | Codificación                                    |
|------------|-------------------------------------------------|
| `float`    | Valor IEEE 754 normal (no-QNAN)                 |
| `null`     | QNAN + TAG_NULL                                 |
| `false`    | QNAN + TAG_FALSE                                |
| `true`     | QNAN + TAG_TRUE                                 |
| `int`      | QNAN + TAG_INT + payload 32 bits                |
| puntero    | SIGN + QNAN + TAG_PTR + heap index 32 bits      |

### Heap y Free List
Objetos complejos (strings, arrays, closures, clases) viven en el heap. El heap usa un **Free List** (`free: Vec<u32>`) para reusar slots sin llamar al sistema operativo en cada alloc.

### CallFrames
Las variables locales son offsets numéricos sobre el registro base del frame actual (`registers[base + slot]`). Acceso O(1) sin hashmaps.

### Upvalues
Variables capturadas por closures. **Abiertas**: índice en registros del frame padre. **Cerradas**: copiadas al heap cuando el frame padre termina.

### Inline Cache
`GetProp`/`SetProp` y llamadas a métodos se cachean por clase y slot. En benchmark de la suite (529 tests), fast-path calls representan ~60% de todas las llamadas.

---

## 5. Runtime Asíncrono

`varn-runtime` envuelve la VM síncrona en un `tokio::task::LocalSet` (hilo único, máximo rendimiento, sin locks).

- **`await`**: La VM emite `Suspend::Task`. El scheduler cede al Tokio event-loop hasta que la promesa se resuelve.
- **`spawn`**: Crea una nueva tarea en el LocalSet.
- **`parallel([A, B])`**: TaskGroup — resuelve cuando todos los hijos resuelven.
- **Generadores**: `Suspend::Yield` envía el valor al `GenChannel`.
- **Poll budget**: 256 ciclos antes de `yield_now()` para no ahogar el event-loop.

---

## 6. LBI — Linker-Bound Interface

Las ops nativas se registran sin tabla centralizada manual. Ver detalles en [LBI_ARCHITECTURE.md](LBI_ARCHITECTURE.md).

Resumen:
1. `#[varn_module("std:fs")]` + `#[varn_fn]` inyectan `NativeOpEntry` en secciones del linker (`.varn_ops$B` en Windows, `__DATA,varn_ops` en macOS, `varn_ops` en Linux).
2. Al arrancar, `iter_native_ops()` escanea la sección y reconstruye el dispatch table.
3. `build_module(id, ctx)` ensambla el objeto Varn para el módulo dado.
4. Cada entry puede declarar una capability requerida (`cap = "fs.write"`).

---

## 7. Sistema de Módulos

Tres espacios de nombres:
- `builtin:*` — símbolos globales inyectados al arrancar (`print`, clases base, etc.).
- `std:*` — stdlib lazy-loaded (`std:fs`, `std:time`, `std:crypto`, etc.).
- Rutas relativas (`"./module"`) — código del usuario.

Resolución centralizada en `varn-modules`. Caché de bytecode en `.vn/cache/` invalidado por hash de contenido.

---

## 8. Formato `.vnc`

`vn build program.vn` produce un `.vnc`: magic `WRC\0` + versión u32 LE + artefacto serializado con postcard. `vn run program.vnc` omite todas las fases de compilación y ejecuta directamente.
