# FIX_PRE_3B — Consolidar nombres de tipos primitivos antes de Phase 3b

> **Objetivo**: los strings de nombres de tipo primitivo (`"int"`, `"float"`,
> `"str"`, `"bool"`, `"char"`, `"bigint"`, `"decimal"`, `"Array"`, `"Map"`,
> `"Set"`, `"Range"`, `"Symbol"`, …) están **hardcodeados en ~30 archivos
> (~140 ocurrencias)**. Ya existe una fuente canónica, pero está bypasseada.
> Phase 3b añade dispatch **tipado por estos nombres** → multiplicará los
> literales. Hay que rutar todo por la fuente canónica **antes** de avanzar.

---

## 0. Fuente canónica (YA EXISTE — destino de la consolidación)

```
varn-base/src/lib.rs
  └─ enum TypeTag                      ← el enum raíz de todos los tipos
       ├─ fn to_intrinsic_str()        ← TypeTag → "int"/"Array"/...  (nombre de tipo en superficie)
       ├─ fn to_runtime_str()          ← TypeTag → nombre de clase runtime
       └─ fn is_primitive()

varn-core/src/intrinsics.rs
  ├─ struct IntrinsicType(TypeTag)     ← as_str() / from_str()  (string ↔ tag, AMBOS sentidos)
  ├─ struct RuntimeTypeName(TypeTag)   ← as_str() (to_runtime_str)
  └─ enum MemberKey                    ← "length"/"toString"/"next"/...  (nombres de miembro)

varn-core/src/{tag_ext.rs, kinds.rs}   ← TypeTagExt, TypeKind
```

`IntrinsicType::from_str("Array") -> Some(Array)` y `IntrinsicType::Array.as_str()
-> "Array"` ya cubren el mapeo en ambos sentidos. **El problema es que casi
nadie lo usa**: reimplementan el `match` con literales.

---

## 1. Grafo de duplicación

```mermaid
graph TD
    TT["TypeTag (varn-base)<br/>to_intrinsic_str / to_runtime_str / is_primitive"]
    IT["IntrinsicType / RuntimeTypeName / MemberKey<br/>(varn-core/intrinsics.rs)"]
    TT --> IT

    subgraph A["A · Runtime value→nombre (varn-vm / varn-types)"]
      A1["heap.rs:140-151  HeapObj→name"]
      A2["exec/props.rs:199-210  get_class_for_value"]
      A3["exec/props.rs:343-347  get_class fast-path (NUEVO)"]
      A4["exec/dispatch/mod.rs:796-816  typeof"]
      A5["exec/calls.rs:404-407  Value→name"]
      A6["types/value/alloc.rs:220-226  panic names"]
    end

    subgraph B["B · Registro de clases intrínsecas (bootstrap)"]
      B1["exec/ctx.rs:148-163  init_intrinsics[names]"]
    end

    subgraph C["C · Declaración de clase nativa (varn-builtins)"]
      C1["modules/primitives/*/*.rs  class: \"Array\" … (×12)"]
      C2["+ contratos .vn  declare class Array"]
    end

    subgraph D["D · Macro marshalling (varn-op-macros)"]
      D1["varn_contract.rs:86-120  classify()/receiver_mapped()"]
    end

    subgraph E["E · Op-id + Checker (NUEVO + disperso)"]
      E1["varn-core/op_id.rs  core_class() (×12) (NUEVO)"]
      E2["checker_annotations.rs  core_class_of_type (NUEVO)"]
      E3["checker: member_exists / member_type / stmts /<br/>patterns_sum / type_inference / infer_impl"]
    end

    subgraph F["F · intrinsic_ops keys (REFACTOR.md)"]
      F1["intrinsic_ops/{array,string,math,types}.rs<br/>\"std:array/push\" …"]
    end

    subgraph G["G · Debug/cosmético (baja prioridad)"]
      G1["opt/hir/dump.rs, opt/ssa/dump.rs"]
      G2["varn-debug/ast.rs, cli/pipeline/debug/*, pipeline/debug/*"]
    end

    A -.->|debe derivar de| IT
    B -.->|debe derivar de| IT
    D -.->|debe usar from_str| IT
    E -.->|debe usar from_str/TypeTag| IT
    A1 -. duplica .-> A2
    A1 -. duplica .-> A4
    E1 -. duplica .-> IT
```

ASCII (resumen): un único nodo canónico `TypeTag/IntrinsicType` rodeado de
**7 clusters satélite** que reimplementan el mapeo con literales. Varios
satélites se duplican entre sí (heap.rs ≈ props.rs ≈ dispatch typeof).

---

## 2. Inventario por dominio

| # | Archivo:línea | Qué hardcodea | Debe usar |
|---|---|---|---|
| **A1** | `varn-vm/src/heap.rs:140-151` | `HeapObj` → `"Array"/"Range"/"Symbol"/"Map"/"Set"` | `RuntimeTypeName` / nuevo `HeapObj::type_name()` |
| **A2** | `varn-vm/src/exec/props.rs:199-210` | `get_class_for_value`: `Value` → 11 nombres | `RuntimeTypeName::from(tag).as_str()` |
| **A3** | `varn-vm/src/exec/props.rs:343-347` | `get_class` fast-path `"Array"/"str"` *(añadido en perf fix)* | `HeapObj::type_name()` |
| **A4** | `varn-vm/src/exec/dispatch/mod.rs:796-816` | `typeof` → `"int"/"float"/"bool"/"str"/"char"/"bigint"/"decimal"` | `RuntimeTypeName` / `type_name()` |
| **A5** | `varn-vm/src/exec/calls.rs:404-407` | `Value` → `"bool"/"int"/"float"/"str"` | idem |
| **A6** | `varn-types/src/value/alloc.rs:220-226` | panic `"Array"/"Map"/"Set"` | `IntrinsicType::*.as_str()` |
| **B1** | `varn-vm/src/exec/ctx.rs:148-163` | lista de 13 clases a inicializar (`Array,str,int,…,Error`) | lista única derivada del registro |
| **C1** | `varn-builtins/src/modules/primitives/*/*.rs` | `class: "Array"` … (1 por primitivo, ×12) | OK como *declaración* — pero alinear con `TypeTag` (ver §3) |
| **C2** | contratos `.vn` | `declare class Array` | fuente de superficie (TS-like), no tocar |
| **D1** | `varn-op-macros/src/varn_contract.rs:86-120` | `classify()`/`receiver_mapped()`: nombre Varn → `Mapped::*` (incluye alias `"string"`,`"number"`) | `IntrinsicType::from_str` → `tag→Mapped` |
| **E1** | `varn-core/src/op_id.rs` `core_class()` | 12 nombres core *(añadido en Phase 1)* | `IntrinsicType::from_str(n)` + predicado `is_core_class` |
| **E2** | `varn-checker/src/checker_annotations.rs` `core_class_of_type` | `TypeKind::Array→"Array"`, `Named→core_class` *(añadido Phase 2)* | `TypeKind→TypeTag→IntrinsicType` |
| **E3** | `varn-checker` (`member_exists.rs`, `member_type.rs`, `checker/stmts.rs`, `binder/decl_values/patterns_sum.rs`, `binder/type_inference.rs`, `checker_expressions/infer/infer_impl.rs`) | comparaciones sueltas con nombres de tipo | `IntrinsicType::from_str` / comparación por `TypeTag` |
| **F1** | `varn-core/src/intrinsic_ops/{array,string,math,types}.rs` | keys `"std:array/push"` … (registro federado REFACTOR.md, hoy dormido para Array/String) | clave `(TypeTag, MemberKey)` en vez de string |
| **G** | `varn-opt/src/{hir,ssa}/dump.rs`, `varn-debug/src/ast.rs`, `varn-cli/src/pipeline/debug/*`, `varn-pipeline/src/debug/*` | strings de display | baja prioridad; opcional |

**Conteo**: ~83 + ~61 ocurrencias de literales en ~30 archivos (greps en §4).

---

## 3. Riesgo concreto para Phase 3b

Phase 3b ("typed monomorphic wrappers + JIT typed-register passing") necesita
decidir, **por tipo primitivo**, qué convención de llamada emitir
(`fn(i64,i64)->i64` para int, etc.). Si se implementa con la práctica actual,
añadirá **otro** `match name { "int" => …, "float" => …, … }` en:

- macro (emitir el entry point tipado por shape),
- JIT codegen (elegir registros GPR vs XMM por tipo),
- checker/opt (anotar el shape).

= 3+ nuevos clusters de literales. **Por eso consolidar primero.**

---

## 4. Plan de consolidación (antes de 3b)

Orden sugerido, cada paso aditivo y validable con `vn run tests/main.vn`
(663/663) + `vn bench`:

1. **`HeapObj::type_name(&self) -> &'static str`** en `varn-vm/src/heap.rs`,
   derivado de `RuntimeTypeName`. Colapsa **A1, A3, A4, A5** (y A2 vía
   `Value`→tag). Un solo `match` sobre la variante del heap obj.
2. **`IntrinsicType::core_class(name) -> Option<TypeTag>`** (o
   `TypeTag::is_core_class()`) en `varn-core`. Reemplaza **E1** (`op_id.rs
   core_class`) y da a **E2/E3** un único predicado. `op_id::core_method_op_id`
   pasa a tomar `TypeTag`, no `&str`.
3. **Lista única `CORE_CLASSES`** (derivada de `TypeTag`/registro) consumida por
   **B1** (`init_intrinsics`) y por el checker. Mata la lista hardcodeada.
4. **Macro `classify`** (**D1**) → `IntrinsicType::from_str` + un único
   `tag → Mapped`. Mantener los alias `"string"/"number"` en un solo lugar.
5. **(opcional, alinea con REFACTOR.md)** claves de `intrinsic_ops` (**F1**)
   pasan de `"std:array/push"` a `(TypeTag::Array, MemberKey)` — esto además
   despierta el dispatch de Array/String que hoy está dormido.

**Invariante**: tras la consolidación, añadir un primitivo nuevo = tocar
`TypeTag` + su contrato `.vn` + su `varn_contract!` (**C1**) — y **nada más**.
Hoy obliga a tocar ~6 lugares.

**No tocar**: contratos `.vn` (**C2**, superficie del lenguaje) ni los
`class:` de **C1** (son la *declaración* autoritativa, 1 por clase) — pero deben
existir como única declaración, no re-derivarse en runtime/checker.

---

## 5. Verificación

- `cargo build -p varn-cli --bin vn` limpio.
- `vn run tests/main.vn` → **663/663**.
- `vn bench tests/main.vn` → exit 0 (trampa JIT/regalloc).
- Greps de control (deben tender a 0 fuera de la fuente canónica):
  - `rg '"(int|float|str|bool|char|Array|Map|Set|Range|Symbol|bigint|decimal)"' crates --type rust`
  - excluir `varn-base/src/lib.rs`, `varn-core/src/intrinsics.rs`, los `.rs` de
    `modules/primitives/*` y los `dump`/`debug`.
