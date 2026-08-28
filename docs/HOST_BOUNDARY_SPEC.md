# Especificación del Límite Host / VM (`HOST_BOUNDARY_SPEC.md`)

Este documento especifica la interfaz de frontera (*Host Boundary*) que separa el runtime de **Varn** del código nativo en Rust, definiendo las reglas de conversión de tipos, manejo de errores y seguridad de memoria.

---

## Tabla de Contenidos

- [1. Visión General del Límite Host](#1-visión-general-del-límite-host)
- [2. Conversión de Tipos (`VmValue` ↔ Rust)](#2-conversión-de-tipos-vmvalue--rust)
- [3. Seguridad de Memoria y GC Roots](#3-seguridad-de-memoria-y-gc-roots)
- [4. Manejo de Errores y Excepciones Nativas](#4-manejo-de-errores-y-excepciones-nativas)

---

## 1. Visión General del Límite Host

El Límite Host es el punto de contacto entre la máquina virtual basada en registros de Varn y las funciones nativas implementadas en Rust (`varn-builtins`).

```mermaid
flowchart LR
    A["VM Frame (Bytecode)"] -->|Call Native Op| B["Host Boundary Intercept"]
    B -->|Convert VmValue -> Rust Native| C["Función Rust (varn-builtins)"]
    C -->|Convert Rust Native -> VmValue| B
    B -->|Return Result| A
```

---

## 2. Conversión de Tipos (`VmValue` ↔ Rust)

La conversión de tipos entre el formato NaN-boxed de 64 bits de la VM y las estructuras de datos nativas de Rust se realiza mediante primitivas optimizadas:

| Tipo Varn | Representación `VmValue` | Tipo Rust | Método de Conversión |
|---|---|---|---|
| `int` | QNAN + Tag Int, payload de **48 bits** | `i64` (rango i48) | `val.to_int()` / `VmValue::from_int(n)` |
| `float` | Standard IEEE 754 | `f64` | `val.to_float()` / `VmValue::from_float(f)` |
| `bool` | QNAN + Tag Bool | `bool` | `val.to_bool()` / `VmValue::from_bool(b)` |
| `str` | Pointer a Heap String | `&str` / `String` | `ctx.expect_string(val)` / `ctx.alloc_string(s)` |
| `Array` | Pointer a Heap Object | `&[VmValue]` | `ctx.expect_array(val)` / `ctx.alloc_array(v)` |

> **`int` es i48, no i64.** El tipo Rust del lado del host es `i64`, pero solo
> los 48 bits bajos sobreviven al boxing: el payload del NaN-box es de 48 bits
> y `VmValue::from_int` enmascara con `MASK_INT48`. Un builtin que devuelva un
> `i64` legítimo fuera de `[-2^47, 2^47-1]` se **trunca en silencio**, sin error
> ni saturación. Rango representable: `-140737488355328 ..= 140737488355327`.
>
> Esta tabla decía `i64` sin más, lo cual describía mal el contrato de la
> frontera de host. Las reglas normativas están en `varn-core/src/numeric.rs`
> (fuente única) y el comportamiento está fijado en `tests/53-int48-wrapping.vn`.

---

## 3. Seguridad de Memoria y GC Roots

Cuando una función nativa de Rust asigna memoria en el Heap de Varn (por ejemplo, al crear un `String` o un `Array` intermedio), debe proteger esa referencia de ser recolectada prematuramente si el GC menor se activa durante la llamada:

```rust
// Regla: Registrar el valor como raíz temporal (GC Root)
let temp_str = ctx.alloc_string("ejemplo");
ctx.push_root(temp_str); // Protege la referencia
// Operación nativa que puede desencadenar GC...
ctx.pop_root();
```

---

## 4. Manejo de Errores y Excepciones Nativas

Las funciones nativas en Rust no deben causar pánicos (`panic!`). Toda falla I/O o de argumento debe retornarse envuelta en un `VmResult::Err`:

```rust
if path.is_empty() {
    return Err(VmError::runtime_error("Path must not be empty"));
}
```

La VM interceptará este error y elevará una excepción recuperable en el código Varn (`try / catch`).

---

## 5. Sistema de Capacidades y Seguridad de Grano Fino (*Capabilities*)

El acceso a recursos externos del sistema operativo (disco, red, variables de entorno, procesos, FFI) se encuentra estrictamente mediado por el `CapabilitySet` en la interfaz `NativeCtx`:

- **Fast-path Bitmask (`u64`)**: Comprobación bit a bit en 1 ciclo de CPU (`0.3ns`) para operaciones no restringidas (`CAP_FS_READ`, `CAP_FS_WRITE`, `CAP_NET_CLIENT`, `CAP_NET_SERVER`, `CAP_SYS_ENV`, `CAP_SYS_EXEC`, `CAP_SYS_FFI`).
- **Filtros granulares de rutas y hosts**: Validación al momento de abrir el recurso (`open()`, `connect()`).
- **Aislamiento en Sandbox (`--sandbox`)**: Ejecución con cero permisos que bloquea determinísticamente cualquier intento de interacción con el host retornando `SecurityError: Permission denied (...)`.
