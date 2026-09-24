# Desviación forzada NEW_SPEC — Roadmap maestro

> **Para agentes:** este documento es el **mapa**. No se ejecuta directamente.
> Cada fase se ejecuta desde su propio plan detallado (checkbox, TDD, commits
> atómicos). La Fase 0 y la Fase A ya tienen plan detallado:
> `docs/plans/2026-09-23-fase-0-A-semantica-numerica.md`.
> Los planes de las fases B–L se escriben (con `superpowers:writing-plans`)
> **al cerrar la fase anterior**, porque cada fase cambia el código que la
> siguiente toca; escribir hoy su código paso a paso produciría pasos obsoletos.

**Goal:** llevar el lenguaje, el compilador, el runtime y la stdlib de Varn al
modelo de `NEW_SPEC.md` (tipos del lenguaje ≠ tipo semántico ≠ representación
física), borrando cada camino que lo contradiga.

**Architecture:** la desviación se hace de adentro hacia afuera: primero la
semántica observable del núcleo (números, conversiones, errores), después el
modelo de tipos del compilador (`TypeTag` → `RuntimeKind`, tipos del lenguaje
propios), después la superficie del sistema de tipos, después layout y
ejecución tipada, y al final la plataforma estándar. Cada fase deja `main.vn`
verde en los 4 cuadrantes y borra lo que reemplaza (Ley 8).

**Tech Stack:** Rust (workspace `crates/*`), Cranelift (JIT), corpus `.vn`
(`tests/main.vn`, `tests/errors/`), `std/` embebida vía `varn-cli/build.rs`.

**Spec:** `NEW_SPEC.md` (se mueve a `docs/lang/SPEC_NUCLEO_Y_PLATAFORMA.md` en
la Tarea 0.1). Se lee junto con `AGENTS.md` (Leyes 1–10) y
`docs/plans/2026-09-20-PLAN-PENDIENTE.md` (JIT desde SSA).

## Global Constraints

- **Regla rectora del spec (§108):** "No agregues un tipo al lenguaje para
  resolver un problema de representación. Agrega una representación al
  compilador."
- **Tipos numéricos públicos: exactamente** `int`, `float`, `bigint`, `decimal` (§2).
- `int` = signed 64-bit two's complement, rango `-2^63 .. 2^63-1`, operaciones
  **checked**; overflow lanza `IntegerOverflow`; sin wrapping silencioso, sin
  promoción a `bigint`, sin conversión a `float` (§2.1).
- `float` = IEEE 754 binary64; respeta `+0`, `-0`, `NaN`, `±Infinity`,
  subnormales y redondeo (§4).
- Conversiones implícitas permitidas: **solo** `int → bigint` e `int → decimal`
  (exactas). Todas las demás requieren `as` (§9).
- `int / int → int`, `float / float → float`, `decimal / decimal → decimal`,
  `bigint / bigint → bigint` (§10).
- Se eliminan del lenguaje: `double`, `number`, `real`, `i8`, `i16`, `i32`,
  `i64`, `u8`, `u16`, `u32`, `u64`, `f32`, `f64` (§104).
- `TypeTag` deja de ser autoridad del sistema de tipos; solo clasifica valores
  runtime (§38, §40, §100).
- `Record<K,V>` prohibido; `Map<K,V>` ≠ `{ [key: K]: V }` (§22, §24, §104).
- `dynamic` se conserva; no se introduce `unknown` (§43).
- Leyes de `AGENTS.md` obligatorias en cada tarea; en particular **Ley 8**
  (borrar, no puentear), **Ley 9** (un commit por cambio), **regla de 400
  líneas** por archivo.
- Puerta de validación por commit (sección 6 de este documento).
- No ejecutar `git` sin autorización explícita del usuario (`AGENTS.md` §8,
  `CONTRIBUTING.md`). Los pasos "Commit" de los planes se ejecutan solo con esa
  autorización vigente.

---

## 0. Estado (2026-09-23, rama `new-spec-fase-0-A`)

**Fase 0, A, B, C, D y E cerradas** (Fase D: `docs/plans/2026-09-23-fase-D-superficie-tipos.md`; Fase E: `docs/plans/2026-09-24-fase-E-capacidades.md`; cada una con su deuda) (Fase B: ADR-0016,
`docs/plans/2026-09-23-fase-B-precision-arbitraria.md`; Fase C:
`docs/plans/2026-09-23-fase-C-runtimekind.md`).

Fase C: `TypeKind::Intrinsic(TypeTag)` → `Primitive(LangPrimitive)` /
`Builtin(BuiltinType)`; `Symbol` es clase de plataforma; `IntrinsicType`,
`typed_ir`, `CgTy` borrados; `TypeTag` → `RuntimeKind` sin
`Void/Never/Dynamic`; campos de clase con `Option<RuntimeKind>` y una sola
tabla de layout. Desviación del plan: los contenedores sin argumentos quedaron
como `BuiltinType` en vez de tipos nombrados (siguen necesitando identidad
estructural en el checker).

Hallazgos de Fase B, corregidos: la aritmética `bigint` no existía (`10n + 5n`
daba `0`); el widening implícito `int → bigint/decimal` no convertía el valor;
`dynamic as float` sobre `bigint`/`decimal` daba `0.0`; el AST HIR muerto se
borró. Pendiente anotado: ~~el contrato `Map<V>` fija las claves en `str`~~ — resuelto en D.6; un campo de clase `bigint`
asignado desde `int` no pasa por el widening (solo locales, globales,
retornos, parámetros).

**Fase 0 y Fase A cerradas.** `tests/main.vn` 1274/0 en JIT y `VARN_NO_JIT=1`;
`cargo test -p varn-cli --test error_corpus` verde.

Hallazgos fuera del plan, corregidos en la rama: `from_ssa` JIT con
self-call frame-aware rompía `main.vn` en HEAD; `class X extends Y` en el
módulo de entrada tumbaba el checker (atom de otro interner); `NaN` y
`Infinity` globales valían `null`; `VmValue::from_f64(NaN)` daba `null`; el
pool de constantes fusionaba `-0.0` con `0.0`; `check_misc.rs` huérfano.

Brechas nuevas anotadas para fases siguientes:
- ~~Tipo tupla `#[T1, T2]` no parsea en posición de tipo~~ — resuelto en D.2.
- No existe diagnóstico "tipo desconocido": `let x: Foo = 1` da VN3001
  (Fase D).
- `docs/TIR_CONTRATO_TIPADO.md` aún describe anchos angostos; el usuario
  tiene cambios locales sin commitear en ese archivo — actualizar al
  integrarlos.
- LSP (`varn-lsp`) no compila en HEAD (imports `Expr`/`Stmt` obsoletos); fuera
  de la puerta.
- Conversiones distintas de `int → float` declinan el JIT desde SSA y el
  bytecode JIT no conoce `OpCode::Convert`: funciones con `as int` quedan en
  el intérprete (medir en Fase G).

---

## 1. Decisiones (se fijan en ADR-0015, Tarea 0.1)

Cerradas con el usuario el 2026-09-23:

| # | Decisión | Fuente |
|---|---|---|
| D1 | `for (const x of expr)` se mantiene. Los `for i in ...` del spec son ilustrativos. | usuario |
| D2 | `Symbol` sale del núcleo: deja de ser `TypeTag`/intrínseco; si sobrevive, es clase de plataforma. | usuario |
| D3 | `float as int`: trunca hacia cero; `NaN`, `±Inf` o fuera de `[-2^63, 2^63)` lanzan `IntegerOverflow`. | usuario |

Fijadas por este plan (el spec deja margen; se elige la opción que no pierde
información en silencio):

| # | Decisión | Por qué |
|---|---|---|
| D4 | `int → bigint` e `int → decimal` son implícitas. | Exactas por construcción; §7 y §9 lo permiten. Ya funciona hoy. |
| D5 | `float → decimal`, `decimal → float`, `int → float` dejan de ser implícitas. | §9. Hoy `compat/mod.rs:71-100` las acepta. |
| D6 | Literales enteros adoptan el tipo numérico del contexto si son **exactamente** representables: `let x: float = 5`, `f * 2`, `d + 1`. Un literal de `float` nunca adopta `int`. | §5 último párrafo. Sin esto, `x * 2.0` en todo el corpus. |
| D7 | Con `dynamic`, la aritmética runtime conserva la promoción `int ⊕ float → float` (es la frontera dinámica, §99), **pero** `int / int` da `int` también en dinámico. | Un programa aceptado por el checker nunca da distinto resultado anotado o sin anotar; el checker rechaza las mezclas estáticas. |
| D8 | `int % int`: signo del dividendo (truncado, como hoy). `i64::MIN % -1 == 0` (exacto, no lanza). `int / 0` y `int % 0` lanzan `DivisionByZero`. `i64::MIN / -1` lanza `IntegerOverflow`. | §10 exige documentar los tres casos. |
| D9 | `float / 0.0` y `float % 0.0` siguen IEEE (`±Inf`/`NaN`), **no** lanzan. | §4. Hoy lanzan (`arith.rs:151-155`, `:179-183`). |
| D10 | Los tipos de error runtime `IntegerOverflow` y `DivisionByZero` son clases de plataforma (`extends Error`), no `TypeTag`. | §41. |
| D11 | El almacenamiento angosto de arrays (`ArrayRepr::I8..F32`) se borra en Fase A y se reintroduce en Fase F solo como optimización derivada de análisis de rango. | §12: la representación la decide el compilador, no un tipo público. Ley 8. |
| D12 | `bigint` pasa a precisión arbitraria (hoy `i128`) y `decimal` a precisión arbitraria (hoy `rust_decimal`, 28 dígitos). La política de redondeo de `decimal /` se fija en el ADR de Fase B. | §6, §8. |
| D13 | Las operaciones enteras especializadas viven como métodos de `int`: `wrapping{Add,Sub,Mul}`, `saturating{Add,Sub,Mul}`, `checked{Add,Sub,Mul}` (→ `int?`), `div`, `floorDiv`, `ceilDiv`, `rem`, `mod`. | §3, §10, §61. |

---

## 2. Matriz de brechas (spec § → estado actual → fase)

Leyenda: ✅ cumple · ⚠️ parcial / con bug · ❌ ausente o contradice.
Evidencia verificada el 2026-09-23 contra `target/release/vn.exe` de `3dbe44cb`.

### 2.1 Números (§2–§12, §61, §93)

| § | Requisito | Estado | Evidencia | Fase |
|---|---|---|---|---|
| 2 | Solo 4 numéricos públicos | ❌ | `TypeTag::{I8..U64,F32}` (`varn-core/src/type_tag.rs:36-43`), `IntrinsicType::from_str` acepta `"i8"`… (`intrinsics.rs:85-92`); tests 107/108/116 | A |
| 2.1 | `int` checked | ⚠️ | `+ - *` checked (test 102). **Bugs:** `i64::MIN % -1` tumba el proceso (`ops_math_cmp.rs:295` panic "remainder with overflow"); `abs(MIN)` y `negate(MIN)` envuelven (`primitives/int/int.rs:41,43`); DCE elimina una `*` que desborda si su resultado no se usa (repro en Tarea A.3) | A |
| 2.1 | Error se llama `IntegerOverflow` | ❌ | Todo error runtime es `Error` genérico con mensaje (`exceptions.rs:100-113`) | A |
| 3 | `wrappingAdd` etc. | ❌ | `property 'wrappingAdd' does not exist on type 'int'` | A |
| 4 | `float` IEEE | ⚠️ | `1.0 / 0.0` lanza `division by zero` (`arith.rs:151-155`) | A |
| 5 | `int → float` explícito | ❌ | Implícito en asignación (`compat/mod.rs:71-84`) y en aritmética mixta (`numeric.rs:95`); test 26 lo celebra | A |
| 5 | `x as float` convierte | ⚠️ | Hack `v + 0.0` vía `Add` genérico (`from_tir/build.rs:797-833`) | A |
| 6 | `bigint` precisión arbitraria | ❌ | `Value::BigInt(Box<i128>)` (`varn-types/src/value/mod.rs:116`), `TirExprKind::BigIntLit(i128)` | B |
| 7 | `bigint as int` con chequeo | ❌ | `cannot store 'reference' in an int register` (`Cast` = `Move`, `ssa/emit/values.rs:240-245`) | A |
| 8 | `decimal` precisión arbitraria | ❌ | `rust_decimal` (96 bits de mantisa) | B |
| 9 | Tabla de conversiones | ⚠️ | `decimal ← float` implícito (`compat/mod.rs:97-100`); `decimal + float` pasa el checker (resultado `Dynamic`) | A |
| 10 | `int / int → int` | ❌ | `5 / 2` imprime `2.5` (`numeric.rs:104-109`, `ops_math_cmp.rs:270`) | A |
| 10 | `div/floorDiv/ceilDiv/rem/mod` | ❌ | ausentes | A |
| 11–12 | anchos solo en backend | ❌ | `BackendTy::{Int8..Float32}` (`varn-tir/src/ty.rs:83-89`), `NarrowRangeCheck`, `CheckNarrowRange`, `narrow_range.rs` | A |
| 61 | API checked/wrapping/saturating | ❌ | ausente | A |
| 93 | overflow analysis / range analysis | ❌ | no existe análisis de rango | L |

### 2.2 Primitivos, strings, bytes, vistas (§13–§18, §54)

| § | Requisito | Estado | Evidencia | Fase |
|---|---|---|---|---|
| 13 | Primitivos `null bool int float bigint decimal char str` + `void never dynamic` | ⚠️ | `Symbol` es primitivo (`type_tag.rs:13`, `is_primitive`) → D2 | C |
| 14 | `char` = Unicode scalar | ✅ | test 34 | — |
| 15 | `str` UTF-8 inmutable | ✅ | — | — |
| 16 | `StringView` | ❌ | ausente | J |
| 17 | `Bytes` sin `u8` público | ⚠️ | `Bytes` existe; `u8` público existe (se borra en A) | A/J |
| 18, 54 | `Span<T>` vista | ❌ | `TypeTag::Span` existe pero sin semántica (1 ref) | J |

### 2.3 Colecciones y estructuras (§19–§25, §45–§46, §53)

| § | Requisito | Estado | Evidencia | Fase |
|---|---|---|---|---|
| 19 | `T[] ≡ Array<T>` | ✅ | 214 usos de `T[]` en corpus | — |
| 20 | `TypedArray` no público | ❌ | `TypeTag::TypedArray`, `IntrinsicType::TypedArray` | C |
| 21 | Tuplas `#[...]` | ❌ **crash** | `let t = #[1,"a",true]; t[1]` → panic `frame_store.rs:202` (index = len) en `ExecCtx::new` | 0 |
| 22 | Records `#{}` igualdad estructural | ✅ | `r == s` → `true` | — |
| 22 | `Record<K,V>` prohibido | ⚠️ | se rechaza por mismatch accidental, no por regla | D |
| 23, 53 | Object ≠ Record ≠ Class | ⚠️ | falta regla de igualdad/identidad documentada y probada | D |
| 24 | `Map<K,V>` ≠ `{[k:K]:V}` | ❌ | `let m: Map<str,int> = o` (o index-signature) se acepta (`compat/mod.rs:570-740`) | D |
| 24 | `new Map()` infiere por contexto | ❌ | `let m: Map<str,int> = new Map()` → `Map<dynamic>` mismatch (`type_inference.rs:461-463`) | D |
| 25 | `Set` por `Hashable/Equatable` | ❌ | `MapKey(VmValue)` hashea bits del valor (`value/map.rs:10-17`) | E |
| 45 | `Range<T>` genérico con step | ⚠️ | `Range` no genérico; `step` devuelve array | D |
| 46 | Rango sin heap en `for` | ⚠️ | verificar en Fase G con `vn debug -p gc` | G |

### 2.4 Álgebra de tipos (§26–§31, §35–§37, §39, §42–§43)

| § | Requisito | Estado | Evidencia | Fase |
|---|---|---|---|---|
| 26 | `T?`, `Array<int?>` ≠ `Array<int>?` | ✅ | test 30, 46 | — |
| 27 | Unions con narrowing | ✅ | test 15 | — |
| 28 | Intersecciones `A & B` | ❌ **crash** | panic `atom.rs:46` desde `resolve_type_node` → `get_interface_members_local` (Atom de otra tabla: Ley 2) | 0 |
| 29 | Literal types | ❌ | parser: `literal types are not supported` | D |
| 30 | Enums como sum types con payload/generics | ✅ | tests 16, 41 | — |
| 31 | Exhaustiveness con literales | ⚠️ | existe para enums (`check/exhaustiveness.rs`); falta literal/union | D |
| 35 | `Type<T>` | ❌ | `undefined variable: Type` | H |
| 36 | `Obj::key` estático | ⚠️ | `P::x` devuelve `null` en runtime | H |
| 37 | `Function` vs `NativeFunction` internos | ✅ | `TypeTag::NativeFn` sin refs → se borra en C | C |
| 39 | Modelo `Type` estructurado | ⚠️ | `TypeKind` (`varn-core/src/kinds.rs:7`) sin `Literal`, `Nullable`, `Record`, `Map`, `Set`, `Span`, `MetaType`; mezcla `TypeTag` como primitivo | C |
| 42 | `Unit` / `void` / `never` | ❌ | `()` es error de parser | D |
| 43 | `dynamic` aislado | ⚠️ | `DynReason` existe en TIR; falta contabilidad/diagnóstico de contaminación | G |

### 2.5 Capacidades y operadores (§32–§34, §56)

| § | Requisito | Estado | Evidencia | Fase |
|---|---|---|---|---|
| 32 | Interfaces estructurales | ✅ | test 13 | — |
| 33 | `Equatable/Comparable/Hashable/...` | ❌ | ninguna existe en `std/` | E |
| 34 | Operadores vía `Add<T,R>` + overloading usuario | ❌ | `check/mod.rs:422-445` decide por tag (`is_numeric`, `Type::Str`) | E |

### 2.6 Runtime, layout, GC (§38, §40–§41, §47–§50, §96–§102)

| § | Requisito | Estado | Evidencia | Fase |
|---|---|---|---|---|
| 38, 100 | `TypeTag` → `RuntimeKind` | ❌ | `TypeTag` es a la vez tipo del lenguaje (`TypeKind::Intrinsic(TypeTag)`), clave de clases intrínsecas y tag runtime | C |
| 40, 103 | `Error/DateTime/Duration/UUID/Regex/VmRef/TaskHandle/NativeFn` fuera de `TypeTag` | ❌ | presentes (`type_tag.rs:28-37`); casi sin referencias (1–4 c/u) | C |
| 41 | Jerarquía de errores de plataforma | ⚠️ | `Error/TypeError/RangeError` ya son clases (`globals.vn:7-21`); faltan `IntegerOverflow`, `DivisionByZero` | A |
| 47, 101 | `TypeLayout` | ❌ | `FieldRepr {size, align, is_gc_ref}` (`type_tag.rs:183-188`) | F |
| 48, 96 | `GCLayout` | ❌ | `is_gc_ref: bool` | F |
| 49–50 | Nichos / layout de uniones | ❌ | `T?` escalar = `Dynamic` hoy (PLAN-PENDIENTE §5.3) | F |
| 97–98 | Opcodes tipados sin fallback por tag | ❌ | `DivInt`/`ModInt` caen a `box_reg` + `is_int()` (`ops_math_cmp.rs:271-284`) | G |
| 102 | `VmValuePayload` fuera de rutas calientes | ⚠️ | 9 archivos; auditar en G | G |

### 2.7 Async, iteración (§44, §70)

| § | Requisito | Estado | Evidencia | Fase |
|---|---|---|---|---|
| 44 | `Iterator/Generator/Future/Task/TaskHandle/Stream/AsyncIterator` separados | ⚠️ | re-exportados de `core:intrinsics` (`std/task/mod.vn:35`); `Future`, `Stream` ausentes | I |

### 2.8 Plataforma estándar (§55–§92)

Estado de `std/` hoy: `buffer`(vacío) `cli` `collections` `compress` `crypto`
`csv`(vacío) `encoding` `env` `ffi` `fs` `http` `io` `json`(vacío) `log`
`markdown` `math` `net` `path` `process` `reflect` `regex` `result` `sqlite`
`sys` `task` `test` `time` `ws`.

| Módulo spec | Hoy | Brecha principal | Fase |
|---|---|---|---|
| `std:core` (§56) | `result` + globals | capacidades, `Option/Result/Error` unificados | E/K |
| `std:collections` (§57) | parcial | `Deque`, `BitSet`; `PriorityQueue` ok | K |
| `std:strings` (§58) | métodos de `str` | `StringBuilder`, `StringView`, graphemes, normalización | J/K |
| `std:bytes` (§59) | `Bytes` + `buffer`(vacío) | `ByteBuffer`, readers/writers binarios, endianness | J/K |
| `std:math` (§60) | `math` | por dominio (`int/float/decimal/bigint`) | K |
| `std:time` (§62) | `time` | `Clock`, `TimeZone`, `Calendar`; `DateTime/Duration` fuera de `TypeTag` | C/K |
| `std:uuid` (§63) | `crypto/uuid.vn` | módulo propio, valor 128-bit | K |
| `std:regex` (§64) | `regex` | `Match`, `Capture` | K |
| `std:io` (§65) | `io` | `Reader/Writer/ReaderAt/WriterAt/Seek` sobre `Span/Bytes` | J/K |
| `std:fs` (§66) | `fs` | `Watcher`, `Permissions` | K |
| `std:process` (§67) | `process` | `Command`, `ExitStatus`, `Signal` | K |
| `std:os` (§68) | `sys` + `env` | renombrar/consolidar | K |
| `std:thread` (§69) | `task/sync.vn` | `RwLock`, `Condvar`, `Barrier`, `Once`, `Atomic` | K |
| `std:async` (§70–71) | `task` | `Future`, `CancellationToken`, `Stream`; IOCP/io_uring/kqueue | I/K |
| `std:net` (§72) | `net`, `ws` | `Dns`, `Tls`, `Quic` | K |
| `std:http` (§73) | `http` | `Cookies`, `Middleware` | K |
| `std:json` + serialización (§74–77) | `encoding/json.vn` | `Serialize<T>/Deserialize<T>` estáticos; formatos sobre el mismo modelo | K (tras H) |
| `std:db` + SQL (§78–79) | `sqlite` | `Connection/Transaction/Pool/Migration`, query tipada | K |
| `std:crypto` (§80–81) | `crypto` | HKDF/AES/ChaCha20/Ed25519/X25519; `secureRandom` separado | K |
| `std:logging` (§82) | `log` | `StructuredLog`, `Sink` | K |
| `std:observability` (§83) | — | todo | K |
| `std:cli` (§84) | `cli` | `Completion`, `Progress`, subcomandos | K |
| `std:config` (§85) | — | todo | K |
| `std:test` / `std:bench` (§86–87) | `test` | property testing, snapshots, bench → `vn bench` | K |
| `std:diagnostics` (§88) | — | todo | K |
| `std:reflection` (§89) | `reflect` | sobre `Type<T>` | H |
| `std:ffi` (§90) | `ffi` | `Span/Bytes` de primera clase | J/K |
| WASM (§91), `std:simd` (§92) | — | todo | L |
| `unsafe` (§95) | — | frontera explícita | L |

---

## 3. Fases

Orden y dependencias:

```text
0 ──► A ──► B
      │
      └──► C ──► D ──► E ──► H ──► K (serialización, reflexión)
            │     │
            │     └──► I (async/iter)
            │
            └──► F ──► G ──► L
                  │
                  └──► J (StringView/Span/Bytes) ──► K (io/net/ffi)
```

- A antes que todo: fija la semántica observable que el resto asume.
- C antes que D/F: el modelo de tipos del lenguaje debe existir antes de añadir
  literales, unit, etc., y antes de derivar layouts.
- G se coordina con PLAN-PENDIENTE F5/F6 (JIT desde SSA). **Regla:** F5 se
  congela mientras dura A (A borra `NarrowRangeCheck`, `BackendTy::Int8..` y
  cambia `IntDiv`; F5 los tocaría en paralelo). F5/F6 se reanudan tras A y
  deben terminar antes de G.

### Fase 0 — Infraestructura de protección y crashes

**Plan detallado:** `2026-09-23-fase-0-A-semantica-numerica.md`, tareas 0.1–0.4.

- 0.1 Spec a `docs/lang/`, ADR-0015 (decisiones D1–D13), ADR-0004 §2 marcado
  como reemplazado.
- 0.2 Runner del corpus negativo `tests/errors/` como test de integración
  (`crates/varn-cli/tests/error_corpus.rs`); hoy no lo ejecuta nadie.
- 0.3 Crash de tuplas (`frame_store.rs:202`).
- 0.4 Crash de intersecciones (`atom.rs:46`).

**Salida:** `cargo test -p varn-cli --test error_corpus` verde; `main.vn` verde
en 4 cuadrantes; tests nuevos 117 (tuplas) y 118 (intersecciones).

### Fase A — Semántica numérica del núcleo

**Plan detallado:** mismo archivo, tareas A.1–A.9.

Entrega: errores `IntegerOverflow`/`DivisionByZero`; `int / int → int` con los
tres casos de §10; IEEE en `float`; DCE que no borra operaciones que pueden
lanzar; conversiones reales (`InstKind::Convert` + opcodes) en lugar del `Move`
y del hack `+ 0.0`; fin de conversiones implícitas lossy con literales
contextuales; borrado total de `i8..u64/f32`; API de enteros de §3/§10/§61;
docs.

**Se borra:** `TypeTag::{I8,I16,I32,U8,U16,U32,U64,F32}`,
`CheckerTyId::{I8..F32}`, `BackendTy::{Int8..Float32}`,
`InstKind::NarrowRangeCheck`, `SsaOp::NarrowRangeCheck`,
`OpCode::CheckNarrowRange`, `varn-vm/src/exec/narrow_range.rs`,
`ArrayRepr::{I8..F32}`, `narrow_elem`, `narrow_tag_of`, tests 107/108/116 y
`tests/errors/*narrow*`, el `+ 0.0` de `from_tir/build.rs`.

### Fase B — Dominios de precisión arbitraria

Spec: §6, §7, §8, §9, §10 (bigint/decimal), §60.

Tareas (cada una con su test `.vn` y commit):

1. **ADR-0016**: crate de bigint (`num-bigint` recomendado: maduro, sin
   `unsafe` propio, `Hash`/`Ord`) y de decimal arbitrario (`bigdecimal`, sobre
   `num-bigint`); política de `decimal /`: escala máxima de operandos + 34
   dígitos significativos, redondeo half-even; documentar en `numeric.rs`.
2. Runtime: `Value::BigInt(Box<BigInt>)`, `HeapObj::BigInt`,
   `SendValue::BigInt` (`varn-types/src/value/{mod,sendable}.rs`); hash/igualdad
   por valor.
3. Literales: lexer/parser guardan el texto del literal `n` (hoy `i128`);
   `TirExprKind::BigIntLit(Arc<str>)`; constante en pool por texto.
4. Aritmética: `arith.rs` bigint `+ - * / % **` (división trunca, `/ 0` →
   `DivisionByZero`); comparaciones mixtas `int`/`bigint` exactas.
5. Decimal: `Value::Decimal(Box<BigDecimal>)`; `rust_decimal` sale del workspace
   (Ley 8), `std/math` y `std/encoding/toml.vn` migran.
6. Conversiones de Fase A extendidas: `bigint as int` (rango),
   `decimal as int` (trunca, rango), `float as decimal`, `decimal as float`,
   `bigint as float`, `float as bigint` (trunca; NaN/Inf lanzan).
7. Métodos `bigint`/`decimal` de `std:math` (§60): `abs`, `pow`, `sqrt`
   (decimal con precisión explícita), `round(scale)`.
8. Serialización cross-isolate y del cache (`BUILD_FINGERPRINT` invalida solo).

**Salida:** test `35-decimal-bigint.vn` ampliado con valores > 2^127 y > 28
dígitos; bench `compare.ps1` sin regresión en benches que no usan bigint.

### Fase C — Separar tipo del lenguaje de clasificación runtime

Spec: §13, §20, §37–§40, §100, §103, D2.

1. **Nuevo modelo del lenguaje en `varn-core`**: `LangPrimitive { Null, Bool,
   Int, Float, BigInt, Decimal, Char, Str, Void, Never, Dynamic, Unit }`
   (enum propio, `#[repr(u8)]`). `TypeKind::Intrinsic(TypeTag)` →
   `TypeKind::Primitive(LangPrimitive)`. `IntrinsicType` se divide:
   `LangPrimitive` (tipos) + `WellKnownClass` (nombres de clases de plataforma:
   `Array`, `Map`, `Set`, `Range`, `Bytes`, `Error`, `TypeError`, …) como
   `&'static str` en `well_known.rs`.
2. **`TypeTag` → `RuntimeKind`** (renombrado + poda): quedan `Null Bool Int
   Float BigInt Decimal Char String Array Map Set Tuple Object Class Function
   Enum Task Generator Range Bytes Record` (§100). Se borran `Symbol`, `Void`,
   `Never`, `Dynamic` (no son valores), `Error`, `TypeError`, `RangeError`,
   `VmRef`, `TaskHandle`, `NativeFn`, `Regex`, `DateTime`, `Duration`, `UUID`,
   `Span`, `TypedArray`. Cada `match` roto se decide explícito (Ley 7).
3. `FieldRepr` se calcula desde `BackendTy` (no desde `TypeTag`) — paso
   intermedio hacia `TypeLayout` de Fase F, que es parte del diseño final: una
   sola función `layout_of(BackendTy)`.
4. `Symbol` → clase de plataforma en `std` (D2): `intrinsics.rs:31`,
   `primitives/symbol/`, 8 usos en corpus.
5. `CheckerTyId` reservados: renumerar sin huecos tras borrar `SYMBOL` y los
   anchos (los de A).
6. `vn debug -p check:types`/golden (`debug_golden.rs`) regenerados.

**Salida:** `grep -rn "TypeTag" crates/varn-checker` = 0 (el checker solo
conoce `LangPrimitive`); `main.vn` verde 4 cuadrantes.

### Fase D — Superficie del sistema de tipos

Spec: §21–§24, §26–§31, §42, §45, §53, §104.

1. Literal types: `TypeKind::Literal(LiteralValue)` (`Int(i64)`, `Str(Arc<str>)`,
   `Bool`, `Char`); parser acepta literales en posición de tipo; subtipado
   `"GET" <: str`; widening de literal en `let` mutable; `const` conserva el
   literal.
2. Exhaustiveness sobre uniones de literales y `T?` (`check/exhaustiveness.rs`).
3. `Unit`: expresión `()`, tipo `Unit`; `void` solo en retorno; `never` bottom
   (inferencia de `throw`/bucle infinito).
4. `Record<K,V>` → diagnóstico dedicado `RecordShapeMismatch`/nuevo
   `ForbiddenRecordGeneric` con sugerencia `Map<K,V>` o `{ [key: K]: V }`.
5. Separar `Map<K,V>` de index signatures: borrar las ramas de
   `compat/mod.rs:570-740` que las igualan; `DynReason::IndexSignature` sigue
   siendo la lectura de índice.
6. `new Map()`/`new Set()` infieren argumentos del tipo esperado
   (`type_inference.rs:461-463` hoy fija `Map<dynamic>`).
7. Object vs Record vs Class: igualdad (`==` referencial en Object/Class,
   estructural profunda en Record/Tuple); tests dedicados.
8. `Range<T>` genérico (`Range<int>`, `Range<char>`), `step` perezoso (no array).
9. `Intersection` con semántica completa (miembros = unión de miembros;
   conflicto = `never`).

### Fase E — Capacidades y operadores

Spec: §25, §32–§34, §56.

1. `std:core` con `Equatable<T>`, `Comparable<T>`, `Hashable`, `Cloneable`,
   `Default`, `Display`, `Debug`, `Iterable<T>`, `Iterator<T>`,
   `AsyncIterable<T>`, `Indexable<K,V>`, `Add/Sub/Mul/Div/Neg<T,R>`.
2. Resolución de operadores por capacidad en el checker: tabla de impls
   primitivas (`int: Add<int,int>`, `str: Add<str,str>`…) como **datos** en
   `std:core`, no `match` por tag en `check/mod.rs:422-445`.
3. Overloading de usuario: una clase que implementa `Add<Vector,Vector>`
   baja a llamada directa (`CallDirect`), sin dispatch dinámico (§34).
4. `Map/Set` con claves de clase usuario vía `Hashable`/`Equatable`
   (`value/map.rs` llama a `hash()`/`equals()` para `RuntimeKind::Class`).

### Fase F — Layout

Spec: §12, §47–§51, §96, §101.

1. `TypeLayout { size, align, stride, abi, repr: ScalarRepr, gc: GcLayout,
   fields: Vec<FieldLayout>, variants, niches }` en un crate/módulo `layout`
   consumido por checker (offsets), VM (`ClassLayout`) y JIT. Borra `FieldRepr`.
2. `GcLayout` = bitmap de referencias; el GC recorre instancias y arrays por
   bitmap, no por inspección de valor (`Array<int>` sin trazado).
3. Nichos: `T?` sobre referencia = puntero nulo; `T?` escalar = (valor, bit)
   (cierra PLAN-PENDIENTE §5.3).
4. Uniones: discriminante + layout por variante + eliminación por nicho.
5. Packing de `Array<int>` en `i32`/`i16` solo con prueba de rango (D11), tras L.

### Fase G — Ejecución tipada

Spec: §43, §46, §97–§99, §102.

1. Opcodes tipados sin fallback: `AddInt/…/DivInt` asumen operandos probados;
   el `else` con `box_reg` (`ops_math_cmp.rs:271-284`) se borra; el verificador
   SSA (`ssa/verify.rs`) garantiza la precondición.
2. `dynamic` localizado: `vn debug -p dyn` cuenta `DynReason` por función; un
   test falla si `NotYetSupported`/`Unannotated` crece en `std/`.
3. `for (const i of a..b)` sin alloc (contador + límite); verificado con
   `vn debug -p gc`.
4. `VmValuePayload` restringido a interop/debug (auditoría de los 9 archivos).
5. Requiere PLAN-PENDIENTE F5/F6 cerrado.

### Fase H — Meta-tipos y reflexión

Spec: §35–§36, §89.

`Type<T>` como expresión con tipo `MetaType(T)`; `Obj::key` resuelto
estáticamente (hoy `null`); `std:reflection` (campos, métodos, atributos,
variantes, layout) generado desde metadata estática; reflexión no usada se
elimina.

### Fase I — Async e iteración

Spec: §44, §70.

`Iterator<T>`, `Generator<T>`, `Future<T>`, `Task<T>`, `TaskHandle<T>`,
`Stream<T>`, `AsyncIterator<T>` como tipos distintos con relaciones
explícitas; `RuntimeKind::{Task, Generator}` solo como clasificación.

### Fase J — Vistas y binario

Spec: §15–§18, §54, §59, §65, §90.

`StringView` (no propietaria, UTF-8), `Span<T>` (vista contigua; `Array<T>` →
`Span<T>` sin copia), `Bytes` = almacenamiento `u8` interno; APIs de `io`/`net`/
`ffi` aceptan `Span`/`Bytes`.

### Fase K — Plataforma estándar

Spec: §55–§92. Un sub-plan por módulo (tabla 2.8), en este orden: `core` →
`collections` → `strings` → `bytes` → `math` → `time` → `uuid` → `regex` →
`io` → `fs` → `process` → `os` → `thread` → `async` → `net` → `http` →
`json`+serialización → `db`/SQL → `crypto` (+ `secureRandom`, §81) →
`logging` → `observability` → `cli` → `config` → `test` → `bench` →
`diagnostics` → `ffi`. Cada módulo: contrato `.vn` + nativo Rust
(`varn-builtins/src/modules/host/*`) + test numerado + doc en
`docs/lang/standard_library.md`.

### Fase L — Optimizador y fronteras de bajo nivel

Spec: §91–§95.

Análisis de rango y overflow (elimina checks de `int` probados), bounds-check
elimination, escape analysis / scalar replacement, auto-vectorización,
`std:simd`, WASM, `unsafe`.

---

## 4. Riesgos

| Riesgo | Mitigación |
|---|---|
| `int / int → int` cambia resultados de programas que compilan (silencioso) | Tarea A.4 añade, antes del cambio, un barrido: se compila `std/` y `tests/` con un diagnóstico temporal que marca cada `Div` int/int y se revisa cada sitio a mano; el diagnóstico se borra en el mismo commit que cambia la regla (no queda código de transición). |
| Quitar `int → float` implícito rompe `std/` en build | `varn-cli/build.rs` falla en error de tipos de `std/`: el compilador enumera cada sitio. Se corrige con `as float` o literal `.0`. |
| Borrar anchos rompe serialización de caché | `BUILD_FINGERPRINT` invalida por build; verificar con `VARN_CACHE_DIR` limpio. |
| Divergencia intérprete/JIT en conversiones | Cada conversión tiene test que corre en `main.vn` (4 cuadrantes) y la fila de tier-parity (`56/58/62/65/101`). |
| DCE más conservador empeora benchmarks | Solo las operaciones que pueden lanzar dejan de ser removibles; medir `cargo xtask compare` antes/después (memoria: el bench local es ruido → comparar solo regresiones > 10 %). |
| Fase C toca 566 referencias a `TypeTag::` | Se hace por crate, de abajo hacia arriba (`core` → `types` → `checker` → `compiler` → `vm` → `jit` → `lsp`), un commit por crate compilando. |

## 5. Qué NO entra (por ahora)

- `struct` (§51): reservado; no se introduce.
- Sintaxis `for x in` (D1).
- `unknown` (§43).

## 6. Puerta de validación (cada commit)

1. `cargo check --workspace --exclude varn-lsp --all-targets` sin warnings.
2. `cargo test -p varn-core -p varn-checker -p varn-compiler -p varn-vm` (los
   tests unitarios tocados).
3. `cargo test -p varn-cli --test error_corpus` (desde Tarea 0.2).
4. `cargo build --release -p varn-cli`, y `tests/main.vn` con `VARN_CACHE_DIR`
   limpio en JIT y `VARN_NO_JIT=1`: `PASSED: N`, `FAILED: 0`.
5. Antes de cerrar cada fase: `.\scripts\verify.ps1 -Fast` (4 cuadrantes).
6. Cambios de runtime/JIT: `cargo xtask compare` sin regresión atribuible.

## 7. Documentos a mantener

Por fase, en el mismo commit que el cambio de comportamiento:
`docs/lang/types.md`, `docs/lang/expressions.md`, `docs/lang/runtime_behavior.md`,
`docs/lang/standard_library.md`, `docs/TIR_CONTRATO_TIPADO.md`,
`crates/varn-core/src/numeric.rs` (doc de módulo), ADR correspondiente.
