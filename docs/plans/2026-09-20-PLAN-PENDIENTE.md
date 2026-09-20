# Varn — Plan pendiente (JIT nativo, motor único)

Fecha: 2026-09-20. Estado: `main` consolidado con **trabajo WIP mezclado
(roto)** a propósito, para reiniciar con contexto limpio y sin ramas.

> **Progreso (misma fecha, sesión de implementación):** B0 + B1/C1 cerrados.
> `main` compila, `cargo check` limpio, `tests/main.vn` **verde en ambas
> tiers** (`PASSED: 1223, FAILED: 0`) con `Ref`+`Dyn` activos y el gate
> `VARN_JIT_ALLOW_REF` **eliminado**. Ver §7 para los defects concretos
> corregidos.

Prioridad de verdad: código > tests > comportamiento > docs.

---

## 0. Estado real de `main`

**Funciona (verde antes del merge `jit-pair-ref`):**
- Fase B del frame por clases: JIT reactivado (`FRAME_LAYOUT_V2_JIT_BAIL=false`),
  subset gate, backstop de helpers deshabilitados.
- **Homes inline** (`home_addr` = `class_vec_ptr + (base[class]+idx)*elem`),
  sin FFI por acceso a home.
- Llamada directa compiled→compiled (`jit_prepare_static_call`/
  `jit_finish_static_call` sobre `FrameStore`).
- Fix de layout compacto: campos de instancia y `Get/SetProperty` por helper
  compact-aware; loop-cache de objeto desactivado.
- Excepciones, `Intrinsic`, `CallNativeOp`, `CallMethod`, `InvokeVirtual`,
  `MakeClosure`, `CallSelf`.
- Limpieza de dead code / warnings rustc.
- `Ref` **gated** por defecto (escape dev `VARN_JIT_ALLOW_REF`).

**Roto ahora (por el merge del par):**
- La rama `jit-pair-ref` (par `Ref`/`Dyn` tag+payload) se mergeó **sin estar
  verde**: markdown falla (`expected str, got heap[46]`), y el intento de C4
  (neutralizar la lattice) se revirtió, así que ese frente no está en `main`.
- `main` compila? VERIFICAR primero; el merge tomó la versión del par en los
  ficheros de `varn-jit`, con las trazas y cambios de `safepoints`/`vars`/
  `op_dispatch`/`emit`/`lower` del par.

**Ramas:** solo quedan ajenas (`tir-stage-3-the-cut`, `worktree-*`). Las de esta
sesión (`jit-fase-b`, `jit-typed-model`, `jit-pair-ref`) ya no existen.

**Trazas disponibles (env-gated):** `VARN_HOME_TRACE` →
`HOME`, `DEFRESULT`, `OPIP`, `PREPCALL`, `INVOKE_NATIVE`, `METHODNATIVE`,
`NATIVEOP`, `INTRINSIC`, `STR BAD`. `VARN_JIT_ALLOW_REF` para bajar `Ref`.

---

## 1. Objetivo

**Máxima superficie en código nativo, una sola ruta por hecho, sin fronteras que
se crucen repetidamente.** Intérprete y CLIF como dos bajadas hermanas del
**mismo SSA tipado**. Nada interno de una fase/ruta cruza a otra (nombres y
sintaxis + clase física, no ids ni tags ambiguos).

Medible: `tests/benchmarks/compare.ps1` — hoy Varn gana 1/12 (arranque + csv_etl).
Meta: ganar a Bun en todo. `matrix` ya se midió **12 ms vs Bun 15.4** en la rama
par (aunque luego falla markdown).

---

## 2. Modelo objetivo (diseño A)

```
TIR (BackendTy por valor, obligatorio)
  → SSA tipada (una)
  → intérprete (FrameStore)  y  CLIF   ← MISMO contrato
```
- Registro: escalar `i64`/`f64` para `Gpr`/`Fpr`; **par tag+payload** para todo
  heap (`Ref` y `Dyn`). Una representación.
- Llamada: una convención (slots por clase); el intérprete y el JIT comparten
  materialización y copia de args.
- Nativas/intrínsecos: **una** tabla ABI + **un** marshal; `this` por el mismo
  camino que los args.
- JIT sin lattice de tipos: el tipo viaja en el IR.

Detalle: `docs/plans/2026-09-20-jit-modelo-tipado-unico.md`.

---

## 3. Pendiente, por bloques (orden de ejecución)

### B0 — Sanear `main` (primero)
- Verificar `cargo build -p varn-jit -p varn-vm` y `cargo check --workspace
  --exclude varn-lsp --all-targets` (0 errores; el merge pudo dejar usos rotos).
- Correr `tests/main.vn` **con `Ref` gated**; si falla por el par mergeado,
  decidir: (a) completar C1, o (b) volver `vars.rs`/`safepoints` a payload-only
  (revertir el par) hasta cerrar C1. Objetivo: `main` verde con `Ref` gated o
  con el par completo — no a medias.

### B1 — C1: par de valor para todo heap (`Ref`+`Dyn`)
Sitios que aún asumen "Dyn/Ref = una palabra (payload)" y hay que migrar a par
(I128). Cada uno: leer con `box_or_load_home` (par), escribir con `def_result`
(o `box_*` + `def_var` I128), y actualizar el home.
- `clif/body/op_dispatch.rs`: aritmética (`AddInt`/`SubInt`/`MulInt`/`ModInt`/
  `Negate`/bitwise/shifts) cuando el dest es clase `Dyn`; `AddImm`/`SubImm`;
  `CallSelf` ramas de dest; `Return`.
- `clif/generic.rs`: `def_boxed`/`box_operand` con par; comparaciones.
- `clif/emit.rs`: `use_int`/`use_f64` sobre registro par (extraer payload);
  `def_const*`; `box_or_pass`.
- `clif/floats.rs`, `clif/strings.rs`, `clif/strconcat.rs`, `clif/methods.rs`,
  `clif/arrays.rs`, `clif/globals.rs`, `clif/fields.rs`, `clif/alloc/*`:
  dest/lectura por clase.
- **Bug activo**: en `tests/36`/markdown, `str.split` recibe un **Instance**
  (`heap[46]`) como `this` (native `split` en el punto único `invoke_native`).
  Un registro `Dynamic` (`reg1` de `parse`, `source`) se **clobbea** antes del
  `split`. El intérprete funciona → es value-flow del JIT.
  - Hipótesis: `Move`/`def_result` con par escribe/lee el registro equivocado, o
    liveness/regalloc reusa un registro vivo.
  - Traza: `OPIP`/`PREPCALL`/`INVOKE_NATIVE` con `VARN_HOME_TRACE`.
- Al cerrar: quitar `VARN_JIT_ALLOW_REF` y el baile de `Ref` en `clif/lower.rs`
  (`uses_disabled_opcode`/gate). Suite verde con `Ref`+`Dyn` activos.

### B2 — C2: una convención de llamada (VM)
- Introducir `ExecCtx::invoke(callee, args_window)` y hacer que:
  - el intérprete (`exec_call_reg`, `exec_call_method_reg`) y
  - el JIT (`clif_call_fallback` → `call_vm_window`) y
  - las nativas (staging `[this, args...]`)
  usen esa **misma** materialización.
- **Borrar**: `clif_call_fallback`/`call_vm_window` como caminos distintos;
  `jit_call_native_fast`; ventanas contiguas duplicadas.
- Archivos: `exec/jit_helpers/calls.rs`, `exec/calls.rs`,
  `exec/dispatch/reg_ops/{calls,method_calls}.rs`, `clif/alloc/calls.rs`.

### B3 — C3: un ABI nativo y un marshal
- `CallNativeOp`, métodos nativos y `CallMethod`-nativo entran por una función
  con la **misma** extracción de `[this, args...]` y **un** `FromVm`.
- **Borrar**: `str_*_intrinsic` dedicados como ruta propia; `jit_call_native_op`
  vs `fnptr` duplicados; el marshal del `this` por camino aparte.
- Archivos: `exec/jit_helpers/{natives,intrinsics}.rs`,
  `exec/intrinsics/str.rs`, `varn-types/src/marshal.rs`, `clif/body/*`,
  `clif/alloc/native.rs`, `clif/methods.rs`.

### B4 — C4: bajar el JIT de SSA/TIR (el grande)
- Consumir SSA/TIR tipado (o serializar **clase/tipo** en `.vnc`, no
  bytecode+lattice).
- **Borrar**: `clif/kinds.rs` (lattice `K`), `state`/`box_for_target`,
  `box_or_load_home` heurístico, re-derivación por op.
- Sub-paso ya intentado y **revertido**: neutralizar la lattice (`state` =
  proyección de clase) — rompe N sitios de B1 aún sin migrar. Hacer B1 primero.
- Ver `docs/plans/2026-09-20-jit-modelo-tipado-unico.md` (C4).

### B5 — C5: `Ref`+`Dyn` globales y flip
- Quitar `VARN_JIT_ALLOW_REF` y el gate de `Ref`.
- Re-correr `compare.ps1` y la matriz de validación.

### B6 — Cobertura de opcodes aún gated / nativas
- `CallSpread`, `MakeClass`/clases, `InvokeRuntimeStatic`: implementar nativo.
- generator/async: suspensión/reanudación por clases.
- `Intrinsic`/`CallNativeOp` calientes (str/array): inline en CLIF o vía el ABI
  único (B3), sin rutas dedicadas.

### B7 — `str_ops` (benchmark 33x más lento)
- Especializar concat/repeat/slice/indexOf/replaceAll (hoy ruta genérica).
- Medir con `compare.ps1 -Only str_ops`.

### B8 — Pendientes del audit (menores, documentados)
- `u64`: aritmética sin signo (`BackendTy::UInt64`); hoy `TypeTag::U64`→`Int`.
- `ArrayRepr` angosto **escritura** (`set_vm`/`push_vm` migran a `Boxed`).
- Limpiar comentarios NaN-box legacy (`vm_value.rs:475`, `object.rs`, etc.).
- Regenerar `docs/AUDIT_RESPONSE.md` con el estado real.

---

## 4. Validación (cada commit)

1. Compilar primero: `cargo build -p varn-jit -p varn-vm`, `cargo check
   --workspace --exclude varn-lsp --all-targets` (0 warnings).
2. Ejecutar el `.exe` **directo** con timeout (no `cargo run` encadenado):
   `.\target\debug\vn.exe run .\tests\main.vn` (da PASSED/FAILED).
3. Tier-parity: `56-tier-parity`, `58-clif-range`, `62-jit-osr`,
   `65-safepoint-roots`, `101-self-recursion-return-kind`.
4. Benchmarks: `tests/benchmarks/compare.ps1 -Only matrix,str_ops,...`.
5. Matriz release (4 cuadrantes std/JIT) antes de declarar terminado.

---

## 5. Ramas

- **`main`**: único destino. Trabajar aquí o en **una** rama, nunca varias.
- Ajenas existentes: `tir-stage-3-the-cut`, `worktree-*` (atadas a worktrees;
  no tocar sin dueño).

---

## 6. Riesgos

- B1 y B4 se tocan: el par de valor (B1) es prerrequisito de neutralizar la
  lattice (B4). No hacer B4 antes de B1.
- B2/B3 tocan el intérprete: mantener `VARN_NO_JIT=1` verde.
- El crash de markdown (`str.split` con `this` Instance) es el bug que bloquea
  `Ref` global; localizado con `INVOKE_NATIVE` (`VARN_HOME_TRACE`).
- `main` está roto por el merge del par; B0 lo resuelve (completar B1 o revertir
  el par).

---

## 7. Progreso B0+B1/C1 (cerrado)

Rama de trabajo: local (sin commits nuevos; cambios en el árbol).

Defects concretos corregidos, todos consecuencia de que un valor de KIND
`Bool`/`Int` puede vivir en un registro de clase física `Dyn`/`Ref`
(`I128` par) y los consumidores lo leían como una sola palabra:

1. `clif/body/mod.rs` — `JumpIfFalse`/`JumpIfTrue`: una condición `K::Bool`
   con clase `Dyn` es un par; `brif` sobre el par es siempre verdadero (bucle
   infinito en `tests/09`). Ahora extrae el payload (`isplit`).
2. `clif/floats.rs` — comparaciones `*Float` y `Intrinsic*` math:
   el resultado bool se escribe con `box_bool` + `def_result`/`def_boxed_leaf`
   cuando el destino es heap-classed, en vez de un `I64` crudo.
3. `clif/emit.rs` — helpers nuevos `def_boxed_leaf`, `def_int_result`,
   `def_bool_result`: una sola proyección destino-clase para resultados
   crudos (Ley 6).
4. `clif/body/op_dispatch.rs` — aritmética (`AddInt`/`SubInt`/`MulInt`,
   `AddImm`/`SubImm` en su fast-path de inducción, `ModInt`, `Negate`),
   antes escribían `I64` crudo en el par.
5. `clif/generic.rs` — `def_boxed`/comparaciones/`IsNull`/unario bool en la
   ruta leaf.
6. `clif/fields.rs` — `GetFixedField` ruta `narrow` (`Int`/`Bool`/`Float`):
   `Bool` es `Dyn` (par) y debe boxearse.
7. `clif/alloc/safepoints.rs` — **bug de layout**: `store_boxed_home` para la
   clase `Ref` escribía un `I64` (8 bytes) en un slot `u32` (4 bytes), pisando
   el slot vecino. Causaba `arr=heap[0]` en `ArrayPush` (`tests/42`). Ahora
   `istore32`.
8. `clif/lower.rs` — gate de `Ref` y `VARN_JIT_ALLOW_REF` eliminados; el
   `eprintln!("REFMETA …")` de diagnóstico también.

Pendiente de este bloque: nada. B2/B3 siguen sin tocar.

---

## 8. Progreso B2/C2 (cerrado)

Commits: `61e2f466` (C2).

- **Una invocación**: `ExecCtx::invoke(callee, window)` es la única ruta
  run-to-completion. `clif_call_fallback` (JIT), `NativeCtx::call_vm` (host),
  `spawn_internal` e isolates pasan por ella; `call_vm_window` eliminada y
  `call_vm` pasa a construir la ventana `[callee, args...]`.
- **Una materialización de frame tipada**: `ExecCtx::push_call_frame`
  (push de activación + `mov_cross` por clase). La usan el intérprete
  (`exec_call_reg`, `exec_call_self`) y el JIT (`jit_prepare_static_call`),
  en vez de repetir el bucle de copia.
- **Borrado**: `jit_call_native_fast` (tripwire fase-A sin llamador en el
  lowering) — fuera de la lista ABI, de la tabla y del cuerpo.

`tests/main.vn`: 1223/0 en JIT y `VARN_NO_JIT=1`.

- **Forma método**: `ExecCtx::push_call_frame_with_this` (r0 = `this` + args
  tipados en r1..) para los caminos con receiver: el fast-path bound-method de
  `exec_call_reg` y `invoke_vm_method_fast` (rama sin rest). Antes repetían el
  push + `mov_cross`; el de `exec_call_reg` además fugaba la activación si el
  receiver fallaba (usaba `?` sin `pop_frame`) — corregido por el helper.
- Las ramas con `has_rest` mantienen su empaquetado del array rest, pero
  sobre el mismo `push_frame`/`mov_cross`.

---

## 9. Progreso B3/C3

Commit de C3 (esta sesión).

- **Un ABI nativo**: `jit_call_native_op` y `jit_call_native_fnptr` (dos
  entradas ABI, dos cuerpos) se colapsan en `jit_call_native(ctx, fn_addr,
  op_id, act_id, reg_start, total)`. `fn_addr == 0` resuelve por `op_id` en
  runtime; si no, ya viene resuelto en compilación. Una entrada en
  `helper_abi`, un cuerpo, una extracción (`call_native_from_homes`).
- **Un marshal**: `CallNativeOp` (JIT) boxea `[receiver, args...]` desde los
  homes y llama `ExecCtx::invoke_native`; el intérprete hace lo mismo vía
  `call_native_with_receiver` (que construye `[this, args...]` y llama
  `invoke_native`). `this` viaja por el mismo camino que los args; `FromVm`
  es el único unmarshal de parámetros.
- **`str_*_intrinsic`**: siguen, pero solo como **inline CLIF** que comparte
  la extracción (forma explícitamente permitida por el plan): `clif/strings.rs`,
  `clif/methods.rs` y el fast-path de `emit_call_native_op` las invocan con
  tag/payload ya extraídos; no son una ruta de dispatch paralela ni pasan por
  su propio marshal.
- **`FromVm`**: un solo trait en `varn-types/src/marshal.rs`; ningún call site
  nativo tiene unmarshal propio.

`tests/main.vn`: 1223/0 en JIT y `VARN_NO_JIT=1`.

---

## 10. Progreso B4/C4 (núcleo semántico)

Cambio (esta sesión): la lattice de flujo deja de re-derivar el tipo por op.

- **`state` = proyección de clase**: `kinds::apply_kinds` ya no clasifica por
  opcode; el kind de un registro es su `SlotClass` (`Gpr→Int`, `Fpr→Float`,
  `Ref/Dyn→Boxed`). `kind_flow` siembra cada bloque con esa proyección, así que
  el fixpoint converge trivialmente. La única verdad flow-proven que se
  conserva es el **origen de un global** (`K::Global`), que `Call` usa para
  pedir un target estático al linker — es procedencia del valor, no su
  representación.
- **Borrado**: `apply_kinds_flow` (la clasificación por op, ~230 líneas),
  `box_for_target` (reconciliación de representación en merges, ya identidad)
  y sus 5 call sites.
- **Evidencia de no-regresión**: cobertura JIT de `tests/main.vn` idéntica
  (antes `clif=1122 bail=1031`; después `clif=1123 bail=1028`). Suite 1223/0
  en ambas tiers.

Lo que **no** se hizo (para C4 al 100% según el plan):
- `clif/kinds.rs` sigue existiendo como tipo `K` + `kind_flow`; borrarlo del
  todo exige reemplazar `state: &[K]` por consultas de clase en ~30 archivos.
- La bajada sigue siendo desde bytecode: `varn-jit` no ve SSA/TIR. Bajar de
  TIR/SSA exige extender el contrato serializado (`.vnc`) con el tipo por
  punto, o exponer TIR al backend; es un cambio de formato (Ley 10: declarar
  ganancia/verificación/borrado antes de hacerlo).

---

## 11. Progreso B6/B7 (esta sesión)

- **Fix de cobertura (bug de C1)**: `istore32` con valor `I32` fallaba el
  verifier de Cranelift y tiraba ~600 funciones al intérprete en silencio.
  Ahora `bail` de `tests/main.vn` bajó de 1031 a 416 (clif=1123). Commit
  `848dfd87`.
- **B7 `charCodeAt`**: llega como `CallNativeOp` (tabla de métodos core), no
  como `Intrinsic`, así que el scan de regiones no lo reconocía, lo marcaba
  como alloc y hacía una llamada nativa por iteración. Ahora se reconoce por
  op-id (`is_str_char_index_op_id`), se registra como string site y se inlinea
  con la misma vista de bytes hoistada. `bench_str_ops char_code`: **316 → 7 ms**
  (checksum idéntico). Commit `2dfbdc9e`.
- **B7 `str_concat`**: se quita el flush/reload muerto (solo asigna; no GC ni
  reentrada). Commit `2257984d`.

Pendiente B7: `slice`/`substring` hoistado (vista de bytes + helper que
asigna), `int_to_str`, `prefix_suffix` (receptor no loop-invariant, coste por
llamada). Medir con `compare.ps1 -Only str_ops` (el wall-time del harness está
dominado por carga de módulos; usar las `ms` internas por sección).

---

## 12. Medición release (2026-09-20)

`cargo build --release -p varn-cli` + 4 cuadrantes verdes (1223/0 cada uno).
`cargo xtask compare` (harness release):

| benchmark | Varn | Bun | ratio |
|---|---|---|---|
| matrix | 27.9 ms | 14.0 ms | 2.00x |
| str_ops | 311.1 ms | 112.1 ms | 3.33x |
| json_native | 42.9 ms | 35.5 ms | 1.20x |
| json_pure | 412.6 ms | 349.1 ms | 1.18x |
| csv_pipeline | 143.5 ms | 111.1 ms | 1.30x |
| csv_etl | 28.5 ms | 29.7 ms | ~tied |
| json_api_payloads | 44.6 ms | 25.5 ms | 1.75x |
| gc_alloc | 150.8 ms | 48.9 ms | 3.12x |
| dto | 234.8 ms | 23.8 ms | 10.0x |
| collection_pipeline | 312.6 ms | 30.5 ms | 10.0x |
| fib | 5841.0 ms | 60.2 ms | **100x** |
| http_routing | 3173.7 ms | 142.1 ms | **25x** |

Scoreboard: 0 wins, 1 tied, 11 rivals. Arranque 2.9x más rápido que Bun.

**Outliers y causa raíz (no arreglados):**
- `fib`: es recursivo pero queda **frame-aware** (`has_boxed_slots`: el bool
  de `n<=1` es `SlotClass::Dyn`, y el slot de callee del `CallSelf` es
  `LoadNull` Dynamic). Al ser frame-aware, cada recursión pasa por
  `emit_call_self` (push de frame + `run_until` en Rust) en vez de una llamada
  directa hardware. Arreglarlo requiere compilar fib como leaf: rama directa
  sobre el payload de un bool de clase `Dyn` (hoy `JumpIfFalse` usa
  `emit_truthy_fast`, que necesita `exec_ctx`), y refinar `has_boxed_slots`.
- `http_routing`/`collection_pipeline`/`dto`: probablemente clases/generadores
  gated o closure-heavy; sin investigar.

---

## 13. `fib` arreglado; raíz de los outliers top-level

Commit `bf062962`: `has_boxed_slots` mira solo la FIRMA; un local boxed no
fuerza frame; `JumpIfFalse` sobre un bool estático rama por payload; `Move` y
`CallSelf` leaf escriben por clase. `fib` pasa de frame-aware a **leaf**:

| | antes | después |
|---|---|---|
| `fib` (release) | 5841 ms (100x) | **89.2 ms (~tied** con Bun 85.8) |

Suite 1223/0 en JIT y `VARN_NO_JIT=1`; cobertura JIT igual (clif=1123,
bail=416).

**Causa raíz del resto de outliers (no arreglada):** `varn-checker/src/emit/
mod.rs:278` marca el proto `<module>` con `is_async: true` **siempre** ("Module
top level permits top-level `await`"). `try_compile` rechaza async en fase B,
así que **todo el código top-level de todo módulo se interpreta**. Los
benchmarks cuyo trabajo pesado está en el top-level (`collection_pipeline`
9.09x, `dto` 8.33x, `http_routing` 20x) quedan bloqueados por eso. Arreglarlo
= JIT de async/generadores (B6, suspensión/reanudación), o marcar el módulo
`is_async` solo si de verdad usa `await` (cambio de semántica de top-level
await a validar).

---

## 14. Módulo JIT: async condicional + `LoadStaticFn` (parcial, con límite medido)

Commits `6b3cda7c` (checker) y `9f5f424e` (JIT/VM):

- El proto `<module>` es `is_async` **solo si el top-level tiene `await`**
  (`FnEmitter::saw_await`). Los módulos sin await son síncronos y JIT-ables.
- `jit_load_static_fn` implementado (espejo del brazo del intérprete) y
  `LoadStaticFn` fuera del gate.

Efecto (release, wall-time):
| benchmark | NO_JIT | JIT |
|---|---|---|
| matrix | 235 ms | **63 ms** |
| str_ops | 738 ms | **315 ms** |
| fib | 4634 ms | **84 ms** |

Cobertura JIT `main.vn`: bail 416 → 306. Suite 1223/0.

**Límite honesto (desbloqueo de clases revertido).** Quité del gate
`MakeClass`/`Method`/`DeclareField`/`Inherit`/etc. (el lowering ya existía en
`clif/classes.rs`). El módulo pasó a JIT, pero **fue una regresión medible**:

| benchmark (release) | intérprete | módulo JIT |
|---|---|---|
| collection_pipeline | 188 ms | 382 ms |
| dto | 116 ms | 225 ms |

La causa es el coste del frame-aware (constructores `this+boxed`, accesos a
campo por helper con flush/reload): para este shape el intérprete gana. Ley 10
— sin ganancia medible no se rompe — así que el gate de clases se mantiene.
El camino correcto es abaratar el constructor/llamada (C2 real) antes de
abrirlo.

---

## 15. Llamada dinámica: flush solo de la ventana (ganancia medida)

Commit `a5ac33ca`: `emit_vm_call`/`emit_call_self` flusheaban **todos** los
homes antes de cada llamada dinámica. Ese flush completo solo hace falta si un
`Try` de este proto puede capturar debajo (entonces el frame compilado se
abandona y el intérprete lo reanuda leyendo homes). Sin `Try`
(`narrow_roots == true`) el frame se descarta entero y basta con que la
**ventana de args** esté en homes (el VM lee args de homes).

| (release) | antes | después |
|---|---|---|
| dto | 8.33x | **6.25x** |
| collection_pipeline | 9.09x | **7.69x** |

Sin regresión (fib ~tied, matrix 1.20x). Suite 1223/0 en JIT y `VARN_NO_JIT=1`.

**Siguiente cuello (medido, no atacado):** `emit_call` fuerza
`class_target = None` porque el plan de init trivial escribe campos a `slot*16`
(falso para `InstanceData` compacto), así que `new X()` **siempre** cruza a
Rust (`emit_vm_call`). Y todo acceso a campo de instancia (`GetFixedField`/IC)
va por helper porque el layout compacto (`FieldRepr`) no está inlineado. Es el
mismo bloque: hacer el lowering de campo/instancia compact-aware.

---

## 16. Scoreboard release (cierre de sesión)

| benchmark | Varn | Bun | ratio |
|---|---|---|---|
| fib | 87.2 ms | 72.7 ms | 1.20x |
| matrix | 33.0 ms | 16.1 ms | 2.04x |
| str_ops | 361.7 ms | 127.6 ms | 3.57x |
| json_native | 57.1 ms | 40.9 ms | 1.39x |
| json_pure | 507.6 ms | 433.4 ms | 1.18x |
| csv_pipeline | 163.1 ms | 130.5 ms | 1.25x |
| csv_etl | 24.9 ms | 24.9 ms | ~tied |
| json_api_payloads | 45.2 ms | 18.9 ms | 2.38x |
| gc_alloc | 176.4 ms | 61.8 ms | 2.86x |
| dto | 288.0 ms | 31.6 ms | 9.09x |
| collection_pipeline | 363.6 ms | 33.1 ms | 11.11x |
| http_routing | 3819.6 ms | 154.3 ms | 25x |

0 wins, 1 tied, 11 rivals; arranque 3.0x más rápido. (Los tiempos varían entre
corridas — dto midió 6.25x en una corrida enfocada; la máquina es ruidosa.)

Punto de partida del plan: `fib` 100x, `str_ops` 33x, `matrix` 33x (debug). Los
outliers restantes (`dto`, `collection_pipeline`, `http_routing`) comparten la
misma causa: **layout compacto de instancia no inlineado** (§15). Es el
siguiente proyecto, no un parche.

---

## 17. Constructor compacto inline — HECHO

Commit `626b4192`. `ClifClassTarget` lleva por campo su `(offset, size, tag,
is_gc_ref)` desde `layout.get_field_by_index(slot)` (`clif_link.rs`), y el fast
path de `emit_call` escribe cada campo compacto inline espejando
`InstanceData::write_field` (stores angostos por anchura, `F32` con `fdemote`,
int→float, ref compacta con `null → COMPACT_REF_UNINIT`, `str`/`char`/Dynamic
de 16B). `alloc_instance_fast` ya zero-fillea → paridad.

`bench dto` (release): **288 → 123 ms**. Cuatro cuadrantes release 1223/0.

**Principio (aplicado):** una clase NO tiene `Shape`; el layout es estático y
`slot` indexa `ClassLayout::fields` 1:1. Nada de IC/shape/helper para clases.

---

## 18. Acceso a campo de clase inline — HECHO

Commit `1de8615e`. Una clase NO tiene shape: el offset+tag del campo se
**bake-an** en `GetFixedField`/`SetFixedField` (4º word = offset compacto; byte
bajo de `w1` = `TypeTag`, `Null` = acceso por slot dinámico; `w2` = slot para
el fallback Object/Record). El JIT inlina load/store por tag; el intérprete
lee/escribe por offset; el helper por slot queda como fallback dinámico.

`bench dto` (release): **9.09x → 1.33x** (288 → 55 ms). collection_pipeline
11.11x → 7.14x. Test nuevo `tests/class_field_layout.vn` (herencia + anchos
mixtos + adyacencia) en `main.vn`. 4 cuadrantes 1233/0.

---

## 19. Clases JIT (gate abierto) — HECHO

Commit `f14c01fe`. Con campo y `new X()` ya inline-compactos (sin helper de
layout), el módulo class-heavy ya no pierde: se quitan `MakeClass`/`Inherit`/
`Method`/`Define*`/`DeclareField`/`BindMethod`/`GetSuper` del gate fase-B (su
lowering y helpers ya existían).

| (release, absoluto) | antes | después |
|---|---|---|
| collection_pipeline | 338 ms | **237 ms** |
| dto | 66 ms | **49 ms** |

4 cuadrantes 1233/0.

---

## 20. Dónde queda el tiempo (medido, no convención de llamada)

`VARN_NO_JIT` vs JIT (release, wall):

| benchmark | JIT | intérprete | causa |
|---|---|---|---|
| collection_pipeline | 204 ms | 192 ms | **allocator/GC** (100k objetos + `push`); el JIT no gana |
| http_routing | 1044 ms | 1456 ms | **mapas dinámicos + strings** (`params[k]=v` sobre `{[key:str]:str}`, split/slice/indexOf) |

Es decir: los outliers restantes NO son la convención de llamada. Son:
- **allocator/GC** (`gc_alloc` 2.86x, `collection_pipeline`): asignación de
  objetos/arrays; siguiente frente = allocator (bump nursery, layout, GC).
- **mapas dinámicos y strings** (`http_routing`, `str_ops` 3.85x):
  `Map`/objeto dinámico siguen en la ruta genérica; `slice`/`substring` sin
  hoist.

Hipótesis de convención de llamada/closure descartada por la medición.















