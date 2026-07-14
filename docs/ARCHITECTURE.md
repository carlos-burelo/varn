# Arquitectura Interna de Varn

Descripción técnica de la implementación del lenguaje Varn, escrito completamente en Rust.

## 1. Pipeline de Compilación y Ejecución

`varn-pipeline` orquesta las fases (son las que reporta `vn bench`):

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
[ varn-opt ]      → HIR → SSA → passes → FunctionProto (bytecode)
      │
      ▼
[ varn-backend ]  → post-passes: liveness, register allocation, slot kinds
      │
      ├──────────────► [ varn-jit ]  → código máquina x86-64 (compilación eager)
      ▼
[ varn-vm ]       → VM register-based, NaN-boxing, GC generacional, inline caches
      │
      ▼
[ varn-runtime ]  → Scheduler async (Tokio + LocalSet) e isolates en hilos
```

> No existe ningún crate `varn-compiler` ni `varn-ir`. La generación de bytecode
> vive en `varn-opt`; los post-passes en `varn-backend`.

---

## 2. Crates y Responsabilidades

### Base
- **`varn-core`**: AST, OpCode (134, sin prefijo `Op`), ModuleId, Span, y las reglas
  numéricas (`numeric.rs` — fuente única para const-folding, intérprete y JIT). Sin
  dependencias internas.
- **`varn-types`**: `VmValue`, `Chunk`, `FunctionProto`, `ClassObj`, `Closure`,
  `ObjData`/`ObjRef`, `Shape`, `ResourceStore`. Tipos compartidos por VM y builtins.
- **`varn-base`** / **`varn-utilities`**: utilidades compartidas (la segunda incluye
  el formato de terminal y colores).
- **`varn-diagnostics`**: errores con spans y subrayados; formato CLI y LSP.

### Frontend
- **`varn-lexer`**: Tokenizer. UTF-8, escapes, ASI (Automatic Semicolon Insertion).
- **`varn-parser`**: Recursive-Descent + Pratt para precedencia. `|>`, ternarios,
  argumentos nombrados.
- **`varn-checker`**: Type checker multi-fase. Hoisting, inferencia, CFA, narrowing,
  resolución de módulos. Produce TypedAST + SemanticDB.

### Compilación
- **`varn-opt`**: **el compilador**. TypedAST → HIR → inlining → SSA → passes
  (`tco`, `const_fold`, `fixed_fields`, `dce`, `cfg`, en bucle de punto fijo) →
  bytecode. Ver [COMPILER_ARCHITECTURE.md](COMPILER_ARCHITECTURE.md).
- **`varn-backend`**: post-passes sobre el bytecode ya emitido — `liveness`,
  `regalloc_post`, `slot_kinds` (este último alimenta el `register_meta` del que
  depende el JIT).

### Ejecución
- **`varn-vm`**: VM register-based con NaN-boxing, heap con **GC generacional**
  (nursery + promoción) y mark-and-sweep en old-gen, inline caches polimórficos de 8
  entradas, upvalues open/closed. Ver [VM_ARCHITECTURE.md](VM_ARCHITECTURE.md).
- **`varn-jit`**: JIT x86-64. Ensamblador propio, register allocation, hoisting de
  loops, safepoints. Compila **eager** al construir el closure (sin umbral de calor);
  si declina una función, esa función se interpreta. Los layouts de memoria que emite
  se **prueban al arrancar**, no se hardcodean.
- **`varn-runtime`**: scheduler async sobre Tokio multi-thread. Las tareas Varn
  `!Send` corren en un `LocalSet`; los isolates son hilos con su propia VM y se
  comunican por canales de mensajes sendables.

### Stdlib y Host
- **`varn-builtins`**: implementaciones nativas (Rust) de `core:`/`runtime:`/globals.
  Usa LBI (Linker-Bound Interface) para autodescubrimiento. Ver
  [LBI_ARCHITECTURE.md](LBI_ARCHITECTURE.md).
- **`varn-op-macros`**: proc macros `#[varn_module]`, `#[varn_fn]`, `#[varn_class]`, …
- **`varn-modules`**: registro canónico de módulos, resolución topológica, bundle
  `.vnb` y resolución de la std activa.

### Herramientas
- **`varn-pipeline`**: orquesta las fases (read, lex, parse, check, compile, optimize,
  execute), caché de bytecode, carga de la stdlib.
- **`varn-cli`**: binario `vn`. Comandos: `run`, `check`, `eval`, `repl`, `bench`,
  `debug`, `build`, `pkg`, `init`, `doctor`, `cache`, `lsp`, `completions`.
- **`varn-lsp`**: LSP (tower-lsp + tokio). Consulta la SemanticDB del checker.
- **`varn-pm`**: package manager. Semver sobre tags git, caché `~/.vn/cache/`, SHA256.
- **`varn-debug`**: volcado de fases (tokens, ast, hir, ssa, bytecode, types, …) y
  profiling.
- **`xtask`**: tooling del repo. `cargo xtask build-std` compila `std/` a `std.vnb`.

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
| `int`      | QNAN + TAG_INT + payload **48 bits** (wrap a 48) |
| puntero    | SIGN + QNAN + TAG_PTR + heap index 32 bits      |

### Heap y GC
Objetos complejos (strings, arrays, objetos, closures, clases) viven en el heap, que es
**generacional**: los objetos nacen en un nursery de 4096 slots y el GC menor promueve
los vivos al old-gen, donde corre un mark-and-sweep tricolor sobre un `Vec<Option<HeapObj>>`
con free list. Escribir una referencia a nursery dentro de un objeto de old-gen requiere
**write barrier** (remembered set).

Un objeto es **una sola allocation**: cabecera y campos comparten el mismo bloque `Rc`
(cola DST dimensionada a la shape). La dirección de esa allocation *es* la identidad del
objeto, así que nunca se mueve.

### CallFrames
Las variables locales son offsets numéricos sobre el registro base del frame actual (`registers[base + slot]`). Acceso O(1) sin hashmaps.

### Upvalues
Variables capturadas por closures. **Abiertas**: índice en registros del frame padre. **Cerradas**: copiadas al heap cuando el frame padre termina.

### Inline Cache
Cada IC site guarda hasta **8 entradas** (polimórfico) indexadas por **shape id**, no por
"class_id". Un site que ve demasiadas shapes se marca megamórfico y deja de cachear.
Intérprete y JIT comparten el mismo IC. Las tasas de hit dependen del workload: medir con
`vn bench -v`, no citar cifras.

---

## 5. Runtime Asíncrono

`varn-runtime` usa un runtime Tokio multi-thread compartido, pero cada VM/closure `!Send` se ejecuta en un `tokio::task::LocalSet`. Para paralelismo entre hilos, Varn expone **isolates**: cada uno es un hilo con su propia VM y su propio heap, y se comunican por **canales tipados** (`channel<T>(n)` → `Sender`/`Receiver`). Los valores cruzan como `SendValue` (representación independiente del heap); los compuestos van envueltos en un `SendEnvelope` y se materializan en el heap del receptor.

- **`await`**: La VM emite `Suspend::Task`. El scheduler cede al Tokio event-loop hasta que la promesa se resuelve.
- **`spawn`**: Crea una nueva tarea en el LocalSet.
- **`parallel([A, B])`**: TaskGroup — resuelve cuando todos los hijos resuelven.
- **`spawnIsolate(fn, args)`**: crea un worker en otro hilo; `fn` debe ser exportada top-level y los argumentos deben ser sendables.
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

`std:*` ya no vive embebido en el binario: sus fuentes están en el árbol
top-level `std/` (fuera de `varn-builtins`) y se compilan a un artefacto
versionado `std.vnb` vía `cargo xtask build-std`. `core:`/`runtime:`/globals
siguen embebidos — son el host. Detalle completo del empaquetado, formato
`.vnb`, resolución (`varn.json` → `VARN_STD` → toolchain) y startup medido en
[STDLIB_ARCHITECTURE.md](STDLIB_ARCHITECTURE.md).

---

## 8. Formato `.vnc`

`vn build program.vn` produce un `.vnc`: magic `WRC\0` + versión u32 LE + artefacto serializado con postcard. `vn run program.vnc` omite todas las fases de compilación y ejecuta directamente.
