# Plan de Refactor: Representación Desboxeada Dirigida por Tipos

> **Para workers agénticos:** SUB-SKILL REQUERIDA: usar `superpowers:subagent-driven-development` o `superpowers:executing-plans` para ejecutar tarea por tarea. Los pasos usan checkbox (`- [ ]`).

**Goal:** Que los tipos estáticos de Varn dirijan la **representación de datos en runtime** (arrays y objetos desboxeados, cargados por Cranelift), eliminando el impuesto de NaN-boxing sobre datos cuyo tipo ya se conoce en compile-time.

**Architecture:** Hoy todo dato es `VmValue` NaN-boxed: `Array<int>` = `Vec<VmValue>`, instancia de clase = `Vec<VmValue>` por slot. El checker ya proyecta tipos precisos a `CgTy` (`Array(Box<CgTy>)`, `Class(Rc<str>)`), pero esa info solo elige **opcodes** (`AddInt` vs `Add`), no la **representación**. El refactor extiende la cadena de tipos hasta el layout de datos: buffers crudos `Vec<i64>`/`Vec<f64>` para arrays tipados, slots de campo crudos para clases tipadas, con Cranelift como único backend que los carga. Esta es la ventaja estructural sobre V8: V8 **especula** el tipo con guards+deopt; Varn lo **prueba** en compile-time y se compromete a layout desboxeado con cero guards.

**Tech Stack:** Rust; `varn-checker` (proyección CgTy), `varn-opt` (HIR/SSA/bytecode), `varn-types` (VmValue, VmArray, ObjData, RegisterMeta), `varn-vm` (heap, GC generacional, interpreter, intrinsics), `varn-jit/clif` (Cranelift backend), `varn-jit/codegen` (template JIT, en retiro).

## Global Constraints

- **Correctitud > optimización.** Ninguna tarea se declara hecha sin: `target/release/vn.exe run tests/main.vn` verde en JIT **y** `VARN_NO_JIT=1` (~95% de features). Ambos modos deben imprimir `ALL TESTS PASSED`, 0 fallos.
- **Bench solo en corriente (`main`).** Nada de branches para baselines. Medir con `vn bench <f>.vn` (execute p50/min) contra los `.ts` pareados (`bun`/`node`). Documentar archivo, build, runs.
- **Purgar caché de bytecode** (`vn cache clean`) antes de validar cambios de compilador — el `.vnc` está keyed por hash de fuente, no de compilador.
- **Sin retrocompatibilidad ni rutas legacy paralelas.** Migración con destino claro y fecha de borrado (ver Fase C: matar template JIT). No feature flags como sustituto de refactor.
- **File-size governance:** ningún archivo nuevo/modificado supera 1000 líneas; extraer módulos por dominio.
- **Máquina de medición actual:** Intel i7-1355U (base 1.7GHz, U-series 15W); absolutos throttled, usar **ratios** Varn-vs-JS.

## Baseline (2026-07-23, mismo hardware)

| bench | backend hoy | Varn | Bun | Node | estado |
|---|---|---|---|---|---|
| fib(35) | Cranelift | ~57 ms | ~74 ms | ~102 ms | **gana 1.3–1.8×** |
| matrix 150 | template (bail clif) | ~56 ms | ~12 ms | ~11 ms | pierde ~5× |
| gc_alloc | template | ~115 ms | ~53 ms | ~58 ms | pierde ~2× |
| math loop | template (bail Intrinsic) | ~49 ms | ~11 ms | ~9 ms | pierde ~5× |
| dto | intérprete (top-level) | grande | — | — | pierde mucho |

**Diagnóstico raíz confirmado en código:**
- `crates/varn-types/src/vm_value.rs:299` → `VmArray(Rc<UnsafeCell<Vec<VmValue>>>)`: elementos NaN-boxed.
- `crates/varn-types/src/value/object.rs` → `ObjData { shape, values: Vec<VmValue> }`: campos inline boxed.
- `crates/varn-jit/src/clif/arrays.rs:172` → `Err("clif: non-int array store")`: clif solo admite store de int (boxed), bailea float/objeto y K-no-probado → cae al template (4–5× lento) o intérprete.
- `CgTy` (`crates/varn-core/src/cg_ty.rs`) **ya** lleva `Array(Box<CgTy>)` y `Class(Rc<str>)`. La info existe; no llega al layout.

---

## Fases y ROI

- **Fase A0** — Cerrar bails de clif en arrays (float + K-state). *Barato, desbloquea matmul/math YA (aún boxed pero inline).* ROI inmediato.
- **Fase A** — Buffers de array **desboxeados** (`Vec<i64>`/`Vec<f64>`). *Palanca estructural mayor: matrix, array_ops, math, y GC (arrays numéricos sin refs → GC no los escanea).*
- **Fase B** — Layout de **objetos** tipados: slots numéricos crudos + bitmap de refs por shape. *dto, gc_alloc, presión de GC.*
- **Fase C** — Cobertura Cranelift (top-level, `LoadStaticFn`, `Intrinsic`) → **matar template JIT**. *Elimina la ruta lenta y su clase de bugs (try/catch).*

Ejecutar en este orden (mayor ROI/menor riesgo primero). Cada fase deja software funcional y medible.

---

## FASE A0 — Cerrar bails de clif en arrays (stepping stone)

Sin cambiar la representación (sigue `Vec<VmValue>`), hacer que clif inline float-stores y no bailee por K-state. Un `VmValue` float ES sus bits f64 crudos y no es heap-ref → no necesita write barrier, igual que int. Desbloquea matmul (que hoy bailea) y arrays float.

**File Structure:**
- Modify: `crates/varn-jit/src/clif/arrays.rs` — `emit_array_set_index` (admitir Float + Bool), `emit_array_get_index` (unbox por SlotKind del dest).
- Modify: `crates/varn-jit/src/clif/kinds.rs` — propagación de K::Int/K::Float a través de `sum = sum + a*b` (por qué `sum` no queda probado Int).
- Modify: `crates/varn-jit/src/clif/lower.rs` — si el store sigue bailando por otra causa, registrar el motivo exacto vía `VARN_CLIF_TRACE`.

### Task A0.1: Diagnosticar por qué matmul bailea (`non-int array store`)

**Files:**
- Inspect: `crates/varn-jit/src/clif/arrays.rs:171` (`if state[val_r] != K::Int`), `crates/varn-jit/src/clif/kinds.rs`.

**Interfaces:**
- Consumes: `state: &[K]` (K por registro), `RegisterMeta` (SlotKind del checker).
- Produces: nota en el plan del motivo real (K-state no propaga `sum:Int`, o el valor viene de un path no tipado).

- [ ] **Step 1:** Crear repro mínimo. `printf 'function m(n:int):int{let a=[];for(let i=0;i<n*n;i=i+1){a.push(i)};let s=0;for(let k=0;k<n;k=k+1){s=s+a[k]*a[k]};return s}\nprint(m(150))' > /tmp/mm.vn`
- [ ] **Step 2:** Trazar el bail exacto. Run: `VARN_CLIF_TRACE=1 ./target/release/vn.exe run /tmp/mm.vn 2>&1 | grep 'CLIF BAIL'`. Esperado: identifica el proto y el opcode/motivo.
- [ ] **Step 3:** Leer `clif/kinds.rs` para ver cómo `K` fluye por `AddInt`/`Add` y por `ArrayGetIndex`. Determinar si el dest de `a[k]*a[k]` queda `K::Int` o `K::Dynamic`.
- [ ] **Step 4:** Documentar el motivo (probable: `ArrayGetIndex` marca el dest `K::Dynamic` salvo que `register_meta[dest].kind == Int`, y en la cadena el meta no llega). Escribir hallazgo en este archivo bajo "Notas A0".

### Task A0.2: Admitir stores de Float y Bool (boxed) en clif

**Files:**
- Modify: `crates/varn-jit/src/clif/arrays.rs:157-223` (`emit_array_set_index`).

**Interfaces:**
- Consumes: `state[val_r]: K`, `use_boxed`/`use_int`/`box_int` de `clif/emit.rs`.
- Produces: stores de `K::Float`/`K::Bool`/`K::Int` inline; solo `K::Str`/`K::Ref`/`K::Dynamic` conservan barrier → siguen por el helper (que ya maneja barrier).

- [ ] **Step 1:** Reemplazar el guard `if state[val_r] != K::Int { return Err(...) }` por: para `K::Int`/`K::Float`/`K::Bool` obtener el valor **ya boxed** (`use_boxed`, no `box_int` sobre raw) — un float/bool en un `Variable` ya está en forma boxed cuando su `K` lo indica; para int usar `box_int` sobre el raw. Para `K::Str`/`K::Ref`/`K::Dynamic` mantener el helper (write barrier).
- [ ] **Step 2:** El store inline es idéntico (data ptr + `key<<3` + `store I64`), el valor es el `VmValue` de 8 bytes. No cambia el layout.
- [ ] **Step 3:** Build: `cargo build --release --bin vn`. Correctitud: `vn cache clean && ./target/release/vn.exe run tests/main.vn` (JIT) y con `VARN_NO_JIT=1`. Esperado ambos: `ALL TESTS PASSED`.
- [ ] **Step 4:** Verificar ruteo: `VARN_CLIF_TRACE=1 ./target/release/vn.exe run /tmp/mm.vn 2>&1 | grep CLIF` → ya no `BAIL non-int array store` (puede quedar otro bail; a A0.3).
- [ ] **Step 5:** Commit: `git commit -m "clif: admit float/bool array stores inline (boxed, no barrier)"`.

### Task A0.3: Propagar K::Int/Float a través de aritmética + ArrayGet

**Files:**
- Modify: `crates/varn-jit/src/clif/kinds.rs` (`kind_flow`/`apply_kinds`).

**Interfaces:**
- Consumes: `RegisterMeta[reg].kind` (SlotKind del checker), opcodes `AddInt`/`MulInt`/`ArrayGetIndex`.
- Produces: dest de aritmética entera → `K::Int`; dest de `ArrayGetIndex` sobre array cuyo `register_meta` prueba Int → `K::Int`.

- [ ] **Step 1:** En `apply_kinds`, para `ArrayGetIndex` fijar `state[dest] = K::Int` si `register_meta[dest].kind == SlotKind::Int` (float análogo). Para `AddInt`/`SubInt`/`MulInt`/`ModInt` dest → `K::Int`.
- [ ] **Step 2:** Build + suite (JIT + NO_JIT) verde.
- [ ] **Step 3:** Medir: `./target/release/vn.exe bench benchmarks/bench_matrix.vn 2>&1 | grep -i "Peak CPU\|execute"` y `VARN_CLIF_TRACE=1 ... run benchmarks/bench_matrix.vn | grep 'CLIF ROUTE'` — matmul debe rutear clif. Comparar vs `bun benchmarks/bench_matrix.ts`.
- [ ] **Step 4:** Commit: `git commit -m "clif: propagate Int/Float kind through arith + ArrayGet"`.

**Salida A0:** matmul y math-con-arrays rutean clif (aún boxed). Esperado: matrix pasa de ~5× perdiendo a ~1.5–2× (sigue el box/unbox por elemento; Fase A lo elimina). Registrar el número real.

---

## FASE A — Buffers de array desboxeados (`Vec<i64>` / `Vec<f64>`)

El cambio estructural: `Array<int>` respaldado por `Vec<i64>` crudo, `Array<float>` por `Vec<f64>`. Sin tag por elemento; clif hace load/store directo. Arrays numéricos **no contienen heap-refs** → el GC los omite por completo (gran win para gc_alloc/matrix). Arrays de str/objeto/heterogéneos conservan `Vec<VmValue>` boxed.

**File Structure:**
- Modify: `crates/varn-types/src/vm_value.rs` — `VmArray` pasa a enum de variantes por tipo de elemento.
- Modify: `crates/varn-vm/src/heap.rs` — `HeapObj::Array`, alloc paths, GC scanning (omitir variantes numéricas).
- Modify: `crates/varn-vm/src/exec/intrinsics/collections.rs` (o donde vivan push/get/set/length de Array) — despacho por variante.
- Modify: `crates/varn-opt/src/hir/lower/*` + `crates/varn-opt/src/ssa/emit.rs` — emitir opcode de `BuildArray`/`push` con el tipo de elemento (de `CgTy::Array(el)`), y opcodes de get/set tipados.
- Modify: `crates/varn-core/src/op_id.rs` / bytecode decode — nuevos opcodes tipados o parámetro de tipo en los existentes (autoridad única: `varn_types::bytecode::decode`).
- Modify: `crates/varn-jit/src/clif/arrays.rs` — load/store crudo por variante (sin box/unbox).
- Modify: `crates/varn-jit/src/clif/lower.rs` — `array_layout` probe extendido con discriminante de variante.

### Diseño de la representación (v2 — resuelve identidad y layout)

```rust
// crates/varn-types/src/vm_value.rs  (reemplaza el struct actual)
// UN solo Rc — la identidad y todos los clones comparten la MISMA celda,
// así una migración de variante es visible para todo alias. repr(C,u8) da
// layout definido: discriminante u8 al inicio, payload Vec en offsets
// estables por variante → probe-able desde el JIT.
pub struct VmArray(pub Rc<UnsafeCell<ArrayRepr>>);

#[repr(C, u8)]
pub enum ArrayRepr {
    Boxed(Vec<VmValue>) = 0,  // str/obj/heterogéneo/Dynamic — como hoy
    I64(Vec<i64>) = 1,        // Array<int>   — sin refs, GC omite
    F64(Vec<f64>) = 2,        // Array<float> — sin refs, GC omite
}
```

Decisión de tipo en compile-time: el compilador conoce `CgTy::Array(el)` en el sitio de `BuildArray`/literal. `el == Int` → `I64`, `el == Float` → `F64`, resto → `Boxed`. Si el tipo es `Dynamic` (no probado), `Boxed` (nunca adivinar).

**Regla de migración (cero errores nuevos):** un write con tipo no coincidente sobre una variante tipada (posible vía alias Dynamic: `function f(x){x.push("s")}; f(a)` con `a: int[]`) NO es error de runtime — el runtime **migra en el acto** la repr a `Boxed` (boxea todos los elementos, swap dentro de la MISMA celda `UnsafeCell`, mismo `Rc`, misma identidad/heap idx). Todos los alias ven la migración porque comparten la celda. Los fast paths JIT llevan **guard de discriminante**: si la variante ya no es la esperada, caen al helper genérico — una migración nunca invalida código compilado.

**Invariante de layout para el JIT:** `ArrayRepr` es `repr(C,u8)` — el probe (`JitArrayLayout` del template y `cached_payload`/`array_layout` de clif) lee: discriminante en offset 0 del `ArrayRepr`, y ptr/len del `Vec` de cada variante en offsets fijos verificados por probe empírico al arranque (técnica existente). Los inline paths actuales deben añadir el guard `discriminante == Boxed` en A.1 (comportamiento idéntico: hasta A.4 solo existen Boxed).

### Task A.1: `VmArray` como enum de tres variantes

**Files:**
- Modify: `crates/varn-types/src/vm_value.rs:299-...` (`VmArray`, sus métodos `new/empty/borrow/borrow_mut/len`).

**Interfaces:**
- Produces: `VmArray::{I64,F64,Boxed}`, `VmArray::element_slotkind() -> SlotKind`, `VmArray::len() -> usize`, accessors `get_i64/set_i64/get_f64/set_f64/get_boxed/set_boxed`.

- [ ] **Step 1:** Reescribir `VmArray` como el enum de arriba. Métodos: `len()`, `push_*`, `get_*`, `set_*` por variante; `discriminant()` para el probe. Constructores `new_i64/new_f64/new_boxed/empty_of(SlotKind)`.
- [ ] **Step 2:** `cargo build -p varn-types`. Arreglar todos los call-sites que rompan (compilador guía). Muchos serán en `heap.rs` e intrinsics.
- [ ] **Step 3:** Commit: `git commit -m "types: VmArray as typed enum (I64/F64/Boxed)"`.

### Task A.2: GC omite arrays numéricos

**Files:**
- Modify: `crates/varn-vm/src/heap.rs` (scanning de `HeapObj::Array` en `collect`/`trigger_gc`/minor GC).

**Interfaces:**
- Consumes: `VmArray` variantes.
- Produces: el marcado de raíces/hijos salta `VmArray::I64|F64` (no contienen heap idx); solo escanea `Boxed`.

- [ ] **Step 1:** En el scan de objetos array, `match` la variante: `I64|F64` → no-op (cero refs); `Boxed` → escanear como hoy.
- [ ] **Step 2:** Build + `VARN_NO_JIT=1 run tests/main.vn` verde (interpreter primero, aísla GC).
- [ ] **Step 3:** `bench benchmarks/bench_gc_alloc.vn` — registrar delta (menos escaneo).
- [ ] **Step 4:** Commit: `git commit -m "gc: skip scanning numeric (I64/F64) arrays"`.

### Task A.3: Intrinsics (push/get/set/length) por variante

**Files:**
- Modify: intrinsics de Array (localizar con `grep -rn "fn.*array_push\|jit_array_push\|ArrayGetIndex" crates/varn-vm/src`).

**Interfaces:**
- Consumes: `VmArray` variantes, `VmValue` (para box/unbox en el borde interpreter).
- Produces: interpreter correcto para las 3 variantes (unbox al leer VmValue del stack, guardar raw en I64/F64).

- [ ] **Step 1:** `push`: si la variante es I64/F64, extraer el raw del `VmValue` arg (`as_int()`/`as_f64()`) y push raw; si Boxed, push VmValue. Get: leer raw y **box** al devolver a un slot VmValue del stack. Set: unbox el arg y guardar raw.
- [ ] **Step 2:** Build + suite JIT + NO_JIT verde (esto valida el path interpreter; el JIT aún usa el helper viejo hasta A.5).
- [ ] **Step 3:** Commit: `git commit -m "vm: array intrinsics dispatch per VmArray variant"`.

### Task A.4: Compilador elige variante en BuildArray/literal

**Files:**
- Modify: `crates/varn-opt/src/hir/lower/expr.rs` (array literal / BuildArray), `crates/varn-opt/src/ssa/emit.rs` (emisión del opcode con tipo de elemento).
- Modify: `crates/varn-core/src/op_id.rs` + `varn_types::bytecode::decode` si se añade un byte de tipo de elemento al opcode.

**Interfaces:**
- Consumes: `CgTy::Array(el)` del sitio (anotación del checker, ya disponible).
- Produces: `BuildArray` (y `push` sobre array vacío tipado) portando `SlotKind` de elemento; el runtime crea la variante correcta.

- [ ] **Step 1:** En el lowering de array literal, leer el `CgTy` del array; mapear `el` → SlotKind (Int/Float/otro→Boxed). Emitir el opcode con ese kind (nuevo operando byte, o variante de opcode `BuildArrayI64`/`BuildArrayF64`/`BuildArray`).
- [ ] **Step 2:** Actualizar `varn_types::bytecode::decode` (autoridad de shapes) para el nuevo operando. Actualizar el disasm (`vn debug -p bytecode`).
- [ ] **Step 3:** Interpreter: `BuildArray*` crea la variante. Build + suite JIT+NO_JIT verde.
- [ ] **Step 4:** Validar con `vn debug -p bytecode benchmarks/bench_matrix.vn` que los arrays de matmul emiten variante I64.
- [ ] **Step 5:** Commit: `git commit -m "opt: BuildArray carries element SlotKind → typed VmArray"`.

### Task A.5: clif load/store crudo por variante (el win)

**Files:**
- Modify: `crates/varn-jit/src/clif/arrays.rs` (`emit_array_get_index`, `emit_array_set_index`, `emit_array_length`).
- Modify: `crates/varn-jit/src/clif/lower.rs` (`array_layout` probe extendido con discriminante de variante).
- Modify: `crates/varn-vm/src/frame.rs` (`JitArrayLayout` helper: offsets + campo discriminante).

**Interfaces:**
- Consumes: discriminante de variante en el payload (leído inline), `register_meta[dest].kind`.
- Produces: para I64/F64, load/store directo (`store f64`/`i64`, sin box/unbox); para Boxed, la ruta actual.

- [ ] **Step 1:** Extender el probe `cached_payload`/`array_layout` para leer el discriminante de variante del `VmArray` (un byte/word en el header del payload). Emitir un branch: variante-esperada (fast, raw) vs slow (helper genérico que maneja cualquier variante + OOB/append).
- [ ] **Step 2:** `emit_array_get_index`: si el array es I64 y dest es Int → `load I64` directo (sin el shift-unbox de la línea 145). Si F64 → `load F64` a un Variable float. Guard de variante: si el discriminante no coincide en runtime, slow helper.
- [ ] **Step 3:** `emit_array_set_index`: I64 → `store` raw i64 directo (sin `box_int`); F64 → `store` raw f64. Boxed → como A0.
- [ ] **Step 4:** Build + suite JIT+NO_JIT verde. **Además** validar con `VARN_NO_JIT=1` que el interpreter da idénticos resultados (paridad).
- [ ] **Step 5:** Medir vs baseline: `bench_matrix`, `bench_array_ops`, `bench_math` vs `bun`/`node`. Registrar ratios.
- [ ] **Step 6:** Commit: `git commit -m "clif: raw unboxed load/store for I64/F64 arrays"`.

**Salida A:** matrix/array_ops deberían caer a paridad o mejor vs V8 (Varn hace menos trabajo: sin box/unbox, GC omite). Registrar números reales en este archivo.

---

## FASE B — Layout de objetos tipados (slots numéricos crudos)

`ObjData.values: Vec<VmValue>` mantiene 8 bytes por campo, pero para clases estáticas los campos int/float se guardan **raw** en el slot y un **bitmap de refs por shape** le dice al GC qué slots escanear. Acceso a campo tipado = load directo sin box/unbox.

**File Structure:**
- Modify: `crates/varn-types/src/value/object.rs` — `Shape` gana `field_kinds: Box<[SlotKind]>` (o bitmap de refs); `ObjData` sin cambio de tamaño (slots siguen 8 bytes, pero semántica raw para numéricos).
- Modify: `crates/varn-vm/src/heap.rs` — GC escanea solo slots `Ref/Str/Dynamic` según el bitmap de la shape.
- Modify: `crates/varn-vm/src/exec/dispatch/reg_ops/*` (`SetFixedField`/`GetFixedField`) — leer/escribir raw según kind del slot.
- Modify: `crates/varn-jit/src/clif/fields.rs` — `GetFixedField`/`SetFixedField` inline sin box/unbox para slots numéricos.
- Modify: `crates/varn-checker/src/checker_annotations.rs` — ya calcula slot index de campo (líneas ~787+); añadir el SlotKind por slot a la anotación de shape.

### Task B.1: Shape con field_kinds + GC selectivo

**Files:**
- Modify: `crates/varn-types/src/value/object.rs` (`Shape`), `crates/varn-vm/src/heap.rs` (scan).

**Interfaces:**
- Produces: `Shape::field_kinds() -> &[SlotKind]`; GC scan de objeto salta slots numéricos.

- [ ] **Step 1:** Añadir `field_kinds: Box<[SlotKind]>` a `Shape` (poblado al declarar la clase, desde los tipos de campo del checker — ya en `class_members`).
- [ ] **Step 2:** GC: al escanear `HeapObj::Object`, iterar `values` con `field_kinds`; escanear solo `Ref/Str/Dynamic`.
- [ ] **Step 3:** Build + `VARN_NO_JIT=1 run tests/main.vn` verde.
- [ ] **Step 4:** Commit: `git commit -m "object: Shape.field_kinds + GC skips numeric slots"`.

### Task B.2: SetFixedField/GetFixedField raw en interpreter

**Files:**
- Modify: `crates/varn-vm/src/exec/dispatch/reg_ops/` (localizar `SetFixedField`/`GetFixedField`).

- [ ] **Step 1:** Guardar int/float raw en el slot (sin box) cuando el kind del campo es Int/Float; al leer, box al devolver a un slot VmValue del stack (o dejar raw si el dest register es tipado — coordinar con register_meta).
- [ ] **Step 2:** Build + suite JIT+NO_JIT verde (interpreter path).
- [ ] **Step 3:** Commit: `git commit -m "vm: fixed-field raw store/load for numeric fields"`.

### Task B.3: clif fixed-field inline sin box/unbox

**Files:**
- Modify: `crates/varn-jit/src/clif/fields.rs`.

- [ ] **Step 1:** Para `GetFixedField` de campo Int/Float con receptor `CgTy::Class` probado: load directo del slot (offset conocido) a Variable tipado, sin unbox. `SetFixedField` análogo, sin box, y **sin write barrier** para numéricos.
- [ ] **Step 2:** Build + suite JIT+NO_JIT verde + paridad con NO_JIT.
- [ ] **Step 3:** Medir `bench_dto`, `bench_gc_alloc`, `bench_class_fields` vs `bun`/`node`.
- [ ] **Step 4:** Commit: `git commit -m "clif: unboxed fixed-field access for typed numeric fields"`.

**Salida B:** dto/gc_alloc caen (acceso de campo directo + GC más barato). Registrar ratios.

---

## FASE C — Cobertura Cranelift completa → matar template JIT

Cerrar los bails restantes para que **todo el hot path** ruteé clif, luego eliminar el template JIT (y con él su clase de bugs, ej. el de try/catch de esta sesión).

**File Structure:**
- Modify: `crates/varn-jit/src/clif/lower.rs` — soporte `LoadStaticFn`, `Intrinsic`, y opcodes de módulo top-level.
- Modify: `crates/varn-jit/src/lib.rs:327` — el cap de 250 palabras y su interacción con clif.
- Delete (Fase C final): `crates/varn-jit/src/codegen/**` (template JIT) + `compiler.rs`, tras confirmar cobertura clif.

### Task C.1: clif soporta `Intrinsic` (math nativo)

**Files:**
- Modify: `crates/varn-jit/src/clif/lower.rs` (arm `OpCode::Intrinsic`).

**Interfaces:**
- Consumes: wire byte del intrinsic (abs=0x00/sqrt=0x01/floor=0x02/...), `register_meta[value].kind == Float`.
- Produces: clif `fabs`/`sqrt`/`floor`/`ceil`/`nearest`/`trunc` nativo (bitcast i64↔f64), con `nan→null` para paridad con el intérprete (ver `varn-math-intrinsic-free-import`). Los no-nativos (sin/cos/pow) → helper `jit_dispatch_intrinsic`.

- [ ] **Step 1:** Añadir arm `OpCode::Intrinsic`: para arg float, bitcast i64→f64, aplicar clif op nativo, bitcast→i64, `emit_nan_to_null`, store. Resto → helper.
- [ ] **Step 2:** **Precondición crítica:** clif debe manejar aritmética float **nativa** (no helper `h.add`), o math seguirá lento aun ruteando clif. Verificar/implementar `AddFloat`/`MulFloat`/`DivFloat` nativos en `clif/generic.rs` (hoy usan `h.add` helper). Esta sub-tarea puede ser propia (C.1b).
- [ ] **Step 3:** Build + suite JIT+NO_JIT verde. Paridad `sqrt(-1)`, `floor(-2.5)` JIT==interp.
- [ ] **Step 4:** Medir `bench_math` vs `bun`/`node`. Debe caer ~5× → paridad.
- [ ] **Step 5:** Commit: `git commit -m "clif: native Intrinsic (fabs/sqrt/floor) + float arith"`.

### Task C.2: clif compila el módulo top-level

**Files:**
- Modify: `crates/varn-jit/src/clif/lower.rs` (`LoadStaticFn` y demás opcodes de módulo que hoy bailan: ver `CLIF BAIL <module>: unsupported opcode LoadStaticFn`).

- [ ] **Step 1:** Implementar `LoadStaticFn` en clif (cargar proto constante como closure). Barrer con `VARN_CLIF_TRACE=1 run tests/main.vn 2>&1 | grep 'CLIF BAIL' | sort -u` la lista de opcodes que bailan a nivel módulo; implementarlos uno por uno.
- [ ] **Step 2:** Revisar el cap `code.len() > 250` (`lib.rs:327`): con clif (regalloc madura) subirlo/quitarlo es seguro para clif (a diferencia del template; ver `varn-dto-toplevel-jit-blocker`). Gate: solo elevar el cap por la ruta clif; el template mantiene 250 hasta ser borrado.
- [ ] **Step 3:** Build + suite JIT+NO_JIT verde (esto ejercita módulos top-level grandes por clif — vigilar el bug str-intrinsic reportado en `varn-dto-toplevel-jit-blocker`; si aparece, es un bail/miscompile de clif a arreglar aquí).
- [ ] **Step 4:** Medir `bench_dto`, `bench_array_ops` (top-level) vs `bun`/`node`.
- [ ] **Step 5:** Commit: `git commit -m "clif: compile module top-level (LoadStaticFn + friends), lift cap for clif path"`.

### Task C.3: Matar el template JIT

**Files:**
- Delete: `crates/varn-jit/src/codegen/**`, `crates/varn-jit/src/compiler.rs`, `crates/varn-jit/src/regalloc.rs`, `crates/varn-jit/src/assembler.rs`, `crates/varn-jit/src/loop_hoist.rs` (todo el template x86).
- Modify: `crates/varn-jit/src/lib.rs` (`compile` deja de tener fallback a template; clif o intérprete).

**Precondición:** C.1 + C.2 completas y **toda** la suite rutea clif sin caer a template (verificar: instrumentar temporalmente el path template para abortar/loguear si se invoca; correr suite; debe no invocarse).

- [ ] **Step 1:** Instrumentar `compile_proto` (template) para loguear si se llama. Correr suite JIT. Confirmar 0 invocaciones.
- [ ] **Step 2:** Borrar los módulos del template. `compile()` = clif-o-error→intérprete.
- [ ] **Step 3:** Build (baja tiempo de compilación y superficie). Suite JIT+NO_JIT verde.
- [ ] **Step 4:** Full bench sweep vs `bun`/`node`. Registrar tabla final en README.
- [ ] **Step 5:** Commit: `git commit -m "jit: remove template x86 backend — Cranelift is the sole JIT"`.

---

## Estrategia de Validación (cada tarea)

1. **Correctitud (gate duro):** `vn cache clean && ./target/release/vn.exe run tests/main.vn` (JIT) **y** `VARN_NO_JIT=1 ...` — ambos `ALL TESTS PASSED`, 0 fallos. Diff de output JIT vs NO_JIT idéntico (paridad).
2. **Ruteo:** `VARN_CLIF_TRACE=1 ... run <bench>.vn | grep 'CLIF ROUTE\|BAIL'` — confirmar que el hot proto va por clif.
3. **Performance:** `vn bench <bench>.vn` (execute p50/min, mirar la línea `Peak CPU freq` para descartar throttle) vs `bun/node benchmarks/<bench>.ts` (best-of-N). Registrar **ratio**, no absoluto.
4. **Sin regresión de arranque:** vigilar `Module precompilation (cold startup)` — la compilación eager de módulos grandes (Fase C) sube el cold-start; si duele, considerar compilación lazy/hot-triggered (fuera de alcance, anotar).

## Riesgos y Mitigación

- **Divergencia JIT/interpreter** al desboxear (box/unbox en bordes mal ubicado). Mitigación: gate de paridad NO_JIT en cada tarea; el diff debe ser vacío.
- **GC roots perdidas** si un slot ref se marca numérico por error → use-after-free. Mitigación: el bitmap de refs (`field_kinds`) es la única fuente; tests de GC bajo presión (`bench_gc_alloc`, `tests/53-gc-class-vtable`).
- **Cobertura clif incompleta al matar template** → funciones caen a intérprete (lento) en vez de template. Mitigación: C.3 gateado por "0 invocaciones template en la suite".
- **`Dynamic` mal proyectado** creando arrays boxed donde deberían ser typed → sin regresión de correctitud, solo pierde el win; medir cobertura con disasm.
- **Identidad de objeto** (`===`, Map keys): NUNCA realocar/mover un objeto al cambiar layout (ver `varn-object-identity-is-rc-address`). El refactor cambia el *contenido* de slots, no la dirección del `Rc`.

## Orden de Ejecución Recomendado

A0 (desbloqueo barato, valida la tesis) → A (buffers typed, mayor ROI) → B (objetos) → C (cobertura + matar template). Cada fase es entregable y medible por sí sola. Si A no da el salto esperado en matrix, **parar y re-diagnosticar** antes de A/B/C (la tesis del boxing sería incompleta).

## Notas A0

**A0.1 COMPLETADA (2026-07-23).** El bail `non-int array store` en matmul NO es un bug de propagación en `clif/kinds.rs`: el checker nunca prueba el tipo de elemento de `let a = []` (literal vacío sin anotación — `infer_expr_type` ExprKind::Array devuelve `Dynamic`, binder/type_inference.rs:80-90), así que `a[k]` es `Dynamic`, el compilador emite `Mul`/`Add` genéricos (no `MulInt`/`AddInt`), `register_meta` no puede probar Int para el acumulador, y `kinds.rs` hace exactamente lo correcto con la info que tiene. Confirmado experimental: anotar `let a: int[] = []` hace rutear la función entera en CLIF **sin tocar el JIT** → matmul 53ms (template) → **24.7ms** (clif, aún boxed) vs JS ~10ms. Ruteo clif ≈ mitad del gap; el resto es el boxing (Fase A). Bails restantes reales: `LoadStaticFn`/`DefineGlobalIdx` a nivel módulo (Fase C) y `unproven int return (Mixed)` (mismo origen Dynamic).

**Consecuencia: A0.2/A0.3 originales sustituidas** (la propagación de kinds no era el problema; la admisión de float-store no aplica al caso Boxed/Mixed):

### Task A0.2′ (sustituye A0.2): store no-probado → helper inline, no bail de función entera

**Files:** Modify: `crates/varn-jit/src/clif/arrays.rs:157-223` (`emit_array_set_index`).

En vez de `Err("clif: non-int array store")` cuando `state[val_r] != K::Int`, emitir una llamada inline al helper existente `jit_array_set_fast(exec_ctx, obj, boxed_key, boxed_val)` (el mismo del slow path actual, que maneja barrier/append/OOB). La función deja de bailar completa por UN store no probado; el resto del cuerpo sigue en clif. Gate: suite JIT+NO_JIT verde; `VARN_CLIF_TRACE` confirma que funciones antes bailadas por este motivo ahora rutean.

### Task A0.3′ (sustituye A0.3): inferencia de tipo de elemento para literales vacíos

**Files:** Modify: `crates/varn-checker/src/binder/type_inference.rs` (ExprKind::Array vacío) + flujo de asignación/push en el checker.

`let a = []` seguido de `a.push(int-expr)` (sin ningún push de otro tipo en el scope) debe inferir `Array<int>` — flow-based element inference, o al mínimo: propagar la anotación del declarador (`let a: int[] = []` ya funciona) Y el caso `let a = [1,2,3]` homogéneo. Esto está en el camino crítico de Fase A: si `CgTy::Array(Dynamic)`, el BuildArray de A.4 elegirá la variante Boxed y no habrá win. Diseño concreto a decidir al despachar (opciones: unificación en el binder al ver el primer push tipado; o retro-anotación en checker_annotations al cerrar el scope). Gate: matmul/array_ops SIN anotaciones rutean clif con `MulInt`/`AddInt` emitidos (verificar con `vn debug -p bytecode`).
