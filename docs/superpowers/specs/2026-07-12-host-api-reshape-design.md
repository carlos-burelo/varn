# Host API Reshape — Design

**Fecha:** 2026-07-12
**Estado:** aprobado (diseño), pendiente de implementación
**Predecesor:** [2026-07-09-stdlib-package-system-design.md](2026-07-09-stdlib-package-system-design.md) — este spec cierra su "future work" §2.
**Secuencia:** se implementa DESPUÉS de que `feat/isolate-channels` aterrice en `main` (el WIP de channels toca `task.vn`/`task_runtime.vn`/`task.rs`, el módulo más afectado por este reshape). Rama nueva desde `main`.

## 1. Objetivo

Cerrar la migración de la stdlib al 100%: eliminar los 4 módulos deferred del
`MODULE_REGISTRY`, renombrar los natives `runtime:*` a nombres públicos, matar
wrappers triviales vía re-exports, y dejar una taxonomía uniforme de tres
capas sin rutas duales.

**En alcance:**

- Migrar `std:types`, `std:collections`, `std:reflect`, `std:task` al árbol `std/`.
- Nuevo módulo embebido contract-only `core:intrinsics`.
- Renombrar ~67 natives en 11 módulos host + `runtime:task`/`runtime:reflect` (convención sin prefijo).
- API pública `std:*` plana (muere `export namespace` como fachada).
- Bump `HOST_API_VERSION` 2 → 3, `std.json` 0.2.0 → 0.3.0 (isolate-channels ya tomó v2/0.2.0).
- Limpieza: ids muertos (`core:collections`, `core:reflect`, `core:async`, `core:iterators`), constantes `CORE_*` huérfanas, `DEFERRED_STDLIB_IDS`, fallback stdlib del provider.

**Fuera de alcance:**

- Resource classes nativas (`File`, `Socket`, `Process`) — proyecto propio posterior.
- Soporte de re-export directo dentro de namespaces (`export namespace fs { export { read } from … }`) — **TODO anotado**: feature de lenguaje futura; cuando exista, los módulos std podrán ofrecer fachada namespace sin wrappers.
- Enforcement real de capabilities (`has_capability()` sigue retornando `true`).

## 2. Taxonomía final

```
crates/varn-builtins/src/modules/
  globals/          core:global                      (sin cambio)
  primitives/       core:int, core:array, …          (sin cambio)
  intrinsics/       core:intrinsics                  ← NUEVO, contract-only
  host/             runtime:{crypto,fs,http,io,json,math,net,path,sys,testing,time}
                    host/task/    runtime:task       ← movido desde modules/std/task/runtime/
                    host/reflect/ runtime:reflect    ← movido desde modules/std/reflect/runtime/
std/                16 módulos: crypto dispose fs http io json math net path sys
                    test time + task collections reflect types
```

Invariantes post-reshape:

1. `MODULE_REGISTRY` solo contiene kinds `core` | `runtime`. `build.rs` de
   `varn-builtins` hace panic ante cualquier `module.json` con
   `"kind": "stdlib"` (la lista `DEFERRED_STDLIB_IDS` se elimina).
2. `runtime:*` es la única frontera con Rust. Ningún `varn_contract!` liga a
   un id `std:*` (hoy `collections.rs` viola esto — se corrige aquí).
3. `core:*` tipa el lenguaje, no es importable por código de usuario (regla
   existente del binder), y funciona sin bundle presente.
4. `std:*` resuelve exclusivamente vía std activa (bundle/árbol). El fallback
   del provider al registro embebido para ids stdlib se elimina.
5. Un `module.json` = un id. Los manifests multi-id desaparecen.

## 3. `core:intrinsics` (nuevo)

Módulo embebido contract-only en `modules/intrinsics/`, kind `core`. Contiene
las declaraciones que el checker asocia por nombre/tag a tipos intrínsecos y
que hoy viven dentro de `task.vn`:

- `declare class Generator<T>` / `declare class AsyncGenerator<T>`
- `interface Iterator<T>` / `interface AsyncIterator<T>`
- `interface TaskHandle<T>`

Mismo mecanismo que ya usan los primitives (`core:array` declara los miembros
de `Array<T>`): cero maquinaria nueva en el checker. Verificado: los ids
`core:async`/`core:iterators` no tienen ningún consumidor por id en Rust ni en
`.vn` — el checker resuelve `Generator`/`Task`/`TaskHandle` por
`IntrinsicType`/`TypeTag` y por nombre (`checker/stmts.rs`, matching
estructural de `"Iterator"`/`"Generator"`), no cargando esos módulos.

`std/task.vn` re-exporta los tipos para el usuario:
`import { Generator, … } from "core:intrinsics"` + `export { … }` — el binder
ya permite imports `core:` desde contexto `std:`. El gate de
`xtask build-std` (`validate_imports`: hoy solo `runtime:*`/`std:*`) se
extiende para permitir `core:intrinsics` (solo ese id, no `core:*` general).

## 4. Migración por módulo

| Módulo | Acción |
|---|---|
| `std:types` | `types.vn` → `std/types.vn` tal cual (utility types puros, sin `.rs`). `pure: true`. |
| `std:collections` | → `std/collections.vn` 100% puro. `List`/`Stack`/`Queue` ya tienen cuerpos `.vn` completos (los natives de `collections.rs` son overrides de perf redundantes). `Record<K,V>` (hoy declare-only, implementado solo en Rust sobre `MapRef`) se reescribe puro sobre `Map<K,V>` intrínseco. `collections.rs` se borra. `pure: true`. |
| `std:reflect` | `runtime/` → `modules/host/reflect/` (natives ya ligan a `runtime:reflect`, solo se mueve). Fachada `reflect.vn` → `std/reflect.vn`. |
| `std:task` | Split de 3 vías: declares intrínsecos → `core:intrinsics`; fachada (`spawn`, `sleep`, `parallel`, `TaskGroup`, re-exports de channel/Sender/Receiver/etc.) → `std/task.vn`; `runtime/` → `modules/host/task/`. |

**Gate de perf (collections):** bench List/Stack/Queue antes/después de borrar
los natives, workload documentado en el plan. Si la regresión es inaceptable,
fallback documentado: free functions en un nuevo `runtime:collections`
(frontera limpia, sin natives ligados a `std:*`). No se asume resultado —
se mide.

## 5. Renames de natives

Convención: **sin prefijo, camelCase**; el id del módulo califica.

| Antes | Después | Módulo |
|---|---|---|
| `fsRead`, `fsWrite`, `fsStat`, `fsReadDir` | `read`, `write`, `stat`, `readDir` | `runtime:fs` |
| `mathAbs`, `mathSqrt`, `mathSin` | `abs`, `sqrt`, `sin` | `runtime:math` |
| `timeNowMs`, `timeMsToParts` | `nowMs`, `msToParts` | `runtime:time` |
| `taskSpawn`, `taskSleep`, `taskParallel` | `spawn`, `sleep`, `parallel` | `runtime:task` |

(Inventario completo por módulo — ~67 natives en 13 contratos — se enumera en
el plan de implementación; la tabla anterior fija la convención.)

El rename se hace en el contrato `.vn` (fuente de verdad) y `varn_contract!`
propaga: cualquier drift entre contrato y Rust es error de compilación —
mecanismo existente, sin herramienta nueva. Colisiones al importar múltiples
runtime en un mismo `std/*.vn` se resuelven con alias en el import
(`import { read as ioRead } from "runtime:io"`).

Los tipos exportados por contratos runtime (`RawStats`, `RuntimeTimeParts`, …)
conservan su nombre — solo funciones renombradas.

## 6. API pública plana y muerte de wrappers

- `std/*.vn` exporta plano, estilo Deno/Node ESM: `import { read } from "std:fs"`.
- Wrapper sin valor agregado → re-export directo del native
  (`import { read } from "runtime:fs"; export { read };` — mecanismo ya usado
  en `task.vn`). Wrapper con valor (composición, validación, clases como
  `Stats`) vive.
- `export namespace fs { … }` como fachada muere → **breaking user-facing**:
  `fs.read(x)` pasa a `read(x)`. Los `tests/*.vn` afectados se actualizan en
  el plan.

## 7. Versionado y compatibilidad

- `HOST_API_VERSION` 2 → 3 (renames = breaking en contratos `runtime:*`).
- `std.json` `version` 0.2.0 → 0.3.0.
- Bundle viejo contra binario nuevo (o viceversa): rechazo duro existente
  (`validate_compat_with`) con hint `cargo xtask build-std`. Sin fallback
  silencioso.
- Dev flow intacto: `VARN_STD=std` (árbol fuente) vía `.cargo/config.toml`.
- Sin capa de compatibilidad ni alias de nombres viejos — breaking controlado
  por rama + validación, según `evolution_strategy` del proyecto.

## 8. Limpieza asociada

- `varn-modules/src/lib.rs`: eliminar `CORE_COLLECTIONS`, `CORE_ASYNC`,
  `CORE_ITERATORS`, `CORE_REFLECT` (constantes sin consumidores).
- `provider_impl.rs`: sin cambios estructurales (el orden std-activa-primero
  ya es correcto); verificar que ningún id stdlib quede servible desde
  `MODULE_REGISTRY`.
- `build.rs`: gate absoluto anti-`"stdlib"`.
- Auditar usos de prefijo `core:` en LSP/debug que asuman los ids muertos.

## 9. Validación (gates del plan)

1. `cargo test` del workspace.
2. Suite completa `tests/*.vn` en **ambos modos**: árbol (`VARN_STD=std`) y
   bundle (`cargo xtask build-std` + `VARN_STD=target/std.vnb`).
3. Bench collections antes/después (workload y config documentados).
4. LSP: completions listan los 16 módulos `std:*` (ruta `combined_specs`).
5. `vn doctor` reporta provenance correcta en ambos modos.
6. Programa sin imports que usa `function*`/`async` tipa y corre sin bundle
   presente (invariante 3).

## 10. Docs a actualizar

- `STDLIB_ARCHITECTURE.md` — tabla de niveles, muere el párrafo "Deferred".
- `LBI_ARCHITECTURE.md` — ubicaciones post-movimiento.
- `HOST_BOUNDARY_SPEC.md` — convención de naming de natives.
- `CRATES_STATE.md` — estado de `varn-builtins`.

## 11. Riesgos

| Riesgo | Mitigación |
|---|---|
| Perf de collections sin natives | Gate de bench + fallback `runtime:collections` documentado (§4) |
| Checker dependía de algún efecto no descubierto de los ids `core:*` muertos | Gate 6 de validación (generators/async sin bundle) + suite completa |
| Renames rompen `.vn` internos no auditados | `varn_contract!` + checker fallan en build — drift imposible en silencio |
| Conflicto con WIP de channels | Secuencia obligatoria: channels aterriza primero; rama limpia desde `main` |
