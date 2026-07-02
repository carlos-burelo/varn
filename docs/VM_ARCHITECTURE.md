# Arquitectura de la VM (varn-vm)

Implementación de la máquina virtual register-based de Varn.

## 1. VmValue y NaN-Boxing

Todos los valores caben en 64 bits reutilizando el espacio de Quiet NaN del estándar IEEE 754.

### Esquema de bits

| Tipo       | Sign | Exponente | Payload (52 bits)                         |
|------------|------|-----------|-------------------------------------------|
| `float`    | ±    | IEEE 754  | Valor fraccionario (cualquier patrón no-QNAN) |
| `null`     | 0    | `111...1` | `TAG_NULL` (`0x0001_0000_0000_0000`)       |
| `false`    | 0    | `111...1` | `TAG_FALSE` (`0x0002_0000_0000_0000`)      |
| `true`     | 0    | `111...1` | `TAG_TRUE` (`0x0003_0000_0000_0000`)       |
| `int`      | 0    | `111...1` | `TAG_INT` + payload 48 bits                |
| puntero    | **1**| `111...1` | `TAG_PTR` + heap index 32 bits             |

El bit de signo encendido reserva los punteros fuera del espacio de Signalling NaN.

### Semántica de enteros

Fuente única de las reglas: `varn-core/src/numeric.rs`. Todos los tiers
(const-folding en compile time, intérprete y JIT) deben ser bit-idénticos.

- `int` es un entero de **48 bits** en complemento a dos (el payload del
  NaN-box). Rango: `±140_737_488_355_327` (`±2^47 - 1`).
- La aritmética entera (`+`, `-`, `*`, `**`) **envuelve (wrap) a 48 bits**.
  No hay promoción silenciosa a `float` en overflow: el tipo estático `int`
  es honesto y los fast paths tipados del JIT no necesitan guards de
  overflow.
- `int / int` produce **siempre `float`** (incluso si la división es
  exacta). El híbrido histórico "exacto → int, inexacto → float" hacía que
  el tipo del valor dependiera de los valores en runtime.
- `int % int` produce `int` (resto truncado). Divisor cero: error de
  runtime, igual que `int / 0`.
- `int ** int` produce `int` (con wrap). Exponente negativo: error de
  runtime (`negative exponent in integer power`).

### Tipos en heap
`HeapObj` aloja lo que no cabe en 64 bits:
- `Str(RuntimeString)` — string interned o owned
- `Array(RuntimeArray)`
- `Object(RuntimeObject)`
- `Closure(Rc<Closure>)`
- `NativeFn(name, NativeFn)`
- `Class(Rc<ClassObj>)`
- `Generator`, `Task`, `Map`, `Set`, `Decimal`, `Range`, etc.

---

## 2. Heap y Free List

El heap es un `Vec<Option<HeapObj>>` con un `Vec<u32>` de slots libres.

- **Alloc**: `free.pop()` reutiliza slot existente; si vacío, `push` al final.
- **Free**: slot devuelto a `free`. Sin GC mark-and-sweep. Gestión determinista por `Rc<RefCell<T>>`.

---

## 3. VM Register-Based

La VM es register-based, no stack-based. Las variables locales son registros en un frame de registros plano.

### Registros y frames
Cada `CallFrame` contiene:
- `ip`: instruction pointer (índice local en el chunk del closure)
- `base`: offset en el array global de registros donde empiezan los registros locales de este frame
- `closure`: puntero `Rc<Closure>` al código y upvalues

Variable local en slot `k` → `registers[frame.base + k]`. Acceso O(1).

### Registros vs stack
Las instrucciones operan sobre slots explícitos (`OpLoad r0, r1`, `OpAdd dst, r0, r1`), no sobre un stack implícito. Elimina push/pop redundantes.

---

## 4. Upvalues (Closures)

Variables capturadas por funciones internas:

1. **Abierto**: mientras el frame padre está vivo, el upvalue apunta al slot en los registros del padre. Lecturas/escrituras se reflejan en tiempo real.
2. **Cerrado**: cuando el frame padre termina, `OpCloseUpvalue` copia el valor del registro al heap. La closure retiene el valor indefinidamente.

---

## 5. Inline Cache

`GetProp` y `SetProp` se optimizan por clase + slot:

- Primera vez: lookup en la shape del objeto. Guarda `(class_id, slot_index)` en el IC del opcode.
- Siguiente vez con mismo objeto de misma clase: acceso directo por slot, sin hash lookup.

La eficacia del IC depende del workload. En algunos programas pequeños el perfil puede mostrar `0` hits simplemente porque casi no hay sites calientes; en workloads orientados a objetos el beneficio aparece en steady state.

---

## 6. Fast-Path Calls

`OpCall` tiene tres rutas:

1. **VM fast-path** (~60%): closure sencillo sin generators, sin rest params complejos. Salta directo al frame hijo sin preparación completa.
2. **Native fast-path** (~38%): `NativeFn` — llama el puntero Rust directamente.
3. **Slow path** (~2%): generators, async, bound methods complejos.

---

## 7. Excepciones y Try/Catch

La VM rastrea bloques try sin desenrollar el stack nativo de Rust. Guarda `(frame_depth, register_depth, catch_ip)` en un `TryHandler`. Si surge `OpThrow`, la VM rebobina hasta las profundidades guardadas y salta al `catch_ip`.

---

## 8. Async y Suspend

La VM es síncrona por diseño. Operaciones asíncronas emiten un `VmSuspend`:

- `Suspend::Task(AsyncTask)` — `await` sobre una promesa
- `Suspend::Timer(Duration)` — `sleep`
- `Suspend::Yield(VmValue)` — valor de generator

El frame queda "congelado". `varn-runtime` (Tokio) lo reanuda cuando la tarea resuelve.

---

## 9. Métricas de Performance

No hay una sola cifra honesta para "la VM de Varn" porque el benchmark actual mide más fases y más métricas que antes, y la mezcla cambia mucho entre workloads.

Ejemplos reales del repo actual (`--runs 10`, build `dev`, 2026-06-05):

- `tests/45-simple-file-test.vn`: p50 end-to-end `912 µs`
- `tests/21-async.vn`: p50 end-to-end `1.901 ms`
- `tests/47-isolates-multithread.vn`: p50 end-to-end `33.16 ms`

El CLI ya imprime además:

- hits/misses de IC por operación
- distribución `vm-fast` / `slow` / `native`
- allocations y GC
- hotspots de opcodes
- stats de JIT
