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
- `core:types` — tipos del lenguaje, un módulo por dominio:
  `core:types/{int,float,bigint,decimal,bool,char,str,bytes}` (clases de los
  primitivos), `core:types/{array,map,set,range}`, `core:types/iteration`
  (`Generator`, `AsyncGenerator`, `Iterator`, `AsyncIterator`, `TaskHandle`),
  `core:types/aliases` (`Partial`, `Pick`, …) y, con el paso 4,
  `core:types/option`/`core:types/result` (`Some`, `None`, `Ok`, `Err`).
- `core:errors` — `Error` y su jerarquía de plataforma (`TypeError`,
  `RangeError`, `IntegerOverflow`, `DivisionByZero`, `MatchError`).
- `core:globals` — `print`, `debug`, `assert`, `assertSummary`, `input`,
  `NaN`, `Infinity`, `isIsolate`, `console`.
- `core:capabilities` — `Equatable`, `Comparable`, `Hashable`, `Cloneable`,
  `Default`, `Display`, `Debug`, `Indexable`, `Add`…`Neg`, `Disposable`,
  `AsyncDisposable`.

El id de un módulo builtin es su lugar en el árbol:
`crates/varn-builtins/src/modules/<capa>/<ruta>/` con un único contrato `.vn`
es `<capa>:<ruta>`. No hay manifiesto (`module.json`) que lo repita. Todo
`core:*` está en scope; que `core:types` agrupe varios módulos es
organización, no una fachada que reexporta.

`core:*` puede contener código Varn además de declaraciones nativas
(`ModuleSpec::has_code`, derivado del contrato). Esos módulos forman el
**prelude**: la bajada a TIR de todo módulo fuera de `core:` añade, como
imports que ningún texto escribe, los nombres del prelude que el módulo usa
como valor (una declaración local los sombrea). Llegan a slots de módulo: un
uso es una carga de slot, nunca una búsqueda por nombre; un módulo que no
nombra ninguno no carga nada. `Option`/`Result` son el primer caso.

`Symbol` se borra del lenguaje: sin iteración por símbolos (ADR-0017) no
tiene uso en el núcleo.

Un tipo del núcleo se identifica por su origen (`core:types`), nunca por su
texto: un `Result` declarado por el usuario no es el del núcleo.

## Migración (cada paso, un commit verde)
1. Una sola regla de capas para binder, loader y bundle
   (`varn_modules::layer::check_import`).
2. Borrar `Symbol`.
3. Reorganizar `core:*` y `runtime:*` por ruta; sin `module.json`.
4. `core:*` con código Varn compilado; `Option`/`Result` a
   `core:types/{option,result}` con métodos (`isSome`, `unwrap`, …,
   `Result.catching`); `std:result` desaparece; `try` reconoce el
   `Option`/`Result` del núcleo por origen (`varn_core::CoreSum`), nunca por
   texto.

Los cuatro pasos están hechos.
