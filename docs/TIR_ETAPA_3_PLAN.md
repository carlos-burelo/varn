# TIR · Etapa 3 — el corte

Rama `tir-stage-3-the-cut`, desde `main @ 040b5995` (Etapa 2 completa + D-2).

Continúa `TIR_ETAPA_2_PLAN.md`. Etapa 2 cerrada: el checker emite un
`varn_tir::TirModule` verde para los 188 módulos del corpus, `NotYetSupported
= 0`, ratio de despacho estático ~69 %. `hir/` se puede borrar — el riesgo
"TIR azucarado" no se materializó.

Esta etapa **reengancha `ssa/build` a TIR y borra HIR**. El corpus queda
**rojo** entre esta etapa y la 4 (decisión 3 del contrato): sin ejecución, el
único instrumento es `vn debug -p bytecode` (compila sin ejecutar) + el
verificador TIR + `cargo test`.

---

## 1. Estado medido de la superficie de ruptura

* **47 archivos** de `varn-compiler/src/` mencionan `HirExpr` / `HirStmt` /
  `HirType`.
* `ssa/ir.rs` usa `HirType` como el tipo de todo `Value` y de varios campos de
  `InstKind`; `InstKind::MakeClosure` lleva `Rc<HirFunction>`.
* `ssa/build/{expr,stmt}/*` (11 archivos, ~3150 líneas) hacen `match` sobre
  `HirExpr` / `HirStmt`.
* `varn-jit/src/clif/` — >20 archivos leen `SlotKind` / `RegisterMeta`.
* `varn-lsp/features/compiler_inspect.rs` (965 líneas) contra HIR.
* `varn-debug/{hir,ssa}.rs` — los volcados.

## 2. Decisión sobre `HirType` en el SSA IR

`ssa/ir.rs` necesita *un* tipo para los `Value`. Opciones:

* **A** — `Value: BackendTy`. El SSA IR habla el mismo tipo semántico que el
  TIR. Los pases (`const_fold`, `cse`, …) siguen razonando sobre él.
* **B** — `Value: K` (la representación física de `DISENO_IDEAL.md` §1:
  `I64`, `F64`, `I8`, `Ptr`, `Pair(valor,bit)`, `Boxed`). El SSA IR ya es
  post-lowering-de-tipos.

**Elegido: A.** `K` es asunto del emisor de bytecode y del JIT (contrato §8:
"`K` deja de ser un sistema de tipos paralelo y pasa a ser la representación
física derivada de `BackendTy`"). El SSA IR se queda con `BackendTy`; la
proyección a `K` ocurre en `ssa/emit/` y en `regalloc`.

`HirType` (8 variantes: Int/Float/Bool/Str/Ref/Dynamic/Array/…) → `BackendTy`.
`Ref` y `Dynamic` de HIR colapsan a `Dynamic(_)`; `Array(HirType)` →
`Array(TyId)`.

## 3. Orden de ejecución

Cada paso deja `cargo build --workspace` verde salvo donde se indica.

### 3.1 `varn-compiler` habla TIR

`varn-compiler` gana dependencia de `varn-tir`. Nuevo módulo
`varn-compiler/src/from_tir/` — la traducción `TirFunction` → `SsaFunc`,
paralela a `ssa/build/`. Empieza cubriendo el subconjunto trivial (literales,
`Var`, `Binary`, `Return`, `If`, `Loop`) y devuelve `OptError::Unsupported`
para el resto. **Sin cablear**: el pipeline sigue por HIR. Corpus verde.

*Control:* `cargo test -p varn-compiler` sobre `SsaFunc`s construidos a mano
desde `TirFunction` mínimos.

### 3.2 `HirType` → `BackendTy` en `ssa/ir.rs`

Cambio mecánico transversal: `Value` y los campos de `InstKind` pasan a
`BackendTy`. `ssa/build/` (el camino HIR viejo) se adapta con un
`HirType → BackendTy` local. `ssa/dump.rs`, `passes/`, `regalloc/` siguen.
`MakeClosure` pasa a `func: FnId` (o `Rc<TirFunction>`).

*Control:* corpus verde por el camino HIR con el IR ya migrado a `BackendTy`.

### 3.3 `from_tir/` cubre todo el corpus

Portar construcción a construcción hasta que los 188 módulos produzcan
`SsaFunc`. `-p tir` y `-p bytecode` son el instrumento.

### 3.4 El corte

El pipeline (`varn-pipeline/compile.rs`) llama a
`varn_checker::emit::emit_module` y pasa el `TirModule` a
`varn_compiler::compile_from_tir`. Se borra:

* `varn-checker/src/checker_annotations/` (1245)
* `varn-compiler/src/hir/lower/` (4506)
* `varn-compiler/src/ssa/build/` (el camino viejo, ~3150)
* `varn-core/src/{typed_ir.rs, cg_ty.rs}` (278)
* `varn-types/src/register_meta.rs` (17)

`hir/mod.rs`, `hir/inline/`, `ctor_summary`, `module_locals` se **portan** a
operar sobre TIR (o mueren si `from_tir/` los subsume — `inline` y
`module_locals` probablemente se rehacen como pases sobre SSA).

`collect_type_annotations` desaparece de `checker/mod.rs`; `emit_module` ocupa
su sitio.

**Corpus rojo a partir de aquí hasta la etapa 4.**

*Control:* los 188 módulos compilan a bytecode (`-p bytecode` sin ejecutar);
`-p tir:check` limpio; `cargo test --workspace`.

### 3.5 La caché de bytecode

`FunctionProto` cambia de forma (`register_meta: Vec<RegisterMeta>` →
`Vec<BackendTy>` o el `K` derivado). Se sube la versión del formato de caché;
no se migra. `vn cache clean` en las notas de la etapa.

---

## 4. Fuera de alcance (etapa 4 y siguientes)

* El intérprete y el JIT no se tocan en la etapa 3 más allá de compilar. Los
  opcodes que las resoluciones habilitan (`LoadGlobalIdx` directo,
  `InvokeVirtual` con índice, `CallIntrinsic`) y `K` derivado de `BackendTy`
  son la etapa 4 y 5.
* `VmValue` de dos palabras, `ObjData` de 16 bytes, GC de raíces a mano:
  `DISENO_IDEAL.md` §7 pasos 2-5, el bloque siguiente al TIR.

---

## 5. Progreso

- [x] 3.1 `varn-compiler` → `varn-tir`, `from_tir/` skeleton (`ff5d5220`)
- [~] 3.2 tipo del SSA IR. **Revisado:** el SSA IR **conserva su enum de tipo
      actual** (`crate::hir::HirType`, ya con handles `TyId`/`ClassId` + tabla)
      durante la etapa 3; se renombra a `SsaTy` cuando muera `hir/`. Evita el
      edit transversal de 47 archivos. Hecho el puente
      `from_tir/ty.rs`: `BackendTy → HirType`, re-internando handles anidados
      en la `TyTable` del lado SSA (por nombre de clase). `char`→`Int`;
      `decimal`/`bigint`/`enum`/`fn`/`map`/`set`/`tuple`→`Ref`. 4 tests.
- [~] 3.3 `from_tir/build.rs` — construcción SSA desde TIR. **En curso.**
      Core SSA-agnóstico copiado de `ssa/build::Builder` (esa versión muere en
      3.4). `TirModule` gana `global_names` para resolver `GlobalSlot(n)`.
      Cubierto: literales, `Var` (Local/Param/Upvalue/GlobalSlot/ByName),
      `Binary`, `Unary`+`IsNull`, `Cast`, `Select`, `Field`
      (`GetFixedField`/`GetProperty`), `Index` (`ArrayGetIndex`/`GetIndex`),
      `Call` (`DirectFn`→`LoadGlobal`+`Call`, si no callee lowered),
      `MethodCall`, `New`/`MakeVariant` (`LoadGlobal`+`Call`), `ArrayLit`/
      `TupleLit`/`ObjectLit`, `Assign` (var/field/index), `Await`. Sentencias
      `Expr`/`Let`/`Return`/`Throw`/`Break`/`Continue`/`If`/`Loop`. 9 tests.
      + `Discriminant`→`GetEnumTag`, `VariantPayload`→`GetFixedField`,
      `TypeTest`→`MethodCall("__instanceof")`, `Yield`→`InstKind::Yield`,
      `Var`+`None`→`This`, `Assign` a `GlobalSlot`.
      `Try` → `InstKind::Try`/`CatchParam`/`PopTry` (sin finally: el emisor ya
      lo aplanó); spread en `Call`/`New`/`Array`/`Object` → las variantes
      `*Spread`.
      **`vn debug -p tir:check` reporta `from_tir: OK (N ssa fn)` por módulo.
      ~157/188 del corpus construyen SSA desde TIR.** Falta (~31 módulos):
      `Closure` (29 — `InstKind::MakeClosure.func` es `Rc<HirFunction>`;
      cambiarlo a un ref TIR-side toca `ssa/emit`/`dump`/`uses`/`dce` y en la
      práctica es parte del corte 3.4), named args (2).
- [ ] 3.4 el corte + borrados (~6000 líneas), corpus rojo
- [ ] 3.5 caché de bytecode
