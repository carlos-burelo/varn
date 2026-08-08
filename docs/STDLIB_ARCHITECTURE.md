# Arquitectura de la Biblioteca Estándar (`std:*` & `.vnb`)

Este documento especifica la estructura, el sistema de resolución y el mecanismo de empaquetado de la biblioteca estándar de **Varn**.

---

## Tabla de Contenidos

- [1. Visión General de la Stdlib](#1-visión-general-de-la-stdlib)
- [2. Módulos de la Biblioteca Estándar](#2-módulos-de-la-biblioteca-estándar)
- [3. Arquitectura del Bundle `.vnb`](#3-arquitectura-del-bundle-vnb)
- [4. Jerarquía de Resolución y Procedencia](#4-jerarquía-de-resolución-y-procedencia)
- [5. Integración con Bindings Nativos LBI](#5-integración-con-bindings-nativos-lbi)

---

## 1. Visión General de la Stdlib

La biblioteca estándar de Varn está escrita principalmente en el propio lenguaje Varn (`std/*.vn`) e integrada directamente en el binario ejecutable mediante un bundle empaquetado `.vnb`. Para operaciones del sistema de bajo nivel (I/O, sockets, criptografía), los módulos se comunican con implementaciones nativas en Rust a través de la Interfaz Ligada al Linker (LBI).

```mermaid
flowchart TD
    A["Código de Usuario (import { readFile } from 'std:fs')"] --> B["varn-modules (Resolución de Nombre)"]
    B --> C{"¿Módulo std:*?"}
    C -- Sí --> D["Cargar desde Bundle .vnb (std/fs.vn)"]
    D --> E["Invocaciones a opcodes nativos host_fn"]
    E --> F["varn-builtins (Ejecución Nativa en Rust)"]
```

---

## 2. Módulos de la Biblioteca Estándar

| Módulo | Descripción | Dependencias Nativas (Rust) |
|---|---|---|
| `std:fs` | Sistema de archivos (lectura, escritura, streams, permisos). | `varn-builtins::fs` |
| `std:io` | Entrada/Salida estándar (`stdin`, `stdout`, `stderr`). | `varn-builtins::io` |
| `std:task` | Concurrencia, `TaskGroup`, `parallel`, `spawnIsolate`. | `varn-runtime` |
| `std:crypto` | Hashing (SHA256, MD5) y encriptación. | `varn-builtins::crypto` |
| `std:time` | Medición de tiempo, temporizadores y formateo de fechas. | `varn-builtins::time` |
| `std:json` | Serialización y parsing ultrarrápido de JSON. | `varn-builtins::json` |
| `std:reflect` | Introspección de clases, decoradores y metadatos (`MetaKey`). | `varn-builtins::reflect` |
| `std:sys` | Información del entorno de ejecución, OS, CPU y memoria. | `varn-builtins::sys` |
| `std:math` | Operaciones matemáticas y funciones trigonométricas. | `varn-core::numeric` |
| `std:testing` | Framework de pruebas unitarias y aserciones. | `varn-builtins::testing` |

---

## 3. Arquitectura del Bundle `.vnb`

Para distribuir el lenguaje en un **único ejecutable autónomo sin dependencias de archivos externos**, el script de compilación `crates/varn-cli/build.rs` compila automáticamente las fuentes de `std/` en un bundle binario comprimido `.vnb`:

```mermaid
flowchart LR
    A["Fuentes std/*.vn"] --> B["build.rs\n(Parsing & Precompilación)"]
    B --> C["std.vnb\n(Bundle Precompilado)"]
    C --> D["varn-cli\n(include_bytes! en ejecutable final)"]
```

---

## 4. Jerarquía de Resolución y Procedencia

Al importar un módulo `std:*`, `varn-modules` determina la fuente del código según el siguiente orden de prioridad:

```mermaid
flowchart TD
    A["Importación std:*"] --> B{"VARN_STD especificada?"}
    B -- VARN_STD=@embedded --> C["Usar Bundle Embebido en Ejecutable (.vnb)"]
    B -- VARN_STD=/ruta/custom --> D["Usar Ruta Personalizada"]
    B -- No especificada --> E{"¿Existe directorio std/ local?\n(dev-checkout)"}
    E -- Sí --> F["Usar Fuentes de std/ (Modo Desarrollo)"]
    E -- No --> C
```

---

## 5. Integración con Bindings Nativos LBI

Los archivos de la stdlib exponen una API limpia con tipos estáticos mientras delegan el trabajo pesado a funciones nativas mediante anotaciones especiales:

```Varn
// std/fs.vn
import { @native_read_file } from "builtin:host"

export function readFile(path: str): str {
    if (path.length === 0) {
        throw new Error("Path cannot be empty")
    }
    return @native_read_file(path)
}
```
