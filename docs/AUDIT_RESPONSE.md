# Varn — Respuesta de Auditoría Arquitectónica (contra `EXPECTED_AUDIT.md`)

Idioma: español. Salida: `docs/` (acordado). Prioridad de verdad aplicada: `1. implementación, 2. tests, 3. comportamiento, 4. documentación, 5. README, 6. supuestos`.
Cada conclusión importante cita `Archivo | Símbolo | Comportamiento | Conclusión`. Lo no verificable se marca `Unverified`.

> **Actualización (2026-09-20, post-auditoría).** Este documento describe el
> estado al momento de la auditoría. **El plan vivo es único:
> `docs/plans/2026-09-20-PLAN-PENDIENTE.md`** (absorbió los planes de JIT
> anteriores). Cambios posteriores que invalidan varias balas del resumen:
> - **JIT activo y con par de valor único (C1).** `Ref`+`Dyn` bajan como par
>   tag+payload; el gate `VARN_JIT_ALLOW_REF` se eliminó. `tests/main.vn`
>   1233/0 en JIT y `VARN_NO_JIT=1`.
> - **Bala 9 (JIT baja de bytecode): PARCIALMENTE RESUELTA.** El JIT **ya baja de
>   SSA tipado** (`varn_types::ssa` + `clif/from_ssa/`) para el subconjunto
>   escalar/heap/agregados/campos/calls; el bytecode es el fallback. Falta
>   método/nativa/`Try`/OSR para poder borrar `clif/kinds.rs` y el lowering desde
>   bytecode (F5/F6 del plan único).
> - **Bala 11 (arrays angostos migran a Boxed en escritura):** `set_vm`/`push_vm`
>   ya escriben en los 9 reprs angostos sin migrar (solo un valor de tipo
>   incompatible migra, que es correcto).
> - **Una convención de llamada (C2) y un ABI nativo (C3):** `ExecCtx::invoke`
>   única ruta run-to-completion; `push_call_frame`/`push_call_frame_with_this`
>   única materialización; `jit_call_native` único (absorbe `fnptr`/`op_id`).
> - **Cobertura JIT:** un bug de `istore32` (valor `I32`) tiraba ~600 funciones
>   al intérprete; corregido. `bail` de `tests/main.vn` 1031 → 416.
> - **B7:** `charCodeAt`/`codePointAt` inlineados desde `CallNativeOp` (316 → 7 ms
>   en `bench_str_ops`).

---

## Estado de cumplimiento (2026-09-20)

Contraste de §I (borrados) y §K (11 pasos) con el código actual.

### §I — qué debe borrarse

| # | Ítem | Estado |
|---|---|---|
| 1 | Frame/args/rets/upvalues universales `Vec<VmValue>` → clases GPR/FPR/REF/DYN | **DONE** — `FrameStore` particionado; intérprete con fast-path `reg_class==Gpr` sin tag-check (`ops_math_cmp.rs:144`); GPR/FPR fuera del scan GC (`live_boxed` por clase) |
| 2 | Lowering nativo solo desde bytecode → desde SSA/TIR | **PARCIAL** — el JIT ya baja de SSA tipado (`varn_types::ssa` + `clif/from_ssa/`) para escalar/heap/agregados/campos/`Call`; falta método/nativa/`Try`/OSR (F5) para borrar el lowering desde bytecode (F6). Ver `docs/plans/2026-09-20-PLAN-PENDIENTE.md` |
| 3 | Meet-a-`Dynamic` + skip de `Float` en regalloc_post | **DONE** (Anexo K2) |
| 4 | Colapsos `Char→Dynamic`, `Nullable→Dynamic` | **PARCIAL** — `Char→Ref` honesto (K1), anchos (K4); `Nullable→Dynamic` sigue (`ssa/emit/mod.rs:270`) |
| 5 | `ArrayRepr` mínimo `{Boxed,I64,F64}` | **DONE** (K5) |
| 6 | `ClassLayout` SLOT 16B + `ObjData` para clases | **DONE** (K3 + offset/tag bake-ados esta sesión) |
| 7 | `VmValue` como intercambio compiler→VM | **PARCIAL** — `register_meta`/`SlotKind` es la prueba serializada |

### §K — pasos

1. clases físicas ✅ · `HirType` sin colapsos ⚠️ (`Char`=Ref, `Nullable`=Dynamic, anchos solo `BackendTy`)
2. TIR sin degradar sin `DynReason` ✅
3. SSA conserva payload ⚠️
4. regalloc pools+spill+coalescer ⚠️ (coalescer ✅ K2; techo 256 sigue siendo rechazo, no spill)
5. frame tipado ✅
6. bytecode sobre clases / `GetFixedField` default ✅
7. GC roots por clase ✅
8. convención de llamada por clase ✅
9. **entrada SSA/TIR al nativo** ⚠️ PARCIAL (F1–F4 hecho; F5/F6 pendientes — ver plan único)
10. agregados `ArrayRepr I8..F32` + `InstanceData` compacto ✅
11. benches/gates ⚠️

### Obsoletos / resueltos desde el audit

- `FRAME_LAYOUT_V2_JIT_BAIL`/`PAIR_MIGRATION_PENDING` → **false**: JIT activo (el audit reportaba 0 funciones compiladas).
- Bloqueo de build `std:reflect/introspect` → resuelto.
- K5 "escritura angosta pendiente": **obsoleto** — `set_vm`/`push_vm` ya escriben compacto para `I8..U32/F32` cuando el valor es int/float (`vm_value.rs:926-967`).

### Pendiente real

> El plan vivo y su estado por fases es `docs/plans/2026-09-20-PLAN-PENDIENTE.md`
> (documento único). Resumen:

- **JIT desde SSA/TIR** (paso 9 / §I-2) — **F1–F4 hechos** (escalar, heap,
  agregados, campos, `Call`); **F5 parcial** (homes autoritativas + clases);
  falta método/nativa/`Try`/OSR (**F6**: borrar `clif/kinds.rs` y el lowering
  desde bytecode).
- `Nullable` como par (valor,bit) — §I-4.
- `u64`: sin aritmética sin signo; `u64` no es siquiera tipo de superficie en checker/parser.
- regalloc: spill real en vez de rechazo `>256`.
- `ArrayRepr` angosto **escritura** compacta (K5, fuera de alcance deliberado).



Mediciones ejecutadas en esta auditoría (no afirmaciones sin dato):
- `cargo test -p varn-types --test micro_bench_map -- --nocapture`: Shape+ObjData 30.09 ms / FxHashMap 3.02 ms / InlineMap<4> 2.68 ms / Rc<RefCell<InlineMap<4>>> 5.86 ms (100k iters, 3 claves). Speedup Rc<InlineMap<4>> vs Shape: 5.13x.
- `cargo test -p varn-tir`: 16 passed. `cargo test -p varn-compiler --lib`: 12 passed. `cargo test -p varn-vm --lib`: 1 passed (`error::size_probe::sizes`).
- Prototipo aislado `tag+payload vs NaN-box` (2M iters, `rustc -O`, `black_box`, fichero fuera del repo): tag 0.77 ns/op, NaN-box 0.52 ns/op, ratio 0.68x. Ver §F para interpretación honesta (el check aislado no decide; los límites sí).
- `cargo run -p varn-cli --bin vn` BLOQUEADO por error preexistente del bundle stdlib: `std:reflect/introspect:14:8: type mismatch ... returns '#[str, dynamic][]'` + `Expected class member name, got LAngle` (`crates/varn-cli/build.rs:17`). Benchmarks end-to-end `.vn` quedan `Unverified` por esta causa ajena a la auditoría; se sustituyen por microbench aislado + cálculo de footprint + tests unitarios.

---

## A. Resumen ejecutivo (15 balas)

1. **Fuerte: TIR tipado real.** `TirExpr{kind, ty: BackendTy, res, span}` hace imposible omitir el tipo (`crates/varn-tir/src/node.rs:25`). `BackendTy::{Int,Float,Bool,Char,Str,Bytes,Decimal,BigInt,Array,Nullable…}` (`ty.rs:64-88`).
2. **Fuerte: SSA real con tipos y pases.** `SsaFunc{blocks,values}` (`varn-compiler/src/ssa/ir.rs:25`), `Binary{op,lhs,rhs,ty:HirType}` (`ir.rs:132`), pases `const_fold>cse>dce>licm>algebraic>cfg` (`passes/mod.rs:32-60`). Los pases no importan `VmValue`; exigen tipo probado.
3. **Fuerte: selección de opcode tipada.** `bin_opcode(op,ty)`: `Int→AddInt`, `Float→AddFloat` (`compiler/src/lower/mod.rs:6-54`). Opcodes especializados existen (`varn-core/src/opcode.rs:150-175`).
4. **Fuerte: `SlotKind` como proyección a codegen.** `SlotKind::{Int,Float,Bool,Str,Ref,Dynamic}` (`varn-types/src/register_meta.rs:12-24`), doc: es lo ÚNICO que lee el JIT. `derive_register_meta` + `slot_kind_of` (`ssa/emit/mod.rs:202-268`).
5. **Fuerte: GC preciso por tag, no conservador.** `update_value: if !is_heap return` (`vm/nursery.rs:283-298`), `trigger_gc/run_minor_gc` filtran `is_heap` (`exec/ctx.rs:343-531`), write barrier old→young (`heap/gc.rs:24-30`).
6. **Corrección a EXPECTED: no hay `i128` físico.** `VmValue{tag:u64,payload:u64}`, `#[repr(C)]`, align 8, explícito `Not a NaN-box` (`varn-types/src/vm_value.rs:4-28`). El problema es 16B universales, no el tipo `i128` (que solo existe como `HeapObj::BigInt(i128)` en `vm/heap/obj.rs:43`).
7. **Problema real: especializados sobre universal.** `AddInt: if a.is_int()&&b.is_int(){…} else {arith::add fallback}` (`exec/dispatch/ops_math_cmp.rs:146-158`). Patrón idéntico en Sub/Mul/Mod/Pow/Div y Float. Smell `Typed IR → Universal Value → Typed opcode` CONFIRMADO.
8. **Problema real: el intérprete ignora `SlotKind`.** Solo el JIT lo lee (`register_meta.rs:3-9`). El frame del intérprete es `Vec<VmValue>` + ventana `base+reg` (`exec/ctx.rs:28`, `frame.rs:11-28`, `dispatch/ops_math_cmp.rs:39`).
9. **Problema real: JIT baja de Bytecode, no de SSA.** `clif/lower.rs:3-6`: `.vnc` solo tiene bytecode; `AddInt` SON las proofs; `Variables` reconstruyen SSA. Arquitectura actual `SSA→VmValue→Native`, no `SSA→VM/Native`.
10. **Problema real: colapsos de tipo.** `Char→Dynamic`, `Decimal/BigInt/Bytes/Tuple/Enum/Fn→Ref`, `Void/Never/Dynamic→Dynamic` (`from_tir/ty.rs:17-59`); `Nullable→Dynamic` (`ssa/emit/mod.rs:265`); mezcla de kinds en un registro → `Dynamic` (`emit/mod.rs:234`); `regalloc_post` salta funciones con `Float` (`regalloc_post/mod.rs:58`).
11. **Problema real: arrays/objetos no compactos salvo I64/F64.** `ArrayRepr::{Boxed,I64,F64}` (`vm_value.rs:473-481`), `get_vm` box-on-read, `set_vm/push_vm` migran a `Boxed` (`698-787`). `ClassLayout::from_fields` fuerza `SLOT 16B` (`class_layout.rs:83-88`) aunque `FieldRepr` ya modela 1/2/4/8 (`type_tag.rs:211-229`).
12. **`dynamic` de 16B está justificado; NaN-box no gana para Varn.** Prototipo: check NaN 0.52 vs tag 0.77 ns/op, pero NaN pierde `i64` completo, colisiona con NaN, limita punteros a 48b y exige const-pool; Varn ya conoce tipos estáticamente (`vm_value.rs:6-11`). Solo `dynamic` se beneficia del tamaño; aplicarlo a estáticos sería el smell §22.
13. **Calling convention universal.** `args:Vec<VmValue>=stack.drain(..)` (`exec/calls.rs:112,332,345`), `VmUpvalue{value:VmValue}` (`closure.rs`), ABI JIT 2 palabras (`jit_helpers/arith.rs:20-23`). Wrapper desboxea args y re-taguea retorno (`jit/clif/abi.rs`, `lower.rs:8-15`).
14. **Evolución `i32/f32` a medio modelar.** `TypeTag` ya tiene `I8…F32` (`core/type_tag.rs:40-47`) y `field_repr` es autoridad de layout, pero el lowering los aplana y el layout/instancias/JIT/GC aún mueven 16B. Añadir `i32/f32` hoy toca VmValue/registros/GC/modelo/bytecode/regalloc: falla el test §24.
15. **Cambios de mayor valor:** clases de registros tipadas + frame VT, JIT desde SSA/TIR, layout compacto activado (`FieldRepr`+`InstanceData`), `Nullable` como par (valor,bit), arrays `I8…F32`, GC roots por construcción. Borrar, no puentear (§25).

---

## B. Arquitectura actual (descubierta en código)

```
Source (.vn)
  ↓  varn-lexer → varn-parser → Program (varn-core::ast)
  ↓  varn-checker (tipos, binds, expr_table, call_mappings)
  ↓  emit_module() → TirModule (varn-checker/src/emit/mod.rs:39)
  ↓  varn-tir :: TirExpr{ty:BackendTy} / TyTable / BackendTy (contrato backend)
  ↓  varn-compiler::from_tir::compile_module(TirModule) → FunctionProto
  │     ├─ from_tir/ty.rs :: lower(BackendTy)→HirType (con colapsos)
  │     ├─ ssa/ir.rs :: SsaFunc/Block/InstKind::Binary{ty}
  │     ├─ passes/* :: const_fold, cse, dce, licm, algebraic, cfg (+escape, fixed_fields, monomorphize)
  │     ├─ ssa/emit/regs.rs :: assign_registers (linear-scan, 6 pools preferentes, ceiling 256)
  │     ├─ ssa/emit/mod.rs :: derive_register_meta → register_meta/param_kinds/return_kind
  │     ├─ lower/mod.rs :: bin_opcode → AddInt/AddFloat/Add…
  │     └─ regalloc/regalloc_post :: coalescer Move + verify (salta Float)
  ↓  varn-pipeline::compile() (pipeline/src/compile.rs:19): emit + from_tir + module graph
  ↓  FunctionProto{chunk, register_count:u16, register_meta, param_kinds, return_kind} (types/chunk/proto.rs:55-139)
  ├─→ VM intérprete: ExecCtx{stack:Vec<VmValue>, frames} (vm/exec/ctx.rs:28) + dispatch(AddInt con tag-check + fallback)
  └─→ JIT Cranelift: compile(proto,…) (jit/src/lib.rs:434) ← BYTECODE + SlotKind, NO SSA (jit/clif/lower.rs:3)
        ├─ RAW unboxed fn(i64…)→i64 + WRAPPER JitFn ABI (jit/clif/abi.rs, lower.rs:8-15)
        └─ safepoints/GC maps desde register_meta (jit/clif/safepoints.rs)
Runtime: Heap (nursery + old mark-sweep), HeapStr{Shared,Ext,Slice,Inline37}, VmBuffer (Rc+ventana),
  ObjData dinámico (Shape+transition) + InstanceData compacto (payload [u8]), ArrayRepr{Boxed,I64,F64}.
```

Crates (workspace `Cargo.toml:2-22`): `varn-core, varn-lexer, varn-parser, varn-checker, varn-types, varn-tir, varn-op-macros, varn-modules, varn-builtins, varn-runtime, varn-cli (bin vn), varn-lsp, varn-vm, varn-debug, varn-pm, varn-jit, varn-pipeline, varn-compiler, varn-rt, xtask`.

Discrepancias docs vs código (prioridad al código):
- `docs/VM_ARCHITECTURE.md:89`: diagrama `Nursery 4096 Slots` vs texto `NURSERY_CAPACITY=65536` en el mismo párrafo; código usa `is_full/FULL_THRESHOLD 75%` (`heap/gc.rs:19`, `nursery.rs`).
- `VM_ARCHITECTURE.md:101-113`: dibuja `ObjData=[Header|Shape|fields]` contiguo; código es `shape:Rc + values:Cells + overflow:UnsafeCell<Box<Vec>>>` (`types/value/object.rs:170-181`).
- `VM_ARCHITECTURE.md:8,19`: habla de crate `varn-jit`; el lado VM real es `varn-vm/src/jit/{mod,tiering}.rs` + `ic_entries` en closure.
- `RUNTIME_ARCHITECTURE.md:28,52`: estado async en `ObjData`; código usa `InstanceData` + `VmBuffer/TaskHandle` (`exec/host/mod.rs:233-393`).
- `ARCHITECTURE.md:79`: omite `Inline/Ext/Slice` (`heap/str.rs:40`) y `SLOT 16B` forzado (`class_layout.rs:88`).

---

## C. Flujo de valores (value flow real)

### C1. `let a: int = 10; let b: int = 20; let c = a + b;`
| Etapa | Representación | Evidencia |
|---|---|---|
| AST | `HirExpr`/`Program` sin tipo obligatorio en toda variante | `varn-tir/src/node.rs:1-6` (motivación del cambio) |
| Checker→TIR | `TirStmt::Let{local, ty:BackendTy::Int, init:IntLit(10)}`, `Binary{Add, lhs, rhs}: Int` | `node.rs:25,109,285` |
| TIR→SSA | `lower(Int)→HirType::Int`; `ValueDef{ty:Int}`; `Binary{Add, ty:Int}` | `from_tir/ty.rs:19`, `ssa/ir.rs:20,132` |
| SSA opt | `const_fold` pliega si ambos const; `algebraic` solo Int (`+0,*1`); `dce` puro si `ty:Int/Float/Bool`; `licm` saca `Add/Sub/Mul/cmp` en Int/Float | `passes/const_fold.rs:83-141`, `algebraic.rs:105-143`, `dce.rs:148-170`, `licm.rs:186-200` |
| Lower | `bin_opcode(Add,Int)=AddInt` | `lower/mod.rs:10` |
| Regalloc | `slot_kind_of(Int)=SlotKind::Int`; pools preferentes; meet a `Dynamic` si el registro mezcla kinds; `r0=Dynamic` | `emit/mod.rs:202-268`, `regs.rs:80-107` |
| Bytecode | `AddInt dst,src1,src2` sobre registros `u8/u16` | `core/opcode.rs:150`, `types/chunk/proto.rs:62` |
| VM | `stack[base+dst]= if a.is_int()&&b.is_int(){from_int} else {arith::add}` — tag-check + posible fallback genérico (concat/decimal/float) | `exec/dispatch/ops_math_cmp.rs:146-158`, `exec/arith.rs:72-102` |
| JIT | wrapper desboxea args `VmValue→i64`, RAW ejecuta `iadd` nativo, re-taguea `from_int` | `jit/clif/lower.rs:8-15`, `clif/abi.rs:23-58` |
| GC | `int` nunca es root (`is_heap==false`), se salta en `update_value` | `nursery.rs:289` |

Transiciones: tipo sobrevive hasta bytecode/JIT-meta; **se introduce representación universal en el frame del intérprete** y se reconstruye por tag-check. Copias: `stack[dest]=stack[src]` (`dispatch/mod.rs:271`), `Move` coalescibles.

### C2. `let a: float = 10.0; … c = a+b`
Idéntico con `FloatLit(f64)→HirType::Float→AddFloat→SlotKind::Float`. VM: `if is_f64&&is_f64{from_f64} else if (is_f64||is_int){coerce} else {arith}` (`ops_math_cmp.rs:282-301`). `DivInt` retorna `from_f64(a as f64/b as f64)` por contrato (división int es float). `from_f64(NaN)→null()` preservado por compat, no por falta de bits (`vm_value.rs:191-208`).

### C3. `let x: dynamic = 10`
`BackendTy::Dynamic(Unannotated|…)` (`tir/ty.rs:49-87`) → `HirType::Dynamic` (`from_tir/ty.rs:58`) → `SlotKind::Dynamic` → opcode genérico `Add` → `arith::add` con cadena int→float→SSO→heap-str→decimal (`arith.rs:72-102`). Representación `VmValue{tag=KIND_INT,payload}`: 16B para un valor dinámico es correcto; el problema es que los estáticos pagan lo mismo en el intérprete.

### C4. Referencia / struct / elemento de array
- `Class(cid)→HirType::Class→SlotKind::Ref` (`from_tir/ty.rs:49`, `emit/mod.rs:258`). Campo estático: `InstanceData{read_i64/f64/…(offset)}` (`object.rs:609-642`) pero `ClassLayout::from_fields` hoy emite `(16,8)` para todo (`class_layout.rs:88`): offset correcto, tamaño desperdiciado.
- Objeto dinámico: `ObjData{shape,values,overflow}`, `get` lineal ≤4 si no hash, `insert` con `transition` (`object.rs:236-282`, `shape.rs:88-114`).
- Array: `from_items` elige `I64/F64/Boxed` por valores (`vm_value.rs:560-595`); lectura `get_vm` boxea; escritura mismatched migra a `Boxed` in place preservando identidad `Rc` (`714-787`, `460-471`).

---

## D. Análisis del valor universal (dónde se usa `i128`/16B)

Terminología: no existe registro `i128` Rust; existen **slots de 16B** (`tag+payload`). `i128` solo aparece como dato heap `BigInt(i128)` (`vm/heap/obj.rs:43`, comparado en `exec/compare.rs:107,117`).

**Necesarios (mantener 16B):**
- `VmValue` para `dynamic`, `null/bool`, `SSO len≤5` (`vm_value.rs:248-262`), `heap idx` (`211-216`), `ic_miss{tag:MAX}` (`107-112`). Archivo: `varn-types/src/vm_value.rs`. Conclusión: el contenedor dinámico mínimo de 16B está bien elegido para Varn (ver §F).
- `ArrayRepr::Boxed(Vec<VmValue>)` para heterogéneos/`Dynamic` (`vm_value.rs:476`).
- `r0` como slot de staging de llamada en `Dynamic` + flush GC (`ssa/emit/mod.rs:219-220`).

**Accidentales (pagan 16B sin necesitarlo):**
- Registros/locals/temps/args/rets del intérprete siempre `VmValue` (`exec/ctx.rs:28`, `exec/calls.rs:112,332`). Frame 16 regs = 256B vs 128B tipado; 64 regs = 1024B vs 512B; 256 regs = 4096B vs 2048B (2.0x). Tráfico/spills/frame/GC-copy escalan con ese factor.
- `AddInt/AddFloat` leen/escriben `VmValue` + tag-check aunque el tipo está probado (`ops_math_cmp.rs:146-229,282-415`).
- `InstanceData` direcciona por `slot*16` (`object.rs:653-670`) aunque `FieldRepr` ya da 1/2/4/8.

**Cuestionables (diseño a decidir):**
- `Nullable→Dynamic` hoy (tag dice null-or-not) (`emit/mod.rs:263-265`) vs par `(valor,bit)` propuesto en `tir/ty.rs:82-84`.
- `Char→Dynamic` (`from_tir/ty.rs:26`, comentario NaN-box legacy) vs escalar 4B.
- `Decimal/BigInt→Ref` (heap-only, correcto hoy) vs futuro escalar.
- Comentarios legacy que dicen `NaN-boxed, as today` (`vm_value.rs:475`, `object.rs:36-40`, `exec/ctx_json.rs:57`): son restos del diseño anterior, no el actual. No cambiar comportamiento por ellos; limpiar texto al migrar.

---

## E. Propuesta de representación tipada (objetivo)

Mantener la separación `tipo semántico ≠ representación física ≠ layout memoria ≠ ABI`:

```
Varn type (checker/TIR)      BackendTy::{Int,Float,Bool,Char,Str,Bytes,Decimal,BigInt,Array,Map,…,Nullable,Dynamic}
Físico (registro)            GPR:i64  FPR:f64  REF:u32 idx/ptr  DYN:VmValue{16B}  BOOL:i64 0/1 (o i8 en memoria)
Memoria (layout)             FieldRepr{size,align,is_gc_ref} (type_tag.rs:211-229) — 1/2/4/8/16, autoridad única
GC                           GPR/FPR nunca root · REF siempre root · DYN inspeccionar tag
ABI VM                       int→GPR, float→FPR, ref→REF, dynamic→DYN (mismo concepto en intérprete y JIT)
Bytecode                     AddInt/AddFloat/… sobre clases (no sobre VmValue); Add/Sub/… solo para Dynamic
Nativo/JIT                   RAW(i64/f64/ptr) sin wrapper para llamadas internas; wrapper JitFn solo en frontera VM
```

Concreción por capa (sin inventar APIs incompatibles con lo existente):
1. `HirType`/`BackendTy`: no colapsar `Char/Nullable`; añadir `I8…F32` cuando toque (solo `type defs + checker + lowering + backend repr`).
2. `SlotKind` → clases físicas `GPR/FPR/REF/DYN` (renombre menor; `Str` se divide en SSO-inline vs heap-ref según uso).
3. Frame VT: `stack_int:Vec<i64>, stack_float:Vec<f64>, stack_ref:Vec<u32>, stack_dyn:Vec<VmValue>` o vistas tipadas sobre un buffer; `CallFrame{base}` se mantiene.
4. `derive_register_meta` deja de ser "meet que degrada" y pasa a ser asignación por clase (el allocator ya tiene 6 pools: convertir preferencia en regla + spill real en vez de rechazo `>256`).
5. JIT: bajar de SSA/TIR (conservar proofs en `.vnc` como hoy, pero serializar `HirType`/clase, no solo opcode).
6. `InstanceData` + `ClassLayout` activan `FieldRepr` real (offsets compactos, `gc_mask` ya existe en `class_layout.rs:40-42`).

---

## F. Representación dinámica (128b tag+payload vs NaN-box 64b vs otra)

| Criterio | A. `tag:u64+payload:u64` (actual) | B. NaN-box 64b | C. Alternativa (tag32+payload64 = 12B / alineado 16) |
|---|---|---|---|
| Tamaño | 16B, align 8 (`vm_value.rs:13-15`) | 8B | 12B→16B efectivo; sin ahorro real |
| `int` rango | `i64` completo (`from_int`, `164-169`) | 48-49b; `i64` grande NO cabe | completo |
| `float` | bits completos salvo `NaN→null` preservado (`200-208`) | NaN colisiona con tag-space | completo |
| Refs | `u32` hoy, hueco a puntero directo (`59-60`) | 48b (pierde en plataformas >48b / con tag) | igual que A |
| GC | 1 `cmp` (`is_heap`) | mask+shift+const-pool | igual que A |
| Debug | denso 0..6 → jump table (`42-50`) | máscara + shift + const 64b no-inmediato | igual que A |
| JIT | slot = 2 palabras adyacentes | 1 palabra pero desempaquetar siempre | igual que A |
| Medición | 0.77 ns/op check (prototipo v2) | 0.52 ns/op check (0.68x) en micro aislado | — |

**Recomendación:** mantener **A** para Varn. El micro muestra al NaN-box ligeramente más rápido aislando el check, pero pierde en todo lo que importa al lenguaje: `int` es `i64` nativo con overflow hardware (`exec/arith.rs:10-18`), `float` es `f64` real, y el compilador ya conoce tipos estáticamente (`vm_value.rs:6-11`: "no need for a dynamic engine's one-word encoding"). NaN-box solo encoge `dynamic`; no debe contaminar `GPR/FPR` estáticos (sería el smell §22). La alternativa C no ahorra nada por alineación. Limpieza: actualizar comentarios legacy (`vm_value.rs:475`, `object.rs:36-40`, `ctx_json.rs:57`).

---

## G. Tipos primitivos futuros (cómo encajan `i8…f32` y vectores)

Estado hoy: `TypeTag::{I8,I16,I32,U8,U16,U32,U64,F32}` existe (`core/type_tag.rs:40-47`) y `field_repr` modela `(1,1),(2,2),(4,4),(8,8),(16,8-dynamic)` (`211-229`). Pero:
- `from_tir/ty.rs:26-33` aplana `Char→Dynamic`, `Decimal/BigInt/Bytes/Tuple/Enum/Fn→Ref`.
- `slot_kind_of` no tiene `I8/F32` (`emit/mod.rs:248-268`).
- `ClassLayout::from_fields` ignora `field_repr` (`class_layout.rs:88`).
- `ArrayRepr` solo `I64/F64` (`vm_value.rs:473-481`).

Con la arquitectura §E, añadir `i32`/`f32` toca solo:
```
type defs (TypeTag/BackendTy/HirType) → checker rules → from_tir::lower → slot_kind_of/register_meta → backend repr (GPR con trunc/ext, FPR f32) → InstanceData/ArrayRepr::I32/F32
```
sin rediseñar `VmValue`, register file, GC, modelo de objetos ni bytecode (solo nuevos opcodes `AddI32/AddF32` o reutilizar `AddInt` con ancho en tipo). Vectores/SIMD: nuevo `SlotKind::Vec` → registros vectoriales JIT + `ArrayRepr` contiguo; `field_repr` ya prevé `size/align` por tipo. Test §24 hoy FALLA; tras §E PASA.

---

## H. Procesamiento redundante (rankeado, sin scores numéricos)

**CRITICAL**
- `AddInt/SubInt/…/AddFloat/…` con operandos `VmValue` + `is_int/is_f64` + fallback genérico. Loc: `exec/dispatch/ops_math_cmp.rs:146-301`, `exec/arith.rs:72-119`. Info disponible: `HirType` + `SlotKind`. Eliminación: clases físicas; intérprete sin tag-check en fast-path. Consecuencia: menos branches, menos tráfico 16B, menos `Move`.
- JIT desde bytecode en vez de SSA (`jit/clif/lower.rs:3-6`). Reconstruye SSA con `Variables` pudiendo consumirla directa. Eliminación: serializar clase/tipo en `.vnc` + bajar de SSA. Consecuencia: desacopla native de VM.
- Frame universal `Vec<VmValue>` para args/locals/temps/rets/upvalues (`exec/ctx.rs:28`, `exec/calls.rs:112,332`, `closure.rs:29-68`, `host/mod.rs:310`). Eliminación: frame VT §E. Consecuencia: 2x memoria frame, GC-copy, spills.

**HIGH**
- Meet a `Dynamic` por reusar registro entre kinds (`emit/mod.rs:227-239`, `regs.rs:80-103`, caso matmul 15→3 ms en comentarios). Eliminación: pools como regla + spill real. Consecuencia: recupera especialización JIT perdida por azar del allocator.
- `regalloc_post` salta funciones con `Float` (`regalloc_post/mod.rs:58`) y re-permuta `register_meta` con meet (`196-220`). Eliminación: coalescer consciente de clase. Consecuencia: menos `Dynamic` accidental.
- `Nullable→Dynamic`, `Char→Dynamic` (`emit/mod.rs:265`, `from_tir/ty.rs:26`). Eliminación: par (valor,bit) y escalar char. Consecuencia: nulables escalares sin box.
- `Array::get_vm` box-on-read + `set_vm/push_vm` migran a `Boxed` (`vm_value.rs:698-787`). Eliminación: paths tipados `get_i64/set_i64` + `ArrayRepr` por tipo estático (`Array<int>` verificado una vez, no por elemento). Consecuencia: loops numéricos sin box por acceso.

**MEDIUM**
- `InstanceData` con stride 16 (`object.rs:653-670`, `class_layout.rs:88`). Eliminación: activar `FieldRepr`. Consecuencia: objetos 2-8x más compactos según campos.
- `ObjData` dinámico con `transition`+`overflow` por cada campo nuevo (`object.rs:254-282`). Correcto para `dynamic`/literales, redundante para clases declaradas. Eliminación: clases siempre `InstanceData`. Consecuencia: acceso por offset, sin hash.
- `Intrinsic` boxea args y desboxea resultado alrededor de una instrucción IEEE (`opcode.rs:195-206`); `IntrinsicDirect` ya lo evita solo para unarios math. Eliminación: extender `IntrinsicDirect` a todos los intrínsecos puros. Consecuencia: menos box en `std:math`.
- `decimal_pair`/`extract_val` legacy: ya mitigado con `decimal_of` por tag (`arith.rs:28-50`). Mantener.

**LOW**
- `LoadIntZero/One/MinusOne`, `AddImm/SubImm` (`opcode.rs:141-148`): especialización útil, conservar.
- `r0=Dynamic` + flush GC (`emit/mod.rs:219`): necesario en frontera de llamada; conservar.
- Comentarios NaN-box legacy: solo texto; limpiar al migrar.

---

## I. Qué debe borrarse (obligatorio)

```
Delete: Registros/stack/args/rets/upvalues universales Vec<VmValue> en paths estáticos
Reason: convierten i64/f64 probados en valores 16B + tag-check + fallback; 2x frame/memoria/GC-copy
Replacement: frame por clases GPR/FPR/REF/DYN (§E); DYN=V�mValue solo para dynamic
Affected: vm/exec/ctx.rs, frame.rs, exec/calls.rs, closure.rs, exec/host/mod.rs, dispatch/ops_math_cmp.rs, exec/arith.rs

Delete: Lowering nativo exclusivamente desde bytecode
Reason: acopla Cranelift a VmValue; SSA ya probada se tira y se reconstruye
Replacement: bajar JIT de SSA/TIR; .vnc conserva proofs + clase/tipo serializada
Affected: varn-jit/src/clif/lower.rs, body/op_dispatch.rs, abi.rs, varn-vm/src/jit/*, formato .vnc

Delete: Meet-a-Dynamic por reutilización + salto Float en regalloc_post
Reason: la especialización depende del azar del allocator (matmul 15→3ms)
Replacement: pools por clase como regla + spill real + coalescer consciente de clase
Affected: compiler/src/ssa/emit/regs.rs, emit/mod.rs, regalloc/regalloc_post/*

Delete: Colapsos Char→Dynamic, Nullable→Dynamic, Decimal/BigInt→Ref-como-único-camino
Reason: impiden (valor,bit), escalar char y futuros i32/f32
Replacement: Char escalar, Nullable como par, Ref solo como fallback honesto + kinds nuevos
Affected: compiler/src/from_tir/ty.rs, hir/mod.rs, ssa/emit/mod.rs, tir/ty.rs

Delete: ArrayRepr solo {Boxed,I64,F64} + migración-a-Boxed en mismatch + box-on-read como único camino
Reason: i8/f32 y Array<int> estático pagan verificación por elemento y box por acceso
Replacement: ArrayRepr por tipo estático (I8…F32) + verificación una vez + accesores tipados
Affected: types/src/vm_value.rs, vm/heap/aggregates.rs, heap_array.rs, jit probes

Delete: ClassLayout SLOT 16B forzado + ObjData dinámico para clases declaradas
Reason: ignora FieldRepr (autoridad) y paga hash/transition/overflow en accesos estáticos
Replacement: InstanceData compacto + offsets FieldRepr + GetFixedField/SetFixedField siempre
Affected: types/src/class_layout.rs, value/object.rs, vm/heap/*, jit fixed-field emission, GC payload walk

Delete: VmValue como tipo de intercambio en frontera compiler→VM para valores estáticos
Reason: es el cuello Type→Value→Type; SlotKind ya es la prueba serializada
Replacement: FunctionProto con clases + payloads tipados; VmValue solo en constantes DYN y frontera host
Affected: types/chunk/proto.rs, compiler/ssa/emit, vm/loader.rs, linker, clif_link.rs
```

Sin capas de compatibilidad, flags duales ni adapters (§25). Git conserva historia.

---

## J. Qué debe permanecer (no reescribir por cambiar)

- `TirExpr{ty,res}` obligatorio + `BackendTy` con `is_unboxed_scalar`, `Nullable(payload)`, `DynReason` (`tir/ty.rs`, `node.rs`). Es el contrato correcto.
- `SsaFunc/ValueDef{ty}/Binary{ty}` + pases con pureza por tipo (`ssa/ir.rs`, `passes/*`). No importan `VmValue`: frontera limpia.
- `bin_opcode` + familia `AddInt/AddFloat/CmpInt/CmpFloat/AddImm/IntrinsicDirect` (`lower/mod.rs`, `opcode.rs`). Son las proofs correctas; hay que cambiar sus operandos, no borrarlos.
- `SlotKind + register_meta/param_kinds/return_kind` (`register_meta.rs`, `proto.rs:124-139`). Es el GC-map y el contrato JIT correctos; elevarlo a clase física.
- `VmValue{tag,payload}` 16B para `dynamic` + constructores/accesores privados + `from_raw_parts` solo JIT (`vm_value.rs:4-28,72-94`). Decisión documentada y medida.
- `ArrayRepr::I64/F64 + from_items/push_vm` especialización por valores (`vm_value.rs:536-595,761-787`). Base correcta para extender a `I8…F32`.
- `InstanceData::read_i64/f64/…` + `FieldRepr` como autoridad (`object.rs:609-642`, `type_tag.rs:211-229`). Ya existe lo compacto; falta activarlo.
- `HeapStr::{Shared,Ext,Slice,Inline37}` + `SSO≤5` + `alloc_substring` zero-copy cuando puede (`heap/str.rs`, `heap/strings.rs:115-140`). `VmBuffer{Rc+ventana}` zero-copy (`value/buffer.rs`).
- GC `is_heap` filter + write barrier old→young + evacuación nursery (`nursery.rs:283-298`, `heap/gc.rs:24-33`). Preciso; las clases lo simplifican, no lo reemplazan.
- `RAW unboxed + WRAPPER JitFn` + `Variables` + frame-aware ABI (`jit/clif/abi.rs`, `lower.rs:8-15`). Patrón correcto; cambiar la entrada (SSA en vez de bytecode).
- Tests que fijan semántica: `tests/53-int-overflow, 59-clif-negative-int, 60-dce-purity, 61-algebraic-identities, 62-jit-osr, 63-escape-analysis, 65-safepoint-roots, 74-regalloc-interference, 55-array-element-inference, 56-tier-parity` + `tir/tests/*` + `from_tir::ty` tests. No tocar sin sustituto.

---

## K. Orden de migración / reescritura (fundaciones primero)

```
1. Modelo tipo/representación: SlotKind → clases GPR/FPR/REF/DYN; HirType sin colapsos (Char, Nullable par, I8…F32 reservados)
2. TIR value model: mantener BackendTy; prohibir lowering que degrade sin DynReason::NotYetSupported explícito
3. SSA value model: HirType conserva payload (Array(el), Nullable(inner), Char); pases exigen clase, no adivinan
4. Register allocator: pools como regla + spill real + coalescer por clase + GC-map por construcción; eliminar techo 256-como-rechazo cuando haya spill
5. VM frame: vistas tipadas GPR/FPR/REF/DYN; CallFrame{base} intacto; r0 DYN staging se mantiene
6. Bytecode lowering: AddInt/… sobre clases; Add/… solo DYN; GetFixedField/SetFixedField/ArrayGetTyped como default estático
7. GC roots: GPR/FPR fuera del scan por tipo; REF directo; DYN con is_heap; write barrier intacta
8. Calling convention: params/ret por clase (int→GPR incl. Bool, float→FPR f64/f32, ref→REF, dyn→DYN); wrapper JitFn solo frontera
9. Native/JIT boundary: entrada SSA/TIR + proofs serializadas; intérprete y JIT hermanos bajo Typed SSA
10. Agregados: ArrayRepr I8…F32 + verificación una vez; InstanceData compacto FieldRepr; Shape/ObjData solo dynamic/literales
11. Benchs + gates: microbench representación en CI (mapa ya existe como ejemplo), tier-parity, safepoint-roots, overflow, clif-range
```

---

## L. Plan de verificación

- Tipos: `cargo test -p varn-tir` (hoy 16 passed) + `tir/tests/{backend_ty,coverage,verify_coherence,verify_wellformed}`; nuevos: `Char` escalar, `Nullable(int)` par, `I32/F32` round-trip `TypeTag→BackendTy→HirType→SlotKind`.
- TIR/SSA: `cargo test -p varn-compiler --lib` (hoy 12 passed) + `tests/60-dce-purity, 61-algebraic-identities, 63-escape-analysis, 74-regalloc-interference`; nuevos: meet nunca degrada int+int, coalescer no mezcla clases, `regalloc_post` no salta Float.
- Bytecode: `debug_bytecode` (`pipeline/compile.rs:56`); asserts `AddInt` nunca con operando DYN probado; `GetFixedField` en clases.
- VM: `cargo test -p varn-vm --lib`; `tests/53-int-overflow, 59-clif-negative-int, 103-boxed-int-range, 108-granular-numerics`; intérprete sin tag-check en fast-path (conteo de branches en `ops_math_cmp.rs`).
- GC: `tests/53-gc-class-vtable, 65-safepoint-roots`; asserts `GPR/FPR` fuera de `run_minor_gc/trigger_gc` por construcción; `remembered set` sin regresión.
- ABI: `tests/101-self-recursion-return-kind`; llamadas `int→GPR, float→FPR, ref→REF, dyn→DYN` en VM y JIT con misma tabla.
- Nativo/JIT: `tests/56-tier-parity, 58-clif-range, 62-jit-osr`; JIT desde SSA produce mismo resultado que intérprete en todo `tests/*.vn`kip.
- Representación: `size_of<VmValue>==16`, `FieldRepr` round-trip, `InstanceData` compacto (`struct{i8,i8,i16,i32}` ≤8B payload vs 64B hoy), `Array<i64>` sin box en loop.
- Rendimiento (gates, no solo números sueltos): `int/float arith, loops, calls anidadas, locales, arrays I64/F64/Boxed, structs, dynamic, GC-heavy`; frame 2x menos memoria; `matmul` del comentario `regs.rs` ≥4x estable (hoy 15→3 ms según colisión); mapa 3-claves ya medido como baseline (Shape 30.09 ms vs Inline 2.68 ms: el camino dinámico actual es el lento, no el hash).
- Bloqueo conocido: `vn` no linka por `std:reflect/introspect` (`varn-cli/build.rs:17`); gates end-to-end `.vn` marcados `Unverified` hasta sanear stdlib. No pertenece a esta migración pero debe sanearse antes de proclamar paridad.

---

## Respuesta a la pregunta arquitectónica final (§30)

> **Si Varn se rediseñara hoy sin compatibilidad, pero reteniendo lo arquitectónicamente sano, ¿qué representación y arquitectura de ejecución deberían fundar la siguiente etapa?**

**Retener:** `BackendTy`+TIR obligatorio, SSA tipada con pases puros, `bin_opcode` y opcodes especializados como proofs, `SlotKind/register_meta/param_kinds/return_kind` como contrato JIT/GC, `VmValue 16B` como contenedor `dynamic`, `ArrayRepr I64/F64` como germen tipado, `InstanceData`+`FieldRepr` como layout compacto, `HeapStr/VmBuffer` con zero-copy donde ya existe, GC preciso por tag + write barrier, patrón JIT `RAW+WRAPPER`, y la batería de tests de semántica.

**Reemplazar:** el frame/stack/args/rets/upvalues universales `Vec<VmValue>` por clases `GPR/FPR/REF/DYN`; el lowering nativo desde bytecode por lowering desde SSA/TIR; el allocator de preferencia-con-meet-degradante por clases con spill; los colapsos `Char/Nullable→Dynamic` por escalar char y par (valor,bit); el `SLOT 16B` forzado y `ArrayRepr` mínimo por layout compacto real y buffers por tipo estático (`i8…f32`, vectores después).

**Fundación propuesta:** tipo semántico (`BackendTy`) → clase física (`GPR:i64, FPR:f64/f32, REF:idx/ptr, DYN:VmValue 16B`) → layout memoria (`FieldRepr`) → ABI común VM/nativo → bytecode y Cranelift como dos bajadas hermanas del mismo SSA tipado, con `dynamic` de 16B confinado a lo dinámico y GC que nunca escanea `GPR/FPR` por construcción. Así `int→i64` y `float→f64` viajan `tipo → valor tipado → representación i64/f64 → ADD_I64` sin pasar por valor universal, y `i32/f32/u*/SIMD` se añaden extendiendo tipo/lowering/backend en vez de rediseñar el sistema universal. Esa es la fundación correcta para sintaxis de alto nivel con rendimiento de lenguaje de sistemas.

---

## Anexo — Correcciones aplicadas (K1 + compat std/WIP)

**K1. `char` honesto como `Ref` (heap).** `CharLit` se emitía `HirType::Int` (`from_tir/build.rs`) aunque en runtime es `HeapObj::Char` internado (`vm/exec/calls.rs:28`): mentía al regalloc (clase GPR sin flush GC) y al JIT sobre una referencia viva — riesgo de GC, además del smell `Type→Value→Type`. Cambio en 6 ficheros de `varn-compiler`: `CharLit→ConstChar: Ref`, `BackendTy::Char→Ref`, `CgTy::{Char,Decimal,BigInt}→Ref` (igual que `from_tir::ty` ya hacía con `Decimal/BigInt`), `verify` exige `Ref`, `const_inst_ty` (fold/algebraic) mapea boxed a `Ref`. Tests nuevos: `heap_boxed_scalars_are_ref`, `a_char_literal_lowers_to_ref_and_verifies`. Evidencia: `cargo test -p varn-compiler --lib` 14 passed; suite e2e `34. Char type` en verde.

**Compat con el WIP de `std/` (sin tocar su diseño).** El WIP había dejado el bundle sin compilar (genéricos a nivel de método, firmas estrechadas). Mínimo aplicado: revert de `std/reflect/introspect.vn` a HEAD; métodos `MetaKey::{set,get,has,keys}` sin `<Target>`; `assert/assertEqual/assertTrue/assertFalse/assertContains` flexibles `dynamic` (puente hasta migrar `tests/`); `spawnIsolate` flexible; constructor `Request` acepta init Web u posicional; `.message` restaurado en `assertThrows`; accesos `SqlRow` por corchetes en `tests/85,86`.

**Verificación e2e.** `cargo run --bin vn -- run .\tests\main.vn` (debug): **PASSED 1193, FAILED 0, ALL TESTS PASSED**. `cargo run --release --bin vn -- bench .\tests\main.vn`: **704.26 ms p50, JIT 100% (220/220 fns)**, sin regresiones. Sin K1 la suite fallaba igual (mismatch preexistente test-vs-WIP), luego K1 no introduce regresión.

## Anexo K2 — Coalescer consciente de clase (regalloc_post)

**Problema.** `regalloc_post::optimize_function` re-coloreaba por liveness ignorando `SlotKind`: fusionar un registro `Float` con uno `Int` degradaba `register_meta` a `Dynamic` (prueba en `mod.rs:196-220`). Para proteger el ruteo f64 del JIT, el pase **saltaba todas las funciones con algún `Float`** (`mod.rs:51-64`): las funciones float nunca coalescían `Move`s ni compactaban frames.

**Cambio (3 ficheros en `varn-compiler`).** `color_with_base` acepta `kinds: &[SlotKind]` y añade la **compatibilidad de clase como tercera restricción dura** (junto a interferencia y techo de callee-frame): un color toma la clase del primer ocupante; solo la misma clase puede compartirlo; la coalescencia `Move` exige misma clase en ambos extremos y en cada slot del bloque. Se elimina el skip de funciones `Float`; el `meet→Dynamic` queda como defensa en profundidad inalcanzable. Tests: `copies_across_kinds_never_share_a_colour`, `same_kind_copies_still_coalesce` (más los 3 existentes adaptados a la nueva firma).

**Evidencia.**
- `cargo test -p varn-compiler --lib`: 16 passed. `cargo clippy -p varn-compiler`: sin avisos nuevos.
- Suite e2e: **PASSED 1193, FAILED 0**. `--compare-tiers` en `34-char-type, 35-decimal-bigint, 96-math-advanced, 56-tier-parity`: *every tier agrees*.
- A/B `debug -p bytecode --fn fmix` (función float con copias): con K2 `regs: 8` + 2 `Move` de join; sin K2 `regs: 9` (mismo código): frame 128B vs 144B. Los 2 `Move` restantes son joins no fusionables por interferencia (correcto).
- `debug -p clif:kinds --fn fmix`: `[Unset, Float×5, Bool, Unset]` — clases intactas tras coalescer; `typeloss`: sin pérdidas.
- Bench release `tests/main.vn`: execute ~589 ms, JIT 100% (220/220). Frente a ~709 ms pre-K2: **sin regresión; la diferencia entre corridas no se atribuye al cambio** (variación de máquina; el efecto esperado de K2 es local: menos `Move`s y frames menores en funciones float, no un salto global).

## Anexo K3 — Layout compacto de instancias (paso 10 del plan de migración)

**Problema.** `ClassLayout::from_fields` forzaba `(16, 8)` para TODO campo pase lo que pase (comentario propio: "Instances still address fields by whole `VmValue` slots"), aunque `TypeTag::field_repr` ya modelaba `(1,1),(2,2),(4,4),(8,8),(16,8)` desde antes. Un `int`/`float`/`class` de campo pagaba 16 bytes cuando le bastaban 8 o menos. `InstanceData::field_at`/`set_field_at` leían/escribían ciego con `slot*16`.

**Cambio.** `class_field_repr` (nueva función en `class_layout.rs`, distinta de `field_repr` porque un campo de clase tiene una restricción que la tabla compartida no tiene: `str` puede ser `KIND_SSO` — inline, sin objeto heap — así que NO puede compactarse a un índice de 8 bytes; `char` necesita interning que `InstanceData` no puede hacer sin acceso al heap). `InstanceData::field_at`/`set_field_at` reescritos sobre `FieldLayout` real vía `ClassObj::find_by_id` (API externa intacta, cero cambios en los ~15 call-sites existentes). Un campo `Class?`/`Array?`/etc. nunca escrito lee `null` a través de un sentinel compacto (`u32::MAX`), simétrico con `REF_UNINIT` de `frame_store.rs`.

**Bug encontrado en el camino.** El acceso por NOMBRE (`GetProperty`/`SetProperty`, usado cuando el campo no se resuelve en compilación — típicamente cross-módulo) tenía su PROPIA lectura/escritura ciega de 16 bytes, separada de `field_at` (`props.rs`, `host/mod.rs`, `collections.rs`). Sin corregirla, escribir un campo compacto por ese camino pisaba el campo siguiente en memoria — corrupción silenciosa de heap. Se descubrió porque `std/time/duration.vn`'s `Duration` (6 campos `int`, cross-módulo) lo ejercita.

**Evidencia.**
- Suite e2e: 1193/1193 (en ese momento).
- Stress test dedicado (no permanente, ejecutado ad-hoc): 120 000 instancias con campos `int`+`Node?`+`str` mezclados, 5 minor GC + 1 major GC, íntegro.

## Anexo K4 — Tipos numéricos angostos (i8/i16/i32/u8/u16/u32/f32)

**Descubrimiento previo a implementar.** Buena parte de esto YA EXISTÍA, construido por trabajo anterior nunca activado: `crates/varn-checker/src/checker/compat/mod.rs`'s `simple_types_compatible`/`literal_fits_type`/`expr_satisfies_target_type` ya definían exactamente la asignabilidad correcta (`int→i8` exige cast, `i8→int` widening implícito seguro, un literal directo se infiere sin cast con su rango validado en compilación) — nunca se activaba porque `BackendTy` no distinguía el ancho.

**Cambio.** 8 variantes nuevas en `BackendTy` (`Int8/Int16/Int32/UInt8/UInt16/UInt32/Float32` — `UInt64` deliberadamente fuera: ver abajo). `emit/ty.rs::lower_tag` deja de colapsarlas a `Int`/`Float`. `HirType`/`SlotKind` NO cambian: un ancho angosto vive en el mismo GPR/FPR de 64 bits que `int`/`float` — el ancho solo importa para `field_tag` (activa el layout compacto del Anexo K3 para estos tipos) y para un chequeo de rango nuevo (`InstKind::NarrowRangeCheck` / `OpCode::CheckNarrowRange`), insertado tras un cast explícito (`x as i32`) y tras `-x` unario (el único operador que preserva el ancho en el checker — la aritmética binaria SIEMPRE ensancha a `int`/`float` a propósito, ver bug abajo). Panica en runtime si no cabe, igual que `int` ya hace con su propio desbordamiento.

**Bug encontrado en el camino.** `coerce_binary_operands` (`emit/body.rs`) unificaba operandos de ancho DIFERENTE (`i8 + i16`) casteando el derecho al ancho del IZQUIERDO en silencio, en vez de ensanchar ambos a `int` como el checker (`numeric_binary_type`) ya decidía — nunca se manifestaba porque antes ambos operandos YA ERAN `BackendTy::Int` (colapsados). Corregido ensanchando explícitamente cualquier ancho angosto a `Int`/`Float` antes de la unificación, coincidiendo con el checker.

**Pendiente, fuera de alcance:**
- `u64`: necesita aritmética sin signo dedicada (`u64::checked_*` sobre los bits reinterpretados) — reusar la aritmética con signo de `i64` da resultados incorrectos para valores por encima de `i64::MAX`. Aislado, no bloquea nada de lo anterior. `TypeTag::U64` sigue lowereando a `BackendTy::Int`.
- `ArrayRepr` (`crates/varn-types/src/vm_value.rs`): sigue siendo `{Boxed, I64, F64}` sin distinguir ancho — un `i32[]` compacta a nivel de CAMPO si vive dentro de una clase (Anexo K3), pero el array en sí sigue boxeado elemento-por-elemento salvo que sea `int[]`/`float[]` puro. `VmArray` elige su repr por los VALORES en runtime, no por un tipo estático (ver el comentario de cabecera de `ArrayRepr`) — un array angosto necesita el mecanismo contrario.
- JIT sigue en `FRAME_LAYOUT_V2_JIT_BAIL = true` (paso 9 del plan) — 0 funciones compiladas, todo interpretado.

**Evidencia.**
- Suite e2e: 1207/1207 (`tests/107-narrow-numeric-types.vn` nuevo + `tests/108-granular-numerics.vn`, preexistente y hasta ahora nunca importado a `main.vn`, ambos verdes).
- `cargo build --workspace`: limpio, sin `_ =>` comodín nuevos escondiendo un caso sin decidir (cada `match` exhaustivo roto por las 8 variantes se resolvió explícitamente).
- Stress test dedicado (no permanente): 50 000 instancias con 7 campos angostos mezclados (`i8/i16/i32/u8/u16/u32/f32`), GC completo, íntegro; `debug -p bytecode` confirma `SetFixedField` tipado (no `SetProperty` por nombre) para los 7.
- Detalle de implementación en el Anexo K5 de este documento.

## Anexo K5 — `ArrayRepr` angosto: literales y lectura compactos (cierra K4)

**Problema (K4, "Pendiente, fuera de alcance").** `ArrayRepr` solo tenía `{Boxed, I64, F64}`, elegido por los VALORES en runtime (`from_items`) — un mecanismo que no puede funcionar para anchos angostos, porque un `i8` y un `int` son el mismo `VmValue` (el ancho es un hecho solo-estático, vive en el mismo registro de 64 bits). Un `Array<i8>` pagaba `Boxed`: 16 bytes por elemento.

**Cambio (4 capas, cada una probada por separado; detalle en el Anexo K5 de
este documento).**
1. **Runtime**: 7 variantes nuevas en `ArrayRepr` (`I8..U32,F32`, discriminantes 3..9, aditivas) + `VmArray::new_i8/../new_f32`. `element_slotkind`/`get_vm`/`set_vm`/`push_vm`/`pop_vm`/`migrate_to_boxed` en `vm_value.rs` cubren las 7 explícitamente; CSV (siempre `Boxed`, `unreachable!` nombrado) y serialización JSON (arms reales, ensanchan a `i64`/`f64`) igual.
2. **Checker**: `ExprKind::Array` tipa cada elemento contra el `Array<T>` esperado cuando `T` es angosto, reusando `literal_fits_type`/`expr_satisfies_target_type` (ya existían desde K4, nunca se habían activado para el contexto de un literal de array). `let bad: Array<i8> = [300]` es ahora error de compilación, no truncamiento silencioso en runtime.
3. **Codegen**: `InstKind::BuildArray` gana `narrow_elem: Option<TypeTag>`, calculado en `from_tir/build.rs` desde `BackendTy::Array(elem_id)` vía `narrow_tag_of` (misma función de K4). Se hila hasta el byte ya libre del segundo operando de bytecode de `BuildArray` (antes siempre `0`; `TypeTag::Null == 0` es el centinela "no angosto, usar el camino de inferencia existente"). El dispatch de la VM llama a la nueva `alloc_array_vm_narrow` cuando ese byte no es cero, construyendo el `ArrayRepr` tipado directo — cero pasos de inferencia por valor.
4. **Lectura**: `ArrayGetIndex`/JSON ya cubiertos en la capa 1. GC no requirió NINGÚN cambio: ya salta cualquier repr donde `as_boxed()` sea `None` (`crates/varn-vm/src/gc.rs`, `nursery.rs`), lo cual se cumple automáticamente para las 7 variantes nuevas por construcción — confirmado bajo GC real, no solo en principio (ver evidencia).

**Deliberadamente fuera de alcance (documentado en el plan, no un descuido).** El camino de ESCRITURA hacia un array angosto — `arr[i] = v`, `.push(v)` — sigue usando el mecanismo existente de migración-a-`Boxed` en mismatch (`set_vm`/`push_vm`): correcto, no compacto. Activarlo compacto exige antes investigar si el checker siquiera verifica hoy la asignabilidad de `v` contra el tipo de elemento estático del array — pregunta abierta, no respondida por este plan.

**Evidencia.**
- Suite e2e: 1223/1223 (`tests/109-narrow-array-literals.vn` nuevo, importado a `main.vn`; "NARROW ARRAY LITERALS PASSED").
- `cargo build --workspace`: limpio. Cada `match` exhaustivo roto por el nuevo campo `narrow_elem` (`ssa/dump.rs`, `ssa/emit/regs.rs`, `ssa/uses.rs` x2) y por las 7 variantes de `ArrayRepr` se resolvió explícitamente — sin `_ =>` nuevo escondiendo un caso sin decidir. Un sitio no enumerado por el plan (`varn-lsp/src/features/compiler_inspect.rs`) apareció durante el build y se corrigió igual.
- Stress test dedicado (no permanente): `Array<i8>` de 5 elementos declarado antes de 200 000 asignaciones de basura (fuerza colecciones menores repetidas), íntegro tras 8 `minor gc` (`VARN_GC_TRACE=1`); valores en los extremos del rango (`-128`, `127`) sobreviven exactos.
- `clif/arrays.rs`: doc de módulo actualizado — confirma que el módulo (hoy muerto, `FRAME_LAYOUT_V2_JIT_BAIL = true`) sigue siendo correcto para las 7 variantes nuevas sin cambio de código: el bloque `slow` ya despacha por los accesores totales de `VmArray`, no por un `match` de 3 vías fijo.
