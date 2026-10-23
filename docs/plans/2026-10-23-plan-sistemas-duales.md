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
