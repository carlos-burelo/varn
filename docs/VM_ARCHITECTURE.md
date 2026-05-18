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
| `int`      | 0    | `111...1` | `TAG_INT` + payload 32 bits                |
| puntero    | **1**| `111...1` | `TAG_PTR` + heap index 32 bits             |

El bit de signo encendido reserva los punteros fuera del espacio de Signalling NaN.

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

En benchmark de la suite completa (534 tests): ~0% IC misses en steady state.

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

En benchmark de la suite completa (534 tests):

```
VM Profile
  IC hits                         0  (0.0% miss rate en steady state)
  calls vm-fast                 687  (60.3%)
  calls slow/prepare             23  (2.0%)
  calls native                  429  (37.7%)
  heap allocs                 1 359
  frame pushes                  133
  frame pops                    192

Throughput: ~475 runs/s  (mean 2.1 ms end-to-end)
```
