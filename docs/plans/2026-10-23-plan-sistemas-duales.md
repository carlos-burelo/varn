# Plan: eliminar sistemas duales antes de la release pública (living, 2026-10-23)

Origen: auditoría de código (Ley 12) del 2026-10-23. Cada item declara
ganancia, verificación y borrado (Ley 10). Un commit atómico por item (Ley 9).
Orden: capa más baja primero (Ley 14). Validación por item:
`.\scripts\verify.ps1 -Quick`; matriz completa (`-Fast`) antes de cerrar cada fase.

## Fase A — cimientos del compilador

### A1. Borrar `HirType`; SSA consume `BackendTy`
- Hallazgo: `varn-compiler/src/hir` define un segundo sistema de tipos
  (`HirType`, `TyId`, `ClassId`, `TyTable`) y `from_tir/ty.rs` lo degrada:
  `Decimal/BigInt/Void/Never/Dynamic → Dynamic`, `Char/Bytes/Enum/Fn → Ref`.
  `class_id` deduplica por nombre sin origen (Ley 16).
- Ganancia: un tipo del checker llega entero a SSA/regalloc/JIT; desaparece la
  clase de bugs "tipo re-derivado"; identidad de clase deja de ser por nombre.
- Verificación: `tests/main.vn` 4 cuadrantes sin cambios de resultado; bytecode
  determinista (dos corridas idénticas); `vn debug -p bails` no empeora.
- Se borra: `hir/mod.rs`, `from_tir/ty.rs`, `TyTable` duplicada, las
  conversiones `lower::*` que dependan de `HirType`.

### A2. Vaciar `DynReason::NotYetSupported`
- Hallazgo: 79 sitios (casi todos en `varn-emit`) degradan un tipo conocido.
- Ganancia: menos `Dynamic`, menos guards `verify_at_runtime`, menos rutas por
  nombre en VM/JIT.
- Verificación: `Coverage::of` sobre `tests/` y `std/` antes/después; la cuenta
  de `NotYetSupported` llega a 0 y la variante se borra.
- Se borra: la variante y los sitios; cada sitio pasa a su tipo real o a una
  razón con nombre (`HostBoundary`, `Union`, …).
- Línea base (2026-10-23, `vn debug -p tir:check` sobre `tests/*.vn`):
  nodos dinámicos por razón: función 9491, `NotYetSupported` 1634, task 1327,
  declared 1105, object 447, union 294, generator 89.
- Sitios más calientes (llamadas a `NotYetSupported` por `vn check tests/*.vn`):
  `ty/lower.rs:23` (`opaque()`), `body/scope.rs:46-47`, `module_globals.rs:109`,
  `body/stmt_decl.rs:258`, `body/expr_closure.rs:28,37,167`, `functions.rs:216-217`.
- Causas dentro de `opaque()`: parámetros de tipo genéricos (`T`,`A`,`B`: ~2200),
  interfaces y alias no objeto (`HasId`, `Printable`, `Options`, `Profile`, `Link`),
  y tipos de nivel de tipo sin resolver (`This`, `Intersection`, `TemplateLiteral`).
- Orden: (1) razón propia para parámetros de tipo borrados; (2) alias/interfaces
  no objeto se bajan por su destino con guarda de recursión; (3) defaults de
  `scope.rs`/`closure` se sustituyen por el tipo del checker; (4) borrar la variante.
- Hecho: parámetros de tipo borrados llevan `DynReason::Generic` (NYS 1634 -> 1438;
  type parameter 196). El resto de NYS son fallbacks por falta de dato del checker:
  `expr_ty` sin entrada en `expr_table`, símbolos global sin `ty`, `this` fuera de clase,
  y defaults de `scope.rs` que luego se sobrescriben. Raíz: el checker no registra el
  tipo de toda expresión; arreglarlo ahí, no en emit (Ley 14).
- Hecho (2026-10-23): funciones anidadas, closures prebound y resultados de
  decoradores llevan su tipo; los bindings sin tipo de origen leen el tipo del símbolo
  del checker (`declared_types`, indexado por offset de declaración). NYS 1438 -> 1321.
- Hallazgo de raíz: las clases core (`Error`, `RangeError`, ...) no tienen
  `BackendTy`: `resolve_type_ref` no las encuentra porque `collect_foreign_classes`
  excluye builtins, así que `catch (e)` y todo valor `Error` caen en NYS. Siguiente
  paso real de A2: representar las clases core en la tabla de clases de TIR.
- Trampa medida: `guard_dynamic` sobre el valor de `for await (const msg of rx)`
  cuelga `tests/47`; el binding tipado va sin guarda.
- Aparte: `Dynamic(Callable)` aparece 9491 veces; un `Fn(SigId)` estático es el
  mayor candidato a ganancia de rendimiento tras A2.

### A3. Inferencia solo en el checker
- Hallazgo: binder (`type_infer_*`) y checker (`infer_*`) infieren expresiones.
- Ganancia: un motor; el binder deja de conocer tipos semánticos (AGENTS §3).
- Verificación: suite 4 cuadrantes; diagnósticos del LSP sin regresión.
- Se borra: `type_infer_expr/ops/member`, `inference_utils` (lo que sobreviva
  se muda al checker).

## Fase B — módulos

### B1. Un solo cargador de módulos
- Hallazgo: `varn-resolver` + `varn-modules` + `varn-pipeline/resolver.rs`
  (`Session`) + `stdlib_loader/layer_link/portable`.
- Ganancia: una identidad, una resolución, una carga, una caché (Ley 8).
- Verificación: 4 cuadrantes con `VARN_CACHE_DIR` limpio.
- Se borra: `Session`, `resolver_embed/disk` duplicados, caminos paralelos.

## Fase C — runtime

### C1. Rutas dinámicas por nombre
- Tras A1/A2, medir cuáles de `GetProperty`, `LoadGlobal`, `index_instance_fallback`,
  `fallback_method_call` siguen alcanzables (bails/tiers) y borrar las muertas.
  Las que sobrevivan se justifican como `dynamic` declarado (Ley 17).

## Fase D — higiene y release

### D1. Flag muerto `FRAME_LAYOUT_V2_JIT_BAIL`
- Ganancia: menos ramas muertas en el camino de llamada. Se borra el const y sus ramas.

### D2. Modelo de permisos único en `vn run`
- Hallazgo: sin flags = `allow_all`; con un `--allow-*` pasa a restrictivo.
- Decisión del dueño requerida (cambia el comportamiento por defecto).

### D3. Tests de cargo (Ley 11)
- Mover lo que valga a `tests/*.vn`; borrar `crates/*/tests`.

### D4. Dogfooding de usabilidad
- Escribir 5 programas `.vn` reales; registrar fricciones; priorizar.

## Estado
- [x] D1  - [x] A1  - [ ] A2  - [ ] A3  - [ ] B1  - [ ] C1  - [ ] D2  - [ ] D3  - [ ] D4
