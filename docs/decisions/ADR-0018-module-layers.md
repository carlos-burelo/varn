# ADR 0018: Capas de módulos y prefijos

## Estado
Aceptada (2026-09-24), con el usuario. Spec §55–§56. Complementa ADR-0017.

## Contexto
Los prefijos crecieron sin contrato: `core:*` (implícitos) mezcla tipos,
errores, funciones globales e interfaces en módulos por accidente
(`core:global`, `core:intrinsics`, `core:int`, `core:str`… y un `core:types`
que sólo tiene alias utilitarios); `Option`/`Result` viven en `std:result`, una
librería que hay que importar, aunque son vocabulario del lenguaje; `core:*`
sólo admite declaraciones respaldadas por natives en Rust.

## Decisión
| Capa | Prefijo | Contenido | Carga |
|---|---|---|---|
| Builtins del lenguaje | `core:*` | lo que el compilador y el runtime conocen | implícita: siempre en scope, no se importa |
| Plataforma host | `runtime:*` | natives de SO que respaldan a `std` | importable sólo desde `std:*` |
| Librería estándar | `std:*` | código Varn | import explícito, nunca implícita |
| Usuario | ruta relativa / `pkg:*` | paquetes | import explícito |

Módulos de `core:*`:
- `core:types` — tipos del lenguaje: clases de los primitivos (`int`,
  `float`, `bigint`, `decimal`, `bool`, `char`, `str`, `Bytes`), colecciones
  (`Array`, `Map`, `Set`, `Range`), `Option`/`Result` (con `Some`, `None`,
  `Ok`, `Err`), `Generator`/`AsyncGenerator`/`Iterator`/`AsyncIterator`,
  `TaskHandle` y los alias utilitarios (`Partial`, `Pick`, …).
- `core:errors` — `Error` y su jerarquía de plataforma (`TypeError`,
  `RangeError`, `IntegerOverflow`, `DivisionByZero`, `MatchError`).
- `core:globals` — `print`, `debug`, `assert`, `assertSummary`, `input`,
  `NaN`, `Infinity`, `isIsolate`, `console`.
- `core:capabilities` — `Equatable`, `Comparable`, `Hashable`, `Cloneable`,
  `Default`, `Display`, `Debug`, `Indexable`, `Add`…`Neg`, `Disposable`,
  `AsyncDisposable`.

`core:*` puede contener código Varn además de declaraciones: se compila en el
bundle y el VM lo ejecuta al arrancar, antes que el programa, poblando los
globals del núcleo. `Option`/`Result` son el primer caso.

`Symbol` se borra del lenguaje: sin iteración por símbolos (ADR-0017) no
tiene uso en el núcleo.

Un tipo del núcleo se identifica por su origen (`core:types`), nunca por su
texto: un `Result` declarado por el usuario no es el del núcleo.

## Migración (cada paso, un commit verde)
1. `runtime:*` sólo importable desde `std:*` (diagnóstico en el import).
2. Reorganizar `core:*` en los cuatro módulos (renombres de ids y archivos).
3. Borrar `Symbol`.
4. `core:*` con código Varn compilado; `Option`/`Result` a `core:types`;
   `std:result` desaparece; las comprobaciones del checker por texto
   `"Result"`/`"Option"` pasan a origen.
