# Varn — Plan único (JIT desde SSA / motor tipado) — consolidado

Fecha: 2026-09-20. **Este es el único documento de plan del proyecto.** Absorbe
`2026-09-20-jit-modelo-tipado-unico.md`, `2026-09-19-jit-fase-b-frame-clases.md`
y `2026-09-20-jit-from-ssa-frame-aware.md` (borrados), y se lee junto con
`docs/AUDIT_RESPONSE.md` (auditoría de arquitectura, con evidencia por archivo).
Si algo aquí contradice al código, **el código manda**; entonces hay un bug de
diseño que registrar.

Prioridad de verdad: **código > tests > comportamiento > docs**.

Reglas de trabajo vigentes (`AGENTS.md`): Ley 1 IO invertido · Ley 2 ids
internos no cruzan módulos · Ley 3 una tabla un dueño · Ley 4 determinismo ·
Ley 5 diagnósticos acumulativos · Ley 6 una fuente de verdad por hecho · Ley 7
`match` exhaustivo sin `_` que esconda · Ley 8 diseño absolutista (borrar, no
puentear) · Ley 9 commits atómicos · Ley 10 la ganancia real vence a la
compatibilidad. **Regla de tamaño: >400 líneas obliga a modularizar por
dominio.**

---

## 1. Estado actual (verde)

- `cargo check --workspace --exclude varn-lsp --all-targets`: limpio.
- `tests/main.vn`: **PASSED 1233, FAILED 0** en JIT y en `VARN_NO_JIT=1`, con
  `VARN_CACHE_DIR` limpio (fuerza el round-trip de serialización).
- Tier-parity `56/58/62/65/101` dentro de `main.vn`: verde.
- El JIT baja de **SSA tipado** (no de bytecode) para un subconjunto creciente;
  el bytecode sigue siendo el fallback para el resto.

---

## 2. Lo hecho (histórico consolidado)

### 2.1 Frame por clases (fase B) — HECHO

El frame pasó de `Vec<VmValue>` contiguo (stride 16B) a `FrameStore` particionado
en `gpr: Vec<i64>`, `fpr: Vec<f64>`, `refs: Vec<u32>`, `dyn_: Vec<VmValue>`, con
`base` = id de activación y bases por clase en `FrameAlloc.bases`. Contrato
compartido en `varn-types::register_meta`: `SlotClass { Gpr, Fpr, Ref, Dyn }`,
`FrameLayout`, `REF_UNINIT = u32::MAX`. Homes vía helpers `home_store`/`home_load`
(una sola ruta por hecho). JIT reactivado (`FRAME_LAYOUT_V2_JIT_BAIL = false`).

### 2.2 C1–C5 (motor tipado, `docs/plans/2026-09-20-jit-modelo-tipado-unico.md`)

- **C1 — par de valor para todo heap (`Ref`+`Dyn`)**: `Ref`/`Dyn` bajan como par
  tag+payload (`I128`); gate `VARN_JIT_ALLOW_REF` eliminado. Defectos corregidos
  (condición `Bool` en clase `Dyn`, aritmética escribiendo `I64` crudo en el par,
  `GetFixedField` narrow, `istore32` con `I32` que tiraba ~600 funciones al
  intérprete). `bail` 1031 → 416.
- **C2 — una convención de llamada**: `ExecCtx::invoke(callee, window)` es la
  única ruta run-to-completion; `push_call_frame`/`push_call_frame_with_this` la
  única materialización tipada; `call_vm_window`/`jit_call_native_fast`
  borrados.
- **C3 — un ABI nativo y un marshal**: `jit_call_native` único (absorbe
  `fnptr`/`op_id`); `invoke_native` y `FromVm` únicos.
- **C4 — núcleo semántico**: `state` es proyección de clase; `apply_kinds_flow`
  y `box_for_target` borrados. (La bajada desde SSA es el pilar grande — ver §2.4.)
- **C5 — gates**: `Ref`+`Dyn` globales, sin gate.

### 2.3 K1–K5 (Anexos de `AUDIT_RESPONSE.md`)

- **K1** `char` honesto como `Ref` (heap).
- **K2** coalescer consciente de clase en `regalloc_post` (sin skip de `Float`).
- **K3** layout compacto de instancias (`class_field_repr`, `InstanceData` real).
- **K4** tipos angostos `i8..u32/f32` (`BackendTy` + `NarrowRangeCheck`).
- **K5** `ArrayRepr` angosto (literales/lectura compactos).

### 2.4 JIT desde SSA — F1–F4 HECHO, F5 parcial

Contrato (Ley 2/6/8): `TIR/SSA tipada (varn-compiler) → bytecode (intérprete) y
SSA serializada en `.vnc → varn-jit baja de SSA (CLIF)`.

- **F1 — ABI frame-aware**: `raw_signature(frame_aware)` con
  `(stack, closure, base, exec_ctx, args…)`; `clif_ty` mapea `Ref/Dyn/Str →
  I128`; `base`/`closure`/`exec_ctx` bindeados.
- **F2 — globals + `Call`**: `LoadGlobalIdx` lee `GlobalStore` vía
  `closure.module_base` + `exec_ctx`; `Call` resuelve con el linker
  (`expected_bits`), hace llamada directa leaf→leaf o fallback canónico
  `ExecCtx::invoke` por el helper nuevo `jit_invoke_window` (ventana boxeada en
  pila nativa). Args/return escalares.
- **F3 — heap**: modelo del intérprete — escalares en registros CLIF, **heap en
  su home** (raíces GC) vía `def_heap`/`use_heap`. `ConstNull`, `ConstStr`,
  `IsNull`, `Typeof`, `ToString`, `IsArray`, `GetEnumTag`, `ObjectKeys`,
  `StrConcat`, `BuildStr`.
- **F4 — agregados y campos**: `BuildArray`/`BuildMap`/`BuildObject`/
  `BuildRecord` (helpers de ventana nuevos), `GetIndex`/`SetIndex`,
  `ArrayLength`/`ArrayPush`, `This`, `GetFixedField`/`SetFixedField`,
  `GetProperty`/`SetProperty` (+IC), y **retorno no escalar** (boxeado a
  `jit_native_result`, raw void).
- **F5 — cimiento + clases**: en cuerpos frame-aware, **homes autoritativas para
  todo registro** (escalar incluido); clases cableadas (`MakeClass`,
  `DeclareField`, `DefineMethod`/`DefineStatic`/accessores, `GetSuper`).

**Formato portable** (`varn_types::ssa`): `SsaProto`/`SsaBlock`/`SsaInst`/
`SsaOp`/`SsaBinOp`/`SsaUnOp`/`SsaTerm`, `serde`+`postcard`, sin ids de fase.
`FunctionProto.ssa: Option<Arc<SsaProto>>` (fingerprint automático en
`varn-modules/build.rs`). `ssa/portable.rs` proyecta; `clif/from_ssa/` baja
(modular por dominio: `mod`, `scalar`, `boxed`, `heap`, `heapvalue`, `props`,
`call`, `globals`, `classops`, `term`, todos <400).

**Bug de raíz corregido**: `regalloc_post` re-permuta `register_meta` y el
bytecode; `SsaProto.regs` se permuta con el mismo `mapping` (si no, las homes
apuntan a registros de otra clase → panic `jit_store_home`).

---

## 3. Decisiones cerradas (no re-litigar)

- **Valor runtime**: `VmValue{tag:u64,payload:u64}` 16B, `repr(C)`, align 8, no
  NaN-box. El contenedor de 16B es correcto para `dynamic`; los estáticos no
  deben pagarlo en el intérprete (frame por clases). Medido en
  `AUDIT_RESPONSE.md` §F.
- **Tipo semántico ≠ representación física ≠ layout ≠ ABI**: `BackendTy` →
  `SlotClass {Gpr,Fpr,Ref,Dyn}` → `FieldRepr` → ABI común VM/nativo.
- **Una invocación** (`ExecCtx::invoke`), **una materialización de frame**,
  **un marshal** (`FromVm`), **un ABI nativo**.
- **`Ref`+`Dyn` = par tag+payload** en el JIT; sin gate.
- **SSA portable sin ids internos**; el JIT consume SSA, no re-deriva tipos.
- **Homes autoritativas** en cuerpos frame-aware (modelo del intérprete); leaf
  solo en registros CLIF.

---

## 4. Obsoleto / desactualizado (limpiar, no imitar)

- `AUDIT_RESPONSE.md` balas 9/§I-2 "JIT solo desde bytecode": **parcialmente
  resuelto** — el JIT ya baja de SSA para el subconjunto F1–F4. Sigue leyendo
  `register_meta` y bytecode como fallback.
- `FRAME_LAYOUT_V2_JIT_BAIL`/`PAIR_MIGRATION_PENDING`: false (JIT activo).
- `VARN_JIT_ALLOW_REF`: eliminado.
- Comentarios NaN-box legacy (`vm_value.rs`, `object.rs`, `ctx_json.rs`): texto
  obsoleto, limpiar al tocar.
- K5 "escritura angosta pendiente": sigue **fuera de alcance** (migración a
  `Boxed` en mismatch); no es regresión.
- Docs vs código (prioridad al código): `VM_ARCHITECTURE.md` (nursery 4096 vs
  `NURSERY_CAPACITY`; `ObjData` contiguo vs `shape:Rc+values+overflow`),
  `RUNTIME_ARCHITECTURE.md` (estado async en `ObjData` vs `InstanceData`),
  `ARCHITECTURE.md` (omite `Inline/Ext/Slice`).

---

## 5. Pendiente real

### 5.1 F5 — resto (llamadas a método/nativa, closures, `Try`, OSR)

Cada uno con su puerta. **Bloqueo común**: los helpers de llamada leen la
ventana de args de homes **contiguas** (`arg_start..`); las values SSA viven en
homes arbitrarias. Salidas (elegir una):
- **(A)** reservar la región de staging en `regalloc_post` (reusa los helpers;
  toca el compilador).
- **(B)** extraer la resolución de método del VM y añadir
  `jit_call_method_window` (toca el VM; sin duplicar dispatch, alineado con C2).
  **Recomendada.**
- `CallMethod`/`InvokeVirtual`/`CallNativeOp`/`Intrinsic`: sobre (B). Es lo que
  **ejercita las clases ya cableadas** (hoy definen/usarían una clase → declinan
  por `new`/la llamada).
- `MakeClosure`: el helper es **ip-coupled** (lee descriptores del bytecode);
  hace falta uno ip-free `(proto_idx, descriptors)` o ventana.
- `Try`/`Throw`: landing pads + resume interpretado.
- OSR sobre SSA: mapa `ip`↔bloque (hoy OSR solo por bytecode).

### 5.2 F6 — borrado final

Solo cuando F5 esté verde en los 4 cuadrantes y sin regresión de benchmarks:
- Borrar `clif/kinds.rs`, `state`, `box_or_load_home` heurístico y el lowering
  desde bytecode (`clif/body/*`); `from_ssa` pasa a ser la única bajada.
- Re-medir `compare.ps1`.

### 5.3 Pendientes del audit (menores)

- `Nullable` como par (valor, bit) — hoy `Dynamic`.
- `u64`: sin aritmética sin signo; no es tipo de superficie en checker/parser.
- regalloc: spill real en vez de rechazo `>256`.
- `ArrayRepr` angosto **escritura** compacta.
- Limpiar comentarios NaN-box legacy.
- Regenerar `AUDIT_RESPONSE.md` con el estado real.

---

## 6. Puerta de validación (cada commit)

1. `cargo check --workspace --exclude varn-lsp --all-targets` sin warnings.
2. `cargo build -p varn-jit -p varn-vm`.
3. `tests/main.vn` verde en JIT y `VARN_NO_JIT=1` con `VARN_CACHE_DIR` limpio
   (round-trip de serialización). Tier-parity `56/58/62/65/101`.
4. Para cambios de runtime: `compare.ps1` sin regresión.
5. Matriz release (4 cuadrantes std `dev-checkout`/`@embedded` × JIT/NO_JIT)
   antes de declarar terminado; `scripts/verify.ps1 -Fast`.

Criterio: `PASSED: N`, `FAILED: 0`, `ALL TESTS PASSED` en las 4 combinaciones.
Nota: el bundle stdlib se compila en `varn-cli/build.rs`; un error de tipos en
`std/` rompe el build. La suite es sensible al estado del caché; si un fallo no
se reproduce con caché limpio, es de la frontera de módulo (Ley 2).

---

## 7. Riesgos

- **Roots GC**: el modelo de homes/stack-map es el del bytecode; no inventar uno
  nuevo. Un `Ref`/`Dyn` fuera de su home y vivo a través de un alloc es el bug
  clásico.
- **ABI frame-aware**: `raw_signature` mezcla escalares nativos con
  `base/closure/exec_ctx`; wrapper y call site deben coincidir (una divergencia
  es un crash, no un fallback).
- **`regalloc_post`**: re-permuta `register_meta`/bytecode/`SsaProto.regs`; un
  nuevo usuario de registros debe seguir la misma permutación.
- **Constantes y epoch**: `LoadConst` bakea handles del heap por contexto
  (`jit_epoch`); el SSA no cambia esa regla.
- **`Call` directo**: solo si el callee es leaf y args/return escalares; si no,
  el fallback `invoke` es la única ruta.

---

## 8. Documentos y estado

- **`docs/AUDIT_RESPONSE.md`** — auditoría arquitectónica (evidencia por
  archivo, §A–§L, Anexos K1–K5). Fuente de los "por qué" de representación.
- Este plan absorbió y **borró**: `docs/plans/2026-09-20-jit-modelo-tipado-unico.md`,
  `docs/plans/2026-09-19-jit-fase-b-frame-clases.md`,
  `docs/plans/2026-09-20-jit-from-ssa-frame-aware.md`.
- También **borrados** (hechos o con otra dirección tomada):
  - `docs/TIR_PLAN_ETAPA_0_1.md`, `docs/TIR_ETAPA_2_PLAN.md`,
    `docs/TIR_ETAPA_3_PLAN.md`, `docs/TIR_ETAPA_4_GLOBALS.md` — **HECHO**.
    El TIR existe (`varn-tir`: `BackendTy`, `Resolution`, nodos, tablas,
    verificador, cobertura); el checker lo emite; el corte se hizo
    (HIR borrado); los globales son índices directos por región de módulo;
    `SlotKind` es el discriminante físico.
  - `docs/PLAN_MAPAS_ESTATICOS.md` — **HECHO**. `Map` es shapeless
    (`Value::Map(MapRef)`); opcodes dedicados `BuildMap`/`MapGetIndex`/
    `MapSetIndex` (+ `ArrayGetIndex`/`ArraySetIndex`); el JIT los baja por
    fast paths; `from_ssa` los cubre.
  - `docs/PLAN_ALOCACION.md` — **otra dirección**. La Fase 0 (atribución con
    `alloc_profile`) se hizo y demostró que los tramos no son aditivos; las
    Fases 1–2 (inline/arena) no se hicieron. En su lugar: layout compacto K3,
    constructores triviales inline, helpers de ventana. La arena bump queda
    descartada como apuesta no medida.
  - `docs/PERFORMANCE_ROADMAP.md` — **desactualizado**. Cifras y bails de otra
    época (`DefineGlobalIdx`/`LoadModule` como bail son historia); las arenas
    planas nunca se persiguieron.
  - `docs/notes/REFACTOR.md` — **desactualizado** (2026-07-13; nombra crates que
    no existen y NaN-boxing universal). Sus ideas vivas (`SlotKind`,
    intrínsecos, `LoadStaticFn`, JIT tipado) se hicieron por otras vías.
- Lo que queda en `docs/` son specs y arquitectura, no planes: `TIR_CONTRATO_TIPADO.md`
  (contrato vigente), `COMPILER_ARCHITECTURE.md`, `RUNTIME_ARCHITECTURE.md`,
  `ARCHITECTURE.md`, `AUDIT_RESPONSE.md` (auditoría), `DISENO_IDEAL.md` y
  `HIPOTESIS_DESCARTADAS.md` (diseño e hipótesis, con valor histórico).
