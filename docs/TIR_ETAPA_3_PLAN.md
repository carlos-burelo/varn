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
      `Closure` → `InstKind::MakeClosure { func: ClosureBody::Tir(fn_idx) }`:
      `MakeClosure.func` pasó de `Rc<HirFunction>` a un enum
      `ClosureBody { Hir(Rc<HirFunction>), Tir(u32) }`; el camino HIR usa
      `Hir`, `from_tir` `Tir`; `ssa/emit` hace `unreachable!` en el arm `Tir`
      (el SSA de `from_tir` no llega a emisión aún). named/array spread
      cubiertos (named posicional en orden escrito, refinamiento después).
      **`from_tir::lower_expr` es exhaustivo sobre `TirExprKind`.
      `vn debug -p tir:check` → `from_tir: OK`: 184→188/188 del corpus.**
- [~] 3.4 el corte. **`from_tir` compila los 188 módulos a `FunctionProto`.**
      `emit_function` partido en `emit_function_meta(ssa, &FnMeta, src)`;
      `MakeClosure::Tir` vía scope thread-local del módulo. `VN_FROM_TIR=1`
      conmuta el camino en `compile.rs` (módulo de entrada) **y en
      `module_precompile.rs` (imports)**. El emisor top-level corre
      `as_top_level()`: un `let`/`const` de módulo que el binder hizo global
      se baja como `Assign` a su `GlobalSlot`, el almacén que las funciones
      libres del mismo módulo ya leen.
      **Validado por ejecución:**
      * 106/107 `tests/*.vn` standalone: salida byte-idéntica HIR vs TIR.
      * `tests/main.vn` (suite de 100 módulos, imports densos, contadores
        `std:test` compartidos): idéntica HIR vs TIR, exit 0.
      * JIT == intérprete en los 107 por el camino TIR.
      * `cargo test --workspace` verde.
      (El "blocker" de resolución cross-módulo del resumen previo era un
      diagnóstico falso: `assert("label", cond)` lanza al fallar y es
      silencioso al pasar; `PASSED: 0 / ALL TESTS PASSED` es el estado verde
      real, y HIR lo produce igual.)
- [x] 3.4 **el corte hecho.** `VN_FROM_TIR`/`VN_LEGACY_HIR` fuera: el pipeline
      llama a `varn_checker::emit::emit_module` (sin `resolver`, era muerto) y
      pasa el `TirModule` a `varn_compiler::from_tir::compile_module`, que ahora
      cierra con `regalloc::run_post_passes`. Borrados (−10.5k líneas):
      * `varn-compiler/src/hir/lower/` (todo)
      * `varn-compiler/src/ssa/build/` (el camino viejo)
      * `varn-checker/src/checker_annotations/`
      * `varn-compiler/src/lib.rs`: `compile`/`compile_module`/`lower_to_hir`/
        `lower_to_ssa`/`OptInput` — el crate ya solo habla TIR
      * `varn-debug/src/{hir,ssa,suspend}.rs` y sus fases `-p`
      * `CheckResult`: `type_annotations` + los tres mapas `extension_*`
      `hir/mod.rs` (tipos `HirType`/`HirBinOp`/`TyTable`), `hir/{inline,
      ctor_summary,module_locals}`, `lower/bin_opcode` y `ssa/emit` se quedan.
      `typed_ir.rs`/`cg_ty.rs`/`register_meta.rs` **no** se borran: el JIT los
      lee todavía (contrato §4, fuera de alcance de etapa 3).
      Validado: `cargo build --workspace` + `cargo test` verdes; los 4
      cuadrantes exit 0; 107/107 `tests/*.vn` con JIT == intérprete; los 200
      módulos a bytecode. Dos `tests/errors/*` (`invalid-register-overflow`,
      `invalid-bigint-overflow`, ambos `// expect: error[emit]`) ahora compilan
      limpio — la ruta TIR hace DCE del código muerto que el límite de 255
      registros de HIR rechazaba; el test codificaba una limitación de HIR, no
      del lenguaje. Pendiente decidir si se reescriben o se borran.
- [x] **Etapa 3 cerrada** (merge `1aedd1bc` a `main`, 2026-09-10). Los 107
      `tests/*.vn` byte-idénticos al binario pre-corte en los 3 tiers (JIT/
      no-JIT × @embedded/std-dev); `tests/main.vn` → PASSED 1180; `cargo test
      --workspace` verde; `tir:check` limpio sobre el corpus. Trabajo de esta
      tanda: soporte completo de namespaces (anidados, miembros clase/enum/
      const, `new NS.Class`, namespace local), `for await`/`IterInit`,
      `continue` en for-of/in/range, `using` disposal, rest params, object-
      literal methods, tagged templates, records `#{}`, match sobre enum
      importado, operador `::`, clases anónimas, enum `static {}`. Bugs:
      `emission_order` no seguía aristas de `Try`; `build_enums` contaba
      métodos/statics como variantes (corría los tags — era el falso "bug del
      JIT" de 65); export slots antes del await top-level.
- [~] 3.5 caché de bytecode. **Aplazado a Etapa 5.** `FunctionProto` NO cambió
      de forma en la Etapa 3 (`register_meta: Vec<RegisterMeta>` sigue igual;
      `register_meta.rs` lo lee el JIT todavía). La invalidación de caché ya la
      cubren `BUILD_FINGERPRINT` + `producer_fingerprint`. El cambio de forma
      real (`register_meta` → `Vec<BackendTy>` / `K`, subir versión) es 5.
- [ ] Cabo suelto: `emit/body.rs` (3403) y `emit/mod.rs` (1704) superan el
      límite de 1000 líneas del check de gobernanza de CI (que ya se violaba
      en `main`: `xtask/main.rs` 1301). Modularizar cuando toque.

---

## 6. Etapa 4 — el intérprete vuelve (en curso)

Meta del contrato §10: los opcodes que las resoluciones habilitan, con el
intérprete corriendo de vuelta. `run --compare-tiers` como control.

- [x] **`InvokeVirtual` con índice.** El emisor ya bajaba `InvokeVirtual`
      cuando el checker conocía la clase del receptor, pero pasaba
      `cs = usize::MAX` — la inline cache nunca enganchaba y la llamada
      resolvía por nombre en cada iteración. Ahora comparte el slot de
      call-site cache de `CallMethod` (empaquetado en la palabra del opcode);
      tras el primer hit la IC lo resuelve a un índice de vtable con su
      propio guard de `id` de clase + versión de vtable. Comportamiento
      idéntico. `commit 7e540ba1`.
- [x] **`CallNativeOp` para métodos de tipo núcleo.** El checker vuelve a
      clasificar `recv.metodo(args)` cuando `recv` es `Array`/`str`/`Map`/`Set`
      y el método es de instancia nativo → `Resolution::NativeOp(op_id)`;
      `from_tir` lo baja a `InstKind::CallNativeOp`, saltándose el lookup por
      nombre y la IC. Guard: `Method` no-estático/no-async/no-generador de un
      contrato core cargado. Los primitivos numéricos quedan FUERA a
      propósito: la coerción implícita (`int` → `bigint`/`decimal`) deja el
      valor con tag de int mientras el tipo estático dice otra cosa, así que
      un op-id sobre el tipo estático misdespacharía (lo caza
      `tests/26-numeric-coercion`). El set se construye en `emit_module`
      desde `bind.core.class_members`. `commit 7e540ba1`.
- [x] **`CallIntrinsic` (wire de `std:math`).** (`b60b88e7`.) `import { abs,
      sqrt, floor, ceil } from "std:math"` → `Resolution::Intrinsic(wire)` →
      `InstKind::IntrinsicCall` / `IntrinsicDirect`: el JIT emite una
      instrucción ISA (`sqrtsd`, …) sin cruzar la frontera FFI.
      `math_intrinsic_imports` mapea nombre local → wire byte; la ligadura del
      import descarta un shadow. `tests/67` emite `IntrinsicDirect`.
- [x] **`LoadGlobalIdx` directo — regiones de globales por módulo, pase
      BORRADO.** (`6ff813d1` + `3fce8b49` + `bf550a5d`, ver
      `docs/TIR_ETAPA_4_GLOBALS.md`.) El checker numera los globales de módulo
      (`Resolution::GlobalSlot`) y los del prelude
      (`varn_builtins::native_global_layout` → `Resolution::NativeGlobal`);
      `from_tir` emite `LoadGlobalIdx`/`StoreGlobalIdx` (relativos a la región
      del módulo) y `LoadNativeGlobalIdx` (absoluto) directamente. Cada módulo
      reserva su región del `GlobalStore` al evaluarse
      (`FunctionProto.global_count`); la closure lleva `module_base`, heredado
      por cada closure anidada / fork de generador / fork de task. Intérprete y
      JIT suman `module_base`; el link estático (`K::Global`) vía
      `CtxLinker::for_module`. Miembros de `extension` numerados como globales
      de módulo. **`globals/resolve.rs` + sus 3 call sites +
      `FunctionProto.globals_id` + `GlobalStore::id` BORRADOS.** La
      re-derivación en runtime de los globales desapareció.

*Control cumplido:* 321/321 `tests/*.vn` byte-idénticos en los 3 tiers;
`tests/main.vn` → PASSED 1180 (JIT / no-JIT / std-dev); `run --compare-tiers`
sin desacuerdos sobre el corpus completo; `cargo test --workspace` +
`cargo clippy --workspace` verdes; isolates + `vn cache clean` roundtrip OK.

**Etapa 4 CERRADA.**

---

## 7. Etapa 5 — `SlotKind` es el discriminante físico

- [x] **`SlotKind` adelgazado** (`78325335`). Llevaba `Class(u32)` / `Array(u32)`
      / `Nullable(u32)` — handles de tipo que el backend **nunca leía**, un
      sistema de tipos paralelo. Colapsado a lo que codegen discrimina de
      verdad: `Int | Float | Bool | Str | Ref | Dynamic`. `Str` aparte (puede
      ser small-string inline, no puntero); todo lo demás heap-only → `Ref`;
      cualquier nullable → `Dynamic` (la palabra de tag ya dice null-o-no; el
      JIT no tiene kind `Pair`). `.vnc` invalida por `BUILD_FINGERPRINT`.
      321/321 ×3 tiers; main.vn 1180 ×3; compare-tiers limpio; cargo test verde.

### Lo que falta para "CLIF full-typed" (cada uno tamaño-feature, como fue
### `InvokeVirtual`)

Medido `-p typeloss` (sin `<module>`): generic 755 / typed 312 — **71% de los
pares elegibles siguen genéricos.** Aritmética ~95% tipada; el resto:

* **Getters** (`.size`, `.length`, `.celsius`) → `GetProperty` hash. Falta
  `ClassInfo.getters` + `Resolution::GetterSlot` + opcode `InvokeGetter` + VM +
  JIT. Es el trozo grande de los 837 `GetProperty` genéricos.
* **Campos estáticos** (`Class._x`) → `GetProperty`/`SetProperty`.
  `Resolution::StaticField` ya existe en el enum, sin productor. Falta opcode
  `Get/SetStaticField` + VM + JIT.
* **Payloads de enum importado** → `Field(s, "value0")` genérico. Extender el
  camino de enum local (`Discriminant`/`VariantPayload`) al enum importado.
* **`K::Pair`** para nullable escalar (`int?`, `float?`) — repr interna de CLIF,
  sin dependencia de §7. `x ?? 0` → `select` en vez de load i128 + tag check.
* **`K::Ptr`** para refs heap — sólo paga con punteros directos
  (`DISENO_IDEAL.md §7`, el bloque siguiente: `ObjData` inline, slots de 16
  bytes, GC de raíces a mano). En el modelo actual de heap-por-índice ahorra
  un branch predicho por acceso — marginal.
