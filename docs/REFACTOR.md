# VARN REFACTOR PLAN — "Lo que TypeScript no puede ser"

> **Objetivo central**: el type system de Varn debe tener efectos directos sobre el código generado —
> no solo sobre el frontend de type checking. Código completamente tipado debe compilar sin
> overhead polimórfico, sin capas de indirección, sin boxing innecesario.

---

## Diagnóstico de estado actual

| Problema | Causa raíz | Costo observable |
|----------|------------|-----------------|
| `int + int` hace unbox→add→box | NaN-boxing universal; stack es `Vec<VmValue>` | Overhead en toda aritmética tipada |
| `abs(x)` (std:math) = 3 capas | wrapper `.vn` → `runtime:math` → Rust | Call + NativeFn alloc por invocación |
| `Closure: 509` allocs/run | `MakeClosure` siempre heap; sin escape analysis | GC pressure en lambdas de corta vida |
| Module lookup = hash | `FxHashMap<ModuleId, VmValue>` en hot path | Hash overhead en imports frecuentes |
| JIT sin info de tipos | JIT observa runtime; no consume `TypeAnnotations` | Warm-up necesario; guards redundantes |
| `NaN-boxing` universal | `VmValue = u64` siempre, incluso para `int` conocido | Instrucciones de CPU desperdiciadas |

### Lo que TypeScript no puede garantizar (y Varn debería poder)

- Que `number + number` nunca tenga overhead de tipo
- Que una lambda sin capturas no alloce
- Que `abs(x: float)` sea una instrucción de CPU (sin capas de módulo)
- Que un import sea un array index, no un hash lookup
- Que un objeto de corta vida no llegue al GC

---

## Principios de implementación

**Modularización por dominio**: cada feature nuevo vive en su propio módulo/archivo.
No ampliar archivos existentes más allá de añadir una función pública que delega.
Un archivo por responsabilidad. Máximo ~300 líneas por archivo.

**Backward compatibility**: cada fase es aditiva. El código dinámico (`Dynamic`) sigue
funcionando exactamente igual. El nuevo comportamiento aplica solo cuando el checker
tiene información concreta.

**Verificabilidad**: cada fase tiene métricas concretas medibles con `vn bench`.

---

## FASE 1 — Typed Register Metadata

**Propósito**: hacer que el compilador comunique información de tipos al backend por registro.
Habilita todas las fases posteriores.

### 1.1 — `SlotKind` y `RegisterMeta` en `varn-types`

**Archivo nuevo**: `crates/varn-types/src/chunk/register_meta.rs`

```rust
/// Tipo concreto de un registro de VM en una función.
/// Producido por el compilador, consumido por el JIT y el intérprete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotKind {
    Int,     // siempre i64 — nunca NaN-boxed en JIT
    Float,   // siempre f64
    Bool,    // siempre bool
    Str,     // siempre string (SSO o heap ptr)
    Ref,     // heap pointer — objeto/closure/módulo
    Dynamic, // VmValue genérico — NaN-boxed (fallback)
}

#[derive(Debug, Clone)]
pub struct RegisterMeta {
    pub kind: SlotKind,
}
```

**Modificación mínima en**: `crates/varn-types/src/chunk.rs`
```rust
// Añadir a FunctionProto (campo opcional para backward compat):
pub register_meta: Vec<RegisterMeta>,  // len == register_count
```

**Regla de diseño**: `register_meta` vacío = comportamiento actual (todo Dynamic).
Nunca rompe código existente.

---

### 1.2 — Llenado de `register_meta` en el compilador

**Archivo nuevo**: `crates/varn-compiler/src/analysis/slot_kinds.rs`

Responsabilidad única: recorrer `TypeAnnotations` post-codegen y asignar `SlotKind`
a cada registro local de la función.

```
SlotKind::Int   ← TypeTag::Int  + no escapa a Dynamic
SlotKind::Float ← TypeTag::Float + no escapa a Dynamic
SlotKind::Bool  ← TypeTag::Bool
SlotKind::Str   ← TypeTag::Str  + no escapa a Dynamic
SlotKind::Ref   ← cualquier tipo de referencia (objeto, closure, array)
SlotKind::Dynamic ← TypeTag::Dynamic o tipo desconocido
```

**Integración**: `crates/varn-compiler/src/codegen/compiler.rs` llama
`slot_kinds::infer(proto, annotations)` al final de compilación de cada función.

**No tocar**: lógica de codegen existente. Solo post-procesado.

---

### 1.3 — Completar typed arithmetic: `ModInt`, `PowInt`, `ModFloat`, `PowFloat`

**Archivo existente**: `crates/varn-core/src/opcode.rs` — añadir 4 opcodes.

**Archivo existente**: `crates/varn-compiler/src/codegen/expr/operators.rs`
— aplicar `NumericKind` a `Mod` y `Pow` igual que se hace con `Add`/`Sub`.

**Archivos de dispatch VM** (añadir arms en match existente):
- `crates/varn-vm/src/exec/ops_arith.rs` (o equivalente)
- `crates/varn-jit/src/codegen/emit_arith.rs`

**Regla**: un opcode nuevo = un arm nuevo en dispatch + un test. Sin dios-archivos.

---

## FASE 2 — Intrínsecos de Compilador

**Propósito**: eliminar las capas `std:X.vn → runtime:X → Rust` para operaciones
que el compilador puede emitir directamente como instrucciones.

### 2.1 — Registro federado de operaciones intrínsecas

**Problema de diseño**: un único `enum IntrinsicOp` con todos los dominios (math,
string, array, types...) se convierte en god-file — crece con cada built-in nuevo,
requiere tocar el mismo archivo para dominios independientes, y acopla compiler +
VM + JIT a un solo punto de cambio.

**Solución: registro federado por dominio.**

El opcode `Intrinsic` lleva un `u8` con encoding `DDDD_OOOO`:
- bits 7-4 = dominio (hasta 16 dominios)
- bits 3-0 = operación dentro del dominio (hasta 16 ops por dominio)

Cada dominio es un archivo independiente. Añadir un built-in nuevo = tocar solo
el archivo de ese dominio. El `wire.rs` central nunca crece.

**Estructura de archivos**:

```
crates/varn-core/src/intrinsics/
├── mod.rs       — reexporta IntrinsicDomain, encode/decode, INTRINSIC_MAP
├── wire.rs      — encode(domain, op) → u8 / decode(u8) → (domain, op)
├── map.rs       — tabla global &str → (domain, op); construida desde cada dominio
├── math.rs      — enum MathOp + sus entradas de mapa
├── string.rs    — enum StringOp + sus entradas de mapa
├── array.rs     — enum ArrayOp + sus entradas de mapa
└── types.rs     — enum TypeCheckOp + sus entradas de mapa
```

**`wire.rs`** — nunca crece, solo define el encoding:

```rust
/// Dominio de un intrínseco. 4 bits (valores 0x0–0xF).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum IntrinsicDomain {
    Math      = 0x0,
    String    = 0x1,
    Array     = 0x2,
    TypeCheck = 0x3,
    // 0x4–0xF reservados para dominios futuros
}

/// Codifica (dominio, op_local) en un byte de wire.
#[inline(always)]
pub const fn encode(domain: IntrinsicDomain, op: u8) -> u8 {
    ((domain as u8) << 4) | (op & 0x0F)
}

/// Decodifica un byte de wire en (dominio, op_local).
#[inline(always)]
pub fn decode(byte: u8) -> (u8, u8) {
    (byte >> 4, byte & 0x0F)
}
```

**`math.rs`** — solo operaciones math, auto-contenido:

```rust
use super::wire::{encode, IntrinsicDomain};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MathOp {
    Abs   = 0x0,
    Sqrt  = 0x1,
    Floor = 0x2,
    Ceil  = 0x3,
    Round = 0x4,
    Sin   = 0x5,
    Cos   = 0x6,
    Tan   = 0x7,
    Log   = 0x8,
    Exp   = 0x9,
    Pow   = 0xA,
    Min   = 0xB,
    Max   = 0xC,
}

impl MathOp {
    pub const fn wire(self) -> u8 {
        encode(IntrinsicDomain::Math, self as u8)
    }
}

/// Entradas para el mapa global. Llamado desde map.rs durante construcción.
/// Clave: "módulo:función" — resuelto desde el binding del checker, no acceso global.
/// `Math` NO es un global en Varn. El checker resuelve `abs` importado de "std:math"
/// al binding `std:math/abs` y ese binding es la clave del mapa.
pub const MAP_ENTRIES: &[(&str, u8)] = &[
    ("std:math/abs",   MathOp::Abs.wire()),
    ("std:math/sqrt",  MathOp::Sqrt.wire()),
    ("std:math/floor", MathOp::Floor.wire()),
    ("std:math/ceil",  MathOp::Ceil.wire()),
    ("std:math/round", MathOp::Round.wire()),
    ("std:math/sin",   MathOp::Sin.wire()),
    ("std:math/cos",   MathOp::Cos.wire()),
    ("std:math/log",   MathOp::Log.wire()),
    ("std:math/exp",   MathOp::Exp.wire()),
    ("std:math/pow",   MathOp::Pow.wire()),
    ("std:math/min",   MathOp::Min.wire()),
    ("std:math/max",   MathOp::Max.wire()),
];
```

Mismo patrón para `string.rs`, `array.rs`, `types.rs`. Cada archivo < 60 líneas.

**`map.rs`** — agrega las entradas de todos los dominios, nunca define ops:

```rust
use super::{array, math, string, types};

pub static INTRINSIC_MAP: &[(&str, u8)] = {
    // Concatenar en compile time (const evaluation).
    // Cada dominio contribuye su MAP_ENTRIES. map.rs no conoce los detalles.
    &[
        math::MAP_ENTRIES,
        string::MAP_ENTRIES,
        array::MAP_ENTRIES,
        types::MAP_ENTRIES,
    ]
    .concat()  // o macro si concat en const context no está disponible
};
```

En la práctica: `map.rs` usa `phf` (perfect hash) o `once_cell::Lazy` para
construir el mapa en startup desde los slice de cada dominio.

**`mod.rs`** — solo reexporta:

```rust
pub mod wire;
pub mod map;
pub mod math;
pub mod string;
pub mod array;
pub mod types;

pub use wire::{decode, encode, IntrinsicDomain};
pub use map::INTRINSIC_MAP;
```

**Nuevo opcode**: `crates/varn-core/src/opcode.rs`
```rust
Intrinsic,  // siguiente word = wire byte (domain:4 | op:4)
```

**Regla de extensión**: añadir dominio nuevo = crear `nuevo_dominio.rs` + añadir
su `MAP_ENTRIES` a `map.rs`. Nunca tocar `wire.rs`, nunca tocar dominios existentes.

---

### 2.2 — Anotación de calls intrínsecas en el checker

**Archivo existente**: `crates/varn-core/src/typed_ir.rs`
— añadir campo `intrinsic: Option<IntrinsicOp>` a `ExprAnnotation`.

**Archivo nuevo**: `crates/varn-checker/src/analysis/intrinsic_resolver.rs`

Responsabilidad única: durante la fase de type checking de call expressions,
si el callee se resuelve a un binding de módulo conocido en `INTRINSIC_MAP`
y todos los argumentos tienen tipos concretos, anotar con `intrinsic: Some(op)`.

**Sobre globals**: Varn no tiene `Math` como global. Los globals son: clases de
error (`TypeError`, `AssertError`, ...), tipos core (`int`, `float`, `str`, `bool`),
y funciones de sistema (`print`, `assert`, `input`). Operaciones matemáticas viven
en `std:math` y se usan vía import. El resolver trabaja con el binding resuelto por
el checker, no con expresiones de acceso globales.

**Forma del binding en el checker**: cuando el usuario escribe:
```
import { abs } from "std:math"
abs(x)
```
El checker resuelve `abs` al binding `std:math/abs`. Esa clave se busca en `INTRINSIC_MAP`.
Cuando el usuario importa con alias (`import { abs as mathAbs }`) el binding interno
sigue siendo `std:math/abs` — el alias es sintaxis, no afecta al resolver.

**Condición de activación**:
- Callee es un identificador local cuyo binding se resuelve a `módulo/función` conocido en `INTRINSIC_MAP`
- Todos los argumentos con tipos concretos (no `Dynamic`)
- El resultado no se usa como valor de primera clase (no `let f = abs`)

Si no se cumple la condición → emit normal de Call. Sin romper nada.

**Integración**: llamado desde `crates/varn-checker/src/check/expr.rs` al finalizar
la resolución de un `CallExpr`.

---

### 2.3 — Emisión de `Intrinsic` en el compilador

**Archivo nuevo**: `crates/varn-compiler/src/codegen/expr/intrinsics.rs`

Responsabilidad única: si `annotations.get_intrinsic(offset)` retorna `Some(op)`,
emitir `OpCode::Intrinsic` + `op as u8` en lugar de la secuencia normal de Call.

```rust
pub fn try_emit_intrinsic(
    c: &mut Compiler,
    call_expr: &CallExpr,
    args_regs: &[Reg],
    dst: Reg,
) -> bool {
    let Some(op) = c.annotations.get_intrinsic(call_expr.offset) else {
        return false;
    };
    // emit: Intrinsic, op as u8, dst, args...
    true
}
```

**Integración**: `crates/varn-compiler/src/codegen/expr/calls.rs` llama
`try_emit_intrinsic` primero; si retorna false, sigue con path normal.

---

### 2.4 — Dispatch de `Intrinsic` en el VM

**Archivo nuevo**: `crates/varn-vm/src/exec/ops_intrinsics.rs`

Tabla estática `IntrinsicOp → fn(&mut Heap, args) → VmValue`.
Sin búsqueda de módulo, sin IC, sin BoundMethod alloc.

```rust
pub fn dispatch_intrinsic(
    op: IntrinsicOp,
    args: &[VmValue],
    heap: &mut Heap,
) -> VmResult<VmValue> {
    match op {
        IntrinsicOp::MathAbs => math::abs(args, heap),
        IntrinsicOp::MathSqrt => math::sqrt(args, heap),
        IntrinsicOp::StrLen => string::len(args, heap),
        IntrinsicOp::ArrPush => array::push(args, heap),
        // ...
    }
}
```

**Sub-archivos** (uno por dominio):
- `crates/varn-vm/src/exec/intrinsics/math.rs`
- `crates/varn-vm/src/exec/intrinsics/string.rs`
- `crates/varn-vm/src/exec/intrinsics/array.rs`
- `crates/varn-vm/src/exec/intrinsics/types.rs`
- `crates/varn-vm/src/exec/intrinsics/mod.rs` — reexporta dispatch

**Integración**: dispatch loop en `crates/varn-vm/src/exec/` — arm para
`OpCode::Intrinsic` → `ops_intrinsics::dispatch_intrinsic(op, args, heap)`.

---

### 2.5 — Inline emit en el JIT

**Archivo nuevo**: `crates/varn-jit/src/codegen/emit_intrinsics.rs`

Para cada `IntrinsicOp`, emit inline sin call overhead:

```rust
pub fn emit_intrinsic(op: IntrinsicOp, ctx: &mut JitCtx, args: &[Reg], dst: Reg) {
    match op {
        IntrinsicOp::MathAbs   => emit_fabs(ctx, args[0], dst),
        IntrinsicOp::MathSqrt  => emit_fsqrt(ctx, args[0], dst),
        IntrinsicOp::MathFloor => emit_floor(ctx, args[0], dst),
        IntrinsicOp::StrLen    => emit_str_len(ctx, args[0], dst),
        IntrinsicOp::ArrLen    => emit_arr_len(ctx, args[0], dst),
        // ...
    }
}
```

Operaciones numéricas → instrucción SSE2/AVX directa.
Operaciones de string/array → inline del body (típicamente 5-10 instrucciones).

---

## FASE 3 — Escape Analysis y Stack/Arena Allocation

**Propósito**: reducir allocations eliminando las que tienen lifetime conocido en compilación.

### 3.1 — Escape analysis: lambdas sin capturas

**Archivo nuevo**: `crates/varn-compiler/src/analysis/escape.rs`

**Módulo** `escape::closure`:

Una lambda NO escapa si:
1. No tiene upvalues (`closure.upvalue_count == 0`)
2. No es retornada desde la función
3. No es asignada a un campo de objeto
4. No es pasada a una función cuya firma acepta `Dynamic` o `Function`

Si no escapa + no tiene upvalues → emitir `LoadStaticFn(proto_idx)` en lugar de `MakeClosure`.

**Nuevo opcode**: `crates/varn-core/src/opcode.rs`
```rust
LoadStaticFn,  // siguiente word = proto index en pool
```

En runtime: `LoadStaticFn` retorna un VmValue que apunta al proto como función
estática — sin alloc de closure object. Llama por el mismo path que `MakeClosure`
pero sin el heap slot.

**Impacto esperado**: `Closure: 509 → <100` en el test suite (lambdas de `map`/`filter` son el caso dominante).

---

### 3.2 — Escape analysis: objetos literales de corta vida

**Módulo** `escape::object` en el mismo `escape.rs`:

**Scalar replacement**: objeto `{ x: int, y: int }` donde:
- Todos los campos tienen tipos concretos (checker lo sabe)
- El objeto no escapa del scope de la función

→ no emitir `MakeObject`. Emitir dos locals tipados `x_0: int`, `y_0: int`.
Accesos a `point.x` → acceso al registro directo.

**Complejidad**: requiere que el compilador rastreé "alias" entre locals y campos.
Implementar solo para objetos con ≤6 campos, sin herencia, sin métodos.

**Prioridad**: después de 3.1 (mayor complejidad, menor impacto relativo).

---

### 3.3 — Frame Arena

**Archivo nuevo**: `crates/varn-vm/src/arena.rs`

```rust
/// Bump allocator de duración de frame.
/// Toda memoria se libera al hacer frame pop — O(1), sin GC.
pub struct FrameArena {
    buf: Box<[u8; ARENA_SIZE]>,
    offset: usize,
}

const ARENA_SIZE: usize = 8 * 1024; // 8KB por frame
```

**Cuándo usar**: cuando el escape analysis marca un objeto con `AllocHint::Frame`
(no escapa del frame actual).

**Nuevo campo opcional en alloc opcodes**:
```rust
// varn-core/src/opcode.rs
MakeObjectArena,  // mismo que MakeObject pero alloca en FrameArena
```

**Integración**:
- `crates/varn-vm/src/frame.rs` — `CallFrame` incluye `arena: Option<FrameArena>`
- `crates/varn-vm/src/exec/` — `MakeObjectArena` → `frame.arena.alloc()`
- Frame pop → drop de arena automático (RAII)

**Nota**: la arena solo aplica a objetos puros (no tienen `Drop` ni finalizers especiales).
Classes con destructores siempre van al heap GC.

---

## FASE 4 — Module System Numérico

**Propósito**: eliminar hashing de strings en el hot path de module lookup.

### 4.1 — `ModuleIdx: u32` como identity en runtime

**Archivo nuevo**: `crates/varn-core/src/module_id/idx.rs`

```rust
/// Índice numérico asignado en tiempo de compilación/link.
/// Reemplaza ModuleId en los opcodes de runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ModuleIdx(pub u32);

pub const MODULE_IDX_INVALID: ModuleIdx = ModuleIdx(u32::MAX);
```

**Archivo nuevo**: `crates/varn-compiler/src/module_index.rs`

Responsabilidad única: asignar `ModuleIdx` a cada import durante compilación.
El índice es local al compilation unit; el linker los consolida.

```rust
pub struct ModuleIndexer {
    map: FxHashMap<ModuleId, ModuleIdx>,
    next: u32,
}
```

**Archivos de VM modificados**:

`crates/varn-vm/src/exec/ctx.rs`:
```rust
// ANTES:
pub modules: FxHashMap<ModuleId, VmValue>,

// DESPUÉS:
pub modules: Vec<Option<VmValue>>,           // indexed by ModuleIdx
pub module_id_map: FxHashMap<ModuleId, ModuleIdx>,  // name→idx (solo para init/debug)
```

**Opcodes modificados**: `ImportModule` y `LoadModule` llevan `ModuleIdx` en lugar
de string index al pool de constantes.

**Compatibilidad**: el `module_id_map` se mantiene para mensajes de error, LSP,
y debug. En el hot path (runtime import lookup) solo se usa `Vec<Option<VmValue>>`.

---

### 4.2 — Linker de índices para precompiled map

**Archivo nuevo**: `crates/varn-compiler/src/linker/module_linker.rs`

Cuando se compila un programa completo (todos los módulos conocidos en precompile),
el linker asigna índices globales consistentes. Los protos resultantes tienen
`ModuleIdx` absolutos.

Para compilaciones incrementales (LSP, REPL): reservar rango de índices por módulo,
invalidar downstream cuando un módulo cambia.

---

## FASE 5 — JIT Guiado por Tipos

**Propósito**: usar `SlotKind` (Fase 1) para eliminar guards y unboxing en JIT desde
el primer hit — sin warm-up.

### 5.1 — Especialización por SlotKind en codegen JIT

**Archivo nuevo**: `crates/varn-jit/src/codegen/typed_emit.rs`

Responsabilidad única: variantes de emit para operaciones sobre registros `Int`/`Float` concretos.

```rust
/// Emite add sin guards cuando ambos operandos tienen SlotKind::Int.
pub fn emit_int_add(ctx: &mut JitCtx, a: Reg, b: Reg, dst: Reg) {
    // mov rax, [stack+a]   (raw i64, sin as_int())
    // add rax, [stack+b]
    // mov [stack+dst], rax
    // sin TAG_INT check, sin NaN test
}

/// Emite add con guards cuando SlotKind::Dynamic.
pub fn emit_dynamic_add(ctx: &mut JitCtx, a: Reg, b: Reg, dst: Reg) {
    // camino actual — preservado intacto
}
```

**Integración en**: `crates/varn-jit/src/codegen/emit_arith.rs`
— consulta `proto.register_meta[reg]` para elegir `typed_emit` vs path actual.

**Impacto**: `fib(n: int)` → assembly sin ningún `as_int()`/`from_int()`.
Equivalente a lo que emitiría un compilador C para la misma función.

---

### 5.2 — AOT compilation para funciones monomorphic

**Archivo nuevo**: `crates/varn-jit/src/aot.rs`

Una función es "AOT-eligible" si:
- Todos sus registros tienen `SlotKind` concreto (ninguno `Dynamic`)
- No contiene `Await` ni `Yield`
- No hace `eval` ni acceso de propiedad totalmente dinámico

El precompile step (`bench_impl.rs`, `pipeline/`) llama `aot::try_compile(proto)`
para funciones elegibles. Si tiene éxito, el resultado se guarda junto al proto.

En runtime: si la función tiene JIT compilado AOT, se usa directamente — sin
pasar por el intérprete ni esperar al threshold de JIT warm-up.

---

## FASE 6 — NaN-boxing Condicional

**Propósito**: en funciones donde todos los registros son `Int` o `Float`,
usar representación nativa en lugar de NaN-boxed u64.

### 6.1 — `TypedFrame` con slots nativos

**Archivo nuevo**: `crates/varn-vm/src/typed_frame.rs`

```rust
/// Frame de ejecución para funciones completamente tipadas.
/// Los slots son representaciones nativas — sin NaN-boxing.
pub struct TypedFrame {
    pub ints:   Vec<i64>,
    pub floats: Vec<f64>,
    pub bools:  Vec<bool>,
    pub refs:   Vec<u32>,   // heap indices
}
```

**Activación**: solo cuando `proto.register_meta` está completamente poblado
con SlotKinds concretos (ninguno `Dynamic`).

**ABI boundary**: cuando una función tipada llama a una dinámica (o viceversa),
el boundary convierte `i64 → VmValue::from_int(v)` / `VmValue::as_int() → i64`.
Este boundary existe en `crates/varn-vm/src/exec/calls.rs`.

**Archivo nuevo**: `crates/varn-vm/src/exec/abi_boundary.rs`

Responsabilidad única: conversión entre `TypedFrame` y `VmValue` en call boundaries.

---

### 6.2 — Deprecación gradual de NaN-boxing en código tipado

Una vez que `TypedFrame` funciona y el JIT con `typed_emit.rs` está activo,
el NaN-boxing en código tipado desaparece naturalmente:

- El intérprete usa `TypedFrame` para funciones monomorphic
- El JIT usa `typed_emit` (nunca boxed para Int/Float)
- Solo código `Dynamic` sigue usando `Vec<VmValue>` NaN-boxed

No hay eliminación abrupta — el NaN-boxing existente se vuelve el fallback para
`Dynamic` y permanece disponible indefinidamente.

---

## ESTRUCTURA DE ARCHIVOS NUEVOS

```
crates/
├── varn-core/src/
│   ├── intrinsics/
│   │   ├── mod.rs          — reexporta IntrinsicOp, INTRINSIC_MAP
│   │   ├── ops.rs          — enum IntrinsicOp
│   │   └── map.rs          — tabla static str → IntrinsicOp
│   └── module_id/
│       ├── mod.rs          — reexporta ModuleId (sin cambios)
│       └── idx.rs          — ModuleIdx(u32)
│
├── varn-types/src/chunk/
│   └── register_meta.rs    — SlotKind, RegisterMeta
│
├── varn-compiler/src/
│   ├── analysis/
│   │   ├── mod.rs
│   │   ├── slot_kinds.rs   — infer SlotKind por registro
│   │   └── escape.rs       — escape analysis (closures + objetos)
│   ├── codegen/expr/
│   │   └── intrinsics.rs   — try_emit_intrinsic
│   ├── linker/
│   │   └── module_linker.rs — asignación de ModuleIdx globales
│   └── module_index.rs     — ModuleIndexer por compilation unit
│
├── varn-checker/src/
│   └── analysis/
│       └── intrinsic_resolver.rs — anotar calls con IntrinsicOp
│
├── varn-vm/src/
│   ├── exec/intrinsics/
│   │   ├── mod.rs          — dispatch_intrinsic
│   │   ├── math.rs         — impl math intrinsics
│   │   ├── string.rs       — impl string intrinsics
│   │   ├── array.rs        — impl array intrinsics
│   │   └── types.rs        — impl type check intrinsics
│   ├── arena.rs            — FrameArena bump allocator
│   ├── typed_frame.rs      — TypedFrame con slots nativos
│   └── exec/abi_boundary.rs — conversión TypedFrame ↔ VmValue
│
└── varn-jit/src/codegen/
    ├── emit_intrinsics.rs  — inline emit por IntrinsicOp
    └── typed_emit.rs       — variantes sin boxing para SlotKind::Int/Float
```

---

## ORDEN DE IMPLEMENTACIÓN

### Sprint 1 — Base y victorias rápidas
```
[ ] Fase 1.3 — ModInt, PowInt, ModFloat, PowFloat (bajo riesgo, completa typed arith)
[ ] Fase 1.1 — SlotKind + RegisterMeta en FunctionProto (fundación para todo)
[ ] Fase 1.2 — Llenado de register_meta en compiler
```

### Sprint 2 — Intrínsecos (mayor impacto observable)
```
[ ] Fase 2.1 — IntrinsicOp enum + INTRINSIC_MAP
[ ] Fase 2.2 — intrinsic_resolver.rs en checker
[ ] Fase 2.3 — try_emit_intrinsic en compiler
[ ] Fase 2.4 — dispatch_intrinsic en VM (math, string, array)
[ ] Fase 2.5 — emit_intrinsics.rs en JIT
```

### Sprint 3 — Escape analysis
```
[ ] Fase 3.1 — escape::closure (lambdas sin capturas → LoadStaticFn)
[ ] Fase 3.2 — escape::object (scalar replacement objetos pequeños)
[ ] Fase 3.3 — FrameArena (objetos con lifetime de frame)
```

### Sprint 4 — JIT guiado por tipos
```
[ ] Fase 5.1 — typed_emit.rs (AddInt sin boxing)
[ ] Fase 5.2 — aot.rs (compilar funciones monomorphic en precompile)
```

### Sprint 5 — Módulos y runtime final
```
[ ] Fase 4.1 — ModuleIdx numérico
[ ] Fase 4.2 — module_linker.rs
[ ] Fase 6.1 — TypedFrame
[ ] Fase 6.2 — abi_boundary.rs
```

---

## MÉTRICAS DE ÉXITO

| Sprint | Métrica | Baseline | Objetivo |
|--------|---------|----------|----------|
| Sprint 1 | `cargo check` limpio | ✓ | ✓ |
| Sprint 2 | `NativeFn` allocs/run | 273 | 0 código tipado |
| Sprint 2 | `Module` allocs/run | 62 | < 10 |
| Sprint 2 | `abs(x: float)` instrucciones JIT (std:math) | ~15 | ~3 (fabs inline) |
| Sprint 3 | `Closure` allocs/run | 509 | < 50 |
| Sprint 3 | `Object` allocs/run | 353 | < 80 |
| Sprint 4 | `fib(30)` execute p50 | baseline | -40% |
| Sprint 4 | JIT warm-up runs needed | 1+ | 0 (AOT) |
| Sprint 5 | module import hot path | hash O(1) avg | array O(1) worst |
| Sprint 5 | `int+int` instruction count | unbox+add+box | add |

---

## RIESGOS

| ID | Riesgo | Mitigación |
|----|--------|------------|
| R1 | ABI boundary TypedFrame ↔ VmValue introduce overhead en llamadas mixtas | Boundary solo en call edges; hot paths son monomorphic |
| R2 | Escape analysis incorrecto → uso después de liberar (arena) | Conservador: en duda, no aplicar. Arena solo con evidencia completa |
| R3 | ModuleIdx inconsistente entre compilation units | Linker asigna idx globales; idx locales solo en unidades aisladas |
| R4 | Intrínsecos rompen code que toma `abs` (std:math) como valor de primera clase | Condición de activación requiere call directo; `let f = abs` → path normal, sin intrinsic |
| R5 | Async/generators complican escape analysis | Excluir explícitamente funciones async/generator del análisis en v1 |
| R6 | SlotKind incorrecto si checker tiene false positives | SlotKind::Dynamic como fallback; nunca unsound, solo subóptimo |

---

## INVARIANTES QUE NO ROMPER NUNCA

1. **Código `Dynamic` funciona igual que hoy** — ningún cambio afecta el path dinámico.
2. **Tests 686/686 pasan en cada commit** — ningún sprint termina sin test suite verde.
3. **`vn run` produce output idéntico antes y después** — refactor no cambia semántica.
4. **Cada archivo nuevo < 300 líneas** — si crece, split por dominio.
5. **Sin god-files** — ningún archivo existente crece más de 50 líneas por sprint.
