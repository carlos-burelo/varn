# Fase C — Tipo del lenguaje ≠ clasificación runtime

> Cada paso compila, deja `main.vn` verde en JIT y `VARN_NO_JIT=1` (caché
> limpio), corpus negativo y goldens verdes, y es un commit. Spec §13, §20,
> §37–§40, §100, §103; decisión D2 (ADR-0015).

**Goal:** `TypeTag` deja de ser a la vez tipo del lenguaje, nombre de clase
de plataforma y clasificación de valores. Queda:
- `LangPrimitive` (varn-core): los primitivos del lenguaje —
  `null bool int float bigint decimal char str` + `void never dynamic`.
- `TypeKind::Primitive(LangPrimitive)` en el checker, y los contenedores de
  plataforma sin argumentos (`Bytes`, `Range`, …) como tipos nombrados.
- `RuntimeKind` (varn-core, antes `TypeTag`): solo clasifica valores en
  runtime (`Null Bool Int Float BigInt Decimal Char Str Array Map Set Tuple
  Object Class Function Enum Task Generator Range Bytes Opaque`).
- Nombres de clases de plataforma (`Error`, `TypeError`, …) como constantes de
  `well_known`, no como variantes de un enum de tipos.

**Situación medida (2026-09-23):** `TypeTag` 489 referencias (checker 224,
core 108, types 62, vm 48); `TypeKind::Intrinsic(` 156 en el checker; tags
usados como tipo del lenguaje: los 11 primitivos + `Symbol`, `Bytes`, `Map`,
`Range`, `Set`, `Array`.

## Pasos

### C.1 Tags sin valor runtime ni tipo propio
Borrar `NativeFn`, `Regex`, `DateTime`, `Duration`, `UUID`, `Span`,
`TypedArray` (0–2 usos, solo nombre); `Error`/`TypeError`/`RangeError` pasan
a `varn_core::well_known::{ERROR, TYPE_ERROR, RANGE_ERROR}` (el VM los usa
como nombre de clase intrínseca). `VmRef` → `Opaque` (payload de interop).

### C.2 `Symbol` sale del núcleo (D2)
`TypeTag::Symbol`, `CheckerTyId::SYMBOL`, `IntrinsicType::Symbol` y el
primitivo `symbol` se borran del sistema de tipos; el valor runtime y su clase
siguen existiendo como tipo nombrado de plataforma `Symbol`.

### C.3 `LangPrimitive` y `TypeKind::Primitive`
`TypeKind::Intrinsic(TypeTag)` → `TypeKind::Primitive(LangPrimitive)`; los usos
de `Intrinsic(Bytes|Map|Set|Range|Array)` pasan a tipos nombrados de
plataforma. `IntrinsicType` se reduce a la tabla nombre↔`LangPrimitive` más
los nombres de clases de plataforma.

### C.4 `TypeTag` → `RuntimeKind`
Renombre y poda: sin `Void`, `Never`, `Dynamic` (no son valores). Consumidores:
`HeapObj::kind()`, `Value::kind()`, `FieldLayout`, JIT de campos.

### C.5 Layout desde `BackendTy`
`FieldRepr` se deriva de `BackendTy` (`layout_of`) en un único sitio; paso
previo a `TypeLayout` (Fase F).

### C.6 Docs
`docs/TIR_CONTRATO_TIPADO.md`, `docs/ARCHITECTURE.md`, roadmap §0.

## Ejecución (2026-09-23)

| Paso | Commit | Nota |
|---|---|---|
| C.1 | `0d96f3a3` | tags solo-nombre borrados; `Error`/`TypeError`/`RangeError` en `well_known` |
| C.2 | `ae71e4d4` | `Symbol` fuera del núcleo |
| C.3 | `d49ea6a7`, `5f0f86b8` | `LangPrimitive` + `BuiltinType`; `IntrinsicType` borrado entero (no reducido) |
| — | `67640b9b` | código muerto: `typed_ir`, `CgTy`, `to_type_tag`, `runtime_tag` |
| C.4 | `dab858a1` | `RuntimeKind`; `VmRef` → `Opaque`; campos `Option<RuntimeKind>`; `FieldAccess` |
| C.5 | `3d358333` | una tabla de layout (`class_field_repr`); `from_tir::field_kind` |

Desviación: `Bytes/Map/Set/Range/Array/Task/TaskHandle/Generator` sin
argumentos son `TypeKind::Builtin(BuiltinType)`, no tipos nombrados — el
checker los compara estructuralmente y convertirlos a `Named` exigía resolver
su clase en cada comparación.
